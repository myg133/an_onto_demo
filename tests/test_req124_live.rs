//! REQ-124 · 控制面 API 的**真库实测**（AC-002/003/004/005/006）。
//!
//! # 为什么必须连真库
//!
//! - 「导出与 CLI **逐字节相同**」在空快照上也成立，所以要用**有内容**的版本；
//! - 「重复 apply ⇒ 409」「冻结后 apply ⇒ 409」是**状态机**，不是参数校验；
//! - `GET /api/projects` 的计数要与 `object_types` / `link_types` 的真实行数对拍。
//!
//! # 缺环境变量时**响亮地跳过**（沿用 REQ-122/123 的口径）
//!
//! 不静默：打印 `*** SKIP: <测试名> -- missing <ENV> ***`（直接写 fd 2，
//! 因为 libtest 会捕获测试线程上的 `eprintln!`）。
//!
//! # 需要哪些环境变量
//!
//! | 变量 | 用途 |
//! | --- | --- |
//! | `ONTOK_TEST_DATABASE_URL`（或 `ONTOK_DATABASE_URL`） | 元数据库 |
//! | `ONTOK_REQ124_SCHEMA` | `schema.json` 的路径（默认：仓库根的 `code/out/schema.json`） |
//! | `ONTOK_REQ124_PROJECT` / `ONTOK_REQ124_VERSION` | 一个**已冻结**的项目/版本，默认 `anspire_r2rml_excl` / `2` |
//! | `ONTOK_REQ124_TENANCY` | 可省；仓库根的 `tenancy.json` |
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（与 REQ-107/122/123 的现场判据同一口径）。

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::console::audit::AuditLog;
use ontok::console::decl::DeclPaths;
use ontok::console::tokens::AdminTokens;
use ontok::console::{ConsoleParts, ConsoleService};
use ontok::db::project::PgProjectStore;
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::{sha256_hex, AuditSink};
use serde_json::{json, Value};
use tower::ServiceExt;

const ADMIN: &str = "FAKE-LIVE-ADMIN-TOKEN-aaaaaaaa";
const QUERY: &str = "FAKE-LIVE-QUERY-TOKEN-bbbbbbbbbb";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn meta_url() -> Option<String> {
    env("ONTOK_DATABASE_URL").or_else(|| env("ONTOK_TEST_DATABASE_URL"))
}

/// 仓库根（worktree 的上一级；`code/out/schema.json` 在那里）。
fn repo_root() -> PathBuf {
    common::crate_root()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(common::crate_root)
}

fn schema_path() -> Option<PathBuf> {
    let p = match env("ONTOK_REQ124_SCHEMA") {
        Some(p) => PathBuf::from(p),
        None => repo_root().join("code").join("out").join("schema.json"),
    };
    p.is_file().then_some(p)
}

fn tenancy_path() -> Option<PathBuf> {
    let p = match env("ONTOK_REQ124_TENANCY") {
        Some(p) => PathBuf::from(p),
        None => repo_root().join("tenancy.json"),
    };
    p.is_file().then_some(p)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req124-live-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 真库 + 真 schema 的服务（声明文件指到临时目录里的空文件：本文件不测 PUT）。
fn live_service(
    pool: sqlx::postgres::PgPool,
    project: &str,
    schema: SchemaTypes,
) -> Arc<ConsoleService> {
    let dir = temp_dir("decl");
    let paths = DeclPaths {
        exclusions: dir.join("exclusions.json"),
        policy: dir.join("query-policy.json"),
        tenancy: dir.join("tenancy.json"),
    };
    let tokens = AdminTokens::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"token":"{ADMIN}","kind":"admin"}},
            {{"token":"{QUERY}","kind":"query","tenant":"C0001"}}
        ]}}"#
    ))
    .expect("token 表");
    Arc::new(ConsoleService::new(ConsoleParts {
        pool,
        project: project.to_string(),
        base: "https://ontok.local/".into(),
        tokens,
        schema,
        paths,
        audit: AuditLog::new(AuditSink::stdout_only()),
        bind: "127.0.0.1:8899".parse().unwrap(),
        allow_public: false,
    }))
}

async fn call(app: axum::Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 32 * 1024 * 1024)
        .await
        .expect("读 body");
    (status, bytes.to_vec())
}

fn get(uri: &str, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::empty()).unwrap()
}

fn post(uri: &str, token: Option<&str>, body: &Value) -> Request<Body> {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn cli(pool_url: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"));
    cmd.args(args);
    cmd.env("ONTOK_DATABASE_URL", pool_url);
    cmd.output().expect("跑 ontok 失败")
}

// ══════════════════════════════════════════════════════════════
// AC-002 / 003 / 004 / 005 / 006 · 一条端到端流水线
// ══════════════════════════════════════════════════════════════

/// 建项目 + v1，导入提案，双轨签，apply，freeze，导出，与 CLI 对拍。
///
/// 冲突语义（R5）也在这条线上：重复 apply / 重复 freeze / 冻结后 import。
#[tokio::test]
async fn live_console_pipeline_is_byte_identical_to_the_cli_and_conflicts_are_409() {
    let Some(_) = meta_url() else {
        return common::skip::missing("ONTOK_TEST_DATABASE_URL", "live_console_pipeline");
    };
    let Some(schema_path) = schema_path() else {
        return common::skip::missing("ONTOK_REQ124_SCHEMA", "live_console_pipeline");
    };
    let url = meta_url().unwrap();
    let pool = ontok::db::pool::connect(&url)
        .await
        .expect("连不上测试库");
    ontok::db::pg::migrate(&pool).await.expect("建表失败");

    let project = common::unique_project("req124pipe");
    let projects = PgProjectStore::new(pool.clone());
    projects
        .get_or_create(&project)
        .await
        .expect("建项目失败");
    let p = projects.get_by_name(&project).await.unwrap().unwrap();
    projects.new_version(p.id).await.expect("建 v1 失败");

    let schema = SchemaTypes::parse(
        &std::fs::read_to_string(&schema_path).expect("读 schema"),
    )
    .expect("schema 不合法");
    let svc = live_service(pool.clone(), &project, schema);
    let app = || ontok::console::http::build_router(Arc::clone(&svc));

    // ── ③ 导入（内联提案；形状与 `propose import` 的 `--file` 相同）──
    let proposal = json!({
        "project": project,
        "base_version": 1,
        "round": 1,
        "items": [{
            "kind": "add_entity_type",
            "target": "客户",
            "payload": {
                "key": "customer",
                "display_name": "客户",
                "properties": [{"name": "客户号", "column": "customer_no", "nullable": false}],
                "evidence": [{"table": "t_customer", "column": "customer_no", "claim": "列名与注释一致"}]
            }
        }]
    });
    let (status, body) = call(
        app(),
        post("/api/import", Some(ADMIN), &json!({"project": project, "proposal": proposal})),
    )
    .await;
    let v = json_of(&body);
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(v["ok"], true);
    assert_eq!(v["proposal"], "p1", "{v}");
    assert_eq!(v["items"], 1, "{v}");
    assert!(v["extracted_by"].is_null(), "人工修订 ⇒ null：{v}");

    // ── AC-008 · 与 `review serve` 同形（同一份序列化）──
    let (status, body) = call(app(), get("/api/review?proposal=p1", Some(ADMIN))).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let review = json_of(&body);
    assert_eq!(review["project"], project.as_str());
    assert_eq!(review["proposal"], "p1");
    assert_eq!(review["base_version"], 1);
    assert_eq!(review["items"].as_array().unwrap().len(), 1);
    assert_eq!(review["progress"]["total"], 1);
    assert!(review["items"][0]["suggestion"].is_object());

    // ── ④ 双轨签 → apply ──
    for track in ["biz", "model"] {
        let (status, body) = call(
            app(),
            post(
                "/api/decide",
                Some(ADMIN),
                &json!({"project": project, "proposal": "p1", "item": 1,
                        "track": track, "decision": "accept"}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{track}: {}", String::from_utf8_lossy(&body));
        let v = json_of(&body);
        assert_eq!(v["ok"], true);
        assert_eq!(v["track"], track);
        assert_eq!(v["via"][track], "console", "控制面签的必须记 console：{v}");
    }

    let (status, body) = call(
        app(),
        post("/api/apply", Some(ADMIN), &json!({"project": project, "proposal": "p1"})),
    )
    .await;
    let applied = json_of(&body);
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(applied["status"], "applied");
    assert_eq!(applied["version"], 2, "{applied}");

    // R5 · 重复 apply 同一提案 ⇒ 409（可读原因）
    let (status, body) = call(
        app(),
        post("/api/apply", Some(ADMIN), &json!({"project": project, "proposal": "p1"})),
    )
    .await;
    let v = json_of(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{}", String::from_utf8_lossy(&body));
    assert_eq!(v["code"], "PROPOSAL_NOT_PENDING");
    assert!(v["hint"].as_str().unwrap().contains("新开一版"), "{v}");

    // ── ④ freeze → 重复 freeze 409 ──
    let (status, body) = call(
        app(),
        post("/api/freeze", Some(ADMIN), &json!({"project": project, "version": 2})),
    )
    .await;
    let frozen = json_of(&body);
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(frozen["status"], "frozen");
    assert_eq!(frozen["version"], 2);

    let (status, body) = call(
        app(),
        post("/api/freeze", Some(ADMIN), &json!({"project": project, "version": 2})),
    )
    .await;
    let v = json_of(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{}", String::from_utf8_lossy(&body));
    assert_eq!(v["code"], "ALREADY_FROZEN");
    assert!(v["message"].as_str().unwrap().contains("v2"), "{v}");

    // 冻结后 import（base_version=2）⇒ 409，点名版本
    let mut again = proposal.clone();
    again["base_version"] = json!(2);
    let (status, body) = call(
        app(),
        post("/api/import", Some(ADMIN), &json!({"project": project, "proposal": again})),
    )
    .await;
    let v = json_of(&body);
    assert_eq!(status, StatusCode::CONFLICT, "{}", String::from_utf8_lossy(&body));
    assert_eq!(v["code"], "BASE_VERSION_FROZEN");
    assert!(v["message"].as_str().unwrap().contains("v2"), "{v}");

    // ── AC-002 · 项目/版本/冻结/计数（与直接查库对拍）──
    let (status, body) = call(app(), get("/api/projects", Some(ADMIN))).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let listed = json_of(&body);
    assert_eq!(listed["project"], project.as_str());
    let versions = listed["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2, "{listed}");
    assert_eq!(versions[0]["version"], 1);
    assert_eq!(versions[0]["frozen"], false);
    assert_eq!(versions[1]["version"], 2);
    assert_eq!(versions[1]["frozen"], true, "{listed}");
    // 与 SQL 逐项对拍
    let (obj, link): (i64, i64) = {
        let v2 = projects.version_by_seq(p.id, &p.name, 2).await.unwrap();
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM object_types WHERE version_id=$1),
                    (SELECT count(*) FROM link_types WHERE version_id=$1)",
        )
        .bind(v2.id)
        .fetch_one(&pool)
        .await
        .expect("对拍 SQL 失败")
    };
    assert_eq!(versions[1]["objects"], json!(obj), "{listed}");
    assert_eq!(versions[1]["relations"], json!(link), "{listed}");

    // ── AC-005 · 导出 TTL：与 CLI 逐字节相同（客户视图 + 建模视图）──
    let dir = temp_dir("ttl");
    for (model_view, tag) in [(false, "customer"), (true, "model")] {
        let out = dir.join(format!("{tag}.ttl"));
        let cli_args: Vec<String> = {
            let mut a = vec![
                "ontology".to_string(),
                "export".to_string(),
                "--project".to_string(),
                project.clone(),
                "--version".to_string(),
                "2".to_string(),
                "--out".to_string(),
                out.display().to_string(),
            ];
            if model_view {
                a.push("--model-view".to_string());
            }
            a
        };
        let args: Vec<&str> = cli_args.iter().map(String::as_str).collect();
        let run = cli(&url, &args);
        assert!(
            run.status.success(),
            "CLI 导出失败：{}",
            String::from_utf8_lossy(&run.stderr)
        );
        let cli_bytes = std::fs::read(&out).expect("读 CLI 产物");
        let uri = format!(
            "/api/export/ttl?project={project}&version=2&model_view={}",
            if model_view { 1 } else { 0 }
        );
        let (status, api_bytes) = call(app(), get(&uri, Some(ADMIN))).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&api_bytes));
        eprintln!(
            "AC-005 现场：{tag} · CLI={} 字节 sha256={} · API={} 字节 sha256={}",
            cli_bytes.len(),
            sha256_hex(&cli_bytes),
            api_bytes.len(),
            sha256_hex(&api_bytes)
        );
        assert_eq!(
            sha256_hex(&api_bytes),
            sha256_hex(&cli_bytes),
            "⛔ {tag} TTL 与 CLI **不是**逐字节相同"
        );
    }

    // ── AC-009 · 审计：写操作都有一行，且不含 token 明文 ──
    let (status, body) = call(app(), get("/api/audit?limit=50", Some(ADMIN))).await;
    assert_eq!(status, StatusCode::OK);
    let audit = json_of(&body);
    let entries = audit["entries"].as_array().unwrap();
    for action in ["import", "apply", "freeze", "decide"] {
        assert!(
            entries.iter().any(|e| e["action"] == action && e["result"] == "OK"),
            "审计里缺 {action} 的成功行：{audit}"
        );
    }
    assert!(
        entries.iter().any(|e| e["result"] == "PROPOSAL_NOT_PENDING"),
        "失败的写操作也要有一行：{audit}"
    );
    let text = String::from_utf8_lossy(&body);
    assert!(!text.contains(ADMIN), "⛔ 审计里出现了 token 明文");
    assert!(!text.contains(QUERY), "⛔ 审计里出现了 token 明文");
}

// ══════════════════════════════════════════════════════════════
// AC-005 / AC-006 · 生产快照：TTL 与 R2RML 都和 CLI 逐字节相同
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn live_export_and_r2rml_are_byte_identical_to_the_cli_on_a_frozen_production_snapshot() {
    let Some(_) = meta_url() else {
        return common::skip::missing("ONTOK_TEST_DATABASE_URL", "live_export_r2rml");
    };
    let Some(schema_path) = schema_path() else {
        return common::skip::missing("ONTOK_REQ124_SCHEMA", "live_export_r2rml");
    };
    let url = meta_url().unwrap();
    let project = env("ONTOK_REQ124_PROJECT").unwrap_or_else(|| "anspire_r2rml_excl".into());
    let version: i32 = env("ONTOK_REQ124_VERSION")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let pool = ontok::db::pool::connect(&url)
        .await
        .expect("连不上测试库");
    ontok::db::pg::migrate(&pool).await.expect("建表失败");

    let schema = SchemaTypes::parse(&std::fs::read_to_string(&schema_path).expect("读 schema"))
        .expect("schema 不合法");
    let svc = live_service(pool.clone(), &project, schema);
    let app = || ontok::console::http::build_router(Arc::clone(&svc));
    let dir = temp_dir("prod");
    let tenancy = tenancy_path();
    let schema_arg = schema_path.to_string_lossy().replace('\\', "/");
    let tenancy_arg = tenancy
        .as_ref()
        .map(|p| p.to_string_lossy().replace('\\', "/"));

    // 客户视图 TTL
    let out = dir.join("customer.ttl");
    let run = cli(
        &url,
        &[
            "ontology", "export", "--project", &project, "--version", &version.to_string(),
            "--out", &out.display().to_string(), "--schema", &schema_arg,
        ],
    );
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let cli_bytes = std::fs::read(&out).unwrap();
    let uri = format!(
        "/api/export/ttl?project={project}&version={version}&model_view=0&schema={schema_arg}"
    );
    let (status, api_bytes) = call(app(), get(&uri, Some(ADMIN))).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&api_bytes));
    eprintln!(
        "AC-005 现场（生产快照 customer）：CLI sha256={} · API sha256={}",
        sha256_hex(&cli_bytes),
        sha256_hex(&api_bytes)
    );
    assert_eq!(sha256_hex(&api_bytes), sha256_hex(&cli_bytes));

    // 建模视图 TTL（带 tenancy，如果有）
    if let Some(tenancy_arg) = &tenancy_arg {
        let out = dir.join("model.ttl");
        let run = cli(
            &url,
            &[
                "ontology", "export", "--project", &project, "--version", &version.to_string(),
                "--model-view", "--out", &out.display().to_string(),
                "--schema", &schema_arg, "--tenancy", tenancy_arg,
            ],
        );
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        let cli_bytes = std::fs::read(&out).unwrap();
        let uri = format!(
            "/api/export/ttl?project={project}&version={version}&model_view=1&schema={schema_arg}&tenancy={tenancy_arg}"
        );
        let (status, api_bytes) = call(app(), get(&uri, Some(ADMIN))).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&api_bytes));
        eprintln!(
            "AC-005 现场（生产快照 model）：CLI sha256={} · API sha256={}",
            sha256_hex(&cli_bytes),
            sha256_hex(&api_bytes)
        );
        assert_eq!(sha256_hex(&api_bytes), sha256_hex(&cli_bytes));
    }

    // R2RML（默认 + --include-inherit）
    for include_inherit in [false, true] {
        let out = dir.join(format!("r2rml-{include_inherit}.ttl"));
        let mut args: Vec<String> = vec![
            "r2rml".to_string(), "generate".to_string(),
            "--project".to_string(), project.clone(),
            "--version".to_string(), version.to_string(),
            "--schema".to_string(), schema_arg.clone(),
            "--out".to_string(), out.display().to_string(),
        ];
        if include_inherit {
            args.push("--include-inherit".to_string());
        }
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let run = cli(&url, &argv);
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        let cli_bytes = std::fs::read(&out).unwrap();
        let body = json!({
            "project": project,
            "version": version,
            "schema": schema_arg,
            "include_inherit": include_inherit,
        });
        let (status, api_bytes) = call(app(), post("/api/r2rml", Some(ADMIN), &body)).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&api_bytes));
        eprintln!(
            "AC-006 现场（include_inherit={include_inherit}）：CLI sha256={} · API sha256={}",
            sha256_hex(&cli_bytes),
            sha256_hex(&api_bytes)
        );
        assert_eq!(
            sha256_hex(&api_bytes),
            sha256_hex(&cli_bytes),
            "⛔ R2RML（include_inherit={include_inherit}）与 CLI 不是逐字节相同"
        );
    }
}
