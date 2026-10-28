//! ⭐ REQ-174 · **多项目各带各的 AM** —— access mapping 随版本入库、按版本加载。
//!
//! # 这份文件治的是什么
//!
//! 打包端一直会写 `am/access-mapping.json`，但导入端**完全不读**它；运行期的 AM
//! 只是进程级 `--access-mapping` 文件 ⇒ 「导完没有 AM」「多项目只能共用一份」。
//! 本文件逐条钉住 demand §三 的 8 条判据：
//!
//! | # | 判据 | 层 |
//! | --- | --- | --- |
//! | **AC-001** | 带 AM 的包 ⇒ `version_access_mappings` 一行：`raw` **逐字节**等于包内文件、`sha256` 等于 manifest | 真库 |
//! | **AC-002** | 不带 AM 的包 ⇒ 无行、导入成功、`@pending` 不出现（与今天一致） | 真库 + 真进程 |
//! | **AC-003** | 两个项目装**两份不同 AM** ⇒ 各自的 `@pending.get.source.base_url` / `auth.service` 是自己那份 | **真进程 + 真库** |
//! | **AC-004** | 激活新版本（AM 不同）⇒ **下一个请求**就用新 AM（无陈旧缓存） | **真进程 + 真库** |
//! | **AC-005** | 单项目 + `--access-mapping 文件` ⇒ **文件优先**（哪怕库里也有一份不同的） | **真进程 + 真库** |
//! | **AC-006** | 包内 AM 不合法（`contract_version: 3` / 类不在本体里）⇒ 导入失败且**没有新版本** | 真库 |
//! | **AC-007** | v1 AM（4 键配方）照常加载 —— 且现在是**从 DB 读出来的** | 真进程 + 真库 |
//! | **AC-008** | 审计日志里不出现 AM 原文（断言不含 `credential_env` 的 canary 与 `base_url` 主机） | **真进程 + 真库** |
//!
//! # 红绿自证
//!
//! 把 `console::provision::write_new_version` 里写 AM 的那次 `access_store::upsert_in`
//! 注掉 ⇒ **AC-001 / AC-003 / AC-004 / AC-007 全红**（不是只红一条）：
//! - AC-001 查不到行；
//! - AC-003 / AC-004 / AC-007 的真进程查不到 DB 里的 AM ⇒ `@pending` 整段消失；
//! - AC-002 仍绿（它判的是「没有 AM 时不写行」）。
//!
//! # 环境
//!
//! | 变量 | 用途 | 缺了怎样 |
//! | --- | --- | --- |
//! | `ONTOK_TEST_DATABASE_URL`（或 `ONTOK_DATABASE_URL`） | 元数据库 | **响亮跳过** |
//! | `ONTOK_SECRET_KEY` | `query serve` 启动的写面密钥闸门 | 真进程那几条**响亮跳过** |
//!
//! ⚠️ 真进程那几条起**真二进制**（`CARGO_BIN_EXE_ontok`）：AM 怎么随版本加载是
//! 「进程怎么装配服务」的产物，进程内测不到那一层。

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ontok::db::project::PgProjectStore;
use ontok::domain::snapshot::{EntityType, Property, Snapshot};
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// admin key（写面 + 建项目）。
const ADMIN: &str = "FAKE-REQ174-ADMIN-aaaaaaaaaaaaaaaaaaaa";
/// 绑项目 A 的查询 key。
const QUERY_A: &str = "FAKE-REQ174-QUERY-A-bbbbbbbbbbbbbbbbbb";
/// 绑项目 B 的查询 key。
const QUERY_B: &str = "FAKE-REQ174-QUERY-B-cccccccccccccccccc";
/// 单项目模式的查询 key。
const QUERY_SINGLE: &str = "FAKE-REQ174-QUERY-S-dddddddddddddddddd";

/// 本体里的类 key（= AM `entities[].class`）。
const CLASS: &str = "vm";
/// 审计 canary：AM 里 `credential_env` 的一个值。
///
/// 它是**环境变量名**，不是凭据本身；但 AC-008 判的是「AM 原文没有出现在日志里」，
/// 所以只要审计行里搜得到这串，就说明 AM 原文被打印了。
const CANARY: &str = "REQ174_AUDIT_CANARY_ENV";

const NEED_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库";

// ══════════════════════════════════════════════════════════════
// 夹具：AM / 快照 / 真包
// ══════════════════════════════════════════════════════════════

/// 一份 v2 AM：一个源 + 一个 api 类。
///
/// `base_url` 与 `auth.service` 是**两个独立入参** —— AC-003 判的就是它们各自
/// 跟着版本走，而不是「从 host 猜出来的一个值」。
fn am_v2(base_url: &str, service: &str) -> String {
    format!(
        r#"{{
          "contract_version": 2,
          "mapping_version": 4,
          "sources": [{{
            "id": "src",
            "base_url": "{base_url}",
            "auth": {{
              "mode": "tc3-hmac-sha256",
              "service": "{service}",
              "credential_env": ["{CANARY}"],
              "signed_headers": ["content-type", "host", "x-tc-action"],
              "timestamp": {{ "header": "X-TC-Timestamp", "format": "unix-seconds" }},
              "scope": ["date", "service", "tc3_request"]
            }},
            "defaults": {{ "Region": "ap-guangzhou" }}
          }}],
          "entities": [{{
            "id": "Widget",
            "class": "{CLASS}",
            "source": "src",
            "capability": {{ "pushdown": ["ids"] }},
            "freshness": "realtime",
            "operation": {{
              "action": "DescribeWidgets",
              "version": "2020-01-01",
              "method": "POST",
              "content_type": "application/json"
            }},
            "request": {{
              "required": ["Region"],
              "template": {{ "Region": "{{{{Region}}}}" }},
              "bindings": {{ "Region": {{ "source": "default" }} }},
              "paging": {{ "offset": "Offset", "limit": "Limit", "value_type": "number" }}
            }},
            "iterator": "$.Response.Widgets[*]",
            "fields": {{ "name": {{ "path": "WidgetName" }} }}
          }}]
        }}"#
    )
}

/// 一份 **v1** AM（不写 `contract_version`，只有旧 4 键的形状）。
fn am_v1(base_url: &str) -> String {
    format!(
        r#"{{
          "mapping_version": 4,
          "sources": [{{ "id": "src", "base_url": "{base_url}" }}],
          "entities": [{{ "id": "Widget", "class": "{CLASS}", "source": "src" }}]
        }}"#
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

/// 用**生产打包器**造一个真包（与 `ontok-builder` 同一个 `assemble`）。
fn bundle_for(project: &str, classes: &[&str], am: Option<&str>) -> Vec<u8> {
    let mut input = ontok::bundle::pack::PackInput::new(
        project,
        1,
        "2026-10-28T00:00:00Z",
        snapshot_for(Uuid::nil(), 1, classes),
    );
    if let Some(text) = am {
        input.am = vec![("access-mapping.json".to_string(), text.as_bytes().to_vec())];
    }
    ontok::bundle::pack::assemble(&input).expect("打包")
}

// ══════════════════════════════════════════════════════════════
// 库 / 目录 / 进程（判据只用标准库）
// ══════════════════════════════════════════════════════════════

fn db_url() -> String {
    ontok::config::test_database_url().expect(NEED_DB)
}

fn secret_key() -> String {
    "ab".repeat(32)
}

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req174-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn unique(tag: &str) -> String {
    format!(
        "req174-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// 起库 + 迁移 + 建一个空项目（**不建版本**：版本由 import 建）。
async fn project_pool(tag: &str) -> Option<(PgPool, String)> {
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", tag);
        return None;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let name = unique(tag);
    PgProjectStore::new(pool.clone())
        .get_or_create(&name)
        .await
        .expect("建项目");
    Some((pool, name))
}

async fn cleanup(pool: &PgPool, names: &[String]) {
    for n in names {
        // `ontology_versions` / 快照五表 / `version_access_mappings` 都是 CASCADE。
        let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(n)
            .execute(pool)
            .await;
    }
}

async fn version_count(pool: &PgPool, project: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM ontology_versions v JOIN projects p ON p.id = v.project_id
         WHERE p.name = $1",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .expect("数版本");
    row.0
}

/// 直接把一个包导入（不经 HTTP）—— AC-001 / AC-002 / AC-006 用。
async fn import_direct(
    pool: &PgPool,
    project: &str,
    am: Option<&str>,
) -> Result<ontok::console::provision::ImportOutcome, ontok::console::OpError> {
    let bytes = bundle_for(project, &[CLASS], am);
    let up = ontok::console::provision::upload_bundle(pool, project, bytes)
        .await
        .expect("上传");
    ontok::console::provision::import_bundle(pool, project, &up.bundle_id).await
}

// ─────────────────── 真进程 ───────────────────

struct Server {
    child: Child,
    port: u16,
    stderr: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("要一个空闲端口");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

/// 单项目模式的 token 表（`version: "1.0.0"`：`project` 由启动参数钉死）。
fn tokens_single() -> String {
    format!(r#"{{"version":"1.0.0","tokens":[{{"label":"agent","token":"{QUERY_SINGLE}"}}]}}"#)
}

/// 多项目模式的 token 表：admin + 两个**各绑一个项目**的查询 key。
fn tokens_multi(a: &str, b: &str) -> String {
    format!(
        r#"{{"version":2,"tokens":[
            {{"label":"fde","token":"{ADMIN}","kind":"admin"}},
            {{"label":"agent-a","token":"{QUERY_A}","kind":"query","project":"{a}"}},
            {{"label":"agent-b","token":"{QUERY_B}","kind":"query","project":"{b}"}}
        ]}}"#
    )
}

fn spawn_multi(dir: &Path, tokens: &Path, audit: Option<&Path>) -> Server {
    let port = free_port();
    let stderr_path = dir.join("multi.stderr.log");
    let mut cmd = Command::new(ontok_bin());
    cmd.args([
        "query",
        "serve",
        "--bind",
        &format!("127.0.0.1:{port}"),
        "--tokens",
        tokens.to_str().unwrap(),
        "--ontology-only",
    ])
    .env("ONTOK_DATABASE_URL", db_url())
    .env("ONTOK_SECRET_KEY", secret_key())
    .stdout(Stdio::null())
    .stderr(Stdio::from(
        std::fs::File::create(&stderr_path).expect("建 stderr 日志"),
    ));
    if let Some(a) = audit {
        cmd.arg("--audit-out").arg(a);
    }
    let srv = Server {
        child: cmd.spawn().expect("起 ontok 子进程"),
        port,
        stderr: stderr_path,
    };
    wait_ready(&srv);
    srv
}

/// 单项目模式的真进程；`am_file = None` ⇒ **只能用库里那份**（AC-007 的关键）。
fn spawn_single(
    dir: &Path,
    project: &str,
    version: i32,
    am_file: Option<&Path>,
    audit: Option<&Path>,
) -> Server {
    let port = free_port();
    let stderr_path = dir.join("single.stderr.log");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_single()).expect("写 token 表");
    let mut cmd = Command::new(ontok_bin());
    cmd.args([
        "query",
        "serve",
        "--bind",
        &format!("127.0.0.1:{port}"),
        "--tokens",
        tokens.to_str().unwrap(),
        "--ontology-only",
        "--project",
        project,
        "--version",
        &version.to_string(),
    ])
    .env("ONTOK_DATABASE_URL", db_url())
    .env("ONTOK_SECRET_KEY", secret_key())
    .stdout(Stdio::null())
    .stderr(Stdio::from(
        std::fs::File::create(&stderr_path).expect("建 stderr 日志"),
    ));
    if let Some(am) = am_file {
        cmd.arg("--access-mapping").arg(am);
    }
    if let Some(a) = audit {
        cmd.arg("--audit-out").arg(a);
    }
    let srv = Server {
        child: cmd.spawn().expect("起 ontok 子进程"),
        port,
        stderr: stderr_path,
    };
    wait_ready(&srv);
    srv
}

fn wait_ready(srv: &Server) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if let Ok((200, _)) = std::panic::catch_unwind(|| {
            http(srv.port, "GET", "/api/health", &Vec::new(), &[])
        }) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "服务在 30 秒内没有就绪（端口 {}）。stderr：\n{}",
        srv.port,
        srv.stderr_text()
    );
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
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
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

// ─────────────────── 经 HTTP 的建包流程 ───────────────────

/// `上传 → 解析 → 激活`，返回 `(bundle_id, version)`。
fn deploy_bundle(srv: &Server, project: &str, am: &str) -> (String, i64) {
    let bundle = bundle_for(project, &[CLASS], Some(am));
    let mut h = bearer(ADMIN);
    h.push(("Content-Type".to_string(), "application/octet-stream".to_string()));
    let (status, body) = http(
        srv.port,
        "POST",
        &format!("/api/projects/{project}/bundles"),
        &h,
        &bundle,
    );
    assert_eq!(
        status, 201,
        "上传应当 201（{project}）：{}",
        String::from_utf8_lossy(&body)
    );
    let bundle_id = json_of(&body)["bundle_id"]
        .as_str()
        .expect("回执要有 bundle_id")
        .to_string();

    let (status, body) = http(
        srv.port,
        "POST",
        &format!("/api/projects/{project}/bundles/{bundle_id}/import"),
        &bearer(ADMIN),
        &[],
    );
    assert_eq!(
        status, 200,
        "解析应当 200（{project}）：{}",
        String::from_utf8_lossy(&body)
    );

    let (status, body) = http(
        srv.port,
        "POST",
        &format!("/api/projects/{project}/bundles/{bundle_id}/activate"),
        &bearer(ADMIN),
        &[],
    );
    assert_eq!(
        status, 200,
        "激活应当 200（{project}）：{}",
        String::from_utf8_lossy(&body)
    );
    let version = json_of(&body)["version"].as_i64().expect("activate 回执带 version");
    (bundle_id, version)
}

/// 按类点名打一条 JSON-LD 查询，拿回 `@pending[0].get`。
fn pending_get(srv: &Server, token: &str) -> Value {
    let body = json!({ "query": format!("SELECT ?c WHERE {{ ?c a :{CLASS} }}") }).to_string();
    let headers = vec![
        ("Authorization".to_string(), format!("Bearer {token}")),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    let (status, raw) = http(
        srv.port,
        "POST",
        "/api/sparql?format=jsonld",
        &headers,
        body.as_bytes(),
    );
    let text = String::from_utf8_lossy(&raw).to_string();
    assert_eq!(
        status, 200,
        "jsonld 查询应当 200。stderr：\n{}\nbody：{text}",
        srv.stderr_text()
    );
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("不是 JSON（{e}）：{text}"));
    let pending = v["@pending"]
        .as_array()
        .unwrap_or_else(|| panic!("回执里没有 @pending（AM 没加载？）：{v}"));
    assert_eq!(pending.len(), 1, "只点名了一个类：{v}");
    pending[0]["get"].clone()
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 导入带 AM 的包 ⇒ 一行，raw 逐字节 + sha256 与 manifest 一致
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_import_persists_am_bytes_and_sha256_exactly() {
    let Some((pool, project)) = project_pool("ac001").await else { return };
    let am = am_v2("https://a.example.com", "svc-a");
    let out = import_direct(&pool, &project, Some(&am)).await.expect("导入应当成功");
    assert_eq!(out.version, 1, "第一版：{out:?}");

    let (raw, sha, sources, entities): (String, String, i32, i32) = sqlx::query_as(
        "SELECT raw, sha256, sources, entities FROM version_access_mappings WHERE version_id = $1",
    )
    .bind(out.version_id)
    .fetch_one(&pool)
    .await
    .expect("version_access_mappings 必须有这一版的行（AC-001）");

    // ① 逐字节（不是「解析后再序列化」）
    assert_eq!(
        raw.as_bytes(),
        am.as_bytes(),
        "raw 必须与包内 am/access-mapping.json **逐字节相同**"
    );
    // ② sha256 == 包内文件字节的 sha256 == manifest 的 components.am.sha256
    let manifest_am = ontok::provision::bundle::verify_and_read(
        bundle_for(&project, &[CLASS], Some(&am)),
        &project,
    )
    .expect("整包校验")
    .manifest
    .am
    .expect("manifest 应当声明 components.am");
    assert_eq!(
        sha, manifest_am.sha256,
        "落库的 sha256 必须与 manifest 的 components.am.sha256 一致"
    );
    assert_eq!(
        sha,
        ontok::query::audit::sha256_hex(am.as_bytes()),
        "sha256 必须按**原样字节**算"
    );
    // ③ 计数（与 manifest 同口径）
    assert_eq!((sources, entities), (1, 1), "sources / entities 计数不对");
    assert_eq!(
        manifest_am.counts.get("sources").copied(),
        Some(sources as i64),
        "与 manifest 的 counts 同口径"
    );
    assert_eq!(
        manifest_am.counts.get("entities").copied(),
        Some(entities as i64),
        "与 manifest 的 counts 同口径"
    );

    eprintln!("· AC-001 · version={} · raw={} B（逐字节相同）· sha256={}", out.version, raw.len(), &sha[..12]);
    eprintln!("· AC-001 · sources={sources} · entities={entities} · manifest 一致");
    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 导入不带 AM 的包 ⇒ 无行、导入成功、@pending 不出现
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac002_import_without_am_writes_no_row_and_pending_stays_absent() {
    let Some((pool, project)) = project_pool("ac002").await else { return };
    let out = import_direct(&pool, &project, None).await.expect("不带 AM 的包导入应当成功");
    assert_eq!(out.version, 1, "{out:?}");

    let row: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM version_access_mappings WHERE version_id = $1",
    )
    .bind(out.version_id)
    .fetch_one(&pool)
    .await
    .expect("数 AM 行");
    assert_eq!(row.0, 0, "包里没有 AM ⇒ **不许**写行（AC-002）");

    // 冻结这一版（单项目模式只服务已冻结版本），再起真进程：**行为与今天一致**。
    //
    // ⚠️ 「与今天一致」在 `--ontology-only` 下的**具体形状**是 503 `NO_DATA_SOURCE`：
    // 没有 AM ⇒ `vm` 就是普通**库源**类 ⇒ 本体服务模式没有源库可查。
    // 关键判据不是状态码，而是**没有 `@pending`**：AM 缺席时绝不会凭空长出一条
    // 「按 API 去取」的指引（那才是「导入丢了 AM」本会造成的错误形态）。
    ontok::db::project::PgProjectStore::new(pool.clone())
        .activate(
            PgProjectStore::new(pool.clone())
                .get_by_name(&project)
                .await
                .expect("读项目")
                .expect("项目在")
                .id,
            out.version_id,
        )
        .await
        .expect("冻结 + 激活");

    let dir = temp_dir("ac002");
    let srv = spawn_single(&dir, &project, out.version, None, None);
    let body = json!({ "query": format!("SELECT ?c WHERE {{ ?c a :{CLASS} }}") }).to_string();
    let headers = vec![
        ("Authorization".to_string(), format!("Bearer {QUERY_SINGLE}")),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    let (status, raw) = http(
        srv.port,
        "POST",
        "/api/sparql?format=jsonld",
        &headers,
        body.as_bytes(),
    );
    let text = String::from_utf8_lossy(&raw).to_string();
    assert_eq!(
        status, 503,
        "没有 AM ⇒ 按普通库源查询；本体服务模式没有源库 ⇒ 503 NO_DATA_SOURCE（与今天一致）：{text}"
    );
    assert_eq!(
        json_of(&raw)["code"],
        json!("NO_DATA_SOURCE"),
        "错误码应当是 NO_DATA_SOURCE（不是别的新形态）：{text}"
    );
    assert!(
        !text.contains("@pending"),
        "没有 AM ⇒ 回执里不该出现 `@pending`：{text}"
    );

    eprintln!("· AC-002 · 无 AM ⇒ 无行 · import 成功 · @pending 不出现（与今天一致）");
    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 两个项目装两份不同 AM（真进程 + 真库）—— 最关键的一条
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac003_two_projects_each_serve_their_own_am() {
    let test = "ac003_two_projects_each_serve_their_own_am";
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", test);
        return;
    }
    if std::env::var("ONTOK_SECRET_KEY").map(|v| v.trim().is_empty()).unwrap_or(true) {
        common::skip::missing("ONTOK_SECRET_KEY", test);
        return;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let a = unique("ac003a");
    let b = unique("ac003b");
    for name in [&a, &b] {
        PgProjectStore::new(pool.clone())
            .get_or_create(name)
            .await
            .expect("建项目");
    }

    let dir = temp_dir("ac003");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(&a, &b)).expect("写 token 表");
    let audit = dir.join("audit.jsonl");
    let srv = spawn_multi(&dir, &tokens, Some(&audit));

    // 两份 AM：**base_url 与 service 都不同**（任一被串了都看得出来）
    let am_a = am_v2("https://a.example.com", "svc-a-174");
    let am_b = am_v2("https://b.example.com", "svc-b-174");
    let (_, va) = deploy_bundle(&srv, &a, &am_a);
    let (_, vb) = deploy_bundle(&srv, &b, &am_b);
    eprintln!("· AC-003 · A={a} (v{va}) · B={b} (v{vb})");

    // 各自用自己的 key 查同一个类 —— 但拿到的是**自己那份** AM
    let get_a = pending_get(&srv, QUERY_A);
    let get_b = pending_get(&srv, QUERY_B);
    eprintln!(
        "· AC-003 · A 的 @pending.get = {}",
        serde_json::to_string(&get_a).expect("序列化")
    );
    eprintln!(
        "· AC-003 · B 的 @pending.get = {}",
        serde_json::to_string(&get_b).expect("序列化")
    );

    assert_eq!(
        get_a["source"]["base_url"], json!("https://a.example.com"),
        "项目 A 的 base_url 必须是**自己那份**：{get_a}"
    );
    assert_eq!(get_a["auth"]["service"], json!("svc-a-174"), "{get_a}");
    assert_eq!(
        get_b["source"]["base_url"], json!("https://b.example.com"),
        "项目 B 的 base_url 必须是**自己那份**：{get_b}"
    );
    assert_eq!(get_b["auth"]["service"], json!("svc-b-174"), "{get_b}");
    // 反向证据：任何一份里都不许出现对方的端点
    let text_a = get_a.to_string();
    let text_b = get_b.to_string();
    assert!(!text_a.contains("b.example.com"), "A 拿到了 B 的 AM：{text_a}");
    assert!(!text_b.contains("a.example.com"), "B 拿到了 A 的 AM：{text_b}");

    // ── AC-008 · 审计里不出现 AM 原文 / 凭据声明 ──
    let audit_text = std::fs::read_to_string(&audit).expect("审计文件应当存在");
    for forbidden in [CANARY, "a.example.com", "b.example.com", "svc-a-174", "svc-b-174"] {
        assert!(
            !audit_text.contains(forbidden),
            "审计日志里出现了 AM 内容 {forbidden:?}（AC-008）：\n{audit_text}"
        );
    }
    assert!(audit_text.contains("bundle.import"), "审计应当记了 import（否则判据是空的）：{audit_text}");
    eprintln!(
        "· AC-008 · 审计 {} 行 · 不含 AM 原文 / canary / base_url / service",
        audit_text.lines().count()
    );

    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
    cleanup(&pool, &[a, b]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 激活新版本（AM 不同）⇒ 下一个请求就用新 AM
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac004_activating_a_new_version_switches_the_am_on_the_next_request() {
    let test = "ac004_activating_a_new_version_switches_the_am_on_the_next_request";
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", test);
        return;
    }
    if std::env::var("ONTOK_SECRET_KEY").map(|v| v.trim().is_empty()).unwrap_or(true) {
        common::skip::missing("ONTOK_SECRET_KEY", test);
        return;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let project = unique("ac004");
    PgProjectStore::new(pool.clone())
        .get_or_create(&project)
        .await
        .expect("建项目");

    let dir = temp_dir("ac004");
    let tokens = dir.join("tokens.json");
    // 两个查询 key 都绑同一个项目也没关系；这里只用一个。
    std::fs::write(&tokens, tokens_multi(&project, &project)).expect("写 token 表");
    let srv = spawn_multi(&dir, &tokens, None);

    let (_, v1) = deploy_bundle(&srv, &project, &am_v2("https://old.example.com", "svc-old-174"));
    let before = pending_get(&srv, QUERY_A);
    assert_eq!(before["source"]["base_url"], json!("https://old.example.com"), "{before}");
    assert_eq!(before["auth"]["service"], json!("svc-old-174"), "{before}");

    // 同一个进程、同一个 key：激活第 2 版之后**下一个请求**必须换 AM
    let (_, v2) = deploy_bundle(&srv, &project, &am_v2("https://new.example.com", "svc-new-174"));
    assert_eq!(v2, v1 + 1, "第二次激活应当是新版本：v1={v1} v2={v2}");
    let after = pending_get(&srv, QUERY_A);
    assert_eq!(
        after["source"]["base_url"], json!("https://new.example.com"),
        "换版本后的**下一个请求**必须用新 AM（缓存按 version_id，不该有陈旧缓存）：{after}"
    );
    assert_eq!(after["auth"]["service"], json!("svc-new-174"), "{after}");

    eprintln!("· AC-004 · v{v1} ⇒ {} / {}", before["source"]["base_url"], before["auth"]["service"]);
    eprintln!("· AC-004 · v{v2} ⇒ {} / {}", after["source"]["base_url"], after["auth"]["service"]);

    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 单项目 + `--access-mapping 文件` ⇒ 文件优先
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac005_single_project_prefers_the_cli_file_over_the_db_row() {
    let test = "ac005_single_project_prefers_the_cli_file_over_the_db_row";
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", test);
        return;
    }
    if std::env::var("ONTOK_SECRET_KEY").map(|v| v.trim().is_empty()).unwrap_or(true) {
        common::skip::missing("ONTOK_SECRET_KEY", test);
        return;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let project = unique("ac005");
    let p = PgProjectStore::new(pool.clone())
        .get_or_create(&project)
        .await
        .expect("建项目");

    // 库里先放一份 **不同** 的 AM（base_url = db.example.com）
    let out = import_direct(&pool, &project, Some(&am_v2("https://db.example.com", "svc-db-174")))
        .await
        .expect("导入");
    PgProjectStore::new(pool.clone())
        .activate(p.id, out.version_id)
        .await
        .expect("激活");
    let stored: (String,) = sqlx::query_as("SELECT raw FROM version_access_mappings WHERE version_id = $1")
        .bind(out.version_id)
        .fetch_one(&pool)
        .await
        .expect("库里的 AM 行");
    assert!(stored.0.contains("db.example.com"), "前置：库里确实有一份 AM");

    // 命令行再给一份不同的（base_url = file.example.com）
    let dir = temp_dir("ac005");
    let am_file = dir.join("access-mapping.json");
    std::fs::write(&am_file, am_v2("https://file.example.com", "svc-file-174")).expect("写文件 AM");

    let srv = spawn_single(&dir, &project, out.version, Some(&am_file), None);
    let get = pending_get(&srv, QUERY_SINGLE);
    assert_eq!(
        get["source"]["base_url"], json!("https://file.example.com"),
        "单项目模式：`--access-mapping 文件` **优先于** DB（AC-005）：{get}"
    );
    assert_eq!(get["auth"]["service"], json!("svc-file-174"), "{get}");
    assert!(
        !get.to_string().contains("db.example.com"),
        "文件优先时不该混进库里那份：{get}"
    );

    eprintln!("· AC-005 · DB=db.example.com · 文件=file.example.com ⇒ 生效 file.example.com");
    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 包内 AM 不合法 ⇒ 导入失败且**没有新版本**
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac006_invalid_am_fails_the_import_without_creating_a_version() {
    let Some((pool, project)) = project_pool("ac006").await else { return };

    // ① `contract_version: 3` —— 未来版本，解析层就该拒
    let bad_contract = am_v2("https://bad.example.com", "svc-bad").replace(
        "\"contract_version\": 2",
        "\"contract_version\": 3",
    );
    // ② 类不在这一版本体里 —— `bind` 失败
    let bad_class = am_v2("https://bad.example.com", "svc-bad").replace(
        &format!("\"class\": \"{CLASS}\""),
        "\"class\": \"no_such_class\"",
    );
    for (label, bad) in [("contract_version=3", &bad_contract), ("class 不在本体里", &bad_class)] {
        let before = version_count(&pool, &project).await;
        let err = import_direct(&pool, &project, Some(bad))
            .await
            .expect_err(&format!("{label} 的 AM 必须让导入失败"));
        assert_eq!(err.code, "am.invalid", "{label}：错误码应当是 am.invalid（{}）", err.message);
        assert!(
            err.message.contains("包里的 AM"),
            "{label}：消息要点出是包里的 AM：{}",
            err.message
        );
        assert!(
            !err.message.contains("bad.example.com"),
            "{label}：错误消息里**不许**回显 AM 内容（AC-008）：{}",
            err.message
        );
        let after = version_count(&pool, &project).await;
        assert_eq!(after, before, "{label}：导入失败后**不许**留下新版本（回滚）");
        eprintln!("· AC-006 · {label} ⇒ {} · versions {before}→{after}（没有新版本）", err.code);
    }

    cleanup(&pool, &[project]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-007 · v1 AM 照常加载（4 键配方）—— 且现在是从 DB 读的
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac007_v1_am_loaded_from_the_database_keeps_the_four_legacy_keys() {
    let test = "ac007_v1_am_loaded_from_the_database_keeps_the_four_legacy_keys";
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", test);
        return;
    }
    if std::env::var("ONTOK_SECRET_KEY").map(|v| v.trim().is_empty()).unwrap_or(true) {
        common::skip::missing("ONTOK_SECRET_KEY", test);
        return;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let project = unique("ac007");
    let p = PgProjectStore::new(pool.clone())
        .get_or_create(&project)
        .await
        .expect("建项目");

    let out = import_direct(&pool, &project, Some(&am_v1("https://v1.example.com")))
        .await
        .expect("v1 AM 的包应当导入成功");
    PgProjectStore::new(pool.clone())
        .activate(p.id, out.version_id)
        .await
        .expect("激活");

    // 不给 `--access-mapping`：AM **只能**来自 DB（这就是 REQ-174 的接线）
    let dir = temp_dir("ac007");
    let srv = spawn_single(&dir, &project, out.version, None, None);
    let get = pending_get(&srv, QUERY_SINGLE);
    let mut keys: Vec<&String> = get.as_object().expect("get 是对象").keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["class", "filter", "pushdown", "ref"],
        "v1 AM ⇒ `@pending.get` 仍然只有旧 4 键（AC-007 / AC-103 不回退）：{get}"
    );
    assert!(get.get("source").is_none(), "v1 不该有 source：{get}");
    eprintln!("· AC-007 · v1 AM（DB）⇒ @pending.get = {get}");

    drop(srv);
    let _ = std::fs::remove_dir_all(&dir);
    cleanup(&pool, &[project]).await;
}
