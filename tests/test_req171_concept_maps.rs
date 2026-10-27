//! ⭐ REQ-171 · **「概念 → 表」这条对齐通道**的真进程判据。
//!
//! | # | 判据 | 它说什么 |
//! | --- | --- | --- |
//! | R1/AC-1 | [`a_concept_maps_to_lands_and_doc_merge_consumes_it`] | `doc ingest --schema` ⇒ 概念断言带 `maps_to.table`（可选列）；`doc merge` 能把它并进提案（不再只靠名字精确命中） |
//! | R1/AC-2 | [`the_four_concept_maps_to_errors_drop_the_whole_concept_and_count`] | 四类反例（形状 / 查不到表 / 对上多张 / 查不到列）**各一条真进程反例** ⇒ 整条概念丢弃 + 计数 +1 + 样例点名 |
//! | R3/AC-4 | [`belongs_to_and_references_are_no_longer_dropped_by_the_predicate_gate`] | 同一份回答里 `belongs_to` / `references` **不再被丢**（原始 vs 活下来）、`belongs_to_share.alive` 不再恒 0；真结构词仍被丢 |
//! | R4/AC-5 | [`the_screen_reports_which_selection_path_was_taken_and_junk_tables_are_excluded`] | 小库 ⇒ 屏幕报「全表 N 张」；`--schema-full-list-max` 关掉 ⇒ 报「按打分带」；垃圾表被排掉且**报数** |
//! | AC-3 | [`without_schema_neither_products_nor_screen_mention_maps_to`] | 不给 `--schema` ⇒ 产物里没有 `maps_to`（概念也没有）、屏幕没有库结构那几行 |
//! | R2 | [`the_protocol_and_the_digest_both_tell_the_model_to_annotate_concepts`] | 运行时协议与摘要**原文**里都要有「概念 → 表」这一半，且摘要里不再写「概念断言不写 maps_to」 |
//!
//! # 为什么是**真进程**
//!
//! 这些都是进程边界上的事实：clap 收的参数、`stats.json` 的字段、屏幕上的那一行、
//! `doc merge` 的产物。单元判据只能说「函数返回了什么」。
//!
//! ⚠️ 不连真库、不花 token：schema 用 `tests/fixtures/req171/schema.json`，
//! agent 用 `tests/fixtures/req147/fake-acp.js` 的 `cmap` / `cmap-good`（真 ACP 子进程、假模型）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

fn crate_root() -> PathBuf {
    common::source_root::crate_root()
}

fn fixture(rel: &str) -> PathBuf {
    crate_root().join("tests/fixtures/req171").join(rel)
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

fn skip() -> bool {
    if !node_ok() || !fake_acp().is_file() {
        eprintln!("SKIP（REQ-171 真进程）：本机没有 node 或假 ACP 夹具 —— 这条判据**未验**。");
        return true;
    }
    false
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req171-{tag}-{}-{}",
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
const DB_ENVS: [&str; 3] =
    ["ONTOK_DATABASE_URL", "ONTOK_TEST_DATABASE_URL", "ONTOK_SECRET_KEY"];

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

/// 跑一次 `doc ingest`（可带若干额外参数），返回 `(stdout, stderr)`。
fn doc_ingest_with(
    out: &Path,
    cache: &Path,
    mode: &str,
    schema: Option<&Path>,
    extra: &[&str],
) -> (String, String) {
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
    cmd.args(extra);
    clean_env(&mut cmd);
    cmd.env("FAKE_ACP_MODE", mode);
    let r = cmd.output().expect("跑不了 doc ingest");
    let stdout = String::from_utf8_lossy(&r.stdout).to_string();
    let stderr = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(
        r.status.success(),
        "doc ingest 应当成功（mode={mode}, extra={extra:?}）：\n{stdout}\n{stderr}"
    );
    (stdout, stderr)
}

fn doc_ingest(out: &Path, cache: &Path, mode: &str, schema: Option<&Path>) -> (String, String) {
    doc_ingest_with(out, cache, mode, schema, &[])
}

fn all_assertions(out: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(out).expect("读不到断言 JSONL");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("JSONL 每行都该是合法 JSON"))
        .collect()
}

// ══════════════════════════════════════════════════════════════
// AC-1 · 概念 → 表落地，且 `doc merge` 吃得下
// ══════════════════════════════════════════════════════════════

#[test]
fn a_concept_maps_to_lands_and_doc_merge_consumes_it() {
    if skip() {
        return;
    }
    let root = temp_dir("cmap-land");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, &cache, "cmap-good", Some(&fixture("schema.json")));

    let lines = all_assertions(&out);
    let concepts: Vec<&Value> = lines.iter().filter(|v| v["kind"] == "concept").collect();
    assert_eq!(concepts.len(), 2, "两条概念都该留下：{lines:#?}");
    let model = concepts
        .iter()
        .find(|c| c["name"] == "模型")
        .unwrap_or_else(|| panic!("没有「模型」这条概念：{concepts:#?}"));
    assert_eq!(
        model["maps_to"]["table"].as_str(),
        Some("llm_models"),
        "概念「模型」要落到 llm_models 上：{model}"
    );
    assert!(
        model["maps_to"].get("column").is_none(),
        "整行概念不给 column：{model}"
    );
    let customer = concepts
        .iter()
        .find(|c| c["name"] == "客户")
        .unwrap_or_else(|| panic!("没有「客户」这条概念：{concepts:#?}"));
    assert_eq!(customer["maps_to"]["table"].as_str(), Some("customers"));
    assert_eq!(customer["maps_to"]["column"].as_str(), Some("email"));

    // 关系（抽象概念之间、没有 join 列）**不该**带 maps_to
    let rel = lines
        .iter()
        .find(|v| v["kind"] == "relation")
        .unwrap_or_else(|| panic!("关系那条没了：{lines:#?}"));
    assert!(
        rel.get("maps_to").is_none(),
        "抽象关系不该有 maps_to（REQ-171：没有 join 列就别写）：{rel}"
    );

    // stats.json：概念与关系**两组**计数都在，且互不串味
    let stats = read_json(&out.with_extension("stats.json"));
    assert_eq!(stats["maps_to"]["concepts"]["kept"], 2, "{stats}");
    assert_eq!(stats["maps_to"]["concepts"]["dropped"], 0, "{stats}");
    assert_eq!(stats["maps_to"]["kept"], 0, "关系那组不该被概念数污染：{stats}");

    // 屏幕：两行都要有
    assert!(err.contains("maps_to(概念)"), "屏幕要单列概念那一行：\n{err}");
    assert!(err.contains("落地 2 条"), "屏幕要报概念落地数：\n{err}");

    // ── `doc merge`：概念靠 `maps_to` 并进提案（不再只走「名字精确命中」）──
    let merged = root.join("merged.json");
    let merged_stats = root.join("merge.stats.json");
    let mut merge = Command::new(bin());
    merge.args([
        "doc",
        "merge",
        "--assertions",
        out.to_str().unwrap(),
        "--proposal",
        fixture("proposal.json").to_str().unwrap(),
        "--out",
        merged.to_str().unwrap(),
        "--stats",
        merged_stats.to_str().unwrap(),
        "--schema",
        fixture("schema.json").to_str().unwrap(),
    ]);
    clean_env(&mut merge);
    let r = merge.output().expect("跑不了 doc merge");
    let m_err = String::from_utf8_lossy(&r.stderr).to_string();
    assert!(r.status.success(), "doc merge 应当成功：\n{m_err}");

    let mstats = read_json(&merged_stats);
    assert!(
        mstats["merged"].as_u64().unwrap_or(0) >= 2,
        "两条概念都要并进去：{mstats}"
    );
    let doc = read_json(&merged);
    let items = doc["items"].as_array().expect("items");
    let model_item = items
        .iter()
        .find(|it| it["payload"]["key"] == "llm_models")
        .unwrap_or_else(|| panic!("提案里没有 llm_models：{doc}"));
    assert_eq!(
        model_item["origin"].as_str(),
        Some("both"),
        "概念 maps_to 命中 ⇒ origin = both：{model_item}"
    );
    let ev = model_item["payload"]["doc_evidence"]
        .as_array()
        .unwrap_or_else(|| panic!("没有 doc_evidence：{model_item}"));
    assert!(!ev.is_empty(), "doc 证据要留下：{model_item}");
    assert_eq!(
        ev[0]["maps_to"]["table"].as_str(),
        Some("llm_models"),
        "证据里要记「靠哪条 maps_to 并进来的」：{model_item}"
    );

    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// AC-2 · 四类反例：整条概念丢弃 + 计数
// ══════════════════════════════════════════════════════════════

#[test]
fn the_four_concept_maps_to_errors_drop_the_whole_concept_and_count() {
    if skip() {
        return;
    }
    let root = temp_dir("cmap-bad");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, &cache, "cmap", Some(&fixture("schema.json")));

    let lines = all_assertions(&out);
    let names: Vec<&str> =
        lines.iter().filter(|v| v["kind"] == "concept").map(|c| c["name"].as_str().unwrap_or("")).collect();
    assert_eq!(names, vec!["模型"], "只有能落表的那条概念留下：{lines:#?}");
    // 四条反例的概念**整条**都不在（不是只丢 maps_to）
    for gone in ["客户", "账户余额", "平台", "单价"] {
        assert!(
            !names.contains(&gone),
            "⛔ 概念「{gone}」的 maps_to 坏了 ⇒ 整条概念都该丢：{names:?}"
        );
    }

    let stats = read_json(&out.with_extension("stats.json"));
    let c = &stats["maps_to"]["concepts"];
    assert_eq!(c["kept"], 1, "{stats}");
    assert_eq!(c["dropped"], 4, "四类各一条：{stats}");
    assert_eq!(c["dropped_shape"], 1, "{stats}");
    assert_eq!(c["dropped_table_unknown"], 1, "{stats}");
    assert_eq!(c["dropped_table_ambiguous"], 1, "{stats}");
    assert_eq!(c["dropped_column_absent"], 1, "{stats}");
    let samples = c["samples"].as_array().cloned().unwrap_or_default();
    assert!(!samples.is_empty(), "丢弃要留样例（⛔ 只丢不报等于静默）：{stats}");
    assert!(
        samples.iter().any(|s| s.as_str().unwrap_or("").contains("no_such_table")),
        "查不到表要点名到表：{samples:?}"
    );
    // 屏幕：分类计数逐个报出来
    for needle in ["形状 1", "查不到表 1", "对上多张 1", "查不到列 1"] {
        assert!(err.contains(needle), "屏幕要报「{needle}」：\n{err}");
    }
    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// AC-4 · `belongs_to` / `references` 放行
// ══════════════════════════════════════════════════════════════

#[test]
fn belongs_to_and_references_are_no_longer_dropped_by_the_predicate_gate() {
    if skip() {
        return;
    }
    let root = temp_dir("belongs");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, &cache, "cmap", Some(&fixture("schema.json")));

    let stats = read_json(&out.with_extension("stats.json"));
    let alive = stats["predicate_counts"].as_object().cloned().unwrap_or_default();
    assert_eq!(
        alive.get("belongs_to").and_then(Value::as_u64),
        Some(1),
        "⛔ belongs_to 仍被丢掉了（这是 R3 的判据）：{stats}"
    );
    assert_eq!(
        alive.get("references").and_then(Value::as_u64),
        Some(1),
        "⛔ references 仍被丢掉了：{stats}"
    );
    // 真结构词（related_to）仍要在这道闸上
    assert_eq!(stats["rejected_by_predicate_gate"]["total"], 1, "{stats}");
    assert_eq!(
        stats["rejected_by_predicate_gate"]["by_predicate"]["related_to"],
        1,
        "{stats}"
    );
    // `belongs_to_share.alive` 不再恒 0（AC-004 的判据）
    let b = &stats["belongs_to_share"]["alive"];
    assert_eq!(b["count"], 1, "AC-004 的分子要非 0：{stats}");
    assert_eq!(b["total"], 2, "活下来 2 条关系（belongs_to + references）：{stats}");
    assert!(
        b["share"].as_f64().unwrap_or(0.0) > 0.0,
        "占比必须是正数（放行前恒 0）：{stats}"
    );
    // 屏幕：原始 / 活下来两个数都要报，且 belongs_to 真的在「活下来」里
    assert!(err.contains("belongs_to"), "屏幕要报 belongs_to：\n{err}");
    assert!(
        err.contains("belongs_to 1"),
        "「活下来的谓词」那一行要有 belongs_to 1：\n{err}"
    );
    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// AC-5 · 选表走哪条路 + 垃圾表
// ══════════════════════════════════════════════════════════════

#[test]
fn the_screen_reports_which_selection_path_was_taken_and_junk_tables_are_excluded() {
    if skip() {
        return;
    }
    let root = temp_dir("select");
    let out = root.join("assertions.jsonl");

    // ① 缺省：4 张业务表 ≤ 200 ⇒ **全表清单**，2 张垃圾表被排掉
    let cache = root.join("cache1.json");
    let (_o, err) = doc_ingest(&out, &cache, "cmap-good", Some(&fixture("schema.json")));
    assert!(err.contains("全表 4 张"), "屏幕要报「全表 N 张」这条路径：\n{err}");
    assert!(err.contains("/ 共 6 张表"), "要报「共几张」（含被排掉的那些）：\n{err}");
    assert!(err.contains("跳过垃圾表 2 张"), "要报排除了几张：\n{err}");
    assert!(
        err.contains("排除了 2 张") && err.contains("alembic_version"),
        "点名排掉了哪几张（样例）：\n{err}"
    );

    // ② `--schema-full-list-max 2` ⇒ 4 > 2 ⇒ 走**打分**那条路
    let cache2 = root.join("cache2.json");
    let (_o, err2) = doc_ingest_with(
        &out,
        &cache2,
        "cmap-good",
        Some(&fixture("schema.json")),
        &["--schema-full-list-max", "2"],
    );
    assert!(err2.contains("按打分带"), "屏幕上要报走的是打分那条路：\n{err2}");
    assert!(!err2.contains("全表 4 张"), "⛔ 两条路不能同时报：\n{err2}");

    // ③ `--schema-keep-junk` ⇒ 一张都不排（用户的表真叫 `_foo` 时）
    let cache3 = root.join("cache3.json");
    let (_o, err3) = doc_ingest_with(
        &out,
        &cache3,
        "cmap-good",
        Some(&fixture("schema.json")),
        &["--schema-keep-junk"],
    );
    assert!(err3.contains("跳过垃圾表 0 张"), "keep-junk ⇒ 不禁：\n{err3}");

    // ④ `--schema-exclude-tables` 可追加（连真业务表也能按用户的话排掉）
    let cache4 = root.join("cache4.json");
    let (_o, err4) = doc_ingest_with(
        &out,
        &cache4,
        "cmap-good",
        Some(&fixture("schema.json")),
        &["--schema-exclude-tables", "customers"],
    );
    assert!(err4.contains("跳过垃圾表 3 张"), "追加模式要生效：\n{err4}");

    // ⑤ 阈值本身要能拒非法入参（`--schema-max-tables` 的 0 是入参错，这里是**字符串**错）
    let mut bad = Command::new(bin());
    bad.args([
        "doc",
        "ingest",
        "--input",
        fixture("docs").to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--agent",
        "acp",
        "--agent-command",
        "node-does-not-exist-req171",
        "--agent-skill",
        skill_fixture().to_str().unwrap(),
        "--schema",
        fixture("schema.json").to_str().unwrap(),
        "--schema-full-list-max",
        "abc",
    ]);
    clean_env(&mut bad);
    let r = bad.output().expect("跑不了 doc ingest");
    assert_eq!(r.status.code(), Some(2), "非法入参必须是退出码 2（clap 层拒）");

    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// AC-3 · 无 `--schema` ⇒ 产物里没有 maps_to（概念也没有）
// ══════════════════════════════════════════════════════════════

#[test]
fn without_schema_neither_products_nor_screen_mention_maps_to() {
    if skip() {
        return;
    }
    let root = temp_dir("no-schema");
    let out = root.join("assertions.jsonl");
    let cache = root.join("cache.json");
    let (_o, err) = doc_ingest(&out, &cache, "cmap", None);

    let text = std::fs::read_to_string(&out).expect("读不到断言 JSONL");
    assert!(
        !text.contains("maps_to"),
        "⛔ 没给 --schema ⇒ 产物里不许出现任何 maps_to（概念也不行）：\n{text}"
    );
    assert!(!text.contains("llm_models"), "⛔ 连表名都不该冒出来：\n{text}");
    let stats = read_json(&out.with_extension("stats.json"));
    assert!(
        stats.get("maps_to").is_none(),
        "⛔ 没给 --schema ⇒ stats.json 里不该有 maps_to 那一节：{stats}"
    );
    assert!(!err.contains("库结构"), "⛔ 没给 schema 就不该报库结构：\n{err}");
    assert!(!err.contains("maps_to(概念)"), "⛔ 屏幕也不该有概念那一行：\n{err}");
    std::fs::remove_dir_all(&root).ok();
}

// ══════════════════════════════════════════════════════════════
// R2 · 协议 + 摘要的**原文**都要有「概念 → 表」
// ══════════════════════════════════════════════════════════════

#[test]
fn the_protocol_and_the_digest_both_tell_the_model_to_annotate_concepts() {
    // ① 运行时指令（ACP 通道的 system 消息）
    let proto = ontok::extract::doc_acp::OUTPUT_PROTOCOL;
    for needle in [
        "概念 → 表",
        "关系 → 两列",
        "\"table\": \"表名\"",
        "child_column",
        "逐个概念尽量标",
        "给不出就不写",
        "两件不同的事",
    ] {
        assert!(proto.contains(needle), "协议里缺「{needle}」：\n{proto}");
    }
    // 协议里**必须**把概念 maps_to 写进 JSON 形状（只写规则没有形状 = 模型看不到这个字段）。
    // 形状是**多行**的（concepts 那行可能折行），所以取 `{"concepts"` 之后的一段整体看。
    let at = proto.find("{\"concepts\"").unwrap_or_else(|| panic!("协议里没有形状：\n{proto}"));
    let shape: String = proto[at..].chars().take(600).collect();
    assert!(
        shape.contains("maps_to") && shape.contains("\"table\""),
        "协议的形状里**必须**给出概念 maps_to（实验 C 的教训）：{shape}"
    );

    // ② schema 摘要（真文档 + 真 schema 计算出来的那一段）
    let view = ontok::extract::schema_digest::SchemaView::from_path(&fixture("schema.json"))
        .expect("夹具 schema 合法");
    let d = view.digest(
        "模型按 Token 计费；客户在账户余额里充值。",
        &ontok::extract::schema_digest::DigestOptions::default(),
    );
    for needle in ["概念 → 表", "逐个概念", "给不出就不写", "两件不同的事"] {
        assert!(d.text.contains(needle), "摘要里缺「{needle}」：\n{}", d.text);
    }
    assert!(
        !d.text.contains("概念断言**不写**"),
        "⛔ 摘要里那条「概念断言不写 maps_to」是老 bug 的源头（实验 C 0 落地）：\n{}",
        d.text
    );
    // 全表清单 + 列明细都要有（列注释是中文的桥）
    assert!(d.text.contains("llm_models"), "{}", d.text);
    assert!(d.text.contains("模型名"), "列注释要进摘要：{}", d.text);
    assert!(!d.text.contains("_volc_dts_pg_ddl"), "垃圾表不进清单：{}", d.text);
}
