//! 规则引擎实现 —— 现有 `extract()` 的第一个 [`ExtractionAgent`]。
//!
//! # 这个文件**证明**抽象能用，不是造死代码
//!
//! [`RuleBasedAgent::extract_group`] 走的是**既有那条规则管线**
//! （`extract::propose::extract`）—— 没有第二份规则、没有复制粘贴的判断。
//! 所以「把规则抽成 agent」这件事是真的跑通了，而不是在旁边摆一个壳。
//!
//! # 为什么 `extract_group` 的产出**没有 Link**
//!
//! 规格 §7.1 把「两张表都有 device_id → 是否提出 Link？」放在**阶段 3 跨组合成**，
//! 不是阶段 1。规则实现遵守这个分工：`extract_group` 只投影
//! **候选类型 + 属性 + 映射声明**。跨组结论由 [`RuleBasedAgent::propose`]
//! 在全量 report 上产出（那是阶段 3+4 的职责）。
//!
//! # 逐字节兼容
//!
//! [`RuleBasedAgent::propose`] 是 [`extract::propose::extract`] 的**包壳**，
//! 不改动它的函数体一个字节。REQ-013 时既有调用点
//! （`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`tests/test_prop_name_quality.rs`、
//! `src/bin/ontok-propose.rs`）**签名与产出全部不变**。
//!
//! ⚠️ **REQ-015 起生产路径不再调本方法**：`ontok-propose` 改走
//! [`super::factory::build_agent`] + [`super::orchestrate::orchestrate`]，
//! 那条路径**分组**跑阶段 1 并在代码侧做阶段 2 核对（缺陷 F2 / F3）。
//!
//! **REQ-020 起本方法也不导出**（`#[cfg(test)]` + `pub(crate)`）：
//! 它是 trait 之外的固有方法，会把 `self.identity` 原样盖进 `extracted_by` ——
//! 「捏一个身份 + 盖章」就是造假溯源的机器，而能力闸门只装在 `orchestrate` 上。
//! 论证见 [`RuleBasedAgent::propose`] 的文档。随之迁进 crate 内的两条
//! 「包成 agent 后逐字节不变 / 统计字段不变」断言在本文件末尾的
//! `mod tests` 里 —— 它们零 I/O，搬进 crate **不损失任何覆盖面**。
//! [`RuleBasedAgent::new`] 保持 `pub`：构造出来只能走 trait 的
//! `extract_group`（产出 `GroupExtraction`，**没有 `extracted_by`**），
//! 造不出假溯源。
//!
//! # REQ-019：为什么只有本实现声明 `produces_proposal`
//!
//! 编排的阶段 3+4 用的是 [`propose::extract_with_context`] —— 与
//! `extract_group` **同一条**规则管线。所以 `items` 真的是本实现的产出，
//! `extracted_by` 记它不是冒名。别的实现（协议 adapter）若做不到这一点，
//! [`ExtractionAgent::capabilities`] 就**不许**声明那个标签 ——
//! 声明了编排会拿规则产物盖上它的身份，那就是静默的假溯源
//! （见 [`super::CAPABILITY_PRODUCES_PROPOSAL`]）。

use anyhow::Result;

use super::group::{
    CandidateLink, CandidateProperty, CandidateType, ColumnRef, ExtractionGroup, GroupExtraction,
};
use super::registry::AgentIdentity;
use super::{ExtractionAgent, CAPABILITY_PRODUCES_PROPOSAL};
use crate::extract::names::TableNames;
use crate::extract::propose;
// REQ-020：`propose` 是 `#[cfg(test)]` 的，这两个类型只出现在它的签名里，
// 所以 import 必须与它同生共死 —— 否则生产构建会报「未使用的 import」。
#[cfg(test)]
use crate::extract::introspect::SchemaReport;
#[cfg(test)]
use crate::extract::propose::ExtractionOutcome;

/// **规则抽取 agent** —— 进程内、零 I/O、确定性、可 diff。
///
/// 结构与协议无关：`protocol = "inproc"`，`endpoint = None`。
pub struct RuleBasedAgent {
    identity: AgentIdentity,
    names: TableNames,
}

impl RuleBasedAgent {
    /// 构造。`names` 是 REQ-003 的表名翻译表 —— 它是规则引擎的知识资产
    /// （`table_names.json` 里记着它自己的 agent/version，形态与 registry 记录一致）。
    pub fn new(identity: AgentIdentity, names: TableNames) -> Self {
        Self { identity, names }
    }

    pub fn names(&self) -> &TableNames {
        &self.names
    }

    /// 阶段 3+4：全量组装成提案 —— **既有行为，逐字节不变**。
    ///
    /// 委托给 [`propose::extract`] 的未改动函数体；
    /// 之后只覆写**溯源**字段（`round` / `extracted_by`），
    /// 让它们来自 [`ExtractionAgent::identity`] 而不是 `extract()` 里的硬编码 1。
    ///
    /// ⚠️ 覆写只动 `round` 与 `extracted_by` 这三个溯源字段，
    /// **`items` 一个字节都不碰** —— 那才是提案的实质内容。
    ///
    /// # REQ-020：`#[cfg(test)]` + `pub(crate)` —— 生产构建里这个方法**不存在**
    ///
    /// 本方法是**固有方法**（不在 `ExtractionAgent` trait 上），且它把
    /// `self.identity` **原样**盖进 `extracted_by`。于是 crate 外只要能
    /// 构造一个 `RuleBasedAgent` 并自带一个**随手捏的身份**，就能造出与 QA
    /// 在 `propose::extract` 上实测到的缺口**完全同形**的假溯源：
    ///
    /// ```text
    /// items              = 规则管线抽的
    /// extracted_by.agent = 调用方随手填的那个字符串
    /// ```
    ///
    /// 能力闸门（[`ExtractionAgent::capabilities`]）挡不住这个：
    /// 它判的是**这个类型**的 `items` 是不是它产的 —— `RuleBasedAgent` 说的
    /// 是**实话**（它确实跑规则管线），谎在**名字**上。而「这个名字对不对」
    /// 只有 agent-registry 答得上来，`RuleBasedAgent` 零 I/O、答不了。
    ///
    /// 所以只能靠**可见性**收口。而且注意一个早就在那儿的事实：
    /// **REQ-015 起生产路径就不调本方法**（`ontok-propose` 走
    /// `build_agent` + `orchestrate`）——
    /// [`crate::agent::orchestrate::orchestrate`] 自己跑
    /// [`propose::extract_with_context`] 再盖章。所以本方法今天只是
    /// 「`extract` 与 agent 路径**逐字节相同**」这条断言的**对照物**，
    /// 生产构建里留着它只是死代码 + 一台现成的盖章机。
    ///
    /// 于是直接 `#[cfg(test)]`：生产二进制里**这个方法根本不存在**，
    /// 也就没有「构造实例 → 盖章」这条路。crate 外能拿到的
    /// `RuleBasedAgent`（`new` 保持 `pub`）只能走 trait 的 `extract_group` ——
    /// 它产出 `GroupExtraction`，**没有 `extracted_by`**，造不出假溯源。
    /// 生产路径仍只有一条，且每一步都带闸门：
    /// [`crate::agent::factory::build_agent`] 造实例（身份来自真实登记、
    /// `protocol` 分派、登记过 [`crate::agent::registry::AgentRegistration::validate`]）
    /// → [`crate::agent::orchestrate::orchestrate`]（先过
    /// `require_proposal_producing` 再盖章）。
    #[cfg(test)]
    pub(crate) fn propose(&self, report: &SchemaReport, project: &str) -> Result<ExtractionOutcome> {
        let mut out = propose::extract(
            report,
            &self.names,
            project,
            &self.identity.agent,
            &self.identity.version,
        )?;
        // 溯源填真（§8.4）：`round` / `extracted_by.round` / `failed_groups`
        // 三个字段的真值来源是 registry 登记的身份，不是 `extract()` 里的
        // 硬编码 1。实现与编排入口共用 [`super::stamp_provenance`]，
        // 免得「溯源怎么填」有两个会漂移的写法。
        //
        // 规则实现从不失败 —— 它是确定性纯函数，所以恒传空切片，
        // 与 REQ-013 之前 `failed_groups: Vec::new()` 的序列化结果一致。
        super::stamp_provenance(&mut out, &self.identity, &[])?;
        Ok(out)
    }
}

impl ExtractionAgent for RuleBasedAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }

    /// REQ-019：本实现**声明** `produces_proposal`，而且这句话**是真的**。
    ///
    /// 编排的阶段 3+4 跑的就是 [`propose::extract_with_context`] —— 上面
    /// `extract_group` 走的是同一条规则管线，**没有第二份规则**。
    /// 于是 `items` 确实由本实现产出，`extracted_by` 记它不是冒名。
    ///
    /// 这也是截至本提交**唯一**敢声明这个标签的实现：声明它等于对
    /// 「阶段 3+4 的产物真是我产的」这句事实签字，签不了的实现就该被编排拒绝。
    fn capabilities(&self) -> Vec<String> {
        vec![CAPABILITY_PRODUCES_PROPOSAL.to_string()]
    }

    /// 阶段 1：抽一组 → 候选类型 + 映射声明。
    ///
    /// 复用既有规则管线（[`propose::extract_with_context`]）跑这一组的 schema 片段，
    /// 再把产出的实体投影成阶段 1 的形状。**没有第二份规则。**
    ///
    /// # REQ-014 · F1：作用域走 `scope`，上下文走 `context_tables`
    ///
    /// 两个字段**必须都传**：
    ///
    /// - `scope` —— 这一组要抽的表（设计点②：按表分组，不跨表聚类）；
    /// - `context_tables` —— 全库表名，只给 [`propose`] 做 FK 消歧。
    ///
    /// 只传 `scope` 的话，组外的 FK 目标会被当成**不存在**，
    /// `t_dev.t_customer_id` 这类列的关系就推不出来，属性名回落成注释的
    /// 「客户标识」—— 与阶段 3/4 的「所属客户」**不一致**。
    /// 判据测试：`test_fk_target_outside_group_scope_still_yields_relation_name`。
    fn extract_group(&self, group: &ExtractionGroup) -> Result<GroupExtraction> {
        let input = group.to_extraction_input();
        // `project` 只影响提案外壳（`Proposal.project`），阶段 1 的投影不用它。
        let outcome = propose::extract_with_context(
            &input.scope,
            &input.context_tables,
            &self.names,
            group.group_id.as_str(),
            &self.identity.agent,
            &self.identity.version,
        )?;

        // 已知表集合 = 作用域 ∪ 上下文（与 `extract_with_context` 同一个式子，
        // REQ-014 · F1：只传作用域的话，组外的 FK 目标会被当成不存在）。
        let known_tables: std::collections::HashSet<&str> = input
            .scope
            .tables
            .iter()
            .map(|t| t.name.as_str())
            .chain(input.context_tables.iter().map(|s| s.as_str()))
            .collect();
        let object_names = self.names.display_name_set();

        let mut types = Vec::new();
        for item in &outcome.proposal.items {
            if item["kind"] != "add_entity_type" {
                continue; // 关系候选单独投影（见下方 `links_for_table` 一段）
            }
            let key = item["payload"]["key"].as_str().unwrap_or_default().to_string();
            let display_name = item["payload"]["display_name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let table = key.clone(); // 规则实现里 key 就是表名（REQ-002 既有约定）
            let schema_table = input
                .scope
                .tables
                .iter()
                .find(|t| t.name == table)
                .expect("提案里的实体一定来自本组作用域");

            // 属性：从**共享的属性名决策**（`propose::build_properties`）逐条投影，
            // 不是从 JSON 里反推 —— 反推就得在 agent 侧再实现一遍
            // 「这个名字哪来的」，那是第二份判断（见 `build_properties` 的文档）。
            let drafts = propose::build_properties(schema_table, &known_tables, &self.names, &object_names);
            let properties: Vec<CandidateProperty> = drafts
                .iter()
                .map(|d| CandidateProperty {
                    name: d.item.name.clone(),
                    source: ColumnRef::new(table.as_str(), d.item.column.as_str()),
                    // 提案条目里 `nullable` 是 `bool`（不是 Option）——
                    // 这里如实转成 `Some`，重建时直接取回，不猜。
                    nullable: Some(d.item.nullable),
                    note: d.item.note.clone(),
                    origin: Some(d.origin),
                })
                .collect();
            let rejected_comments = drafts.iter().filter(|d| d.comment_rejected).count();

            // `is_new`：规则实现提的类型**不在** existing_types 里才算新类型。
            // 注意 `existing_types` 为空时全部算新类型 —— 组自足了就是新类型，
            // 这与设计点①「或标记为新类型」一致。
            let is_new = !group.existing_types.iter().any(|e| e.key == key);
            types.push(CandidateType {
                key,
                display_name,
                is_new,
                properties,
                // REQ-022：把提案实体条目要带的字段**原样**搬进候选，
                // 阶段 4 重建时原样写回 —— 取值必须与全量路径同源，
                // 所以直接从 `extract_with_context` 的产出里读，不另算。
                description: item["payload"]["description"].as_str().map(|s| s.to_string()),
                aliases: item["payload"]["aliases"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default(),
                evidence: item["payload"]["evidence"].as_array().cloned().unwrap_or_default(),
                // REQ-005 的降级痕迹：统计里 `props_comment_rejected` 记的就是它，
                // 候选不报上来，重建路径就会把 6 次降级写成 0 次。
                rejected_comments: Some(rejected_comments),
            });
        }

        // ── REQ-022：关系候选（阶段 3 跨组合成的输入）─────────────────
        //
        // 为什么不能从上面那个 `outcome` 里投影：全量路径末尾的 AC-007 自检
        // 会把「终点实体不在本组」的关系全部删掉（记进 `links_dangling`），
        // 而组只有一张表 ⇒ 指向组外表的关系在 `items` 里**一条都不剩**。
        // 重建路径需要它们，否则提案会少 51 条关系。
        //
        // 所以直接调**共享的** [`propose::links_for_table`] ——
        // 与全量路径同一个函数（REQ-022 把关系规则从循环里抽出来的原因），
        // 「`*_id` 怎么变成一条关系」仍然只有一份实现。
        let links = input
            .scope
            .tables
            .iter()
            .flat_map(|t| {
                let from_cn = propose::entity_display_name(t, &self.names).unwrap_or_default();
                propose::links_for_table(t, &from_cn, &known_tables, &self.names)
            })
            .map(|l| CandidateLink {
                key: l.payload.key,
                target: l.target,
                from_type: l.payload.from_type,
                to_type: l.payload.to_type,
                cardinality: l.payload.cardinality,
                semantic: l.payload.semantic,
                evidence: l.payload.evidence,
            })
            .collect();

        Ok(GroupExtraction { types, links })
    }
}

// ──────────────────────────────────────────────────────────
// REQ-020 · 迁进 crate 的两条断言
//
// 这两条原先住在 `tests/test_extraction_agent.rs`，它们调用的是**本文件里
// `pub(crate)` 的 `propose`**。方法收窄后集成测试从 crate 外调不到，
// 所以一并搬进来 —— 两条都是**零 I/O** 的纯函数断言，搬进来不损失覆盖面，
// 判据的严格程度一个字没改。
//
// 为什么值得搬：`test_rule_based_agent_output_is_byte_identical_to_legacy_extract`
// 钉的是「抽象层不许引入行为漂移」这条纪律；它守的是 `extract` 与
// `RuleBasedAgent::propose` **两条路径的产出必须逐字节一致** ——
// 也就是说，它是那个收窄的**双向对照的另一端**：收窄只该改「谁能调」，
// 不该改「调出来是什么」。断言留在 crate 内反而更贴近它要守的关系。
// ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
    use crate::extract::propose::extract;

    const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req013-test",
  "names": {
    "t_dev": "设备",
    "t_customer": "客户"
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

    /// 一份小而有关系的 schema：`t_dev` 有一列 `t_customer_id` 指向 `t_customer`
    /// （零外键库靠 `*_id` 命名约定推关系）。
    fn schema() -> SchemaReport {
        let tables = vec![
            table(
                "t_dev",
                Some("设备表"),
                vec![
                    col("id", None),
                    col("sn", Some("序列号")),
                    col("model", Some("型号")),
                    col("t_customer_id", Some("客户标识")),
                ],
            ),
            table(
                "t_customer",
                Some("客户表"),
                vec![col("id", None), col("name", Some("客户名称"))],
            ),
        ];
        SchemaReport {
            server_version: "16.14".into(),
            transaction_read_only: "on".into(),
            schema: "public".into(),
            table_count: tables.len(),
            column_count: tables.iter().map(|t| t.columns.len()).sum(),
            commented_column_count: tables.iter().map(|t| t.commented_count).sum(),
            foreign_key_count: 0,
            tables,
            introspected_at: "2026-10-07T10:00:00Z".into(),
        }
    }

    fn names() -> TableNames {
        TableNames::load(NAMES_JSON).expect("翻译表加载失败")
    }

    /// 抽象层的第一条纪律是**没引入任何行为变化**。
    ///
    /// 断言的是**序列化后的完整 JSON 字节**，不是几个字段 ——
    /// 只比字段的话，哪天多塞了一个 key 进 `extracted_by` 就没人知道了。
    /// ★ REQ-020：收窄可见性只许改「谁能调」，这条断言就是那道保险。
    #[test]
    fn test_rule_based_agent_output_is_byte_identical_to_legacy_extract() {
        let (report, tn) = (schema(), names());

        // 既有路径：五个参数的 extract()（两条门禁与 CLI 走的就是它）
        let legacy =
            extract(&report, &tn, "req013", "mcode-agent", "2026.10.07").expect("旧路径抽取失败");

        // 新路径：经过 RuleBasedAgent + legacy_cli 身份（round 恒为 1）
        let agent = RuleBasedAgent::new(
            AgentIdentity::legacy_cli("mcode-agent", "2026.10.07"),
            tn.clone(),
        );
        let via_agent = agent.propose(&report, "req013").expect("agent 路径抽取失败");

        assert_eq!(
            serde_json::to_string_pretty(&legacy).unwrap(),
            serde_json::to_string_pretty(&via_agent).unwrap(),
            "包成 agent 之后提案的 JSON 字节必须逐字节一致"
        );
        // 显式再钉一遍溯源三元组（变异时要能一眼看出是哪一项漂了）
        assert_eq!(via_agent.proposal.round, 1);
        assert_eq!(via_agent.proposal.extracted_by.agent, "mcode-agent");
        assert_eq!(via_agent.proposal.extracted_by.version, "2026.10.07");
        assert_eq!(via_agent.proposal.extracted_by.round, 1);
        assert!(via_agent.proposal.extracted_by.failed_groups.is_empty());
    }

    /// `ExtractionStats` 的统计口径**不许动**（BA 已知它会失真，但那是后续批次的事）。
    /// 把 10 个字段名钉死 —— 改名/增删字段会让 E2E 报告的判读器读不到数。
    #[test]
    fn test_extraction_stats_fields_are_untouched() {
        let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("a", "1.0.0"), names());
        let out = agent.propose(&schema(), "req013").unwrap();
        let stats_json = serde_json::to_value(&out.stats).unwrap();
        let mut keys: Vec<&str> = stats_json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        let mut expected = vec![
            "tables_seen",
            "total_properties",
            "entities_with_cn_name",
            "props_via_wordlist",
            "props_via_comment",
            "props_via_relation",
            "props_comment_rejected",
            "props_dropped",
            "links_inferred",
            "links_dangling",
            "tables_without_cn_name",
        ];
        expected.sort_unstable();
        assert_eq!(
            keys, expected,
            "ExtractionStats 的字段集不许变（10 个标量 + 1 个列表）"
        );
    }
}
