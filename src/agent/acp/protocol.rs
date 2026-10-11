//! **ACP 的 JSON-RPC 2.0 编解码层**（ADR-002 · REQ-023）。
//!
//! # 这一层为什么与「子进程」分开
//!
//! 因为 ACP 的全部协议知识都是**字节与形状**，而子进程只是这些字节的搬运工。
//! 把两者混在一个类型里，就只能靠「真的起一个 `mcode acp`」来测协议 ——
//! 而那 11.7 秒、依赖登录态、CI 上根本不可复现（实测记录见
//! `BA/docs/reports/evidence/2026-10-08-ACP往返探测脚本.py`）。
//!
//! 所以本模块是**纯函数**：进 [`&str`] 字节，出结构体。测试喂一段真实抓下来的
//! JSON 就行，**不起进程、不碰网络、不需要登录**。
//!
//! # 三条实测事实直接约束这一层（BA · 2026-10-08 实测，不是猜的）
//!
//! | 事实 | 后果 |
//! | --- | --- |
//! | **F-1 · 答案在流式通知里，不在最终响应里** | 必须累积 [`UpdatePayload::AgentMessageChunk`]。只读终响应 ⇒ 永远拿到空答案，而「拿不到文本」极易被误当成「agent 抽不出东西」，**测试照样绿** |
//! | **F-2 · ACP 自带 `usage_update`** | 成本闸门的数据源现成，不要自己造计量 |
//! | **F-3 · `initialize` 与 `session/new` 的 result 形状不同** | 解析代码**不能**共用一套假设 |

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{MoneyAmount, UsageReport};

/// 我们声明的 ACP 协议版本（实测 `mcode acp 0.5.5` 回的就是 1）。
pub const PROTOCOL_VERSION: i64 = 1;

/// **一条流的正常结束标记**。
///
/// 只有它意味着「这一轮的正文是**完整的**」。其余取值（`max_tokens` /
/// `max_turn_requests` / `refusal` / `cancelled` …）全都表示这一轮**没说完** ——
/// 而没说完的正文拿去解析，会得到一个**截断的 JSON**，然后被当成
/// 「agent 抽不出东西」。**宁可报错，不解析半截文本**。
pub const STOP_REASON_END_TURN: &str = "end_turn";

// ──────────────────────────────────────────────────────────
// 出站：请求
// ──────────────────────────────────────────────────────────

/// 一条 JSON-RPC 2.0 **请求**（有 `id`，等一个响应）。
#[derive(Debug, Clone, Serialize)]
pub struct RpcRequest {
    pub jsonrpc: &'static str,
    pub id: i64,
    pub method: String,
    pub params: Value,
}

impl RpcRequest {
    pub fn new(id: i64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        }
    }

    /// 编码成**一行** JSON（ACP stdio 是行分隔的，不是 LSP 那种
    /// `Content-Length` 头 —— 探测脚本里 `json.dumps(obj) + "\n"` 跑通过）。
    ///
    /// 返回 `Result` 而不是 `String`：编码失败在类型上就说不通
    /// （`Value` 的 map 键必然是字符串），但让它变成一次 `Err` 比一个
    /// `panic` 便宜 —— 后者会把整个抽取进程带走。
    pub fn encode_line(&self) -> Result<String> {
        let mut s = serde_json::to_string(self).context("ACP 请求编码成 JSON 失败")?;
        s.push('\n');
        Ok(s)
    }
}

/// `initialize` 的请求参数。
pub fn initialize_params() -> Value {
    serde_json::json!({
        "protocolVersion": PROTOCOL_VERSION,
        // 本客户端**不提供**文件系统读写能力。
        //
        // 为什么显式写 `false` 而不是省略：ACP 的 `clientCapabilities` 是
        // 「客户端有什么」的声明，省略等于「什么都没有」，
        // 而让子进程以为能写文件却写不了，只会在某个深水区炸出一个看不懂的错。
        "clientCapabilities": {
            "fs": { "readTextFile": false, "writeTextFile": false }
        }
    })
}

/// `session/new` 的请求参数。
///
/// ⚠️ **`cwd` 必须存在**：`session/new` 要在那里起会话。
/// ⚠️ **这里不传 `configOptions.model`** —— ADR-002 的 N5 说
/// 「要锁模型就在建会话时固定 `configOptions.model`」，但**请求侧那个参数的
/// 确切形状本次没有实测**（BA 的探测只抓了响应侧的 `configOptions`）。
/// 发一个没量过的字段过去，是拿「协议错」赌「字段对」。
/// 响应侧的 [`NewSessionResult::config_options`] 已经原样带回来，
/// 谁要锁模型，从那里读服务端**承认**有哪些选项，再照着补请求。
pub fn new_session_params(cwd: &str) -> Value {
    serde_json::json!({
        "cwd": cwd,
        "mcpServers": []
    })
}

/// `session/prompt` 的请求参数。`prompt` 是 ACP 的内容块数组。
pub fn prompt_params(session_id: &str, text: &str) -> Value {
    serde_json::json!({
        "sessionId": session_id,
        "prompt": [{ "type": "text", "text": text }]
    })
}

// ──────────────────────────────────────────────────────────
// 入站：响应 / 通知
// ──────────────────────────────────────────────────────────

/// JSON-RPC 错误对象。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

/// 一条 JSON-RPC **响应**（有 `id`，带 `result` 或 `error`）。
///
/// ⚠️ `id` 存成 [`Value`] 而不是 `i64`：JSON-RPC 2.0 规定 `id` 可以是
/// 字符串或数字，而且「无法解析请求」时服务端必须回 `id: null`。
/// 用 `i64` 直接反序列化会让 `null` 变成一条**编解码报错**，
/// 于是「服务端看不懂我的请求」这件最有价值的事被报成「JSON 坏了」。
#[derive(Debug, Clone, Deserialize)]
pub struct RpcResponse {
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// 数字形态的请求 id（`null` / 字符串形态返回 `None`）。
    pub fn numeric_id(&self) -> Option<i64> {
        self.id.as_ref().and_then(Value::as_i64)
    }

    /// 是不是「我在等的那一条」（`id` 相等）。
    pub fn matches_id(&self, id: i64) -> bool {
        self.numeric_id() == Some(id)
    }

    /// 取 `result`；有 `error` 就报错（**fail-closed**，绝不把 `error` 当空结果）。
    pub fn into_result(self) -> Result<Value> {
        match self.error {
            Some(e) => bail!(
                "ACP 子进程返回 JSON-RPC 错误：code={} message={}{}",
                e.code,
                e.message,
                match &e.data {
                    Some(d) => format!(" data={d}"),
                    None => String::new(),
                }
            ),
            None => self
                .result
                .context("ACP 响应既没有 result 也没有 error —— 这不是合法的 JSON-RPC 响应"),
        }
    }
}

/// 一条 JSON-RPC **通知**（有 `method`，**没有** `id`，不回响应）。
#[derive(Debug, Clone, Deserialize)]
pub struct RpcNotification {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// 一条入站消息 —— 响应与通知是**两种不同的东西**。
///
/// 为什么要显式区分（而不是都叫 `Value`）：`session/prompt` 的读取循环
/// 必须**只**把匹配 `id` 的响应当成「这一轮结束了」。
/// 把通知也当成响应 ⇒ 读到第一条 `session/update` 就收工，
/// 正文一个字都没读到，而最终响应还在路上 —— 这是 F-1 之外的第二个
/// 「拿到空答案」的死法。
#[derive(Debug, Clone)]
pub enum Incoming {
    Response(RpcResponse),
    Notification(RpcNotification),
}

impl Incoming {
    pub fn as_response(&self) -> Option<&RpcResponse> {
        match self {
            Incoming::Response(r) => Some(r),
            Incoming::Notification(_) => None,
        }
    }
}

/// 解析**一行** JSON-RPC 消息。
///
/// 分派依据是**有没有 `method`**：请求/响应不会有它，通知一定会有它。
pub fn decode_line(line: &str) -> Result<Incoming> {
    let v: Value = serde_json::from_str(line)
        .with_context(|| format!("ACP 收到的一行不是合法 JSON：{line}"))?;
    let obj = v
        .as_object()
        .with_context(|| format!("ACP 收到的一行不是 JSON 对象：{line}"))?;
    if let Some(method) = obj.get("method") {
        let method = method
            .as_str()
            .with_context(|| format!("ACP 通知的 method 不是字符串：{line}"))?
            .to_string();
        // 用整条 `v` 反序列化而不是手拼 —— params 的形状由它自己的类型负责。
        let n: RpcNotification = serde_json::from_value(v.clone())
            .with_context(|| format!("ACP 通知解码失败（method={method}）：{line}"))?;
        return Ok(Incoming::Notification(n));
    }
    if !(obj.contains_key("id") || obj.contains_key("result") || obj.contains_key("error")) {
        bail!(
            "ACP 收到一条既无 method 也无 id/result/error 的消息，它不是 JSON-RPC 消息：{line}"
        );
    }
    let r: RpcResponse = serde_json::from_value(v)
        .with_context(|| format!("ACP 响应解码失败（形状不对）：{line}"))?;
    Ok(Incoming::Response(r))
}

// ──────────────────────────────────────────────────────────
// ★ F-3：两条方法的结果形状**不同**，两套类型各管各的
// ──────────────────────────────────────────────────────────

/// 对端 agent 的自我介绍 —— **只在 `initialize` 的 result 里**。
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    /// ⚠️ ADR-002 · **N5**：这是 **`mcode` 二进制的版本**，
    /// **不是模型版本**。把它当模型版本来锁抽取效果，就是锁了个不相干的东西。
    /// 见 [`crate::agent::acp`] 的模块头。
    #[serde(default)]
    pub version: Option<String>,
}

/// `initialize` 的 result —— ★ F-3 **形状 A**。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    #[serde(default)]
    pub protocol_version: i64,
    #[serde(default)]
    pub agent_capabilities: Value,
    /// 没有它 [`decode_initialize`] 就报错 —— 身份必须来自真实握手（§8.4）。
    pub agent_info: AgentInfo,
}

/// `session/new` 的 result —— ★ F-3 **形状 B**。
///
/// ⚠️ **这里没有 `agentInfo`、没有 `capabilities`**（实测确认）。
/// 把它反序列化成 [`InitializeResult`] 必然失败，反过来也一样 ——
/// 这个「必然失败」由 `tests/test_acp_protocol.rs` 钉住，
/// 免得哪天有人图省事把两套类型并成一个。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionResult {
    pub session_id: String,
    #[serde(default)]
    pub modes: Option<Value>,
    /// 服务端**承认**有哪些会话配置项（ADR-002 · N5：锁模型从这里读）。
    #[serde(default)]
    pub config_options: Option<Value>,
}

/// 解 `initialize` 的 result。
pub fn decode_initialize(result: &Value) -> Result<InitializeResult> {
    serde_json::from_value(result.clone()).context(
        "initialize 的 result 解不开 —— 按实测（ADR-002 · F-3 形状 A）它必须含 agentInfo。\n\
         如果对端改了协议版本，这里会失败；**不要**改成「拿不到就填默认值」——\
         身份填默认值 = §8.4 的溯源是假的。",
    )
}

/// 解 `session/new` 的 result。
pub fn decode_new_session(result: &Value) -> Result<NewSessionResult> {
    serde_json::from_value(result.clone()).context(
        "session/new 的 result 解不开 —— 按实测（ADR-002 · F-3 形状 B）它必须含 sessionId。",
    )
}

// ──────────────────────────────────────────────────────────
// `session/update` —— ★ F-1 与 F-2 的落点
// ──────────────────────────────────────────────────────────

/// `session/update` 的 `params`。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateParams {
    #[serde(default)]
    pub session_id: Option<String>,
    pub update: UpdatePayload,
}

/// 更新体的分类。
///
/// ⚠️ `Other` 吸收一切**不认识**的 `sessionUpdate` 取值（实测里就有
/// `available_commands_update` / `session_info_update` 两条）。
/// 必须容忍：把「对端多发了一种更新」当成致命错，
/// 等于让协议向前兼容性的任何改进都能弄挂抽取。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum UpdatePayload {
    /// ★ **F-1：正文在这里，而且只在这里。**
    ///
    /// `content` 存成不透明的 [`Value`] 而不是「文本块」类型：
    /// ACP 的 content 是一个**联合类型**，非文本块（图片、resource）
    /// 不该让整条消息解码失败 —— 那样丢的不是一块内容，是**整整一轮抽取**。
    /// 文本由 [`content_text`] 单独取，取不到就是「这一块没有文本」，不是错误。
    AgentMessageChunk {
        #[serde(default)]
        message_id: Option<String>,
        #[serde(default)]
        content: Value,
    },
    /// ★ **F-2：ACP 自带用量上报** —— 成本闸门的数据源是现成的。
    UsageUpdate {
        used: u64,
        #[serde(default)]
        size: Option<u64>,
        #[serde(default)]
        cost: Option<MoneyAmount>,
    },
    /// 不认识的更新种类（`available_commands_update` / `session_info_update` /
    /// 未来的新种类 …）—— **容忍并忽略**。
    #[serde(other)]
    Other,
}

/// 从 ACP 的 `content` 里取出全部文本片段。
///
/// 接受三种实测/协议上可能的形状，返回**全部**文本（而不是第一条）：
///
/// | 形状 | 说明 |
/// | --- | --- |
/// | `{"type":"text","text":"…"}` | 实测到的形状 |
/// | `[{"type":"text","text":"…"}, …]` | content 是内容块数组（chunk 可以分片） |
/// | 其它 / 非文本块 | 返回空 —— **不是错误** |
///
/// 返回 `Vec` 而不是 `String`：一个 `agent_message_chunk` 里有多个文本块时，
/// 只取第一条就是**静默丢字**。
pub fn content_text(content: &Value) -> Vec<String> {
    fn one(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Array(items) => items.iter().for_each(|i| one(i, out)),
            Value::Object(_) => {
                if let Some(t) = v.get("text").and_then(Value::as_str) {
                    out.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    one(content, &mut out);
    out
}

/// 解一条 `session/update` 通知的 `params`。
pub fn decode_session_update(params: &Value) -> Result<SessionUpdateParams> {
    serde_json::from_value(params.clone())
        .context("session/update 的 params 解不开（缺 update 或 sessionUpdate 字段？）")
}

/// 一条 `usage_update` 归一后的形状（去掉 JSON-RPC 包装）。
#[derive(Debug, Clone, PartialEq)]
pub struct UsageSample {
    /// **会话累计**用量 —— 注意是累计，不是这一组的增量。
    pub used: u64,
    /// 协议自报的配额上限（`usage_update.size`）。
    pub size: Option<u64>,
    /// 协议自报的费用（**会话累计**）。
    pub cost: Option<MoneyAmount>,
}

/// 一组**自己**消耗掉的量（本会话累计与上一轮累计之差）。
#[derive(Debug, Clone, PartialEq)]
pub struct UsageDelta {
    pub used: u64,
    pub size: Option<u64>,
    pub cost: Option<MoneyAmount>,
}

impl UsageSample {
    /// # ★ 记账的关键一步：会话累计 → **本组的增量**
    ///
    /// ACP 报的是**整个会话**的累计用量（实测一轮里 `used: 13993` 是会话总量）。
    /// 而 [`crate::agent::ExtractionAgent::usage`] 的契约是
    /// 「按 `group_id` 记该组的最新累计值」，编排据此**按组取最新后相加**。
    ///
    /// 于是直接报会话累计会出事：91 组各报同一个会话总量，
    /// 相加就是 **91 倍** —— 成本闸门会在 `13993/91 ≈ 154` 的地方就停下，
    /// 而账是错的。所以这里必须换算成**增量**，
    /// 相加之后才等于会话真实总量。
    ///
    /// `baseline` 是本轮开始前最后一次看到的累计值。
    /// **首次抽取时没有 baseline**，此时按 0 起算 ——
    /// 于是第一组的数会**把会话建立的开销算进去**。
    /// 这是**故意**的：宁可多算一点（提前触发闸门），不可少算（闸门形同虚设）。
    ///
    /// 用量出现**回退**时夹到 0 并在调用方可见 —— 会话重置之类的情况，
    /// 报一个负的增量会让整个合计下溢成假的「几乎没花钱」。
    pub fn delta_since(&self, baseline: Option<&UsageSample>) -> UsageDelta {
        let used_before = baseline.map_or(0, |b| b.used);
        let used = self.used.saturating_sub(used_before);
        let cost = match (baseline.and_then(|b| b.cost.as_ref()), self.cost.as_ref()) {
            (Some(before), Some(now)) if before.currency == now.currency => {
                Some(MoneyAmount {
                    amount: (now.amount - before.amount).max(0.0),
                    currency: now.currency.clone(),
                })
            }
            // 币种变了 / 之前没报过：没法相减，如实说「这一组的费用不知道」。
            // 宁可 `None`（闸门少一道）也不猜一个数 —— 猜出来的钱是假的账。
            _ => self.cost.as_ref().map(|c| {
                let base = baseline.map_or(0.0, |b| {
                    b.cost
                        .as_ref()
                        .filter(|bc| bc.currency == c.currency)
                        .map_or(0.0, |bc| bc.amount)
                });
                MoneyAmount {
                    amount: (c.amount - base).max(0.0),
                    currency: c.currency.clone(),
                }
            }),
        };
        UsageDelta {
            used,
            size: self.size,
            cost,
        }
    }

    /// 增量 → REQ-022 的协议中立上报（`UsageReport`）。
    ///
    /// `attempt` 是**调用方**给的「该组第几次尝试」—— 协议层数不出来
    /// （它只看见会话累计），而编排要靠它把重试的每一笔分开记账（REQ-050）。
    pub fn delta_to_report(
        &self,
        baseline: Option<&UsageSample>,
        group_id: &str,
        attempt: u32,
    ) -> UsageReport {
        let d = self.delta_since(baseline);
        UsageReport {
            group_id: group_id.to_string(),
            attempt,
            used: d.used,
            size: d.size,
            cost: d.cost,
        }
    }
}

// ──────────────────────────────────────────────────────────
// ★ F-1 的落点：一轮 prompt 的累积器
// ──────────────────────────────────────────────────────────

/// 一轮 `session/prompt` 结束后的完整结果。
#[derive(Debug, Clone, PartialEq)]
pub struct TurnOutcome {
    /// **累积**起来的正文（来自 `agent_message_chunk`）。
    pub text: String,
    /// 累积了几条 chunk —— 只为诊断与测试断言（「正文确实是攒出来的」）。
    pub chunks: usize,
    /// 本轮最后一条 `usage_update`（**会话累计值**，不是增量）。
    pub usage: Option<UsageSample>,
    /// 终响应里的 `stopReason`。
    pub stop_reason: String,
}

/// # ★ 一轮 prompt 的累积器 —— **F-1 在代码里的唯一落点**
///
/// # 为什么它必须存在，而不是「读终响应就完了」
///
/// 实测（ADR-002 · F-1）里 `session/prompt` 的终响应是：
///
/// ```text
/// {"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}
/// ```
///
/// **正文一个字都没有。** 正文在 9 条 `session/update` 通知的
/// `agent_message_chunk` 里。而编排看到的契约是「一个组的抽取结果」，
/// 它不知道也不该知道 ACP 的流式细节。
///
/// 于是这个类型就是那条**责任边界**：协议层负责把散在通知里的正文
/// **攒成一个字符串**再交出去。没有它，唯一的做法就是只读终响应 ——
/// 拿到空文本，解析失败，报「这一组抽不出东西」。
///
/// ⚠️ **这个 bug 极其难被发现**：空答案长得**和「agent 真的什么都没抽到」一模一样**
/// —— 都表现为「这一组失败」。所以判据不能是「调用成功了吗」，
/// 必须是「累积到的文本对不对」，见
/// `tests/test_acp_protocol.rs::accumulated_chunks_are_the_only_source_of_the_answer`。
#[derive(Debug, Default)]
pub struct PromptAccumulator {
    text: String,
    chunks: usize,
    usage: Option<UsageSample>,
    stop_reason: Option<String>,
    /// 吸收了几条通知（含不认识种类的）—— 只为诊断。
    notifications: usize,
    /// 因为 id 对不上而**丢掉**的过期响应（超时重同步会撞上）。
    stale_responses: usize,
}

impl PromptAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 吸收一条入站消息。**响应由调用方判 id**，只有通知进这里累积。
    pub fn absorb(&mut self, msg: &Incoming) -> Result<()> {
        match msg {
            Incoming::Response(_) => Ok(()),
            Incoming::Notification(n) => {
                if n.method != "session/update" {
                    // 未知的通知方法：容忍（协议向前兼容）。
                    return Ok(());
                }
                let params = decode_session_update(&n.params)?;
                self.notifications += 1;
                match params.update {
                    UpdatePayload::AgentMessageChunk { content, .. } => {
                        // ★ 只有 `agent_message_chunk` 进正文。
                        //
                        // 为什么不是「凡是带 text 的更新都进」：
                        // ACP 另有 `agent_thought_chunk`（模型的**思考过程**）。
                        // 把思考过程也算进正文 = 让抽取结果被模型的内心戏污染，
                        // 而 JSON 解析器分不出哪段是答案、哪段是自言自语。
                        let parts = content_text(&content);
                        if !parts.is_empty() {
                            self.chunks += 1;
                            for p in parts {
                                self.text.push_str(&p);
                            }
                        }
                    }
                    UpdatePayload::UsageUpdate { used, size, cost } => {
                        // 同一条会话里可能收到多次；**最后一条**才是这一轮结束时的累计值。
                        self.usage = Some(UsageSample {
                            used,
                            size,
                            cost,
                        });
                    }
                    UpdatePayload::Other => {}
                }
                Ok(())
            }
        }
    }

    /// 记下一条**过期**响应（`id` 对不上当前这一轮）。
    ///
    /// 超时后重同步会撞上这种东西：上一轮迟到的那条 `{"id":3,…}` 正好
    /// 出现在下一轮的流里。不认 id 就一定会把它当成「这一轮结束了」——
    /// 而那一轮其实一个字都还没说完。**JSON-RPC 的 `id` 就是给这个用的。**
    pub fn note_stale_response(&mut self) {
        self.stale_responses += 1;
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn chunks(&self) -> usize {
        self.chunks
    }
    pub fn notifications(&self) -> usize {
        self.notifications
    }
    pub fn stale_responses(&self) -> usize {
        self.stale_responses
    }
    /// 本轮最后一条 `usage_update`（**会话累计值**）。
    pub fn usage(&self) -> Option<&UsageSample> {
        self.usage.as_ref()
    }
    pub fn stop_reason(&self) -> Option<&str> {
        self.stop_reason.as_deref()
    }

    /// 收尾。`response` 是**匹配上 id 的那条终响应**。
    ///
    /// # 三道 fail-closed 检查，每一道都对应一种「看起来像成功」的死法
    ///
    /// 1. **没有终响应** ⇒ 这一轮没结束，不知道正文完不完整。
    /// 2. **`stopReason` 不是 `end_turn`** ⇒ 这一轮**没说完**。
    ///    `max_tokens` 的正文是**截断的 JSON**，解析它会得到一个
    ///    「看起来合法但内容残缺」的结果 —— 那比报错糟得多：
    ///    报错会进 `failed_groups` 被人看见，截断的 JSON 会变成一份
    ///    **少了一半属性的提案**，而且没人知道。
    /// 3. **正文为空** ⇒ 一个字符都没回。这一条**专门**用来把 F-1 的失败
    ///    从「静默的空抽取」变成「一条讲清原因的报错」——
    ///    如果实现哪天真退化成只读终响应，这里立刻炸。
    pub fn finish(mut self, response: RpcResponse) -> Result<TurnOutcome> {
        let stop = response
            .into_result()?
            .get("stopReason")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .context("终响应的 result 里没有 stopReason —— 无法判断这一轮说没说完")?;
        self.stop_reason = Some(stop.clone());

        if stop != STOP_REASON_END_TURN {
            bail!(
                "ACP 这一轮以 stopReason={stop:?} 结束，不是 {STOP_REASON_END_TURN:?} —— \
                 正文**没说完**（累积到 {chunks} 条 chunk / {len} 字）。\n\
                 \n\
                 为什么必须报错而不是照样解析：`max_tokens` 之类的结束原因意味着正文是\
                 **截断的 JSON**，而截断的 JSON 有时仍能解析成功 —— 那种结果会变成一份\
                 **静默缺了一半属性**的提案，比整组失败更难发现。\n\
                 \n\
                 截断的原文已保留在错误里（如需排查请重跑并开 debug）。",
                chunks = self.chunks,
                len = self.text.chars().count(),
            );
        }

        if self.text.trim().is_empty() {
            bail!(
                "ACP 这一轮正常结束（stopReason={stop:?}），但**一个正文字符都没收到**\
                 （收到 {notifications} 条通知 / {chunks} 条带文本的 chunk）。\n\
                 \n\
                 ⚠️ 这一条专门用来把「读不到正文」变成**响亮的失败**。\n\
                 按 ADR-002 的 F-1，正文只在 `session/update` → `agent_message_chunk` 里，\
                 终响应只有 stopReason。**若你看到这条错，先查累积逻辑有没有被摘掉。**\n\
                 \n\
                 它必须显式炸出来，而不是安静地返回空文本：空文本会被解析成\
                 「这一组抽不出东西」，与「agent 真的什么都没抽到」**长得一模一样**。",
                notifications = self.notifications,
                chunks = self.chunks,
            );
        }

        Ok(TurnOutcome {
            text: self.text,
            chunks: self.chunks,
            usage: self.usage,
            stop_reason: stop,
        })
    }
}