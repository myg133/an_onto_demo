//! REQ-124 · 控制面 API 的**进程内判据**（不连库、不起 socket）。
//!
//! 覆盖：AC-001（401 / 403 / 200 / 免 token 的 health）、
//! AC-007（三份声明的读 + 保存前校验 + 失败不改原文件）、
//! AC-009（审计：写操作与失败各一行、无 token 明文）、
//! AC-003 的**请求契约**（缺 project / items / 两个来源同时给 ⇒ 400）。
//!
//! 「与 CLI 逐字节相同」「apply/freeze 的 409」需要真库，见
//! `tests/test_req124_live.rs`（缺环境变量时**响亮地跳过**）。
//!
//! 本文件**不连库**：服务用 `connect_lazy` 指向一个连不上的地址，
//! 于是「鉴权/声明校验在连库之前就答完」这两类判据不需要数据库；
//! 真会走到库里的端点只断言「不是 401/403」（即门已经过了）。

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::console::audit::AuditLog;
use ontok::console::decl::DeclPaths;
use ontok::console::tokens::AdminTokens;
use ontok::console::{ConsoleParts, ConsoleService};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::{sha256_hex, AuditSink};
use serde_json::Value;
use tower::ServiceExt;

const ADMIN: &str = "FAKE-ADMIN-TOKEN-aaaaaaaaaaaaaaaa";
const QUERY: &str = "FAKE-QUERY-TOKEN-bbbbbbbbbbbbbbbbbbbb";

const SCHEMA: &str = r#"{"tables":[
  {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true},
              {"name":"customer_name","data_type":"text"}]},
  {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
   "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
              {"name":"customer_id","data_type":"text"}]}
]}"#;

const POLICY_OK: &str = r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
  "perTable":{"orders":{"tenantColumn":"customer_id","reason":"订单归属客户"}},
  "publicDimensions":[]}"#;

const EXCLUSIONS_OK: &str = r#"{"version":"0.1.0","rule":"只排除非业务表","declared_by":"ba",
  "exclude":[{"table":"orders","category":"cdc_residue","reason":"这是同步残留的表","decided_by":"ba"}]}"#;

const TENANCY_OK: &str = r#"{"columns":["customer_id"],"reviewed_by":"安全负责人"}"#;

/// 一个临时目录（每个测试一份；目录名带 pid + 纳秒）。
fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req124-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn write_decls(dir: &PathBuf) -> DeclPaths {
    let paths = DeclPaths {
        exclusions: dir.join("exclusions.json"),
        policy: dir.join("query-policy.json"),
        tenancy: dir.join("tenancy.json"),
    };
    std::fs::write(&paths.exclusions, EXCLUSIONS_OK).unwrap();
    std::fs::write(&paths.policy, POLICY_OK).unwrap();
    std::fs::write(&paths.tenancy, TENANCY_OK).unwrap();
    paths
}

/// 一个**不连库**的服务：懒连接池 + 夹具 schema + 临时声明文件。
fn test_service(tag: &str) -> (Arc<ConsoleService>, DeclPaths) {
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
    let svc = Arc::new(ConsoleService::new(ConsoleParts {
        pool: sqlx::postgres::PgPoolOptions::new()
            // 夹具池**故意连不上**：判据要的是「鉴权/校验在连库之前就答完」。
            .acquire_timeout(std::time::Duration::from_millis(200))
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool"),
        project: "demo".into(),
        base: "https://ontok.local/".into(),
        tokens,
        schema,
        paths: paths.clone(),
        audit: AuditLog::new(AuditSink::stderr_only()),
        bind: "127.0.0.1:8899".parse().unwrap(),
        allow_public: false,
    }));
    (svc, paths)
}

async fn call(
    app: axum::Router,
    req: Request<Body>,
) -> (StatusCode, String) {
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读 body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn get(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::empty()).unwrap()
}

fn send(method: &str, uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 鉴权分离
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn health_needs_no_token_and_carries_no_data() {
    let (svc, _) = test_service("health");
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/health", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["service"], "ontok-console");
    assert_eq!(v["api"], "v1");
    assert!(v["version"].as_str().is_some());
    // ⛔ 零数据：项目名 / 版本 / 声明路径都不出去。
    for forbidden in ["demo", "exclusions", "query-policy", "127.0.0.1", ADMIN] {
        assert!(
            !body.contains(forbidden),
            "health 里出现了 {forbidden:?}（AC-001 要求零数据）：{body}"
        );
    }
}

#[tokio::test]
async fn no_token_is_401_everywhere_except_health() {
    let (svc, _) = test_service("401");
    for uri in [
        "/api/projects",
        "/api/review?proposal=p1",
        "/api/audit",
        "/api/config/policy",
        "/api/export/ttl?project=demo&version=1",
    ] {
        let (status, body) = call(
            ontok::console::http::build_router(Arc::clone(&svc)),
            get(uri, None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}: {body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["code"], "UNAUTHORIZED");
        assert!(!body.contains(ADMIN), "401 不许回显 token：{body}");
    }
    // POST 同样要 token
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send("POST", "/api/apply", None, r#"{"project":"demo","proposal":"p1"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_query_token_gets_403_not_401_on_management_endpoints() {
    let (svc, _) = test_service("403");
    for uri in [
        "/api/projects",
        "/api/audit",
        "/api/config/policy",
        "/api/export/ttl?project=demo&version=1",
    ] {
        let (status, body) = call(
            ontok::console::http::build_router(Arc::clone(&svc)),
            get(uri, Some(QUERY)),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {body}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["code"], "TOKEN_KIND_FORBIDDEN", "{uri}: {body}");
        assert!(v["hint"].as_str().unwrap().contains("admin"), "{uri}: {body}");
    }
    // 写端点同样 403（不是 401、也不是 400）。
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send("POST", "/api/freeze", Some(QUERY), r#"{"project":"demo","version":1}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn an_admin_token_passes_the_gate_but_cannot_be_used_as_a_query_token_here() {
    let (svc, _) = test_service("admin");
    // 管理端点：门已过（数据库连不上 ⇒ 500，但**不是** 401/403）。
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/projects", Some(ADMIN)),
    )
    .await;
    assert_ne!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_ne!(status, StatusCode::FORBIDDEN, "{body}");
    // 控制面没有查询端点 —— 管理 token 在这里**没有**第二条路（R1 的反向）。
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/sparql", Some(ADMIN)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "控制面不提供查询端点");
}

#[tokio::test]
async fn a_token_in_the_query_string_does_not_work() {
    let (svc, _) = test_service("qs");
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get(&format!("/api/projects?token={ADMIN}"), None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 带着正确的头但 query string 里也有 token ⇒ 头说了算；query 里的那个参数
    // 不认识的 ⇒ 400（可行动），⛔ 不被悄悄采用。
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get(
            &format!("/api/export/ttl?project=demo&version=1&token={ADMIN}"),
            Some(ADMIN),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("不生效") || body.contains("不要放"), "{body}");
}

#[tokio::test]
async fn unknown_paths_are_401_without_a_token_and_404_with_one() {
    let (svc, _) = test_service("404");
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/raw", None),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/raw", Some(ADMIN)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "NOT_FOUND");
    assert!(v["hint"].as_str().unwrap().contains("/api/config/"));
}

// ══════════════════════════════════════════════════════════════
// AC-007 · 声明读写 + 保存前校验（⛔ 失败不改原文件）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn config_get_returns_the_raw_bytes() {
    let (svc, paths) = test_service("cfg-get");
    for (kind, path) in [
        ("exclusions", &paths.exclusions),
        ("policy", &paths.policy),
        ("tenancy", &paths.tenancy),
    ] {
        let (status, body) = call(
            ontok::console::http::build_router(Arc::clone(&svc)),
            get(&format!("/api/config/{kind}"), Some(ADMIN)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}");
        assert_eq!(body, std::fs::read_to_string(path).unwrap(), "{kind} 必须是原文");
    }
    // 不认识的声明名 ⇒ 404 + 点名三份。
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/config/unknown", Some(ADMIN)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("exclusions") && body.contains("tenancy"), "{body}");
}

/// 六种非法各一条：每一条都必须 400、点名，且**原文件 sha256 不变**。
#[tokio::test]
async fn six_illegal_puts_are_rejected_and_leave_the_file_byte_identical() {
    let (svc, paths) = test_service("cfg-bad");
    let cases: [(&str, &str, &str); 6] = [
        // ① 引用不存在的表
        (
            "policy",
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
               "perTable":{"ghost_table":{"tenantColumn":"customer_id","reason":"合理的理由"}},
               "publicDimensions":[]}"#,
            "ghost_table",
        ),
        // ② main_only 但无 reason
        (
            "policy",
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
               "perTable":{"orders":{"tenantColumn":"customer_id","coverage":"main_only"}},
               "publicDimensions":[]}"#,
            "reason",
        ),
        // ③ publicDimensions 缺 reason
        (
            "policy",
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
               "publicDimensions":[{"table":"orders","reason":"   "}]}"#,
            "reason",
        ),
        // ④ 非法 JSON
        ("policy", "{not json", "不合法"),
        // ⑤ exclusions 的 category 不在枚举内
        (
            "exclusions",
            r#"{"version":"0.1.0","rule":"规则","declared_by":"ba",
               "exclude":[{"table":"orders","category":"invented","reason":"足够长的理由文字","decided_by":"ba"}]}"#,
            "category",
        ),
        // ⑥ tenancy 引用不存在的列
        (
            "tenancy",
            r#"{"columns":["ghost_column"]}"#,
            "ghost_column",
        ),
    ];
    for (kind, body, needle) in cases {
        let path = match kind {
            "policy" => &paths.policy,
            "exclusions" => &paths.exclusions,
            _ => &paths.tenancy,
        };
        let before = sha256_hex(&std::fs::read(path).unwrap());
        let (status, text) = call(
            ontok::console::http::build_router(Arc::clone(&svc)),
            send("PUT", &format!("/api/config/{kind}"), Some(ADMIN), body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{kind} 应拒绝：{text}");
        assert!(text.contains(needle), "拒绝要点名 {needle:?}：{text}");
        let after = sha256_hex(&std::fs::read(path).unwrap());
        assert_eq!(before, after, "⛔ 校验失败不许改动原文件（{kind}）");
    }
}

#[tokio::test]
async fn a_legal_put_saves_the_bytes_and_reports_the_sha256() {
    let (svc, paths) = test_service("cfg-ok");
    let new_policy = r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
      "perTable":{"orders":{"tenantColumn":"customer_id","coverage":"main_only",
                            "reason":"子账号产生的订单主账号看不到"}},
      "publicDimensions":[]}"#;
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send("PUT", "/api/config/policy", Some(ADMIN), new_policy),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["bytes"], new_policy.len());
    assert_eq!(v["sha256"], sha256_hex(new_policy.as_bytes()));
    assert_eq!(
        std::fs::read_to_string(&paths.policy).unwrap(),
        new_policy,
        "落盘的必须是**请求体原文**（逐字节）"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-009 · 审计（写操作 + 失败各一行，无 token 明文）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn audit_records_success_and_failure_without_the_token_plaintext() {
    let (svc, _) = test_service("audit");
    // 一次失败（400）+ 一次成功（200）。
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "PUT",
            "/api/config/policy",
            Some(ADMIN),
            r#"{"version":"1.0.0","tenantSubject":"ghost","tenantIdColumn":"id"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send("PUT", "/api/config/tenancy", Some(ADMIN), TENANCY_OK),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/audit?limit=10", Some(ADMIN)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{body}");
    assert_eq!(entries[0]["action"], "config");
    assert_eq!(entries[0]["result"], "DECLARATION_INVALID");
    assert_eq!(entries[1]["result"], "OK");
    for e in entries {
        let tok = e["token"].as_str().unwrap();
        assert!(tok.starts_with("sha256:"), "{e}");
        assert_eq!(tok.len(), "sha256:".len() + 12, "{e}");
        assert!(!body.contains(ADMIN), "⛔ 审计里出现了 token 明文：{body}");
        assert!(!body.contains(QUERY), "⛔ 审计里出现了 token 明文：{body}");
    }
    assert_eq!(entries[0]["target"], "config=policy");
    assert_eq!(entries[1]["target"], "config=tenancy");

    // 审计本身要 token；query token ⇒ 403。
    let (status, _) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/audit", Some(QUERY)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_auth_failure_is_audited_anonymously() {
    let (svc, _) = test_service("audit-auth");
    let _ = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/projects", None),
    )
    .await;
    let _ = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/projects", Some(QUERY)),
    )
    .await;
    let (_, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        get("/api/audit", Some(ADMIN)),
    )
    .await;
    let v: Value = serde_json::from_str(&body).unwrap();
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{body}");
    assert_eq!(entries[0]["action"], "auth");
    assert_eq!(entries[0]["result"], "UNAUTHORIZED");
    assert!(entries[0]["token"].is_null(), "401 那一条没有身份可记：{body}");
    assert_eq!(entries[1]["result"], "TOKEN_KIND_FORBIDDEN");
    assert!(entries[1]["token"].as_str().unwrap().starts_with("sha256:"));
}

// ══════════════════════════════════════════════════════════════
// AC-003 的请求契约（不连库就能判的部分）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn import_rejects_bad_requests_before_touching_the_database() {
    let (svc, _) = test_service("import-contract");
    // ① 两个来源同时给
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            r#"{"project":"demo","path":"a.json","proposal":{"items":[]}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("只能给一个"), "{body}");

    // ② 两个都不给
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send("POST", "/api/import", Some(ADMIN), r#"{"project":"demo"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("path") && body.contains("proposal"), "{body}");

    // ③ 内联提案缺 `project`
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            r#"{"project":"demo","proposal":{"items":[]}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("PROPOSAL_MISSING_PROJECT"), "{body}");

    // ④ 提案缺 `items`
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            r#"{"project":"demo","proposal":{"project":"demo"}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("PROPOSAL_MISSING_ITEMS"), "{body}");

    // ⑤ 请求体不是 JSON
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send("POST", "/api/import", Some(ADMIN), "{oops"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("JSON"), "{body}");

    // ⑥ 项目不是本服务的作用域
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            r#"{"project":"other","proposal":{"project":"other","items":[]}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("PROJECT_MISMATCH"), "{body}");
}

/// 变异哨兵：把 HTTP 层改成「自己建提案」而不是调 [`ontok::cli::import_proposal_core`]
/// 时，这一条会红 —— `importable: false` 的硬闸（AC-015）只存在于那个函数里。
#[tokio::test]
async fn import_goes_through_the_domain_function_so_the_importable_gate_still_fires() {
    let (svc, _) = test_service("import-gate");
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            r#"{"project":"demo","proposal":{"project":"demo","importable":false,"items":[]}}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("PROPOSAL_NOT_IMPORTABLE"), "{body}");
    assert!(body.contains("被拒绝档"), "要点名这是被拒绝档：{body}");
}

#[tokio::test]
async fn paths_that_do_not_exist_are_400_with_the_path_named() {
    let (svc, _) = test_service("import-file");
    let missing = std::env::temp_dir().join("ontok-req124-definitely-missing.json");
    let _ = std::fs::remove_file(&missing);
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            &format!(
                r#"{{"project":"demo","path":{}}}"#,
                serde_json::to_string(&missing.display().to_string()).unwrap()
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("IMPORT_FILE_NOT_FOUND"), "{body}");
    assert!(body.contains("definitely-missing"), "要点名路径：{body}");

    // 坏 JSON 的文件
    let dir = temp_dir("badjson");
    let bad = dir.join("bad.json");
    std::fs::write(&bad, "{not json").unwrap();
    let (status, body) = call(
        ontok::console::http::build_router(Arc::clone(&svc)),
        send(
            "POST",
            "/api/import",
            Some(ADMIN),
            &format!(
                r#"{{"project":"demo","path":{}}}"#,
                serde_json::to_string(&bad.display().to_string()).unwrap()
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("IMPORT_BAD_JSON"), "{body}");
}

// 反空转：`common` 模块必须被用到（否则本文件与仓里其他测试的纪律不一致）。
#[allow(dead_code)]
fn _crate_root_is_available() -> std::path::PathBuf {
    common::crate_root()
}
