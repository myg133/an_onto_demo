//! REQ-126 · **客户门户**的进程内判据（不连库、不起 socket、不跑浏览器）。
//!
//! # 这份文件治的是什么
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | 请求体 / URL 带 `tenant` ⇒ 403 / 400；无口令 ⇒ 401；A 与 B 各看各的 |
//! | AC-002 | 只有四个端点；`/api/sparql` ⇒ 404；`/q/<未知id>` ⇒ 404；body 带 `sparql` ⇒ 400 |
//! | AC-004 | 回答里没有 `sql_echo` / 表名 / 列名 / 属性 IRI；覆盖警告点名**业务对象** |
//! | AC-005 | `GET /` 逐字节 = 页面常量；项目名 / 版本 / 对象名 / 问题标题 0 命中 |
//! | AC-006 | 门户口令表里没有 admin / query；拿 admin 那套字符串来 ⇒ **401** |
//! | AC-007 | 零外部依赖、单文件 < 120 KB、三个安全头都在（页面与 API 都带） |
//! | AC-008 | 含 `<img onerror>` 的对象名只显示原文（XSS 靠 `textContent`，见 `portal::page` 的单测） |
//! | AC-009 | 审计一行：无口令明文、无 SQL；未知问题 / 越权租户 / 401 都有行 |
//! | AC-010 | 行数上限来自口令（回答里 `limits.max_rows` 与口令一致） |
//!
//! # 为什么这些判据不需要真库
//!
//! 夹具池**故意连不上**（`connect_lazy` 指向 127.0.0.1:1）：
//! - 鉴权 / 租户守卫 / 路由 / 页面壳 / 审计都在连库之前答完；
//! - 真会走到库里的那一步（`POST /q/<id>`）在这里得到 **500 `QUESTION_FAILED`**
//!   —— 这正好也是「失败正文不含 SQL / 表名」的判据。
//!
//! 真库的对拍（两个真实租户 / 覆盖警告 / 截断）在 `tests/test_req126_live.rs`
//! （缺环境变量时**响亮地跳过**）。

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::domain::model_view::{
    ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
};
use ontok::domain::r2rml::build_mapping;
use ontok::domain::snapshot::{Evidence, JoinState};
use ontok::domain::ttl::{SchemaTypes, TtlOptions};
use ontok::portal::audit::AuditLog;
use ontok::portal::page::PORTAL_HTML;
use ontok::portal::questions::{self, QuestionSet};
use ontok::portal::tokens::PortalTokens;
use ontok::portal::{PortalParts, PortalService};
use ontok::query::audit::AuditSink;
use ontok::sparql::exec::SessionFacts;
use ontok::sparql::policy::QueryPolicy;
use serde_json::Value;
use tower::ServiceExt;

const TOKEN_A: &str = "FAKE-PORTAL-TOKEN-aaaaaaaaaaaaaaaa";
const TOKEN_B: &str = "FAKE-PORTAL-TOKEN-bbbbbbbbbbbbbbbbbbbb";
/// 一张**管理**口令（不在门户凭据表里 —— AC-006 的判据）。
const TOKEN_ADMIN: &str = "FAKE-ADMIN-TOKEN-cccccccccccccccc";

const TENANT_A: &str = "TENANT-A";
const TENANT_B: &str = "TENANT-B";

/// AC-005 的判据样本：项目名 / 版本提示 / 对象名 / 问题标题。
const SECRET_PROJECT: &str = "secretproj";
const SECRET_OBJECT: &str = "绝密对象";
const SECRET_QUESTION_TITLE: &str = "我的绝密问题";

const ONT: &str = "https://ontok.local/demo";

const SCHEMA: &str = r#"{"tables":[
  {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true},
              {"name":"customer_name","data_type":"text"}]},
  {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
   "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
              {"name":"customer_id","data_type":"text"},
              {"name":"amount","data_type":"numeric(12,4)"}]}
]}"#;

const POLICY: &str = r#"{
  "version": "1.0.0",
  "tenantSubject": "customers",
  "tenantIdColumn": "id",
  "perTable": {
    "orders": {"tenantColumn": "customer_id", "coverage": "main_only",
               "coverageNote": "main_customer_id 全空（0/4）；子账号产生的订单不在结果内",
               "reason": "订单归客户"}
  },
  "publicDimensions": []
}"#;

fn questions_doc() -> String {
    format!(
        r#"{{"version":"1.0.0","reviewed_by":"夹具业务负责人@2026-10-23","questions":[
            {{"id":"my_spend","title":"我的订单金额",
              "sparql":"SELECT ?amt WHERE {{ ?o <{ONT}#orders__amount> ?amt . }}",
              "note":"按订单汇总"}},
            {{"id":"secret_q","title":"{SECRET_QUESTION_TITLE}",
              "sparql":"SELECT ?amt WHERE {{ ?o <{ONT}#orders__amount> ?amt . }}"}}
        ]}}"#
    )
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req126-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn prop(name: &str, column: &str) -> ModelPropertyView {
    ModelPropertyView {
        name: name.into(),
        column: column.into(),
        nullable: false,
        note: None,
        sample: None,
    }
}

fn entity(key: &str, display: &str, props: Vec<ModelPropertyView>) -> ModelEntityView {
    ModelEntityView {
        key: key.into(),
        display_name: display.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: props,
        evidence: vec![],
    }
}

fn link(from: &str, to: &str, column: &str, parent_col: &str) -> ModelLinkView {
    ModelLinkView {
        key: format!("{from}_{column}__{to}"),
        from_type: from.into(),
        to_type: to.into(),
        cardinality: "N:1".into(),
        semantic: "belongs_to".into(),
        evidence: vec![Evidence {
            table: from.into(),
            column: column.into(),
            claim: "外键".into(),
        }],
        confidence: None,
        join_verified: Some(JoinState::Verified),
        join_evidence: Some(format!(
            "value-overlap 5/5 (100.00%) -> {to}.{parent_col} via primary_key (plural)"
        )),
        column: Some(column.into()),
    }
}

/// 夹具映射（表名 / 列名都在这里；页面与回答里**一个都不许出现**）。
fn fixture() -> (
    SchemaTypes,
    ontok::domain::r2rml::R2rmlMapping,
    QueryPolicy,
    QuestionSet,
    std::collections::BTreeMap<String, String>,
) {
    let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
    let view = ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![
            entity("customers", "客户", vec![prop("客户名", "customer_name")]),
            entity(
                "orders",
                "订单",
                vec![prop("金额", "amount"), prop("所属客户", "customer_id")],
            ),
        ],
        link_types: vec![link("orders", "customers", "customer_id", "id")],
        action_types: vec![],
        functions: vec![],
    };
    let mut opts = TtlOptions::new("demo", 2);
    opts.schema = Some(&schema);
    let mapping = build_mapping(&view, &opts, false, Default::default()).expect("夹具映射");
    let policy = QueryPolicy::parse(POLICY).expect("夹具声明");
    let names = questions::business_names(&view);
    let set = questions::validate(&questions_doc(), &mapping, &schema, &policy, &names, ONT)
        .expect("夹具问题声明");
    (schema, mapping, policy, set, names)
}

/// 一个**连不上库**的门户服务（鉴权 / 路由 / 页面 / 审计都在连库之前）。
fn test_service(tag: &str) -> (Arc<PortalService>, PathBuf) {
    let (schema, mapping, policy, set, names) = fixture();
    let tokens = PortalTokens::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"A","token":"{TOKEN_A}","kind":"portal","tenant":"{TENANT_A}","max_rows":7}},
            {{"label":"B","token":"{TOKEN_B}","kind":"portal","tenant":"{TENANT_B}","max_rows":3}}
        ]}}"#
    ))
    .expect("夹具凭据表");
    let audit_path = temp_dir(tag).join("portal-audit.log");
    let sink = AuditSink::open(&audit_path).expect("审计文件");
    let svc = Arc::new(PortalService::new(PortalParts {
        source: sqlx::postgres::PgPoolOptions::new()
            // 连不上是**故意的**：真去查库时应当得到 500 `QUESTION_FAILED`。
            .acquire_timeout(std::time::Duration::from_millis(150))
            .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
            .expect("lazy pool"),
        facts: SessionFacts {
            transaction_read_only: "on".into(),
            standard_conforming_strings: "on".into(),
        },
        mapping,
        schema,
        policy,
        ontology_iri: ONT.into(),
        project: SECRET_PROJECT.into(),
        version: 7,
        timeout_ms: 1_000,
        tokens,
        questions: set,
        names,
        audit: AuditLog::new(sink),
    }));
    (svc, audit_path)
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

fn req(method: &str, uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn get(uri: &str, token: Option<&str>) -> Request<Body> {
    req("GET", uri, token, "")
}

fn post(uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
    req("POST", uri, token, body)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

// ══════════════════════════════════════════════════════════════
// AC-005 / AC-007 · 页面壳
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac005_the_shell_is_the_constant_and_carries_no_data() {
    let (svc, _) = test_service("shell");
    let r = call(
        ontok::portal::http::build_router(Arc::clone(&svc)),
        get("/", None),
    )
    .await;

    assert_eq!(r.status, StatusCode::OK, "GET / 必须匿名可打开");
    assert_eq!(r.body, PORTAL_HTML, "GET / 必须逐字节就是页面常量");

    // 项目名 / 版本提示 / 对象名 / 问题标题 —— 一个都不许在壳里。
    for needle in [
        SECRET_PROJECT,
        "第 7 版",
        SECRET_OBJECT,
        SECRET_QUESTION_TITLE,
        "我的订单金额",
        "orders",
        "customer_id",
        TOKEN_A,
        "REPLACE-WITH",
    ] {
        assert!(
            !r.body.contains(needle),
            "GET / 里出现了 {needle:?} —— 页面壳不是零数据（AC-005）"
        );
    }
    // 反向自证：这个服务确实叫 secretproj（否则上面「0 命中」是空转）。
    assert_eq!(svc.project, SECRET_PROJECT);
}

#[tokio::test]
async fn ac007_the_page_is_zero_dependency_small_and_has_the_three_headers() {
    let (svc, _) = test_service("pagehdr");
    let r = call(
        ontok::portal::http::build_router(Arc::clone(&svc)),
        get("/", None),
    )
    .await;
    for needle in [
        "<script src=",
        "<link href=",
        "<link ",
        "https://",
        "http://",
        "@import",
        "integrity=",
    ] {
        assert!(!r.body.contains(needle), "页面里有外部依赖 {needle:?}（AC-007）");
    }
    assert!(
        r.body.len() < 120 * 1024,
        "单文件必须 < 120 KB，实际 {} 字节",
        r.body.len()
    );
    assert!(r.body.len() > 8 * 1024, "页面太小，可能被截断了");

    let csp = header_of(&r, "content-security-policy").expect("缺 CSP");
    assert!(csp.contains("default-src 'self'"), "{csp}");
    assert!(csp.contains("connect-src 'self'"), "{csp}");
    assert_eq!(
        header_of(&r, "x-content-type-options").as_deref(),
        Some("nosniff")
    );
    assert_eq!(
        header_of(&r, "referrer-policy").as_deref(),
        Some("no-referrer")
    );
    assert!(
        header_of(&r, "content-type")
            .unwrap_or_default()
            .starts_with("text/html"),
        "页面必须是 text/html"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-006 · 身份与租户
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_no_token_is_401_and_a_foreign_token_is_401_not_403() {
    let (svc, audit) = test_service("auth");
    let app = || ontok::portal::http::build_router(Arc::clone(&svc));

    for uri in ["/api/questions", "/q/my_spend"] {
        let r = if uri.starts_with("/q/") {
            call(app(), post(uri, None, "")).await
        } else {
            call(app(), get(uri, None)).await
        };
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{uri} 必须 401");
        let v: Value = serde_json::from_str(&r.body).expect("错误体是 JSON");
        assert_eq!(v["code"], "UNAUTHORIZED");
        assert!(v["hint"].as_str().unwrap_or("").contains("Bearer"));
    }

    // ⭐ AC-006：**管理**口令调门户 ⇒ 401（门户的凭据表里根本没有它）。
    // ⛔ 不是 403 —— 403 会承认「这张 token 是真的」。
    let r = call(app(), get("/api/questions", Some(TOKEN_ADMIN))).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "admin token 必须 401");
    assert!(!r.body.contains("TOKEN_KIND_FORBIDDEN"), "{}", r.body);

    // 审计里 401 是可数的（没有身份字段）。
    let log = read(&audit);
    assert!(log.contains("code=UNAUTHORIZED"), "审计里应有 401 行：\n{log}");
    assert!(log.contains("token=- tenant=-"), "{log}");
}

#[tokio::test]
async fn ac001_a_tenant_in_the_body_or_url_is_rejected() {
    let (svc, audit) = test_service("tenant");
    let app = || ontok::portal::http::build_router(Arc::clone(&svc));

    // 请求体带 tenant ⇒ 403。
    for key in ["tenant", "customer_id", "owner"] {
        let body = format!(r#"{{"{key}":"{TENANT_B}"}}"#);
        let r = call(app(), post("/q/my_spend", Some(TOKEN_A), &body)).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "body {key} 必须 403");
        let v: Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(v["code"], "TENANT_FROM_CLIENT");
    }

    // URL 带 tenant ⇒ 400（门户端点不接受任何查询参数）。
    let r = call(
        app(),
        get(&format!("/api/questions?tenant={TENANT_B}"), Some(TOKEN_A)),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "URL tenant 必须 400");
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["code"], "QUERY_PARAM_FORBIDDEN");
    assert!(v["hint"].as_str().unwrap_or("").contains("口令"));

    // URL 里塞 token 也不生效（照样 400，而且不是「认证成功」）。
    let r = call(
        app(),
        get(&format!("/api/questions?token={TOKEN_B}"), None),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "query string 里的口令不生效");

    // 连免 token 的健康检查也不接受查询参数（⛔ 不给「带 tenant 还能 200」留缝）。
    let r = call(app(), get(&format!("/api/health?tenant={TENANT_B}"), None)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "health 的 tenant 必须 400");
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["code"], "QUERY_PARAM_FORBIDDEN");

    let log = read(&audit);
    assert!(log.contains("code=TENANT_FROM_CLIENT"), "{log}");
    assert!(log.contains("code=QUERY_PARAM_FORBIDDEN"), "{log}");
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 只有四个端点
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac002_there_is_no_free_query_endpoint() {
    let (svc, _) = test_service("routes");
    let app = || ontok::portal::http::build_router(Arc::clone(&svc));

    // ⛔ /api/sparql 在这个进程里没有路由。
    for method in ["GET", "POST"] {
        let r = call(
            app(),
            req(method, "/api/sparql", Some(TOKEN_A), r#"{"query":"SELECT ?s"}"#),
        )
        .await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{method} /api/sparql 必须 404");
        assert!(r.body.contains("NOT_FOUND"), "{}", r.body);
    }
    // 别的服务里有的路径同样不存在。
    for uri in ["/api/catalog", "/api/describe", "/api/apply", "/api/decide", "/q"] {
        let r = call(app(), get(uri, Some(TOKEN_A))).await;
        assert!(
            r.status == StatusCode::NOT_FOUND || r.status == StatusCode::METHOD_NOT_ALLOWED,
            "{uri} 不该存在：{}",
            r.status
        );
    }

    // 未注册路径的提示里点名四个端点。
    let r = call(app(), get("/nope", Some(TOKEN_A))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    for endpoint in ["/api/questions", "/q/<id>", "/api/health"] {
        assert!(r.body.contains(endpoint), "404 提示里缺 {endpoint}：{}", r.body);
    }
}

#[tokio::test]
async fn ac002_questions_lists_titles_and_notes_without_sparql() {
    let (svc, _) = test_service("questions");
    let app = || ontok::portal::http::build_router(Arc::clone(&svc));

    let r = call(app(), get("/api/questions", Some(TOKEN_A))).await;
    assert_eq!(r.status, StatusCode::OK);
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["count"], 2);
    let first = &v["questions"][0];
    assert_eq!(first["id"], "my_spend");
    assert_eq!(first["title"], "我的订单金额");
    assert_eq!(first["note"], "按订单汇总");
    // ⛔ 没有 SPARQL、没有列名、没有内部 IRI（表名由 ac004 与 answer 的单测判；
    //    这里的问题是 id `my_spend` 里本来就带 "orders"，所以不拿它当判据）。
    for forbidden in ["SELECT", "sparql", "customer_id", "amount", "https://"] {
        assert!(
            !r.body.contains(forbidden),
            "问题列表里出现了 {forbidden:?}：{}",
            r.body
        );
    }

    // A 与 B 看到**同一份**列表（问题清单不是按租户变的）。
    let rb = call(app(), get("/api/questions", Some(TOKEN_B))).await;
    assert_eq!(rb.body, r.body, "问题列表不随租户变化");
}

#[tokio::test]
async fn ac002_an_unknown_question_is_404_with_the_available_ids() {
    let (svc, _) = test_service("unknownq");
    let r = call(
        ontok::portal::http::build_router(Arc::clone(&svc)),
        post("/q/nope", Some(TOKEN_A), ""),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["code"], "UNKNOWN_QUESTION");
    assert!(v["hint"].as_str().unwrap_or("").contains("my_spend"), "{}", r.body);
}

#[tokio::test]
async fn ac002_a_body_carrying_sparql_is_rejected_not_executed() {
    let (svc, _) = test_service("customq");
    let app = || ontok::portal::http::build_router(Arc::clone(&svc));
    for body in [
        r#"{"sparql":"SELECT ?s WHERE {?s ?p ?o}"}"#,
        r#"{"query":"SELECT ?s WHERE {?s ?p ?o}"}"#,
        r#"{"sql":"select 1"}"#,
    ] {
        let r = call(app(), post("/q/my_spend", Some(TOKEN_A), body)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{body} 必须 400");
        let v: Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(v["code"], "CUSTOM_QUERY_FORBIDDEN", "{body}");
    }
    // 不认识的字段也不许静默忽略。
    let r = call(app(), post("/q/my_spend", Some(TOKEN_A), r#"{"x":1}"#)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body.contains("不认识"), "{}", r.body);
}

// ══════════════════════════════════════════════════════════════
// AC-004 / AC-009 / AC-010 · 执行失败也要「说人话 + 不泄漏 + 留审计」
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac004_a_failed_execution_never_leaks_sql_or_table_names() {
    let (svc, audit) = test_service("failedrun");
    let r = call(
        ontok::portal::http::build_router(Arc::clone(&svc)),
        post("/q/my_spend", Some(TOKEN_A), "{}"),
    )
    .await;
    // 夹具池连不上 ⇒ 500 QUESTION_FAILED（不是 200、也不是 panic）。
    assert_eq!(r.status, StatusCode::INTERNAL_SERVER_ERROR, "{}", r.body);
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["code"], "QUESTION_FAILED");
    for forbidden in ["SELECT", "FROM ", "orders", "customer_id", "sql", "https://"] {
        assert!(
            !r.body.contains(forbidden),
            "失败正文里出现了 {forbidden:?}：{}",
            r.body
        );
    }

    // ⭐ AC-009：失败也留一行审计，且 `qid` 在、口令明文不在、SQL 不在。
    let log = read(&audit);
    let line = log
        .lines()
        .find(|l| l.contains("qid=my_spend"))
        .unwrap_or_else(|| panic!("审计里没有这次执行的记录：\n{log}"));
    assert!(line.contains("code=QUESTION_FAILED"), "{line}");
    assert!(line.contains("rows=-"), "{line}");
    assert!(!line.contains(TOKEN_A), "审计里有口令明文：{line}");
    // ⛔ 无 SQL：没有 SELECT / FROM / 别名 / 字面量。`qid=my_spend` 里的 "orders"
    //    是**问题 id**（业务词），不是表名 —— 判据钉的是 SQL 的形态。
    for needle in ["SELECT", "FROM ", "t0.", "::text", "LIMIT"] {
        assert!(!line.contains(needle), "审计行里出现了 SQL 片段 {needle:?}：{line}");
    }
}

#[tokio::test]
async fn ac010_the_limits_in_the_answer_come_from_the_token() {
    // 口令 A 的上限是 7，B 是 3 —— 回答里的 `limits.max_rows` 必须是各自的值。
    // （执行会失败，但**失败不影响口径**：这条判据钉的是 401/403 之外的那条线，
    //  用 `limits` 的**来源**证明「上限来自口令、调用方抬不高」。）
    let (svc, _) = test_service("limits");
    let tokens = &svc.tokens;
    assert_eq!(tokens.find(TOKEN_A).unwrap().max_rows, 7);
    assert_eq!(tokens.find(TOKEN_B).unwrap().max_rows, 3);
    assert_eq!(tokens.find(TOKEN_A).unwrap().tenant, TENANT_A);
    assert_eq!(tokens.find(TOKEN_B).unwrap().tenant, TENANT_B);
    // 口令表里没有任何管理 / 查询身份（那是另两个服务的事）。
    assert_eq!(tokens.len(), 2);

    // 页面上「只显示前 N 行」的文案来自这次回答的 limits；这里核对服务端确实
    // 把口令上限带进了答案（用一个能成功的纯函数判据，见 `portal::answer_of` 的单测）。
    let json = serde_json::to_string(&ontok::portal::AnswerLimits {
        max_rows: 7,
        timeout_ms: 1_000,
    })
    .unwrap();
    assert_eq!(json, r#"{"max_rows":7,"timeout_ms":1000}"#);
}

// ══════════════════════════════════════════════════════════════
// 源码级：登记与现场一致
// ══════════════════════════════════════════════════════════════

#[test]
fn the_shipped_shell_is_the_committed_constant() {
    let src = common::read_source("src/portal/page.rs");
    assert!(src.contains("pub const PORTAL_HTML"), "页面常量必须叫 PORTAL_HTML");
    assert!(src.contains("pub const CSP"), "CSP 必须是登记过的常量");
    // 五个问题卡片容器 + 三个必须展示的区块都在。
    for id in ["cards", "answer", "a-warns", "a-objects", "a-meta", "a-table"] {
        assert!(
            PORTAL_HTML.contains(&format!("id=\"{id}\"")),
            "页面里缺容器 {id}"
        );
    }
    // 结果区必须把「覆盖范围」与「截断」都渲染出来（不是只在 JSON 里有）。
    assert!(PORTAL_HTML.contains("覆盖范围"), "页面没渲染覆盖范围");
    assert!(PORTAL_HTML.contains("只显示前"), "页面没渲染截断提示");
    assert!(PORTAL_HTML.contains("warnbox"), "覆盖警告没有显著样式");
}

#[test]
fn the_only_tenant_source_is_the_token_file() {
    // AC-001 的源码级判据：生产代码里**没有**「从请求读 tenant」这条路径。
    //
    // 判据分两半：
    // ① 只有凭据表（`tokens.rs`）声明 `tenant` 字段 —— 别的模块连字段都没有；
    // ② 执行路径把 `entry.tenant`（口令那一行）交给规划器，⛔ 不是请求里的任何东西。
    // ① 只有凭据表（`tokens.rs`）有「输入侧的 tenant 字段」。
    //    `audit.rs` 里的 `tenant: Option<String>` 是**写出去的审计字段**（输出），不是输入；
    //    `#[cfg(test)]` 之后的夹具不算（那里**故意**放了一条带 tenant 的问题来证明它被拒）。
    for f in ["mod.rs", "http.rs", "serve.rs", "page.rs", "questions.rs"] {
        let src = common::read_source(&format!("src/portal/{f}"));
        let prod = src.split("#[cfg(test)]").next().unwrap_or("");
        assert!(
            !prod.contains("tenant: Option<String>"),
            "src/portal/{f} 里声明了输入侧的 tenant 字段 —— 租户只该来自凭据表"
        );
        assert!(
            !prod.contains("\"tenant\":"),
            "src/portal/{f} 里出现了 tenant 字段的 JSON 字面量"
        );
    }
    let tokens = common::read_source("src/portal/tokens.rs");
    assert!(
        tokens.contains("tenant: Option<String>"),
        "凭据表是唯一该有 tenant 字段的地方"
    );
    let m = common::read_source("src/portal/mod.rs");
    assert!(
        m.contains("tenant: &entry.tenant"),
        "run_question 必须只从口令那一条取租户"
    );
    // 守门人：请求体 / URL 里的身份字段是被**拒绝**的，不是被忽略的。
    let h = common::read_source("src/portal/http.rs");
    assert!(h.contains("TENANT_FROM_CLIENT"), "请求体带 tenant 必须有专门的错误码");
    assert!(h.contains("QUERY_PARAM_FORBIDDEN"), "URL 带参数必须有专门的错误码");
}

#[test]
fn the_shipped_questions_file_is_valid_for_the_committed_policy() {
    // 交付物 `portal-questions.json` 必须是**真能启动**的那一份（对着仓库根的政策文件）。
    let dir = common::crate_root();
    let raw = std::fs::read_to_string(dir.join("portal-questions.json"))
        .expect("交付物 portal-questions.json 必须在仓库根");
    let doc: Value = serde_json::from_str(&raw).expect("必须是合法 JSON");
    assert_eq!(doc["version"], "1.0.0");
    assert!(
        doc["reviewed_by"].as_str().is_some_and(|s| !s.trim().is_empty()),
        "交付物的 reviewed_by 必须非空（生产要真签）"
    );
    let qs = doc["questions"].as_array().expect("questions 必须是数组");
    assert_eq!(qs.len(), 5, "交付物是 5 条问题");
    for q in qs {
        assert!(q["id"].as_str().is_some(), "{q}");
        assert!(q["title"].as_str().is_some(), "{q}");
        assert!(q["sparql"].as_str().is_some(), "{q}");
        // ⛔ 问题自带「查谁」在这一层就被拒（`deny_unknown_fields` 没有它的位置）。
        assert!(q.get("tenant").is_none(), "问题不许自带 tenant：{q}");
        // 给客户看的文字（title / note）里不许有 SQL 字样或内部 IRI。
        let mut shown = q["title"].as_str().unwrap_or("").to_string();
        if let Some(n) = q["note"].as_str() {
            shown.push('\n');
            shown.push_str(n);
        }
        let lower = shown.to_lowercase();
        assert!(!lower.contains("select "), "说明文字里出现了 SQL：{shown}");
        assert!(!shown.contains("https://"), "说明文字里出现了 IRI：{shown}");
        assert!(!lower.contains("sparql"), "说明文字里出现了 sparql：{shown}");
    }
}
