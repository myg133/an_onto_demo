//! `ontok` 二进制入口 —— **服务面**（REQ-152）。
//!
//! 只有三个子命令：`query serve` / `mcp-server` / `serve`。
//! 构建面（勘查 / 提案 / 审核 / 导入导出 / access 验收）在 `ontok-builder`，
//! 见 `src/bin/ontok-builder.rs`。
//!
//! ⛔ **本文件不许引用 `ontok::cli`**：那是构建面入口，它的可达图里有执行器
//! （`access::net` / `access::tc3` / `ureq`）。AC-006 的判据是
//! `strings target/release/ontok | grep -c 'X-TC-Action'` ⇒ 0 ——
//! 引用 `cli` 就会把它变成非 0（守卫：`tests/test_req152_bins.rs`）。
//!
//! AC-012 的 16 条命令从这里进。错误一律非零退出 ——
//! 「打印了错误但退出码是 0」会让 shell 脚本和判定器都以为成功了。

use clap::Parser;

#[tokio::main]
async fn main() {
    ontok::pipe_guard::install_broken_pipe_guard();
    let cli = ontok::service::Cli::parse();
    if let Err(e) = ontok::service::run(cli).await {
        eprintln!("✗ {e}");
        std::process::exit(1);
    }
    // 输出管道被下游关掉时（`ontok ... | head`），
    // 冲刷会失败 —— 那不是错误，直接正常退出。
    if std::io::stdout().flush().is_err() {
        std::process::exit(0);
    }
}

use std::io::Write;
