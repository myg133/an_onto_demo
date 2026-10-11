//! **阶段 3+4 · 从候选重建提案**（REQ-022 · 规格 §7.1 阶段 4 的字面做法）。
//!
//! # 这条路径解决的是**物理上产不出提案**这个问题
//!
//! REQ-019 之前的阶段 3+4 是**无条件**跑规则路径，然后把 agent 身份盖到
//! 规则产出的 `items` 上。于是 adapter 想通过能力闸门，
//! **唯一出路是撒谎声明** [`crate::agent::CAPABILITY_PRODUCES_PROPOSAL`] ——
//! 闸门从「防呆」变成了「逼你签字」。
//!
//! 本模块提供**另一条出口**：编排拿各组阶段的候选，
//! **由候选本身**组装出提案条目。声明
//! [`crate::agent::CAPABILITY_REBUILDS_PROPOSAL`] 的实现走这条路。
//!
//! ```text
//! 阶段 0 · 分组    一表一组（编排）
//! 阶段 1 · 抽取    【外部 agent】候选类型 + 属性 + 映射声明 + 关系候选
//!        ↓
//! 阶段 2 · 验证    ★ 纯代码，不可外包（在 orchestrate 里，先于本模块）
//!        ↓
//! 阶段 3 · 跨组合成  合并各组候选（按 key 去重、冲突即报错）、施加 AC-007 自检
//! 阶段 4 · 提案    纯代码：候选 → ProposalItem ← **本模块**
//! ```
//!
//! # ★ 等价性判据：规则 agent 走这条路，产物与规则路径**逐字节相同**
//!
//! 这是本模块存在的**全部理由**。若「从候选重建」与规则路径的产物有任何差别，
//! 那它就不是「第二条可插拔路径」，而是「第二条**会偷偷改提案**的路径」。
//!
//! 判据由 `tests/test_agent_orchestration.rs` 的
//! `test_rule_agent_rebuilt_proposal_is_byte_identical_to_the_rule_pipeline` 钉住：
//! 同一个规则 agent（只多声明一个能力标签）分别走两条路，
//! `proposal` 与 `stats` **逐字节比对**。
//!
//! # 为什么「重建」不是「再跑一遍规则」
//!
//! 重建只做**搬运与取舍**（合并、序列化、AC-007 自检），
//! **不含任何命名判断** —— 名字是 agent 在阶段 1 给的。
//! 规则路径之所以能产出同样的字节，是因为**它给的名字与候选是同一份决策**
//! （`RuleBasedAgent::extract_group` 调的 `propose::build_properties` 与
//! `extract_with_context` 是**同一个函数**，全库只有这一份属性名决策）。
//!
//! # fail-closed：候选承载不下就**报错**，不拼一个残缺的提案
//!
//! 缺 `nullable` / `origin` / `rejected_comments` 时**拒绝重建**。
//! 理由：这三项都会**原样写进产物**（`nullable` 是客户视图的「可空」标记；
//! `origin` 决定 `stats.props_via_*` 三个计数；`rejected_comments` 是 REQ-005
//! 的降级痕迹）。填个默认值发出去 = **发一份数据在说谎的提案**，
//! 而且没人会发现 —— 那正是 REQ-019 到 REQ-022 一路在消灭的那类东西。

use anyhow::{Context, Result};
use std::collections::HashSet;

use super::group::{
    CandidateLink, CandidateProperty, CandidateType, GroupExtraction, PropertyOrigin,
};
use crate::extract::introspect::SchemaReport;
use crate::extract::propose::{
    self, EntityItem, EntityPayload, ExtractionOutcome, ExtractionStats, LinkItem, LinkPayload,
    Proposal, ProposalProvenance, PropertyItem,
};

/// 一个组的阶段 1 产出 —— 带 `group_id`，为了报错时能指名道姓。
#[derive(Debug, Clone)]
pub struct GroupCandidates {
    pub group_id: String,
    pub extraction: GroupExtraction,
}

// ──────────────────────────────────────────────────────────
// 抽取后守卫：这份答案**是否真的**承载了重建契约
// ──────────────────────────────────────────────────────────

// ── ★ 缺口的两类（REQ-024 复审 F2：判据必须是**可测的**，不能靠注释分类）──
//
// 曾经的形状：`contract_gaps` 产出一把 `Vec<String>`，调用方只看
// 「非空 ⇒ 整轮 bail」。那把**答案内容类**缺口也一并处决了 ——
// 而 QA-A 给出的反例是真的：一个重建型实现，某组**合法地**返回 0 个类型
// （其余两组齐备），整轮就崩在那儿。
//
// 「契约缺口是系统性的」是**假设**，不是保证：它对「整个实现压根没有这个字段」
// 成立，对「某个属性缺 nullable / 这条关系缺证据 / 这组答案为空」**不成立**。
// 所以这里按**可观测的形状**分开，而不是按缺项的名字分。

/// 一处缺口属于哪一类 —— 判据是**这份答案自己**能不能证明承载能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapKind {
    /// **结构性**：答案**做了承诺**（产出了实例），却**每一个实例**都缺这个字段。
    ///
    /// 这是实现层面的事实：同一份契约、后面每一组都会以同样方式不合，
    /// 继续跑是把钱花在注定失败的组上 ⇒ 立即 [`anyhow::bail!`]。
    Structural,

    /// **内容类**：承载能力**已被这份答案证明**（有实例填了），只是
    /// 这一组 / 这一项**抽漏了**；或者这一组**一个实例都没有**。
    ///
    /// 后者也算内容类，理由是**它没有做任何承诺**：空答案谈不上「兑现不了承诺」，
    /// 它就是「没抽出来」—— 正是设计点⑤（`ontology-platform-spec.md` ⑤）
    /// 「重试 1–2 次，仍失败则该组标记『抽取失败』，不静默跳过」的那一件事。
    /// ⇒ 降级为**组级失败**：进 `failed_groups`、继续下一组、最终报告显式列出。
    Content,
}

/// 一处缺口 + 它的分类。
#[derive(Debug, Clone)]
pub struct ContractGap {
    pub kind: GapKind,
    /// 人读的说明（`contract_gaps` 仍返回它，判据本身一个字没少）。
    pub text: String,
}

/// **一个字段的承载情况 → 它的缺口属于哪一类**。
///
/// 这是整个分级的**唯一判据**，所以它是可测的（不靠注释、不靠字段名白名单）：
///
/// | `filled` / `applicable` | 判据 | 分类 |
/// | --- | --- | --- |
/// | `0 / n`（n > 0） | **每个**实例都缺 ⇒ 整份答案都没有这个字段 | [`GapKind::Structural`] |
/// | `0 / 0` | 压根没有实例 ⇒ 这份答案没做承诺，只是没抽出来 | [`GapKind::Content`] |
/// | `k / n`（0 < k < n） | 有的填了有的没填 ⇒ 能力已证明，这一项抽漏了 | [`GapKind::Content`] |
fn gap_kind_of(filled: usize, applicable: usize) -> GapKind {
    if applicable > 0 && filled == 0 {
        GapKind::Structural
    } else {
        GapKind::Content
    }
}

/// 给缺口文本**按分类**补一句「为什么是这一类」——分类必须出现在人能读到的地方，
/// 否则线上看到一个内容类缺口降级，没法判断这是设计还是漏判。
fn with_kind_note(text: String, kind: GapKind) -> String {
    match kind {
        GapKind::Structural => format!(
            "{text}\n    〔分类：**结构性** —— 这一份答案里**没有任何一个实例**提供这个字段，\
             所以这是实现层面承载不下，不是某一项抽漏了。整轮停在这里。〕"
        ),
        GapKind::Content => format!(
            "{text}\n    〔分类：**内容类** —— 承载能力已被这份答案证明，\
             这一处只是**这一组/这一项没抽出来**。按设计点⑤记为该组「抽取失败」，不静默跳过、也不整轮处决。〕"
        ),
    }
}

/// 列出这一组候选**承载不下**重建的地方（缺一项，重建就会 bail 或说谎），
/// **并按 [`GapKind`] 分类**。守卫据此分流：结构性 ⇒ `bail!`，内容类 ⇒ 组级失败。
///
/// 分级判据见 [`gap_kind_of`]；为什么必须分级见该函数上方的模块注释。
pub fn classified_contract_gaps(extraction: &GroupExtraction) -> Vec<ContractGap> {
    let mut out: Vec<ContractGap> = Vec::new();

    // 计数口径是**按字段**而不是按缺口：同一个字段在一份答案里可能出现几十次，
    // 「这个字段一次都没出现过」与「这个字段出现了一部分」是两种完全不同的病。
    let props: Vec<&CandidateProperty> = extraction
        .types
        .iter()
        .flat_map(|t| t.properties.iter())
        .collect();

    // ★ 「没有类型」是缺口，**不是**「没有缺口」——
    // 缺这一支时重建会得到 0 个条目，而 report 里有表，那份「0 个实体的提案」
    // 比报错更糟（整轮级后果）。但它归**内容类**：空答案没有做任何承诺，
    // 它是「这一组没抽出来」，按设计点⑤该记组级失败继续跑，
    // 而不是让另外两组已经齐备的成果陪它一起陪葬。
    if extraction.types.is_empty() {
        out.push(ContractGap {
            kind: GapKind::Content,
            text: "这一组没有提出任何类型 —— 无从判断候选是否承载得下提案（**未证明 ≠ 已证明**）。\
             空答案同样重建不出提案：重建会得到 0 个条目，而 report 里有表。"
                .to_string(),
        });
    }

    // `rejected_comments` —— 适用面 = 类型数。
    let rc_applicable = extraction.types.len();
    let rc_filled = extraction
        .types
        .iter()
        .filter(|t| t.rejected_comments.is_some())
        .count();
    for t in &extraction.types {
        if t.rejected_comments.is_none() {
            let kind = gap_kind_of(rc_filled, rc_applicable);
            out.push(ContractGap {
                kind,
                text: with_kind_note(
                    format!(
                        "类型 {:?}：缺 rejected_comments —— 重建路径把它写进 \
                         stats.props_comment_rejected（填 0 = 说谎：那是 REQ-005 质量门槛的降级痕迹，\
                         真实源库那次抽取是 6）",
                        t.key
                    ),
                    kind,
                ),
            });
        }
    }

    // `origin` / `nullable` —— 适用面 = 全部类型的全部属性（跨类型合并计数：
    // 属性是这两个字段唯一的适用对象，分类型计会漏掉「A 类型全有、B 类型全无」这种
    // 只看单类型就看不出来的情况）。
    let org_filled = props.iter().filter(|p| p.origin.is_some()).count();
    let nul_filled = props.iter().filter(|p| p.nullable.is_some()).count();
    for t in &extraction.types {
        for (i, p) in t.properties.iter().enumerate() {
            if p.origin.is_none() {
                let kind = gap_kind_of(org_filled, props.len());
                out.push(ContractGap {
                    kind,
                    text: with_kind_note(
                        format!(
                            "类型 {:?} 的 properties[{i}]（{}）：缺 origin —— 「这个名字是照抄注释、\
                             从关系推的、还是自己造的」，重建路径靠它还原 stats.props_via_*；\
                             代码不替 agent 猜「大概来自注释」",
                            t.key, p.name
                        ),
                        kind,
                    ),
                });
            }
            if p.nullable.is_none() {
                let kind = gap_kind_of(nul_filled, props.len());
                out.push(ContractGap {
                    kind,
                    text: with_kind_note(
                        format!(
                            "类型 {:?} 的 properties[{i}]（{}）：缺 nullable —— 它是**源库的事实**\
                             （阶段 1 的输入里就有 schema 片段），不是 agent 的判断；\
                             填 false 就是编，而编出来的「可空」标记会**原样**出现在客户视图里",
                            t.key, p.name
                        ),
                        kind,
                    ),
                });
            }
        }
    }

    // ★ 关系侧也要看。只遍历 `types` 的判据是**看不见**这里的：
    // `link_item` 逐字复制 `l.evidence` 且不做核对，于是「判据说没问题、
    // 重建照样把证据丢了」这件事会一路静默到产物里。
    let ev_applicable = extraction.links.len();
    let ev_filled = extraction
        .links
        .iter()
        .filter(|l| !l.evidence.is_empty())
        .count();
    for l in &extraction.links {
        if l.evidence.is_empty() {
            let kind = gap_kind_of(ev_filled, ev_applicable);
            out.push(ContractGap {
                kind,
                text: with_kind_note(
                    format!(
                        "关系 {:?}（{}）：缺 evidence —— 规则管线确实会填（propose::links_for_table \
                         从 `*_id` 列推出 table/column/claim 三件），而重建路径把它逐字搬进提案，\
                         **不做核对**；空证据 = 这一条关系在提案里没有任何机器可验证的锚点",
                        l.key, l.target
                    ),
                    kind,
                ),
            });
        }
    }

    out
}

/// 列出这一组候选**承载不下**重建的地方（缺一项，重建就会 bail 或说谎）。
///
/// 判据本身是 [`classified_contract_gaps`]（**唯一一份**判据）；
/// 这里只是丢掉分类、只留文字，供「只看有没有缺口」的老调用方
/// （[`can_rebuild_proposal`]、ACP parse 层的自检）使用。
///
/// # ★ 这个函数是**抽取之后**的守卫，不是能力标签的来源
///
/// 曾经的形状是反的：`capabilities()` 在**抽取之前**读一个由 `extract_group`
/// 填的运行时状态，于是那个 `if` 分支经 [`crate::agent::orchestrate::orchestrate`]
/// 这唯一生产入口**不可达**（闸门先拒，答案后到）。现在两件事拆开：
///
/// | | 职责 | 时机 |
/// | --- | --- | --- |
/// | [`crate::agent::ExtractionAgent::capabilities`] | **静态**：这个实现**是否具备**产出即提案的能力 | 无（纯函数，不随答案翻转）|
/// | 本函数 | 这**一份答案**是否真的承载契约 | **抽取之后** |
///
/// 这个顺序不是风格问题。判据先于数据产生，就只能读一个「还没发生」的状态；
/// 那样的判据要么恒假（今天的形状），要么恒真（下一个更糟的形状）。
///
/// # 判据的适用范围
///
/// - **未声明** `rebuilds_proposal` 的实现：守卫**不适用**（它不走重建路径）；
/// - **声明了**的实现：**每组抽完立刻核对**，不合契约就响亮失败（见
///   [`crate::agent::orchestrate::orchestrate`] 里的守卫调用）。
///
/// 不合契约而放行 = 让「声明了却承载不下」成为常态 —— 那正是
/// `source` 字段栽过的跟头（声明了却没用上），只不过这次是在**另一侧**。
///
/// # 每一条为什么是「承载不下」而不是「差一点」
///
/// | 缺项 | 缺了会怎样 |
/// | --- | --- |
/// | 任何类型 | 空答案重建出 0 个条目，而 report 里有表 —— 那份提案比报错更糟 |
/// | `rejected_comments` | `stats.props_comment_rejected` 写 0（填默认值 = 说谎） |
/// | `origin` | `stats.props_via_*` 三个计数写 0（真实源库 964 个属性里 579 个来自列注释） |
/// | `nullable` | 客户视图的「可空」标记要一个**源库事实**，猜出来的值**没人查得出错** |
/// | `links[].evidence` | `link_item` 逐字搬运、**无 fail-closed 核对** ⇒ 关系侧证据会**静默**丢 |
pub fn contract_gaps(extraction: &GroupExtraction) -> Vec<String> {
    classified_contract_gaps(extraction)
        .into_iter()
        .map(|g| g.text)
        .collect()
}

/// 这一份候选是否**如实**地承载了重建契约（缺口清单为空）。
///
/// 独立成函数是为了让判定可测 —— 判定逻辑散在 adapter 里就没人能单独钉它。
///
/// ⚠️ 这是**逐份答案**的判定，**不是**实现的能力声明。
/// 实现「是否具备」那个能力由 [`crate::agent::ExtractionAgent::capabilities`]
/// 静态回答；两者不许互相推导（推导出来的声明会随答案翻转，见上面的表）。
pub fn can_rebuild_proposal(extraction: &GroupExtraction) -> bool {
    contract_gaps(extraction).is_empty()
}

/// **阶段 3 + 阶段 4：把各组候选组装成一份提案**。
///
/// `groups` 的**顺序就是阶段 0 的分组顺序**（一表一组，表在报告里的顺序）。
/// 顺序决定 `items` 的顺序，而顺序会进 `proposal.json` 的字节 ——
/// 所以这里**不去重排**，只做去重（同一个 `key` 在两组里都出现 = 同一个类型）。
///
/// 溯源三件（`round` / `extracted_by.round` / `failed_groups`）**不在这里填** ——
/// 与规则路径一样由 `orchestrate` 调 [`crate::agent::stamp_provenance`] 统一填，
/// 两处各填一次就是两个会漂移的口子。
///
/// `project` 只影响提案外壳（`Proposal.project`）。
pub fn rebuild_outcome(
    groups: &[GroupCandidates],
    report: &SchemaReport,
    project: &str,
    agent: &str,
    agent_version: &str,
) -> Result<ExtractionOutcome> {
    // ── 阶段 3 · 跨组合成：按 key 去重，同 key 不同内容 ⇒ 报错 ──
    let mut types: Vec<CandidateType> = Vec::with_capacity(groups.len());
    for g in groups {
        for t in &g.extraction.types {
            if let Some(prev) = types.iter().find(|p| p.key == t.key) {
                if candidates_differ(prev, t) {
                    anyhow::bail!(
                        "阶段 3 跨组合成失败：类型 key {:?} 在多个组里给出了**不同**的内容（来自组 {}，与先前组冲突）。\n\
                         \n\
                         为什么必须报错而不是「取第一个」：两个组对同一个 key 的说法不一致时，\
                         取任意一个都是**静默地丢掉另一个** —— 丢掉的那半份不会出现在任何地方，\
                         提案看起来完整，内容却是残的。\n\
                         设计点④说 key 是跨轮次锚点：一个 key 只能有一份定义。",
                        t.key,
                        g.group_id,
                    );
                }
                continue; // 内容一致 = 同一个类型，保留先到的那份
            }
            types.push(t.clone());
        }
    }

    let mut links: Vec<CandidateLink> = Vec::new();
    for g in groups {
        for l in &g.extraction.links {
            if let Some(prev) = links.iter().find(|p| p.key == l.key) {
                if link_candidates_differ(prev, l) {
                    anyhow::bail!(
                        "阶段 3 跨组合成失败：关系 {:?} 在多个组里给出了**不同**的内容（来自组 {}）。\
                         同上：取舍是策略，但「两个说法不一致却发出去」不是。",
                        l.key,
                        g.group_id,
                    );
                }
                continue;
            }
            links.push(l.clone());
        }
    }

    // ── 阶段 4 · 提案条目：纯搬运，零命名判断 ──
    let mut items: Vec<serde_json::Value> = Vec::with_capacity(types.len() + links.len());
    let mut stats = ExtractionStats {
        tables_seen: report.tables.len(),
        ..Default::default()
    };

    for t in &types {
        let entity = entity_item(t)?;
        // 统计按**实际来源**记，与 `extract_with_context` 的三分支逐一对应
        // （关系推导的名字同时计入 relation 与 wordlist —— 那是既有口径）。
        for p in &t.properties {
            stats.total_properties += 1;
            match p.origin {
                Some(PropertyOrigin::Relation) => {
                    stats.props_via_relation += 1;
                    stats.props_via_wordlist += 1;
                }
                Some(PropertyOrigin::WordList) => stats.props_via_wordlist += 1,
                Some(PropertyOrigin::Comment) => stats.props_via_comment += 1,
                None => unreachable!("entity_item 已经对缺 origin fail-closed 了"),
            }
        }
        stats.props_comment_rejected += t
            .rejected_comments
            .context(rebuild_contract_error(&t.key, "rejected_comments"))?;
        if entity.payload.display_name.is_empty() {
            stats.tables_without_cn_name.push(entity.payload.key.clone());
        } else {
            stats.entities_with_cn_name += 1;
        }
        items.push(serde_json::to_value(entity).context("实体条目序列化失败")?);
    }

    // ── AC-007 自检：关系两端都得在实体集合里 ──
    //
    // 与 `extract_with_context` **同一道**自检、**同一个**计数口径：
    // 悬空关系会进 `stats.links_dangling`，而 CLI 见到它非 0 就拒绝出提案
    // （审核期 AC-007 也会拦）。搬到这里不是为了改判定，是为了让
    // 「重建路径」不成为「绕开 AC-007 的第二条路」。
    let keys: HashSet<&str> = types.iter().map(|t| t.key.as_str()).collect();
    stats.links_inferred = links.len();
    let before = links.len();
    links.retain(|l| keys.contains(l.from_type.as_str()) && keys.contains(l.to_type.as_str()));
    stats.links_dangling = before - links.len();
    for l in links {
        items.push(serde_json::to_value(link_item(&l)?).context("关系条目序列化失败")?);
    }

    Ok(ExtractionOutcome {
        proposal: Proposal {
            project: project.to_string(),
            // 轮次与基版本由 `orchestrate` 覆写（它们不是本模块的判断）。
            base_version: propose::DEFAULT_PROPOSAL_BASE_VERSION,
            round: propose::DEFAULT_PROPOSAL_ROUND,
            extracted_by: ProposalProvenance {
                agent: agent.to_string(),
                version: agent_version.to_string(),
                round: propose::DEFAULT_PROPOSAL_ROUND,
                // 失败组由 `orchestrate` 用 `stamp_provenance` 填真值。
                failed_groups: Vec::new(),
                // 勘查留痕：与规则路径**同一个**函数生成的同一句话
                // （否则重建出来的提案在 provenance 上会说「不是这次勘查的」）。
                source: Some(propose::introspect_provenance(report)),
            },
            items,
        },
        stats,
    })
}

/// 候选类型 → 提案实体条目。
///
/// # `target` 为什么等于 `display_name`
///
/// REQ-002 的既有约定：实体条目的 `target` 是**给人看的标识**，
/// 规则路径里它就是 `from_cn`（= 该实体的展示名）。
/// 所以这里不需要契约里再存一个 `target` 字段 ——
///
/// ⚠️ **只对规则路径成立**：外置实现可以给 `display_name` 之外的展示文本，
/// 那时它需要自己声明。REQ-023 接协议 adapter 时若发现不够，
/// 加一个 `CandidateType::target: Option<String>` 是廉价的，
/// 而现在先不加 —— **不存在的字段不能被重建路径自己编出来**。
fn entity_item(t: &CandidateType) -> Result<EntityItem> {
    let mut properties = Vec::with_capacity(t.properties.len());
    for (i, p) in t.properties.iter().enumerate() {
        properties.push(PropertyItem {
            name: p.name.clone(),
            column: p.source.column.clone(),
            // ★ fail-closed：`nullable` 是源库的**事实**，不是 agent 的判断。
            // 缺了它就报错，而不是猜 `false` —— 猜出来的「可空」标记
            // 会直接出现在客户视图里，那是要给人看的合同。
            nullable: p.nullable.with_context(|| {
                format!(
                    "阶段 4 重建失败：类型 {:?} 的 properties[{i}] 没有 nullable。\n\
                     \n\
                     为什么不用 `false` 顶替：nullable 是源库的**事实**，不是 agent 的判断 —— \
                     阶段 1 的输入里就有 schema 片段，缺它只说明这一项没被读。\n\
                     猜出来的「可空」标记会**原样**出现在客户视图里（那是给人看的合同），\
                     而且没人查得出它错 —— 那正是本模块存在的理由：宁可报错，不发说不清的数据。\n\
                     \n\
                     怎么办：如实填上（读本组 schema 里那一列的 nullable），\
                     或别声明 {}。",
                    t.key,
                    crate::agent::CAPABILITY_REBUILDS_PROPOSAL
                )
            })?,
            note: p.note.clone(),
        });
    }
    // `origin` 是「重建统计」的必需项（缺它 → `props_via_*` 会写 0）。
    // 这里集中核一次并给出**一次**清楚的错误，而不是在两个统计分支里各报错一次。
    for (i, p) in t.properties.iter().enumerate() {
        if p.origin.is_none() {
            anyhow::bail!(
                "阶段 4 重建失败：类型 {:?} 的 properties[{i}] 没有声明 origin（属性名来源）。\n\
                 \n\
                 缺它就数不出 stats.props_via_comment / props_via_wordlist / props_via_relation，\
                 而这三个计数写 0 就是在说谎（真实源库 964 个属性里 579 个来自列注释）。\n\
                 代码不替 agent 猜「这个名字大概来自注释」—— 猜出来的统计没人查得出错，\
                 只会一直被人当成真的读下去。",
                t.key
            );
        }
    }
    Ok(EntityItem {
        kind: "add_entity_type".into(),
        target: t.display_name.clone(),
        payload: EntityPayload {
            key: t.key.clone(),
            display_name: t.display_name.clone(),
            description: t.description.clone(),
            aliases: t.aliases.clone(),
            evidence: t.evidence.clone(),
            properties,
        },
    })
}

/// 候选关系 → 提案关系条目。
fn link_item(l: &CandidateLink) -> Result<LinkItem> {
    Ok(LinkItem {
        kind: "add_link_type".into(),
        target: l.target.clone(),
        payload: LinkPayload {
            key: l.key.clone(),
            from_type: l.from_type.clone(),
            to_type: l.to_type.clone(),
            cardinality: l.cardinality.clone(),
            semantic: l.semantic.clone(),
            evidence: l.evidence.clone(),
        },
    })
}

fn rebuild_contract_error(type_key: &str, field: &str) -> String {
    format!(
        "阶段 4 重建失败：类型 {type_key:?} 的候选缺字段 {field:?}。\n\
         \n\
         这一版的阶段 1 契约要求候选承载提案要写的**每一项**（描述 / 别名 / 证据 / 说明 / \
         可空 / 名字来源 / 拒用计数），少一项重建出来的提案就不是原来那一份 —— \
         而字段「消失」在产物里看起来像「本来就没有」，是**静默**的改写。\n\
         \n\
         怎么办：要么把这项如实补上，要么别声明 {} ——\
         声明了却承载不下，就是这份重建路径存在的意义被架空了。",
        crate::agent::CAPABILITY_REBUILDS_PROPOSAL
    )
}

/// 两个同 key 的候选类型是否**内容不同**（用于跨组合成的冲突检出）。
///
/// 判据用序列化后逐字节比：结构体可能有多种等价写法，
/// 但只要**序列化的东西**一样，重建出来的提案就一样 —— 那不是冲突。
fn candidates_differ(a: &CandidateType, b: &CandidateType) -> bool {
    serde_json::to_string(a).ok() != serde_json::to_string(b).ok()
}

fn link_candidates_differ(a: &CandidateLink, b: &CandidateLink) -> bool {
    serde_json::to_string(a).ok() != serde_json::to_string(b).ok()
}