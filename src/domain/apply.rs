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

    /// ⭐ REQ-129 · AC-006：条目被标为 `applicable: false` ⇒ **拒绝 apply 并点名**。
    ///
    /// # 为什么是「拒绝」而不是「跳过」
    ///
    /// 跳过等于**默认**处理了它：冲突条目（doc 的基数与 DB 推断不一致）会静默消失，
    /// 而人以为自己审过、以为冲突已经被解决。需求原文是「applier 拿到
    /// `applicable: false` 的条目 ⇒ **拒绝（不是忽略）**，并点名」。
    /// 处置方式是**人的动作**：把冲突两边挑一个，改 `applicable` 或删掉该条目，再重导。
    #[error("条目被标为不可应用（applicable: false），拒绝 apply：{}", .items.join("；"))]
    NotApplicable { items: Vec<String> },

    /// ⭐ REQ-129 · 规则 2 的第二道闸：doc 条目的 `band` 不许是 `strong`。
    ///
    /// 合并器已经把它压到 `medium`（`doc_merge::cap_band`）；这里是**纵深防御** ——
    /// `import` 之后 payload 还能被别的工具改，而「doc 断言冒充强证据」正是
    /// demand 规则 2 要防的事。
    #[error("doc 条目的 band 不许是 strong：{}", .items.join("；"))]
    DocBandStrong { items: Vec<String> },
}

// ──────────────────────────────────────────────────────────
// ⭐ REQ-129 · 文档来源条目的**三条硬判据**（唯一实现，纪律 H）
// ──────────────────────────────────────────────────────────

/// 条目来源字段（`payload.origin`）。**只在 doc merge 的产物里出现。**
pub const ORIGIN_FIELD: &str = "origin";
/// 来源：整条都是文档断言（只可能是关系 —— 概念映射不到表就不并入）。
pub const ORIGIN_DOC: &str = "doc";
/// 来源：doc 与 DB 指向同一实体 / 同一条边。
pub const ORIGIN_BOTH: &str = "both";

/// payload 里的一个非空字符串字段。
fn payload_str<'a>(it: &'a ItemView, key: &str) -> Option<&'a str> {
    it.payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 条目来源（`None` = 不是 doc merge 的产物，走既有语义）。
pub fn origin_of(it: &ItemView) -> Option<&str> {
    payload_str(it, ORIGIN_FIELD)
}

/// 这条条目是否来自文档合并（`doc` 或 `both`）。
pub fn is_doc_origin(it: &ItemView) -> bool {
    matches!(origin_of(it), Some(ORIGIN_DOC) | Some(ORIGIN_BOTH))
}

/// 条目是否可被应用（**缺省 = 可**：普通提案没有这一栏）。
pub fn applicable_of(it: &ItemView) -> bool {
    it.payload
        .get("applicable")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// 谁签的字（`None` = 未签）。doc 条目的签名由**评审流程**写进提案文件，
/// 合并器只写 `null`（`doc_merge` 的规则 2）。
pub fn reviewed_by_of(it: &ItemView) -> Option<&str> {
    payload_str(it, "reviewed_by")
}

/// 置信档位（`low` / `medium`；doc 条目**不许** `strong`）。
pub fn band_of(it: &ItemView) -> Option<&str> {
    payload_str(it, "band")
}

/// ⭐ 规则 7：**未签的 doc-only 条目** —— apply 时忽略（返回 `(id, target)` 供点名）。
///
/// # 为什么只忽略 `doc`、不忽略 `both`
///
/// `both` 条目的**结构**是 DB 侧的（那张表、那条边本来就该进本体），doc 侧只贡献了
/// `payload.doc_evidence`（审计面）。忽略它会把 DB 自己的实体也丢掉 —— 那不是
/// 「未签不进快照」，那是**未签就丢数据**。`doc` 条目的结构**全部**来自文档，
/// 所以它必须先有人签字。
pub fn unsigned_doc_only(items: &[ItemView]) -> Vec<(String, String)> {
    items
        .iter()
        .filter(|it| origin_of(it) == Some(ORIGIN_DOC) && reviewed_by_of(it).is_none())
        .map(|it| (it.id.clone(), it.target.clone()))
        .collect()
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
    // ⭐ REQ-129 · AC-006 的第一条：`applicable: false` ⇒ **拒绝并点名**。
    //
    // 位置刻意在 `summarize` **之前**：这条判据与「审核过没过」无关 ——
    // 一份带冲突条目的提案，即使审核全绿、即使有人把冲突条目也签了，
    // 也**不许**进快照（冲突要人来挑边，不是由「都接受」替人挑）。
    let not_applicable: Vec<String> = input
        .items
        .iter()
        .filter(|it| !applicable_of(it))
        .map(|it| format!("{}（{}）", it.target, it.id))
        .collect();
    if !not_applicable.is_empty() {
        return Err(ApplyError::NotApplicable {
            items: not_applicable,
        });
    }

    // ⭐ REQ-129 · 规则 2 的第二道闸：doc 条目 band = strong ⇒ 拒绝（fail-closed）。
    let doc_band_strong: Vec<String> = input
        .items
        .iter()
        .filter(|it| is_doc_origin(it) && band_of(it) == Some("strong"))
        .map(|it| format!("{}（{}）", it.target, it.id))
        .collect();
    if !doc_band_strong.is_empty() {
        return Err(ApplyError::DocBandStrong {
            items: doc_band_strong,
        });
    }

    let st = crate::domain::review::summarize(input.items, input.baseline_types);
    if !st.passed {
        return Err(ApplyError::NotPassed {
            blocked: st.blocked.len(),
            dangling: st.dangling_relations.len(),
        });
    }
    // ⭐ REQ-129 · 规则 7：未签的 doc-only 条目**不进快照**（忽略，但点名在报告里）。
    let ignored: std::collections::BTreeSet<String> = unsigned_doc_only(input.items)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    if st.applicable.is_empty() || st.applicable.iter().all(|id| ignored.contains(id)) {
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
        // ⭐ REQ-129 · 规则 7：未签的 doc-only 条目 ⇒ 忽略（不进快照）。
        if ignored.contains(&it.id) {
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
        // | 提案里 | 快照里 |
        // | --- | --- |
        // | `"true"` / `"inherit"` / `"false"`（字符串，当前写法） | 对应三态 |
        // | `true` / `false`（布尔，rev.1 的写法 / 手写提案） | `Verified` / `NamingOnly` |
        // | 缺席 / `null` | `None`（**不**默认成 `"false"`） |
        // | 认不出的字符串 | `None`（不猜；TTL 少一行标注是**可见**的失败） |
        join_verified: join_state_from_payload(obj.get("join_verified")),
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
fn join_state_from_payload(v: Option<&serde_json::Value>) -> Option<crate::domain::snapshot::JoinState> {
    use crate::domain::snapshot::JoinState;
    match v? {
        serde_json::Value::String(s) => JoinState::parse(s),
        serde_json::Value::Bool(true) => Some(JoinState::Verified),
        serde_json::Value::Bool(false) => Some(JoinState::NamingOnly),
        _ => None,
    }
}

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

    // ── REQ-129 · 文档合并条目的三条硬判据（AC-006 / 规则 2 / 规则 7）──

    /// 带两张表（`order` / `customers`）的基线快照。
    fn base_with_two_entities() -> Snapshot {
        let mut s = Snapshot::new(Uuid::nil(), 1);
        for (key, name) in [("order", "订单"), ("customers", "客户")] {
            s.entity_types.push(EntityType {
                key: key.into(),
                display_name: name.into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            });
        }
        s
    }

    /// 一条 `add_link_type`：`order.customer_id → customers`。
    fn doc_link(payload_extra: serde_json::Value) -> ItemView {
        let mut payload = serde_json::json!({
            "key": "order_customer_id__customers",
            "from_type": "order",
            "to_type": "customers",
            "cardinality": "N:1",
            "semantic": "下单",
            "evidence": [{"table": "order", "column": "customer_id", "claim": "文档断言"}],
        });
        if let Some(extra) = payload_extra.as_object() {
            for (k, v) in extra {
                payload[k] = v.clone();
            }
        }
        ItemView {
            id: "1:add_link_type:订单→客户".into(),
            kind: crate::domain::review::ItemKind::AddLinkType,
            target: "订单→客户".into(),
            from_type: Some("order".into()),
            to_type: Some("customers".into()),
            biz_status: crate::domain::review::Decision::Accepted,
            model_status: crate::domain::review::Decision::Accepted,
            payload,
        }
    }

    fn baseline_keys() -> std::collections::BTreeSet<String> {
        ["order".to_string(), "customers".to_string()].into_iter().collect()
    }

    /// ⭐ AC-006：`applicable: false` ⇒ applier **拒绝并点名**（不是忽略）。
    ///
    /// 变异自证（AC-006 的「把 applicable 去掉」）：去掉这一栏，本测试的第一段
    /// 就不再报 `NotApplicable` —— 冲突条目会静默进本体，红。
    #[test]
    fn applier_rejects_an_inapplicable_item_and_names_it() {
        let base = base_with_two_entities();
        let items = vec![doc_link(serde_json::json!({
            "origin": "both",
            "band": "medium",
            "reviewed_by": null,
            "applicable": false,
            "conflicts": [{"doc_says": "1:1", "db_says": "N:1"}],
        }))];
        let r = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &items,
            baseline_types: &baseline_keys(),
        });
        match r {
            Err(ApplyError::NotApplicable { items }) => {
                assert_eq!(items.len(), 1);
                assert!(
                    items[0].contains("订单→客户"),
                    "报错必须**点名**：{}",
                    items[0]
                );
            }
            other => panic!("applicable: false 必须被**拒绝**（不是忽略），实际：{other:?}"),
        }

        // 对照：把 applicable 去掉 ⇒ 不再走到拒绝分支（这条对照证明上一段判的是这一栏）。
        let ok_item = doc_link(serde_json::json!({"origin": "doc", "reviewed_by": "qa"}));
        let r2 = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &[ok_item],
            baseline_types: &baseline_keys(),
        });
        assert!(r2.is_ok(), "没有 applicable 栏 ⇒ 可应用：{:?}", r2.err());
    }

    /// ⭐ 规则 2 的第二道闸：doc 条目 `band = strong` ⇒ 拒绝。
    #[test]
    fn applier_rejects_a_doc_item_with_band_strong() {
        let base = base_with_two_entities();
        let items = vec![doc_link(serde_json::json!({
            "origin": "doc",
            "band": "strong",
            "reviewed_by": "qa",
        }))];
        let r = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &items,
            baseline_types: &baseline_keys(),
        });
        match r {
            Err(ApplyError::DocBandStrong { items }) => {
                assert!(items[0].contains("订单→客户"), "{}", items[0])
            }
            other => panic!("doc 条目 band=strong 必须被拒绝，实际：{other:?}"),
        }
    }

    /// ⭐ AC-007 的**纯函数版**：未签的 doc-only 条目 ⇒ 不进快照；签一条 ⇒ 进去。
    ///
    /// 这条判据不碰库也不碰 TTL，所以它比端到端更早把「签/不签的差别」钉死。
    /// 端到端那一遍（`tests/test_req129_doc_merge.rs`）证的是 TTL 里的字节。
    #[test]
    fn unsigned_doc_items_are_ignored_and_signed_ones_are_applied() {
        let base = base_with_two_entities();
        let bt = baseline_keys();
        // 提案里还有一条**与文档无关**的条目：apply 因此不会因为「全被忽略」而报空。
        let plain = ItemView {
            id: "2:add_entity_type:活动".into(),
            kind: crate::domain::review::ItemKind::AddEntityType,
            target: "活动".into(),
            from_type: None,
            to_type: None,
            biz_status: crate::domain::review::Decision::Accepted,
            model_status: crate::domain::review::Decision::Accepted,
            payload: serde_json::json!({"key": "activity", "display_name": "活动"}),
        };

        // ① 未签 ⇒ 忽略（快照里没有这条边，但新增的实体照常进）
        let unsigned = vec![
            plain.clone(),
            doc_link(serde_json::json!({
                "origin": "doc",
                "band": "medium",
                "reviewed_by": null,
            })),
        ];
        let snap = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &unsigned,
            baseline_types: &bt,
        })
        .expect("未签条目被忽略后仍应有一次成功的 apply");
        assert!(
            snap.entity("activity").is_some(),
            "与 doc 无关的条目必须照常应用"
        );
        assert!(
            snap.link_types.is_empty(),
            "未签的 doc 条目不许进快照：{:?}",
            snap.link_types
        );

        // ② 签一条 ⇒ 应用（快照里出现这条边，语义是文档里的谓词）
        let signed = vec![
            plain,
            doc_link(serde_json::json!({
                "origin": "doc",
                "band": "medium",
                "reviewed_by": "qa-req129",
            })),
        ];
        let snap2 = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &signed,
            baseline_types: &bt,
        })
        .expect("已签的 doc 条目应当被应用");
        assert_eq!(snap2.link_types.len(), 1, "签了就该进快照");
        assert_eq!(snap2.link_types[0].key, "order_customer_id__customers");
        assert_eq!(snap2.link_types[0].semantic, "下单");
        assert_eq!(snap2.link_types[0].from_type, "order");
        assert_eq!(snap2.link_types[0].to_type, "customers");
    }

    /// 规则 7 的边界：`origin: "both"` **不受**签名闸约束（它的结构来自 DB 侧）。
    #[test]
    fn a_both_item_is_applied_even_when_unsigned() {
        let base = base_with_two_entities();
        let items = vec![doc_link(serde_json::json!({
            "origin": "both",
            "band": "medium",
            "reviewed_by": null,
        }))];
        let snap = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &items,
            baseline_types: &baseline_keys(),
        })
        .expect("both 条目应当被应用");
        assert_eq!(snap.link_types.len(), 1);
        assert!(unsigned_doc_only(&items).is_empty(), "both 不在忽略清单里");
    }

    /// `Notice`：未签 doc-only 条目全被忽略时，**不是**静默成功 —— 报 `NothingApplicable`。
    #[test]
    fn an_all_unsigned_doc_proposal_is_nothing_applicable() {
        let base = base_with_two_entities();
        let items = vec![doc_link(serde_json::json!({
            "origin": "doc",
            "reviewed_by": null,
        }))];
        let r = apply(ApplyInput {
            base: &base,
            proposal_extracted_by: None,
            items: &items,
            baseline_types: &baseline_keys(),
        });
        assert!(
            matches!(r, Err(ApplyError::NothingApplicable)),
            "全被忽略 ⇒ 必须报 NothingApplicable（不产一份与 base 相同的新版本）：{r:?}"
        );
    }
}
