//! ⭐ REQ-162 · **页面壳的兼容判据**（旧路由 `/` 与新本地评审台 `/` 是同一份常量）。
//!
//! REQ-125 的这份文件原来钉的是「七个管理视图 + 口令框」。REQ-162 把 console 改成
//! **本地评审台**（一屏两栏：评审 + 导出）之后，那批视图全删了 —— 本文件随之重写：
//!
//! | 判据 | 为什么还在这一份里 |
//! | --- | --- |
//! | `GET /` 免凭据 + 逐字节等于常量 | 页面壳是**匿名可达**的外送通道（出口 I22） |
//! | 三个安全头逐字在 | AC-011（旧口径不变） |
//! | 零外部资源 + ≤300 KB | AC-008（新口径：300 KB） |
//! | 壳内零数据 | 项目名 / 对象名一个都不进 HTML |
//! | 无 `localStorage` / 无口令框 / 无旧视图 | REQ-162 的删除面（替代判据在 `test_req162_console`） |
//!
//! ⚠️ 旧路由（`console::http::build_router`，REQ-160 `review open` 还在用）也挂同一个
//! [`CONSOLE_HTML`]：所以这一份顺带证明「换个路由表，页面壳不漂」。

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ontok::console::audit::AuditLog;
use ontok::console::decl::DeclPaths;
use ontok::console::page::{CONSOLE_HTML, CSP};
use ontok::console::tokens::AdminTokens;
use ontok::console::{ConsoleParts, ConsoleService};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::AuditSink;
use tower::ServiceExt;

const ADMIN: &str = "FAKE-ADMIN-TOKEN-0123456789abcdef";

const SCHEMA: &str = r#"{"tables":[
  {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true}]}
]}"#;

/// AC-001 的判据样本：项目名 / 对象名（一个都不许进壳）。
const SECRET_PROJECT: &str = "secretproj";
const SECRET_OBJECT: &str = "绝密对象";

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req125b-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn test_service(tag: &str) -> Arc<ConsoleService> {
    let dir = temp_dir(tag);
    let paths = DeclPaths {
        exclusions: dir.join("exclusions.json"),
        policy: dir.join("query-policy.json"),
        tenancy: dir.join("tenancy.json"),
    };
    let poison = format!(r#"{{"version":"9.9.9","note":"{SECRET_OBJECT}"}}"#);
    std::fs::write(&paths.exclusions, &poison).unwrap();
    std::fs::write(&paths.policy, &poison).unwrap();
    std::fs::write(&paths.tenancy, &poison).unwrap();
    let tokens = AdminTokens::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"ops","token":"{ADMIN}","kind":"admin"}}
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
        audit: AuditLog::new(AuditSink::stdout_only()),
        bind: "127.0.0.1:8899".parse().unwrap(),
        allow_public: false,
        // ⭐ REQ-159 · DSN 加密器（本文件不测 DSN 端点，用固定测试密钥）。
        secret_cipher: ontok::provision::dsn::SecretCipher::for_tests(),
    }))
}

struct Reply {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }
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

fn page_request() -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn the_shell_is_anonymous_zero_data_and_byte_identical() {
    let svc = test_service("shell");
    let r = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        page_request(),
    )
    .await;

    assert_eq!(r.status, StatusCode::OK, "`GET /` 必须匿名可打开");
    assert_eq!(r.body, CONSOLE_HTML, "GET / 必须逐字节就是页面常量");
    for needle in [SECRET_PROJECT, SECRET_OBJECT, ADMIN, "第 7 版"] {
        assert!(!r.body.contains(needle), "页面壳里出现了 {needle:?}");
    }
    // 反向自证：这个服务确实叫 secretproj（否则上面「0 命中」是空转）。
    assert_eq!(svc.project, SECRET_PROJECT);
}

#[tokio::test]
async fn the_three_security_headers_and_csp_are_on_the_page() {
    let svc = test_service("headers");
    let r = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        page_request(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let csp = r.header("content-security-policy").expect("缺 CSP");
    assert_eq!(csp, CSP, "响应头必须逐字等于登记的那份 CSP");
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("connect-src 'self'"));
    assert_eq!(r.header("x-content-type-options").as_deref(), Some("nosniff"));
    assert_eq!(r.header("referrer-policy").as_deref(), Some("no-referrer"));
    assert!(
        r.header("content-type").unwrap_or_default().starts_with("text/html"),
        "页面必须是 text/html"
    );
}

#[test]
fn the_page_is_small_self_contained_and_has_no_browser_state() {
    assert!(
        CONSOLE_HTML.len() < 300 * 1024,
        "单文件必须 ≤300 KB，实际 {} 字节",
        CONSOLE_HTML.len()
    );
    assert!(CONSOLE_HTML.len() > 10 * 1024, "页面太小，可能被截断");
    for needle in [
        "<script src=",
        "<link ",
        "https://",
        "http://",
        "@import",
        "integrity=",
        "localStorage",
        "sessionStorage",
        "indexedDB",
        "innerHTML",
        "document.write",
        "XMLHttpRequest",
        "<input",
    ] {
        assert!(
            !CONSOLE_HTML.contains(needle),
            "页面里出现了 {needle:?}（REQ-162 · AC-008）"
        );
    }
    // 旧工作台的视图 / 口令框 / 旧端点，一个都不许回来。
    for gone in [
        "id=\"v-projects\"",
        "id=\"v-bundles\"",
        "id=\"v-config\"",
        "id=\"v-audit\"",
        "id=\"tab-projects\"",
        "id=\"token\"",
        "登录",
        "项目与版本",
        "声明编辑",
        "/api/projects",
        "/api/import",
        "/api/apply",
        "/api/audit",
        "api/sparql",
    ] {
        assert!(
            !CONSOLE_HTML.contains(gone),
            "{gone} 是被 REQ-162 删掉的旧工作台残留"
        );
    }
    // 新页面就该是这两栏。
    for needle in ["评审（核心）", "导出", "定版", "导出构建包", "全选接受", "全选拒绝"] {
        assert!(CONSOLE_HTML.contains(needle), "新页面缺 {needle:?}");
    }
}

#[test]
fn the_served_page_is_the_committed_constant() {
    let src = common::read_source("src/console/page.rs");
    assert!(
        src.contains("pub const CONSOLE_HTML"),
        "页面常量必须叫 CONSOLE_HTML"
    );
    assert!(
        src.contains("pub const CSP"),
        "CSP 必须是登记过的常量（测试逐字对）"
    );
}
