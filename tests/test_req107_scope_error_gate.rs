//! REQ-107 · AC-006：Z1 零闸出口的判据（TC-018 / TC-019 / TC-020）。
//!
//! # 洞是什么
//!
//! `check_scope` 的项目/版本不匹配分支把 `state.project` **裸拼**进 400 正文：
//! 既不过 `mcp::tools::guard`、也不走 `safe_error`、也不转义。
//! 同一个项目名在**首页**会被 `page.rs` 拒发，在这里却能一个字不差地读回来 ——
//! 同一份数据、两个出口、两种处置，正是本需求要收口的那个病。
//!
//! # 本文件的判据全是**行为**的（不需要数据库）
//!
//! `state_from_view` + `build_router` 都是 `pub`，`Router::oneshot` 在进程内打真实请求。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::domain::view::CustomerView;
use ontok::server::http::{build_router, state_from_view};
use tower::ServiceExt;

const TOKEN: &str = "correct-horse-battery";

fn state(project: &str) -> ontok::server::http::AppState {
    state_from_view(
        &CustomerView { objects: vec![], relations: vec![] },
        project,
        2,
        TOKEN,
    )
}

async fn get(project_bound: &str, uri: &str) -> (StatusCode, String) {
    let app = build_router(state(project_bound));
    let r = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot 的错误类型是 Infallible");
    let status = r.status();
    let body = String::from_utf8_lossy(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap())
        .to_string();
    (status, body)
}

// ══════════════════════════════════════════════════════════════
// TC-018 · 回显项目名 ⇒ 400 响应体必须不含项目名
// ══════════════════════════════════════════════════════════════

/// ⛔ 服务端绑定的项目名 = `外键登记表`（含守门词「外键」）；
/// 请求故意让项目名不匹配 ⇒ 进入 `check_scope` 那条分支。
#[tokio::test]
async fn test_req107_z1_scope_error_does_not_echo_the_project_name() {
    let (status, body) = get("外键登记表", "/api/graph?project=wrong&version=2").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "错参应当是 400：{body}");
    assert!(
        !body.contains("外键登记表"),
        "Z1 把项目名原样回显进了 400 正文 —— 同一个名字在首页会被拒发，这里却读得到：{body}"
    );
    assert!(
        !body.contains("外键"),
        "更严的一格：连词干都不许漏（`safe_error` 命中时应当整句换掉）：{body}"
    );
    // ⛔ 不许用「返回空 400」过关：错误必须说话。
    assert!(body.contains("error"), "400 正文必须有一句人话：{body}");
    assert!(!body.trim().is_empty(), "400 正文不许是空的");
}

// ══════════════════════════════════════════════════════════════
// TC-019 · 对照组：干净项目名**仍应**回显
// ══════════════════════════════════════════════════════════════

/// 修复**不许**退化成「一律换成通用文案」——
/// 那样运维就再也看不出「本服务到底绑的是哪一份」。
#[tokio::test]
async fn test_req107_z1_clean_project_name_is_still_echoed() {
    let (status, body) = get("样例项目", "/api/graph?project=wrong&version=2").await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "错参应当是 400：{body}");
    assert!(
        body.contains("样例项目"),
        "干净项目名必须仍然回显（否则运维定位不到是哪一份）：{body}"
    );
}

// ══════════════════════════════════════════════════════════════
// TC-020 · 同一函数其余 4xx 分支也不得回显调用方输入
// ══════════════════════════════════════════════════════════════

/// 缺 `project` / 缺 `version` / `version` 非数字 —— 三条分支的文案里
/// 不得出现调用方传进来的原始值。收口前这三条就已经是固定文案（护栏已存在），
/// 本条的价值是**防止修 Z1 时顺手把 raw 拼回去**。
#[tokio::test]
async fn test_req107_check_scope_other_4xx_branches_do_not_echo_input() {
    // ① 缺 project
    let (s, b) = get("样例项目", "/api/graph?version=2").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(!b.contains("version=2"), "缺 project 的分支不许回显入参：{b}");

    // ② 缺 version
    let (s, b) = get("样例项目", "/api/graph?project=样例项目").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b.contains("error"), "必须有人话：{b}");

    // ③ version 非数字（且入参里带守门词）⇒ 不许回显。
    let (s, b) = get("样例项目", "/api/graph?project=样例项目&version=外键").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(
        !b.contains("外键"),
        "`version` 解析失败的分支把入参回显了（修 Z1 时顺手拼回去了？）：{b}"
    );
    assert!(b.contains("error"), "必须有人话：{b}");

    // ④ 反空转：本文件的四条请求**真的**打到了路由（不是 401 被身份门挡掉）。
    let (s, _) = get("样例项目", "/api/graph?project=样例项目&version=2").await;
    assert_eq!(s, StatusCode::OK, "同一个 state 下正确参数必须 200 —— 否则上面全是空转");
}

// ══════════════════════════════════════════════════════════════
// 附加 · 源码层：Z1 必须**经由管线**的错误路径（不是裸 `format!` 进 `err()`）
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_z1_goes_through_the_pipeline_error_path() {
    let src = common::strip_comments(&common::read_source("src/server/http.rs"));
    let code = src.split("#[cfg(test)]").next().unwrap_or("").to_string();

    let start = code.find("fn check_scope(").expect("找不到 check_scope");
    let tail = &code[start..];
    let end = tail[1..]
        .find("\nasync fn ")
        .map(|n| n + 1)
        .unwrap_or(tail.len());
    let body = &tail[..end];

    assert!(
        body.contains("OutletId::Z1"),
        "`check_scope` 里找不到 Z1 的管线调用点：{body}"
    );
    assert!(
        body.contains("本服务只对外提供"),
        "Z1 的文案原文应当保留（变化由 `safe_error` 在**命中时**决定，不是手改文案）"
    );
    assert!(
        body.contains("Product::ErrorText("),
        "Z1 没有走管线的错误路径（`Product::ErrorText`）—— 那就是收口前的零闸形状"
    );
    assert!(
        !body.contains("publish_error_text("),
        "第 2 轮起 `publish_error_text` 这个「任意 &str → 可写出产物」的公开工厂**已删除**\
         （A11 R-C：客户产物只许有一个公开构造入口 `render`）"
    );
    assert!(
        body.contains("rendered.as_str()"),
        "`err(…, msg)` 的正文必须来自**管线产物**（`rendered.as_str()`），而不是裸 `format!`"
    );
    // 管线入口必须在 `err(` 之前被调用（先过错误闸、再进响应）。
    let pub_at = body.find("pipeline::render(").expect("找不到 pipeline::render");
    let err_at = body.rfind("err(StatusCode::BAD_REQUEST").expect("找不到 err 调用");
    assert!(pub_at < err_at, "错误文案必须先过 `safe_error` 才进响应");
}
