//! **MCP 的 JSON-RPC 2.0 编解码层**（REQ-027）。
//!
//! # 为什么这一层是**纯函数**，而且必须单独放
//!
//! 因为「是不是标准 MCP」这件事，判据只能落在**字节与形状**上。
//! 如果编解码和进程、数据库缠在一起，就只能靠「真的起一个
//! Claude Desktop」来测 —— 那依赖登录态、装插件、慢，CI 上不可复现。
//! 于是本模块进 [`&str`] 字节、出 [`serde_json::Value`]，测试喂一段
//! **按规范手写**的 JSON-RPC 就行：不 起进程、不碰网络、不需要登录。
//! 与 ACP 那批（`src/agent/acp/protocol.rs`）是同一套做法。
//!
//! ⚠️ **本模块不许出现 `crate::db::` / `sqlx`**。它属于边界层，
//! 和 `src/domain/` 的 AC-002「零 I/O」是同一条纪律的延伸 ——
//! 只是这一条管的是「协议编解码不许知道数据从哪来」。

use serde_json::{json, Value};

/// **我们实现的 MCP 协议版本**。
///
/// ⚠️ 是**字符串**不是数字。规范里 `protocolVersion` 是 `"2025-06-18"`
/// 这种日期串，写成数字是自造协议，真实客户端会在版本协商阶段就断。
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// **本服务器支持的版本，从新到旧**。
///
/// 规范（Lifecycle · Version Negotiation）要求：客户端请求的版本我们支持
/// 就**原样回同一个**；不支持就回一个我们支持的（**SHOULD** 是最新的）。
/// 所以这份清单是**协商的依据**，不是装饰 —— 少了它就只有一条路：
/// 永远回固定版本，客户端拿到不支持的版本就断开。
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-06-18", "2025-03-26", "2024-11-05"];

// ──────────────────────────────────────────────────────────
// JSON-RPC 2.0 标准错误码
// ──────────────────────────────────────────────────────────

/// 请求里的 JSON 解析不出来。
pub const PARSE_ERROR: i64 = -32700;
/// 请求不是合法的 JSON-RPC 请求（缺 `jsonrpc` / `method` 等）。
pub const INVALID_REQUEST: i64 = -32600;
/// 方法不存在 —— 规范 Tools 章对「未知工具」给的例子就是这个码。
pub const METHOD_NOT_FOUND: i64 = -32601;
/// 参数不合法 —— 「Invalid arguments」用这个。
pub const INVALID_PARAMS: i64 = -32602;
/// 服务器内部错误。
pub const INTERNAL_ERROR: i64 = -32603;

// ──────────────────────────────────────────────────────────
// 入站：解码
// ──────────────────────────────────────────────────────────

/// 一条 JSON-RPC **请求**（有 `id`，要回一个响应）。
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// 保留原样（[`Value`]）而不是收窄成 `i64` ——
    /// JSON-RPC 2.0 允许 `id` 是**字符串或数字**，收窄会让字符串 id 的
    /// 客户端（如部分 SDK 的 uuid id）在回包时对不上，客户端判定失败。
    pub id: Value,
    pub method: String,
    pub params: Value,
}

/// 一条 JSON-RPC **通知**（有 `method`，**没有** `id`）。
///
/// 通知**不许回响应** —— 回了就是协议违规，真实客户端会当成额外的
/// 未知响应然后卡住等一个永远不会来的 id。
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

/// 一行入站消息。
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Request(Request),
    Notification(Notification),
    /// 不认识 / 不合法。**带上一条错误响应**，而不是静默丢弃：
    /// 静默丢弃会让客户端一直等一个永远不会来的响应。
    Invalid { id: Option<Value>, code: i64, message: String },
}

/// 解码一行 JSON-RPC。
///
/// **不做**任何协议级校验之外的判断（不认识的方法留给上层），
/// 因为「解码」和「分发」是两步：混在一起就没法用假字节测解码。
pub fn decode_line(line: &str) -> Incoming {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Incoming::Invalid {
                // 解析失败时没有 id 可用 —— JSON-RPC 规定回 `id: null`。
                id: None,
                code: PARSE_ERROR,
                message: format!("不是合法 JSON：{e}"),
            }
        }
    };

    if v.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Incoming::Invalid {
            id: v.get("id").cloned(),
            code: INVALID_REQUEST,
            message: "缺 jsonrpc:\"2.0\"".to_string(),
        };
    }

    let Some(method) = v.get("method").and_then(Value::as_str) else {
        return Incoming::Invalid {
            id: v.get("id").cloned(),
            code: INVALID_REQUEST,
            message: "缺 method —— 入站只接受请求与通知，响应不该发给服务器".to_string(),
        };
    };

    let params = v.get("params").cloned().unwrap_or_else(|| json!({}));

    match v.get("id") {
        // ⚠️ `id: null` 视作**通知**。JSON-RPC 2.0 明确说 id 为 null
        // 的请求「无法区分是通知还是 id 错了的请求」，鼓励当通知处理。
        None | Some(Value::Null) => Incoming::Notification(Notification {
            method: method.to_string(),
            params,
        }),
        Some(id) => Incoming::Request(Request {
            id: id.clone(),
            method: method.to_string(),
            params,
        }),
    }
}

// ──────────────────────────────────────────────────────────
// 出站：编码
// ──────────────────────────────────────────────────────────

/// 成功响应。**一条也是一行** —— stdio 是行分隔的。
pub fn encode_result(id: &Value, result: Value) -> String {
    // `to_string` 不会产生裸换行（换行被转义成 `\n`），
    // 满足规范「MUST NOT contain embedded newlines」。
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// 错误响应。
pub fn encode_error(id: &Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut err = json!({ "code": code, "message": message });
    if let Some(d) = data {
        err["data"] = d;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": err })
}

pub fn encode_error_line(id: &Value, code: i64, message: &str, data: Option<Value>) -> String {
    encode_error(id, code, message, data).to_string()
}

/// **无 id 的错误响应**（`id: null`）—— 解析失败时用。
///
/// JSON-RPC 2.0 要求：无法确定 id 时回 `id: null`，不许不回。
pub fn encode_error_no_id(code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": Value::Null, "error": json!({ "code": code, "message": message }) })
        .to_string()
}

/// 协商协议版本。
///
/// 规范：「服务端支持请求的版本就 MUST 回同一个；否则 MUST 回一个它支持的」。
/// 客户端不支持我们回的这个版本时会主动断开 —— 那比「回一个假的、
/// 让客户端拿着错误的假设往下走」诚实。
pub fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    match requested {
        Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => {
            // 命中的都是本文件里的字面量，`&'static` 安全。
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|s| *s == v)
                .unwrap_or(PROTOCOL_VERSION)
        }
        _ => PROTOCOL_VERSION,
    }
}

/// `initialize` 的 result。
///
/// ⚠️ `capabilities` **必须包含 `tools`** —— 规范 Tools 章：「支持 tools 的
/// 服务器 MUST 声明 tools 能力」。只声明 `tools: {}`（不写 `listChanged`）
/// 是合法的：我们不推列表变更通知。
///
/// ⚠️ **不声明 `resources` / `prompts` / `logging`**。声明了却不做，
/// 客户端会真的去发 `resources/list`，然后拿到 -32601 —— 与其让客户端
/// 撞一次，不如一开始就不声明（规范：只使用协商成功的���力）。
pub fn initialize_result(protocol_version: &str, instructions: &str) -> Value {
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "ontok",
            "title": "本体问答服务",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": instructions
    })
}

/// `tools/list` 的 result。`tools` 是每项 `{name,title,description,inputSchema}`。
pub fn tools_list_result(tools: Value) -> Value {
    json!({ "tools": tools })
}

/// `tools/call` 的**成功** result。
///
/// ⚠️ 按规范同时给 `content`（文本）与 `structuredContent`（结构化）：
/// 规范 Tool Result 章要求「返回结构化内容的工具 SHOULD 同时把序列化后的
/// JSON 放进一个 TextContent 块」。客户端与下游 LLM 两条路都能走。
pub fn tool_call_result(json_text: &str, structured: Value) -> Value {
    json!({
        "content": [ { "type": "text", "text": json_text } ],
        "structuredContent": structured,
        "isError": false
    })
}

/// `tools/call` 的**执行失败** result（`isError: true`）。
///
/// 规范区分两件事：**协议错误**（未知工具 / 参数不合法 → JSON-RPC `error`）
/// 与**执行错误**（业务上取不到 → `isError: true`）。守门器命中属于后者：
/// 连接仍然健康，只是这一问给不出数据。
pub fn tool_call_error_result(reason: &str) -> Value {
    json!({
        "content": [ { "type": "text", "text": reason } ],
        "isError": true
    })
}

/// `ping` 的 result（空对象）。
pub fn empty_result() -> Value {
    json!({})
}