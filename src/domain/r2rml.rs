//! REQ-121 · **R2RML 映射生成** —— 纯函数，零 I/O、零新依赖（Turtle 手写）。
//!
//! # 一句话
//!
//! 把「建模视图 + join 证据 + 真实 schema」写成一份 R2RML Turtle：
//! 本体里的每个类 → 一张表，每个数据类型属性 → 一列，每条**验过的**关系 → 一个 join。
//!
//! 数字员工链路是 `TTL → Agent 写 SPARQL → 服务端按 R2RML 查库 → Agent 解释`。
//! 本模块产出的就是中间那份「按 R2RML 查库」的映射 —— TTL 与关系库之间**唯一的桥**。
//!
//! # 证据等级决定生成什么
//!
//! | `ontok:joinVerified` | 本模块的处理 |
//! | --- | --- |
//! | `"true"` | **生成** `rr:predicateObjectMap` + `rr:joinCondition`，并标 `ontok:joinState "true"` |
//! | `"inherit"` | **默认不生成**；`include_inherit = true` 时生成，POM 上标 `ontok:joinState "inherit"` + `ontok:joinEvidence` |
//! | `"false"` | ⛔ **任何模式下都不生成 join**；只写进文件尾的注释（含「为什么没验」） |
//! | `None` | 同 `false` 的待遇（不生成），但理由如实写「这一版没有连接验证信息」 |
//!
//! 「把犹豫藏起来，数字员工就会把犹豫当结论」—— 所以 `false` 不但不生成，
//! 还必须在**人能看见的地方**留下候选与原因（文件尾注释，AC-006）。
//!
//! # 唯一的实现，唯一的 IRI（纪律 H）
//!
//! R2RML 里的 `rr:class` / `rr:predicate` / `rr:template` 必须与 REQ-119/120 的 TTL
//! **逐字**对得上，否则 SPARQL 查出来的实例与本体里的实例是两套 IRI ——
//! 而且**不会报错**。所以本模块不重算任何一种命名：
//!
//! | 名字 | 唯一来源 |
//! | --- | --- |
//! | 本体 IRI / 实例 IRI 模板 | [`TtlOptions::ontology_iri`] / [`TtlOptions::instance_iri_template`] |
//! | 类本地名 | [`local_model`]（与建模视图同一个） |
//! | 属性本地名（前向 / 反向） | [`crate::domain::ttl::model_link_locals`]（与建模视图同一个） |
//! | 数据类型 | [`xsd_range`]（schema 优先 → 列名 → 中文名 → `xsd:string` 兜底） |
//!
//! ⛔ 这里出现第二份命名规则，就是「桥接错位」的根。
//!
//! # 与 `schema.json` 对账（AC-008，防静默错误）
//!
//! 表名 / 列名 / 主键 / 唯一列全部来自 `--schema`（`ontok-introspect` 产物），**不猜**：
//!
//! - `rr:tableName` 必须是 schema 里存在的表；
//! - `rr:columnName` / `rr:child` / `rr:parent` 必须是该表里存在的列；
//! - 主键必须是**单列**主键；复合主键 / 无主键 ⇒ **不生成**该类的 TriplesMap，
//!   逐条列入「未生成清单」（AC-011）；
//! - 父列必须真的是父表的**主键或唯一列** —— join 到一个不唯一的列，
//!   一条 SPARQL 会悄悄返回多份结果。
//!
//! 任一对账失败 ⇒ [`R2rmlError`]（**点名到 `表.列`**），调用方必须 fail-closed
//! （不写半份映射）。「跳过并继续」在这里是错的：对账失败说明输入之间已经不一致，
//! 继续生成只会得到一份看着正常的错映射。
//!
//! # 父列从哪来（`rr:parent` 可能是唯一列，不一定主键）
//!
//! 快照的 `LinkType` 只存**子列**（`evidence[0].column`）；父列在 REQ-120 的取证记录里：
//!
//! ```text
//! value-overlap 5/5 (100.00%) -> admin_users.id via primary_key (plural)
//! inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)
//! ```
//!
//! 本模块**从 `ontok:joinEvidence` 里解析父表.父列**，而不是假定它是主键 ——
//! `llm_model.model_id` / `service_apps.app_id` 都是单列唯一列，不是主键。
//! 解析不出 / 与关系终点不一致 / 列不存在 ⇒ 点名报错（不猜一个「像主键的列」）。
//!
//! # 确定性（AC-010）
//!
//! 类按 `key` 排序、属性按（列名, 业务名）排序、关系按（key, from, to）排序、
//! 文件尾的清单同样排序；⛔ 不写时间、不用哈希序。同一 project+version
//! 两次生成**逐字节相同**。
//!
//! # 本模块**不**做什么
//!
//! - 不做 SPARQL 引擎（REQ-122）、不执行任何 SQL；
//! - 不做多表 union / 视图 / 复杂表达式（本轮只做 `rr:tableName` + 列 + join）；
//! - **不映射反向属性**：一条 R2RML join 只有一个方向（`rr:child` → `rr:parent`）；
//!   反向查询由引擎在前向映射上反解。这是本轮已登记的已知限制（文件头也写一遍）。
//!
//! # 实现注记：一处与本模块无关的已知边界
//!
//! 属性谓词的本地名是 `{类}__{local_model(列)}`，与建模视图逐字相同。若同一个类里
//! 两列的列名被 `local_model` 压成同一个本地名（`a-b` 与 `a_b`），TTL 与 R2RML
//! 会**一样**撞成同一个属性 IRI —— 本版数据没有这种情况（1024 个属性零碰撞），
//! 本轮不改命名口径（改它等于改 REQ-119 已验收的 IRI）。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelTrackView};
use crate::domain::snapshot::JoinState;
use crate::domain::ttl::{
    effective_column, escape, local_model, model_link_locals, tenancy_report, xsd_range, TtlOptions,
    XsdType,
};

// ══════════════════════════════════════════════════════════════
// 产物
// ══════════════════════════════════════════════════════════════

/// 生成结果：**文件字节** + 结构统计（统计只给人看，不改变字节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct R2rmlDocument {
    doc: String,
    stats: R2rmlStats,
}

impl R2rmlDocument {
    /// 完整文件。**写出去的就是它**（stdout 与 `--out` 是同一份字节）。
    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn stats(&self) -> &R2rmlStats {
        &self.stats
    }

    pub fn len(&self) -> usize {
        self.doc.len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
    }

    pub fn into_doc(self) -> String {
        self.doc
    }
}

/// 结构统计（CLI stderr 回执与验收报告用）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct R2rmlStats {
    /// 生成的 `rr:TriplesMap` 数（= 有单列主键的类数）。
    pub triples_maps: usize,
    /// 数据类型属性的 `rr:predicateObjectMap` 数（含补出来的主键列）。
    pub datatype_maps: usize,
    /// join 的 `rr:predicateObjectMap` 数。
    pub join_maps: usize,
    /// `true` 边数（**全部**生成）。
    pub verified_edges: usize,
    /// `inherit` 边数（不论是否生成）。
    pub inherited_edges: usize,
    /// 其中**真的生成**了 join 的 inherit 边数（`--include-inherit` 下 = inherited_edges）。
    pub inherited_emitted: usize,
    /// `false` 边数（永不生成）。
    pub naming_only_edges: usize,
    /// 没有 `joinVerified` 的边数（同样不生成）。
    pub unverified_edges: usize,
    /// 未生成 TriplesMap 的类：`(类 key, 原因)`，已排序。
    pub skipped_classes: Vec<(String, String)>,
    /// 被排除的边：`(关系 key, 原因)`，已排序。
    pub excluded_edges: Vec<(String, String)>,
}

impl R2rmlStats {
    /// `rr:joinCondition` 数 —— 每条 join 恰好一个条件，所以与 [`Self::join_maps`] 相等。
    ///
    /// 单独留一个方法是为了让验收判据能**按名字**对账（AC-004 判的是 joinCondition 数），
    /// 而不是靠「读者知道它等于 join_maps」。
    pub fn join_conditions(&self) -> usize {
        self.join_maps
    }

    /// 一行人类可读摘要（CLI stderr）。
    pub fn summary(&self) -> String {
        format!(
            "{} 个 TriplesMap / {} 个谓词-宾语映射（数据类型 {} · join {}）· joinCondition {}",
            self.triples_maps,
            self.datatype_maps + self.join_maps,
            self.datatype_maps,
            self.join_maps,
            self.join_conditions()
        )
    }
}

/// 对账失败。**点名**到 `表.列`，且不写任何文件（fail-closed）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct R2rmlError {
    failures: Vec<String>,
}

impl R2rmlError {
    fn of(failures: BTreeSet<String>) -> Self {
        Self { failures: failures.into_iter().collect() }
    }

    fn one(message: String) -> Self {
        Self { failures: vec![message] }
    }

    /// 逐条失败原因（已排序、已去重）。
    pub fn failures(&self) -> &[String] {
        &self.failures
    }

    pub fn len(&self) -> usize {
        self.failures.len()
    }

    pub fn is_empty(&self) -> bool {
        self.failures.is_empty()
    }
}

impl fmt::Display for R2rmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "R2RML 与 schema 对账失败（{} 处）—— ⛔ 未写出任何文件（fail-closed）：",
            self.failures.len()
        )?;
        for x in &self.failures {
            writeln!(f, "  - {x}")?;
        }
        Ok(())
    }
}

impl std::error::Error for R2rmlError {}

// ══════════════════════════════════════════════════════════════
// 计划（先算清，再渲染）
// ══════════════════════════════════════════════════════════════

/// 一个类的映射计划。
struct MapPlan {
    /// 类 key（= 表名 = `ontok:key`）。
    key: String,
    /// 类 IRI 本地名（与 TTL 的 `:<local>` 同一个）。
    class_local: String,
    /// 单列主键列名。
    pk: String,
    /// 租户列（已排序；空 = 该类没有租户标注）。
    tenant_columns: Vec<String>,
    /// 数据类型属性：按列名排序。
    datatypes: Vec<DatatypePlan>,
    /// join：按谓词本地名排序。
    joins: Vec<JoinPlan>,
}

struct DatatypePlan {
    column: String,
    xsd: XsdType,
}

struct JoinPlan {
    /// 谓词 IRI 的本地名（与 TTL 的 `:<local>` 同一个）。
    predicate_local: String,
    /// 子列（快照 `evidence[0].column`）。
    child: String,
    /// 父 TriplesMap 的 key。
    parent_key: String,
    /// 父列（从 `joinEvidence` 解析，可能是唯一列）。
    parent: String,
    state: JoinState,
    /// 证据原文（`ontok:joinEvidence`）。
    evidence: String,
}

/// 生成 R2RML 映射。
///
/// `opts.schema` **必填**（主键 / 唯一列 / 列存在性只能来自真实 schema）。
/// `include_inherit` = `false`（默认）时 `inherit` 边不生成 join，只留在文件尾清单里。
pub fn generate(
    view: &ModelTrackView,
    opts: &TtlOptions<'_>,
    include_inherit: bool,
) -> Result<R2rmlDocument, R2rmlError> {
    let Some(schema) = opts.schema else {
        return Err(R2rmlError::one(
            "缺少 --schema：主键 / 复合主键 / 列存在性都只能来自真实 schema（⛔ 不猜）".to_string(),
        ));
    };

    let mut failures: BTreeSet<String> = BTreeSet::new();
    let tenancy = tenancy_report(view, opts.tenancy);
    let mut skipped_classes: Vec<(String, String)> = Vec::new();

    // ── ① 类：单列主键的才有 TriplesMap ─────────────────────────
    let mut entities: Vec<&ModelEntityView> = view.entity_types.iter().collect();
    entities.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.display_name.cmp(&b.display_name)));

    let mut maps: Vec<MapPlan> = Vec::new();
    for e in &entities {
        if !schema.has_table(&e.key) {
            failures.insert(format!(
                "表 {} 不在 schema.json 里 —— 类 {} 的 rr:tableName 无处可落",
                e.key, e.key
            ));
            continue;
        }
        let Some(pk) = schema.primary_key(&e.key) else {
            let composite = schema.composite_primary_key(&e.key);
            let reason = if composite.is_empty() {
                "没有单列主键（schema 里 primary_key 为空）".to_string()
            } else {
                format!("复合主键（{}）", composite.join("、"))
            };
            skipped_classes.push((e.key.clone(), reason));
            continue;
        };
        let pk = pk.to_string();

        // 属性 → 列 + xsd（与建模视图同一套：先排序，再逐条查 schema）
        let mut props: Vec<&crate::domain::model_view::ModelPropertyView> =
            e.properties.iter().collect();
        props.sort_by(|a, b| a.column.cmp(&b.column).then_with(|| a.name.cmp(&b.name)));
        let mut datatypes: Vec<DatatypePlan> = Vec::new();
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for p in props {
            let column = effective_column(p);
            if column.is_empty() {
                failures.insert(format!("表 {} 有一个属性连列名都没有（⛔ 不猜）", e.key));
                continue;
            }
            if !schema.has_column(&e.key, column) {
                failures.insert(format!(
                    "表 {} 缺列 {}（属性「{}」的 rr:columnName 无处可落）",
                    e.key,
                    column,
                    if p.name.trim().is_empty() { column } else { p.name.trim() }
                ));
                continue;
            }
            // 同一列只映射一次：谓词 IRI 就是 `:类__列`，两条 POM 会写成同一个谓词。
            if !covered.insert(column.to_string()) {
                continue;
            }
            let label = if p.name.trim().is_empty() { column } else { p.name.trim() };
            let (xsd, _basis, _source) = xsd_range(
                Some(&e.key),
                Some(column),
                opts.schema,
                label,
                p.note.as_deref(),
            );
            datatypes.push(DatatypePlan { column: column.to_string(), xsd });
        }
        // B-2 的同一条口径：主教列通常被抽取侧从 properties 里剥掉了
        // （`licenses` 没有 `id`）。没有它，`{id}` 模板就指不到任何列。
        if !covered.contains(&pk) {
            let (xsd, _basis, _source) =
                xsd_range(Some(&e.key), Some(&pk), opts.schema, &pk, None);
            datatypes.push(DatatypePlan { column: pk.clone(), xsd });
        }
        datatypes.sort_by(|a, b| a.column.cmp(&b.column));

        let mut tenant_columns: Vec<String> = if tenancy.annotated() {
            tenancy.hits.get(&e.key).map(|c| c.iter().cloned().collect()).unwrap_or_default()
        } else {
            Vec::new()
        };
        tenant_columns.sort();
        tenant_columns.dedup();

        maps.push(MapPlan {
            key: e.key.clone(),
            class_local: local_model(&e.key),
            pk,
            tenant_columns,
            datatypes,
            joins: Vec::new(),
        });
    }

    // ── ② 关系：三态决定生成什么 ───────────────────────────────
    //
    // 排序口径与 TTL 逐字相同（`key` / `from` / `to`）—— 否则 `model_link_locals`
    // 分配出来的属性 IRI 会与 TTL 不一致。
    let mut links: Vec<&ModelLinkView> = view.link_types.iter().collect();
    links.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.from_type.cmp(&b.from_type))
            .then_with(|| a.to_type.cmp(&b.to_type))
    });
    let (predicate_locals, _inverse_locals) = model_link_locals(&links);

    let map_index: BTreeMap<String, usize> =
        maps.iter().enumerate().map(|(i, m)| (m.key.clone(), i)).collect();

    let mut verified_edges = 0usize;
    let mut inherited_edges = 0usize;
    let mut inherited_emitted = 0usize;
    let mut naming_only_edges = 0usize;
    let mut unverified_edges = 0usize;
    let mut excluded: Vec<(String, String)> = Vec::new();

    for (i, l) in links.iter().enumerate() {
        let state = l.join_verified;
        let emit = match state {
            Some(JoinState::Verified) => {
                verified_edges += 1;
                true
            }
            Some(JoinState::Inherited) => {
                inherited_edges += 1;
                if include_inherit {
                    inherited_emitted += 1;
                    true
                } else {
                    excluded.push((edge_label(l), inherit_reason(l)));
                    false
                }
            }
            Some(JoinState::NamingOnly) => {
                naming_only_edges += 1;
                excluded.push((edge_label(l), naming_reason(l)));
                false
            }
            None => {
                unverified_edges += 1;
                excluded.push((
                    edge_label(l),
                    "这一版没有 ontok:joinVerified（REQ-120 之前的快照 / 人工修订）—— 没说过，不替它说".to_string(),
                ));
                false
            }
        };
        if !emit {
            continue;
        }

        let Some(&child_idx) = map_index.get(&l.from_type) else {
            failures.insert(format!(
                "关系 {}（{} → {}）标了 joinVerified \"{}\"，但子类 {} 没有 TriplesMap（见表尾未生成清单）",
                l.key,
                l.from_type,
                l.to_type,
                state.map(JoinState::as_str).unwrap_or("?"),
                l.from_type
            ));
            continue;
        };
        if !map_index.contains_key(&l.to_type) {
            failures.insert(format!(
                "关系 {}（{} → {}）标了 joinVerified \"{}\"，但父类 {} 没有 TriplesMap（rr:parentTriplesMap 无处可指）",
                l.key,
                l.from_type,
                l.to_type,
                state.map(JoinState::as_str).unwrap_or("?"),
                l.to_type
            ));
            continue;
        }
        let Some(child) = l.column.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
            failures.insert(format!(
                "关系 {}（{} → {}）标了 joinVerified \"{}\" 但快照没有实现列（evidence[0].column）—— \
                 无法确定 rr:child",
                l.key,
                l.from_type,
                l.to_type,
                state.map(JoinState::as_str).unwrap_or("?"),
            ));
            continue;
        };
        if !schema.has_column(&l.from_type, child) {
            failures.insert(format!(
                "表 {} 缺子列 {}（关系 {} 的 rr:child）",
                l.from_type, child, l.key
            ));
            continue;
        }
        let evidence = l.join_evidence.as_deref().unwrap_or("").trim().to_string();
        let Some((parent_table, parent_column)) = parse_parent_target(&evidence) else {
            failures.insert(format!(
                "关系 {}（{} → {}）的父列无法从 ontok:joinEvidence 解析：{:?} —— \
                 rr:parent 只能是证据里写明的那个列（⛔ 不拿主键顶替）",
                l.key, l.from_type, l.to_type, evidence
            ));
            continue;
        };
        if parent_table != l.to_type {
            failures.insert(format!(
                "关系 {} 的证据指向 {}，但关系终点是 {} —— rr:joinCondition 会连错表",
                l.key, parent_table, l.to_type
            ));
            continue;
        }
        if !schema.has_column(&l.to_type, &parent_column) {
            failures.insert(format!(
                "表 {} 缺父列 {}（关系 {} 的 rr:parent）",
                l.to_type, parent_column, l.key
            ));
            continue;
        }
        if !schema.is_unique_column(&l.to_type, &parent_column) {
            failures.insert(format!(
                "表 {}.{} 不是主键也不是唯一列（关系 {} 的 rr:parent）—— \
                 join 到一个不唯一的列，一条 SPARQL 会悄悄返回多份结果",
                l.to_type, parent_column, l.key
            ));
            continue;
        }
        if evidence.is_empty() {
            failures.insert(format!(
                "关系 {}（{} → {}）标了 joinVerified \"{}\" 但没有 ontok:joinEvidence —— \
                 文件尾无法写明「凭什么」",
                l.key,
                l.from_type,
                l.to_type,
                state.map(JoinState::as_str).unwrap_or("?"),
            ));
            continue;
        }

        maps[child_idx].joins.push(JoinPlan {
            predicate_local: predicate_locals[i].clone(),
            child: child.to_string(),
            parent_key: l.to_type.clone(),
            parent: parent_column,
            state: state.expect("emit 只发生在 Some(..) 上"),
            evidence,
        });
    }

    // join 按谓词本地名排序（确定性；同一张 TriplesMap 内不依赖输入顺序）。
    for m in &mut maps {
        m.joins.sort_by(|a, b| {
            a.predicate_local.cmp(&b.predicate_local).then_with(|| a.parent.cmp(&b.parent))
        });
    }

    if !failures.is_empty() {
        return Err(R2rmlError::of(failures));
    }

    skipped_classes.sort();
    excluded.sort();
    let stats = R2rmlStats {
        triples_maps: maps.len(),
        datatype_maps: maps.iter().map(|m| m.datatypes.len()).sum(),
        join_maps: maps.iter().map(|m| m.joins.len()).sum(),
        verified_edges,
        inherited_edges,
        inherited_emitted,
        naming_only_edges,
        unverified_edges,
        skipped_classes: skipped_classes.clone(),
        excluded_edges: excluded.clone(),
    };

    let doc = render(view, opts, &maps, &skipped_classes, &excluded, &stats, include_inherit);
    Ok(R2rmlDocument { doc, stats })
}

/// 关系的人工标签：`from → to（child from.col）`。
fn edge_label(l: &ModelLinkView) -> String {
    let child = l.column.as_deref().unwrap_or("?");
    format!("{} → {}（child {}.{}）", l.from_type, l.to_type, l.from_type, child)
}

/// `false` 边的「为什么没验」。
fn naming_reason(l: &ModelLinkView) -> String {
    match l.join_evidence.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => e.to_string(),
        None => "仅列名推断（没有数据证据）".to_string(),
    }
}

/// 默认不生成的 `inherit` 边：说明它**不是**被当成 `true` 藏掉了。
fn inherit_reason(l: &ModelLinkView) -> String {
    let why = l.join_evidence.as_deref().unwrap_or("").trim();
    if why.is_empty() {
        "inherit（同名同义列继承，本表未实测）：默认不生成，--include-inherit 才生成".to_string()
    } else {
        format!("inherit（本表未实测）：{why} —— 默认不生成，--include-inherit 才生成")
    }
}

/// 从 REQ-120 的证据原文里解析 `-> <表>.<列> via …`。
///
/// 只认**最后**一个 `->`（继承证据的前半段还有 `inherited from a.b (…)`），
/// 且 `via` 之前必须恰好是 `表.列`。解析不出 ⇒ `None`（调用方点名报错）。
fn parse_parent_target(evidence: &str) -> Option<(String, String)> {
    let tail = evidence.rsplit_once("->")?.1.trim();
    let target = tail.split_once(" via").map(|(t, _)| t.trim()).unwrap_or_else(|| tail.trim());
    let (table, column) = target.rsplit_once('.')?;
    let table = table.trim();
    let column = column.trim();
    if table.is_empty() || column.is_empty() {
        return None;
    }
    Some((table.to_string(), column.to_string()))
}

// ══════════════════════════════════════════════════════════════
// 渲染
// ══════════════════════════════════════════════════════════════

/// 固定前缀表（顺序固定 ⇒ 逐字节确定性）。
const PREFIX_TABLE: &[(&str, &str)] = &[
    ("rr", "http://www.w3.org/ns/r2rml#"),
    ("ontok", "https://ontok.local/schema/"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
];

fn render(
    view: &ModelTrackView,
    opts: &TtlOptions<'_>,
    maps: &[MapPlan],
    skipped_classes: &[(String, String)],
    excluded: &[(String, String)],
    stats: &R2rmlStats,
    include_inherit: bool,
) -> String {
    let mut out = String::new();

    // ── 前缀 ────────────────────────────────────────────────
    out.push_str(&format!("@prefix map: <{}r2rml/{}#> .\n", opts.normalized_base(), local_model(opts.project)));
    out.push_str(&format!("@prefix : <{}#> .\n", opts.ontology_iri()));
    for (p, iri) in PREFIX_TABLE {
        out.push_str(&format!("@prefix {p}: <{iri}> .\n"));
    }
    out.push('\n');

    // ── 文件头：来源 / 三态策略 / 已知限制（AC 的「文件头说明」）──
    out.push_str("# ── REQ-121 · R2RML 映射（本体 ↔ 关系库的桥）──\n");
    out.push_str(&format!(
        "# 来源：项目 {} · 版本 v{} · 建模视图（{} 类 / {} 关系）· 快照 seq {}\n",
        opts.project,
        opts.version,
        view.entity_types.len(),
        view.link_types.len(),
        view.seq
    ));
    out.push_str(&format!(
        "# 三态策略：joinVerified \"true\" ⇒ 生成 joinCondition（{} 条）；\
         \"inherit\" ⇒ {}（{} 条，{}）；\"false\" ⇒ ⛔ 永不生成（{} 条，文件尾列原因）\n",
        stats.verified_edges,
        if include_inherit { "生成并标 ontok:joinState \"inherit\"" } else { "默认不生成" },
        stats.inherited_edges,
        if include_inherit {
            format!("{} 条已生成", stats.inherited_emitted)
        } else {
            "文件尾列清单".to_string()
        },
        stats.naming_only_edges
    ));
    out.push_str(&format!(
        "# 未标 joinVerified 的边（REQ-120 之前）：{} 条，同样不生成\n",
        stats.unverified_edges
    ));
    let tenant_note = if !tenancy_report(view, opts.tenancy).annotated() {
        "未标注（没给 --tenancy / 未签）—— 这份映射不能直接给数字员工用（REQ-122 无从注入租户过滤）"
            .to_string()
    } else {
        let classes = maps.iter().filter(|m| !m.tenant_columns.is_empty()).count();
        let columns: usize = maps.iter().map(|m| m.tenant_columns.len()).sum();
        format!("已标注（{classes} 个类 / {columns} 个列）—— REQ-122 据此注入租户过滤")
    };
    out.push_str(&format!("# 租户列：{tenant_note}\n"));
    out.push_str("# 已知限制：\n");
    out.push_str("#   · 复合主键 / 无单列主键的表**不生成** TriplesMap（R2RML 模板要拼多列），见文件尾\n");
    out.push_str("#   · 反向对象属性**不映射**（一条 join 只有一个方向；反向查询由引擎在前向映射上反解）\n");
    out.push_str("#   · 父列取自 ontok:joinEvidence 的取证记录（REQ-120），本轮不重新探测数据\n");
    out.push_str("#   · 只做 rr:tableName + 列 + join：视图 / 多表 union / 表达式不在本轮范围\n");
    out.push_str(&format!(
        "# 结构：{} · 数据类型属性 {} · join {} · 未生成类 {} · 被排除边 {}\n\n",
        stats.summary(),
        stats.datatype_maps,
        stats.join_maps,
        stats.skipped_classes.len(),
        stats.excluded_edges.len()
    ));

    // ── 逐类：一个 TriplesMap ────────────────────────────────
    for (i, m) in maps.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&triples_map(opts, m, maps));
    }

    // ── 文件尾：未生成清单 + 被排除的边（AC-006 / AC-011）────
    if !skipped_classes.is_empty() {
        out.push_str("\n# ── 未生成清单：复合主键 / 无单列主键（R2RML 模板要拼多列，本轮不做）──\n");
        for (key, reason) in skipped_classes {
            out.push_str(&format!("# · {key} —— {reason}\n"));
        }
    }
    if !excluded.is_empty() {
        out.push_str("\n# ── 被排除的边（joinVerified \"false\" / 没有证据）：⛔ 永不生成 join ──\n");
        out.push_str("# （false = 仅列名推断；行尾是「为什么没验」，供人排查）\n");
        for (edge, reason) in excluded {
            out.push_str(&format!("# · {edge}：{}\n", one_line(reason)));
        }
    }
    out
}

/// 一个 `rr:TriplesMap` 块（主语行 + 三件套 + 全部谓词-宾语映射）。
fn triples_map(opts: &TtlOptions<'_>, m: &MapPlan, maps: &[MapPlan]) -> String {
    let template = opts
        .instance_iri_template()
        .replace("{class}", &m.key)
        .replace("{id}", &format!("{{{}}}", m.pk));
    let mut lines: Vec<String> = Vec::new();
    lines.push("a rr:TriplesMap".to_string());
    lines.push(format!("rr:logicalTable [ rr:tableName {} ]", plain(&m.key)));
    for column in &m.tenant_columns {
        lines.push(format!("ontok:tenantColumn {}", plain(column)));
    }
    lines.push(format!(
        "rr:subjectMap [\n        rr:template {} ;\n        rr:class :{}\n    ]",
        plain(&template),
        m.class_local
    ));
    for d in &m.datatypes {
        lines.push(format!(
            "rr:predicateObjectMap [\n        rr:predicate :{}__{} ;\n        rr:objectMap [ rr:column {} ; rr:datatype {} ]\n    ]",
            m.class_local,
            local_model(&d.column),
            plain(&d.column),
            d.xsd.iri()
        ));
    }
    for j in &m.joins {
        let parent = maps
            .iter()
            .find(|x| x.key == j.parent_key)
            .map(|x| x.class_local.as_str())
            .unwrap_or(j.parent_key.as_str());
        let mut pom: Vec<String> = Vec::new();
        pom.push(format!("rr:predicate :{} ;", j.predicate_local));
        pom.push(format!("# joinEvidence: {}", one_line(&j.evidence)));
        if j.state == JoinState::Inherited {
            pom.push(format!("ontok:joinState {} ;", plain(JoinState::Inherited.as_str())));
            pom.push(format!("ontok:joinEvidence {} ;", plain(&j.evidence)));
        } else {
            pom.push(format!("ontok:joinState {} ;", plain(JoinState::Verified.as_str())));
        }
        pom.push(format!(
            "rr:objectMap [\n            rr:parentTriplesMap map:TM_{} ;\n            rr:joinCondition [ rr:child {} ; rr:parent {} ]\n        ]",
            parent,
            plain(&j.child),
            plain(&j.parent)
        ));
        lines.push(format!("rr:predicateObjectMap [\n        {}\n    ]", pom.join("\n        ")));
    }
    let mut out = String::new();
    out.push_str(&format!("map:TM_{} ", m.class_local));
    for (i, l) in lines.iter().enumerate() {
        if i == 0 {
            out.push(' ');
        } else {
            out.push_str("\n    ");
        }
        out.push_str(l);
        if i + 1 == lines.len() {
            out.push_str(" .\n");
        } else {
            out.push_str(" ;");
        }
    }
    out
}

// ══════════════════════════════════════════════════════════════
// 文本原语（与 TTL 同一个转义器，⛔ 不抄第二份）
// ══════════════════════════════════════════════════════════════

/// Turtle 字符串字面量：`"…"`（转义走 [`escape`]）。
fn plain(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

/// 注释行里的一行文本：真换行 / 回车压成空格（注释里换行会把一条理由劈成两行）。
fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

// ══════════════════════════════════════════════════════════════
// 测试
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView};
    use crate::domain::ttl::SchemaTypes;

    /// 一份最小建模视图：两个类 + 一条 `true` 边 + 一条 `inherit` + 一条 `false`。
    fn view() -> ModelTrackView {
        ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                ModelEntityView {
                    key: "orders".into(),
                    display_name: "订单".into(),
                    description: None,
                    aliases: vec![],
                    parent: None,
                    disjoint_with: vec![],
                    properties: vec![
                        ModelPropertyView {
                            name: "金额".into(),
                            column: "amount".into(),
                            nullable: false,
                            note: None,
                            sample: None,
                        },
                        ModelPropertyView {
                            name: "客户".into(),
                            column: "customer_id".into(),
                            nullable: false,
                            note: None,
                            sample: None,
                        },
                        ModelPropertyView {
                            name: "下单时间".into(),
                            column: "created_at".into(),
                            nullable: false,
                            note: None,
                            sample: None,
                        },
                    ],
                    evidence: vec![],
                },
                ModelEntityView {
                    key: "customers".into(),
                    display_name: "客户".into(),
                    description: None,
                    aliases: vec![],
                    parent: None,
                    disjoint_with: vec![],
                    properties: vec![ModelPropertyView {
                        name: "客户号".into(),
                        column: "customer_no".into(),
                        nullable: false,
                        note: None,
                        sample: None,
                    }],
                    evidence: vec![],
                },
            ],
            link_types: vec![
                ModelLinkView {
                    key: "orders_customer_id__customers".into(),
                    from_type: "orders".into(),
                    to_type: "customers".into(),
                    cardinality: "N:1".into(),
                    semantic: "belongs_to".into(),
                    evidence: vec![crate::domain::snapshot::Evidence {
                        table: "orders".into(),
                        column: "customer_id".into(),
                        claim: "外键".into(),
                    }],
                    confidence: None,
                    join_verified: Some(JoinState::Verified),
                    join_evidence: Some(
                        "value-overlap 5/5 (100.00%) -> customers.id via primary_key (plural)"
                            .into(),
                    ),
                    column: Some("customer_id".into()),
                },
                ModelLinkView {
                    key: "orders_license_id__licenses".into(),
                    from_type: "orders".into(),
                    to_type: "customers".into(),
                    cardinality: "N:1".into(),
                    semantic: "belongs_to".into(),
                    evidence: vec![crate::domain::snapshot::Evidence {
                        table: "orders".into(),
                        column: "license_id".into(),
                        claim: "外键".into(),
                    }],
                    confidence: None,
                    join_verified: Some(JoinState::Inherited),
                    join_evidence: Some(
                        "inherited from license_quota.license_id (verified 5/5) -> customers.id via primary_key (plural)"
                            .into(),
                    ),
                    column: Some("license_id".into()),
                },
                ModelLinkView {
                    key: "orders_amount__customers".into(),
                    from_type: "orders".into(),
                    to_type: "customers".into(),
                    cardinality: "N:1".into(),
                    semantic: "belongs_to".into(),
                    evidence: vec![],
                    confidence: None,
                    join_verified: Some(JoinState::NamingOnly),
                    join_evidence: Some("insufficient_sample: 有效样本 1 行 < 3 条下限".into()),
                    column: Some("amount".into()),
                },
            ],
            action_types: vec![],
            functions: vec![],
        }
    }

    fn schema_json() -> String {
        r#"{"tables":[
            {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
             "columns":[{"name":"order_id","data_type":"bigint","is_primary_key":true},
                        {"name":"customer_id","data_type":"bigint"},
                        {"name":"license_id","data_type":"bigint"},
                        {"name":"amount","data_type":"numeric(10,2)"},
                        {"name":"created_at","data_type":"timestamp without time zone"}]},
            {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
             "columns":[{"name":"id","data_type":"bigint","is_primary_key":true},
                        {"name":"customer_no","data_type":"text"}]}
        ]}"#
        .to_string()
    }

    struct Fixture {
        view: ModelTrackView,
        schema: SchemaTypes,
    }

    fn fixture() -> Fixture {
        Fixture { view: view(), schema: SchemaTypes::parse(&schema_json()).expect("schema") }
    }

    fn opts<'a>(schema: &'a SchemaTypes, tenancy: Option<&'a crate::domain::ttl::TenancyDecl>) -> TtlOptions<'a> {
        let mut o = TtlOptions::new("demo", 2);
        o.schema = Some(schema);
        o.tenancy = tenancy;
        o
    }

    #[test]
    fn every_class_with_single_pk_gets_a_complete_triples_map() {
        let f = fixture();
        let doc = generate(&f.view, &opts(&f.schema, None), false).expect("generate");
        let text = doc.doc();
        assert_eq!(doc.stats().triples_maps, 2);
        assert_eq!(text.matches("a rr:TriplesMap").count(), 2);
        // 三件套：logicalTable（含 tableName）+ subjectMap（template + class）+ ≥1 POM
        for table in ["orders", "customers"] {
            assert!(text.contains(&format!("rr:logicalTable [ rr:tableName \"{table}\" ]")), "{text}");
        }
        assert_eq!(text.matches("rr:subjectMap [").count(), 2);
        assert_eq!(text.matches("rr:template ").count(), 2);
        assert_eq!(text.matches("rr:class :").count(), 2);
        // 每个 TriplesMap 至少一个 POM：两类的 POM 总数 > TriplesMap 数
        assert!(doc.stats().datatype_maps >= 2, "{:?}", doc.stats());
        // 空壳：不允许出现「只有 subjectMap 没有 predicateObjectMap」的块
        // （文件尾的清单不是 TriplesMap，先切掉再逐块数）。
        let body = text.split("\n# ──").next().unwrap_or(text);
        for block in body.split("map:TM_").skip(1) {
            assert!(block.contains("rr:predicateObjectMap ["), "空壳 TriplesMap：{block}");
        }
    }

    #[test]
    fn subject_template_expands_instance_iri_with_the_primary_key_column() {
        let f = fixture();
        let doc = generate(&f.view, &opts(&f.schema, None), false).expect("generate");
        let text = doc.doc();
        assert!(
            text.contains("rr:template \"https://ontok.local/demo#orders/{order_id}\""),
            "template 必须由 ontok:instanceIri 的 {{id}} 展开成主键列：{text}"
        );
        assert!(text.contains("rr:template \"https://ontok.local/demo#customers/{id}\""));
        // 与 TTL 的 instanceIri 同一个模板
        let ttl = crate::domain::ttl::serialize_model(&f.view, &opts(&f.schema, None));
        assert!(ttl.doc().contains("ontok:instanceIri \"https://ontok.local/demo#{class}/{id}\""));
    }

    #[test]
    fn datatypes_match_the_ttl_rdfs_range() {
        let f = fixture();
        let o = opts(&f.schema, None);
        let doc = generate(&f.view, &o, false).expect("generate");
        let ttl = crate::domain::ttl::serialize_model(&f.view, &o);
        // orders.amount ⇒ xsd:decimal（真实列类型 numeric）·orders.created_at ⇒ xsd:dateTime
        assert!(doc.doc().contains("rr:column \"amount\" ; rr:datatype xsd:decimal"), "{}", doc.doc());
        assert!(doc.doc().contains("rr:column \"created_at\" ; rr:datatype xsd:dateTime"));
        // 与 TTL 的 range 逐条一致（同一个 xsd_range 的唯一实现）
        for (prop_local, want) in [
            ("orders__amount", "xsd:decimal"),
            ("orders__created_at", "xsd:dateTime"),
            ("customers__customer_no", "xsd:string"),
        ] {
            let block = ttl
                .doc()
                .split(&format!(":{prop_local} "))
                .nth(1)
                .unwrap_or_else(|| panic!("TTL 里找不到 {prop_local}"));
            assert!(
                block.lines().take(6).any(|l| l.contains(&format!("rdfs:range {want}"))),
                "{prop_local} 的 TTL range 不是 {want}：{block}"
            );
            assert!(
                doc.doc()
                    .split(&format!("rr:predicate :{prop_local} ;"))
                    .nth(1)
                    .is_some_and(|b| b.contains(&format!("rr:datatype {want}"))),
                "{prop_local} 的 R2RML datatype 不是 {want}"
            );
        }
    }

    #[test]
    fn verified_edge_becomes_a_join_condition_with_child_and_parent_columns() {
        let f = fixture();
        let doc = generate(&f.view, &opts(&f.schema, None), false).expect("generate");
        let text = doc.doc();
        assert_eq!(doc.stats().join_maps, 1, "{}", doc.stats().summary());
        assert_eq!(doc.stats().join_conditions(), 1);
        assert!(text.contains("rr:joinCondition [ rr:child \"customer_id\" ; rr:parent \"id\" ]"), "{text}");
        assert!(text.contains("rr:parentTriplesMap map:TM_customers"));
        // AC-004：join 旁必须有一行注释写明 joinEvidence
        assert!(
            text.contains("# joinEvidence: value-overlap 5/5 (100.00%) -> customers.id via primary_key (plural)"),
            "{text}"
        );
        assert!(text.contains("ontok:joinState \"true\""));
    }

    #[test]
    fn inherit_edges_are_absent_by_default_and_flagged_when_included() {
        let f = fixture();
        let o = opts(&f.schema, None);
        let default = generate(&f.view, &o, false).expect("generate");
        assert_eq!(default.stats().join_maps, 1);
        assert!(!default.doc().contains("ontok:joinState \"inherit\""));
        assert!(default.doc().contains("# · orders → customers（child orders.license_id）"));

        let with = generate(&f.view, &o, true).expect("generate");
        assert_eq!(with.stats().join_maps, 2);
        assert_eq!(with.stats().inherited_emitted, 1);
        assert!(with.doc().contains("ontok:joinState \"inherit\""), "{}", with.doc());
        assert!(with
            .doc()
            .contains("ontok:joinEvidence \"inherited from license_quota.license_id (verified 5/5) -> customers.id via primary_key (plural)\""));
        assert!(with.doc().contains("rr:joinCondition [ rr:child \"license_id\" ; rr:parent \"id\" ]"));
    }

    #[test]
    fn false_edges_never_produce_a_join_in_any_mode_and_are_listed_with_a_reason() {
        let f = fixture();
        let o = opts(&f.schema, None);
        for include in [false, true] {
            let doc = generate(&f.view, &o, include).expect("generate");
            assert!(
                !doc.doc().contains("rr:child \"amount\""),
                "false 边生成了 join（include_inherit={include}）：{}",
                doc.doc()
            );
            assert!(doc.doc().contains(
                "# · orders → customers（child orders.amount）：insufficient_sample: 有效样本 1 行 < 3 条下限"
            ));
        }
    }

    #[test]
    fn tenant_columns_land_on_the_triples_map_only_when_the_declaration_is_signed() {
        let f = fixture();
        let unsigned = crate::domain::ttl::TenancyDecl::parse(
            r#"{"columns":["customer_id"],"reviewed_by":""}"#,
        )
        .expect("parse");
        let doc = generate(&f.view, &opts(&f.schema, Some(&unsigned)), false).expect("generate");
        assert!(!doc.doc().contains("ontok:tenantColumn"), "未签声明不许落标注");

        let signed = crate::domain::ttl::TenancyDecl::parse(
            r#"{"columns":["customer_id"],"reviewed_by":"安全负责人"}"#,
        )
        .expect("parse");
        let doc = generate(&f.view, &opts(&f.schema, Some(&signed)), false).expect("generate");
        assert!(doc.doc().contains("ontok:tenantColumn \"customer_id\""));
        assert!(doc.doc().contains("# 租户列：已标注（1 个类 / 1 个列）"), "{}", doc.doc());
    }

    #[test]
    fn composite_primary_keys_are_skipped_and_listed_with_a_reason() {
        let mut f = fixture();
        // 第三个类：复合主键、没有任何边 ⇒ 只跳过它，其余照常生成。
        f.view.entity_types.push(ModelEntityView {
            key: "audit_logs".into(),
            display_name: "审计日志".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![],
        });
        let mut json: serde_json::Value = serde_json::from_str(&schema_json()).unwrap();
        json["tables"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "audit_logs",
            "primary_key": null,
            "composite_primary_key": ["tenant", "subject_id"],
            "unique_columns": [],
            "columns": [
                {"name": "tenant", "data_type": "text"},
                {"name": "subject_id", "data_type": "bigint"}
            ]
        }));
        f.schema = SchemaTypes::parse(&json.to_string()).expect("schema");
        let doc = generate(&f.view, &opts(&f.schema, None), false).expect("generate");
        assert_eq!(doc.stats().triples_maps, 2, "复合主键的类不许有 TriplesMap");
        assert!(!doc.doc().contains("rr:tableName \"audit_logs\""));
        assert_eq!(
            doc.stats().skipped_classes,
            vec![("audit_logs".to_string(), "复合主键（tenant、subject_id）".to_string())]
        );
        assert!(doc.doc().contains("# · audit_logs —— 复合主键（tenant、subject_id）"));
    }

    #[test]
    fn a_skipped_parent_class_makes_the_verified_edge_a_named_failure() {
        let mut f = fixture();
        let mut json: serde_json::Value = serde_json::from_str(&schema_json()).unwrap();
        json["tables"][1]["primary_key"] = serde_json::Value::Null;
        json["tables"][1]["composite_primary_key"] = serde_json::json!(["tenant", "id"]);
        f.schema = SchemaTypes::parse(&json.to_string()).expect("schema");
        let err = generate(&f.view, &opts(&f.schema, None), false).unwrap_err();
        assert!(
            err.failures().iter().any(|m| m.contains("父类 customers 没有 TriplesMap")),
            "{err}"
        );
        assert!(err.to_string().contains("orders_customer_id__customers"), "{err}");
    }

    #[test]
    fn schema_reconciliation_names_every_missing_table_and_column() {
        let mut f = fixture();
        let mut json: serde_json::Value = serde_json::from_str(&schema_json()).unwrap();
        // 删掉 orders 的 amount 列
        let cols = json["tables"][0]["columns"].as_array_mut().unwrap();
        cols.retain(|c| c["name"] != "amount");
        f.schema = SchemaTypes::parse(&json.to_string()).expect("schema");
        let err = generate(&f.view, &opts(&f.schema, None), false).unwrap_err();
        assert!(
            err.failures().iter().any(|m| m.contains("表 orders 缺列 amount")),
            "{err}"
        );
        // 再删一张表 ⇒ 表名也要点名
        let mut json2: serde_json::Value = serde_json::from_str(&schema_json()).unwrap();
        json2["tables"].as_array_mut().unwrap().remove(1);
        let mut f2 = fixture();
        f2.schema = SchemaTypes::parse(&json2.to_string()).expect("schema");
        let err2 = generate(&f2.view, &opts(&f2.schema, None), false).unwrap_err();
        assert!(
            err2.failures().iter().any(|m| m.contains("表 customers 不在 schema.json 里")),
            "{err2}"
        );
    }

    #[test]
    fn a_parent_column_that_is_not_unique_is_a_named_failure() {
        let f = fixture();
        let mut v = f.view.clone();
        // 证据指向 customers.customer_no —— 它既不是主键也不在 unique_columns 里。
        v.link_types[0].join_evidence = Some(
            "value-overlap 5/5 (100.00%) -> customers.customer_no via unique_column (exact)".into(),
        );
        let err = generate(&v, &opts(&f.schema, None), false).unwrap_err();
        assert!(
            err.failures()
                .iter()
                .any(|m| m.contains("表 customers.customer_no 不是主键也不是唯一列")),
            "{err}"
        );
    }

    #[test]
    fn a_join_evidence_that_lacks_a_parent_column_is_a_named_failure() {
        let f = fixture();
        let mut v = f.view.clone();
        v.link_types[0].join_evidence = Some("value-overlap 5/5 (100.00%)".into());
        let err = generate(&v, &opts(&f.schema, None), false).unwrap_err();
        assert!(err.failures().iter().any(|m| m.contains("父列无法从 ontok:joinEvidence 解析")), "{err}");
    }

    #[test]
    fn the_same_input_twice_is_byte_identical() {
        let f = fixture();
        let o = opts(&f.schema, None);
        let a = generate(&f.view, &o, true).expect("generate");
        let b = generate(&f.view, &o, true).expect("generate");
        assert_eq!(a.doc(), b.doc());
        assert_eq!(a.stats(), b.stats());
    }

    #[test]
    fn parent_target_parser_reads_the_target_after_the_last_arrow() {
        assert_eq!(
            parse_parent_target("value-overlap 5/5 (100.00%) -> customers.id via primary_key (plural)"),
            Some(("customers".into(), "id".into()))
        );
        assert_eq!(
            parse_parent_target(
                "inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)"
            ),
            Some(("licenses".into(), "id".into()))
        );
        assert_eq!(
            parse_parent_target("value-overlap 5/5 (100.00%) -> llm_model.model_id via unique_column (substring)"),
            Some(("llm_model".into(), "model_id".into()))
        );
        assert_eq!(parse_parent_target("insufficient_sample: 有效样本 1 行"), None);
        assert_eq!(parse_parent_target(""), None);
    }

    #[test]
    fn missing_schema_is_a_hard_error_not_a_heuristic_run() {
        let v = view();
        let mut o = TtlOptions::new("demo", 2);
        o.schema = None;
        let err = generate(&v, &o, false).unwrap_err();
        assert!(err.failures()[0].contains("缺少 --schema"), "{err}");
    }

    #[test]
    fn one_line_flattens_newlines_in_evidence_comments() {
        assert_eq!(one_line("a\nb\r\nc"), "a b  c");
    }
}
