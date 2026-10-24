//! REQ-123 · **HTTP 适配层** —— REST 四个端点 + MCP over HTTP（JSON-RPC + SSE）。
//!
//! # 一个进程、两种协议、**一份鉴权**
//!
//! 身份门是 [`require_token`]（一个 `middleware::from_fn_with_state`），同时挂在
//! `/api/catalog`、`/api/describe`、`/api/sparql`、`/mcp` 上。它做三件事：
//! ① 只认 `Authorization: Bearer`（⛔ query string / cookie）；② 查 token 表；
//! ③ 把 [`Auth`] 放进请求扩展（handler 只能用表里那一行，⛔ 读不到请求体里的 tenant）。
//!
//! `/api/health` **在门之外**（AC-001：免 token）。它只给「这个部署是什么」：
//! `service` / `version`（**产品**版本）/ `project` / `ontology_version`（**本体**版本，
//! ⭐ REQ-139）/ `syntax` / `protocols` / `transports`（本体模式另带 `mode`）——
//! ⛔ 租户 id、连接串、表名一个都不给（R2）。`project` / `ontology_version` 与
//! `/api/catalog` 的 `project` / `version` 是**同一对字段**（R4）。
//!
//! # 每个 handler 都只做「解析 → 调适配器 → 序列化」
//!
//! 查询/目录/详情/取样的**全部逻辑**在 [`super::run_sparql`] / [`super::run_catalog`] /
//! [`super::run_describe`] / [`super::run_sample`] 里 —— MCP 的 `tools/call` 调的是
//! **同一批函数**。所以「REST 拦住了、MCP 忘了拦」在结构上没有发生的位置
//! （纪律 H / AC-004）。
//!
//! # 响应正文只有两个写点
//!
//! - [`respond`]：REST 的 JSON 响应（`Json(` 只在这里出现一次）；
//! - `p::encode_result(`：MCP 的 JSON-RPC 信封（复用 `mcp::protocol`，⛔ 不另写一套协议）。
//!
//! 两者都登记在 `domain::pipeline::OUTLETS`（I14 / I15 / I16），
//! `tests/test_req107_source_enumeration.rs` 的 T1 会逐个对账。
//!
//! # 已知限制
//!
//! - GET `/mcp` 的 SSE 是**服务端单向**流：只发 `notifications/message` 与注释型
//!   keep-alive。客户端发来的消息请走 POST（Streamable HTTP 的正常形态）。
//! - 客户端**断开**只能在下一次写时被发现（写失败 ⇒ 收工）；空闲超时是兜底。

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{json, Value};

use crate::mcp::protocol as p;

use super::error::ApiError;
use super::tokens::TokenEntry;
use super::ttl::TtlView;
use super::{
    audit_anonymous, audit_ttl, run_catalog, run_describe, run_sample, run_sparql, Caller,
    CatalogRequest, DescribeRequest, Protocol, QueryService, SampleRequest, SparqlRequest,
    SYNTAX_VERSION,
};

/// 请求体上限（一条 SPARQL 远小于这个数；挡住「用超大 body 拖垮进程」）。
const MAX_BODY_BYTES: usize = 256 * 1024;

/// SSE keep-alive 的间隔（也是「空闲」的探测粒度）。
const SSE_KEEPALIVE: Duration = Duration::from_secs(5);

/// ⭐ REQ-127 · 整份 OWL（Turtle）的端点路径。
pub const TTL_PATH: &str = "/api/ontology.ttl";

/// ⭐ REQ-127 · 两个 MCP resource 的 URI（与 HTTP 同一份字节）。
pub const RESOURCE_CUSTOMER: &str = "ontok://ontology/customer.ttl";
pub const RESOURCE_MODEL: &str = "ontok://ontology/model.ttl";

/// TTL 响应的 `Content-Type`（AC-001）。
pub const TTL_CONTENT_TYPE: &str = "text/turtle; charset=utf-8";

/// 服务端发给客户端的 `initialize.instructions`。
///
/// 只写**怎么用**与**租户从哪来**，不写任何数据。⛔ 不放项目名/租户名。
pub const INSTRUCTIONS: &str = "\
ontok 查询服务（远程 MCP）。工具：catalog（翻页看类/属性/关系）、describe（看一个名字的详情）、\
sparql_query（执行 SPARQL v1 子集）、sample（取样例值）。
整份本体：resources/list 里的 ontok://ontology/customer.ttl（客户视图）与 \
ontok://ontology/model.ttl（建模视图，含主键/枚举/租户列/证据等级），用 resources/read 取原文（Turtle）。
租户由你的凭据决定：请求里不需要也不能改变租户；带 tenant 字段时必须与凭据一致，否则 403。
SPARQL v1 只支持单条 SELECT + 基本图模式（1–3 跳）+ FILTER + LIMIT/OFFSET + 单列 ORDER BY +
COUNT/SUM/MIN/MAX/AVG + 单列 GROUP BY；不支持 OPTIONAL/UNION/子查询/BIND/属性路径/函数。
出错时读 hint（可行动）；找不到名字时先用 catalog。";

/// 身份门放进请求扩展的那一行 token（handler 只能拿它，⛔ 拿不到请求体里的 tenant）。
#[derive(Debug, Clone)]
pub struct Auth(pub TokenEntry);

/// 组装路由。
///
/// - `/api/health` 免 token（产品版本 + 项目名 + **本体**版本 + 语法版本；⛔ 无租户/表名）；
/// - `/api/*` 与 `/mcp` 全部过 [`require_token`]；
/// - **未注册的 `/api/*` 也走身份门**（无 token ⇒ 401，不泄露「这个接口存不存在」）；
///   已授权的人问路 ⇒ 404 + 可用端点清单。
pub fn build_router(svc: Arc<QueryService>) -> Router {
    // ⭐ REQ-135 · 指标层挂在**最外层**：连 401 / 404 都计数（失败也是事实）。
    let metrics = svc.metrics.clone();
    Router::new()
        .route("/api/health", get(health))
        // ⭐ REQ-135 · `/metrics`：免 token（与 `/api/health` 同口径），零业务数据。
        .route(crate::metrics::METRICS_PATH, get(get_metrics))
        .route("/api/sparql", post(post_sparql))
        .route("/api/catalog", get(get_catalog))
        .route("/api/describe", get(get_describe))
        // ⭐ REQ-127 · 整份 OWL：默认客户视图，`?view=model` 建模视图（同一道身份门）。
        .route(TTL_PATH, get(get_ontology_ttl))
        .route("/mcp", post(mcp_post).get(mcp_sse))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(svc.clone(), require_token))
        .with_state(svc)
        .layer(middleware::from_fn_with_state(metrics, crate::metrics::track))
}

// ══════════════════════════════════════════════════════════════
// 响应构造（**唯一**的 JSON 写点）
// ══════════════════════════════════════════════════════════════

/// REST 的 JSON 响应。全文件只有这一处 `Json(`（登记为 OUTLETS 的 I14）。
fn respond<T: Serialize>(status: StatusCode, body: T) -> Response {
    (status, Json(body)).into_response()
}

/// MCP 的 JSON-RPC 响应：正文是**已经编码好的**信封（`p::encode_*` 的返回值）。
///
/// 不在这里 `Json(Value)` 再序列化一遍：那会让 `tools/call` 的
/// `content[0].text` 与 REST 正文多一层转义，逐字节对拍就不可能相等。
fn respond_rpc(status: StatusCode, line: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))],
        line,
    )
        .into_response()
}

/// 未注册路径。
///
/// ⚠️ 它**在身份门之后**被调用：`/api/*` 与 `/mcp` 的未注册路径，无 token 时
/// 先得到 **401**（探测不到路径）；带 token 才看到这段清单（已授权的人问路该说实话）。
async fn not_found() -> Response {
    respond(
        StatusCode::NOT_FOUND,
        json!({
            "code": "NOT_FOUND",
            "message": "没有这个路径。",
            "hint": "可用：GET /api/catalog、GET /api/describe?name=…、POST /api/sparql、\
                     GET /api/ontology.ttl（?view=model 取建模视图）、\
                     POST /mcp（JSON-RPC）、GET /mcp（SSE）、GET /api/health、GET /metrics（后两者免 token）。",
        }),
    )
}

// ══════════════════════════════════════════════════════════════
// 身份门（**唯一**一处；REST 与 MCP 共用）
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
    // `str::get(..7)` 是**边界安全**的：`v[..7]` 在多字节字符上会 panic。
    match v.get(..7) {
        Some(p) if p.eq_ignore_ascii_case("bearer ") => v[7..].trim(),
        _ => "",
    }
}

/// 唯一免 token 的路径（AC-001：只给版本与语法版本，零数据）。
pub const HEALTH_PATH: &str = "/api/health";

/// 这个路径要不要身份门？
///
/// ⛔ 判定只看**路径前缀**，不看方法 / 参数 —— 于是「新加一个 `/api/*` 路由却忘了
/// 加门」在这里是**默认要门**，忘不掉。`/api/health` 是唯一豁免，且它在下一行里
/// 被写成一个**精确**路径（不是前缀），别的地方不会被顺带豁免。
pub fn needs_auth(path: &str) -> bool {
    if path == HEALTH_PATH {
        return false;
    }
    path == "/mcp" || path.starts_with("/mcp/") || path == "/api" || path.starts_with("/api/")
}

/// 身份门。**没有开关能绕过它** —— 只有它认识的那张表说了算。
async fn require_token(
    State(svc): State<Arc<QueryService>>,
    mut req: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    if !needs_auth(req.uri().path()) {
        return next.run(req).await;
    }
    let proto = proto_of(req.uri().path());
    let presented = bearer(req.headers());
    match svc.tokens.find(presented) {
        Some(entry) => {
            req.extensions_mut().insert(Auth(entry.clone()));
            next.run(req).await
        }
        None => {
            let e = ApiError::unauthorized();
            // 审计：这一次**没有身份**可用（不写「哪个 token 错了」——那等于泄漏）。
            // ⭐ REQ-127：TTL 端点即使 401 也记下「要的是哪个视图」（AC-006 的四个字段之一）。
            let view = ttl_view_hint(req.uri().path(), req.uri().query());
            audit_anonymous(&svc, "auth", proto, view, e.code, started);
            respond(e.status, e.body())
        }
    }
}
fn proto_of(path: &str) -> Protocol {
    if path.starts_with("/mcp") {
        Protocol::Mcp
    } else {
        Protocol::Rest
    }
}

// ══════════════════════════════════════════════════════════════
// REST
// ══════════════════════════════════════════════════════════════

/// `GET /api/health` —— **免 token**，给产品版本 + 语法版本 + **本部署是哪个项目 / 哪版本体**。
///
/// ⭐ REQ-136 · 本体服务模式多一个 `"mode":"ontology-only"`（AC-001 的判据）。
/// 普通模式**不加**这个字段（既有响应逐字节不变，AC-006）—— 探针要区分部署时，
/// 看「有没有 mode 字段」即可；带上它的部署就是「只有本体、没有数据源」。
///
/// ⭐ REQ-139 · 两个新字段（两种模式都有，R3）：
///
/// - `project`：项目名（`--project`）。它本来就出现在 TTL (`ontok:sourceProject`) 与
///   `/api/catalog` 里，公开无害；⛔ 租户 id / 连接串 / 表名**照样一个都不给**（R2）。
/// - `ontology_version`：**本体**版本（`--version` 指向的、已冻结的快照版本号，整数）。
///   与 `version`（**产品**版本 = `CARGO_PKG_VERSION`，如 `0.1.0`）是两件事，⛔ 别混读。
///
/// ⛔ R4：这两个值**就是** `svc.project` / `svc.version` —— 与 [`catalog::page`] 里
/// `CatalogPage { project, version }` 填的是**同一对字段**（都在 [`QueryService::new`] 里由
/// `ServiceParts` 一次定下，运行期不变）。所以不存在「两处各算一次」：health 不重新查库、
/// 不从 IRI/TTL 里反解项目名，只把启动时那一个事实读出来。
async fn health(State(svc): State<Arc<QueryService>>) -> Response {
    let started = Instant::now();
    // ⛔ 只读「这个服务是哪个项目 / 哪版本体 / 什么产品版本 / 说哪种语法（以及哪种模式）」。
    // 租户、连接串、表清单、类清单**仍然一个都不出去**（R2）。
    audit_anonymous(&svc, "health", Protocol::Rest, None, "OK", started);
    let mut body = json!({
        "service": "ontok-query",
        "version": env!("CARGO_PKG_VERSION"),
        "syntax": SYNTAX_VERSION,
        "protocols": ["rest", "mcp-http"],
        "transports": ["http", "sse"],
        // ⭐ REQ-139 · 与 `/api/catalog` 的 `project` / `version` 同一对字段（R4）。
        "project": svc.project,
        "ontology_version": svc.version,
    });
    if svc.mode.is_ontology_only() {
        // `mode` 是**部署形状**，不是数据：它告诉探针「这个进程不会去连业务库」。
        body["mode"] = json!(svc.mode.as_str());
    }
    respond(StatusCode::OK, body)
}

/// ⭐ REQ-135 · `GET /metrics` —— **免 token**（与 `/api/health` 同口径）的聚合数字。
///
/// 响应体由 [`crate::metrics::Metrics::http_response`] **唯一**构造（三个服务共用），
/// 这里只把本进程的注册表交出去。⛔ 不读任何业务字段：指标里没有租户 / 表名 / SQL。
async fn get_metrics(State(svc): State<Arc<QueryService>>) -> Response {
    svc.metrics.http_response()
}

/// `POST /api/sparql` —— body `{query, max_rows?, tenant?}`。
async fn post_sparql(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let caller = Caller {
        token: auth.0,
        protocol: Protocol::Rest,
    };
    let value = match parse_json(&body) {
        Ok(v) => v,
        Err(e) => {
            super::audit_call(&svc, "sparql", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };
    let req = match SparqlRequest::from_value(&value) {
        Ok(r) => r,
        Err(e) => {
            super::audit_call(&svc, "sparql", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };
    match run_sparql(&svc, &caller, req).await {
        // ⭐ 回执的**唯一**序列化路径：`Json(receipt)` 与 MCP 的
        // `content[0].text = receipt.serialize()` 出自同一个 `Serialize` 实现。
        Ok(receipt) => respond(StatusCode::OK, receipt),
        Err(e) => respond(e.status, e.body()),
    }
}

/// `GET /api/catalog` —— 分页的类 / 属性 / 关系（含证据等级与枚举值）。
async fn get_catalog(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let started = Instant::now();
    let caller = Caller {
        token: auth.0,
        protocol: Protocol::Rest,
    };
    let req = match CatalogRequest::from_query(&q) {
        Ok(r) => r,
        Err(e) => {
            super::audit_call(&svc, "catalog", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };
    match run_catalog(&svc, &caller, &req) {
        Ok(page) => respond(StatusCode::OK, page),
        Err(e) => respond(e.status, e.body()),
    }
}

/// `GET /api/describe?name=` —— 类/属性详情；未知 ⇒ 404 + 相近项。
async fn get_describe(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let started = Instant::now();
    let caller = Caller {
        token: auth.0,
        protocol: Protocol::Rest,
    };
    let req = match DescribeRequest::from_query(&q) {
        Ok(r) => r,
        Err(e) => {
            super::audit_call(&svc, "describe", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };
    match run_describe(&svc, &caller, &req) {
        Ok(d) => respond(StatusCode::OK, d),
        Err(e) => respond(e.status, e.body()),
    }
}

/// ⭐ REQ-127 · `GET /api/ontology.ttl` —— **整份 OWL（Turtle）**。
///
/// - 默认**客户视图**；`?view=model` 才是建模视图（R1：客户 token 不该一眼拿到内部列名）；
/// - 两种视图都**不注入租户**（本体是共享结构，不含客户数据）；
/// - 字节是启动时用 `prepare_ttl_export` + `pipeline::render` 算好并缓存的
///   —— 与 `ontok ontology export` 同参数时**逐字节相同**（AC-004）；
/// - `ETag` = 正文 sha256 前 16 位；`If-None-Match` 命中 ⇒ 304 且无体（AC-005）；
/// - 每次请求一行审计（含 401 / 400 / 304）（AC-006）。
async fn get_ontology_ttl(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let started = Instant::now();
    let caller = Caller {
        token: auth.0,
        protocol: Protocol::Rest,
    };
    let view = match ttl_view_from_query(&q) {
        Ok(v) => v,
        Err(e) => {
            // 参数不合法也记一行（AC-006：每次请求一行）。这一刻还没有可发的字节。
            audit_ttl(&svc, &caller, TtlView::Customer, None, started, e.code);
            return respond(e.status, e.body());
        }
    };

    let body = svc.ttl.get(view);
    if if_none_match(&headers, body.etag()) {
        audit_ttl(&svc, &caller, view, Some(0), started, "NOT_MODIFIED");
        // 304 **无体**：只回 ETag（AC-005 的判据是 Content-Length 为 0 或不存在）。
        return ttl_response(StatusCode::NOT_MODIFIED, "", body.etag());
    }

    audit_ttl(&svc, &caller, view, Some(body.bytes()), started, "OK");
    ttl_response(StatusCode::OK, body.text(), body.etag())
}

/// `?view=` 的解析：默认客户视图；`model` 显式要建模视图。
///
/// ⛔ 不认识的参数 / 取值 **400**，不静默忽略 —— 拼错 `view=modle` 却拿到客户视图，
/// 调用方会以为「建模视图里没有主键」，那是错误的事实。
fn ttl_view_from_query(q: &BTreeMap<String, String>) -> Result<TtlView, ApiError> {
    let mut view = TtlView::Customer;
    for (k, v) in q {
        match k.as_str() {
            "view" => {
                view = TtlView::parse(v).ok_or_else(|| {
                    ApiError::bad_request(
                        format!("view 只认 customer / model，收到 {v:?}。"),
                        "默认是**客户视图**（GET /api/ontology.ttl）；要含主键 / 枚举 / 租户列 / \
                         证据等级的**建模视图**写 `?view=model`。⛔ 拼错不会静默回退。",
                    )
                })?
            }
            other @ ("token" | "access_token" | "api_key") => {
                return Err(ApiError::bad_request(
                    format!("query string 里不要放 {other}。"),
                    "令牌只认 `Authorization: Bearer <token>` 头；\
                     放在 query string 里**不生效**，服务端也不会读取它（R5）。",
                ))
            }
            other => {
                return Err(ApiError::bad_request(
                    format!("不认识的查询参数 {other:?}。"),
                    "`/api/ontology.ttl` 只接受 `view=customer|model`（省略 = 客户视图）。",
                ))
            }
        }
    }
    Ok(view)
}

/// `If-None-Match` 是否命中（支持 `*`、弱校验 `W/`、逗号分隔的多值）。
fn if_none_match(headers: &HeaderMap, etag: &str) -> bool {
    let Some(raw) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let want = etag.trim_matches('"');
    raw.split(',').any(|part| {
        let t = part.trim();
        let t = t.strip_prefix("W/").unwrap_or(t).trim();
        t == "*" || t.trim_matches('"') == want
    })
}

/// TTL 响应（唯一的 Turtle 响应构造点；正文来自 `OntologyTtl` 的缓存字节）。
fn ttl_response(status: StatusCode, text: &str, etag: &str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(TTL_CONTENT_TYPE),
    );
    // ETag 由内容摘要拼出来（`"<16 hex>"`），只含 ASCII；失败也只是少一个头，不该 panic。
    if let Ok(value) = HeaderValue::from_str(etag) {
        headers.insert(header::ETAG, value);
        // 本体随版本冻结：客户端每次都要拿 ETag 复核，⛔ 不让中间层自己猜。
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    (status, headers, text.to_string()).into_response()
}

/// 401 路径上「要的是哪个视图」的**不解析**提示（只给审计用；不参与任何判定）。
fn ttl_view_hint(path: &str, query: Option<&str>) -> Option<&'static str> {
    if path != TTL_PATH {
        return None;
    }
    let requested = query
        .unwrap_or("")
        .split('&')
        .find_map(|kv| kv.strip_prefix("view="))
        .unwrap_or("");
    Some(if requested == "model" {
        TtlView::Model.as_str()
    } else {
        TtlView::Customer.as_str()
    })
}

// ══════════════════════════════════════════════════════════════
// MCP over HTTP
// ══════════════════════════════════════════════════════════════

/// 本服务暴露的四个工具（`tools/list` 与 `tools/call` 都从这里取名字）。
pub const TOOL_NAMES: &[&str] = &["catalog", "describe", "sparql_query", "sample"];

/// `POST /mcp` —— JSON-RPC 2.0（`initialize` / `tools/list` / `tools/call` / `ping`）。
async fn mcp_post(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    let caller = Caller {
        token: auth.0,
        protocol: Protocol::Mcp,
    };
    let text = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(e) => {
            let e = ApiError::bad_request(
                format!("请求体不是 UTF-8：{e}"),
                "MCP 的 POST body 是一行 JSON-RPC 2.0（UTF-8）。",
            );
            super::audit_call(&svc, "mcp", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };
    match p::decode_line(text) {
        p::Incoming::Request(req) => {
            // `tools/call` / `resources/read` 的审计在**适配器**里（一次调用一条，含 bytes/错误码）；
            // 其余方法（initialize / tools/list / resources/list / ping）在这里补一条信封级审计。
            if req.method != "tools/call" && req.method != "resources/read" {
                super::audit_call(&svc, "mcp", &caller, None, None, started, "OK", None);
            }
            let line = dispatch(&svc, &caller, &req).await;
            respond_rpc(StatusCode::OK, line)
        }
        // 通知**不许回响应**（回了就是协议违规）。HTTP 侧用一个空 202 表示「收到了」。
        p::Incoming::Notification(_) => respond_rpc(StatusCode::ACCEPTED, String::new()),
        p::Incoming::Invalid { id, code, message } => {
            let line = match id {
                Some(id) => p::encode_error_line(&id, code, &message, None),
                None => p::encode_error_no_id(code, &message),
            };
            // 解析层错误也要有审计（一次调用、一次失败）。
            super::audit_call(
                &svc,
                "mcp",
                &caller,
                None,
                None,
                started,
                "BAD_REQUEST",
                None,
            );
            respond_rpc(StatusCode::OK, line)
        }
    }
}

/// JSON-RPC 分发。返回**已编码**的一行（`p::encode_*` 的输出）。
async fn dispatch(svc: &Arc<QueryService>, caller: &Caller, req: &p::Request) -> String {
    let id = &req.id;
    match req.method.as_str() {
        "initialize" => {
            let requested = req.params.get("protocolVersion").and_then(Value::as_str);
            let agreed = p::negotiate_protocol_version(requested);
            // ⭐ REQ-136 · 本体服务模式的 instructions **要说清「没有数据源」**：
            // Agent 只看这一段就决定要不要调 sparql_query；不说 = 它一定会调一次、失败一次。
            let instructions = if svc.mode.is_ontology_only() {
                format!(
                    "{INSTRUCTIONS}\n\
                     ⚠️ 本部署是**本体服务模式**（无业务库）：catalog / describe / resources 正常，\
                     sparql_query 与 sample 恒返回 NO_DATA_SOURCE（503 语义，重试无效）。"
                )
            } else {
                INSTRUCTIONS.to_string()
            };
            let mut result = p::initialize_result(agreed, &instructions);
            // ⭐ REQ-127：本服务**真的**实现了 resources（list/read），所以在这里声明能力。
            // 共享的 `p::initialize_result` 不声明它 —— stdio 的 `mcp-server` 没有资源，
            // 在那里声明会让客户端撞一次 -32601。
            if let Some(caps) = result.get_mut("capabilities").and_then(Value::as_object_mut) {
                caps.insert(
                    "resources".to_string(),
                    json!({ "subscribe": false, "listChanged": false }),
                );
            }
            p::encode_result(id, result)
        }
        "notifications/initialized" => p::encode_result(id, p::empty_result()),
        "ping" => p::encode_result(id, p::empty_result()),
        "tools/list" => p::encode_result(id, p::tools_list_result(Value::Array(tool_defs()))),
        "tools/call" => tools_call(svc, caller, req).await,
        // ⭐ REQ-127 · 整份 OWL 作为 **resource**（不是 tool）：见 `RESOURCE_*` 与交付说明。
        "resources/list" => p::encode_result(
            id,
            json!({ "resources": resource_defs() }),
        ),
        "resources/read" => resources_read(svc, caller, req),
        other => p::encode_error_line(
            id,
            p::METHOD_NOT_FOUND,
            &format!("不支持的方法：{other}"),
            Some(json!({
                "code": "METHOD_NOT_FOUND",
                "hint": "本服务支持 initialize / tools/list / tools/call / ping（以及 notifications/initialized）。"
            })),
        ),
    }
}

/// `tools/call`。**四个工具都调到与 REST 完全相同的适配器函数**。
async fn tools_call(svc: &Arc<QueryService>, caller: &Caller, req: &p::Request) -> String {
    let id = &req.id;
    let name = req.params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = req
        .params
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Null);

    if !TOOL_NAMES.contains(&name) {
        return p::encode_error_line(
            id,
            p::INVALID_PARAMS,
            &format!("没有这个工具：{name}"),
            Some(json!({
                "code": "UNKNOWN_TOOL",
                "hint": format!("可用工具：{}。", TOOL_NAMES.join(" / ")),
            })),
        );
    }

    match name {
        "sparql_query" => match SparqlRequest::from_value(&arguments) {
            Ok(r) => match run_sparql(svc, caller, r).await {
                // ⭐ `content[0].text` 与 REST 正文逐字节相同（同一个 `serialize`）。
                Ok(receipt) => tool_ok(id, &receipt.serialize(), serde_json::to_value(&receipt).unwrap_or(Value::Null)),
                Err(e) => tool_err(id, &e),
            },
            Err(e) => tool_err(id, &e),
        },
        "catalog" => match CatalogRequest::from_value(&arguments) {
            Ok(r) => match run_catalog(svc, caller, &r) {
                Ok(page) => tool_ok(id, &as_text(&page), serde_json::to_value(&page).unwrap_or(Value::Null)),
                Err(e) => tool_err(id, &e),
            },
            Err(e) => tool_err(id, &e),
        },
        "describe" => match DescribeRequest::from_value(&arguments) {
            Ok(r) => match run_describe(svc, caller, &r) {
                Ok(d) => tool_ok(id, &as_text(&d), serde_json::to_value(&d).unwrap_or(Value::Null)),
                Err(e) => tool_err(id, &e),
            },
            Err(e) => tool_err(id, &e),
        },
        "sample" => match SampleRequest::from_value(&arguments) {
            Ok(r) => match run_sample(svc, caller, r).await {
                Ok(receipt) => tool_ok(id, &receipt.serialize(), serde_json::to_value(&receipt).unwrap_or(Value::Null)),
                Err(e) => tool_err(id, &e),
            },
            Err(e) => tool_err(id, &e),
        },
        // 上面已经用 `TOOL_NAMES` 拦过；这里保留一个**可行动**的兜底而不是 panic。
        other => p::encode_error_line(
            id,
            p::INVALID_PARAMS,
            &format!("没有这个工具：{other}"),
            Some(json!({ "code": "UNKNOWN_TOOL", "hint": format!("可用工具：{}。", TOOL_NAMES.join(" / ")) })),
        ),
    }
}

/// `tools/call` 的成功结果（MCP 规范：`content` 文本 + `structuredContent`）。
fn tool_ok(id: &Value, text: &str, structured: Value) -> String {
    p::encode_result(id, p::tool_call_result(text, structured))
}

/// `tools/call` 的**执行**失败（`isError: true`；连接仍然健康）。
///
/// 正文与 REST 用的是**同一个** `ApiError::body()` ⇒ 两个协议的 `code` / `hint`
/// 逐字节相同（AC-004 的错误侧判据）。
fn tool_err(id: &Value, e: &ApiError) -> String {
    p::encode_result(
        id,
        json!({
            "content": [ { "type": "text", "text": e.json() } ],
            "structuredContent": e.body(),
            "isError": true,
        }),
    )
}

/// 结构化值的序列化（catalog / describe 的 `content[0].text`）。
fn as_text<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| format!("{{\"error\":\"序列化失败：{e}\"}}"))
}

/// `tools/list` 的工具清单。
fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "catalog",
            "title": "目录（类 / 属性 / 关系）",
            "description": "分页列出可查的类、数据类型属性与关系，含关系证据等级（true/inherit/false）\
                            与注释里明写的枚举值。写 SPARQL 之前先看它。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "page": { "type": "integer", "minimum": 1, "description": "页码，从 1 起" },
                    "page_size": { "type": "integer", "minimum": 1, "maximum": 200, "description": "每页条数，默认 50" }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "describe",
            "title": "类 / 属性详情",
            "description": "看一个类或一个属性的详情：主键、全部属性（含类型/枚举值/样例）、出入关系、\
                            以及这张表的租户口径（filter/public/unregistered）。未知名字会返回相近项。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "类本地名（:customers）或属性本地名（:customers__id）" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "sparql_query",
            "title": "执行 SPARQL（v1 子集）",
            "description": "执行一条只读 SPARQL SELECT（租户条件由服务端按声明注入，⛔ 调用方改不了）。\
                            回执含 columns/rows/row_count/truncated/sql_echo/tenant_applied/tables/\
                            public_dimensions/warnings/coverage/join_states/join_states_scope/\
                            join_states_scope_note/limits。\
                            ⚠️ join_states 的范围 = 「**查询涉及的表之间**的全部映射边」\
                            （join_states_scope 恒为 all_edges_between_involved_tables），\
                            ⛔ 不是本次查询实际走过的那几条：只要两张表都出现在 tables 里，\
                            映射里它们之间的每一条边都会列出，所以会有 used=false 的条目\
                            （= 涉及但本次没用到；人话说明见 join_states_scope_note）。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "一条 SELECT（v1 子集）" },
                    "max_rows": { "type": "integer", "minimum": 1, "description": "行数上限；不会超过凭证上限" },
                    "tenant": { "type": "string", "description": "**只用于核对**：必须等于凭证绑定的租户，否则 403。正常调用不用写。" }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "sample",
            "title": "取样例值",
            "description": "取某个类（实例 IRI）或某个属性（值）的少量样例。走的是同一条 SPARQL 查询链路\
                            （带租户注入与限额），只是查询由服务端生成。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "类本地名（:customers）或属性本地名（:customers__id）" },
                    "limit": { "type": "integer", "minimum": 1, "description": "取几条（默认 10，不会超过凭证上限）" }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
    ]
}

/// `resources/list` 的清单（两个视图各一条；URI 与 `resources/read` 的入参一一对应）。
fn resource_defs() -> Vec<Value> {
    vec![
        json!({
            "uri": RESOURCE_CUSTOMER,
            "name": "ontology-customer.ttl",
            "title": "本体（客户视图 · Turtle）",
            "mimeType": TTL_CONTENT_TYPE,
            "description": "整份 OWL/Turtle 的**客户视图**：业务名 + 关系，不含主键 / 列名 / 枚举 / \
                            租户标注 / join 证据。默认就是这一份。",
        }),
        json!({
            "uri": RESOURCE_MODEL,
            "name": "ontology-model.ttl",
            "title": "本体（建模视图 · Turtle）",
            "mimeType": TTL_CONTENT_TYPE,
            "description": "整份 OWL/Turtle 的**建模视图**：含 ontok:key / 列名 / 主键 / 枚举 / \
                            租户列 / join 证据等级。内部列名在此轨是**要被裁决的内容**，\
                            ⛔ 不要把它当成客户可见材料。",
        }),
    ]
}

/// URI → 视图（只认两条；⛔ 不做前缀匹配 —— `ontok://ontology/../` 之类不该被猜出来）。
fn resource_view(uri: &str) -> Option<TtlView> {
    match uri {
        RESOURCE_CUSTOMER => Some(TtlView::Customer),
        RESOURCE_MODEL => Some(TtlView::Model),
        _ => None,
    }
}

/// `resources/read` —— 返回**与 HTTP 同视图完全相同的字节**（同一个 `OntologyTtl` 缓存）。
///
/// 协议形状：`{"contents":[{"uri","mimeType","text"}]}`。`text` 是 Turtle 原文，
/// 客户端解析出来之后 sha256 与 `GET /api/ontology.ttl?view=…` 的正文相等（AC-007）。
fn resources_read(svc: &Arc<QueryService>, caller: &Caller, req: &p::Request) -> String {
    let started = Instant::now();
    let uri = req.params.get("uri").and_then(Value::as_str).unwrap_or("");
    let Some(view) = resource_view(uri) else {
        audit_ttl(svc, caller, TtlView::Customer, None, started, "RESOURCE_NOT_FOUND");
        return p::encode_error_line(
            &req.id,
            p::INVALID_PARAMS,
            &format!("没有这个资源：{uri}"),
            Some(json!({
                "code": "RESOURCE_NOT_FOUND",
                "hint": format!("可用：{RESOURCE_CUSTOMER} / {RESOURCE_MODEL}。"),
            })),
        );
    };
    let body = svc.ttl.get(view);
    audit_ttl(svc, caller, view, Some(body.bytes()), started, "OK");
    p::encode_result(
        &req.id,
        json!({
            "contents": [ {
                "uri": uri,
                "mimeType": TTL_CONTENT_TYPE,
                "text": body.text(),
            } ]
        }),
    )
}

/// `GET /mcp` —— SSE 流（服务端消息）。
///
/// 三条资源约束都在建流**之前**判：每 token 连接数、全局连接数（超限 ⇒ 429）；
/// 空闲超时由流自己数（到点主动收尾并记审计）。
///
/// # 为什么用 `stream::unfold` 而不是 channel
///
/// `Sse` 要一个 `TryStream`。用 channel（`tokio::sync::mpsc` + `ReceiverStream`）需要
/// `tokio-stream` 的 `sync` feature，而那会把 `tokio-util` 拉进依赖树。
/// `unfold` 把「第几步 → 下一个事件」写成状态机：不需要 channel、不需要后台任务，
/// 连接数名额（`slot`）跟着状态机一起被 drop —— 客户端断开时 axum 丢掉 body ⇒
/// `slot` 归还，**不需要**等一次写失败。
async fn mcp_sse(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
) -> Response {
    let started = Instant::now();
    let caller = Caller {
        token: auth.0,
        protocol: Protocol::Mcp,
    };
    let slot = match svc.acquire_sse(&caller.token) {
        Ok(s) => s,
        Err(e) => {
            super::audit_call(&svc, "sse", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };

    let idle = Duration::from_millis(svc.sse_idle_timeout_ms);
    let ready = json!({
        "jsonrpc": "2.0",
        "method": "notifications/message",
        "params": { "level": "info", "data": { "event": "ready", "syntax": SYNTAX_VERSION } }
    })
    .to_string();

    // 状态机：`step = 0` 先发 ready；之后每 `SSE_KEEPALIVE` 发一条注释型 keep-alive，
    // 到空闲上限就结束（`None`），并在结束前记一条审计。
    let state = (0u64, started, slot, Arc::clone(&svc), caller);
    let stream = futures_util::stream::unfold(state, move |(step, started, slot, svc, caller)| {
        // `ready` 必须在**闭包外**被捕获、每次 clone 进 future：FnMut 返回的 future
        // 不能借用闭包自身捕获的状态（那是借用检查器不允许的跨轮借用）。
        let ready = ready.clone();
        async move {
            if step == 0 {
                let first =
                    Ok::<Event, Infallible>(Event::default().event("message").data(ready));
                return Some((first, (1, started, slot, svc, caller)));
            }
            // 睡到**下一个 keep-alive** 或**空闲上限**，取较近的那个 ——
            // 否则「8 s 的空闲超时」会被 5 s 的 keep-alive 网格取整成 10 s。
            let elapsed = started.elapsed();
            if elapsed >= idle {
                super::audit_call(&svc, "sse", &caller, None, None, started, "OK", Some("idle_timeout"));
                return None; // slot 在状态里 ⇒ 在这里 drop（名额归还）
            }
            let wait = SSE_KEEPALIVE.min(idle - elapsed);
            tokio::time::sleep(wait).await;
            if started.elapsed() >= idle {
                super::audit_call(&svc, "sse", &caller, None, None, started, "OK", Some("idle_timeout"));
                return None;
            }
            let ping = Ok::<Event, Infallible>(Event::default().comment("keep-alive"));
            Some((ping, (step + 1, started, slot, svc, caller)))
        }
    });

    Sse::new(stream).into_response()
}

// ══════════════════════════════════════════════════════════════
// 小工具
// ══════════════════════════════════════════════════════════════

fn parse_json(body: &[u8]) -> Result<Value, ApiError> {
    if body.is_empty() {
        return Err(ApiError::bad_request(
            "请求体是空的。",
            "POST 一个 JSON 对象：{\"query\": \"SELECT ?s WHERE { ?s a :customers }\", \"max_rows\": 100}。",
        ));
    }
    serde_json::from_slice(body).map_err(|e| {
        ApiError::bad_request(
            format!("请求体不是合法 JSON：{e}"),
            "body 必须是 JSON 对象，形如 {\"query\": \"…\", \"max_rows\": 100}；\
             `max_rows` 可省，`tenant` 只用于核对（正常不用写）。",
        )
    })
}

// ══════════════════════════════════════════════════════════════
// 判据（进程内 oneshot，不连库、不起 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::audit::AuditSink;
    use crate::query::tokens::TokenTable;
    use crate::query::ServiceParts;
    use crate::sparql::exec::SessionFacts;
    use axum::body::Body;
    use tower::ServiceExt;

    const TOKEN_A: &str = "FAKE-TOKEN-aaaaaaaaaaaaaaaaaaaa";
    const TOKEN_B: &str = "FAKE-TOKEN-bbbbbbbbbbbbbbbbbbbb";

    /// 一个**不连库**的服务：懒连接池 + 夹具映射。
    ///
    /// 401 / 403 / 429 / health / catalog / describe 都在**执行 SQL 之前**结束，
    /// 所以这些判据不需要数据库；真正执行查询的判据在 `tests/test_req123_live.rs`。
    pub(crate) fn test_service() -> Arc<QueryService> {
        test_service_with_audit(AuditSink::stdout_only())
    }

    /// 同 [`test_service`]，但审计落到指定 sink（REQ-127 的审计判据读文件对账）。
    fn test_service_with_audit(audit: AuditSink) -> Arc<QueryService> {
        test_service_with_policy(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id"}},"publicDimensions":[]}"#,
            audit,
        )
    }

    /// 同 [`test_service`]，但租户声明由调用方给（REQ-135 用它造 `TENANT_UNRESOLVED`：
    /// 声明里**故意不登记** `orders`，查询 orders ⇒ 规划期 fail-closed）。
    fn test_service_with_policy(policy_json: &str, audit: AuditSink) -> Arc<QueryService> {
        let (schema, mapping, model) = crate::query::test_fixture();
        let policy = crate::sparql::policy::QueryPolicy::parse(policy_json).expect("声明");
        // IRI 基要先算出来（`opts` 借了 `schema`，之后 `schema` 要 move 进服务）。
        let ontology_iri = {
            let mut opts = crate::domain::ttl::TtlOptions::new("demo", 2);
            opts.schema = Some(&schema);
            opts.ontology_iri()
        };
        let tokens = TokenTable::parse(&format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"label":"a","token":"{TOKEN_A}","tenant":"C0001","max_rows":5,"max_concurrency":1}},
                {{"label":"b","token":"{TOKEN_B}","tenant":"C0002","max_rows":5,"max_concurrency":1}}
            ]}}"#
        ))
        .expect("token 表");
        let svc = Arc::new(QueryService::new(ServiceParts {
            mode: crate::query::ServiceMode::Query,
            source: Some(
                sqlx::postgres::PgPoolOptions::new()
                    // 夹具池**故意连不上**：判据要的是「鉴权/限额在连库之前就答完」，
                    // 短 acquire_timeout 让「确实要连库」的那条分支快速变成 500（而不是等 30 s）。
                    .acquire_timeout(std::time::Duration::from_millis(300))
                    .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
                    .expect("lazy pool"),
            ),
            facts: Some(SessionFacts {
                transaction_read_only: "on".into(),
                standard_conforming_strings: "on".into(),
            }),
            mapping: Some(mapping),
            model,
            schema,
            policy,
            ontology_iri,
            project: "demo".into(),
            version: 2,
            timeout_ms: 1000,
            sse_idle_timeout_ms: 1_000,
            sse_max_connections: 2,
            max_concurrent_queries: 2,
            tokens,
            audit,
            // ⭐ REQ-127 · 夹具两视图：走同一个 `pipeline::render`（不连库）。
            ttl: crate::query::ttl::fixture(),
        }));
        svc
    }

    /// ⭐ REQ-136 · **本体服务模式**的夹具：**没有源库、没有映射、没有 schema**。
    ///
    /// 与 [`test_service`] 的差别只有 `ServiceParts` 的三个 `None` + `mode` ——
    /// 所有端点走的是同一套 handler（这正是「两个模式只在过滤处分叉」的证据）。
    /// 判据里它还带一条硬约束：任何请求都**不许**尝试连库（这里连连接池都不存在）。
    pub(crate) fn test_service_ontology_only() -> Arc<QueryService> {
        test_service_ontology_only_with_audit(AuditSink::stdout_only())
    }

    /// ⭐ REQ-137 · 本体模式 + 可读审计（AC-005：审计行 `tenant=-`）。
    fn test_service_ontology_only_with_audit(audit: AuditSink) -> Arc<QueryService> {
        let (schema, _mapping, model) = crate::query::test_fixture();
        let policy = crate::sparql::policy::QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id"}},"publicDimensions":[]}"#,
        )
        .expect("声明");
        let ontology_iri = {
            let opts = crate::domain::ttl::TtlOptions::new("demo", 2);
            opts.ontology_iri()
        };
        // ⭐ REQ-137 · 本体模式的 token **不带 tenant**（带上会被 `parse_for_mode` 拒绝）。
        let tokens = TokenTable::parse_for_mode(
            &format!(
                r#"{{"version":"1.0.0","tokens":[
                {{"label":"a","token":"{TOKEN_A}","max_rows":5,"max_concurrency":1}}
            ]}}"#
            ),
            crate::query::ServiceMode::OntologyOnly,
        )
        .expect("token 表");
        let _ = schema;
        Arc::new(QueryService::new(ServiceParts {
            mode: crate::query::ServiceMode::OntologyOnly,
            source: None,
            facts: None,
            mapping: None,
            model,
            // 本体模式没有 schema（`SchemaTypes::default()` = 空表；⛔ 不读文件）。
            schema: crate::domain::ttl::SchemaTypes::default(),
            policy,
            ontology_iri,
            project: "demo".into(),
            version: 2,
            timeout_ms: 1000,
            sse_idle_timeout_ms: 1_000,
            sse_max_connections: 2,
            max_concurrent_queries: 2,
            tokens,
            audit,
            ttl: crate::query::ttl::fixture(),
        }))
    }

    async fn call(app: Router, req: axum::http::Request<Body>) -> (StatusCode, HeaderMap, String) {
        let res = app.oneshot(req).await.expect("oneshot");
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
            .await
            .expect("读 body");
        (status, headers, String::from_utf8_lossy(&bytes).to_string())
    }

    fn get(uri: &str, token: Option<&str>) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::builder().method("GET").uri(uri);
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    fn post(uri: &str, token: Option<&str>, body: &str) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::from(body.to_string())).unwrap()
    }

    /// AC-001 / R2：`/api/health` 免 token，且**不含业务数据**（租户 / 表名 / SQL / 连接串）。
    ///
    /// ⭐ REQ-139 · 项目名与**本体**版本现在**该**出现（`project` / `ontology_version`；
    /// 普通模式也有 —— R3），但下面这些仍然零出现（R2）。
    /// 变异：把整个 `QueryService` / 快照序列化进 health ⇒ 红。
    #[tokio::test]
    async fn health_needs_no_token_and_carries_no_business_data() {
        let app = build_router(test_service());
        let (status, headers, body) = call(app, get("/api/health", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["service"], "ontok-query");
        assert_eq!(v["syntax"], SYNTAX_VERSION);
        assert!(v["version"].as_str().is_some());
        // ⭐ REQ-139 · 这两个字段**必须**在（普通模式 —— R3）。
        assert_eq!(v["project"], "demo", "{v}");
        assert_eq!(v["ontology_version"], 2, "{v}");
        // ⛔ 零业务数据：租户 id、表名、类名、SQL、连接串、口令都不许出现。
        for forbidden in [
            "C0001",
            "customers",
            "orders",
            "sql",
            "postgres",
            "password",
        ] {
            assert!(
                !body.contains(forbidden),
                "health 里出现了 {forbidden:?}（R2 要求不泄数据）：{body}"
            );
        }
    }

    /// ⭐ REQ-139 · AC-003（R4）：health 的 `project` / `ontology_version` 与 catalog 的
    /// `project` / `version` **是同一个数** —— 两个模式各断言一次。
    ///
    /// 为什么这条不是空转：断言「两边相等」**并且**「等于服务的已知值」—— 只写前者时，
    /// 两处一起退化成 `""` / `0` 也会绿；只写后者时，health 自己另算一遍也看不出来。
    /// 变异（把 `ontology_version` 恒写 `0`）⇒ 相等断言立刻红（catalog 仍是 2）。
    #[tokio::test]
    async fn health_identity_equals_catalog_in_both_modes() {
        for (mode, svc) in [
            ("query", test_service()),
            ("ontology-only", test_service_ontology_only()),
        ] {
            let (status, _, health_body) =
                call(build_router(Arc::clone(&svc)), get("/api/health", None)).await;
            assert_eq!(status, StatusCode::OK, "{mode}");
            let h: Value = serde_json::from_str(&health_body).unwrap();

            let (status, _, catalog_body) = call(
                build_router(Arc::clone(&svc)),
                get("/api/catalog?page=1&page_size=1", Some(TOKEN_A)),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{mode}: {catalog_body}");
            let c: Value = serde_json::from_str(&catalog_body).unwrap();

            assert_eq!(
                h["project"], c["project"],
                "{mode}：health 的 project 必须与 catalog 的 project 同值\n\
                 health={h}\ncatalog={c}"
            );
            assert_eq!(
                h["ontology_version"], c["version"],
                "{mode}：health 的 ontology_version 必须与 catalog 的 version 同值\n\
                 health={h}\ncatalog={c}"
            );
            // 两边一起退化也过不了：这两个必须等于服务里那**一对**字段。
            assert_eq!(h["project"], svc.project.as_str(), "{mode}: {h}");
            assert_eq!(h["ontology_version"], svc.version, "{mode}: {h}");
            assert!(h["ontology_version"].as_i64().is_some_and(|n| n > 0), "{mode}: {h}");
        }
    }

    /// ⭐ REQ-139 · AC-004（R5）：与基线响应做**键集合**对比。
    ///
    /// 基线（`develop` `93e6af2`）的 `/api/health` 键集合就是下面这 5 个；
    /// REQ-139 只**新增** `project` / `ontology_version`，⛔ 不删不改任何一个旧字段，
    /// 且 `mode` 仍然**只**在本体模式出现（有客户端靠它在不在判断部署形状）。
    #[tokio::test]
    async fn health_keeps_all_baseline_keys_and_mode_stays_ontology_only() {
        const BASELINE: [&str; 5] = ["service", "version", "syntax", "protocols", "transports"];
        const ADDED: [&str; 2] = ["project", "ontology_version"];

        for (mode, svc, is_ontology) in [
            ("query", test_service(), false),
            ("ontology-only", test_service_ontology_only(), true),
        ] {
            let (_, _, body) = call(build_router(svc), get("/api/health", None)).await;
            let v: Value = serde_json::from_str(&body).unwrap();
            let obj = v.as_object().expect("health 必须是 JSON 对象");

            let mut want: Vec<&str> = BASELINE.iter().chain(ADDED.iter()).copied().collect();
            if is_ontology {
                want.push("mode");
            }
            let mut got: Vec<&str> = obj.keys().map(String::as_str).collect();
            got.sort_unstable();
            want.sort_unstable();
            assert_eq!(got, want, "{mode}：health 的键集合变了\n{body}");

            // 键在、值被改，同样是破坏 ⇒ 旧字段的值也照样钉住。
            assert_eq!(v["service"], "ontok-query", "{mode}");
            assert_eq!(v["version"], env!("CARGO_PKG_VERSION"), "{mode}");
            assert_eq!(v["syntax"], SYNTAX_VERSION, "{mode}");
            assert_eq!(v["protocols"], json!(["rest", "mcp-http"]), "{mode}");
            assert_eq!(v["transports"], json!(["http", "sse"]), "{mode}");
            if is_ontology {
                assert_eq!(v["mode"], "ontology-only", "{mode}");
            }
        }
    }

    /// AC-003：无 token ⇒ 401（且**不透露**任何信息）；错 token ⇒ 401。
    #[tokio::test]
    async fn missing_or_wrong_token_is_401_everywhere_except_health() {
        let svc = test_service();
        for uri in ["/api/catalog", "/api/describe?name=:customers", "/mcp"] {
            let app = build_router(Arc::clone(&svc));
            let (status, _, body) = call(app, get(uri, None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
            let v: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["code"], "UNAUTHORIZED");
            assert!(!v["hint"].as_str().unwrap_or("").is_empty());
            assert!(!body.contains(TOKEN_A), "401 正文不许回显任何 token：{body}");
        }
        let app = build_router(Arc::clone(&svc));
        let (status, _, _) = call(app, get("/api/catalog", Some("FAKE-TOKEN-zzzzzzzzzzzzzzzzzzzz"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // POST /api/sparql 同样要 token
        let app = build_router(Arc::clone(&svc));
        let (status, _, _) = call(app, post("/api/sparql", None, r#"{"query":"SELECT ?s WHERE { ?s a :customers }"}"#)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    /// AC-003：token 放在 **query string** 里 ⇒ 401（只认头）。
    #[tokio::test]
    async fn a_token_in_the_query_string_does_not_work() {
        let app = build_router(test_service());
        let uri = format!("/api/catalog?token={TOKEN_A}&access_token={TOKEN_A}");
        let (status, _, body) = call(app, get(&uri, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "query string 里的 token 不许生效：{body}");

        // 带着**正确的头**但 query string 里也有 token ⇒ 头说了算；query 里的那个是
        // 不认识的参数 ⇒ 400（可行动），而不是被悄悄采用。
        let uri = format!("/api/catalog?token={TOKEN_A}");
        let (status, _, body) = call(build_router(test_service()), get(&uri, Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert!(v["hint"].as_str().unwrap().contains("不生效"), "{v}");
    }

    /// ⭐ AC-003 的核心：请求体带**别人的** tenant ⇒ 403 `TENANT_MISMATCH`。
    #[tokio::test]
    async fn a_body_tenant_that_disagrees_with_the_token_is_403() {
        let app = build_router(test_service());
        let body = r#"{"query":"SELECT ?s WHERE { ?s a :customers }","tenant":"C0002"}"#;
        let (status, _, text) = call(app, post("/api/sparql", Some(TOKEN_A), body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["code"], "TENANT_MISMATCH");
        assert!(v["hint"].as_str().unwrap().contains("租户由凭证决定"), "{v}");

        // 一致 ⇒ 不被这条拦住（会走到连库那一步，本夹具没有库 ⇒ 500；
        // 判据只看「不是 403」）。
        let body = r#"{"query":"SELECT ?s WHERE { ?s a :customers }","tenant":"C0001"}"#;
        let (status, _, text) = call(build_router(test_service()), post("/api/sparql", Some(TOKEN_A), body)).await;
        assert_ne!(status, StatusCode::FORBIDDEN, "{text}");
    }

    /// 同一个租户检查在 **MCP 的 tools/call** 上也成立（AC-004 的安全侧）。
    #[tokio::test]
    async fn mcp_sparql_query_also_rejects_a_foreign_tenant() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"sparql_query","arguments":{"query":"SELECT ?s WHERE { ?s a :customers }","tenant":"C0002"}}}"#;
        let (status, _, body) = call(build_router(test_service()), post("/mcp", Some(TOKEN_A), line)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        let result = &v["result"];
        assert_eq!(result["isError"], true, "{v}");
        assert_eq!(result["structuredContent"]["code"], "TENANT_MISMATCH");
    }

    /// AC-002：`initialize` / `tools/list` 可用；工具清单含 `sparql_query` 与另三个。
    #[tokio::test]
    async fn mcp_initialize_and_tools_list_are_standard() {
        let app = build_router(test_service());
        let line = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#;
        let (_, _, body) = call(app, post("/mcp", Some(TOKEN_A), line)).await;
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        assert!(v["result"]["capabilities"]["tools"].is_object());
        // ⭐ REQ-127：resources 能力**真的实现了**才声明（list/read 在下面的判据里真调）。
        assert!(
            v["result"]["capabilities"]["resources"].is_object(),
            "initialize 必须声明 resources（否则客户端不会发 resources/list）：{v}"
        );
        assert!(v["result"]["instructions"].as_str().unwrap().contains("sparql_query"));
        assert!(
            v["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("ontok://ontology/model.ttl"),
            "instructions 要告诉 Agent 整份本体在哪：{v}"
        );

        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let (_, _, body) = call(build_router(test_service()), post("/mcp", Some(TOKEN_A), line)).await;
        let v: Value = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, TOOL_NAMES);
        assert!(names.contains(&"sparql_query"));
        // 每个工具都要有 inputSchema（MCP 规范要求）
        for t in v["result"]["tools"].as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        }
        // B1：Agent 只看 `tools/list` 的 description ⇒ `join_states` 的范围必须写在这里
        // （⛔ 只列字段名不够；改坏措辞即红）。
        let q_desc = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sparql_query")
            .expect("sparql_query 工具")["description"]
            .as_str()
            .expect("description 是字符串")
            .to_string();
        assert!(
            q_desc.contains("all_edges_between_involved_tables"),
            "MCP description 必须写明 join_states 的范围值：{q_desc}"
        );
        assert!(
            q_desc.contains("不是本次查询实际走过的那几条"),
            "MCP description 必须明说「不是本次查询走到的边」：{q_desc}"
        );
        assert!(
            q_desc.contains("join_states_scope_note"),
            "MCP description 必须提到人话说明字段：{q_desc}"
        );
    }

    /// 通知不许回响应（HTTP 侧是空 202）。
    #[tokio::test]
    async fn mcp_notifications_get_no_body() {
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let (status, _, body) = call(build_router(test_service()), post("/mcp", Some(TOKEN_A), line)).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert!(body.is_empty(), "{body}");
    }

    /// AC-001：catalog 分页 + 关系证据等级 + 枚举值。
    #[tokio::test]
    async fn catalog_pages_through_classes_properties_and_relations() {
        let app = build_router(test_service());
        let (status, _, body) = call(app, get("/api/catalog?page=1&page_size=200", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["counts"]["classes"], 2);
        assert_eq!(v["counts"]["relations"], 1);
        assert!(v["total_items"].as_u64().unwrap() >= 5);
        let items = v["items"].as_array().unwrap();
        // 关系条目必须带证据等级
        let rel = items
            .iter()
            .find(|i| i["kind"] == "relation")
            .expect("目录里必须有关系");
        assert_eq!(rel["evidence_level"], "true");
        assert!(rel["evidence"].as_str().unwrap().contains("value-overlap"));
        // 枚举值来自注释里明写的那一条
        let st = items
            .iter()
            .find(|i| i["name"] == "customers__status")
            .expect("customers__status 属性");
        assert_eq!(st["enum_values"], "0=停用;1=正常");

        // 分页：page_size=2 时 has_more=true，第三页开始为空但不报错
        let (_, _, body) = call(build_router(test_service()), get("/api/catalog?page=1&page_size=2", Some(TOKEN_A))).await;
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        assert_eq!(v["has_more"], true);
        let total = v["total_pages"].as_u64().unwrap();
        let (_, _, body) = call(
            build_router(test_service()),
            get(&format!("/api/catalog?page={}&page_size=2", total + 1), Some(TOKEN_A)),
        )
        .await;
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 0);
        assert_eq!(v["has_more"], false);
    }

    /// AC-001：describe 未知名字 ⇒ 404 + 相近项；已知名字 ⇒ 200。
    #[tokio::test]
    async fn describe_returns_404_with_suggestions_for_unknown_names() {
        let app = build_router(test_service());
        let (status, _, body) = call(app, get("/api/describe?name=:customerz", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["code"], "UNKNOWN_NAME");
        let s = v["suggestions"].as_array().unwrap();
        assert!(
            s.iter().any(|x| x.as_str().unwrap().starts_with("customers")),
            "相近项里必须有 customers：{v}"
        );
        assert!(v["hint"].as_str().unwrap().contains("catalog"), "{v}");

        let (status, _, body) = call(build_router(test_service()), get("/api/describe?name=:orders__amount", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["kind"], "property");
        assert_eq!(v["class"], "orders");
        assert_eq!(v["datatype"], "xsd:decimal");

        // 类详情里有租户口径
        let (status, _, body) = call(build_router(test_service()), get("/api/describe?name=:orders", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["kind"], "class");
        assert_eq!(v["tenant"]["mode"], "filter");
        assert_eq!(v["tenant"]["column"], "customer_id");
    }

    /// AC-005：同一 token 并发上限 = 1 ⇒ 第二个请求 429（在**执行之前**就拒绝）。
    #[tokio::test]
    async fn a_second_concurrent_query_is_429() {
        let svc = test_service();
        // 占掉唯一的名额（模拟一条正在跑的查询）
        let slot = svc.occupy_query_slot(TOKEN_A);
        let body = r#"{"query":"SELECT ?s WHERE { ?s a :customers }"}"#;
        let (status, _, text) = call(build_router(Arc::clone(&svc)), post("/api/sparql", Some(TOKEN_A), body)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["code"], "TOO_MANY_CONCURRENT");
        drop(slot);
        // 归还后不再是 429（会走到连库那步 ⇒ 500，但不是 429）
        let (status, _, _) = call(build_router(Arc::clone(&svc)), post("/api/sparql", Some(TOKEN_A), body)).await;
        assert_ne!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    /// AC-005：SSE 连接数超限 ⇒ 429（同一 token 上限 = 1）。
    #[tokio::test]
    async fn an_extra_sse_stream_is_429() {
        let svc = test_service();
        let slot = svc.occupy_sse_slot(TOKEN_A);
        let (status, _, text) = call(build_router(Arc::clone(&svc)), get("/mcp", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["code"], "TOO_MANY_STREAMS");
        drop(slot);
    }

    /// GET `/mcp` 的响应必须是 `text/event-stream`（AC-002 的传输判据）。
    #[tokio::test]
    async fn get_mcp_returns_an_event_stream() {
        let res = build_router(test_service())
            .oneshot(get("/mcp", Some(TOKEN_A)))
            .await
            .expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.starts_with("text/event-stream"), "content-type = {ct}");
        // 不要读 body：那是长连接。丢掉即可（任务会在空闲超时后收尾）。
        drop(res);
    }

    /// 请求体解析：缺字段 / 多了不认识的字段 ⇒ 400 且 hint 可行动。
    #[tokio::test]
    async fn bad_bodies_are_400_with_actionable_hints() {
        let app = build_router(test_service());
        let (status, _, body) = call(app, post("/api/sparql", Some(TOKEN_A), "")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["code"], "BAD_REQUEST");
        assert!(v["hint"].as_str().unwrap().contains("query"), "{v}");

        let app = build_router(test_service());
        let (status, _, body) = call(
            app,
            post("/api/sparql", Some(TOKEN_A), r#"{"query":"SELECT ?s WHERE { ?s a :customers }","max_row":1}"#),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert!(v["message"].as_str().unwrap().contains("max_row"), "{v}");
        assert!(v["hint"].as_str().unwrap().contains("max_rows"), "{v}");

        // catalog 的不认识参数
        let (status, _, body) = call(build_router(test_service()), get("/api/catalog?pageSize=2", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert!(v["hint"].as_str().unwrap().contains("page_size"), "{v}");
    }

    /// 未知路径：**没 token** ⇒ 401（探测不到路径），**有 token** ⇒ 404（说真话）。
    #[tokio::test]
    async fn unknown_paths_are_401_without_a_token_and_404_with_one() {
        let (status, _, _) = call(build_router(test_service()), get("/api/raw", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _, body) = call(build_router(test_service()), get("/api/raw", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["code"], "NOT_FOUND");
    }

    // ══════════════════════════════════════════════════════════
    // REQ-127 · 整份 OWL（TTL）：两视图 / ETag / 审计 / MCP resource
    // ══════════════════════════════════════════════════════════

    /// AC-003：默认**客户视图**（8 个建模关键词 0 命中）；`?view=model` 才有。
    #[tokio::test]
    async fn ttl_defaults_to_customer_and_model_is_explicit() {
        let (status, headers, body) =
            call(build_router(test_service()), get(TTL_PATH, Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], TTL_CONTENT_TYPE);
        assert!(headers.contains_key(header::ETAG), "200 必须带 ETag");
        for needle in [
            "ontok:tenantKey",
            "ontok:tenantColumn",
            "ontok:tenantUnannotated",
            "ontok:joinEvidence",
            "ontok:joinVerified",
            "ontok:primaryKeyColumn",
            "ontok:isPrimaryKey",
            "ontok:allowedValue",
        ] {
            assert_eq!(
                body.matches(needle).count(),
                0,
                "客户视图不该出现 {needle}：{body}"
            );
        }

        let (status, _, model) = call(
            build_router(test_service()),
            get("/api/ontology.ttl?view=model", Some(TOKEN_A)),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        for needle in [
            "ontok:joinVerified",
            "ontok:primaryKeyColumn",
            "ontok:allowedValue",
        ] {
            assert!(
                model.matches(needle).count() > 0,
                "建模视图必须有 {needle}：{model}"
            );
        }
        assert_ne!(body, model, "两视图必须是两份不同的字节");
        assert!(model.len() > body.len(), "建模视图应比客户视图大");

        // 显式 `view=customer` 与默认逐字节相同（默认不是「另一个隐藏视图」）。
        let (_, _, explicit) = call(
            build_router(test_service()),
            get("/api/ontology.ttl?view=customer", Some(TOKEN_A)),
        )
        .await;
        assert_eq!(body, explicit);
    }

    /// AC-002：只认 query token —— 无 token / 错 token / **admin token** 全 401。
    ///
    /// `TokenTable` 只有 query token 一种登记；admin token（另一份表里的字符串）
    /// 在这里就是「不认识的 token」⇒ 401。两套凭证不通用是这个结构的必然结果。
    #[tokio::test]
    async fn ttl_accepts_only_the_query_token() {
        const ADMIN_TOKEN: &str = "ADMIN-TOKEN-cccccccccccccccccccc";
        for uri in [TTL_PATH, "/api/ontology.ttl?view=model"] {
            let (status, _, body) = call(build_router(test_service()), get(uri, None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} 无 token");
            let v: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["code"], "UNAUTHORIZED");
            assert!(!body.contains(TOKEN_A));

            let (status, _, _) =
                call(build_router(test_service()), get(uri, Some(ADMIN_TOKEN))).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} admin token");

            let (status, _, _) = call(build_router(test_service()), get(uri, Some(TOKEN_A))).await;
            assert_eq!(status, StatusCode::OK, "{uri} query token");
        }
    }

    /// 不认识的 `view` / 参数 ⇒ 400（⛔ 不静默回退成客户视图）。
    #[tokio::test]
    async fn ttl_rejects_unknown_views_and_parameters() {
        for uri in [
            "/api/ontology.ttl?view=modle",
            "/api/ontology.ttl?view=",
            "/api/ontology.ttl?view=model&schema=code/out/schema.json",
            "/api/ontology.ttl?token=x",
        ] {
            let (status, _, body) =
                call(build_router(test_service()), get(uri, Some(TOKEN_A))).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
            let v: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(v["code"], "BAD_REQUEST");
            assert!(!v["hint"].as_str().unwrap_or("").is_empty());
        }
    }

    /// AC-005：ETag = 正文 sha256 前 16 位；`If-None-Match` 命中 ⇒ 304 且**无体**。
    #[tokio::test]
    async fn ttl_etag_and_if_none_match() {
        use crate::query::audit::sha256_hex;

        let (_, headers, body) =
            call(build_router(test_service()), get(TTL_PATH, Some(TOKEN_A))).await;
        let etag = headers[header::ETAG].to_str().expect("ETag 是 ASCII").to_string();
        assert_eq!(
            etag,
            format!("\"{}\"", &sha256_hex(body.as_bytes())[..16]),
            "ETag 必须是内容 sha256 前 16 位（带引号）"
        );

        // 命中的 If-None-Match（带引号 / 不带引号都要认）。
        for presented in [etag.clone(), etag.trim_matches('"').to_string()] {
            let req = axum::http::Request::builder()
                .method("GET")
                .uri(TTL_PATH)
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN_A}"))
                .header(header::IF_NONE_MATCH, presented)
                .body(Body::empty())
                .unwrap();
            let res = build_router(test_service())
                .oneshot(req)
                .await
                .expect("oneshot");
            assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
            assert_eq!(
                res.headers()[header::ETAG].to_str().unwrap(),
                etag.as_str()
            );
            let len = res
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("0");
            assert_eq!(len, "0", "304 的 Content-Length 必须是 0（或无）");
            let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
            assert!(bytes.is_empty(), "304 不许带体");
        }

        // 不命中 ⇒ 200（不是 304）。
        let req = axum::http::Request::builder()
            .method("GET")
            .uri(TTL_PATH)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN_A}"))
            .header(header::IF_NONE_MATCH, "\"0000000000000000\"")
            .body(Body::empty())
            .unwrap();
        let res = build_router(test_service()).oneshot(req).await.expect("oneshot");
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// AC-006：每次请求一行审计 —— 成功（token 摘要 / view / bytes / code）与 401 都有。
    #[tokio::test]
    async fn ttl_audit_has_view_and_bytes_for_success_and_401() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req127-audit-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("audit.log");
        let svc = test_service_with_audit(AuditSink::open(&path).expect("审计文件"));
        let app = || build_router(Arc::clone(&svc));

        let (status, headers, body) = call(app(), get(TTL_PATH, Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::OK);
        // 304 也记（bytes=0）；再用真实摘要去命中它。
        let etag = headers[header::ETAG].to_str().unwrap().to_string();
        let req = axum::http::Request::builder()
            .method("GET")
            .uri(TTL_PATH)
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN_A}"))
            .header(header::IF_NONE_MATCH, etag)
            .body(Body::empty())
            .unwrap();
        let res = app().oneshot(req).await.expect("oneshot");
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
        // 无 token ⇒ 401 也记一行。
        let (status, _, _) = call(app(), get("/api/ontology.ttl?view=model", None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let text = std::fs::read_to_string(&path).expect("读审计");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "三次请求三行：\n{text}");
        assert!(
            lines[0].contains("event=ttl")
                && lines[0].contains("token=sha256:")
                && lines[0].contains("view=customer")
                && lines[0].contains(&format!("bytes={}", body.len()))
                && lines[0].contains("code=OK"),
            "成功行字段不全：{}",
            lines[0]
        );
        assert!(
            lines[1].contains("code=NOT_MODIFIED") && lines[1].contains("bytes=0"),
            "304 行：{}",
            lines[1]
        );
        assert!(
            lines[2].contains("event=auth")
                && lines[2].contains("token=-")
                && lines[2].contains("view=model")
                && lines[2].contains("code=UNAUTHORIZED"),
            "401 行：{}",
            lines[2]
        );
        assert!(!text.contains(TOKEN_A), "⛔ 审计里出现了 token 明文：{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AC-007：MCP 选 **resource** —— `resources/list` 列出两条 URI，
    /// `resources/read` 的正文与 HTTP 同视图**逐字节相同**（sha256 相等）。
    #[tokio::test]
    async fn mcp_resource_reads_the_same_ttl_bytes_as_http() {
        use crate::query::audit::sha256_hex;

        let (status, _, body) = call(
            build_router(test_service()),
            post("/mcp", Some(TOKEN_A), r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_str(&body).unwrap();
        let uris: Vec<&str> = v["result"]["resources"]
            .as_array()
            .expect("resources[]")
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        assert_eq!(uris, vec![RESOURCE_CUSTOMER, RESOURCE_MODEL]);

        for (uri, http_uri) in [
            (RESOURCE_CUSTOMER, TTL_PATH),
            (RESOURCE_MODEL, "/api/ontology.ttl?view=model"),
        ] {
            let line = format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{{"uri":"{uri}"}}}}"#
            );
            let (_, _, rpc) = call(build_router(test_service()), post("/mcp", Some(TOKEN_A), &line)).await;
            let v: Value = serde_json::from_str(&rpc).unwrap();
            let text = v["result"]["contents"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{uri} 没有 text：{v}"));
            assert_eq!(v["result"]["contents"][0]["mimeType"], TTL_CONTENT_TYPE);

            let (_, _, http_body) = call(build_router(test_service()), get(http_uri, Some(TOKEN_A))).await;
            assert_eq!(text, http_body, "{uri} 与 {http_uri} 必须逐字节相同");
            assert_eq!(
                sha256_hex(text.as_bytes()),
                sha256_hex(http_body.as_bytes()),
                "{uri} 的 sha256 必须与 HTTP 相同"
            );
        }
    }

    /// `resources/read` 的不认识 URI ⇒ 协议错误（带可行动的 hint），⛔ 不返回空内容。
    #[tokio::test]
    async fn mcp_resources_read_unknown_uri_is_an_error() {
        let line = r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"ontok://ontology/evil.ttl"}}"#;
        let (_, _, body) = call(build_router(test_service()), post("/mcp", Some(TOKEN_A), line)).await;
        let v: Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].is_object(), "{v}");
        assert_eq!(v["error"]["data"]["code"], "RESOURCE_NOT_FOUND");
        assert!(v["error"]["data"]["hint"].as_str().unwrap().contains(RESOURCE_MODEL));
    }

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-135 · `/metrics`
    // ══════════════════════════════════════════════════════════

    /// ⭐ AC-001 / AC-002：`/metrics` **免 token**、`Content-Type` 逐字正确、
    /// 六个指标都有 `# HELP` / `# TYPE`，且整段文本通过格式校验器（不依赖 promtool）。
    #[tokio::test]
    async fn metrics_is_anonymous_and_is_valid_prometheus_text() {
        let app = build_router(test_service());
        let (status, headers, body) = call(app, get("/metrics", None)).await;
        assert_eq!(status, StatusCode::OK, "AC-001：/metrics 免 token");
        assert_eq!(
            headers[header::CONTENT_TYPE],
            crate::metrics::CONTENT_TYPE,
            "AC-001：Content-Type 必须是 text/plain; version=0.0.4"
        );
        for name in [
            "ontok_requests_total",
            "ontok_request_duration_seconds",
            "ontok_rows_returned_total",
            "ontok_tenant_denied_total",
            "ontok_ttl_bytes",
            "ontok_mapping_joins",
        ] {
            assert!(
                body.contains(&format!("# HELP {name} ")),
                "缺 # HELP {name}：\n{body}"
            );
            assert!(
                body.contains(&format!("# TYPE {name} ")),
                "缺 # TYPE {name}：\n{body}"
            );
        }
        if let Err(errs) = crate::metrics::validate_exposition(&body) {
            panic!(
                "AC-002：/metrics 的文本不合法：\n{}",
                errs.join("\n") + "\n\n----- 原文 -----\n" + &body
            );
        }
        // ⛔ AC-003：零业务数据（真实租户 / 表名 / SQL 一个都不许出现）。
        for forbidden in ["C0001", "C0002", "customers", "orders", "deductions", "SELECT"] {
            assert!(
                !body.contains(forbidden),
                "/metrics 里出现了 {forbidden:?}：\n{body}"
            );
        }
    }

    /// ⭐ AC-004 · `ontok_requests_total`：连发 3 次 `/api/catalog` ⇒ 恰好 +3；
    /// 同一次抓取还把耗时直方图的 `_count` 一起推上去。
    #[tokio::test]
    async fn three_requests_are_counted_exactly_once_each() {
        let svc = test_service();
        for _ in 0..3 {
            let app = build_router(Arc::clone(&svc));
            let (status, _, _) = call(app, get("/api/catalog", Some(TOKEN_A))).await;
            assert_eq!(status, StatusCode::OK);
        }
        let (_, _, body) = call(build_router(Arc::clone(&svc)), get("/metrics", None)).await;
        assert!(
            body.contains(
                "ontok_requests_total{service=\"ontok-query\",endpoint=\"/api/catalog\",code=\"200\"} 3"
            ),
            "3 次 catalog 必须恰好记 3：\n{body}"
        );
        assert!(
            body.contains(
                "ontok_request_duration_seconds_count{service=\"ontok-query\",endpoint=\"/api/catalog\"} 3"
            ),
            "直方图 _count 必须跟着涨：\n{body}"
        );
    }

    /// ⭐ AC-004 的核心：**真造一次拒绝**（请求体带别人的租户）⇒ `tenant_denied` +1。
    ///
    /// 同一条判据在 MCP 的 `tools/call` 上也成立（两个协议共用 `run_sparql`）。
    /// 变异：把 `svc.metrics.note_denial_code(e.code)` 删掉 / 改成恒 0 ⇒ 本测试红。
    #[tokio::test]
    async fn a_foreign_tenant_bumps_the_security_counter() {
        let svc = test_service();
        let metric = |body: &str| -> String {
            body.lines()
                .find(|l| {
                    l.starts_with(
                        "ontok_tenant_denied_total{service=\"ontok-query\",reason=\"TENANT_MISMATCH\"}",
                    )
                })
                .unwrap_or_else(|| panic!("缺 tenant_denied 序列：\n{body}"))
                .to_string()
        };
        let (_, _, before) = call(build_router(Arc::clone(&svc)), get("/metrics", None)).await;
        assert!(metric(&before).ends_with(" 0"), "起始必须是 0：{}", metric(&before));

        // ① REST：请求体里的 tenant 与凭据不一致。
        let body = r#"{"query":"SELECT ?s WHERE { ?s a :customers }","tenant":"C0002"}"#;
        let (status, _, text) =
            call(build_router(Arc::clone(&svc)), post("/api/sparql", Some(TOKEN_A), body)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{text}");

        // ② MCP：同一个越权尝试走另一个协议。
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"sparql_query","arguments":{"query":"SELECT ?s WHERE { ?s a :customers }","tenant":"C0002"}}}"#;
        let (status, _, text) =
            call(build_router(Arc::clone(&svc)), post("/mcp", Some(TOKEN_A), line)).await;
        assert_eq!(status, StatusCode::OK, "{text}");

        let (_, _, after) = call(build_router(Arc::clone(&svc)), get("/metrics", None)).await;
        assert!(
            metric(&after).ends_with(" 2"),
            "两次越权尝试必须记 2（REST + MCP）：{}",
            metric(&after)
        );
        // 别的 reason 不受影响（标签没串）。
        assert!(
            after.contains(
                "ontok_tenant_denied_total{service=\"ontok-query\",reason=\"TENANT_UNRESOLVED\"} 0"
            ),
            "{after}"
        );
    }

    /// ⭐ AC-004 · 第三种越权信号：**表没登记**（`TENANT_UNRESOLVED`，规划期 fail-closed）。
    ///
    /// 声明里故意只留 `customers`，查询 `orders` ⇒ 规划器在**连库之前**就拒绝；
    /// 这同时证明「不需要数据库也能造出这条拒绝」。
    /// 变异：把 `run_sparql_event` 错误分支里的 `note_denial_code` 删掉 ⇒ 本测试红。
    #[tokio::test]
    async fn an_unregistered_table_bumps_the_unresolved_counter() {
        let svc = test_service_with_policy(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{},"publicDimensions":[]}"#,
            AuditSink::stdout_only(),
        );
        let body = r#"{"query":"SELECT ?o ?amt WHERE { ?o :orders__amount ?amt }"}"#;
        let (status, _, text) =
            call(build_router(Arc::clone(&svc)), post("/api/sparql", Some(TOKEN_A), body)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["code"], "TENANT_UNRESOLVED", "{v}");

        let (_, _, metrics) = call(build_router(Arc::clone(&svc)), get("/metrics", None)).await;
        assert!(
            metrics.contains(
                "ontok_tenant_denied_total{service=\"ontok-query\",reason=\"TENANT_UNRESOLVED\"} 1"
            ),
            "表没登记也必须计一次拒绝：\n{metrics}"
        );
        // 另一条 reason 没被连带（标签没串）。
        assert!(
            metrics.contains(
                "ontok_tenant_denied_total{service=\"ontok-query\",reason=\"TENANT_MISMATCH\"} 0"
            ),
            "{metrics}"
        );
    }

    /// ⭐ AC-004 · `rows_returned_total` / `ttl_bytes` / `mapping_joins` 三条的取值来源。
    ///
    /// 行数在真库实测里由 `run_sparql` 的成功分支 +N（本夹具没有库）；
    /// 这里断言的是**同一个计数器**：`Metrics::rows(n)` 加多少、`/metrics` 就是多少。
    /// TTL 与映射规模是启动即定的事实，直接与服务的字段对账。
    #[tokio::test]
    async fn the_gauges_and_the_rows_counter_report_real_values() {
        let svc = test_service();
        svc.metrics.rows(5);
        let (_, _, body) = call(build_router(Arc::clone(&svc)), get("/metrics", None)).await;

        assert!(
            body.contains("ontok_rows_returned_total{service=\"ontok-query\"} 5"),
            "{body}"
        );
        assert!(
            body.contains(&format!(
                "ontok_ttl_bytes{{view=\"customer\"}} {}",
                svc.ttl.customer().bytes()
            )),
            "TTL 客户视图字节数必须与缓存正文一致：\n{body}"
        );
        assert!(
            body.contains(&format!(
                "ontok_ttl_bytes{{view=\"model\"}} {}",
                svc.ttl.model().bytes()
            )),
            "{body}"
        );
        assert!(
            body.contains(&format!(
                "ontok_mapping_joins {}",
                svc.mapping.as_ref().expect("夹具映射").join_count()
            )),
            "映射 join 数必须与 `R2rmlMapping::join_count()` 一致：\n{body}"
        );
        assert!(
            svc.mapping.as_ref().expect("夹具映射").join_count() > 0,
            "夹具映射必须真的有 join（否则这条判据是空转）"
        );
    }

    // ⭐ REQ-136 · 本体服务模式（`--ontology-only`）
    // ══════════════════════════════════════════════════════════

    /// ⭐ AC-001：health 200 且带 `mode:"ontology-only"`；普通模式**没有** `mode` 键。
    ///
    /// ⭐ REQ-139 · 两种模式现在都多了 `project` / `ontology_version`（R3），所以
    /// 「普通模式逐字节不变」这句只对 **`mode`** 成立：探针仍然靠「有没有 `mode`」判部署形状。
    /// 变异：把 `mode` 无条件写进 health ⇒ 第二条断言红（普通模式的行为变了，AC-006）。
    #[tokio::test]
    async fn ontology_mode_health_says_so_and_query_mode_has_no_mode_key() {
        let (status, _, body) = call(
            build_router(test_service_ontology_only()),
            get("/api/health", None),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["mode"], "ontology-only", "{v}");
        assert_eq!(v["service"], "ontok-query");

        let (_, _, normal) = call(build_router(test_service()), get("/api/health", None)).await;
        assert!(
            !normal.contains("\"mode\""),
            "普通模式的 health 不许出现 mode 字段（AC-006）：{normal}"
        );
    }

    /// ⭐ AC-002：本体模式的 catalog **必须列关系**，即使那条边没有 join 列。
    ///
    /// 夹具里 `orders -> customers` 是有 join 的边；这里的关键断言是
    /// **`counts.relations` 不为 0、且 `counts` 里没有 `excluded_relations` 这个键**。
    /// 变异：让 catalog 走映射对账（回到现状）⇒ 在「无 schema」下连映射都建不出来，
    /// 这条测试的 `test_service_ontology_only` 就会在构建期 panic。
    #[tokio::test]
    async fn ontology_mode_catalog_lists_relations_and_has_no_excluded_count() {
        let (status, _, body) = call(
            build_router(test_service_ontology_only()),
            get("/api/catalog?page=1&page_size=200", Some(TOKEN_A)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["counts"]["classes"], 2, "{v}");
        assert_eq!(v["counts"]["relations"], 1, "关系不许被吃掉：{v}");
        assert_eq!(v["counts"]["properties"], 4, "属性 = 快照属性（⛔ 不补主键）：{v}");
        assert!(
            v["counts"].get("excluded_relations").is_none(),
            "本体模式的 counts 不许有 excluded_relations：{v}"
        );

        // 类条目带 display_name（普通模式没有这个字段）。
        let class = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == "class")
            .expect("必须有类");
        assert!(
            class["display_name"].as_str().is_some_and(|d| !d.is_empty()),
            "类必须带非空 display_name：{class}"
        );
        assert_eq!(class["primary_key"], "", "没有 schema ⇒ 没有主键，⛔ 不拿 id 顶替");

        // 关系条目：有名字、有 from/to、有证据等级（true/inherit/false/unverified 之一）。
        let rel = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == "relation")
            .expect("目录里必须有关系（本需求存在的理由）");
        assert_eq!(rel["from"], "orders");
        assert_eq!(rel["to"], "customers");
        assert!(
            ["true", "inherit", "false", "unverified"].contains(&rel["evidence_level"].as_str().unwrap()),
            "关系必须带 joinVerified 等级：{rel}"
        );
        assert_eq!(rel["evidence_level"], "true", "夹具这条边是实测通过的真边：{rel}");
    }

    /// ⭐ AC-003：本体模式的 describe 给 `display_name` / `key` / 正确的属性数。
    ///
    /// 变异：把 `display_name` 恒 `None` ⇒ 第一条断言红（那正是「名字为空」的现状）。
    #[tokio::test]
    async fn ontology_mode_describe_has_a_display_name_and_a_key() {
        let (status, _, body) = call(
            build_router(test_service_ontology_only()),
            get("/api/describe?name=:customers", Some(TOKEN_A)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["kind"], "class");
        assert_eq!(v["name"], "customers");
        assert_eq!(v["display_name"], "customers", "{v}");
        assert_eq!(v["key"], "customers", "{v}");
        // 快照里 customers 只有两列（customer_name / status）——没有 schema 就没有 id 列。
        assert_eq!(
            v["properties"].as_array().unwrap().len(),
            2,
            "属性数必须等于快照里的列数（⛔ 不造主键）：{v}"
        );
        assert!(
            !v["properties"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["column"] == "id"),
            "⛔ 不许凭空造 id 列：{v}"
        );
        assert_eq!(v["tenant"]["mode"], "not_applicable", "{v}");
        assert_eq!(v["relations"].as_array().unwrap().len(), 1, "{v}");

        // 属性详情也带业务名（`客户名`）。
        let (status, _, body) = call(
            build_router(test_service_ontology_only()),
            get("/api/describe?name=:customers__customer_name", Some(TOKEN_A)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["kind"], "property");
        assert_eq!(v["display_name"], "客户名", "{v}");
        assert_eq!(v["column"], "customer_name", "{v}");
    }

    /// ⭐ AC-004：REST 503 `NO_DATA_SOURCE`；MCP 同一个 `code`/`hint`（HTTP 200 + isError）。
    ///
    /// 这是「本部署没有数据源」与「查了没有」的分界：⛔ 不是 200 空结果、⛔ 不是 500。
    /// 变异：让 SPARQL 在本体模式直接跑（或返回空回执）⇒ 状态码与 code 双双红。
    #[tokio::test]
    async fn ontology_mode_sparql_is_503_no_data_source_in_both_protocols() {
        let body = r#"{"query":"SELECT ?s WHERE { ?s a :customers }"}"#;
        let (status, _, text) = call(
            build_router(test_service_ontology_only()),
            post("/api/sparql", Some(TOKEN_A), body),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{text}");
        let rest: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(rest["code"], "NO_DATA_SOURCE", "{rest}");
        let hint = rest["hint"].as_str().expect("hint 必须非空");
        for needle in ["本体", "没有接业务库", "重试无效"] {
            assert!(hint.contains(needle), "hint 缺 {needle:?}：{hint}");
        }
        // 连请求里的租户都没被拿去查库（503 在租户比对之前）；查询原文不出现在正文里。
        assert!(!text.contains("SELECT"), "错误正文不许回显查询原文：{text}");

        // MCP：同一个错误（信封里），HTTP 仍是 200 —— 传输没坏，是工具执行失败。
        let line = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"sparql_query","arguments":{"query":"SELECT ?s WHERE { ?s a :customers }"}}}"#;
        let (status, _, rpc) = call(
            build_router(test_service_ontology_only()),
            post("/mcp", Some(TOKEN_A), line),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rpc}");
        let v: Value = serde_json::from_str(&rpc).unwrap();
        assert_eq!(v["result"]["isError"], true, "{v}");
        assert_eq!(v["result"]["structuredContent"]["code"], "NO_DATA_SOURCE", "{v}");
        // 「同语义」的硬判据：两个协议的 code 与 hint **逐字相同**。
        assert_eq!(
            v["result"]["structuredContent"]["hint"], rest["hint"],
            "REST 与 MCP 的 hint 必须逐字相同"
        );
        assert_eq!(
            v["result"]["structuredContent"]["message"], rest["message"],
            "REST 与 MCP 的 message 必须逐字相同"
        );

        // sample 也是取数据 ⇒ 同一个 503 语义（⛔ 不返回空样例）。
        let line = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"sample","arguments":{"name":":customers"}}}"#;
        let (status, _, rpc) = call(
            build_router(test_service_ontology_only()),
            post("/mcp", Some(TOKEN_A), line),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rpc}");
        let v: Value = serde_json::from_str(&rpc).unwrap();
        assert_eq!(v["result"]["structuredContent"]["code"], "NO_DATA_SOURCE", "{v}");
    }

    /// ⭐ AC-008：MCP 的 `catalog` / `describe` / `resources/read` 在本体模式**照常工作**。
    ///
    /// 变异：把 MCP 侧也按「没有数据源」一刀切（连 catalog 都 503）⇒ 本测试红。
    #[tokio::test]
    async fn ontology_mode_mcp_catalog_describe_and_resources_still_work() {
        let svc = test_service_ontology_only();
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"catalog","arguments":{"page":1,"page_size":200}}}"#;
        let (_, _, rpc) = call(build_router(Arc::clone(&svc)), post("/mcp", Some(TOKEN_A), line)).await;
        let v: Value = serde_json::from_str(&rpc).unwrap();
        assert_eq!(v["result"]["isError"], false, "{v}");
        assert_eq!(v["result"]["structuredContent"]["counts"]["relations"], 1, "{v}");

        let line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe","arguments":{"name":":orders"}}}"#;
        let (_, _, rpc) = call(build_router(Arc::clone(&svc)), post("/mcp", Some(TOKEN_A), line)).await;
        let v: Value = serde_json::from_str(&rpc).unwrap();
        assert_eq!(v["result"]["structuredContent"]["display_name"], "orders", "{v}");

        // 整份本体（resource）照样能读：字节来自启动时的缓存，与数据源无关。
        let line = format!(r#"{{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{{"uri":"{RESOURCE_CUSTOMER}"}}}}"#);
        let (_, _, rpc) = call(build_router(Arc::clone(&svc)), post("/mcp", Some(TOKEN_A), &line)).await;
        let v: Value = serde_json::from_str(&rpc).unwrap();
        assert_eq!(
            v["result"]["contents"][0]["text"].as_str().unwrap(),
            svc.ttl.customer().text(),
            "resource 的正文必须与 HTTP 同视图逐字节相同"
        );

        // initialize 的 instructions 要说清「本部署没有数据源」（Agent 只看这一段）。
        let line = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#;
        let (_, _, rpc) = call(build_router(Arc::clone(&svc)), post("/mcp", Some(TOKEN_A), line)).await;
        let v: Value = serde_json::from_str(&rpc).unwrap();
        let instructions = v["result"]["instructions"].as_str().unwrap();
        assert!(
            instructions.contains("本体服务模式") && instructions.contains("NO_DATA_SOURCE"),
            "instructions 必须说清没有数据源：{instructions}"
        );
    }

    /// ⭐ REQ-137 · AC-005 / R3：本体模式的审计行写 `tenant=-`，
    /// ⛔ 日志里不出现任何占位租户（尤其全 0 UUID）。
    ///
    /// 变异：把 `audit_call` 里的 `tenant` 改回 `Some(..)`（哪怕写空串）⇒ 第一条断言红；
    /// 把 token 解析放行一个全 0 租户 ⇒ 第二条断言红。
    #[tokio::test]
    async fn ontology_mode_audit_has_dash_tenant_and_no_placeholder_uuid() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req137-audit-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("audit.log");
        let svc = test_service_ontology_only_with_audit(AuditSink::open(&path).expect("审计文件"));

        // 一次 catalog（200）+ 一次 SPARQL（503）：两种出口各一行。
        let (status, _, body) = call(
            build_router(Arc::clone(&svc)),
            get("/api/catalog?page=1&page_size=200", Some(TOKEN_A)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, _, body) = call(
            build_router(Arc::clone(&svc)),
            post(
                "/api/sparql",
                Some(TOKEN_A),
                r#"{"query":"SELECT ?s WHERE { ?s a :customers }"}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");

        let text = std::fs::read_to_string(&path).expect("读审计");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "两次请求两行：\n{text}");
        for line in &lines {
            assert!(
                line.contains("tenant=-"),
                "本体模式的审计行必须是 tenant=-（⛔ 不是空串、不是占位租户）：{line}"
            );
        }
        assert!(
            !text.contains("00000000-0000-0000-0000-000000000000"),
            "⛔ 占位租户进了审计：{text}"
        );
        assert!(!text.contains(TOKEN_A), "⛔ 审计里出现了 token 明文：{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
