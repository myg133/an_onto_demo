//! REQ-050 · 编排层同组重试**少计成本**（资金风险，不是记账瑕疵）
//!
//! # 这个文件钉住什么
//!
//! | AC | 判据 |
//! | --- | --- |
//! | **AC-001** | 同组重试两次（120 / 160）⇒ `used == 280`，**不是 160** |
//! | **AC-002** | `max_used = 200`（**夹在两次尝试之间**）⇒ 闸门**拦住**；只按 160 判就会放行 |
//! | **AC-003** | 两次尝试**币种不同** ⇒ `cost == None`（不可比 ≠ 相加） |
//! | **AC-004** | **不重试**的组，数字与改动前一致（不被这次修复带偏） |
//!
//! # 根因
//!
//! `BudgetReport` 过去按 `group_id` 建键**覆盖**（只留最后一笔）。
//! 而同组重试会产生**两笔不同的钱** —— 第一笔被静默顶掉。
//! `used` / `cost` 是**整轮累计**，直接喂给 `UsageBudget::exceeded_by`，
//! 所以少计 = **闸门按偏低的数放行**，放过真实超额。
//!
//! # 为什么不能简单改成「无脑累加每条上报」
//!
//! `agent.usage()` 返回的是**全量历史快照**，编排在每组之后重读整个列表。
//! 无脑累加会让先前的账被**反复计入**。现在靠 `(group_id, attempt)` 建键：
//! 快照重读命中同一个键 ⇒ 覆盖不翻倍；重试落到新键 ⇒ 新账进账。
//! `tests/test_agent_rebuild.rs` 里那条「同组同值重复上报不翻倍」仍在钉这件事。

mod common;

use std::sync::Mutex;

use ontok::agent::orchestrate::{orchestrate, OrchestrationInput};
use ontok::agent::registry::AgentIdentity;
use ontok::agent::{
    CandidateProperty, CandidateType, ColumnRef, ExtractionAgent, ExtractionGroup,
    GroupExtraction, MoneyAmount, PropertyOrigin, UsageBudget, UsageReport,
    CAPABILITY_PRODUCES_PROPOSAL, CAPABILITY_REPORTS_USAGE,
};
use ontok::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
use ontok::extract::names::TableNames;

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req050-test",
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

/// **会失败一次才成功的用量上报替身** —— 照抄 LLM 路径的真实形状：
/// `record_usage` 在收到 HTTP 200 之后、校验正文**之前**调用，
/// 所以「这次尝试失败」**不代表这次尝试没花钱**。
struct RetryingUsageAgent {
    identity: AgentIdentity,
    /// `group_id` → 每次尝试各自的 `used`（顺序即尝试次序）。
    spend: Vec<(String, Vec<u64>)>,
    /// 每次尝试的币种；长度 1 = 每次同币种。
    currencies: Vec<String>,
    /// 第 0 次尝试是否**报错**（钱已经记下了）。
    fail_first: bool,
    log: Mutex<Vec<UsageReport>>,
}

impl RetryingUsageAgent {
    fn new(spend: Vec<(&str, Vec<u64>)>, fail_first: bool) -> Self {
        Self {
            identity: AgentIdentity {
                agent: "vendor-retry".into(),
                version: "2026.10.10".into(),
                round: 1,
            },
            spend: spend
                .into_iter()
                .map(|(g, v)| (g.to_string(), v))
                .collect(),
            currencies: vec!["USD".into()],
            fail_first,
            log: Mutex::new(Vec::new()),
        }
    }

    fn with_currencies(mut self, currencies: &[&str]) -> Self {
        self.currencies = currencies.iter().map(|c| c.to_string()).collect();
        self
    }
}

impl ExtractionAgent for RetryingUsageAgent {
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
        let mut log = self.log.lock().expect("log 锁");
        // ★ 与两个真实 adapter 同一套编号：按「该组已记几笔」编 `attempt`。
        //   这里是**替身自己在写上报**，所以按同样的义务来编号 ——
        //   一直报 0 就是 REQ-050 修复前的少计行为。
        let attempt = log.iter().filter(|r| r.group_id == g.group_id).count() as u32;
        let plan = self
            .spend
            .iter()
            .find(|(id, _)| *id == g.group_id)
            .map(|(_, v)| v)
            .ok_or_else(|| anyhow::anyhow!("替身没有为 {} 安排用量", g.group_id))?;
        let used = *plan
            .get(attempt as usize)
            .ok_or_else(|| anyhow::anyhow!("{} 的第 {} 次尝试没有安排用量", g.group_id, attempt))?;
        let idx = (attempt as usize).min(self.currencies.len() - 1);
        let currency = self.currencies[idx].clone();
        log.push(UsageReport {
            group_id: g.group_id.clone(),
            attempt,
            used,
            size: None,
            cost: Some(MoneyAmount {
                amount: used as f64 / 1000.0,
                currency,
            }),
        });

        // 钱已经记完了，下面才决定这次尝试成不成功 —— 与 LLM 路径同序。
        if self.fail_first && attempt == 0 {
            anyhow::bail!("模拟：第 0 次尝试失败（正文空 / 被截断）");
        }

        let name = g.schema[0].name.clone();
        Ok(GroupExtraction {
            links: Vec::new(),
            types: vec![CandidateType {
                key: name.clone(),
                display_name: name,
                is_new: true,
                properties: vec![CandidateProperty {
                    name: "标识".into(),
                    source: ColumnRef::new(&g.schema[0].name, "id"),
                    nullable: Some(false),
                    note: None,
                    origin: Some(PropertyOrigin::Comment),
                }],
                description: None,
                aliases: Vec::new(),
                evidence: Vec::new(),
                rejected_comments: Some(0),
            }],
        })
    }

    fn usage(&self) -> Vec<UsageReport> {
        self.log.lock().expect("log 锁").clone()
    }
}

fn run(spend: Vec<(&str, Vec<u64>)>, fail_first: bool, max_used: u64) -> ontok::agent::orchestrate::ExtractionRun {
    let (report, n) = (schema(), names());
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.budget = Some(UsageBudget { max_used, max_cost: None });
    let agent = RetryingUsageAgent::new(spend, fail_first);
    orchestrate(&agent, &report, &input).expect("越闸不是抽取失败，不该是 Err")
}

// ──────────────────────────────────────────────────────────
// AC-001 · 重试的花费全部进账
// ──────────────────────────────────────────────────────────

/// 同组重试两次，用量 `[120, 160]` ⇒ 账上必须是 **280**。
///
/// 修复前按 `group_id` **覆盖** ⇒ 只留最后一笔 ⇒ 160。
/// 这是「两条腿都记账但一条被吃掉」的直接判据。
#[test]
fn test_retry_spend_is_all_counted_not_just_the_last_attempt() {
    // 上限放到 1000，先把闸门排除掉，单独看账。
    let run = run(vec![("table:t_a", vec![120, 160]), ("table:t_b", vec![90])], true, 1000);

    assert_eq!(
        run.stage.budget.used, 370,
        "同组重试的两笔（120 + 160）**都要进账**，加上另一组的 90 = 370；\
         修复前是 160 + 90 = 250 —— 第一笔钱被静默吃掉了"
    );
    let cost = run.stage.budget.cost.expect("两次尝试都报了费用");
    // 0.120 + 0.160 + 0.090
    assert!(
        (cost.amount - 0.370).abs() < 1e-9,
        "费用也必须按两笔相加 = 0.370 USD，实际 {:.6}",
        cost.amount
    );
    assert_eq!(cost.currency, "USD");
}

// ──────────────────────────────────────────────────────────
// AC-002 · 费用闸按真实累计判定（本需求的真正目的）
// ──────────────────────────────────────────────────────────

/// `max_used = 200` **夹在两次尝试之间**（160 < 200 < 280）⇒ 必须**拦住**。
///
/// 修复前账上只有 160，`160 < 200` ⇒ **放行** —— 那就是放过真实超额。
/// 只修报表不修闸门等于没修：这条断言盯的是**闸门的结果**，不是账。
#[test]
fn test_budget_gate_trips_on_the_real_total_not_the_last_attempt_only() {
    let run = run(vec![("table:t_a", vec![120, 160]), ("table:t_b", vec![90])], true, 200);

    assert_eq!(
        run.stage.budget.used, 280,
        "先确认这一组的两笔（120 + 160）都进了账；修复前这里只有 160"
    );
    assert!(
        run.stage.budget.stopped,
        "真实累计 280 ≥ 上限 200，闸门必须停 —— 只按最后一笔 160 判会放行（REQ-050 的资金风险）"
    );
    let reason = run.stage.budget.reason.clone().unwrap_or_default();
    assert!(
        reason.contains("280") && reason.contains("200"),
        "停的原因必须带**真实累计**与上限（280 / 200），实际：{reason}"
    );
    assert_eq!(
        run.stage.budget.skipped_groups,
        vec!["table:t_b".to_string()],
        "越闸之后没跑的组必须逐个点名 —— 第二组的 90 **不该**再花出去"
    );
    assert!(
        !run
            .stage
            .budget
            .used
            .to_string()
            .contains("370"),
        "第二组压根没跑，账上不该出现它的 90"
    );
}

/// 上限 300 **高于第一组的真实累计 280、低于整轮 370** ⇒ 两组都跑完，最后停。
///
/// 这条把「拦住」与「上限恰好等于 280」解耦：判据是**比较**，不是某个写死的数。
/// 修复前第一组只记 160 < 300，第二组 90 之后累计也才 250 < 300 ⇒ **整轮都不会停**。
#[test]
fn test_budget_gate_trips_even_when_limit_is_between_the_two_attempts() {
    let run = run(vec![("table:t_a", vec![120, 160]), ("table:t_b", vec![90])], true, 300);
    assert_eq!(
        run.stage.budget.used, 370,
        "第一组 280 未越 300 ⇒ 两组都跑；整轮真实累计 280 + 90 = 370"
    );
    assert!(
        run.stage.budget.stopped,
        "真实累计 370 ≥ 上限 300 ⇒ 必须停；按修复前的 160 判就会一路放行"
    );
    assert!(
        run.stage.budget.skipped_groups.is_empty(),
        "两组都跑完了，没有「没跑的组」—— 停发生在跑完之后"
    );
}

/// 上限**高于**真实累计 ⇒ 不许停（防止把闸门修成永远停）。
#[test]
fn test_budget_gate_does_not_trip_when_the_real_total_is_under_the_limit() {
    let run = run(vec![("table:t_a", vec![120, 160]), ("table:t_b", vec![90])], true, 1000);
    assert!(
        !run.stage.budget.stopped,
        "真实累计 370 < 上限 1000 ⇒ 不该停。若这条红了，说明修复把账算成了别的数"
    );
    assert!(
        run.stage.budget.skipped_groups.is_empty(),
        "没越闸就不该有「没跑的组」"
    );
}

// ──────────────────────────────────────────────────────────
// AC-003 · 币种不可比时行为不变
// ──────────────────────────────────────────────────────────

/// 两次尝试**币种不同** ⇒ `cost == None`，**不得**把「不可比」变成「相加」。
///
/// `checked_add` 的既有语义：不可比就如实说「不知道」，
/// 而不是折算出一个假的钱数（`:totals` 的注释明说这一点）。
#[test]
fn test_retry_with_two_currencies_still_reports_no_cost() {
    let (report, n) = (schema(), names());
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.budget = Some(UsageBudget { max_used: 1000, max_cost: None });
    let agent = RetryingUsageAgent::new(
        vec![("table:t_a", vec![120, 160]), ("table:t_b", vec![90])],
        true,
    )
    .with_currencies(&["USD", "EUR"]);
    let run = orchestrate(&agent, &report, &input).expect("不该是 Err");

    assert_eq!(
        run.stage.budget.used, 370,
        "用量闸的单位是协议自己的（token），币种不可比**只影响费用**，不影响用量累计"
    );
    assert!(
        run.stage.budget.cost.is_none(),
        "USD 与 EUR 不可比 ⇒ cost 必须是 None（`checked_add` 的既有语义）；\
         把它算成某个数就是**发明折算**，那是账目造假。实际：{:?}",
        run.stage.budget.cost
    );
}

// ──────────────────────────────────────────────────────────
// AC-004 · 不重试的路径不受影响
// ──────────────────────────────────────────────────────────

/// **不重试**（每组只成功一次）⇒ 账与修复前逐条相同。
///
/// 判据与「改动前」对照靠的是仓里**既有的**那条断言：
/// `tests/test_agent_rebuild.rs` 的
/// `test_budget_stops_the_run_and_records_skipped_groups_separately_from_failures`
/// 在基线 `c082556` 就钉着 `used == 20`（每组一条上报），
/// 本次修复后它**仍然绿** —— 那就是「正常路径没被带偏」的直接证据。
/// 这里再钉一条单组的精确值，把「没被带偏」落到具体数字上。
#[test]
fn test_non_retrying_run_keeps_its_exact_numbers() {
    let run = run(
        vec![("table:t_a", vec![120]), ("table:t_b", vec![90])],
        false,
        1000,
    );
    assert_eq!(
        run.stage.budget.used, 210,
        "不重试 ⇒ 每组一笔 ⇒ 120 + 90 = 210（修复前也是这个数）"
    );
    let cost = run.stage.budget.cost.expect("都报了费用");
    assert!(
        (cost.amount - 0.210).abs() < 1e-9,
        "费用同样逐条相同 = 0.210 USD，实际 {:.6}",
        cost.amount
    );
    assert!(!run.stage.budget.stopped, "210 < 1000 ⇒ 不该停");
    assert_eq!(run.stage.groups_ok, 2, "两组都跑完");
}

/// 快照被**重复读**（编排在每组之后重读整个 `usage()` 列表）不翻倍。
///
/// 这是本次修复必须**同时**守住的那一头：按「组 × 第几笔」建键之后，
/// 重读的同一笔命中同一个键 ⇒ 覆盖 ⇒ 不重复计入。
/// 2 组 × 每组 1 笔，但整轮会读 2 次快照 ⇒ 仍是 210，不是 420。
#[test]
fn test_rereading_the_same_snapshot_does_not_double_count() {
    let run = run(
        vec![("table:t_a", vec![120]), ("table:t_b", vec![90])],
        false,
        1000,
    );
    assert_eq!(
        run.stage.budget.used, 210,
        "编排每组之后都重读整个快照；同一笔被读到两次**不许翻倍**"
    );
}
