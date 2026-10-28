//! ⭐ REQ-159 · **构建包导入的领域编排** —— 7 个端点各自的「读状态 → 调既有函数 → 回执」。
//!
//! # 这里**没有**的东西（每一条都是刻意的）
//!
//! | 没有 | 走哪条既有路径 |
//! | --- | --- |
//! | 建版本 / 取 `seq` | [`crate::db::apply::begin_new_version`]（`proposal apply` 的同一个） |
//! | 写快照五行 | [`crate::db::apply::write_snapshot_in`]（同上；REQ-011/012/033 收口过的那份） |
//! | 冻结 / 生效指针 | [`crate::db::project::PgProjectStore::activate`] |
//! | 包格式 / manifest / 8 个错误码 | [`crate::provision::bundle`] |
//! | DSN 加解密 / 探测 | [`crate::provision::dsn`] |
//!
//! # 为什么这些端点**不**走 `check_project`（单项目作用域）
//!
//! REQ-124 的既有端点（import / apply / freeze / export …）是「一个进程只服务
//! `--project` 那一个项目」，那是**既有**纪律，一个字节不改。
//!
//! REQ-159 的这 7 个端点是**控制面自己**的能力：建项目、往**任意**项目上传包。
//! 它们的路径里带 `:p`，作用域就是那个 `:p`（必须存在，否则 404）——
//! 否则「建了项目却不能往里传包」。
//!
//! ⚠️ 这**不**等于多项目查询：`query serve` 仍然一进程一项目（那是 P1b），
//! 本文件一行都没碰它。

use axum::http::StatusCode;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use uuid::Uuid;

use crate::db::access_store::PgAccessStore;
use crate::db::bundle_store::PgBundleStore;
use crate::db::project::{PgProjectStore, ProjectError, ProjectRow};
use crate::db::snapshot_store::PgStore;
use crate::query::source_gate::AccessMapping;

use crate::provision::bundle::{self, BundleError, State, VerifiedBundle};
use crate::provision::dsn::{self, SecretCipher, DsnTest};
use super::OpError;

// ══════════════════════════════════════════════════════════════
// 小工具
// ══════════════════════════════════════════════════════════════

/// [`BundleError`] → [`OpError`]（HTTP 层只有一个错误出口，见 `console::http::fail_op`）。
fn fail(e: BundleError) -> OpError {
    OpError::new(e.status, e.code, e.message, e.hint)
}

fn rfc3339(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 按名字取项目；不存在 ⇒ **404**（`project.not_found`）。
///
/// ⚠️ 这是 REQ-159 端点的作用域来源（路径里的 `:p`），**不是** `svc.project`。
async fn project_row(pool: &PgPool, name: &str) -> Result<ProjectRow, OpError> {
    PgProjectStore::new(pool.clone())
        .get_by_name(name)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "project.not_found",
                format!("项目 {name:?} 不存在。"),
                "先用 `POST /api/projects` 建项目（它会返回一次性 key），再用返回的名字上传包。",
            )
        })
}

/// 路径里的包 id：接受 `bdl_<uuid>`（响应里的形状）与裸 UUID（便于手敲 / 脚本）。
pub fn parse_bundle_id(raw: &str) -> Result<Uuid, OpError> {
    let t = raw.trim();
    let t = t.strip_prefix("bdl_").unwrap_or(t);
    Uuid::parse_str(t).map_err(|_| {
        OpError::bad_request(
            "bundle_id.invalid",
            format!("包 id {raw:?} 不是 UUID（也不是 `bdl_<uuid>` 的形状）。"),
            "包 id 用 `POST /api/projects/:p/bundles` 的响应里的 `bundle_id`，\
             或 `GET /api/projects/:p/bundles/:id` 已知的那一个。",
        )
    })
}

/// 回执里用统一的 `bdl_<uuid>` 形状（demand §五-2 的例子就是这么写的）。
pub fn bundle_id_text(id: Uuid) -> String {
    format!("bdl_{id}")
}

/// 一版快照的**真实**计数（从库里读出来的那一版，不是包里的声明）。
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Counts {
    pub entity_types: usize,
    pub link_types: usize,
    pub properties: usize,
    pub action_types: usize,
    pub functions: usize,
    pub concept_groups: usize,
}

fn counts_of(snap: &crate::domain::snapshot::Snapshot) -> Counts {
    Counts {
        entity_types: snap.entity_types.len(),
        link_types: snap.link_types.len(),
        properties: snap.entity_types.iter().map(|e| e.properties.len()).sum(),
        action_types: snap.action_types.len(),
        functions: snap.functions.len(),
        concept_groups: snap.concept_groups.len(),
    }
}

// ══════════════════════════════════════════════════════════════
// ① POST /api/projects
// ══════════════════════════════════════════════════════════════

/// 建项目的回执（**key 明文在这里出现一次**；之后走 `GET …/key` 可再次查看）。
#[derive(Debug, Clone, Serialize)]
pub struct CreateProjectOutcome {
    pub project: String,
    pub label: Option<String>,
    /// 明文 key。⭐ 2026-10-27 契约变更后它**可再次查看**（`GET /api/projects/:p/key`），
    /// 但库里存的是 `sha256`（鉴权用）+ `AES-256-GCM 密文`（查看用），⛔ 不是明文列。
    pub key: String,
    pub key_kind: &'static str,
    pub key_label: String,
    pub key_digest: String,
    pub bound_project: String,
    pub created_at: String,
}

/// 生成一把 query key：`ontok_q_` + 64 位十六进制（32 字节 CSPRNG）。
///
/// 为什么用 `ring::rand::SystemRandom` 而不是 `uuid::new_v4()` 拼两下：
/// 这把 key 是**客户端长期持有的凭据**，要的就是「口令学随机」，而 `ring`
/// 已经在这个二进制里（TLS 在用，见 `Cargo.toml` 的登记）——
/// 复用它的 CSPRNG 比从 UUID 里凑 32 字节更直白。
pub fn generate_key() -> Result<String, OpError> {
    use ring::rand::SecureRandom;
    let mut buf = [0u8; 32];
    ring::rand::SystemRandom::new().fill(&mut buf).map_err(|_| {
        OpError::internal("生成 key 时拿不到随机数（SystemRandom 失败）—— 这不是参数问题。")
    })?;
    Ok(format!("ontok_q_{}", hex_lower(&buf)))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// 一个标签（`label` / `key_label`）的清洗：**空 ⇒ None**，控制字符 ⇒ 拒。
fn clean_label(raw: Option<&str>, what: &'static str, required: bool) -> Result<Option<String>, OpError> {
    let t = raw.map(str::trim).unwrap_or("");
    if t.is_empty() {
        if required {
            return Err(OpError::bad_request(
                "project.label.required",
                format!("{what} 是必填的，收到空值。"),
                "v1 是**一项目一把 key**，所以这把 key 是给谁用的必须写下来（审计归属）——\
                 demand §十四-4 的已知限制就靠这一栏兜。",
            ));
        }
        return Ok(None);
    }
    if t.chars().count() > 200 {
        return Err(OpError::bad_request(
            "project.label.too_long",
            format!("{what} 超过 200 字（{} 字）。", t.chars().count()),
            "标签是给人读的短名字（「运维助手」「fde-michael」），不是说明文档。",
        ));
    }
    if t.chars().any(|c| c.is_control()) {
        return Err(OpError::bad_request(
            "project.label.invalid",
            format!("{what} 里含控制字符。"),
            "标签会进审计行（一行一条），控制字符会把那一行撕开。",
        ));
    }
    Ok(Some(t.to_string()))
}

// ══════════════════════════════════════════════════════════════
// ⓪ GET /api/projects —— 列表（Studio 顶栏的项目下拉）
// ══════════════════════════════════════════════════════════════

/// 列表里的一行 —— **Studio 顶栏直接吃这个形状**（REQ-159 P1c-2）。
///
/// ⛔ 这里没有 DSN、没有 key、没有密文，也没有项目 `id`：
/// 下拉只需要「叫什么 / 给谁看 / 第几版 / 多大」，多一个字段就是多一条泄漏面。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectListEntry {
    /// 项目名（Studio 切换时作为 `?project=` 的值）。
    pub name: String,
    /// 项目备注（`POST /api/projects` 的 `label`）；没写 ⇒ `null`。
    pub label: Option<String>,
    /// 生效版本的 `seq`；**还没激活过任何一版 ⇒ `null`**（⛔ 不是 0、不是报错）。
    pub active_version: Option<i32>,
    /// 生效版本是否已冻结（激活必然冻结；没有生效版本 ⇒ `false`）。
    pub frozen: bool,
    /// 生效版本快照里的对象类型数（没有生效版本 ⇒ 0）。
    pub entity_types: i64,
    /// 生效版本快照里的关系类型数（没有生效版本 ⇒ 0）。
    pub link_types: i64,
}

/// `GET /api/projects` 的回执（只包一层 `projects`：形状定死，前端不许再猜）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectListOutcome {
    pub projects: Vec<ProjectListEntry>,
}

/// `GET /api/projects` —— 全部项目（**admin 才能调**，门在 [`crate::query::http`]）。
///
/// # 三条实现约束（都是判据）
///
/// 1. **一条 SQL**：`projects LEFT JOIN ontology_versions` + 两个 `count(*)` 子查询。
///    ⛔ 不逐个项目调 [`PgStore::get_snapshot`] —— 那会对每个项目跑 5 条查询并把整份
///    快照反序列化一遍，只为拿两个数字。
/// 2. **没有生效版本不报错**：`active_version_id IS NULL` ⇒ `seq` 为 `null`、
///    `frozen` 为 `false`，两个计数子查询自然为 0（`version_id = NULL` 匹配不到行）。
///    ⛔ 不返回 503 `project.no_active_version` —— 那会让「刚建好的项目」在下拉里消失。
/// 3. **计数取生效版本的快照**（`object_types` / `link_types` 的**行**），
///    不是包里 `manifest` 的声明：要么是这一版真在服务的内容，要么是 0。
pub async fn list_projects(pool: &PgPool) -> Result<ProjectListOutcome, OpError> {
    let rows: Vec<(String, Option<String>, Option<i32>, bool, i64, i64)> = sqlx::query_as(
        "SELECT p.name,
                p.label,
                v.seq,
                (v.frozen_at IS NOT NULL) AS frozen,
                (SELECT count(*) FROM object_types o
                  WHERE o.version_id = p.active_version_id) AS entity_types,
                (SELECT count(*) FROM link_types l
                  WHERE l.version_id = p.active_version_id) AS link_types
           FROM projects p
           LEFT JOIN ontology_versions v ON v.id = p.active_version_id
          ORDER BY p.name",
    )
    .fetch_all(pool)
    .await
    .map_err(OpError::db)?;

    Ok(ProjectListOutcome {
        projects: rows
            .into_iter()
            .map(
                |(name, label, active_version, frozen, entity_types, link_types)| {
                    ProjectListEntry {
                        name,
                        label,
                        active_version,
                        frozen,
                        entity_types,
                        link_types,
                    }
                },
            )
            .collect(),
    })
}

/// `POST /api/projects`。
pub async fn create_project(
    pool: &PgPool,
    cipher: &SecretCipher,
    project: &str,
    label: Option<&str>,
    key_label: Option<&str>,
) -> Result<CreateProjectOutcome, OpError> {
    let project = project.trim();
    if !bundle::valid_project_name(project) {
        return Err(OpError::bad_request(
            "project.name.invalid",
            format!("项目名 {project:?} 不合法（要求 `^[a-z0-9][a-z0-9_-]{{1,62}}$`）。"),
            "小写字母 / 数字 / 连字符 / 下划线，2–63 字符，且不能以连字符或下划线开头 —— \
             与 manifest 里 `project` 的 pattern 是**同一条**（包要用这个名字校验，两边必须一致）。\
             ⭐ 2026-10-27 起**允许下划线**：库里既有项目名大量带 `_`，禁掉它等于让历史项目打不了包。",
        ));
    }
    let label = clean_label(label, "`label`（项目备注）", false)?;
    let key_label = clean_label(key_label, "`key_label`（这把 key 给谁用）", true)?
        .expect("required = true 时一定 Some");

    let key = generate_key()?;
    let digest = crate::query::audit::sha256_hex(key.as_bytes());
    // ⭐ 契约变更：**摘要 + 可逆密文**两份都写（查看端点要解密，鉴权只查摘要）。
    let ciphertext = cipher
        .encrypt(&key)
        .map_err(|e| OpError::internal(format!("query key 加密失败：{e}")))?;

    let mut tx = pool.begin().await.map_err(OpError::db)?;
    // ① 项目行（撞名 ⇒ 409 project.exists）
    let row = match PgProjectStore::create_labeled_in(&mut tx, project, label.as_deref()).await {
        Ok(r) => r,
        Err(ProjectError::NameTaken(_)) => {
            return Err(OpError::conflict(
                "project.exists",
                format!("项目 {project:?} 已经存在。"),
                "项目名就是包的防串包凭据，不能重名。要给已有项目换 key，\
                 用 `POST /api/projects/:p/key/rotate`（P1a 未实现，见交付的已知限制）。",
            ))
        }
        Err(e) => return Err(OpError::db(e)),
    };
    // ② key 行（摘要 + 密文）
    let key_row =
        PgBundleStore::create_key_in(&mut tx, row.id, &digest, &ciphertext, &key_label)
            .await
            .map_err(OpError::db)?;
    tx.commit().await.map_err(OpError::db)?;

    Ok(CreateProjectOutcome {
        project: row.name.clone(),
        label,
        key,
        key_kind: "query",
        key_label: key_row.label,
        // 与既有 token 审计同形：`sha256:` + 12 位（够人眼对账，不够反推）。
        key_digest: format!(
            "sha256:{}",
            crate::query::audit::digest_prefix(&digest, 12)
        ),
        bound_project: row.name,
        created_at: rfc3339(key_row.created_at),
    })
}

// ══════════════════════════════════════════════════════════════
// ①b GET /api/projects/:p/key —— **再次查看**（契约变更 2026-10-27 10:25）
// ══════════════════════════════════════════════════════════════

/// 查看 key 的回执。字段与契约给的一致（`revoked_at` 存在是为了让「这把已经作废」
/// 这件事在响应里**可见**，而不是要靠别处推断）。
#[derive(Debug, Clone, Serialize)]
pub struct RevealKeyOutcome {
    pub project: String,
    /// ⚠️ 明文。**只有 admin token 能拿到**，而且每次调用都写一条审计（AC-012）。
    pub key: String,
    pub label: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

/// `GET /api/projects/:p/key` 的领域实现。
///
/// # 三条硬要求（契约）
///
/// 1. **admin token 才能调** —— 由既有身份门保证（`query` token 一律 403，
///    且本路径的 403 码是 `key.not_admin`，见 `console::http::require_token`）；
/// 2. **每次调用写一条审计** —— `action=reveal_key`，那是「能查看」的唯一补偿；
/// 3. ⛔ 响应 / 日志 / 审计里**不出现** ciphertext 与密钥本身。
///
/// 还有一条是这里自己加的**自检**：解出来的明文，其 `sha256` 必须等于
/// `token_sha256` 那一列。密文与摘要是同一条 SQL 写的，正常永远相等；
/// 不相等说明这行被人改过（或密文串了项目）—— 那时**宁可 500 也不返回一把
/// 可能不是这个项目的 key**。
pub async fn reveal_key(
    pool: &PgPool,
    cipher: &SecretCipher,
    project: &str,
) -> Result<RevealKeyOutcome, OpError> {
    let p = project_row(pool, project).await?;
    let store = PgBundleStore::new(pool.clone());
    let row = store
        .active_key(p.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::not_found(
                "key.not_found",
                format!("项目 {:?} 没有有效的 key（没建过，或已被吊销）。", p.name),
                "key 在 `POST /api/projects` 时生成。⛔ 没有 key 不代表可以「补发一把」——\
                 补发等于换 key，要走 `rotate`（P1a 未实现）。",
            )
        })?;

    let key = cipher.decrypt(&row.token_ciphertext).map_err(|e| {
        // 解密失败**不是** 400：「这把密钥解不开这条密文」是服务端状态问题。
        OpError::internal(format!(
            "项目 {:?} 的 key 密文解不开：{e}",
            p.name
        ))
    })?;
    let got = crate::query::audit::sha256_hex(key.as_bytes());
    if got != row.token_sha256 {
        return Err(OpError::internal(format!(
            "项目 {:?} 的 key 密文与摘要不是同一把（解出来的摘要 {got}，库里的 {}）—— \
             拒绝返回一把可能不属于这个项目的 key。",
            p.name, row.token_sha256
        )));
    }

    Ok(RevealKeyOutcome {
        project: p.name,
        key,
        label: row.label,
        created_at: rfc3339(row.created_at),
        revoked_at: row.revoked_at.map(rfc3339),
    })
}

// ══════════════════════════════════════════════════════════════
// ② POST /api/projects/:p/bundles
// ══════════════════════════════════════════════════════════════

/// 上传成功后的回执。
#[derive(Debug, Clone, Serialize)]
pub struct UploadOutcome {
    pub bundle_id: String,
    pub project: String,
    pub state: &'static str,
    pub requires_db: bool,
    pub components: Value,
    pub counts: Counts,
    pub sha256_verified: bool,
    pub bytes: i64,
    pub bundle_sha256: String,
    pub uploaded_at: String,
}

/// 上传：**只校验不解析**（⛔ 不建版本、不写快照）。
///
/// 校验顺序由 [`bundle::verify_and_read`] 收口（体积 → zip → 路径 → manifest →
/// files[] sha256 → project 同名 → 计数自洽）。
pub async fn upload_bundle(
    pool: &PgPool,
    project: &str,
    file: Vec<u8>,
) -> Result<UploadOutcome, OpError> {
    let p = project_row(pool, project).await?;
    let verified = bundle::verify_and_read(file, &p.name).map_err(fail)?;

    let store = PgBundleStore::new(pool.clone());
    let bundle_bytes = verified.bytes.len() as i64;
    let row = store
        .create_bundle(
            p.id,
            &verified.manifest.raw,
            &verified.bundle_sha256,
            bundle_bytes,
            &verified.bytes,
        )
        .await
        .map_err(OpError::db)?;

    Ok(UploadOutcome {
        bundle_id: bundle_id_text(row.id),
        project: p.name,
        state: State::Uploaded.as_str(),
        requires_db: verified.requires_dsn(),
        components: verified
            .manifest
            .raw
            .get("components")
            .cloned()
            .unwrap_or(Value::Null),
        counts: counts_of(&verified.snapshot),
        sha256_verified: true,
        bytes: bundle_bytes,
        bundle_sha256: verified.bundle_sha256.clone(),
        uploaded_at: rfc3339(row.uploaded_at),
    })
}

// ══════════════════════════════════════════════════════════════
// ③ GET /api/projects/:p/bundles/:id
// ══════════════════════════════════════════════════════════════

/// 包的状态与组件清单。
#[derive(Debug, Clone, Serialize)]
pub struct BundleStatus {
    pub bundle_id: String,
    pub project: String,
    pub state: String,
    pub requires_db: bool,
    pub dialect: Option<String>,
    pub bytes: i64,
    pub bundle_sha256: String,
    /// 上传时已逐个核过 sha256 才会落库 ⇒ 这条恒为 `true`；
    /// 留着它是为了让 UI 的「完整性」一栏有确定的来源（而不是靠「没报错」推断）。
    pub sha256_verified: bool,
    pub uploaded_at: String,
    pub components: Value,
    pub files: Vec<bundle::ManifestFile>,
    pub manifest: Value,
    pub imported_version: Option<i32>,
    pub imported_at: Option<String>,
    pub error: Option<Value>,
}

pub async fn bundle_status(
    pool: &PgPool,
    project: &str,
    bundle_id: &str,
) -> Result<BundleStatus, OpError> {
    let p = project_row(pool, project).await?;
    let id = parse_bundle_id(bundle_id)?;
    let store = PgBundleStore::new(pool.clone());
    let row = store
        .get_bundle_in_project(id, p.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| bundle_not_found(bundle_id, &p.name))?;
    let manifest = bundle::parse_manifest(&row.manifest).map_err(fail)?;

    let imported_version = match row.imported_version_id {
        Some(v) => Some(version_seq(pool, v).await?),
        None => None,
    };
    let error = match (&row.error_code, &row.error_message) {
        (Some(c), Some(m)) => Some(json!({"code": c, "message": m})),
        _ => None,
    };

    Ok(BundleStatus {
        bundle_id: bundle_id_text(row.id),
        project: p.name,
        state: row.state,
        requires_db: manifest.requires_dsn(),
        dialect: manifest.dialect().map(str::to_string),
        bytes: row.bytes,
        bundle_sha256: row.bundle_sha256,
        sha256_verified: true,
        uploaded_at: rfc3339(row.uploaded_at),
        components: manifest
            .raw
            .get("components")
            .cloned()
            .unwrap_or(Value::Null),
        files: manifest.files,
        manifest: manifest.raw,
        imported_version,
        imported_at: row.imported_at.map(rfc3339),
        error,
    })
}

fn bundle_not_found(raw: &str, project: &str) -> OpError {
    OpError::not_found(
        "bundle.not_found",
        format!("项目 {project:?} 下没有包 {raw:?}。"),
        "包 id 是 `POST /api/projects/:p/bundles` 的响应里的 `bundle_id`（`bdl_<uuid>`）。\
         一个包只属于上传它的那个项目 —— 拿别的项目的 id 在这里也是 404。",
    )
}

async fn version_seq(pool: &PgPool, version_id: Uuid) -> Result<i32, OpError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT seq FROM ontology_versions WHERE id = $1")
            .bind(version_id)
            .fetch_optional(pool)
            .await
            .map_err(OpError::db)?;
    row.map(|r| r.0).ok_or_else(|| {
        OpError::internal("包的 `imported_version_id` 指向一个不存在的版本行 —— 元数据库不自洽。")
    })
}

// ══════════════════════════════════════════════════════════════
// ④ PUT /api/projects/:p/database
// ══════════════════════════════════════════════════════════════

/// 配 DSN 的回执：⛔ **没有** `dsn` 字段（AC-009）。
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseOutcome {
    pub database_id: String,
    pub project: String,
    pub dialect: String,
    pub host: String,
    pub database: String,
    pub user: String,
    pub updated_at: String,
    /// 三态：换 DSN 后回到 `null`（旧结论对新 DSN 不成立）。
    pub last_check_ok: Option<bool>,
}

/// 存 DSN（**加密列**）。
pub async fn put_database(
    pool: &PgPool,
    cipher: &SecretCipher,
    project: &str,
    dsn: &str,
    dialect: &str,
) -> Result<DatabaseOutcome, OpError> {
    let p = project_row(pool, project).await?;
    let dialect = dialect.trim().to_ascii_lowercase();
    if dialect != "postgresql" {
        return Err(OpError::bad_request(
            "db.dialect.unsupported",
            format!("dialect = {dialect:?}，v1 只支持 `postgresql`。"),
            "连通性探测走的是 sqlx 的 PostgreSQL 驱动：认下 mysql / sqlserver 的 DSN 却测不了，\
             等于给用户一个假承诺。包里的 `components.db.dialect` 仍可选那几种（描述用），\
             但**配 DSN 并测连通**在 v1 只有 postgresql。",
        ));
    }
    let parts = dsn::parse_dsn(dsn).map_err(|e| {
        OpError::bad_request(
            "db.dsn.invalid",
            // ⚠️ scrub：错误文本里可能出现整条 DSN（AC-009）
            cipher.scrub(&e.to_string(), dsn),
            "形如 `postgresql://user:password@host:5432/dbname`（口令里的特殊字符要百分号转义）。",
        )
    })?;
    let ct = cipher.encrypt(dsn).map_err(|e| {
        OpError::internal(format!("DSN 加密失败：{e}"))
    })?;

    let store = PgBundleStore::new(pool.clone());
    let row = store
        .upsert_database(p.id, &dialect, &ct)
        .await
        .map_err(OpError::db)?;

    Ok(DatabaseOutcome {
        // 表以 `project_id` 为主键（一项目一条 DSN），所以 id 是它的**稳定派生**。
        database_id: format!("db_{}", p.id),
        project: p.name,
        dialect: row.dialect,
        host: parts.host,
        database: parts.database,
        user: parts.user,
        updated_at: rfc3339(row.updated_at),
        last_check_ok: row.last_check_ok,
    })
}

// ══════════════════════════════════════════════════════════════
// ⑤ POST /api/projects/:p/database/test
// ══════════════════════════════════════════════════════════════

/// 测连通性：**成功与失败都返 200**（AC-005：让 UI 能区分「测过了但不通」）。
pub async fn test_database(
    pool: &PgPool,
    cipher: &SecretCipher,
    project: &str,
) -> Result<DsnTest, OpError> {
    let p = project_row(pool, project).await?;
    let store = PgBundleStore::new(pool.clone());
    let row = store
        .get_database(p.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| {
            OpError::new(
                StatusCode::NOT_FOUND,
                "db.dsn.missing",
                format!("项目 {:?} 还没配 DSN。", p.name),
                "先 `PUT /api/projects/:p/database` 存一条 DSN，再来测 —— \
                 测的对象是**已存的那一条**，不是请求体里的字符串（那样测完什么都没留下）。",
            )
        })?;

    let plain = cipher
        .decrypt(&row.dsn_ciphertext)
        .map_err(|e| OpError::internal(format!("DSN 解密失败：{e}")))?;
    let outcome = dsn::probe(&plain, cipher).await;
    // 结果落库（失败也记）—— `import` 只认 `last_check_ok = true`。
    store
        .note_check(
            p.id,
            outcome.ok,
            outcome.error.as_ref().map(|e| e.code),
        )
        .await
        .map_err(OpError::db)?;
    Ok(outcome)
}

// ══════════════════════════════════════════════════════════════
// ⑥ POST /api/projects/:p/bundles/:id/import
// ══════════════════════════════════════════════════════════════

/// 解析入库的回执。
#[derive(Debug, Clone, Serialize)]
pub struct ImportOutcome {
    pub bundle_id: String,
    pub project: String,
    pub version: i32,
    pub version_id: Uuid,
    pub state: &'static str,
    /// ⛔ **恒为 false**：import 从不冻结（冻结在 activate）。写出来是为了让
    /// 「解析成功 ≠ 对外生效」这条约束在回执里**可见**。
    pub frozen: bool,
    pub counts: Counts,
    pub warnings: Vec<String>,
    /// 同一个包重复 import ⇒ `true`，且**不重复建版本**（AC-008）。
    pub already_imported: bool,
}

/// ⭐ REQ-174 · 包内 AM 的**已校验原文**（`None` = 包里没有 `components.am`）。
///
/// 为什么先校验再建版本：`AccessMapping::parse` → `bind(这一版的模型)` 失败时
/// **一个版本都不该建**（AC-006）。把校验放在 `write_new_version` 的事务**之前**，
/// 「版本都不建」就是结构上的事实，而不是靠一次 rollback 兜。
struct ValidatedAm {
    /// `am/access-mapping.json` 的**原样字节**（UTF-8）—— 落库后必须能逐字节比回来。
    raw: String,
    /// 与 manifest 的 `components.am.sha256` **同一个口径**（包内文件字节的 sha256）。
    sha256: String,
    /// `sources[]` 的声明条数（与 manifest 同口径）。
    sources: usize,
    /// `entities[]` 的条数。
    entities: usize,
}

/// 从**已整包校验**的包取 `components.am` 的原文，并绑定到这一版快照的模型。
///
/// # 三条边界
///
/// - 包里没有 `components.am` ⇒ `Ok(None)`：不写行、不报错（AC-002）；
/// - 声明了但文件不在 / 不是 UTF-8 ⇒ 500（`verify_and_read` 已核过 `files[]`，
///   走到这里还缺就是**数据不自洽**，不是包的形状问题）；
/// - `parse` / `bind` 失败 ⇒ 422 `am.invalid`，消息**只说位置与原因，⛔ 不回显 AM 原文**
///   （AC-008：`raw` 里有 `base_url` / `credential_env` 之类的部署信息）。
fn validated_am(verified: &VerifiedBundle) -> Result<Option<ValidatedAm>, OpError> {
    let Some(decl) = verified.manifest.am.as_ref() else {
        return Ok(None);
    };
    let bytes = verified.files.get(&decl.path).ok_or_else(|| {
        OpError::internal(format!(
            "包的 `components.am.path` = {:?} 不在包内文件里 —— 整包校验漏了一步（这是 bug）。",
            decl.path
        ))
    })?;
    let sha256 = crate::query::audit::sha256_hex(bytes);
    if sha256 != decl.sha256 {
        // `verify_and_read` 的 ⑤ 逐个核过 sha256；走到这里说明两处口径分了家。
        return Err(OpError::internal(format!(
            "包内 AM {:?} 的 sha256 与 manifest 声明的不一致（{} vs {}）—— \
             校验层与这里不是同一个口径（这是 bug）。",
            decl.path, sha256, decl.sha256
        )));
    }
    let raw = std::str::from_utf8(bytes)
        .map_err(|e| am_invalid(format!("包里的 AM 不是 UTF-8：{e}")))?
        .to_string();
    let model = crate::domain::model_view::project_model_track(&verified.snapshot);
    let parsed = AccessMapping::parse(&raw)
        .and_then(|m| m.bind(&model))
        .map_err(|e| am_invalid(format!("包里的 AM 不合法：{e}")))?;
    Ok(Some(ValidatedAm {
        raw,
        sha256,
        sources: parsed.source_count(),
        entities: parsed.api_entities().len(),
    }))
}

/// 包内 AM 不合法的统一出口（`am.invalid`，422）。
fn am_invalid(message: String) -> OpError {
    OpError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "am.invalid",
        message,
        "包里的 `am/access-mapping.json` 必须能解析，且 `entities[].class` 都要在**包里这份本体**\
         里存在；`contract_version` 只认 1 / 2。\
         ⛔ 本次 import 没有产生任何版本（AM 校验在**建版本之前**）。",
    )
}

/// 解析入库：**单事务**，产生**新版本**，**不冻结**。
pub async fn import_bundle(
    pool: &PgPool,
    project: &str,
    bundle_id: &str,
) -> Result<ImportOutcome, OpError> {
    let p = project_row(pool, project).await?;
    let id = parse_bundle_id(bundle_id)?;
    let store = PgBundleStore::new(pool.clone());
    let row = store
        .get_bundle_in_project(id, p.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| bundle_not_found(bundle_id, &p.name))?;

    // ── 幂等：这个包已经产生过版本 ⇒ 返回**已有结果**（AC-008）。
    if let Some(vid) = row.imported_version_id {
        let snap = PgStore::new(pool.clone())
            .get_snapshot(vid)
            .await
            .map_err(OpError::db)?
            .ok_or_else(|| {
                OpError::internal(
                    "包的 `imported_version_id` 指向一个没有快照的版本 —— 元数据库不自洽。",
                )
            })?;
        let (seq, frozen_at) = version_frozen(pool, vid).await?;
        return Ok(ImportOutcome {
            bundle_id: bundle_id_text(row.id),
            project: p.name,
            version: seq,
            version_id: vid,
            state: row_state_or(&row.state, State::Imported),
            frozen: frozen_at.is_some(),
            counts: counts_of(&snap),
            warnings: warnings_for(&snap, None),
            already_imported: true,
        });
    }

    // ── 包字节：先核**整包** sha256（库里那一行说的与现在读到的是不是同一份）
    let data = store.payload(row.id).await.map_err(OpError::db)?.ok_or_else(|| {
        OpError::internal("包的字节行不见了（`project_bundles` 有行、payload 没有）。")
    })?;
    let got = crate::query::audit::sha256_hex(&data);
    if got != row.bundle_sha256 {
        let e = BundleError {
            status: StatusCode::CONFLICT,
            code: "bundle.sha256.mismatch",
            message: format!(
                "包 {bundle_id} 的整包 sha256 与上传时记下的不符：库里 {}，现在 {got}。",
                row.bundle_sha256
            ),
            hint: "包字节在库里被改过（或换了一把 sha256 实现）。⛔ 拒绝解析：\
                  一个说不清自己是什么的包不该进本体库。".into(),
        };
        note_failure(&store, row.id, &e).await;
        return Err(fail(e));
    }

    // ── 重新整包校验：**不信**库里的 manifest 列，重新从字节里读一份
    //    （顺带把快照反序列化出来；失败 ⇒ 422 bundle.parse.failed）
    let verified = match bundle::verify_and_read(data, &p.name) {
        Ok(v) => v,
        Err(e) => {
            note_failure(&store, row.id, &e).await;
            return Err(fail(e));
        }
    };

    // ── `requires_dsn = true` 的闸（AC-004：422 且**一个版本都不产生**）
    if verified.requires_dsn() {
        match store.get_database(p.id).await.map_err(OpError::db)? {
            None => {
                let e = dsn_missing(&p.name, "还没配 DSN");
                store
                    .set_state(
                        row.id,
                        State::DbPending.as_str(),
                        Some((e.code, &e.message)),
                    )
                    .await
                    .map_err(OpError::db)?;
                return Err(fail(e));
            }
            Some(db) if db.last_check_ok != Some(true) => {
                let e = dsn_missing(
                    &p.name,
                    &format!(
                        "DSN 已配置但**没测通**（last_check_ok = {}）",
                        match db.last_check_ok {
                            None => "null（从没测过）".to_string(),
                            Some(false) => "false（上次测不通）".to_string(),
                            Some(true) => "true".to_string(),
                        }
                    ),
                );
                store
                    .set_state(
                        row.id,
                        State::DbPending.as_str(),
                        Some((e.code, &e.message)),
                    )
                    .await
                    .map_err(OpError::db)?;
                return Err(fail(e));
            }
            Some(_) => {}
        }
    }

    // ── ⭐ REQ-174 · 包内 AM：**先校验，再建版本**（AC-006 的「版本都不建」）
    //
    // 放在 `write_new_version` 之前是有意的：这里失败时**一个事务都还没开**，
    // 所以「没有新版本」不是靠 rollback 兜出来的，而是根本没有走建版本那一步。
    // 落库（写 `version_access_mappings`）仍在 `write_new_version` 的事务里，
    // 于是「版本建了、AM 没写」也不可能出现。
    let am = match validated_am(&verified) {
        Ok(v) => v,
        Err(e) => {
            // 与其它 import 失败同一条：包状态记 `failed`（**不**产生版本）。
            // `e.message` 里只有位置与原因，⛔ 没有 AM 原文（AC-008）。
            let _ = store
                .set_state(row.id, State::Failed.as_str(), Some((e.code, &e.message)))
                .await;
            return Err(e);
        }
    };

    // ── 落库：**单事务**（失败 ⇒ 一个字节都不落）
    let result = write_new_version(pool, p.id, row.id, &verified, am.as_ref()).await;
    let version_id = match result {
        Ok(v) => v,
        Err(e) => {
            // ⚠️ 事务已在 `write_new_version` 内部回滚 —— 这里只把**包的元数据**
            // 标成 failed（那不是「残留」：AC-006 判的是 ontology_versions /
            // object_types / link_types 没有新行）。
            let code = e.code;
            store
                .set_state(row.id, State::Failed.as_str(), Some((code, &e.message)))
                .await
                .map_err(OpError::db)?;
            return Err(e);
        }
    };

    let (seq, _) = version_frozen(pool, version_id).await?;
    Ok(ImportOutcome {
        bundle_id: bundle_id_text(row.id),
        project: p.name,
        version: seq,
        version_id,
        state: State::Imported.as_str(),
        frozen: false,
        counts: counts_of(&verified.snapshot),
        warnings: warnings_for(&verified.snapshot, Some(&verified)),
        already_imported: false,
    })
}

fn row_state_or(saved: &str, fallback: State) -> &'static str {
    State::parse(saved).map(State::as_str).unwrap_or(fallback.as_str())
}

async fn version_frozen(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<(i32, Option<chrono::DateTime<chrono::Utc>>), OpError> {
    let row: Option<(i32, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT seq, frozen_at FROM ontology_versions WHERE id = $1")
            .bind(version_id)
            .fetch_optional(pool)
            .await
            .map_err(OpError::db)?;
    row.ok_or_else(|| OpError::internal("版本行不见了 —— 元数据库不自洽。"))
}

fn dsn_missing(project: &str, why: &str) -> BundleError {
    BundleError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "db.dsn.missing",
        message: format!(
            "包声明了 `requires_dsn = true`，但项目 {project:?} 的 DSN 不可用：{why}。"
        ),
        hint: "先 `PUT /api/projects/:p/database` 存 DSN，再 `POST /api/projects/:p/database/test` \
               测通（import 只认**测通过**的 DSN：配了但没测，等于没验证过它可用），然后再 import。\
               ⛔ 本次 import 没有产生任何版本。"
            .into(),
    }
}

async fn note_failure(store: &PgBundleStore, id: Uuid, e: &BundleError) {
    // 失败标记本身失败不该覆盖原来的错误（那是次生故障），所以忽略结果。
    let _ = store
        .set_state(id, State::Failed.as_str(), Some((e.code, &e.message)))
        .await;
}

/// 一个包**最多**能给出的警告（不是错误：包没问题，但有些内容 v1 不入库）。
fn warnings_for(
    snap: &crate::domain::snapshot::Snapshot,
    verified: Option<&VerifiedBundle>,
) -> Vec<String> {
    let mut out = Vec::new();
    if snap.action_types.is_empty() && snap.functions.is_empty() {
        // 不警告：空集合是正常形态（切片 1 的占位）。
    }
    if let Some(v) = verified {
        // ⭐ REQ-174 · 声明了 `components.am` ⇒ AM 随版本入库（不再警告）。
        // 只有「有 `am/` 文件却没声明组件」才是**没被落库**的内容 —— 用户有权知道。
        // （判据看**包里的文件**，不看 `components.am` 声明：一个包可以只把 AM 放进
        // `files[]` 而不声明组件，那时它照样是一份没被落库的内容。）
        if v.manifest.am.is_none() && v.files.keys().any(|k| k.starts_with("am/")) {
            out.push(
                "包里有 `am/`（访问映射），但 manifest 没有 `components.am` ⇒ 本次只落本体快照，\
                 那份 AM **不会**随版本入库。要让它入库，请用 `bundle pack --am <文件>` 重新打包。"
                    .to_string(),
            );
        }
        if let Some(db) = &v.manifest.db {
            if !db.requires_dsn {
                out.push(
                    "包里有 `db/schema.json` 但 `requires_dsn = false` —— 按 demand §七-6，\
                     这种情况**不要求**配 DSN（源库描述只作参考）。"
                        .to_string(),
                );
            }
        }
    }
    out
}

/// 真正写库的那一段：**一个事务**，建版本 + 写快照五行 + 包的元数据。
///
/// # 为什么包的元数据也在同一个事务里
///
/// 「版本建出来了，但包还显示 uploaded」会让同一个包被**再导一次**（`imported_version_id`
/// 还是 NULL）⇒ 两个版本描述同一份内容。所以三件事要么都成，要么都不成。
async fn write_new_version(
    pool: &PgPool,
    project_id: Uuid,
    bundle_row_id: Uuid,
    verified: &VerifiedBundle,
    am: Option<&ValidatedAm>,
) -> Result<Uuid, OpError> {
    let mut tx = pool.begin().await.map_err(OpError::db)?;

    // ① 建版本（**共用** `proposal apply` 的那一份：锁项目 + max(seq)+1）
    let new = match crate::db::apply::begin_new_version(&mut tx, project_id).await {
        Ok(v) => v,
        Err(e) => {
            tx.rollback().await.ok();
            return Err(pack_db_error(e));
        }
    };

    // ② 快照：**重新分配** version_id / seq（包里的只是构建端标识）
    let mut snap = verified.snapshot.clone();
    snap.version_id = new.version_id;
    snap.seq = new.seq;

    // ③ 写快照五行 —— 走 `proposal apply` 的同一个 `write_snapshot_in`
    if let Err(e) = crate::db::apply::write_snapshot_in(&mut tx, &snap).await {
        // ⛔ 回滚：一个字节不落（AC-006）
        tx.rollback().await.ok();
        return Err(pack_db_error(e));
    }

    // ③b ⭐ REQ-174 · 包内 AM（已校验）：与版本**同一事务**。
    //
    // 写入失败 ⇒ 连版本一起回滚（AC-006）。这里写的只是字节 + 摘要 + 两个计数，
    // 解析 / 绑定已在事务之前做过（见 [`validated_am`]）。
    if let Some(am) = am {
        if let Err(e) = PgAccessStore::upsert_in(
            &mut tx,
            new.version_id,
            &am.raw,
            &am.sha256,
            am.sources,
            am.entities,
        )
        .await
        {
            tx.rollback().await.ok();
            return Err(OpError::db(e));
        }
    }

    // ④ 包的元数据（同一个事务；SQL 的唯一来源在 `db::bundle_store`）
    if let Err(e) =
        crate::db::bundle_store::PgBundleStore::mark_imported_in(&mut tx, bundle_row_id, new.version_id)
            .await
    {
        tx.rollback().await.ok();
        return Err(OpError::db(e));
    }

    if let Err(e) = tx.commit().await {
        return Err(OpError::db(e));
    }
    Ok(new.version_id)
}

/// `StoreError` → 可行动的错误（import 的失败面）。
fn pack_db_error(e: crate::db::pg::StoreError) -> OpError {
    let msg = e.to_string();
    // 唯一约束冲突（23505）在**同一个 version_id 内** = 包里有重复 key。
    // 这不是「服务端坏了」，是包的内容自相矛盾 ⇒ 422（与 parse.failed 同一档）。
    if msg.contains("duplicate key") || msg.contains("unique") {
        return OpError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "bundle.parse.failed",
            format!("解析入库时撞上唯一约束（包里有重复的 key）：{msg}"),
            "同一版里的 `entity_types` / `link_types` / `action_types` / `functions` / \
             `concept_groups` 的 key 必须唯一。⛔ 事务已回滚，一个字节都没落。",
        );
    }
    OpError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "bundle.import.failed",
        format!("解析入库失败（事务已回滚）：{msg}"),
        "这是服务端错误，不是包的形状问题。用户可见的效果是**零残留**（没有新版本、\
         没有新的对象行）；把出现的时间报给维护者。",
    )
}

// ══════════════════════════════════════════════════════════════
// ⑦ POST /api/projects/:p/bundles/:id/activate
// ══════════════════════════════════════════════════════════════

/// 人工激活的回执。
#[derive(Debug, Clone, Serialize)]
pub struct ActivateOutcome {
    pub bundle_id: String,
    pub project: String,
    pub version: i32,
    pub version_id: Uuid,
    pub frozen: bool,
    pub frozen_at: Option<String>,
    pub active_version_id: Uuid,
    pub state: &'static str,
}

/// 人工激活：**冻结该版本 + 写 `projects.active_version_id`**（同一事务）。
///
/// # 为什么是独立一步
///
/// demand §四-3：「切流量（`active`）是**独立一步**：解析成功 ≠ 对外生效」。
/// 解析失败不阻塞已有版本继续服务，激活也一样 —— 它只动**新**版本与指针。
pub async fn activate_bundle(
    pool: &PgPool,
    project: &str,
    bundle_id: &str,
) -> Result<ActivateOutcome, OpError> {
    let p = project_row(pool, project).await?;
    let id = parse_bundle_id(bundle_id)?;
    let store = PgBundleStore::new(pool.clone());
    let row = store
        .get_bundle_in_project(id, p.id)
        .await
        .map_err(OpError::db)?
        .ok_or_else(|| bundle_not_found(bundle_id, &p.name))?;

    let Some(version_id) = row.imported_version_id else {
        return Err(OpError::conflict(
            "bundle.not_imported",
            format!(
                "包 {bundle_id} 还没解析入库（状态 {}）—— 没有版本可激活。",
                row.state
            ),
            "先 `POST /api/projects/:p/bundles/:id/import`（它会产出新版本），\
             再 `activate`。⛔ 解析与生效是两步：中间那一步是给人确认的。",
        ));
    };

    let frozen = PgProjectStore::new(pool.clone())
        .activate(p.id, version_id)
        .await
        .map_err(|e| match e {
            ProjectError::VersionNotFound { .. } => OpError::not_found(
                "bundle.version.not_found",
                format!("包 {bundle_id} 指向的版本行不存在。"),
                "包的 `imported_version_id` 与 `ontology_versions` 不一致 —— 报给维护者。",
            ),
            other => OpError::db(other),
        })?;

    store
        .set_state(row.id, State::Activated.as_str(), None)
        .await
        .map_err(OpError::db)?;

    Ok(ActivateOutcome {
        bundle_id: bundle_id_text(row.id),
        project: p.name,
        version: frozen.seq,
        version_id,
        frozen: frozen.frozen_at.is_some(),
        frozen_at: frozen.frozen_at.map(rfc3339),
        active_version_id: version_id,
        state: State::Activated.as_str(),
    })
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ **AC-d1 的形状判据**：`GET /api/projects` 的一行**只有那 6 个字段**
    /// （`name` / `label` / `active_version` / `frozen` / `entity_types` / `link_types`）。
    ///
    /// 为什么要在**序列化**这一层钉：这个回执是 Studio 顶栏下拉的数据源，而它同时
    /// 经过 admin 门。多一个字段（`dsn_ciphertext` / `token_sha256` / 项目 `id`）不会
    /// 让任何用例变红 —— 只有「逐键比对」会。
    ///
    /// 变异（红）：给 [`ProjectListEntry`] 加 `pub dsn: Option<String>` ⇒ 本用例红。
    #[test]
    fn a_project_list_row_carries_exactly_six_fields_and_no_secret() {
        let v = serde_json::to_value(ProjectListEntry {
            name: "cloud-resource".into(),
            label: Some("腾讯云资源本体".into()),
            active_version: Some(5),
            frozen: true,
            entity_types: 30,
            link_types: 34,
        })
        .expect("序列化");
        let mut keys: Vec<&str> = v
            .as_object()
            .expect("对象")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "active_version",
                "entity_types",
                "frozen",
                "label",
                "link_types",
                "name"
            ],
            "回执字段变了 —— Studio 的下拉会把多出来的东西一起显示 / 或漏读"
        );
        let text = v.to_string();
        for absent in ["dsn", "cipher", "token", "sha256", "secret"] {
            assert!(
                !text.to_ascii_lowercase().contains(absent),
                "列表回执里出现了 {absent:?}：{text}"
            );
        }
    }

    /// ⭐ **AC-d1 的「没有生效版本」判据**：`active_version` 是 **`null`**、不是 0，
    /// 计数是 0、`frozen` 是 `false` —— 前端据此显示「未激活」，而不是把 v0 当真。
    ///
    /// 变异（红）：把 `active_version: Option<i32>` 改成 `i32`（默认 0）⇒ 本用例红。
    #[test]
    fn a_project_without_an_active_version_serialises_as_null_not_zero() {
        let v = serde_json::to_value(ProjectListEntry {
            name: "my-ontology".into(),
            label: None,
            active_version: None,
            frozen: false,
            entity_types: 0,
            link_types: 0,
        })
        .expect("序列化");
        assert!(v["active_version"].is_null(), "{v}");
        assert_eq!(v["frozen"], serde_json::json!(false));
        assert_eq!(v["entity_types"], serde_json::json!(0));
        assert_eq!(v["link_types"], serde_json::json!(0));
        assert!(v["label"].is_null(), "没写备注 ⇒ null（不是空串）");
    }
}
