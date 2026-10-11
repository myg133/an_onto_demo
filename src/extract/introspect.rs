//! 源库只读勘查器（REQ-003 · AC-101/AC-102）
//!
//! 从**客户源库**读出结构化 schema 元数据，交给抽取器转成本体提案。
//!
//! # 安全红线（AC-101）
//!
//! 本文件是整个平台里**唯一**接触客户数据的代码。三条硬约束：
//!
//! 1. **连接串强制追加 `default_transaction_read_only=on`。**
//!    写死在 [`force_read_only`] 里，不接受调用方传入 ——
//!    「靠连接串里带了只读参数」这种约定是不设防的，忘了加就等于没有。
//! 2. **只发 `SELECT`。** 本文件不含任何 INSERT/UPDATE/DELETE/DDL 字面量，
//!    由 `BA/verify-req003.sh` 的 AC-101 grep 守住。
//! 3. **只读 `information_schema` 与 `pg_catalog`。**
//!    不读任何业务表的数据行 —— 用户已明确授权边界：
//!    **发 schema 元数据，不发数据行**。
//!
//! # 为什么勘查与抽取要分开
//!
//! 勘查是**确定性**的（同样的库必然同样的产出，可缓存、可 diff、可复现）。
//! 抽取是**不确定性**的（LLM 有随机性）。两者混在一起就没法判断
//! 「产物变了」是因为库变了还是因为模型变了。
//! 所以这里只管勘查，LLM 那一步在 `extract` 侧。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

/// 环境变量名 —— 客户源库连接串。**与测试库刻意分开命名**，
/// 避免有人拿测试库连接串误当源库，或反之。
pub const SOURCE_DATABASE_URL_ENV: &str = "ONTOK_SOURCE_DATABASE_URL";

/// 要勘查的 schema。默认 `public`。
const TARGET_SCHEMA: &str = "public";

/// 勘查出的单列。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    /// SQL 类型原样（`character varying` / `timestamp without time zone` …）
    pub data_type: String,
    pub nullable: bool,
    /// 列注释。真实库里覆盖极不均匀 —— REQ-003 的评估显示
    /// 1068 列里约 64% 有注释，是抽取最重要的信息源。
    pub comment: Option<String>,
    pub is_primary_key: bool,
}

/// 勘查出的单表。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMeta {
    pub name: String,
    pub comment: Option<String>,
    pub columns: Vec<ColumnMeta>,
    /// 便捷统计 —— 抽取器据此判断这张表信息是否足够。
    pub column_count: usize,
    pub commented_count: usize,
}

impl TableMeta {
    /// 注释覆盖率。0 = 抽取器只能靠列名猜；高 = 有黄金信息。
    pub fn comment_ratio(&self) -> f64 {
        if self.column_count == 0 {
            return 0.0;
        }
        self.commented_count as f64 / self.column_count as f64
    }
}

/// 勘查产出的完整报告。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaReport {
    /// 勘查时刻的服务端版本 —— 留痕，便于比对。
    pub server_version: String,
    /// 勘查时刻的 `transaction_read_only` 实际值。
    /// **AC-101 要求把这个值写进产物**：只读保证不能只靠代码里那句注释。
    pub transaction_read_only: String,
    pub schema: String,
    pub table_count: usize,
    pub column_count: usize,
    pub commented_column_count: usize,
    /// 真实库的外键数。REQ-003 已查明本库为 0 —— 关系抽取只能靠命名约定。
    pub foreign_key_count: usize,
    pub tables: Vec<TableMeta>,
    /// 勘查时刻的 ISO-8601 时间。
    pub introspected_at: String,
}

/// **强制只读**：把连接串改造成带 `default_transaction_read_only=on` 的形式。
///
/// 已带该参数则保留；没带则追加。**不接受调用方绕过** ——
/// 这是 AC-101 的核心，改成 `pub(crate)` 也只在本 crate 内可见。
fn force_read_only(url: &str) -> Result<String> {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let mut params: Vec<String> = query
        .map(|q| {
            q.split('&')
                .filter(|p| !p.is_empty())
                .map(|p| p.to_string())
                .collect()
        })
        .unwrap_or_default();

    if !params.iter().any(|p| p.starts_with("default_transaction_read_only=")) {
        params.push("default_transaction_read_only=on".to_string());
    }
    if params.is_empty() {
        return Ok(base.to_string());
    }
    Ok(format!("{base}?{}", params.join("&")))
}

/// **AC-101 fail-closed**：服务端读回的 `transaction_read_only` 不是 `on`
/// 就拒绝勘查 —— 宁可不出方案，也不在不确定是否只读的连接上碰客户库。
///
/// **为什么抽成纯函数（REQ-003 S1）**：独立审核实测「把 fail-closed 校验阉割掉」
/// 这个变异，**148 个测试全绿** —— 因为这道判据当时只存在于函数体里，
/// 没有任何测试真的执行它。抽成纯函数后，「拒绝」这个决定本身可测。
fn ensure_read_only(read_only: &str) -> Result<()> {
    if read_only != "on" {
        bail!(
            "transaction_read_only = {read_only}，不是 on。\
             拒绝在不保证只读的前提下勘查客户库。"
        );
    }
    Ok(())
}

/// 构造**只读连接池** —— [`introspect`] 与只读防回归测试**共用这一条路径**。
///
/// 抽出来不是为了好看（S1）：审核实测「删掉 `after_connect` 里的 SET」与
/// 「删掉 `force_read_only` 的调用点」两个变异，**148 个测试全绿** ——
/// 因为 `force_read_only` 原本只测了纯函数，**没人测它有没有被真的调用**。
/// 抽成独立函数后，行为测试能拿它连库、读回 `transaction_read_only`；
/// 剩下行为层看不见的接线（调用点是否还在），由
/// `tests/test_read_only_guard.rs` 守。
///
/// `url` 会被 [`force_read_only`] 改造；调用方传原始连接串即可。
pub(crate) async fn read_only_pool(url: &str) -> Result<PgPool> {
    let safe_url = force_read_only(url)?;
    let opts: PgConnectOptions = safe_url
        .parse()
        .context("源库连接串无法解析（口令不入库，只走环境变量）")?;
    // max_connections 压到 2：源库是客户资产，连接池开大既无必要也不礼貌。
    // 顺带缓解 S6 —— 复审查明测试库在非标准端口 30432 是代理，会重置超额连接。
    PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(30))
        // **双保险**：startup 参数（force_read_only）之外，连上后立刻再 SET 一次。
        //
        // 为什么要冗余 —— 实测踩过：sqlx 0.8 解析连接串时**没有**把这个
        // startup 参数透传给服务端，`current_setting('default_transaction_read_only')`
        // 读回来是 `off`。只靠连接串参数 = 不设防。
        //
        // 下面是「没设防会被抓住」的证据：勘查器在跑任何查询**之前**校验
        // transaction_read_only，不是 `on` 就直接退出 —— 宁可拒绝勘查，
        // 也不在不确定是否只读的连接上碰客户库（AC-101 fail-closed）。
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET default_transaction_read_only = on")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect_with(opts)
        .await
        .context("连不上源库（确认只读账号可用、库名 schema 正确）")
}

/// 连源库并勘查。
pub async fn introspect(url: &str) -> Result<SchemaReport> {
    let pool = read_only_pool(url).await?;

    let server_version: String = sqlx::query_scalar("SELECT version()")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|_| "unknown".to_string());

    // AC-101 留痕：只读保证要能被外部验证，不能只写在注释里。
    let read_only: String = sqlx::query_scalar(
        "SELECT current_setting('default_transaction_read_only')",
    )
    .fetch_one(&pool)
    .await
    .context("读不回 transaction_read_only，只读保证无法验证")?;

    ensure_read_only(&read_only)?;

    let foreign_key_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.table_constraints \
         WHERE table_schema = $1 AND constraint_type = 'FOREIGN KEY'",
    )
    .bind(TARGET_SCHEMA)
    .fetch_one(&pool)
    .await
    .unwrap_or(0);

    let rows: Vec<(String, String, String, String, bool, Option<String>, bool)> = sqlx::query_as(
        "SELECT t.table_name,
                t.table_name,
                c.column_name,
                c.data_type,
                c.is_nullable = 'YES',
                col_description(
                    (quote_ident(t.table_schema) || '.' || quote_ident(t.table_name))::regclass::oid,
                    c.ordinal_position
                ) AS cmt,
                COALESCE(pk.is_pk, false)
         FROM information_schema.tables t
         JOIN information_schema.columns c
           ON c.table_schema = t.table_schema AND c.table_name = t.table_name
         LEFT JOIN (
             SELECT kcu.table_name, kcu.column_name, true AS is_pk
             FROM information_schema.table_constraints tc
             JOIN information_schema.key_column_usage kcu
               ON kcu.constraint_name = tc.constraint_name
              AND kcu.table_schema = tc.table_schema
             WHERE tc.table_schema = $1 AND tc.constraint_type = 'PRIMARY KEY'
         ) pk ON pk.table_name = t.table_name AND pk.column_name = c.column_name
         WHERE t.table_schema = $1 AND t.table_type = 'BASE TABLE'
         ORDER BY t.table_name, c.ordinal_position",
    )
    .bind(TARGET_SCHEMA)
    .fetch_all(&pool)
    .await
    .context("读 information_schema 失败")?;

    // 表注释单独取：information_schema.tables 没有 comment 列
    let tbl_comments: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT c.relname,
                obj_description((quote_ident(n.nspname) || '.' || quote_ident(c.relname))::regclass::oid)
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = $1 AND c.relkind = 'r'",
    )
    .bind(TARGET_SCHEMA)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    let tcmap: std::collections::HashMap<String, Option<String>> =
        tbl_comments.into_iter().collect();

    pool.close().await;

    // 折叠成表级结构
    let mut tables: Vec<TableMeta> = Vec::new();
    for (tname, _tname2, cname, dtype, nullable, cmt, is_pk) in rows {
        let col = ColumnMeta {
            name: cname,
            data_type: dtype,
            nullable,
            comment: cmt.filter(|s| !s.trim().is_empty()),
            is_primary_key: is_pk,
        };
        match tables.iter_mut().find(|t| t.name == tname) {
            Some(t) => t.columns.push(col),
            None => tables.push(TableMeta {
                comment: tcmap.get(&tname).cloned().flatten().filter(|s| !s.trim().is_empty()),
                column_count: 0,
                commented_count: 0,
                name: tname,
                columns: vec![col],
            }),
        }
    }
    for t in &mut tables {
        t.column_count = t.columns.len();
        t.commented_count = t.columns.iter().filter(|c| c.comment.is_some()).count();
    }

    let table_count = tables.len();
    let column_count: usize = tables.iter().map(|t| t.column_count).sum();
    let commented_column_count: usize = tables.iter().map(|t| t.commented_count).sum();

    Ok(SchemaReport {
        server_version: server_version.lines().next().unwrap_or("").to_string(),
        transaction_read_only: read_only,
        schema: TARGET_SCHEMA.to_string(),
        table_count,
        column_count,
        commented_column_count,
        foreign_key_count: foreign_key_count.max(0) as usize,
        tables,
        introspected_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// 勘查未注释表（真实库有 20+ 张零注释表，抽取器对它们只能靠列名猜）。
pub fn uncommented_tables(report: &SchemaReport) -> Vec<&TableMeta> {
    report
        .tables
        .iter()
        .filter(|t| t.commented_count == 0)
        .collect()
}

/// 自检：确保连接串确实被改造成只读。仅在测试里用。
#[cfg(test)]
mod tests {
    use super::{ensure_read_only, force_read_only, read_only_pool};

    #[test]
    fn appends_read_only_when_absent() {
        assert_eq!(
            force_read_only("postgresql://u:p@h:5432/d").unwrap(),
            "postgresql://u:p@h:5432/d?default_transaction_read_only=on"
        );
    }

    #[test]
    fn preserves_existing_params_and_keeps_read_only() {
        let got = force_read_only("postgresql://u:p@h:5432/d?sslmode=require").unwrap();
        assert!(got.contains("sslmode=require"));
        assert!(got.contains("default_transaction_read_only=on"));
    }

    #[test]
    fn does_not_duplicate_read_only() {
        let got = force_read_only("postgresql://u:p@h/d?default_transaction_read_only=on").unwrap();
        assert_eq!(got.matches("default_transaction_read_only").count(), 1);
    }

    // ── REQ-003 · S1 只读防回归 ─────────────────────────
    //
    // 独立审核实测三个变异（删 after_connect 的 SET / 阉割 fail-closed /
    // 删 force_read_only 调用点）**148 个测试全绿** —— 只读保证零防回归。
    // 下面两条把这道红线变成可执行判据。

    /// fail-closed **这个决定本身**：读回的不是 `on` 就必须报错。
    /// 变异：把 `ensure_read_only` 改成永远 `Ok(())` ⇒ 本测试立刻变红。
    #[test]
    fn ensure_read_only_rejects_anything_but_on() {
        assert!(ensure_read_only("on").is_ok(), "on 是唯一放行值");
        for bad in ["off", "Off", "ON", "", "true", "on ", "1"] {
            assert!(
                ensure_read_only(bad).is_err(),
                "transaction_read_only = {bad:?} 必须 fail-closed 拒绝勘查"
            );
        }
    }

    /// **`introspect` 真的走只读池** —— 拿测试库连一次，断言服务端读回 `on`。
    ///
    /// 抓的是「`after_connect` 里的 SET 被删」：sqlx 0.8 不会把 startup 参数
    /// 透传给服务端，SET 一拿掉读回就是 `off`，本测试变红（实测过）。
    ///
    /// **抓不到**「`force_read_only` 的调用点被删」—— after_connect 的 SET
    /// 独立就够 `on`，库上看不出差别。那个变异由
    /// `tests/test_read_only_guard.rs` 的接线判据守。
    ///
    /// 用测试库（`ONTOK_TEST_DATABASE_URL`）而不是客户源库 —— 只读语义与库无关，
    /// 且不接触客户资产。
    #[tokio::test]
    async fn read_only_pool_really_reads_back_on() {
        let url = crate::config::test_database_url().expect(
            "缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库，请导出环境变量（见 .env.example）",
        );
        let pool = read_only_pool(&url).await.expect("连不上测试库");
        let read_only: String = sqlx::query_scalar("SELECT current_setting('default_transaction_read_only')")
            .fetch_one(&pool)
            .await
            .expect("读不回 transaction_read_only");
        pool.close().await;
        assert_eq!(
            read_only, "on",
            "只读池连上后服务端必须读回 transaction_read_only = on"
        );
    }
}
