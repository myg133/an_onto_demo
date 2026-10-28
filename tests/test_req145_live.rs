//! REQ-145 · **真库实测**：TTL `sourceKind` + 查询 `@pending` + `--access-mapping`。
//!
//! # 为什么必须连真库
//!
//! 「哪些类来自 API」是按**本体里的类 key** 对账的（映射文件写错了类名 ⇒ 拒绝启动），
//! 而类 key / 主键列 / 关系名只在真实快照 + 真实 schema 上才成立：
//! - `customers` 是**有 FK 汇聚的真实类**（40 条边指向它）⇒ 跨源边是**推出来的**，不是夹具造的；
//! - `order → customers` 是**真实存在**的边（`order_customer_id__customers`）；
//! - 主键列 `id` 决定 `@pending[].get.filter.ids` 能不能从查询里认出来。
//!
//! 合成夹具能证明的只有「形状对」，证明不了「与这份本体对得上」。
//!
//! # 缺环境变量时**响亮地跳过**（REQ-131）
//!
//! | 变量 | 用途 |
//! | --- | --- |
//! | `ONTOK_DATABASE_URL`（或 `ONTOK_TEST_DATABASE_URL`） | 元数据库（项目 / 版本 / 快照） |
//! | `ONTOK_SOURCE_DATABASE_URL` | 源库（只读） |
//! | `ONTOK_REQ123_SCHEMA`（或 `ONTOK_REQ145_SCHEMA`） | `schema.json` 路径 |
//! | `ONTOK_REQ123_TENANT`（或 `ONTOK_REQ145_TENANT`） | 一个存在的租户 id |
//!
//! # 本文件钉住的 AC
//!
//! AC-001（TTL 两视图只标 api 类、零接口细节）/ AC-002（不给映射零变化）/
//! AC-003（`access`）/ AC-004（`@pending` + `complete:false`）/ AC-005（rows 报错）/
//! AC-006（跨源边不下发 `get` + 遍历报错）/ AC-007（`complete == pending.is_empty()`）/
//! AC-008（`mapping_version` 有/无）。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（与 REQ-122/123/127 的现场判据同一口径）。

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::query::source_gate::AccessMapping;
use ontok::query::tokens::TokenTable;
use ontok::query::ttl::{OntologyTtl, TtlSource};
use ontok::query::{http, QueryService, ServiceParts};
use serde_json::Value;
use tower::ServiceExt;

const TOK: &str = "FAKE-LIVE-TOKEN-req145-aaaaaaaa";

/// api 类：`customers`（真实本体里 40 条边指向它 ⇒ 天然存在跨源边）。
const API_CLASS: &str = "customers";
/// 库源类：`order`（与 `customers` 之间**真的有边** `order__customers`）。
const DB_CLASS: &str = "order";
/// 跨源边（前向谓词本地名）。
const CROSS_EDGE: &str = "order__customers";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn env2(a: &str, b: &str) -> Option<String> {
    env(a).or_else(|| env(b))
}

fn meta_url() -> Option<String> {
    env("ONTOK_DATABASE_URL").or_else(|| env("ONTOK_TEST_DATABASE_URL"))
}

fn schema_path() -> Option<String> {
    env2("ONTOK_REQ145_SCHEMA", "ONTOK_REQ123_SCHEMA")
}

fn tenant() -> Option<String> {
    env2("ONTOK_REQ145_TENANT", "ONTOK_REQ123_TENANT")
}

/// 四件夹具齐了吗？缺**第一件**就响亮地说一句然后返回 `false`（一条用例一行标记）。
///
/// 唯一实现是 [`common::skip::missing`]（REQ-131）：默认打 `*** SKIP: … ***` 后正常返回，
/// `ONTOK_REQUIRE_FIXTURES=1` 时**失败**（CI 用）。
fn fixtures_ready(test: &str) -> bool {
    let missing: Option<&str> = if env("ONTOK_SOURCE_DATABASE_URL").is_none() {
        Some("ONTOK_SOURCE_DATABASE_URL")
    } else if meta_url().is_none() {
        Some("ONTOK_DATABASE_URL")
    } else if schema_path().is_none() {
        Some("ONTOK_REQ123_SCHEMA")
    } else if tenant().is_none() {
        Some("ONTOK_REQ123_TENANT")
    } else {
        None
    };
    match missing {
        Some(name) => {
            common::skip::missing(name, test);
            false
        }
        None => true,
    }
}

/// 真库用例串行闸（每个用例自己建一个只读池；源库代理对超额连接会重置 —— 见 REQ-123）。
static LIVE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    LIVE_GATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// 映射文件（**最小形状**；`entities[0].class` 必须是真实类 key，否则 bind 会拒绝）。
fn access_json(mapping_version: u64) -> String {
    format!(
        r#"{{"contract_version":1,"mapping_version":{mapping_version},
            "entities":[{{"id":"Customer","class":"customers",
                          "capability":{{"pushdown":["ids"]}},"freshness":"realtime"}}]}}"#
    )
}

/// 建一个查询服务。`access = None` ⇒ **与基线同一条路**（R1）。
///
/// ⚠️ 两视图 TTL 走**生产入口** `OntologyTtl::prepare`（与 `serve` 同一段）：判据要验的
/// 就是「服务真发出去的那份字节」，⛔ 不在测试里另拼一份。
async fn build_service(access: Option<&str>) -> Option<Arc<QueryService>> {
    build_service_with(access, None).await
}

/// 同上，但可把审计落到**文件**（R-2 的判据要读回审计行）。
async fn build_service_with(
    access: Option<&str>,
    audit_path: Option<&std::path::Path>,
) -> Option<Arc<QueryService>> {
    let meta = meta_url()?;
    let source = env("ONTOK_SOURCE_DATABASE_URL")?;
    let schema_path = schema_path()?;
    let tenant_id = tenant()?;
    let project = env("ONTOK_REQ145_PROJECT").unwrap_or_else(|| "anspire_r2rml_excl".into());
    let version: i32 = env("ONTOK_REQ145_VERSION").and_then(|v| v.parse().ok()).unwrap_or(2);

    let pool = match ontok::db::pool::connect(&meta).await {
        Ok(p) => p,
        Err(e) => {
            common::skip::missing(&format!("可连的 ONTOK_DATABASE_URL（{e}）"), "build_service");
            return None;
        }
    };
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    let store = ontok::db::snapshot_store::PgStore::new(pool.clone());
    let p = projects.get_by_name(&project).await.ok().flatten()?;
    let v = projects.version_by_seq(p.id, &p.name, version).await.ok()?;
    assert!(v.frozen_at.is_some(), "真库实测要求该版本已冻结（与 serve 同一条闸门）");
    let snap = store.get_snapshot(v.id).await.ok().flatten()?;

    let raw = std::fs::read_to_string(&schema_path).expect("schema 文件可读");
    let schema = ontok::domain::ttl::SchemaTypes::parse(&raw).expect("schema 合法");
    let raw = std::fs::read_to_string(common::crate_root().join("query-policy.json"))
        .expect("query-policy.json 可读");
    let policy = ontok::sparql::policy::QueryPolicy::parse(&raw).expect("声明合法");

    let opts = ontok::domain::ttl::TtlOptions {
        project: project.as_str(),
        version,
        base: "https://ontok.local/",
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema),
        api_classes: None,
    };
    let model = ontok::domain::model_view::project_model_track(&snap);
    let mapping = ontok::domain::r2rml::build_mapping(
        &model,
        &opts,
        false,
        ontok::domain::r2rml::R2rmlAudit::default(),
    )
    .expect("映射必须能生成（否则 schema 与快照对不上）");
    let ontology_iri = opts.ontology_iri();

    // ⭐ REQ-145 · 与 `query serve` 同一条加载路径：parse → bind（对账）。
    let access = match access {
        Some(raw) => Some(
            AccessMapping::parse(raw)
                .expect("映射文件形状合法")
                .bind(&model)
                .expect("映射里的类必须在这一版本体里"),
        ),
        None => None,
    };

    // ⭐ REQ-145 · 两视图走生产入口（`prepare`），api 类由 `TtlSource` 带进去。
    let schema_file = std::path::PathBuf::from(&schema_path);
    let ttl = OntologyTtl::prepare(
        &pool,
        &TtlSource {
            project: &project,
            version,
            base: "https://ontok.local/",
            schema: Some(schema_file.as_path()),
            tenancy: None,
            api_classes: access.as_ref().map(|a| a.api_classes()),
        },
    )
    .await
    .expect("两视图 TTL 必须能渲染（serve 在这句上 fail-closed）");

    let src = ontok::sparql::exec::connect_read_only(&source)
        .await
        .expect("源库必须能连上且只读");

    let tokens = TokenTable::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"a","token":"{TOK}","tenant":"{tenant_id}","max_rows":200,"max_concurrency":2}}]}}"#
    ))
    .expect("夹具 token 表");

    Some(Arc::new(QueryService::new(ServiceParts {
        mode: ontok::query::ServiceMode::Query,
        source: Some(src.pool.clone()),
        facts: Some(src.facts.clone()),
        mapping: Some(mapping),
        model,
        schema,
        policy,
        ontology_iri,
        project,
        version,
        timeout_ms: 10_000,
        sse_idle_timeout_ms: 2_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        tokens,
        audit: match audit_path {
            // R-2 的判据要读回审计行 ⇒ 落到文件（stdout 照样也写，与生产同一条 `record`）。
            Some(path) => ontok::query::audit::AuditSink::open(path)
                .expect("审计文件可写（判据自己建的临时目录）"),
            None => ontok::query::audit::AuditSink::stdout_only(),
        },
        // ⭐ REQ-178 · 判据验的是「服务发出去的字节」⇒ 预渲染一对（渲染时机由
        // `test_req178_ttl_on_demand` 钉）。
        ttl: ontok::query::ttl::TtlCache::preloaded(ttl),
        access,
    })))
}

async fn rest(
    svc: &Arc<QueryService>,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, String) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOK}"));
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    let req = b
        .body(body.map(|s| Body::from(s.to_string())).unwrap_or_else(Body::empty))
        .unwrap();
    let res = http::build_router(Arc::clone(svc)).oneshot(req).await.expect("router");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 16 * 1024 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

/// 一条 SPARQL（`format` 显式写：本需求的判定**不看**格式，但两种格式的表达不同）。
async fn sparql(svc: &Arc<QueryService>, format: &str, query: &str) -> (StatusCode, Value) {
    let body = serde_json::json!({ "query": query }).to_string();
    let (status, text) = rest(svc, "POST", &format!("/api/sparql?format={format}"), Some(&body)).await;
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("不是 JSON（{e}）：{text}"));
    (status, v)
}

fn api_class_query() -> String {
    format!("SELECT ?c ?n WHERE {{ ?c a :{API_CLASS} . ?c :{API_CLASS}__customer_name ?n }}")
}

fn db_class_query() -> String {
    format!("SELECT ?o WHERE {{ ?o a :{DB_CLASS} }} LIMIT 3")
}

fn empty_result_query() -> String {
    // 库源类 + 一个不可能命中的字面量 ⇒ 0 行，但**完整**（`complete: true`）。
    format!("SELECT ?o WHERE {{ ?o a :{DB_CLASS} . ?o :{DB_CLASS}__id \"__no_such_id__\" }}")
}

// ══════════════════════════════════════════════════════════════
// AC-001 · TTL 两视图只标 api 类，且零接口细节
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_ttl_marks_api_classes_in_both_views_and_leaks_nothing() {
    let _g = one_at_a_time();
    let test = "ac001_ttl_marks_api_classes_in_both_views_and_leaks_nothing";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(Some(&access_json(2))).await.expect("服务可构建");

    // ① HTTP 上的两视图（服务真发出去的那份字节）。
    let (status, customer) = rest(&svc, "GET", "/api/ontology.ttl", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, model) = rest(&svc, "GET", "/api/ontology.ttl?view=model", None).await;
    assert_eq!(status, StatusCode::OK);

    for (label, doc) in [("客户视图", &customer), ("建模视图", &model)] {
        assert_eq!(
            doc.matches("ontok:sourceKind").count(),
            1,
            "{label}：只有 api 类写 sourceKind（库源默认 db，不写）\n{doc}"
        );
        assert!(
            doc.contains("ontok:sourceKind \"api\""),
            "{label} 缺 ontok:sourceKind \"api\""
        );
    }

    // ② 库源类**没有**该谓词：`order` 的类块里不能出现它。
    let order_block = model
        .split(&format!(":{DB_CLASS} a owl:Class"))
        .nth(1)
        .and_then(|s| s.split(" .\n").next())
        .unwrap_or_else(|| panic!("模型视图必须有 :{DB_CLASS} 类块"));
    assert!(
        !order_block.contains("sourceKind"),
        "库源类不该有 sourceKind：{order_block}"
    );

    // ③ ⛔ TTL 里不许出现映射的任何接口细节（端点 / 参数 / 凭据 / 契约条目）。
    //
    // ⚠️ 词表只收**映射文件里的**那些（`api_key` / `endpoint` / `operation` 这类词在
    // 建模视图里**本来就存在** —— 它们是真实列名/注释，不是本需求漏出去的）。
    for needle in [
        "tencent", "DescribeCustomers", "CustomerSet", "pushdown", "realtime", "reviewed_by",
        "iterator", "max_scan_rows", "access#", "@v2",
    ] {
        for (label, doc) in [("客户视图", &customer), ("建模视图", &model)] {
            assert!(
                !doc.contains(needle),
                "{label} 里漏了映射细节 {needle:?} —— TTL 只能有「是不是 api」这一个事实"
            );
        }
    }
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 不给映射 ⇒ 零变化
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac002_without_mapping_nothing_new_appears() {
    let _g = one_at_a_time();
    let test = "ac002_without_mapping_nothing_new_appears";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(None).await.expect("服务可构建");
    assert!(svc.access.is_none(), "没给映射 ⇒ 服务里就没有映射");

    // ① TTL 两视图：没有 sourceKind（与基线逐字节相同的必要条件）。
    for uri in ["/api/ontology.ttl", "/api/ontology.ttl?view=model"] {
        let (status, doc) = rest(&svc, "GET", uri, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!doc.contains("sourceKind"), "没加载映射却出现 sourceKind（{uri}）");
    }

    // ② catalog / describe：没有 access 字段。
    let (_, catalog) = rest(&svc, "GET", "/api/catalog?page=1&page_size=200", None).await;
    assert!(!catalog.contains("\"access\""), "没加载映射却出现 access（catalog）");
    let (_, describe) = rest(&svc, "GET", "/api/describe?name=:customers", None).await;
    assert!(!describe.contains("\"access\""), "没加载映射却出现 access（describe）");

    // ③ SPARQL 两模式：没有 @pending / complete / unresolved_classes / mapping_version。
    //
    // ⚠️ 按**字段**判，⛔ 不做子串匹配：`coverage[].coverage` 的取值就叫 `"complete"`
    // （REQ-123 的既有字段），子串匹配会把它误判成新字段。
    let (status, jsonld) = sparql(&svc, "jsonld", &api_class_query()).await;
    assert_eq!(status, StatusCode::OK, "没加载映射 ⇒ customers 就是普通库源查询：{jsonld}");
    assert!(jsonld.get("@pending").is_none(), "没加载映射却出现 @pending：{jsonld}");
    let meta = &jsonld["meta"];
    for needle in ["complete", "unresolved_classes", "mapping_version"] {
        assert!(
            meta.get(needle).is_none(),
            "没加载映射却出现 meta.{needle}：{jsonld}"
        );
    }
    let (status, rows) = sparql(&svc, "rows", &api_class_query()).await;
    assert_eq!(status, StatusCode::OK);
    for needle in ["complete", "unresolved_classes"] {
        assert!(
            rows.get(needle).is_none(),
            "行回执不该有 {needle}：{rows}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// AC-003 · catalog / describe 的 access
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac003_describe_reports_access_for_api_and_db_classes() {
    let _g = one_at_a_time();
    let test = "ac003_describe_reports_access_for_api_and_db_classes";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(Some(&access_json(2))).await.expect("服务可构建");

    let (status, api) = rest(&svc, "GET", &format!("/api/describe?name=:{API_CLASS}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let api: Value = serde_json::from_str(&api).expect("JSON");
    let access = &api["access"];
    assert_eq!(access["kind"], "api", "{api}");
    assert_eq!(access["queryable_by"][0], "fetch");
    assert_eq!(access["pushdown_fields"][0], "ids");
    assert_eq!(access["freshness"], "realtime");

    let (status, db) = rest(&svc, "GET", &format!("/api/describe?name=:{DB_CLASS}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let db: Value = serde_json::from_str(&db).expect("JSON");
    assert_eq!(db["access"]["kind"], "db", "{db}");
    assert_eq!(db["access"]["queryable_by"][0], "sparql");
    assert!(
        db["access"].get("pushdown_fields").is_none(),
        "库源没有「下推字段」这回事：{}",
        db["access"]
    );

    // catalog 里也必须带（同一个判定，不另算一遍）。
    let (_, catalog) = rest(&svc, "GET", "/api/catalog?page=1&page_size=200", None).await;
    let catalog: Value = serde_json::from_str(&catalog).expect("JSON");
    let item = catalog["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|i| i["kind"] == "class" && i["name"] == API_CLASS)
        .unwrap_or_else(|| panic!("catalog 里必须有 api 类：{catalog}"));
    assert_eq!(item["access"]["kind"], "api", "{item}");
}

// ══════════════════════════════════════════════════════════════
// AC-004 · JSON-LD：@graph: [] + @pending + complete:false
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac004_api_class_query_returns_pending_and_an_empty_graph() {
    let _g = one_at_a_time();
    let test = "ac004_api_class_query_returns_pending_and_an_empty_graph";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(Some(&access_json(2))).await.expect("服务可构建");
    let (status, v) = sparql(&svc, "jsonld", &api_class_query()).await;
    assert_eq!(status, StatusCode::OK, "jsonld 不是报错：半成品 + 说明书：{v}");

    // ① `@graph` 恒空（规则 7：整类在 API 源不是错误）。
    assert_eq!(v["@graph"], serde_json::json!([]), "{v}");
    // ② `@pending` 一条，且**不在** `@graph` 里（规则 2：结构上就是两个字段）。
    let pending = v["@pending"].as_array().expect("@pending 是数组");
    assert_eq!(pending.len(), 1, "{v}");
    assert_eq!(pending[0]["@type"], format!("ontok:{API_CLASS}"));
    assert_eq!(pending[0]["ontok:sourceKind"], "api");
    assert_eq!(pending[0]["reason"], "source_is_api");
    // ③ `meta.complete: false` + `unresolved_classes`
    assert_eq!(v["meta"]["complete"], false, "{v}");
    assert_eq!(v["meta"]["unresolved_classes"], serde_json::json!([API_CLASS]));
    assert_eq!(v["meta"]["mapping_version"], 2);
    // ④ `get` 四件套（含 `@v<version>`）
    let get = &pending[0]["get"];
    assert_eq!(get["class"], API_CLASS);
    assert_eq!(get["pushdown"], serde_json::json!(["ids"]));
    assert_eq!(get["ref"], "access#Customer@v2");
    assert!(get["filter"].is_object(), "filter 恒是对象（空对象也合法）");

    // ⑤ 带实例标识时：`@id` + `filter.ids` 都认得出来（平台填，不是 Agent 推）。
    let q = format!(
        "SELECT ?c WHERE {{ ?c a :{API_CLASS} . ?c :{API_CLASS}__id \"7d491821-27c3-4bf8-8c31-19ee5acd936d\" }}"
    );
    let (status, v2) = sparql(&svc, "jsonld", &q).await;
    assert_eq!(status, StatusCode::OK);
    let p2 = v2["@pending"].as_array().expect("@pending");
    assert_eq!(p2.len(), 1, "{v2}");
    assert_eq!(
        p2[0]["@id"], "ontok:customers/7d491821-27c3-4bf8-8c31-19ee5acd936d",
        "{v2}"
    );
    assert_eq!(
        p2[0]["get"]["filter"]["ids"],
        serde_json::json!(["7d491821-27c3-4bf8-8c31-19ee5acd936d"])
    );
}

// ══════════════════════════════════════════════════════════════
// AC-005 · rows：明确报错（⛔ 不返回空表）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac005_rows_on_an_api_class_is_not_db_source_not_an_empty_table() {
    let _g = one_at_a_time();
    let test = "ac005_rows_on_an_api_class_is_not_db_source_not_an_empty_table";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(Some(&access_json(2))).await.expect("服务可构建");
    let (status, v) = sparql(&svc, "rows", &api_class_query()).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "rows 挂不住 pending ⇒ 明确报错：{v}");
    assert_eq!(v["code"], "NOT_DB_SOURCE", "{v}");
    assert!(
        v["hint"].as_str().is_some_and(|h| h.contains("jsonld") && h.contains("拆成两段")),
        "hint 必须可行动（改要 jsonld / 拆两段）：{v}"
    );
    // ⛔ 不许返回空表：行回执的 `rows` / `row_count` 一个都不能出现。
    assert!(v.get("rows").is_none(), "⛔ 返回了 rows（空表会被读成「查了没有」）：{v}");
    assert!(v.get("row_count").is_none(), "⛔ 返回了 row_count：{v}");
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 跨源边：不下发 get；遍历报 CROSS_SOURCE_UNSUPPORTED
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac006_cross_source_edges_carry_no_get_and_traversal_is_unsupported() {
    let _g = one_at_a_time();
    let test = "ac006_cross_source_edges_carry_no_get_and_traversal_is_unsupported";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(Some(&access_json(2))).await.expect("服务可构建");

    // ① 只**走**这条边、没有点名 api 类 ⇒ pending，但**不给** `get`（规则 4）。
    let edge_only = format!("SELECT ?o ?c WHERE {{ ?o a :{DB_CLASS} . ?o :{CROSS_EDGE} ?c }}");
    let (status, v) = sparql(&svc, "jsonld", &edge_only).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let pending = v["@pending"].as_array().expect("@pending");
    assert_eq!(pending.len(), 1, "{v}");
    assert_eq!(pending[0]["reason"], "cross_source_edge", "{v}");
    assert!(
        pending[0].get("get").is_none(),
        "规则 4：跨源边**不下发** get（给了也查不了，比不给更糟）：{v}"
    );
    // ⭐ R-1 · 没有 `get` ⇒ 必须给 `ref`（否则拿不到映射）…
    assert_eq!(pending[0]["ref"], "access#Customer@v2", "跨源边那一条必须带 ref：{v}");
    // …和 `local_hint`（否则 Agent 只能猜「库侧哪一列有 FK 值」）。
    let hint = &pending[0]["local_hint"];
    assert_eq!(hint["uses_column"], "customer_id", "{v}");
    assert_eq!(
        hint["get_ids_by"], "SELECT DISTINCT \"customer_id\" FROM \"order\"",
        "取数语句要能直接照抄：{v}"
    );
    // 三步提示：取 ID → 按 ref 取 → 拼结果。
    let text = pending[0]["hint"].as_str().unwrap_or_default();
    assert!(
        text.contains('①') && text.contains('②') && text.contains('③'),
        "跨源边的 hint 要写清三步：{text}"
    );
    assert_eq!(v["meta"]["complete"], false);

    // ② 还要**往外走**（点名 api 类的属性）⇒ 跨源遍历 ⇒ 报错（两种格式一致）。
    let walk = format!(
        "SELECT ?o ?n WHERE {{ ?o a :{DB_CLASS} . ?o :{CROSS_EDGE} ?c . ?c :{API_CLASS}__customer_name ?n }}"
    );
    for format in ["jsonld", "rows"] {
        let (status, v) = sparql(&svc, format, &walk).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{format}：{v}");
        assert_eq!(v["code"], "CROSS_SOURCE_UNSUPPORTED", "{format}：{v}");
        assert!(
            v["hint"].as_str().is_some_and(|h| h.contains("拆成两段") && h.contains("别重试")),
            "{format}：hint 必须说「拆两段、别重试」：{v}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// AC-007 · `complete == @pending.is_empty()`（**不变式**，三组数据）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac007_complete_always_equals_pending_is_empty() {
    let _g = one_at_a_time();
    let test = "ac007_complete_always_equals_pending_is_empty";
    if !fixtures_ready(test) {
        return;
    }
    let svc = build_service(Some(&access_json(2))).await.expect("服务可构建");

    // 三组数据：有 pending / 无 pending / 空结果（完整）。
    //
    // ⚠️ 「无 pending 有行」这条用 `balances`（策略里 `customer_id` 非空、每租户都有行），
    // ⛔ 不用 `order`：那个租户可能一单都没有，于是「0 行」与「少了 api 那一块」在图形上
    // 分不开 —— 判据要的是**真有节点**的那种「完整」。
    let cases = [
        ("有 pending（api 类）", api_class_query(), false, 0usize),
        (
            "无 pending（库源有行）",
            "SELECT ?b WHERE { ?b a :balances } LIMIT 3".to_string(),
            true,
            1,
        ),
        ("空结果（库源 0 行）", empty_result_query(), true, 0),
    ];
    for (label, query, expect_complete, min_graph) in cases {
        let (status, v) = sparql(&svc, "jsonld", &query).await;
        assert_eq!(status, StatusCode::OK, "{label}：{v}");
        let pending_empty = v["@pending"].as_array().is_none_or(|p| p.is_empty());
        let complete = v["meta"]["complete"]
            .as_bool()
            .unwrap_or_else(|| panic!("{label} 缺 meta.complete：{v}"));
        assert_eq!(
            complete, pending_empty,
            "{label}：不变式 complete == @pending.is_empty() 被破坏（complete={complete}, pending_empty={pending_empty}）"
        );
        assert_eq!(complete, expect_complete, "{label}：这组数据的期望是 complete={expect_complete}");
        // `unresolved_classes` 与 `@pending` 同源（同一个判定算出来的）。
        let unresolved = v["meta"]["unresolved_classes"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            unresolved.is_empty(),
            pending_empty,
            "{label}：unresolved_classes 与 @pending 必须同源"
        );
        let graph_len = v["@graph"].as_array().map(|g| g.len()).unwrap_or(0);
        assert!(
            graph_len >= min_graph,
            "{label}：@graph 节点数 {graph_len} < 期望 {min_graph}（api 源那条必须是空图）"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// AC-008 · meta.mapping_version（有映射才出现）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac008_mapping_version_appears_only_with_a_mapping() {
    let _g = one_at_a_time();
    let test = "ac008_mapping_version_appears_only_with_a_mapping";
    if !fixtures_ready(test) {
        return;
    }

    // 有映射：与传参一致（`mapping_version` = 7）。
    let svc = build_service(Some(&access_json(7))).await.expect("服务可构建");
    let (_, v) = sparql(&svc, "jsonld", &api_class_query()).await;
    assert_eq!(v["meta"]["mapping_version"], 7, "{v}");
    assert_eq!(
        v["@pending"][0]["get"]["ref"], "access#Customer@v7",
        "`get.ref` 的 `@v<n>` 必须与 `meta.mapping_version` 同源"
    );
    // 库源查询也带（它是「这次用的是哪版映射」的事实，不是 pending 的附属）。
    let (_, vdb) = sparql(&svc, "jsonld", &db_class_query()).await;
    assert_eq!(vdb["meta"]["mapping_version"], 7, "{vdb}");

    // 无映射：字段不出现（R1）。
    let svc0 = build_service(None).await.expect("服务可构建");
    let (_, v0) = sparql(&svc0, "jsonld", &api_class_query()).await;
    assert!(
        v0["meta"].get("mapping_version").is_none(),
        "没加载映射却出现 mapping_version：{v0}"
    );
}

// ══════════════════════════════════════════════════════════════
// R-2 · 审计的 `code` 必须等于客户端实际收到的
// ══════════════════════════════════════════════════════════════

/// R-2 的判据：审计**逐行**读回来，比对「客户端拿到什么」与「审计写了什么」。
///
/// 变异（必须红）：把 `code` 写成内部状态名（`API_PENDING`）⇒ 第 ① 条红；
/// 忘了打 `pending=true` ⇒ 两条都红。
#[tokio::test]
async fn r2_audit_code_equals_what_the_client_received() {
    let _g = one_at_a_time();
    let test = "r2_audit_code_equals_what_the_client_received";
    if !fixtures_ready(test) {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "ontok-req145-audit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let audit_file = dir.join("audit.log");
    let svc = build_service_with(Some(&access_json(2)), Some(&audit_file))
        .await
        .expect("服务可构建");

    // ① rows 打 api 类：客户端拿到 **400 `NOT_DB_SOURCE`** ⇒ 审计必须写同一个码。
    let (rows_status, rows_body) = sparql(&svc, "rows", &api_class_query()).await;
    assert_eq!(rows_status, StatusCode::BAD_REQUEST, "{rows_body}");
    assert_eq!(rows_body["code"], "NOT_DB_SOURCE");
    // ② jsonld 打 api 类：客户端拿到 **200**（半成品）⇒ 审计写 `OK`。
    let (jsonld_status, _) = sparql(&svc, "jsonld", &api_class_query()).await;
    assert_eq!(jsonld_status, StatusCode::OK);

    let text = std::fs::read_to_string(&audit_file).expect("审计文件可读");
    let lines: Vec<&str> = text.lines().filter(|l| l.contains("event=sparql")).collect();
    assert_eq!(lines.len(), 2, "一次调用一行审计：{text}");

    // ① 行回执那条：code 与响应一致 + pending=true（「半个答案」用独立字段说）。
    assert!(
        lines[0].contains("code=NOT_DB_SOURCE"),
        "审计的 code 必须是客户端收到的那个（R-2）：{}",
        lines[0]
    );
    assert!(lines[0].contains("pending=true"), "半个答案要标 pending=true：{}", lines[0]);
    assert!(lines[0].contains("rows=0"), "这一路一行 SQL 都没跑：{}", lines[0]);

    // ② jsonld 那条：200 ⇒ OK + pending=true。
    assert!(lines[1].contains("code=OK"), "{}", lines[1]);
    assert!(lines[1].contains("pending=true"), "{}", lines[1]);

    // ⛔ 任何一行都不许出现内部状态名（那是 R-2 要消灭的形状）。
    assert!(
        !text.contains("API_PENDING"),
        "⛔ 审计里出现了内部状态名 API_PENDING（R-2）：{text}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
