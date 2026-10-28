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
    let cap = SCHEMA.len() + SCHEMA_AGENT_REGISTRY_PATCH.len() + BACKFILL_PATCH.len() + JOIN_STATE_PATCH.len() + SCHEMA_PROJECT_BUNDLES.len() + SCHEMA_VERSION_ACCESS_MAPPINGS.len() + 512;
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
    // REQ-120 · 三态列的类型归一（通常空转，见 `JOIN_STATE_PATCH`）。
    batch.push_str(JOIN_STATE_PATCH);
    batch.push_str(SCHEMA_AGENT_REGISTRY_PATCH);
    // REQ-109 · `ai_cache` 与 agent-registry 同一条 batch（同一把交易级咨询锁）。
    // 新表、无外键、无历史遗留 ⇒ 不需要 ALTER 补列那条路径。
    batch.push_str(crate::db::ai_cache::SCHEMA_AI_CACHE);
    // ⭐ REQ-159 · 项目 / key / 构建包 / 源库 DSN 四张表（+ 一张包字节卫星表）。
    // 它们是**新表、无历史遗留** ⇒ 不进 [`ALTER_PATCHES`]（那张表的跳过判据是逐列的）。
    batch.push_str(SCHEMA_PROJECT_BUNDLES);
    // ⭐ REQ-174 · access mapping 随版本入库（`version_access_mappings`）。
    batch.push_str(SCHEMA_VERSION_ACCESS_MAPPINGS);
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

    /// REQ-111 · 决策指向一个不存在的条目序号。
    ///
    /// CLI 与审核界面**共用**这一条错误（同一个 `decide_item`），
    /// 所以「第 N 个条目不存在」在两个入口上是同一句话、同一个判据。
    #[error("提案里没有第 {seq} 个条目")]
    ItemNotFound { seq: i32 },

    /// REQ-010 · 概念组声明不合法。三条硬约束的理由见 `domain::concept`。
    #[error("概念组声明不合法：{0}")]
    BadConceptGroup(String),

    /// REQ-108 · 关系条目的置信度不合法（缺 method / 分数越界 / band 与分数对不上）。
    ///
    /// 与 `domain::apply::ApplyError::BadConfidence` 是**同一条事实的两次呈现**：
    /// 领域层先判（纯函数可单测），存储层把它翻成自己的错误类型。
    #[error("关系条目的 confidence 不合法：{0}")]
    BadConfidence(String),

    /// ⭐ REQ-129 · AC-006：条目被标为 `applicable: false` ⇒ **拒绝 apply 并点名**。
    ///
    /// 与 `domain::apply::ApplyError::NotApplicable` 是同一条事实的两次呈现
    /// （领域层判、存储层翻）。CLI 打印的是这一条 —— 报错原文里必须**有名字**。
    #[error("条目被标为不可应用（applicable: false），拒绝 apply：{0}")]
    ItemsNotApplicable(String),

    /// ⭐ REQ-129 · 规则 2：doc 条目的 `band` 不许是 `strong`（纵深防御）。
    #[error("doc 条目的 band 不许是 strong：{0}")]
    DocBandStrong(String),
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
  -- REQ-108 · 置信度三栏。**全可空**：NULL = 这个版本没有这条信息
  --（旧版本 / 人工修订），与 0.0（确定不成立）是两件事。
  -- ⚠️ 客户视图**不读**这三栏 —— `domain::view::project` 读的是
  -- `SnapshotView`（它压根没有这三栏），所以「零泄漏」是结构保证。
  confidence          REAL,
  confidence_method   TEXT,
  confidence_signals  JSONB,
  -- REQ-120 · 连接的验证状态（AC-007，三态）：'true' / 'inherit' / 'false'。
  -- TEXT 而不是 BOOLEAN：BA rev.2 把「同名同义列继承」加成第三态。
  -- NULL = 没有这条信息（REQ-120 之前的版本 / 手写提案）。
  join_verified       TEXT,
  join_evidence       TEXT,
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
  -- REQ-113 · 本轮输入的**可查记录**（今天只有 `exclusions`）。
  -- NULL = 没有记录（人工修订 / REQ-113 之前的提案），与「排除了 0 张」是两件事。
  inputs           JSONB,
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
  -- REQ-108 · 置信度三栏（审核界面与校准命令读它）。
  -- NULL = 该条目没有分数（实体条目 / 人工提案），**不是 0 分**。
  confidence          REAL,
  confidence_method   TEXT,
  confidence_signals  JSONB,
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
const ALTER_PATCHES: [(&str, &str, &str); 18] = [
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
    // ── REQ-108 · 置信度三栏（proposal_items / link_types 各三列）─────────
    //
    // 六条一起进这张表，理由与上面三条相同：**能不能跳过是逐列决定的**，
    // 稳态库上一把排他锁都不该取（REQ-039）。类型选 `REAL` 而不是 `NUMERIC`：
    // 分数是两位小数的近似量，不是金额；`REAL` 与 `domain::confidence::Confidence::score`
    // （f64）在 `0.0–1.0`、两位小数的取值域上往返无损。
    (
        "proposal_items",
        "confidence",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS confidence REAL;",
    ),
    (
        "proposal_items",
        "confidence_method",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS confidence_method TEXT;",
    ),
    (
        "proposal_items",
        "confidence_signals",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS confidence_signals JSONB;",
    ),
    (
        "link_types",
        "confidence",
        "ALTER TABLE link_types ADD COLUMN IF NOT EXISTS confidence REAL;",
    ),
    (
        "link_types",
        "confidence_method",
        "ALTER TABLE link_types ADD COLUMN IF NOT EXISTS confidence_method TEXT;",
    ),
    (
        "link_types",
        "confidence_signals",
        "ALTER TABLE link_types ADD COLUMN IF NOT EXISTS confidence_signals JSONB;",
    ),
    // ── REQ-111 · 决策审计两栏 × 两轨（`via` 区分 CLI / 页面）─────────────
    //
    // 「谁在什么时候改的」必须能**查库**回答（AC-009），而不是只看 CLI 的回执：
    // CLI 与页面是两个入口，写的是同一行 —— 审计只能落在**这一行**上，
    // 否则「同 item+track 连签两次只留终态、不产生两条记录」（AC-005）
    // 与「每次决策都有 decided_at / via」会互相打架（追加一张日志表就是两条记录）。
    //
    // 于是两轨各两栏：`*_decided_at` 是最后一次改判的时间，`*_via` 是最后一次
    // 改判的入口（`cli` / `ui`）。后写覆盖前写，与「决策是幂等覆盖」同一条语义。
    (
        "proposal_items",
        "biz_decided_at",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS biz_decided_at TIMESTAMPTZ;",
    ),
    (
        "proposal_items",
        "biz_via",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS biz_via TEXT;",
    ),
    (
        "proposal_items",
        "model_decided_at",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS model_decided_at TIMESTAMPTZ;",
    ),
    (
        "proposal_items",
        "model_via",
        "ALTER TABLE proposal_items ADD COLUMN IF NOT EXISTS model_via TEXT;",
    ),
    // ── REQ-113 · 提案的输入记录（`inputs`）──────────────────────────────
    //
    // 为什么必须**落库**，而不是「审核界面自己去读 exclusions.json」：
    // 评审人看到的那份记录必须是**这一轮抽取实际用的**那一份。若界面回头去读文件，
    // 「抽取之后文件被改了」就会让界面显示的排除清单与提案里的表对不上 ——
    // 而那个不一致**没有任何标记**（看起来都很正常），正是 R5 要消灭的形态。
    //
    // 允许 NULL 且无 DEFAULT：既有提案读出来是 NULL =「没有记录」，
    // 不往历史行回填一个空 `{}` —— 那等于替旧提案声明「这轮没排除任何表」。
    (
        "proposals",
        "inputs",
        "ALTER TABLE proposals ADD COLUMN IF NOT EXISTS inputs JSONB;",
    ),
    // ── REQ-120 · 连接验证两栏（`link_types`）─────────────────────────────
    //
    // 为什么**必须落库**：TTL 是从**快照**导出的（`ontology export` ←
    // `snapshot_store::load_links`），不是从提案导出的。只在 struct 上加字段，
    // `ontok:joinVerified` 在「提案 → 快照 → TTL」这一跳上会**恒为 NULL**，
    // 于是 AC-007 的判据（TTL 里能查到证据）结构上不可达。
    //
    // 无 DEFAULT 且允许 NULL：既有行读出来是 NULL =「这一版没有这条信息」，
    // 不往历史行回填 false —— 回填等于替旧版本断言「这条关系只是命名推断」。
    (
        "link_types",
        "join_verified",
        "ALTER TABLE link_types ADD COLUMN IF NOT EXISTS join_verified TEXT;",
    ),
    (
        "link_types",
        "join_evidence",
        "ALTER TABLE link_types ADD COLUMN IF NOT EXISTS join_evidence TEXT;",
    ),
    // ── REQ-159 · `projects` 补两列（构建包导入：项目备注 + 生效版本指针）──────
    //
    // 为什么 **位置固定在最末两格**：`ALTER_PATCHES` 与 `patched_columns_present`
    // 的探查询是**按列序号**对账的（N 变了几条，那段代码一个字都不用改），
    // 所以「加列」只许**追加**，⛔ 不许插到中间 —— 插进去会让已部署库上的
    // 探查询顺序与 DDL 顺序错位（虽然目前两者都以本数组为唯一来源，
    // 但「只许追加」是这张表的可维护性前提，写在最能被看到的地方）。
    //
    // `label`：项目备注（`POST /api/projects` 的 `label`）。允许 NULL = 没写备注，
    // 与 `Some("")`（写了但为空）不是一回事 —— 建项目时**空串归一成 NULL**。
    //
    // `active_version_id`：**人工激活**写的那一格（`POST …/activate`）。
    // ⛔ 不加 UNIQUE / NOT NULL：新项目还没有生效版本（NULL 有意义），
    // 而 REQ-159 的 P1a **不碰** `query serve` 的读取路径（多项目模式是 P1b），
    // 所以这一列此刻只是「被写下来」，读它的是 P1b。
    (
        "projects",
        "label",
        "ALTER TABLE projects ADD COLUMN IF NOT EXISTS label TEXT;",
    ),
    (
        "projects",
        "active_version_id",
        "ALTER TABLE projects ADD COLUMN IF NOT EXISTS active_version_id UUID REFERENCES ontology_versions(id);",
    ),
];

/// REQ-120 · **把 `link_types.join_verified` 从布尔归一成文本**（三态）。
///
/// # 为什么必须是「无条件但通常空转」的一条，而不是 [`ALTER_PATCHES`] 的一条
///
/// `ALTER_PATCHES` 的跳过判据是「**列是否存在**」。本轮先落的是 `BOOLEAN`（两态），
/// BA rev.2 之后要三态（`true` / `inherit` / `false`），于是**列已存在、类型却不对**
/// —— 恰好是那张表的跳过判据看不见的情形。
///
/// 放一个 `DO` 块：**稳态库上一把锁都不取**（只读 `information_schema`，
/// 条件为假就不发 `ALTER`），只有真的还是 `boolean` 的库才做一次类型转换。
/// 与 [`BACKFILL_PATCH`] 同一条纪律：**无条件执行、可重复跑、代价可忽略**。
///
/// ⚠️ 类型转换是**有损的**：`boolean → text` 只可能得到 `'true'` / `'false'`，
/// 不会凭空变出 `'inherit'`（继承态是本轮 rev.2 才引入的，历史上没有这条数据）。
const JOIN_STATE_PATCH: &str = r#"
-- REQ-120 · link_types.join_verified: BOOLEAN → TEXT（三态 true / inherit / false）。
DO $$
BEGIN
  IF EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'link_types'
      AND column_name = 'join_verified'
      AND data_type = 'boolean'
  ) THEN
    ALTER TABLE link_types
      ALTER COLUMN join_verified TYPE TEXT
      USING (CASE WHEN join_verified THEN 'true' ELSE 'false' END);
  END IF;
END $$;
"#;

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

/// ⭐ REQ-159 · **构建包导入**的四张表（+ 一张包字节卫星表）。
///
/// # 为什么是独立常量、且**不进** [`ALTER_PATCHES`]
///
/// 它们是**新表**，没有历史遗留列要补 —— 而 `ALTER_PATCHES` 的跳过判据是
/// 「某一列是否存在」（见 [`patched_columns_present`] 的锁表）。新表塞进去
/// 只会在稳态库上多几次存在性判断，不改变任何行为。
///
/// `CREATE TABLE IF NOT EXISTS` 在表已存在时**一把锁都不取**（同
/// `SCHEMA_AGENT_REGISTRY` / `ai_cache` 的先例），所以每轮 `migrate()` 照发无代价。
///
/// # 与契约（`BA/demands/REQ-159/demand.md` §十六 · §五）的逐条对应
///
/// | 表 | 契约给的形状 | 本文件 |
/// | --- | --- | --- |
/// | `project_keys` | 一项目一把 key，只存 `sha256` | 逐字（+ `ON CONFLICT` 不写，靠唯一索引兜） |
/// | `project_keys_one_active` | 部分唯一索引 | 逐字 —— 「一项目一把」落在**数据库**上（AC-010） |
/// | `project_bundles` | 包的状态机 + manifest 原文 + 整包 sha256 | 逐字 |
/// | `project_databases` | DSN 密文（⛔ 永不回显） | 逐字 |
/// | （无） | 契约里**没有**存包字节的列 | 见下面的 `project_bundle_payloads` |
///
/// # ⚠️ 唯一一处对契约的**扩展**：`project_bundle_payloads`
///
/// 契约的 `project_bundles` 只有 `bytes BIGINT`（**大小**）与 `bundle_sha256`
/// （指纹），**没有**存字节的地方。而端点的分工是「上传时只校验不解析、
/// import 时才解析」—— 包字节必须活过这两次请求。三条路里：
///
/// - 放进程内存 ⇒ 重启即丢，`import` 变成「看运气」；
/// - 放磁盘 spool ⇒ 多一个 `--bundle-dir` 参数、多一个「文件与行不一致」的失败面，
///   而且它在 `tests/test_req107_source_enumeration.rs` 的写点登记里是**又一条出口**
///   （包字节不是客户视图，但那次登记的必要性本身就说明它不是免费的）；
/// - **放一张卫星表** ⇒ 与行同生共死、幂等、可事务、零残留（AC-006）。
///
/// 选第三条。**四张契约表的形状逐字未改**，扩展只多一张表 —— 交付里显式登记。
/// 包体积的真实量级是百 KB 级（demand §十-1：DB 只带描述不带数据），
/// 上限 100 MB 是**拒绝阈值**，不是常态；`BYTEA` 在 PG 里走 TOAST，不影响别的列的行宽。
const SCHEMA_PROJECT_BUNDLES: &str = r#"
-- ⭐ REQ-159 · key 表：**只存 sha256**，明文只在 `POST /api/projects` 的响应里出现一次。
CREATE TABLE IF NOT EXISTS project_keys (
  id                UUID PRIMARY KEY,
  project_id        UUID NOT NULL REFERENCES projects(id),
  token_sha256      TEXT NOT NULL UNIQUE,
  -- ⭐ 契约变更（2026-10-27 10:25）：key 要**能再次查看**，所以多一列可逆密文。
  -- 两列都留：鉴权走 `token_sha256`（快、不碰密钥），查看走 `token_ciphertext`
  -- （要密钥才能解）。⛔ 只留 ciphertext 的方案会让每次鉴权都解密。
  token_ciphertext  TEXT NOT NULL,
  -- ⛔ NOT NULL：这把 key 给谁用（审计归属）。空串在建 key 的那一层就被拒。
  label             TEXT NOT NULL,
  created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
  revoked_at        TIMESTAMPTZ
);

-- ⚠️ 幂等补列：`token_ciphertext` 是契约变更之后才加的，而本表在**同一个 PR** 里
-- 已经存在过一小段时间（本分支的本地测试库）。`CREATE TABLE IF NOT EXISTS` 不会
-- 给已存在的表补列，所以这里补一条；补完**尽可能**把 NOT NULL 收上去 ——
-- 有 NULL 行时（本地遗留行，明文已不可考）保持可空并留给运行期拒绝，⛔ 不伪造密文。
ALTER TABLE project_keys ADD COLUMN IF NOT EXISTS token_ciphertext TEXT;
DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name = 'project_keys'
               AND column_name = 'token_ciphertext'
               AND is_nullable = 'YES')
     AND NOT EXISTS (SELECT 1 FROM project_keys WHERE token_ciphertext IS NULL)
  THEN
    ALTER TABLE project_keys ALTER COLUMN token_ciphertext SET NOT NULL;
  END IF;
END $$;

-- 「一项目一把」**落在数据库上**（AC-010）：并发两把 active key 时，
-- 第二把必须被 23505 拒掉，而不是只靠应用层先查后插（那是 TOCTOU）。
CREATE UNIQUE INDEX IF NOT EXISTS project_keys_one_active
  ON project_keys(project_id) WHERE revoked_at IS NULL;

-- ⭐ REQ-159 · 构建包：状态机 + manifest 原文 + 整包指纹。
--
-- `state` 的取值集合由 `provision::bundle::BundleState` 收口
-- （uploaded / db_pending / imported / activated / failed），
-- 这里不加 CHECK：状态机的**唯一**权威是那个枚举，加一份 CHECK 就是两份定义。
CREATE TABLE IF NOT EXISTS project_bundles (
  id                   UUID PRIMARY KEY,
  project_id           UUID NOT NULL REFERENCES projects(id),
  state                TEXT NOT NULL,
  manifest             JSONB NOT NULL,
  bundle_sha256        TEXT NOT NULL,
  bytes                BIGINT NOT NULL,
  uploaded_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  imported_version_id  UUID REFERENCES ontology_versions(id),
  imported_at          TIMESTAMPTZ,
  error_code           TEXT,
  error_message        TEXT
);

-- 包字节（见上面 SCHEMA_PROJECT_BUNDLES 的说明：这是对契约的唯一扩展）。
CREATE TABLE IF NOT EXISTS project_bundle_payloads (
  bundle_id  UUID PRIMARY KEY REFERENCES project_bundles(id),
  data       BYTEA NOT NULL
);

-- ⭐ REQ-159 · 源库 DSN：**密文列**（AES-256-GCM，密钥来自环境变量）。
-- ⛔ 这一列的值在任何响应 / 日志 / 审计里都不出现 —— 判据见
-- `tests/test_req159_bundles.rs` 的 AC-009。
CREATE TABLE IF NOT EXISTS project_databases (
  project_id       UUID PRIMARY KEY REFERENCES projects(id),
  dialect          TEXT NOT NULL,
  dsn_ciphertext   TEXT NOT NULL,
  updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
  -- 三态：NULL = 还没测过 / true = 上次测通 / false = 上次测不通。
  -- `import` 只认 true（「配 DSN」与「DSN 可用」是两件事，demand §四 状态机）。
  last_check_ok    BOOLEAN,
  last_check_at    TIMESTAMPTZ,
  last_error_code  TEXT
);
"#;

/// ⭐ REQ-174 · **access mapping 随版本入库** —— 一张新表，无历史遗留。
///
/// # 为什么挂在「版本」上，不是「项目」上
///
/// `activate` 换的是 `projects.active_version_id` ⇒ AM 自动跟着**生效版本**走，
/// 不需要第二套「AM 生效指针」；激活 / 回滚的语义与本体完全一致（demand §2.1）。
///
/// # 为什么 `raw` 是 TEXT 而不是 JSONB
///
/// 判据是「`raw` 与包内 `am/access-mapping.json` **逐字节相同**」（AC-001）。
/// JSONB 会重排键序 / 归一空白 ⇒ 存进去就再也比不回来了。这一列只搬字节，
/// 解析只发生在**写入前**（`AccessMapping::parse` + `bind`，见
/// `console::provision::import_bundle`），读出来的原文交给服务端在**加载快照的同一批**
/// 里解析。
///
/// # 为什么 `ON DELETE CASCADE`
///
/// 与 `object_types` / `link_types` 同一条：版本没了，挂在上面的 AM 也不该留 ——
/// 否则删版本会留下一条谁也认不出的映射行。
///
/// # 为什么**不进** [`ALTER_PATCHES`]
///
/// 新表，没有历史列要补。`CREATE TABLE IF NOT EXISTS` 在表已存在时一把锁都不取
/// （同 `SCHEMA_AGENT_REGISTRY` / `ai_cache` / `SCHEMA_PROJECT_BUNDLES` 的先例）。
const SCHEMA_VERSION_ACCESS_MAPPINGS: &str = r#"
-- ⭐ REQ-174 · 每个版本一份 access mapping（导入时校验通过才写）。
--
-- `raw` = 包内 `am/access-mapping.json` 的**原样字节**（UTF-8）。
-- ⛔ 这一列不进日志 / 错误消息 / 审计（AC-008）：只有 `db::access_store` 读写它。
CREATE TABLE IF NOT EXISTS version_access_mappings (
  version_id  UUID PRIMARY KEY REFERENCES ontology_versions(id) ON DELETE CASCADE,
  raw         TEXT        NOT NULL,
  sha256      TEXT        NOT NULL,
  sources     INTEGER     NOT NULL,
  entities    INTEGER     NOT NULL,
  stored_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#;
