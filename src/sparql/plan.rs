//! REQ-122 · **规划器** —— SPARQL 子集 → 一张只读 SQL（含租户强制注入）。
//!
//! # 它做四件事，顺序是刻意的
//!
//! | # | 事 | 失败时 |
//! | --- | --- | --- |
//! | ① | 把每条三元组对到 **R2RML 映射**（类 → 表、属性 → 列、关系 → joinCondition） | 未知类 / 未知属性 ⇒ **报错**（列出可用项；⛔ 不返回空结果） |
//! | ② | 把 BGP 连成**一棵 join 树**（沿 `rr:joinCondition`） | 两段模式互不相连 ⇒ 拒绝（⛔ 不隐式笛卡尔积） |
//! | ③ | 按**声明**逐表决定租户过滤 | 任一表无法确定 ⇒ **整条拒绝**，点名表（fail-closed） |
//! | ④ | 投影 / 分组 / 排序 / 上限 → SQL 文本 | 任一项超出 v1 子集 ⇒ 报错 |
//!
//! # 注入只有一处实现（纪律 H）
//!
//! 租户条件在 [`Planner::decide_tenants`] 里产生、[`Planner::where_clause`] 里**统一**拼进
//! `WHERE` 的**最前面**，与用户条件用 `AND` 连接。没有第二处「顺便也加一下」的代码 ——
//! 两处实现就会有一条路径被忘掉，而被忘掉的那次**不报错**。
//!
//! # 为什么注入条件排在最前面
//!
//! ① 读 `sql_echo` 的人**第一眼**就能看见（AC-004/R5 的判据是文本，不是「代码里有」）；
//! ② 它是**整条 SQL 的前提**，不是可选的优化项 —— 排在后面会读成「附加过滤」。
//!
//! # 反向关系
//!
//! R2RML 只写前向 join（`child` → `parent`）。反向查询（父类当主语）用
//! [`crate::domain::r2rml::MappedJoin::inverse_local`] 在**同一条** join 上反解，
//! ⛔ 不另算一份命名（那会让 TTL 里的反向属性名与引擎认的名字漂移）。

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::domain::r2rml::{MappedClass, MappedJoin, MappedProperty, R2rmlMapping};
use crate::domain::ttl::{SchemaTypes, XsdType};

use super::error::QueryError;
use super::policy::{QueryPolicy, TableDecision};
use super::query::{
    self, AggFunc, CmpOp, Expr, Literal, ObjectTerm, Operand, Projection, Query, SelectItem,
    SubjectTerm,
};
use super::sql;

/// RDF 的 `rdf:type`（`a` 展开后的 IRI）。
pub const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// 规划输入。
pub struct PlanInput<'a> {
    pub mapping: &'a R2rmlMapping,
    pub schema: &'a SchemaTypes,
    /// `TtlOptions::ontology_iri()`（与 TTL / R2RML 同一个）。
    pub ontology_iri: &'a str,
    pub policy: &'a QueryPolicy,
    pub tenant: &'a str,
    /// `--max-rows`（默认 1000）。
    pub max_rows: u64,
}

/// 一条被注入的租户条件（回执里要能逐表核对）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantApplied {
    pub table: String,
    pub column: String,
    pub value: String,
}

/// 一个**不过滤**的公共维度（回执里要能看出「为什么这张表没有条件」）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PublicDimensionUsed {
    pub table: String,
    pub reason: String,
}

/// ⭐ REQ-140 · 一个投影列**在 RDF 里是什么**。
///
/// # 为什么必须由规划器说出来（⛔ 不从列名猜）
///
/// 「这一列是实例 IRI 还是字面量」「它的谓词本地名是什么」「它的 xsd 类型是什么」——
/// 三件事在**绑定变量的那一刻**才确定，而列名里一个都没有：
///
/// - `?m` 的值是 `https://…#llm_model/<uuid>`，列名只有 `m`；
/// - 谓词本地名是 `llm_model__model_name`（`rr:predicate`），⛔ 不是列名 `model_name`；
/// - xsd 类型来自 `rr:datatype`（模型轨的 range 推断），⛔ 不是 `::text`。
///
/// 所以 JSON-LD 渲染只吃这里的事实（[`crate::query::jsonld`]），字符串前缀只做兜底。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnKind {
    /// 类变量：值就是**完整实例 IRI**（`iri_sql` 拼的 `{ontology_iri}#{类}/{主键}`）。
    ///
    /// `class` = 类 key（= 表名），`class_local` = RDF 类本地名（`:<local>`，与 TTL 同一个）。
    Node { class: String, class_local: String },
    /// 数据类型属性的值：类 key / 列名 / `rr:predicate` 本地名 / `rr:datatype` 的本地名。
    Literal {
        class: String,
        column: String,
        predicate: String,
        xsd: &'static str,
    },
    /// 聚合（COUNT / SUM / MIN / MAX / AVG）：**没有实体可挂** ⇒ 进 `summary`。
    Aggregate { func: &'static str },
    /// 理论上不可达（投影项要么带聚合函数，要么有取值）；渲染侧与 [`Self::Aggregate`] 同路。
    Unknown,
}

/// ⭐ REQ-140 · 投影列 = 列名 + [`ColumnKind`]（与 [`Planned::columns`] 同序同长）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedColumn {
    pub name: String,
    pub kind: ColumnKind,
}

/// 规划结果 —— SQL 已就绪，等执行。
#[derive(Debug, Clone)]
pub struct Planned {
    pub sql: String,
    pub columns: Vec<String>,
    /// ⭐ REQ-140 · 每个投影列的 `var → 类 / 谓词 / xsd` 映射（JSON-LD 子图的**唯一**类型来源）。
    pub projection: Vec<ProjectedColumn>,
    /// 涉及的表（FROM 顺序，去重）。
    pub tables: Vec<String>,
    pub tenant_applied: Vec<TenantApplied>,
    pub public_dimensions: Vec<PublicDimensionUsed>,
    pub warnings: Vec<String>,
    /// `--max-rows` 压过 LIMIT ⇒ 需要多取一行来判断是否真的截断。
    pub truncated_possible: bool,
    /// 实际写进 SQL 的 LIMIT。
    pub effective_limit: u64,
}

/// 规划：SPARQL → SQL。纯函数（不连库、不看时间）。
pub fn plan(sparql: &str, input: &PlanInput<'_>) -> Result<Planned, QueryError> {
    let q = query::parse(sparql, input.ontology_iri)?;
    let mut warnings: Vec<String> = Vec::new();
    check_declaration(input, &mut warnings)?;
    let mut p = Planner::new(input, warnings);
    p.resolve_patterns(&q)?;
    p.connect_nodes()?;
    p.decide_tenants()?;
    p.finish(&q)
}

// ══════════════════════════════════════════════════════════════
// 声明 ↔ 映射 / schema 对账（**在规划之前**，fail-closed）
// ══════════════════════════════════════════════════════════════

fn check_declaration(input: &PlanInput<'_>, warnings: &mut Vec<String>) -> Result<(), QueryError> {
    let policy = input.policy;
    let subject = policy.tenant_subject.trim();

    let class = input
        .mapping
        .class(subject)
        .or_else(|| input.mapping.class_by_local(subject))
        .ok_or_else(|| {
            QueryError::new(format!(
                "声明里的 tenantSubject = {subject:?} 不是本版映射里的类 —— \
                 租户主体必须是映射里真实存在的类（通常就是客户表）。\n  \
                 本版有 {} 个类，例如：{}",
                input.mapping.classes().len(),
                class_hint(input.mapping)
            ))
        })?;

    let id_column = policy.tenant_id_column.trim();
    if !input.schema.has_column(&class.key, id_column) {
        return Err(QueryError::new(format!(
            "声明里的 tenantIdColumn = {id_column:?} 在表 {} 里不存在 —— \
             租户标识列必须是真实列。\n  这张表的列：{}",
            class.key,
            column_hint(input.schema, &class.key)
        )));
    }

    // 逐表登记：**表在 schema 里、列却不存在** ⇒ 声明写错了（fail-closed）。
    // 表根本不在 schema 里 ⇒ 可能这份声明是给别的版本/项目用的，只警告。
    for (table, rule) in &policy.per_table {
        let column = rule.tenant_column.trim();
        if input.schema.has_table(table) && !input.schema.has_column(table, column) {
            return Err(QueryError::new(format!(
                "声明里登记的 {table}.{column} 在 schema 里不存在 —— ⛔ 不会「跳过这一条继续跑」\
                 （那等于这张表没有租户过滤）。\n  这张表的列：{}",
                column_hint(input.schema, table)
            )));
        }
        if !input.mapping.class(table).is_some() {
            warnings.push(format!(
                "声明里登记的表 {table} 不在本版映射里（可能已排除，或声明是给别的版本写的）"
            ));
        }
    }
    for p in &policy.public_dimensions {
        if input.mapping.class(&p.table).is_none() {
            warnings.push(format!(
                "声明里的公共维度 {} 不在本版映射里（可能已排除，或声明是给别的版本写的）",
                p.table
            ));
        }
    }

    // R2RML 的 `ontok:tenantColumn` 是 REQ-121 的标注，**口径以声明为准**；
    // 两者不一致时必须让人看见（否则会以为映射里的标注在生效）。
    for class in input.mapping.classes() {
        if class.tenant_columns.is_empty() {
            continue;
        }
        if let TableDecision::Filter { column, .. } = policy.decide(&class.key) {
            if !class.tenant_columns.iter().any(|c| c.eq_ignore_ascii_case(column.trim())) {
                warnings.push(format!(
                    "表 {} 在映射里标了 ontok:tenantColumn {}，而声明里写的是 {} —— \
                     以声明为准（REQ-121 的标注只是给人看的提示）",
                    class.key,
                    class.tenant_columns.join("、"),
                    column
                ));
            }
        }
    }

    Ok(())
}

fn class_hint(mapping: &R2rmlMapping) -> String {
    let mut names: Vec<String> =
        mapping.classes().iter().map(|c| format!(":{}", c.class_local)).collect();
    let total = names.len();
    names.truncate(24);
    let more = if total > names.len() { format!(" 等 {total} 个") } else { String::new() };
    format!("{}{more}", names.join("、"))
}

fn column_hint(schema: &SchemaTypes, table: &str) -> String {
    schema.column_names(table).unwrap_or_default().join("、")
}

// ══════════════════════════════════════════════════════════════
// 规划器
// ══════════════════════════════════════════════════════════════

/// 一张表的一个实例（= 一个类的一次出现）。
#[derive(Debug, Clone)]
struct Node {
    class_key: String,
    alias: String,
}

/// 列引用（表别名 + 列）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ColRef {
    node: usize,
    column: String,
}

/// 一条 join 边：`t_a.col_a = t_b.col_b`（方向无关，只是两侧）。
#[derive(Debug, Clone)]
struct Edge {
    a: usize,
    col_a: String,
    b: usize,
    col_b: String,
}

/// 规划期产生的约束（**不含**用户 FILTER —— 那些要等变量绑定完再渲染）。
#[derive(Debug, Clone)]
enum Cond {
    /// R2RML 里 NULL 不产生三元组 ⇒ 变量宾语要求 `IS NOT NULL`。
    NotNull(ColRef),
    /// 列 与 字面量。
    ColLit(ColRef, CmpOp, Literal),
    /// 列 与 实例 id（由实例 IRI 反解出来的字符串）。
    ColId(ColRef, CmpOp, String),
}

struct Planner<'a> {
    mapping: &'a R2rmlMapping,
    schema: &'a SchemaTypes,
    policy: &'a QueryPolicy,
    ontology_iri: &'a str,
    tenant: &'a str,
    max_rows: u64,
    warnings: Vec<String>,

    nodes: Vec<Node>,
    node_of: BTreeMap<String, usize>,
    var_node: BTreeMap<String, usize>,
    var_col: BTreeMap<String, ColRef>,
    edges: Vec<Edge>,
    conds: Vec<Cond>,

    /// `(node, 列)` —— 注入条件（**唯一一处**记录）。
    injected: Vec<(usize, String)>,
    tenant_applied: Vec<TenantApplied>,
    public_used: Vec<PublicDimensionUsed>,
}

impl<'a> Planner<'a> {
    fn new(input: &PlanInput<'a>, warnings: Vec<String>) -> Self {
        Self {
            mapping: input.mapping,
            schema: input.schema,
            policy: input.policy,
            ontology_iri: input.ontology_iri,
            tenant: input.tenant,
            max_rows: input.max_rows,
            warnings,
            nodes: Vec::new(),
            node_of: BTreeMap::new(),
            var_node: BTreeMap::new(),
            var_col: BTreeMap::new(),
            edges: Vec::new(),
            conds: Vec::new(),
            injected: Vec::new(),
            tenant_applied: Vec::new(),
            public_used: Vec::new(),
        }
    }

    // ── ① 逐条三元组对到映射 ──────────────────────────────────

    fn resolve_patterns(&mut self, q: &Query) -> Result<(), QueryError> {
        for pat in &q.patterns {
            if pat.predicate == RDF_TYPE {
                self.resolve_type_pattern(pat)?;
                continue;
            }
            let local = self.local_of(&pat.predicate, "属性")?;
            match self.classify_predicate(&local)? {
                PredKind::Datatype(class, prop) => self.resolve_datatype(pat, class, prop)?,
                PredKind::Forward(child, join) => self.resolve_join(pat, child, join, true)?,
                PredKind::Inverse(_child, join) => {
                    // 反向：主语是**父类**（rr:parent 那一侧）。
                    let parent = self.mapping.class(&join.parent_table).ok_or_else(|| {
                        QueryError::new(format!(
                            "映射自相矛盾：join :{} 的父表 {} 不在映射里",
                            join.predicate_local, join.parent_table
                        ))
                    })?;
                    self.resolve_join(pat, parent, join, false)?
                }
            }
        }
        Ok(())
    }

    fn resolve_type_pattern(
        &mut self,
        pat: &query::TriplePattern,
    ) -> Result<(), QueryError> {
        let ObjectTerm::Iri(class_iri) = &pat.object else {
            return Err(QueryError::new(format!(
                "v1 不支持 `a` 的宾语是 {} —— 类必须是写死的 IRI（如 `:customers`）",
                match &pat.object {
                    ObjectTerm::Var(v) => format!("变量 ?{v}"),
                    ObjectTerm::Literal(l) => format!("字面量 {:?}", l.text),
                    ObjectTerm::Iri(_) => unreachable!(),
                }
            )));
        };
        let class = self.class_of(class_iri)?;
        match &pat.subject {
            SubjectTerm::Var(v) => {
                self.bind_var_node(v, &class.key)?;
            }
            SubjectTerm::Iri(iri) => {
                let node = self.bind_node(&class.key);
                let id = self.instance_id(iri, class)?;
                self.conds.push(Cond::ColId(
                    ColRef { node, column: class.pk.clone() },
                    CmpOp::Eq,
                    id,
                ));
            }
        }
        Ok(())
    }

    fn resolve_datatype(
        &mut self,
        pat: &query::TriplePattern,
        class: &'a MappedClass,
        prop: &'a MappedProperty,
    ) -> Result<(), QueryError> {
        let subject_id = match &pat.subject {
            SubjectTerm::Var(v) => {
                self.bind_var_node(v, &class.key)?;
                None
            }
            SubjectTerm::Iri(iri) => Some(self.instance_id(iri, class)?),
        };
        let node = self.bind_node(&class.key);
        if let Some(id) = subject_id {
            self.conds.push(Cond::ColId(
                ColRef { node, column: class.pk.clone() },
                CmpOp::Eq,
                id,
            ));
        }
        let col = ColRef { node, column: prop.column.clone() };
        match &pat.object {
            ObjectTerm::Var(v) => {
                self.bind_var_col(v, col.clone(), &class.key)?;
                self.conds.push(Cond::NotNull(col));
            }
            ObjectTerm::Literal(lit) => {
                self.conds.push(Cond::ColLit(col, CmpOp::Eq, lit.clone()));
            }
            ObjectTerm::Iri(iri) => {
                return Err(QueryError::new(format!(
                    "{iri} 是实例 IRI，但 :{} 是**数据类型属性**（映射到 {}.{}）—— \
                     它的宾语只能是字面量（字符串/数字/true/false）",
                    prop.local, class.key, prop.column
                )))
            }
        }
        Ok(())
    }

    /// 关系谓词：`forward = true` 时主语是子类、宾语是父类；反向则对调。
    fn resolve_join(
        &mut self,
        pat: &query::TriplePattern,
        subject_class: &'a MappedClass,
        join: &'a MappedJoin,
        forward: bool,
    ) -> Result<(), QueryError> {
        // 主语侧的类 + 主语侧的 join 列；宾语侧的类 + 宾语侧的 join 列。
        let (obj_class_key, subj_col, obj_col) = if forward {
            (join.parent_table.clone(), join.child.clone(), join.parent_column.clone())
        } else {
            // 反向：主语是父类（按 rr:parent），宾语是子类（按 rr:child）
            (join.child_table.clone(), join.parent_column.clone(), join.child.clone())
        };
        let obj_class = self.mapping.class(&obj_class_key).ok_or_else(|| {
            QueryError::new(format!("映射自相矛盾：类 {obj_class_key} 不在映射里"))
        })?;

        let subj_node = match &pat.subject {
            SubjectTerm::Var(v) => self.bind_var_node(v, &subject_class.key)?,
            SubjectTerm::Iri(iri) => {
                let node = self.bind_node(&subject_class.key);
                let id = self.instance_id(iri, subject_class)?;
                self.conds.push(Cond::ColId(
                    ColRef { node, column: subject_class.pk.clone() },
                    CmpOp::Eq,
                    id,
                ));
                node
            }
        };

        match &pat.object {
            ObjectTerm::Var(v) => {
                let obj_node = self.bind_var_node(v, &obj_class.key)?;
                if subj_node == obj_node {
                    // `?x :a__b ?x` —— 自连接。v1 不支持（同一张表需要两个别名）
                    return Err(QueryError::new(
                        "v1 不支持把关系的两端写成同一个变量（自连接需要两个表别名）",
                    ));
                }
                self.edges.push(Edge {
                    a: subj_node,
                    col_a: subj_col,
                    b: obj_node,
                    col_b: obj_col,
                });
            }
            ObjectTerm::Iri(iri) => {
                let id = self.instance_id(iri, obj_class)?;
                self.conds.push(Cond::ColId(
                    ColRef { node: subj_node, column: subj_col },
                    CmpOp::Eq,
                    id,
                ));
            }
            ObjectTerm::Literal(lit) => {
                return Err(QueryError::new(format!(
                    "关系谓词 :{} 的宾语必须是实例 IRI 或变量，不能是字面量 {:?}",
                    join.predicate_local, lit.text
                )))
            }
        }
        Ok(())
    }

    // ── 名字解析（类型 / 属性 / 实例 IRI）────────────────────────

    fn local_of(&self, iri: &str, what: &str) -> Result<String, QueryError> {
        let ns = format!("{}#", self.ontology_iri);
        match iri.strip_prefix(&ns) {
            Some(local) if !local.is_empty() && !local.contains('/') && !local.contains('#') => {
                Ok(local.to_string())
            }
            _ => Err(QueryError::new(format!(
                "{what} IRI <{iri}> 不在本体命名空间 <{ns}> 里 —— \
                 v1 只认本体自己的{}（前缀 `:` 或写全 IRI）",
                if what == "属性" { "属性" } else { what }
            ))),
        }
    }

    fn class_of(&self, iri: &str) -> Result<&'a MappedClass, QueryError> {
        let local = self.local_of(iri, "类")?;
        self.mapping.class_by_local(&local).ok_or_else(|| {
            QueryError::new(format!(
                "v1 里没有类 :{local} —— 未知类不会返回空结果，而是报错。\n  \
                 本版的类（{} 个）：{}\n  \
                 用 `ontok ontology show --project <P> --version <V> --model-view` 可看全量",
                self.mapping.classes().len(),
                class_hint(self.mapping)
            ))
        })
    }

    /// 实例 IRI → id（按映射里的实例 IRI 模板反解）。
    fn instance_id(&self, iri: &str, class: &MappedClass) -> Result<String, QueryError> {
        let prefix = format!("{}#{}/", self.ontology_iri, class.key);
        match iri.strip_prefix(&prefix) {
            Some(id) if !id.is_empty() && !id.contains('/') && !id.contains('#') => {
                Ok(id.to_string())
            }
            _ => Err(QueryError::new(format!(
                "实例 IRI <{iri}> 与映射里的模板不匹配：类 {} 的实例 IRI 形如 <{prefix}<{}>>",
                class.key, class.pk
            ))),
        }
    }

    /// 谓词本地名 → 属性 / 前向 join / 反向 join（**歧义就报错**，不猜）。
    fn classify_predicate(&self, local: &str) -> Result<PredKind<'a>, QueryError> {
        let mut hits: Vec<PredKind<'a>> = Vec::new();
        for class in self.mapping.classes() {
            if let Some(prop) = class.property(local) {
                hits.push(PredKind::Datatype(class, prop));
            }
            if let Some(join) = class.join(local) {
                hits.push(PredKind::Forward(class, join));
            }
            if let Some(join) = class.joins.iter().find(|j| j.inverse_local == local) {
                hits.push(PredKind::Inverse(class, join));
            }
        }
        match hits.len() {
            1 => Ok(hits.remove(0)),
            0 => Err(self.unknown_predicate(local)),
            _ => {
                let kinds: Vec<String> = hits
                    .iter()
                    .map(|h| match h {
                        PredKind::Datatype(c, p) => {
                            format!("{}.{} 的数据类型属性", c.key, p.column)
                        }
                        PredKind::Forward(c, j) => {
                            format!("{} → {} 的前向关系", c.key, j.parent_table)
                        }
                        PredKind::Inverse(c, j) => {
                            format!("{} → {} 的反向关系", c.key, j.parent_table)
                        }
                    })
                    .collect();
                Err(QueryError::new(format!(
                    "属性 :{local} 在本版映射里有歧义（{}）—— 请改用完整 IRI 或换一个名字。",
                    kinds.join("；")
                )))
            }
        }
    }

    /// 未知属性 ⇒ **报错 + 可用清单**（R2：⛔ 不许返回空结果）。
    fn unknown_predicate(&self, local: &str) -> QueryError {
        // 前缀里带着类名（`:deductions__xxx`）就先给这个类的属性清单 —— 那是最可能的笔误。
        let owner = local.split("__").next().unwrap_or("");
        if let Some(class) = self.mapping.class_by_local(owner) {
            let mut names: Vec<String> = class
                .properties
                .iter()
                .map(|p| format!(":{}", p.local))
                .collect();
            for j in &class.joins {
                names.push(format!(":{}", j.predicate_local));
                names.push(format!(":{}（反向）", j.inverse_local));
            }
            let total = names.len();
            names.truncate(30);
            let more = if total > names.len() { format!(" 等 {total} 个") } else { String::new() };
            return QueryError::new(format!(
                "属性 :{local} 不存在 —— 类 {} 里没有这个属性。\n  它有的属性：{}{more}\n  \
                 用 `ontok ontology show --project <P> --version <V> --model-view` 可看全量",
                class.key,
                names.join("、")
            ));
        }
        QueryError::new(format!(
            "属性 :{local} 不存在 —— 本版映射里 {}{}都没有这个谓词。\n  \
             本版的类（{} 个）：{}\n  \
             用 `ontok ontology show --project <P> --version <V> --model-view` 可看全量",
            if owner.is_empty() { String::new() } else { format!("也没有类 :{owner}，") },
            "",
            self.mapping.classes().len(),
            class_hint(self.mapping)
        ))
    }

    // ── 变量绑定 ───────────────────────────────────────────────

    fn bind_node(&mut self, class_key: &str) -> usize {
        if let Some(i) = self.node_of.get(class_key) {
            return *i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            class_key: class_key.to_string(),
            alias: sql::alias(i),
        });
        self.node_of.insert(class_key.to_string(), i);
        i
    }

    fn bind_var_node(&mut self, var: &str, class_key: &str) -> Result<usize, QueryError> {
        let node = self.bind_node(class_key);
        if let Some(existing) = self.var_node.get(var) {
            if *existing != node {
                let other = &self.nodes[*existing].class_key;
                return Err(QueryError::new(format!(
                    "变量 ?{var} 同时指向两个类：{other} 与 {class_key} —— \
                     v1 没有 UNION / OPTIONAL，一个变量只能属于一个类。\
                     要么拆成两条查询，要么用映射里已有的关系把它们连起来"
                )));
            }
        }
        if let Some(col) = self.var_col.get(var) {
            return Err(QueryError::new(format!(
                "变量 ?{var} 既被当作字面量（{}.{}），又被当作实例（{class_key}）—— \
                 v1 不支持这种混用",
                self.nodes[col.node].class_key, col.column
            )));
        }
        self.var_node.insert(var.to_string(), node);
        Ok(node)
    }

    fn bind_var_col(
        &mut self,
        var: &str,
        col: ColRef,
        class_key: &str,
    ) -> Result<(), QueryError> {
        if let Some(node) = self.var_node.get(var) {
            return Err(QueryError::new(format!(
                "变量 ?{var} 既是实例（{}），又是 {class_key}.{} 的字面量 —— \
                 v1 不支持这种混用",
                self.nodes[*node].class_key, col.column
            )));
        }
        if let Some(existing) = self.var_col.get(var) {
            if *existing != col {
                return Err(QueryError::new(format!(
                    "变量 ?{var} 同时被绑到两列（{}.{} 与 {}.{}）—— \
                     v1 不支持「两个属性相等」这种模式",
                    self.nodes[existing.node].class_key,
                    existing.column,
                    class_key,
                    col.column
                )));
            }
        }
        self.var_col.insert(var.to_string(), col);
        Ok(())
    }

    // ── ② 连成 join 树 ─────────────────────────────────────────

    fn connect_nodes(&mut self) -> Result<(), QueryError> {
        if self.nodes.is_empty() {
            return Err(QueryError::new("查询里没有任何可执行的模式"));
        }
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        seen.insert(0);
        let mut used: BTreeSet<usize> = BTreeSet::new();
        loop {
            let mut progressed = false;
            for n in 0..self.nodes.len() {
                if seen.contains(&n) {
                    continue;
                }
                let candidate = self
                    .edges
                    .iter()
                    .enumerate()
                    .find(|(i, e)| {
                        !used.contains(i)
                            && ((e.a == n && seen.contains(&e.b))
                                || (e.b == n && seen.contains(&e.a)))
                    })
                    .map(|(i, e)| (i, e.clone()));
                if let Some((i, _)) = candidate {
                    used.insert(i);
                    seen.insert(n);
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        if seen.len() != self.nodes.len() {
            let missing: Vec<String> = (0..self.nodes.len())
                .filter(|i| !seen.contains(i))
                .map(|i| format!(":{}", self.nodes[i].class_key))
                .collect();
            let connected: Vec<String> = (0..self.nodes.len())
                .filter(|i| seen.contains(i))
                .map(|i| format!(":{}", self.nodes[i].class_key))
                .collect();
            return Err(QueryError::new(format!(
                "查询里有互不相连的模式：{} 与 {} 之间没有 join。\n  \
                 ⛔ v1 拒绝隐式笛卡尔积（那会把不同租户/不同实体的行乘起来，而且不报错）。\n  \
                 请沿映射里已有的关系把它们连起来（`?a :类__父类 ?b`），或拆成两条查询。",
                missing.join("、"),
                connected.join("、")
            )));
        }
        Ok(())
    }

    // ── ③ 租户注入（**唯一**一处）──────────────────────────────

    fn decide_tenants(&mut self) -> Result<(), QueryError> {
        let mut unregistered: Vec<String> = Vec::new();
        for i in 0..self.nodes.len() {
            let table = self.nodes[i].class_key.clone();
            match self.policy.decide(&table) {
                TableDecision::Filter { column, reason, coverage, coverage_note } => {
                    let column = column.trim().to_string();
                    // 声明里写了列、schema 里没有 —— check_declaration 已经拦过；
                    // 这里再拦一次是因为 **注入的列必须真的存在**（宁可多一次）。
                    if !self.schema.has_column(&table, &column) {
                        return Err(QueryError::new(format!(
                            "租户过滤列 {table}.{column}（来自声明）在 schema 里不存在 —— \
                             ⛔ 不会「跳过这一条继续跑」"
                        )));
                    }
                    // ⭐ I-1 · 覆盖范围不完整 ⇒ **必须喊出来**（⛔ 不许让调用方把
                    // 「主账号少看到钱」读成「本来就没有」）。这一段是机制，口径来自声明。
                    if coverage.is_incomplete() {
                        // 警告正文优先用声明里的短句 `coverageNote`；没写就退回 `reason`
                        // （reason 是评审长文，塞进 warning 会读不动 —— 但**聊胜于无**，
                        // 绝不因为「太长」而闭嘴）。
                        let why = coverage_note
                            .as_deref()
                            .or(reason.as_deref())
                            .map(str::trim)
                            .filter(|r| !r.is_empty())
                            .unwrap_or("声明未写明代价");
                        self.warnings.push(format!(
                            "表 {table}：租户过滤用 {column}，覆盖范围 = {} —— {why}\
                             （依据见声明里的 reason）—— ⛔ 不要把 0 行或偏小的数字读成「没有业务」",
                            coverage.as_str()
                        ));
                    }
                    self.injected.push((i, column.clone()));
                    self.tenant_applied.push(TenantApplied {
                        table,
                        column,
                        value: self.tenant.trim().to_string(),
                    });
                }
                TableDecision::Public { reason } => {
                    self.public_used.push(PublicDimensionUsed { table, reason });
                }
                TableDecision::Unregistered => unregistered.push(table),
            }
        }
        if !unregistered.is_empty() {
            let list = unregistered.join("、");
            let example = format!("\"{}\"", unregistered[0]);
            return Err(QueryError::new(format!(
                "租户过滤无法确定：表 {list} 既没有在声明里登记过滤列，也不在 publicDimensions —— \
                 ⛔ 整条查询拒绝（fail-closed：没有「猜一个列」或「不过滤放行」这两条路）。\n  \
                 请在 query-policy.json 里登记其中一种：\n    \
                 \"perTable\": {{ {example}: {{ \"tenantColumn\": \"<属于一个租户的那一列>\" }} }}\n    \
                 若这张表对所有租户确实相同（产品目录 / 全局配置）：\n    \
                 \"publicDimensions\": [ {{ \"table\": {example}, \"reason\": \"<为什么它没有租户归属>\" }} ]"
            )));
        }
        if self.tenant.trim().is_empty() {
            return Err(QueryError::new(
                "租户为空：--tenant 必填 —— ⛔ 没有租户就没有隔离（不会退化成「查全部」）",
            ));
        }
        Ok(())
    }

    // ── ④ 投影 / 分组 / 排序 / 分页 → SQL ───────────────────────

    fn finish(&mut self, q: &Query) -> Result<Planned, QueryError> {
        // 投影项先定下来（列名与 SQL 表达式成对）。
        let items: Vec<(String, String, Option<AggFunc>, Option<Val>)> =
            match &q.projection {
                Projection::All => {
                    let mut out = Vec::new();
                    for v in q.vars_in_order() {
                        let val = self.resolve_var(&v)?;
                        let expr = self.value_expr(&val)?;
                        out.push((v, expr, None, Some(val)));
                    }
                    out
                }
                Projection::Items(items) => {
                    let mut out = Vec::new();
                    for it in items {
                        match it {
                            SelectItem::Var(v) => {
                                let val = self.resolve_var(v)?;
                                let expr = self.value_expr(&val)?;
                                out.push((v.clone(), expr, None, Some(val)));
                            }
                            SelectItem::Agg { func, var, distinct, alias } => {
                                let expr = self.agg_expr(*func, var.as_deref(), *distinct)?;
                                out.push((
                                    alias.clone(),
                                    expr,
                                    Some(*func),
                                    None,
                                ));
                            }
                        }
                    }
                    out
                }
            };

        // 聚合与非聚合混用 ⇒ 必须 GROUP BY，且分组键必须是那个非聚合投影变量。
        let has_agg = items.iter().any(|(_, _, f, _)| f.is_some());
        let non_agg: Vec<&str> = items
            .iter()
            .filter(|(_, _, f, _)| f.is_none())
            .map(|(n, _, _, _)| n.as_str())
            .collect();
        let group_expr: Option<String> = match &q.group_by {
            Some(g) => {
                if !non_agg.iter().any(|n| *n == g) {
                    return Err(QueryError::new(format!(
                        "GROUP BY ?{g} 要求 ?{g} 也出现在 SELECT 里 —— \
                         否则结果里没有分组的键，读不出来（v1 只支持按投影列分组）"
                    )));
                }
                if non_agg.iter().any(|n| *n != g) {
                    return Err(QueryError::new(format!(
                        "GROUP BY ?{g} 时，SELECT 里非聚合的变量只能有 ?{g} —— \
                         现在还有 {}。v1 的 GROUP BY 只支持单列",
                        non_agg
                            .iter()
                            .filter(|n| **n != g)
                            .map(|n| format!("?{n}"))
                            .collect::<Vec<_>>()
                            .join("、")
                    )));
                }
                let val = self.resolve_var(g)?;
                Some(self.group_expr_of(&val)?)
            }
            None => {
                if has_agg && !non_agg.is_empty() {
                    return Err(QueryError::new(format!(
                        "聚合（{}）与非聚合变量混用，但没有 GROUP BY —— \
                         要么加 `GROUP BY ?x`，要么把非聚合变量去掉",
                        non_agg.iter().map(|n| format!("?{n}")).collect::<Vec<_>>().join("、")
                    )));
                }
                None
            }
        };

        // SELECT
        let mut select_parts: Vec<String> = Vec::new();
        for (name, expr, func, _) in &items {
            let sql = match func {
                Some(_) => format!("({expr})::text AS {}", sql::ident(name)),
                None => format!("({expr})::text AS {}", sql::ident(name)),
            };
            select_parts.push(sql);
        }
        if select_parts.is_empty() {
            return Err(QueryError::new(
                "SELECT 里一个可投影的变量都没有（`SELECT *` 但模式里没有变量）",
            ));
        }

        // FROM + JOIN
        let (from_clause, extra_conds) = self.from_clause();

        // WHERE：**注入条件在最前面**
        let mut where_parts: Vec<String> = Vec::new();
        for (node, column) in self.injected.clone() {
            where_parts.push(self.injection_sql(node, &column)?);
        }
        for cond in &self.conds {
            where_parts.push(self.cond_sql(cond)?);
        }
        for (a, b) in extra_conds {
            where_parts.push(self.col_col_sql(&a, CmpOp::Eq, &b)?);
        }
        for f in &q.filters {
            where_parts.push(self.filter_sql(f)?);
        }

        // ORDER BY（单列；必须是投影列）
        let order_clause = match &q.order_by {
            Some(o) => {
                let item = items.iter().find(|(n, _, _, _)| n == &o.var);
                let Some((_, expr, func, val)) = item else {
                    return Err(QueryError::new(format!(
                        "ORDER BY ?{} 的 ?{} 必须出现在 SELECT 里（v1 只支持按投影列排序）",
                        o.var, o.var
                    )));
                };
                // `SELECT DISTINCT` 时 PG 要求 ORDER BY 的表达式**出现在投影列表里**；
                // 用输出列名（别名）排序，语义就是「按投影出来的那一列」（排序口径也与
                // 结果列一致 —— 不会出现「排的是 number、显示的是 text」这种错位）。
                let key = if q.distinct {
                    sql::ident(&o.var)
                } else {
                    match (func, val) {
                        (Some(_), _) => expr.clone(),
                        (None, Some(v)) => self.order_expr_of(v)?,
                        _ => unreachable!("投影项要么是聚合，要么有值表达式"),
                    }
                };
                Some(format!("{key}{}", if o.descending { " DESC" } else { "" }))
            }
            None => None,
        };

        // LIMIT：--max-rows 是**硬上限**；多取一行用于判断 truncated。
        let (effective_limit, capped) = match q.limit {
            None => (self.max_rows, true),
            Some(n) if n > self.max_rows => {
                self.warnings.push(format!(
                    "LIMIT {n} 超过 --max-rows {}，已压到 {}（要更多请显式调大 --max-rows）",
                    self.max_rows, self.max_rows
                ));
                (self.max_rows, true)
            }
            Some(n) => (n, false),
        };
        if effective_limit == 0 {
            return Err(QueryError::new(
                "LIMIT 0 等于「什么都没查」，却看起来像「查了没有」—— 要查就给 ≥1",
            ));
        }

        // ── 拼 SQL（**顺序固定**：注入 → 结构约束 → 用户 FILTER）──────
        let mut sql = String::from("SELECT ");
        if q.distinct {
            sql.push_str("DISTINCT ");
        }
        sql.push_str(&select_parts.join(", "));
        sql.push_str(" FROM ");
        sql.push_str(&from_clause);
        sql.push_str(" WHERE ");
        let where_sql = if where_parts.is_empty() {
            "TRUE".to_string()
        } else {
            where_parts.join(" AND ")
        };
        sql.push_str(&where_sql);
        if let Some(g) = &group_expr {
            sql.push_str(" GROUP BY ");
            sql.push_str(g);
        }
        if let Some(o) = &order_clause {
            sql.push_str(" ORDER BY ");
            sql.push_str(o);
        }
        // ⚠️ 多取 1 行：**只有**这样才能区分「正好 1000 行」与「还有更多」。
        // 多取的那一行不进结果；`sql_echo` 逐字包含它（不让回执与执行差一个字节）。
        sql.push_str(&format!(
            " LIMIT {} /* REQ-122: 上限 {} 行 + 1 行探针（判断 truncated），探针不进结果 */",
            effective_limit + 1,
            effective_limit
        ));
        if let Some(o) = q.offset {
            if o > 0 {
                sql.push_str(&format!(" OFFSET {o}"));
            }
        }

        let tables: Vec<String> =
            self.nodes.iter().map(|n| n.class_key.clone()).collect();
        let columns: Vec<String> = items.iter().map(|(n, _, _, _)| n.clone()).collect();
        // ⭐ REQ-140 · 列的 RDF 身份（与 `columns` 同序同长）：JSON-LD 只用这一份事实，
        // ⛔ 不从列名反猜「谁是实体、谁是什么谓词」。
        let projection: Vec<ProjectedColumn> = items
            .iter()
            .map(|(name, _, func, val)| ProjectedColumn {
                name: name.clone(),
                kind: self.column_kind(*func, val.as_ref()),
            })
            .collect();
        debug_assert_eq!(columns.len(), projection.len());

        Ok(Planned {
            sql,
            columns,
            projection,
            tables,
            tenant_applied: self.tenant_applied.clone(),
            public_dimensions: self.public_used.clone(),
            warnings: self.warnings.clone(),
            truncated_possible: capped,
            effective_limit,
        })
    }

    fn from_clause(&self) -> (String, Vec<(ColRef, ColRef)>) {
        let mut out = String::new();
        let mut extra: Vec<(ColRef, ColRef)> = Vec::new();
        // 第一个节点直接 FROM
        out.push_str(&format!(
            "{} {}",
            sql::ident(&self.nodes[0].class_key),
            self.nodes[0].alias
        ));
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        seen.insert(0);
        let mut used: BTreeSet<usize> = BTreeSet::new();
        loop {
            let mut progressed = false;
            for n in 0..self.nodes.len() {
                if seen.contains(&n) {
                    continue;
                }
                let found = self
                    .edges
                    .iter()
                    .enumerate()
                    .find(|(i, e)| {
                        !used.contains(i)
                            && ((e.a == n && seen.contains(&e.b))
                                || (e.b == n && seen.contains(&e.a)))
                    })
                    .map(|(i, e)| (i, e.clone()));
                let Some((i, e)) = found else { continue };
                used.insert(i);
                seen.insert(n);
                // 新节点是 n；另一侧是已见的 a/b
                let (new_col, old_col) = if e.a == n {
                    (ColRef { node: n, column: e.col_a.clone() }, ColRef { node: e.b, column: e.col_b.clone() })
                } else {
                    (ColRef { node: n, column: e.col_b.clone() }, ColRef { node: e.a, column: e.col_a.clone() })
                };
                out.push_str(&format!(
                    " JOIN {} {} ON ({} = {})",
                    sql::ident(&self.nodes[n].class_key),
                    self.nodes[n].alias,
                    self.col_sql(&new_col),
                    self.col_sql(&old_col)
                ));
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
        // 剩下的边 = 同一对表之间的额外约束（例如 balances 的两个客户列都指向 customers）
        for (i, e) in self.edges.iter().enumerate() {
            if used.contains(&i) {
                continue;
            }
            extra.push((
                ColRef { node: e.a, column: e.col_a.clone() },
                ColRef { node: e.b, column: e.col_b.clone() },
            ));
        }
        (out, extra)
    }

    // ── 表达式渲染 ─────────────────────────────────────────────

    fn col_sql(&self, c: &ColRef) -> String {
        format!("{}.{}", self.nodes[c.node].alias, sql::ident(&c.column))
    }

    fn iri_sql(&self, node: usize) -> Result<String, QueryError> {
        let class = &self.nodes[node];
        let pk = self
            .mapping
            .class(&class.class_key)
            .map(|c| c.pk.clone())
            .ok_or_else(|| {
                QueryError::new(format!("映射自相矛盾：类 {} 不在映射里", class.class_key))
            })?;
        let prefix = format!("{}#{}/", self.ontology_iri, class.class_key);
        Ok(format!(
            "({} || {})",
            sql::text_literal(&prefix)?,
            self.col_sql(&ColRef { node, column: pk })
        ))
    }

    fn data_type_of(&self, c: &ColRef) -> Option<&str> {
        self.schema.data_type(&self.nodes[c.node].class_key, &c.column)
    }

    /// ⭐ REQ-140 · 一个投影列的 RDF 身份（[`ColumnKind`]）。
    ///
    /// 谓词本地名与 `rr:datatype` 都**查映射**（同一个 `MappedProperty`）——
    /// 与 `classify_predicate` 认谓词用的是同一份数据，⛔ 不另算一份命名规则。
    /// 兜底分支（映射里找不到该列）只在 `SELECT *` 投影出主键列这类情况出现：
    /// 谓词名退回 `{类}__{列}`（与 R2RML 的命名同形），类型退回 `xsd:string`。
    fn column_kind(&self, func: Option<AggFunc>, val: Option<&Val>) -> ColumnKind {
        if let Some(f) = func {
            return ColumnKind::Aggregate { func: f.as_str() };
        }
        match val {
            Some(Val::Node(n)) => {
                let class_key = self.nodes[*n].class_key.clone();
                let class_local = self
                    .mapping
                    .class(&class_key)
                    .map(|c| c.class_local.clone())
                    .unwrap_or_else(|| class_key.clone());
                ColumnKind::Node { class: class_key, class_local }
            }
            Some(Val::Col(c)) => {
                let class_key = self.nodes[c.node].class_key.clone();
                let mapped = self
                    .mapping
                    .class(&class_key)
                    .and_then(|m| m.properties.iter().find(|p| p.column == c.column));
                let (predicate, xsd) = match mapped {
                    Some(p) => (p.local.clone(), p.xsd.key()),
                    None => (
                        format!("{class_key}__{}", c.column),
                        XsdType::String.key(),
                    ),
                };
                ColumnKind::Literal {
                    class: class_key,
                    column: c.column.clone(),
                    predicate,
                    xsd,
                }
            }
            None => ColumnKind::Unknown,
        }
    }

    fn value_expr(&self, v: &Val) -> Result<String, QueryError> {
        match v {
            Val::Node(n) => self.iri_sql(*n),
            Val::Col(c) => Ok(self.col_sql(c)),
        }
    }

    fn group_expr_of(&self, v: &Val) -> Result<String, QueryError> {
        match v {
            // 分组按主键列（与按 IRI 分组等价，且能用上索引）
            Val::Node(n) => {
                let pk = self
                    .mapping
                    .class(&self.nodes[*n].class_key)
                    .map(|c| c.pk.clone())
                    .unwrap_or_else(|| "id".into());
                Ok(self.col_sql(&ColRef { node: *n, column: pk }))
            }
            Val::Col(c) => Ok(self.col_sql(c)),
        }
    }

    fn order_expr_of(&self, v: &Val) -> Result<String, QueryError> {
        self.group_expr_of(v)
    }

    fn agg_expr(
        &self,
        func: AggFunc,
        var: Option<&str>,
        distinct: bool,
    ) -> Result<String, QueryError> {
        let f = func.as_str().to_ascii_lowercase();
        let Some(var) = var else {
            return Ok(format!("{f}(*)"));
        };
        let val = self.resolve_var(var)?;
        if func.needs_numeric() {
            let Val::Col(c) = &val else {
                return Err(QueryError::new(format!(
                    "{}(?{var})：?{var} 是实例（IRI），不是数值属性 —— \
                     {} 只能用在数据类型属性上",
                    func.as_str(),
                    func.as_str()
                )));
            };
            let ty = self.data_type_of(c).unwrap_or("");
            if !sql::is_numeric_type(ty) {
                return Err(QueryError::new(format!(
                    "{}(?{var})：{}.{} 的类型是 {}，不是数值 —— \
                     {} 只能用在数值列上（要计数请用 COUNT）",
                    func.as_str(),
                    self.nodes[c.node].class_key,
                    c.column,
                    if ty.is_empty() { "未知" } else { ty },
                    func.as_str()
                )));
            }
        }
        let arg = match &val {
            Val::Node(n) => {
                let pk = self
                    .mapping
                    .class(&self.nodes[*n].class_key)
                    .map(|c| c.pk.clone())
                    .unwrap_or_else(|| "id".into());
                self.col_sql(&ColRef { node: *n, column: pk })
            }
            Val::Col(c) => self.col_sql(c),
        };
        Ok(format!("{f}({}{arg})", if distinct { "DISTINCT " } else { "" }))
    }

    fn resolve_var(&self, var: &str) -> Result<Val, QueryError> {
        if let Some(n) = self.var_node.get(var) {
            return Ok(Val::Node(*n));
        }
        if let Some(c) = self.var_col.get(var) {
            return Ok(Val::Col(c.clone()));
        }
        Err(QueryError::new(format!(
            "变量 ?{var} 没有出现在任何三元组模式里 —— \
             （v1 不支持 BIND / 子查询，变量必须先被一条模式绑定）"
        )))
    }

    fn injection_sql(&self, node: usize, column: &str) -> Result<String, QueryError> {
        let class = &self.nodes[node];
        let c = ColRef { node, column: column.to_string() };
        let ty = self.data_type_of(&c);
        let lit = sql::tenant_sql(self.tenant, ty, &class.class_key, column)?;
        Ok(format!("{} = {lit}", self.col_sql(&c)))
    }

    fn cond_sql(&self, cond: &Cond) -> Result<String, QueryError> {
        match cond {
            Cond::NotNull(c) => Ok(format!("{} IS NOT NULL", self.col_sql(c))),
            Cond::ColLit(c, op, lit) => {
                let ty = self.data_type_of(c);
                Ok(format!(
                    "{} {} {}",
                    self.col_sql(c),
                    op.sql(),
                    sql::literal_sql(lit, ty)?
                ))
            }
            Cond::ColId(c, op, id) => {
                let ty = self.data_type_of(c);
                // 实例 id 来自 IRI（文本）；数值列就按数值写，其余按文本。
                let lit = Literal { text: id.clone(), kind: infer_literal_kind(id) };
                Ok(format!(
                    "{} {} {}",
                    self.col_sql(c),
                    op.sql(),
                    sql::literal_sql(&lit, ty)?
                ))
            }
        }
    }

    fn col_col_sql(&self, a: &ColRef, op: CmpOp, b: &ColRef) -> Result<String, QueryError> {
        let (ta, tb) = (self.data_type_of(a), self.data_type_of(b));
        // 两侧类型不同 ⇒ 统一按文本比较（PG 不会隐式转 varchar↔numeric）
        let (na, nb) = (ta.map(sql::is_numeric_type), tb.map(sql::is_numeric_type));
        if na != nb {
            Ok(format!(
                "({})::text {} ({})::text",
                self.col_sql(a),
                op.sql(),
                self.col_sql(b)
            ))
        } else {
            Ok(format!("{} {} {}", self.col_sql(a), op.sql(), self.col_sql(b)))
        }
    }

    fn filter_sql(&self, e: &Expr) -> Result<String, QueryError> {
        match e {
            Expr::Or(a, b) => Ok(format!("({} OR {})", self.filter_sql(a)?, self.filter_sql(b)?)),
            Expr::And(a, b) => Ok(format!("({} AND {})", self.filter_sql(a)?, self.filter_sql(b)?)),
            Expr::Not(a) => Ok(format!("(NOT {})", self.filter_sql(a)?)),
            Expr::Cmp(op, l, r) => self.cmp_sql(*op, l, r),
        }
    }

    fn cmp_sql(&self, op: CmpOp, l: &Operand, r: &Operand) -> Result<String, QueryError> {
        use Operand::{Iri as OIri, Literal as OLit};
        // 常量 vs 常量 ⇒ 拒绝（v1 里没有意义，而且容易把「比较」写成「恒真」）
        if matches!((l, r), (OLit(_), OLit(_)) | (OIri(_), OIri(_)) | (OLit(_), OIri(_)) | (OIri(_), OLit(_)))
        {
            return Err(QueryError::new(
                "FILTER 的两侧至少要有一侧是变量 —— 常量之间的比较不会缩小结果集",
            ));
        }
        let left = self.operand_of(l)?;
        let right = self.operand_of(r)?;
        let text = |s: String| format!("({s})::text");
        match (&left, &right) {
            (Term::Col(c), Term::Lit(lit)) => {
                Ok(format!("{} {} {}", self.col_sql(c), op.sql(), sql::literal_sql(lit, self.data_type_of(c))?))
            }
            (Term::Lit(lit), Term::Col(c)) => {
                Ok(format!("{} {} {}", sql::literal_sql(lit, self.data_type_of(c))?, op.sql(), self.col_sql(c)))
            }
            (Term::Col(a), Term::Col(b)) => self.col_col_sql(a, op, b),
            // 只要有一侧是实例 IRI（文本表达式），两侧都转 text 比较
            (Term::Node(n), _) => Ok(format!(
                "{} {} {}",
                text(self.value_expr(&Val::Node(*n))?),
                op.sql(),
                text(self.term_text(&right)?)
            )),
            (_, Term::Node(n)) => Ok(format!(
                "{} {} {}",
                text(self.term_text(&left)?),
                op.sql(),
                text(self.value_expr(&Val::Node(*n))?)
            )),
            (Term::Lit(a), Term::Lit(b)) => Ok(format!(
                "{} {} {}",
                sql::text_literal(&a.text)?,
                op.sql(),
                sql::text_literal(&b.text)?
            )),
            (Term::Iri(_), _) | (_, Term::Iri(_)) => Ok(format!(
                "{} {} {}",
                text(self.term_text(&left)?),
                op.sql(),
                text(self.term_text(&right)?)
            )),
        }
    }

    fn term_text(&self, t: &Term) -> Result<String, QueryError> {
        match t {
            Term::Col(c) => Ok(self.col_sql(c)),
            Term::Lit(l) => sql::text_literal(&l.text),
            Term::Iri(i) => sql::text_literal(i),
            Term::Node(n) => self.value_expr(&Val::Node(*n)),
        }
    }

    fn operand_of(&self, o: &Operand) -> Result<Term, QueryError> {
        match o {
            Operand::Var(v) => match self.resolve_var(v)? {
                Val::Node(n) => Ok(Term::Node(n)),
                Val::Col(c) => Ok(Term::Col(c)),
            },
            Operand::Iri(i) => Ok(Term::Iri(i.clone())),
            Operand::Literal(l) => Ok(Term::Lit(l.clone())),
        }
    }
}

/// 投影变量的取值方式。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Val {
    /// 实例（IRI）
    Node(usize),
    /// 字面量（列）
    Col(ColRef),
}

/// FILTER 比较的一侧渲染成什么。
#[derive(Debug, Clone)]
enum Term {
    Node(usize),
    Col(ColRef),
    Iri(String),
    Lit(Literal),
}

enum PredKind<'a> {
    Datatype(&'a MappedClass, &'a MappedProperty),
    Forward(&'a MappedClass, &'a MappedJoin),
    Inverse(&'a MappedClass, &'a MappedJoin),
}

/// 实例 id 的字面量类型：纯数字按数字，其余按字符串。
fn infer_literal_kind(s: &str) -> query::LiteralKind {
    use query::LiteralKind;
    if s.is_empty() {
        return LiteralKind::String;
    }
    let body = s.strip_prefix('-').unwrap_or(s);
    if !body.is_empty() && body.chars().all(|c| c.is_ascii_digit()) {
        LiteralKind::Integer
    } else {
        LiteralKind::String
    }
}

// ══════════════════════════════════════════════════════════════
// 测试：纯规划器（不连库）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model_view::{
        ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
    };
    use crate::domain::r2rml::build_mapping;
    use crate::domain::snapshot::{Evidence, JoinState};
    use crate::domain::ttl::TtlOptions;

    /// 与 TTL / R2RML 同一个本体 IRI（`TtlOptions::new("demo", 2)`）。
    const ONT: &str = "https://ontok.local/demo";

    // ── 夹具：三个类、两条验过的 join ──────────────────────────

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

    fn view() -> ModelTrackView {
        ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                entity(
                    "customers",
                    vec![prop("客户名", "customer_name"), prop("租户号", "tenant_no")],
                ),
                entity(
                    "orders",
                    vec![
                        prop("金额", "amount"),
                        prop("客户", "owner_id"),
                        prop("状态", "status"),
                        prop("下单时间", "created_at"),
                        prop("产品", "product_id"),
                        prop("租户号", "tenant_no"),
                    ],
                ),
                entity("products", vec![prop("产品名", "product_name")]),
            ],
            link_types: vec![
                link("orders", "customers", "owner_id", "id"),
                link("orders", "products", "product_id", "id"),
            ],
            action_types: vec![],
            functions: vec![],
        }
    }

    const SCHEMA: &str = r#"{"tables":[
      {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id","tenant_no"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"customer_name","data_type":"text"},
                  {"name":"tenant_no","data_type":"text"}]},
      {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
       "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
                  {"name":"owner_id","data_type":"text"},
                  {"name":"product_id","data_type":"text"},
                  {"name":"amount","data_type":"numeric(12,4)"},
                  {"name":"status","data_type":"integer"},
                  {"name":"created_at","data_type":"timestamp without time zone"},
                  {"name":"tenant_no","data_type":"text"}]},
      {"name":"products","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"product_name","data_type":"text"}]}
    ]}"#;

    struct Fixture {
        schema: SchemaTypes,
        mapping: R2rmlMapping,
    }

    fn fixture() -> Fixture {
        let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
        let mut opts = TtlOptions::new("demo", 2);
        opts.schema = Some(&schema);
        assert_eq!(opts_iri(&opts), ONT, "夹具的本体 IRI 必须与常量一致");
        let mapping = build_mapping(&view(), &opts, false, Default::default())
            .expect("夹具映射必须能生成");
        Fixture { schema, mapping }
    }

    fn opts_iri(opts: &TtlOptions<'_>) -> String {
        opts.ontology_iri()
    }

    impl Fixture {
        fn plan_with(
            &self,
            sparql: &str,
            policy: &QueryPolicy,
            tenant: &str,
            max_rows: u64,
        ) -> Result<Planned, QueryError> {
            plan(
                sparql,
                &PlanInput {
                    mapping: &self.mapping,
                    schema: &self.schema,
                    ontology_iri: ONT,
                    policy,
                    tenant,
                    max_rows,
                },
            )
        }

        fn plan(&self, sparql: &str) -> Result<Planned, QueryError> {
            self.plan_with(sparql, &policy(), "C0001", 1000)
        }

        fn err(&self, sparql: &str) -> String {
            self.plan(sparql).expect_err("这条查询应当被拒绝").into_message()
        }
    }

    /// 本项目的初版口径（与仓库根的 query-policy.json 同一个形状）。
    fn policy() -> QueryPolicy {
        QueryPolicy::parse(
            r#"{
              "version": "1.0.0",
              "tenantSubject": "customers",
              "tenantIdColumn": "id",
              "perTable": {
                "orders": { "tenantColumn": "owner_id", "reason": "订单属于一个客户" }
              },
              "publicDimensions": [
                { "table": "products", "reason": "产品目录对所有客户相同" }
              ],
              "reviewed_by": "测试"
            }"#,
        )
        .expect("夹具声明")
    }

    const A: &str = "https://ontok.local/demo#customers/A";
    const B: &str = "https://ontok.local/demo#customers/B";

    // ── AC-004 / R4：注入由**声明**驱动，且每张非公共表都有 ──────

    #[test]
    fn every_non_public_table_gets_the_declared_tenant_condition() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT ?o ?amount { ?o :orders__customers ?c . ?o :orders__amount ?amount }"#)
            .unwrap();
        // orders 按声明里的列，customers 按 tenantIdColumn
        assert!(p.sql.contains(r#""owner_id" = 'C0001'"#), "{}", p.sql);
        assert!(p.sql.contains(r#""id" = 'C0001'"#), "{}", p.sql);
        assert_eq!(p.tenant_applied.len(), 2);
        let mut applied: Vec<(String, String, String)> = p
            .tenant_applied
            .iter()
            .map(|t| (t.table.clone(), t.column.clone(), t.value.clone()))
            .collect();
        applied.sort();
        assert_eq!(
            applied,
            vec![
                ("customers".to_string(), "id".to_string(), "C0001".to_string()),
                ("orders".to_string(), "owner_id".to_string(), "C0001".to_string()),
            ]
        );
        // 判据：tenant_applied 覆盖 tables 里除公共维度外的每一张表
        for t in &p.tables {
            assert!(
                p.tenant_applied.iter().any(|x| &x.table == t)
                    || p.public_dimensions.iter().any(|x| &x.table == t),
                "表 {t} 既没有注入也不在公共维度里：{}",
                p.sql
            );
        }
    }

    #[test]
    fn a_public_dimension_is_not_filtered_and_says_why() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT ?o ?n { ?o :orders__products ?p . ?p :products__product_name ?n }"#)
            .unwrap();
        assert!(p.sql.contains(r#""product_name""#), "{}", p.sql);
        assert!(!p.sql.contains(r#""products"."#), "公共维度不该有过滤条件：{}", p.sql);
        assert_eq!(p.public_dimensions.len(), 1);
        assert_eq!(p.public_dimensions[0].table, "products");
        assert!(p.public_dimensions[0].reason.contains("产品目录"));
        assert_eq!(p.tenant_applied.len(), 1, "只有 orders 需要注入");
    }

    #[test]
    fn renaming_the_declared_column_changes_the_injection_and_the_code_does_not_move() {
        let f = fixture();
        let swapped = QueryPolicy::parse(
            &serde_json::to_string(&serde_json::json!({
                "version": "1.0.0",
                "tenantSubject": "customers",
                "tenantIdColumn": "id",
                "perTable": { "orders": { "tenantColumn": "tenant_no" } },
                "publicDimensions": [{ "table": "products", "reason": "产品目录" }]
            }))
            .unwrap(),
        )
        .unwrap();
        let q = r#"SELECT ?o ?amount { ?o :orders__amount ?amount }"#;
        let before = f.plan_with(q, &policy(), "C0001", 1000).unwrap();
        let after = f.plan_with(q, &swapped, "C0001", 1000).unwrap();
        assert!(before.sql.contains(r#""owner_id" = 'C0001'"#), "{}", before.sql);
        assert!(!before.sql.contains("tenant_no"), "{}", before.sql);
        // 换声明 ⇒ 注入列跟着变（代码一行没动）
        assert!(after.sql.contains(r#""tenant_no" = 'C0001'"#), "{}", after.sql);
        assert!(!after.sql.contains(r#""owner_id""#), "{}", after.sql);
        assert_eq!(after.tenant_applied[0].column, "tenant_no");
    }

    #[test]
    fn an_unregistered_table_is_rejected_and_named() {
        let f = fixture();
        let narrow = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{},"publicDimensions":[]}"#,
        )
        .unwrap();
        let e = f
            .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &narrow, "C0001", 1000)
            .expect_err("未登记的表必须整条拒绝");
        let m = e.into_message();
        assert!(m.contains("orders"), "{m}");
        assert!(m.contains("整条查询拒绝"), "{m}");
        assert!(m.contains("perTable"), "{m}");
        assert!(m.contains("publicDimensions"), "{m}");
    }

    #[test]
    fn a_declared_column_that_the_schema_does_not_have_is_rejected_by_name() {
        let f = fixture();
        let broken = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"no_such_tenant_col"}},"publicDimensions":[]}"#,
        )
        .unwrap();
        let m = f
            .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &broken, "C0001", 1000)
            .expect_err("声明写了不存在的列必须拒绝")
            .into_message();
        assert!(m.contains("orders.no_such_tenant_col"), "{m}");
        assert!(m.contains("不会「跳过这一条继续跑」"), "{m}");
    }

    #[test]
    fn a_tenant_subject_that_is_not_a_class_is_rejected() {
        let f = fixture();
        let broken = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"nope","tenantIdColumn":"id",
                "perTable":{},"publicDimensions":[]}"#,
        )
        .unwrap();
        let m = f
            .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &broken, "C0001", 1000)
            .expect_err("tenantSubject 不是类必须拒绝")
            .into_message();
        assert!(m.contains("tenantSubject"), "{m}");
        assert!(m.contains(":customers"), "{m}");
    }

    // ── AC-005：注入是 AND，反向筛也挡得住 ─────────────────────

    #[test]
    fn the_injection_is_the_first_where_term_and_is_anded() {
        let f = fixture();
        let p = f
            .plan(&format!(
                r#"SELECT ?o ?c {{ ?o :orders__customers ?c . FILTER (?c != <{A}>) }}"#
            ))
            .unwrap();
        // 注入条件在 WHERE 最前面（R5：一眼能看见）
        assert!(
            p.sql.contains(r#"WHERE t0."owner_id" = 'C0001' AND t1."id" = 'C0001' AND"#),
            "{}",
            p.sql
        );
        // 用户 FILTER 是**后面**的另一个 AND 项（注入不可能被它 OR 掉）
        assert!(
            p.sql.contains("!= ('https://ontok.local/demo#customers/A')::text"),
            "{}",
            p.sql
        );
        let after_where = p.sql.split(" WHERE ").nth(1).unwrap();
        assert!(after_where.starts_with(r#"t0."owner_id" = 'C0001'"#), "{}", p.sql);
        assert!(!p.sql.contains(r#"OR t0."owner_id""#), "{}", p.sql);
        assert!(!p.sql.contains(r#"OR t1."id""#), "{}", p.sql);
    }

    #[test]
    fn an_instance_iri_constant_becomes_an_equality_on_the_primary_key() {
        let f = fixture();
        let p = f
            .plan(&format!(r#"SELECT ?amount {{ ?o :orders__customers <{A}> . ?o :orders__amount ?amount }}"#))
            .unwrap();
        assert!(p.sql.contains(r#"t0."owner_id" = 'A'"#), "{}", p.sql);
    }

    // ── R2：未知类 / 未知属性必须报错并给可用清单 ───────────────

    #[test]
    fn an_unknown_property_names_the_class_and_lists_what_it_has() {
        let f = fixture();
        let m = f.err(r#"SELECT ?o ?x { ?o :orders__no_such_column ?x }"#);
        assert!(m.contains(":orders__no_such_column"), "{m}");
        assert!(m.contains(":orders__amount"), "应列出可用属性：{m}");
        assert!(m.contains("ontology show"), "{m}");
    }

    #[test]
    fn an_unknown_class_lists_the_available_classes() {
        let f = fixture();
        let m = f.err(r#"SELECT ?s { ?s a :no_such_class }"#);
        assert!(m.contains(":no_such_class"), "{m}");
        assert!(m.contains(":customers"), "{m}");
        assert!(m.contains(":orders"), "{m}");
    }

    #[test]
    fn a_datatype_property_with_an_iri_object_is_rejected() {
        let f = fixture();
        let m = f.err(&format!(r#"SELECT ?amount {{ ?o :orders__amount <{A}> }}"#));
        assert!(m.contains("数据类型属性"), "{m}");
    }

    // ── 规划：沿 R2RML 的 joinCondition 连表 ────────────────────

    #[test]
    fn two_hops_become_two_joins_on_the_r2rml_join_columns() {
        let f = fixture();
        let p = f
            .plan(
                r#"SELECT ?n { ?o :orders__products ?p . ?p :products__product_name ?n }"#,
            )
            .unwrap();
        assert!(p.sql.contains(r#"FROM "orders" t0"#), "{}", p.sql);
        assert!(
            p.sql.contains(r#"JOIN "products" t1 ON (t1."id" = t0."product_id")"#)
                || p.sql.contains(r#"JOIN "products" t1 ON (t0."product_id" = t1."id")"#),
            "{}",
            p.sql
        );
        assert_eq!(p.tables, vec!["orders", "products"]);
    }

    #[test]
    fn the_inverse_predicate_resolves_back_to_the_same_join() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT ?o { ?c :customers__orders ?o . ?o :orders__amount ?a }"#)
            .unwrap();
        assert!(
            p.sql.contains(r#"JOIN "orders" t1 ON (t1."owner_id" = t0."id")"#)
                || p.sql.contains(r#"JOIN "orders" t1 ON (t0."id" = t1."owner_id")"#),
            "{}",
            p.sql
        );
    }

    #[test]
    fn disconnected_patterns_are_rejected_not_cross_joined() {
        let f = fixture();
        let m = f.err(r#"SELECT ?o ?c { ?o a :orders . ?c a :customers }"#);
        assert!(m.contains("互不相连"), "{m}");
        assert!(m.contains("笛卡尔积"), "{m}");
    }

    // ── 聚合 / 分组 / 排序 / 上限 ──────────────────────────────

    #[test]
    fn aggregates_grouping_and_ordering_land_in_sql() {
        let f = fixture();
        let p = f
            .plan(
                r#"SELECT ?status (COUNT(*) AS ?n) (SUM(?amount) AS ?total)
                    { ?o :orders__status ?status . ?o :orders__amount ?amount }
                    GROUP BY ?status ORDER BY DESC(?total)"#,
            )
            .unwrap();
        assert!(p.sql.contains(r#"count(*))::text AS "n""#), "{}", p.sql);
        assert!(p.sql.contains(r#"sum(t0."amount"))::text AS "total""#), "{}", p.sql);
        assert!(p.sql.contains(r#"GROUP BY t0."status""#), "{}", p.sql);
        assert!(p.sql.contains(r#"ORDER BY sum(t0."amount") DESC"#), "{}", p.sql);
    }

    #[test]
    fn sum_on_a_text_column_is_rejected() {
        let f = fixture();
        let m = f.err(
            r#"SELECT (SUM(?n) AS ?s) { ?o :orders__customers ?c . ?c :customers__customer_name ?n }"#,
        );
        assert!(m.contains("SUM"), "{m}");
        assert!(m.contains("不是数值"), "{m}");
    }

    #[test]
    fn order_by_a_variable_that_is_not_projected_is_rejected() {
        let f = fixture();
        let m = f.err(r#"SELECT ?o { ?o :orders__amount ?a } ORDER BY ?a"#);
        assert!(m.contains("必须出现在 SELECT"), "{m}");
    }

    // ── I-1 · 覆盖范围不完整 ⇒ 回执里必须出现（且只在命中时）──────────

    #[test]
    fn i1_an_incomplete_coverage_table_is_announced_in_warnings() {
        let f = fixture();
        let incomplete = QueryPolicy::parse(
            r#"{
              "version": "1.0.0",
              "tenantSubject": "customers",
              "tenantIdColumn": "id",
              "perTable": {
                "orders": {
                  "tenantColumn": "owner_id",
                  "coverage": "main_only",
                  "coverageNote": "子账号产生的订单不在结果内，金额可能偏低",
                  "reason": "子账号产生的订单按 owner_id 过滤时主账号看不到（长文，供评审）"
                }
              },
              "publicDimensions": [{ "table": "products", "reason": "产品目录" }]
            }"#,
        )
        .unwrap();
        let p = f
            .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &incomplete, "C0001", 1000)
            .unwrap();
        let hit: Vec<&String> = p
            .warnings
            .iter()
            .filter(|w| w.contains("orders") && w.contains("main_only"))
            .collect();
        assert_eq!(hit.len(), 1, "命中 main_only 表必须给一条警告：{:?}", p.warnings);
        assert!(
            hit[0].contains("子账号") && hit[0].contains("可能偏低"),
            "警告正文要来自声明（coverageNote）：{}",
            hit[0]
        );
        // 短句优先：评审长文（reason）不该整段塞进给数字员工读的警告
        assert!(
            !hit[0].contains("长文，供评审"),
            "有 coverageNote 时警告不该退回 reason：{}",
            hit[0]
        );
        // 注入本身不受影响（口径归口径，机制照旧）
        assert!(p.sql.contains(r#""owner_id" = 'C0001'"#), "{}", p.sql);
    }

    #[test]
    fn i1_a_complete_table_produces_no_coverage_warning() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#)
            .unwrap();
        assert!(
            !p.warnings.iter().any(|w| w.contains("覆盖范围")),
            "complete 的表不该出现覆盖范围警告：{:?}",
            p.warnings
        );
    }

    #[test]
    fn i1_an_unqueried_incomplete_table_does_not_warn() {
        let f = fixture();
        let incomplete = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"owner_id","coverage":"main_only",
                "reason":"子账号订单主账号看不到"}},
                "publicDimensions":[{"table":"products","reason":"产品目录"}]}"#,
        )
        .unwrap();
        // 只查 customers（complete）——orders 没被碰，就不该报警
        let p = f
            .plan_with(
                r#"SELECT ?c ?n { ?c a :customers . ?c :customers__customer_name ?n }"#,
                &incomplete,
                "C0001",
                1000,
            )
            .unwrap();
        assert!(
            !p.warnings.iter().any(|w| w.contains("orders")),
            "没被查询的表不该出现在 warnings：{:?}",
            p.warnings
        );
    }

    #[test]
    fn the_row_cap_clips_a_larger_limit_and_says_so() {
        let f = fixture();
        let p = f.plan_with(
            r#"SELECT ?o { ?o :orders__amount ?a } LIMIT 5000"#,
            &policy(),
            "C0001",
            100,
        )
        .unwrap();
        assert_eq!(p.effective_limit, 100);
        assert!(p.truncated_possible);
        // 多取 1 行探针（判断 truncated）；sql_echo 与执行逐字一致
        assert!(p.sql.contains("LIMIT 101"), "{}", p.sql);
        assert!(p.warnings.iter().any(|w| w.contains("LIMIT 5000")), "{:?}", p.warnings);
    }

    #[test]
    fn offset_and_limit_both_land_in_sql() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT ?o ?a { ?o :orders__amount ?a } ORDER BY DESC(?a) LIMIT 10 OFFSET 20"#)
            .unwrap();
        assert!(p.sql.contains(r#"ORDER BY t0."amount" DESC"#), "{}", p.sql);
        assert!(p.sql.contains("LIMIT 11"), "{}", p.sql);
        assert!(p.sql.contains("OFFSET 20"), "{}", p.sql);
    }

    #[test]
    fn filters_compare_against_the_real_column_type() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT ?o ?a { ?o :orders__amount ?a . ?o :orders__status ?s . FILTER (?a > 100 && ?s = 1) }"#)
            .unwrap();
        // 数值列 ⇒ 数字字面量不加引号；整数列同理
        assert!(p.sql.contains(r#"t0."amount" > 100"#), "{}", p.sql);
        assert!(p.sql.contains(r#"t0."status" = 1"#), "{}", p.sql);
    }

    #[test]
    fn a_string_literal_for_a_numeric_column_is_rejected_at_plan_time() {
        let f = fixture();
        let m = f.err(r#"SELECT ?o { ?o :orders__amount ?a . FILTER (?a > "100") }"#);
        assert!(m.contains("不做隐式转换"), "{m}");
    }

    #[test]
    fn a_filter_variable_that_is_never_bound_is_rejected() {
        let f = fixture();
        let m = f.err(r#"SELECT ?o { ?o :orders__amount ?a . FILTER (?nope > 1) }"#);
        assert!(m.contains("?nope"), "{m}");
        assert!(m.contains("没有出现在任何三元组模式"), "{m}");
    }

    #[test]
    fn select_star_projects_every_bound_variable() {
        let f = fixture();
        let p = f
            .plan(r#"SELECT * { ?o :orders__customers ?c . ?c :customers__customer_name ?n }"#)
            .unwrap();
        assert_eq!(p.columns, vec!["o", "c", "n"]);
    }

    #[test]
    fn an_empty_tenant_is_rejected_before_any_sql() {
        let f = fixture();
        let m = f
            .plan_with(r#"SELECT ?o { ?o :orders__amount ?a }"#, &policy(), "  ", 1000)
            .expect_err("空租户必须拒绝")
            .into_message();
        assert!(m.contains("租户为空"), "{m}");
    }

    #[test]
    fn a_numeric_tenant_column_with_a_non_numeric_tenant_is_rejected() {
        let f = fixture();
        let numeric = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"status"}},"publicDimensions":[]}"#,
        )
        .unwrap();
        let m = f
            .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &numeric, "C0001", 1000)
            .expect_err("数值租户列 + 非数字租户值必须拒绝")
            .into_message();
        assert!(m.contains("orders.status"), "{m}");
        assert!(m.contains("不做隐式转换"), "{m}");
    }

    #[test]
    fn the_plan_is_deterministic() {
        let f = fixture();
        let q = r#"SELECT ?o ?a { ?o :orders__customers ?c . ?o :orders__amount ?a }"#;
        let a = f.plan(q).unwrap().sql;
        let b = f.plan(q).unwrap().sql;
        assert_eq!(a, b);
    }
}
