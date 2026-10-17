//! REQ-093 · 只读图 HTTP 服务（`GET` 只有这三个出口，其余一律 405）。
//!
//! # 为什么是 axum
//!
//! 选型不是随手挑的，四条理由：
//!
//! 1. **同栈**。本 crate 的运行时已经是 tokio —— `main.rs` 的
//!    `#[tokio::main]`、sqlx 的 `runtime-tokio-rustls` 都是它。
//!    axum 跑在 tokio 上，不需要第二个 reactor。
//! 2. **不引第二套 TLS**。axum 纯 Rust，不碰 OpenSSL 系统库；
//!    本服务只绑 `127.0.0.1` 走明文 HTTP，所以 TLS 栈一个都不需要。
//! 3. **extractor 让「只读」能被机械核对**。路由 `.route(path, get(h))`
//!    只声明了 GET，axum 自动对同路径的其它方法回 **405**
//!    —— 「不接受变更」这件事落在**类型**上，不是靠我记得写判断。
//! 4. **可测**。`Router` 能直接 `oneshot`，不必真起 socket，
//!    405/401 都能在进程内断言。
//!
//! 落选：actix-web（自带 actix-rt，等于在 tokio 旁边再养一个 reactor）；
//! warp（过滤器组合读起来比 extractor 难，评审成本高）；
//! 裸 hyper（路由/查询解析/鉴权全手写，405 与 401 两处正是最容易写漏的）。
//!
//! # 身份门（最小版，硬要求）
//!
//! 令牌从环境变量 [`crate::config::VIEW_TOKEN_ENV`] 读，
//! 请求经 `Authorization: Bearer <令牌>` 携带。
//!
//! - ⛔ **没有开发模式开关**。缺环境变量 ⇒ 拒绝启动；令牌不符 ⇒ 401。
//! - ⛔ 令牌**不出现在**任何响应体、页面源码、日志、错误信息里。
//!
//! # 只读的三层
//!
//! 1. 路由层：只注册 GET；
//! 2. 接线层：本模块不引用任何写本体的函数；
//! 3. 存储层：只调 `get_by_name` / `version_by_seq` / `get_snapshot`。

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;


use crate::db::project::PgProjectStore;
use crate::db::snapshot_store::PgStore;
use crate::domain::view::{CustomerView, EntityView};

/// 服务状态。启动时**一次性**载入并冻结，运行时不再查库。
#[derive(Clone)]
pub struct AppState {
    /// 节点标识 → 该对象的客户视图（`/api/node` 用）。
    nodes: Arc<BTreeMap<String, EntityView>>,
    graph: Arc<crate::domain::graph::GraphProjection>,
    project: Arc<str>,
    version: i32,
    token: Arc<str>,
}

/// 起服务所需的参数。
pub struct ServeArgs {
    pub project: String,
    pub version: i32,
    /// 必须落在回环地址上 —— 见 `serve` 里的闸门。
    pub bind: SocketAddr,
}

/// 载入一版**已冻结**的客户视图。
///
/// 冻结闸门与 `mcp::prepare` 同一条（fail-closed）：**未冻结不是降级服务，
/// 是拒绝**。规格要求客户看的必须是定稿之后的东西。
async fn load_frozen_view(
    pool: &sqlx::PgPool,
    project: &str,
    version: i32,
) -> Result<CustomerView, ApiError> {
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());

    let p = ps
        .get_by_name(project)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NoProject(project.to_string()))?;

    let v = ps
        .version_by_seq(p.id, &p.name, version)
        .await
        .map_err(|_| ApiError::NoVersion(project.to_string(), version))?;

    if v.frozen_at.is_none() {
        return Err(ApiError::NotFrozen(project.to_string(), version));
    }

    let snap = store
        .get_snapshot(v.id)
        .await
        .map_err(|e| ApiError::Store(e.to_string()))?
        .ok_or_else(|| ApiError::NoSnapshot(project.to_string(), version))?;

    Ok(crate::domain::view::project(&crate::cli::snapshot_to_view(
        &snap,
    )))
}

/// 对外能给出的四种「拿不到」。
///
/// ⚠️ **今天它们全部发生在启动阶段**（[`load_frozen_view`] 由 [`serve`] 调用）：
/// 服务只绑一版已冻结的内容，绑不上就**拒绝启动**（与 `mcp::prepare` 同一条）。
/// 请求阶段的 4xx 走 [`check_scope`]，覆盖「缺参数 / 指名要别的一版」。
/// 保留 `status()` 与 `IntoResponse` 是为了让「哪种情况该回哪个码」有单一定义，
/// 并由 `api_error_maps_to_the_intended_status` 钉住 —— 不是留着备用的死代码。
#[derive(Debug)]
enum ApiError {
    NoProject(String),
    NoVersion(String, i32),
    NotFrozen(String, i32),
    NoSnapshot(String, i32),
    /// 读库本身失败。**只留一句话**，细节进 stderr ——
    /// 错误正文会直接送到客户端，库连接串之类的东西不该从那里出去。
    Store(String),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::NoProject(_) | ApiError::NoVersion(..) | ApiError::NoSnapshot(..) => {
                StatusCode::NOT_FOUND
            }
            // 未冻结 = 状态冲突，不是「找不到」。
            ApiError::NotFrozen(..) => StatusCode::CONFLICT,
            ApiError::Store(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 人话错误文案。
    ///
    /// ⚠️ 一律过 `safe_error`：项目名是**调用方给的自由文本**，
    /// 原样回显就等于用错误响应把守门词送出去（与
    /// `mcp/tools.rs::safe_error` 同一条理由）。
    fn message(&self) -> String {
        let raw = match self {
            ApiError::NoProject(p) => format!("没有叫「{p}」的项目。"),
            ApiError::NoVersion(p, v) => format!("项目「{p}」没有第 {v} 版。"),
            ApiError::NotFrozen(p, v) => format!(
                "项目「{p}」的第 {v} 版还没定稿，暂不给人看。定稿之后再来。",
            ),
            ApiError::NoSnapshot(p, v) => format!("项目「{p}」的第 {v} 版没有可看的内容。"),
            ApiError::Store(e) => {
                eprintln!("✗ 只读图服务读库失败：{e}");
                "读取时出了点问题，请联系维护者。".to_string()
            }
        };
        crate::mcp::tools::safe_error(&raw)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let msg = self.message();
        (self.status(), Json(json!({ "error": msg }))).into_response()
    }
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

/// 过守门器再发出去。
///
/// ⛔ 守门器**复用 `mcp::tools::guard` 本身** —— 不是抄一份判据。
/// 抄一份就会漂：MCP 那条路放过的东西，这条路可能挡住（或反过来）。
/// 命中时 `guard` 把具体词条写 stderr、返回不含词条的通用文案，
/// 所以守门词不会从错误响应里漏出去。
fn guarded<T: serde::Serialize>(value: &T) -> Result<Json<serde_json::Value>, Response> {
    let v = serde_json::to_value(value)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &format!("生成响应时出错：{e}")))?;
    match crate::mcp::tools::guard(&v) {
        Ok(text) => Ok(Json(serde_json::from_str(&text).unwrap_or(json!({})))),
        Err(msg) => {
            // `guard` 已经把命中的词条打到 stderr 了；这里只说「哪个出口」。
            eprintln!("✗ 只读图服务守门器命中建模术语，已拒绝出图。");
            Err(err(StatusCode::INTERNAL_SERVER_ERROR, &msg))
        }
    }
}

/// 校验 query 里的 project / version，并确认就是本服务绑定的这一版。
///
/// **为什么要求 query 仍然带 project/version**（明明启动时已经定死了）：
/// 规格把「缺参数 ⇒ 4xx」列成验收项；而**校验一个必须带上、
/// 且必须等于绑定值的参数**，既满足了那一条，又不会把只读服务的
/// 读取面从「一版」扩成「所有版」。
fn check_scope(
    q: &BTreeMap<String, String>,
    state: &AppState,
) -> Result<(), Response> {
    let project = q.get("project").map(String::as_str).unwrap_or_default();
    if project.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "要说明看哪个项目：请带上 project 参数。",
        ));
    }
    let raw = q.get("version").map(String::as_str).unwrap_or_default();
    if raw.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "要说明看哪一版：请带上 version 参数。",
        ));
    }
    let version: i32 = raw.parse().map_err(|_| {
        err(
            StatusCode::BAD_REQUEST,
            "version 要写一个数字（比如 2）。",
        )
    })?;
    if project != state.project.as_ref() || version != state.version {
        return Err(err(
            StatusCode::BAD_REQUEST,
            &format!(
                "本服务只对外提供 {} 第 {} 版这一份内容，别的一概不给。",
                state.project, state.version
            ),
        ));
    }
    Ok(())
}

async fn get_graph(
    State(state): State<AppState>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Result<Json<serde_json::Value>, Response> {
    check_scope(&q, &state)?;
    guarded(&*state.graph)
}

async fn get_node(
    State(state): State<AppState>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Result<Json<serde_json::Value>, Response> {
    check_scope(&q, &state)?;
    let id = q.get("id").map(String::as_str).unwrap_or_default();
    if id.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "要看哪个对象？请带上 id 参数。",
        ));
    }
    let node = state.nodes.get(id).ok_or_else(|| {
        err(
            StatusCode::NOT_FOUND,
            "图里没有这个对象，刷新一下页面就好。",
        )
    })?;
    guarded(node)
}

/// 首页。
///
/// # ⚠️ REQ-106 · 项目名过闸在**渲染之前**（AC-003）
///
/// REQ-106 之前这里是 `Html(index_html(&state.project, state.version))`，
/// 一条**零闸**的路：项目名原样进 JS 字面量，既不过 `mcp::tools::guard`，
/// 也不转义。同一个出口家族里，`/api/graph` `/api/node` 走 [`guarded`]，
/// MCP 侧走 `mcp::tools::guard` —— **只有首页没有**。
///
/// 现在判据在 [`crate::server::page::index_html`] 里，与其它出口**同一道**
/// （`mcp::tools::guard` 本身，不是抄一份），且**转义**也在那里。
/// 本函数只负责把 `Err` 变成**非 200**，⛔ 不自己再判一次。
///
/// 状态码取 [`StatusCode::INTERNAL_SERVER_ERROR`]：与 [`guarded`] 命中时同款 ——
///
/// 项目名是**服务端绑定时的启动参数**（`--project`），不是请求方能传的参数，
/// 所以它不合格**不是客户端的错**。返回 4xx 会把运维的错说成访客的错。
///
/// ⛔ 错误正文用 `guard` 的 `Err` 文案，**不要再拼词条**：`guard` 的 `Err`
/// 按设计就不含任何建模术语（命中词只进 stderr）。
async fn index(State(state): State<AppState>) -> Result<Html<String>, Response> {
    match crate::server::page::index_html(&state.project, state.version) {
        Ok(html) => Ok(Html(html)),
        Err(msg) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, &msg)),
    }
}

/// 只读服务对「你想改点什么」的**唯一**回答。
///
/// ⚠️ 这段文案与 `Allow` 头**只在这里生成一次**：axum 对已注册路径的错方法
/// 回的是**空 body** 的 405，而未注册路径走 [`fallback`] —— 此前前者空、后者有话，
/// 同一个 405 两种说法。`normalize_405` 把两者**都**收口到这里。
///
/// 口径：**参数错误与路径错误都必须说话，不许静默**。
/// 「安静地什么都不返回」是最贵的失败形态 —— 用的人只会以为服务坏了。
fn read_only_not_allowed() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "GET, HEAD")],
        Json(json!({
            "error": "这里只读：只能看，没有任何可以改动的地方。"
        })),
    )
        .into_response()
}

/// 把**每一个** 405 的正文与 `Allow` 头统一成 [`read_only_not_allowed`]。
///
/// 为什么用响应映射而不是 `method_not_allowed_fallback`：后者是 axum **0.8**
/// 才有的 API，本 crate 锁在 0.7（`method-not-allowed-fallback` 也不在 0.7 的
/// feature 表里）。升级 axum 去换一个响应改写的活，不划算。
///
/// ⛔ 这层**必须排在路由之后**（用 `.layer()` 加在已建好的 Router 上），
/// 否则它改的是「还没路由」的状态码，等于什么都没做。
async fn normalize_405(res: Response) -> Response {
    if res.status() != StatusCode::METHOD_NOT_ALLOWED {
        return res;
    }
    read_only_not_allowed()
}

/// 未注册路径。GET ⇒ 404；任何变更动词 ⇒ 405（**不是** 404）——
/// 只读服务对「你想改点什么」的正确回答是「这里只读」。
/// 405 的正文由 [`normalize_405`] 统一补上，这里只给状态码。
async fn fallback(method: Method) -> Response {
    if method == Method::GET {
        return err(StatusCode::NOT_FOUND, "没有这个页面。");
    }
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

/// 身份门。**没有开关能绕过它**。
async fn require_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .trim();

    if !token_matches(presented, &state.token) {
        // ⛔ 响应里只说「没通过」，**不回显令牌、不说哪里错了** ——
        // 401 的正文若带上期望值，等于把令牌送出去。
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            Json(json!({
                "error": "没有可用的访问口令，拿不到内容。"
            })),
        )
            .into_response();
    }
    next.run(req).await
}

/// 定长比较，避免按字节早退把令牌**前缀**猜出来。
///
/// ⚠️ 已知边界：长度不同直接返回 false，所以**长度是可见的**。
/// 令牌由运维自己设定且不经网络传输，风险可接受；
/// 为此再引一个 `subtle` 依赖不值得（记为已知限制，不写成「已封堵」）。
fn token_matches(presented: &str, expected: &str) -> bool {
    let a = presented.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 组装路由。**只有 GET。**
///
/// `.layer(map_response(normalize_405))` 是 P1 修复的另一半：已注册路径的
/// 405 由 axum 自动产生（正文空），未注册路径的由 [`fallback`] 产生 ——
/// 响应映射把两者**收口成同一句人话**（见 [`normalize_405`]）。
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/graph", get(get_graph))
        .route("/api/node", get(get_node))
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .layer(middleware::map_response(normalize_405))
        .with_state(state)
}

/// 载入 → 起服务。**只绑回环地址**。
/// 由客户视图构造服务端状态。**生产与测试走的是这一个函数。**
///
/// # 为什么必须是**一个**函数（这正是 P1 防护的本体）
///
/// 上一轮我写了一条自称「防 P1 回归」的测试，但它把这里的接线**逐字抄了一遍**
/// 建自己的 `AppState`。QA 的变异 M1 证明：把 bug 放回本函数，**两次运行全绿**。
/// —— 纪律 H 的第二次：**忠实的重复实现不只发生在业务代码里，测试夹具同样会**；
/// 夹具抄一份生产接线，测试就只验证了夹具自己。
/// 而且一条自称防回归却不防的测试**比没有更坏**：它让人误以为有防护。
///
/// ⇒ 所以 [`serve`] 与 `server::http::tests` **都调这里**，没有第二份接线。
/// 把 bug 放回本函数 → 生产坏、**测试同时红**。
pub fn state_from_view(
    view: &CustomerView,
    project: &str,
    version: i32,
    token: &str,
) -> AppState {
    // ── REQ-104 · 出口层属性级剔除 ──
    //
    // 放在 `state_from_view` 里而不是 `serve()` 里，是因为这个文件里
    // **`serve` 与 `server::http::tests` 都调它，没有第二份接线** ——
    // 放这里，生产接线与测试接线**不可能分叉**。
    //
    // ⚠️ 剔除发生在**守门器（`guarded`）之前**，守门器**一行没改**：
    // `demo3` 第 2 版有 3 个节点因为属性文案带 `pk` / `key` 被逐节点判拒发（HTTP 500），
    // 剔除后这 4 条属性不再进节点视图 ⇒ 91 个节点全部打得开；
    // 而真·建模术语剔不掉，照旧到达守门器、照旧拒发。
    let filtered = crate::domain::property_filter::filter_view(view);
    let report = filtered.report();
    if !report.is_empty() {
        eprintln!("{report}");
    }
    let view = &filtered.view;

    let graph = crate::domain::graph::project(view);

    // 节点索引的键**直接取自 `graph.nodes[i].key`** ——
    // 也就是 `/api/graph` 序列化给客户的**同一个 key**，不是另算一份。
    //
    // ⚠️ 这里曾出过 P1：按**展示名**建表时重名会**后者覆盖前者** ⇒ 落到最后一个，
    // 而图上 `attribute_count` 取的是**第一个** ⇒ 客户点开看到 9 项、图上写 10 项，
    // 另一个节点直接 404。user view 的全部价值承诺就是「点开看」，
    // 这不是边缘情况的美观问题，是核心场景当场失效。
    //
    // ⛔ 修法**不是**「两边都改成第一个」（那仍靠人记得同步），
    // 而是让 `/api/graph` 与 `/api/node` 的键**取自同一份 nodes 数组**。
    let mut nodes: BTreeMap<String, EntityView> = BTreeMap::new();
    for (n, o) in graph.nodes.iter().zip(view.objects.iter()) {
        nodes.insert(n.key.clone(), o.clone());
    }

    // ⚠️ 这是**普通 `assert!`，不是 `debug_assert!`** ——
    // `debug_assert!` 在 **release 构建里被编译掉**，等于 release 下没有这层兜底
    // （QA 变异 M1 实测：release 起服务直接 panic，而测试毫无反应）。
    // `zip` 会**静默截断**：长度不等时节点就少 ⇒ 一定有节点打不开（404）。
    // 这属于编程错误，不是可降级的运行状况 ⇒ 响亮地炸，别悄悄少节点。
    assert_eq!(
        nodes.len(),
        graph.nodes.len(),
        "节点索引与图的节点数必须一致 —— 否则 /api/node 会与 /api/graph 对不上"
    );

    AppState {
        nodes: Arc::new(nodes),
        graph: Arc::new(graph),
        project: Arc::from(project),
        version,
        token: Arc::from(token),
    }
}

pub async fn serve(pool: &sqlx::PgPool, args: ServeArgs) -> anyhow::Result<()> {
    // ⛔ 只绑回环：一个「给人看的本体图」若绑到 0.0.0.0，
    // 就等于把客户数据挂到网上等人试令牌。宁可起不来。
    if !args.bind.ip().is_loopback() {
        anyhow::bail!(
            "只允许绑回环地址（如 127.0.0.1:8787），收到 {:?}。\
             这份内容是对外的，请留在本机并用访问口令把门。",
            args.bind.ip()
        );
    }

    // 令牌只有环境变量一个来源；缺了直接拒绝启动，不给默认值。
    let token = crate::config::require_env(crate::config::VIEW_TOKEN_ENV)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if token.trim().len() < 8 {
        anyhow::bail!(
            "访问口令太短（至少 8 个字符）：一个 4 位口令在客户数据前面等于没设。"
        );
    }

    let view = load_frozen_view(pool, &args.project, args.version)
        .await
        .map_err(|e| anyhow::anyhow!("只读图服务拒绝启动：{}", e.message()))?;

    // ⚠️ 接线**只有这一处**：`state_from_view` 同时被 `serve()`（这里）
    // 与 `server::http::tests` 调用。测试**不得**自己另搭一份 AppState ——
    // 上一轮就是这么写的，结果测试完全不守护生产（纪律 H，见该函数文档）。
    let state = state_from_view(&view, &args.project, args.version, &token);

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    eprintln!(
        "· 只读图服务就绪：项目 {} 第 {} 版（只读 · 需访问口令）",
        args.project, args.version
    );
    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("只读图服务退出：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt; // `Router::oneshot`

    const TOKEN: &str = "correct-horse-battery";

    fn st() -> AppState {
        AppState {
            nodes: Arc::new(BTreeMap::new()),
            graph: Arc::new(crate::domain::graph::GraphProjection {
                nodes: vec![],
                edges: vec![],
            }),
            project: Arc::from("demo3"),
            version: 2,
            token: Arc::from(TOKEN),
        }
    }

    /// 一份**带建模术语**的图 —— 用来证明守门器在 HTTP 路径上真的会拦。
    fn st_poisoned() -> AppState {
        AppState {
            graph: Arc::new(crate::domain::graph::GraphProjection {
                nodes: vec![crate::domain::graph::Node {
                    key: "订单".into(),
                    // 「外键」在守门词表里 —— 正常数据里绝不该出现
                    label: "外键约束表".into(),
                    attribute_count: 1,
                    parent: None,
                }],
                edges: vec![],
            }),
            ..st()
        }
    }

    /// 打一发请求。`oneshot` 的错误类型是 `Infallible`，所以这里 `expect`
    /// 不是「掩盖失败」—— 那条错误分支根本不存在。
    ///
    /// 收 `AppState` 而不是 `Router`：路由由本函数现搭，
    /// 省得每个测试都记得先 `build_router`，也免得 router 被复用。
    async fn call(state: AppState, method: &str, uri: &str, token: Option<&str>) -> Response {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        build_router(state)
            .oneshot(b.body(Body::empty()).unwrap())
            .await
            .expect("oneshot 的错误类型是 Infallible")
    }

    async fn call_status(
        state: AppState,
        method: &str,
        uri: &str,
        token: Option<&str>,
    ) -> StatusCode {
        call(state, method, uri, token).await.status()
    }

    async fn body_of(r: Response) -> String {
        String::from_utf8_lossy(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap())
            .to_string()
    }

    // ── AC-004 · 无身份拿不到数据 ─────────────────────────

    #[tokio::test]
    async fn without_token_every_endpoint_is_401() {
        for uri in [
            "/",
            "/api/graph?project=demo3&version=2",
            "/api/node?project=demo3&version=2&id=订单",
        ] {
            let s = call_status(st(), "GET", uri, None).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "{uri} 应当 401");
        }
    }

    #[tokio::test]
    async fn wrong_token_is_401() {
        for bad in ["", "nope", "correct-horse", "CORRECT-HORSE-BATTERY"] {
            let s = call_status(st(), "GET", "/api/graph?project=demo3&version=2", Some(bad)).await;
            assert_eq!(s, StatusCode::UNAUTHORIZED, "错口令 {bad:?} 应当 401");
        }
    }

    /// ⛔ 令牌不许出现在任何**回给客户端的东西**里。
    ///
    /// 三处一起查：401 正文、成功页面源码、成功响应正文。
    #[tokio::test]
    async fn the_token_never_appears_in_anything_sent_back() {
        let unauthorized = body_of(
            call(st(), "GET", "/api/graph?project=demo3&version=2", Some("wrong")).await,
        )
        .await;
        assert!(
            !unauthorized.contains(TOKEN),
            "401 正文里漏了令牌：{unauthorized}"
        );

        let page = body_of(call(st(), "GET", "/", Some(TOKEN)).await).await;
        assert!(!page.contains(TOKEN), "页面源码里漏了令牌");

        let ok = body_of(
            call(st(), "GET", "/api/graph?project=demo3&version=2", Some(TOKEN)).await,
        )
        .await;
        assert!(!ok.contains(TOKEN), "响应正文里漏了令牌：{ok}");
    }

    // ── AC-003 · 只读：变更动词一律 405 ───────────────────

    /// ⚠️ `Allow` 里有 **HEAD**：axum 给每个 GET 路由自动配 HEAD，
    /// 而 HEAD 是**只读**方法（与 GET 同一份响应，只是不带正文）——
    /// 把它算进「只读」是对的，不是漏网。正文一律查不到数据。
    ///
    /// ⚠️ 正文**必须有人话**：此前已注册路径的 405 是 axum 的**空 body**，
    /// 而未注册路径才说话 —— 同一个 405 两种说法。已由 `every_405_carries_a_human_message` 钉住。
    #[tokio::test]
    async fn change_verbs_are_405_on_registered_api_paths() {
        for m in ["POST", "PUT", "PATCH", "DELETE"] {
            for uri in ["/api/graph", "/api/node"] {
                let r = call(st(), m, uri, Some(TOKEN)).await;
                assert_eq!(
                    r.status(),
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{m} {uri} 应当 405"
                );
                let allow = r
                    .headers()
                    .get(header::ALLOW)
                    .map(|v| v.to_str().unwrap().to_string())
                    .unwrap_or_default();
                assert!(
                    allow.contains("GET"),
                    "405 应当说明只接受 GET，实际：{allow}"
                );
                for verb in ["POST", "PUT", "PATCH", "DELETE"] {
                    assert!(
                        !allow.contains(verb),
                        "405 的 Allow 里不该出现变更动词 {verb}：{allow}"
                    );
                }
            }
        }
    }

    /// ⛔ **所有** 405 都要有人话 —— 已注册路径与未注册路径**同一句**。
    ///
    /// 「安静地什么都不返回」是最贵的失败形态：用的人只会以为服务坏了，
    /// 而这里其实是「只读」这个**设计**在说话。
    #[tokio::test]
    async fn every_405_carries_a_human_message() {
        let cases = [
            ("/api/graph", "已注册"),
            ("/api/node", "已注册"),
            ("/", "已注册"),
            ("/api/apply", "未注册"),
        ];
        for (uri, kind) in cases {
            for m in ["POST", "PUT", "PATCH", "DELETE"] {
                let r = call(st(), m, uri, Some(TOKEN)).await;
                assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED, "{m} {uri}");
                let b = body_of(r).await;
                assert!(
                    !b.trim().is_empty(),
                    "{kind}路径 {m} {uri} 的 405 正文是空的 —— 参数/路径错误必须说话"
                );
                assert!(
                    b.contains("只读"),
                    "{kind}路径 {m} {uri} 的 405 没说明这是只读：{b}"
                );
            }
        }
    }

    /// HEAD 也不给正文 —— 它是只读的另一种问法。
    #[tokio::test]
    async fn head_is_allowed_but_carries_no_body() {
        let r = call(st(), "HEAD", "/api/graph?project=demo3&version=2", Some(TOKEN)).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(
            body_of(r).await.is_empty(),
            "HEAD 的正文必须是空的"
        );
    }

    /// 已注册路径之外的 `/api/*` 也一样 405 —— 只读服务对「你想改点什么」
    /// 的正确回答是「这里只读」，不是「没这个页面」。
    #[tokio::test]
    async fn change_verbs_on_unknown_api_paths_are_405_too() {
        for m in ["POST", "PUT", "PATCH", "DELETE"] {
            let s = call_status(st(), m, "/api/apply", Some(TOKEN)).await;
            assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED, "{m} /api/apply 应当 405");
        }
    }

    #[tokio::test]
    async fn get_on_unknown_path_is_404() {
        let s = call_status(st(), "GET", "/api/nope", Some(TOKEN)).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    /// ⛔ **身份门必须排在路由前面**。
    ///
    /// 否则没口令的人可以用「405 还是 404」把**哪些路径存在**一个个试出来 ——
    /// 这本身是一条信息泄露。正确顺序：无口令一律 401，
    /// 连「这个接口存不存在」都不告诉它。
    #[tokio::test]
    async fn unauthenticated_probes_cannot_discover_which_paths_exist() {
        for (m, uri) in [
            ("GET", "/api/graph"),
            ("GET", "/api/definitely-not-a-route"),
            ("POST", "/api/apply"),
            ("DELETE", "/"),
        ] {
            let s = call_status(st(), m, uri, None).await;
            assert_eq!(
                s,
                StatusCode::UNAUTHORIZED,
                "{m} {uri} 无口令时应当 401（不该泄露该路径是否存在）"
            );
        }
    }

    // ── REQ-106 · AC-003 / AC-004：首页的项目名注入面 ─────────────
    //
    // ⚠️ 这一组**刻意不碰数据库**：项目名是 `serve()` 绑定的启动参数，
    // 闸门与转义都发生在**渲染之前**，所以用 `state_from_view`（生产接线）
    // 造一个空视图就够判据用 —— 与共享集群的连接占用完全解耦。

    /// 项目名**故意带注入串**时的服务端状态。走**生产接线**
    /// [`state_from_view`]，⛔ 不手搓 `AppState`（REQ-093 P1 防护的同一条：
    /// 生产接线与测试接线不可能分叉）。
    fn st_named(project: &str) -> AppState {
        crate::server::http::state_from_view(
            &CustomerView {
                objects: vec![],
                relations: vec![],
            },
            project,
            2,
            TOKEN,
        )
    }

    /// 页面里 `project: <字面量>, version:` 那一段的字面量原文。
    ///
    /// 断言它**能被 `serde_json` 原样解析回项目名** —— 这是「JS 结构没被破坏」
    /// 最强的一条：解析失败就是字面量坏了；解析回来不等于原名就是转义错了。
    fn project_literal(html: &str) -> &str {
        const HEAD: &str = "var who = { project: ";
        const TAIL: &str = ", version: ";
        let start = html
            .find(HEAD)
            .unwrap_or_else(|| panic!("页面里没有 who 的定义"))
            + HEAD.len();
        let end = start
            + html[start..]
                .find(TAIL)
                .unwrap_or_else(|| panic!("who 的定义被截断了 —— 字面量没闭合"));
        &html[start..end]
    }

    /// ⛔ **注入面**：项目名含 `"` / `\` / `</script>` 时，
    /// 页面必须仍是**合法 HTML**，JS 结构不被破坏。
    ///
    /// 覆盖两种坏法：
    /// - **JS 层**（`"` `\` `;`）⇒ 改写 `var who = {...}` 的结构；
    /// - **HTML 层**（`</script>` `<!--`）⇒ HTML 解析器先于 JS 解析器
    ///   结束脚本块，`serde_json` 转义救不了它（正斜杠 JSON 不转义）。
    #[tokio::test]
    async fn a_hostile_project_name_cannot_break_the_page() {
        let hostiles = [
            r#"has"quote"#,
            r"has\backslash",
            r#"x"; fetch("//evil.example"); var y="#,
            "</script><script>alert(1)</script>",
            "<!--<script>",
            "换行\n与\r回车",
            " 控制符",
        ];
        // 对照组：干净页面，用来比「结构标记的数量」。
        // ⛔ 没有它，上面的断言可能只是「页面变短了」也照样绿（纪律 E）。
        let clean_status = call_status(st_named("demo3"), "GET", "/", Some(TOKEN)).await;
        assert_eq!(
            clean_status,
            StatusCode::OK,
            "对照组本身必须是 200，否则下面全是空转"
        );
        let clean = body_of(call(st_named("demo3"), "GET", "/", Some(TOKEN)).await).await;

        for hostile in hostiles {
            let r = call(st_named(hostile), "GET", "/", Some(TOKEN)).await;
            assert_eq!(
                r.status(),
                StatusCode::OK,
                "项目名 {hostile:?} 不含建模术语，页面应当照常出"
            );
            let html = body_of(r).await;

            // ① 字面量能原样解析回项目名 ⇒ JS 结构完好。
            assert_eq!(
                serde_json::from_str::<String>(project_literal(&html)).unwrap_or_else(|e| {
                    panic!("项目名 {hostile:?} 的字面量解析失败：{e}\n字面量：{}", project_literal(&html))
                }),
                hostile,
                "项目名 {hostile:?} 应当被正确转义后原样还原"
            );

            // ② HTML 层结构标记的数量**逐个不变** ⇒ 没有多出/少掉脚本块。
            //    注意 `<script>` 在页面上本来就出现多次，所以比的是**数量**。
            for mark in ["<script>", "</script>", "<style>", "</html>", "<body"] {
                assert_eq!(
                    html.matches(mark).count(),
                    clean.matches(mark).count(),
                    "项目名 {hostile:?} 改变了结构标记 `{mark}` 的数量 —— 脚本块被改坏了"
                );
            }

            // ③ 注入的原串**没有**原样出现在页面里。
            assert!(
                !html.contains("</script><script>") && !html.contains("fetch(\"//evil"),
                "项目名 {hostile:?} 有一部分原样落进了页面"
            );
        }
    }

    /// ⛔ **闸门面**：项目名含建模术语 ⇒ 首页**拒绝出页面**（非 200）。
    ///
    /// 与 `instructions()` / `guarded()` 同一口径：同一份项目名在 MCP 侧
    /// 本来就过闸，只有首页没有 —— 这条测试钉住的是「现在有了」。
    #[tokio::test]
    async fn a_project_name_with_modeling_terms_is_refused_on_the_homepage() {
        for bad in ["外键", "基数", "主键", "证据"] {
            let name = format!("demo{bad}demo");
            let r = call(st_named(&name), "GET", "/", Some(TOKEN)).await;
            assert!(
                !r.status().is_success(),
                "项目名含 {bad:?} 应当被首页拒绝，实际 {:?}",
                r.status()
            );
            let b = body_of(r).await;
            assert!(
                !b.contains(bad),
                "守门词 {bad:?} 本身不许从错误响应里漏出去：{b}"
            );
        }
    }

    /// 对照组（纪律 E）：**干净的项目名必须过得去**。
    /// 没有这条，上一条全绿也可能只是「所有首页都被拒了」。
    #[tokio::test]
    async fn a_clean_project_name_still_serves_the_homepage() {
        let r = call(st_named("demo3"), "GET", "/", Some(TOKEN)).await;
        assert_eq!(r.status(), StatusCode::OK);
        let b = body_of(r).await;
        assert!(b.contains("demo3"), "页面应带上项目名");
    }

    /// 对照组（纪律 E）：没有访问口令时，首页**同样**是 401 ——
    /// 闸门不能变成一条绕过身份门的路。
    #[tokio::test]
    async fn the_homepage_gate_does_not_bypass_the_token_gate() {
        assert_eq!(
            call_status(st_named("demo外键demo"), "GET", "/", None).await,
            StatusCode::UNAUTHORIZED,
            "无口令应当先被身份门拦下"
        );
    }

    // ── AC-002 · 守门器在 HTTP 路径上同样生效 ─────────────

    /// ⛔ 图里出现建模术语 ⇒ **拒绝出图**，不给数据。
    ///
    /// 与 `cli.rs` 对 `--html` / `--json` 的处置同一条：命中就拒发，
    /// 而不是「先发出去再说」。
    #[tokio::test]
    async fn modeling_terms_in_the_graph_are_refused_over_http() {
        let r = call(st_poisoned(), "GET", "/api/graph?project=demo3&version=2", Some(TOKEN)).await;
        assert_eq!(
            r.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "命中守门器应当拒发"
        );
        let b = body_of(r).await;
        assert!(
            !b.contains("外键"),
            "守门词本身不许从错误响应里漏出去：{b}"
        );
        assert!(
            !b.contains("订单"),
            "拒绝时不得顺带把图数据发出去：{b}"
        );
    }

    /// 对照组（纪律 E）：**干净的图必须过得去**。
    /// 没有这条，上一条全绿也可能只是「所有请求都被拒了」。
    #[tokio::test]
    async fn a_clean_graph_is_served() {
        let s = call_status(st(), "GET", "/api/graph?project=demo3&version=2", Some(TOKEN)).await;
        assert_eq!(s, StatusCode::OK);
    }

    /// 守门器扫描器自检：它对已知守门词**必须**命中。
    /// 若哪天它恒返回空，上面两条绿得毫无意义。
    #[test]
    fn the_guard_scanner_is_not_vacuous() {
        let r = crate::mcp::tools::guard(&json!({ "note": "这个字段叫 key" }));
        assert!(r.is_err(), "守门器对已知建模术语必须拦下");
    }

    // ── 参数校验 · 人话 4xx ───────────────────────────────

    #[tokio::test]
    async fn missing_or_wrong_scope_is_a_4xx_with_a_human_message() {
        for (uri, want) in [
            ("/api/graph", StatusCode::BAD_REQUEST),
            ("/api/graph?project=demo3", StatusCode::BAD_REQUEST),
            ("/api/graph?version=2", StatusCode::BAD_REQUEST),
            ("/api/graph?project=demo3&version=abc", StatusCode::BAD_REQUEST),
            ("/api/graph?project=demo3&version=1", StatusCode::BAD_REQUEST),
            ("/api/graph?project=other&version=2", StatusCode::BAD_REQUEST),
        ] {
            let r = call(st(), "GET", uri, Some(TOKEN)).await;
            assert_eq!(r.status(), want, "{uri}");
            let b = body_of(r).await;
            assert!(
                b.contains("error") && !b.contains("null"),
                "{uri} 应当给一句人话错误：{b}"
            );
        }
    }

    #[tokio::test]
    async fn node_endpoint_requires_an_id_and_404s_on_unknown() {
        let s = call_status(
            st(),
            "GET",
            "/api/node?project=demo3&version=2",
            Some(TOKEN),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "缺 id 应当 400");

        let s = call_status(
            st(),
            "GET",
            "/api/node?project=demo3&version=2&id=查无此物",
            Some(TOKEN),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "未知对象应当 404");
    }

    // ── P1 · 重名对象的寻址必须两边一致 ───────────────────

    /// ⛔ **P1 的防回归**：图上每个节点都要打得开，且**属性条数等于图上标的**。
    ///
    /// 样本**刻意含两个重名对象**（属性数 3 与 1）—— 就是 `demo3` 第 2 版里
    /// 「客户账号绑定」那个形态。在**无重名**的干净数据上，这条测试永远绿，
    /// 抓不到当初那个 bug（当时两边都按名字建表，重名时后者覆盖前者）。
    ///
    /// ⛔ **本夹具调的是 [`state_from_view`] —— 与 `serve()` 同一个函数。**
    /// 上一轮它在这里**逐字抄了一遍**生产接线，于是 QA 的变异 M1 把 bug 放回
    /// `serve()` 时两次运行全绿（纪律 H 的第二次：测试夹具同样会「忠实地重复实现」）。
    /// 现在没有第二份接线可抄：改坏 `state_from_view` ⇒ 生产坏、**这里同时红**。
    fn view_with_duplicate_names() -> CustomerView {
        use crate::domain::view::{EntityView, PropertyView};
        let props = |n: usize| {
            (0..n)
                .map(|i| PropertyView {
                    name: format!("属性{i}"),
                    note: None,
                    required: false,
                    sample: None,
                })
                .collect()
        };
        CustomerView {
            objects: vec![
                EntityView {
                    name: "客户账号绑定".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: props(3),
                },
                EntityView {
                    name: "客户账号绑定".into(), // ← 同名，属性数不同
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: props(1),
                },
                EntityView {
                    name: "客户".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![],
                },
            ],
            relations: vec![],
        }
    }

    #[tokio::test]
    async fn every_node_including_duplicates_opens_with_a_matching_attribute_count() {
        // ⛔ 生产接线，不是复刻品
        let state = state_from_view(&view_with_duplicate_names(), "demo3", 2, TOKEN);
        assert_eq!(
            state.graph.nodes.len(),
            3,
            "样本必须真的含重名：图上应有 3 个节点（两个同名 + 一个客户）"
        );

        let mut checked = 0usize;
        for n in &state.graph.nodes {
            let uri = format!(
                "/api/node?project=demo3&version=2&id={}",
                urlencode(&n.key)
            );
            let r = call(state.clone(), "GET", &uri, Some(TOKEN)).await;
            assert_eq!(
                r.status(),
                StatusCode::OK,
                "图上的每个节点都要打得开，404 只读服务就是残废的：id={:?}",
                n.key
            );
            let body = body_of(r).await;
            let got = serde_json::from_str::<serde_json::Value>(&body).unwrap();
            let got_n = got["properties"].as_array().unwrap().len();
            assert_eq!(
                got_n, n.attribute_count,
                "点开后的属性条数必须等于图上标的：id={:?}",
                n.key
            );
            checked += 1;
        }
        assert_eq!(checked, 3, "必须逐个核过，不能抽样");
    }

    /// ⛔ **接线层**：同一个 `state_from_view` 产出的东西，
    /// `/api/graph` 序列化出去的 id 与 `/api/node` 认的 id 必须**逐字相同**。
    ///
    /// 上一轮这条检查是靠测试自己另建一张表做的 —— 那张表和生产的那张**可能不同**。
    /// 现在直接比对**同一份 `state`** 的两端。
    #[tokio::test]
    async fn the_graph_endpoint_and_the_node_endpoint_agree_on_every_id() {
        let state = state_from_view(&view_with_duplicate_names(), "demo3", 2, TOKEN);
        let app = build_router(state.clone());

        // ① 走真实路由，取回 /api/graph 序列化出去的 id
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/graph?project=demo3&version=2")
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let served = serde_json::from_str::<serde_json::Value>(&body_of(r).await).unwrap();
        let served_ids: Vec<String> = served["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap().to_string())
            .collect();

        // ② 逐个拿这些 id 去打 /api/node —— 一个都不许 404
        for id in &served_ids {
            let uri = format!("/api/node?project=demo3&version=2&id={}", urlencode(id));
            let r = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(&uri)
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                r.status(),
                StatusCode::OK,
                "/api/graph 给出的 id={id:?} 到了 /api/node 却打不开"
            );
        }
        assert_eq!(served_ids.len(), 3, "样本含 3 个节点（含 1 组重名）");
        assert!(served_ids.contains(&"客户账号绑定#2".to_string()));
    }

    #[test]
    fn duplicate_names_get_distinct_node_ids_ending_in_a_hash_two() {
        let g = crate::domain::graph::project(&view_with_duplicate_names());
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.key.as_str()).collect();
        assert_eq!(ids, vec!["客户账号绑定", "客户账号绑定#2", "客户"]);
        assert_eq!(g.nodes[0].attribute_count, 3);
        assert_eq!(g.nodes[1].attribute_count, 1);
    }

    /// 极简 URL 编码：只处理本测试用到的字符（中文、`#`）。
    /// 引一个依赖为一个测试不值得。
    fn urlencode(s: &str) -> String {
        let mut out = String::new();
        for b in s.as_bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(*b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    // ── 令牌比较 ──────────────────────────────────────────

    #[test]
    fn token_compare_accepts_only_the_exact_value() {
        assert!(token_matches("correct-horse", "correct-horse"));
        assert!(!token_matches("correct-hors", "correct-horse"));
        assert!(!token_matches("Correct-horse", "correct-horse"));
        assert!(!token_matches("", "correct-horse"));
        assert!(!token_matches("correct-horse ", "correct-horse"));
    }

    // ── 错误映射 · 单一定义 ───────────────────────────────

    /// 「哪种情况该回哪个码」必须有单一定义，且错误文案**自身**也要过守门器。
    ///
    /// ⚠️ 后半句才是关键：项目名是调用方给的自由文本，原样回显就等于
    /// 用错误响应把守门词送出去（`mcp/tools.rs::safe_error` 的同一条理由）。
    #[test]
    fn api_error_maps_to_the_intended_status() {
        let cases: Vec<(ApiError, StatusCode)> = vec![
            (ApiError::NoProject("demo3".into()), StatusCode::NOT_FOUND),
            (ApiError::NoVersion("demo3".into(), 7), StatusCode::NOT_FOUND),
            (ApiError::NotFrozen("demo3".into(), 1), StatusCode::CONFLICT),
            (
                ApiError::NoSnapshot("demo3".into(), 2),
                StatusCode::NOT_FOUND,
            ),
            (ApiError::Store("db down".into()), StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (e, want) in cases {
            assert_eq!(e.status(), want, "{e:?} 的状态码不对");
            let msg = e.message();
            assert!(!msg.is_empty(), "{e:?} 必须给人话，不能是空串");
        }
    }

    /// ⛔ 错误文案自身过守门器：项目名里带建模术语时不得原样回显。
    #[test]
    fn error_messages_themselves_pass_the_guard() {
        for name in ["demo3", "外键登记表", "叫 key 的项目"] {
            let e = ApiError::NoProject(name.to_string());
            let msg = e.message();
            assert!(
                crate::domain::view::scan_modeling_terms(&msg).is_empty(),
                "错误文案泄露了建模术语（项目名={name:?}）：{msg}"
            );
        }
    }
}
