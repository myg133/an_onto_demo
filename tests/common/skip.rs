//! REQ-131 · 夹具缺失时的**响亮跳过**（默认）与**可强制失败**（CI）。
//!
//! # 为什么有这个模块
//!
//! 「现场判据」（REQ-123 / 124 / 126 / 127 / 129 的真库用例与真实产物判据）在**没有夹具**
//! 的机器上只能提前返回。原来的写法是 `missing(..).unwrap()`：它确实**打印**了
//! `*** SKIP ***`，但 `unwrap()` 对 `None` **panic** ⇒ 用例 **FAILED**。
//!
//! | 现象 | 改前 | 改后（默认） | 改后（`ONTOK_REQUIRE_FIXTURES=1`） |
//! | --- | --- | --- | --- |
//! | 缺夹具时的结果 | ❌ FAILED（**假红**） | ✅ 正常返回（exit 0） | ❌ FAILED（**故意的**） |
//! | 输出里看得见吗 | 看得见 | 看得见（固定标记） | 看得见（失败原文） |
//! | 会假绿吗 | 不会 | **不会** | 不会 |
//!
//! ⚠️ 关键语义：**缺夹具不是通过**。默认行为只是把「没跑」变成**可数的一行**，
//! 而不是把它变成绿 —— 标记里明写「这条判据没有执行，不是通过」。
//!
//! # 两种行为
//!
//! ```text
//! # ① 默认：响亮跳过，退出码 0
//! cargo test --test test_req129_doc_merge
//! *** SKIP: ac006_... -- 缺 ONTOK_TEST_DATABASE_URL -- 这条判据没有执行，不是通过 ***
//!
//! # ② CI：同样的缺失 ⇒ 失败（退出码非 0）
//! $env:ONTOK_REQUIRE_FIXTURES="1"; cargo test --test test_req129_doc_merge
//! *** FAIL: ac006_... -- 缺 ONTOK_TEST_DATABASE_URL -- ONTOK_REQUIRE_FIXTURES=1 —— CI 要求夹具必须存在 ***
//! thread '...' panicked at ...: ONTOK_REQUIRE_FIXTURES=1：CI 要求夹具必须存在（...）
//! ```
//!
//! CI 设 `ONTOK_REQUIRE_FIXTURES=1`，跳过就**再也混不过去**（需求 R2）；
//! 本地不设 ⇒ 没夹具的机器上跑全量**不会多出假红**，真正的红灯不被掩盖（需求 R1）。
//!
//! # 计数（AC-001 / AC-005）
//!
//! 前缀固定为 [`SKIP_PREFIX`]，且**一条用例一行**：
//!
//! ```text
//! cargo test 2>&1 | Select-String -SimpleMatch '*** SKIP:'   # PowerShell
//! cargo test 2>&1 | grep -c '^\*\*\* SKIP:'                  # bash
//! ```
//!
//! ⚠️ 为什么直接写 **fd 2** 而不是 `eprintln!`：libtest 会捕获测试线程上的
//! `eprintln!`，默认 `cargo test` **只在失败时**打印那部分 —— 跳过必须**看得见**
//! （REQ-122 · N2 的既有口径，与本仓 `test_req12x_live.rs` 一致）。

use std::io::Write as _;

/// 跳过标记的固定前缀（CI 用它数「本次跳过了几条」）。
pub const SKIP_PREFIX: &str = "*** SKIP:";

/// 标记里必须出现的那句话 —— 免得有人把「跳过」读成「通过」。
pub const NOT_EXECUTED: &str = "这条判据没有执行，不是通过";

/// 强制开关：设成 [`TRUTHY`] 里的任一写法 ⇒ 缺夹具 **失败**。
pub const REQUIRE_ENV: &str = "ONTOK_REQUIRE_FIXTURES";

/// 强制失败时的说明原文（AC-002 要它出现在错误信息里）。
pub const CI_REASON: &str = "CI 要求夹具必须存在";

/// 视为「开」的取值（大小写不敏感、两侧空白忽略）。其余一律算「关」。
const TRUTHY: [&str; 4] = ["1", "true", "yes", "on"];

/// 纯函数：给定环境变量的**原始取值**，判断要不要强制失败。
///
/// 抽成纯函数是为了让 `tests/test_req131_fixture_skip.rs` 能直接判它，
/// 而不必每个取值都起一个子进程。
/// `None` = 变量没设。
#[must_use]
pub fn is_required(value: Option<&str>) -> bool {
    match value {
        Some(v) => TRUTHY.contains(&v.trim().to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// 当前环境是否要求「夹具必须存在」。
#[must_use]
pub fn required() -> bool {
    is_required(std::env::var(REQUIRE_ENV).ok().as_deref())
}

/// 缺夹具时调用：**默认**打一行可 grep 的标记后返回（用例照常成功）；
/// `ONTOK_REQUIRE_FIXTURES=1` 时**失败**。
///
/// `name` = 缺的是什么（环境变量名 / 文件相对路径 / 「可连的 …（错误）」），
/// `test` = 用例名（与 `#[test] fn` 同名，便于 grep 定位）。
///
/// 调用方约定：调用后**立刻**从用例返回，⛔ 不要继续跑后续断言
/// （否则就是拿假夹具判真事 = 假绿）。
pub fn missing(name: &str, test: &str) {
    let mut err = std::io::stderr();

    if required() {
        // 先打一行**可 grep 的失败原文**：panic 的默认输出里带换行与位置，
        // 单行标记更便于 CI 汇总（`*** FAIL:` 与 `*** SKIP:` 不会混淆）。
        let _ = writeln!(
            err,
            "*** FAIL: {test} -- 缺 {name} -- {REQUIRE_ENV}=1 —— {CI_REASON} ***"
        );
        let _ = err.flush();
        panic!(
            "{REQUIRE_ENV}=1：{CI_REASON}（用例 {test} 缺 {name}）；\
             这条判据没有执行，不能当作通过。缺夹具时本地可以跳过，CI 不许。"
        );
    }

    let _ = writeln!(
        err,
        "{SKIP_PREFIX} {test} -- 缺 {name} -- {NOT_EXECUTED} ***"
    );
    let _ = err.flush();
}

// ──────────────────────────────────────────────────────────
// 自检（本模块自身的行为判据住在 tests/test_req131_fixture_skip.rs：
// 那里用**子进程**跑一个「永远缺夹具」的用例，因此能看到真实的退出码）
// ──────────────────────────────────────────────────────────
