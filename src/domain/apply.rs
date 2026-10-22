//! 提案应用 —— **纯变换，零 I/O**（AC-002）。
//!
//! 输入：base 的快照 + 提案里**可应用**的条目。
//! 输出：新版本的快照。
//!
//! 存储与事务由 `db::apply` 负责；本文件只管「怎么变」。
//! 拆开的好处：apply 的正确性可以脱离数据库单测。

use crate::domain::review::{EntryState, ItemKind, ItemView};
use crate::domain::snapshot::{EntityType, Evidence, LinkType, Property, Snapshot};
use uuid::Uuid;

/// apply 的输入。
#[derive(Debug, Clone)]
pub struct ApplyInput<'a> {
    /// base 版本的快照
    pub base: &'a Snapshot,
    /// 提案的 `extracted_by`。**`None` = 人工修订**
    pub proposal_extracted_by: Option<crate::domain::snapshot::ExtractedBy>,
    /// 全部条目（函数自己算哪些可应用，不信任调用方的筛选）
    pub items: &'a [ItemView],
    /// base 版本里已存在的类型 key —— 关系类校验用
    pub baseline_types: &'a std::collections::BTreeSet<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ApplyError {
    #[error("提案未通过审核（blocked {blocked} 条，悬空关系 {dangling} 条），不能 apply")]
    NotPassed { blocked: usize, dangling: usize },

    #[error("没有可应用的条目")]
    NothingApplicable,

    #[error("key 冲突：{0} 已存在于 base 版本")]
    KeyConflict(String),

    #[error("关系类条目两端不存在：{0}")]
    DanglingLink(String),

    #[error("删除了不存在的类型：{0}")]
    RemovingMissing(String),

    /// REQ-010 · 概念声明不合法（悬空成员 / 嵌套 / 重叠 / 空组 / key 重复）
    #[error("概念组声明不合法：{0}")]
    BadConceptGroup(String),

    /// REQ-108 · 关系条目的 `confidence` 不合法（缺 method / 分数越界 / band 与分数对不上）。
    ///
    /// **fail-closed**：`import` 期已经拦过一道（AC-008），这里是第二条通道
    /// （`payload` 是 JSONB，理论上可以被别的工具改过）。宁可 apply 失败，
    /// 也不要把一份读不懂的分数带进冻结版本。
    #[error("关系条目的 confidence 不合法：{0}")]
    BadConfidence(String),
}

/// 逐条应用，返回新快照。
///
/// # ADR-001：`extracted_by` 的取值
///
/// **新版本原样取提案的值；提案没有则记 `None`（人工修订）。**
/// **不继承 base，不推断，不按场景分支。**
///
/// 理由：继承会让「agent 偷偷换版本」不可见 —— 而那正是规格 §8.4 要防的问题。
/// 这个函数里**没有任何一行读 `base.extracted_by`**，那是刻意的。
pub fn apply(input: ApplyInput<'_>) -> Result<Snapshot, ApplyError> {
    let st = crate::domain::review::summarize(input.items, input.baseline_types);
    if !st.passed {
        return Err(ApplyError::NotPassed {
            blocked: st.blocked.len(),
            dangling: st.dangling_relations.len(),
        });
    }
    if st.applicable.is_empty() {
        return Err(ApplyError::NothingApplicable);
    }

    // 从 base 深拷贝一份，逐条改
    let mut out = input.base.clone();
    out.version_id = Uuid::new_v4(); // 调用方会覆盖成新版本 id
    out.seq = input.base.seq + 1;

    // ADR-001：原样取提案的值，**不看 base**
    out.extracted_by = input.proposal_extracted_by.clone();

    let applicable: std::collections::BTreeSet<String> = st.applicable.iter().cloned().collect();

    // 先建 key 索引
    let mut keys: std::collections::BTreeSet<String> =
        out.entity_types.iter().map(|e| e.key.clone()).collect();

    for it in input.items {
        if !applicable.contains(&it.id) {
            continue;
        }
        match it.kind {
            ItemKind::AddEntityType => {
                let ekey = it.type_key();
                if keys.contains(&ekey) {
                    return Err(ApplyError::KeyConflict(ekey));
                }
                let e = entity_from_payload(&ekey, &it.payload)?;
                keys.insert(e.key.clone());
                out.entity_types.push(e);
            }
            ItemKind::AddProperty => {
                let target = &it.payload
                    .get("parent_key")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&it.target)
                    .to_string();
                let prop = property_from_payload(&it.payload)?;
                let t = out
                    .entity_types
                    .iter_mut()
                    .find(|e| e.key == *target)
                    .ok_or_else(|| ApplyError::RemovingMissing(target.clone()))?;
                if t.properties.iter().any(|p| p.column == prop.column) {
                    return Err(ApplyError::KeyConflict(format!("{target}.{}", prop.column)));
                }
                t.properties.push(prop);
            }
            ItemKind::AddLinkType => {
                let p = &it.payload;
                let from = p.get("from_type").and_then(|v| v.as_str()).unwrap_or_default();
                let to = p.get("to_type").and_then(|v| v.as_str()).unwrap_or_default();
                if !keys.contains(from) || !keys.contains(to) {
                    return Err(ApplyError::DanglingLink(format!("{from}→{to}")));
                }
                let l = link_from_payload(&it.target, p)?;
                if out.link_types.iter().any(|x| x.key == l.key) {
                    return Err(ApplyError::KeyConflict(l.key.clone()));
                }
                out.link_types.push(l);
            }
            ItemKind::AddConceptGroup => {
                // REQ-010 · 概念组是**展示层声明**：
                // 这里**只往 concept_groups 追加**，不碰 entity_types /
                // link_types / properties —— 红线，见 domain::concept 模块文档。
                // 同一个 key 声明两次 = 重复声明，拒（否则 diff 会读成
                // 「删了旧的、加了新的」，而它们其实是同一个概念）。
                let key = it.type_key();
                if out.concept_groups.iter().any(|c| c.key == key) {
                    return Err(ApplyError::KeyConflict(format!("概念组 {key}")));
                }
                let c = crate::domain::concept::group_from_payload(&key, &it.payload)
                    .ok_or_else(|| ApplyError::BadConceptGroup(format!("{key}：payload 缺 members")))?;
                out.concept_groups.push(c);
            }
            ItemKind::RemoveEntityType => {
                // 用 type_key（系统 key），不是 target（展示名）——
                // 与 summarize 的 available 集合保持同一命名空间
                let key = it.type_key();
                if !keys.contains(&key) {
                    return Err(ApplyError::RemovingMissing(key));
                }
                // 连带删掉指向它的关系 —— 否则留下野指针
                out.link_types
                    .retain(|l| l.from_type != key && l.to_type != key);
                // 引用它的子类型「提根」，不连带删（规格 §4.6）
                for e in out.entity_types.iter_mut() {
                    if e.parent.as_deref() == Some(key.as_str()) {
                        e.parent = None;
                    }
                }
                out.entity_types.retain(|e| e.key != key);
                out.action_types.retain(|a| a.target != key);
                keys.remove(&key);
            }
            ItemKind::ModifyEntityType => {
                let p = &it.payload;
                let key = it.type_key();
                let t = out
                    .entity_types
                    .iter_mut()
                    .find(|e| e.key == key)
                    .ok_or_else(|| ApplyError::RemovingMissing(key))?;
                if let Some(v) = p.get("display_name").and_then(|v| v.as_str()) {
                    t.display_name = v.to_string();
                }
                if let Some(v) = p.get("description") {
                    t.description = v.as_str().map(String::from);
                }
                if let Some(arr) = p.get("aliases").and_then(|v| v.as_array()) {
                    t.aliases = arr.iter().filter_map(|x| x.as_str().map(String::from)).collect();
                }
                if p.get("parent").is_some() {
                    t.parent = p.get("parent").and_then(|v| v.as_str()).map(String::from);
                }
            }
        }
    }

    // 层级自检：apply 完不得有环 / 悬空父。
    // 这是最后一道闸 —— 提案审核只查关系类，层级是另一码事。
    let parents = out.parents();
    let problems = crate::domain::hierarchy::validate_hierarchy(&parents);
    if !problems.is_empty() {
        return Err(ApplyError::NotPassed {
            blocked: problems.len(),
            dangling: 0,
        });
    }

    // REQ-010 · 概念组自检 —— **最后一道闸**，与层级自检同一位置、同一理由：
    // 审核期只查「每条声明自己的成员是否都已被接受」，**跨条目的**问题
    // （两个组抢同一个成员、同一个 key 声明两次、把概念当成员、
    // 组 key 恰好与实体 key 撞名）只有把全部条目应用完之后才看得出来。
    //
    // 诚实说明：悬空成员与「组 key 不是实体」这两种嵌套在正常路径上
    // 审核期就拦下了（走的是同一条「成员不可用」通路），这里拦的是
    // 审核期看不见的那几种 —— 包括**调用方给的 baseline 与快照对不上**
    // 这种纵深防御场景。
    //
    // 判据取自**结果快照的实体集合**而不是 base —— 因为概念组可以引用
    // 本提案内刚被接受的新实体，那正是它该做的事。
    let keys: std::collections::BTreeSet<String> =
        out.entity_types.iter().map(|e| e.key.clone()).collect();
    let cg_problems = crate::domain::concept::validate_groups(&out.concept_groups, &keys);
    if !cg_problems.is_empty() {
        return Err(ApplyError::BadConceptGroup(
            cg_problems.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("；"),
        ));
    }

    Ok(out)
}

// ── payload → 领域对象 ──────────────────────────────────
//
// 提案里的 payload 是**不完整**的（抽取器可能只给关键字段），
// 缺失项用默认值补齐 —— 宁可字段少，不要 apply 失败。

fn entity_from_payload(key: &str, p: &serde_json::Value) -> Result<EntityType, ApplyError> {
    let obj = p.as_object().ok_or_else(|| ApplyError::KeyConflict(key.to_string()))?;
    Ok(EntityType {
        key: obj.get("key").and_then(|v| v.as_str()).unwrap_or(key).to_string(),
        display_name: obj
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or(key)
            .to_string(),
        description: obj.get("description").and_then(|v| v.as_str()).map(String::from),
        aliases: obj
            .get("aliases")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        parent: obj.get("parent").and_then(|v| v.as_str()).map(String::from),
        disjoint_with: obj
            .get("disjoint_with")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        properties: obj
            .get("properties")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|p| property_from_payload(p).ok()).collect())
            .unwrap_or_default(),
        evidence: vec![],
    })
}


fn property_from_payload(p: &serde_json::Value) -> Result<Property, ApplyError> {
    let obj = p.as_object().ok_or_else(|| ApplyError::KeyConflict("property".into()))?;
    Ok(Property {
        name: obj.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        column: obj
            .get("column")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApplyError::KeyConflict("property.column 缺失".into()))?
            .to_string(),
        nullable: obj.get("nullable").and_then(|v| v.as_bool()).unwrap_or(true),
        note: obj.get("note").and_then(|v| v.as_str()).map(String::from),
        sample: obj.get("sample").and_then(|v| v.as_str()).map(String::from),
    })
}


fn link_from_payload(key: &str, p: &serde_json::Value) -> Result<LinkType, ApplyError> {
    let obj = p.as_object().ok_or_else(|| ApplyError::KeyConflict(key.to_string()))?;
    Ok(LinkType {
        key: obj.get("key").and_then(|v| v.as_str()).unwrap_or(key).to_string(),
        from_type: obj
            .get("from_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApplyError::DanglingLink(key.to_string()))?
            .to_string(),
        to_type: obj
            .get("to_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApplyError::DanglingLink(key.to_string()))?
            .to_string(),
        cardinality: obj
            .get("cardinality")
            .and_then(|v| v.as_str())
            .unwrap_or("1:N")
            .to_string(),
        semantic: obj.get("semantic").and_then(|v| v.as_str()).unwrap_or("has").to_string(),
        // ⭐ REQ-120 · **关系侧证据原来是丢的**（这里写死 `vec![]`）。
        //
        // 后果不是「少一栏说明」，而是建模视图拿不到「这条关系由哪一列实现」：
        // `ModelLinkView.column` 的唯一来源就是它（`evidence[0].column`）。
        // 没有它，同一对 (from, to) 的两条边（`customer_account_binding` 的
        // `main_customer_id` / `sub_customer_id` 都指向 `customers`）在 TTL 里
        // 会写成**同一个 IRI** —— 两条 `owl:inverseOf` 挤在一个主语上。
        //
        // 读不懂的条目**不丢整条关系**：只保留能读出 `table/column/claim` 的项。
        // （与 `confidence` 的 fail-closed 不同：证据是**附加信息**，
        //   缺了它关系本身仍然成立；而分数读不懂时「这一版到底多可信」是无解的。）
        evidence: obj
            .get("evidence")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(evidence_from_json).collect())
            .unwrap_or_default(),
        // REQ-108 · 置信度随条目进快照（`--model-view` 要显示它，客户视图**不读**它）。
        //
        // 缺 `confidence` ⇒ `None`（人工写的提案 / 旧提案）。
        // 有但读不懂 ⇒ 报错，**不**静默降级成 `None`：
        // 「有一份读不懂的分数」与「没有分数」在审核界面上的含义完全不同。
        confidence: match obj.get("confidence") {
            None | Some(serde_json::Value::Null) => None,
            Some(v) => Some(
                crate::domain::confidence::from_untrusted(v)
                    .map_err(ApplyError::BadConfidence)?,
            ),
        },
        // REQ-120 · 连接验证（AC-007）：**提案说什么就落什么**，三态原样透传。
        //
        // `join_verified` 缺席 ⇒ `None`（手写提案 / REQ-120 之前的提案）——
        // ⛔ 不默认成 `false`：「没说过」与「说过是命名推断」是两件事。
        // 但给了却读不懂（不是布尔）⇒ **丢弃整份提案**是本模块既有的
        // `BadConfidence` 同款纪律吗？不是 —— 这里**不**新增错误变体：
        // 一个非布尔的 `join_verified` 只可能是手写笔误，把它当 `None`
        // 会让 TTL 少一行标注（可见）、而拒绝整份提案会让 96 张表白抽一遍
        // （代价不对等）。所以取 `as_bool()`，读不出就是 `None`。
        join_verified: obj.get("join_verified").and_then(|v| v.as_bool()),
        join_evidence: obj
            .get("join_evidence")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// 提案条目里的**一条证据** → 快照 [`Evidence`]（缺字段 ⇒ `None`，不是空串占位）。
///
/// 为什么不用空串兜底：`{table: "", column: "", claim: ""}` 会让「这条关系
/// 有 1 条证据」变成一句没有任何信息的断言 —— 与「没有证据」必须可区分。
fn evidence_from_json(v: &serde_json::Value) -> Option<Evidence> {
    let obj = v.as_object()?;
    let table = obj.get("table").and_then(|x| x.as_str())?;
    let column = obj.get("column").and_then(|x| x.as_str())?;
    let claim = obj.get("claim").and_then(|x| x.as_str())?;
    if table.is_empty() || column.is_empty() || claim.is_empty() {
        return None;
    }
    Some(Evidence {
        table: table.to_string(),
        column: column.to_string(),
        claim: claim.to_string(),
    })
}

/// 条目是否两轨都通过。
pub fn is_fully_accepted(it: &ItemView) -> bool {
    crate::domain::review::entry_state(it) == EntryState::Passed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_refuses_when_not_passed() {
        // 0 个条目 → summarize 判 not passed
        let base = Snapshot::new(Uuid::nil(), 1);
        let items: Vec<ItemView> = vec![];
        let bt = std::collections::BTreeSet::new();
        let r = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &items,
            baseline_types: &bt,
        });
        assert!(matches!(r, Err(ApplyError::NotPassed { .. })));
    }
}
