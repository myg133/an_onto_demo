//! REQ-110 · **Turtle（TTL）序列化器 —— 纯函数，零 I/O，零依赖**。
//!
//! # 一句话
//!
//! 把本平台的两种投影（客户视图 / 建模视图）**确定性地**写成 OWL Turtle。
//! 默认导出客户视图（页面看到什么就导出什么，R1）；`--model-view` 显式选建模视图。
//!
//! # 为什么手写而不是引一个 RDF 库
//!
//! 需求「不做」里写死了：**不引入任何新依赖**（不用 oxrdf / sophia）。
//! 本轮只需要**序列化**（不做解析、不做推理），映射表是规格 §5.5 那九行 ——
//! 引一个 RDF 栈来干这九行，换来的是一个新的供应链面与一次依赖评审。
//!
//! # 确定性（R6 / AC-003）
//!
//! 同一 project + version 的两次导出**逐字节相同**，靠三件事：
//!
//! 1. **排序**：类按 key / 名字字典序，属性按（类, 列名 / 属性名），关系按 key；
//! 2. **不落时间**：`dcterms:created` **刻意不写** —— 它会让两次导出差一个字节。
//!    ⇒ 与外部脚本 `ai-ontology.ttl` 的**唯一**三元组差异就是这一条（已登记）；
//! 3. **不用 HashMap 迭代序**：本文件只用 `BTreeMap` / `sort`。
//!
//! # 有损项必须**显式**声明（R3 / AC-008）
//!
//! 文件头有 `#` 注释块，逐项写清：`ActionType` 未导出（数量）、`semantic` 降级为
//! `rdfs:comment`、`cardinality` 只映射 Functional / InverseFunctional。
//!
//! # ⚠️ AC-002 与 AC-008 的直接冲突（以及本模块的取舍）
//!
//! - AC-008 要求文件头出现 `cardinality` 这个词；
//! - AC-002 要求把序列化结果喂给 `scan_modeling_terms`，命中即拒绝；
//! - 而 `cardinality` **正是**守门词表里的一员（`view::MODELING_TERMS`）。
//!
//! ⇒ 「扫整份文件」与「文件头必须写 cardinality」**不可能同时成立**。
//! 本模块的取舍：**把「文件头」与「数据正文」分开产出**（[`TtlDocument`]），
//! 发布闸只扫 [`TtlDocument::body`]（**全部由数据产生的字节**：IRI、label、
//! comment 字面量、溯源），文件头是**编译期常量**（只随两个计数变化），
//! 由 `tests/test_req110_ttl.rs` 的 `header_declares_the_three_losses` 单独钉住。
//!
//! ⛔ 这不是「扫结构字段」：body 就是最终要写进文件的**那批字节**，只是把
//! **常量头**（与数据无关、且被 AC-008 强行要求含守门词）排除在扫描外。
//! 反过来说：任何**由数据生成**的文本（哪怕它落在注释里）都在扫描范围内。
//!
//! # 与参照物 `code/out/ai-ontology.ttl` 的对应
//!
//! 参照物由外部脚本 `.local/export_ttl.py` 从 `--model-view` 的 JSON 生成
//! （83 类 / 866 数据属性 / 52 对象属性 / 5954 三元组）。本模块的建模视图映射
//! **逐条照它的口径**（含 `ontok:key` / `ontok:column` / `ontok:nullable` /
//! `ontok:cardinality` / `ontok:semantic` / `skos:altLabel` / 属性注释只在
//! `note != name` 时输出），于是**去掉 `dcterms:created` 之后三元组集合同构**。
//!
//! 客户视图的映射**不是**参照物的降级版：客户视图里没有 key / 列名 / 基数记号，
//! 只有人话（对象名、属性名、说明、`belongs_to` 的父对象名、`how_many` 的人话）。
//! 所以客户视图这一侧只导 TBox 骨架：
//!
//! | 客户视图 | Turtle | 说明 |
//! | --- | --- | --- |
//! | `EntityView.name` | `owl:Class` + `rdfs:label` + IRI 本地名 | 名字进 IRI（转义见 [`local_customer`]） |
//! | `EntityView.note` | `rdfs:comment` | 过闸（客户可见文案） |
//! | `EntityView.belongs_to` | `rdfs:subClassOf` | 父对象的**名字** |
//! | `EntityView.aliases` | `skos:altLabel` | 与参照脚本同口径 |
//! | `PropertyView` | `owl:DatatypeProperty` + `rdfs:domain` + `rdfs:range rdfs:Literal` | `nullable = !required` |
//! | `RelationView` | `owl:ObjectProperty` + `rdfs:domain` / `rdfs:range` | 见下方基数映射 |
//! | `RelationView.how_many` | `owl:FunctionalProperty` / `owl:InverseFunctionalProperty` | 1:N ⇒ Functional；N:1 ⇒ InverseFunctional；1:1 ⇒ 两者 |
//! | `RelationView.meaning` | `rdfs:comment` | **降级**（AC-008 已声明有损） |
//! | 样例值（`sample`） | **不导出** | 那是实例面的东西（G1：只导 TBox） |
//!
//! # 已知限制
//!
//! - 建模视图的 IRI 本地名用参照脚本的 `local()`（非 `[0-9A-Za-z_]` 一律换 `_`）：
//!   它**不是单射**（`a-b` 与 `a_b` 会撞）。参照物如此，本轮照抄以保持等价；
//!   客户视图**不用**它（中文名字会被压成一串下划线），另用 [`local_customer`]。
//! - 关系 IRI 是 `from__to`（参照脚本如此）。同一对类型之间若有多条关系，
//!   会撞成同一个 subject —— 本轮数据没有这种情况（已作为已知限制登记）。

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView};
use crate::domain::view::CustomerView;

/// 默认 IRI 前缀（与参照物 `ai-ontology.ttl` 一致）。
pub const DEFAULT_BASE: &str = "https://ontok.local/";

/// 序列化参数。**不含时间**：时间会让两次导出差一个字节（R6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtlOptions<'a> {
    pub project: &'a str,
    pub version: i32,
    /// IRI 基。尾斜杠会被补齐；`#` 会被去掉（本体 IRI 用 `base + project`，前缀再补 `#`）。
    pub base: &'a str,
    /// 动作层条数 —— 只用于文件头的**有损声明**（`ActionType 未导出：N 个`）。
    /// ⛔ 不导出任何动作内容（OWL 无对应）。
    pub action_type_count: usize,
}

impl<'a> TtlOptions<'a> {
    /// 用默认 base 与「动作层 0 个」构造（测试与纯函数调用方用）。
    pub fn new(project: &'a str, version: i32) -> Self {
        Self { project, version, base: DEFAULT_BASE, action_type_count: 0 }
    }

    /// base 归一化：去掉结尾 `#`，补齐结尾 `/`。
    fn normalized_base(&self) -> String {
        let b = self.base.trim_end_matches('#');
        if b.ends_with('/') {
            b.to_string()
        } else {
            format!("{b}/")
        }
    }
}

/// 序列化结果：**文件头**（常量 + 两个计数）与**数据正文**分开。
///
/// ⛔ 分开的**唯一**理由写在模块头：发布闸只扫 [`TtlDocument::body`]，
/// 因为文件头按 AC-008 必须出现 `cardinality`（守门词），而数据正文才是
/// 泄漏面。写文件 / 发 HTTP 用的始终是 [`TtlDocument::doc`] 或
/// [`TtlDocument::into_doc`] —— 两者是**同一批字节**（doc = 头 ++ 正文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtlDocument {
    doc: String,
    body: String,
}

impl TtlDocument {
    /// 完整文件（头 ++ 正文）。**写出去的就是它**。
    pub fn doc(&self) -> &str {
        &self.doc
    }

    /// 数据正文（发布闸扫这一份）。
    pub fn body(&self) -> &str {
        &self.body
    }

    /// 拿走完整文件（只在确实需要 owned `String` 的地方用）。
    pub fn into_doc(self) -> String {
        self.doc
    }

    /// 字节数（回执 / 测试用；不改变内容）。
    pub fn len(&self) -> usize {
        self.doc.len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
    }
}

// ══════════════════════════════════════════════════════════════
// 入口
// ══════════════════════════════════════════════════════════════

/// **客户视图** → Turtle。发布闸要扫 [`TtlDocument::body`]（见模块头）。
pub fn serialize_customer(view: &CustomerView, opts: &TtlOptions<'_>) -> TtlDocument {
    let mut ns = NameSpace::new(opts, local_model(opts.project));

    // ① 先给类分配 IRI 本地名（按名字字典序 ⇒ 分配结果与输入顺序无关）。
    let classes = distinct_customer_classes(view);
    let class_locals: Vec<String> = classes.iter().map(|(name, _)| ns.local_for(name)).collect();

    // ② 再给属性分配（同一个 NameSpace，跨类别也不撞车）。
    let mut prop_blocks: Vec<String> = Vec::new();
    for (i, (_, c)) in classes.iter().enumerate() {
        let obj_local = class_locals[i].clone();
        for p in distinct_properties(c) {
            let subject_key = format!("{obj_local}__{}", local_customer(p.name));
            let subject = ns.local_for(&subject_key);
            prop_blocks.push(datatype_block(&subject, &obj_local, &p));
        }
    }

    // ③ 关系（按 (from,to,meaning,how_many) 排序）；两端用**已分配**的本地名，
    //    否则「两个名字撞成同一个形状」时关系会指到邻居身上。
    let relations = distinct_relations(view);
    let mut rel_blocks: Vec<String> = Vec::new();
    for r in &relations {
        let from_local = ns.local_for(r.from);
        let to_local = ns.local_for(r.to);
        let subject = ns.local_for(&format!("{from_local}__{to_local}"));
        rel_blocks.push(object_block_customer(&from_local, &to_local, &subject, r));
    }

    // ④ 类块（父对象名同样用已分配的本地名：父/子同名必然同 IRI）。
    let mut class_blocks: Vec<String> = Vec::new();
    for (i, (name, c)) in classes.iter().enumerate() {
        let parents: Vec<String> = c.parents.iter().map(|p| ns.local_for(p)).collect();
        class_blocks.push(customer_class_block(&class_locals[i], name, &parents, c));
    }

    let header = ontology_header(ns.ontology(), opts, CUSTOMER_ONTOLOGY_NOTE, None);
    finish(ns.ontology(), opts, Body {
        ontology: header,
        classes: class_blocks,
        props: prop_blocks,
        rels: rel_blocks,
    })
}

/// **建模视图** → Turtle。建模术语在这一轨是**要被裁决的内容**，故不过发布闸
/// （轨道由 `pipeline::spec_of(OutletId::D9)` 显式声明，不是「没调闸所以豁免」）。
pub fn serialize_model(view: &ModelTrackView, opts: &TtlOptions<'_>) -> TtlDocument {
    let ns = NameSpace::new(opts, local_model(opts.project));

    let mut entities: Vec<&ModelEntityView> = view.entity_types.iter().collect();
    entities.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.display_name.cmp(&b.display_name)));

    let mut class_blocks: Vec<String> = Vec::new();
    let mut prop_blocks: Vec<String> = Vec::new();
    for e in &entities {
        let class_local = local_model(&e.key);
        class_blocks.push(model_class_block(&class_local, e));
        let mut props: Vec<&ModelPropertyView> = e.properties.iter().collect();
        props.sort_by(|a, b| a.column.cmp(&b.column).then_with(|| a.name.cmp(&b.name)));
        for p in props {
            prop_blocks.push(model_datatype_block(&class_local, p, &e.key));
        }
    }

    let mut links: Vec<&ModelLinkView> = view.link_types.iter().collect();
    links.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.from_type.cmp(&b.from_type))
            .then_with(|| a.to_type.cmp(&b.to_type))
    });
    let mut rel_blocks: Vec<String> = Vec::new();
    for l in &links {
        rel_blocks.push(model_object_block(l));
    }

    finish(
        ns.ontology(),
        opts,
        Body {
            ontology: ontology_header(
                ns.ontology(),
                opts,
                &model_ontology_note(opts.action_type_count),
                view.extracted_by.as_ref().map(|e| (e.agent.as_str(), e.version.as_str(), e.round)),
            ),
            classes: class_blocks,
            props: prop_blocks,
            rels: rel_blocks,
        },
    )
}

// ══════════════════════════════════════════════════════════════
// 组装
// ══════════════════════════════════════════════════════════════

/// 固定前缀表（**顺序固定**，R6）。声明哪些前缀不影响三元组；这里把参照物
/// 出现过的那几个都声明上，另加 `skos`（别名要用）。
const PREFIX_TABLE: &[(&str, &str)] = &[
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("prov", "http://www.w3.org/ns/prov#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
];

/// 客户视图本体节点上的说明（**不含守门词**：它是正文，要过闸）。
const CUSTOMER_ONTOLOGY_NOTE: &str =
    "本导出为 OWL 本体，只描述世界由什么构成，不描述能对它做什么。动作层未导出，见文件头有损声明。";

/// 建模视图本体节点上的说明（与参照脚本逐字一致，只替换动作层条数）。
fn model_ontology_note(action_types: usize) -> String {
    format!(
        "本导出为 OWL 本体，只描述「世界由什么构成」，不描述「能对它做什么」。\
         源模型中另有 {action_types} 个 ActionType（写回、副作用、权限、审计），\
         OWL/SHACL/SWRL 均无干净对应，故未导出。见规格 §5.5。"
    )
}

/// IRI / 本地名的分配器。
struct NameSpace {
    /// `https://ontok.local/anspire_ai`
    ontology: String,
    /// 名字 → 已分配的本地名。
    assigned: BTreeMap<String, String>,
    /// 已经用过的本地名（防撞车）。
    used: BTreeSet<String>,
}

impl NameSpace {
    fn new(opts: &TtlOptions<'_>, project_local: String) -> Self {
        Self {
            ontology: format!("{}{}", opts.normalized_base(), project_local),
            assigned: BTreeMap::new(),
            used: BTreeSet::new(),
        }
    }

    fn ontology(&self) -> &str {
        &self.ontology
    }

    /// 名字 → 本地名。**同一个名字永远得到同一个本地名**；
    /// 不同的名字若被转义成同一个形状（`a-b` 与 `a_b`），第二个起追加 `_2`、`_3`…
    ///
    /// ⚠️ 分配顺序由**调用方的排序**决定 ⇒ 「同一份输入 ⇒ 逐字节相同」
    /// 不依赖任何哈希序。
    fn local_for(&mut self, name: &str) -> String {
        if let Some(v) = self.assigned.get(name) {
            return v.clone();
        }
        let base = local_customer(name);
        let mut candidate = base.clone();
        let mut n = 1usize;
        while self.used.contains(&candidate) {
            n += 1;
            candidate = format!("{base}_{n}");
        }
        self.used.insert(candidate.clone());
        self.assigned.insert(name.to_string(), candidate.clone());
        candidate
    }
}

struct Body {
    ontology: String,
    classes: Vec<String>,
    props: Vec<String>,
    rels: Vec<String>,
}

fn ontology_header(
    ontology: &str,
    opts: &TtlOptions<'_>,
    note: &str,
    extracted_by: Option<(&str, &str, i32)>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("<{ontology}> a owl:Ontology ;\n"));
    out.push_str(&format!(
        "    rdfs:label {} ;\n",
        zh(&format!("{} 本体（v{}）", opts.project, opts.version))
    ));
    out.push_str(&format!("    rdfs:comment {} ;\n", zh(note)));
    if let Some((agent, version, round)) = extracted_by {
        // 参照脚本：`prov:wasGeneratedBy [ a prov:Activity ; prov:wasAssociatedWith "agent" ;
        // ontok:agentVersion "v" ; ontok:round N ]`
        out.push_str("    prov:wasGeneratedBy [ a prov:Activity ;\n");
        out.push_str(&format!("            prov:wasAssociatedWith {} ;\n", plain(agent)));
        out.push_str(&format!("            ontok:agentVersion {} ;\n", plain(version)));
        out.push_str(&format!("            ontok:round {round} ] ;\n"));
    }
    out.push_str(&format!("    ontok:sourceProject {} ;\n", plain(opts.project)));
    out.push_str(&format!("    ontok:version {} .\n", opts.version));
    out
}

fn finish(ontology: &str, opts: &TtlOptions<'_>, b: Body) -> TtlDocument {
    // 头：固定前缀 + 有损声明（AC-008）。
    let mut doc = String::new();
    doc.push_str(&format!("@prefix : <{ontology}#> .\n"));
    doc.push_str("@prefix dcterms: <http://purl.org/dc/terms/> .\n");
    doc.push_str("@prefix ontok: <https://ontok.local/schema/> .\n");
    for (p, iri) in PREFIX_TABLE {
        doc.push_str(&format!("@prefix {p}: <{iri}> .\n"));
    }
    doc.push('\n');
    doc.push_str(&lossy_header(opts));
    doc.push('\n');

    let mut body = String::new();
    body.push_str(&b.ontology);
    for group in [&b.classes, &b.props, &b.rels] {
        if group.is_empty() {
            continue;
        }
        body.push('\n');
        body.push_str(&group.join("\n"));
    }
    doc.push_str(&body);
    TtlDocument { doc, body }
}

/// 文件头的**有损声明**（AC-008：三个关键词一个都不能少）。
///
/// ⛔ 它是**编译期常量 + 一个计数**：不含任何项目数据，所以把它排除在
/// 发布闸的扫描之外不会放过任何数据字节（理由见模块头）。
fn lossy_header(opts: &TtlOptions<'_>) -> String {
    format!(
        "# ── 有损声明：本文件是 OWL 本体的降级表达，逐项如下 ──\n\
         # · ActionType：未导出（{} 个）—— OWL / SHACL / SWRL 均无干净对应（规格 §5.5）\n\
         # · semantic：降级为 rdfs:comment（语义只能作注释，不可推理）\n\
         # · cardinality：只映射 Functional / InverseFunctional（1:1 / 1:N / N:1；多对多无对应）\n\
         # · 不含实例数据（ABox）；不含 SHACL 形状（nullable / 必填不导出）\n",
        opts.action_type_count
    )
}

// ══════════════════════════════════════════════════════════════
// 客户视图：块
// ══════════════════════════════════════════════════════════════

/// 客户视图里的一个类（同名对象**合并**：客户视图用名字当标识，同名就是同一个东西）。
struct CustomerClass<'a> {
    notes: Vec<&'a str>,
    parents: Vec<&'a str>,
    aliases: Vec<&'a str>,
    properties: Vec<CustomerProp<'a>>,
}

struct CustomerProp<'a> {
    name: &'a str,
    note: Option<&'a str>,
    required: bool,
}

/// 稳定的类清单：**按名字字典序**（R6），同名对象合并。
fn distinct_customer_classes(view: &CustomerView) -> Vec<(String, CustomerClass<'_>)> {
    let mut map: BTreeMap<&str, CustomerClass<'_>> = BTreeMap::new();
    for o in &view.objects {
        let e = map.entry(o.name.as_str()).or_insert_with(|| CustomerClass {
            notes: Vec::new(),
            parents: Vec::new(),
            aliases: Vec::new(),
            properties: Vec::new(),
        });
        if let Some(n) = o.note.as_deref().filter(|n| !n.trim().is_empty()) {
            e.notes.push(n);
        }
        if let Some(p) = o.belongs_to.as_deref().filter(|p| !p.trim().is_empty()) {
            e.parents.push(p);
        }
        for a in &o.aliases {
            if !a.trim().is_empty() {
                e.aliases.push(a.as_str());
            }
        }
        for p in &o.properties {
            e.properties.push(CustomerProp {
                name: p.name.as_str(),
                note: p.note.as_deref().filter(|n| !n.trim().is_empty()),
                required: p.required,
            });
        }
    }
    let mut out: Vec<(String, CustomerClass<'_>)> = Vec::with_capacity(map.len());
    for (name, mut c) in map {
        // 去重 + 排序：输出只取决于**集合**，不取决于输入顺序。
        c.notes.sort_unstable();
        c.notes.dedup();
        c.parents.sort_unstable();
        c.parents.dedup();
        c.aliases.sort_unstable();
        c.aliases.dedup();
        out.push((name.to_string(), c));
    }
    out
}

fn distinct_properties<'a>(c: &'a CustomerClass<'a>) -> Vec<CustomerProp<'a>> {
    let mut map: BTreeMap<&str, CustomerProp<'_>> = BTreeMap::new();
    for p in &c.properties {
        // 同名属性合并：`required` 取「任一必填」（保守：不把必填说成可选）；
        // `note` 取字典序最小的那条 —— 让结果只取决于集合。
        let e = map.entry(p.name).or_insert(CustomerProp {
            name: p.name,
            note: p.note,
            required: p.required,
        });
        e.required = e.required || p.required;
        if let Some(n) = p.note {
            e.note = Some(match e.note {
                Some(cur) if cur <= n => cur,
                _ => n,
            });
        }
    }
    map.into_values().collect()
}

fn customer_class_block(
    local: &str,
    name: &str,
    parent_locals: &[String],
    c: &CustomerClass<'_>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:Class".to_string());
    // `rdfs:label` 用**名字**，不是本地名 —— 客户看的是标签，不是 IRI。
    lines.push(format!("rdfs:label {}", zh(name)));
    for n in &c.notes {
        lines.push(format!("rdfs:comment {}", zh(n)));
    }
    for p in parent_locals {
        lines.push(format!("rdfs:subClassOf :{p}"));
    }
    for a in &c.aliases {
        lines.push(format!("skos:altLabel {}", zh(a)));
    }
    block(&format!(":{local}"), lines)
}

struct CustomerRelation<'a> {
    from: &'a str,
    to: &'a str,
    how_many: &'a str,
    meaning: &'a str,
}

/// 稳定的关系清单：按（from, to, meaning, how_many）字典序；完全相同的关系只留一条。
fn distinct_relations(view: &CustomerView) -> Vec<CustomerRelation<'_>> {
    let mut set: BTreeSet<(&str, &str, &str, &str)> = BTreeSet::new();
    for r in &view.relations {
        set.insert((r.from.as_str(), r.to.as_str(), r.meaning.as_str(), r.how_many.as_str()));
    }
    set.into_iter()
        .map(|(from, to, meaning, how_many)| CustomerRelation { from, to, how_many, meaning })
        .collect()
}

fn object_block_customer(
    from_local: &str,
    to_local: &str,
    subject_local: &str,
    r: &CustomerRelation<'_>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(
        match cardinality_features(r.how_many) {
            Cardinality::Both => {
                "a owl:FunctionalProperty, owl:InverseFunctionalProperty, owl:ObjectProperty"
            }
            Cardinality::Functional => "a owl:FunctionalProperty, owl:ObjectProperty",
            Cardinality::InverseFunctional => "a owl:InverseFunctionalProperty, owl:ObjectProperty",
            Cardinality::None => "a owl:ObjectProperty",
        }
        .to_string(),
    );
    lines.push(format!("rdfs:label {}", zh(&format!("{} → {}", r.from, r.to))));
    lines.push(format!(
        "rdfs:comment {}",
        zh(&format!("语义：{}（{}）", r.meaning, r.how_many))
    ));
    lines.push(format!("rdfs:domain :{from_local}"));
    lines.push(format!("rdfs:range :{to_local}"));
    block(&format!(":{subject_local}"), lines)
}

/// 客户视图里的基数**只有人话**（`how_many` 已被 `view::cardinality_cn` 翻译过）。
/// 按规格 §5.5 的表反推 OWL 特征；认不出的（含未翻译的记号）**不给特征** ——
/// 未翻译记号本身会被发布闸的第三档（形态判定）抓住。
enum Cardinality {
    Functional,
    InverseFunctional,
    Both,
    None,
}

fn cardinality_features(how_many: &str) -> Cardinality {
    match how_many {
        "一对多" => Cardinality::Functional,        // 1:N
        "多对一" => Cardinality::InverseFunctional, // N:1
        "唯一对应" => Cardinality::Both,            // 1:1
        _ => Cardinality::None,                     // 多对多 / 认不出
    }
}

// ══════════════════════════════════════════════════════════════
// 建模视图：块（口径逐条照 `.local/export_ttl.py`）
// ══════════════════════════════════════════════════════════════

fn model_class_block(class_local: &str, e: &ModelEntityView) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:Class".to_string());
    let label = if e.display_name.trim().is_empty() { e.key.as_str() } else { e.display_name.as_str() };
    lines.push(format!("rdfs:label {}", zh(label)));
    lines.push(format!("ontok:key {}", plain(&e.key)));
    if let Some(d) = e.description.as_deref().filter(|d| !d.trim().is_empty()) {
        lines.push(format!("rdfs:comment {}", zh(d)));
    }
    for a in &e.aliases {
        if !a.trim().is_empty() {
            lines.push(format!("skos:altLabel {}", zh(a)));
        }
    }
    if let Some(p) = e.parent.as_deref().filter(|p| !p.trim().is_empty()) {
        lines.push(format!("rdfs:subClassOf :{}", local_model(p)));
    }
    for d in &e.disjoint_with {
        if !d.trim().is_empty() {
            lines.push(format!("owl:disjointWith :{}", local_model(d)));
        }
    }
    block(&format!(":{class_local}"), lines)
}

fn datatype_block(subject_local: &str, obj_local: &str, p: &CustomerProp<'_>) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:label {}", zh(p.name)));
    lines.push(format!("rdfs:domain :{obj_local}"));
    lines.push("rdfs:range rdfs:Literal".to_string());
    if let Some(n) = p.note {
        lines.push(format!("rdfs:comment {}", zh(n)));
    }
    lines.push(format!("ontok:nullable {}", bool_lit(!p.required)));
    block(&format!(":{subject_local}"), lines)
}

fn model_datatype_block(
    class_local: &str,
    p: &ModelPropertyView,
    entity_key: &str,
) -> String {
    let column = if p.column.trim().is_empty() { p.name.as_str() } else { p.column.as_str() };
    let subject = format!(":{class_local}__{}", local_model(column));
    let label = if p.name.trim().is_empty() { column } else { p.name.as_str() };
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:domain :{class_local}"));
    lines.push("rdfs:range rdfs:Literal".to_string());
    lines.push(format!("rdfs:label {}", zh(label)));
    lines.push(format!("ontok:column {}", plain(column)));
    lines.push(format!("ontok:nullable {}", bool_lit(p.nullable)));
    // 参照脚本：**只在 note != name 时**输出注释（770 条 note 里只有 8 条满足）。
    if let Some(n) = p.note.as_deref() {
        if !n.trim().is_empty() && n != label {
            lines.push(format!("rdfs:comment {}", zh(n)));
        }
    }
    if let Some(s) = p.sample.as_deref() {
        lines.push(format!("ontok:sample {}", plain(s)));
    }
    let _ = entity_key; // subject 的本地名已含类 key；参数留着让调用点可读
    block(&subject, lines)
}

fn model_object_block(l: &ModelLinkView) -> String {
    let from_local = local_model(&l.from_type);
    let to_local = local_model(&l.to_type);
    let subject = format!(":{from_local}__{to_local}");
    let mut lines: Vec<String> = Vec::new();
    lines.push(
        match l.cardinality.as_str() {
            "1:1" => "a owl:FunctionalProperty, owl:InverseFunctionalProperty, owl:ObjectProperty",
            "1:N" => "a owl:FunctionalProperty, owl:ObjectProperty",
            "N:1" => "a owl:InverseFunctionalProperty, owl:ObjectProperty",
            _ => "a owl:ObjectProperty",
        }
        .to_string(),
    );
    lines.push(format!("rdfs:label {}", zh(&format!("{} → {}", l.from_type, l.to_type))));
    lines.push(format!("rdfs:domain :{from_local}"));
    lines.push(format!("rdfs:range :{to_local}"));
    lines.push(format!("ontok:cardinality {}", plain(&l.cardinality)));
    lines.push(format!("ontok:semantic {}", plain(&l.semantic)));
    if !l.semantic.trim().is_empty() {
        lines.push(format!(
            "rdfs:comment {}",
            zh(&format!(
                "语义 {}（has/belongs_to 无 OWL 对应物，导出有损，仅作注释）",
                l.semantic
            ))
        ));
    }
    if !l.key.trim().is_empty() {
        lines.push(format!("ontok:key {}", plain(&l.key)));
    }
    block(&subject, lines)
}

// ══════════════════════════════════════════════════════════════
// 文本原语
// ══════════════════════════════════════════════════════════════

/// 一个块：`:<local> a … ;\n    谓词 宾语 ;\n    … 谓词 宾语 .`
fn block(subject: &str, lines: Vec<String>) -> String {
    let mut out = String::with_capacity(64 + lines.iter().map(String::len).sum::<usize>());
    out.push_str(subject);
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

/// 普通字符串字面量：`"…"`（转义见 [`escape`]）。**不带语言标签**。
fn plain(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

/// 中文文本字面量：`"…"@zh`（R6：中文按 UTF-8 原样，不加 `\u` 转义）。
fn zh(s: &str) -> String {
    format!("\"{}\"@zh", escape(s))
}

fn bool_lit(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// Turtle 字符串转义：`\` `"` 换行 / 回车 / 制表 / 其余控制字符。
///
/// ⛔ 中文**不转义**（R6：按 UTF-8 原样输出）。控制字符用 `\u00XX` ——
/// 不这么做的话，一个裸 `\r` 会让 Turtle 文件在 Windows 上被读成两行。
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// **建模视图**的 IRI 本地名 —— 与参照脚本 `local()` **逐字一致**：
/// 非 `[0-9A-Za-z_]` 一律换 `_`；空 → `unnamed`；数字开头 → 前缀 `_`。
///
/// ⚠️ 不是单射（`a-b` 与 `a_b` 撞）。参照物如此，本轮照抄以保持等价；
/// 客户视图**不用**它（见 [`local_customer`]）。
pub fn local_model(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() {
        s = "unnamed".to_string();
    }
    if s.as_bytes()[0].is_ascii_digit() {
        s.insert(0, '_');
    }
    s
}

/// **客户视图**的 IRI 本地名 —— 客户看到的是人话（中文名），`local_model` 会把
/// 每个汉字压成 `_`（两个汉字的两个对象直接撞车）。
///
/// 这里做的是**保留**：字母（含汉字）、数字、`_`、`-`、`.` 原样；其余字符（空格、
/// 引号、`#`、`/`、`:`、`%` …）换成 `_`；空 → `unnamed`；数字开头 → 前缀 `_`。
///
/// 为什么保留 Unicode 而不是百分号转义：`:` 前缀写法（`:<local>`）里
/// 百分号转义**不合法**；而 Turtle 的 PN_LOCAL 允许 Unicode 字母 ——
/// 汉字原样是最小惊讶，也和 `rdfs:label` 里看到的名字对得上。
///
/// ⚠️ 与其他名字撞车由 [`NameSpace::local_for`] 统一加后缀（`_2` / `_3`）。
pub fn local_customer(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    if s.is_empty() {
        s = "unnamed".to_string();
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView};
    use crate::domain::view::{EntityView, PropertyView, RelationView};

    fn cv() -> CustomerView {
        CustomerView {
            objects: vec![
                EntityView {
                    name: "订单".into(),
                    note: Some("一张订单".into()),
                    belongs_to: None,
                    aliases: vec!["单据".into()],
                    properties: vec![
                        PropertyView {
                            name: "下单时间".into(),
                            note: None,
                            required: true,
                            sample: Some("2026-01-01".into()),
                        },
                        PropertyView {
                            name: "金额".into(),
                            note: Some("含税".into()),
                            required: false,
                            sample: None,
                        },
                    ],
                },
                EntityView {
                    name: "客户".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![],
                },
            ],
            relations: vec![RelationView {
                from: "订单".into(),
                to: "客户".into(),
                how_many: "多对一".into(),
                meaning: "归属于".into(),
            }],
        }
    }

    fn mv() -> ModelTrackView {
        ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                ModelEntityView {
                    key: "order".into(),
                    display_name: "订单".into(),
                    description: Some("一张订单".into()),
                    aliases: vec!["单据".into()],
                    parent: Some("root".into()),
                    disjoint_with: vec!["customer".into()],
                    properties: vec![ModelPropertyView {
                        name: "下单时间".into(),
                        column: "created_at".into(),
                        nullable: false,
                        note: Some("下单时间（列注释）".into()),
                        sample: None,
                    }],
                    evidence: vec![],
                },
                ModelEntityView {
                    key: "customer".into(),
                    display_name: "客户".into(),
                    description: None,
                    aliases: vec![],
                    parent: None,
                    disjoint_with: vec![],
                    properties: vec![],
                    evidence: vec![],
                },
            ],
            link_types: vec![ModelLinkView {
                key: "order_customer".into(),
                from_type: "order".into(),
                to_type: "customer".into(),
                cardinality: "N:1".into(),
                semantic: "belongs_to".into(),
                evidence: vec![],
                confidence: None,
            }],
            action_types: vec![],
            functions: vec![],
        }
    }

    fn opts() -> TtlOptions<'static> {
        TtlOptions { project: "demo", version: 2, base: DEFAULT_BASE, action_type_count: 0 }
    }

    #[test]
    fn header_declares_the_three_losses() {
        let d = serialize_customer(&cv(), &opts());
        for kw in ["ActionType", "semantic", "cardinality"] {
            assert!(d.doc().contains(kw), "文件头必须有 `{kw}`（AC-008）：\n{}", d.doc());
        }
        assert!(d.doc().contains("未导出（0 个）"), "数量必须写出来（AC-008）：\n{}", d.doc());
        // 正文**不许**含那个被 AC-008 强行要求写进文件头的守门词 —— 否则「扫正文」等于必挂。
        assert!(
            !d.body().contains("cardinality"),
            "正文里出现了 `cardinality` —— 发布闸会恒命中：\n{}",
            d.body()
        );
    }

    #[test]
    fn customer_export_is_deterministic_and_order_independent() {
        let a = serialize_customer(&cv(), &opts()).into_doc();
        let b = serialize_customer(&cv(), &opts()).into_doc();
        assert_eq!(a, b, "同一份视图两次导出必须逐字节相同（AC-003）");

        // 反序输入（对象 / 属性 / 关系都倒过来）必须产出同字节 ——
        // 「只取决于集合」这条比「两次调用相同」强：它挡的是 HashMap 迭代序。
        let mut rev = cv();
        rev.objects.reverse();
        for o in &mut rev.objects {
            o.properties.reverse();
        }
        rev.relations.reverse();
        assert_eq!(a, serialize_customer(&rev, &opts()).into_doc(), "输出依赖了输入顺序");
    }

    #[test]
    fn customer_counts_are_one_per_thing() {
        let d = serialize_customer(&cv(), &opts()).into_doc();
        assert_eq!(d.matches("a owl:Class").count(), 2, "{d}");
        assert_eq!(d.matches("a owl:DatatypeProperty").count(), 2, "{d}");
        assert_eq!(d.matches("owl:ObjectProperty").count(), 1, "{d}");
        assert!(d.contains("skos:altLabel"), "别名应写成 skos:altLabel：{d}");
        assert!(d.contains("rdfs:label \"订单\"@zh"), "中文标签必须原样输出：{d}");
    }

    #[test]
    fn model_export_maps_the_nine_rows() {
        let doc = serialize_model(&mv(), &opts());
        let d = doc.doc();
        assert!(d.contains("ontok:key \"order\""), "{d}");
        assert!(d.contains("ontok:column \"created_at\""), "{d}");
        assert!(d.contains("ontok:nullable false"), "{d}");
        assert!(d.contains("ontok:cardinality \"N:1\""), "{d}");
        assert!(d.contains("ontok:semantic \"belongs_to\""), "{d}");
        assert!(d.contains("owl:InverseFunctionalProperty"), "N:1 必须是 inverse-functional：{d}");
        assert!(d.contains("rdfs:subClassOf :root"), "{d}");
        assert!(d.contains("owl:disjointWith :customer"), "{d}");
        assert!(d.contains("rdfs:range rdfs:Literal"), "{d}");
        // 动作层不导出（只在文件头声明数量）—— 正文里根本没有动作内容。
        assert!(!doc.body().contains("ActionType "), "动作层不该进正文");
    }

    #[test]
    fn model_comment_only_when_note_differs_from_name() {
        // 参照脚本：`if p.get("note") and p["note"] != p.get("name")`
        let mut v = mv();
        v.entity_types[0].properties[0].note = Some("下单时间".into());
        let d = serialize_model(&v, &opts()).into_doc();
        assert_eq!(
            d.matches("rdfs:comment \"下单时间（列注释）\"").count(),
            0,
            "note == name 时不该输出注释：{d}"
        );
    }

    #[test]
    fn escaping_covers_quotes_backslash_and_controls() {
        let s = escape("a\"b\\c\nd\re\tf\u{1}");
        assert_eq!(s, "a\\\"b\\\\c\\nd\\re\\tf\\u0001");
        assert!(escape("中文").contains("中文"), "中文不许转义成 \\u");
    }

    #[test]
    fn local_names_are_safe_and_distinct() {
        assert_eq!(local_model("order"), "order");
        assert_eq!(local_model("2fa"), "_2fa");
        assert_eq!(local_model("a-b"), "a_b");
        assert_eq!(local_model(""), "unnamed");
        // 客户视图保留汉字；空格 / 引号换成下划线。
        assert_eq!(local_customer("订单"), "订单");
        assert_eq!(local_customer("a b"), "a_b");
        assert_eq!(local_customer("3月"), "_3月");
        let mut ns = NameSpace::new(&opts(), "demo".to_string());
        // 撞车：第二个同形名字拿到后缀，且**同一个名字**永远拿到同一个后缀。
        let a = ns.local_for("a-b");
        let b = ns.local_for("a_b");
        assert_ne!(a, b, "两个不同的名字不许共用一个 IRI");
        assert_eq!(ns.local_for("a-b"), a);
        assert_eq!(ns.local_for("a_b"), b);
    }

    #[test]
    fn base_option_is_normalized() {
        let mut o = opts();
        o.base = "https://example.org/onto";
        let d = serialize_customer(&cv(), &o).into_doc();
        assert!(d.contains("<https://example.org/onto/demo> a owl:Ontology"), "{d}");
        assert!(d.contains("@prefix : <https://example.org/onto/demo#> ."), "{d}");
    }

    #[test]
    fn extracted_by_becomes_prov_activity() {
        let mut v = mv();
        v.extracted_by = Some(crate::domain::snapshot::ExtractedBy {
            agent: "inproc:ontok::agent::RuleBasedAgent".into(),
            version: "0.1.0".into(),
            round: 1,
            failed_groups: vec![],
            source: None,
        });
        let d = serialize_model(&v, &opts()).into_doc();
        assert!(d.contains("prov:wasGeneratedBy [ a prov:Activity"), "{d}");
        assert!(
            d.contains("prov:wasAssociatedWith \"inproc:ontok::agent::RuleBasedAgent\""),
            "{d}"
        );
        assert!(d.contains("ontok:round 1"), "{d}");
    }
}
