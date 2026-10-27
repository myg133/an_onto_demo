//! REQ-159 **P1c-2b** · Studio 节点着色「两种模式」的**动态**判据（Node + 假 DOM）。
//!
//! # 为什么这批判据要在 Node 里跑
//!
//! 着色是**纯前端**行为：`deg` 分位数怎么切、图例写什么、切换时有没有重建图，
//! 这些在 Rust 里一条也测不到（Rust 只 `include_str!` 了那三个文件）。本机没有
//! headless 浏览器，所以用 `tests/fixtures/req159e/color_probe.js` 的最小白 DOM 桩
//! 把 `src/studio/studio.js` **原样**跑起来 —— 验的是线上那一份代码。
//!
//! # 覆盖
//!
//! | 判据 | 谁验 |
//! | --- | --- |
//! | AC-e1 | 默认「可查性」、切完进 `sessionStorage`、URL 一个字节没动（`search/hash` 的 setter 直接抛） |
//! | AC-e2 | 切到中心度后节点色 ≥2 种且**真的变了**，图例换成中心度那套 |
//! | AC-e3 | 已知 `deg` 分布（0..9 各两个）⇒ P50=4 / P80=7 / P95=9，六个边界节点逐一 exact |
//! | AC-e4 | 图例跟着模式变：中心度图例里 ⛔ 没「需取数 / API 源 / 库源」，可查性图例里 ⛔ 没「枢纽」 |
//! | AC-e5 | 切换⛔ 不重建图：reload 0 / g-root 新建 0 / node-g 新建 0 / 节点对象全等 / cx 偏移 0 |
//! | AC-e7 | 模式 A 的三态三色逐字未变（琥珀 / 蓝 / 灰） |
//!
//! 静态那一半（不许出现绝对阈值、图例两套不许串味、`applyColorMode` 不许建图）
//! 在 `src/studio/mod.rs` 的 `tests` 里 —— 那几条是 `cargo test --lib studio` 的一部分。
//!
//! ⚠️ 本文件不经手数据库，也不碰 `src/console/**`；`node` 不在就是**显式 SKIP**
//! （⛔ 不静默当作通过）。

mod common;

use std::process::Command;

fn crate_root() -> std::path::PathBuf {
    common::source_root::crate_root()
}

/// 探针脚本（跑的是**工作区里**那份 `studio.js`，不是编译期快照）。
fn probe_js() -> std::path::PathBuf {
    crate_root().join("tests/fixtures/req159e/color_probe.js")
}

/// Node 在不在？不在就**显式跳过**并说清原因（⛔ 不把它伪装成通过）。
fn require_node() -> bool {
    match Command::new("node").arg("--version").output() {
        Ok(o) if o.status.success() => true,
        _ => {
            eprintln!(
                "SKIP：本用例需要 `node`（探针用 Node + 假 DOM 跑 studio.js）。\
                 它不是本 crate 的依赖，只在测试期用；装不上就跳过，⛔ 不把它伪装成通过。"
            );
            false
        }
    }
}

/// **AC-e2 / e3 / e4 / e5 / e7** · 探针必须退出 0，且关键结论行真的打出来了。
///
/// 反空转：只断言「退出 0」的话，把探针改成「打印一句 OK 就 exit(0)」也能骗过去。
/// 所以这里另外钉四条**输出内容**（阈值、图例标题、reload/布局计数、全过行）。
#[test]
fn the_two_color_modes_behave_like_the_acceptance_criteria() {
    if !require_node() {
        return;
    }
    let path = probe_js();
    assert!(path.is_file(), "探针不在：{}", path.display());

    let out = Command::new("node")
        .arg(&path)
        .output()
        .expect("起 node 失败（装了 node 但起不来？）");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    /* 不让它只在失败时可见：`cargo test -- --nocapture` 能看到逐条断言 */
    println!("{stdout}");

    assert!(
        out.status.success(),
        "探针失败（退出码 {:?}）：\n{stdout}\n{stderr}",
        out.status.code()
    );
    for needle in [
        /* AC-e5：切换没有重建图 */
        "AC-e5 探针：reload=0 · g-root 新建=0 · node-g 新建=0",
        /* AC-e3：分位数切点 */
        "AC-e3 · P95 = 9",
        "AC-e3 · 六个边界节点（P95 / P95−1 / P80 / P80−1 / P50 / P50−1）判档全部 exact",
        /* AC-e4：图例跟着模式变 */
        "AC-e4 · 图例换成了「中心度着色」",
        /* AC-e2：颜色确实变了 */
        "AC-e2 · 切到中心度后节点色 ≥2 种（确实变了）",
        /* 收尾 */
        "全部通过",
    ] {
        assert!(
            stdout.contains(needle),
            "探针输出里缺少 `{needle}` —— 判据被删/被绕过？\n{stdout}"
        );
    }
}
