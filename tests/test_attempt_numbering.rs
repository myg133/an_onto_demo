//! REQ-051 · 真实 adapter 的 `attempt` 编号覆盖 + 编排层碰撞检测
//!
//! # 这个文件钉住什么
//!
//! | AC | 判据 |
//! | --- | --- |
//! | **AC-001** | 同组重试时**真实** `LlmAgent` / `AcpAgent` 的 `attempt` 递增（0、1），两笔 `used` **逐笔进账** |
//! | **AC-002** | 编排层：同一 `(group_id, attempt)` 撞出**不同**用量 ⇒ 报错并停 |
//! | **AC-003** | 同键**同值**（快照重读）⇒ **放行**；同键**异值** ⇒ **触发** |
//!
//! # 根因：REQ-050 的 7 条 AC 测试用的全是**替身**
//!
//! `RetryingUsageAgent` 自己重写了一遍编号逻辑，`test_acp_protocol.rs` 直接把
//! `0` 当字面量传给 `delta_to_report` ⇒ **两个真实 adapter 的编号代码一行都没被测到**。
//! 于是变异 M3（`LlmAgent` 恒报 0）、M3b（`AcpAgent` 恒报 0）、M4（ACP 错位 +1）
//! **168 条测试全绿** —— 判据不是不够强，是**根本不存在**。
//!
//! 本文件全部走**真实 adapter**（`ScriptedTransport` / `ScriptedTransport` ACP），
//! 编号逻辑跑的是 `AcpAgent::record_usage` / `LlmAgent::record_usage` 的**真实代码**。
//!
//! **零 I/O、零花费**：响应是构造出来的，两个 adapter 都留了传输层注入口
//! （`transport.rs` 模块头：「协议层零花费测、adapter 层用 `ScriptedTransport` 测」）。

use std::sync::Mutex;

use ontok::agent::acp::{
    AcpAgent, AgentInfo, Handshake, NewSessionResult, ScriptedTransport as AcpScripted,
    TurnOutcome, UsageSample,
};
use ontok::agent::llm::protocol::{Choice, Message_, Usage as LlmUsage};
use ontok::agent::llm::{ChatResponse, LlmAgent, ScriptedTransport as LlmScripted};
use ontok::agent::orchestrate::{orchestrate, OrchestrationInput};
use ontok::agent::registry::AgentIdentity;
use ontok::agent::{
    ColumnSample, DecisionList, ExtractionAgent, ExtractionGroup, GroupExtraction,
    GroupSource, MoneyAmount, UsageBudget, UsageReport, CAPABILITY_PRODUCES_PROPOSAL,
    CAPABILITY_REPORTS_USAGE,
};
use ontok::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
use ontok::extract::names::TableNames;
use serde_json::json;

// ──────────────────────────────────────────────────────────
// 造数据（与 `test_budget_retry_accounting.rs` 同一套 schema 形状）
// ──────────────────────────────────────────────────────────

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req051-test",
  "names": { "t_a": "甲表", "t_b": "乙表" }
}"#;

fn names() -> TableNames {
    serde_json::from_str(NAMES_JSON).expect("名字表应当可解析")
}

fn col(name: &str) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        data_type: "text".into(),
        nullable: true,
        comment: Some(format!("{name} 列")),
        is_primary_key: false,
    }
}

fn table(name: &str) -> TableMeta {
    let columns = vec![col("id"), col("label")];
    TableMeta {
        name: name.to_string(),
        comment: Some(format!("{name} 表")),
        column_count: columns.len(),
        commented_count: columns.len(),
        columns,
    }
}

fn schema() -> SchemaReport {
    let tables = vec![table("t_a"), table("t_b")];
    let column_count: usize = tables.iter().map(|t| t.columns.len()).sum();
    let commented_column_count: usize = tables.iter().map(|t| t.commented_count).sum();
    SchemaReport {
        server_version: "PostgreSQL 17.2".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: tables.len(),
        column_count,
        commented_column_count,
        foreign_key_count: 0,
        tables,
        introspected_at: "2026-10-10T00:00:00Z".into(),
    }
}

fn group_for(table_name: &str) -> ExtractionGroup {
    ExtractionGroup {
        group_id: format!("table:{table_name}"),
        source: GroupSource {
            schema: "public".into(),
            introspected_at: "2026-10-10T00:00:00Z".into(),
            server_version: "PostgreSQL 17.2".into(),
            transaction_read_only: "on".into(),
            foreign_key_count: 0,
        },
        schema: vec![table(table_name)],
        context_tables: vec![],
        samples: vec![ColumnSample {
            table: table_name.into(),
            column: "label".into(),
            values: vec!["示例".into()],
        }],
        constraints: DecisionList {
            forbidden_terms: vec!["内部代号".into()],
            require_verifiable_evidence: true,
            min_sample_match: 1,
        },
        domain_hints: vec![],
        existing_types: Vec::new(),
    }
}

/// 一条**合法**的候选正文（来源列真实存在于本组 schema，阶段 2 才核得过）。
fn good_body(table_name: &str) -> String {
    json!({
        "types": [{
            "key": table_name,
            "display_name": "甲",
            "table": table_name,
            "properties": [
                {"name": "标识", "table": table_name, "column": "id"},
                {"name": "标签", "table": table_name, "column": "label"}
            ]
        }],
        "links": []
    })
    .to_string()
}

// ──────────────────────────────────────────────────────────
// LLM 侧的构造
// ──────────────────────────────────────────────────────────

fn llm_resp(body: Option<&str>, total_tokens: u64) -> ChatResponse {
    ChatResponse {
        choices: vec![Choice {
            message: Message_ {
                content: body.map(|b| b.to_string()),
                reasoning_content: Some("先看 schema…".into()),
            },
            finish_reason: Some("stop".into()),
        }],
        usage: Some(LlmUsage {
            prompt_tokens: total_tokens.saturating_sub(1),
            completion_tokens: 1,
            total_tokens: Some(total_tokens),
            completion_tokens_details: None,
        }),
    }
}

fn llm_agent(turns: Vec<anyhow::Result<ChatResponse>>) -> LlmAgent {
    LlmAgent::with_transport(
        Box::new(LlmScripted::new(turns)),
        "vendor-llm",
        "1.0.0",
        "test-model",
        300,
        1,
    )
    .expect("agent 应可造出")
}

// ──────────────────────────────────────────────────────────
// ACP 侧的构造
// ──────────────────────────────────────────────────────────

fn handshake() -> Handshake {
    Handshake {
        protocol_version: 1,
        agent_info: AgentInfo {
            name: "minimax-code".into(),
            title: Some("MiniMax Code".into()),
            version: Some("0.5.5".into()),
        },
        agent_capabilities: json!({"loadSession": true}),
        session: NewSessionResult {
            session_id: "mvs_test".into(),
            modes: None,
            config_options: None,
        },
    }
}

fn acp_turn(text: &str, used: u64) -> TurnOutcome {
    TurnOutcome {
        text: text.to_string(),
        chunks: 1,
        // ⚠️ ACP 报的是**会话累计**，不是这一组的增量 ——
        // 换算成增量是 `UsageSample::delta_since` 的职责（REQ-022 的老坑）。
        usage: Some(UsageSample {
            used,
            size: Some(1_000_000),
            cost: None,
        }),
        stop_reason: "end_turn".into(),
    }
}

fn acp_agent(turns: Vec<anyhow::Result<TurnOutcome>>) -> AcpAgent {
    AcpAgent::with_transport(Box::new(AcpScripted::new(handshake(), turns)), 1)
        .expect("握手应当成功")
}

/// 抽取**必定失败**的正文 —— 让 `drive_group` 真的去重试。
///
/// 形态照抄坑一：HTTP 200 / `session/prompt` 正常返回，但正文不是候选 JSON
/// ⇒ 解析失败 ⇒ 重试，而**钱已经花掉了**（两个 adapter 都是「先记账，再解析」）。
const BAD_BODY: &str = "这不是 JSON，正文被截断了";

/// 只补一个能力标签的薄包装 —— 让真实 adapter 能进 `orchestrate`。
///
/// # 纪律：包装**只**碰 `capabilities`
///
/// `extract_group` 与 `usage` **原样转发**给被包装的真实 adapter，
/// 所以编号逻辑（`record_usage`）跑的是**真实代码**，一分钱都没替。
/// 它补的那个 `produces_proposal` 标签是**编排入口的契约**要求
/// （真实 adapter 刻意不声明它 —— 见 `acp/mod.rs` 的「静默假溯源」论证），
/// 与本文件要测的编号逻辑无关。
struct Proposing(Box<dyn ExtractionAgent>);

impl ExtractionAgent for Proposing {
    fn identity(&self) -> &AgentIdentity {
        self.0.identity()
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            CAPABILITY_PRODUCES_PROPOSAL.to_string(),
            CAPABILITY_REPORTS_USAGE.to_string(),
        ]
    }

    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        self.0.extract_group(g)
    }

    fn usage(&self) -> Vec<UsageReport> {
        self.0.usage()
    }
}

/// 上限放到 1000，先把闸门排除掉，单独看账。
fn run_with(agent: &dyn ExtractionAgent) -> ontok::agent::orchestrate::ExtractionRun {
    let (report, n) = (schema(), names());
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.budget = Some(UsageBudget { max_used: 1000, max_cost: None });
    orchestrate(agent, &report, &input).expect("不该是 Err")
}

// ──────────────────────────────────────────────────────────
// 账本期望值的**单一真相源**（REQ-054 · AC-004）
// ──────────────────────────────────────────────────────────

/// 两组三次尝试跑完之后，账上**应该**是多少 —— LLM 与 ACP 两条编排测试共用它。
///
/// 拆开就是 120（t_a 第 0 次）+ 160（t_a 第 1 次）+ 90（t_b）= **370**；
/// ACP 那边报的是**会话累计**（120 → 280 → 370），`delta_since` 差分之后同样是这三个增量。
///
/// ⚠️ 这里**刻意不写行号**（REQ-053 首稿把 `:333` / `:337` 写进了注释，
/// 加完 8 行注释后两处立刻失效）—— 锚点一律指**符号名**，行号漂移与代码重排都不怕。
const EXPECTED_USED: u64 = 370;

// ──────────────────────────────────────────────────────────
// AC-001 · 真实 LlmAgent 的编号
// ──────────────────────────────────────────────────────────

/// ★ 同组重试两次 ⇒ `attempt` 必须是 **0、1**，两笔 `used` 都在账上。
///
/// 变异 M3（`LlmAgent` 恒报 `attempt: 0`）会让这条变红：
/// 两条上报撞成同一个键，第二笔顶掉第一笔 ⇒ 账上只剩 160，而不是 280。
#[test]
fn llm_agent_numbers_every_attempt_of_the_same_group() {
    let agent = llm_agent(vec![
        // 第 0 次尝试：**钱花了**（120），正文被截断 ⇒ 解析失败
        Ok(llm_resp(None, 120)),
        // 第 1 次尝试：又花了 160，正文合法
        Ok(llm_resp(Some(&good_body("t_a")), 160)),
    ]);
    let g = group_for("t_a");

    assert!(
        agent.extract_group(&g).is_err(),
        "第 0 次尝试正文为空 ⇒ 必须失败（否则没有重试，也就没有本测试的判据）"
    );
    agent
        .extract_group(&g)
        .expect("第 1 次尝试应当抽得出来");

    let usage = agent.usage();
    assert_eq!(
        usage.len(),
        2,
        "两次尝试 = 两笔账；`record_usage` 在解析**之前**调用，\
         失败的那次也必须留下记录（钱已经花了）"
    );
    assert_eq!(
        usage.iter().map(|r| r.attempt).collect::<Vec<_>>(),
        vec![0, 1],
        "★ 同组重试必须递增编号（0、1）。恒报 0 ⇒ 第二笔顶掉第一笔 ⇒ 少计"
    );
    assert_eq!(
        usage.iter().map(|r| r.used).collect::<Vec<_>>(),
        vec![120, 160],
        "每次尝试各自的增量都要如实上报"
    );
    assert!(
        usage.iter().all(|r| r.group_id == "table:t_a"),
        "两笔都属于同一组"
    );
    assert_eq!(
        usage.iter().map(|r| r.used).sum::<u64>(),
        280,
        "逐笔进账 = 120 + 160"
    );
    // 编排的 map 键就是 (group_id, attempt) ⇒ 两个键不同 ⇒ 两笔都会被算。
    let keys: std::collections::BTreeSet<(String, u32)> = usage
        .iter()
        .map(|r| (r.group_id.clone(), r.attempt))
        .collect();
    assert_eq!(keys.len(), 2, "两个键必须互不相同，否则编排只会留最后一笔");
}

/// 真实 `LlmAgent` 走**完整编排** ⇒ 账上是 `EXPECTED_USED`（370），而不是 160。
///
/// ⚠️ **两组**：本测试那条 `run.stage.budget.used == EXPECTED_USED` 断言 =
///   第一组 120 + 160、第二组 90。
///   REQ-053 订正：原文案写「280 而不是 160」——「280」是**上一条**单组测试
///   `llm_agent_numbers_every_attempt_of_the_same_group` 的数，被错搬到了这里，
///   与同测试的断言自相矛盾。**「而不是 160」那一半是对的**，别一起改掉：
///   删掉编号后两笔撞成同一个键 ⇒ 碰撞检测**当场停** ⇒ 第二组压根没跑 ⇒
///   账上只剩最后一次读到的 160。
///   ⚠️ 也不是 250 —— 250 是「两笔都记账」那个数；理由写在**本测试**那条
///   `EXPECTED_USED` 断言的失败信息里（变异实测：删掉编号后 `left` 恒为 160）。
///   ⚠️ 本段**刻意不写行号** —— 行号会随上面的注释增删而漂移（REQ-053 首稿
///   就把 `:333` / `:337` 写进了注释，加完 8 行注释后两处立刻失效）。
///   REQ-054：锚点改成**符号名**。REQ-053 当时改成「按断言内容指代」，可
///   `used == 370` 这段内容在本文件里**出现两次**（LLM、ACP 各一条），
///   只能靠「下面那条」消歧 ⇒ 锚点**不唯一**，两处注释共用 `EXPECTED_USED` 才唯一。
#[test]
fn llm_retry_spend_reaches_the_budget_as_two_separate_entries() {
    let inner = llm_agent(vec![
        Ok(llm_resp(None, 120)),
        Ok(llm_resp(Some(&good_body("t_a")), 160)),
        Ok(llm_resp(Some(&good_body("t_b")), 90)),
    ]);
    let run = run_with(&Proposing(Box::new(inner)));

    assert_eq!(
        run.stage.budget.used,
        EXPECTED_USED,
        "第一组两次尝试 120 + 160，第二组一次 90 ⇒ {EXPECTED_USED}。\
         若编号被删（恒报 0），两笔会撞成同一个键 ⇒ 碰撞检测**当场停**、\
         第二组压根没跑 ⇒ 账上只剩**最后一次读到的 160**。\
         ⚠️ 不是 250 —— 250 是「两笔都记账」那个数，\
         而撞键之后流程已经停了，第二组那 90 根本没发生；\
         报 250 会把排障的人引到「两笔都记了账」这个错误方向上去"
    );
    assert!(
        run.stage.budget.usage_collisions.is_empty(),
        "正确编号的 adapter 不该触发碰撞检测（零假阳性）：{:?}",
        run.stage.budget.usage_collisions
    );
    assert!(
        !run.stage.budget.stopped,
        "没越闸（{EXPECTED_USED} < 1000）、没碰撞 ⇒ 不该停，实际原因：{:?}",
        run.stage.budget.reason
    );
}

// ──────────────────────────────────────────────────────────
// AC-001 · 真实 AcpAgent 的编号
// ──────────────────────────────────────────────────────────

/// ★ 同组重试两次 ⇒ `attempt` 必须是 **0、1**，两笔 `used` 都在账上。
///
/// 变异 M3b（恒报 0）与 M4（错位 +1，即两次都报 1）都会让这条变红：
/// 两次上报撞成同一个键 ⇒ 第二笔顶掉第一笔。
#[test]
fn acp_agent_numbers_every_attempt_of_the_same_group() {
    // ⚠️ 数字是**会话累计**：120 → 280 ⇒ 两次的增量分别是 120、160。
    //   （错把累计当增量报 ⇒ 第二次报 280，账会翻成 400 —— 既有套件钉着这条。）
    let agent = acp_agent(vec![
        Ok(acp_turn(BAD_BODY, 120)),
        Ok(acp_turn(&good_body("t_a"), 280)),
    ]);
    let g = group_for("t_a");

    assert!(
        agent.extract_group(&g).is_err(),
        "第 0 次尝试正文不是候选 JSON ⇒ 必须失败，才有重试"
    );
    agent
        .extract_group(&g)
        .expect("第 1 次尝试应当抽得出来");

    let usage = agent.usage();
    assert_eq!(usage.len(), 2, "两次尝试 = 两笔账");
    assert_eq!(
        usage.iter().map(|r| r.attempt).collect::<Vec<_>>(),
        vec![0, 1],
        "★ 同组重试必须递增编号（0、1）。恒报 0 或错位 +1 都会让这条变红"
    );
    assert_eq!(
        usage.iter().map(|r| r.used).collect::<Vec<_>>(),
        vec![120, 160],
        "报的是**增量**（会话累计做差）：280 − 120 = 160"
    );
    assert_eq!(
        usage.iter().map(|r| r.used).sum::<u64>(),
        280,
        "逐笔进账 = 120 + 160"
    );
    let keys: std::collections::BTreeSet<(String, u32)> = usage
        .iter()
        .map(|r| (r.group_id.clone(), r.attempt))
        .collect();
    assert_eq!(keys.len(), 2, "两个键必须互不相同");
}

/// 真实 `AcpAgent` 走**完整编排** ⇒ 账上是 `EXPECTED_USED`（370），而不是 160。
///
/// （变异实测：删掉编号后是 **160**，不是 250 —— 撞键当场停，第二组那 90 没发生。）
#[test]
fn acp_retry_spend_reaches_the_budget_as_two_separate_entries() {
    let inner = acp_agent(vec![
        Ok(acp_turn(BAD_BODY, 120)),
        Ok(acp_turn(&good_body("t_a"), 280)),
        Ok(acp_turn(&good_body("t_b"), 370)),
    ]);
    let run = run_with(&Proposing(Box::new(inner)));

    assert_eq!(
        run.stage.budget.used,
        EXPECTED_USED,
        "会话累计 370 = 三次尝试的增量 120 + 160 + 90 ⇒ 账上 {EXPECTED_USED}。\
         若编号被删（恒报 0），两笔会撞成同一个键 ⇒ 碰撞检测**当场停**、\
         第二组压根没跑 ⇒ 账上只剩**最后一次读到的 160**\
         （不是 250 —— 250 是「两笔都记账」那个数，而撞键之后流程已经停了，\
         第二组那 90 根本没发生）"
    );
    assert!(
        run.stage.budget.usage_collisions.is_empty(),
        "正确编号的 adapter 不该触发碰撞检测：{:?}",
        run.stage.budget.usage_collisions
    );
}

// ──────────────────────────────────────────────────────────
// AC-002 / AC-003 · 碰撞检测的边界
// ──────────────────────────────────────────────────────────

/// **恒报 `attempt: 0`** 的替身 —— 就是 M3 / M3b 的形状：
/// 编号永远是 0，于是同组重试的第二笔撞进第一笔的键。
///
/// 用同一个替身造出碰撞检测的两条边界，**只**靠 `vary_used` 区分 ——
/// 这样「真碰撞」与「合法同值」之间的差别就只剩**值变没变**这一个变量：
///
/// | `vary_used` | 每次尝试的 `used` | 该不该报碰撞 |
/// | --- | --- | --- |
/// | `true` | **不同**（120 → 160）| **该报** —— 两笔不同的钱挤在一个键里 |
/// | `false` | **相同**（恒为 120）| **不该报** —— 那是「同一笔被读到两次」|
struct FlatNumberingAgent {
    identity: AgentIdentity,
    log: Mutex<Vec<UsageReport>>,
    /// 第 n 次尝试上报的 `used`；`vary_used = false` 时一律取第 0 个。
    spend: Vec<u64>,
    vary_used: bool,
}

impl FlatNumberingAgent {
    fn new(spend: Vec<u64>, vary_used: bool) -> Self {
        Self {
            identity: AgentIdentity {
                agent: "vendor-flat".into(),
                version: "1.0.0".into(),
                round: 1,
            },
            log: Mutex::new(Vec::new()),
            spend,
            vary_used,
        }
    }
}

impl ExtractionAgent for FlatNumberingAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }

    fn capabilities(&self) -> Vec<String> {
        vec![
            CAPABILITY_PRODUCES_PROPOSAL.to_string(),
            CAPABILITY_REPORTS_USAGE.to_string(),
        ]
    }

    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let used = {
            let mut log = self.log.lock().expect("log 锁");
            // 每次尝试恰好记一条 ⇒ 「已记条数」就是「第几次尝试」。
            let idx = log.iter().filter(|r| r.group_id == g.group_id).count();
            let used = if self.vary_used {
                *self.spend.get(idx).unwrap_or(&self.spend[0])
            } else {
                self.spend[0]
            };
            // ★ 坏在哪：编号恒为 0（不管这是第几次尝试）。
            log.push(UsageReport {
                group_id: g.group_id.clone(),
                attempt: 0,
                used,
                size: None,
                cost: None,
            });
            used
        };
        // 这一组永远抽不出来 ⇒ `drive_group` 会重试 ⇒ 编号逻辑被真的走到。
        anyhow::bail!("模拟：这一组永远抽不出来（已花 {used}）")
    }

    fn usage(&self) -> Vec<UsageReport> {
        self.log.lock().expect("log 锁").clone()
    }
}

/// ★ AC-003 上边界：**真碰撞必须触发**（两个不同用量的同键上报）。
#[test]
fn same_key_with_a_different_value_is_a_real_collision_and_must_trip() {
    let agent = FlatNumberingAgent::new(vec![120, 160], true);
    let run = run_with(&agent);

    assert_eq!(
        run.stage.budget.usage_collisions.len(),
        1,
        "第一组重试两次，两次都是 (t_a, 0) 但用量 120 / 160 ⇒ 必须报 1 处碰撞。\
         实际：{:?}",
        run.stage.budget.usage_collisions
    );
    let msg = &run.stage.budget.usage_collisions[0];
    assert!(
        msg.contains("table:t_a") && msg.contains("120") && msg.contains("160"),
        "碰撞信息必须点名**哪一组、哪个键、两个数各是多少**，否则人看不懂：{msg}"
    );
    assert!(
        run.stage.budget.stopped,
        "账本不可信 ⇒ 必须停（继续跑只会把更多的钱花在一个算不清的账上）"
    );
    let reason = run.stage.budget.reason.clone().unwrap_or_default();
    assert!(
        reason.contains("碰撞"),
        "停的原因必须说明是碰撞，**不能**被「已用 X / 上限 Y」盖掉 —— \
         后者会把人引去调预算，而病根在 adapter。实际：{reason}"
    );
    assert_eq!(
        run.stage.budget.skipped_groups,
        vec!["table:t_b".to_string()],
        "停了之后第二组根本不该跑"
    );
}

/// ★ AC-003 下边界：**合法同键同值必须放行**（快照重读同一笔）。
///
/// 这是碰撞检测最容易误伤的地方：编排在每组之后重读整个 `usage()`，
/// 于是同一笔账会被**反复读到**。同键**同值**必须当成「同一笔」而不是碰撞。
#[test]
fn same_key_with_the_same_value_is_a_legitimate_reread_and_must_not_trip() {
    let agent = FlatNumberingAgent::new(vec![120], false);
    let run = run_with(&agent);

    assert!(
        run.stage.budget.usage_collisions.is_empty(),
        "同组同值的上报就是「同一笔被读到两次」—— 那是**合法**情形，\
         误报会让所有重试场景变红。实际：{:?}",
        run.stage.budget.usage_collisions
    );
    // 与上一条**只差一个变量**：`vary_used` 由 true 变 false。
    // 每组被抽两次，四条上报全是 (该组, 0) = 120 ⇒ 两个键、各覆盖成一条 ⇒ 120 × 2。
    assert_eq!(
        run.stage.budget.used, 240,
        "同键覆盖 ⇒ 每组只算一次：t_a 的 120 + t_b 的 120。\
         变成 480 就是把同值重读当成了两笔（翻倍误伤）"
    );
    assert!(
        !run.stage.budget.stopped,
        "没有碰撞 ⇒ 不该因碰撞停。实际原因：{:?}",
        run.stage.budget.reason
    );
}

/// 真实 adapter 在**编排的快照重读**下不误报 —— 零假阳性的最强证据。
///
/// 编排每抽完一组就把 `usage()` 整个重读一次，两组 ⇒ `absorb` 至少调 2 次，
/// 而快照是**逐次变长**的：第一读只有 1 条，第二读有 2 条。
/// 若检测把「同键第二次出现」当成碰撞，这里必红。
#[test]
fn reading_growing_snapshots_of_a_real_agent_never_trips_the_detector() {
    let inner = llm_agent(vec![
        Ok(llm_resp(Some(&good_body("t_a")), 120)),
        Ok(llm_resp(Some(&good_body("t_b")), 90)),
    ]);
    let run = run_with(&Proposing(Box::new(inner)));

    assert!(
        run.stage.budget.usage_collisions.is_empty(),
        "真实 adapter + 编排反复重读快照 ⇒ 零碰撞。实际：{:?}",
        run.stage.budget.usage_collisions
    );
    assert_eq!(
        run.stage.budget.used, 210,
        "每组各一笔：120 + 90"
    );
    assert!(
        !run.stage.budget.stopped,
        "不该停。实际原因：{:?}",
        run.stage.budget.reason
    );
}

/// 费用是 `None` 与「费用不同」也要分得清：形态不同才判碰撞。
#[test]
fn a_cost_only_difference_is_still_a_collision() {
    #[derive(Debug)]
    struct CostOnlyAgent {
        identity: AgentIdentity,
        log: Mutex<Vec<UsageReport>>,
    }

    impl ExtractionAgent for CostOnlyAgent {
        fn identity(&self) -> &AgentIdentity {
            &self.identity
        }

        fn capabilities(&self) -> Vec<String> {
            vec![
                CAPABILITY_PRODUCES_PROPOSAL.to_string(),
                CAPABILITY_REPORTS_USAGE.to_string(),
            ]
        }

        fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
            let idx = {
                let mut log = self.log.lock().expect("log 锁");
                let idx = log.iter().filter(|r| r.group_id == g.group_id).count();
                // `used` 两次都一样，**只有 cost 不同** ⇒ 仍然是两笔不同的钱。
                log.push(UsageReport {
                    group_id: g.group_id.clone(),
                    attempt: 0,
                    used: 120,
                    size: None,
                    cost: Some(MoneyAmount {
                        amount: if idx == 0 { 0.12 } else { 0.30 },
                        currency: "USD".into(),
                    }),
                });
                idx
            };
            if idx == 0 {
                anyhow::bail!("模拟：第 0 次尝试失败")
            }
            Ok(GroupExtraction {
                links: Vec::new(),
                types: Vec::new(),
            })
        }

        fn usage(&self) -> Vec<UsageReport> {
            self.log.lock().expect("log 锁").clone()
        }
    }

    let agent = CostOnlyAgent {
        identity: AgentIdentity {
            agent: "vendor-cost".into(),
            version: "1.0.0".into(),
            round: 1,
        },
        log: Mutex::new(Vec::new()),
    };
    let run = run_with(&agent);

    assert!(
        !run.stage.budget.usage_collisions.is_empty(),
        "`used` 相同但 `cost` 不同 ⇒ 仍然是**两笔不同的钱**，必须触发。\
         漏掉它等于放过一条改费用不改用量的绕行路径"
    );
}