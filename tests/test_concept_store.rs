//! REQ-010 · 概念组的**存储契约**。
//!
//! 覆盖三件事：
//! 1. `Snapshot.concept_groups` 能落库取回，且 `None` / `[]` 三态无损；
//! 2. **红线**：概念组的存在**不改变实体表** —— 存一份带概念组的快照
//!    之后，`object_types` 的行数与 key 与不带概念组时逐字节一致；
//! 3. 重存不追加（`save_snapshot` 的既有语义对概念组同样成立）。

mod common;
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::concept::ConceptGroup;
use ontok::domain::snapshot::{EntityType, Evidence, ExtractedBy, Snapshot};
use std::collections::BTreeSet;
use uuid::Uuid;

async fn fresh_version(store: &PgStore, prefix: &str) -> (PgProjectStore, Uuid) {
    let ps = PgProjectStore::new(store.pool().clone());
    ps.migrate().await.unwrap();
    let p = common::fresh_project(&ps, prefix).await;
    let v = ps.new_version(p.id).await.unwrap();
    (ps, v.id)
}

fn entity(key: &str, name: &str) -> EntityType {
    EntityType {
        key: key.into(),
        display_name: name.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    }
}

/// 故意把三态都塞满：`scope_hint = None`（还没写说明）、
/// `extracted_by = Some`（谁声明的）、`evidence` 非空。
fn group_with_all_three_states(key: &str) -> ConceptGroup {
    ConceptGroup {
        key: key.into(),
        display_name: "套餐".into(),
        members: vec!["package_templates".into(), "package_products".into()],
        scope_hint: None,
        // REQ-028 · F4：刻意置 `Some(true)` 而不是 None ——
        // 这条快照往返测试要证明**非默认值**也能无损存回。
        // 若置 None，`low_confidence: None` 在「没建列」与「建了列但是 NULL」
        // 两种情况下都读得出来，这个字段的往返就等于没被测到。
        low_confidence: Some(true),
        extracted_by: Some(ExtractedBy {
            agent: "prefix-cluster-seed".into(),
            version: "0.1.0".into(),
            round: 1,
            failed_groups: vec![],
            source: None,
        }),
        evidence: vec![Evidence {
            table: "package_products".into(),
            column: String::new(),
            claim: "套餐下可买的产品".into(),
        }],
    }
}

fn base_snapshot(vid: Uuid) -> Snapshot {
    let mut s = Snapshot::new(vid, 1);
    s.entity_types = vec![
        entity("package_templates", "套餐模板"),
        entity("package_products", "套餐产品"),
        entity("orders", "订单"),
    ];
    s
}

#[tokio::test]
async fn test_concept_group_survives_snapshot_roundtrip() {
    let store = PgStore::new(test_pool().await);
    let (_ps, vid) = fresh_version(&store, "cg_rt").await;

    let mut snap = base_snapshot(vid);
    snap.concept_groups = vec![group_with_all_three_states("package")];
    store.save_snapshot(&snap).await.unwrap();

    let back = store.get_snapshot(vid).await.unwrap().expect("快照应存在");
    assert_eq!(back.concept_groups, snap.concept_groups, "概念组必须逐字段无损往返");
    let g = &back.concept_groups[0];
    assert!(g.scope_hint.is_none(), "NULL 要还原成 None（还没写说明），不是空串");
    assert!(g.extracted_by.is_some());
    assert_eq!(g.members.len(), 2);
    assert_eq!(g.evidence[0].table, "package_products");
}

#[tokio::test]
async fn test_empty_concept_group_collection_roundtrips_as_empty_not_null() {
    let store = PgStore::new(test_pool().await);
    let (_ps, vid) = fresh_version(&store, "cg_empty").await;

    let snap = base_snapshot(vid);
    assert!(snap.concept_groups.is_empty());
    store.save_snapshot(&snap).await.unwrap();
    let back = store.get_snapshot(vid).await.unwrap().unwrap();
    assert!(back.concept_groups.is_empty(), "空集合仍应是空集合（能读出来本身就是证据）");
}

/// **红线**（REQ-010 硬要求）：概念组是展示层声明，
/// `apply`/存盘**不得改变实体表**。
#[tokio::test]
async fn test_concept_groups_do_not_touch_entity_rows() {
    let store = PgStore::new(test_pool().await);
    let (_ps, vid_a) = fresh_version(&store, "cg_redline_a").await;
    let (_ps, vid_b) = fresh_version(&store, "cg_redline_b").await;

    // A：没有概念组。B：同一份实体 + 三个概念组。
    let mut a = base_snapshot(vid_a);
    let mut b = base_snapshot(vid_b);
    b.concept_groups = vec![
        group_with_all_three_states("package"),
        ConceptGroup { key: "order".into(), display_name: "订单".into(), members: vec!["orders".into()], ..group_with_all_three_states("tmp") },
    ];
    // 第二个组只留一个成员，避免与第一个组重叠（校验层会拒）
    b.concept_groups[1].evidence = vec![];

    store.save_snapshot(&a).await.unwrap();
    store.save_snapshot(&b).await.unwrap();

    let ba = store.get_snapshot(vid_a).await.unwrap().unwrap();
    let bb = store.get_snapshot(vid_b).await.unwrap().unwrap();

    assert_eq!(ba.entity_types.len(), bb.entity_types.len(), "实体行数不得因概念组而变");
    let ka: Vec<&str> = ba.entity_types.iter().map(|e| e.key.as_str()).collect();
    let kb: Vec<&str> = bb.entity_types.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(ka, kb, "实体 key 序列必须完全一致");
    assert_eq!(
        serde_json::to_string(&ba.entity_types).unwrap(),
        serde_json::to_string(&bb.entity_types).unwrap(),
        "实体行**逐字节**一致 —— 概念组是展示层声明"
    );
}

#[tokio::test]
async fn test_resave_replaces_concept_groups_not_appends() {
    let store = PgStore::new(test_pool().await);
    let (_ps, vid) = fresh_version(&store, "cg_re").await;

    let mut snap = base_snapshot(vid);
    snap.concept_groups = vec![group_with_all_three_states("package")];
    store.save_snapshot(&snap).await.unwrap();

    snap.concept_groups = vec![ConceptGroup {
        key: "package".into(),
        display_name: "套餐（已改名）".into(),
        members: vec!["package_templates".into()],
        scope_hint: Some("只留模板".into()),
        extracted_by: None,
        low_confidence: None, // REQ-028 · F4：默认「没声明过」
        evidence: vec![],
    }];
    store.save_snapshot(&snap).await.unwrap();

    let back = store.get_snapshot(vid).await.unwrap().unwrap();
    assert_eq!(back.concept_groups.len(), 1, "重存是替换，不是追加");
    assert_eq!(back.concept_groups[0].display_name, "套餐（已改名）");
    assert_eq!(back.concept_groups[0].scope_hint.as_deref(), Some("只留模板"));
    assert!(back.concept_groups[0].extracted_by.is_none(), "人工修订 = NULL（ADR-001）");
}

/// 概念组不得凭空引入实体 —— 存进去的成员必须在实体集合内。
/// 落库层不强制（那是 apply 期的职责），但**读回来**必须仍能被校验抓住：
/// 否则「校验只查新建、存量没人查」会成为盲区。
#[tokio::test]
async fn test_stored_dangling_member_is_still_detectable() {
    let store = PgStore::new(test_pool().await);
    let (_ps, vid) = fresh_version(&store, "cg_dangling").await;

    let mut snap = base_snapshot(vid);
    snap.concept_groups = vec![ConceptGroup {
        key: "package".into(),
        display_name: "套餐".into(),
        members: vec!["package_templates".into(), "package_ghost".into()],
        scope_hint: None,
        extracted_by: None,
        low_confidence: None, // REQ-028 · F4：默认「没声明过」
        evidence: vec![],
    }];
    store.save_snapshot(&snap).await.unwrap();

    let back = store.get_snapshot(vid).await.unwrap().unwrap();
    let known: BTreeSet<String> = back.entity_types.iter().map(|e| e.key.clone()).collect();
    let problems = ontok::domain::concept::validate_groups(&back.concept_groups, &known);
    assert!(
        problems.contains(&ontok::domain::concept::ConceptProblem::DanglingMember {
            group: "package".into(),
            member: "package_ghost".into()
        }),
        "存进去的悬空成员必须在读回后仍被判出，got={problems:?}"
    );
}
