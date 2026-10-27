//! ⭐ REQ-159 P1c-1 · **构建包导入 8 条端点的共享适配层**（一处分语义，两个面各挂一份路由）。
//!
//! # 为什么抽出来
//!
//! 这 8 条端点现在同时挂在**两个面**上：
//!
//! | 面 | 入口 | 谁用 |
//! | --- | --- | --- |
//! | 控制面 | [`super::http::build_router`]（`ontok console serve`） | FDE 本地（console 不部署到服务器） |
//! | 服务面 | [`crate::query::http`]（`ontok query serve`） | **Studio**（`/studio`，部署形态里唯一的进程） |
//!
//! ADR-013 的结论是「这 8 条必须跟着 `query serve` 走」；但 console 侧**不删**
//! （FDE 本地仍要用）。两边各写一份 handler 的具体后果不是重复，而是**漂移**：
//! 状态码（201 / 200）、审计 `action`（`project.create` / `bundle.import` / …）、
//! 错误码（`bundle.sha256.mismatch` / `key.not_admin`）—— 任何一处只在一边改，
//! 就会出现「console 能查 key、query 上换个码」这种事，而它只能靠人肉对账发现。
//!
//! ⇒ **端点语义只有这一份**。两个面各自的 handler 只剩
//! 「从自己那份 state 里借出 `pool` / `cipher` / `audit`，然后调同一个函数」。
//! 差异被隔离在 [`Deps`] 这个**借用结构**上，而不是散落在 8 份 handler 里。
//!
//! # ⛔ 这不是一条新的写库路径
//!
//! 每个函数都只调 [`super::provision`]（REQ-159 P1a 的领域层）。那一层
//! **零引用 `ConsoleService`**（签名全是 `(pool, cipher, …)`），包解析在
//! [`super::bundle`]、加密在 [`super::dsn`] —— 本文件一行业务判断都没有，
//! 它只做「解析请求 → 调领域函数 → 记一行审计 → 序列化」。
//!
//! # 审计（R5 / AC-010）
//!
//! 每个**写**操作一行（成功与失败都记），行形状与 REQ-124 的控制面审计**同一份**
//! （[`super::audit::AuditEntry::line`]）：`audit ts=… action=… token=… target=… result=… elapsed_ms=…`。
//! - `token` 是**摘要**（`sha256:` + 12 位），不是明文；
//! - `target` 是端点自己写死的短标签（项目名 / 包 id / key 摘要 / DSN 的 host+db），
//!   ⛔ 绝不拼请求体，⛔ 绝不出现 key 明文或 DSN 明文。
//!
//! ⚠️ `GET …/bundles/:id` 是**只读**状态查询，不记审计行（与 console 侧一字不差）。

use std::time::Instant;

use axum::body::Bytes;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;

use super::audit::AuditLog;
use super::dsn::SecretCipher;
use super::http::{fail, fail_op, parse_body, respond};
use super::{bundle, provision, OpError};
use crate::query::error::ApiError;

/// 写端点的**领域依赖** —— 两个面各自把自己那份 borrow 进来。
///
/// 为什么要一个结构而不是三个位置参数：8 个函数都要这三样，位置参数一多就没人读得懂
/// 调用点（`f(pool, cipher, audit, who, body, …)`）。借用的好处是**零拷贝**、
/// 且类型上保证「handler 拿不到别的 state」（例如它拿不到 token 表，也就改不了身份）。
pub struct Deps<'a> {
    /// **元数据库**（项目 / 版本 / 快照 / key / DSN 密文）—— ⛔ 不是客户源库。
    pub pool: &'a PgPool,
    /// DSN 与 query key 的加密列（启动时从 `ONTOK_SECRET_KEY` 来一次）。
    pub cipher: &'a SecretCipher,
    /// 控制面审计落点（内存环 + 唯一的 stdout / 文件写出点）。
    pub audit: &'a AuditLog,
}

// ══════════════════════════════════════════════════════════════
// 收尾：**一行审计 + 一个响应**（8 个端点共用，⛔ 不各写一遍）
// ══════════════════════════════════════════════════════════════

/// 成功：记 `result=OK` 再回响应。
fn ok(
    d: &Deps<'_>,
    who: &str,
    action: &'static str,
    target: String,
    started: Instant,
    status: StatusCode,
    body: Value,
) -> Response {
    d.audit.record_digest(Some(who), action, target, "OK", started);
    respond(status, body)
}

/// 失败：记**错误码**（不是 `OK`）再回错误契约（`{code,message,hint}`）。
fn failed(
    d: &Deps<'_>,
    who: &str,
    action: &'static str,
    target: String,
    started: Instant,
    e: ApiError,
) -> Response {
    d.audit.record_digest(Some(who), action, target, e.code, started);
    fail(e)
}

/// [`OpError`] 版本（领域层错误 ⇒ 先翻译成错误契约，再走同一个收尾）。
fn failed_op(
    d: &Deps<'_>,
    who: &str,
    action: &'static str,
    target: String,
    started: Instant,
    e: OpError,
) -> Response {
    failed(d, who, action, target, started, e.into_api())
}

/// JSON 回执（⛔ 序列化失败不 panic：`Value::Null` 比进程 panic 好）。
fn as_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

// ══════════════════════════════════════════════════════════════
// ⓪ GET /api/projects —— 列表（Studio 顶栏下拉）
// ══════════════════════════════════════════════════════════════

/// `GET /api/projects` —— 全部项目（**只读**）。
///
/// 与 `bundle_status` 同口径：**不记审计行** —— 它读的是项目名 / 版本号 / 两个计数，
/// 没有 key、没有 DSN、没有密文，写一行审计只会把「谁在刷新下拉」灌满审计文件。
/// 真正的敏感查看是 `GET …/key`（那条每次都记 `reveal_key`）。
pub async fn list_projects(d: &Deps<'_>) -> Response {
    match provision::list_projects(d.pool).await {
        Ok(out) => respond(StatusCode::OK, as_json(&out)),
        Err(e) => fail_op(e),
    }
}

// ══════════════════════════════════════════════════════════════
// ① POST /api/projects —— 建项目 + 发 key
// ══════════════════════════════════════════════════════════════

/// `POST /api/projects` 的请求体。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProjectRequest {
    pub project: String,
    /// 项目备注（可选）。
    #[serde(default)]
    pub label: Option<String>,
    /// **这把 key 给谁用**（v1 必填：一项目一把 key，审计归属只能靠它）。
    #[serde(default)]
    pub key_label: Option<String>,
}

/// `POST /api/projects` —— **201**（建了东西）。
///
/// ⭐ 回执里**只有这一次**有 key 明文（另一次是 `GET …/key`，每次写审计）。
pub async fn create_project(d: &Deps<'_>, who: &str, body: &Bytes) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let req: CreateProjectRequest = parse_body(
            body,
            "body 形如 {\"project\":\"cloud-resource\",\n  \
             \"label\":\"腾讯云资源本体\",\n  \
             \"key_label\":\"数字员工：运维助手\"}。",
        )?;
        target = format!("project={}", req.project.trim());
        let out = provision::create_project(
            d.pool,
            d.cipher,
            &req.project,
            req.label.as_deref(),
            req.key_label.as_deref(),
        )
        .await
        .map_err(OpError::into_api)?;
        target = format!("project={} key={}", out.project, out.key_digest);
        Ok::<Value, ApiError>(as_json(&out))
    }
    .await;
    match result {
        Ok(v) => ok(
            d,
            who,
            "project.create",
            target,
            started,
            StatusCode::CREATED,
            v,
        ),
        Err(e) => failed(d, who, "project.create", target, started, e),
    }
}

// ══════════════════════════════════════════════════════════════
// ② POST /api/projects/:p/bundles —— 上传（原始字节）
// ══════════════════════════════════════════════════════════════

/// `POST /api/projects/:p/bundles` —— **`application/octet-stream`，body 就是包字节**。
///
/// ⭐ 契约更正（2026-10-27）：**不用 multipart**。理由有三，都不是风格问题：
/// 1. axum 的 `multipart` feature 会拉 `multer`（不在依赖树里 ⇒ 违反 R1）；
/// 2. 前端拿到的就是一个 `File`，直接读 ArrayBuffer 发原始字节更短；
/// 3. 少一层封装 = 少一处「多一个 CRLF 就 sha256 不符」的失败面。
///
/// ⚠️ 仍然**接受** `Content-Type` 缺省或写错（很多 curl / 脚本不会设头）；
/// 唯一显式拒绝的是 `multipart/form-data` —— 那种情况下 body 里是封装后的字节，
/// 当成包解析必然在 zip 那一步报一个**看不懂的错误**，不如在这里点名。
pub async fn upload_bundle(
    d: &Deps<'_>,
    who: &str,
    project: &str,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        target = format!("project={project}");
        let ct = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ct.starts_with("multipart/form-data") {
            return Err(bundle::reject_encapsulated(&ct).into_api());
        }
        let file = body.to_vec();
        target = format!("project={project} bytes={}", file.len());
        let out = provision::upload_bundle(d.pool, project, file)
            .await
            .map_err(OpError::into_api)?;
        target = format!(
            "project={project} bundle={} bytes={}",
            out.bundle_id, out.bytes
        );
        Ok::<Value, ApiError>(as_json(&out))
    }
    .await;
    match result {
        Ok(v) => ok(
            d,
            who,
            "bundle.upload",
            target,
            started,
            StatusCode::CREATED,
            v,
        ),
        Err(e) => failed(d, who, "bundle.upload", target, started, e),
    }
}

// ══════════════════════════════════════════════════════════════
// ③ GET /api/projects/:p/bundles/:id —— 状态与组件清单（只读，不记审计）
// ══════════════════════════════════════════════════════════════

/// `GET /api/projects/:p/bundles/:id` —— 状态与组件清单（只读，**不记审计行**）。
pub async fn bundle_status(d: &Deps<'_>, project: &str, id: &str) -> Response {
    match provision::bundle_status(d.pool, project, id).await {
        Ok(out) => respond(StatusCode::OK, as_json(&out)),
        Err(e) => fail_op(e),
    }
}

// ══════════════════════════════════════════════════════════════
// ④ GET /api/projects/:p/key —— 再次查看（每次写审计）
// ══════════════════════════════════════════════════════════════

/// `GET /api/projects/:p/key` —— **再次查看** query key。
///
/// 三条硬要求（契约 2026-10-27 10:25）：
/// 1. **admin 才能调** —— `query` 打这条路径的 403 码是 `key.not_admin`
///    （两个面的身份门都判这一条）；
/// 2. **每次调用写一条审计**（`action=reveal_key`）—— 这是「能查看」的唯一补偿；
/// 3. ⛔ 响应里只有 key 明文（给 admin 看）+ 标签 + 时间；**没有** ciphertext、没有密钥。
pub async fn reveal_key(d: &Deps<'_>, who: &str, project: &str) -> Response {
    let started = Instant::now();
    let mut target = format!("project={project}");
    let result = async {
        let out = provision::reveal_key(d.pool, d.cipher, project)
            .await
            .map_err(OpError::into_api)?;
        // 审计只记项目与摘要（⛔ 不记明文）：
        target = format!(
            "project={project} key=sha256:{}",
            crate::query::audit::digest_prefix(
                &crate::query::audit::sha256_hex(out.key.as_bytes()),
                12
            )
        );
        Ok::<Value, ApiError>(as_json(&out))
    }
    .await;
    match result {
        Ok(v) => ok(d, who, "reveal_key", target, started, StatusCode::OK, v),
        Err(e) => failed(d, who, "reveal_key", target, started, e),
    }
}

// ══════════════════════════════════════════════════════════════
// ⑤ PUT /api/projects/:p/database —— 存 DSN（加密列）
// ══════════════════════════════════════════════════════════════

/// `PUT /api/projects/:p/database` 的请求体。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseRequest {
    pub dsn: String,
    #[serde(default)]
    pub dialect: Option<String>,
}

/// `PUT /api/projects/:p/database` —— 存 DSN（加密列）。⛔ 响应里没有 `dsn`。
pub async fn put_database(d: &Deps<'_>, who: &str, project: &str, body: &Bytes) -> Response {
    let started = Instant::now();
    // ⚠️ 审计的 target **永远**只写 host / db（见下面赋值处），
    // 绝不把请求体或 DSN 拼进去 —— AC-009 的判据就在这一行上。
    let mut target = format!("project={project}");
    let result = async {
        let req: DatabaseRequest = parse_body(
            body,
            "body 形如 {\"dsn\":\"postgresql://user:pass@host:5432/db\",\
             \"dialect\":\"postgresql\"}。",
        )?;
        let dialect = req.dialect.as_deref().unwrap_or("postgresql");
        let out = provision::put_database(d.pool, d.cipher, project, &req.dsn, dialect)
            .await
            .map_err(OpError::into_api)?;
        target = format!(
            "project={project} host={} database={}",
            out.host, out.database
        );
        Ok::<Value, ApiError>(as_json(&out))
    }
    .await;
    match result {
        Ok(v) => ok(d, who, "database.put", target, started, StatusCode::OK, v),
        Err(e) => failed(d, who, "database.put", target, started, e),
    }
}

// ══════════════════════════════════════════════════════════════
// ⑥ POST /api/projects/:p/database/test —— 测连通（失败也 200）
// ══════════════════════════════════════════════════════════════

/// `POST /api/projects/:p/database/test` —— **失败也返 200**（由 `ok` 区分）。
pub async fn test_database(d: &Deps<'_>, who: &str, project: &str) -> Response {
    let started = Instant::now();
    let target = format!("project={project}");
    match provision::test_database(d.pool, d.cipher, project).await {
        Ok(out) => {
            let result = if out.ok {
                "OK"
            } else {
                out.error.as_ref().map(|e| e.code).unwrap_or("db.connect.unknown")
            };
            d.audit
                .record_digest(Some(who), "database.test", target, result, started);
            // ⚠️ 成功与失败都 200：由 `ok` 区分（demand §五-5）。
            respond(
                StatusCode::OK,
                json!({
                    "ok": out.ok,
                    "latency_ms": out.latency_ms as u64,
                    "server_version": out.server_version,
                    "error": out.error.map(|e| json!({"code": e.code, "message": e.message})),
                }),
            )
        }
        Err(e) => failed_op(d, who, "database.test", target, started, e),
    }
}

// ══════════════════════════════════════════════════════════════
// ⑦ POST /api/projects/:p/bundles/:id/import —— 解析入库
// ══════════════════════════════════════════════════════════════

/// `POST /api/projects/:p/bundles/:id/import` —— 解析入库（单事务，**不冻结**）。
pub async fn import_bundle(d: &Deps<'_>, who: &str, project: &str, id: &str) -> Response {
    let started = Instant::now();
    let mut target = format!("project={project} bundle={id}");
    let result = async {
        let out = provision::import_bundle(d.pool, project, id)
            .await
            .map_err(OpError::into_api)?;
        target = format!(
            "project={project} bundle={} version={}",
            out.bundle_id, out.version
        );
        Ok::<Value, ApiError>(as_json(&out))
    }
    .await;
    match result {
        Ok(v) => ok(d, who, "bundle.import", target, started, StatusCode::OK, v),
        Err(e) => failed(d, who, "bundle.import", target, started, e),
    }
}

// ══════════════════════════════════════════════════════════════
// ⑧ POST /api/projects/:p/bundles/:id/activate —— 冻结 + 生效
// ══════════════════════════════════════════════════════════════

/// `POST /api/projects/:p/bundles/:id/activate` —— 冻结 + 写生效指针（同一事务）。
pub async fn activate_bundle(d: &Deps<'_>, who: &str, project: &str, id: &str) -> Response {
    let started = Instant::now();
    let mut target = format!("project={project} bundle={id}");
    let result = async {
        let out = provision::activate_bundle(d.pool, project, id)
            .await
            .map_err(OpError::into_api)?;
        target = format!(
            "project={project} bundle={} version={}",
            out.bundle_id, out.version
        );
        Ok::<Value, ApiError>(as_json(&out))
    }
    .await;
    match result {
        Ok(v) => ok(d, who, "bundle.activate", target, started, StatusCode::OK, v),
        Err(e) => failed(d, who, "bundle.activate", target, started, e),
    }
}
