//! **AI 语义通道**（REQ-109）—— 指纹、缓存、调用编排、结果校验。
//!
//! 一句话：**AI 只回答一个很窄的问题——「这条候选关系在业务上说得通吗」**，
//! 并给出引用坐标；其余一切仍由规则与采样决定。
//!
//! # 它为什么不产结构
//!
//! 输出里有一个 `choice`（「你会选哪个目标列」），但**它不改变任何结构**：
//! 提案的 `from` / `to` 仍是规则抽出来的。`semantic_fit` 问的是
//! **规则那条关系**说得通吗 —— 设计概要开篇那句就是本题的唯一正确读法。
//! `choice` 只进 `ai` 溯源供人复核（协议要求它必须在候选集合内）。
//!
//! 于是「让 AI 改关系」这件事**在类型上做不到**：本模块产出的
//! [`Judgement`] 里没有表名/列名能写回提案，只有 `semantic_fit` 与审计字段。
//!
//! # 分层（照抄 `agent::llm`）
//!
//! | 层 | 职责 | I/O |
//! | --- | --- | --- |
//! | 本模块 | 指纹、协议校验、缓存抽象、编排、预算 | **零**（`LlmTransport` 是注入的 trait） |
//! | [`crate::db::ai_cache`] | `ai_cache` 表读写 | 有（PG） |
//! | [`crate::agent::llm::transport`] | 真 HTTP | 有 |
//!
//! 因此**所有 AC 都能用 `ScriptedTransport` 跑**（AC-011）：本模块一次网络都不打。
//!
//! # 指纹（R1：缺一项就是静默错）
//!
//! ```text
//! fingerprint = sha256_hex(
//!     "v1\n" + relation_key + "\n" + schema_digest + "\n"
//!   + prompt_sha256 + "\n" + model_id + "\n" + agent_version + "\n")
//! ```
//!
//! 六项的每一项都能单独说清「少了它会怎样」：
//!
//! | 项 | 少了它 | 
//! | --- | --- |
//! | `relation_key` | 两条不同关系共用一份缓存 ⇒ 答案张冠李戴 |
//! | `schema_digest` | 列/类型/注释改了仍命中旧答案 ⇒ 判断基于不存在的 schema |
//! | `prompt_sha256` | **改提示词却复用旧结果**（R1 点名的病） |
//! | `model_id` | 换模型仍复用旧结果 ⇒ 供应商漂移无声 |
//! | `agent_version` | 判定逻辑改了仍复用旧结果 |
//! | 版本前缀 `v1` | 指纹算法本身改了仍复用旧结果 |
//!
//! `schema_digest` 对「源表 + 目标表」的名称/类型/注释做**排序后**序列化再哈希
//! （排序保证确定性 —— 同一 schema 换个列顺序不该换指纹）。
//!
//! # 缓存语义（R2 / R3 / R6）
//!
//! | 情形 | 行为 |
//! | --- | --- |
//! | 命中 | 直接用，**绝不调用模型**；`cache_hit = true` |
//! | 未命中 | 调用 → 校验 → 写缓存；`cache_hit = false` |
//! | `--refresh` | 忽略缓存并**覆盖**；统计「强制刷新 N 条」 |
//! | 调用/校验失败 | 该条 `semantic_fit` **缺失**（不是 0）；计入失败数 |
//!
//! 「缺失不是 0」在评分卡那一侧由 `Option<f64>` 保证（见
//! `domain::confidence::score_with_semantic`）—— 本模块**只负责不产出 0**：
//! 失败的条目根本不会出现在返回的 map 里。
//!
//! # 预算（R5）
//!
//! - 每轮调用**先记账再解析**（与 `agent::llm` 同一条纪律：钱已经花了）；
//! - `tokens_used >= budget` ⇒ 停止，记 `budget_stopped`（停在第几条）并上报已用/上限；
//! - 单次调用可能**越过**预算一个调用的量 —— 调用前不知道要花多少，
//!   所以上限是「不再发起下一次」的闸，不是「保证不超」的闸（装配层据此不出产物）。
//!
//! # 凭据纪律
//!
//! key 只从环境变量来（[`crate::agent::llm::LlmConfig::from_env`]），
//! 本模块**不回显、不落盘、不打印**；[`LlmSettings::from_env`] 的报错只说**变量名**。

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::llm::protocol::Message;
use crate::agent::llm::{completion_text, ChatRequest, LlmConfig, LlmTransport};
use crate::domain::confidence::AiProvenance;
use crate::extract::sample::sha256_hex;

// ──────────────────────────────────────────────────────────
// 环境变量与提示词
// ──────────────────────────────────────────────────────────

/// LLM 端点（`https://…/v1`）。
///
/// 为什么它必须与 `agent::llm` 的 `API_KEY_ENV` / `MODEL_ENV` 一起被点名校验：
/// AC-002 要求「缺任一 ⇒ exit≠0 **并点名缺哪个变量**」，
/// 而 `LlmConfig::from_env` 只读 key/model（base_url 由调用方从登记取）。
/// 本需求没有登记表，端点只能来自环境变量 —— 于是三个变量在**同一个地方**校验，
/// 缺哪个就报哪个，不留「配了 key 却没配端点，跑到一半才 401/连不上」的缝。
pub const BASE_URL_ENV: &str = "ONTOK_LLM_BASE_URL";

/// 指纹算法的版本前缀。**改指纹构成 = 改这里**（否则旧缓存会被新算法误命中）。
pub const FINGERPRINT_VERSION: &str = "v1";

/// `rationale` 上限（设计概要 §4.2：1–60 **字**）。
pub const RATIONALE_MAX_CHARS: usize = 60;

/// 固定提示词模板 —— **改动会改变 `prompt_sha256`，从而让所有缓存失效**（R1）。
///
/// 模板里**不含任何本次输入**：关系与 schema 走 user 消息，
/// 所以模板哈希是「判定规则」的哈希，而输入变化由 `relation_key` / `schema_digest`
/// 承担。两条分开的理由：改 schema 不该让「提示词版本」变，改提示词必须让缓存全废。
pub const SYSTEM_PROMPT: &str = r#"你是数据库本体抽取流程里的语义判定器。你只做一件事：判断规则给出的一条候选关系在业务上是否说得通。

输入是一条 JSON：
{"relation": {"from": "源表.源列", "to": "目标表.目标列", "to_candidates": ["目标表的候选列", ...]},
 "schema_slice": [{"table": "表名", "columns": [{"name": "列名", "type": "类型", "comment": "注释或 null"}]}],
 "task": "..."}

判定规则：
1. semantic_fit 回答的是：relation.from → relation.to 这条关系，在业务上说得通吗？
   1.0 = 明显成立；0.5 = 说不清；0.0 = 明显错误。
2. choice 是你认为最合理的 (table, column)，table 必须是 relation.to 的那张表，column 必须在 to_candidates 里；无法判断就给 null。
   choice 只用于记录，不改变评分。如果目标列不对，semantic_fit 就应该低，不要为了表达「应该指向别处」而给高分。
3. cites 列出你据以判断的坐标，每项必须是 schema_slice 里真实存在的 {"table": ..., "column": ...}。
   不许编造表名或列名；没有可引用的坐标就给空数组 []。
4. rationale 是一句中文理由。**硬上限 60 个字符（含标点）**，超过会被整条作废 —— 宁可只写 15 个字。
   不要复述输入，不要写「因此评分 X」这种话，只写判断依据。
5. 只判语义，不许猜数据：你看不到任何数据行，也不许假设「大概有」。
   不确定就给 0.5 左右并在 rationale 里说清不确定；完全无法判断就给 null，semantic_fit 给 0.0。
6. 只输出严格 JSON（不要 markdown 代码块，不要任何解释文字），形状：
   {"choice": {"table": "...", "column": "..."} | null,
    "semantic_fit": 0.0 到 1.0 之间的数,
    "rationale": "不超过 60 个字符的中文理由",
    "cites": [{"table": "...", "column": "..."}]}
"#;

/// 一条请求里固定的 task 文案（进 user 消息，不进模板哈希）。
const TASK_TEXT: &str = "判断 relation.from → relation.to 这条关系在业务上是否说得通，按协议返回 JSON。";

/// 提示词与它的哈希。**哈希在构造时算一次**，不每次现算 ——
/// 免得「同一轮里改了模板却没重算哈希」这种静默错（R1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompts {
    pub system: String,
    pub sha256: String,
}

impl Prompts {
    /// 标准模板（`prompt_sha256` 由它算出）。
    pub fn standard() -> Self {
        Self::from_system(SYSTEM_PROMPT)
    }

    /// 任意模板（测试用：改一个字 ⇒ 指纹必须变，AC-005）。
    pub fn from_system(system: impl Into<String>) -> Self {
        let system = system.into();
        let sha256 = sha256_hex(system.as_bytes());
        Self { system, sha256 }
    }
}

impl Default for Prompts {
    fn default() -> Self {
        Self::standard()
    }
}

// ──────────────────────────────────────────────────────────
// 环境变量 → 端点/凭据（AC-002）
// ──────────────────────────────────────────────────────────

/// `--llm` 需要的三件套 + 真 HTTP 传输的构造参数。
///
/// `Debug` 刻意**不打印 api_key**（`LlmConfig` 的 `Debug` 会打印它）——
/// 这里手写 `Debug`，只报端点与模型名。
pub struct LlmSettings {
    pub base_url: String,
    pub config: LlmConfig,
}

impl std::fmt::Debug for LlmSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmSettings")
            .field("base_url", &self.base_url)
            .field("model", &self.config.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl LlmSettings {
    /// 读三件环境变量。**缺哪几个就说哪几个**（AC-002），不静默降级。
    ///
    /// 空串视同缺失（与 `agent::llm` 的 `require_env` 同一条判据）。
    pub fn from_env() -> Result<Self> {
        let keys = [
            BASE_URL_ENV,
            crate::agent::llm::API_KEY_ENV,
            crate::agent::llm::MODEL_ENV,
        ];
        let missing: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|k| std::env::var(k).map(|v| v.trim().is_empty()).unwrap_or(true))
            .collect();
        if !missing.is_empty() {
            bail!(
                "--llm 需要以下环境变量，当前缺失：{}\n\
                 \n\
                 为什么缺了必须报错退出、而不是「跳过 AI 继续抽」：\n\
                 那样产出的提案看起来完全正常，但**实际上没跑 AI** —— \
                 分数里既没有语义项、也没有任何字段能让人看出这一点（R4）。\n\
                 \n\
                 怎么配：三个变量都只走环境变量（凭据不入库、不进日志、不写进任何文件）。",
                missing.join(" / ")
            );
        }
        let base_url = std::env::var(BASE_URL_ENV)?.trim().to_string();
        let config = LlmConfig::from_env().context("读取 LLM 配置（key / model）失败")?;
        Ok(Self { base_url, config })
    }

    /// 造真 HTTP 传输（`agent::llm` 既有那一条，不另起一套）。
    pub fn transport(&self) -> Result<Box<dyn LlmTransport>> {
        Ok(Box::new(crate::agent::llm::HttpTransport::new(
            &self.base_url,
            &self.config.api_key,
            self.config.timeout,
        )?))
    }
}

// ──────────────────────────────────────────────────────────
// 请求：两张表的切片（⛔ 不许塞整份 schema）
// ──────────────────────────────────────────────────────────

/// 切片里的一列。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SliceColumn {
    pub name: String,
    pub data_type: String,
    pub comment: Option<String>,
}

/// 切片里的一张表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SliceTable {
    pub table: String,
    pub columns: Vec<SliceColumn>,
}

/// 一条待判定的关系。
///
/// `to_candidates` 是**目标表的列名**（提案列在前）：AI 的 `choice` 必须落在
/// 这个集合里，否则整条判断作废（协议校验）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JudgeRequest {
    /// 它在 `Proposal::items` 里的下标（回填分数时用）
    pub item_index: usize,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    pub to_candidates: Vec<String>,
    /// **只有这两张表**（源表 + 目标表）；同表自引用时只有一张。
    pub schema_slice: Vec<SliceTable>,
}

impl JudgeRequest {
    /// 关系键 —— 指纹的第一项，也是报告里给读的人看的标识。
    pub fn relation_key(&self) -> String {
        format!(
            "{}.{} -> {}.{}",
            self.from_table, self.from_column, self.to_table, self.to_column
        )
    }

    /// `schema_digest` —— 对切片做**排序后**的规范化序列化再哈希。
    ///
    /// 排序是确定的唯一障碍：`pg_catalog` 的返回顺序在换索引/换版本时可能变，
    /// 而「同一份 schema 两次运行」必须命中同一份缓存（AC-004）。
    pub fn schema_digest(&self) -> String {
        let mut tables: Vec<&SliceTable> = self.schema_slice.iter().collect();
        tables.sort_by(|a, b| a.table.cmp(&b.table));
        let mut canon = String::new();
        for t in tables {
            canon.push_str("T\t");
            canon.push_str(&t.table);
            canon.push('\n');
            let mut cols: Vec<&SliceColumn> = t.columns.iter().collect();
            cols.sort_by(|a, b| a.name.cmp(&b.name));
            for c in cols {
                canon.push_str("C\t");
                canon.push_str(&c.name);
                canon.push('\t');
                canon.push_str(&c.data_type);
                canon.push('\t');
                canon.push_str(c.comment.as_deref().unwrap_or(""));
                canon.push('\n');
            }
        }
        sha256_hex(canon.as_bytes())
    }

    /// **指纹**（R1）—— 六项缺一不可，见模块头。
    pub fn fingerprint(&self, prompt_sha: &str, model: &str, agent_version: &str) -> String {
        let material = format!(
            "{FINGERPRINT_VERSION}\n{}\n{}\n{}\n{}\n{}\n",
            self.relation_key(),
            self.schema_digest(),
            prompt_sha,
            model,
            agent_version
        );
        sha256_hex(material.as_bytes())
    }

    /// user 消息的 JSON（**紧凑序列化**：同输入逐字节相同）。
    fn user_message(&self) -> Result<String> {
        let relation = serde_json::json!({
            "from": format!("{}.{}", self.from_table, self.from_column),
            "to": format!("{}.{}", self.to_table, self.to_column),
            "to_candidates": self.to_candidates,
        });
        let doc = serde_json::json!({
            "relation": relation,
            "schema_slice": self.schema_slice,
            "task": TASK_TEXT,
        });
        serde_json::to_string(&doc).context("AI 请求体序列化失败")
    }
}

// ──────────────────────────────────────────────────────────
// 输出协议（严格 JSON）
// ──────────────────────────────────────────────────────────

/// 引用坐标（`表.列`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cite {
    pub table: String,
    pub column: String,
}

impl Cite {
    /// 人读形式 `表.列`（进报告与 `ai.cites`）。
    pub fn key(&self) -> String {
        format!("{}.{}", self.table, self.column)
    }
}

/// AI 认为最合理的候选（**只记录，不改变结构**）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub table: String,
    pub column: String,
}

/// 一条**已校验**的语义判断（缓存里存的就是它 —— 不含调用元数据）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedJudgement {
    pub semantic_fit: f64,
    pub rationale: String,
    #[serde(default)]
    pub cites: Vec<Cite>,
    #[serde(default)]
    pub choice: Option<Choice>,
}

/// **被作废**的那次回答（也进缓存 —— 见 [`CachedOutput`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedRejection {
    /// 作废类别（`parse` / `choice` / `fit` / `rationale` / `cites`）
    pub kind: Rejection,
    /// 人读理由（报告里原样回放）
    pub detail: String,
}

/// 缓存里存的**回答本身**（判断或作废记录二选一）。
///
/// # 为什么作废的回答也要缓存
///
/// 「同输入 ⇒ 同一份回答，不重复付费」是缓存的存在理由。若只缓存**合规**的回答，
/// 那每一条被协议作废的关系（本机实测：82 条里 6 条 rationale 超 60 字）
/// 每次重跑都会**再付一次钱**，而且拿到的还是同一份不合规回答 ——
/// 于是「第二次运行 0 次调用」这个判据永远不成立，缓存对最贵的那几条恰好失效。
///
/// ⚠️ 回放作废记录时**绝不产出判断**：`judgement == None`，
/// 引擎只把作废计数加回来。所以它不会把「模型答歪了」变成「AI 说这条不成立」。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
// ★ `deny_unknown_fields` 是**故意的**：本表早期版本直接把判定对象存成顶层
// （`{semantic_fit, rationale, cites, choice}`）。若不拒绝未知字段，serde 会把
// 那份旧行**静默**读成 `{judgement: None, rejected: None}` —— 于是每一条旧缓存
// 都变成「缓存坏了」，而真正的解析路径永远走不到兼容分支（本机实测踩到过：
// 76 条命中全被判成坏缓存）。
#[serde(deny_unknown_fields)]
pub struct CachedOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judgement: Option<CachedJudgement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected: Option<CachedRejection>,
}

/// 缓存里的一条：回答 + 当初那次调用的元数据。
///
/// `tokens` 是**当初那次**的用量：命中缓存时不重复计费，但溯源要如实写
/// 「这条判断花了多少」—— 报 0 会让报告里的 token 总量对不上账。
#[derive(Debug, Clone, PartialEq)]
pub struct CachedEntry {
    pub output: CachedOutput,
    pub model: String,
    pub prompt_sha: String,
    pub tokens: u64,
}

/// 一条被接受、可以直接喂给评分卡的判断。
#[derive(Debug, Clone, PartialEq)]
pub struct Judgement {
    pub semantic_fit: f64,
    pub rationale: String,
    pub cites: Vec<Cite>,
    pub choice: Option<Choice>,
    pub model: String,
    pub prompt_sha: String,
    pub tokens: u64,
    pub cache_hit: bool,
}

impl Judgement {
    /// 贴到 `Confidence::ai` 上的溯源（demand 交付物 6 的四个字段）。
    pub fn provenance(&self) -> AiProvenance {
        AiProvenance {
            model: self.model.clone(),
            prompt_sha: self.prompt_sha.clone(),
            tokens: self.tokens,
            cache_hit: self.cache_hit,
            rationale: Some(self.rationale.clone()),
            cites: Some(self.cites.iter().map(Cite::key).collect()),
            choice: self
                .choice
                .as_ref()
                .map(|c| format!("{}.{}", c.table, c.column)),
        }
    }
}

/// 一条判断被作废的原因（**分类计数进报告** —— AC-008 要求「作废并计数」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    /// 不是合法 JSON / 缺字段
    Parse,
    /// `choice` 不在候选集合内
    Choice,
    /// `semantic_fit` 越界或缺
    Fit,
    /// `rationale` 缺失 / 空 / 超长
    Rationale,
    /// `cites` 里有不存在的 `表.列`
    Cites,
}

impl Rejection {
    pub fn as_str(self) -> &'static str {
        match self {
            Rejection::Parse => "解析失败",
            Rejection::Choice => "choice 越界",
            Rejection::Fit => "semantic_fit 越界",
            Rejection::Rationale => "rationale 不合规",
            Rejection::Cites => "cites 编造",
        }
    }
}

/// 去掉可选的 markdown 代码围栏。
///
/// 模板明确要求「不要代码块」，但真实模型偶尔会加。**只剥围栏、不猜内容**：
/// 剥完必须仍是合法 JSON，否则作废 —— 这是「宽容地读、严格地收」，
/// 不是把协议放宽（协议校验一条没少）。
fn strip_code_fence(text: &str) -> &str {
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

/// **校验并解析**一次模型输出。
///
/// 协议四条（设计概要 §4.2）逐条落地：
/// `choice` 在候选内 / `semantic_fit ∈ [0,1]` / `rationale` 1–60 字 /
/// `cites` 每项都是切片里真实存在的 `表.列`。
///
/// 返回 `Err((类别, 人读理由))` —— 类别用于计数，理由用于报告，
/// 两者都不能少：「cites 编造 3 条」和「那 3 条是哪几个假坐标」是两个问题。
pub fn parse_judgement(text: &str, req: &JudgeRequest) -> Result<CachedJudgement, (Rejection, String)> {
    let body = strip_code_fence(text);
    let v: Value = serde_json::from_str(body).map_err(|e| {
        (
            Rejection::Parse,
            format!(
                "响应不是合法 JSON：{e}（截断到 200 字：{}）",
                body.chars().take(200).collect::<String>()
            ),
        )
    })?;

    // ── semantic_fit ∈ [0,1] ─────────────────────────────
    let semantic_fit = v.get("semantic_fit").and_then(Value::as_f64).ok_or_else(|| {
        (Rejection::Fit, "缺 semantic_fit（或不是数字）".to_string())
    })?;
    if !(0.0..=1.0).contains(&semantic_fit) {
        return Err((
            Rejection::Fit,
            format!("semantic_fit = {semantic_fit} 越界（必须在 0.0–1.0）"),
        ));
    }

    // ── rationale 1–60 字 ────────────────────────────────
    let rationale = v
        .get("rationale")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let n = rationale.chars().count();
    if n == 0 {
        return Err((Rejection::Rationale, "rationale 是空的".to_string()));
    }
    if n > RATIONALE_MAX_CHARS {
        return Err((
            Rejection::Rationale,
            format!("rationale {n} 字，超过上限 {RATIONALE_MAX_CHARS} 字"),
        ));
    }

    // ── cites 必须是真实坐标 ─────────────────────────────
    //
    // 判据是「在**本次切片**里真实存在」，比「在本 schema 里存在」更严：
    // 模型只被给了这两张表，它引用的表若不在切片里，那只能是编的。
    let known: Vec<(String, String)> = req
        .schema_slice
        .iter()
        .flat_map(|t| {
            t.columns
                .iter()
                .map(move |c| (t.table.clone(), c.name.clone()))
        })
        .collect();
    let mut cites: Vec<Cite> = Vec::new();
    let raw_cites = v.get("cites").ok_or_else(|| {
        (Rejection::Cites, "缺 cites（没有引用坐标就无法复核）".to_string())
    })?;
    let arr = raw_cites.as_array().ok_or_else(|| {
        (Rejection::Cites, "cites 不是数组".to_string())
    })?;
    for (i, c) in arr.iter().enumerate() {
        let table = c.get("table").and_then(Value::as_str).unwrap_or("");
        let column = c.get("column").and_then(Value::as_str).unwrap_or("");
        if table.is_empty() || column.is_empty() {
            return Err((
                Rejection::Cites,
                format!("cites[{i}] 缺 table/column"),
            ));
        }
        if !known.iter().any(|(t, k)| t == table && k == column) {
            return Err((
                Rejection::Cites,
                format!("cites[{i}] = {table}.{column} 不在本次 schema 切片里 —— 编造的坐标"),
            ));
        }
        cites.push(Cite { table: table.to_string(), column: column.to_string() });
    }

    // ── choice ∈ 候选集合（或 null）──────────────────────
    let choice = match v.get("choice") {
        None | Some(Value::Null) => None,
        Some(c) => {
            let table = c.get("table").and_then(Value::as_str).unwrap_or("");
            let column = c.get("column").and_then(Value::as_str).unwrap_or("");
            if table != req.to_table {
                return Err((
                    Rejection::Choice,
                    format!("choice.table = {table:?} 不是目标表 {:?}", req.to_table),
                ));
            }
            if !req.to_candidates.iter().any(|k| k == column) {
                return Err((
                    Rejection::Choice,
                    format!(
                        "choice.column = {column:?} 不在候选集合内（{:?}）",
                        req.to_candidates
                    ),
                ));
            }
            Some(Choice { table: table.to_string(), column: column.to_string() })
        }
    };

    Ok(CachedJudgement { semantic_fit, rationale: rationale.to_string(), cites, choice })
}

// ──────────────────────────────────────────────────────────
// 缓存（trait + 内存实现）—— **命中绝不调用**这条在编排里
// ──────────────────────────────────────────────────────────

/// 指纹 → [`CachedEntry`]。**同步** trait：编排是同步的，真实存储层
/// （`db::ai_cache::PgAiCache`）在编排**之前/之后**批量读/写，
/// 不在调用链中间混 async（那会迫使 `block_on`，而本仓明令不写 `block_on`）。
pub trait AiCache {
    fn get(&mut self, fingerprint: &str) -> Option<CachedEntry>;
    fn put(&mut self, fingerprint: &str, entry: CachedEntry);
}

/// 内存缓存 —— 测试用，也是 PG 预加载/回写之间的**中转**。
#[derive(Debug, Default)]
pub struct MemoryAiCache {
    entries: HashMap<String, CachedEntry>,
    /// 本次**新写入**的条目（未落库）。`PgAiCache::persist` 只写这些。
    pub fresh: Vec<(String, CachedEntry)>,
}

impl MemoryAiCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 用一批已落库的条目造缓存（PG 预加载的落点）。
    pub fn with_entries(entries: impl IntoIterator<Item = (String, CachedEntry)>) -> Self {
        Self { entries: entries.into_iter().collect(), fresh: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空（AC-003 的「删掉缓存后第三次运行必须重新调用」）。
    pub fn clear(&mut self) {
        self.entries.clear();
        self.fresh.clear();
    }

    /// 直接看某个指纹在不在（测试断言「确实命中的是这一条」）。
    pub fn contains(&self, fingerprint: &str) -> bool {
        self.entries.contains_key(fingerprint)
    }
}

impl AiCache for MemoryAiCache {
    fn get(&mut self, fingerprint: &str) -> Option<CachedEntry> {
        self.entries.get(fingerprint).cloned()
    }

    fn put(&mut self, fingerprint: &str, entry: CachedEntry) {
        self.entries.insert(fingerprint.to_string(), entry.clone());
        self.fresh.push((fingerprint.to_string(), entry));
    }
}

// ──────────────────────────────────────────────────────────
// 编排 + 预算
// ──────────────────────────────────────────────────────────

/// 一轮 AI 判定的统计（报告里逐项打印，**失败也要看得见**）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AiStats {
    /// 请求了几条
    pub requested: usize,
    /// 最终拿到有效判断几条
    pub judged: usize,
    /// 缓存命中几条（**命中不调用**；含「作废记录的回放」，后者另计）
    pub cache_hits: usize,
    /// 回放的**作废记录**条数（命中了缓存、但缓存里是一份不合规回答 ⇒ 照样作废）
    pub rejected_cache_hits: usize,
    /// 真正调用模型几次
    pub calls: usize,
    /// 实际消耗 token（含命中记录里的历史用量吗？**不含** —— 见 `tokens_spent`）
    pub tokens_spent: u64,
    /// 上限（`--budget-tokens`）
    pub budget: u64,
    /// 因预算停止：停在第几条（1-based）
    pub budget_stopped: Option<usize>,
    /// 传输层失败 / 无 usage（**失败不计入判断**，但可能已经花了钱）
    pub failed_calls: usize,
    pub rejected_parse: usize,
    pub rejected_choice: usize,
    pub rejected_fit: usize,
    pub rejected_rationale: usize,
    pub rejected_cites: usize,
    /// `--refresh` 下被忽略的缓存条数（R3 的「强制刷新 N 条」）
    pub refresh_ignored: usize,
    /// 作废样例（最多留 5 条，报告里点名）
    pub rejection_samples: Vec<String>,
}

impl AiStats {
    pub fn rejected(&self) -> usize {
        self.rejected_parse
            + self.rejected_choice
            + self.rejected_fit
            + self.rejected_rationale
            + self.rejected_cites
    }

    /// 缓存命中率 = 命中 / **请求数**（分母为 0 ⇒ `None`，报告里显示 `—`）。
    ///
    /// ⚠️ 分母是 `requested` 而不是 `judged`：作废记录也命中缓存（回放时照样作废），
    /// 若拿 `judged` 当分母，一次「76 命中 / 5 判定」会算出 1520% 这种荒谬的数
    /// （本机实测踩到过）。这里的语义是「多少次请求由缓存满足」。
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.requested == 0 {
            None
        } else {
            Some(self.cache_hits as f64 / self.requested as f64)
        }
    }

    /// 预算是否已被越过（装配层据此**不出产物**）。
    ///
    /// 两种越过方式都算：
    /// - 还有关系没判（`budget_stopped` 有值）；
    /// - 全部判完了，但累计用量已经**超过**上限（最后一次调用把预算顶过去了）。
    ///
    /// 后者不能漏：AC-006 的判据是「把预算设成 1 token ⇒ 第一次调用后即停」——
    /// 只有一条关系时，「停」的表现就是**跑完这一条、然后拒绝出产物**。
    pub fn over_budget(&self) -> bool {
        self.budget_stopped.is_some() || self.tokens_spent > self.budget
    }

    fn reject(&mut self, why: Rejection, detail: String) {
        match why {
            Rejection::Parse => self.rejected_parse += 1,
            Rejection::Choice => self.rejected_choice += 1,
            Rejection::Fit => self.rejected_fit += 1,
            Rejection::Rationale => self.rejected_rationale += 1,
            Rejection::Cites => self.rejected_cites += 1,
        }
        if self.rejection_samples.len() < 5 {
            self.rejection_samples.push(format!("{}：{detail}", why.as_str()));
        }
    }
}

/// AI 语义判定器 —— 拿注入的传输与缓存跑一批关系。
///
/// `agent_version` 是**判定逻辑的版本**（进指纹）：改了本模块的解析/提示词策略，
/// 就该让它变，否则旧缓存会被新逻辑误用。本 REQ 把它固定为
/// [`AGENT_VERSION`]；将来升级时改常量即可。
pub const AGENT_VERSION: &str = "ai-semantic-1";

/// 一次判定的编排器。
pub struct AiSemanticJudge<'a> {
    transport: &'a mut dyn LlmTransport,
    cache: &'a mut dyn AiCache,
    model: String,
    agent_version: String,
    prompts: Prompts,
    budget: u64,
    refresh: bool,
    max_tokens: u32,
}

impl<'a> AiSemanticJudge<'a> {
    pub fn new(
        transport: &'a mut dyn LlmTransport,
        cache: &'a mut dyn AiCache,
        model: impl Into<String>,
        prompts: Prompts,
        budget: u64,
        refresh: bool,
        max_tokens: u32,
    ) -> Self {
        Self {
            transport,
            cache,
            model: model.into(),
            agent_version: AGENT_VERSION.to_string(),
            prompts,
            budget,
            refresh,
            max_tokens,
        }
    }

    /// 换个 agent 版本（测试断言「版本进指纹」）。
    pub fn with_agent_version(mut self, v: impl Into<String>) -> Self {
        self.agent_version = v.into();
        self
    }

    /// 跑一批。返回 `(item_index → 判断, 统计)`。
    ///
    /// **失败 / 作废的条目不在 map 里** —— 那不是 0 分，是「没判断」（R6）。
    pub fn judge(&mut self, reqs: &[JudgeRequest]) -> (HashMap<usize, Judgement>, AiStats) {
        let mut out: HashMap<usize, Judgement> = HashMap::new();
        let mut stats = AiStats { requested: reqs.len(), budget: self.budget, ..Default::default() };

        for (i, req) in reqs.iter().enumerate() {
            // ── 预算闸：**在发起下一次之前**判 ──
            //
            // 单次调用的花费在调用前不可知，所以这里是「不再发起」的闸：
            // 越过的那一笔照样记账（钱已经花了），只是不会再花第二笔。
            if stats.tokens_spent >= self.budget {
                stats.budget_stopped = Some(i + 1);
                break;
            }

            let fp = req.fingerprint(&self.prompts.sha256, &self.model, &self.agent_version);

            // ── 缓存：命中**绝不调用**（R2）──
            if self.refresh {
                if self.cache.get(&fp).is_some() {
                    stats.refresh_ignored += 1;
                }
            } else if let Some(entry) = self.cache.get(&fp) {
                stats.cache_hits += 1;
                match (entry.output.judgement, entry.output.rejected) {
                    (Some(j), _) => {
                        stats.judged += 1;
                        out.insert(
                            req.item_index,
                            Judgement {
                                semantic_fit: j.semantic_fit,
                                rationale: j.rationale,
                                cites: j.cites,
                                choice: j.choice,
                                model: entry.model,
                                prompt_sha: entry.prompt_sha,
                                tokens: entry.tokens,
                                cache_hit: true,
                            },
                        );
                    }
                    // ★ 作废记录的回放：**不产出判断**，只把作废计数加回来。
                    // 这样「同一份不合规回答」不会每次重跑都再付一次钱，
                    // 也绝不会被当成「AI 判断这条不成立」（那是 0 分，不是缺失）。
                    (None, Some(r)) => {
                        stats.rejected_cache_hits += 1;
                        stats.reject(r.kind, format!("{}（缓存回放）{detail}", req.relation_key(), detail = r.detail));
                    }
                    (None, None) => {
                        stats.reject(
                            Rejection::Parse,
                            format!("{} 的缓存条目既没有判断也没有作废记录（缓存坏了）", req.relation_key()),
                        );
                    }
                }
                continue;
            }

            // ── 未命中：调用 ──
            let user = match req.user_message() {
                Ok(u) => u,
                Err(e) => {
                    // 请求体都拼不出来 = 本条的输入有问题，不是模型的错。
                    stats.failed_calls += 1;
                    stats.reject(Rejection::Parse, format!("{} 请求体构造失败：{e:#}", req.relation_key()));
                    continue;
                }
            };
            let chat = ChatRequest {
                model: self.model.clone(),
                messages: vec![
                    Message::system(self.prompts.system.clone()),
                    Message::user(user),
                ],
                max_tokens: self.max_tokens,
            };
            stats.calls += 1;
            let resp = match self.transport.complete(&chat) {
                Ok(r) => r,
                Err(e) => {
                    // 传输层失败：这一条**没有判断**（不是 0 分）。
                    stats.failed_calls += 1;
                    stats.reject(
                        Rejection::Parse,
                        format!("{} 调用失败：{:#}", req.relation_key(), e),
                    );
                    continue;
                }
            };

            // ── 先记账，再解析（钱已经花了）──
            let Some(usage) = resp.usage.as_ref() else {
                // 没有 usage ⇒ 这笔钱无从计入预算（少算 = 闸门形同虚设）。
                // 与 `agent::llm::record_usage` 同一条 fail-closed 纪律。
                stats.failed_calls += 1;
                stats.reject(
                    Rejection::Parse,
                    format!("{} 的响应没有 usage —— 无法记账，判断作废", req.relation_key()),
                );
                continue;
            };
            let tokens = usage.total_tokens();
            stats.tokens_spent += tokens;

            let text = match completion_text(&resp) {
                Ok(t) => t,
                Err(e) => {
                    stats.failed_calls += 1;
                    stats.reject(
                        Rejection::Parse,
                        format!("{} 正文不可用：{:#}", req.relation_key(), e),
                    );
                    continue;
                }
            };
            match parse_judgement(&text, req) {
                Ok(j) => {
                    let entry = CachedEntry {
                        output: CachedOutput { judgement: Some(j.clone()), rejected: None },
                        model: self.model.clone(),
                        prompt_sha: self.prompts.sha256.clone(),
                        tokens,
                    };
                    self.cache.put(&fp, entry);
                    stats.judged += 1;
                    out.insert(
                        req.item_index,
                        Judgement {
                            semantic_fit: j.semantic_fit,
                            rationale: j.rationale,
                            cites: j.cites,
                            choice: j.choice,
                            model: self.model.clone(),
                            prompt_sha: self.prompts.sha256.clone(),
                            tokens,
                            cache_hit: false,
                        },
                    );
                }
                Err((why, detail)) => {
                    // 作废的回答**也进缓存**（见 [`CachedOutput`] 的论证）：
                    // 不缓存的话，每次重跑都会为同一份不合规回答再付一次钱。
                    let entry = CachedEntry {
                        output: CachedOutput {
                            judgement: None,
                            rejected: Some(CachedRejection { kind: why, detail: detail.clone() }),
                        },
                        model: self.model.clone(),
                        prompt_sha: self.prompts.sha256.clone(),
                        tokens,
                    };
                    self.cache.put(&fp, entry);
                    stats.reject(why, format!("{} {detail}", req.relation_key()));
                }
            }
        }

        (out, stats)
    }
}

/// 便利入口：一批请求 + 一次性判定（装配层与测试用同一个函数，不抄第二份）。
#[allow(clippy::too_many_arguments)]
pub fn judge_all(
    transport: &mut dyn LlmTransport,
    cache: &mut dyn AiCache,
    reqs: &[JudgeRequest],
    model: &str,
    prompts: Prompts,
    budget: u64,
    refresh: bool,
    max_tokens: u32,
) -> (HashMap<usize, Judgement>, AiStats) {
    let mut judge = AiSemanticJudge::new(transport, cache, model, prompts, budget, refresh, max_tokens);
    judge.judge(reqs)
}

// ──────────────────────────────────────────────────────────
// 约定：`--llm` 与预算必须成对（R5）
// ──────────────────────────────────────────────────────────

/// 装配层的预算闸：**AI 开启时 `--budget-tokens` 必填且 >0**（R5 / AC-006）。
///
/// 放在本模块而不是 bin 里：这条规则属于「AI 语义通道能不能开」，
/// 与「哪个 CLI 打开了它」无关；将来别的入口（MCP / 服务）打开 AI 时
/// 不该再抄一遍判据。
pub fn require_ai_budget(budget_tokens: Option<u64>) -> Result<u64> {
    match budget_tokens {
        Some(b) if b > 0 => Ok(b),
        Some(0) => bail!(
            "--llm 需要 `--budget-tokens` > 0（当前是 0）。\n\
             0 的意思是「一次都不许调用」，那与「不开 AI」是同一件事 ——\
             要跑 AI 就给一个真实上限（例如 100000）。"
        ),
        _ => bail!(
            "--llm 需要 `--budget-tokens`（AI 开启时**必填且 >0**）。\n\
             \n\
             为什么强制：语义判断**逐条调用模型**，没有上限时一次全量运行的\
             花费由 schema 规模决定，而调用方无从预估（设计概要 §七 · G4）。\
             预算不是「省钱的建议」，是**开 AI 的前提**：超了会停，并按\
             「已用 / 上限 / 停在第几条」报告（R5）。"
        ),
    }
}
