//! ⭐ REQ-159 · **构建包导入**的存储层 —— `project_keys` / `project_bundles` /
//! `project_bundle_payloads` / `project_databases` 四张表的唯一读写入口。
//!
//! # 为什么单独一个文件，而不并进 `db::project`
//!
//! [`crate::db::project`] 是「项目 / 版本」——本仓最老的一对概念，读它的人比读
//! 这份新代码的人多得多。构建包导入的四张表是**新面**（P1a 全新增，不碰读路径），
//! 混进去会让「哪些 SQL 属于既有流水线、哪些属于 REQ-159」变成读代码才能回答的问题。
//!
//! # 本文件**没有**的写路径
//!
//! - **建版本**：走 [`crate::db::apply::begin_new_version`]（与 `proposal apply` 同一个）；
//! - **写快照五行**：走 [`crate::db::apply::write_snapshot_in`]（同上）；
//! - **冻结 / 生效指针**：走 [`crate::db::project::PgProjectStore::activate`]。
//!
//! 换句话说：这里只有**这四张新表**的 SQL。既有五张快照表一个字节都不由本文件写。
//!
//! # `dsn_ciphertext` 的边界
//!
//! 本层只**搬字节**：加解密在 [`crate::provision::dsn`]，而它的密钥来自环境变量。
//! 这里刻意不提供任何「按明文查 / 打印」的方法 —— 读出来的永远是密文，
//! 想回显明文必须经过解密那一层，而那一层没有对外的序列化出口。

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::db::pg::StoreError;

/// 一把 key。
///
/// ⭐ **两列都存**（契约变更 2026-10-27 10:25）：
/// - `token_sha256`：鉴权按摘要查（快、不碰密钥）；
/// - `token_ciphertext`：`GET /api/projects/:p/key` 要能**再次查看**（AC-011）。
///
/// ⛔ 这个结构里没有「明文 key」这一栏：明文只会**当场解密出来**交给那个端点，
/// 不经过任何可序列化的类型（AC-009：响应 / 日志 / 审计里搜不到它进过内存的证据）。
#[derive(Debug, Clone)]
pub struct ProjectKeyRow {
    pub id: Uuid,
    pub project_id: Uuid,
    /// `sha256(key)` 的小写十六进制（64 位）。
    pub token_sha256: String,
    /// key 的 AES-256-GCM 密文（`v1:<hex>`）。⛔ 不直接回显。
    pub token_ciphertext: String,
    /// 给谁用（审计归属）。⛔ 非空。
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// 一个构建包的状态行。
#[derive(Debug, Clone)]
pub struct BundleRow {
    pub id: Uuid,
    pub project_id: Uuid,
    /// `uploaded` / `db_pending` / `imported` / `activated` / `failed`。
    pub state: String,
    /// `manifest.json` 的**原文**（校验通过的那一份，供 `GET` 回显组件清单）。
    pub manifest: Value,
    /// 整包 sha256（上传时算，import 时**再核一次**）。
    pub bundle_sha256: String,
    pub bytes: i64,
    pub uploaded_at: DateTime<Utc>,
    pub imported_version_id: Option<Uuid>,
    pub imported_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

/// 一个项目的源库 DSN（**密文**）与上次连通性检查结果。
#[derive(Debug, Clone)]
pub struct ProjectDatabaseRow {
    pub project_id: Uuid,
    pub dialect: String,
    /// AES-256-GCM 密文（`v1:<hex>`）。⛔ 永不回显。
    pub dsn_ciphertext: String,
    pub updated_at: DateTime<Utc>,
    /// 三态：`None` = 还没测过；`Some(true)` / `Some(false)` = 上次的结果。
    pub last_check_ok: Option<bool>,
    pub last_check_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
}

#[derive(Clone)]
pub struct PgBundleStore {
    pool: PgPool,
}

/// 「包已导入」的 SQL —— **全仓唯一**一份字面量（pool 与 tx 两个调用点共用）。
///
/// `state` 由这里写死成 `'imported'`（不是参数）：这个状态是这条 SQL 的**语义**，
/// 让调用方能传一个别的值进来只会多一种非法状态的来源。
const MARK_IMPORTED_SQL: &str = "UPDATE project_bundles
     SET state = 'imported', imported_version_id = $2, imported_at = now(),
         error_code = NULL, error_message = NULL
     WHERE id = $1";

impl PgBundleStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    // ══════════════════════════════════════════════════════════
    // project_keys
    // ══════════════════════════════════════════════════════════

    /// 写一把 key：**摘要 + 可逆密文两列**（明文只在调用方的响应里出现）。
    ///
    /// ⚠️ 本函数**不**先查「这个项目有没有 active key」—— 那是应用层的先查后插，
    /// 并发下挡不住。真正的闸是 `project_keys_one_active` 那个部分唯一索引
    /// （AC-010 的判据）：第二把 active key 会在这里拿到 `23505`。
    pub async fn create_key(
        &self,
        project_id: Uuid,
        token_sha256: &str,
        token_ciphertext: &str,
        label: &str,
    ) -> Result<ProjectKeyRow, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row =
            Self::create_key_in(&mut tx, project_id, token_sha256, token_ciphertext, label).await?;
        tx.commit().await?;
        Ok(row)
    }

    /// [`Self::create_key`] 的**事务内**版本（SQL 只有上面那一份）。
    ///
    /// ⭐ REQ-159 · `POST /api/projects` 的「项目行 + key 行」必须在同一个事务里，
    /// 否则会留下一个**没有 key 的项目**（见 `db::project::create_labeled_in` 的说明）。
    pub async fn create_key_in(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        project_id: Uuid,
        token_sha256: &str,
        token_ciphertext: &str,
        label: &str,
    ) -> Result<ProjectKeyRow, StoreError> {
        let row: (
            Uuid,
            Uuid,
            String,
            String,
            String,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        ) = sqlx::query_as(
            "INSERT INTO project_keys (id, project_id, token_sha256, token_ciphertext, label)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, project_id, token_sha256, token_ciphertext, label, created_at, revoked_at",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(token_sha256)
        .bind(token_ciphertext)
        .bind(label)
        .fetch_one(&mut **tx)
        .await?;
        Ok(ProjectKeyRow {
            id: row.0,
            project_id: row.1,
            token_sha256: row.2,
            token_ciphertext: row.3,
            label: row.4,
            created_at: row.5,
            revoked_at: row.6,
        })
    }

    /// 一个项目的**有效** key（`revoked_at IS NULL`）。一项目最多一把。
    pub async fn active_key(&self, project_id: Uuid) -> Result<Option<ProjectKeyRow>, StoreError> {
        let row: Option<(
            Uuid,
            Uuid,
            String,
            String,
            String,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
        )> = sqlx::query_as(
            "SELECT id, project_id, token_sha256, token_ciphertext, label, created_at, revoked_at
             FROM project_keys WHERE project_id = $1 AND revoked_at IS NULL",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ProjectKeyRow {
            id: r.0,
            project_id: r.1,
            token_sha256: r.2,
            token_ciphertext: r.3,
            label: r.4,
            created_at: r.5,
            revoked_at: r.6,
        }))
    }

    /// ⭐ REQ-175 · 按**摘要**找一把**未吊销**的 key 绑定的项目名（运行期鉴权用）。
    ///
    /// `token_sha256` 上有 `UNIQUE` ⇒ 最多一行。`revoked_at IS NOT NULL` 的行**查不到**
    /// —— 与 [`Self::active_key`] 同一个「有效」口径（只是查的方向相反：那边按项目，
    /// 这边按摘要）。
    ///
    /// ⛔ 本方法**不返回**密文 / 明文 / 摘要本身：调用方（`query::db_keys`）手上已经有
    /// 摘要，它只需要「这把 key 绑的是哪个项目」。项目行不存在（被删 / 改名）⇒ `None`
    /// —— 那种 key 从鉴权角度就是「无效」，不是 500。
    pub async fn project_for_active_key(
        &self,
        token_sha256: &str,
    ) -> Result<Option<String>, StoreError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT p.name
               FROM project_keys k
               JOIN projects p ON p.id = k.project_id
              WHERE k.token_sha256 = $1 AND k.revoked_at IS NULL",
        )
        .bind(token_sha256)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| r.0))
    }

    // ══════════════════════════════════════════════════════════
    // project_bundles（+ payload）
    // ══════════════════════════════════════════════════════════

    /// 落一个**已校验通过**的包：状态行 + 包字节，**同一事务**。
    ///
    /// 为什么字节与行必须一起落：`GET /api/projects/:p/bundles/:id` 能查到行，
    /// 就意味着 `POST …/import` 一定读得到字节。两半分开写会出现「状态是
    /// uploaded、import 时才报找不到包」——那就是一次安静的半截状态。
    pub async fn create_bundle(
        &self,
        project_id: Uuid,
        manifest: &Value,
        bundle_sha256: &str,
        bytes: i64,
        data: &[u8],
    ) -> Result<BundleRow, StoreError> {
        let id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO project_bundles
               (id, project_id, state, manifest, bundle_sha256, bytes)
             VALUES ($1, $2, 'uploaded', $3, $4, $5)",
        )
        .bind(id)
        .bind(project_id)
        .bind(manifest)
        .bind(bundle_sha256)
        .bind(bytes)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO project_bundle_payloads (bundle_id, data) VALUES ($1, $2)")
            .bind(id)
            .bind(data)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get_bundle(id).await?.ok_or_else(|| {
            StoreError::Db(sqlx::Error::RowNotFound)
        })
    }

    const COLUMNS: &'static str = "id, project_id, state, manifest, bundle_sha256, bytes, \
         uploaded_at, imported_version_id, imported_at, error_code, error_message";

    fn decode(row: &sqlx::postgres::PgRow) -> Result<BundleRow, sqlx::Error> {
        use sqlx::Row;
        Ok(BundleRow {
            id: row.try_get("id")?,
            project_id: row.try_get("project_id")?,
            state: row.try_get("state")?,
            manifest: row.try_get("manifest")?,
            bundle_sha256: row.try_get("bundle_sha256")?,
            bytes: row.try_get("bytes")?,
            uploaded_at: row.try_get("uploaded_at")?,
            imported_version_id: row.try_get("imported_version_id")?,
            imported_at: row.try_get("imported_at")?,
            error_code: row.try_get("error_code")?,
            error_message: row.try_get("error_message")?,
        })
    }

    pub async fn get_bundle(&self, id: Uuid) -> Result<Option<BundleRow>, StoreError> {
        let sql = format!("SELECT {} FROM project_bundles WHERE id = $1", Self::COLUMNS);
        let row = sqlx::query(&sql).bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(Self::decode).transpose().map_err(Into::into)
    }

    /// 按 `(项目, 包)` 取 —— 端点的路径里两段都是作用域，缺一不可。
    pub async fn get_bundle_in_project(
        &self,
        id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<BundleRow>, StoreError> {
        let sql = format!(
            "SELECT {} FROM project_bundles WHERE id = $1 AND project_id = $2",
            Self::COLUMNS
        );
        let row = sqlx::query(&sql)
            .bind(id)
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(Self::decode).transpose().map_err(Into::into)
    }

    /// 包字节（import 时读）。`None` = 行在、字节不在（不该发生的半截状态）。
    pub async fn payload(&self, id: Uuid) -> Result<Option<Vec<u8>>, StoreError> {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT data FROM project_bundle_payloads WHERE bundle_id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    /// 改状态（并清/写错误两栏）。`error = None` ⇒ 两栏置 NULL。
    pub async fn set_state(
        &self,
        id: Uuid,
        state: &str,
        error: Option<(&str, &str)>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE project_bundles SET state = $2, error_code = $3, error_message = $4
             WHERE id = $1",
        )
        .bind(id)
        .bind(state)
        .bind(error.map(|e| e.0))
        .bind(error.map(|e| e.1))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 解析入库成功：状态 + 新版本指针 + 成功时间，**一条 UPDATE**。
    pub async fn mark_imported(&self, id: Uuid, version_id: Uuid) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        Self::mark_imported_in(&mut tx, id, version_id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// [`Self::mark_imported`] 的**事务内**版本。
    ///
    /// ⭐ REQ-159 · import 的「建版本 + 写快照五行 + 标包已导入」必须同一个事务：
    /// 否则会出现「版本建出来了、包还显示 uploaded」⇒ 同一个包被再导一次 ⇒
    /// 两个版本描述同一份内容。
    pub async fn mark_imported_in(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: Uuid,
        version_id: Uuid,
    ) -> Result<(), StoreError> {
        sqlx::query(MARK_IMPORTED_SQL)
            .bind(id)
            .bind(version_id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    // ══════════════════════════════════════════════════════════
    // project_databases
    // ══════════════════════════════════════════════════════════

    pub async fn get_database(
        &self,
        project_id: Uuid,
    ) -> Result<Option<ProjectDatabaseRow>, StoreError> {
        let row: Option<(Uuid, String, String, DateTime<Utc>, Option<bool>, Option<DateTime<Utc>>, Option<String>)> =
            sqlx::query_as(
                "SELECT project_id, dialect, dsn_ciphertext, updated_at,
                        last_check_ok, last_check_at, last_error_code
                 FROM project_databases WHERE project_id = $1",
            )
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| ProjectDatabaseRow {
            project_id: r.0,
            dialect: r.1,
            dsn_ciphertext: r.2,
            updated_at: r.3,
            last_check_ok: r.4,
            last_check_at: r.5,
            last_error_code: r.6,
        }))
    }

    /// 写 / 换 DSN（密文）。**换 DSN ⇒ 上次的检查结果作废**（置 NULL）。
    ///
    /// 为什么必须作废：`import` 只认 `last_check_ok = true`。换了 DSN 还留着
    /// 旧的 `true`，等于让一个**从没测过的** DSN 直接通过 import 的闸门。
    pub async fn upsert_database(
        &self,
        project_id: Uuid,
        dialect: &str,
        dsn_ciphertext: &str,
    ) -> Result<ProjectDatabaseRow, StoreError> {
        sqlx::query(
            "INSERT INTO project_databases (project_id, dialect, dsn_ciphertext)
             VALUES ($1, $2, $3)
             ON CONFLICT (project_id) DO UPDATE
               SET dialect = EXCLUDED.dialect,
                   dsn_ciphertext = EXCLUDED.dsn_ciphertext,
                   updated_at = now(),
                   last_check_ok = NULL,
                   last_check_at = NULL,
                   last_error_code = NULL",
        )
        .bind(project_id)
        .bind(dialect)
        .bind(dsn_ciphertext)
        .execute(&self.pool)
        .await?;
        self.get_database(project_id).await?.ok_or_else(|| {
            StoreError::Db(sqlx::Error::RowNotFound)
        })
    }

    /// 记一次连通性检查结果（`ok` + 分类码）。失败也记 —— 那是可查的事实。
    pub async fn note_check(
        &self,
        project_id: Uuid,
        ok: bool,
        error_code: Option<&str>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE project_databases
             SET last_check_ok = $2, last_check_at = now(), last_error_code = $3
             WHERE project_id = $1",
        )
        .bind(project_id)
        .bind(ok)
        .bind(error_code)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
