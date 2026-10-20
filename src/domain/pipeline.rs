//! REQ-107 · 客户类出口的**唯一管线**（结构收口，不是新功能）。
//!
//! # 一句话
//!
//! 出口只声明**格式 + 轨道 + 闸粒度**，管线负责
//! 「前处理（剔除 / 清洗）→ 发布闸 → 可写出的产物」。
//! 「拦不拦」由 [`OutletSpec::track`] 决定，**出口代码无权决定**。
//!
//! # 为什么要有这个模块（病根）
//!
//! 同一份客户视图今天从 13 个出口出去，每个出口**各自重新决定**一遍
//! 「要不要剔除 / 要不要清洗 / 要不要拦」。已经发生过两次事故：
//!
//! 1. REQ-104：HTML 出口做了属性级剔除、JSON 与 `/api/node` 没做
//!    ⇒ 同一份数据两种结果、3 个节点返 500；
//! 2. REQ-106：守门器靠调用方自觉在出口前调用 ⇒ 漏一个出口就漏一道闸；
//!    JSON 出口还是「先写出去、后判定」，闸门对 stdout 形同虚设。
//!
//! ⇒ 病根不是「代码不整齐」，是**没有单一收口点**。
//!
//! # ⛔ 类型层不变量（AC-003 的主判据）
//!
//! [`OutletRender`] 与 [`GuardedValue`] 的字段**全部私有**、且**没有公开构造器**。
//! 出口拿不到它们的实例，除非经由 [`render`] / [`publish_value`] ——
//! 也就是「必须先过前处理与发布闸」。下面两条 doctest 把这件事钉在**编译期**：
//!
//! ```compile_fail
//! use ontok::domain::pipeline::OutletRender;
//! // 字段私有 ⇒ 管线之外**字面量构造**不出来。
//! let forged = OutletRender { text: String::from("样例客户") };
//! ```
//!
//! ```compile_fail
//! use ontok::domain::pipeline::OutletRender;
//! // 没有公开构造器 ⇒ 「造一个空壳再填」这条路也不通。
//! let forged = OutletRender::build(String::from("样例客户"));
//! ```
//!
//! ⚠️ `compile_fail` 只能证明「这一段编不过」，证明不了「编不过的原因是私有」。
//! 所以另有一层**源码否定式**判据（`tests/test_req107_outlet_pipeline.rs`）：
//! 出口文件里对判据的直调点数必须为 0、且本模块里不出现 `pub` 字段声明。
//!
//! # 判据**只调用、不复制**（纪律 H）
//!
//! 本模块里的 `scan_modeling_terms` / `mcp::tools::guard` / `mcp::tools::safe_error` /
//! `property_filter::filter_view` 全部是**调用**。判据本体仍然只有一份：
//! - 术语判据：`domain::view::scan_modeling_terms`（`mcp::tools::guard` 是它的值级门面）；
//! - 标识符形态判据：`domain::identifier_shape::has_raw_identifier`。
//!
//! ⛔ 本模块**不许**出现第二份判据实现：那会让「管线内」与「出口内」两套判据漂移，
//! 正是 REQ-029 已经栽过一次的形态。
//!
//! # 前处理与发布闸为什么是两个阶段（AC-010-2）
//!
//! | | 前处理 | 发布闸 |
//! | --- | --- | --- |
//! | 判据 | 源库标识符**形态**（`customer_id` / `key`） | 建模**术语**（`主键` / `基数` / `1:N`） |
//! | 命中后 | **减掉那一条**，其余照发 | **整份不发**（fail-closed） |
//! | 失败语义 | 静默删条目 ⇒ 必须报 stderr | 拒绝发布 ⇒ 退出码 / 500 / isError |
//!
//! 顺序不可交换：先剔后闸，才能让「可减」的条目减掉（REQ-104），
//! 而让「不可减」的条目（对象名里带 `主键`）仍然到达闸门被拒。
//!
//! # 三个卡点的裁定
//!
//! 见 `.feature/design-req107.md` §三（改名口径 / 闸粒度 / stderr 报告形状）。
//! 这里只留结论：
//! - ① JSON **不改名**（名字是标识，不是文案）；
//! - ② 双粒度，但由 [`Granularity`] **声明**，不由出口自决；
//! - ③ stderr 报告形状唯一（[`DropReport::stderr_line`]），客户正文只报数量。
//!
//! # 33 条出口登记表（AC-002 / AC-007）
//!
//! [`OUTLETS`] 是**机读**的：每条带 `path` + `call_site`（该出口调用管线的源码片段
//! 唯一子串）+ 轨道 + 格式。判据在 `tests/test_req107_*` 里做**双向对账**
//! （条数 / 分组 / ID 集合 / needle 命中）。
//!
//! ⚠️ **已知边界（⛔ 不许写成「已封堵」）**：needle 式对账抓得住「登记与源码漂移」，
//! **抓不住**「语义等价改写」。补强在类型层（本模块 §类型层不变量）与源码否定式。

use crate::domain::html_view::{render_html, RenderedPage};
use crate::domain::overview::{render_overview_text, Overview};
use crate::domain::property_filter::{filter_view, FilteredView};
use crate::domain::view::{scan_modeling_terms, CustomerView};
use serde_json::Value;

// ══════════════════════════════════════════════════════════════
// 轨道 / 格式 / 粒度
// ══════════════════════════════════════════════════════════════

/// 出口的**轨道** —— 决定「要不要过发布闸」的唯一事实。
///
/// ⛔ 建模类豁免必须是**显式声明**（AC-007）：`Track::Modeling` 出现在这里与
/// [`OUTLETS`] 里，而不是「因为它没调闸所以它就是建模」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Track {
    /// 对外客户视图。**必须**过发布闸。
    Customer,
    /// 建模轨（FDE 通道）。建模术语在这一轨是**要被裁决的内容**，不是泄露 ⇒ 豁免。
    Modeling,
    /// 内部运维输出（`project` / `version` / `concept import` / `diff` …）。不过闸。
    Internal,
    /// 静态文本类外送（`tools/list` / `initialize`）。AC-008 裁定：**过**运行时闸。
    Static,
    /// 错误文案（Z1）。比闸更保守：命中即**整句**换成通用说明。
    ErrorPath,
}

impl Track {
    /// 要过发布闸吗？**只看轨道** —— 出口代码无权决定。
    ///
    /// - `Customer` / `Static` ⇒ 过闸（`Static` 是 AC-008 的裁定：运行时闸）；
    /// - `Modeling` / `Internal` ⇒ 不过闸（显式豁免，理由在 [`OUTLETS`] 的
    ///   `exemption_reason` 里逐条写明）；
    /// - `ErrorPath` ⇒ 不过闸，但走 `safe_error`（更保守）。
    pub const fn gated(self) -> bool {
        matches!(self, Track::Customer | Track::Static)
    }

    /// 人读的名字（报错与交付物用）。判据**不依赖**它。
    pub const fn as_str(self) -> &'static str {
        match self {
            Track::Customer => "客户",
            Track::Modeling => "建模",
            Track::Internal => "内部",
            Track::Static => "静态",
            Track::ErrorPath => "错误路径",
        }
    }
}

/// 出口的**格式** —— 出口唯一被允许提出的偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Html,
    Json,
    Text,
}

impl Format {
    pub const fn as_str(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Json => "json",
            Format::Text => "text",
        }
    }
}

/// 发布闸的**粒度**（AC-004 卡点②的裁定结果）。
///
/// 双粒度**保留**，但它是**声明**而不是出口自决：
/// 聚合视图没有「节点」这个可分离单位（逐节点只能靠悄悄删节点 ⇒ 图与节点索引对不上，
/// 且客户看不见自己少了什么）；`/api/node` 的可分离单位就是一个节点
/// （整份拒发 ⇒ 一个坏节点废掉整页，正是 REQ-104 修掉的那个事故）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// 整份拒发。
    Whole,
    /// 逐节点拒发（只拒那一个节点）。
    PerNode,
}

impl Granularity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Granularity::Whole => "whole",
            Granularity::PerNode => "per_node",
        }
    }
}

/// 一条出口的**声明**。出口层被允许提供的全部信息就是这些。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutletSpec {
    /// 登记表里的编号（`C3` / `H3` / `M1-M5` / `Z1` …）。
    pub id: &'static str,
    pub track: Track,
    pub format: Format,
    pub granularity: Granularity,
}

/// `OutletSpec` 的**唯一构造方式**（关联常量）。
///
/// ⛔ 刻意不提供 `fn new(...)`：出口要接哪个规格，必须用**具名常量**
/// （`OutletSpec::C3`）—— 「随手拼一个 spec」在源码里立刻看得见。
impl OutletSpec {
    const fn of(
        id: &'static str,
        track: Track,
        format: Format,
        granularity: Granularity,
    ) -> Self {
        Self { id, track, format, granularity }
    }

    // ── 客户类 13 条（§一）──
    pub const C1: Self = Self::of("C1", Track::Customer, Format::Html, Granularity::Whole);
    pub const C2: Self = Self::of("C2", Track::Customer, Format::Html, Granularity::Whole);
    pub const C3: Self = Self::of("C3", Track::Customer, Format::Json, Granularity::Whole);
    pub const C4: Self = Self::of("C4", Track::Customer, Format::Text, Granularity::Whole);
    pub const C5: Self = Self::of("C5", Track::Customer, Format::Json, Granularity::Whole);
    pub const H1: Self = Self::of("H1", Track::Customer, Format::Html, Granularity::Whole);
    pub const H2: Self = Self::of("H2", Track::Customer, Format::Json, Granularity::Whole);
    /// ⚠️ **逐节点**：一个坏节点只拒发那一个节点（卡点②的裁定）。
    pub const H3: Self = Self::of("H3", Track::Customer, Format::Json, Granularity::PerNode);
    /// M1–M5 共用**同一个**调用点（`serve.rs` 的 `tools/call` 分支）。
    pub const MCP_TOOLS: Self =
        Self::of("M1-M5", Track::Customer, Format::Json, Granularity::Whole);

    // ── 零闸位点 ──
    /// Z1 `check_scope` 的不匹配分支。
    pub const Z1: Self = Self::of("Z1", Track::ErrorPath, Format::Json, Granularity::Whole);
    /// Z2 `tools/list`（AC-008：运行时闸）。
    pub const Z2: Self = Self::of("Z2", Track::Static, Format::Json, Granularity::Whole);
    /// Z3 `initialize` 的整段 result（模板正文 + serverInfo + 项目名）。
    pub const Z3: Self = Self::of("Z3", Track::Static, Format::Json, Granularity::Whole);

    // ── 建模轨 ──
    /// D1 `ontology show --model-view`：**唯一**真走管线的建模出口。
    ///
    /// 它的存在让「误标轨道 ⇒ 行为要变」可观察（AC-007 的判据）：
    /// 同一个 spec 把 `track` 换成 `Customer`，同一份产物就会被闸拦住。
    pub const D1: Self = Self::of("D1", Track::Modeling, Format::Json, Granularity::Whole);

    // ── 内部轨 ──
    /// 内部运维输出的轨道声明（本轮的 9 条内部出口只**登记**，不改写出路径）。
    pub const INTERNAL: Self = Self::of("I", Track::Internal, Format::Text, Granularity::Whole);
}

// ══════════════════════════════════════════════════════════════
// 产物
// ══════════════════════════════════════════════════════════════

/// 出口交给管线的**产物**。出口不再提供「怎么呈现」的任何开关。
pub enum Product<'a> {
    /// 客户视图 → HTML：`render_html` 自带展示级清洗（`sanitize_display`）。
    CustomerHtml { view: &'a CustomerView, sampling: &'a [String] },
    /// 客户视图 → JSON：`filter_view` 只剔**不改名**（卡点①的裁定）。
    CustomerJson(&'a CustomerView),
    /// 业务总览：**两条格式都渲染、都过闸**，按声明返回其中一条。
    ///
    /// ⚠️ 这个「过度拦截」是**既有行为**（`cli.rs` 收口前就是两条都判），
    /// 按 AC-005 原样保留；是否收窄留给后续需求（已登记为已知限制）。
    Overview(&'a Overview),
    /// 已经渲染完成的文本（首页整页、建模轨投影）。
    Text(String),
    /// 错误文案 → `safe_error`（Z1）。
    ErrorText(String),
}

// ══════════════════════════════════════════════════════════════
// 前处理报告（卡点③的裁定：形状唯一）
// ══════════════════════════════════════════════════════════════

/// 剔除报告的形状常量 —— **唯一**的 stderr 抬头。
///
/// ⛔ 出口不许自己拼报告文本：那正是「各出口 stderr 报告内容各异」的来源。
pub const DROP_REPORT_PREFIX: &str = "⚠ 发布闸前处理";

/// 一条剔除记录的种类。判据与形状都按它分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    /// 对象名带建模用语 ⇒ 整条对象不上页面。
    Object,
    /// 属性名是源库标识符形态 ⇒ 整条属性剔除。
    WholeProp,
    /// 属性名干净、说明带技术用语 ⇒ 只隐去说明。
    NoteProp,
    /// 对象说明带技术用语 ⇒ 整段隐去。
    SilencedNote,
}

impl DropKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            DropKind::Object => "对象名",
            DropKind::WholeProp => "整条属性",
            DropKind::NoteProp => "只隐去说明",
            DropKind::SilencedNote => "对象说明",
        }
    }
}

/// 一条剔除记录。`label` 是**运维面**的定位串（含原名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropItem {
    pub kind: DropKind,
    pub label: String,
}

/// 前处理的**唯一**报告载体（客户正文只报数量；stderr 报原名）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropReport {
    items: Vec<DropItem>,
}

impl DropReport {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> &[DropItem] {
        &self.items
    }

    /// 某一类的条数（页面正文与报告抬头都用它，不各自数一遍）。
    pub fn count(&self, kind: DropKind) -> usize {
        self.items.iter().filter(|i| i.kind == kind).count()
    }

    /// ⭐ **唯一的 stderr 报告形状**（卡点③的裁定：两级 + 形状唯一）。
    ///
    /// 形状：
    /// `⚠ 发布闸前处理：共剔除 N 项（对象名 a · 整条属性 b · 只隐去说明 c · 对象说明 d）。明细（前 20 条）：…`
    ///
    /// 前 20 条上限与页面出口、`property_filter` 的原口径一致（运维 grep 得到、又不刷屏）。
    pub fn stderr_line(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        let mut out = format!(
            "{DROP_REPORT_PREFIX}：共剔除 {} 项（对象名 {} · 整条属性 {} · 只隐去说明 {} · 对象说明 {}）",
            self.items.len(),
            self.count(DropKind::Object),
            self.count(DropKind::WholeProp),
            self.count(DropKind::NoteProp),
            self.count(DropKind::SilencedNote),
        );
        const SHOW: usize = 20;
        let shown: Vec<&str> = self.items.iter().take(SHOW).map(|i| i.label.as_str()).collect();
        if shown.is_empty() {
            out.push('。');
        } else {
            out.push_str(&format!("。明细（前 {} 条）：{}", shown.len(), shown.join("、")));
            if self.items.len() > shown.len() {
                out.push_str(&format!("（其余 {} 条省略）", self.items.len() - shown.len()));
            }
        }
        out
    }

    /// 非空才写 stderr。**出口一律调它**，不再各自 `eprintln!`。
    pub fn emit(&self) {
        if !self.is_empty() {
            eprintln!("{}", self.stderr_line());
        }
    }

    /// 从 `property_filter::filter_view` 的结果建报告（JSON / 装配期那条路）。
    fn from_filtered(&mut self, filtered: &FilteredView) {
        for d in &filtered.dropped {
            let kind = if d.whole_property { DropKind::WholeProp } else { DropKind::NoteProp };
            self.items.push(DropItem { kind, label: d.line() });
        }
        for name in &filtered.silenced_objects {
            self.items.push(DropItem {
                kind: DropKind::SilencedNote,
                label: format!("{name}（对象说明）"),
            });
        }
    }

    /// 从 `html_view::render_html` 的结果建报告（HTML 那条路）。
    ///
    /// ⚠️ `RenderedPage` 的两份清单已经是**格式化后的字符串**（`对象/属性`），
    /// 这里只做分类，不改写内容 —— 改它会同时改掉页面正文的计数。
    fn from_page(&mut self, page: &RenderedPage) {
        for name in &page.skipped {
            self.items.push(DropItem {
                kind: DropKind::Object,
                label: format!("{name}（对象名）"),
            });
        }
        for s in &page.skipped_props {
            let kind = if s.ends_with("（说明文字带技术用语）") {
                DropKind::NoteProp
            } else {
                DropKind::WholeProp
            };
            self.items.push(DropItem { kind, label: s.clone() });
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 失败
// ══════════════════════════════════════════════════════════════

/// 管线失败。
///
/// ⚠️ 管线**不自己 `exit`、不自己决定状态码**：那属于出口的「政策」而不是「判定」。
/// - CLI：`cli::publish_customer` 把 `Leaked` 映射成 `EXIT_LEAKED_MODELING_TERMS`；
/// - HTTP：映射成 500；
/// - MCP：映射成 `isError: true` 的 result。
///
/// 判定（拦不拦）在管线内，政策（怎么退出）在出口侧 —— 这一刀是刻意的：
/// REQ-106 的教训是「判了但写已经在前面」，而不是「退出码写错了地方」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    /// 发布闸命中：**一个字节都不出去**。
    Leaked {
        outlet: &'static str,
        /// 给响应/CLI 看的**不含词条**的人话。
        message: String,
        /// 命中的词条 —— **只许进 stderr**（REQ-106 AC-002）。
        hits: Vec<String>,
    },
    /// 产物本身不合法（序列化等）。这不是数据问题，是 bug。
    Render {
        outlet: &'static str,
        what: String,
    },
}

impl PipelineError {
    fn leaked(spec: OutletSpec, hits: Vec<String>) -> Self {
        PipelineError::Leaked {
            outlet: spec.id,
            message: format!(
                "这一问不给数据：内容里出现了 {} 项只有内部使用的术语（为免泄漏，此处不列出具体词条）。这是发布闸门，请联系维护者。",
                hits.len()
            ),
            hits,
        }
    }

    fn leaked_with_message(spec: OutletSpec, message: String) -> Self {
        PipelineError::Leaked { outlet: spec.id, message, hits: Vec::new() }
    }

    fn render(spec: OutletSpec, what: impl Into<String>) -> Self {
        PipelineError::Render { outlet: spec.id, what: what.into() }
    }

    /// 不含词条的人话（HTTP 500 / MCP `isError` / CLI stderr 用）。
    pub fn message(&self) -> &str {
        match self {
            PipelineError::Leaked { message, .. } => message,
            PipelineError::Render { what, .. } => what,
        }
    }

    /// 命中的词条。**⛔ 只许写 stderr**。
    pub fn hits(&self) -> &[String] {
        match self {
            PipelineError::Leaked { hits, .. } => hits,
            PipelineError::Render { .. } => &[],
        }
    }

    pub fn outlet(&self) -> &'static str {
        match self {
            PipelineError::Leaked { outlet, .. } | PipelineError::Render { outlet, .. } => outlet,
        }
    }

    pub fn is_leak(&self) -> bool {
        matches!(self, PipelineError::Leaked { .. })
    }
}

// ══════════════════════════════════════════════════════════════
// 可写出的产物（字段私有、无公开构造器 —— AC-003 的类型层）
// ══════════════════════════════════════════════════════════════

/// 文本轨的产物：**已过前处理与发布闸**的字节。
///
/// ⛔ 字段私有、无公开构造器 ⇒ 出口造不出来。`CustomerRender` 是它在 AC 里的叫法。
#[derive(Debug)]
pub struct OutletRender {
    text: String,
    report: DropReport,
    outlet: &'static str,
}

/// 结构化轨（HTTP / MCP）的产物：**已过发布闸**的值 + 它的序列化文本。
#[derive(Debug)]
pub struct GuardedValue {
    text: String,
    value: Value,
}

impl OutletRender {
    fn build(spec: OutletSpec, text: String, report: DropReport) -> Self {
        Self { text, report, outlet: spec.id }
    }

    /// 要写出去的字节。**出口唯一被允许使用的东西**。
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// 这个产物的前处理报告（只读；出口不许自己再拼一份）。
    pub fn report(&self) -> &DropReport {
        &self.report
    }

    /// 因对象名不可用而整条没出去的**对象个数**（页面提示语要用）。
    pub fn dropped_object_count(&self) -> usize {
        self.report.count(DropKind::Object)
    }

    /// 产物来自哪条出口（报错定位用）。
    pub fn outlet(&self) -> &'static str {
        self.outlet
    }

    /// 拿走字节（**只在确实需要 owned `String` 的地方用**，如 `axum::response::Html`）。
    ///
    /// ⚠️ 它不放松任何东西：拿走的仍然是过了前处理与发布闸的那一份。
    pub fn into_text(self) -> String {
        self.text
    }
}

impl GuardedValue {
    fn build(text: String, value: Value) -> Self {
        Self { text, value }
    }

    /// 已过闸的序列化文本（`tools/call` 的 `content[0].text` 用它）。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 已过闸的结构化值（HTTP 的响应体 / MCP 的 `structuredContent`）。
    pub fn into_value(self) -> Value {
        self.value
    }
}

// ══════════════════════════════════════════════════════════════
// 入口
// ══════════════════════════════════════════════════════════════

/// 文本轨的**唯一入口**（CLI 的五条客户出口、首页、建模投影）。
///
/// 命中发布闸 ⇒ `Err(PipelineError::Leaked)`；**一个字节都不会产出**。
pub fn render(spec: OutletSpec, product: Product<'_>) -> Result<OutletRender, PipelineError> {
    match product {
        Product::CustomerHtml { view, sampling } => {
            let page = render_html(view, sampling);
            let mut report = DropReport::default();
            report.from_page(&page);
            // ⚠️ 报告先出（剔除发生在闸之前，运维要先看到「减了什么」再看「拦了什么」）。
            report.emit();
            gate_text(spec, &page.html)?;
            Ok(OutletRender::build(spec, page.html, report))
        }

        Product::CustomerJson(view) => {
            let filtered = filter_view(view);
            let mut report = DropReport::default();
            report.from_filtered(&filtered);
            report.emit();
            let text = serde_json::to_string_pretty(&filtered.view)
                .map_err(|e| PipelineError::render(spec, format!("客户视图序列化失败：{e}")))?;
            gate_text(spec, &text)?;
            Ok(OutletRender::build(spec, text, report))
        }

        Product::Overview(ov) => {
            let text = render_overview_text(ov);
            let json_text = serde_json::to_string_pretty(ov)
                .map_err(|e| PipelineError::render(spec, format!("业务总览序列化失败：{e}")))?;
            // ⚠️ 两条格式**都判**（既有行为，AC-005 原样保留）。
            gate_both(spec, &json_text, &text)?;
            let chosen = match spec.format {
                Format::Text => text,
                Format::Json => json_text,
                Format::Html => {
                    return Err(PipelineError::render(spec, "业务总览没有 HTML 形态"))
                }
            };
            Ok(OutletRender::build(spec, chosen, DropReport::default()))
        }

        Product::Text(text) => {
            gate_text(spec, &text)?;
            Ok(OutletRender::build(spec, text, DropReport::default()))
        }

        Product::ErrorText(raw) => {
            // 比闸更保守：命中就是**整句换掉**（`safe_error` 的既有语义）。
            Ok(publish_error_text(spec, &raw))
        }
    }
}

/// 错误文案轨的唯一入口（Z1）。
///
/// 比发布闸**更保守**：`safe_error` 命中时把**整句**换成通用说明
/// （闸只拒发、不改写；错误路径必须给客户端一句人话，所以只能换）。
///
/// ⚠️ 干净文案**原样返回** —— 运维必须能看出「本服务绑的是哪个项目」
/// （`tests/test_req107_scope_error_gate.rs` 的对照组钉住这一点）。
pub fn publish_error_text(spec: OutletSpec, raw: &str) -> OutletRender {
    let safe = crate::mcp::tools::safe_error(raw);
    OutletRender::build(spec, safe, DropReport::default())
}

/// 结构化轨的唯一入口（HTTP `/api/graph` `/api/node`、MCP 全部响应、首页不由它走）。
///
/// 命中 ⇒ `Err`，消息**逐字**取自 `mcp::tools::guard` —— 所以 500 正文与
/// `isError: true` 的文案在收口前后**一个字节都不变**。
pub fn publish_value(spec: OutletSpec, value: Value) -> Result<GuardedValue, PipelineError> {
    if spec.track.gated() {
        // ⛔ 判据只复用，不复制：`guard` 的内部实现一行没改。
        match crate::mcp::tools::guard(&value) {
            Ok(text) => Ok(GuardedValue::build(text, value)),
            Err(message) => Err(PipelineError::leaked_with_message(spec, message)),
        }
    } else {
        let text = serde_json::to_string_pretty(&value)
            .map_err(|e| PipelineError::render(spec, format!("产物序列化失败：{e}")))?;
        Ok(GuardedValue::build(text, value))
    }
}

/// **装配期**的前处理：`/api/graph` `/api/node` 共用一份已剔视图，
/// MCP 五个工具共用一份已剔视图。
///
/// ⚠️ 它返回的是 `FilteredView`（**未过闸**）—— 闸在每次响应时按出口规格执行，
/// 因为 `Granularity::PerNode` 要求「按节点判」，而装配期还没有「节点」这个单位。
pub fn prepare_view(view: &CustomerView) -> FilteredView {
    let filtered = filter_view(view);
    let mut report = DropReport::default();
    report.from_filtered(&filtered);
    report.emit();
    filtered
}

/// 单份文本的闸（HTML 页 / 客户视图 JSON / 建模投影）。
fn gate_text(spec: OutletSpec, text: &str) -> Result<(), PipelineError> {
    if !spec.track.gated() {
        return Ok(());
    }
    let hits = scan_modeling_terms(text);
    if hits.is_empty() {
        Ok(())
    } else {
        Err(PipelineError::leaked(spec, hits))
    }
}

/// 两份文本**并集**判（`ontology overview` 的既有过度拦截，原样保留）。
fn gate_both(spec: OutletSpec, json_text: &str, text: &str) -> Result<(), PipelineError> {
    if !spec.track.gated() {
        return Ok(());
    }
    let mut hits = scan_modeling_terms(json_text);
    for h in scan_modeling_terms(text) {
        if !hits.contains(&h) {
            hits.push(h);
        }
    }
    if hits.is_empty() {
        Ok(())
    } else {
        Err(PipelineError::leaked(spec, hits))
    }
}

// ══════════════════════════════════════════════════════════════
// 33 条出口登记表（AC-002 / AC-007 / AC-010-1）
// ══════════════════════════════════════════════════════════════

/// 一条出口在管线里的**执行形态**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// 真的**经由**管线写出（客户 13 + D1 + Z1/Z2/Z3）。
    Pipeline,
    /// 建模轨：**显式声明豁免**，产物是人话行/自有闭包接线，本轮不改道（D2–D8）。
    DeclaredOnly,
    /// 内部运维输出：不过闸，也不需要判据（内部 9）。
    InternalOnly,
}

impl Enforcement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Enforcement::Pipeline => "pipeline",
            Enforcement::DeclaredOnly => "declared_only",
            Enforcement::InternalOnly => "internal_only",
        }
    }
}

/// 登记项。
#[derive(Debug, Clone, Copy)]
pub struct Outlet {
    /// 编号（**唯一**）。
    pub id: &'static str,
    /// 出口所在的源文件（相对 crate 根）。
    pub path: &'static str,
    /// 该出口的**写出点**或**管线调用点**的唯一源码子串。
    ///
    /// ⚠️ 已知边界：它是拼写级的。判据钉的是「登记与源码没有漂移」，
    /// 钉不住「语义等价改写」—— 补强在类型层与源码否定式。
    pub call_site: &'static str,
    /// 出口声明（格式 + 轨道 + 闸粒度）。
    pub spec: OutletSpec,
    pub enforcement: Enforcement,
    /// 豁免依据。**建模轨必须非空**（AC-007：「豁免不许沉默」）。
    pub exemption_reason: &'static str,
}

impl Outlet {
    pub fn track(&self) -> Track {
        self.spec.track
    }
    pub fn format(&self) -> Format {
        self.spec.format
    }
}

const CUSTOMER: &str = "";

/// ⭐ **33 条出口的机读登记表**（口径 D：CLI 22 + HTTP 4 + MCP 7）。
///
/// 分组：客户 13（C1–C5 / H1–H3 / M1–M5）+ 建模 8（D1–D8）+ 内部 9 + 静态 2（Z2,Z3）
/// + 错误路径 1（Z1）= **33**。
///
/// ⚠️ Z4（`INDEX_HTML` 模板）**不占出口位**（AC §七 A1）：它不是一次「写出」，
/// 只是被渲染函数读取的静态素材 ⇒ 单列在 [`STATIC_TEMPLATES`]。
pub const OUTLETS: &[Outlet] = &[
    // ── C1–C5 · CLI 客户类 5 条 ─────────────────────────────
    Outlet {
        id: "C1",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text(html)",
        spec: OutletSpec::C1,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C2",
        path: "src/cli.rs",
        // C2 与 C1 共用**同一份**产物（`html`），只是写到文件而不是 stdout。
        call_site: "std::fs::write(p, html)",
        spec: OutletSpec::C2,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C3",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_line(json);",
        spec: OutletSpec::C3,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C4",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text(text);",
        spec: OutletSpec::C4,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C5",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_line(json_text);",
        spec: OutletSpec::C5,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── H1–H3 · HTTP 3 条数据路由 ───────────────────────────
    Outlet {
        id: "H1",
        path: "src/server/http.rs",
        call_site: "OutletSpec::H1",
        spec: OutletSpec::H1,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "H2",
        path: "src/server/http.rs",
        call_site: "OutletSpec::H2",
        spec: OutletSpec::H2,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "H3",
        path: "src/server/http.rs",
        call_site: "OutletSpec::H3",
        spec: OutletSpec::H3,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── M1–M5 · MCP 5 个工具（同一调用点）────────────────────
    Outlet {
        id: "M1",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::MCP_TOOLS",
        spec: OutletSpec::MCP_TOOLS,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M2",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::MCP_TOOLS",
        spec: OutletSpec::MCP_TOOLS,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M3",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::MCP_TOOLS",
        spec: OutletSpec::MCP_TOOLS,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M4",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::MCP_TOOLS",
        spec: OutletSpec::MCP_TOOLS,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M5",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::MCP_TOOLS",
        spec: OutletSpec::MCP_TOOLS,
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── D1–D8 · 建模轨 8 条（**显式**声明，不靠「没调闸」推出来）──
    Outlet {
        id: "D1",
        path: "src/cli.rs",
        call_site: "OutletSpec::D1",
        spec: OutletSpec::D1,
        enforcement: Enforcement::Pipeline,
        exemption_reason: "建模术语在建模轨是**要被裁决的内容**，不是泄露（cli.rs 的 `--model-view` 分支原话 + `model_view.rs` 顶部）；轨道由 OutletSpec 声明，闸按声明不执行。",
    },
    Outlet {
        id: "D2",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text_with(&text,",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`review show` 是 FDE 通道的建模投影：每行无条件打 `key=`，过客户闸会恒命中。改用结构化溯源自检（`audit_fde_channel`），与词表**无交集**。",
    },
    Outlet {
        id: "D3",
        path: "src/cli.rs",
        call_site: "× 2 轨 → accepted",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`review decide --accept-all` 打的是裁决结果（提案 id / 条目数 / 轨道名）—— 建模动作回执，不是客户视图。",
    },
    Outlet {
        id: "D4",
        path: "src/cli.rs",
        call_site: "item {} 的 {} 轨 → {}",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`review decide --item/--track` 同上：单条裁决回执（序号 / 轨道 / 决定），零客户视图字段。",
    },
    Outlet {
        id: "D5",
        path: "src/cli.rs",
        call_site: "提案 {alias} 已导入，共",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`propose import` 打的是导入回执（别名 / 条目数 / 条目 kind + target_key）——target_key 是建模 key 本身，属建模轨内容。",
    },
    Outlet {
        id: "D6",
        path: "src/cli.rs",
        call_site: "被拦：{} —— {}",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`proposal apply` 的**被拦清单**是建模轨的诊断（被哪条领域校验挡住），payload 未转义是已登记缺陷（`cli.rs` 自陈 + REQ-078 G-6「载荷面=有」）。",
    },
    Outlet {
        id: "D7",
        path: "src/cli.rs",
        call_site: "提案已应用 → v{seq}",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`proposal apply` 成功回执（新版本号 + id）—— 建模动作结果，零客户视图字段。",
    },
    Outlet {
        id: "D8",
        path: "src/cli.rs",
        call_site: "应用失败：{e}",
        spec: OutletSpec::D1,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`proposal apply` 失败回执（领域错误原文）。⚠️ 失败也写 stdout 是既有行为，本轮不改（改它属于另一个需求的 stdout/stderr 口径统一）。",
    },
    // ── 内部 9 条（可不过闸，但必须**显式**标「内部」）─────────
    Outlet {
        id: "I1",
        path: "src/cli.rs",
        call_site: "概念声明 v{} 已导入提案",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：概念声明导入回执（指纹 / 规则 / 提案别名），运维面。",
    },
    Outlet {
        id: "I2",
        path: "src/cli.rs",
        call_site: "内容指纹\\t{d}",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`concept digest` 的指纹/版本/契约三行，校验工具输出。",
    },
    Outlet {
        id: "I3",
        path: "src/cli.rs",
        call_site: "项目 {} id={}",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`project new` 回执（项目名 + uuid）。",
    },
    Outlet {
        id: "I4",
        path: "src/cli.rs",
        call_site: "{n}\\t{i}",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`project list` 的 TSV。",
    },
    Outlet {
        id: "I5",
        path: "src/cli.rs",
        call_site: "新版本 seq={} id={}",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`version new` 回执。",
    },
    Outlet {
        id: "I6",
        path: "src/cli.rs",
        call_site: "v{}\\t{}\\t{}",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`version list` 的 TSV（序号 / id / 冻结状态）。",
    },
    Outlet {
        id: "I7",
        path: "src/cli.rs",
        call_site: "已冻结 v{}",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`version freeze` 回执。",
    },
    Outlet {
        id: "I8",
        path: "src/cli.rs",
        call_site: "无差异",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`ontology diff` 无差异。BA 已裁定（R1）：diff 归**建模轨**、豁免发布闸；换行不转义登记为已知限制（R3）。",
    },
    Outlet {
        id: "I9",
        path: "src/cli.rs",
        call_site: "共 {} 处变化：",
        spec: OutletSpec::INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`ontology diff` 有差异（逐条 `Change::summary()`）。同上 R1/R3。",
    },
    // ── 静态 2 条 + 错误路径 1 条 ───────────────────────────
    Outlet {
        id: "Z1",
        path: "src/server/http.rs",
        call_site: "OutletSpec::Z1",
        spec: OutletSpec::Z1,
        enforcement: Enforcement::Pipeline,
        exemption_reason: "错误路径：不过闸，但走 `safe_error`（命中即整句换掉），比闸更保守。",
    },
    Outlet {
        id: "Z2",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::Z2",
        spec: OutletSpec::Z2,
        enforcement: Enforcement::Pipeline,
        exemption_reason: "静态文本：AC-008 裁定**纳入运行时闸**（见 .feature/design-req107.md §五）。",
    },
    Outlet {
        id: "Z3",
        path: "src/mcp/serve.rs",
        call_site: "OutletSpec::Z3",
        spec: OutletSpec::Z3,
        enforcement: Enforcement::Pipeline,
        exemption_reason: "静态文本 + 动态项目名/版本：AC-008 裁定**纳入运行时闸**（测试守卫枚举不了动态部分）。",
    },
];

/// Z4 —— **不是出口**（AC §七 A1）：`INDEX_HTML` 只是被渲染函数读取的静态素材。
///
/// 它的守卫在这里登记，判据在测试里：
/// - **运行时**：H1 的管线对**渲染后的整页**（模板 + 已转义项目名）过闸 ⇒ 模板本体被覆盖；
/// - **测试**：`tests/test_graph_server.rs` 的 `the_page_itself_has_no_modeling_terms`
///   + `tests/test_req107_static_texts.rs` 的模板守卫断言。
pub const STATIC_TEMPLATES: &[(&str, &str)] = &[(
    "src/server/page.rs",
    "INDEX_HTML",
)];

/// 按 ID 取一条登记项。
pub fn outlet(id: &str) -> Option<&'static Outlet> {
    OUTLETS.iter().find(|o| o.id == id)
}

/// 按轨道分组计数（不做断言，断言在测试里）。
pub fn outlet_count(track: Track) -> usize {
    OUTLETS.iter().filter(|o| o.track() == track).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 轨道决定闸，不由出口决定。
    #[test]
    fn only_customer_and_static_tracks_are_gated() {
        assert!(Track::Customer.gated());
        assert!(Track::Static.gated(), "AC-008 裁定静态文本走运行时闸");
        assert!(!Track::Modeling.gated(), "建模轨是**显式豁免**，不是漏接");
        assert!(!Track::Internal.gated());
        assert!(!Track::ErrorPath.gated(), "错误路径走 safe_error，比闸更保守");
    }

    /// 登记表的口径必须**恰好**是 33，分组 13/8/9/2/1。
    #[test]
    fn registry_is_the_33_baseline() {
        assert_eq!(OUTLETS.len(), 33, "口径 D 是 33（见 acceptance.md §零）");
        assert_eq!(outlet_count(Track::Customer), 13);
        assert_eq!(outlet_count(Track::Modeling), 8);
        assert_eq!(outlet_count(Track::Internal), 9);
        assert_eq!(outlet_count(Track::Static), 2, "Z2/Z3；Z4 不是出口（A1）");
        assert_eq!(outlet_count(Track::ErrorPath), 1);
        // ID 唯一
        let mut ids: Vec<&str> = OUTLETS.iter().map(|o| o.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "ID 必须唯一");
    }

    /// 建模轨的豁免理由**不许沉默**（AC-007）。
    #[test]
    fn every_modeling_outlet_declares_a_reason() {
        for o in OUTLETS.iter().filter(|o| o.track() == Track::Modeling) {
            assert!(
                !o.exemption_reason.trim().is_empty(),
                "建模出口 {} 没有写豁免依据 —— 豁免不许靠沉默",
                o.id
            );
        }
    }

    /// 报告形状唯一：空报告不产文本，非空报告带固定抬头与四类计数。
    #[test]
    fn drop_report_has_one_shape() {
        let mut r = DropReport::default();
        assert!(r.stderr_line().is_empty());
        r.items.push(DropItem { kind: DropKind::WholeProp, label: "样例订单/客户编号".into() });
        r.items.push(DropItem { kind: DropKind::Object, label: "样例客户（对象名）".into() });
        r.items.push(DropItem { kind: DropKind::SilencedNote, label: "样例订单（对象说明）".into() });
        let line = r.stderr_line();
        assert!(line.starts_with(DROP_REPORT_PREFIX), "{line}");
        assert!(line.contains("共剔除 3 项"), "{line}");
        assert!(line.contains("对象名 1"), "{line}");
        assert!(line.contains("整条属性 1"), "{line}");
        assert!(line.contains("对象说明 1"), "{line}");
        assert!(line.contains("样例订单/客户编号"), "运维面必须带原名：{line}");
    }

    /// 声明为「建模」的产物不会被闸拦住；同一份产物声明为「客户」就必须被拦。
    /// 这是 AC-007「误标要能被观察到」的最小自证。
    #[test]
    fn mislabeling_a_modeling_product_turns_it_red() {
        let modeling = serde_json::json!({ "key": "样例主键", "cardinality": "1:N" });
        let text = serde_json::to_string_pretty(&modeling).unwrap();
        // 前提自检：这份产物**确实**含建模术语，否则下面两条是空转。
        assert!(!scan_modeling_terms(&text).is_empty());

        let ok = render(OutletSpec::D1, Product::Text(text.clone()));
        assert!(ok.is_ok(), "建模轨不该被闸拦住");

        let mut mislabelled = OutletSpec::D1;
        mislabelled.track = Track::Customer;
        let err = render(mislabelled, Product::Text(text)).expect_err("误标为客户类必须被拦");
        assert!(err.is_leak());
        assert!(!err.message().is_empty());
        assert!(!err.message().contains("主键"), "错误文案不许回显词条：{}", err.message());
    }

    /// 装配期前处理：过闸前就减掉标识符形态的属性，真·建模术语**不吃掉**。
    #[test]
    fn prepare_view_drops_identifiers_but_keeps_modeling_terms() {
        use crate::domain::view::{CustomerView, EntityView, PropertyView};
        let view = CustomerView {
            objects: vec![EntityView {
                name: "样例客户".into(),
                note: None,
                belongs_to: None,
                aliases: vec![],
                properties: vec![
                    PropertyView {
                        name: "样例关联的 key 凭据".into(),
                        note: None,
                        required: false,
                        sample: None,
                    },
                    PropertyView {
                        name: "主键标识".into(),
                        note: None,
                        required: false,
                        sample: None,
                    },
                ],
            }],
            relations: vec![],
        };
        let filtered = prepare_view(&view);
        assert_eq!(filtered.view.objects[0].properties.len(), 1);
        assert_eq!(filtered.view.objects[0].properties[0].name, "主键标识");
        assert!(!filtered.dropped.is_empty());
    }
}
