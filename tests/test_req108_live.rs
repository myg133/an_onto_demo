//! REQ-108 · **真实源库实测**（AC-002 / AC-006 / AC-007）
//!
//! # 这个文件为什么必须存在
//!
//! AC-006 是本需求**最核心、最可证伪**的一条：「9 条已知错误必须全部落拒绝」。
//! 它不可能由构造样本证明 —— 构造样本只能证明「我写了一个能拒绝它们的函数」，
//! 证明不了「在真实数据上它们真的被拒绝」。所以这条判据必须连真库跑。
//!
//! # 怎么跑
//!
//! ```text
//! export SRC_URL="postgresql://…（只读源库，口令不入库）"
//! cargo test --test test_req108_live -- --nocapture
//! ```
//!
//! 连接串只从环境变量读（`SRC_URL` 或 `ONTOK_SOURCE_DATABASE_URL`），
//! **代码与文档里都没有它**。没设这两个变量时本文件**直接跳过**
//! （打印一行 `SKIP` 并返回）—— 不用 `#[ignore]`：
//! 那不是「不跑」，而是「没有源库可跑」，两者的透明度不一样。
//!
//! # 它**不**证明什么
//!
//! - 不证明 81/82 条关系的语义对错（那要人判）；
//! - 不证明权重是最优的（那要校准数据，见 `ontok confidence calibrate`）；
//! - 不证明「换个客户库也对」—— 本判据的证据强度就是**这一个库**。

use ontok::domain::confidence::{Band, BandCounts, DataEvidence};
use ontok::extract::introspect::{introspect_with_keys, KeyCatalog, SchemaReport};
use ontok::extract::names::TableNames;
use ontok::extract::propose::{extract, link_candidates, score_links, LinkCandidate};
use ontok::extract::sample::{sample_all_with, sample_pool, sample_one, SampleSpec, SAMPLE_TIMEOUT};

/// §零 的 9 条**已知错误**（表, 列, 目标表）。
const KNOWN_WRONG: [(&str, &str, &str); 9] = [
    ("balance_operation_log", "batch_id", "gift_code_batch"),
    ("instance_manager", "session_id", "simulate_login_sessions"),
    ("internal_operation_records", "batch_id", "gift_code_batch"),
    ("llm_model_peak_config", "model_id", "llm_model"),
    ("llm_model_sort", "model_id", "llm_model"),
    ("llm_provider", "model_id", "llm_model"),
    ("llm_provider_groups", "model_id", "llm_model"),
    ("dim_customer_binding", "channel_code", "marketing_channel"),
    ("marketing_channel_customer", "channel_code", "marketing_channel"),
];

/// §零 末尾的 5 条**部分命中**（表, 列）。
const PARTIAL_HITS: [(&str, &str); 5] = [
    ("dim_customer_binding", "customer_id"),
    ("enterprise_recharge_order", "customer_id"),
    ("order", "subscription_id"),
    ("service_apps", "channel_id"),
    ("usage_reconciliation_log", "subscription_id"),
];

fn source_url() -> Option<String> {
    for key in ["SRC_URL", "ONTOK_SOURCE_DATABASE_URL"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn names() -> TableNames {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/extract/table_names.json"
    ))
    .expect("读不到翻译表");
    TableNames::load(&raw).expect("翻译表不合法")
}

/// 一条**候选**关系（不要求它真的出现在提案里）。
///
/// 为什么要能构造「提案里没有的」候选：E8/E9 在 REQ-028 的歧义闸下
/// 压根不会被推断出来（`rpt_dws_channel` 也让 `channel` 成了歧义词干）。
/// 但 AC-006 问的是「这 9 条关系**如果被提案**，评分卡会不会拒绝」——
/// 所以这里按 §零 的坐标直接构造，逐条验证评分卡本身。
fn candidate_for(report: &SchemaReport, keys: &KeyCatalog, from: &str, column: &str, to: &str) -> LinkCandidate {
    let from_col = report
        .tables
        .iter()
        .find(|t| t.name == from)
        .and_then(|t| t.columns.iter().find(|c| c.name == column));
    assert!(from_col.is_some(), "源列 {from}.{column} 不存在 —— 源库 schema 变了，判据要跟着改");
    assert!(
        report.tables.iter().any(|t| t.name == to),
        "目标表 {to} 不存在 —— 源库 schema 变了，判据要跟着改"
    );
    LinkCandidate {
        item_index: 0,
        from_table: from.to_string(),
        from_column: column.to_string(),
        to_table: to.to_string(),
        from_key: keys.get(from).and_then(|k| k.key_column()).map(str::to_string),
        to_key_column: keys.get(to).and_then(|k| k.key_column()).map(str::to_string),
    }
}

fn spec_of(c: &LinkCandidate) -> SampleSpec {
    c.sample_spec().expect("这 9 条的目标表都有单列主键")
}

/// AC-002 · **只读 + 确定性**：两次采样得到同一份 digest，且会话读回 `on`。
#[tokio::test]
async fn test_req108_ac002_sampling_is_read_only_and_deterministic() {
    let Some(url) = source_url() else {
        eprintln!("SKIP test_req108_ac002：未设置 SRC_URL / ONTOK_SOURCE_DATABASE_URL");
        return;
    };
    // ① 只读会话（sample_pool 内部会 SHOW transaction_read_only 并 fail-closed）
    let pool = sample_pool(&url).await.expect("只读采样池");
    let ro: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(&pool)
        .await
        .expect("读不回 transaction_read_only");
    assert_eq!(ro, "on", "AC-002：采样会话必须只读（SHOW transaction_read_only = on）");

    // 采样坐标一律来自**目录事实**（主键列），不猜
    let (report, keys) = introspect_with_keys(&url).await.expect("只读勘查");
    let specs: Vec<SampleSpec> = KNOWN_WRONG
        .iter()
        .map(|(f, c, t)| spec_of(&candidate_for(&report, &keys, f, c, t)))
        .collect();

    // ② 两次运行 ⇒ 逐条 digest 相同（同数据同结果）
    //
    // 超时放宽到 20s：**墙钟超时不是采样结果**（那是「这次没采到」）。
    // 用生产那 2s 跑判据，一条慢查询就会让「确定性」变成偶发红 ——
    // 那是把「SQL 或数据变了」与「机器忙」混成同一件事故。
    // 改成：超时单独断言（下面 `timed_out` 必须为空），确定性比的是真采到的那些。
    let slow = std::time::Duration::from_secs(20);
    let a = sample_all_with(&pool, &specs, slow).await;
    let b = sample_all_with(&pool, &specs, slow).await;
    let timed_out: Vec<&str> = a
        .iter()
        .zip(KNOWN_WRONG.iter())
        .filter(|(e, _)| e.reason == Some("timeout"))
        .map(|(_, (f, c, _))| *f)
        .collect();
    assert!(
        timed_out.is_empty(),
        "采样超时（20s）：{timed_out:?} —— 确定性判据在超时上无从谈起，先查源库负载/索引"
    );
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        let (f, c, t) = KNOWN_WRONG[i];
        assert_eq!(
            x, y,
            "AC-002：{f}.{c}→{t} 两次采样不同（{:?} vs {:?}）—— 样本必须由固定排序+固定上界决定",
            x, y
        );
    }
    assert_eq!(a, b, "AC-002：同一份数据两次采样必须逐字段相同（含 digest）");
    for (i, e) in a.iter().enumerate() {
        let (f, c, t) = KNOWN_WRONG[i];
        assert!(
            e.digest.is_some() || e.sampled == 0,
            "{f}.{c}→{t}：有样本就必须有 digest（只存哈希，不存原值）"
        );
        if let Some(d) = &e.digest {
            assert!(d.starts_with("sha256:"), "digest 形状：{d}");
            assert_eq!(d.len(), "sha256:".len() + 64);
        }
    }
    pool.close().await;
}

/// AC-006 · **9 条已知错误全部落「拒绝」**（逐条打印分数，失败时点名）。
///
/// 判据是「**一条不落**」：任何一条的 band 不是 `rejected` 就红。
#[tokio::test]
async fn test_req108_ac006_nine_known_errors_all_rejected() {
    let Some(url) = source_url() else {
        eprintln!("SKIP test_req108_ac006：未设置 SRC_URL / ONTOK_SOURCE_DATABASE_URL");
        return;
    };
    let (report, keys) = introspect_with_keys(&url).await.expect("只读勘查");
    let pool = sample_pool(&url).await.expect("只读采样池");
    // 抽取只跑一次（9 条判据共用同一份 items —— 它们判的是同一个「当前本体」）
    let names = names();
    let outcome = extract(&report, &names, "req108-live", "test", "0").expect("抽取");
    let produced_links: Vec<(String, String, String)> = outcome
        .proposal
        .items
        .iter()
        .filter(|it| it["kind"] == "add_link_type")
        .filter_map(|it| {
            Some((
                it["payload"]["from_type"].as_str()?.to_string(),
                it["payload"]["evidence"][0]["column"].as_str()?.to_string(),
                it["payload"]["to_type"].as_str()?.to_string(),
            ))
        })
        .collect();

    println!("\n── AC-006 · 9 条已知错误（逐条实测）──────────────");
    let mut failures: Vec<String> = Vec::new();
    for (f, c, t) in KNOWN_WRONG {
        let cand = candidate_for(&report, &keys, f, c, t);
        // 关系最终会不会被推断出来是另一回事（E8/E9 因歧义闸压根不产出）；
        // 这里问的是「**如果**它作为一条关系被提案，评分卡判什么」。
        let produced = produced_links.iter().any(|(pf, pc, pt)| pf == f && pc == c && pt == t);
        let evidence = sample_one(&pool, &spec_of(&cand), SAMPLE_TIMEOUT).await;
        let mut items = vec![serde_json::json!({
            "kind": "add_link_type",
            "target": format!("{f}→{t}"),
            "payload": {
                "key": format!("{f}_{c}__{t}"),
                "from_type": f, "to_type": t,
                "cardinality": "N:1", "semantic": "belongs_to",
                "evidence": [{"table": f, "column": c, "claim": "命名约定"}]
            }
        })];
        let mut ev = std::collections::HashMap::new();
        ev.insert(0usize, evidence.clone());
        let scored = score_links(&items, &report, &keys, &ev);
        let conf = &scored[0].1;
        ontok::extract::propose::attach_confidence(&mut items, &scored);
        println!(
            "  {f}.{c} → {t}\n      score={:.2} band={} veto={} sampled={} matched={} produced={}",
            conf.score,
            conf.band.as_str(),
            conf.veto.as_ref().map(|v| v.signal.as_str()).unwrap_or("-"),
            evidence.sampled,
            evidence.matched,
            produced
        );
        if conf.band != Band::Rejected {
            failures.push(format!(
                "{f}.{c} → {t}：band={} score={:.2}（必须 < 0.30）",
                conf.band.as_str(),
                conf.score
            ));
        }
        // 产物里只许出现聚合量，不许出现原值
        let text = serde_json::to_string(&items).unwrap();
        assert!(
            !text.contains("\"values\"") && !text.contains("\"sample_values\""),
            "产物里出现了原值字段"
        );
    }
    assert!(
        failures.is_empty(),
        "AC-006：以下已知错误**没有**落进拒绝档：\n{}",
        failures.join("\n")
    );
    pool.close().await;
}

/// AC-007 · **部分命中不得落「高」**：机制在地（部分命中 ⇒ ≤0.75）。
///
/// ⚠️ 这条判据的**口径**必须说清楚（见交付报告 §「AC 自身的问题」）：
/// 交付它是「机制判据」—— 只要 `0 < matched < sampled`，分数就被 0.75 压住。
/// 至于验收文档里那 5 条**具体关系**在**本采样口径下**是否真的部分命中，
/// 取决于「按主键取前 5 行」取到的是哪些行；实测有 3 条是 5/5 全命中
/// （详见 `test_req108_ac007_live_report` 的打印）。
#[test]
fn test_req108_ac007_partial_hit_never_scores_high() {
    use ontok::domain::confidence::{score, SampleFacts, ScoreInput, TargetKey, TypeFamily};
    for (sampled, matched) in [(5u32, 1u32), (5, 2), (5, 3), (5, 4), (4, 1), (2, 1)] {
        let c = score(&ScoreInput {
            column: "customer_id".into(),
            to_table: "customers".into(),
            from_type: TypeFamily::Str,
            to_type: Some(TypeFamily::Str),
            target_key: TargetKey::Primary,
            data: Some(SampleFacts { sampled, matched, digest: None, reason: None }),
            composite_target_key: false,
            polymorphic: false,
            self_ref: false,
        });
        assert!(
            c.score <= 0.75 && c.band != Band::High,
            "{matched}/{sampled} 部分命中不得落高档，实际 score={} band={}",
            c.score,
            c.band.as_str()
        );
        assert_eq!(c.veto.as_ref().unwrap().signal, "partial_hit");
    }
}

/// 实测报告：82 条真实关系的**分档分布** + 5 条部分命中的实测命中率。
///
/// 这条**只打印不断言分布数字**（那些数字是数据快照的性质，不是代码的性质）——
/// 断言它们会把判据变成「源库别变」。它断言的是**口径**：
/// 四档之和 == 关系条数，且每条都有一个 `method`。
#[tokio::test]
async fn test_req108_live_band_distribution_report() {
    let Some(url) = source_url() else {
        eprintln!("SKIP test_req108_live_band_distribution_report：未设置 SRC_URL / ONTOK_SOURCE_DATABASE_URL");
        return;
    };
    let (report, keys) = introspect_with_keys(&url).await.expect("只读勘查");
    let names = names();
    let outcome = extract(&report, &names, "req108-live", "test", "0").expect("抽取");
    let candidates = link_candidates(&outcome.proposal.items, &keys);

    let pool = sample_pool(&url).await.expect("只读采样池");
    let mut evidence = std::collections::HashMap::new();
    for c in &candidates {
        let e = match c.sample_spec() {
            Some(spec) => sample_one(&pool, &spec, SAMPLE_TIMEOUT).await,
            None => ontok::extract::sample::SampleEvidence::none("no_key_column"),
        };
        evidence.insert(c.item_index, e);
    }
    let scored = score_links(&outcome.proposal.items, &report, &keys, &evidence);
    let counts = BandCounts::of(scored.iter().map(|(_, c)| &c.band));

    println!("\n── 真实源库分档分布 ──────────────────────────────");
    println!("  关系总数    : {}", scored.len());
    println!(
        "  高 {} · 中 {} · 低 {} · 拒绝 {}",
        counts.high, counts.middle, counts.low, counts.rejected
    );
    println!("  进主提案    : {}（高+中）", counts.high + counts.middle);
    let no_data = scored
        .iter()
        .filter(|(_, c)| c.data_evidence == DataEvidence::None)
        .count();
    println!("  无数据证据  : {no_data}");
    println!("\n  ── 5 条「部分命中」在本采样口径下的实测 ──");
    for (t, col) in PARTIAL_HITS {
        if let Some((idx, c)) = scored.iter().find(|(i, _)| {
            let it = &outcome.proposal.items[*i];
            it["payload"]["from_type"] == t && it["payload"]["evidence"][0]["column"] == col
        }) {
            let e = evidence.get(idx).cloned().unwrap_or_default();
            println!(
                "  {t}.{col}: {}/{} → score={:.2} band={}",
                e.matched,
                e.sampled,
                c.score,
                c.band.as_str()
            );
        } else {
            println!("  {t}.{col}: 未被推断出来（不在提案里）");
        }
    }

    // 口径断言：四档之和 == 关系条数；每条都有 method；高档不含部分命中
    assert_eq!(counts.total(), scored.len(), "四档之和必须等于关系条数");
    for (_, c) in &scored {
        assert_eq!(c.method, ontok::domain::confidence::METHOD, "每条分数都必须带 method");
        if let Some(v) = &c.veto {
            if v.signal == "partial_hit" {
                assert_ne!(c.band, Band::High, "部分命中落进了高档 —— AC-007 破防");
            }
        }
    }
    pool.close().await;
}
