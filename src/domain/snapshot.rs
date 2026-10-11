//! 快照契约 —— **本 MVP 最重要的一个结构**，验收 AC-003 / AC-010 / AC-011。
//!
//! 本层**零 I/O**（AC-002）：只有数据结构和纯函数，不出现 `sqlx`。
//! 存取由 `src/db/` 负责。
//!
//! # 三态语义（本文件的主要坑）
//!
//! `None` / `[]` / `{}` 三者**各有含义，不能互相吞掉**：
//!
//! | 值 | 含义 | 典型字段 |
//! |---|---|---|
//! | `None` | 明确没有 / 未适用 | `description: None`、`extracted_by: None`（人工修订） |
//! | `[]` | 集合存在但为空 | `aliases: []`、`disjoint_with: []`、`failed_groups: []` |
//! | `{}` | 对象存在但字段空 | `permission: {}` |
//!
//! serde + sqlx 组合下这三者容易互相吞（`NULL` 变 `[]` 或反过来），
//! 所以 `tests/test_snapshot.rs` 故意放三态并断言往返无损。
//!
//! # 为什么 `parent` 存 key 而不是 id
//!
//! 快照是给人看、也给 diff 用的。存 id 看不出「父从机器人设备变成了设备」
//! —— 评审者面对的是一串 UUID。这是规格 §4.6 的隐含决定，在此显式化。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// **这个溯源值是从哪条通道来的**（ADR-003 决策 (b) · REQ-022）。
///
/// # 为什么需要它（ADR-003 的实测缺口）
///
/// ADR-003 实证：`ontok propose import` 从 JSON **原样**取 `extracted_by`、
/// 零校验，于是 `{"agent":"totally-not-a-real-agent","version":"9.9.9-fake"}`
/// 也能入库，提案元数据与本体版本里长得和真的一样。
/// 本系统**无法**证明声明是真的（ADR-003 D2：这不是安全漏洞，是数据可信度问题），
/// 那就至少**留痕**：这个值是**工具自己跑出来的**，还是**从一份外部文件读进来的**。
///
/// | 取值 | 含义 | 今天**谁**写它
/// | --- | --- | --- |
/// | `declared` | **从外部 JSON 声明里读进来的**（未经验证）| `cli.rs` 的两条 import 通道（`concept import` / `propose import`）—— **今天唯一会写这个字段的通道** |
/// | `tool_produced` | 本工具的抽取编排跑出来的 | ⚠ **今天无人写入**（见 [`ProvenanceSource::ToolProduced`] 的说明）—— **为将来预留** |
///
/// ⚠ **本类型今天只有一个值在用**（REQ-022 复审实测：全仓 `ProvenanceSource::`
/// 的构造点只有 `cli.rs` 的两处，且**都是** `Declared`）。所以
/// [`ExtractedBy::is_declared`] 目前的实际判别力来自**三态**而不是二值：
/// `None`（旧数据 / 人工修订）、`Some(Declared)`、`Some(ToolProduced)`（暂无写入方）。
/// **不要**因为「反正只有 declared 在用」就把这个字段简化掉 —— 三态里
/// `None` 与 `declared` 的区别正是 ADR-003 要留的痕。
///
/// # 为什么 `tool_produced` 保留（而不是删掉）
///
/// 保留的理由是**反序列化仍然 fail-closed**：将来真有自产路径时，
/// 「`source` 不在这两个值之内」必须在**反序列化时**就失败，而不是被读成别的。
/// 删除它不会让今天更真，只会让将来那条路径无值可写、或诱导它去写
/// `Declared`（那才是把自产说成外部声明）。
///
/// ⚠ **绝不允许**为了让这张表好看，给某条路径硬塞一个 `ToolProduced` ——
/// 「这条路径真的自己产出了 items」是需要证据的断言，不是想让文档成立就能写上的字。
///
/// # 为什么是**枚举**而不是自由字符串
///
/// 自由字符串能填 `"ok"`、`"trusted"`、`"yes"` —— 读的人无法判断
/// 「这一格到底在说什么」，而这一格的全部价值就是**判读**。
/// 枚举让「不是这两个之一」在**反序列化时**就失败（fail-closed）。
///
/// ⚠️ `None` = **旧数据 / 人工修订**（ADR-001：`extracted_by` 缺失记 `null`）。
/// `None` **不是**「自产」—— 不知道来源就说不知道，不往好的方向猜。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceSource {
    /// 工具自产（本工具的抽取编排产出）。
    ///
    /// ⚠ **今天无人写入这个值**（REQ-022 复审 · 本文档此前在这里说过假话：
    /// 它声称写入方是「REQ-022 起的 `orchestrate`」，而全仓**没有任何一处**
    /// 构造它）。之所以没有，如实说清：
    ///
    /// - `orchestrate` 产出的溯源是
    ///   [`crate::extract::propose::ProposalProvenance`] —— **另一个类型**
    ///   （`source` 是「勘查留痕」那句**字符串**，不是来源通道）；
    /// - `ontok-propose` **不连库**，它只写 `proposal.json`；
    /// - 提案要进库**必须**再走一次 `import`，而两条 import 通道
    ///   （`cli.rs`）**都**标 `Declared`。
    ///
    /// 换句话说：**本类型目前只由「读外部 JSON」的那条通道产生**。
    ///
    /// # 将来由谁写（这是**待满足的前提**，不是既成事实）
    ///
    /// 只有当某条路径**自己产出 items 并直接写进快照表**、
    /// 且**不经过任何外部 JSON** 时，它才该写这个值。
    /// 那种路径今天**不存在**；它出现时必须同时：
    /// (1) 在本文件上方的表格里把「谁写」从「无人写入」改成那条路径；
    /// (2) 让该路径**证明** items 是它产的（对照
    ///     [`crate::agent::orchestrate::require_proposal_producing`] 的能力闸门），
    ///     而不是「因为它在工具里，所以是自产」。
    ToolProduced,
    /// 外部声明（从 JSON 读入，系统**未**验证其真伪）。
    Declared,
}

/// 抽取来源（规格 §8.4）。
///
/// ⚠️ **与 `crate::extract::propose::ProposalProvenance` 不是同一个类型**
/// （REQ-022 之前两者**同名**，都叫 `ExtractedBy`，形状还不一样）。
/// ADR-003 把这个重复记成「命名重复本身是隐患」——
/// 本类型留在快照层（落库的那份，名字与 JSON 字段 `extracted_by` 对齐），
/// 提案侧那个已改名为 [`ProposalProvenance`](crate::extract::propose::ProposalProvenance)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtractedBy {
    /// 抽取器名
    pub agent: String,
    /// **具体版本号**，不是 "latest" —— 版本漂移是 §8.4 要防的问题
    pub version: String,
    /// 第几轮抽取
    pub round: i32,
    /// 失败的分组。**不参与收敛判定**，但必须记 —— 漏记等于假装全成功
    #[serde(default)]
    pub failed_groups: Vec<String>,
    /// **来源通道**（ADR-003 决策 (b) · REQ-022）：工具自产 / 外部声明。
    ///
    /// `None` = 旧数据或人工修订 —— **不猜**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ProvenanceSource>,
}

impl ExtractedBy {
    /// 是否全部成功。`failed_groups` 非空即否。
    pub fn fully_succeeded(&self) -> bool {
        self.failed_groups.is_empty()
    }

    /// 这个溯源值**是不是**从外部声明读进来的（ADR-003）。
    ///
    /// 存在的意义是把「这条声明**未经系统验证**」这件事变成一个
    /// **可判定的查询**，而不是靠读代码的人记得 ADR-003 的背景。
    pub fn is_declared(&self) -> bool {
        self.source == Some(ProvenanceSource::Declared)
    }
}

/// 证据（审计用，**不投影到客户视图**）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub table: String,
    pub column: String,
    pub claim: String,
}

/// 属性。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Property {
    /// 业务名（中文）。空则由 view 层用列名人话化兜底
    #[serde(default)]
    pub name: String,
    /// 源列名 —— **只在本层与建模视图出现，绝不进客户视图**
    pub column: String,
    pub nullable: bool,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub sample: Option<String>,
}

/// 对象类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityType {
    pub key: String,
    pub display_name: String,
    /// `None` = 没有说明。与 `Some("")` 不同，后者是「有说明但为空」
    #[serde(default)]
    pub description: Option<String>,
    /// 别名。空数组 = 没有别名，**不是缺失**
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 父类型的 **key**（不是 id）
    #[serde(default)]
    pub parent: Option<String>,
    /// 与之互斥的类型 key 列表
    #[serde(default)]
    pub disjoint_with: Vec<String>,
    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// 关系类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkType {
    pub key: String,
    pub from_type: String,
    pub to_type: String,
    /// `"1:1"` / `"1:N"` / `"N:M"`
    pub cardinality: String,
    /// `has` / `belongs_to` / `depends_on` / …
    pub semantic: String,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// **写回绑定（占位）** —— 规格 §5.2。
///
/// 切片 1 **不实现任何写回**，但结构必须在：
/// 「只建不用」的东西最容易被后人当垃圾清理掉，而它是切片 2 的地基。
/// AC-003 的 grep 守住这 4 个子字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteBinding {
    pub system: String,
    pub table: String,
    pub field: String,
    /// 定位行：{"col": "ticket_id"} 之类
    #[serde(default)]
    pub key_expr: Value,
    /// 目标值映射：{"dispatched": "DISPATCHED"}
    #[serde(default)]
    pub value_map: BTreeMap<String, String>,
    /// 能写哪些值。**能为空** —— 「不支持」是有效表达，不是缺失
    #[serde(default)]
    pub supported_values: Vec<String>,
    /// 乐观锁字段。`None` = 该表没有并发控制
    #[serde(default)]
    pub conflict_field: Option<String>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// **动作类型（占位）** —— 语义网里没有对应物，见规格 §5.5。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionType {
    pub key: String,
    pub target: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub preconditions: Value,
    #[serde(default)]
    pub effects: Vec<WriteBinding>,
    #[serde(default)]
    pub permission: Value,
    #[serde(default)]
    pub audit_policy: Value,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// **函数（占位）** —— OWL/SHACL/SWRL 都没有干净对应。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Function {
    pub key: String,
    pub signature: String,
    #[serde(default)]
    pub allowed_types: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// 完整本体快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub version_id: uuid::Uuid,
    pub seq: i32,
    /// **可空**：`None` = 人工修订（ADR-001）。取值由提案带入，
    /// **不继承 base、不推断**
    #[serde(default)]
    pub extracted_by: Option<ExtractedBy>,
    #[serde(default)]
    pub entity_types: Vec<EntityType>,
    #[serde(default)]
    pub link_types: Vec<LinkType>,
    /// 切片 1 恒空，但**结构必须在** —— 缺结构与「空集合」是两回事
    #[serde(default)]
    pub action_types: Vec<ActionType>,
    #[serde(default)]
    pub functions: Vec<Function>,
    /// REQ-010 · **业务概念组**（§7.1 阶段 3）。
    ///
    /// 【红线】这一栏**不改变实体如何落库**：加了它，`entity_types`
    /// 的行数与 key 与不加它时逐字节一致。归并判断错了，去掉这一栏即可，
    /// 底层结构不动 —— 见 [`crate::domain::concept`] 模块文档。
    ///
    /// 放在快照里而不是另开一张表，是因为「一个版本 = 一份完整的本体状态」：
    /// 概念组与它描述的实体必须**同生共死**，否则会出现「实体升了、
    /// 概念组还是上一版的」这种没人说得清谁对的状态。
    #[serde(default)]
    pub concept_groups: Vec<crate::domain::concept::ConceptGroup>,
}

impl Snapshot {
    pub fn new(version_id: uuid::Uuid, seq: i32) -> Self {
        Self {
            version_id,
            seq,
            extracted_by: None,
            entity_types: vec![],
            link_types: vec![],
            action_types: vec![],
            functions: vec![],
            concept_groups: vec![],
        }
    }

    pub fn entity(&self, key: &str) -> Option<&EntityType> {
        self.entity_types.iter().find(|e| e.key == key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.entity(key).is_some()
    }

    /// 层级关系表（key → 父 key），喂给 `hierarchy` 的纯函数。
    pub fn parents(&self) -> BTreeMap<String, Option<String>> {
        self.entity_types
            .iter()
            .map(|e| (e.key.clone(), e.parent.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_new_is_all_empty_not_null() {
        let s = Snapshot::new(uuid::Uuid::nil(), 1);
        assert!(s.extracted_by.is_none());
        assert!(s.entity_types.is_empty());
        assert!(s.action_types.is_empty(), "占位字段存在但为空集合");
        assert!(s.functions.is_empty());
        assert!(s.concept_groups.is_empty(), "REQ-010：字段存在但为空集合");
    }

    #[test]
    fn test_snapshot_extracted_by_failed_groups_do_not_block_success() {
        // 失败的分组**不参与收敛判定** —— 但必须被记下来
        let e = ExtractedBy {
            agent: "x".into(),
            version: "1.0.0".into(),
            round: 1,
            failed_groups: vec!["t_foo".into()],
            source: None,
        };
        assert!(!e.fully_succeeded());
        let ok = ExtractedBy { failed_groups: vec![], ..e.clone() };
        assert!(ok.fully_succeeded());
    }

    #[test]
    fn test_snapshot_parents_feeds_hierarchy() {
        let mut s = Snapshot::new(uuid::Uuid::nil(), 1);
        s.entity_types = vec![
            EntityType {
                key: "asset".into(),
                display_name: "资产".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
            EntityType {
                key: "device".into(),
                display_name: "设备".into(),
                description: None,
                aliases: vec![],
                parent: Some("asset".into()),
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
        ];
        let p = s.parents();
        assert_eq!(crate::domain::hierarchy::ancestors_of("device", &p), vec!["asset"]);
    }
}
