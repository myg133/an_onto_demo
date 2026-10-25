//! `ontok access validate` —— 契约的**离线**自洽体检（⛔ 不连库、⛔ 不发网络请求）。
//!
//! # 四条「有牙」的检查（AC-002 逐条对应一个变异）
//!
//! | # | 检查 | 造坏的方式 | 码 |
//! | --- | --- | --- | --- |
//! | 1 | `iri_template` 与本体的 `ontok:instanceIri` 同形（R3） | 改 template | [`CODE_IRI_TEMPLATE`] |
//! | 2 | `capability.pushdown` 与 `local_only` **不重叠**（R4） | 同一字段写两处 | [`CODE_CAPABILITY_OVERLAP`] |
//! | 3 | relation 的 `from` / `to` 都在 `entities` 里（R7） | 写个不存在的类 | [`CODE_RELATION_ENDPOINT`] |
//! | 4 | `fields.x.path` 在 fixture 里**真的存在** | 写个不存在的路径 | [`CODE_FIELD_PATH`] |
//!
//! 第 4 条是设计稿 §六 点名的「最便宜的正确性检查」：它不需要凭据、不需要网络，
//! 却能把「字段名抄错 / 响应结构记错」这两类**最常见的映射事故**挡在发布之前。
//!
//! # 另外四条（不是 AC 点名的，但少了它们前四条可以被绕开）
//!
//! | 检查 | 为什么必须有 | 码 |
//! | --- | --- | --- |
//! | 映射的类在**本体里存在** | 类不在本体 ⇒ 第 1 条无从对账，而「跳过检查」看起来像「通过」 | [`CODE_CLASS_NOT_IN_ONTOLOGY`] |
//! | fixture 读得了、是合法 JSON | 读不了 ⇒ 第 4 条会静默变成「0 行里都没找到」 | [`CODE_FIXTURE_UNREADABLE`] |
//! | `iterator` 在 fixture 里选得出节点 | 空 ⇒ 第 4 条恒真（0 行循环一遍，什么都没检查） | [`CODE_ITERATOR_EMPTY`] |
//! | `identity.key_path` 每行都有 | 缺一行 ⇒ 那条实例的 IRI 拼成 `…/{id}` 字面量 | [`CODE_KEY_MISSING`] |
//!
//! # REQ-149 · 没有 fixture 不是错误，是「没证据」
//!
//! 旧口径把「没有夹具」判成 `FIXTURE_UNREADABLE`，于是为了让体检过，只能**从映射反向造夹具**：
//! `1/1` 覆盖、却零证据力。现在：
//!
//! | 情形 | 行为 |
//! | --- | --- |
//! | 有 `fixture` | 走原来的字段路径校验（不变） |
//! | **没有** `fixture` | **`ok = true`**（退出码 0）+ 一条 [`CODE_FIELD_PATH_NOT_CHECKED`] 警告（点名实体）+ 该实体**不出现在** `field_coverage` 里 |
//! | 写了 `fixture` 但读不到 | 仍是错误（说了要有却拿不出来，不能算通过） |
//!
//! 另外会说明这份夹具的**证据力**（`fixture_note`）：
//! `fixture_source: declared` = 作者声明独立采集（工具不核实）；
//! `fixture_source: derived` = 明确警告「由映射键反向生成，无独立证据力」；
//! 没声明时按结构启发式判断（每行只含映射引用到的字段 ⇒ 疑似反向生成，
//! 见 [`CODE_FIXTURE_LOOKS_DERIVED`]）。
//!
//! # 口径：**一次报全**，且每处都点名「哪条 entity / 哪个字段」
//!
//! ⛔ 不是「遇到第一条就退」—— 作者改一处、重跑一次、再改一处，是最劝退的体验，
//! 而这份检查的全部价值就是「让人一次改完」。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 只查**自洽**：查不出「腾讯云是不是真的支持 `Zone` 下推」。那需要人签（R6 就是为此）。
//! - 第 4 条只要求「**至少一行**里这个路径存在」；部分行缺失降级为 `warnings`
//!   （云 API 里合法可为空的字段不该把整份映射判红）。**报出覆盖率**，⛔ 不静默。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{
    expand_iri_template, expand_ttl_instance_iri, path, placeholders, Entity, Mapping, Ontology,
    Problem, CODE_CAPABILITY_OVERLAP, CODE_CLASS_NOT_IN_ONTOLOGY, CODE_FIELD_PATH,
    CODE_FIELD_PATH_NOT_CHECKED, CODE_FIXTURE_LOOKS_DERIVED, CODE_FIXTURE_UNREADABLE,
    CODE_IRI_TEMPLATE, CODE_ITERATOR_EMPTY, CODE_KEY_MISSING, CODE_RELATION_ENDPOINT,
};

/// 一条 entity 的对账面（`validate` 报告里逐条给出，AC-007 直接贴它）。
#[derive(Debug, Clone, Serialize)]
pub struct EntityCheck {
    pub id: String,
    pub class: String,
    /// 类在本体里的完整 IRI（类不在本体 ⇒ `null`）。
    pub class_iri: Option<String>,
    /// `access.json` 里写的原值（可能是 `ontok:…` 简写）。
    pub iri_template: String,
    /// 展开成完整 IRI 之后的样子（`ontok:` → `<opntology_iri>#`）。
    pub expanded_iri_template: String,
    /// 本体 `ontok:instanceIri` 的**原值**（含 `{class}` / `{id}`）。
    pub ttl_instance_iri: Option<String>,
    /// 本体模板按**本类**展开后的期望值 —— 与上一栏逐字对比就是 AC-007。
    pub ttl_expanded: Option<String>,
    /// 契约里声明的 fixture（`None` ⇒ 这个实体**没做**字段路径校准）。
    ///
    /// ⚠️ REQ-149 · 缺口 D：没有夹具时这个键**整个不出现**，而不是显示一个空字符串
    /// —— 空字符串会让人以为「有夹具，只是路径是空的」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture: Option<String>,
    pub fixture_rows: usize,
    /// 夹具来源（`declared` / `derived`）；缺省 ⇒ 不写（旧输出的键集合不变）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_source: Option<String>,
    /// 关于这份夹具**证据力**的人话（缺省 ⇒ 不写；见 [`CODE_FIXTURE_LOOKS_DERIVED`]）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_note: Option<String>,
    pub key_path: String,
    pub fields_checked: usize,
    pub reviewed: bool,
    /// `fields.<名>` 在 fixture 里的覆盖率（`1/1` 表示每一行都有）。
    ///
    /// ⚠️ **没有 fixture 时整个键不出现**（REQ-149 · D）：⛔ 不许显示成 `1/1`，
    /// 也不许显示成 `0/0` —— 那会看起来像「检查过，0 行」而不是「没检查」。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_coverage: Option<BTreeMap<String, String>>,
}

/// 体检报告（stdout 的唯一内容）。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ok: bool,
    pub contract_version: u32,
    pub mapping_version: u32,
    pub project: String,
    pub ontology_iri: String,
    pub entity_count: usize,
    pub relation_count: usize,
    pub entities: Vec<EntityCheck>,
    pub errors: Vec<Problem>,
    pub warnings: Vec<String>,
}

impl Report {
    /// 人读的一行结论（stderr 用）。
    pub fn headline(&self) -> String {
        // REQ-149 · 没有 fixture 的实体**没做**字段路径校验：
        // 头上那句「四条检查全过」必须补一句，否则又是「没证据看起来像有证据」。
        let unchecked = self
            .entities
            .iter()
            .filter(|e| e.field_coverage.is_none())
            .count();
        let unchecked_note = if unchecked == 0 {
            String::new()
        } else {
            format!("；其中 {unchecked} 条**未做字段路径校准**（没有 fixture，⛔ 不是通过）")
        };
        if self.ok {
            format!(
                "✓ access 契约自洽：{} 条 entity / {} 条关系，四条检查全过（{} 条提醒）{}",
                self.entity_count,
                self.relation_count,
                self.warnings.len(),
                unchecked_note
            )
        } else {
            format!(
                "✗ access 契约有 {} 处问题（{} 条 entity / {} 条关系已体检）",
                self.errors.len(),
                self.entity_count,
                self.relation_count
            )
        }
    }
}

/// 跑一遍全部检查。
///
/// `mapping_dir` = `--mapping` 所在目录：`fixture` 的相对路径**先按它解析**
/// （作者就在 mapping 旁边放 fixture，这是最不容易错的口径），再按 CWD 兜底。
pub fn validate(mapping: &Mapping, ontology: &Ontology, mapping_dir: &Path) -> Report {
    let mut errors: Vec<Problem> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut checks: Vec<EntityCheck> = Vec::new();

    for (i, e) in mapping.entities.iter().enumerate() {
        let loc = format!("entities[{i}]（id={}）", e.id);
        let mut check = EntityCheck {
            id: e.id.clone(),
            class: e.class.clone(),
            class_iri: None,
            iri_template: e.iri_template.clone(),
            expanded_iri_template: expand_iri_template(&e.iri_template, &ontology.iri),
            ttl_instance_iri: None,
            ttl_expanded: None,
            fixture: e.fixture.clone(),
            fixture_rows: 0,
            fixture_source: e.fixture_source.map(|s| s.as_str().to_string()),
            fixture_note: None,
            key_path: e.key_path.clone(),
            fields_checked: e.fields.len(),
            reviewed: e.reviewed(),
            // 有 fixture ⇒ 与旧输出一样是个（可能为空的）对象；
            // 没 fixture ⇒ `None`，序列化时整个键不出现。
            field_coverage: e.fixture.as_ref().map(|_| BTreeMap::new()),
        };

        check_iri_template(e, &loc, ontology, &mut check, &mut errors, &mut warnings);
        check_capability(e, &loc, &mut errors);
        check_fixture(e, &loc, mapping_dir, &mut check, &mut errors, &mut warnings);

        checks.push(check);
    }

    check_relations(mapping, &mut errors, &mut warnings);

    if !mapping.entities.iter().any(|e| e.reviewed()) {
        warnings.push(
            "所有 entity 都未签（`reviewed_by` 为空）—— 本契约可以体检，但 `fetch` 会拒绝取数（R6）。"
                .to_string(),
        );
    }

    Report {
        ok: errors.is_empty(),
        contract_version: mapping.contract_version,
        mapping_version: mapping.mapping_version,
        project: mapping.project.clone(),
        ontology_iri: ontology.iri.clone(),
        entity_count: mapping.entities.len(),
        relation_count: mapping.relations.len(),
        entities: checks,
        errors,
        warnings,
    }
}

// ══════════════════════════════════════════════════════════════
// 检查 1 · iri_template ⇔ ontok:instanceIri（R3）
// ══════════════════════════════════════════════════════════════

fn check_iri_template(
    e: &Entity,
    loc: &str,
    ontology: &Ontology,
    check: &mut EntityCheck,
    errors: &mut Vec<Problem>,
    warnings: &mut Vec<String>,
) {
    let Some(decl) = ontology.class(&e.class) else {
        let sample: Vec<&str> = ontology.classes.keys().take(5).map(|s| s.as_str()).collect();
        errors.push(Problem::new(
            CODE_CLASS_NOT_IN_ONTOLOGY,
            format!("{loc} 字段 class"),
            format!(
                "类 `{}` 不在本体里（本体共 {} 个类，前 5 个：{}）。\
                 类不在本体 ⇒ 拼出来的 IRI 接不上图，第 1 条检查也无从对账。",
                e.class,
                ontology.classes.len(),
                if sample.is_empty() { "（空）".to_string() } else { sample.join(" / ") }
            ),
        ));
        return;
    };
    check.class_iri = Some(decl.iri.clone());
    let class_local = Ontology::local_name(&decl.iri).to_string();

    let Some(raw) = decl.instance_iri.as_deref() else {
        errors.push(Problem::new(
            CODE_IRI_TEMPLATE,
            format!("{loc} 字段 iri_template"),
            format!(
                "本体里类 `{}` 没有 `ontok:instanceIri` —— 没有模板就无法对账（⛔ 跳过检查不算通过）。\
                 先跑 `ontok ontology export` 导出一份带模板的 TTL。",
                check.class
            ),
        ));
        return;
    };

    let expected = expand_ttl_instance_iri(raw, &class_local);
    let got = expand_iri_template(&e.iri_template, &ontology.iri);
    check.ttl_instance_iri = Some(raw.to_string());
    check.ttl_expanded = Some(expected.clone());

    if got == expected {
        return;
    }

    // 差异只在占位符上 ⇒ 给一条更具体的修改建议（这是最常见的写法错）。
    let shape = |s: &str| {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(st) = rest.find('{') {
            out.push_str(&rest[..st]);
            match rest[st..].find('}') {
                Some(en) => {
                    out.push_str("{}");
                    rest = &rest[st + en + 1..];
                }
                None => {
                    out.push_str(&rest[st..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    };
    let hint = if shape(&got) == shape(&expected) {
        format!(
            "占位符名字对不上：本体用的是 {:?}。`iri_template` 的占位符固定写 `{{id}}`\
             （填什么由 `identity.key_path` 决定），`{{class}}` 也可用。",
            placeholders(&expected)
        )
    } else {
        "模板的**前缀 / 类名段**也要逐字一致（`ontok:` 会被展开成 `<本体 IRI>#`）。".to_string()
    };

    errors.push(Problem::new(
        CODE_IRI_TEMPLATE,
        format!("{loc} 字段 iri_template"),
        format!(
            "`iri_template` 与本体的 `ontok:instanceIri` 不一致（R3）：\n\
             access.json : {got:?}\n\
             本体 TTL    : {expected:?}（原值 {raw:?}，按类 `{class_local}` 展开）\n\
             {hint}"
        ),
    ));

    if !e.iri_template.contains("{id}") {
        warnings.push(format!(
            "{} 的 `iri_template` 里没有 `{{id}}` 占位符 —— 同类的两个实例会撞成同一个 IRI。",
            check.id
        ));
    }
}

// ══════════════════════════════════════════════════════════════
// 检查 2 · pushdown ∩ local_only = ∅（R4）
// ══════════════════════════════════════════════════════════════

fn check_capability(e: &Entity, loc: &str, errors: &mut Vec<Problem>) {
    let mut overlap: Vec<&String> = e
        .capability
        .pushdown
        .iter()
        .filter(|f| e.capability.local_only.iter().any(|l| l == *f))
        .collect();
    overlap.sort();
    overlap.dedup();
    if !overlap.is_empty() {
        errors.push(Problem::new(
            CODE_CAPABILITY_OVERLAP,
            format!("{loc} 字段 capability"),
            format!(
                "这些字段同时写在 `pushdown` 与 `local_only` 里：{:?}。\n\
                 两者互斥：`pushdown` = 「服务端能筛」、`local_only` = 「只能取回来自己筛」。\
                 同一字段写两处，执行器就无法判断要不要在本地补一遍 —— \
                 按 `pushdown` 读会**静默丢掉本地过滤**，回执里也就不会标 `filtered_locally`。",
                overlap
            ),
        ));
    }
    // 注意：`pushdown` 与 `local_only` **都为空**是合法的（有些 API 一个过滤条件都不支持）；
    // 这里刻意不把「pushdown 空」升级成错误 —— 那会逼作者编一个不存在的下推能力。
    let _ = (&e.capability.pushdown, &e.capability.local_only);
}

// ══════════════════════════════════════════════════════════════
// 检查 3 · relation 两端都在 entities 里（R7）
// ══════════════════════════════════════════════════════════════

fn check_relations(mapping: &Mapping, errors: &mut Vec<Problem>, warnings: &mut Vec<String>) {
    for (i, r) in mapping.relations.iter().enumerate() {
        let loc = format!("relations[{i}]（predicate={}）", r.predicate);
        let from = mapping.entity(&r.from);
        let to = mapping.entity(&r.to);
        if from.is_none() {
            errors.push(Problem::new(
                CODE_RELATION_ENDPOINT,
                format!("{loc} 字段 from"),
                format!(
                    "`from` = `{}` 不在 `entities` 里（R7）。\
                     关系两端必须是**已声明的映射条目**（id 或类名），否则这条边没有起点。",
                    r.from
                ),
            ));
        }
        if to.is_none() {
            errors.push(Problem::new(
                CODE_RELATION_ENDPOINT,
                format!("{loc} 字段 to"),
                format!(
                    "`to` = `{}` 不在 `entities` 里（R7）。\
                     若那是库源的类，v1 不支持跨源 join —— 应标 `cross_source: true` 且不进本文件。",
                    r.to
                ),
            ));
        }
        if r.from == r.to {
            errors.push(Problem::new(
                CODE_RELATION_ENDPOINT,
                format!("{loc} 字段 from/to"),
                format!("自环：`from` 与 `to` 都是 `{}`（R7 明确不许空 join / 自环）。", r.from),
            ));
        }

        // via 的形状检查：join 要两端列，field 要字段名。
        match r.via.as_ref().map(|v| v.kind.as_str()) {
            Some("join") => {
                let v = r.via.as_ref().expect("kind=join");
                if v.local.is_none() || v.remote.is_none() {
                    errors.push(Problem::new(
                        CODE_RELATION_ENDPOINT,
                        format!("{loc} 字段 via"),
                        "`via.kind = \"join\"` 时 `local` 与 `remote` 都必须给（两边各按哪一列拼）。"
                            .to_string(),
                    ));
                }
            }
            Some("field") => {
                let v = r.via.as_ref().expect("kind=field");
                if v.field.is_none() {
                    errors.push(Problem::new(
                        CODE_RELATION_ENDPOINT,
                        format!("{loc} 字段 via"),
                        "`via.kind = \"field\"` 时必须给 `field`（同一行里指过去的那个字段）。"
                            .to_string(),
                    ));
                }
            }
            Some(other) => warnings.push(format!(
                "{loc}：`via.kind` = `{other}`（v1 只认 `join` / `field`，其余形态会被执行器忽略）"
            )),
            None => warnings.push(format!("{loc}：没有 `via` —— 执行器不知道这条边怎么连。")),
        }

        // 规则 7：⛔ 不做跨源 join；R-4：跨源边必须显式标不可遍历。
        if let (Some(f), Some(t)) = (from, to) {
            let cross = f.source != t.source;
            if cross && r.via.as_ref().map(|v| v.kind.as_str()) == Some("join") {
                errors.push(Problem::new(
                    CODE_RELATION_ENDPOINT,
                    format!("{loc} 字段 via"),
                    format!(
                        "跨源 join：`{}`（源 {}）⇄ `{}`（源 {}）。\
                         v1 明确不做跨源 join（设计稿规则 7）—— 没有一层能执行它。",
                        r.from, f.source, r.to, t.source
                    ),
                ));
            }
            if cross && r.traversable {
                errors.push(Problem::new(
                    CODE_RELATION_ENDPOINT,
                    format!("{loc} 字段 traversable"),
                    "跨源边必须 `traversable: false`（demand §三 R-4）：\
                     给了能遍历的样子却查不了，比不给更糟。"
                        .to_string(),
                ));
            }
            if cross && !r.cross_source {
                warnings.push(format!(
                    "{loc}：两端不在同一 source，但没有显式写 `cross_source: true` —— 建议标出来，\
                     下游才知道这条边走不通。"
                ));
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 检查 4 · fields.x.path 在 fixture 里真的存在
// ══════════════════════════════════════════════════════════════

/// `fixture` 的相对路径解析：先按 mapping 目录，再按 CWD。
pub fn resolve_fixture(mapping_dir: &Path, fixture: &str) -> PathBuf {
    let p = Path::new(fixture);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let by_mapping = mapping_dir.join(p);
    if by_mapping.is_file() {
        return by_mapping;
    }
    p.to_path_buf()
}

fn check_fixture(
    e: &Entity,
    loc: &str,
    mapping_dir: &Path,
    check: &mut EntityCheck,
    errors: &mut Vec<Problem>,
    warnings: &mut Vec<String>,
) {
    // REQ-149 · 缺口 D：没有 fixture **不是错误**。
    //
    // 旧口径把「没有夹具」判成 FIXTURE_UNREADABLE，于是为了让 validate 过，
    // 只能从映射反向造夹具 —— 那不是证据，是自证。现在显式区分：
    //   · 没夹具 ⇒ warning + 该实体**不出现在** field_coverage 里 + exit 0；
    //   · `fetch --dry-run-fixture` 才拒绝（`FIXTURE_REQUIRED`）；
    //   · `fetch --allow-network` 不受影响。
    let Some(fixture) = e.fixture.as_deref().filter(|s| !s.trim().is_empty()) else {
        check.field_coverage = None;
        check.fixture_note = Some(
            "没有 `fixture` ⇒ 字段路径**未校准**（⛔ 不是通过）。\
             补一份真实响应，或用 `fetch --allow-network --dump-raw` 采一份。"
                .to_string(),
        );
        warnings.push(format!(
            "{CODE_FIELD_PATH_NOT_CHECKED} · {loc}：契约没有 `fixture` ⇒ 字段路径**没做校验**；\
             该实体**不会**出现在 `field_coverage` 里（⛔ 不显示成 1/1，也不显示成 0/0）。\
             这是「没证据」，不是「通过」—— 补 `fixture` 或改用真调用通路。"
        ));
        return;
    };

    let path_on_disk = resolve_fixture(mapping_dir, fixture);
    let text = match std::fs::read_to_string(&path_on_disk) {
        Ok(t) => t,
        Err(err) => {
            errors.push(Problem::new(
                CODE_FIXTURE_UNREADABLE,
                format!("{loc} 字段 fixture"),
                format!(
                    "读不到 fixture `{}`（按 `--mapping` 所在目录找过，也按 CWD 找过）：{err}。\
                     写了 `fixture` 却读不到 ⇒ 字段路径没法校验；\
                     ⛔ 不会跳过检查算通过。要么把文件放回去，要么删掉 `fixture`\
                     （那样会得到一条 FIELD_PATH_NOT_CHECKED 警告，而不是假通过）。",
                    path_on_disk.display()
                ),
            ));
            return;
        }
    };
    let doc: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(err) => {
            errors.push(Problem::new(
                CODE_FIXTURE_UNREADABLE,
                format!("{loc} 字段 fixture"),
                format!("`{}` 不是合法 JSON：{err}", path_on_disk.display()),
            ));
            return;
        }
    };

    // iterator 必须选得出节点 —— 否则下面那圈「每行都查一遍」会**零次执行**而全绿。
    let rows = path::iterate(&doc, &e.iterator);
    check.fixture_rows = rows.len();
    if rows.is_empty() {
        errors.push(Problem::new(
            CODE_ITERATOR_EMPTY,
            format!("{loc} 字段 iterator"),
            format!(
                "`iterator` = `{}` 在 fixture `{}` 里选不出任何节点。\
                 最常见的原因：`$.` 前缀写漏、把数组名写成了整份响应、或 fixture 与映射不是同一支 API。",
                e.iterator,
                path_on_disk.display()
            ),
        ));
        return;
    }

    // 键：每一行都要有，否则那一行的 IRI 会拼成字面量 `{id}`。
    let mut key_missing = 0usize;
    for row in &rows {
        if !path::exists(row, &e.key_path) {
            key_missing += 1;
        }
    }
    if key_missing > 0 {
        errors.push(Problem::new(
            CODE_KEY_MISSING,
            format!("{loc} 字段 identity.key_path"),
            format!(
                "`{}` 在 fixture 的 {} / {} 行里取不到 —— 这些行拼不出实例 IRI\
                 （结果是 `…/{{id}}` 这样的字面量）。",
                e.key_path,
                key_missing,
                rows.len()
            ),
        ));
    }

    // 字段路径：至少一行有 ⇒ 通过；全部行都有 ⇒ 无提醒；部分行有 ⇒ 报覆盖率。
    let total = rows.len();
    for (name, f) in &e.fields {
        let mut hit = 0usize;
        for row in &rows {
            if path::exists(row, &f.path) {
                hit += 1;
            }
        }
        if let Some(cov) = check.field_coverage.as_mut() {
            cov.insert(name.clone(), format!("{hit}/{total}"));
        }
        if hit == 0 {
            errors.push(Problem::new(
                CODE_FIELD_PATH,
                format!("{loc} 字段 fields.{name}.path"),
                format!(
                    "路径 `{}` 在 fixture 的 {total} 行里**一次都没出现** —— 映射写歪了。\
                     （⛔ 空值 `\"\"` / `null` 算存在；这里是**键根本不在**。）\
                     先把 fixture 里那一行贴出来，再对着响应结构改路径。",
                    f.path
                ),
            ));
        } else if hit < total {
            warnings.push(format!(
                "{loc}：字段 `{name}` 的路径 `{}` 只在 {hit}/{total} 行里存在 —— \
                 缺失的行会**没有这个属性**（R2RML 口径：NULL 不产生三元组），确认这是有意为之。",
                f.path
            ));
        }
    }

    annotate_fixture_provenance(e, loc, &rows, check, warnings);
}

/// REQ-149 · 缺口 D 的第二落点：**这份夹具有没有独立证据力**。
///
/// 工具无法凭文件内容证明夹具是真实响应；它能做两件事：
///
/// 1. 作者显式声明（`fixture_source: declared | derived`）⇒ 照声明说，并说明「不核实」；
/// 2. 未声明时按**结构**做启发式：如果夹具的每一行**只**包含映射引用到的字段
///    （没有任何未映射字段），那它极可能是从映射键反向生成的 —— 此时 `1/1`
///    覆盖是**自证**。真实响应几乎总会带一些没被映射的字段（`RequestId` / 额外属性），
///    所以这个判据是「疑似」而不是「证明」，措辞也必须是「疑似」。
fn annotate_fixture_provenance(
    e: &Entity,
    loc: &str,
    rows: &[&serde_json::Value],
    check: &mut EntityCheck,
    warnings: &mut Vec<String>,
) {
    match e.fixture_source {
        Some(super::FixtureSource::Derived) => {
            check.fixture_note = Some(
                "fixture_source = \"derived\"：作者声明这份夹具由**映射键反向生成** ⇒ \
                 `field_coverage` 全过属于自证，**没有独立证据力**。"
                    .to_string(),
            );
            warnings.push(format!(
                "{CODE_FIXTURE_LOOKS_DERIVED} · {loc}：`fixture_source` = \"derived\" —— \
                 这份夹具是由映射键反向生成的，字段路径 1/1 只是**自证**。\
                 要真正的校准，请用真实响应替换它（`fetch --allow-network --dump-raw`）。"
            ));
        }
        Some(super::FixtureSource::Declared) => {
            check.fixture_note = Some(
                "fixture_source = \"declared\"：作者声明这是独立采集的响应。\
                 ⚠️ 本工具**不核实**这个声明 —— 它只说明「作者认为这份夹具有证据力」。"
                    .to_string(),
            );
        }
        None => {
            if fixture_looks_reverse_generated(e, rows) {
                check.fixture_note = Some(
                    "自动判断：fixture 的每一行**只**包含映射引用到的字段（没有任何未映射字段）\
                     —— 疑似按映射键反向生成，`field_coverage` 全过属于自证，\
                     **没有独立证据力**。若这确实是真实响应，请在契约里写 \
                     `\"fixture_source\": \"declared\"`。"
                        .to_string(),
                );
                warnings.push(format!(
                    "{CODE_FIXTURE_LOOKS_DERIVED} · {loc}：夹具的每一行只含映射引用到的字段\
                     （零个未映射字段）⇒ 疑似由映射键反向生成，字段路径全过是**自证**。\
                     真实响应通常带有未被映射的字段；请用 `--dump-raw` 采一份，或显式声明来源。"
                ));
            }
        }
    }
}

/// 夹具是否**看起来**由映射键反向生成：每一行（对象）的键都落在
/// 「`fields.*.path` 的首段 ∪ `identity.key_path` 的首段」里。
///
/// 只是启发式：真实响应若被作者裁剪到只剩映射字段，也会命中 —— 所以提示语是「疑似」。
fn fixture_looks_reverse_generated(e: &Entity, rows: &[&serde_json::Value]) -> bool {
    use std::collections::BTreeSet;
    let mut mapped: BTreeSet<&str> = BTreeSet::new();
    for f in e.fields.values() {
        if let Some(top) = f.path.split('.').find(|s| !s.is_empty()) {
            mapped.insert(top);
        }
    }
    if let Some(top) = e.key_path.split('.').find(|s| !s.is_empty()) {
        mapped.insert(top);
    }

    let mut saw_object = false;
    for row in rows {
        let Some(obj) = row.as_object() else {
            return false;
        };
        saw_object = true;
        if obj.keys().any(|k| !mapped.contains(k.as_str())) {
            return false; // 有未映射字段 ⇒ 至少不是「只按映射键拼出来的」
        }
    }
    saw_object
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{parse, parse_ontology};

    const TTL: &str = r#"
@prefix : <https://ontok.local/ops360#> .
@prefix ontok: <https://ontok.local/schema/> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

:云资源 a owl:Class ;
    ontok:instanceIri "https://ontok.local/ops360#{class}/{id}" .
"#;

    fn mapping_json(tpl: &str, pushdown: &str, local_only: &str, field_path: &str) -> String {
        format!(
            r#"{{
  "contract_version": 1,
  "project": "ops360",
  "sources": [{{ "id": "s1", "base_url": "https://api.example.com" }}],
  "entities": [{{
    "id": "cvm_instance",
    "class": "云资源",
    "source": "s1",
    "reviewed_by": "张三@2026-10-25",
    "operation": {{ "action": "SomeAction" }},
    "iterator": "$.Response.InstanceSet[*]",
    "identity": {{ "key_path": "InstanceId" }},
    "iri_template": "{tpl}",
    "fields": {{ "name": {{ "path": "{field_path}" }} }},
    "capability": {{ "pushdown": {pushdown}, "local_only": {local_only}, "max_scan_rows": 100 }},
    "freshness": {{ "mode": "realtime" }},
    "fixture": "cvm.json"
  }}]
}}"#
        )
    }

    /// 每个用例**各自一个**临时目录。
    ///
    /// ⚠️ 共用同一个目录会出一道**假红**：`std::fs::write` 是「截断 + 写」，
    /// 而并行的另一个用例正好在这两步之间读文件 ⇒ 读到一个空文件 ⇒
    /// 报的是 `FIXTURE_UNREADABLE`，而被断言的 `FIELD_PATH_MISSING` 不见了。
    /// 实测过：单独跑绿、全量跑红（`cargo test --lib access::` 复现）。
    fn fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ontok-access-validate-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cvm.json"),
            r#"{"Response":{"TotalCount":1,"InstanceSet":[{"InstanceId":"ins-1","InstanceName":"web-1","ExtraUnmapped":"x"}]}}"#,
        )
        .unwrap();
        dir
    }

    fn run(tag: &str, json: &str) -> Report {
        let m = parse(json).expect("测试映射应当解析成功");
        let o = parse_ontology(TTL);
        validate(&m, &o, &fixture_dir(tag))
    }

    #[test]
    fn happy_path_is_ok() {
        let r = run(
            "happy",
            &mapping_json("ontok:云资源/{id}", r#"["zone"]"#, r#"["status"]"#, "InstanceName"),
        );
        assert!(r.ok, "{:?}", r.errors);
        assert_eq!(r.entities[0].fixture_rows, 1);
        assert_eq!(
            r.entities[0]
                .field_coverage
                .as_ref()
                .expect("有 fixture ⇒ field_coverage 必须在")
                .get("name")
                .map(|s| s.as_str()),
            Some("1/1")
        );
    }

    #[test]
    fn check1_iri_template_mismatch() {
        let r = run(
            "iri",
            &mapping_json("ontok:cvm_instance/{id}", "[]", "[]", "InstanceName"),
        );
        assert!(!r.ok);
        let p = r
            .errors
            .iter()
            .find(|p| p.code == CODE_IRI_TEMPLATE)
            .expect("必须报 IRI_TEMPLATE_MISMATCH");
        assert!(p.message.contains("cvm_instance"), "报错要点名两个串：{}", p.message);
    }

    #[test]
    fn check2_capability_overlap() {
        let r = run(
            "overlap",
            &mapping_json("ontok:云资源/{id}", r#"["zone","status"]"#, r#"["status"]"#, "InstanceName"),
        );
        assert!(!r.ok);
        assert!(r.errors.iter().any(|p| p.code == CODE_CAPABILITY_OVERLAP
            && p.message.contains("status")));
    }

    #[test]
    fn check3_relation_endpoint_unknown() {
        let json = mapping_json("ontok:云资源/{id}", "[]", "[]", "InstanceName").replace(
            "\"entities\": [",
            "\"relations\": [{ \"predicate\": \"p\", \"from\": \"cvm_instance\", \"to\": \"不存在\" }],\n  \"entities\": [",
        );
        let r = run("relation", &json);
        assert!(!r.ok);
        assert!(r
            .errors
            .iter()
            .any(|p| p.code == CODE_RELATION_ENDPOINT && p.message.contains("不存在")));
    }

    #[test]
    fn check4_field_path_missing() {
        let r = run(
            "field",
            &mapping_json("ontok:云资源/{id}", "[]", "[]", "Placement.Zone"),
        );
        assert!(!r.ok);
        let p = r
            .errors
            .iter()
            .find(|p| p.code == CODE_FIELD_PATH)
            .unwrap_or_else(|| panic!("必须报 FIELD_PATH_MISSING，实际：{:?}", r.errors));
        assert!(p.location.contains("fields.name.path"), "{}", p.location);
    }

    #[test]
    fn class_missing_from_ontology_is_an_error_not_a_skip() {
        let json = mapping_json("ontok:云资源/{id}", "[]", "[]", "InstanceName")
            .replace("\"class\": \"云资源\"", "\"class\": \"不存在的类\"");
        let r = run("class", &json);
        assert!(r.errors.iter().any(|p| p.code == CODE_CLASS_NOT_IN_ONTOLOGY));
    }

    #[test]
    fn empty_iterator_is_an_error_not_a_vacuous_pass() {
        let json = mapping_json("ontok:云资源/{id}", "[]", "[]", "InstanceName")
            .replace("$.Response.InstanceSet[*]", "$.Response.Nope[*]");
        let r = run("iter", &json);
        assert!(r.errors.iter().any(|p| p.code == CODE_ITERATOR_EMPTY));
        assert!(!r.ok, "iterator 选不出节点时绝不许判通过");
    }

    // ══════════════════════════════════════════════════════════
    // REQ-149 · 缺口 D：没夹具 ≠ 不通过，但也不是「通过」
    // ══════════════════════════════════════════════════════════

    /// 删掉 `fixture` ⇒ `validate` **exit 0**（`ok == true`）+ 一条 `FIELD_PATH_NOT_CHECKED`
    /// + 该实体**没有** `field_coverage`（序列化后键都不出现）。
    ///
    /// 变异：把下面 `"fixture": null` 改回 `"fixture": "cvm.json"` ⇒
    /// `field_coverage` 又出现、警告消失 —— 这条断言会红。
    #[test]
    fn missing_fixture_is_a_named_warning_not_an_error_and_no_coverage() {
        let json = mapping_json("ontok:云资源/{id}", "[]", "[]", "InstanceName")
            .replace("\"fixture\": \"cvm.json\"", "\"fixture\": null");
        let r = run("nofixture", &json);
        assert!(r.ok, "没有 fixture 不许判失败：{:?}", r.errors);
        let w = r
            .warnings
            .iter()
            .find(|w| w.contains(CODE_FIELD_PATH_NOT_CHECKED))
            .unwrap_or_else(|| panic!("必须有一条 FIELD_PATH_NOT_CHECKED：{:?}", r.warnings));
        assert!(w.contains("cvm_instance"), "警告必须点名实体：{w}");
        assert!(w.contains("没做校验"), "要说清是「没做」，不是「通过」：{w}");
        assert!(r.entities[0].field_coverage.is_none(), "没有 fixture 时 field_coverage 必须是 None");

        // 序列化层面也要「不出现」——⛔ 不许变成 `{}` 或 `1/1`。
        let text = serde_json::to_string(&r).unwrap();
        let entity_json = &serde_json::from_str::<serde_json::Value>(&text).unwrap()["entities"][0];
        assert!(
            entity_json.get("field_coverage").is_none(),
            "没有 fixture 的实体不许在输出里出现 field_coverage 键：{entity_json}"
        );
        assert!(text.contains("fixture_note"), "要能看出「这份夹具没有独立证据力」：{text}");
    }

    /// 显式声明 `fixture_source: derived` ⇒ 明确告知「自证、无独立证据力」。
    #[test]
    fn declared_derived_fixture_is_called_out() {
        let json = mapping_json("ontok:云资源/{id}", "[]", "[]", "InstanceName").replace(
            "\"fixture\": \"cvm.json\"",
            "\"fixture\": \"cvm.json\", \"fixture_source\": \"derived\"",
        );
        let r = run("derived", &json);
        assert!(r.ok, "{:?}", r.errors);
        assert!(
            r.warnings.iter().any(|w| w.contains(CODE_FIXTURE_LOOKS_DERIVED)),
            "声明 derived 必须有一条醒目警告：{:?}",
            r.warnings
        );
        let note = r.entities[0].fixture_note.as_deref().unwrap_or_default();
        assert!(note.contains("没有独立证据力"), "{note}");
    }

    /// 未声明来源、但夹具每行**只**含映射引用到的字段 ⇒ 启发式提醒「疑似反向生成」。
    #[test]
    fn reverse_generated_looking_fixture_gets_a_heuristic_note() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-access-validate-reverse-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("cvm.json"),
            r#"{"Response":{"TotalCount":1,"InstanceSet":[{"InstanceId":"ins-1","InstanceName":"web-1"}]}}"#,
        )
        .unwrap();
        let m = parse(&mapping_json("ontok:云资源/{id}", "[]", "[]", "InstanceName")).unwrap();
        let o = parse_ontology(TTL);
        let r = validate(&m, &o, &dir);
        assert!(r.ok, "{:?}", r.errors);
        assert!(
            r.entities[0]
                .fixture_note
                .as_deref()
                .unwrap_or_default()
                .contains("疑似"),
            "要能看出可疑：{:?}",
            r.entities[0].fixture_note
        );
        assert!(r.warnings.iter().any(|w| w.contains(CODE_FIXTURE_LOOKS_DERIVED)));
    }
}
