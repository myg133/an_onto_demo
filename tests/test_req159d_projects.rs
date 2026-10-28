//! ⭐ REQ-159 **P1c-2** · `GET /api/projects` 与「切项目看图」的真库 / 真进程判据。
//!
//! 背景：Studio（`/studio`，挂在 `query serve` 上）的顶栏要一个项目下拉，而
//! 服务端原先**没有**「列项目」这条读端点（只有 `POST /api/projects` 建项目）。
//! 本文件验的就是新补的那一条，以及它和 `?project=` 的配合。
//!
//! | 判据 | 测的是什么 |
//! | --- | --- |
//! | **AC-d1** | admin ⇒ 200 且形状是 `{projects:[{name,label,active_version,frozen,entity_types,link_types}]}`；`kind=query` ⇒ **403 `key.not_admin`**；⛔ 回执里没有 DSN / key / 密文 |
//! | **AC-d1b** | **没有生效版本**的项目不报错：`active_version = null`、计数 0、`frozen=false`（⛔ 不是 503、不是 500） |
//! | **AC-d2** | 两个类数不同的项目：`?project=A` / `?project=B` 的 catalog 计数不同，且与列表里的 `entity_types` 一一对上（Studio 切项目后图会跟着变） |
//! | **AC-d4** | 上传是 `application/octet-stream` + **原始字节**（用生产打包器造的真包）；`multipart/form-data` 被点名拒绝（415 `bundle.upload.invalid`） |
//! | 附带 | `POST /api/projects` 语义不变（仍 201、仍发一次性 key） |
//!
//! # 环境
//!
//! `ONTOK_TEST_DATABASE_URL`（元数据库）+ `ONTOK_SECRET_KEY`（64 位十六进制）。
//! 都起**真进程**（`ontok query serve`）：这条端点的鉴权发生在身份门里（进程怎么起），
//! 进程内是测不到的。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ontok::domain::snapshot::{EntityType, Property, Snapshot};
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use uuid::Uuid;

/// admin key（写面 + 列表）。长度 ≥ 24（`MIN_TOKEN_LEN`）。
const ADMIN: &str = "FAKE-REQ159D-ADMIN-aaaaaaaaaaaaaaaaaaaa";
/// 数字员工的查询 key（**不该**能列项目）。
const QUERY: &str = "FAKE-REQ159D-QUERY-bbbbbbbbbbbbbbbbbbbb";

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req159d-{tag}-{}-{}",
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

/// 用**生产打包器**（`bundle::pack::assemble`，与 `ontok-builder` 同一个函数）造真包。
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
fn tokens_multi(query_project: &str) -> String {
    format!(
        r#"{{"version":2,"tokens":[
            {{"label":"fde","token":"{ADMIN}","kind":"admin"}},
            {{"label":"agent","token":"{QUERY}","kind":"query","project":"{query_project}"}}
        ]}}"#
    )
}

fn spawn_multi(bin: &Path, tokens: &Path) -> Server {
    let port = free_port();
    let child = Command::new(bin)
        .args([
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
        .stderr(Stdio::null())
        .spawn()
        .expect("起 ontok 子进程");
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

fn unique(tag: &str) -> String {
    format!(
        "req159d-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

/// 回执里的一行（按项目名找）。
fn entry_of<'a>(list: &'a Value, name: &str) -> Option<&'a Value> {
    list["projects"]
        .as_array()?
        .iter()
        .find(|p| p["name"].as_str() == Some(name))
}

// ══════════════════════════════════════════════════════════════
// AC-d1 · admin 200 + 形状；query 403 key.not_admin；⛔ 不泄密
// ══════════════════════════════════════════════════════════════

/// **AC-d1** · 真进程 + 真库：列表的形状、鉴权、以及「回执里没有秘密」。
///
/// 变异（红）：把 `GET /api/projects` 从 `is_write_path` 的前缀里摘出去 ⇒ query key
/// 拿到 200 ⇒ 「query ⇒ 403」那条断言红；给回执加一个 `key`/`dsn` 字段 ⇒ 逐键断言红。
#[tokio::test]
async fn ac_d1_list_projects_is_admin_only_and_carries_no_secret() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let with_version = unique("d1a");
    let without_version = unique("d1b");
    let vid = make_project(&pool, &with_version, &["vm", "disk", "vpc"]).await;
    activate(&pool, &with_version, vid).await;
    // 只建项目、**不激活**：`active_version_id IS NULL` 的那一支
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    projects
        .get_or_create(&without_version)
        .await
        .expect("建一个没有版本的項目");

    let dir = temp_dir("d1");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(&with_version)).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens);
    let port = srv.port;

    // ① admin ⇒ 200
    let (status, body) = http(port, "GET", "/api/projects", &bearer(ADMIN), &[]);
    assert_eq!(
        status,
        200,
        "admin 列项目应当 200：{}",
        String::from_utf8_lossy(&body)
    );
    let list = json_of(&body);
    let arr = list["projects"].as_array().expect("回执必须有 projects 数组");
    assert!(!arr.is_empty(), "共享库里至少有一个项目（本用例刚建了两个）");

    // ② 形状：**每一行**都恰好 6 个键（不只是我造的那两个）
    for p in arr {
        let mut keys: Vec<&str> = p.as_object().expect("每行是对象").keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "active_version",
                "entity_types",
                "frozen",
                "label",
                "link_types",
                "name"
            ],
            "AC-d1 的形状变了：{p}"
        );
    }
    // ③ 有生效版本的那一行：seq / frozen / 计数都对
    let a = entry_of(&list, &with_version).expect("列表里应当有刚建的项目");
    assert_eq!(a["active_version"], json!(1), "生效版本 seq：{a}");
    assert_eq!(a["frozen"], json!(true), "激活即冻结：{a}");
    assert_eq!(a["entity_types"], json!(3), "对象类型取生效版本快照：{a}");
    assert_eq!(a["link_types"], json!(0), "关系类型取生效版本快照：{a}");
    assert!(a["label"].is_null(), "没写备注 ⇒ null：{a}");

    // ④ 没有生效版本的那一行：**null / 0 / false**，⛔ 不是 500、不是 503
    let b = entry_of(&list, &without_version).expect("没有生效版本的项目也必须在列表里");
    assert!(b["active_version"].is_null(), "没有生效版本 ⇒ active_version = null：{b}");
    assert_eq!(b["frozen"], json!(false), "{b}");
    assert_eq!(b["entity_types"], json!(0), "{b}");
    assert_eq!(b["link_types"], json!(0), "{b}");

    // ⑤ 回执里**没有**任何秘密（DSN / key / 密文 / 摘要）
    let text = String::from_utf8_lossy(&body).to_ascii_lowercase();
    for forbidden in ["dsn", "ciphertext", "token", "sha256", "secret", "ontok_q_"] {
        assert!(
            !text.contains(forbidden),
            "列表回执里出现了 {forbidden:?}（⛔ 不许暴露 DSN / key / 密文）：{text}"
        );
    }

    // ⑥ query key ⇒ **403 `key.not_admin`**（不是 401：这张 key 是真的，只是不是写面的钥匙）
    let (status, body) = http(port, "GET", "/api/projects", &bearer(QUERY), &[]);
    assert_eq!(
        status,
        403,
        "query key 列项目应当 403：{}",
        String::from_utf8_lossy(&body)
    );
    let err = json_of(&body);
    assert_eq!(err["code"], "key.not_admin", "{err}");
    assert!(!err["hint"].as_str().unwrap_or("").is_empty(), "403 要给人话：{err}");

    // ⑦ 没有 token ⇒ 401（这条端点不给匿名探路）
    let (status, _) = http(port, "GET", "/api/projects", &[], &[]);
    assert_eq!(status, 401, "无 token 应当 401");

    // ⑧ `POST /api/projects` 的语义**没被改动**：仍 201、仍发一次性 key
    let created = unique("d1c");
    let payload = json!({"project": created, "label": "P1c-2 建项目", "key_label": "数字员工：探针"});
    let (status, body) = http(
        port,
        "POST",
        "/api/projects",
        &with_json(bearer(ADMIN)),
        payload.to_string().as_bytes(),
    );
    assert_eq!(
        status,
        201,
        "POST /api/projects 应当仍是 201：{}",
        String::from_utf8_lossy(&body)
    );
    let made = json_of(&body);
    assert_eq!(made["project"], created.as_str());
    assert!(made["key"].as_str().is_some_and(|k| !k.is_empty()), "仍要发一次性 key：{made}");

    cleanup(&pool, &[with_version, without_version, created]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-d2 · 切项目 ⇒ 图的类数跟着变（与列表里的计数一致）
// ══════════════════════════════════════════════════════════════

/// **AC-d2** · 两个**类数不同**的项目都激活：`?project=A` / `?project=B` 的 catalog
/// 计数必须不同，且与 `GET /api/projects` 里的 `entity_types` 一一对上 —— 这正是
/// Studio 切项目后「图重绘成另一套」的服务端依据（AC-d2 的 DOM 那一半在
/// `tests/fixtures/req159d/studio_probe.js`：节点数 2 → 5）。
///
/// 变异（红）：让 `?project=` 被忽略（永远返回钉死的那个项目）⇒ 两次计数相同 ⇒ 红。
#[tokio::test]
async fn ac_d2_switching_projects_changes_the_class_count_the_graph_would_draw() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let big = unique("d2big");
    let small = unique("d2small");
    let v_big = make_project(&pool, &big, &["vm", "disk", "vpc", "subnet", "sg"]).await;
    activate(&pool, &big, v_big).await;
    let v_small = make_project(&pool, &small, &["vm"]).await;
    activate(&pool, &small, v_small).await;

    let dir = temp_dir("d2");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(&big)).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens);
    let port = srv.port;

    // ① 列表里的两个计数
    let (status, body) = http(port, "GET", "/api/projects", &bearer(ADMIN), &[]);
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let list = json_of(&body);
    let a = entry_of(&list, &big).expect("big 在列表里").clone();
    let b = entry_of(&list, &small).expect("small 在列表里").clone();
    assert_eq!(a["entity_types"], json!(5), "{a}");
    assert_eq!(b["entity_types"], json!(1), "{b}");

    // ② 带 ?project= 的 catalog 计数与列表一致
    for (name, want) in [(&big, 5i64), (&small, 1i64)] {
        let path = format!("/api/catalog?page=1&page_size=200&project={name}");
        let (status, body) = http(port, "GET", &path, &bearer(ADMIN), &[]);
        assert_eq!(
            status,
            200,
            "{path} 应当 200：{}",
            String::from_utf8_lossy(&body)
        );
        let cat = json_of(&body);
        assert_eq!(
            cat["counts"]["classes"], json!(want),
            "{path} 的类数不对：{cat}"
        );
        assert_eq!(cat["project"], json!(name), "回执要说是哪个项目：{cat}");
    }

    // ③ 两次的类数**确实不同**（切项目不是假的）
    let (_, body_a) = http(port, "GET", &format!("/api/catalog?page=1&page_size=200&project={big}"), &bearer(ADMIN), &[]);
    let (_, body_b) = http(port, "GET", &format!("/api/catalog?page=1&page_size=200&project={small}"), &bearer(ADMIN), &[]);
    let na = json_of(&body_a)["counts"]["classes"].as_i64().unwrap_or(-1);
    let nb = json_of(&body_b)["counts"]["classes"].as_i64().unwrap_or(-2);
    assert_ne!(na, nb, "切项目后类数必须变（这正是图重绘的依据）：{na} vs {nb}");

    // ④ 数字员工**不许**跳项目（一 key 一项目）—— 否则 Studio 的切换就破了隔离
    let (status, body) = http(
        port,
        "GET",
        &format!("/api/catalog?page=1&page_size=50&project={small}"),
        &bearer(QUERY),
        &[],
    );
    assert_eq!(
        status,
        403,
        "query key 带 ?project= 应当 403：{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(json_of(&body)["code"], "key.project_pinned", "{body:?}");

    cleanup(&pool, &[big, small]).await;
}

// ══════════════════════════════════════════════════════════════
// AC-d4 · 上传是 octet-stream + 原始字节（真包），⛔ 不是 multipart
// ══════════════════════════════════════════════════════════════

/// **AC-d4** · 用**生产打包器**造的真包，按 Studio 的发法（octet-stream + 原始字节）
/// 上传；再用 `multipart/form-data` 发一次，必须被**点名拒绝**（415 `bundle.upload.invalid`）
/// —— 那证明这条路径收的就是裸字节，而不是「碰巧 multipart 也能过」。
#[tokio::test]
async fn ac_d4_upload_takes_raw_octet_stream_bytes_and_rejects_multipart() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let project = unique("d4");
    let dir = temp_dir("d4");
    let tokens = dir.join("tokens.json");
    std::fs::write(&tokens, tokens_multi(&project)).expect("写 token 表");
    let srv = spawn_multi(&ontok_bin(), &tokens);
    let port = srv.port;

    // 建项目（Studio 的 ＋ 那一步）
    let payload = json!({"project": project, "label": "P1c-2 上传", "key_label": "数字员工：运维"});
    let (status, body) = http(
        port,
        "POST",
        "/api/projects",
        &with_json(bearer(ADMIN)),
        payload.to_string().as_bytes(),
    );
    assert_eq!(status, 201, "{}", String::from_utf8_lossy(&body));

    // 真包（生产打包器）
    let bundle = bundle_for(&project, &["vm", "disk"]);
    let up_path = format!("/api/projects/{project}/bundles");

    // ① octet-stream + 原始字节 ⇒ 201
    let mut h = bearer(ADMIN);
    h.push(("Content-Type".to_string(), "application/octet-stream".to_string()));
    let (status, body) = http(port, "POST", &up_path, &h, &bundle);
    assert_eq!(
        status,
        201,
        "原始字节上传应当 201：{}",
        String::from_utf8_lossy(&body)
    );
    let up = json_of(&body);
    assert_eq!(up["bytes"], json!(bundle.len() as i64), "回执的字节数：{up}");
    assert_eq!(up["sha256_verified"], json!(true), "{up}");
    assert_eq!(up["requires_db"], json!(false), "这个包不含 db：{up}");
    let bundle_id = up["bundle_id"].as_str().expect("回执要有 bundle_id").to_string();

    // ② 包状态可查（Studio 会用它确认状态）
    let (status, body) = http(
        port,
        "GET",
        &format!("/api/projects/{project}/bundles/{bundle_id}"),
        &bearer(ADMIN),
        &[],
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(json_of(&body)["state"], "uploaded", "{body:?}");

    // ③ multipart ⇒ 415 + 点名（⛔ 不是「神秘 zip 错误」）
    let mut h = bearer(ADMIN);
    h.push((
        "Content-Type".to_string(),
        "multipart/form-data; boundary=----probe".to_string(),
    ));
    let (status, body) = http(port, "POST", &up_path, &h, &bundle);
    assert_eq!(
        status,
        415,
        "multipart 应当 415：{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(json_of(&body)["code"], "bundle.upload.invalid", "{body:?}");

    cleanup(&pool, &[project]).await;
}
