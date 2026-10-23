//! REQ-123 · **catalog / describe** —— Agent 在写 SPARQL 之前「看有什么可用」的两个出口。
//!
//! # 数据从哪来（⛔ 不猜、不编）
//!
//! | 事实 | 来源 |
//! | --- | --- |
//! | 类 / 属性 / 关系的**可用名字** | [`crate::domain::r2rml::R2rmlMapping`]（与 `sparql query` **同一个** `build_mapping`） |
//! | 关系**证据等级** | 映射里的 `MappedJoin::state`（`true` / `inherit` / `false`）+ `evidence` 原文 |
//! | 枚举值 / 可空 / 样例 | 建模视图的 `ModelPropertyView.note / nullable / sample`；枚举值只认注释里**明写**的那几种形态（[`crate::domain::ttl::parse_allowed_values`]，⛔ 不编） |
//! | 租户口径 | `query-policy.json`（`QueryPolicy::decide`） |
//!
//! # 为什么只列**映射里的**关系
//!
//! 目录的用途是「照着写一条能跑的 SPARQL」。映射里没有的边（证据 `inherit` 且
//! 服务未开 `--include-inherit`、或 `false`）**写进查询会被 planner 拒绝** ——
//! 把它们列进目录等于教调用方写一条注定失败的查询。
//! 所以：`catalog` 只列**可查**的边，并在 `counts` 里给出 `excluded_relations` 的条数
//! （来自映射的审计信息），让「少了哪些」是一个**可读到的数字**而不是沉默。
//!
//! # 分页
//!
//! 三类条目（class / property / relation）拍平成**一个**稳定序列再分页：
//! 类（按名字）→ 属性（按名字）→ 关系（按名字）。`total_items` / `total_pages` /
//! `has_more` 都从同一个序列算出来，⛔ 不让调用方自己猜「还有没有」。

use serde::Serialize;

use crate::domain::model_view::ModelTrackView;
use crate::domain::r2rml::R2rmlMapping;
use crate::domain::ttl::parse_allowed_values;

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
    pub excluded_relations: usize,
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
    /// 租户口径（来自声明，不是代码推断）。
    pub tenant: TenantInfo,
    pub properties: Vec<PropertyDetail>,
    pub relations: Vec<RelationDetail>,
}

/// 一份租户决定的人读形状。
#[derive(Debug, Clone, Serialize)]
pub struct TenantInfo {
    /// `filter` / `public` / `unregistered`。
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

/// 拍平成一个**稳定**序列（类 → 属性 → 关系，各自按名字升序）。
pub fn build_items(mapping: &R2rmlMapping, model: &ModelTrackView) -> Vec<CatalogItem> {
    let mut items: Vec<CatalogItem> = Vec::new();

    let mut classes: Vec<&crate::domain::r2rml::MappedClass> = mapping.classes().iter().collect();
    classes.sort_by(|a, b| a.class_local.cmp(&b.class_local));
    for c in &classes {
        items.push(CatalogItem::Class(ClassItem {
            name: c.class_local.clone(),
            table: c.key.clone(),
            primary_key: c.pk.clone(),
            property_count: c.properties.len(),
            relation_count: c.joins.len(),
            description: model_entity(model, &c.key).and_then(|e| e.description.clone()),
        }));
    }

    let mut props: Vec<(&str, &crate::domain::r2rml::MappedProperty)> = Vec::new();
    for c in mapping.classes() {
        for p in &c.properties {
            props.push((c.key.as_str(), p));
        }
    }
    props.sort_by(|a, b| a.1.local.cmp(&b.1.local));
    for (class_key, p) in props {
        items.push(CatalogItem::Property(PropertyItem {
            name: p.local.clone(),
            class: class_key.to_string(),
            column: p.column.clone(),
            datatype: p.xsd.iri().to_string(),
            enum_values: enum_values_of(model, class_key, &p.column),
        }));
    }

    let mut joins: Vec<(&str, &crate::domain::r2rml::MappedJoin)> = Vec::new();
    for c in mapping.classes() {
        for j in &c.joins {
            joins.push((c.key.as_str(), j));
        }
    }
    joins.sort_by(|a, b| a.1.predicate_local.cmp(&b.1.predicate_local));
    for (_class_key, j) in joins {
        items.push(CatalogItem::Relation(RelationItem {
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

    items
}

/// 各类计数（从同一份 `items` 数出来，⛔ 不另算一遍）。
pub fn counts(mapping: &R2rmlMapping, items: &[CatalogItem]) -> CatalogCounts {
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
        excluded_relations: mapping.excluded_edges().len(),
        skipped_classes: mapping.skipped_classes().len(),
    }
}

fn skipped_classes(mapping: &R2rmlMapping) -> Vec<SkippedClass> {
    mapping
        .skipped_classes()
        .iter()
        .map(|(table, reason)| SkippedClass {
            table: table.clone(),
            reason: reason.clone(),
        })
        .collect()
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
        counts: counts(&svc.mapping, &svc.catalog_items),
        skipped_classes: skipped_classes(&svc.mapping),
        items,
    }
}

// ══════════════════════════════════════════════════════════════
// describe
// ══════════════════════════════════════════════════════════════

/// 按名字取详情：先类、后属性；都没有 ⇒ `UNKNOWN_NAME` + 相近项。
pub fn describe(svc: &QueryService, raw: &str) -> Result<Description, ApiError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request(
            "describe 缺 name。",
            "带上要查的类或属性：`GET /api/describe?name=:customers`（属性如 `:customers__id`）。",
        ));
    }
    let local = name.strip_prefix(':').unwrap_or(name);

    if let Some(c) = svc
        .mapping
        .class_by_local(local)
        .or_else(|| svc.mapping.class(local))
    {
        return Ok(Description::Class(class_detail(svc, c)));
    }
    if let Some(p) = find_property(&svc.mapping, local) {
        return Ok(Description::Property(property_detail(svc, p.0, &p.1)));
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
        tenant,
        properties,
        relations,
    }
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
    }
}

/// 与 `class_key` 有关的关系：出边（forward）+ 入边（inverse）。
///
/// `queryable` 恒真（[`build_items`] 只从映射里取）；`describe` 用它给「反向怎么查」。
fn relations_of(svc: &QueryService, class_key: &str) -> Vec<RelationDetail> {
    let mut out: Vec<RelationDetail> = Vec::new();
    for c in svc.mapping.classes() {
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
        let a = build_items(&mapping, &model);
        let b = build_items(&mapping, &model);
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
    }
}
