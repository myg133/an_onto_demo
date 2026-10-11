//! Project 与 OntologyVersion 的存储。
//!
//! **三态语义**：`frozen_at` 是 `Option<DateTime<Utc>>`，
//! 与 `created_at` 的非空语义不同。AC-010 的快照往返测试要守住这个坑 ——
//! `NULL` 是「未冻结」，**不是**「数据缺失」，不能被默认值吞成 `now()`。

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("数据库错误：{0}")]
    Db(#[from] sqlx::Error),
    #[error("项目名已存在：{0}")]
    NameTaken(String),
    #[error("版本不存在：project={project} seq={seq}")]
    VersionNotFound { project: String, seq: i32 },
    #[error("版本 {seq} 已冻结，不可再写入")]
    VersionFrozen { seq: i32 },
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ProjectRow {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct VersionRow {
    pub id: Uuid,
    pub project_id: Uuid,
    pub seq: i32,
    /// `None` = **未冻结**。这是有意义的第三态，不是缺失。
    pub frozen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewProject {
    pub name: String,
}

#[derive(Clone)]
pub struct PgProjectStore {
    pool: PgPool,
}

impl PgProjectStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 建表（幂等）。测试与 CLI 首次运行都需要。
    pub async fn migrate(&self) -> Result<(), ProjectError> {
        crate::db::pg::migrate(&self.pool).await?;
        Ok(())
    }

    pub async fn create(&self, p: NewProject) -> Result<ProjectRow, ProjectError> {
        let id = Uuid::new_v4();
        let row: ProjectRow = sqlx::query_as(
            "INSERT INTO projects (id, name) VALUES ($1, $2)
             ON CONFLICT (name) DO NOTHING
             RETURNING id, name, created_at",
        )
        .bind(id)
        .bind(&p.name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ProjectError::NameTaken(p.name.clone()))?;
        Ok(row)
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<ProjectRow>, ProjectError> {
        let row = sqlx::query_as("SELECT id, name, created_at FROM projects WHERE name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    /// 按名字取，找不到就建 —— CLI 首次跑时省一步。
    pub async fn get_or_create(&self, name: &str) -> Result<ProjectRow, ProjectError> {
        if let Some(p) = self.get_by_name(name).await? {
            return Ok(p);
        }
        match self.create(NewProject { name: name.to_string() }).await {
            Ok(p) => Ok(p),
            // 并发下另一个连接先建了 —— 重新读一次即可
            Err(ProjectError::NameTaken(_)) => self
                .get_by_name(name)
                .await?
                .ok_or(ProjectError::NameTaken(name.to_string())),
            Err(e) => Err(e),
        }
    }

    /// 新建版本。
    ///
    /// `seq = max(seq) + 1`，**在事务里先锁住 project 行**。
    ///
    /// 为什么锁 project 而不是版本行：首次建版时该 project **一条版本行都没有**，
    /// `SELECT ... FOR UPDATE` 锁不到任何东西 → 4 个并发调用全算出 seq=1，
    /// 撞 `UNIQUE(project_id, seq)`。锁 project 行则永远存在，天然串行化。
    ///
    /// （`test_version_seq_under_concurrency_is_unique` 就是抓这个的。）
    pub async fn new_version(&self, project_id: Uuid) -> Result<VersionRow, ProjectError> {
        let mut tx = self.pool.begin().await?;

        // 锁 project 行 —— 它必定存在，串行化同一 project 的建版本
        sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_one(&mut *tx)
            .await?;

        let row: VersionRow = sqlx::query_as(
            "INSERT INTO ontology_versions (id, project_id, seq)
             VALUES ($1, $2, COALESCE((SELECT MAX(seq) + 1 FROM ontology_versions WHERE project_id = $2), 1))
             RETURNING id, project_id, seq, frozen_at, created_at",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(row)
    }

    pub async fn list_versions(&self, project_id: Uuid) -> Result<Vec<VersionRow>, ProjectError> {
        let rows = sqlx::query_as(
            "SELECT id, project_id, seq, frozen_at, created_at
             FROM ontology_versions WHERE project_id = $1 ORDER BY seq",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// 按 seq 取版本；`project_name` 只用于错误信息。
    pub async fn version_by_seq(
        &self,
        project_id: Uuid,
        project_name: &str,
        seq: i32,
    ) -> Result<VersionRow, ProjectError> {
        sqlx::query_as(
            "SELECT id, project_id, seq, frozen_at, created_at
             FROM ontology_versions WHERE project_id = $1 AND seq = $2",
        )
        .bind(project_id)
        .bind(seq)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ProjectError::VersionNotFound { project: project_name.to_string(), seq })
    }

    pub async fn latest_version(&self, project_id: Uuid) -> Result<Option<VersionRow>, ProjectError> {
        let row = sqlx::query_as(
            "SELECT id, project_id, seq, frozen_at, created_at
             FROM ontology_versions WHERE project_id = $1 ORDER BY seq DESC LIMIT 1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// 冻结版本（显式动作 —— 规格 §9.1「冻结是判据」）。
    /// 重复冻结不报错，幂等。
    pub async fn freeze(&self, version_id: Uuid) -> Result<VersionRow, ProjectError> {
        let row = sqlx::query_as(
            "UPDATE ontology_versions SET frozen_at = COALESCE(frozen_at, now())
             WHERE id = $1
             RETURNING id, project_id, seq, frozen_at, created_at",
        )
        .bind(version_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(ProjectError::VersionNotFound { project: "?".into(), seq: -1 })?;
        Ok(row)
    }

    /// 版本是否已冻结 —— 写路径的闸门（AC-006）。
    pub async fn is_frozen(&self, version_id: Uuid) -> Result<bool, ProjectError> {
        let v: Option<bool> = sqlx::query_scalar("SELECT frozen_at IS NOT NULL FROM ontology_versions WHERE id = $1")
            .bind(version_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(v.ok_or(ProjectError::VersionNotFound { project: "?".into(), seq: -1 })?)
    }
}
