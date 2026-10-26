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
//! ⭐ REQ-140 · JSON-LD（`format=jsonld`）**没有**第三个写点：它复用 [`respond`]
//! （同一个 `Json(`/同一个序列化器），只把 `Content-Type` 改成 `application/ld+json`
//! —— 见 [`respond_jsonld`]。于是「出口表」不需要登记新出口，也不存在
//! 「同一个回执有两套序列化」。
//!
//! ⭐ REQ-142 · **默认翻转**：上面那条 JSON-LD 现在是 REST / MCP 的**默认**回执
//! （数字员工要图）。退回原来的行回执：`?format=rows` / `Accept: application/json` /
//! MCP `format:"rows"`（三选一，字节与 REQ-123 逐字节相同，R2）。
//! ⚠️ `Accept: */*` **不再**等于「要表」—— curl / 浏览器的默认头不改变默认。
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
    audit_anonymous, audit_ttl, run_catalog, run_describe, run_sample, run_sparql, run_sparql_ex,
    Caller, CatalogRequest, DescribeRequest, Protocol, QueryFormat, QueryService, SampleRequest,
    SparqlRequest, DEFAULT_FORMAT, SYNTAX_VERSION,
};

/// 请求体上限（一条 SPARQL 远小于这个数；挡住「用超大 body 拖垮进程」）。
const MAX_BODY_BYTES: usize = 256 * 1024;

/// ⭐ REQ-142 · 「要**行回执**」的 `Accept` 媒体类型。
///
/// `?format=rows` 之外唯一的隐式退回路径；⛔ `*/*` / `application/*` 都不算
/// （它们不是偏好，只是「随便」—— 而 REQ-142 的默认是图）。
const ROWS_MEDIA: &str = "application/json";

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
⭐ sparql_query 的**默认回执是 JSON-LD 上下文子图**（@context/@graph/summary/meta，
同 @id 合并、按 @id 排序）；要表格（columns/rows/…）传 format:\"rows\"。
出错时读 hint（可行动）；找不到名字时先用 catalog。";

/// 身份门放进请求扩展的那一行 token（handler 只能拿它，⛔ 拿不到请求体里的 tenant）。
#[derive(Debug, Clone)]
pub struct Auth(pub TokenEntry);

/// 组装路由。
///
/// - `/api/health` 免 token（产品版本 + 项目名 + **本体**版本 + 语法版本；⛔ 无租户/表名）；
/// - `/api/*` 与 `/mcp` 全部过 [`require_token`]；
/// - ⭐ REQ-155 · `/studio` 与两个静态资源**不在门里**（`needs_auth` 只认 `/api` 与 `/mcp`），
///   但它们是**编译期常量、零数据** —— 壳可匿名取得，拿了也没有任何本体内容；
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
        // ⭐ REQ-155 · Studio 页面壳（免 token、零数据、**只读**）。
        // 三条都只注册 `get` ⇒ `POST/PUT/DELETE /studio` 由 axum 直接回 405（S1）。
        // ⛔ 不改 `/api/*` 的任何行为：这里只是把三个**编译期常量**贴到同一个源上。
        .route(crate::studio::STUDIO_PATH, get(crate::studio::index))
        .route(crate::studio::STUDIO_INDEX_PATH, get(crate::studio::index))
        .route(crate::studio::CSS_PATH, get(crate::studio::css))
        .route(crate::studio::JS_PATH, get(crate::studio::js))
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

/// `POST /api/sparql` —— body `{query, max_rows?, tenant?, format?}`。
///
/// ⭐ REQ-140 / **REQ-142** · **回执格式**（三处入口，⭐ 默认已翻转为 JSON-LD）：
///
/// | 入口 | 默认 | 怎么要回**行回执** |
/// | --- | --- | --- |
/// | REST | **JSON-LD** | `?format=rows` / `Accept: application/json` / body 的 `format:"rows"` |
/// | MCP | **JSON-LD** | `sparql_query` 的 `arguments.format = "rows"` |
///
/// 优先级：**显式 `?format=` > body 的 `format` > `Accept` 头**（后两者冲突时以更具体的
/// 那个为准。⚠️ REQ-140 已实现并断言的就是这个顺序，本需求只改最底层的兜底）。
/// `Accept` 只认两个**明确**的媒体类型：`application/ld+json` ⇒ 图、
/// `application/json` ⇒ 表；`*/*` / `application/*` / 缺失 / 其它 ⇒ **默认（图）**。
/// ⛔ `*/*` 不再等于「要表」—— 这正是 REQ-142 的语义改变点（curl / 浏览器的默认头
/// 不再能悄悄把默认行为改回表）。
///
/// ⛔ 默认（三处都不写）走 [`DEFAULT_FORMAT`] = `JsonLd`；要旧的行回执就
/// 显式写 `format=rows`，其响应体与 REQ-123 **逐字节相同**（R2）。
async fn post_sparql(
    State(svc): State<Arc<QueryService>>,
    Extension(auth): Extension<Auth>,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
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
    // ⭐ REQ-140 · 格式在**执行之后**才用得上，但参数错要在执行之前报（省一次查询）。
    let format = match sparql_format(&q, &headers, req.format) {
        Ok(f) => f,
        Err(e) => {
            super::audit_call(&svc, "sparql", &caller, None, None, started, e.code, None);
            return respond(e.status, e.body());
        }
    };
    // ⭐ REQ-140 · 默认（rows）**就是原来那一行**：`run_sparql` → `Json(receipt)`。
    // JSON-LD 是另一条渲染分支，但**同一次查询**（`run_sparql_ex` 只是多交出列的 RDF 身份）。
    //
    // ⭐ REQ-145 · 两条分支对「只碰 api 源」的表达**不同**（同一个判定，见
    // `query::SparqlAnswer`）：jsonld 挂得住 `@pending` ⇒ 返回「半成品 + 说明书」；
    // rows 挂不住 ⇒ `run_sparql` 直接给 `NOT_DB_SOURCE`（R5：⛔ 不返回空表）。
    if format.is_jsonld() {
        return match run_sparql_ex(&svc, &caller, req).await {
            Ok(super::SparqlAnswer::Rows(out)) => respond_jsonld(super::jsonld::render_with_mapping(
                &out.receipt,
                &out.projection,
                &svc.ontology_iri,
                svc.access.as_ref().map(|a| a.mapping_version()),
                // ⭐ REQ-154 · 映射声明什么就给什么（没加载映射 ⇒ `None` ⇒ 键不出现）。
                svc.access.as_ref().map(|a| a.contract_version()),
            )),
            Ok(super::SparqlAnswer::Pending(p)) => respond_jsonld(super::jsonld::render_pending(
                &p.receipt,
                &p.pending,
                &svc.ontology_iri,
                p.mapping_version,
                p.contract_version,
            )),
            Err(e) => respond(e.status, e.body()),
        };
    }
    match run_sparql(&svc, &caller, req).await {
        // ⭐ 回执的**唯一**序列化路径：`Json(receipt)` 与 MCP 的
        // `content[0].text = receipt.serialize()` 出自同一个 `Serialize` 实现。
        Ok(receipt) => respond(StatusCode::OK, receipt),
        Err(e) => respond(e.status, e.body()),
    }
}

/// ⭐ REQ-140 / REQ-142 · `?format=` / body / `Accept` 三处的**唯一**解析
/// （优先级见 [`post_sparql`]）。
///
/// 拼错 / 不认识的 query 参数一律 **400**（与 `/api/ontology.ttl` 的 `?view=` 同一条口径：
/// ⛔ 不静默回退到默认 —— 那样调用方会拿着一个「图」以为拿到了表，或者反过来）。
fn sparql_format(
    q: &BTreeMap<String, String>,
    headers: &HeaderMap,
    from_body: Option<QueryFormat>,
) -> Result<QueryFormat, ApiError> {
    let mut explicit: Option<QueryFormat> = None;
    for (k, v) in q {
        match k.as_str() {
            "format" => {
                explicit = Some(QueryFormat::parse(v).ok_or_else(|| {
                    ApiError::bad_request(
                        format!("format 只认 rows / jsonld，收到 {v:?}。"),
                        "默认是 **JSON-LD 上下文子图**，不写 `format` 就是图；\
                         要**行回执**（表格）写 `?format=rows`（或 `Accept: application/json`）。\
                         要显式要图写 `?format=jsonld`。⛔ 拼错不会静默回退。",
                    )
                })?)
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
                    "`/api/sparql` 只接受 `format=rows|jsonld`（省略 = JSON-LD 上下文子图）。",
                ))
            }
        }
    }
    if let Some(f) = explicit {
        return Ok(f);
    }
    if let Some(f) = from_body {
        return Ok(f);
    }
    // ⭐ REQ-142 · 默认翻转：`Accept` 给了**明确**的媒体类型就听它的，否则默认 = 图。
    if let Some(f) = accept_format(headers) {
        return Ok(f);
    }
    Ok(DEFAULT_FORMAT)
}

/// `Accept` 里的**明确**偏好：`application/ld+json` ⇒ 图；`application/json` ⇒ 表。
///
/// ⛔ `*/*` / `application/*` / 缺失 / 其它类型 **不是**偏好（返回 `None`）—— 它们不是
/// 「我要表」的声明，而是「随便」。REQ-142 起 `None` 落到默认（**图**）；要表必须写
/// `application/json`（或 `?format=rows`）。
///
/// 两个都写时以 `ld+json` 为准：它是**更具体**的那一个（本服务的默认也正是图），
/// 且与 `?format=` 的「显式压过隐式」同一条口径。
fn accept_format(headers: &HeaderMap) -> Option<QueryFormat> {
    let mut wants_rows = false;
    let mut wants_jsonld = false;
    for raw in headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|v| v.to_str().ok())
    {
        for part in raw.split(',') {
            let media = part.split(';').next().unwrap_or("").trim();
            if media.eq_ignore_ascii_case(super::jsonld::CONTENT_TYPE_MEDIA) {
                wants_jsonld = true;
            } else if media.eq_ignore_ascii_case(ROWS_MEDIA) {
                wants_rows = true;
            }
        }
    }
    if wants_jsonld {
        Some(QueryFormat::JsonLd)
    } else if wants_rows {
        Some(QueryFormat::Rows)
    } else {
        None
    }
}

/// ⭐ REQ-140 · JSON-LD 响应：**复用** [`respond`] 这唯一一个 `Json(` 写点，
/// 只把 `Content-Type` 换成 `application/ld+json`。
///
/// 正文由 `Json(JsonLd)` 序列化 —— 与 MCP 的 `content[0].text`（`JsonLd::to_text()`）
/// 出自**同一个** `Serialize`，所以两个协议逐字节相同。
/// 这样「出口表」不用加第 4 个写点：`Json(` 在本文件里仍然只有一处（I14）。
fn respond_jsonld(body: super::jsonld::JsonLd<'_>) -> Response {
    let mut res = respond(StatusCode::OK, body);
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(super::jsonld::CONTENT_TYPE),
    );
    res
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
            // ⭐ REQ-151 · 也不能再说「恒失败」：纯 api 类（有 access 映射）会返回 @pending。
            // 说清**哪种**查询会失败，Agent 才不会把「有指引」当成「服务坏了」。
            let instructions = if svc.mode.is_ontology_only() {
                format!(
                    "{INSTRUCTIONS}\n\
                     ⚠️ 本部署是**本体服务模式**（无业务库）：catalog / describe / resources 正常；\
                     需要库的 sparql_query 与 sample 返回 NO_DATA_SOURCE（503 语义，重试无效）；\
                     只涉及 api 类（catalog 的 access.kind=api）的查询用默认 format=jsonld 时，\
                     会得到 `@graph: []` + `@pending` 取数指引（format=rows 时报 NOT_DB_SOURCE）。\n\
                     ⚠️ `max_rows=0` 是**参数错误（400 BAD_REQUEST）**，与部署模式无关；\
                     需要库的查询仍先得到 503（模式闸在前），纯 api 类才会看到这个 400。\n\
                     ⚠️ 本体模式的 `@pending[].get.filter` 可能是 `{{}}`：那表示**平台无法替你\
                     确定取数标识**（没有库侧主键）。⛔ **不要直接全量拉取**（账单 / 告警一类接口\
                     会烧钱并触发限流）：先按该类的实例标识属性（`describe` 看属性，\
                     如 `:cvm_instance__instance_id`）在**你自己的取数侧**拿到标识，再带 \
                     `get.filter.ids` 重新组织取数；暂时拿不到标识时，至少用 `get.pushdown` \
                     里的字段收窄范围。"
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
            Ok(r) => {
                // ⭐ REQ-140 / REQ-142 · `format` 只决定渲染：**省略 ⇒ JSON-LD**（默认，
                // `QueryFormat::default()` = `JsonLd`）；`rows` 走**原来那一行**（行回执，
                // 与 REST 的 `?format=rows` 逐字节相同）。
                let format = r.format.unwrap_or_default();
                if !format.is_jsonld() {
                    return match run_sparql(svc, caller, r).await {
                        // ⭐ `content[0].text` 与 REST 正文逐字节相同（同一个 `serialize`）。
                        Ok(receipt) => tool_ok(
                            id,
                            &receipt.serialize(),
                            serde_json::to_value(&receipt).unwrap_or(Value::Null),
                        ),
                        Err(e) => tool_err(id, &e),
                    };
                }
                match run_sparql_ex(svc, caller, r).await {
                    // ⭐ REQ-145 · MCP 与 REST **同一个**判定与同一份渲染：
                    // `rows` ⇒ `NOT_DB_SOURCE`（`tools_call` 的 rows 分支走 `run_sparql`）；
                    // `jsonld` ⇒ `@graph: []` + `@pending` + `complete:false`。
                    Ok(super::SparqlAnswer::Rows(out)) => {
                        let graph = super::jsonld::render_with_mapping(
                            &out.receipt,
                            &out.projection,
                            &svc.ontology_iri,
                            svc.access.as_ref().map(|a| a.mapping_version()),
                            svc.access.as_ref().map(|a| a.contract_version()),
                        );
                        tool_ok(id, &graph.to_text(), graph.to_value())
                    }
                    Ok(super::SparqlAnswer::Pending(p)) => {
                        let graph = super::jsonld::render_pending(
                            &p.receipt,
                            &p.pending,
                            &svc.ontology_iri,
                            p.mapping_version,
                            p.contract_version,
                        );
                        tool_ok(id, &graph.to_text(), graph.to_value())
                    }
                    Err(e) => tool_err(id, &e),
                }
            }
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
                            （= 涉及但本次没用到；人话说明见 join_states_scope_note）。\
                            ⭐ 回执格式：**默认** format=\"jsonld\" —— 上下文子图（@context/@graph/summary/meta，\
                            同 @id 的多行合并成一个节点、节点按 @id 排序、聚合进 summary），且**不回 rows**；\
                            要**行回执**（columns/rows/…，即旧的回执）写 format:\"rows\"，字节与以前完全一致。",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "一条 SELECT（v1 子集）" },
                    "max_rows": { "type": "integer", "minimum": 1, "description": "行数上限；不会超过凭证上限" },
                    "tenant": { "type": "string", "description": "**只用于核对**：必须等于凭证绑定的租户，否则 403。正常调用不用写。" },
                    "format": {
                        "type": "string",
                        "enum": ["rows", "jsonld"],
                        "default": "jsonld",
                        "description": "回执格式：jsonld（**默认**，上下文子图，供推理）/ rows（行回执表）。只影响渲染。"
                    }
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
            access: None,
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
            access: None,
        }))
    }

    /// ⭐ REQ-151 · **本体模式 + access 映射**的夹具，且**故意塞一个连不上的源库池**。
    ///
    /// 为什么 `source: Some(坏池)`：生产上本体模式是 `None`（`serve` 根本不读 URL），
    /// 这里塞一个池是为了让「任何代码路径都拿不到池」变成**可测**的事实 ——
    /// 池的 `acquire_timeout` 只有 100 ms，真去连会立刻变成 500/TIMEOUT，
    /// 于是「api 类仍是 200 @pending / 库类仍是 503」这组断言本身就证明零连接尝试。
    fn test_service_ontology_only_with_access(audit: AuditSink) -> Arc<QueryService> {
        // ⭐ REQ-154 · v1 已签映射的形状（`contract_version: 1` ⇒ `get` 只有旧 4 键）。
        test_service_ontology_only_with_mapping(
            r#"{"contract_version":1,"mapping_version":3,
                "entities":[{"id":"Customer","class":"customers",
                             "capability":{"pushdown":["ids"]},"freshness":"realtime"}]}"#,
            audit,
        )
    }

    /// ⭐ REQ-154 · 同 [`test_service_ontology_only_with_access`]，但映射由调用方给
    /// （`meta.contract_version` 的判据要用**同一份服务、只换映射**来对拍 v1 / v2）。
    fn test_service_ontology_only_with_mapping(
        mapping_json: &str,
        audit: AuditSink,
    ) -> Arc<QueryService> {
        let (schema, _mapping, model) = crate::query::test_fixture();
        let _ = schema;
        let policy = crate::sparql::policy::QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id"}},"publicDimensions":[]}"#,
        )
        .expect("声明");
        let ontology_iri = {
            let opts = crate::domain::ttl::TtlOptions::new("demo", 2);
            opts.ontology_iri()
        };
        let tokens = TokenTable::parse_for_mode(
            &format!(
                r#"{{"version":"1.0.0","tokens":[
                {{"label":"a","token":"{TOKEN_A}","max_rows":5,"max_concurrency":1}}
            ]}}"#
            ),
            crate::query::ServiceMode::OntologyOnly,
        )
        .expect("token 表");
        // `customers` 是 api 类、`orders` 是库类 ⇒ 一句查询就能造出「纯 api / 纯库 / 混合」。
        let access = crate::query::source_gate::AccessMapping::parse(mapping_json)
            .expect("映射形状")
            .bind(&model)
            .expect("映射与本体对账");
        let poisoned = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(100))
            .connect_lazy("postgresql://nonexistent.invalid:1/x")
            .expect("lazy pool");
        Arc::new(QueryService::new(ServiceParts {
            mode: crate::query::ServiceMode::OntologyOnly,
            // ⚠️ 刻意的「不可能状态」：本体模式不该有池 —— 若代码真去拿，判据会红。
            source: Some(poisoned),
            facts: None,
            // 与 `serve` 一致：本体模式不建 R2RML 映射（映射只用于库源规划）。
            mapping: None,
            model,
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
            access: Some(access),
        }))
    }

    async fn call(app: Router, req: axum::http::Request<Body>) -> (StatusCode, HeaderMap, String) {        let res = app.oneshot(req).await.expect("oneshot");
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

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-151 · 本体模式只对「需要库」的查询 503；纯 api 类给 @pending
    // ══════════════════════════════════════════════════════════

    /// AC-001 / AC-002：本体模式 + 纯 api 类 ⇒ jsonld 200 + `@pending`；rows 400 `NOT_DB_SOURCE`。
    ///
    /// 夹具**故意带一个连不上的源库池**（`source: Some(坏池)`）：
    /// 若这条路径尝试取连接，响应会是 500/TIMEOUT，而不是这里的 200/400 ——
    /// 于是本测试同时是 **AC-006「零连接尝试」** 的判据。
    #[tokio::test]
    async fn req151_ontology_mode_pure_api_gets_pending_and_rows_gets_not_db_source() {
        let svc = test_service_ontology_only_with_access(AuditSink::stdout_only());
        // AC-006 的结构面：唯一取池入口在本模式恒 None（哪怕 source 字段被塞了池）。
        assert!(
            svc.source_pool().is_none(),
            "本体模式下 source_pool 必须恒 None（否则「绝不碰源库」只靠顺序）"
        );

        let q = r#"{"query":"SELECT ?c WHERE { ?c a :customers }"}"#;
        let (status, _, body) = call(
            build_router(Arc::clone(&svc)),
            post("/api/sparql?format=jsonld", Some(TOKEN_A), q),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "AC-001：纯 api 类不是 503：{body}");
        let v: Value = serde_json::from_str(&body).expect("JSON-LD");
        assert_eq!(v["@graph"], json!([]), "AC-001：@graph 必须空：{v}");
        let pending = v["@pending"].as_array().expect("@pending 数组");
        assert_eq!(pending.len(), 1, "{v}");
        assert_eq!(pending[0]["reason"], "source_is_api", "{v}");
        let get = &pending[0]["get"];
        assert_eq!(get["class"], "customers", "R5：与普通模式同一份 get：{v}");
        assert_eq!(get["pushdown"], json!(["ids"]), "{v}");
        assert_eq!(get["ref"], "access#Customer@v3", "{v}");
        assert!(get["filter"].is_object(), "filter 恒是对象：{v}");
        assert_eq!(v["meta"]["complete"], false, "AC-001：{v}");
        assert_eq!(v["meta"]["unresolved_classes"], json!(["customers"]), "{v}");
        assert_eq!(v["meta"]["mapping_version"], 3, "{v}");

        // AC-002：行回执挂不住「少了一块」⇒ 400 NOT_DB_SOURCE（⛔ 不是 503、⛔ 不是空表）。
        let (status, _, body) = call(
            build_router(Arc::clone(&svc)),
            post("/api/sparql?format=rows", Some(TOKEN_A), q),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "AC-002：{body}");
        let v: Value = serde_json::from_str(&body).expect("错误 JSON");
        assert_eq!(v["code"], "NOT_DB_SOURCE", "{v}");
        assert!(
            v["hint"].as_str().is_some_and(|h| h.contains("jsonld") && h.contains("拆成两段")),
            "AC-002：hint 必须可行动：{v}"
        );
        assert!(v.get("rows").is_none(), "⛔ 不许返回空表：{v}");
    }

    /// ⭐ REQ-154 · `meta.contract_version` = **映射声明的版本**（v1 ⇒ 1 / v2 ⇒ 2），
    /// 并且 `get` 的键集合**一个不多一个不少**（v1 4 键 / v2 10 键，R1）。
    ///
    /// 为什么这条判据同时钉「键数」：版本放 `meta` 的目的就是让 `get` 保持原样 ——
    /// 若哪天有人图省事把 `contract_version` 塞进 `get`，v1 会变成 5 键、v2 变成 11 键，
    /// 这里立刻红。变异自证：把 meta 里的值写死成 `1` ⇒ v2 那条红。
    #[tokio::test]
    async fn req154_meta_carries_contract_version_without_touching_get_keys() {
        let v1_mapping = r#"{"contract_version":1,"mapping_version":3,
            "entities":[{"id":"Customer","class":"customers",
                         "capability":{"pushdown":["ids"]},"freshness":"realtime"}]}"#;
        // v2：源 + 六段配方都写全 ⇒ `get` 恰好 10 键。
        let v2_mapping = r#"{
            "contract_version": 2,
            "mapping_version": 3,
            "sources": [{
                "id": "s1",
                "base_url": "https://api.example.test",
                "auth": { "mode": "tc3-hmac-sha256",
                          "credential_env": ["FAKE_SECRET_ID", "FAKE_SECRET_KEY"] }
            }],
            "entities": [{
                "id": "Customer", "class": "customers", "source": "s1",
                "capability": { "pushdown": ["ids"] },
                "freshness": "realtime",
                "operation": { "method": "POST", "path": "/" },
                "request": { "template": { "InstanceIds": ["{ids}"] } },
                "iterator": "$.Response.CustomerSet[*]",
                "fields": { "name": { "path": "CustomerName" } }
            }]
        }"#;
        let q = r#"{"query":"SELECT ?c WHERE { ?c a :customers }"}"#;

        for (label, mapping, expected_cv, expected_keys) in [
            (
                "v1 映射",
                v1_mapping,
                1u64,
                vec!["class", "filter", "pushdown", "ref"],
            ),
            (
                "v2 映射",
                v2_mapping,
                2u64,
                vec![
                    "class", "filter", "pushdown", "ref", "source", "operation", "request",
                    "iterator", "fields", "auth",
                ],
            ),
        ] {
            let svc = test_service_ontology_only_with_mapping(mapping, AuditSink::stdout_only());
            let (status, _, body) = call(
                build_router(Arc::clone(&svc)),
                post("/api/sparql?format=jsonld", Some(TOKEN_A), q),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{label}：{body}");
            let v: Value = serde_json::from_str(&body).expect("JSON-LD");
            assert_eq!(
                v["meta"]["contract_version"], expected_cv,
                "{label}：`meta.contract_version` 必须是映射声明的版本：{v}"
            );
            assert_eq!(
                v["meta"]["mapping_version"], 3,
                "{label}：两个版本是两个字段，⛔ 不许互相顶替：{v}"
            );
            let get = v["@pending"][0]["get"]
                .as_object()
                .unwrap_or_else(|| panic!("{label}：按类点名必须给 get：{v}"));
            let mut keys: Vec<&str> = get.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let mut want = expected_keys.clone();
            want.sort_unstable();
            assert_eq!(
                keys, want,
                "{label}：`get` 的键集合不许变（版本放 meta，不放在 get 里）：{v}"
            );

            // MCP 的 `sparql_query` 走**另一处**调用来渲染（同一个值）—— 两处都要有。
            if expected_cv == 2 {
                let line = format!(
                    r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"sparql_query","arguments":{q}}}}}"#
                );
                let (status, _, rpc) = call(build_router(Arc::clone(&svc)), post("/mcp", Some(TOKEN_A), &line)).await;
                assert_eq!(status, StatusCode::OK, "{rpc}");
                let v: Value = serde_json::from_str(&rpc).expect("MCP 行");
                assert_eq!(
                    v["result"]["structuredContent"]["meta"]["contract_version"], 2,
                    "MCP 与 REST 必须同一份渲染（同一个前置版本字段）：{v}"
                );
            }
        }
    }

    /// AC-003 / AC-004：本体模式下**需要库**的查询仍 503，且响应与基线**逐字节相同**。
    ///
    /// 变异：把混合查询也放行（`needs_db` 恒 false）⇒ 混合那条红；
    /// 改 `ApiError::no_data_source()` 的文案 ⇒ 两条都红（R2）。
    #[tokio::test]
    async fn req151_ontology_mode_db_and_mixed_still_503_byte_identical() {
        let svc = test_service_ontology_only_with_access(AuditSink::stdout_only());
        // R2 的判据：503 正文必须还是 REQ-136 那一份（同一个构造函数）。
        let expected = serde_json::to_string(&ApiError::no_data_source().body()).expect("JSON");
        for (label, query) in [
            ("AC-003 纯库类", r#"{"query":"SELECT ?o WHERE { ?o a :orders }"}"#),
            (
                "AC-004 混合（库 + api）",
                r#"{"query":"SELECT ?c ?o WHERE { ?c a :customers . ?o a :orders }"}"#,
            ),
        ] {
            let (status, _, body) = call(
                build_router(Arc::clone(&svc)),
                post("/api/sparql?format=jsonld", Some(TOKEN_A), query),
            )
            .await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{label}：{body}");
            assert_eq!(body, expected, "{label}：码与文案必须与基线逐字节相同（R2）");
        }
    }

    /// AC-005 / AC-007：`sample` 仍 503（MCP 信封 200 + `isError`）；审计三行 = 客户端实收。
    #[tokio::test]
    async fn req151_ontology_mode_sample_still_503_and_audit_codes_match() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req151-audit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("audit.log");
        let svc = test_service_ontology_only_with_access(
            AuditSink::open(&path).expect("审计文件"),
        );

        // AC-001：200（半成品）⇒ 审计写 OK。
        let (status, _, body) = call(
            build_router(Arc::clone(&svc)),
            post(
                "/api/sparql?format=jsonld",
                Some(TOKEN_A),
                r#"{"query":"SELECT ?c WHERE { ?c a :customers }"}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        // AC-002：400 NOT_DB_SOURCE。
        let (status, _, _) = call(
            build_router(Arc::clone(&svc)),
            post(
                "/api/sparql?format=rows",
                Some(TOKEN_A),
                r#"{"query":"SELECT ?c WHERE { ?c a :customers }"}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // AC-003：503 NO_DATA_SOURCE。
        let (status, _, _) = call(
            build_router(Arc::clone(&svc)),
            post(
                "/api/sparql?format=jsonld",
                Some(TOKEN_A),
                r#"{"query":"SELECT ?o WHERE { ?o a :orders }"}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        // AC-005：sample 打 api 类 ⇒ 仍 503 语义（取样必须真连库）。
        let line = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"sample","arguments":{"name":":customers"}}}"#;
        let (status, _, rpc) = call(
            build_router(Arc::clone(&svc)),
            post("/mcp", Some(TOKEN_A), line),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rpc}");
        let v: Value = serde_json::from_str(&rpc).expect("JSON-RPC");
        assert_eq!(v["result"]["isError"], true, "{v}");
        assert_eq!(
            v["result"]["structuredContent"]["code"], "NO_DATA_SOURCE",
            "AC-005：{v}"
        );

        // AC-007：四条审计，`code` 必须是客户端**实际收到**的那个（R4）。
        let text = std::fs::read_to_string(&path).expect("读审计");
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 4, "四次调用四行：\n{text}");
        let expect = [
            ("code=OK", true),            // jsonld 200
            ("code=NOT_DB_SOURCE", true), // rows 400
            ("code=NO_DATA_SOURCE", false), // 库类 503
            ("code=NO_DATA_SOURCE", false), // sample 503 语义
        ];
        for (line, (code, is_pending)) in lines.iter().zip(expect) {
            assert!(line.contains(code), "审计 code 必须是客户端实收（R4）：{line}");
            // `pending=false` 在 `AuditRecord` 的 Display 里不打印（只有 true 才出现）。
            assert_eq!(
                line.contains("pending=true"),
                is_pending,
                "pending 标记不对：{line}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-140 · 回执格式的三处入口（REST 的两种写法共用一个解析）
    // ══════════════════════════════════════════════════════════

    fn q(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn accept(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append(header::ACCEPT, HeaderValue::from_str(v).expect("Accept 头"));
        }
        h
    }

    /// AC-001（REST 的三条路）+ AC-002（`Accept` 四行）+ 默认翻转：
    /// `?format` / `Accept` / body 三处，**都没线索 ⇒ JSON-LD**（REQ-142）。
    #[test]
    fn the_response_format_has_one_resolver_for_all_three_entry_points() {
        // ① 什么都不写 ⇒ **JSON-LD**（REQ-142 翻转的**唯一**改变点；AC-001）
        assert_eq!(
            sparql_format(&q(&[]), &HeaderMap::new(), None).unwrap(),
            QueryFormat::JsonLd
        );
        // ①' 显式要行回执才是表（AC-002 的退回路径）
        assert_eq!(
            sparql_format(&q(&[("format", "rows")]), &HeaderMap::new(), None).unwrap(),
            QueryFormat::Rows
        );
        // ② `?format=jsonld` ⇒ jsonld；`rows` 显式写了也是 rows
        assert_eq!(
            sparql_format(&q(&[("format", "jsonld")]), &HeaderMap::new(), None).unwrap(),
            QueryFormat::JsonLd
        );
        assert_eq!(
            sparql_format(&q(&[("format", "rows")]), &accept(&["application/ld+json"]), None)
                .unwrap(),
            QueryFormat::Rows,
            "显式 ?format=rows 压过 Accept（更具体的那一个说了算）"
        );
        // ③ `Accept: application/ld+json` ⇒ jsonld（带参数也认）
        assert_eq!(
            sparql_format(&q(&[]), &accept(&["application/ld+json;q=0.9"]), None).unwrap(),
            QueryFormat::JsonLd
        );
        // ④ 默认（AC-002）：`*/*` / `application/*` / 其它类型 ⇒ **jsonld**；
        //    只有**明确**的 `application/json` 才是「要表」。
        for default_ish in ["*/*", "text/html, */*", "application/*"] {
            assert_eq!(
                sparql_format(&q(&[]), &accept(&[default_ish]), None).unwrap(),
                QueryFormat::JsonLd,
                "Accept: {default_ish} 不该把默认（图）压回表"
            );
        }
        assert_eq!(
            sparql_format(&q(&[]), &accept(&["application/json"]), None).unwrap(),
            QueryFormat::Rows,
            "Accept: application/json 是隐式要行回执的唯一写法"
        );
        // ⑤ body 的 `format` 也认（MCP 与 REST 的 body 是同一个 DTO）
        assert_eq!(
            sparql_format(&q(&[]), &HeaderMap::new(), Some(QueryFormat::JsonLd)).unwrap(),
            QueryFormat::JsonLd
        );
        assert_eq!(
            sparql_format(&q(&[]), &HeaderMap::new(), Some(QueryFormat::Rows)).unwrap(),
            QueryFormat::Rows
        );
        assert_eq!(
            sparql_format(&q(&[("format", "rows")]), &HeaderMap::new(), Some(QueryFormat::JsonLd))
                .unwrap(),
            QueryFormat::Rows,
            "?format= 优先于 body.format（REQ-140 已实现并断言的顺序，本需求未改）"
        );
    }

    /// ⭐ REQ-142 · AC-001 / AC-004 的**入口级**判据（不连库）：
    /// REST 三处都没线索 ⇒ 图；MCP 省略 `format` ⇒ 取**同一个**默认值（R1）。
    #[test]
    fn req142_the_default_is_jsonld_on_both_entries() {
        // REST：body 无 format、无 `?format=`、无 Accept。
        assert_eq!(
            sparql_format(&BTreeMap::new(), &HeaderMap::new(), None).unwrap(),
            QueryFormat::JsonLd
        );
        // curl / 浏览器的默认头（`*/*`）也压不回表 —— 这正是本次的语义改变点。
        assert_eq!(
            sparql_format(&BTreeMap::new(), &accept(&["*/*"]), None).unwrap(),
            QueryFormat::JsonLd
        );
        // MCP：`sparql_query` 省略 format ⇒ `unwrap_or_default()` 取的就是这个值。
        let req = SparqlRequest::from_value(
            &serde_json::json!({"query": "SELECT ?s WHERE { ?s a :customers }"}),
        )
        .expect("省略 format");
        assert_eq!(req.format, None);
        assert_eq!(req.format.unwrap_or_default(), DEFAULT_FORMAT);
        assert!(DEFAULT_FORMAT.is_jsonld());
        assert_eq!(QueryFormat::default(), DEFAULT_FORMAT, "默认值只有一处");
        // 退回行回执的路仍然在（MCP 与 REST 共用同一个解析）。
        assert_eq!(
            SparqlRequest::from_value(&serde_json::json!({
                "query": "SELECT ?s WHERE { ?s a :customers }", "format": "rows"
            }))
            .unwrap()
            .format,
            Some(QueryFormat::Rows)
        );
    }

    /// ⛔ 拼错 / 不认识的参数**报 400**（⛔ 不静默回退到行回执）。
    #[test]
    fn a_bogus_format_or_an_unknown_query_parameter_is_a_400() {
        let e = sparql_format(&q(&[("format", "json-ld")]), &HeaderMap::new(), None).unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
        assert!(e.message.contains("jsonld"), "{}", e.message);
        assert!(e.hint.contains("?format=jsonld"), "{}", e.hint);

        let e = sparql_format(&q(&[("form", "jsonld")]), &HeaderMap::new(), None).unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
        assert!(e.message.contains("form"), "{}", e.message);

        let e = sparql_format(&q(&[("token", "x")]), &HeaderMap::new(), None).unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
        assert!(e.hint.contains("Authorization"), "{}", e.hint);
    }

    /// AC-011：JSON-LD 的响应**没有** `rows`（有也不会是行的形状）。
    ///
    /// 这里走的是「不连库」的夹具：执行必然失败（500），所以只验**参数解析**这一层。
    /// ⛔ 正文层面的判据**不在本文件**（也不需要连库）——它在 `query::jsonld` 的两个单测里：
    /// `meta_text_is_the_rows_receipt_minus_columns_and_rows`（钉住正文里没有 `"rows":[`
    /// 与 `"columns":[`）与 `meta_carries_the_same_warnings_and_coverage_as_the_rows_receipt`
    /// （钉住顶层没有 `rows` 键）。三入口的真库实测只写在交付报告里，⛔ 不在这里引用仓库外的文件。
    #[tokio::test]
    async fn a_bogus_format_is_rejected_before_any_query_runs() {
        let (status, _, body) = call(
            build_router(test_service()),
            post(
                "/api/sparql?format=bogus",
                Some(TOKEN_A),
                r#"{"query":"SELECT ?s WHERE { ?s a :customers }"}"#,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["code"], "BAD_REQUEST");
        assert!(v["hint"].as_str().unwrap().contains("jsonld"), "{body}");
    }

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-155 P2 · Studio 挂在查询服务上（AC-004 / AC-008 / AC-008b / AC-008c）
    // ══════════════════════════════════════════════════════════

    fn any_method(method: &str, uri: &str, token: Option<&str>) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        b.body(Body::empty()).unwrap()
    }

    /// ⭐ AC-004 · 三条路由都 200，`Content-Type` 分别是 html / css / js，且**免口令**。
    ///
    /// 免口令是 ADR-010-A S2 的形状：浏览器导航带不了 `Authorization`，所以壳必须匿名可拿；
    /// 壳里零数据（下一条用例）是它的代价条款。
    /// 顺带钉住三个安全响应头 —— 「照 console 的做法」不是形容词，是这三行。
    #[tokio::test]
    async fn ac004_studio_assets_are_served_with_the_right_content_types_without_a_token() {
        let cases = [
            ("/studio", "text/html; charset=utf-8"),
            ("/studio/", "text/html; charset=utf-8"),
            ("/studio/studio.css", "text/css; charset=utf-8"),
            ("/studio/studio.js", "application/javascript; charset=utf-8"),
        ];
        for (uri, want_type) in cases {
            let (status, headers, body) = call(build_router(test_service()), get(uri, None)).await;
            assert_eq!(status, StatusCode::OK, "{uri} 应当免口令可拿");
            assert_eq!(
                headers[header::CONTENT_TYPE], want_type,
                "{uri} 的 Content-Type 不对"
            );
            assert!(!body.is_empty(), "{uri} 的正文是空的");
            assert_eq!(headers["x-content-type-options"], "nosniff", "{uri}");
            assert_eq!(headers["referrer-policy"], "no-referrer", "{uri}");
            let csp = headers["content-security-policy"].to_str().unwrap();
            for needle in [
                "default-src 'self'",
                "script-src 'self'",
                "style-src 'self'",
                "connect-src 'self'",
            ] {
                assert!(csp.contains(needle), "{uri} 的 CSP 缺 {needle:?}：{csp}");
            }
            assert!(!csp.contains("unsafe-inline"), "{uri} 的 CSP 放开了内联：{csp}");
        }
    }

    /// ⭐ AC-008（S1）· `/studio` 只接受 GET：写类请求一律 **405**（三个资源都如此）。
    ///
    /// 405 由 axum 的 `MethodRouter` 给（路由只注册了 `get`），⛔ 没有第二个判定点。
    /// 变异（红）：把某条路由写成 `get(h).post(h)` ⇒ 本用例红；恢复复绿。
    #[tokio::test]
    async fn ac008_studio_is_get_only() {
        for uri in ["/studio", "/studio/studio.css", "/studio/studio.js"] {
            for method in ["POST", "PUT", "DELETE"] {
                let (status, headers, body) =
                    call(build_router(test_service()), any_method(method, uri, Some(TOKEN_A))).await;
                assert_eq!(
                    status,
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{method} {uri} 应当是 405（页面没有任何写能力），实际 {status}：{body}"
                );
                // 带上 token 也一样 405：这不是鉴权问题，是**方法**问题。
                // ⚠️ axum 的 `get()` 同时登记 HEAD ⇒ Allow 是 `GET,HEAD`；写类方法一个都不许有。
                let allow = headers[header::ALLOW].to_str().unwrap_or("").to_string();
                assert!(allow.contains("GET"), "{method} {uri} 的 Allow 里应当有 GET：{allow}");
                for forbidden in ["POST", "PUT", "DELETE", "PATCH"] {
                    assert!(
                        !allow.contains(forbidden),
                        "{method} {uri} 的 Allow 里出现了 {forbidden}：{allow}"
                    );
                }
            }
        }
    }

    /// ⭐ AC-008b（S2）· 壳里**零数据**：打真路由拿到的 HTML 里五个真实类名 + 快照变量 = 0。
    ///
    /// 变异（红）：往 `src/studio/index.html` 插一句 `cvm_instance` ⇒ 红；撤掉复绿。
    #[tokio::test]
    async fn ac008b_the_served_shell_carries_no_ontology_data() {
        let (status, _, html) = call(build_router(test_service()), get("/studio", None)).await;
        assert_eq!(status, StatusCode::OK);
        for needle in [
            "cvm_instance",
            "cbs_disk",
            "vpc",
            "subnet",
            "security_group",
            "ONTOK_SNAPSHOT",
        ] {
            assert_eq!(
                html.matches(needle).count(),
                0,
                "GET /studio 的正文里出现了 {needle:?} —— 页面壳必须零数据（AC-008b）"
            );
        }
        // 反空转：壳里**该**有的东西在（否则「零命中」可能只是拿到了一段错误文案）。
        assert!(html.contains("/studio/studio.js"), "壳里没有脚本引用");
        assert!(html.contains("/studio/studio.css"), "壳里没有样式引用");
    }

    /// ⭐ AC-008c（S4 / S6）· 挂上 Studio **不改变** `/api/*` 的鉴权：无口令仍是 401，
    /// 且 401 正文里不出现任何本体数据（类名一个都不许漏）。
    ///
    /// 变异（红）：把 `/studio` 加进 `needs_auth` 的豁免列表时顺手放宽 `/api` ⇒ 红；
    /// 在 401 正文里塞一句「可用类：customers」⇒ 红。
    #[tokio::test]
    async fn ac008c_without_a_token_the_api_still_401s_and_leaks_nothing() {
        for uri in ["/api/catalog", "/api/describe?name=:customers"] {
            let (status, _, body) = call(build_router(test_service()), get(uri, None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri} 无口令应当 401");
            for needle in ["customers", "orders", "id", "tenant"] {
                assert!(
                    !body.contains(needle),
                    "{uri} 的 401 正文里出现了 {needle:?}（S4：401 不泄漏数据）：{body}"
                );
            }
        }
        // 同一批端点在**没有** Studio 路由时也是这个行为（对照）：带口令就该 200。
        let (status, _, body) =
            call(build_router(test_service()), get("/api/catalog", Some(TOKEN_A))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("customers"), "带口令时目录里该有类：{body}");
    }
}
