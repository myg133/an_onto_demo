//! REQ-131 · 缺夹具时的**响亮跳过**：本需求自己的判据。
//!
//! 被测物是 [`common::skip`]（`tests/common/skip.rs`）。三条行为各一条判据：
//!
//! | 判据 | 断言什么 |
//! | --- | --- |
//! | AC-001 | 缺夹具 ⇒ 子进程**退出码 0**，且 fd 2 上有一行固定标记：用例名 + 缺什么 + 「这条判据没有执行，不是通过」 |
//! | AC-002 | `ONTOK_REQUIRE_FIXTURES=1` ⇒ 同一情形**退出码非 0**，且错误信息说明「CI 要求夹具必须存在」 |
//! | 回归（AC-003 的另一面） | 全仓 `tests/` 里**一条** `missing(..).unwrap()` 都不许留 —— `.unwrap()` 对 `None` panic 正是改前的病 |
//!
//! # 为什么用**子进程**
//!
//! 「退出码」只能从**进程**那一层看：在同一个测试进程里 panic 会把整轮测试打红，
//! 而 AC-001 要的恰恰是「这条用例自己**不失败**」。⇒ 用一个**只在子进程里才真的跑**的
//! 牺牲用例（[`zz_fixture_missing_victim`]）+ `std::env::current_exe()` 重入自己，
//! 这样两条 AC 都拿**真实退出码**判，而不是拿「函数返回了什么」猜。
//!
//! ⚠️ 本文件的判据在 CI（`ONTOK_REQUIRE_FIXTURES=1`）下**照样绿**：
//! 子进程的环境是被显式摆出来的（AC-001 那条会 `env_remove`），
//! 所以「CI 强制夹具」不会把这两条自检判红 —— 那是两件事。

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// 子进程里才会真的跑那条「缺夹具」的用例。
///
/// 不这么隔离的话，本地全量会多出一条**合成的** SKIP（它不是在测什么东西缺了），
/// 把「可数」这件事本身搅浑。
const VICTIM_ENV: &str = "ONTOK_REQ131_SELF_CHECK";

/// 牺牲用例名（也是 `--exact` 的过滤器，与 `#[test] fn` 同名）。
const VICTIM: &str = "zz_fixture_missing_victim";

/// 一个**永远不存在**的夹具路径（这是判据的一部分，⛔ 不是待补的 TODO）。
const ABSENT: &str = "tests/fixtures/req131/this-fixture-must-never-exist.json";

/// 在子进程里跑 [`VICTIM`]；`require` = 要不要设 `ONTOK_REQUIRE_FIXTURES`。
fn run_victim(require: Option<&str>) -> Output {
    let exe = std::env::current_exe().expect("拿不到当前测试二进制");
    let mut cmd = Command::new(exe);
    cmd.arg(VICTIM).arg("--exact").arg("--nocapture");
    cmd.env(VICTIM_ENV, "1");
    match require {
        Some(v) => {
            cmd.env(common::skip::REQUIRE_ENV, v);
        }
        None => {
            // ⚠️ 显式清掉：CI 设了它，但 AC-001 判的是「默认」那一档。
            cmd.env_remove(common::skip::REQUIRE_ENV);
        }
    }
    cmd.output().expect("跑不动子进程")
}

fn stderr_of(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// 跑到这里就算完成 —— 子进程里的「缺夹具」用例，**故意**什么都拿不到。
#[test]
fn zz_fixture_missing_victim() {
    if std::env::var(VICTIM_ENV).as_deref() != Ok("1") {
        return; // 只在自检子进程里才有意义
    }
    common::skip::missing(ABSENT, VICTIM);
}

/// **AC-001**：缺夹具 ⇒ 响亮跳过、**不失败**（exit 0），且标记可 grep、可数。
#[test]
fn ac001_missing_fixture_skips_loudly_and_does_not_fail() {
    let o = run_victim(None);
    let err = stderr_of(&o);
    assert!(
        o.status.success(),
        "缺夹具默认必须**成功返回**（exit 0，AC-001）—— 实际 {:?}：\n{err}",
        o.status
    );

    let marks: Vec<&str> = err
        .lines()
        .filter(|l| l.starts_with(common::skip::SKIP_PREFIX))
        .collect();
    assert_eq!(
        marks.len(),
        1,
        "一条用例恰好一行 `{}` 标记（多一行/少一行都数不准）：\n{err}",
        common::skip::SKIP_PREFIX
    );
    let line = marks[0];
    assert!(
        line.contains(VICTIM),
        "标记里必须有**用例名**（不然数出来的是谁？）：{line}"
    );
    assert!(
        line.contains("this-fixture-must-never-exist.json"),
        "标记里必须说**缺什么**：{line}"
    );
    assert!(
        line.contains(common::skip::NOT_EXECUTED),
        "标记里必须写明「{}」（跳过 ≠ 通过）：{line}",
        common::skip::NOT_EXECUTED
    );
}

/// **AC-002**：`ONTOK_REQUIRE_FIXTURES=1` ⇒ 同一缺失变成**失败**（exit != 0）。
#[test]
fn ac002_require_fixtures_env_turns_the_same_skip_into_a_failure() {
    let o = run_victim(Some("1"));
    let err = stderr_of(&o);
    assert!(
        !o.status.success(),
        "{}={:?} 时缺夹具必须**失败**（AC-002）—— 实际 {:?}：\n{err}",
        common::skip::REQUIRE_ENV,
        "1",
        o.status
    );
    assert!(
        err.contains(common::skip::CI_REASON),
        "错误信息必须说明「{}」（不然 CI 上的人只看见 panic）：\n{err}",
        common::skip::CI_REASON
    );
    assert!(
        err.contains(VICTIM),
        "错误信息必须点名是用例 {VICTIM}：\n{err}"
    );
    assert!(
        !err.lines()
            .any(|l| l.starts_with(common::skip::SKIP_PREFIX)),
        "强制模式下**不许**再打 SKIP 标记（它不是跳过，是失败）：\n{err}"
    );
}

/// **AC-002（开关语义）**：默认关；`0` / `false` / 空 / 未设 都是关；只有那几个开值算开。
#[test]
fn ac002_require_switch_is_off_by_default_and_off_for_falsy_values() {
    assert!(common::skip::is_required(Some("1")));
    assert!(
        common::skip::is_required(Some(" 1 ")),
        "两侧空白不该让开关失灵（`.env` 文件里常带空格）"
    );
    assert!(common::skip::is_required(Some("TRUE")));
    for off in [None, Some(""), Some("  "), Some("0"), Some("false"), Some("no")] {
        assert!(
            !common::skip::is_required(off),
            "{off:?} 不该打开强制开关 —— 「没设」与「设成 0」都是本地模式"
        );
    }

    // `required()` 只许由环境变量决定（⛔ 不许在这条判据里写死期望值：
    // CI 会真的设它，写死就把 CI 判红了）。
    let raw = std::env::var(common::skip::REQUIRE_ENV).ok();
    assert_eq!(
        common::skip::required(),
        common::skip::is_required(raw.as_deref()),
        "required() 必须等价于 is_required(环境变量)"
    );
}

/// **回归判据**（AC-003 的另一面）：`tests/` 下**不许**再有 `missing(..).unwrap()`。
///
/// 那正是改前的病：`missing()` 返回 `None` 表示「缺夹具」，`.unwrap()` 把它变成
/// panic ⇒ 缺夹具的机器上**多出假红**。谁把这条写法写回来，这里立刻红。
///
/// ⚠️ 跳过本文件自己：它带着**反向样本**（`has_unwrap_on_missing` 的自检串），
/// 扫自己必假红。扫别人不受影响。
#[test]
fn n1_no_test_still_unwraps_a_missing_fixture() {
    // ① 自检：判据先证明自己认得出病，也认得出「长得像但不是」的形态。
    assert!(
        has_unwrap_on_missing("return missing(\"X\", T).unwrap();"),
        "🔴 判据空转：正样本都认不出来"
    );
    assert!(
        !has_unwrap_on_missing("let v = name_missing(1).unwrap();"),
        "🔴 误报：`*_missing(` 是别的函数，不是缺夹具"
    );
    assert!(
        !has_unwrap_on_missing("let v = missing_count().unwrap();"),
        "🔴 误报：`missing_count(` 也不是"
    );
    assert!(
        !has_unwrap_on_missing("missing(\"X\", T); let v = other().unwrap();"),
        "🔴 误报：`.unwrap()` 不在 `missing(..)` 上"
    );

    // ② 实扫
    let root = common::crate_root().join("tests");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(!files.is_empty(), "没扫到任何 tests/**/*.rs，判据就是空转");

    let mut offenders = Vec::new();
    for f in &files {
        if f.file_name().is_some_and(|n| n == "test_req131_fixture_skip.rs") {
            continue; // 自带反向样本，见 doc
        }
        let Ok(raw) = std::fs::read_to_string(f) else {
            continue;
        };
        let stripped = common::strip_comments(&raw);
        for (i, line) in stripped.lines().enumerate() {
            if has_unwrap_on_missing(line) {
                let rel = f
                    .strip_prefix(&root)
                    .unwrap_or(f.as_path())
                    .to_string_lossy()
                    .replace('\\', "/");
                offenders.push(format!("tests/{rel}:{}  {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "这些地方还在用 `missing(..).unwrap()` —— 缺夹具时会 panic = 假红（REQ-131）：\n  - {}",
        offenders.join("\n  - ")
    );
}

/// 该行**代码**里是否出现「对 `missing(..)` 的 `.unwrap()`」。
///
/// 只认**标识符边界**后的 `missing(`（`name_missing(` / `missing_count(` 不算），
/// 且 `.unwrap()` 必须紧跟在**这次调用**的收尾 `)` 之后（数括号深度定位，
/// 不用正则 —— 参数里本来就有括号）。
fn has_unwrap_on_missing(code_line: &str) -> bool {
    const NEEDLE: &str = "missing(";
    let mut search = 0usize;
    while let Some(rel) = code_line[search..].find(NEEDLE) {
        let at = search + rel;
        search = at + NEEDLE.len();
        let prev_is_ident = code_line[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if prev_is_ident {
            continue;
        }
        // `missing(` 的左括号：数到与它配对的 `)` 为止。
        let open = at + NEEDLE.len() - 1;
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in code_line[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end) = close {
            if code_line[end..].trim_start().starts_with(".unwrap()") {
                return true;
            }
        }
    }
    false
}

/// 递归收集 `tests/` 下的 `.rs`（稳定顺序 ⇒ 报错可复现）。
fn collect_rs(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            collect_rs(&p, acc);
        } else if p.extension().is_some_and(|e| e == "rs") {
            acc.push(p);
        }
    }
}
