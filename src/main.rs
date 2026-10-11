//! `ontok` 二进制入口。
//!
//! AC-012 的 16 条命令从这里进。错误一律非零退出 ——
//! 「打印了错误但退出码是 0」会让 shell 脚本和判定器都以为成功了。

use clap::Parser;

#[tokio::main]
async fn main() {
    ontok::cli::install_broken_pipe_guard();
    let cli = ontok::cli::Cli::parse();
    if let Err(e) = ontok::cli::run(cli).await {
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
