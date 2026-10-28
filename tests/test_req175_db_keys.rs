//! ⭐ REQ-175 · **建项目发的 key 必须能鉴权** —— `POST /api/projects` 拿到的 key
//! 调 `GET /api/catalog` 必须 **200**（静态表未命中时按摘要查 `project_keys`）。
//!
//! # 判据（能红能绿）
//!
//! | 判据 | 测的是什么 | 怎么变红 |
//! | --- | --- | --- |
//! | **AC-001** | 真进程 + 真库：`POST /api/projects` 发的 key ⇒ `GET /api/catalog` **200** 且 `project` 是它绑的那个 | 把「按摘要查库」注掉 ⇒ 401 |
//! | **AC-002** | 同一把 key ⇒ `GET /api/projects` **403 `key.not_admin`** | DB key 被当成 admin ⇒ 200/403 别的码 |
//! | **AC-003** | 同前缀、改末位的伪造 key ⇒ **401**（⛔ 不因「前缀像」放行） | 比较前缀 / 放宽等值 ⇒ 放行 |
//! | **AC-004** | 吊销（`revoked_at`）后 **≤ TTL 变 401**；TTL 内命中缓存（不每请求打库） | 没有缓存 ⇒ 第二条断言红；缓存永不过期 ⇒ 第三条红 |
//! | **AC-005** | 单项目模式：DB key 绑**另一个**项目 ⇒ **403 `key.project_mismatch`** | 复用 `key.not_admin` / 放行 ⇒ 红 |
//! | **AC-006** | 静态 token **优先**：同一摘要既在静态表又在库里 ⇒ 用静态表那条（且一次库都不查） | 先查库 ⇒ 项目变成 DB 那条 |
//! | **AC-007** | 审计里只有摘要（`sha256:` + 12 位），⛔ 没有 token 原文 | 审计写原文 ⇒ 红 |
//! | **追加** | token 绑**不存在的项目** ⇒ **404 `project.not_found`**（⛔ 不是 500） | 状态码改回 500 ⇒ 红 |
//!
//! # 环境
//!
//! - `ONTOK_TEST_DATABASE_URL`（元数据库，库名需含 test/scratch/tmp/ci/dev）**必需**；
//! - `ONTOK_SECRET_KEY`（64 位十六进制）—— 起真进程时必须给（写面加载加密器）。
//!
//! AC-001/002/003/005/追加 起**真进程**（`ontok query serve`）+ 真库；AC-004/006 用
//! 进程内路由 + 真库（它们要构造**毫秒级 TTL** / 同摘要的静态表，CLI 没有这两个旋钮）。
//!
//! # 已知边界
//!
//! `project_keys` **没有租户列** ⇒ DB key 的 `tenant = None`。本文件全部用本体服务模式
//! （`--ontology-only`，没有租户概念），所以 catalog / describe / TTL 都是正常路径；
//! 普通模式（连业务库）下的 SPARQL 会得到「租户为空」的参数错误，那是这张表缺少
//! 租户归属的既有限制，不在 REQ-175 的范围里。

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
use ontok::db::bundle_store::PgBundleStore;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::{EntityType, Property, Snapshot};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::{sha256_hex, AuditSink};
use ontok::query::db_keys::{DbKeyAuth, KEY_PROJECT_MISMATCH};
use ontok::query::http::build_router_multi;
use ontok::query::multi::MultiProject;
use ontok::query::serve::Deployment;
use ontok::query::tokens::TokenTable;
use ontok::query::ServiceMode;
use ontok::sparql::policy::QueryPolicy;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

/// 写面的 admin key（不绑项目）。长度 ≥ 24（`MIN_TOKEN_LEN`）。
const ADMIN: &str = "FAKE-REQ175-ADMIN-aaaaaaaaaaaaaaaaaaaa";

/// 本体服务模式的占位租户声明（与 `query::serve::NO_TENANT_POLICY` 同一个形状）。
const NO_TENANT: &str = r#"{"version":"1.0.0","tenantSubject":"__no_tenant__",
    "tenantIdColumn":"__no_tenant__","perTable":{},"publicDimensions":[]}"#;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req175-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn db_url() -> String {
    ontok::config::test_database_url().expect(NEED_TEST_DB)
}

fn secret_key() -> String {
    "ab".repeat(32)
}

fn unique(tag: &str) -> String {
    format!(
        "req175-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
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

fn snapshot_for(version_id: Uuid, seq: i32, classes: &[&str]) -> Snapshot {
    let mut s = Snapshot::new(version_id, seq);
    s.entity_types = classes.iter().map(|k| entity(k)).collect();
    s
}

/// 建项目 + 一个版本（**不激活**），返回版本 id。项目名已存在时复用它
/// （AC-001 的项目就是 `POST /api/projects` 刚建出来的那一个）。
async fn make_project(pool: &PgPool, name: &str, classes: &[&str]) -> Uuid {
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = projects.get_or_create(name).await.expect("建项目");
    let v = projects.new_version(p.id).await.expect("新版本");
    store
        .save_snapshot(&snapshot_for(v.id, v.seq, classes))
        .await
        .expect("存快照");
    v.id
}

async fn activate(pool: &PgPool, name: &str, version_id: Uuid) {
    let projects = PgProjectStore::new(pool.clone());
    let p = projects
        .get_by_name(name)
        .await
        .expect("读项目")
        .expect("项目在");
    projects.activate(p.id, version_id).await.expect("激活");
}

/// 往 `project_keys` 插一行**有效** key（只给鉴权用：密文是占位串，审计/鉴权都不解密）。
async fn insert_key(pool: &PgPool, project: &str, token: &str, label: &str) -> Uuid {
    let p = PgProjectStore::new(pool.clone())
        .get_by_name(project)
        .await
        .expect("读项目")
        .expect("项目在");
    let digest = sha256_hex(token.as_bytes());
    let row = PgBundleStore::new(pool.clone())
        .create_key(
            p.id,
            &digest,
            "v1:req175-fixture-not-a-real-ciphertext",
            label,
        )
        .await
        .expect("插 key 行");
    row.id
}

async fn cleanup(pool: &PgPool, names: &[String]) {
    for n in names {
        // `project_keys` / `ontology_versions` / 快照五表都是 ON DELETE CASCADE。
        let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(n)
            .execute(pool)
            .await;
    }
}

/// 多项目模式的部署级输入（**本体服务模式**：不连业务库 ⇒ 判据不需要源库）。
fn deployment() -> Deployment {
    Deployment {
        mode: ServiceMode::OntologyOnly,
        schema: SchemaTypes::default(),
        schema_path: None,
        policy: QueryPolicy::parse(NO_TENANT).expect("占位声明"),
        base: "https://ontok.local/".into(),
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

// ══════════════════════════════════════════════════════════════
// 进程内路由（AC-004 / AC-006）
// ══════════════════════════════════════════════════════════════

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value, String) {
    let res = app.clone().oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读 body");
    let text = String::from_utf8_lossy(&bytes).to_string();
    let v = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, v, text)
}

async fn get(app: &axum::Router, uri: &str, token: Option<&str>) -> (StatusCode, Value, String) {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION.as_str(), format!("Bearer {t}"));
    }
    call(app, b.body(Body::empty()).unwrap()).await
}

// ══════════════════════════════════════════════════════════════
// 真进程 + 裸 HTTP（AC-001/002/003/005/追加）
// ══════════════════════════════════════════════════════════════

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("要一个空闲端口");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// 起一个真进程；`mode_args` 决定单项目 / 多项目（多项目 = 只给 `--ontology-only`）。
fn spawn(bin: &Path, tokens: &Path, mode_args: &[&str], audit: Option<&Path>) -> Server {
    let port = free_port();
    let mut cmd = Command::new(bin);
    cmd.args([
        "query",
        "serve",
        "--bind",
        &format!("127.0.0.1:{port}"),
        "--tokens",
        tokens.to_str().unwrap(),
    ])
    .args(mode_args)
    .env("ONTOK_DATABASE_URL", db_url())
    .env("ONTOK_SECRET_KEY", secret_key())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    if let Some(path) = audit {
        cmd.args(["--audit-out", path.to_str().unwrap()]);
    }
    let child = cmd.spawn().expect("起 ontok 子进程");
    let srv = Server { child, port };
    wait_ready(port);
    srv
}

fn wait_ready(port: u16) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if let Ok((200, _)) =
            std::panic::catch_unwind(|| http(port, "GET", "/api/health", &Vec::new(), &Vec::new()))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("服务在 30 秒内没有就绪（端口 {port}）");
}

/// 裸 HTTP/1.1（`Connection: close`）：返回 `(状态码, 正文)`。
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("连上服务");
    s.set_read_timeout(Some(Duration::from_secs(20)))
        .expect("设读超时");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).expect("写请求头");
    s.write_all(body).expect("写请求体");
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
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    (status, if chunked { dechunk(raw) } else { raw.to_vec() })
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = raw.windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_txt = String::from_utf8_lossy(&raw[..eol]).to_string();
        let size = usize::from_str_radix(size_txt.trim().split(';').next().unwrap_or("0"), 16)
            .unwrap_or(0);
        raw = &raw[eol + 2..];
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[..size.min(raw.len())]);
        raw = &raw[(size + 2).min(raw.len())..];
    }
    out
}

fn json_of(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap_or(Value::Null)
}

fn bearer(token: &str) -> Vec<(String, String)> {
    vec![("Authorization".to_string(), format!("Bearer {token}"))]
}

fn with_json(mut h: Vec<(String, String)>) -> Vec<(String, String)> {
    h.push(("Content-Type".to_string(), "application/json".to_string()));
    h
}

fn tokens_admin_only() -> String {
    format!(
        r#"{{"version":2,"tokens":[{{"label":"fde","token":"{ADMIN}","kind":"admin"}}]}}"#
    )
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-002 / AC-003 / AC-007 · 真进程 + 真库
// ══════════════════════════════════════════════════════════════

/// **AC-001** · `POST /api/projects` 发的 key 调 `GET /api/catalog` ⇒ **200**，且返回的
/// `project` 就是这把 key 绑的项目。
///
/// 这条是 REQ-175 的**主判据**：生产上同一个动作在 REQ-175 之前返回 401。
///
/// 变异（红绿自证）：把 `DbKeyAuth::resolve` 的查库那一步注掉 / 让 `require_token`
/// 不读 DB key ⇒ 本用例在 `assert_eq!(status, 200)` 处红。
#[tokio::test]
async fn ac_001_the_key_issued_by_post_projects_authenticates_catalog() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let project = unique("ac1");
    let dir = temp_dir("ac1");
    let tokens = dir.join("tokens.json");
    let audit = dir.join("audit.log");
    std::fs::write(&tokens, tokens_admin_only()).expect("写 token 表");
    let srv = spawn(&ontok_bin(), &tokens, &["--ontology-only"], Some(&audit));

    // ① 生产路径：建项目 + 发 key（只有 admin 能做）。
    let payload = json!({
        "project": project,
        "label": "REQ-175 · DB key 鉴权",
        "key_label": "数字员工：探针",
    });
    let (status, body) = http(
        srv.port,
        "POST",
        "/api/projects",
        &with_json(bearer(ADMIN)),
        payload.to_string().as_bytes(),
    );
    assert_eq!(
        status,
        201,
        "POST /api/projects 应当 201：{}",
        String::from_utf8_lossy(&body)
    );
    let made = json_of(&body);
    let key = made["key"]
        .as_str()
        .expect("回执里必须有一次性 key")
        .to_string();
    assert!(key.starts_with("ontok_q_"), "key 前缀是产品约定：{key}");
    assert_eq!(
        key.len(),
        "ontok_q_".len() + 64,
        "key = 前缀 + 32 字节十六进制：{key}"
    );

    // ② 让这个项目有生效版本（本用例只判「发的 key 能鉴权」，不重测上传/导入链路）。
    let vid = make_project(&pool, &project, &["vm", "disk"]).await;
    activate(&pool, &project, vid).await;

    // ③ AC-001：同一把 key ⇒ catalog 200，且是它绑的那个项目。
    let (status, body) = http(
        srv.port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &bearer(&key),
        &[],
    );
    assert_eq!(
        status,
        200,
        "POST /api/projects 发的 key 必须能鉴权（REQ-175 的主判据）：{}",
        String::from_utf8_lossy(&body)
    );
    let cat = json_of(&body);
    assert_eq!(cat["project"], project.as_str(), "必须服务 key 绑的项目：{cat}");
    assert!(cat["items"].is_array(), "catalog 形状不变：{cat}");

    // ④ AC-002：同一把 key 打写面 ⇒ 403 `key.not_admin`（读 key 不能写）。
    let (status, body) = http(srv.port, "GET", "/api/projects", &bearer(&key), &[]);
    assert_eq!(
        status,
        403,
        "DB 发的 query key 列项目应当 403：{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(json_of(&body)["code"], "key.not_admin");

    // ⑤ AC-003：同前缀、只改末位的伪造 key ⇒ 401（⛔ 不能因为前缀像就放行）。
    let mut forged = key.clone();
    let last = forged.pop().expect("key 非空");
    forged.push(if last == '0' { '1' } else { '0' });
    assert_ne!(forged, key);
    assert_eq!(&forged[..key.len() - 1], &key[..key.len() - 1], "只差末位");
    let (status, body) = http(
        srv.port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &bearer(&forged),
        &[],
    );
    assert_eq!(
        status,
        401,
        "伪造 key 必须 401（不是 403、更不是 200）：{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(json_of(&body)["code"], "UNAUTHORIZED");

    // ⑥ AC-007：审计里**只有摘要**（sha256: + 12 位），⛔ 没有 token 原文。
    let digest = sha256_hex(key.as_bytes());
    let wanted = format!("sha256:{}", &digest[..12]);
    let audit_text = std::fs::read_to_string(&audit).expect("审计文件应当被写出来");
    assert!(
        audit_text.contains(&wanted),
        "审计里要有这把 DB key 的摘要前缀 {wanted}：\n{audit_text}"
    );
    assert!(
        !audit_text.contains(&key),
        "⛔ 审计里不许出现 token 原文：\n{audit_text}"
    );
    // 成功那次 catalog 的那一行也得是 OK（证明确实是鉴权通过后记的账）。
    assert!(
        audit_text.lines().any(|l| l.contains("event=catalog") && l.contains("code=OK")),
        "审计里要有一条成功的 catalog：\n{audit_text}"
    );

    drop(srv);
    cleanup(&pool, &[project]).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 吊销 ⇒ ≤ TTL 内 401（真库；TTL 缩到毫秒级）
// ══════════════════════════════════════════════════════════════

/// **AC-004** · `revoked_at` 置值后，缓存**最多** TTL 就失效 ⇒ 401。
///
/// 判据把 TTL 缩到 **2s**（生产是 60s）并显式验证三件事：
/// ① 未吊销 ⇒ 200 且进了缓存；② **立刻**吊销后、TTL 内仍是 200（证明缓存真的在挡库，
/// 不是每请求打库 —— 这也是「≤ TTL」这个承诺的另一半）；③ 过 TTL ⇒ **401**。
///
/// 变异：注掉 `cached_project` 的 TTL 判断 ⇒ ③ 红（永远 200）；让 `resolve` 每次都
/// 打库 ⇒ ② 红（吊销立刻生效，与缓存契约不符）。
#[tokio::test]
async fn ac_004_a_revoked_db_key_becomes_401_within_the_ttl() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let project = unique("ac4");
    let vid = make_project(&pool, &project, &["vm"]).await;
    activate(&pool, &project, vid).await;

    // key 形状与生产一致（前缀 + 64 hex），但内容**每次不同**：共享库上重跑不会撞
    // `project_keys.token_sha256` 的 UNIQUE（上一轮用例 panic 时留下的行也不会干扰）。
    let token = format!("ontok_q_{}", sha256_hex(unique("ac4-token").as_bytes()));
    let key_id = insert_key(&pool, &project, &token, "agent").await;

    let db_keys = Arc::new(DbKeyAuth::with_ttl(pool.clone(), Duration::from_secs(2)));
    let multi = Arc::new(MultiProject::with_db_keys(
        pool.clone(),
        TokenTable::default(),
        deployment(),
        AuditSink::stdout_only(),
        Arc::clone(&db_keys),
    ));
    let app = build_router_multi(Arc::clone(&multi));

    // ① 预热两次：第一次把快照装起来（慢的那一次），第二次只走鉴权 + 指针（快）——
    //    否则「第一次请求的装快照耗时」会吃掉 TTL，让下面那条「TTL 内仍命中」变成
    //    与加载速度有关的假红。
    for _ in 0..2 {
        let (status, body, text) = get(&app, "/api/catalog?page=1&page_size=50", Some(&token)).await;
        assert_eq!(status, StatusCode::OK, "有效 DB key 应当 200：{text}");
        assert_eq!(body["project"], project.as_str());
    }
    assert_eq!(db_keys.cached_len(), 1, "命中必须进缓存（键 = 摘要）");

    // ② 立刻吊销：TTL 内仍 200（缓存命中 —— ⛔ 不是每请求打库）。
    sqlx::query("UPDATE project_keys SET revoked_at = now() WHERE id = $1")
        .bind(key_id)
        .execute(&pool)
        .await
        .expect("吊销");
    let (status, _, text) = get(&app, "/api/catalog?page=1&page_size=50", Some(&token)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "TTL 内缓存仍命中（60s 语义；这一条证明没有每请求打库）：{text}"
    );

    // ③ 过 TTL ⇒ 重新查库 ⇒ 已吊销 ⇒ 401。
    tokio::time::sleep(Duration::from_millis(2_300)).await;
    let (status, body, text) = get(&app, "/api/catalog?page=1&page_size=50", Some(&token)).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "吊销后 ≤ TTL 必须变 401（⛔ 不是 403：403 会泄露「这把 key 存在」）：{text}"
    );
    assert_eq!(body["code"], "UNAUTHORIZED");
    assert_eq!(db_keys.cached_len(), 0, "查库确认吊销后要清掉那条缓存");

    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 单项目模式：DB key 绑另一个项目 ⇒ 403 key.project_mismatch
// ══════════════════════════════════════════════════════════════

/// **AC-005** · 真进程 + 真库：单项目模式（`--project A --version 1`）下
///
/// - 一把绑 **A** 的 DB key ⇒ **200**（DB key 在单项目模式也能用）；
/// - 一把绑 **B**（另一个项目）的 DB key ⇒ **403 `key.project_mismatch`**；
/// - 另起一个钉在 **B** 的进程，同一把 key ⇒ **200**（证明 403 不是「key 坏了」，
///   而是「这个部署不服务那个项目」）。
///
/// 变异：把 mismatch 复用 `key.not_admin` / 直接放行 ⇒ 第二条红（码或状态不对）。
#[tokio::test]
async fn ac_005_a_db_key_bound_to_another_project_is_403_project_mismatch() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let a = unique("ac5a");
    let b = unique("ac5b");
    let v_a = make_project(&pool, &a, &["vm", "disk"]).await;
    activate(&pool, &a, v_a).await;
    // A 的 DB key（直接插库：`POST /api/projects` 对已存在的项目是 409）。
    let token_a = format!("ontok_q_{}", sha256_hex(unique("ac5-token").as_bytes()));
    insert_key(&pool, &a, &token_a, "agent-a").await;

    let dir = temp_dir("ac5");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_admin_only()).expect("写 token 表");

    // P1：钉在 A。admin token 能建项目 B 并给它发 key。
    let srv_a = spawn(
        &ontok_bin(),
        &tokens,
        &["--project", &a, "--version", "1", "--ontology-only"],
        None,
    );

    // ① 单项目模式 + DB key 绑的就是 --project ⇒ 200。
    let (status, body) = http(
        srv_a.port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &bearer(&token_a),
        &[],
    );
    assert_eq!(
        status,
        200,
        "绑 A 的 DB key 在钉 A 的进程里应当 200：{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(json_of(&body)["project"], a.as_str());

    // ② 用 admin 建项目 B ⇒ B 的 DB key；再给 B 加版本并激活。
    let payload = json!({"project": b, "label": "REQ-175 AC-005", "key_label": "agent-b"});
    let (status, body) = http(
        srv_a.port,
        "POST",
        "/api/projects",
        &with_json(bearer(ADMIN)),
        payload.to_string().as_bytes(),
    );
    assert_eq!(
        status,
        201,
        "建项目 B 应当 201：{}",
        String::from_utf8_lossy(&body)
    );
    let key_b = json_of(&body)["key"]
        .as_str()
        .expect("B 的 key")
        .to_string();
    let v_b = make_project(&pool, &b, &["bill", "item", "price"]).await;
    activate(&pool, &b, v_b).await;

    // ③ AC-005：绑 B 的 key 打钉在 A 的进程 ⇒ 403 `key.project_mismatch`。
    let (status, body) = http(
        srv_a.port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &bearer(&key_b),
        &[],
    );
    let text = String::from_utf8_lossy(&body).to_string();
    assert_eq!(status, 403, "绑另一个项目的 key 必须 403（⛔ 不是 200/401）：{text}");
    let err = json_of(&body);
    assert_eq!(err["code"], KEY_PROJECT_MISMATCH, "{err}");
    assert_eq!(err["code"], "key.project_mismatch", "新码要在错误码表登记：{err}");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or("")
            .contains(&b) && err["message"].as_str().unwrap_or("").contains(&a),
        "消息要点名两个项目：{err}"
    );
    assert!(
        !err["hint"].as_str().unwrap_or("").is_empty(),
        "403 要给可行动的 hint：{err}"
    );

    // ④ 钉在 B 的进程里，同一把 key ⇒ 200（证明 403 是项目不匹配，不是 key 无效）。
    let srv_b = spawn(
        &ontok_bin(),
        &tokens,
        &["--project", &b, "--version", "1", "--ontology-only"],
        None,
    );
    let (status, body) = http(
        srv_b.port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &bearer(&key_b),
        &[],
    );
    assert_eq!(
        status,
        200,
        "钉在 B 的进程里，绑 B 的 key 应当 200：{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(json_of(&body)["project"], b.as_str());

    drop(srv_a);
    drop(srv_b);
    cleanup(&pool, &[a, b]).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 静态 token 优先（同一个摘要也在库里）
// ══════════════════════════════════════════════════════════════

/// **AC-006** · 同一个 `sha256` 既在**静态表**又在 `project_keys` 里 ⇒ 用**静态表**
/// 那一条（项目 A），且**一次库都不为鉴权查**（`cached_len() == 0`）。
///
/// 这条比「既有测试全绿」更硬：它不是「静态 token 还能用」，而是「静态表优先」。
///
/// 变异：把 `resolve_multi` 改成先查库 ⇒ 项目变成 B（类数不同）⇒ 红；
/// 即使两个项目类数相同，`cached_len() == 1` 也会红。
#[tokio::test]
async fn ac_006_the_static_token_wins_over_a_db_row_with_the_same_digest() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let a = unique("ac6a");
    let b = unique("ac6b");
    let v_a = make_project(&pool, &a, &["vm", "disk"]).await;
    let v_b = make_project(&pool, &b, &["bill", "item", "price"]).await;
    activate(&pool, &a, v_a).await;
    activate(&pool, &b, v_b).await;

    // 同一把 token：静态表绑 A，库里那一行绑 B。token 每次不同（共享库上重跑不撞 UNIQUE）。
    let token = format!("FAKE-REQ175-STATIC-{}", unique("ac6-token"));
    insert_key(&pool, &b, &token, "db-row-same-digest").await;

    let tokens = TokenTable::parse_multi(
        &format!(
            r#"{{"version":2,"tokens":[
                {{"label":"static-a","token":"{token}","project":"{a}"}}
            ]}}"#
        ),
        ServiceMode::OntologyOnly,
    )
    .expect("静态表");
    let db_keys = Arc::new(DbKeyAuth::new(pool.clone()));
    let multi = Arc::new(MultiProject::with_db_keys(
        pool.clone(),
        tokens,
        deployment(),
        AuditSink::stdout_only(),
        Arc::clone(&db_keys),
    ));
    let app = build_router_multi(Arc::clone(&multi));

    let (status, body, text) = get(&app, "/api/catalog?page=1&page_size=50", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(
        body["project"],
        a.as_str(),
        "⛔ 静态表优先：项目必须是静态表里的 A，不是库里那行绑的 B：{body}"
    );
    assert_eq!(
        db_keys.cached_len(),
        0,
        "静态表命中时**一次库都不该为鉴权查**（REQ-175 的「行为逐字节不变」）"
    );

    cleanup(&pool, &[a, b]).await;
}

// ══════════════════════════════════════════════════════════════
// 追加（2026-10-28 生产实测）· token 绑不存在的项目 ⇒ 404，不是 500
// ══════════════════════════════════════════════════════════════

/// **追加判据** · 多项目模式 + 真进程 + 真库：token 表里 `project` 写成**不存在**的名字
/// ⇒ **404 `project.not_found`**（配置错），⛔ 不是 500（那会让运维以为进程挂了）。
///
/// 同一批的 `no_active_version` 仍是 503、`snapshot_missing` / `load_failed` 仍是 500
/// —— 本用例只钉这一个码的状态。
#[tokio::test]
async fn the_token_bound_to_a_missing_project_is_404_not_500() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let missing = unique("missing");
    let dir = temp_dir("404");
    let tokens = dir.join("tokens.json");
    let query = "FAKE-REQ175-MISSING-PROJECT-bbbbbbbbbb";
    // 多项目模式：query key 必须绑项目；这个项目**故意不存在**。
    std::fs::write(
        &tokens,
        format!(
            r#"{{"version":2,"tokens":[
                {{"label":"fde","token":"{ADMIN}","kind":"admin"}},
                {{"label":"agent","token":"{query}","kind":"query","project":"{missing}"}}
            ]}}"#
        ),
    )
    .expect("写 token 表");

    let srv = spawn(&ontok_bin(), &tokens, &["--ontology-only"], None);
    let (status, body) = http(
        srv.port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &bearer(query),
        &[],
    );
    let text = String::from_utf8_lossy(&body).to_string();
    assert_eq!(
        status,
        404,
        "token 绑了不存在的项目是**配置错** ⇒ 404（⛔ 不是 500）：{text}"
    );
    let err = json_of(&body);
    assert_eq!(err["code"], "project.not_found", "{err}");
    assert!(
        err["message"].as_str().unwrap_or("").contains(&missing),
        "消息要点名那个不存在的项目：{err}"
    );
    assert!(
        err["hint"].as_str().unwrap_or("").contains("token 表"),
        "hint 要说清改 token 表（不要动）—— 这版文案是生产实测后确认过的：{err}"
    );

    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
}
