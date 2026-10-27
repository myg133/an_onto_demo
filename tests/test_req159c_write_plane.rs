//! ⭐ REQ-159 **P1c-1** · 写面（8 条管理端点）搬到 `query serve` 的真库 / 真进程判据。
//!
//! 背景：这 8 条原本做在 `console`（CLI 在 `ontok-builder` 里，**不部署到服务器**）
//! ⇒ 部署不出去。ADR-013 决定把它们挂到 `query serve`，由 Studio（`/studio`）调用。
//!
//! | 判据 | 测的是什么 |
//! | --- | --- |
//! | **AC-c1** | 真进程 + 真库：create → upload → import → activate → reveal key 整条链跑通 |
//! | **AC-c2** | `kind=query` 的 key 调**每一条**写端点 ⇒ 403 `key.not_admin`；读端点仍 200 |
//! | **AC-c3** | `kind=admin` 带 `project` ⇒ **拒绝启动**并点名（admin 不绑项目） |
//! | **AC-c4** | 缺 `ONTOK_SECRET_KEY` ⇒ 在**绑 socket 之前**退出 + 人话 |
//! | **AC-c5** | 不给 `?project=` 时，读路径与**基线二进制**（`ONTOK_BASELINE_BIN`）status + body sha256 全等 |
//! | **AC-c6** | 上传的 sha256 被篡改 ⇒ 整包拒（复用 P1a 的判据，证明搬过来没丢校验） |
//! | **AC-c7** | create / upload / import / activate / reveal 各一行审计，⛔ 无 key / DSN 明文 |
//! | **AC-c8** | admin + `?project=B` ⇒ 返回 B 项目的数据（两项目类数不同） |
//! | **AC-c9** | query key + `?project=` ⇒ 403 `key.project_pinned`；**不给**该参数时仍正常 |
//!
//! # 环境
//!
//! `ONTOK_TEST_DATABASE_URL`（元数据库）+ `ONTOK_SECRET_KEY`（64 位十六进制）。
//! AC-c5 还要 `ONTOK_BASELINE_BIN`（`1c418ce` 编出来的 `ontok.exe`）—— 没给就**响亮跳过**，
//! ⛔ 不假装通过。
//!
//! ⚠️ 本文件的用例都起**真进程**（`ontok query serve`）：鉴权 / 启动闸门 / 逐字节比对
//! 这三类判据在进程内是**测不到的**（它们的行为就发生在「进程怎么起」这一步）。

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ontok::domain::snapshot::{EntityType, Property, Snapshot};
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// admin key（写面）。长度 ≥ 24（`MIN_TOKEN_LEN`）。
const ADMIN: &str = "FAKE-REQ159C-ADMIN-aaaaaaaaaaaaaaaaaaaa";
/// 数字员工的查询 key。
const QUERY: &str = "FAKE-REQ159C-QUERY-bbbbbbbbbbbbbbbbbbbb";

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req159c-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 每个用例自己的项目名（共享库上并行跑不会互相踩）。
///
/// ⚠️ 必须过 `pack::is_valid_project_name`（P1a 起允许下划线，但本文件只用小写/数字/连字符）。
fn unique(tag: &str) -> String {
    format!(
        "req159c-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn db_url() -> String {
    ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL")
}

fn secret_key() -> String {
    // 64 位十六进制（32 字节）。⛔ 不写死密钥 —— 判据只要「有且形状对」。
    "ab".repeat(32)
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

/// 用**生产打包器**（`bundle::pack::assemble`，与 `ontok-builder` 同一个函数）造一个真包。
///
/// 为什么不用手搓 zip：手搓的那份只能证明「我们的校验认我们自己拼的字节」，
/// 而这里要证明的是「搬过来的端点仍然接得住**构建端真的会产出的那种包**」。
fn bundle_for(project: &str, classes: &[&str]) -> Vec<u8> {
    let input = ontok::bundle::pack::PackInput::new(
        project,
        1,
        "2026-10-27T02:30:00Z",
        snapshot_for(Uuid::nil(), 1, classes),
    );
    ontok::bundle::pack::assemble(&input).expect("打包")
}

// ─────────────────── 库：建项目 / 激活（判据的夹具，不经过被测端点） ───────────────────

async fn make_project(pool: &PgPool, name: &str, classes: &[&str]) -> Uuid {
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    let store = ontok::db::snapshot_store::PgStore::new(pool.clone());
    let p = projects.get_or_create(name).await.expect("建项目");
    let v = projects.new_version(p.id).await.expect("新版本");
    let snap = snapshot_for(v.id, v.seq, classes);
    store.save_snapshot(&snap).await.expect("存快照");
    v.id
}

async fn activate(pool: &PgPool, name: &str, version_id: Uuid) {
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    let p = projects
        .get_by_name(name)
        .await
        .expect("读项目")
        .expect("项目在");
    projects.activate(p.id, version_id).await.expect("激活");
}

async fn cleanup(pool: &PgPool, names: &[String]) {
    for n in names {
        // `ontology_versions` / 快照五表都是 ON DELETE CASCADE。
        let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(n)
            .execute(pool)
            .await;
    }
}

// ─────────────────── 起进程 / 裸 HTTP（判据只用标准库） ───────────────────

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

/// token 表（多项目模式：query 必须绑项目；admin **不绑**）。
fn tokens_multi(query_project: Option<&str>) -> String {
    let query = match query_project {
        Some(p) => format!(
            r#"{{"label":"agent","token":"{QUERY}","kind":"query","project":"{p}"}}"#
        ),
        None => String::new(),
    };
    let sep = if query.is_empty() { "" } else { "," };
    format!(
        r#"{{"version":2,"tokens":[
            {{"label":"fde","token":"{ADMIN}","kind":"admin"}}{sep}{query}
        ]}}"#
    )
}

/// **既有部署**形状的 token 表：query 条目**不写 `kind`**（那是 P1c-1 才有的字段）。
///
/// 为什么这条必须单独造一份：AC-c2 要证明的不是「显式 `kind=query` 会被挡」——
/// 那太容易了；要证明的是**升级二进制之后，旧表里那张查数 key 仍然只是查数 key**
/// （缺 `kind` ⇒ `query`）。变异（红绿自证 ②）：把 `TokenKind` 的 `#[default]`
/// 挪到 `Admin` ⇒ 旧表这张 key 变成写钥匙 ⇒ 本用例拿到 400/200 而不是 403 ⇒ 红。
fn tokens_multi_legacy(query_project: &str) -> String {
    format!(
        r#"{{"version":2,"tokens":[
            {{"label":"fde","token":"{ADMIN}","kind":"admin"}},
            {{"label":"agent","token":"{QUERY}","project":"{query_project}"}}
        ]}}"#
    )
}

/// 起一个**多项目模式**的服务（`--ontology-only`：判据不需要客户源库）。
fn spawn_multi(
    bin: &Path,
    tokens: &Path,
    audit: Option<&Path>,
    extra: &[&str],
) -> Server {
    let port = free_port();
    let mut cmd = Command::new(bin);
    cmd.args([
        "query",
        "serve",
        "--bind",
        &format!("127.0.0.1:{port}"),
        "--tokens",
        tokens.to_str().unwrap(),
        "--ontology-only",
    ])
    .args(extra)
    .env("ONTOK_DATABASE_URL", db_url())
    .env("ONTOK_SECRET_KEY", secret_key())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    if let Some(a) = audit {
        cmd.arg("--audit-out").arg(a);
    }
    let child = cmd.spawn().expect("起 ontok 子进程");
    let srv = Server { child, port };
    wait_ready(port);
    srv
}

/// 跑一次**预期会失败**的启动，返回 (exit_success, stderr)。
fn run_expecting_failure(bin: &Path, args: &[&str], env: &[(&str, Option<&str>)]) -> (bool, String) {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .env("ONTOK_DATABASE_URL", db_url())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (k, v) in env {
        match v {
            Some(v) => {
                cmd.env(k, v);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }
    let out = cmd.output().expect("跑 ontok");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
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
fn http(port: u16, method: &str, path: &str, headers: &[(String, String)], body: &[u8]) -> (u16, Vec<u8>) {
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

fn sha(body: &[u8]) -> String {
    ontok::query::audit::sha256_hex(body)
}

// ══════════════════════════════════════════════════════════════
// AC-c1 · 真进程 + 真库：create → upload → import → activate → reveal
// ══════════════════════════════════════════════════════════════

/// **AC-c1** · 8 条端点里最要紧的那条链，在 `query serve` 上跑通。
///
/// 判据比「每个端点各自 200」更强：**同一把 key** 从 create 拿到、经 upload/import/
/// activate 之后，`GET …/key` 必须id 返回它 —— 中间任何一环把 key 换了 / 密文写坏了，
/// 这条断言就红。
#[tokio::test]
async fn ac_c1_create_upload_import_activate_and_reveal_the_key() {
    let pool = common::test_pool().await;
    let dir = temp_dir("c1");
    let tokens = dir.join("tokens.json");
    // query key 绑一个没人用的项目名（本用例只走 admin）。
    std::fs::write(&tokens, tokens_multi(Some("req159c-not-used"))).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens, None, &Vec::new());
    let port = srv.port;
    let admin = bearer(ADMIN);

    let project = unique("c1");
    let body = json!({"project": project, "label": "P1c-1 链路", "key_label": "数字员工：运维助手"})
        .to_string();
    let (status, body) =
        http(port, "POST", "/api/projects", &with_json(bearer(ADMIN)), body.as_bytes());
    assert_eq!(status, 201, "建项目应当 201：{}", String::from_utf8_lossy(&body));
    let created = json_of(&body);
    let key_1 = created["key"].as_str().expect("回执里必须有 key").to_string();
    assert_eq!(created["project"], project.as_str());
    assert_eq!(created["key_kind"], "query");
    assert!(!key_1.is_empty(), "key 不能是空的");
    eprintln!(
        "· [1/5 create ] POST /api/projects ⇒ 201 · project={project} · key_kind=query · key=sha256:{}",
        &ontok::query::audit::sha256_hex(key_1.as_bytes())[..12]
    );

    // ② 上传（原始字节，不是 multipart）
    let bundle = bundle_for(&project, &["vm", "disk"]);
    let mut h = bearer(ADMIN);
    h.push(("Content-Type".to_string(), "application/octet-stream".to_string()));
    let (status, body) = http(
        port,
        "POST",
        &format!("/api/projects/{project}/bundles"),
        &h,
        &bundle,
    );
    assert_eq!(status, 201, "上传应当 201：{}", String::from_utf8_lossy(&body));
    let up = json_of(&body);
    let bundle_id = up["bundle_id"].as_str().expect("bundle_id").to_string();
    assert_eq!(up["state"], "uploaded");
    assert_eq!(up["sha256_verified"], true, "上传必须校验过 sha256：{up}");
    eprintln!(
        "· [2/5 upload ] POST …/bundles ⇒ 201 · bundle={bundle_id} · bytes={} · sha256_verified=true",
        up["bytes"]
    );

    // ③ 包状态（只读）
    let (status, body) = http(
        port,
        "GET",
        &format!("/api/projects/{project}/bundles/{bundle_id}"),
        &admin,
        &[],
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(json_of(&body)["bundle_id"], bundle_id.as_str());

    // ④ 解析入库（不冻结）
    let (status, body) = http(
        port,
        "POST",
        &format!("/api/projects/{project}/bundles/{bundle_id}/import"),
        &admin,
        &[],
    );
    assert_eq!(status, 200, "解析应当 200：{}", String::from_utf8_lossy(&body));
    let imported = json_of(&body);
    assert_eq!(imported["version"], 1, "第一版：{imported}");
    eprintln!(
        "· [3/5 import ] POST …/import ⇒ 200 · version={} · frozen={}",
        imported["version"], imported["frozen"]
    );

    // ⑤ 冻结 + 生效
    let (status, body) = http(
        port,
        "POST",
        &format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
        &admin,
        &[],
    );
    assert_eq!(status, 200, "激活应当 200：{}", String::from_utf8_lossy(&body));
    eprintln!(
        "· [4/5 activat] POST …/activate ⇒ 200 · version={} · frozen={} · active=true",
        json_of(&body)["version"], json_of(&body)["frozen"]
    );

    // ⑥ 再次查看 key：必须与创建时**逐字节相同**
    let (status, body) = http(port, "GET", &format!("/api/projects/{project}/key"), &admin, &Vec::new());
    assert_eq!(status, 200, "查看 key 应当 200：{}", String::from_utf8_lossy(&body));
    let revealed = json_of(&body);
    assert_eq!(
        revealed["key"], key_1.as_str(),
        "查看回来的 key 与创建时不是同一把（密文列写坏了？）"
    );
    eprintln!(
        "· [5/5 key    ] GET …/key ⇒ 200 · key=sha256:{} · 与创建时逐字节相同",
        &ontok::query::audit::sha256_hex(revealed["key"].as_str().unwrap_or("").as_bytes())[..12]
    );

    // ⑦ 真库里确实有这一版，并且已冻结（证明「端点说成功」不是嘴上说的）
    let frozen: Option<(Option<chrono::DateTime<chrono::Utc>>,)> = sqlx::query_as(
        "SELECT v.frozen_at FROM ontology_versions v JOIN projects p ON p.id = v.project_id \
         WHERE p.name = $1 ORDER BY v.seq DESC LIMIT 1",
    )
    .bind(&project)
    .fetch_optional(&pool)
    .await
    .expect("查版本");
    assert!(
        frozen.and_then(|f| f.0).is_some(),
        "activate 之后库里那一版必须已冻结"
    );

    let mut srv = srv;
    srv.stop();
    cleanup(&pool, &[project]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c2 · query key 调 8 条写端点 ⇒ 403 key.not_admin；读端点仍 200
// ══════════════════════════════════════════════════════════════

/// **AC-c2** · 每一条写端点都必须把数字员工的 key 挡在门外，码**统一**是 `key.not_admin`。
///
/// 变异（红绿自证 ①）：把 `require_token` / `resolve_multi` 里的 `is_write_path` 判断去掉
/// ⇒ 8 条里会有一条给出 500/400/200（而不是 403）⇒ 本用例红。
/// 变异（红绿自证 ②）：把 `TokenKind::default` 改成 `Admin` ⇒ 这张 query key 变成 admin
/// ⇒ 不再是 403 ⇒ 红。
#[tokio::test]
async fn ac_c2_a_query_key_gets_403_key_not_admin_on_every_write_endpoint() {
    let pool = common::test_pool().await;
    let project = unique("c2");
    let vid = make_project(&pool, &project, &["vm", "disk"]).await;
    activate(&pool, &project, vid).await;

    let dir = temp_dir("c2");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi_legacy(&project)).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens, None, &Vec::new());
    let port = srv.port;

    let fake_id = "00000000-0000-0000-0000-000000000000";
    let cases: Vec<(&str, &str, &str, Vec<u8>)> = vec![
        ("建项目", "POST", "/api/projects", br#"{"project":"x"}"#.to_vec()),
        (
            "上传",
            "POST",
            "/api/projects/x/bundles",
            vec![0u8, 1, 2, 3],
        ),
        (
            "包状态",
            "GET",
            "/api/projects/x/bundles/00000000-0000-0000-0000-000000000000",
            vec![],
        ),
        (
            "解析",
            "POST",
            "/api/projects/x/bundles/00000000-0000-0000-0000-000000000000/import",
            vec![],
        ),
        (
            "激活",
            "POST",
            "/api/projects/x/bundles/00000000-0000-0000-0000-000000000000/activate",
            vec![],
        ),
        (
            "配 DSN",
            "PUT",
            "/api/projects/x/database",
            br#"{"dsn":"postgresql://u:p@h/d"}"#.to_vec(),
        ),
        ("测连通", "POST", "/api/projects/x/database/test", vec![]),
        ("查看 key", "GET", "/api/projects/x/key", vec![]),
    ];
    assert_eq!(cases.len(), 8, "8 条写端点一条都不许漏");
    for (what, method, path, body) in &cases {
        let (status, res) = http(
            port,
            method,
            path,
            &with_json(bearer(QUERY)),
            body,
        );
        assert_eq!(status, 403, "[{what}] 应当 403：{}", String::from_utf8_lossy(&res));
        let v = json_of(&res);
        assert_eq!(
            v["code"], "key.not_admin",
            "[{what}] 403 的码必须是 key.not_admin（契约/Studio 前端只认这个）：{v}"
        );
        assert!(
            v["hint"].as_str().unwrap_or("").contains("admin"),
            "[{what}] hint 要说清用什么：{v}"
        );
    }

    // 读端点**一个都没变**：同一把 query key 读 catalog / describe / ttl 全是 200。
    for uri in [
        "/api/catalog?page=1&page_size=50",
        "/api/describe?name=vm",
        "/api/ontology.ttl",
    ] {
        let (status, body) = http(port, "GET", uri, &bearer(QUERY), &Vec::new());
        assert_eq!(status, 200, "[读 {uri}] query key 必须照常能读：{}", String::from_utf8_lossy(&body));
    }
    let _ = fake_id;

    let mut srv = srv;
    srv.stop();
    cleanup(&pool, &[project]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c3 · admin 带 project ⇒ 拒绝启动
// ══════════════════════════════════════════════════════════════

/// **AC-c3** · `kind=admin` 的条目**不许**绑项目（与 console 同一条规则）；
/// 写了就**整份拒绝启动**，并点名第几条 / 哪个 label。
#[test]
fn ac_c3_an_admin_key_with_a_project_refuses_to_start() {
    let dir = temp_dir("c3");
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        format!(
            r#"{{"version":2,"tokens":[
                {{"label":"fde-ok","token":"{ADMIN}","kind":"admin"}},
                {{"label":"fde-wrong","token":"{QUERY}","kind":"admin","project":"cloud-resource"}}
            ]}}"#
        ),
    )
    .expect("写 token 表");

    let port = free_port();
    let (ok, stderr) = run_expecting_failure(
        &ontok_bin(),
        &[
            "query",
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--tokens",
            tokens.to_str().unwrap(),
            "--ontology-only",
        ],
        &[("ONTOK_SECRET_KEY", Some(&secret_key()))],
    );
    assert!(!ok, "admin 带 project 必须拒绝启动：\n{stderr}");
    assert!(stderr.contains("第 2 条"), "要点名第几条：\n{stderr}");
    assert!(stderr.contains("fde-wrong"), "要点名 label：\n{stderr}");
    assert!(stderr.contains("不绑项目"), "{stderr}");
    assert!(
        !stderr.contains("就绪"),
        "拒绝了就不该打就绪横幅（闸门在绑 socket 之前）：\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c4 · 缺 ONTOK_SECRET_KEY ⇒ 绑 socket 之前退出
// ══════════════════════════════════════════════════════════════

/// **AC-c4** · 没有 `ONTOK_SECRET_KEY` ⇒ 进程**非 0 退出**，话里说清缺什么 / 怎么补 /
/// 换密钥的后果；并且**端口没有被监听**（证明它确实在绑 socket 之前就退了）。
#[test]
fn ac_c4_a_missing_secret_key_exits_before_binding_the_socket() {
    let dir = temp_dir("c4");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(Some("req159c-c4-project"))).expect("写 token 表");

    let port = free_port();
    let (ok, stderr) = run_expecting_failure(
        &ontok_bin(),
        &[
            "query",
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--tokens",
            tokens.to_str().unwrap(),
            "--ontology-only",
        ],
        &[("ONTOK_SECRET_KEY", None), ("ONTOK_DSN_KEY", None)],
    );
    assert!(!ok, "缺密钥必须拒绝启动：\n{stderr}");
    for needle in [
        "ONTOK_SECRET_KEY",
        "拒绝启动",
        "openssl rand -hex 32",
        "同一把",
        "写面",
    ] {
        assert!(stderr.contains(needle), "缺 {needle:?}：\n{stderr}");
    }
    assert!(!stderr.contains("就绪"), "⛔ 不该走到就绪横幅：\n{stderr}");
    // 端口没被监听（进程已经退了；这里再确认真实退出而不是「起来但报错」）。
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "缺密钥时不该有人在 {port} 上监听"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c6 · 篡改 sha256 ⇒ 整包拒（搬过来没丢校验）
// ══════════════════════════════════════════════════════════════

/// **AC-c6** · 把包内 `ontology/snapshot.json` 的内容换掉（manifest 里的 sha256 不动）
/// ⇒ 上传必须整包拒，码还是 P1a 那一个 `bundle.sha256.mismatch`。
///
/// 为什么这条必须在新端点上再跑一遍：包解析是同一条代码，但**入口换了**
/// （body 上限、Content-Type、错误映射都在新的一层上）—— 那些都可能把校验绕过或吞掉。
#[tokio::test]
async fn ac_c6_a_tampered_file_digest_rejects_the_whole_bundle() {
    let pool = common::test_pool().await;
    let dir = temp_dir("c6");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(Some("req159c-not-used"))).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens, None, &Vec::new());
    let port = srv.port;

    let project = unique("c6");
    let (status, body) = http(
        port,
        "POST",
        "/api/projects",
        &with_json(bearer(ADMIN)),
        json!({"project": project, "key_label": "c6"}).to_string().as_bytes(),
    );
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));

    // 造一个**真包**，然后把快照内容改掉、manifest 不动 ⇒ 摘要对不上。
    let good = bundle_for(&project, &["vm", "disk"]);
    let entries = ontok::bundle::zip::read(&good).expect("读包");
    let mut tampered: Vec<(String, Vec<u8>)> = Vec::new();
    for e in entries {
        let data = if e.path == "ontology/snapshot.json" {
            let mut t = e.data.clone();
            // 只改一个字节：内容变了、长度不变（于是错的只能是摘要，不是结构）。
            let at = t.len() / 2;
            t[at] = if t[at] == b'x' { b'y' } else { b'x' };
            t
        } else {
            e.data.clone()
        };
        tampered.push((e.path.clone(), data));
    }
    let bad = ontok::bundle::zip::write(&tampered).expect("重打包");

    let mut h = bearer(ADMIN);
    h.push(("Content-Type".to_string(), "application/octet-stream".to_string()));
    let (status, body) = http(
        port,
        "POST",
        &format!("/api/projects/{project}/bundles"),
        &h,
        &bad,
    );
    assert!(
        (400..500).contains(&status),
        "篡改的包必须整包拒（拿到 {status}）：{}",
        String::from_utf8_lossy(&body)
    );
    let v = json_of(&body);
    assert_eq!(
        v["code"], "bundle.sha256.mismatch",
        "码必须还是 P1a 那一个：{v}"
    );
    // 真库里**没有**落下这个包（拒绝 = 零残留）。
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT b.bundle_sha256 FROM project_bundles b JOIN projects p ON p.id = b.project_id WHERE p.name = $1")
            .bind(&project)
            .fetch_all(&pool)
            .await
            .expect("查包");
    assert!(rows.is_empty(), "被拒的包不该落库：{rows:?}");

    let mut srv = srv;
    srv.stop();
    cleanup(&pool, &[project]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c7 · 审计：每个写操作一行，⛔ 无 key / DSN 明文
// ══════════════════════════════════════════════════════════════

/// **AC-c7** · create / upload / import / activate / reveal 各留一行审计
/// （`--audit-out` 那份文件），且文件里搜不到 key 明文 / DSN 明文 / token 明文。
#[tokio::test]
async fn ac_c7_each_write_leaves_one_audit_line_without_any_secret() {
    let pool = common::test_pool().await;
    let dir = temp_dir("c7");
    let tokens = dir.join("tokens.json");
    let audit = dir.join("audit.log");
    std::fs::write(&tokens, tokens_multi(Some("req159c-not-used"))).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens, Some(&audit), &Vec::new());
    let port = srv.port;
    let admin = bearer(ADMIN);

    let project = unique("c7");
    let (status, body) = http(
        port,
        "POST",
        "/api/projects",
        &with_json(bearer(ADMIN)),
        json!({"project": project, "key_label": "c7"}).to_string().as_bytes(),
    );
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));
    let key = json_of(&body)["key"].as_str().expect("key").to_string();

    let bundle = bundle_for(&project, &["vm", "disk"]);
    let mut h = bearer(ADMIN);
    h.push(("Content-Type".to_string(), "application/octet-stream".to_string()));
    let (status, body) = http(
        port,
        "POST",
        &format!("/api/projects/{project}/bundles"),
        &h,
        &bundle,
    );
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));
    let bundle_id = json_of(&body)["bundle_id"].as_str().expect("id").to_string();

    for path in [
        format!("/api/projects/{project}/bundles/{bundle_id}/import"),
        format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
    ] {
        let (status, body) = http(port, "POST", &path, &admin, &Vec::new());
        assert_eq!(status, 200, "{path}：{}", String::from_utf8_lossy(&body));
    }
    let (status, _) = http(port, "GET", &format!("/api/projects/{project}/key"), &admin, &Vec::new());
    assert_eq!(status, 200);

    let mut srv = srv;
    srv.stop(); // 先停，确保文件写完（审计是逐行 write + 不缓冲，但这里让判据确定）

    let text = std::fs::read_to_string(&audit).expect("审计文件");
    for action in [
        "action=project.create",
        "action=bundle.upload",
        "action=bundle.import",
        "action=bundle.activate",
        "action=reveal_key",
    ] {
        assert!(text.contains(action), "审计里缺 {action}：\n{text}");
    }
    // 同一行形状（控制面那一条）：ts / token 摘要 / target / result / elapsed_ms。
    // 写面用的是**控制面**那一行的形状（`action=` / `target=`），与查询面的
    // `event=` / `q=` 行并存于同一个落点（一个进程一份审计）。
    let write_line = text
        .lines()
        .find(|l| l.contains("action=bundle.upload"))
        .expect("至少要有一行 bundle.upload");
    for needle in [
        "ts=",
        "action=bundle.upload",
        "token=sha256:",
        "target=project=",
        "result=OK",
        "elapsed_ms=",
    ] {
        assert!(write_line.contains(needle), "审计行形状缺 {needle:?}：{write_line}");
    }
    // 交付用的打码样例（审计行本身就是打码的：token 只有摘要）。
    for line in text.lines().filter(|l| l.contains("action=")).take(5) {
        eprintln!("· 审计：{line}");
    }
    // ⛔ 明文否定判据：key 明文、token 明文、DSN 明文、密文前缀一个都不许出现。
    assert!(!text.contains(&key), "审计里出现了 key 明文：\n{text}");
    assert!(!text.contains(ADMIN), "审计里出现了 admin token 明文");
    assert!(!text.contains(QUERY), "审计里出现了 query token 明文");
    assert!(!text.contains("postgresql://"), "审计里出现了连接串");
    assert!(!text.contains("v1:"), "审计里出现了密文（不该写这一列）");

    cleanup(&pool, &[project]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c8 · admin + ?project= 能切到另一个项目
// ══════════════════════════════════════════════════════════════

/// **AC-c8** · Studio 顶栏切项目：admin 带 `?project=B` 就必须返回 **B** 的数据
/// （两项目类数不同 ⇒ 数字会对不上）。
///
/// 同时钉住多项目模式下 admin **不带**参数时的语义：403 `key.admin_has_no_project`
/// —— admin 不绑项目，服务端⛔ 不替它猜一个（猜错就是串包）。
#[tokio::test]
async fn ac_c8_an_admin_can_point_the_read_endpoints_at_another_project() {
    let pool = common::test_pool().await;
    let a = unique("c8a");
    let b = unique("c8b");
    let va = make_project(&pool, &a, &["vm", "disk"]).await;
    activate(&pool, &a, va).await;
    // B 有 3 个类：与 A 的 2 个不同，于是「拿到的是谁的数据」是可判的。
    let vb = make_project(&pool, &b, &["vm", "disk", "vpc"]).await;
    activate(&pool, &b, vb).await;

    let dir = temp_dir("c8");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(Some(&a))).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens, None, &Vec::new());
    let port = srv.port;
    let admin = bearer(ADMIN);

    // ① admin + ?project=B ⇒ B（3 类）
    let (status, body) = http(
        port,
        "GET",
        &format!("/api/catalog?project={b}&page=1&page_size=50"),
        &admin,
        &[],
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let cat = json_of(&body);
    assert_eq!(cat["project"], b.as_str(), "必须返回 B 项目：{cat}");
    assert_eq!(cat["counts"]["classes"], 3, "B 有 3 个类：{cat}");

    // ② admin + ?project=A ⇒ A（2 类）：切回去也要对
    let (status, body) = http(
        port,
        "GET",
        &format!("/api/catalog?project={a}"),
        &admin,
        &[],
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let cat = json_of(&body);
    assert_eq!(cat["project"], a.as_str());
    assert_eq!(cat["counts"]["classes"], 2, "{cat}");

    // ③ TTL 也认这个参数（且 `?project=+view=model` 不因「未知参数」被 400）
    let (status, ttl) = http(
        port,
        "GET",
        &format!("/api/ontology.ttl?project={b}&view=model"),
        &admin,
        &[],
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&ttl));
    let text = String::from_utf8_lossy(&ttl);
    assert!(text.contains("vpc"), "B 的 TTL 里应当有 vpc：{}", &text[..text.len().min(600)]);

    // ④ 多项目模式 + admin **不带**参数 ⇒ 403（不猜项目）
    let (status, body) = http(port, "GET", "/api/catalog", &admin, &Vec::new());
    assert_eq!(status, 403, "{}", String::from_utf8_lossy(&body));
    assert_eq!(json_of(&body)["code"], "key.admin_has_no_project");

    // ⑤ 非法项目名 ⇒ 400（与 manifest.project 同一套正则）
    let (status, body) = http(port, "GET", "/api/catalog?project=Bad_Name", &admin, &Vec::new());
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    assert_eq!(json_of(&body)["code"], "BAD_REQUEST");

    let mut srv = srv;
    srv.stop();
    cleanup(&pool, &[a, b]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c9 · query key + ?project= ⇒ 403 key.project_pinned
// ══════════════════════════════════════════════════════════════

/// **AC-c9** · 数字员工**不许跳项目**（跳了就是串包）：带 `?project=` ⇒
/// **403 `key.project_pinned`**；**不给**这个参数 ⇒ 照常按 key 的项目读（200）。
///
/// 两条必须都在：只测 403 的话，「我们把参数一加、query key 全挂了」也能过。
#[tokio::test]
async fn ac_c9_a_query_key_cannot_repin_the_project() {
    let pool = common::test_pool().await;
    let mine = unique("c9a");
    let other = unique("c9b");
    let v = make_project(&pool, &mine, &["vm", "disk"]).await;
    activate(&pool, &mine, v).await;
    let vo = make_project(&pool, &other, &["vm", "disk", "vpc"]).await;
    activate(&pool, &other, vo).await;

    let dir = temp_dir("c9");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(Some(&mine))).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens, None, &Vec::new());
    let port = srv.port;

    // ① 带参数（哪怕是自己那个项目）⇒ 403：参数本身对 query key 就是**不存在的权限**。
    for target in [other.as_str(), mine.as_str()] {
        let (status, body) = http(
            port,
            "GET",
            &format!("/api/catalog?project={target}"),
            &bearer(QUERY),
            &[],
        );
        assert_eq!(status, 403, "?project={target}：{}", String::from_utf8_lossy(&body));
        assert_eq!(json_of(&body)["code"], "key.project_pinned", "{body:?}");
    }
    // ② 不带参数 ⇒ 正常（按 key 绑的项目读）
    let (status, body) = http(port, "GET", "/api/catalog", &bearer(QUERY), &Vec::new());
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let cat = json_of(&body);
    assert_eq!(cat["project"], mine.as_str(), "必须按 key 的项目读：{cat}");
    assert_eq!(cat["counts"]["classes"], 2, "{cat}");

    let mut srv = srv;
    srv.stop();
    cleanup(&pool, &[mine, other]);
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-c5 · 不给 ?project= 时，读路径与基线**逐字节相同**
// ══════════════════════════════════════════════════════════════

/// **AC-c5** · 同一个部署（单项目模式 / 本体服务模式 / 同一份 token 表），
/// **基线二进制**（`1c418ce`）与新二进制的响应 **status + body sha256** 全等。
///
/// 覆盖：`/api/health`（免 token）、`/api/catalog`、`/api/describe`、
/// `/api/ontology.ttl`（两视图）、`POST /api/sparql`（本体模式 ⇒ 503 的正文也在比对里）、
/// `POST /mcp`（JSON-RPC），外加 401 / 404 两条错误路径。
///
/// ⚠️ 基线二进制由 `ONTOK_BASELINE_BIN` 指定。没给 ⇒ **响亮跳过**（打印原因）：
/// 这条判据没有基线就没法判，⛔ 不假装通过。
#[tokio::test]
async fn ac_c5_without_the_project_parameter_the_read_path_is_byte_identical_to_the_baseline() {
    let Some(baseline) = std::env::var("ONTOK_BASELINE_BIN").ok().map(PathBuf::from) else {
        eprintln!(
            "⏭ 跳过 AC-c5：没有 ONTOK_BASELINE_BIN（基线 ontok 二进制）。\
             设成 `1c418ce` 编出来的 ontok.exe 之后重跑本用例。"
        );
        return;
    };
    assert!(baseline.is_file(), "ONTOK_BASELINE_BIN 不是文件：{baseline:?}");

    let pool = common::test_pool().await;
    let project = unique("c5");
    let v = make_project(&pool, &project, &["vm", "disk"]).await;
    activate(&pool, &project, v).await;

    let dir = temp_dir("c5");
    let tokens = dir.join("tokens.json");
    // **v1 格式**：老部署就是这一份。基线二进制不认识 `kind`（deny_unknown_fields）
    // ⇒ 这条对拍必须用**老表**，这也正是「缺 kind = query」的现场证明。
    std::fs::write(
        &tokens,
        r#"{"version":"1.0.0","tokens":[
            {"label":"c5-agent","token":"FAKE-REQ159C-C5-aaaaaaaaaaaaaaaa","max_rows":10}
        ]}"#,
    )
    .expect("写 token 表");

    let port_new = free_port();
    let port_old = free_port();
    let args = [
        "query",
        "serve",
        "--tokens",
        tokens.to_str().unwrap(),
        "--project",
        &project,
        "--version",
        "1",
        "--ontology-only",
    ];
    let new_srv = spawn_single(&ontok_bin(), port_new, &args, true);
    let old_srv = spawn_single(&baseline, port_old, &args, true);
    wait_ready(port_new);
    wait_ready(port_old);

    let token = "FAKE-REQ159C-C5-aaaaaaaaaaaaaaaa";
    let auth = bearer(token);
    let mcp = vec![
        ("Authorization".to_string(), format!("Bearer {token}")),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    let sparql_body = br#"{"query":"SELECT ?s WHERE { ?s a :vm }"}"#.to_vec();
    let mcp_init = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_vec();
    let mcp_call = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"catalog","arguments":{"page":1,"page_size":50}}}"#.to_vec();

    // (说明, method, path, headers, body) —— body 直接放进元组，避免「按名字猜 body」。
    let cases: Vec<(&str, &str, &str, Vec<(String, String)>, Vec<u8>)> = vec![
        ("health", "GET", "/api/health", vec![], vec![]),
        (
            "catalog",
            "GET",
            "/api/catalog?page=1&page_size=50",
            auth.clone(),
            vec![],
        ),
        ("describe", "GET", "/api/describe?name=vm", auth.clone(), vec![]),
        ("ttl-customer", "GET", "/api/ontology.ttl", auth.clone(), vec![]),
        (
            "ttl-model",
            "GET",
            "/api/ontology.ttl?view=model",
            auth.clone(),
            vec![],
        ),
        ("sparql", "POST", "/api/sparql", mcp.clone(), sparql_body),
        ("mcp-initialize", "POST", "/mcp", mcp.clone(), mcp_init),
        ("mcp-catalog", "POST", "/mcp", mcp.clone(), mcp_call),
        ("401-no-token", "GET", "/api/catalog", vec![], vec![]),
        ("404", "GET", "/api/nope", auth.clone(), vec![]),
    ];

    let mut compared = 0usize;
    for (what, method, path, headers, body) in cases {
        let (s_old, b_old) = http(port_old, method, path, &headers, &body);
        let (s_new, b_new) = http(port_new, method, path, &headers, &body);
        assert_eq!(s_old, s_new, "[{what}] 状态码变了：基线 {s_old} / 现在 {s_new}");
        let d_old = sha(&b_old);
        let d_new = sha(&b_new);
        assert_eq!(
            d_old, d_new,
            "[{what}] 响应体**不再逐字节相同**（AC-c5 / R2 破了）：\n  基线 {s_old} {d_old} ({} 字节)\n  现在 {s_new} {d_new} ({} 字节)\n  基线体：{}\n  现在体：{}",
            b_old.len(),
            b_new.len(),
            String::from_utf8_lossy(&b_old),
            String::from_utf8_lossy(&b_new),
        );
        compared += 1;
        eprintln!("· [{what}] {method} {path} ⇒ {s_new} · sha256:{d_new}");
    }
    assert_eq!(compared, 10, "十个请求都要真的比过");

    let mut new_srv = new_srv;
    let mut old_srv = old_srv;
    new_srv.stop();
    old_srv.stop();
    cleanup(&pool, &[project]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 起一个**单项目模式**的服务（`--project` + `--version` 由 `args` 给）。
fn spawn_single(bin: &Path, port: u16, args: &[&str], with_secret: bool) -> Server {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .args(["--bind", &format!("127.0.0.1:{port}")])
        .env("ONTOK_DATABASE_URL", db_url())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if with_secret {
        cmd.env("ONTOK_SECRET_KEY", secret_key());
    }
    let child = cmd.spawn().expect("起 ontok 子进程");
    Server { child, port }
}

