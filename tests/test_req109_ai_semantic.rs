//! REQ-109 · AI 语义项接入 —— **无 key 也能全绿**（AC-011）。
//!
//! 全部用 `ScriptedTransport`（`agent::llm` 的测试替身）：
//! 不联网、不花钱、结果确定。真实端点只在手工验证时用，**不进自动化测试**。
//!
//! 覆盖：
//!
//! | AC | 本文件的用例 |
//! | --- | --- |
//! | AC-001 | `ac001_ai_off_is_byte_identical_to_scorecard_v1` |
//! | AC-003 | `ac003_cache_hit_means_zero_calls` |
//! | AC-004 | `ac004_same_input_twice_gives_identical_bytes` |
//! | AC-005 | `ac005_prompt_change_invalidates_the_cache` |
//! | AC-006 | `ac006_budget_stops_before_the_next_call` |
//! | AC-007 | `ac007_semantic_fit_cannot_flip_any_veto` |
//! | AC-008 | `ac008_fabricated_cite_voids_the_judgement` |
//! | AC-009 | `ac009_bad_rationale_voids_the_judgement` |
//! | AC-010 | `ac010_customer_view_has_zero_ai_leakage` |
//! | R6 | `r6_missing_semantic_is_absent_not_zero` |
//! | R1 | `r1_fingerprint_changes_with_every_component` |
//!
//! ⚠️ 本文件**不碰数据库**：缓存用 `MemoryAiCache`。`ai_cache` 表的读写由
//! `db::ai_cache` 负责，真库用例在 `tests/test_req109_ai_cache_pg.rs`（无库则跳过）。

use std::collections::HashMap;

use ontok::agent::llm::protocol::{Choice as ChatChoice, Message_};
use ontok::agent::llm::{ChatResponse, ScriptedTransport, Usage};
use ontok::domain::confidence as cf;
use ontok::extract::ai_semantic::*;
use ontok::extract::introspect::{ColumnMeta, KeyCatalog, SchemaReport, TableKeys, TableMeta};

// ──────────────────────────────────────────────────────────
// 构造器
// ──────────────────────────────────────────────────────────

fn col(name: &str, ty: &str, comment: Option<&str>) -> SliceColumn {
    SliceColumn {
        name: name.into(),
        data_type: ty.into(),
        comment: comment.map(str::to_string),
    }
}

/// 一条「源 `llm_model_sort.model_id` → 目标 `llm_model.id`」的请求。
///
/// 这就是需求里点名的已知错关系形态：目标表另有业务键 `model_id`，
/// 规则只能指 `id`。**本文件只关心协议/缓存/预算，不关心模型判得对不对**
/// （BA 裁定：准确率是观察项，不是通过条件）。
fn req_n(index: usize, from_column: &str) -> JudgeRequest {
    JudgeRequest {
        item_index: index,
        from_table: "llm_model_sort".into(),
        from_column: from_column.into(),
        to_table: "llm_model".into(),
        to_column: "id".into(),
        to_candidates: vec!["id".into(), "model_id".into()],
        schema_slice: vec![
            SliceTable {
                table: "llm_model_sort".into(),
                columns: vec![
                    col("id", "character varying", Some("主键UUID")),
                    col("model_id", "character varying", Some("模型ID")),
                ],
            },
            SliceTable {
                table: "llm_model".into(),
                columns: vec![
                    col("id", "character varying", Some("主键UUID")),
                    col("model_id", "character varying", Some("模型ID(如: GLM-5)")),
                    col("model_name", "character varying", Some("模型名称")),
                ],
            },
        ],
    }
}

fn req(index: usize) -> JudgeRequest {
    req_n(index, "model_id")
}

/// 一份**合法**的模型输出（choice 落在候选内、cites 真实、rationale 合规）。
fn good_answer() -> String {
    serde_json::json!({
        "choice": {"table": "llm_model", "column": "model_id"},
        "semantic_fit": 0.2,
        "rationale": "目标表另有业务键 model_id，本列引用的应是业务键而不是 UUID 主键",
        "cites": [{"table": "llm_model", "column": "model_id"}]
    })
    .to_string()
}

fn answer(content: &str, tokens: u64) -> ChatResponse {
    ChatResponse {
        choices: vec![ChatChoice {
            message: Message_ {
                content: Some(content.to_string()),
                reasoning_content: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: Some(Usage {
            prompt_tokens: tokens,
            completion_tokens: 0,
            total_tokens: Some(tokens),
            completion_tokens_details: None,
        }),
    }
}

fn transport(content: impl Into<String>, tokens: u64) -> ScriptedTransport {
    let c = content.into();
    ScriptedTransport::always_answer(move |_| Ok(answer(&c, tokens)))
}

// ──────────────────────────────────────────────────────────
// R1 · 指纹
// ──────────────────────────────────────────────────────────

/// **六项缺一项就换指纹** —— R1 的原话是「缺 prompt 项等于静默错」。
///
/// 变异自证：把 `fingerprint` 里的 `prompt_sha` 那一行删掉，本用例第一条红。
#[test]
fn r1_fingerprint_changes_with_every_component() {
    let r = req(1);
    let base = r.fingerprint("prompt-a", "qwen", "agent-v1");
    assert_eq!(base.len(), 64, "sha256 十六进制 64 字符");

    // ① 同一份输入 ⇒ 同一个指纹（否则缓存永远不命中）
    assert_eq!(base, r.fingerprint("prompt-a", "qwen", "agent-v1"));

    // ②..⑤ 四项任一变化都必须换指纹
    for (what, fp) in [
        ("prompt", r.fingerprint("prompt-b", "qwen", "agent-v1")),
        ("model", r.fingerprint("prompt-a", "other-model", "agent-v1")),
        ("agent_version", r.fingerprint("prompt-a", "qwen", "agent-v2")),
    ] {
        assert_ne!(base, fp, "{what} 变了，指纹必须变（否则会复用不该复用的缓存）");
    }

    // ⑥ 关系变了（同一 schema 切片之外的另一条关系）
    let mut r2 = r.clone();
    r2.from_column = "created_by".into();
    assert_ne!(base, r2.fingerprint("prompt-a", "qwen", "agent-v1"), "关系换了要换指纹");

    // ⑦ schema 变了（加一列 / 改注释）
    let mut r3 = r.clone();
    r3.schema_slice[1].columns[1].comment = Some("改过的注释".into());
    assert_ne!(base, r3.fingerprint("prompt-a", "qwen", "agent-v1"), "schema 变了要换指纹");
}

/// `schema_digest` 对列序**不敏感**（排序后序列化）—— 同一份 schema 必须同一指纹。
#[test]
fn r1_schema_digest_ignores_column_order() {
    let a = req(1);
    let mut b = a.clone();
    b.schema_slice[1].columns.reverse();
    b.schema_slice.reverse();
    assert_eq!(a.schema_digest(), b.schema_digest());
    assert_eq!(
        a.fingerprint("p", "m", "v"),
        b.fingerprint("p", "m", "v"),
        "换个列序不该让缓存失效（那只是 pg_catalog 的返回顺序）"
    );
}

// ──────────────────────────────────────────────────────────
// AC-003 · 缓存命中 ⇒ 0 次调用
// ──────────────────────────────────────────────────────────

#[test]
fn ac003_cache_hit_means_zero_calls() {
    // 两条**不同**的关系（同一关系会被同一次运行内的缓存直接命中 —— 那是对的）
    let reqs = vec![req_n(1, "model_id"), req_n(2, "created_by")];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(good_answer(), 17);

    // ① 第一次：都未命中 ⇒ 每条一次调用
    let (j1, s1) = judge_all(
        &mut t, &mut cache, &reqs, "qwen36-27B", Prompts::standard(), 100_000, false, 300,
    );
    assert_eq!(s1.calls, 2);
    assert_eq!(s1.cache_hits, 0);
    assert_eq!(j1.len(), 2);
    assert!(j1.values().all(|j| !j.cache_hit));
    assert_eq!(t.call_count(), 2);
    assert_eq!(cache.fresh.len(), 2, "两条判断都要落缓存");

    // ② 第二次：全命中 ⇒ **0 次调用**
    let (j2, s2) = judge_all(
        &mut t, &mut cache, &reqs, "qwen36-27B", Prompts::standard(), 100_000, false, 300,
    );
    assert_eq!(s2.calls, 0, "命中缓存时**绝不调用**（R2）");
    assert_eq!(s2.cache_hits, 2);
    assert_eq!(t.call_count(), 2, "调用次数必须还是 2");
    assert!(j2.values().all(|j| j.cache_hit), "命中要如实标 cache_hit = true");
    assert_eq!(j2[&1].semantic_fit, j1[&1].semantic_fit);
    assert_eq!(cache.fresh.len(), 2, "命中不产生新的待写条目");

    // ③ 清空缓存（等价于「删掉 ai_cache 表内容」）⇒ 必须重新调用
    cache.clear();
    let (_, s3) = judge_all(
        &mut t, &mut cache, &reqs[..1], "qwen36-27B", Prompts::standard(), 100_000, false, 300,
    );
    assert_eq!(s3.calls, 1, "缓存被清掉后必须重新调用一次");
    assert_eq!(t.call_count(), 3);
}

/// R3 · `--refresh`：忽略缓存并覆盖，且打印用的计数是「强制刷新 N 条」。
#[test]
fn r3_refresh_ignores_cache_and_overwrites() {
    let reqs = vec![req(1)];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(good_answer(), 17);

    let (_, s1) = judge_all(
        &mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300,
    );
    assert_eq!(s1.calls, 1);

    // refresh = true ⇒ 即使命中也不许用，必须再调一次并覆盖
    let (j2, s2) = judge_all(
        &mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, true, 300,
    );
    assert_eq!(s2.calls, 1, "--refresh 必须真的重新调用");
    assert_eq!(s2.cache_hits, 0);
    assert_eq!(s2.refresh_ignored, 1, "「强制刷新 N 条」要能报出来");
    assert!(j2.values().all(|j| !j.cache_hit));
}

// ──────────────────────────────────────────────────────────
// AC-005 · 改 prompt ⇒ 指纹变 ⇒ 重新调用
// ──────────────────────────────────────────────────────────

#[test]
fn ac005_prompt_change_invalidates_the_cache() {
    let reqs = vec![req(1)];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(good_answer(), 17);

    let p1 = Prompts::standard();
    let (_, s1) = judge_all(&mut t, &mut cache, &reqs, "m", p1.clone(), 100_000, false, 300);
    assert_eq!(s1.calls, 1);

    // 改一个字（**只改提示词，输入完全不变**）
    let mut altered = SYSTEM_PROMPT.to_string();
    altered.push('。');
    let p2 = Prompts::from_system(altered);
    assert_ne!(p1.sha256, p2.sha256, "提示词哈希必须变");

    let (_, s2) = judge_all(&mut t, &mut cache, &reqs, "m", p2, 100_000, false, 300);
    assert_eq!(s2.calls, 1, "prompt 变了必须重新调用（R1：缺 prompt 项等于静默错）");
    assert_eq!(s2.cache_hits, 0, "不许复用旧提示词的结果");
    assert_eq!(t.call_count(), 2);
}

// ──────────────────────────────────────────────────────────
// AC-006 · 预算闸
// ──────────────────────────────────────────────────────────

/// 预算 1 token：**第一次调用之后立即停**，不再发起第二次。
#[test]
fn ac006_budget_stops_before_the_next_call() {
    let reqs = vec![req(1), req(2), req(3)];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(good_answer(), 17);

    let (j, s) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 1, false, 300);
    assert_eq!(s.calls, 1, "预算 1 ⇒ 只允许第一次调用");
    assert_eq!(t.call_count(), 1);
    assert_eq!(s.tokens_spent, 17, "越过预算的那一笔照样记账（钱已经花了）");
    assert_eq!(s.budget_stopped, Some(2), "要能报出「停在第几条」");
    assert!(s.over_budget(), "装配层据此**不出产物**");
    assert_eq!(j.len(), 1, "已判的那一条仍在（不是 0 分，是判过了）");
}

/// R5 · `--llm` 必须带预算；0 与缺失都要拒，且文案说清为什么。
#[test]
fn ac006_budget_is_required_and_must_be_positive() {
    for bad in [None, Some(0u64)] {
        let err = require_ai_budget(bad).expect_err("缺预算/0 预算必须拒绝");
        let msg = format!("{err:#}");
        assert!(msg.contains("--budget-tokens"), "要点名该给哪个开关：{msg}");
    }
    assert_eq!(require_ai_budget(Some(1)).unwrap(), 1);
    assert_eq!(require_ai_budget(Some(100_000)).unwrap(), 100_000);
}

// ──────────────────────────────────────────────────────────
// AC-008 / AC-009 · 输出协议校验
// ──────────────────────────────────────────────────────────

/// 编造一个 `cites` 坐标 ⇒ 该条 AI 判断作废（**不进 map、不污染分数**）且计数 1。
#[test]
fn ac008_fabricated_cite_voids_the_judgement() {
    let reqs = vec![req(7)];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(
        serde_json::json!({
            "choice": {"table": "llm_model", "column": "model_id"},
            "semantic_fit": 0.9,
            "rationale": "看起来像是对的",
            "cites": [{"table": "nonexistent", "column": "col"}]
        })
        .to_string(),
        17,
    );
    let (j, s) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);
    assert!(j.is_empty(), "编造引用的判断必须整条作废（不是「忽略 cites 照用分数」）");
    assert_eq!(s.rejected_cites, 1);
    assert_eq!(s.judged, 0);
    assert_eq!(cache.len(), 1, "作废的回答会被缓存（为了不重复付费），但它是**作废记录**");
    // ★ 回放这份缓存：仍然不产出判断，且**不再调用**（同输入不重复付费）
    let (j2, s2) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);
    assert!(j2.is_empty(), "回放作废记录绝不产出判断（否则「答歪了」会变成「AI 说不成立」）");
    assert_eq!(s2.calls, 0, "作废记录也算缓存命中 —— 不许为同一份不合规回答反复付费");
    assert_eq!(s2.cache_hits, 1);
    assert_eq!(s2.rejected_cache_hits, 1);
    assert_eq!(s2.rejected_cites, 1, "作废计数照样加回来");
    assert_eq!(t.call_count(), 1);
    assert!(
        s.rejection_samples.iter().any(|m| m.contains("nonexistent.col")),
        "报告里要点名假坐标：{:?}",
        s.rejection_samples
    );
}

/// `rationale` 缺失 / 空 / 超 60 字 ⇒ 三条全作废，且不污染分数。
#[test]
fn ac009_bad_rationale_voids_the_judgement() {
    let too_long = "很".repeat(RATIONALE_MAX_CHARS + 1);
    let cases = [
        serde_json::json!({"choice": null, "semantic_fit": 0.5, "cites": []}),
        serde_json::json!({"choice": null, "semantic_fit": 0.5, "rationale": "   ", "cites": []}),
        serde_json::json!({"choice": null, "semantic_fit": 0.5, "rationale": too_long, "cites": []}),
    ];
    for body in cases {
        let reqs = vec![req(1)];
        let mut cache = MemoryAiCache::new();
        let mut t = transport(body.to_string(), 17);
        let (j, s) = judge_all(
            &mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300,
        );
        assert!(j.is_empty(), "rationale 不合规必须作废：{body}");
        assert_eq!(s.rejected_rationale, 1);
        assert_eq!(s.judged, 0);
    }
    // 60 字**整**是合法的（边界不能写成 ≤59）
    let ok = serde_json::json!({
        "choice": null, "semantic_fit": 0.5,
        "rationale": "字".repeat(RATIONALE_MAX_CHARS), "cites": []
    });
    let reqs = vec![req(1)];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(ok.to_string(), 17);
    let (j, _) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);
    assert_eq!(j.len(), 1, "正好 60 字必须通过");
}

/// 协议的四条校验逐条拒（不只 cites / rationale）：
/// `choice` 不在候选内、`semantic_fit` 越界、非 JSON。
#[test]
fn protocol_rejects_choice_out_of_set_range_and_garbage() {
    let cases: [(String, fn(&AiStats) -> usize); 3] = [
        (
            serde_json::json!({
                "choice": {"table": "llm_model", "column": "model_name"},
                "semantic_fit": 0.9, "rationale": "选了候选之外的列", "cites": []
            })
            .to_string(),
            (|s| s.rejected_choice) as fn(&AiStats) -> usize,
        ),
        (
            serde_json::json!({
                "choice": null, "semantic_fit": 1.5, "rationale": "分数越界", "cites": []
            })
            .to_string(),
            (|s| s.rejected_fit) as fn(&AiStats) -> usize,
        ),
        (
            "这不是 JSON".to_string(),
            (|s| s.rejected_parse) as fn(&AiStats) -> usize,
        ),
    ];
    for (body, pick) in cases {
        let reqs = vec![req(1)];
        let mut cache = MemoryAiCache::new();
        let mut t = transport(body.clone(), 17);
        let (j, s) = judge_all(
            &mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300,
        );
        assert!(j.is_empty(), "必须作废：{body}");
        assert_eq!(pick(&s), 1, "计数要落到对应类别上：{body}");
    }
}

/// `usage` 缺失 ⇒ 判断作废（**不知道花了多少**不能当成 0）。
#[test]
fn missing_usage_voids_the_judgement_instead_of_reporting_zero() {
    let reqs = vec![req(1)];
    let mut cache = MemoryAiCache::new();
    let mut t = ScriptedTransport::always_answer(|_| {
        Ok(ChatResponse {
            choices: vec![ChatChoice {
                message: Message_ {
                    content: Some(good_answer()),
                    reasoning_content: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        })
    });
    let (j, s) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);
    assert!(j.is_empty(), "没有 usage 就无法记账 —— 与 agent::llm 同一条 fail-closed");
    assert_eq!(s.failed_calls, 1);
    assert_eq!(s.tokens_spent, 0, "没报就是没报，不许编一个数");
}

/// 正文为空 / 被截断 ⇒ 走 `agent::llm::completion_text` 的既有判据（不判成功）。
#[test]
fn empty_content_is_not_a_successful_judgement() {
    let reqs = vec![req(1)];
    let mut cache = MemoryAiCache::new();
    let mut t = ScriptedTransport::always_answer(|_| {
        Ok(ChatResponse {
            choices: vec![ChatChoice {
                message: Message_ { content: None, reasoning_content: Some("思考中".into()) },
                finish_reason: Some("length".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 12,
                total_tokens: Some(17),
                completion_tokens_details: None,
            }),
        })
    });
    let (j, s) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);
    assert!(j.is_empty());
    assert_eq!(s.failed_calls, 1);
    assert_eq!(s.tokens_spent, 17, "钱花了就要记（哪怕判断作废）");
}

// ──────────────────────────────────────────────────────────
// AC-004 · 幂等：同输入两次 ⇒ 产物逐字节相同
// ──────────────────────────────────────────────────────────

/// 第一次「调用后判」与第二次「命中缓存后判」，得到的判断**逐字段相同**，
/// 因此贴到条目上的 `confidence`（含 `ai`）序列化后逐字节相同。
///
/// ⚠️ 为什么是这一层而不是「两次 CLI 运行的四个产物」：自动化测试**不许**
/// 打真实端点（AC-011），而 CLI 的 AI 阶段必须真发 HTTP。产物级的两跑
/// 由手工验证覆盖（见 `.feature/verification-report.md` 的 AC-004 一节）。
#[test]
fn ac004_same_input_twice_gives_identical_bytes() {
    let reqs = vec![req_n(1, "model_id"), req_n(2, "created_by")];
    let mut cache = MemoryAiCache::new();
    let mut t = transport(good_answer(), 17);
    let (j1, _) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);
    let (j2, _) = judge_all(&mut t, &mut cache, &reqs, "m", Prompts::standard(), 100_000, false, 300);

    for idx in [1usize, 2] {
        let a = &j1[&idx];
        let b = &j2[&idx];
        assert_eq!(a.semantic_fit, b.semantic_fit);
        assert_eq!(a.rationale, b.rationale);
        assert_eq!(a.cites, b.cites);
        assert_eq!(a.choice, b.choice);
        assert_eq!(a.model, b.model);
        assert_eq!(a.prompt_sha, b.prompt_sha);
        assert_eq!(a.tokens, b.tokens, "缓存命中要回放**当初**的用量，不重复计费");
        assert!(!a.cache_hit && b.cache_hit, "两跑的唯一差别就是 cache_hit");
        // 除 cache_hit 外，序列化后的溯源逐字节相同
        let pa = a.provenance();
        let pb = b.provenance();
        assert_eq!(
            serde_json::to_string(&pa).unwrap().replace("\"cache_hit\":false", ""),
            serde_json::to_string(&pb).unwrap().replace("\"cache_hit\":true", "")
        );
    }
}

// ──────────────────────────────────────────────────────────
// AC-001 / R6 / AC-007 · 评分卡接入
// ──────────────────────────────────────────────────────────

fn score_input(sampled: u32, matched: u32) -> cf::ScoreInput {
    cf::ScoreInput {
        column: "model_id".into(),
        to_table: "llm_model".into(),
        from_type: cf::TypeFamily::Str,
        to_type: Some(cf::TypeFamily::Str),
        target_key: cf::TargetKey::Primary,
        data: Some(cf::SampleFacts {
            sampled,
            matched,
            digest: Some("sha256:aa".into()),
            reason: None,
        }),
        composite_target_key: false,
        polymorphic: false,
        self_ref: false,
    }
}

fn signal<'a>(c: &'a cf::Confidence, name: &str) -> Option<&'a cf::Signal> {
    c.signals.iter().find(|s| s.name == name)
}

/// **AC-001**：AI 关（v1）的产物与 REQ-108 同形 —— 没有 `ai`、没有
/// `semantic_fit`、method 仍是 v1。
///
/// ⚠️ 这里断言的是**形状**；「逐字节相同」的强判据由
/// `tests/test_req108_confidence.rs` 的既有用例 + 「AI 关两次运行 sha256 相同」
/// （手工验证）共同承担 —— REQ-108 的 230 个 lib 用例全绿本身就是
/// 「v1 算术一个字节没动」的证据。
#[test]
fn ac001_ai_off_is_byte_identical_to_scorecard_v1() {
    let c = cf::score(&score_input(5, 5));
    assert_eq!(c.method, cf::METHOD);
    assert!(c.ai.is_none());
    assert!(signal(&c, cf::SIGNAL_SEMANTIC_FIT).is_none());
    let v = serde_json::to_value(&c).unwrap();
    assert!(v.get("ai").is_none(), "AI 关时产物里**不许**出现 ai 键（AC-001 的字节口径）");
    assert!(
        !v.to_string().contains("semantic_fit"),
        "AI 关时不许出现 semantic_fit"
    );

    // 同一输入、v2 但语义缺失 ⇒ 分数与 v1 **逐位相同**（等比压缩的全部意义）
    let c2 = cf::score_with_semantic(&score_input(5, 5), None);
    assert_eq!(c2.score, c.score, "语义缺失时归一化结果必须与 v1 相同");
    assert_eq!(c2.band, c.band);
    assert_eq!(c2.signals.len(), c.signals.len(), "语义缺失 ⇒ 不产生第 6 个信号");
}

/// **R6**：语义缺失 ≠ 0。
///
/// 判据不是「分数一样」而是**信号条数**：`None` 不进 `signals`；
/// `Some(0.0)` 进 `signals` 且把分数拉下来。两者必须可区分。
#[test]
fn r6_missing_semantic_is_absent_not_zero() {
    let absent = cf::score_with_semantic(&score_input(5, 5), None);
    let zero = cf::score_with_semantic(&score_input(5, 5), Some(0.0));
    assert_eq!(absent.signals.len(), 5, "缺失 ⇒ 5 个信号（不参与归一化）");
    assert!(signal(&absent, cf::SIGNAL_SEMANTIC_FIT).is_none());
    assert_eq!(zero.signals.len(), 6, "0 分 ⇒ 6 个信号（明确否定，参与归一化）");
    assert_eq!(signal(&zero, cf::SIGNAL_SEMANTIC_FIT).unwrap().value, 0.0);
    assert!(
        zero.score < absent.score,
        "0 分必须真的把分数拉下来（{} vs {}）",
        zero.score,
        absent.score
    );
    // 缺失时方法仍是 v2（「这一轮开了 AI」是运行事实），但权重归一化等价于 v1
    assert_eq!(absent.method, cf::METHOD_V2);
}

/// v2 权重表就是设计概要 §3.1 那张，且合计 1.0。
#[test]
fn v2_weights_match_the_design_table() {
    let sum: f64 = cf::WEIGHTS_V2.iter().map(|(_, w)| w).sum();
    assert!((sum - 1.0).abs() < 1e-12, "v2 权重合计必须为 1.0，实际 {sum}");
    assert_eq!(
        cf::WEIGHTS_V2,
        [
            ("sample_hit_rate", 0.3825),
            ("name_match", 0.17),
            ("not_all_null", 0.1275),
            ("type_compatible", 0.085),
            ("target_is_key", 0.085),
            (cf::SIGNAL_SEMANTIC_FIT, 0.15),
        ]
    );
    // 等比压缩：v2 的前五项 = v1 × 0.85（逐项）
    for (i, (name, w)) in cf::WEIGHTS.iter().enumerate() {
        assert_eq!(cf::WEIGHTS_V2[i].0, *name, "前五项顺序必须与 v1 一致");
        assert!((cf::WEIGHTS_V2[i].1 - w * 0.85).abs() < 1e-12, "{name} 不是等比压缩");
    }
}

/// **AC-007**：语义分再高也翻不了 veto。
#[test]
fn ac007_semantic_fit_cannot_flip_any_veto() {
    // ① 0 命中 + AI 1.0 ⇒ 仍 ≤ 0.20 且落拒绝
    let zero_hit = cf::score_with_semantic(&score_input(5, 0), Some(1.0));
    assert!(zero_hit.score <= 0.20, "0 命中必须 ≤ 0.20，实际 {}", zero_hit.score);
    assert_eq!(zero_hit.veto.as_ref().unwrap().signal, "zero_hit");
    assert_eq!(zero_hit.band, cf::Band::Rejected);

    // ② 类型不相容 + AI 1.0 ⇒ 仍 ≤ 0.15
    let mut bad_type = score_input(5, 5);
    bad_type.to_type = Some(cf::TypeFamily::Num);
    let t = cf::score_with_semantic(&bad_type, Some(1.0));
    assert!(t.score <= 0.15, "类型不相容必须 ≤ 0.15，实际 {}", t.score);
    assert_eq!(t.veto.as_ref().unwrap().signal, "type_incompatible");
    assert_eq!(t.band, cf::Band::Rejected);

    // ③ 全空 + AI 1.0 ⇒ 仍 ≤ 0.45
    let null = cf::score_with_semantic(&score_input(0, 0), Some(1.0));
    assert!(null.score <= 0.45, "全空必须 ≤ 0.45，实际 {}", null.score);

    // ④ 无采样 + AI 1.0 ⇒ 仍 ≤ 0.45
    let mut no_data = score_input(5, 5);
    no_data.data = None;
    let nd = cf::score_with_semantic(&no_data, Some(1.0));
    assert!(nd.score <= 0.45, "无数据证据必须 ≤ 0.45，实际 {}", nd.score);
}

/// 溯源与语义信号必须**成对**出现（`from_untrusted` 的入口校验）。
#[test]
fn ai_provenance_and_semantic_signal_must_come_in_pairs() {
    let with_ai_and_signal = serde_json::json!({
        "score": 0.70, "method": cf::METHOD_V2,
        "signals": [
            {"name": "sample_hit_rate", "value": 1.0, "weight": 0.3825, "sampled": 5, "matched": 5},
            {"name": "semantic_fit", "value": 0.9, "weight": 0.15}
        ],
        "data_evidence": "sampled",
        "ai": {"model": "m", "prompt_sha": "p", "tokens": 17, "cache_hit": false}
    });
    let c = cf::from_untrusted(&with_ai_and_signal).expect("成对出现必须放行");
    assert_eq!(c.method, cf::METHOD_V2);
    assert_eq!(c.ai.as_ref().unwrap().tokens, 17);

    // 只有信号没有溯源
    let mut no_ai = with_ai_and_signal.clone();
    no_ai.as_object_mut().unwrap().remove("ai");
    let err = cf::from_untrusted(&no_ai).unwrap_err();
    assert!(err.contains("ai"), "要点名缺的是溯源：{err}");

    // 只有溯源没有信号
    let mut no_signal = with_ai_and_signal.clone();
    no_signal["signals"] = serde_json::json!([
        {"name": "sample_hit_rate", "value": 1.0, "weight": 0.3825, "sampled": 5, "matched": 5}
    ]);
    let err = cf::from_untrusted(&no_signal).unwrap_err();
    assert!(err.contains("semantic_fit"), "{err}");

    // v1 的权重表里没有语义项 ⇒ 拿 v1 的 method 夹带语义信号必须被拒
    let mut wrong_method = with_ai_and_signal.clone();
    wrong_method["method"] = serde_json::json!(cf::METHOD);
    let err = cf::from_untrusted(&wrong_method).unwrap_err();
    assert!(err.contains("权重表"), "v1 不接受 semantic_fit：{err}");
}

// ──────────────────────────────────────────────────────────
// AC-010 · 客户视图零泄漏 / 建模轨必须有
// ──────────────────────────────────────────────────────────

#[test]
fn ac010_customer_view_has_zero_ai_leakage() {
    use ontok::domain::snapshot::{Evidence, LinkType, Snapshot};

    // 一条**被 AI 判断过**的关系：confidence 里带 semantic_fit 信号 + ai 溯源
    let c = cf::from_untrusted(&serde_json::json!({
        "score": 0.70, "method": cf::METHOD_V2,
        "signals": [
            {"name": "sample_hit_rate", "value": 1.0, "weight": 0.3825, "sampled": 5, "matched": 5},
            {"name": "semantic_fit", "value": 0.9, "weight": 0.15}
        ],
        "data_evidence": "sampled",
        "ai": {"model": "qwen36-27B", "prompt_sha": "abc", "tokens": 17, "cache_hit": true,
               "rationale": "业务键存在", "cites": ["llm_model.model_id"]}
    }))
    .expect("这份 confidence 合法");

    let mut snap = Snapshot::new(uuid::Uuid::nil(), 1);
    snap.link_types.push(LinkType {
        key: "llm_model_sort_has_llm_model".into(),
        from_type: "llm_model_sort".into(),
        to_type: "llm_model".into(),
        cardinality: "1:N".into(),
        semantic: "belongs_to".into(),
        evidence: vec![Evidence {
            table: "llm_model_sort".into(),
            column: "model_id".into(),
            claim: "列名推断".into(),
        }],
        confidence: Some(c),
    });

    // ① 客户视图：结构上就读不到 confidence（SnapshotView 里没有这一栏）
    let customer = ontok::domain::view::project(&ontok::cli::snapshot_to_view(&snap));
    let json = serde_json::to_string(&customer).unwrap();
    for banned in ["semantic_fit", "cache_hit", "\"ai\"", "prompt_sha", "qwen36-27B"] {
        assert!(!json.contains(banned), "客户视图泄漏了 {banned}：{json}");
    }

    // ② 建模轨：必须有（AC-010 的另一半）
    let model = ontok::domain::model_view::project_model_track(&snap);
    let mj = serde_json::to_string(&model).unwrap();
    for needed in ["semantic_fit", "\"ai\"", "cache_hit", "prompt_sha", "qwen36-27B"] {
        assert!(mj.contains(needed), "建模轨缺 {needed}：{mj}");
    }

    // ③ 审核界面（`review show --confidence`）也必须看得见
    let row = ontok::db::proposal::ProposalItemRow {
        id: uuid::Uuid::nil(),
        proposal_id: uuid::Uuid::nil(),
        seq: 1,
        kind: "add_link_type".into(),
        target_key: "模型排序→大模型".into(),
        payload: serde_json::json!({ "confidence": snap.link_types[0].confidence }),
        biz_status: "pending".into(),
        model_status: "pending".into(),
    };
    let text = ontok::cli::render_confidence_table(&[row]);
    for needed in ["ai qwen36-27B", "fit 0.90", "cache_hit true", "tokens 17", "业务键存在"] {
        assert!(text.contains(needed), "审核界面缺 {needed}：{text}");
    }
    assert!(text.contains("evidence-scorecard-v2"), "抬头要写实际方法：{text}");
}

// ──────────────────────────────────────────────────────────
// 接缝：propose::ai_requests / score_links_with_ai
// ──────────────────────────────────────────────────────────

fn report() -> SchemaReport {
    let table = |name: &str, cols: Vec<(&str, &str, Option<&str>)>| TableMeta {
        name: name.into(),
        comment: None,
        column_count: cols.len(),
        commented_count: cols.iter().filter(|c| c.2.is_some()).count(),
        columns: cols
            .into_iter()
            .map(|(n, t, c)| ColumnMeta {
                name: n.into(),
                data_type: t.into(),
                nullable: false,
                comment: c.map(str::to_string),
                is_primary_key: n == "id",
            })
            .collect(),
    };
    SchemaReport {
        server_version: "16.14".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: 2,
        column_count: 5,
        commented_column_count: 5,
        foreign_key_count: 0,
        tables: vec![
            table(
                "llm_model",
                vec![
                    ("id", "character varying", Some("主键UUID")),
                    ("model_id", "character varying", Some("模型ID")),
                ],
            ),
            table(
                "llm_model_sort",
                vec![
                    ("id", "character varying", Some("主键UUID")),
                    ("model_id", "character varying", Some("模型ID")),
                    ("region", "character varying", Some("区域")),
                ],
            ),
        ],
        introspected_at: "2026-10-21T00:00:00Z".into(),
    }
}

fn link_items() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "kind": "add_link_type",
            "target": "llm_model_sort_has_llm_model",
            "payload": {
                "from_type": "llm_model_sort",
                "to_type": "llm_model",
                "evidence": [{"table": "llm_model_sort", "column": "model_id", "claim": "列名推断"}]
            }
        }),
        serde_json::json!({
            "kind": "add_entity_type",
            "target": "llm_model",
            "payload": {"display_name": "大模型"}
        }),
    ]
}

fn keys() -> KeyCatalog {
    let mut k = KeyCatalog::new();
    k.insert(
        "llm_model",
        TableKeys {
            primary_key: Some("id".into()),
            unique_columns: vec!["id".into()],
            composite_primary_key: vec![],
        },
    );
    k.insert(
        "llm_model_sort",
        TableKeys {
            primary_key: Some("id".into()),
            unique_columns: vec!["id".into()],
            composite_primary_key: vec![],
        },
    );
    k
}

/// `ai_requests` 只给两张表、候选列把提案列排在前、且 `--llm-only` 能过滤。
#[test]
fn ai_requests_slice_has_only_the_two_tables_and_candidates() {
    let items = link_items();
    let cands = ontok::extract::propose::link_candidates(&items, &keys());
    assert_eq!(cands.len(), 1, "只有关系条目参与");

    let reqs = ontok::extract::propose::ai_requests(&cands, &report(), None);
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.relation_key(), "llm_model_sort.model_id -> llm_model.id");
    assert_eq!(r.schema_slice.len(), 2, "⛔ 不许塞整份 schema：只有源表 + 目标表");
    assert_eq!(r.to_candidates[0], "id", "提案选中的列排在候选第一位");
    assert!(r.to_candidates.contains(&"model_id".to_string()));

    // 过滤闸：不匹配 ⇒ 不送 AI（语义因此缺失，不是 0）
    assert!(ontok::extract::propose::ai_requests(&cands, &report(), Some("orders")).is_empty());
    assert_eq!(
        ontok::extract::propose::ai_requests(&cands, &report(), Some("llm_model")).len(),
        1
    );
}

/// 接缝：AI 的 `semantic_fit` 进评分卡（v2），且溯源贴上条目。
#[test]
fn score_links_with_ai_attaches_semantic_and_provenance() {
    let items = link_items();
    let evidence = HashMap::new();
    let base = ontok::extract::propose::score_links(&items, &report(), &keys(), &evidence);

    // AI 关：v1、无 ai
    assert_eq!(base.len(), 1);
    assert_eq!(base[0].1.method, cf::METHOD);
    assert!(base[0].1.ai.is_none());

    // AI 开但没有这条的判断 ⇒ v2、语义缺失、**不记 0**
    let empty: HashMap<usize, Judgement> = HashMap::new();
    let no_sem = ontok::extract::propose::score_links_with_ai(
        &items, &report(), &keys(), &evidence, Some(&empty),
    );
    assert_eq!(no_sem[0].1.method, cf::METHOD_V2);
    assert!(signal(&no_sem[0].1, cf::SIGNAL_SEMANTIC_FIT).is_none());
    assert!(no_sem[0].1.ai.is_none());

    // AI 判了 0.9 ⇒ 信号在、溯源在
    let mut m: HashMap<usize, Judgement> = HashMap::new();
    let idx = base[0].0;
    m.insert(
        idx,
        Judgement {
            semantic_fit: 0.9,
            rationale: "业务键存在".into(),
            cites: vec![Cite { table: "llm_model".into(), column: "model_id".into() }],
            choice: None,
            model: "qwen36-27B".into(),
            prompt_sha: "abc".into(),
            tokens: 17,
            cache_hit: false,
        },
    );
    let scored =
        ontok::extract::propose::score_links_with_ai(&items, &report(), &keys(), &evidence, Some(&m));
    let c = &scored[0].1;
    assert_eq!(signal(c, cf::SIGNAL_SEMANTIC_FIT).unwrap().value, 0.9);
    let ai = c.ai.as_ref().expect("有判断就必须有溯源");
    assert_eq!(ai.model, "qwen36-27B");
    assert_eq!(ai.tokens, 17);
    assert!(!ai.cache_hit);
    // 语义分只通过 0.15 的权重起作用，veto 段没被碰：
    // 这里没有采样证据 ⇒ 触发的仍是 REQ-108 那条 `no_data_evidence`（≤0.45），
    // 与语义分无关 —— AI 既没增加 veto，也没解除它。
    assert_eq!(
        c.veto.as_ref().map(|v| v.signal.as_str()),
        Some("no_data_evidence")
    );
    assert!(c.score <= 0.45);
}

/// 无 key 时 `LlmSettings::from_env` 必须**点名缺哪个变量**（AC-002 的判据层）。
///
/// ⚠️ 本用例动的是**进程环境变量**，所以只在单线程语义下安全 ——
/// 它只读不写（三个变量在测试进程里本来就是空的，CI 上也不该有），
/// 若某个环境里恰好配了真 key，本用例会跳过而不是误报。
#[test]
fn ac002_missing_env_names_the_variable() {
    let any_set = [
        BASE_URL_ENV,
        ontok::agent::llm::API_KEY_ENV,
        ontok::agent::llm::MODEL_ENV,
    ]
    .iter()
    .any(|k| std::env::var(k).map(|v| !v.trim().is_empty()).unwrap_or(false));
    if any_set {
        eprintln!("SKIP ac002_missing_env_names_the_variable：本机配了 ONTOK_LLM_*，改由 CLI 用例覆盖");
        return;
    }
    let err = LlmSettings::from_env().expect_err("没有凭据时必须报错，不许静默降级");
    let msg = format!("{err:#}");
    for k in [
        BASE_URL_ENV,
        ontok::agent::llm::API_KEY_ENV,
        ontok::agent::llm::MODEL_ENV,
    ] {
        assert!(msg.contains(k), "错误信息必须点名 {k}：{msg}");
    }
}
