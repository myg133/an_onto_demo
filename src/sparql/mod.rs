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
//! | R5 | 注入**可见**：`sql_echo` 就是真正执行的那条 SQL | [`QueryResult::sql_echo`] + [`receipt`] 的单测 |
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

    Ok(receipt(planned, rows))
}

/// 把「规划结果 + 执行结果」组装成回执 —— **`sql_echo` 只在这里产生**。
///
/// # 为什么单独抽成一个纯函数（REQ-122 · N1）
///
/// 「`sql_echo` 就是真正执行的那条 SQL」是整条**注入可视化**验收的地基
/// （R5 / AC-004 / AC-005 / AC-009 都踩在它上面）。原先它只出现在
/// [`run_query`] 里，而那是个要连库的 async 函数 ⇒ 没有人能**不连库**校验它。
/// QA 的实测说明代价有多大：把 echo 里的租户条件**全删掉**（执行不变），
/// lib 516 / sparql 17 / live 3 **全绿**。
///
/// 抽出来之后，`sql_echo` 的取值可以被逐字校验（本文件 `#[cfg(test)] mod tests`）：
/// 它**逐字等于** `planned.sql`（[`exec::run`] 收到的也是这同一个字段），
/// 且逐字包含注入条件、`LIMIT N+1` 探针与尾部注释。
/// 任何「让回执好看一点」的后加工都会在那两条判据上变红。
fn receipt(planned: plan::Planned, rows: exec::Rows) -> QueryResult {
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
    QueryResult {
        columns: planned.columns,
        rows: rows.rows,
        row_count,
        truncated,
        // ⛔ 逐字取自 `planned.sql`：**执行用的就是这一个字符串**，
        // 这里不许 replace / 截断 / 美化（N1 的变异正是这一类）。
        sql_echo: planned.sql,
        tenant_applied: planned.tenant_applied,
        tables: planned.tables,
        public_dimensions: planned.public_dimensions,
        warnings,
    }
}

// ══════════════════════════════════════════════════════════════
// N1 判据：`sql_echo` 的内容不是「顺带有的」，而是**被钉住的**
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model_view::{
        ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
    };
    use crate::domain::r2rml::{build_mapping, R2rmlMapping};
    use crate::domain::snapshot::{Evidence, JoinState};
    use crate::domain::ttl::{SchemaTypes, TtlOptions};

    const ONT: &str = "https://ontok.local/demo";

    fn prop(name: &str, column: &str) -> ModelPropertyView {
        ModelPropertyView {
            name: name.into(),
            column: column.into(),
            nullable: false,
            note: None,
            sample: None,
        }
    }

    fn entity(key: &str, props: Vec<ModelPropertyView>) -> ModelEntityView {
        ModelEntityView {
            key: key.into(),
            display_name: key.into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: props,
            evidence: vec![],
        }
    }

    fn link(from: &str, to: &str, column: &str, parent_col: &str) -> ModelLinkView {
        ModelLinkView {
            key: format!("{from}_{column}__{to}"),
            from_type: from.into(),
            to_type: to.into(),
            cardinality: "N:1".into(),
            semantic: "belongs_to".into(),
            evidence: vec![Evidence {
                table: from.into(),
                column: column.into(),
                claim: "外键".into(),
            }],
            confidence: None,
            join_verified: Some(JoinState::Verified),
            join_evidence: Some(format!(
                "value-overlap 5/5 (100.00%) -> {to}.{parent_col} via primary_key (plural)"
            )),
            column: Some(column.into()),
        }
    }

    const SCHEMA: &str = r#"{"tables":[
      {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"customer_name","data_type":"text"}]},
      {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
       "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
                  {"name":"customer_id","data_type":"text"},
                  {"name":"amount","data_type":"numeric(12,4)"}]}
    ]}"#;

    const POLICY: &str = r#"{
      "version": "1.0.0",
      "tenantSubject": "customers",
      "tenantIdColumn": "id",
      "perTable": { "orders": { "tenantColumn": "customer_id" } },
      "publicDimensions": []
    }"#;

    fn fixture() -> (SchemaTypes, R2rmlMapping) {
        let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
        let view = ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                entity("customers", vec![prop("客户名", "customer_name")]),
                entity(
                    "orders",
                    vec![prop("金额", "amount"), prop("客户", "customer_id")],
                ),
            ],
            link_types: vec![link("orders", "customers", "customer_id", "id")],
            action_types: vec![],
            functions: vec![],
        };
        let mut opts = TtlOptions::new("demo", 2);
        opts.schema = Some(&schema);
        let mapping =
            build_mapping(&view, &opts, false, Default::default()).expect("夹具映射必须能生成");
        (schema, mapping)
    }

    fn planned(sparql: &str) -> plan::Planned {
        let (schema, mapping) = fixture();
        let policy = QueryPolicy::parse(POLICY).expect("夹具声明");
        plan::plan(
            sparql,
            &PlanInput {
                mapping: &mapping,
                schema: &schema,
                ontology_iri: ONT,
                policy: &policy,
                tenant: "C0001",
                max_rows: 1000,
            },
        )
        .expect("这条查询应当能规划")
    }

    /// **N1 的主判据**：`sql_echo` 就是执行的那条 SQL，而且注入 / 探针 / 尾部注释都在里面。
    ///
    /// # 变异验证（QA 可复现，命令见下）
    ///
    /// ```text
    /// # 把 receipt() 里的 sql_echo 改成「删掉注入条件」的样子：
    /// #   sql_echo: strip_tenant_conditions(&planned.sql)  （或任何 replace）
    /// # 期望：本测试红，且第一条断言就红（sql_echo != 执行的 SQL）。
    /// ```
    #[test]
    fn sql_echo_is_the_executed_sql_and_shows_the_injection_and_the_probe() {
        let p = planned(r#"SELECT ?o ?a WHERE { ?o :orders__amount ?a }"#);
        let executed_sql = p.sql.clone();
        let res = receipt(p, exec::Rows { rows: vec![], more: false });

        // ① **逐字节**相等：echo 与执行用的字符串是同一个（⛔ 不是「长得像」）
        assert_eq!(
            res.sql_echo, executed_sql,
            "sql_echo 必须逐字等于执行的 SQL —— 回执被再加工过"
        );

        // ② 注入条件在 echo 里，且是 WHERE 的最前面的项（AND 语义）
        assert!(
            res.sql_echo
                .contains(r#"WHERE t0."customer_id" = 'C0001' AND"#),
            "echo 的 WHERE 最前面必须是注入条件：{}",
            res.sql_echo
        );

        // ③ `LIMIT N+1` 探针与尾部注释逐字在 echo 里（验收口径：LIMIT <= max_rows + 1）
        assert!(
            res.sql_echo.contains(
                "LIMIT 1001 /* REQ-122: 上限 1000 行 + 1 行探针（判断 truncated），探针不进结果 */"
            ),
            "echo 必须逐字带 LIMIT N+1 与探针注释：{}",
            res.sql_echo
        );
        assert!(
            res.sql_echo.trim_end().ends_with("探针不进结果 */"),
            "尾部必须是那句探针注释：{}",
            res.sql_echo
        );
    }

    /// 每一条 `tenant_applied` 都必须在 echo 里**恰好**出现一次。
    ///
    /// 这条是上面那条的推广：表多了、join 多了以后，「只删了其中一张表的条件」
    /// 也必须红 —— 单表用例抓不住这种半删。
    #[test]
    fn every_applied_tenant_condition_appears_in_the_echo_exactly_once() {
        let p = planned(
            r#"SELECT ?n WHERE { ?o :orders__customers ?c . ?c :customers__customer_name ?n }"#,
        );
        let res = receipt(p, exec::Rows { rows: vec![], more: false });

        assert_eq!(res.tenant_applied.len(), 2, "两张表都要注入：{:?}", res.tenant_applied);
        for t in &res.tenant_applied {
            let needle = format!("\"{}\" = '{}'", t.column, t.value);
            assert_eq!(
                res.sql_echo.matches(&needle).count(),
                1,
                "表 {} 的注入条件 `{needle}` 必须**恰好**在 echo 里出现一次：{}",
                t.table,
                res.sql_echo
            );
        }
    }

    /// 注入条件的**数量**也必须与 `tenant_applied` 一致（防止 echo 里多出「看起来像注入」的东西）。
    #[test]
    fn the_echo_does_not_invent_tenant_conditions_that_were_not_applied() {
        let p = planned(r#"SELECT ?o ?a WHERE { ?o :orders__amount ?a }"#);
        let res = receipt(p, exec::Rows { rows: vec![], more: false });
        assert_eq!(res.tenant_applied.len(), 1);
        assert_eq!(
            res.sql_echo.matches(r#""customer_id" = 'C0001'"#).count(),
            1,
            "echo 里的注入条件数必须等于 tenant_applied（1）：{}",
            res.sql_echo
        );
    }
}
