//! REQ-055 · **建模轨投影**测试（AC-001 / AC-002）。
//!
//! 测试名带 `test_model_view_` 前缀。
//!
//! # 本文件要证明的是「分离」，不是「建模轨有数据」
//!
//! 「建模轨输出含建模术语」单独一条**不构成**分离的证据 ——
//! 那只说明建模轨有数据。真正的命题是**同一条判据、同一份快照、相反的判决**：
//!
//! - 建模轨输出 ⇒ [`scan_modeling_terms`] **命中**（建模术语是要被裁决的内容）；
//! - 客户视图输出 ⇒ [`scan_modeling_terms`] **空**（建模术语不得泄露给客户）。
//!
//! 两条由**同一个 fixture 快照**驱动，见
//! [`test_model_view_same_snapshot_terms_land_in_model_track_only`]。

use ontok::domain::model_view::{project_model_track, ModelTrackView};
use ontok::domain::snapshot::{
    EntityType, Evidence, ExtractedBy, LinkType, Property, ProvenanceSource, Snapshot,
};
use ontok::domain::view::{
    project, scan_modeling_terms, CustomerView, EntityTypeView, LinkTypeView, PropertyTypeView,
    SnapshotView,
};

// ── fixture ──────────────────────────────────────────────

/// 一份**刻意富含建模术语**的快照。
///
/// 每一处建模术语都要能在下游被指名道姓地问出来：
/// - `key` = `customer` / `subscription`（系统标识）
/// - 列名 `customer_pk` 含 `pk`；`customer_id` 含外键语义
/// - `cardinality` = `1:N`（基数记号）
/// - `disjoint_with` = `["supplier"]`（不相交）
/// - `evidence` 非空（证据）
/// - `extracted_by` 非空（抽取归属）
fn fixture_snapshot() -> Snapshot {
    let mut s = Snapshot::new(uuid::Uuid::nil(), 7);
    s.extracted_by = Some(ExtractedBy {
        agent: "ontok-introspect".into(),
        version: "0.3.1".into(),
        round: 2,
        failed_groups: vec!["t_subscription_probe".into()],
        source: Some(ProvenanceSource::Declared),
    });
    s.entity_types = vec![
        EntityType {
            key: "customer".into(),
            display_name: "客户".into(),
            description: Some("签约的企业主体".into()),
            aliases: vec!["用户".into()],
            parent: None,
            disjoint_with: vec!["supplier".into()],
            properties: vec![Property {
                name: "客户编号".into(),
                column: "customer_pk".into(),
                nullable: false,
                note: Some("营业执照号".into()),
                sample: Some("«已脱敏»".into()),
            }],
            evidence: vec![Evidence {
                table: "t_customer".into(),
                column: "customer_no".into(),
                claim: "主键列名与注释一致".into(),
            }],
        },
        EntityType {
            key: "subscription".into(),
            display_name: "客户订阅".into(),
            description: Some("客户购买的调用额度套餐".into()),
            aliases: vec![],
            parent: Some("customer".into()),
            disjoint_with: vec![],
            properties: vec![Property {
                name: "客户编号".into(),
                column: "customer_id".into(),
                nullable: false,
                note: None,
                sample: Some("«已脱敏»".into()),
            }],
            evidence: vec![],
        },
    ];
    s.link_types = vec![LinkType {
        key: "customer_has_subscription".into(),
        from_type: "customer".into(),
        to_type: "subscription".into(),
        cardinality: "1:N".into(),
        semantic: "has".into(),
        evidence: vec![Evidence {
            table: "t_subscription".into(),
            column: "customer_id".into(),
            claim: "外键指向 t_customer".into(),
        }],
        confidence: None,
    }];
    s
}

/// 快照 → 客户视图的投影输入。
///
/// ⚠️ 这是 `cli.rs::snapshot_to_view` 的**测试内同形副本**，存在的原因只有一个：
/// 让**同一份 `Snapshot` 实例**同时喂给两个投影（建模轨收 `&Snapshot`，
/// 客户视图收 `SnapshotView`）。生产路径用 `cli.rs` 那一份，这里不参与生产。
fn to_customer_input(s: &Snapshot) -> SnapshotView {
    SnapshotView {
        entity_types: s
            .entity_types
            .iter()
            .map(|e| EntityTypeView {
                key: e.key.clone(),
                display_name: e.display_name.clone(),
                description: e.description.clone(),
                aliases: e.aliases.clone(),
                parent: e.parent.clone(),
                disjoint_with: e.disjoint_with.clone(),
                properties: e
                    .properties
                    .iter()
                    .map(|p| PropertyTypeView {
                        name: p.name.clone(),
                        column: p.column.clone(),
                        nullable: p.nullable,
                        note: p.note.clone(),
                        sample: p.sample.clone(),
                    })
                    .collect(),
                evidence: e.evidence.iter().map(|ev| serde_json::to_value(ev).unwrap()).collect(),
            })
            .collect(),
        link_types: s
            .link_types
            .iter()
            .map(|l| LinkTypeView {
                key: l.key.clone(),
                from_type: l.from_type.clone(),
                to_type: l.to_type.clone(),
                cardinality: l.cardinality.clone(),
                semantic: l.semantic.clone(),
            })
            .collect(),
    }
}

// ── AC-001 · 判别力自证（两条必须同时成立）──────────────────

/// 【AC-001 · 建模轨这一条】**含建模术语的快照 ⇒ 建模轨输出含建模术语**。
#[test]
fn test_model_view_output_carries_modeling_terms() {
    let snap = fixture_snapshot();
    let mv: ModelTrackView = project_model_track(&snap);
    let json = serde_json::to_string(&mv).unwrap();

    // ① 守门器**有反应**：建模轨这份输出确实被判为含建模术语。
    //
    // ⚠️ 只断言这一条不够 —— 「守门器叫了」不能证明「叫的是哪几个词」，
    // 所以下面 ② 逐个指名道姓地在**内容**里核对。
    //
    // ⚠️ 另一个口径：`english_term_hit` **只返回第一个命中词**
    // （`view.rs` 里返回的是 `Option<&'static str>`），
    // 所以不能断言这些词全都出现在 `leaked` 列表里 —— 那是守门器的
    // 报告口径（只报首犯），不是「建模轨缺词」的证据。这里因此分两层断言：
    // `leaked` 判「有没有被抓住」，JSON 文本判「到底有没有这些词」。
    let leaked = scan_modeling_terms(&json);
    assert!(
        !leaked.is_empty(),
        "建模轨输出竟然不含任何建模术语 —— 那它和客户视图有什么区别？\n实际输出：{json}"
    );

    // ② 逐个指名道姓地在建模轨输出里找这些词。
    for (term, where_) in [
        ("\"key\"", "实体层系统标识的键名"),
        ("customer_pk", "列映射里的主键列名"),
        ("\"cardinality\"", "关系层基数的键名"),
        ("\"evidence\"", "证据的键名"),
        ("\"extracted_by\"", "抽取归属的键名"),
        ("\"disjoint_with\"", "不相交的键名"),
    ] {
        assert!(
            json.contains(term),
            "建模轨输出里找不到 {term}（{where_}）；实际输出：{json}"
        );
    }
}

/// 【AC-001 · 客户视图这一条】**同一份快照 ⇒ 客户视图输出不含建模术语**。
#[test]
fn test_customer_view_output_carries_none_of_them() {
    let snap = fixture_snapshot();
    let cv: CustomerView = project(&to_customer_input(&snap));
    let json = serde_json::to_string(&cv).unwrap();

    let leaked = scan_modeling_terms(&json);
    assert!(leaked.is_empty(), "客户视图泄露了建模术语：{leaked:?}\n实际输出：{json}");
}

/// 【AC-001 的核心】**同一条判据、同一份快照、相反的判决**。
///
/// 两条单独验都不构成「分离」的证据 —— 合并成一条才构成：
/// 变量只有「用哪个投影」，其余（快照、判据、序列化）全部相同。
#[test]
fn test_model_view_same_snapshot_terms_land_in_model_track_only() {
    let snap = fixture_snapshot();

    let model_json = serde_json::to_string(&project_model_track(&snap)).unwrap();
    let customer_json = serde_json::to_string(&project(&to_customer_input(&snap))).unwrap();

    let model_hits = scan_modeling_terms(&model_json);
    let customer_hits = scan_modeling_terms(&customer_json);

    assert!(!model_hits.is_empty(), "建模轨这一侧必须是脏的");
    assert!(
        customer_hits.is_empty(),
        "客户视图这一侧必须是干净的：{customer_hits:?}\n实际输出：{customer_json}"
    );

    // ⚠️ **判别力必须来自「数据」，不只是「字段名」**。
    //
    // 只断言 `!model_hits.is_empty()` 是不够的：建模轨的结构里
    // `key` / `evidence` / `extracted_by` 这些**字段名本身**就是建模术语，
    // 于是「把内容全清空、只留字段名」的退化实现也能蒙混过关。
    //
    // `未翻译的基数记号 1:N` 这条命中走的是**形态守门**
    // （`view.rs` 的 `cardinality_notation_hit`：单字符 + 分隔符 + 单字符），
    // **字段名永远触发不了它** —— 只有值 `1:N` 能。
    // 所以要求命中里有一条来自数据，就把「只剩字段名」那种退化排除掉了。
    assert!(
        model_hits.iter().any(|h| h.contains("1:N")),
        "建模轨的命中里必须有一条来自**数据**（基数记号 1:N），不能只靠字段名撑起来；\
         实际命中：{model_hits:?}\n实际输出：{model_json}"
    );
    // 对称地钉住客户视图：它连 `1:N` 这种形态都不能有
    //（客户视图把基数翻成了人话「一对多」）。
    assert!(
        !customer_json.contains("1:N"),
        "客户视图里出现了未翻译的基数记号：{customer_json}"
    );
}

// ── AC-001 · 字段级对照：客户视图丢掉的，正是建模轨留住的 ────

/// 建模轨**保留**客户视图逐条注明丢弃的那几栏。
#[test]
fn test_model_track_keeps_what_customer_view_drops() {
    let mv = project_model_track(&fixture_snapshot());
    let v = serde_json::to_value(&mv).unwrap();
    let root = v.as_object().unwrap();

    // 顶层：抽取归属 + 两个占位结构
    assert!(root.contains_key("extracted_by"), "顶层缺 extracted_by");
    assert!(root.contains_key("action_types"), "顶层缺 action_types（§5.5 占位）");
    assert!(root.contains_key("functions"), "顶层缺 functions（§5.5 占位）");
    assert_eq!(root["seq"], serde_json::json!(7));
    // 抽取归属的**内容**也要在（不只是那个键存在）—— FDE 判断
    // 「这份建模是谁产的、哪一轮、哪些分组失败了」全靠这几项。
    assert_eq!(root["extracted_by"]["agent"], serde_json::json!("ontok-introspect"));
    assert_eq!(root["extracted_by"]["version"], serde_json::json!("0.3.1"));
    assert_eq!(root["extracted_by"]["round"], serde_json::json!(2));
    assert_eq!(
        root["extracted_by"]["failed_groups"],
        serde_json::json!(["t_subscription_probe"]),
        "失败的分组必须带出来：漏记等于假装全成功"
    );

    let e0 = &root["entity_types"][0];
    for k in ["key", "disjoint_with", "evidence"] {
        assert!(e0.as_object().unwrap().contains_key(k), "实体层缺 `{k}`");
    }
    assert_eq!(e0["key"], serde_json::json!("customer"));
    assert_eq!(e0["disjoint_with"], serde_json::json!(["supplier"]));

    // 属性层：列映射
    let p0 = &e0["properties"][0];
    assert_eq!(p0["column"], serde_json::json!("customer_pk"), "列映射必须原样带出");

    // 关系层：基数与语义保留**原记号**，不翻成人话
    let l0 = &root["link_types"][0];
    assert_eq!(l0["cardinality"], serde_json::json!("1:N"), "基数必须是原记号，不是人话");
    assert_eq!(l0["semantic"], serde_json::json!("has"), "语义必须是原记号，不是人话");
    assert!(l0.as_object().unwrap().contains_key("evidence"), "关系层缺 evidence");
}

/// 客户视图**不**含那几栏 —— 与上一条互为对照。
#[test]
fn test_customer_view_omits_the_same_fields() {
    let cv = project(&to_customer_input(&fixture_snapshot()));
    let v = serde_json::to_value(&cv).unwrap();
    let root = v.as_object().unwrap();

    for k in ["key", "evidence", "disjoint_with", "column", "cardinality", "extracted_by"] {
        assert!(!root.contains_key(k), "客户视图顶层不该有 `{k}`");
    }
    let e0 = &root["objects"][0];
    let e0o = e0.as_object().unwrap();
    for k in ["key", "evidence", "disjoint_with"] {
        assert!(!e0o.contains_key(k), "客户视图实体层不该有 `{k}`");
    }
    let p0 = e0o["properties"][0].as_object().unwrap();
    assert!(!p0.contains_key("column"), "客户视图属性层不该有 `column`");
    let l0 = root["relations"][0].as_object().unwrap();
    for k in ["cardinality", "evidence", "key"] {
        assert!(!l0.contains_key(k), "客户视图关系层不该有 `{k}`");
    }
}

// ── AC-002 · 不破坏既有守门 ──────────────────────────────

/// 建模轨的引入**没有放宽**客户视图的守门：黑名单词表一条没少。
///
/// 判据是**词表本身**（`MODELING_TERMS`），因为「守门被放宽」最常见的形态
/// 就是从词表里删词 —— 那是运行时守门器失守最省事也最隐蔽的做法。
#[test]
fn test_model_view_leaves_blacklist_intact() {
    use ontok::domain::view::MODELING_TERMS;
    for term in [
        "pk",
        "primary_key",
        "cardinality",
        "ordinal",
        "evidence",
        "disjoint",
        "extracted_by",
        "proposed_by",
        "approved_by",
        "key",
        "外键",
        "基数",
        "主键",
        "证据",
    ] {
        assert!(
            MODELING_TERMS.contains(&term),
            "建模术语黑名单被削弱了：`{term}` 不在词表里了"
        );
    }
}

/// 守门器**仍然**拦得住客户视图里的建模术语（不是「词表还在」就算数）。
#[test]
fn test_model_view_does_not_weaken_the_gate() {
    for term in ["primary_key", "cardinality", "ordinal", "evidence", "disjoint"] {
        let probe = format!("{{\"note\":\"{term}\"}}");
        assert!(
            !scan_modeling_terms(&probe).is_empty(),
            "守门器放过含有 `{term}` 的客户视图 —— 它被削弱了"
        );
    }
    // 中文守门档
    for term in ["外键", "基数", "主键", "证据"] {
        let probe = format!("{{\"note\":\"这条说明提到了{term}\"}}");
        assert!(
            !scan_modeling_terms(&probe).is_empty(),
            "守门器放过含有 `{term}` 的客户视图 —— 它被削弱了"
        );
    }
}

// ── 建模轨自身的取值纪律 ──────────────────────────────────

/// 空集合也要把**键**留在输出里 —— FDE 靠「有没有这一栏」判断
/// 「快照层根本没给」与「给了但是空的」的区别（快照层三态语义）。
#[test]
fn test_model_track_keeps_empty_collections_as_keys() {
    let mut snap = fixture_snapshot();
    snap.extracted_by = None; // 三态之一：旧数据 / 人工修订 —— 不猜
    snap.action_types = vec![];
    snap.functions = vec![];
    snap.entity_types[1].evidence = vec![];

    let v = serde_json::to_value(project_model_track(&snap)).unwrap();
    assert!(v.as_object().unwrap().contains_key("extracted_by"), "`None` 也必须留键");
    assert_eq!(v["extracted_by"], serde_json::Value::Null, "三态必须是 null，不是缺字段");
    assert_eq!(v["entity_types"][1]["evidence"], serde_json::json!([]), "空证据是空数组");
}

/// 建模轨**不伪造**快照层没有的东西：没有 `pk` 字段就别造一个。
///
/// 快照层 `Property` 只有 `column` 与 `nullable`。凭空补一个 `pk: bool`
/// 会给 FDE 一个**假的确定性信号** —— 比「这一栏不存在」危险得多。
#[test]
fn test_model_track_invents_no_primary_key_field() {
    let mv = project_model_track(&fixture_snapshot());
    let p = &mv.entity_types[0].properties[0];
    let v = serde_json::to_value(p).unwrap();
    let o = v.as_object().unwrap();
    assert!(!o.contains_key("pk"), "快照层没有独立 pk 位，建模轨不许伪造");
    assert!(!o.contains_key("primary_key"), "同上");
    // 能给 FDE 判主键的两样都在
    assert_eq!(o["column"], serde_json::json!("customer_pk"));
    assert_eq!(o["nullable"], serde_json::json!(false));
}

/// 建模轨**不碰**快照本身（纯投影：只读不改）。
#[test]
fn test_model_track_does_not_mutate_snapshot() {
    let snap = fixture_snapshot();
    let before = serde_json::to_value(&snap).unwrap();
    let _ = project_model_track(&snap);
    assert_eq!(before, serde_json::to_value(&snap).unwrap(), "投影必须是只读的");
}
