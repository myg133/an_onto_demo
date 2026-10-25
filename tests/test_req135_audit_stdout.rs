//! REQ-135 · **审计默认落 stdout**（AC-005 / AC-006）。
//!
//! # 为什么用「自己跑自己」的子进程
//!
//! Rust 的测试用例**捕获**自己进程的 stdout，用例内部断言不了「这一行真的到了
//! stdout」。所以这里把「写 20 行审计」放进一个 `#[ignore]` 的子用例，父用例
//! `Command::new(current_exe())` 把它当子进程跑（`--exact … --ignored --nocapture`），
//! 再读**子进程的 stdout** —— 那就是容器里 `kubectl logs` 会看到的东西。
//!
//! 两条判据（需求 §3 的表格）：
//!
//! | 情形 | 判据 |
//! | --- | --- |
//! | 不给 `--audit-out` | stdout 里 20 行 `audit ` |
//! | 给了 `--audit-out` | **文件 20 行 + stdout 20 行**（⛔ 不是只写文件） |
//!
//! AC-006：每行都能按 `k=v` 解析，字段齐全（`ts` / `token=sha256:` / `event` / `code`），
//! 且**没有 token 明文**。

use std::path::{Path, PathBuf};
use std::process::Command;

use ontok::query::audit::{sha256_hex, AuditRecord, AuditSink};

/// 审计里**永远不许出现**的明文（只出现它的 sha256 前缀）。
const PLAINTEXT: &str = "FAKE-AUDIT-TOKEN-0123456789abcdef";

const CHILD: &str = "child_writes_twenty_audit_lines";

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req135-audit-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 子进程：写 20 行审计（有 `ONTOK_AUDIT_CHILD_FILE` 时**同时**写文件）。
///
/// ⚠️ 本用例默认不跑（`#[ignore]`）：它是父用例的**被测样本**，不是独立判据。
#[test]
#[ignore = "由 audit_lines_go_to_stdout_* 作为子进程调用（--ignored --exact）"]
fn child_writes_twenty_audit_lines() {
    // 测试框架先打 `test <name> ... `（**不带换行**）；先自己换一行，
    // 否则第一行审计会长在框架的输出尾巴上，父进程按行读会少一行。
    println!();
    let sink = match std::env::var("ONTOK_AUDIT_CHILD_FILE") {
        Ok(p) if !p.trim().is_empty() => {
            AuditSink::open(Path::new(p.trim())).expect("子进程打开审计文件")
        }
        _ => AuditSink::stdout_only(),
    };
    let digest = format!("sha256:{}", &sha256_hex(PLAINTEXT.as_bytes())[..12]);
    for i in 0..20usize {
        sink.record(&AuditRecord {
            ts: "2026-10-24T00:00:00.000Z".into(),
            event: "sparql",
            proto: "rest",
            token_digest: Some(digest.clone()),
            tenant: Some("TENANT-A".into()),
            query_digest: Some("sha256:0011223344556677".into()),
            rows: Some(i),
            elapsed_ms: i as u128,
            // 一半成功、一半是**安全信号**（拒绝）。
            code: if i % 2 == 0 { "OK" } else { "TENANT_MISMATCH" },
            view: None,
            bytes: None,
            note: None,
            // REQ-145：本判据不涉及「半个答案」。
            pending: false,
        });
    }
}

/// 跑一次子进程，返回它**真的写到 stdout 的东西**（不是测试框架的捕获缓冲）。
fn run_child(file: Option<&Path>) -> String {
    let exe = std::env::current_exe().expect("当前测试二进制");
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", CHILD, "--ignored", "--nocapture", "--test-threads=1"])
        .env_remove("ONTOK_AUDIT_CHILD_FILE");
    if let Some(f) = file {
        cmd.env("ONTOK_AUDIT_CHILD_FILE", f);
    }
    let out = cmd.output().expect("跑不起子进程");
    assert!(
        out.status.success(),
        "子进程失败：\nSTDOUT:\n{}\nSTDERR:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn audit_lines(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|l| l.starts_with("audit "))
        .collect()
}

/// ⭐ AC-005 第一半：**不给** `--audit-out` ⇒ 20 行全部出现在 stdout。
#[test]
fn without_a_file_the_audit_goes_to_stdout() {
    let stdout = run_child(None);
    let lines = audit_lines(&stdout);
    assert_eq!(
        lines.len(),
        20,
        "不给 --audit-out 时 20 行审计必须都在 stdout：\n{stdout}"
    );
}

/// ⭐ AC-005 第二半：给了 `--audit-out` ⇒ **文件 20 行 且 stdout 20 行**。
///
/// 变异：把 `record_line` 改成「有文件就不打 stdout」⇒ 本用例红（需求明确禁止）。
#[test]
fn with_a_file_the_audit_goes_to_both_places() {
    let dir = temp_dir("both");
    let file = dir.join("audit.log");
    let stdout = run_child(Some(&file));

    let lines = audit_lines(&stdout);
    assert_eq!(
        lines.len(),
        20,
        "给了 --audit-out 也**必须**照样写 stdout（⛔ 不许只写文件）：\n{stdout}"
    );

    let on_disk = std::fs::read_to_string(&file).expect("读回审计文件");
    assert_eq!(
        on_disk.lines().count(),
        20,
        "文件里也必须是 20 行（追加写）：\n{on_disk}"
    );
    // 两边是**同一份字节**（同一行形状、同一个 sink）。
    assert_eq!(
        lines.join("\n"),
        on_disk.lines().collect::<Vec<_>>().join("\n"),
        "stdout 与文件的行必须逐行相同"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⭐ AC-006：20 行逐行按 `k=v` 解析，字段齐全；⛔ 无 token 明文。
#[test]
fn every_line_parses_and_carries_no_token_plaintext() {
    let stdout = run_child(None);
    let lines = audit_lines(&stdout);
    assert_eq!(lines.len(), 20, "{stdout}");
    for (i, line) in lines.iter().enumerate() {
        // ① 固定字段（顺序也是固定的：运维 grep 的口径）。
        for needle in [
            "audit ts=",
            "event=sparql",
            "proto=rest",
            "token=sha256:",
            "tenant=TENANT-A",
            "q=sha256:",
            "rows=",
            "elapsed_ms=",
            "code=",
        ] {
            assert!(line.contains(needle), "第 {i} 行缺 {needle:?}：{line}");
        }
        // ② 真的能按 k=v 解析（不是「看起来像」）。
        let parsed: Vec<(&str, &str)> = line
            .split_whitespace()
            .filter(|t| *t != "audit")
            .map(|t| {
                t.split_once('=')
                    .unwrap_or_else(|| panic!("第 {i} 行有非 k=v 的记号：{t:?} —— {line}"))
            })
            .collect();
        let get = |k: &str| {
            parsed
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| *v)
                .unwrap_or_else(|| panic!("第 {i} 行缺 {k}：{line}"))
        };
        assert!(get("ts").starts_with("2026-10-24T"), "{line}");
        assert!(get("token").starts_with("sha256:"), "{line}");
        assert_eq!(get("token").len(), "sha256:".len() + 12, "{line}");
        assert!(matches!(get("code"), "OK" | "TENANT_MISMATCH"), "{line}");
        assert_eq!(get("rows"), i.to_string(), "行数必须来自入参：{line}");
        // ③ ⛔ 无明文。
        assert!(
            !line.contains(PLAINTEXT),
            "第 {i} 行出现了 token 明文：{line}"
        );
    }
    // 拒绝那一半真的存在（否则「安全信号」只是措辞）。
    assert!(
        lines.iter().any(|l| l.contains("code=TENANT_MISMATCH")),
        "样本里必须有拒绝行：{stdout}"
    );
}
