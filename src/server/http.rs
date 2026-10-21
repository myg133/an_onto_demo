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
//! # ⭐ REQ-110 · 门只挂在 `/api/*` 上（页面可达性）
//!
//! 收口前 `require_token` 是**全局 layer**，连 `GET /` 这个**静态页面壳**也拦。
//! 后果是一个死循环：要看到页面得先有口令，要有口令得先看到页面
//! （浏览器导航**不可能**带 `Authorization` 头）。
//!
//! 现在：
//!
//! | 路由 | token | 内容 |
//! | --- | --- | --- |
//! | `GET /` | **免** | **静态壳，不含任何本体数据**（项目名 / 版本 / 图都不是服务端渲染进去的） |
//! | `GET /api/scope` | 必须 | 页面壳拿口令后问「看哪个项目、哪一版」 |
//! | `GET /api/graph` / `/api/node` / `/api/ontology.ttl` | 必须 | 全部数据 |
//!
//! 代价：「未登录连有哪些路径都探测不到」变成「能探测到有个服务，但拿不到任何数据」。
//! 判据**改写、没删**（`unauthenticated_probes_learn_nothing_but_the_shell_exists`），
//! 取舍写进 `BA/decisions/ADR-004-页面可达性与门禁.md`。
//!
//! ⚠️ `/api/*` 下**未注册**的路径也走 401（`api` 子路由自带 fallback，
//! 身份门排在它前面）—— `/api/` 这一层的路径探测仍然探测不到。
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
    /// ⭐ REQ-110 · **已过前处理**（`pipeline::prepare_view`）的客户视图。
    ///
    /// `/api/ontology.ttl` 用它序列化 —— 与 `/api/graph` / `/api/node`
    /// 取自**同一份**视图（不多不少），所以「页面看到的」与「导出的」一致。
    view: Arc<CustomerView>,
    project: Arc<str>,
    version: i32,
    token: Arc<str>,
    /// ⭐ REQ-110 · 快照里动作层（`ActionType`）的条数。
    ///
    /// **只**用于 TTL 文件头的有损声明（`ActionType 未导出：N 个`）——
    /// 动作内容一个字节都不导出（OWL 无对应）。与 `--version` 一起冻结，
    /// 所以 CLI 与 HTTP 的声明数字必然相同（AC-007 的逐字节比较依赖它）。
    action_type_count: usize,
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
///
/// 返回值：
/// - 第一项：客户视图；
/// - 第二项：快照里**动作层条数**（只供 TTL 文件头声明有损，不导出内容）；
/// - 第三项 ⭐ REQ-115：**已声明的概念组**（成员已换算成展示名）。
///
/// # 为什么概念组在**载入期**就取好
///
/// 与客户视图同一时刻、同一份快照取出来，之后服务运行期不再查库
/// （`AppState` 的既有约定：一次性载入并冻结）。于是「图上显示的分组」
/// 与「这份快照里人签过的分组」只能是同一批 —— 不存在运行中变组的窗口。
///
/// ⚠️ 换算只做「实体 key → 展示名」（[`crate::domain::graph::declared_groups`]）；
/// 「展示名 → 节点标识」在 [`state_from_view_with_groups`] 里随投影一起完成。
async fn load_frozen_view(
    pool: &sqlx::PgPool,
    project: &str,
    version: i32,
) -> Result<(CustomerView, usize, Vec<crate::domain::graph::DeclaredGroup>), ApiError> {
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

    let action_type_count = snap.action_types.len();
    // ⭐ REQ-115：分组与视图取自**同一份快照**。
    let groups = crate::domain::graph::declared_groups(&snap);
    Ok((
        crate::domain::view::project(&crate::cli::snapshot_to_view(&snap)),
        action_type_count,
        groups,
    ))
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
/// # ⚠️ REQ-107：本函数已并入管线，**不再**在出口层判
///
/// 收口前这里是 `guarded()`：出口自己调 `mcp::tools::guard` 决定「发不发」。
/// 现在出口只声明「我是哪条出口」（[`crate::domain::pipeline::OutletId::H2`] /
/// [`crate::domain::pipeline::OutletId::H3`]），
/// 判定在 [`crate::domain::pipeline::publish_value`] 里。
/// ⛔ 守门器本体仍然只复用 `mcp::tools::guard`，**不是**抄一份。
///
/// # 两种 `Err` 分开说
///
/// - **闸命中**：`guard` 已经把命中的词条打到 stderr 了；这里只说「哪个出口」，
///   响应正文用 `guard` 的 `Err` 文案（按设计不含任何建模术语）；
/// - **序列化失败**：那是 bug，不是数据问题 —— 文案必须说清是「生成响应时出错」，
///   不要冒充成「发布闸命中」（冒充会让运维去查数据，而问题在代码里）。
fn publish_json(
    id: crate::domain::pipeline::OutletId,
    value: Result<serde_json::Value, serde_json::Error>,
) -> Result<Json<serde_json::Value>, Response> {
    let value = value.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("生成响应时出错：{e}"),
        )
    })?;
    match crate::domain::pipeline::publish_value(id, value) {
        Ok(rendered) => Ok(Json(rendered.into_value())),
        Err(e) if e.is_leak() => {
            // `guard` 已经把命中的词条打到 stderr 了；这里只说「哪个出口」。
            eprintln!("✗ 只读图服务守门器命中建模术语，已拒绝出图。");
            Err(err(StatusCode::INTERNAL_SERVER_ERROR, e.message()))
        }
        Err(e) => {
            eprintln!("✗ 只读图服务生成响应失败：{}", e.message());
            Err(err(StatusCode::INTERNAL_SERVER_ERROR, e.message()))
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
        // ⭐ REQ-107 · AC-006（Z1）：项目名**不再裸拼**进 400 正文。
        //
        // 收口前这里是 `format!("本服务只对外提供 {} …", state.project)` 直接走 `err()`：
        // 无闸、无 `safe_error`、无转义 —— 同一个项目名在首页被拒发，
        // 在这里却能一个字不差地读回来。
        //
        // 现在走管线的**错误路径**（`pipeline::render` + `Product::ErrorText`）：
        // 命中守门词 ⇒ 整句换成通用说明；干净项目名 ⇒ 原样回显（运维要能看出绑的是哪一份）。
        //
        // ⚠️ 第 2 轮：`publish_error_text` 这个「任意 `&str` → 可写出产物」的公开工厂
        // **已删掉**（QA N6 / A11 R-C）—— 错误路径也只剩 `render` 一个入口，
        // 于是「全仓公开构造客户产物的函数」恰好只有 `render` / `publish_value` 两个。
        let rendered = crate::domain::pipeline::render(
            crate::domain::pipeline::OutletId::Z1,
            crate::domain::pipeline::Product::ErrorText(format!(
                "本服务只对外提供 {} 第 {} 版这一份内容，别的一概不给。",
                state.project, state.version
            )),
        );
        // `Product::ErrorText` 恒 `Ok`（`safe_error` 只改写、不拒绝）——
        // 这里的 `Err` 分支只是把「不可能」写成一句人话兜底，不冒充任何判定。
        let rendered = match rendered {
            Ok(r) => r,
            Err(_) => return Err(err(StatusCode::BAD_REQUEST, "请求的项目或版本不在本服务范围内。")),
        };
        return Err(err(StatusCode::BAD_REQUEST, rendered.as_str()));
    }
    Ok(())
}

async fn get_graph(
    State(state): State<AppState>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Result<Json<serde_json::Value>, Response> {
    check_scope(&q, &state)?;
    // ⭐ REQ-107 · 出口 H2：声明「客户 / JSON / **整份**」—— 发不发由管线判。
    publish_json(
        crate::domain::pipeline::OutletId::H2,
        serde_json::to_value(&*state.graph),
    )
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
    // ⭐ REQ-107 · 出口 H3：声明「客户 / JSON / **逐节点**」—— 一个坏节点只拒那一个。
    publish_json(
        crate::domain::pipeline::OutletId::H3,
        serde_json::to_value(node),
    )
}

/// 首页（**静态壳**，REQ-110）。
///
/// # ⛔ 壳里**没有**任何本体数据（R4 / AC-005）
///
/// 收口前这里是 `page::index_html(&state.project, state.version)`：项目名与版本
/// **服务端渲染进 JS 字面量**。那等于「谁也拿不到页面」—— `require_token` 挂在
/// 全局，浏览器导航又不可能带 `Authorization` 头。
///
/// 现在 `/` 免 token，但壳里**零数据**：项目名 / 版本在拿口径之后走
/// `GET /api/scope`（仍要 token）。于是「能探测到有个服务在」是唯一代价。
///
/// # 为什么仍然走管线 H1
///
/// 模板是常量，但它**是被渲染函数读取的静态素材**（Z4）。H1 把**最终字节**
/// 交给同一道闸：将来谁往模板里写进一个建模术语，首页会当场拒发，
/// 而不是「模板没人管」。这条与 REQ-107 的 AC-008 一致。
async fn index() -> Result<Html<String>, Response> {
    match crate::domain::pipeline::render(
        crate::domain::pipeline::OutletId::H1,
        crate::domain::pipeline::Product::Text(crate::server::page::index_html()),
    ) {
        Ok(rendered) => Ok(Html(rendered.into_text())),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, e.message())),
    }
}

/// ⭐ REQ-110 · `GET /api/scope` —— 页面壳拿口令后问「看哪个项目、哪一版」。
///
/// 为什么要这条：`/` 是静态壳（壳内零数据），而 `/api/graph` 要求
/// `project` / `version` 查询参数。页面必须在**拿到口令之后**才知道作用域。
///
/// 仍受门禁：无 token ⇒ 401（身份门）；项目名含建模术语 ⇒ 500（发布闸 H5，
/// 与首页从前那条「项目名过闸」是**同一道**判据，只是换了个出口）。
async fn get_scope(State(state): State<AppState>) -> Result<Json<serde_json::Value>, Response> {
    // ⭐ REQ-107 · 出口 H5：声明「客户 / JSON / **整份**」。
    publish_json(
        crate::domain::pipeline::OutletId::H5,
        Ok(json!({
            "project": state.project.as_ref(),
            "version": state.version,
        })),
    )
}

/// ⭐ REQ-110 · `GET /api/ontology.ttl` —— 页面上的「导出 TTL」按钮打的就是这里。
///
/// # 与 CLI 是**同一条**序列化路径（纪律 H）
///
/// 这里与 `ontok ontology export` 调的是**同一个** `Product::CustomerTurtle`
/// （→ `pipeline::render` → `ttl::serialize_customer`）。本函数只负责：
/// ① 把管线的 `Err` 映射成 500；② 补 `Content-Type` / `Content-Disposition`。
/// ⛔ 这里**没有**第二份 TTL 生成逻辑，也不碰 `state.view` 之外的任何数据。
///
/// # 为什么返回 `(HeaderMap, String)` 而不是手搓 `Response`
///
/// `Response::builder` / `.into_response()` / `Body::from(` 在出口层是**禁调**
/// （`tests/test_req107_source_enumeration.rs` 的 F3/F4/B1）：响应体必须来自
/// 管线产物。`(HeaderMap, String)` 由 axum 自己变成响应 —— 2xx 只可能来自
/// `Ok(<管线产物>)` 这条分支。
async fn get_ttl(State(state): State<AppState>) -> Result<impl IntoResponse, Response> {
    let opts = crate::domain::ttl::TtlOptions {
        project: state.project.as_ref(),
        version: state.version,
        base: crate::domain::ttl::DEFAULT_BASE,
        action_type_count: state.action_type_count,
    };
    let rendered = match crate::domain::pipeline::render(
        crate::domain::pipeline::OutletId::H4,
        crate::domain::pipeline::Product::CustomerTurtle { view: &state.view, opts },
    ) {
        Ok(r) => r,
        Err(e) if e.is_leak() => {
            // `guard` 已经把命中的词条打到 stderr 了；这里只说「拒绝」。
            eprintln!("✗ 只读图服务：TTL 导出命中发布闸，已拒绝导出。");
            return Err(err(StatusCode::INTERNAL_SERVER_ERROR, e.message()));
        }
        Err(e) => {
            eprintln!("✗ 只读图服务：TTL 导出生成失败：{}", e.message());
            return Err(err(StatusCode::INTERNAL_SERVER_ERROR, e.message()));
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/turtle; charset=utf-8"),
    );
    let name = ttl_file_name(state.project.as_ref(), state.version);
    // `HeaderValue::from_str` 只在含非法字节时失败；文件名已经过白名单化，
    // 这里仍然给出**可用的**兜底，而不是 `unwrap`（一个文件名不该让服务 panic）。
    headers.insert(
        header::CONTENT_DISPOSITION,
        axum::http::HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("attachment")),
    );
    Ok((headers, rendered.into_text()))
}

/// 导出文件名：`<project>-v<n>.ttl`（R5）。
///
/// 项目名是**启动参数**，可能含引号 / 斜杠 / 非 ASCII —— `Content-Disposition`
/// 里放原文就能改写响应头。这里只留 `[A-Za-z0-9._-]`，其余换 `_`。
/// ⚠️ 代价：中文项目名会变成一串下划线；文件名只为「下载下来认得出」，
/// 不承担标识职责（正文里的 `ontok:sourceProject` 才是原文）。
fn ttl_file_name(project: &str, version: i32) -> String {
    let mut s: String = project
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s = "ontology".to_string();
    }
    format!("{s}-v{version}.ttl")
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
///
/// # ⭐ REQ-110 · 身份门只挂在 `/api` 子路由上
///
/// `GET /` 是**静态壳**（零数据），必须免 token —— 否则浏览器永远打不开页面
/// （导航请求带不了 `Authorization` 头）。`/api/*` 全部仍 gated。
///
/// ⚠️ **身份门排在 `api` 的 fallback 之前**（`.layer` 加在已建好的子 Router 上）：
/// 未注册的 `/api/xxx` 无口令同样是 401，不泄露「这个接口存不存在」。
/// 带口令的未注册 `/api/xxx` 才是 404（那是**已授权的人**在问路，该说实话）。
pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/graph", get(get_graph))
        .route("/node", get(get_node))
        // ⭐ REQ-110 · 页面壳取作用域（要 token；项目名在这里过发布闸）。
        .route("/scope", get(get_scope))
        // ⭐ REQ-110 · TTL 导出（要 token；与 `ontology export` 同一条序列化路径）。
        .route("/ontology.ttl", get(get_ttl))
        // 未注册的 `/api/*`：带口令 ⇒ 404 / 变更动词 ⇒ 405（正文由 `normalize_405` 补）。
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/", get(index))
        .nest("/api", api)
        .fallback(fallback)
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
///
/// # ⭐ REQ-115：分组是**另一个入口**，接线仍只有一份
///
/// 本函数**不带概念组**，产出的图是「分组通道未接通」的旧形状
/// （[`crate::domain::graph::GraphProjection::groups`] = `None`）。
/// 它保留下来只为一件事：REQ-107 的 golden `H2_graph.json` 逐字节冻结，
/// 而那份字节就是**这个形状**。生产路径（[`serve`]）**永远**走
/// [`state_from_view_with_groups`] —— 分组接通后线上必然有 `groups` 字段
/// （没有组时是 `[]`，不是缺席，R6）。
///
/// 两条入口共用 [`state_from_view_inner`]：投影、节点索引、断言、
/// `AppState` 装配**一份代码**。差别只有「用哪个投影函数」这一行。
pub fn state_from_view(
    view: &CustomerView,
    project: &str,
    version: i32,
    token: &str,
    action_type_count: usize,
) -> AppState {
    state_from_view_inner(view, None, project, version, token, action_type_count)
}

/// ⭐ REQ-115 · **生产入口**：带权威概念组（`concept_groups`）的状态。
///
/// `declared` 来自 [`crate::domain::graph::declared_groups`]（同一份冻结快照）。
/// 传空切片 = **接通了分组通道、但这一版没有组** ⇒ 线上是 `"groups":[]`
/// 且每个节点 `"group": null`（AC-003 的「没有分组」）。
///
/// ⛔ 分组**只**来自 `declared`：本函数不推断、不补默认组、不读第二处数据源。
pub fn state_from_view_with_groups(
    view: &CustomerView,
    declared: &[crate::domain::graph::DeclaredGroup],
    project: &str,
    version: i32,
    token: &str,
    action_type_count: usize,
) -> AppState {
    state_from_view_inner(view, Some(declared), project, version, token, action_type_count)
}

/// 两条入口的**唯一**实现（`groups = None` ⇒ 旧形状；`Some` ⇒ 接通分组通道）。
fn state_from_view_inner(
    view: &CustomerView,
    groups: Option<&[crate::domain::graph::DeclaredGroup]>,
    project: &str,
    version: i32,
    token: &str,
    action_type_count: usize,
) -> AppState {
    // ── REQ-104 · 出口层属性级剔除 ──
    //
    // 放在这里而不是 `serve()` 里，是因为这个文件里
    // **`serve` 与 `server::http::tests` 都调它，没有第二份接线** ——
    // 放这里，生产接线与测试接线**不可能分叉**。
    //
    // ⚠️ 剔除发生在**守门器（管线）之前**，守门器**一行没改**：
    // `demo3` 第 2 版有 3 个节点因为属性文案带 `pk` / `key` 被逐节点判拒发（HTTP 500），
    // 剔除后这 4 条属性不再进节点视图 ⇒ 91 个节点全部打得开；
    // 而真·建模术语剔不掉，照旧到达守门器、照旧拒发。
    //
    // ⭐ REQ-107：前处理与它的 stderr 报告**都收进管线**（`prepare_view`）——
    // 三条出口（JSON / HTTP / MCP）共用同一份前处理与同一种报告形状。
    let filtered = crate::domain::pipeline::prepare_view(view);
    let view = &filtered.view;

    // ⭐ REQ-115：接不接分组通道，只差这一行。
    let graph = match groups {
        Some(declared) => crate::domain::graph::project_grouped(view, declared),
        None => crate::domain::graph::project(view),
    };

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
        // ⭐ REQ-110：TTL 导出与页面/图共用**这份已过前处理的视图**。
        // 存的是 `filtered.view` 本身（不是再投影一次），所以「导出的属性」
        // 与「`/api/node` 给的属性」不可能分叉。
        view: Arc::new(filtered.view),
        project: Arc::from(project),
        version,
        token: Arc::from(token),
        action_type_count,
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

    let (view, action_type_count, declared_groups) = load_frozen_view(pool, &args.project, args.version)
        .await
        .map_err(|e| anyhow::anyhow!("只读图服务拒绝启动：{}", e.message()))?;

    // ⚠️ 接线**只有这一处**：本函数与 `server::http::tests` 都走
    // `state_from_view*`（两条入口共用一份实现）。测试**不得**自己另搭一份
    // `AppState` —— 上一轮就是这么写的，结果测试完全不守护生产
    // （纪律 H，见 `state_from_view` 文档）。
    //
    // ⭐ REQ-115：生产**永远**走带分组的那条入口 —— 线上 `groups` 字段因此恒在
    // （没有组时是 `[]`），页面的「未分组」才是一个**可判读的结论**而不是缺字段。
    let state = state_from_view_with_groups(
        &view,
        &declared_groups,
        &args.project,
        args.version,
        &token,
        action_type_count,
    );

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    eprintln!(
        "· 只读图服务就绪：项目 {} 第 {} 版（只读 · 页面壳免口令，/api/* 需访问口令）",
        args.project, args.version
    );
    // REQ-110：页面壳不再内嵌作用域，故这里把「打开哪个地址」直接打出来。
    eprintln!(
        "· 浏览器打开 http://{}/ ，在页面顶部填口令；页面壳本身不含任何本体数据。",
        args.bind
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
                // 本夹具是「分组通道未接通」的旧形状（H2 golden 那一份）。
                groups: None,
            }),
            // REQ-110：TTL 导出的数据面与图/节点共用同一份（这里是一份空视图）。
            view: Arc::new(CustomerView { objects: vec![], relations: vec![] }),
            project: Arc::from("demo3"),
            version: 2,
            token: Arc::from(TOKEN),
            action_type_count: 0,
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
                    group: None,
                }],
                edges: vec![],
                groups: None,
            }),
            ..st()
        }
    }

    /// 一份**客户视图里的说明带建模术语**的状态 —— TTL 导出必须拒发（AC-002）。
    ///
    /// ⚠️ 说明的措辞**刻意不含 ASCII 标识符**（`order_no` 那类）：那会被
    /// `property_filter` 当成「技术串」把整段说明隐去（REQ-104），
    /// 于是发布闸根本看不到它 —— 测得的就是**前处理**而不是**闸**。
    /// `主键服务中心` 是 REQ-104 文档里点名的「剔不掉、会到达闸门」的形态。
    fn st_with_poisoned_note() -> AppState {
        use crate::domain::view::{EntityView, PropertyView};
        let view = CustomerView {
            objects: vec![EntityView {
                name: "订单".into(),
                note: Some("本表的主键服务中心，按业务口径解读".into()),
                belongs_to: None,
                aliases: vec![],
                properties: vec![PropertyView {
                    name: "订单号".into(),
                    note: None,
                    required: true,
                    sample: None,
                }],
            }],
            relations: vec![],
        };
        state_from_view(&view, "demo3", 2, TOKEN, 0)
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

    /// ⭐ REQ-110：页面壳免 token（否则浏览器永远打不开），**其它全部 401**。
    ///
    /// 这是 REQ-093 原判据「无 token 一律 401」的**收窄版**：
    /// `/` 从 401 变 200 是**需求要求的**（AC-005），不是判据放宽 ——
    /// 每一条 `/api/*` 仍然逐条断言 401。
    #[tokio::test]
    async fn without_token_the_shell_is_public_but_every_api_is_401() {
        // ① 壳：200（且不含数据 —— 见 AC-005 的独立判据）。
        assert_eq!(
            call_status(st(), "GET", "/", None).await,
            StatusCode::OK,
            "页面壳必须免 token 可打开（ADR-004）"
        );
        // ② 数据面：一条都不许漏。
        for uri in [
            "/api/graph?project=demo3&version=2",
            "/api/node?project=demo3&version=2&id=订单",
            "/api/scope",
            "/api/ontology.ttl",
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

    /// ⛔ **身份门必须排在 `/api` 的 fallback 前面**。
    ///
    /// # ⭐ REQ-110 · AC-010：这条判据**改写、没删**（原判据「未登录探测不到任何路径」）
    ///
    /// 原判据要求 `/` 也无口令 401 —— 那正是「浏览器打不开页面」这个障碍的成因。
    /// 改写后的判据：
    ///
    /// | 请求 | 期望 | 理由 |
    /// | --- | --- | --- |
    /// | `GET /` | **200** | 静态壳，**不含任何数据**（ADR-004 的取舍） |
    /// | `GET /api/…`（含**未注册**的） | **401** | `/api` 这一层的路径探测仍然探测不到 |
    /// | `POST /api/…` | **401**（不是 405） | 变更动词也不许泄露「这个 API 存不存在」 |
    ///
    /// ⇒ 代价被**精确**记录：能探测到「有个服务在」，探测不到「它有哪些接口、里面是什么」。
    #[tokio::test]
    async fn unauthenticated_probes_learn_nothing_but_the_shell_exists() {
        // 壳：公开，但只是一张空壳。
        let r = call(st(), "GET", "/", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = body_of(r).await;
        assert!(
            body.contains("业务关系图") && !body.contains("demo3"),
            "壳必须能打开、且不含项目名：{body}"
        );

        for (m, uri) in [
            ("GET", "/api/graph"),
            ("GET", "/api/definitely-not-a-route"),
            ("POST", "/api/apply"),
            ("DELETE", "/api/node"),
        ] {
            let s = call_status(st(), m, uri, None).await;
            assert_eq!(
                s,
                StatusCode::UNAUTHORIZED,
                "{m} {uri} 无口令时应当 401（不该泄露该 API 是否存在）"
            );
        }
    }

    // ── REQ-106 · AC-003 / AC-004：项目名的转义面与闸门面 ─────────
    //
    // ⭐ REQ-110 改道（**判据保留、断言改写**）：
    //
    // 项目名不再进页面（`/` 是静态壳，AC-005 要求壳里零数据），于是
    // 「注入面」与「闸门面」从**首页**搬到了 `/api/scope` —— 那是今天**唯一**
    // 把项目名送回客户端的出口（H5，gated）。
    //
    // ⚠️ 这一组**刻意不碰数据库**：项目名是 `serve()` 绑定的启动参数，
    // 所以用 `state_from_view`（生产接线）造一个空视图就够判据用。

    /// 项目名（及动作层条数）**故意带注入串**时的服务端状态。走**生产接线**
    /// [`state_from_view`]，⛔ 不手搓 `AppState`（REQ-093 P1 防护的同一条：
    /// 生产接线与测试接线不可能分叉）。
    fn st_named(project: &str) -> AppState {
        state_from_view(&CustomerView { objects: vec![], relations: vec![] }, project, 2, TOKEN, 0)
    }

    /// ⛔ **注入面**（REQ-106 AC-003 的改写版）：项目名含 `"` / `\` / `</script>`
    /// 时，页面壳**一个字节都不受影响** —— 因为壳里压根没有它；
    /// 而 `/api/scope` 返回的 JSON 必须能被原样解析回**完全一样的**项目名。
    ///
    /// 断言从「页面结构标记数量不变」换成更强的版本：
    /// **壳的字节与项目名逐字节无关**（任意项目名下 `GET /` 的响应体完全相同）。
    #[tokio::test]
    async fn a_hostile_project_name_cannot_break_the_shell_or_the_scope_json() {
        let hostiles = [
            r#"has"quote"#,
            r"has\backslash",
            r#"x"; fetch("//evil.example"); var y="#,
            "</script><script>alert(1)</script>",
            "<!--<script>",
            "换行\n与\r回车",
            " 控制符",
        ];
        // 对照组：干净项目名。⛔ 没有它，下面的「字节相同」可能只是「都空」。
        let clean_status = call_status(st_named("demo3"), "GET", "/", None).await;
        assert_eq!(clean_status, StatusCode::OK, "对照组本身必须是 200，否则下面全是空转");
        let clean = body_of(call(st_named("demo3"), "GET", "/", None).await).await;
        assert!(clean.len() > 1000, "壳短得不像页面：{} 字节", clean.len());

        for hostile in hostiles {
            // ① 壳：**免 token**、200、且与项目名逐字节无关。
            let r = call(st_named(hostile), "GET", "/", None).await;
            assert_eq!(r.status(), StatusCode::OK, "页面壳不该受项目名影响");
            let html = body_of(r).await;
            assert_eq!(
                html, clean,
                "项目名 {hostile:?} 改变了页面壳的字节 —— 壳里不该有项目名"
            );
            assert!(!html.contains(hostile), "项目名 {hostile:?} 原样落进了壳里");

            // ② `/api/scope`：JSON 里的项目名必须能原样还原（serde 转义面）。
            let r = call(st_named(hostile), "GET", "/api/scope", Some(TOKEN)).await;
            assert_eq!(r.status(), StatusCode::OK, "干净（不含守门词）的项目名应当出 /api/scope");
            let body = body_of(r).await;
            let v: serde_json::Value = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("{hostile:?} 的 /api/scope 不是合法 JSON：{e}\n{body}"));
            assert_eq!(
                v["project"].as_str(),
                Some(hostile),
                "项目名 {hostile:?} 必须被正确转义后原样还原"
            );
        }
    }

    /// ⛔ **闸门面**（REQ-106 AC-004 的改写版）：项目名含建模术语 ⇒
    /// `/api/scope` **拒绝出内容**（非 200），且守门词不回声。
    ///
    /// 与 `instructions()` / `guarded()` 同一口径：项目名不受快照投影约束，
    /// 所以每条把它送出去的出口都要过闸 —— 今天就是 H5。
    #[tokio::test]
    async fn a_project_name_with_modeling_terms_is_refused_on_the_scope_endpoint() {
        for bad in ["外键", "基数", "主键", "证据"] {
            let name = format!("demo{bad}demo");
            let r = call(st_named(&name), "GET", "/api/scope", Some(TOKEN)).await;
            assert!(
                !r.status().is_success(),
                "项目名含 {bad:?} 应当被 /api/scope 拒绝，实际 {:?}",
                r.status()
            );
            let b = body_of(r).await;
            assert!(!b.contains(bad), "守门词 {bad:?} 本身不许从错误响应里漏出去：{b}");
            assert!(!b.contains(&name), "整个项目名也不许从错误响应里漏出去：{b}");
        }
    }

    /// 对照组（纪律 E）：**干净的项目名必须过得去**。
    /// 没有这条，上一条全绿也可能只是「所有 scope 请求都被拒了」。
    #[tokio::test]
    async fn a_clean_project_name_still_serves_the_scope() {
        let r = call(st_named("demo3"), "GET", "/api/scope", Some(TOKEN)).await;
        assert_eq!(r.status(), StatusCode::OK);
        let b = body_of(r).await;
        assert!(b.contains("demo3"), "/api/scope 应带上项目名：{b}");
        assert!(b.contains("\"version\":2"), "/api/scope 应带上版本号：{b}");
    }

    /// 对照组（纪律 E）：**免 token 的壳不许成为绕过身份门的路**。
    ///
    /// 壳公开 ≠ 数据公开：唯一携带项目名的 `/api/scope` 仍然必须 401。
    #[tokio::test]
    async fn the_public_shell_does_not_open_the_scope_endpoint() {
        assert_eq!(
            call_status(st_named("demo外键demo"), "GET", "/api/scope", None).await,
            StatusCode::UNAUTHORIZED,
            "无口令时 /api/scope 应当被身份门拦下（壳公开不改变这一点）"
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
        let state = state_from_view(&view_with_duplicate_names(), "demo3", 2, TOKEN, 0);
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
        let state = state_from_view(&view_with_duplicate_names(), "demo3", 2, TOKEN, 0);
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

    // ── ⭐ REQ-110 · TTL 导出与页面可达性（AC-002/004/005/006/007）──

    /// 一份**小而真**的客户视图（含父子关系、必填/可选、说明）。
    fn ttl_sample_view() -> CustomerView {
        use crate::domain::view::{EntityView, PropertyView, RelationView};
        CustomerView {
            objects: vec![
                EntityView {
                    name: "订单".into(),
                    note: Some("一张订单".into()),
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![
                        PropertyView {
                            name: "下单时间".into(),
                            note: None,
                            required: true,
                            sample: Some("2026-01-01".into()),
                        },
                        PropertyView { name: "金额".into(), note: Some("含税".into()), required: false, sample: None },
                    ],
                },
                EntityView {
                    name: "客户".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec!["主顾".into()],
                    properties: vec![],
                },
            ],
            relations: vec![RelationView {
                from: "订单".into(),
                to: "客户".into(),
                how_many: "多对一".into(),
                meaning: "归属于".into(),
            }],
        }
    }

    /// AC-004：三种请求的状态码与响应头**逐项**断言。
    #[tokio::test]
    async fn ac004_ttl_endpoint_is_gated_and_sets_turtle_headers() {
        let state = state_from_view(&ttl_sample_view(), "demo3", 2, TOKEN, 0);

        // ① 无 token ⇒ 401。
        assert_eq!(
            call_status(state.clone(), "GET", "/api/ontology.ttl", None).await,
            StatusCode::UNAUTHORIZED
        );
        // ② 错 token ⇒ 401。
        assert_eq!(
            call_status(state.clone(), "GET", "/api/ontology.ttl", Some("nope")).await,
            StatusCode::UNAUTHORIZED
        );

        // ③ 正确 token ⇒ 200 + 两个头。
        let r = call(state, "GET", "/api/ontology.ttl", Some(TOKEN)).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            r.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("text/turtle; charset=utf-8"),
            "Content-Type 必须是 text/turtle"
        );
        assert_eq!(
            r.headers()
                .get(header::CONTENT_DISPOSITION)
                .and_then(|v| v.to_str().ok()),
            Some(r#"attachment; filename="demo3-v2.ttl""#),
            "文件名口径见 R5"
        );
        let body = body_of(r).await;
        assert!(body.starts_with("@prefix : <https://ontok.local/demo3#> ."), "{body}");
        assert!(body.contains("a owl:Class"), "{body}");
    }

    /// AC-005：用 `secretproj` / 第 7 版起状态，`GET /` **免 token** 且
    /// 响应体里**没有任何本体数据**（项目名 / 版本 / 对象名）。
    #[tokio::test]
    async fn ac005_the_shell_carries_no_data_for_secretproj() {
        use crate::domain::view::EntityView;
        let view = CustomerView {
            objects: vec![EntityView {
                name: "绝密对象".into(),
                note: Some("绝密说明".into()),
                belongs_to: None,
                aliases: vec![],
                properties: vec![],
            }],
            relations: vec![],
        };
        let state = state_from_view(&view, "secretproj", 7, TOKEN, 0);

        let r = call(state, "GET", "/", None).await;
        assert_eq!(r.status(), StatusCode::OK, "`/` 必须免 token 可打开");
        let body = body_of(r).await;
        // 交付证据（`cargo test -- --nocapture` 可见）：这次检查的**实际结果**。
        eprintln!(
            "AC-005 现场：project=secretproj version=7 · GET / 无 token = 200 · \
             shell={} 字节 · grep「secretproj」={} 「绝密对象」={} 「第 <b>7</b> 版」={} · \
             body==INDEX_HTML：{}",
            body.len(),
            body.contains("secretproj"),
            body.contains("绝密对象"),
            body.contains("第 <b>7</b> 版"),
            body == crate::server::page::index_html(),
        );
        for bad in ["secretproj", "绝密对象", "绝密说明"] {
            assert!(!body.contains(bad), "页面壳里出现了 `{bad}` —— 壳必须零数据：{body}");
        }
        // 版本号也不能以作用域的形式出现（壳里没有 `who`/`SCOPE` 的字面量）。
        assert!(!body.contains("secretproj"), "{body}");
        assert!(!body.contains("第 <b>7</b> 版"), "{body}");
        // 反向：壳**就是**那一份静态模板（逐字节），没有服务端拼装的数据。
        assert_eq!(body, crate::server::page::index_html(), "壳的字节必须与模板完全一致");
    }

    /// AC-002：客户视图的说明里带「主键」⇒ TTL 导出**拒绝**（HTTP 500 + 不含词条）。
    #[tokio::test]
    async fn ac002_a_poisoned_note_is_refused_on_the_ttl_endpoint() {
        let r = call(st_with_poisoned_note(), "GET", "/api/ontology.ttl", Some(TOKEN)).await;
        assert_eq!(
            r.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "客户视图导出必须过守门器（AC-002）"
        );
        let b = body_of(r).await;
        assert!(!b.contains("主键"), "守门词不许从错误响应漏出：{b}");
        assert!(!b.contains("订单"), "拒绝时不许顺带把视图数据发出去：{b}");
        assert!(!b.contains("@prefix"), "拒绝时必须一个正文字节都不给：{b}");
    }

    /// AC-006：其它 API 仍全部 gated —— 逐个断言。
    #[tokio::test]
    async fn ac006_every_data_api_is_still_gated() {
        let state = state_from_view(&ttl_sample_view(), "demo3", 2, TOKEN, 0);
        for uri in [
            "/api/graph?project=demo3&version=2",
            "/api/node?project=demo3&version=2&id=订单",
            "/api/ontology.ttl",
            "/api/scope",
        ] {
            assert_eq!(
                call_status(state.clone(), "GET", uri, None).await,
                StatusCode::UNAUTHORIZED,
                "{uri} 无 token 必须 401"
            );
        }
    }

    /// AC-007：页面上「导出 TTL」下载到的字节 == CLI 导出的字节（**逐字节**）。
    ///
    /// CLI 那条路的出口是 `publish_customer(OutletId::C6, …)` + `emit_domain_text`
    /// （`emit_domain_text` 写的就是 `OutletRender::as_str()`，不补换行）；
    /// HTTP 那条路是 `OutletId::H4` 的响应体。两条都走 `Product::CustomerTurtle`。
    /// 这里直接比**字节**（比 sha256 更强），并顺手钉住 `filter_view` 的幂等性
    /// （HTTP 的 `state.view` 已剔过一次，管线里又剔一次）。
    #[tokio::test]
    async fn ac007_http_and_cli_ttl_bytes_are_identical() {
        let view = ttl_sample_view();
        let state = state_from_view(&view, "demo3", 2, TOKEN, 0);

        let opts = crate::domain::ttl::TtlOptions {
            project: "demo3",
            version: 2,
            base: crate::domain::ttl::DEFAULT_BASE,
            action_type_count: 0,
        };
        let cli_bytes = crate::domain::pipeline::render(
            crate::domain::pipeline::OutletId::C6,
            crate::domain::pipeline::Product::CustomerTurtle { view: &view, opts },
        )
        .expect("干净视图必须过闸")
        .into_text();

        let r = call(state, "GET", "/api/ontology.ttl", Some(TOKEN)).await;
        assert_eq!(r.status(), StatusCode::OK);
        let http_bytes = body_of(r).await;
        assert_eq!(http_bytes, cli_bytes, "HTTP 下载的字节必须与 CLI 导出逐字节相同");
        assert!(!http_bytes.is_empty());
    }

    /// ⛔ 壳里**不许**出现本体数据这件事本身要有反空转：
    /// 上面 AC-005 的 `body == index_html()` 若模板恰好是空串也会绿。
    #[test]
    fn the_shell_template_is_not_vacuous() {
        let html = crate::server::page::index_html();
        assert!(html.len() > 1000, "壳只有 {} 字节 —— 多半读错了", html.len());
        assert!(html.contains("<svg"), "壳里没有画布");
        assert!(html.contains("api/ontology.ttl"), "壳里没有导出按钮的落点");
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
