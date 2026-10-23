//! REQ-126 · **门户的 HTTP 适配层** —— 解析 → 调领域函数 → 序列化。
//!
//! # 只有四个端点
//!
//! | 方法 | 路径 | 身份 |
//! | --- | --- | --- |
//! | GET | `/` | 匿名（浏览器导航带不了 `Authorization`；壳内零数据） |
//! | GET | `/api/health` | 匿名（只给版本） |
//! | GET | `/api/questions` | 门户口令 |
//! | POST | `/q/<id>` | 门户口令 |
//!
//! ⛔ 没有第五个。`/api/sparql`、`/api/catalog`、`/api/describe`、任何写操作
//! 在这个进程里**没有路由**（落到 [`not_found`]，正文可读并列出四个端点）。
//!
//! # 两个响应写点（都登记在 OUTLETS）
//!
//! - [`respond`]：JSON 响应（`Json(` 在**本文件只有一处**，I24）；
//! - [`index`]：页面壳（`Html(` 在**本文件只有一处**，I23）。
//!
//! 两者都叠上三个安全头（[`crate::portal::page::SECURITY_HEADERS`]）—— 连 401 的
//! 错误正文也带：客户浏览器拿到的是一个**说人话**的 JSON，而不是一段纯文本。
//!
//! # 身份门是**唯一**入口，且只认门户口令
//!
//! - 没有 / 错的 token ⇒ **401**（不透露路径存不存在、不透露 token 差在哪）；
//! - `kind = "admin"` / `"query"` 的 token ⇒ **401**：门户的凭据表里根本没有它们
//!   （AC-006 的原话是「console 的 admin token 调不了 portal（401）」）。
//!   ⛔ 这里不做 403 —— 403 会告诉对方「这张 token 是真的，只是用错了地方」，
//!   而门户连「运维有 admin token」这件事都不该承认。
//! - ⛔ 只认 `Authorization: Bearer`：query string / cookie 里的口令不生效。
//!
//! # 租户不许从请求来（AC-001）
//!
//! 三道门，任何一道都不放行：
//!
//! | 入口 | 结果 |
//! | --- | --- |
//! | URL 上任何查询参数（含 `?tenant=` / `?token=`；**`/` 与 `/api/health` 也不例外**） | **400** `QUERY_PARAM_FORBIDDEN` |
//! | 请求体里的 `tenant` / `customer` / `customer_id` / `owner` | **403** `TENANT_FROM_CLIENT` |
//! | 请求体里的 `sparql` / `query` | **400** `CUSTOM_QUERY_FORBIDDEN` |
//! | 请求体里其它不认识的字段 | **400**（`deny_unknown_fields` 的同一条口径） |
//!
//! 请求体对 `/q/<id>` 是**可选**的（浏览器发 `{}` 也行）：门户的问题没有参数。
//!
//! # 429 从哪来
//!
//! 只有一处：[`super::limit`] 的每口令名额池。同一张口令同时在跑的问题数到了
//! `max_concurrency` ⇒ 第 N+1 个**立刻** 429 `TOO_MANY_CONCURRENT`（⛔ 不排队）。
//! 名额按口令隔离 ⇒ A 打满不影响 B。响应体仍是标准错误契约 `{code,message,hint}`，
//! 文案里没有表名 / 列名 / SQL / IRI。
//!
//! # 审计
//!
//! 每次问题执行一行（成功、失败、429 都记），鉴权失败也记（`code=UNAUTHORIZED`），
//! 这样「401 有多少次」「429 有多少次」都是能数出来的事实。`/api/health` 不记：
//! 探针每分钟几十次，记了只会把真事件淹掉。

use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path as AxumPath, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use super::tokens::PortalTokenEntry;
use super::{audit, page, run_question, PortalService};
use crate::query::error::ApiError;

/// 唯一免 token 的**数据**路径（只给版本，零数据）。
pub const HEALTH_PATH: &str = "/api/health";

/// 页面壳（**免 token、零数据**）。
pub const PAGE_PATH: &str = "/";

/// 问题列表（带 token；⛔ 不含 SPARQL）。
pub const QUESTIONS_PATH: &str = "/api/questions";

/// 执行一个问题的路径前缀。
pub const ASK_PREFIX: &str = "/q/";

/// 请求体上限。门户的 POST 不需要 body（最多一个 `{}`）；64 KiB 足够挡住
/// 「用超大 body 拖垮进程」，同时给「误发了别的东西」留一个可读的 400。
const MAX_BODY_BYTES: usize = 64 * 1024;

/// 身份门放进请求扩展的那一行口令（handler 只能拿到它）。
#[derive(Debug, Clone)]
pub struct Auth(pub PortalTokenEntry);

/// 组装路由。
pub fn build_router(svc: Arc<PortalService>) -> Router {
    Router::new()
        // ⭐ 页面壳与健康检查在身份门之外（浏览器导航带不了头；壳内零数据）。
        .route(PAGE_PATH, get(index))
        .route(HEALTH_PATH, get(health))
        .route(QUESTIONS_PATH, get(list_questions))
        .route("/q/:id", post(ask))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(svc.clone(), require_token))
        .with_state(svc)
}

// ══════════════════════════════════════════════════════════════
// 响应构造（**唯一**的两个写点）
// ══════════════════════════════════════════════════════════════

/// 三个安全头叠到任何响应上（页面与 API 都带；AC-007）。
fn with_security(mut res: Response) -> Response {
    for (name, value) in page::security_headers() {
        res.headers_mut().insert(name, value);
    }
    res
}

/// JSON 响应。全文件只有这一处 `Json(`（登记为 OUTLETS 的 I24）。
fn respond(status: StatusCode, body: Value) -> Response {
    with_security((status, Json(body)).into_response())
}

/// 错误的**唯一**出口（正文来自 [`ApiError::body`]，与查询面 / 控制面同一个形状）。
fn fail(e: ApiError) -> Response {
    respond(e.status, e.body())
}

/// 未注册路径。**在身份门之后**：无 token 时先得到 401（探测不到路径）。
///
/// 有 token 时得到 404 + 可读的「这里只有四个端点」—— 这正是 AC-002 要的
/// 「⛔ 没有 /api/sparql」的现场判据。
async fn not_found() -> Response {
    fail(ApiError::with_code(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "没有这个路径。",
        "门户只有四个端点：GET /（页面）、GET /api/questions（问题列表）、\
         POST /q/<id>（执行一个问题）、GET /api/health（免 token）。\
         ⛔ 没有自由查询入口（没有 /api/sparql），也没有任何写操作。",
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

/// 「门户不接受查询参数」的**唯一**构造点。
///
/// 为什么连 `/` 与 `/api/health` 也要拒：**规则有例外就会被滥用**。
/// 页面壳与健康检查确实零数据、带上查询串也无害 —— 但一条「`tenant` 在大多数
/// 端点会被拒」的规则，留两个例外就等于给了「先试试 / 会不会放行」一个探针，
/// 也让 AC-001 的判据从「一律 400/403」退化成「除两处以外」。
fn query_param_forbidden() -> ApiError {
    ApiError::with_code(
        StatusCode::BAD_REQUEST,
        "QUERY_PARAM_FORBIDDEN",
        "门户端点不接受 URL 查询参数。",
        "租户由访问口令决定：⛔ 不接受 `tenant`（或 `token` 之类的参数）。\
         去掉查询串再试 —— 你看到的永远只有口令绑定的那个租户的数据。",
    )
}

/// 身份门：没有 / 错的 token ⇒ 401（门户**不做** 403，见模块头）。
async fn require_token(
    State(svc): State<Arc<PortalService>>,
    mut req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let bad_query = req.uri().query().is_some_and(|q| !q.trim().is_empty());

    // ⭐ 豁免的只有**路径**（浏览器导航带不了 `Authorization`），不豁免查询串：
    // `/` 与 `/api/health` 带参数同样 400（与其它端点同一条规则）。
    if path == HEALTH_PATH || path == PAGE_PATH {
        if bad_query {
            audit::anonymize(&svc.audit, "QUERY_PARAM_FORBIDDEN", Instant::now());
            return fail(query_param_forbidden());
        }
        return next.run(req).await;
    }

    let started = Instant::now();
    let presented = bearer(req.headers());
    let Some(entry) = svc.tokens.find(presented) else {
        audit::anonymize(&svc.audit, "UNAUTHORIZED", started);
        return fail(ApiError::unauthorized());
    };

    // ⛔ 租户只能来自口令：URL 上的任何查询参数都不放行（含 ?tenant= / ?token=）。
    if bad_query {
        svc.audit
            .record(&entry, "-", None, started.elapsed(), "QUERY_PARAM_FORBIDDEN");
        return fail(query_param_forbidden());
    }

    req.extensions_mut().insert(Auth(entry.clone()));
    next.run(req).await
}

// ══════════════════════════════════════════════════════════════
// GET / · 页面壳（免 token · 零数据）
// ══════════════════════════════════════════════════════════════

/// ⭐ `GET /` —— **自包含**的页面壳。
///
/// 三件事各自只在一处：
/// - 正文：`portal::page::PORTAL_HTML`（壳里零数据，AC-005）；
/// - 响应头：`portal::page::security_headers()`（CSP / nosniff / no-referrer，AC-007）；
/// - 入口：本函数（`require_token` 对 `/` 放行**路径**；带查询串照样 400）。
///
/// ⛔ 不许在这里拼任何数据（项目名 / 版本 / 对象名 / 问题标题）：`/` 是匿名的。
async fn index() -> Response {
    // ⚠️ `Html` 的写点由 OUTLETS 的 I23 登记；`(headers, Html)` 的顺序保证
    // `content-type` 由 `Html` 给，三个安全头另外叠上去。
    with_security(Html(page::PORTAL_HTML).into_response())
}

// ══════════════════════════════════════════════════════════════
// GET /api/health（免 token）
// ══════════════════════════════════════════════════════════════

/// **免 token**，只给版本。⛔ 项目名 / 版本号 / 租户 / 问题数都不出去。
///
/// 连查询参数也不接受（`?tenant=` 一律 400）：健康检查是**唯一**该免鉴权的路径，
/// 但它不该成为「带 tenant 还能拿到 200」的那条缝。
/// （主闸在 [`require_token`] 里对**所有路径**统一判；这里再判一次是纵深防御。）
async fn health(uri: axum::http::Uri) -> Response {
    if uri.query().is_some_and(|q| !q.trim().is_empty()) {
        return fail(ApiError::with_code(
            StatusCode::BAD_REQUEST,
            "QUERY_PARAM_FORBIDDEN",
            "健康检查不接受查询参数。",
            "GET /api/health 只回服务名与版本；⛔ 不接受 `tenant` 之类的参数。",
        ));
    }
    respond(
        StatusCode::OK,
        json!({
            "service": "ontok-portal",
            "version": env!("CARGO_PKG_VERSION"),
            "api": "v1",
        }),
    )
}

// ══════════════════════════════════════════════════════════════
// GET /api/questions（标题 + 说明；⛔ 不含 SPARQL）
// ══════════════════════════════════════════════════════════════

async fn list_questions(State(svc): State<Arc<PortalService>>) -> Response {
    let cards = svc.questions.cards();
    respond(
        StatusCode::OK,
        json!({
            "count": cards.len(),
            "questions": cards,
        }),
    )
}

// ══════════════════════════════════════════════════════════════
// POST /q/<id>（执行一个问题）
// ══════════════════════════════════════════════════════════════

/// 请求体守卫：门户的问题**没有参数**。
///
/// - 空 body / `{}` ⇒ 放行（浏览器两种都可能发）；
/// - `tenant` 一类字段 ⇒ **403**（AC-001：拒绝，不是忽略）；
/// - `sparql` / `query` ⇒ **400**（⛔ 不许执行自定义查询）；
/// - 别的字段 ⇒ 400（拼错字段名不许静默）。
fn guard_body(body: &Bytes) -> Result<(), ApiError> {
    if body.is_empty() {
        return Ok(());
    }
    let v: Value = serde_json::from_slice(body).map_err(|_| {
        ApiError::bad_request(
            "请求体不是合法 JSON。",
            "门户的 POST /q/<id> **不需要**请求体：直接 POST，或发一个 `{}`。",
        )
    })?;
    let Some(obj) = v.as_object() else {
        return Err(ApiError::bad_request(
            "请求体必须是 JSON 对象。",
            "门户的 POST /q/<id> **不需要**请求体：直接 POST，或发一个 `{}`。",
        ));
    };
    // ① 身份字段：⛔ 一个都不接受（含 SQL 里那种 `customer_id`）。
    for key in ["tenant", "customer", "customer_id", "owner", "tenant_id"] {
        if obj.contains_key(key) {
            return Err(ApiError::with_code(
                StatusCode::FORBIDDEN,
                "TENANT_FROM_CLIENT",
                format!("请求里不能带 `{key}`。"),
                "租户由访问口令决定 —— 这是门户唯一的身份来源（结构上没有第二个）。\
                 ⛔ 门户不会「按你说的租户查」：要看别的租户请换一张绑定该租户的口令。",
            ));
        }
    }
    // ② 自定义查询：⛔ 不许执行（连「忽略」都不算 —— 要让对方知道这条路不存在）。
    for key in ["sparql", "query", "sql"] {
        if obj.contains_key(key) {
            return Err(ApiError::with_code(
                StatusCode::BAD_REQUEST,
                "CUSTOM_QUERY_FORBIDDEN",
                format!("门户不接受自定义查询（请求体里有 `{key}`）。"),
                "只能执行 GET /api/questions 里列出的预置问题 —— 每个问题都有人签过口径。\
                 ⛔ 门户没有自由查询入口（没有 /api/sparql）。",
            ));
        }
    }
    // ③ 其它字段：⛔ 不静默忽略（静默忽略会让调用方以为参数生效了）。
    if !obj.is_empty() {
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        return Err(ApiError::bad_request(
            format!("请求体里有不认识的字段：{}。", keys.join("、")),
            "门户的问题**没有参数**：发一个 `{}` 或不发 body 即可。",
        ));
    }
    Ok(())
}

async fn ask(
    State(svc): State<Arc<PortalService>>,
    Extension(auth): Extension<Auth>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    if let Err(e) = guard_body(&body) {
        // 审计：`qid` 走安全过滤（客户端可控输入不许改写日志形状）。
        svc.audit
            .record(&auth.0, &id, None, started.elapsed(), e.code);
        return fail(e);
    }
    let Some(q) = svc.questions.get(&id) else {
        svc.audit
            .record(&auth.0, &id, None, started.elapsed(), "UNKNOWN_QUESTION");
        return fail(ApiError::with_code(
            StatusCode::NOT_FOUND,
            "UNKNOWN_QUESTION",
            format!("没有叫 {id:?} 的问题。"),
            format!(
                "可用的问题：{}。用 GET /api/questions 取完整列表（标题 + 说明）。",
                svc.questions.ids().join("、")
            ),
        ));
    };
    match run_question(&svc, &auth.0, q).await {
        Ok(answer) => respond(
            StatusCode::OK,
            serde_json::to_value(answer).unwrap_or_else(|e| {
                // 理论上不会失败（全是 String / Vec）；真失败也不许 panic。
                json!({"code": "INTERNAL", "message": format!("回答序列化失败：{e}"), "hint": "联系服务方"})
            }),
        ),
        Err(e) => fail(e),
    }
}
