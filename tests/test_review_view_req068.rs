//! REQ-068 · **两轨前缀识别里剩下的两处 `contains`**（F-2 同机制残留 · 第四、五处）。
//!
//! 测试名带 `test_req068_` 前缀。
//!
//! # 病根（同一族，第三轮）
//!
//! | 需求 | 修的是哪一处 |
//! | --- | --- |
//! | REQ-058 | `verify_track_separation` 的两处**两轨前缀**认轨 |
//! | REQ-064 | `verify_provenance_scope` 规则① 的**归属前缀**认轨 |
//! | **本条** | `verify_provenance_scope` **规则②** 的两处**两轨前缀**认轨 |
//!
//! 原来（`verify_provenance_scope` 规则②）：
//!
//! ```text
//! if line.contains(BIZ_LINE_PREFIX) || line.contains(MODEL_LINE_PREFIX) { … }
//! .position(|l| l.contains(BIZ_LINE_PREFIX) || l.contains(MODEL_LINE_PREFIX))
//! ```
//!
//! `claim` / 展示名 / `未通过原因` / `agent` 都是**提案方原样放进 payload 的任意文本**，
//! 它们里面只要出现「业务轨：」或「建模轨：」这几个字，就会被当成**结构行**。
//!
//! # 修法
//!
//! 两处都改走 REQ-058/064 已落地的**唯一**行首识别处
//! [`line_head_prefix_of`]。
//!
//! ⚠️ **`:1039` 那一处不是「换个函数名」就完事** —— 它的问句必须一起改，
//! 否则会得到一条**恒假**的判据。理由写在源码 `:1055` 附近的注释里，
//! 本文件 `test_req068_glued_track_line_after_provenance_is_still_flagged`
//! 钉住它**确实还会开火**（恒假的判据 = 看起来有防护其实没有）。
//!
//! # 每条断言都对着一个变异
//!
//! | 测试 | 杀的变异 |
//! | --- | --- |
//! | `..._agent_name_carrying_track_prefix_...` | **M1**（`:1039` 改回 `contains`）、**M3**（`:1039` 改混合写法） |
//! | `..._glued_track_line_after_provenance_...` | 「收窄过头」—— 证明新判据**不是恒假** |
//! | `..._non_item_line_mentioning_prefix_before_provenance_...` | **M2**（`:1046` 改回 `contains`）、**M4**（`:1046` 改混合写法） |
//! | `..._pure_tightening_sample_counts` | **AC-003** 的包含论证 + 样本计数 |
//! | `..._no_second_contains_based_prefix_recogniser` | **AC-004** 复发守卫（源码文本探针） |
//! | `..._head_only_recognition_behaviourally` | **AC-004** 的语义兜底（不依赖源码文本） |
//! | `..._all_four_provenance_states` / `..._indented_...` | 「收窄过头，真归属行认不出来」 |
//! | `test_req074_first_item_detection_ignores_every_payload_derived_non_head_line` | **M1 / M2 / M4**（`:1046` 退回 `contains` / 别名绕过）、**M3**（守卫 + 那条构造样本**同时**改） |
//! | `test_req074_provenance_after_first_item_line_is_still_flagged` | 「`:1046` 那一格**恒不**开火」（整格删掉 / 恒返回 `None`） |
//!
//! # ⚠️ 关于 AC-004 那条守卫的**效力边界**（必读，不要误以为它能抓语义）
//!
//! `test_req068_no_second_contains_based_prefix_recogniser` 是**源码文本探针**：
//! 它读的是 `src/domain/review_view.rs` 的**文本**，断言「文件里没有第二处
//! 对这三类前缀做 `contains` 的实现」。
//!
//! **它能抓**：
//!
//! - 有人又写了 `line.contains(BIZ_LINE_PREFIX)`（**哪怕是死代码** —— 它数的是出现次数）；
//! - 同义写法 `contains(&BIZ_LINE_PREFIX)` / `contains(BIZ_LINE_PREFIX[..])`（匹配前先去掉 `&`）；
//! - 有人把 `line_head_prefix_of` 复制成第二份（定义数 ≠ 1）；
//! - 有人把 recogniser 内部的 `starts_with` 改回 `contains`；
//! - 有人把规则④ 分支 (a) 那处**唯一被论证过的** `contains` 挪出 `prov_lines.is_empty()` 保护区。
//!
//! **它抓不到**（**不是**能靠它防住的）：
//!
//! - **代码语义**。它证明不了 `line_head_prefix_of` 是对的，只证明「没有第二处 `contains`」；
//! - **别名绕过**。写成 `let p = BIZ_LINE_PREFIX; line.contains(p)` 它**抓不到**；
//! - **别的文件**。只覆盖 `review_view.rs`；在别的模块里再造一套前缀识别，它一声不吭；
//! - 换一种等价拼写（比如 `trim_start().starts_with` 被搬到 recogniser 之外内联）。
//!
//! ⇒ 所以它**必须**与语义测试
//! `test_req068_head_only_recognition_behaviourally` 配对使用 ——
//! 那条**不看源码**，只看渲染结果与判决。
//!
//! # ⚠️ 但语义兜底**只覆盖 `:1039`**，对 `:1046` 完全无感（实测，REQ-068 终验补）
//!
//! 兜底的输入是**真实渲染** —— 而 `:1046` 的见证样本（「归属行**之前**有一行
//! 行首不是轨、正文含两轨前缀」）在真实渲染里**根本不存在**
//! （渲染器把归属行固定打在最前，它前面只有不含 payload 的 `整体通过：` 行）。
//!
//! | 变异 | 语义兜底 | 本守卫 |
//! | --- | --- | --- |
//! | M1 / M3（`:1039` 退回 `contains`） | 🔴 红 | 🔴 红 |
//! | 别名绕过 `:1039` | 🔴 红 | 🟢 绿 |
//! | M2 / M4（`:1046` 退回 `contains`） | 🟢 **绿（无感）** | 🔴 红 |
//! | 别名绕过 `:1046` | 🟢 **绿（无感）** | 🟢 绿 |
//!
//! ⇒ **「抓不到」只在上面那几条里成立，整套并不是拦不住别名**：别名 `:1039`
//! 会让 2 条行为测试红；别名 `:1046` 会让 `..._non_item_line_...` 红（它用构造样本）。
//!
//! # ⚠️⚠️ REQ-074：上表**已过时**，两个位点的保护强度**现在对等可见**
//!
//! 上表描述的是 **REQ-074 之前**的状态。REQ-074 把 `:1046` 从「单点」变成「多点」
//! —— 当时 `:1046` 的全部保护力压在**两处**：一条文本守卫 + **一句手写字面量**
//! 的构造样本。**同时改掉这两处 ⇒ `:1046` 静默退化**（下面 M3/M4 实测复现）。
//!
//! ## 两个位点的差异与**原因**（AC-001）
//!
//! 差异**不在判据形状**（两处都是按行首认），而在**见证样本从哪来**：
//!
//! - **`:1039`** 问的是**归属行自己的正文**，正文里的 `agent=<名>` 是**提案方原样写的
//!   任意字符串** ⇒ `extracted_by` 就能让它带上两轨前缀 ⇒ **真实渲染输出**直接把它顶到
//!   「旧命中 / 新不挂」里 ⇒ **只喂真实渲染**的语义测试就分得出新旧。
//! - **`:1046`** 问的是「第一条条目行在第几行」，新旧写法**只在**
//!   「归属行**之前**有一行含两轨前缀、而它行首不是轨」时**才分道扬镳**；
//!   而渲染器把归属行固定打在**所有条目行之前**，它前面只有一行不含 payload 的
//!   `整体通过：` 行 ⇒ **在真实渲染输出上新旧同判** ⇒ 语义兜底对它**完全无感**。
//!
//! ⇒ `:1046` 的见证样本**只能**是构造文本。**这是已知边界、不是缺陷**
//! （它守的是「渲染器一改版就踩的坑」），但**护栏不该只有一个点**。
//!
//! ## `:1046` 现在的三道防线（AC-002）
//!
//! | 防线 | 性质 | 独立于 |
//! | --- | --- | --- |
//! | `..._no_second_contains_based_prefix_recogniser` | **源码文本探针** | 全部行为测试 |
//! | `..._non_item_line_mentioning_track_prefix_before_provenance_...` | 行为 · **一句手写字面量** | 守卫 |
//! | `test_req074_first_item_detection_ignores_every_payload_derived_non_head_line` | 行为 · **载荷由渲染器落到真实行上**（行框架来自渲染器，载荷文字仍是测试字面量） | 上面两条 |
//!
//! 第三道的载荷行由 [`render_review_text`] 从 `未通过原因：` / `被拦：` / `悬空关系：`
//! 三个出口打出来（行首不是轨、正文含轨前缀），再被搬到归属行之前的**每一个**位置。
//! ⇒ 「同时改掉守卫模式与那句构造样本」动不到它。
//!
//! ⚠️ **准确说清渲染器出了什么（QA N-4）**：出的是这些行的**框架** —— 缩进、
//! `未通过原因：` / `被拦：` / `悬空关系：[…]` 的前后缀都由渲染器打；
//! **行内那段载荷文字本身仍是测试写的字面量**（经 `ProposalStatus` 的字段进去）。
//! 本条依赖的是**行框架 + 落点**（行首不是轨、正文含轨前缀），那才是 `:1046` 区分新旧之处；
//! **不是**「载荷内容来自渲染器」—— 那个说法说过头了。
//!
//! | 变异 | REQ-074 **之前** | REQ-074 **之后** |
//! | --- | --- | --- |
//! | M1 `:1046` 退回 `contains` | 🔴 红（守卫 + 那条样本） | 🔴 红（守卫 + **两条**样本） |
//! | M2 别名绕过 `:1046` | 🔴 红（**仅**那条样本） | 🔴 红（**两条**样本） |
//! | **M3** 守卫模式 **+** 那条样本**同时**改 | 🟢 **绿 ⇒ 静默退化** | 🔴 **红** |
//! | **M4** M3 **叠加**删掉那条语料里的载荷样本（见下方口径说明） | 🟢 **绿 ⇒ 静默退化** | 🔴 **红** |
//! | M5 语义保持的重构 | 🟢 绿 | 🟢 绿 |
//!
//! ## ⚠️⚠️ 「M4」在本仓库有**三种口径** —— 查之前先看这一段（REQ-074 · N-1）
//!
//! `M1`~`M5` 这套编号是**按需求各自编号**的（REQ-063/068/069/070 各有自己的 M4），
//! 所以「M4」在不同文件里指的是**不同的变异**。本条涉及的三种，逐条列明：
//!
//! | 出处 | 它的 M4 指的是 | 备注 |
//! | --- | --- | --- |
//! | **REQ-068 原始口径**（本文件模块头 `:39`、`:474`） | `:1046` 改**混合写法**（`starts_with(..) \|\| contains(..)`） | REQ-068 原义，**未改动** |
//! | **本节 REQ-074 口径**（上方表格 / `src/domain/review_view.rs` 同一节的表格） | **M3 再叠加**「把 `..._pure_tightening_sample_counts` 里那条 `变异(归属前有含前缀的备注行)` 语料删掉」 | 为 M3 加码，使「语料里的载荷样本也一并丢失」 |
//! | **REQ-070 口径**（`test_review_view_req070.rs` 的 M4） | M3b 叠加语料外载荷丢失（M9′） | 与本条无关，**仅列出以免混淆** |
//!
//! ⚠️ 上表与 `src/domain/review_view.rs` 里 REQ-074 那节**用的是同一套口径**（M3/M4 均指「叠加删语料」那一组）。
//! REQ-068 原始口径的 M4（混合写法）**仍然有效**，由
//! [`test_req068_non_item_line_...`] 覆盖 —— 本条**没有**取代它，只是额外补了「叠加」那一格。
//!
//! ⚠️ **别把上表读成「别名绕过被封堵了」**：M2 指**语义被破坏**的别名；
//! 语义**未变**的别名（`let bp = BIZ_LINE_PREFIX;` 之后**仍按行首认**）**仍然绿**
//! —— 语义没变就没有任何东西该红。换名的第二套 recogniser（M2′）同理，见下。
//!
//! # ⚠️ REQ-073：**共用实现**之后，这道边界**一点没变**（实测，见下表）
//!
//! REQ-073 把判据的复刻删掉、改成**与生产共用同一份 [`line_head_prefix_of]`**。
//! 那消掉的是**复刻漂移**（判据改了而测试没改 ⇒ 守卫对着过时认知判断）。
//! **它没有**把本守卫从「文本探针」变成「语义检查」—— 它**仍然是文本探针**。
//!
//! | 变异（REQ-073 实测） | 结果 | 是谁抓到的 |
//! | --- | --- | --- |
//! | **M1** 改判据 recogniser 的实现（`starts_with`→`contains`） | 🔴 红（6 条） | 行为层 + 本守卫 |
//! | **M2** 第二处写近似 recogniser（**同名**） | 🔴 **编译错误** E0428 | 编译器 |
//! | **M2′** 第二处写近似 recogniser（**换名** `shadow_head_prefix_of`） | 🟢 **绿** | —— 无 |
//! | **M3** 语义**被破坏**的别名（`head.contains(p)`） | 🔴 红（6 条） | 行为层 |
//! | **M3′** 语义**不变**的别名（调用点 `starts_with(_b)`，`_b = BIZ_LINE_PREFIX`） | 🟢 **绿** | —— 无 |
//!
//! ⚠️ **两条绿的含义必须分开读**：
//! - **M2′**：① 只数**名字** `fn line_head_prefix_of(` ⇒ **换个名字的第二套实现它数不到**；
//! - **M3′**：调用点把共用 recogniser **整个绕开**，而它断言的全是**文本形状**
//!   ⇒ 语义没变就没有任何东西变红。
//!
//! ⇒ **本条不声称封堵别名绕过。** 「共用实现抓不到别名绕过」是**已登记的已知边界**，
//! 归 **REQ-075（守卫的守卫）**；「换名的第二套实现数不到」同理。
//! 本条只关掉**复刻漂移**那一格。
//!
//! # REQ-075 之后：本守卫的边界**没有变宽**，只补了一个「自己不知道自己失效」的洞
//!
//! REQ-075 在这里改的**只有锚点**（[`def_line_of`] → [`def_head`]）：
//! REQ-073 只认 `pub ` / `pub(crate) `，**受限可见性**（`pub(in crate::…)` /
//! `pub(super)` / `pub(self)`）一出现，④ 就**静默跳过、守卫照样绿**。
//! ⇒ 改成**按语法剥前缀**，并补了自检变异 **M7**。
//!
//! ⚠️ **照实说，M7 修的不是「守卫抓不到」**，是「**守卫不知道自己抓不到了**」。
//! 上表里那两条 🟢 —— M2′（换名的第二套实现）与 M3′（语义不变的别名）——
//! **REQ-075 之后仍然绿**。REQ-075 在 REQ-070 那边补的是**行为扫描网**，
//! 那张网只扫 `EvidenceDegeneracy::label`，**盖不到本文件的前缀识别**。
//! ⇒ **本文件仍然不声称封堵别名绕过。**

use ontok::domain::review::{Decision, EntryState, ItemKind, ItemVerdict, ItemView, ProposalStatus};
use ontok::domain::review_view::{
    line_head_prefix_of, project_review_track_with_provenance, project_review_track_with_summary,
    project_review_track_with_summary_and_provenance, render_review_text, verify_provenance_scope,
    verify_track_separation, BIZ_LINE_PREFIX, MODEL_LINE_PREFIX, PROVENANCE_LINE_PREFIX,
    ReviewTrackView,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

// ── fixture ──────────────────────────────────────────────

fn baseline() -> BTreeSet<String> {
    ["customer".to_string(), "subscription".to_string()].into_iter().collect()
}

/// 一个**建模栏齐备**的条目 —— 展示名与证据 claim 都是 payload 原样文本。
fn full_item(target: &str, payload: Value) -> ItemView {
    ItemView {
        id: format!("1:add_entity_type:{target}"),
        kind: ItemKind::AddEntityType,
        target: target.into(),
        from_type: None,
        to_type: None,
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload,
    }
}

/// 三条目 —— N > 1 是 REQ-058 立的规矩：**「逐条目重复打印」那个误导
/// 只有 N > 1 时才现形**，单条目夹具会让规则①恒真。
fn three_items(target: &str, payload: Value) -> Vec<ItemView> {
    (0..3)
        .map(|i| {
            let mut it = full_item(target, payload.clone());
            it.id = format!("{i}:add_entity_type:{target}");
            it
        })
        .collect()
}

fn declared_eb() -> Value {
    json!({ "agent": "mcode-agent", "version": "2026.10.07", "round": 1, "failed_groups": [] })
}

fn eb_with_agent(agent: &str) -> Value {
    json!({ "agent": agent, "version": "2026.10.07", "round": 1, "failed_groups": [] })
}

fn plain_payload() -> Value {
    json!({
        "key": "customer",
        "properties": [{ "name": "客户编号", "column": "customer_pk" }],
        "evidence": [{ "table": "t_customer", "column": "customer_no", "claim": "客户编号唯一" }]
    })
}

/// 投影与渲染**一次**产出（不自造 rendered）。
fn view_and_text(items: &[ItemView], extracted_by: Option<&Value>) -> (ReviewTrackView, String) {
    let (view, _s) = project_review_track_with_summary_and_provenance(items, &baseline(), extracted_by);
    let view = view;
    let text = render_review_text(&view);
    (view, text)
}

/// **手工**给某个条目塞一条 `reason`（`未通过原因：…` 那一行）。
///
/// ⚠️ `summarize` 不会产出任意 reason，只有**被拒**的条目才有；
/// 这里直接构造 [`ProposalStatus`]（字段全是 pub），
/// 好让 `未通过原因` 这一行**由真实渲染器**打出来 —— 它是
/// payload 派生文本落在**非条目行**上的真实样本（AC-003 的计数靠它）。
fn view_and_text_with_reason(
    items: &[ItemView],
    extracted_by: Option<&Value>,
    reason_of_item1: &str,
) -> (ReviewTrackView, String) {
    let status = ProposalStatus {
        passed: true,
        verdicts: items
            .iter()
            .enumerate()
            .map(|(i, it)| ItemVerdict {
                id: it.id.clone(),
                target: it.target.clone(),
                state: EntryState::Passed,
                reason: (i == 0).then(|| reason_of_item1.to_string()),
            })
            .collect(),
        applicable: items.iter().map(|it| it.id.clone()).collect(),
        blocked: vec![],
        dangling_relations: vec![],
    };
    let view = project_review_track_with_provenance(items, &status, extracted_by);
    let text = render_review_text(&view);
    (view, text)
}

/// REQ-074：让**真实渲染器**打出尾部三类 payload 派生行 ——
/// `未通过原因：` / `被拦：` / `悬空关系：`。
///
/// # 为什么需要它
///
/// 这三行的**行首**既不是两轨前缀、也不是归属前缀，而**正文**里带着
/// 提案方自己写的两轨前缀 ⇒ 它们落在 `:1046`「旧命中 / 新不挂」的那一格。
///
/// ⚠️ **准确说清渲染器出了什么（QA N-4）**：这三行的**框架**是渲染器打的 ——
/// 缩进、`未通过原因：` / `被拦：` / `悬空关系：[…]` 的前后缀都来自 [`render_review_text`]；
/// **行内那段载荷文字仍是本测试传进来的字面量**（经 [`ProposalStatus`] 的三个字段）。
/// 本条依赖的是**行框架 + 落点**，不是载荷内容本身。
///
/// ⚠️ [`render_review_text`] 把归属行固定打在**所有条目行之前**，
/// 而它前面只有一行不含 payload 的 `整体通过：` ⇒ `:1046` 在真实输出上
/// **新旧同判**（真正的原因见本文件模块注释里 REQ-074 那节）。
/// 唯一能区分两者的形态是「归属行**之前**有一行含轨前缀、但行首不是轨」，
/// 而那种形态**只能靠把渲染器打出来的这一行搬到归属行之前**来造 ——
/// 这正是 [`test_req074_first_item_detection_ignores_every_payload_derived_non_head_line`]
/// 干的事。⇒ 本 helper 是那条测试的**唯一载荷来源**。
fn view_and_text_with_tail_payload(
    items: &[ItemView],
    extracted_by: Option<&Value>,
    reason_of_item1: &str,
    blocked_reason: &str,
    dangling: &str,
) -> (ReviewTrackView, String) {
    let verdict = |it: &ItemView, state: EntryState, reason: Option<&str>| ItemVerdict {
        id: it.id.clone(),
        target: it.target.clone(),
        state,
        reason: reason.map(str::to_string),
    };
    let status = ProposalStatus {
        passed: true,
        verdicts: items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                verdict(
                    it,
                    EntryState::Passed,
                    (i == 0).then_some(reason_of_item1),
                )
            })
            .collect(),
        applicable: items.iter().map(|it| it.id.clone()).collect(),
        blocked: vec![verdict(&items[0], EntryState::Pending, Some(blocked_reason))],
        dangling_relations: vec![dangling.to_string()],
    };
    let view = project_review_track_with_provenance(items, &status, extracted_by);
    let text = render_review_text(&view);
    (view, text)
}

/// 「这一行的行首（去掉缩进）是哪个前缀」—— **直接调用生产实现**。
///
/// # ⚠️ REQ-073：这里**曾经**有一份 `head_prefix_of` 的**复刻**
///
/// 原写法在本文件里**另写了一份** [`ontok::domain::review_view::line_head_prefix_of`]
/// 的近似实现，只服务于「包含论证 / 样本计数 / 前提断言」。
/// 病根是**判据改了而复刻没改** ⇒ 守卫会**对着过时的认知**判断
/// （AC-001 要消除的正是这一份，不是「文本探针」本身）。
///
/// 现在**直接 `use` 生产那一份** ⇒ 判据与守卫**不可能再分叉**。
/// 代价与理由见 AC-002：`tests/*.rs` 是集成测试、只看得到 `pub`，
/// 所以生产侧是 `#[doc(hidden)] pub`（另一条路「移进 `#[cfg(test)] mod tests`」
/// 会让守卫的 `include_str!` **读到它自己的字面量**，定义数变成 3、恒红）。
#[inline]
fn head_prefix_of(line: &str) -> Option<&'static str> {
    line_head_prefix_of(line)
}

fn is_track_head(line: &str) -> bool {
    matches!(head_prefix_of(line), Some(BIZ_LINE_PREFIX) | Some(MODEL_LINE_PREFIX))
}

fn is_prov_head(line: &str) -> bool {
    head_prefix_of(line) == Some(PROVENANCE_LINE_PREFIX)
}

/// 旧写法：整行包含。
fn old_track_predicate(line: &str) -> bool {
    line.contains(BIZ_LINE_PREFIX) || line.contains(MODEL_LINE_PREFIX)
}

/// 新写法（`:1046`）：行首认。
fn new_track_predicate(line: &str) -> bool {
    is_track_head(line)
}

/// 旧写法（`:1039`）：整行包含（问的是**归属行本身**）。
fn old_prov_track_predicate(line: &str) -> bool {
    line.contains(BIZ_LINE_PREFIX) || line.contains(MODEL_LINE_PREFIX)
}

/// 新写法（`:1039`）：**去掉归属前缀后的正文**行首是不是某一轨。
fn new_prov_track_predicate(line: &str) -> bool {
    let body = line.trim_start().strip_prefix(PROVENANCE_LINE_PREFIX).unwrap_or_default();
    matches!(head_prefix_of(body), Some(BIZ_LINE_PREFIX) | Some(MODEL_LINE_PREFIX))
}

// ── AC-002 · `:1039` ──────────────────────────────────────

/// 【AC-002 · 杀 M1 / M3】**`extracted_by.agent` 里含「业务轨：」⇒ 零违规**。
///
/// # 杀的是哪个变异
///
/// - **M1**：`:1039` 改回 `line.contains(BIZ_LINE_PREFIX) || line.contains(MODEL_LINE_PREFIX)`；
/// - **M3**：`:1039` 改成 `starts_with(..) || contains(..)` 的**混合写法**
///   （BA 在 REQ-058 里栽过的同一个坑）。
///
/// # 为什么 `agent` 是对的施暴点
///
/// `agent` 被 [`render_provenance_line`] 打在**归属行**上（`agent=<名>`），
/// 而它是**提案方自报的任意字符串**（REQ-069 的转义只管 LF/CR 与退化标记，
/// 管不到「业务轨：」这几个字）⇒ 归属行**自己**就带着两轨前缀。
/// 旧写法会把这一行误判成「归属行混进了两轨前缀」—— **误报**。
#[test]
fn test_req068_agent_name_carrying_track_prefix_is_not_a_false_positive() {
    let items = three_items("客户", plain_payload());
    let (v, text) = view_and_text(&items, Some(&eb_with_agent(&format!("{BIZ_LINE_PREFIX}执行者"))));

    // ── 前提：样本**真的**落在「旧命中、新不命中」里（否则这条是恒真的） ──
    let prov_line = text
        .lines()
        .find(|l| is_prov_head(l))
        .unwrap_or_else(|| panic!("渲染里必须有一条行首归属行\n实际输出：\n{text}"));
    assert!(
        old_prov_track_predicate(prov_line),
        "前提不成立：归属行本身没有含两轨前缀（旧写法压根不会开火 ⇒ 这条测不到任何东西）\n归属行：{prov_line}"
    );
    assert!(
        !is_track_head(prov_line),
        "前提不成立：归属行行首被认成了两轨行（那测的是别的东西）\n归属行：{prov_line}"
    );
    assert!(
        !new_prov_track_predicate(prov_line),
        "前提不成立：新判据竟然命中了 ⇒ 这个样本不再是「旧过新挂」\n归属行：{prov_line}"
    );

    let violations = verify_provenance_scope(&v, &text);
    assert!(
        violations.is_empty(),
        "agent 名里带「{BIZ_LINE_PREFIX}」被判成「归属行混进两轨前缀」（误报）：{violations:#?}\n实际输出：\n{text}"
    );
    // 对照组：两轨判据也不该被牵连。
    let tv = verify_track_separation(&v, &text);
    assert!(tv.is_empty(), "两轨分界被判违规：{tv:#?}\n实际输出：\n{text}");
}

/// 【不变量】**新判据不是恒假**：归属行后面**真的**粘着一条轨行时，仍必须被抓。
///
/// # 为什么必须有这条
///
/// `:1039` 若照字面改成 `line_head_prefix_of(line) == Some(BIZ_LINE_PREFIX)`，
/// 它会**恒假**（`line` 取自 `prov_lines`，而 `prov_lines` 就是按
/// 「行首是归属前缀」筛出来的 ⇒ 恒等于 `Some(PROVENANCE_LINE_PREFIX)`）。
/// **恒假的判据 = 看起来有防护其实没有** —— 本项目反复栽过的形态。
/// 这条断言是新判据**确实还会开火**的证据。
#[test]
fn test_req068_glued_track_line_after_provenance_is_still_flagged() {
    let items = three_items("客户", plain_payload());
    let (v, good) = view_and_text(&items, Some(&declared_eb()));

    // 对照组没过 ⇒ 后面「必须开火」这句话是空的。
    assert!(
        verify_provenance_scope(&v, &good).is_empty(),
        "对照组没过 ⇒ 判据恒为真，下面那条证明不了任何东西\n实际输出：\n{good}"
    );

    let prov_line = good
        .lines()
        .find(|l| is_prov_head(l))
        .unwrap_or_else(|| panic!("未变异输出里必须有行首归属行\n实际输出：\n{good}"))
        .to_string();
    // 变异：把一条**业务轨行**粘在归属前缀**后面**（正文行首立刻就是两轨前缀），
    // 归属正文本身保留 ⇒ 规则③ 的表态词仍在，不引入噪声。
    let body = prov_line.trim_start().strip_prefix(PROVENANCE_LINE_PREFIX).unwrap();
    let glued_line = format!("{PROVENANCE_LINE_PREFIX}{BIZ_LINE_PREFIX}客户 · biz=accepted · {body}");
    let mutant = good
        .lines()
        .map(|l| if l == prov_line { glued_line.clone() } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    // 前提：新判据在这个样本上**确实命中**（否则下面断言的是恒假的判据）。
    assert!(
        new_prov_track_predicate(&glued_line),
        "前提不成立：变异样本没落在新判据里（判据恒假 ⇒ 本条测试无效）\n变异行：{glued_line}"
    );

    let violations = verify_provenance_scope(&v, &mutant);
    assert!(
        !violations.is_empty(),
        "归属行后面直接粘着一条业务轨行没有被抓住（收窄过头）：\n变异输出：\n{mutant}"
    );
    assert_eq!(
        violations.len(),
        1,
        "应当**只有**「粘着两轨行」这一条违规（说明规则①③④ 都还在正常判）：{violations:#?}\n变异输出：\n{mutant}"
    );
    assert!(
        violations[0].contains("两轨前缀"),
        "违规理由没指出是「两轨前缀」那一条：{violations:?}\n变异输出：\n{mutant}"
    );
}

// ── AC-002 · `:1046` ──────────────────────────────────────

/// 【AC-002 · 杀 M2 / M4】**归属行之前有一行「行首不是轨、正文含两轨前缀」⇒ 零违规**。
///
/// # 杀的是哪个变异
///
/// - **M2**：`:1046` 改回 `.position(|l| l.contains(..) || l.contains(..))`；
/// - **M4**：`:1046` 改混合写法（`starts_with(..) || contains(..)`）。
///
/// 旧写法会把那一行当成「**第一条条目行**」⇒ `first_item` 偏小
/// ⇒ 归属行被误报成「排在条目行之后」。
///
/// ⚠️ **样本是构造的 rendered 文本**，原因如实说明：
/// 真实渲染器固定把归属行打在**所有条目行之前**，而它前面只有一行
/// `整体通过：是/否`（不含 payload 文本）⇒ **在当前渲染器的输出上，
/// 旧写法与新写法逐条同判**。这条守的是**渲染器一改版**（把归属行排到条目区之后、
/// 或在前面加一行摘要/备注）就会踩到的坑 —— 那正是规则② 自己文档里写的
/// 「抓：塞进某一条的行 / 排到条目区里」。真实渲染器产出的同类样本由
/// `test_req068_pure_tightening_sample_counts` 的「未通过原因含两轨前缀」覆盖。
#[test]
fn test_req068_non_item_line_mentioning_track_prefix_before_provenance_is_not_a_false_positive() {
    let items = three_items("客户", plain_payload());
    let (v, good) = view_and_text(&items, Some(&declared_eb()));

    let noisy = format!("  备注：这里提到 {BIZ_LINE_PREFIX} 但本行行首不是任何前缀");
    let mut ls: Vec<String> = good.lines().map(str::to_string).collect();
    let prov_at = ls
        .iter()
        .position(|l| is_prov_head(l))
        .unwrap_or_else(|| panic!("未变异输出里必须有行首归属行\n实际输出：\n{good}"));
    ls.insert(prov_at, noisy.clone());
    let crafted = ls.join("\n") + "\n";

    // ── 前提一：这一行确实是「旧命中、新不命中」 ──
    assert!(old_track_predicate(&noisy), "前提不成立：备注行不含两轨前缀\n备注行：{noisy}");
    assert!(!new_track_predicate(&noisy), "前提不成立：备注行被新判据认成了条目行\n备注行：{noisy}");

    // ── 前提二：旧写法会把归属行误报成「排在条目行之后」，新写法不会 ──
    let old_first = crafted.lines().position(old_track_predicate);
    let prov_idx = crafted
        .lines()
        .position(is_prov_head)
        .unwrap_or_else(|| panic!("构造文本里必须有行首归属行\n实际输出：\n{crafted}"));
    assert!(old_first.is_some(), "前提不成立：找不到行\n实际输出：\n{crafted}");
    assert!(
        old_first < Some(prov_idx),
        "前提不成立：旧写法压根不会误报（第一处命中在归属行之后）⇒ 这条测不到任何东西\n实际输出：\n{crafted}"
    );
    let new_first = crafted.lines().position(new_track_predicate);
    assert!(
        new_first.map_or(true, |nf| nf > prov_idx),
        "前提不成立：新写法也会把归属行判成排在条目行之后\n实际输出：\n{crafted}"
    );

    let violations = verify_provenance_scope(&v, &crafted);
    assert!(
        violations.is_empty(),
        "一条只是**提到**两轨前缀的备注行被当成了第一条条目行 ⇒ 归属行被误报成「排在条目行之后」：\
         {violations:#?}\n实际输出：\n{crafted}"
    );
}

// ── REQ-074 · AC-002 · `:1046` 的**语义兜底** ────────────────
//
// ⚠️ `:1039` 与 `:1046` 的**保护强度不对等**（原因写在 `src/domain/review_view.rs`
// 与本文件模块注释里），下面两条把 `:1046` 从「单点」变成「多点」：
//
// | 防线 | 性质 | 独立于 |
// | --- | --- | --- |
// | `..._non_item_line_...`（REQ-068） | 行为 · **一句手写字面量** | 守卫 |
// | `test_req074_first_item_detection_ignores_every_payload_derived_non_head_line` | 行为 · **载荷由渲染器落到真实行上** | 上条 + 守卫 |
// | `test_req074_provenance_after_first_item_line_is_still_flagged` | 行为 · **反向变异**（证明这一格**确实还会开火**） | 上面两条 |

/// 【REQ-074 · AC-002 · 杀 M1 / M2 / M3 / M4】**载荷行搬到哪里都不许变成「第一条条目行」**。
///
/// # ⚠️ 先说清这里的「M4」是**哪一种**（REQ-074 · N-1：M4 有三套口径）
///
/// 本条下面列的 **M4 = `:1046` 改混合写法**（`starts_with(..) || contains(..)`），
/// 即 **REQ-068 原始口径**（同 [`test_req068_non_item_line_...`]）。
/// ⚠️ **模块头 REQ-074 那一节的 M4 不是这个** —— 那里指「M3 再叠加删掉语料里的载荷样本」。
/// 两处各自都写了「口径」并互相指路；**改动任何一处之前先读模块头那段口径表。**
///
/// # 杀的是哪几个变异
///
/// - **M1**：`:1046` 退回 `.position(|l| l.contains(..) || l.contains(..))`；
/// - **M2**：`:1046` 别名绕过（`let bp = BIZ_LINE_PREFIX; l.contains(&bp)`）—— **语义与 M1 相同**；
/// - **M4**（REQ-068 口径）：`:1046` 改混合写法（`starts_with(..) || contains(..)`）；
/// - **M3**：守卫模式**与** `..._non_item_line_...` 那条构造样本**同时**被改掉。
///   ⇒ 本条**既不读源码、也不用那句手写字面量**，所以 M3 动不到它。
///
/// # 为什么是「网」而不是「又加一条样本」
///
/// 载荷行**不是测试自己拼的一整行**： [`view_and_text_with_tail_payload`] 让
/// [`render_review_text`] 打出 `未通过原因：` / `被拦：` / `悬空关系：` 的**行框架**，
/// 再把测试写的载荷文字放进去 —— **行首不是轨、正文含轨前缀**，
/// 正是 `:1046`「旧命中 / 新不挂」的那一格。
/// 本条把**每一条**这样的行，插到归属行之前的**每一个**位置，逐份核判决。
///
/// ⚠️ **准确说清「渲染器出了什么」（QA N-4）**：渲染器出的是这一行的**框架**
/// —— `      未通过原因：` / `被拦：` / `悬空关系：[…]` 的前缀、缩进与后缀都是它打的；
/// 而**行内那段载荷文字本身仍是测试写的字面量**（`因为 {BIZ_LINE_PREFIX} 不匹配` 等，
/// 经 `ProposalStatus` 三个字段进去）。所以它的性质是
/// **「载荷由渲染器落到真实行上」**，**不是**「载荷内容来自渲染器」。
/// 本条依赖的是**行框架 + 落点**（行首不是轨、正文含轨前缀），
/// 那才是 `:1046` 区分新旧的地方 —— 不是载荷文字本身。
///
/// ⚠️ **它不覆盖 M5（`:1046` 保留行首语义的重构）** —— 语义没变就不该红。
/// 也不覆盖「把 `position` 整格删掉」—— 那是
/// [`test_req074_provenance_after_first_item_line_is_still_flagged`] 的活。
///
/// # 前提断言写在**每一份**构造文本上（防这条退化成恒真）
///
/// - 未变异的真实输出必须零违规；
/// - 见证行必须落在「旧命中 / 新不挂」里（`new_track_predicate` 走**生产**实现）；
/// - **旧写法在这份构造文本上真的会误报**（`old_first < prov_idx`）——
///   否则「零违规」只是「两条判据都不命中」的恒真；
/// - 见证行**条数**与**核过的位置数**都有下限（网不许缩成单点）。
#[test]
fn test_req074_first_item_detection_ignores_every_payload_derived_non_head_line() {
    let items = three_items("客户", plain_payload());

    // 两份语料，BIZ / MODEL 的载荷各来一遍 ⇒ 见证行互不相同。
    let cases: Vec<(String, ReviewTrackView, String)> = vec![
        {
            let (v, t) = view_and_text_with_tail_payload(
                &items,
                Some(&declared_eb()),
                &format!("因为 {BIZ_LINE_PREFIX} 不匹配"),
                &format!("依赖 {MODEL_LINE_PREFIX} 不满足"),
                &format!("{BIZ_LINE_PREFIX}悬空"),
            );
            ("BIZ 载荷".to_string(), v, t)
        },
        {
            let (v, t) = view_and_text_with_tail_payload(
                &items,
                Some(&declared_eb()),
                &format!("因为 {MODEL_LINE_PREFIX} 不匹配"),
                &format!("依赖 {BIZ_LINE_PREFIX} 不满足"),
                &format!("{MODEL_LINE_PREFIX}悬空"),
            );
            ("MODEL 载荷".to_string(), v, t)
        },
    ];

    let mut witnesses: BTreeSet<String> = BTreeSet::new();
    let mut checked = 0usize;

    for (name, v, text) in &cases {
        // ── 前提 0：未变异的真实输出本身必须干净 ──
        let base = verify_provenance_scope(v, text);
        assert!(
            base.is_empty(),
            "前提不成立：{name} 的真实输出就不干净 ⇒ 下面每一份断言都是空的\n{base:#?}\n实际输出：\n{text}"
        );

        let prov_idx = text
            .lines()
            .position(is_prov_head)
            .unwrap_or_else(|| panic!("前提不成立：{name} 的真实输出没有行首归属行\n实际输出：\n{text}"));

        // ── 前提 1：这份真实输出里**真的**有「旧命中、新不挂」的非归属行 ──
        //
        // ⚠️ `new_track_predicate` → `is_track_head` → **生产** `line_head_prefix_of`，
        // 这里量的是**生产行为**，不是测试自己那份复刻。
        let local: Vec<&str> = text
            .lines()
            .filter(|l| old_track_predicate(l) && !new_track_predicate(l) && !is_prov_head(l))
            .collect();
        assert!(
            !local.is_empty(),
            "前提不成立：{name} 的真实输出里没有「行首不是轨、正文含轨前缀」的非归属行\n\
             ⇒ 这条测不到任何东西（网退化成空集 ⇒ 恒真）\n实际输出：\n{text}"
        );

        for w in local {
            witnesses.insert(w.to_string());
            let ls: Vec<String> = text.lines().map(str::to_string).collect();
            // 插到归属行之前的**每一个**位置（含最前面那个）。
            for at in (0..=prov_idx).rev() {
                let mut crafted_ls = ls.clone();
                crafted_ls.insert(at, w.to_string());
                let crafted = crafted_ls.join("\n") + "\n";

                // ── 前提 2：旧写法在这份构造文本上**真的会误报** ──
                let old_first = crafted.lines().position(old_track_predicate);
                let cprov = crafted
                    .lines()
                    .position(is_prov_head)
                    .unwrap_or_else(|| panic!("构造文本里必须有行首归属行\n实际输出：\n{crafted}"));
                assert!(
                    old_first.is_some_and(|f| f < cprov),
                    "前提不成立：旧写法在这个样本上压根不会误报（第一处命中不在归属行之前）\n\
                     ⇒ 「零违规」是恒真的，这一份证明不了任何东西\n\
                     载荷行：{w}\n插入位置：{at}\n实际输出：\n{crafted}"
                );

                let violations = verify_provenance_scope(v, &crafted);
                assert!(
                    violations.is_empty(),
                    "一条只是**提到**两轨前缀的 {name} 载荷行被当成了第一条条目行\n\
                     ⇒ `:1046` 退回/绕过了行首认（归属行被误报成「排在条目行之后」）：{violations:#?}\n\
                     载荷行：{w}\n插入位置：{at}\n实际输出：\n{crafted}"
                );
                checked += 1;
            }
        }
    }

    // ── 前提 3：这张网不许缩成「一两句话面量」 ──
    assert!(
        witnesses.len() >= 4,
        "见证行只有 {} 条 ⇒ 这张网退化成了单点（要求 ≥4 条来自不同渲染出口的载荷行）",
        witnesses.len()
    );
    assert!(
        checked >= 8,
        "只核了 {checked} 份构造文本 ⇒ 覆盖不足（网退化成单点）"
    );
    println!(
        "REQ-074 `:1046` 语义兜底 —— 见证行 {} 条 · 构造文本 {checked} 份 · 语料 {} 份",
        witnesses.len(),
        cases.len()
    );
}

/// 【REQ-074 · 反向变异】**归属行真的排在条目行之后 ⇒ 必须报红**。
///
/// # 为什么必须有这条（它是本条发现的**第二个洞**）
///
/// 规则② 后半句（`:1046` 那一格）在**整个测试目录里没有任何一条断言说它会开火** ——
/// 搜 `排在条目行` 只搜得到注释。⇒ 下面三种退化**全都绿**：
///
/// 1. 把 `.position(..)` 整格删掉（`if let` 永不命中 ⇒ 归属行排到条目区之后也不报）；
/// 2. 把它改成恒返回 `Some(0)`；
/// 3. 把它改成恒返回 `None`。
///
/// 这与 REQ-074 修的 `:1039` 那个洞**同形**：
/// [`test_req068_glued_track_line_after_provenance_is_still_flagged`] 为 `:1039`
/// 钉了「不是恒假」，`:1046` 一直没有对应的一条。
///
/// # 怎么造这个形态
///
/// 真实渲染器把归属行固定打在**所有条目行之前** ⇒ 这个违规形态在生产输出上
/// **不存在**。只能把**真实渲染出来的**归属行**搬到条目区之后** ——
/// 归属行本身一个字都没改，改的只是它在文本里的位置。
#[test]
fn test_req074_provenance_after_first_item_line_is_still_flagged() {
    let items = three_items("客户", plain_payload());
    let (v, good) = view_and_text(&items, Some(&declared_eb()));

    // 对照组没过 ⇒ 下面那条证明不了任何东西。
    assert!(
        verify_provenance_scope(&v, &good).is_empty(),
        "对照组没过 ⇒ 判据恒为真，下面「必须开火」证明不了任何东西\n实际输出：\n{good}"
    );

    let ls: Vec<String> = good.lines().map(str::to_string).collect();
    let prov_at = ls
        .iter()
        .position(|l| is_prov_head(l))
        .unwrap_or_else(|| panic!("未变异输出里必须有行首归属行\n实际输出：\n{good}"));
    let prov_line = ls[prov_at].clone();
    let mut moved = ls.clone();
    moved.remove(prov_at);
    moved.push(prov_line.clone());
    let crafted = moved.join("\n") + "\n";

    // ── 前提：归属行真的排在第一条条目行之后（且归属行只有一条） ──
    assert_eq!(
        crafted.lines().filter(|l| is_prov_head(l)).count(),
        1,
        "前提不成立：构造文本里归属行不是恰好一条 ⇒ 规则① 会先报，量到的就不是 `:1046`\n实际输出：\n{crafted}"
    );
    let first_item = crafted
        .lines()
        .position(new_track_predicate)
        .unwrap_or_else(|| panic!("前提不成立：构造文本里没有条目行\n实际输出：\n{crafted}"));
    let cprov = crafted.lines().position(is_prov_head).unwrap();
    assert!(
        cprov > first_item,
        "前提不成立：归属行不在条目行之后 ⇒ 这条测不到 `:1046`\n实际输出：\n{crafted}"
    );

    let violations = verify_provenance_scope(&v, &crafted);
    assert_eq!(
        violations.len(),
        1,
        "应当**只有**「归属行排在条目行之后」这一条违规（说明规则①③④ 都还在正常判，\
         量到的确实是 `:1046` 那一格）：{violations:#?}\n变异输出：\n{crafted}"
    );
    assert!(
        violations[0].contains("排在条目行"),
        "违规理由不是「排在条目行之后」那一条：{violations:?}\n变异输出：\n{crafted}"
    );
}

// ── AC-003 · 纯收紧：包含论证 + 样本计数 ─────────────────

/// 【AC-003】两个判据各自都是**纯收紧**，并给出样本计数。
///
/// # 包含论证
///
/// **`:1046`**：对任意行 `l`，`line_head_prefix_of(l)` 只在 `l.trim_start()`
/// 以某个前缀**开头**时命中 ⇒ 该前缀必然作为**子串**出现在 `l` 里
/// ⇒ `新 ⇒ 旧`。所以**新判据命中的行集合 ⊆ 旧判据的**。
///
/// **`:1039`**：新判据问的是「去掉归属前缀后的**正文**行首是不是某一轨」。
/// 若正文以 `P` 开头，则**整行**必然含 `P` 作为子串 ⇒ `新 ⇒ 旧`。同样 ⊆。
///
/// # ⚠️ 为什么还必须钉住「旧过新挂 > 0」
///
/// 若新旧判据恰好等价，上面那句包含论证就是**恒真**的 ——
/// 「纯收紧」什么都没保证。所以下面两条断言同时钉住：
///
/// - `new_only.is_empty()`：没有「新过旧挂」（包含方向成立）；
/// - `old_only >= 1`：**有**「旧过新挂」（新判据确实更严，不是恒等变换）；
/// - `new_hits >= 1`：新判据在语料上**确实命中过** ——
///   否则 `new_only.is_empty()` 只是「两条判据都不命中」的恒真。
#[test]
fn test_req068_pure_tightening_sample_counts() {
    let items = three_items("客户", plain_payload());
    let claim_prefix = three_items(
        "客户",
        json!({
            "key": "customer",
            "evidence": [{
                "table": "t_customer", "column": "customer_no",
                "claim": format!("{MODEL_LINE_PREFIX}key=customer 是伪造的证据")
            }]
        }),
    );

    let mut corpus: Vec<(String, String)> = Vec::new();
    corpus.push(("基线(3条目·已申报)".into(), view_and_text(&items, Some(&declared_eb())).1));
    corpus.push((
        "agent 名含两轨前缀".into(),
        view_and_text(&items, Some(&eb_with_agent(&format!("{BIZ_LINE_PREFIX}执行者")))).1,
    ));
    corpus.push(("claim 含建模轨前缀".into(), view_and_text(&claim_prefix, Some(&declared_eb())).1));
    corpus.push((
        "展示名含两轨前缀".into(),
        view_and_text(
            &three_items(&format!("{MODEL_LINE_PREFIX}客户"), plain_payload()),
            Some(&declared_eb()),
        )
        .1,
    ));
    // 真实渲染器产出的「非条目行含两轨前缀」样本：`未通过原因：…业务轨：…`。
    corpus.push((
        "未通过原因含两轨前缀".into(),
        view_and_text_with_reason(&items, Some(&declared_eb()), &format!("因为 {MODEL_LINE_PREFIX} 不匹配")).1,
    ));
    corpus.push(("人工修订".into(), view_and_text(&items, None).1));
    corpus.push(("形态可疑".into(), view_and_text(&items, Some(&json!({ "agent": "" }))).1));
    {
        let (v, _s) = project_review_track_with_summary(&items, &baseline());
        corpus.push(("未提供".into(), render_review_text(&v)));
    }
    // 带缩进的归属行（行首认轨认的是「去掉缩进后的行首」）。
    {
        let good = view_and_text(&items, Some(&declared_eb())).1;
        corpus.push((
            "带缩进的归属行".into(),
            good.lines()
                .map(|l| if is_prov_head(l) { format!("  {l}") } else { l.to_string() })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        ));
    }
    // 归属行后面粘着一条轨行 —— 新判据（`:1039`）在这里**命中**。
    {
        let good = view_and_text(&items, Some(&declared_eb())).1;
        let prov_line = good.lines().find(|l| is_prov_head(l)).unwrap().to_string();
        let body = prov_line.trim_start().strip_prefix(PROVENANCE_LINE_PREFIX).unwrap();
        corpus.push((
            "变异(归属后粘着轨行)".into(),
            good.lines()
                .map(|l| {
                    if l == prov_line {
                        format!("{PROVENANCE_LINE_PREFIX}{BIZ_LINE_PREFIX}客户 · biz=accepted · {body}")
                    } else {
                        l.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        ));
    }
    // 归属行**之前**有一行「行首不是轨、正文含两轨前缀」 —— `:1046` 的差异样本。
    {
        let good = view_and_text(&items, Some(&declared_eb())).1;
        let noisy = format!("  备注：这里提到 {BIZ_LINE_PREFIX} 但本行行首不是任何前缀");
        let mut ls: Vec<String> = good.lines().map(str::to_string).collect();
        let at = ls.iter().position(|l| is_prov_head(l)).unwrap();
        ls.insert(at, noisy);
        corpus.push(("变异(归属前有含前缀的备注行)".into(), ls.join("\n") + "\n"));
    }

    // ── `:1039` 的计数 ──
    let mut total_a = 0usize;
    let mut old_only_a = 0usize;
    let mut new_only_a: Vec<String> = Vec::new();
    let mut new_hits_a = 0usize;
    // ── `:1046` 的计数 ──
    let mut total_b = 0usize;
    let mut old_only_b = 0usize;
    let mut new_only_b: Vec<String> = Vec::new();
    let mut new_hits_b = 0usize;

    for (name, text) in &corpus {
        for l in text.lines() {
            total_b += 1;
            match (new_track_predicate(l), old_track_predicate(l)) {
                (true, true) => new_hits_b += 1,
                (true, false) => new_only_b.push(format!("{name} :: {l}")),
                (false, true) => old_only_b += 1,
                (false, false) => {}
            }
            if is_prov_head(l) {
                total_a += 1;
                match (new_prov_track_predicate(l), old_prov_track_predicate(l)) {
                    (true, true) => new_hits_a += 1,
                    (true, false) => new_only_a.push(format!("{name} :: {l}")),
                    (false, true) => old_only_a += 1,
                    (false, false) => {}
                }
            }
        }
    }

    assert!(new_only_a.is_empty(), ":1039 纯收紧被破坏（新过旧挂 {} 处）：{new_only_a:#?}", new_only_a.len());
    assert!(
        old_only_a >= 1,
        ":1039 旧过新挂 = 0 ⇒ 语料没覆盖到差异，包含论证退化成恒真（归属行样本共 {total_a} 行）"
    );
    assert!(
        new_hits_a >= 1,
        ":1039 新判据在语料上**一次都没命中** ⇒ `new_only.is_empty()` 只是「两条都不命中」的恒真"
    );

    assert!(new_only_b.is_empty(), ":1046 纯收紧被破坏（新过旧挂 {} 处）：{new_only_b:#?}", new_only_b.len());
    assert!(
        old_only_b >= 1,
        ":1046 旧过新挂 = 0 ⇒ 语料没覆盖到差异，包含论证退化成恒真（总行数 {total_b}）"
    );
    assert!(
        new_hits_b >= 1,
        ":1046 新判据在语料上**一次都没命中** ⇒ `new_only.is_empty()` 只是「两条都不命中」的恒真"
    );

    println!(
        "AC-003 样本计数 —— :1039 归属行 {total_a} 行 · 新命中 {new_hits_a} · 旧过新挂 {old_only_a} · 新过旧挂 {}；\
         :1046 全语料 {total_b} 行 · 新命中 {new_hits_b} · 旧过新挂 {old_only_b} · 新过旧挂 {}",
        new_only_a.len(),
        new_only_b.len()
    );
}

// ── AC-004 · 复发守卫 ────────────────────────────────────

/// 本文件所需的生产源码 —— 用来**数**认前缀的实现有几套。
const SRC: &str = include_str!("../src/domain/review_view.rs");

/// 只留**代码行**（丢掉整行注释）。
///
/// ⚠️ 为什么必须先滤掉注释：源码注释里**故意**写着
/// `contains(PROVENANCE_LINE_PREFIX)` 这种字样（在解释「原来是这么写的」），
/// 拿全文去数会把**注释**当成实现 ⇒ 守卫变成恒红。
fn code_lines(src: &str) -> Vec<&str> {
    src.lines().filter(|l| !l.trim_start().starts_with("//")).collect()
}

/// 前缀常量在**源码里**的名字。
///
/// ⚠️⚠️ 守卫找的必须是**常量名**（`BIZ_LINE_PREFIX`），**不是**展开后的字面量
/// （`"业务轨："`）—— 源码引用的是常量名，拿字面量去搜**一条也打不中**，
/// 守卫就会退化成「永远绿」的恒真判据（REQ-064 在它的守卫注释里写过同一条）。
/// 下面每条检查都在注释里标了这一点，并由自检变异反证它真能变红。
const PREFIX_IDS: [(&str, &str); 3] = [
    ("BIZ_LINE_PREFIX", BIZ_LINE_PREFIX),
    ("MODEL_LINE_PREFIX", MODEL_LINE_PREFIX),
    ("PROVENANCE_LINE_PREFIX", PROVENANCE_LINE_PREFIX),
];

/// 匹配前先**去掉 `&`** —— 否则 `contains(&BIZ_LINE_PREFIX)` 这种同义写法
/// 打不中 `contains(BIZ_LINE_PREFIX`，守卫会在**最常见的等价写法**上失效。
/// ⚠️ 下面 M5 那个自检变异专门钉这件事（第一版这里就是错的：注释里宣称
/// 能覆盖 `&` 形式，代码实际上**覆盖不到** ⇒ 那是一条**自己骗自己**的注释）。
fn without_ref(line: &str) -> String {
    line.replace('&', "")
}

/// 剥掉定义行上的**可见性前缀**（以及 `async` / `const` / `unsafe` / `extern`）。
///
/// # ⚠️ REQ-075（N1）：**受限可见性**也是可见性
///
/// REQ-073 只把 `pub ` 与 `pub(crate) ` 加进白名单，于是
/// `pub(in crate::domain) fn …` / `pub(super) fn …` / `pub(self) fn …`
/// **一条都打不中** ⇒ 检查 ④ `if let Some(..)` **整格静默跳过、守卫照样绿**。
/// 那与 REQ-073 刚修掉的是**同一个形状**（「守卫的一部分静默跳过，而守卫报绿」）。
///
/// ⇒ 这里改成**按语法剥**：`pub` 后面要么是空白，要么是 `(…)`，
/// 括号里是什么（`crate` / `super` / `self` / `in crate::…`）**一律不看**。
/// 未来新增的可见性写法不需要再改这个白名单。
///
/// ⚠️ 括号**没闭合**时**不剥**（宁可打不中也不乱剥）—— 打不中由下面的
/// [`prefix_guard_findings`] ①（定义数）与 M7 自检报红，不是静默放过。
fn def_head(line: &str) -> &str {
    let mut t = line.trim_start();
    for _ in 0..8 {
        let before = t;
        if let Some(rest) = t.strip_prefix("pub") {
            let after_parens = rest
                .strip_prefix('(')
                .and_then(|inner| inner.find(')').map(|close| inner[close + 1..].trim_start()));
            t = match after_parens {
                Some(s) if !s.is_empty() => s,
                _ if rest.starts_with(' ') => rest.trim_start(),
                // `pubfoo` / `pub` 单独出现 / 括号没闭合 ⇒ 不认识，原样返回。
                _ => before,
            };
        }
        for kw in ["async ", "const ", "unsafe ", "extern "] {
            t = t.strip_prefix(kw).unwrap_or(t);
        }
        if t == before {
            break;
        }
    }
    t
}

/// 定位 recogniser 的**定义行**与**函数体结束行**（`code` 的下标，半开区间）。
///
/// # 为什么必须是**一个 helper**，而不是就地 `starts_with("fn line_head_prefix_of(")`
///
/// REQ-073 把生产函数提成了 `#[doc(hidden)] pub` ⇒ 定义行变成
/// `pub fn line_head_prefix_of(...)`。旧锚点 `starts_with("fn …")` 对它**打不中**，
/// 而调用点写的是 `if let` ⇒ **整格检查静默跳过、守卫照样绿**。
/// 这正是本项目反复栽的「取不到东西就算通过 ⇒ 恒真」。
///
/// ⚠️ REQ-075（N1）把这条**再推一步**：只认两种可见性写法是不够的，
/// 受限可见性（`pub(in crate::…)` 等）同样会让锚点打不中 ⇒ 可见性前缀由
/// [`def_head`] **按语法剥**，不靠白名单。
///
/// ⚠️ 返回 `None` 仍然是可能的（函数被改名 / 整体消失）——
/// 那种情况由 [`prefix_guard_findings`] 的 ① 报红（定义数 ≠ 1），
/// ④ 跳过**不会**造成「无防护却显示有防护」的新假绿。
fn def_line_of(code: &[&str]) -> Option<(usize, usize)> {
    let start = code
        .iter()
        .position(|l| def_head(l).starts_with("fn line_head_prefix_of("))?;
    let end = code[start + 1..]
        .iter()
        .position(|l| l.trim() == "}")
        .map_or(code.len(), |e| start + 1 + e);
    Some((start, end))
}

/// 这条守卫的全部检查 —— 返回**问题清单**（空 = 通过）。
///
/// ⚠️ 这是**源码文本探针**：它断言的是文本形状，不是代码语义。效力边界写在
/// 本文件的模块注释里（能抓什么 / 抓不到什么）。
fn prefix_guard_findings(src: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let code = code_lines(src);

    // ① 认前缀的函数**只有一个定义**。
    let defs = src.matches("fn line_head_prefix_of(").count();
    if defs != 1 {
        bad.push(format!("认前缀的函数应当只有一个定义，实际 {defs} 处"));
    }
    if src.contains("fn track_prefix_of(") {
        bad.push("`track_prefix_of` 又冒出来了 ⇒ 存在两套认前缀的实现".into());
    }

    // ② **两轨前缀**不许用 `contains` 认 —— 一处都不许（含死代码）。
    //
    // 搜的是 `contains(BIZ_LINE_PREFIX`（**只到左括号，不闭合**），
    // 这样 `contains(BIZ_LINE_PREFIX[..])` 这类写法一并命中；
    // 配合 [`without_ref`]，`contains(&BIZ_LINE_PREFIX)` 也命中。
    for (id, _v) in PREFIX_IDS.iter().take(2) {
        let pat = format!("contains({id}");
        let hits: Vec<&str> =
            code.iter().filter(|l| without_ref(l).contains(pat.as_str())).copied().collect();
        if !hits.is_empty() {
            bad.push(format!("用 `contains` 认两轨前缀 {id}：{} 处 ⇒ {hits:?}", hits.len()));
        }
    }

    // ③ **归属前缀**的 `contains` 只允许有一处，且必须在规则④ 分支 (a) 的
    //    `if prov_lines.is_empty()` **之后**（那里它只补理由、不可能翻转判决）。
    let (prov_id, _prov_v) = PREFIX_IDS[2];
    let prov_pat = format!("contains({prov_id}");
    let prov_at: Vec<usize> = code
        .iter()
        .enumerate()
        .filter(|(_, l)| without_ref(l).contains(prov_pat.as_str()))
        .map(|(i, _)| i)
        .collect();
    if prov_at.len() != 1 {
        bad.push(format!(
            "用 `contains` 认归属前缀的代码行应当**恰好一处**（规则④ 分支 (a)），实际 {} 处：{prov_at:?}",
            prov_at.len()
        ));
    }
    let guard_at = code.iter().position(|l| l.trim_start().starts_with("if prov_lines.is_empty()"));
    match (prov_at.first().copied(), guard_at) {
        (Some(p), Some(g)) if p > g => {}
        (Some(p), g) => bad.push(format!(
            "归属前缀的 `contains`（第 {p} 行）必须落在 `if prov_lines.is_empty()`（第 {g:?} 行）**之后**的保护区里，\
             否则它会把「通过」翻成「违规」"
        )),
        (None, _) => {}
    }

    // ④ recogniser 函数体：三个前缀**各**用 `starts_with` 认一次、
    //    且认出来的就是同一个常量，函数体里不得出现 `contains`。
    //
    // ⚠️ REQ-073：锚点是 `def_line_of`，**不是** `starts_with("fn line_head_prefix_of(")` ——
    // 本条把生产函数提成了 `#[doc(hidden)] pub`，定义行变成
    // `pub fn line_head_prefix_of(...)`，`starts_with("fn …")` 就**打不中**了。
    // 而下面写的是 `if let Some(start) = …` ⇒ **打不中时这一整格静默跳过**，
    // 守卫照样绿 ——「取不到东西就算通过」正是本项目反复栽的恒真形态。
    // M6 那个自检变异专门钉这件事（把定义改成 `pub fn` 之外的形状）。
    if let Some((start, end)) = def_line_of(&code) {
        let body = &code[start..end];
        for (id, _v) in PREFIX_IDS {
            let sw = body.iter().filter(|l| l.contains(&format!("starts_with({id})"))).count();
            let some = body.iter().filter(|l| l.contains(&format!("Some({id})"))).count();
            if sw != 1 || some != 1 {
                bad.push(format!(
                    "recogniser 里 {id} 应当**恰好**由 `starts_with({id})` 认一次、`Some({id})` 回一次，\
                     实际 starts_with {sw} 次 / Some {some} 次"
                ));
            }
        }
        if body.iter().any(|l| l.contains(".contains(")) {
            bad.push("recogniser 函数体里出现了 `.contains(` ⇒ 它不再只认行首".into());
        }
    }

    bad
}

/// 【AC-004】**三类前缀的识别都由 `line_head_prefix_of` 承担，文件里没有第二处
/// 对它们做 `contains` 的实现。**
///
/// ⚠️ 这是一条**源码文本探针**，**只验文本、不验语义** ——
/// 完整的效力边界（能抓什么 / **抓不到什么**）写在**本文件的模块注释**里，
/// 别把它当成「能抓住代码语义变更」的守卫。
#[test]
fn test_req068_no_second_contains_based_prefix_recogniser() {
    let findings = prefix_guard_findings(SRC);
    assert!(findings.is_empty(), "前缀识别的复发守卫红了：{findings:#?}");

    // ⚠️⚠️ **自检：守卫必须真的能红。**
    // 「取不到东西就算通过」的判据恒为真，是本项目反复栽的坑（见 REQ-064）。
    // 下面每个变异都**从 SRC 真实文本出发**做字符串手术，
    // 真值**完全依赖 SRC** —— 不是 `assert!("ab".contains("ab"))` 那种空断言。
    let m1 = SRC.replace(
        "        // ⚠️ REQ-068（AC-001）：「第一条条目行」同样只按**行首**认，",
        "        if line.contains(BIZ_LINE_PREFIX) {}\n        // ⚠️ REQ-068（AC-001）：「第一条条目行」同样只按**行首**认，",
    );
    assert_ne!(m1, SRC, "变异 M1 打不中 ⇒ 源码形状变了，守卫需要更新");
    assert!(!prefix_guard_findings(&m1).is_empty(), "守卫杀不掉 M1（又写一处两轨 `contains`）⇒ 它恒为真");

    let m2 = format!("{SRC}\nfn line_head_prefix_of(line: &str) -> Option<&'static str> {{ let _ = line; None }}\n");
    assert!(!prefix_guard_findings(&m2).is_empty(), "守卫杀不掉 M2（复制出第二套 recogniser）⇒ 它恒为真");

    let m3 = SRC.replace(
        "starts_with(BIZ_LINE_PREFIX)",
        "contains(BIZ_LINE_PREFIX)",
    );
    assert_ne!(m3, SRC, "变异 M3 打不中 ⇒ recogniser 的写法变了，守卫需要更新");
    assert!(!prefix_guard_findings(&m3).is_empty(), "守卫杀不掉 M3（recogniser 内部退回 contains）⇒ 它恒为真");

    // M4：把规则④ 分支 (a) 那处**被论证过**的 `contains` 挪出保护区。
    let prov_line_src = code_lines(SRC)
        .into_iter()
        .find(|l| l.contains("contains(PROVENANCE_LINE_PREFIX"))
        .expect("源码里应当有规则④ 分支 (a) 那处 contains");
    let m4 = SRC.replacen(prov_line_src, "", 1).replace(
        "    let prov_lines: Vec<(usize, &str)> = lines",
        &format!("    let _leak = lines[0].contains({PROVENANCE_LINE_PREFIX});\n    let prov_lines: Vec<(usize, &str)> = lines"),
    );
    assert_ne!(m4, SRC, "变异 M4 打不中 ⇒ 规则④ 分支 (a) 的写法变了，守卫需要更新");
    assert!(!prefix_guard_findings(&m4).is_empty(), "守卫杀不掉 M4（把归属 contains 挪出保护区）⇒ 它恒为真");

    // M5：`contains(&BIZ_LINE_PREFIX)` 这种**等价写法** —— 第一版守卫在这上面
    // 是**漏的**（模式写成 `contains(BIZ_LINE_PREFIX)`，`&` 让它打不中）。
    let m5 = SRC.replace(
        "        // ⚠️ REQ-068（AC-001）：「第一条条目行」同样只按**行首**认，",
        "        if line.contains(&BIZ_LINE_PREFIX) {}\n        // ⚠️ REQ-068（AC-001）：「第一条条目行」同样只按**行首**认，",
    );
    assert_ne!(m5, SRC, "变异 M5 打不中 ⇒ 源码形状变了，守卫需要更新");
    assert!(!prefix_guard_findings(&m5).is_empty(), "守卫杀不掉 M5（`contains(&常量)` 等价写法）⇒ 它恒为真");

    // ⚠️⚠️ M6（**REQ-073 新增**）：**锚点必须认得 `pub` 前缀**。
    //
    // 本条把生产函数提成了 `#[doc(hidden)] pub`。第一版的 ④ 锚点是
    // `l.starts_with("fn line_head_prefix_of(")` ⇒ 加上 `pub ` 之后**打不中**，
    // 而调用点是 `if let` ⇒ **④ 整格静默跳过、守卫照样绿**。
    // 那是「看起来有防护其实没有」，只是**方向反过来**（无防护却显示有防护）。
    //
    // 这条自检把「④ 真的取到了函数体」钉成**可执行断言**：
    // 先把 `pub ` 摘掉（若源码形状变了，`.replace` 打不中 ⇒ 后面 `assert_ne!` 先红），
    // 再把 recogniser 内部改成 `contains` —— **只有 ④ 报得出来**
    // （② 报的是「用 contains 认两轨前缀」，措辞不同 ⇒ 下面按 ④ 的措辞精确断言）。
    let m6_def = SRC.replace(
        "#[doc(hidden)]\npub fn line_head_prefix_of(line: &str)",
        "fn line_head_prefix_of(line: &str)",
    );
    assert_ne!(m6_def, SRC, "变异 M6 打不中 ⇒ recogniser 的定义行形状变了，[`def_line_of`] 需要更新");
    // M6a：**去掉 `pub` 之后 ④ 仍然能定位**（证明锚点没退化成「只认 `pub`」）。
    assert!(
        def_line_of(&code_lines(&m6_def)).is_some(),
        "④ 的锚点在**私有**形状下定位不到 ⇒ [`def_line_of`] 只认 `pub` 的一种写法"
    );
    // M6b：**保留 `pub` 时 ④ 也必须能定位**（这正是第一版漏掉的那一格）。
    assert!(
        def_line_of(&code_lines(SRC)).is_some(),
        "④ 的锚点在 `#[doc(hidden)] pub` 形状下定位不到 ⇒ ④ 正在**静默跳过**（恒真守卫）"
    );
    // M6c：**锚点取到之后**，函数体里的 `contains` 仍要被抓出来（证明它不是「取到就放过」）。
    let m6 = SRC.replace(
        "    let head = line.trim_start();",
        "    let head = line;\n    let _ = head.contains(BIZ_LINE_PREFIX);",
    );
    assert_ne!(m6, SRC, "变异 M6 打不中 ⇒ recogniser 的函数体形状变了，守卫需要更新");
    assert!(
        prefix_guard_findings(&m6).iter().any(|f| f.contains("recogniser 函数体里出现了 `.contains(`")),
        "④ 取到了函数体却没报函数体里的 `.contains(` ⇒ ④ 是恒真：\n{:#?}",
        prefix_guard_findings(&m6)
    );

    // ⚠️⚠️ M7（**REQ-075 新增 · N1**）：锚点必须认得**受限可见性**写法。
    //
    // REQ-073 只把 `pub ` / `pub(crate) ` 加进白名单。将来把 recogniser 改成
    // `pub(in crate::domain) fn …` 之类**受限可见性**时，锚点**打不中**，
    // 而调用点是 `if let` ⇒ **④ 整格静默跳过、守卫照样绿** ——
    // 那与 REQ-073 刚挖出来的缺陷是**同一个形状**（「加 pub 静默禁用守卫检查④」）。
    //
    // ⚠️ 下面 M7 的**第一段**只看 `pub` 与括号，与白名单**无关**；
    // 真正被钉住的是 [`def_head`]：**括号里写什么都不影响它剥掉这个前缀**。
    let m7_def = SRC.replace(
        "#[doc(hidden)]\npub fn line_head_prefix_of(line: &str)",
        "#[doc(hidden)]\npub(in crate::domain) fn line_head_prefix_of(line: &str)",
    );
    assert_ne!(m7_def, SRC, "变异 M7 打不中 ⇒ recogniser 的定义行形状变了，[`def_head`] 需要更新");
    assert!(
        def_line_of(&code_lines(&m7_def)).is_some(),
        "④ 的锚点在**受限可见性**（`pub(in crate::…)`）形状下定位不到 \
         ⇒ ④ 正在**静默跳过**（恒真守卫）。白名单写法已被 [`def_head`] 取代。"
    );
    // 另两种受限写法也要认（`pub(super)` / `pub(self)`）—— 白名单不会自动覆盖新写法。
    for (open, close) in [("pub(super) ", ")"), ("pub(self) ", ")")] {
        let variant = SRC.replace(
            "#[doc(hidden)]\npub fn line_head_prefix_of(line: &str)",
            &format!("#[doc(hidden)]\n{open}fn line_head_prefix_of(line: &str{close}"),
        );
        assert_ne!(variant, SRC, "变异 M7 打不中 ⇒ 定义行形状变了，[`def_head`] 需要更新");
        assert!(
            def_line_of(&code_lines(&variant)).is_some(),
            "④ 的锚点认不得 `{open}` 形状 ⇒ 那种可见性一出现，④ 就静默跳过"
        );
    }
    // M7b：**锚点取到之后**检查 ④ 仍要报得出来（证明不是「取到就放过」）。
    let m7 = m7_def.replace(
        "    let head = line.trim_start();",
        "    let head = line;\n    let _ = head.contains(BIZ_LINE_PREFIX);",
    );
    assert_ne!(m7, SRC, "变异 M7b 打不中 ⇒ recogniser 的函数体形状变了，守卫需要更新");
    assert!(
        prefix_guard_findings(&m7).iter().any(|f| f.contains("recogniser 函数体里出现了 `.contains(`")),
        "④ 在受限可见性下取到了函数体却没报函数体里的 `.contains(` ⇒ ④ 是恒真：\n{:#?}",
        prefix_guard_findings(&m7)
    );
}

/// 【AC-004 · 语义兜底】**payload 里塞满三类前缀 ⇒ 判决逐条不变**。
///
/// ⚠️ 这条**不看源码**，只看渲染结果与判决 —— 它是 AC-004 守卫的**语义一半**。
/// 源码文本探针抓不到「换了种拼写的第二套识别」，这条抓得到
/// （只要它落在被测代码路径上）。
#[test]
fn test_req068_head_only_recognition_behaviourally() {
    let payload = json!({
        "key": "customer",
        "properties": [{ "name": "客户编号", "column": "customer_pk" }],
        "evidence": [{
            "table": "t_customer", "column": "customer_no",
            "claim": format!("{BIZ_LINE_PREFIX}{MODEL_LINE_PREFIX}{PROVENANCE_LINE_PREFIX}全是 payload 正文")
        }]
    });
    let items = three_items(
        &format!("{MODEL_LINE_PREFIX}{PROVENANCE_LINE_PREFIX}客户"),
        payload,
    );
    let (v, text) = view_and_text(&items, Some(&eb_with_agent(&format!("{MODEL_LINE_PREFIX}{BIZ_LINE_PREFIX}执行者"))));

    // 前提：三类前缀**都**真的落在了**非行首**的位置上（否则这条是恒真的）。
    //
    // ⚠️ 这里问的是「**含**这个前缀，但不是**以**它开头」——
    // 不是「含它、而行首什么前缀都没有」：payload 文本只会被渲染器塞进
    // **已经**是结构行的行里（业务轨行 / 建模轨行 / 归属行），
    // 所以「行首不带任何前缀却含前缀」那种行在真实输出里**根本不存在**
    // （本条第一版就是这么写错的，是它自己跑出来的 —— 前提断言拒绝执行）。
    for c in [BIZ_LINE_PREFIX, MODEL_LINE_PREFIX, PROVENANCE_LINE_PREFIX] {
        let mid = text.lines().filter(|l| l.contains(c) && !l.trim_start().starts_with(c)).count();
        assert!(mid >= 1, "前提不成立：渲染里没有「含 {c} 但不在行首」的出现 ⇒ 这条测不到任何东西\n实际输出：\n{text}");
    }

    // 结构计数：仍然只有「1 条归属行 + 每个条目各一轨」。
    assert_eq!(
        text.lines().filter(|l| is_prov_head(l)).count(),
        1,
        "归属行数不对（payload 里的归属前缀被算成了结构行）\n实际输出：\n{text}"
    );
    assert_eq!(
        text.lines().filter(|l| is_track_head(l)).count(),
        6,
        "两轨行数不对（3 个条目应当 6 行）\n实际输出：\n{text}"
    );

    assert!(
        verify_provenance_scope(&v, &text).is_empty(),
        "payload 塞满三类前缀被判违规：{:#?}\n实际输出：\n{text}",
        verify_provenance_scope(&v, &text)
    );
    assert!(
        verify_track_separation(&v, &text).is_empty(),
        "两轨分界被判违规：{:#?}\n实际输出：\n{text}",
        verify_track_separation(&v, &text)
    );
}

// ── 反向变异：真归属行仍然被认出 ─────────────────────────

/// 【反向变异】**四种归属状态的真归属行都必须被认出 ⇒ 零违规**。
///
/// 杀「收窄过头」：若新判据要求列 0 / 逐字模板，真归属行会**认不出来**
/// ⇒ 规则①报「行数为 0」。
#[test]
fn test_req068_all_four_provenance_states_still_recognised() {
    let items = three_items("客户", plain_payload());
    let cases: Vec<(&str, ReviewTrackView, String)> = vec![
        { let (v, t) = view_and_text(&items, Some(&declared_eb())); ("已申报", v, t) },
        { let (v, t) = view_and_text(&items, None); ("人工修订", v, t) },
        {
            let (v, _s) = project_review_track_with_summary(&items, &baseline());
            let t = render_review_text(&v);
            ("未提供", v, t)
        },
        { let (v, t) = view_and_text(&items, Some(&json!({ "agent": "" }))); ("形态可疑", v, t) },
    ];

    for (name, v, text) in cases {
        assert_eq!(
            text.lines().filter(|l| is_prov_head(l)).count(),
            1,
            "前提不成立：{name} 之下渲染没有恰好一条行首归属行\n实际输出：\n{text}"
        );
        let violations = verify_provenance_scope(&v, &text);
        assert!(
            violations.is_empty(),
            "{name} 之下真归属行被认不出来（判据收窄过头）：{violations:#?}\n实际输出：\n{text}"
        );
    }
}

/// 【反向变异】**带缩进的归属行仍然被认出**（行首认轨认的是「去掉缩进后的行首」）。
#[test]
fn test_req068_indented_provenance_line_is_still_recognised() {
    let items = three_items("客户", plain_payload());
    let (v, good) = view_and_text(&items, Some(&declared_eb()));
    let indented = good
        .lines()
        .map(|l| if is_prov_head(l) { format!("  {l}") } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    assert!(
        verify_provenance_scope(&v, &indented).is_empty(),
        "带缩进的归属行被认不出来（收窄过头 · 要求列 0）：{:#?}\n变异输出：\n{indented}",
        verify_provenance_scope(&v, &indented)
    );
}