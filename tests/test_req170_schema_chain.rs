//! ⭐ REQ-170 · **构建链带上「库结构」**的真进程判据。
//!
//! | # | 判据 | 它说什么 |
//! | --- | --- | --- |
//! | R1 | [`dsn_and_schema_together_exit_2_and_name_both`] | 两个库结构入口**互斥** ⇒ 退出码 2、点名两个参数、⛔ 不建工作目录 |
//! | R2 | [`doc_ingest_with_a_schema_sends_the_digest_and_lands_maps_to`] | `doc ingest --schema` ⇒ 关系断言里有 `maps_to`，两端表/列都落到了真实表上；屏幕上报「带了几张 / 共几张」 |
//! | R2 | [`an_unknown_column_in_maps_to_is_dropped_and_counted`] | 列不存在 ⇒ **整条关系**丢弃 + 分类计数（⛔ 不编列名） |
//! | R2 | [`changing_the_schema_invalidates_the_prompt_cache`] | 摘要进 `prompt_sha` ⇒ 换 schema 必须重新调用（与换 skill 同一条纪律） |
//! | R2 | [`without_schema_the_products_carry_no_maps_to`] | 不给 `--schema` ⇒ 产物里**没有** `maps_to`（零回归的机器判据；逐字节比对见 changelog 的真进程证据） |
//! | R4 | [`build_run_with_schema_merges_the_relation_into_the_draft`] | `build run --schema`（真进程）⇒ 草稿里的关系带 `maps_to` 与实现列 |
//!
//! # 为什么是**真进程**
//!
//! 这些都是**进程边界**上的事实：退出码 2 由 `main` 决定、`--schema` 是 clap 收的、
//! 指纹缓存落在 `<out>.cache.json`、草稿是 `build run` 第 ③ 段写的。
//! 单元判据只能说「函数被调用过」，说不了这些。
//!
//! ⚠️ 本文件**不连库**：schema 用 `tests/fixtures/req170/schema.json`（`ontok-introspect`
//! 的形状），agent 用 `tests/fixtures/req147/fake-acp.js` 的 `maps` / `maps-bad` 模式
//!（真 ACP 子进程、假模型）。真库那条证据是交付报告里的一次性实测，不是这里的恒常判据。

mod common;

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

/// crate 根：走运行时锚点（⛔ 不用编译期常量 —— REQ-046 的事故形态）。
fn crate_root() -> PathBuf {
    common::source_root::crate_root()
}

fn fixture(rel: &str) -> PathBuf {
    crate_root().join("tests/fixtures/req170").join(rel)
}

fn fake_acp() -> PathBuf {
    crate_root().join("tests/fixtures/req147/fake-acp.js")
}

fn skill_fixture() -> PathBuf {
    crate_root().join("tests/fixtures/req147/skill-fixture.md")
}

fn node_ok() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req170-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 三个「旧台」环境变量：本单的链不该碰它们。
const DB_ENVS: [&str; 3] = [
    "ONTOK_DATABASE_URL",
    "ONTOK_TEST_DATABASE_URL",
    "ONTOK_SECRET_KEY",
];

fn clean_env(cmd: &mut Command) -> &mut Command {
    for k in DB_ENVS {
        cmd.env_remove(k);
    }
    cmd
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", path.display()))
}

// ══════════════════════════════════════════════════════════════
// R1 · 两个入口互斥（不需要 node：入参闸在拉子进程之前）
// ══════════════════════════════════════════════════════════════

#[test]
fn dsn_and_schema_together_exit_2_and_name_both() {
    let root = temp_dir("exclusive");
    let work = root.join("never-built");
    let mut cmd = Command::new(bin());
    cmd.args([
        "build",
        "run",
        "--input",
        fixture("docs").to_str().unwrap(),
        "--work-dir",
        work.to_str().unwrap(),
        "--agent",
        "acp",
        "--agent-command",
        "ontok-agent-that-does-not-exist-req170",
        "--agent-skill",
        skill_fixture().to_str().unwrap(),
        "--bind",
        "127.0.0.1:0",
        "--no-browser",
        "--schema",
        fixture("schema.json").to_str().unwrap(),
        "--dsn",
        "postgresql://readonly:pw@127.0.0.1:5432/src",
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 build run");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert_eq!(r.status.code(), Some(2), "互斥是**入参错**（2）：\n{err}");
    for needle in ["--dsn", "--schema", "互斥"] {
        assert!(err.contains(needle), "要点名 {needle:?}：\n{err}");
    }
    assert!(
        !work.exists(),
        "⛔ 入参闸不该建工作目录（否则就是先动了盘再报错）"
    );
    assert!(
        !err.contains("第 0/4 段"),
        "⛔ 互斥的闸必须在勘查之前：\n{err}"
    );
    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// R2 · `doc ingest --schema`（真 ACP 子进程）
// ══════════════════════════════════════════════════════════════

/// 跑一次 `doc ingest`，返回 `(stdout, stderr)`。
fn doc_ingest(out: &Path, schema: Option<&Path>, cache: &Path, mode: &str) -> (String, String) {
    let mut cmd = Command::new(bin());
    cmd.args([
        "doc",
        "ingest",
        "--input",
        fixture("docs").to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        out.with_extension("stats.json").to_str().unwrap(),
        "--cache",
        cache.to_str().unwrap(),
        "--agent",
        "acp",
        "--agent-command",
        &format!("node {}", fake_acp().display()),
        "--agent-skill",
        skill_fixture().to_str().unwrap(),
        "--agent-timeout-secs",
        "60",
    ]);
    if let Some(s) = schema {
        cmd.args(["--schema", s.to_str().unwrap()]);
    }
    clean_env(&mut cmd);
    cmd.env("FAKE_ACP_MODE", mode);
    let r = cmd.output().expect("跑不了 doc ingest");
    let stdout = String::from_utf8_lossy(&r.stdout).to_string();
    let stderr = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(
        r.status.success(),
        "doc ingest 应当成功（mode={mode}）：\n{stdout}\n{stderr}"
    );
    (stdout, stderr)
}

/// 断言 JSONL 里那条**关系**（本夹具只有一条）。
fn the_relation(out: &Path) -> Value {
    let text = std::fs::read_to_string(out).expect("读不到断言 JSONL");
    text.lines()
        .map(|l| serde_json::from_str::<Value>(l).expect("JSONL 每行都该是合法 JSON"))
        .find(|v| v["kind"] == "relation")
        .unwrap_or_else(|| panic!("断言里没有关系那一行：\n{text}"))
}

#[test]
fn doc_ingest_with_a_schema_sends_the_digest_and_lands_maps_to() {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（REQ-170 真进程）：本机没有 node 或假 ACP 夹具 —— 这条判据**未验**。");
        return;
    }
    let root = temp_dir("ingest-maps");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, Some(&fixture("schema.json")), &cache, "maps");

    let rel = the_relation(&out);
    let maps = &rel["maps_to"];
    assert_eq!(
        maps["child"]["table"].as_str(),
        Some("cloud_disks"),
        "child 端要落到「云硬盘」这张表上（概念名 ↔ 中文名）：{rel}"
    );
    assert_eq!(maps["child"]["column"].as_str(), Some("server_id"));
    assert_eq!(
        maps["parent"]["table"].as_str(),
        Some("servers"),
        "parent 端要落到「云主机」：{rel}"
    );
    assert_eq!(maps["parent"]["column"].as_str(), Some("id"));

    // 屏幕：带了几张 / 共几张
    assert!(err.contains("库结构"), "屏幕上要报库结构这一行：\n{err}");
    assert!(
        err.contains("/ 共 3 张表") || err.contains("/ 共 2 张表"),
        "要报「带了多少 / 共多少」：\n{err}"
    );
    assert!(
        err.contains("maps_to") && err.contains("落地 1 条"),
        "要报 maps_to 的落地/丢弃：\n{err}"
    );

    // stats.json：`maps_to` 这一节在，且 kept=1
    let stats = read_json(&out.with_extension("stats.json"));
    assert_eq!(stats["maps_to"]["kept"], 1, "{stats}");
    assert_eq!(stats["maps_to"]["dropped"], 0, "{stats}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn an_unknown_column_in_maps_to_is_dropped_and_counted() {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（REQ-170 真进程）：本机没有 node 或假 ACP 夹具 —— 这条判据**未验**。");
        return;
    }
    let root = temp_dir("ingest-bad");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, Some(&fixture("schema.json")), &cache, "maps-bad");

    let text = std::fs::read_to_string(&out).expect("读不到断言 JSONL");
    assert!(
        !text.contains("\"kind\":\"relation\""),
        "⛔ 列不存在的**整条关系**都不许留下：\n{text}"
    );
    let stats = read_json(&out.with_extension("stats.json"));
    assert_eq!(stats["maps_to"]["dropped"], 1, "{stats}");
    assert_eq!(stats["maps_to"]["dropped_column_absent"], 1, "{stats}");
    assert_eq!(stats["maps_to"]["kept"], 0, "{stats}");
    let samples = stats["maps_to"]["samples"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        samples
            .iter()
            .any(|s| s.as_str().unwrap_or("").contains("no_such_column")),
        "丢弃要点名到列：{samples:?}"
    );
    // 屏幕上也要看得见（⛔ 只丢不报等于静默）
    assert!(
        err.contains("查不到列 1"),
        "屏幕要报分类计数：\n{err}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn changing_the_schema_invalidates_the_prompt_cache() {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（REQ-170 真进程）：本机没有 node 或假 ACP 夹具 —— 这条判据**未验**。");
        return;
    }
    let root = temp_dir("prompt-sha");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let stats_of = |p: &Path| read_json(&p.with_extension("stats.json"));

    // ① 首跑：缓存未命中，调用 1 次
    doc_ingest(&out, Some(&fixture("schema.json")), &cache, "maps");
    let first = stats_of(&out);
    assert_eq!(first["calls"], 1, "{first}");
    let sha_a = first["prompt_sha"].as_str().unwrap_or_default().to_string();

    // ② 同一份 schema 再跑：**0 次调用**（缓存命中 ⇒ 摘要确实进了指纹，且是稳定的）
    doc_ingest(&out, Some(&fixture("schema.json")), &cache, "maps");
    let second = stats_of(&out);
    assert_eq!(second["calls"], 0, "同 schema 必须命中缓存：{second}");
    assert_eq!(second["cache_hits"], 1, "{second}");
    assert_eq!(second["prompt_sha"], sha_a, "同 schema ⇒ 同 prompt_sha");

    // ③ 换一份 schema（多一张「云硬盘快照」表 ⇒ 摘要变）⇒ 必须重新调用
    doc_ingest(&out, Some(&fixture("schema-wider.json")), &cache, "maps");
    let third = stats_of(&out);
    assert_ne!(
        third["prompt_sha"], sha_a,
        "⛔ 换 schema 必须换 prompt_sha（否则会回放旧表列下的回答）：{third}"
    );
    assert_eq!(
        third["calls"], 1,
        "⛔ 换 schema 之后必须重新调用模型：{third}"
    );
    assert_eq!(third["cache_hits"], 0, "{third}");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn without_schema_the_products_carry_no_maps_to() {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（REQ-170 真进程）：本机没有 node 或假 ACP 夹具 —— 这条判据**未验**。");
        return;
    }
    let root = temp_dir("no-schema");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, None, &cache, "maps");

    let text = std::fs::read_to_string(&out).expect("读不到断言 JSONL");
    assert!(
        !text.contains("maps_to"),
        "⛔ 没给 --schema ⇒ 产物里不许出现 maps_to：\n{text}"
    );
    assert!(
        !text.contains("server_id"),
        "⛔ 没给 --schema ⇒ 连列名都不该冒出来：\n{text}"
    );
    let stats = read_json(&out.with_extension("stats.json"));
    assert!(
        stats.get("maps_to").is_none(),
        "⛔ 没给 --schema ⇒ stats.json 里不该有 maps_to 那一节（零回归）：{stats}"
    );
    // 屏幕：不该出现库结构那两行
    assert!(!err.contains("库结构"), "⛔ 没给 schema 就不该报库结构：\n{err}");
    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// R4 · `build run --schema` 真进程：草稿里的关系带 maps_to
// ══════════════════════════════════════════════════════════════

#[test]
fn build_run_with_schema_merges_the_relation_into_the_draft() {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（REQ-170 真进程）：本机没有 node 或假 ACP 夹具 —— 这条判据**未验**。");
        return;
    }
    let root = temp_dir("build-schema");
    let work = root.join("wd");
    let mut cmd = Command::new(bin());
    cmd.args([
        "build",
        "run",
        "--input",
        fixture("docs").to_str().unwrap(),
        "--work-dir",
        work.to_str().unwrap(),
        "--agent",
        "acp",
        "--agent-command",
        &format!("node {}", fake_acp().display()),
        "--agent-skill",
        skill_fixture().to_str().unwrap(),
        "--agent-timeout-secs",
        "60",
        "--bind",
        "127.0.0.1:0",
        "--no-browser",
        "--proposal",
        fixture("proposal.json").to_str().unwrap(),
        "--schema",
        fixture("schema.json").to_str().unwrap(),
    ]);
    clean_env(&mut cmd);
    cmd.env("FAKE_ACP_MODE", "maps");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("起不了 ontok-builder build run");

    // stderr 逐行读：看到第 ④ 段（评审台就绪）就把进程收掉 —— ④ 是前台常驻。
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if tx.send(line.unwrap_or_default()).is_err() {
                break;
            }
        }
    });
    // stdout 也要排空（管道写满会把子进程卡住）
    let stdout = child.stdout.take().expect("stdout");
    std::thread::spawn(move || {
        let mut buf = String::new();
        let mut r = stdout;
        let _ = r.read_to_string(&mut buf);
    });

    let mut seen: Vec<String> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let mut reached_desk = false;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_secs(180)) {
            Ok(line) => {
                let hit = line.contains("第 4/4 段");
                seen.push(line);
                if hit {
                    reached_desk = true;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    // ④ 起来了就收掉（草稿在第 ③ 段已经落盘）
    let _ = child.kill();
    let _ = child.wait();
    let log = seen.join("\n");
    assert!(reached_desk, "没走到第 ④ 段（起评审台）—— 说明前面的段失败了：\n{log}");

    let draft = read_json(&work.join("out/proposal.json"));
    let items = draft["items"].as_array().expect("草稿要有 items");
    let link = items
        .iter()
        .find(|it| it["kind"] == "add_link_type")
        .unwrap_or_else(|| panic!("草稿里没有关系条目：{draft}"));
    let doc_ev = &link["payload"]["doc_evidence"][0];
    assert_eq!(
        doc_ev["maps_to"]["child"]["table"].as_str(),
        Some("cloud_disks"),
        "草稿的证据里要留着这条关系是靠哪两列并进来的：{link}"
    );
    assert_eq!(
        doc_ev["maps_to"]["parent"]["table"].as_str(),
        Some("servers"),
        "{link}"
    );
    // 实现列也要落到 link 上（r2rml 的 join 证据从这里读）
    assert_eq!(
        link["payload"]["evidence"][0]["column"].as_str(),
        Some("server_id"),
        "{link}"
    );

    // 屏幕上：三段都跑过，且第 ① 段报了库结构
    for needle in ["第 1/4 段", "第 2/4 段", "库结构", "maps_to"] {
        assert!(log.contains(needle), "屏幕输出里缺 {needle:?}：\n{log}");
    }
    std::fs::remove_dir_all(&root).ok();
}
