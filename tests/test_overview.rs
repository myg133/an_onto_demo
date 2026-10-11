//! REQ-010 · **业务总览**的判别力。
//!
//! 覆盖需求里的五条：下钻到成员属性与关系、关系重定向、同名属性去重、
//! 未归组对象照旧单独显示、以及「没组 = 现在的行为」。

use ontok::domain::concept::ConceptGroup;
use ontok::domain::overview::{build_overview, render_overview_text};
use ontok::domain::view::{scan_modeling_terms, CustomerView, EntityView, PropertyView, RelationView};
use ontok::domain::snapshot::Evidence;
use std::collections::{BTreeMap, BTreeSet};

fn prop(name: &str) -> PropertyView {
    PropertyView { name: name.into(), note: None, required: false, sample: None }
}

fn obj(name: &str, props: &[&str]) -> EntityView {
    EntityView {
        name: name.into(),
        note: None,
        belongs_to: None,
        aliases: vec![],
        properties: props.iter().map(|p| prop(p)).collect(),
    }
}

fn rel(from: &str, to: &str, how_many: &str) -> RelationView {
    RelationView {
        from: from.into(),
        to: to.into(),
        how_many: how_many.into(),
        meaning: "归属于".into(),
    }
}

fn g(key: &str, name: &str, members: &[&str], hint: Option<&str>) -> ConceptGroup {
    ConceptGroup {
        key: key.into(),
        display_name: name.into(),
        members: members.iter().map(|s| s.to_string()).collect(),
        scope_hint: hint.map(String::from),
        extracted_by: None,
        low_confidence: None, // REQ-028 · F4：默认「没声明过」
        evidence: vec![],
    }
}

/// key → 展示名。真实数据里 key 就是源表名。
fn names(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, n)| (k.to_string(), n.to_string())).collect()
}

fn fixture() -> (CustomerView, BTreeMap<String, String>) {
    let view = CustomerView {
        objects: vec![
            obj("套餐模板", &["模板编号", "创建时间", "状态"]),
            obj("套餐产品", &["产品编号", "创建时间", "状态"]),
            obj("套餐权益", &["权益编号", "创建时间"]),
            obj("订单", &["订单编号", "创建时间"]),
            obj("试用记录", &["试用编号"]),
        ],
        relations: vec![
            rel("订单", "套餐模板", "多对一"),
            rel("订单", "套餐产品", "多对一"),
            rel("套餐产品", "套餐模板", "多对一"),
        ],
    };
    let n = names(&[
        ("t067", "套餐模板"),
        ("t066", "套餐产品"),
        ("t065", "套餐权益"),
        ("t064", "订单"),
        ("t087", "试用记录"),
    ]);
    (view, n)
}

#[test]
fn test_no_groups_reduces_to_plain_object_list() {
    // 向后兼容：一个组都不给时，总览 = 现在的对象清单，一个不少
    let (view, n) = fixture();
    let o = build_overview(&view, &[], &n);
    assert!(o.concepts.is_empty());
    assert_eq!(o.stats.concept_count, 0);
    assert_eq!(o.stats.objects_total, 5);
    assert_eq!(o.uncovered.len(), 5, "没组时全部对象单独显示，一个不许丢");
    let shown: Vec<&str> = o.uncovered.iter().map(|u| u.name.as_str()).collect();
    let all: Vec<&str> = view.objects.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(shown, all);
}

#[test]
fn test_concept_lists_members_with_scope_hint() {
    let (view, n) = fixture();
    let o = build_overview(
        &view,
        &[g("package", "套餐", &["t067", "t066", "t065"], Some("给客户看的三类套餐配置"))],
        &n,
    );
    assert_eq!(o.stats.concept_count, 1);
    let c = &o.concepts[0];
    assert_eq!(c.name, "套餐");
    assert_eq!(c.summary.as_deref(), Some("给客户看的三类套餐配置"));
    let ms: Vec<&str> = c.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(ms, vec!["套餐模板", "套餐产品", "套餐权益"]);
    assert_eq!(o.stats.objects_in_groups, 3);
    assert_eq!(o.stats.objects_uncovered, 2, "订单与试用记录没被组覆盖，要单独显示");
    assert_eq!(o.uncovered.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), vec!["订单", "试用记录"]);
}

/// **重定向的可见效果必须在「跨组关系」上看**。
///
/// # 为什么第一版这个测试是假绿（变异自证抓出来的）
///
/// 原先只放一个组，关系是「订单 → 套餐模板」。在「套餐」这张卡上，
/// 指向组内成员的那一端**就是卡片自己**，另一端（订单）不在组内 ——
/// 于是「重定向」与「不重定向」输出的问句**完全一样**。
/// 关掉重定向，16 条测试照样全绿。
///
/// # 重定向真正改变输出的场景
///
/// 关系 `扣费流水 → 套餐模板`，且两端分属两个不同的组：
/// - 在「账务」这张卡上，套餐模板那一端要被换成**它所属的概念**「套餐」；
/// - 不重定向就会显示「一个 账务，有几个 套餐模板？」—— 客户看到的是
///   一张具体的表，而不是业务概念。
#[test]
fn test_cross_group_relation_is_redirected_to_the_other_concept() {
    let view = CustomerView {
        objects: vec![
            obj("套餐模板", &["模板编号"]),
            obj("套餐产品", &["产品编号"]),
            obj("扣费流水", &["流水号"]),
            obj("余额账户", &["账户编号"]),
        ],
        relations: vec![rel("扣费流水", "套餐模板", "多对一")],
    };
    let n = names(&[
        ("t067", "套餐模板"),
        ("t066", "套餐产品"),
        ("t026", "扣费流水"),
        ("t015", "余额账户"),
    ]);
    let o = build_overview(
        &view,
        &[
            g("package", "套餐", &["t067", "t066"], None),
            g("billing", "账务", &["t026", "t015"], None),
        ],
        &n,
    );

    let billing = o.concepts.iter().find(|c| c.name == "账务").expect("应有「账务」概念");
    let qs: Vec<&str> = billing.relations.iter().map(|r| r.question.as_str()).collect();
    assert_eq!(
        qs,
        vec!["一个 账务，有几个 套餐？"],
        "指向另一个概念成员的那一端必须重定向成概念名，got={qs:?}"
    );
    // 关掉重定向时会变成「一个 账务，有几个 套餐模板？」—— 上面这条断言会红
    assert!(!qs.iter().any(|q| q.contains("套餐模板")), "还指着具体成员表：{qs:?}");

    // **两端都在组里 ⇒ 两端都被重定向**，这是对称的：
    // 在「套餐」这张卡上，「扣费流水」那一端指向的是它所属的概念「账务」。
    let package = o.concepts.iter().find(|c| c.name == "套餐").expect("应有「套餐」概念");
    let pq: Vec<&str> = package.relations.iter().map(|r| r.question.as_str()).collect();
    assert_eq!(pq, vec!["一个 套餐，有几个 账务？"], "got={pq:?}");
    assert!(!pq.iter().any(|q| q.contains("扣费流水")), "还指着具体成员表：{pq:?}");
    assert!(!pq.iter().any(|q| q.contains("余额账户")), "没关系的成员不许出现：{pq:?}");
}

#[test]
fn test_relations_point_to_concept_not_to_member_table() {
    let (view, n) = fixture();
    let o = build_overview(
        &view,
        &[g("package", "套餐", &["t067", "t066", "t065"], None)],
        &n,
    );
    let c = &o.concepts[0];
    let qs: Vec<&str> = c.relations.iter().map(|r| r.question.as_str()).collect();
    // 「订单」指向组内成员 ⇒ 重定向成指向「套餐」；两条并成一条（同一个问句）。
    // 方向站在**本卡片这一端**（与 html_view 的问句同一条纪律）：
    // 「一个 套餐，有几个 订单？」才是判读人脑子里真正在问的那个。
    assert_eq!(qs, vec!["一个 套餐，有几个 订单？"], "指向组内成员的关系要重定向到概念并去重：{qs:?}");
    assert_eq!(c.relations[0].answer, "多个", "多对一的另一端是「多个」");
    // 重定向后不许再出现具体成员表名当端点
    for member in ["套餐模板", "套餐产品"] {
        assert!(
            !c.relations.iter().any(|r| r.question.contains(member)),
            "关系端点还指着成员 {member}，没重定向：{:?}",
            c.relations.iter().map(|r| &r.question).collect::<Vec<_>>()
        );
    }
    assert!(o.stats.relation_ends_redirected >= 2);
}

#[test]
fn test_inside_group_relation_is_skipped_and_counted() {
    let (view, n) = fixture();
    let o = build_overview(
        &view,
        &[g("package", "套餐", &["t067", "t066", "t065"], None)],
        &n,
    );
    // 套餐产品 → 套餐模板 两端都在组内 ⇒ 不能变成「一个 套餐，有几个 套餐？」
    assert_eq!(o.concepts[0].relations_inside_skipped, 1);
    assert_eq!(o.stats.relations_inside_skipped, 1);
    assert!(!render_overview_text(&o).contains("几个 套餐？"), "组内关系不许硬凑成问句");
    assert!(render_overview_text(&o).contains("组内关系已省略"), "省略必须报出来");
}

#[test]
fn test_same_named_properties_are_merged_with_provenance() {
    let (view, n) = fixture();
    let o = build_overview(
        &view,
        &[g("package", "套餐", &["t067", "t066", "t065"], None)],
        &n,
    );
    let c = &o.concepts[0];
    let names: Vec<&str> = c.properties.iter().map(|p| p.name.as_str()).collect();
    // 3 张表各有一个「创建时间」⇒ 合并成 1 行；「状态」两张表各一个 ⇒ 1 行
    assert_eq!(names, vec!["模板编号", "创建时间", "状态", "产品编号", "权益编号"]);
    assert_eq!(c.properties.iter().find(|p| p.name == "创建时间").unwrap().from_members.len(), 3);
    assert_eq!(
        c.properties.iter().find(|p| p.name == "状态").unwrap().from_members,
        vec!["套餐模板", "套餐产品"]
    );
    // 合并前 3+3+2=8 项 ⇒ 合并后 5 项
    assert_eq!(o.stats.properties_before_dedup, 8);
    assert_eq!(o.stats.properties_after_dedup, 5);
    let text = render_overview_text(&o);
    assert!(text.contains("← 合并自 套餐模板、套餐产品、套餐权益"), "去重要给出依据：\n{text}");
}

#[test]
fn test_required_is_or_across_members() {
    let (view, n) = fixture();
    let mut view = view;
    view.objects[0].properties[0].required = true;
    let o = build_overview(&view, &[g("package", "套餐", &["t067", "t066"], None)], &n);
    let row = o.concepts[0].properties.iter().find(|p| p.name == "模板编号").unwrap();
    assert!(row.required);
}

#[test]
fn test_concept_name_degenerated_to_entity_key_is_dropped_and_reported() {
    let (view, n) = fixture();
    // 概念名叫「t067」—— 退化成了实体 key，客户视图里同类名字整批消失
    let o = build_overview(&view, &[g("package", "t067", &["t066"], None)], &n);
    assert!(o.concepts.is_empty(), "退化名不许上总览");
    assert_eq!(o.stats.concepts_dropped, 1);
    assert!(o.notes.iter().any(|x| x.contains("不可用")), "必须报出来：{:?}", o.notes);
    assert_eq!(o.uncovered.len(), 5, "整张卡掉了，成员回到未归组，一个不许丢");
}

#[test]
fn test_english_only_concept_name_is_dropped() {
    let (view, n) = fixture();
    let o = build_overview(&view, &[g("package", "package", &["t066"], None)], &n);
    assert!(o.concepts.is_empty(), "纯英文概念名对业务人员没有信息量");
    assert_eq!(o.stats.concepts_dropped, 1);
}

#[test]
fn test_member_missing_from_view_is_reported_not_silently_dropped() {
    let (view, n) = fixture();
    let mut o = build_overview(&view, &[g("package", "套餐", &["t067", "package_ghost"], None)], &n);
    // 构造一个「成员在快照里但没进客户视图」的情形：names 有它，view 没有
    o = build_overview(&view, &[g("package", "套餐", &["t067", "package_ghost"], None)], &n);
    assert_eq!(o.concepts[0].members.len(), 1, "只显示查得到的成员");
    assert!(o.notes.iter().any(|x| x.contains("1 个成员不在客户视图")), "notes={:?}", o.notes);
}

#[test]
fn test_duplicate_member_display_names_are_reported() {
    // 真实源库：t016 与 t027 都翻成「客户账号绑定」
    let view = CustomerView {
        objects: vec![obj("客户账号绑定", &["账号"]), obj("试用记录", &["试用"])],
        relations: vec![],
    };
    let n = names(&[
        ("t016", "客户账号绑定"),
        ("t027", "客户账号绑定"),
        ("t087", "试用记录"),
    ]);
    let o = build_overview(&view, &[g("binding", "账号绑定", &["t016", "t027"], None)], &n);
    assert_eq!(o.concepts[0].members.len(), 1, "同一个显示名只列一次");
    assert_eq!(o.concepts[0].duplicate_member_names, 1);
    assert!(o.notes.iter().any(|x| x.contains("显示名")), "notes={:?}", o.notes);
}

/// 证据（表名）**不许**出现在客户读到的总览里 —— 表名是源库标识符。
///
/// ⚠️ REQ-084：判据串是**假名**（`t067`），与本测试内的输入侧是同一个串 ——
/// `t067` 在 `fixture()` 的 `n` 里能翻成「套餐模板」，翻译一旦失效就原样吐回，
/// 判据随之变红。**改输入侧必须同批改判据侧**，只改一侧 ⇒ 恒真 ⇒ 守卫静默失效。
#[test]
fn test_evidence_table_names_never_reach_the_overview() {
    let (view, n) = fixture();
    let mut groups = vec![g("package", "套餐", &["t067", "t066"], Some("给客户看的三类套餐配置"))];
    groups[0].evidence = vec![Evidence {
        table: "t067".into(),
        column: "template_no".into(),
        claim: "套餐下可买的产品".into(),
    }];
    let o = build_overview(&view, &groups, &n);
    let text = render_overview_text(&o);
    let json = serde_json::to_string(&o).unwrap();
    for hay in [&text, &json] {
        assert!(!hay.contains("t067"), "表名泄进了总览：\n{hay}");
        assert!(!hay.contains("template_no"), "列名泄进了总览：\n{hay}");
    }
    // 守门器也不许命中
    assert!(scan_modeling_terms(&json).is_empty(), "{:?}", scan_modeling_terms(&json));
    assert!(scan_modeling_terms(&text).is_empty(), "{:?}", scan_modeling_terms(&text));
}

/// 总览的文案也必须过**同一道**建模术语守门 —— 概念名是新文案来源。
#[test]
fn test_overview_text_is_gated_by_modeling_term_scanner() {
    let (view, n) = fixture();
    // 名字里带建模术语的概念：守门器必须能抓住（CLI 据此 exit 2）
    let bad = g("package", "套餐主键组", &["t067"], None);
    let o = build_overview(&view, &[bad], &n);
    let text = render_overview_text(&o);
    let hits = scan_modeling_terms(&text);
    assert!(!hits.is_empty(), "带建模术语的概念名应当被守门器抓住（got empty）");
}

#[test]
fn test_overview_never_shows_member_property_columns() {
    // S-2 红线相关：总览只吃客户视图，视图里没有列名，总览就不可能有
    let (view, n) = fixture();
    let o = build_overview(&view, &[g("package", "套餐", &["t067"], None)], &n);
    let text = render_overview_text(&o);
    for col in ["template_no", "product_id", "created_at", "id"] {
        assert!(!text.contains(col), "源库列名泄进总览：{col}\n{text}");
    }
}

#[test]
fn test_home_map_drives_uncovered_complement() {
    // 归属映射 = home_map；未归组必须是它的补集，一个不多一个不少
    let (view, n) = fixture();
    let groups = vec![
        g("package", "套餐", &["t067", "t066", "t065"], None),
        g("trial", "试用", &["t087"], None),
    ];
    let o = build_overview(&view, &groups, &n);
    let covered: BTreeSet<String> = o
        .concepts
        .iter()
        .flat_map(|c| c.members.iter().map(|m| m.name.clone()))
        .collect();
    let uncovered: BTreeSet<String> = o.uncovered.iter().map(|u| u.name.clone()).collect();
    assert!(covered.is_disjoint(&uncovered));
    assert_eq!(covered.len() + uncovered.len(), view.objects.len());
    assert_eq!(uncovered, BTreeSet::from(["订单".to_string()]));
}

/// S-2 红线在总览这一层同样成立：属性名带技术用语 ⇒ **丢弃**，不许原样透出，
/// 且丢弃必须被计数、被报出（与 HTML 页面同一套判据，同一个函数）。
#[test]
fn test_technical_property_names_are_dropped_not_passed_through() {
    let mut o1 = obj("兑换码", &["兑换码", "核销人customer_id"]);
    o1.properties[1].name = "核销人customer_id".into();
    let view = CustomerView {
        objects: vec![o1, obj("试用记录", &["试用编号"])],
        relations: vec![],
    };
    let n = names(&[("t034", "兑换码"), ("t087", "试用记录")]);
    let o = build_overview(&view, &[g("gift", "兑换码", &["t034"], None)], &n);

    let card = &o.concepts[0];
    let shown: Vec<&str> = card.properties.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(shown, vec!["兑换码"], "带源库标识符的属性名必须丢弃：{shown:?}");
    assert_eq!(card.properties_dropped, 1);
    assert_eq!(o.stats.properties_dropped, 1);
    let text = render_overview_text(&o);
    assert!(!text.contains("customer_id"), "技术串原样透进了总览：\n{text}");
    assert!(text.contains("1 项属性名称带系统内部的技术叫法"), "剔除必须让判读人看见：\n{text}");
    assert!(o.notes.iter().any(|x| x.contains("没上总览")), "notes={:?}", o.notes);
}

/// 「客户ID」这类只带 ID 记号的名字：摘掉记号**保留**，
/// 且摘完与别的成员的「客户」合并成一行（清洗后再去重）。
#[test]
fn test_id_token_is_stripped_then_merged() {
    let mut a = obj("客户", &["客户ID", "状态"]);
    a.properties[0].name = "客户ID".into();
    let mut b = obj("客户订阅", &["客户", "到期时间"]);
    b.properties[0].name = "客户".into();
    let view = CustomerView { objects: vec![a, b], relations: vec![] };
    let n = names(&[("t025", "客户"), ("t023", "客户订阅")]);
    let o = build_overview(&view, &[g("c", "客户", &["t025", "t023"], None)], &n);
    let names: Vec<&str> = o.concepts[0].properties.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["客户", "状态", "到期时间"], "清洗后再去重：{names:?}");
    let row = o.concepts[0].properties.iter().find(|p| p.name == "客户").unwrap();
    assert_eq!(row.from_members, vec!["客户", "客户订阅"]);
}
