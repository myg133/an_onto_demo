//! REQ-022 · 阶段 3+4 **真正可插拔** + 成本闸门 + 重建契约的 fail-closed
//!
//! # 这个文件钉住什么
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | **规则 agent 走「从候选重建」这条路，产物与规则路径逐字节相同** | 这是「第二条路」存在的**全部理由**：不一样就不是可插拔，是「会偷偷改提案的第二条路」 |
//! | 重建路径**真的被走过**（`stage.assembly` / 源级两条分支都在） | 与上一条互补：两条路字节相同 ⇒ 「悄悄退回规则路径」在产物里**看不出来**，必须有独立的判据 |
//! | 不声明能力的实现**仍然**走规则路径 | 「可插拔」不是「一律重建」 |
//! | 预算越闸 ⇒ **停**、未跑的组进 `skipped_groups`、**不进** `failed_groups` | 成本超限与抽取失败是两回事；混在一起就是把花钱决策伪装成抽取质量事故 |
//! | 声明「会上报用量」却没给预算 ⇒ **入口拒绝** | 规格 §7.1 的强制义务不能只写在文档里 |
//> | 累计用量**不重复累加** | 协议报的是累计值；累加累计值会让闸门在远未超预算时就停 |
//! | 候选缺 `nullable` / `origin` / `rejected_comments` ⇒ **重建报错** | 缺了就填默认值 = 发一份数据在说谎的提案（`nullable` 进客户视图、`origin` 决定三个 `props_via_*`、`rejected_comments` 是 REQ-005 的降级痕迹） |
//! | 同一 `key` 在两组里内容不同 ⇒ **报错** | 取任意一个 = 静默丢掉另一半，而丢掉的部分不会出现在任何地方 |

mod common;

use std::sync::Mutex;

use ontok::agent::orchestrate::{orchestrate, OrchestrationInput};
use ontok::agent::assemble::{classified_contract_gaps, ContractGap, GapKind};
use ontok::agent::UsageBudget;
use ontok::agent::registry::AgentIdentity;
use ontok::agent::{
    build_agent, CandidateLink, CandidateProperty, CandidateType, ColumnRef, ExtractionAgent,
    ExtractionGroup, GroupExtraction, GroupFailure, MoneyAmount, PropertyOrigin, RuleBasedAgent,
    UsageReport, Assembly, CAPABILITY_PRODUCES_PROPOSAL, CAPABILITY_REBUILDS_PROPOSAL,
    CAPABILITY_REPORTS_USAGE,
};
use ontok::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
use ontok::extract::names::TableNames;
use ontok::extract::propose::extract;

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req022-test",
  "names": {
    "t_dev": "设备",
    "t_customer": "客户",
    "t_region": "区域"
  }
}"#;

fn col(name: &str, comment: Option<&str>) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        data_type: "text".into(),
        nullable: true,
        comment: comment.map(|s| s.to_string()),
        is_primary_key: false,
    }
}

fn table(name: &str, comment: Option<&str>, columns: Vec<ColumnMeta>) -> TableMeta {
    let commented_count = columns.iter().filter(|c| c.comment.is_some()).count();
    TableMeta {
        name: name.to_string(),
        comment: comment.map(|s| s.to_string()),
        column_count: columns.len(),
        commented_count,
        columns,
    }
}

/// 三张表；`t_dev.t_customer_id` 按 `*_id` 约定指向 `t_customer` ——
/// 那条关系**跨组**，正是「阶段 1 候选里必须带得到组外的关系」的判据素材。
fn schema() -> SchemaReport {
    let tables = vec![
        table(
            "t_dev",
            Some("设备表"),
            vec![
                col("id", None),
                col("sn", Some("序列号")),
                col("t_customer_id", Some("客户标识")),
            ],
        ),
        table(
            "t_customer",
            Some("客户表"),
            vec![col("id", None), col("code", Some("客户编码"))],
        ),
        table("t_region", Some("区域表"), vec![col("id", None)]),
    ];
    let column_count: usize = tables.iter().map(|t| t.column_count).sum();
    SchemaReport {
        server_version: "PostgreSQL 17.2".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: tables.len(),
        column_count,
        commented_column_count: 3,
        foreign_key_count: 0,
        introspected_at: "2026-10-07T10:00:00Z".into(),
        tables,
    }
}

fn names() -> TableNames {
    TableNames::load(NAMES_JSON).expect("翻译表应可载入")
}

fn identity() -> AgentIdentity {
    AgentIdentity {
        agent: "mcode-agent".into(),
        version: "2026.10.07".into(),
        round: 1,
    }
}

// ──────────────────────────────────────────────────────────
// 替身 A：**规则 agent 的加壳**（只多声明一个能力标签）
//
// 为什么用它当「走重建路径的实现」：它的阶段 1 候选仍然是**真实规则管线**
// 出来的（不是手搓的假候选），所以「重建 == 规则路径」这条等价性
// 才有说服力 —— 手搓候选只能证明「重建能把候选变成条目」。
// ──────────────────────────────────────────────────────────

struct RebuildableRuleAgent {
    inner: RuleBasedAgent,
}

impl RebuildableRuleAgent {
    fn new() -> Self {
        Self {
            inner: RuleBasedAgent::new(identity(), names()),
        }
    }
}

impl ExtractionAgent for RebuildableRuleAgent {
    fn identity(&self) -> &AgentIdentity {
        self.inner.identity()
    }
    fn capabilities(&self) -> Vec<String> {
        // 只加一个标签：**声明「我的候选能重建提案」**。
        // 其余（怎么抽的）一个字没动 —— 于是两条路唯一的差别就是组装方式。
        vec![
            CAPABILITY_PRODUCES_PROPOSAL.to_string(),
            CAPABILITY_REBUILDS_PROPOSAL.to_string(),
        ]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        self.inner.extract_group(g)
    }
}

/// ★ **等价性判据**：规则 agent 走「从候选重建」，`proposal.json` 与
/// `stats.json` 与规则路径**逐字节相同**。
///
/// # 拿掉/改坏哪一处会让它红
///
/// - `assemble::entity_item` 不带 `aliases` / `description` / `evidence` ⇒ 红；
/// - `assemble::rebuild_outcome` 不做关系合并 ⇒ 红（少 1 条 `add_link_type`）；
/// - `CandidateProperty::origin` 没上报 / 统计按 `is_empty` 猜 ⇒ 红；
/// - 规则 agent 的投影少搬一个字段 ⇒ 红（**两侧同时变**才不红）。
#[test]
fn test_rule_agent_rebuilt_proposal_is_byte_identical_to_the_rule_pipeline() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");

    // 路径 1：既有规则路径（RuleBasedAgent 只声明 produces_proposal）
    let rule_agent = RuleBasedAgent::new(identity(), names());
    let rule_run = orchestrate(&rule_agent, &report, &input).expect("规则路径应跑得通");

    // 路径 2：同一个规则引擎，只多声明一个能力标签 ⇒ 走重建路径
    let rebuilt_run = orchestrate(&RebuildableRuleAgent::new(), &report, &input)
        .expect("重建路径应跑得通");

    // 基线：REQ-002 契约里那条既有的全量入口（`extract`），逐字节对照
    let legacy = extract(&report, &n, "extracted", "mcode-agent", "2026.10.07")
        .expect("既有全量路径应跑得通");

    let same = |tag: &str, got: String, want: String| {
        assert_eq!(
            got, want,
            "{tag}：从候选重建的产物必须与规则路径**逐字节相同** —— 不一样就说明重建路径在偷偷改提案"
        );
    };
    let pp = |p: &ontok::extract::propose::Proposal| serde_json::to_string_pretty(p).unwrap();
    let sp = |s: &ontok::extract::propose::ExtractionStats| serde_json::to_string_pretty(s).unwrap();
    let rp = pp(&rebuilt_run.outcome.proposal);
    let rl = pp(&rule_run.outcome.proposal);
    let rs = sp(&rebuilt_run.outcome.stats);
    let rls = sp(&rule_run.outcome.stats);
    same("重建 vs 规则路径 · proposal", rp.clone(), rl);
    same("重建 vs 规则路径 · stats", rs.clone(), rls);
    same("重建 vs 既有全量入口 · proposal", rp.clone(), pp(&legacy.proposal));
    same("重建 vs 既有全量入口 · stats", rs, sp(&legacy.stats));

    // 显式再钉一遍「实质内容」不是空的：否则「两条路都产出 0 条 items」
    // 也会逐字节相同 —— 恒真的等价性等于没有等价性。
    let n_ent = rebuilt_run
        .outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .count();
    let n_link = rebuilt_run
        .outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_link_type")
        .count();
    assert_eq!(n_ent, 3, "三个实体必须都在（重建不是产出空壳）");
    assert_eq!(
        n_link, 1,
        "跨组的 t_customer_id 关系必须由**那一张表自己的组**贡献 —— 阶段 1 候选不带关系就会少这一条"
    );
    assert_eq!(
        rebuilt_run.outcome.stats.total_properties, 6,
        "属性总数必须与规则路径一致（这份 fixture 的 6 列全部成为属性：id 列未标主键）"
    );
}

/// ★ 互补判据：**重建路径真的被走过**。
///
/// 为什么必须单列：上一条断言的**两条路字节相同** —— 于是「悄悄退回规则路径」
/// 在产物里**一个字都看不出来**。这条盯的是 `stage.assembly` 这个
/// 编排自己记下的事实。
#[test]
fn test_rebuilt_run_records_that_it_rebuilt_and_not_fell_back_to_the_rule_path() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");

    let rebuilt = orchestrate(&RebuildableRuleAgent::new(), &report, &input).unwrap();
    assert_eq!(
        rebuilt.stage.assembly,
        Assembly::RebuiltFromCandidates,
        "声明了 rebuilds_proposal 就必须走重建路径；退回规则路径是**静默退化**，产物上看不出来"
    );
    assert_eq!(
        rebuilt.stage.candidate_links, 1,
        "阶段 1 的候选必须真的带上了关系（否则重建路径的提案会少关系）"
    );

    let rule = orchestrate(&RuleBasedAgent::new(identity(), names()), &report, &input).unwrap();
    assert_eq!(
        rule.stage.assembly,
        Assembly::RulePipeline,
        "没声明 rebuilds_proposal 的实现**不许**被改道 —— 「可插拔」是两条路并存，不是一律重建"
    );
}

/// 源级护栏：编排里两条组装分支都在，且重建分支真的调 `rebuild_outcome`。
///
/// 拿掉重建分支（`Assembly::RebuiltFromCandidates =>` 那支）会让
/// 上一条判据红（编译不过 / `assembly` 不再是重建）——
/// 这里保证的是**它不会被悄悄改成「两条路都走规则」**。
#[test]
fn test_orchestration_source_keeps_both_assembly_branches() {
    let src = include_str!("../src/agent/orchestrate.rs");
    let body = src
        .split("pub fn orchestrate")
        .nth(1)
        .expect("找得到 orchestrate 函数体");
    assert!(
        body.contains("rebuild_outcome("),
        "编排必须真的调重建路径（否则「第二条路」只是文档）"
    );
    assert!(
        body.contains("Assembly::RulePipeline =>") && body.contains("Assembly::RebuiltFromCandidates =>"),
        "两条组装分支都必须还在 —— 少一条就是「悄悄退回规则路径」"
    );
    assert!(
        body.contains("verify_mappings("),
        "阶段 2 的代码侧核对不可外包（架构红线），重构成两条路也不许挪走"
    );
    assert!(
        body.find("verify_mappings(") < body.find("rebuild_outcome("),
        "阶段 2 必须在阶段 4 **之前**：先核对声明，再用声明组装"
    );
}

// ──────────────────────────────────────────────────────────
// REQ-022 · 成本闸门（规格 §7.1 的强制义务）
// ──────────────────────────────────────────────────────────

/// **协议 agent 的进程内替身**：抽一组就给该组上报一次**累计**用量
/// （模拟 ACP 的 `usage_update`）。
///
/// ⚠️ 两条语义必须如实实现，否则这条判据测不到它要测的东西：
///
/// 1. **每个分组一次会话**（ADR-002 的外置义务：91 组 = 91 次会话），
///    所以会话级累计量**就是该组自己的**用量；
/// 2. `used` 是**累计值**（不是增量）—— 编排必须「按组取最新后相加」，
///    把它当增量累加的话，闸门会在远未超预算时就停，而账还是错的。
struct UsageReportingAgent {
    identity: AgentIdentity,
    per_group: u64,
    /// 每组自己的累计（模拟 adapter 的「每组一个 session」状态）。
    /// `&self` 上的方法只能这么存 —— I/O 在 adapter 里，编排只读快照。
    state: Mutex<Vec<(String, u64)>>,
}

impl UsageReportingAgent {
    fn new(agent: &str, per_group: u64) -> Self {
        Self {
            identity: AgentIdentity {
                agent: agent.into(),
                version: "2026.10.08".into(),
                round: 1,
            },
            per_group,
            state: Mutex::new(Vec::new()),
        }
    }
}

impl ExtractionAgent for UsageReportingAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REPORTS_USAGE.to_string(), CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        {
            let mut st = self.state.lock().unwrap();
            let used = match st.iter_mut().find(|(id, _)| *id == g.group_id) {
                Some((_, u)) => {
                    *u += self.per_group;
                    *u
                }
                None => {
                    st.push((g.group_id.clone(), self.per_group));
                    self.per_group
                }
            };
            // 同一次会话会被读**两次**（抽完立刻读一次 + 下一组之前再读一次
            // 就会看不到新值 —— 这里刻意多报一次同值，验证编排**不重复累加**）。
            let report = UsageReport {
                group_id: g.group_id.clone(),
                // ⚠️ REQ-050：这条替身**不编第几笔**（两条上报都是 0），
                // 于是它们被编排认作**同一笔** ⇒ 只算一次。
                // 这正是本测试要钉的性质（「同一笔被读到两次不翻倍」），
                // 而「重试的第二笔」由 `attempt` 区分、走另一条键。
                attempt: 0,
                used,
                size: Some(1_000_000),
                cost: Some(MoneyAmount { amount: used as f64 / 1000.0, currency: "USD".into() }),
            };
            // 追加一条**同组同值**的重复上报：编排按组取最新 ⇒ 不受影响；
            // 若它按「累加所有上报」实现，这里就会翻倍。
            st.push((format!("{}#dup", g.group_id), used));
        }
        Ok(GroupExtraction {
            links: Vec::new(),
            types: vec![CandidateType {
                key: table.clone(),
                display_name: table.clone(),
                is_new: true,
                properties: vec![CandidateProperty {
                    name: "名称".into(),
                    source: ColumnRef::new(&table, "id"),
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
        self.state
            .lock()
            .unwrap()
            .iter()
            .map(|(id, used)| UsageReport {
                // `#dup` 后缀是「同组第二次上报」的标记，回报时去掉，
                // 让编排看到的就是「同一 group_id 多条上报」。
                group_id: id.trim_end_matches("#dup").to_string(),
                // 两条都是 `attempt: 0` ⇒ 同一个键 ⇒ 覆盖 ⇒ 不翻倍。
                // （REQ-050：真重试时 adapter 会给出不同的 `attempt`，
                //   那是**另一笔钱**，必须都进账 —— 见 orchestrate 的 `by_report`。）
                attempt: 0,
                used: *used,
                size: Some(1_000_000),
                cost: Some(MoneyAmount { amount: *used as f64 / 1000.0, currency: "USD".into() }),
            })
            .collect()
    }
}

/// ★ 越闸 ⇒ **停**，并且「没跑」与「抽不出来」**分两栏记**。
///
/// 判别力来自三处同时核对：
/// 1. `stopped == true` 且原因里有具体的数；
/// 2. 未跑的组在 `skipped_groups`，**不在** `failed_groups`
///    （也不进 `extracted_by.failed_groups`）；
/// 3. 提案里**只含跑完的组**的类型 —— 停不是「跑完全部再假装」；
/// 4. 累计用量按「每组最新值」相加 = 跑完那几组的**最后一组**的累计值。
#[test]
fn test_budget_stops_the_run_and_records_skipped_groups_separately_from_failures() {
    let (report, n) = (schema(), names());
    let mut input = OrchestrationInput::new(&n, "extracted");
    // 每组 10 单位；上限 15 ⇒ 跑完第 2 组（累计 20 ≥ 15）就该停，
    // 第 3 组（t_region）**根本没跑**。
    input.budget = Some(UsageBudget { max_used: 15, max_cost: None });

    let agent = UsageReportingAgent::new("vendor-acp", 10);
    let run = orchestrate(&agent, &report, &input).expect("越闸不是抽取失败，不该是 Err");

    assert!(run.stage.budget.stopped, "越闸必须显式停下并上报");
    let reason = run.stage.budget.reason.clone().unwrap_or_default();
    assert!(
        reason.contains("20") && reason.contains("15"),
        "停的原因要带**具体数字**（已用 / 上限），实际：{reason}"
    );
    // 下面这条才是真的判据（原占位行已删）：越闸的组**不是抽取失败**。
    // （原占位写的是 `P || !P` —— 恒真、永不失败，看着像断言却什么也不表达；
    //   本项目七次假绿里有好几次就是这种「看起来像判据」的东西。）
    assert!(
        run.stage.failed_groups.is_empty(),
        "越闸的组**不是抽取失败** —— 它压根没被调用，不该进 failed_groups（实际：{:?}）",
        run.stage.failed_groups
    );
    assert_eq!(
        run.stage.budget.used, 20,
        "累计用量 = 每组**最新**累计值相加（2 组 × 10），不能把同一组的多次上报加起来"
    );
    assert_eq!(
        run.stage.budget.skipped_groups,
        vec!["table:t_region".to_string()],
        "越闸之后**没被调用**的组必须逐个点名"
    );
    assert!(
        !run.stage.budget.skipped_groups.is_empty(),
        "未跑的组必须被显式列出 —— 否则「停」在产物里看不出是哪几组没跑"
    );
    for g in &run.stage.budget.skipped_groups {
        assert!(
            !run.stage
                .failed_groups
                .iter()
                .any(|f: &GroupFailure| f.group_id == *g),
            "{g} 既在 skipped 又在 failed —— 两栏混在一起就是把花钱决策伪装成抽取质量事故"
        );
        assert!(
            !run
                .outcome
                .proposal
                .extracted_by
                .failed_groups
                .iter()
                .any(|v| v.get("group_id").and_then(|x| x.as_str()) == Some(g.as_str())),
            "{g} 不许进提案的 extracted_by.failed_groups（那是抽取失败的契约字段）"
        );
    }
    let keys: Vec<&str> = run
        .outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .map(|i| i["payload"]["key"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        keys,
        vec!["t_dev", "t_customer"],
        "提案只含**跑完**的组（越闸不是「照常全跑再假装」）"
    );
}

/// 没跑满预算时，一切照常：**不许**有任何「停」的痕迹（别把闸门写成永远报警）。
#[test]
fn test_budget_within_limit_never_reports_a_stop() {
    let (report, n) = (schema(), names());
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.budget = Some(UsageBudget { max_used: 10_000, max_cost: None });

    let run = orchestrate(&UsageReportingAgent::new("vendor-acp", 10), &report, &input).unwrap();
    assert!(!run.stage.budget.stopped, "预算充足时不该停");
    assert!(run.stage.budget.skipped_groups.is_empty(), "预算充足时不该有跳过的组");
    assert_eq!(run.stage.budget.used, 30, "跑满 3 组的累计用量");
    assert!(run.stage.failed_groups.is_empty());
}

/// 声明了「我会上报用量」却没给预算 ⇒ **入口拒绝**，且一行业务代码都没跑。
///
/// 这是规格 §7.1 那条强制义务的**唯一**执行点：删掉 `require_budget(agent, input)?`
/// 这一行，本测试立刻红（于是变成 Ok + 一份没有闸门的抽取）。
#[test]
fn test_agent_reporting_usage_without_a_budget_is_refused_before_any_work() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted"); // **没有 budget**

    let err = orchestrate(&UsageReportingAgent::new("vendor-acp", 10), &report, &input)
        .err()
        .expect("声明了会上报用量却没给预算 ⇒ 必须拒绝");
    let msg = format!("{err:#}");
    assert!(msg.contains("budget") || msg.contains("预算"), "要说清缺的是预算，实际：{msg}");
    assert!(msg.contains("vendor-acp"), "要指名道姓哪个 agent 被拒，实际：{msg}");
    assert!(
        msg.contains("§7.1") || msg.contains("强制义务"),
        "要引用那条义务的出处（否则下一个人会以为这是随手加的），实际：{msg}"
    );
}

/// 零成本实现（不声明 `reports_usage`）**不受**这条拒绝影响 —— 否则闸门
/// 会把唯一能跑的实现也挡掉（那是「义务」变成「 outage」）。
#[test]
fn test_zero_cost_agent_needs_no_budget() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let run = orchestrate(&RuleBasedAgent::new(identity(), names()), &report, &input)
        .expect("进程内规则实现零成本，不该被要求设预算");
    assert!(!run.stage.budget.stopped);
    assert!(run.stage.budget.skipped_groups.is_empty());
}

/// 金额闸：**同币种**才比；币种不同**不**触发（不可比的数不由代码替人折算）。
#[test]
fn test_cost_budget_only_trips_on_a_matching_currency() {
    let b = UsageBudget {
        max_used: u64::MAX,
        max_cost: Some(MoneyAmount { amount: 1.0, currency: "USD".into() }),
    };
    assert!(
        !b.exceeded_by(0, Some(&MoneyAmount { amount: 99.0, currency: "CNY".into() })),
        "币种不同不可比 ⇒ 不触发（折算是人的决定，不是代码的）"
    );
    assert!(
        b.exceeded_by(0, Some(&MoneyAmount { amount: 1.0, currency: "USD".into() })),
        "同币种且用满上限 ⇒ 触发（用满即止）"
    );
    assert!(
        !b.exceeded_by(0, None),
        "协议没报费用 ≠ 花了钱；只按已知的量判，不臆造"
    );
}

// ──────────────────────────────────────────────────────────
// REQ-022 · 重建契约的 fail-closed
// ──────────────────────────────────────────────────────────

/// 一个「声明了能重建、但候选缺字段」的替身。
///
/// `missing` 指明它**故意不报**哪一项 —— 重建必须**逐项报错**，
/// 而不是替它补默认值。
struct IncompleteRebuildAgent {
    identity: AgentIdentity,
    /// `nullable` / `origin` / `rejected_comments`
    missing: &'static str,
    /// 关系候选（用来测「跨组冲突」）
    conflict_link: bool,
}

impl ExtractionAgent for IncompleteRebuildAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        // 这条候选**故意**指向一个不存在的列 —— 阶段 2 必须报它，
        // 而阶段 4 仍不许拿它当「已核对过」。
        Ok(GroupExtraction {
            types: vec![CandidateType {
                key: table.clone(),
                display_name: table.clone(),
                is_new: true,
                properties: vec![CandidateProperty {
                    name: "编出来的属性".into(),
                    source: ColumnRef::new(&table, "no_such_column"),
                    nullable: if self.missing == "nullable" { None } else { Some(false) },
                    note: None,
                    origin: if self.missing == "origin" { None } else { Some(PropertyOrigin::Comment) },
                }],
                description: None,
                aliases: Vec::new(),
                evidence: Vec::new(),
                rejected_comments: if self.missing == "rejected_comments" { None } else { Some(0) },
            }],
            links: if self.conflict_link {
                vec![CandidateLink {
                    key: format!("{table}_ghost__t_ghost"),
                    target: format!("{table}→幽灵"),
                    from_type: table.clone(),
                    to_type: "t_ghost".into(),
                    cardinality: "N:1".into(),
                    semantic: "belongs_to".into(),
                    // ⚠️ REQ-024 起这条**带**证据了。这条关系要测的是
                    // 「终点实体不存在 ⇒ AC-007 记悬空」，不是证据缺失 ——
                    // 而抽取后守卫（`assemble::contract_gaps`）会核对
                    // `links[].evidence`。缺证据的判据另由
                    // `EvidenceLessLinkAgent` 单独覆盖（见文件里那条测试）：
                    // 两个缺陷挤在一个替身上，谁先触发就说不清了。
                    evidence: vec![serde_json::json!({
                        "table": table,
                        "column": "ghost_id",
                        "claim": "指向一个不存在的类型（AC-007 悬空用例）",
                    })],
                }]
            } else {
                Vec::new()
            },
        })
    }
}

/// 实体侧**逐项齐备**、唯一缺口在关系侧（`links[].evidence` 为空）的替身。
///
/// 为什么单独一个替身：`IncompleteRebuildAgent` 的关系缺口一旦改成
/// 「带证据」，它就测不了证据缺失了；而两个缺口挤在一个替身上，
/// 触发顺序一变就分不清是哪条判据抓到的。
struct EvidenceLessLinkAgent {
    identity: AgentIdentity,
}

impl ExtractionAgent for EvidenceLessLinkAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        Ok(GroupExtraction {
            types: vec![CandidateType {
                key: table.clone(),
                display_name: table.clone(),
                is_new: true,
                properties: vec![CandidateProperty {
                    name: "名称".into(),
                    source: ColumnRef::new(&table, "id"),
                    nullable: Some(false),
                    note: None,
                    origin: Some(PropertyOrigin::Comment),
                }],
                description: None,
                aliases: Vec::new(),
                evidence: Vec::new(),
                rejected_comments: Some(0),
            }],
            // 端点都在本组作用域内（不悬空），所以这组**只**缺证据。
            links: vec![CandidateLink {
                key: format!("{table}_customer_id__t_customer"),
                target: format!("{table}→客户"),
                from_type: table.clone(),
                to_type: "t_customer".into(),
                cardinality: "N:1".into(),
                semantic: "belongs_to".into(),
                evidence: Vec::new(),
            }],
        })
    }
}

/// ★ 缺 `nullable` ⇒ **重建报错**（不猜 `false`）。
///
/// 为什么严：`nullable` 会原样进客户视图的「可空」标记 ——
/// 那是给人看的**合同**，猜出来的值没人查得出错。
#[test]
fn test_rebuild_refuses_a_candidate_without_nullable() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = IncompleteRebuildAgent {
        identity: identity(),
        missing: "nullable",
        conflict_link: false,
    };
    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("缺 nullable 必须报错（fail-closed），不许拼一份猜出来的提案");
    let msg = format!("{err:#}");
    assert!(msg.contains("nullable"), "要说清缺的是哪一项，实际：{msg}");
    assert!(
        msg.contains("false") || msg.contains("猜"),
        "要说清「为什么不填默认值」，实际：{msg}"
    );
}

/// 缺 `origin` ⇒ 报错（否则 `stats.props_via_*` 会写 0，而那是**谎话**）。
#[test]
fn test_rebuild_refuses_a_candidate_without_property_origin() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = IncompleteRebuildAgent {
        identity: identity(),
        missing: "origin",
        conflict_link: false,
    };
    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("缺 origin 必须报错：三个 props_via_* 计数写 0 就是说谎");
    let msg = format!("{err:#}");
    assert!(msg.contains("origin"), "要说清缺的是哪一项，实际：{msg}");
    assert!(
        msg.contains("props_via") ,
        "要说清缺它会让哪个统计失真，实际：{msg}"
    );
}

/// ★ 抽取后守卫：声明了产出能力的实现，**每组抽完立刻核对**。
///
/// # 这条替代了「能力标签跟着答案翻转」那个坏形状
///
/// 曾经的形状是 `capabilities()` 读一个由 `extract_group` 填的运行时状态 ——
/// 判据**先于**它要核对的数据产生，于是经 `orchestrate` 这唯一生产入口
/// **不可达**（闸门在入口先拒，答案后到）。现在判据在**抽取之后**。
///
/// # 判别力来自三处同时核对
///
/// 1. 守卫**必须响亮**：不合契约 ⇒ `orchestrate` 返回 `Err`（不是静默放行、
///   也不是只记一次失败后继续烧钱）；
/// 2. 错误信息要**点名是哪一项**（缺 `origin` 就说 `origin`，
///   让人不必回去翻产物才发现）；
/// 3. 错误信息要**点名是哪一组**（组级失败是设计点⑤的形状，
///   「这一组不行」与「这轮全崩」是两件不同的事）。
#[test]
fn test_rebuild_contract_guard_rejects_a_group_before_it_reaches_the_proposal() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = IncompleteRebuildAgent {
        identity: identity(),
        missing: "origin",
        conflict_link: false,
    };

    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("★ 声明了产出能力却拿不出承载契约的候选 ⇒ 必须当场响亮失败");
    let msg = format!("{err:#}");

    assert!(
        msg.contains("origin"),
        "要指名缺的是哪一项（不能让人回去翻产物才发现），实际：{msg}"
    );
    assert!(
        msg.contains("table:t_dev"),
        "要指名是哪一组（组级失败与整轮崩溃是两件事），实际：{msg}"
    );
    assert!(
        msg.contains(CAPABILITY_REBUILDS_PROPOSAL),
        "要说清它声明了什么却做不到，实际：{msg}"
    );
    assert!(
        !msg.contains("阶段 4 重建失败"),
        "★ 这条必须来自**抽取后守卫**，而不是阶段 4 的 fail-closed。\n\
         阶段 4 的报错要等所有组跑完才炸，那时钱已经花完了；\
         守卫要在**这一组**抽完时就说话。实际：{msg}"
    );

    // ── REQ-024 复审 F1：这条提示里曾有 3 处「字面量 n + 一串空格」的笔误 ──
    //
    // 它是**给人读的错误提示**，出现在运维最需要看清的那一段里 ——
    // 「一个 n 加 31 个空格」既不像话也不像排版，读者会以为是文件损坏。
    // 所以钉住：提示里**不许有**这种行。
    let typo_lines: Vec<&str> = msg
        .lines()
        .filter(|l| {
            l.starts_with('n')
                && l.strip_prefix('n')
                    .is_some_and(|rest| rest.starts_with(' ') && !rest.trim().is_empty())
        })
        .collect();
    assert!(
        typo_lines.is_empty(),
        "★ 错误提示里出现了「字面量 n + 空格」的笔误（复审 F1）：{typo_lines:?}\n\
         完整提示：{msg}"
    );
}

/// ★ 守卫覆盖 `links[].evidence`（判据原先只看 `types`）。
///
/// 背景：`assemble::link_item` 把 `l.evidence` **逐字**搬进提案，
/// **不做核对** —— 而判据只遍历 `extraction.types`，压根看不见关系侧。
/// 于是「判据说没问题、重建照样把证据丢了」会一路静默到产物里。
#[test]
fn test_rebuild_contract_guard_catches_a_link_without_evidence() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    // 实体侧逐项齐备（origin / nullable / rejected_comments 都在），
    // 唯一的缺口在关系侧 —— 这正是「判据只遍历 types 就看不见那一侧」的可观测形状。
    let agent = EvidenceLessLinkAgent {
        identity: identity(),
    };

    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("★ 关系缺证据必须被守卫抓到，否则证据会静默丢进产物");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("evidence"),
        "★ 关系缺证据必须被守卫抓到（判据原先只遍历 types），实际：{msg}"
    );
}

/// ★ 守卫覆盖 `nullable`（判据原先不查它，而 `entity_item` 对它硬报错）。
///
/// 这一条堵的是「**通过判据 → 重建时 bail**」这个可构造的错位：
/// 判据说行，重建路径却因为缺一个字段炸掉。
#[test]
fn test_rebuild_contract_guard_catches_a_property_without_nullable() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = IncompleteRebuildAgent {
        identity: identity(),
        missing: "nullable",
        conflict_link: false,
    };
    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("缺 nullable 必须被守卫抓到");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("nullable") && msg.contains("重建契约守卫"),
        "★ 缺 nullable 必须被守卫**在判据里**抓到，而不是等到阶段 4 才 bail，实际：{msg}"
    );
}

/// 未声明 `rebuilds_proposal` 的实现**不受守卫约束**（它们不走重建路径）。
///
/// 守卫对规则路径必须是**零影响**，否则第 1 轮产物会变 ——
/// 这条钉住「守卫只作用于声明了产出能力的实现」。
#[test]
fn test_the_contract_guard_does_not_touch_the_rule_pipeline() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    // 规则 agent 只声明 `produces_proposal` ⇒ 走规则路径 ⇒ 守卫不适用。
    let run = orchestrate(&RuleBasedAgent::new(identity(), names()), &report, &input)
        .expect("规则路径不该被守卫拦下");
    assert!(
        run.stage.failed_groups.is_empty(),
        "守卫不该作用于未声明 rebuilds_proposal 的实现，实际：{:?}",
        run.stage.failed_groups
    );
    assert!(
        !run.outcome.proposal.items.is_empty(),
        "规则路径照常产出提案"
    );
}

// ──────────────────────────────────────────────────────────
// ★ REQ-024 复审 F2：守卫**分级** —— 结构性 bail，内容类降级为组级失败
// ──────────────────────────────────────────────────────────
//
// 曾经的形状是「任何缺口 ⇒ 整轮 bail」，理由是「契约缺口是系统性的」。
// 那句话对**实现层面**的缺口成立，对**答案内容类**缺口不成立 ——
// 而 REQ-024 自己新补的 `nullable` / `links[].evidence` 判据与
// 「某组 0 个类型」正是后者。
//
// 分级判据是**可测的**（`assemble::gap_kind_of`）：
// 按**字段**统计「填了几个 / 适用几个」——
//   · `0 / n`（n > 0）⇒ 结构性（整份答案**每个实例**都缺 ⇒ 实现承载不下）
//   · `0 / 0`        ⇒ 内容类（没有实例 ⇒ 压根没做承诺，就是没抽出来）
//   · `k / n`        ⇒ 内容类（能力已证明，这一项抽漏了）

/// 造一个属性候选（`origin` 默认齐备，缺口由调用方指定）。
fn cand_prop(
    table: &str,
    column: &str,
    name: &str,
    nullable: Option<bool>,
    origin: Option<PropertyOrigin>,
) -> CandidateProperty {
    CandidateProperty {
        name: name.to_string(),
        source: ColumnRef::new(table, column),
        nullable,
        note: None,
        origin,
    }
}

fn cand_type(table: &str, props: Vec<CandidateProperty>) -> CandidateType {
    CandidateType {
        key: table.to_string(),
        display_name: table.to_string(),
        is_new: true,
        properties: props,
        description: None,
        aliases: Vec::new(),
        evidence: Vec::new(),
        rejected_comments: Some(0),
    }
}

fn kinds_of(gaps: &[ContractGap]) -> Vec<GapKind> {
    gaps.iter().map(|g| g.kind).collect()
}

/// ★ 判据是**逐字段**测出来的，不是逐答案 —— 这条是分级的全部依据。
///
/// 同一份答案里两个缺口分属两类：这是「按字段统计填了几个 / 适用几个」这个判据
/// 唯一的可观测形状。按答案分类（一个答案一个判决）做不到这件事，
/// 按字段名白名单分类则会把 `nullable` 和 `origin` 焊死成两类（而它们其实是同一类）。
#[test]
fn test_contract_gaps_are_classified_per_field_not_per_answer() {
    // 一个类型、两个属性：
    //   origin：两个都缺      ⇒ 适用 2 / 填 0 ⇒ **结构性**
    //   nullable：只有第二个缺 ⇒ 适用 2 / 填 1 ⇒ **内容类**
    let ex = GroupExtraction {
        types: vec![cand_type(
            "t_dev",
            vec![
                cand_prop("t_dev", "id", "编号", Some(false), None),
                cand_prop("t_dev", "sn", "序列号", None, None),
            ],
        )],
        links: Vec::new(),
    };

    let gaps = classified_contract_gaps(&ex);
    let origin_gap = gaps
        .iter()
        .find(|g| g.text.contains("origin"))
        .expect("缺 origin 必须被报出来");
    let nullable_gap = gaps
        .iter()
        .find(|g| g.text.contains("nullable"))
        .expect("缺 nullable 必须被报出来");

    assert_eq!(
        origin_gap.kind,
        GapKind::Structural,
        "★ 两个属性都没填 origin ⇒ 整份答案都没有这个字段 ⇒ 结构性。实际文本：{}",
        origin_gap.text
    );
    assert_eq!(
        nullable_gap.kind,
        GapKind::Content,
        "★ 同一个属性填了 nullable、另一个没填 ⇒ 能力已证明，这一项抽漏了 ⇒ 内容类。实际文本：{}",
        nullable_gap.text
    );
}

/// 分类必须写进**人能读到的文本**里 —— 线上看到降级，得能判断这是设计还是漏判。
#[test]
fn test_each_gap_text_states_its_class_and_why() {
    let ex = GroupExtraction {
        types: vec![cand_type(
            "t_dev",
            vec![cand_prop("t_dev", "id", "编号", Some(false), None)],
        )],
        links: Vec::new(),
    };
    let gaps = classified_contract_gaps(&ex);
    let g = gaps.first().expect("缺 origin 必须被报出来");
    assert!(
        g.text.contains("分类：**结构性**"),
        "★ 缺口文本要自报分类，否则运维看到降级分不清是设计还是漏判，实际：{}",
        g.text
    );
    assert!(
        g.text.contains("没有任何一个实例"),
        "★ 结构性要说明判据（为什么判成结构性），实际：{}",
        g.text
    );
}

/// 「这一组 0 个类型」归**内容类** —— 空答案没有做任何承诺。
///
/// 这正是 QA-A 的实测反例：重建型实现里 `t_region` 这一组**合法地**返回 0 个类型
/// （其余两组齐备），旧形状会**整轮崩溃**，把另外两组的成果一起陪葬。
#[test]
fn test_an_empty_group_is_classified_as_content_not_structural() {
    let ex = GroupExtraction {
        types: Vec::new(),
        links: Vec::new(),
    };
    let gaps = classified_contract_gaps(&ex);
    assert!(
        !gaps.is_empty(),
        "★ 空答案仍是缺口（未证明 ≠ 已证明），不能因为「降级」就当它没问题"
    );
    assert_eq!(
        kinds_of(&gaps),
        vec![GapKind::Content],
        "★ 空答案 = 没做承诺 = 一次抽取失败 ⇒ 内容类，不是结构性。实际：{:#?}",
        gaps
    );
    assert!(
        gaps[0].text.contains("没有提出任何类型"),
        "文本要点名「0 个类型」这件事本身，实际：{}",
        gaps[0].text
    );
}

/// 一个「每组两个属性、第二个缺 `nullable`」的重建型实现。
///
/// 承载能力**已被它自己证明**（第一个属性填了）⇒ 每个组只差一项 ⇒ 内容类。
struct PartiallyNullAgent {
    identity: AgentIdentity,
}

impl ExtractionAgent for PartiallyNullAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let t = g.schema[0].name.clone();
        // 列够 2 张的组才造得出「一个填了、一个没填」这个形状；
        // 单列组（t_region）照实造 1 个属性 ⇒ 那个组没有缺口。
        let (a, b) = if t == "t_dev" {
            ("id", "sn")
        } else if t == "t_customer" {
            ("id", "code")
        } else {
            // 单列组：给两个属性会指向不存在的列，阶段 2 会先报它 ——
            // 那样就分不清测试要验的是守卫分级还是映射核对了。
            return Ok(GroupExtraction {
                types: vec![cand_type(
                    &t,
                    vec![cand_prop(&t, "id", "编号", Some(false), Some(PropertyOrigin::Comment))],
                )],
                links: Vec::new(),
            });
        };
        Ok(GroupExtraction {
            types: vec![cand_type(
                &t,
                vec![
                    cand_prop(&t, a, "第一项", Some(false), Some(PropertyOrigin::Comment)),
                    cand_prop(&t, b, "第二项", None, Some(PropertyOrigin::Comment)),
                ],
            )],
            links: Vec::new(),
        })
    }
}

/// ★ 内容类缺口 ⇒ **降级为组级失败、继续跑**，不整轮处决。
///
/// 旧形状在这里会返回 `Err` ⇒ 一次单项抽漏拖垮所有组。规格设计点⑤
/// 「校验失败 → 重试 1–2 次 → 仍失败则该组标记『抽取失败』，不静默跳过」
/// 要的正是现在这个形状。
#[test]
fn test_content_gaps_degrade_to_group_failure_instead_of_killing_the_round() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = PartiallyNullAgent {
        identity: identity(),
    };

    let run = orchestrate(&agent, &report, &input)
        .expect("★ 内容类缺口只降级为组级失败，绝不整轮崩溃（QA-A 的反例形状）");

    // 两组各有 1 处内容类缺口，一组齐备。
    assert_eq!(
        run.stage.groups_ok, 1,
        "只有齐备的那组算成功（缺项的组确实没产出可用的候选），实际 failed={:?}",
        run.stage.failed_groups
    );
    let failed_ids: Vec<&str> = run
        .stage
        .failed_groups
        .iter()
        .map(|f| f.group_id.as_str())
        .collect();
    assert_eq!(
        failed_ids.len(),
        2,
        "两个双列组各记一次失败，实际：{failed_ids:?}"
    );
    for want in ["t_dev", "t_customer"] {
        assert!(
            failed_ids.iter().any(|id| id.contains(want)),
            "失败组要指名是哪一组（组级失败与整轮崩溃是两件事），缺 {want}，实际：{failed_ids:?}"
        );
    }

    // 失败原因要说清是**哪一类**缺口、以及缺哪个字段。
    let reason = &run.stage.failed_groups[0].reason;
    assert!(
        reason.contains("内容类") && reason.contains("nullable"),
        "★ 失败原因要自报分类与缺项，否则运维分不清这是设计还是漏判，实际：{reason}"
    );

    // ⚠️ `attempts` 必须照实记 **1**：这一组只被调用过一次。
    //
    // `drive_group` 只在 `extract_group` 返回 `Err` 时重试，`Ok` 就立刻返回 ——
    // 它没有「成功之后再试一次」这个动作。守卫是对那次**已成功返回**的答案
    // 做事后核对，所以实际尝试次数是 1。写成 `input.attempts`（默认 2）就是
    // 在「实际尝试次数」这个字段里放一个编出来的数，而它会进提案元数据。
    for f in &run.stage.failed_groups {
        assert_eq!(
            f.attempts, 1,
            "★ 内容类缺口的组实际只被抽取了 1 次（drive_group 成功即返回，不重试），\
             attempts 记成别的数就是谎报重试次数，实际：{f:?}"
        );
    }

    // 齐备那组的成果**必须**进提案 —— 整轮继续跑的意义就在这儿。
    assert!(
        !run.outcome.proposal.items.is_empty(),
        "★ 齐备那组的候选必须重建进提案（内容类缺口不许把成果一起陪葬）"
    );

    // 设计点⑤：失败必须进 `extracted_by.failed_groups`（显式暴露，不静默跳过），
    // 而它按 ADR-001 **不参与收敛判定**。
    let provenance_failures = &run.outcome.proposal.extracted_by.failed_groups;
    assert_eq!(
        provenance_failures.len(),
        2,
        "★ 失败组必须显式进提案元数据（设计点⑤），实际：{provenance_failures:?}"
    );
}

/// `t_region` 这一组**合法地**返回 0 个类型，其余两组齐备 —— QA-A 的实测反例。
///
/// 旧形状（任何缺口 ⇒ `bail!`）在这里**整轮崩溃**，另外两组的成果一并作废。
struct EmptyRegionGroupAgent {
    identity: AgentIdentity,
}

impl ExtractionAgent for EmptyRegionGroupAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let t = g.schema[0].name.clone();
        if t == "t_region" {
            // 这一组什么也没提 —— **合法**的零答案：重建不了这一组，不等于整个实现不能用。
            return Ok(GroupExtraction {
                types: Vec::new(),
                links: Vec::new(),
            });
        }
        let (a, b) = if t == "t_dev" {
            ("id", "sn")
        } else {
            ("id", "code")
        };
        Ok(GroupExtraction {
            types: vec![cand_type(
                &t,
                vec![
                    cand_prop(&t, a, "第一项", Some(false), Some(PropertyOrigin::Comment)),
                    cand_prop(&t, b, "第二项", Some(false), Some(PropertyOrigin::Comment)),
                ],
            )],
            links: Vec::new(),
        })
    }
}

/// ★ QA-A 的实测反例成了一条测试：一组空答案不许拖垮整轮。
#[test]
fn test_one_empty_group_does_not_kill_the_whole_round() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = EmptyRegionGroupAgent {
        identity: identity(),
    };

    let run = orchestrate(&agent, &report, &input)
        .expect("★ 一组返回 0 个类型只该降级为组级失败，其余两组照常重建");
    assert_eq!(
        run.stage.groups_ok, 2,
        "另外两组齐备 ⇒ 算成功。实际：{:?}",
        run.stage.failed_groups
    );
    assert_eq!(run.stage.failed_groups.len(), 1, "只有 t_region 那一组失败");
    assert!(
        run.stage.failed_groups[0].group_id.contains("t_region"),
        "失败组要指名 t_region，实际：{:?}",
        run.stage.failed_groups
    );
    assert!(
        run.stage.failed_groups[0].reason.contains("内容类"),
        "空答案的失败原因要自报分类，实际：{}",
        run.stage.failed_groups[0].reason
    );
    assert!(
        !run.outcome.proposal.items.is_empty(),
        "★ 另两组的成果必须照常进提案"
    );
    assert_eq!(
        run.outcome.proposal.extracted_by.failed_groups.len(),
        1,
        "★ 失败要显式进提案元数据（设计点⑤），且不参与收敛判定（ADR-001）"
    );
}

/// ★ 但「**所有组**都没抽出来」仍然 fail-closed —— 0 条目的提案比报错更糟。
///
/// 降级成组级失败之后，这条整轮级后果不再由逐组判据兜着
/// （逐组看，每一组的 0 答案都合法）。而 0 条目的提案**看起来完整、内容是空的**。
#[test]
fn test_a_rebuild_with_zero_candidates_refuses_to_emit_an_empty_proposal() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    // 每一组都返回 0 个类型。
    struct AllEmptyAgent {
        identity: AgentIdentity,
    }
    impl ExtractionAgent for AllEmptyAgent {
        fn identity(&self) -> &AgentIdentity {
            &self.identity
        }
        fn capabilities(&self) -> Vec<String> {
            vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
        }
        fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
            Ok(GroupExtraction {
                types: Vec::new(),
                links: Vec::new(),
            })
        }
    }
    let agent = AllEmptyAgent {
        identity: identity(),
    };

    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("★ 一个候选都没有 ⇒ 必须报错，绝不产出一份 0 条目的提案");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("0 条目"),
        "要说清「0 条目的提案比报错更糟」，实际：{msg}"
    );
    assert!(
        msg.contains(&report.tables.len().to_string()),
        "要报出组数与表数，让人能核对「真的什么都没抽出来」，实际：{msg}"
    );
}

/// 缺 `rejected_comments` ⇒ 报错（REQ-005 的降级痕迹不能写成 0）。
#[test]
fn test_rebuild_refuses_a_candidate_without_rejected_comment_count() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = IncompleteRebuildAgent {
        identity: identity(),
        missing: "rejected_comments",
        conflict_link: false,
    };
    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("缺降级计数必须报错");
    assert!(format!("{err:#}").contains("rejected_comments"));
}

/// 阶段 2 的核对**照样**跑（重建路径也不许拿「我声明了」当「核对过了」）。
#[test]
fn test_rebuild_path_still_verifies_declarations_in_code() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let agent = IncompleteRebuildAgent {
        identity: identity(),
        missing: "",
        conflict_link: false,
    };
    let run = orchestrate(&agent, &report, &input).expect("候选完整 ⇒ 应跑得通");
    assert!(
        run.stage.violations.len() >= 3,
        "候选里指向 `no_such_column` 的声明必须被阶段 2 逐组报出来，实际：{:?}",
        run.stage.violations
    );
    // 关系候选指向不存在的类型 ⇒ AC-007 自检把它记成悬空，**不**当成功
    let agent2 = IncompleteRebuildAgent {
        identity: identity(),
        missing: "",
        conflict_link: true,
    };
    let run2 = orchestrate(&agent2, &report, &input).expect("AC-007 自检不是 Err，是计入统计");
    assert_eq!(
        run2.outcome.stats.links_dangling, 3,
        "指向不存在实体的关系必须被 AC-007 记成悬空（重建路径不许绕开这道自检）"
    );
    assert!(
        !run2
            .outcome
            .proposal
            .items
            .iter()
            .any(|i| i["kind"] == "add_link_type"),
        "悬空的关系不许进提案条目"
    );
}

/// 同一个 `key` 在两个组里给出**不同**内容 ⇒ 报错（不许「取第一个」）。
///
/// 为什么这条重要：静默取一个 = 丢掉另一半，而丢掉的部分**不会出现在任何地方** ——
/// 提案看起来完整，内容却是残的。
#[test]
fn test_rebuild_refuses_conflicting_duplicates_across_groups() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    // 这个 schema 里三张表各自的 key 不同，所以冲突由「同组同 key 两个候选」触发：
    // 上面那个替身每组只提一个 —— 冲突走另一条更直接的判据：
    // 同一个候选类型被**同一组**提两次且内容不同。
    let agent = ConflictingAgent { identity: identity() };
    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("同 key 不同内容必须报错");
    let msg = format!("{err:#}");
    assert!(msg.contains("跨组合成") || msg.contains("不同"), "要说清是跨组合成的冲突，实际：{msg}");
    assert!(msg.contains("t_dev"), "要指名道姓是哪个 key，实际：{msg}");
}

/// 每组把同一个 `t_dev` 提**两次**且 `display_name` 不同。
struct ConflictingAgent {
    identity: AgentIdentity,
}

impl ExtractionAgent for ConflictingAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        let mk = |display: &str| CandidateType {
            key: "t_dev".into(), // ★ 两张不同的表都声称这个 key
            display_name: display.into(),
            is_new: true,
            properties: Vec::new(),
            description: None,
            aliases: Vec::new(),
            evidence: Vec::new(),
            rejected_comments: Some(0),
        };
        Ok(GroupExtraction {
            types: vec![mk(&format!("{table}-A"))],
            links: Vec::new(),
        })
    }
}

/// 组**外**的实体不许被凭空造出来：重建路径只认候选里有的类型。
#[test]
fn test_rebuild_only_proposes_types_the_agent_actually_declared() {
    let (report, n) = (schema(), names());
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.budget = Some(UsageBudget { max_used: 10_000, max_cost: None });
    let run = orchestrate(&UsageReportingAgent::new("vendor-acp", 10), &report, &input).unwrap();
    let keys: Vec<&str> = run
        .outcome
        .proposal
        .items
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .map(|i| i["payload"]["key"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        keys,
        vec!["t_dev", "t_customer", "t_region"],
        "重建路径的实体集合 = agent 候选里的 key 集合，一个不多一个不少"
    );
}

/// 登记表里的 `capabilities` **不作数**（与 REQ-019 同一纪律）：
/// 只有实现自己声明的标签才决定走哪条路。
#[test]
fn test_registry_capabilities_do_not_choose_the_assembly_path() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let registration = ontok::agent::registry::AgentRegistration {
        agent: "mcode-agent".into(),
        version: "2026.10.07".into(),
        protocol: "inproc".into(),
        endpoint: None,
        // 登记表里**写着**它能重建 —— 实现并没有声明 ⇒ 必须无视它
        capabilities: vec![CAPABILITY_REBUILDS_PROPOSAL.into()],
        registered_at: "2026-10-07T10:00:00Z".into(),
    };
    let agent = build_agent(&registration, names(), 1).unwrap();
    let run = orchestrate(&*agent, &report, &input).unwrap();
    assert_eq!(
        run.stage.assembly,
        Assembly::RulePipeline,
        "登记表里写 `rebuilds_proposal` 不构成能力声明（那是人的期望，不是实现的事实）"
    );
}
/// ★ 补上「规则 agent 天然给不出」的字段的判别力。
///
/// # 为什么必须单独写这条
///
/// 变异自证发现的一个**真实缺口**：规则实现的 `aliases` **恒为空**
/// （`extract_with_context` 里写死 `aliases: Vec::new()`），
/// 所以「重建时把 `aliases` 丢了」这种改动，用规则 agent 走的两条路
/// **比不出来** —— 15 条判据全绿。这不是判据写错了，是**这份数据里没有它**。
///
/// 于是这里用一个**候选里带满字段**的替身（进程内假 agent）补上：
/// 描述 / 别名 / 证据 / 属性说明 / 值域注记，逐个断言重建后**原样还在**。
/// 变异（删掉任一项的搬运）会让本条立刻红。
struct FullFieldRebuildAgent {
    identity: AgentIdentity,
}

impl ExtractionAgent for FullFieldRebuildAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_REBUILDS_PROPOSAL.to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        Ok(GroupExtraction {
            links: Vec::new(),
            types: vec![CandidateType {
                key: table.clone(),
                display_name: format!("{table}-展示名"),
                is_new: true,
                properties: vec![CandidateProperty {
                    name: "处理状态".into(),
                    source: ColumnRef::new(&table, "status"),
                    nullable: Some(true),
                    note: Some("处理中/已扣费/失败".into()),
                    origin: Some(PropertyOrigin::Comment),
                }],
                description: Some("这是一段只存在于候选里的说明".into()),
                aliases: vec!["设备台账".into(), "资产".into()],
                evidence: vec![serde_json::json!({
                    "table": table,
                    "column": "status",
                    "claim": "这一条证据只存在于候选里"
                })],
                // 只在 t_dev 这一组报 2 次拒用 —— 于是统计必须是 **2**：
                // 取 0（没搬）、6（三组都算）、或 3（按组算错）都能被这条分辨出来。
                rejected_comments: Some(if table == "t_dev" { 2 } else { 0 }),
            }],
        })
    }
}

#[test]
fn test_rebuild_carries_every_candidate_only_field_verbatim() {
    let (report, n) = (schema(), names());
    let input = OrchestrationInput::new(&n, "extracted");
    let run = orchestrate(&FullFieldRebuildAgent { identity: identity() }, &report, &input)
        .expect("候选完整 ⇒ 应跑得通");

    let item = run
        .outcome
        .proposal
        .items
        .iter()
        .find(|i| i["kind"] == "add_entity_type" && i["payload"]["key"] == serde_json::json!("t_dev"))
        .expect("提案里应有 t_dev");

    assert_eq!(
        item["payload"]["description"],
        serde_json::json!("这是一段只存在于候选里的说明"),
        "description 必须原样写回（丢它 = 提案内容被静默改写）"
    );
    assert_eq!(
        item["payload"]["aliases"],
        serde_json::json!(["设备台账", "资产"]),
        "aliases 必须原样写回 —— **规则实现给不出别名**（恒空），这一项只有本判据守着"
    );
    assert_eq!(
        item["payload"]["evidence"],
        serde_json::json!([{
            "table": "t_dev", "column": "status",
            "claim": "这一条证据只存在于候选里"
        }]),
        "evidence 必须原样写回"
    );
    let prop = &item["payload"]["properties"][0];
    assert_eq!(prop["name"], serde_json::json!("处理状态"));
    assert_eq!(prop["column"], serde_json::json!("status"), "列名取自「属性 ← 来源列」声明");
    assert_eq!(prop["nullable"], serde_json::json!(true), "nullable 原样写回（客户视图的可空标记）");
    assert_eq!(
        prop["note"],
        serde_json::json!("处理中/已扣费/失败"),
        "note 必须原样写回（丢它 = 值域信息从客户视图消失）"
    );

    // 被拒注释的计数也必须从候选还原进统计（REQ-005 的降级痕迹）
    assert_eq!(
        run.outcome.stats.props_comment_rejected, 2,
        "rejected_comments 必须进统计：漏记等于「这次没降级」（fail-open）"
    );
}
