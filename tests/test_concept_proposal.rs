//! REQ-010 · 概念组的**提案通道**。
//!
//! 核心断言：`add_concept_group` 与实体类型条目**同权** ——
//! 两轨都明确通过才可 apply，成员不可用就拦下，apply 之后
//! 实体表逐字节不动。

mod common;
use common::test_pool;
use ontok::db::apply::PgApplier;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{
    parse_kind, NewItem, NewProposal, PgProposalStore,
};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::apply::{apply, ApplyError, ApplyInput};
use ontok::domain::concept::ConceptGroup;
use ontok::domain::review::{summarize, Decision, ItemKind, ItemView, Track};
use ontok::domain::snapshot::{
    ActionType, EntityType, ExtractedBy, Function, LinkType, Property, Snapshot, WriteBinding,
};
use serde_json::json;
use std::collections::BTreeSet;
use uuid::Uuid;

// ── 纯逻辑 ────────────────────────────────────────────────

fn item(kind: ItemKind, target: &str, payload: serde_json::Value) -> ItemView {
    ItemView {
        id: format!("{kind:?}-{target}"),
        kind,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload,
    }
}

fn cg(key: &str, members: &[&str]) -> ItemView {
    item(
        ItemKind::AddConceptGroup,
        key,
        json!({ "key": key, "display_name": key, "members": members }),
    )
}

fn keys(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// 提案通道①：没两轨都通过，概念组一样不可用。
#[test]
fn test_concept_group_needs_both_tracks() {
    let baseline = keys(&["t067", "t066"]);
    let items = vec![cg("package", &["t067", "t066"])];

    let mut one_track = items.clone();
    one_track[0].model_status = Decision::Pending;
    let st = summarize(&one_track, &baseline);
    assert!(!st.passed, "只有业务轨通过 ⇒ 整体不通过（与实体类型同一把尺）");

    let st = summarize(&items, &baseline);
    assert!(st.passed, "两轨都通过 ⇒ 可 apply");
    assert_eq!(st.applicable, vec![items[0].id.clone()]);
}

/// 提案通道②：任一轨显式拒绝 → 否决（不是「另一轨能救」）。
#[test]
fn test_concept_group_vetoed_by_either_track() {
    let baseline = keys(&["t067"]);
    let mut it = cg("package", &["t067"]);
    it.biz_status = Decision::Rejected;
    let st = summarize(&[it], &baseline);
    assert!(!st.passed);
    assert_eq!(st.verdicts[0].state, ontok::domain::review::EntryState::Vetoed);
}

/// 提案通道③：成员还没被接受 → 拦下，且**理由要指名道姓**说缺谁。
#[test]
fn test_concept_group_blocked_when_member_not_accepted() {
    let baseline = keys(&["t067"]); // t066 还没进本体
    let st = summarize(&[cg("package", &["t067", "t066"])], &baseline);
    assert!(!st.passed);
    assert_eq!(st.blocked.len(), 1);
    let reason = st.blocked[0].reason.clone().unwrap_or_default();
    assert!(reason.contains("t066"), "理由必须指名缺哪个成员：{reason}");
    assert!(st.applicable.is_empty(), "被拦下的条目不得进 applicable");
}

/// 提案通道④：没声明成员 = 退化输入，拦下（不是「空组也算过」）。
#[test]
fn test_concept_group_without_members_is_blocked() {
    let st = summarize(&[cg("empty", &[])], &keys(&["x"]));
    assert!(!st.passed);
    assert!(st.blocked[0].reason.clone().unwrap_or_default().contains("没有声明任何成员"));
}

/// 同一提案内先加实体、再让概念组引用它 —— 顺序无关，两边都过才可 apply。
#[test]
fn test_concept_group_can_reference_entity_added_in_same_proposal() {
    let baseline = keys(&[]);
    let ent = item(
        ItemKind::AddEntityType,
        "套餐模板",
        json!({ "key": "t067", "display_name": "套餐模板", "properties": [] }),
    );
    let group = cg("package", &["t067"]);
    let st = summarize(&[ent, group], &baseline);
    assert!(st.passed, "概念组可以引用本提案内刚被接受的新实体");
    assert_eq!(st.applicable.len(), 2);
}

/// 一条**有代表性的**属性。
///
/// 刻意把 `Property` 的五个字段都填出取值（中文业务名、源列名、
/// 可空性、note、sample），因为红线的比对是整栏 serde 序列化 ——
/// 只填 `name` 和 `column` 的话，改 `nullable` / `note` 照样全绿，
/// 那就又是一个**属性层盲区**换了个字段名。
fn prop(
    name: &str,
    column: &str,
    nullable: bool,
    note: Option<&str>,
    sample: Option<&str>,
) -> Property {
    Property {
        name: name.into(),
        column: column.into(),
        nullable,
        note: note.map(String::from),
        sample: sample.map(String::from),
    }
}

/// 红线 fixture。
///
/// **属性、关系、动作、函数都必须有非空取值** —— 否则「逐字节相等」
/// 比的是 `[] == []`，空数组相等恒成立，等于没比。
/// 这正是本文件上一轮栽的坑：所有实体的 `properties` 都是空的，
/// 于是属性层根本没进过比对范围。
fn base_snapshot() -> Snapshot {
    let mut s = Snapshot::new(Uuid::nil(), 1);
    s.entity_types = vec![
        EntityType {
            key: "t067".into(),
            display_name: "套餐模板".into(),
            description: Some("客户可购买的一档套餐配置".into()),
            aliases: vec!["模板包".into()],
            parent: None,
            disjoint_with: vec!["t066".into()],
            properties: vec![
                prop("模板编号", "tmpl_id", false, Some("业务主键，人读用"), Some("T-0001")),
                prop("模板名称", "tmpl_name", false, None, Some("家庭套餐")),
                prop("适用客户等级", "apply_level", true, Some("为空表示不限等级"), None),
                // **业务名留空**的一列 —— 保住 S-2 那条「未知列不许瞎翻译」
                // 的兜底路径（人话化按列名来）
                prop("", "legacy_tmpl_no", true, Some("历史编号，已废弃"), None),
            ],
            evidence: vec![],
        },
        EntityType {
            key: "t066".into(),
            display_name: "套餐产品".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![
                prop("产品编号", "prod_id", false, None, Some("P-0001")),
                prop("计量单位", "unit", true, Some("该产品的计价单位"), None),
            ],
            evidence: vec![],
        },
    ];
    // 关系类型：原来 base 快照里 link_types **恒空**，
    // 所以「从不比较 link_types」这件事根本没被任何断言看见过。
    s.link_types = vec![LinkType {
        key: "template_has_product".into(),
        from_type: "t067".into(),
        to_type: "t066".into(),
        cardinality: "1:N".into(),
        semantic: "has".into(),
        evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
    }];
    // 占位栏（切片 1 生产代码恒空）。这里给非空取值，是为了让
    // 「apply 不得动它们」这句话**可证伪** —— 它们也是快照的一部分，
    // `apply` 的 `RemoveEntityType` 分支就会改 `action_types`。
    s.action_types = vec![ActionType {
        key: "publish_template".into(),
        target: "t067".into(),
        parameters: json!({ "effective_from": "date" }),
        preconditions: json!({ "status": "draft" }),
        effects: vec![WriteBinding {
            system: "源库".into(),
            table: "t067".into(),
            field: "status".into(),
            key_expr: json!({ "tmpl_id": "$1" }),
            value_map: [("published".to_string(), "ON".to_string())].into_iter().collect(),
            supported_values: vec![],
            conflict_field: Some("updated_at".into()),
            evidence: vec![],
        }],
        permission: json!({ "role": "运营" }),
        audit_policy: json!({ "retain_days": 365 }),
        evidence: vec![],
    }];
    s.functions = vec![Function {
        key: "template_coverage".into(),
        signature: "template_coverage(t067) -> i32".into(),
        allowed_types: vec!["t067".into()],
        evidence: vec![],
    }];
    s
}

/// 红线比对的投影 —— 快照的**本体部分**。
///
/// 逐栏交代摘掉了什么、为什么，不笼统写「排除无关字段」：
///
/// | 摘掉 | 理由 |
/// |---|---|
/// | `version_id` | `apply` 生成新的随机 UUID（`apply.rs`：「调用方会覆盖成新版本 id」）。它是版本身份，不是本体内容 |
/// | `seq` | 版本序号 +1，是版本序不是本体内容。同一个 `base` 推出，两侧恒等，比它没有信息量 |
/// | `extracted_by` | **本来就该变**：ADR-001 规定新版本原样取提案的值（提案没带则记 `None`），不继承 base。排除它是必须的 —— 但**排除 ≠ 不管**，`extracted_by` 由 [`test_extracted_by_follows_proposal_not_base`] 单独钉死 |
/// | `concept_groups` | **这就是被检的那一栏本身**。比的是「除它以外什么都没动」，所以它必须出框 |
///
/// 留下的四栏**全部进比对**，其中最要紧的是 `entity_types`：
/// 它是整栏 serde 序列化，`properties` 的 `name` / `column` / `nullable` /
/// `note` / `sample`、`description` / `aliases` / `parent` /
/// `disjoint_with` / `evidence` 一个都不落。
/// `link_types` / `action_types` / `functions` 同样进框 ——
/// 上一轮只比 `entity_types`，等于把这两栏放在门外，
/// 注入 `out.link_types.clear()` 时三条测试全绿。
fn snapshot_body(s: &Snapshot) -> String {
    let mut v = serde_json::to_value(s).expect("快照必须可序列化");
    let o = v.as_object_mut().expect("快照序列化后是对象");
    o.remove("version_id");
    o.remove("seq");
    o.remove("extracted_by");
    o.remove("concept_groups");
    serde_json::to_string(&v).expect("投影必须可序列化")
}

/// `base_snapshot()` 里那份带版本号的抽取来源（ADR-001 的对照组）。
fn extractor(agent: &str, version: &str, round: i32) -> ExtractedBy {
    ExtractedBy {
        agent: agent.into(),
        version: version.into(),
        round,
        failed_groups: vec![],
        source: None,
    }
}

/// 红线（原始版本，保留）：apply 概念组之后，实体行逐字节不变。
///
/// ⚠️ **这条断言只覆盖 `entity_types` 这一栏**，而且上一轮 fixture 里
/// 实体没有属性，于是它连属性层都够不着。覆盖面已由下面三条补齐 ——
/// 本条按原样保留，不放宽。
#[test]
fn test_apply_concept_group_leaves_entities_byte_identical() {
    let base = base_snapshot();
    let baseline = keys(&["t067", "t066"]);
    let items = vec![cg("package", &["t067", "t066"])];

    let out = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .expect("概念组应可 apply");

    assert_eq!(out.concept_groups.len(), 1);
    assert_eq!(out.concept_groups[0].key, "package");
    assert_eq!(out.concept_groups[0].members.len(), 2);
    assert_eq!(
        serde_json::to_string(&out.entity_types).unwrap(),
        serde_json::to_string(&base.entity_types).unwrap(),
        "实体行必须逐字节一致 —— 概念组是展示层声明"
    );
    assert_eq!(out.entity_types.len(), base.entity_types.len());
}

/// **红线（补齐覆盖面）**：带概念组 apply 与**不做归并** apply，
/// 除 `concept_groups` 之外必须**逐字节一致**。
///
/// ⚠️ **这条测试的判别力有边界，别把它当成唯一兜底**（REQ-011 核实）：
/// 它比的是「两次 apply 的结果之差」。**两次 apply 同时犯同一个错误时恒成立**
/// —— 比如两次都把 `properties` 清空，它照样绿。
/// 真正兜底的是对偶的那条 [`test_concept_group_only_appends_and_touches_nothing_else`]：
/// 它拿 apply 的结果和 **base** 比，「apply 动没动 base」它答得了。
/// 两条必须**同时**留着；要删只能两条一起删。
///
/// 两次 apply 带**同一批非概念组条目**（加一个属性 + 改一个展示名），
/// 于是两侧都真的做了同一套改动再比 —— 不是拿「有变」和「空转」比。
#[test]
fn test_concept_group_changes_nothing_besides_concept_groups() {
    let base = base_snapshot();
    let baseline = keys(&["t067", "t066"]);
    let by = extractor("extractor", "1.4.0", 2);

    let common = vec![
        item(
            ItemKind::AddProperty,
            "模板说明",
            json!({ "parent_key": "t067", "name": "模板说明",
                    "column": "tmpl_desc", "nullable": true, "note": "归并前后都得在" }),
        ),
        item(
            ItemKind::ModifyEntityType,
            "套餐模板",
            json!({ "key": "t067", "display_name": "套餐模板（旧称模板包）" }),
        ),
    ];

    let mut with_cg = common.clone();
    with_cg.push(cg("package", &["t067", "t066"]));
    let without_cg = common.clone();

    let a = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: Some(by.clone()),
        items: &with_cg,
        baseline_types: &baseline,
    })
    .expect("带概念组应可 apply");
    let b = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: Some(by.clone()),
        items: &without_cg,
        baseline_types: &baseline,
    })
    .expect("不做归并应可 apply");

    // 前提也要先钉住，否则上面那句比对可能在比两坨一样的东西
    assert_eq!(a.concept_groups.len(), 1, "前提：带概念组那次确实归并了");
    assert!(b.concept_groups.is_empty(), "前提：对照组没有归并");
    assert_eq!(
        a.entity_types.iter().map(|e| e.properties.len()).sum::<usize>(),
        7,
        "前提：属性确实进了快照（4 + 2 + 本次新增的 1），否则比的是空数组"
    );
    assert_eq!(a.link_types.len(), 1, "前提：关系类型非空，否则比的是 []");

    assert_eq!(
        snapshot_body(&a),
        snapshot_body(&b),
        "红线：除 concept_groups 外，快照的每一栏都必须与不做归并时逐字节一致\n\
         覆盖 entity_types（含 properties 的 name/column/nullable/note/sample）、\
         link_types、action_types、functions"
    );
}

/// 红线的另一面：**只**往 `concept_groups` 追加，别的一动没动。
///
/// 上面那条是「归并 vs 不归并」，这条是「apply 之后 vs base」——
/// 两次 apply 之间可能同时成立的错误（比如两次都把关系清空了）
/// 在上面那条里看不出来，这里能看出来。
#[test]
fn test_concept_group_only_appends_and_touches_nothing_else() {
    let base = base_snapshot();
    let baseline = keys(&["t067", "t066"]);
    let items = vec![cg("package", &["t067", "t066"])];

    let out = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .expect("概念组应可 apply");

    assert_eq!(out.concept_groups.len(), 1, "前提：确实追加了");
    assert_eq!(
        snapshot_body(&out),
        snapshot_body(&base),
        "只许往 concept_groups 追加 —— entity_types / link_types / action_types / \
         functions 一律逐字节不动"
    );
    // 逐栏点名：失败时要能一眼看出是哪一栏被动过
    assert_eq!(
        serde_json::to_string(&out.link_types).unwrap(),
        serde_json::to_string(&base.link_types).unwrap(),
        "link_types 必须逐字节不动（上一轮**从不比较**这一栏）"
    );
    for (i, (o, e)) in out.entity_types.iter().zip(base.entity_types.iter()).enumerate() {
        assert_eq!(
            serde_json::to_string(&o.properties).unwrap(),
            serde_json::to_string(&e.properties).unwrap(),
            "实体 {} 的属性必须逐字节不动（含 name/column/nullable/note/sample）",
            e.key
        );
        assert_eq!(o.properties.len(), e.properties.len(), "实体 {} 的属性条数", e.key);
        let _ = i;
    }
}

/// `extracted_by` 是**本来就该变**的字段 —— 排除在红线比对之外，
/// 但排除不等于不管：ADR-001 的语义在这里**单独钉死**。
///
/// 新版本原样取**提案**的值，**不继承 base、不推断**（ADR-001）。
/// 顺带把「本来就会变的版本身份」也一并钉住，替代那句
/// 「它们当然会变所以不管」——会变也得变在对的地方。
#[test]
fn test_extracted_by_follows_proposal_not_base() {
    let mut base = base_snapshot();
    base.extracted_by = Some(extractor("old_agent", "0.9.0", 1));
    let baseline = keys(&["t067", "t066"]);
    let by = extractor("new_agent", "1.4.0", 2);
    let items = vec![cg("package", &["t067", "t066"])];

    let out = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: Some(by.clone()),
        items: &items,
        baseline_types: &baseline,
    })
    .expect("概念组应可 apply");

    assert_eq!(
        out.extracted_by.as_ref(),
        Some(&by),
        "ADR-001：新版本原样取提案的值 —— 继承 base 就是 §8.4 要防的「agent 偷偷换版本」"
    );
    assert_ne!(out.extracted_by, base.extracted_by, "前提：两侧确实不同，否则这条是空转");
    // apply 自己生成随机 version_id，调用方随后覆盖 —— 那是「本来就该变」的
    // 那一栏，必须变在随机 id 上，不能悄悄沿用 base 的
    assert_ne!(
        out.version_id, base.version_id,
        "apply 自己生成随机 version_id，调用方随后覆盖成新版本 id"
    );
    assert_eq!(out.seq, base.seq + 1, "版本序号 +1");
    // 除这几栏 + 概念组之外，红线仍然成立
    assert_eq!(snapshot_body(&out), snapshot_body(&base));
}

/// 提案没带 `extracted_by` ⇒ 记 `None`（人工修订），**不继承 base**。
#[test]
fn test_concept_group_does_not_inherit_extracted_by_from_base() {
    let mut base = base_snapshot();
    base.extracted_by = Some(extractor("old_agent", "0.9.0", 1));
    let baseline = keys(&["t067", "t066"]);
    let items = vec![cg("package", &["t067", "t066"])];

    let out = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .expect("概念组应可 apply");

    assert_eq!(
        out.extracted_by, None,
        "人工修订就是 None —— 继承 base 的抽取来源会让「谁改的」查无对证"
    );
    assert_eq!(snapshot_body(&out), snapshot_body(&base), "红线不受影响");
}

/// 最后的闸：跨条目的重叠，单条审核看不出来。
#[test]
fn test_apply_rejects_two_groups_claiming_same_member() {
    let base = base_snapshot();
    let baseline = keys(&["t067", "t066"]);
    // 两条都单看合法（成员都在本提案的可用集合里），合起来才矛盾
    let items = vec![cg("package", &["t067"]), cg("pack", &["t067"])];
    let st = summarize(&items, &baseline);
    assert!(st.passed, "逐条看都合法 —— 正是需要最后一道闸的原因");

    let err = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .unwrap_err();
    assert!(
        matches!(&err, ApplyError::BadConceptGroup(m) if m.contains("同时归属")),
        "重叠必须在 apply 期被拒，got={err:?}"
    );
}

/// 嵌套归并（A∪B、B∪C）在**正常路径**上审核期就拦下了：
/// 「A∪B」这条声明的成员 `package` 是一个**组 key**，不是实体，
/// 于是它在可用集合里查不到 —— 与悬空成员同一条通路。
/// 理由必须让人一眼看出「你把另一个概念当成员了」。
#[test]
fn test_nested_group_blocked_at_review_time() {
    let mut base = base_snapshot();
    base.entity_types.push(EntityType {
        key: "pack".into(),
        display_name: "打包".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![prop("打包编号", "pack_id", false, None, None)],
        evidence: vec![],
    });
    let baseline = keys(&["t067", "t066", "pack"]);
    let items = vec![cg("package", &["t067"]), cg("pack", &["package"])];
    let st = summarize(&items, &baseline);
    assert!(!st.passed, "把概念当成员，审核期必须拦");
    let reason = st.blocked[0].reason.clone().unwrap_or_default();
    assert!(reason.contains("package"), "理由要指名那个当成员的组：{reason}");
    let _ = base;
}

/// apply 期的嵌套闸不是死代码：当**组 key 恰好也是一个实体 key**时
/// （命名撞车，key 命名空间是共享的），审核期查不出它是「概念套概念」，
/// 只有把全部条目应用完、对照结果快照的实体集合才判得出来。
#[test]
fn test_apply_rejects_nested_group_when_group_key_is_also_an_entity() {
    let mut base = base_snapshot();
    base.entity_types.push(EntityType {
        key: "pack".into(),
        display_name: "打包".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![prop("打包编号", "pack_id", false, None, None)],
        evidence: vec![],
    });
    // 库里同时存在 key=`package` 的**实体** —— 于是「pack 的成员是 package」
    // 在审核期看是合法的（package 确实可用），但它同时也是概念组的 key。
    base.entity_types.push(EntityType {
        key: "package".into(),
        display_name: "打包方案".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![prop("方案编号", "pkg_no", false, Some("与 group key 同名"), None)],
        evidence: vec![],
    });
    let baseline = keys(&["t067", "t066", "pack", "package"]);
    let items = vec![cg("package", &["t067"]), cg("pack", &["package"])];
    let st = summarize(&items, &baseline);
    assert!(st.passed, "命名撞车时审核期看不出来 —— 正是需要最后一道闸的原因");

    let err = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .unwrap_err();
    assert!(
        matches!(&err, ApplyError::BadConceptGroup(m) if m.contains("嵌套归并")),
        "嵌套归并必须在 apply 期被拒，got={err:?}"
    );
}

/// 同一概念 key 声明两次 = 重复声明。
#[test]
fn test_apply_rejects_duplicate_group_key() {
    let base = base_snapshot();
    let baseline = keys(&["t067", "t066"]);
    let items = vec![
        cg("package", &["t067"]),
        cg("package", &["t066"]),
    ];
    let err = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .unwrap_err();
    assert!(matches!(err, ApplyError::KeyConflict(_)), "got={err:?}");
}

/// 纵深防御：即使调用方给的 baseline 与快照对不上（正常路径不会发生），
/// 最后一道闸仍要抓住悬空成员。
#[test]
fn test_apply_catches_dangling_member_even_with_lying_baseline() {
    let base = base_snapshot();
    // baseline 里谎称有 package_ghost，但 base 快照里没有
    let baseline = keys(&["t067", "t066", "package_ghost"]);
    let items = vec![cg("package", &["t067", "package_ghost"])];
    let err = apply(ApplyInput {
        base: &base,
        proposal_extracted_by: None,
        items: &items,
        baseline_types: &baseline,
    })
    .unwrap_err();
    assert!(
        matches!(&err, ApplyError::BadConceptGroup(m) if m.contains("不在本项目实体集合内")),
        "got={err:?}"
    );
}

/// payload 缺 members ⇒ 显式失败，不静默造一个空组。
#[test]
fn test_apply_rejects_payload_without_members() {
    let base = base_snapshot();
    let baseline = keys(&["t067"]);
    // summarize 会先拦（空成员），所以这里直接验证 group_from_payload 的契约
    assert!(ontok::domain::concept::group_from_payload("x", &json!({})).is_none());
    let g = ontok::domain::concept::group_from_payload("x", &json!({ "key": "x", "members": ["a"] })).unwrap();
    assert_eq!(g.members, vec!["a".to_string()]);
    let _ = (base, baseline);
}

// ── 存储与端到端（DB）────────────────────────────────────

#[tokio::test]
async fn test_add_concept_group_kind_survives_storage_roundtrip() {
    let pool = test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let gs = PgProposalStore::new(pool.clone());
    ps.migrate().await.unwrap();
    let p = common::fresh_project(&ps, "cgkind").await;
    let v = ps.new_version(p.id).await.unwrap();

    let payload = json!({
        "key": "package",
        "display_name": "套餐",
        "members": ["t067", "t066"],
        "scope_hint": "给客户看的三类套餐配置",
    });
    let row = gs
        .create(
            p.id,
            NewProposal { base_version_id: v.id, round: 1, extracted_by: None },
            &[NewItem { kind: ItemKind::AddConceptGroup, target_key: "套餐".into(), payload }],
        )
        .await
        .unwrap();

    let rows = gs.items(row.id).await.unwrap();
    assert_eq!(rows[0].kind, "add_concept_group");
    assert_eq!(parse_kind(&rows[0].kind), ItemKind::AddConceptGroup);
    assert_eq!(ItemKind::AddConceptGroup.as_str(), "add_concept_group");
    let views = PgProposalStore::to_item_views(&rows);
    assert_eq!(views[0].type_key(), "package", "type_key 必须取 payload.key（不是展示名）");
}

/// 端到端：概念声明 → 提案 → 双轨审核 → apply → 新版本带概念组，
/// 且**新版本的实体行与 base 逐字节一致**。
#[tokio::test]
async fn test_end_to_end_concept_group_via_dual_track_review() {
    let pool = test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let gs = PgProposalStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    ps.migrate().await.unwrap();

    let p = common::fresh_project(&ps, "cge2e").await;
    let v1 = ps.new_version(p.id).await.unwrap();

    // base 版本先放实体 + 属性 + 关系 + 动作 + 函数（fixture 全非空，
    // 否则下面那些比对等于在比空数组）
    let mut base = base_snapshot();
    base.version_id = v1.id;
    base.seq = 1;
    store.save_snapshot(&base).await.unwrap();

    // 概念声明 → 提案条目
    let decl_groups = vec![ConceptGroup {
        key: "package".into(),
        display_name: "套餐".into(),
        members: vec!["t067".into(), "t066".into()],
        scope_hint: Some("给客户看的两类套餐配置".into()),
        extracted_by: None,
        low_confidence: None, // REQ-028 · F4：默认「没声明过」
        evidence: vec![],
    }];
    let items_json: Vec<serde_json::Value> = decl_groups
        .iter()
        .map(|g| json!({ "kind": "add_concept_group", "target": g.display_name, "payload": g }))
        .collect();
    let new_items: Vec<NewItem> = items_json
        .iter()
        .enumerate()
        .map(|(i, it)| NewItem::from_json(it, i).unwrap())
        .collect();

    let prop = gs
        .create(
            p.id,
            NewProposal { base_version_id: v1.id, round: 2, extracted_by: None },
            &new_items,
        )
        .await
        .unwrap();

    // 未审核 ⇒ apply 必须被拒
    let applier = PgApplier::new(gs.pool().clone());
    assert!(
        applier.apply_proposal(prop.id).await.is_err(),
        "没走双轨审核的概念组不得 apply"
    );

    // 双轨都通过
    let items = gs.items(prop.id).await.unwrap();
    for it in &items {
        ontok::db::apply::accept_both_tracks(&gs, it.id).await.unwrap();
    }

    let v2_id = applier.apply_proposal(prop.id).await.unwrap();

    let snap = store.get_snapshot(v2_id).await.unwrap().expect("新版本应有快照");
    assert_eq!(snap.concept_groups.len(), 1, "概念组应随版本落库");
    assert_eq!(snap.concept_groups[0].key, "package");
    assert_eq!(snap.concept_groups[0].scope_hint.as_deref(), Some("给客户看的两类套餐配置"));

    // 红线：新版本的实体行与 base 逐字节一致
    assert_eq!(
        serde_json::to_string(&snap.entity_types).unwrap(),
        serde_json::to_string(&base.entity_types).unwrap(),
        "概念组不得改变实体表的行数与 key"
    );

    // 红线（补齐覆盖面）：上一轮这条断言**只比 entity_types**，
    // 关系类型 / 动作 / 函数三栏从没被比过，而它们同样随版本落库。
    assert_eq!(
        serde_json::to_string(&snap.link_types).unwrap(),
        serde_json::to_string(&base.link_types).unwrap(),
        "关系类型必须逐字节不变"
    );
    // ✅ REQ-011 **已修复**：这五栏现在是**一个不少**地落库的。
    //
    // 原先 `db::apply::write_snapshot_in` 开头把 5 张表全 DELETE 了，
    // 函数体却只 INSERT `object_types` / `link_types` / `concept_groups`
    // —— `action_types` 与 `functions` **删了不写**。领域层
    // （`domain::apply` 整体继承 base、`RemoveEntityType` 里
    // `retain(|a| a.target != key)`）算得再对也没机会生效，
    // 一次 apply 就把两栏静默清空，事务内不报错、不告警。
    // 上一轮这里断言的是 `0`（把 bug 钉成了期望值），
    // 现在断言真实语义：**base 有 N 条 → 新版本仍是 N 条，且逐字节相同**。
    assert_eq!(base.action_types.len(), 1, "前提：base 快照确实有动作类型");
    assert_eq!(base.functions.len(), 1, "前提：base 快照确实有函数");
    assert_eq!(
        snap.action_types.len(),
        base.action_types.len(),
        "apply 的落库路径必须把 action_types 写回新版本（REQ-011 修的就是这条）"
    );
    assert_eq!(
        serde_json::to_string(&snap.action_types).unwrap(),
        serde_json::to_string(&base.action_types).unwrap(),
        "动作类型必须逐字节不变：key / target / parameters / preconditions / \
         effects / permission / audit_policy / evidence 一栏都不许少"
    );
    assert_eq!(
        snap.functions.len(),
        base.functions.len(),
        "apply 的落库路径必须把 functions 写回新版本（REQ-011 修的就是这条）"
    );
    assert_eq!(
        serde_json::to_string(&snap.functions).unwrap(),
        serde_json::to_string(&base.functions).unwrap(),
        "函数必须逐字节不变：key / signature / allowed_types / evidence 一栏都不许少"
    );
    // 属性层要真过了库（存的是 jsonb，读回来还要逐字节相同）
    assert_eq!(
        serde_json::to_string(&snap.entity_types[0].properties).unwrap(),
        serde_json::to_string(&base.entity_types[0].properties).unwrap(),
        "属性必须穿过 jsonb 往返后逐字节不变"
    );
    assert_eq!(
        snap.entity_types[0].properties.len(),
        base.entity_types[0].properties.len(),
        "属性条数不变"
    );
    assert_eq!(
        snap.entity_types.iter().map(|e| e.properties.len()).sum::<usize>(),
        base.entity_types.iter().map(|e| e.properties.len()).sum::<usize>(),
        "全部实体的属性条数总和不变"
    );
}

/// 审核状态：概念组条目也能被单轨判定（`set_decision` 走同一条路）。
#[tokio::test]
async fn test_concept_group_item_accepts_per_track_decision() {
    let pool = test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let gs = PgProposalStore::new(pool.clone());
    ps.migrate().await.unwrap();
    let p = common::fresh_project(&ps, "cgdecide").await;
    let v = ps.new_version(p.id).await.unwrap();

    let prop = gs
        .create(
            p.id,
            NewProposal { base_version_id: v.id, round: 1, extracted_by: None },
            &[NewItem {
                kind: ItemKind::AddConceptGroup,
                target_key: "套餐".into(),
                payload: json!({ "key": "package", "display_name": "套餐", "members": ["t067"] }),
            }],
        )
        .await
        .unwrap();
    let it = gs.items(prop.id).await.unwrap().remove(0);

    gs.set_decision(it.id, Track::Biz, Decision::Accepted).await.unwrap();
    let after = gs.item_by_seq(prop.id, it.seq).await.unwrap().unwrap();
    assert_eq!(after.biz_status, "accepted");
    assert_eq!(after.model_status, "pending", "只判一轨 ⇒ 另一轨仍是 pending");
    assert_eq!(parse_kind(&after.kind), ItemKind::AddConceptGroup);
}
