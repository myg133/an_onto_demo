//! **OpenAI 兼容 `/v1/chat/completions` 的请求/响应形状**（REQ-034 · 零 I/O 纯模块）。
//!
//! # 分层
//!
//! | 模块 | 职责 | I/O |
//! | --- | --- | --- |
//! | [`super`]（本模块） | 请求体/响应体的 JSON 形状、**用量口径**、正文提取 | **零**，纯函数 |
//! | [`super::transport`] | 真的把请求发出去（`ureq` 阻塞式） | 有 |
//! | [`super`]（`LlmAgent`） | [`ExtractionAgent`] 实现、prompt 构建、候选解析 | 间接 |
//!
//! 切开的理由与 [`crate::agent::acp`] 完全一致：**协议知识全在形状里，网络只是搬运工。**
//! 本模块可以用**构造出来的 JSON** 测到 AC-003 的两个坑，
//! 而**一次网络都不用打、一次钱都不用花** —— 推理模型的空正文与 `reasoning_tokens`
//! 都是**形状问题**，不是网络问题。
//!
//! # ★ 两个坑写在这一层，不写在 adapter 层（实测依据见模块 [`super`] 的文档）
//!
//! ## 坑一：`content` **可能为空**，此时**不是成功**
//!
//! 该 model 是推理模型。`max_tokens` 给小时服务端回
//! `content: null` + `finish_reason: "length"`，token 全进了 `reasoning_content`。
//! HTTP **200**、形状完全合法 —— 若把它当成功，产出就是一份**空抽取**，
//! 而账面上「这一组成功了」。这是设计点⑤禁止的**静默跳过**，
//! 且比报错更难发现（提案看起来是完整的，内容是空的 —— 与
//! `orchestrate.rs:779` 拒绝 0 条目提案是同一类病）。
//! ⇒ [`completion_text`] 在正文为空 / `finish_reason == "length"` 时**报错**。
//!
//! ## 坑二：成本**必须含 `reasoning_tokens`**
//!
//! 实测 `reasoning_tokens: 59` / `completion_tokens: 60` ——
//! **98% 的 token 花在推理上，正文只有 1 个。**
//! 只计正文 token 会**严重低估**成本，于是成本闸门形同虚设。
//! ⇒ [`Usage::total_tokens`] 取服务端自报的 `total_tokens`（含推理），
//! 缺 `total_tokens` 时回落到 `prompt + completion`（`completion` 本身已含推理部分）。
//! 口径的完整论证见 [`Usage::total_tokens`]。

use serde::{Deserialize, Serialize};

/// 请求体 —— 只发**契约真正要用的**字段。
///
/// ⚠️ **不发没量过的字段**：ADR-002 · N3 记着 ACP 那次的教训
/// （「请求侧那个参数的形状本次没有实测，所以不发那个字段 ——
/// 发一个没量过的字段，是拿协议错赌字段对」）。同一条纪律。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    /// 模型名 —— **从环境变量来**（部署事实），不写死（AC-002）。
    pub model: String,
    pub messages: Vec<Message>,
    /// 输出上限。**推理模型必须给足**：给小了正文为空、token 全进推理（坑一）。
    ///
    /// 默认值见 [`super::LlmConfig::DEFAULT_MAX_TOKENS`] —— 那是实测能拿到正文的量级。
    pub max_tokens: u32,
}

/// 一条对话消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

/// 响应体。**只声明本实现真正读的字段**，其余一律忽略
/// （OpenAI 兼容端点之间字段差异很大，少声明一个就少一处假想的具体协议）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// **可以缺** —— 有的实现不回报量。但缺了就无法记账（见 [`Usage::total_tokens`]）。
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// 一个候选回复。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub message: Message_,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// 回复消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message_ {
    /// **可能为 `null`** —— 推理模型在 `max_tokens` 不够时就是空正文（坑一）。
    ///
    /// 所以这里是 `Option`，且 [`completion_text`] 会**显式拒绝**空正文。
    #[serde(default)]
    pub content: Option<String>,
    /// 推理内容（`max_tokens` 不够时 token 全落在这里）。**不参与抽取**，但它是
    /// 「正文为什么空」的**证据**，错误信息里要点名，让人知道不是随机故障。
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

/// 服务端自报的用量（OpenAI 兼容形状）。
///
/// # ★ `used` 的单位与算法 —— 本 REQ 的成本口径（AC-003 坑二）
///
/// | 项 | 值 |
/// | --- | --- |
/// | **单位** | **token**（与 ACP 同一单位，见 [`crate::agent::UsageReport`]） |
/// | **算法** | 服务端自报 `total_tokens`；缺则 `prompt_tokens + completion_tokens` |
/// | **含推理吗** | **含**。`completion_tokens` 本身已包含 `reasoning_tokens` |
///
/// 为什么单位与 ACP 一致（而不是另发明一套）：[`crate::agent::UsageBudget::max_used`]
/// 是**协议中立**的一个数字，它不解释单位。两侧都声明「token」，
/// 一个跨协议的 `max_used` 才有意义；若 LLM 报「请求数」而 ACP 报「token」，
/// 同一个预算字段就会在换实现时**含义漂移** —— 那比直接写错更难发现。
///
/// # 为什么优先信 `total_tokens` 而不是自己加总
///
/// `completion_tokens` 在各家实现里是否已含 `reasoning_tokens`**并无保证**
/// （有的含、有的另列 `reasoning_tokens` 字段）。自己加总就是在赌某一家的口径。
/// 服务端自己报的 `total_tokens` 是它的**账单口径**，与它实际计费的那一栏同源。
/// 实测值（`reasoning_tokens: 59` / `completion_tokens: 60`）表明本通道的
/// `completion_tokens` **已含**推理，所以 `prompt + completion` 回落路径也正确 ——
/// 但那是**这个通道**的性质，不是协议保证，故优先用服务端口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    /// 服务端账单口径的总量。**优先采用**（见类型文档）。
    #[serde(default)]
    pub total_tokens: Option<u64>,
    /// 推理 token（有些实现在 `completion_tokens` 之外另列）。
    ///
    /// **只用于诊断与自证**：它让「正文 1 个 token、推理 59 个」这件事在账上可见。
    /// 计量**不靠它加总**（口径见 [`Usage::total_tokens`]）——
    /// 若本实现改成 `completion + reasoning`，在上游 `completion` 已含推理的通道上
    /// 会**重复计一次**，账凭空翻倍。两种口径只能选一种，选错的代价都是账错。
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: u64,
}

impl Usage {
    /// **本实现上报的 `used`** —— 单位 **token**，含推理（AC-003 坑二）。
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens
            .unwrap_or(self.prompt_tokens.saturating_add(self.completion_tokens))
    }

    /// 推理 token（供诊断；**不**参与计量，见字段文档）。
    pub fn reasoning_tokens(&self) -> u64 {
        self.completion_tokens_details
            .map(|d| d.reasoning_tokens)
            .unwrap_or(0)
    }
}

/// 取出正文 —— **空正文 / 截断一律报错，不当成功**（AC-003 坑一）。
///
/// # 为什么这里必须是 `Result` 而不是 `String`
///
/// HTTP 200 + 合法 JSON + `content: null` 是**完全成功的一次 HTTP 交互**。
/// 若本函数返回 `String`（空串），调用方就会拿一份空抽取去走阶段 2，
/// 于是「这一组成功抽出了 0 个类型」与「这一组真的没有类型」**长得一模一样**。
/// 而 `extract_group` 返回 `Err` 会走 [`crate::agent::drive_group`] 的重试与
/// **显式失败**（设计点⑤），缺口会进 `extracted_by.failed_groups`，被人看见。
///
/// 这与 `orchestrate.rs:779` 拒绝「0 条目提案」是**同一类病**的两个入口：
/// 那里是整轮级，这里是组级。
pub fn completion_text(resp: &ChatResponse) -> anyhow::Result<String> {
    let choice = resp.choices.first().ok_or_else(|| {
        anyhow::anyhow!(
            "LLM 响应里 `choices` 是空的 —— HTTP 200 但没有任何候选回复。\n\
             这不是「模型没话说」，是**响应形状与本实现预期不符**：\
             若换个部署环境的端点不支持 `/v1/chat/completions`，多半会这样。\n\
             宁可在这一组失败，也不要把它当成「成功抽出了 0 个类型」。"
        )
    })?;

    // 截断先判：它是**因**，正文为空是**果**。先说因，让人知道该调大 max_tokens。
    if choice.finish_reason.as_deref() == Some("length") {
        let reasoning = choice
            .message
            .reasoning_content
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        anyhow::bail!(
            "LLM 回答被**截断**（finish_reason = \"length\"）—— 输出上限不够，\
             token 全花在了推理上、正文一个字没轮到。\n\
             \n\
             这是推理模型的已知形态（实测：max_tokens=20 时 20 个 token 全进 \
             `reasoning_content`，正文为空）。\n\
             本实现的处理：**不当成功** —— 返回空抽取会让「抽出了 0 个类型」与\
             「真的没有类型」在产物里长得一模一样（设计点⑤禁止的静默跳过）。\n\
             \n\
             怎么修：调大 max_tokens（本实现默认 {}，来自环境变量 \
             {}），或换一个推理更浅的模型。\n\
             \n\
             诊断：推理内容 {reasoning_chars} 字符。",
            super::LlmConfig::DEFAULT_MAX_TOKENS,
            super::MAX_TOKENS_ENV,
            reasoning_chars = reasoning.chars().count(),
        );
    }

    let text = choice.message.content.as_deref().unwrap_or("");
    if text.trim().is_empty() {
        // 正文为空的**另一种成因**：没截断，但正文就是空的。
        // 两种都要拒 —— 判据是「没有正文」，不是「哪一种原因」。
        anyhow::bail!(
            "LLM 响应的**正文是空的**（content 为空或 null，finish_reason = {:?}）—— \
             不当作成功。\n\
             \n\
             推理模型在输出上限不够、或答案被内容过滤掉时就是这个形态\
             （实测 max_tokens=20：正文为空，20 个 token 全进 `reasoning_content`）。\n\
             若本实现把空正文当成成功，抽取出的是一份**空抽取**，而账面上「这一组成功了」—— \
             「什么都没抽」与「抽得很全」在产物里长得一模一样。\n\
             \n\
             怎么修：调大 max_tokens（默认 {}，环境变量 {}）或换模型。\n\
             \n\
             诊断：推理内容 {reasoning_chars} 字符。",
            choice.finish_reason,
            super::LlmConfig::DEFAULT_MAX_TOKENS,
            super::MAX_TOKENS_ENV,
            reasoning_chars = choice
                .message
                .reasoning_content
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .chars()
                .count(),
        );
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp_with(content: Option<&str>, finish: Option<&str>, usage: Usage) -> ChatResponse {
        ChatResponse {
            choices: vec![Choice {
                message: Message_ {
                    content: content.map(str::to_string),
                    reasoning_content: Some("思考中…".into()),
                },
                finish_reason: finish.map(str::to_string),
            }],
            usage: Some(usage),
        }
    }

    fn usage(total: Option<u64>) -> Usage {
        Usage {
            prompt_tokens: 100,
            completion_tokens: 60,
            total_tokens: total,
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: 59,
            }),
        }
    }

    /// ★ AC-003 坑二（判别力）：实测形态 `reasoning_tokens: 59` /
    /// `completion_tokens: 60` —— 98% 的 token 花在推理上。
    /// 报出的 `used` **必须含推理**，且必须**不是**正文 token 数。
    #[test]
    fn used_counts_reasoning_tokens_not_just_visible_content() {
        let u = usage(None);
        // 回落口径 prompt + completion = 160（completion 已含那 59 个推理 token）
        assert_eq!(u.total_tokens(), 160);
        // 推理 token 单独可见（诊断用）
        assert_eq!(u.reasoning_tokens(), 59);
        // ★ 若实现改成「只计正文」，这里会是 1 —— 那就是**严重低估**成本。
        assert_ne!(u.total_tokens(), 1, "只计正文 token 会把 160 记成 1，闸门形同虚设");
    }

    /// 服务端给了 `total_tokens` 时**优先用它**，不自己加总
    /// （加总要赌上游是否已把 reasoning 计入 completion）。
    #[test]
    fn server_reported_total_wins_over_local_arithmetic() {
        let u = usage(Some(1234));
        assert_eq!(u.total_tokens(), 1234);
    }

    /// ★ AC-003 坑一（判别力）：**正文为空 + finish_reason=length**
    /// 必须是**错误**，不是成功。
    #[test]
    fn empty_content_with_length_finish_is_an_error_not_success() {
        let r = resp_with(None, Some("length"), usage(None));
        let err = completion_text(&r).expect_err("正文为空绝不能当成成功");
        let msg = format!("{err:#}");
        assert!(msg.contains("截断"), "要说清是截断，实际：{msg}");
        assert!(msg.contains("推理"), "要点名推理 token 的去向，实际：{msg}");
    }

    /// `content: null` 且 finish_reason 也是 null —— 另一种空正文形态，同样拒。
    #[test]
    fn null_content_without_length_finish_is_also_an_error() {
        let r = resp_with(None, None, usage(None));
        let err = completion_text(&r).expect_err("正文为空绝不能当成成功");
        assert!(format!("{err:#}").contains("正文是空的"));
    }

    /// 空白正文（`"   "`）同样是空 —— 判据是「没有正文」不是「是 null」。
    #[test]
    fn whitespace_only_content_is_an_error() {
        let r = resp_with(Some("   \n "), Some("stop"), usage(None));
        assert!(completion_text(&r).is_err());
    }

    /// 正常正文照常通过（防止把上面几条写成「一律失败」）。
    #[test]
    fn normal_content_passes_through() {
        let r = resp_with(Some("{\"types\":[]}"), Some("stop"), usage(None));
        assert_eq!(completion_text(&r).unwrap(), "{\"types\":[]}");
    }

    /// `choices` 为空也是形状不符，不是成功。
    #[test]
    fn empty_choices_is_an_error() {
        let r = ChatResponse {
            choices: vec![],
            usage: Some(usage(None)),
        };
        assert!(format!("{:#}", completion_text(&r).unwrap_err()).contains("choices"));
    }
}
