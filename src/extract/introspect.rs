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

// ──────────────────────────────────────────────────────────
// REQ-108 · AC-001：主键与单列唯一索引
//
// 为什么**不**给 `TableMeta` 加字段（这一点反直觉，写清免得后人再踩）：
// `TableMeta` / `SchemaReport` 的字面量构造散布在 10+ 个测试与
// `src/bin/ontok-gate-preflight.rs`、`tests/test_fk_naming.rs` 里 ——
// 后两者是**受保护文件**（不得改动）。给这两个类型加字段会让它们编译不过，
// 而那正是 `src/agent/group.rs:197` 早就记下的那条约束
//（「给 `SchemaReport` 加字段会让那个受保护文件编译不过」）。
//
// 所以键信息走**产物契约**：字段落在 `schema.json` 的 `tables[i]` 上
//（AC-001 要的「每张表带 `primary_key` / `unique_columns`」），
// 由 [`KeyCatalog::inject_into_product`] 注入、[`KeyCatalog::from_product`] 读回。
// 两边互为逆运算，由单元测试钉住；**没有第二份真相** ——
// 目录数据只来自 `pg_catalog` / `information_schema` 这两条只读查询。
// ──────────────────────────────────────────────────────────

/// 一张表的键事实（REQ-108 · AC-001）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableKeys {
    /// 单列主键的列名；**复合主键或没有主键 ⇒ `None`**。
    ///
    /// ⛔ 不许用「列名是不是 `id`」来猜：真实源库 96 张表里有 3 张的主键
    /// 叫别的名字（`alembic_version.version_num` / `service_apps.app_id` /
    /// `sub_account_bind_permission.sub_customer_id`）。猜 `id` 的实现对
    /// `service_apps` 会指到一个**不存在**的列上，于是「目标列是不是键」
    /// 与「采样命中」两件事同时判错 —— 那正是本需求要消灭的近似。
    #[serde(default)]
    pub primary_key: Option<String>,
    /// 单列唯一索引覆盖的列名（去重、字典序，含主键自带的唯一索引）。
    #[serde(default)]
    pub unique_columns: Vec<String>,
    /// 复合主键的全部列名（≥2 列时非空；R2 只对**这一种**给单独上限）。
    ///
    /// 与 `primary_key == None` 分开记：`None` 既可能是「复合」也可能是
    /// 「没有主键」，而这两种在评分卡里的含义不同（R2 vs 普通列 0.4）。
    #[serde(default)]
    pub composite_primary_key: Vec<String>,
}

impl TableKeys {
    /// **引用目标用哪一列验命中**：单列主键 → 第一个单列唯一列 → 无。
    pub fn key_column(&self) -> Option<&str> {
        if let Some(pk) = self.primary_key.as_deref() {
            return Some(pk);
        }
        if !self.composite_primary_key.is_empty() {
            return None;
        }
        self.unique_columns.first().map(|s| s.as_str())
    }

    pub fn has_composite_primary_key(&self) -> bool {
        !self.composite_primary_key.is_empty()
    }
}

/// 表名 → 键事实。产物里没有键字段的旧 `schema.json` 读回来是**空目录**。
///
/// ⚠️ 「目录里没有这张表」与「这张表没有主键」是**两件事**：
/// 前者是**不知道**（旧产物），后者是**事实**。调用方必须分开处理 ——
/// 把「不知道」当成「没有主键」会静默地把所有关系压进低分档。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KeyCatalog {
    tables: std::collections::BTreeMap<String, TableKeys>,
}

impl KeyCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, table: &str) -> Option<&TableKeys> {
        self.tables.get(table)
    }

    pub fn insert(&mut self, table: impl Into<String>, keys: TableKeys) {
        self.tables.insert(table.into(), keys);
    }

    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// 把键事实**写进产物 JSON 的 `tables[i]`**（AC-001 的落地动作）。
    ///
    /// 只写三样：`primary_key` / `unique_columns` / `composite_primary_key`。
    /// `primary_key` 为 `None` 时写 **JSON `null`**（不是省略键）——
    /// 「这张表没有单列主键」是需要被读出来的事实，不是缺字段。
    pub fn inject_into_product(&self, product: &mut serde_json::Value) {
        let Some(tables) = product.get_mut("tables").and_then(|t| t.as_array_mut()) else {
            return;
        };
        for t in tables {
            let Some(name) = t.get("name").and_then(|n| n.as_str()).map(str::to_string) else {
                continue;
            };
            let Some(keys) = self.tables.get(&name) else { continue };
            let Some(obj) = t.as_object_mut() else { continue };
            obj.insert(
                "primary_key".to_string(),
                keys.primary_key
                    .as_ref()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .unwrap_or(serde_json::Value::Null),
            );
            obj.insert(
                "unique_columns".to_string(),
                serde_json::to_value(&keys.unique_columns).unwrap_or(serde_json::Value::Array(vec![])),
            );
            obj.insert(
                "composite_primary_key".to_string(),
                serde_json::to_value(&keys.composite_primary_key)
                    .unwrap_or(serde_json::Value::Array(vec![])),
            );
        }
    }

    /// 从产物 JSON 读回键事实 —— [`Self::inject_into_product`] 的逆运算。
    ///
    /// **判「字段在不在」而不是「值空不空」**：`"primary_key": null` 是
    /// 一条事实（没有单列主键），而**没有 `primary_key` 这个键**是旧产物。
    /// 两者混起来，旧产物会被读成「96 张表都没有主键」。
    pub fn from_product(product: &serde_json::Value) -> Self {
        let mut out = Self::new();
        let Some(tables) = product.get("tables").and_then(|t| t.as_array()) else {
            return out;
        };
        for t in tables {
            let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
            let Some(obj) = t.as_object() else { continue };
            // 旧产物（REQ-108 之前）根本没有这个键 ⇒ 跳过整张表
            if !obj.contains_key("primary_key") {
                continue;
            }
            out.insert(
                name,
                TableKeys {
                    primary_key: obj
                        .get("primary_key")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    unique_columns: obj
                        .get("unique_columns")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                    composite_primary_key: obj
                        .get("composite_primary_key")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                },
            );
        }
        out
    }
}

/// 把主键行 / 唯一索引行折叠成 [`KeyCatalog`]（纯函数，可单测）。
///
/// 入参是**目录原样的行**（表名, 列名），顺序不重要 —— 输出按表名与列名排序，
/// 同一份库跑两次必然同字节（AC-002 的确定性从这一步就开始）。
fn fold_keys(pk_rows: &[(String, String)], uniq_rows: &[(String, String)]) -> KeyCatalog {
    let mut by_table: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (table, column) in pk_rows {
        let v = by_table.entry(table.clone()).or_default();
        if !v.contains(column) {
            v.push(column.clone());
        }
    }
    let mut catalog = KeyCatalog::new();
    for (table, cols) in by_table {
        let keys = if cols.len() == 1 {
            TableKeys {
                primary_key: Some(cols[0].clone()),
                ..Default::default()
            }
        } else {
            // 复合主键：单列主键记 null，**但复合事实要留下**（R2 要用）
            let mut sorted = cols;
            sorted.sort();
            TableKeys { primary_key: None, composite_primary_key: sorted, ..Default::default() }
        };
        catalog.insert(table, keys);
    }
    for (table, column) in uniq_rows {
        let entry = catalog
            .tables
            .entry(table.clone())
            .or_insert_with(TableKeys::default);
        if !entry.unique_columns.contains(column) {
            entry.unique_columns.push(column.clone());
        }
    }
    for keys in catalog.tables.values_mut() {
        keys.unique_columns.sort();
        keys.unique_columns.dedup();
    }
    catalog
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
///
/// **本次签名与产出逐字节不变**（REQ-003 起的既有入口，被
/// `ontok-gate-preflight` 等调用点依赖）。要拿 REQ-108 的键事实请用
/// [`introspect_with_keys`] —— 本函数只是它的薄壳。
pub async fn introspect(url: &str) -> Result<SchemaReport> {
    Ok(introspect_with_keys(url).await?.0)
}

/// 连源库并勘查，**同时**返回主键 / 单列唯一索引目录（REQ-108 · AC-001）。
///
/// 与 [`introspect`] 走**同一条只读池**、同一次连接，产物里
/// `SchemaReport` 那部分与旧版逐字段相同 —— 键目录是**附加**输出，
/// 由调用方（`ontok-introspect`）注入产物的 `tables[i]`。
pub async fn introspect_with_keys(url: &str) -> Result<(SchemaReport, KeyCatalog)> {
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

    // ── REQ-108 · AC-001：主键与单列唯一索引 ───────────────────────────
    //
    // 两条都**只读 `information_schema` / `pg_catalog`**（与上面同一红线：
    // 本文件不碰任何业务数据行）。
    //
    // ⚠️ 主键按**列**取、再在 Rust 里折叠成「单列 / 复合」：
    // `information_schema` 里没有「这个主键是单列吗」这一列，
    // 在 SQL 侧拼一个 `count(*) = 1` 会把「复合」与「无主键」压成同一个
    // `false` —— 而 R2（复合键上限 0.50）恰恰要区分这两者。
    let pk_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT tc.table_name, kcu.column_name
         FROM information_schema.table_constraints tc
         JOIN information_schema.key_column_usage kcu
           ON kcu.constraint_name = tc.constraint_name
          AND kcu.table_schema = tc.table_schema
         WHERE tc.table_schema = $1 AND tc.constraint_type = 'PRIMARY KEY'
         ORDER BY tc.table_name, kcu.ordinal_position",
    )
    .bind(TARGET_SCHEMA)
    .fetch_all(&pool)
    .await
    .context("读主键失败")?;

    // 单列唯一索引。判据用 `indnkeyatts = 1` 而不是 `indnatts`：
    // `indnatts` 把 `INCLUDE` 列也算进去（PG 11+），
    // `UNIQUE (a) INCLUDE (b)` 会被误判成复合，于是 `a` 丢掉 0.9 档。
    let uniq_rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT c.relname, a.attname
         FROM pg_index i
         JOIN pg_class c ON c.oid = i.indrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = i.indkey[0]
         WHERE n.nspname = $1
           AND i.indisunique
           AND i.indnkeyatts = 1
           AND a.attnum > 0
           AND NOT a.attisdropped
         ORDER BY c.relname, a.attname",
    )
    .bind(TARGET_SCHEMA)
    .fetch_all(&pool)
    .await
    .context("读单列唯一索引失败")?;

    pool.close().await;

    let keys = fold_keys(&pk_rows, &uniq_rows);

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

    Ok((
        SchemaReport {
            server_version: server_version.lines().next().unwrap_or("").to_string(),
            transaction_read_only: read_only,
            schema: TARGET_SCHEMA.to_string(),
            table_count,
            column_count,
            commented_column_count,
            foreign_key_count: foreign_key_count.max(0) as usize,
            tables,
            introspected_at: chrono::Utc::now().to_rfc3339(),
        },
        keys,
    ))
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
    use super::{ensure_read_only, fold_keys, force_read_only, read_only_pool, KeyCatalog};

    // ── REQ-108 · AC-001：主键 / 唯一列的折叠与产物往返 ──────────────

    fn rows(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    /// 单列主键 ⇒ `primary_key = Some(col)`；复合 ⇒ 单列记 null、复合事实留下。
    ///
    /// 这条钉的是 R2 的前提：**「复合」不能被折叠成「没有主键」**。
    #[test]
    fn fold_keys_distinguishes_single_composite_and_absent_primary_key() {
        let cat = fold_keys(
            &rows(&[
                ("service_apps", "app_id"),
                ("llm_auth_cache_trigger_state", "subject_type"),
                ("llm_auth_cache_trigger_state", "subject_id"),
                ("llm_auth_cache_trigger_state", "state_scope"),
            ]),
            &rows(&[("service_apps", "app_id")]),
        );
        let sa = cat.get("service_apps").expect("service_apps 有主键事实");
        assert_eq!(sa.primary_key.as_deref(), Some("app_id"), "主键叫 app_id，不叫 id");
        assert_eq!(sa.key_column(), Some("app_id"));
        assert!(!sa.has_composite_primary_key());

        let com = cat.get("llm_auth_cache_trigger_state").expect("复合主键表也在目录里");
        assert_eq!(com.primary_key, None, "复合主键没有「单列主键名」");
        assert_eq!(
            com.composite_primary_key,
            vec!["state_scope", "subject_id", "subject_type"],
            "复合事实必须留下（R2 靠它给 0.50 上限），且按列名排序 ⇒ 确定性"
        );
        assert_eq!(com.key_column(), None, "复合键不能当单列引用目标");

        // 目录里没有这张表 ⇒ `None`（不知道），**不是**「没有主键」
        assert!(cat.get("no_such_table").is_none());
    }

    /// 唯一列：去重 + 字典序。**含主键自带的唯一索引**（AC-001 的判据要求
    /// `marketing_channel.id` 出现在 `unique_columns` 里）。
    #[test]
    fn fold_keys_sorts_and_dedupes_unique_columns() {
        let cat = fold_keys(
            &rows(&[("marketing_channel", "id")]),
            &rows(&[
                ("marketing_channel", "id"),
                ("marketing_channel", "code"),
                ("marketing_channel", "id"),
            ]),
        );
        let mc = cat.get("marketing_channel").unwrap();
        assert_eq!(mc.unique_columns, vec!["code", "id"], "去重 + 字典序");
        assert!(mc.unique_columns.contains(&"id".to_string()));
        assert_eq!(mc.primary_key.as_deref(), Some("id"));
    }

    /// 产物往返：`inject_into_product` → JSON → `from_product` **逐字段相等**。
    ///
    /// 变异自证：把 `inject` 里 `primary_key` 的 `None` 改成「省略这个键」，
    /// 往返后 `service_apps` 那类表会从「有主键」变成「旧产物」⇒ 本测试红。
    #[test]
    fn key_catalog_survives_the_product_round_trip() {
        let cat = fold_keys(
            &rows(&[("llm_model", "id"), ("service_apps", "app_id")]),
            &rows(&[("llm_model", "id"), ("llm_model", "model_id")]),
        );
        let mut product = serde_json::json!({
            "schema": "public",
            "tables": [
                {"name": "llm_model", "column_count": 17},
                {"name": "service_apps", "column_count": 3},
                {"name": "un_keyed_unknown", "column_count": 1}
            ]
        });
        cat.inject_into_product(&mut product);

        // AC-001 的字面要求：**目录覆盖到的**每张表都带这两个字段，
        // 且 `primary_key` 为 `None` 时写的是 JSON `null`（不是省略键）。
        //
        // ⚠️ 目录里**没有**的表（本 fixture 的 `un_keyed_unknown`）保持原样：
        // 那是「这次勘查没采到它」，不是「它没有主键」—— 凭空补一个 `null`
        // 等于替数据库做一个没查过的断言（fail-open）。
        for name in ["llm_model", "service_apps"] {
            let t = product["tables"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == serde_json::json!(name))
                .unwrap();
            assert!(t.as_object().unwrap().contains_key("primary_key"), "{name} 缺 primary_key");
            assert!(t.as_object().unwrap().contains_key("unique_columns"), "{name} 缺 unique_columns");
        }
        let unknown = &product["tables"][2];
        assert!(
            !unknown.as_object().unwrap().contains_key("primary_key"),
            "目录里没有的表不许被补上字段（那是伪造事实）"
        );
        assert_eq!(product["tables"][0]["primary_key"], serde_json::json!("id"));
        assert_eq!(
            product["tables"][0]["unique_columns"],
            serde_json::json!(["id", "model_id"])
        );

        let back = KeyCatalog::from_product(&product);
        for name in ["llm_model", "service_apps"] {
            assert_eq!(
                back.get(name),
                cat.get(name),
                "{name} 的键事实在产物往返后必须逐字段相同"
            );
        }
        // 目录里没有的表：产物里没有键字段 ⇒ 读回来也没有（不伪造）
        assert!(back.get("un_keyed_unknown").is_none());
    }

    /// **旧产物**（REQ-108 之前）读回来是**空目录**，而不是「96 张表都没主键」。
    #[test]
    fn old_products_yield_an_empty_catalog_not_a_fake_one() {
        let old = serde_json::json!({
            "tables": [{"name": "llm_model", "column_count": 17}]
        });
        let cat = KeyCatalog::from_product(&old);
        assert!(cat.is_empty(), "旧产物没有键字段 ⇒ 不知道，不是「没有主键」");
        assert!(cat.get("llm_model").is_none());
    }

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
