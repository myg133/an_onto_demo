//! REQ-109 · **CLI 闸门**：无 key / 无预算 / 非法开关必须 `exit≠0` 并点名。
//!
//! 这三条**不需要网络、不需要数据库、不需要真 key** —— 它们是入口判据，
//! 在读 schema 之前就判完（AC-002 要的是「开口就说清」，不是「跑完才失败」）。
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-002 | 三件套缺任一 ⇒ exit≠0 且**点名缺哪个变量**；⛔ 不许静默降级 |
//! | AC-006 | `--llm` 缺 `--budget-tokens` / 给 0 ⇒ exit≠0 |
//!
//! 另有两条反向用例，防「把闸门写成一律失败」：
//! AI 关时**没有**任何 LLM 环境变量也必须能正常跑完；`--refresh` 只在 AI 开时有意义。

use std::path::{Path, PathBuf};

mod common;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_ontok-propose");

const LLM_ENVS: [&str; 3] = [
    "ONTOK_LLM_BASE_URL",
    "ONTOK_LLM_API_KEY",
    "ONTOK_LLM_MODEL",
];

fn manifest_dir() -> PathBuf {
    common::crate_root()
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// 跑一次：先清空三个 LLM 变量，再按 `envs` 注入。
fn run(args: &[String], envs: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(manifest_dir());
    for k in LLM_ENVS {
        cmd.env_remove(k);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.args(args);
    let out = cmd.output().expect("跑不动 ontok-propose 二进制");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn gate_args(out: &Path) -> Vec<String> {
    let m = manifest_dir();
    vec![
        "--schema".into(),
        m.join("fixtures/schema-req109.json").to_string_lossy().into_owned(),
        "--names".into(),
        m.join("src/extract/table_names.json").to_string_lossy().into_owned(),
        "--out".into(),
        out.join("proposal.json").to_string_lossy().into_owned(),
        "--stats".into(),
        out.join("stats.json").to_string_lossy().into_owned(),
        "--project".into(),
        "req109gate".into(),
        "--no-sample".into(),
    ]
}

fn out_dir(tag: &str) -> PathBuf {
    let p = manifest_dir().join("out").join("req109-gate").join(tag);
    std::fs::create_dir_all(&p).expect("建输出目录");
    p
}

/// **AC-002**：`--llm` 但三件套全缺 ⇒ 非 0 退出，且三个变量名都要出现在提示里。
#[test]
fn ac002_llm_without_any_env_names_all_three() {
    let d = out_dir("no-env");
    let mut a = gate_args(&d);
    a.extend(["--llm".into(), "--budget-tokens".into(), "1000".into()]);
    let r = run(&a, &[]);
    assert_ne!(r.code, 0, "缺凭据必须非 0 退出（R4：不许静默降级）");
    let all = r.all();
    for k in LLM_ENVS {
        assert!(all.contains(k), "错误信息必须点名 {k}：{all}");
    }
    assert!(
        all.contains("缺失") || all.contains("缺"),
        "要说清是「缺」而不是别的错：{all}"
    );
    // 产物一个都不许出（入口就拦了）
    assert!(!d.join("proposal.json").exists());
}

/// **AC-002**：只缺一个 ⇒ 点名**那一个**（不许把配好的也说成缺）。
#[test]
fn ac002_names_the_single_missing_variable() {
    let full = [
        ("ONTOK_LLM_BASE_URL", "http://127.0.0.1:1/v1"),
        ("ONTOK_LLM_API_KEY", "test-only-not-a-real-key"),
        ("ONTOK_LLM_MODEL", "fake-model-gate"),
    ];
    for missing in LLM_ENVS {
        let envs: Vec<(&str, &str)> = full
            .iter()
            .copied()
            .filter(|(k, _)| *k != missing)
            .collect();
        let d = out_dir(&format!("missing-{missing}"));
        let mut a = gate_args(&d);
        a.extend(["--llm".into(), "--budget-tokens".into(), "1000".into()]);
        let r = run(&a, &envs);
        assert_ne!(r.code, 0, "缺 {missing} 必须非 0 退出");
        let all = r.all();
        assert!(all.contains(missing), "错误信息必须点名 {missing}：{all}");
        for other in LLM_ENVS.iter().filter(|k| **k != missing) {
            assert!(
                !all.contains(other),
                "只缺 {missing} 时不该点名 {other}（那会把人引到错的变量上）：{all}"
            );
        }
    }
}

/// **AC-006**：`--llm` 缺预算 / 预算为 0 ⇒ 非 0 退出并点名开关。
#[test]
fn ac006_llm_requires_a_positive_budget() {
    let envs = [
        ("ONTOK_LLM_BASE_URL", "http://127.0.0.1:1/v1"),
        ("ONTOK_LLM_API_KEY", "test-only-not-a-real-key"),
        ("ONTOK_LLM_MODEL", "fake-model-gate"),
    ];
    for extra in [Vec::<String>::new(), vec!["--budget-tokens".into(), "0".into()]] {
        let d = out_dir(if extra.is_empty() { "no-budget" } else { "zero-budget" });
        let mut a = gate_args(&d);
        a.push("--llm".into());
        a.extend(extra);
        let r = run(&a, &envs);
        assert_ne!(r.code, 0, "AI 开时缺预算/0 预算必须非 0 退出");
        assert!(
            r.all().contains("--budget-tokens"),
            "要点名该给哪个开关：{}",
            r.all()
        );
        assert!(!d.join("proposal.json").exists());
    }
}

/// `--refresh` 只在 `--llm` 下有意义：AI 关时给了它必须报错，
/// 而不是「静默无操作」（那会让人以为刷新过）。
#[test]
fn refresh_without_llm_is_rejected() {
    let d = out_dir("refresh-no-llm");
    let mut a = gate_args(&d);
    a.push("--refresh".into());
    let r = run(&a, &[]);
    assert_ne!(r.code, 0, "--refresh 在 AI 关时必须拒绝");
    assert!(r.all().contains("--refresh"), "{}", r.all());
}

/// **反向用例**：AI 关 + 无任何 LLM 环境变量 ⇒ 必须正常跑完，
/// 且产物里**没有** `ai` / `semantic_fit`（AC-001 的字节口径）。
#[test]
fn ai_off_runs_clean_and_has_no_ai_fields() {
    let d = out_dir("ai-off");
    let a = gate_args(&d);
    let r = run(&a, &[]);
    assert_eq!(r.code, 0, "AI 关时不该被 LLM 闸门拦住：{}\n{}", r.stdout, r.stderr);

    for f in ["proposal.json", "stats.json", "proposal-pending.json", "proposal-rejected.json"] {
        let p = d.join(f);
        assert!(p.exists(), "AI 关也必须产出四份产物（缺 {f}）");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("\"ai\""), "{f} 里出现了 ai 键（AI 关必须逐字节同 REQ-108）");
        assert!(!text.contains("semantic_fit"), "{f} 里出现了 semantic_fit");
        assert!(!text.contains("evidence-scorecard-v2"), "{f} 里出现了 v2 方法名");
    }

    // 每条带分数的条目，method 都必须是 v1
    let pending: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join("proposal-pending.json")).unwrap())
            .unwrap();
    let mut n = 0;
    for it in pending["items"].as_array().unwrap() {
        if let Some(c) = it.get("confidence") {
            assert_eq!(c["method"], "evidence-scorecard-v1");
            n += 1;
        }
    }
    assert!(n > 0, "这份 fixture 至少要有一条关系条目，否则本用例是空的");
}

/// **AC-001 的进程级补充**：LLM 环境变量**存在但 AI 没开**时，产物与「环境全空」
/// 逐字节相同 —— 开不开 AI 只由 `--llm` 决定，不由环境变量决定。
#[test]
fn ai_off_is_not_affected_by_present_llm_env() {
    let a_dir = out_dir("env-present");
    let b_dir = out_dir("env-absent");
    let ra = run(&gate_args(&a_dir), &[
        ("ONTOK_LLM_BASE_URL", "http://127.0.0.1:1/v1"),
        ("ONTOK_LLM_API_KEY", "test-only-not-a-real-key"),
        ("ONTOK_LLM_MODEL", "fake-model-gate"),
    ]);
    let rb = run(&gate_args(&b_dir), &[]);
    assert_eq!(ra.code, 0);
    assert_eq!(rb.code, 0);
    for f in ["proposal.json", "stats.json", "proposal-pending.json", "proposal-rejected.json"] {
        let a = std::fs::read(a_dir.join(f)).expect("读第一份产物");
        let b = std::fs::read(b_dir.join(f)).expect("读第二份产物");
        assert_eq!(
            ontok::extract::sample::sha256_hex(&a),
            ontok::extract::sample::sha256_hex(&b),
            "{f} 在「环境变量存在但不加 --llm」时变了 —— 开不开 AI 只能由开关决定"
        );
    }
}
