//! 抽取 agent 抽象层（REQ-013 · 规格 §7.1 阶段 1「可插拔」的落地）。
//!
//! # 这个 trait 覆盖到哪、**不**覆盖到哪
//!
//! 规格 §7.1 的四阶段链路里，本模块**只做阶段 1**：
//!
//! ```text
//! 阶段 0 · 准备       【纯代码】读 schema / 采样 / 按表分组
//! 阶段 1 · 分组抽取   【外部 agent】每组一次调用   ← 本模块
//!        ↓
//! 阶段 2 · 验证       【纯代码】★ 不可外包
//! 阶段 3 · 跨组合成   【代码 + agent 判断】
//! 阶段 4 · 提案       【纯代码】转 ProposalItem
//! ```
//!
//! **阶段 2 不在 trait 里，也不许被外包**（§7.1「为什么阶段 2 不能外包」：
//! 「若连『提议的依据是否属实』都由 agent 自己判定，这句话就空转了」）。
//! 所以 [`ExtractionAgent`] 只产出**候选 + 映射声明**，
//! 「这条声明属不属实」由 [`verify_mappings`] 这样的纯函数去查 ——
//! **外置「怎么想」，不外置「想得对不对」。**
//!
//! # 为什么这里写了 trait，而 `db/mod.rs` 明写「刻意不写 trait」
//!
//! `src/db/mod.rs:6-7` 与 `src/db/pg.rs:3` 都写着：
//! 「刻意不写 trait（范围规划 §3 暂缓项）：本 MVP 只有 PostgreSQL 一个实现，
//! 只有一个实现时抽 trait 是过早抽象。」
//!
//! **首要理由是规格的许诺，不是当下的实现数量。** 规格 §7.1 明写阶段 1 是
//! 「【外部 agent】**可插拔**」—— 不做 trait，规格要求接进来的外部 agent
//! 就无处安放。「只有一个实现时抽 trait 是过早抽象」这条纪律在这里不适用，
//! 因为可替换性是**规格写死的要求**，不是设计洁癖。
//!
//! **实现数量只是次要佐证，且必须按实况说**：REQ-023 起**生产实现有 2 个**
//! —— [`rule_based::RuleBasedAgent`]（进程内规则）与 [`acp::AcpAgent`]
//! （`mcode acp` 子进程协议 adapter）。另两个实现（`AlwaysFails` /
//! `FailsThenSucceeds`）只存在于 `tests/`，是测试替身，不计入。
//!
//! ⚠️ 但「实有 2 个」**不等于**「都跑得起来」：[`acp::AcpAgent`] 已于 REQ-024
//! 进了 [`factory::build_agent`] 的分派表（登记 `protocol = "acp"` +
//! `endpoint` 即可造出），但它只声明 `reports_usage` ⇒ 编排的
//! `require_proposal_producing` 仍在**入口拒绝整轮**。
//! 那是**有意**的（见 [`acp`] 模块头）：宁可入口拒绝，也不发溯源对不上的提案。
//!
//! | | 存储层 | 抽取 agent 层 |
//! | --- | --- | --- |
//! | 实有实现（今日） | 1（只有 PG） | **2**（`RuleBasedAgent` / `AcpAgent`） |
//! | 工厂能造出的 | 1 | **2**（`build_agent` 分派 `inproc` 与 `acp`） |
//! | 能跑完整轮（产出提案）| 1 | **1**（只有规则实现声明了产出能力） |
//! | 规格许诺 | 无扩展计划 | §7.1 只许诺「**可插拔**」，**未列举个数** |
//! | 抽 trait 的动因 | 无 —— 无从谈起 | §7.1「可插拔」是硬要求 |
//!
//! ⚠️ REQ-025：这一行原先写的是「**≥3（规则 / 外部协议 / LLM 兜底）**」，
//! **那是假的，删掉了**（QA 独立核实）：
//! 规格 §7.1 只写「【外部 agent】可插拔」，从未承诺三个实现；
//! 「LLM 兜底」在 `BA/` 的**规格 / 计划 / 需求文档**里 **0 命中**
//! （`docs/design`、`docs/judging`、`docs/plans`、`demands`、`decisions`
//! 逐个 grep 均为 0）—— 全仓仅有的 **4 处**命中全在 `BA/docs/reports/`
//! 的 QA 报告里，且都是**引用这句假陈述以便批评它**，不算许诺。按实况说：
//! **已实现 2 个**（规则 / ACP stdio），LLM 兜底是**偏差列表 RN-5，未做**。
//! 上面的「实有实现 / 工厂能造出」两行本来就是照实况写的 ——
//! 偏偏许诺那一行拔高成了「≥3」，**许诺自己喊了狼来了**。
//!
//! 换句话说：**这一层抽 trait 是为规格许诺的「可插拔」预留接缝** ——
//! REQ-023 把 ACP adapter 写了出来，REQ-024 把它接上了分派点，
//! 而 LLM 兜底（RN-5）**仍是未做的计划**，不是「已承诺的第三个」。
//!
//! # 为什么方法**同步**（不是 `async fn`）
//!
//! 本 crate 的 `tokio` 只开了 `rt-multi-thread` + `macros`（**没有 `net` / `time`**），
//! 且本任务**不引入任何新依赖** —— 没有 `async-trait`，`async fn` 在 trait 里
//! 也无法做成 `dyn` 可用（Rust 1.86 的 MSRV 下 native async fn in trait
//! 还不 dyn-safe）。
//!
//! 所以：**trait 方法保持同步，把 I/O 留给 adapter。**
//! 真正的协议客户端（A2A / MCP / HTTP）将来在自己的 adapter 里处理异步，
//! 同步外壳只暴露一个 `fn`。这样 trait 保持 `dyn` 可用，且
//! 「规则实现完全无 I/O」这件事在类型上就是真的。
//!
//! ⚠️ **已知毛刺（如实记录，不在本任务解决）**：adapter 在同步方法里
//! `block_on` 会与调用方的 runtime 打架（tokio 在 async 上下文内
//! `block_on` 直接 panic）。真正的协议 adapter 落地时，
//! 调用点必须移到 `spawn_blocking` 里，或让 CLI 的那一段同步执行。
//! 现有调用方（`extract()`）本来就是同步纯函数，所以**当前不受影响**。

pub mod acp;
pub mod assemble;
pub mod factory;
pub mod group;
pub mod llm;
pub mod orchestrate;
pub mod registry;
pub mod rule_based;

pub use factory::{build_agent, implemented_protocols};
pub use group::{
    derive_group_id, verify_mappings, CandidateLink, CandidateProperty, CandidateType, ColumnRef,
    ColumnSample, DecisionList, ExistingType, ExtractionGroup, ExtractionInput, GroupExtraction,
    GroupFailure, GroupOutcome, GroupSource, MappingViolation, PropertyOrigin,
};
pub use llm::LlmAgent;
pub use orchestrate::{
    orchestrate, BudgetReport, ExtractionRun, GroupViolation, OrchestrationInput, StageReport,
    DEFAULT_ATTEMPTS, DEFAULT_BASE_VERSION,
};
pub use registry::{AgentIdentity, AgentRegistration};
pub use rule_based::RuleBasedAgent;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::extract::propose::ExtractionOutcome;

/// **覆写提案的三个溯源字段**（§8.4）——`round` / `extracted_by.round` /
/// `extracted_by.failed_groups`。
///
/// 放在模块根是因为**两条路径都要用它**：规则实现的
/// [`RuleBasedAgent::propose`]（REQ-013 的既有入口）与编排入口
/// [`orchestrate`]。写两份等于给「溯源怎么填」开两个可以漂移的口子，
/// 而溯源漂了正是 §8.4 要防的事故 —— 所以只有一份实现。
///
/// **`items` 一个字节都不碰**：那才是提案的实质内容，
/// 覆写只动「谁抽的 / 第几轮 / 哪几组没抽出来」。
pub(crate) fn stamp_provenance(
    outcome: &mut ExtractionOutcome,
    identity: &AgentIdentity,
    failed_groups: &[GroupFailure],
) -> Result<()> {
    outcome.proposal.round = identity.round;
    outcome.proposal.extracted_by.round = identity.round;
    outcome.proposal.extracted_by.failed_groups = failed_groups
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<serde_json::Value>, _>>()
        .context("失败的分组序列化失败（要进 extracted_by.failed_groups，REQ-002 契约字段）")?;
    Ok(())
}

/// **能力标签：「本实现的产出**就是**提案的实质内容」**（REQ-019 · 缺陷 P1）。
///
/// # 这个标签挡的是什么
///
/// 编排的阶段 3+4 目前是**无条件**跑本地规则管线（[`crate::extract::propose::extract_with_context`]）
/// 生成 `items`，然后 [`stamp_provenance`] 把 **agent 的身份**盖到那份规则产物上。
/// 于是接上一个真正的外部 agent 之后会得到这样一份提案：
///
/// ```text
/// items                 = 规则引擎抽的（agent 的候选对它零影响）
/// extracted_by.agent    = 那个外部 agent
/// ```
///
/// `extracted_by` 说的与 `items` 的真实来源不是一回事 —— **静默的假溯源**，
/// 正是 [`factory`] 模块头自己点名的「本项目吃过七次亏的那一类」。
/// `factory` 因为这个理由堵死了「回落」，却没堵住「盖章层造同一类假」。
///
/// 更糟的是它还会**掩盖抽取失败**：若 agent 整组失败（`failed_groups` 非空），
/// 规则管线照样产出满当当的 items，于是「什么都没抽」与「抽得很全」
/// 在产物里长得一模一样。
///
/// # 为什么做成**能力标签**而不是 `protocol == "inproc"`
///
/// 按协议硬编码等于把「哪个协议碰巧能用」焊进代码，而这条判据问的其实是
/// 一个**实现层的事实**：阶段 3+4 的 `items` 是不是这个实现自己产的。
/// 将来某个协议 adapter 真做到了「产出即提案」，它声明同一个标签就放行 ——
/// 不需要改这里的任何一行。
///
/// # 声明放在**实现**上，不放在 agent-registry 的 `capabilities` 列
///
/// 两者是不同的东西，不要混：
///
/// - **登记里的 `capabilities`** = 用户写下的**期望/路由元数据**（谁负责哪一段）；
/// - **本 trait 的 [`ExtractionAgent::capabilities`]** = 这个实现**实际**具备什么。
///
/// 只有实现自己知道阶段 3+4 的 `items` 是不是它产的。所以闸门只看后者 ——
/// 否则任何人在登记表里写一行 `produces_proposal` 就能给自己开一道后门，
/// 假溯源照样发生，而那比回落更难发现：回落至少发生在 factory（有日志），
/// 自我声明则完全无声。
pub const CAPABILITY_PRODUCES_PROPOSAL: &str = "produces_proposal";

/// **能力标签：「我的阶段 1 产出**足以重建**提案的实质内容」**（REQ-022 · RN-2）。
///
/// # 它与 [`CAPABILITY_PRODUCES_PROPOSAL`] 的区别（不是同一个意思的两种说法）
///
/// | | `produces_proposal` | `rebuilds_proposal` |
/// | --- | --- | --- |
/// | 断言的事实 | 阶段 3+4 的 `items` **就是**本地规则管线跑的产物 | 我的候选**承载得下**提案的全部实质内容 |
/// | 编排走哪条路 | **规则路径**（`propose::extract_with_context`） | **重建路径**（`assemble::rebuild_outcome`，规格 §7.1 阶段 4 的字面做法） |
/// | 谁是今天的实现 | `RuleBasedAgent` | 无（协议 adapter 属 REQ-023） |
///
/// 两个标签**并存**、由实现自己声明，编排按标签分派 —— 仍然没有一处
/// `if agent is …RuleBasedAgent`。这条性质由
/// `tests/test_agent_orchestration.rs::test_gate_is_data_driven_not_keyed_on_protocol_or_concrete_type`
/// 盯着。
///
/// # 为什么声明它之前必须先把阶段 1 契约补齐（REQ-013 → REQ-022 的那一步）
///
/// REQ-013 时阶段 1 的候选**只有** `key` / `display_name` / `is_new` / `properties`，
/// 而提案的实体条目还要 `description` / `aliases` / `evidence`、`add_link_type` 条目
/// 整条都不投影。于是那个阶段从「产出即提案」只剩「**可声明**」——
/// 实现想放行，唯一出路是**撒谎声明** `produces_proposal`。
/// REQ-022 补齐了契约（见 [`group`]），「可声明」才变成「可**验证**」：
/// 重建路径会**逐字段核**候选是否承载得下，缺字段就**报错**而不是拼个残缺的提案。
pub const CAPABILITY_REBUILDS_PROPOSAL: &str = "rebuilds_proposal";

/// **能力标签：「本实现会上报用量」**（REQ-022 · 规格 §7.1 的成本闸门义务）。
///
/// # 为什么要做成能力标签而不是「按 protocol 判断」
///
/// 规格 §7.1 把「外置 agent 必须有成本闸门」写成**强制义务**，
/// ADR-002 把它记成 RN-2 的未落地项（N4）。但「哪些实现算外置」
/// 不是代码能答的事实（登记里的 `protocol` 是数据，且将来协议会变）。
///
/// 于是做成**实现自己签字**的标签：声明了就等于承认
/// 「我的每一次抽取都会花钱（或耗 token），而编排因此必须给我设闸」。
/// 声明之后**没设预算就不许跑** —— 义务从「文档里写着」变成
/// `orchestrate` 入口的一条 fail-closed 拒绝。
///
/// 与 [`ExtractionAgent::usage`] 的关系同 [`CAPABILITY_PRODUCES_PROPOSAL`】：
/// 标签是**声明**，方法返回值是**事实**。声明了却报不出用量是撒谎，
/// 这一点**无法在代码里证明**（与 REQ-019 里「登记表 capabilities ≠ 实现能力」
/// 同一处边界，如实记录，不假装能防）。
pub const CAPABILITY_REPORTS_USAGE: &str = "reports_usage";

// ──────────────────────────────────────────────────────────
// REQ-022 · 用量与预算（协议中立 · 规格 §7.1 成本闸门）
// ──────────────────────────────────────────────────────────

/// **一笔费用** —— 金额 + 币种（**不许只有一个数**：0.5 美元与 0.5 人民币不是一回事）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoneyAmount {
    pub amount: f64,
    pub currency: String,
}

impl MoneyAmount {
    /// 同币种相加。**币种不同则返回 `None`** —— 那不是「加起来」的问题，
    /// 是两个不可比的数，折算成什么币由**人**决定，不由本函数猜。
    pub fn checked_add(&self, other: &MoneyAmount) -> Option<MoneyAmount> {
        if self.currency != other.currency {
            return None;
        }
        Some(MoneyAmount {
            amount: self.amount + other.amount,
            currency: self.currency.clone(),
        })
    }
}

/// **一条用量上报** —— 协议中立形状（REQ-022）。
///
/// # 数据源是现成的，不要自己造计量（ADR-002 · F-2 实测）
///
/// ```text
/// {"sessionUpdate":"usage_update","used":13993,"size":1000000,
///  "cost":{"amount":0,"currency":"USD"}}
/// ```
///
/// ACP 自带 `usage_update` 通知，所以「花了多少」不需要本项目自己发明一套计量 ——
/// adapter 把那条通知映射成本类型即可。字段名沿用协议里的叫法
/// （`used` / `size`），是为了让映射是一一对应而不是「翻译」——
/// 翻译表正是会悄悄漂移的东西。
///
/// `used` 的**单位由协议自己定**（ACP 是 token）。本类型不解释它、也不换算它：
/// 预算上限也是同一种单位（[`UsageBudget::max_used`]），
/// 「拿 token 上限去比请求数」这种错配在类型上就写不出来 —— 只要两边都声明同一个
/// 单位的数字，跨协议比错就只剩**人的**疏忽。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    /// 这笔用量归属哪一组（空串 = 整轮合计，不分组）。
    pub group_id: String,
    /// 同一 `group_id` 的**第几次上报**（从 0 起）。
    ///
    /// # 为什么必须显式带，而不是靠 `group_id` 区分
    ///
    /// 编排每抽完一组就把 `usage()` 的**整个快照**读一次（要当场的累计量判闸门），
    /// 所以它必须能分辨两件长得一模一样的事：
    ///
    /// - **同一笔被读了两次** —— 同一组、同一笔账 ⇒ **只算一次**；
    /// - **重试产生的第二笔** —— 同一组、**另一笔**的钱 ⇒ **两笔都要算**。
    ///
    /// 只按 `group_id` 建键时这两件事在数据上**无法区分**，历史上选了「替换」，
    /// 于是重试的第一笔钱被静默丢掉（REQ-050）—— 闸门按偏低的数放行，
    /// 那不是记账瑕疵，是**资金风险**。
    ///
    /// ⚠️ **adapter 的义务**：每为同一组记一笔，就报它已有的笔数（0、1、2…）。
    /// 一直报 0 会退回 REQ-050 之前的少计行为。
    #[serde(default)]
    pub attempt: u32,
    /// 这一笔**自己**消耗掉的量（**增量**，不是会话累计）。
    ///
    /// ⚠️ 契约以**本类型**为准：`ExtractionAgent::usage` 返回的每一条都是
    /// 「某一次尝试自己花掉的量」。adapter 若拿到的是会话累计，
    /// **必须先换算成增量**再报（ACP 见 [`acp::protocol::UsageSample::delta_since`]）——
    /// 直接报累计会让 N 组各报一遍同一个会话总量，合计 N 倍。
    pub used: u64,
    /// 协议自报的配额上限（`usage_update.size`），`None` = 没报。
    #[serde(default)]
    pub size: Option<u64>,
    /// 协议自报的费用，`None` = 没报（**没有费用**与**没报费用**是两回事）。
    #[serde(default)]
    pub cost: Option<MoneyAmount>,
}

/// **预算闸门** —— 超了就停（规格 §7.1：「外置 agent 必须有成本闸门」）。
///
/// 两个上限**各自独立**：只有 `max_used` 也算一道闸（token 闸），
/// 只有 `max_cost` 也算（金额闸）。两个都没有 = 没设闸 ——
/// 那对**零成本实现**合理，对声明了 [`CAPABILITY_REPORTS_USAGE`] 的实现
/// **不允许**（编排会拒绝，义务不靠自觉）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageBudget {
    /// 用量上限（协议自己的计量单位）。
    pub max_used: u64,
    /// 费用上限（`None` = 不设金额闸）。币种与上报不一致时**不触发**
    /// —— 不可比的数不该由代码替人折算。
    #[serde(default)]
    pub max_cost: Option<MoneyAmount>,
}

impl UsageBudget {
    /// 当前累计量是否**已经越闸**。
    ///
    /// 用 `>=` 而不是 `>`：预算是「最多允许用多少」，用满即止。
    pub fn exceeded_by(&self, used: u64, cost: Option<&MoneyAmount>) -> bool {
        if used >= self.max_used {
            return true;
        }
        match (&self.max_cost, cost) {
            (Some(limit), Some(spent))
                if limit.currency == spent.currency && spent.amount >= limit.amount =>
            {
                true
            }
            _ => false,
        }
    }
}

/// **抽取 agent 的抽象接口** —— 协议中立。
///
/// 一个实现 = 一种「怎么想」的方式（规则引擎 / 外部协议 agent / LLM 兜底）。
/// 实现**不感知** A2A / MCP / ACP / HTTP —— 那是 adapter 的事，见模块头。
///
/// 规格 §7.1 阶段 1 对输入输出的原文：
///
/// > 每组一次调用
/// > 输入：schema 片段 + 样本 + 决策清单约束 + 领域提示 + 已有类型清单
/// > 输出：候选类型 + 属性 + 「属性 ← 来源列」的映射声明
///
/// 这五个输入在 [`ExtractionGroup`] 里一一对应，输出在 [`GroupExtraction`] 里。
pub trait ExtractionAgent {
    /// 身份 —— **必须来自 agent-registry 的真实登记**，不是硬编码默认值。
    ///
    /// 规格 §8.4 的硬要求：「一轮抽取一旦开始，agent 与其版本在整个抽取会话内锁定」。
    /// 调用方据此把 `extracted_by` 填真，而不是记「最新版」或空串。
    fn identity(&self) -> &AgentIdentity;

    /// 阶段 1：抽一组。**每组一次调用**（规格 §7.1 阶段 1）。
    ///
    /// 返回 `Err` 表示**这一组整体失败**（超时 / 协议错 / 输出不合法且重试已耗尽）。
    /// 调用方用 [`drive_group`] 施加重试并把失败转成显式的
    /// [`GroupOutcome::Failed`] —— 设计点⑤「不静默跳过」。
    ///
    /// 注意这里**不返回 Link**：规格把「两张表都有 device_id → 是否提出 Link？」
    /// 放在**阶段 3 跨组合成**，阶段 1 的产物里不该有跨组结论。
    fn extract_group(&self, group: &ExtractionGroup) -> Result<GroupExtraction>;

    /// 本实现**实际具备**的能力标签（见 [`CAPABILITY_PRODUCES_PROPOSAL`]）。
    ///
    /// # 默认空 = 「什么都不主张」⇒ 被编排**拒绝**
    ///
    /// 这个默认值是刻意的 fail-closed：新写的实现忘写这个方法时，
    /// 得到的是**一条讲清原因的报错**，而不是一份溯源是假的提案。
    ///
    /// 编排只按**标签**判，不按 `protocol`、也不按实现类型判（见编排模块头）。
    fn capabilities(&self) -> Vec<String> {
        Vec::new()
    }

    /// 本次抽取里已知失败的组。**空 = 没有失败**（规则实现恒为空）。
    ///
    /// 这些值会原样进提案元数据的 `extracted_by.failed_groups`，
    /// 且**不参与收敛判定**（设计点⑤：否则「连续两轮 diff 小于阈值」
    /// 会把「什么都没抽」误判成「已收敛」）。
    fn failed_groups(&self) -> Vec<GroupFailure> {
        Vec::new()
    }

    /// **本实现至今上报的用量**（REQ-022 · 成本闸门的数据源，协议中立）。
    ///
    /// # 语义：**每次尝试一条增量**，编排按「组 × 第几笔」并进账
    ///
    /// 返回的是**至今全部上报的快照**（编排在每组之后读整个快照，
    /// 因为闸门要的是当场的累计量）。所以：
    ///
    /// - 每一笔对应**一次尝试自己花掉的量**（[`UsageReport::used`] 是增量，
    ///   不是会话累计 —— adapter 必须先换算，见该字段的说明）；
    /// - 同一组重试 ⇒ **多条**，用 [`UsageReport::attempt`] 区分，
    ///   编排**每条都算**（REQ-050：少计会让闸门按偏低的数放行）；
    /// - 快照被重复读 ⇒ 同样的 `(group_id, attempt)` 被反复读到，
    ///   编排按该键覆盖 ⇒ **不翻倍**。
    ///
    /// # 默认空 = 「本实现没有可上报的用量」
    ///
    /// 零 I/O 的进程内规则实现如实如此（它不用模型、不耗 token）。
    /// 对外置 adapter 这是**漏报** —— 它该声明 [`CAPABILITY_REPORTS_USAGE`]
    /// 并如实填充。声明了却不报，是**撒谎**，这一点代码证明不了（如实记录）。
    ///
    /// # 为什么是 `&self` 上的方法而不是返回值
    ///
    /// [`ExtractionAgent`] 的方法全是同步的（见模块头：MSRV 下没有 `async fn in trait`
    /// 的 dyn 版本，也没有 `async-trait` 依赖），`drive_group` 也只收 `&dyn`。
    /// adapter 需要在内部累积用量，就用 `Mutex` / `Cell` 之类 —— **I/O 在 adapter 里，
    /// 编排层只读一个快照**（模块头的分层纪律）。
    fn usage(&self) -> Vec<UsageReport> {
        Vec::new()
    }
}

/// **阶段 3+4 走哪条组装路径** —— 由**能力声明**决定（REQ-022）。
///
/// | 声明的标签 | 走哪条 | 为什么
/// | --- | --- | --- |
/// | [`CAPABILITY_REBUILDS_PROPOSAL`] | [`Assembly::RebuiltFromCandidates`] | 候选承载得下提案的实质内容 ⇒ 从候选重建（规格 §7.1 阶段 4）
/// | 只有 [`CAPABILITY_PRODUCES_PROPOSAL`] | [`Assembly::RulePipeline`] | `items` 就是本地规则管线的产物 ⇒ 盖章名实相符
/// | 都没有 | — | [`orchestrate`] 拒绝（见 `require_proposal_producing`）
/// | 两个都声明 | [`Assembly::RebuiltFromCandidates`] | **重建优先**：更具体的那条声明，是实现对「我的候选能重建」的直接签字
///
/// # 为什么判据是标签而不是具体类型（这一条不能破）
///
/// 参数是 `&dyn ExtractionAgent`，函数体里**拿不到**任何具体类型 ——
/// `if agent is RuleBasedAgent` 在这里写不出来（模块头立的就是这条规矩）。
/// 判据只读 [`ExtractionAgent::capabilities`]：登记表里的 `capabilities` 列
/// **不作数**（那是人的期望），只有实现自己答得上来「`items` 是谁产的」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Assembly {
    /// 全量规则路径（既有行为，逐字节不变）。
    #[default]
    RulePipeline,
    /// 从各组候选**重建**提案（规格 §7.1 阶段 4 的字面做法）。
    RebuiltFromCandidates,
}

/// 读能力标签 → 选组装路径。**没有第三个出口**：都没声明就是 [`Assembly`] 的
/// 不变式之外的情形，由 [`orchestrate`] 的闸门先拒绝掉。
pub fn assembly_mode(agent: &dyn ExtractionAgent) -> Assembly {
    let caps = agent.capabilities();
    if caps.iter().any(|c| c == CAPABILITY_REBUILDS_PROPOSAL) {
        return Assembly::RebuiltFromCandidates;
    }
    Assembly::RulePipeline
}

/// 阶段 1 的调用外壳：**重试 + 显式失败**（规格 §7.1 设计点⑤）。
///
/// - 成功 → [`GroupOutcome::Ok`]
/// - `attempts` 次仍失败 → [`GroupOutcome::Failed`]，**带最后一次的原因**
///
/// 为什么不把重试写进 trait：重试是**编排策略**（次数、超时、是否换策略），
/// 不是 agent 自己的能力。协议 adapter 内部也许还有它自己的重试层，
/// 两者叠在一起时策略得由编排说了算，所以重试留在 trait 外面。
///
/// `attempts` 会被夹到 `>= 1` —— 传 0 的话「重试 0 次」等于「不调用」，
/// 那会让这一组**根本没被抽过**却报成功，是设计点⑤禁止的静默跳过。
pub fn drive_group(
    agent: &dyn ExtractionAgent,
    group: &ExtractionGroup,
    attempts: u32,
) -> GroupOutcome {
    let attempts = attempts.max(1);
    let mut last = String::from("（未尝试 —— attempts 被夹到至少 1，此分支不可达）");
    for _ in 1..=attempts {
        match agent.extract_group(group) {
            Ok(extraction) => return GroupOutcome::Ok { extraction },
            Err(e) => last = format!("{e:#}"),
        }
    }
    GroupOutcome::Failed {
        failure: GroupFailure {
            group_id: group.group_id.clone(),
            table: group
                .schema
                .first()
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "(空组)".to_string()),
            attempts,
            reason: last,
        },
    }
}
