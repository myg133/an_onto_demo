//! REQ-122 · **租户声明**（`query-policy.json`）—— 机制是产品的，口径是**这份数据**。
//!
//! # ADR-006 的那条线，落在代码里就这一个文件
//!
//! | 层 | 归属 | 在哪 |
//! | --- | --- | --- |
//! | 机制 | 产品 | 本模块 + [`super::plan`]：读声明 → 逐表决定「过滤列 / 公共维度 / 不可查」→ 注入 |
//! | 声明 | 项目 | `query-policy.json`（租户主体、逐表过滤列、公共维度 + 理由） |
//!
//! ⛔ **本模块（以及整条查询链路）里不出现任何租户列名字面量**。
//! 判据一：`git grep -n '"customer_id"' -- src/` 里没有一处**用于注入决策**；
//! 判据二：把声明里的列名改成 `tenant_no`，注入跟着变、代码一行不动。
//!
//! # 三种决定（只有三种，没有第四种）
//!
//! | 声明状态 | 决定 | 注入 |
//! | --- | --- | --- |
//! | `table == tenantSubject` | 按 `tenantIdColumn` 过滤 | ✅ |
//! | `perTable[table].tenantColumn` | 按**声明写的列**过滤 | ✅ |
//! | `publicDimensions[]` 里点名 | 公共维度，不过滤 | ❌（`sql_echo` 里没有它的条件） |
//! | 都没登记 | **不可查** → 整条查询拒绝（点名表 + 提示登记） | 拒绝 |
//!
//! # 为什么解析是**严格**的
//!
//! `deny_unknown_fields`：拼错一个字段名（`"publicDimension"`）⇒ 报错。
//! 若宽容地忽略未知字段，那份文件会「看起来配了公共维度、实际一张都没生效」，
//! 于是一张客户私有表被当成公共维度放行 —— **猜错 = 跨租户泄漏，且不报错**。

use std::collections::BTreeMap;

use serde::Deserialize;

use super::error::QueryError;

/// 一份租户声明。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryPolicy {
    /// 声明格式版本（本实现只认 `1.x`）。
    pub version: String,
    /// 租户主体类（= 映射里的类 key，通常就是那张「客户表」）。
    #[serde(rename = "tenantSubject")]
    pub tenant_subject: String,
    /// 主体类上的租户标识列（`customers.id` 的 `id`）。
    #[serde(rename = "tenantIdColumn")]
    pub tenant_id_column: String,
    /// 逐表登记：表 → 过滤列（+ 可选理由）。
    #[serde(rename = "perTable", default)]
    pub per_table: BTreeMap<String, TableRule>,
    /// 公共维度：对所有租户都一样的表（产品目录 / 全局配置）。**必须写理由**。
    #[serde(rename = "publicDimensions", default)]
    pub public_dimensions: Vec<PublicDimension>,
    /// 谁审的这份口径（可空，但空就意味着「没人认领这份声明」）。
    #[serde(rename = "reviewed_by", default)]
    pub reviewed_by: Option<String>,
    /// 自由说明（给人看）。
    #[serde(default)]
    pub notes: Option<String>,
}

/// 一张表的登记。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableRule {
    /// 过滤列名（真实 schema 里的列名，原样）。
    #[serde(rename = "tenantColumn")]
    pub tenant_column: String,
    /// 为什么是这一列（可选，但多租户列的表**应当**写）。
    #[serde(default)]
    pub reason: Option<String>,
}

/// 一个公共维度。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicDimension {
    pub table: String,
    /// **必填**：不写理由的「公共」等于「没人负责」。
    pub reason: String,
}

/// 一张表的租户决定 —— 机制的全部输出只有这三种（+ 拒绝）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableDecision {
    /// 按这一列过滤。
    Filter { column: String, reason: Option<String> },
    /// 公共维度，不过滤。
    Public { reason: String },
    /// 没登记 ⇒ 不可查（调用方负责拒绝并点名）。
    Unregistered,
}

impl QueryPolicy {
    /// 解析 + **自洽性校验**（与映射 / schema 的对账在 [`super::plan`] 里，那里两者都在手）。
    pub fn parse(json: &str) -> Result<Self, QueryError> {
        let policy: QueryPolicy = serde_json::from_str(json)
            .map_err(|e| QueryError::new(format!("租户声明不是合法的 query-policy.json：{e}")))?;
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<(), QueryError> {
        let mut problems: Vec<String> = Vec::new();

        let major = self.version.trim().split('.').next().unwrap_or("");
        if major != "1" {
            problems.push(format!(
                "version = {:?}：本实现只认 1.x 的声明格式（升级格式要配一次迁移）",
                self.version
            ));
        }
        if self.tenant_subject.trim().is_empty() {
            problems.push("tenantSubject 不能为空（它决定「什么是一个租户」）".into());
        }
        if self.tenant_id_column.trim().is_empty() {
            problems.push("tenantIdColumn 不能为空（它是主体表上的租户 id 列）".into());
        }

        for (table, rule) in &self.per_table {
            if table.trim().is_empty() {
                problems.push("perTable 里有一个空表名".into());
            }
            if rule.tenant_column.trim().is_empty() {
                problems.push(format!("perTable.{table}.tenantColumn 不能为空"));
            }
            if let Some(public) = self
                .public_dimensions
                .iter()
                .find(|p| same_table(&p.table, table))
            {
                problems.push(format!(
                    "表 {table} 同时出现在 perTable 和 publicDimensions（理由：{}）—— \
                     一份声明里不能既说「按列过滤」又说「全局公共」",
                    public.reason
                ));
            }
        }

        let mut seen: Vec<String> = Vec::new();
        for p in &self.public_dimensions {
            if p.table.trim().is_empty() {
                problems.push("publicDimensions 里有一个空表名".into());
                continue;
            }
            if p.reason.trim().is_empty() {
                problems.push(format!(
                    "publicDimensions 里的 {} 没有写 reason —— 不写理由的「公共」等于没人负责",
                    p.table
                ));
            }
            if seen.iter().any(|t| same_table(t, &p.table)) {
                problems.push(format!("publicDimensions 里 {} 重复登记", p.table));
            }
            seen.push(p.table.clone());
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(QueryError::new(format!(
                "租户声明不合法（{} 处）：\n  - {}",
                problems.len(),
                problems.join("\n  - ")
            )))
        }
    }

    /// 逐表决定：**过滤 / 公共 / 不可查**。
    pub fn decide(&self, table: &str) -> TableDecision {
        if same_table(table, &self.tenant_subject) {
            return TableDecision::Filter {
                column: self.tenant_id_column.clone(),
                reason: Some("tenantSubject —— 租户主体表按 tenantIdColumn 过滤".into()),
            };
        }
        if let Some((_, rule)) = self
            .per_table
            .iter()
            .find(|(t, _)| same_table(t, table))
        {
            return TableDecision::Filter {
                column: rule.tenant_column.clone(),
                reason: rule.reason.clone(),
            };
        }
        if let Some(p) = self
            .public_dimensions
            .iter()
            .find(|p| same_table(&p.table, table))
        {
            return TableDecision::Public { reason: p.reason.clone() };
        }
        TableDecision::Unregistered
    }

    /// 声明里登记的**全部**表名（含主体 / 逐表 / 公共维度）—— 对账用。
    pub fn declared_tables(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        out.push(self.tenant_subject.as_str());
        out.extend(self.per_table.keys().map(|s| s.as_str()));
        out.extend(self.public_dimensions.iter().map(|p| p.table.as_str()));
        out
    }
}

/// 表名比较：trim + 大小写不敏感（schema 里全是小写，声明由人写）。
fn same_table(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// **声明模板**（AC-004：缺声明时不仅要报错，还要给出模板）。
///
/// 编译期内嵌 `query-policy.example.json` —— ⛔ 不在运行时去猜仓库在哪。
/// 文件不存在 ⇒ **编译失败**（比「运行时找不到模板」早一步暴露）。
pub const TEMPLATE: &str = include_str!("../../query-policy.example.json");

/// 模板文件名（报错里给的「路径」）。
pub const TEMPLATE_FILE: &str = "query-policy.example.json";

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> String {
        r#"{
          "version": "1.0.0",
          "tenantSubject": "customers",
          "tenantIdColumn": "id",
          "perTable": {
            "deductions": { "tenantColumn": "main_owner_id", "reason": "子账号归主账号" },
            "balances": { "tenantColumn": "owner_id" }
          },
          "publicDimensions": [
            { "table": "package_templates", "reason": "套餐定义是产品目录" }
          ],
          "reviewed_by": "业务方"
        }"#
        .to_string()
    }

    #[test]
    fn the_three_decisions_are_the_only_three() {
        let p = QueryPolicy::parse(&base()).unwrap();
        assert_eq!(
            p.decide("customers"),
            TableDecision::Filter {
                column: "id".into(),
                reason: Some("tenantSubject —— 租户主体表按 tenantIdColumn 过滤".into())
            }
        );
        assert_eq!(
            p.decide("deductions"),
            TableDecision::Filter {
                column: "main_owner_id".into(),
                reason: Some("子账号归主账号".into())
            }
        );
        assert_eq!(
            p.decide("package_templates"),
            TableDecision::Public { reason: "套餐定义是产品目录".into() }
        );
        assert_eq!(p.decide("balances_archive"), TableDecision::Unregistered);
    }

    #[test]
    fn table_names_are_compared_case_insensitively_and_trimmed() {
        let p = QueryPolicy::parse(&base()).unwrap();
        assert!(matches!(p.decide(" Deductions "), TableDecision::Filter { .. }));
        assert!(matches!(p.decide("PACKAGE_TEMPLATES"), TableDecision::Public { .. }));
    }

    #[test]
    fn a_typo_in_a_field_name_is_an_error_not_a_silent_default() {
        let raw = base().replace("\"perTable\"", "\"perTables\"");
        let e = QueryPolicy::parse(&raw).unwrap_err();
        assert!(e.message().contains("perTables"), "{e}");
        assert!(e.message().contains("query-policy.json"), "{e}");
    }

    #[test]
    fn a_public_dimension_without_a_reason_is_rejected() {
        let raw = base().replace("\"reason\": \"套餐定义是产品目录\"", "\"reason\": \"  \"");
        let e = QueryPolicy::parse(&raw).unwrap_err();
        assert!(e.message().contains("没有写 reason"), "{e}");
    }

    #[test]
    fn the_same_table_cannot_be_both_filtered_and_public() {
        let raw = base().replace("\"balances\": { \"tenantColumn\": \"owner_id\" }", "\"balances\": { \"tenantColumn\": \"owner_id\" }, \"package_templates\": { \"tenantColumn\": \"id\" }");
        let e = QueryPolicy::parse(&raw).unwrap_err();
        assert!(e.message().contains("同时出现在"), "{e}");
    }

    #[test]
    fn an_unknown_major_version_is_rejected() {
        let e = QueryPolicy::parse(&base().replace("\"1.0.0\"", "\"2.0.0\"")).unwrap_err();
        assert!(e.message().contains("只认 1.x"), "{e}");
    }

    #[test]
    fn an_empty_tenant_subject_is_rejected() {
        let e = QueryPolicy::parse(&base().replace("\"customers\"", "\" \"")).unwrap_err();
        assert!(e.message().contains("tenantSubject"), "{e}");
    }

    #[test]
    fn the_shipped_template_parses_and_explains_itself() {
        let p = QueryPolicy::parse(TEMPLATE).expect("模板必须是一份合法声明");
        assert_eq!(p.version, "1.0.0");
        assert!(p.notes.as_deref().is_some_and(|n| n.contains("机制")));
        // 模板里的占位表名必须**不是**任何真实表 —— 拿它直接跑会「不可查」，
        // 从而不会悄悄放过一张没登记的表。
        assert!(matches!(p.decide("some_customer_table"), TableDecision::Filter { .. }));
        assert!(matches!(p.decide("some_public_catalog"), TableDecision::Public { .. }));
    }
}
