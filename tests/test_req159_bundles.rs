//! ⭐ REQ-159 P1a · **构建包导入**的真库实测。
//!
//! 覆盖（契约版本 2026-10-27，含 key 可再次查看的变更）：
//!
//! | 判据 | 测的是什么 |
//! | --- | --- |
//! | AC-001 | 建项目返回 key；库里**只有 `sha256` + 密文**，两者都不等于明文 |
//! | AC-002 | 篡改一个文件的 sha256 ⇒ 整包拒 `bundle.sha256.mismatch` |
//! | AC-003 | 包 `project` ≠ 接收项目 ⇒ `bundle.project.mismatch` |
//! | AC-004 | `requires_dsn=true` 未配 DSN ⇒ 422 `db.dsn.missing`，且**版本数不变** |
//! | AC-005 | DSN 错误 ⇒ `database/test` 200 + `ok=false` + 分类（auth / dns） |
//! | AC-006 | 解析中途失败 ⇒ **零残留**（无新版本、无新对象行） |
//! | AC-007 | 解析成功 ⇒ 新版本**未冻结**；`activate` 后冻结 + 生效指针 |
//! | AC-008 | 同包重复 import ⇒ 幂等（不重复建版本） |
//! | AC-009 | 响应 / 审计里搜不到 DSN 明文；key 明文只出现在 create 与 reveal 两处 |
//! | AC-010 | 一项目一把：并发两把 active key ⇒ **数据库唯一索引**拒绝 |
//! | AC-011 | `GET …/key` 与创建时那把**逐字节相同**；query token 打它 ⇒ 403 `key.not_admin` |
//! | AC-012 | `GET …/key` 调用后审计里**有且只有一条** `reveal_key` |
//!
//! 另外 8 个错误码各自有一条实测（`too_large` 走纯校验路径，见那一条的注释）。
//!
//! # 环境
//!
//! `ONTOK_TEST_DATABASE_URL`（元数据库；`database/test` 的「测通了」那一条也用它当源库）。
//! 缺了就按本仓惯例**响亮地失败**（`common::test_pool` 的 panic 带完整指引）。

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::console::audit::AuditLog;
use ontok::console::decl::DeclPaths;
use ontok::console::dsn::SecretCipher;
use ontok::console::tokens::AdminTokens;
use ontok::console::{ConsoleParts, ConsoleService};
use ontok::db::project::{NewProject, PgProjectStore};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::{EntityType, LinkType, Property, Snapshot};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::{sha256_hex, AuditSink};
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use tower::ServiceExt;

const ADMIN: &str = "FAKE-REQ159-ADMIN-TOKEN-aaaaaaaaaa";
const QUERY: &str = "FAKE-REQ159-QUERY-TOKEN-bbbbbbbbbb";

const SCHEMA: &str = r#"{"tables":[
  {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true},
              {"name":"customer_name","data_type":"text"}]}
]}"#;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req159-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn service(pool: PgPool) -> Arc<ConsoleService> {
    let dir = temp_dir("decl");
    let paths = DeclPaths {
        exclusions: dir.join("exclusions.json"),
        policy: dir.join("query-policy.json"),
        tenancy: dir.join("tenancy.json"),
    };
    let tokens = AdminTokens::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"ops","token":"{ADMIN}","kind":"admin"}},
            {{"label":"agent","token":"{QUERY}","kind":"query","tenant":"C0001"}}
        ]}}"#
    ))
    .expect("token 表");
    Arc::new(ConsoleService::new(ConsoleParts {
        pool,
        // 既有端点（REQ-124）的单项目作用域；REQ-159 的新端点按路径里的 `:p` 走。
        project: "demo".into(),
        base: "https://ontok.local/".into(),
        tokens,
        schema: SchemaTypes::parse(SCHEMA).expect("夹具 schema"),
        paths,
        audit: AuditLog::new(AuditSink::stdout_only()),
        bind: "127.0.0.1:8899".parse().unwrap(),
        allow_public: false,
        secret_cipher: SecretCipher::for_tests(),
    }))
}

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value, String) {
    let res = app.clone().oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 256 * 1024 * 1024)
        .await
        .expect("读 body");
    let text = String::from_utf8_lossy(&bytes).to_string();
    let v = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, v, text)
}

fn auth_of(token: Option<&str>) -> Vec<(String, String)> {
    match token {
        Some(t) => vec![(header::AUTHORIZATION.as_str().into(), format!("Bearer {t}"))],
        None => vec![],
    }
}

async fn get(app: &axum::Router, uri: &str, token: Option<&str>) -> (StatusCode, Value, String) {
    let mut b = Request::builder().method("GET").uri(uri);
    for (k, v) in auth_of(token) {
        b = b.header(k, v);
    }
    call(app, b.body(Body::empty()).unwrap()).await
}

async fn post_json(
    app: &axum::Router,
    uri: &str,
    token: Option<&str>,
    body: &Value,
) -> (StatusCode, Value, String) {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in auth_of(token) {
        b = b.header(k, v);
    }
    call(app, b.body(Body::from(body.to_string())).unwrap()).await
}

async fn put_json(
    app: &axum::Router,
    uri: &str,
    token: Option<&str>,
    body: &Value,
) -> (StatusCode, Value, String) {
    let mut b = Request::builder()
        .method("PUT")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    for (k, v) in auth_of(token) {
        b = b.header(k, v);
    }
    call(app, b.body(Body::from(body.to_string())).unwrap()).await
}

async fn post_bytes(
    app: &axum::Router,
    uri: &str,
    token: Option<&str>,
    ct: &str,
    body: Vec<u8>,
) -> (StatusCode, Value, String) {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, ct);
    for (k, v) in auth_of(token) {
        b = b.header(k, v);
    }
    call(app, b.body(Body::from(body)).unwrap()).await
}

async fn post_empty(app: &axum::Router, uri: &str, token: Option<&str>) -> (StatusCode, Value, String) {
    let mut b = Request::builder().method("POST").uri(uri);
    for (k, v) in auth_of(token) {
        b = b.header(k, v);
    }
    call(app, b.body(Body::empty()).unwrap()).await
}

// ── zip 构造（stored-only，与构建端口径一致）─────────────────

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc ^= *b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// store-only zip（method 0、无 data descriptor、entry 顺序即入参顺序）。
fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for (name, data) in entries {
        let off = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // method 0
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc32(data).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // method 0
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc32(data).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn entity(key: &str) -> EntityType {
    EntityType {
        key: key.into(),
        display_name: format!("{key} 的中文名"),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![Property {
            name: "编号".into(),
            column: "id".into(),
            nullable: false,
            note: None,
            sample: None,
        }],
        evidence: vec![],
    }
}

fn snapshot() -> Snapshot {
    let mut s = Snapshot::new(uuid::Uuid::nil(), 5);
    s.entity_types = vec![entity("vm"), entity("disk")];
    s.link_types = vec![LinkType {
        key: "vm_has_disk".into(),
        from_type: "vm".into(),
        to_type: "disk".into(),
        cardinality: "1:N".into(),
        semantic: "has".into(),
        evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
    }];
    s
}

/// `components.am` 的声明形态。
#[derive(Clone, Copy)]
enum AmCounts {
    /// 计数与 AM 里的数组长度一致（应当收）。
    Honest,
    /// 故意多报 1（应当拒：组件计数不自洽）。
    Lying,
    /// 完全不声明 `components.am`（不对账；包照收，但 import 要给 am 的警告）。
    Undeclared,
}

/// 一个包的形状（哪些地方要「造坏」，就改这里）。
struct Spec {
    /// 包内声明的 project
    project: String,
    /// `files[]` 里的 sha256 是否按真实内容算
    honest_sha: bool,
    /// `files[]` 是否漏登记 `ontology/snapshot.json`
    omit_file_entry: bool,
    /// 是否多登记一个包里没有的文件
    extra_file_entry: bool,
    /// `bundle_version`
    bundle_version: String,
    /// 包内额外塞一个（路径非法的）entry
    evil_entry: Option<String>,
    /// 是否不带 manifest.json
    no_manifest: bool,
    /// `components.db`
    requires_dsn: bool,
    /// 快照（None ⇒ 用 [`snapshot`]）
    snapshot: Option<Snapshot>,
    /// 是否额外带 am/access-mapping.json（+ 声明计数的三种形态）
    am: Option<(Value, AmCounts)>,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            project: String::new(),
            honest_sha: true,
            omit_file_entry: false,
            extra_file_entry: false,
            bundle_version: "1.0".into(),
            evil_entry: None,
            no_manifest: false,
            requires_dsn: false,
            snapshot: None,
            am: None,
        }
    }
}

impl Spec {
    fn for_project(project: &str) -> Self {
        Self {
            project: project.to_string(),
            ..Default::default()
        }
    }
}

fn build(spec: &Spec) -> Vec<u8> {
    let snap = spec.snapshot.clone().unwrap_or_else(snapshot);
    let snap_bytes = serde_json::to_vec(&snap).unwrap();
    let snap_sha = sha256_hex(&snap_bytes);
    let props: usize = snap.entity_types.iter().map(|e| e.properties.len()).sum();

    let mut files = vec![json!({
        "path": "ontology/snapshot.json",
        "sha256": if spec.honest_sha { snap_sha.clone() } else { "0".repeat(64) },
        "bytes": snap_bytes.len(),
    })];
    if spec.omit_file_entry {
        // 只漏登记快照那一条（`files[]` 仍非空 —— 否则会先撞 schema 的 minItems: 1）
        files.retain(|f| f["path"] != "ontology/snapshot.json");
    }
    if spec.extra_file_entry {
        files.push(json!({"path": "ghost.json", "sha256": "1".repeat(64), "bytes": 1}));
    }

    let mut components = json!({
        "ontology": {
            "path": "ontology/snapshot.json",
            "sha256": if spec.honest_sha { snap_sha.clone() } else { "0".repeat(64) },
            "entity_types": snap.entity_types.len(),
            "link_types": snap.link_types.len(),
            "properties": props,
        }
    });
    if spec.requires_dsn {
        components["db"] = json!({
            "path": "db/schema.json",
            "sha256": sha256_hex(b"{\"tables\":[]}"),
            "requires_dsn": true,
            "dialect": "postgresql",
        });
        files.push(json!({
            "path": "db/schema.json",
            "sha256": sha256_hex(b"{\"tables\":[]}"),
            "bytes": "{\"tables\":[]}".len(),
        }));
    }

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some((am, mode)) = &spec.am {
        let b = serde_json::to_vec(am).unwrap();
        if !matches!(mode, AmCounts::Undeclared) {
            let bump = if matches!(mode, AmCounts::Lying) { 1 } else { 0 };
            components["am"] = json!({
                "path": "am/access-mapping.json",
                "sha256": sha256_hex(&b),
                "entities": am["entities"].as_array().map(|a| a.len()).unwrap_or(0) + bump,
                "sources": am["sources"].as_array().map(|a| a.len()).unwrap_or(0),
            });
        }
        files.push(json!({
            "path": "am/access-mapping.json",
            "sha256": sha256_hex(&b),
            "bytes": b.len(),
        }));
        entries.push(("am/access-mapping.json".to_string(), b));
    }

    let manifest = json!({
        "bundle_version": spec.bundle_version,
        "project": spec.project,
        "ontology_seq": 5,
        "built_at": "2026-10-27T10:00:00Z",
        "builder": {"name": "ontok-builder", "version": "0.2.9", "host": "lion-mbp"},
        "components": components,
        "files": files,
    });

    let mut all: Vec<(String, Vec<u8>)> = Vec::new();
    if !spec.no_manifest {
        all.push((
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ));
    }
    all.push(("ontology/snapshot.json".into(), snap_bytes));
    if spec.requires_dsn {
        all.push(("db/schema.json".into(), b"{\"tables\":[]}".to_vec()));
    }
    all.extend(entries);
    if let Some(evil) = &spec.evil_entry {
        all.push((evil.clone(), b"x".to_vec()));
    }
    zip(&all)
}

// ── 库上的小工具 ────────────────────────────────────────────

/// 一个**全新**的项目名（符合 manifest 的 `^[a-z0-9][a-z0-9-]{1,62}$`），**不建行**。
///
/// ⚠️ 不能用 `common::unique_project`：它产出 `t_{prefix}_{pid}_{n}`（含下划线），
/// 而 REQ-159 的项目名与 manifest 里 `project` 的 pattern 是**同一条**
/// （否则「包里的名字 == 接收项目名」可以成立、但包自己的 schema 不成立）。
fn fresh_name(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "t-{}-{}-{}",
        prefix,
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

/// 建一个**全新**项目行（不走 HTTP）。撞名仍然响亮失败。
async fn new_project(pool: &PgPool, prefix: &str) -> String {
    let name = fresh_name(prefix);
    PgProjectStore::new(pool.clone())
        .create(NewProject {
            name: name.clone(),
        })
        .await
        .unwrap_or_else(|e| {
            panic!("建 project `{name}` 失败（本测试要求全新名字，撞名说明被污染了）：{e}")
        });
    name
}

async fn version_count(pool: &PgPool, project: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM ontology_versions v JOIN projects p ON p.id = v.project_id
         WHERE p.name = $1",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .expect("count versions");
    n
}

async fn object_count(pool: &PgPool, project: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM object_types o
         JOIN ontology_versions v ON v.id = o.version_id
         JOIN projects p ON p.id = v.project_id WHERE p.name = $1",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .expect("count objects");
    n
}

/// 建项目（走 HTTP，顺便拿到那把 key）。
async fn create_project(app: &axum::Router, project: &str) -> Value {
    let (st, v, body) = post_json(
        app,
        "/api/projects",
        Some(ADMIN),
        &json!({"project": project, "label": "测试项目", "key_label": "数字员工：运维助手"}),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{body}");
    v
}

async fn upload(app: &axum::Router, project: &str, zip: Vec<u8>) -> (StatusCode, Value, String) {
    post_bytes(
        app,
        &format!("/api/projects/{project}/bundles"),
        Some(ADMIN),
        "application/octet-stream",
        zip,
    )
    .await
}

async fn import(app: &axum::Router, project: &str, bundle_id: &str) -> (StatusCode, Value, String) {
    post_empty(
        app,
        &format!("/api/projects/{project}/bundles/{bundle_id}/import"),
        Some(ADMIN),
    )
    .await
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-011 / AC-012 · key 的存法与再查看
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_key_is_stored_as_sha256_plus_ciphertext_never_as_plaintext() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac001");

    let out = create_project(&app, &project).await;
    let key = out["key"].as_str().expect("响应里有 key 明文").to_string();
    assert!(key.starts_with("ontok_q_"), "key 前缀不对：{key}");
    assert_eq!(key.len(), "ontok_q_".len() + 64, "32 字节 ⇒ 64 位 hex");
    assert_eq!(out["key_kind"], "query");
    assert_eq!(out["bound_project"], project.as_str());
    assert_eq!(out["key_label"], "数字员工：运维助手");
    assert_eq!(out["key_digest"].as_str().unwrap().len(), "sha256:".len() + 12);

    // 库里：摘要 == sha256(key)，密文 ≠ 明文，且**没有任何一列**等于明文。
    let (digest, ct): (String, String) =
        sqlx::query_as("SELECT token_sha256, token_ciphertext FROM project_keys k
                        JOIN projects p ON p.id = k.project_id WHERE p.name = $1")
            .bind(&project)
            .fetch_one(&pool)
            .await
            .expect("key 行");
    assert_eq!(digest, sha256_hex(key.as_bytes()), "摘要必须是 sha256(key)");
    assert!(ct.starts_with("v1:"), "密文要有版本前缀：{ct}");
    assert!(!ct.contains(&key), "密文里出现了明文 key");
    assert_ne!(ct, key);

    // 整个 project_keys 行里不许有明文 key（把整行转成文本搜一遍）
    let row_text: (String,) = sqlx::query_as(
        "SELECT k::text FROM project_keys k JOIN projects p ON p.id = k.project_id
         WHERE p.name = $1",
    )
    .bind(&project)
    .fetch_one(&pool)
    .await
    .expect("整行");
    assert!(
        !row_text.0.contains(&key),
        "project_keys 的整行文本里出现了明文 key：{}",
        row_text.0
    );
}

#[tokio::test]
async fn ac011_the_key_can_be_revealed_byte_for_byte_but_only_to_admin() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac011");
    let created = create_project(&app, &project).await;
    let key = created["key"].as_str().unwrap().to_string();

    // ① admin 能再次查看，且**逐字节相同**
    let (st, v, body) = get(&app, &format!("/api/projects/{project}/key"), Some(ADMIN)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["key"], key.as_str(), "再次查看的 key 必须与创建时逐字节相同");
    assert_eq!(v["project"], project.as_str());
    assert_eq!(v["label"], "数字员工：运维助手");
    assert!(v["revoked_at"].is_null());
    // ⛔ 响应里没有密文
    assert!(!body.contains("v1:"), "响应里不许出现密文：{body}");
    assert!(!body.contains("token_ciphertext"));

    // ② query token ⇒ 403 **key.not_admin**（这是契约指定的码，不是通用的 403）
    let (st, v, body) = get(&app, &format!("/api/projects/{project}/key"), Some(QUERY)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(v["code"], "key.not_admin", "{body}");
    assert!(!body.contains(&key), "403 的正文里不许回显 key");

    // ③ 没有 token ⇒ 401（不透露这个端点存不存在）
    let (st, v, _) = get(&app, &format!("/api/projects/{project}/key"), None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(v["code"], "UNAUTHORIZED");

    // ④ 不存在的项目 ⇒ 404
    let (st, v, _) = get(&app, "/api/projects/no-such-project-159/key", Some(ADMIN)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(v["code"], "project.not_found");
}

#[tokio::test]
async fn ac012_each_reveal_writes_exactly_one_audit_line_without_the_plaintext() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac012");
    let created = create_project(&app, &project).await;
    let key = created["key"].as_str().unwrap().to_string();

    let before = audit_entry_count(&app).await;
    let (st, _, _) = get(&app, &format!("/api/projects/{project}/key"), Some(ADMIN)).await;
    assert_eq!(st, StatusCode::OK);
    let after = audit_entry_count(&app).await;
    assert_eq!(after, before + 1, "一次 reveal 必须只多一条（AC-012）");

    let (_, v, body) = get(&app, "/api/audit?limit=50", Some(ADMIN)).await;
    let entries = v["entries"].as_array().expect("entries");
    let reveals: Vec<&Value> = entries
        .iter()
        .filter(|e| e["action"] == "reveal_key")
        .collect();
    assert_eq!(reveals.len(), 1, "审计里应当有且只有一条 reveal_key：{body}");
    let line = reveals[0].to_string();
    assert!(!line.contains(&key), "审计里出现了 key 明文：{line}");
    assert!(!line.contains("v1:"), "审计里出现了密文：{line}");
    assert!(line.contains("sha256:"), "审计里应当有摘要便于对账：{line}");
}

async fn audit_entry_count(app: &axum::Router) -> usize {
    let (_, v, body) = get(app, "/api/audit?limit=1000", Some(ADMIN)).await;
    v["count"]
        .as_u64()
        .unwrap_or_else(|| panic!("读不到审计条数：{body}")) as usize
}

#[tokio::test]
async fn ac010_one_active_key_per_project_is_enforced_by_the_database_index() {
    // 两条连接：要的是「真的并发插入」，不是串行 if。
    let pool = common::test_pool_with(2).await;
    common::migrate(&pool).await;
    let project = new_project(&pool, "req159ac010").await;
    let id = PgProjectStore::new(pool.clone())
        .get_by_name(&project)
        .await
        .unwrap()
        .unwrap()
        .id;

    let store_a = ontok::db::bundle_store::PgBundleStore::new(pool.clone());
    let store_b = ontok::db::bundle_store::PgBundleStore::new(pool.clone());
    // ⚠️ 摘要列是**全局唯一**（`token_sha256 UNIQUE`）：每次跑都用新值，
    // 否则同一个测试重跑会撞上一轮留下的那行（那是测试自身的假红）。
    let da = sha256_hex(format!("key-a-{project}").as_bytes());
    let db_ = sha256_hex(format!("key-b-{project}").as_bytes());
    let (a, b) = tokio::join!(
        store_a.create_key(id, &da, "v1:aa", "第一把"),
        store_b.create_key(id, &db_, "v1:bb", "第二把"),
    );
    let ok = usize::from(a.is_ok()) + usize::from(b.is_ok());
    let errs: Vec<String> = [a.err(), b.err()]
        .into_iter()
        .flatten()
        .map(|e| e.to_string())
        .collect();
    let msg = errs.join(" | ");
    assert_eq!(ok, 1, "一项目一把：只能有一把成功（实际成功 {ok}；错误：{msg}）");
    assert!(
        msg.contains("unique") || msg.contains("duplicate") || msg.contains("23505"),
        "拒绝必须来自**数据库唯一索引**，不是应用层的先查后插：{msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// 8 个错误码（逐条实测）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn code_bundle_manifest_invalid_when_manifest_is_missing() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cm").await;

    let spec = Spec {
        no_manifest: true,
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.manifest.invalid", "{body}");
}

#[tokio::test]
async fn code_bundle_version_unsupported() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cv").await;

    let spec = Spec {
        bundle_version: "2.0".into(),
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.version.unsupported", "{body}");
    assert!(body.contains("1.0"), "要回显支持的版本：{body}");
}

#[tokio::test]
async fn code_bundle_path_illegal_for_traversal_entries() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cp").await;

    let spec = Spec {
        evil_entry: Some("../evil.json".into()),
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.path.illegal", "{body}");
    assert!(body.contains(".."), "要点名那个路径：{body}");
}

#[tokio::test]
async fn code_bundle_files_mismatch_when_the_list_omits_a_file() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cf").await;

    let spec = Spec {
        // 包里两件（快照 + db 描述），`files[]` 只登记后者 ⇒ 集合不等
        requires_dsn: true,
        omit_file_entry: true,
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.files.mismatch", "{body}");
    assert!(body.contains("ontology/snapshot.json"), "要点名文件：{body}");
}

#[tokio::test]
async fn code_bundle_sha256_mismatch_when_a_digest_is_tampered() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cs").await;

    let spec = Spec {
        honest_sha: false,
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.sha256.mismatch", "{body}");
    assert!(body.contains("ontology/snapshot.json"), "要点名文件：{body}");
}

#[tokio::test]
async fn code_bundle_project_mismatch_when_the_bundle_belongs_to_another_project() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cprj").await;

    let spec = Spec::for_project("some-other-project");
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{body}");
    assert_eq!(v["code"], "bundle.project.mismatch", "{body}");
    assert!(body.contains("some-other-project") && body.contains(&project), "{body}");
}

#[tokio::test]
async fn code_bundle_too_large() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159cl").await;

    // 上限 + 1 MB 的**原始字节**（不是 zip）。路由的 body 上限留了 8 MB 余量，
    // 所以这一条能走到 handler 里被**我们的**判据接住（而不是 axum 的纯文本 413）。
    let big = vec![0u8; ontok::console::bundle::MAX_BUNDLE_BYTES + 1024 * 1024];
    let (st, v, body) = upload(&app, &project, big).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{body}");
    assert_eq!(v["code"], "bundle.too_large", "{body}");
    assert!(body.contains("100 MB") || body.contains("上限"), "{body}");
}

// ══════════════════════════════════════════════════════════════
// 上传 → import → activate 的主线（AC-007 / AC-008）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac007_import_creates_a_new_unfrozen_version_and_activate_freezes_it() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac007");
    create_project(&app, &project).await;

    let spec = Spec::for_project(&project);
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::CREATED, "{body}");
    assert_eq!(v["state"], "uploaded");
    assert_eq!(v["sha256_verified"], true);
    assert_eq!(v["counts"]["entity_types"], 2);
    assert_eq!(v["counts"]["link_types"], 1);
    assert_eq!(v["counts"]["properties"], 2);
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();

    // GET 状态
    let (st, v, body) = get(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["state"], "uploaded");
    assert!(v["imported_version"].is_null());
    assert_eq!(v["files"].as_array().unwrap().len(), 1);

    // import
    let (st, v, body) = import(&app, &project, &bundle_id).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let seq = v["version"].as_i64().unwrap();
    assert!(seq >= 1);
    assert_eq!(v["state"], "imported");
    assert_eq!(v["frozen"], false, "⛔ import 不许冻结");
    assert_eq!(v["already_imported"], false);
    assert_eq!(v["counts"]["entity_types"], 2);
    let version_id = uuid::Uuid::parse_str(v["version_id"].as_str().unwrap()).unwrap();

    // 库里：未冻结 + 快照内容真的落了
    let frozen: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT frozen_at FROM ontology_versions WHERE id = $1")
            .bind(version_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(frozen.0.is_none(), "import 之后版本必须**未冻结**");
    let snap = PgStore::new(pool.clone())
        .get_snapshot(version_id)
        .await
        .unwrap()
        .expect("快照");
    assert_eq!(snap.entity_types.len(), 2);
    assert_eq!(snap.seq, seq as i32, "seq 必须是**服务端重新分配**的");
    assert_eq!(snap.link_types[0].key, "vm_has_disk");

    // activate
    let (st, v, body) = post_empty(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["frozen"], true);
    assert_eq!(v["state"], "activated");
    assert_eq!(v["version_id"], version_id.to_string());

    let (active, frozen_at): (Option<uuid::Uuid>, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as(
            "SELECT p.active_version_id, v.frozen_at FROM projects p
             JOIN ontology_versions v ON v.id = p.active_version_id WHERE p.name = $1",
        )
        .bind(&project)
        .fetch_one(&pool)
        .await
        .expect("生效指针");
    assert_eq!(active, Some(version_id), "projects.active_version_id 必须指向它");
    assert!(frozen_at.is_some(), "activate 必须冻结那一版");

    // 再激活一次 ⇒ 幂等（不报错、冻结时间不刷新）
    let (st, v, body) = post_empty(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["frozen_at"], chrono::DateTime::<chrono::Utc>::from(frozen_at.unwrap())
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
}

#[tokio::test]
async fn ac008_repeated_import_of_the_same_bundle_is_idempotent() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac008");
    create_project(&app, &project).await;

    let (_, v, _) = upload(&app, &project, build(&Spec::for_project(&project))).await;
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();

    let (st1, v1, b1) = import(&app, &project, &bundle_id).await;
    assert_eq!(st1, StatusCode::OK, "{b1}");
    let n_after_first = version_count(&pool, &project).await;

    let (st2, v2, b2) = import(&app, &project, &bundle_id).await;
    assert_eq!(st2, StatusCode::OK, "重复 import 不是错误（幂等）：{b2}");
    assert_eq!(v2["already_imported"], true, "{b2}");
    assert_eq!(v2["version"], v1["version"], "必须返回**同一个**版本");
    assert_eq!(
        version_count(&pool, &project).await,
        n_after_first,
        "重复 import 不许再建版本"
    );
}

#[tokio::test]
async fn ac006_a_failed_parse_leaves_zero_residue() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac006");
    create_project(&app, &project).await;

    // 包里两个同名实体 ⇒ 写 object_types 时撞 UNIQUE(version_id, key) ⇒ 事务回滚。
    let mut bad = snapshot();
    bad.entity_types.push(entity("vm"));
    let spec = Spec {
        snapshot: Some(bad),
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::CREATED, "上传只校验结构，重复 key 要到入库才撞：{body}");
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();

    let versions_before = version_count(&pool, &project).await;
    let objects_before = object_count(&pool, &project).await;

    let (st, v, body) = import(&app, &project, &bundle_id).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(v["code"], "bundle.parse.failed", "{body}");
    assert!(
        body.contains("回滚") || body.contains("duplicate") || body.contains("唯一"),
        "错误里要说清是唯一冲突且已回滚：{body}"
    );

    let versions_after = version_count(&pool, &project).await;
    let objects_after = object_count(&pool, &project).await;
    println!(
        "[AC-006 零残留] 版本数 {} → {}（不变）；object_types 行数 {} → {}（不变）；import 返回 {} {}",
        versions_before, versions_after, objects_before, objects_after, st, v["code"]
    );
    assert_eq!(
        versions_after,
        versions_before,
        "解析失败后不许留下新版本"
    );
    assert_eq!(
        object_count(&pool, &project).await,
        objects_before,
        "解析失败后不许留下新的 object_types 行"
    );

    // 包自己的状态标记成 failed（那不是「残留」，是可查的事实）
    let (st, v, body) = get(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["state"], "failed");
    assert_eq!(v["error"]["code"], "bundle.parse.failed");
}

// ══════════════════════════════════════════════════════════════
// AC-004 / AC-005 · DSN 两条
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac004_requires_dsn_without_a_dsn_is_422_and_creates_no_version() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac004");
    create_project(&app, &project).await;

    let spec = Spec {
        requires_dsn: true,
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::CREATED, "{body}");
    assert_eq!(v["requires_db"], true);
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();

    let before = version_count(&pool, &project).await;
    let (st, v, body) = import(&app, &project, &bundle_id).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(v["code"], "db.dsn.missing", "{body}");
    assert_eq!(
        version_count(&pool, &project).await,
        before,
        "AC-004：没有 DSN 时 import 不许产生版本"
    );

    // 状态被记成 db_pending（状态机里的一档，可查）
    let (_, v, body) = get(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(v["state"], "db_pending", "{body}");
    assert_eq!(v["error"]["code"], "db.dsn.missing");

    // 配了 DSN 但**没测通**仍然拒（「配了」与「可用」是两件事）
    let (st, _, body) = put_json(
        &app,
        &format!("/api/projects/{project}/database"),
        Some(ADMIN),
        &json!({"dsn": "postgresql://nobody:wrong@127.0.0.1:1/none", "dialect": "postgresql"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    let (st, v, body) = import(&app, &project, &bundle_id).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(v["code"], "db.dsn.missing", "{body}");
    assert!(body.contains("没测通") || body.contains("测"), "{body}");
}

#[tokio::test]
async fn ac005_database_test_returns_200_with_classified_errors() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac005");
    create_project(&app, &project).await;
    let uri = format!("/api/projects/{project}/database");

    // ① 换成正确的主机 / 错口令 ⇒ auth
    let real = std::env::var("ONTOK_TEST_DATABASE_URL").expect("测试库 URL");
    let wrong_pw = with_wrong_password(&real);
    let (st, v, body) = put_json(
        &app,
        &uri,
        Some(ADMIN),
        &json!({"dsn": wrong_pw, "dialect": "postgresql"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(!body.contains("postgres123"), "PUT 的响应里出现了口令：{body}");
    assert!(v.get("dsn").is_none(), "PUT 的响应里不许有 dsn：{body}");
    assert!(v["host"].as_str().is_some() && v["database"].as_str().is_some());

    let (st, v, body) = post_empty(&app, &format!("{uri}/test"), Some(ADMIN)).await;
    assert_eq!(st, StatusCode::OK, "失败也必须 200（AC-005）：{body}");
    assert_eq!(v["ok"], false, "{body}");
    assert_eq!(v["error"]["code"], "db.connect.auth", "{body}");
    println!("[AC-005 分类] 错口令 ⇒ HTTP {} ok={} code={} :: {}", st, v["ok"], v["error"]["code"], v["error"]["message"]);
    assert!(!body.contains("postgres123"), "测试响应里出现了口令：{body}");

    // ② 解析不出来的域名 ⇒ dns
    let (st, _, _) = put_json(
        &app,
        &uri,
        Some(ADMIN),
        &json!({"dsn": "postgresql://u:p@ontok-no-such-host-159.invalid:5432/db"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, v, body) = post_empty(&app, &format!("{uri}/test"), Some(ADMIN)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["ok"], false, "{body}");
    assert_eq!(v["error"]["code"], "db.connect.dns", "{body}");
    println!("[AC-005 分类] 域名不可解析 ⇒ HTTP {} ok={} code={} :: {}", st, v["ok"], v["error"]["code"], v["error"]["message"]);

    // ③ 真能连上 ⇒ ok=true + server_version（用元数据库本身当「源库」）
    let (st, _, _) = put_json(
        &app,
        &uri,
        Some(ADMIN),
        &json!({"dsn": real, "dialect": "postgresql"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, v, body) = post_empty(&app, &format!("{uri}/test"), Some(ADMIN)).await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(v["ok"], true, "真实可达的库必须测通：{body}");
    assert!(
        v["server_version"].as_str().unwrap_or("").contains("PostgreSQL"),
        "{body}"
    );
    assert!(v["latency_ms"].as_u64().is_some());
    println!("[AC-005 分类] 真能连上 ⇒ HTTP {} ok={} latency={}ms server_version={}", st, v["ok"], v["latency_ms"], v["server_version"]);

    // ④ 从没配过 DSN 的项目 ⇒ 404 db.dsn.missing（先配再测是唯一顺序）
    let other = fresh_name("req159ac005b");
    create_project(&app, &other).await;
    let (st, v, body) = post_empty(
        &app,
        &format!("/api/projects/{other}/database/test"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(v["code"], "db.dsn.missing", "{body}");
}

/// 把一条 DSN 的口令换成错的（其余不动）。
fn with_wrong_password(dsn: &str) -> String {
    match dsn.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((userinfo, tail)) => {
                let user = userinfo.split(':').next().unwrap_or("u");
                format!("{scheme}://{user}:definitely-wrong-159@{tail}")
            }
            None => format!("{dsn}"),
        },
        None => dsn.to_string(),
    }
}

// ══════════════════════════════════════════════════════════════
// AC-009 · 明文不外泄（响应 + 审计）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac009_no_dsn_plaintext_anywhere_and_key_only_in_the_two_places() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159ac009");
    let created = create_project(&app, &project).await;
    let key = created["key"].as_str().unwrap().to_string();

    let real = std::env::var("ONTOK_TEST_DATABASE_URL").expect("测试库 URL");
    let secret = "pg-super-secret-159".to_string();
    let dsn = with_password(&real, &secret);

    let (st, _, put_body) = put_json(
        &app,
        &format!("/api/projects/{project}/database"),
        Some(ADMIN),
        &json!({"dsn": dsn}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{put_body}");
    let (_, _, test_body) = post_empty(
        &app,
        &format!("/api/projects/{project}/database/test"),
        Some(ADMIN),
    )
    .await;
    let (_, v, upload_body) = upload(&app, &project, build(&Spec::for_project(&project))).await;
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();
    let (_, _, import_body) = import(&app, &project, &bundle_id).await;
    let (_, _, activate_body) = post_empty(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
        Some(ADMIN),
    )
    .await;
    let (_, _, bundle_body) = get(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}"),
        Some(ADMIN),
    )
    .await;

    // ① DSN 明文（含口令）在**任何**响应里都搜不到
    for (what, body) in [
        ("PUT database", &put_body),
        ("database/test", &test_body),
        ("upload", &upload_body),
        ("import", &import_body),
        ("activate", &activate_body),
        ("GET bundle", &bundle_body),
    ] {
        assert!(!body.contains(&secret), "{what} 的响应里出现了 DSN 口令：{body}");
        assert!(!body.contains(&dsn), "{what} 的响应里出现了整条 DSN：{body}");
    }

    // ② key 明文只出现在 create 与 reveal 两个响应里 —— 别的响应一个都没有
    for (what, body) in [
        ("PUT database", &put_body),
        ("database/test", &test_body),
        ("upload", &upload_body),
        ("import", &import_body),
        ("activate", &activate_body),
        ("GET bundle", &bundle_body),
    ] {
        assert!(!body.contains(&key), "{what} 的响应里出现了 key 明文：{body}");
    }

    // ③ 审计里既没有 DSN 明文，也没有 key 明文 / 密文
    let (_, _, audit_body) = get(&app, "/api/audit?limit=1000", Some(ADMIN)).await;
    assert!(!audit_body.contains(&secret), "审计里出现了 DSN 口令：{audit_body}");
    assert!(!audit_body.contains(&dsn), "审计里出现了整条 DSN");
    assert!(!audit_body.contains(&key), "审计里出现了 key 明文");
    assert!(!audit_body.contains("v1:"), "审计里出现了密文");
    // 但审计必须留下了可对账的痕迹（否则「谁配的 DSN」无从回答）
    assert!(audit_body.contains("database.put"), "{audit_body}");
    assert!(audit_body.contains(&project), "{audit_body}");

    // ④ 库里存的是密文，不是明文
    let (ct,): (String,) =
        sqlx::query_as("SELECT d.dsn_ciphertext FROM project_databases d
                        JOIN projects p ON p.id = d.project_id WHERE p.name = $1")
            .bind(&project)
            .fetch_one(&pool)
            .await
            .expect("DSN 行");
    assert!(ct.starts_with("v1:"), "{ct}");
    assert!(!ct.contains(&secret), "密文里出现了口令");
}

fn with_password(dsn: &str, password: &str) -> String {
    match dsn.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((userinfo, tail)) => {
                let user = userinfo.split(':').next().unwrap_or("u");
                format!("{scheme}://{user}:{password}@{tail}")
            }
            None => dsn.to_string(),
        },
        None => dsn.to_string(),
    }
}

// ══════════════════════════════════════════════════════════════
// 杂项判据
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn multipart_upload_is_rejected_with_a_pointer_to_octet_stream() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159mp").await;

    let (st, v, body) = post_bytes(
        &app,
        &format!("/api/projects/{project}/bundles"),
        Some(ADMIN),
        "multipart/form-data; boundary=X",
        b"--X\r\nContent-Disposition: form-data; name=\"file\"\r\n\r\nPK\r\n--X--\r\n".to_vec(),
    )
    .await;
    assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");
    assert_eq!(v["code"], "bundle.upload.invalid", "{body}");
    assert!(body.contains("octet-stream"), "要点名正确用法：{body}");
}

#[tokio::test]
async fn unknown_project_and_bundle_are_404_not_405() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));

    // 项目不存在 ⇒ 404（前端的「服务端未就绪」判据只认 404）
    let (st, v, body) = upload(&app, "no-such-project-159", build(&Spec::for_project("x"))).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(v["code"], "project.not_found");
    println!("[404-1 项目不存在] {} {} ⇒ {} {}", "POST", "/api/projects/no-such-project-159/bundles", st, v["code"]);

    let project = fresh_name("req159n404");
    create_project(&app, &project).await;
    let (st, v, body) = get(
        &app,
        &format!("/api/projects/{project}/bundles/bdl_{}", uuid::Uuid::new_v4()),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(v["code"], "bundle.not_found");
    println!("[404-2 包不存在] GET /api/projects/<p>/bundles/bdl_<random> ⇒ {} {}", st, v["code"]);

    // 没导入就 activate ⇒ 409（状态机问题，不是 404）
    let (_, v, _) = upload(&app, &project, build(&Spec::for_project(&project))).await;
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();
    let (st, v, body) = post_empty(
        &app,
        &format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
        Some(ADMIN),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{body}");
    assert_eq!(v["code"], "bundle.not_imported", "{body}");

    // ⭐ 未注册的路径 ⇒ **404**（⛔ 不是 405）：前端只把 404 当「服务端未就绪」。
    //    两条都取 P1a 明确**不做**的端点，保证这条判据不会因为「后来实现了」而失效。
    for (method, uri) in [
        // demand §五-8：项目状态（P1a 不做）
        ("GET", format!("/api/projects/{project}")),
        // demand §五-9：换 key（P1a 不做）
        ("POST", format!("/api/projects/{project}/key/rotate")),
    ] {
        let mut b = Request::builder()
            .method(method)
            .uri(&uri)
            .header(header::AUTHORIZATION, format!("Bearer {ADMIN}"));
        if method == "POST" {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        let (st, v, body) = call(&app, b.body(Body::from("{}")).unwrap()).await;
        assert_eq!(
            st,
            StatusCode::NOT_FOUND,
            "{method} {uri} 必须是 404（未上线），收到 {st}：{body}"
        );
        assert_eq!(v["code"], "NOT_FOUND", "{body}");
        println!("[404-3 未注册路径] {method} {uri} ⇒ {st} {}（hint 里列了现有端点）", v["code"]);
        // 404 的 hint 要列出现有端点，否则前端与人都不知道有什么可用
        assert!(
            v["hint"]
                .as_str()
                .unwrap_or("")
                .contains("/api/projects/:p/bundles"),
            "404 的 hint 要列出现有端点：{body}"
        );
    }
}

#[tokio::test]
async fn creating_a_project_twice_is_409_and_needs_a_non_empty_key_label() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159dup");

    create_project(&app, &project).await;
    let (st, v, body) = post_json(
        &app,
        "/api/projects",
        Some(ADMIN),
        &json!({"project": project, "key_label": "又一次"}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{body}");
    assert_eq!(v["code"], "project.exists", "{body}");

    // key_label 必填（一项目一把 key ⇒ 「给谁用」必须写下来）
    let (st, v, body) = post_json(
        &app,
        "/api/projects",
        Some(ADMIN),
        &json!({"project": fresh_name("brandnew"), "key_label": ""}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "project.label.required", "{body}");

    // 项目名 pattern 与 manifest 同源
    let (st, v, body) = post_json(
        &app,
        "/api/projects",
        Some(ADMIN),
        &json!({"project": "Bad_Name", "key_label": "x"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "project.name.invalid", "{body}");
}

/// ⭐ 契约更正（2026-10-27）：项目名 pattern 允许**下划线**
/// （`^[a-z0-9][a-z0-9_-]{1,62}$`）—— 库里既有项目名大量带 `_`
/// （`req129_ac006_22140_1793033086208` 一类），而这条 pattern 同时是
/// `POST /api/projects` 与 `manifest.project` 的同一条判据 ⇒ 禁掉 `_`
/// 等于让历史项目既建不了行、也打不了包（真库已实测复现）。
///
/// 两面都钉：① 带下划线的名字**真的能建出来**（不是只放行字符串校验）；
/// ② 旧行为**不回退**（大写 / 空格 / `..` / 前导下划线仍然拒）。
#[tokio::test]
async fn an_underscore_project_name_is_accepted_but_the_old_rejections_still_hold() {
    // ① 纯函数面：pattern 本体的正例 / 反例。
    for ok in ["foo_bar_159", "a_b", "req129_ac006_22140_1793033086208", "t_1"] {
        assert!(
            ontok::console::bundle::valid_project_name(ok),
            "`{ok}` 应当合法（下划线已放行）"
        );
    }
    for bad in ["Bad_Name", "foo bar", "..", "_leading", "-leading", "a", "foo/bar"] {
        assert!(
            !ontok::console::bundle::valid_project_name(bad),
            "`{bad}` 必须仍然被拒（旧行为不回退）"
        );
    }

    // ② 真建行：带下划线的名字打 HTTP 也得 CREATED（而不是 400 project.name.invalid）。
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let name = format!("foo_bar_159_{}", std::process::id());
    let (st, v, body) = post_json(
        &app,
        "/api/projects",
        Some(ADMIN),
        &json!({"project": name, "key_label": "下划线项目"}),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "带下划线的项目名必须能建：{body}");
    assert_eq!(v["project"], name.as_str(), "{body}");

    // ③ 同一个端点：大写仍然拒（旧行为不回退，防「顺手把整个校验删了」）。
    let (st, v, body) = post_json(
        &app,
        "/api/projects",
        Some(ADMIN),
        &json!({"project": "Bad_Name", "key_label": "x"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "project.name.invalid", "{body}");
}

#[tokio::test]
async fn every_new_endpoint_rejects_a_query_token_with_403() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = new_project(&pool, "req159q403").await;

    // ⚠️ 除 `GET …/key`（契约给的码是 key.not_admin）之外，其余都是 TOKEN_KIND_FORBIDDEN。
    for (method, uri) in [
        ("POST", "/api/projects".to_string()),
        ("GET", format!("/api/projects/{project}/key")),
        ("POST", format!("/api/projects/{project}/bundles")),
        ("GET", format!("/api/projects/{project}/bundles/x")),
        ("PUT", format!("/api/projects/{project}/database")),
        ("POST", format!("/api/projects/{project}/database/test")),
    ] {
        let mut b = Request::builder()
            .method(method)
            .uri(&uri)
            .header(header::AUTHORIZATION, format!("Bearer {QUERY}"));
        if method == "POST" || method == "PUT" {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        let (st, v, body) = call(&app, b.body(Body::from("{}")).unwrap()).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{method} {uri}: {body}");
        let want = if uri.ends_with("/key") {
            "key.not_admin"
        } else {
            "TOKEN_KIND_FORBIDDEN"
        };
        assert_eq!(v["code"], want, "{method} {uri}: {body}");
    }
}

#[tokio::test]
async fn am_component_counts_are_reconciled_with_the_am_json() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159am");
    create_project(&app, &project).await;

    // ① 声明的 entities 比 AM 里实际多 1 ⇒ 拒（组件计数不自洽）
    let am = json!({
        "contract_version": 2,
        "mapping_version": 1,
        "project": project,
        "sources": [{"id": "s1", "kind": "http", "base_url": "https://x"}],
        "entities": [],
    });
    let spec = Spec {
        am: Some((am.clone(), AmCounts::Lying)),
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.manifest.invalid", "{body}");
    assert!(body.contains("components.am.entities"), "{body}");

    // ② 不声明计数 ⇒ 不对账，包照收（schema 里这些计数可选）
    let spec = Spec {
        am: Some((am, AmCounts::Undeclared)),
        ..Spec::for_project(&project)
    };
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    assert_eq!(st, StatusCode::CREATED, "{body}");
    assert_eq!(v["requires_db"], false);
    // 包里有 am，但 v1 的 import 只落本体快照 —— 这条要在 warnings 里说出来
    let bundle_id = v["bundle_id"].as_str().unwrap().to_string();
    let (_, v, body) = import(&app, &project, &bundle_id).await;
    assert!(
        v["warnings"]
            .as_array()
            .map(|a| a.iter().any(|w| w.as_str().unwrap_or("").contains("am/")))
            .unwrap_or(false),
        "带 am 的包 import 时要给出警告：{body}"
    );
}

#[tokio::test]
async fn the_manifest_project_field_is_matched_verbatim() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let app = ontok::console::http::build_router(service(pool.clone()));
    let project = fresh_name("req159verbatim");
    create_project(&app, &project).await;

    // 大小写不同也算不同名（防串包靠逐字相同）
    let spec = Spec::for_project(&project.to_uppercase());
    let (st, v, body) = upload(&app, &project, build(&spec)).await;
    // 大写字母过不了 manifest 的 project pattern ⇒ manifest.invalid；先确认这一点
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(v["code"], "bundle.manifest.invalid", "{body}");
}

// 让 `NewProject` 的导入不白导（common 里的 new_project 用的就是它）
#[allow(dead_code)]
fn _unused(p: NewProject) -> String {
    p.name
}

// ══════════════════════════════════════════════════════════════
// 迁移幂等（连跑 N 次不报错）+ 新列/新索引真的在
// ══════════════════════════════════════════════════════════════

/// ⭐ 迁移**幂等**的判据：同一个库上连跑三次 `migrate()`，一次都不许报错。
///
/// 这不是「跑一次能建表」—— 那只能证明首次迁移。真正会出问题的是**第二次**：
/// 稳态库上 `ALTER TABLE … ADD COLUMN IF NOT EXISTS` 仍取 `AccessExclusiveLock`
/// （见 `db::pg::patched_columns_present` 的锁表），而重复 `CREATE TABLE` /
/// `CREATE INDEX IF NOT EXISTS` 必须是 no-op。
#[tokio::test]
async fn migration_is_idempotent_and_the_new_columns_and_index_exist() {
    let pool = common::test_pool().await;
    for round in 1..=3 {
        ontok::db::pg::migrate(&pool)
            .await
            .unwrap_or_else(|e| panic!("第 {round} 次 migrate 失败（幂等性被破坏）：{e}"));
        println!("[迁移幂等] 第 {round} 次 migrate() ⇒ Ok（同一个库，连跑三次）");
    }

    // 四张表都在
    for table in [
        "project_keys",
        "project_bundles",
        "project_bundle_payloads",
        "project_databases",
    ] {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM information_schema.tables
             WHERE table_schema = current_schema() AND table_name = $1",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "表 {table} 不存在");
    }

    // `projects` 的两列
    for col in ["label", "active_version_id"] {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = 'projects'
               AND column_name = $1",
        )
        .bind(col)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "projects.{col} 不存在（ALTER_PATCHES 没生效）");
    }

    // key 的两列（摘要 + 可逆密文）
    for col in ["token_sha256", "token_ciphertext"] {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM information_schema.columns
             WHERE table_schema = current_schema() AND table_name = 'project_keys'
               AND column_name = $1",
        )
        .bind(col)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 1, "project_keys.{col} 不存在");
    }

    // 「一项目一把」的部分唯一索引（AC-010 的落点）
    let (idx,): (String,) = sqlx::query_as(
        "SELECT indexdef FROM pg_indexes
         WHERE schemaname = current_schema() AND indexname = 'project_keys_one_active'",
    )
    .fetch_one(&pool)
    .await
    .expect("project_keys_one_active 索引不存在");
    assert!(idx.contains("UNIQUE"), "索引必须是 UNIQUE：{idx}");
    assert!(idx.contains("revoked_at IS NULL"), "索引必须是**部分**索引（一项目一把）：{idx}");
}