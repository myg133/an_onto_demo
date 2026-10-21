//! 源库 → 本体提案抽取器（REQ-003 · AC-103）
//!
//! 输入 [`introspect::SchemaReport`]，输出 REQ-002 提案 JSON ——
//! 也就是 `ontok propose import` 能直接吃进去的格式。
//!
//! # 为什么拆成「确定性骨架 + 语义表」两半
//!
//! 真实源库 `<业务库名>` 的实测结果逼出了这个设计：
//!
//! | 事实 | 数字 | 后果 |
//! | --- | --- | --- |
//! | 表注释 | **3 / 91** | `display_name` 几乎无从自动获取 |
//! | 列注释 | 681 / 1051（65%） | 属性名大多**有**黄金信息 |
//! | 外键 | **0** | 关系只能靠 `*_id` 命名约定推断 |
//!
//! `display_name` 是硬约束：REQ-002 的 `usable_as_human_name` 规定
//! 「词元命中任一实体 key 的值不可用」，所以**英文表名当 display_name 会让实体整批消失**
//! （REQ-002 评估的场景 A：91 张表存活 0）。
//!
//! 于是：
//! - **确定性部分**（本文件）：表→实体、列→属性、`*_id`→关系、列注释→属性名。
//!   纯规则、可重跑、可 diff。
//! - **语义部分**（[`names::TableNames`]）：表名→中文名的翻译表。
//!   由 LLM 产出、**随代码入库**、带 `extracted_by` 标记 ——
//!   这正是 REQ-002 AC-011/AC-014 想解决的问题（「谁在哪个版本翻的」）。
//!
//! 抽不到中文名的表**照常产出实体**，只是会被 `usable_as_human_name` 过滤掉 ——
//! 显式的失败比静默的缺失好：E2E 报告能数出「有多少张表没翻出来」。

use super::introspect::{ColumnMeta, SchemaReport, TableMeta};
use super::names::TableNames;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// ──────────────────────────────────────────────────────────
// 提案的数据形状
//
// 与 REQ-002 的 `fixtures/proposal-demo.json` 对齐 —— 改这里要同步改那份 fixture。
// ──────────────────────────────────────────────────────────

/// 一个实体类型（对应源库一张表）。
///
/// **属性嵌套在实体内部**（`properties`），不是独立的 `add_property` 条目 ——
/// 这是 REQ-002 提案契约的形状，`ontok propose import` 只认这一种。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityItem {
    pub kind: String,
    /// `target` 是给人看的标识（REQ-002 里是中文显示名）。
    pub target: String,
    pub payload: EntityPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPayload {
    pub key: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub aliases: Vec<String>,
    /// 溯源：这份本体从哪张表哪几列推出来的。客户视图不显示（`evidence` 在守门词里）。
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub evidence: Vec<serde_json::Value>,
    pub properties: Vec<PropertyItem>,
}

/// **提案侧的溯源块**（REQ-002 契约字段 `extracted_by`）。
///
/// # ⚠️ REQ-022 改名：它**不是** `crate::domain::snapshot::ExtractedBy`
///
/// REQ-013~020 期间本类型也叫 `ExtractedBy`，与快照层那个**同名不同型** ——
/// 两者形状不同（`failed_groups` 一个是结构化 JSON、一个是 `Vec<String>`），
/// 持久化位置也不同（一个进 `proposal.json`，一个进 `ontology_versions`）。
/// 同名两型已经造成过一次真实的误读（ADR-003 的额外坑），所以：
///
/// - **提案侧**叫 `ProposalProvenance`（一份提案的「谁抽的」）；
/// - **快照侧**保留 `snapshot::ExtractedBy`（落库的那个，名字与 JSON 字段对齐）。
///
/// # `source` 这个字段名在两侧**含义不同** —— 别混
///
/// - 本类型的 [`ProposalProvenance::source`] = **勘查留痕**
///   （`introspect <时刻> schema=<schema> read_only=<on/off>`），REQ-003 增补，
///   它证明的是「这批 schema 是什么时候、在哪个 schema、只读事务里读出来的」；
/// - `snapshot::ExtractedBy::source` = **来源通道**（工具自产 / 外部声明），
///   ADR-003 决策 (b) 加的，证明的是「这个溯源值是本工具跑出来的，
///   还是从一份外部 JSON 里读进来的」。
///
/// 两个字段都叫 `source`，但一个是「schema 的来源」，一个是「声明的来源」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalProvenance {
    pub agent: String,
    pub version: String,
    /// 抽取轮次。
    pub round: i32,
    /// 本轮失败的分组（REQ-002 契约字段，AC-015 用）。
    #[serde(default)]
    pub failed_groups: Vec<serde_json::Value>,
    /// 勘查时刻 + 只读留痕（REQ-003 增补）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// 一个属性（对应源库一列）。嵌套在实体的 `properties` 里。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyItem {
    /// **必须中文或为空**。空则回落 `humanize_column(column)`，仍认不出就丢弃。
    pub name: String,
    /// 源库列名 —— 抽取溯源用，客户视图不显示。
    pub column: String,
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 一个关系（零外键下靠命名约定推断）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkItem {
    pub kind: String,
    pub target: String,
    pub payload: LinkPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkPayload {
    pub key: String,
    pub from_type: String,
    pub to_type: String,
    pub cardinality: String,
    pub semantic: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub evidence: Vec<serde_json::Value>,
}

/// 完整提案 —— **顶层形状与 REQ-002 的 `fixtures/proposal-demo.json` 一致**：
/// `project` / `base_version` / `round` / `extracted_by` / `items`。
///
/// ⚠️ 改这里必须同步改那份 fixture，否则 `ontok propose import` 直接报「提案缺 project」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub project: String,
    pub base_version: u32,
    pub round: i32,
    pub extracted_by: ProposalProvenance,
    pub items: Vec<serde_json::Value>,
}

// ──────────────────────────────────────────────────────────
// 抽取
// ──────────────────────────────────────────────────────────

/// **提案 `round` 的既有取值 = 1**（REQ-002 契约 + `fixtures/proposal-demo.json`）。
///
/// 抽取器写死的默认值；走 agent 路径时由 `stamp_provenance` 覆写成
/// [`crate::agent::registry::AgentIdentity::round`] 的真值。
/// 提出常量是为了「重建路径」与「规则路径」**用同一个数** ——
/// 两处各写一个 `1` 看着一样，改的时候只改一处就分叉了。
pub(crate) const DEFAULT_PROPOSAL_ROUND: i32 = 1;

/// **提案 `base_version` 的既有取值 = 1**（第 1 轮没有基版本可比）。
///
/// 抽取器写死的默认值；走编排路径时由 `OrchestrationInput::base_version` 覆写
/// —— 那一版与 `existing_types` 同源（见 `orchestrate` 的版本归因说明）。
pub(crate) const DEFAULT_PROPOSAL_BASE_VERSION: u32 = 1;

/// **勘查留痕那句话**（`ProposalProvenance::source` 的取值）。
///
/// REQ-022 抽成函数是因为**两个调用点必须说出同一句话**：
/// 规则路径与「从候选重建」路径。若各写一份 format!，
/// 将来改其中一个就会让两种来源的提案在 provenance 上分叉 ——
/// 而 `extracted_by.source` 是 §8.4 要给人解释 diff 的凭据之一。
pub(crate) fn introspect_provenance(report: &SchemaReport) -> String {
    format!(
        "introspect {} schema={} read_only={}",
        report.introspected_at, report.schema, report.transaction_read_only
    )
}

/// 抽取结果 + 统计（统计用于 E2E 报告，判定器要读）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionOutcome {
    pub proposal: Proposal,
    pub stats: ExtractionStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionStats {
    pub tables_seen: usize,
    /// 属性总数（嵌套在实体 payload 里）。
    pub total_properties: usize,
    /// 拿到了中文 display_name 的实体数 —— AC-104 的核心指标。
    pub entities_with_cn_name: usize,
    /// 只能靠 `humanize_column` 兜底才进得了客户视图的属性数。
    pub props_via_wordlist: usize,
    /// 直接用了列注释的属性数 —— 抽取质量的主要来源。
    pub props_via_comment: usize,
    /// **REQ-008**：属性名是**从关系推导**出来的数量（外键）。
    ///
    /// 单列一栏是因为它**不是**注释的功劳：以前这些列的显示名由
    /// 「这一张表的注释碰巧怎么写」决定，于是同一个 `customer_id`
    /// 在客户视图上有 4 种叫法。记在 `props_via_comment` 里
    /// 会把「关系推的」和「注释说的」混成一笔糊涂账。
    #[serde(default)]
    pub props_via_relation: usize,
    /// **REQ-005**：注释被判为低质量（一个汉字都没有、且不是合法枚举值域）、
    /// 因而**没有**被当属性名的数量。
    ///
    /// 这不是丢弃 —— 它们会回退到 `humanize_column()` 的词表/形态推断，
    /// 兜底也认不出时才由 `project()` 丢弃（S-2）。
    ///
    /// 单独计数是为了让这次「主动降级」**可见**：
    /// REQ-005 的教训是 fail-open（错数据静默发给客户，机器不报错），
    /// 所以每一次降级都得在统计里留下痕迹，而不是悄悄发生。
    #[serde(default)]
    pub props_comment_rejected: usize,
    /// 认不出、会被 `project()` 丢弃的属性数。
    pub props_dropped: usize,
    pub links_inferred: usize,
    /// 关系指向了不存在实体的数量 —— **必须为 0**，否则 AC-007 会拦。
    pub links_dangling: usize,
    /// 没翻出中文名的表（会被客户视图整批过滤掉）。
    pub tables_without_cn_name: Vec<String>,
}

/// 关系后缀 → (cardinality, semantic)。零外键下靠 `*_id` 命名约定推断。（REQ-087：这里的 `N:1` 是命名约定的复述，**不是观测结论** —— 本表主键列的覆写与全部已知限制登记见 [`crate::extract::cardinality`]）
const REL_SUFFIXES: &[(&str, &str, &str)] = &[
    ("_ids", "1:N", "has"),
    ("_id", "N:1", "belongs_to"),
    ("_no", "N:1", "references"),
    ("_code", "N:1", "references"),
];

/// 从源库 schema 抽本体提案 —— **既有入口，签名与产出逐字节不变**。
///
/// # REQ-013：这是**规则引擎实现**的包壳（规格 §7.1 阶段 1 的第一个 agent）
///
/// 函数体**一行未改** —— 既有 5 个调用点
/// （`tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `tests/test_prop_name_quality.rs`
/// / `src/bin/ontok-propose.rs`）的签名与产出逐字节不变。
///
/// 真正的 agent 抽象在 [`crate::agent::ExtractionAgent`]，
/// 规则实现是 [`crate::agent::RuleBasedAgent`]。
///
/// **与 agent 路径的关系（准确版，别写成「完全等价」）**：
/// [`crate::agent::RuleBasedAgent::propose`] 内部就是调用本函数，然后**额外覆写**
/// `round` / `extracted_by.round` / `failed_groups` 三个溯源字段
/// （见 `src/agent/rule_based.rs`）—— 本函数**不**覆写这三个。
/// 所以只有当 `AgentIdentity::legacy_cli(...)` 那种 `round = 1` 的身份
/// （`legacy_cli` 恒把 `round` 钉成 1，且 `failed_groups` 恒为空，
/// 与本函数硬编码的值相同）时，两条路径才**逐字节相同**；
/// `round ≠ 1` 时**不等价** —— agent 路径会填真值，本函数仍写死 1。
/// 保留这个薄壳是为了让既有调用点**零改动**。
///
/// ⚠️ **`round` 在函数体里硬编码 1**：这是 REQ-013 之前的历史行为，
/// 走 agent 路径时由 [`crate::agent::RuleBasedAgent::propose`] 覆写为
/// registry 登记的真值。本壳**不改**它 —— 改了就不是逐字节不变了。
///
/// # REQ-014：本函数 = 「上下文为空」
///
/// 等价于 [`extract_with_context`]（`report`, `&[]`, …），即**已知表集合只由
/// 作用域构成**。既有 5 个调用点走的都是这条全量路径（作用域 = 全库），
/// 所以**不需要改一个字节**；阶段 1 的分组路径见 [`extract_with_context`]。
pub fn extract(
    report: &SchemaReport,
    names: &TableNames,
    project: &str,
    agent: &str,
    agent_version: &str,
) -> Result<ExtractionOutcome> {
    extract_with_context(report, &[], names, project, agent, agent_version)
}

/// **分组抽取入口**（规格 §7.1 阶段 1「每组一次调用」）。
///
/// # 作用域 vs 上下文（REQ-014 · 缺陷 F1）
///
/// `SchemaReport.tables` 原本**同时扮演两个角色**：
///
/// 1. **作用域** —— 这一趟要遍历哪些表（产出实体 / 属性 / 统计）；
/// 2. **已知表集合** —— [`infer_link`] 靠它判断「`customer_id` 指向的表
///    是不是真的存在」，据此在 `customers` / `customer_rebate` 之间消歧。
///
/// 全量路径下两者重合，于是这个双重身份一直没人看见。**一按表分组就分家了**：
/// 组的作用域只有本组那几张表，而 FK 目标常常在**组外**。于是
/// 「`t_dev.t_customer_id`」这条列在阶段 1 拿不到关系，
/// 属性名回落成注释的「客户标识」；同一条列在全量路径（阶段 3/4）上
/// 却是关系推导的「所属客户」。**同一列、同一 schema、两个名字。**
///
/// 本函数把两个角色拆开：
///
/// - `report.tables` = **作用域**，逐字不变（守住设计点②「分组按表，不按业务域」——
///   本函数**不做任何跨表聚类**，一组仍然是那几张表）；
/// - `context_tables` = **上下文**，只参与 [`infer_link`] 的存在性判断，
///   **不遍历、不产出实体、不进统计**。
///
/// 理由：「目标表是否真实存在」是**可核验的事实**，不是 agent 的判断。
/// 设计点①说 agent 做映射不做命名、③说证据必须机器可验证 ——
/// 那么「`customer_id` 指向哪张真实存在的表」就该由代码在**完整上下文**里解出来，
/// 而不是因为分组缩小了视野就解不出来。
///
/// # ⚠️ 作用域受限时 `links_dangling` 的口径
///
/// 函数末尾的 AC-007 / AC-103 自检要求关系**两端都在实体集合里**。
/// 作用域只有一组时，指向组外表的关系会被这道自检记成 `links_dangling`。
/// **这不是缺陷**：阶段 1 不投影 Link（这些关系是阶段 3 的活），
/// [`crate::agent::RuleBasedAgent::extract_group`] 也压根不读 `stats`。
/// 全量路径（[`extract`]）传空上下文，`links_dangling` 仍是 AC-007 门禁。
///
/// # REQ-020：`pub(crate)` —— 从 crate 外**编译期**就调不到
///
/// 本函数把调用方给的 `agent` / `agent_version` **原样**写进 `extracted_by`，
/// 而 `items` 永远由**本地规则管线**产出。这两个事实对不上时就是**静默的假溯源**
/// （REQ-019 的 `CAPABILITY_PRODUCES_PROPOSAL` 闸门挡的正是同一件事，
/// 只不过那道闸门只装在 `orchestrate` 上）。
///
/// 所以本函数**不导出**：crate 外拿不到它，也就没有「不经过
/// `orchestrate` 的 `require_proposal_producing` 却产出 `Proposal`」这条路。
/// 实测改前 crate 外无任何调用点（生产两处都在 crate 内：`orchestrate.rs` 的
/// 阶段 3+4、`rule_based.rs` 的 `extract_group`），因此这是零调用点迁移。
///
/// # 顺带说明 [`extract`] 为什么**还**是 `pub`
///
/// 它是本文件的**公开历史入口**（REQ-013 时既有 5 个调用点）。今天它必须
/// 保持 `pub`，是因为 `tests/drop_rate.rs` 与 `tests/test_fk_naming.rs`
/// 两个**由他人维护、本 REQ 不得改动**的集成测试从 crate 外调用它 ——
/// 它们断的是丢弃率与外键命名的回归门禁，不能由本 REQ 顺手改掉。
/// 详见 REQ-020 交付报告的「未闭合项」。
pub(crate) fn extract_with_context(
    report: &SchemaReport,
    context_tables: &[String],
    names: &TableNames,
    project: &str,
    agent: &str,
    agent_version: &str,
) -> Result<ExtractionOutcome> {
    let mut stats = ExtractionStats { tables_seen: report.tables.len(), ..Default::default() };
    let extracted_by = ProposalProvenance {
        agent: agent.to_string(),
        version: agent_version.to_string(),
        round: DEFAULT_PROPOSAL_ROUND,
        failed_groups: Vec::new(),
        source: Some(introspect_provenance(report)),
    };

    let mut entity_types: Vec<EntityItem> = Vec::new();
    let mut link_types: Vec<LinkItem> = Vec::new();
    let mut total_props = 0usize;
    // REQ-014 · F1：**已知表集合 = 作用域 ∪ 上下文**。
    //
    // 作用域部分（`report.tables`）与 REQ-013 逐字节相同；上下文部分只是
    // **多了一批名字**，让 `infer_link` 在组外也能解出关系。
    // ⚠️ 作用域**没有**因此变宽：`for t in &report.tables` 与
    // `tables_seen = report.tables.len()` 都只看作用域，
    // 所以实体 / 属性 / 统计的产出口径一个字没动（守住设计点②）。
    let known_tables: std::collections::HashSet<&str> = report
        .tables
        .iter()
        .map(|t| t.name.as_str())
        .chain(context_tables.iter().map(|s| s.as_str()))
        .collect();
    // REQ-008：对象的中文显示名集合 —— 判「这条注释是不是只在复述这个外键」用。
    let object_names = names.display_name_set();

    for t in &report.tables {
        // ── 实体 ──
        // 优先：语义表 → 表注释（清洗成单行）→ 空（后续会被过滤，但显式可见）。
        // 规则本身在 [`entity_display_name`] —— REQ-022 起它被**两个**调用点共用
        // （本函数与阶段 1 的分组投影），「展示名怎么来」只有一份实现。
        let display_name = match entity_display_name(t, &names) {
            Some(n) => n,
            None => {
                stats.tables_without_cn_name.push(t.name.clone());
                String::new() // 故意留空：让 usable_as_human_name 显式过滤
            }
        };
        if !display_name.is_empty() {
            stats.entities_with_cn_name += 1;
        }

        let mut props: Vec<PropertyItem> = Vec::new();
        // 关系条目的 target 要用中文，先留一份（display_name 下一行会 move 进去）
        let from_cn = display_name.clone();
        entity_types.push(EntityItem {
            kind: "add_entity_type".into(),
            target: from_cn.clone(),
            payload: EntityPayload {
                key: t.name.clone(),
                display_name,
                description: t.comment.clone(),
                aliases: Vec::new(),
                evidence: vec![serde_json::json!({
                    "table": t.name,
                    "claim": "由源库表结构抽取",
                })],
                properties: Vec::new(),
            },
        });

        // ── 属性 ──
        //
        // REQ-022：名字决策 + 逐条来源搬进了 [`build_properties`] ——
        // **本路径与阶段 1 的分组投影共用同一份实现**，见那里的理由。
        let drafts = build_properties(t, &known_tables, names, &object_names);
        for d in &drafts {
            if d.origin == PropertyOrigin::Relation {
                // 关系推导出来的名字**不是**「从注释拿的」，单列一个计数，
                // 免得统计里的 `props_via_comment` 把它们算成注释的功劳。
                stats.props_via_relation += 1;
                stats.props_via_wordlist += 1; // 语义上仍属「非注释」那一栏
            } else if d.item.name.is_empty() {
                stats.props_via_wordlist += 1; // 空名 → project() 走词表兜底
            } else {
                stats.props_via_comment += 1;
            }
            if d.comment_rejected {
                stats.props_comment_rejected += 1;
            }
            props.push(d.item.clone());
        }
        total_props += props.len();
        // 属性**嵌套在实体 payload 内** —— REQ-002 提案契约的形状
        entity_types.last_mut().expect("刚 push 过一个实体").payload.properties = props;

        // ── 关系 ──
        //
        // REQ-022：推断规则搬进 [`links_for_table`]，**本函数与阶段 1 的分组投影
        // 共用同一份** —— 「`*_id` 怎么变成一条关系」只有一个实现，
        // 于是「从候选重建提案」那条新路径不可能偷偷长出第二份判断。
        let links = links_for_table(t, &from_cn, &known_tables, names);
        stats.links_inferred += links.len();
        link_types.extend(links);
    }

    // 自检：关系两端必须都在实体集合里（AC-103 / AC-007）
    let keys: std::collections::HashSet<&str> = entity_types
        .iter()
        .map(|e| e.payload.key.as_str())
        .collect();
    let before = link_types.len();
    link_types.retain(|l| {
        keys.contains(l.payload.from_type.as_str()) && keys.contains(l.payload.to_type.as_str())
    });
    stats.links_dangling = before - link_types.len();
    stats.total_properties = total_props;

    // 扁平化成 REQ-002 契约要求的单 items 数组
    let mut items: Vec<serde_json::Value> = Vec::new();
    for e in &entity_types {
        items.push(serde_json::to_value(e)?);
    }
    for l in &link_types {
        items.push(serde_json::to_value(l)?);
    }

    Ok(ExtractionOutcome {
        proposal: Proposal {
            project: project.to_string(),
            // 与「从候选重建」路径用**同一个常量**，不是各写一个 1
            // （见 [`DEFAULT_PROPOSAL_ROUND`] / [`DEFAULT_PROPOSAL_BASE_VERSION`]
            //   的理由：两处各写一个字面量，改的时候只改一处就分叉了）。
            // 走编排路径时这两个值都会被覆写成真值（`round` 由 `stamp_provenance`、
            // `base_version` 由 `OrchestrationInput::base_version`）。
            base_version: DEFAULT_PROPOSAL_BASE_VERSION,
            round: DEFAULT_PROPOSAL_ROUND,
            extracted_by,
            items,
        },
        stats,
    })
}



/// **一张表的展示名** —— 语义表 → 表注释（清洗成单行）→ `None`。
///
/// # 为什么抽出来、为什么是 `Option`
///
/// REQ-022 之前这段逻辑**内联**在全量路径里，规则实现只有一处调用点。
/// 现在有两处要共用同一份规则：
///
/// 1. [`extract_with_context`] 的全量路径（阶段 3+4）；
/// 2. [`crate::agent::RuleBasedAgent::extract_group`] 的阶段 1 投影 ——
///    它把候选交给编排「从候选重建提案」，而候选里的 `display_name`
///    必须与全量路径**逐字节同源**，否则重建出来的提案就不是原来那一份。
///
/// # `None` 与 `Some("")` 的真实关系（REQ-022 复审修正了这里）
///
/// ⚠ 本文档**此前**声称「`None` 与 `Some("")` **刻意不同** … 合成一个返回值就
/// 抹掉了这个区别」。那句话是**错的**，害得 [`crate::agent::assemble`] 里
/// `display_name.is_empty()` 的写法被当成一处 bug。实际情况是：
///
/// 1. **`Some("")` 按构造就不可达**，两条分支都堵着：
///    注释分支是 `Some(c) if !c.is_empty()`；翻译表分支要成立就得让
///    `TableNames` 里存一个空串，而 [`TableNames::load`] 对空中文名
///    **fail-closed 直接报错**（"等于没翻"）。绕过 `load()` 才有理论可能 ——
///    而那不是生产路径。
/// 2. **两个调用点都**故意**把 `None` 塌成空串**：
///    [`extract_with_context`] 拿到 `None` 时既记进 `tables_without_cn_name`
///    又写 `String::new()`（注释原文：「故意留空：让 usable_as_human_name
///    显式过滤」），[`crate::agent::RuleBasedAgent::extract_group`] 则是
///    `.unwrap_or_default()`。
///
/// 所以 [`crate::agent::assemble`] 用 `is_empty()` **不是把两者合回去**，
/// 而是在 `Some("")` 不可达的前提下的**忠实复刻**规则路径的记法：
/// 记进 `tables_without_cn_name`、不记进 `entities_with_cn_name`。
/// 要「真的区分」就得把 `CandidateType::display_name` 改成 `Option<String>`
/// —— 那是为一个不可达的值改一个共享契约的形状，收益为零而风险不为零
/// （两条路径的形状分叉正是本模块禁止的事）。
///
/// 📌 因此本函数的契约是：**`None` = 没解析出展示名**，
/// 且**保证不返回 `Some("")`**。调用方可以放心按「有无」两态处理。
pub(crate) fn entity_display_name(t: &TableMeta, names: &TableNames) -> Option<String> {
    if let Some(n) = names.lookup(&t.name) {
        return Some(n.to_string());
    }
    match t.comment.as_deref().map(sanitize_comment) {
        Some(c) if !c.is_empty() => Some(c),
        _ => None,
    }
}

/// **一张表的关系条目** —— 全库**唯一**一份「`*_id` → 一条关系」的规则。
///
/// # 为什么必须是共享函数（REQ-022 的硬纪律）
///
/// 阶段 3+4 新增了一条**由 agent 产出决定提案**的路径
/// （`agent::assemble` 的「从候选重建」）。若关系规则留在全量路径的循环里，
/// 阶段 1 的候选就拿不到关系声明 —— 重建路径要么少 51 条关系（提案变了），
/// 要么得**另写一份**推断逻辑（那就多了一份能悄悄漂移的规则）。
/// 所以这里抽出来，**两个调用点共用一份**：全量路径与阶段 1 投影。
///
/// `known_tables` 是**已知表集合 = 作用域 ∪ 上下文**（REQ-014 · F1）：
/// 只传作用域时，指向组外表的关系会被当成不存在。
///
/// 自身引用（`t_dev.t_dev_id`）在这里挡掉；「两端都得在实体集合里」
/// 那道 AC-007 自检**不**在这里 —— 它是**整份提案**的不变量，
/// 由调用方在跨组合并之后施加（见 [`extract_with_context`] 末尾
/// 与 `agent::assemble::rebuild_outcome`）。
pub(crate) fn links_for_table(
    t: &TableMeta,
    from_cn: &str,
    known_tables: &std::collections::HashSet<&str>,
    names: &TableNames,
) -> Vec<LinkItem> {
    let mut out = Vec::new();
    for c in &t.columns {
        let Some((target, card, sem, tier)) = infer_link(c, &t.name, known_tables) else { continue };
        // infer_link 已在 known_tables 内确认过目标存在；这里再挡一次自身引用
        if target == t.name {
            continue;
        }
        let to_cn = names.lookup(&target).unwrap_or(&target).to_string();
        out.push(LinkItem {
            kind: "add_link_type".into(),
            target: format!("{from_cn}→{to_cn}"),
            payload: LinkPayload {
                key: format!("{}_{}__{}", t.name, c.name, target),
                from_type: t.name.clone(),
                to_type: target,
                cardinality: card.into(),
                semantic: sem.into(),
                evidence: vec![serde_json::json!({
                    "table": t.name,
                    "column": c.name,
                    "claim": tier.claim(c.is_primary_key),
                    "inference": match tier {
                        LinkEvidence::Exact => "exact",
                        LinkEvidence::TailMatch => "tail_match",
                    },
                })],
            },
        });
    }
    out
}

/// **属性名从哪儿来**（REQ-022）—— 与 [`ExtractionStats`] 的三个 `props_via_*`
/// 计数一一对应，**互斥**。
/// # 为什么它住在 `extract` 而不是 `agent` 的契约里
///
/// 名字怎么选是**抽取层**的判断（REQ-005 质量门槛 / REQ-007 值域 / REQ-008 关系推导），
/// 阶段 1 契约只是**如实转述**这个事实。定义放在这里、契约侧
/// `pub use` 同一个类型（[`crate::agent::group::PropertyOrigin`]）——
/// 两处同名枚举各写一遍就是一份会漂移的重复定义，本项目吃过这种亏。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyOrigin {
    /// 直接用列注释（且注释过了 REQ-005 的质量门槛）。
    Comment,
    /// 走 `humanize_column()` 词表 / 形态兜底才拿到的名字。
    WordList,
    /// 从关系推导出来的名字（REQ-008）。
    Relation,
}

/// 一条属性的**决策结果** —— 提案条目 + 它的来源 + REQ-005 的拒用痕迹。
///
/// 三个字段是一起的：光有 [`PropertyItem`] 就知道不了「这个名字哪来的」，
/// 而阶段 1 的候选必须能重述它（重建路径靠它还原 `props_via_*` 三个计数）。
pub(crate) struct PropertyDraft {
    pub item: PropertyItem,
    pub origin: PropertyOrigin,
    /// 注释因质量门槛被**拒用**（REQ-005 的主动降级痕迹 —— 必须可见，
    /// 漏记等于「这次没降级」，那正是 fail-open）。
    pub comment_rejected: bool,
}

/// **一张表的全部属性条目 + 逐条来源** —— 全库**唯一**一份属性名决策实现。
///
/// # 为什么必须是共享函数（REQ-022 的硬纪律，与 [`links_for_table`] 同理）
///
/// 阶段 3+4 新增了一条「从候选重建提案」的路径（`agent::assemble`）。
/// 重建路径要能从候选里还原提案，就必须知道每个属性名的来源；
/// 若这份判断留在全量路径的循环里，候选就拿不到它 ⇒ 重建出来的
/// `stats.props_via_*` 三个计数只能全写 0，而 stats.json 是 E2E 判读器读的东西
/// —— **写 0 就是在说谎**（真实库 964 个属性里 579 个来自注释）。
///
/// 反过来另一条路也不通：在 `agent` 侧**重新推断**一遍「这名字是不是关系名」
/// ——那是第二份判断，且会与规则管线悄悄分叉（注释恰好等于关系名的那几条就会数错）。
///
/// 所以这里抽出来，两个调用点共用：
/// 1. [`extract_with_context`]（全量路径）；
/// 2. [`crate::agent::RuleBasedAgent::extract_group`]（阶段 1 分组投影）。
///
/// `object_names` 由调用方**算一次**传进来（重建 91 次没意义，只是浪费）。
pub(crate) fn build_properties(
    t: &TableMeta,
    known_tables: &std::collections::HashSet<&str>,
    names: &TableNames,
    object_names: &std::collections::BTreeSet<String>,
) -> Vec<PropertyDraft> {
    let mut out = Vec::new();
    for c in &t.columns {
        // 主键列不做属性：它是标识不是业务属性，且投影里会与编号重复。
        if c.is_primary_key && c.name == "id" {
            continue;
        }
        let mut comment_rejected = false;
            // 注释优先，但**不是无条件优先**（REQ-005）：先过质量门槛。
            let comment = c.comment.as_deref().map(sanitize_comment).filter(|s| !s.is_empty());

            // ── REQ-008 · 任务 1：外键属性名**从关系推导**，不从注释拿 ──
            //
            // `infer_link` 只在「目标表真实存在」时才给得出关系，
            // 所以**带角色限定的外键（`main_customer_id` / `sub_customer_id` /
            // `source_customer_id` / `from_customer_id` / `to_customer_id` /
            // `related_customer_id`）根本走不到这一支** ——
            // 库里没有 `main_customers` 这张表，关系推不出来，
            // 于是它们的注释（「主账号ID」/「子账号ID」/「划拨来源主账号ID」）
            // 照旧当属性名，**语义差异被完整保留**。
            // 这一点是逐列核对 `schema.json` 37 条 `customer_id` 得出的，
            // 不是假设（见测试 `test_fk_role_qualified_columns_keep_their_comment`）。
            let fk_target = infer_link(c, &t.name, &known_tables)
                .map(|(target, _, _, _)| target)
                .filter(|target| target != &t.name);
            let fk_name = fk_target
                .as_deref()
                .and_then(|target| names.lookup(target))
                .and_then(|target_cn| crate::domain::view::fk_property_name(&c.name, target_cn));

            // 注释质量门槛（REQ-005/007）**先于**关系推导执行 ——
            // 否则一条 `License ID` 会借着「它是外键」绕过门槛当上属性名，
            // 那是 S-2 的破防（本轮第一版就是这么写错的，被既有测试抓住）。
            let quality = comment.as_deref().map(classify_comment);
            // 第三个返回值是「这个名字到底从哪来」——
            // 统计必须按**实际来源**记，不能按「关系名算不算得出来」记，
            // 否则 9 个「注释赢了、关系名只是备选」的外键会被算成关系推导。
            let (name, note, from_relation) = match (&comment, quality, &fk_name) {
                // 注释是人话时才有资格参与「关系 vs 注释」的取舍
                (Some(txt), Some(CommentQuality::HumanReadable), Some(rel))
                    if fk_target
                        .as_deref()
                        .is_some_and(|t| fk_comment_is_restatement(txt, &object_names, t)) =>
                {
                    // 外键 + 注释「只是在复述这个外键」⇒ 用关系推导的名字，
                    // 并把那段复述**丢掉**：留着它，页面上就还是
                    // 「所属客户 · 说明：客户标识」—— 换了个字段名的同一串废话。
                    (rel.clone(), None, true)
                }
                (Some(txt), Some(CommentQuality::HumanReadable), Some(_)) => {
                    // 外键 + 注释带限定（`子账号ID` / `账号ID（余额所属账号）`）⇒
                    // 关系表达不了这个限定，**注释照用**（当属性名，不是当说明）。
                    (txt.clone(), comment.clone(), false)
                }
                (Some(txt), Some(CommentQuality::HumanReadable), None) => {
                    // 非外键：注释优先的正常路径，一个字没改。
                    (txt.clone(), comment.clone(), false)
                }
                // ── 合法枚举值域（REQ-007 行为变更）─────────────────────────
                //
                // `name` **置空**、`note` **保留**。这是一个**位置**问题，
                // 与 REQ-005 的「值域该不该翻」那个**内容**问题是两回事：
                // - REQ-005 的结论仍然成立：值域**不翻**。客户看到「处理中」
                //   回库查到的是 `processing`，翻了反而对不上。
                // - 但值域是「这一列有哪些取值」，它属于**说明**，
                //   不属于「这一列叫什么」。当成属性名时页面上会出现
                //   「属性名 = `0: llm, 1: emb`」—— 判读人看到的是一个
                //   由英文常量拼成的**假属性名**。
                //
                // 置空 name 之后 `project()` 会去问 `humanize_column(column)`，
                // 词表里 `type → 类型` / `status → 状态` 本来就收着，
                // 于是属性名回到人话，值域仍然原样留在说明里。
                //
                // ⚠️ **不要与 `LowQuality`「顺手统一」** —— 两者对 `note`
                //    的处理**故意相反**，理由见下：
                //
                // **REQ-008 增补**：外键也能有值域注释。这时属性名**从关系拿**
                // （比空名等词表更准），值域仍然原样留在说明里。
                (Some(_), Some(CommentQuality::EnumDomain), Some(rel)) => {
                    (rel.clone(), comment.clone(), true)
                }
                (Some(_), Some(CommentQuality::EnumDomain), None) => {
                    (String::new(), comment.clone(), false)
                }
                // 防御性兜底：`quality` 恒由 `comment` 推出，理论不可达；
                // 留着它是让「加新档位」时**编译期**就暴露，而不是静默走错分支。
                (Some(_), None, _) => (String::new(), None, false),
                // 低质量（纯英文且不是值域，也不是人话）：
                //   name 置空 → `project()` 走 `humanize_column()` 词表/形态兜底；
                //   note **同样置空** —— 否则同一段英文只是**换个字段名**
                //   （`note` 而非 `name`）照样原样漏进客户视图。
                //   词表也认不出时属性被 `project()` 丢弃，那是 S-2 既有的
                //   fail-closed 行为：宁可少一个属性，也不给客户看技术串。
                //
                //   **为什么 `EnumDomain` 不能照抄这里的 `note: None`**：
                //   `LowQuality` 的注释是**给系统自己看的**（`license key`、
                //   `SessionId`），它不是业务信息，留在 `note` 里只是换个
                //   字段名继续泄露。而 `EnumDomain` 的注释是**库里真存的值**，
                //   是客户唯一能对上号的东西 —— 抹掉它等于把「这一列有哪些取值」
                //   这个信息删干净。**一个要遮、一个要留，这就是两者不统一的理由。**
                //
                // **REQ-008 增补**：**即使这一列是外键**，`License ID` 这种
                // 低质量注释也**绝不许**借「关系推导」当上属性名 ——
                // 那是 S-2 的破防，判读人会在页面上看到 `License ID`。
                (Some(_), Some(CommentQuality::LowQuality), _) => {
                    comment_rejected = true;
                    (String::new(), None, false)
                }
                // 没有注释：关系推得出就用它（与词表兜底同效，只是更准）。
                (None, _, Some(rel)) => (rel.clone(), None, true),
                (None, _, None) => (String::new(), None, false),
            };
        // 「这个名字到底从哪来」——统计必须按**实际来源**记，
        // 不能按「关系名算不算得出来」记，否则 9 个「注释赢了、关系名只是
        // 备选」的外键会被算成关系推导。
        let origin = if from_relation {
            PropertyOrigin::Relation
        } else if name.is_empty() {
            PropertyOrigin::WordList
        } else {
            PropertyOrigin::Comment
        };
        out.push(PropertyDraft {
            item: PropertyItem { name, column: c.name.clone(), nullable: c.nullable, note },
            origin,
            comment_rejected,
        });
    }
    out
}

/// 列注释 → 属性名。
///
/// 真实库里的注释常是「真实姓名|企业名称」这种多义写法，取第一段做展示名，
/// 完整内容留给 `note`。`|` 是最常见的分隔符（勘查发现大量此类写法）。
fn sanitize_comment(raw: &str) -> String {
    raw.split('|')
        .next()
        .unwrap_or(raw)
        .split(['\n', '\r'])
        .next()
        .unwrap_or(raw)
        .trim()
        .to_string()
}

// ──────────────────────────────────────────────────────────
// REQ-005 · 注释质量门槛
//
// **病根**（不是「词表缺词」）：
// 早期版本这里是无条件 `let name = comment.clone().unwrap_or_default();` ——
// 注释写什么就拿什么当属性名。而 `view::project()` 只在 `name` 为**空串**时
// 才去问 `humanize_column()`。于是**一条英文注释会把词表彻底挡在门外**：
// 词表里有 `license → 授权`、`COLUMN_LABELS` 里有 `license_id → 所属授权`，
// 全都因为「注释非空」而永远没机会出手。
//
// 实测真实源库 91 表 / 964 属性里，**恰好 8 条**注释一个汉字都没有，
// 全部是这一条路径漏出去的（这就是客户视图里那 8 个非中文属性名）。
//
// 修法不是往词表里加词条 —— 词里本来就有，问题是**顺序**。
// 而是给「注释能不能用」设一道**质量门槛**，低质量的让位给词表/形态推断。
// ──────────────────────────────────────────────────────────

/// 一条（已 [`sanitize_comment`] 过的）列注释能不能当客户可见的属性名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentQuality {
    /// 含汉字 —— 直接当展示名（注释优先的正常路径）。
    HumanReadable,
    /// **合法枚举值域** —— 显式豁免，**有意保留英文不翻**。
    ///
    /// **但「不翻」不等于「可以当属性名」**（REQ-007）：值域要留在 `note`，
    /// `name` 仍然置空去走词表。值域是**这一列有哪些取值**，
    /// 属于说明；属性名是**这一列叫什么**。`0: llm, 1: emb` 当属性名，
    /// 页面上就是一条由英文常量拼成的假名字。
    /// 处置见 [`extract`] 里的 `CommentQuality::EnumDomain` 分支。
    EnumDomain,
    /// 低质量：一个汉字都没有，且不是枚举值域。
    /// 不可当展示名 → 回退到词表 / 形态推断。
    LowQuality,
}

/// 判一个属性「是不是裸的值」——**不含任何空白**，只由标识符字符构成。
///
/// 枚举值域里的每个原子都得满足它：`processing` ✓ / `llm` ✓ / `emb` ✓，
/// 而 `license key`（带空格）、`user quota policies …`（带空格）✗。
fn is_value_atom(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '*'))
}

/// 剥掉值域原子的「编码:」前缀 —— `0: llm` → `llm`。
///
/// 只在前缀本身是纯编码时才剥（`0` / `1` / `type` 这类），
/// 免得把 `https://…` 之类也当成值。
fn strip_code_prefix(part: &str) -> &str {
    let Some((pre, rest)) = part.split_once(':') else { return part };
    let pre = pre.trim();
    if !pre.is_empty() && pre.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        rest.trim()
    } else {
        part
    }
}

/// 有没有「编码:」前缀 —— `0: llm` ✓ / `name` ✗。
fn has_code_prefix(part: &str) -> bool {
    strip_code_prefix(part) != part
}

/// 这串原子**是不是一份字段名列表**（`name,email`）而不是**枚举值域**。
///
/// # 病根（REQ-008 · 任务 3b · fail-open）
///
/// [`is_value_atom`] 只看**字符集合**（ASCII 字母数字 + `_ - . *`），
/// 既不看长度、也不看「像不像值」，于是
/// `name,email` / `a/b/c` 都被判成合法值域 ⇒ **有意不翻** ⇒
/// 英文原样进客户视图，守门器 exit 0（错数据静默发出去）。
/// 这是「机制有洞、数据没踩」：实测真实库当前只踩到 2 条**正确**的值域，
/// 但下个客户库完全可能踩到。
///
/// # 判据（形态，不是新词表）
///
/// | 判据 | 依据 |
/// | --- | --- |
/// | **全都**是我们认识的**列名用词**（[`view::is_known_column_word`]） | 字段名列表的每个原子都能当列名用；`Y` / `N` / `processing` 都不是 |
/// | 且**一个都没有**「编码:」前缀 | `0: llm, 1: emb` 是**编了号的值**，不是字段名 |
///
/// 「全有或全无」与 [`resolve_stem`] 同一纪律：**宁可把一个真值域
/// 误当成字段名列表**（后果 = 少一条说明，S-2 仍然丢弃英文，fail-closed），
/// 也不要把字段名列表放过去（后果 = 英文原样漏给客户，fail-open）。
///
/// ⚠️ **已知的剩余漏洞**：`a/b/c` 这类**单字母**串仍会被判成值域
/// （它们不是列名用词）。它要求「单字母 + 三项以上」这种更窄的形态判据，
/// 本轮**没有**做 —— 记在报告里，不假装已修。
fn is_field_name_list(parts: &[&str]) -> bool {
    parts.len() >= 2
        && !parts.iter().any(|p| has_code_prefix(p))
        && parts
            .iter()
            .all(|p| crate::domain::view::is_known_column_word(strip_code_prefix(p)))
}

/// 这条注释是不是**合法的枚举值域** —— 也就是「它列的就是库里真存的值」。
///
/// 判的是**形态**不是词表（与 `view::cardinality_notation_hit`、
/// `view::untranslated_semantic_hit` 同一条纪律）：
/// 能按值域分隔符 `/ , ; |` 切成 **≥2 个裸值原子**，且每个原子都过
/// [`is_value_atom`]（剥掉 `0:` 这类编码前缀之后），
/// **而且不是一份字段名列表**（[`is_field_name_list`]，REQ-008）。
/// [`is_value_atom`]（剥掉 `0:` 这类编码前缀之后）。
///
/// **为什么不翻枚举值域**（有意为之，有测试钉住）：
/// 客户看到「处理中/已扣费/失败」，回库一查是 `processing/charged/failed`
/// —— **对不上**。值域用中文重写一遍，破坏的是信息，不是修好它。
/// 客户要的是「这列有哪些值」，而**值的真身就是那些英文常量**。
///
/// 真实源库上恰好两条命中（REQ-005 类 B）：
/// - `t012.status` = `processing/charged/failed`
/// - `t052.type` = `0: llm, 1: emb`
///
/// **REQ-008 · 任务 3b**：还要排除「**字段名列表**」——
/// `name,email` / `name,phone,email` 这种形态同样是「≥2 个裸原子」，
/// 但它列的是**字段名**、不是**值**，判成值域就等于「有意不翻」→
/// 英文原样进客户视图（fail-open）。判据见 [`is_field_name_list`]。
///
/// 反过来，下面这些**不是**值域（切不成 ≥2 个裸原子）：
/// `license key` / `SessionId` / `License ID` / `Provider ID` /
/// `user quota policies in JSON array format`。
fn is_enum_domain(s: &str) -> bool {
    let parts: Vec<&str> = s
        .split(['/', ',', ';', '|'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    parts.len() >= 2
        && parts.iter().all(|p| is_value_atom(strip_code_prefix(p)))
        && !is_field_name_list(&parts)
}

/// 注释能不能当客户可见的属性名。**抽取器唯一的「注释优先」总闸。**
///
/// 一条外键列的注释是不是**只是在复述这个外键本身**（REQ-008 · 任务 1）。
///
/// # 判据
///
/// 剥掉技术记号（ASCII `id` 词元 + 中文 `标识`/`编号`）之后，剩下的**必须是一个
/// 我们叫得出名字的对象**（[`view::is_known_entity_name`]：全汉字 +
/// 命中对象显示名或业务词表的中文值）。
///
/// | 注释 | 剥完剩 | 判定 | 为什么 |
/// | --- | --- | --- | --- |
/// | `客户标识` / `客户ID` | `客户` | **复述** | 说的就是目标对象，没新信息 |
/// | `用户id` / `用户 ID` | `用户` | **复述** | 「用户」是「客户」在本库里的同指叫法（词表收录） |
/// | `子账号ID` | `子账号` | **限定** | 换主体了：这是子账号，不是「所属客户」 |
/// | `账号ID（余额所属账号）` | 带括号 | **限定** | 括号里是关系表达不了的补充 |
/// | `被代扣Open用户ID` | 含拉丁字母 | **限定** | 带角色（谁被扣费） |
/// | `系统订单序列号id` | `系统订单序列号` | **限定** | 不是对象名，多了「系统/序列号」 |
/// | `license 标识` | `license` | **复述** | 那是 `licenses` 的英文名（否则页面上留一个纯英文属性名） |
/// | `主账号id` 之类 | 含汉字限定 | **限定** | 非 ASCII，不走 3b 那条路 |
///
/// # 判错了会怎样
///
/// - 把**限定**当成复述 → 子账号被写成「所属客户」，**信息丢失**（危险方向）；
/// - 把**复述**当成限定 → 「所属客户 · 说明：客户标识」，不一致还在（正是本轮要治的病）。
///
/// 所以判据刻意**偏保守**：只认「剥完正好是一个对象名」这一种形态，
/// 其余一律当限定保留。**未知形态 = 保留原注释**，绝不会静默改写语义。
fn fk_comment_is_restatement(
    comment: &str,
    object_names: &std::collections::BTreeSet<String>,
    target_table: &str,
) -> bool {
    // 1) 剥 ASCII 的 `id` 词元（独立词，不会误伤 `identity`）
    let mut s = String::with_capacity(comment.len());
    let mut tok = String::new();
    let flush = |out: &mut String, tok: &mut String| {
        if tok.is_empty() {
            return;
        }
        if !tok.eq_ignore_ascii_case("id") {
            out.push_str(tok);
        }
        tok.clear();
    };
    for ch in comment.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            tok.push(ch);
        } else {
            flush(&mut s, &mut tok);
            s.push(ch);
        }
    }
    flush(&mut s, &mut tok);
    // 2) 剥中文技术记号后缀
    for tail in ["标识", "编号"] {
        if let Some(rest) = s.trim_end().strip_suffix(tail) {
            s = rest.to_string();
        }
    }
    // 3) 只认「剥完正好说的是这个目标」这两种形态
    let core = s.trim().trim_matches(|c: char| " \t:：,，、()（）-—_".contains(c));
    if crate::domain::view::is_known_entity_name(core, object_names) {
        return true;
    }
    // 3b) **目标自己的英文名**：`license 标识` 剥掉 `标识` 剩 `license` ——
    //     那就是 `licenses` 的英文名，同样是**复述**。
    //     不认这一路的话，页面上会留着一个**纯英文属性名** `license 标识`
    //     （实测确实存在，见报告），而关系明明已经能给出「所属授权」。
    let low = core.to_ascii_lowercase();
    !low.is_empty()
        && low.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && english_forms(target_table).contains(&low)
}

/// 目标表名的两种英文写法：原样 + 去掉一个尾 `s`（`licenses` / `license`）。
///
/// ⚠️ **为什么不用 [`singular_to_table`]**：那个函数是给「猜目标表」用的，
/// 它带 `-ses → -s` 这条规则（`buses → bus`），于是 `licenses` 会命中 `-ses`
/// 返回**原样**的 `licenses` —— 拿它比 `license` 永远比不相等。
/// 这里要的是「表名单复数两种写法」，判据必须更窄。
///
/// ⚠️ **REQ-108 起实现搬到了 [`crate::domain::confidence::name_forms`]**：
/// 评分卡的 `name_match` 也要判「`customer_id` 是不是 `customers` 的直接展开」，
/// 而那必须与这里的判据**逐字相同**。两处各写一份 = 同一件事两种归一，
/// 迟早会出现「关系是 `customer_id → customers`，评分卡却判它 0.6」这种自相矛盾。
fn english_forms(table: &str) -> [String; 2] {
    crate::domain::confidence::name_forms(table)
}

/// 值域分隔符。`is_enum_domain` 用 ASCII 那组（REQ-005 定的），
/// 这里**必须额外收中文标点** —— 客户写 `0: 未分类，1: 个人用户` 用的是全角逗号。
///
/// ⚠️ **刻意不收 `:` `=`**：那是 `编码:值` 的**配对符**，
/// 收进来就等于把配对拆散，值域会被切成一堆没有配对的碎片（实测全判不出）。
const VALUE_SEP_CJK: &[char] = &['/', ',', ';', '|', '，', '；', '、'];

/// 这条注释是不是**中文值域** —— 整条都是「编码:值」的配对，没有独立的说明文字。
///
/// # 病根（REQ-009 · 阻断项 ②）
///
/// 两条判据各管一半，中文值域**两边都抓不到**：
/// - [`classify_comment`] 的汉字门槛：「有一个汉字 → `HumanReadable` → 照用」
///   ⇒ `0: 未分类, 1: 个人用户` 里全是汉字，直接当上了属性名；
/// - [`is_value_atom`] 只收 ASCII ⇒ 值域判定不认中文原子。
///
/// 结果页面上出现「属性名 = `0: 未分类, 1: 个人用户, 2: 企业用户`」——
/// 判读人**看不出这一列叫「客户类型」**，Q1/Q2 会被答错。
///
/// # 判据（形态规则，不是枚举表）
///
/// **每一个**原子都必须是「**纯编码**`[:：=]`**值**」的配对，且**整条没有一个多余片段**。
///
/// | 判据 | 作用 |
/// | --- | --- |
/// | 切得出 **≥2 个**原子 | 单个 `0: 1` 不是值域清单 |
/// | **每个**原子都含配对符，且配对符**左边不含汉字** | `个人:person` 的左边是汉字 ⇒ 这是**键名**不是编码 |
/// | 配对符**右边非空** | `状态:` 后面没东西，不是配对 |
///
/// **有汉字 ≠ 是人话** —— 汉字可以出现在**值**里（`未分类`），
/// 也可以出现在**独立说明**里（`状态`）。区别在于：值域里每个汉字都被一个
/// 编码「领走」了；人话里有一段汉字**不属于任何配对**。
///
/// # 4 正例 / 4 反例
///
/// | 注释 | 判定 | 为什么 |
/// | --- | --- | --- |
/// | `0: 未分类, 1: 个人用户, 2: 企业用户` | **值域** | 3 个原子全是配对，无独立说明 |
/// | `0: 未认证, 1: 已认证` | **值域** | 同上 |
/// | `0=禁用 1=启用` | **值域** | `=` 也是配对符 |
/// | `0: llm, 1: emb` | **值域** | 与 REQ-007 同一形态（走同一条路） |
/// | `状态 rech:成功 pending:支付中 free:取消` | 人话 | `状态` 是**独立说明**，不属于任何配对 |
/// | `用户类型 个人:person 公司:Corp` | 人话 | `用户类型` 独立；且 `个人` 在配对符左边 |
/// | `商品状态:active-启用,inactive-停用` | 人话 | `商品状态` 独立 |
/// | `是否启用：true=启用，false=禁用` | 人话 | `是否启用` 独立 |
///
/// ⚠️ **不退回按占比的方案**（REQ-005 评估结论，见 [`classify_comment`]）。
/// 本判据是**结构**的（配对是否穷尽），与汉字占比无关。
fn is_han_value_domain(s: &str) -> bool {
    // ⚠️ **不能先按分隔符切原子** —— 空格在值域里**身兼两职**：
    //   `0: 未分类` 里它是「编码和值之间」的，`0=禁用 1=启用` 里它是「两项之间」的。
    //   先切会把 `0: 未分类` 切成 `0:` 和 `未分类` 两个碎片，配对当场就散了
    //   （实测 `0=禁用 1=启用` 因此判不出值域）。
    //   所以改成**逐个配对地扫**：吃掉一个完整的「编码+配对符+值」再找下一个。
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut pairs = 0usize;
    while i < chars.len() {
        // 跳过两项之间的分隔符与空白
        while i < chars.len() && (chars[i].is_whitespace() || VALUE_SEP_CJK.contains(&chars[i])) {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        // 读「编码」：到配对符为止。**编码里不许有空白，也不许有汉字** ——
        // `个人:person` 的左边是汉字，那是键名；`状态 1:已激活` 的左边含空白，
        // 说明前面那段是独立说明。
        let code_start = i;
        while i < chars.len() && !PAIR_SEPS.contains(&chars[i]) && !chars[i].is_whitespace() {
            i += 1;
        }
        let code: String = chars[code_start..i].iter().collect();
        if code.is_empty() || code.chars().any(crate::domain::view::is_han) {
            return false; // 汉字在配对符左边 ⇒ 这是键名/独立说明，不是值域
        }
        if i >= chars.len() || !PAIR_SEPS.contains(&chars[i]) {
            return false; // 这一段没有配对符 ⇒ 不成形
        }
        i += 1; // 吃掉配对符（单字符）
        // ⚠️ **配对符后面可能有空白** —— `0: 未分类` 里编码与值之间就隔着一个空格。
        //   不跳过它，值会读成空串，整条被判成不成形。
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        // 读「值」：到下一个分隔符/空白为止。值里**允许**有汉字 —— 那正是值域。
        let val_start = i;
        while i < chars.len() && !chars[i].is_whitespace() && !VALUE_SEP_CJK.contains(&chars[i]) {
            i += 1;
        }
        if i == val_start {
            return false; // 配对符右边是空的（`状态:`）⇒ 不是配对
        }
        pairs += 1;
    }
    pairs >= 2
}

/// 「编码 `[:：=]` 值」的配对符 —— 全角冒号一并收（客户真的这么写）。
const PAIR_SEPS: &[char] = &[':', '：', '='];

/// 三档，顺序即优先级：
/// 0. **整条都是「编码:值」配对**（可能是汉字值） → [`CommentQuality::EnumDomain`]。
/// 1. **有一个汉字** → [`CommentQuality::HumanReadable`]。**只看有没有，不看占比。**
/// 2. **没有汉字、但是合法枚举值域** → [`CommentQuality::EnumDomain`]（有意不翻）。
/// 3. **没有汉字、也不是值域** → [`CommentQuality::LowQuality`] → 回退词表。
///
/// **为什么第 1 条只看「有没有汉字」而不是「汉字占比」**（REQ-005 评估结论）：
/// 试过按占比设门槛，真实源库上**大面积误杀**合法注释 ——
/// `状态 rech:成功 pending:支付中 free:取消` 汉字占比 0.40、
/// `用户类型 个人:person 公司:Corp` 0.44、
/// `商品状态:active-启用,inactive-停用` 0.36。
/// 这些**是**中文人话，只是习惯把英文常量贴在中英文后面。
/// 门槛定在 0.5 会把它们全判成低质量、赶去走词表（多半认不出 → 丢属性），
/// 那就正好撞上「145 个含英文但不全是英文的属性」这条例外清单 ——
/// **它们不是泄露**。所以判据是**形态（有没有汉字）**，不是**比例**。
pub fn classify_comment(sanitized: &str) -> CommentQuality {
    // ⚠️ **必须排在汉字门槛之前**（REQ-009 · 阻断项 ②）：
    // `0: 未分类, 1: 个人用户` 里的汉字是**值**不是**名字**，
    // 先问「有没有汉字」就永远问不到这里。判据是形态不是占比（见上）。
    if is_han_value_domain(sanitized) {
        return CommentQuality::EnumDomain;
    }
    if sanitized.chars().any(crate::domain::view::is_han) {
        return CommentQuality::HumanReadable;
    }
    if is_enum_domain(sanitized) {
        return CommentQuality::EnumDomain;
    }
    CommentQuality::LowQuality
}

/// 一条推断关系的**来源档次** —— 只影响 `evidence` 里怎么**如实写**，
/// 不影响关系本身（关系照样进提案）。
///
/// 分两档不是为了「分级打分」，是为了让**读提案的人知道这条是怎么来的**：
/// 「`order_id` → `orders`」和「`template_id` → `package_templates`」
/// 都是命名约定，但前者是词干的直接展开、后者是词干的**尾缀**，
/// 证据强度不同。混在一栏里，评审者就只能逐条自己去比对表名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkEvidence {
    /// 词干直接展开命中：`order_id` → `orders`。
    Exact,
    /// 词干是表名的**尾缀**：`template_id` → `package_templates`。
    /// 表名带业务前缀时才会走这一档（`package_*` / `llm_*` / `rpt_*`）。
    TailMatch,
}

impl LinkEvidence {
    fn claim(self, by_pk: bool) -> &'static str {
        crate::extract::cardinality::evidence_claim(
            // 「目标表怎么找到的」（尾缀档）与「基数怎么定的」（主键）
            // 是**两件独立的事**，各有各的措辞 —— 合成一档就得分不清了。
            matches!(self, LinkEvidence::TailMatch),
            by_pk,
        )
    }
}

/// 从列名推断关系。返回 `(目标表, 基数, 语义, 来源档次)`。
///
/// 零外键 —— 这是**命名约定启发式**，不是数据库保证。
/// `known` 是本 schema 的表名集合，用来消歧（`customer_id` 到底指
/// `customers` 还是 `customer_rebate`）。
///
/// # 两档判据（REQ-028）
///
/// 1. **Exact** —— 词干的直接展开命中（`order_id` → `orders`）。**逐字节未改**。
/// 2. **TailMatch** —— 词干是某个已知表名的**尾缀**（`template_id` →
///    `package_templates`）。**仅在第 1 档全部落空时才走。**
///
/// # 为什么第 2 档不违反「宁可少一条，不可错一条」
///
/// 本项目在 [`is_value_atom`] 那条纪律上写的是：**宁可把一个真值域误当成
/// 字段名列表（少一条说明），也不要把字段名列表放过去（英文原样漏给客户）**。
/// 放到关系推断上就是：**宁可少一条关系，不可错一条关系**。所以第 2 档
/// 带四道闸，任一道不过就**什么都不产出**：
///
/// - **闸 A · 只在第 1 档落空时走** ⇒ 既有 51 条关系**逐字节不变**。
///   真实源库实测：放宽后新增 31 条、**移除 0 条**。第 1 档命中时
///   根本不进入第 2 档，所以「动核心启发式」的担心在事实层面不成立。
///   ⚠️ **但闸 A 在当前语料上【不可观测】，别当成它被验证过**：独立枚举
///   91 张表的全部列，「第 1 档与第 2 档**同时有结果且结果不同**」的列 =
///   **0 条**（同时有结果且相同的 11 条，第 1 档落空的 111 条）。
///   换言之「既有 51 条 +0/−0」**不是闸 A 的功劳，而是两档在这 91 张表上
///   本就同解**。闸 A 的价值目前**只由变异测试（M4）**证明成立，
///   真实数据上尚未产生任何可观测影响。
/// - **闸 B · 唯一命中才产出** ⇒ 词干同时是 N（N≥2）张表的尾缀时**不产出**。
///   真实源库实测会命中 **17 个歧义列**（此前注释误写为 8 个，低报）：
///   `product_id`×4、`product_code`×2、`role_id`/`role_code`×2、
///   `channel_code`×5 + `t075.channel_id`、`binding_id`×2、
///   `config_id`×1（命中 6 张表）。每一个**确实**无法从命名上判定，
///   猜错的代价（把「买了哪个商品」说成另一个商品）远大于少一条。
///   17/17 实测产出 0 条关系 ⇒ 闸 B 生效。
/// - **闸 C · 自身表也算一个候选** ⇒ 详见 [`tail_match_target`]。
///   这是**故意**的：去掉它会让 `package_products.product_code`
///   唯一命中 `recharge_products` 而**产出错关系**。
/// - **闸 D · 词干已被 owner 自己占用** ⇒ 详见 [`stem_bound_to_owner_domain`]。
///   闸 B/C/D 挡的都是「同名冲突」，**挡不住「跨域」**：闸 D 挡的是
///   「另一个业务域里恰好有张表名以该词干结尾」。
fn infer_link(
    c: &ColumnMeta,
    owner: &str,
    known: &std::collections::HashSet<&str>,
) -> Option<(String, &'static str, &'static str, LinkEvidence)> {
    // `*_ids`（复数）必须先判，否则会被 `*_id` 后缀吃掉
    for (suffix, card, sem) in REL_SUFFIXES.iter().map(|(suf, cc, mm)| crate::extract::cardinality::card_for(*suf, cc, mm, c.is_primary_key)) {
        // 匹配不上这个后缀 → 试下一个，**不是放弃整列**
        let Some(stem) = c.name.strip_suffix(suffix) else { continue };
        if stem.is_empty() {
            continue;
        }
        // 候选：复数形式、原形、简单单数还原。取第一个真实存在的表。
        for cand in [
            format!("{stem}s"),
            stem.to_string(),
            singular_to_table(stem),
        ] {
            if cand != c.name && known.contains(cand.as_str()) {
                return Some((cand, card, sem, LinkEvidence::Exact));
            }
        }
        // ── 闸 A 之后才轮到词干尾缀匹配 ──
        if let Some(target) = tail_match_target(stem, &c.name, owner, known) {
            return Some((target, card, sem, LinkEvidence::TailMatch));
        }
    }
    None
}

/// 词干尾缀匹配：找**唯一一张**表名以 `stem` / `stem+s` 结尾的已知表。
///
/// # 为什么探针要带 `stem+s`
///
/// `package_templates` 以 `template`（词干）结尾吗？**不以** ——
/// 它以 `templates` 结尾。只探 `stem` 的话，核心那条
/// `template_id → package_templates` 根本命不中（实测 `stem`-only
/// 只有 66 条关系，核心用例仍是 `None`）。所以两探针都要。
///
/// # 为什么 `owner`（自身表）**也**计入候选（闸 C）—— 本函数最反直觉的一处
///
/// 直觉上「表不该指向自己」，把 `owner` 加进排除条件更「干净」。但真实源库上
/// 这样做会**产出错关系**：
///
/// | owner（表） | 列 | 排除 owner 之后 | 结果 |
/// | --- | --- | --- | --- |
/// | `package_products` | `product_code` | 唯一剩 `recharge_products` | ❌ **错**：套餐商品表指向充值商品表 |
///
/// 保留 owner 在候选里，该列就有 2 个候选 ⇒ 闸 B 拦下 ⇒ **不产出**，
/// 正是要的行为。所以 `owner` 被**显式传进来**却**刻意不用** ——
/// 目的是让这条纪律可以被一条变异（把 owner 加回排除条件）直接证伪，
/// 而不是藏在「压根没写这个过滤条件」里看不见。
///
/// 代价是 `admin_user_roles.role_id`（本可以判成 `admin_roles`）也一并
/// 被拦下 —— **主动放弃一条可能正确的**，因为「可能正确」不是「正确」。
/// 这与本模块「少一条胜过错一条」是同一条纪律。
///
/// # 确定性
///
/// `known` 是 `HashSet`，迭代顺序不定。这里 `hits.len() == 1` 时才取值，
/// 那个元素与顺序无关 ⇒ **结果确定**。仍然排序是为了歧义时可复现地报告。
fn tail_match_target(
    stem: &str,
    column: &str,
    owner: &str,
    known: &std::collections::HashSet<&str>,
) -> Option<String> {
    let plural = format!("{stem}s");
    let mut hits: Vec<&str> = known
        .iter()
        .copied()
        // ⚠️ 闸 C：`owner`（正在推断的这张表）**刻意不排除** ——
        // 排除它会让 `package_products.product_code` 唯一命中
        // `recharge_products`，产出一条**错**关系。理由见函数文档。
        .filter(|t| *t != column && (t.ends_with(stem) || t.ends_with(&plural)))
        .collect();
    hits.sort_unstable();
    // 闸 B：N ≥ 2 = 歧义 ⇒ 什么都不产出。
    if hits.len() != 1 {
        return None;
    }
    let target = hits[0];
    // ── 闸 D · 词干已被 owner 自己占用 ⇒ 跨域同名碰撞，不产出 ──
    if stem_bound_to_owner_domain(stem, owner, target) {
        return None;
    }
    Some(target.to_string())
}

/// 闸 D 的判据：**词干是 owner 名字里的一个「作用域内层词」，
/// 而候选表与 owner 毫无共同词汇** ⇒ 跨域同名碰撞，不产出。
///
/// # 它挡的是什么（真实源库 91 表实测）
///
/// 闸 B 只问「词干是不是某张表的**唯一**尾缀」。这个前提不成立：
/// 一个**跨业务域通用的普通名词**（`task` = 任务）会在好几个域里各自出现，
/// 但只有**一个**域碰巧把它放在表名的末尾 —— 于是「唯一」成立，闸 B 放行，
/// 关系却是错的：
///
/// | owner（表） | 列 | 唯一尾缀命中 | 实际所属域 |
/// | --- | --- | --- | --- |
/// | `instance_task_record` | `task_id` | `file_parser_task` | 实例/云手机域（`ins_id`/`site_url`）|
/// | `intance_task_plans` | `task_id` | `file_parser_task` | 同上（表名首词还是 `intance` 拼写错误）|
/// | `file_parser_task` | — | — | 文档解析域（`doc_name`/`task_result`）|
///
/// 后果不只是少一条关系：属性名是**客户读的东西**，
/// `instance_task_record.task_id` 会从 `任务标识` 变成 `所属文件解析任务`。
///
/// # 判据（三条，缺一不可）
///
/// 1. 词干出现在 owner 表名的**词元**里（`instance_task_record` 含 `task`）；
/// 2. 它**不是** owner 的**首词元** —— 首词元是这张表的**作用域前缀**
///    （`app_billing_transactions` = app 域的计费流水），`*_id` 指的就是
///    作用域本身，作用域的定义表可以住在别处；而 `instance_` 才是
///    `instance_task_record` 的作用域，`task` 已被这个作用域限定过了；
/// 3. owner 与候选表**除该词干外没有任何共同词元**
///    ⇒ 候选来自一个词汇完全不搭界的域。
///
/// 2+3 同时成立 ⇒ 命中的是「不同域里恰好同名的普通名词」，不是引用。
///
/// # 为什么不用「排掉某几张表」来修
///
/// 那只把这次的错盖住，缺口原样留着。所以这里判的是**词元构成**，
/// 不含任何具体表名；换一批数据、同样的形状，照样挡得住。
///
/// # 真实 91 表实测：2/2 命中，0 误杀
///
/// 挡下 `instance_task_record.task_id` 与 `intance_task_plans.task_id`
/// 两条错关系；QA 独立复核判「对」的 26 条**一条未伤**。
///
/// ⚠️ **如实记录本判据的证据强度 —— 请勿当成已被独立验证**：
///
/// - 条件 2（首词元豁免）是在观察到「不豁免就会误杀
///   `app_billing_transactions.app_id → service_apps` 与
///   `subscription_priority.subscription_id → customer_subscriptions`」
///   **之后**才补上的，其独立性没有被本语料独立验证；
/// - 91 表上闸 D **只命中那 2 条**，未命中任何未经复核的行，
///   因此「换一批数据仍成立」属于**未验证**；
/// - 去掉条件 2 的更简版本（只判 1+3）判别力完全相同，
///   但会误杀上述 2 条 QA 判「对」的关系 —— 且其中
///   `subscription_priority.subscription_id` 会因失去唯一命名来源而
///   **属性名变空**（`所属订阅` → `''`）。
///
/// 两个版本都写在这里，供评审者复核这个取舍。
fn stem_bound_to_owner_domain(stem: &str, owner: &str, target: &str) -> bool {
    fn tokens(s: &str) -> Vec<&str> {
        s.split('_').filter(|p| !p.is_empty()).collect()
    }
    let o = tokens(owner);
    let Some(pos) = o.iter().position(|p| *p == stem) else {
        // 词干不在 owner 名字里 ⇒ 没有「已被占用」这回事，放行。
        return false;
    };
    // 条件 2：首词元 = 作用域前缀，豁免。
    if pos == 0 {
        return false;
    }
    // target 以 stem 结尾 ⇒ stem 必是它的某个词元，故交集恒非空，
    // 「除 stem 外无交集」== 交集里每一项都等于 stem。
    let ot: std::collections::HashSet<&str> = o.into_iter().collect();
    let tt: std::collections::HashSet<&str> = tokens(target).into_iter().collect();
    ot.intersection(&tt).all(|p| *p == stem)
}

/// `activity` → `activity`（不变）；这里只处理简单的复数还原。
fn singular_to_table(stem: &str) -> String {
    for (suf, rep) in [("ies", "y"), ("ses", "s"), ("s", "")] {
        if let Some(base) = stem.strip_suffix(suf) {
            return format!("{base}{rep}");
        }
    }
    stem.to_string()
}

// ──────────────────────────────────────────────────────────
// REQ-108 · 置信度接线（AC-004 / AC-008）
//
// 分工是刻意的，别合并：
//   · **采集**（`sample` 模块，异步、碰客户库）只产出聚合量；
//   · **评分**（`domain::confidence`，纯函数）只吃事实、不算 I/O；
//   · **本段**是两者的接缝：把 `items` 里的关系条目解成坐标、组装
//     `ScoreInput`，再把分数贴回条目。
//
// 为什么接缝放在这里而不是 bin 里：`ontok-propose` 与「从候选重建」两条
// 路径都要产生同样的 confidence —— 写在 bin 里就只覆盖第一条。
// ──────────────────────────────────────────────────────────

/// 提案里一条关系条目在**源库里的坐标**。
///
/// `from_column` 取自条目的 `payload.evidence[0].column`（REQ-003 起就在那儿，
/// 抽取器每写一条关系都带上），**不另抽一份**：再算一次「哪一列推出的这条关系」
/// 就是第二份判断，迟早与 `links_for_table` 分叉。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCandidate {
    /// 它在 `Proposal::items` 里的下标（回填 confidence 时用）
    pub item_index: usize,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    /// 源表排序键（采样 `ORDER BY`）；`None` ⇒ 采样器按值排序
    pub from_key: Option<String>,
    /// 目标表的键列（`KeyCatalog` 的目录事实）；`None` ⇒ 不能采样（记 no_key_column）
    pub to_key_column: Option<String>,
}

impl LinkCandidate {
    /// 采样坐标。目标表没有单列键 ⇒ `None`（`v1` 不支持复合键引用，R2）。
    pub fn sample_spec(&self) -> Option<crate::extract::sample::SampleSpec> {
        Some(crate::extract::sample::SampleSpec {
            from_table: self.from_table.clone(),
            from_column: self.from_column.clone(),
            from_key: self.from_key.clone(),
            to_table: self.to_table.clone(),
            to_key_column: self.to_key_column.clone()?,
        })
    }
}

/// 从提案 `items` 里解出全部关系条目的坐标（**按 items 顺序**）。
///
/// 不是关系条目、或 payload 里取不到 `from_type` / 列名的条目直接跳过 ——
/// 那些条目本来就不参与评分（实体条目、概念组条目）。
pub fn link_candidates(
    items: &[serde_json::Value],
    keys: &crate::extract::introspect::KeyCatalog,
) -> Vec<LinkCandidate> {
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if it.get("kind").and_then(|k| k.as_str()) != Some("add_link_type") {
            continue;
        }
        let p = &it["payload"];
        let (Some(from), Some(to)) = (
            p.get("from_type").and_then(|v| v.as_str()),
            p.get("to_type").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let column = p
            .get("evidence")
            .and_then(|e| e.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("column"))
            .and_then(|c| c.as_str())
            .or_else(|| p.get("column").and_then(|c| c.as_str()));
        let Some(column) = column else { continue };
        out.push(LinkCandidate {
            item_index: i,
            from_table: from.to_string(),
            from_column: column.to_string(),
            to_table: to.to_string(),
            from_key: keys.get(from).and_then(|k| k.key_column()).map(str::to_string),
            to_key_column: keys.get(to).and_then(|k| k.key_column()).map(str::to_string),
        });
    }
    out
}

/// 按「源表.列 → 目标表.键列」逐条评分，返回 `(item_index, confidence)`（**按 items 顺序**）。
///
/// `evidence` 的键是 `item_index`：**没有条目** ⇒ 该关系没采（`--no-sample` 或
/// 没有键列），评分卡会标 `data_evidence: none`。**这不会被当成有证据**。
pub fn score_links(
    items: &[serde_json::Value],
    report: &SchemaReport,
    keys: &crate::extract::introspect::KeyCatalog,
    evidence: &std::collections::HashMap<usize, crate::extract::sample::SampleEvidence>,
) -> Vec<(usize, crate::domain::confidence::Confidence)> {
    use crate::domain::confidence as cf;

    let candidates = link_candidates(items, keys);
    let mut out = Vec::with_capacity(candidates.len());
    for cand in candidates {
        let from_col = find_column(report, &cand.from_table, &cand.from_column);
        let from_columns: Vec<&str> = report
            .tables
            .iter()
            .find(|t| t.name == cand.from_table)
            .map(|t| t.columns.iter().map(|c| c.name.as_str()).collect())
            .unwrap_or_default();
        // 目标键列：**目录说哪一列就是哪一列**（AC-001 的意义在这里兑现 ——
        // `service_apps` 的主键是 `app_id`，猜 `id` 会让命中率恒为 0）
        let to_col = cand
            .to_key_column
            .as_deref()
            .and_then(|k| find_column(report, &cand.to_table, k));
        let target_key = match keys.get(&cand.to_table) {
            None => cf::TargetKey::Unknown,
            Some(k) => match cand.to_key_column.as_deref() {
                Some(col) if k.primary_key.as_deref() == Some(col) => cf::TargetKey::Primary,
                Some(col) if k.unique_columns.iter().any(|u| u == col) => cf::TargetKey::Unique,
                _ => cf::TargetKey::Plain,
            },
        };
        let data = evidence.get(&cand.item_index).map(|e| cf::SampleFacts {
            sampled: e.sampled,
            matched: e.matched,
            digest: e.digest.clone(),
            reason: e.reason.map(str::to_string),
        });
        let input = cf::ScoreInput {
            column: cand.from_column.clone(),
            to_table: cand.to_table.clone(),
            from_type: from_col
                .map(|c| cf::type_family(&c.data_type))
                .unwrap_or(cf::TypeFamily::Other),
            to_type: to_col.map(|c| cf::type_family(&c.data_type)),
            target_key,
            data,
            composite_target_key: keys
                .get(&cand.to_table)
                .map(|k| k.has_composite_primary_key())
                .unwrap_or(false),
            polymorphic: cf::is_polymorphic_column(&from_columns, &cand.from_column),
            self_ref: cand.from_table == cand.to_table,
        };
        out.push((cand.item_index, cf::score(&input)));
    }
    out
}

fn find_column<'a>(
    report: &'a SchemaReport,
    table: &str,
    column: &str,
) -> Option<&'a ColumnMeta> {
    report
        .tables
        .iter()
        .find(|t| t.name == table)?
        .columns
        .iter()
        .find(|c| c.name == column)
}

/// 把分数贴回条目：`item["confidence"] = { score, method, band, signals, veto, data_evidence }`。
///
/// ⚠️ 除了新增这一个键，**条目其余字段一个字节都不动** —— 这是
/// 「置信度不改变抽取结果，只改变呈现与审核顺序」（设计概要 §一）在代码里的落点。
pub fn attach_confidence(
    items: &mut [serde_json::Value],
    scored: &[(usize, crate::domain::confidence::Confidence)],
) {
    for (idx, c) in scored {
        if let Some(item) = items.get_mut(*idx) {
            if let Some(obj) = item.as_object_mut() {
                if let Ok(v) = serde_json::to_value(c) {
                    obj.insert("confidence".to_string(), v);
                }
            }
        }
    }
}

/// 分档路由的结果（AC-009）。
///
/// 三份 items **互不相交**、合起来是原来的全部条目 —— 这是本结构的核心不变量，
/// 由 [`route_items`] 的单元测试逐条钉住。少了一条 = 静默丢条目，
/// 多了一条 = 同一条关系既在提案里又在拒绝清单里。
#[derive(Debug, Clone, Default)]
pub struct Routing {
    /// 高 + 中：进主提案（`proposal.json` 的 items）
    pub kept: Vec<serde_json::Value>,
    /// 低 + 「有链接但没评分」：进「待查清单」，**不进主提案**
    pub pending: Vec<serde_json::Value>,
    /// 拒绝：**不提案**，只记理由
    pub rejected: Vec<serde_json::Value>,
    /// 四档条数（只统计关系条目 —— 实体条目不参与分档）
    pub counts: crate::domain::confidence::BandCounts,
    /// 没被评过分的关系条目数（理论上 0；非 0 说明条目缺列名，进待查）
    pub unscored_links: usize,
}

/// **按档路由**（AC-009）：高+中进提案 · 低进待查 · 拒绝不提案。
///
/// | 档 | 分数 | 去向 |
/// | --- | --- | --- |
/// | 高 | ≥0.90 | 进提案（审核界面默认折叠） |
/// | 中 | 0.60–0.90 | 进提案（逐条显示） |
/// | 低 | 0.30–0.60 | 待查清单（**不进主提案**） |
/// | 拒绝 | <0.30 | 不提案，只记理由 |
///
/// # 实体条目与其它条目**原样保留**
///
/// 分档只决定**关系**的呈现顺序，不碰实体、属性、概念组条目 ——
/// 「置信度不改变抽取结果，只改变呈现与审核顺序」（设计概要 §一）。
/// 所以 `kept` 里除关系外的条目顺序与输入完全一致。
///
/// # 没有分数的关系条目进「待查」
///
/// 那是「不知道」，不是「高」也不是「拒绝」。把它放进提案等于
/// 让一条没验过的关系冒充验过的（R1 ④ 的反面）；放进拒绝清单又是在
/// 断言它错。待查清单是唯一诚实的去向，并且计数**单独报出来**。
pub fn route_items(
    items: Vec<serde_json::Value>,
    scored: &[(usize, crate::domain::confidence::Confidence)],
) -> Routing {
    use crate::domain::confidence::Band;

    let mut by_index: std::collections::HashMap<usize, &crate::domain::confidence::Confidence> =
        std::collections::HashMap::with_capacity(scored.len());
    for (idx, c) in scored {
        by_index.insert(*idx, c);
    }
    let mut out = Routing::default();
    for (i, item) in items.into_iter().enumerate() {
        let is_link = item.get("kind").and_then(|k| k.as_str()) == Some("add_link_type");
        if !is_link {
            out.kept.push(item);
            continue;
        }
        match by_index.get(&i) {
            Some(c) => {
                out.counts.add(c.band);
                match c.band {
                    Band::High | Band::Middle => out.kept.push(item),
                    Band::Low => out.pending.push(item),
                    Band::Rejected => out.rejected.push(item),
                }
            }
            None => {
                out.unscored_links += 1;
                out.pending.push(item);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 判据的唯一来源：真实源库 91 表 / 964 属性里**恰好**这 8 条注释没有汉字。
    /// 全量断言 —— 少一条或多一条，真实源库一改就会红。
    #[test]
    fn test_real_schema_has_exactly_eight_hanless_comments() {
        // 类 A：必须被降级（纯英文，不是值域）
        for c in [
            "license key",
            "License ID",
            "SessionId",
            "user quota policies in JSON array format",
            "Provider ID",
        ] {
            assert_eq!(
                classify_comment(c),
                CommentQuality::LowQuality,
                "「{c}」是纯英文注释，必须判为低质量并回退词表"
            );
        }
        // 类 B：合法枚举值域，**有意不翻**
        for c in ["processing/charged/failed", "0: llm, 1: emb"] {
            assert_eq!(
                classify_comment(c),
                CommentQuality::EnumDomain,
                "「{c}」是合法枚举值域，必须豁免（翻了反而对不上库里的实际值）"
            );
        }
    }

    /// **类 B 的钉子**：这两个值域**必须原样保留英文**。
    ///
    /// 动机（REQ-005 教训）：这个词表本轮加过一批词，下一轮有人
    /// 「顺手把 `status`/`type` 也翻了」就会毁掉信息 ——
    /// 客户看到「处理中」，回库查到的却是 `processing`，**对不上**。
    /// 本测试就是拦住那次顺手改动的。
    #[test]
    fn test_enum_domains_are_intentionally_not_translated() {
        assert!(is_enum_domain("processing/charged/failed"));
        assert!(is_enum_domain("0: llm, 1: emb"));
        // 值域的判据是形态：再加几种拼法仍然要认出来
        for c in ["active/inactive", "0: llm, 1: emb, 2: rerank", "a;b;c", "ok,failed,timeout"] {
            assert!(is_enum_domain(c), "「{c}」是值域形态，应当豁免");
        }
        // 绝不能把英文散文误判成值域
        for c in [
            "license key",
            "SessionId",
            "License ID",
            "Provider ID",
            "user quota policies in JSON array format",
        ] {
            assert!(!is_enum_domain(c), "「{c}」是散文/标识符，不是值域");
        }
    }

    /// **误杀防线**：中英混排的注释**全部**要判为人话。
    ///
    /// 这些是 REQ-005 明确划在范围外的「145 个含英文但不全是英文」——
    /// 中文里本来就用这些词（`ID`/`Token`/`API`），**不是泄露**。
    /// 门槛一旦写成「汉字占比」，这一批会大面积被误杀（实测占比 0.30~0.44）。
    #[test]
    fn test_mixed_chinese_comments_are_never_rejected() {
        for c in [
            "状态 rech:成功 pending:支付中 free:取消",       // 汉字占比 0.40
            "用户类型 个人:person 公司:Corp",                // 0.44
            "商品状态:active-启用,inactive-停用",             // 0.36
            "状态：pending-待认证, completed-已完成",          // 0.36
            "状态：active-有效, inactive-无效",
            "license 标识",                                  // 扣费流水.license_id
            "模拟登录会话ID",                                 // 操作日志.session_id
            "直接指定Provider(优先级最高)",                    // 模型路由规则.provider_id
            "RSA 公钥 PEM",
            "请求Token数",
        ] {
            assert_eq!(
                classify_comment(c),
                CommentQuality::HumanReadable,
                "「{c}」含汉字，是合法中文注释，绝不可被门槛误杀"
            );
        }
    }

    /// **REQ-009 · 阻断项 ② 的钉子**：中文值域**不许**占属性名。
    ///
    /// 这 6 条是真实客户源库里**实测**踩到的形态（不是构造的）：
    /// `t025.type` / `t025.verify` / `t025.status` /
    /// `t025.is_foreign_model_allowed` / `t006.status` / `t052.type`。
    /// 判成 `EnumDomain` 后走 REQ-007 那条路：`name` 置空 → `project()`
    /// 去问 `humanize_column()` → `type → 类型`；值域原样留在**说明**里。
    #[test]
    fn test_chinese_value_domain_never_becomes_a_property_name() {
        for c in [
            "0: 未分类, 1: 个人用户, 2: 企业用户", // t025.type
            "0: 未认证, 1: 已认证",                 // t025.verify
            "0: 已停用, 1: 已启用",                 // t025.status / t006.status
            "0: 不允许, 1: 允许",                   // t025.is_foreign_model_allowed
            "0=禁用 1=启用",                         // 服务应用.status
            "0: llm, 1: emb",                       // t052.type（与 REQ-007 同一形态）
        ] {
            assert_eq!(
                classify_comment(c),
                CommentQuality::EnumDomain,
                "「{c}」整条都是「编码:值」配对，是值域不是属性名"
            );
        }
    }

    /// **误杀防线（②这一侧）**：有独立说明文字的**必须**仍然是人话。
    ///
    /// 这是本轮最危险的失败方向：判据一旦写成「含配对符就是值域」，
    /// 上面 [`test_mixed_chinese_comments_are_never_rejected`] 里的合法中文注释
    /// 会**整批**被赶去走词表（多半认不出 → 丢属性）。
    /// 区别只在**有没有一段不属于任何配对的独立说明**。
    #[test]
    fn test_chinese_comment_with_independent_explanation_stays_human() {
        for c in [
            "状态 rech:成功 pending:支付中 free:取消", // 独立说明「状态」
            "用户类型 个人:person 公司:Corp",          // 独立说明「用户类型」
            "商品状态:active-启用,inactive-停用",       // 独立说明「商品状态」
            "是否启用：true=启用，false=禁用",          // 独立说明「是否启用」
            "状态 1:已激活, 0:待激活",                 // 独立说明「状态」
            "来源类型(0: 认证 1: 分享)",               // 独立说明「来源类型」
        ] {
            assert_eq!(
                classify_comment(c),
                CommentQuality::HumanReadable,
                "「{c}」有独立说明文字，是人话，不该被判成值域"
            );
        }
    }

    /// 配对符左边是**汉字**时，那是键名/说明，不是编码 —— 仍然是人话。
    ///
    /// 另含一条**字节边界**回归：配对符前面是多字节汉字时，
    /// 早期实现用字节下标切字符串会 panic（`个人:person` 的 `:` 落在汉字中间）。
    #[test]
    fn test_han_on_the_code_side_is_a_key_not_a_code() {
        assert_eq!(classify_comment("个人:person 公司:Corp"), CommentQuality::HumanReadable);
        // 汉字紧贴配对符：切分点必须落在字符边界上，不能 panic
        assert_eq!(classify_comment("用:户 类:型"), CommentQuality::HumanReadable);
    }

    /// 门槛**只**看「有没有汉字」，不看占比 —— 这里把占比的反例钉死。
    #[test]
    fn test_gate_is_presence_of_han_not_ratio() {
        // 汉字只有 1 个，占比极低 —— 仍然是合法人话
        assert_eq!(classify_comment("ID号"), CommentQuality::HumanReadable);
        assert_eq!(classify_comment(" API 端点"), CommentQuality::HumanReadable);
    }

    /// ★ [`entity_display_name`] **保证不返回 `Some("")`**（REQ-022 复审）。
    ///
    /// 这条契约是 [`crate::agent::assemble`] 里 `display_name.is_empty()`
    /// 之所以**不是**「把 `None` 与 `Some("")` 合回去」的全部依据，
    /// 所以它必须被钉住 —— 否则那处代码随时可能被后人「修」成 bug 而无人知晓。
    ///
    /// ⚠ 变异自证：把 `Some(c) if !c.is_empty()` 的守卫删掉（让空串以
    /// `Some("")` 走出），本测试立刻红。
    #[test]
    fn test_entity_display_name_never_returns_some_empty_string() {
        let t = |comment: Option<&str>| TableMeta {
            name: "t_dev".to_string(),
            comment: comment.map(|c| c.to_string()),
            columns: vec![],
            column_count: 0,
            commented_count: 0,
        };
        // 翻译表命中 ⇒ 给出中文名
        let names = TableNames {
            agent: "t".into(),
            version: "1".into(),
            generated_at: "2026-10-08T00:00:00Z".into(),
            source: "unit".into(),
            names: [("t_dev".to_string(), "设备".to_string())].into_iter().collect(),
        };
        assert_eq!(entity_display_name(&t(Some("设备表")), &names), Some("设备".into()));

        // 没翻译、有真注释 ⇒ 清洗后给出
        let empty = TableNames {
            agent: "t".into(),
            version: "1".into(),
            generated_at: "2026-10-08T00:00:00Z".into(),
            source: "unit".into(),
            names: Default::default(),
        };
        assert_eq!(
            entity_display_name(&t(Some("设备表 | 别名")), &empty),
            Some("设备表".into())
        );

        // ★ 清洗出**空串** ⇒ 必须是 `None`，**不是** `Some("")`。
        //   `Some("")` 会让下游 `is_empty()` 分支与 `None` 分支**记出不同的数**
        //   （一个不记进 `tables_without_cn_name`、一个记），而规则路径与重建
        //   路径对这张表的记法会就此分叉 —— 那正是「静默的口径漂移」。
        for c in ["   ", "", "|", "\n", "  |  ", "\r\n"] {
            assert_eq!(
                entity_display_name(&t(Some(c)), &empty),
                None,
                "清洗后为空的注释（{c:?}）必须判成「没解析出展示名」（None），\
                 不能返回 Some(\"\") —— 那会让两个调用点的统计口径悄悄分叉"
            );
        }
        // 没有注释 ⇒ `None`
        assert_eq!(entity_display_name(&t(None), &empty), None);
    }

    // ──────────────────────────────────────────────────────────
    // REQ-028 · infer_link 两档判据 + 四道闸
    // ──────────────────────────────────────────────────────────

    fn col(name: &str) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: "bigint".into(),
            nullable: false,
            comment: None,
            is_primary_key: false,
        }
    }

    fn known<'a>(v: &'a [&'a str]) -> std::collections::HashSet<&'a str> {
        v.iter().copied().collect()
    }

    /// REQ-028 的**核心用例**：源库里这条外键真实存在，
    /// 而词干 `template` 不是任何表名的**直接展开**（表叫 `package_templates`）。
    /// 修之前这里推不出关系 ⇒ §9.5「客户买了什么套餐」结构上答不出来。
    #[test]
    fn test_core_case_template_id_reaches_package_templates() {
        let k = known(&["customer_subscriptions", "order", "package_templates", "package_benefits"]);
        for table in ["customer_subscriptions", "order", "package_benefits"] {
            let got = infer_link(&col("template_id"), "customer_subscriptions", &k).expect("三张表的 template_id 都指向套餐模板");
            assert_eq!(got.0, "package_templates", "{table}.template_id 必须指向 package_templates");
            assert_eq!(got.3, LinkEvidence::TailMatch);
            assert_eq!(got.1, "N:1");
            assert_eq!(got.2, "belongs_to");
        }
    }

    /// 闸 A：词干直接展开**命中时，第 2 档绝不能插手**。
    ///
    /// 判据不是「输出对」，而是**档次标记**必须是 `Exact` ——
    /// 若实现改成「先跑尾缀匹配」，这里仍可能碰巧解出同一个表名，
    /// 但既有 51 条关系的证据档次就会被悄悄改写。
    #[test]
    fn test_exact_match_is_unaffected_and_keeps_its_tier() {
        let k = known(&["orders", "order", "customers", "customer_rebate"]);
        // `order_id` → `orders`（既有行为，逐字节不变）
        let got = infer_link(&col("order_id"), "orders", &k).unwrap();
        assert_eq!((got.0.as_str(), got.3), ("orders", LinkEvidence::Exact));
        // `customer_id` → `customers`（消歧后取直接展开的那个）
        let got = infer_link(&col("customer_id"), "licenses", &k).unwrap();
        assert_eq!((got.0.as_str(), got.3), ("customers", LinkEvidence::Exact));
    }

    /// 闸 B：词干同时是 ≥2 张表的尾缀 ⇒ **歧义，不产出**。
    ///
    /// 这几条全部来自真实源库（91 表实测），且每一个都**确实**无法
    /// 从命名上判定 —— 猜错的后果（把「买了哪个商品」说成另一个商品）
    /// 远大于少一条关系。
    #[test]
    fn test_ambiguous_stem_emits_nothing() {
        let k = known(&[
            "package_products",
            "recharge_products",
            "admin_roles",
            "admin_user_roles",
            "service_config",
            "website_notice_config",
            "keyword_filter_config",
            "usage_quotas",
        ]);
        for c in ["product_id", "product_code", "role_id", "role_code", "config_id"] {
            assert_eq!(
                infer_link(&col(c), "some_owner", &k),
                None,
                "「{c}」的词干命中多张表 ⇒ 必须什么都不产出（宁可少一条，不可错一条）"
            );
        }
        // 歧义列所在的表自身也不该被解出关系
        assert_eq!(infer_link(&col("product_id"), "package_products", &known(&["package_products", "recharge_products"])), None);
    }

    /// 闸 C：**自身表（owner）也算一个候选**，且这是刻意设计的。
    ///
    /// 去掉它，`package_products.product_code` 会唯一命中
    /// `recharge_products` —— 一个**错**关系（套餐商品表 ≠ 充值商品表）。
    /// 这条测试就是为了钉住「别哪天顺手把它优化掉」：
    /// 变异 = 往 `tail_match_target` 的过滤条件里加回 `*t != owner`。
    #[test]
    fn test_self_table_counts_as_a_candidate() {
        // 有自身表在列 ⇒ 2 个候选 ⇒ 不产出
        assert_eq!(
            infer_link(&col("product_code"), "package_products", &known(&["package_products", "recharge_products"])),
            None,
            "自身表必须计入候选，否则会唯一命中 recharge_products 并产出错关系"
        );
        // 自身表不在已知集合里（分组路径下的常态）⇒ 唯一命中，可产出
        let got = infer_link(&col("app_id"), "billing", &known(&["service_apps", "billing"])).unwrap();
        assert_eq!((got.0.as_str(), got.3), ("service_apps", LinkEvidence::TailMatch));
    }

    /// 探针必须同时含 `stem` 与 `stem+s`：`package_templates` 以
    /// **`templates`** 结尾，只探 `template` 的话核心用例命不中。
    #[test]
    fn test_plural_probe_is_required() {
        let k = known(&["package_templates"]);
        assert!(
            !known(&["package_templates"]).iter().any(|t| t.ends_with("template")),
            "本测试的前提：`package_templates` 并不以词干 `template` 结尾"
        );
        assert_eq!(
            infer_link(&col("template_id"), "customer_subscriptions", &k).map(|g| g.0),
            Some("package_templates".to_string()),
            "只探词干会漏掉核心用例 —— 必须同时探 stem+s"
        );
    }

    /// 角色限定的外键**仍然**推不出关系（REQ-008 的既有纪律不许被这次放宽破掉）。
    ///
    /// `main_customer_id` 的词干是 `main_customer`，库里没有以它结尾的表，
    /// 所以它保持注释名 —— 这一条由 `tests/test_fk_naming.rs` 端到端守着，
    /// 这里守的是判据本身。
    #[test]
    fn test_role_qualified_fks_still_infer_nothing() {
        let k = known(&["customers", "customer_rebate", "t_dev", "t_customer"]);
        for c in [
            "main_customer_id",
            "sub_customer_id",
            "source_customer_id",
            "from_customer_id",
            "to_customer_id",
            "related_customer_id",
        ] {
            assert_eq!(
                infer_link(&col(c), "some_owner", &k),
                None,
                "「{c}」必须仍推不出关系 —— 角色限定靠注释表达，放宽不许吃掉它"
            );
        }
    }

    /// 闸 D：**跨域同名碰撞不产出**。真实源库 91 表实测出这 2 条**错**关系。
    ///
    /// 词干 `task`（任务）是跨业务域通用的普通名词：实例/云手机域和文档解析域
    /// 各有各的「任务」。但只有 `file_parser_task` 把它放在表名末尾
    /// ⇒ 闸 B 判为「唯一」并放行 —— 关系却是错的。
    ///
    /// 闸 D 判「词干已被 owner 自己占用 + 候选与 owner 无共同词汇」⇒ 不产出。
    #[test]
    fn test_cross_domain_stem_collision_emits_nothing() {
        let k = known(&[
            "instance_task_record",
            "intance_task_plans",
            "instance_task_manager",
            "file_parser_task",
        ]);
        for owner in ["instance_task_record", "intance_task_plans"] {
            assert_eq!(
                infer_link(&col("task_id"), owner, &k),
                None,
                "{owner}.task_id 属实例任务域，绝不能被唯一尾缀命中拽向文档解析域的 file_parser_task"
            );
        }
    }

    /// 闸 D 的**判别力**：把判据退回「不检查同域占用」，上面那 2 条会重新出现。
    /// 变异 = 从 [`stem_bound_to_owner_domain`] 的返回里去掉该检查。
    #[test]
    fn test_gate_d_is_what_suppresses_the_cross_domain_collision() {
        let k = known(&["instance_task_record", "file_parser_task"]);
        // 闸 B 单独看：确实是「唯一命中」，闸 B 自己放行
        assert!(tail_match_target("task", "task_id", "instance_task_record", &k).is_none());
        // 闸 D 的判据本身返回 true（即「这是跨域碰撞」）
        assert!(stem_bound_to_owner_domain("task", "instance_task_record", "file_parser_task"));
        // 而同域的 owner 不触发
        assert!(!stem_bound_to_owner_domain("model", "llm_model_peak_config", "llm_model"));
    }

    /// 闸 D 不得误伤**共享词汇**的同域关系（真实源库实测这几条都是对的）。
    #[test]
    fn test_gate_d_keeps_same_domain_tail_matches() {
        let cases = [
            ("llm_model_peak_config", "model_id", "llm_model"),
            ("llm_model_sort", "model_id", "llm_model"),
            ("llm_routing_rules", "group_id", "llm_provider_groups"),
            ("llm_routing_rules", "provider_id", "llm_provider"),
            ("llm_provider_group_items", "group_id", "llm_provider_groups"),
        ];
        for (owner, column, target) in cases {
            let tables = [owner, target, "llm_provider", "llm_provider_groups"];
            let k = known(&tables);
            let got = infer_link(&col(column), owner, &k)
                .unwrap_or_else(|| panic!("{owner}.{column} → {target} 是同域关系，闸 D 不得挡下"));
            assert_eq!(got.0, target);
        }
    }

    /// 闸 D 的**首词元豁免**：`app` / `subscription` 是 owner 的作用域前缀，
    /// 它们指向的定义表住在别的域里是正常的 —— 不得当成跨域碰撞。
    ///
    /// 这两条 QA 独立复核判为**语义正确**，闸 D 不得伤到它们。
    #[test]
    fn test_gate_d_exempts_the_owners_scope_leading_token() {
        let t1 = ["app_billing_transactions", "service_apps"];
        let got = infer_link(&col("app_id"), "app_billing_transactions", &known(&t1))
            .unwrap_or_else(|| panic!("app 是 app_billing_transactions 的作用域前缀，闸 D 不得挡下"));
        assert_eq!(got.0, "service_apps");

        let t2 = ["subscription_priority", "customer_subscriptions"];
        let got = infer_link(&col("subscription_id"), "subscription_priority", &known(&t2))
            .unwrap_or_else(|| panic!("subscription 是 subscription_priority 的作用域前缀，闸 D 不得挡下"));
        assert_eq!(got.0, "customer_subscriptions");

        // 判据本身：首词元豁免、作用域内层词不豁免
        assert!(!stem_bound_to_owner_domain("app", "app_billing_transactions", "service_apps"));
        assert!(!stem_bound_to_owner_domain("subscription", "subscription_priority", "customer_subscriptions"));
        assert!(stem_bound_to_owner_domain("task", "instance_task_record", "file_parser_task"));
    }

    /// 闸 D 只在第 2 档生效：词干直接展开命中（第 1 档）时完全不受影响。
    #[test]
    fn test_gate_d_does_not_touch_the_exact_tier() {
        // 词干 `order` 直接展开命中 `orders`（既有行为，逐字节不变）⇒ 闸 D 不参与
        let k = known(&["orders", "order"]);
        let got = infer_link(&col("order_id"), "order", &k).unwrap();
        assert_eq!((got.0.as_str(), got.3), ("orders", LinkEvidence::Exact));
    }

    /// 证据档次要能落进 `evidence`，让读提案的人知道这条是怎么来的。
    #[test]
    fn test_link_evidence_records_the_tier() {
        assert!(LinkEvidence::Exact.claim(false).contains("直接展开"));
        assert!(LinkEvidence::TailMatch.claim(false).contains("尾缀"));
    }

    // ──────────────────────────────────────────────────────
    // REQ-108 · 置信度接线（AC-004 / AC-008）
    // ──────────────────────────────────────────────────────

    use crate::domain::confidence::{Band, DataEvidence};
    use crate::extract::introspect::{KeyCatalog, SchemaReport, TableKeys};
    use crate::extract::sample::SampleEvidence;

    fn meta(name: &str, cols: &[(&str, &str, bool)]) -> TableMeta {
        let columns: Vec<ColumnMeta> = cols
            .iter()
            .map(|(n, t, pk)| ColumnMeta {
                name: n.to_string(),
                data_type: t.to_string(),
                nullable: false,
                comment: None,
                is_primary_key: *pk,
            })
            .collect();
        TableMeta {
            name: name.to_string(),
            comment: None,
            column_count: columns.len(),
            commented_count: 0,
            columns,
        }
    }

    /// 三张表的迷你 schema：`customers(id varchar PK)` /
    /// `service_apps(app_id varchar PK)` / `logs(customer_id varchar, app_id varchar)`.
    fn schema_108() -> SchemaReport {
        SchemaReport {
            server_version: "PostgreSQL 16.14".into(),
            transaction_read_only: "on".into(),
            schema: "public".into(),
            table_count: 3,
            column_count: 5,
            commented_column_count: 0,
            foreign_key_count: 0,
            tables: vec![
                meta("customers", &[("id", "character varying", true)]),
                meta("service_apps", &[("app_id", "character varying", true)]),
                meta(
                    "logs",
                    &[("id", "integer", true), ("customer_id", "character varying", false), ("app_id", "character varying", false)],
                ),
            ],
            introspected_at: "2026-10-21T00:00:00Z".into(),
        }
    }

    fn keys_108() -> KeyCatalog {
        let mut k = KeyCatalog::new();
        k.insert(
            "customers",
            TableKeys { primary_key: Some("id".into()), unique_columns: vec!["id".into()], composite_primary_key: vec![] },
        );
        k.insert(
            "service_apps",
            TableKeys {
                primary_key: Some("app_id".into()),
                unique_columns: vec!["app_id".into()],
                composite_primary_key: vec![],
            },
        );
        k.insert(
            "logs",
            TableKeys { primary_key: Some("id".into()), unique_columns: vec!["id".into()], composite_primary_key: vec![] },
        );
        k
    }

    fn link_item(from: &str, to: &str, column: &str) -> serde_json::Value {
        serde_json::json!({
            "kind": "add_link_type",
            "target": format!("{from}→{to}"),
            "payload": {
                "key": format!("{from}_{column}__{to}"),
                "from_type": from,
                "to_type": to,
                "cardinality": "N:1",
                "semantic": "belongs_to",
                "evidence": [{"table": from, "column": column, "claim": "命名约定"}]
            }
        })
    }

    /// 坐标解析：只认关系条目，列名取自 `payload.evidence[0].column`，
    /// 键列取**目录事实**（不是「大概叫 id」）。
    #[test]
    fn test_link_candidates_use_catalog_keys_not_name_guesses() {
        let items = vec![
            serde_json::json!({"kind": "add_entity_type", "target": "客户", "payload": {}}),
            link_item("logs", "service_apps", "app_id"),
            link_item("logs", "customers", "customer_id"),
        ];
        let cands = link_candidates(&items, &keys_108());
        assert_eq!(cands.len(), 2, "实体条目不是候选");
        assert_eq!(cands[0].item_index, 1);
        assert_eq!(cands[0].from_column, "app_id");
        assert_eq!(
            cands[0].to_key_column.as_deref(),
            Some("app_id"),
            "service_apps 的主键是 app_id —— 猜 id 的实现会在这里指到不存在的列"
        );
        assert_eq!(cands[0].from_key.as_deref(), Some("id"));
        assert_eq!(cands[1].to_key_column.as_deref(), Some("id"));
        // 采样坐标真的用目录里的键列
        assert_eq!(cands[0].sample_spec().unwrap().to_key_column, "app_id");
    }

    /// 满分关系 ⇒ 1.0 高档；同一份 schema 下没有数据证据 ⇒ 只由结构算且被压到低档。
    #[test]
    fn test_score_links_high_band_and_no_sample_band() {
        let items = vec![link_item("logs", "customers", "customer_id")];
        let mut ev = std::collections::HashMap::new();
        ev.insert(
            0usize,
            SampleEvidence { sampled: 5, matched: 5, digest: Some("sha256:aa".into()), reason: None },
        );
        let scored = score_links(&items, &schema_108(), &keys_108(), &ev);
        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].1.score, 1.0);
        assert_eq!(scored[0].1.band, Band::High);
        assert_eq!(scored[0].1.data_evidence, DataEvidence::Sampled);
        assert_eq!(
            scored[0].1.signals.iter().find(|s| s.name == "target_is_key").unwrap().value,
            1.0
        );

        let empty = std::collections::HashMap::new();
        let scored = score_links(&items, &schema_108(), &keys_108(), &empty);
        assert_eq!(scored[0].1.data_evidence, DataEvidence::None, "没采 ⇒ none");
        assert!(!scored[0].1.band.enters_proposal(), "没采 ⇒ 不得直接进主提案");
    }

    /// 类型不相容（text → int）在接线上也生效：0.15 上限、落拒绝档。
    #[test]
    fn test_score_links_type_mismatch_lands_rejected() {
        let mut report = schema_108();
        // 把 customers.id 改成 integer，源列仍是 varchar
        report.tables[0].columns[0].data_type = "integer".into();
        let items = vec![link_item("logs", "customers", "customer_id")];
        let mut ev = std::collections::HashMap::new();
        ev.insert(
            0usize,
            SampleEvidence { sampled: 5, matched: 3, digest: Some("sha256:bb".into()), reason: None },
        );
        let scored = score_links(&items, &report, &keys_108(), &ev);
        assert!(scored[0].1.score <= 0.15, "实际 {}", scored[0].1.score);
        assert_eq!(scored[0].1.band, Band::Rejected);
        assert_eq!(scored[0].1.veto.as_ref().unwrap().signal, "type_incompatible");
    }

    /// 贴分数**只加一个键**，其余字段逐字节不变（AC-005「不改抽取结果」）。
    #[test]
    fn test_attach_confidence_adds_one_key_and_changes_nothing_else() {        let mut items = vec![link_item("logs", "customers", "customer_id")];
        let before = items[0].clone();
        let mut ev = std::collections::HashMap::new();
        ev.insert(
            0usize,
            SampleEvidence { sampled: 5, matched: 5, digest: Some("sha256:aa".into()), reason: None },
        );
        let scored = score_links(&items, &schema_108(), &keys_108(), &ev);
        attach_confidence(&mut items, &scored);

        let after = &items[0];
        assert!(after.get("confidence").is_some(), "confidence 必须贴上");
        // 去掉 confidence 之后与原文逐字节相同
        let mut stripped = after.clone();
        stripped.as_object_mut().unwrap().remove("confidence");
        assert_eq!(
            serde_json::to_string(&stripped).unwrap(),
            serde_json::to_string(&before).unwrap(),
            "除了新增 confidence 键，条目其余部分一个字节都不许动"
        );
        assert_eq!(after["confidence"]["method"], "evidence-scorecard-v1");
        assert_eq!(after["confidence"]["band"], "high");
    }

    /// **AC-009 的路由不变量**：三份互不相交、合起来是原来那些条目，
    /// 且实体条目的相对顺序不变（分档只动关系）。
    ///
    /// 变异自证：把 `Band::Rejected => out.rejected.push(item)` 改成
    /// `out.kept.push(item)`，第一条断言立刻红 —— 而那时的表现正是
    /// 「被拒绝的关系照样进提案，客户会在页面上看到一条 0.20 分的关系」。
    #[test]
    fn test_route_items_splits_without_losing_or_duplicating() {
        let mut items = vec![
            serde_json::json!({"kind": "add_entity_type", "target": "客户", "payload": {"key": "customers"}}),
            link_item("logs", "customers", "customer_id"),
            link_item("logs", "service_apps", "app_id"),
            link_item("logs", "nope", "x_id"),
            serde_json::json!({"kind": "add_entity_type", "target": "应用", "payload": {"key": "service_apps"}}),
        ];
        // 三条关系：高 / 中 / 拒绝
        let scored = vec![
            (1usize, crate::domain::confidence::score(&crate::domain::confidence::ScoreInput {
                column: "customer_id".into(),
                to_table: "customers".into(),
                from_type: crate::domain::confidence::TypeFamily::Str,
                to_type: Some(crate::domain::confidence::TypeFamily::Str),
                target_key: crate::domain::confidence::TargetKey::Primary,
                data: Some(crate::domain::confidence::SampleFacts { sampled: 5, matched: 5, digest: None, reason: None }),
                composite_target_key: false,
                polymorphic: false,
                self_ref: false,
            })),
            (2usize, crate::domain::confidence::score(&crate::domain::confidence::ScoreInput {
                column: "app_id".into(),
                to_table: "service_apps".into(),
                from_type: crate::domain::confidence::TypeFamily::Str,
                to_type: Some(crate::domain::confidence::TypeFamily::Str),
                target_key: crate::domain::confidence::TargetKey::Primary,
                data: Some(crate::domain::confidence::SampleFacts { sampled: 5, matched: 3, digest: None, reason: None }),
                composite_target_key: false,
                polymorphic: false,
                self_ref: false,
            })),
            (3usize, crate::domain::confidence::score(&crate::domain::confidence::ScoreInput {
                column: "x_id".into(),
                to_table: "nope".into(),
                from_type: crate::domain::confidence::TypeFamily::Num,
                to_type: Some(crate::domain::confidence::TypeFamily::Str),
                target_key: crate::domain::confidence::TargetKey::Primary,
                data: Some(crate::domain::confidence::SampleFacts { sampled: 5, matched: 0, digest: None, reason: None }),
                composite_target_key: false,
                polymorphic: false,
                self_ref: false,
            })),
        ];
        items.shrink_to_fit();
        let total = items.len();
        let routing = route_items(items, &scored);

        // ① 三份互不相交，合起来不丢不重
        assert_eq!(
            routing.kept.len() + routing.pending.len() + routing.rejected.len(),
            total,
            "路由必须覆盖全部条目（少一条 = 静默丢弃）"
        );
        let mut keys: Vec<String> = routing
            .kept
            .iter()
            .chain(routing.pending.iter())
            .chain(routing.rejected.iter())
            .map(|i| i.to_string())
            .collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), before, "同一条目出现在两份清单里");
        assert_eq!(routing.unscored_links, 0);

        // ② 档位去向
        assert_eq!(routing.counts.high, 1);
        assert_eq!(routing.counts.middle, 1);
        assert_eq!(routing.counts.rejected, 1);
        assert_eq!(routing.counts.low, 0);
        assert_eq!(routing.kept.len(), 4, "2 实体 + 高 + 中");
        assert_eq!(routing.rejected.len(), 1);
        assert!(
            routing
                .kept
                .iter()
                .all(|i| i["payload"]["from_type"] != "logs" || i["confidence"]["band"] != "rejected"),
            "拒绝档绝不能出现在 kept 里"
        );
        // ③ 实体条目顺序不变（客户在前、应用在后）
        let entities: Vec<&str> = routing
            .kept
            .iter()
            .filter(|i| i["kind"] == "add_entity_type")
            .map(|i| i["payload"]["key"].as_str().unwrap())
            .collect();
        assert_eq!(entities, vec!["customers", "service_apps"]);
    }

    /// 没有分数的关系条目进「待查」，**不冒充「高」**（少了一条都没验过）。
    #[test]
    fn test_route_items_puts_unscored_links_into_pending() {
        let items = vec![link_item("logs", "customers", "customer_id")];
        let routing = route_items(items, &[]);
        assert_eq!(routing.kept.len(), 0);
        assert_eq!(routing.pending.len(), 1);
        assert_eq!(routing.unscored_links, 1);
        assert_eq!(routing.counts.total(), 0);
    }
}
