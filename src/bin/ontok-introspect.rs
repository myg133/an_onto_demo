//! 源库勘查 CLI（REQ-003）
//!
//! 用法：
//! ```bash
//! export ONTOK_SOURCE_DATABASE_URL="postgresql://…"
//! cargo run --bin ontok-introspect -- --out out/schema.json
//! ```
//!
//! **只读**：连接串会被强制加上 `default_transaction_read_only=on`，
//! 且勘查前会校验服务端的 `transaction_read_only` 确实是 `on`，
//! 不是就直接报错退出（AC-101）。

use anyhow::{Context, Result};
use ontok::extract::introspect::{introspect, SOURCE_DATABASE_URL_ENV};

#[tokio::main]
async fn main() -> Result<()> {
    let mut out = String::from("out/schema.json");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => {
                out = args.next().context("--out 需要一个路径参数")?;
            }
            "-h" | "--help" => {
                println!(
                    "源库只读勘查器\n\n\
                     环境变量：{SOURCE_DATABASE_URL_ENV}（源库连接串，口令不入库）\n\
                     参数：--out <path>  产出 JSON 路径（默认 out/schema.json）"
                );
                return Ok(());
            }
            other => anyhow::bail!("未知参数 {other}"),
        }
    }

    let url = std::env::var(SOURCE_DATABASE_URL_ENV).with_context(|| {
        format!("缺少环境变量 {SOURCE_DATABASE_URL_ENV}（源库连接串，不入库）")
    })?;

    let report = introspect(&url).await?;

    // 人工可读的摘要 —— 让勘查结果不依赖打开 JSON 才知道概况
    println!("源库勘查完成（只读）");
    println!("  服务端        : {}", report.server_version);
    println!("  只读确认      : transaction_read_only = {}", report.transaction_read_only);
    println!("  schema        : {}", report.schema);
    println!("  表 / 列       : {} / {}", report.table_count, report.column_count);
    println!(
        "  有注释列      : {} ({:.0}%)",
        report.commented_column_count,
        report.commented_column_count as f64 * 100.0 / report.column_count.max(1) as f64
    );
    println!("  外键约束      : {}", report.foreign_key_count);
    println!("  勘查时刻      : {}", report.introspected_at);

    if let Some(parent) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out, serde_json::to_string_pretty(&report)?)
        .with_context(|| format!("写不出 {out}"))?;
    println!("  产出          : {out}");
    Ok(())
}
