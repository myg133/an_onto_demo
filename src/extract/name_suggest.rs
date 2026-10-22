//! **`ontok name suggest`**（REQ-118 · 决策点一）—— 产品内置的中文名提案。
//!
//! # 它替代的是哪一段对话
//!
//! 过去 `table_names.json` 是**对话里的子 agent** 产的（文件里写着
//! `agent=ba-michael-20261021`）。产品有 `ontok-translate-names`（REQ-090），
//! 但主流程没用它 —— 于是「换个人跑同样的源库，拿不到同样的名字」。
//! 本模块把这件事变成**产品能力**：输入是 `schema.json`，输出是**带证据、置信度、
//! 理由、出处**的提案（[`crate::extract::proposal::ProposalItem`]）。
//!
//! # 它**不**产出的东西
//!
//! - 不产出 `TableNames` —— 那是 [`as_names`] 的职责（`--as-names` 显式转换）。
//!   两者分开的理由：提案是**评审对象**（每条可 diff），翻译表是**下游输入**
//!   （`ontok-propose --names` 只认 `{agent, version, generated_at, source, names}`）。
//!   一份文件同时当两种东西，会让「评审」与「喂下游」的字段互相拉扯。
//! - 不写库、不改本体：本模块零 I/O，`cli::name_cmd` 只写本地文件。
//!
//! # 为什么每条都要 `evidence`
//!
//! AC-009 的判据是「抽查 10 条，人据 evidence 能判断真伪」。所以提示词要求
//! **引用输入原文**（表注释 / 列名 / 列注释片段），而 [`validate_batch`] 会
//! 逐条检查「至少一条 evidence 能在输入里找到落点」——找不到就丢弃并计数。
//! ⛔ 不做「差不多」的匹配：一旦放宽，evidence 就从证据退化成提示
//! （与 REQ-116 的证据闸同一条论证）。
//!
//! # 与 REQ-090 的关系
//!
//! REQ-090 的 `ontok-translate-names` 仍是**增量生成翻译表**的工具（保护人工旧译、
//! 缺口闸、退化自检）。本命令是**从零提案**（每次都判全部表），两者不互相替代：
//! 前者维护知识资产，后者产出可评审的候选。`--as-names` 是两者的桥。

use crate::agent::llm::LlmTransport;
use crate::extract::ai_semantic::Prompts;
use crate::extract::introspect::{SchemaReport, TableMeta};
use crate::extract::names::TableNames;
use crate::extract::proposal::{
    decided_by_ai, digest_of, evidence_references_input, finalize_item, parse_proposals,
    render_document, run_batches, sha256_text, BatchRequest, Drop, EvidenceTokens,
    ProposalCache, ProposalHeader, ProposalItem, ProposalStats, SOURCE_AI,
};

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 提案逻辑版本 —— **进指纹**。改提示词策略 / 解析 / 校验就改这里。
pub const AGENT_VERSION: &str = "name-suggest-1";

/// 每次请求送几张表。
///
/// 12 是「一张表的注释 + 12 列 + 五字段输出」的折中：给 20 张时输出会顶到
/// `max_tokens`（正文被截断 ⇒ 整批作废），给 5 张时请求数翻倍。
pub const DEFAULT_BATCH_SIZE: usize = 12;

/// 每张表最多送几列（优先送**有注释**的列）。
pub const MAX_COLUMNS_IN_PROMPT: usize = 12;

/// 中文名的长度上限（提示词用；校验只看「非空 / 不等于表名」）。
pub const NAME_MAX_CHARS: usize = 12;

/// 固定提示词 —— **改动会改变 `prompt_sha256`，从而让所有缓存失效**（P3）。
/// 模板里**不含任何本次输入**：表结构走 user 消息。
pub const SYSTEM_PROMPT: &str = r#"你是企业数据库的业务术语翻译器。你的唯一任务：为给定的每一张表起一个**业务人员日常会说**的中文名，并给出可核实的依据。

输入是一个 JSON：{"tables": [{"table": "表名", "table_comment": "表注释或空串", "columns": [{"name": "列名", "type": "类型", "pk": true/false, "comment": "列注释或 null"}]}], "task": "..."}

输出必须是严格 JSON（不要 markdown 代码块、不要任何解释文字）：
{"proposals": [{"subject": "表名", "name": "中文名", "claim": "一句话判断", "evidence": ["可核实的依据"], "confidence": 0.0到1.0, "reason": "一句话说明依据如何支撑结论"}]}

规则（每一条都会被程序校验，不合规的条目会被整条丢弃）：
1. subject 必须是输入里的表名，**逐字一致**。⛔ 不许编造表名，也不许漏掉输入里的表。
2. name 是 2–12 个简体中文字，业务人员日常说法（「客户」「销售订单」「收货地址」）。
   ⛔ 不许等于表名本身、⛔ 不许中英混排、⛔ 不许用建模术语（主键、外键、字段、实体、实例、记录表、主数据、基数、维度表）。
3. claim 是一句话判断，例如「这张表表达的业务对象是「客户套餐订阅」」。
4. evidence 给 1–3 条，**每一条都必须是输入里能查到的原文**：
   抄表注释片段，或写「列 customer_id（列注释原文）」，或写「表名 orders」。
   ⛔ 不许写「模型认为」「推测」「应该是」这类不可核实的话。
5. confidence 是 0–1 的数：注释与列名一致可给 0.8 以上；只靠表名猜、或注释为空，给 0.6 以下。
6. reason 一句话，说清**证据如何**支撑这个名字，不要复述输入、不要写「因此置信度 X」。
7. 表没有注释时，靠表名与列名判断，并在 reason 里明说信息不足（不要装确定）。
"#;

/// 每条请求里固定的 task 文案（进 user 消息，不进模板哈希）。
const TASK_TEXT: &str = "为输入里的每一张表给出中文业务名与可核实的依据，按协议返回严格 JSON。";

// ──────────────────────────────────────────────────────────
// 输入切片
// ──────────────────────────────────────────────────────────

/// 一张表的**提示词切片**（只送判断需要的东西；⛔ 不送整份 schema）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSlice {
    pub table: String,
    pub table_comment: String,
    pub columns: Vec<ColumnSlice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSlice {
    pub name: String,
    pub data_type: String,
    pub is_primary_key: bool,
    pub comment: Option<String>,
}

impl TableSlice {
    pub fn from_meta(t: &TableMeta) -> Self {
        // 优先送有注释的列（信息量最大），不足 MAX_COLUMNS_IN_PROMPT 再补其余列。
        let mut with_comment: Vec<&crate::extract::introspect::ColumnMeta> = t
            .columns
            .iter()
            .filter(|c| c.comment.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_some())
            .collect();
        let mut without: Vec<&crate::extract::introspect::ColumnMeta> = t
            .columns
            .iter()
            .filter(|c| c.comment.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_none())
            .collect();
        with_comment.append(&mut without);
        let columns = with_comment
            .into_iter()
            .take(MAX_COLUMNS_IN_PROMPT)
            .map(|c| ColumnSlice {
                name: c.name.clone(),
                data_type: c.data_type.clone(),
                is_primary_key: c.is_primary_key,
                comment: c
                    .comment
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            })
            .collect();
        TableSlice {
            table: t.name.clone(),
            table_comment: t
                .comment
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_string(),
            columns,
        }
    }

    /// 送模型用的紧凑 JSON 形状（键顺序由 `json!` 的 BTreeMap 保证稳定）。
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "table": self.table,
            "table_comment": self.table_comment,
            "columns": self.columns.iter().map(|c| serde_json::json!({
                "name": c.name,
                "type": c.data_type,
                "pk": c.is_primary_key,
                "comment": c.comment,
            })).collect::<Vec<_>>(),
        })
    }

    /// AC-009 的可核实面：表名 + 列名 + 注释。
    pub(crate) fn tokens(&self) -> EvidenceTokens {
        let mut comments = Vec::new();
        if !self.table_comment.is_empty() {
            comments.push(self.table_comment.clone());
        }
        for c in &self.columns {
            if let Some(cmt) = &c.comment {
                comments.push(cmt.clone());
            }
        }
        EvidenceTokens::new(
            self.table.clone(),
            self.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            comments,
        )
    }
}

// ──────────────────────────────────────────────────────────
// 批与请求
// ──────────────────────────────────────────────────────────

/// 一批表 + 它对应的请求。
#[derive(Debug, Clone)]
pub struct NameBatch {
    pub slices: Vec<TableSlice>,
    pub request: BatchRequest,
}

/// 把一个 schema 按**表名排序**切成批（确定性：批边界只由输入决定）。
pub fn plan_batches(report: &SchemaReport, batch_size: usize) -> anyhow::Result<Vec<NameBatch>> {
    if batch_size == 0 {
        anyhow::bail!("--batch-size 必须是正整数（0 意味着一个请求都不发）");
    }
    let mut tables: Vec<&TableMeta> = report.tables.iter().collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = Vec::new();
    for chunk in tables.chunks(batch_size) {
        let slices: Vec<TableSlice> = chunk.iter().map(|t| TableSlice::from_meta(t)).collect();
        let body = serde_json::json!({
            "tables": slices.iter().map(TableSlice::to_json).collect::<Vec<_>>(),
            "task": TASK_TEXT,
        });
        let user_message = serde_json::to_string(&body)?;
        // 批的 `input_digest` 只对**这一批**的表算：换了批次组成就换缓存。
        let input_digest = sha256_text(&user_message);
        let key = format!(
            "{}..{}",
            slices.first().map(|s| s.table.as_str()).unwrap_or("-"),
            slices.last().map(|s| s.table.as_str()).unwrap_or("-")
        );
        out.push(NameBatch {
            request: BatchRequest {
                key,
                item_count: slices.len(),
                input_digest,
                user_message,
            },
            slices,
        });
    }
    Ok(out)
}

/// 整份 schema 的规范化指纹（进文件头 `input_sha256`，用于「这份提案基于哪份输入」）。
pub fn schema_digest(report: &SchemaReport) -> anyhow::Result<String> {
    let mut slices: Vec<TableSlice> = report.tables.iter().map(TableSlice::from_meta).collect();
    slices.sort_by(|a, b| a.table.cmp(&b.table));
    digest_of(&slices.iter().map(TableSlice::to_json).collect::<Vec<_>>())
}

// ──────────────────────────────────────────────────────────
// 解析 + 校验
// ──────────────────────────────────────────────────────────

/// 一条通过的命名提案。
#[derive(Debug, Clone, PartialEq)]
pub struct NameSuggestion {
    pub table: String,
    pub name: String,
    pub item: ProposalItem,
}

/// 校验一批回答：丢弃 + 计数，绝不修正模型输出。
///
/// 除公共校验外，本命令多判四件事：
/// 1. `subject` 必须**在这一批里**（模型编造的表名会污染名字映射）；
/// 2. `name` 非空、且**不等于表名**（等于就没翻，下游会当退化值丢掉）；
/// 3. 至少一条 evidence 能在这张表的输入里找到落点（AC-009）；
/// 4. `name` 不许撞客户发布闸的建模术语（否则客户视图整份 500 —— 见
///    `domain::pipeline::group_name_hits`）。
pub fn validate_batch(
    batch: &NameBatch,
    body: &str,
    decided_by: &str,
    stats: &mut ProposalStats,
) -> std::result::Result<Vec<NameSuggestion>, String> {
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
        let item = match finalize_item(&raw, decided_by) {
            Ok(i) => i,
            Err(d) => {
                stats.reject(d, subject);
                continue;
            }
        };
        let name = raw.name.as_deref().map(str::trim).unwrap_or("").to_string();
        if name.is_empty() {
            stats.reject(Drop::MissingName, subject);
            continue;
        }
        if name.eq_ignore_ascii_case(&slice.table) {
            stats.reject(Drop::NameEqualsTable, format!("{} → {name}", slice.table));
            continue;
        }
        if !evidence_references_input(&item.evidence, &slice.tokens()) {
            stats.reject(
                Drop::EvidenceNotFromInput,
                format!("{}：{:?}", slice.table, item.evidence),
            );
            continue;
        }
        // 客户发布闸：实体 display_name 会进客户视图；带建模术语的名字会让整份视图被拒。
        if let Some(term) = crate::domain::pipeline::group_name_hits(&name).first() {
            stats.reject(
                Drop::CustomerGate,
                format!("{} 的名字 {name:?} 含建模术语 {term:?}", slice.table),
            );
            continue;
        }
        out.push(NameSuggestion {
            table: slice.table.clone(),
            name,
            item,
        });
    }
    Ok(out)
}

/// 跑全部批次：缓存 / 预算 / 记账都在 [`run_batches`] 里（三命令共用）。
pub fn suggest_all(
    transport: &mut dyn LlmTransport,
    cache: &mut dyn ProposalCache,
    batches: &[NameBatch],
    model: &str,
    prompts: &Prompts,
    budget: u64,
    max_tokens: u32,
) -> (Vec<NameSuggestion>, ProposalStats) {
    let decided_by = decided_by_ai(model);
    let reqs: Vec<BatchRequest> = batches.iter().map(|b| b.request.clone()).collect();
    let (mut suggestions, mut stats) = run_batches(
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
    // 稳定排序：按表名（`run_batches` 不知道条目的排序键 —— 那是各命令自己的事）。
    suggestions.sort_by(|a, b| a.table.cmp(&b.table));
    stats.proposed = suggestions.len();
    stats.low_confidence = suggestions
        .iter()
        .filter(|s| s.item.is_low_confidence())
        .count();
    (suggestions, stats)
}

// ──────────────────────────────────────────────────────────
// 渲染
// ──────────────────────────────────────────────────────────

/// 提案文件的头部（三命令共用同一形状，见 `proposal::ProposalHeader`）。
pub fn header(
    report: &SchemaReport,
    model: &str,
    prompt_sha: &str,
) -> anyhow::Result<ProposalHeader> {
    Ok(ProposalHeader {
        kind: "name",
        source: SOURCE_AI,
        model: Some(model.to_string()),
        prompt_sha: Some(prompt_sha.to_string()),
        agent_version: AGENT_VERSION.to_string(),
        input_sha256: schema_digest(report)?,
        decided_by: decided_by_ai(model),
    })
}

/// 提案 JSON（P1：落盘、可 diff）。`names` 映射只是**方便 diff**，
/// 下游要 `TableNames` 请用 [`as_names`]。
pub fn render_proposal(
    suggestions: &[NameSuggestion],
    report: &SchemaReport,
    header: &ProposalHeader,
    stats: &ProposalStats,
) -> anyhow::Result<String> {
    let items: Vec<ProposalItem> = suggestions.iter().map(|s| s.item.clone()).collect();
    let names: std::collections::BTreeMap<&str, &str> = suggestions
        .iter()
        .map(|s| (s.table.as_str(), s.name.as_str()))
        .collect();
    let named: std::collections::BTreeSet<&str> =
        suggestions.iter().map(|s| s.table.as_str()).collect();
    let missing: Vec<&str> = {
        let mut v: Vec<&str> = report
            .tables
            .iter()
            .map(|t| t.name.as_str())
            .filter(|t| !named.contains(t))
            .collect();
        v.sort();
        v
    };
    let mut extra = serde_json::Map::new();
    extra.insert(
        "names".to_string(),
        serde_json::to_value(names)?,
    );
    extra.insert(
        "coverage".to_string(),
        serde_json::json!({
            "tables_in_schema": report.tables.len(),
            "named": suggestions.len(),
            "missing": missing,
        }),
    );
    extra.insert(
        "low_confidence_threshold".to_string(),
        serde_json::json!(crate::extract::proposal::LOW_CONFIDENCE_THRESHOLD),
    );
    render_document(header, &items, stats, extra)
}

/// `--as-names`：转成 **`ontok-propose --names` 可直接消费**的 `TableNames` 形状（AC-002）。
///
/// 额外带 `_proposal` 块：**这条名字来自哪份提案、谁签的**（P4）。
/// `TableNames` 没有 `deny_unknown_fields`，所以 `--names` 那边原样忽略它。
///
/// `generated_at` 取 **schema 的勘查时刻**而不是 `now()`：两件事都不能破 ——
/// AC-005 要求两次运行逐字节相同，而「这份名字基于哪次勘查」本身是事实。
pub fn as_names(
    suggestions: &[NameSuggestion],
    report: &SchemaReport,
    model: &str,
    proposal_file: &str,
    proposal_sha: &str,
    prompt_sha: &str,
) -> anyhow::Result<String> {
    let mut names: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for s in suggestions {
        names.insert(s.table.clone(), s.name.clone());
    }
    let named: std::collections::BTreeSet<&str> =
        suggestions.iter().map(|s| s.table.as_str()).collect();
    let mut missing: Vec<&str> = report
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .filter(|t| !named.contains(t))
        .collect();
    missing.sort();
    let doc = serde_json::json!({
        "agent": decided_by_ai(model),
        "version": format!("{}+ai.{model}", env!("CARGO_PKG_VERSION")),
        "generated_at": report.introspected_at,
        "source": format!(
            "{} schema, {} tables (introspected {})",
            report.schema,
            report.tables.len(),
            report.introspected_at
        ),
        "names": names,
        "_proposal": {
            "file": proposal_file,
            "sha256": proposal_sha,
            "decided_by": decided_by_ai(model),
            "prompt_sha": prompt_sha,
            "reviewed_by": serde_json::Value::Null,
            "low_confidence_threshold": crate::extract::proposal::LOW_CONFIDENCE_THRESHOLD,
        },
        "_coverage": {
            "tables_in_schema": report.tables.len(),
            "named": suggestions.len(),
            "untranslated": missing,
        },
    });
    let mut text = serde_json::to_string_pretty(&doc)?;
    text.push('\n');
    // AC-001 / REQ-090 的同一道闸：产物**先过** `TableNames::load` 再落地。
    // 过不了 ⇒ 不写文件（坏产物连落地都不落地）。
    TableNames::load(&text).map_err(|e| {
        anyhow::anyhow!("--as-names 产物未通过 TableNames::load 的校验（**未写出**）：{e:#}")
    })?;
    Ok(text)
}

/// 名字映射（`表名 → 中文名`），给报告与测试用。
pub fn name_map(suggestions: &[NameSuggestion]) -> std::collections::BTreeMap<String, String> {
    suggestions
        .iter()
        .map(|s| (s.table.clone(), s.name.clone()))
        .collect()
}

/// 便于上层断言「这批表有没有被覆盖」。
pub fn missing_tables(report: &SchemaReport, suggestions: &[NameSuggestion]) -> Vec<String> {
    let named: std::collections::BTreeSet<&str> =
        suggestions.iter().map(|s| s.table.as_str()).collect();
    let mut v: Vec<String> = report
        .tables
        .iter()
        .map(|t| t.name.clone())
        .filter(|t| !named.contains(t.as_str()))
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::protocol::{Choice, Usage};
    use crate::agent::llm::{ChatResponse, ScriptedTransport};
    use crate::extract::proposal::MemoryProposalCache;
    use crate::extract::introspect::{ColumnMeta, TableMeta};

    fn col(name: &str, comment: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            data_type: "bigint".to_string(),
            nullable: false,
            comment: comment.map(str::to_string),
            is_primary_key: name == "id",
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
            table_count: 2,
            column_count: 4,
            commented_column_count: 3,
            foreign_key_count: 0,
            introspected_at: "2026-10-22T00:00:00Z".into(),
            tables: vec![
                table(
                    "customers",
                    Some("客户主档"),
                    vec![col("id", Some("客户标识")), col("name", Some("客户名称"))],
                ),
                table("_volc_dts_pg_ddl_abc", None, vec![col("tag", None)]),
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

    fn batch_for(report: &SchemaReport) -> NameBatch {
        plan_batches(report, 10).expect("分批").remove(0)
    }

    fn valid_answer() -> String {
        serde_json::json!({"proposals": [
            {"subject": "customers", "name": "客户", "claim": "这张表是「客户」",
             "evidence": ["表注释：客户主档", "列 name（客户名称）"],
             "confidence": 0.93, "reason": "表注释与列注释一致"},
            {"subject": "_volc_dts_pg_ddl_abc", "name": "同步临时表",
             "claim": "这张表是同步工具的临时表",
             "evidence": ["表名 _volc_dts_pg_ddl_abc", "列 tag"],
             "confidence": 0.55, "reason": "表名为同步工具命名，且无任何注释"}
        ]})
        .to_string()
    }

    #[test]
    fn plan_is_deterministic_and_respects_input() {
        let r = report();
        let a = plan_batches(&r, 1).unwrap();
        let b = plan_batches(&r, 1).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].slices[0].table, "_volc_dts_pg_ddl_abc", "按表名排序");
        assert_eq!(a[0].request.user_message, b[0].request.user_message);
        assert_eq!(schema_digest(&r).unwrap(), schema_digest(&b_report()).unwrap());
    }

    fn b_report() -> SchemaReport {
        // 表顺序颠倒但内容相同 ⇒ 同一份 schema 指纹（确定性来自排序）
        let mut r = report();
        r.tables.reverse();
        r
    }

    #[test]
    fn validate_accepts_good_items_and_marks_low_confidence() {
        let b = batch_for(&report());
        let mut stats = ProposalStats::default();
        let got = validate_batch(&b, &valid_answer(), "ai:m", &mut stats).expect("协议合法");
        assert_eq!(got.len(), 2);
        let m = name_map(&got);
        assert_eq!(m.get("customers").map(String::as_str), Some("客户"));
        let low = got.iter().find(|s| s.table.starts_with("_volc")).unwrap();
        assert!(low.item.is_low_confidence(), "0.55 < 0.6 要标低置信（R4）");
        assert_eq!(low.item.reviewed_by(), None, "提案永不权威（P2）");
        assert_eq!(stats.rejected_total(), 0);
    }

    #[test]
    fn fabricated_table_and_name_equal_to_table_are_dropped() {
        let b = batch_for(&report());
        let mut stats = ProposalStats::default();
        let body = serde_json::json!({"proposals": [
            {"subject": "ghost_table", "name": "幽灵", "claim": "x",
             "evidence": ["表名 ghost_table"], "confidence": 0.9, "reason": "编的表"},
            {"subject": "customers", "name": "customers", "claim": "x",
             "evidence": ["表名 customers"], "confidence": 0.9, "reason": "等于没翻"}
        ]})
        .to_string();
        let got = validate_batch(&b, &body, "ai:m", &mut stats).unwrap();
        assert!(got.is_empty());
        assert_eq!(stats.rejected.get("unknown_subject").copied(), Some(1));
        assert_eq!(stats.rejected.get("name_equals_table").copied(), Some(1));
    }

    #[test]
    fn evidence_without_input_anchor_is_dropped() {
        let b = batch_for(&report());
        let mut stats = ProposalStats::default();
        let body = serde_json::json!({"proposals": [
            {"subject": "customers", "name": "客户", "claim": "x",
             "evidence": ["模型认为这像客户表"], "confidence": 0.9, "reason": "编的"}
        ]})
        .to_string();
        let got = validate_batch(&b, &body, "ai:m", &mut stats).unwrap();
        assert!(got.is_empty());
        assert_eq!(stats.rejected.get("unverifiable_phrase").copied(), Some(1));
    }

    #[test]
    fn as_names_roundtrips_through_table_names_load() {
        let b = batch_for(&report());
        let mut stats = ProposalStats::default();
        let got = validate_batch(&b, &valid_answer(), "ai:m", &mut stats).unwrap();
        let text = as_names(&got, &report(), "m", "out/n.proposal.json", "abc", "psha").unwrap();
        let loaded = TableNames::load(&text).expect("AC-002：必须能被 --names 消费");
        assert_eq!(loaded.lookup("customers"), Some("客户"));
        assert_eq!(loaded.agent, "ai:m");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["_proposal"]["reviewed_by"], serde_json::Value::Null);
        assert_eq!(v["_proposal"]["sha256"], "abc");
        assert_eq!(v["_coverage"]["named"], 2);
    }

    #[test]
    fn proposal_document_has_adr_shape_and_null_reviewed_by() {
        let b = batch_for(&report());
        let mut stats = ProposalStats::default();
        let got = validate_batch(&b, &valid_answer(), "ai:m", &mut stats).unwrap();
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let h = header(&report(), "m", &prompts.sha256).unwrap();
        let text = render_proposal(&got, &report(), &h, &stats).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["kind"], "name");
        assert_eq!(v["source"], "ai");
        assert_eq!(v["reviewed_by"], serde_json::Value::Null);
        assert_eq!(v["proposal_format"], 1);
        for p in v["proposals"].as_array().unwrap() {
            for k in ["claim", "evidence", "confidence", "reason", "decided_by", "reviewed_by"] {
                assert!(p.get(k).is_some(), "AC-001：每条都要有 {k}");
            }
            assert_eq!(p["reviewed_by"], serde_json::Value::Null);
            assert!(!p["evidence"].as_array().unwrap().is_empty());
            assert_ne!(p["reason"].as_str().unwrap().trim(), "");
        }
        assert_eq!(v["coverage"]["named"], 2);
        assert_eq!(v["coverage"]["tables_in_schema"], 2);
    }

    #[test]
    fn cache_makes_second_run_free_and_identical() {
        let prompts = Prompts::from_system(SYSTEM_PROMPT);
        let mut transport = ScriptedTransport::new(vec![Ok(reply(&valid_answer(), 250))]);
        let mut cache = MemoryProposalCache::new();
        let batches = plan_batches(&report(), 10).unwrap();
        let (s1, st1) = suggest_all(&mut transport, &mut cache, &batches, "m", &prompts, 10_000, 300);
        let h = header(&report(), "m", &prompts.sha256).unwrap();
        let a = render_proposal(&s1, &report(), &h, &st1).unwrap();
        let (s2, st2) = suggest_all(&mut transport, &mut cache, &batches, "m", &prompts, 10_000, 300);
        let b = render_proposal(&s2, &report(), &h, &st2).unwrap();
        assert_eq!(st1.tokens_spent, 250);
        assert_eq!(st2.tokens_spent, 0, "第二次 0 token");
        assert_eq!(st2.calls, 0);
        assert_eq!(a, b, "第二次输出逐字节相同");
    }
}


