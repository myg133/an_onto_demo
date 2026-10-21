//! REQ-107 · 客户类出口的**唯一管线**（结构收口，不是新功能）。
//!
//! # 一句话
//!
//! 出口只交代**自己是谁**（[`OutletId`]，一个枚举），管线负责
//! 「查表得到轨道 / 格式 / 粒度 → 前处理（剔除 / 清洗）→ 发布闸 → 可写出的产物」。
//! 「拦不拦」由 `spec_of(id).track` 决定 —— 出口**连一个可写的轨道字段都拿不到**
//! （[`OutletSpec`] 全字段私有、无公开构造器、也不出现在任何公开函数的参数里）。
//!
//! # REQ-107 第 2 轮（QA 3 条阻断的修复）
//!
//! | 阻断 | 修法 |
//! | --- | --- |
//! | B2 · `spec.track` 可被出口赋值 | 出口只传 [`OutletId`]；轨道由 [`spec_of`] **查表**决定。`OutletSpec` 不再是 `pub` 字段的载体 |
//! | B3 · 出口可以「先写后判」 | 写出函数（`cli::emit_domain_text/line`）**只接受** [`OutletRender`]，拿裸 `&str` 写客户字节**编译不过** |
//! | B4 · 公开构造器只挡 `new` 拼写 | 客户产物的公开消费方法穷举登记；判据**枚举全部 `pub fn` 返回该类型**的定义点（不是匹配某个名字） |
//! | B1 · 新出口不接管线仍绿 | `tests/test_req107_source_enumeration.rs` 做**源码 → 登记表**枚举：扫 `src/` 全目录的写点与 [`OutletId`] 出现点，逐个对账 |
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
//! use ontok::domain::pipeline::{DropReport, OutletRender};
//! // 三个字段全是私有的 ⇒ 管线之外**字面量构造**不出来。
//! // ⚠️ 这里刻意把**三个字段都写全**：只写 `text` 的话，即使把 `text` 改成 `pub`，
//! //    这条 doctest 仍会因「缺字段」而编不过 ⇒ 它证明的就不是「字段私有」了。
//! let forged = OutletRender {
//!     text: String::from("样例客户"),
//!     report: DropReport::default(),
//!     outlet: "C3",
//! };
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
//! # 44 条出口登记表（AC-002 / AC-007）
//!
//! [`OUTLETS`] 是**机读**的：每条带 `path` + `call_site`（该出口调用管线的源码片段
//! 唯一子串）+ 轨道 + 格式。判据在 `tests/test_req107_*` 里做**双向对账**
//! （条数 / 分组 / ID 集合 / needle 命中）。
//!
//! ⚠️ REQ-107 收口时是 33 条；REQ-110 新增 **6** 条（TTL 导出：C6 / C7 / H4 / H5 / D9 / D10），
//! REQ-111 又新增 **5** 条（本地审核界面：R1–R5，**全部内部轨**），
//! 所以口径变成 44（客户 17 / 建模 10 / 内部 14 / 静态 2 / 错误路径 1）。
//! 条数变了不是「放宽」—— 每一条新出口都带自己的写点与证据（T1/T3/T8 逐个对账）。
//!
//! ⚠️ **已知边界（⛔ 不许写成「已封堵」）**：needle 式对账抓得住「登记与源码漂移」，
//! **抓不住**「语义等价改写」。补强在类型层（本模块 §类型层不变量）与源码否定式。

use crate::domain::html_view::{render_html, RenderedPage};
use crate::domain::model_view::ModelTrackView;
use crate::domain::overview::{render_overview_text, Overview};
use crate::domain::property_filter::{filter_view, FilteredView};
use crate::domain::ttl::{serialize_customer, serialize_model, TtlOptions};
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
    ///
    /// ⛔ 第 2 轮起：**字段全部私有**。出口再也拿不到这个类型的一个值 ——
    /// 它只作为 [`OUTLETS`] 的私字段与管线内部查表结果存在。
    id: &'static str,
    track: Track,
    format: Format,
    granularity: Granularity,
}

/// `OutletSpec` 的**唯一构造方式**：模块私有（⛔ 没有 `pub fn new`、没有 `pub const`）。
impl OutletSpec {
    const fn of(
        id: &'static str,
        track: Track,
        format: Format,
        granularity: Granularity,
    ) -> Self {
        Self { id, track, format, granularity }
    }

    /// 编号（人读 / 报错定位）。
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// 轨道。**只读** —— 出口手上根本没有 [`OutletSpec`] 的值可赋值。
    pub const fn track(self) -> Track {
        self.track
    }

    pub const fn format(self) -> Format {
        self.format
    }

    pub const fn granularity(self) -> Granularity {
        self.granularity
    }
}

/// ⭐ REQ-107 第 2 轮 · 出口的**唯一身份**。
///
/// 出口只能交代「我是哪一条出口」，**不能**交代「我这条要不要过闸」。
/// 轨道 / 格式 / 粒度由 [`spec_of`] **查表**决定 —— 于是
/// 「出口把 `spec.track` 改成 `Track::Internal` 从而关掉闸门」（QA 变异 #4）
/// 在源码里**表达不出来**：既没有 `OutletSpec` 的值，也没有可写的字段。
///
/// 新增一条出口 ⇒ 必须在这里加一个变体 + 在 [`spec_of`] 里登记轨道，
/// 源码枚举判据（`tests/test_req107_source_enumeration.rs`）会逐个对账
/// `src/` 里出现的 `OutletId::` 记号，未登记的组合立刻红。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutletId {
    // ── 客户类 15 条（REQ-110 加了 C6/C7/H4/H5）──
    C1,
    C2,
    C3,
    C4,
    C5,
    /// REQ-110 · `ontology export`：客户视图 TTL → stdout。
    C6,
    /// REQ-110 · `ontology export --out`：客户视图 TTL → 文件（与 C6 同一份字节）。
    C7,
    H1,
    H2,
    H3,
    /// REQ-110 · `GET /api/ontology.ttl`（**仍要 token**）。
    H4,
    /// REQ-110 · `GET /api/scope`（供静态页面壳取「看哪个项目 / 哪一版」，**要 token**）。
    H5,
    /// M1–M5 共用**同一个**调用点（`serve.rs` 的 `tools/call` 分支）。
    McpTools,
    // ── 零闸位点 ──
    Z1,
    Z2,
    Z3,
    // ── 建模轨 ──
    D1,
    /// REQ-110 · `ontology export --model-view`：建模视图 TTL → stdout（显式豁免闸）。
    D9,
    /// REQ-110 · `ontology export --model-view --out`：建模视图 TTL → 文件（同 D9）。
    D10,
    // ── REQ-111 · 本地审核界面（内部：FDE 通道，只绑回环 + 口令）──
    /// `GET /api/review` —— 条目 + 双轨状态 + 置信度 + AI 理由 + 证据。
    R1,
    /// `POST /api/decide` —— 唯一写点的**回执**（写本身走 `review decide` 的同一函数）。
    R2,
    /// `GET /api/preview` —— 批量签的**预览**（条数 + `items[]` 白名单）。
    R3,
    /// `GET /` —— 审核页面壳（**零数据**）。
    R4,
    /// 审核界面的错误通道（正文是 `{"error": …}`，含服务端给的具体原因）。
    R5,
}

impl OutletId {
    /// 登记表里的编号（报错与登记对账用）。
    pub const fn code(self) -> &'static str {
        match self {
            OutletId::C1 => "C1",
            OutletId::C2 => "C2",
            OutletId::C3 => "C3",
            OutletId::C4 => "C4",
            OutletId::C5 => "C5",
            OutletId::C6 => "C6",
            OutletId::C7 => "C7",
            OutletId::H1 => "H1",
            OutletId::H2 => "H2",
            OutletId::H3 => "H3",
            OutletId::H4 => "H4",
            OutletId::H5 => "H5",
            OutletId::McpTools => "M1-M5",
            OutletId::Z1 => "Z1",
            OutletId::Z2 => "Z2",
            OutletId::Z3 => "Z3",
            OutletId::D1 => "D1",
            OutletId::D9 => "D9",
            OutletId::D10 => "D10",
            OutletId::R1 => "R1",
            OutletId::R2 => "R2",
            OutletId::R3 => "R3",
            OutletId::R4 => "R4",
            OutletId::R5 => "R5",
        }
    }
}

/// ⭐ **轨道 / 格式 / 粒度的唯一事实源**：出口 ID → 规格。
///
/// ⛔ 模块私有。出口**给不出一条规格**，只能给出 ID；因此
/// 「这条出口要不要过闸」这件事在出口代码里没有表达形式。
///
/// D2–D8 / I1–I9 / Z4 不在这里：它们不真走管线，轨道只在 [`OUTLETS`]
/// 里作为**登记事实**出现（[`Enforcement::DeclaredOnly`] / [`Enforcement::InternalOnly`]）。
const fn spec_of(id: OutletId) -> OutletSpec {
    match id {
        OutletId::C1 => OutletSpec::of("C1", Track::Customer, Format::Html, Granularity::Whole),
        OutletId::C2 => OutletSpec::of("C2", Track::Customer, Format::Html, Granularity::Whole),
        OutletId::C3 => OutletSpec::of("C3", Track::Customer, Format::Json, Granularity::Whole),
        OutletId::C4 => OutletSpec::of("C4", Track::Customer, Format::Text, Granularity::Whole),
        OutletId::C5 => OutletSpec::of("C5", Track::Customer, Format::Json, Granularity::Whole),
        // REQ-110 · TTL 导出：客户轨 / 文本 / 整份。
        //
        // ⚠️ 门禁在**序列化之后**（`ttl::TtlDocument::body`）—— 见 `render` 的
        // `CustomerTurtle` 分支与 `ttl.rs` 模块头（AC-002 与 AC-008 的冲突取舍）。
        OutletId::C6 => OutletSpec::of("C6", Track::Customer, Format::Text, Granularity::Whole),
        // C7 与 C6 写**同一份字节**（`--out` 文件 vs stdout），只是落点不同。
        OutletId::C7 => OutletSpec::of("C7", Track::Customer, Format::Text, Granularity::Whole),
        OutletId::H1 => OutletSpec::of("H1", Track::Customer, Format::Html, Granularity::Whole),
        OutletId::H2 => OutletSpec::of("H2", Track::Customer, Format::Json, Granularity::Whole),
        OutletId::H4 => OutletSpec::of("H4", Track::Customer, Format::Text, Granularity::Whole),
        OutletId::H5 => OutletSpec::of("H5", Track::Customer, Format::Json, Granularity::Whole),
        // ⚠️ **逐节点**：一个坏节点只拒发那一个节点（卡点②的裁定）。
        OutletId::H3 => OutletSpec::of("H3", Track::Customer, Format::Json, Granularity::PerNode),
        OutletId::McpTools => {
            OutletSpec::of("M1-M5", Track::Customer, Format::Json, Granularity::Whole)
        }
        // Z1 `check_scope` 的不匹配分支：不过闸，但走 `safe_error`（更保守）。
        OutletId::Z1 => OutletSpec::of("Z1", Track::ErrorPath, Format::Json, Granularity::Whole),
        // Z2 `tools/list`（AC-008：运行时闸）。
        OutletId::Z2 => OutletSpec::of("Z2", Track::Static, Format::Json, Granularity::Whole),
        // Z3 `initialize` 的整段 result（模板正文 + serverInfo + 项目名）。
        OutletId::Z3 => OutletSpec::of("Z3", Track::Static, Format::Json, Granularity::Whole),
        // D1 `ontology show --model-view`：**唯一**真走管线的建模出口。
        OutletId::D1 => OutletSpec::of("D1", Track::Modeling, Format::Json, Granularity::Whole),
        // REQ-110 · 建模视图 TTL：同属建模轨（显式豁免，理由见 OUTLETS 的
        // `exemption_reason`），产物是文本而不是 JSON。
        OutletId::D9 => OutletSpec::of("D9", Track::Modeling, Format::Text, Granularity::Whole),
        OutletId::D10 => OutletSpec::of("D10", Track::Modeling, Format::Text, Granularity::Whole),
        // REQ-111 · 本地审核界面：**内部**轨（FDE 通道）。
        //
        // 为什么不是 `Modeling`：建模轨在登记表里是「D1–D10 这 10 条」这一组
        // **精确集合**（`tests/test_req107_track_manifest.rs` 钉住），
        // 那组的口径是「同一份客户产物的建模视图」。审核界面产出的是
        // **审核对象的原始候选定义**（key / 列 / 基数 / 置信度 / 理由），
        // 既不是客户视图、也不是建模视图 —— 它是**内部工具**的数据面。
        // `Internal` 的语义（不过闸、内部运维）正是它，且不会把「建模轨 10 条」
        // 这条既有不变量改宽。
        OutletId::R1 => OutletSpec::of("R1", Track::Internal, Format::Json, Granularity::Whole),
        OutletId::R2 => OutletSpec::of("R2", Track::Internal, Format::Json, Granularity::Whole),
        OutletId::R3 => OutletSpec::of("R3", Track::Internal, Format::Json, Granularity::Whole),
        OutletId::R4 => OutletSpec::of("R4", Track::Internal, Format::Html, Granularity::Whole),
        OutletId::R5 => OutletSpec::of("R5", Track::Internal, Format::Json, Granularity::Whole),
    }
}

/// 登记表里「非管线轨」的规格占位（轨道是**登记事实**，出口不消费它）。
const MODELING: OutletSpec = OutletSpec::of("D", Track::Modeling, Format::Text, Granularity::Whole);
const INTERNAL: OutletSpec = OutletSpec::of("I", Track::Internal, Format::Text, Granularity::Whole);

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
    /// ⭐ REQ-110 · 客户视图 → **Turtle**（CLI `ontology export` / HTTP `/api/ontology.ttl`）。
    ///
    /// 前处理（属性级剔除）与发布闸都在**本管线**里，出口拿不到「序列化好但没过闸」
    /// 的字节。闸扫的是 `TtlDocument::body`（数据正文），理由见 `ttl.rs` 模块头。
    ///
    /// ⛔ C6 / C7 / H4 三条出口共用**这一个**产物：TTL 只有一份序列化实现
    /// （纪律 H），`--out` 与 stdout / HTTP 写的是同一批字节。
    CustomerTurtle {
        view: &'a CustomerView,
        opts: TtlOptions<'a>,
    },
    /// ⭐ REQ-110 · 建模视图 → **Turtle**（`ontology export --model-view`）。
    /// 建模轨显式豁免发布闸（建模术语在这一轨是要被裁决的内容）。
    ModelTurtle {
        view: &'a ModelTrackView,
        opts: TtlOptions<'a>,
    },
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

/// 文本轨的**唯一入口**（CLI 的五条客户出口、首页、建模投影、错误文案）。
///
/// 第一个参数是 [`OutletId`]（**不是** spec）：出口交代自己是谁，
/// 轨道 / 格式 / 粒度在这里查表得到。
///
/// 命中发布闸 ⇒ `Err(PipelineError::Leaked)`；**一个字节都不会产出**。
pub fn render(id: OutletId, product: Product<'_>) -> Result<OutletRender, PipelineError> {
    let spec = spec_of(id);
    // ① 产物形态必须与出口身份相容（第 2 轮新增）。
    //
    // 这一条堵的是「换一个 ID 来关闸」：`Product::CustomerJson` 只能从
    // `Track::Customer` 的出口进来 —— 拿 `OutletId::D1`（建模轨，不过闸）
    // 去发布客户视图会被当场拒绝，而不是静默绕过闸门。
    if !pairing_ok(spec, &product) {
        return Err(PipelineError::render(
            spec,
            format!(
                "出口 {} 的产物形态与轨道不相容（{}）：客户类产物只能从客户轨入口发布",
                spec.id,
                spec.track.as_str()
            ),
        ));
    }
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

        // ⭐ REQ-110 · 客户视图 TTL。
        //
        // 顺序与前处理/闸的既有纪律一致：**先剔、再序列化、再判、最后才产出可写出的字节**。
        // 闸扫的是**序列化之后的正文**（不是结构字段）—— 例如属性名/说明里的
        // 「主键」会原样变成 `rdfs:comment` 字面量，扫结构字段会漏掉序列化过程
        // 自己引入的文本（IRI 本地名、标签、注释都算）。
        //
        // ⚠️ 与 `CustomerJson` 同一份前处理（`filter_view`），所以 CLI 导出的
        // 「客户视图 TTL」与 `/api/graph` 看到的是同一批属性（不多不少）。
        Product::CustomerTurtle { view, opts } => {
            let filtered = filter_view(view);
            let mut report = DropReport::default();
            report.from_filtered(&filtered);
            report.emit();
            let doc = serialize_customer(&filtered.view, &opts);
            gate_text(spec, doc.body())?;
            Ok(OutletRender::build(spec, doc.into_doc(), report))
        }

        // ⭐ REQ-110 · 建模视图 TTL。轨道是 `Track::Modeling` ⇒ `gate_text` 直接放行
        // （**显式豁免**，不是「忘了调闸」：`spec_of(OutletId::D9)` 查表决定）。
        Product::ModelTurtle { view, opts } => {
            let doc = serialize_model(view, &opts);
            gate_text(spec, doc.body())?;
            Ok(OutletRender::build(spec, doc.into_doc(), DropReport::default()))
        }

        Product::ErrorText(raw) => {
            // 比闸更保守：命中就是**整句换掉**（`safe_error` 的既有语义）。
            //
            // ⚠️ 干净文案**原样返回** —— 运维必须能看出「本服务绑的是哪个项目」
            // （`tests/test_req107_scope_error_gate.rs` 的对照组钉住这一点）。
            Ok(OutletRender::build(spec, crate::mcp::tools::safe_error(&raw), DropReport::default()))
        }
    }
}

/// 产物形态 ↔ 轨道 的相容性（[`render`] 的第一道运行时不变式）。
///
/// | 产物 | 允许的轨道 | 理由 |
/// | --- | --- | --- |
/// | `CustomerHtml` / `CustomerJson` / `Overview` | **只能** `Customer` | 客户视图必须过闸；换 ID 不能关闸 |
/// | `Text` | `Customer`（过闸）或 `D1`（建模豁免） | 首页整页 / 建模投影 |
/// | `ErrorText` | **只能** `ErrorPath` | 只有 Z1 走 `safe_error` 改写语义 |
fn pairing_ok(spec: OutletSpec, product: &Product<'_>) -> bool {
    match product {
        Product::CustomerHtml { .. }
        | Product::CustomerJson(_)
        | Product::Overview(_)
        | Product::CustomerTurtle { .. } => spec.track == Track::Customer,
        Product::Text(_) => spec.track == Track::Customer || spec.id == "D1",
        Product::ModelTurtle { .. } => spec.track == Track::Modeling,
        Product::ErrorText(_) => spec.track == Track::ErrorPath,
    }
}

/// 结构化轨的唯一入口（HTTP `/api/graph` `/api/node`、MCP 全部响应、首页不由它走）。
///
/// 命中 ⇒ `Err`，消息**逐字**取自 `mcp::tools::guard` —— 所以 500 正文与
/// `isError: true` 的文案在收口前后**一个字节都不变**。
pub fn publish_value(id: OutletId, value: Value) -> Result<GuardedValue, PipelineError> {
    let spec = spec_of(id);
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
// 44 条出口登记表（AC-002 / AC-007 / AC-010-1）
// ══════════════════════════════════════════════════════════════

/// 一条出口在管线里的**执行形态**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// 真的**经由**管线写出（客户 13 + D1 + Z1/Z2/Z3）。
    Pipeline,
    /// 建模轨：**显式声明豁免**，产物是人话行/自有闭包接线，本轮不改道（D2–D8）。
    DeclaredOnly,
    /// 内部运维输出：不过闸，也不需要判据（内部 14，含 REQ-111 的 R1–R5）。
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
    /// 钉不住「语义等价改写」—— 补强在类型层与源码枚举
    /// （`tests/test_req107_source_enumeration.rs` 做**源码 → 登记表**方向的枚举，
    /// 每个写点/`OutletId::` 记号都必须逐个落在登记集合里）。
    pub call_site: &'static str,
    /// 出口声明（格式 + 轨道 + 闸粒度）。
    ///
    /// ⛔ **私有字段**（第 2 轮）：出口连一个 `OutletSpec` 的值都拿不到，
    /// 更谈不上去改它的 `track`。读轨道请走 `track()` / `format()` / `granularity()`。
    spec: OutletSpec,
    pub enforcement: Enforcement,
    /// 豁免依据。**建模轨必须非空**（AC-007：「豁免不许沉默」）。
    pub exemption_reason: &'static str,
}

impl Outlet {
    pub fn track(&self) -> Track {
        self.spec.track()
    }
    pub fn format(&self) -> Format {
        self.spec.format()
    }
    /// 闸粒度（卡点②的裁定结果）。只读 —— 出口无权改。
    pub fn granularity(&self) -> Granularity {
        self.spec.granularity()
    }
}

const CUSTOMER: &str = "";

/// ⭐ **44 条出口的机读登记表**（口径 D + REQ-110 的 6 条 TTL 出口 + REQ-111 的 5 条审核界面出口）。
///
/// 分组：客户 17（C1–C7 / H1–H5 / M1–M5）+ 建模 10（D1–D8 / D9 / D10）+ 内部 14 +
/// 静态 2（Z2,Z3）+ 错误路径 1（Z1）= **44**。
///
/// ⚠️ Z4（`INDEX_HTML` 模板）**不占出口位**（AC §七 A1）：它不是一次「写出」，
/// 只是被渲染函数读取的静态素材 ⇒ 单列在 [`STATIC_TEMPLATES`]。
pub const OUTLETS: &[Outlet] = &[
    // ── C1–C5 · CLI 客户类 5 条 ─────────────────────────────
    Outlet {
        id: "C1",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text(html)",
        spec: spec_of(OutletId::C1),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C2",
        path: "src/cli.rs",
        // C2 与 C1 共用**同一份**产物（`html`），只是写到文件而不是 stdout。
        call_site: "std::fs::write(p, html.as_str())",
        spec: spec_of(OutletId::C2),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C3",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_line(json);",
        spec: spec_of(OutletId::C3),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C4",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text(text);",
        spec: spec_of(OutletId::C4),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C5",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_line(json_text);",
        spec: spec_of(OutletId::C5),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── REQ-110 · C6 / C7 · `ontology export` 的两条客户出口 ──
    Outlet {
        id: "C6",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text(customer_ttl)",
        spec: spec_of(OutletId::C6),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "C7",
        path: "src/cli.rs",
        // C7 与 C6 写**同一份产物**（`customer_ttl`），只是写到文件而不是 stdout。
        call_site: "std::fs::write(p, customer_ttl.as_str())",
        spec: spec_of(OutletId::C7),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── H1–H3 · HTTP 3 条数据路由 ───────────────────────────
    Outlet {
        id: "H1",
        path: "src/server/http.rs",
        call_site: "Ok(Html(rendered.into_text()))",
        spec: spec_of(OutletId::H1),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "H2",
        path: "src/server/http.rs",
        call_site: "Ok(Json(rendered.into_value()))",
        spec: spec_of(OutletId::H2),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "H3",
        path: "src/server/http.rs",
        call_site: "Ok(Json(rendered.into_value()))",
        spec: spec_of(OutletId::H3),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── REQ-110 · H4 / H5 · 两条新 HTTP 出口（**都要 token**）──
    //
    // H4 的响应体不是 `Json(`/`Html(`（是 `text/turtle` + `Content-Disposition`），
    // 所以它的写点在 `get_ttl` 里以 `Ok((headers, …))` 的形状出现 ——
    // `tests/test_req107_source_enumeration.rs` 的 `WRITE_CONSTRUCTS` 已同步收口该形状。
    Outlet {
        id: "H4",
        path: "src/server/http.rs",
        call_site: "Ok((headers, rendered.into_text()))",
        spec: spec_of(OutletId::H4),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // H5 与 H2/H3 **共用** `publish_json` 那个写点（同一个 `Json(` 构造）。
    Outlet {
        id: "H5",
        path: "src/server/http.rs",
        call_site: "Ok(Json(rendered.into_value()))",
        spec: spec_of(OutletId::H5),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── M1–M5 · MCP 5 个工具（同一调用点）────────────────────
    Outlet {
        id: "M1",
        path: "src/mcp/serve.rs",
        call_site: "p::tool_call_result(&text, structured)",
        spec: spec_of(OutletId::McpTools),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M2",
        path: "src/mcp/serve.rs",
        call_site: "p::tool_call_result(&text, structured)",
        spec: spec_of(OutletId::McpTools),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M3",
        path: "src/mcp/serve.rs",
        call_site: "p::tool_call_result(&text, structured)",
        spec: spec_of(OutletId::McpTools),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M4",
        path: "src/mcp/serve.rs",
        call_site: "p::tool_call_result(&text, structured)",
        spec: spec_of(OutletId::McpTools),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    Outlet {
        id: "M5",
        path: "src/mcp/serve.rs",
        call_site: "p::tool_call_result(&text, structured)",
        spec: spec_of(OutletId::McpTools),
        enforcement: Enforcement::Pipeline,
        exemption_reason: CUSTOMER,
    },
    // ── D1–D8 · 建模轨 8 条（**显式**声明，不靠「没调闸」推出来）──
    Outlet {
        id: "D1",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_line(mv_json);",
        spec: spec_of(OutletId::D1),
        enforcement: Enforcement::Pipeline,
        exemption_reason: "建模术语在建模轨是**要被裁决的内容**，不是泄露（cli.rs 的 `--model-view` 分支原话 + `model_view.rs` 顶部）；轨道由 `spec_of(OutletId::D1)` 查表声明，闸按查表结果不执行。",
    },
    // ── REQ-110 · D9 / D10 · `ontology export --model-view` 的两条建模出口 ──
    //
    // 为什么与 D1 分开登记：写点不同（TTL 序列化 vs JSON 投影），而且 `--out`
    // 与 stdout 也各算一条出口（与 C1/C2、C6/C7 同一条口径）。
    Outlet {
        id: "D9",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text(model_ttl)",
        spec: spec_of(OutletId::D9),
        enforcement: Enforcement::Pipeline,
        exemption_reason: "建模轨 TTL：`--model-view` 显式选择，建模术语（key / 列名 / 基数 / semantic）在这一轨是**要被裁决的内容**；轨道由 `spec_of(OutletId::D9)` 查表声明（`Track::Modeling`），闸按查表结果不执行，⛔ 不是「没调闸所以豁免」。",
    },
    Outlet {
        id: "D10",
        path: "src/cli.rs",
        call_site: "std::fs::write(p, model_ttl.as_str())",
        spec: spec_of(OutletId::D10),
        enforcement: Enforcement::Pipeline,
        exemption_reason: "同 D9：`--out` 写的是**同一份**建模视图 TTL 字节（`model_ttl`），只是落到文件而不是 stdout。",
    },
    // ── REQ-111 · R1–R5 · 本地审核界面（内部轨：FDE 通道）──────────────
    //
    // 为什么**必须登记**：审核界面是新增的客户端可见写出口（浏览器的页面 + JSON）。
    // 「不登记 = 没人管」正是 REQ-107 收口的那个洞；新增写出口必须进这张表，
    // 而 `tests/test_req107_source_enumeration.rs` 的 T1 会从 `src/` **全目录**
    // 枚举，任何没登记的写点当场红。
    //
    // 为什么不走管线：这五条的语气与 D2–D8 / I1–I9 相同 —— 产物是**审核对象的
    // 原始候选定义**（key / 列 / 基数 / 置信度 / AI 理由）与**运维回执**，
    // 过客户闸会恒命中（那些词在客户轨是泄露、在这一轨是内容）。
    // 与 D2–D8 的区别：它们打 stdout，这里回 HTTP；与 `serve` 的区别：
    // 这是**内部**工具（只绑回环 + 每个 `/api/*` 要口令），不是客户出口。
    Outlet {
        id: "R1",
        path: "src/review_ui/http.rs",
        call_site: "Ok(Json(review_body))",
        spec: spec_of(OutletId::R1),
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`GET /api/review` 的条目列表（双轨状态 / 置信度 / AI 理由 / 证据 / 候选定义）。客户视图零读取；只绑回环 + Bearer 口令。",
    },
    Outlet {
        id: "R2",
        path: "src/review_ui/http.rs",
        call_site: "Ok(Json(decide_body))",
        spec: spec_of(OutletId::R2),
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`POST /api/decide` 的**回执**（写入后的双轨状态 + 审计 via/decided_at + 进度）。写本身复用 `review decide` 的 `decide_item`，不在 HTTP 层写 SQL。",
    },
    Outlet {
        id: "R3",
        path: "src/review_ui/http.rs",
        call_site: "Ok(Json(preview_body))",
        spec: spec_of(OutletId::R3),
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`GET /api/preview` 的批量预览（条数 + items[] 白名单）。只读；它是 AC-006「先看条数再执行」的数据来源，服务端没有批量写接口。",
    },
    Outlet {
        id: "R4",
        path: "src/review_ui/http.rs",
        call_site: "Html(crate::review_ui::page::REVIEW_HTML)",
        spec: spec_of(OutletId::R4),
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：审核页面**壳**（零数据：无项目名 / 无条目 / 无决策）。数据全走 R1–R3（都要口令）；浏览器导航带不了 Authorization，所以壳必须免口令。",
    },
    Outlet {
        id: "R5",
        path: "src/review_ui/http.rs",
        call_site: "Json(error_body(",
        spec: spec_of(OutletId::R5),
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：审核界面的**错误通道**（`{\"error\": …}`）。正文必须带具体原因（AC-007：已冻结 / 没有第 N 个条目 / 参数错），否则页面只能显示「失败」二字。不含令牌。",
    },
    Outlet {
        id: "D2",
        path: "src/cli.rs",
        call_site: "crate::cli::emit_domain_text_with(&text,",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`review show` 是 FDE 通道的建模投影：每行无条件打 `key=`，过客户闸会恒命中。改用结构化溯源自检（`audit_fde_channel`），与词表**无交集**。",
    },
    Outlet {
        id: "D3",
        path: "src/cli.rs",
        call_site: "× 2 轨 → accepted",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`review decide --accept-all` 打的是裁决结果（提案 id / 条目数 / 轨道名）—— 建模动作回执，不是客户视图。",
    },
    Outlet {
        id: "D4",
        path: "src/cli.rs",
        call_site: "item {} 的 {} 轨 → {}",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`review decide --item/--track` 同上：单条裁决回执（序号 / 轨道 / 决定），零客户视图字段。",
    },
    Outlet {
        id: "D5",
        path: "src/cli.rs",
        call_site: "提案 {alias} 已导入，共",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`propose import` 打的是导入回执（别名 / 条目数 / 条目 kind + target_key）——target_key 是建模 key 本身，属建模轨内容。",
    },
    Outlet {
        id: "D6",
        path: "src/cli.rs",
        call_site: "被拦：{} —— {}",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`proposal apply` 的**被拦清单**是建模轨的诊断（被哪条领域校验挡住），payload 未转义是已登记缺陷（`cli.rs` 自陈 + REQ-078 G-6「载荷面=有」）。",
    },
    Outlet {
        id: "D7",
        path: "src/cli.rs",
        call_site: "提案已应用 → v{seq}",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`proposal apply` 成功回执（新版本号 + id）—— 建模动作结果，零客户视图字段。",
    },
    Outlet {
        id: "D8",
        path: "src/cli.rs",
        call_site: "应用失败：{e}",
        spec: MODELING,
        enforcement: Enforcement::DeclaredOnly,
        exemption_reason: "`proposal apply` 失败回执（领域错误原文）。⚠️ 失败也写 stdout 是既有行为，本轮不改（改它属于另一个需求的 stdout/stderr 口径统一）。",
    },
    // ── 内部 9 条（可不过闸，但必须**显式**标「内部」）─────────
    Outlet {
        id: "I1",
        path: "src/cli.rs",
        call_site: "概念声明 v{} 已导入提案",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：概念声明导入回执（指纹 / 规则 / 提案别名），运维面。",
    },
    Outlet {
        id: "I2",
        path: "src/cli.rs",
        call_site: "内容指纹\\t{d}",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`concept digest` 的指纹/版本/契约三行，校验工具输出。",
    },
    Outlet {
        id: "I3",
        path: "src/cli.rs",
        call_site: "项目 {} id={}",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`project new` 回执（项目名 + uuid）。",
    },
    Outlet {
        id: "I4",
        path: "src/cli.rs",
        call_site: "{n}\\t{i}",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`project list` 的 TSV。",
    },
    Outlet {
        id: "I5",
        path: "src/cli.rs",
        call_site: "新版本 seq={} id={}",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`version new` 回执。",
    },
    Outlet {
        id: "I6",
        path: "src/cli.rs",
        call_site: "v{}\\t{}\\t{}",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`version list` 的 TSV（序号 / id / 冻结状态）。",
    },
    Outlet {
        id: "I7",
        path: "src/cli.rs",
        call_site: "已冻结 v{}",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`version freeze` 回执。",
    },
    Outlet {
        id: "I8",
        path: "src/cli.rs",
        call_site: "无差异",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`ontology diff` 无差异。BA 已裁定（R1）：diff 归**建模轨**、豁免发布闸；换行不转义登记为已知限制（R3）。",
    },
    Outlet {
        id: "I9",
        path: "src/cli.rs",
        call_site: "共 {} 处变化：",
        spec: INTERNAL,
        enforcement: Enforcement::InternalOnly,
        exemption_reason: "内部：`ontology diff` 有差异（逐条 `Change::summary()`）。同上 R1/R3。",
    },
    // ── 静态 2 条 + 错误路径 1 条 ───────────────────────────
    Outlet {
        id: "Z1",
        path: "src/server/http.rs",
        call_site: "err(StatusCode::BAD_REQUEST, rendered.as_str())",
        spec: spec_of(OutletId::Z1),
        enforcement: Enforcement::Pipeline,
        exemption_reason: "错误路径：不过闸，但走 `safe_error`（命中即整句换掉），比闸更保守。",
    },
    Outlet {
        id: "Z2",
        path: "src/mcp/serve.rs",
        call_site: "p::encode_result(id, rendered.into_value())",
        spec: spec_of(OutletId::Z2),
        enforcement: Enforcement::Pipeline,
        exemption_reason: "静态文本：AC-008 裁定**纳入运行时闸**（见 .feature/design-req107.md §五）。",
    },
    Outlet {
        id: "Z3",
        path: "src/mcp/serve.rs",
        call_site: "p::encode_result(id, rendered.into_value())",
        spec: spec_of(OutletId::Z3),
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

    /// 登记表的口径必须**恰好**是 44，分组 17/10/14/2/1。
    ///
    /// ⚠️ REQ-107 收口时是 33（13/8/9/2/1）；REQ-110 新增 6 条 TTL 出口
    /// （C6/C7/H4/H5 客户 + D9/D10 建模）⇒ 39（17/10/9/2/1）；
    /// REQ-111 新增 5 条审核界面出口（R1–R5，**全部内部轨**）⇒ 44（17/10/14/2/1）。
    /// 这条断言**仍然逐项精确相等**：多一条少一条、换一条轨道都会红。
    #[test]
    fn registry_is_the_declared_baseline() {
        assert_eq!(
            OUTLETS.len(),
            44,
            "口径 = 33（REQ-107）+ 6（REQ-110 的 TTL 出口）+ 5（REQ-111 的审核界面出口）"
        );
        assert_eq!(outlet_count(Track::Customer), 17);
        assert_eq!(outlet_count(Track::Modeling), 10);
        assert_eq!(
            outlet_count(Track::Internal),
            14,
            "内部 9 + REQ-111 的 R1–R5（审核界面是内部工具，不是客户/建模出口）"
        );
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

    /// 声明为「建模」的产物不会被闸拦住；同一份产物走**客户轨**就必须被拦。
    /// 这是 AC-007「误标要能被观察到」的最小自证（第 2 轮起：轨道由 `OutletId` 查表，
    /// 所以在 crate 内也要用两条不同的 ID 表达「同一份产物、不同轨道」）。
    #[test]
    fn mislabeling_a_modeling_product_turns_it_red() {
        let modeling = serde_json::json!({ "key": "样例主键", "cardinality": "1:N" });
        let text = serde_json::to_string_pretty(&modeling).unwrap();
        // 前提自检：这份产物**确实**含建模术语，否则下面两条是空转。
        assert!(!scan_modeling_terms(&text).is_empty());

        let ok = render(OutletId::D1, Product::Text(text.clone()));
        assert!(ok.is_ok(), "建模轨不该被闸拦住");
        assert_eq!(ok.unwrap().as_str(), text, "建模轨必须逐字节原样输出");

        // 干净文本走客户轨 ⇒ 放行（证明「客户轨」不是恒红）。
        let clean = r#"{"name":"样例客户"}"#.to_string();
        assert!(render(OutletId::C3, Product::Text(clean)).is_ok());

        let err = render(OutletId::C3, Product::Text(text.clone()))
            .expect_err("客户轨遇到建模术语必须被拦（轨道决定，不是出口决定）");
        assert!(err.is_leak());
        assert!(!err.message().is_empty());
        assert!(!err.message().contains("主键"), "错误文案不许回显词条：{}", err.message());

        // ⭐ 第 2 轮：**换 ID 关闸**这条路也被堵住 —— 客户类产物不能从建模轨进来。
        let cv = crate::domain::view::CustomerView { objects: vec![], relations: vec![] };
        let mismatch = render(OutletId::D1, Product::CustomerJson(&cv))
            .expect_err("客户类产物不许从建模轨出口发布（换 ID 不能关闸）");
        assert!(
            !mismatch.is_leak(),
            "这一条应当报「产物形态与轨道不相容」，而不是冒充发布闸命中"
        );
        assert!(mismatch.message().contains("不相容"), "{}", mismatch.message());
    }

    /// ⭐ 第 2 轮 · `spec_of` 的轨道必须与登记表一致（同一份事实，两个读者）。
    #[test]
    fn spec_table_and_registry_agree_on_track() {
        for (id, want) in [
            (OutletId::C1, Track::Customer),
            (OutletId::C5, Track::Customer),
            (OutletId::H1, Track::Customer),
            (OutletId::H3, Track::Customer),
            (OutletId::Z1, Track::ErrorPath),
            (OutletId::Z2, Track::Static),
            (OutletId::Z3, Track::Static),
            (OutletId::D1, Track::Modeling),
        ] {
            assert_eq!(spec_of(id).track(), want, "{} 的轨道查表结果不对", id.code());
            let o = outlet(id.code()).unwrap_or_else(|| panic!("登记表里没有 {}", id.code()));
            assert_eq!(o.track(), want, "登记表与 spec_of 对 {} 的轨道说法不一致", id.code());
        }
        // M1–M5 共用**一个** ID（登记表里是五条，逐条对账）。
        assert_eq!(spec_of(OutletId::McpTools).track(), Track::Customer);
        for i in 1..=5 {
            let o = outlet(&format!("M{i}")).expect("登记表里必须有 M1–M5");
            assert_eq!(o.track(), Track::Customer, "M{i} 的轨道必须与 spec_of 一致");
        }
        // C2 复用 C1 的产物与声明（QA N4：C2 没有自己的管线调用点）。
        assert_eq!(spec_of(OutletId::C2).track(), Track::Customer);
        assert_eq!(spec_of(OutletId::C2).format(), Format::Html);
        // 逐节点粒度只有 H3 一条（卡点②的裁定）。
        assert_eq!(spec_of(OutletId::H3).granularity(), Granularity::PerNode);
        assert_eq!(spec_of(OutletId::H2).granularity(), Granularity::Whole);
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
