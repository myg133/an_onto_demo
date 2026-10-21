//! 提案的**应用** —— 事务、版本创建、状态回写。
//!
//! 纯变换在 `domain::apply`；本文件只管「怎么落库、怎么保证不落半截」。
//!
//! # 单事务铁律
//!
//! 建新版本 → 写快照 → 回写 proposal.status 必须**在一个事务**里。
//! 中途失败若不回滚，会留下「有个新版本但内容是空的」或
//! 「提案标了 applied 但版本没建成」—— 后者更坏：人以为已经应用了。

use crate::db::pg::StoreError;
use crate::db::project::PgProjectStore;
use crate::db::proposal::PgProposalStore;
use crate::db::snapshot_store::{
    delete_snapshot_rows, insert_snapshot_rows, update_version_extracted_by, PgStore,
};
use crate::domain::apply::{apply, ApplyError, ApplyInput};
use crate::domain::review::Track;
use crate::domain::snapshot::Snapshot;
use sqlx::postgres::PgPool;
use uuid::Uuid;

pub struct PgApplier {
    pool: PgPool,
}

impl PgApplier {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 应用提案，产出新版本。
    ///
    /// # ADR-001
    /// 新版本的 `extracted_by` **原样取提案的值**；提案没有则记 `None`。
    /// **不继承 base，不推断。** 继承会让「agent 偷偷换版本」不可见。
    ///
    /// **不自动冻结** —— 冻结是显式动作（规格 §9.1「冻结是判据」）。
    pub async fn apply_proposal(&self, proposal_id: Uuid) -> Result<Uuid, StoreError> {
        let pstore = PgProposalStore::new(self.pool.clone());
        let projects = PgProjectStore::new(self.pool.clone());
        let store = PgStore::new(self.pool.clone());

        let proposal = pstore
            .get(proposal_id)
            .await?
            .ok_or_else(|| StoreError::ProposalNotPending(proposal_id.to_string(), "不存在".into()))?;
        if proposal.status != "pending" {
            return Err(StoreError::ProposalNotPending(
                proposal_id.to_string(),
                proposal.status.clone(),
            ));
        }

        // base 必须未冻结
        if projects.is_frozen(proposal.base_version_id).await.unwrap_or(false) {
            return Err(StoreError::BaseVersionFrozen);
        }
        let base = store
            .get_snapshot(proposal.base_version_id)
            .await?
            .ok_or(StoreError::BaseVersionFrozen)?;

        // 提案里的 extracted_by（ADR-001：原样取，缺失记 None）
        let proposal_eb = proposal
            .extracted_by
            .as_ref()
            .and_then(|j| serde_json::from_value(j.clone()).ok());

        // 条目 + 基线类型（关系类校验要用）
        let rows = pstore.items(proposal_id).await?;
        let items = PgProposalStore::to_item_views(&rows);
        let baseline: std::collections::BTreeSet<String> =
            base.entity_types.iter().map(|e| e.key.clone()).collect();

        // 纯变换（可能失败 —— 失败时**什么都没写**，这是分层的收益）
        let mut next = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: proposal_eb,
            items: &items,
            baseline_types: &baseline,
        })
        .map_err(map_apply_error)?;

        // ── 落库，单事务 ──
        let mut tx = self.pool.begin().await?;

        // 在事务内再建版本：锁 project 行拿 seq，避免与并发 apply 撞 seq
        let locked: (Uuid,) = sqlx::query_as("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(proposal.project_id)
            .fetch_one(&mut *tx)
            .await?;
        let _ = locked;

        let new_version: (Uuid, i32) = sqlx::query_as(
            "INSERT INTO ontology_versions (id, project_id, seq)
             VALUES ($1, $2, COALESCE((SELECT MAX(seq)+1 FROM ontology_versions WHERE project_id=$2),1))
             RETURNING id, seq",
        )
        .bind(Uuid::new_v4())
        .bind(proposal.project_id)
        .fetch_one(&mut *tx)
        .await?;
        next.version_id = new_version.0;
        next.seq = new_version.1;

        // 写快照行（复用 save 的 SQL，但走同一事务）
        write_snapshot_in(&mut tx, &next).await?;

        // 回写提案状态
        sqlx::query("UPDATE proposals SET status = 'applied' WHERE id = $1")
            .bind(proposal_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(next.version_id)
    }
}

fn map_apply_error(e: ApplyError) -> StoreError {
    match e {
        ApplyError::NotPassed { blocked, dangling } => {
            StoreError::ProposalNotPassed { blocked, dangling }
        }
        ApplyError::NothingApplicable => StoreError::NothingApplicable,
        ApplyError::KeyConflict(k) => StoreError::KeyConflict(k),
        ApplyError::DanglingLink(l) => StoreError::TypeNotFound(l),
        ApplyError::RemovingMissing(t) => StoreError::TypeNotFound(t),
        ApplyError::BadConceptGroup(m) => StoreError::BadConceptGroup(m),
        // REQ-108 · 读不懂的分数 ⇒ 拒绝 apply（fail-closed，不静默降级成无分数）
        ApplyError::BadConfidence(m) => StoreError::BadConfidence(m),
    }
}

/// 在既有事务里写快照行。
///
/// 与 `PgStore::save_snapshot` 的差别只有两点：
///   ① 走 `&mut Transaction` 而非自开事务（否则嵌套事务，外层回滚不到它）
///   ② **不做冻结检查** —— 新版本刚建出来必然未冻结；
///      冻结闸门在 `save_snapshot` 那条路径上（AC-006）
///
/// REQ-012 追加第三点：**不把唯一冲突映成 `KeyConflict`**（save 那边映了）。
///
/// REQ-033 再追加第四点：**本函数不再自己实现任何一步 SQL**。
/// 删旧行（`delete_snapshot_rows`）、写新行（`insert_snapshot_rows`）、
/// 版本行 `extracted_by`（`update_version_extracted_by`）三步都在
/// `db::snapshot_store` 里，三份抄写全部收口 —— 本函数只剩**编排**：
/// 三步的**先后顺序**与调用方传的**同一个事务**都不变。
pub(crate) async fn write_snapshot_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    snap: &Snapshot,
) -> Result<(), StoreError> {
    // ① 删旧行 —— REQ-033 · 共享层，本函数不再自抄循环。
    delete_snapshot_rows(tx, snap.version_id).await?;

    // ② 写新行 —— REQ-012 · 五栏行写入走 `insert_snapshot_rows`，**唯一**实现。
    //
    // 本函数原先自己抄了一份 INSERT，抄漏了 `action_types` / `functions`
    // （REQ-011 的缺陷本体：DELETE 之后没写回来，一次 apply 静默清空两栏）。
    // REQ-011 补上了那两段，但两份实现仍然并存；REQ-012 之后**只有一份**。
    //
    // **本入口的错误映射保持原样**：冲突 ⇒ `StoreError::Db`（不是 `KeyConflict`）。
    // 这与 `save_snapshot` 的映射不同，所以共享层返回原始 `sqlx::Error`，
    // 映射留在各自调用方 —— 对外语义逐字未变。
    insert_snapshot_rows(tx, snap)
        .await
        .map_err(|e| StoreError::Db(e.source))?;

    // ③ 版本行 —— REQ-033 · 与 `save_snapshot` 共用同一个构造点。
    update_version_extracted_by(snap.version_id, &snap.extracted_by)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// 便捷：一次双轨接受的快捷路径（CLI 演示用）。
pub async fn accept_both_tracks(
    pstore: &PgProposalStore,
    item_id: Uuid,
) -> Result<(), StoreError> {
    pstore
        .set_decision(item_id, Track::Biz, crate::domain::review::Decision::Accepted)
        .await?;
    pstore
        .set_decision(
            item_id,
            Track::Model,
            crate::domain::review::Decision::Accepted,
        )
        .await?;
    Ok(())
}

// ── REQ-012 · apply 侧对外错误语义的**运行时**判定 ──────────────
//
// 为什么不放在 `tests/`：`write_snapshot_in` 是 `pub(crate)`，
// 集成测试够不着。而「唯一冲突在 apply 侧必须仍是 `StoreError::Db`
// 而不是 `save_snapshot` 的 `StoreError::KeyConflict`」这条语义
// 在生产路径上**不可达** —— apply 总是写一个全新的 version_id，
// `UNIQUE (version_id, key)` 撞不上。唯一能真正跑到它的办法
// 就是直接调它。
//
// 这是 `src/db/` 下第一个带 DB 的 `#[cfg(test)]` 模块：其余
// 单元测试都在 `src/domain/`（零 I/O）。这里破例的理由是
// 「不可达分支的语义不变」只能这样证。
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::project::PgProjectStore;
    use crate::domain::snapshot::{ActionType, Snapshot};

    async fn pool() -> PgPool {
        let url = crate::config::test_database_url()
            .expect("缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库，请导出环境变量");
        // REQ-042 · 建连接套上有限重试（REQ-041 漏掉的第 5 处覆盖缺口）。
        //
        // **为什么这里自己写一份，而不复用 `tests/common/mod.rs` 的 `retry_connect`**：
        // 本函数在 `#[cfg(test)] mod tests` 内部，是**库自身的单元测试**；
        // 集成测试（`tests/`）依赖库，方向是单向的 —— 库反过来够不着
        // `tests/common`。所以只能就地重写一份。
        //
        // **这不是生产路径**：`#[cfg(test)]` 的内容 `cargo build` 根本不编译，
        // 生产二进制里没有这行代码。所以「给生产加重试会掩盖真故障」的顾虑
        // 在这里不成立。
        //
        // 保持与 `tests/common/mod.rs` 的 `retry_connect` 同一套语义：
        // 2 次重试 + 指数退避，只包住建连接本身。
        let mut last: Option<crate::db::pool::DbError> = None;
        for attempt in 0..=2usize {
            if attempt > 0 {
                // 300ms / 600ms，与 common::backoff_for 同型
                tokio::time::sleep(std::time::Duration::from_millis(300 << (attempt - 1)))
                    .await;
            }
            match crate::db::pool::connect_for_tests(&url, 1).await {
                Ok(p) => return p,
                Err(e) => last = Some(e),
            }
        }
        panic!("连不上测试库：已尝试 3 次仍失败，最后一次：{:?}", last)
    }

    fn snap_with_duplicate_actions(version_id: Uuid) -> Snapshot {
        let dup = ActionType {
            key: "publish_customer".into(),
            target: "customer".into(),
            parameters: serde_json::json!({}),
            preconditions: serde_json::json!({}),
            effects: vec![],
            permission: serde_json::json!({}),
            audit_policy: serde_json::json!({}),
            evidence: vec![],
        };
        Snapshot {
            version_id,
            seq: 1,
            extracted_by: None,
            entity_types: vec![],
            link_types: vec![],
            action_types: vec![dup.clone(), dup],
            functions: vec![],
            concept_groups: vec![],
        }
    }

    /// REQ-012 · `write_snapshot_in` 遇唯一冲突 ⇒ **`StoreError::Db`**。
    ///
    /// 与 `PgStore::save_snapshot` 的 `StoreError::KeyConflict(key)` **不同**，
    /// 而且这个不同是**刻意保留**的：共享层返回原始 `sqlx::Error` + key，
    /// 映射留在各自调用方。这条测试就是「映射确实还在各自那边」的凭据。
    #[tokio::test]
    async fn write_snapshot_in_maps_unique_conflict_to_db_not_key_conflict() {
        let pool = pool().await;
        let projects = PgProjectStore::new(pool.clone());
        crate::db::pg::migrate(&pool).await.expect("建表失败");
        let project = common_project(&projects, "req012applyerr").await;
        let v = projects.new_version(project.id).await.expect("建版本失败").id;

        let mut tx = pool.begin().await.unwrap();
        let got = write_snapshot_in(&mut tx, &snap_with_duplicate_actions(v)).await;
        // 无论成败都要回滚，别把测试数据留在共享库
        let _ = tx.rollback().await;

        match got {
            Err(StoreError::Db(e)) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("duplicate key") || msg.contains("unique"),
                    "应当是唯一约束冲突（23505），实际错误：{msg}"
                );
            }
            Err(StoreError::KeyConflict(k)) => panic!(
                "apply 侧的冲突语义被改成了 KeyConflict({k}) —— \
                 REQ-012 要求对外语义逐字未变（这里应仍是 StoreError::Db）"
            ),
            other => panic!("期望唯一约束冲突，实际得到：{other:?}"),
        }
    }

    async fn common_project(
        projects: &PgProjectStore,
        prefix: &str,
    ) -> crate::db::project::ProjectRow {
        use crate::db::project::NewProject;
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let name = format!("t_{prefix}_{}_{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        projects
            .create(NewProject { name })
            .await
            .expect("建项目失败")
    }
}
