//! **`ontok exclude suggest`**（REQ-118 · 决策点二）—— 产品内置的「该不该进本体」提案。
//!
//! # 它替代的是哪一段对话
//!
//! REQ-113 让产品能**消费**一份排除文件（`--exclude`），但**判断本身**发生在对话里
//! （那份 `exclusions-ai.json` 的 `decided_by` 写着 `ai-table-judge`，是个对话里的 agent）。
//! 本模块让产品自己逐表判一遍，并产出**两边都能用**的一份文件：
//!
//! ```text
//!                     ┌─ proposals[]  ADR-005 的统一提案形状（claim/evidence/confidence/reason/decided_by/reviewed_by）
//! exclude suggest ────┤
//!                     └─ exclude[]    REQ-113 的 `Exclusions` 形状（table/category/reason/decided_by/confidence/evidence）
//!                                        ↑ `ontok-propose --exclude <这份文件>` 直接消费（AC-003）
//! ```
//!
//! # 为什么一份文件同时是两种形状
//!
//! AC-001 要「每条提案都有 evidence / reason / confidence / reviewed_by」，
//! AC-003 要「这份文件能被 `--exclude` 直接消费」。两个要求指向**同一批判断**，
//! 拆成两份文件就要回答「哪份是权威」—— 而两份长得不一样的文件必然漂移。
//! 一份文件里放两个数组，`reviewed_by: null` 只出现一次（P2），
//! `ontok-propose` 只读它认识的 `exclude[]`，评审界面读 `proposals[]`。
//!
//! ⚠️ `exclude[]` 里每条的 `evidence` 是**字符串**（REQ-113 的字段类型），
//! 而 `proposals[]` 里是**数组**（ADR-005）。转换是 `join("；")` ——
//! 信息不丢，只是换了容器。两条都来自同一次校验，⛔ 不是两套判断。
//!
//! # 「保留」也是判断，也要进 `proposals[]`
//!
//! 只有「排除」是结论，「保留」不是默认值：模型必须逐表显式给 `exclude: true|false`。
//! `--exclude` 只消费 true 的那些；`proposals[]` 全留 —— 因为「为什么这张表**应该**
//! 进本体」同样是评审要看的（R4：拿不准的保留要标 `low_confidence`，
//! 并同时进 `keep_but_flagged[]` 让审核界面优先显示）。
//!
//! # 七类枚举不在这里定义
//!
//! `category` 的合法值**只有一处**：[`crate::extract::exclusions::Category`]。
//! 本模块只负责把模型给的字符串交给 `Category::parse`，不合法就丢弃并计数 ——
//! ⛔ 不发明第八类、不做同义词映射。
//!
//! # 增量：`--exclusions <file>`
//!
//! 已有一份人签过的排除清单时，其中**仍在 schema 里**的表不再送模型（省钱、也不覆盖
//! 人的决定），原样带进产物的 `exclude[]`；schema 里已消失的条目进 `stale` 清单
//! （⚠️ 只报，不删 —— 与 REQ-090 的 `absent_from_schema` 同一条纪律）。

use crate::agent::llm::LlmTransport;
use crate::extract::ai_semantic::Prompts;
use crate::extract::exclusions::{Category, Entry, Exclusions, REASON_MIN};
use crate::extract::introspect::SchemaReport;
use crate::extract::name_suggest::TableSlice;
use crate::extract::proposal::{
    decided_by_ai, evidence_references_input, finalize_item, parse_proposals, render_document,
    run_batches, sha256_text, BatchRequest, Drop, ProposalCache,
    ProposalHeader, ProposalItem, ProposalStats, SOURCE_AI,
};

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 提案逻辑版本 —— **进指纹**。
pub const AGENT_VERSION: &str = "exclude-suggest-1";

/// 每次请求送几张表。10 比命名小：每条要回 category / exclude / 理由 / 依据 / 置信度。
pub const DEFAULT_BATCH_SIZE: usize = 10;

/// 这份排除清单**自己的**版本号（进 `exclude[].version` 与溯源）。
pub const EXCLUSION_FILE_VERSION: &str = "ai-proposal-1";

/// 排除清单的一句话规则（进 `exclude.rule`，评审时最先看的就是它）。
pub const RULE: &str = "逐表判断「这张表是不是客户业务世界里的事物」：只排除同步残留 / 迁移簿记 / 备份副本 / 内部管道 / 报表中间表 / 已废弃 / 超出建模范围这七类；拿不准一律保留并标低置信，⛔ 不为了「干净」而排除。";

/// 固定提示词 —— **改动会改变 `prompt_sha256`，从而让所有缓存失效**（P3）。
pub const SYSTEM_PROMPT: &str = r#"你是数据库本体建模的作用域判定器。你的唯一任务：逐表判断「这张表该不该进入客户业务本体」，并给出可核实的依据。

输入是一个 JSON：{"tables": [{"table": "表名", "table_comment": "表注释或空串", "columns": [{"name": "列名", "type": "类型", "pk": true/false, "comment": "列注释或 null"}]}], "task": "..."}

输出必须是严格 JSON（不要 markdown 代码块、不要任何解释文字）：
{"proposals": [{"subject": "表名", "exclude": true 或 false, "category": "排除时必填", "claim": "一句话判断", "evidence": ["可核实的依据"], "confidence": 0.0到1.0, "reason": "一句话理由"}]}

判定规则：
1. subject 必须是输入里的表名，**逐字一致**；输入里的**每一张表**都要给出一条判断。
2. exclude = true 表示「这张表不该进本体」。此时 category 必须**恰好**是下面七个值之一：
   cdc_residue（CDC / 数据同步残留）
   migration_bookkeeping（迁移框架簿记表）
   backup_snapshot（备份 / 快照副本）
   internal_pipeline（内部管道：outbox / 缓存 / 触发器状态）
   report_intermediate（报表中间结果表）
   deprecated（已废弃）
   out_of_scope（超出本轮建模范围）
   ⛔ 不许发明第八类；不许把「业务表但暂时用不上」判成任何一类。
3. exclude = false 表示「这是客户业务对象，应进本体」。此时省略 category。
   ⛔ 业务表不是「排除」的反面默认值：只有你**判断过**它是业务对象，才写 false。
4. 拿不准时一律 exclude = false（保留），confidence 给 0.6 以下，并在 reason 里说清拿不准什么。
   ⛔ 宁可多保留，也不要为了「干净」排除一张真业务表。
5. evidence 给 1–3 条，**每一条都必须是输入里能查到的原文**：表名 / 列名 / 表注释或列注释的片段。
   ⛔ 不许写「模型认为」「推测」「可能是」这类不可核实的话。
6. claim 是一句话判断，例如「这张表是数据同步工具生成的临时 DDL，不属于客户业务」。
7. reason 必须让**业务人员**读懂（≥ 6 个字）：说清这张表是什么、为什么不该进（或该进）。
"#;

/// 每条请求里固定的 task 文案（进 user 消息，不进模板哈希）。
const TASK_TEXT: &str = "逐表判断这张表该不该进入客户业务本体；排除项必须给七类枚举之一。按协议返回严格 JSON。";

// ──────────────────────────────────────────────────────────
// 批与请求
// ──────────────────────────────────────────────────────────

/// 一批表 + 它对应的请求。
#[derive(Debug, Clone)]
pub struct ExcludeBatch {
    pub slices: Vec<TableSlice>,
    pub request: BatchRequest,
}

/// 规划：**已被人排除的表不再送模型**（增量），其余按表名排序切批。
///
/// 返回 `(要判定的批, 沿用的条目, 陈旧条目)`。
pub fn plan_batches(
    report: &SchemaReport,
    existing: Option<&Exclusions>,
    batch_size: usize,
) -> anyhow::Result<(Vec<ExcludeBatch>, Vec<Entry>, Vec<Entry>)> {
    if batch_size == 0 {
        anyhow::bail!("--batch-size 必须是正整数（0 意味着一个请求都不发）");
    }
    let known: std::collections::BTreeSet<&str> =
        report.tables.iter().map(|t| t.name.as_str()).collect();
    let mut carried: Vec<Entry> = Vec::new();
    let mut stale: Vec<Entry> = Vec::new();
    let mut skip: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    if let Some(ex) = existing {
        for e in ex.entries() {
            if known.contains(e.table.as_str()) {
                skip.insert(e.table.clone());
                carried.push(e.clone());
            } else {
                stale.push(e.clone());
            }
        }
    }
    carried.sort_by(|a, b| a.table.cmp(&b.table));
    stale.sort_by(|a, b| a.table.cmp(&b.table));

    let mut tables: Vec<&crate::extract::introspect::TableMeta> = report
        .tables
        .iter()
        .filter(|t| !skip.contains(t.name.as_str()))
        .collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = Vec::new();
    for chunk in tables.chunks(batch_size) {
        let slices: Vec<TableSlice> = chunk.iter().map(|t| TableSlice::from_meta(t)).collect();
        let body = serde_json::json!({
            "tables": slices.iter().map(TableSlice::to_json).collect::<Vec<_>>(),
            "task": TASK_TEXT,
        });
        let user_message = serde_json::to_string(&body)?;
        let input_digest = sha256_text(&user_message);
        let key = format!(
            "{}..{}",
            slices.first().map(|s| s.table.as_str()).unwrap_or("-"),
            slices.last().map(|s| s.table.as_str()).unwrap_or("-")
        );
        out.push(ExcludeBatch {
            request: BatchRequest {
                key,
                item_count: slices.len(),
                input_digest,
                user_message,
            },
            slices,
        });
    }
    Ok((out, carried, stale))
}

// ──────────────────────────────────────────────────────────
// 解析 + 校验
// ──────────────────────────────────────────────────────────

/// 一条通过的「该不该进本体」判断。
#[derive(Debug, Clone, PartialEq)]
pub struct ExcludeDecision {
    pub table: String,
    pub excluded: bool,
    pub category: Option<Category>,
    pub item: ProposalItem,
}

/// 校验一批回答：丢弃 + 计数，绝不修正模型输出。
///
/// 本命令多判四件事：
/// 1. `subject` 必须在这一批里；
/// 2. `exclude` 必须**显式**给出（缺省不是「保留」）；
/// 3. `exclude: true` ⇒ `category` 必须是七类之一，`reason` 长度 ≥ [`REASON_MIN`]；
/// 4. 至少一条 evidence 能在这张表的输入里找到落点。
pub fn validate_batch(
    batch: &ExcludeBatch,
    body: &str,
    decided_by: &str,
    stats: &mut ProposalStats,
) -> std::result::Result<Vec<ExcludeDecision>, String> {
    let parsed = parse_proposals(body)?;
    for m in &parsed.malformed {
        stats.reject(Drop::ResponseParse, m.clone());
    }
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for raw in parsed.proposals {
        let subject = raw.subject.trim().to_string();
        let Some(slice) = batch.slices.iter().find(|s| s.table == subject) else {
            stats.reject(
                Drop::UnknownSubject,
                format!("{subject:?}（不在本批 {} 张表里）", batch.slices.len()),
            );
            continue;
        };
        if !seen.insert(subject.clone()) {
            stats.reject(Drop::DuplicateSubject, subject);
            continue;
        }
        let excluded = match raw.exclude {
            Some(v) => v,
            None => {
                stats.reject(Drop::MissingDecision, subject);
                continue;
            }
        };
        let item = match finalize_item(&raw, decided_by) {
            Ok(i) => i,
            Err(d) => {
                stats.reject(d, subject);
                continue;
            }
        };
        if !evidence_references_input(&item.evidence, &slice.tokens()) {
            stats.reject(
                Drop::EvidenceNotFromInput,
                format!("{}：{:?}", slice.table, item.evidence),
            );
            continue;
        }
        let mut category = None;
        if excluded {
            let Some(raw_cat) = raw.category.as_deref().map(str::trim).filter(|s| !s.is_empty())
            else {
                stats.reject(Drop::MissingCategory, subject);
                continue;
            };
            let Some(cat) = Category::parse(raw_cat) else {
                stats.reject(
                    Drop::BadCategory,
                    format!(
                        "{subject}：{raw_cat:?} 不在七类里（{}）",
                        Category::ALL
                            .iter()
                            .map(|c| c.as_str())
                            .collect::<Vec<_>>()
                            .join(" / ")
                    ),
                );
                continue;
            };
            if item.reason.chars().count() < REASON_MIN {
                stats.reject(
                    Drop::ShortReason,
                    format!("{subject}：{} 个字", item.reason.chars().count()),
                );
                continue;
            }
            category = Some(cat);
        }
        out.push(ExcludeDecision {
            table: slice.table.clone(),
            excluded,
            category,
            item,
        });
    }
    Ok(out)
}

/// 跑全部批次（三命令共用 [`run_batches`]）。
pub fn suggest_all(
    transport: &mut dyn LlmTransport,
    cache: &mut dyn ProposalCache,
    batches: &[ExcludeBatch],
    model: &str,
    prompts: &Prompts,
    budget: u64,
    max_tokens: u32,
) -> (Vec<ExcludeDecision>, ProposalStats) {
    let decided_by = decided_by_ai(model);
    let reqs: Vec<BatchRequest> = batches.iter().map(|b| b.request.clone()).collect();
    let (mut decisions, mut stats) = run_batches(
        transport,
        cache,
        &reqs,
        model,
        prompts,
        AGENT_VERSION,
        budget,
        max_tokens,
        |i, body, stats| validate_batch(&batches[i], body, &decided_by, stats),
    );
    decisions.sort_by(|a, b| a.table.cmp(&b.table));
    stats.proposed = decisions.len();
    stats.low_confidence = decisions
        .iter()
        .filter(|d| d.item.is_low_confidence())
        .count();
    (decisions, stats)
}

// ──────────────────────────────────────────────────────────
// 渲染
// ──────────────────────────────────────────────────────────

/// 提案文件头（`decided_by` = `ai:<model>`；`reviewed_by` 由 `render_document` 恒写 null）。
pub fn header(model: &str, prompt_sha: &str, input_sha256: String) -> ProposalHeader {
    ProposalHeader {
        kind: "exclude",
        source: SOURCE_AI,
        model: Some(model.to_string()),
        prompt_sha: Some(prompt_sha.to_string()),
        agent_version: AGENT_VERSION.to_string(),
        input_sha256,
        decided_by: decided_by_ai(model),
    }
}

/// `exclude[]` 的一条（REQ-113 形状）。`evidence` 是**字符串**（该结构的字段类型）。
fn entry_json(
    table: &str,
    category: Category,
    reason: &str,
    decided_by: &str,
    confidence: f64,
    evidence: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "table": table,
        "category": category.as_str(),
        "reason": reason,
        "decided_by": decided_by,
        "confidence": confidence,
        "evidence": evidence.join("；"),
    })
}

/// 沿用的（人已签过的）条目原样进 `exclude[]` —— ⛔ 不改写 `decided_by` / `reason`。
fn carried_entry_json(e: &Entry) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("table".into(), serde_json::Value::from(e.table.clone()));
    m.insert("category".into(), serde_json::Value::from(e.category.as_str()));
    m.insert("reason".into(), serde_json::Value::from(e.reason.clone()));
    m.insert("decided_by".into(), serde_json::Value::from(e.decided_by.clone()));
    if let Some(c) = e.confidence {
        m.insert("confidence".into(), serde_json::json!(c));
    }
    if let Some(ev) = &e.evidence {
        m.insert("evidence".into(), serde_json::Value::from(ev.clone()));
    }
    serde_json::Value::Object(m)
}

/// 提案 JSON：**一份文件、两种形状**（`proposals[]` + `exclude[]`）。
///
/// 落盘前会**真的**把 `exclude[]` 交给 [`Exclusions::parse`] 复核（与 `--exclude`
/// 消费时同一套判据）—— 过不了就不写（AC-003 的 fail-closed 版本）。
pub fn render_proposal(
    decisions: &[ExcludeDecision],
    carried: &[Entry],
    stale: &[Entry],
    header: &ProposalHeader,
    stats: &ProposalStats,
) -> anyhow::Result<String> {
    let items: Vec<ProposalItem> = decisions.iter().map(|d| d.item.clone()).collect();
    let mut excluded_json: Vec<serde_json::Value> =
        carried.iter().map(carried_entry_json).collect();
    let mut kept_flagged: Vec<serde_json::Value> = Vec::new();
    for d in decisions.iter().filter(|d| d.excluded) {
        let cat = d.category.expect("校验阶段已保证 exclude: true ⇒ category 有值");
        excluded_json.push(entry_json(
            &d.table,
            cat,
            &d.item.reason,
            &d.item.decided_by,
            d.item.confidence,
            &d.item.evidence,
        ));
    }
    for d in decisions.iter().filter(|d| !d.excluded && d.item.is_low_confidence()) {
        // 「保留但存疑」：表**照常进本体**，但评审人应当看到疑虑（REQ-113 的 Flagged）。
        kept_flagged.push(serde_json::json!({
            "table": d.table,
            "why": d.item.reason,
            "concern": d.item.evidence.join("；"),
        }));
    }
    excluded_json.sort_by(|a, b| {
        a.get("table")
            .and_then(|v| v.as_str())
            .cmp(&b.get("table").and_then(|v| v.as_str()))
    });
    kept_flagged.sort_by(|a, b| {
        a.get("table")
            .and_then(|v| v.as_str())
            .cmp(&b.get("table").and_then(|v| v.as_str()))
    });

    let excluded_count = decisions.iter().filter(|d| d.excluded).count();
    let kept_count = decisions.len() - excluded_count;
    let mut extra = serde_json::Map::new();
    extra.insert(
        "version".to_string(),
        serde_json::Value::from(EXCLUSION_FILE_VERSION),
    );
    extra.insert("rule".to_string(), serde_json::Value::from(RULE));
    // REQ-113 的 `declared_by`：谁声明了这份清单（AI 先判时就是判它的那个东西）。
    extra.insert(
        "declared_by".to_string(),
        serde_json::Value::from(header.decided_by.clone()),
    );
    extra.insert(
        "exclude".to_string(),
        serde_json::Value::Array(excluded_json.clone()),
    );
    extra.insert(
        "keep_but_flagged".to_string(),
        serde_json::Value::Array(kept_flagged),
    );
    extra.insert(
        "counts".to_string(),
        serde_json::json!({
            "judged": decisions.len(),
            "excluded": excluded_count,
            "kept": kept_count,
            "carried_over": carried.len(),
            "stale": stale.len(),
        }),
    );
    extra.insert(
        "stale_excluded_tables".to_string(),
        serde_json::Value::Array(
            stale
                .iter()
                .map(|e| serde_json::Value::from(e.table.clone()))
                .collect(),
        ),
    );
    extra.insert(
        "consumable_by_exclude".to_string(),
        serde_json::Value::from(!excluded_json.is_empty()),
    );
    let text = render_document(header, &items, stats, extra)?;

    // AC-003 的 fail-closed：**真的**用 REQ-113 的解析器复核一遍再落盘。
    // 空 `exclude` 是「这次没有排除项」，不是坏文件 —— 但它**没法**喂给 `--exclude`
    // （REQ-113 明确拒绝空数组：「一份不排除任何表的文件等于没做出决定」），
    // 所以这里只复核非空的情形，并在 `consumable_by_exclude` 里标 false。
    if !excluded_json.is_empty() {
        Exclusions::parse(text.as_bytes(), "<exclude suggest 产物>").map_err(|e| {
            anyhow::anyhow!(
                "内部错误：exclude suggest 的产物没通过 REQ-113 的 Exclusions 校验 —— \
                 **未写出**（AC-003 要求这份文件能被 --exclude 直接消费）。\n{e:#}"
            )
        })?;
    }
    Ok(text)
}

/// 便于测试/报告：被判排除的表名（不含沿用的）。
pub fn excluded_tables(decisions: &[ExcludeDecision]) -> Vec<String> {
    decisions
        .iter()
        .filter(|d| d.excluded)
        .map(|d| d.table.clone())
        .collect()
}

/// 便于测试/报告：被判保留的表名。
pub fn kept_tables(decisions: &[ExcludeDecision]) -> Vec<String> {
    decisions
        .iter()
        .filter(|d| !d.excluded)
        .map(|d| d.table.clone())
        .collect()
}

/// 一份 `Exclusions` 的 sha256（进 `--as-*` 溯源；这里给 CLI 用）。
pub fn existing_digest(ex: &Exclusions) -> String {
    ex.sha256().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::protocol::{Choice, Usage};
    use crate::agent::llm::{ChatResponse, ScriptedTransport};
    use crate::extract::proposal::MemoryProposalCache;
    use crate::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};

    fn col(name: &str, comment: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            data_type: "text".to_string(),
            nullable: true,
            comment: comment.map(str::to_string),
            is_primary_key: false,
        }
    }

    fn table(name: &str, comment: Option<&str>, cols: Vec<ColumnMeta>) -> TableMeta {
        TableMeta {
            name: name.to_string(),
            comment: comment.map(str::to_string),
            column_count: cols.len(),
            commented_count: cols.iter().filter(|c| c.comment.is_some()).count(),
            columns: cols,
        }
    }

    fn report() -> SchemaReport {
        SchemaReport {
            server_version: "PostgreSQL test".into(),
            transaction_read_only: "on".into(),
            schema: "public".into(),
            table_count: 3,
            column_count: 3,
            commented_column_count: 2,
            foreign_key_count: 0,
            introspected_at: "2026-10-22T00:00:00Z".into(),
            tables: vec![
                table("customers", Some("客户主档"), vec![col("id", Some("客户标识"))]),
                table(
                    "alembic_version",
                    None,
                    vec![col("version_num", Some("迁移版本号"))],
                ),
                table(
                    "_volc_dts_pg_ddl_abc",
                    None,
                    vec![col("tag", None)],
                ),
            ],
        }
    }

    fn reply(content: &str, tokens: u64) -> ChatResponse {
        ChatResponse {
            choices: vec![Choice {
                message: crate::agent::llm::protocol::Message_ {
                    content: Some(content.to_string()),
                    reasoning_content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: tokens.saturating_sub(1),
                completion_tokens: 1,
                total_tokens: Some(tokens),
                completion_tokens_details: None,
            }),
        }
    }

    fn batch() -> ExcludeBatch {
        plan_batches(&report(), None, 10).unwrap().0.remove(0)
    }

    fn answer() -> String {
        serde_json::json!({"proposals": [
            {"subject": "customers", "exclude": false, "claim": "这是客户业务对象",
             "evidence": ["表注释：客户主档", "列 id（客户标识）"],
             "confidence": 0.92, "reason": "表注释明确写客户主档，列注释也是客户语义"},
            {"subject": "alembic_version", "exclude": true, "category": "migration_bookkeeping",
             "claim": "这是迁移框架的簿记表", "evidence": ["列 version_num（迁移版本号）"],
             "confidence": 0.99, "reason": "迁移工具的版本记账表，客户业务里没有对应事物"},
            {"subject": "_volc_dts_pg_ddl_abc", "exclude": true, "category": "cdc_residue",
             "claim": "这是同步工具的临时表", "evidence": ["表名 _volc_dts_pg_ddl_abc", "列 tag"],
             "confidence": 0.97, "reason": "表名是同步服务生成的临时 DDL，无业务列"}
        ]})
        .to_string()
    }

    #[test]
    fn validate_requires_explicit_decision_and_enum_category() {
        let b = batch();
        let mut stats = ProposalStats::default();
        let body = serde_json::json!({"proposals": [
            {"subject": "customers", "claim": "x",
             "evidence": ["表注释：客户主档"], "confidence": 0.9, "reason": "没给判断"},
            {"subject": "alembic_version", "exclude": true, "category": "因为没用",
             "claim": "x", "evidence": ["列 version_num（迁移版本号）"],
             "confidence": 0.9, "reason": "自由发挥的类别"}
        ]})
        .to_string();
        let got = validate_batch(&b, &body, "ai:m", &mut stats).unwrap();
        assert!(got.is_empty());
        assert_eq!(stats.rejected.get("missing_decision").copied(), Some(1));
        assert_eq!(stats.rejected.get("bad_category").copied(), Some(1));
    }

    #[test]
    fn validate_accepts_keep_and_exclude_and_marks_low_confidence() {
        let b = batch();
        let mut stats = ProposalStats::default();
        let got = validate_batch(&b, &answer(), "ai:m", &mut stats).unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(excluded_tables(&got).len(), 2);
        assert_eq!(kept_tables(&got), vec!["customers"]);
        assert!(got.iter().all(|d| d.item.reviewed_by().is_none()));
        // 0.92 不低；两条排除 0.97/0.99 也不低
        assert_eq!(stats.low_confidence, 0);
    }

    #[test]
    fn short_reason_is_dropped() {
        let b = batch();
        let mut stats = ProposalStats::default();
        let body = serde_json::json!({"proposals": [
            {"subject": "alembic_version", "exclude": true, "category": "migration_bookkeeping",
             "claim": "x", "evidence": ["列 version_num（迁移版本号）"],
             "confidence": 0.9, "reason": "没用"}
        ]})
        .to_string();
        let got = validate_batch(&b, &body, "ai:m", &mut stats).unwrap();
        assert!(got.is_empty());
        assert_eq!(stats.rejected.get("short_reason").copied(), Some(1));
    }

    #[test]
    fn output_is_directly_consumable_by_req113_parser() {
        let b = batch();
        let mut stats = ProposalStats::default();
        let got = validate_batch(&b, &answer(), "ai:m", &mut stats).unwrap();
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let h = header("m", &prompts.sha256, "input".to_string());
        let text = render_proposal(&got, &[], &[], &h, &stats).expect("产物必须能被 --exclude 消费");
        let ex = Exclusions::parse(text.as_bytes(), "p.json").expect("AC-003");
        assert_eq!(ex.entries().len(), 2);
        assert_eq!(ex.version(), EXCLUSION_FILE_VERSION);
        assert!(ex.rule().contains("七类"));
        assert_eq!(ex.declared_by(), "ai:m");
        assert_eq!(ex.reviewed_by(), None, "P2：没人签过");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["reviewed_by"], serde_json::Value::Null);
        assert_eq!(v["counts"]["judged"], 3);
        assert_eq!(v["counts"]["excluded"], 2);
        assert_eq!(v["consumable_by_exclude"], true);
        for p in v["proposals"].as_array().unwrap() {
            assert!(p["evidence"].is_array(), "ADR-005：提案里的 evidence 是数组");
        }
    }

    #[test]
    fn carried_over_entries_are_not_resent_and_keep_their_decided_by() {
        let existing_text = serde_json::json!({
            "version": "0.1.1",
            "rule": "只排除不是客户业务世界里的事物",
            "declared_by": "张三（业务）",
            "reviewed_by": "李四（评审）",
            "exclude": [{
                "table": "alembic_version",
                "category": "migration_bookkeeping",
                "reason": "迁移工具的版本记账表，客户业务里没有对应事物",
                "decided_by": "张三（业务）2026-10-01 确认",
                "confidence": 0.99
            }]
        })
        .to_string();
        let ex = Exclusions::parse(existing_text.as_bytes(), "old.json").unwrap();
        let (batches, carried, stale) = plan_batches(&report(), Some(&ex), 10).unwrap();
        assert_eq!(carried.len(), 1);
        assert!(stale.is_empty());
        let sent: Vec<&str> = batches
            .iter()
            .flat_map(|b| b.slices.iter().map(|s| s.table.as_str()))
            .collect();
        assert!(!sent.contains(&"alembic_version"), "已被人排除的表不该再送模型");
        assert_eq!(sent.len(), 2);
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let h = header("m", &prompts.sha256, "input".to_string());
        let text = render_proposal(&[], &carried, &stale, &h, &ProposalStats::default()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["exclude"][0]["decided_by"], "张三（业务）2026-10-01 确认");
        assert_eq!(v["counts"]["carried_over"], 1);
        // 单靠沿用条目也是合法 --exclude 文件（AC-003）
        Exclusions::parse(text.as_bytes(), "p.json").unwrap();
    }

    #[test]
    fn stale_exclusions_are_reported_not_deleted() {
        let existing_text = serde_json::json!({
            "version": 1, "rule": "规则", "declared_by": "某人",
            "exclude": [{
                "table": "gone_table", "category": "deprecated",
                "reason": "已经下线的业务表，保留排除",
                "decided_by": "某人"
            }]
        })
        .to_string();
        let ex = Exclusions::parse(existing_text.as_bytes(), "old.json").unwrap();
        let (batches, carried, stale) = plan_batches(&report(), Some(&ex), 10).unwrap();
        assert!(carried.is_empty());
        assert_eq!(stale.len(), 1, "schema 里已消失的排除必须报出来");
        assert_eq!(batches.iter().map(|b| b.slices.len()).sum::<usize>(), 3);
    }

    #[test]
    fn empty_exclusion_set_is_marked_not_consumable() {
        let decisions = vec![ExcludeDecision {
            table: "customers".into(),
            excluded: false,
            category: None,
            item: finalize_item(
                &crate::extract::proposal::RawProposal {
                    subject: "customers".into(),
                    claim: "客户业务对象".into(),
                    evidence: vec!["表注释：客户主档".into()],
                    confidence: Some(0.9),
                    reason: "表注释明确".into(),
                    ..Default::default()
                },
                "ai:m",
            )
            .unwrap(),
        }];
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let h = header("m", &prompts.sha256, "input".to_string());
        let text = render_proposal(&decisions, &[], &[], &h, &ProposalStats::default()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["consumable_by_exclude"], false);
        assert!(Exclusions::parse(text.as_bytes(), "p.json").is_err(), "空 exclude 会被 REQ-113 拒");
    }

    #[test]
    fn cache_makes_second_run_free_and_identical() {
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let mut transport = ScriptedTransport::new(vec![Ok(reply(&answer(), 400))]);
        let mut cache = MemoryProposalCache::new();
        let (batches, carried, _stale) = plan_batches(&report(), None, 10).unwrap();
        let h = header("m", &prompts.sha256, "input".to_string());
        let (d1, s1) = suggest_all(&mut transport, &mut cache, &batches, "m", &prompts, 10_000, 300);
        let a = render_proposal(&d1, &carried, &[], &h, &s1).unwrap();
        let (d2, s2) = suggest_all(&mut transport, &mut cache, &batches, "m", &prompts, 10_000, 300);
        let b = render_proposal(&d2, &carried, &[], &h, &s2).unwrap();
        assert_eq!(s2.tokens_spent, 0);
        assert_eq!(s2.calls, 0);
        assert_eq!(a, b, "第二次逐字节相同");
    }
}



