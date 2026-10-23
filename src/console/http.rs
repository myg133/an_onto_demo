//! REQ-124 · **控制面的 HTTP 适配层** —— 解析 → 调领域函数 → 序列化。
//!
//! # 两个响应写点（都登记在 OUTLETS）
//!
//! - [`respond`]：JSON 响应（`Json(` 在**本文件只有一处**，I18）；
//! - [`respond_text`]：Turtle / 原样的文本响应（TTL 走 I19，R2RML 走 I20）。
//!
//! ⛔ 没有第三个写点：错误体也走 [`respond`]（形状是 `query::error::ApiError` 的
//! `{code, message, hint}`）。
//!
//! # 身份门是**唯一**入口
//!
//! [`require_token`] 挂在整张路由上（`middleware::from_fn_with_state`）：
//! - 没有 / 错的 token ⇒ **401**（不透露路径存不存在、不透露 token 差在哪）；
//! - **query token** ⇒ **403** `TOKEN_KIND_FORBIDDEN`（R1：认得它，但不接受它）；
//! - admin token ⇒ 放进请求扩展，handler 只能拿到这一行。
//!
//! ⛔ 只认 `Authorization: Bearer`：query string / cookie 里的 token 不生效
//! （与 REQ-123 同一条口径；query string 里出现 `token=` 还会被点名 400）。
//!
//! # 审计
//!
//! 每个**写操作**（import / apply / freeze / config PUT / decide）与两条**产物生成**
//! （export / r2rml）各记一行，**成功与失败都记**。鉴权失败也记（`action=auth`），
//! 这样「403 有多少次」是能数出来的事实。`/api/health` 不记：探针每分钟几十次，
//! 记了只会把真事件淹掉。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path as AxumPath, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use super::decl::DeclKind;
use super::{tokens::TokenKind, ConsoleService, OpError};
use crate::query::error::ApiError;
use crate::review_ui::http::{decide_json, review_json, scope_for, ReviewFailure};

/// 唯一免 token 的路径（AC-001：只给版本，零数据）。
pub const HEALTH_PATH: &str = "/api/health";

/// 请求体上限。内联提案（`{project, proposal}`）可能有几十万字节；
/// 4 MiB 足够放下最大的提案，同时挡住「用超大 body 拖垮进程」。
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// 身份门放进请求扩展的那一行 token（handler 只能拿到它）。
#[derive(Debug, Clone)]
pub struct Auth(pub super::tokens::TokenEntry);

/// 组装路由。
pub fn build_router(svc: Arc<ConsoleService>) -> Router {
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route("/api/projects", get(get_projects))
        .route("/api/import", post(post_import))
        .route("/api/apply", post(post_apply))
        .route("/api/freeze", post(post_freeze))
        .route("/api/export/ttl", get(get_export_ttl))
        .route("/api/r2rml", post(post_r2rml))
        .route(
            "/api/config/:kind",
            get(get_config).put(put_config),
        )
        .route("/api/audit", get(get_audit))
        .route("/api/review", get(get_review))
        .route("/api/decide", post(post_decide))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(svc.clone(), require_token))
        .with_state(svc)
}

// ══════════════════════════════════════════════════════════════
// 响应构造（**唯一**的两个写点）
// ══════════════════════════════════════════════════════════════

/// JSON 响应。全文件只有这一处 `Json(`（登记为 OUTLETS 的 I18）。
fn respond(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// 文本响应（TTL / R2RML / 声明原文）。**不改一个字节**：产物是已经过闸的字符串。
fn respond_text(status: StatusCode, content_type: &'static str, text: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, HeaderValue::from_static(content_type))],
        text,
    )
        .into_response()
}

/// `GET /api/export/ttl` 的响应（I19：`text/turtle`）。
fn respond_ttl(text: String) -> Response {
    respond_text(StatusCode::OK, "text/turtle", text)
}

/// `POST /api/r2rml` 的响应（I20：`text/turtle`）。
fn respond_r2rml(text: String) -> Response {
    respond_text(StatusCode::OK, "text/turtle", text)
}

/// 错误的**唯一**出口（正文来自 [`ApiError::body`]，与查询面同一个形状）。
fn fail(e: ApiError) -> Response {
    respond(e.status, e.body())
}

fn fail_op(e: OpError) -> Response {
    fail(e.into_api())
}

fn fail_review(f: ReviewFailure) -> Response {
    let code = match f.status {
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::CONFLICT => "CONFLICT",
        StatusCode::BAD_REQUEST => "BAD_REQUEST",
        StatusCode::FORBIDDEN => "FORBIDDEN",
        _ => "INTERNAL",
    };
    fail(ApiError::with_code(
        f.status,
        code,
        f.message,
        "审核端点与 `ontok review serve` 是**同一份实现**：先确认提案仍是 pending、\
         基版本仍未冻结（冻结后要新开一版），再重试。",
    ))
}

/// 未注册路径。**在身份门之后**：无 token 时先得到 401（探测不到路径）。
async fn not_found() -> Response {
    fail(ApiError::with_code(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "没有这个路径。",
        "可用：GET /api/health（免 token）、GET /api/projects、POST /api/import、POST /api/apply、\
         POST /api/freeze、GET /api/export/ttl、POST /api/r2rml、\
         GET|PUT /api/config/{exclusions|policy|tenancy}、GET /api/audit、\
         GET /api/review、POST /api/decide。",
    ))
}

// ══════════════════════════════════════════════════════════════
// 身份门（**唯一**一处）
// ══════════════════════════════════════════════════════════════

/// 从 `Authorization` 头取凭据。⛔ **只读头**：query string / cookie 一律不看。
fn bearer(headers: &HeaderMap) -> &str {
    let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
    else {
        return "";
    };
    // `str::get(..7)` 是**边界安全**的：多字节字符上 `v[..7]` 会 panic。
    match v.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("bearer ") => v[7..].trim(),
        _ => "",
    }
}

/// 身份门：401（没有 / 错的 token）与 403（query token）在**这里**分开。
async fn require_token(
    State(svc): State<Arc<ConsoleService>>,
    mut req: Request,
    next: Next,
) -> Response {
    if req.uri().path() == HEALTH_PATH {
        return next.run(req).await;
    }
    let started = Instant::now();
    let presented = bearer(req.headers());
    match svc.tokens.find(presented) {
        Some(entry) if entry.kind == TokenKind::Admin => {
            req.extensions_mut().insert(Auth(entry.clone()));
            next.run(req).await
        }
        // ⛔ R1：**认得**它，但不接受它。401 会说「你没有凭据」——那是假话，
        // 因为这张 token 是真的，只是不是管理凭据。
        Some(entry) => {
            svc.audit.record(
                entry,
                "auth",
                "-",
                "TOKEN_KIND_FORBIDDEN",
                started,
            );
            fail(ApiError::with_code(
                StatusCode::FORBIDDEN,
                "TOKEN_KIND_FORBIDDEN",
                format!(
                    "这是一张 {} token，不能调用管理端点。",
                    entry.kind.as_str()
                ),
                "管理端点只接受 `kind = \"admin\"` 的 token；查询 token 请用于 `ontok query serve`。\
                 ⛔ 同一个 token 不许两用（admin-tokens.json 加载时就拒绝重复的 token）。",
            ))
        }
        None => {
            svc.audit
                .record_anonymous("auth", "-", "UNAUTHORIZED", started);
            fail(ApiError::unauthorized())
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 小工具
// ══════════════════════════════════════════════════════════════

/// 请求体 → 强类型 DTO（`deny_unknown_fields` 在 DTO 上：拼错字段名 ⇒ 400 + 点名）。
fn parse_body<T: DeserializeOwned>(body: &Bytes, hint: &'static str) -> Result<T, ApiError> {
    if body.is_empty() {
        return Err(ApiError::bad_request("请求体是空的。", hint));
    }
    serde_json::from_slice(body).map_err(|e| {
        ApiError::bad_request(
            format!("请求体不是合法 JSON（或字段不对）：{e}"),
            hint,
        )
    })
}

/// 项目名必须等于服务作用域 —— ⛔ 一个进程只服务一个项目（与 `query serve` 同一条）。
fn check_project(svc: &ConsoleService, project: &str) -> Result<(), OpError> {
    if project.trim() != svc.project {
        return Err(OpError::bad_request(
            "PROJECT_MISMATCH",
            format!(
                "请求里的 project = {project:?}，而本服务只服务 {:?}。",
                svc.project
            ),
            "换一个服务的 `--project`，或者把请求里的 project 改成服务作用域。\
             一个进程只服务一个项目是刻意的：跨项目作用域是「导错项目」的前置条件。",
        ));
    }
    Ok(())
}

/// 解析路径参数（`schema` / `tenancy` / `exclude` 都是**服务端路径**）。
fn path_arg(raw: &str, what: &'static str) -> Result<PathBuf, OpError> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(OpError::bad_request(
            "PATH_EMPTY",
            format!("{what} 是一个空路径。"),
            "要么省略这个参数，要么给一个存在的文件路径（服务端路径）。",
        ));
    }
    Ok(PathBuf::from(t))
}

fn read_json_file(path: &Path, code: &'static str, what: &'static str) -> Result<Value, OpError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        OpError::bad_request(
            code,
            format!("读不到{what} {}：{e}", path.display()),
            "这是**服务端路径**：控制面按它去读文件（与 CLI 的同名参数同义）。",
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        OpError::bad_request(
            if code == "IMPORT_FILE_NOT_FOUND" {
                "IMPORT_BAD_JSON"
            } else {
                "BAD_JSON"
            },
            format!("{what} {} 不是合法 JSON：{e}", path.display()),
            "body/文件必须是 JSON 对象；serde 的错误里带行列号。",
        )
    })
}

// ══════════════════════════════════════════════════════════════
// GET /api/health（免 token）
// ══════════════════════════════════════════════════════════════

/// **免 token**，只给版本（AC-001）。⛔ 项目名 / 版本 / 声明路径都不出去。
async fn health() -> Response {
    respond(
        StatusCode::OK,
        json!({
            "service": "ontok-console",
            "version": env!("CARGO_PKG_VERSION"),
            "api": "v1",
        }),
    )
}

// ══════════════════════════════════════════════════════════════
// GET /api/projects
// ══════════════════════════════════════════════════════════════

async fn get_projects(State(svc): State<Arc<ConsoleService>>) -> Response {
    match super::ops::projects_summary(&svc.pool, &svc.project).await {
        Ok(summary) => respond(StatusCode::OK, serde_json::to_value(summary).unwrap_or(Value::Null)),
        Err(e) => fail_op(e),
    }
}

// ══════════════════════════════════════════════════════════════
// POST /api/import
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest {
    project: String,
    /// 服务端路径（与 `propose import --file` 同义）。
    #[serde(default)]
    path: Option<String>,
    /// **内联**提案（同一个 JSON 形状：`{project, base_version?, items[], …}`）。
    #[serde(default)]
    proposal: Option<Value>,
}

async fn post_import(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let req: ImportRequest = parse_body(
            &body,
            "body 形如 {\"project\":\"demo\",\"path\":\"/abs/proposal.json\"} \
             或 {\"project\":\"demo\",\"proposal\":{…}}。",
        )?;
        target = match (&req.path, req.proposal.is_some()) {
            (Some(p), false) => format!("project={} path={p}", req.project),
            (None, true) => format!("project={} source=<inline>", req.project),
            (Some(_), true) => {
                return Err(ApiError::bad_request(
                    "`path` 与 `proposal` 只能给一个。",
                    "文件走 `path`（服务端路径），内联走 `proposal`（JSON 对象）。",
                ))
            }
            (None, false) => {
                return Err(ApiError::bad_request(
                    "缺 `path` 或 `proposal`。",
                    "文件走 `path`，内联走 `proposal` —— 二者给且只给一个。",
                ))
            }
        };
        check_project(&svc, &req.project).map_err(OpError::into_api)?;
        let (doc, source) = match req.path {
            Some(p) => {
                let path = PathBuf::from(&p);
                let doc = read_json_file(
                    &path,
                    "IMPORT_FILE_NOT_FOUND",
                    "提案文件",
                )
                .map_err(OpError::into_api)?;
                (doc, path)
            }
            None => (
                req.proposal.clone().unwrap_or(Value::Null),
                PathBuf::from("<inline proposal>"),
            ),
        };
        // ⭐ 与 `propose import` **同一个函数**（`src/cli.rs:import_proposal_core`）。
        let receipt = crate::cli::import_proposal_core(
            &svc.pool,
            &doc,
            &source,
            Some(&svc.project),
            true,
        )
        .await
        .map_err(OpError::into_api)?;
        Ok::<Value, ApiError>(json!({
            "ok": true,
            "project": receipt.project,
            "proposal": receipt.alias,
            "proposal_id": receipt.proposal_id,
            "items": receipt.items.len(),
            "extracted_by": receipt.extracted_by,
            "exclusions": receipt.exclusions,
        }))
    }
    .await;
    match result {
        Ok(v) => {
            svc.audit.record(&auth.0, "import", target, "OK", started);
            respond(StatusCode::OK, v)
        }
        Err(e) => {
            svc.audit.record(&auth.0, "import", target, e.code, started);
            fail(e)
        }
    }
}

// ══════════════════════════════════════════════════════════════
// POST /api/apply · POST /api/freeze
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyRequest {
    project: String,
    /// 别名（`p1` / `p2` …）或 UUID。
    proposal: String,
}

async fn post_apply(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let req: ApplyRequest = parse_body(
            &body,
            "body 形如 {\"project\":\"demo\",\"proposal\":\"p1\"}（也可给提案 UUID）。",
        )?;
        target = format!("project={} proposal={}", req.project, req.proposal);
        check_project(&svc, &req.project).map_err(OpError::into_api)?;
        let receipt =
            super::ops::apply_proposal_core(&svc.pool, &svc.project, &req.proposal)
                .await
                .map_err(OpError::into_api)?;
        Ok::<Value, ApiError>(serde_json::to_value(receipt).unwrap_or(Value::Null))
    }
    .await;
    match result {
        Ok(v) => {
            svc.audit.record(&auth.0, "apply", target, "OK", started);
            respond(StatusCode::OK, v)
        }
        Err(e) => {
            svc.audit.record(&auth.0, "apply", target, e.code, started);
            fail(e)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FreezeRequest {
    project: String,
    version: i32,
}

async fn post_freeze(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let req: FreezeRequest = parse_body(
            &body,
            "body 形如 {\"project\":\"demo\",\"version\":2}。",
        )?;
        target = format!("project={} version={}", req.project, req.version);
        check_project(&svc, &req.project).map_err(OpError::into_api)?;
        let out = super::ops::freeze_for_api(&svc.pool, &svc.project, req.version)
            .await
            .map_err(OpError::into_api)?;
        Ok::<Value, ApiError>(serde_json::to_value(out).unwrap_or(Value::Null))
    }
    .await;
    match result {
        Ok(v) => {
            svc.audit.record(&auth.0, "freeze", target, "OK", started);
            respond(StatusCode::OK, v)
        }
        Err(e) => {
            svc.audit.record(&auth.0, "freeze", target, e.code, started);
            fail(e)
        }
    }
}

// ══════════════════════════════════════════════════════════════
// GET /api/export/ttl
// ══════════════════════════════════════════════════════════════

async fn get_export_ttl(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let mut project: Option<String> = None;
        let mut version: Option<i32> = None;
        let mut model_view = false;
        let mut schema: Option<PathBuf> = None;
        let mut tenancy: Option<PathBuf> = None;
        let mut base: Option<String> = None;
        for (k, v) in &q {
            match k.as_str() {
                "project" => project = Some(v.clone()),
                "version" => {
                    version = Some(v.trim().parse::<i32>().map_err(|_| {
                        ApiError::bad_request(
                            format!("version 要写一个整数，收到 {v:?}。"),
                            "版本是 `GET /api/projects` 里的 `versions[].version`（从 1 起）。",
                        )
                    })?)
                }
                "model_view" => {
                    model_view = match v.trim() {
                        "1" | "true" | "yes" => true,
                        "0" | "false" | "no" | "" => false,
                        other => {
                            return Err(ApiError::bad_request(
                                format!("model_view 只认 1/0（或 true/false），收到 {other:?}。"),
                                "客户视图（默认）与建模视图是**两种**产物：都要就各导一次。",
                            ))
                        }
                    }
                }
                "schema" => schema = Some(path_arg(v, "schema").map_err(OpError::into_api)?),
                "tenancy" => tenancy = Some(path_arg(v, "tenancy").map_err(OpError::into_api)?),
                "base" => base = Some(v.clone()),
                other @ ("token" | "access_token" | "api_key") => {
                    return Err(ApiError::bad_request(
                        format!("query string 里不要放 {other}。"),
                        "令牌只认 `Authorization: Bearer <token>` 头；放在 query string 里**不生效**，\
                         服务端也不会读取它。",
                    ))
                }
                other => {
                    return Err(ApiError::bad_request(
                        format!("不认识的查询参数 {other:?}。"),
                        "export/ttl 只接受 project / version / model_view / schema / tenancy / base。",
                    ))
                }
            }
        }
        let project = project.ok_or_else(|| {
            ApiError::bad_request(
                "缺 `project` 参数。",
                "形如 `GET /api/export/ttl?project=demo&version=2&model_view=1`。",
            )
        })?;
        let version = version.ok_or_else(|| {
            ApiError::bad_request(
                "缺 `version` 参数。",
                "版本是 `GET /api/projects` 里的 `versions[].version`。",
            )
        })?;
        target = format!(
            "project={project} version={version} model_view={model_view}"
        );
        check_project(&svc, &project).map_err(OpError::into_api)?;
        let base = base.unwrap_or_else(|| svc.base.clone());
        let params = crate::cli::TtlExportParams {
            project: &svc.project,
            version,
            model_view,
            base: &base,
            tenancy: tenancy.as_deref(),
            schema: schema.as_deref(),
        };
        // ⭐ 与 `ontology export` **共用的读取/投影**（`src/cli.rs:prepare_ttl_export`）。
        let prep = crate::cli::prepare_ttl_export(&svc.pool, params)
            .await
            .map_err(OpError::into_api)?;
        // ⭐ 同一个 `domain::pipeline::render` + 同一份 `TtlPrepared` ⇒ 与 CLI 逐字节相同
        // （AC-005）。控制面这一侧命中泄漏是 409，不是进程退出。
        let rendered = super::ops::render_ttl_export(&prep).map_err(OpError::into_api)?;
        super::ops::report_ttl_export(&prep, rendered.range_stats());
        target = format!(
            "{} objects={} relations={}",
            target,
            prep.object_count(),
            prep.relation_count()
        );
        Ok::<_, ApiError>(rendered.into_text())
    }
    .await;
    match result {
        Ok(text) => {
            svc.audit.record(&auth.0, "export", target, "OK", started);
            respond_ttl(text)
        }
        Err(e) => {
            svc.audit.record(&auth.0, "export", target, e.code, started);
            fail(e)
        }
    }
}

// ══════════════════════════════════════════════════════════════
// POST /api/r2rml
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct R2rmlRequest {
    project: String,
    version: i32,
    /// **必填**，服务端路径（与 `r2rml generate --schema` 同义）。
    schema: String,
    #[serde(default)]
    exclude: Option<String>,
    #[serde(default)]
    include_inherit: bool,
}

async fn post_r2rml(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let req: R2rmlRequest = parse_body(
            &body,
            "body 形如 {\"project\":\"demo\",\"version\":2,\"schema\":\"/abs/schema.json\",\
             \"exclude\":\"/abs/exclusions.json\",\"include_inherit\":false}。",
        )?;
        target = format!(
            "project={} version={} include_inherit={}",
            req.project, req.version, req.include_inherit
        );
        check_project(&svc, &req.project).map_err(OpError::into_api)?;
        let schema = path_arg(&req.schema, "schema").map_err(OpError::into_api)?;
        let exclude = match req.exclude.as_deref() {
            Some(raw) => Some(path_arg(raw, "exclude").map_err(OpError::into_api)?),
            None => None,
        };
        let params = crate::cli::R2rmlParams {
            project: &svc.project,
            version: req.version,
            schema: &schema,
            include_inherit: req.include_inherit,
            base: &svc.base,
            tenancy: None,
            exclude: exclude.as_deref(),
            fk_signed: None,
        };
        // ⭐ 与 `r2rml generate` **同一个函数**（`src/cli.rs:r2rml_core`）。
        let out = crate::cli::r2rml_core(&svc.pool, params)
            .await
            .map_err(OpError::into_api)?;
        target = format!(
            "{} triples_maps={}",
            target,
            out.doc.stats().triples_maps
        );
        Ok::<_, ApiError>(out.doc.into_doc())
    }
    .await;
    match result {
        Ok(text) => {
            svc.audit.record(&auth.0, "r2rml", target, "OK", started);
            respond_r2rml(text)
        }
        Err(e) => {
            svc.audit.record(&auth.0, "r2rml", target, e.code, started);
            fail(e)
        }
    }
}

// ══════════════════════════════════════════════════════════════
// GET / PUT /api/config/{exclusions|policy|tenancy}
// ══════════════════════════════════════════════════════════════

fn decl_kind_or_404(raw: &str) -> Result<DeclKind, ApiError> {
    DeclKind::parse(raw).ok_or_else(|| {
        ApiError::with_code(
            StatusCode::NOT_FOUND,
            "UNKNOWN_DECLARATION",
            format!("没有叫 {raw:?} 的声明。"),
            "只有三份：`exclusions`（排除）、`policy`（租户口径）、`tenancy`（租户列标注）。",
        )
    })
}

async fn get_config(
    State(svc): State<Arc<ConsoleService>>,
    AxumPath(kind): AxumPath<String>,
) -> Response {
    let k = match decl_kind_or_404(&kind) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    match k.read(&svc.paths) {
        Ok(text) => respond_text(StatusCode::OK, "application/json", text),
        Err(e) => fail_op(e),
    }
}

async fn put_config(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    AxumPath(kind): AxumPath<String>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let k = match decl_kind_or_404(&kind) {
        Ok(k) => k,
        Err(e) => return fail(e),
    };
    let target = format!("config={}", k.as_str());
    let path = k.path(&svc.paths).to_path_buf();

    // ① 原文必须是 UTF-8 JSON 文本（⛔ 不让非 UTF-8 走到解析器里，错误会变成乱码）。
    let text = match std::str::from_utf8(&body) {
        Ok(t) => t,
        Err(e) => {
            svc.audit
                .record(&auth.0, "config", target, "BAD_UTF8", started);
            return fail(ApiError::bad_request(
                format!("请求体不是 UTF-8：{e}"),
                "三份声明都是 JSON 文本（UTF-8）。",
            ));
        }
    };
    if text.trim().is_empty() {
        svc.audit
            .record(&auth.0, "config", target, "EMPTY_BODY", started);
        return fail(ApiError::bad_request(
            "请求体是空的。",
            "PUT 的 body 是**整份**声明原文（不是补丁）。",
        ));
    }

    // ② **保存前校验**（R4）：形状交给既有解析器，表/列交给 schema。
    //    ⛔ 校验失败 ⇒ 直接返回，**文件一个字节都不动**。
    if let Err(e) = super::decl::validate(k, &path, text, &svc.schema) {
        svc.audit.record(&auth.0, "config", target, e.code, started);
        return fail_op(e);
    }

    // ③ 保存（写后读回再算 sha256；写失败不会被报成「已保存」）。
    let before = std::fs::read(&path).ok().map(|b| crate::query::audit::sha256_hex(&b));
    match super::decl::save(&path, text) {
        Ok((bytes, sha256)) => {
            svc.audit.record(&auth.0, "config", target, "OK", started);
            respond(
                StatusCode::OK,
                json!({
                    "ok": true,
                    "kind": k.as_str(),
                    "path": path.display().to_string(),
                    "bytes": bytes,
                    "sha256": sha256,
                    "previous_sha256": before,
                }),
            )
        }
        Err(e) => {
            svc.audit.record(&auth.0, "config", target, e.code, started);
            fail_op(e)
        }
    }
}

// ══════════════════════════════════════════════════════════════
// GET /api/audit
// ══════════════════════════════════════════════════════════════

async fn get_audit(
    State(svc): State<Arc<ConsoleService>>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let mut limit = 100usize;
    for (k, v) in &q {
        match k.as_str() {
            "limit" => {
                match v.trim().parse::<usize>() {
                    Ok(n) if n > 0 => limit = n.min(1000),
                    _ => {
                        return fail(ApiError::bad_request(
                            format!("limit 要写一个正整数，收到 {v:?}。"),
                            "`limit` 默认 100、上限 1000（内存里保留最近 1000 条）。",
                        ))
                    }
                }
            }
            other => {
                return fail(ApiError::bad_request(
                    format!("不认识的查询参数 {other:?}。"),
                    "audit 只接受 `limit`（默认 100，上限 1000）。",
                ))
            }
        }
    }
    let entries = svc.audit.recent(limit);
    respond(
        StatusCode::OK,
        json!({
            "count": entries.len(),
            "order": "oldest_first",
            "entries": entries,
        }),
    )
}

// ══════════════════════════════════════════════════════════════
// GET /api/review · POST /api/decide
// ══════════════════════════════════════════════════════════════

async fn get_review(
    State(svc): State<Arc<ConsoleService>>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let mut project: Option<String> = None;
    let mut proposal: Option<String> = None;
    for (k, v) in &q {
        match k.as_str() {
            "project" => project = Some(v.clone()),
            "proposal" => proposal = Some(v.clone()),
            other => {
                return fail(ApiError::bad_request(
                    format!("不认识的查询参数 {other:?}。"),
                    "review 只接受 `project` 与 `proposal`（`p1` = 该项目内最近导入的提案）。",
                ))
            }
        }
    }
    let project = project.unwrap_or_else(|| svc.project.clone());
    let Some(proposal) = proposal else {
        return fail(ApiError::bad_request(
            "缺 `proposal` 参数。",
            "形如 `GET /api/review?proposal=p1`；`p1` 是该项目内最近导入的提案。",
        ));
    };
    if let Err(e) = check_project(&svc, &project) {
        return fail_op(e);
    }
    let scope = match scope_for(&svc.pool, &svc.project, &proposal).await {
        Ok(s) => s,
        Err(f) => return fail_review(f),
    };
    match review_json(&svc.pool, &scope).await {
        Ok(v) => respond(StatusCode::OK, v),
        Err(f) => fail_review(f),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideRequest {
    project: String,
    proposal: String,
    item: i32,
    track: String,
    decision: String,
}

async fn post_decide(
    State(svc): State<Arc<ConsoleService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let mut target = String::from("-");
    let result = async {
        let req: DecideRequest = parse_body(
            &body,
            "body 形如 {\"project\":\"demo\",\"proposal\":\"p1\",\"item\":1,\
             \"track\":\"biz\",\"decision\":\"accept\"}。",
        )?;
        target = format!(
            "project={} proposal={} item={} track={} decision={}",
            req.project, req.proposal, req.item, req.track, req.decision
        );
        check_project(&svc, &req.project).map_err(OpError::into_api)?;
        let scope = scope_for(&svc.pool, &svc.project, &req.proposal)
            .await
            .map_err(|f| {
                ApiError::with_code(f.status, "REVIEW_SCOPE_FAILED", f.message, "先确认提案别名 / 项目名。")
            })?;
        // ⭐ 与 `review serve` 的 `POST /api/decide` **同一个函数**（`decide_json`），
        // `via` 记 `console`（审计面能区分页面签的与 API 签的）。
        decide_json(
            &svc.pool,
            &scope,
            req.item,
            &req.track,
            &req.decision,
            crate::db::proposal::VIA_CONSOLE,
        )
        .await
        .map_err(|f| {
            let code = match f.status {
                StatusCode::NOT_FOUND => "ITEM_NOT_FOUND",
                StatusCode::CONFLICT => "CONFLICT",
                StatusCode::BAD_REQUEST => "BAD_REQUEST",
                _ => "INTERNAL",
            };
            ApiError::with_code(f.status, code, f.message, "审核只服务未冻结的 pending 提案。")
        })
    }
    .await;
    match result {
        Ok(v) => {
            svc.audit.record(&auth.0, "decide", target, "OK", started);
            respond(StatusCode::OK, v)
        }
        Err(e) => {
            svc.audit.record(&auth.0, "decide", target, e.code, started);
            fail(e)
        }
    }
}
