//! ⭐ REQ-176 · **发布闸的部署级业务词白名单**（`--term-allowlist <file>`）。
//!
//! # 判据（能红能绿）
//!
//! | # | 输入 | 期望 | 怎么变红 |
//! | --- | --- | --- | --- |
//! | **AC-001** | 不给 flag，客户视图含 `evidence` | **拒绝**（exit 2，点名 `evidence`） | 默认放行 ⇒ 红 |
//! | **AC-002** | 白名单含 `evidence` | **放行**；stderr 点名 `evidence` **与出口 C6** | 只放行不留痕 ⇒ 红 |
//! | **AC-003** | 白名单含 `evidence`，视图还有 `主键` | **仍拒**；错误消息**只列 `主键`** | 把整份视图放行 / 仍列 evidence ⇒ 红 |
//! | **AC-004** | 文件不存在 / 某行有空白 | **启动失败**，消息点名文件与行号 | 当作空表继续跑 ⇒ 红 |
//! | **AC-005** | 同一份白名单给 `ontology export` 与 `query serve` | 判定一致、**同一份产物**（字节相同） | 两边各读各的 ⇒ 红 |
//! | **AC-006** | 白名单里有词表外的词 | 启动**警告「这行没意义」**，**不失败** | 直接失败 ⇒ 红 |
//!
//! # 环境
//!
//! `ONTOK_TEST_DATABASE_URL` 必需（库名需含 test/scratch/tmp/ci/dev）；
//! `ONTOK_SECRET_KEY` 只在起 `query serve` 真进程时需要。
//!
//! # 红绿自证
//!
//! 把 `src/domain/pipeline.rs::gate_text` 里的
//! `crate::term_allowlist::filter_hits(...)` 换回 `scan_modeling_terms(text)` ⇒
//! AC-002/005 红（白名单形同虚设），AC-001/003 仍绿。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::{EntityType, Property, Snapshot};

/// 客户视图里那个「业务词 = 建模术语」的实体显示名（demand 的触发场景）。
const EVIDENCE: &str = "evidence";
/// 同一份视图里真正的建模术语（AC-003：不许被白名单顺带放行）。
const PRIMARY_KEY: &str = "主键";

/// 单项目模式的查询 key（≥ 24 字符）。
const QUERY_KEY: &str = "FAKE-REQ176-QUERY-aaaaaaaaaaaaaaaaaaaa";

const EXIT_LEAKED: i32 = ontok::cli::EXIT_LEAKED_MODELING_TERMS;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn ontok_builder() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok-builder"))
}

fn ontok() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn db_url() -> String {
    ontok::config::test_database_url().expect(NEED_TEST_DB)
}

fn secret_key() -> String {
    "ab".repeat(32)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req176-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 建「项目 + 第 1 版 + 快照 + 冻结」。
///
/// `entity_display` 是**实体的人读名**（`evidence` 走这里）；
/// `with_primary_key` 为真时再塞一个属性 `主键`（AC-003 的第二个命中词）。
async fn frozen_view(prefix: &str, entity_display: &str, with_primary_key: bool) -> (String, i32) {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = common::fresh_project(&ps, prefix).await;
    let v = ps.new_version(p.id).await.expect("建版本失败");

    let mut snap = Snapshot::new(v.id, v.seq);
    // 实体 key 刻意**不叫 evidence**：客户视图按 display_name 投影，
    // 若 key 也叫 evidence，`usable_as_human_name` 会判「退化成了 key」而整条丢弃。
    snap.entity_types.push(EntityType {
        key: "alpha".into(),
        display_name: entity_display.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: if with_primary_key {
            vec![Property {
                name: PRIMARY_KEY.into(),
                column: "pk_col".into(),
                nullable: false,
                note: None,
                sample: None,
            }]
        } else {
            vec![]
        },
        evidence: vec![],
    });
    store.save_snapshot(&snap).await.expect("写快照失败");
    ps.freeze(v.id).await.expect("冻结失败");
    (p.name, v.seq)
}

async fn cleanup(project: &str) {
    if let Ok(pool) = ontok::db::pool::connect(&db_url()).await {
        let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(project)
            .execute(&pool)
            .await;
    }
}

/// `ontok-builder ontology export --out <file>` —— **真进程**。
fn run_export(project: &str, version: i32, out: &Path, allowlist: Option<&Path>) -> std::process::Output {
    let mut cmd = Command::new(ontok_builder());
    cmd.args(["ontology", "export", "--project", project, "--version", &version.to_string()]);
    cmd.args(["--out", out.to_str().expect("临时路径必须是 UTF-8")]);
    cmd.env("ONTOK_DATABASE_URL", db_url());
    cmd.env("ONTOK_SECRET_KEY", secret_key());
    if let Some(p) = allowlist {
        cmd.arg("--term-allowlist").arg(p);
    }
    cmd.output().expect("跑 ontok-builder ontology export 失败")
}

fn write_allowlist(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).expect("写白名单文件");
    p
}

/// 取 stderr 里**发布闸命中**那一行（AC-003 只许列剩下的词）。
fn leak_line(stderr: &str) -> &str {
    stderr
        .lines()
        .find(|l| l.contains("泄露了建模术语"))
        .unwrap_or_else(|| panic!("stderr 里没有发布闸命中行：\n{stderr}"))
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 不给 flag ⇒ 行为不变（含 evidence 仍被拒）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_without_the_flag_an_evidence_view_is_still_refused() {
    let dir = temp_dir("ac001");
    let file = dir.join("customer.ttl");
    let (project, version) = frozen_view("req176-ac001", EVIDENCE, false).await;

    let out = run_export(&project, version, &file, None);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!(
        "AC-001（不给 flag）：exit={:?} · stdout={} 字节\nstderr: {stderr}",
        out.status.code(),
        out.stdout.len()
    );

    assert_eq!(
        out.status.code(),
        Some(EXIT_LEAKED),
        "不给 --term-allowlist 时必须与基线一致（拒绝出图）；stderr={stderr}"
    );
    assert!(out.stdout.is_empty(), "拒绝时 stdout 不许有字节");
    assert!(stderr.contains(EVIDENCE), "错误消息必须点名 evidence：{stderr}");
    assert!(stderr.contains("C6"), "错误消息必须点名出口：{stderr}");
    assert!(!file.exists(), "闸在写之前：被拒绝的发布不许产出文件");

    cleanup(&project).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-002 / AC-005 · 白名单含 evidence ⇒ 放行 + **留痕**；与 serve 同一份产物
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac002_with_the_allowlist_the_same_view_is_published_and_the_exemption_is_named() {
    let dir = temp_dir("ac002");
    let file = dir.join("customer.ttl");
    let allow = write_allowlist(&dir, "terms.txt", "# 审计/取证域的业务词\nevidence\n");
    let (project, version) = frozen_view("req176-ac002", EVIDENCE, false).await;

    let out = run_export(&project, version, &file, Some(&allow));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!(
        "AC-002（白名单含 evidence）：exit={:?} · 产物 {} 字节\nstderr: {stderr}",
        out.status.code(),
        std::fs::read(&file).map(|b| b.len()).unwrap_or(0)
    );

    assert_eq!(
        out.status.code(),
        Some(0),
        "白名单含 evidence 时同一份视图必须放行；stderr={stderr}"
    );
    // ① 启动回执：加载了几个词、分别是哪些。
    assert!(stderr.contains("加载 1 个词"), "启动必须回执加载数：{stderr}");
    assert!(stderr.contains("evidence"), "启动回执必须列出词条：{stderr}");
    // ② 每次豁免一行：出口 + 被豁免的词（⛔ 不许静默放行）。
    let exemption = stderr
        .lines()
        .find(|l| l.contains("发布闸豁免"))
        .unwrap_or_else(|| panic!("stderr 里没有豁免留痕行：\n{stderr}"));
    assert!(exemption.contains("C6"), "豁免行必须点名出口 C6：{exemption}");
    assert!(exemption.contains("evidence"), "豁免行必须点名被豁免的词：{exemption}");
    // ③ 产物里确实有那个业务词（放行的是它，不是别的）。
    let ttl = std::fs::read_to_string(&file).expect("读导出产物");
    assert!(ttl.contains(EVIDENCE), "放行后产物里应当有 evidence：\n{ttl}");

    // ── AC-005 的上半：query serve 用同一份白名单启动，TTL 与 CLI 逐字节相同 ──
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        format!(r#"{{"version":"1.0.0","tokens":[{{"label":"agent","token":"{QUERY_KEY}"}}]}}"#),
    )
    .expect("写 tokens");
    let srv = Server::spawn(&dir, &project, version, Some(&allow), &tokens);
    let stderr_srv = srv.wait_ready();
    eprintln!("AC-005（query serve 启动日志）：\n{stderr_srv}");

    assert!(
        stderr_srv.contains("发布闸豁免"),
        "serve 装配两视图 TTL 时也必须留痕豁免：\n{stderr_srv}"
    );
    let exemption_srv = stderr_srv
        .lines()
        .find(|l| l.contains("发布闸豁免"))
        .expect("serve 的豁免行");
    assert!(exemption_srv.contains("C6"), "serve 的豁免行要点名出口：{exemption_srv}");
    assert!(
        exemption_srv.contains("evidence"),
        "serve 的豁免行要点名词条：{exemption_srv}"
    );

    let (status, body) = http_get(srv.port, "/api/ontology.ttl", QUERY_KEY);
    eprintln!("AC-005：GET /api/ontology.ttl ⇒ {status} · {} 字节", body.len());
    assert_eq!(status, 200, "同一条白名单下 serve 应当能发出 TTL");
    assert_eq!(
        body,
        std::fs::read(&file).expect("读 CLI 产物"),
        "AC-005：`ontology export` 与 `query serve` 用同一份白名单 ⇒ 同一份产物（逐字节）"
    );

    srv.stop();
    cleanup(&project).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 白名单只豁免 evidence；主键仍然拒，且错误消息**只列主键**
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac003_a_real_modeling_term_is_still_refused_and_only_the_remaining_terms_are_listed() {
    let dir = temp_dir("ac003");
    let file = dir.join("customer.ttl");
    let allow = write_allowlist(&dir, "terms.txt", "evidence\n");
    let (project, version) = frozen_view("req176-ac003", EVIDENCE, true).await;

    let out = run_export(&project, version, &file, Some(&allow));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let line = leak_line(&stderr);
    eprintln!(
        "AC-003（白名单 evidence + 视图含 主键）：exit={:?}\n命中行: {line}\n完整 stderr: {stderr}",
        out.status.code()
    );

    assert_eq!(
        out.status.code(),
        Some(EXIT_LEAKED),
        "白名单只豁免 evidence：主键必须仍然拒绝；stderr={stderr}"
    );
    assert!(line.contains(PRIMARY_KEY), "错误消息必须列剩下的主键：{line}");
    assert!(
        !line.contains(EVIDENCE),
        "错误消息不许再列已被豁免的 evidence（「只列剩下的词」）：{line}"
    );
    assert!(!file.exists(), "拒绝时不许产出产物");

    cleanup(&project).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 文件不存在 / 某行非法 ⇒ 启动失败，点名文件与行号
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac004_a_missing_or_invalid_allowlist_fails_before_anything_is_written() {
    let dir = temp_dir("ac004");
    let file = dir.join("customer.ttl");
    let (project, version) = frozen_view("req176-ac004", EVIDENCE, false).await;

    // ① 文件不存在。
    let missing = dir.join("does-not-exist.txt");
    let out = run_export(&project, version, &file, Some(&missing));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("AC-004（文件不存在）：exit={:?}\nstderr: {stderr}", out.status.code());
    assert_ne!(out.status.code(), Some(0), "白名单文件不存在必须启动失败");
    assert!(
        stderr.contains(&missing.display().to_string()),
        "错误消息必须点名文件路径：{stderr}"
    );
    assert!(!file.exists(), "启动失败时不许写出任何产物");

    // ② 某行有空白（词条中间）。
    let bad = write_allowlist(&dir, "bad.txt", "evidence\nevi dence\n");
    let out = run_export(&project, version, &file, Some(&bad));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("AC-004（非法行）：exit={:?}\nstderr: {stderr}", out.status.code());
    assert_ne!(out.status.code(), Some(0), "非法行必须启动失败（⛔ 不许当空表继续跑）");
    assert!(stderr.contains("bad.txt"), "错误消息必须点名文件：{stderr}");
    assert!(stderr.contains("第 2 行"), "错误消息必须点名行号：{stderr}");
    assert!(!file.exists(), "启动失败时不许写出任何产物");

    cleanup(&project).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 词表外的词 ⇒ 警告「这行没意义」，但不失败
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac006_a_term_outside_the_vocabulary_only_warns() {
    let dir = temp_dir("ac006");
    let file = dir.join("customer.ttl");
    let allow = write_allowlist(&dir, "terms.txt", "evidence\ninvoice_no\n");
    let (project, version) = frozen_view("req176-ac006", EVIDENCE, false).await;

    let out = run_export(&project, version, &file, Some(&allow));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("AC-006（词表外的词）：exit={:?}\nstderr: {stderr}", out.status.code());

    assert_eq!(out.status.code(), Some(0), "词表外的词不许让启动失败；stderr={stderr}");
    assert!(stderr.contains("invoice_no"), "警告要点名那个词：{stderr}");
    assert!(stderr.contains("这行没意义"), "AC-006 的原文：{stderr}");
    assert!(file.exists(), "警告不改变判定：同一份视图仍被放行");

    cleanup(&project).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// 真进程 + 裸 HTTP（只用标准库）
// ══════════════════════════════════════════════════════════════

struct Server {
    child: Child,
    port: u16,
    stderr: PathBuf,
}

impl Server {
    fn spawn(
        dir: &Path,
        project: &str,
        version: i32,
        allowlist: Option<&Path>,
        tokens: &Path,
    ) -> Self {
        let port = free_port();
        let stderr = dir.join(format!("serve-{port}.stderr.log"));
        let mut cmd = Command::new(ontok());
        cmd.args([
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
        ));
        if let Some(p) = allowlist {
            cmd.arg("--term-allowlist").arg(p);
        }
        let child = cmd.spawn().expect("起 ontok query serve 子进程");
        Self { child, port, stderr }
    }

    fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }

    /// 等到横幅出现（或进程提前退出）。返回 stderr 全文。
    fn wait_ready(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let text = self.stderr_text();
            if text.contains("就绪") {
                return text;
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

/// 裸 HTTP/1.1 GET（`Authorization: Bearer` + `Connection: close`）。
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
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("0").trim(), 16)
            .unwrap_or(0);
        i += nl + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[i..(i + size).min(raw.len())]);
        i += size + 2;
    }
    out
}
