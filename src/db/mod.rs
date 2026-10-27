//! 存储层 —— **有 I/O 的一层**。
//!
//! AC-002 的对照面：`src/domain/` 是零 I/O 的纯逻辑，这里不是。
//! 两者不要互相渗透（domain 里不出现 `sqlx` / `db::`）。
//!
//! **刻意不写 `trait`**（范围规划 §3 暂缓项）：本 MVP 只有 PostgreSQL
//! 一个实现，只有一个实现时抽 trait 是过早抽象。
//!
//! ⚠️ **REQ-013 补一句边界**：那条纪律说的是**存储层**（PG 只有一个实现）。
//! `agent-registry` 在这一层是**一个具体实现**（`PgAgentRegistry`），
//! 也**没有**为它抽 trait —— 它没被要求可替换。
//! 真正被抽成 trait 的是**抽取 agent 层**（`crate::agent::ExtractionAgent`），
//! 理由是规格 §7.1 明写「【外部 agent】可插拔」—— 这是规格**许诺**的实现维度，
//! 与这里的「单一实现」是两回事。
//! ⚠️ **REQ-025 修一处假话**（原文写的是「抽取 agent 层今天也只有 1 个生产实现」
//! +「『≥3（规则 / 外部协议 / LLM 兜底）』是规格许诺的目标形态」，两处都不成立）：
//! 规格 §7.1 只许诺「**可插拔**」，**从未承诺三个实现**，「LLM 兜底」在 `BA/` 全文档
//! 0 命中；而 REQ-023 起抽取 agent 层**已有 2 个生产实现**
//! （`RuleBasedAgent` / `AcpAgent`）。按实况说：**已实现 2 个**，
//! LLM 兜底是偏差列表 **RN-5，未做**。
//! 详见 `src/agent/mod.rs` 的模块头（同一处承诺，两边不许说法漂移）。

pub mod agent_registry;
// REQ-109 · AI 语义判断的指纹缓存（`ai_cache` 表）。
// 它是**存储层**：编排（`extract::ai_semantic`）是同步的，库读写在它前后各一次。
pub mod ai_cache;
pub mod apply;
// ⭐ REQ-159 · 构建包导入的四张表（`project_keys` / `project_bundles` /
// `project_bundle_payloads` / `project_databases`）。⛔ 不并进 `project`：
// 那是最老的一对概念（项目 / 版本），混进去会让「这条 SQL 属于哪条流水线」
// 变成读代码才能回答的问题。
pub mod bundle_store;
pub mod pg;
pub mod pool;
pub mod project;
pub mod proposal;
pub mod snapshot_store;

pub use agent_registry::PgAgentRegistry;
pub use ai_cache::PgAiCache;
pub use bundle_store::PgBundleStore;
pub use pg::StoreError;
pub use pool::DbError;
pub use snapshot_store::PgStore;
