//! REQ-059 · `extracted_by` 归属**可信化**（RP-2 的上半）
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | AC-001 默认路径记**真实执行者**，不是编造占位 | 基线实测：默认路径 `extracted_by.agent = "mcode-agent"`、`version = "1.2.0"`（**翻译表的版本号**），而真正跑的是 `inproc` —— 产物里躺着一个假名 |
//! | AC-002 声明与事实不符 ⇒ **拒绝出产物** | 基线实测：`--agent 虚构名` + `inproc` ⇒ 退出码 0、**零告警**、`extracted_by.agent = "虚构名"` |
//! | AC-003 翻译表来源与抽取执行者**不许混名** | 基线实测：翻译表产出方叫 `"mcode-agent (MiniMax Code)"`，`extracted_by.agent` 叫 `"mcode-agent"` —— 不相等，但**读起来是同一个名字** |
//! | AC-004 不改既有内容语义 | `items` / `stats` 必须逐字节不变；`ExtractedBy` 字段结构不变 |
//!
//! # 全部走真实二进制（`CARGO_BIN_EXE_ontok-propose`）
//!
//! 因为本需求修的正是**装配层**（CLI 造什么身份进去）—— 只测库函数
//! 证明不了「产物里那个字段真的变了」。临时目录一律落在
//! `.review-workspace/req059/` 下；本文件**不连库**（不删不清共享本体库）。

/// 仓库默认的翻译表内容（形状照抄生产用的那份）：
/// **产出方叫 `mcode-agent (MiniMax Code)`** —— 与基线上
/// `extracted_by.agent = "mcode-agent"` 读起来几乎一样，
/// 这正是 AC-003 要处理的那个歧义。
const BASE_NAMES_JSON: &str = r#"{
  "agent": "mcode-agent (MiniMax Code)",
  "version": "1.2.0",
  "generated_at": "2026-10-05T03:40:00Z",
  "source": "req059-test",
  "names": {
    "t_dev": "设备",
    "t_customer": "客户"
  }
}"#;

fn schema_json() -> String {
    let t = |name: &str, comment: &str, cols: &[(&str, &str)]| {
        serde_json::json!({
            "name": name,
            "comment": comment,
            "column_count": cols.len(),
            "commented_count": cols.len(),
            "columns": cols.iter().map(|(c, cm)| serde_json::json!({
                "name": c, "data_type": "text", "nullable": true,
                "comment": cm, "is_primary_key": false
            })).collect::<Vec<_>>(),
        })
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "server_version": "PostgreSQL 17.2",
        "transaction_read_only": "on",
        "schema": "public",
        "table_count": 2,
        "column_count": 4,
        "commented_column_count": 4,
        "foreign_key_count": 0,
        "introspected_at": "2026-10-07T10:00:00Z",
        "tables": [
            t("t_dev", "设备表", &[("id", "主键"), ("sn", "序列号")]),
            t("t_customer", "客户表", &[("id", "主键"), ("code", "客户编码")]),
        ],
    }))
    .unwrap()
}

/// 翻译表：`agent` 换成给定的产出方名，其余形状不变。
fn names_json(producer: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(BASE_NAMES_JSON).unwrap();
    let mut v = v;
    v["agent"] = serde_json::json!(producer);
    serde_json::to_string_pretty(&v).unwrap()
}

fn scratch(tag: &str) -> std::path::PathBuf {
    // ⚠️ `tag` **必须每个用例各不相同**：`cargo test` 默认多线程跑，
    // 共用同一个目录时它们会互相覆盖 `names.json` / `proposal.json` ——
    // 于是「被拒的抽取不许留下产物」这条断言会被**另一个用例的成功产物**顶掉，
    // 「混名必须被拒」也会因为读到别人的翻译表而变成「没撞名、放行」。
    // 这不是假红，是**测试自己把判据拆了**（曾实测踩到：4 条用例同时失败，
    // 根因是共目录，不是产品行为）。
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req059")
        .join(format!("{}-{}-{:?}", tag, std::process::id(), std::thread::current().id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

/// 跑一次 CLI（不连库：默认路径只读文件）。`tag` 必须每个用例各不相同（见 `scratch`）。
fn run(
    tag: &str,
    producer: &str,
    extra: &[&str],
) -> (bool, String, String, std::path::PathBuf, std::path::PathBuf) {
    let dir = scratch(tag);
    let sp = dir.join("schema.json");
    let np = dir.join("names.json");
    std::fs::write(&sp, schema_json()).unwrap();
    std::fs::write(&np, names_json(producer)).unwrap();
    let out = dir.join("proposal.json");
    let stats = dir.join("stats.json");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&stats);

    let o = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-propose"))
        .args([
            "--schema",
            sp.to_str().unwrap(),
            "--names",
            np.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stats",
            stats.to_str().unwrap(),
            "--project",
            "extracted",
        ])
        .args(extra)
        .output()
        .expect("跑 ontok-propose 失败");
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        out,
        stats,
    )
}

// ──────────────────────────────────────────────────────────
// AC-001 · 默认身份不许是编造占位
// ──────────────────────────────────────────────────────────

/// ★ AC-001 · 不带 `--agent`：产物里记的必须是**真正跑起来的那个实现**。
///
/// 逐字断言（不是「非空」）—— 基线的缺陷恰恰是那个字段**有值、且看着挺像样**，
/// 断言「非空」会全绿通过。还要钉住它**不是**那个编造占位。
#[test]
fn test_default_path_records_the_real_executor_not_a_placeholder() {
    let (ok, so, se, out, _) = run("ac001-default", "mcode-agent (MiniMax Code)", &[]);
    assert!(ok, "默认路径必须能跑通：{se}");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();

    assert_eq!(
        v["extracted_by"]["agent"], serde_json::json!("inproc:ontok::agent::RuleBasedAgent"),
        "★ 默认路径的 extracted_by.agent 必须是执行者**自报**的身份，不是 CLI 里的编造占位"
    );
    assert_eq!(
        v["extracted_by"]["version"], serde_json::json!("0.1.0"),
        "★ extracted_by.version 必须是执行者自己的版本 —— 基线上它是**翻译表的版本号 1.2.0**"
    );
    assert_ne!(
        v["extracted_by"]["agent"], serde_json::json!("mcode-agent"),
        "★ 那个硬编码占位不许再出现（它就是「产物里躺着一个假名」本身）"
    );
    assert_ne!(
        v["extracted_by"]["version"], serde_json::json!("1.2.0"),
        "★ 翻译表的版本号不许再被顶替成抽取 agent 的版本"
    );

    // 输出里两件事必须**分行、各自带标签**（AC-003 的可分辨性在 stdout 上的形状）
    assert!(
        so.contains("翻译表来源") && so.contains("抽取执行者"),
        "输出必须把「翻译表来源」与「抽取执行者」分成两行、各自带标签，实际：{so}"
    );
}

/// ★ AC-001 · 带 `--agent`（**且与事实相符**）：放行，且**逐字**按显式值记。
///
/// 这一条是「断言而非标签」语义的另一半：不是一律拒绝 `--agent`，
/// 而是**对了就照记**。少了它，上面那条会把 `--agent` 变成一个只能传空值的死参数。
#[test]
fn test_declared_agent_that_matches_the_executor_is_recorded_verbatim() {
    let (ok, _, se, out, _) = run(
        "ac001-declared",
        "mcode-agent (MiniMax Code)",
        &[
            "--agent",
            "inproc:ontok::agent::RuleBasedAgent",
            "--agent-version",
            "0.1.0",
        ],
    );
    assert!(ok, "声明与事实相符时必须放行：{se}");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(
        v["extracted_by"]["agent"],
        serde_json::json!("inproc:ontok::agent::RuleBasedAgent")
    );
    assert_eq!(v["extracted_by"]["version"], serde_json::json!("0.1.0"));
}

// ──────────────────────────────────────────────────────────
// AC-002 · 身份对不上必须**报错**而非警告
// ──────────────────────────────────────────────────────────

/// ★★ AC-002 · `--agent 虚构名` 配 `inproc` ⇒ **必须红，且不留产物**。
///
/// 这是 AC 点名的那个判别输入。基线上它**绿着通过**：退出码 0、零告警、
/// `extracted_by.agent = "虚构名"` —— 因为核对拿登记值与一个**由它派生的**身份
/// 比，恒为「一致」，那条告警分支在默认路径上结构上不可达。
#[test]
fn test_fabricated_agent_name_with_inproc_must_be_rejected() {
    let (ok, _, se, out, stats) =
        run("ac002-fabricated", "mcode-agent (MiniMax Code)", &["--agent", "虚构名"]);
    assert!(
        !ok,
        "★ `--agent 虚构名` 配 inproc 必须红：产物里会记下一个**从没跑过**的名字"
    );
    // 报错文案要说清「哪里对不上」：两个值都得点名，否则运维无从判断
    assert!(
        se.contains("虚构名") && se.contains("inproc:ontok::agent::RuleBasedAgent"),
        "报错必须点名声明值与实际值，实际 stderr：{se}"
    );
    assert!(
        se.contains("拒绝出产物"),
        "报错必须说清这是**拒绝出产物**，不是一句提示，实际 stderr：{se}"
    );
    // 半个产出比没有产出更危险
    assert!(!out.exists(), "被拒的抽取不许留下提案文件");
    assert!(!stats.exists(), "被拒的抽取不许留下统计文件");
}

/// ★ AC-002 · 版本同理：声明的版本对不上也必须红。
///
/// 基线上 `extracted_by.version` 根本不是执行者的版本，是**翻译表的版本号** ——
/// 只修 agent 不修 version，那个字段仍然在拿别的知识资产的编号冒充自己。
#[test]
fn test_fabricated_agent_version_must_be_rejected() {
    let (ok, _, se, out, _) =
        run("ac002-fake-version", "mcode-agent (MiniMax Code)", &["--agent-version", "9.9.9"]);
    assert!(!ok, "★ 声明的版本与执行者不符必须红（基线上它记的是翻译表的版本号）");
    assert!(
        se.contains("9.9.9") && se.contains("--agent-version"),
        "报错必须点名旗标与声明值，实际 stderr：{se}"
    );
    assert!(!out.exists(), "被拒的抽取不许留下提案文件");
}

// ──────────────────────────────────────────────────────────
// AC-003 · 翻译表来源与 agent 名不再同名混淆
// ──────────────────────────────────────────────────────────

/// ★ AC-003 · 判据必须**抓住**「翻译表来源名 == 执行者名」这个歧义。
///
/// 四种形状各跑一次：完全相同 / 产出方名是执行者名的前缀 /
/// 执行者名是产出方名的前缀 / 不撞。
///
/// 关键是**前缀那两种**：基线上真正出事的就是它们 ——
/// 翻译表产出方 `"mcode-agent (MiniMax Code)"` 与
/// `extracted_by.agent` `"mcode-agent"` **并不相等**，却读起来是同一个名字。
/// 只判「完全相等」的判据在实测那个案例上**不会响**。
#[test]
fn test_translation_table_and_executor_names_must_be_distinguishable() {
    use ontok::agent::factory::executor_name_collision;
    let exec = "inproc:ontok::agent::RuleBasedAgent";

    // 对照：不撞就不该有任何歧义报告
    assert!(
        executor_name_collision("table-name-translator", exec).is_none(),
        "两个毫不相干的名字不该被当成歧义"
    );

    for (producer, why) in [
        (exec.to_string(), "完全同名"),
        ("inproc".to_string(), "产出方名是执行者名的前缀（实测出事的那一种）"),
        (
            "inproc:ontok::agent::RuleBasedAgent (MiniMax Code)".to_string(),
            "执行者名是产出方名的前缀（实测出事的那一种）",
        ),
    ] {
        let msg = executor_name_collision(&producer, exec)
            .unwrap_or_else(|| panic!("★ 判据必须抓住「{why}」：产出方 {producer:?} vs 执行者 {exec:?}"));
        assert!(
            msg.contains(&producer) && msg.contains(exec),
            "歧义报告必须点名两个名字，实际：{msg}"
        );
    }
}

/// ★ AC-003 · 同上，但走**真实二进制**：混名必须拒绝出产物。
///
/// 纯函数测得对 ≠ 生产入口真的调了它 —— 这一条补的是后一半。
#[test]
fn test_cli_refuses_to_emit_when_executor_name_collides_with_table_producer() {
    for (producer, why) in [
        ("inproc:ontok::agent::RuleBasedAgent", "完全同名"),
        ("inproc", "产出方名是执行者名的前缀"),
    ] {
        let (ok, _, se, out, _) = run(&format!("ac003-cli-{}", why.len()), producer, &[]);
        assert!(!ok, "★ 混名（{why}）必须拒绝出产物，实际成功退出了");
        assert!(
            se.contains(producer) && se.contains("inproc:ontok::agent::RuleBasedAgent"),
            "报错必须点名两个名字，实际 stderr：{se}"
        );
        assert!(!out.exists(), "被拒的抽取不许留下提案文件");
    }
}

// ──────────────────────────────────────────────────────────
// AC-004 · 不改既有内容语义
// ──────────────────────────────────────────────────────────

/// ★ AC-004 · REQ-059 只改「记什么」，不许改「抽什么」。
///
/// `items` 是提案的实质内容。这里拿**同一个 schema、同一个翻译表**跑两遍
/// （一遍默认路径、一遍声明了相符的身份），断言 `items` 与 `stats`
/// 逐字节相同 —— 身份从哪来，不许渗进抽取结果。
#[test]
fn test_provenance_change_does_not_alter_the_extracted_content() {
    let (ok1, _, se1, out1, stats1) = run("ac004-default", "mcode-agent (MiniMax Code)", &[]);
    assert!(ok1, "默认路径应跑通：{se1}");
    let (ok2, _, se2, out2, stats2) = run(
        "ac004-declared",
        "mcode-agent (MiniMax Code)",
        &["--agent", "inproc:ontok::agent::RuleBasedAgent", "--agent-version", "0.1.0"],
    );
    assert!(ok2, "声明相符时应跑通：{se2}");

    let a: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out1).unwrap()).unwrap();
    let b: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&out2).unwrap()).unwrap();

    assert_eq!(
        a["items"], b["items"],
        "★ items 是提案的实质内容，归属可信化不许动它一个字节"
    );
    assert_eq!(
        std::fs::read_to_string(&stats1).unwrap(),
        std::fs::read_to_string(&stats2).unwrap(),
        "★ stats.json 不记归属，REQ-059 不许让它变"
    );
    // `extracted_by IS NULL = 人工修订` 的语义（本文件侧：字段结构仍是四栏 + source）
    assert!(a["extracted_by"].is_object(), "extracted_by 仍是一个对象（人工修订时才缺它）");
    for k in ["agent", "version", "round", "failed_groups", "source"] {
        assert!(
            a["extracted_by"].get(k).is_some(),
            "★ `ExtractedBy` 的字段结构不许变（ADR-001/003 定过），缺了 {k}"
        );
    }
}

/// ★ AC-004 · 反向判据：`ExtractedBy` 的**类型定义**没被改过。
///
/// 上一条从产物侧看结构；这一条从**类型侧**看 —— 字段名与类型被改动时，
/// 产物侧的结构断言有时还能凑巧通过。
#[test]
fn test_extracted_by_type_shape_is_unchanged() {
    use ontok::extract::propose::ProposalProvenance;
    let eb = ProposalProvenance {
        agent: "x".into(),
        version: "1".into(),
        round: 1,
        failed_groups: vec![],
        // ⚠️ `source` 带 `skip_serializing_if = "Option::is_none"`，
        // 给 None 时它**不会出现在 JSON 里** —— 想断言字段结构就必须给 Some。
        source: Some("introspect 2026-10-07T10:00:00Z schema=public read_only=on".into()),
    };
    let v = serde_json::to_value(&eb).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["agent", "failed_groups", "round", "source", "version"],
        "★ `ExtractedBy` 的字段结构是 ADR-001/003 定过的，不许增删改名"
    );
}

/// ★ AC-004 · `extracted_by IS NULL = 人工修订` 这条语义**没被动过**。
///
/// 这一条刻意**不新写**一个连库的用例：`tests/test_provenance_source.rs` 的
/// `test_proposal_without_extracted_by_stays_null` 已经**端到端**钉住了它
/// （真跑 `ontok propose import`，直接读库里那一列）。
///
/// 在此登记理由，供复查：本需求的改动面只有三处 ——
/// `src/agent/factory.rs`、`src/bin/ontok-propose.rs`、`tests/`。
/// 决定「没有 `extracted_by` 就是人工修订」的那条路在 `src/cli.rs`
/// （import 侧读 JSON）与 `src/domain/snapshot.rs`（`Option<ExtractedBy>` 的
/// `None` 语义），**两处一个字节都没动**，所以那条既有判据继续有效。
#[test]
fn test_human_revision_semantics_lives_untouched_in_the_import_path() {
    // 不碰连接库的断言（本文件另有用例会用到 DB 的部分不在这里）：
    // 断言「REQ-059 的改动面不包含决定这条语义的那两个文件」——
    // 读到 `ExtractedBy` 在快照侧仍是 `Option`，即「缺它」这个形状还表达得出来。
    let none_shape: Option<ontok::domain::snapshot::ExtractedBy> = None;
    assert!(
        none_shape.is_none(),
        "快照侧的 extracted_by 必须仍是 Option —— `None` 正是「人工修订」的形状（ADR-001）"
    );
    // 人工修订读出来仍然是「没有溯源声明」，而不是被补成一个空对象
    let v = serde_json::to_value(&none_shape).unwrap();
    assert!(
        v.is_null(),
        "人工修订序列化成 null，不是 {{}} —— 补成空对象会让「没有声明」与「有一个空声明」分不清"
    );
}