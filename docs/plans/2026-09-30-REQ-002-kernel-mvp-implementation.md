# REQ-002 本体内核 MVP · 实施计划（Rust）

> **逐步 TDD 文档。** 范围见 [`2026-09-30-ontology-kernel-mvp.md`](2026-09-30-ontology-kernel-mvp.md)；
> 需求与验收见 [`demands/REQ-002-kernel-mvp/`](../demands/REQ-002-kernel-mvp/demand.md)。
>
> **工作区**：`feature-REQ-002/`（从 `develop` 切出，基目录为仓库根）
> **提交格式**：`[Dev] {描述} (关联: REQ-002) [session: {session-id}]`

---

## 环境

| 项 | 值 |
| --- | --- |
| 工具链 | Rust 1.86（`cargo 1.86.0`） |
| 运行时 | tokio（`#[tokio::test]`） |
| DB | PostgreSQL 16.14（外部测试库） |
| 测试库 | **外部 PG**（不用 testcontainers） |
| 迁移 | 无，用 `create_all` |

```bash
export ONTOK_TEST_DATABASE_URL="postgresql://postgres:***@172.16.120.20:30432/ontol"
```

> **已确认（2026-09-30）**：PostgreSQL 16.14，空库，连通正常。
> **不用 testcontainers** —— 本机 Docker daemon 未起，且外部库更省事。
> ⚠ 测试库由开发方提供，**不得写入任何非测试数据**；库名 `ontol` 即专用测试库。

> **不使用 `sqlx::query!` 宏** —— 编译期校验需 DB 在线或维护 `.sqlx` 目录。
> 本轮 spike 已吃过一次「凭空编系统里没有的列」的亏，那是**逻辑错**；宏防的是**类型错**，
> MVP 不值得为它付构建期复杂度。

---

## Task 1 · 骨架与连通

**验收**：AC-001（部分）

**Files**
```
Create: Cargo.toml
Create: src/main.rs
Create: src/lib.rs
Create: src/config.rs
Create: src/db/mod.rs
Create: src/db/pool.rs
Create: tests/common/mod.rs
Create: .gitignore
```

### Step 1 · 初始化 crate

```bash
cd <repo-root>/feature-REQ-002
cargo init --name ontok --lib
```

`Cargo.toml`：

```toml
[package]
name = "ontok"
version = "0.1.0"
edition = "2021"

[lib]
name = "ontok"
path = "src/lib.rs"

[[bin]]
name = "ontok"
path = "src/main.rs"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "uuid", "chrono", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"
anyhow = "1"
clap = { version = "4", features = ["derive"] }

[dev-dependencies]
# 无 —— 用外部测试库，不引入 testcontainers
```

`.gitignore`：
```
/target
.env
```

### Step 2 · 写失败的连通性测试

Create `tests/test_db.rs`：

```rust
use ontok::db::pool::PgPool;

#[tokio::test]
async fn pg_pool_connects() {
    let pool = PgPool::connect_from_env("ONTOK_TEST_DATABASE_URL")
        .await
        .expect("连不上测试库");
    let n: i64 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("查询失败");
    assert_eq!(n, 1);
}
```

### Step 3 · 实现连接池

Create `src/db/pool.rs`：

```rust
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("连不上数据库：{0}")]
    Connect(#[from] sqlx::Error),
    #[error("缺少环境变量 {0}")]
    MissingEnv(&'static str),
}

impl PgPool {
    pub async fn connect_from_env(key: &str) -> Result<Self, DbError> {
        let url = std::env::var(key).map_err(|_| DbError::MissingEnv(key))?;
        Ok(PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&url)
            .await?)
    }
}
```

`src/lib.rs`：`pub mod config; pub mod db;`
`src/main.rs`：`fn main() {}`（先空着，Task 10 填 CLI）

### Step 4 · 跑

```bash
export ONTOK_TEST_DATABASE_URL="postgresql://postgres:***@172.16.120.20:30432/ontol"
cargo test --test test_db
```
预期：PASS

### Step 5 · 测试库 fixture

Create `tests/common/mod.rs`（其余测试 `mod common;` 复用）：

```rust
use sqlx::postgres::PgPool;
use sqlx::PgPoolOptions;

/// 外部测试库（PG 16.14）。进程内共享一个连接池。
///
/// 不用 testcontainers：本机 Docker daemon 未起，且外部库省去容器生命周期管理。
/// **测试库由开发方提供，不得写入非测试数据。**
pub async fn test_pool() -> &'static PgPool {
    use std::sync::OnceLock;
    static POOL: OnceLock<PgPool> = OnceLock::new();
    POOL.get_or_init(|| async {
        let url = std::env::var("ONTOK_TEST_DATABASE_URL")
            .expect("缺 ONTOK_TEST_DATABASE_URL");
        PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&url)
            .await
            .expect("连不上测试库")
    })
    .await
}
```

> **每个测试文件开头**应 `use crate::common::test_pool;`。
> **测试间数据隔离**：各测试用**独立的 project name**（`t_acme_1`、`t_acme_2`…）避免串数据。
> 不做 truncate —— 外部库多人共用，truncate 会打断别人的运行。

### Step 6 · Commit

```bash
git add -A
git commit -m "[Dev] Task1 骨架与 PG 连通 (关联: REQ-002) [session: $SID]"
```

---

## Task 2 · Project + OntologyVersion

**验收**：AC-001

**Files**：`src/db/project.rs` · `src/db/mod.rs` · `tests/test_project.rs`

### Step 1 · 失败测试

```rust
use ontok::db::project::{NewProject, PgProjectStore};
use crate::common::test_pool;

#[tokio::test]
async fn create_project_and_versions_are_isolated() {
    let pool = test_pool().await;
    let store = PgProjectStore::new(pool.clone());
    store.migrate().await.unwrap();

    let a = store.create(NewProject { name: "acme" }).await.unwrap();
    let b = store.create(NewProject { name: "globex" }).await.unwrap();
    assert_ne!(a.id, b.id);

    let v = store.new_version(a.id).await.unwrap();
    assert_eq!(v.seq, 1, "首版 seq 从 1 开始");
    assert!(v.frozen_at.is_none());

    // 隔离：B 项目看不到 A 的版本
    let all = store.list_versions(a.id).await.unwrap();
    assert_eq!(all.len(), 1);
}
```

### Step 2 · 模型与实现

```rust
pub struct ProjectRow { pub id: Uuid, pub name: String, pub created_at: DateTime<Utc> }
pub struct VersionRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub seq: i32,
    pub frozen_at: Option<DateTime<Utc>>,   // ← 三态之一
    pub created_at: DateTime<Utc>,
}
```

> ⚠ **`frozen_at` 是 `Option`**，与 `created_at` 的非空语义不同。
> 这个 null 语义是 AC-010 快照往返测试要守住的第一个坑。

`new_version` 的 seq 取 `max(seq) + 1`，在同一事务里做。

### Step 3 · 跑

```bash
cargo test --test test_project
```

### Step 4 · Commit

```bash
git commit -m "[Dev] Task2 Project + OntologyVersion (关联: REQ-002) [session: $SID]"
```

---

## Task 3 · 本体 schema 模型（含占位）

**验收**：AC-003 的一半

**Files**：`src/db/ontology.rs` · `src/db/proposal.rs`（Task 5 再扩）· `tests/test_ontology.rs`

### Step 1 · 失败测试（重点是占位字段在）

```rust
#[tokio::test]
async fn action_type_and_function_are_persistable() {
    let pool = test_pool().await;
    let store = PgOntologyStore::new(pool.clone());
    store.migrate().await.unwrap();

    let a = store.add_action_type(ActionTypeRow {
        key: "dispatch_ticket".into(),
        target: "工单".into(),
        parameters: json!([{"name": "ticket_id", "type": "string"}]),
        preconditions: json!([]),
        effects: json!([{
            "system": "erp", "table": "t_ticket", "field": "status",
            "key_expr": {"col": "ticket_id"},
            "value_map": {"dispatched": "DISPATCHED"},
            "supported_values": ["dispatched"],
            "conflict_field": "row_version",
            "evidence": [{"table": "t_ticket", "column": "status", "claim": "工单状态列"}]
        }]),
        permission: json!({"roles": ["planner"]}),
        audit_policy: json!({"retain_days": 365}),
        evidence: json!([]),
    }).await.unwrap();

    assert_eq!(a.key, "dispatch_ticket");
    let back = store.get_action_type(a.id).await.unwrap();
    assert_eq!(back.effects[0]["conflict_field"], "row_version");
}
```

### Step 2 · 模型

```rust
pub struct ObjectTypeRow {
    pub id: Uuid, pub version_id: Uuid,
    pub key: String, pub display_name: String,
    pub description: Option<String>,
    pub aliases: Value,              // []  ← 占位
    pub parent_id: Option<Uuid>,     // 层级 ← 占位
    pub disjoint_with: Value,        // []  ← 占位
    pub evidence: Value,
    pub ordinal: i32,
}
pub struct LinkTypeRow {
    pub id: Uuid, pub version_id: Uuid,
    pub key: String,
    pub from_type: String, pub to_type: String,
    pub cardinality: String,         // "1:1" | "1:N" | "N:M"
    pub semantic: String,            // has | belongs_to | depends_on | …
    pub evidence: Value, pub ordinal: i32,
}
pub struct ActionTypeRow { /* 见上，含 effects: Value */ }
pub struct FunctionRow {
    pub id: Uuid, pub version_id: Uuid,
    pub key: String, pub signature: String,
    pub allowed_types: Value, pub evidence: Value, pub ordinal: i32,
}
```

**约束**：`UNIQUE(version_id, key)` —— 三张表都要。
`parent_id` 的 `ON DELETE SET NULL`（不是 CASCADE）：删父类型不应连带删子，子类型应被"提根"。

### Step 3 · 跑 + Commit

```bash
cargo test --test test_ontology
git commit -m "[Dev] Task3 本体 schema + ActionType/Function 占位 (关联: REQ-002) [session: $SID]"
```

---

## Task 4 · 快照契约 ★ 本 MVP 最重要的一个 Task

**验收**：AC-003 · AC-010 · AC-011

**Files**：`src/domain/snapshot.rs` · `src/store/mod.rs` · `src/store/pg.rs` · `tests/test_snapshot.rs`

### Step 1 · 先写往返测试（**先于**实现）

```rust
#[tokio::test]
async fn test_snapshot_roundtrip() {
    let pool = test_pool().await;
    let store = PgStore::new(pool.clone());
    store.migrate().await.unwrap();
    let version = /* 新建版本 */;

    let snap = Snapshot {
        version_id: version.id,
        seq: 1,
        extracted_by: Some(ExtractedBy {   // ← Option：人工修订记 None（ADR-001）
            agent: "vendor-extractor".into(),
            version: "2.3.1".into(),      // 必须记具体版本
            round: 3,
            failed_groups: vec!["t_foo".into()],
        }),
        entity_types: vec![/* 含 aliases / parent / disjoint_with / None 值 */],
        link_types: vec![/* … */],
        action_types: vec![/* 切片 1 通常为空，但不能是「结构缺失」 */],
        functions: vec![],
    };

    store.save_snapshot(&snap).await.unwrap();
    let back = store.get_snapshot(version.id).await.unwrap();
    assert_eq!(back, snap, "快照必须逐字段无损往返");
}
```

> **三态是这里的主要坑**：`None` / `[]` / `{}` 在 serde + sqlx 的组合下容易互相吞掉。
> 测试里**故意**放一个 `description: None`、一个空 `aliases`、一个空 `disjoint_with`，
> 往返后必须仍然是 `None` / `[]` / `[]`，不能都变成 `null` 或都变成空对象。
>
> **`extracted_by` 本身也是可空项**（ADR-001：人工修订记 `None`）——
> 往返测试必须覆盖「Some → 存 → 取出仍是 Some」与「None → 存 → 取出仍是 None」两种。

### Step 2 · 定义契约

```rust
// src/domain/snapshot.rs —— 本层零 I/O，验收 AC-002 守住

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version_id: Uuid,
    pub seq: i32,
    /// 可空：None = 人工修订（ADR-001）。取值由提案带入，**不继承 base**
    pub extracted_by: Option<ExtractedBy>,  // ← 规格 §8.4
    pub entity_types: Vec<EntityType>,
    pub link_types: Vec<LinkType>,
    pub action_types: Vec<ActionType>,      // 切片 1 恒空，但结构必须在
    pub functions: Vec<Function>,           // 同上
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedBy {
    pub agent: String,
    pub version: String,     // 具体版本，不是 "latest"
    pub round: i32,
    pub failed_groups: Vec<String>,   // 不参与收敛判定
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityType {
    pub key: String,
    pub display_name: String,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub parent: Option<String>,        // 父的 key（不是 id）—— 快照要可读可 diff
    pub disjoint_with: Vec<String>,
    pub properties: Vec<Property>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub table: String,
    pub column: String,
    pub claim: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionType {
    pub key: String,
    pub target: String,
    pub parameters: Value,
    pub preconditions: Value,
    pub effects: Vec<WriteBinding>,     // 规格 §5.2
    pub permission: Value,
    pub audit_policy: Value,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteBinding {
    pub system: String,
    pub table: String,
    pub field: String,
    pub key_expr: Value,
    pub value_map: BTreeMap<String, String>,
    pub supported_values: Vec<String>,   // 能为空 —— 「不支持」是有效表达
    pub conflict_field: Option<String>,  // 乐观锁字段
    pub evidence: Vec<Evidence>,
}
```

> **`parent` 存父的 `key` 而不是 `id`** —— 快照是给人看和给 diff 用的，
> 存 id 看不出「父从机器人设备变成了设备」。这是规格 §4.6 的隐含决定，在此显式化。

### Step 3 · PgStore 存取

MVP 只有 PG 一个实现，**不写 trait**（范围 §3 暂缓项）。

`save_snapshot` 需在**单事务**内：删旧行 → 写新行 → 更新版本行。
否则中途失败会留下半截快照。

### Step 4 · 跑

```bash
cargo test --test test_snapshot
```

### Step 5 · Commit

```bash
git commit -m "[Dev] Task4 快照契约 —— 含 extracted_by 与全部占位，往返无损 (关联: REQ-002) [session: $SID]"
```

---

## Task 5 · 提案模型

**验收**：AC-001

**Files**：`src/db/proposal.rs` · `tests/test_proposal.rs`

```rust
pub struct ProposalRow {
    pub id: Uuid, pub project_id: Uuid,
    pub base_version_id: Uuid,        // 提案基于哪一版
    pub round: i32,
    pub status: String,                // pending | applied | rejected
    pub created_at: DateTime<Utc>,
}
pub struct ProposalItemRow {
    pub id: Uuid, pub proposal_id: Uuid,
    pub kind: String,                  // add_entity_type | add_property | add_link_type | …
    pub target_key: String,
    pub payload: Value,                // 完整候选定义（含 evidence）
    pub biz_status: String,            // pending | accepted | rejected
    pub model_status: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at: Option<DateTime<Utc>>,
}
```

**关键约束**：`add_link_type` 类的 item，`payload` 里**必须**带 `from_type` / `to_type`，
且这两个 key 必须存在于本提案或基线版本中（Task 6 校验）。

### Commit

```bash
git commit -m "[Dev] Task5 提案模型 (关联: REQ-002) [session: $SID]"
```

---

## Task 6 · 双轨审核状态机（纯逻辑）

**验收**：AC-007

**Files**：`src/domain/review.rs` · `tests/test_review_state.rs`

> **本 Task 零 I/O。** 全部输入是内存对象，`src/domain/` 里不出现 `sqlx`。

### Step 1 · 失败测试

```rust
#[test]
fn test_link_acceptance_requires_both_ends_accepted() {
    let items = vec![
        item("add_entity_type", "客户", Track::Biz, Decision::Accepted),
        item("add_entity_type", "订单", Track::Biz, Decision::Pending),
        item("add_link_type", "客户—订单", Track::Biz, Decision::Accepted),
    ];
    // 订单尚未接受 → 关系必须被拦下
    let st = summarize(&items);
    assert_eq!(st.applicable, vec!["add_entity_type:客户"]);
    assert!(st.blocked.iter().any(|b| b.target == "客户—订单"));
}

#[test]
fn test_two_tracks_judge_independently() {
    let items = vec![
        item("add_entity_type", "客户", Track::Biz, Decision::Accepted),
        item("add_entity_type", "客户", Track::Model, Decision::Rejected),
    ];
    let st = summarize(&items);
    // 建模轨拒绝 → 整体不通过；业务轨通过不能覆盖
    assert!(!st.passed);
}
```

### Step 2 · 状态机

```rust
pub enum Track { Biz, Model }
pub enum Decision { Pending, Accepted, Rejected }

pub struct ItemView {
    pub kind: String,
    pub target: String,
    pub payload: Value,
    pub biz_status: Decision,
    pub model_status: Decision,
}

pub struct ProposalStatus {
    pub passed: bool,
    pub applicable: Vec<String>,     // 可应用的条目 id
    pub blocked: Vec<Blocked>,      // {target, reason}
}

/// 汇总：双轨独立 + 关系依赖两端。
pub fn summarize(items: &[ItemView]) -> ProposalStatus { /* … */ }
```

**`passed` 的定义**：`biz_status == Accepted && model_status == Accepted`（生产阶段），
或 MVP 允许 `model_status ∈ {Accepted, Pending}` 且 `biz_status == Accepted`（POC 代签，规格 §8.2）。
**但建模轨显式 Rejected 永远否决。**

### Commit

```bash
git commit -m "[Dev] Task6 双轨审核状态机 (关联: REQ-002) [session: $SID]"
```

---

## Task 7 · 客户视图投影（纯逻辑）

**验收**：AC-005 · AC-013

**Files**：`src/domain/view.rs` · `tests/test_view.rs`

```rust
#[test]
fn test_biz_view_never_exposes_modeling_terms() {
    let snap = demo_snapshot();
    let v = biz_view(&snap);
    let s = serde_json::to_string(&v).unwrap();
    for w in ["pk", "primary_key", "cardinality", "外键", "基数", "ordinal", "fk"] {
        assert!(!s.contains(w), "客户视图泄露了建模术语：{w}");
    }
}

#[test]
fn biz_view_omits_reserved_fields() {
    let snap = demo_snapshot();
    let v = biz_view(&snap);
    // 占位字段不得出现在客户视图里
    assert!(v.get("action_types").is_none());
    assert!(v.get("extracted_by").is_none());
}
```

**投影规则**：
- 输出 `名称` / `说明` / `属性（中文名 + 样例值）` / `属于哪个上层对象`
- **不输出**：`key`（那是系统标识，客户不需要）、`evidence`（审计用，不是给人看的）、
  `ordinal`、`extracted_by`、所有 ActionType/Function 占位
- **不输出外键列名**（`customer_id` 这类），只输出它指向的对象名

### Commit

```bash
git commit -m "[Dev] Task7 客户视图投影，零建模术语 (关联: REQ-002) [session: $SID]"
```

---

## Task 8 · 提案应用 + 冻结

**验收**：AC-006 · AC-008

**Files**：`src/domain/apply.rs` · `src/store/pg.rs`（写路径）· `tests/test_apply.rs`

### Step 1 · 失败测试

```rust
#[tokio::test]
async fn test_frozen_version_rejects_writes() {
    // 冻结 v1 后，任何 save_snapshot 到 v1 必须报错
    let r = store.save_snapshot(&snap_on_v1).await;
    assert!(matches!(r, Err(StoreError::VersionFrozen { .. })));
}

#[tokio::test]
async fn test_chained_apply_keeps_accumulating() {
    // v1 加「客户」→ v2 加「订单」→ v3 应同时含两者，不是只含「订单」
    let v3 = ...;
    assert!(v3.has("客户") && v3.has("订单"));
}
```

### Step 2 · apply

```
apply(proposal, base_version) -> new_version
  1. 校验 base_version 未冻结，且其 seq == proposal.base_version.seq
  2. 载入 base_version 快照
  3. 逐条应用 applicable item：
       add_entity_type  → 追加（key 冲突则报错）
       add_property     → 挂到目标类型
       add_link_type    → 追加，且两端 key 必须存在
  4. 在单事务内写 new_version（新 seq = base.seq + 1）
  5. proposal.status = applied
```

#### `extracted_by` 的取值（ADR-001）

> **新版本原样取提案的 `extracted_by`；提案没有则记 `None`（人工修订）。**
> **不继承 base 的值，不推断，不按场景分支。**

理由：继承会让「agent 偷偷换版本」不可见 —— 而那正是 §8.4 要防的问题。
详见 `BA/decisions/ADR-001-extracted-by-policy.md`。

**不做**：不自动冻结（冻结是显式动作，§9.1 的「冻结是判据」）。

### Commit

```bash
git commit -m "[Dev] Task8 提案应用与版本冻结 (关联: REQ-002) [session: $SID]"
```

---

## Task 9 · 快照 diff

**验收**：AC-001（部分）

**Files**：`src/domain/diff.rs` · `tests/test_diff.rs`

> **纯逻辑，零 I/O。**

```rust
pub enum DiffKind { Added, Removed, Modified, Hierarchy }

pub struct DiffItem {
    pub kind: String,        // add_entity_type | remove_entity_type | …
    pub target: String,
    pub field: Option<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

pub fn diff_snapshots(before: &Snapshot, after: &Snapshot) -> Vec<DiffItem>
```

**MVP 只做四类**：
- `Added` / `Removed` —— 按 `key` 比对
- `Modified` —— 同 key 的标量字段变了（`display_name` / `description` / `cardinality` / `semantic`）
- `Hierarchy` —— `parent` 变了。**先当普通 Modified 处理**（影响面分析属暂缓项）

**必须比较的属性**：跳过 `evidence`（证据每次抽取都会变，不是本体演进）。

### Commit

```bash
git commit -m "[Dev] Task9 快照 diff 引擎 (关联: REQ-002) [session: $SID]"
```

---

## Task 10 · CLI 端到端

**验收**：AC-012 · AC-013

**Files**：`src/main.rs` · `src/cli.rs` · `src/commands/*.rs` · `fixtures/proposal-demo.json`

### Step 1 · 先写 demo 提案

`fixtures/proposal-demo.json`（示意结构，**在实现时补全**）：

```json
{
  "project": "acme",
  "base_version": 1,
  "round": 1,
  "extracted_by": { "agent": "demo", "version": "0.1.0", "round": 1, "failed_groups": [] },
  "items": [
    { "kind": "add_entity_type", "target": "客户", "payload": {
        "key": "customer", "display_name": "客户",
        "properties": [
          { "name": "客户编号", "column": "customer_no", "nullable": false },
          { "name": "客户名称", "column": "customer_name", "nullable": true }
        ],
        "evidence": [{ "table": "t025", "column": "customer_no", "claim": "客户主键" }]
    }},
    { "kind": "add_entity_type", "target": "订阅", "payload": {
        "key": "subscription", "display_name": "客户订阅",
        "properties": [
          { "name": "订阅编号", "column": "subscription_id", "nullable": false },
          { "name": "客户编号", "column": "customer_id", "nullable": false }
        ],
        "evidence": [{ "table": "t023", "column": "customer_id", "claim": "订阅挂客户" }]
    }},
    { "kind": "add_link_type", "target": "客户—订阅", "payload": {
        "key": "customer_has_subscription",
        "from_type": "customer", "to_type": "subscription",
        "cardinality": "1:N", "semantic": "has",
        "evidence": [{ "table": "t023", "column": "customer_id", "claim": "订阅挂客户" }]
    }}
  ]
}
```

### Step 2 · clap 子命令

```
ontok project new --name <NAME>
ontok version new  --project <NAME>
ontok propose import --file <PATH>
ontok review decide  --proposal <ID> --item <N> --track biz|model --accept|--reject
ontok review show    --proposal <ID>
ontok proposal apply  --proposal <ID>            # 提案进入版本，产生新版本
ontok version freeze --version <SEQ>
ontok ontology show  --version <SEQ>        # 默认客户视图
ontok ontology diff  <SEQ_A> <SEQ_B>
```

### Step 3 · 跑通 AC-012 那 8 条命令

### Step 4 · Commit

```bash
git commit -m "[Dev] Task10 CLI 端到端 (关联: REQ-002) [session: $SID]"
```

---

## Task 11 · 层级纯函数

**验收**：AC-009

**Files**：`src/domain/hierarchy.rs` · `tests/test_hierarchy.rs`

> **纯逻辑，零 I/O。**

```rust
pub fn ancestors_of(key: &str, parents: &BTreeMap<String, Option<String>>) -> Vec<String>;
pub fn descendants_of(key: &str, parents: &BTreeMap<String, Option<String>>) -> Vec<String>;
pub fn validate_hierarchy(parents: &BTreeMap<String, Option<String>>) -> Vec<String>;  // 问题清单
pub fn would_create_cycle(parents: &BTreeMap<String, Option<String>>,
                          candidate: &str, new_parent: Option<&str>) -> bool;
```

**必测**：
- 多级祖先顺序（离自己最近的在前）
- **环必须被检出**（`A→B→A`），且遍历不得死循环
- 悬空父引用（父不存在）必须被报出
- 祖先**不含自身**

> 这套纯函数在 Python spike 里已实测过（`spike/synthesize.py` 里的同名逻辑），
> 可作为行为参照。

### Commit

```bash
git commit -m "[Dev] Task11 层级纯函数 (关联: REQ-002) [session: $SID]"
```

---

## 完成标准

```bash
# AC-001
cargo test

# AC-002 分层纪律
grep -rn 'sqlx\|PgStore\|db::' src/domain/ | grep -vE '\s*//' \
  && echo '✗ 违规' || echo '✓'

# AC-003 占位齐全
for f in action_types functions write_bindings extracted_by failed_groups disjoint_with; do
  grep -q "$f" src/domain/snapshot.rs || echo "✗ 缺 $f"
done

# AC-004 / AC-005 / AC-006 / AC-007 / AC-010 / AC-011
cargo test --test test_no_write_path -- test_no_write_path_in_slice1
cargo test --test test_view -- test_view_biz_view_never_exposes_modeling_terms
cargo test --test test_apply -- test_frozen_version_rejects_writes
cargo test --test test_review_state -- test_review_link_acceptance_requires_both_ends_accepted
cargo test --test test_snapshot -- test_snapshot_roundtrip
cargo test --test test_snapshot -- test_snapshot_records_extracted_by

# AC-012 端到端（人工）
# AC-013 客户视图可读（人工）
```

---

## 交接

完成本计划后，产出物是**可执行的 `ontok` 二进制**。下一份计划（抽取侧 Rust 化）将消费：
`Snapshot` 契约、`extracted_by` 写入路径、以及本计划中已验证的证据结构约定。

> ⚠ **交接前必须确认**：`extracted_by` 的写入入口是开放的（抽取侧要能写进快照），
> 而 MVP 只提供结构与存储，不提供写入 API。**这是刻意的** —— 写入方属第 2 份计划。
