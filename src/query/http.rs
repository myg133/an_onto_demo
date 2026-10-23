//! REQ-123 · **HTTP 适配层** —— REST 四个端点 + MCP over HTTP（JSON-RPC + SSE）。
//!
//! # 一个进程、两种协议、**一份鉴权**
//!
//! 身份门是 [`require_token`]（一个 `middleware::from_fn_with_state`），同时挂在
//! `/api/catalog`、`/api/describe`、`/api/sparql`、`/mcp` 上。它做三件事：
//! ① 只认 `Authorization: Bearer`（⛔ query string / cookie）；② 查 token 表；
//! ③ 把 [`Auth`] 放进请求扩展（handler 只能用表里那一行，⛔ 读不到请求体里的 tenant）。
//!
//! `/api/health` **在门之外**（AC-001：免 token），但它只给版本与语法版本 —— 零数据。
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
use super::{
    audit_anonymous, run_catalog, run_describe, run_sample, run_sparql, Caller, CatalogRequest,
    DescribeRequest, Protocol, QueryService, SampleRequest, SparqlRequest, SYNTAX_VERSION,
};

/// 请求体上限（一条 SPARQL 远小于这个数；挡住「用超大 body 拖垮进程」）。
const MAX_BODY_BYTES: usize = 256 * 1024;

/// SSE keep-alive 的间隔（也是「空闲」的探测粒度）。
const SSE_KEEPALIVE: Duration = Duration::from_secs(5);

/// 服务端发给客户端的 `initialize.instructions`。
///
/// 只写**怎么用**与**租户从哪来**，不写任何数据。⛔ 不放项目名/租户名。
pub const INSTRUCTIONS: &str = "\
ontok 查询服务（远程 MCP）。工具：catalog（翻页看类/属性/关系）、describe（看一个名字的详情）、\
sparql_query（执行 SPARQL v1 子集）、sample（取样例值）。
租户由你的凭据决定：请求里不需要也不能改变租户；带 tenant 字段时必须与凭据一致，否则 403。
SPARQL v1 只支持单条 SELECT + 基本图模式（1–3 跳）+ FILTER + LIMIT/OFFSET + 单列 ORDER BY +
COUNT/SUM/MIN/MAX/AVG + 单列 GROUP BY；不支持 OPTIONAL/UNION/子查询/BIND/属性路径/函数。
出错时读 hint（可行动）；找不到名字时先用 catalog。";

/// 身份门放进请求扩展的那一行 token（handler 只能拿它，⛔ 拿不到请求体里的 tenant）。
#[derive(Debug, Clone)]
pub struct Auth(pub TokenEntry);

/// 组装路由。
///
/// - `/api/health` 免 token（只给版本 + 语法版本，零数据）；
/// - `/api/*` 与 `/mcp` 全部过 [`require_token`]；
/// - **未注册的 `/api/*` 也走身份门**（无 token ⇒ 401，不泄露「这个接口存不存在」）；
///   已授权的人问路 ⇒ 404 + 可用端点清单。
pub fn build_router(svc: Arc<QueryService>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/sparql", post(post_sparql))
        .route("/api/catalog", get(get_catalog))
        .route("/api/describe", get(get_describe))
        .route("/mcp", post(mcp_post).get(mcp_sse))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(svc.clone(), require_token))
        .with_state(svc)
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
                     POST /mcp（JSON-RPC）、GET /mcp（SSE）、GET /api/health（免 token）。",
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
            audit_anonymous(&svc, "auth", proto, e.code, started);
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

/// `GET /api/health` —— **免 token**，只给版本与语法版本（AC-001：不含任何数据）。
async fn health(State(svc): State<Arc<QueryService>>) -> Response {
    let started = Instant::now();
    // ⛔ 这里刻意**不读** svc 的任何数据字段（项目名、租户、类清单都不出去）。
    // 只把「这个服务是什么版本、说哪种语法」告诉探针。
    audit_anonymous(&svc, "health", Protocol::Rest, "OK", started);
    respond(
        StatusCode::OK,
        json!({
            "service": "ontok-query",
            "version": env!("CARGO_PKG_VERSION"),
            "syntax": SYNTAX_VERSION,
            "protocols": ["rest", "mcp-http"],
            "transports": ["http", "sse"],
        }),
    )
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
            // `tools/call` 的审计在**适配器**里（一次工具调用一条，含 rows/耗时/错误码）；
            // 其余方法（initialize / tools/list / ping）在这里补一条信封级审计。
            if req.method != "tools/call" {
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
            p::encode_result(id, p::initialize_result(agreed, INSTRUCTIONS))
        }
        "notifications/initialized" => p::encode_result(id, p::empty_result()),
        "ping" => p::encode_result(id, p::empty_result()),
        "tools/list" => p::encode_result(id, p::tools_list_result(Value::Array(tool_defs()))),
        "tools/call" => tools_call(svc, caller, req).await,
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
                            public_dimensions/warnings/coverage/join_states/limits。",
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
        let (schema, mapping, model) = crate::query::test_fixture();
        let policy = crate::sparql::policy::QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id"}},"publicDimensions":[]}"#,
        )
        .expect("声明");
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
            source: sqlx::postgres::PgPoolOptions::new()
                // 夹具池**故意连不上**：判据要的是「鉴权/限额在连库之前就答完」，
                // 短 acquire_timeout 让「确实要连库」的那条分支快速变成 500（而不是等 30 s）。
                .acquire_timeout(std::time::Duration::from_millis(300))
                .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
                .expect("lazy pool"),
            facts: SessionFacts {
                transaction_read_only: "on".into(),
                standard_conforming_strings: "on".into(),
            },
            mapping,
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
            audit: AuditSink::stderr_only(),
        }));
        svc
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

    /// AC-001：`/api/health` 免 token，且**不含任何数据**（项目名 / 租户 / 类名都不出现）。
    #[tokio::test]
    async fn health_needs_no_token_and_carries_no_data() {
        let app = build_router(test_service());
        let (status, headers, body) = call(app, get("/api/health", None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["service"], "ontok-query");
        assert_eq!(v["syntax"], SYNTAX_VERSION);
        assert!(v["version"].as_str().is_some());
        // ⛔ 零数据：项目名、租户 id、类名都不许出现。
        for forbidden in ["demo", "C0001", "customers", "orders", "sql"] {
            assert!(
                !body.contains(forbidden),
                "health 里出现了 {forbidden:?}（AC-001 要求零数据）：{body}"
            );
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
        assert!(v["result"]["instructions"].as_str().unwrap().contains("sparql_query"));

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
}
