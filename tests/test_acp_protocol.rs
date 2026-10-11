//! **ACP 协议层**（REQ-023 · ADR-002 F-1 / F-2 / F-3）
//!
//! # 这个文件喂的是**真实抓下来的字节**，不是手编的理想 JSON
//!
//! 来源：`BA/docs/reports/evidence/2026-10-08-ACP往返探测脚本.py`
//! （BA 用真的 `mcode acp` 驱动进程跑通的一次完整往返，耗时 11.7 秒）。
//! 手编一份「理想 JSON」当测试输入是这类协议测试**最常见的自欺**：
//! 理想形状永远解析得开，于是测试证明的是「serde 会工作」，
//! 而真正要防的是「对端的实际形状与我想的不一样」。
//!
//! ⚠️ **不许真的去调 `mcode acp`**：11.7 秒、依赖登录态、CI 不可复现。
//! 这一层是纯函数，录下来的字节就是全部的输入。

use ontok::agent::acp::{
    protocol, Incoming, PromptAccumulator, RpcResponse, UsageSample, STOP_REASON_END_TURN,
};
use ontok::agent::MoneyAmount;

// ──────────────────────────────────────────────────────────
// 真实抓下来的字节（2026-10-08，`mcode acp` 0.5.5）
// ──────────────────────────────────────────────────────────

const SID: &str = "mvs_8a03510bcef548ae84ca07ca5556214f";

/// `initialize` 的真实响应。⚠️ **身份在这里**，不在 `session/new` 里（F-3）。
const LINE_INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"mcpCapabilities":{"http":true,"sse":true},"promptCapabilities":{"image":false,"audio":false,"embeddedContext":false},"sessionCapabilities":{"list":{},"fork":{},"resume":{},"close":{}}},"agentInfo":{"name":"minimax-code","title":"MiniMax Code","version":"0.5.5"},"_meta":{"minimax-code/extensions":{}}}}"#;

/// `session/new` 的真实响应形状：⚠️ **没有 agentInfo、没有 capabilities**（F-3）。
const LINE_NEW_SESSION: &str = r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"mvs_8a03510bcef548ae84ca07ca5556214f","modes":{"currentModeId":"default"},"configOptions":{"sessionMode":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}}}}"#;

/// 实测到的第一条通知：`available_commands_update` —— **不是**正文。
const LINE_AVAILABLE_COMMANDS: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"mvs_8a03510bcef548ae84ca07ca5556214f","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}"#;

/// 实测到的第二条通知：`session_info_update` —— 同样**不是**正文。
const LINE_SESSION_INFO: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"mvs_8a03510bcef548ae84ca07ca5556214f","update":{"sessionUpdate":"session_info_update","updatedAt":"2026-10-08T04:00:00Z"}}}"#;

/// ★ **F-1 的全部所在**：正文在这一条里。
const LINE_CHUNK: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"mvs_8a03510bcef548ae84ca07ca5556214f","update":{"sessionUpdate":"agent_message_chunk","messageId":"msg_01","content":{"type":"text","text":"收到"}}}}"#;

/// ★ **F-2 的数据源**：ACP 自带的用量上报。
const LINE_USAGE: &str = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"mvs_8a03510bcef548ae84ca07ca5556214f","update":{"sessionUpdate":"usage_update","used":13993,"size":1000000,"cost":{"amount":0,"currency":"USD"}}}}"#;

/// ★ **F-1 的另一半**：终响应里**一个字都没有**，只有 `stopReason`。
const LINE_FINAL: &str = r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#;

/// 实测的完整消息序列（顺序照抄探测记录）。
const CAPTURED_TURN: &[&str] = &[
    LINE_AVAILABLE_COMMANDS,
    LINE_SESSION_INFO,
    LINE_CHUNK,
    LINE_USAGE,
    LINE_FINAL,
];

fn chunk_line(text: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{SID}","update":{{"sessionUpdate":"agent_message_chunk","messageId":"m","content":{{"type":"text","text":{}}}}}}}}}"#,
        serde_json::to_string(text).unwrap()
    )
}

fn usage_line(used: u64, cost: Option<(f64, &str)>) -> String {
    let cost = match cost {
        Some((a, c)) => format!(r#"{{"amount":{a},"currency":"{c}"}}"#),
        None => "null".to_string(),
    };
    format!(
        r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{SID}","update":{{"sessionUpdate":"usage_update","used":{used},"size":1000000,"cost":{cost}}}}}}}"#
    )
}

/// 把一串行折叠成「累积器 + 匹配上 id=3 的那条终响应」。
fn fold(lines: &[String]) -> (PromptAccumulator, RpcResponse) {
    let mut acc = PromptAccumulator::new();
    let mut final_resp = None;
    for line in lines {
        let msg = protocol::decode_line(line).expect("抓下来的字节必须解得开");
        match msg {
            Incoming::Notification(_) => acc.absorb(&msg).expect("通知必须吸收得下"),
            Incoming::Response(r) => {
                if r.matches_id(3) {
                    final_resp = Some(r);
                } else {
                    acc.note_stale_response();
                }
            }
        }
    }
    (acc, final_resp.expect("序列里必须有 id=3 的终响应"))
}

fn captured() -> Vec<String> {
    CAPTURED_TURN.iter().map(|s| s.to_string()).collect()
}

// ──────────────────────────────────────────────────────────
// ★ F-1：答案只在流式通知里 —— 本任务最关键的一条
// ──────────────────────────────────────────────────────────

/// ★ **本任务的核心判据**。
///
/// 一句话说完它防的是什么：
///
/// > ACP 的正文在 `session/update` → `agent_message_chunk` 里，
/// > **终响应只有 `stopReason`**。所以「读终响应」的实现会**永远拿到空文本**，
/// > 而空文本会被解析成「这一组抽不出东西」——
/// > **它长得和「agent 真的什么都没抽到」一模一样，测试照样绿。**
///
/// 所以判据不能是「调用成功了吗」，必须是**累积到的文本等于那句答案**。
#[test]
fn accumulated_chunks_are_the_only_source_of_the_answer() {
    let (acc, final_resp) = fold(&captured());

    // 一、终响应自己**不带正文**。这是「只读终响应 ⇒ 空答案」的前提，
    //    单独钉住：哪天真把正文放进了终响应，这条会提醒我们更新协议认知。
    let result = final_resp.result.as_ref().expect("终响应有 result");
    assert!(
        result.get("text").is_none()
            && result.get("content").is_none()
            && result.get("message").is_none(),
        "按 2026-10-08 实测，终响应只有 stopReason，实际：{result}"
    );
    assert_eq!(result["stopReason"], STOP_REASON_END_TURN);

    // 二、正文**只能**从累积器拿到，且**就是**那句答案。
    assert_eq!(
        acc.text(),
        "收到",
        "累积 `agent_message_chunk` 必须得到实测里的那句正文 —— \
         这条断言是 F-1 的判据：只读终响应的实现会在这里拿到空串并失败"
    );

    // 三、收尾之后正文仍然在手里（finish 把累积器消费掉，但正文带出去）。
    let turn = acc.finish(final_resp).expect("end_turn 应当正常收尾");
    assert_eq!(turn.text, "收到");
    assert_eq!(turn.chunks, 1, "实测那一轮只有一条带文本的 chunk");
}

/// **累积**这件事单独有判据 —— 因为「读终响应」与「读**第一条** chunk」
/// 与「读**最后一条** chunk」是**三种不同的错**，它们在这里必须一起分不出来。
#[test]
fn chunk_accumulation_concatenates_in_order_not_just_the_last_one() {
    let mut lines = captured();
    // 把「收到」拆成三条 chunk。**任何一条都不含完整答案**，
    // 所以「只取最后一条」会得到「！」而不是「收到！」。
    lines[2] = chunk_line("收");
    lines.push_before_final(chunk_line("到"));
    lines.push_before_final(chunk_line("！"));

    let (acc, final_resp) = fold(&lines);
    assert_eq!(
        acc.text(),
        "收到！",
        "chunk 必须**按到达顺序拼起来**。只取最后一条会得到「！」—— \
         而这恰恰是 F-1 之外的第二个「拿到半个答案」的死法"
    );
    assert_eq!(acc.chunks(), 3, "三条带文本的 chunk 都该被计数");
    acc.finish(final_resp).expect("应当正常收尾");
}

trait PushBeforeFinal {
    fn push_before_final(&mut self, line: String);
}

impl PushBeforeFinal for Vec<String> {
    fn push_before_final(&mut self, line: String) {
        let at = self.len() - 1; // 最后一条是终响应
        self.insert(at, line);
    }
}

/// **一个正文字符都没有** ⇒ 必须**报错**，而不是安静地返回空文本。
///
/// 这一条是 F-1 的**反向哨兵**：哪天累积逻辑被摘掉 / 对端改了形状，
/// 表现就是「正常结束但一个字没有」。它必须**响亮地炸**。
#[test]
fn a_turn_with_no_chunks_at_all_fails_loudly() {
    let lines = vec![
        LINE_AVAILABLE_COMMANDS.to_string(),
        LINE_SESSION_INFO.to_string(),
        LINE_FINAL.to_string(),
    ];
    let (acc, final_resp) = fold(&lines);
    let err = acc
        .finish(final_resp)
        .expect_err("一个正文字符都没有 ⇒ 必须报错，不许返回空文本");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("一个正文字符都没收到"),
        "错误要说清是「正文一个字都没有」，实际：{msg}"
    );
    assert!(
        msg.contains("agent_message_chunk"),
        "错误要点名正文真正的去处（ADR-002 · F-1），排查的人要能立刻定位，实际：{msg}"
    );
}

// ──────────────────────────────────────────────────────────
// ★ F-2：usage_update → UsageReport
// ──────────────────────────────────────────────────────────

/// ACP 自带的 `usage_update` 直通 REQ-022 的 `UsageReport`（**不造计量**）。
#[test]
fn usage_update_maps_onto_the_protocol_neutral_usage_report() {
    let (acc, final_resp) = fold(&captured());
    let sample = acc.usage().cloned().expect("实测那一轮报了 usage_update");

    assert_eq!(sample.used, 13993, "字段名与协议同名，不做「翻译」");
    assert_eq!(sample.size, Some(1_000_000));
    assert_eq!(
        sample.cost,
        Some(MoneyAmount {
            amount: 0.0,
            currency: "USD".into()
        }),
        "「报 0 美元」与「没报费用」是两回事，0 必须如实保留成 Some(0)"
    );

    let turn = acc.finish(final_resp).unwrap();
    assert_eq!(turn.usage.unwrap().used, 13993);
}

/// ★ **端到端**：`usage_update` 的真实字节 → 累积器 → `UsageReport`（闸门真正消费的那个类型）。
///
/// # 为什么上面那条测试**不够**
///
/// `usage_update_maps_onto_the_protocol_neutral_usage_report` 到
/// `UsageSample` 就停了，而 adapter 的成本记账是从 `UsageReport` 开始的
/// （`AcpAgent::record_usage`）。中间隔着一次 `delta_to_report`。
///
/// 变异自证跑出来的后果：把 `UpdatePayload::UsageUpdate` 的处理整段摘掉，
/// **只有那一条测试会红** —— adapter 侧的用量测试全部走 `ScriptedTransport`，
/// 它喂的是**造好的** `TurnOutcome`，绕过了协议解析，于是完全无感。
/// 也就是说「协议层解析不出用量」这件事，**没有任何一条 adapter 测试会发现**。
///
/// 这一条把两端接上：它吃**真实抓下来的字节**，吐出**闸门真正读的那个类型**。
#[test]
fn captured_usage_bytes_flow_all_the_way_into_the_budget_gates_input() {
    let (acc, final_resp) = fold(&captured());
    let turn = acc.finish(final_resp).expect("应当正常收尾");

    // 第二组：会话累计从 13993 涨到 20500（合成一条同形状的通知）
    let mut acc2 = PromptAccumulator::new();
    acc2.absorb(&protocol::decode_line(&usage_line(20500, Some((0.75, "USD")))).unwrap())
        .unwrap();
    let second = acc2.usage().cloned().unwrap();

    // 编排的 `BudgetReport::absorb` 吃的就是这两种东西
    // （两组各自的**第一次**尝试 ⇒ `attempt` 都是 0；编排靠它区分「另一笔钱」）
    let first_report = turn
        .usage
        .as_ref()
        .unwrap()
        .delta_to_report(None, "table:t_dev", 0);
    let second_report = second.delta_to_report(turn.usage.as_ref(), "table:t_customer", 0);

    assert_eq!(first_report.used, 13993);
    assert_eq!(first_report.size, Some(1_000_000));
    assert_eq!(
        first_report.cost,
        Some(MoneyAmount {
            amount: 0.0,
            currency: "USD".into()
        }),
        "「报 0 美元」必须保留成 Some(0) —— 与「没报费用」是两回事"
    );
    assert_eq!(second_report.used, 6507, "第二组报增量");
    assert_eq!(second_report.cost.as_ref().map(|c| c.amount), Some(0.75));

    // 合计 = 会话真实累计（编排就是靠这条等式判断账对不对）
    assert_eq!(first_report.used + second_report.used, 20500);
}

/// ★ **会话累计 → 每组增量**。漏了这一步，91 组会算出 **91 倍**。
///
/// 编排（`BudgetReport::absorb`）吃的是**每条增量**，
/// 而 ACP 报的是**整个会话**的累计值。直接报累计 ⇒ 总账 = 真实 × 91。
#[test]
fn session_cumulative_usage_is_converted_into_a_per_group_delta() {
    // 第一组：会话累计 13993，基线还没有 ⇒ 增量 13993
    //         （**故意**把会话建立的开销算进去：宁可多算，不可少算）
    let first = UsageSample {
        used: 13993,
        size: Some(1_000_000),
        cost: Some(MoneyAmount {
            amount: 0.5,
            currency: "USD".into(),
        }),
    };
    let r1 = first.delta_to_report(None, "table:t_dev", 0);

    // 第二组：会话累计涨到 20500 ⇒ 这一组自己用了 6507
    let second = UsageSample {
        used: 20500,
        size: Some(1_000_000),
        cost: Some(MoneyAmount {
            amount: 0.75,
            currency: "USD".into(),
        }),
    };
    let r2 = second.delta_to_report(Some(&first), "table:t_customer", 0);

    assert_eq!(r1.group_id, "table:t_dev");
    assert_eq!(r1.used, 13993, "首组无基线 ⇒ 按会话累计起算");
    assert_eq!(r1.cost.as_ref().map(|c| c.amount), Some(0.5));

    assert_eq!(
        r2.used, 6507,
        "第二组必须报**增量**而不是会话累计 —— 报 20500 的话，两组相加 34493，\
         而会话真实只用 20500，账虚高了 68%"
    );
    assert_eq!(r2.cost.as_ref().map(|c| c.amount), Some(0.25));

    // 关键性质：**各组增量之和 = 会话真实累计**。
    assert_eq!(
        r1.used + r2.used,
        second.used,
        "编排按组相加，所以增量之和必须等于会话累计，否则总账就是错的"
    );
}

/// 累计值**回退**时夹到 0 —— 报一个负的增量会让合计下溢成「几乎没花钱」。
#[test]
fn a_backwards_usage_counter_is_clamped_instead_of_underflowing() {
    let before = UsageSample {
        used: 500,
        size: None,
        cost: None,
    };
    let after = UsageSample {
        used: 10, // 会话被重置了
        size: None,
        cost: None,
    };
    assert_eq!(after.delta_since(Some(&before)).used, 0);
}

// ──────────────────────────────────────────────────────────
// ★ F-3：两条方法的结果形状不同
// ──────────────────────────────────────────────────────────

/// `initialize` 与 `session/new` 的 result **不可互换**。
///
/// 防的是「图省事把两套类型并成一个」—— 那会让 `agentInfo` 悄悄变成空，
/// 而空身份 = §8.4 的溯源是假的。
#[test]
fn initialize_and_session_new_results_are_not_interchangeable() {
    let init_resp = protocol::decode_line(LINE_INITIALIZE)
        .expect("initialize 响应解得开")
        .as_response()
        .expect("initialize 是响应不是通知")
        .clone();
    let new_resp = protocol::decode_line(LINE_NEW_SESSION)
        .expect("session/new 响应解得开")
        .as_response()
        .expect("session/new 是响应不是通知")
        .clone();

    // 身份只能从 initialize 拿。
    let init = protocol::decode_initialize(init_resp.result.as_ref().unwrap()).unwrap();
    assert_eq!(init.protocol_version, 1);
    assert_eq!(init.agent_info.name, "minimax-code");
    assert_eq!(init.agent_info.version.as_deref(), Some("0.5.5"));

    let session = protocol::decode_new_session(new_resp.result.as_ref().unwrap()).unwrap();
    assert_eq!(session.session_id, SID);

    // ★ 交叉解**必须**失败 —— 这就是 F-3 的判据。
    let a = protocol::decode_new_session(init_resp.result.as_ref().unwrap());
    assert!(
        a.is_err(),
        "initialize 的 result 里没有 sessionId，解成 session/new **必须**失败"
    );
    let b = protocol::decode_initialize(new_resp.result.as_ref().unwrap());
    assert!(
        b.is_err(),
        "session/new 的 result 里没有 agentInfo，解成 initialize **必须**失败"
    );
}

/// 未知方法的通知要**容忍** —— 协议向前兼容的任何改进都不该弄挂抽取。
#[test]
fn notifications_for_unknown_methods_are_tolerated() {
    let mut acc = PromptAccumulator::new();
    for line in [
        r#"{"jsonrpc":"2.0","method":"session/request_permission","params":{"x":1}}"#,
        r#"{"jsonrpc":"2.0","method":"session/cancel","params":{}}"#,
    ] {
        let msg = protocol::decode_line(line).unwrap();
        acc.absorb(&msg).expect("不认识的通知方法必须被容忍");
    }
    assert!(acc.text().is_empty(), "不认识的方法不该贡献正文");
}

// ──────────────────────────────────────────────────────────
// 实测里真实出现过的「不是正文的更新」必须被忽略
// ──────────────────────────────────────────────────────────

#[test]
fn the_two_non_text_updates_from_the_capture_are_ignored_not_fatal() {
    // 这两条是**实测**里真实出现过的前两条通知（available_commands / session_info）。
    // 把它们当成致命错，等于让协议加一种更新就能弄挂整轮抽取。
    let lines = captured();
    let (acc, final_resp) = fold(&lines);
    assert_eq!(
        acc.notifications(),
        4,
        "实测那一轮共 4 条通知（两条无关 + 一条正文 + 一条用量）"
    );
    assert_eq!(acc.text(), "收到", "无关的更新不许污染正文");
    acc.finish(final_resp).unwrap();
}

/// **只有** `agent_message_chunk` 进正文 —— 模型的思考过程不算答案。
///
/// ACP 另有 `agent_thought_chunk`。把它也算进去 = 让抽取结果被模型的
/// 内心戏污染，而 JSON 解析器分不出哪段是答案、哪段是自言自语。
#[test]
fn thought_chunks_are_not_treated_as_the_answer() {
    let mut acc = PromptAccumulator::new();
    for line in [
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{SID}","update":{{"sessionUpdate":"agent_thought_chunk","content":{{"type":"text","text":"我先想想 JSON 该怎么写"}}}}}}}}"#
        ),
        chunk_line(r#"{"types":[]}"#),
    ] {
        let msg = protocol::decode_line(&line).unwrap();
        acc.absorb(&msg).unwrap();
    }
    assert_eq!(
        acc.text(),
        r#"{"types":[]}"#,
        "思考过程必须被排除：混进正文会让解析器把自言自语当成数据"
    );
    assert_eq!(acc.chunks(), 1, "只有正文那条被计数");
}

/// content 是**数组**时（chunk 分片）要取**全部**文本 —— 只取第一条是静默丢字。
#[test]
fn array_content_yields_every_text_block() {
    let mut acc = PromptAccumulator::new();
    let line = format!(
        r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{SID}","update":{{"sessionUpdate":"agent_message_chunk","content":[{{"type":"text","text":"A"}},{{"type":"image","data":"…"}},{{"type":"text","text":"B"}}]}}}}}}"#
    );
    acc.absorb(&protocol::decode_line(&line).unwrap()).unwrap();
    assert_eq!(
        acc.text(),
        "AB",
        "多个文本块都要拼上，非文本块跳过 —— 只取第一条就是静默丢字"
    );
}

// ──────────────────────────────────────────────────────────
// 收尾检查：stopReason 与失败路径
// ──────────────────────────────────────────────────────────

/// ★ `stopReason` 不是 `end_turn` ⇒ **报错**，不许照样解析。
///
/// 理由：像 `max_tokens` 这种结束原因意味着正文是**截断的 JSON**，
/// 而截断的 JSON **有时仍能解析成功** —— 那种结果会变成一份
/// **静默缺了一半属性**的提案，比整组失败难发现得多。
#[test]
fn a_truncated_turn_is_rejected_instead_of_being_parsed() {
    let mut lines = captured();
    lines[2] = chunk_line(r#"{"types":[{"key":"t_dev","display_na"#); // 截断
    let last = lines.len() - 1;
    lines[last] = r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"max_tokens"}}"#.into();

    let (acc, final_resp) = fold(&lines);
    let err = acc
        .finish(final_resp)
        .expect_err("stopReason=max_tokens 必须拒绝");
    let msg = format!("{err:#}");
    assert!(msg.contains("max_tokens"), "错误要指名道姓，实际：{msg}");
    assert!(
        msg.contains("截断"),
        "错误要说清危险在于「截断的 JSON 仍可能解析成功」，实际：{msg}"
    );
}

/// 终响应里有 `error` 时不许当成空结果继续（fail-closed）。
#[test]
fn a_json_rpc_error_is_never_treated_as_an_empty_result() {
    let mut acc = PromptAccumulator::new();
    acc.absorb(&protocol::decode_line(LINE_CHUNK).unwrap()).unwrap();
    let resp = protocol::decode_line(
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32603,"message":"模型不可用","data":{"retry":false}}}"#,
    )
    .unwrap();
    let resp = resp.as_response().unwrap().clone();
    let err = acc.finish(resp).expect_err("error 响应必须报错");
    let msg = format!("{err:#}");
    assert!(msg.contains("-32603") && msg.contains("模型不可用"), "实际：{msg}");
}

/// **过期**响应（超时重同步撞上的上一轮那条）不许当成「这一轮结束了」。
#[test]
fn a_stale_response_id_is_never_mistaken_for_the_current_turn() {
    let mut acc = PromptAccumulator::new();
    acc.absorb(&protocol::decode_line(LINE_CHUNK).unwrap()).unwrap();

    // id=2 的响应属于上一轮：若认它，这一轮就被判成「已结束」。
    let stale = protocol::decode_line(r#"{"jsonrpc":"2.0","id":2,"result":{"stopReason":"end_turn"}}"#)
        .unwrap();
    let stale = stale.as_response().unwrap().clone();
    assert!(!stale.matches_id(3), "id=2 不该匹配当前这一轮（id=3）");

    acc.note_stale_response();
    assert_eq!(acc.stale_responses(), 1, "过期响应要被计数，不能默默吞掉");
    assert_eq!(acc.text(), "收到", "正文仍然只来自 chunk");
}

/// 坏行必须报错**并带出行内容** —— 不然排障时只有一句「JSON 解析失败」。
#[test]
fn a_malformed_line_is_rejected_with_the_offending_line() {
    let err = protocol::decode_line("{not json at all")
        .expect_err("坏行必须报错，不许当噪声跳过");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("{not json at all"),
        "错误里必须带出那行原文，实际：{msg}"
    );
}

/// `id: null` 的响应（服务端看不懂请求时 JSON-RPC 规定要回 null）不许炸成「JSON 坏了」。
#[test]
fn a_null_id_response_decodes_without_panicking() {
    let msg = protocol::decode_line(r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}"#)
        .expect("id:null 是合法的 JSON-RPC 响应");
    let resp = msg.as_response().expect("它仍是响应");
    assert_eq!(resp.numeric_id(), None);
    assert!(resp.error.is_some());
}

// ──────────────────────────────────────────────────────────
// 请求编码
// ──────────────────────────────────────────────────────────

#[test]
fn outgoing_requests_are_single_lines_of_json_rpc_20() {
    let req = protocol::RpcRequest::new(3, "session/prompt", protocol::prompt_params(SID, "只回两个字：收到"));
    let line = req.encode_line().unwrap();
    assert!(line.ends_with('\n'), "stdio 是行分隔的，必须带换行");
    assert_eq!(line.matches('\n').count(), 1, "**一行**，不能是 pretty-print");

    let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 3);
    assert_eq!(v["method"], "session/prompt");
    assert_eq!(v["params"]["sessionId"], SID);
    assert_eq!(v["params"]["prompt"][0]["text"], "只回两个字：收到");

    // 客户端能力**显式**声明为「不提供文件读写」—— 省略等于「什么都没有」，
    // 而让子进程以为能写文件却写不了，只会在某个深水区炸出看不懂的错。
    let init = protocol::initialize_params();
    assert_eq!(init["clientCapabilities"]["fs"]["readTextFile"], false);
    assert_eq!(init["clientCapabilities"]["fs"]["writeTextFile"], false);

    // ⚠️ N5：本 REQ **不发** configOptions.model —— 请求侧形状未实测，
    // 发一个没量过的字段是拿协议错赌字段对。
    let ns = protocol::new_session_params("/home/user/proj");
    assert_eq!(ns["cwd"], "/home/user/proj");
    assert!(
        ns.get("configOptions").is_none(),
        "未实测的 model 参数不许发出去（ADR-002 · N5）"
    );
}