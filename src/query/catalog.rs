//! REQ-123 · **catalog / describe** —— Agent 在写 SPARQL 之前「看有什么可用」的两个出口。
//!
//! ⭐ REQ-136 · **两个数据来源、一个构建函数**（普通模式 / 本体服务模式）：
//!
//! | | 普通模式（有源库） | 本体服务模式（`--ontology-only`） |
//! | --- | --- | --- |
//! | 类 / 关系从哪来 | 同一份快照，**只留**映射里可查的（[`R2rmlMapping`]） | 同一份快照，**全留**（[`ModelTrackView`]） |
//! | 属性 | 映射（含 schema 补出的主键属性） | 快照（⛔ 不补主键：没有 schema 就没有主键） |
//! | 关系的 `evidence_level` | 映射的 `MappedJoin::state`（`true`/`inherit`/`false`） | 快照的 `join_verified`（多一个 **`unverified`** = 这一版没记录，TTL 里不写 `ontok:joinVerified`） |
//! | `counts.excluded_relations` | 有（「少了哪些」是可读到的数字） | **不出现**（没有对账 ⇒ 没有「被排除的边」这个类别） |
//! | `display_name` / `key` | 不出现（**既有响应的字节不变**，AC-006） | 出现（快照的 `rdfs:label` / 系统标识） |
//!
//! 分叉只在**过滤与取值**：遍历快照的那一段是 [`snapshot_index`] **一个函数**
//! （纪律 H —— 两份遍历一旦漂移，就是「同一份快照，两个模式给出两套本体」，而且不会报错）。
//!
//! # 数据从哪来（⛔ 不猜、不编）
//!
//! | 事实 | 来源 |
//! | --- | --- |
//! | 类 / 属性 / 关系的**可用名字** | [`crate::domain::r2rml::R2rmlMapping`]（与 `sparql query` **同一个** `build_mapping`）；本体模式改用快照 + [`crate::domain::ttl::local_model`] / [`crate::domain::ttl::model_link_locals`]（与 TTL 同名规则**同一份**） |
//! | 关系**证据等级** | 映射里的 `MappedJoin::state`（`true` / `inherit` / `false`）+ `evidence` 原文；本体模式取快照的 `join_verified` / `join_evidence`（`None` ⇒ `unverified`） |
//! | 枚举值 / 可空 / 样例 | 建模视图的 `ModelPropertyView.note / nullable / sample`；枚举值只认注释里**明写**的那几种形态（[`crate::domain::ttl::parse_allowed_values`]，⛔ 不编） |
//! | 租户口径 | `query-policy.json`（`QueryPolicy::decide`）；本体模式恒 `not_applicable`（没有数据源） |
//!
//! # 为什么普通模式只列**映射里的**关系
//!
//! 目录的用途是「照着写一条能跑的 SPARQL」。映射里没有的边（证据 `inherit` 且
//! 服务未开 `--include-inherit`、或 `false`）**写进查询会被 planner 拒绝** ——
//! 把它们列进目录等于教调用方写一条注定失败的查询。
//! 所以：`catalog` 只列**可查**的边，并在 `counts` 里给出 `excluded_relations` 的条数
//! （来自映射的审计信息），让「少了哪些」是一个**可读到的数字**而不是沉默。
//!
//! ⭐ 本体服务模式**反过来**：那里本来就查不了数据（SPARQL 恒 503），目录的用途变成
//! 「告诉外部平台这个本体有什么」—— 于是**边必须全列**，⛔ 不许用「join 对不上」
//! 把它们吃掉（R3；本需求存在的理由）。
//!
//! # 分页
//!
//! 三类条目（class / property / relation）拍平成**一个**稳定序列再分页：
//! 类（按名字）→ 属性（按名字）→ 关系（按名字）。`total_items` / `total_pages` /
//! `has_more` 都从同一个序列算出来，⛔ 不让调用方自己猜「还有没有」。

use serde::Serialize;

use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView};
use crate::domain::r2rml::R2rmlMapping;
use crate::domain::ttl::{effective_column, local_model, model_link_locals, parse_allowed_values, xsd_range};

use super::error::ApiError;
use super::QueryService;

/// 一页最多多少条（`page_size` 的硬上限）。
pub const MAX_PAGE_SIZE: usize = 200;
/// 默认每页条数。
pub const DEFAULT_PAGE_SIZE: usize = 50;

/// 目录里的一条。
///
/// `kind` 是判别式：`class` / `property` / `relation`。三类的字段都**平铺**在同一层，
/// 这样 Agent 只需要读 `kind` 再取字段，不需要按 kind 找子对象。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatalogItem {
    /// 一个类（= 一张表 = 一个 TriplesMap）。
    Class(ClassItem),
    /// 一个数据类型属性（= 一列）。
    Property(PropertyItem),
    /// 一条可查的关系（= 一个 `rr:joinCondition`）。
    Relation(RelationItem),
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassItem {
    /// SPARQL 里写的名字（`:customers` 去掉冒号后的部分）。
    pub name: String,
    /// 源表名（= 映射的类 key）。
    pub table: String,
    pub primary_key: String,
    pub property_count: usize,
    /// **前向**关系条数（反向可由 `inverse` 看到）。
    pub relation_count: usize,
    pub description: Option<String>,
    /// ⭐ REQ-136 · 快照里的人读名（`rdfs:label`，例：`云资源`）。
    ///
    /// **只在**本体服务模式填（普通模式 `None` ⇒ 字段不出现）：既有响应的字节不变（AC-006）。
    /// 为什么还要它：本体模式的类名是 `local_model(key)`（`doc_01_6ab92914`），
    /// 只看 `name` 认不出业务对象；`display_name` 与 TTL 里的 `rdfs:label`／客户视图的
    /// `:云资源` 是同一个事实。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ⭐ REQ-145 · 这个类的**数据源**（该走哪个入口取数）。
    ///
    /// `None`（⇒ 字段不出现）= 这个部署没加载 `--access-mapping`：既有响应逐字节不变（R1）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<ClassAccess>,
}

/// ⭐ REQ-145 · 一个类「数据在哪、怎么取」—— `catalog` / `describe` 的 `access` 字段。
///
/// 两个形状（契约 §八 第 3 点）：
///
/// ```json
/// { "kind": "api", "queryable_by": ["fetch"], "pushdown_fields": ["ids"], "freshness": "realtime" }
/// { "kind": "db",  "queryable_by": ["sparql"] }
/// ```
///
/// `queryable_by` 是**许可清单**，不是建议：`["sparql"]` 的类可以写进 SPARQL；
/// `["fetch"]` 的类写进 SPARQL 只会拿到 `@pending`（jsonld）或 `NOT_DB_SOURCE`（rows）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ClassAccess {
    /// `api` / `db`。
    pub kind: &'static str,
    /// `["fetch"]`（api）/ `["sparql"]`（db）。
    pub queryable_by: Vec<&'static str>,
    /// 能直接下推给 API 的过滤字段（只有 api 有；映射没说就**不出现**，⛔ 不编）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pushdown_fields: Option<Vec<String>>,
    /// 数据新鲜度（只有 api 可能有；映射没写就不出现）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,
}

impl ClassAccess {
    /// 库源：`sparql` 可查，没有「下推字段」这回事。
    pub fn db() -> Self {
        Self {
            kind: "db",
            queryable_by: vec!["sparql"],
            pushdown_fields: None,
            freshness: None,
        }
    }

    /// api 源：只能 `fetch`（平台不代取 —— 凭据不出客户边界）。
    pub fn api(entity: &super::access::Entity) -> Self {
        Self {
            kind: "api",
            queryable_by: vec!["fetch"],
            pushdown_fields: Some(entity.pushdown.clone()),
            freshness: entity.freshness.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PropertyItem {
    /// SPARQL 谓词名（`:类__列`）。
    pub name: String,
    /// 所属类的名字。
    pub class: String,
    /// 源列名。
    pub column: String,
    /// `xsd:string` / `xsd:decimal` / …
    pub datatype: String,
    /// 注释里**明写**的枚举值域（没有就是 `null`，⛔ 不编）。
    pub enum_values: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationItem {
    /// 前向谓词名（`:子类__父类`，有歧义时带列名）。
    pub name: String,
    /// 反向谓词名（`:父类__子类`）—— 反向查询用它。
    pub inverse: String,
    pub from: String,
    pub to: String,
    /// 实现这条关系的**子表列**。
    pub column: String,
    pub cardinality: Option<String>,
    /// 证据等级：`true`（实测）/ `inherit`（继承）/ `false`（仅名字推断）。
    pub evidence_level: String,
    /// 证据原文（`value-overlap 5/5 …`）。
    pub evidence: String,
}

/// 各类条目的计数（一次翻页看不全时，调用方据此判断值不值得翻）。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogCounts {
    pub classes: usize,
    pub properties: usize,
    pub relations: usize,
    /// 映射里**没有生成** join 的边数（`inherit` / `false` / 未标）—— 它们不可查。
    ///
    /// ⭐ REQ-136 · 本体服务模式**没有这个字段**（`None` ⇒ 不序列化）：
    /// 那里不做映射对账，「被排除的边」这个类别不存在 —— 关系**全部列出**（R3）。
    /// 反过来说：出现 `excluded_relations=10` 就说明这份目录是走映射对账出来的，
    /// 那正是本需求要消灭的形状（AC-002 的判据之一）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded_relations: Option<usize>,
    /// 没有 TriplesMap 的类（复合主键 / 无主键）—— 它们不可查，但**要说出来**。
    pub skipped_classes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedClass {
    pub table: String,
    pub reason: String,
}

/// 一页目录。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogPage {
    pub project: String,
    pub version: i32,
    pub page: usize,
    pub page_size: usize,
    pub total_items: usize,
    pub total_pages: usize,
    pub has_more: bool,
    pub counts: CatalogCounts,
    /// 不可查的类（⛔ 不是静默少了几张表）。
    pub skipped_classes: Vec<SkippedClass>,
    pub items: Vec<CatalogItem>,
}

/// 一个类的详情。
#[derive(Debug, Clone, Serialize)]
pub struct ClassDetail {
    pub name: String,
    pub table: String,
    pub primary_key: String,
    pub description: Option<String>,
    /// ⭐ REQ-136 · 快照里的人读名（`rdfs:label`，例：`云资源`）。
    ///
    /// 只有本体服务模式填（普通模式 `None` ⇒ 字段不出现，既有响应逐字节不变）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// ⭐ REQ-136 · 类的**系统标识**（= 快照 `entity_types.key`，与 `table` 同值）。
    ///
    /// 为什么要重复一个字段：`name` 是 SPARQL/IRI 侧的本地名（`local_model(key)`），
    /// `key` 是建模侧的系统标识 —— 对外平台用它回指 catalog 里的 `table`；
    /// 三者在**普通模式**下由 `table` 表达，本体模式显式给出，免得调用方去猜「哪个是 key」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// 租户口径（来自声明，不是代码推断）。
    ///
    /// 本体服务模式下 `mode = "not_applicable"`：没有数据源 ⇒ 没有「按哪一列过滤」这回事。
    pub tenant: TenantInfo,
    /// ⭐ REQ-145 · 这个类的**数据源**（`None` = 没加载映射 ⇒ 字段不出现）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<ClassAccess>,
    pub properties: Vec<PropertyDetail>,
    pub relations: Vec<RelationDetail>,
}

/// 一份租户决定的人读形状。
#[derive(Debug, Clone, Serialize)]
pub struct TenantInfo {
    /// `filter` / `public` / `unregistered` / **`not_applicable`**（本体服务模式）。
    pub mode: String,
    pub column: Option<String>,
    /// `complete` / `main_only`（`mode = filter` 时有值）。
    pub coverage: Option<String>,
    /// 为什么这么定（声明的 `reason`）。
    pub reason: Option<String>,
}

/// 一个属性的详情。
#[derive(Debug, Clone, Serialize)]
pub struct PropertyDetail {
    pub name: String,
    pub class: String,
    pub column: String,
    pub datatype: String,
    pub enum_values: Option<String>,
    pub nullable: Option<bool>,
    pub sample: Option<String>,
    pub note: Option<String>,
    /// ⭐ REQ-136 · 快照里的业务名（`资源总数` 这类）。只有本体服务模式填。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// 一条关系的详情 + 它在这追查询里能不能用。
#[derive(Debug, Clone, Serialize)]
pub struct RelationDetail {
    pub name: String,
    pub inverse: String,
    pub from: String,
    pub to: String,
    pub column: String,
    pub cardinality: Option<String>,
    pub evidence_level: String,
    pub evidence: String,
    /// `false` = 映射里没有这条边（写了会被 planner 拒绝）。
    ///
    /// ⭐ 本体服务模式**恒 `false`**：那个部署没有数据源，任何边都查不了
    /// （这不是「这条边不可查」，而是「这个部署不查数据」—— 见 `TenantInfo.mode`）。
    pub queryable: bool,
}

/// `describe` 的结果：类或属性。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Description {
    Class(ClassDetail),
    Property(PropertyDetail),
}

// ══════════════════════════════════════════════════════════════
// 构建（启动时一次）
// ══════════════════════════════════════════════════════════════

/// ⭐ REQ-136 · **从快照取类 / 关系 / 属性**的唯一一份（纪律 H）。
///
/// ⛔ 本结构不含 schema、不含映射：它只把**快照投影**（[`ModelTrackView`]）变成
/// 「有哪些类、有哪些边、每条边叫什么」。两个模式拿的是**同一份**列表，
/// 分叉只在 [`build_items`] 里「留不留、用谁的数字」。
struct SnapshotIndex<'a> {
    /// 全部类，按**类本地名**（`local_model(key)`）升序 —— 与映射的排序口径一致。
    classes: Vec<&'a ModelEntityView>,
    /// 全部关系（按 `key` / `from` / `to`，与 `model_link_locals` 要求的顺序一致）。
    links: Vec<&'a ModelLinkView>,
    /// 与 `links` 一一对应的**前向**谓词本地名（`:子__父`）。
    forward: Vec<String>,
    /// 与 `links` 一一对应的**反向**谓词本地名（`:父__子`）。
    inverse: Vec<String>,
}

/// 唯一的「从快照取数」实现（两个模式共用；⛔ 不另写第二份遍历）。
fn snapshot_index(model: &ModelTrackView) -> SnapshotIndex<'_> {
    let mut classes: Vec<&ModelEntityView> = model.entity_types.iter().collect();
    classes.sort_by(|a, b| {
        local_model(&a.key)
            .cmp(&local_model(&b.key))
            .then_with(|| a.key.cmp(&b.key))
    });

    let mut links: Vec<&ModelLinkView> = model.link_types.iter().collect();
    links.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.from_type.cmp(&b.from_type))
            .then_with(|| a.to_type.cmp(&b.to_type))
    });
    // 谓词名与 TTL / R2RML 同源（REQ-120 的命名规则只有这一份）。
    let (forward, inverse) = model_link_locals(&links);
    SnapshotIndex {
        classes,
        links,
        forward,
        inverse,
    }
}

impl SnapshotIndex<'_> {
    /// 该类作为**子**出现的关系条数（= 映射里 `joins.len()` 的口径：只数前向）。
    fn forward_degree(&self, class_key: &str) -> usize {
        self.links
            .iter()
            .filter(|l| l.from_type == class_key)
            .count()
    }
}

/// 拍平成一个**稳定**序列（类 → 属性 → 关系，各自按名字升序）。
///
/// ⭐ REQ-136 · 两个模式**同一个函数**：
///
/// | 模式 | `mapping` | 类 / 关系从哪来 | 分叉点 |
/// | --- | --- | --- | --- |
/// | 普通（[`super::ServiceMode::Query`]） | `Some` | **只留**映射里可查的那些（类要有 TriplesMap，边要生成了 join） | `if m.class(key).is_some()` / mapping 的 join 列表 |
/// | 本体（[`super::ServiceMode::OntologyOnly`]） | `None` | **全留**快照里的类与边（⛔ 没有「对账不上」这个类别） | 无（不过滤） |
///
/// 为什么不是两份实现：两份遍历一旦漂移，就是「同一份快照，两个模式给出两套本体」，
/// 而且**不会报错**（本需求存在的理由正是这种静默丢失）。
pub fn build_items(
    model: &ModelTrackView,
    mapping: Option<&R2rmlMapping>,
    access: Option<&super::access::AccessMapping>,
) -> Vec<CatalogItem> {
    let idx = snapshot_index(model);
    let mut items: Vec<CatalogItem> = Vec::new();

    // ⭐ REQ-145 · 一个类的 `access`（`None` = 没加载映射 ⇒ 字段不出现，R1）。
    let class_access = |key: &str| -> Option<ClassAccess> {
        let a = access?;
        Some(match a.entity(key) {
            Some(e) => ClassAccess::api(e),
            None => ClassAccess::db(),
        })
    };

    // ── ① 类：同一份快照列表，只在「留不留 / 用谁的数字」处分叉 ──
    for e in &idx.classes {
        // ⭐ REQ-145 · api 类**也要列出来**（需求 §3）：即便它在映射里没有 TriplesMap
        // （复合主键 / 无主键会被跳过），只要 access 说它来自 API，Agent 就该知道它存在
        // ——「查了也没有数」与「这个类不存在」是两件完全不同的事。
        let is_api = access.is_some_and(|a| a.is_api(&e.key));
        let item = match mapping {
            // 普通模式：没有 TriplesMap 的类**不可查** ⇒ 不列（既有行为）。
            Some(m) => match m.class(&e.key) {
                Some(c) => ClassItem {
                    name: c.class_local.clone(),
                    table: c.key.clone(),
                    primary_key: c.pk.clone(),
                    property_count: c.properties.len(),
                    relation_count: c.joins.len(),
                    description: e.description.clone(),
                    display_name: None,
                    access: class_access(&e.key),
                },
                None if is_api => ClassItem {
                    // 映射里没有它（比如主键不唯一）⇒ 主键**不报**（⛔ 不猜一列）。
                    name: local_model(&e.key),
                    table: e.key.clone(),
                    primary_key: String::new(),
                    property_count: e.properties.len(),
                    relation_count: idx.forward_degree(&e.key),
                    description: e.description.clone(),
                    display_name: None,
                    access: class_access(&e.key),
                },
                None => continue,
            },
            // 本体模式：全部列。没有 schema ⇒ 没有主键（`primary_key` 是空串，
            // ⛔ 不拿 `id` 顶替：那会让调用方以为这个本体有实例标识列）。
            None => ClassItem {
                name: local_model(&e.key),
                table: e.key.clone(),
                primary_key: String::new(),
                property_count: e.properties.len(),
                relation_count: idx.forward_degree(&e.key),
                description: e.description.clone(),
                display_name: Some(e.display_name.clone()),
                access: class_access(&e.key),
            },
        };
        items.push(CatalogItem::Class(item));
    }

    // ── ② 属性：普通模式必须走映射（映射会补一条主键属性，且 `datatype` 来自 schema）；
    //     本体模式没有映射 ⇒ 直接取快照属性的列名（⛔ 不补主键、不猜类型）。──
    let mut properties: Vec<CatalogItem> = Vec::new();
    match mapping {
        Some(m) => {
            let mut props: Vec<(&str, &crate::domain::r2rml::MappedProperty)> = Vec::new();
            for c in m.classes() {
                for p in &c.properties {
                    props.push((c.key.as_str(), p));
                }
            }
            props.sort_by(|a, b| a.1.local.cmp(&b.1.local));
            for (class_key, p) in props {
                properties.push(CatalogItem::Property(PropertyItem {
                    name: p.local.clone(),
                    class: class_key.to_string(),
                    column: p.column.clone(),
                    datatype: p.xsd.iri().to_string(),
                    enum_values: enum_values_of(model, class_key, &p.column),
                }));
            }
        }
        None => {
            for e in &idx.classes {
                let mut props: Vec<&ModelPropertyView> = e.properties.iter().collect();
                props.sort_by(|a, b| {
                    a.column.cmp(&b.column).then_with(|| a.name.cmp(&b.name))
                });
                for p in props {
                    let column = effective_column(p);
                    if column.is_empty() {
                        continue; // ⛔ 连列名都没有的属性不猜（与映射生成器同一条 fail-closed）
                    }
                    let label = if p.name.trim().is_empty() {
                        column
                    } else {
                        p.name.trim()
                    };
                    // 与 TTL / R2RML 的 `rr:datatype` 是**同一个**推断函数（schema 为 None ⇒
                    // 列名/中文名启发式）。⛔ 这里不新增任何类型规则。
                    let (xsd, _basis, _source) =
                        xsd_range(Some(&e.key), Some(column), None, label, p.note.as_deref());
                    properties.push(CatalogItem::Property(PropertyItem {
                        name: format!("{}__{}", local_model(&e.key), local_model(column)),
                        class: e.key.clone(),
                        column: column.to_string(),
                        datatype: xsd.iri().to_string(),
                        enum_values: p.note.as_deref().and_then(parse_allowed_values),
                    }));
                }
            }
        }
    }
    // 属性按谓词名升序（两个模式**同一条**排序口径 ⇒ 分页稳定）。
    properties.sort_by(|a, b| match (a, b) {
        (CatalogItem::Property(x), CatalogItem::Property(y)) => x.name.cmp(&y.name),
        _ => std::cmp::Ordering::Equal,
    });
    items.append(&mut properties);

    // ── ③ 关系：同一份快照边列表，只在「留不留」与「用谁的证据」处分叉 ──
    let mut relations: Vec<CatalogItem> = Vec::new();
    match mapping {
        Some(m) => {
            let mut joins: Vec<(&str, &crate::domain::r2rml::MappedJoin)> = Vec::new();
            for c in m.classes() {
                for j in &c.joins {
                    joins.push((c.key.as_str(), j));
                }
            }
            joins.sort_by(|a, b| a.1.predicate_local.cmp(&b.1.predicate_local));
            for (_class_key, j) in joins {
                relations.push(CatalogItem::Relation(RelationItem {
                    name: j.predicate_local.clone(),
                    inverse: j.inverse_local.clone(),
                    from: j.child_table.clone(),
                    to: j.parent_table.clone(),
                    column: j.child.clone(),
                    cardinality: model_cardinality(model, &j.child_table, &j.parent_table, &j.child),
                    evidence_level: j.state.as_str().to_string(),
                    evidence: j.evidence.clone(),
                }));
            }
        }
        None => {
            // `idx.forward` / `idx.inverse` 与 `idx.links` **一一对应**（同一份排序 + 同一个
            // `model_link_locals`）—— 按下标取，不用再拼一次名字。
            for (i, l) in idx.links.iter().enumerate() {
                relations.push(CatalogItem::Relation(RelationItem {
                    name: idx.forward[i].clone(),
                    inverse: idx.inverse[i].clone(),
                    from: l.from_type.clone(),
                    to: l.to_type.clone(),
                    // 快照里没有实现列（文档类本体：关系只有语义）⇒ 空串，⛔ 不猜一列。
                    column: l.column.clone().unwrap_or_default(),
                    cardinality: Some(l.cardinality.clone()),
                    // `evidence_level` 就是 TTL 的 `ontok:joinVerified`：
                    // true / inherit / false / **unverified**（= 这一版没记录，TTL 里不写这一行）。
                    evidence_level: l
                        .join_verified
                        .map(|s| s.as_str().to_string())
                        .unwrap_or_else(|| "unverified".to_string()),
                    evidence: l.join_evidence.clone().unwrap_or_default(),
                }));
            }
        }
    }
    // 关系按谓词名升序（与普通模式的排序口径一致：同一份序列 ⇒ 分页稳定）。
    relations.sort_by(|a, b| match (a, b) {
        (CatalogItem::Relation(x), CatalogItem::Relation(y)) => x.name.cmp(&y.name),
        _ => std::cmp::Ordering::Equal,
    });
    items.append(&mut relations);

    items
}

/// 各类计数（从同一份 `items` 数出来，⛔ 不另算一遍）。
///
/// ⭐ REQ-136 · `excluded_relations` 只有普通模式有值（`None` ⇒ 本体模式，字段不出现）。
pub fn counts(mapping: Option<&R2rmlMapping>, items: &[CatalogItem]) -> CatalogCounts {
    let classes = items
        .iter()
        .filter(|i| matches!(i, CatalogItem::Class(_)))
        .count();
    let properties = items
        .iter()
        .filter(|i| matches!(i, CatalogItem::Property(_)))
        .count();
    let relations = items
        .iter()
        .filter(|i| matches!(i, CatalogItem::Relation(_)))
        .count();
    CatalogCounts {
        classes,
        properties,
        relations,
        excluded_relations: mapping.map(|m| m.excluded_edges().len()),
        skipped_classes: mapping.map(|m| m.skipped_classes().len()).unwrap_or(0),
    }
}

fn skipped_classes(mapping: Option<&R2rmlMapping>) -> Vec<SkippedClass> {
    mapping
        .map(|m| {
            m.skipped_classes()
                .iter()
                .map(|(table, reason)| SkippedClass {
                    table: table.clone(),
                    reason: reason.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 一页目录。`page` 从 1 起；越界返回**空 items** 而不是 404
/// （翻页翻过头是常见操作，不该是一次「错误」）。
pub fn page(svc: &QueryService, page: usize, page_size: usize) -> CatalogPage {
    let total_items = svc.catalog_items.len();
    let total_pages = total_items.div_ceil(page_size.max(1));
    let start = (page.saturating_sub(1)).saturating_mul(page_size);
    let items = if start >= total_items {
        Vec::new()
    } else {
        let end = (start + page_size).min(total_items);
        svc.catalog_items[start..end].to_vec()
    };
    CatalogPage {
        project: svc.project.clone(),
        version: svc.version,
        page,
        page_size,
        total_items,
        total_pages,
        has_more: page < total_pages,
        counts: counts(svc.mapping.as_ref(), &svc.catalog_items),
        skipped_classes: skipped_classes(svc.mapping.as_ref()),
        items,
    }
}

// ══════════════════════════════════════════════════════════════
// describe
// ══════════════════════════════════════════════════════════════

/// 按名字取详情：先类、后属性；都没有 ⇒ `UNKNOWN_NAME` + 相近项。
///
/// ⭐ REQ-136 · 两个模式各有一条**取详情**的路，但名字解析的口径相同：
/// `:name`（可省冒号）→ 类本地名 / 类 key；属性写 `:类__列` 或 `类.列`。
/// 普通模式从**映射**取（主键 / 类型 / 租户口径都在那儿）；
/// 本体模式从**快照**取（没有 schema ⇒ 不报主键，`display_name` 从快照来）。
pub fn describe(svc: &QueryService, raw: &str) -> Result<Description, ApiError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request(
            "describe 缺 name。",
            "带上要查的类或属性：`GET /api/describe?name=:customers`（属性如 `:customers__id`）。",
        ));
    }
    let local = name.strip_prefix(':').unwrap_or(name);

    let Some(mapping) = svc.mapping.as_ref() else {
        // ⭐ 本体服务模式：没有映射，名字从快照解析。
        return describe_from_snapshot(svc, name, local);
    };

    if let Some(c) = mapping
        .class_by_local(local)
        .or_else(|| mapping.class(local))
    {
        return Ok(Description::Class(class_detail(svc, c)));
    }
    if let Some(p) = find_property(mapping, local) {
        return Ok(Description::Property(property_detail(svc, p.0, &p.1)));
    }

    // ⭐ REQ-145 · **api 类**即便在映射里没有 TriplesMap，也要能 describe：
    // catalog 里已经把它列出来了（`access.kind: "api"`），这里 404 会让调用方以为
    // 那条目录是假的。走快照那条路（名字口径与本体模式相同）。
    if svc
        .access
        .as_ref()
        .is_some_and(|a| a.api_classes().keys().any(|k| k == local || local_model(k) == local))
    {
        return describe_from_snapshot(svc, name, local);
    }

    let mut suggestions = nearest(svc, local, 5);
    suggestions.sort();
    suggestions.dedup();
    Err(ApiError::unknown_name(name, suggestions))
}

fn class_detail(svc: &QueryService, c: &crate::domain::r2rml::MappedClass) -> ClassDetail {
    let tenant = match svc.policy.decide(&c.key) {
        crate::sparql::policy::TableDecision::Filter {
            column,
            reason,
            coverage,
            ..
        } => TenantInfo {
            mode: "filter".into(),
            column: Some(column),
            coverage: Some(coverage.as_str().to_string()),
            reason,
        },
        crate::sparql::policy::TableDecision::Public { reason } => TenantInfo {
            mode: "public".into(),
            column: None,
            coverage: Some("complete".into()),
            reason: Some(reason),
        },
        crate::sparql::policy::TableDecision::Unregistered => TenantInfo {
            mode: "unregistered".into(),
            column: None,
            coverage: None,
            reason: Some(
                "这张表在 query-policy.json 里没有登记 ⇒ 现在**查不了**（fail-closed）。\
                 请在 perTable 或 publicDimensions 里登记。"
                    .into(),
            ),
        },
    };

    let properties = c
        .properties
        .iter()
        .map(|p| property_detail(svc, &c.key, p))
        .collect();
    let relations = relations_of(svc, &c.key);
    ClassDetail {
        name: c.class_local.clone(),
        table: c.key.clone(),
        primary_key: c.pk.clone(),
        description: model_entity(&svc.model, &c.key).and_then(|e| e.description.clone()),
        // 普通模式不新增字段（AC-006：既有响应逐字节不变）。
        display_name: None,
        key: None,
        tenant,
        // ⭐ REQ-145 · api 类 / 库源类的数据源（`None` = 没加载映射 ⇒ 字段不出现）。
        access: access_of(svc, &c.key),
        properties,
        relations,
    }
}

/// ⭐ REQ-145 · 类的 `access`（`None` = 没加载 `--access-mapping`）。
fn access_of(svc: &QueryService, class_key: &str) -> Option<ClassAccess> {
    let a = svc.access.as_ref()?;
    Some(match a.entity(class_key) {
        Some(e) => ClassAccess::api(e),
        None => ClassAccess::db(),
    })
}

fn property_detail(
    svc: &QueryService,
    class_key: &str,
    p: &crate::domain::r2rml::MappedProperty,
) -> PropertyDetail {
    let mp = model_prop(&svc.model, class_key, &p.column);
    PropertyDetail {
        name: p.local.clone(),
        class: class_key.to_string(),
        column: p.column.clone(),
        datatype: p.xsd.iri().to_string(),
        enum_values: mp.and_then(|m| m.note.as_deref()).and_then(parse_allowed_values),
        nullable: mp.map(|m| m.nullable),
        sample: mp.and_then(|m| m.sample.clone()),
        note: mp.and_then(|m| m.note.clone()),
        display_name: None,
    }
}

// ── 本体服务模式的 describe（从快照取，⛔ 不碰映射）──────────────

/// 本体服务模式的 `describe`：名字按「类本地名 / 类 key / display_name / 属性谓词名」解析。
///
/// 与普通模式的**同一个**返回类型（[`Description`]），只是事实来源换成快照：
/// ⛔ 不报主键（没有 schema）、⛔ 不报租户口径（没有数据源）、`queryable` 恒 `false`。
fn describe_from_snapshot(svc: &QueryService, name: &str, local: &str) -> Result<Description, ApiError> {
    let idx = snapshot_index(&svc.model);

    // ① 类：本地名 → key → display_name（客户视图 TTL 里的 `:云资源` 也能直接问）
    if let Some(e) = idx
        .classes
        .iter()
        .find(|e| local_model(&e.key) == local || e.key == local || e.display_name == local)
    {
        return Ok(Description::Class(class_detail_snapshot(svc, &idx, e)));
    }

    // ② 属性：`类__列` 或 `类.列`（与普通模式同一套写法）
    if let Some((e, p)) = find_property_snapshot(&idx, local) {
        return Ok(Description::Property(property_detail_snapshot(e, p)));
    }

    let mut suggestions = nearest(svc, local, 5);
    suggestions.sort();
    suggestions.dedup();
    Err(ApiError::unknown_name(name, suggestions))
}

fn class_detail_snapshot(
    svc: &QueryService,
    idx: &SnapshotIndex<'_>,
    e: &ModelEntityView,
) -> ClassDetail {
    let mut props: Vec<&ModelPropertyView> = e.properties.iter().collect();
    props.sort_by(|a, b| a.column.cmp(&b.column).then_with(|| a.name.cmp(&b.name)));
    ClassDetail {
        name: local_model(&e.key),
        table: e.key.clone(),
        // 没有 schema ⇒ 没有主键。空串是**事实**（不是漏填）。
        primary_key: String::new(),
        description: e.description.clone(),
        display_name: Some(e.display_name.clone()),
        key: Some(e.key.clone()),
        tenant: TenantInfo {
            mode: "not_applicable".into(),
            column: None,
            coverage: None,
            reason: Some(
                "本体服务模式（`--ontology-only`）：本部署没有业务库，\
                 「按哪一列做租户过滤」不适用；catalog/describe 里的事实全部来自冻结快照。"
                    .into(),
            ),
        },
        // ⭐ REQ-145 · 本体服务模式也可能加载了映射（`--access-mapping` 与
        // `--ontology-only` 不互斥）⇒ 这里同样报 `access`（没加载就是 `None`）。
        access: access_of(svc, &e.key),
        properties: props
            .into_iter()
            .map(|p| property_detail_snapshot(e, p))
            .collect(),
        relations: relations_of_snapshot(idx, &e.key),
    }
}

fn property_detail_snapshot(e: &ModelEntityView, p: &ModelPropertyView) -> PropertyDetail {
    let column = effective_column(p);
    let label = if p.name.trim().is_empty() {
        column
    } else {
        p.name.trim()
    };
    // 与 TTL / R2RML 同一个推断函数（schema = None ⇒ 启发式，⛔ 不另写规则）。
    let (xsd, _basis, _source) =
        xsd_range(Some(&e.key), Some(column), None, label, p.note.as_deref());
    PropertyDetail {
        name: format!("{}__{}", local_model(&e.key), local_model(column)),
        class: e.key.clone(),
        column: column.to_string(),
        datatype: xsd.iri().to_string(),
        enum_values: p.note.as_deref().and_then(parse_allowed_values),
        nullable: Some(p.nullable),
        sample: p.sample.clone(),
        note: p.note.clone(),
        display_name: Some(p.name.clone()),
    }
}

/// 快照里与 `class_key` 有关的关系（出边 + 入边）—— 与 [`relations_of`] 的**同一个**形状。
fn relations_of_snapshot(idx: &SnapshotIndex<'_>, class_key: &str) -> Vec<RelationDetail> {
    let mut out: Vec<RelationDetail> = Vec::new();
    for (i, l) in idx.links.iter().enumerate() {
        if l.from_type != class_key && l.to_type != class_key {
            continue;
        }
        out.push(RelationDetail {
            name: idx.forward[i].clone(),
            inverse: idx.inverse[i].clone(),
            from: l.from_type.clone(),
            to: l.to_type.clone(),
            column: l.column.clone().unwrap_or_default(),
            cardinality: Some(l.cardinality.clone()),
            evidence_level: l
                .join_verified
                .map(|s| s.as_str().to_string())
                .unwrap_or_else(|| "unverified".to_string()),
            evidence: l.join_evidence.clone().unwrap_or_default(),
            // 没有数据源 ⇒ 一条都查不了（R4）。
            queryable: false,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 快照里找属性：`类__列` / `类.列` / 谓词本地名。
fn find_property_snapshot<'a>(
    idx: &SnapshotIndex<'a>,
    local: &str,
) -> Option<(&'a ModelEntityView, &'a ModelPropertyView)> {
    for e in &idx.classes {
        for p in &e.properties {
            let column = effective_column(p);
            if format!("{}__{}", local_model(&e.key), local_model(column)) == local {
                return Some((e, p));
            }
        }
    }
    if let Some((table, column)) = local.split_once('.') {
        let e = idx
            .classes
            .iter()
            .find(|e| e.key == table.trim() || local_model(&e.key) == table.trim())?;
        let p = e
            .properties
            .iter()
            .find(|p| effective_column(p).eq_ignore_ascii_case(column.trim()))?;
        return Some((e, p));
    }
    None
}

/// 与 `class_key` 有关的关系：出边（forward）+ 入边（inverse）。
///
/// `queryable` 恒真（[`build_items`] 只从映射里取）；`describe` 用它给「反向怎么查」。
fn relations_of(svc: &QueryService, class_key: &str) -> Vec<RelationDetail> {
    let mut out: Vec<RelationDetail> = Vec::new();
    let Some(mapping) = svc.mapping.as_ref() else {
        return out;
    };
    for c in mapping.classes() {
        for j in &c.joins {
            if j.child_table != class_key && j.parent_table != class_key {
                continue;
            }
            out.push(RelationDetail {
                name: j.predicate_local.clone(),
                inverse: j.inverse_local.clone(),
                from: j.child_table.clone(),
                to: j.parent_table.clone(),
                column: j.child.clone(),
                cardinality: model_cardinality(
                    &svc.model,
                    &j.child_table,
                    &j.parent_table,
                    &j.child,
                ),
                evidence_level: j.state.as_str().to_string(),
                evidence: j.evidence.clone(),
                queryable: true,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 在映射里找属性（按本地名；也接受 `类__列` 之外的写法 `类.列`）。
fn find_property<'a>(
    mapping: &'a R2rmlMapping,
    local: &str,
) -> Option<(&'a str, &'a crate::domain::r2rml::MappedProperty)> {
    // ① 直接按本地名（`:orders__amount`）
    for c in mapping.classes() {
        if let Some(p) = c.property(local) {
            return Some((c.key.as_str(), p));
        }
    }
    // ② 退一步：`表.列` 或 `表__列`（后者已被 ① 覆盖，这里只补点号写法）
    if let Some((table, column)) = local.split_once('.') {
        let c = mapping.class(table.trim())?;
        let p = c
            .properties
            .iter()
            .find(|p| p.column.eq_ignore_ascii_case(column.trim()))?;
        return Some((c.key.as_str(), p));
    }
    None
}

/// 相近名（拼错时的可行动提示）。
///
/// 只在「距离足够小」或「前缀相同」时才算相近 —— 否则会给出五个毫不相干的建议，
/// 那比没有建议更糟。
fn nearest(svc: &QueryService, local: &str, n: usize) -> Vec<String> {
    let mut candidates: Vec<(usize, String)> = Vec::new();
    let lower = local.to_ascii_lowercase();
    for item in &svc.catalog_items {
        let (name, kind) = match item {
            CatalogItem::Class(c) => (c.name.as_str(), "class"),
            CatalogItem::Property(p) => (p.name.as_str(), "property"),
            CatalogItem::Relation(r) => (r.name.as_str(), "relation"),
        };
        let d = levenshtein(&lower, &name.to_ascii_lowercase());
        let prefix = name.to_ascii_lowercase().starts_with(&lower)
            || name.to_ascii_lowercase().contains(&lower);
        if d <= 3 || prefix {
            candidates.push((d, format!("{name}（{kind}）")));
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    candidates.dedup_by(|a, b| a.1 == b.1);
    candidates
        .into_iter()
        .take(n)
        .map(|(_, name)| name)
        .collect()
}

/// 编辑距离（两行滚动数组，O(n·m)）；名字都不长，够用。
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ── 建模视图小工具 ──────────────────────────────────────────

fn model_entity<'a>(
    model: &'a ModelTrackView,
    class_key: &str,
) -> Option<&'a crate::domain::model_view::ModelEntityView> {
    model.entity_types.iter().find(|e| e.key == class_key)
}

fn model_prop<'a>(
    model: &'a ModelTrackView,
    class_key: &str,
    column: &str,
) -> Option<&'a crate::domain::model_view::ModelPropertyView> {
    model_entity(model, class_key).and_then(|e| {
        e.properties
            .iter()
            .find(|p| p.column.eq_ignore_ascii_case(column))
    })
}

fn enum_values_of(model: &ModelTrackView, class_key: &str, column: &str) -> Option<String> {
    model_prop(model, class_key, column)
        .and_then(|p| p.note.as_deref())
        .and_then(parse_allowed_values)
}

/// 关系基数（来自建模视图；映射里没有这一栏，⛔ 不推断）。
fn model_cardinality(
    model: &ModelTrackView,
    from: &str,
    to: &str,
    column: &str,
) -> Option<String> {
    model
        .link_types
        .iter()
        .find(|l| {
            l.from_type == from
                && l.to_type == to
                && l.column.as_deref().map(|c| c.eq_ignore_ascii_case(column)) == Some(true)
        })
        .map(|l| l.cardinality.clone())
}

// ══════════════════════════════════════════════════════════════
// 判据（纯函数：不连库）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_is_a_real_distance() {
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("customers", "customer"), 1);
    }

    /// 目录序列**稳定**：同一份映射两次构建逐字节相同（分页才有意义）。
    #[test]
    fn the_item_sequence_is_stable_and_sorted_by_kind() {
        let (_, mapping, model) = crate::query::test_fixture();
        let a = build_items(&model, Some(&mapping), None);
        let b = build_items(&model, Some(&mapping), None);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );

        // 类在前、属性在中、关系在后，各自按名字升序。
        let kinds: Vec<&str> = a
            .iter()
            .map(|i| match i {
                CatalogItem::Class(_) => "class",
                CatalogItem::Property(_) => "property",
                CatalogItem::Relation(_) => "relation",
            })
            .collect();
        let first_prop = kinds.iter().position(|k| *k == "property").unwrap();
        let first_rel = kinds.iter().position(|k| *k == "relation").unwrap();
        assert!(kinds[..first_prop].iter().all(|k| *k == "class"));
        assert!(kinds[first_prop..first_rel].iter().all(|k| *k == "property"));
        assert!(kinds[first_rel..].iter().all(|k| *k == "relation"));

        // 各自内部按名字升序（分页切片要稳定）。
        let names = |kind: &str| -> Vec<String> {
            a.iter()
                .filter_map(|i| match (kind, i) {
                    ("class", CatalogItem::Class(c)) => Some(c.name.clone()),
                    ("property", CatalogItem::Property(p)) => Some(p.name.clone()),
                    ("relation", CatalogItem::Relation(r)) => Some(r.name.clone()),
                    _ => None,
                })
                .collect()
        };
        for kind in ["class", "property", "relation"] {
            let mut sorted = names(kind);
            sorted.sort();
            assert_eq!(names(kind), sorted, "{kind} 段必须按名字升序");
        }
    }

    // ══════════════════════════════════════════════════════════
    // ⭐ REQ-136 · 本体服务模式 vs 普通模式（同一个构建函数，只在过滤处分叉）
    // ══════════════════════════════════════════════════════════

    /// 一个**文档类**本体：两张「表」没有任何实现列，关系只有语义。
    ///
    /// 这正是 ops360 的形状（20 类 / 10 边 / 全 `join_verified = None`）。
    fn documentary_fixture() -> ModelTrackView {
        use crate::domain::model_view::{
            ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
        };

        let prop = |n: &str, c: &str| ModelPropertyView {
            name: n.into(),
            column: c.into(),
            nullable: true,
            note: None,
            sample: None,
        };
        let entity = |k: &str, d: &str, props: Vec<ModelPropertyView>| ModelEntityView {
            key: k.into(),
            display_name: d.into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: props,
            evidence: vec![],
        };
        ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                entity(
                    "doc_01_aaaa",
                    "云资源",
                    vec![prop("资源总数", "p_01"), prop("异常数", "p_02")],
                ),
                entity("doc_02_bbbb", "告警", vec![prop("告警总数", "p_01")]),
            ],
            link_types: vec![ModelLinkView {
                key: "rel_19".into(),
                from_type: "doc_02_bbbb".into(),
                to_type: "doc_01_aaaa".into(),
                cardinality: "N:1".into(),
                semantic: "关联".into(),
                evidence: vec![],
                confidence: None,
                // ⚠️ 文档类本体的边**没有 join 证据**（也不可能有：没有列）。
                join_verified: None,
                join_evidence: None,
                column: None,
            }],
            action_types: vec![],
            functions: vec![],
        }
    }

    /// ⭐ AC-002 的核心：本体模式**必须列出没有 join 列的关系**。
    ///
    /// 变异：把本体模式的分支改回「走映射对账」（`Some(mapping)` 那条路）⇒
    /// 这份快照在没有 schema 时连映射都建不出来；就算用合成 schema 建出来，`relations` 也会是 0。
    #[test]
    fn ontology_mode_lists_relations_that_have_no_join_column() {
        let model = documentary_fixture();
        let items = build_items(&model, None, None);
        let counts = counts(None, &items);
        assert_eq!(counts.classes, 2, "{items:#?}");
        assert_eq!(counts.relations, 1, "关系不许被「对账不上」吃掉：{items:#?}");
        assert_eq!(counts.properties, 3, "属性 = 快照里的列，一条不多一条不少");
        assert_eq!(counts.excluded_relations, None, "本体模式没有「被排除的边」这个类别");

        let rel = items
            .iter()
            .find_map(|i| match i {
                CatalogItem::Relation(r) => Some(r),
                _ => None,
            })
            .expect("必须有一条关系");
        assert_eq!(rel.name, "doc_02_bbbb__doc_01_aaaa");
        assert_eq!(rel.inverse, "doc_01_aaaa__doc_02_bbbb");
        assert_eq!(rel.from, "doc_02_bbbb");
        assert_eq!(rel.to, "doc_01_aaaa");
        assert_eq!(rel.column, "", "⛔ 不猜实现列");
        assert_eq!(rel.evidence_level, "unverified", "这一版没记录 join 验证状态");
        assert_eq!(rel.evidence, "");
        assert_eq!(rel.cardinality.as_deref(), Some("N:1"));

        // 序列化后**没有** `excluded_relations` 这个键（⛔ 不出现 =10 那种形状）。
        let page = serde_json::to_value(&counts).unwrap();
        assert!(
            page.get("excluded_relations").is_none(),
            "本体模式的 counts 不该有 excluded_relations：{page}"
        );
    }

    /// 本体模式**不造 schema、不补主键列**：属性数 = 快照属性数，`primary_key` 是空串。
    ///
    /// 变异：给本体模式也补一条 `__id` 属性（或填 `primary_key = "id"`）⇒ 本测试红。
    #[test]
    fn ontology_mode_does_not_fabricate_a_primary_key_column() {
        let model = documentary_fixture();
        let items = build_items(&model, None, None);
        let classes: Vec<&ClassItem> = items
            .iter()
            .filter_map(|i| match i {
                CatalogItem::Class(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(classes.len(), 2);
        for c in &classes {
            assert_eq!(c.primary_key, "", "没有 schema ⇒ 没有主键，⛔ 不拿 id 顶替");
            assert!(
                c.display_name.as_deref().is_some_and(|d| !d.is_empty()),
                "本体模式必须给 display_name：{c:?}"
            );
        }
        assert!(
            !items.iter().any(|i| matches!(i, CatalogItem::Property(p) if p.column == "id")),
            "⛔ 不许凭空造 id 列（那是「自动造 schema」）：{items:#?}"
        );
        // 类上的属性数就是快照的属性数（2 / 1），不是「+1 主键」。
        let by_name: std::collections::BTreeMap<&str, usize> = classes
            .iter()
            .map(|c| (c.name.as_str(), c.property_count))
            .collect();
        assert_eq!(by_name["doc_01_aaaa"], 2);
        assert_eq!(by_name["doc_02_bbbb"], 1);
    }

    /// ⭐ AC-006 的另一半：普通模式的条目**不多出**本体模式的新字段。
    ///
    /// 变异：把 `display_name` 改成恒 `Some`（或去掉 `skip_serializing_if`）⇒ 本测试红
    /// （普通模式的响应字节就变了）。
    #[test]
    fn query_mode_items_have_no_ontology_only_fields() {
        let (_, mapping, model) = crate::query::test_fixture();
        let items = build_items(&model, Some(&mapping), None);
        for item in &items {
            let v = serde_json::to_value(item).unwrap();
            assert!(
                v.get("display_name").is_none(),
                "普通模式不许新增 display_name：{v}"
            );
            assert!(v.get("key").is_none(), "普通模式不许新增 key：{v}");
        }
        let counts = counts(Some(&mapping), &items);
        assert_eq!(counts.excluded_relations, Some(0));
        assert_eq!(counts.classes, 2);
        assert_eq!(counts.relations, 1);
    }
}
