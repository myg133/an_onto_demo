//! **把请求发出去 / 把响应拿回来**（REQ-034 · 唯一的 I/O 层）。
//!
//! # 为什么单独一层
//!
//! [`crate::agent::acp::process`] 的分层论证在这里**原样成立**：
//! ACP 的子进程与 LLM 的 HTTP 端点都是**搬运工**，协议知识全在 [`super::protocol`]
//! 的形状里。混在一起就只能靠「真的打一次模型」测协议 —— 而那要花钱、要网络、
//! 结果还随模型版本漂移。切开之后：
//!
//! - **协议层**用构造的 JSON 测（[`super::protocol`] 的单测，**零花费零网络**）；
//! - **adapter 层**用 [`ScriptedTransport`] 测（毫秒级、不联网、不花钱）；
//! - 真要打真实端点时走 [`HttpTransport`]。
//!
//! # ★ 凭据纪律（AC-002）
//!
//! - **api key 只从环境变量来**，由 [`LlmConfig::from_env`] 读入；
//! - **base_url 由调用方从「登记的 `endpoint` 字段」传入**，本模块**不写死任何地址**；
//! - 本模块的 `Debug` 实现**刻意不打印 api key**（见 [`HttpTransport`] 的手写 impl）；
//! - 错误信息里**不回显 key**，也不用 key 拼 URL。

use std::time::Duration;

use anyhow::{bail, Context, Result};

use super::protocol::{ChatRequest, ChatResponse};

/// **一次「发出去 + 拿回来」** —— 测试替身与真 HTTP 的共同接口。
///
/// 与 [`crate::agent::acp::AcpTransport`] 同一形状：`&mut self`
/// （HTTP 连接池可复用，且「此刻只有一次往返」这件事写在类型上）。
pub trait LlmTransport: Send {
    /// 跑一次对话补全。
    fn complete(&mut self, req: &ChatRequest) -> Result<ChatResponse>;
}

// ──────────────────────────────────────────────────────────
// 真 HTTP（`ureq`，阻塞式 —— 与本 crate 的同步 trait 纪律一致）
// ──────────────────────────────────────────────────────────

/// **真的**打 OpenAI 兼容端点。
pub struct HttpTransport {
    agent: ureq::Agent,
    /// 完整 URL（`base_url` + 路径）。**来自登记**，不写死。
    url: String,
    /// ⚠️ 私有，且**手写 `Debug` 不打印它**（见 impl）。
    api_key: String,
}

/// `ureq` 的 builder 在 3.x 上是 typestate；这里只做最小封装。
fn build_ureq_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

impl HttpTransport {
    /// `base_url` **由调用方从登记的 `endpoint` 字段取**（AC-002，与 ACP 同做法）。
    pub fn new(base_url: &str, api_key: &str, timeout: Duration) -> Result<Self> {
        let base = base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            bail!("endpoint 是空的 —— base_url 是**登记的事实**，不写死也不能为空");
        }
        Ok(Self {
            agent: build_ureq_agent(timeout),
            url: format!("{base}/chat/completions"),
            api_key: api_key.to_string(),
        })
    }
}

/// ⚠️ **手写 `Debug`，且刻意不打印 `api_key`** ——
///
/// derive 出来的 `Debug` 会把 key 原样打进日志、panic 消息、测试失败输出。
/// 本项目吃过「敏感信息进日志」的亏（AC-002：口令/凭据不入日志），
/// 所以这里连字段名都不打。
impl std::fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpTransport")
            .field("url", &self.url)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl LlmTransport for HttpTransport {
    fn complete(&mut self, req: &ChatRequest) -> Result<ChatResponse> {
        let body = serde_json::to_string(req).context("请求体序列化失败")?;
        let resp = self
            .agent
            .post(&self.url)
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .send(&body)
            // ⚠️ **不回显请求体**：它含 schema 与样本值（客户数据）。
            // 这里只报 URL 与 HTTP 状态码。
            .map_err(|e| {
                anyhow::anyhow!(
                    "LLM 端点调用失败：{url} —— {kind}。\n\
                     （请求体与 api key 都不回显：前者含 schema 与样本值，后者是凭据。）",
                    url = self.url,
                    kind = match &e {
                        ureq::Error::StatusCode(c) => format!("HTTP 状态码 {c}"),
                        other => other.to_string(),
                    },
                )
            })?;

        let text = resp
            .into_body()
            .read_to_string()
            .context("读取 LLM 响应体失败")?;
        serde_json::from_str(&text).with_context(|| {
            format!(
                "LLM 响应不是预期的 OpenAI 兼容形状（截断到 500 字）：\n{}",
                &text.chars().take(500).collect::<String>()
            )
        })
    }
}

// ──────────────────────────────────────────────────────────
// 测试替身
// ──────────────────────────────────────────────────────────

/// 按脚本依次回 [`ChatResponse`] / [`Result`]。
///
/// 照抄 [`crate::agent::acp::ScriptedTransport`] 的形状（队列 + fallback）：
/// 队列用完即报错，所以「第 N 组失败」这类脚本能自然写出来；
/// [`Self::always_answer`] 给「无论问什么都这样答」的大多数用例。
pub struct ScriptedTransport {
    turns: std::sync::Mutex<std::collections::VecDeque<Result<ChatResponse>>>,
    fallback: Option<Box<dyn Fn(&ChatRequest) -> Result<ChatResponse> + Send>>,
    /// 收到过的请求 —— 测试用来断言「发出去的确实是契约里那五个输入」
    /// 以及 `max_tokens` 是不是真的发出去了（坑一的防线之一）。
    pub requests: std::sync::Mutex<Vec<ChatRequest>>,
}

impl ScriptedTransport {
    /// 按脚本依次回。
    pub fn new(turns: Vec<Result<ChatResponse>>) -> Self {
        Self {
            turns: std::sync::Mutex::new(turns.into_iter().collect()),
            fallback: None,
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 无论问什么都按同一个函数回答。
    pub fn always_answer(answer: impl Fn(&ChatRequest) -> Result<ChatResponse> + Send + 'static) -> Self {
        Self {
            turns: std::sync::Mutex::new(Default::default()),
            fallback: Some(Box::new(answer)),
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 收到过的请求条数（测试断言「调用了几次」用）。
    pub fn call_count(&self) -> usize {
        self.requests.lock().expect("requests 锁").len()
    }
}

impl LlmTransport for ScriptedTransport {
    fn complete(&mut self, req: &ChatRequest) -> Result<ChatResponse> {
        self.requests.lock().expect("requests 锁").push(req.clone());
        if let Some(next) = self.turns.lock().expect("turns 锁").pop_front() {
            return next;
        }
        match &self.fallback {
            Some(f) => f(req),
            None => bail!("ScriptedTransport 的脚本已用完，且没有设 fallback"),
        }
    }
}
