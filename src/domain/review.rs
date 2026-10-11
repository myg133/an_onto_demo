//! 双轨审核状态机 —— 纯逻辑，**零 I/O**。
//!
//! 规格 §8.3：客户审语义正确性（业务轨），FDE 审建模质量（建模轨）。
//! 两条轨独立判定；**关系类变更依赖它两端的实体类型已被接受**，
//! 否则会产生引用了不存在实体的野指针。
//!
//! 验收 AC-007。

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 审核轨
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Track {
    /// 业务轨：客户审语义正确性
    Biz,
    /// 建模轨：FDE 审建模质量（主键、基数、拆分合并）
    Model,
}

/// 单轨的判定
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Pending,
    Accepted,
    /// 显式拒绝 —— **永远否决**，任何轨的显式拒绝都不能被另一轨的通过覆盖
    Rejected,
}

impl Decision {
    /// 稳定的 ASCII 名（存库用）。**不随 enum 变体名漂移。**
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Pending => "pending",
            Decision::Accepted => "accepted",
            Decision::Rejected => "rejected",
        }
    }
}

/// 条目类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    AddEntityType,
    AddProperty,
    AddLinkType,
    RemoveEntityType,
    ModifyEntityType,
    /// REQ-010 · **新增业务概念组**（§7.1 阶段 3）。
    ///
    /// 为什么它和实体类型一样是 ProposalItem、而不是「配置文件直接生效」：
    /// 概念组是**对客户可见的语义声明**。「`package_templates` /
    /// `package_products` / `package_benefits` 是同一个概念」这句话如果绕过
    /// 审核直接生效，客户看到的页面就变了却没人负责 —— 那是 §2.2 说的
    /// 「让提议者自己决定」。它与 `AddEntityType` 同轨同权：两轨都明确通过
    /// 才可 apply，任一轨显式拒绝即否决。
    AddConceptGroup,
}

impl ItemKind {
    /// 稳定的 ASCII 名 —— 用作 id 前缀，**不随 enum 变体名变化而漂移**。
    /// （早期用 `format!("{:?}", kind)` 会产生 `AddEntityType` 而非 `add_entity_type`）
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::AddEntityType => "add_entity_type",
            ItemKind::AddProperty => "add_property",
            ItemKind::AddLinkType => "add_link_type",
            ItemKind::RemoveEntityType => "remove_entity_type",
            ItemKind::ModifyEntityType => "modify_entity_type",
            ItemKind::AddConceptGroup => "add_concept_group",
        }
    }

    /// 是否为**关系类** —— 这类条目依赖两端实体已被接受
    pub fn is_relational(self) -> bool {
        matches!(self, ItemKind::AddLinkType)
    }

    /// 是否为**概念组类**（REQ-010）—— 依赖它的**每个成员**都已被接受。
    ///
    /// 与关系类的区别：关系只有两端，概念组有 N 个成员。判定逻辑一样，
    /// 只是「两端」换成了「成员集合」。
    pub fn is_concept_group(self) -> bool {
        matches!(self, ItemKind::AddConceptGroup)
    }

    /// 是否为**新增类** —— 只有它能让新类型进入本提案的可用集合
    pub fn is_creating_type(self) -> bool {
        matches!(self, ItemKind::AddEntityType)
    }

    /// 是否为**删除类** —— 通过后要把该类型从可用集合里扣掉，
    /// 且**本提案内任何关系都不得指向它**
    pub fn is_removing_type(self) -> bool {
        matches!(self, ItemKind::RemoveEntityType)
    }
}

/// 提案条目（审核的输入）。**不碰数据库** —— 存储层负责把行转成它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemView {
    pub id: String,
    pub kind: ItemKind,
    /// 条目描述的类型 key（关系类时为 `from→to` 的展示名）
    pub target: String,
    /// 关系类的两端类型 key；其他类为空
    pub from_type: Option<String>,
    pub to_type: Option<String>,
    pub biz_status: Decision,
    pub model_status: Decision,
    /// 条目的完整候选定义（含 evidence）。
    ///
    /// **审核状态机本身不读它**（只看 kind / target / 两端 / 双轨状态），
    /// 但 `apply` 需要它来构造新版本 —— 早期版本把它留在存储层，
    /// 结果 apply 要么拿不到 payload、要么被迫回调存储层，两头都别扭。
    /// 放在这里 = 条目是**自足**的：审核和 apply 用同一个结构。
    #[serde(default)]
    pub payload: serde_json::Value,
}

impl ItemView {
    /// 这个条目引入/作用的那个类型的 **key**（不是展示名）。
    ///
    /// # 为什么必须有这个方法
    ///
    /// 条目的 `target` 是**给人看的展示名**（「客户」），
    /// 而关系类条目的 `from_type` / `to_type` 来自 payload，是**系统 key**（`customer`）。
    /// 早期版本把 `target` 直接塞进 `available` 集合，
    /// 于是集合里是「客户」、查的时候用 `customer` —— **两个命名空间混了**，
    /// 关系全被判成悬空。端到端跑起来才发现：单测里两边恰好都用中文名，测不出来。
    ///
    /// 统一到 **key 命名空间**：`available` 存 key，两端用 key 查。
    pub fn type_key(&self) -> String {
        self.payload
            .get("key")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| self.target.clone())
    }
}

/// 某条目当前所处的状态（`pending` 的不参与判定）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryState {
    /// 至少一轨仍是 pending —— 未决
    Pending,
    /// 两轨都通过
    Passed,
    /// 任一轨显式拒绝 —— **否决**
    Vetoed,
}

/// 单条目的判定结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemVerdict {
    pub id: String,
    pub target: String,
    pub state: EntryState,
    /// 被拒的原因（仅 `Vetoed` 时有值）
    pub reason: Option<String>,
}

/// 提案整体状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalStatus {
    /// 整体是否通过（可进入 apply）
    pub passed: bool,
    /// 逐条判定
    pub verdicts: Vec<ItemVerdict>,
    /// 可直接应用的条目 id
    pub applicable: Vec<String>,
    /// 被依赖关系拦下的条目
    pub blocked: Vec<ItemVerdict>,
    /// 两端依赖不满足的关系类条目
    pub dangling_relations: Vec<String>,
}

/// 单条目的状态判定
pub fn entry_state(item: &ItemView) -> EntryState {
    // 显式拒绝永远否决，与另一轨无关
    if item.biz_status == Decision::Rejected || item.model_status == Decision::Rejected {
        return EntryState::Vetoed;
    }
    // **两轨都必须明确通过。**
    //
    // 规格 §8.2 的「代签」是**谁来写这个决定**的问题（客户可由 FDE 代签，
    // 但必须显式记录「已代签」及依据），**不是「这个决定可以不作」**。
    // 早期版本误把 `model_status == Pending` 当作通过，理由是「POC 允许代签」——
    // 那会让「FDE 还没看」的类型被 apply 出去，违反 §8.2 第 1 条「必须完整执行」。
    if item.biz_status == Decision::Accepted && item.model_status == Decision::Accepted {
        EntryState::Passed
    } else {
        EntryState::Pending
    }
}

/// 汇总提案状态。
///
/// 流程：
/// 1. 逐条判定
/// 2. 收集「本提案内已被接受的新类型」+「基线里已存在的类型」
///    （基线类型由调用方传入 —— 存储层才知道快照里有什么）
/// 3. 关系类条目：两端都必须在可用集合里，否则拦下
pub fn summarize(items: &[ItemView], baseline_types: &BTreeSet<String>) -> ProposalStatus {
    let verdicts: Vec<ItemVerdict> = items
        .iter()
        .map(|it| {
            let st = entry_state(it);
            let reason = if st == EntryState::Vetoed {
                Some(if it.biz_status == Decision::Rejected {
                    "业务轨显式拒绝".to_string()
                } else {
                    "建模轨显式拒绝".to_string()
                })
            } else {
                None
            };
            ItemVerdict { id: it.id.clone(), target: it.target.clone(), state: st, reason }
        })
        .collect();

    // 可用类型 = 基线已有的 + 本提案内已通过的新增类型 − 本提案内已通过的删除目标
    //
    // 早期版本只做加法不做减法 → 删掉「订单」后，指向它的链接仍判定可用，
    // apply 之后就留下**引用了不存在实体的野指针**（AC-007 明令要防的）。
    //
    // **一律用 `type_key()`（系统 key）而非 `target`（展示名）** ——
    // 关系两端从 payload 取的是 key，命名空间必须一致（见 `type_key` 的说明）。
    let mut available: BTreeSet<String> = baseline_types.clone();
    for it in items {
        if entry_state(it) != EntryState::Passed {
            continue;
        }
        let key = it.type_key();
        if it.kind.is_creating_type() {
            available.insert(key);
        } else if it.kind.is_removing_type() {
            available.remove(&key);
        }
    }

    let mut applicable = Vec::new();
    let mut blocked = Vec::new();
    let mut dangling_relations = Vec::new();

    for (it, v) in items.iter().zip(verdicts.iter()) {
        match v.state {
            EntryState::Vetoed | EntryState::Pending => {
                // 未通过的不算 blocked —— blocked 特指「本该能用但被依赖拦下」
            }
            EntryState::Passed => {
                if it.kind.is_relational() {
                    let ends_ok = match (&it.from_type, &it.to_type) {
                        (Some(f), Some(t)) => {
                            available.contains(f) && available.contains(t)
                        }
                        // 没给两端 = 声明不完整，按拦下处理
                        _ => false,
                    };
                    if ends_ok {
                        applicable.push(it.id.clone());
                    } else {
                        let missing: Vec<&str> = match (&it.from_type, &it.to_type) {
                            (Some(f), Some(t)) => [f.as_str(), t.as_str()]
                                .into_iter()
                                .filter(|x| !available.contains(*x))
                                .collect(),
                            _ => vec!["(未声明两端)"],
                        };
                        dangling_relations.push(it.id.clone());
                        blocked.push(ItemVerdict {
                            id: it.id.clone(),
                            target: it.target.clone(),
                            state: EntryState::Pending,
                            reason: Some(format!(
                                "关系类条目依赖两端已被接受，但 {} 尚未可用",
                                missing.join("、")
                            )),
                        });
                    }
                } else if it.kind.is_concept_group() {
                    // REQ-010：概念组的每个成员都必须已被接受。
                    //
                    // 为什么不放在 `domain::apply` 才查：apply 是**最后一道闸**，
                    // 而审核期就拦住才能让业务人员看到「你这条概念组还差一个对象」
                    // 而不是 apply 失败后重头再来。理由①见 `domain::concept`。
                    let members = concept_members(it);
                    let missing: Vec<String> = members
                        .iter()
                        .filter(|m| !available.contains(*m))
                        .cloned()
                        .collect();
                    if members.is_empty() {
                        // 退化输入：没有成员的组在总览里是一张空卡
                        blocked.push(ItemVerdict {
                            id: it.id.clone(),
                            target: it.target.clone(),
                            state: EntryState::Pending,
                            reason: Some("概念组没有声明任何成员".to_string()),
                        });
                    } else if !missing.is_empty() {
                        blocked.push(ItemVerdict {
                            id: it.id.clone(),
                            target: it.target.clone(),
                            state: EntryState::Pending,
                            reason: Some(format!(
                                "概念组依赖全部成员已被接受，但 {} 尚未可用",
                                missing.join("、")
                            )),
                        });
                    } else {
                        applicable.push(it.id.clone());
                    }
                } else {
                    applicable.push(it.id.clone());
                }
            }
        }
    }

    // 整体通过 = 三条都要满足
    //
    //   ① 提案非空且有可应用条目
    //   ② **没有悬空关系** —— 早期版本漏了这一条，导致
    //      「1 条可用 + 1 条悬空」时自报 passed=true；
    //      而 `passed` 的文档契约是「可进入 apply」，
    //      调用方以它为闸门就会在悬空状态下执行（AC-007 的拦截意图落空）
    //   ③ 所有条目都明确通过
    let passed = !items.is_empty()
        && !applicable.is_empty()
        && dangling_relations.is_empty()
        && blocked.is_empty()
        && items.iter().all(|it| entry_state(it) == EntryState::Passed);

    ProposalStatus { passed, verdicts, applicable, blocked, dangling_relations }
}

/// 记录一次单轨判定，返回新条目（不可变更新）。
pub fn apply_decision(item: &ItemView, track: Track, decision: Decision) -> ItemView {
    let mut next = item.clone();
    match track {
        Track::Biz => next.biz_status = decision,
        Track::Model => next.model_status = decision,
    }
    next
}

/// 提案内被显式拒绝的条目数 —— 供 UI 提示「有几条被否」。
pub fn veto_count(items: &[ItemView]) -> usize {
    items.iter().filter(|it| entry_state(it) == EntryState::Vetoed).count()
}

/// 概念组条目的成员 key 列表（从 `payload.members` 取）。
///
/// **认不出 `members` 就给空 vec** —— 空 vec 会被 [`summarize`] 当
/// 「没有声明任何成员」拦下，而不是当成「成员集合为空、没问题」。
/// 这是本项目一贯的方向：解析失败要**显式暴露**，不许静默当默认值。
fn concept_members(it: &ItemView) -> Vec<String> {
    it.payload
        .get("members")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}
