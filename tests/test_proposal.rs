//! Task 5 · 提案模型
//!
//! 重点：关系类条目的 payload 必须带两端；行 → 状态机输入的接缝。

mod common;
use common::test_pool;
use ontok::db::pg::StoreError;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{
    parse_decision, parse_kind, NewItem, NewProposal, PgProposalStore,
};
use ontok::domain::review::{Decision, ItemKind, ItemView, Track};
use ontok::domain::snapshot::ExtractedBy;
use serde_json::json;
use uuid::Uuid;


async fn fresh() -> (PgProjectStore, PgProposalStore, Uuid) {
    let pool = test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let gs = PgProposalStore::new(pool);
    ps.migrate().await.unwrap();
    gs.migrate().await.unwrap();
    let p = common::fresh_project(&ps, "prop").await;
    let v = ps.new_version(p.id).await.unwrap();
    (ps, gs, v.id)
}

fn entity_item(key: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddEntityType,
        target_key: key.into(),
        payload: json!({ "key": key, "display_name": key, "properties": [] }),
    }
}

fn link_item(from: &str, to: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddLinkType,
        target_key: format!("{from}→{to}"),
        payload: json!({
            "key": format!("{from}_has_{to}"),
            "from_type": from, "to_type": to,
            "cardinality": "1:N", "semantic": "has"
        }),
    }
}

#[tokio::test]
async fn test_proposal_create_with_items() {
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "prop2").await;
    let v = ps2.new_version(p.id).await.unwrap();

    let row = gs
        .create(
            p.id,
            NewProposal { base_version_id: v.id, round: 1, extracted_by: None },
            &[entity_item("customer"), entity_item("subscription"), link_item("customer", "subscription")],
        )
        .await
        .unwrap();

    assert_eq!(row.status, "pending");
    let items = gs.items(row.id).await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].seq, 1, "seq 从 1 起，CLI 的 --item 1 指它");
    assert_eq!(items[2].biz_status, "pending");
    assert_eq!(items[2].model_status, "pending");
}

#[tokio::test]
async fn test_proposal_link_item_requires_both_ends_in_payload() {
    // **关键约束**：关系类 payload 缺 from_type/to_type 必须报错。
    // 缺了的话状态机会判成「悬空」被拦下 —— 宁可导入失败也不要静默放行。
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "nolink").await;
    let v = ps2.new_version(p.id).await.unwrap();

    let bad = NewItem {
        kind: ItemKind::AddLinkType,
        target_key: "客户→订单".into(),
        payload: json!({ "key": "x", "cardinality": "1:N" }), // 缺两端
    };
    let r = gs
        .create(p.id, NewProposal { base_version_id: v.id, round: 1, extracted_by: None }, &[bad])
        .await;
    assert!(r.is_err(), "关系类缺两端必须报错，实际：{r:?}");

    let bad2 = NewItem {
        kind: ItemKind::AddLinkType,
        target_key: "客户→订单".into(),
        payload: json!({ "key": "x", "from_type": "customer" }), // 只给一端
    };
    assert!(
        gs.create(p.id, NewProposal { base_version_id: v.id, round: 1, extracted_by: None }, &[bad2])
            .await
            .is_err(),
        "只给一端也必须报错"
    );
}

#[tokio::test]
async fn test_proposal_empty_items_rejected() {
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "empty").await;
    let v = ps2.new_version(p.id).await.unwrap();
    let r = gs
        .create(p.id, NewProposal { base_version_id: v.id, round: 1, extracted_by: None }, &[])
        .await;
    assert!(matches!(r, Err(StoreError::NothingApplicable)));
}

#[tokio::test]
async fn test_proposal_set_decision_per_track() {
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "dec").await;
    let v = ps2.new_version(p.id).await.unwrap();
    let row = gs
        .create(p.id, NewProposal { base_version_id: v.id, round: 1, extracted_by: None }, &[entity_item("customer")])
        .await
        .unwrap();
    let item = gs.items(row.id).await.unwrap()[0].clone();

    gs.set_decision(item.id, Track::Biz, Decision::Accepted).await.unwrap();
    let after = gs.items(row.id).await.unwrap()[0].clone();
    assert_eq!(after.biz_status, "accepted");
    assert_eq!(after.model_status, "pending", "另一轨不受影响");
}

#[tokio::test]
async fn test_proposal_item_by_seq() {
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "seq").await;
    let v = ps2.new_version(p.id).await.unwrap();
    let row = gs
        .create(
            p.id,
            NewProposal { base_version_id: v.id, round: 1, extracted_by: None },
            &[entity_item("a"), entity_item("b")],
        )
        .await
        .unwrap();

    let it = gs.item_by_seq(row.id, 2).await.unwrap().expect("seq 2 应存在");
    assert_eq!(it.target_key, "b");
    assert!(gs.item_by_seq(row.id, 99).await.unwrap().is_none());
}

#[tokio::test]
async fn test_proposal_to_item_views_bridges_to_domain() {
    // **两层接缝**：DB 行 → domain 的 ItemView
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "bridge").await;
    let v = ps2.new_version(p.id).await.unwrap();
    let row = gs
        .create(
            p.id,
            NewProposal { base_version_id: v.id, round: 1, extracted_by: None },
            &[entity_item("customer"), link_item("customer", "subscription")],
        )
        .await
        .unwrap();
    gs.set_decision(gs.items(row.id).await.unwrap()[0].id, Track::Biz, Decision::Accepted).await.unwrap();
    gs.set_decision(gs.items(row.id).await.unwrap()[0].id, Track::Model, Decision::Accepted).await.unwrap();

    let rows = gs.items(row.id).await.unwrap();
    let views: Vec<ItemView> = PgProposalStore::to_item_views(&rows);
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].kind, ItemKind::AddEntityType);
    assert_eq!(views[0].biz_status, Decision::Accepted);
    assert_eq!(views[0].model_status, Decision::Accepted);
    // 关系类的两端从 payload 里取出来
    assert_eq!(views[1].from_type.as_deref(), Some("customer"));
    assert_eq!(views[1].to_type.as_deref(), Some("subscription"));
    // id 稳定可读，不是 UUID
    assert!(views[0].id.starts_with("1:add_entity_type:"), "实际：{}", views[0].id);
}

#[tokio::test]
async fn test_proposal_unknown_decision_is_pending_not_accepted() {
    // **安全方向**：未知状态必须当 Pending（未决），
    // 绝不能当 Accepted —— 把未知当通过是危险方向。
    assert_eq!(parse_decision("accepted"), Decision::Accepted);
    assert_eq!(parse_decision("rejected"), Decision::Rejected);
    assert_eq!(parse_decision("pending"), Decision::Pending);
    assert_eq!(parse_decision("garbage"), Decision::Pending);
    assert_eq!(parse_decision(""), Decision::Pending);
    assert_eq!(parse_decision("ACCEPTED"), Decision::Pending, "大小写不同也算未知");
}

#[tokio::test]
async fn test_proposal_parse_kind_known_and_unknown() {
    // 5 种 kind 必须各自对应（`as_str` 与 `parse_kind` 要互为逆运算）
    for k in [
        ItemKind::AddEntityType,
        ItemKind::AddProperty,
        ItemKind::AddLinkType,
        ItemKind::RemoveEntityType,
        ItemKind::ModifyEntityType,
    ] {
        assert_eq!(parse_kind(k.as_str()), k, "{} 往返失配", k.as_str());
    }
    // 未知 kind 落到**非关系**分支 —— 落到 AddLinkType 会让它去查两端，
    // 落到 AddEntityType 会让它进 available 集合放大影响面。
    // AddProperty 两头都不沾，是最保守的落点。
    assert_eq!(parse_kind("frobnicate"), ItemKind::AddProperty);
    assert!(!parse_kind("frobnicate").is_relational());
    assert!(!parse_kind("frobnicate").is_creating_type());
}

#[tokio::test]
async fn test_proposal_extracted_by_none_roundtrips() {
    // ADR-001：提案没有 extracted_by 就是「人工修订」，必须能与抽取来的区分
    let (_ps, gs, _v) = fresh().await;
    let ps2 = PgProjectStore::new(gs.pool().clone());
    let p = common::fresh_project(&ps2, "ebnone").await;
    let v = ps2.new_version(p.id).await.unwrap();

    let human = gs
        .create(p.id, NewProposal { base_version_id: v.id, round: 1, extracted_by: None }, &[entity_item("a")])
        .await
        .unwrap();
    assert!(human.extracted_by.is_none(), "人工修订提案应记 NULL");

    let v2 = ps2.new_version(p.id).await.unwrap();
    let eb = ExtractedBy { agent: "demo".into(), version: "0.1.0".into(), round: 1, failed_groups: vec![], source: None };
    let auto = gs
        .create(p.id, NewProposal { base_version_id: v2.id, round: 1, extracted_by: Some(eb) }, &[entity_item("b")])
        .await
        .unwrap();
    assert!(auto.extracted_by.is_some(), "抽取提案应记下来源");
}
