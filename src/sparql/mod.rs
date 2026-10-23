//! REQ-122 · **SPARQL 查询规划与租户注入**（数字员工链路的第三块地基）。
//!
//! ```text
//! Agent 写:  SELECT ?金额 WHERE { ?d :deductions__customers ?c . ?d :deductions__deduction_amount ?金额 }
//! 服务端:    SELECT ... FROM "deductions" t0 ... WHERE t0."main_customer_id" = 'C0001' AND ...
//!            └── 注入由**声明**决定，Agent 看不见也改不了
//! ```
//!
//! # 分层（ADR-006）
//!
//! | 层 | 归属 | 在哪 |
//! | --- | --- | --- |
//! | **机制** | 产品 | 本模块：解析 SPARQL 子集 → 沿 R2RML 的 joinCondition 连表 → **按声明**逐表注入 → 只读执行 |
//! | **声明** | 项目 | `query-policy.json`（见 [`policy`]）：租户主体、逐表过滤列、公共维度 + 理由 |
//! | **代码** | —— | ⛔ **不出现任何租户列名字面量**；换库只改声明 |
//!
//! # 五个硬性质（每条都有测试/实测）
//!
//! | # | 性质 | 判据在哪 |
//! | --- | --- | --- |
//! | R1 | 不支持的语法**报错**，⛔ 不静默忽略 | [`query`] 的拒绝用例 |
//! | R2 | 未知类/属性**报错**并给可用清单，⛔ 不返回空结果 | [`plan`] 的 `unknown_predicate` / `class_of` |
//! | R3 | 租户无法确定 ⇒ **整条拒绝**，点名表 | [`plan`] 的 `decide_tenants` |
//! | R4 | 注入在 SQL 生成后、执行前，**每张涉及的表都有** | [`plan`] 的 `where_clause`（唯一实现） |
//! | R5 | 注入**可见**：`sql_echo` 就是真正执行的那条 SQL | [`QueryResult::sql_echo`] |
//!
//! # 映射从哪来（⛔ 不解析 TTL）
//!
//! 全部来自 [`crate::domain::r2rml::build_mapping`] —— 与 `ontok r2rml generate`
//! **同一个函数**。两份实现会漂移，而漂移的那次**不报错**。
//!
//! # 已知限制（写在这里，不藏在代码里）
//!
//! - 一个变量只能属于一个类（没有 UNION/OPTIONAL）；不连通的两段模式直接拒绝（不隐式笛卡尔积）；
//! - 只支持单列 GROUP BY / ORDER BY，且 ORDER BY 的变量必须出现在 SELECT 里；
//! - `ORDER BY` 是**单列**且不带附加 tie-break ⇒ 排序键相同的行，其相对顺序由 PG 决定
//!   （同一份数据两次查询可能不同）。要稳定顺序请把主键/时间也投影出来。
//! - 反向关系靠 R2RML 前向 join 反解；**每条 join 只有一个方向**（R2RML 如此）；自连接不支持；
//! - 没有子账号 id → 主账号的解析层（ADR-006 的 Limitation，属将来）。**但它不再沉默**：
//!   声明里 `coverage: "main_only"` 的表，命中时会在 `warnings[]` 里点名
//!   （见 [`policy::Coverage`]）——「主账号少看到钱」必须让调用方知道。
//! - `SELECT DISTINCT` 是 v1 清单之外的**唯一**额外项（它不改变过滤语义，只是去重；
//!   已获 BA 批准，见 [`query`] 模块头）。⛔ 再想扩清单必须先改文档与验收。
//!
//! # `truncated` 是怎么判的（I-2 裁定）
//!
//! SQL 里的 `LIMIT` 是 **`effective_limit + 1`**（多取一行探针）：
//! 只有这样才分得清「结果正好 N 行」与「还有第 N+1 行」。探针行**不进结果**，
//! 判据是 `truncated = capped && 探针行存在`，`capped` = 查询没写 LIMIT 或写的 LIMIT
//! 超过 `--max-rows`。`sql_echo` **逐字包含** `LIMIT N+1` 与尾部的探针注释
//! （回执与执行不差一个字节）—— 验收口径：`LIMIT <= max_rows + 1`。

pub mod error;
pub mod exec;
pub mod plan;
pub mod policy;
pub mod query;
pub mod sql;

use serde::Serialize;
use sqlx::postgres::PgPool;

pub use error::QueryError;
pub use plan::{PlanInput, Planned, PublicDimensionUsed, TenantApplied};
pub use policy::QueryPolicy;

/// 查询回执（AC-009 的形状）。
///
/// 字段顺序与 AC 一致；`public_dimensions` 是**多加**的一项 —— 没有它，
/// 「`tenant_applied` 覆盖 `tables` 里除公共维度外的每一张表」这条判据
/// 只能靠人去翻声明，无法机检。
#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub row_count: usize,
    /// 结果被 `--max-rows` 截断。判定见模块头「`truncated` 是怎么判的」。
    pub truncated: bool,
    /// 真正执行的 SQL（租户条件**在 WHERE 最前面**，人第一眼就能看见；
    /// `LIMIT` = 上限 + 1 行探针，末尾带注释）。
    pub sql_echo: String,
    pub tenant_applied: Vec<TenantApplied>,
    pub tables: Vec<String>,
    /// 声明为公共维度、因而**没有**注入的表（含理由）。
    pub public_dimensions: Vec<PublicDimensionUsed>,
    /// 可行动提示：截断 / LIMIT 被压 / 覆盖范围不完整（`coverage != complete`）等。
    pub warnings: Vec<String>,
}

/// 规划 + 执行。**唯一**入口。
///
/// ⚠️ 执行失败时的错误里带着 `SQL:` —— 一条 PG 报错没有 SQL 是不可行动的
/// （「column does not exist」得看见是哪条 SELECT）。
pub async fn run_query(
    pool: &PgPool,
    input: &PlanInput<'_>,
    sparql: &str,
    timeout_ms: u64,
) -> Result<QueryResult, QueryError> {
    let planned = plan::plan(sparql, input)?;
    let rows = exec::run(
        pool,
        &planned.sql,
        planned.columns.len(),
        planned.effective_limit,
        timeout_ms,
    )
    .await
    .map_err(|e| QueryError::new(format!("{}\n  SQL: {}", e.message(), planned.sql)))?;

    let truncated = planned.truncated_possible && rows.more;
    let mut warnings = planned.warnings.clone();
    if truncated {
        warnings.push(format!(
            "结果超过 {0} 行（--max-rows），已截断：只返回前 {0} 行。\
             想拿全量请缩小查询条件（加 FILTER / 更具体的 join），或显式调大 --max-rows",
            planned.effective_limit
        ));
    }
    let row_count = rows.rows.len();
    Ok(QueryResult {
        columns: planned.columns,
        rows: rows.rows,
        row_count,
        truncated,
        sql_echo: planned.sql,
        tenant_applied: planned.tenant_applied,
        tables: planned.tables,
        public_dimensions: planned.public_dimensions,
        warnings,
    })
}
