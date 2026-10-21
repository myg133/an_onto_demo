//! **业务概念组** —— 规格 §7.1 阶段 3「跨组合成」的落地（REQ-010）。
//!
//! # 这个类型是什么
//!
//! 一组**实体 key** 被声明为「客户脑子里的同一个业务概念」。
//! 真实源库 `public` 的 91 张表里，19% 是「一个概念拆成多表」
//! （`package_templates` / `package_products` / `package_benefits` 是三个
//! 「套餐」；`customer_rate_limit` / `customer_tpm_rate_limit` 是两个「限流」），
//! 直接翻译成 91 个对象，客户看到的是一张平铺的表目录，不是一套业务。
//!
//! # 这个类型**不是**什么 —— 红线
//!
//! > 概念组是**展示层的声明**，**不改变实体如何落库**。
//!
//! `apply` 之后实体表的行数与 key 必须与不做归并时**逐字节一致**。
//! 归并判断错了，去掉分组声明即可，**底层结构不动**。
//!
//! 这条红线是设计 §2.2「Agent 是提议者不是决定者」的直接延伸：
//! 一个由规则或 agent 猜出来的「这是同一个概念」，猜错的代价必须是
//! **零**（改个展示层声明就行），而不是**改数据**。
//! 所以概念组里**没有** `key 重写`、没有 `id 空间`、没有 `视图写绑定` ——
//! 任何一处能让「客户视图」和「落库结构」分家的地方都不在这里。
//!
//! 本模块**零 I/O**（AC-002）。

use crate::domain::snapshot::{Evidence, ExtractedBy};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// 一个业务概念 = 一组被声明为「同一个业务概念」的实体 key。
///
/// 成员用 **实体 key**（不是展示名）：key 是稳定标识，改名只改
/// `display_name`（§7.1 设计点 ④）。用展示名做成员等于让「客户」改名时
/// 分组静默失效 —— 而这正是本项目栽过的「两个命名空间混了」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptGroup {
    /// 概念标识。一旦生成不可变（与实体 key 同一条纪律）
    pub key: String,
    /// 给人看的概念名（中文）
    pub display_name: String,
    /// 成员实体 key。**顺序 = 声明顺序**，投影时按它在客户视图里
    /// 首次出现的顺序排（`sort` 留给人，不进这个字段）
    #[serde(default)]
    pub members: Vec<String>,
    /// 一句话说明：这个概念管什么。**`None` = 还没写**
    #[serde(default)]
    pub scope_hint: Option<String>,
    /// 声明者对「这个分组拿不准」的**结构化**标记（REQ-028 · F4）。
    ///
    /// # 为什么不从 `scope_hint` 文本里读
    ///
    /// 原来的做法是 `scope_hint.contains("低置信") || contains("待确认")`。
    /// `scope_hint` 是**给人看的说明文字**，拿它当机器信号 =
    /// 把「文档里写了什么」当成「系统处于什么状态」。任何一个分组者
    /// 改个措辞（「低置信」→「存疑」），标记就**静默消失**，
    /// 而且**没有任何测试会红** —— 这正是本项目反复栽的 fail-open。
    ///
    /// 现在它是声明文件里的一个**字段**：要改就改字段，
    /// 改不动 `scope_hint` 的措辞，也不会有人以为标记还在。
    ///
    /// # 三态而不是两态
    ///
    /// `Some(true)` = 明确声明拿不准；`Some(false)` = 明确声明有把握；
    /// `None` = **没声明**（老声明文件缺这个字段时的默认）。
    /// 三态可区分，才谈得上「补声明」这件事本身可被观测。
    ///
    /// # `None` 不等于「有把握」
    ///
    /// 消费方（`mcp::tools::confirmation_of`）只认 `Some(true)`。
    /// `None` 一律按「未声明」处理，**不**当成 `false` 去对外断言
    /// 「这个分组是有把握的」—— 那是把「没说过」说成「说过没问题」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low_confidence: Option<bool>,
    /// 谁声明的、哪个版本（§8.4）。`None` = 人工修订（ADR-001）
    #[serde(default)]
    pub extracted_by: Option<ExtractedBy>,
    /// 证据。**不投影到客户视图** —— 表名是源库标识符
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

impl ConceptGroup {
    /// 低置信标记的**唯一**读法：`Some(true)` 才算。
    ///
    /// `None` = 没声明过。**不要**把它当成 `false` 对外断言
    /// 「这个分组是有把握的」—— 见 [`ConceptGroup::low_confidence`] 的理由。
    pub fn is_low_confidence(&self) -> bool {
        self.low_confidence == Some(true)
    }
}

/// `scope_hint` 里「低置信 / 待确认」这类**措辞**。
///
/// # ⚠️ 这**不是**判据，只是一致性检测的探针（REQ-028 · F4）
///
/// 它唯一的作用是让 [`validate_groups`] 报 [`ConceptProblem::ScopeHintDrift`]：
/// 「有人改了说明里的措辞，却没同步结构化字段」。**任何消费方都不得
/// 用它推断状态** —— 它是给人看的文字，措辞随时会变，
/// 而消费方要的是**结构上稳定**的字段 [`ConceptGroup::low_confidence`]。
pub const LOW_CONFIDENCE_MARKERS: &[&str] = &["低置信", "待确认"];

/// 在 `scope_hint` 里找出第一个命中的低置信措辞（探针，见 [`LOW_CONFIDENCE_MARKERS`]）。
pub fn low_confidence_marker(scope_hint: Option<&str>) -> Option<&'static str> {
    let hint = scope_hint.unwrap_or("");
    LOW_CONFIDENCE_MARKERS.iter().copied().find(|m| hint.contains(m))
}

// ──────────────────────────────────────────────────────────
// 三条硬约束
//
// 注意措辞：这三条**不是「必须」**。它们是「如果不做，会坏成什么样
// 具体、可复现、且审核闸门被架空」的理由。设计要的是**可审核的声明**，
// 守门器只是让「不可审核的声明」进不来。
// ──────────────────────────────────────────────────────────

/// 校验结果里的一类问题。**空 = 全部合法**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConceptProblem {
    /// ① 悬空成员：成员不在本项目实体集合内
    DanglingMember { group: String, member: String },
    /// 退化：一个成员都没有
    EmptyGroup { group: String },
    /// 同一个概念 key 声明了两次
    DuplicateGroupKey { key: String },
    /// ③ 同一实体归属多个概念（重叠）
    OverlappingMembership { member: String, groups: Vec<String> },
    /// ② 嵌套归并：某概念的成员本身是另一个概念
    NestedGroup { group: String, parent: String },
    /// ④（REQ-028 · F4）**声明自相矛盾**：`scope_hint` 里写了
    /// 「低置信 / 待确认」，而结构化字段 [`ConceptGroup::low_confidence`]
    /// 没有声明 `true`。
    ///
    /// # 这条为什么存在
    ///
    /// 结构化字段一旦成为唯一判据，「说明文字」与「字段」就会**分家**：
    /// 有人把 `scope_hint` 改成「存疑」却没动字段（或反之），
    /// 于是**文字与事实各说各话**，而机器只认其中一个。
    ///
    /// 这条问题把「分家」变成**报错**（[`validate_groups`] 的问题会让
    /// `apply` 失败，见 `domain::apply`）。代价是：改 `scope_hint` 的
    /// 措辞有时会**要求**同步改字段 —— 但那本来就是「改了一个事实、
    /// 另一个事实没跟上」，本来就该有人来看一眼。
    ///
    /// 反过来（字段是 `true`、文字里没有措辞）**不报错**：
    /// 字段是判据，文字只是给人看的，措辞不该有约束力。
    ScopeHintDrift { group: String, marker: String },
}

impl std::fmt::Display for ConceptProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConceptProblem::DanglingMember { group, member } => {
                write!(f, "概念「{group}」的成员 {member} 不在本项目实体集合内")
            }
            ConceptProblem::EmptyGroup { group } => write!(f, "概念「{group}」没有任何成员"),
            ConceptProblem::DuplicateGroupKey { key } => write!(f, "概念 key 重复声明：{key}"),
            ConceptProblem::OverlappingMembership { member, groups } => write!(
                f,
                "实体 {member} 同时归属 {} 个概念：{}",
                groups.len(),
                groups.join("、")
            ),
            ConceptProblem::NestedGroup { group, parent } => {
                write!(f, "概念「{group}」被当成「{parent}」的成员（嵌套归并）")
            }
            ConceptProblem::ScopeHintDrift { group, marker } => write!(
                f,
                "概念「{group}」的说明里写了「{marker}」，但结构化字段 \
                 low_confidence 没有声明 true —— 文字与字段分家了（判据以字段为准）"
            ),
        }
    }
}

/// 校验一组概念声明。
///
/// # ① 成员必须全在本项目实体集合内 —— **为什么**
///
/// 概念组是**关于这个项目的实体集合**的声明。成员不在集合内，意味着
/// 这份声明是照着**另一个版本的本体**（或另一张没被抽出来的表）写的：
///
/// - 成员数于是虚报覆盖范围 —— 总览会说「客户这个概念覆盖 6 个对象」，
///   而实际只看得见 5 个。**静默少一个对象**是本项目栽过最贵的
///   fail-open：判读人会以为「系统里就这么些」（`html_view` 那条
///   「剔除必须报出来」的纪律同源）；
/// - 更要命的是**审核闸门被架空**：双轨审核的前提是「人能对着真东西核对」。
///   成员根本不存在，业务人员**没有任何东西可看**，条目就自动绿灯通过 ——
///   一条无法被反驳的声明，等于没有审核。
/// - 关系重定向也无从谈起：指向这个成员的关系在视图里查无此对象。
///
/// 所以这一条不是洁癖，是**让审核有意义**的最低条件。
///
/// # ② 嵌套归并必须被拒 —— **为什么 A∪B、B∪C、A∪C 三条声明会互相矛盾**
///
/// 三条声明**逐条看都没问题**（每条都只是在说「这两个是一回事」），
/// 但总览要回答的是「一个实体属于哪个概念」——**单归属**。
/// 嵌套一旦允许，这个答案就不存在了：
///
/// - A 同时是「A∪B」的成员和「A∪C」的成员 ⇒ A 有两个家；
/// - B∪C 让 B、C 传递地同属一组 ⇒ 同一对对象可能出现在两张卡上；
/// - 关系重定向于是有**两个都合法的目标**：一条「订单 → 客户」的关系，
///   按 A∪B 指向「A∪B」，按 A∪C 指向「A∪C」。
///   选哪个？选深的不选浅的？先声明的赢？**任何一种裁决规则都是
///   代码替 agent/业务人员做了他没做的决定** —— 正好踩中 §2.2。
/// - 更根本的一点：嵌套把「一个概念拆成多表」升级成了
///   **数据层级的声明**（A∪(B∪C) 是本体里的一个上位类），
///   那已经不是展示层了 —— 它越过了本文档开头那条红线。
///
/// 结论：不是「暂不支持」，是**会让声明失去意义**。
/// 真需要「概念里还有概念」时，它该是另一种提案（一条被双轨审核的关系），
/// 而不是把 group key 塞进 members。
///
/// # ③ 组之间**不允许**重叠 —— 本项目的判断
///
/// **判断：拒（一个实体最多属一个概念）。** 理由四条，按重要性：
///
/// 1. **关系重定向必须是全函数**。重叠时同一条关系有两个合法目标，
///    页面上会出现「一个 客户，有几个 客户？」这种废问句
///    （`html_view::relation_as_question` 拼的就是这个问句）。
///    取其一就是代码偷偷定优先级，**没人审过**。
/// 2. **总览是判读时的导航索引，不是分类学**。一个卡片出现在两个概念下，
///    判读人无法判断哪个是它的家，而 §7.1 阶段 3 问的恰恰是
///    「合并还是分开」—— **重叠是「两个都是」= 没回答这个问题**。
///    声明的职责是记录答案，不是记录犹豫。
/// 3. **审核粒度会失真**。审「A 和 B 是一回事」的人，并没有审
///    「A 属于 B 那个概念、也属于 C 那个概念」。这正是 §2.2 说的
///    「让 agent 自己做判断、自己说自己对」的空转。
/// 4. **代价是零**。概念组不碰落库结构（红线），所以「一个对象在两个
///    业务维度上都是主角」这种真实需求**没有任何损失**——
///    那是**多对多关系**，该由一条独立提案、双轨审核来表达，
///    而不是把重叠偷渡进分组里。将来要支持重叠，正确形态是
///    `concept_link`（显式多对多），不是放宽这一条。
///
/// # 顺带（不算三条之内）
///
/// - **空组**：总览会渲染出一张什么都没有的卡，判读人以为「这个概念
///   系统里没有东西」。退化输入，拒。
/// - **同 key 重复**：两版声明同名会让 diff 变成「删了旧的、加了新的」，
///   而它们其实是同一个概念的两次声明。
pub fn validate_groups(
    groups: &[ConceptGroup],
    known_entity_keys: &BTreeSet<String>,
) -> Vec<ConceptProblem> {
    let mut problems: Vec<ConceptProblem> = Vec::new();

    // 组 key 唯一
    let mut seen_keys: BTreeSet<&str> = BTreeSet::new();
    for g in groups {
        if !seen_keys.insert(g.key.as_str()) {
            problems.push(ConceptProblem::DuplicateGroupKey { key: g.key.clone() });
        }
        // ④ REQ-028 · F4：说明文字与结构化字段**分家**。
        // 判据只认字段；文字只用来发现「有人改了措辞却没改字段」。
        if let Some(marker) = low_confidence_marker(g.scope_hint.as_deref()) {
            if !g.is_low_confidence() {
                problems.push(ConceptProblem::ScopeHintDrift {
                    group: g.key.clone(),
                    marker: marker.to_string(),
                });
            }
        }
    }

    // 组 key 本身也是 key 命名空间的一份 —— 嵌套归并在这里被抓
    let group_keys: BTreeSet<&str> = groups.iter().map(|g| g.key.as_str()).collect();

    // 成员归属：实体 key → 组 key（**单归属**）
    let mut home: BTreeMap<String, String> = BTreeMap::new();
    for g in groups {
        if g.members.is_empty() {
            problems.push(ConceptProblem::EmptyGroup { group: g.key.clone() });
            continue;
        }
        for m in &g.members {
            // ② 嵌套：一个概念的成员是另一个概念
            if group_keys.contains(m.as_str()) && m != &g.key {
                problems.push(ConceptProblem::NestedGroup {
                    group: m.clone(),
                    parent: g.key.clone(),
                });
                continue;
            }
            // ① 悬空
            if !known_entity_keys.contains(m) {
                problems.push(ConceptProblem::DanglingMember {
                    group: g.key.clone(),
                    member: m.clone(),
                });
                continue;
            }
            // ③ 重叠
            match home.get(m) {
                Some(prev) if prev != &g.key => {
                    let mut gs = vec![prev.clone(), g.key.clone()];
                    gs.sort();
                    problems.push(ConceptProblem::OverlappingMembership {
                        member: m.clone(),
                        groups: gs,
                    });
                }
                _ => {
                    home.insert(m.clone(), g.key.clone());
                }
            }
        }
    }
    problems
}

/// 一个**客户可见的组字段**撞上了客户发布闸的词表（REQ-115 · C-1，原 B-2）。
///
/// # 为什么组字段要过客户闸
///
/// `display_name`、组 `key`（线上 `id`）与 `scope_hint` 都会出现在**客户出口**里：
/// `/api/graph` 的 `groups[]`，以及业务总览（C4/C5/M1）的 `summary`。
/// 这些出口的响应体**整份**过 `scan_modeling_terms`。一个字段里带 `Key`
/// （例如「未持 Key 的试用请求」被写进 `scope_hint`）会让**每一次**出图/出总览都 500 ——
/// 客户看到「读不到」，运维要翻 stderr 才知道是某个组字段的问题。
/// 所以判在**导入时**：坏声明根本进不来，报错直接点名「哪个组、哪个字段、哪个词」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNameHit {
    /// 出问题的组 key（声明里的那个标识；报错要点得出是哪一个组）。
    pub group: String,
    /// 出问题的字段：`display_name` / `scope_hint` / `key`（线上 `id`）。
    pub field: &'static str,
    /// 命中的守门词条（原样取自 `scan_modeling_terms`）。
    pub term: String,
}

impl std::fmt::Display for GroupNameHit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "组 `{}` 的 {} 含建模术语 `{}`",
            self.group, self.field, self.term
        )
    }
}

/// **C-1 · 组字段的客户闸**：导入前扫一遍**客户可见**的组字段。
///
/// 空 = 可以导入。**fail-closed**：命中就拒，⛔ 不静默剔除、⛔ 不改写字段
/// （改写等于替声明作者做措辞决定 —— 他该自己把「未持 Key」改成「未持凭证」）。
///
/// 扫三个字段，因为三个都**出现在客户出口**里：
/// - `display_name` —— 客户在页面上看到的组名；
/// - `scope_hint` —— 客户在总览里看到的组说明（也是图上的组说明，C-3 同一口径）；
/// - `key` —— 线上是 `groups[].id`（**值**也会被闸扫，不只是键名）。
pub fn scan_group_names(groups: &[ConceptGroup]) -> Vec<GroupNameHit> {
    let mut hits: Vec<GroupNameHit> = Vec::new();
    for g in groups {
        let fields: [(&'static str, &str); 2] =
            [("display_name", g.display_name.as_str()), ("key", g.key.as_str())];
        for (field, value) in fields {
            push_hits(&mut hits, g, field, value);
        }
        // `scope_hint` 是可选项：`None` = 没写说明，不判。
        if let Some(hint) = g.scope_hint.as_deref() {
            push_hits(&mut hits, g, "scope_hint", hint);
        }
    }
    hits
}

/// 一个字段的命中收集（判据**只有一处实现**：借管线的 `group_name_hits`）。
fn push_hits(out: &mut Vec<GroupNameHit>, g: &ConceptGroup, field: &'static str, value: &str) {
    // ⚠️ 借管线的 `group_name_hits`（它转调 `scan_modeling_terms`）。
    // ⛔ 别在这里直接再调一次判据 —— `test_req107_no_second_judgement_outside_pipeline` 会红。
    for term in crate::domain::pipeline::group_name_hits(value) {
        out.push(GroupNameHit {
            group: g.key.clone(),
            field,
            term,
        });
    }
}

/// 实体 key → 概念 key 的**单归属**映射。关系重定向与总览都以它为准。
///
/// # 为什么必须是**函数**而不是「一堆候选」
///
/// 总览要回答「一个实体属于哪个概念」，而这个答案会同时用在三处：
/// 关系重定向、属性去重的范围、未归组清单的补集。
/// 三处必须给同一个答案，否则同一个对象在页面的不同位置会显示成
/// 不同的概念 —— 判读人当场就发现了。
/// 所以这里返回 `Option`（无组 = `None`）而不是 `Vec`（候选组列表）。
pub fn home_map(groups: &[ConceptGroup]) -> BTreeMap<String, String> {
    let mut home: BTreeMap<String, String> = BTreeMap::new();
    for g in groups {
        for m in &g.members {
            home.entry(m.clone()).or_insert_with(|| g.key.clone());
        }
    }
    home
}

/// 概念 key → 组的索引（`home_map` 的反向）。
pub fn group_index(groups: &[ConceptGroup]) -> BTreeMap<String, &ConceptGroup> {
    groups.iter().map(|g| (g.key.clone(), g)).collect()
}

// ──────────────────────────────────────────────────────────
// 声明文件（过渡用初始声明就长这样）
//
// 为什么声明要**独立成文件**，而不是直接改进代码里那份翻译表：
//   翻译表是「这张表叫什么」，声明是「这几个东西是不是一回事」——
//   后者的**评审者不是作者**（HK Agent 或业务人员），它的版本节奏也
//   不同（声明按轮次复核，翻译表按源库变更）。
//   两件事混在一份文件里，改一个必然碰另一个的评审记录。
// ──────────────────────────────────────────────────────────

/// 一份**概念声明**。
///
/// 【⚠ 过渡声明不是结论】真实源库 19% 的表是「一个概念拆多表」，
/// 但**哪几个是同一个概念是语义判断**。过渡声明只做到「按表名前缀
/// 聚类 + 人工钉住明显的例外」，最终由 HK Agent / 业务人员确认。
/// 所以它带 `version` 与 `history`，并遵守**改内容必须改版本**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptDeclaration {
    /// 当前版本（语义化版本号）。`history` 最后一条必须与它一致
    pub version: String,
    /// 谁声明的（**记规则，不记「我」** —— 声明是规则的产物，不是谁的判断）
    #[serde(default)]
    pub declared_by: String,
    /// 聚类规则的一句话说明。评审者要先看规则，才知道这些分组是怎么来的
    #[serde(default)]
    pub rule: String,
    /// 业务确认人。`None` = **还没人确认**（过渡声明的常态）
    #[serde(default)]
    pub confirmed_by: Option<String>,
    /// 这份声明**从哪来**（REQ-115 · AC-010）。
    ///
    /// - `Some("algorithm-suggest")` = `concept suggest` 从关系拓扑算出来的**建议**；
    /// - `None` = 旧声明（那时还没有这一栏）。
    ///
    /// ⚠️ **它不进 [`content_digest`]**：来源记的是「谁算的」，不是「内容是什么」。
    /// 把来源算进指纹会让「同一批分组、换了个算法版本」必须升声明版本 ——
    /// 那两件事本来就该分别看待。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// **审核人**（REQ-115 · AC-010）。**恒有键**：`null` = 还没人审过。
    ///
    /// 与 `confirmed_by` 的区别：那个是「业务上认不认这组」，这个是
    /// 「谁把这份**文件**审过」。建议层产出的文件永远是 `null` ——
    /// 「算法算过」不等于「人审过」，把两者写成同一个字段就是冒充。
    #[serde(default)]
    pub reviewed_by: Option<String>,
    /// 追加式版本历史。见 [`ConceptDeclaration::verify`]
    #[serde(default)]
    pub history: Vec<DeclarationRevision>,
    pub groups: Vec<ConceptGroup>,
}

/// 一次改动的留痕。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeclarationRevision {
    pub version: String,
    /// 本版本**内容**的指纹（见 [`content_digest`]）
    pub content_digest: String,
    /// 这一版改了什么 / 为什么
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeclarationError {
    #[error("声明文件解析失败：{0}")]
    Parse(String),
    #[error("改内容必须改版本：当前内容指纹 {actual} 与版本 {version} 记录的不一致（记录的是 {recorded}）")]
    DigestMismatch { version: String, recorded: String, actual: String },
    #[error("版本历史最后一条是 {last:?}，与当前 version {current:?} 不一致")]
    HistoryTailMismatch { current: String, last: String },
    #[error("版本历史为空：声明必须至少有一条版本记录")]
    EmptyHistory,
    #[error("版本号 {0} 在历史里出现多次")]
    DuplicateVersion(String),
}

/// 声明文件的机械契约（**改内容必须改版本**）。
///
/// # 为什么要有这个契约
///
/// 声明文件是人改的（HK Agent / 业务人员），而它直接决定客户看到什么。
/// 没有契约时，改了内容不升版本 → 两个版本的声明长得一样却叫同一个版本号
/// → 「这一版到底包含什么」再也答不出来，评审记录失去意义。
/// 这正是 §8.4 对 `extracted_by` 版本漂移的处理方式，只是落在声明文件上。
///
/// # 怎么强制
///
/// `history` 是**追加式**的，最后一条必须同时满足两件事：
/// 1. `version` 等于当前 `version`；
/// 2. `content_digest` 等于当前内容算出的指纹。
///
/// 于是「改了 members 却没加历史条目」当场被抓 —— 不需要人去记得。
pub fn content_digest(groups: &[ConceptGroup], rule: &str, declared_by: &str) -> String {
    // **刻意不算 version / history 本身** —— 指纹的职责是「内容有没有变」，
    // 版本号正是那个「变」要记的东西，把它算进去就成了自指。
    // 算 `rule` 与 `declared_by` 是因为它们改变了声明的**依据**：
    // 同一批成员、不同的聚类规则，是两份不同的声明。
    let canonical = serde_json::json!({
        "declared_by": declared_by,
        "rule": rule,
        "groups": groups,
    });
    // FNV-1a 64 —— **不是安全哈希**，只是内容指纹。
    // 不引依赖是因为这个值只需要「同内容同值、异内容异值」。
    let bytes = canonical.to_string().into_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

impl ConceptDeclaration {
    pub fn parse(text: &str) -> Result<Self, DeclarationError> {
        serde_json::from_str(text).map_err(|e| DeclarationError::Parse(e.to_string()))
    }

    /// 校验「改内容必须改版本」。**返回 `Err` 就是不许进提案通道。**
    pub fn verify(&self) -> Result<(), DeclarationError> {
        if self.history.is_empty() {
            return Err(DeclarationError::EmptyHistory);
        }
        let last = self.history.last().expect("非空");
        if last.version != self.version {
            return Err(DeclarationError::HistoryTailMismatch {
                current: self.version.clone(),
                last: last.version.clone(),
            });
        }
        let actual = content_digest(&self.groups, &self.rule, &self.declared_by);
        if last.content_digest != actual {
            return Err(DeclarationError::DigestMismatch {
                version: self.version.clone(),
                recorded: last.content_digest.clone(),
                actual,
            });
        }
        let mut seen = BTreeSet::new();
        for h in &self.history {
            if !seen.insert(h.version.clone()) {
                return Err(DeclarationError::DuplicateVersion(h.version.clone()));
            }
        }
        Ok(())
    }

    /// 声明 → 提案 item 的 JSON 形状。
    ///
    /// `target` 用**概念展示名**（给人看的那一栏），`payload.key` 才是 key ——
    /// 与实体类型条目同一套约定（`ItemView::type_key` 从 payload 取 key）。
    pub fn to_items(&self) -> Vec<serde_json::Value> {
        self.groups
            .iter()
            .map(|g| {
                serde_json::json!({
                    "kind": crate::domain::review::ItemKind::AddConceptGroup.as_str(),
                    "target": g.display_name,
                    "payload": g,
                })
            })
            .collect()
    }
}

/// 概念声明的 payload → [`ConceptGroup`]。
///
/// 与 `domain::apply` 里其它 `*_from_payload` 同一套约定：
/// 提案里的 payload **不完整**是常态（抽取器只给关键字段），
/// 缺失项补默认值 —— 宁可字段少，不要 apply 失败。
/// 但 `members` **不接受缺失**：没有成员的组是退化输入，
/// 交给 [`validate_groups`] 当 `EmptyGroup` 拒掉，而不是在这里静默造一个空组。
pub fn group_from_payload(key: &str, p: &serde_json::Value) -> Option<ConceptGroup> {
    let members: Vec<String> = p
        .get("members")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    Some(ConceptGroup {
        key: p.get("key").and_then(|v| v.as_str()).unwrap_or(key).to_string(),
        display_name: p
            .get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or(key)
            .to_string(),
        members,
        scope_hint: p.get("scope_hint").and_then(|v| v.as_str()).map(String::from),
        low_confidence: p
            .get("low_confidence")
            .and_then(|v| v.as_bool()),
        extracted_by: p
            .get("extracted_by")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value::<ExtractedBy>(v.clone()).ok()),
        evidence: p
            .get("evidence")
            .and_then(|v| serde_json::from_value::<Vec<Evidence>>(v.clone()).ok())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(key: &str, members: &[&str]) -> ConceptGroup {
        ConceptGroup {
            key: key.into(),
            display_name: key.into(),
            members: members.iter().map(|s| s.to_string()).collect(),
            scope_hint: None,
            extracted_by: None,
            low_confidence: None,
            evidence: vec![],
        }
    }

    fn keys(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_valid_partition_passes() {
        let groups = vec![g("package", &["t067", "t066"]), g("balance", &["t015"])];
        let known = keys(&["t067", "t066", "t015", "orders"]);
        assert_eq!(validate_groups(&groups, &known), vec![]);
    }

    #[test]
    fn test_dangling_member_is_caught() {
        let groups = vec![g("package", &["t067", "package_ghost"])];
        let known = keys(&["t067"]);
        let p = validate_groups(&groups, &known);
        assert_eq!(
            p,
            vec![ConceptProblem::DanglingMember {
                group: "package".into(),
                member: "package_ghost".into()
            }]
        );
    }

    #[test]
    fn test_overlap_is_caught() {
        let groups = vec![g("a", &["x"]), g("b", &["x"])];
        let known = keys(&["x"]);
        let p = validate_groups(&groups, &known);
        assert_eq!(
            p,
            vec![ConceptProblem::OverlappingMembership { member: "x".into(), groups: vec!["a".into(), "b".into()] }]
        );
    }

    #[test]
    fn test_nesting_is_caught_even_when_all_entities_exist() {
        // 成员既是实体、又是另一个概念 → 嵌套优先报，且不影响别的判定
        let mut inner = g("inner", &["x"]);
        inner.display_name = "内层".into();
        let outer = g("outer", &["inner"]);
        let known = keys(&["x", "inner"]);
        let p = validate_groups(&[outer, inner], &known);
        assert!(p.contains(&ConceptProblem::NestedGroup { group: "inner".into(), parent: "outer".into() }));
    }

    #[test]
    fn test_empty_and_duplicate_are_caught() {
        let known = keys(&["x"]);
        let p = validate_groups(&[g("e", &[]), g("d", &["x"]), g("d", &["x"])], &known);
        assert!(p.contains(&ConceptProblem::EmptyGroup { group: "e".into() }));
        assert!(p.contains(&ConceptProblem::DuplicateGroupKey { key: "d".into() }));
    }

    #[test]
    fn test_home_map_is_single_assignment() {
        // 重叠时取先声明的那个 —— 但**校验层已经拒了**，
        // 这里只保证「不 panic、答案确定」
        let m = home_map(&[g("a", &["x", "y"]), g("b", &["y"])]);
        assert_eq!(m.get("x").map(String::as_str), Some("a"));
        assert_eq!(m.get("y").map(String::as_str), Some("a"));
    }

    // ──────────────────────────────────────────────────────────
    // REQ-115 · C-1：组字段的客户闸（导入前判，不是出图时判）
    // ──────────────────────────────────────────────────────────

    #[test]
    fn test_clean_group_names_pass_the_customer_gate() {
        assert_eq!(scan_group_names(&[g("customer_account", &["customers"])]), vec![]);
    }

    /// 组名里写「未持 Key」⇒ 报「组 + 字段 + 词条」，不是一句笼统的拒绝。
    #[test]
    fn test_group_display_name_with_a_modeling_term_is_named_precisely() {
        let mut bad = g("api_entitlement", &["licenses"]);
        bad.display_name = "未持 Key 的试用请求".into();
        let hits = scan_group_names(&[bad]);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].group, "api_entitlement");
        assert_eq!(hits[0].field, "display_name");
        assert_eq!(hits[0].term, "key");
        assert_eq!(
            hits[0].to_string(),
            "组 `api_entitlement` 的 display_name 含建模术语 `key`"
        );
    }

    /// 组 key 会作为线上 `groups[].id` 的**值**被闸扫 ⇒ 也要判。
    #[test]
    fn test_group_key_itself_is_scanned_as_a_wire_value() {
        let mut bad = g("primary_key", &["m"]);
        bad.display_name = "干净组名".into();
        let hits = scan_group_names(&[bad]);
        assert!(hits.iter().any(|h| h.field == "key"), "{hits:?}");
        assert!(
            hits.iter().any(|h| h.term == "primary_key"),
            "词组守门词（primary_key）也该被抓住：{hits:?}"
        );
    }

    /// ⭐ C-1：`scope_hint` 是**客户可见**的组说明（业务总览的 `summary`），
    /// 所以它也要过闸 —— 这正是 QA 阻断的那一句（「未持 Key」）。
    #[test]
    fn test_scope_hint_with_a_modeling_term_is_rejected() {
        let mut grp = g("api_entitlement", &["licenses"]);
        grp.display_name = "API 密钥与授权".into();
        grp.scope_hint = Some("客户手上有什么凭证、以及未持 Key 的试用请求".into());
        let hits = scan_group_names(&[grp]);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].group, "api_entitlement");
        assert_eq!(hits[0].field, "scope_hint");
        assert_eq!(hits[0].term, "key");
        assert_eq!(
            hits[0].to_string(),
            "组 `api_entitlement` 的 scope_hint 含建模术语 `key`"
        );
    }

    /// 三个客户可见字段都干净 ⇒ 放行；改回业务措辞（「未持凭证」）即可导入。
    #[test]
    fn test_customer_safe_wording_passes() {
        let mut grp = g("api_entitlement", &["licenses"]);
        grp.display_name = "API 密钥与授权".into();
        grp.scope_hint = Some("客户手上有什么凭证、以及未持凭证的试用请求".into());
        assert_eq!(scan_group_names(&[grp]), vec![]);
    }

    fn decl(groups: Vec<ConceptGroup>, version: &str) -> ConceptDeclaration {
        let d = content_digest(&groups, "按表名前缀聚类", "prefix-cluster-seed");
        ConceptDeclaration {
            version: version.into(),
            declared_by: "prefix-cluster-seed".into(),
            rule: "按表名前缀聚类".into(),
            confirmed_by: None,
            source: None,
            reviewed_by: None,
            history: vec![DeclarationRevision {
                version: version.into(),
                content_digest: d,
                note: "初版".into(),
            }],
            groups,
        }
    }

    #[test]
    fn test_declaration_passes_when_digest_matches() {
        let d = decl(vec![g("a", &["x"])], "0.1.0");
        assert_eq!(d.verify(), Ok(()));
    }

    #[test]
    fn test_editing_content_without_new_revision_is_caught() {
        // 「改内容必须改版本」—— 加一个成员但不升版本
        let mut d = decl(vec![g("a", &["x"])], "0.1.0");
        d.groups[0].members.push("y".into());
        assert!(matches!(d.verify(), Err(DeclarationError::DigestMismatch { .. })));
    }

    #[test]
    fn test_bumping_version_without_history_entry_is_caught() {
        let mut d = decl(vec![g("a", &["x"])], "0.1.0");
        d.version = "0.2.0".into();
        assert!(matches!(d.verify(), Err(DeclarationError::HistoryTailMismatch { .. })));
    }

    #[test]
    fn test_digest_is_sensitive_to_rule_and_content() {
        let a = content_digest(&[g("a", &["x"])], "按前缀", "s");
        let b = content_digest(&[g("a", &["x", "y"])], "按前缀", "s");
        let c = content_digest(&[g("a", &["x"])], "按语义", "s");
        assert_ne!(a, b, "成员变了，指纹必须变");
        assert_ne!(a, c, "聚类规则变了，指纹必须变");
        assert_eq!(a, content_digest(&[g("a", &["x"])], "按前缀", "s"), "同内容同值");
    }

    #[test]
    fn test_empty_history_is_caught() {
        let mut d = decl(vec![g("a", &["x"])], "0.1.0");
        d.history.clear();
        assert_eq!(d.verify(), Err(DeclarationError::EmptyHistory));
    }

    #[test]
    fn test_items_carry_add_concept_group_kind_and_payload_key() {
        let d = decl(vec![g("package", &["t067"])], "0.1.0");
        let items = d.to_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"], "add_concept_group");
        assert_eq!(items[0]["payload"]["key"], "package");
        assert_eq!(items[0]["payload"]["members"][0], "t067");
    }

    // ──────────────────────────────────────────────────────────
    // REQ-028 · F4：低置信判据从「读文字」改成「读字段」
    // ──────────────────────────────────────────────────────────

    fn g_with(key: &str, hint: Option<&str>, lc: Option<bool>) -> ConceptGroup {
        ConceptGroup {
            low_confidence: lc,
            scope_hint: hint.map(String::from),
            ..g(key, &["t067"])
        }
    }

    /// 结构化字段是**唯一**判据：`scope_hint` 写成什么样都不影响结果。
    ///
    /// 这条测试就是 F4 的核心断言 —— 把旧判据（「低置信」「待确认」两个词）
    /// 挪走之后它**必须**红，否则说明有人把文本匹配又接回了消费路径。
    #[test]
    fn test_low_confidence_reads_the_field_not_the_wording() {
        // 措辞里有「低置信」，但字段没说 ⇒ false（字段是判据）
        assert!(!g_with("a", Some("⚠ 低置信：待确认"), None).is_low_confidence());
        // 措辞里**完全没有**那两个词，但字段说了 ⇒ true
        assert!(g_with("b", Some("服务应用与服务配置"), Some(true)).is_low_confidence());
        // 措辞改了（本次 REQ 要防的那种漂移）⇒ 结果不变
        assert!(g_with("c", Some("⚠ 存疑：可能该分开"), Some(true)).is_low_confidence());
    }

    /// `None` ≠ `false`：没声明过不能说成「有把握」。
    #[test]
    fn test_absent_field_is_not_a_false_assertion() {
        let known = keys(&["t067"]);
        for lc in [None, Some(false)] {
            assert!(!g_with("a", None, lc).is_low_confidence());
        }
        assert!(g_with("a", None, Some(true)).is_low_confidence());
        // validate_groups 不该因为「没声明」就报错 —— 那不是矛盾，是沉默
        assert_eq!(validate_groups(&[g_with("a", None, None)], &known), vec![]);
    }

    /// 文字与字段**分家** ⇒ 报错（ScopeHintDrift），而不是静默取其一。
    #[test]
    fn test_scope_hint_wording_without_the_field_is_a_problem() {
        let known = keys(&["t067"]);
        let p = validate_groups(&[g_with("a", Some("⚠ 低置信：可能该分开"), None)], &known);
        assert_eq!(p.len(), 1, "文字写了低置信、字段没写 ⇒ 必须报一条");
        assert!(matches!(p[0], ConceptProblem::ScopeHintDrift { .. }));

        // 字段写了 true ⇒ 一致，不报
        assert_eq!(
            validate_groups(&[g_with("a", Some("⚠ 低置信：可能该分开"), Some(true))], &known),
            vec![],
            "字段是判据，文字与之一致时不该报"
        );
        // 反向不报：字段 true、措辞没了 —— 措辞不该有约束力
        assert_eq!(
            validate_groups(&[g_with("a", Some("已复核"), Some(true))], &known),
            vec![],
            "措辞被改掉不该报错 —— 判据是字段，措辞只是给人看的"
        );
    }

    /// 真实过渡声明：5 个低置信组必须**结构化地**声明为 true。
    ///
    /// 用 `include_str!` 而不是运行时读文件 —— 文件不在，**编译就失败**。
    const DECL: &str = include_str!("concept_groups.transition.json");

    #[test]
    fn test_real_declaration_declares_low_confidence_structurally() {
        let decl: ConceptDeclaration =
            serde_json::from_str(DECL).expect("过渡声明必须可解析");
        assert_eq!(decl.verify(), Ok(()), "声明仍须满足「改内容必须改版本」");

        let flagged: Vec<&str> = decl
            .groups
            .iter()
            .filter(|g| g.is_low_confidence())
            .map(|g| g.key.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec![
                "concept_agent",
                "concept_instance",
                "concept_internal",
                "concept_license",
                "concept_service",
            ],
            "真实声明里恰好这 5 组写了「低置信」"
        );

        // 判据换成字段之后，这 5 组必须**仍然**被判成低置信 ——
        // 这正是「改成结构化但结果不变」的验收。
        let by_text: Vec<&str> = decl
            .groups
            .iter()
            .filter(|g| low_confidence_marker(g.scope_hint.as_deref()).is_some())
            .map(|g| g.key.as_str())
            .collect();
        assert_eq!(by_text, flagged, "结构化字段与原文本判据必须指向同一批组");

        // 全量组不应报漂移
        let entity_keys: BTreeSet<String> =
            decl.groups.iter().flat_map(|g| g.members.iter().cloned()).collect();
        let drift: Vec<String> = validate_groups(&decl.groups, &entity_keys)
            .into_iter()
            .filter(|p| matches!(p, ConceptProblem::ScopeHintDrift { .. }))
            .map(|p| p.to_string())
            .collect();
        assert_eq!(drift, Vec::<String>::new(), "真实声明不得有文字/字段分家");
    }
}
