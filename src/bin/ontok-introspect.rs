//! 源库勘查 CLI（REQ-003 · REQ-108）
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
//!
//! # REQ-108 · AC-001：产物里每张表带 `primary_key` / `unique_columns`
//!
//! 键事实来自 `pg_catalog` / `information_schema` 两条只读查询，
//! 由 [`KeyCatalog::inject_into_product`] 写进 `tables[i]`。
//! ⛔ 不用「列名是不是 `id`」来猜 —— 真实源库有 3 张表的主键叫别的名字，
//! 猜 `id` 会指向不存在的列（`ontok-propose` 的采样命中会因此全判 0）。

use anyhow::{Context, Result};
use ontok::extract::introspect::{introspect_with_keys, SOURCE_DATABASE_URL_ENV};

/// REQ-152：本文件不再自己成 bin —— 它是 `ontok-builder introspect` 的实现。
/// argv 由外层 clap 子命令**原样**递进来，参数/帮助/退出码一个字节都没变。
pub async fn main(args: Vec<String>) -> Result<()> {
    let mut out = String::from("out/schema.json");
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => {
                out = args.next().context("--out 需要一个路径参数")?;
            }
            "-h" | "--help" => {
                println!(
                    "源库只读勘查器\n\n\
                     环境变量：{SOURCE_DATABASE_URL_ENV}（源库连接串，口令不入库）\n\
                     参数：--out <path>  产出 JSON 路径（默认 out/schema.json）\n\
                     REQ-108：产物每张表带 primary_key / unique_columns（来自 pg_catalog，不靠列名猜）"
                );
                return Ok(());
            }
            other => anyhow::bail!("未知参数 {other}"),
        }
    }

    let url = std::env::var(SOURCE_DATABASE_URL_ENV).with_context(|| {
        format!("缺少环境变量 {SOURCE_DATABASE_URL_ENV}（源库连接串，不入库）")
    })?;

    let (report, keys) = introspect_with_keys(&url).await?;

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
    // REQ-108 · AC-001：主键/唯一列的覆盖情况必须**当场可见** ——
    // 否则「产物里有这两个字段」只能靠打开 JSON 才知道，漏采也没人发现。
    let single_pk = report
        .tables
        .iter()
        .filter(|t| keys.get(&t.name).and_then(|k| k.primary_key.as_ref()).is_some())
        .count();
    let composite: Vec<&str> = report
        .tables
        .iter()
        .filter(|t| keys.get(&t.name).map(|k| k.has_composite_primary_key()).unwrap_or(false))
        .map(|t| t.name.as_str())
        .collect();
    let uniq = report
        .tables
        .iter()
        .filter(|t| keys.get(&t.name).map(|k| !k.unique_columns.is_empty()).unwrap_or(false))
        .count();
    println!(
        "  单列主键      : {} / {} 表（无单列主键 {} · 其中复合 {}：{}）",
        single_pk,
        report.table_count,
        report.table_count - single_pk,
        composite.len(),
        if composite.is_empty() { "无".to_string() } else { composite.join(", ") }
    );
    println!("  单列唯一索引  : {} / {} 表", uniq, report.table_count);
    println!("  勘查时刻      : {}", report.introspected_at);

    // 产物 = SchemaReport 的 JSON ++ 每张表的键字段（AC-001）
    let mut product = serde_json::to_value(&report)?;
    keys.inject_into_product(&mut product);

    if let Some(parent) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out, serde_json::to_string_pretty(&product)?)
        .with_context(|| format!("写不出 {out}"))?;
    println!("  产出          : {out}");
    Ok(())
}
