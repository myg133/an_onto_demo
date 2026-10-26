//! REQ-022 复审 F1 · **成本闸门的报告不许说假话**
//!
//! # 这个文件钉住什么
//!
//! 复审实测（真实 91 表 schema + `--budget-tokens 0` + 当前 inproc agent）发现：
//! 一次**完整可用**的提案被 CLI 判成「部分的」，并被建议「不要当完整提案 import /
//! 放宽预算重抽」—— 与成本闸门的目的正好相反。
//!
//! 根因是**措辞无条件**：`ontok-propose` 只看 `stage.budget.stopped` 就打
//! 「本轮提案是**部分**的」，而 `stopped` 与「items 是否残缺」在两条组装路径上
//! **关系不同**：
//!
//! | 组装路径 | items 来自 | 越闸后 items 残缺吗 |
//! |---|---|---|
//! | `RebuiltFromCandidates` | **只由跑完那些组**的候选重建 | **是** —— 未跑的组没进候选 |
//! | `RulePipeline` | `extract_with_context(report, …)`，而 `report` 是**全量**勘查结果、**不受预算影响** | **否** —— 覆盖全部表 |
//!
//! 所以「提案是部分的」这句话**只在重建路径上为真**。
//!
//! ⚠ 「残缺」这个事实**编排层已经测过**（`test_agent_rebuild.rs` 那个预算用例
//! 用的 `UsageReportingAgent` 声明了 `CAPABILITY_REBUILDS_PROPOSAL` ⇒ 重建路径，
//! 断言「提案只含跑完的组」**是对的**）。**缺的是 CLI 这层**——复审前
//! 「预算 + CLI 措辞」零覆盖，而零覆盖正是这句话能长期说假话的原因。
//!
//! # 判别力来自哪几处
//!
//! 1. **退出码仍非零**、**未跑的组仍被点名** —— 预算超限**就是**超限，
//!    这两条语义**不许**因为「提案其实完整」而被顺手改掉；
//! 2. 规则路径下 stdout **不含**「本轮提案是**部分**的」；
//! 3. 规则路径下 stderr **不含**「不要当完整提案 import」（那是**有害建议**：
//!    规则路径下放宽预算换不来更多 items）；
//! 4. **产物层面**：越闸跑出来的 `items` 与不设预算跑出来的**逐条相同** ——
//!    这一条让「不是部分的」成为**可被产物推翻的事实**，而不是一句措辞。

//! ⚠ 本文件**不连库**（第 1 轮 `ontok-propose` 完全不连库），所以不引 `mod common`。

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.08",
  "generated_at": "2026-10-08T10:00:00Z",
  "source": "req022b-test",
  "names": {
    "t_dev": "设备",
    "t_customer": "客户",
    "t_region": "地域"
  }
}"#;

/// 三张表 —— 够让预算在第 1 组之后就停下，又够让「覆盖全部表」可核对。
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
        "table_count": 3,
        "column_count": 5,
        "commented_column_count": 5,
        "foreign_key_count": 0,
        "introspected_at": "2026-10-08T10:00:00Z",
        "tables": [
            t("t_dev", "设备表", &[("id", "主键"), ("sn", "序列号"), ("customer_id", "客户ID")]),
            t("t_customer", "客户表", &[("id", "主键"), ("code", "客户编码")]),
            t("t_region", "地域表", &[("id", "主键")]),
        ]
    }))
    .unwrap()
}

fn workspace(tag: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req022")
        .join(format!("budget-truth-{}-{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

/// 跑一次 CLI（第 1 轮 ⇒ 不连库；所以本文件**不**依赖数据库）。
///
/// 返回 `(是否成功, stdout, stderr, 产物 items)`。
fn run_cli(tag: &str, extra: &[&str]) -> (bool, String, String, serde_json::Value) {
    let dir = workspace(tag);
    let sp = dir.join("schema.json");
    let np = dir.join("names.json");
    std::fs::write(&sp, schema_json()).unwrap();
    std::fs::write(&np, NAMES_JSON).unwrap();
    let out = dir.join("proposal.json");
    let stats = dir.join("stats.json");
    let o = propose_bin()
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
    let items = std::fs::read_to_string(&out)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("items").cloned())
        .unwrap_or(serde_json::Value::Null);
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
        items,
    )
}

/// ★ 规则路径 + 预算超限 ⇒ 报告**不许**说这份提案是残缺的。
///
/// `--budget-tokens 0` 配当前 inproc agent（它不声明 `reports_usage`，
/// 所以 `require_budget` 不拦）—— 与复审实测**同一条可达路径**。
#[test]
fn test_budget_exceeded_on_rule_path_does_not_call_a_complete_proposal_partial() {
    let (ok, stdout, stderr, items) = run_cli("rule", &["--budget-tokens", "0"]);

    // ① 预算超限**仍然**是超限：非零退出 + 点名未跑的组（语义不许被改掉）
    assert!(!ok, "预算超限必须非零退出（这是闸门存在的理由）");
    assert!(
        stdout.contains("预算超限"),
        "必须报出预算超限，实际 stdout：{stdout}"
    );
    assert!(
        stdout.contains("未跑") && stdout.contains("不是抽取失败"),
        "未跑的组必须被点名，且要与抽取失败分清，实际 stdout：{stdout}"
    );

    // ② 规则路径下**不许**说提案是部分的 —— 这就是复审发现的那句假话
    assert!(
        !stdout.contains("本轮提案是**部分**的"),
        "规则路径的 items 来自**全量报告**的规则管线，提案并不残缺 —— \
         说它「是部分的」就是报告说假话。实际 stdout：{stdout}"
    );

    // ③ 反过来要**明说**它完整，且给出可核对的覆盖面
    assert!(
        stdout.contains("不是部分"),
        "不许只是「不说假话」就完事 —— 必须正面说清这份提案能不能用，实际 stdout：{stdout}"
    );
    assert!(
        stdout.contains("3/3"),
        "要报出覆盖了几张表 / 共几张（那是「能不能用」的判据），实际 stdout：{stdout}"
    );

    // ④ stderr **不许**给有害建议
    assert!(
        !stderr.contains("不要当完整提案 import"),
        "规则路径下「放宽预算换不来更多 items」，劝人别 import 是**有害建议**，实际 stderr：{stderr}"
    );

    // ⑤ 产物层面：三条表**一条不少**（这是「不残缺」的事实，不只是措辞）
    let mut keys: Vec<String> = items
        .as_array()
        .expect("items 应是数组")
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .map(|i| i["payload"]["key"].as_str().unwrap_or_default().to_string())
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec!["t_customer", "t_dev", "t_region"],
        "越闸后 items 仍覆盖**全部三张表** —— 若这里少了表，stdout 那句「不是部分的」才是不实的"
    );
}

/// ★ 越闸跑出来的 items 与不设预算跑出来的**逐条相同**（规则路径）。
///
/// 这条让「提案不残缺」成为**能被产物推翻的事实**：把 stdout 那句措辞改回
/// 「本轮提案是部分的」，只要这个等式还成立，就说明**是措辞在说谎**。
#[test]
fn test_budget_exceeded_leaves_rule_path_items_identical_to_an_unbudgeted_run() {
    let (_, stdout_budget, _, items_budget) = run_cli("cmp-budget", &["--budget-tokens", "0"]);
    assert!(
        stdout_budget.contains("预算超限"),
        "对照组前提：带预算那次必须真的越闸了，实际 stdout：{stdout_budget}"
    );
    let (ok_plain, _, _, items_plain) = run_cli("cmp-plain", &[]);
    assert!(ok_plain, "不设预算那次必须成功退出（对照组）");

    assert_eq!(
        items_budget, items_plain,
        "规则路径下越闸**没有**改变 items —— 预算只截断了阶段 1 的 agent 抽取，\
         而 items 压根不看预算。两者若不等，那「提案是部分的」这句话反而是对的，\
         CLI 的措辞就该按产物事实重新分叉。"
    );
}

/// 预算**充足**时不许有任何「停」的痕迹 —— 别把闸门写成永远报警
/// （否则「报告超限」这句话本身就失去了信息量）。
#[test]
fn test_no_budget_configured_never_claims_the_proposal_is_partial() {
    let (ok, stdout, _, items) = run_cli("nobudget", &[]);
    assert!(ok, "不设预算不该失败");
    assert!(
        !stdout.contains("预算超限") && !stdout.contains("本轮提案是"),
        "没设预算就不该有任何关于「部分/超限」的话（闸门不许永远报警），实际 stdout：{stdout}"
    );
    // 对照组：三条实体都在（关系条数不钉 —— `*_id` 推断与本文件要证的
    // 事情无关，钉它只会让本文件被另一条规则的变化带着走）
    let mut keys: Vec<String> = items
        .as_array()
        .expect("items 应是数组")
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .map(|i| i["payload"]["key"].as_str().unwrap_or_default().to_string())
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["t_customer", "t_dev", "t_region"], "对照组：三条表一条不少");
}

/// REQ-152：构建面命令已并入 **`ontok-builder`**（子命令 `propose`）。
///
/// 参数、产物与退出码一个字节都没变 —— 只是二进制名字变了、多一层子命令。
/// 返回 `Command` 而不是 `&str`: 调用点大多是 `Command::new(..).arg(..)` 的链，
/// 换成字符串就得在每个调用点各插一行 `.arg("propose")`（漏一个就红得莫名其妙）。
fn propose_bin() -> std::process::Command {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"));
    c.arg("propose");
    c
}
