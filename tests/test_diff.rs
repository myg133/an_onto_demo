//! Task 9 · 快照 diff 引擎
//!
//! 纯逻辑，零 I/O。核心断言是「evidence 变化不得产生 diff」。

use ontok::domain::diff::{diff_snapshots, DiffItem, DiffKind};
use ontok::domain::snapshot::{
    EntityType, Evidence, LinkType, Property, Snapshot,
};
use uuid::Uuid;

fn ent(key: &str, name: &str) -> EntityType {
    EntityType {
        key: key.into(),
        display_name: name.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![Evidence { table: "t".into(), column: "c".into(), claim: "x".into() }],
    }
}

fn prop(name: &str, col: &str, nullable: bool) -> Property {
    Property { name: name.into(), column: col.into(), nullable, note: None, sample: None }
}

fn link(key: &str, f: &str, t: &str, card: &str) -> LinkType {
    LinkType {
        key: key.into(), from_type: f.into(), to_type: t.into(),
        cardinality: card.into(), semantic: "has".into(), evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
    }
}

fn find<'a>(d: &'a [DiffItem], target: &str, field: &str) -> Option<&'a DiffItem> {
    d.iter().find(|x| x.target == target && x.field.as_deref() == Some(field))
}

#[test]
fn test_diff_empty_to_empty_is_empty() {
    let a = Snapshot::new(Uuid::nil(), 1);
    let b = Snapshot::new(Uuid::nil(), 2);
    assert!(diff_snapshots(&a, &b).is_empty(), "两个空快照之间不该有 diff");
}

#[test]
fn test_diff_ignores_evidence_entirely() {
    // **最容易写错的地方**：证据每次抽取都会变，
    // 把它算进 diff 会让每次 diff 都几百条噪声，
    // 评审的人会开始无视 diff —— 那比漏报更危险。
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户")];
    let mut b = a.clone();
    b.entity_types[0].evidence = vec![
        Evidence { table: "t2".into(), column: "c2".into(), claim: "换了说法".into() },
    ];
    b.entity_types[0].evidence.push(Evidence { table: "t3".into(), column: "c3".into(), claim: "又一条".into() });
    let d = diff_snapshots(&a, &b);
    assert!(d.is_empty(), "只有 evidence 变化时必须零 diff，实际：{d:?}");
}

#[test]
fn test_diff_ignores_version_id_and_seq() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户")];
    let mut b = a.clone();
    b.version_id = Uuid::new_v4();
    b.seq = 7;
    assert!(diff_snapshots(&a, &b).is_empty(), "版本号本身不是 diff");
}

#[test]
fn test_diff_added_entity() {
    let a = Snapshot::new(Uuid::nil(), 1);
    let mut b = a.clone();
    b.entity_types = vec![ent("customer", "客户")];
    let d = diff_snapshots(&a, &b);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].kind, DiffKind::Added);
    assert_eq!(d[0].target, "customer");
    assert!(d[0].field.is_none(), "整型新增不该挂在某个字段上");
    assert!(d[0].summary().contains("customer"));
}

#[test]
fn test_diff_removed_entity() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户"), ent("t064", "订单")];
    let mut b = a.clone();
    b.entity_types.retain(|e| e.key != "t064");
    let d = diff_snapshots(&a, &b);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].kind, DiffKind::Removed);
    assert_eq!(d[0].target, "t064");
}

#[test]
fn test_diff_modified_scalar_fields() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户")];
    let mut b = a.clone();
    b.entity_types[0].display_name = "签约客户".into();
    b.entity_types[0].description = Some("有说明了".into());
    b.entity_types[0].aliases = vec!["用户".into()];
    b.entity_types[0].disjoint_with = vec!["supplier".into()];

    let d = diff_snapshots(&a, &b);
    for f in ["display_name", "description", "aliases", "disjoint_with"] {
        assert!(find(&d, "customer", f).is_some(), "{f} 变了却没报出来：{d:?}");
    }
}

#[test]
fn test_diff_distinguishes_none_from_empty_string() {
    // 三态在 diff 里也要区分：None → Some("") 是一次真实变化
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户")];
    let mut b = a.clone();
    b.entity_types[0].description = Some(String::new());
    let d = diff_snapshots(&a, &b);
    let it = find(&d, "customer", "description").expect("None → Some(\"\") 是变化");
    assert!(it.before.as_ref().unwrap().is_null());
    assert_eq!(it.after.as_ref().unwrap().as_str(), Some(""));
    assert!(it.summary().contains("（空串）"), "空串要能被人读出来：{}", it.summary());
}

#[test]
fn test_diff_parent_change_is_hierarchy_kind() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("asset", "资产"), ent("device", "设备")];
    a.entity_types[1].parent = Some("asset".into());
    let mut b = a.clone();
    b.entity_types[1].parent = None; // 提根

    let d = diff_snapshots(&a, &b);
    let it = find(&d, "device", "parent").expect("parent 变了要报");
    assert_eq!(it.kind, DiffKind::Hierarchy, "层级变化应单独成类");
}

#[test]
fn test_diff_same_parent_no_change() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("asset", "资产"), ent("device", "设备")];
    a.entity_types[1].parent = Some("asset".into());
    let b = a.clone();
    assert!(diff_snapshots(&a, &b).is_empty(), "parent 没变就不该报");
}

#[test]
fn test_diff_properties_added_removed_modified() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户")];
    a.entity_types[0].properties = vec![prop("客户编号", "customer_no", false), prop("旧的", "old_col", true)];

    let mut b = a.clone();
    b.entity_types[0].properties = vec![prop("客户编号", "customer_no", true), prop("客户名称", "customer_name", true)];
    // customer_no：nullable 改了
    // old_col：删了
    // customer_name：新增

    let d = diff_snapshots(&a, &b);
    assert!(
        d.iter().any(|x| x.kind == DiffKind::Added && x.target == "customer.customer_name"),
        "新增属性要报：{d:?}"
    );
    assert!(
        d.iter().any(|x| x.kind == DiffKind::Removed && x.target == "customer.old_col"),
        "删除属性要报：{d:?}"
    );
    let m = find(&d, "customer.customer_no", "nullable").expect("nullable 变了要报");
    assert_eq!(m.kind, DiffKind::Modified);
}

#[test]
fn test_diff_link_added_removed_modified() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.link_types = vec![link("c_has_o", "customer", "t064", "1:N")];
    let mut b = a.clone();
    b.link_types = vec![link("c_has_o", "customer", "t064", "N:M")];

    let d = diff_snapshots(&a, &b);
    let it = find(&d, "c_has_o", "cardinality").expect("基数变了要报");
    assert_eq!(it.kind, DiffKind::Modified);
    assert!(it.summary().contains("1:N") && it.summary().contains("N:M"));

    let c = Snapshot::new(Uuid::nil(), 3);
    let added = diff_snapshots(&c, &a);
    assert_eq!(added[0].kind, DiffKind::Added);
    assert_eq!(added[0].target, "c_has_o");

    let removed = diff_snapshots(&a, &c);
    assert_eq!(removed[0].kind, DiffKind::Removed);
}

#[test]
fn test_diff_link_semantic_change_reported() {
    // has → belongs_to 是语义反转，比基数改动更重，必须报
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.link_types = vec![link("rel", "customer", "t064", "1:N")];
    let mut b = a.clone();
    b.link_types[0].semantic = "belongs_to".into();
    let d = diff_snapshots(&a, &b);
    assert!(find(&d, "rel", "semantic").is_some(), "语义反转要报：{d:?}");
}

#[test]
fn test_diff_multiple_changes_all_reported() {
    let mut a = Snapshot::new(Uuid::nil(), 1);
    a.entity_types = vec![ent("customer", "客户"), ent("old", "旧的")];
    a.link_types = vec![link("rel", "customer", "old", "1:N")];
    let mut b = a.clone();
    b.entity_types.retain(|e| e.key != "old");
    b.entity_types[0].display_name = "新客户名".into();
    b.entity_types.push(ent("t064", "订单"));
    b.link_types.clear();

    let d = diff_snapshots(&a, &b);
    assert!(d.iter().any(|x| x.kind == DiffKind::Removed && x.target == "old"));
    assert!(d.iter().any(|x| x.kind == DiffKind::Added && x.target == "t064"));
    assert!(find(&d, "customer", "display_name").is_some());
    assert!(d.iter().any(|x| x.kind == DiffKind::Removed && x.target == "rel"));
    assert_eq!(d.len(), 4, "一次 diff 应报全所有变化，实际 {d:#?}");
}
