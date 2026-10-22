//! **AI 提案框架**（REQ-118）—— 三条 `suggest` 共用的一套提案形状 / 校验 / 缓存 / 预算。
//!
//! # 一句话
//!
//! ADR-005 把「决策点」的定义压在两条上：需要语义判断，且结果要人签字。
//! 本模块只负责**提案**这一半 —— 它产出的每一条都长成 ADR-005 §「提案的统一形状」：
//!
//! ```json
//! { "subject": "customers", "claim": "这张表是「客户」",
//!   "evidence": ["表名 customers", "列 customer_name（客户名称）"],
//!   "confidence": 0.93, "reason": "表名与列注释一致",
//!   "decided_by": "ai:qwen36-27B", "reviewed_by": null }
//! ```
//!
//! `subject` 是**机器可读的对象标识**（表名 / 组 key）。ADR-005 的六字段里没有它，
//! 但下游（`--as-names` 转换、`--exclude` 转换）必须知道这条提案说的是**谁** ——
//! 把标识塞进 `claim` 的正文里等于让下游去解析自然语言。
//!
//! # 四条铁律落在哪一行
//!
//! | # | 规则 | 落点 |
//! | --- | --- | --- |
//! | **P1** | 提案必须落盘、可 diff | [`render_document`] 的 **pretty JSON + 稳定排序**；三条命令的 `--out` 必填（`cli` 层） |
//! | **P2** | 提案永不权威 | [`ProposalItem::reviewed_by`] 是**私有字段**：外部模块**构造不出**带签字的条目，只能经 [`finalize_item`]，而它恒写 `None` |
//! | **P3** | 可复现 | [`BatchRequest::fingerprint`] = `sha256(v1 + key + input_digest + prompt_sha + model + agent_version)`；命中缓存**绝不调用** |
//! | **P4** | 可溯源 | [`decided_by_ai`] / [`RULE_DECIDED_BY`] 两个值把「谁提的」写进每一条 |
//!
//! # 纪律 H：形状与校验**只有这一处**
//!
//! 三条命令（`name suggest` / `exclude suggest` / `concept suggest --ai`）都：
//!
//! 1. 用 [`RawProposal`] + [`parse_proposals`] 读模型回答（同一个协议）；
//! 2. 用 [`finalize_item`] 做**公共校验**（claim/evidence/reason/confidence/decided_by）；
//! 3. 用 [`run_batches`] 跑缓存 + 预算 + 记账（同一个编排）；
//! 4. 用 [`render_document`] 落盘（同一个渲染器）。
//!
//! 各命令**只**负责「提示词 + 额外的机器校验」（名字不能等于表名、category 必须在七类里、
//! 分组成员必须在实体集合里）。所以「新增一条提案通道」不会新增第二份提案形状。
//!
//! # 缓存为什么复用 REQ-116 的实现而不是再写一份
//!
//! [`crate::extract::doc_ingest`] 的缓存载荷（**回答原文** + model + prompt_sha + tokens）
//! 与这里要存的东西逐字段相同。复制一份的代价不是 100 行代码，而是**两种磁盘形状**：
//! 同一个 `<out>.cache.json` 名字下会有两套 `version` / `entries` 约定，
//! 而「第二次运行 0 token」的判据看不出区别。这里直接 `pub use` 那一份，
//! 连 `cache_to_json` / `cache_from_json` 都是同一对函数。
//!
//! ⚠️ 缓存里存的是**回答原文**，不是已校验的提案：每次运行都会**重新过一遍校验**
//! （丢弃规则可能改进），缓存只省「模型调用」，不省「校验」。
//!
//! # 预算为什么是「不再发起下一次」而不是「保证不超」
//!
//! 单次调用的花费在调用前不可知（取决于提示词长度与模型的推理量）。
//! 所以 [`run_batches`] 在**发起下一次之前**判 `tokens_spent >= budget`，
//! 越过的那一笔照样记账（钱已经花了），只是不会再花第二笔；
//! 装配层据此**不产出任何提案文件**（AC-006：不产半份文件）。
//!
//! # 本模块零 I/O
//!
//! 传输（[`LlmTransport`]）与缓存（[`ProposalCache`]）都是注入的 trait；
//! 读写文件由 `cli` 负责。所以全部 AC 都能用 `ScriptedTransport` + 内存缓存跑。

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::agent::llm::protocol::Message;
use crate::agent::llm::{completion_text, ChatRequest, LlmTransport};
use crate::extract::ai_semantic::Prompts;
use crate::extract::sample::sha256_hex;

// 缓存与预算的**实现**只有一处（REQ-116 的本地文件缓存 / 默认上限 / 输出上限）。
// 这里只做别名，让提案通道读起来是「提案的缓存」，而不是「文档的缓存」。
pub use crate::extract::doc_ingest::{
    cache_from_json, cache_to_json, require_budget, resolve_max_tokens,
    CachedChunk as CachedResponse, DocCache as ProposalCache,
    MemoryDocCache as MemoryProposalCache, DEFAULT_BUDGET_TOKENS, DEFAULT_MAX_TOKENS,
};

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 提案文件的**格式版本**。改字段语义 = 改这里（下游按它判断能不能读）。
pub const PROPOSAL_FORMAT: u32 = 1;

/// 指纹算法的版本前缀。**改指纹构成 = 改这里**（否则旧缓存会被新算法误命中）。
pub const FINGERPRINT_VERSION: &str = "v1";

/// R4：低于这个置信度的提案**照样输出**，但标 `low_confidence: true`。
pub const LOW_CONFIDENCE_THRESHOLD: f64 = 0.6;

/// P4：AI 提的（`decided_by = "ai:<model>"` 的前缀）。
pub const SOURCE_AI: &str = "ai";
/// P4：规则算的（`decided_by = "rule"`）。
pub const SOURCE_RULE: &str = "rule";
/// 规则路径的 `decided_by` 常量值。
pub const RULE_DECIDED_BY: &str = "rule";

/// AC-009：这些措辞**不可核实**，出现即整条丢弃（⛔ 不修正、不改写）。
///
/// 只收「把模型自己当依据」这一类。⛔ 不要把「不确定」「可能」收进来 ——
/// 它们恰恰是**诚实**的表达，收进来会逼模型装确定。
pub const UNVERIFIABLE_MARKERS: &[&str] = &[
    "模型认为",
    "模型判断",
    "模型觉得",
    "AI 认为",
    "AI认为",
    // ⚠️ **不要**把「模型」「语言模型」「大语言模型」收进来：本产品的业务域里
    // 真的有「客户模型级 TPM 限流」这类表，它们的理由天然会写「针对大语言模型」。
    // 实测踩过：一条 `customer_tpm_rate_limit` 的合法提案因为理由里有
    // 「大语言模型」被误杀（AC-009 要抓的是「模型自证」，不是业务名词）。
    "我觉得",
];

/// R4 / P4 的人读阈值（报告里打印用；判据本身是 [`LOW_CONFIDENCE_THRESHOLD`]）。
pub fn low_confidence_label() -> String {
    format!("{LOW_CONFIDENCE_THRESHOLD:.2}")
}

/// `decided_by` 的唯一拼法（P4）：`ai:<model>`。
pub fn decided_by_ai(model: &str) -> String {
    format!("{SOURCE_AI}:{model}")
}

// ──────────────────────────────────────────────────────────
// 提案形状（P1 / P2 / P4）
// ──────────────────────────────────────────────────────────

/// 一条**已校验**的提案。
///
/// `reviewed_by` 刻意**私有**：P2 说「未签字的提案不得进入快照」，
/// 若任何模块都能 `ProposalItem { reviewed_by: Some("我"), .. }`，
/// 那条纪律就退化成「靠调用方记得别写」。现在构造只有 [`finalize_item`] 一条路，
/// 它恒写 `None` —— 签名只能来自**评审流程**（本 REQ 不做，见 demand §「不做」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalItem {
    /// 机器可读的对象标识（表名 / 组 key）。
    pub subject: String,
    /// 判断本身（ADR-005：`claim`）。
    pub claim: String,
    /// 依据（ADR-005：`evidence[]`）。**至少一条**；空 ⇒ 整条丢弃。
    pub evidence: Vec<String>,
    /// 0.0–1.0。越界 / 缺失 ⇒ 整条丢弃（不猜 0.5）。
    pub confidence: f64,
    /// 为什么这么判。空 ⇒ 整条丢弃。
    pub reason: String,
    /// 谁提的（P4）：`ai:<model>` 或 `rule`。
    pub decided_by: String,
    /// 谁签的（P2）。**生产路径恒 `None`** —— 见类型注释。
    reviewed_by: Option<String>,
    /// R4：置信度低于 [`LOW_CONFIDENCE_THRESHOLD`]。评审界面据此优先展示。
    #[serde(default, skip_serializing_if = "is_false")]
    pub low_confidence: bool,
}

impl ProposalItem {
    /// 谁签的字。**只读** —— 没有 `set_reviewed_by`，签字段只能由评审流程写入库。
    pub fn reviewed_by(&self) -> Option<&str> {
        self.reviewed_by.as_deref()
    }

    /// 是否被标记为低置信（R4）。
    pub fn is_low_confidence(&self) -> bool {
        self.low_confidence
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 模型原始回答里的一条（**未校验**）。
///
/// 公共字段（claim/evidence/confidence/reason）由协议固定；
/// `name` / `members` / `category` / `exclude` 是三条命令各自的**额外机器字段**，
/// 缺省即可 —— 所以一份回答形状只在这里定义一次。
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct RawProposal {
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub claim: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub reason: String,
    /// `name suggest` / `concept suggest --ai`：中文业务名 / 组名。
    #[serde(default)]
    pub name: Option<String>,
    /// `concept suggest --ai`：分组成员（实体 key）。
    #[serde(default)]
    pub members: Vec<String>,
    /// `exclude suggest`：REQ-113 的七类枚举之一（仅 `exclude: true` 时有意义）。
    #[serde(default)]
    pub category: Option<String>,
    /// `exclude suggest`：这张表**该不该**进本体。
    #[serde(default)]
    pub exclude: Option<bool>,
}

/// 一次模型回答的解析结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedResponse {
    pub proposals: Vec<RawProposal>,
    /// 形状不对的单条（不是数组项 / 类型不符）。**计数并公示**，不静默。
    pub malformed: Vec<String>,
}

/// 去掉模型偶尔加的 markdown 围栏。**只剥围栏、不猜内容**。
///
/// 剥完必须仍是合法 JSON，否则整份回答作废 —— 与 `ai_semantic` 同一口径。
pub fn strip_code_fence(text: &str) -> &str {
    let t = text.trim();
    if !t.starts_with("```") {
        return t;
    }
    let after = match t.find('\n') {
        Some(i) => &t[i + 1..],
        None => return t,
    };
    match after.rfind("```") {
        Some(i) => after[..i].trim(),
        None => after.trim(),
    }
}

/// **协议里唯一的解析入口**：`{"proposals": [...]}` → [`RawProposal`] 列表。
///
/// 整个响应不是合法 JSON / 缺 `proposals` 数组 ⇒ `Err`（整份作废）；
/// 单条形状不对 ⇒ 进 [`ParsedResponse::malformed`]（只丢那一条）。
pub fn parse_proposals(text: &str) -> std::result::Result<ParsedResponse, String> {
    let body = strip_code_fence(text);
    let v: Value = serde_json::from_str(body).map_err(|e| {
        format!(
            "响应不是合法 JSON：{e}（截断到 200 字：{}）",
            body.chars().take(200).collect::<String>()
        )
    })?;
    let arr = v
        .get("proposals")
        .and_then(Value::as_array)
        .ok_or_else(|| "响应里没有 proposals 数组（协议要求 {\"proposals\": [...]}）".to_string())?;
    let mut out = ParsedResponse::default();
    for (i, raw) in arr.iter().enumerate() {
        match serde_json::from_value::<RawProposal>(raw.clone()) {
            Ok(p) => out.proposals.push(p),
            Err(e) => out
                .malformed
                .push(format!("proposals[{i}] 形状不合法：{e}")),
        }
    }
    Ok(out)
}

/// 一条提案被丢弃的原因（**分类计数** —— AC-001 要求「丢弃并计数」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    /// `subject` 空
    NoSubject,
    /// `subject` 不在本次输入里（模型编造的对象）
    UnknownSubject,
    /// `claim` 空
    EmptyClaim,
    /// 没有 evidence
    NoEvidence,
    /// `reason` 空
    EmptyReason,
    /// 证据/理由里有**不可核实**的措辞（AC-009）
    UnverifiablePhrase,
    /// 没有一条 evidence 能在输入里找到落点（AC-009）
    EvidenceNotFromInput,
    /// confidence 缺失 / 越界
    BadConfidence,
    /// `decided_by` 空（P4：没有出处）
    NoDecidedBy,
    /// 名字缺失 / 空
    MissingName,
    /// 名字与表名相同（等于没翻，下游会当退化值丢掉）
    NameEqualsTable,
    /// `exclude` 判断缺失（这张表该不该进本体必须显式给）
    MissingDecision,
    /// `exclude: true` 却没有 category
    MissingCategory,
    /// category 不在 REQ-113 的七类枚举里
    BadCategory,
    /// `exclude: true` 但 reason 太短（业务读不懂）
    ShortReason,
    /// 同一个对象出现两次
    DuplicateSubject,
    /// 组成员与已接受的组重叠（`validate_groups` 会判 `OverlappingMembership`）
    Overlap,
    /// 组里一个成员都不在实体集合里（编造的组）
    NoMembers,
    /// 组字段撞客户发布闸（组名会在 `/api/graph` 与总览里出现）
    CustomerGate,
    /// 整份回答解析不了
    ResponseParse,
    /// 响应没有 usage ⇒ 无法记账（fail-closed，不产出判断）
    NoUsage,
    /// 正文为空 / 被截断（那是输出上限的属性，不是输入的）
    Truncated,
    /// 传输层失败
    CallFailed,
}

impl Drop {
    /// 进 `stats.rejected` 的稳定键（**不是**人读文案：机器按它统计）。
    pub fn code(self) -> &'static str {
        match self {
            Drop::NoSubject => "no_subject",
            Drop::UnknownSubject => "unknown_subject",
            Drop::EmptyClaim => "empty_claim",
            Drop::NoEvidence => "no_evidence",
            Drop::EmptyReason => "empty_reason",
            Drop::UnverifiablePhrase => "unverifiable_phrase",
            Drop::EvidenceNotFromInput => "evidence_not_from_input",
            Drop::BadConfidence => "bad_confidence",
            Drop::NoDecidedBy => "no_decided_by",
            Drop::MissingName => "missing_name",
            Drop::NameEqualsTable => "name_equals_table",
            Drop::MissingDecision => "missing_decision",
            Drop::MissingCategory => "missing_category",
            Drop::BadCategory => "bad_category",
            Drop::ShortReason => "short_reason",
            Drop::DuplicateSubject => "duplicate_subject",
            Drop::Overlap => "overlap",
            Drop::NoMembers => "no_members",
            Drop::CustomerGate => "customer_gate",
            Drop::ResponseParse => "response_parse",
            Drop::NoUsage => "no_usage",
            Drop::Truncated => "truncated",
            Drop::CallFailed => "call_failed",
        }
    }

    /// 人读一句话（报告与样例用）。
    pub fn label(self) -> &'static str {
        match self {
            Drop::NoSubject => "缺对象标识",
            Drop::UnknownSubject => "对象不在本次输入里",
            Drop::EmptyClaim => "claim 是空的",
            Drop::NoEvidence => "没有 evidence（不可评审）",
            Drop::EmptyReason => "reason 是空的",
            Drop::UnverifiablePhrase => "理由里有不可核实的措辞",
            Drop::EvidenceNotFromInput => "evidence 在输入里找不到落点",
            Drop::BadConfidence => "confidence 缺失或越界",
            Drop::NoDecidedBy => "decided_by 是空的",
            Drop::MissingName => "名字缺失",
            Drop::NameEqualsTable => "名字与表名相同",
            Drop::MissingDecision => "该不该进本体没有显式判断",
            Drop::MissingCategory => "排除项缺 category",
            Drop::BadCategory => "category 不在七类枚举内",
            Drop::ShortReason => "排除理由太短（业务读不懂）",
            Drop::DuplicateSubject => "同一个对象出现了两次",
            Drop::Overlap => "分组成员与已接受的组重叠",
            Drop::NoMembers => "组成员一个都不在实体集合里",
            Drop::CustomerGate => "组字段撞客户发布闸",
            Drop::ResponseParse => "响应解析失败",
            Drop::NoUsage => "响应没有 usage（无法记账）",
            Drop::Truncated => "正文为空 / 被截断",
            Drop::CallFailed => "模型调用失败",
        }
    }
}

/// **公共校验 + 构造**：三条命令唯一的提案构造入口。
///
/// 只判「每条提案都该满足」的五件事；命令特有的校验（表名在不在本批、
/// category 合不合法、成员在不在实体集合）在各自模块里追加。
///
/// ⚠️ `reviewed_by` 在这里**恒写 `None`** —— P2 的结构性落点。
pub fn finalize_item(
    raw: &RawProposal,
    decided_by: &str,
) -> std::result::Result<ProposalItem, Drop> {
    if decided_by.trim().is_empty() {
        return Err(Drop::NoDecidedBy);
    }
    let subject = raw.subject.trim();
    if subject.is_empty() {
        return Err(Drop::NoSubject);
    }
    let claim = raw.claim.trim();
    if claim.is_empty() {
        return Err(Drop::EmptyClaim);
    }
    let evidence: Vec<String> = raw
        .evidence
        .iter()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect();
    if evidence.is_empty() {
        return Err(Drop::NoEvidence);
    }
    let reason = raw.reason.trim();
    if reason.is_empty() {
        return Err(Drop::EmptyReason);
    }
    let confidence = match raw.confidence {
        Some(c) if c.is_finite() && (0.0..=1.0).contains(&c) => c,
        _ => return Err(Drop::BadConfidence),
    };
    // AC-009：不可核实的措辞，证据与理由**两处都查**。
    if evidence.iter().any(|e| has_unverifiable_marker(e)) || has_unverifiable_marker(reason) {
        return Err(Drop::UnverifiablePhrase);
    }
    Ok(ProposalItem {
        subject: subject.to_string(),
        claim: claim.to_string(),
        evidence,
        confidence,
        reason: reason.to_string(),
        decided_by: decided_by.trim().to_string(),
        reviewed_by: None,
        low_confidence: confidence < LOW_CONFIDENCE_THRESHOLD,
    })
}

/// 是否含不可核实措辞（大小写不敏感；中文不受影响）。
pub fn has_unverifiable_marker(text: &str) -> bool {
    let lower = text.to_lowercase();
    UNVERIFIABLE_MARKERS
        .iter()
        .any(|m| lower.contains(&m.to_lowercase()))
}

// ──────────────────────────────────────────────────────────
// 证据的可核实性（AC-009）
// ──────────────────────────────────────────────────────────

/// 一条 evidence 要能对上的输入面：对象名 / 列名 / 注释。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvidenceTokens {
    /// 表名（或其它对象标识）。
    pub subject: String,
    /// 列名。
    pub columns: Vec<String>,
    /// 表注释 + 列注释（原文）。
    pub comments: Vec<String>,
}

impl EvidenceTokens {
    pub fn new(
        subject: impl Into<String>,
        columns: impl IntoIterator<Item = String>,
        comments: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            columns: columns.into_iter().filter(|c| !c.trim().is_empty()).collect(),
            comments: comments
                .into_iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect(),
        }
    }
}

/// 至少一条 evidence 能在输入里找到落点。
///
/// 落点只有三种：对象名、列名、注释里的**连续 4 个非空白字符**。
/// 为什么注释用「4 字窗口」而不是整句包含：模型会摘半句（「用户套餐订阅」），
/// 要求整句等于逼它抄长句；而 4 字窗口已经足够让人**回到原文核实**这半句。
/// ⛔ 不做编辑距离 / 不做同义改写匹配 —— 一旦允许「差不多」，
/// evidence 就从**证据**退化成**提示**（与 REQ-116 的证据闸同一条论证）。
pub fn evidence_references_input(evidence: &[String], tokens: &EvidenceTokens) -> bool {
    evidence.iter().any(|e| references(e, tokens))
}

/// 单条 evidence 是否可核实（[`evidence_references_input`] 的单条实现）。
pub fn references(evidence: &str, tokens: &EvidenceTokens) -> bool {
    let lower = evidence.to_lowercase();
    if lower.trim().is_empty() {
        return false;
    }
    if !tokens.subject.trim().is_empty()
        && contains_token(&lower, &tokens.subject.to_lowercase())
    {
        return true;
    }
    if tokens
        .columns
        .iter()
        .any(|c| !c.trim().is_empty() && contains_token(&lower, &c.to_lowercase()))
    {
        return true;
    }
    tokens
        .comments
        .iter()
        .any(|c| comment_window_hit(&lower, c))
}

/// ASCII 标识符按**词元边界**匹配：`id` 不该命中 `provider`。
fn contains_token(haystack_lower: &str, needle_lower: &str) -> bool {
    let needle = needle_lower.trim();
    if needle.is_empty() {
        return false;
    }
    let ascii_ident = needle
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ascii_ident {
        return haystack_lower.contains(needle);
    }
    let h: Vec<char> = haystack_lower.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.len() > h.len() {
        return false;
    }
    for start in 0..=(h.len() - n.len()) {
        if h[start..start + n.len()] != n[..] {
            continue;
        }
        let before_ok = start == 0 || !is_ident_char(h[start - 1]);
        let after_ok = start + n.len() == h.len() || !is_ident_char(h[start + n.len()]);
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 注释里连续 4 个非空白字符出现在证据里 ⇒ 这条证据引用了原文。
fn comment_window_hit(evidence_lower: &str, comment: &str) -> bool {
    let chars: Vec<char> = comment.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() < 4 {
        return false;
    }
    for w in chars.windows(4) {
        let s: String = w.iter().collect::<String>().to_lowercase();
        if evidence_lower.contains(&s) {
            return true;
        }
    }
    false
}

// ──────────────────────────────────────────────────────────
// 统计
// ──────────────────────────────────────────────────────────

/// 一轮提案的统计（**失败也要看得见**）。
///
/// `requested` 是**请求批次数**（预算闸按批停），`requested_items` 是送模型的候选对象数。
/// 两个数都在报告里：只看批次数会把「一批 12 张表」说成「只判了 1 条」。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProposalStats {
    pub requested: usize,
    pub requested_items: usize,
    pub proposed: usize,
    /// 低于阈值的条数（R4：照样输出，只是标了 `low_confidence`）。
    pub low_confidence: usize,
    /// 丢弃分类计数（键是 [`Drop::code`]，BTreeMap ⇒ 输出顺序确定）。
    pub rejected: BTreeMap<String, usize>,
    /// 丢弃样例（最多 8 条，报告里点名）。
    pub rejection_samples: Vec<String>,
    /// 缓存命中几批（**命中不调用**）。
    pub cache_hits: usize,
    /// 真正调用模型几次。
    pub calls: usize,
    pub failed_calls: usize,
    /// **本轮**花掉的 token（命中缓存的批次不计 —— 那是上一次的钱）。
    pub tokens_spent: u64,
    /// 命中缓存里**历史用量**之和（溯源用，不进本轮预算）。
    pub tokens_replayed: u64,
    pub budget: u64,
    /// 因预算停止：停在第几批（1-based）。
    pub budget_stopped_at: Option<usize>,
}

impl ProposalStats {
    /// 丢弃总数。
    pub fn rejected_total(&self) -> usize {
        self.rejected.values().sum()
    }

    /// 预算是否已被越过（装配层据此**不出产物**）。
    ///
    /// 两种越过方式都算：还有批次没跑（`budget_stopped_at` 有值），
    /// 或全部跑完但累计用量已经超过上限（最后一次调用把预算顶过去了）。
    pub fn over_budget(&self) -> bool {
        self.budget_stopped_at.is_some() || self.tokens_spent > self.budget
    }

    /// 缓存命中率 = 命中批次数 / 请求批次数（分母 0 ⇒ `None`）。
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.requested == 0 {
            None
        } else {
            Some(self.cache_hits as f64 / self.requested as f64)
        }
    }

    /// 记一次丢弃（分类计数 + 留一条样例）。
    pub fn reject(&mut self, why: Drop, detail: String) {
        *self.rejected.entry(why.code().to_string()).or_insert(0) += 1;
        if self.rejection_samples.len() < 8 {
            self.rejection_samples
                .push(format!("{}：{}", why.label(), detail));
        }
    }

    /// 把已校验条目并入统计（`proposed` / `low_confidence`）。
    pub fn count_accepted(&mut self, items: &[ProposalItem]) {
        self.proposed += items.len();
        self.low_confidence += items.iter().filter(|i| i.is_low_confidence()).count();
    }

    /// 进**文件**的那部分统计（**只含内容属性**）。
    ///
    /// # 为什么 token / 缓存命中数**不进文件**
    ///
    /// AC-005 判的是「同一输入第二次运行 ⇒ 输出文件**逐字节相同**」。
    /// `tokens_spent` / `calls` / `cache_hits` / `tokens_replayed` 描述的是**这一次运行
    /// 花了多少钱**，它们**必然**逐次不同（第二次全是 0 / 命中）—— 把它们写进文件，
    /// 这条判据就永远不可能成立，而唯一的「修法」是让测试别比字节，那正是纪律 G 里
    /// 「去掉缓存」那一类变异要抓的东西。
    ///
    /// 真正属于提案内容的是：判了几批 / 提了几条 / 丢了几条、按什么原因丢。
    /// 这些在缓存回放时会被**重新推导一遍**（缓存存的是回答原文，不是结论），
    /// 所以它们逐字节稳定。运行经济学只出现在 stderr 报告里。
    pub fn content_json(&self) -> serde_json::Value {
        serde_json::json!({
            "requested": self.requested,
            "requested_items": self.requested_items,
            "proposed": self.proposed,
            "low_confidence": self.low_confidence,
            "rejected": self.rejected,
            "rejected_total": self.rejected_total(),
            "rejection_samples": self.rejection_samples,
        })
    }
}

// ──────────────────────────────────────────────────────────
// 请求与指纹（P3）
// ──────────────────────────────────────────────────────────

/// 一次模型请求 = 一批候选对象。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRequest {
    /// 人读标识（报告 / 样例里的「第几批 / 哪张表」）。
    pub key: String,
    /// 这一批送了几条候选（只用于记账）。
    pub item_count: usize,
    /// 输入本身的规范化指纹（`sha256`）—— **进指纹**，所以输入变了缓存必失效。
    pub input_digest: String,
    /// user 消息正文（**紧凑序列化**：同输入逐字节相同）。
    pub user_message: String,
}

impl BatchRequest {
    /// **指纹**（ADR-005 P3 / demand「指纹缓存」）：
    /// `sha256(v1 + key + input_digest + prompt_sha + model + agent_version)`。
    ///
    /// 四项缺一不可：
    /// - `input_digest`：两条不同输入不能共用一份缓存；
    /// - `prompt_sha`：**改提示词却复用旧结果**是本仓点过名的病（REQ-109 R1）；
    /// - `model`：换模型仍复用旧结果 ⇒ 供应商漂移无声；
    /// - `agent_version`：解析/校验逻辑改了仍复用旧结果。
    pub fn fingerprint(&self, prompt_sha: &str, model: &str, agent_version: &str) -> String {
        let material = format!(
            "{FINGERPRINT_VERSION}\n{}\n{}\n{}\n{}\n{}\n",
            self.key, self.input_digest, prompt_sha, model, agent_version
        );
        sha256_hex(material.as_bytes())
    }
}

// ──────────────────────────────────────────────────────────
// 编排 + 预算（三命令共用）
// ──────────────────────────────────────────────────────────

/// 跑一批请求：**缓存命中绝不调用**、先记账再解析、越预算即停。
///
/// `consume(index, 回答正文, stats)` 是各命令的**解析 + 校验**回调：
/// 返回 `Ok(items)` 表示这份回答按协议解析成功（条目可能全是 0 条），
/// `Err(detail)` 表示**整份回答**作废（记 `response_parse`，且照样缓存 —— 见下）。
///
/// 返回**各命令自己的条目类型**：提案的公共形状由 [`finalize_item`] 保证，
/// 但名字 / 排除类别 / 分组成员是各命令的机器字段，不能被压进一个共同结构体
/// （那会让「ADFIN 六字段」变成七个字段，且每个命令都得带两个用不上的 `Option`）。
/// 纪律 H 守的是**形状与校验只有一处**，不是「所有命令返回同一个 Rust 类型」。
///
/// # 为什么作废的回答也要缓存
///
/// 「同输入 ⇒ 同一份回答，不重复付费」是缓存的存在理由。若只缓存合规回答，
/// 同一份被协议作废的回答每次重跑都会**再付一次钱**，而拿到的还是同一份不合规回答
/// —— 于是「第二次运行 0 token」对最贵的那几条恰好不成立（与 REQ-109 / REQ-116 同一条论证）。
///
/// # 不缓存的三类
///
/// 传输失败 / 无 usage / 正文截断：这三样是**环境或调用参数**的属性，不是输入的属性。
/// 缓存它们会让「调大 max_tokens 再跑」变成空操作。
pub fn run_batches<T, F>(
    transport: &mut dyn LlmTransport,
    cache: &mut dyn ProposalCache,
    reqs: &[BatchRequest],
    model: &str,
    prompts: &Prompts,
    agent_version: &str,
    budget: u64,
    max_tokens: u32,
    mut consume: F,
) -> (Vec<T>, ProposalStats)
where
    F: FnMut(usize, &str, &mut ProposalStats) -> std::result::Result<Vec<T>, String>,
{
    let mut items: Vec<T> = Vec::new();
    let mut stats = ProposalStats {
        requested: reqs.len(),
        requested_items: reqs.iter().map(|r| r.item_count).sum(),
        budget,
        ..Default::default()
    };

    for (i, req) in reqs.iter().enumerate() {
        // ── 预算闸：**在发起下一次之前**判 ──
        if stats.tokens_spent >= budget {
            stats.budget_stopped_at = Some(i + 1);
            break;
        }

        let fp = req.fingerprint(&prompts.sha256, model, agent_version);

        // ── 缓存：命中**绝不调用** ──
        if let Some(entry) = cache.get(&fp) {
            stats.cache_hits += 1;
            stats.tokens_replayed += entry.tokens;
            match consume(i, &entry.response, &mut stats) {
                Ok(mut got) => items.append(&mut got),
                Err(detail) => stats.reject(
                    Drop::ResponseParse,
                    format!("{}（缓存回放）{detail}", req.key),
                ),
            }
            continue;
        }

        let chat = ChatRequest {
            model: model.to_string(),
            messages: vec![
                Message::system(prompts.system.clone()),
                Message::user(req.user_message.clone()),
            ],
            max_tokens,
        };
        stats.calls += 1;
        let resp = match transport.complete(&chat) {
            Ok(r) => r,
            Err(e) => {
                stats.failed_calls += 1;
                stats.reject(Drop::CallFailed, format!("{}：{e:#}", req.key));
                continue;
            }
        };

        // ── 先记账，再解析（钱已经花了）──
        let Some(usage) = resp.usage.as_ref() else {
            stats.failed_calls += 1;
            stats.reject(
                Drop::NoUsage,
                format!("{} 的响应没有 usage —— 无法记账，这一批作废", req.key),
            );
            continue;
        };
        let tokens = usage.total_tokens();
        stats.tokens_spent += tokens;

        let text = match completion_text(&resp) {
            Ok(t) => t,
            Err(e) => {
                stats.failed_calls += 1;
                stats.reject(Drop::Truncated, format!("{} 正文不可用：{e:#}", req.key));
                continue;
            }
        };

        // 解析不了 / 校验不过的回答**也写缓存**（同一份回答不该每次重跑再付一次钱）。
        cache.put(
            &fp,
            CachedResponse {
                response: text.clone(),
                model: model.to_string(),
                prompt_sha: prompts.sha256.clone(),
                tokens,
            },
        );
        match consume(i, &text, &mut stats) {
            Ok(mut got) => items.append(&mut got),
            Err(detail) => stats.reject(Drop::ResponseParse, format!("{} {detail}", req.key)),
        }
    }

    (items, stats)
}

/// 提案的**稳定排序**：按 `subject`，再按 `claim`。
///
/// 排序不是为了好看：同一份输入两次运行必须**逐字节相同**（AC-005），
/// 而「批的顺序」由输入决定、「批内顺序」由模型决定 —— 只有排一次序，
/// 输出的字节才不再依赖模型的措辞顺序。
pub fn sort_items(items: &mut [ProposalItem]) {
    items.sort_by(|a, b| {
        a.subject
            .cmp(&b.subject)
            .then_with(|| a.claim.cmp(&b.claim))
    });
}

// ──────────────────────────────────────────────────────────
// 渲染（P1）
// ──────────────────────────────────────────────────────────

/// 提案文件的公共头部（三命令共用）。`extra` 里放各命令的兼容字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalHeader {
    /// `name` / `exclude` / `concept`。
    pub kind: &'static str,
    /// `ai` / `rule`（AC-007）。
    pub source: &'static str,
    /// 调用的模型；`rule` 路径为 `None`（⛔ 不写一个没跑过的模型名）。
    pub model: Option<String>,
    /// 提示词指纹；`rule` 路径为 `None`。
    pub prompt_sha: Option<String>,
    /// 抽取/提案逻辑版本（进指纹）。
    pub agent_version: String,
    /// 输入的规范化指纹（可 diff、可复现的锚点）。
    pub input_sha256: String,
    /// P4：`ai:<model>` 或 `rule`。
    pub decided_by: String,
}

/// 把提案渲染成**磁盘形状**（pretty JSON + 末尾换行）。
///
/// key 顺序：`serde_json::Map` 在未开 `preserve_order` 时是 `BTreeMap`（按键排序），
/// 所以两次调用逐字节相同**不依赖**插入顺序 —— 这是 AC-005 的一半。
/// `extra` 里的同名字段**覆盖**公共字段（`exclude` 的 `reviewed_by` / `source`
/// 与这里相同值，覆盖是幂等的）。
pub fn render_document(
    header: &ProposalHeader,
    items: &[ProposalItem],
    stats: &ProposalStats,
    extra: Map<String, Value>,
) -> Result<String> {
    let mut doc = Map::new();
    doc.insert("proposal_format".to_string(), Value::from(PROPOSAL_FORMAT));
    doc.insert("kind".to_string(), Value::from(header.kind));
    doc.insert("source".to_string(), Value::from(header.source));
    doc.insert(
        "model".to_string(),
        match &header.model {
            Some(m) => Value::from(m.clone()),
            None => Value::Null,
        },
    );
    doc.insert(
        "prompt_sha".to_string(),
        match &header.prompt_sha {
            Some(s) => Value::from(s.clone()),
            None => Value::Null,
        },
    );
    doc.insert(
        "agent_version".to_string(),
        Value::from(header.agent_version.clone()),
    );
    doc.insert(
        "input_sha256".to_string(),
        Value::from(header.input_sha256.clone()),
    );
    doc.insert("decided_by".to_string(), Value::from(header.decided_by.clone()));
    // P2：**恒有键、恒 null**。不是 `skip_serializing_if` —— 那一栏缺席与
    // 「没人签过」在评审界面上是两件事，文件里必须长得不一样。
    doc.insert("reviewed_by".to_string(), Value::Null);
    doc.insert(
        "proposals".to_string(),
        serde_json::to_value(items)?,
    );
    doc.insert("stats".to_string(), stats.content_json());
    for (k, v) in extra {
        doc.insert(k, v);
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(doc))?;
    text.push('\n');
    Ok(text)
}

/// 输入的规范化指纹（通用工具）：对一组**可序列化**的项按字符串排序后哈希。
///
/// 排序保证「同一份输入换个下标顺序 ⇒ 同一个指纹」（与 `ai_semantic::schema_digest`
/// 同一条理由：`pg_catalog` 的返回顺序会变，输入指纹不该跟着变）。
pub fn digest_of<T: Serialize>(items: &[T]) -> Result<String> {
    let mut parts: Vec<String> = Vec::with_capacity(items.len());
    for it in items {
        parts.push(serde_json::to_string(it)?);
    }
    parts.sort();
    Ok(sha256_hex(parts.join("\n").as_bytes()))
}

/// `sha256(text)` 的便捷包装（各命令写 `input_sha256` 用；不另立实现）。
pub fn sha256_text(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

/// 便于测试/报告：置信度分箱（与 REQ-108 的校准分箱同一边界，不另立一套）。
///
/// 返回 `[(区间文案, 条数)]`，区间顺序固定：`0.00–0.30 / 0.30–0.60 / 0.60–0.75 /
/// 0.75–0.90 / 0.90–1.00`。**空集也返回五行**（分母 0 要显示出来，不是 `—`）。
pub fn confidence_histogram(values: &[f64]) -> Vec<(&'static str, usize)> {
    let bins: [(&'static str, f64, f64); 5] = [
        ("0.00–0.30", 0.0, 0.30),
        ("0.30–0.60", 0.30, 0.60),
        ("0.60–0.75", 0.60, 0.75),
        ("0.75–0.90", 0.75, 0.90),
        ("0.90–1.00", 0.90, 1.0),
    ];
    let mut out: Vec<(&'static str, usize)> = Vec::with_capacity(5);
    for (label, lo, hi) in bins {
        let n = values
            .iter()
            .filter(|v| {
                // 最后一个箱含上界 1.0；其余左闭右开（0.30 落在第二箱）。
                if (hi - 1.0).abs() < f64::EPSILON {
                    **v >= lo && **v <= hi
                } else {
                    **v >= lo && **v < hi
                }
            })
            .count();
        out.push((label, n));
    }
    out
}

/// 分箱的一行报告（三条命令同一句话）。
pub fn confidence_histogram_line(values: &[f64]) -> String {
    confidence_histogram(values)
        .into_iter()
        .map(|(label, n)| format!("{label}: {n}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 预算必备条件的**转调**（唯一判据在 `doc_ingest::require_budget`）。
///
/// 存在的理由：三条命令都要在**干活之前**把「没给预算」变成一条点名报错，
/// 而这条规则的实现只该有一份。`rule` 路径（算法分组）不受预算约束。
pub fn require_proposal_budget(budget: Option<u64>) -> Result<u64> {
    require_budget(budget)
}

/// 读一个本地指纹缓存文件；不存在 ⇒ 空缓存（**不是错误**）。
///
/// 坏缓存 ⇒ **报错**（不静默重建：那会把「缓存坏了」变成一次花钱的正常运行）。
pub fn load_cache_file(path: &std::path::Path) -> Result<MemoryProposalCache> {
    match std::fs::read_to_string(path) {
        Ok(text) => cache_from_json(&text)
            .map_err(|e| anyhow::anyhow!("指纹缓存读不懂：{}：{e:#}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MemoryProposalCache::new()),
        Err(e) => Err(anyhow::anyhow!(
            "读不到指纹缓存 {}：{e}",
            path.display()
        )),
    }
}

/// 预算超限时的**唯一报错文案**（三条命令同一句，⛔ 不各写一份）。
pub fn over_budget_error(
    what: &str,
    stats: &ProposalStats,
    budget: u64,
    cache_path: &std::path::Path,
) -> anyhow::Error {
    let where_ = match stats.budget_stopped_at {
        Some(n) => format!("停在第 {n} 批（共 {} 批）", stats.requested),
        None => format!(
            "{} 批已全部跑完，但累计用量越过了上限",
            stats.requested
        ),
    };
    anyhow::anyhow!(
        "{what} 预算超限 —— 本轮**不产出任何提案文件**（AC-006：不产半份文件）。\n\
         \n\
         已用 / 上限 / 停在第几项：{} / {} token / {where_}。\n\
         \n\
         为什么越预算时一个产物都不写：提案文件的契约里没有「这只是前 K 批」这一栏，\
         一份半份的提案与完整的那份**长得一模一样** —— 评审人会把残缺当全量签字。\n\
         已经花掉的那些回答**已经写进本地缓存**（{}），放宽预算重跑会直接命中、\
         不重复计费。",
        stats.tokens_spent,
        budget,
        cache_path.display()
    )
}

/// 预算闸的**唯一装配点**：越预算 ⇒ 报错（调用方在写产物**之前**调它）。
pub fn ensure_within_budget(
    what: &str,
    stats: &ProposalStats,
    budget: u64,
    cache_path: &std::path::Path,
) -> Result<()> {
    if stats.over_budget() {
        return Err(over_budget_error(what, stats, budget, cache_path));
    }
    Ok(())
}

/// `--budget-tokens` 省略时的提示行（三命令同一句）。
pub fn default_budget_notice(budget: u64) -> String {
    format!("未给 --budget-tokens，用默认上限 {budget}（超过即停，且不产出提案文件）")
}

/// `--out` 必填（P1）的唯一报错文案。
pub fn missing_out_error(kind: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "缺 --out：{kind} 的提案**必须落盘**（ADR-005 P1：可 diff、可评审、可回滚）。\n\
         ⛔ 不提供「只打到 stdout」的模式：一份没落盘的提案进不了评审记录，\n\
         而「谁签过字」这件事必须能回到一个具体文件。"
    )
}

/// 文件名形状检查：`--out` 给的是目录而不是文件时，报一句人话。
pub fn require_out_is_file(path: &std::path::Path) -> Result<()> {
    if path.is_dir() {
        bail!(
            "--out 指向的是一个**目录**（{}）。提案是一个文件，不是目录 —— \
             请给一个文件名，例如 out/names.proposal.json。",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::protocol::{Choice, Usage};
    use crate::agent::llm::{ChatResponse, ScriptedTransport};

    fn raw(subject: &str, claim: &str, ev: &[&str], conf: f64, reason: &str) -> RawProposal {
        RawProposal {
            subject: subject.to_string(),
            claim: claim.to_string(),
            evidence: ev.iter().map(|s| s.to_string()).collect(),
            confidence: Some(conf),
            reason: reason.to_string(),
            ..Default::default()
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

    fn req(key: &str, message: &str) -> BatchRequest {
        BatchRequest {
            key: key.to_string(),
            item_count: 1,
            input_digest: sha256_text(message),
            user_message: message.to_string(),
        }
    }

    // ── 公共校验 ──────────────────────────────────────────

    #[test]
    fn finalize_requires_evidence_and_reason_and_counts_drops() {
        let mut stats = ProposalStats::default();
        // 缺 evidence ⇒ 丢弃
        match finalize_item(&raw("t", "是甲", &[], 0.9, "理由"), "ai:m") {
            Err(Drop::NoEvidence) => stats.reject(Drop::NoEvidence, "t".into()),
            other => panic!("缺 evidence 必须丢：{other:?}"),
        }
        // 缺 reason ⇒ 丢弃
        match finalize_item(&raw("t", "是甲", &["表名 t"], 0.9, "  "), "ai:m") {
            Err(Drop::EmptyReason) => stats.reject(Drop::EmptyReason, "t".into()),
            other => panic!("缺 reason 必须丢：{other:?}"),
        }
        assert_eq!(stats.rejected_total(), 2);
        assert_eq!(stats.rejected.get("no_evidence").copied(), Some(1));
        assert_eq!(stats.rejected.get("empty_reason").copied(), Some(1));
    }

    #[test]
    fn finalize_rejects_unverifiable_wording() {
        let r = raw("t", "是甲", &["模型认为这像客户表"], 0.9, "理由");
        assert_eq!(finalize_item(&r, "ai:m"), Err(Drop::UnverifiablePhrase));
        let r = raw("t", "是甲", &["表名 t"], 0.9, "AI 认为它像客户");
        assert_eq!(finalize_item(&r, "ai:m"), Err(Drop::UnverifiablePhrase));
        // ⛔ 反向：诚实的不确定措辞**不能**被这条闸误杀
        let r = raw("t", "是甲", &["表名 t"], 0.5, "不能确定，注释为空");
        assert!(finalize_item(&r, "ai:m").is_ok(), "「不能确定」是诚实表达，不许当不可核实");
        // ⛔ 反向：业务域里的「大语言模型」不是模型自证（实测踩过：
        // `customer_tpm_rate_limit` 的合法理由被过宽的 marker 误杀）
        let r = raw(
            "customer_tpm_rate_limit",
            "客户模型限流配置",
            &["表注释：客户模型级TPM限流配置表"],
            0.95,
            "表注释写明 TPM 限流配置，针对大语言模型的每分钟令牌数",
        );
        assert!(
            finalize_item(&r, "ai:m").is_ok(),
            "业务名词「大语言模型」不许被当成不可核实措辞"
        );
    }

    #[test]
    fn low_confidence_is_marked_not_dropped() {
        let r = raw("t", "是甲", &["表名 t"], 0.42, "注释为空");
        let item = finalize_item(&r, "ai:m").expect("低置信照样通过");
        assert!(item.is_low_confidence(), "R4：低置信要标，不许丢");
        assert_eq!(item.confidence, 0.42);
        let r = raw("t", "是甲", &["表名 t"], 0.6, "注释与列名一致");
        let item = finalize_item(&r, "ai:m").expect("阈值上沿");
        assert!(!item.is_low_confidence(), "0.6 不算低（阈值是 < 0.6）");
    }

    #[test]
    fn bad_confidence_is_dropped_not_guessed() {
        let r = raw("t", "是甲", &["表名 t"], 1.5, "理由");
        assert_eq!(finalize_item(&r, "ai:m"), Err(Drop::BadConfidence));
        let r = RawProposal {
            subject: "t".into(),
            claim: "是甲".into(),
            evidence: vec!["表名 t".into()],
            confidence: None,
            reason: "理由".into(),
            ..Default::default()
        };
        assert_eq!(finalize_item(&r, "ai:m"), Err(Drop::BadConfidence));
    }

    /// P2：`reviewed_by` 恒为 null，且**没有**公开构造器能写它。
    #[test]
    fn reviewed_by_is_always_null_on_the_production_path() {
        let item = finalize_item(&raw("t", "是甲", &["表名 t"], 0.9, "理由"), "ai:m").unwrap();
        assert_eq!(item.reviewed_by(), None, "提案永不权威（P2）");
        let v = serde_json::to_value(&item).unwrap();
        assert_eq!(v["reviewed_by"], Value::Null, "字段必须存在且是 null");
        assert_eq!(v["decided_by"], "ai:m", "P4：谁提的必须可追");
    }

    /// P4：`decided_by` 两种来源的拼法。
    #[test]
    fn decided_by_distinguishes_ai_from_rule() {
        assert_eq!(decided_by_ai("qwen36-27B"), "ai:qwen36-27B");
        assert_eq!(RULE_DECIDED_BY, "rule");
        assert_ne!(decided_by_ai("x"), RULE_DECIDED_BY);
    }

    // ── 协议解析 ──────────────────────────────────────────

    #[test]
    fn parse_proposals_strips_fence_and_reports_malformed_items() {
        let text = "```json\n{\"proposals\":[{\"subject\":\"a\",\"claim\":\"是甲\",\
                    \"evidence\":[\"表名 a\"],\"confidence\":0.9,\"reason\":\"理由\"},\
                    {\"subject\":123}]}\n```";
        let parsed = parse_proposals(text).expect("合法 JSON");
        assert_eq!(parsed.proposals.len(), 1);
        assert_eq!(parsed.malformed.len(), 1, "单条形状不合法要计数，不许整份作废");
        assert!(parse_proposals("不是 JSON").is_err());
        assert!(parse_proposals("{\"x\":[]}").is_err(), "缺 proposals ⇒ 整份作废");
    }

    // ── 证据可核实性（AC-009）────────────────────────────

    fn tokens() -> EvidenceTokens {
        EvidenceTokens::new(
            "customer_subscriptions",
            vec!["customer_id".into(), "id".into()],
            vec!["用户套餐订阅表".into(), "客户标识".into()],
        )
    }

    #[test]
    fn evidence_must_touch_the_input() {
        assert!(references("表名 customer_subscriptions", &tokens()));
        assert!(references("列 customer_id 指向 customers", &tokens()));
        assert!(references("表注释写「用户套餐订阅」", &tokens()), "注释 4 字窗口");
        assert!(
            !references("模型觉得这是一张客户表", &tokens()),
            "没有任何输入落点的证据不算证据"
        );
        // `id` 不能命中 `provider`（词元边界）
        assert!(
            !references("列 provider 说明这是供应商", &tokens()),
            "id 不该在 provider 里被当成列名命中"
        );
    }

    // ── 缓存 / 预算（P3 / AC-005 / AC-006）───────────────

    fn one_answer_proposals() -> String {
        "{\"proposals\":[{\"subject\":\"t1\",\"claim\":\"是甲\",\"evidence\":[\"表名 t1\"],\
         \"confidence\":0.9,\"reason\":\"注释与列名一致\"}]}"
            .to_string()
    }

    #[test]
    fn second_run_hits_cache_zero_tokens_and_same_bytes() {
        let answer = one_answer_proposals();
        let mut transport = ScriptedTransport::new(vec![Ok(reply(&answer, 100))]);
        let mut cache = MemoryProposalCache::new();
        let prompts = Prompts::from_system("系统提示");
        let reqs = vec![req("batch-1", "{\"tables\":[\"t1\"]}")];

        let consume = |_i: usize, body: &str, stats: &mut ProposalStats| {
            let parsed = parse_proposals(body)?;
            let mut out = Vec::new();
            for r in parsed.proposals {
                match finalize_item(&r, "ai:m") {
                    Ok(item) => out.push(item),
                    Err(d) => stats.reject(d, "x".into()),
                }
            }
            Ok(out)
        };
        let (items1, s1) = run_batches(
            &mut transport, &mut cache, &reqs, "m", &prompts, "v1", 1000, 300, consume,
        );
        let header = ProposalHeader {
            kind: "name",
            source: SOURCE_AI,
            model: Some("m".into()),
            prompt_sha: Some(prompts.sha256.clone()),
            agent_version: "v1".into(),
            input_sha256: sha256_text("input"),
            decided_by: decided_by_ai("m"),
        };
        let bytes1 = render_document(&header, &items1, &s1, Map::new()).unwrap();

        // 第二次：**同一份回答脚本已经用完** —— 命中缓存就不该再要一次回答。
        let (items2, s2) = run_batches(
            &mut transport, &mut cache, &reqs, "m", &prompts, "v1", 1000, 300, consume,
        );
        assert_eq!(s1.tokens_spent, 100);
        assert_eq!(s2.tokens_spent, 0, "第二次运行必须 0 token（P3）");
        assert_eq!(s2.calls, 0, "命中缓存 ⇒ 一次调用都不许发");
        assert_eq!(s2.cache_hits, 1);
        assert_eq!(s2.tokens_replayed, 100, "回放的是历史用量，不进本轮预算");
        let bytes2 = render_document(&header, &items2, &s2, Map::new()).unwrap();
        assert_eq!(bytes1, bytes2, "第二次输出必须逐字节相同 —— 这里连 stats 也相同");
    }

    #[test]
    fn prompt_model_or_version_change_invalidates_cache() {
        let answer = one_answer_proposals();
        let fp = req("b", "同一份输入").fingerprint("psha1", "m1", "v1");
        assert_ne!(fp, req("b", "同一份输入").fingerprint("psha2", "m1", "v1"));
        assert_ne!(fp, req("b", "同一份输入").fingerprint("psha1", "m2", "v1"));
        assert_ne!(fp, req("b", "同一份输入").fingerprint("psha1", "m1", "v2"));
        assert_ne!(fp, req("b", "别的输入").fingerprint("psha1", "m1", "v1"));
        let _ = answer;
    }

    #[test]
    fn budget_one_stops_and_never_produces_half_a_result() {
        let answer = one_answer_proposals();
        let mut transport = ScriptedTransport::new(vec![Ok(reply(&answer, 100))]);
        let mut cache = MemoryProposalCache::new();
        let prompts = Prompts::from_system("系统提示");
        let reqs = vec![req("b1", "一"), req("b2", "二")];
        let consume = |_i: usize, _body: &str, _s: &mut ProposalStats| Ok(Vec::<ProposalItem>::new());
        let (_, stats) = run_batches(
            &mut transport, &mut cache, &reqs, "m", &prompts, "v1", 1, 300, consume,
        );
        assert_eq!(stats.tokens_spent, 100, "越界那一笔照样记账（钱已经花了）");
        assert!(stats.budget_stopped_at.is_some(), "必须记下停在第几批");
        assert!(stats.over_budget(), "越预算 ⇒ 装配层不许出产物");
    }

    /// AC-006 的另一半：越预算时**一个产物文件都不写**（由 `ensure_within_budget` 报错）。
    #[test]
    fn over_budget_gate_returns_error() {
        let stats = ProposalStats {
            requested: 2,
            tokens_spent: 100,
            budget: 1,
            budget_stopped_at: Some(2),
            ..Default::default()
        };
        let err = ensure_within_budget(
            "name suggest",
            &stats,
            1,
            std::path::Path::new("out/x.cache.json"),
        )
        .expect_err("越预算必须 Err");
        let msg = format!("{err:#}");
        assert!(msg.contains("100"), "必须报已用");
        assert!(msg.contains("停在第 2 批"), "必须报停在第几项");
        assert!(msg.contains("不产出任何提案文件"));
    }

    /// AC-005 的**结构前提**：运行经济学不得进文件。
    ///
    /// 变异「把 `stats` 换回整个结构体」会让第二次运行的 `cache_hits` / `tokens_spent`
    /// 与第一次不同 ⇒ 逐字节判据红。这条测试把那个变异钉在**文件形状**上，
    /// 而不是只靠集成测试比字节。
    #[test]
    fn file_stats_carry_no_run_economics() {
        let stats = ProposalStats {
            requested: 8,
            requested_items: 96,
            proposed: 95,
            low_confidence: 2,
            rejected: BTreeMap::from([("no_evidence".to_string(), 1)]),
            rejection_samples: vec!["没有 evidence".to_string()],
            cache_hits: 0,
            calls: 8,
            failed_calls: 0,
            tokens_spent: 41100,
            tokens_replayed: 0,
            budget: 200_000,
            budget_stopped_at: None,
        };
        let v = stats.content_json();
        for banned in [
            "tokens_spent",
            "tokens_replayed",
            "cache_hits",
            "calls",
            "failed_calls",
            "budget",
            "budget_stopped_at",
        ] {
            assert!(v.get(banned).is_none(), "运行经济学 `{banned}` 不该进提案文件");
        }
        assert_eq!(v["proposed"], 95);
        assert_eq!(v["rejected"]["no_evidence"], 1);
        assert_eq!(v["rejected_total"], 1);
    }

    /// R4 的分箱边界：左闭右开，只有最后一箱含 1.0。
    #[test]
    fn confidence_histogram_has_stable_boundaries() {
        let bins = confidence_histogram(&[0.0, 0.299, 0.30, 0.599, 0.60, 0.749, 0.75, 0.899, 0.90, 1.0]);
        assert_eq!(bins[0].1, 2, "0.0 与 0.299");
        assert_eq!(bins[1].1, 2, "0.30 与 0.599（0.30 落第二箱）");
        assert_eq!(bins[2].1, 2, "0.60 与 0.749");
        assert_eq!(bins[3].1, 2, "0.75 与 0.899");
        assert_eq!(bins[4].1, 2, "0.90 与 1.0（1.0 落最后一箱）");
        // 空集也返回五行（分母为 0 要显示出来）
        assert_eq!(confidence_histogram(&[]).len(), 5);
        assert!(confidence_histogram(&[]).iter().all(|(_, n)| *n == 0));
    }
}




