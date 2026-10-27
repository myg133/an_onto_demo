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
//! | **F-1** | [`a_read_only_role_still_sees_primary_keys_so_r2rml_maps_the_tables`] | **只读角色（只有 SELECT）**跑 `build run --dsn` ⇒ `schema.json` 里每张源表 `primary_key` **非空**、摘要**不许**说「没有主键」、`r2rml generate` 出 **>0** TriplesMap |
//!
//! # 为什么是**真进程**
//!
//! 这些都是**进程边界**上的事实：退出码 2 由 `main` 决定、`--schema` 是 clap 收的、
//! 指纹缓存落在 `<out>.cache.json`、草稿是 `build run` 第 ③ 段写的。
//! 单元判据只能说「函数被调用过」，说不了这些。
//!
//! ⚠️ 只有 **F-1** 那条连真库（判据本身就是「只读账号下的可见性」，模拟不了）；
//! 其余判据不连库：schema 用 `tests/fixtures/req170/schema.json`（`ontok-introspect`
//! 的形状），agent 用 `tests/fixtures/req147/fake-acp.js` 的 `maps` / `maps-bad` 模式
//!（真 ACP 子进程、假模型）。
//!
//! ⚠️ F-1 的教训（QA 退回项）：**自证要用目标形态**。本需求的形态是「只读账号」，
//! 用 owner（`postgres`）自证永远绿 —— 而且失效是**安静的**（build run 四段都通、
//! TTL 照样写出 2056B，只是 0 个 TriplesMap）。所以这条判据必须用只有 SELECT 的角色跑。

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

// ══════════════════════════════════════════════════════════════
// F-3（QA 退回项）· `--schema-max-tables 0` 是入参错 ⇒ exit 2
// ══════════════════════════════════════════════════════════════

/// ⭐ **F-3**：`--schema-max-tables 0` ⇒ 退出码 **2**（不是 1）。
///
/// 退回前的形态：那条闸写在 `doc_ingest_cmd` 里用 `anyhow::bail!`，落到退出码 **1**
///（环境错），与「入参不合法」的 2 混了。修法是让 **clap** 在解析阶段就拒
///（`value_parser(...).range(1..)`）—— 与其它入参错同一条出口，而且**一个字节都不写**。
#[test]
fn a_zero_schema_max_tables_is_an_input_error_with_exit_2() {
    let root = temp_dir("max-tables-zero");
    let out = root.join("a.jsonl");
    let mut cmd = Command::new(bin());
    cmd.args([
        "doc",
        "ingest",
        "--input",
        fixture("docs").to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--agent",
        "acp",
        "--agent-command",
        "ontok-agent-that-does-not-exist-req170",
        "--agent-skill",
        skill_fixture().to_str().unwrap(),
        "--schema",
        fixture("schema.json").to_str().unwrap(),
        "--schema-max-tables",
        "0",
    ]);
    clean_env(&mut cmd);
    let r = cmd.output().expect("跑不了 doc ingest");
    let err = String::from_utf8_lossy(&r.stderr).to_string();
    assert_eq!(
        r.status.code(),
        Some(2),
        "`--schema-max-tables 0` 是**入参错**（2），不是环境错（1）：\n{err}"
    );
    assert!(
        err.contains("--schema-max-tables"),
        "要点名那个参数：\n{err}"
    );
    assert!(
        !out.exists(),
        "⛔ 入参错时一个产物都不许写：{}",
        out.display()
    );
    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// F-1（QA 阻断项）· **只读角色**也必须看到主键
//
// 事故形态（QA 在只读账号 `qa_ro` 上实测）：
//
// | 账号 | `schema.json` 里 | r2rml |
// | --- | --- | --- |
// | owner（`postgres`） | `primary_key: "id"` | 2 TriplesMap / 5 POM / 2952B |
// | **只有 SELECT 的角色** | `primary_key: null` | **0 TriplesMap / 2056B**（头部「未生成类 2 —— 没有单列主键」） |
//
// 根因：`information_schema.table_constraints` 对**只有 SELECT** 的角色不可见
//（PG 的口径是「属主 or 持有 SELECT 之外的权限」）。而 `pg_index` / `pg_constraint`
// 对所有可连账号可见 —— 主键/唯一列/外键计数必须从那里取。
//
// 为什么这条判据**必须用真库 + 只读角色**：这是**权限可见性**的事实，
// 用 owner 跑永远绿；而失效是**安静**的（四段都通、TTL 照样写出，只是 0 个 TriplesMap）。
// ══════════════════════════════════════════════════════════════

fn db_url() -> Option<String> {
    for k in ["ONTOK_TEST_DATABASE_URL", "ONTOK_DATABASE_URL"] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// `postgresql://u:p@h:port/db` → `postgresql://<user>:<pw>@h:port/db`（只换 userinfo）。
fn with_credentials(url: &str, user: &str, pw: &str) -> String {
    let (scheme, rest) = url.split_once("://").expect("连接串要有 scheme");
    let at = rest.rfind('@').expect("连接串要有 userinfo@host");
    format!("{scheme}://{user}:{pw}@{}", &rest[at + 1..])
}

/// `postgresql://u:p@h:port/db?params` → 同一条连接串，只把库名换成 `db`。
fn swap_database(url: &str, db: &str) -> String {
    let (head, query) = match url.split_once('?') {
        Some((h, q)) => (h, format!("?{q}")),
        None => (url, String::new()),
    };
    let at = head.rfind('@').expect("连接串要有 userinfo@host");
    let tail = &head[at + 1..];
    let slash = tail.find('/').map(|i| at + 1 + i);
    match slash {
        Some(i) => format!("{}/{db}{query}", &head[..i]),
        None => format!("{head}/{db}{query}"),
    }
}

fn run_cli(args: &[&str], envs: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    for k in DB_ENVS {
        cmd.env_remove(k);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.env_remove("ONTOK_SOURCE_DATABASE_URL");
    let out = cmd.output().expect("起不了 ontok-builder");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// 跑 `build run` 直到第 ④ 段（评审台就绪）再杀掉，返回 stderr 全文。
///
/// ④ 是前台常驻，不杀就挂住 —— 草稿在第 ③ 段已经落盘，所以杀掉不影响断言。
fn build_run_until_desk(args: &[String], envs: &[(&str, &str)]) -> String {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    for k in DB_ENVS {
        cmd.env_remove(k);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.env_remove("ONTOK_SOURCE_DATABASE_URL");
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("起不了 build run");
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
    let stdout = child.stdout.take().expect("stdout");
    std::thread::spawn(move || {
        let mut buf = String::new();
        let mut r = stdout;
        let _ = r.read_to_string(&mut buf);
    });
    let mut seen: Vec<String> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_secs(180)) {
            Ok(line) => {
                let hit = line.contains("第 4/4 段");
                seen.push(line);
                if hit {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    seen.join("\n")
}

/// ⭐ **F-1 的硬判据**：只有 SELECT 的角色 ⇒ 主键照样看得到 ⇒ r2rml 出得了 TriplesMap。
///
/// 它一次钉三件事（缺一个都会让「只读账号」这条路**安静地**退化成 0 个 TriplesMap）：
///
/// 1. `schema.json` 里每张源表的 `primary_key` **非空**（这两张表真的都有主键）；
/// 2. 下发给模型的摘要**不许**写「主键：无（没有单列主键）」（那是假信息，会让 `maps_to` 无从对齐）；
/// 3. `r2rml generate` 出 **>0** 个 TriplesMap，且 `rr:tableName` 指向真表。
///
/// ⚠️ 会真建：两张带中文注释的源表 + 一个只有 SELECT 的角色 + 一个产品项目/版本。
/// 用**本轮唯一**的名字（pid + 纳秒），跑完清理；建角色被拒时**响亮跳过**（不是假绿）。
#[tokio::test]
async fn a_read_only_role_still_sees_primary_keys_so_r2rml_maps_the_tables() {
    const NAME: &str = "a_read_only_role_still_sees_primary_keys_so_r2rml_maps_the_tables";
    let Some(url) = db_url() else {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", NAME);
        return;
    };
    if !node_ok() || !fake_acp().is_file() {
        common::skip::missing("node / 假 ACP 夹具", NAME);
        return;
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tag = format!("{}_{stamp}", std::process::id());
    let role = format!("req170_ro_{tag}");
    let pw = "req170_ro_pw";
    let disk = format!("req170_disks_{tag}");
    let server = format!("req170_servers_{tag}");
    let project = format!("req170_ro_{tag}");
    // ⚠️ **本轮独占一个库**（源表 + 产品表都落在里面），跑完整个删掉。
    //
    // 为什么不复用共享测试库：这次踩到了 —— 上一轮被判红留下的同名源表
    //（注释同样是「云硬盘/云主机」）会让「按概念名落表」变成**歧义**，
    // 于是 `maps_to` 被丢、断言在**与修复无关**的地方假红。
    // 独占一个库之后，「schema.json 里只有本轮建的两张表」本身就是判据的一部分。
    let dbase = format!("req170_f1_{tag}");

    let admin = ontok::db::pool::connect(&url)
        .await
        .expect("连不上 ONTOK_TEST_DATABASE_URL 指的库");
    sqlx::query(&format!("CREATE DATABASE \"{dbase}\""))
        .execute(&admin)
        .await
        .unwrap_or_else(|e| {
            panic!("建独立测试库 {dbase} 失败（这条判据要求 CREATE DATABASE 权限）：{e}")
        });
    let url = swap_database(&url, &dbase);
    let pool = ontok::db::pool::connect(&url)
        .await
        .expect("连不上本轮的独立测试库");

    // ── 源表：两张都**真的**有单列主键 + 中文注释（摘要把它们匹配进来）──
    for sql in [
        format!("CREATE TABLE \"{disk}\" (id text PRIMARY KEY, server_id text NOT NULL)"),
        format!("COMMENT ON TABLE \"{disk}\" IS '云硬盘'"),
        format!("CREATE TABLE \"{server}\" (id text PRIMARY KEY)"),
        format!("COMMENT ON TABLE \"{server}\" IS '云主机'"),
    ] {
        sqlx::query(&sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("建源表失败：{sql} —— {e}"));
    }

    // ── 只读角色：**只有 SELECT**（这就是本需求的形态）──
    let created = sqlx::query(&format!("CREATE ROLE \"{role}\" LOGIN PASSWORD '{pw}'"))
        .execute(&admin)
        .await;
    if let Err(e) = created {
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{dbase}\" WITH (FORCE)"))
            .execute(&admin)
            .await;
        common::skip::missing(
            &format!("可建角色的测试库账号（CREATE ROLE 被拒：{e}）"),
            NAME,
        );
        return;
    }
    for sql in [
        format!("GRANT USAGE ON SCHEMA public TO \"{role}\""),
        format!("GRANT SELECT ON ALL TABLES IN SCHEMA public TO \"{role}\""),
    ] {
        sqlx::query(&sql)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{sql} 失败：{e}"));
    }
    let ro_url = with_credentials(&url, &role, pw);

    // ── 文档 + 基础提案（entity key = **真表名**，maps_to 落表才对得上）──
    let root = temp_dir("readonly");
    let docs = root.join("docs");
    std::fs::create_dir_all(&docs).expect("建语料目录");
    std::fs::write(
        docs.join("cloud.md"),
        "# 云硬盘与云主机\n\n云硬盘挂载到云主机：云硬盘的 server_id 指向云主机的主键 id。\n",
    )
    .expect("写语料");
    let proposal = root.join("base-proposal.json");
    std::fs::write(
        &proposal,
        format!(
            r#"{{"project":"{project}","base_version":1,"items":[
              {{"kind":"add_entity_type","target":"云硬盘","payload":{{"key":"{disk}","display_name":"云硬盘","band":"low","reviewed_by":null,
                "properties":[{{"name":"云硬盘编号","column":"id"}},{{"name":"所属云主机","column":"server_id"}}]}}}},
              {{"kind":"add_entity_type","target":"云主机","payload":{{"key":"{server}","display_name":"云主机","band":"low","reviewed_by":null,
                "properties":[{{"name":"云主机编号","column":"id"}}]}}}}]}}"#
        ),
    )
    .expect("写基础提案");
    let work = root.join("wd");

    // ── ① 真进程：**以只读角色**跑 build run --dsn ──
    let err = build_run_until_desk(
        &[
            "build".into(),
            "run".into(),
            "--input".into(),
            docs.display().to_string(),
            "--work-dir".into(),
            work.display().to_string(),
            "--project".into(),
            project.clone(),
            "--agent".into(),
            "acp".into(),
            "--agent-command".into(),
            format!("node {}", fake_acp().display()),
            "--agent-skill".into(),
            skill_fixture().display().to_string(),
            "--agent-timeout-secs".into(),
            "60".into(),
            "--bind".into(),
            "127.0.0.1:0".into(),
            "--no-browser".into(),
            "--proposal".into(),
            proposal.display().to_string(),
            "--dsn".into(),
            ro_url.clone(),
        ],
        &[("FAKE_ACP_MODE", "maps")],
    );

    // 失败时先把清理做掉，别把源表/角色留在共享库里
    let cleanup = |pool: sqlx::postgres::PgPool, role: String, disk: String, server: String| async move {
        for sql in [
            format!("DROP OWNED BY \"{role}\""),
            format!("DROP ROLE IF EXISTS \"{role}\""),
            format!("DROP TABLE IF EXISTS \"{disk}\", \"{server}\""),
        ] {
            let _ = sqlx::query(&sql).execute(&pool).await;
        }
    };

    assert!(
        err.contains("第 4/4 段"),
        "只读角色下 build run 四段都该走通（走到第 ④ 段）：\n{err}"
    );
    assert!(
        err.contains("maps_to") && err.contains("落地 1 条"),
        "只读角色下 maps_to 也要落地（主键/列都该看得见）：\n{err}"
    );
    // ② 摘要（**真正下发给模型的那一段**）不许写「没有主键」/「未勘明」。
    //    这是 QA 抓到的假信息形态：主键读不到 ⇒ 摘要说「无」 ⇒ agent 永远给不出对的 `maps_to`。
    //    ⚠️ 摘要正文不进 stderr（它只进 prompt），所以这里拿**同一份 schema.json** 重算一遍。
    let view = ontok::extract::schema_digest::SchemaView::from_path(&work.join("schema.json"))
        .expect("schema.json 要能被摘要视图读回");
    let digest = view.digest(
        "云硬盘挂载到云主机：云硬盘的 server_id 指向云主机的主键 id。",
        40,
    );
    assert!(
        !digest.text.contains("主键：无") && !digest.text.contains("未勘明"),
        "⛔ 只读角色下摘要写出了「没有主键」—— 这就是 QA 抓到的假信息：\n{}",
        digest.text
    );
    assert!(
        digest.text.contains("主键：id"),
        "摘要必须给出**真实**主键（只读账号也读过来了）：\n{}",
        digest.text
    );

    let schema = read_json(&work.join("schema.json"));
    let tables = schema["tables"].as_array().expect("schema.json 要有 tables");
    assert_eq!(
        tables.len(),
        2,
        "本轮的独立库里**只有**这两张源表（多出来就说明库不干净）：{schema}"
    );
    // ⭐ QA 的原话：「`schema.json` 里**每张表** `primary_key` 非空（真库确实有主键时）」。
    // 这两张表都真的 declared 了 `PRIMARY KEY (id)`，所以一张都不许是 null。
    for t in tables {
        let name = t["name"].as_str().unwrap_or("<无名字>");
        assert_eq!(
            t["primary_key"].as_str(),
            Some("id"),
            "⛔ 只读角色下 {name} 的主键丢了（`information_schema.table_constraints` 对只有 \
             SELECT 的角色不可见）—— schema.json 里必须是真实的 `id`：{t}"
        );
        let composite = t["composite_primary_key"].as_array().cloned().unwrap_or_default();
        assert!(composite.is_empty(), "{name} 不是复合主键：{t}");
        // 每列的 is_primary_key 也要对（摘要的「主键：无」就是从它来的）
        let pk_flags: Vec<bool> = t["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .map(|c| c["is_primary_key"].as_bool().unwrap_or(false))
            .collect();
        assert!(pk_flags.iter().any(|b| *b), "{name} 至少一列要标 is_primary_key：{t}");
    }
    // 两张源表都在（名字里带本轮 tag）
    for name in [&disk, &server] {
        assert!(
            tables.iter().any(|t| t["name"] == name.as_str()),
            "schema.json 里没有源表 {name}：{schema}"
        );
    }

    // ── ③ 真进程：把草稿变成定版，再 `r2rml generate` 出 TTL ──
    let db_env: &[(&str, &str)] = &[("ONTOK_DATABASE_URL", url.as_str())];
    let draft = work.join("out/proposal.json");
    for args in [
        vec!["project", "new", "--name", project.as_str()],
        vec!["version", "new", "--project", project.as_str()],
    ] {
        let (code, _o, e) = run_cli(&args, db_env);
        assert_eq!(code, Some(0), "`{}` 失败：\n{e}", args.join(" "));
    }
    let draft_s = draft.display().to_string();
    for args in [
        vec!["import", "--file", draft_s.as_str()],
        vec!["review", "decide", "--proposal", "p1", "--accept-all", "--project", project.as_str()],
        vec!["proposal", "apply", "--proposal", "p1"],
        vec!["version", "freeze", "--project", project.as_str(), "--version", "2"],
    ] {
        let (code, o, e) = run_cli(&args, db_env);
        assert_eq!(code, Some(0), "`{}` 失败：\n{o}\n{e}", args.join(" "));
    }
    let ttl = root.join("mapping.ttl");
    let schema_path = work.join("schema.json");
    let (code, o, e) = run_cli(
        &[
            "r2rml",
            "generate",
            "--project",
            project.as_str(),
            "--version",
            "2",
            "--schema",
            schema_path.to_str().unwrap(),
            "--out",
            ttl.to_str().unwrap(),
        ],
        db_env,
    );
    assert_eq!(code, Some(0), "r2rml generate 失败：\n{o}\n{e}");
    let text = std::fs::read_to_string(&ttl).expect("读不到 mapping.ttl");
    let maps = text.matches("a rr:TriplesMap").count();
    assert!(
        maps > 0,
        "⛔ 只读角色下 r2rml 出了 **0** 个 TriplesMap（QA 抓到的形态）—— 主键丢了，\
         所有类都被判「没有单列主键」而跳过：\n{text}"
    );
    assert!(
        text.contains(&format!("rr:tableName \"{disk}\"")),
        "TTL 里要有真实表的 TriplesMap（{disk}）：\n{text}"
    );
    assert!(
        text.contains(&format!("rr:tableName \"{server}\"")),
        "TTL 里要有真实表的 TriplesMap（{server}）：\n{text}"
    );
    assert!(
        maps >= 2,
        "两张表都该有 TriplesMap（只读账号下丢一张都不行），实际 {maps} 个：\n{text}"
    );

    cleanup(pool, role, disk, server).await;
    // 本轮独占的库整个删掉（`WITH (FORCE)` 把可能还没断的连接踢掉）
    let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{dbase}\" WITH (FORCE)"))
        .execute(&admin)
        .await;
    std::fs::remove_dir_all(&root).ok();
}
