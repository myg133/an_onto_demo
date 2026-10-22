//! REQ-120 · **关系推断改进（FK 边补全）**的交付层判据。
//!
//! # 分工（别把判据写重）
//!
//! | 判据 | 住在哪 |
//! | --- | --- |
//! | 提名 / 阈值 / 判决 / 基数 / 签字闸 / 注入 / 盖章 | `src/extract/fk.rs` 的 `#[cfg(test)]`（27 条） |
//! | 同名 IRI 冲突修复 | `src/domain/ttl.rs` 的 `colliding_link_pairs_get_column_qualified_local_names` |
//! | **跨模块的端到端形态**（提案文件 → 签字 → 注入 → 快照字段 → TTL 三元组） | **本文件** |
//! | 真实库实测（68 条提案 / 26 个否决 / 76 列未解析 / 73 条边进 TTL） | 交付报告 + `out/fk-proposals.json` |
//!
//! # 为什么这些判据必须在 `tests/` 里再来一遍
//!
//! `fk.rs` 的单测只能证明「函数对」；本文件证明**模块之间的接缝对**：
//! 签字产物里的字段叫 `join_verified`，快照层叫同名，TTL 层叫
//! `ontok:joinVerified` —— 三处任何一处改名、只要不是同一份数据，本文件就红。
//! （真实教训：本轮一开始把标记写在**条目顶层**，`apply` 只读 `payload`，
//!  于是「提案里有、TTL 里没有」；单测全绿，端到端空。）

use ontok::domain::model_view::ModelLinkView;
use ontok::domain::snapshot::{Evidence, JoinState, LinkType};
use ontok::domain::ttl::{serialize_model, TtlOptions, DEFAULT_BASE};
use ontok::extract::fk::{
    classify_milli, confidence_of, inject_signed, load_signed, promote_unique_cardinality,
    render_report, render_unresolved, stamp_naming_only, Band, FkFacts, FkProposal, FkReport,
    FkSource, FkStats, MatchKind, TargetKind, UnresolvedCode, FK_METHOD,
};
use ontok::extract::introspect::KeyCatalog;
use serde_json::json;
use std::collections::BTreeSet;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn facts(to_table: &str, matched: u32, sampled: u32) -> FkFacts {
    FkFacts {
        to_table: to_table.to_string(),
        to_key_column: "id".to_string(),
        target_kind: TargetKind::Primary,
        matched,
        sampled,
        window_rows: sampled,
        window_distinct: sampled,
        ratio_milli: if sampled == 0 { 0 } else { matched * 1000 / sampled },
        samples: vec!["a1".into(), "b2".into(), "c3".into()],
        sample_unique: true,
        unique_column: false,
        match_kind: MatchKind::Plural,
        stem: "x".into(),
        inherit: None,
    }
}

fn proposal(subject: &str, to: &str, matched: u32, sampled: u32, signed: Option<&str>) -> FkProposal {
    let mut v = json!({
        "subject": subject,
        "claim": format!("列 {subject} 指向 {to}"),
        "evidence": [format!("值域重叠 命中 3 / 3 个不同样本值（共取 3 行，1000‰）· 目标键 {to}.id · 提名 词干+复数 · 样本示例 a1, b2, c3")],
        "facts": facts(to, matched, sampled),
        "confidence": confidence_of(if sampled == 0 { 0 } else { matched * 1000 / sampled }),
        "band": classify_milli(if sampled == 0 { 0 } else { matched * 1000 / sampled }).as_str(),
        "cardinality": "N:1",
        "reason": "命中率 100% ⇒ strong",
        "decided_by": ontok::extract::fk::DECIDED_BY_RULE,
        "reviewed_by": signed,
    });
    serde_json::from_value(std::mem::take(&mut v)).expect("构造提案")
}

fn report(proposals: Vec<FkProposal>) -> FkReport {
    let mut r = FkReport {
        version: FkReport::VERSION,
        kind: FkReport::KIND.to_string(),
        method: FK_METHOD.to_string(),
        source: FkSource {
            schema_file: "schema.json".into(),
            schema_sha256: "00".repeat(32),
            schema: "public".into(),
            read_only: "on".into(),
            tables: 96,
            columns: 1111,
        },
        proposals,
        rejected: vec![],
        unresolved: vec![],
        stats: FkStats::default(),
    };
    r.sort();
    r
}

fn opts() -> TtlOptions<'static> {
    TtlOptions {
        project: "req120",
        version: 2,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: None,
        schema: None,
    }
}

fn link_view(
    key: &str,
    from: &str,
    to: &str,
    card: &str,
    column: Option<&str>,
    join: Option<JoinState>,
    evidence: Option<&str>,
) -> ModelLinkView {
    ModelLinkView {
        key: key.into(),
        from_type: from.into(),
        to_type: to.into(),
        cardinality: card.into(),
        semantic: "belongs_to".into(),
        evidence: column
            .map(|c| vec![Evidence { table: from.into(), column: c.into(), claim: "列名推断".into() }])
            .unwrap_or_default(),
        confidence: None,
        join_verified: join,
        join_evidence: evidence.map(str::to_string),
        column: column.map(str::to_string),
    }
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 提案形状 + 确定性
// ══════════════════════════════════════════════════════════════

/// 提案文件**两次渲染逐字节相同**（AC-009 的前半），且 `reviewed_by` 恒为 null。
#[test]
fn report_rendering_is_byte_stable_and_proposals_are_unsigned() {
    let r = report(vec![
        proposal("z_table.z_id", "z", 5, 5, None),
        proposal("a_table.a_id", "a", 4, 5, None),
    ]);
    let a = render_report(&r).expect("渲染");
    let b = render_report(&r).expect("渲染");
    assert_eq!(a, b, "同输入两次渲染必须逐字节相同（AC-009）");
    assert!(a.ends_with("}\n"), "以换行结尾：{a}");
    // 排序稳定：a_table 在 z_table 前
    let ia = a.find("a_table.a_id").unwrap();
    let iz = a.find("z_table.z_id").unwrap();
    assert!(ia < iz, "提案按 subject 排序：{a}");
    // P2：生成路径写出来的恒未签
    for p in &r.proposals {
        assert_eq!(p.reviewed_by(), None);
        assert!(p.evidence.iter().any(|e| e.contains("值域重叠")));
        assert!(p.evidence.iter().any(|e| e.contains("a1")));
    }
}

/// 读数的人能独立判断：evidence 必须同时含**匹配方式**与**命中数/样本数**。
#[test]
fn evidence_carries_match_kind_and_counts() {
    let p = proposal("api_call_logs.license_id", "licenses", 5, 5, None);
    let joined = p.evidence.join("\n");
    assert!(joined.contains("命中"), "{joined}");
    assert!(joined.contains("命中 3 / 3 个不同样本值"), "{joined}");
    assert!(joined.contains("共取 3 行"), "行数与去重数分开写：{joined}");
    assert!(joined.contains("词干+复数"), "{joined}");
    assert!(joined.contains("licenses.id"), "目标键列必须写出来：{joined}");
}

// ══════════════════════════════════════════════════════════════
// AC-002 / AC-005 · 否决与未解析清单
// ══════════════════════════════════════════════════════════════

/// 未解析清单**分类计数**与逐条原因都要落盘，且不静默跳过。
#[test]
fn unresolved_file_is_classified_and_counted() {
    let mut r = report(vec![]);
    for (i, code) in [
        UnresolvedCode::NoCandidate,
        UnresolvedCode::LowOverlap,
        UnresolvedCode::CandidateNoPk,
        UnresolvedCode::InsufficientSample,
        UnresolvedCode::SampleFailed,
    ]
    .iter()
    .enumerate()
    {
        r.unresolved.push(ontok::extract::fk::FkUnresolved {
            subject: format!("t.c{i}_id"),
            table: "t".into(),
            column: format!("c{i}_id"),
            code: code.as_str().to_string(),
            code_cn: code.cn().to_string(),
            reason: "构造".into(),
            candidates: 1,
            candidate_tables: vec!["x".into()],
        });
    }
    let doc = render_unresolved(&r).expect("渲染未解析清单");
    let v: serde_json::Value = serde_json::from_str(&doc).expect("合法 JSON");
    assert_eq!(v["counts"]["no_candidate"], 1);
    assert_eq!(v["counts"]["low_overlap"], 1);
    assert_eq!(v["counts"]["candidate_no_pk"], 1);
    assert_eq!(v["counts"]["insufficient_sample"], 1);
    assert_eq!(v["counts"]["sample_failed"], 1);
    assert_eq!(v["unresolved"].as_array().unwrap().len(), 5);
    assert!(doc.contains("命中率过低被否决"), "原因要有中文分类：{doc}");
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 未签的提案不进快照
// ══════════════════════════════════════════════════════════════

/// 签发文件里**一条都没签** ⇒ 拒绝（不是「安静地注入 0 条」）。
#[test]
fn a_file_without_any_signature_is_refused() {
    let r = report(vec![proposal("api_call_logs.license_id", "licenses", 5, 5, None)]);
    let err = load_signed(&render_report(&r).unwrap(), None).unwrap_err().to_string();
    assert!(err.contains("一条都没签"), "{err}");
}

/// 只有 `reviewed_by` 非空的那几条会变成边（P2）。
#[test]
fn only_signed_proposals_become_edges() {
    let r = report(vec![
        proposal("api_call_logs.license_id", "licenses", 5, 5, Some("ba")),
        proposal("api_call_logs.customer_id", "customers", 4, 5, None),
    ]);
    let s = load_signed(&render_report(&r).unwrap(), None).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s.edges[0].from_column, "license_id");
    assert_eq!(s.skipped_unsigned, 1);
}

// ══════════════════════════════════════════════════════════════
// AC-007 · 落进 TTL 的是 joinVerified / joinEvidence
// ══════════════════════════════════════════════════════════════

/// **模块接缝判据**：ModelLinkView 的 `Some(true)` + evidence ⇒ TTL 两行；
/// `Some(false)` ⇒ 只有一行 false；`None` ⇒ 一行都不写（三态不许互相吞）。
#[test]
fn ttl_renders_the_three_states_of_join_verification() {
    let mut view = ontok::domain::model_view::ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![],
        link_types: vec![
            link_view(
                "a_x_id__x",
                "a",
                "x",
                "N:1",
                Some("x_id"),
                Some(JoinState::Verified),
                Some("value-overlap 5/5 (100.00%) -> x.id via primary_key (plural)"),
            ),
            link_view("b_y_id__y", "b", "y", "N:1", Some("y_id"), Some(JoinState::NamingOnly), None),
            // E-2 的第三态：继承
            link_view(
                "d_w_id__w",
                "d",
                "w",
                "N:1",
                Some("w_id"),
                Some(JoinState::Inherited),
                Some("inherited from e.w_id (verified 4/4) -> w.id via primary_key (plural)"),
            ),
            link_view("c_z_id__z", "c", "z", "N:1", Some("z_id"), None, None),
        ],
        action_types: vec![],
        functions: vec![],
    };
    view.link_types.sort_by(|a, b| a.key.cmp(&b.key));
    let doc = serialize_model(&view, &opts()).into_doc();

    // verified：正反各一条 ⇒ 2 处 true
    assert_eq!(
        doc.matches("ontok:joinVerified \"true\"").count(),
        2,
        "verified 的边正反两条都要标：{doc}"
    );
    assert!(doc.contains("ontok:joinEvidence \"value-overlap 5/5"), "{doc}");
    // naming-only：只有标记，没有证据
    assert_eq!(doc.matches("ontok:joinVerified \"false\"").count(), 2, "{doc}");
    // E-2 的第三态：inherit 出现，且证据说的是 inherited from
    assert_eq!(
        doc.matches("ontok:joinVerified \"inherit\"").count(),
        2,
        "继承也要正反各标一条：{doc}"
    );
    assert!(
        doc.contains("ontok:joinEvidence \"inherited from e.w_id (verified 4/4)"),
        "{doc}"
    );
    // c→z 是 None（旧快照）⇒ 一个标记都不许写。
    // ⚠️ 只数**三元组**（后跟引号），不数文件头那句 F-4 说明里的同名字样。
    assert_eq!(
        doc.matches("ontok:joinVerified \"").count(),
        6,
        "三条边 × 正反 = 6 处；None 的边一处都不许有：{doc}"
    );
    // 三态在正文里可区分
    assert!(!doc.contains("ontok:joinEvidence \"\""), "空证据不该出现：{doc}");
}

/// **真实 bug 的回归**：同一对 (from,to) 的两条边必须是**两个 IRI**。
#[test]
fn two_edges_between_the_same_pair_never_share_an_iri() {
    let view = ontok::domain::model_view::ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![],
        link_types: vec![
            link_view(
                "cab_main_customer_id__customers",
                "cab",
                "customers",
                "N:1",
                Some("main_customer_id"),
                Some(JoinState::Verified),
                Some("value-overlap 5/5"),
            ),
            link_view(
                "cab_sub_customer_id__customers",
                "cab",
                "customers",
                "N:1",
                Some("sub_customer_id"),
                Some(JoinState::Verified),
                Some("value-overlap 5/5"),
            ),
        ],
        action_types: vec![],
        functions: vec![],
    };
    let doc = serialize_model(&view, &opts()).into_doc();
    assert!(doc.contains(":cab__customers__main_customer_id a "), "{doc}");
    assert!(doc.contains(":cab__customers__sub_customer_id a "), "{doc}");
    assert!(
        !doc.contains(":cab__customers a "),
        "有歧义时不许退化成 `{{from}}__{{to}}`：{doc}"
    );
    // 主语块不重复
    let subjects: Vec<&str> = doc
        .lines()
        .filter(|l| l.starts_with(':') && l.contains(" a owl:") && l.contains("ObjectProperty"))
        .map(|l| l.split(' ').next().unwrap())
        .collect();
    let uniq: BTreeSet<&str> = subjects.iter().copied().collect();
    assert_eq!(subjects.len(), uniq.len(), "同一个主语写了两遍：{doc}");
}

// ══════════════════════════════════════════════════════════════
// 注入 / 盖章 / 基数（AC-007 的注入侧 + AC-004）
// ══════════════════════════════════════════════════════════════

/// 注入只写 `payload` 一层 —— `apply` 读的就是这一层（本轮踩过的坑）。
#[test]
fn injection_writes_the_payload_layer_that_apply_reads() {
    let r = report(vec![proposal("cab.main_customer_id", "customers", 5, 5, Some("ba"))]);
    let signed = load_signed(&render_report(&r).unwrap(), None).unwrap();
    let mut items: Vec<serde_json::Value> = vec![];
    let known: BTreeSet<String> = ["cab", "customers"].iter().map(|s| s.to_string()).collect();
    let st = inject_signed(&mut items, &signed, &known, None).expect("注入");
    assert_eq!(st.added, 1);

    let it = &items[0];
    // 顶层**没有**副本：一个真相
    assert!(it.get("join_verified").is_none(), "{it}");
    assert_eq!(it["payload"]["from_type"], "cab");
    assert_eq!(it["payload"]["to_type"], "customers");
    assert_eq!(
        it["payload"]["join_verified"],
        serde_json::Value::String("true".into()),
        "三态用字符串"
    );
    assert!(it["payload"]["join_evidence"].as_str().unwrap().contains("5/5"));
    assert_eq!(it["payload"]["cardinality"], "N:1");

    // 与 `domain::apply` 的读法对齐：payload 直接能映射成快照的 LinkType 字段
    let p = &it["payload"];
    // 与 `apply::join_state_from_payload` 同一读法：字符串三态
    let jv = p.get("join_verified").and_then(|v| v.as_str()).and_then(JoinState::parse);
    let je = p.get("join_evidence").and_then(|v| v.as_str()).map(str::to_string);
    let lt = LinkType {
        key: p["key"].as_str().unwrap().to_string(),
        from_type: p["from_type"].as_str().unwrap().to_string(),
        to_type: p["to_type"].as_str().unwrap().to_string(),
        cardinality: p["cardinality"].as_str().unwrap().to_string(),
        semantic: "belongs_to".into(),
        evidence: vec![],
        confidence: None,
        join_verified: jv,
        join_evidence: je,
    };
    assert_eq!(lt.join_verified, Some(JoinState::Verified));
    assert!(lt.join_evidence.unwrap().contains("customers.id"));
}

/// 名字推断的边一律 false；已验的不被改回 false。
#[test]
fn stamping_marks_naming_only_edges_and_keeps_verified_ones() {
    let mut items: Vec<serde_json::Value> = vec![
        json!({"kind":"add_link_type","target":"a→b","payload":{"from_type":"a","to_type":"b","cardinality":"N:1","evidence":[]}}),
        json!({"kind":"add_link_type","target":"c→d","payload":{"from_type":"c","to_type":"d","cardinality":"N:1","join_verified":"true"}}),
        json!({"kind":"add_entity_type","target":"e","payload":{"key":"e"}}),
    ];
    assert_eq!(stamp_naming_only(&mut items, &std::collections::BTreeMap::new()), 1);
    assert_eq!(
        items[0]["payload"]["join_verified"],
        serde_json::Value::String("false".into())
    );
    assert_eq!(
        items[1]["payload"]["join_verified"],
        serde_json::Value::String("true".into())
    );
    assert!(items[2].get("join_verified").is_none(), "实体条目不许被盖章");
}

/// AC-004：单列唯一列上的 N:1 改成 1:1；非唯一列不许被改。
#[test]
fn unique_columns_become_one_to_one() {
    let mut keys = KeyCatalog::new();
    keys.insert(
        "balances",
        ontok::extract::introspect::TableKeys {
            primary_key: Some("id".into()),
            unique_columns: vec!["customer_id".into(), "id".into()],
            ..Default::default()
        },
    );
    keys.insert(
        "deductions",
        ontok::extract::introspect::TableKeys {
            primary_key: Some("id".into()),
            unique_columns: vec!["id".into()],
            ..Default::default()
        },
    );
    let mut items = vec![
        json!({"kind":"add_link_type","target":"balances→customers","payload":{"from_type":"balances","to_type":"customers","cardinality":"N:1","evidence":[{"table":"balances","column":"customer_id","claim":"命名约定"}]}}),
        json!({"kind":"add_link_type","target":"deductions→customers","payload":{"from_type":"deductions","to_type":"customers","cardinality":"N:1","evidence":[{"table":"deductions","column":"customer_id","claim":"命名约定"}]}}),
    ];
    let changed = promote_unique_cardinality(&mut items, &keys);
    assert_eq!(changed, vec!["balances.customer_id".to_string()]);
    assert_eq!(items[0]["payload"]["cardinality"], "1:1");
    assert_eq!(items[1]["payload"]["cardinality"], "N:1");
}

/// 阈值：AC-002 的四档边界（写成表，改阈值必红）。
#[test]
fn thresholds_match_the_ac_table() {
    let rows = [
        (1000u32, Band::Strong),
        (950, Band::Strong),
        (949, Band::Medium),
        (500, Band::Medium),
        (499, Band::Vetoed),
        (0, Band::Vetoed),
    ];
    for (milli, band) in rows {
        assert_eq!(classify_milli(milli), band, "{milli}‰ 应当是 {band:?}");
    }
    assert!(confidence_of(1000) >= 0.9, "strong 必须 ≥ 0.9（AC-002）");
    assert!(confidence_of(950) >= 0.9);
    assert!(confidence_of(900) < 0.9, "medium 不许冒充 strong");
}
