//! **内置 LLM 兜底 agent**（REQ-034）—— 工厂分派 / 两个推理模型的坑 / 成本闸门。
//!
//! # 这个文件里最要紧的是 AC-003 的两条**判别力自证**
//!
//! | 测试 | 它杀掉的是哪种错误实现 |
//! | --- | --- |
//! | [`empty_body_is_not_treated_as_success`] | 「正文为空也算成功」⇒ 空抽取冒充成功抽取（设计点⑤禁止的静默跳过）|
//! | [`usage_includes_reasoning_tokens`] | 「只计正文 token」⇒ 成本**严重低估**（实测 98% 的 token 在推理上）|
//!
//! 两条都**不联网、不花钱**：响应是构造出来的 JSON，走
//! [`ontok::agent::llm::ScriptedTransport`]。理由是这两个坑**都是形状问题**，
//! 不是网络问题 —— 打真端点只会让测试随模型版本漂移，且每次都花钱。
//!
//! ⚠️ 与 `test_acp_extraction.rs` 同样的纪律：**不起进程、不需要登录态**。

use ontok::agent::llm::protocol::{
    Choice, CompletionTokensDetails, Message_, Usage as LlmUsage,
};
use ontok::agent::llm::{ChatResponse, LlmAgent, ScriptedTransport, API_KEY_ENV, MODEL_ENV};
use ontok::agent::orchestrate::{orchestrate, OrchestrationInput};
use ontok::agent::{
    implemented_protocols, AgentRegistration, ColumnSample, DecisionList, ExistingType,
    ExtractionAgent, ExtractionGroup, GroupSource, UsageBudget,
    CAPABILITY_PRODUCES_PROPOSAL, CAPABILITY_REBUILDS_PROPOSAL, CAPABILITY_REPORTS_USAGE,
};
use ontok::extract::introspect::{ColumnMeta, TableMeta};
use ontok::extract::names::TableNames;
use serde_json::json;

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req034-test",
  "names": { "t_dev": "设备", "t_customer": "客户" }
}"#;

// ──────────────────────────────────────────────────────────
// 造数据
// ──────────────────────────────────────────────────────────

fn col(name: &str, nullable: bool, comment: Option<&str>) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        data_type: "text".into(),
        nullable,
        comment: comment.map(|s| s.to_string()),
        is_primary_key: false,
    }
}

fn table(name: &str, comment: Option<&str>, columns: Vec<ColumnMeta>) -> TableMeta {
    let column_count = columns.len();
    let commented_count = columns.iter().filter(|c| c.comment.is_some()).count();
    TableMeta {
        name: name.to_string(),
        comment: comment.map(|s| s.to_string()),
        columns,
        column_count,
        commented_count,
    }
}

fn group() -> ExtractionGroup {
    ExtractionGroup {
        group_id: "table:t_dev".into(),
        source: GroupSource {
            schema: "public".into(),
            introspected_at: "2026-10-08T04:00:00Z".into(),
            server_version: "PostgreSQL 16.14".into(),
            transaction_read_only: "on".into(),
            foreign_key_count: 1,
        },
        schema: vec![table(
            "t_dev",
            Some("设备台账"),
            vec![
                col("sn", false, Some("设备序列号")),
                col("model", true, Some("设备型号")),
            ],
        )],
        context_tables: vec!["t_dev".into(), "t_customer".into()],
        samples: vec![ColumnSample {
            table: "t_dev".into(),
            column: "model".into(),
            values: vec!["XR-200".into()],
        }],
        constraints: DecisionList {
            forbidden_terms: vec!["内部代号".into()],
            require_verifiable_evidence: true,
            min_sample_match: 1,
        },
        domain_hints: vec!["做 SaaS 计费的厂商".into()],
        existing_types: vec![ExistingType {
            key: "t_customer".into(),
            display_name: "客户".into(),
        }],
    }
}

/// 一个合法的候选 JSON（来源列真实存在于本组 schema，阶段 2 才核得过）。
fn good_body() -> String {
    json!({
        "types": [{
            "key": "t_dev",
            "display_name": "设备",
            "table": "t_dev",
            "properties": [
                {"name": "序列号", "table": "t_dev", "column": "sn"},
                {"name": "型号",   "table": "t_dev", "column": "model"}
            ]
        }],
        "links": []
    })
    .to_string()
}

fn names() -> TableNames {
    TableNames::load(NAMES_JSON).expect("翻译表应可载入")
}

/// 造一个走脚本传输的 agent（**不联网**）。
fn agent_with(turns: Vec<Result<ChatResponse, anyhow::Error>>) -> LlmAgent {
    LlmAgent::with_transport(
        Box::new(ScriptedTransport::new(turns)),
        "llm-fallback",
        "1.0.0",
        "test-reasoning-model",
        300,
        1,
    )
    .expect("agent 应可造出")
}

/// **实测形态的 usage**：`reasoning_tokens: 59` / `completion_tokens: 60`
/// ⇒ 98% 的 token 花在推理上，正文只有 1 个。
fn measured_usage() -> LlmUsage {
    LlmUsage {
        prompt_tokens: 100,
        completion_tokens: 60,
        total_tokens: None,
        completion_tokens_details: Some(CompletionTokensDetails {
            reasoning_tokens: 59,
        }),
    }
}

fn ok_resp(body: &str) -> ChatResponse {
    ChatResponse {
        choices: vec![Choice {
            message: Message_ {
                content: Some(body.to_string()),
                reasoning_content: Some("先看 schema…".into()),
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(measured_usage()),
    }
}

/// **坑一的形态**：正文为空 + `finish_reason = "length"`，token 全进推理。
fn empty_body_resp() -> ChatResponse {
    ChatResponse {
        choices: vec![Choice {
            message: Message_ {
                content: None,
                reasoning_content: Some("思考了 20 个 token…".into()),
            },
            finish_reason: Some("length".into()),
        }],
        usage: Some(LlmUsage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: Some(120),
            completion_tokens_details: Some(CompletionTokensDetails {
                reasoning_tokens: 20,
            }),
        }),
    }
}

// ──────────────────────────────────────────────────────────
// AC-001 · 作为第三个实现接入既有抽象
// ──────────────────────────────────────────────────────────

/// 工厂分派表里有 `llm`，且**共 3 个**协议。
#[test]
fn factory_knows_three_protocols_including_llm() {
    let m = implemented_protocols();
    assert_eq!(
        m.len(),
        3,
        "REQ-034 起实现了 inproc / acp / llm 三个，len()==2 编码的是 REQ-024 当时的事实，实际：{m:?}"
    );
    for p in ["inproc", "acp", "llm"] {
        assert!(m.contains_key(p), "{p} 必须在已实现协议表里，实际：{m:?}");
    }
}

/// `protocol = "llm"` + 空 endpoint ⇒ 报错（端点是登记的事实，不写死也不为空）。
///
/// ⚠️ 本测试**操作进程级环境变量**，所以与下面那条合并到同一个测试里跑 ——
/// Rust 测试默认多线程并发，改同一个变量会互相踩。
#[test]
fn llm_registration_endpoint_and_credentials_fail_closed() {
    let reg = AgentRegistration {
        agent: "llm-fallback".into(),
        version: "1.0.0".into(),
        protocol: "llm".into(),
        endpoint: None,
        capabilities: vec![],
        registered_at: "2026-10-10T00:00:00Z".into(),
    };

    // ── 情形一：有 endpoint、缺环境变量 ⇒ 必须报错，且**不回落规则实现** ──
    let reg_with_endpoint = AgentRegistration {
        endpoint: Some("https://llm-endpoint.invalid:8443/v1".into()),
        ..reg.clone()
    };
    unsafe {
        std::env::remove_var(API_KEY_ENV);
        std::env::remove_var(MODEL_ENV);
    }
    let err = ontok::agent::build_agent(&reg_with_endpoint, names(), 1)
        .err()
        .expect("缺凭据时必须报错，绝不回落规则实现");
    let msg = format!("{err:#}");
    // ⚠️ `LlmConfig::from_env` 是**逐个**检查、**头一个缺失就停**（fail-fast），
    // 所以这条只断言**第一个**缺失的变量名。`MODEL_ENV` 那条由情形二覆盖。
    assert!(
        msg.contains(API_KEY_ENV),
        "错误信息要指名缺哪个环境变量（且**不回显其值**），实际：{msg}"
    );
    // ★ **不得**把 key 的值回显进错误信息 —— 这一条钉住 AC-002 的凭据纪律。
    assert!(
        !msg.contains("dummy"),
        "错误信息绝不能回显凭据的值，实际：{msg}"
    );
    // ★ 不得**回落到规则实现**：那会让 extracted_by 记下没跑过的 agent。
    // 判据是「返回的是 Err 而不是 Ok」—— 若发生回落，`build_agent` 会返回
    // 一个 `RuleBasedAgent`，`.err()` 就是 `None`，上面那行 expect 先炸。
    assert!(
        msg.contains("没有回落到进程内规则实现"),
        "错误信息要点明没有回落，实际：{msg}"
    );

    // ── 情形二：endpoint 为空 ⇒ 必须报错（即便凭据齐了）──
    unsafe {
        std::env::set_var(API_KEY_ENV, "dummy-key-for-construction-only");
        std::env::set_var(MODEL_ENV, "dummy-model");
    }
    let err = ontok::agent::build_agent(&reg, names(), 1)
        .err()
        .expect("endpoint 为空必须报错");
    assert!(
        format!("{err:#}").contains("endpoint"),
        "错误信息要点明 endpoint 是登记的事实，实际：{err:#}"
    );

    // ── 情形二之二：有 key、**缺 model** ⇒ 必须报错并点名 MODEL_ENV ──
    unsafe {
        std::env::set_var(API_KEY_ENV, "dummy-key-for-construction-only");
        std::env::remove_var(MODEL_ENV);
    }
    let err = ontok::agent::build_agent(&reg_with_endpoint, names(), 1)
        .err()
        .expect("缺 model 时必须报错");
    let msg = format!("{err:#}");
    assert!(
        msg.contains(MODEL_ENV),
        "错误信息要指名缺 model 的那个环境变量，实际：{msg}"
    );
    assert!(
        !msg.contains("dummy-key"),
        "错误信息绝不能回显 api key 的值，实际：{msg}"
    );

    // ── 情形三：两者齐备 ⇒ 造得出实现，且身份来自**登记**（§8.4）──
    // 端点是不可达的 `.invalid` 域名，但我们**从不去连它** ——
    // 这里只验「登记 + 环境变量 ⇒ 造得出」，不验网络。
    //
    // ⚠️ 必须**重新设回** MODEL_ENV：情形二之二刚把它 remove 掉，
    // 而进程级环境变量是跨调用持久的。
    unsafe {
        std::env::set_var(API_KEY_ENV, "dummy-key-for-construction-only");
        std::env::set_var(MODEL_ENV, "dummy-model");
    }
    let built = ontok::agent::build_agent(&reg_with_endpoint, names(), 1);
    unsafe {
        std::env::remove_var(API_KEY_ENV);
        std::env::remove_var(MODEL_ENV);
    }
    let agent = built.expect("有 endpoint + 环境变量时应当造得出实现");

    assert_eq!(agent.identity().agent, "llm-fallback");
    assert_eq!(agent.identity().version, "1.0.0");

    // 能力只声明 reports_usage（论证见 llm 模块头）
    let caps = agent.capabilities();
    assert!(
        caps.iter().any(|c| c == CAPABILITY_REPORTS_USAGE),
        "LLM 实现会上报用量，必须声明 reports_usage，实际：{caps:?}"
    );
    assert!(
        !caps.iter().any(|c| c == CAPABILITY_REBUILDS_PROPOSAL),
        "★ LLM 黑盒实现**不能**声明 rebuilds_proposal（origin / rejected_comments 填不出来），实际：{caps:?}"
    );
}

/// **能力声明是纯函数**（不随这一份答案翻转）—— 与 ACP 钉的是同一条不变量。
#[test]
fn capabilities_are_static_and_never_depend_on_the_answer() {
    let good = agent_with(vec![Ok(ok_resp(&good_body()))]);
    let before = good.capabilities();
    let _ = good.extract_group(&group());
    assert_eq!(
        before,
        good.capabilities(),
        "能力标签描述「是否具备某能力」，不描述「这一份答案长什么样」"
    );

    // 一次失败之后仍然不变
    let bad = agent_with(vec![Ok(empty_body_resp())]);
    let before = bad.capabilities();
    assert!(bad.extract_group(&group()).is_err());
    assert_eq!(before, bad.capabilities(), "失败也不该翻转能力声明");
}

// ──────────────────────────────────────────────────────────
// AC-003 · 推理模型的两个坑
// ──────────────────────────────────────────────────────────

/// ★★ AC-003 坑一（判别力自证）：**正文为空 ⇒ 不当成成功**。
///
/// 杀掉的那种实现：`let text = content.unwrap_or_default()` 然后继续 ——
/// 它会产出一份**空抽取**并让这一组「成功」，于是「什么都没抽」与「抽得很全」
/// 在产物里长得一模一样（设计点⑤禁止的静默跳过）。
#[test]
fn empty_body_is_not_treated_as_success() {
    let agent = agent_with(vec![Ok(empty_body_resp())]);
    let err = agent
        .extract_group(&group())
        .expect_err("★ 正文为空绝不能当成成功 —— 那是一份空抽取冒充成功抽取");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("截断") || msg.contains("正文是空的"),
        "错误信息要点明是空正文/截断，实际：{msg}"
    );

    // ★★ 反面自证：**同一个传输在正文正常时必须成功** ——
    // 证明上面那条不是「本实现对一切输入都失败」这种无判别力的断言。
    let ok_agent = agent_with(vec![Ok(ok_resp(&good_body()))]);
    let extraction = ok_agent
        .extract_group(&group())
        .expect("正常正文必须成功（否则上一条断言没有判别力）");
    assert_eq!(extraction.types.len(), 1, "应解析出 1 个类型");
    assert_eq!(extraction.types[0].properties.len(), 2, "应解析出 2 个属性");
}

/// ★★ AC-003 坑二（判别力自证）：**`used` 必须含 `reasoning_tokens`**。
///
/// 杀掉的那种实现：`used = 正文 token 数`（或干脆不读 `completion_tokens_details`）
/// —— 实测 98% 的 token 花在推理上，只计正文会**严重低估**成本，
/// 于是成本闸门形同虚设。
#[test]
fn usage_includes_reasoning_tokens() {
    let agent = agent_with(vec![Ok(ok_resp(&good_body()))]);
    agent.extract_group(&group()).expect("应成功");

    let usage = agent.usage();
    assert_eq!(usage.len(), 1, "抽一组应上报一条用量，实际：{usage:?}");

    let reported = usage[0].used;
    // 实测形态：prompt 100 + completion 60（**含**推理 59）= 160
    assert_eq!(
        reported, 160,
        "used 应为 prompt_tokens + completion_tokens = 160（含推理），实际：{reported}"
    );
    // ★ 判别力的核心：正文只有 1 个 token。若实现只计正文，这里会是 1。
    assert_ne!(
        reported, 1,
        "★ 只计正文 token 会把 160 记成 1 —— 成本被低估 160 倍，闸门形同虚设"
    );
    // 且必须 ≥ completion_tokens（推理是 completion 的一部分）
    assert!(
        reported >= 60,
        "used 必须不小于 completion_tokens（含推理的那部分），实际：{reported}"
    );
}

/// 记账发生在**解析之前** —— 正文为空（这一组会失败）时**钱仍然进账**。
///
/// 这是「先记账，再解析」纪律（与 `AcpAgent::extract_group` 同一条）：
/// 若空正文直接 return，那笔钱永远不进账，成本闸门少算，账是平的、看不出问题。
#[test]
fn usage_is_recorded_even_when_the_body_is_empty() {
    let agent = agent_with(vec![Ok(empty_body_resp())]);
    assert!(agent.extract_group(&group()).is_err(), "本组应失败");
    let usage = agent.usage();
    assert_eq!(
        usage.len(),
        1,
        "★ 请求已成功（HTTP 200）且钱已花掉，用量必须仍然上报，否则闸门少算"
    );
    assert_eq!(usage[0].used, 120, "该响应的 total_tokens = 120");
}

/// `usage` 字段缺失 ⇒ **报错，不报 0**（fail-closed）。
#[test]
fn missing_usage_is_an_error_not_a_zero() {
    let resp = ChatResponse {
        choices: vec![Choice {
            message: Message_ {
                content: Some(good_body()),
                reasoning_content: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    };
    let agent = agent_with(vec![Ok(resp)]);
    let err = agent
        .extract_group(&group())
        .expect_err("缺 usage 必须报错：报 0 是把「不知道」说成「免费」");
    let msg = format!("{err:#}");
    assert!(msg.contains("usage"), "错误信息要点明缺 usage，实际：{msg}");
}

/// `max_tokens` **真的发出去了**（构造时就拒 0）—— 坑一的第一道防线。
#[test]
fn zero_max_tokens_is_rejected_at_construction() {
    let err = LlmAgent::with_transport(
        Box::new(ScriptedTransport::new(vec![])),
        "llm-fallback",
        "1.0.0",
        "m",
        0, // ← 输出上限为 0
        1,
    )
    .err()
    .expect("max_tokens=0 必须在构造时就拒：那一轮会每一组都失败");
    assert!(format!("{err:#}").contains("max_tokens"));
}

/// 发出去的请求**确实带着 `max_tokens` 与 model**（不是硬编码 0）。
#[test]
fn request_carries_max_tokens_and_the_model_from_config() {
    let t = ScriptedTransport::new(vec![Ok(ok_resp(&good_body()))]);
    let agent = LlmAgent::with_transport(
        Box::new(t),
        "llm-fallback",
        "1.0.0",
        "test-reasoning-model",
        300,
        1,
    )
    .expect("应可造出");
    agent.extract_group(&group()).expect("应成功");

    // transport 记下了收到的请求 —— 用 Arc 把同一个探针同时握在两侧
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = RecordingTransport {
        seen: seen.clone(),
        resp: ok_resp(&good_body()),
    };
    let agent2 = LlmAgent::with_transport(
        Box::new(recorder),
        "llm-fallback",
        "1.0.0",
        "test-reasoning-model",
        300,
        1,
    )
    .expect("应可造出");
    agent2.extract_group(&group()).expect("应成功");

    let seen = seen.lock().expect("seen 锁");
    assert_eq!(seen.len(), 1, "应恰好发出去 1 次请求，实际：{}", seen.len());
    assert_eq!(
        seen[0].max_tokens, 300,
        "★ 请求必须真的带上 max_tokens（给小了正文就是空的 —— 坑一）"
    );
    assert!(
        seen[0].max_tokens > 0,
        "★ max_tokens=0 等于把「正文为空」变成常态"
    );
    assert_eq!(
        seen[0].model, "test-reasoning-model",
        "model 应来自配置（部署事实），不写死"
    );
    // prompt 里应含规格 §7.1 阶段 1 的五个输入（复用 acp::parse::build_prompt）
    let user = seen[0]
        .messages
        .iter()
        .find(|m| m.role == "user")
        .expect("应有一条 user 消息");
    for needle in ["t_dev", "设备序列号", "XR-200", "禁用词", "t_customer"] {
        assert!(
            user.content.contains(needle),
            "prompt 应含阶段 1 的五个输入之一 {needle:?}，实际：{}",
            user.content
        );
    }
}

/// 一个把请求**录下来**再回固定响应的传输（用来断言「真的发出去什么」）。
struct RecordingTransport {
    seen: std::sync::Arc<std::sync::Mutex<Vec<ontok::agent::llm::ChatRequest>>>,
    resp: ChatResponse,
}

impl ontok::agent::llm::LlmTransport for RecordingTransport {
    fn complete(&mut self, req: &ontok::agent::llm::ChatRequest) -> anyhow::Result<ChatResponse> {
        self.seen.lock().expect("seen 锁").push(req.clone());
        Ok(self.resp.clone())
    }
}

// ──────────────────────────────────────────────────────────
// AC-004 · 成本闸门能拦住
// ──────────────────────────────────────────────────────────

/// ★ AC-004：把 `max_used` 调到**低于本次调用的实际用量** ⇒
/// **这一组失败 / 整轮停**，而非静默超额。
///
/// 杀掉的那种实现：`usage()` 返回空 vec（或者上报一个偏小的数）
/// —— 闸门于是看不到任何用量，一轮跑完也不停。
#[test]
fn max_used_below_actual_usage_stops_the_run_instead_of_silently_overspending() {
    // 单组用量的实测口径 = 160 token（prompt 100 + completion 60，含推理 59）
    let g = group();
    let agent = agent_with(vec![Ok(ok_resp(&good_body()))]);

    // ── 闸门判据本身：160 用量 vs max_used=1 ⇒ 越闸 ──
    // 先单独抽一组，让它真的上报用量（否则下面 `agent.usage()` 是空的）。
    assert!(agent.extract_group(&g).is_ok(), "正常正文应成功");
    let usage = agent.usage();
    assert_eq!(usage.len(), 1, "抽一组应上报一条用量");
    assert_eq!(usage[0].used, 160, "前置条件：这一组用了 160 token");

    let tight = UsageBudget {
        max_used: 1,
        max_cost: None,
    };
    assert!(
        tight.exceeded_by(usage[0].used, None),
        "★ max_used=1 < 实际 160，必须判为越闸 —— 否则就是静默超额"
    );
    // 对照：闸门给得够就不越（证明上面不是恒真断言）
    let roomy = UsageBudget {
        max_used: 10_000,
        max_cost: None,
    };
    assert!(
        !roomy.exceeded_by(usage[0].used, None),
        "max_used=10000 > 160 时不该越闸（否则上一条没有判别力）"
    );
}

/// 整轮里 `max_used` 给得太紧 ⇒ `skipped_groups` 非空（剩下的组**没跑**）。
///
/// 这是「闸门拦住了」的可观察后果：越闸之后编排**不再调** [`drive_group`]，
/// 于是那些组根本没发请求 —— 这正是「不静默超额」的实质。
#[test]
fn tight_budget_leaves_later_groups_skipped_rather_than_overspending() {
    let names = names();

    // ★ 必须是**两张表**：`build_groups` 按表分组（设计点②：一表一组），
    // 只有一张表 ⇒ 只有一个组 ⇒ 第一个组跑完越闸之后**没有「后面的组」可跳过**，
    // 那样 `skipped_groups` 恒为空，这条测试就没有判别力。
    let g = group();
    let mut report = g.to_schema_report();
    report.tables.push(table(
        "t_customer",
        Some("客户台账"),
        vec![col("name", false, Some("客户名称"))],
    ));

    // 入口能力闸门先于成本闸门，所以要走到阶段 1 必须补一个产出能力。
    //
    // ⚠️ 这里补的是 **`produces_proposal`**（走规则路径），**不是**
    // `rebuilds_proposal`：后者会让编排走重建路径，而 LLM 候选必然
    // 缺 `origin` / `rejected_comments` ⇒ 被**契约守卫**整轮拦下
    // （那正是 `llm` 模块头论证的缺口，与 ACP 同一个结论）。
    // 用 `produces_proposal` 只是为了让本测试**够得着成本闸门**，
    // 它不改变 LLM 实现本身的真实能力声明。
    //
    // 脚本给**两个**响应：第一个组用掉 160，第二个组**根本不该被调用** ——
    // 若闸门失效，它会被调用，而 ScriptedTransport 的队列正好能验出这一点。
    let inner = agent_with(vec![
        Ok(ok_resp(&good_body())),
        Ok(ok_resp(&good_body())),
    ]);
    let agent = CapabilityWrapper {
        inner,
        caps: vec![
            CAPABILITY_REPORTS_USAGE.to_string(),
            CAPABILITY_PRODUCES_PROPOSAL.to_string(),
        ],
    };

    let mut input = OrchestrationInput::new(&names, "extracted");
    // max_used = 1，远小于实际的 160
    input.budget = Some(UsageBudget {
        max_used: 1,
        max_cost: None,
    });

    let run = orchestrate(&agent, &report, &input).expect("能走到阶段 1");
    assert!(
        run.stage.budget.stopped,
        "★ 越闸后预算必须报 stopped，实际 used={} skipped={:?}",
        run.stage.budget.used,
        run.stage.budget.skipped_groups
    );
    assert_eq!(
        run.stage.budget.used, 160,
        "账上必须是真的 160 token（含推理），实际：{}",
        run.stage.budget.used
    );
    assert_eq!(
        run.stage.budget.skipped_groups.len(),
        1,
        "★ 越闸之后的那个组应进 skipped_groups（**没跑**），实际：{:?}",
        run.stage.budget.skipped_groups
    );
    // ★ 最强的一条：**只发出去 1 次请求**。第二个组若被调用，
    // 账上会是 320（而 `absorb` 是按组取最新值相加，320 正是「静默超额」的形态）。
    assert_eq!(
        run.stage.groups_ok, 1,
        "只应有 1 组真的跑完，实际：{}",
        run.stage.groups_ok
    );
}

/// 一个只**加能力标签**的包装器（其余全部转发）。
///
/// 用它是为了让测试走到「闸门触发」那一步，而不必让 LLM 实现去声明它
/// **不该声明**的 `rebuilds_proposal`（论证见 `llm` 模块头）。
struct CapabilityWrapper {
    inner: LlmAgent,
    caps: Vec<String>,
}

impl ExtractionAgent for CapabilityWrapper {
    fn identity(&self) -> &ontok::agent::AgentIdentity {
        self.inner.identity()
    }
    fn extract_group(
        &self,
        group: &ExtractionGroup,
    ) -> anyhow::Result<ontok::agent::GroupExtraction> {
        self.inner.extract_group(group)
    }
    fn capabilities(&self) -> Vec<String> {
        self.caps.clone()
    }
    fn usage(&self) -> Vec<ontok::agent::UsageReport> {
        self.inner.usage()
    }
}

/// 声明了 `reports_usage` 却没设预算 ⇒ 编排**入口拒绝**（fail-closed，成本闸门的既有义务）。
///
/// ⚠️ 入口两道闸的**顺序**是 `require_proposal_producing` → `require_budget`
/// （`orchestrate.rs:580-581`）。裸 LLM agent 先被能力闸门拒掉，**够不着**成本闸门，
/// 所以这里必须补一个产出能力，才能验到 `require_budget` 那一条。
#[test]
fn reporting_usage_without_a_budget_is_refused_at_the_entry() {
    let g = group();
    let report = g.to_schema_report();
    let names = names();

    let inner = agent_with(vec![Ok(ok_resp(&good_body()))]);
    let agent = CapabilityWrapper {
        inner,
        caps: vec![
            CAPABILITY_REPORTS_USAGE.to_string(),
            CAPABILITY_PRODUCES_PROPOSAL.to_string(),
        ],
    };

    let mut input = OrchestrationInput::new(&names, "extracted");
    input.budget = None; // ← 没设预算

    let res = orchestrate(&agent, &report, &input);
    let msg = match res {
        Ok(_) => panic!("★ 声明了 reports_usage 却没预算，入口必须拒绝整轮"),
        Err(e) => format!("{e:#}"),
    };
    assert!(
        msg.contains("预算"),
        "错误信息要说清「声明了会上报用量就必须给预算」，实际：{msg}"
    );
}
