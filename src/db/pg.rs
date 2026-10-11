//! PostgreSQL 存储实现。
//!
//! 本 MVP **只有 PG 一个实现，刻意不写 trait**（范围规划 §3 暂缓项）。

use sqlx::postgres::PgPool;
use sqlx::Row;

/// 建表 DDL（幂等）。
///
/// 用 `raw_sql` 一次执行多语句 —— sqlx 的 `query` 不接受多条。
///
/// # REQ-013 + REQ-025：一次迁移 = **一个 batch**（锁 + 全部 DDL）
///
/// ## 为什么建表需要锁
///
/// `CREATE TABLE IF NOT EXISTS` 在 PG 里**不是并发安全的**：两个连接同时建同一张表时，
/// 两边都可能通过「表不存在」的检查，然后双双撞上
/// `duplicate key value violates unique constraint "pg_type_typname_nsp_index"`
/// —— `pg_type` 是 catalog，**catalog 的写不走事务快照**，所以 `IF NOT EXISTS` 拦不住。
///
/// 测试是并行跑的（几乎每个 `#[tokio::test]` 各调一次 `migrate()`），
/// 而空 schema 的并发首跑（首次部署、CI 干净库）最容易踩到它。
///
/// ## REQ-025 改了什么（关键）
///
/// 此前那把锁只包住第三次 `raw_sql`（`SCHEMA_AGENT_REGISTRY`），
/// 9 张基表（`SCHEMA`）与 2 条 ALTER（当时的 `SCHEMA_PATCH`；REQ-039 起拆成
/// [`ALTER_PATCHES`] 与 [`BACKFILL_PATCH`]，名字变了、这段记的仍是当时的事实）
/// 都跑在**锁之外**。
/// 实测三档对照（空 schema / 只建 registry / 9 张老表已存在，各 5 轮 × 8 并发）
/// **带锁与去锁的数字完全相同** —— 那把锁没有任何可测量的效果，
/// 真正会炸的 9 张基表根本不在它的覆盖面里。**锁加错了地方。**
///
/// 现在锁提到最前面，覆盖**全部** DDL，并把三段合成**同一个 batch**：
/// - 一处修复胜过三处补丁；
/// - 顺带少两次网络往返（3 条 `raw_sql(&PgPool)` → 1 条）。
///
/// ⚠️ **「同一个 batch」是硬要求，不是风格问题**：锁是**交易级**的
/// （`pg_advisory_xact_lock`），随那条 batch 的 `COMMIT` 自动释放。
/// 拆成三条 `raw_sql` 就是三个事务、各拿一次又立刻释放的锁 ——
/// 那等于没有锁，且会让「已加锁 ⇒ 并发安全」这句话重新变成假话。
///
/// 取交易级锁而不是会话级的三个理由：
/// - 随事务结束自动释放 —— DDL 失败也不会留下死锁；
/// - **不占着连接排队** —— 对比「`acquire()` 一条连接 + 会话级锁」的写法，
///   那种写法在几十个测试并行 `migrate()` 时会把峰值连接数推高；
/// - 不影响 `migrate()` 的 `Send`（见函数体内那条踩坑注释）。
///
/// ## 这把锁**不**许读成什么
///
/// 1. **不是迁移框架**：没有版本表、没有 down 迁移、没有变更历史；
///    它只是「同一批幂等 DDL 的一次性串行化」。
/// 2. **不覆盖不取这把锁的 DDL**：咨询锁只约束自觉取锁的代码。
///    本仓另一条路径（若有）自己建表，就既不被这里保护，也不被这里看见。
/// 3. **跨版本不互相排斥**：REQ-025 把锁键从 `hashtext('ontok::migrate::registry')`
///    改成 `hashtext('ontok::migrate')`（覆盖面变了，名字得跟上）。
///    滚动升级期间**旧版本进程取的是旧键**，两者不互斥 —— 单二进制部署，
///    且升级期恰好是「表都建好了」的稳态，实测风险为 0，但这是**已知边界**，不是遗漏。
///
/// 取锁是**无条件**的（无任何存在性查询，无「表已存在就跳过」分支）——
/// 每跑一次 `migrate()` 取一次锁，哪怕表早已存在。
/// 覆盖面的判据由 `tests/test_db.rs::test_migrate_on_empty_schema_survives_concurrent_first_run`
/// 钉住：锁被移回「只包 registry」时那个测试必须失败。
const MIGRATE_LOCK_PREFIX: &str = "
BEGIN;
SELECT pg_advisory_xact_lock(hashtext('ontok::migrate'), 1);
";

/// batch 的收尾 —— 与 [`MIGRATE_LOCK_PREFIX`] 成对，**缺一不可**。
const MIGRATE_COMMIT: &str = "COMMIT;
";

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    // REQ-025 · 一条 batch：锁 + SCHEMA（9 张基表）+ 该发的 ALTER + 回填 UPDATE
    // + SCHEMA_AGENT_REGISTRY（REQ-013 那两张表）。
    //
    // ⚠️ **不要**改成「`pool.acquire()` 拿住一条连接再取锁」那种写法：
    // sqlx 0.8 下 `&mut PgConnection` 的 `Executor<'c>` 是 HRTB，
    // 会让 `migrate()` 的 future **丢掉 `Send`**（HEAD 的 migrate 是 Send）。
    // 「占着池连接排队等锁」还会把峰值连接数推高 —— 本任务实测到
    // 两次别的测试 `Connection reset by peer`。交易级锁没有这两个问题。
    //
    // REQ-039 · **已存在的列不进 DDL**（见 [`patched_columns_present`]）：
    // 省掉的那几条 `ALTER` 不是省时间，是省**排他锁** ——
    // 这是共享测试库上 `40P01` 的唯一来源，去掉它才让全量测试可判读。
    //
    // ⚠️ 探查询在 batch **之外**、即**咨询锁之前**发出，于是同一个 batch 里
    // 拿不到同一把锁串行化。这是有意的取舍，且**不引入漏建**：
    // 查到的答案是「列已存在」，而列只会被建、不会被删（本仓无 DROP COLUMN），
    // 所以「查过时不存在」之后被别人建出来，只会让这条 ALTER 变成 no-op，
    // 反过来「查过时存在」也不可能被本仓的任何路径抹掉。
    // 真正的 fail-closed 在 [`patched_columns_present`] 里：探查询的**形状**
    // 对不上就报错拒绝迁移（而不是「假装列不存在」再照发 ALTER）。
    let present = patched_columns_present(pool).await?;

    // 容量按「全都要发」估算 —— 稳态下真实长度短得多，但多出来的容量
    // 只是没被用到的 Vec 内存，不是一次分配失败。
    let cap = SCHEMA.len() + SCHEMA_AGENT_REGISTRY_PATCH.len() + BACKFILL_PATCH.len() + 512;
    let mut batch = String::with_capacity(cap);
    batch.push_str(MIGRATE_LOCK_PREFIX);
    batch.push_str(SCHEMA);
    for ((_, _, ddl), present) in ALTER_PATCHES.iter().zip(present.iter()) {
        // ⚠️ 上一轮这里是 `present.get(i).copied().unwrap_or(false)`。
        // 那个 `unwrap_or(false)` 把「形状没看懂」伪装成「列不存在」，于是照发 ALTER ——
        // 而形状没看懂恰恰是**代码与库不同源**的证据（见 [`patched_columns_present`]）。
        // REQ-039 修复轮：`patched_columns_present` 已保证长度**恰好**相等
        // （对不上就报错），所以 `zip` 走完即全覆盖，不需要任何 fallback。
        if !present {
            batch.push('\n');
            batch.push_str(ddl);
        }
    }
    batch.push_str(BACKFILL_PATCH);
    batch.push_str(SCHEMA_AGENT_REGISTRY_PATCH);
    batch.push_str(MIGRATE_COMMIT);
    sqlx::raw_sql(batch.as_str()).execute(pool).await?;
    Ok(())
}

/// 一次性读出 [`ALTER_PATCHES`] 里每一列「是否已存在」。
///
/// # REQ-039 · 为什么必须先查、为什么不直接照发 `IF NOT EXISTS`
///
/// `ADD COLUMN IF NOT EXISTS` 里的 `IF NOT EXISTS` 只省掉「列不存在时报错」。
/// 它**不**省掉开事务时对目标表取的 `AccessExclusiveLock` —— 那把锁**持有到提交**。
/// 实测（`pg_locks`，同一事务内自查）：表已存在时
/// - `CREATE TABLE IF NOT EXISTS` 取到的锁：**空**（PG 先查存在性再决定要不要开锁）
/// - `ALTER TABLE … ADD COLUMN IF NOT EXISTS`（列已存在）：**`AccessExclusiveLock`**
///
/// 于是每个 `#[tokio::test]` 各跑一次 `migrate()`，就是每条测试一次排他锁，
/// 与同二进制里并行跑的 `INSERT`（外键检查取 `RowShareLock`）撞成锁环。
/// REQ-039 前在共享集群上实测（3 进程并发 × 5 轮 = 15 次运行，p50 ≈ 157 ms）：
/// **12 次报 `40P01`**。死锁报告里的两个 relation 正是
/// `ontology_versions`（等 `RowShareLock`）与 `action_types`（等 `AccessExclusiveLock`）。
///
/// 查一次就把那三条 ALTER 摘掉 ⇒ 稳态库上 `migrate()` **一把排他锁都不取**。
/// ⚠️ 修复轮实测：上一轮这句**并没有做到** —— 解码取错了列，
/// 只有第 1 条 ALTER 真的被摘掉，另两条照发。见下面「解码」一节。
///
/// # fail-closed 有**两种**，别混（修复轮补的区分）
///
/// **(a) 查不到列** ⇒ `EXISTS` 为 `false` ⇒ 当作「不存在」⇒ 照发 ALTER。
/// 用 `to_regclass()` 而不是 `'x'::regclass`：表不存在时前者返回 `NULL`、
/// 后者**直接抛错**。而「表不存在」正是**空 schema 首次迁移**的常态 ——
/// 那时必须照发 ALTER（尽管 `SCHEMA` 的 `CREATE TABLE` 本就把列建出来了，
/// ALTER 仍是幂等的 no-op），绝不能因为查不到就当成「已有」而跳过。
///
/// **(b) 看不懂结果的形状**（行数/列数与 `ALTER_PATCHES` 对不上）⇒ **报错**，不照发。
/// 见下面「解码」一节 —— 这一支上一轮是错的，它正是本轮缺陷能静默交付的原因。
///
/// 另两条判定是 `attnum > 0`（排掉系统列）与 `NOT attisdropped`（排掉被 DROP 掉的列）——
/// 后者尤其重要：被删过的列**名字还在 `pg_attribute` 里**，只按名字判会误判成「存在」。
///
/// # 单次往返
///
/// SQL 照 [`ALTER_PATCHES`] 生成，所以「查什么」和「改什么」永远同源；
/// 而三条 `EXISTS` 挤进一条语句，是因为共享集群上 `SELECT 1` 的 p50 就 ~160 ms，
/// 每轮全量测试要跑几十次 `migrate()`，往返数是这个函数的真实成本。
///
/// # 解码：**按列索引**取，不是按 1 元组取（REQ-039 修复轮 · 修一个真缺陷）
///
/// 上面那条 SQL 是 `SELECT EXISTS(…), EXISTS(…), EXISTS(…)` ——
/// 它返回的是**一行 N 列**，不是 N 行 1 列。
/// 而 `sqlx::query_as::<_, Vec<(bool,)>>` 对多列结果**只取第 0 列、且不校验列数**
/// （实测：raw `rows=1, cols=3`，而解码出来的 `Vec` 长度是 **1**）。
/// 于是 `present.get(i).copied().unwrap_or(false)` 对 i=1、2 一律判「不存在」
/// ⇒ `action_types` 与 `concept_groups` 的 ALTER **每次照发**，
/// 两把 `AccessExclusiveLock` 照取。上一轮那句「稳态库一把排他锁都不取」是**假的**。
///
/// 这里用 `row.try_get::<bool, _>(i)` **按列索引**取：N 变了几条 `ALTER_PATCHES`
/// 这段代码一个字都不用改，也不会再有「只认第 0 列」这种静默截断。
///
/// # 形状对不上 ⇒ **报错**，不是 fallback（本轮刻意改了上一轮的做法）
///
/// 上一轮写的是 `get(i).copied().unwrap_or(false)`，注释说
/// 「行数对不上也要倒向『补上』，而不是倒向『跳过』」。**方向对了，机制错了**：
/// `unwrap_or(false)` 把「探查询的形状我没看懂」**伪装成**「列不存在」，
/// 然后照发 ALTER —— 既不报错也不 warn。上面那个缺陷能一路走到交付、
/// 甚至在黑盒判据下全绿，靠的就是这个 fallback：它**主动吞掉**了唯一的症状。
///
/// fail-closed 的正解是「**看不懂就拒绝迁移**」，不是「假装没查到」。
/// 两种「看不懂」（行数不对 / 列数不对）都是**代码与库不同源**的证据，
/// 而 SQL 是照 [`ALTER_PATCHES`] 生成的 —— 形状对不上只可能是代码自己坏了。
/// 此时报错是正确的：迁移失败是一次响亮的、可定位的失败；
/// 照发 ALTER 则是一次安静的、把排他锁重新请回共享集群的失败。
///
/// 用 `sqlx::Error::Protocol` 承载，与本仓既有的「契约/形状被违反」写法一致
/// （见 `db::proposal.rs` 的 payload 校验）。选它而不是新加一个错误枚举：
/// `migrate()` 的返回类型是 `sqlx::Error`，全仓 90+ 处调用点因此**零改动**。
async fn patched_columns_present(pool: &PgPool) -> Result<Vec<bool>, sqlx::Error> {
    let sel: Vec<String> = ALTER_PATCHES
        .iter()
        .map(|(table, column, _)| {
            format!(
                "EXISTS (SELECT 1 FROM pg_attribute \
                 WHERE attrelid = to_regclass('{table}') AND attname = '{column}' \
                   AND attnum > 0 AND NOT attisdropped)"
            )
        })
        .collect();
    // 表名/列名都是本文件里的编译期常量，不是外部输入 —— 不存在注入面。
    let sql = format!("SELECT {}", sel.join(", "));
    let rows = sqlx::query(&sql).fetch_all(pool).await?;

    // 无 FROM 子句 ⇒ 恒为一行；列数恒等于 `ALTER_PATCHES` 的长度。
    // 两条都写出来断言，是因为「恒等」是本函数**自己**建立的不变量，
    // 不是数据库替我们保证的 —— 一旦有人改了 `sel` 的生成方式，这里要立刻响。
    if rows.len() != 1 {
        return Err(sqlx::Error::Protocol(format!(
            "REQ-039 探查询形状异常：期望 1 行（无 FROM 的 SELECT），实际 {} 行。\
             SQL 与 ALTER_PATCHES 已不同源 —— 拒绝迁移（照发 ALTER 会把排他锁请回共享集群）",
            rows.len()
        )));
    }
    let row = &rows[0];
    let want = ALTER_PATCHES.len();
    let got = row.columns().len();
    if got != want {
        return Err(sqlx::Error::Protocol(format!(
            "REQ-039 探查询形状异常：期望 {want} 列（= ALTER_PATCHES 长度），实际 {got} 列。\
             SQL 与 ALTER_PATCHES 已不同源 —— 拒绝迁移"
        )));
    }

    let mut out = Vec::with_capacity(want);
    for i in 0..want {
        // `try_get` 在列序号/类型对不上时**自己**会报错，这里不必再兜一层。
        out.push(row.try_get::<bool, _>(i)?);
    }
    Ok(out)
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("数据库错误：{0}")]
    Db(#[from] sqlx::Error),

    #[error("版本 {seq} 已冻结，不可写入（冻结是判据，规格 §9.1）")]
    VersionFrozen { seq: i32 },

    #[error("key 冲突：{0} 已存在于该版本")]
    KeyConflict(String),

    #[error("提案 {0} 不是 pending 状态（当前 {1}）")]
    ProposalNotPending(String, String),

    #[error("提案未通过审核，不能 apply（blocked {blocked} 条，悬空关系 {dangling} 条）")]
    ProposalNotPassed { blocked: usize, dangling: usize },

    #[error("提案基于的版本已被冻结")]
    BaseVersionFrozen,

    #[error("找不到类型：{0}")]
    TypeNotFound(String),

    #[error("提案没有可应用的条目")]
    NothingApplicable,

    /// REQ-010 · 概念组声明不合法。三条硬约束的理由见 `domain::concept`。
    #[error("概念组声明不合法：{0}")]
    BadConceptGroup(String),
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
  id          UUID PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ontology_versions (
  id            UUID PRIMARY KEY,
  project_id    UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  seq           INT  NOT NULL,
  frozen_at     TIMESTAMPTZ,                     -- ← NULL = 未冻结（三态之一）
  -- 抽取来源。**NULL = 人工修订**（ADR-001：apply 时原样取提案的值，
  -- 缺失记 NULL，不继承 base、不推断）
  extracted_by  JSONB,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (project_id, seq)
);

CREATE TABLE IF NOT EXISTS object_types (
  id             UUID PRIMARY KEY,
  version_id     UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key            TEXT NOT NULL,
  display_name   TEXT NOT NULL,
  description    TEXT,                            -- ← NULL 有意义（AC-010）
  aliases        JSONB NOT NULL DEFAULT '[]',    -- 占位（AC-003）
  parent_key     TEXT,                            -- 存父的 key 而非 id（Task 4）
  disjoint_with  JSONB NOT NULL DEFAULT '[]',     -- 占位（AC-003）
  properties     JSONB NOT NULL DEFAULT '[]',
  evidence       JSONB NOT NULL DEFAULT '[]',
  ordinal        INT  NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS link_types (
  id           UUID PRIMARY KEY,
  version_id   UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key          TEXT NOT NULL,
  from_type    TEXT NOT NULL,
  to_type      TEXT NOT NULL,
  cardinality  TEXT NOT NULL,
  semantic     TEXT NOT NULL,
  evidence     JSONB NOT NULL DEFAULT '[]',
  ordinal      INT  NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS action_types (
  id              UUID PRIMARY KEY,
  version_id      UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key             TEXT NOT NULL,
  target          TEXT NOT NULL,
  parameters      JSONB NOT NULL DEFAULT '[]',
  preconditions   JSONB NOT NULL DEFAULT '[]',
  effects         JSONB NOT NULL DEFAULT '[]',   -- WriteBinding 占位（AC-003）
  permission      JSONB NOT NULL DEFAULT '{}',
  audit_policy    JSONB NOT NULL DEFAULT '{}',
  evidence        JSONB NOT NULL DEFAULT '[]',
  ordinal         INT  NOT NULL DEFAULT 0,   -- REQ-011 · 写入序，读侧 ORDER BY 用
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS functions (
  id             UUID PRIMARY KEY,
  version_id     UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key            TEXT NOT NULL,
  signature      TEXT NOT NULL,
  allowed_types  JSONB NOT NULL DEFAULT '[]',
  evidence       JSONB NOT NULL DEFAULT '[]',
  ordinal        INT NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);

CREATE TABLE IF NOT EXISTS proposals (
  id               UUID PRIMARY KEY,
  project_id       UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  base_version_id  UUID NOT NULL REFERENCES ontology_versions(id),
  round            INT  NOT NULL,
  extracted_by     JSONB,                          -- ← NULL = 人工修订（ADR-001）
  status           TEXT NOT NULL DEFAULT 'pending',
  created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS proposal_items (
  id           UUID PRIMARY KEY,
  proposal_id  UUID NOT NULL REFERENCES proposals(id) ON DELETE CASCADE,
  seq          INT  NOT NULL,
  kind         TEXT NOT NULL,
  target_key   TEXT NOT NULL,
  payload      JSONB NOT NULL,
  biz_status   TEXT NOT NULL DEFAULT 'pending',
  model_status TEXT NOT NULL DEFAULT 'pending',
  UNIQUE (proposal_id, seq)
);

-- REQ-010 · 业务概念组（§7.1 阶段 3）
--
-- 【红线】这张表**存的是展示层声明**，不参与任何落库结构：
-- 它没有外键指向 object_types（成员存 JSONB 数组），所以
-- 「去掉分组声明」不需要动 object_types 一行。
-- 悬空成员由 `domain::concept::validate_groups` 在 apply 期拦（见该文件理由①）。
CREATE TABLE IF NOT EXISTS concept_groups (
  id            UUID PRIMARY KEY,
  version_id    UUID NOT NULL REFERENCES ontology_versions(id) ON DELETE CASCADE,
  key           TEXT NOT NULL,
  display_name  TEXT NOT NULL,
  members       JSONB NOT NULL DEFAULT '[]',
  scope_hint    TEXT,                           -- ← NULL = 还没写说明（三态之一）
  extracted_by  JSONB,                          -- ← NULL = 人工修订（ADR-001）
  low_confidence BOOLEAN,                       -- ← NULL = 没声明过（三态，REQ-028 · F4）
  evidence      JSONB NOT NULL DEFAULT '[]',
  ordinal       INT NOT NULL DEFAULT 0,
  UNIQUE (version_id, key)
);
"#;

/// 建表 DDL 的**幂等补丁**：三条 `ALTER`，逐条独立。
///
/// `CREATE TABLE IF NOT EXISTS` 不会给已存在的表补列 —— 本迭代中途
/// 给几张表加过列，旧库上必须靠 `ALTER` 补。两边都写 `IF NOT EXISTS`，
/// 重复跑安全。
///
/// # 为什么是「三条分开的一条」而不是一个大字符串（REQ-039）
///
/// 因为**能不能跳过是逐列决定的**：[`migrate`] 要照 [`patched_columns_present`]
/// 的结果把「已存在」的列从 batch 里摘掉，一大坨字符串没法逐条摘。
/// 元组里的 `(表名, 列名)` 同时就是探查询「查什么」的答案 ——
/// 「改什么」与「查什么」永远同源，加第 4 条只改这一张表。
const ALTER_PATCHES: [(&str, &str, &str); 3] = [
    // REQ-004 · 抽取来源（ADR-001：apply 时原样取提案的值，缺失记 NULL，
    // 不继承 base、不推断）。
    (
        "ontology_versions",
        "extracted_by",
        "ALTER TABLE ontology_versions ADD COLUMN IF NOT EXISTS extracted_by JSONB;",
    ),
    // REQ-011 · action_types 补 ordinal（其余四栏早就有）。
    // 不补的话 load_actions 无法 ORDER BY，返回顺序由优化器在
    // 「索引扫描(按 unique key 序)」与「顺序扫描(按插入序)」之间二选一，
    // 同一个库换个计划就会变 —— 读出来的顺序没有任何代码在管。
    (
        "action_types",
        "ordinal",
        "ALTER TABLE action_types ADD COLUMN IF NOT EXISTS ordinal INT NOT NULL DEFAULT 0;",
    ),
    // REQ-028 · F4 · concept_groups 补 low_confidence。
    //
    // **为什么必须落库，而不只是 struct 上加个字段**：`mcp-server` 的
    // `confirmation_of()` 读的是**快照里的组**（`mcp::serve` ←
    // `snapshot_store::load_concept_groups`），不是声明文件。只改 struct
    // 的话，这个字段在「提案 → 快照 → 读回」这一跳上**恒为 NULL**，
    // 于是一切低置信组在服务进程里都变成 `low_confidence: false` ——
    // **换了个更好看的壳，fail-open 一模一样**。
    //
    // 无 DEFAULT 且允许 NULL：既有行读出来是 NULL（「没声明过」），
    // 不往既有行回填 false —— 回填等于替业务侧做了一个从没做过的断言。
    (
        "concept_groups",
        "low_confidence",
        "ALTER TABLE concept_groups ADD COLUMN IF NOT EXISTS low_confidence BOOLEAN;",
    ),
];

/// REQ-012 · 回填历史 `action_types.ordinal`。**无条件**执行，从不跳过。
///
/// 它与上面三条 ALTER 的区别是**实质**的，不是风格：
///
/// - 三条 ALTER 取 `AccessExclusiveLock` —— 共享测试库上 `40P01` 的来源（REQ-039）；
/// - 这条 UPDATE 只取 `RowExclusiveLock`，且**不改 `version_id`** ⇒ 不触发外键检查
///   ⇒ **一把 `ontology_versions` 上的锁都不取**（实测 `pg_locks` 为空）。
///   它与别的测试的普通写入**同模式**，撞不出锁环。
///
/// 跳过的代价也更实：它正是 `test_backfill_…` 真正在测的那件事。
const BACKFILL_PATCH: &str = r#"
-- REQ-012 · 回填历史 action_types.ordinal。
--
-- 【问题】上面那条 ALTER 只**加列**，DEFAULT 0 让所有既有行并列 0。
-- 而读侧 `load_actions` 已经 `ORDER BY ordinal`（REQ-011）——
-- 读侧以为自己是稳的，历史数据却全给它同一个键：
-- 同版本多行的相对顺序退回由优化器的访问路径决定
-- （唯一索引扫描按 key 序 vs 顺序扫描按堆插入序），同一个库换个计划就变。
-- 实测：2161 行里 2149 行 ordinal = 0。
--
-- 【哪些行需要回填】同一 version_id 内 **ordinal = 0 且不止一行** 的那些。
-- 判定写在子查询的 `WHERE ordinal = 0` 上：
--   · 正常写入的版本，`enumerate()` 从 0 起 ⇒ ordinal = 0 至多**一行**，
--     这行单独重算仍是 0（见下面的幂等性说明）⇒ **值不变**；
--   · 所以「ordinal = 0 的行多于一行」等价于「这些行是 DEFAULT 补出来的」，
--     不需要额外标记列，也不需要猜「这行是不是第一行」。
--
-- 【顺序】按 `key` 排，不是按物理行序。`action_types` 上有
-- `UNIQUE (version_id, key)`，`(version_id, key)` 是全序 ⇒
-- 每次重跑得到**同一个**结果。用物理行序的话，重跑一次顺序就变了。
-- 历史写入序已不可考，`key` 字典序是一个**确定性且可复现**的替代。
--
-- 【幂等】回填后同一版本内 ordinal 变成 0,1,2,…，只剩一行还是 0
-- ⇒ 第二次跑子查询只匹配到那**一行**，重算得 0，与现值相同。
-- 迁移会跑很多遍（每个测试进程都跑），所以这条必须重跑无副作用。
--
-- 【不覆盖已有值】子查询只取 `ordinal = 0` 的行，非 0 的一律不碰 ——
-- 那些是正常写入产生的真实序，保留。也不 DELETE、不动任何其它列。
UPDATE action_types AS a
SET ordinal = sub.rn - 1
FROM (
  SELECT id,
         ROW_NUMBER() OVER (PARTITION BY version_id ORDER BY key) AS rn
  FROM action_types
  WHERE ordinal = 0
) AS sub
WHERE a.id = sub.id
  AND a.ordinal = 0;
"#;

/// 抽取 agent 注册表的建表 DDL（REQ-013）。
///
/// 走 [`crate::db::agent_registry::SCHEMA_AGENT_REGISTRY`] 而不是拼进上面的
/// [`ALTER_PATCHES`] 字面量：DDL 仍是**一份**常量，物理上却能被编进上面那条
/// 单一 batch —— 少抄一份，也少一次网络往返，更不必手工同步两处字面量。
///
/// ⚠️ REQ-025：它现在是**纯 DDL**（自带 `BEGIN` / 锁 / `COMMIT` 的版本已删），
/// 事务边界由 [`MIGRATE_LOCK_PREFIX`] / [`MIGRATE_COMMIT`] 提供。
///
/// ⚠️ REQ-039：它**没有**进 [`ALTER_PATCHES`]，所以**永远照发**。
/// 这两张表是本仓独有的、没有历史遗留问题；而 `CREATE TABLE IF NOT EXISTS`
/// 在表已存在时实测**一把锁都不取**（见 [`patched_columns_present`] 的锁表），
/// 放进那张表只会平白多一次存在性判断。
const SCHEMA_AGENT_REGISTRY_PATCH: &str = crate::db::agent_registry::SCHEMA_AGENT_REGISTRY;
