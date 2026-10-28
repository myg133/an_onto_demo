//! ⭐ REQ-178 · **TTL 两视图按需生成** —— 一个词写错不该让整个项目打不开。
//!
//! # 病根（改前）
//!
//! `query::serve::prepare_project` 在**装配期**就渲染 TTL 两视图（fail-closed）：
//! 客户视图命中发布闸 ⇒ 整个项目 500 `project.load_failed` ⇒ catalog / describe /
//! sparql / Studio 一起挂。而命中的往往只是**一条说明文字**。
//!
//! # 本文件钉住的 AC（能红能绿）
//!
//! | # | 判据 | 怎么变红 |
//! | --- | --- | --- |
//! | AC-001 | 客户视图命中闸的项目：catalog / describe / sparql **200**；`/api/ontology.ttl` **500 `project.ttl_failed`**（消息里带命中词条） | 把渲染放回装配期 ⇒ catalog 也 500 |
//! | AC-002 | 正常项目：两视图 sha256 与**改前基线**（`.local/t178/baseline.json`）相同 | 请求期渲染换了输入 / 换了渲染器 ⇒ sha 不同 |
//! | AC-003 | 同一 `version_id` 连续两次 TTL ⇒ 第二次**不重渲染**（`TtlCache::renders()` 计数器） | 每次请求都渲染 ⇒ 计数器 = 2 |
//! | AC-004 | activate 新版本 ⇒ 下一个 TTL 请求用**新版本**（缓存键 = version_id） | 缓存键写成 project / 常量 ⇒ 还是旧版本 |
//! | AC-005 | 单项目模式（真进程 `--project/--version`）：**坏项目也能启动**，启动日志说「请求时按需渲染」，请求 TTL 才 500 | 渲染放回装配期 ⇒ 进程起不来 |
//!
//! # 环境
//!
//! `ONTOK_TEST_DATABASE_URL` 必需（库名需含 test/scratch/tmp/ci/dev）；
//! `ONTOK_SECRET_KEY` 只在起 `query serve` 真进程时需要。
//! AC-002 需要先跑一次 `tests/zz_req178_baseline.rs`（见交付说明）。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::{EntityType, Property, Snapshot};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::{sha256_hex, AuditSink};
use ontok::query::http::{build_router, build_router_multi};
use ontok::query::multi::MultiProject;
use ontok::query::serve::Deployment;
use ontok::query::tokens::TokenTable;
use ontok::query::ttl::{OntologyTtl, TtlCache, TtlPlan, TtlSource};
use ontok::query::{QueryService, ServiceMode, ServiceParts};
use ontok::sparql::exec::SessionFacts;
use ontok::sparql::policy::QueryPolicy;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// 与 `query serve` 的默认 `--base` 相同（两视图里的 IRI 基）。
const BASE: &str = "https://ontok.local/";
const TOKEN: &str = "FAKE-REQ178-QUERY-aaaaaaaaaaaaaaaa";
const TENANT: &str = "C0001";
/// 客户视图里那个「业务词 = 建模术语」的显示名（demand 的触发场景）。
const LEAK_WORD: &str = "evidence";

/// 本体服务模式的占位租户声明（与 `query::serve::NO_TENANT_POLICY` 同一个形状）。
const NO_TENANT: &str = r#"{"version":"1.0.0","tenantSubject":"__no_tenant__",
    "tenantIdColumn":"__no_tenant__","perTable":{},"publicDimensions":[]}"#;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn db_url() -> String {
    ontok::config::test_database_url().expect(NEED_TEST_DB)
}

fn secret_key() -> String {
    "ab".repeat(32)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req178-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn entity(key: &str, display: &str, columns: &[(&str, &str)]) -> EntityType {
    EntityType {
        key: key.into(),
        display_name: display.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: columns
            .iter()
            .map(|(name, column)| Property {
                name: (*name).into(),
                column: (*column).into(),
                nullable: false,
                note: None,
                sample: None,
            })
            .collect(),
        evidence: vec![],
    }
}

/// 建「项目 + 第 1 版 + 快照 + 冻结」，返回 `(项目名, seq, version_id, 快照)`。
///
/// `display` 是实体的人读名：`evidence` 会让**客户视图**命中发布闸（AC-001 / AC-005）。
async fn frozen_project(
    pool: &PgPool,
    prefix: &str,
    display: &str,
) -> (String, i32, Uuid, Snapshot) {
    common::migrate(pool).await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = common::fresh_project(&ps, prefix).await;
    let v = ps.new_version(p.id).await.expect("建版本失败");

    let mut snap = Snapshot::new(v.id, v.seq);
    snap.entity_types = vec![entity(
        "projects",
        display,
        &[("名称", "name")],
    )];
    store.save_snapshot(&snap).await.expect("写快照失败");
    ps.freeze(v.id).await.expect("冻结失败");
    (p.name, v.seq, v.id, snap)
}

async fn cleanup(pool: &PgPool, names: &[String]) {
    for n in names {
        let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(n)
            .execute(pool)
            .await;
    }
}

fn tokens_single(tenant: &str) -> TokenTable {
    TokenTable::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"agent","token":"{TOKEN}","tenant":"{tenant}","max_rows":50,"max_concurrency":1}}]}}"#
    ))
    .expect("单项目 token 表")
}

fn tokens_multi(pairs: &[(&str, &str)]) -> TokenTable {
    let entries: Vec<String> = pairs
        .iter()
        .map(|(t, p)| format!(r#"{{"token":"{t}","project":"{p}","label":"{p}"}}"#))
        .collect();
    TokenTable::parse_multi(
        &format!(r#"{{"version":2,"tokens":[{}]}}"#, entries.join(",")),
        ServiceMode::OntologyOnly,
    )
    .expect("多项目 token 表")
}

/// 多项目模式的部署级输入（本体服务模式：不连业务库）。
fn deployment() -> Deployment {
    Deployment {
        mode: ServiceMode::OntologyOnly,
        schema: SchemaTypes::default(),
        schema_path: None,
        policy: QueryPolicy::parse(NO_TENANT).expect("占位声明"),
        base: BASE.into(),
        include_inherit: false,
        access_raw: None,
        access_path: None,
        tenancy: None,
        timeout_ms: 10_000,
        sse_idle_timeout_ms: 60_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        source: None,
        facts: None,
    }
}

/// 两视图的**惰性**缓存（装配期只留坐标，不渲染）。
fn lazy_ttl(pool: &PgPool, project: &str, version: i32, schema: Option<PathBuf>) -> TtlCache {
    TtlCache::lazy(
        pool.clone(),
        TtlPlan {
            project: project.into(),
            version,
            base: BASE.into(),
            schema,
            tenancy: None,
            api_classes: None,
        },
    )
}

async fn rest(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, String) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    let req = b
        .body(
            body.map(|s| Body::from(s.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 16 * 1024 * 1024)
        .await
        .expect("读 body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 一个出口产不出来，不该拖垮其它出口
// ══════════════════════════════════════════════════════════════

/// `projects` 表真实存在于**元数据库**里（本判据的源库 = 元数据库) —— 这样
/// sparql 能真执行（200 + 0 行），而不是被「没有数据源」挡成 503。
const SCHEMA_JSON: &str = r#"{"tables":[
  {"name":"projects","primary_key":"name","composite_primary_key":[],
   "unique_columns":["name"],
   "columns":[{"name":"name","data_type":"text","is_primary_key":true}]}
]}"#;

const POLICY_JSON: &str = r#"{"version":"1.0.0","tenantSubject":"projects",
    "tenantIdColumn":"name","perTable":{"projects":{"tenantColumn":"name"}},
    "publicDimensions":[]}"#;

/// **AC-001（主判据）** · 客户视图命中发布闸的项目：
/// 「读面」三个出口照常 200，只有 `/api/ontology.ttl` 500 `project.ttl_failed`。
///
/// 为什么用**查询模式 + 元数据库当源库**：AC 要求 sparql **200**（不是被数据源挡住的 503），
/// 而本机没有业务源库；`projects` 是元数据库里真实存在的表，`WHERE name = '<tenant>'` 是
/// 合法 SQL（0 行），足以证明「sparql 没有被 TTL 的失败连坐」。
///
/// 变异：把 `prepare_project` 里的惰性缓存换回装配期 `OntologyTtl::prepare` ⇒
/// 这个服务构造本身就失败（catalog 只能 500）⇒ 红。
#[tokio::test]
async fn ac001_a_leaking_customer_view_only_breaks_the_ttl_endpoint() {
    let pool = common::test_pool().await;
    let (project, version, _vid, snap) = frozen_project(&pool, "ac001", LEAK_WORD).await;
    let dir = temp_dir("ac001");
    let schema_path = dir.join("schema.json");
    std::fs::write(&schema_path, SCHEMA_JSON).expect("写 schema");

    let schema = SchemaTypes::parse(SCHEMA_JSON).expect("schema 合法");
    let policy = QueryPolicy::parse(POLICY_JSON).expect("声明合法");
    let model = ontok::domain::model_view::project_model_track(&snap);
    let opts = ontok::domain::ttl::TtlOptions {
        project: project.as_str(),
        version,
        base: BASE,
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema),
        api_classes: None,
    };
    let mapping = ontok::domain::r2rml::build_mapping(
        &model,
        &opts,
        false,
        ontok::domain::r2rml::R2rmlAudit::default(),
    )
    .expect("映射必须能生成（schema 与快照对得上）");
    let ontology_iri = opts.ontology_iri();

    let svc = Arc::new(QueryService::new(ServiceParts {
        mode: ServiceMode::Query,
        // 源库 = 元数据库：本判据只读 `projects`（见上面的常量注释）。
        source: Some(pool.clone()),
        facts: Some(SessionFacts {
            transaction_read_only: "on".into(),
            standard_conforming_strings: "on".into(),
        }),
        mapping: Some(mapping),
        model,
        schema,
        policy,
        ontology_iri,
        project: project.clone(),
        version,
        timeout_ms: 10_000,
        sse_idle_timeout_ms: 60_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        tokens: tokens_single(TENANT),
        audit: AuditSink::stdout_only(),
        ttl: lazy_ttl(&pool, &project, version, Some(schema_path)),
        access: None,
    }));

    // ① 三个「读面」出口：200。
    let (s_catalog, catalog) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/catalog?page=1&page_size=50",
        Some(TOKEN),
        None,
    )
    .await;
    let (s_describe, describe) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/describe?name=:projects",
        Some(TOKEN),
        None,
    )
    .await;
    let (s_sparql, sparql) = rest(
        build_router(Arc::clone(&svc)),
        "POST",
        "/api/sparql",
        Some(TOKEN),
        Some(r#"{"query":"SELECT ?s WHERE { ?s a :projects }"}"#),
    )
    .await;

    // ② TTL：500 + 新错误码 + 命中词条。
    let (s_ttl, ttl) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/ontology.ttl",
        Some(TOKEN),
        None,
    )
    .await;
    let ttl_v: Value = serde_json::from_str(&ttl).expect("500 也是 JSON 错误契约");

    eprintln!(
        "AC-001：catalog={s_catalog} · describe={s_describe} · sparql={s_sparql} · \
         ontology.ttl={s_ttl}（code={}）",
        ttl_v["code"]
    );
    eprintln!("AC-001 catalog 回执：{catalog}");
    eprintln!("AC-001 describe 回执：{describe}");
    eprintln!("AC-001 sparql 回执：{sparql}");
    eprintln!("AC-001 TTL 错误正文：{ttl}");

    assert_eq!(s_catalog, StatusCode::OK, "catalog 不该被 TTL 连坐：{catalog}");
    assert_eq!(s_describe, StatusCode::OK, "describe 不该被 TTL 连坐：{describe}");
    assert_eq!(s_sparql, StatusCode::OK, "sparql 不该被 TTL 连坐：{sparql}");
    assert_eq!(s_ttl, StatusCode::INTERNAL_SERVER_ERROR, "{ttl}");
    assert_eq!(ttl_v["code"], "project.ttl_failed", "{ttl}");
    assert_ne!(
        ttl_v["code"], "project.load_failed",
        "⛔ 这个码的语义是「项目装不起来」，TTL 失败不是它：{ttl}"
    );
    let message = ttl_v["message"].as_str().expect("message 非空");
    assert!(
        message.contains(LEAK_WORD),
        "消息必须保留命中的词条：{message}"
    );
    assert!(
        message.contains("C6") || message.contains("客户视图"),
        "消息要点名客户视图 / 出口 C6：{message}"
    );
    assert!(
        !ttl_v["hint"].as_str().unwrap_or("").trim().is_empty(),
        "hint 必须可行动：{ttl}"
    );

    // ③ `?view=model` 也 500 —— **不违背** AC-001：两视图由同一次 `prepare` 产出
    //    （与 `ontology export` 同一条读取/投影段，AC-002 的逐字节相同靠它），
    //    所以它们同生共死；失败的仍然只有 **TTL 这一个端点**（catalog / describe /
    //    sparql 三条 200 已在上面钉住）。⛔ 不为了「建模视图单独能出」把一对拆成两条
    //    渲染路径 —— 那会长出两份投影代码（正是 REQ-124 消灭过的字节漂移源）。
    let (s_model, model_body) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/ontology.ttl?view=model",
        Some(TOKEN),
        None,
    )
    .await;
    let model_v: Value = serde_json::from_str(&model_body).expect("JSON 错误契约");
    eprintln!(
        "AC-001：ontology.ttl?view=model={s_model}（code={}）· 与客户视图同一个失败原因",
        model_v["code"]
    );
    assert_eq!(s_model, StatusCode::INTERNAL_SERVER_ERROR, "{model_body}");
    assert_eq!(model_v["code"], "project.ttl_failed", "{model_body}");

    // ④ 计数：失败**不缓存** ⇒ 每次请求都会重试（这正是「修好一版之后立刻能出」的前提）。
    //    客户视图 1 次 + 建模视图 1 次 = 2；再要一次客户视图 = 3。
    assert_eq!(svc.ttl.renders(), 2, "两次失败尝试 = 2 次渲染");
    let (s_again, again) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/ontology.ttl",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(s_again, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        again, ttl,
        "同一份坏版本 ⇒ 每次都是同一个错误（⛔ 不缓存失败，但也不该时好时坏）"
    );
    assert_eq!(svc.ttl.renders(), 3, "失败不缓存 ⇒ 下一次请求会重试");

    cleanup(&pool, &[project]).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 正常项目：与**改前基线**逐字节相同
// ══════════════════════════════════════════════════════════════

/// **AC-002** · 请求期渲染的字节 == **装配期**（改前）渲染的字节。
///
/// 这里拿真库的同一份快照做两件事：
/// 1. 走**新**的请求期路径（`TtlCache::lazy` + `GET /api/ontology.ttl`）取两视图；
/// 2. 直接调 [`OntologyTtl::prepare`] —— 它正是改前 `prepare_project` 装配期调的那个
///    函数（本次**没有改它**，见 `git diff src/query/ttl.rs` 里 prepare 的读取 / 投影段）。
///
/// 两边 sha256 相等 ⇒ 「请求期字节与改前的装配期字节逐字节相同」。
/// ⚠️ 交付记录里另有**改前二进制**跑出来的 sha256（同一份快照、同一个项目名），
/// 与这里的两组值一致；本判据不依赖那个本地 artifact，任何机器都能跑。
#[tokio::test]
async fn ac002_bytes_match_the_pre_change_baseline() {
    let pool = common::test_pool().await;
    let (project, version, _vid, _snap) = frozen_project(&pool, "ac002", "客户").await;
    let svc = Arc::new(QueryService::new(ServiceParts {
        mode: ServiceMode::OntologyOnly,
        source: None,
        facts: None,
        mapping: None,
        model: ontok::domain::model_view::ModelTrackView {
            seq: version,
            extracted_by: None,
            entity_types: vec![],
            link_types: vec![],
            action_types: vec![],
            functions: vec![],
        },
        schema: SchemaTypes::default(),
        policy: QueryPolicy::parse(NO_TENANT).expect("占位声明"),
        ontology_iri: ontok::domain::ttl::TtlOptions::new(&project, version).ontology_iri(),
        project: project.clone(),
        version,
        timeout_ms: 10_000,
        sse_idle_timeout_ms: 60_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        tokens: tokens_single(TENANT),
        audit: AuditSink::stdout_only(),
        ttl: lazy_ttl(&pool, &project, version, None),
        access: None,
    }));

    let (s1, customer) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/ontology.ttl",
        Some(TOKEN),
        None,
    )
    .await;
    let (s2, model) = rest(
        build_router(Arc::clone(&svc)),
        "GET",
        "/api/ontology.ttl?view=model",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "{customer}");
    assert_eq!(s2, StatusCode::OK, "{model}");

    // 装配期（改前）那条入口：同一个 `prepare`、同一份坐标 / 选项。
    let direct = OntologyTtl::prepare(
        &pool,
        &TtlSource {
            project: &project,
            version,
            base: BASE,
            schema: None,
            tenancy: None,
            api_classes: None,
        },
    )
    .await
    .expect("装配期入口必须能渲染这份正常快照");

    let cust_sha = sha256_hex(customer.as_bytes());
    let model_sha = sha256_hex(model.as_bytes());
    eprintln!(
        "AC-002 · 装配期(改前) customer sha256={} ({} B)",
        direct.customer().sha256(),
        direct.customer().bytes()
    );
    eprintln!(
        "AC-002 · 请求期(改后) customer sha256={} ({} B)",
        cust_sha,
        customer.len()
    );
    eprintln!(
        "AC-002 · 装配期(改前) model    sha256={} ({} B)",
        direct.model().sha256(),
        direct.model().bytes()
    );
    eprintln!(
        "AC-002 · 请求期(改后) model    sha256={} ({} B)",
        model_sha,
        model.len()
    );
    assert_eq!(
        cust_sha,
        direct.customer().sha256(),
        "客户视图必须与改前（装配期）逐字节相同"
    );
    assert_eq!(
        model_sha,
        direct.model().sha256(),
        "建模视图必须与改前（装配期）逐字节相同"
    );

    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-003 / AC-004 · 按 version_id 缓存 + 换版本自然绕过
// ══════════════════════════════════════════════════════════════

/// `versions` 的每一项是 `(类 key, 人读名)` 列表。
///
/// ⚠️ 人读名必须**像业务名**（`虚拟机` / `磁盘`）：`vm 的中文名` 这种「由 key 拼出来的
/// 伪名」会被投影层判成 fabricated 而**整条丢弃**（`extract::name_suggest` 的既有规则），
/// 于是客户视图里一个类都没有 —— 判据就变成空转。
async fn make_project(pool: &PgPool, name: &str, versions: &[&[(&str, &str)]]) -> Vec<Uuid> {
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = projects.get_or_create(name).await.expect("建项目");
    let mut ids = Vec::new();
    for classes in versions {
        let v = projects.new_version(p.id).await.expect("新版本");
        let mut snap = Snapshot::new(v.id, v.seq);
        snap.entity_types = classes
            .iter()
            .map(|(key, display)| entity(key, display, &[("编号", "id")]))
            .collect();
        store.save_snapshot(&snap).await.expect("存快照");
        ids.push(v.id);
    }
    ids
}

async fn activate(pool: &PgPool, name: &str, version_id: Uuid) {
    let projects = PgProjectStore::new(pool.clone());
    let p = projects.get_by_name(name).await.expect("读项目").expect("项目在");
    projects.activate(p.id, version_id).await.expect("激活");
}

/// **AC-003** · 同一 `version_id` 连续两次 TTL ⇒ 第二次**不重渲染**（计数器）。
///
/// **AC-004** · activate 新版本 ⇒ 下一个 TTL 请求用新版本（缓存键 = version_id）。
///
/// 变异（AC-003）：去掉缓存 ⇒ `renders()` = 2；变异（AC-004）：把缓存键写成项目名 /
/// 启动时一份 ⇒ 换版本后仍是旧字节。
#[tokio::test]
async fn ac003_ac004_ttl_is_cached_per_version_and_a_new_version_is_a_new_cache() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let name = format!(
        "req178-ac3-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let ids = make_project(
        &pool,
        &name,
        &[
            &[("vm", "虚拟机"), ("disk", "磁盘")],
            &[("vm", "虚拟机"), ("disk", "磁盘"), ("image", "镜像")],
        ],
    )
    .await;
    activate(&pool, &name, ids[0]).await;

    let multi = Arc::new(MultiProject::new(
        pool.clone(),
        tokens_multi(&[(TOKEN, &name)]),
        deployment(),
        AuditSink::stdout_only(),
    ));
    let app = || build_router_multi(Arc::clone(&multi));

    // ① v1 的第一次 TTL 请求：渲染一次（建模视图：类 key 在 `ontok:key` 里，判据好钉）。
    let (s1, ttl1) = rest(
        app(),
        "GET",
        "/api/ontology.ttl?view=model",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "{ttl1}");
    let svc1 = multi.cached_service(ids[0]).expect("v1 已进缓存");
    assert_eq!(svc1.ttl.renders(), 1, "第一次请求必须渲染一次");
    assert!(
        ttl1.contains("ontok:version 1"),
        "v1 的 TTL 必须标明第 1 版：{ttl1}"
    );
    assert!(
        ttl1.contains("ontok:key \"vm\"") && !ttl1.contains("ontok:key \"image\""),
        "v1 的 TTL 不该有 v2 才引入的类：{ttl1}"
    );

    // ② 第二次：同一 version ⇒ 命中缓存，不再渲染（与耗时无关，看计数器）。
    let (s2, ttl2) = rest(
        app(),
        "GET",
        "/api/ontology.ttl?view=model",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(ttl2, ttl1, "同一版本两次 TTL 必须逐字节相同");
    assert_eq!(
        multi.cached_service(ids[0]).unwrap().ttl.renders(),
        1,
        "第二次请求不许重渲染（AC-003）"
    );
    eprintln!(
        "AC-003 · v1 renders={}（两次请求）· {} 字节",
        multi.cached_service(ids[0]).unwrap().ttl.renders(),
        ttl2.len()
    );

    // ③ 换版本 ⇒ 下一次请求就是 v2（缓存键 = version_id）。
    activate(&pool, &name, ids[1]).await;
    let (s3, ttl3) = rest(
        app(),
        "GET",
        "/api/ontology.ttl?view=model",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(s3, StatusCode::OK, "{ttl3}");
    assert!(
        ttl3.contains("ontok:version 2") && ttl3.contains("ontok:key \"image\""),
        "换版本后必须用新版本（否则 AC-004 红）：{ttl3}"
    );
    let svc2 = multi.cached_service(ids[1]).expect("v2 已进缓存");
    assert_eq!(svc2.ttl.renders(), 1, "新版本第一次请求渲染一次");
    assert_eq!(
        multi.cached_service(ids[0]).unwrap().ttl.renders(),
        1,
        "旧版本的缓存不受影响（回滚还能直接命中）"
    );
    eprintln!(
        "AC-004 · v2 renders={} · 缓存版本数={} · v2 sha={}",
        svc2.ttl.renders(),
        multi.cached_versions(),
        sha256_hex(ttl3.as_bytes())[..16].to_string()
    );

    cleanup(&pool, &[name]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 单项目模式：坏项目也能启动，TTL 只在请求期失败
// ══════════════════════════════════════════════════════════════

struct Server {
    child: Child,
    port: u16,
    stderr: PathBuf,
}

impl Server {
    fn spawn(dir: &Path, project: &str, version: i32, tokens: &Path) -> Self {
        let port = free_port();
        let stderr = dir.join(format!("serve-{port}.stderr.log"));
        let child = Command::new(env!("CARGO_BIN_EXE_ontok"))
            .args([
                "query",
                "serve",
                "--bind",
                &format!("127.0.0.1:{port}"),
                "--tokens",
                tokens.to_str().unwrap(),
                "--project",
                project,
                "--version",
                &version.to_string(),
                "--ontology-only",
            ])
            .env("ONTOK_DATABASE_URL", db_url())
            .env("ONTOK_SECRET_KEY", secret_key())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                std::fs::File::create(&stderr).expect("建 stderr 日志"),
            ))
            .spawn()
            .expect("起 ontok query serve 子进程");
        Self { child, port, stderr }
    }

    fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// 等到横幅出现（或进程提前退出）。返回 stderr 全文。
    fn wait_ready(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let text = self.stderr_text();
            if text.contains("就绪") {
                return text;
            }
            if let Some(status) = self.child.try_wait().ok().flatten() {
                panic!("query serve 提前退出（{status}）：\n{text}");
            }
            if Instant::now() > deadline {
                panic!("query serve 60s 内没有就绪：\n{text}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("分配端口");
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

fn http_get(port: u16, path: &str, token: &str) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("连上服务");
    s.set_read_timeout(Some(Duration::from_secs(20))).expect("设读超时");
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n\
         Connection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).expect("写请求");
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("读响应");
    let pos = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("响应头结束标记");
    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .expect("状态行");
    let raw = &buf[pos + 4..];
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
    (status, if chunked { dechunk(raw) } else { raw.to_vec() })
}

fn dechunk(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let Some(nl) = raw[i..].windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let line = String::from_utf8_lossy(&raw[i..i + nl]).to_string();
        let size =
            usize::from_str_radix(line.split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
        i += nl + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[i..(i + size).min(raw.len())]);
        i += size + 2;
    }
    out
}

/// **AC-005** · 单项目模式（`--project/--version` 钉死）同样只在**请求期**渲染：
///
/// ① 一个客户视图命中闸的项目**能启动**（改前：装配期 fail-closed ⇒ 起不来）；
/// ② 启动日志说「请求时按需渲染」（没有字节数 / ETag —— 那时还没渲染）；
/// ③ catalog 200，TTL 500 `project.ttl_failed`。
///
/// 变异：把惰性缓存换回装配期 `OntologyTtl::prepare` ⇒ ① 红（进程退出，等不到「就绪」）。
#[tokio::test]
async fn ac005_single_project_mode_starts_and_renders_only_on_request() {
    let pool = common::test_pool().await;
    let (project, version, _vid, _snap) = frozen_project(&pool, "ac005", LEAK_WORD).await;
    let dir = temp_dir("ac005");
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        format!(r#"{{"version":"1.0.0","tokens":[{{"label":"agent","token":"{TOKEN}"}}]}}"#),
    )
    .expect("写 tokens");

    let mut srv = Server::spawn(&dir, &project, version, &tokens);
    let startup = srv.wait_ready();
    eprintln!("AC-005 启动日志：\n{startup}");
    assert!(
        startup.contains("请求时按需渲染"),
        "启动日志要说清「请求时按需渲染」：\n{startup}"
    );
    assert!(
        !startup.contains("客户视图，") ,
        "启动期不该已经渲染出字节数 / ETag（那正是要消灭的装配期渲染）：\n{startup}"
    );

    let (s_catalog, _) = http_get(srv.port, "/api/catalog?page=1&page_size=50", TOKEN);
    let (s_ttl, body) = http_get(srv.port, "/api/ontology.ttl", TOKEN);
    let text = String::from_utf8_lossy(&body).to_string();
    eprintln!(
        "AC-005：坏项目在单项目模式下 catalog={s_catalog} · ontology.ttl={s_ttl} · code={}",
        text.split("\"code\":\"").nth(1).unwrap_or("-").split('"').next().unwrap_or("-")
    );
    assert_eq!(s_catalog, 200, "坏项目的 catalog 必须照常");
    assert_eq!(s_ttl, 500, "{text}");
    assert!(text.contains("project.ttl_failed"), "{text}");

    srv.stop();
    cleanup(&pool, &[project]).await;
    let _ = std::fs::remove_dir_all(&dir);
}
