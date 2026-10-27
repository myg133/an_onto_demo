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

// ══════════════════════════════════════════════════════════════
// ⭐ REQ-162 整改（QA BLOCK-1 / MED-1）· 把两处**被弱化的判据**补回原强度
// ══════════════════════════════════════════════════════════════

/// **端点集合冻结（精确相等）** —— 不是「几个新端点在」。
///
/// 旧页面的 `the_api_path_set_is_frozen` 把 `api/*` 归一化后与白名单**逐条相等**；
/// REQ-162 重写时退成了「4 个存在 + 8 个不在」。这里恢复成集合比对：
/// 页面里**每一个** `/api/*` 路径都必须在这 6 条里，且 6 条一个不少、一个不多。
///
/// ⚠️ 为什么没有 `api/versions`：一屏界面不需要版本列表（`/api/state` 已带最新定版），
/// 它由 `test_req162_console::bundle_…` 的契约测试覆盖 —— 「页面不用」与「服务端没有」
/// 是两件事，这里冻结的是**页面消费的集合**。
#[test]
fn the_api_path_set_is_frozen() {
    const FROZEN: [&str; 6] = [
        "api/state",
        "api/draft",
        "api/decide",
        "api/finalize",
        "api/bundle",
        "api/bundle/download",
    ];
    let mut found: Vec<String> = Vec::new();
    let mut rest: &str = CONSOLE_HTML;
    while let Some(at) = rest.find("api/") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| {
                !(c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '_'
                    || c == '/'
                    || c == '.'
                    || c == '-')
            })
            .unwrap_or(tail.len());
        let path = tail[..end].trim_end_matches('/').to_string();
        if path != "api" && !found.contains(&path) {
            found.push(path);
        }
        rest = &rest[at + 4..];
    }
    found.sort();
    let mut want: Vec<String> = FROZEN.iter().map(|s| (*s).to_string()).collect();
    want.sort();
    assert_eq!(
        found, want,
        "页面里的 /api/* 路径集合变了：本地评审台页面只消费这 6 条纯文件端点（AC-002 / REQ-162 §五）"
    );
}

/// **五个危险动作都 guarded，且每个都写明「动作名 + 目标 + 后果」**。
///
/// 旧页面对应的是 `the_five_dangerous_actions_are_guarded`（`guarded({` 恰好 5 处）。
/// REQ-162 的五个动作是：定版 / 导出构建包 / 全选接受 / 全选拒绝 / 下载构建包 ——
/// 前四个会改状态或产出包，最后一个把包**带出工作目录**。
///
/// ⛔ 判据不是「`confirm(` 存在」：那会让「只留一句文案、不写目标/后果」也绿。
#[test]
fn the_five_dangerous_actions_are_guarded_with_target_and_consequence() {
    assert_eq!(
        CONSOLE_HTML.matches("guarded({").count(),
        5,
        "危险动作必须恰好 5 处 guarded({{…}})：定版 / 导出包 / 全选接受 / 全选拒绝 / 下载"
    );
    assert_eq!(
        CONSOLE_HTML.matches("action: \"").count(),
        5,
        "每个确认框都要写动作名"
    );
    assert_eq!(
        CONSOLE_HTML.matches("target: \"").count(),
        5,
        "每个确认框都要写目标"
    );
    assert_eq!(
        CONSOLE_HTML.matches("after: \"").count(),
        5,
        "每个确认框都要写后果"
    );
    // 逐条对：动作名 → 目标 / 后果（缺一个就红）。
    for (action, target, after) in [
        (
            "定版（冻结当前草稿）",
            "草稿 ",
            "定版后不可改，要改就再定一版（旧定版不跟着变）",
        ),
        (
            "导出构建包",
            "定版 v",
            "只导出定版、不导出草稿；包写进 .ontok/bundles/",
        ),
        (
            "全选接受",
            "全部 ",
            "一次写入 ",
        ),
        (
            "全选拒绝",
            "全部 ",
            "一次写入 ",
        ),        (
            "下载构建包",
            "定版 v",
            "把包复制到本机下载目录（不改工作目录里的任何文件）",
        ),
    ] {
        let at = CONSOLE_HTML
            .find(&format!("action: \"{action}\""))
            .unwrap_or_else(|| panic!("缺危险动作 {action:?} 的 guarded 规格"));
        let spec = &CONSOLE_HTML[at..(at + 420).min(CONSOLE_HTML.len())];
        assert!(
            spec.contains(&format!("target: \"{target}")),
            "{action:?} 的确认框没写目标（期望以 {target:?} 开头）：{spec}"
        );
        assert!(
            spec.contains(&format!("after: \"{after}")),
            "{action:?} 的确认框没写后果（期望以 {after:?} 开头）：{spec}"
        );
    }
    // ⛔ 只有一个 `window.confirm(` 调用点（guarded 的唯一实现），别在业务函数里再散落。
    assert_eq!(
        CONSOLE_HTML.matches("window.confirm(").count(),
        1,
        "确认必须只走 guarded() 一个入口"
    );
}

/// MED-2 · 批量动作的确认文案必须**写出会一次签多少条**（135 条不是小数目）。
#[test]
fn the_bulk_action_says_how_many_it_will_sign_at_once() {
    for action in ["全选接受", "全选拒绝"] {
        let at = CONSOLE_HTML
            .find(&format!("action: \"{action}\""))
            .unwrap_or_else(|| panic!("缺 {action:?} 的 guarded 规格"));
        let spec = &CONSOLE_HTML[at..(at + 420).min(CONSOLE_HTML.len())];
        assert!(
            spec.contains("\"全部 \" + list.length + \" 条"),
            "{action:?} 确认必须写出条数：{spec}"
        );
        assert!(
            spec.contains("一次写入 \" + list.length + \" 条"),
            "{action:?} 确认必须写出「一次写入 N 条裁决」：{spec}"
        );
    }
}
