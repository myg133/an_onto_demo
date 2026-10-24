//! REQ-127 · **真库实测**：两视图在生产快照上与 CLI 逐字节相同，端点真的守住了 401/304/审计。
//!
//! # 为什么这些判据不能只靠单测
//!
//! - AC-004「与 `ontok ontology export` 逐字节相同」在**空快照**上也成立 —— 必须拿有内容的
//!   冻结版本（默认 `anspire_r2rml_excl` / v2：84 对象 / 78 关系）去撞；
//! - 「默认给客户视图」在客户视图与建模视图恰好相同的库上也成立 —— 同上；
//! - 「admin token ⇒ 401」只有在**真的有一份 admin 令牌表**时才有意义（本文件拿一个
//!   长得像 admin token 的串去打，服务端只认 `--tokens` 那份 query 表）。
//!
//! # 缺环境变量时**响亮地跳过**（沿用 REQ-122/123/124 的口径）
//!
//! 不静默：打印 `*** SKIP: <测试名> -- missing <ENV> ***`（直接写 fd 2，因为 libtest 会
//! 捕获测试线程上的 `eprintln!`），于是默认 `cargo test` 的输出里也看得见「这几条没跑」。
//!
//! # 需要哪些环境变量
//!
//! | 变量 | 用途 |
//! | --- | --- |
//! | `ONTOK_TEST_DATABASE_URL`（或 `ONTOK_DATABASE_URL`） | 元数据库（项目 / 版本 / 快照） |
//! | `ONTOK_REQ127_SCHEMA` | `schema.json` 的路径（默认：仓库根的 `code/out/schema.json`） |
//! | `ONTOK_REQ127_TENANCY` | 租户声明（默认：仓库根的 `tenancy.json`；缺了就跳过建模视图对拍） |
//! | `ONTOK_REQ127_PROJECT` / `ONTOK_REQ127_VERSION` | 默认 `anspire_r2rml_excl` / `2` |
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（与 REQ-107/122/123/124 的现场判据同一口径）。
//!
//! 跑法：
//!
//! ```text
//! $env:ONTOK_TEST_DATABASE_URL="postgresql://…"
//! $env:ONTOK_REQ127_SCHEMA="D:\…\code\out\schema.json"
//! cargo test --test test_req127_live -- --nocapture
//! ```

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::query::audit::{sha256_hex, AuditSink};
use ontok::query::tokens::TokenTable;
use ontok::query::{http, QueryService, ServiceParts};
use serde_json::Value;
use tower::ServiceExt;

/// 真 query token（≥ 24 字符；`tokens.example.json` 里的假值是模板，不是凭据）。
const TOK_QUERY: &str = "FAKE-LIVE-QUERY-TOKEN-aaaaaaaaaaaaaaaa";
/// 「另一份表里的」admin token：`query serve` 只加载 `--tokens` 的 query 表 ⇒ 这个串必须 401。
const TOK_ADMIN: &str = "FAKE-LIVE-ADMIN-TOKEN-bbbbbbbbbbbbbbbbbbbb";
/// 拼错一位的 query token。
const TOK_WRONG: &str = "FAKE-LIVE-QUERY-TOKEN-aaaaaaaaaaaaaaab";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn meta_url() -> Option<String> {
    env("ONTOK_DATABASE_URL").or_else(|| env("ONTOK_TEST_DATABASE_URL"))
}

fn repo_root() -> PathBuf {
    common::crate_root()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(common::crate_root)
}

fn schema_path() -> Option<PathBuf> {
    let p = match env("ONTOK_REQ127_SCHEMA") {
        Some(p) => PathBuf::from(p),
        None => repo_root().join("code").join("out").join("schema.json"),
    };
    p.is_file().then_some(p)
}

fn tenancy_path() -> Option<PathBuf> {
    let p = match env("ONTOK_REQ127_TENANCY") {
        Some(p) => PathBuf::from(p),
        None => common::crate_root().join("tenancy.json"),
    };
    p.is_file().then_some(p)
}

fn project() -> String {
    env("ONTOK_REQ127_PROJECT").unwrap_or_else(|| "anspire_r2rml_excl".into())
}

fn version() -> i32 {
    env("ONTOK_REQ127_VERSION")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req127-live-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 跑**真的 CLI 二进制**（`CARGO_BIN_EXE_ontok`）—— 对拍的另一头必须是命令行，不是同一段代码。
fn cli(pool_url: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok"));
    cmd.args(args);
    cmd.env("ONTOK_DATABASE_URL", pool_url);
    cmd.output().expect("跑 ontok 失败")
}

async fn call(
    app: axum::Router,
    req: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("读 body")
        .to_vec();
    (status, headers, bytes)
}

/// 把响应体当 JSON 读（失败时说清楚是什么字节）。
fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|e| {
        panic!(
            "响应不是 JSON（{e}）：{}",
            String::from_utf8_lossy(bytes)
        )
    })
}

fn get(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::empty()).unwrap()
}

fn post(uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

/// 真库 + **真 `OntologyTtl::prepare`**（生产构造器）的服务。
///
/// `source` 用元数据库的池：本文件的判据只打 `/api/ontology.ttl` 与 `/mcp` 的 resource，
/// 这两条路径**不碰源库**（碰了才说明 TTL 出口里混进了查询逻辑，那是另一条要红的判据）。
async fn live_service(pool_url: &str, tag: &str) -> Option<(Arc<QueryService>, tempfile::Dir)> {
    let schema_file = schema_path()?;
    let project = project();
    let version = version();
    let tenancy = tenancy_path();

    let pool = match ontok::db::pool::connect(pool_url).await {
        Ok(p) => p,
        Err(e) => {
            common::skip::missing(&format!("可连的元数据库（{e}）"), tag);
            return None;
        }
    };
    ontok::db::pg::migrate(&pool).await.expect("建表失败");

    let raw = std::fs::read_to_string(&schema_file).expect("schema 可读");
    let schema = ontok::domain::ttl::SchemaTypes::parse(&raw).expect("schema 合法");

    // ⭐ 被测的就是**服务启动时**这一段（`serve` 里同一句）。
    let ttl = ontok::query::ttl::OntologyTtl::prepare(
        &pool,
        &ontok::query::ttl::TtlSource {
            project: &project,
            version,
            base: "https://ontok.local/",
            schema: Some(schema_file.as_path()),
            tenancy: tenancy.as_deref(),
        },
    )
    .await
    .expect("两视图必须能渲染（serve 在这句上 fail-closed）");

    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    let store = ontok::db::snapshot_store::PgStore::new(pool.clone());
    let p = projects.get_by_name(&project).await.ok().flatten()?;
    let v = projects.version_by_seq(p.id, &p.name, version).await.ok()?;
    assert!(
        v.frozen_at.is_some(),
        "真库实测要求该版本已冻结（与 serve 同一条闸门）"
    );
    let snap = store.get_snapshot(v.id).await.ok().flatten()?;

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
    };
    let model = ontok::domain::model_view::project_model_track(&snap);
    let mapping = ontok::domain::r2rml::build_mapping(
        &model,
        &opts,
        false,
        ontok::domain::r2rml::R2rmlAudit::default(),
    )
    .expect("映射必须能生成（否则说明 schema 与快照对不上）");
    let ontology_iri = opts.ontology_iri();

    let tokens = TokenTable::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"live","token":"{TOK_QUERY}","tenant":"C0001","max_rows":200,"max_concurrency":2}}
        ]}}"#
    ))
    .expect("夹具 token 表");

    let dir = tempfile::Dir::new(&format!("audit-{tag}"));
    let audit = AuditSink::open(&dir.path().join("audit.log")).expect("审计文件");

    let svc = Arc::new(QueryService::new(ServiceParts {
        source: pool.clone(),
        facts: ontok::sparql::exec::SessionFacts {
            transaction_read_only: "on".into(),
            standard_conforming_strings: "on".into(),
        },
        mapping,
        model,
        schema,
        policy,
        ontology_iri,
        project,
        version,
        timeout_ms: 5_000,
        sse_idle_timeout_ms: 2_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        tokens,
        audit,
        ttl,
    }));
    Some((svc, dir))
}

/// 一个想知道「文件放哪」的小助手（避免再引一个 `tempfile` 依赖）。
mod tempfile {
    use std::path::{Path, PathBuf};

    pub struct Dir(PathBuf);

    impl Dir {
        pub fn new(tag: &str) -> Self {
            let d = std::env::temp_dir().join(format!(
                "ontok-req127-live-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&d).expect("建临时目录");
            Self(d)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-003 / AC-004 · 两视图在生产快照上与 CLI 逐字节相同
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn live_two_views_are_byte_identical_to_the_cli_on_a_frozen_production_snapshot() {
    let Some(url) = meta_url() else {
        return common::skip::missing("ONTOK_TEST_DATABASE_URL", "live_two_views");
    };
    let Some(schema_file) = schema_path() else {
        return common::skip::missing("ONTOK_REQ127_SCHEMA", "live_two_views");
    };
    let Some((svc, _audit_dir)) = live_service(&url, "bytes").await else {
        return;
    };
    let app = || http::build_router(Arc::clone(&svc));
    let project = project();
    let version = version();
    let schema_arg = schema_file.to_string_lossy().replace('\\', "/");
    let dir = temp_dir("cli");

    // ── 客户视图：默认响应 vs `ontok ontology export`（不带 --schema） ──
    let out = dir.join("customer.ttl");
    let run = cli(
        &url,
        &[
            "ontology", "export", "--project", &project, "--version", &version.to_string(),
            "--out", &out.display().to_string(),
        ],
    );
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let cli_customer = std::fs::read(&out).unwrap();

    let (status, headers, body) = call(app(), get("/api/ontology.ttl", Some(TOK_QUERY))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        http::TTL_CONTENT_TYPE,
        "AC-001 的 Content-Type"
    );
    eprintln!(
        "AC-004 现场（customer）：CLI sha256={} · HTTP sha256={}",
        sha256_hex(&cli_customer),
        sha256_hex(&body)
    );
    assert_eq!(
        sha256_hex(&body),
        sha256_hex(&cli_customer),
        "AC-004：默认视图必须与 `ontok ontology export`（客户视图）逐字节相同"
    );

    // ⭐ 客户视图的 CLI 形态**只有一种**是「默认视图」的对照物：**不带 `--schema`**。
    //
    // 下面这条判据把「带 `--schema` 的客户视图」与它之间的差异**钉死成一条文件头注释**，
    // 于是两件事同时成立：
    //   ① 服务端选的形态（不带 `--schema`）有据可查、且是 AC-004 字面上的 `ontok ontology export`；
    //   ② 如果有人用带 `--schema` 的形态对拍而看到 sha256 不同，他知道**差在哪一行、
    //      为什么**（不是序列化器漂移，也不是列名泄漏）。
    // 客户视图结构上没有列名，所以 `--schema` 只改那句「类型来源」说明 —— 不碰任何三元组。
    let out2 = dir.join("customer-schema.ttl");
    let run = cli(
        &url,
        &[
            "ontology", "export", "--project", &project, "--version", &version.to_string(),
            "--out", &out2.display().to_string(), "--schema", &schema_arg,
        ],
    );
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let cli_customer_with_schema = std::fs::read_to_string(&out2).unwrap();
    let cli_customer_text = String::from_utf8(cli_customer.clone()).unwrap();
    let differing: Vec<(&str, &str)> = cli_customer_text
        .lines()
        .zip(cli_customer_with_schema.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        differing.len(),
        1,
        "带/不带 `--schema` 的客户视图只许差 1 行（文件头的类型来源说明）；实际差 {} 行：\n{differing:?}",
        differing.len()
    );
    assert!(
        differing[0].0.starts_with("# · xsd 类型来源"),
        "唯一允许的差异必须是文件头「类型来源」那句，实际是：{:?}",
        differing[0]
    );
    assert_eq!(
        cli_customer_text.lines().count(),
        cli_customer_with_schema.lines().count(),
        "差异只能是**同一行**的措辞，不能多/少行"
    );

    // ── 建模视图：`?view=model` vs CLI --model-view --schema --tenancy ──
    let Some(tenancy) = tenancy_path() else {
        return common::skip::missing("ONTOK_REQ127_TENANCY", "live_two_views（建模视图那一半）");
    };
    let tenancy_arg = tenancy.to_string_lossy().replace('\\', "/");
    let out = dir.join("model.ttl");
    let run = cli(
        &url,
        &[
            "ontology", "export", "--project", &project, "--version", &version.to_string(),
            "--model-view", "--out", &out.display().to_string(),
            "--schema", &schema_arg, "--tenancy", &tenancy_arg,
        ],
    );
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let cli_model = std::fs::read(&out).unwrap();

    let (status, _, model) = call(
        app(),
        get("/api/ontology.ttl?view=model", Some(TOK_QUERY)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    eprintln!(
        "AC-004 现场（model）：CLI sha256={} · HTTP sha256={}",
        sha256_hex(&cli_model),
        sha256_hex(&model)
    );
    assert_eq!(
        sha256_hex(&model),
        sha256_hex(&cli_model),
        "AC-004：`?view=model` 必须与 `ontok ontology export --model-view --schema … --tenancy …` 逐字节相同"
    );

    // ── AC-003 · 八个建模关键词：客户视图 0 命中，建模视图至少三类 > 0 ──
    let customer_text = String::from_utf8(body.clone()).expect("Turtle 是 UTF-8");
    let model_text = String::from_utf8(model.clone()).expect("Turtle 是 UTF-8");
    for needle in [
        "ontok:tenantKey",
        "ontok:tenantColumn",
        "ontok:tenantUnannotated",
        "ontok:joinEvidence",
        "ontok:joinVerified",
        "ontok:primaryKeyColumn",
        "ontok:isPrimaryKey",
        "ontok:allowedValue",
    ] {
        assert_eq!(
            customer_text.matches(needle).count(),
            0,
            "AC-003：客户视图不该出现 {needle}"
        );
    }
    for needle in [
        "ontok:joinVerified",
        "ontok:primaryKeyColumn",
        "ontok:allowedValue",
    ] {
        assert!(
            model_text.matches(needle).count() > 0,
            "AC-003：建模视图必须有 {needle}"
        );
    }
    eprintln!(
        "AC-003 现场：customer {} 字节 · model {} 字节",
        customer_text.len(),
        model_text.len()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-002 / AC-005 / AC-006 / AC-007 · 认证 / 304 / 审计 / MCP resource
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn live_auth_etag_audit_and_mcp_resource() {
    let Some(url) = meta_url() else {
        return common::skip::missing("ONTOK_TEST_DATABASE_URL", "live_auth_etag");
    };
    let Some((svc, audit_dir)) = live_service(&url, "auth").await else {
        return;
    };
    let app = || http::build_router(Arc::clone(&svc));

    // ── AC-002 四种 ──
    let (status, _, body) = call(app(), get("/api/ontology.ttl", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "无 token");
    let v: Value = json(&body);
    assert_eq!(v["code"], "UNAUTHORIZED");

    let (status, _, _) = call(app(), get("/api/ontology.ttl", Some(TOK_WRONG))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "错 token");

    let (status, _, _) = call(app(), get("/api/ontology.ttl", Some(TOK_ADMIN))).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "AC-002：admin token 必须 401（两套凭证不通用）"
    );

    let (status, headers, body) = call(app(), get("/api/ontology.ttl", Some(TOK_QUERY))).await;
    assert_eq!(status, StatusCode::OK, "query token");
    assert_eq!(headers[header::CONTENT_TYPE], http::TTL_CONTENT_TYPE);

    // ── AC-005 · ETag = sha256 前 16 位；命中 ⇒ 304 且无体 ──
    let etag = headers[header::ETAG]
        .to_str()
        .expect("ETag 是 ASCII")
        .to_string();
    assert_eq!(etag, format!("\"{}\"", &sha256_hex(&body)[..16]), "ETag 口径");
    let req = Request::builder()
        .method("GET")
        .uri("/api/ontology.ttl")
        .header(header::AUTHORIZATION, format!("Bearer {TOK_QUERY}"))
        .header(header::IF_NONE_MATCH, etag.clone())
        .body(Body::empty())
        .unwrap();
    let (status, headers, body304) = call(app(), req).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(headers[header::ETAG].to_str().unwrap(), etag);
    let len = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("0");
    assert_eq!(len, "0", "304 的 Content-Length 必须是 0（或无）");
    assert!(body304.is_empty(), "304 不许带体");
    eprintln!("AC-005 现场：200 ETag={etag} · If-None-Match ⇒ 304 · Content-Length={len}");

    // ── AC-006 · 五条请求五行（401 ×3 / 200 / 304）；无 token 明文；401 也有行 ──
    //
    // 在 MCP 那一段**之前**读：这一段的请求集合是确定的（3 个 401 + 1 个 200 + 1 个 304），
    // 行序也是确定的 —— 后面 MCP 的请求各自还会再记行，混在一起数只会把判据写成玄学。
    let audit_path = audit_dir.path().join("audit.log");
    let text = std::fs::read_to_string(&audit_path).expect("读审计");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 5, "五次请求五行（401 ×3 / 200 / 304）：\n{text}");
    for line in &lines[..3] {
        assert!(
            line.contains("event=auth")
                && line.contains("token=-")
                && line.contains("view=customer")
                && line.contains("code=UNAUTHORIZED"),
            "401 行：{line}"
        );
    }
    assert!(
        lines[3].contains("event=ttl")
            && lines[3].contains("token=sha256:")
            && lines[3].contains("tenant=C0001")
            && lines[3].contains("view=customer")
            && lines[3].contains(&format!("bytes={}", body.len()))
            && lines[3].contains("code=OK"),
        "200 行：{}",
        lines[3]
    );
    assert!(
        lines[4].contains("code=NOT_MODIFIED")
            && lines[4].contains("bytes=0")
            && lines[4].contains("view=customer"),
        "304 行：{}",
        lines[4]
    );
    assert!(!text.contains(TOK_QUERY), "⛔ 审计里出现了 token 明文：{text}");
    assert!(!text.contains(TOK_ADMIN), "⛔ 审计里出现了 admin token 明文：{text}");
    eprintln!("AC-006 现场（{} 行）：\n{}", lines.len(), text);

    // ── AC-007 · MCP resource：正文与 HTTP 同视图逐字节相同 ──
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#;
    let (_, _, listing) = call(app(), post("/mcp", Some(TOK_QUERY), line)).await;
    let v: Value = json(&listing);
    let uris: Vec<&str> = v["result"]["resources"]
        .as_array()
        .expect("resources[]")
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(
        uris,
        vec![http::RESOURCE_CUSTOMER, http::RESOURCE_MODEL],
        "resources/list 必须列出两个视图"
    );

    for (uri, http_uri) in [
        (http::RESOURCE_CUSTOMER, "/api/ontology.ttl"),
        (http::RESOURCE_MODEL, "/api/ontology.ttl?view=model"),
    ] {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{{"uri":"{uri}"}}}}"#
        );
        let (_, _, rpc) = call(app(), post("/mcp", Some(TOK_QUERY), &line)).await;
        let v: Value = json(&rpc);
        let text = v["result"]["contents"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{uri} 没有 text：{v}"));
        let (_, _, http_body) = call(app(), get(http_uri, Some(TOK_QUERY))).await;
        eprintln!(
            "AC-007 现场：{uri} sha256={} · {http_uri} sha256={}",
            sha256_hex(text.as_bytes()),
            sha256_hex(&http_body)
        );
        assert_eq!(
            sha256_hex(text.as_bytes()),
            sha256_hex(&http_body),
            "{uri} 的正文必须与 {http_uri} 逐字节相同"
        );
    }

    // ── AC-006 续 · MCP 侧也逐条记（`resources/list` 走信封级一行；`resources/read` 走 TTL 行）──
    let text = std::fs::read_to_string(&audit_path).expect("读审计");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        10,
        "MCP 段再记 5 行（list 1 + read 2 + read 里两次 HTTP 2）：\n{text}"
    );
    let mcp_lines: Vec<&&str> = lines.iter().filter(|l| l.contains("proto=mcp")).collect();
    assert_eq!(mcp_lines.len(), 3, "MCP 三条请求三条审计：{text}");
    assert!(
        mcp_lines[1].contains("event=ttl")
            && mcp_lines[1].contains("view=customer")
            && mcp_lines[1].contains("bytes="),
        "resources/read（customer）的审计行：{}",
        mcp_lines[1]
    );
    assert!(
        mcp_lines[2].contains("event=ttl") && mcp_lines[2].contains("view=model"),
        "resources/read（model）的审计行：{}",
        mcp_lines[2]
    );
}


