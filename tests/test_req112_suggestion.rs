//! REQ-112 · 审核界面接入 AI 建议 —— AC-001 ~ AC-009 的判据。
//!
//! # 三层判据，各证一件事
//!
//! | 层 | 证什么 | 怎么证 |
//! | --- | --- | --- |
//! | **纯函数** | 规则表逐条落地、确定性、无模型 | 直接构造 `Confidence`，断言 `kind` 与依据文案 |
//! | **HTTP 面** | 每条带建议、预览白名单、零写库、零 token | `build_router` + `Router::oneshot` 打真路由，前后比对整行 |
//! | **源码面** | 建议链路上**没有** LLM 调用点 | 剥注释后扫 `review_ui/` 与评分卡模块 |
//!
//! # 为什么纯函数部分不连库
//!
//! AC-001 要的是「同一 `confidence` 两次调用逐字段相同」。这件事与数据库无关 ——
//! 连库只会把「确定性」这个命题稀释成「这次连得上库时确定」。所以规则表 7 条
//! 各自一个 `#[test]`，不建夹具、不建表。
//!
//! # 构造用的数值来自**真实数据**
//!
//! 每条规则的用例都照抄 `anspire_real` 里对应条目的实际信号（见各测试注释里的
//! `表.列__目标`），这样「规则在合成数据上成立、在真实数据上不成立」这种漂移
//! 会在跑测试时就露出来。

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{NewItem, NewProposal, PgProposalStore, ProposalItemRow};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::confidence::{
    hit_rate_value, suggestion, veto_cap, AiProvenance, Band, Confidence, DataEvidence, Signal,
    SuggestKind, Veto, BAND_HIGH, BAND_LOW, BAND_MIDDLE, FIT_ACCEPT, FIT_CONFLICT, METHOD_V2,
    REASON_NO_CONFIDENCE, SIGNAL_SEMANTIC_FIT, WEIGHTS_V2,
};
use ontok::domain::review::ItemKind;
use ontok::domain::snapshot::Snapshot;
use ontok::review_ui::http::{build_router, prepare};
use ontok::review_ui::ReviewArgs;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "correct-horse-battery";

// ══════════════════════════════════════════════════════════════
// 一 · 纯函数：规则表逐条（AC-001 / AC-002 / AC-003 / AC-009）
// ══════════════════════════════════════════════════════════════

fn weight_of(name: &str) -> f64 {
    WEIGHTS_V2
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, w)| *w)
        .expect("信号必须在评分卡 v2 的权重表里 —— 否则这条用例造的不是真信号")
}

fn sig(name: &str, value: f64) -> Signal {
    Signal {
        name: name.to_string(),
        value,
        weight: weight_of(name),
        sampled: None,
        matched: None,
        sample_digest: None,
    }
}

fn hit_signal(sampled: u32, matched: u32) -> Signal {
    let mut s = sig("sample_hit_rate", hit_rate_value(sampled, matched));
    s.sampled = Some(sampled);
    s.matched = Some(matched);
    s
}

fn ai_provenance() -> AiProvenance {
    AiProvenance {
        model: "fake-1".to_string(),
        prompt_sha: "deadbeef".to_string(),
        // ⭐ 0 token：这条建议的取值**与花多少钱无关**，测试里也把它钉成 0。
        tokens: 0,
        cache_hit: false,
        rationale: Some("两端语义匹配".to_string()),
        cites: None,
        choice: None,
    }
}

/// 造一条**形状合法**的 `confidence`（信号顺序照 v2 权重表）。
///
/// `fit = None` 表示「这条没被 AI 判过」（AI 关），**不是 0 分**（AC-001 的区分）。
fn conf(score: f64, sampled: u32, matched: u32, fit: Option<f64>) -> Confidence {
    let mut signals = vec![
        hit_signal(sampled, matched),
        sig("name_match", 1.0),
        sig("not_all_null", 1.0),
        sig("type_compatible", 1.0),
        sig("target_is_key", 1.0),
    ];
    if let Some(f) = fit {
        signals.push(sig(SIGNAL_SEMANTIC_FIT, f));
    }
    Confidence {
        score,
        method: METHOD_V2.to_string(),
        band: Band::of(score),
        signals,
        veto: None,
        data_evidence: DataEvidence::Sampled,
        ai: fit.map(|_| ai_provenance()),
    }
}

fn with_veto(mut c: Confidence, signal: &str) -> Confidence {
    c.veto = Some(Veto {
        signal: signal.to_string(),
        cap: veto_cap(signal).expect("veto 必须在评分卡的上限表里"),
        reason: "合成用例".to_string(),
    });
    c
}

/// **规则 ①** · `veto = zero_hit` ⇒ `reject`。
///
/// 真实条目：`instance_manager.session_id → simulate_login_sessions`
/// （`sampled=5 / matched=0`、AI 判 0.00、veto `zero_hit`）。
///
/// ⚠️ 这条正是 demand 说的「5/5 命中却被 AI 判 0.0」的邻居：
/// 它的**证据**是 0 命中，所以先命中规则 ① —— 硬反驳优先于一切分数。
#[test]
fn rule1a_zero_hit_veto_is_reject() {
    let c = with_veto(conf(0.20, 5, 0, Some(0.0)), "zero_hit");
    let s = suggestion(Some(&c));

    assert_eq!(s.kind, SuggestKind::Reject, "零命中是硬反驳，只能是拒绝");
    assert_eq!(s.reason, "采样 0/5 全不命中（硬反驳）");
    assert_eq!(s.signals.veto.as_deref(), Some("zero_hit"));
    assert_eq!(s.signals.matched, Some(0));
    assert_eq!(s.signals.sampled, Some(5));
    assert!(!s.kind.enters_batch());
}

/// **规则 ①** · `veto = type_incompatible` ⇒ `reject`。
///
/// 真实条目：`dim_customer_binding.channel_code → marketing_channel`
/// （类型族不相容 ⇒ 值永远比不中；分数被 0.15 压顶）。
#[test]
fn rule1b_type_incompatible_veto_is_reject() {
    let mut c = conf(0.15, 5, 5, Some(0.0));
    // 类型不相容的那条证据信号也要跟着为 0，否则 veto 与信号自相矛盾。
    for s in c.signals.iter_mut() {
        if s.name == "type_compatible" {
            s.value = 0.0;
        }
    }
    let c = with_veto(c, "type_incompatible");
    let s = suggestion(Some(&c));

    assert_eq!(s.kind, SuggestKind::Reject);
    assert_eq!(s.reason, "类型不相容（硬反驳）");
    assert_eq!(s.signals.veto.as_deref(), Some("type_incompatible"));
    assert!(!s.kind.enters_batch());
}

/// **规则 ②** · `band = high` 且 `fit ≥ 0.80` ⇒ `accept`。
///
/// 真实条目：`admin_user_roles.admin_user_id → admin_users`
/// （`score=1.00` · `high` · `5/5` · `fit=1.00`）。
#[test]
fn rule2_high_band_and_fit_at_least_080_is_accept() {
    let s = suggestion(Some(&conf(1.00, 5, 5, Some(1.00))));

    assert_eq!(s.kind, SuggestKind::Accept);
    assert_eq!(s.reason, "采样 5/5 + AI fit 1.00");
    assert_eq!(s.signals.band.as_deref(), Some("high"));
    assert_eq!(s.signals.semantic_fit, Some(1.0));
    assert!(s.kind.enters_batch(), "接受必须进「按建议接受」白名单");

    // 边界：`fit` 恰好 0.80 也算达到（规则表写的是 `≥ 0.8`）。
    assert_eq!(
        suggestion(Some(&conf(1.00, 5, 5, Some(FIT_ACCEPT)))).kind,
        SuggestKind::Accept,
        "fit = 0.80 必须落在接受侧（边界属于闭区间）"
    );
    // 对照（纪律 E）：`fit` 差一点点就不该接受 —— 否则上面的断言只是「凡 high 必接受」。
    assert_ne!(
        suggestion(Some(&conf(1.00, 5, 5, Some(0.79)))).kind,
        SuggestKind::Accept
    );
}

/// **规则 ③** · `band = high` + `fit < 0.50` + `matched == sampled` ⇒ **`conflict`**。
///
/// 这是全需求**最要紧**的一条：REQ-109 实测过 3 条「5/5 命中却被 AI 判 0.0」。
/// 判成 `accept` 是无视 AI，判成 `reject` 是无视数据 —— 两者都是**替人做决定**。
#[test]
fn rule3_full_hits_but_ai_denies_is_conflict() {
    let s = suggestion(Some(&conf(0.95, 5, 5, Some(0.00))));

    assert_eq!(s.kind, SuggestKind::Conflict);
    assert_ne!(s.kind, SuggestKind::Accept, "⛔ 不许替人选「接受」");
    assert_ne!(s.kind, SuggestKind::Reject, "⛔ 不许替人选「拒绝」");
    assert_eq!(s.reason, "5/5 命中但 AI 判 0.00 —— 需人工判");
    assert_eq!(s.signals.semantic_fit, Some(0.0));
    // ⭐ AC-003 第二条判据：冲突条目**不进**批量白名单。
    assert!(!s.kind.enters_batch(), "冲突条目绝不能进「按建议接受」");

    // 边界：`fit` 恰好 0.50 **不算**冲突（规则写的是 `< 0.5`）。
    assert_ne!(
        suggestion(Some(&conf(0.95, 5, 5, Some(FIT_CONFLICT)))).kind,
        SuggestKind::Conflict,
        "fit = 0.50 不属于「AI 否定」，不许标冲突"
    );
    // 边界：命中不满（matched < sampled）也不算冲突 —— 那时证据本来就不支持。
    assert_ne!(
        suggestion(Some(&conf(0.95, 5, 4, Some(0.00)))).kind,
        SuggestKind::Conflict,
        "matched != sampled 时证据并不「支持」，冲突的前提不成立"
    );
}

/// **规则 ④** · `band = middle` ⇒ `lean_accept`。
///
/// 真实条目：`customer_subscriptions.template_id → package_templates`
/// （`score=0.88` · `middle` · `5/5` · `fit=1.00`）。
#[test]
fn rule4_middle_band_is_lean_accept() {
    let s = suggestion(Some(&conf(0.88, 5, 5, Some(1.00))));

    assert_eq!(s.kind, SuggestKind::LeanAccept);
    assert_eq!(s.reason, "证据中等（5/5），建议看一眼再签");
    assert_eq!(s.signals.band.as_deref(), Some("middle"));
    assert!(s.kind.enters_batch(), "倾向接受也进白名单（AC-006）");
}

/// **规则 ⑤** · `band = low` ⇒ `review`。
///
/// 真实条目：`activity_keys.activity_id → activity`
/// （`score=0.45` · `low` · 源列全空 ⇒ veto `all_null`；`all_null` 不是硬反驳，
/// 所以走档位规则而不是规则 ①）。
#[test]
fn rule5_low_band_is_review() {
    let c = with_veto(conf(0.45, 0, 0, Some(1.00)), "all_null");
    let s = suggestion(Some(&c));

    assert_eq!(s.kind, SuggestKind::Review);
    assert_eq!(s.reason, "证据不足（分数 0.45）—— 建议人工核对");
    assert_eq!(s.signals.band.as_deref(), Some("low"));
    assert!(!s.kind.enters_batch(), "存疑条目不进批量白名单");
}

/// **规则 ⑥** · `band = rejected` ⇒ `reject`（无 veto 时是「分数低于阈值」）。
#[test]
fn rule6_rejected_band_is_reject() {
    let s = suggestion(Some(&conf(0.20, 5, 0, None)));

    assert_eq!(s.kind, SuggestKind::Reject);
    assert_eq!(s.reason, "分数 0.20 低于 0.30 —— 建议拒绝");
    assert_eq!(s.signals.band.as_deref(), Some("rejected"));
    assert!(!s.kind.enters_batch());
}

/// **规则 ⑦** · 无 `confidence` ⇒ `none`（**不给规则抽取的条目编建议**）。
///
/// 真实条目：83 个实体条目（`add_entity_type`）全部没有置信度。
#[test]
fn rule7_without_confidence_is_none() {
    let s = suggestion(None);

    assert_eq!(s.kind, SuggestKind::None);
    assert_eq!(s.reason, REASON_NO_CONFIDENCE);
    assert!(s.signals.band.is_none(), "没有置信度就没有档位可引用");
    assert!(s.signals.score.is_none());
    // ⭐ AC-007：无建议**不进**批量白名单。
    assert!(!s.kind.enters_batch(), "⛔ `none` 绝不能进「按建议接受」");
}

/// 规则表的**空格**（需求没写、但真实数据里有一半）：`band = high` 但没做 AI 语义判断。
///
/// 这里不猜成 `accept` —— 「高分」不等于「可以签」；落 `review` 并说清缺的是哪一项。
#[test]
fn high_band_without_semantic_judgement_is_review_not_accept() {
    let s = suggestion(Some(&conf(1.00, 5, 5, None)));

    assert_eq!(s.kind, SuggestKind::Review);
    assert_eq!(s.reason, "采样 5/5 但无 AI 语义判断 —— 建议抽查");
    assert!(!s.kind.enters_batch());

    // 另一格：fit 落在 [0.50, 0.80) —— 语义分不够高，也不替人签。
    let mid = suggestion(Some(&conf(0.95, 5, 5, Some(0.50))));
    assert_eq!(mid.kind, SuggestKind::Review);
    assert_eq!(mid.reason, "采样 5/5，AI fit 0.50 未达 0.80");
}

/// **AC-001** · 纯函数：同一输入两次调用**逐字段相同**。
#[test]
fn ac001_same_confidence_twice_gives_the_same_suggestion() {
    let c = conf(0.95, 5, 5, Some(0.00));
    let first = suggestion(Some(&c));
    let second = suggestion(Some(&c));

    assert_eq!(first, second, "同一 confidence ⇒ 逐字段相同");
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap(),
        "序列化后也必须逐字节相同（页面上的文案不许抖）"
    );
    // 连调 100 次都一样：没有时间 / 随机 / 缓存参与。
    for _ in 0..100 {
        assert_eq!(suggestion(Some(&c)), first);
    }
    // 换个顺序构造同一份 confidence 也一样（纯函数只看值，不看调用历史）。
    let rebuilt = conf(0.95, 5, 5, Some(0.00));
    assert_eq!(suggestion(Some(&rebuilt)), first);
}

/// **AC-009** · 与评分卡不漂移：档位被人为改掉，建议必须跟着变。
///
/// 判据是「建议读的是**评分卡发出来的 band**」—— 而不是在页面里按分数再算一遍
/// （那种实现下改了 `band` 建议不会变，这条测试就会红）。
#[test]
fn ac009_changing_the_band_changes_the_suggestion() {
    let mut c = conf(1.00, 5, 5, Some(1.00));
    assert_eq!(suggestion(Some(&c)).kind, SuggestKind::Accept);

    c.band = Band::Middle;
    assert_eq!(
        suggestion(Some(&c)).kind,
        SuggestKind::LeanAccept,
        "把 band 从 high 改成 middle，建议必须跟着变 —— 否则就是页面里另算了一套"
    );

    c.band = Band::Low;
    assert_eq!(suggestion(Some(&c)).kind, SuggestKind::Review);

    c.band = Band::Rejected;
    assert_eq!(suggestion(Some(&c)).kind, SuggestKind::Reject);

    // 阈值常量与 `Band::of` **同源**（同一个模块、同一批常量）。
    assert_eq!(Band::of(BAND_HIGH), Band::High);
    assert_eq!(Band::of(BAND_MIDDLE), Band::Middle);
    assert_eq!(Band::of(BAND_LOW), Band::Low);
    assert!(BAND_HIGH > BAND_MIDDLE && BAND_MIDDLE > BAND_LOW);
    // 语义分阈值也在同一模块里，且区间不重叠（0.50 < 0.80）。
    assert!(FIT_CONFLICT < FIT_ACCEPT);
}

/// **AC-008** · 依据文案 **≤ 40 字**（页面一行放得下，不会被裁掉）。
#[test]
fn ac008_every_reason_is_at_most_40_chars() {
    let cases = vec![
        suggestion(None),
        suggestion(Some(&with_veto(conf(0.20, 5, 0, Some(0.0)), "zero_hit"))),
        suggestion(Some(&with_veto(conf(0.15, 5, 5, Some(0.0)), "type_incompatible"))),
        suggestion(Some(&conf(1.00, 5, 5, Some(1.00)))),
        suggestion(Some(&conf(0.95, 5, 5, Some(0.00)))),
        suggestion(Some(&conf(0.95, 5, 5, Some(0.50)))),
        suggestion(Some(&conf(1.00, 5, 5, None))),
        suggestion(Some(&conf(0.88, 5, 5, Some(1.00)))),
        suggestion(Some(&with_veto(conf(0.45, 0, 0, Some(1.00)), "all_null"))),
        suggestion(Some(&conf(0.20, 5, 0, None))),
        suggestion(Some(&conf(0.40, 0, 0, None))),
    ];
    for s in &cases {
        let n = s.reason.chars().count();
        assert!(
            n <= 40,
            "依据文案 {} 字，超过 40 字上限：{:?}（kind = {:?}）",
            n,
            s.reason,
            s.kind
        );
        assert!(!s.reason.trim().is_empty(), "每条建议都必须有依据文案");
    }
}

// ══════════════════════════════════════════════════════════════
// 二 · HTTP 面：夹具
// ══════════════════════════════════════════════════════════════

struct Fixture {
    pool: PgPool,
    project: String,
    proposal_id: Uuid,
}

/// 一份**可入库**的置信度（v2：语义项与 AI 溯源成对，权重照 v2 权重表）。
fn confidence_json(
    score: f64,
    sampled: u32,
    matched: u32,
    type_compatible: f64,
    fit: Option<f64>,
    veto: Option<&str>,
) -> Value {
    let w = |n: &str| weight_of(n);
    let mut signals = vec![
        json!({"name": "sample_hit_rate", "value": hit_rate_value(sampled, matched), "weight": w("sample_hit_rate"), "sampled": sampled, "matched": matched}),
        json!({"name": "name_match", "value": 1.0, "weight": w("name_match")}),
        json!({"name": "not_all_null", "value": if sampled > 0 { 1.0 } else { 0.0 }, "weight": w("not_all_null")}),
        json!({"name": "type_compatible", "value": type_compatible, "weight": w("type_compatible")}),
        json!({"name": "target_is_key", "value": 1.0, "weight": w("target_is_key")}),
    ];
    if let Some(f) = fit {
        signals.push(json!({"name": SIGNAL_SEMANTIC_FIT, "value": f, "weight": w(SIGNAL_SEMANTIC_FIT)}));
    }
    let mut v = json!({
        "score": score,
        "band": Band::of(score).as_str(),
        "method": METHOD_V2,
        "signals": signals,
        "data_evidence": "sampled",
    });
    if let Some(sig_name) = veto {
        v["veto"] = json!({
            "signal": sig_name,
            "cap": veto_cap(sig_name).unwrap(),
            "reason": "夹具里的 veto",
        });
    }
    if fit.is_some() {
        v["ai"] = json!({
            "model": "fake-1",
            "prompt_sha": "deadbeef",
            "tokens": 0,
            "cache_hit": false,
            "rationale": "两端语义匹配",
        });
    }
    v
}

fn entity(key: &str, name: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddEntityType,
        target_key: name.to_string(),
        payload: json!({ "key": key, "display_name": name, "properties": [] }),
    }
}

fn relation(name: &str, confidence: Value) -> NewItem {
    NewItem {
        kind: ItemKind::AddLinkType,
        target_key: name.to_string(),
        payload: json!({
            "key": name,
            "from_type": "customer",
            "to_type": "order",
            "cardinality": "N:1",
            "semantic": "belongs_to",
            "evidence": [{"table": "t", "column": "c", "claim": "按命名约定推断"}],
            "confidence": confidence,
        }),
    }
}

async fn fixture(prefix: &str, items: Vec<NewItem>) -> Fixture {
    let pool = test_pool().await;
    common::migrate(&pool).await;

    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());

    let p = common::fresh_project(&projects, prefix).await;
    let v1 = projects.new_version(p.id).await.unwrap();
    store.save_snapshot(&Snapshot::new(v1.id, 1)).await.unwrap();
    let prop = proposals
        .create(
            p.id,
            NewProposal { base_version_id: v1.id, round: 1, extracted_by: None },
            &items,
        )
        .await
        .unwrap();

    Fixture { pool, project: p.name, proposal_id: prop.id }
}

/// 九条条目，六种建议各至少一条（序号 = 在数组里的位置）：
///
/// | # | 内容 | 期望 kind |
/// | --- | --- | --- |
/// | 1 | 实体（无置信度） | `none` |
/// | 2 | 1.00 / 5✓5 / fit 1.00 | `accept` |
/// | 3 | 1.00 / 3✓3 / fit 1.00 | `accept` |
/// | 4 | 0.88 / 5✓5 / fit 1.00 | `lean_accept` |
/// | 5 | 0.95 / 5✓5 / fit 0.00 | **`conflict`** |
/// | 6 | 0.45 / 0✓0 / veto all_null | `review` |
/// | 7 | 0.20 / 5✓0 / veto zero_hit | `reject` |
/// | 8 | 0.15 / 5✓5 / veto type_incompatible | `reject` |
/// | 9 | 实体（无置信度） | `none` |
fn mixed_items() -> Vec<NewItem> {
    vec![
        entity("customer", "客户"),
        relation("a_ok_1", confidence_json(1.00, 5, 5, 1.0, Some(1.00), None)),
        relation("a_ok_2", confidence_json(1.00, 3, 3, 1.0, Some(1.00), None)),
        relation("b_lean", confidence_json(0.88, 5, 5, 1.0, Some(1.00), None)),
        relation("c_conflict", confidence_json(0.95, 5, 5, 1.0, Some(0.00), None)),
        relation("d_low", confidence_json(0.45, 0, 0, 1.0, Some(1.00), Some("all_null"))),
        relation("e_zero_hit", confidence_json(0.20, 5, 0, 1.0, Some(0.50), Some("zero_hit"))),
        relation(
            "f_type_mismatch",
            confidence_json(0.15, 5, 5, 0.0, Some(0.00), Some("type_incompatible")),
        ),
        entity("order", "订单"),
    ]
}

fn args(f: &Fixture) -> ReviewArgs {
    ReviewArgs {
        project: f.project.clone(),
        proposal: f.proposal_id.to_string(),
        bind: "127.0.0.1:0".parse().unwrap(),
        token: TOKEN.to_string(),
    }
}

async fn app(f: &Fixture) -> Router {
    let state = prepare(&f.pool, &args(f)).await.expect("夹具应当通过启动闸门");
    build_router(state)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, String) {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"));
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&v).unwrap()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app
        .clone()
        .oneshot(req)
        .await
        .expect("oneshot 的错误类型是 Infallible");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 22).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn body_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("不是合法 JSON：{e}\n{s}"))
}

fn kinds_of(v: &Value) -> Vec<String> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|it| it["suggestion"]["kind"].as_str().unwrap().to_string())
        .collect()
}

async fn rows(f: &Fixture) -> Vec<ProposalItemRow> {
    PgProposalStore::new(f.pool.clone())
        .items(f.proposal_id)
        .await
        .unwrap()
}

// ══════════════════════════════════════════════════════════════
// 三 · HTTP 面：AC-008 / AC-005 / AC-006 / AC-007 / AC-004
// ══════════════════════════════════════════════════════════════

/// **AC-008** · 每条都带 `suggestion`，且六种 kind 与规则表对得上。
#[tokio::test]
async fn ac008_every_item_carries_a_suggestion_with_kind_and_reason() {
    let f = fixture("req112_kinds", mixed_items()).await;
    let app = app(&f).await;

    let (status, body) = call(&app, "GET", "/api/review", None).await;
    assert_eq!(status, StatusCode::OK, "读接口必须能用：{body}");
    let v = body_json(&body);

    let expected = vec![
        "none",        // #1 实体
        "accept",      // #2
        "accept",      // #3
        "lean_accept", // #4
        "conflict",    // #5
        "review",      // #6
        "reject",      // #7
        "reject",      // #8
        "none",        // #9 实体
    ];
    assert_eq!(kinds_of(&v), expected, "九条条目的建议必须逐条对得上规则表：{v}");

    for it in v["items"].as_array().unwrap() {
        let s = &it["suggestion"];
        assert!(
            s["kind"].as_str().is_some(),
            "每条都必须带 suggestion.kind：{it}"
        );
        let reason = s["reason"].as_str().expect("每条都必须带 suggestion.reason");
        assert!(!reason.is_empty(), "依据不许为空：{it}");
        assert!(reason.chars().count() <= 40, "依据 ≤ 40 字：{reason}");
        // 字段结构就是 `{kind, reason}` —— 不多不少（AC-008 的字段口径）。
        let keys: Vec<&String> = s.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 2, "suggestion 只有 kind / reason 两个键：{s}");
    }

    // 分组计数由后端给，且必须与逐条 kind 数出来的一致（AC-008 的「分组计数一致」）。
    let counts = &v["suggestion_counts"];
    for k in ["accept", "lean_accept", "review", "reject", "conflict", "none"] {
        let n = kinds_of(&v).iter().filter(|x| x.as_str() == k).count();
        assert_eq!(
            counts[k].as_u64().unwrap() as usize,
            n,
            "suggestion_counts.{k} 必须等于逐条统计"
        );
    }
    assert_eq!(counts["conflict"].as_u64(), Some(1));
    assert_eq!(counts["none"].as_u64(), Some(2));
}

/// **AC-005** · 建议**不写库**：拉一次 `/api/review` 之后，`proposal_items` 整行不变。
///
/// 判据取「整行逐字段相等」（`ProposalItemRow` 的 `PartialEq`），
/// 因此任何一列被写（包括 `decided_at` / `via`）都会红。
#[tokio::test]
async fn ac005_reading_the_review_api_writes_nothing() {
    let f = fixture("req112_zero_write", mixed_items()).await;
    let app = app(&f).await;

    let before = rows(&f).await;
    assert!(
        before.iter().all(|r| r.biz_decided_at.is_none()
            && r.model_decided_at.is_none()
            && r.biz_via.is_none()
            && r.model_via.is_none()),
        "夹具初始状态：两轨都未签、审计列为 NULL"
    );

    // 反复拉（模拟「页面刷新 + 多人打开」）—— 建议是纯函数，读一百次也不该有副作用。
    for _ in 0..3 {
        let (status, body) = call(&app, "GET", "/api/review", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    let (status, _) = call(
        &app,
        "GET",
        "/api/preview?track=biz&scope=unsigned&by=suggestion",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let after = rows(&f).await;
    assert_eq!(after, before, "⛔ 读接口（含预览）一行都不许改");
    assert!(
        after.iter().all(|r| r.biz_decided_at.is_none() && r.biz_via.is_none()),
        "decided_at / via 必须保持 NULL"
    );
    // 顺带把「建议本身」也钉住：读两次，建议逐字相同。
    let (_, a) = call(&app, "GET", "/api/review", None).await;
    let (_, b) = call(&app, "GET", "/api/review", None).await;
    assert_eq!(a, b, "同一份提案两次打开，建议必须逐字相同");
}

/// **AC-006** · 批量「按建议接受」：预览返回 `{count, items[]}`，且只含 accept / lean_accept。
#[tokio::test]
async fn ac006_preview_by_suggestion_lists_only_accept_and_lean_accept() {
    let f = fixture("req112_preview", mixed_items()).await;
    let app = app(&f).await;

    let (status, body) = call(
        &app,
        "GET",
        "/api/preview?track=biz&scope=unsigned&by=suggestion",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "按建议预览必须能用：{body}");
    let v = body_json(&body);

    assert_eq!(v["by"], json!("suggestion"), "口径要回显：{v}");
    assert_eq!(v["count"], json!(3), "只有 #2 #3（accept）与 #4（lean_accept）：{v}");
    let whitelist: Vec<i64> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_i64().unwrap())
        .collect();
    assert_eq!(whitelist, vec![2, 3, 4], "白名单必须恰好是这三条：{v}");
    assert_eq!(v["excluded"]["conflict"], json!(1), "冲突要单独报数：{v}");
    assert_eq!(v["excluded"]["review"], json!(1));
    assert_eq!(v["excluded"]["reject"], json!(2));
    assert_eq!(v["excluded"]["none"], json!(2));
    // 白名单 3 + 冲突 1 + 存疑 1 + 拒绝 2 + 无建议 2 = 9 条，一条不漏。
    assert_eq!(v["count"].as_u64().unwrap() + 1 + 1 + 2 + 2, 9);

    // ⭐ 白名单里的每一条，它的建议**确实**是 accept / lean_accept（不是页面凑出来的）。
    let (_, review_body) = call(&app, "GET", "/api/review", None).await;
    let rv = body_json(&review_body);
    for it in rv["items"].as_array().unwrap() {
        let seq = it["item"].as_i64().unwrap();
        let kind = it["suggestion"]["kind"].as_str().unwrap();
        if whitelist.contains(&seq) {
            assert!(
                kind == "accept" || kind == "lean_accept",
                "#{seq} 在白名单里，但建议是 {kind}"
            );
        } else {
            assert!(
                kind != "accept" && kind != "lean_accept",
                "#{seq} 建议是 {kind}，却不在白名单里"
            );
        }
    }

    // 口径不许混用：`by=suggestion` + `min_confidence` ⇒ 400。
    let (status, body) = call(
        &app,
        "GET",
        "/api/preview?track=biz&scope=unsigned&by=suggestion&min_confidence=0.9",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "两种口径混用必须报错：{body}");
    assert!(body.contains("min_confidence"), "要说清是哪个参数冲突：{body}");

    // 非法 by 值也要说话（不静默当成默认口径）。
    let (status, body) = call(&app, "GET", "/api/preview?track=biz&by=mood", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("by"), "要说清合法取值：{body}");
}

/// **AC-007** · `none` 组不参与批量：无置信度的条目**绝不**出现在白名单里。
#[tokio::test]
async fn ac007_none_group_never_enters_the_whitelist() {
    let f = fixture("req112_none", mixed_items()).await;
    let app = app(&f).await;

    let (_, body) = call(
        &app,
        "GET",
        "/api/preview?track=biz&scope=unsigned&by=suggestion",
        None,
    )
    .await;
    let v = body_json(&body);

    // 库里自己的事实：哪几条根本没有 confidence。
    let rows = rows(&f).await;
    let no_conf: Vec<i64> = rows
        .iter()
        .filter(|r| r.payload.get("confidence").map_or(true, |c| c.is_null()))
        .map(|r| r.seq as i64)
        .collect();
    assert_eq!(no_conf, vec![1, 9], "夹具里两条实体条目没有置信度");

    let whitelist: Vec<i64> = v["items"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
    for seq in &no_conf {
        assert!(
            !whitelist.contains(seq),
            "⛔ 无置信度的 #{seq} 出现在白名单里（AC-007）：{v}"
        );
    }
    assert_eq!(v["excluded"]["none"], json!(2), "无建议要单独报数：{v}");

    // 两条轨都验（白名单判据不该只对 biz 生效）。
    let (_, mbody) = call(
        &app,
        "GET",
        "/api/preview?track=model&scope=unsigned&by=suggestion",
        None,
    )
    .await;
    let mv = body_json(&mbody);
    let mwhitelist: Vec<i64> = mv["items"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
    for seq in &no_conf {
        assert!(!mwhitelist.contains(seq), "model 轨上 #{seq} 也不许进白名单");
    }
}

/// **AC-003** · 冲突条目不进白名单；**而且**手动把它 POST 进去也改不了建议。
#[tokio::test]
async fn ac003_conflict_cannot_be_forced_into_the_batch() {
    let f = fixture("req112_conflict", mixed_items()).await;
    let app = app(&f).await;

    let (_, body) = call(
        &app,
        "GET",
        "/api/preview?track=biz&scope=unsigned&by=suggestion",
        None,
    )
    .await;
    let v = body_json(&body);
    let whitelist: Vec<i64> = v["items"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
    assert!(!whitelist.contains(&5), "冲突条目 #5 不许进白名单：{v}");

    // 人可以在页面上**逐条**签它（那是人的决定，不是建议替他做的）——
    // 但签完之后，**建议本身不变**，而且它仍然不在批量白名单里。
    let (status, reply) = call(
        &app,
        "POST",
        "/api/decide",
        Some(json!({"item": 5, "track": "biz", "decision": "accept"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "逐条签必须能用：{reply}");

    let (_, after) = call(&app, "GET", "/api/review", None).await;
    let av = body_json(&after);
    let five = av["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|it| it["item"] == json!(5))
        .expect("#5 必须在列表里");
    assert_eq!(
        five["suggestion"]["kind"],
        json!("conflict"),
        "签完之后建议必须**仍是**冲突（建议不是决策的回声）"
    );

    // 冲突条已签 ⇒ 它连「未签」都不是了；再预览，白名单里依旧没有它。
    let (_, again) = call(
        &app,
        "GET",
        "/api/preview?track=biz&scope=unsigned&by=suggestion",
        None,
    )
    .await;
    let gv = body_json(&again);
    let wl: Vec<i64> = gv["items"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect();
    assert!(!wl.contains(&5), "签过之后也不许混进白名单：{gv}");
    assert_eq!(gv["excluded"]["decided"], json!(1), "已签条目要单独报数：{gv}");
}

/// **AC-004** · 清空所有 `ONTOK_LLM_*` 之后，建议链路照常工作、且**零 token**。
///
/// 两件事分开证：
/// 1. **环境层**：把 `ONTOK_LLM_*` 全删掉 ⇒ `/api/review` 仍然 200、每条带建议。
///    若这条链路上真有模型调用，删掉 `ONTOK_LLM_MODEL` / `ONTOK_LLM_BASE_URL`
///    之后它会**失败或降级**（REQ-109 的网关是 fail-closed 的）。
/// 2. **数据层**：建议里的 `signals` 与 `confidence.ai.tokens` 无关 ——
///    拉一遍接口，`ai.tokens` 之和不变（0 也是不变）。
#[tokio::test]
async fn ac004_suggestion_chain_works_with_all_llm_env_cleared() {
    let f = fixture("req112_no_llm", mixed_items()).await;
    let app = app(&f).await;

    let before = rows(&f).await;
    let tokens_of = |rows: &[ProposalItemRow]| -> u64 {
        rows.iter()
            .map(|r| {
                r.payload
                    .get("confidence")
                    .and_then(|c| c.get("ai"))
                    .and_then(|a| a.get("tokens"))
                    .and_then(|t| t.as_u64())
                    .unwrap_or(0)
            })
            .sum()
    };
    let tokens_before = tokens_of(&before);

    let cleared: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("ONTOK_LLM"))
        .collect();
    for k in &cleared {
        std::env::remove_var(k);
    }
    let remaining = std::env::vars().filter(|(k, _)| k.starts_with("ONTOK_LLM")).count();
    assert_eq!(remaining, 0, "本进程里不许还剩任何 ONTOK_LLM_*");

    let (status, body) = call(&app, "GET", "/api/review", None).await;
    assert_eq!(status, StatusCode::OK, "清空 LLM 环境后读接口必须照常：{body}");
    let v = body_json(&body);
    assert_eq!(
        v["items"].as_array().unwrap().len(),
        9,
        "条目一条都不能少（没有「AI 不可用就跳过」）"
    );
    for it in v["items"].as_array().unwrap() {
        assert!(
            it["suggestion"]["kind"].as_str().is_some(),
            "清空 LLM 环境后每条仍必须带建议：{it}"
        );
    }

    let after = rows(&f).await;
    assert_eq!(tokens_of(&after), tokens_before, "⛔ 建议链路不许产生任何 token");
    assert_eq!(after, before, "⛔ 也不许顺带写库（AC-005 的同一件事）");
}

/// **AC-004** · 源码面：建议链路上**没有**任何 LLM 调用点。
///
/// 为什么还要这一条：环境层只能证明「删掉环境变量也能跑」，
/// 证明不了「没有引入一个默认端点的调用」。这里直接把**调用点**扫成 0。
#[test]
fn ac004_suggestion_chain_has_no_llm_call_site() {
    let mut files = vec![
        common::read_source("src/review_ui/http.rs"),
        common::read_source("src/review_ui/page.rs"),
        common::read_source("src/review_ui/mod.rs"),
        common::read_source("src/domain/confidence.rs"),
    ];
    // 剥注释：注释里可以**谈论**模型（例如解释「为什么零模型」），
    // 但代码里不许出现调用点/环境变量名（与仓内其它源码守卫同一口径）。
    for src in files.iter_mut() {
        *src = common::strip_comments(src);
    }
    let needles = [
        "ai_semantic",
        "ONTOK_LLM",
        "ureq",
        "LlmClient",
        "ExtractionAgent",
        "chat_completion",
        "chat/completions",
        "llm_client",
    ];
    for (i, src) in files.iter().enumerate() {
        for needle in needles {
            assert!(
                !src.contains(needle),
                "建议链路上出现了模型调用相关标识 {needle:?}（第 {i} 个文件）—— \
                 REQ-112 的硬约束是**零模型调用**"
            );
        }
    }

    // 反向自检（纪律 E）：扫描器**不是恒真**。
    // 同一套 needle 去扫那个**真的会调模型**的模块，必须命中 ——
    // 否则上面那一串 `contains` 只是「扫了个寂寞」。
    let real_llm = common::read_source("src/extract/ai_semantic.rs");
    assert!(
        needles.iter().any(|n| real_llm.contains(n)),
        "自检失败：在真正的 AI 模块里都扫不到这些 needle，说明扫描口径已经失效"
    );
}

/// **AC-006 / 需求范围** · 没有新增写端点。
#[tokio::test]
async fn no_suggestion_specific_write_endpoint_exists() {
    let f = fixture("req112_no_new_endpoint", mixed_items()).await;
    let app = app(&f).await;

    let before = rows(&f).await;
    for uri in ["/api/suggestion", "/api/decide-bulk", "/api/suggest/batch"] {
        let (status, body) = call(
            &app,
            "POST",
            uri,
            Some(json!({"items": [2, 3, 4], "track": "biz", "decision": "accept"})),
        )
        .await;
        assert!(
            status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
            "⛔ 不许存在批量写端点 {uri}，实际 {status}：{body}"
        );
    }
    assert_eq!(rows(&f).await, before, "⛔ 不存在的端点当然也不许写库");
}

/// **AC-008** · 页面：建议列 + 依据 + 冲突单独分组（排最前）。
#[test]
fn ac008_page_shows_suggestion_and_puts_the_conflict_group_first() {
    let html = ontok::review_ui::page::REVIEW_HTML;

    // 建议列与依据
    assert!(html.contains("建议："), "页面必须有建议列");
    assert!(html.contains("依据："), "页面必须显示建议依据");
    assert!(html.contains("suggestion"), "页面必须读后端发的 suggestion 字段");
    for k in ["accept", "lean_accept", "review", "reject", "conflict", "none"] {
        assert!(html.contains(k), "页面必须认识 kind = {k}");
    }
    // 冲突组：单独成组、排最前、文案说清「需人工判」
    assert!(html.contains("冲突 —— 证据支持但 AI 否定"), "冲突组要有独立抬头");
    assert!(html.contains("groupHead("), "分组要走同一个抬头构造函数");
    let conflict_at = html.find("⚠ 冲突 —— 证据支持但 AI 否定").unwrap();
    let rest_at = html.find("\"其余条目\"").unwrap();
    assert!(
        conflict_at < rest_at,
        "冲突组必须排在「其余条目」之前（AC-008：冲突单独分组、排在列表最前）"
    );
    // 冲突卡片不能在「其余」里被再渲染一次（顺序上先分组收集再渲染）
    assert!(
        html.contains("kind === \"conflict\""),
        "页面必须按 kind 把冲突挑出来单独渲染"
    );
    // 按建议筛选 + 按建议批量 + 不新增批量写接口
    assert!(html.contains("by=suggestion"), "批量必须走预览接口的 by=suggestion");
    assert!(html.contains("f-sugg"), "页面必须有「按建议筛选」");
    assert!(!html.contains("decide-bulk"), "⛔ 页面不许有批量写接口");
    // 分组计数来自后端（页面不自己数）
    assert!(html.contains("suggestion_counts"), "分组计数必须取后端那一份");
}
