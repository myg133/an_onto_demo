//! ⭐ REQ-174 · **版本级 access mapping** 的存储层 —— `version_access_mappings`
//! 这一张表的唯一读写入口。
//!
//! # 这个文件管什么
//!
//! 只有两件事：
//!
//! - [`PgAccessStore::upsert_in`]：把**导入时已校验通过**的 AM 原文写进某个版本；
//! - [`PgAccessStore::get_raw`]：按 `version_id` 取出原文（服务端加载快照那一次读）。
//!
//! # 这个文件**不**管什么
//!
//! - **解析 / 校验**：`AccessMapping::parse` + `bind` 在
//!   [`crate::console::provision::import_bundle`] 里、**写库之前**做 ——
//!   「不合法就整笔导入失败、版本都不建」这条语义在调用方，不在这里；
//! - **投递 / 绑定**：`prepare_project` 拿到原文后按**那一版本体**再解析一次；
//! - **生效指针**：`activate` 换 `projects.active_version_id`（`db::project`），
//!   本文件不碰 `projects`。
//!
//! # `raw` 的边界
//!
//! 本层只**搬字节**：AM 原文里可能写着 `base_url` / `credential_env` 之类的部署信息，
//! 所以这里⛔ 不提供任何「打印 / 回显 / 序列化」的出口，也⛔ 不把它拼进任何错误消息。
//! 错误只可能来自 SQL（`StoreError::Db`），那条消息里没有 AM 内容。
//!
//! # 为什么单独一个文件
//!
//! 与 [`crate::db::bundle_store`] 同一条的理由：这是**新面**（P1 新增），
//! 混进 `db::project` / `db::snapshot_store` 会让「哪些 SQL 属于既有流水线、
//! 哪些属于 REQ-174」变成读代码才能回答的问题。

use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::db::pg::StoreError;

#[derive(Clone)]
pub struct PgAccessStore {
    pool: PgPool,
}

impl PgAccessStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 写 / 换某个版本的 AM（**调用方必须先校验过**）。
    ///
    /// `version_id` 是主键：同一个版本重复写只会覆盖，不会长出第二行
    /// （版本本身是不可变对象，覆盖只在「同一个包被重导」这种异常路径上发生，
    /// 而 `import_bundle` 的幂等闸门在那之前就返回了）。
    pub async fn upsert(
        &self,
        version_id: Uuid,
        raw: &str,
        sha256: &str,
        sources: usize,
        entities: usize,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        Self::upsert_in(&mut tx, version_id, raw, sha256, sources, entities).await?;
        tx.commit().await?;
        Ok(())
    }

    /// [`Self::upsert`] 的**事务内**版本（SQL 只有一份）。
    ///
    /// ⭐ import 的「建版本 + 写快照五行 + 写 AM + 标包已导入」必须**同一个事务**：
    /// AM 不合法 / 写入失败 ⇒ 版本一起回滚（AC-006：没有新版本）。
    pub async fn upsert_in(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        version_id: Uuid,
        raw: &str,
        sha256: &str,
        sources: usize,
        entities: usize,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO version_access_mappings (version_id, raw, sha256, sources, entities)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (version_id) DO UPDATE
               SET raw = EXCLUDED.raw,
                   sha256 = EXCLUDED.sha256,
                   sources = EXCLUDED.sources,
                   entities = EXCLUDED.entities,
                   stored_at = now()",
        )
        .bind(version_id)
        .bind(raw)
        .bind(sha256)
        .bind(sources as i32)
        .bind(entities as i32)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// 某个版本的 AM 原文。`None` = 这个版本没有 AM（**不是错误**：纯库源本体不需要它）。
    ///
    /// ⚠️ 只返回原文，不解析 —— 解析要按**那一版本体**对账，是调用方的事
    /// （`query::serve::prepare_project`）。返回的字符串⛔ 不许进日志 / 审计。
    pub async fn get_raw(&self, version_id: Uuid) -> Result<Option<String>, StoreError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT raw FROM version_access_mappings WHERE version_id = $1")
                .bind(version_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }
}
