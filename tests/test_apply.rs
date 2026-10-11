//! Task 8 · 提案应用与版本冻结
//!
//! 验收 AC-006（冻结后一切写入被拒）· AC-008 · ADR-001。

mod common;
use common::test_pool;
use ontok::db::apply::PgApplier;
use ontok::db::pg::StoreError;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{NewItem, NewProposal, PgProposalStore};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::review::{Decision, ItemKind, Track};
use ontok::domain::snapshot::{ActionType, ExtractedBy, Function, Snapshot, WriteBinding};
use serde_json::json;
use uuid::Uuid;

struct Ctx {
    projects: PgProjectStore,
    proposals: PgProposalStore,
    store: PgStore,
    applier: PgApplier,
    project_id: Uuid,
    v1: Uuid,
}

async fn ctx() -> Ctx {
    let pool = test_pool().await;
    let projects = PgProjectStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let applier = PgApplier::new(pool);
    projects.migrate().await.unwrap();
    let p = common::fresh_project(&projects, "apply").await;
    let v1 = projects.new_version(p.id).await.unwrap().id;
    // REQ-011 · base 版本**必须带占位栏**，否则本文件二十多个落库测试
    // 对 action_types / functions 零判别力：把这两栏整栏删掉，它们照样全绿。
    store.save_snapshot(&base_snapshot(v1)).await.unwrap();
    Ctx { projects, proposals, store, applier, project_id: p.id, v1 }
}

/// base 版本里那两个「切片 2 才写」的占位栏 —— 给非空取值。
///
/// `target` 故意指向 `customer`：`domain::apply` 的 `RemoveEntityType`
/// 分支会 `retain(|a| a.target != key)`，这样「删实体连带清动作类型」
/// 也能被下面那条测试真正走到。
fn base_snapshot(version_id: Uuid) -> Snapshot {
    Snapshot {
        version_id,
        seq: 1,
        extracted_by: None,
        entity_types: vec![],
        link_types: vec![],
        action_types: vec![ActionType {
            key: "publish_customer".into(),
            target: "customer".into(),
            parameters: json!({ "effective_from": "date" }),
            preconditions: json!({ "status": "draft" }),
            effects: vec![WriteBinding {
                system: "源库".into(),
                table: "customer".into(),
                field: "status".into(),
                key_expr: json!({ "cust_id": "$1" }),
                value_map: [("published".to_string(), "PUBLISHED".to_string())]
                    .into_iter()
                    .collect(),
                supported_values: vec![],
                conflict_field: Some("updated_at".into()),
                evidence: vec![],
            }],
            permission: json!({ "role": "运营" }),
            audit_policy: json!({ "retain_days": 365 }),
            evidence: vec![],
        }],
        functions: vec![Function {
            key: "customer_count".into(),
            signature: "customer_count(customer) -> i32".into(),
            allowed_types: vec!["customer".into()],
            evidence: vec![],
        }],
        concept_groups: vec![],
    }
}

fn ent(key: &str, display: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddEntityType,
        target_key: key.into(),
        payload: json!({
            "key": key, "display_name": display,
            "properties": [{"name": "编号", "column": format!("{key}_no"), "nullable": false}],
            "evidence": [{"table": "t", "column": "c", "claim": "x"}]
        }),
    }
}

fn link(from: &str, to: &str) -> NewItem {
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

/// 带父级的实体条目 —— 层级自检（B1）要用。
/// `ent()` 不带 `parent`，因为绝大多数测试不关心层级。
fn ent_with_parent(key: &str, display: &str, parent: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddEntityType,
        target_key: key.into(),
        payload: json!({
            "key": key, "display_name": display, "parent": parent,
            "properties": [], "evidence": []
        }),
    }
}

/// 改挂父级 —— 成环场景要用（`ModifyEntityType` 会写 `parent`）。
fn reparent(key: &str, parent: &str) -> NewItem {
    NewItem {
        kind: ItemKind::ModifyEntityType,
        target_key: key.into(),
        payload: json!({ "key": key, "parent": parent }),
    }
}

async fn accept_all(c: &Ctx, pid: Uuid) {
    for it in c.proposals.items(pid).await.unwrap() {
        c.proposals.set_decision(it.id, Track::Biz, Decision::Accepted).await.unwrap();
        c.proposals.set_decision(it.id, Track::Model, Decision::Accepted).await.unwrap();
    }
}

#[tokio::test]
async fn test_apply_creates_new_version_with_content() {
    let c = ctx().await;
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("customer", "客户"), ent("subscription", "客户订阅"), link("customer", "subscription")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;

    let v2 = c.applier.apply_proposal(p.id).await.unwrap();
    let snap = c.store.get_snapshot(v2).await.unwrap().unwrap();
    assert_eq!(snap.seq, 2, "新版本 seq 应是 base + 1");
    assert_eq!(snap.entity_types.len(), 2);
    assert_eq!(snap.link_types.len(), 1);
    assert!(snap.has("customer") && snap.has("subscription"));
    // payload 里的 properties / evidence 真落进去了
    assert_eq!(snap.entity("customer").unwrap().properties.len(), 1);
}

#[tokio::test]
async fn test_apply_marks_proposal_applied() {
    let c = ctx().await;
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("a", "甲")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    c.applier.apply_proposal(p.id).await.unwrap();
    assert_eq!(c.proposals.get(p.id).await.unwrap().unwrap().status, "applied");
}

#[tokio::test]
async fn test_apply_twice_is_rejected() {
    // 已 applied 的提案不得重复应用（会凭空多出两个版本）
    let c = ctx().await;
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("a", "甲")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    c.applier.apply_proposal(p.id).await.unwrap();
    let r = c.applier.apply_proposal(p.id).await;
    assert!(matches!(r, Err(StoreError::ProposalNotPending(_, _))), "实际：{r:?}");
}

#[tokio::test]
async fn test_apply_not_passed_is_rejected() {
    // 只接受一轨 → 不得 apply
    let c = ctx().await;
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("a", "甲")])
        .await
        .unwrap();
    let it = c.proposals.items(p.id).await.unwrap()[0].clone();
    c.proposals.set_decision(it.id, Track::Biz, Decision::Accepted).await.unwrap();
    // model 轨仍 pending

    let r = c.applier.apply_proposal(p.id).await;
    assert!(matches!(r, Err(StoreError::ProposalNotPassed { .. })), "实际：{r:?}");
    // **且不得留下任何版本** —— 失败时什么都没写
    let vs = c.projects.list_versions(c.project_id).await.unwrap();
    assert_eq!(vs.len(), 1, "失败的 apply 不得建出新版本");
}

#[tokio::test]
async fn test_apply_with_dangling_link_is_rejected() {
    // 关系类两端只接受一端 → 悬空 → 不得 apply
    let c = ctx().await;
    let p = c.proposals
        .create(c.project_id,
            NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("customer", "客户"), ent("order", "订单"), link("customer", "order")])
        .await
        .unwrap();
    let items = c.proposals.items(p.id).await.unwrap();
    for it in &items {
        c.proposals.set_decision(it.id, Track::Biz, Decision::Accepted).await.unwrap();
        c.proposals.set_decision(it.id, Track::Model, Decision::Accepted).await.unwrap();
    }
    // 把「订单」撤回 → 关系悬空
    c.proposals.set_decision(items[1].id, Track::Model, Decision::Pending).await.unwrap();

    let r = c.applier.apply_proposal(p.id).await;
    assert!(matches!(r, Err(StoreError::ProposalNotPassed { .. })), "实际：{r:?}");
}

#[tokio::test]
async fn test_chained_apply_keeps_accumulating() {
    // v1 加「客户」→ v2 加「订单」→ **v3 应同时含两者**，不是只含订单
    let c = ctx().await;

    let p1 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("customer", "客户")])
        .await
        .unwrap();
    accept_all(&c, p1.id).await;
    let v2 = c.applier.apply_proposal(p1.id).await.unwrap();

    let p2 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: v2, round: 2, extracted_by: None }, &[ent("order", "订单")])
        .await
        .unwrap();
    accept_all(&c, p2.id).await;
    let v3 = c.applier.apply_proposal(p2.id).await.unwrap();

    let snap = c.store.get_snapshot(v3).await.unwrap().unwrap();
    assert!(snap.has("customer"), "v3 应仍含 v1 加的「客户」，实际：{:?}",
        snap.entity_types.iter().map(|e| &e.key).collect::<Vec<_>>());
    assert!(snap.has("order"), "v3 应含 v2 加的「订单」");
    assert_eq!(snap.seq, 3);
}

#[tokio::test]
async fn test_extracted_by_is_not_inherited_on_apply() {
    // 与 `test_extracted_by_is_not_inherited` 同义，但**从 apply 侧看**：
    // 前者验「新版本的值」，这条验「base 的值原封不动留在 base 上」
    // —— 排除「apply 顺手把 base 也改了」这种副作用。
    let c = ctx().await;
    c.store
        .set_extracted_by(
            c.v1,
            &Some(ExtractedBy {
                agent: "vendor-extractor".into(),
                version: "2.3.1".into(),
                round: 1,
                failed_groups: vec![],
                source: None,
            }),
        )
        .await
        .unwrap();

    let p = c
        .proposals
        .create(
            c.project_id,
            NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("a", "甲")],
        )
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let v2 = c.applier.apply_proposal(p.id).await.unwrap();

    // base 仍是原值（没被 apply 顺手清掉）
    let b = c.store.get_snapshot(c.v1).await.unwrap().unwrap().extracted_by;
    assert_eq!(
        b.as_ref().map(|e| e.version.as_str()),
        Some("2.3.1"),
        "apply 不得修改 base 版本自己的 extracted_by"
    );
    // 新版本是 None
    assert_eq!(c.store.get_snapshot(v2).await.unwrap().unwrap().extracted_by, None);
}

#[tokio::test]
async fn test_extracted_by_taken_from_proposal_not_base() {
    // 对偶：提案**有** extracted_by 时，新版本取提案的值（不是 base 的）
    let c = ctx().await;
    c.store.set_extracted_by(c.v1, &Some(ExtractedBy {
        agent: "old-agent".into(), version: "1.0.0".into(),
        round: 1, failed_groups: vec![], source: None,
    })).await.unwrap();

    let new_eb = ExtractedBy {
        agent: "new-agent".into(), version: "2.0.0".into(),
        round: 5, failed_groups: vec!["t_x".into()], source: None,
    };
    let p1 = c.proposals
        .create(c.project_id,
            NewProposal { base_version_id: c.v1, round: 5, extracted_by: Some(new_eb.clone()) },
            &[ent("a", "甲")])
        .await
        .unwrap();
    accept_all(&c, p1.id).await;
    let v2 = c.applier.apply_proposal(p1.id).await.unwrap();

    let snap = c.store.get_snapshot(v2).await.unwrap().unwrap();
    assert_eq!(
        snap.extracted_by, Some(new_eb),
        "新版本应取**提案的** extracted_by，且 failed_groups 也要带过去"
    );
}

#[tokio::test]
async fn test_extracted_by_is_not_inherited() {
    // **AC-014 守门测试**。ADR-001：新版本原样取提案的值，
    // 提案没有则记 None（人工修订）—— **不继承 base**。
    //
    // 继承会让「agent 偷偷换版本」不可见，而那正是规格 §8.4 要防的。
    // 本文件的其他测试用「per_version」那种间接判据，
    // 这条按 AC-014 的名字直接断言继承语义。
    let c = ctx().await;
    c.store
        .set_extracted_by(
            c.v1,
            &Some(ExtractedBy {
                agent: "vendor-extractor".into(),
                version: "2.3.1".into(),
                round: 1,
                failed_groups: vec![],
                source: None,
            }),
        )
        .await
        .unwrap();

    // 提案**不带** extracted_by = 人工修订
    let p = c
        .proposals
        .create(
            c.project_id,
            NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("a", "甲")],
        )
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let v2 = c.applier.apply_proposal(p.id).await.unwrap();

    assert_eq!(
        c.store.get_snapshot(v2).await.unwrap().unwrap().extracted_by,
        None,
        "新版本**不得继承** base 的 extracted_by"
    );
}

#[tokio::test]
async fn test_failed_groups_not_inherited() {
    // **AC-015**：`failed_groups`（抽取失败的分组）也不继承。
    //
    // 理由与 extracted_by 相同 —— 继承会让「这轮抽取其实有分组失败了」
    // 在下一版里消失，评审者看到的是一份「全部成功」的快照。
    let c = ctx().await;
    c.store
        .set_extracted_by(
            c.v1,
            &Some(ExtractedBy {
                agent: "vendor-extractor".into(),
                version: "2.3.1".into(),
                round: 1,
                failed_groups: vec!["t_orders".into(), "t_billing".into()],
                source: None,
            }),
        )
        .await
        .unwrap();

    // 提案带一个**新的** extracted_by，它的 failed_groups 是空的
    let p = c
        .proposals
        .create(
            c.project_id,
            NewProposal {
                base_version_id: c.v1,
                round: 2,
                extracted_by: Some(ExtractedBy {
                    agent: "vendor-extractor".into(),
                    version: "2.4.0".into(),
                    round: 2,
                    failed_groups: vec![],
                    source: None,
                }),
            },
            &[ent("b", "乙")],
        )
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let v2 = c.applier.apply_proposal(p.id).await.unwrap();

    let got = c.store.get_snapshot(v2).await.unwrap().unwrap().extracted_by;
    let got = got.expect("提案带了 extracted_by，新版本应有值");
    assert!(
        got.failed_groups.is_empty(),
        "**不得继承** base 的 failed_groups：base 失败过 {:?}，新版本却显示空",
        vec!["t_orders", "t_billing"]
    );
    assert_eq!(got.version, "2.4.0", "版本号取提案的");
    // 反证：base 确实有失败分组
    let b = c.store.get_snapshot(c.v1).await.unwrap().unwrap().extracted_by.unwrap();
    assert_eq!(b.failed_groups.len(), 2);
}

#[tokio::test]
async fn test_apply_does_not_auto_freeze() {
    // 冻结是显式动作（规格 §9.1「冻结是判据」）
    let c = ctx().await;
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("a", "甲")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let v2 = c.applier.apply_proposal(p.id).await.unwrap();
    assert!(!c.projects.is_frozen(v2).await.unwrap(), "apply **不得**自动冻结");
}

#[tokio::test]
async fn test_apply_on_frozen_base_is_rejected() {
    let c = ctx().await;
    c.projects.freeze(c.v1).await.unwrap();
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("a", "甲")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let r = c.applier.apply_proposal(p.id).await;
    assert!(matches!(r, Err(StoreError::BaseVersionFrozen)), "实际：{r:?}");
}

#[tokio::test]
async fn test_frozen_version_rejects_writes() {
    // **AC-006 守门测试**：冻结 v1 后，一切写入 v1 的路径都必须被拒。
    // 「一切」包括：直接存快照、以它为 base 的 apply。逐条断言。
    let c = ctx().await;

    // 路径 ①：直接存快照
    let mut snap = c.store.get_snapshot(c.v1).await.unwrap().unwrap();
    snap.entity_types = vec![ontok::domain::snapshot::EntityType {
        key: "x".into(), display_name: "X".into(), description: None,
        aliases: vec![], parent: None, disjoint_with: vec![],
        properties: vec![], evidence: vec![],
    }];
    c.store.save_snapshot(&snap).await.unwrap();

    c.projects.freeze(c.v1).await.unwrap();

    let r = c.store.save_snapshot(&snap).await;
    assert!(
        matches!(r, Err(StoreError::VersionFrozen { .. })),
        "路径① 冻结后 save_snapshot 必须被拒，实际：{r:?}"
    );

    // 路径 ②：以冻结版本为 base 的 apply
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("y", "乙")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let r2 = c.applier.apply_proposal(p.id).await;
    assert!(
        matches!(r2, Err(StoreError::BaseVersionFrozen)),
        "路径② 冻结后 apply 必须被拒，实际：{r2:?}"
    );

    // 路径 ③：冻结操作本身幂等（重复冻结不是「写入内容」）
    let f = c.projects.freeze(c.v1).await.unwrap();
    assert!(f.frozen_at.is_some());
}

#[tokio::test]
async fn test_apply_remove_entity_type_cleans_links() {
    // 删类型必须连带清掉指向它的关系，不能留野指针
    let c = ctx().await;
    let p1 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("customer", "客户"), ent("order", "订单"), link("customer", "order")])
        .await
        .unwrap();
    accept_all(&c, p1.id).await;
    let v2 = c.applier.apply_proposal(p1.id).await.unwrap();
    assert_eq!(c.store.get_snapshot(v2).await.unwrap().unwrap().link_types.len(), 1);

    let p2 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: v2, round: 2, extracted_by: None },
            &[NewItem { kind: ItemKind::RemoveEntityType, target_key: "order".into(), payload: json!({}) }])
        .await
        .unwrap();
    accept_all(&c, p2.id).await;
    let v3 = c.applier.apply_proposal(p2.id).await.unwrap();

    let snap = c.store.get_snapshot(v3).await.unwrap().unwrap();
    assert!(!snap.has("order"));
    assert!(snap.link_types.is_empty(), "指向已删类型的关系必须被清掉：{:?}", snap.link_types);
}

#[tokio::test]
async fn test_apply_add_property_to_existing_type() {
    let c = ctx().await;
    let p1 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None }, &[ent("customer", "客户")])
        .await
        .unwrap();
    accept_all(&c, p1.id).await;
    let v2 = c.applier.apply_proposal(p1.id).await.unwrap();

    let p2 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: v2, round: 2, extracted_by: None },
            &[NewItem {
                kind: ItemKind::AddProperty,
                target_key: "customer".into(),
                payload: json!({"parent_key": "customer", "name": "客户名称", "column": "customer_name", "nullable": true}),
            }])
        .await
        .unwrap();
    accept_all(&c, p2.id).await;
    let v3 = c.applier.apply_proposal(p2.id).await.unwrap();

    let snap = c.store.get_snapshot(v3).await.unwrap().unwrap();
    let props = &snap.entity("customer").unwrap().properties;
    assert_eq!(props.len(), 2, "应在原有 1 个属性上追加");
    assert!(props.iter().any(|p| p.column == "customer_name"));
}

// ── B1 · 落库路径的第二道防线：`apply` 末尾的层级自检 ──────────────
//
// `src/domain/apply.rs:167-176`：apply 完拿 `out.parents()` 跑
// `validate_hierarchy`，有问题就拒。**这一段此前零测试覆盖** ——
// 独立审核的变异测试证明：整段删掉，15 条 apply 测试仍然 15 passed。
//
// 为什么必须守：提案审核（`summarize`）**只查关系类**，层级是另一码事。
// 环一旦落进版本快照，规格 §4.6 要求的遍历查询就不再终止 ——
// 而版本是**不可变**的，错了改不回来。

#[tokio::test]
async fn test_apply_result_snapshot_passes_hierarchy_check() {
    // 正向：apply 成功之后，产出快照的层级校验结果必须为空。
    let c = ctx().await;
    let p = c
        .proposals
        .create(
            c.project_id,
            NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("asset", "资产"), ent_with_parent("device", "设备", "asset")],
        )
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let v2 = c.applier.apply_proposal(p.id).await.unwrap();

    let snap = c.store.get_snapshot(v2).await.unwrap().unwrap();
    // 先确认父级真落库了（否则「校验为空」是因为压根没有层级可校验）
    assert_eq!(
        snap.entity("device").unwrap().parent.as_deref(),
        Some("asset"),
        "前提：device 的父级应落进快照"
    );
    let problems = ontok::domain::hierarchy::validate_hierarchy(&snap.parents());
    assert!(problems.is_empty(), "v2 快照的层级校验必须为空，实际：{problems:?}");
}

#[tokio::test]
async fn test_apply_rejects_hierarchy_cycle() {
    // **变异靶子**：把 `src/domain/apply.rs:169-176` 整段删掉，这条必须变红。
    //
    // asset→device→asset 的环只可能在这里被抓住：两条条目都不是关系类，
    // `summarize` 判它们「可应用」且整体 passed=true，
    // 所以删掉自检 → 环被写进**新的版本快照** → apply 返回 Ok。
    let c = ctx().await;
    let p1 = c
        .proposals
        .create(
            c.project_id,
            NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("asset", "资产")],
        )
        .await
        .unwrap();
    accept_all(&c, p1.id).await;
    let v2 = c.applier.apply_proposal(p1.id).await.unwrap();

    // 把 asset 挂到 device 下，同时新建 device 挂在 asset 下 → 成环
    let p2 = c
        .proposals
        .create(
            c.project_id,
            NewProposal { base_version_id: v2, round: 2, extracted_by: None },
            &[reparent("asset", "device"), ent_with_parent("device", "设备", "asset")],
        )
        .await
        .unwrap();
    accept_all(&c, p2.id).await;

    let r = c.applier.apply_proposal(p2.id).await;
    assert!(matches!(r, Err(StoreError::ProposalNotPassed { .. })), "成环必须被拒，实际：{r:?}");

    // 且**什么都没写** —— 失败时不得留下新版本、不得把提案标成 applied
    let vs = c.projects.list_versions(c.project_id).await.unwrap();
    assert_eq!(vs.len(), 2, "成环的 apply 不得建出新版本");
    assert_eq!(
        c.proposals.get(p2.id).await.unwrap().unwrap().status,
        "pending",
        "被拒的提案不得标记为 applied"
    );
}

#[tokio::test]
async fn test_apply_rejects_dangling_parent() {
    // 悬空父引用是同一道自检的另一半（`validate_hierarchy` 报两类问题）。
    // 同样地：这两条条目都不是关系类，`summarize` 一律放行。
    let c = ctx().await;
    let p = c
        .proposals
        .create(
            c.project_id,
            NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("asset", "资产"), ent_with_parent("device", "设备", "根本不存在的类型")],
        )
        .await
        .unwrap();
    accept_all(&c, p.id).await;

    let r = c.applier.apply_proposal(p.id).await;
    assert!(matches!(r, Err(StoreError::ProposalNotPassed { .. })), "悬空父引用必须被拒，实际：{r:?}");

    let vs = c.projects.list_versions(c.project_id).await.unwrap();
    assert_eq!(vs.len(), 1, "悬空父引用的 apply 不得建出新版本");
}

/// REQ-011 · BL-1 落库往返：占位栏必须跟着版本一起走。
///
/// 缺陷本体：`db::apply::write_snapshot_in` 开头 DELETE 五张表，
/// 函数体只 INSERT 三张 —— `action_types` / `functions`
/// **删了不写，事务内不报错、不告警**。
/// 领域层（`domain::apply` 整体继承 base）算出的正确结果
/// 在写快照那一刻被整栏丢弃。
///
/// 断言就一句话：**base 有 N 条 → 新版本仍是 N 条，且逐字节相同**。
#[tokio::test]
async fn test_apply_carries_action_types_and_functions_into_new_version() {
    let c = ctx().await;
    let base = c.store.get_snapshot(c.v1).await.unwrap().unwrap();
    assert_eq!(base.action_types.len(), 1, "前提：base 确实有动作类型");
    assert_eq!(base.functions.len(), 1, "前提：base 确实有函数");

    // 一次普通 apply：只加一个实体，占位栏不该被牵动
    let p = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("customer", "客户")])
        .await
        .unwrap();
    accept_all(&c, p.id).await;
    let v2 = c.applier.apply_proposal(p.id).await.unwrap();

    let snap = c.store.get_snapshot(v2).await.unwrap().unwrap();
    assert_eq!(
        snap.action_types.len(), 1,
        "apply 必须把动作类型写进新版本 —— 原先它在开头被 DELETE 后就再没写回来"
    );
    assert_eq!(
        serde_json::to_string(&snap.action_types).unwrap(),
        serde_json::to_string(&base.action_types).unwrap(),
        "动作类型必须逐字节不变（key / target / parameters / preconditions / \
         effects / permission / audit_policy / evidence 一栏都不许少）"
    );
    assert_eq!(snap.functions.len(), 1, "apply 必须把函数写进新版本");
    assert_eq!(
        serde_json::to_string(&snap.functions).unwrap(),
        serde_json::to_string(&base.functions).unwrap(),
        "函数必须逐字节不变（key / signature / allowed_types / evidence）"
    );
}

/// REQ-011 · BL-1 的另一半：领域层那句 `retain(|a| a.target != key)`
/// 终于能**落库**了。
///
/// 这条在修 BL-1 之前是不可能绿的：领域层算对了，但写快照时整栏丢弃，
/// 新版本恒为 0 条 —— 领域层算得再对也没有生效的机会。
#[tokio::test]
async fn test_apply_removing_entity_drops_actions_targeting_it() {
    let c = ctx().await;
    // base 的动作类型 `target = customer`，先把 customer 建出来
    let p1 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: c.v1, round: 1, extracted_by: None },
            &[ent("customer", "客户")])
        .await
        .unwrap();
    accept_all(&c, p1.id).await;
    let v2 = c.applier.apply_proposal(p1.id).await.unwrap();
    assert_eq!(
        c.store.get_snapshot(v2).await.unwrap().unwrap().action_types.len(), 1,
        "前提：加实体那轮动作类型还在"
    );

    // 再删掉 customer
    let p2 = c.proposals
        .create(c.project_id, NewProposal { base_version_id: v2, round: 2, extracted_by: None },
            &[NewItem { kind: ItemKind::RemoveEntityType, target_key: "customer".into(), payload: json!({}) }])
        .await
        .unwrap();
    accept_all(&c, p2.id).await;
    let v3 = c.applier.apply_proposal(p2.id).await.unwrap();

    let snap = c.store.get_snapshot(v3).await.unwrap().unwrap();
    assert!(
        snap.action_types.is_empty(),
        "指向已删类型的动作类型必须被清掉：{:?}",
        snap.action_types.iter().map(|a| a.key.clone()).collect::<Vec<_>>()
    );
    // 对照：函数这一栏领域层不参与清理，必须原样留下（证明不是「整栏清空」蒙对）
    assert_eq!(snap.functions.len(), 1, "函数不受 RemoveEntityType 影响，必须留着");
}

/// REQ-011 · BL-3：读侧顺序由 `ordinal` 决定，不由优化器决定。
///
/// `action_types` 原先没有 `ordinal` 列，`load_actions` 也就没有
/// `ORDER BY` —— 返回顺序是「索引扫描（按 unique key 序）」还是
/// 「顺序扫描（按插入序）」，全看优化器当场选哪个计划。
/// 这里的 key **故意与期望顺序相反**（写入 z→m→a）：
/// 一旦 `ORDER BY ordinal` 丢了、计划走索引，就得吐出 a→m→z，立刻变红。
#[tokio::test]
async fn test_action_types_order_is_write_order_not_planner_order() {
    let c = ctx().await;
    let mut snap = c.store.get_snapshot(c.v1).await.unwrap().unwrap();
    snap.action_types = vec![
        act("z_action", "customer"),
        act("m_action", "customer"),
        act("a_action", "customer"),
    ];
    c.store.save_snapshot(&snap).await.unwrap();

    let back = c.store.get_snapshot(c.v1).await.unwrap().unwrap();
    assert_eq!(
        back.action_types.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(),
        vec!["z_action", "m_action", "a_action"],
        "action_types 必须按写入序（ordinal）读回，不能是 key 序也不是优化器随便挑的序"
    );
}

fn act(key: &str, target: &str) -> ActionType {
    ActionType {
        key: key.into(),
        target: target.into(),
        parameters: json!({ "effective_from": "date" }),
        preconditions: json!({ "status": "draft" }),
        effects: vec![],
        permission: json!({ "role": "运营" }),
        audit_policy: json!({ "retain_days": 365 }),
        evidence: vec![],
    }
}
