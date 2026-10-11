//! **编排入口** —— 按表分组 → 逐组 [`drive_group`] → 阶段 2 核对 → 汇总失败组。
//!
//! # 缺陷 F2：抽象层名义可插拔、管线没接通
//!
//! REQ-013 建好了 [`ExtractionAgent`] 与 [`drive_group`]，但生产入口
//! `RuleBasedAgent::new(...)` + `.propose(...)` **完全绕开**它们：
//! 构造具体类型、调固有方法，编排原语一个调用点也没有。
//! 本文件是那个缺失的**接线点**。
//!
//! # 这个入口做什么 / 不做什么
//!
//! ```text
//! 阶段 0 · 准备       分组（[derive_group_id]，一表一组）
//! 阶段 1 · 分组抽取   逐组 drive_group(agent, group, attempts)   ← 外包给 agent
//! 阶段 2 · 验证       verify_mappings(&extraction, group)        ← ★ 本文件，纯代码
//!                      失败组 + 违规项汇总 → extracted_by.failed_groups
//! 阶段 3+4 · 组装     ★ **两条路并存**，由能力声明决定（见下）
//!                      · 规则路径：既有的全量 propose::extract_with_context
//!                      · 重建路径：从各组候选 assemble::rebuild_outcome（§7.1 阶段 4）
//! ```
//!
//! ## 阶段 3+4 为什么**两条路并存**（REQ-022 · 根问题的正面回答）
//!
//! REQ-019 之前这里是**一条**规则路径，然后 [`stamp_provenance`] 把
//! 调用方那个 agent 的身份盖到**规则产出的 `items`** 上：
//!
//! ```text
//! items              = 规则引擎抽的（agent 的候选对它零影响）
//! extracted_by.agent = 调用方登记的那个 agent      ← 对不上号
//! ```
//!
//! 于是 adapter 在**物理上产不出提案** —— 想过 REQ-019 的能力闸门，
//! 唯一出路是**撒谎声明** [`CAPABILITY_PRODUCES_PROPOSAL`]。
//! 闸门从「防呆」变成了「逼你签字」。
//!
//! REQ-022 补上第二条路：声明 [`CAPABILITY_REBUILDS_PROPOSAL`] 的实现，
//! 阶段 4 **由它自己的候选**重建提案条目（`assemble::rebuild_outcome`，
//! 规格 §7.1 阶段 4 的字面做法）。
//! 「产出即提案」于是从「**可声明**」变成「**可验证**」：
//! 重建路径逐字段核候选承载得下承载不下，**缺字段就报错**，
//! 而不是拼一份缺了描述/别名/证据/关系的残缺提案。
//!
//! ## 为什么仍然没有 `if agent is …`
//!
//! 选路问的是**能力标签**（[`assembly_mode`]），不是类型、不是 `protocol`。
//! 编排签名仍只收 `&dyn ExtractionAgent` —— 函数体里拿不到具体类型，
//! 所以「按类型分支」这件事在类型上就不成立（模块头立的就是这条规矩）。
//!
//! ## ★ REQ-022：成本闸门（规格 §7.1 的强制义务）
//!
//! 外置 agent 必须有成本闸门。数据源**现成**（ADR-002 · F-2 实测：ACP 自带
//! `usage_update` 通知），本文件只做**协议中立**的三件事：
//!
//! - [`ExtractionAgent::usage`]：实现上报用量的抽象（默认空 = 零 I/O 实现如实如此）；
//! - [`UsageBudget`]：闸门由**装配层**给（[`OrchestrationInput::budget`]），
//!   编排层只判「越没越」；
//! - **显式的停**：越闸就停，**已跑完的组**与**没跑的组**分两栏记，
//!   后者进 [`BudgetReport::skipped_groups`] ——
//!   **不进 `failed_groups`**：成本超限与抽取失败是两回事，
//!   混在一起就等于让「我们没钱了」伪装成「这个表抽不出来」。
//!
//! 声明 [`CAPABILITY_REPORTS_USAGE`] 的实现**必须**给预算（[`require_budget`]）——
//! 义务从「文档里写着」变成入口上的一条 fail-closed 拒绝。
//!
//! ## 阶段 2 为什么必须在这里、且必须由代码做（架构红线）
//!
//! 规格 §7.1：「若连『提议的依据是否属实』都由 agent 自己判定，这句话就空转了」。
//! 所以拿到每组的 [`GroupExtraction`] 之后，**立刻**用 [`verify_mappings`]
//! 在代码侧核对 agent 的映射声明（表/列是否真实存在、key 是否自洽），
//! 违规项**显式汇总**、**只报错不丢弃**（丢弃与否是策略，本函数不替策略做主）。
//!
//! 违规项不写进 `proposal.json` / `stats.json`（那是 REQ-002 的既有契约，
//! 加字段就破坏下游），由调用方（CLI）显式报出。
//!
//! ## 阶段 3+4 为什么仍是全量规则路径（如实记录，不是偷懒）
//!
//! 规格把「两张表都有 `device_id` → 是否提出 Link？」放在**阶段 3 跨组合成**，
//! 而 [`GroupExtraction`] **按设计不投影 Link**。更硬的约束是：
//! 阶段 1 的候选形状 [`CandidateType`] **不带 `description` / `aliases` /
//! `evidence`**，而提案的实体条目要带 —— 所以「从候选纯代码重建提案」
//! （规格 §7.1 阶段 4 的字面做法）**在 REQ-013 定的阶段 1 契约下会改变提案内容**。
//! 改阶段 1 契约超出本任务范围，故阶段 3+4 沿用既有的
//! [`crate::extract::propose::extract_with_context`] 全量路径。
//!
//! **代价（如实报，不藏）**：规则实现下阶段 1 与阶段 3+4 各跑一遍规则管线
//! （真实 schema 上是 91 组 + 1 次全量）。这是分阶段架构的必然成本，
//! 不是重复劳动的疏忽 —— 换成协议 agent 时阶段 1 是网络调用，
//! 阶段 3+4 仍是一次本地全量，成本结构完全不同。
//!
//! ## ★ REQ-019：这份「代价」曾经是一台**假溯源制造机**
//!
//! 上面这段设计里藏着一处**装配层讲原则、盖章层破原则**的破口：
//! 阶段 3+4 是**无条件**跑规则实现，然后 [`stamp_provenance`] 把
//! **调用方那个 agent 的身份**盖到**规则产出的 `items`** 上。
//!
//! ```text
//! items              = 规则引擎抽的（agent 的候选对它零影响）
//! extracted_by.agent = 调用方登记的那个 agent      ← 对不上号
//! ```
//!
//! 实测（QA 自造的模型 agent）：agent 提了含 8 条违规的候选，提案却与规则
//! agent **逐字节相同**。接真 agent 之后更糟：阶段 1 整组失败时
//! `failed_groups` 非空而 `items` 照样满当当 ——
//! **「什么都没抽」与「抽得很全」在产物里长得一样**。
//!
//! 本任务**只挡住，不造对**：让 agent 的候选真正成为提案需要改阶段 1 契约
//! （`CandidateType` 加 `description` / `aliases` / `evidence`），
//! 那是 RN-2 的活。所以这里的判据是**能力闸门**（[`require_proposal_producing`]）：
//! agent 必须**显式声明** [`CAPABILITY_PRODUCES_PROPOSAL`]，
//! 否则编排**明确拒绝**并说清原因 —— 宁可拒绝，不发假溯源的提案。

use anyhow::{Context, Result};

use super::assemble::{
    classified_contract_gaps as rebuild_contract_gaps, rebuild_outcome, ContractGap, GapKind,
    GroupCandidates,
};
use super::group::{
    derive_group_id, verify_mappings, CandidateType, ColumnSample, DecisionList, ExistingType,
    ExtractionGroup, GroupFailure, GroupOutcome, GroupSource, MappingViolation,
};
use super::{
    assembly_mode, drive_group, stamp_provenance, ExtractionAgent, Assembly, MoneyAmount,
    UsageBudget, CAPABILITY_PRODUCES_PROPOSAL, CAPABILITY_REBUILDS_PROPOSAL,
    CAPABILITY_REPORTS_USAGE,
};
use crate::extract::introspect::SchemaReport;
use crate::extract::names::TableNames;
use crate::extract::propose::{self, ExtractionOutcome};

// ──────────────────────────────────────────────────────────
// REQ-019 · 默认重试次数
// ──────────────────────────────────────────────────────────

/// **每组的默认尝试次数**（规格 §7.1 设计点⑤「校验失败重试 1–2 次，仍失败才标记抽取失败」）。
///
/// # 为什么默认值必须 > 1（REQ-019 · 隐患「重试在生产路径等于关闭」）
///
/// REQ-015 时生产入口把这个值**钉死成 1**，于是 [`drive_group`] 的重试能力
/// 在真实运行里**从不生效**。对规则实现无害（纯函数、无 I/O，必然成功），
/// 但接上网络 agent 之后：**超时 = 秒失败、零重试** —— 与规格明写的
/// 「重试 1–2 次才标记抽取失败」直接冲突，且失败得毫无征兆。
///
/// 换句话说：**重试对规则实现无意义，对网络 agent 是必需品。**
/// 默认值取 2（规格区间的中位），它对规则实现不产生任何行为变化
/// —— 首次成功即返回，不会有第二次调用。
pub const DEFAULT_ATTEMPTS: u32 = 2;

/// **「这一轮比对了哪一版」的缺省值 = 1**（第 1 轮没有基版本）。
///
/// 见 [`OrchestrationInput::base_version`]。1 不是随手取的整数 ——
/// 它就是 REQ-002 契约里第 1 轮提案 `base_version` 的既有取值
/// （`fixtures/proposal-demo.json`），保持它才让第 1 轮产物逐字节不变。
pub const DEFAULT_BASE_VERSION: u32 = 1;

// ──────────────────────────────────────────────────────────
// 编排输入
// ──────────────────────────────────────────────────────────

/// 一次编排运行需要的、**与 agent 无关**的那些东西。
///
/// 全部按引用/默认值给出，所以 CLI 只要三行就能构造。
pub struct OrchestrationInput<'a> {
    /// 表名翻译表（REQ-003）—— 阶段 3+4 的规则路径要用。
    pub names: &'a TableNames,
    /// 提案项目名（`Proposal.project`）。
    pub project: &'a str,
    /// 决策清单约束（规格 §7.1 阶段 1 的输入之一）—— 传给每组，
    /// 阶段 2 判禁用词时用。
    pub constraints: DecisionList,
    /// 已有类型清单（设计点①）—— 传给每组，阶段 2 核 key 自洽时用。
    pub existing_types: Vec<ExistingType>,
    /// **`existing_types` 是从哪一版载入的**（REQ-019 · 版本归因修复）。
    ///
    /// 会原样写进 [`Proposal::base_version`](crate::extract::propose::Proposal::base_version)，
    /// 而那一列是下游 `ontok propose import` 选基版本、
    /// `apply` 取 diff 快照的**唯一依据**（规格 §8.4 的版本锁定归因）。
    ///
    /// # 为什么它必须与 `existing_types` 同源
    ///
    /// 提案里的 `base_version` 与「这一轮实际比对了哪一版」是两个独立事实。
    /// 它们一旦分叉，**错得无声**：阶段 1 的 key 双向核对是拿 A 版验的
    /// （`ExistingDisplayNameChanged` 里回显的「已知显示名」来自 A），
    /// 而 apply 的 diff 基线挂在 B 版上 —— 于是
    /// 「`t_dev` 的显示名相对 B 版没改」与「相对 A 版改名了」同时为真，
    /// 审核期看到的是前者，阶段 2 报出来的是后者，谁也没错，结论却相反。
    ///
    /// 所以本字段**不是**「提案想挂在哪一版」（那是 import 的决定），
    /// 而是**「这一轮实际比对了哪一版」的事实陈述**，
    /// 由装配层连同 `existing_types` 一起从库里解析出来。调用方有义务
    /// 让两者一致 —— 换基版本就得连 `existing_types` 一起换。
    ///
    /// 默认 [`DEFAULT_BASE_VERSION`]（= 1）：`round = 1` 没有基版本可比，
    /// 「已有类型」按定义为空，声明 1 就是如实的。
    pub base_version: u32,
    /// 领域提示（「这是一家做 SaaS 计费的厂商」之类）。
    pub domain_hints: Vec<String>,
    /// 样本（脱敏）。**按组作用域切分**：一个样本只随它自己那张表的组走，
    /// 不跨组泄漏（样本是「这一列长什么样」的证据，跨组就没有意义了）。
    pub samples: Vec<ColumnSample>,
    /// 每组重试次数（设计点⑤）。`drive_group` 内部会夹到 `>= 1`。
    ///
    /// 默认 [`DEFAULT_ATTEMPTS`]（= 2），**不是 1**：规则实现下重试无意义，
    /// 网络 agent 下它是必需品（规格 §7.1 设计点⑤）。见该常量的文档。
    pub attempts: u32,
    /// **成本闸门**（REQ-022 · 规格 §7.1「外置 agent 必须有成本闸门」）。
    ///
    /// `None` = 不设闸。**零成本实现（进程内规则引擎）如实如此** ——
    /// 它不调模型、不耗 token，给它编一个预算上限只是表演。
    ///
    /// ⚠️ 对声明了 [`CAPABILITY_REPORTS_USAGE`] 的实现，`None` 会被
    /// [`require_budget`] **拒绝**：规格把闸门写成强制义务，
    /// 义务不能靠自觉，只能靠入口上的一条拒绝。
    ///
    /// 闸门触发后的行为见 [`BudgetReport`] —— 特别是
    /// **没跑的组记在 `skipped_groups`，不进 `failed_groups`**。
    pub budget: Option<UsageBudget>,
}

impl<'a> OrchestrationInput<'a> {
    /// 最小构造：只有翻译表与项目名，其余取默认
    /// （无约束 / 无已有类型 / 每组 [`DEFAULT_ATTEMPTS`] 次尝试）。
    ///
    /// ⚠️ **默认不是 1**（REQ-019）：见 [`DEFAULT_ATTEMPTS`] 的理由 ——
    /// 钉死 1 等于在生产路径上关掉重试，而规格 §7.1 设计点⑤要求「重试 1–2 次」。
    /// 调用方仍可显式改写这个字段（`input.attempts = n`），改写即生效。
    pub fn new(names: &'a TableNames, project: &'a str) -> Self {
        Self {
            names,
            project,
            constraints: DecisionList::default(),
            existing_types: Vec::new(),
            base_version: DEFAULT_BASE_VERSION,
            domain_hints: Vec::new(),
            samples: Vec::new(),
            attempts: DEFAULT_ATTEMPTS,
            budget: None,
        }
    }
}

// ──────────────────────────────────────────────────────────
// 阶段 0 · 分组
// ──────────────────────────────────────────────────────────

/// 按表分组（阶段 0）。**一表一组**，组 id 用 [`derive_group_id`] 给。
///
/// # 为什么「一表一组」而不是更大的块
///
/// 设计点②：哪些表属于同一业务域**正是要判断的事**，交给 agent 分组就成了循环依赖。
/// 表边界是客观的，跨表归并留给阶段 3。
///
/// # 上下文带全库表名，但不进作用域（REQ-014 · F1）
///
/// `context_tables` 放**全库**表名（存在性判据，只给 FK 消歧用），
/// `schema` 只放本组那一张表。二者**必须都传**，否则组外的 FK 目标会被当成
/// 不存在，属性名回落成注释值 —— 那条缺陷的判据测试是
/// `test_fk_target_outside_group_scope_still_yields_relation_name`。
fn build_groups(report: &SchemaReport, input: &OrchestrationInput<'_>) -> Vec<ExtractionGroup> {
    let context_tables: Vec<String> = report.tables.iter().map(|t| t.name.clone()).collect();
    let source = GroupSource {
        schema: report.schema.clone(),
        introspected_at: report.introspected_at.clone(),
        server_version: report.server_version.clone(),
        transaction_read_only: report.transaction_read_only.clone(),
        foreign_key_count: report.foreign_key_count,
    };
    report
        .tables
        .iter()
        .map(|t| ExtractionGroup {
            group_id: derive_group_id(&t.name),
            source: source.clone(),
            schema: vec![t.clone()],
            context_tables: context_tables.clone(),
            samples: input
                .samples
                .iter()
                .filter(|s| s.table == t.name)
                .cloned()
                .collect(),
            constraints: input.constraints.clone(),
            domain_hints: input.domain_hints.clone(),
            existing_types: input.existing_types.clone(),
        })
        .collect()
}

// ──────────────────────────────────────────────────────────
// 阶段 1+2 的结果（显式，可核对）
// ──────────────────────────────────────────────────────────

/// 一条映射违规 + 它出自哪一组 —— **只报错不丢弃**。
///
/// 带上 `group_id` / `table` 是为了能指名道姓：「哪一组、哪张表、
/// 哪个类型、哪个属性」四层定位，少一层就只能靠猜。
#[derive(Debug, Clone)]
pub struct GroupViolation {
    pub group_id: String,
    pub table: String,
    pub violation: MappingViolation,
}

/// 阶段 1 + 阶段 2 的执行结果 —— **全部显式**，没有一项是「静默跳过」。
#[derive(Debug, Clone, Default)]
pub struct StageReport {
    /// 分组总数（阶段 0 产出几组）。
    pub groups: usize,
    /// 抽成功的组数。
    pub groups_ok: usize,
    /// 成功组里 agent 提的候选类型数（不含跨组合成）。
    pub candidate_types: usize,
    /// 成功组里 agent 提的候选属性数。
    pub candidate_properties: usize,
    /// 成功的组里 agent 提的**关系候选**数（REQ-022 · 阶段 3 的输入）。
    pub candidate_links: usize,
    /// 失败的组 —— `drive_group` 的失败 **与** [`ExtractionAgent::failed_groups`]
    /// 自报的失败合并去重后的结果（设计点⑤）。
    pub failed_groups: Vec<GroupFailure>,
    /// 阶段 2 查出的映射违规。**非空必须报出来**，不许吞。
    pub violations: Vec<GroupViolation>,
    /// **阶段 3+4 实际走的是哪条组装路径**（REQ-022）。
    ///
    /// 为什么单列一个字段而不是让调用方猜：两条路径的产物可能**长得一样**
    /// （规则实现走重建路径时逐字节相同），
    /// 于是「这一轮到底走没走重建」在产物里看不出来 ——
    /// 而「悄悄退回规则路径」正是最需要被看见的那种退化。
    /// 调用方（CLI）必须把它打出来。
    pub assembly: Assembly,
    /// **成本闸门的结果**（REQ-022）。`skipped_groups` 非空 = 越闸停了。
    pub budget: BudgetReport,
}

// ──────────────────────────────────────────────────────────
// REQ-022 · 成本闸门的结果（显式三栏：跑完 / 没跑 / 为什么停）
// ──────────────────────────────────────────────────────────

/// **预算闸门的执行结果** —— 「停」这件事必须留痕，而且分得清是哪一种停。
///
/// | 记在哪 | 含义 |
/// | --- | --- |
/// | [`StageReport::failed_groups`] | 这一组**试过但没抽出来**（超时 / 协议错 / 重试耗尽）|
/// | [`BudgetReport::skipped_groups`] | 这一组**根本没跑**（钱用完了）—— 不是抽取失败 |
/// | [`StageReport::groups_ok`] | 跑完且成功 |
///
/// ⚠️ **两者绝不可合并**：把「没钱了」写进 `failed_groups` 会让
/// `extracted_by.failed_groups` 说「这些表抽不出来」，
/// 而下游判读把它读成「抽取质量问题」。
/// 成本超限是**运行环境的决定**，抽取失败是**实现的事实**。
#[derive(Debug, Clone, Default)]
pub struct BudgetReport {
    /// 设的上限（`None` = 没设闸）。回显出来是为了让人看清「按哪道闸停的」。
    pub limit_used: Option<u64>,
    pub limit_cost: Option<MoneyAmount>,
    /// 本轮**累计**用量（把每一次尝试各自的增量相加 —— 每条上报都是**增量**，
    /// 所以必须全加；见 [`BudgetReport::absorb`]）。
    pub used: u64,
    /// 本轮累计费用（协议没报就是 `None`；「没报」与「0」是两回事）。
    pub cost: Option<MoneyAmount>,
    /// **是否越闸停了**（规格 §7.1「超预算即停并上报」）。
    pub stopped: bool,
    /// 因越闸**没有跑的组** —— 显式，与 `failed_groups` 分开（见上表）。
    pub skipped_groups: Vec<String>,
    /// 停的原因（人话，进 CLI 输出，不进提案产物）。
    pub reason: Option<String>,
    /// ★ REQ-051：**上报碰撞** —— 同一个 `(group_id, attempt)` 报出了**不同的钱**。
    ///
    /// 每一项是一句人话。非空 = **已停**（见 [`BudgetReport::absorb`]）。
    ///
    /// # ⚠️ 「撞键了」**不等于**「账已经错了」（REQ-052 订正）
    ///
    /// REQ-051 最初在这里写的是「碰撞 ⇒ 本轮账本不可信」。**那句话对一半**：
    /// 它把**两种后果完全不同**的情形混成了一句。
    ///
    /// | `group_id` | 被顶掉的旧值属于谁 | 账对不对 |
    /// | --- | --- | --- |
    /// | **空** | 整个 round 共用的那**一个**键 | **可能仍是对的** —— `totals()` 取最后一次读；若实现报的是**整轮口径累计**，那就是正确总额。坏掉的是**增量契约** |
    /// | **非空** | **这一组自己**的那一笔 | **定不下来** —— 取决于这个实现的**累计口径**：本组口径累计 ⇒ 账对；整轮/会话口径累计 ⇒ **多计**；增量 ⇒ **少计**、真的资金缺口。逐条见 [`BudgetReport::absorb`] |
    ///
    /// ⚠️ 特别地：**空 `group_id`** 恰恰**不会**造成 N 倍计入
    /// （只有一个键，`insert` 覆盖而非叠加）—— 「N 倍计入」只在**每组各有
    /// 不同 `group_id`** 时才成立。QA 实测：两组各 +100 ⇒ `used = 200`（正确总额）。
    ///
    /// 那为什么仍然当成**比越闸更严重**的一类停：越闸只说明「钱花到头了」，
    /// 数字本身可信；碰撞说明**这个实现的账本不可信** —— 不管这一轮的数字
    /// 恰好对不对，**契约已经破了**，继续跑就是拿一个算不清的账去花更多的钱。
    /// 方向是 fail-closed 的：**停 = 白跑，不会多花钱**。
    ///
    /// ⚠️ **它只可能来自 adapter 没遵守 [`crate::agent::UsageReport::attempt`] 契约**
    /// （新 adapter 忘编号、或恒报 0 —— 那正是 REQ-050 的复发形态）。
    /// 编排侧无法证明实现遵守了契约，于是把矛盾**显式炸出来**而不是覆盖掉。
    pub usage_collisions: Vec<String>,
    /// **每一笔账**（`(group_id, 该组第几笔)` → 该笔的 `used` / `cost`）。
    ///
    /// # 键为什么是「组 × 第几笔」而不是只有 `group_id`
    ///
    /// 编排每抽完一组就把 `usage()` 的**整个快照**读一次（闸门要当场的累计量），
    /// 所以同一笔账会被**反复读到**；而同一组重试会产生**好几笔不同的钱**。
    /// 这两件事只按 `group_id` 分不开：
    ///
    /// | 快照里出现的东西 | 是同一笔吗 | 该怎么处理 |
    /// | --- | --- | --- |
    /// | 同一 `(group_id, attempt)`，值也相同 | **是**（快照被重读）| 覆盖 ⇒ 不翻倍 |
    /// | 同一 `group_id`，`attempt` 不同 | **否**（重试的钱）| 都算 ⇒ 不能少算 |
    ///
    /// ⚠️ REQ-050 之前这里只按 `group_id` 建键、**覆盖**：
    /// 于是同组重试的第一笔钱被静默顶掉，而 `used` / `cost` 是**整轮累计**、
    /// 直接喂给 [`UsageBudget::exceeded_by`] —— 少计 = **闸门按偏低的数放行**。
    by_report: std::collections::BTreeMap<(String, u32), (u64, Option<MoneyAmount>)>,
}

/// 把一条费用写进人话（碰撞信息里要看得见它，且**不回显任何凭据** —— 这里只有金额与币种）。
fn fmt_money(c: Option<&MoneyAmount>) -> String {
    match c {
        Some(m) => format!(" / {:.6} {}", m.amount, m.currency),
        None => "（没报费用）".to_string(),
    }
}

/// 两笔同键上报**是不是同一笔钱**（REQ-051 碰撞检测的判据）。
///
/// # 为什么不能直接用 `==`
///
/// `MoneyAmount` derive 了 `PartialEq`，但 `f64` 的 `NaN != NaN`：
/// 一个报了 `NaN` 金额的退化实现会被判成「两笔不同」⇒ **误报** ⇒
/// 一次误报会把所有重试场景变红，比漏检更糟（AC-002 的硬要求）。
/// 所以 NaN 与 NaN 单独算**相同** —— 判据只认「值真的不一样」。
///
/// `serde_json` 解析不出 `NaN`（JSON 没有这个字面量），所以真实路径走不到这里；
/// 留着它是为了让「零假阳性」这个论断不依赖「没人会手工造 `NaN`」这种假设。
fn report_same_spend(
    a: &(u64, Option<&MoneyAmount>),
    b: &(u64, Option<&MoneyAmount>),
) -> bool {
    if a.0 != b.0 {
        return false;
    }
    match (a.1, b.1) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.currency == y.currency
                && (x.amount == y.amount || (x.amount.is_nan() && y.amount.is_nan()))
        }
        // 「报了费用」与「没报费用」是两回事 —— 形态不同就是不同的一笔。
        _ => false,
    }
}

impl BudgetReport {
    /// 把实现上报的用量并进本轮账，并按闸门判定是否越闸。
    ///
    /// # 语义：**每条上报都是一次尝试自己花掉的增量，全部相加**（REQ-050）
    ///
    /// 旧实现按 `group_id` **覆盖**，理由是「协议报的是累计值」——
    /// 但两个 adapter 上报的**都已经是增量**
    /// （ACP 在 `UsageSample::delta_since` 里做过 baseline 差分，
    /// LLM 是请求级响应），所以那条理由不成立，
    /// 覆盖只会把重试的账吃掉。真相以 [`crate::agent::UsageReport::used`] 为准。
    ///
    /// **为什么不能改成「无脑累加每条上报」**：`agent.usage()` 返回的是
    /// **全量历史快照**（编排在每组之后重读整个列表），
    /// 无脑累加会让先前每一组的账被**反复计入**（A 组 120、B 组 90 ⇒ 记成 330）。
    /// 按 `(group_id, attempt)` 建键正好躲开这一点：
    /// 快照重读命中同一个键 ⇒ 覆盖不翻倍；重试落到新键 ⇒ 新账进账。
    fn absorb(&mut self, reports: &[super::UsageReport], limit: Option<&UsageBudget>) {
        for r in reports {
            // ★ REQ-051 · 碰撞检测：同一个 `(组, 第几笔)` 报出了**不同的钱**。
            //
            // **热路径代价 = 0**：`BTreeMap::insert` 本来就要走一次查找才能定位，
            // 而它**返回被顶掉的旧值** —— 判据就搭在这个返回值上，
            // 所以碰撞检测没有多一次 map 查找，只多一次 `u64` 比较
            // 与一次只在 `Option` 有值时才做的 `MoneyAmount` 比较。
            // 非碰撞路径（正常路径）连格式化都不会发生。
            //
            // 两种可能，**只有一种**是真碰撞：
            // · 同键**同值** = 快照被重读（编排在每组之后重读整个 `usage()`）⇒ 放行；
            // · 同键**异值** = 该实现没遵守 `attempt` 契约（恒报 0 / 忘编号）
            //   ⇒ 停下并记下来。
            //
            // ★ REQ-052：**「没递增 attempt」在两种情形下后果完全不同** ——
            // 所以下面那句人话必须**分开说**，不能一律喊「账本不可信」。
            // 判据是 `group_id` 是不是空的（`insert` 已经把旧值顶掉了，
            // 但**旧值还留不留得下来**，取决于它属于谁）：
            //
            // · **空 `group_id`** ⇒ 整个 round 只占**一个**键 `("", attempt)`，
            //   而 `totals()` 取的是**最后一次读到的值**。若这个实现报的是
            //   **整轮累计**（空 `group_id` 通常就是这个意思），那最后一次读到的
            //   **恰是本轮正确总额** ⇒ **账是对的**，坏掉的是增量契约。
            //   ⚠️ 单键覆盖**不会**把各组的账叠起来 ⇒ 空 `group_id`
            //   **不会**造成 N 倍计入（这一点 QA 实测过：两组各 +100 ⇒ used = 200）。
            //   ⚠️ REQ-054：这里**没有**上文那种「本组 vs 整轮」的口径之分 ——
            //   整个 round 只占一个键，「按组累计」与「整轮累计」根本分不开。
            //   真正要看的是它报的是**不是整轮口径的累计**：报增量、或**按组
            //   各自报总额**，前面的组都已被顶掉 ⇒ **一样会少计**
            //   （REQ-053 的原文只点了「增量」，把「按组报总额」漏在条件之外）。
            // · **非空 `group_id`** ⇒ 撞上同一个键同样说明**没有递增 attempt**
            //   （判定依据与空 `group_id` 一模一样），被顶掉的是**这一组自己
            //   上一读**的值。账本最后留下的是**本次读到的那个数**，所以
            //   「哪个数才是对的」**取决于这个实现的累计口径** ——
            //   ⚠️ REQ-054：**光分「累计 / 增量」还不够，累计本身还有口径之分**。
            //   三种都说得通，哪一种都**不能**从这条信息里推断出来：
            //   · **本组口径的累计**（本组 120 → 280，280 就是这一组的总额）
            //     ⇒ 最后一读恰是该组的正确总额 ⇒ **账是对的**（坏掉的是增量契约）；
            //   · **整轮 / 会话口径的累计**（本组报的是**含别的组**的总额 ——
            //     本组自己只花了 280，连同前面已入账的 100 一起报成 220 → 380）
            //     ⇒ 账上留下 380 ⇒ **前面那个组的钱被再算一遍** ⇒ `used` **多计**。
            //     ⚠️ 方向与少计**相反**：多记**不会**多花钱（[`UsageBudget::exceeded_by`]
            //     只会被抬得更高 ⇒ 更早停），但账本偏高会把排障的人带偏。
            //     ⚠️ 「闸门提前停」在**单轮内够不到**：碰撞已经让本轮 `stopped`、
            //     闸门**未被求值**；只有口径**跨 round** 时才真会提前停 ——
            //     下一轮是**空账本**（[`orchestrate`] 每轮新建 [`BudgetReport`]），
            //     第一读就偏高、**不再有碰撞**，闸门才被真的求值。
            //   · **增量**（120 → 160）⇒ 前一笔已被顶掉、不在账本任何地方 ⇒ `used`
            //     [`UsageBudget::exceeded_by`] **少计** ⇒ 闸门可能按偏低的数放行
            //     ⇒ **真的有资金缺口**（三种里**唯一**会多花钱的方向）。
            //   ⇒ 排障**该核对的是两件事**：这个组报的是累计还是增量；
            //     若累计，累计的**口径**是**本组**、**整轮/会话**、还是**跨 round**。
            //   （REQ-053 的文案只说了「累计 vs 增量」，漏了口径 ⇒ 被 QA 实测证伪：
            //   第 1 组花 100 入账、第 2 组报整轮累计 220 → 380 撞键，
            //   账上 **480**、真花 **380** ⇒ **多计 100**，「账是对的」仍是假的。
            //   该路径**单轮内可达**、生产可达（一次编排可达 91 组）。）
            //   ⚠️ 还有一种**只在费用上错开**的：同键 `used` 相同、`cost` 不同
            //   （`report_same_spend` 两个字段都比）⇒ 用量这一栏可能仍对，
            //   错的是**费用闸** —— 别把「费用不同」一律读成「用量错」。
            //
            // · **重试恰好花同样的钱** ⇒ 同键**同值** ⇒ `report_same_spend` 判同、
            //   **根本不触发** ⇒ 静默少计、零诊断。**本块不覆盖它**：判据就是
            //   「同键异值」，要覆盖它就得改触发条件，而触发条件是 AC-003 的
            //   硬约束（见下）。**既有盲区**，非本需求引入。
            //
            // ⚠️ 两种都停：**判定依据（有没有递增 attempt）完全一样**，
            // 这里只改**人话**，**不改触发条件**（AC-003 的硬约束）。
            let bumped = self
                .by_report
                .insert((r.group_id.clone(), r.attempt), (r.used, r.cost.clone()));
            if let Some(prev) = bumped {
                if !report_same_spend(&(prev.0, prev.1.as_ref()), &(r.used, r.cost.as_ref())) {
                    // 同一个触发、两种裁决 —— 差别只在**人话怎么说**。
                    let verdict = if r.group_id.is_empty() {
                        "**这一轮的钱数仍可能是对的**：空 group_id ⇒ 整个 round 只占一个\
                         键 (\"\", attempt)，totals() 取最后一次读到的值。\
                         若这个实现报的是**整轮口径的累计**，那正是本轮的正确总额\
                         （单键覆盖不会把各组的账叠起来 ⇒ 不会出现 N 倍计入）。\
                         坏掉的是 `UsageReport.used` 的**增量契约**，**不是这一轮的账**。\
                         ⚠️ 但若它报的是**增量**，或**按组各自报总额**，前面的组已被顶掉、\
                         账就少计了 ⇒ 去核对这个实现报的是**不是整轮口径**。"
                    } else {
                        "**账不一定错，但哪个数才是对的还定不下来**：非空 group_id 撞上同一个键 ⇒ 这个实现\
                         **没有递增 attempt**，被顶掉的是**这一组自己上一读**的值，\
                         账本最后留下的是**本次读到的数**。⚠️ **累计和增量都还不够分** —— \
                         累计还有**口径**之分，三种都说得通、从这条信息里**分不出来**：\
                         ① **本组口径的累计**（本组 120 → 280，280 即该组总额）⇒ 最后一读恰是\
                         该组的正确总额 ⇒ **账是对的**（坏掉的是增量契约）；\
                         ② **整轮 / 会话口径的累计**（本组报的是**含别的组**的总额：本组自己只花 280，\
                         连同前面已入账的 100 一起报成 220 → 380）⇒ 前面那个组的钱被**再算一遍** ⇒ \
                         **多计** ⇒ 账偏高（**不会**多花钱；只有口径**跨 round** 时才可能让闸门\
                         提前停，同一轮里碰撞已让本轮停、闸门未被求值）；\
                         ③ **增量**（120 → 160）⇒ 前一笔已被顶掉、不在账本任何地方 ⇒ used **少计** \
                         ⇒ 闸门可能按偏低的数放行 ⇒ **真的有资金缺口**（三者里**唯一**会多花钱的）。\
                         ⚠️ 若同键 `used` 相同、只有 `cost` 不同 ⇒ 用量这一栏可能仍对，错的是**费用闸**。\
                         ⇒ **去核对两件事**：这个组报的是累计还是增量；若累计，累计的**口径**是\
                         **本组**、**整轮/会话**、还是**跨 round**。"
                    };
                    self.usage_collisions.push(format!(
                        "组 {group} 的第 {attempt} 笔上报了两次且**用量不同**：\
                         上一读 used={prev_used}{prev_cost}，\
                         本次读 used={new_used}{new_cost}。\n\
                         同一个 (group_id, attempt) 本该**只对应一笔钱**；\
                         撞出不同的值说明这个实现在同组重试时**没有递增 attempt**。{verdict}",
                        group = r.group_id,
                        attempt = r.attempt,
                        prev_used = prev.0,
                        prev_cost = fmt_money(prev.1.as_ref()),
                        new_used = r.used,
                        new_cost = fmt_money(r.cost.as_ref()),
                    ));
                }
            }
        }
        let (used, cost) = self.totals();
        self.used = used;
        self.cost = cost;
        // ★ 碰撞**先于**闸门判，且原因**粘性**：
        // 账本不可信时不该让「已用 X / 上限 Y」覆盖掉真正的病因 ——
        // 后者只会把人引去调预算，而病根在 adapter。
        if !self.usage_collisions.is_empty() {
            self.stopped = true;
            // ⚠️ 汇总行**不**下「账本不可信」的定论（REQ-052）：空 `group_id` 与非空
            // `group_id` **都**可能账仍对、契约已破（REQ-003 去掉了「非空即账错」
            // 的错误定论），逐条的「该核对什么」在每条碰撞信息里。
            self.reason = Some(format!(
                "用量上报碰撞（{} 处）—— 账本存疑，已停（每处的原因见下）：\n{}",
                self.usage_collisions.len(),
                self.usage_collisions.join("\n")
            ));
            return;
        }
        let Some(limit) = limit else { return };
        if limit.exceeded_by(used, self.cost.as_ref()) {
            self.stopped = true;
            self.reason = Some(match (&self.cost, &limit.max_cost) {
                (Some(spent), Some(climit)) if climit.currency == spent.currency => format!(
                    "费用闸：已花 {:.6} {}，上限 {:.6} {}（用量 {used}）",
                    spent.amount, spent.currency, climit.amount, climit.currency
                ),
                _ => format!("用量闸：已用 {used}，上限 {}", limit.max_used),
            });
        }
    }

    /// 合计：**每一条上报各自的值**相加（每条都是一次尝试的增量）。
    ///
    /// ⚠️ **`None` 有两种含义，别混**（REQ-050 · AC-003）：
    /// 「这一轮没人报过费用」与「报过，但其中两笔币种不可比」。
    /// 后者**必须一直是 `None`** —— 不可比之后若让后来的可比数把它填回去，
    /// 等于**凭空造出一个钱数**，费用闸就会拿这个假数判停不停。
    fn totals(&self) -> (u64, Option<MoneyAmount>) {
        let mut used = 0u64;
        // 「没人报过费用」与「报过但**不可比**」都让 `cost` 为 `None`，
        // 可它们**不是同一件事** —— 所以不能只靠 `Option` 区分：
        // 一旦某两笔币种不可比，**后来的可比数不许把它救回来**。
        let mut cost: Option<MoneyAmount> = None;
        let mut incomparable = false;
        for (u, c) in self.by_report.values() {
            used = used.saturating_add(*u);
            // 币种不可比就返回 None（`checked_add`）—— 那不是「加起来」的问题。
            let Some(c) = c else { continue };
            if incomparable {
                continue;
            }
            cost = match cost {
                None => Some(c.clone()),
                Some(a) => match a.checked_add(c) {
                    Some(sum) => Some(sum),
                    None => {
                        incomparable = true;
                        None
                    }
                },
            };
        }
        (used, cost)
    }

    /// 没设闸时给 CLI 报的第一句人话。
    pub fn configured(&self) -> bool {
        self.limit_used.is_some() || self.limit_cost.is_some()
    }
}

/// **REQ-022：声明了「我会上报用量」就必须给预算**（fail-closed）。
///
/// 规格 §7.1 把成本闸门写成外置 agent 的**强制义务**，
/// ADR-002 记成 RN-2 的未落地项（N4）。
/// 不设成「入口拒绝」的话，它就只是一句文档里的话 ——
/// 而文档里的话，正是本项目这串 REQ 在一个个消灭的东西。
///
/// 判据是**能力标签**（[`CAPABILITY_REPORTS_USAGE`]），
/// 不按 `protocol`、不按具体类型 —— 与另外两个闸门同一纪律。
fn require_budget(agent: &dyn ExtractionAgent, input: &OrchestrationInput<'_>) -> Result<()> {
    if input.budget.is_some() {
        return Ok(());
    }
    if !agent.capabilities().iter().any(|c| c == CAPABILITY_REPORTS_USAGE) {
        return Ok(());
    }
    anyhow::bail!(
        "拒绝抽取：agent {agent_name:?} 声明了能力 {CAPABILITY_REPORTS_USAGE:?}（它会上报用量），\
         但这次编排没有给预算。\n\
         \n\
         为什么必须拒：规格 §7.1 把成本闸门写成外置 agent 的**强制义务**，\
         ADR-002 记的 N4「成本闸门未落地」说的就是这一条。\n\
         缺了闸门，一次 91 组的外置抽取能烧掉多少钱**事先没人知道**，\
         烧完了也没有一个地方记下「为什么停在这」。\n\
         \n\
         怎么跑通：给 `OrchestrationInput::budget` 一个上限（`--budget-tokens` / 未来的 `--budget-cost`）。\n\
         \n\
         零成本实现（进程内规则引擎）**不需要**闸门，也不该声明这个能力 —— \
         它不调模型、不耗 token，声明了反而要按这份契约去上报用量。",
        agent_name = agent.identity().agent,
    )
}

/// 一次完整运行的结果 = 提案（含统计）+ 阶段 1/2 的显式报告。
pub struct ExtractionRun {
    pub outcome: ExtractionOutcome,
    pub stage: StageReport,
}

// ──────────────────────────────────────────────────────────
// 编排
// ──────────────────────────────────────────────────────────

// ── REQ-019 · 能力闸门：没有「产出即提案」的声明就**拒绝组装** ──

/// # 为什么放在**阶段 3+4 之前**、而不是产完再校验
///
/// 放之前：拒绝发生在**任何产出之前** —— 没有提案、没有统计、没有文件，
/// 调用方拿到的只有一条讲清原因的 `Err`。
///
/// 放之后：`items` 已经跑完规则管线，再判断就是「算了再倒掉」，
/// 而且中途还可能有人把结果捡走 —— 那种「拒绝」是靠不住的。
///
/// # 判定依据是**能力标签**，不是 `protocol`
///
/// 见 [`CAPABILITY_PRODUCES_PROPOSAL`] 的模块文档：闸门问的是
/// 「阶段 3+4 的 `items` 是不是这个实现自己产的」这个**实现层事实**，
/// 只有实现自己答得上来（[`ExtractionAgent::capabilities`]）。
/// 所以将来哪个协议 adapter 真做到了，声明同一个标签即可放行，
/// 判据本身一行都不用改。
///
/// # 为什么「拒绝」优于「照常产出一份盖了错章的提案」
///
/// 假溯源不是「差一点对」，是**全错**：`extracted_by` 与 `items` 的真实来源
/// 对不上号，于是 §8.4 的版本锁定与 diff 归因**全部失真**，而且没人会发现 ——
/// 提案看起来是完整的。**宁可拒绝。**
fn require_proposal_producing(agent: &dyn ExtractionAgent) -> Result<()> {
    let caps = agent.capabilities();
    if caps.iter().any(|c| c == CAPABILITY_PRODUCES_PROPOSAL)
        || caps.iter().any(|c| c == CAPABILITY_REBUILDS_PROPOSAL)
    {
        return Ok(());
    }
    let id = agent.identity();
    anyhow::bail!(
        "拒绝组装提案：抽取 agent {agent_name:?} 没有声明任何「产出即提案」的能力。\n\
         \n\
         为什么拒：阶段 3+4 的 `items` 必须由这个实现自己产出（规则路径），\
         或者由它自己的候选重建（重建路径）。两条都不满足时，提案内容是规则引擎的、\
         extracted_by 记的却是 {agent_name:?} —— 两边对不上号，\
         规格 §8.4 的版本锁定与 diff 归因**全失真**，而且是**静默**的假。\
         同一处还会掩盖抽取失败：agent 整组失败（failed_groups 非空）时，\
         items 照样是满的，「什么都没抽」与「抽得很全」在产物里长得一样。\n\
         \n\
         它现在声明的能力：{caps:?}\n\
         \n\
         怎么才能跑通（两条路，任选其一）：\n\
           1) 用**进程内规则实现**（登记 protocol=\"inproc\"）—— 阶段 3+4 的\
n             规则产物**就是**它的产出，盖章名实相符（声明 {CAPABILITY_PRODUCES_PROPOSAL:?}）；\n\
           2) 让 {agent_name:?} 的阶段 1 契约**逐字段承载提案要写的每一项**\
n             （描述 / 别名 / 证据 / 属性说明 / 可空 / 名字来源 / 关系候选），\
n             由编排从候选**重建**提案（规格 §7.1 阶段 4 的字面做法），\
n             再如实声明 {CAPABILITY_REBUILDS_PROPOSAL:?}。\
n             重建路径会**逐字段核**承载得下承载不下，缺字段直接报错 ——\
n             声明了却承载不下，就等于给一份残缺提案盖章。\n\
         \n\
         在那之前宁可拒绝：发一份溯源是假的提案，比不发更糟。",
        agent_name = id.agent,
    )
}

/// **阶段 1 + 2 的编排入口**：按表分组 → 逐组 [`drive_group`] → 逐组
/// [`verify_mappings`] → 汇总失败组 → 阶段 3+4 组装。
///
/// `agent` 是 `&dyn ExtractionAgent`：**函数体内没有任何具体类型分支**，
/// 调用方也拿不到具体类型。接第二个实现不需要改这个函数。
///
/// # 阶段 3+4 走哪条路（REQ-022）
///
/// [`assembly_mode`] 按**能力标签**选：
/// 声明 [`CAPABILITY_REBUILDS_PROPOSAL`] → 从候选重建（§7.1 阶段 4）；
/// 只声明 [`CAPABILITY_PRODUCES_PROPOSAL`] → 既有的全量规则路径。
/// 两条路的 `items` 都由**如实声明**的能力担保，
/// 所以 [`stamp_provenance`] 盖上 agent 身份在两条路上都是实话。
///
/// # 成本闸门在**阶段 1 之前**就要有答案（REQ-022）
///
/// [`require_budget`] 与 [`require_proposal_producing`] 一样放在最前面：
/// 声明了会上报用量却没给预算 ⇒ **一行业务代码都不跑**。
/// 为什么不能「先抽几组再发现没钱」：那已经是你**多花的钱**了。
///
/// 越闸之后：**剩下的组一律不调** [`drive_group`]，进
/// [`BudgetReport::skipped_groups`]；已经跑完的组照常走完阶段 2 与组装，
/// 提案是**部分**的 —— 部分这件事必须由调用方**显式报出**（CLI 会以非零退出），
/// 而不是让一份少了一半的提案长得和完整的一样。
///
/// # 失败组汇总为什么是两路合并
///
/// - [`drive_group`] 的失败：重试耗尽、这一组**确实没抽出来**；
/// - [`ExtractionAgent::failed_groups`] 的自报：agent 内部有编排层看不见的
///   失败（例如协议 adapter 内部降级、某组只出了一半结果）。
///
/// 只读前者会漏掉后者，只读前者会让 `failed_groups()` 变成**没人调用的
/// 死方法**（那正是 REQ-013 留下的问题之一）。按 `group_id` 去重合并，
/// 免得同一组在 `extracted_by.failed_groups` 里出现两条。
///
/// # 统计口径（REQ-015 的明确要求）
///
/// [`ExtractionOutcome::stats`] **全部来自阶段 3+4 的作用域**，
/// **组级统计一律不进**。理由是 `ExtractionGroup::to_schema_report` 只按本组
/// 表重算 `counts`（`group.rs` 已确立），若把各组 `tables_seen` / 属性数
/// 加总后与全量数混在一起，`翻译覆盖 = N / tables_seen` 这类比值的分母
/// 就会在说谎 —— 而 CLI 打印的正是这类比值。
///
/// 组级的东西只出现在 [`StageReport`] 里，**单位是组**，不冒充全局数。
/// （重建路径的全局统计由**合并后的候选**重算，见
/// [`crate::agent::assemble::rebuild_outcome`]。）
///
/// # ★ 先过两道闸门（REQ-019 / REQ-022）
///
/// [`require_proposal_producing`] 与 [`require_budget`] 放在**最前面**：
/// 任一没过，一行业务代码都不跑。
pub fn orchestrate(
    agent: &dyn ExtractionAgent,
    report: &SchemaReport,
    input: &OrchestrationInput<'_>,
) -> Result<ExtractionRun> {
    require_proposal_producing(agent)?;
    require_budget(agent, input)?;

    let groups = build_groups(report, input);
    let assembly = assembly_mode(agent);
    let mut stage = StageReport {
        groups: groups.len(),
        assembly,
        budget: BudgetReport {
            limit_used: input.budget.as_ref().map(|b| b.max_used),
            limit_cost: input.budget.as_ref().and_then(|b| b.max_cost.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    // 重建路径要用的各组候选（阶段 3 的输入）。规则路径不需要它 ——
    // 那边阶段 3+4 有自己的全量作用域。
    let mut candidates: Vec<GroupCandidates> = Vec::new();

    for group in &groups {
        // ── REQ-022 · 成本闸门：越闸 ⇒ 剩下的组**不调 agent** ──
        // 失败与「没跑」分两栏记（见 BudgetReport 的文档）。
        if stage.budget.stopped {
            stage.budget.skipped_groups.push(group.group_id.clone());
            continue;
        }

        let table = group
            .schema
            .first()
            .map(|t| t.name.clone())
            .unwrap_or_else(|| "(空组)".to_string());

        // ── 阶段 1：外包给 agent，但重试与「失败要显式」由 drive_group 兜住 ──
        match drive_group(agent, group, input.attempts) {
            GroupOutcome::Ok { extraction } => {
                stage.groups_ok += 1;
                stage.candidate_types += extraction.types.len();
                stage.candidate_links += extraction.links.len();
                stage.candidate_properties +=
                    extraction.types.iter().map(|t: &CandidateType| t.properties.len()).sum::<usize>();

                // ── 阶段 2：★ 代码侧核对，不可外包（规格架构红线）──────────
                // 注意它在**候选之后、组装之前**：先核对再组装，
                // 组装路径拿到的就已经是「核过声明」的候选。
                for violation in verify_mappings(&extraction, group) {
                    stage.violations.push(GroupViolation {
                        group_id: group.group_id.clone(),
                        table: table.clone(),
                        violation,
                    });
                }

                if assembly == Assembly::RebuiltFromCandidates {
                    // ── ★ 抽取后守卫：声明了产出能力，就得**真的**承载得住 ──
                    //
                    // 时机是这里，不是阶段 1 之前：判据必须晚于它要核对的那份答案。
                    // （曾经的形状是 `capabilities()` 读一个由 `extract_group` 填的
                    // 运行时状态 —— 判据在数据产生**之前**执行，于是那个 `if`
                    // 分支经本函数这唯一生产入口**不可达**。）
                    //
                    // # ★ 分级：结构性缺口 bail，内容类缺口降级为组级失败
                    //
                    // 曾经的形状是「任何缺口 ⇒ 整轮 bail」，理由是「契约缺口是
                    // **系统性**的」。那句在 REQ-024 自己新补的两条判据上**不成立** ——
                    // `nullable` 缺一个、`links[].evidence` 缺一条、某一组**合法地**
                    // 返回 0 个类型：这些是**答案内容类**缺口，只此一份答案的这一次。
                    // 拿整轮处决去处理它们，等于「一组空答案拖垮另外两组已齐备的成果」
                    // —— 而 QA-A 给的正是这个反例（`t_region` 返回 0 类型 ⇒ 整轮崩）。
                    //
                    // 所以判据按 [`GapKind`] 分流（判据本身见 `assemble::gap_kind_of`，
                    // 是**可测的**：按字段统计「填了几个 / 适用几个」，不靠注释分类）：
                    //
                    // - **结构性**（`0 / n`，n > 0）：整份答案**每个实例**都缺这个字段 ⇒
                    //   实现层面承载不下 ⇒ `bail!`。此时「后面每一组都会以同样方式不合」
                    //   不再是假设，是**这份答案自己给出的证据**，继续跑是白烧钱。
                    // - **内容类**（`k / n` 且 k > 0，或 `0 / 0`）：承载能力已被这份答案
                    //   证明，只是这一组/这一项抽漏了 ⇒ 按设计点⑤记**组级失败**、
                    //   继续跑，最终报告显式列出（`extracted_by.failed_groups`，
                    //   且按 ADR-001 **不参与收敛判定** —— 这本来就不是结构性违约，
                    //   而是一次抽取失败）。
                    //
                    // 未声明 `rebuilds_proposal` 的实现**不进这一支**
                    // （它们不走重建路径，守卫对它们不适用）。
                    let gaps = rebuild_contract_gaps(&extraction);
                    let structural: Vec<&ContractGap> = gaps
                        .iter()
                        .filter(|g| g.kind == GapKind::Structural)
                        .collect();
                    if !structural.is_empty() {
                        anyhow::bail!(
                            "阶段 4 重建契约守卫拦下了组 {}（表 {table}）—— **结构性**缺口。\n\
                             \n\
                             这个实现声明了 {CAPABILITY_REBUILDS_PROPOSAL:?}，\
                             而这一组的候选**逐项核对下来承载不下**提案：\n{}\n\
                             \n\
                             为什么这里就整轮停，而不是记一次失败继续跑：\n\
                             · 阶段 4 的 fail-closed 要等**所有组**跑完才炸，那时提案还是没有，\
                               钱已经花完了；\n\
                             · 只记 failed_groups 继续跑也一样 —— 上面的缺口是**结构性**的：\
                               这一份答案里**没有任何一个实例**提供这些字段，\
                               同一个实现后面每一组都会以同样的方式不合，\
                               继续跑只是把钱花在注定失败的组上。\n\
                             \n\
                             怎么办：把上面点名的字段**如实**补上（注意 nullable 与 links[].evidence \
                             是**事实与锚点**，不是判断），或者别声明 {CAPABILITY_REBUILDS_PROPOSAL:?} —— \
                             声明了却承载不下，正是这条守卫存在的意义。",
                            group.group_id,
                            structural.iter().map(|g| format!("  · {}", g.text)).collect::<Vec<_>>().join("\n"),
                        );
                    }
                    if !gaps.is_empty() {
                        // 内容类：这一组**没抽出来**，不牵连后面的组。
                        //
                        // ⚠️ 这里要把 `groups_ok` **减回去**：`drive_group` 成功时已经加过了，
                        // 但这一组的候选被守卫丢掉了，**并没有产出任何东西**。不减的话
                        // 这一组会同时算进 `groups_ok` 和 `failed_groups`，
                        // 而 `groups_ok + failed_groups.len()` 在预算提示里表示
                        // 「已跑的组数」—— 同一组数两遍，那句话就在说谎。
                        stage.groups_ok = stage.groups_ok.saturating_sub(1);
                        stage.failed_groups.push(GroupFailure {
                            group_id: group.group_id.clone(),
                            table: table.clone(),
                            // ⚠️ 照实记 **1**，不是 `input.attempts`。
                            //
                            // [`drive_group`] 只在 `extract_group` 返回 `Err` 时重试，
                            // 一旦 `Ok` 就**立刻返回**（`crate::agent::drive_group`
                            // 里「Ok 就 return」的那一支）——
                            // 它根本没有「成功之后再试一次」这个动作。这里是对
                            // 那次**已经成功返回**的答案做事后核对，所以这一组
                            // 实际只被调用了 1 次。写 `input.attempts`（默认 2）
                            // 是在 `GroupFailure::attempts`（文档写明「实际尝试次数」）
                            // 里放一个数出来的假数 —— 而这个字段会进提案元数据。
                            attempts: 1,
                            reason: format!(
                                "重建契约**内容类**缺口（承载能力已证明，这一组没抽齐）：\n{}",
                                gaps.iter().map(|g| format!("  · {}", g.text)).collect::<Vec<_>>().join("\n")
                            ),
                        });
                        // 这一组**不进** `candidates` —— 它的候选是残缺的，
                        // 让它进重建路径就是「补默认值」或「静默丢一半」。
                        continue;
                    }
                    candidates.push(GroupCandidates {
                        group_id: group.group_id.clone(),
                        extraction,
                    });
                }
            }
            GroupOutcome::Failed { failure } => stage.failed_groups.push(failure),
        }

        // ── REQ-022：每抽完一组就把实现上报的用量并进账（ADR-002-F-2）──
        // 协议 agent 的用量是**在抽取过程中**陆续上报的（ACP 的 `usage_update`
        // 是流式通知），所以必须在**每组之后**读一次，而不是最后读一次 ——
        // 最后读一次的话，越闸已经晚了：超预算的那些组全跑完了。
        stage.budget.absorb(&agent.usage(), input.budget.as_ref());
    }

    // agent 自报的失败组也要进来 —— 不读它，这条路径就是死代码，
    // 而「失败被静默吞掉」正是设计点⑤禁止的那件事。
    for f in agent.failed_groups() {
        if !stage
            .failed_groups
            .iter()
            .any(|known| known.group_id == f.group_id)
        {
            stage.failed_groups.push(f);
        }
    }

    // ── 阶段 3+4：两条并存的路，由能力声明选（模块头说明了为什么）────────
    //
    // ⚠️ REQ-019：能走到这里，说明 `require_proposal_producing` 已经确认
    // 「items 真的是这个 agent 产的（按它声明的方式）」。**正因为如此**，
    // 下面给 items 盖上 agent 身份才是如实的 —— 闸门拿掉，
    // 这一行就成了静默的假溯源。
    let identity = agent.identity();
    let mut outcome = match assembly {
        Assembly::RulePipeline => propose::extract_with_context(
            report,
            &[],
            input.names,
            input.project,
            &identity.agent,
            &identity.version,
        )
        .context("阶段 3+4 全量组装失败")?,
        Assembly::RebuiltFromCandidates => {
            // ★ 整轮级兜底：**一个候选都没有** ⇒ 不许重建出一份 0 条目的提案。
            //
            // 内容类缺口降级成组级失败之后，「所有组都没抽出来」这条路径就不再
            // 由守卫的 `bail!` 兜着了（那是逐组的判据，逐组看每一组都合法）。
            // 而 0 条目的提案**比报错更糟**：report 里有表，提案里却一个实体都没有，
            // 看起来完整、内容是空的。所以「整轮什么都没有」在这里 fail-closed。
            //
            // 注意这**只**在零候选时触发：部分组失败（`failed_groups` 非空）
            // 照常重建 —— 那正是设计点⑤要的「部分失败显式暴露」，
            // 调用方能从 `extracted_by.failed_groups` 与非零退出码看见。
            if candidates.is_empty() {
                anyhow::bail!(
                    "阶段 3+4 重建路径一个候选都没有 —— 拒绝产出一份 **0 条目**的提案。\n\
                     \n\
                     共 {} 组，成功产出候选 0 组，抽取失败 {} 组。\n\
                     为什么 0 条目必须报错：报告里有 {} 张表，而这份提案里一个实体都没有 —— \
                     它**看起来完整、内容是空的**，比报错更难被发现。\n\
                     为什么这里才拦：守卫是**逐组**判据（「这一组能不能承载契约」），\
                     「所有组都没承载」这件事只有汇总之后才看得见。\n\
                     失败组的明细见上面每组的失败原因（本轮一条都没进候选，\
                     所以失败原因全在 failed_groups 里）。",
                    stage.groups,
                    stage.failed_groups.len(),
                    report.tables.len(),
                );
            }
            rebuild_outcome(&candidates, report, input.project, &identity.agent, &identity.version)
                .context("阶段 3+4 从候选重建失败")?
        }
    };
    // 溯源填真（§8.4）：round / extracted_by.round / failed_groups 三处。
    // `items` 一个字节都不碰 —— 那才是提案的实质内容。
    stamp_provenance(&mut outcome, identity, &stage.failed_groups)?;

    // REQ-019 · 版本归因：`base_version` 记的是**这一轮实际比对了哪一版**，
    // 而 `existing_types` 正是从那一版载入的 —— 同一个 `input` 上的两个字段，
    // 所以它们不可能分叉。放在 `stamp_provenance` **之后**、不并进那个函数，
    // 是因为它不是 agent 的身份（`AgentIdentity` 不含它，协议 agent 也不必知道），
    // 而是**装配层**从库里解析出来的事实。见 [`OrchestrationInput::base_version`]。
    //
    // ⚠️ 原来这里是 `extract_with_context` 写死的 1 —— 与 `existing_types`
    // 的实际来源各说各话，且错得无声（第 2 轮比的是 vN，声明的却是 v1）。
    outcome.proposal.base_version = input.base_version;

    Ok(ExtractionRun { outcome, stage })
}

