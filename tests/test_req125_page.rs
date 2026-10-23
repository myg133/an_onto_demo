//! REQ-125 · **工作台页面壳**的进程内判据（不连库、不起 socket、不跑浏览器）。
//!
//! # 这份文件治的是什么
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | 拿 `secretproj` / 第 7 版 / `绝密对象` 起服务，`GET /` 正文里 grep ⇒ **0 命中** |
//! | AC-002 | 零外部依赖（`<script src=` / `<link href=` / `https://` / `@import` / `integrity=` = 0）且 < 200 KB |
//! | AC-003 | 壳里只有 `sessionStorage`；⛔ `localStorage` / URL / 源码里的 token |
//! | AC-004 | 七个视图的容器都在，且**只**调既有 `/api/*` |
//! | AC-005 | `GET /` 不碰数据；`/api/*` 仍然要 token（没有因为加页面而开洞） |
//! | AC-010 | 用户数据一律 `textContent`：壳里 `innerHTML` = 0 |
//! | AC-011 | 三个响应头逐项在 |
//!
//! # 为什么这些判据不需要真库
//!
//! 页面壳是**静态常量**（零数据），`GET /` 的 handler 不读任何服务状态 —— 所以
//! 夹具池故意连不上（`connect_lazy` 指向 127.0.0.1:1）。这也顺便证明
//! 「页面壳没有顺手查库」：真查了就会 500 而不是 200。
//!
//! 交互层（warning 文案、取消不发请求、XSS）在
//! `tests/fixtures/req125/page_probe.js` 里跑（Node + 假 DOM，见交付报告的实测）。

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::console::audit::AuditLog;
use ontok::console::decl::DeclPaths;
use ontok::console::page::{CONSOLE_HTML, CSP};
use ontok::console::tokens::AdminTokens;
use ontok::console::{ConsoleParts, ConsoleService};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::AuditSink;
use tower::ServiceExt;

const ADMIN: &str = "FAKE-ADMIN-TOKEN-0123456789abcdef";
const QUERY: &str = "FAKE-QUERY-TOKEN-0123456789abcdef";

const SCHEMA: &str = r#"{"tables":[
  {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true}]}
]}"#;

/// AC-001 的判据样本：项目名 / 版本提示 / 对象名。
const SECRET_PROJECT: &str = "secretproj";
const SECRET_OBJECT: &str = "绝密对象";
const SECRET_VERSION_HINT: &str = "第 7 版";

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req125-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 三份声明里都塞上「绝密对象」—— 页面壳若**任何**一处读过声明，判据就会红。
fn write_decls(dir: &PathBuf) -> DeclPaths {
    let paths = DeclPaths {
        exclusions: dir.join("exclusions.json"),
        policy: dir.join("query-policy.json"),
        tenancy: dir.join("tenancy.json"),
    };
    let poison = format!(
        r#"{{"version":"9.9.9","note":"{SECRET_OBJECT}","declared_by":"qa"}}"#
    );
    std::fs::write(&paths.exclusions, &poison).unwrap();
    std::fs::write(&paths.policy, &poison).unwrap();
    std::fs::write(&paths.tenancy, &poison).unwrap();
    paths
}

fn test_service(tag: &str) -> Arc<ConsoleService> {
    let dir = temp_dir(tag);
    let paths = write_decls(&dir);
    let tokens = AdminTokens::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"ops","token":"{ADMIN}","kind":"admin"}},
            {{"label":"agent","token":"{QUERY}","kind":"query","tenant":"C0001"}}
        ]}}"#
    ))
    .expect("token 表");
    let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
    Arc::new(ConsoleService::new(ConsoleParts {
        pool: sqlx::postgres::PgPoolOptions::new()
            // 连不上是**故意的**：页面壳若真去查库，判据会拿到 500。
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool"),
        project: SECRET_PROJECT.into(),
        base: "https://ontok.local/".into(),
        tokens,
        schema,
        paths,
        audit: AuditLog::new(AuditSink::stderr_only()),
        bind: "127.0.0.1:8899".parse().unwrap(),
        allow_public: false,
    }))
}

struct Reply {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: String,
}

fn header_of(reply: &Reply, name: &str) -> Option<String> {
    reply
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

async fn call(app: axum::Router, req: Request<Body>) -> Reply {
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let headers: Vec<(String, String)> = res
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect();
    let body = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读 body");
    Reply {
        status,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    }
}

fn get(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::empty()).unwrap()
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 页面壳零数据
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_the_shell_carries_no_data_for_secretproj() {
    let svc = test_service("ac001");
    let r = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/", None),
    )
    .await;

    assert_eq!(r.status, StatusCode::OK, "GET / 必须匿名可打开（AC-001）");
    assert_eq!(
        r.body, CONSOLE_HTML,
        "GET / 必须逐字节就是页面常量（服务端不注入任何东西）"
    );

    // 判据：项目名 / 对象名 / 版本提示 —— 一个都不许在壳里。
    for needle in [SECRET_PROJECT, SECRET_OBJECT, SECRET_VERSION_HINT] {
        assert!(
            !r.body.contains(needle),
            "GET / 里出现了 {needle:?} —— 页面壳不是零数据（AC-001）"
        );
    }
    // 版本号的裸值也判一遍（壳里不该有任何「第 N 版」的渲染结果）。
    assert!(
        !r.body.contains("secretproj"),
        "项目名的小写形态也不许出现"
    );
    // 声明路径 / token 明文同样不许出现。
    for needle in [ADMIN, QUERY, "exclusions.json", "query-policy.json"] {
        assert!(!r.body.contains(needle), "页面壳里出现了 {needle:?}");
    }

    // 反向自证：这个服务确实叫 secretproj（否则上面「0 命中」是空转）。
    assert_eq!(svc.project, SECRET_PROJECT);
}

#[tokio::test]
async fn ac001_the_page_needs_no_token_but_every_api_still_does() {
    let svc = test_service("ac001b");
    let app = || ontok::console::http::build_router(Arc::clone(&svc));

    // `/` 免 token；带上 token 也还是同一份壳（页面壳不因身份而变）。
    let anon = call(app(), get("/", None)).await;
    assert_eq!(anon.status, StatusCode::OK);
    let authed = call(app(), get("/", Some(ADMIN))).await;
    assert_eq!(authed.status, StatusCode::OK);
    assert_eq!(anon.body, authed.body, "页面壳不随身份变化");

    // `/api/*` 一个都没被放开。
    for uri in [
        "/api/projects",
        "/api/review?proposal=p1",
        "/api/audit",
        "/api/config/exclusions",
    ] {
        let r = call(app(), get(uri, None)).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{uri} 必须仍然要 token");
    }

    // query token 调管理端点仍是 403（REQ-124 的 R1 不因加页面而软化）。
    let r = call(app(), get("/api/projects", Some(QUERY))).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "query token 必须是 403");
    assert!(r.body.contains("TOKEN_KIND_FORBIDDEN"), "{}", r.body);
}

// ══════════════════════════════════════════════════════════════
// AC-011 · 三个响应头（`curl -I` 的三项）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac011_the_three_security_headers_are_on_the_page() {
    let svc = test_service("ac011");
    let r = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/", None),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    let csp = header_of(&r, "content-security-policy").expect("缺 Content-Security-Policy");
    assert!(
        csp.contains("default-src 'self'"),
        "CSP 至少要 default-src 'self'：{csp}"
    );
    assert!(
        csp.contains("connect-src 'self'"),
        "CSP 要把请求锁在同源：{csp}"
    );
    assert_eq!(csp, CSP, "响应头必须逐字等于登记的那份 CSP");

    assert_eq!(
        header_of(&r, "x-content-type-options").as_deref(),
        Some("nosniff"),
        "缺 X-Content-Type-Options: nosniff"
    );
    assert_eq!(
        header_of(&r, "referrer-policy").as_deref(),
        Some("no-referrer"),
        "缺 Referrer-Policy: no-referrer"
    );
    assert!(
        header_of(&r, "content-type")
            .unwrap_or_default()
            .starts_with("text/html"),
        "页面必须是 text/html"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 零外部依赖 + 体积
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac002_zero_external_dependency_and_under_200kb() {
    let svc = test_service("ac002");
    let r = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/", None),
    )
    .await;
    let body = r.body;
    for needle in [
        "<script src=",
        "<link href=",
        "<link ",
        "https://",
        "http://",
        "@import",
        "integrity=",
    ] {
        assert!(
            !body.contains(needle),
            "页面壳里出现了外部依赖 {needle:?}（AC-002）"
        );
    }
    assert!(
        body.len() < 200 * 1024,
        "单文件必须 < 200 KB，实际 {} 字节",
        body.len()
    );
    // 体积下限自证：页面不是空壳（七个 tab + 内联 CSS/JS 都在）。
    assert!(body.len() > 20 * 1024, "页面太小，可能被截断了：{}", body.len());
    assert_eq!(body, CONSOLE_HTML);
}

// ══════════════════════════════════════════════════════════════
// AC-003 · token 只进 sessionStorage
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac003_the_token_never_goes_to_long_term_storage_or_the_url() {
    let svc = test_service("ac003");
    let r = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/", None),
    )
    .await;
    let body = r.body;

    assert!(!body.contains("localStorage"), "⛔ 不许用 localStorage");
    assert!(!body.contains("indexedDB"), "⛔ 不许用 indexedDB");
    assert!(!body.contains("document.cookie"), "⛔ 不许把口令写 cookie");
    assert!(body.contains("sessionStorage"), "口令必须存 sessionStorage");

    // 源码里不许出现任何 token 值（连假值都不放：会被人当模板抄走）。
    for needle in [ADMIN, QUERY, "Bearer FAKE", "REPLACE-WITH"] {
        assert!(!body.contains(needle), "页面壳里出现了 token {needle:?}");
    }

    // 口令不进 URL：不读 location.search / hash，也不拼 `token=` 查询参数。
    assert!(!body.contains("location.search"));
    assert!(!body.contains("location.hash"));
    assert!(!body.contains("token="), "URL 里不许出现 token");
}

// ══════════════════════════════════════════════════════════════
// AC-004 / AC-005 · 七个视图只消费既有 /api/*
// ══════════════════════════════════════════════════════════════

#[test]
fn ac004_the_seven_views_consume_only_the_existing_endpoints() {
    // 七个视图容器。
    for view in ["projects", "review", "export", "config", "audit", "query", "catalog"] {
        assert!(
            CONSOLE_HTML.contains(&format!("id=\"v-{view}\"")),
            "缺视图容器 v-{view}"
        );
        assert!(
            CONSOLE_HTML.contains(&format!("id=\"tab-{view}\"")),
            "缺 tab 按钮 tab-{view}"
        );
    }

    // 视图消费的**每一条**端点都是既有端点（REQ-124 的 10 组 + REQ-123 的 3 个读端点）。
    for endpoint in [
        "api/projects",
        "api/apply",
        "api/freeze",
        "api/review",
        "api/decide",
        "api/export/ttl",
        "api/r2rml",
        "api/config/",
        "api/audit",
        "api/sparql",
        "api/catalog",
        "api/describe",
    ] {
        assert!(
            CONSOLE_HTML.contains(endpoint),
            "页面没有消费 {endpoint}"
        );
    }

    // ⛔ 页面里只有一个 fetch 调用点（其余全走 `request` / `json` / `post`）——
    // 这样「某个视图偷偷直连数据库 / 读文件」在结构上没有位置。
    assert_eq!(
        CONSOLE_HTML.matches("fetch(").count(),
        1,
        "页面只应有一个 fetch 调用点（request 层）"
    );
    // 所有请求都走相对路径（同源）。出现绝对 URL 就是外部依赖（AC-002 已判）或绕过 CSP。
    assert!(!CONSOLE_HTML.contains("\"//"), "不许出现协议相对 URL");
    assert!(!CONSOLE_HTML.contains("XMLHttpRequest"), "不许自己造请求层");
}

// ══════════════════════════════════════════════════════════════
// AC-010 · 无 innerHTML 用于用户数据
// ══════════════════════════════════════════════════════════════

#[test]
fn ac010_no_innerhtml_anywhere() {
    for needle in [
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
    ] {
        assert!(
            !CONSOLE_HTML.contains(needle),
            "页面壳里出现了 {needle:?}（AC-010：用户数据只走 textContent）"
        );
    }
    // 反向自证：确实有 textContent 的写入点（不是「零个写点所以全绿」）。
    assert!(
        CONSOLE_HTML.matches("textContent").count() >= 3,
        "页面应该有若干 textContent 写点，实际 {} 处",
        CONSOLE_HTML.matches("textContent").count()
    );
}

// ══════════════════════════════════════════════════════════════
// 源码级：壳里的页面常量就是仓库里的那份（防「测试读 A、线上发 B」）
// ══════════════════════════════════════════════════════════════

#[test]
fn the_served_page_is_the_committed_constant() {
    let src = common::read_source("src/console/page.rs");
    assert!(src.contains("pub const CONSOLE_HTML"), "页面常量必须叫 CONSOLE_HTML");
    assert!(
        src.contains("pub const CSP"),
        "CSP 必须是登记过的常量（测试逐字对）"
    );
    // 常量里确实有七个视图的标题（防止有人把 HTML 换成空串还全绿）。
    for title in ["项目与版本", "评审", "导出", "声明编辑", "审计", "查询台", "目录浏览"] {
        assert!(CONSOLE_HTML.contains(title), "壳里缺标题 {title}");
    }
}
