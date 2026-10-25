//! ⭐ REQ-140 · **SPARQL 结果 → JSON-LD 上下文子图**（数字员工平台要的「图」，不是「行」）。
//!
//! ```text
//! 行回执（REQ-122/123 的形状，⛔ 一个字节都不改）        JSON-LD（本模块）
//! columns: ["m","name"]                                {
//! rows: [[iri, "Claude Haiku 4.5"]]                      "@context": {"ontok": …, "xsd": …},
//!                                                        "@graph": [{"@id": "ontok:llm_model/…",
//!                                                                    "@type": "ontok:llm_model",
//!                                                                    "ontok:llm_model__model_name": "…"}],
//!                                                        "summary": {"nodes":1,"row_count":1},
//!                                                        "meta": {…与行回执逐字节相同…}
//!                                                      }
//! ```
//!
//! # 三条口径（对应 BA 的 R2 / R4 / R5）
//!
//! | 口径 | 做法 |
//! | --- | --- |
//! | **确定性** | 节点按 `@id` 升序；属性键按名字升序；同一个输入 ⇒ 同一份字节 |
//! | **不造假节点** | 聚合 / 只有字面量的结果**不进** `@graph`，进 `summary` |
//! | **空结果** | `@graph: []`（不是 `null`、不是缺字段） |
//!
//! # `meta` 为什么是**一个结构体**而不是 `serde_json::Value`
//!
//! `serde_json::Map` 的底层是 `BTreeMap`（本 crate 没开 `preserve_order`）⇒ 任何
//! `Value` 的成员都会被**按键排序**。而 AC-005 要求 `meta.warnings` / `meta.coverage`
//! 与 `format=rows` 时**完全一致** —— 若把回执转成 `Value` 再塞进 `meta`，
//! `coverage` 的对象就会从 `{"table":…,"coverage":…}` 变成 `{"coverage":…,"table":…}`，
//! 「值相同、字节不同」正是最容易在评审里被读成「吞了限定条件」的形态。
//!
//! 所以 [`Meta`] 是一个**借用回执字段**的结构体：字段顺序与 [`SparqlReceipt`] 一致
//! （去掉 `columns` / `rows` 两个表格字段），于是 `meta` 的字节**就是**行回执正文
//! 去掉前两个成员之后的那一段（判据见本文件 `meta_text_is_the_rows_receipt_minus_…`）。
//! `@graph` 里的节点仍是 `Value`（成员排序正是 R2 的确定性来源）。
//!
//! # 列的 RDF 身份从哪来（⛔ 不从列名猜）
//!
//! 全部来自 [`crate::sparql::ProjectedColumn`] —— 规划器在**绑定变量的那一刻**记下的
//! `var → 类 / 谓词 / xsd`（`rr:predicate` / `rr:datatype` 的原值）。列名里只有
//! `m` / `name`，猜不出「谁是实体、谓词叫什么」。字符串前缀（`{base}#{类}/`）只用来
//! **把完整 IRI 缩成 `ontok:` 前缀**，不参与「这列是节点还是字面量」的判定。
//!
//! # 同 `@id` 合并（这是「子图」与「表」的分水岭）
//!
//! 一个实体在多行出现（两条订单同一个客户）⇒ 它的属性**并到同一个节点**上。
//! 判据是 `@id` 字符串相等（实例 IRI 由 `iri_sql` 拼出来，同实体必同串）。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - **字面量挂在「它所属类的那个节点」上**：`?c :customers__customer_name ?n` 的 `?n`
//!   挂到本行 `?c` 的 `@id`。若查询**没有投影**那个类变量（`SELECT ?n WHERE {?c …}`），
//!   就没有可挂的实体 ⇒ 值进 `summary.detached`（⛔ 不造一个没有 IRI 的假节点）。
//! - `@graph` 的顺序是规范的（按 `@id`），但 `summary.groups[]`（GROUP BY）与
//!   `rows` 一样**沿用 SQL 的返回顺序**：要稳定顺序请在查询里写 `ORDER BY`。
//! - 只做 **扁平** 图：节点之间**不做**嵌套，也没有 `@reverse` / 命名图 / 推理。
//! - 只做 JSON-LD：⛔ 不做 RDF/XML / Turtle（需求「不做」一栏）。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::sparql::{ColumnKind, ProjectedColumn, PublicDimensionUsed, TenantApplied};

use super::{CoverageEntry, JoinStateEntry, LimitInfo, SparqlReceipt};

/// JSON-LD 的上下文前缀（`@context` 里只有这两个）。
pub const ONTOK_PREFIX: &str = "ontok";
/// `xsd` 命名空间（`@context` 里的第二个前缀）。
pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema#";

/// `format=jsonld` 响应的 `Content-Type`。
pub const CONTENT_TYPE: &str = "application/ld+json; charset=utf-8";

/// 同一个媒体类型（不带参数）—— `Accept` 匹配用它（`application/ld+json;q=0.9` 也认）。
pub const CONTENT_TYPE_MEDIA: &str = "application/ld+json";

// ══════════════════════════════════════════════════════════════
// 输出形状（**结构体**，不是 Value：成员顺序就是声明顺序）
// ══════════════════════════════════════════════════════════════

/// 一份 JSON-LD 上下文子图（`Serialize` 一次成型；REST 与 MCP 用**同一个**值）。
#[derive(Debug, Clone, Serialize)]
pub struct JsonLd<'a> {
    #[serde(rename = "@context")]
    pub context: BTreeMap<&'static str, String>,
    #[serde(rename = "@graph")]
    pub graph: Vec<Value>,
    /// ⭐ REQ-145 · **待取的节点**（顶层，与 `@graph` 并列 —— 契约 §九 第 1 点）。
    ///
    /// 三条纪律（**结构上**成立，不靠人记）：
    ///
    /// | # | 纪律 | 怎么保证的 |
    /// | --- | --- | --- |
    /// | 1 | **不进 `@graph`**（契约规则 2） | 它是**另一个字段**，[`Self::graph`] 只由有事实的行构造 |
    /// | 2 | `meta.complete == @pending.is_empty()`（AC-007） | 两个值都从**同一个** `pending` 切片算 |
    /// | 3 | 没 pending 时也要能看出来「完整」 | `Some([])` ⇒ `"@pending": []`；`None` ⇒ 字段不出现（没加载映射） |
    #[serde(rename = "@pending", skip_serializing_if = "Option::is_none")]
    pub pending: Option<Vec<PendingNode>>,
    pub summary: Value,
    pub meta: Meta<'a>,
}

/// ⭐ REQ-145 · `@pending[]` 的一条 ——「这一块平台没算，你要这样自己取」。
///
/// 字段顺序即序列化顺序（`to_text` 与 REST 正文逐字节相同）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingNode {
    /// 待取节点的 IRI（**认不出实例标识时不出现** —— ⛔ 不编一个假 id）。
    #[serde(rename = "@id", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 它属于哪个类（`ontok:<类本地名>`）。
    #[serde(rename = "@type")]
    pub class_iri: String,
    /// 这一类**来自 API**（与 TTL 的 `ontok:sourceKind "api"` 是同一件事）。
    #[serde(rename = "ontok:sourceKind")]
    pub source_kind: &'static str,
    /// 为什么拿不到：`source_is_api` / `cross_source_edge`（稳定字符串，Agent 按它分支）。
    pub reason: &'static str,
    /// 取数指引。**跨源边没有它**（契约规则 4：给了也查不了，比不给更糟）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<PendingGet>,
}

/// `@pending[].get` ——「按什么去取」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingGet {
    /// 取哪个类。
    pub class: String,
    /// 过滤条件（`{}` = 平台不知道实例标识，按类取）。
    pub filter: PendingFilter,
    /// 能直接下推给 API 的过滤字段。
    pub pushdown: Vec<String>,
    /// `access#<条目>@v<映射版本>` —— 可追溯到**签过的那一条**契约（规则 3）。
    #[serde(rename = "ref")]
    pub reference: String,
}

/// `get.filter`（目前只有 `ids`；`{}` 是合法形状 —— 契约规则 7）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingFilter {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
}

/// `meta` = 行回执**去掉** `columns` / `rows` 的全部字段（顺序 = [`SparqlReceipt`] 的声明顺序）。
///
/// 借用而不是克隆：它就是「同一次查询的那份回执」的另一半，⛔ 不是「照着抄一遍」的
/// 第二份数据 —— 想让它少一个字段，只能在 [`Meta`] 里删一行，那正是变异 ③。
#[derive(Debug, Clone, Serialize)]
pub struct Meta<'a> {
    pub row_count: usize,
    pub truncated: bool,
    pub sql_echo: &'a str,
    pub tenant_applied: &'a [TenantApplied],
    pub tables: &'a [String],
    pub public_dimensions: &'a [PublicDimensionUsed],
    pub warnings: &'a [String],
    pub coverage: &'a [CoverageEntry],
    pub join_states: &'a [JoinStateEntry],
    pub join_states_scope: &'a str,
    pub join_states_scope_note: &'a str,
    pub limits: &'a LimitInfo,
    /// ⭐ REQ-145 · 结果**完整吗**（契约规则 1：有 `@pending` ⇒ 必为 `false`）。
    ///
    /// ⚠️ 不变式（AC-007）：`complete == pending.is_empty()` —— 两个值都从
    /// **同一个** `pending` 算出来（见 [`render`] / [`render_pending`]），⛔ 不各算一遍。
    /// `None` = 这个部署没加载 access 映射 ⇒ 字段不出现（R1：与基线逐字节相同）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complete: Option<bool>,
    /// 本次没算进去的类（本地名，升序去重）。与 `@pending` 的一致性由构造点保证。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_classes: Option<Vec<String>>,
    /// ⭐ REQ-145 · 加载的 access 映射版本（`get.ref` 的 `@v<n>` 与它同源）。无映射 ⇒ 不出现。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapping_version: Option<u64>,
}

impl<'a> JsonLd<'a> {
    /// 正文（REST body 与 MCP `content[0].text` 都取这一个）。
    pub fn to_text(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| {
            // 全是 String / Vec / Value，理论上不会失败；真失败也不许 panic。
            format!("{{\"error\":\"JSON-LD 序列化失败：{e}\"}}")
        })
    }

    /// 同一个图的 `Value` 形态（MCP 的 `structuredContent` 用具）。
    ///
    /// ⚠️ `Value` 的成员按键排序（`serde_json::Map` = `BTreeMap`）⇒ 它的**内容**与
    /// [`Self::to_text`] 相同、**成员顺序不同**。REST 正文与 MCP 文本走 `to_text`，
    /// 不是这条路 —— 否则 `meta` 的对象成员顺序会与 `format=rows` 漂移。
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// 一个节点（`@id` → 类型 + 属性）。属性用 `BTreeMap` ⇒ 键序**恒为升序**（R2）。
struct Node {
    class_local: String,
    props: BTreeMap<String, Value>,
}

impl Node {
    fn new(class_local: &str) -> Self {
        Self {
            class_local: class_local.to_string(),
            props: BTreeMap::new(),
        }
    }
}

/// **唯一**的 JSON-LD 构造点：行回执 + 列的 RDF 身份 + 本体 IRI → 一份 [`JsonLd`]。
///
/// 纯函数（不连库、不看时间、不读环境）：同一组入参**逐字节**得到同一个正文。
///
/// - `receipt`：行回执（`format=rows` 的**同一个**对象；`meta` 直接借用它的字段）；
/// - `projection`：列名 + `var → 类 / 谓词 / xsd`（来自 `Planned`）；
/// - `ontology_iri`：`TtlOptions::ontology_iri()`（与 TTL / R2RML / `iri_sql` 同一个）。
pub fn render<'a>(
    receipt: &'a SparqlReceipt,
    projection: &[ProjectedColumn],
    ontology_iri: &str,
) -> JsonLd<'a> {
    render_with_mapping(receipt, projection, ontology_iri, None)
}

/// ⭐ REQ-145 · 同 [`render`]，另外告诉它**加载的映射版本**（`meta.mapping_version`）。
///
/// 两个入口而不是给 [`render`] 多一个必填参数：`None` 那条是**没加载映射**的老路径，
/// 它的字节已经被一堆判据钉住（R1）——多一个参数只会让每个老调用点都要回答一个
/// 它根本不关心的问题。
pub fn render_with_mapping<'a>(
    receipt: &'a SparqlReceipt,
    projection: &[ProjectedColumn],
    ontology_iri: &str,
    mapping_version: Option<u64>,
) -> JsonLd<'a> {
    let base = format!("{ontology_iri}#");
    let mut context: BTreeMap<&'static str, String> = BTreeMap::new();
    context.insert(ONTOK_PREFIX, base.clone());
    context.insert("xsd", XSD_NS.to_string());

    // ⭐ REQ-145 · `@pending` 与 `meta.complete` **同源**：库源路径的 pending 恒空，
    // 所以 `complete` 只能是 `true`（有 pending 的那条路走 [`render_pending`]）。
    // `None` = 没加载映射 ⇒ 三个字段都不出现（R1）。
    let pending: Option<Vec<PendingNode>> = receipt.complete.map(|_| Vec::new());

    let meta = Meta {
        row_count: receipt.row_count,
        truncated: receipt.truncated,
        sql_echo: &receipt.sql_echo,
        tenant_applied: &receipt.tenant_applied,
        tables: &receipt.tables,
        public_dimensions: &receipt.public_dimensions,
        warnings: &receipt.warnings,
        coverage: &receipt.coverage,
        join_states: &receipt.join_states,
        join_states_scope: receipt.join_states_scope,
        join_states_scope_note: receipt.join_states_scope_note,
        limits: &receipt.limits,
        complete: receipt.complete,
        unresolved_classes: receipt.unresolved_classes.clone(),
        mapping_version,
    };

    // 有聚合 ⇒ 没有任何实体可挂（R4）：全部进 summary，`@graph` 恒为 []。
    if has_aggregate(projection) {
        return JsonLd {
            context,
            graph: Vec::new(),
            pending,
            summary: aggregate_summary(receipt, projection),
            meta,
        };
    }

    // ── 行 → 节点（同 `@id` 合并属性）────────────────────────────
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    let mut detached: Vec<Value> = Vec::new();

    for row in &receipt.rows {
        // 第一遍：本行的 `类 → @id`（字面量要挂到**自己那个类**的节点上，
        // ⛔ 不是「本行第一个节点」—— 那会把客户的姓名挂到订单上）。
        let mut row_ids: BTreeMap<&str, &str> = BTreeMap::new();
        for (i, col) in projection.iter().enumerate() {
            if let ColumnKind::Node { class, .. } = &col.kind {
                if let Some(Some(iri)) = row.get(i) {
                    row_ids.insert(class.as_str(), iri.as_str());
                }
            }
        }
        // 第二遍：节点先落地，字面量再挂（保证「先有 @id 再有属性」）。
        for (i, col) in projection.iter().enumerate() {
            let ColumnKind::Node { class_local, .. } = &col.kind else {
                continue;
            };
            if let Some(Some(iri)) = row.get(i) {
                nodes
                    .entry(iri.clone())
                    .or_insert_with(|| Node::new(class_local));
            }
        }
        for (i, col) in projection.iter().enumerate() {
            let ColumnKind::Literal {
                class,
                predicate,
                xsd,
                ..
            } = &col.kind
            else {
                continue;
            };
            // R2RML：NULL 不产生三元组 ⇒ 空值不进图（⛔ 不写 null 属性）。
            let Some(Some(text)) = row.get(i) else {
                continue;
            };
            let value = literal_json(text, xsd);
            match row_ids.get(class.as_str()) {
                Some(id) => {
                    if let Some(node) = nodes.get_mut(*id) {
                        node.props.insert(predicate.clone(), value);
                    }
                }
                // 本行没有「这个类」的实体 ⇒ 没有主语可挂（见模块头「已知限制」）。
                None => detached.push(json!({ "predicate": predicate, "value": value })),
            }
        }
    }

    // 节点按 `@id` 升序（BTreeMap）+ 属性按键升序（Map 是 BTreeMap）⇒ 逐字节确定。
    let mut graph: Vec<Value> = Vec::with_capacity(nodes.len());
    for (iri, node) in &nodes {
        let mut obj = Map::new();
        obj.insert("@id".to_string(), json!(abbreviate(iri, &base)));
        obj.insert(
            "@type".to_string(),
            json!(format!("{ONTOK_PREFIX}:{}", node.class_local)),
        );
        for (predicate, value) in &node.props {
            obj.insert(format!("{ONTOK_PREFIX}:{predicate}"), value.clone());
        }
        graph.push(Value::Object(obj));
    }

    JsonLd {
        context,
        graph,
        pending,
        summary: plain_summary(nodes.len(), receipt.row_count, detached),
        meta,
    }
}

/// ⭐ REQ-145 · **只碰 api 源**的那份产物：`@graph: []` + `@pending[]` + `complete:false`。
///
/// 为什么 `@graph` **恒空**且**不是报错**（契约规则 7）：Agent 一条链走得完 ——
/// 拿到 `complete:false` + `get` 就去取数，不需要「先失败一次、再猜着换写法」。
///
/// 三条不变式都在这里**一处**成立：
/// - `@graph` 里没有一个节点（`graph: Vec::new()`；pending 是另一个字段，规则 2）；
/// - `meta.complete == @pending.is_empty()`（`false == false`，AC-007）；
/// - `meta.unresolved_classes` 与 `@pending[].@type` 是**同一批类**（都从 `pending` 取）。
pub fn render_pending<'a>(
    receipt: &'a SparqlReceipt,
    pending: &[super::access::ApiPending],
    ontology_iri: &str,
    mapping_version: u64,
) -> JsonLd<'a> {
    let base = format!("{ontology_iri}#");
    let mut context: BTreeMap<&'static str, String> = BTreeMap::new();
    context.insert(ONTOK_PREFIX, base.clone());
    context.insert("xsd", XSD_NS.to_string());

    let nodes: Vec<PendingNode> = pending.iter().map(pending_node).collect();
    // 不变式 `complete == @pending.is_empty()`：**从同一个切片**算，⛔ 不写常量 `false`
    // （写常量就会在「pending 正好为空」时自相矛盾）。
    let complete = nodes.is_empty();
    let mut unresolved: Vec<String> = pending.iter().map(|p| p.class_local.clone()).collect();
    unresolved.sort();
    unresolved.dedup();

    let meta = Meta {
        row_count: receipt.row_count,
        truncated: receipt.truncated,
        sql_echo: &receipt.sql_echo,
        tenant_applied: &receipt.tenant_applied,
        tables: &receipt.tables,
        public_dimensions: &receipt.public_dimensions,
        warnings: &receipt.warnings,
        coverage: &receipt.coverage,
        join_states: &receipt.join_states,
        join_states_scope: receipt.join_states_scope,
        join_states_scope_note: receipt.join_states_scope_note,
        limits: &receipt.limits,
        complete: Some(complete),
        unresolved_classes: Some(unresolved),
        mapping_version: Some(mapping_version),
    };

    let mut summary = Map::new();
    summary.insert("nodes".to_string(), json!(0));
    summary.insert("row_count".to_string(), json!(0));
    summary.insert("pending".to_string(), json!(nodes.len()));

    JsonLd {
        context,
        // ⛔ 规则 2：`@pending` **不进** `@graph`（这里连一个节点都没有）。
        graph: Vec::new(),
        pending: Some(nodes),
        summary: Value::Object(summary),
        meta,
    }
}

/// 服务端的 [`super::access::ApiPending`] → 对外的 `@pending[]` 条目。
fn pending_node(p: &super::access::ApiPending) -> PendingNode {
    PendingNode {
        id: p
            .id
            .as_deref()
            .map(|id| format!("{ONTOK_PREFIX}:{}/{}", p.class_local, id)),
        class_iri: format!("{ONTOK_PREFIX}:{}", p.class_local),
        source_kind: crate::domain::ttl::API_SOURCE_KIND,
        reason: p.reason,
        get: p.get.as_ref().map(|g| PendingGet {
            class: g.class.clone(),
            filter: PendingFilter {
                ids: g.ids.clone(),
            },
            pushdown: g.pushdown.clone(),
            reference: g.reference.clone(),
        }),
    }
}

/// 有没有聚合列（有 ⇒ 没有实体，`@graph` 恒 `[]`）。
fn has_aggregate(projection: &[ProjectedColumn]) -> bool {
    projection
        .iter()
        .any(|c| matches!(c.kind, ColumnKind::Aggregate { .. } | ColumnKind::Unknown))
}

/// 非聚合结果的 `summary`：**没进图的量**要看得见。
///
/// - `nodes` / `rows`：进去几个实体、原来几行（合并率一眼可见）；
/// - `detached`：没有主语可挂的字面量（只在「查询没投影实体」时出现，按谓词 + 值排序）。
fn plain_summary(nodes: usize, rows: usize, mut detached: Vec<Value>) -> Value {
    let mut summary = Map::new();
    summary.insert("nodes".to_string(), json!(nodes));
    summary.insert("row_count".to_string(), json!(rows));
    if !detached.is_empty() {
        // 这堆值没有行身份（只有谓词与值），排序不丢信息、且让输出与行序无关。
        detached.sort_by_key(|v| v.to_string());
        summary.insert("detached".to_string(), Value::Array(detached));
    }
    Value::Object(summary)
}

/// 聚合结果的 `summary`：`{"n": 42}`（单行）/ `{"n_groups": k, "groups": […]}`（GROUP BY）。
///
/// ⛔ 不在 `@graph` 里造一个 `@id` 为空的节点：聚合没有实体，它的事实就是那些数字。
fn aggregate_summary(receipt: &SparqlReceipt, projection: &[ProjectedColumn]) -> Value {
    let mut summary = Map::new();
    if receipt.rows.len() > 1 {
        // GROUP BY：每组一个对象（键 = 投影列名），顺序沿用 SQL 的返回顺序。
        let mut groups: Vec<Value> = Vec::with_capacity(receipt.rows.len());
        for row in &receipt.rows {
            let mut group = Map::new();
            for (i, col) in projection.iter().enumerate() {
                if let Some(Some(text)) = row.get(i) {
                    group.insert(col.name.clone(), column_json(&col.kind, text));
                }
            }
            groups.push(Value::Object(group));
        }
        summary.insert("n_groups".to_string(), json!(groups.len()));
        summary.insert("groups".to_string(), Value::Array(groups));
        return Value::Object(summary);
    }
    let row = receipt.rows.first();
    for (i, col) in projection.iter().enumerate() {
        let Some(Some(text)) = row.and_then(|r| r.get(i)) else {
            continue;
        };
        summary.insert(col.name.clone(), column_json(&col.kind, text));
    }
    Value::Object(summary)
}

/// 一个**投影列**的值：聚合 → 数字优先的原生值；其它列（GROUP BY 的键）→ 节点/字面量。
fn column_json(kind: &ColumnKind, text: &str) -> Value {
    match kind {
        ColumnKind::Aggregate { func } => aggregate_json(func, text),
        ColumnKind::Literal { xsd, .. } => literal_json(text, xsd),
        // 分组键是实例 IRI：保持完整 IRI（summary 里没有前缀缩写的语境）。
        ColumnKind::Node { .. } | ColumnKind::Unknown => json!(text),
    }
}

/// 聚合值的原生表示：`COUNT` 恒是整数；其余能解析成数字就给数字，否则保留原文。
///
/// ⚠️ 为什么不是「一律 `parse::<i64>`」：`SUM` / `AVG` 在 `numeric` 列上是小数，
/// `MIN` / `MAX` 在 `text` 列上是字符串 —— 硬转会把 `123.45` 变成「解析失败 ⇒ 原文/空」。
fn aggregate_json(func: &str, text: &str) -> Value {
    let t = text.trim();
    if func == "COUNT" {
        return match t.parse::<i64>() {
            Ok(n) => json!(n),
            Err(_) => json!(text),
        };
    }
    number_or_text(t)
}

/// 字面量的 JSON 形式（需求：**数字 / 布尔用原生 JSON**，其余按 `rr:datatype` 标 `@type`）。
///
/// - `xsd:integer` / `xsd:decimal`：能解析成 JSON 数字就给数字（`"5"` → `5`）；
/// - `xsd:boolean`：`true/false/t/f/1/0` → 原生布尔；
/// - `xsd:date` / `xsd:dateTime`：**不**降级成裸字符串（那会被读成 `xsd:string`），
///   写成 `{"@value": …, "@type": "xsd:date"}`；
/// - 其余（含 `xsd:string`、以及类型未知的兜底）：裸字符串（JSON-LD 默认就是 `xsd:string`）。
///
/// ⚠️ 解析失败**不报错、不丢值**：原文照出（数据库里也真有 `'N/A'` 进数值列的场景）。
fn literal_json(text: &str, xsd: &str) -> Value {
    let t = text.trim();
    match xsd {
        "integer" => match t.parse::<i64>() {
            Ok(n) => json!(n),
            Err(_) => json!(text),
        },
        "decimal" => number_or_text(t),
        "boolean" => match t {
            "true" | "t" | "1" => json!(true),
            "false" | "f" | "0" => json!(false),
            _ => json!(text),
        },
        // 有专名的类型必须带 `@type`（裸字符串在 JSON-LD 里恒为 xsd:string）。
        "date" => json!({ "@value": text, "@type": "xsd:date" }),
        "dateTime" => json!({ "@value": text, "@type": "xsd:dateTime" }),
        _ => json!(text),
    }
}

/// 能当 JSON 数字就写数字（保留 PG 的十进制文本：`1.5000` → `1.5`），否则原文。
fn number_or_text(t: &str) -> Value {
    match t.parse::<serde_json::Number>() {
        Ok(n) => Value::Number(n),
        Err(_) => json!(t),
    }
}

/// 完整实例 IRI → `ontok:<类>/<主键>`（**只**做前缀缩写，不改判定）。
///
/// 不认识的 IRI 原样保留：⛔ 不猜、不截断、不补前缀。
fn abbreviate(iri: &str, base: &str) -> String {
    match iri.strip_prefix(base) {
        Some(rest) => format!("{ONTOK_PREFIX}:{rest}"),
        None => iri.to_string(),
    }
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数，不连库、不起 socket）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::JOIN_STATES_SCOPE_NOTE;

    /// 本体 IRI（夹具用**短**基址：本模块只认「传进来的那一个 `base`」，
    /// 与真实项目名无关 —— 真实值由 `svc.ontology_iri` 给）。
    const ONT: &str = "https://x";

    /// 一个最小回执（只填本模块读的字段）。
    fn receipt(columns: &[&str], rows: Vec<Vec<Option<&str>>>) -> SparqlReceipt {
        let rows: Vec<Vec<Option<String>>> = rows
            .into_iter()
            .map(|r| r.into_iter().map(|v| v.map(str::to_string)).collect())
            .collect();
        SparqlReceipt {
            columns: columns.iter().map(|c| c.to_string()).collect(),
            row_count: rows.len(),
            rows,
            truncated: true,
            sql_echo: "SELECT 1".into(),
            tenant_applied: vec![],
            tables: vec!["llm_model".into()],
            public_dimensions: vec![],
            warnings: vec!["覆盖范围 main_only：deductions".into()],
            coverage: vec![CoverageEntry {
                table: "llm_model".into(),
                coverage: "main_only",
            }],
            join_states: vec![JoinStateEntry {
                from: "a".into(),
                to: "b".into(),
                predicate: "a__b".into(),
                used: false,
                state: "true",
                evidence: "value-overlap 5/5".into(),
            }],
            join_states_scope: "all_edges_between_involved_tables",
            join_states_scope_note: JOIN_STATES_SCOPE_NOTE,
            limits: LimitInfo {
                max_rows: 200,
                token_max_rows: 200,
                timeout_ms: 5000,
            },
            // ⭐ REQ-145 · 夹具 = **没加载映射**的部署（三个字段都不出现，R1）。
            complete: None,
            unresolved_classes: None,
        }
    }

    fn node_col(name: &str) -> ProjectedColumn {
        ProjectedColumn {
            name: name.into(),
            kind: ColumnKind::Node {
                class: "llm_model".into(),
                class_local: "llm_model".into(),
            },
        }
    }

    fn literal_col(name: &str, predicate: &str, xsd: &'static str) -> ProjectedColumn {
        ProjectedColumn {
            name: name.into(),
            kind: ColumnKind::Literal {
                class: "llm_model".into(),
                column: name.into(),
                predicate: predicate.into(),
                xsd,
            },
        }
    }

    fn agg_col(name: &str, func: &'static str) -> ProjectedColumn {
        ProjectedColumn {
            name: name.into(),
            kind: ColumnKind::Aggregate { func },
        }
    }

    /// 任意类的实例列（`class` 同时当类 key 与本地名，与夹具一致）。
    fn node_col_of(name: &str, class: &str) -> ProjectedColumn {
        ProjectedColumn {
            name: name.into(),
            kind: ColumnKind::Node {
                class: class.into(),
                class_local: class.into(),
            },
        }
    }

    /// 任意类的字面量列。
    fn literal_col_of(
        name: &str,
        class: &str,
        predicate: &str,
        xsd: &'static str,
    ) -> ProjectedColumn {
        ProjectedColumn {
            name: name.into(),
            kind: ColumnKind::Literal {
                class: class.into(),
                column: name.into(),
                predicate: predicate.into(),
                xsd,
            },
        }
    }

    fn value_of(g: &JsonLd<'_>) -> Value {
        g.to_value()
    }

    /// AC-002：形状（`@context` / `@graph` / `meta`）+ 节点含 `@id` / `@type` / 至少一个属性。
    #[test]
    fn shape_is_context_graph_meta_with_typed_nodes() {
        let r = receipt(
            &["m", "name"],
            vec![vec![Some("https://x#llm_model/9bab"), Some("Claude")]],
        );
        let g = render(&r, &[node_col("m"), literal_col("name", "llm_model__model_name", "string")], ONT);
        let v = value_of(&g);
        assert_eq!(v["@context"]["ontok"], format!("{ONT}#"));
        assert_eq!(v["@context"]["xsd"], XSD_NS);
        let graph = v["@graph"].as_array().expect("@graph 是数组");
        assert_eq!(graph.len(), 1);
        let n = &graph[0];
        assert_eq!(n["@id"], "ontok:llm_model/9bab");
        assert_eq!(n["@type"], "ontok:llm_model");
        assert_eq!(n["ontok:llm_model__model_name"], "Claude");
        assert!(v["meta"].is_object());
        // 顶层键顺序 = 声明顺序（@context → @graph → summary → meta）
        let text = g.to_text();
        let at = |k: &str| text.find(&format!("\"{k}\"")).expect(k);
        assert!(at("@context") < at("@graph") && at("@graph") < at("summary") && at("summary") < at("meta"));
    }

    /// AC-003（**分水岭**）：同一实体多行 ⇒ `@graph` 里只出现一次，属性并起来。
    #[test]
    fn the_same_id_from_many_rows_is_merged_into_one_node() {
        let r = receipt(
            &["c", "o"],
            vec![
                vec![Some("https://x#customers/C1"), Some("https://x#orders/O1")],
                vec![Some("https://x#customers/C1"), Some("https://x#orders/O2")],
                vec![Some("https://x#customers/C1"), Some("https://x#orders/O3")],
            ],
        );
        let mut c = node_col("c");
        c.kind = ColumnKind::Node {
            class: "customers".into(),
            class_local: "customers".into(),
        };
        let mut o = node_col("o");
        o.kind = ColumnKind::Node {
            class: "orders".into(),
            class_local: "orders".into(),
        };
        let g = render(&r, &[c, o], ONT);
        let v = value_of(&g);
        let graph = v["@graph"].as_array().unwrap();
        // 3 行 ⇒ 4 个节点（1 客户 + 3 订单）：同 `@id` 只一份。
        assert_eq!(graph.len(), 4, "{v}");
        assert_eq!(v["summary"]["row_count"], 3);
        assert_eq!(v["summary"]["nodes"], 4);
        let ids: Vec<&str> = graph.iter().map(|n| n["@id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            vec!["ontok:customers/C1", "ontok:orders/O1", "ontok:orders/O2", "ontok:orders/O3"],
            "节点必须按 @id 升序"
        );
    }

    /// ⭐ R-1a · **冗余不漂**（⛔ 不是字面数字）：`summary.nodes` 恒等于 `@graph` 长度，
    /// `summary.row_count` 恒等于 `meta.row_count`（= 回执的 `row_count`），
    /// 且 `@graph` 里没有重复 `@id`。
    ///
    /// # 为什么必须写成「关系」而不是「数字」
    ///
    /// `assert_eq!(summary.nodes, 61)` 只在**数字被改**时红；它抓不住真正危险的那件事：
    /// 两个字段**各算一遍**、并在某类数据下漂移。最典型的是把 `nodes` 写成 `row_count` ——
    /// 「1 行 1 个实体」时两者恰好相等，**只有「多行同实体」才会露出来**。
    /// 所以这里不写任何字面数字，只钉关系，且数据覆盖最容易让两者不等的四组：
    /// **0 行 / 1 行 / 多行含重复实体（节点 < 行）/ 每行多个实体（节点 > 行）**。
    ///
    /// # 变异自证（QA 可复现）
    ///
    /// 把 `render()` 里的 `summary: plain_summary(nodes.len(), receipt.row_count, detached)`
    /// 改成 `plain_summary(receipt.row_count, receipt.row_count, detached)`
    /// ⇒ 本判据在**第 3 组**（3 行 → 1 节点）与**第 4 组**（3 行 → 4 节点）立刻红。
    #[test]
    fn summary_counts_are_invariants_of_the_graph_not_magic_numbers() {
        let customer = || node_col_of("c", "customers");
        let customer_name = || literal_col_of("n", "customers", "customers__customer_name", "string");
        let order = || node_col_of("o", "orders");

        let c1 = "https://x#customers/C1";
        let cases: Vec<(&str, SparqlReceipt, Vec<ProjectedColumn>)> = vec![
            (
                "0 行（空结果）",
                receipt(&["c", "n"], vec![]),
                vec![customer(), customer_name()],
            ),
            (
                "1 行（1 实体）",
                receipt(&["c", "n"], vec![vec![Some(c1), Some("Ada")]]),
                vec![customer(), customer_name()],
            ),
            (
                "多行含重复实体（3 行 → 1 节点）",
                receipt(
                    &["c", "n"],
                    vec![
                        vec![Some(c1), Some("Ada")],
                        vec![Some(c1), Some("Ada")],
                        vec![Some(c1), Some("Ada")],
                    ],
                ),
                vec![customer(), customer_name()],
            ),
            (
                "每行多个实体（3 行 → 4 节点）",
                receipt(
                    &["o", "c"],
                    vec![
                        vec![Some("https://x#orders/O1"), Some(c1)],
                        vec![Some("https://x#orders/O2"), Some(c1)],
                        vec![Some("https://x#orders/O3"), Some(c1)],
                    ],
                ),
                vec![order(), customer()],
            ),
        ];

        for (case, r, proj) in cases {
            let g = render(&r, &proj, ONT);
            let v = value_of(&g);
            let graph = v["@graph"]
                .as_array()
                .unwrap_or_else(|| panic!("{case}: @graph 必须是数组：{v}"));
            // ① `nodes` 与 `@graph` 的长度是**同一个数**（⛔ 两处各算一遍）
            assert_eq!(
                v["summary"]["nodes"],
                json!(graph.len()),
                "{case}: summary.nodes 必须恒等于 @graph 长度（漂了就是两处各算一遍）"
            );
            // ② `row_count` 两边同源（meta 借用回执，summary 也是回执的那个值）
            assert_eq!(
                v["summary"]["row_count"],
                json!(r.row_count),
                "{case}: summary.row_count 必须是回执的 row_count"
            );
            assert_eq!(
                v["summary"]["row_count"], v["meta"]["row_count"],
                "{case}: summary.row_count 必须恒等于 meta.row_count"
            );
            // ③ `@graph` 里没有重复 `@id`（否则「节点数」就没有意义）
            let ids: Vec<&str> = graph.iter().map(|n| n["@id"].as_str().unwrap()).collect();
            let mut uniq = ids.clone();
            uniq.sort_unstable();
            uniq.dedup();
            assert_eq!(uniq.len(), ids.len(), "{case}: @graph 里不许有重复 @id：{ids:?}");
        }

        // 不变式的**适用范围**是「非聚合」：聚合没有实体 ⇒ summary 里只有聚合别名，
        // ⛔ 不许为了「让不变式好看」给它补一个 nodes/row_count。
        let agg = value_of(&render(
            &receipt(&["n"], vec![vec![Some("42")]]),
            &[agg_col("n", "COUNT")],
            ONT,
        ));
        assert_eq!(agg["@graph"], json!([]));
        assert!(agg["summary"].get("nodes").is_none(), "聚合结果不该有 nodes：{agg}");
        assert!(
            agg["summary"].get("row_count").is_none(),
            "聚合结果不该有 row_count：{agg}"
        );
    }

    /// 字面量挂到**它自己那个类**的节点上（⛔ 不是「本行第一个节点」）。
    #[test]
    fn a_literal_lands_on_the_node_of_its_own_class_not_the_first_one() {
        let mut o = node_col("o");
        o.kind = ColumnKind::Node {
            class: "orders".into(),
            class_local: "orders".into(),
        };
        let mut name = literal_col("n", "customers__customer_name", "string");
        name.kind = ColumnKind::Literal {
            class: "customers".into(),
            column: "customer_name".into(),
            predicate: "customers__customer_name".into(),
            xsd: "string",
        };
        let r = receipt(
            &["o", "c", "n"],
            vec![vec![
                Some("https://x#orders/O1"),
                Some("https://x#customers/C1"),
                Some("Ada"),
            ]],
        );
        let mut c = node_col("c");
        c.kind = ColumnKind::Node {
            class: "customers".into(),
            class_local: "customers".into(),
        };
        let v = value_of(&render(&r, &[o, c, name], ONT));
        let graph = v["@graph"].as_array().unwrap();
        let order = graph.iter().find(|n| n["@id"] == "ontok:orders/O1").unwrap();
        let cust = graph.iter().find(|n| n["@id"] == "ontok:customers/C1").unwrap();
        assert!(order.get("ontok:customers__customer_name").is_none(), "{order}");
        assert_eq!(cust["ontok:customers__customer_name"], "Ada");
    }

    /// AC-004：IRI / 数字 / 字符串 / 布尔 / 日期 各一条。
    #[test]
    fn nodes_and_literals_are_told_apart_by_the_translator_not_by_the_string() {
        let r = receipt(
            &["m", "n", "s", "b", "d"],
            vec![vec![
                Some("https://x#llm_model/1"),
                Some("42"),
                Some("hello"),
                Some("true"),
                Some("2026-01-02"),
            ]],
        );
        let v = value_of(&render(
            &r,
            &[
                node_col("m"),
                literal_col("n", "llm_model__ctx", "integer"),
                literal_col("s", "llm_model__model_name", "string"),
                literal_col("b", "llm_model__active", "boolean"),
                literal_col("d", "llm_model__since", "date"),
            ],
            ONT,
        ));
        let n = &v["@graph"][0];
        assert_eq!(n["@id"], "ontok:llm_model/1", "IRI ⇒ @id（并缩成 ontok:）");
        assert_eq!(n["ontok:llm_model__ctx"], 42, "整数 ⇒ 原生 JSON 数字");
        assert_eq!(n["ontok:llm_model__model_name"], "hello", "字符串 ⇒ 裸字符串");
        assert_eq!(n["ontok:llm_model__active"], true, "布尔 ⇒ 原生布尔");
        assert_eq!(
            n["ontok:llm_model__since"],
            json!({"@value": "2026-01-02", "@type": "xsd:date"}),
            "有专名的 datatype 必须带 @type"
        );
    }

    /// AC-005（**不许吞警告**）：`meta.warnings` / `coverage` 与行回执逐字相同。
    #[test]
    fn meta_carries_the_same_warnings_and_coverage_as_the_rows_receipt() {
        let r = receipt(&["m"], vec![vec![Some("https://x#llm_model/1")]]);
        let g = render(&r, &[node_col("m")], ONT);
        let v = value_of(&g);
        let rows_side: Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v["meta"]["warnings"], rows_side["warnings"]);
        assert_eq!(v["meta"]["coverage"], rows_side["coverage"]);
        assert_eq!(v["meta"]["truncated"], rows_side["truncated"]);
        assert_eq!(v["meta"]["join_states"], rows_side["join_states"]);
        assert_eq!(v["meta"]["tenant_applied"], rows_side["tenant_applied"]);
        assert_eq!(v["meta"]["sql_echo"], rows_side["sql_echo"]);
        // ⛔ AC-011：不回 rows（也不回 columns）
        assert!(v.get("rows").is_none(), "JSON-LD 不许回 rows：{v}");
        assert!(v["meta"].get("rows").is_none());
        assert!(v["meta"].get("columns").is_none());
    }

    /// ⭐ AC-005 的**字节级**判据（不是「值相同」那么弱）：`meta` 里的每一个子块
    /// 与行回执正文里那一段**逐字节相同** —— 因为 `Meta` 借用的就是同一次序列化。
    ///
    /// 变异 ③（把 `warnings` 从 `Meta` 里删掉）⇒ 本判据第一条就红。
    #[test]
    fn meta_text_is_the_rows_receipt_minus_columns_and_rows() {
        let r = receipt(
            &["m", "name"],
            vec![vec![Some("https://x#llm_model/1"), Some("Claude")]],
        );
        let text = render(&r, &[node_col("m"), literal_col("name", "llm_model__model_name", "string")], ONT)
            .to_text();
        let rows_text = r.serialize();
        // 行回执正文 = `{"columns":…,"rows":…,` + **meta 的全部字段** + `}`
        let columns = serde_json::to_string(&r.columns).unwrap();
        let rows = serde_json::to_string(&r.rows).unwrap();
        let prefix = format!("{{\"columns\":{columns},\"rows\":{rows},");
        let tail = rows_text
            .strip_prefix(&prefix)
            .expect("行回执正文必须以 columns/rows 两个字段开头");
        let meta_text = format!("{{{tail}");
        assert!(
            text.contains(&meta_text),
            "meta 必须逐字等于行回执去掉 columns/rows 的那一段：\n  meta={meta_text}\n  text={text}"
        );
        // 逐块再钉一遍（三个最容易被「顺手省掉」的字段）
        for block in [
            format!("\"warnings\":{}", serde_json::to_string(&r.warnings).unwrap()),
            format!("\"coverage\":{}", serde_json::to_string(&r.coverage).unwrap()),
            format!("\"tenant_applied\":{}", serde_json::to_string(&r.tenant_applied).unwrap()),
            format!("\"join_states\":{}", serde_json::to_string(&r.join_states).unwrap()),
            format!("\"limits\":{}", serde_json::to_string(&r.limits).unwrap()),
        ] {
            assert!(text.contains(&block), "meta 里少了/改了 {block}：{text}");
        }
        // ⛔ 双份：JSON-LD 正文里没有 columns / rows
        assert!(!text.contains("\"rows\":["), "⛔ 正文里出现了 rows 数组：{text}");
        assert!(!text.contains("\"columns\":["), "⛔ 正文里出现了 columns 数组：{text}");
    }

    /// AC-006 / R4：聚合 ⇒ `@graph: []`、值进 `summary`（⛔ 不造假节点）。
    #[test]
    fn an_aggregate_goes_to_summary_and_never_creates_a_fake_node() {
        let r = receipt(&["n"], vec![vec![Some("42")]]);
        let v = value_of(&render(&r, &[agg_col("n", "COUNT")], ONT));
        assert_eq!(v["@graph"], json!([]));
        assert_eq!(v["summary"]["n"], 42, "COUNT 是原生数字");
        // GROUP BY：多行 ⇒ groups[]（同样不进 @graph）
        let r = receipt(
            &["c", "n"],
            vec![
                vec![Some("https://x#customers/C1"), Some("2")],
                vec![Some("https://x#customers/C2"), Some("3")],
            ],
        );
        let mut c = node_col("c");
        c.kind = ColumnKind::Node {
            class: "customers".into(),
            class_local: "customers".into(),
        };
        let v = value_of(&render(&r, &[c, agg_col("n", "COUNT")], ONT));
        assert_eq!(v["@graph"], json!([]));
        assert_eq!(v["summary"]["n_groups"], 2);
        assert_eq!(v["summary"]["groups"][0]["n"], 2);
    }

    /// AC-008 / R5：空结果 ⇒ `@graph: []`（数组，不是 null、不缺字段）。
    #[test]
    fn an_empty_result_is_an_empty_graph_not_null() {
        let r = receipt(&["m", "name"], vec![]);
        let g = render(&r, &[node_col("m"), literal_col("name", "llm_model__model_name", "string")], ONT);
        let v = value_of(&g);
        assert_eq!(v["@graph"], json!([]));
        assert!(v["@graph"].is_array());
        assert_eq!(v["summary"]["nodes"], 0);
        assert!(v.get("@context").is_some() && v.get("meta").is_some());
        assert!(g.to_text().contains("\"@graph\":[]"), "空结果必须写成 `[]`：{}", g.to_text());
    }

    /// AC-007 / R2：同一份输入两次 ⇒ 逐字节相同；且属性键 / 节点顺序与**行的顺序**无关。
    #[test]
    fn the_same_input_is_byte_identical_twice_and_ignores_row_order() {
        let proj = [
            literal_col("name", "llm_model__model_name", "string"),
            node_col("m"),
        ];
        let a = receipt(
            &["name", "m"],
            vec![
                vec![Some("B"), Some("https://x#llm_model/2")],
                vec![Some("A"), Some("https://x#llm_model/1")],
            ],
        );
        let b = receipt(
            &["name", "m"],
            vec![
                vec![Some("A"), Some("https://x#llm_model/1")],
                vec![Some("B"), Some("https://x#llm_model/2")],
            ],
        );
        let ga = render(&a, &proj, ONT).to_text();
        let ga2 = render(&a, &proj, ONT).to_text();
        let gb = render(&b, &proj, ONT).to_text();
        assert_eq!(ga, ga2, "同输入两次必须逐字节相同");
        assert_eq!(ga, gb, "节点按 @id / 属性按键排序 ⇒ 与行序无关");
        assert!(
            ga.find("ontok:llm_model/1").unwrap() < ga.find("ontok:llm_model/2").unwrap(),
            "@id 升序：{ga}"
        );
    }

    /// 没有主语可挂的字面量进 `summary.detached`（⛔ 不造一个 `@id` 为空的假节点）。
    #[test]
    fn literals_without_a_projected_entity_go_to_summary_not_to_a_fake_node() {
        let r = receipt(&["n"], vec![vec![Some("Ada")], vec![Some("Bob")]]);
        let v = value_of(&render(&r, &[literal_col("n", "customers__customer_name", "string")], ONT));
        assert_eq!(v["@graph"], json!([]));
        assert_eq!(v["summary"]["nodes"], 0);
        let d = v["summary"]["detached"].as_array().unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!(d[0]["predicate"], "customers__customer_name");
    }
}

