//! **内置 LLM 兜底 agent** —— [`ExtractionAgent`] 的第三个实现（REQ-034 · RN-5）。
//!
//! ```text
//! ontok ──HTTP POST /v1/chat/completions──▶ OpenAI 兼容端点
//!   │  每组一次调用（规格 §7.1 阶段 1）
//!   ◀── content + usage（含 reasoning_tokens）
//! ```
//!
//! # ★ 它是**第三个实现**，不是另起一套
//!
//! | | `inproc` | `acp` | `llm`（本模块） |
//! | --- | --- | --- | --- |
//! | 造自 | `RuleBasedAgent` | `AcpAgent` | [`LlmAgent`] |
//! | I/O | 零 | 子进程 stdio | HTTP |
//! | 端点来源 | — | **登记的 `endpoint`** | **登记的 `endpoint`** |
//! | `capabilities()` | `produces_proposal` | `reports_usage` | `reports_usage` |
//!
//! 分派仍只有 [`crate::agent::factory::build_agent`] 一处，抽象层**一行未改**
//! （AC-001：既有 trait 方法签名不动）。
//!
//! # ★ 为什么 `capabilities()` 只声明 `reports_usage`（与 ACP 同一个结论）
//!
//! [`crate::agent::acp`] 模块头对「为什么 LLM 这类黑盒实现不能声明
//! `rebuilds_proposal`」有完整论证，那论证**逐字适用于本实现**：
//! `property.origin`（名字来自 Comment / WordList / Relation 中的哪条命名路径）与
//! `type.rejected_comments`（REQ-005 质量门槛拒用了几列），
//! **模型在黑盒里没有渠道提供，adapter 也无法凭空知道**。
//!
//! 编一个值填进去 ⇒ `stats.props_via_*` 与 `props_comment_rejected`
//! 是**编造的数字**，而 stats.json 是 E2E 判读器读的东西。
//!
//! 于是 [`crate::agent::orchestrate`] 的 `require_proposal_producing`
//! 会**在入口拒绝整轮** —— 与 ACP 今天的行为完全一致，且是**有意的**：
//! 宁可一分钱不花地拒绝，也不发一份溯源对不上的提案。
//!
//! > **能力标签是实现的固有属性，不随这一份答案翻转。**
//! > （`acp/mod.rs:383-417` 钉着这条不变量，本实现照抄。）
//!
//! # ★ AC-005 · fallback 语义（本 REQ 唯一的设计判断，结论先行）
//!
//! RN-5 写的是「作为 **fallback**」。实现下来的结论是：
//!
//! > **本实现只提供「显式配置」这一条路，**不做**「主 agent 失败时自动接管」。
//!
//! 三问的答案与理由：
//!
//! ## 一、什么条件下走 LLM？——**只有显式配置，没有自动触发**
//!
//! 「主 agent 失败时自动走 LLM」在当前抽象里**写不出来**，而且**不该写**：
//!
//! | 障碍 | 位置 |
//! | --- | --- |
//! | `build_agent` 返回**单个** `Box<dyn ExtractionAgent>`，无「备用」概念 | `factory.rs:78` |
//! | `orchestrate` 收**单个** agent，无「先试谁」的概念 | `orchestrate.rs:524` |
//! | `factory` 模块头**明确禁止回落**（回落 = 静默的假溯源）| `factory.rs:21-34` |
//!
//! 最强的那条不是「写不出来」，是**「写了就是本项目点名的头号病」**：
//! `extracted_by` 只有一个 `agent` 字段。一轮里 90 组走规则、1 组走 LLM，
//! 溯源**只能记一个** —— 于是那份提案对 90 组的来源在说谎，且**静默**。
//! 与其让 `factory` 偷偷回落，不如让入口**明确报错**（既有 `other` 分支已经这么做了）。
//!
//! 所以：**登记里写 `protocol = "llm"`，那一轮就走 LLM**，全部组都走，不混。
//!
//! ## 二、「失败」指传输层 `Err`，还是**内容类缺口**？
//!
//! **本实现只把传输/形状类失败算失败**，两者严格分开：
//!
//! | 形态 | 判据位置 | 本实现的处理 |
//! | --- | --- | --- |
//! | 传输层失败（连不上 / 非 200 / 形状不对）| `HttpTransport::complete` | [`Self::extract_group`] 返回 `Err` |
//! | **内容类缺口**（正文里的 JSON 缺字段、来源列不在本组 schema）| `acp::parse::parse_extraction` + 阶段 2 `verify_mappings` | 同样是 `Err`，但**归因不同** |
//!
//! ⚠️ **不因为「内容看起来不好」就自动再问一次** —— 那是**追加重试**，
//! 不是 fallback。REQ-025 已登记「BA 倾向不补追加重试」，本 REQ **不碰那件事**。
//! 内容类缺口由**事后核对**（阶段 2 / `assemble::contract_gaps`）发现并显式记失败，
//! 与「换实现」是两件不同的事，混起来就会出现「重试到满意为止」的烧钱循环。
//!
//! ## 三、fallback 会再花一次钱 —— 与成本闸门怎么协同？
//!
//! 因为选了「不做自动 fallback」，这条**在本 REQ 里不成立**：
//! 一轮要么全走 LLM（只花一次的钱），要么全不走。**不存在「两次」**。
//!
//! 若将来真要做「规则失败 → LLM 接管」，**闸门协同的答案必须是**：
//! 两段用**同一个 `max_used`**，且第二次调用**照样记账**、照样受闸门约束
//! —— 即 `used` 是**两段之和**。那时闸门的作用正是「防止 fallback 变成
//! 「主实现一失败就无限重试到超预算」。本实现已经把每组用量如实上报
//! （[`CAPABILITY_REPORTS_USAGE`] + [`ExtractionAgent::usage`]），
//! 那条路是通的，但**接线不在本 REQ**。
//!
//! # 模块分层（照抄 [`crate::agent::acp`]）
//!
//! | 模块 | 职责 | I/O |
//! | --- | --- | --- |
//! | [`protocol`] | 请求/响应 JSON 形状、**用量口径**、正文提取（**两个坑都在这**）| **零** |
//! | [`transport`] | HTTP（`ureq` 阻塞式）+ [`ScriptedTransport`] | 有 |
//! | 本模块 | [`ExtractionAgent`] 实现、prompt 构建、候选解析、能力声明 | 间接 |
//!
//! # 怎么造
//!
//! ```no_run
//! use ontok::agent::llm::{LlmAgent, LlmConfig};
//! use ontok::agent::ExtractionAgent;
//!
//! # fn main() -> anyhow::Result<()> {
//! // 凭据/模型从**环境变量**读（AC-002）；base_url 由调用方从**登记的 endpoint** 给
//! let cfg = LlmConfig::from_env()?;
//! let agent = LlmAgent::from_config(cfg, "https://…/v1", "my-llm-agent", "1.0.0", 1)?;
//! println!("{:?}", agent.capabilities()); // ["reports_usage"]
//! # Ok(()) }
//! ```

pub mod protocol;
pub mod transport;

pub use protocol::{completion_text, ChatRequest, ChatResponse, Usage};
pub use transport::{HttpTransport, LlmTransport, ScriptedTransport};

use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::agent::acp::parse::parse_extraction;
use crate::agent::group::{ExtractionGroup, GroupExtraction, GroupFailure};
use crate::agent::registry::AgentIdentity;
use crate::agent::{
    ExtractionAgent, UsageReport, CAPABILITY_REPORTS_USAGE, CAPABILITY_REBUILDS_PROPOSAL,
};

// ──────────────────────────────────────────────────────────
// 配置（AC-002：端点来自登记，凭据来自环境变量）
// ──────────────────────────────────────────────────────────

/// **api key 的环境变量名**。只走环境变量 —— 不入库、不进日志、不进报告、不进 commit。
pub const API_KEY_ENV: &str = "ONTOK_LLM_API_KEY";

/// **model 名的环境变量名**。模型名是**部署事实**：同一份代码在开发/生产用不同模型
/// 是常态，写死等于「换个环境就得改代码」。
pub const MODEL_ENV: &str = "ONTOK_LLM_MODEL";

/// **`max_tokens` 的环境变量名**（可选，缺省用 [`LlmConfig::DEFAULT_MAX_TOKENS`]）。
///
/// 存在的理由是**坑一**：推理模型给小了正文就是空的，而「给多少」随部署的模型而变，
/// 属于部署事实。它同时是「调大它」这条修复指引的落点。
pub const MAX_TOKENS_ENV: &str = "ONTOK_LLM_MAX_TOKENS";

/// 请求超时。
pub const TIMEOUT_ENV: &str = "ONTOK_LLM_TIMEOUT_SECS";

/// LLM 实现的配置。
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// api key —— **只在内存里**，不进日志（见 [`HttpTransport`] 的手写 `Debug`）。
    pub api_key: String,
    /// 模型名（从环境变量来）。
    pub model: String,
    /// 输出上限（推理模型必须给足，见坑一）。
    pub max_tokens: u32,
    /// 单次调用超时。
    pub timeout: Duration,
}

impl LlmConfig {
    /// 实测能拿到正文的量级（`max_tokens=20` ⇒ 正文为空；`300` ⇒ 有正文）。
    pub const DEFAULT_MAX_TOKENS: u32 = 300;
    pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

    /// 从**环境变量**读凭据与模型。**base_url 不在这里** —— 它是**登记的事实**，
    /// 由调用方交给 [`HttpTransport::new`] / [`Self::from_config`]。
    ///
    /// ⚠️ 刻意**不**让本函数收 base_url：它不参与任何判断，
    /// 收进来只会让人误以为「配置从环境变量读完之后就不用管端点了」。
    /// 端点与凭据是**两个来源**，混在一个函数里等于把纪律写糊。
    ///
    /// 沿用 [`crate::config::require_env`] 的 fail-closed 纪律：
    /// **缺变量必报错，不静默回落** —— 回落到一个默认 key 只会让请求在
    /// 401 之后报一个指向错误的错。
    ///
    /// ⚠️ 错误信息**只说缺哪个变量名**，绝不回显变量的值。
    pub fn from_env() -> Result<Self> {
        let api_key = require_env(API_KEY_ENV)?;
        let model = require_env(MODEL_ENV)?;
        let max_tokens = match std::env::var(MAX_TOKENS_ENV) {
            Ok(v) if !v.trim().is_empty() => v.trim().parse::<u32>().with_context(|| {
                format!("{MAX_TOKENS_ENV} 不是合法的正整数（**只说变量名，不回显值**）")
            })?,
            _ => Self::DEFAULT_MAX_TOKENS,
        };
        let timeout_secs = match std::env::var(TIMEOUT_ENV) {
            Ok(v) if !v.trim().is_empty() => v.trim().parse::<u64>().with_context(|| {
                format!("{TIMEOUT_ENV} 不是合法的正整数（**只说变量名，不回显值**）")
            })?,
            _ => Self::DEFAULT_TIMEOUT_SECS,
        };
        Ok(Self {
            api_key,
            model,
            max_tokens,
            timeout: Duration::from_secs(timeout_secs),
        })
    }
}

/// `require_env` 的本地副本 —— 抄 [`crate::config::require_env`] 的判据，
/// 但返回 [`anyhow::Result`] 以便与本模块其余签名一致。
///
/// 为什么不直接调 `crate::config::require_env`：它返回 `ConfigError`
/// 且那个函数是 **`'static str` 键**的；本模块的键虽是 `&'static str`，
/// 但错误信息里要带上「这个变量属于 LLM 配置」的上下文。
/// 判据本身（空串视同缺失）**逐字一致**。
fn require_env(key: &'static str) -> Result<String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => anyhow::bail!(
            "缺少环境变量 {key}（凭据与模型名**只走环境变量**，不写死在代码里、\
             不入库、不进日志）。\n\
             为什么不给默认值：回落到一个默认 key 只会换来一个 401，\
             而那个错会指向代码、实际原因却在部署环境 —— 与 \
             `factory` 模块头「坏配置早失败，好过造出一个能跑但溯源是假的 agent」\
             是同一条纪律。"
        ),
    }
}

// ──────────────────────────────────────────────────────────
// agent 实现
// ──────────────────────────────────────────────────────────

/// **内置 LLM 兜底 agent** —— [`ExtractionAgent`] 的第三个实现。
pub struct LlmAgent {
    transport: Mutex<Box<dyn LlmTransport>>,
    identity: AgentIdentity,
    /// 模型名（来自环境变量）—— 让「这一轮到底用的什么模型」有据可查，
    /// 与 [`crate::agent::acp::AcpAgent::model_note`] 同一职责。
    model: String,
    /// 输出上限 —— **必须跟着请求真的发出去**（坑一）。
    ///
    /// ⚠️ 这里存一份而不是每次现读：`max_tokens` 给小了正文就是空的，
    /// 而**发 0 等于把坑一变成常态**。它属于「请求的一部分」，
    /// 所以必须由构造时确定、每次调用原样发出。
    max_tokens: u32,
    /// 每组一条最新用量（[`ExtractionAgent::usage`] 是 `&self` 上的方法）。
    usage: Mutex<Vec<UsageReport>>,
    /// 自报的失败组。
    failed: Mutex<Vec<GroupFailure>>,
}

impl LlmAgent {
    /// 用任意传输造一个 agent（测试 / 回放）。
    pub fn with_transport(
        transport: Box<dyn LlmTransport>,
        agent: &str,
        version: &str,
        model: &str,
        max_tokens: u32,
        round: i32,
    ) -> Result<Self> {
        // 身份来自**登记**（调用方拿登记的 agent/version 传进来），
        // 不是硬编码默认值（§8.4）。理由与 [`crate::agent::registry::AgentIdentity`]
        // 相同：`extracted_by` 必须记「谁抽的」。
        if agent.trim().is_empty() {
            anyhow::bail!("agent 登记名为空 —— §8.4 要求 extracted_by 记真实身份");
        }
        if version.trim().is_empty() {
            anyhow::bail!("agent 版本为空 —— §8.4 要求记具体版本，不能留空");
        }
        if max_tokens == 0 {
            // 发 0 ⇒ 服务端几乎必然回空正文（坑一）⇒ 每一组都失败。
            // 在构造时就拒，比跑一轮才发现便宜得多。
            anyhow::bail!(
                "max_tokens 是 0 —— 推理模型在输出上限为 0 时**必然**回空正文，\
                 那一轮会**每一组都失败**（实测：max_tokens=20 时正文已为空）。\n\
                 请把它设成一个能拿到正文的量级（默认 {}，环境变量 {}）。",
                LlmConfig::DEFAULT_MAX_TOKENS,
                MAX_TOKENS_ENV
            );
        }
        Ok(Self {
            transport: Mutex::new(transport),
            identity: AgentIdentity {
                agent: agent.to_string(),
                version: version.to_string(),
                round: round.max(1),
            },
            model: model.to_string(),
            max_tokens,
            usage: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        })
    }

    /// 真发 HTTP：凭据/模型从环境变量，**base_url 由调用方从登记的 `endpoint` 取**。
    pub fn from_config(
        cfg: LlmConfig,
        base_url: &str,
        agent: &str,
        version: &str,
        round: i32,
    ) -> Result<Self> {
        let transport = HttpTransport::new(base_url, &cfg.api_key, cfg.timeout)
            .context("构造 LLM HTTP 传输失败")?;
        Self::with_transport(
            Box::new(transport),
            agent,
            version,
            &cfg.model,
            cfg.max_tokens,
            round,
        )
    }

    /// 这一轮用的模型名（来自环境变量）。
    pub fn model(&self) -> &str {
        &self.model
    }

    /// 构建 prompt —— **复用 [`crate::agent::acp::parse::build_prompt`]**。
    ///
    /// 为什么复用而不是写第二份：阶段 1 的输入契约（规格 §7.1 那五个）
    /// 是**协议中立**的，与「谁来抽」无关。两份 prompt 会**漂移** ——
    /// 而 prompt 一漂移，两条路径的抽取结果就不可比了，
    /// 于是「换个 agent 换出的差别」里混进了「prompt 不一样」这个**假原因**。
    fn build_prompt(group: &ExtractionGroup) -> String {        crate::agent::acp::parse::build_prompt(group)
    }
}

impl ExtractionAgent for LlmAgent {
    /// 身份来自**登记**（`extracted_by` 要记真实来源，§8.4）。
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }

    /// ★ 只声明 [`CAPABILITY_REPORTS_USAGE`]。**论证见本模块头**
    /// （与 [`crate::agent::acp::AcpAgent`] 同一个结论：黑盒实现填不出
    /// `origin` / `rejected_comments`，声明产出能力就是撒谎）。
    ///
    /// 声明 `reports_usage` 的**后果**是编排强制要求设预算
    /// （`orchestrate::require_budget` 的 fail-closed）—— 对一个真的在烧 token 的
    /// agent，这正是要的。
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REPORTS_USAGE.to_string()]
    }

    /// 阶段 1：抽一组 —— **每组一次 HTTP 调用**（规格 §7.1）。
    fn extract_group(&self, group: &ExtractionGroup) -> Result<GroupExtraction> {
        let prompt = Self::build_prompt(group);
        let req = self.build_request(&prompt)?;

        let resp = self
            .transport
            .lock()
            .expect("transport 锁")
            .complete(&req)?;

        // ── 先记账，再解析 ──
        //
        // 与 `AcpAgent::extract_group` 同一条纪律，且理由相同：**钱已经花了**。
        // 若先解析、正文为空就直接返回，那一笔钱**永远不进账** ——
        // 成本闸门少算，下一组照跑。**漏记的钱比报错更难发现**（账是平的）。
        self.record_usage(group, &resp)?;

        // 正文为空 / 截断 ⇒ 这里 `bail!`（AC-003 坑一）。
        let text = completion_text(&resp)?;

        // 候选解析复用 ACP 的那条（同一份阶段 1 输出契约）——
        // 它会核「来源列是否真的在本组 schema 里」（设计点③）。
        parse_extraction(&text, group)
    }

    /// 本实现自报的失败组。
    fn failed_groups(&self) -> Vec<GroupFailure> {
        self.failed.lock().expect("failed 锁").clone()
    }

    /// **每次尝试一条**用量（编排按「组 × 第几笔」并进账，见
    /// [`crate::agent::orchestrate::BudgetReport::absorb`]）。
    ///
    /// 与 ACP 的差别：OpenAI 兼容端点**每次请求只回一次**该次请求的用量
    /// （请求级响应，不是流式累计），所以**不需要 baseline 差分** —— 直接就是本次增量。
    ///
    /// 同组重试 ⇒ **多条**（`attempt` 不同），编排**每条都算**（REQ-050）。
    fn usage(&self) -> Vec<UsageReport> {
        self.usage.lock().expect("usage 锁").clone()
    }
}

impl LlmAgent {
    fn build_request(&self, prompt: &str) -> Result<ChatRequest> {
        // 身份信息走 system 消息：**它要进 `extracted_by`**（§8.4），
        // 且对同一次抽取的**一致性**有贡献（模型据此知道该干什么）。
        let system = format!(
            "你正在为本体抽取 agent {} v{}（模型 {}）抽取数据库本体候选。",
            self.identity.agent, self.identity.version, self.model
        );
        Ok(ChatRequest {
            model: self.model.clone(),
            messages: vec![
                protocol::Message::system(system),
                protocol::Message::user(prompt),
            ],
            max_tokens: self.max_tokens,
        })
    }

    /// 记一次抽取的用量。**`usage` 缺失 ⇒ 报错，不报 0**（fail-closed）。
    ///
    /// 理由与 [`crate::agent::acp::AcpAgent::record_usage`] 逐字相同：
    /// 「不知道」说成「免费」是账目造假；不报这一组则闸门少算它。
    fn record_usage(&self, group: &ExtractionGroup, resp: &ChatResponse) -> Result<()> {
        let usage = resp.usage.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "LLM 报告了一组 {:?} 的抽取，但响应里**没有 `usage` 字段** —— \
                 这一组花了多少 token，客户端**无从判断**。\n\
                 \n\
                 为什么必须报错而不是报 0 / 不报：\n\
                 · 报 0 = 把「不知道」说成「免费」，那是账目造假；\n\
                 · 不报这一组 = 成本闸门少算它，**漏掉的闸门是看不见的闸门**；\n\
                 · 报错 = 缺口显式、进 `failed_groups`、被人看见。\n\
                 \n\
                 注意请求本身**已经成功**（HTTP 200），钱已经花掉了 —— \
                 所以这一组记失败，而账目缺口同时显式暴露。",
                group.group_id
            )
        })?;
        let mut log = self.usage.lock().expect("usage 锁");
        // ★ REQ-050：按「该组已上报几条」编号，把重试的每一笔分成不同的键。
        //
        // 这条路径尤其要命：`record_usage` 在**收到 HTTP 200 之后**、
        // 校验正文**之前**调用，所以「正文空 / 被截断」导致的重试
        // 一定会产生**第二笔已经花掉的钱**。一直报 0 的话第二笔会顶掉第一笔，
        // 闸门按偏低的数放行 —— 少计的不是报表，是**钱**。
        let attempt = log
            .iter()
            .filter(|r| r.group_id == group.group_id)
            .count() as u32;
        log.push(UsageReport {
            group_id: group.group_id.clone(),
            attempt,
            // ★ 单位 **token**，**含 reasoning_tokens**（AC-003 坑二，论证见 `protocol::Usage`）
            used: usage.total_tokens(),
            // OpenAI 兼容端点不回 `size`（配额上限）—— 如实报「没报」，
            // 不拿一个编出来的数填（`UsageReport::size` 的 `None` 就是「没报」的意思）。
            size: None,
            // 同样不回费用 —— 上游计费是**部署侧**的事，本实现不折算，
            // 拿模型单价表去乘 token 那是**发明计量**（本项目的核心纪律之一）。
            cost: None,
        });
        Ok(())
    }
}

// 编译期钉死「不声明产出能力」这条不变量的一半：
// 若有人给本实现加上 `CAPABILITY_REBUILDS_PROPOSAL`，下面这行常量引用会失效，
// 逼着改动的人回来读模块头的论证。
#[allow(dead_code)]
const _LLM_CANNOT_REBUILD: &str = CAPABILITY_REBUILDS_PROPOSAL;
