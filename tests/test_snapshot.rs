//! Task 4 · 快照契约往返
//!
//! 验收 AC-003 / AC-010 / AC-011。
//!
//! **本文件是 AC-010 的核心**：`None` / `[]` / `{}` 三态往返必须无损，
//! 不能都变成 `null`，也不能都变成空集合。

mod common;
use common::test_pool;
use ontok::db::pg::StoreError;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::{
    ActionType, EntityType, Evidence, ExtractedBy, LinkType, Property, Snapshot, WriteBinding,
};
use uuid::Uuid;

async fn fresh_version(store: &PgStore) -> (PgProjectStore, Uuid) {
    let ps = PgProjectStore::new(store.pool().clone());
    ps.migrate().await.unwrap();
    let p = common::fresh_project(&ps, "snap").await;
    let v = ps.new_version(p.id).await.unwrap();
    (ps, v.id)
}

/// 一份**故意塞满三态**的快照。
fn three_state_snapshot(vid: Uuid) -> Snapshot {
    Snapshot {
        version_id: vid,
        seq: 1,
        extracted_by: Some(ExtractedBy {
            agent: "vendor-extractor".into(),
            version: "2.3.1".into(),
            round: 3,
            failed_groups: vec!["t_foo".into()],
            source: None,
        }),
        entity_types: vec![
            EntityType {
                key: "customer".into(),
                display_name: "客户".into(),
                description: Some("签约的企业主体".into()),
                aliases: vec!["用户".into()],
                parent: None,
                disjoint_with: vec!["supplier".into()],
                properties: vec![Property {
                    name: "客户编号".into(),
                    column: "customer_no".into(),
                    nullable: false,
                    note: Some("营业执照号".into()),
                    sample: Some("«已脱敏»".into()),
                }],
                evidence: vec![Evidence {
                    table: "t025".into(),
                    column: "customer_no".into(),
                    claim: "客户主键".into(),
                }],
            },
            EntityType {
                key: "subscription".into(),
                display_name: "客户订阅".into(),
                // ← 三态之一：None（没有说明）
                description: None,
                // ← 三态之二：空数组（没有别名，不是缺失）
                aliases: vec![],
                // ← 三态之三：父 key 存的是 **key 不是 id**
                parent: Some("customer".into()),
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
            EntityType {
                // ← **Some("") 必须与 None 区分开**
                key: "device".into(),
                display_name: "设备".into(),
                description: Some(String::new()),
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
        ],
        link_types: vec![LinkType {
            key: "customer_has_subscription".into(),
            from_type: "customer".into(),
            to_type: "subscription".into(),
            cardinality: "1:N".into(),
            semantic: "has".into(),
            evidence: vec![],
            confidence: None,
        }],
        action_types: vec![ActionType {
            key: "dispatch_ticket".into(),
            target: "工单".into(),
            parameters: serde_json::json!([{"name": "ticket_id", "type": "string"}]),
            preconditions: serde_json::json!([]),
            effects: vec![WriteBinding {
                system: "erp".into(),
                table: "t_ticket".into(),
                field: "status".into(),
                key_expr: serde_json::json!({"col": "ticket_id"}),
                value_map: [("dispatched".to_string(), "DISPATCHED".to_string())].into(),
                supported_values: vec!["dispatched".into()],
                conflict_field: Some("row_version".into()),
                evidence: vec![Evidence {
                    table: "t_ticket".into(),
                    column: "status".into(),
                    claim: "工单状态列".into(),
                }],
            }],
            permission: serde_json::json!({"roles": ["planner"]}),
            audit_policy: serde_json::json!({"retain_days": 365}),
            evidence: vec![],
        }],
        functions: vec![ontok::domain::snapshot::Function {
            key: "active_subscriptions".into(),
            signature: "(customer) -> int".into(),
            allowed_types: vec!["subscription".into()],
            evidence: vec![],
        }],
        // REQ-010：`Snapshot` 新增展示层字段，本文件只做**编译期**补齐，
        // 概念组本身的往返与三态覆盖放在 tests/test_concept_store.rs。
        concept_groups: vec![],
    }
}

#[tokio::test]
async fn test_snapshot_roundtrip() {
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    let snap = three_state_snapshot(vid);
    store.save_snapshot(&snap).await.unwrap();

    let back = store.get_snapshot(vid).await.unwrap().expect("快照应存在");
    // extracted_by 存在版本行上，单独取
    let mut back = back;
    back.extracted_by = store.get_extracted_by(vid).await.unwrap().flatten();

    assert_eq!(back, snap, "快照必须逐字段无损往返");
}

#[tokio::test]
async fn test_snapshot_three_states_survive() {
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    store.save_snapshot(&three_state_snapshot(vid)).await.unwrap();
    let back = store.get_snapshot(vid).await.unwrap().unwrap();

    // ① None 仍是 None
    assert_eq!(
        back.entity("subscription").unwrap().description,
        None,
        "description 的 None 不得变成空串"
    );
    // ② [] 仍是 []
    assert!(
        back.entity("subscription").unwrap().aliases.is_empty(),
        "空数组不得变成 null 或缺失"
    );
    // ③ Some("") 与 None 必须区分
    assert_eq!(
        back.entity("device").unwrap().description,
        Some(String::new()),
        "Some(\"\") 不得被吞成 None"
    );
    // ④ parent 存的是 key
    assert_eq!(back.entity("subscription").unwrap().parent.as_deref(), Some("customer"));
}

#[tokio::test]
async fn test_snapshot_records_extracted_by() {
    // AC-011：快照能区分「同一本体、不同 agent 版本」
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    let eb = ExtractedBy {
        agent: "vendor-extractor".into(),
        version: "2.3.1".into(),
        round: 3,
        failed_groups: vec![],
        source: None,
    };
    store.set_extracted_by(vid, &Some(eb.clone())).await.unwrap();

    let got = store.get_extracted_by(vid).await.unwrap().expect("版本应存在");
    assert_eq!(got, Some(eb), "必须记下具体版本号，不是 latest");
    assert_eq!(got.unwrap().version, "2.3.1");
}

#[tokio::test]
async fn test_extracted_by_none_means_human_edit() {
    // ADR-001：None = 人工修订，必须能与「抽取来的」区分开
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    store.set_extracted_by(vid, &None).await.unwrap();
    let got = store.get_extracted_by(vid).await.unwrap().expect("版本应存在");
    assert_eq!(got, None, "人工修订应记 None");
}

#[tokio::test]
async fn test_extracted_by_is_per_version_not_global() {
    // ADR-001 的存储层语义：`extracted_by` 挂在**版本行**上，不是项目上。
    // 两个版本各自独立 —— 这才让 apply 时的「不继承」有地方可落
    // （继承与否是 apply 的判定，不是存储的判定；apply 侧断言见 test_apply.rs）。
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let ps = PgProjectStore::new(store.pool().clone());
    let p = common::fresh_project(&ps, "eb").await;

    let v1 = ps.new_version(p.id).await.unwrap();
    let v2 = ps.new_version(p.id).await.unwrap();

    let eb = ExtractedBy {
        agent: "vendor-extractor".into(),
        version: "2.3.1".into(),
        round: 1,
        failed_groups: vec![],
        source: None,
    };
    store.set_extracted_by(v1.id, &Some(eb.clone())).await.unwrap();

    assert_eq!(
        store.get_extracted_by(v1.id).await.unwrap().flatten(),
        Some(eb.clone()),
        "v1 应记下抽取来源"
    );
    assert_eq!(
        store.get_extracted_by(v2.id).await.unwrap().flatten(),
        None,
        "**存储层不得自动继承** —— v2 没写就是 None，继承是 apply 的错误"
    );
}

#[tokio::test]
async fn test_frozen_version_rejects_snapshot_write() {
    // AC-006 的第一道：冻结后写快照必须被拒
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (ps, vid) = fresh_version(&store).await;
    let _ = &ps;

    store.save_snapshot(&three_state_snapshot(vid)).await.unwrap();
    ps.freeze(vid).await.unwrap();

    let mut snap = three_state_snapshot(vid);
    snap.entity_types[0].display_name = "改名了".into();
    let r = store.save_snapshot(&snap).await;
    assert!(
        matches!(r, Err(StoreError::VersionFrozen { .. })),
        "冻结版本必须拒绝写入，实际：{r:?}"
    );
}

#[tokio::test]
async fn test_snapshot_placeholder_structures_present_when_empty() {
    // 切片 1 action_types 恒空，但**结构必须在** ——
    // 缺结构与「空集合」是两回事，后者才有意义
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    let mut snap = three_state_snapshot(vid);
    snap.action_types.clear();
    snap.functions.clear();
    store.save_snapshot(&snap).await.unwrap();

    let back = store.get_snapshot(vid).await.unwrap().unwrap();
    assert!(back.action_types.is_empty(), "空集合仍应是空集合");
    assert!(back.functions.is_empty());
    // 而非「字段缺失导致读取报错」—— 能读出来本身就是证据
    assert_eq!(back.entity_types.len(), 3);
}

#[tokio::test]
async fn test_snapshot_duplicate_key_is_rejected() {
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    let mut snap = three_state_snapshot(vid);
    // 复制一个同 key 的类型
    let dup = snap.entity_types[0].clone();
    snap.entity_types.push(dup);
    let r = store.save_snapshot(&snap).await;
    assert!(
        matches!(r, Err(StoreError::KeyConflict(_))),
        "同版本内 key 重复必须报错，实际：{r:?}"
    );
}

#[tokio::test]
async fn test_snapshot_resave_replaces_not_appends() {
    // 二次保存应是**替换**而非追加
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    let mut snap = three_state_snapshot(vid);
    store.save_snapshot(&snap).await.unwrap();
    assert_eq!(store.get_snapshot(vid).await.unwrap().unwrap().entity_types.len(), 3);

    snap.entity_types.truncate(1);
    snap.link_types.clear(); // 一并删掉关系，验证删除也生效
    store.save_snapshot(&snap).await.unwrap();
    let back = store.get_snapshot(vid).await.unwrap().unwrap();
    assert_eq!(back.entity_types.len(), 1, "重存应替换掉旧行，而非追加");
    assert!(
        back.link_types.is_empty(),
        "重存时删掉的关系也应消失（否则是追加不是替换）：{:?}",
        back.link_types
    );
    assert_eq!(back.entity_types[0].key, "customer", "保留的应是第一个");
}

#[tokio::test]
async fn test_snapshot_resave_same_data_does_not_duplicate() {
    // **有判别力的形状**：连存两次**完全相同**的快照。
    //   - 若实现是「删旧写新」→ 结果 3 条（正确）
    //   - 若实现是「只追加」     → 结果 6 条（错误，一眼看出重复）
    // 早期版本的 resave 测试只在「缩小」后断言长度，
    // 追加式实现同样会给 1 —— 那个断言没有判别力。
    let store = PgStore::new(test_pool().await);
    store.migrate().await.unwrap();
    let (_ps, vid) = fresh_version(&store).await;

    let snap = three_state_snapshot(vid);
    store.save_snapshot(&snap).await.unwrap();
    store.save_snapshot(&snap).await.unwrap();
    store.save_snapshot(&snap).await.unwrap();

    let back = store.get_snapshot(vid).await.unwrap().unwrap();
    assert_eq!(
        back.entity_types.len(),
        3,
        "存 3 次同一份快照仍应是 3 条，重复说明是追加式写入：{:?}",
        back.entity_types.iter().map(|e| &e.key).collect::<Vec<_>>()
    );
    assert_eq!(back.link_types.len(), 1, "关系同样不该重复");
    assert_eq!(back.action_types.len(), 1, "动作类型同样不该重复");
    assert_eq!(back.functions.len(), 1, "函数同样不该重复");
}
