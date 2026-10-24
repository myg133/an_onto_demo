//! REQ-128 · `ontok-propose --names` **必填** + 占位表「警告但不拒绝」。
//!
//! # 这个文件钉住什么
//!
//! `--names` 过去有一个默认值 `src/extract/table_names.json`，而那份文件是
//! **脱敏占位样例**（91 条 `t001 → 对象 001`）。两种后果都是**静默白跑**：
//!
//! | 场景 | 过去 | 现在 |
//! | --- | --- | --- |
//! | 容器里不传 | 去 `/data/src/extract/...` 找 ⇒ 文件不存在 | 启动即失败（可行动的错误） |
//! | 仓库根不传 | 读到占位表 ⇒ 实体名全成「对象 001」，**不报错** | 启动即失败（同上） |
//! | 显式传占位表 | 照跑，没人知道 | 照跑，但 stderr 告警 |
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | 不传 `--names` ⇒ 退出码非 0；stderr 含「必须显式提供翻译表」、脱敏样例说明、两条生成命令 |
//! | AC-002 | 显式传 `src/extract/table_names.json` ⇒ 退出码 0 + 产物落盘（**确实跑完**），stderr 有占位警告 |
//! | AC-003 | 传一份正常翻译表 ⇒ 退出码 0，且 stderr **没有**占位警告 |
//!
//! ⚠ 本文件**不连库**：三条判据都落在第 1 轮（第 1 轮 `ontok-propose` 完全不连库）。
//! ⚠ 临时目录落在仓内 `out/`（`.gitignore` 已覆盖），不往 `../.review-workspace` 写。

use std::path::PathBuf;
use std::process::Command;

mod common;

const BIN: &str = env!("CARGO_BIN_EXE_ontok-propose");

/// 一份**正常**翻译表：`fixtures/schema-req109.json` 的 3 张表都有中文名。
const REAL_NAMES: &str = r#"{
  "agent": "ontok-translate-names",
  "version": "1.0.0",
  "generated_at": "2026-10-24T00:00:00Z",
  "source": "fixtures/schema-req109.json (3 tables)",
  "names": {
    "llm_model": "大模型",
    "llm_model_sort": "模型排序",
    "orders": "订单"
  }
}"#;

fn crate_root() -> PathBuf {
    // ⚠️ 走**运行时**锚点：裸 `env!("CARGO_MANIFEST_DIR")` 是 `tests/test_anchor_guard.rs`
    // G-1 明令禁止的写法（只许出现在 `tests/common/source_root.rs`）。
    // 🔖 这处**不属于 REQ-131**：它是 REQ-131 修掉假红之后**暴露出来的既有红**
    // （基线 `1772507` 上 `test_anchor_guard` 就是红的），按 BA 裁定在本分支顺手修掉。
    common::crate_root()
}

fn schema() -> PathBuf {
    crate_root().join("fixtures/schema-req109.json")
}

/// 仓内那份 91 条的脱敏样本（`t001 → 对象 001`）。
fn placeholder_names() -> PathBuf {
    crate_root().join("src/extract/table_names.json")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = crate_root()
        .join("out")
        .join("req128")
        .join(format!("{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let out = Command::new(BIN)
        .current_dir(crate_root())
        .args(args)
        .output()
        .expect("跑不动 ontok-propose 二进制");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// **AC-001**：不传 `--names` ⇒ 失败，且错误要**可行动**（说清为什么 + 怎么生成）。
#[test]
fn ac001_without_names_exits_nonzero_with_actionable_error() {
    let d = scratch("missing-names");
    let out = d.join("proposal.json");
    let stats = d.join("stats.json");
    let r = run(&[
        "--schema",
        schema().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
        "--project",
        "req128-required",
    ]);

    assert_ne!(
        r.code, 0,
        "不传 --names 必须非 0 退出（退化成默认值就是**静默白跑**）：\nSTDOUT:\n{}\nSTDERR:\n{}",
        r.stdout, r.stderr
    );
    // 三样必须都在（AC-001 原文）：为什么必填 / 占位表是什么 / 怎么生成真实表。
    for needle in [
        "必须显式提供翻译表",
        "table_names.json 是脱敏占位样例",
        "ontok-translate-names",
        "ontok name suggest --as-names",
    ] {
        assert!(
            r.stderr.contains(needle),
            "stderr 必须含 {needle:?}（可行动的错误信息），实际：\n{}",
            r.stderr
        );
    }
    // 没读完输入就失败 ⇒ 不许留下半份产物。
    assert!(
        !out.exists(),
        "缺 --names 时不许产出提案 —— 半个产出比没有产出更危险"
    );
    assert!(!stats.exists(), "缺 --names 时不许产出统计");
}

/// **AC-002**：显式传占位表 ⇒ **警告，但继续跑完**（不能因为警告而中止）。
#[test]
fn ac002_placeholder_names_warns_but_still_completes() {
    let d = scratch("placeholder");
    let out = d.join("proposal.json");
    let stats = d.join("stats.json");
    let r = run(&[
        "--schema",
        schema().to_str().unwrap(),
        "--names",
        placeholder_names().to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
        "--project",
        "req128-placeholder",
        "--no-sample",
    ]);

    assert_eq!(
        r.code, 0,
        "占位表只能**警告**，不能拒绝（本地测试需要它）：\nSTDOUT:\n{}\nSTDERR:\n{}",
        r.stdout, r.stderr
    );
    assert!(
        r.stderr.contains("脱敏占位样例") && r.stderr.contains("这会让实体名全是占位符"),
        "必须在 stderr 说清「这会让实体名全是占位符」，实际：\n{}",
        r.stderr
    );

    // 「确实跑完了」不能只看退出码：产物要落盘，统计要覆盖全量 3 张表。
    assert!(out.exists(), "警告之后必须照常写出提案");
    assert!(stats.exists(), "警告之后必须照常写出统计");
    let sv: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&stats).unwrap()).unwrap();
    assert_eq!(
        sv["tables_seen"], 3,
        "占位表警告不许截断运行：tables_seen 应覆盖全量 schema，实际统计：{sv}"
    );
}

/// **AC-003**：正常翻译表 ⇒ 没有占位警告（告警不能写成「永远告警」）。
#[test]
fn ac003_real_names_table_has_no_placeholder_warning() {
    let d = scratch("real-names");
    let names = d.join("names.json");
    std::fs::write(&names, REAL_NAMES).unwrap();
    let out = d.join("proposal.json");
    let stats = d.join("stats.json");
    let r = run(&[
        "--schema",
        schema().to_str().unwrap(),
        "--names",
        names.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--stats",
        stats.to_str().unwrap(),
        "--project",
        "req128-real",
        "--no-sample",
    ]);

    assert_eq!(
        r.code, 0,
        "正常翻译表必须跑成功：\nSTDOUT:\n{}\nSTDERR:\n{}",
        r.stdout, r.stderr
    );
    assert!(
        !r.stderr.contains("脱敏占位样例") && !r.stderr.contains("占位符"),
        "正常翻译表不该出现占位警告（否则告警恒真、失去信息量），实际：\n{}",
        r.stderr
    );
    assert!(out.exists() && stats.exists(), "成功运行必须落盘提案与统计");
}

/// **AC-002 的边界**：规则是**严格大于 80%**（80% 不告警，81% 才告警）。
///
/// 只测 100% / 0% 抓不到「阈值被改成 `>= 80%` 或 50%」这类变异，所以这里把
/// 边界两侧各钉一条：5 条里 4 条占位（80%）⇒ 静默；6 条里 5 条占位（83%）⇒ 告警。
/// 检测只看翻译表自身的条目分布，与 schema 覆盖率无关，所以夹具可以不带这 3 张表。
#[test]
fn ac002_threshold_is_strictly_above_80_percent() {
    let make_names = |placeholder_count: usize, total: usize| -> String {
        let mut names = serde_json::Map::new();
        for i in 1..=placeholder_count {
            names.insert(format!("t{i:03}"), serde_json::Value::from(format!("对象 {i:03}")));
        }
        for i in placeholder_count + 1..=total {
            names.insert(format!("real_table_{i}"), serde_json::Value::from(format!("真实表 {i}")));
        }
        serde_json::json!({
            "agent": "req128-threshold",
            "version": "1.0.0",
            "generated_at": "2026-10-24T00:00:00Z",
            "source": "req128-threshold-test",
            "names": names,
        })
        .to_string()
    };

    // 4/5 = 80% ⇒ **不**告警（判据是 `> 80%`，不是 `>=`）。
    let d80 = scratch("threshold-80");
    let n80 = d80.join("names.json");
    std::fs::write(&n80, make_names(4, 5)).unwrap();
    let r80 = run(&[
        "--schema",
        schema().to_str().unwrap(),
        "--names",
        n80.to_str().unwrap(),
        "--out",
        d80.join("proposal.json").to_str().unwrap(),
        "--stats",
        d80.join("stats.json").to_str().unwrap(),
        "--no-sample",
    ]);
    assert_eq!(r80.code, 0, "80% 只是不告警，不是失败：\n{}", r80.stderr);
    assert!(
        !r80.stderr.contains("脱敏占位样例"),
        "4/5 = 80% **不**该告警（规则是严格 > 80%），实际：\n{}",
        r80.stderr
    );

    // 5/6 ≈ 83% > 80% ⇒ 必须告警。
    let d83 = scratch("threshold-83");
    let n83 = d83.join("names.json");
    std::fs::write(&n83, make_names(5, 6)).unwrap();
    let r83 = run(&[
        "--schema",
        schema().to_str().unwrap(),
        "--names",
        n83.to_str().unwrap(),
        "--out",
        d83.join("proposal.json").to_str().unwrap(),
        "--stats",
        d83.join("stats.json").to_str().unwrap(),
        "--no-sample",
    ]);
    assert_eq!(r83.code, 0, "告警不等于拒绝：\n{}", r83.stderr);
    assert!(
        r83.stderr.contains("脱敏占位样例") && r83.stderr.contains("占位符"),
        "5/6 ≈ 83% > 80% 必须告警，实际：\n{}",
        r83.stderr
    );
}
