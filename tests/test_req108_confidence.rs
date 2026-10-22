//! REQ-108 · **分档路由与两道视图的隔离**（AC-009 / AC-010）
//!
//! # 这个文件证明什么
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-009 | 四档去向：高/中进提案 · 低进待查 · 拒绝不提案；三份互不相交（`route_items`） |
//! | AC-010 | **客户视图零泄漏**（JSON 里 `confidence` / `score` / `band` 一个都没有）+ **建模视图必须有** |
//! | AC-008 | 缺 `method` 的条目在入口就被拒（`NewItem::from_json`） |
//! | AC-011 的显示层 | `render_confidence_table` 的四档计数与档位标签 |
//!
//! # 为什么「零泄漏」是结构性的，而不是一个 grep 结论
//!
//! 客户视图的投影 [`ontok::domain::view::project`] 收的是
//! [`ontok::domain::view::SnapshotView`] —— 那个类型里**没有** `confidence` 字段
//! （本 REQ 也没有加它，`view.rs` 是红线）。所以「客户视图不显示置信度」
//! 不是靠这里不写，而是靠**它拿不到那份数据**。
//! 本文件只是把这条结构事实钉成可执行的判据。

use ontok::domain::confidence::{
    score, Band, Confidence, DataEvidence, SampleFacts, ScoreInput, TargetKey, TypeFamily, METHOD,
};
use ontok::domain::model_view::project_model_track;
use ontok::domain::snapshot::{Evidence, LinkType, Snapshot};
use ontok::domain::view::project;

/// 一条「全部信号都满」的分数（拿来当真数据用）。
fn perfect() -> Confidence {
    score(&ScoreInput {
        column: "customer_id".into(),
        to_table: "customers".into(),
        from_type: TypeFamily::Str,
        to_type: Some(TypeFamily::Str),
        target_key: TargetKey::Primary,
        data: Some(SampleFacts { sampled: 5, matched: 5, digest: Some("sha256:aa".into()), reason: None }),
        composite_target_key: false,
        polymorphic: false,
        self_ref: false,
    })
}

fn link(key: &str, confidence: Option<Confidence>) -> LinkType {
    LinkType {
        key: key.into(),
        from_type: "orders".into(),
        to_type: "customers".into(),
        cardinality: "N:1".into(),
        semantic: "belongs_to".into(),
        evidence: vec![Evidence {
            table: "orders".into(),
            column: "customer_id".into(),
            claim: "命名约定".into(),
        }],
        confidence,
        join_verified: None,
        join_evidence: None,
    }
}

fn snapshot_with(confidence: Option<Confidence>) -> Snapshot {
    let mut s = Snapshot::new(uuid::Uuid::nil(), 2);
    s.entity_types = vec![
        ontok::domain::snapshot::EntityType {
            key: "orders".into(),
            display_name: "订单".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![],
        },
        ontok::domain::snapshot::EntityType {
            key: "customers".into(),
            display_name: "客户".into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![],
        },
    ];
    s.link_types = vec![link("orders_customer_id__customers", confidence)];
    s
}

// ── AC-010 · 客户视图零泄漏 ────────────────────────────────

/// **客户视图（JSON）里不许出现 `confidence` / `score` / `band`**。
///
/// 变异自证：给 `SnapshotView` / `LinkTypeView` 加上 `confidence` 字段并投影出去，
/// 本测试立刻红 —— 那条路径会直接把分数漏给客户。
#[test]
fn test_req108_ac010_customer_view_has_zero_confidence_leak() {
    let snap = snapshot_with(Some(perfect()));
    let cv = project(&ontok::cli::snapshot_to_view(&snap));
    let json = serde_json::to_string(&cv).unwrap();

    for forbidden in ["confidence", "\"score\"", "\"band\"", "evidence-scorecard"] {
        assert!(
            !json.contains(forbidden),
            "AC-010：客户视图里出现了 `{forbidden}` —— 置信度是内部质量指标，不对客户承诺\n{json}"
        );
    }
    // 阴性对照：客户视图**确实**有内容（否则上面是「空 == 空」）
    assert!(json.contains("订单") && json.contains("客户"), "客户视图不该是空的：{json}");
    assert!(json.contains("relations"), "客户视图应当有 relations 段：{json}");
}

/// **建模视图必须有**：每条关系都带 `confidence` 键（旧版本是 `null`，不是缺字段）。
#[test]
fn test_req108_ac010_model_view_carries_confidence_on_every_link() {
    let mv = project_model_track(&snapshot_with(Some(perfect())));
    let v = serde_json::to_value(&mv).unwrap();
    let links = v["link_types"].as_array().unwrap();
    assert_eq!(links.len(), 1);
    assert!(
        links[0].as_object().unwrap().contains_key("confidence"),
        "AC-010：建模视图的关系必须带 confidence 键"
    );
    assert_eq!(links[0]["confidence"]["score"], serde_json::json!(1.0));
    assert_eq!(links[0]["confidence"]["band"], serde_json::json!("high"));
    assert_eq!(links[0]["confidence"]["method"], serde_json::json!(METHOD));
    assert_eq!(
        links[0]["confidence"]["data_evidence"],
        serde_json::json!("sampled")
    );
    // 证据（聚合量）也在：审核者要能看出「5/5」还是「没采」
    let sig = &links[0]["confidence"]["signals"][0];
    assert_eq!(sig["sampled"], serde_json::json!(5));
    assert_eq!(sig["matched"], serde_json::json!(5));
    assert!(sig["sample_digest"].as_str().unwrap().starts_with("sha256:"));

    // 旧版本（没有分数）⇒ 键在、值为 null —— 不伪造一个 0.0
    let legacy = project_model_track(&snapshot_with(None));
    let lv = serde_json::to_value(&legacy).unwrap();
    assert!(lv["link_types"][0].as_object().unwrap().contains_key("confidence"));
    assert_eq!(lv["link_types"][0]["confidence"], serde_json::Value::Null);
}

/// 同一份快照：**建模视图有分数、客户视图没有** —— 变量只有「用哪个投影」。
#[test]
fn test_req108_ac010_same_snapshot_two_projections_opposite_verdicts() {
    let snap = snapshot_with(Some(perfect()));
    let model_json = serde_json::to_string(&project_model_track(&snap)).unwrap();
    let customer_json = serde_json::to_string(&project(&ontok::cli::snapshot_to_view(&snap))).unwrap();
    assert!(model_json.contains("\"confidence\""), "建模轨必须有");
    assert!(!customer_json.contains("confidence"), "客户视图必须没有：{customer_json}");
}

// ── AC-009 · 四档去向 ─────────────────────────────────────

/// 四档路由：高/中进提案 · 低进待查 · 拒绝不提案（三份互不相交）。
#[test]
fn test_req108_ac009_bands_route_to_the_three_lists() {
    use ontok::extract::propose::route_items;
    let mk = |name: &str| {
        serde_json::json!({
            "kind": "add_link_type",
            "target": name,
            "payload": {
                "key": name, "from_type": "a", "to_type": "b",
                "cardinality": "N:1", "semantic": "belongs_to",
                "evidence": [{"table": "a", "column": "b_id", "claim": "x"}]
            }
        })
    };
    let items = vec![
        serde_json::json!({"kind": "add_entity_type", "target": "客户", "payload": {"key": "customers"}}),
        mk("high"),
        mk("middle"),
        mk("low"),
        mk("rejected"),
    ];
    let scored = vec![
        (1usize, score(&ScoreInput { column: "customer_id".into(), to_table: "customers".into(), from_type: TypeFamily::Str, to_type: Some(TypeFamily::Str), target_key: TargetKey::Primary, data: Some(SampleFacts { sampled: 5, matched: 5, digest: None, reason: None }), composite_target_key: false, polymorphic: false, self_ref: false })),
        (2usize, score(&ScoreInput { column: "customer_id".into(), to_table: "customers".into(), from_type: TypeFamily::Str, to_type: Some(TypeFamily::Str), target_key: TargetKey::Primary, data: Some(SampleFacts { sampled: 5, matched: 3, digest: None, reason: None }), composite_target_key: false, polymorphic: false, self_ref: false })),
        (3usize, score(&ScoreInput { column: "customer_id".into(), to_table: "customers".into(), from_type: TypeFamily::Str, to_type: Some(TypeFamily::Str), target_key: TargetKey::Primary, data: None, composite_target_key: false, polymorphic: false, self_ref: false })),
        (4usize, score(&ScoreInput { column: "batch_id".into(), to_table: "customers".into(), from_type: TypeFamily::Str, to_type: Some(TypeFamily::Str), target_key: TargetKey::Primary, data: Some(SampleFacts { sampled: 5, matched: 0, digest: None, reason: None }), composite_target_key: false, polymorphic: false, self_ref: false })),
    ];
    // 先确认这四条确实落在四档（否则下面的路由断言是空的）
    let bands: Vec<Band> = scored.iter().map(|(_, c)| c.band).collect();
    assert_eq!(bands, vec![Band::High, Band::Middle, Band::Low, Band::Rejected]);

    let r = route_items(items, &scored);
    let names = |v: &[serde_json::Value]| -> Vec<String> {
        v.iter()
            .filter(|i| i["kind"] == "add_link_type")
            .map(|i| i["target"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names(&r.kept), vec!["high", "middle"], "高+中进提案");
    assert_eq!(names(&r.pending), vec!["low"], "低进待查");
    assert_eq!(names(&r.rejected), vec!["rejected"], "拒绝不提案");
    assert_eq!(r.counts, ontok::domain::confidence::BandCounts { high: 1, middle: 1, low: 1, rejected: 1 });
    // 实体条目留在提案里（分档不碰非关系条目）
    assert!(r.kept.iter().any(|i| i["kind"] == "add_entity_type"));
    // **「不提案」档里的关系在 kept 里一条都找不到**（AC-009 的原话）
    assert!(
        r.kept.iter().all(|i| i["target"] != "rejected"),
        "拒绝档的关系出现在了提案里"
    );
}

// ── AC-008 · method 必填 ─────────────────────────────────

/// 缺 `method` 的条目在**入口**（`NewItem::from_json`）就被拒 —— import 因此非 0 退出。
#[test]
fn test_req108_ac008_import_rejects_item_without_method() {
    let item = serde_json::json!({
        "kind": "add_link_type",
        "target": "订单→客户",
        "payload": {
            "key": "orders_customer_id__customers",
            "from_type": "orders", "to_type": "customers",
            "cardinality": "N:1", "semantic": "belongs_to"
        },
        "confidence": {"score": 0.9}
    });
    let err = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap_err().to_string();
    assert!(err.contains("method"), "错误里要点名 method：{err}");
    assert!(err.contains("不可复核"), "错误里要说清「没有 method 的分数不可复核」：{err}");

    // 有 method 的同一份 ⇒ 通过，且分数搬进了 payload（apply 那条路只读 payload）
    let mut ok = item.clone();
    ok["confidence"]["method"] = serde_json::json!(METHOD);
    // ★ A-10：有 confidence 就必须有证据链 —— 补一份最小 signals 只是让 fixture 完整，
    //   本用例断言的东西（method 必填 / 分数搬进 payload / band 派生校验）一条没变。
    // ★ A-13（R5）：`data_evidence` 缺省 = sampled ⇒ 这份最小证据链必须带
    //   `sample_hit_rate`（含 sampled/matched），否则 fixture 自身就是违规形状。
    ok["confidence"]["signals"] = serde_json::json!([
        {"name": "sample_hit_rate", "value": 1.0, "weight": 0.45, "sampled": 5, "matched": 5},
        {"name": "name_match", "value": 1.0, "weight": 0.2}
    ]);
    let parsed = ontok::db::proposal::NewItem::from_json(&ok, 0).unwrap();
    assert_eq!(parsed.payload["confidence"]["method"], serde_json::json!(METHOD));

    // band 与分数对不上 ⇒ 也拒（派生量被改过）
    let mut tampered = item.clone();
    tampered["confidence"]["method"] = serde_json::json!(METHOD);
    tampered["confidence"]["band"] = serde_json::json!("low");
    assert!(ontok::db::proposal::NewItem::from_json(&tampered, 0).is_err());
}

// ── A-10（R1 修复）· 有 confidence 就必须有证据链 ────────────

/// R1 的原样复现：`{"score":0.99,"method":"evidence-scorecard-v1"}`（**没有 signals**）
/// 在条目入口就被拒 —— `propose import` 因此非 0 退出。
///
/// 这就是 BA 裁定 A-10 的判据 1：删字段和改数字是同一类漏洞（都只要一行），
/// 不能靠「文件里写了 veto」兜底，因为这里连 `signals` 都没写。
///
/// 变异自证：把 `confidence::from_untrusted` 里 `signals` 的 `None | Null` 分支
/// 改回 `Vec::new()`（旧实现），本测试立刻红 —— 那时一份无证据的 0.99 会成功入库。
#[test]
fn test_req108_ac014_item_with_confidence_but_no_signals_is_rejected() {
    let mut item = serde_json::json!({
        "kind": "add_link_type",
        "target": "订单→客户",
        "payload": {
            "key": "orders_customer_id__customers",
            "from_type": "orders", "to_type": "customers",
            "cardinality": "N:1", "semantic": "belongs_to"
        },
        "confidence": {"score": 0.99, "method": METHOD}
    });

    let err = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap_err().to_string();
    assert!(err.contains("有置信度但缺证据链"), "文案要说清「有置信度但缺证据链」：{err}");
    assert!(err.contains("不可复核"), "{err}");

    // 空数组同样拒（「写了证据链但一条都没有」与「没写」等价）
    item["confidence"]["signals"] = serde_json::json!([]);
    let err = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap_err().to_string();
    assert!(err.contains("有置信度但缺证据链"), "{err}");
    assert!(err.contains("空数组"), "空数组与缺失要能区分开：{err}");

    // 阴性对照：同一份条目**带上**证据链就放行（否则上面只是「什么都拒」）
    // ★ A-13（R5）：仍然带 `sample_hit_rate`（缺省 data_evidence = sampled）。
    item["confidence"]["signals"] = serde_json::json!([
        {"name": "sample_hit_rate", "value": 1.0, "weight": 0.45, "sampled": 5, "matched": 5},
        {"name": "name_match", "value": 1.0, "weight": 0.2}
    ]);
    let parsed = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap();
    assert_eq!(parsed.payload["confidence"]["score"], serde_json::json!(0.99));
}

/// **A-13（R5）· 声明了采样证据但删掉 `sample_hit_rate` ⇒ 条目入口就拒**。
///
/// QA 实测的攻击链（原样）：`score 0.99` + `data_evidence: sampled` +
/// 只剩 `name_match` / `type_compatible` / `target_is_key` / `not_all_null`。
/// 旧行为是 `import` / `decide` / `apply` 全部 EXIT=0，快照里写成 `0.99 / high`。
///
/// 这条测试走的是**与 `propose import` 相同的入口**（`NewItem::from_json`
/// → `confidence::from_untrusted`），所以断言「import 会 exit≠0」不是另一套逻辑。
///
/// 变异自证：删掉 `validate_self_evidence` 第 4 段（R5），本测试立刻红。
#[test]
fn test_req108_ac013_sampled_without_sample_hit_rate_is_rejected_at_import() {
    let mut item = serde_json::json!({
        "kind": "add_link_type",
        "target": "订单→客户",
        "payload": {
            "key": "orders_customer_id__customers",
            "from_type": "orders", "to_type": "customers",
            "cardinality": "N:1", "semantic": "belongs_to"
        },
        "confidence": {
            "score": 0.99,
            "method": METHOD,
            "data_evidence": "sampled",
            "signals": [
                {"name": "name_match", "value": 1.0, "weight": 0.2},
                {"name": "type_compatible", "value": 1.0, "weight": 0.1},
                {"name": "target_is_key", "value": 1.0, "weight": 0.1},
                {"name": "not_all_null", "value": 1.0, "weight": 0.15}
            ]
        }
    });
    let err = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap_err().to_string();
    assert!(
        err.contains("声明了采样证据但 signals 里没有 sample_hit_rate"),
        "文案必须点名缺失的信号：{err}"
    );

    // 反向：声明没采过样，却带一条采样信号 ⇒ 同样拒
    item["confidence"]["data_evidence"] = serde_json::json!("none");
    item["confidence"]["signals"] = serde_json::json!([
        {"name": "sample_hit_rate", "value": 1.0, "weight": 0.45, "sampled": 5, "matched": 5}
    ]);
    let err = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap_err().to_string();
    assert!(err.contains("data_evidence = none"), "{err}");
    assert!(err.contains("却又摆出一份采样证据"), "{err}");

    // 阴性对照：`--no-sample` 的形状（none + 无采样信号）⇒ 放行
    item["confidence"]["signals"] = serde_json::json!([
        {"name": "name_match", "value": 1.0, "weight": 0.2},
        {"name": "type_compatible", "value": 1.0, "weight": 0.1},
        {"name": "target_is_key", "value": 1.0, "weight": 0.1}
    ]);
    item["confidence"]["score"] = serde_json::json!(0.40);
    item["confidence"]["band"] = serde_json::json!("low");
    assert!(
        ontok::db::proposal::NewItem::from_json(&item, 0).is_ok(),
        "`--no-sample` 的真产出形状不许被误伤"
    );
}

/// **A-10 的对照（不许误伤）**：老的人工提案 —— **完全没有 `confidence` 字段** ——
/// 必须继续成功导入。
///
/// 两者能精确区分，靠的是**位置**而不是「有没有 `signals`」：
/// `proposal::NewItem::from_json`（以及 `apply::link_from_item`）只在
/// `confidence` **存在且非 null** 时才调 `from_untrusted`；没有这个键的条目
/// 根本走不到证据链校验。`confidence: null` 与缺失同义（JSON 的 null = 没有分数，
/// 不是「0 分的分数」），见交付报告。
#[test]
fn test_req108_ac014_legacy_item_without_confidence_still_imports() {
    let item = serde_json::json!({
        "kind": "add_link_type",
        "target": "订单→客户",
        "payload": {
            "key": "orders_customer_id__customers",
            "from_type": "orders", "to_type": "customers",
            "cardinality": "N:1", "semantic": "belongs_to"
        }
    });
    let parsed = ontok::db::proposal::NewItem::from_json(&item, 0).unwrap();
    assert!(
        !parsed.payload.as_object().unwrap().contains_key("confidence"),
        "老提案不该被凭空补一个 confidence"
    );

    // `null` 与「没有这个键」同义：同样放行、同样不补字段
    let mut nulled = item.clone();
    nulled["confidence"] = serde_json::Value::Null;
    let parsed = ontok::db::proposal::NewItem::from_json(&nulled, 0).unwrap();
    assert!(!parsed.payload.as_object().unwrap().contains_key("confidence"));
}

// ── AC-015 · `importable: false` 是硬闸（第 2 轮）────────────

/// QA 的 N2 实测：原样 `propose import` `proposal-rejected.json` → **EXIT=0**。
///
/// 这条把它钉死成判据：带 `importable: false` ⇒ 导入必须被拒，且理由要说出
/// 「这是被拒绝档」；缺省 / `true` 仍然放行（主提案与待查清单）。
///
/// 变异自证：把 `ensure_importable` 里那段 `bail!` 删掉（回到「声明不生效」），
/// 本测试立刻红 —— 那时的表现就是 10 条已知错误被一键灌进本体。
#[test]
fn test_req108_ac015_importable_false_is_a_hard_gate() {
    let p = std::path::Path::new("proposal-rejected.json");
    let err = ontok::cli::ensure_importable(p, &serde_json::json!({"importable": false}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("importable = false"), "要点名是哪个字段：{err}");
    assert!(err.contains("被拒绝档"), "要说清「这是被拒绝档」：{err}");
    assert!(err.contains("不能导入"), "{err}");

    // 缺省（REQ-108 之前的产物）与 `true`（主提案 / 待查清单）⇒ 放行
    assert!(ontok::cli::ensure_importable(p, &serde_json::json!({"items": []})).is_ok());
    assert!(ontok::cli::ensure_importable(p, &serde_json::json!({"importable": true})).is_ok());

    // 说不清的取值一律拒（fail-closed）
    for bad in [serde_json::json!("false"), serde_json::json!(0), serde_json::json!(null)] {
        assert!(
            ontok::cli::ensure_importable(p, &serde_json::json!({"importable": bad})).is_err(),
            "importable = {bad:?} 必须被拒（不能猜成能导）"
        );
    }
}

// ── 审核界面的置信度表（AC-010 的「审核界面必须有」）────────

/// `review show --confidence` 的那张表：四档计数 + 档位标签 + 证据摘要 + veto 理由。
#[test]
fn test_req108_review_confidence_table_shape() {
    use ontok::db::proposal::ProposalItemRow;
    let row = |seq: i32, target: &str, c: serde_json::Value| ProposalItemRow {
        id: uuid::Uuid::nil(),
        proposal_id: uuid::Uuid::nil(),
        seq,
        kind: "add_link_type".into(),
        target_key: target.into(),
        payload: serde_json::json!({ "confidence": c }),
        biz_status: "pending".into(),
        model_status: "pending".into(),
        // REQ-111 · 决策审计两栏：这里构造的是**没签过**的条目 ⇒ 全 None。
        biz_decided_at: None,
        biz_via: None,
        model_decided_at: None,
        model_via: None,
    };
    let rows = vec![
        row(1, "订单→客户", serde_json::to_value(perfect()).unwrap()),
        row(
            2,
            "日志→批次",
            serde_json::json!({
                "score": 0.2, "method": METHOD, "band": "rejected",
                "signals": [
                    {"name":"sample_hit_rate","value":0.0,"weight":0.45,"sampled":5,"matched":0,
                     "sample_digest":"sha256:bb"}
                ],
                "veto": {"signal":"zero_hit","cap":0.2,"reason":"采了 5 行、命中 0 行"},
                "data_evidence": "sampled"
            }),
        ),
    ];
    let text = ontok::cli::render_confidence_table(&rows);
    assert!(text.contains("evidence-scorecard-v1"), "{text}");
    assert!(text.contains("高 1"), "四档计数要在：{text}");
    assert!(text.contains("拒绝 1"), "{text}");
    assert!(text.contains("[高]") && text.contains("[拒绝]"), "每条要带档位标签：{text}");
    assert!(text.contains("证据 5/5"), "证据摘要（命中/样本）要在：{text}");
    assert!(text.contains("veto zero_hit"), "拒绝要带理由：{text}");
    // 排序确定：高档在前
    let hi = text.find("[高]").unwrap();
    let rej = text.find("[拒绝]").unwrap();
    assert!(hi < rej, "高档必须排在拒绝之前：{text}");

    // 没有分数时给一句人话，不是空表
    let empty = ontok::cli::render_confidence_table(&[]);
    assert!(empty.contains("没有分数"), "{empty}");

    // 数据证据缺失时也要说清楚（不能看不出「没采」）
    //
    // ★ A-10：这里改用评分卡**真实产出**（`--no-sample` 的形状），不再手拼一个
    //   `signals: vec![]` 的 Confidence —— 后者在新规则下已不是合法值
    //   （`render_confidence_table` 读回时会判为「没有分数」，本断言就变空了）。
    let no_data = score(&ScoreInput {
        column: "customer_id".into(),
        to_table: "customers".into(),
        from_type: TypeFamily::Str,
        to_type: Some(TypeFamily::Str),
        target_key: TargetKey::Primary,
        data: None,
        composite_target_key: false,
        polymorphic: false,
        self_ref: false,
    });
    assert_eq!(no_data.data_evidence, DataEvidence::None);
    let text =
        ontok::cli::render_confidence_table(&[row(3, "无证据", serde_json::to_value(no_data).unwrap())]);
    assert!(text.contains("data_evidence=none"), "{text}");
}
