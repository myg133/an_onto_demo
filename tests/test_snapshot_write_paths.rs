//! REQ-012 · 落库路径去重（根因）+ 历史 `ordinal` 回填。
//!
//! # 这个文件守的是什么
//!
//! REQ-011 的缺陷本体是**两份同构实现**：
//! `PgStore::save_snapshot`（`ontology snapshot save`）与
//! `PgApplier → write_snapshot_in`（`propose apply`，**生产写路径**）
//! 各抄了一份五栏 INSERT，生产那份抄漏了 `action_types` / `functions` ——
//! 一次 apply 把两栏静默清空，事务内不报任何错。
//!
//! REQ-011 补上了那两段 INSERT，**病根没除**：两份实现仍然并存。
//! REQ-012 把「行写入」收成一份，并补上 SCHEMA_PATCH 只加列不回填的历史洞。
//!
//! # 判定口径
//!
//! 这里同时用两种判据，缺一不可：
//! - **运行期**（`test_two_entry_points_write_identical_rows` 等）：把同一个
//!   `Snapshot` 分别经两条入口落库，比对数据库里的行。
//!   这是 REQ-011 当初**应该**有、却没有的那条测试。
//! - **结构期**（`test_apply_does_not_reimplement_the_inserts`）：
//!   判别「重复回来了」。共享层被绕过（某一边又自己抄一份）在运行期
//!   **未必**表现为行为差异 —— 抄得一模一样时行为当然一致。
//!   要抓住的是**结构**本身，所以必须有源码断言兜底
//!   （沿用 `test_no_write_path.rs` 的既有做法：`include_str!` + 精确匹配）。

mod common;
use common::{crate_root, read_source};
use common::test_pool;
use ontok::db::apply::PgApplier;
use ontok::db::pg::StoreError;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{NewItem, NewProposal, PgProposalStore};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::concept::ConceptGroup;
use ontok::domain::review::{Decision, ItemKind, Track};
use ontok::domain::snapshot::{
    ActionType, EntityType, Evidence, Function, LinkType, Property, Snapshot, WriteBinding,
};
use ontok::domain::snapshot::ExtractedBy;
use serde_json::json;
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;

// ── 五张被 `insert_snapshot_rows` 写的表 ──────────────────────

const SNAPSHOT_TABLES: [&str; 5] = [
    "object_types",
    "link_types",
    "action_types",
    "functions",
    "concept_groups",
];

/// 每一张表的「除 id / version_id 外的全部列」，按 key 排序。
///
/// 刻意**不**逐列手写 SELECT —— 手写就又抄了一份列清单，
/// 「加一栏忘了同步」正是本 REQ 要根治的失败模式。改成
/// 「从 `information_schema` 现读列名」⇒ 少一栏时两边同时发现。
async fn rows_except_id_and_version(pool: &PgPool, table: &str, version_id: Uuid) -> Vec<Vec<String>> {
    let cols: Vec<(String,)> = sqlx::query_as(
        "SELECT column_name FROM information_schema.columns
         WHERE table_name = $1 AND column_name NOT IN ('id', 'version_id')
         ORDER BY column_name",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap();

    let list = cols
        .iter()
        .map(|(c,)| c.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {list} FROM {table} WHERE version_id = $1 ORDER BY key");

    let mut out: Vec<Vec<String>> = Vec::new();
    let rows = sqlx::query(&sql).bind(version_id).fetch_all(pool).await.unwrap();
    for row in rows {
        let mut vals = vec![];
        for i in 0..cols.len() {
            let v: Option<String> = row.try_get(i).unwrap_or(None);
            vals.push(v.unwrap_or_else(|| "NULL".to_string()));
        }
        out.push(vals);
    }
    out
}

fn ev(table: &str, column: &str) -> Evidence {
    Evidence { table: table.into(), column: column.into(), claim: format!("{table}.{column}") }
}

/// 五栏全满的快照 —— 每栏都给**非空且各不相同的**取值，
/// 少一栏 / 少一列 / 序列化分叉都会在这条比对里现形。
fn rich_snapshot(version_id: Uuid) -> Snapshot {
    Snapshot {
        version_id,
        seq: 1,
        extracted_by: Some(ExtractedBy {
            agent: "mcode-agent".into(),
            version: "v1".into(),
            round: 1,
            failed_groups: vec!["grp_a".into()],
            source: None,
        }),
        entity_types: vec![
            EntityType {
                key: "customer".into(),
                display_name: "客户".into(),
                description: Some("客户主档".into()),
                aliases: vec!["顾客".into(), "买家".into()],
                parent: None,
                disjoint_with: vec!["vendor".into()],
                properties: vec![Property {
                    name: "编号".into(),
                    column: "cust_no".into(),
                    nullable: false,
                    note: Some("业务主键".into()),
                    sample: Some("C001".into()),
                }],
                evidence: vec![ev("t_customer", "cust_no")],
            },
            EntityType {
                key: "order".into(),
                display_name: "订单".into(),
                description: None, // ← 三态之「没写」：必须落成 NULL，不是 ''
                aliases: vec![],
                parent: Some("customer".into()),
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
        ],
        link_types: vec![LinkType {
            key: "customer_has_order".into(),
            from_type: "customer".into(),
            to_type: "order".into(),
            cardinality: "1:N".into(),
            semantic: "has".into(),
            evidence: vec![ev("t_order", "cust_no")],
            confidence: None,
        }],
        action_types: vec![
            ActionType {
                key: "publish_customer".into(),
                target: "customer".into(),
                parameters: json!({ "effective_from": "date" }),
                preconditions: json!({ "status": "draft" }),
                effects: vec![WriteBinding {
                    system: "源库".into(),
                    table: "customer".into(),
                    field: "status".into(),
                    key_expr: json!({ "cust_id": "$1" }),
                    value_map: [("published".to_string(), "PUBLISHED".to_string())]
                        .into_iter()
                        .collect(),
                    supported_values: vec!["draft".into()],
                    conflict_field: Some("updated_at".into()),
                    evidence: vec![ev("t_customer", "status")],
                }],
                permission: json!({ "role": "运营" }),
                audit_policy: json!({ "retain_days": 365 }),
                evidence: vec![ev("t_customer", "status")],
            },
            ActionType {
                key: "archive_order".into(),
                target: "order".into(),
                parameters: json!({}),
                preconditions: json!({}),
                effects: vec![],
                permission: json!({}),
                audit_policy: json!({}),
                evidence: vec![],
            },
        ],
        functions: vec![Function {
            key: "customer_count".into(),
            signature: "customer_count(customer) -> i32".into(),
            allowed_types: vec!["customer".into(), "order".into()],
            evidence: vec![ev("t_order", "cust_no")],
        }],
        concept_groups: vec![ConceptGroup {
            key: "package_core".into(),
            display_name: "核心套餐".into(),
            members: vec!["customer".into(), "order".into()],
            scope_hint: Some("包含下单主体".into()),
            extracted_by: None, // ← ADR-001：人工修订 = SQL NULL
            low_confidence: Some(false),
            evidence: vec![ev("t_order", "pkg_id")],
        }],
    }
}

// ── F2 · 两条入口必须写出一模一样的行 ────────────────────────

/// REQ-012 的核心判定，也是 REQ-011 当初**应该**有而没有的那条测试。
///
/// 做法：把同一个 `Snapshot` 值分别经
/// - 入口 A：`save_snapshot`（CLI 那条），落到 v1；
/// - 入口 B：`apply_proposal`（生产写路径），落到 v2；
///
/// 然后**逐表逐列**比数据库里的行。两条入口只差「加了一个实体类型」
/// 这一个已知的 delta，其余必须逐字节相同。
///
/// # 为什么这条能直接逮住 REQ-011
///
/// REQ-011 漏掉 `action_types` / `functions` 两栏时，
/// 「apply 前后计数」是 `[action 2 → 0]`。这里比的是**整表整行**，
/// 连列顺序、序列化结果、`ordinal` 都在内 —— 少一栏立刻现形。
#[tokio::test]
async fn test_two_entry_points_write_identical_rows() {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let applier = PgApplier::new(pool.clone());
    projects.migrate().await.unwrap();

    let project = common::fresh_project(&projects, "req012").await;
    let v1 = projects.new_version(project.id).await.unwrap().id;

    // 入口 A：save_snapshot
    let base = rich_snapshot(v1);
    store.save_snapshot(&base).await.unwrap();

    // 入口 B：apply —— 唯一的新增项是一个**全新**的实体类型
    let p = proposals
        .create(
            project.id,
            NewProposal { base_version_id: v1, round: 1, extracted_by: None },
            &[NewItem {
                kind: ItemKind::AddEntityType,
                target_key: "subscription".into(),
                payload: json!({
                    "key": "subscription", "display_name": "订阅",
                    "properties": [], "evidence": []
                }),
            }],
        )
        .await
        .unwrap();
    for it in proposals.items(p.id).await.unwrap() {
        proposals.set_decision(it.id, Track::Biz, Decision::Accepted).await.unwrap();
        proposals.set_decision(it.id, Track::Model, Decision::Accepted).await.unwrap();
    }
    let v2 = applier.apply_proposal(p.id).await.unwrap();

    // ── ① 四张表必须**逐行逐列**相同 ──
    // 这四张里包含 REQ-011 漏掉的那两张（action_types / functions）。
    for t in ["link_types", "action_types", "functions", "concept_groups"] {
        let a = rows_except_id_and_version(&pool, t, v1).await;
        let b = rows_except_id_and_version(&pool, t, v2).await;
        assert!(!a.is_empty(), "前提失效：{t} 在 v1 里一行都没有，这条比对没有判别力");
        assert_eq!(
            a, b,
            "{t}：save_snapshot 与 apply 写出的行必须逐字节一致 —— \
             两条入口共用 insert_snapshot_rows，出现差异说明又分叉了"
        );
    }

    // ── ② object_types：只多出「subscription」一行，且既有行的
    //    ordinal / 全部列都不许动 ──
    // 两侧都是 `ORDER BY key`，而 "customer" < "order" < "subscription" ——
    // 新增行必然排在最后，前 len(a) 行应当与 v1 逐行相同。
    let a = rows_except_id_and_version(&pool, "object_types", v1).await;
    let b = rows_except_id_and_version(&pool, "object_types", v2).await;
    assert_eq!(b.len(), a.len() + 1, "apply 只应新增一个实体类型行");
    assert!(
        b.last().unwrap().iter().any(|c| c == "subscription"),
        "新增行的 key 必须是 subscription，实际末行：{:?}",
        b.last()
    );
    assert_eq!(
        a,
        &b[..b.len() - 1],
        "object_types：既有实体类型的行（含 ordinal）不得被 apply 改动"
    );
}

/// REQ-012 · `save_snapshot` 侧的对外错误语义**逐字未变**：
/// 唯一约束冲突仍映射成 [`StoreError::KeyConflict`]，且**带上出错的 key**。
///
/// 构造方式：一份快照里放两个同 key 的实体 —— 第二条 INSERT 撞
/// `UNIQUE (version_id, key)`（23505）。
#[tokio::test]
async fn test_save_snapshot_keeps_key_conflict_mapping() {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    projects.migrate().await.unwrap();
    let project = common::fresh_project(&projects, "req012conflict").await;
    let v1 = projects.new_version(project.id).await.unwrap().id;

    let mut snap = rich_snapshot(v1);
    snap.entity_types.push(snap.entity_types[0].clone()); // 同 key 复制一份

    match store.save_snapshot(&snap).await {
        Err(StoreError::KeyConflict(k)) => {
            assert_eq!(k, "customer", "KeyConflict 必须带上出错那一行的 key");
        }
        other => panic!("save_snapshot 的唯一冲突必须仍映射成 KeyConflict，实际得到：{other:?}"),
    }
}

// ── F2 · 结构：重复不许回来 ────────────────────────────────

/// REQ-012 · **共享层被绕过**（某一边又自己抄一份 INSERT）必须被抓住。
///
/// # 为什么这条不是多余的
///
/// 上面那条运行期比对抓的是「写出来的行为不一样」。
/// 如果有人把五段 INSERT 原样抄回 `apply.rs`，**行为完全一致**，
/// 运行期照样全绿 —— 而 REQ-011 的病因（两份实现）就此复活，
/// 下一个加栏的人照样会漏。所以必须有结构判据。
#[tokio::test]
async fn test_apply_does_not_reimplement_the_inserts() {
    let apply_src = include_str!("../src/db/apply.rs");

    for t in SNAPSHOT_TABLES {
        let stmt = format!("INSERT INTO {t}");
        assert!(
            !apply_src.contains(&stmt),
            "db/apply.rs 出现了 `{stmt}` —— 五栏行写入必须只有 \
             snapshot_store::insert_snapshot_rows 一份实现。\
             两份实现并存正是 REQ-011「静默丢数据」的根因，不得复活。"
        );
    }

    // apply 侧也不许自己长出 save 那套映射
    assert!(
        !apply_src.contains("map_key_conflict"),
        "db/apply.rs 不该出现 map_key_conflict —— apply 侧的冲突语义是 StoreError::Db"
    );

    // 反向确认：五段 INSERT 确实**存在于**共享层（否则上面全是空断言）
    let shared = include_str!("../src/db/snapshot_store.rs");
    let mut hits = 0;
    for t in SNAPSHOT_TABLES {
        let stmt = format!("INSERT INTO {t}");
        // 共享层应恰好一处：写在 insert_snapshot_rows 里
        assert!(
            shared.contains(&stmt),
            "共享层 snapshot_store.rs 里找不到 `{stmt}` —— 判别力前提失效"
        );
        hits += 1;
    }
    assert_eq!(hits, 5, "五栏都要在共享层里有对应实现");
}

// ── REQ-033 · 结构：删旧行与版本行 UPDATE 各只有一份 ──────────
//
// REQ-012 把五栏 INSERT 收成了唯一一份，但当时 BA 的裁定范围只到「那 5 段
// INSERT」，同一条落库链上的另外两步仍各留着抄写：
//   · 「删旧行」五表循环 —— 两份**逐字相同**（save / apply）
//   · 版本行 UPDATE extracted_by —— 三份
// REQ-011 的病因从来不是「INSERT 写错了」，是「同一条链被抄了两份」。
// 只治 INSERT 等于把同一个病根换个地方再种一次。

/// REQ-033 的三条判据针。每条都必须**恰好**存在一份。
const REQ033_NEEDLES: [(&str, &str); 3] = [
    (
        "五张快照表的清空顺序",
        r#""link_types", "object_types", "action_types", "functions", "concept_groups""#,
    ),
    ("删旧行的 DELETE", "DELETE FROM {t} WHERE version_id = $1"),
    (
        "版本行 extracted_by 的 UPDATE",
        "UPDATE ontology_versions SET extracted_by = $2 WHERE id = $1",
    ),
];

/// 读一个 `.rs` 的**代码**（跳过 `//` 注释行与空行，空白归一化成单空格）。
///
/// 归一化是必要的：五张表的清单在源文件里是**一个元素一行**，
/// 直接用原文字面量匹配会漏 —— 归一化后单行写与多行写得到同一个串。
fn code_text(rel: &str) -> String {
    read_source(rel)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 数 `src/` 全树里某个字面量出现几次，返回 `文件:次数` 明细。
///
/// **为什么跳过注释行**：沿用 `test_extraction_agent.rs::test_domain_layer_stays_io_free`
/// 的既有做法 —— 模块头的注释里写着这条纪律本身（共享层的文档注释里
/// 就列着五张表的顺序），那不是第二份实现；注释掉的 SQL 更不是。
///
/// ⚠️ **已知边界（如实记录，不声称完备）**：`code_text()` 只 `filter`
/// 掉以 `//` 开头的**行**，对 `/* */` **块注释一无所知**。所以真实边界是：
/// - 抄写藏在 **`//` 行注释**里 ⇒ 判据**看不见**（真盲区，漏报）；
/// - 抄写藏在 **`/* */` 块注释**里 ⇒ 判据**看得见**，而且会被**计入计数**
///   ⇒ 误报成「多了一份实现」（假阳性）。
///
/// ⚠️ 这个方向**与旧注释写的正好相反**（旧文案说「块注释看不见」）。
/// QA 探针 P7 实测证伪了旧文案：判据在块注释里会数到。
///
/// 之所以仍够用：`//` 是本仓**主导风格**（`src/` 下 6762 行行注释），
/// 而 `src/` 里真实块注释**0 处** —— 实测唯一 2 处 `/*` 命中是 `//` 注释里的
/// markdown `**` 加粗，注释掉的 SQL 也是 0 处。
/// 结论（够用）成立，但**理由是「本仓没有块注释」，不是「块注释看不见」**。
/// 要同时堵上两个口子得写真正的词法分析，超出本判据该做的事。
fn src_code_hits(needle: &str) -> Vec<(String, usize)> {
    let root = crate_root().join("src");
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("读不到 src/").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let rel = format!("src/{}", path.strip_prefix(&root).unwrap().to_string_lossy());
            let n = code_text(&rel).matches(needle).count();
            if n > 0 {
                out.push((rel, n));
            }
        }
    }
    out.sort();
    out
}

/// REQ-033 · 「删旧行」与「版本行 UPDATE」在全仓各自**只有一份**实现。
///
/// # 判据口径：**计数**，不是 `!contains`
///
/// 上一批（REQ-012）的判据是「`apply.rs` 里不许出现某串」。
/// REQ-033 强一档：**全 `src/` 树里数出现次数，必须恰好等于 1**。
/// 差别很实际：「只锁 `apply.rs`」挡不住有人把副本放进**第三个文件**
/// （新加的写路径模块），计数能挡。反过来若将来真出现第二份，
/// 计数会从 1 变 2 而变红，并打印「长在哪、几份」。
#[test]
fn test_delete_loop_and_version_update_have_exactly_one_home() {
    // 判别力前提：确实扫到了文件，否则下面全是空断言
    assert!(
        !src_code_hits("fn ").is_empty(),
        "src/ 一个文件都没扫到 —— 本判据是空断言"
    );

    for (what, needle) in REQ033_NEEDLES {
        let hits = src_code_hits(needle);
        let total: usize = hits.iter().map(|(_, n)| n).sum();
        assert_eq!(
            total, 1,
            "`{what}` 必须全仓只有一份实现，实际 {total} 份，分布 {hits:?} —— \
             同一份落库逻辑存在第二份抄写，就是 REQ-011 那个\
             「加一栏忘了同步」的失败模式的起点"
        );
    }

    // 直接点名 `apply.rs`：BA 的裁定就是「apply.rs 不再出现那两处」。
    // 这里用**原文**匹配（连注释一起看），比上面更严 ——
    // 免得有人把 SQL 抄进去、注释说是「参考」。
    let apply_src = include_str!("../src/db/apply.rs");
    for (what, needle) in REQ033_NEEDLES {
        assert!(
            !apply_src.contains(needle),
            "apply.rs 里又出现了 `{what}` —— 它必须只有 \
             snapshot_store 里那一份实现"
        );
    }

    // 反向确认：三样确实**在**共享层里（否则上面全是空断言）
    let shared = code_text("src/db/snapshot_store.rs");
    for (what, needle) in REQ033_NEEDLES {
        assert!(
            shared.contains(needle),
            "共享层 snapshot_store.rs 的代码里找不到 `{what}` —— 判别力前提失效"
        );
    }
}

// ── REQ-033 · 行为：收口之后，冻结闸门与事务顺序逐字未变 ──────

/// REQ-033 · 收口之后，落库链的两条性质：**冻结闸门在最前** + **删旧行覆盖五表**。
///
/// # 为什么合并成一条而不是两条
///
/// 沿用本文件既有的做法（见上面 `test_backfill_…` 的理由）：每个
/// `#[tokio::test]` 都自建连接池并**各跑一次 `migrate()`**，而建表 DDL 要拿
/// `AccessExclusiveLock`。本文件里的测试是**并行**的，于是每多一条 DB 测试
/// 就多一份并发 DDL 压力砸在共享集群上。本 REQ 实测：新增两条时首轮就打出过
/// `40P01 deadlock detected`（migrate 的 AccessExclusiveLock 与另一条测试
/// 普通读写的 RowShareLock 互等），重跑即绿 —— 是竞争，不是代码缺陷，
/// 但那正是本文件早就付出过代价去压的东西。
///
/// 两条性质共用同一个 project，串在一条里跑，连接与迁移次数降到 1/2。
///
/// ① **冻结闸门必须先于任何 DELETE / 版本行 UPDATE 触发**。
///
/// # 这条钉的是什么
///
/// 收口本身多了一个能放错位置的地方：把删旧行提到冻结闸门之前。
/// 后果是**在冻结版本上删数据** —— 旧行被抹掉，新行写不进去
/// （闸门返回错误、事务回滚），一个已经冻结的版本内容变空。
/// 这是比 REQ-011 那次更糟的事故面：那次丢的是一栏，这次毁的是整个版本。
///
/// # 为什么「断言返回 VersionFrozen」不够
///
/// 那种实现**也能**返回 `VersionFrozen`：把 DELETE 提到闸门之前，
/// 闸门照样在末尾返回 `Err`。所以判别力必须来自额外那两条断言 ——
/// **五张表的旧行一行都没少，且版本行 `extracted_by` 没被改成 NULL**。
/// 只有「什么都没发生」才是闸门真的在最前面。
///
/// ② **重存时五张表都要被清空**。
///
/// 收口前的重存断言只逐张表点过 `object_types` / `link_types` /
/// `action_types` / `functions` 的**计数**，`concept_groups` 从没被单独钉过。
/// 而删旧行循环漏掉某张表的后果恰恰是「那一张表的旧行**残留**」——
/// 残留在只断言别处的计数时未必看得见。这里改成**五张表逐张断言归零**。
#[tokio::test]
async fn test_freeze_gate_precedes_delete_and_delete_covers_five_tables() {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    projects.migrate().await.unwrap();
    let project = common::fresh_project(&projects, "req033chain").await;

    // ══ ① 冻结闸门先于删旧行 ══════════════════════════════════
    let v = projects.new_version(project.id).await.unwrap().id;
    // 先存一份**五张表全满**的
    store.save_snapshot(&rich_snapshot(v)).await.unwrap();
    for t in SNAPSHOT_TABLES {
        assert!(
            !rows_except_id_and_version(&pool, t, v).await.is_empty(),
            "前提失效：{t} 在冻结之前一行都没有，这条断言没有判别力"
        );
    }

    projects.freeze(v).await.unwrap();

    // 再存一份**五张表全空**、且 `extracted_by = None` 的。
    // 两条「抢跑」都会留下可观测痕迹：
    //   · DELETE 抢在闸门前 → 五张表旧行被抹掉；
    //   · 版本行 UPDATE 抢在闸门前 → `extracted_by` 被写成 NULL。
    // 正确实现必须是**什么都没发生**。
    let mut empty = rich_snapshot(v);
    empty.entity_types.clear();
    empty.link_types.clear();
    empty.action_types.clear();
    empty.functions.clear();
    empty.concept_groups.clear();
    empty.extracted_by = None;

    let r = store.save_snapshot(&empty).await;
    assert!(
        matches!(r, Err(StoreError::VersionFrozen { .. })),
        "冻结版本必须拒绝写入，实际：{r:?}"
    );

    for t in SNAPSHOT_TABLES {
        let rows = rows_except_id_and_version(&pool, t, v).await;
        assert!(
            !rows.is_empty(),
            "{t} 的旧行被那次**被拒**的写入删掉了 —— \
             冻结闸门与「删旧行」的先后顺序被改动了"
        );
    }
    assert!(
        store.get_extracted_by(v).await.unwrap().unwrap().is_some(),
        "那次被拒的写入把版本行的 extracted_by 写成了 NULL —— \
         冻结闸门与版本行 UPDATE 的先后顺序被改动了"
    );

    // ══ ② 删旧行覆盖五张表 ════════════════════════════════════
    // 换一个**未冻结**的版本：落库链会跑完整三步。
    // 存空快照 ⇒ 删旧行执行、再写 0 行 ⇒ 五张表归零。
    // 漏删某张表 ⇒ 那张表残留旧行。
    let v2 = projects.new_version(project.id).await.unwrap().id;
    store.save_snapshot(&rich_snapshot(v2)).await.unwrap();
    for t in SNAPSHOT_TABLES {
        assert!(
            !rows_except_id_and_version(&pool, t, v2).await.is_empty(),
            "前提失效：{t} 一开始就没有行，这条断言没有判别力"
        );
    }

    let mut empty2 = rich_snapshot(v2);
    empty2.entity_types.clear();
    empty2.link_types.clear();
    empty2.action_types.clear();
    empty2.functions.clear();
    empty2.concept_groups.clear();
    store.save_snapshot(&empty2).await.unwrap();

    for t in SNAPSHOT_TABLES {
        let rows = rows_except_id_and_version(&pool, t, v2).await;
        assert!(
            rows.is_empty(),
            "{t} 存空快照后还剩 {} 行 —— 删旧行循环漏了这张表，旧行残留了",
            rows.len()
        );
    }
}

// ── F1 · 历史 ordinal 回填 ────────────────────────────────

/// 「插入顺序 ≠ key 字典序」这条前提自检的**唯一实现**（REQ-040 · AC-004）。
///
/// 确定性判据只在「插入顺序 ≠ 字典序」时才区分得出「按 key 排」与
/// 「按物理行序排」：一旦两者一致，回填用哪种排法都给出同一个答案，
/// 测试会**全绿**而判据**已经失效** —— 这是本文件最危险的一种失败方式，
/// 因为它没有任何红灯。
///
/// 同一个断言会被调用两次，**主体不同**：
/// - 插入前：断言入参 `keys`（廉价早失败，不碰网络）；
/// - 插入前、拼完 SQL 后：断言 `push_values` **实际迭代过的那份**（见
///   [`insert_legacy_actions`] 里的说明）—— 这一条才是**真正**的兜底。
///
/// 为什么是就地断言、而不是靠 `ctid` 之类去核对物理行序：
/// 物理量不可靠（实测同一页 `offset ≥ 10` 时按 `ctid` **文本**排序就排错，
/// 而 `ctid` 的自然序是数值序）。
fn assert_insert_order_is_not_dict_order(pushed: &[&str]) {
    let mut dict_order = pushed.to_vec();
    dict_order.sort_unstable();
    assert_ne!(
        pushed,
        dict_order.as_slice(),
        "历史行的插入顺序必须与 key 字典序相反 —— 否则「按 key 排」与「按物理行序排」\
         给出同一个结果，回填的实现对测试而言就不可区分了"
    );
}

/// 造一批「历史行」：`ordinal` 全是 DEFAULT 0，且**插入顺序故意与
/// key 字典序相反** —— 这样若回填依赖物理行序，结果就会与期望不符。
///
/// # REQ-040 · 为什么是一条多行 INSERT，不是 `for k in keys` 逐条插
///
/// 回填 `UPDATE`（`src/db/pg.rs` 的 `BACKFILL_PATCH`）的子查询
/// `SELECT … FROM action_types WHERE ordinal = 0` **不带 `version_id` 过滤**，
/// 是**全局**的；而断言只读本版本（`ordinals_of` 的 `WHERE version_id = $1`）。
/// 于是并发 `migrate()` 的回填只要**看得到这批行的一部分**，就会按那一部分重算
/// ordinal，剩下的随后被本测试自己的 `migrate()` 收尾时又被 EPQ 跳过 ——
/// 前提被污染，断言读到 `[0,0,1]` 或 `[("a_first",0),("m_mid",1),("z_last",1)]`。
///
/// 逐条 INSERT 时「部分可见」是**必然**的：3 条各占一个自动提交事务，
/// 第 1、2 条与第 3 条之间对别的会话就是一个真实窗口。
/// 共享集群上这个窗口不是理论上的 —— `action_types` 已有上万行 `ordinal = 0`，
/// 每个测试进程都要跑好几次 `migrate()`，回填的占空比很高。
///
/// 一条多行 INSERT = **一个隐式事务**：对别的会话要么 3 行全都不可见、
/// 要么全都可见。**「只看到一部分」这个前提本身被消掉了** ——
/// 这也是它能同时堵住上面两个签名的原因（两个签名都要求回填看到过子集）。
///
/// ⚠️ **行序必须与原来逐条插完全一致** —— 本函数存在的全部理由就是
/// 「插入顺序与 key 字典序相反」这个前提：调用点传的是
/// `["z_last", "m_mid", "a_first"]`，字典序恰好是 `["a_first", "m_mid", "z_last"]`。
/// 万一行序被写成字典序，确定性判据会**安静地失去判别力而测试仍然全绿** ——
/// 那比测试红更糟，所以这条约束写在这里、并且由下面的自检在**代码里**兜住。
///
/// 「自检断言的那份」与「`push_values` 收到的那份」必须**无法分离** ——
/// 详见函数体内 `rows` / `pushed` 两行的说明（REQ-040 快速修复轮）。
async fn insert_legacy_actions(pool: &PgPool, version_id: Uuid, keys: &[&str]) {
    // ⚠️ 前提自检 · 第 1 次（廉价早失败，主体 = 入参 `keys`）。
    assert_insert_order_is_not_dict_order(keys);

    // ══ REQ-040 快速修复轮：把「被断言的那份」与「被插入的那份」焊死 ══
    //
    // 修复前，上面那条自检断言的是**函数入参** `keys`，而真正决定物理行序的是
    // **`push_values` 收到的那份**。两者之间没有任何绑定 —— 中间插一句
    // `push_values(&dict_order, …)` 或 `push_values(&reversed, …)`，
    // 自检照样全绿，**物理行序却真的变成字典序了**（QA 的 M10/M11 实测存活 5/5）。
    //
    // 现在有两条绑带：
    // 1. `rows` 是喂给 `push_values` 的**唯一**切片（= 入参本身，不复制、不重排）；
    // 2. `pushed` 由 `push_values` 的闭包按**它实际迭代的顺序**逐个记下 key，
    //    插入后在 `execute` 之前对 `pushed` 跑**同一个**断言
    //    （`assert_insert_order_is_not_dict_order`）。
    //
    // 于是断言的对象**就是**被插入的那份数据本身，而不是一个「碰巧长得一样」
    // 的平行变量：想让自检与实际插入不一致，就必须**先改数据或改断言**，
    // 没有任何一条「换一份切片喂给 `push_values`」的捷径。
    let rows: &[&str] = keys;
    let mut pushed: Vec<&str> = Vec::with_capacity(rows.len());

    let mut qb = sqlx::QueryBuilder::new(
        "INSERT INTO action_types
           (id, version_id, key, target, parameters, preconditions,
            effects, permission, audit_policy, evidence, ordinal) ",
    );
    qb.push_values(rows, |mut row, k| {
        pushed.push(*k);
        row.push_bind(Uuid::new_v4())
            .push_bind(version_id)
            .push_bind(*k)
            .push_bind("customer")
            .push("'{}', '{}', '[]', '{}', '{}', '[]', 0");
    });

    // ⚠️ 前提自检 · 第 2 次（主体 = **`push_values` 实际迭代过的那份**）。
    // 位置在 `push_values` 之后、`execute` 之前：`push_values` 只拼 SQL、
    // **不发网络**，所以这里红 = 一行都没写进数据库。
    assert_insert_order_is_not_dict_order(&pushed);

    // ══ REQ-042 · AC-003：恒等断言（`assert_eq!`，不是性质断言） ══
    //
    // # 为什么上面那条**性质**断言不够
    //
    // `assert_insert_order_is_not_dict_order` 断言的是「`pushed ≠ 字典序`」——
    // 这是一条**性质**：满足它的排列有无穷多个。所以只要改动**仍然**落在
    // 「非字典序」这个集合里（哪怕是另一个非字典序的排列），它照样全绿。
    // QA 实测的存活变异 **M13c** 就是这种：把行序换成另一个非字典序的排列，
    // 判据看不见，而**物理行序真的变了**。
    //
    // # 这条恒等断言怎么补上那个洞
    //
    // 钉的是「`push_values` 实际迭代的顺序 **== 调用者传进来的顺序**」——
    // 这不再是性质，而是**唯一解**。M13c 换成的任何一个排列都 ≠ `keys`，
    // 于是必然红。
    //
    // ⚠️ 断言对象仍然是 `pushed`（**被真正迭代的那份**），不是入参的平行副本，
    // 所以 REQ-040 那两条绑带（`rows` 是唯一切片 / `pushed` 按实际迭代序记录）
    // 继续成立：想让自检与实际插入不一致，仍必须**先改数据或改这条断言**。
    //
    // 不改 SQL 字面量、不从拼好的 SQL 解析回来、不动 `push_values` 的构造方式。
    assert_eq!(
        pushed.as_slice(),
        keys,
        "`push_values` 实际迭代的顺序必须与传入的 `keys` **完全一致**（恒等，不是「非字典序」）—— \
         上面那条性质断言只排除了「恰好是字典序」这一个排列，换成**另一个**非字典序排列 \
         （QA 的 M13c）它照样全绿，而物理行序已经变了"
    );

    qb.build().execute(pool).await.unwrap();
}

async fn ordinals_of(pool: &PgPool, version_id: Uuid) -> Vec<(String, i32)> {
    sqlx::query_as("SELECT key, ordinal FROM action_types WHERE version_id = $1 ORDER BY key")
        .bind(version_id)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// REQ-012 · F1 回填的全部三条性质，**合并在一条测试里**。
///
/// # 为什么合并成一个而不是三条
///
/// 每个 `#[tokio::test]` 都会自建连接池并**各跑一次 `migrate()`**（建表 DDL
/// 要拿 AccessExclusive 锁）。测试二进制内的测试是**并行**的，于是 N 条回填测试
/// = N 份并发的 DDL 压力全砸在共享集群上。实测拆成四条时打出了
/// `40P01 deadlock detected`（两条 migrate 与彼此的普通读写互等）。
/// 三条性质共用同一批版本，串在一条里跑，连接与迁移次数都降到 1/3。
///
/// 三个版本分别覆盖：
/// - `v_legacy`  —— 全是 DEFAULT 0 的历史行，插进去的顺序**故意与 key 字典序相反**
/// - `v_written` —— 已有非 0 的真实序（且故意不连续）
/// - 重复 migrate —— 幂等性
#[tokio::test]
async fn test_backfill_is_deterministic_preserves_and_idempotent() {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    projects.migrate().await.unwrap();
    let project = common::fresh_project(&projects, "req012fill").await;

    let v_legacy = projects.new_version(project.id).await.unwrap().id;
    let v_written = projects.new_version(project.id).await.unwrap().id;

    // 历史行：插入顺序 z/m/a 与 key 字典序 a/m/z **相反** ——
    // 若回填依赖物理行序，z_last 会拿到 0，这条红。
    insert_legacy_actions(&pool, v_legacy, &["z_last", "m_mid", "a_first"]).await;

    // 前提：`v_legacy` 的历史行此刻确实还全是 DEFAULT 0。
    //
    // ⚠️ REQ-040 · **必须紧贴上面那次 INSERT 读**。
    // 原代码把这条放在 `v_written` 那两次 INSERT 之后 —— 断言的不是
    // 「刚插入的那一刻」，而是「隔了 3 个网络来回之后」。而回填是无条件、
    // 全局执行的，插入提交到这条读取之间每多一个来回，就多一次
    // 「被并发回填整批改掉」的机会（共享集群 `SELECT 1` p50 ≈ 157 ms）。
    // **断言本身一个字没改**（表达式、期望值、消息都照旧），
    // 只是回到它该在的位置 —— 它断言的就是紧邻其上的那次 INSERT。
    //
    // 读侧自带 `WHERE version_id = $1`（见 `ordinals_of`），所以 `v_written`
    // 的非 0 真实序不受并发回填影响（回填子查询只取 `ordinal = 0`），
    // 本来就不需要为它加前提断言 —— 原注释「两个版本」是笔误，这里一并订正。
    assert_eq!(
        ordinals_of(&pool, v_legacy).await.iter().map(|(_, o)| *o).collect::<Vec<_>>(),
        vec![0, 0, 0],
        "前提：迁移前历史行应当全是 DEFAULT 0"
    );

    // 正常写入产生的真实序（5 / 9 —— 故意不连续，看有没有被压平）
    for (k, o) in [("already_written", 5), ("also_written", 9)] {
        sqlx::query(
            "INSERT INTO action_types
               (id, version_id, key, target, parameters, preconditions,
                effects, permission, audit_policy, evidence, ordinal)
             VALUES ($1,$2,$3,'customer','{}','{}','[]','{}','{}','[]',$4)",
        )
        .bind(Uuid::new_v4())
        .bind(v_written)
        .bind(k)
        .bind(o)
        .execute(&pool)
        .await
        .unwrap();
    }

    // 跑迁移（内含回填 UPDATE）
    common::migrate(&pool).await;

    // ── ① 确定性：按 key 字典序，不是物理行序 ──
    assert_eq!(
        ordinals_of(&pool, v_legacy).await,
        vec![
            ("a_first".to_string(), 0),
            ("m_mid".to_string(), 1),
            ("z_last".to_string(), 2),
        ],
        "回填必须按 key 字典序（确定性、可复现），不是物理行序"
    );

    // ── ② 不覆盖已有非 0 的真实序 ──
    assert_eq!(
        ordinals_of(&pool, v_written).await,
        vec![
            ("already_written".to_string(), 5),
            ("also_written".to_string(), 9),
        ],
        "已有非 0 的 ordinal 是正常写入产生的真实序，回填必须原样保留"
    );

    // ── ③ 幂等：迁移会跑很多遍（每个测试进程都跑） ──
    let first_legacy = ordinals_of(&pool, v_legacy).await;
    let first_written = ordinals_of(&pool, v_written).await;
    for _ in 0..3 {
        common::migrate(&pool).await;
    }
    assert_eq!(
        first_legacy,
        ordinals_of(&pool, v_legacy).await,
        "回填必须幂等：再跑三次迁移，历史行的 ordinal 一行都不许变"
    );
    assert_eq!(
        first_written,
        ordinals_of(&pool, v_written).await,
        "回填必须幂等：再跑三次迁移，已有真实序一行都不许变"
    );

    // ── ④ 判定条件本身可见：同一版本内不许有并列的 ordinal ──
    // 读侧 `load_actions` 是 `ORDER BY ordinal` —— 键并列就等于
    // 「顺序交给优化器决定」，那正是 REQ-011 解决了一半的问题。
    let tied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
           SELECT version_id, ordinal FROM action_types
           WHERE version_id = ANY($1)
           GROUP BY version_id, ordinal HAVING count(*) > 1) x",
    )
    .bind(&[v_legacy, v_written])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tied, 0, "这两个版本里仍有 {tied} 组「同版本内 ordinal 并列」");

    let zero_tied: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
           SELECT version_id FROM action_types WHERE ordinal = 0 AND version_id = ANY($1)
           GROUP BY version_id HAVING count(*) > 1) x",
    )
    .bind(&[v_legacy, v_written])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        zero_tied, 0,
        "回填后仍有版本内并列 ordinal = 0 —— 回填没盖住，读侧 ORDER BY ordinal 在这些版本上仍是假的"
    );
}

// ══════════════════════════════════════════════════════════════
// REQ-038 · `set_extracted_by` 的冻结闸门 + 闸门/删旧行的顺序断言
// ══════════════════════════════════════════════════════════════

/// 取 `src/db/snapshot_store.rs` 的行数组。
fn snapshot_store_lines() -> Vec<String> {
    read_source("src/db/snapshot_store.rs")
        .lines()
        .map(str::to_string)
        .collect()
}

/// 定位一个**方法体**的行区间 `[start, end)`。
///
/// 为什么要按函数切区间：`save_snapshot` 与 `set_extracted_by` 里**都**有
/// `SELECT frozen_at IS NOT NULL ...` 这行。若在全文件里找第一次出现，
/// 换个方法顺序就会指向另一个函数 ⇒ 判据变成随机数。
/// 区间右界取「下一个同缩进的新方法声明」；取不到就取到文件末尾。
fn method_span(lines: &[String], decl: &str) -> (usize, usize) {
    let start = lines
        .iter()
        .position(|l| l.contains(decl))
        .unwrap_or_else(|| panic!("snapshot_store.rs 里找不到方法声明 {decl:?} —— 判据锚点没了"));
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(start + 1) {
        let is_new_fn = l.starts_with("    pub async fn ")
            || l.starts_with("    pub fn ")
            || l.starts_with("    async fn ")
            || l.starts_with("    fn ")
            || l.starts_with("    // ──");
        if is_new_fn {
            end = i;
            break;
        }
    }
    (start, end)
}

/// 在 `[start, end)` 行区间内找第一处含 `needle` 的**行号**，找不到就 panic。
fn line_of(lines: &[String], start: usize, end: usize, needle: &str, in_who: &str) -> usize {
    lines[start..end]
        .iter()
        .position(|l| l.contains(needle))
        .map(|i| start + i)
        .unwrap_or_else(|| {
            panic!("{in_who} 的方法体（第 {}..{} 行）里找不到 {needle:?} —— 判据锚点没了",
                start + 1, end)
        })
}

// ── 顺序断言：冻结闸门必须早于「删旧行」 ──────────────────

/// REQ-038 · `save_snapshot` 里「冻结闸门」的文本行号 **必须小于**
/// `delete_snapshot_rows` 的调用行号。
///
/// # 为什么要结构期判据钉顺序（**实测结论，不是推断**）
///
/// 既有的运行期判据 `test_freeze_gate_precedes_delete_and_delete_covers_five_tables`
/// **抓不住顺序被调换**。REQ-038 实测（M3 变异：把 `delete_snapshot_rows`
/// 提到 `begin()` 之后、闸门之前）：
///   · 该运行期判据**单跑 3 次全部 ok** —— 存活；
///   · 本结构期判据 **2/2 红**，报「冻结闸门在第 44 行，删旧行在第 40 行」。
///
/// 原因：闸门挪到 DELETE 之后时，`Some(true)` 分支 `return Err` 而不 `commit`，
/// 抢跑的 DELETE 随事务**一起回滚**，五张表旧行原样还在 ⇒ 运行期看到的
/// 「什么都没发生」与正确实现**完全一样**。回滚把次序问题掩盖了。
/// 这层掩盖依赖两件事同时成立：① 错误路径不 `commit`；
/// ② 闸门与 DELETE 共用同一个 `tx`。任何一条被改（给错误路径补一次
/// `tx.commit()`，或把 DELETE 挪出事务），回滚这层保护立刻失效、
/// 顺序被调换就变成「在冻结版本上删数据」的真缺陷，且**无人报警**。
/// 顺序是**源码结构事实**，用行号钉住，成本一行 `assert!`。
#[test]
fn test_freeze_gate_precedes_delete_call_in_source_order() {
    let lines = snapshot_store_lines();
    let (s, e) = method_span(&lines, "pub async fn save_snapshot");

    let gate = line_of(
        &lines,
        s,
        e,
        "SELECT frozen_at IS NOT NULL FROM ontology_versions WHERE id = $1",
        "save_snapshot",
    );
    let del = line_of(&lines, s, e, "delete_snapshot_rows(&mut tx", "save_snapshot");

    assert!(
        gate < del,
        "冻结闸门在第 {} 行，「删旧行」调用在第 {} 行 —— \
         闸门必须**早于** DELETE：闸门一旦落在它之后，就等于在冻结版本上删数据",
        gate + 1,
        del + 1
    );
}

/// REQ-038 · `set_extracted_by` 的闸门与写入必须**同处一个事务**。
///
/// # 这条挡的是 TOCTOU，不是「有没有闸门」
///
/// 运行期断言（`test_set_extracted_by_rejects_frozen_version`）在**单线程**下
/// 分不出两种写法：
///   · 事务内查 + 事务内写（正确）
///   · pool 上先查 + pool 上后写（有闸门，但查与写之间有窗口）
/// 两者在没有任何并发插入时返回完全一样的结果。
///
/// 语义差别是实的：后者把「查」与「写」拆成两个**自动提交**语句，
/// 并发 `freeze()` 可以落在这两者之间 ⇒ 「查时未冻结 / 写时已冻结」照样写进去。
/// `save_snapshot` 的闸门从一开始就在事务内（见其 ① 步注释「避免 TOCTOU」），
/// 本函数不得是同类型上的第二种写法。
#[test]
fn test_set_extracted_by_gate_and_write_share_one_transaction() {
    let lines = snapshot_store_lines();
    let (s, e) = method_span(&lines, "pub async fn set_extracted_by");
    let body: String = lines[s..e].join("\n");

    assert!(
        body.contains("self.pool.begin()"),
        "`set_extracted_by` 里没有开事务 —— 闸门与写入之间的 TOCTOU 窗口是真实的"
    );
    // 闸门查在事务上；UPDATE 也必须在事务上。
    assert!(
        body.contains(".fetch_optional(&mut *tx)"),
        "冻结闸门没有在事务内查 —— 与 `save_snapshot` 的形状不一致"
    );
    assert!(
        body.contains(".execute(&mut *tx)"),
        "版本行 UPDATE 没有在事务内执行 —— 「查在事务里、写在事务外」是半拉子，窗口照旧"
    );
    assert!(
        !body.contains("&self.pool"),
        "`set_extracted_by` 里仍有一处直接打在 `&self.pool` 上 —— \
         闸门已放行但写入不在同一事务，等于没关掉窗口"
    );
    assert!(
        body.contains("tx.commit()"),
        "事务开了却从不 commit —— 写入根本不会落库（应当回滚）"
    );
}

