//! REQ-056 · **提案条目的建模投影（FDE 通道）** —— 纯逻辑，**零 I/O**。
//!
//! 规格 §8.3 拆双轨：**业务轨给客户看、建模轨给 FDE 看**。
//!
//! # 为什么 REQ-055 的 [`crate::domain::model_view`] 解不了这一段
//!
//! REQ-055 补的建模轨投影，收的是**已落库的快照**（`&Snapshot`）。
//! 而 `review show` 发生在 **apply 之前** —— 那一刻 FDE 手里的对象是
//! **提案条目** [`ItemView`]，不是快照。
//!
//! **时序不同 ⇒ 不是同一个对象。** 于是 REQ-055 那份投影在裁决时刻给不出东西，
//! 而这正是规格 §8.3 想消灭的现象：**FDE 在 apply 前只能闭眼点通过**。
//!
//! 本模块从**提案条目**里取建模轨该看的那几栏：类型 `key`、`evidence`、
//! **列映射**、**基数**、**两端类型**，让 FDE 在点「通过」之前看得见。
//!
//! # 与客户视图的分离：靠「类型不同 + 通道不同」，不靠运行时开关
//!
//! 与 [`crate::domain::model_view`] 同一套论证。两个模块**刻意都不写进**
//! `view.rs` —— 那里是建模术语黑名单的所在地，
//! 把「保留建模术语的那一份」放进去会让两者的分离变得不可核对。
//!
//! | | 客户视图 | 建模轨（本模块） |
//! | --- | --- | --- |
//! | 输出类型 | [`crate::domain::view::CustomerView`] | [`ReviewTrackView`] |
//! | 函数 | `view::project` | [`project_review_track`] |
//! | 术语守门 | **过** `view::scan_modeling_terms`（命中即拒绝发布）| **不过** —— 建模术语在这里是**要被裁决的内容** |
//! | 消费方 | 客户 / 判读手册 / HTML | FDE 在 apply 前裁决建模质量 |
//!
//! # 「不过守门」是一条**带条件的豁免**，而 REQ-057 把它的前提判据修对了
//!
//! 「不过守门」如果只写在注释里，任何人删掉建模字段它都照样成立 ——
//! 那就退化成了「这条通道天生可以随便带建模术语出去」。
//!
//! 所以豁免的前提必须**可执行**。REQ-056 给出的版本是：
//! [`audit_fde_channel`] 拿**守门器自己**（`view::scan_modeling_terms`）
//! 扫一遍渲染文本，命中就算前提成立。
//!
//! ## ⚠️ 那个版本恒为真，判据没落在它声称的地方
//!
//! 渲染器对每个条目**无条件**打 `key=`，而 `key` ∈ `MODELING_TERMS`
//! ⇒ `scan_modeling_terms` **必然命中** ⇒ `premise_holds` 恒为 `true`。
//!
//! 于是 `premise_holds` 实际测的是「**提案是否非空**」，
//! 而不是「**建模字段是否真打出来了**」—— `cli.rs` 那句警告永远不会触发。
//!
//! 病根不在判据的算法，在**判据与数据源不是同一个东西**：
//! 建模栏的取值是「payload 取不到就回退或打空」，
//! 而判据问的是「输出里有没有建模术语」——
//! **回退值和空值于是都被判成「有」**。
//!
//! ## 现在的判据：读**结构化溯源**，不读文本
//!
//! [`audit_fde_channel`] 现在收 [`ReviewTrackView`] 而不是 `&str`：
//! `premise_holds` = 「**有某一栏建模字段既来自 payload、又非空**」。
//!
//! 这个问句与「文本里有没有某个词」**没有任何交集**：
//! - 往 [`crate::domain::view`] 的词表里加词（如「客户」）——
//!   客户视图侧**更严**（命中即拒），而 FDE 侧 `premise_holds` **一个 bit 都不变**。
//! - 建模栏退化成空壳 / 回退值 ⇒ **立刻** `false`。
//!
//! REQ-056 原本想让两轨「共用一套词表、判定方向相反」。
//! 实测证明那带来的是**一致性，不是正确性**：**词表越宽、自检越松**。
//! 现在两轨的判据各走各的路，共用的只是数据源 [`ItemView`]。
//!
//! # REQ-058 · 两条尾巴：退化 evidence（F-1）与分界前缀锐边（F-2）
//!
//! REQ-057 把判据换成了「字段溯源」，但**字段本身仍然取自渲染结果** ——
//! 尾巴有两根，都在本模块内：
//!
//! - **F-1**：`evidence` 退化（`""` / `{}` / `null` / `[]` / `0` …）经
//!   [`render_evidence_entry`] 渲染成**非空串** ⇒ `sourced_fields` 判
//!   「有依据」⇒ **空证据冒充复核依据**。现在判据问
//!   [`evidence_line_carries_data`]，退化类别由 [`evidence_degeneracy`]
//!   从**原始 `Value`** 上判出来 —— **保留呈现，不冒充有依据**。
//! - **F-2**：`contains` 认轨 + `trim_start_matches` 剥前缀 ⇒ 展示名或
//!   证据 claim 里含「业务轨：」/「建模轨：」就误报（实测 2 条 / 6 条）。
//!   现在**行首**认轨（[`line_head_prefix_of`]）且**只剥一次**。
//!   ⚠️ **REQ-064**：这个洞**同一个文件里还有第三处** ——
//!   [`verify_provenance_scope`] 的规则①曾仍用 `contains` 认归属前缀，
//!   ⇒ claim 里含「提案级抽取归属：」就被数成一条归属行。
//!   ⚠️ **REQ-068**：还剩**第四、五处** —— 同一条规则的②里两处仍用 `contains`
//!   认**两轨前缀**，⇒ claim / 展示名 / `未通过原因` 里只要含「业务轨：」
//!   就会被当成条目行 / 被误报成「归属行混进了两轨前缀」。
//!   **五处前缀识别现在都是同一个 [`line_head_prefix_of`]** ——
//!   两套实现必然漂移，而「两套前缀识别不一致」正是 F-2 要消灭的那类 bug。
//!
//! # REQ-060 · 提案级抽取归属：**让它被看到**
//!
//! REQ-056 建了两轨，REQ-057 把自检判据换成了读字段溯源。两者都留下同一个洞：
//! **FDE 在 `review show` 里看不到这份提案是哪个 agent 抽的。**
//!
//! ## 为什么现在才安全（顺序是被刻意反过来固化的）
//!
//! REQ-056 的 Dev 说过「猜错归属比不给更危险」。于是 BA 把顺序反过来：
//! **先让归属本身可信（REQ-059，已完成），再让它被看见（本条）**。
//! REQ-059 之后 `extracted_by.agent` 记的是执行者**自报**的身份，
//! 混名在**出产物前**就被拒（`executor_name_collision`）——
//! 所以这里展示的不是「一个可能是假名的字段」。
//!
//! ## 归属是**提案级**的，`ProposalItemRow` 压根没有这一栏
//!
//! [`crate::db::proposal::ProposalRow`] 有 `extracted_by`，
//! [`crate::db::proposal::ProposalItemRow`] **没有**。
//! 于是措辞必须说清这一点，否则 FDE 会以为「每个条目由不同 agent 抽」——
//! 那正是 AC-002 判据要抓的误导。
//!
//! ## ⚠️ 为什么它挂在 [`ReviewTrackView`] 顶层、绝不进 [`ModelFacing`]
//!
//! **这是 AC-004 的结构性保证，不是约定。**
//!
//! [`audit_fde_channel`] 的 `premise_holds` 逐条调用
//! [`ModelFacing::sourced_fields`]，而那个方法的接收者是 `&self = &ModelFacing`。
//! 归属只要**不进** [`ModelFacing`]，判据在**类型层面**就看不见它 ——
//! 于是「把归属塞进建模轨那一行」这件事连编译都不必就能被排除在 `premise_holds` 之外。
//!
//! 反过来，若把它塞进 `ModelFacing`（或让 `sourced_fields` 读它），
//! `premise_holds` 就会因为这个**渲染产物**而变真 ——
//! REQ-057 的「判据不依赖渲染文本」当场作废。
//! [`verify_provenance_scope`] 把这条要求做成**可执行**的判据（它同时抓
//! 「逐条目重复打印」那个误导变体）。
//!
//! # REQ-065 · 退化标签**必须携带原始载荷**（一条不变量）
//!
//! REQ-058 让退化 evidence 渲染成**看得见**的保留标记。QA 复验时自己构造了
//! 15 个形状**逐标签**审，发现「看得见」只做了一半 ——
//!
//! ```text
//! {"table":"","column":"","claim":""}  →  ␀有键无值␀
//! {"foo":"","bar":null}                →  ␀有键无值␀   ← 逐字相同
//! ```
//!
//! 两个**性质完全不同**的提案（前者知道三段式 schema 却没填值，后者发了无关键）
//! 在输出上**完全分不出来** —— 键名被藏起来了。
//! 这与 REQ-058 已修的 `AllDegenerateArray`（`[1,2]` 渲染成 `全退化数组`）
//! 是**同一个毛病的另一头**。
//!
//! ## 不变量（本条唯一要立的东西）
//!
//! > **任何退化标签的渲染结果，都必须携带该元素的原始载荷信息。**
//!
//! 落到代码上是一句更强、也更好核对的话：
//! **标签里的载荷部分只是 `payload` 的函数** ——
//! [`EvidenceDegeneracy`] 的每个变体**只提供类别名**，
//! 「带出什么」由 [`evidence_payload_fingerprint`] 一处决定。
//!
//! ⚠️ REQ-065 据此写下的是一句**有条件**的话：
//! **走 [`EvidenceDegeneracy::label`] 默认路径的新增类别**带得上载荷。
//! **那个前提是「这条路径上没有别的出口」** —— 一旦有人在 `label()` 里
//! 加一条提前 `return`（QA 的 C5），或绕过 `label()` 另开出口（C6），
//! 它就不成立了。**只要出口不止一条，「结构上」三个字就不成立。**
//!
//! ### REQ-070：把出口收到**一处**，再在**消费侧**收口
//!
//! 现在这个前提由生产代码兑现，而不是由注释承诺：
//!
//! - [`seal_evidence_line`] 在 [`payload_evidence`] —— 渲染器的**唯一**调用点 ——
//!   对**每一个**证据元素复核：带定界符 ⇒ 必须带得下载荷指纹；
//!   不带定界符却**一个载荷原子都没露** ⇒ 整行换回忠实呈现；
//! - 它手里同时握着**原始 `Value`** 与**渲染器算好的串**，
//!   所以**在渲染器内部加几条提前 `return` 都绕不过它** ——
//!   分支改得了后者，改不了它随后按前者做的核对。
//!
//! ⇒ **载荷保护不再取决于触发形状是否落在任何一份语料里**（AC-001）。
//!
//! ### ⚠️ 这一条**仍然不是**绝对命题（三格边界，逐条写明）
//!
//! - **封条只查「有没有」，不查「全不全」**：行里留**一个**原子（哪怕只是个键名）
//!   就能绕过规则 ②。「完整呈现」那一层仍由 REQ-065 的语料判据负责，**本条不取代它**；
//! - **封条挂在一条链上**：把渲染器直接接到 `evidence` 上、或删掉封条，会重新打开那个洞。
//!   `tests/test_review_view_req070.rs` 有两条**结构守卫**盯着，**但它们是源码文本探针**：
//!   按 REQ-068 的同一口径，**这是改版探测，不是现役防线** —— 抓不到别名绕过；
//! - **两侧一起改**规则 ② 不会响：既把某形状新判成退化、又让它原样吐出。
//!
//! **REQ-065 那份语料覆盖什么，仍照它自己写的那样**（`tests/test_review_view_req065.rs`
//! 的 `corpus()`：28 个形状 / 20 个退化 / 8 个类别全覆盖，含 8 个**非退化近似形状**）。
//! **它不再是不变量唯一的弹药** —— 语料外的形状由上面那条封条接着。
//!
//! ### 为什么「非空」**不是**这条不变量
//!
//! `␀有键无值␀` 非空 —— 它照样藏了键名。
//! REQ-058 的 B-1 锁（`test_req058_recognisable_shapes_never_render_empty`）测的是
//! **非空**；本条测的是它的**上一层**：**看得见的串里能看见原始载荷**。
//! 两条并存、缺一不可：**非空**挡「静默丢」，**带载荷**挡「藏内容」。
//!
//! ### 它适用于本模块里哪些既有判据
//!
//! | 既有判据 | 与本条的关系 |
//! | --- | --- |
//! | REQ-058 B-1「任何元素都渲染成非空串」 | **并存**（挡「静默丢」）；本条是加强，**不取代**它 |
//! | REQ-058「退化标签仍显示原始内容」（只点了 4 个取值） | 本条把它**泛化**：断言只看**载荷**，不看**类别**（否则下一个人再加一个类别就白加） |
//! | [`evidence_object_body`] 之后的紧凑 JSON 兜底分支 | 同一纪律的**另一个出口**：宁可原样呈现，也不静默丢 |
//! | [`ProposalProvenanceView::from_extracted_by`] 的 `Suspected { reason, raw }` | **同一不变量在归属侧的既有实例**（`raw` 带原始取值）；本条把它写成明文规则 |
//! | [`ModelFacing::type_key`] 的 `KEY_MISSING_MARK`（「让缺失可见」） | 同族的「不许静默」纪律，方向相反（**缺失** vs **退化**） |
//!
//! ### ⚠️ 本条的**边界**（写明是为了不被当成「全查过了」）
//!
//! - **载荷转义不可逆**（payload 字面含 `␁` 与含 `␀` 输出同形）—— REQ-058 的 N-2，
//!   QA 已判**不影响判据正确性**，本条不做；
//! - ~~**payload 换行可注入行结构** —— REQ-069~~ —— **REQ-069 已落地**：
//!   所有取自 payload 的文本在**进投影那一刻**一律走 [`escape_payload_text`]，
//!   LF / CR 被转义成同区的 `␊` / `␍` ⇒ **payload 造不出行结构**。
//!   ⚠️ 那条边界里「若有人另开一条不经 JSON 编码的出口」的担心仍然成立，
//!   但**换行这一格已不再依赖 JSON 编码** —— 它由 [`escape_payload_text`] 独立挡住。
//!   ⇒ 「挡住了」说的是**当前这些出口**，不是「任何写法都挡得住」。
//!
//! # 本模块**不**做什么
//!
//! - **不碰抽取逻辑** —— 产物 sha 应当不变；
//! - **不碰两轨分派** —— `review.rs` 的 `summarize` / `apply_decision` 一行不改；
//! - **不引入任何 I/O** —— 领域层零 I/O（AC-002）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::review::{summarize, Decision, EntryState, ItemKind, ItemView, ProposalStatus};
// REQ-057：本模块**不再** `use view::scan_modeling_terms` ——
// 判据与词表解耦是这个需求的核心，见 [`audit_fde_channel`]。

// ──────────────────────────────────────────────────────────
// 投影
// ──────────────────────────────────────────────────────────

/// 建模轨投影 —— **`review show` 里 FDE 逐条裁决建模质量时看的那一份**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewTrackView {
    pub items: Vec<ReviewItemView>,
    /// 整体是否通过（可进入 apply）—— 与 `review show` 原有第一行同源。
    pub passed: bool,
    pub blocked: Vec<BlockedItem>,
    pub dangling_relations: Vec<String>,
    /// **提案级**抽取归属（REQ-060）。
    ///
    /// # ⚠️ 它**刻意不在** [`ReviewItemView`] / [`ModelFacing`] 里
    ///
    /// `extracted_by` 只存在于 [`crate::db::proposal::ProposalRow`]，
    /// [`crate::db::proposal::ProposalItemRow`] **没有**这一栏 ——
    /// **归属天生就是提案级的**，挂成条目级会让 FDE 误以为
    /// 「每个条目由不同 agent 抽」。
    ///
    /// 而 [`ModelFacing::sourced_fields`] 的接收者是 `&ModelFacing`，
    /// 归属不进它 ⇒ [`audit_fde_channel`] 的 `premise_holds`
    /// 在**类型层面**就与它无关。**这是 AC-004 的结构性保证**，
    /// 不是「记得别加进去」的约定。
    #[serde(default)]
    pub provenance: ProposalProvenanceView,
}

// ──────────────────────────────────────────────────────────
// 提案级抽取归属（REQ-060）
// ──────────────────────────────────────────────────────────

/// 提案级抽取归属在 FDE 通道里的**四态**呈现。
///
/// # 为什么不是 `Option<String>`
///
/// 「有归属」与「没有归属」之外还有第三种：**有值但形态可疑**
/// （空串 / 纯空白 / 缺 `agent` / `agent` 不是非空字符串）。
/// 用 `Option` 表达它就只剩 `Some("")` —— 而**空串与真名长得一样**，
/// 渲染出来 FDE 分不出「这个 agent 叫空串」和「这里本该有归属却没写」。
///
/// REQ-057 刚证过「判据恒真」这一类问题（`premise_holds` 恒 `true`）。
/// 本条**不许**写一个「有就打、没有就打空」然后自称安全 ——
/// 所以可疑必须是一个**独立且显眼**的状态。
///
/// 第四态 [`ProvenanceState::NotProvided`]（调用方压根没提供）同样不许省：
/// 它与 [`ProvenanceState::HumanRevised`] 对 FDE 的含义**相反**
/// （见该变体文档），合并 = 给出假安心。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProvenanceState {
    /// 有可用的归属声明。
    Declared {
        /// 抽取者名。
        agent: String,
        /// 具体版本（可能缺失/可疑 ⇒ 见 [`ProvenanceState::Suspected`]）。
        version: Option<String>,
        /// 抽取轮次。
        round: Option<i64>,
        /// 本轮失败的分组 —— 非空意味着**这一轮没有全成功**。
        failed_groups: Vec<String>,
    },
    /// `extracted_by` 为 `None` = **人工修订**（ADR-001）。
    ///
    /// ⚠️ 刻意**不叫** `None` / `Absent`：`None` 这个词在渲染里
    /// 既可能是「人工修订」，也可能是「没接上」，分不开。
    HumanRevised,
    /// 调用方**没有提供**这一栏 —— 投影入口没收到 `extracted_by`。
    ///
    /// # 为什么必须与 [`ProvenanceState::HumanRevised`] 分开
    ///
    /// 「库里是 NULL（人工修订）」与「这条路径压根没接归属」
    /// 是**两件不同的事**，对 FDE 的含义也相反：
    /// 前者可以据此**放心**（这一份是人审过的），
    /// 后者只能**保持警惕**（我不知道有没有人审过）。
    /// 混成一个状态 ⇒ 没接通的路径会显示成「人工修订」，
    /// FDE 由此获得一个**假的安心** —— 比不给更危险。
    NotProvided,
    /// 有值但**形态可疑** —— 显式标注，不照打。
    ///
    /// ⚠️ 刻意**不归入** [`ProvenanceState::HumanRevised`]：
    /// 「写了但写得不对」与「压根没写」对 FDE 是两件不同的事。
    Suspected {
        /// 可疑的**具体原因**（不是一句「可疑」）。
        reason: String,
        /// 原始取值（截断后）—— 让 FDE 看得见「库里到底存了什么」。
        raw: String,
    },
}

/// 提案级抽取归属的**展示投影**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalProvenanceView {
    pub state: ProvenanceState,
}

/// 反序列化缺这一栏时的兜底 —— 落 [`ProvenanceState::NotProvided`]。
///
/// ⚠️ **不落** `HumanRevised`：一份没带归属字段的存档**没有**说
/// 「这一份是人审过的」，那样默认会给 FDE 一个**假安心**。
impl Default for ProposalProvenanceView {
    fn default() -> Self {
        Self::not_provided()
    }
}

impl ProposalProvenanceView {
    /// 调用方**没提供**归属时的那一份（老的两参入口）。
    ///
    /// 与 [`ProvenanceState::HumanRevised`] 分开 —— 见该变体文档。
    pub fn not_provided() -> Self {
        Self { state: ProvenanceState::NotProvided }
    }

    /// 从提案行的 `extracted_by`（JSONB，可空）投影。
    ///
    /// `None` = **SQL NULL** = 人工修订（ADR-001）——
    /// 不是「没提供」，后者是 [`Self::not_provided`]。
    ///
    /// # 判定顺序很要紧：先看**形态**，再看**有没有**
    ///
    /// - `None` ⇒ [`ProvenanceState::HumanRevised`]（ADR-001 的人工修订）；
    /// - 非 `None` 但形态可疑 ⇒ [`ProvenanceState::Suspected`]（**不是** HumanRevised
    ///   —— 「写了但写得不对」与「压根没写」对 FDE 是两件不同的事）；
    /// - 其余 ⇒ [`ProvenanceState::Declared`]。
    pub fn from_extracted_by(extracted_by: Option<&Value>) -> Self {
        let Some(v) = extracted_by else {
            return Self { state: ProvenanceState::HumanRevised };
        };

        // ⚠️ 显式 null（`"extracted_by": null` 存成 JSON null）≠ SQL NULL ——
        // 它是**写进来了但没有值**，形态可疑，不许混进「人工修订」。
        if v.is_null() {
            return Self {
                state: ProvenanceState::Suspected {
                    reason: "extracted_by 是 JSON null（不是人工修订的 SQL NULL）".to_string(),
                    raw: "null".to_string(),
                },
            };
        }

        let Some(obj) = v.as_object() else {
            return Self {
                state: ProvenanceState::Suspected {
                    reason: format!("extracted_by 不是对象（实际是 {}）", type_name(v)),
                    raw: truncate(&v.to_string()),
                },
            };
        };

        // 缺 `agent` —— 归属的**最小必备字段**。
        let agent_raw = match obj.get("agent") {
            None => {
                return Self {
                    state: ProvenanceState::Suspected {
                        reason: "缺 `agent` 栏（没有它就无法回答「谁抽的」）".to_string(),
                        raw: truncate(&v.to_string()),
                    },
                }
            }
            Some(a) => a,
        };
        let agent = match agent_raw.as_str() {
            None => {
                return Self {
                    state: ProvenanceState::Suspected {
                        reason: format!("`agent` 不是字符串（实际是 {}）", type_name(agent_raw)),
                        raw: truncate(&v.to_string()),
                    },
                }
            }
            Some(s) => s.trim(),
        };
        if agent.is_empty() {
            return Self {
                state: ProvenanceState::Suspected {
                    reason: if agent_raw.as_str().is_some_and(|s| !s.is_empty()) {
                        "`agent` 只有空白".to_string()
                    } else {
                        "`agent` 是空串".to_string()
                    },
                    raw: truncate(&v.to_string()),
                },
            };
        }

        // 走到这里 `agent` 已经是**非空、非空白**的真名 ⇒ `Declared`。
        // 下面几个栏**不参与可信度判定** —— 缺 `version` 只说明版本不详，
        // 而 `agent` 缺失才是「回答不了谁抽的」。二者不该混成一种可疑。
        let version = obj
            .get("version")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            // ⚠️ REQ-069：转义放在 trim / 空值判定**之后**（同 [`payload_key`] 的理由）。
            .map(escape_payload_text);
        let round = obj.get("round").and_then(|x| x.as_i64());
        let failed_groups = obj
            .get("failed_groups")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().map(render_group_ref).collect())
            .unwrap_or_default();

        Self {
            state: ProvenanceState::Declared {
                // ⚠️ REQ-069：`agent` 是提案方声明的任意字符串，而它被打在
                // **归属行**上 —— 换行能凭空造出一整行归属/结构行（实测）。
                // 上面那些空值/类型判定都已在**转义之前**跑过 ⇒ 判定输入不变。
                agent: escape_payload_text(agent),
                version,
                round,
                failed_groups,
            },
        }
    }
}

/// JSON 取值的类型名 —— 用来在可疑提示里说清「它到底是什么」。
fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "布尔",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

/// 单个失败分组的渲染 —— 与 `render_evidence_entry` 同一套「不静默丢」的规矩。
fn render_group_ref(v: &Value) -> String {
    // ⚠️ REQ-069：分组名取自 payload，且拼在**归属行**上 ⇒ 同样走转义出口。
    v.as_str().map(escape_payload_text).unwrap_or_else(|| truncate(&v.to_string()))
}

/// 原始取值截断 —— 可疑提示里要给出证据，但不能把整段 JSON 糊到人脸上。
const RAW_MAX: usize = 120;

fn truncate(s: &str) -> String {
    if s.chars().count() <= RAW_MAX {
        return s.to_string();
    }
    let head: String = s.chars().take(RAW_MAX).collect();
    format!("{head}…（已截断，原长 {} 字符）", s.chars().count())
}

/// 被依赖关系拦下的条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockedItem {
    pub target: String,
    pub reason: Option<String>,
}

/// 一个提案条目在**两轨**上分别该看的东西。
///
/// # 为什么拆成两个子结构而不是一坨字段（AC-003）
///
/// 建模轨的字段**天生就会出现在同一行里**：一个关系的 `key`、两端、基数
/// 本来就是同一件事的三个面。若平铺成单一结构，「哪些栏属于建模轨」
/// 只能靠文档说明 —— 而文档会被改烂。
///
/// 拆成 [`BizFacing`] / [`ModelFacing`] 之后，
/// **业务轨那一侧在类型上就没有建模字段可放**，
/// 渲染时也自然分成两行 —— 判据因此是可执行的，不靠约定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewItemView {
    /// 提案内序号（`--item N` 指的就是它）。
    pub seq: i32,
    pub kind: ItemKind,
    /// 业务轨 + 建模轨共同作用出来的**整体**判定（两轨都通过才算通过）。
    pub state: EntryState,
    /// 被拒的原因（仅 `Vetoed` 时有值）。
    pub reason: Option<String>,
    /// **业务轨该看的** —— 客户视角的呈现。
    pub biz: BizFacing,
    /// **建模轨该看的** —— FDE 裁决建模质量的依据。
    pub model: ModelFacing,
}

/// 业务轨该看的：条目对外的**那个标识**，以及业务轨裁决。
///
/// ⚠️ 这里**刻意没有** `key` / `evidence` / `cardinality` / 列名 ——
/// 客户视图逐条注明丢弃的正是这些；FDE 要的也正是这些。
/// 两者不混，是同一个动作的两面。
///
/// # ⚠️ `display_target` 是**提案方放进去的那个字符串**，本模块不替它改名
///
/// 字段名里的 `display_` 指的是**真实抽取路径**的取值：
/// `agent::assemble` 把 `target` 设成 `t.display_name`（实体）
/// 与「人看的标签」（关系），提案存储层原样存进 `target_key` 列。
///
/// ⚠️ 但 [`ItemView::target`] 自己的文档写的是「条目描述的类型 **key**」，
/// 而**人工修订的提案**（ADR-001）完全可以放别的东西。
/// 所以本字段**只做透传，不做解释** —— 它显示提案方写的那一栏，
/// 不声称它一定是展示名，也**不拿它去当 key 用**
/// （key 走 [`ModelFacing::type_key`]，那份从 payload 取）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BizFacing {
    /// 提案方写在条目 `target` 上的那一栏（真实抽取路径 = 展示名）。
    pub display_target: String,
    pub status: Decision,
}

/// 建模轨该看的：系统标识、列映射、证据、基数、两端类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFacing {
    /// **系统标识**（payload 的 `key`）—— 客户视图刻意丢弃这一栏；
    /// FDE 判断「两条提议是不是同一个东西」靠它。
    ///
    /// # ⚠️ REQ-057（F-D）：`None` 表示**payload 里没有 key**，不回退展示名
    ///
    /// REQ-056 这里写的是 `it.type_key()`，而 [`crate::domain::review::ItemView::type_key`]
    /// 在 payload 缺 `key` 时**回退到 `target`**（展示名）。
    /// 于是 FDE 看到的 `key=主键服务中心` —— 一个**展示名被当成了 key**，
    /// 而且看起来毫无异样。
    ///
    /// `review.rs` 是红线文件，**不许改**，所以修法落在这里：
    /// 本栏改为**只认 payload**，`None` 就是「没有 key」。
    /// 渲染时打 `key=（无 key）`（见 [`KEY_MISSING_MARK`]）——
    /// **让缺失可见**，而不是拿一个假的顶上。
    #[serde(default)]
    pub type_key: Option<String>,
    /// **列映射**：属性展示名 ← 源库列名。
    pub columns: Vec<ModelColumn>,
    /// **证据**（溯源）—— 客户视图明确丢弃，FDE 恰恰要靠它复核。
    pub evidence: Vec<String>,
    /// **基数**（关系类才有）。
    pub cardinality: Option<String>,
    /// **两端类型 key**（关系类才有）—— 注意是 key，不是展示名。
    pub from_type: Option<String>,
    pub to_type: Option<String>,
    pub status: Decision,
}

impl ModelFacing {
    /// 本条目里**确实来自 payload、且非空**的建模栏。
    ///
    /// # 这就是 REQ-057 换掉的那条判据
    ///
    /// REQ-056 的 `premise_holds` 问的是「渲染文本里有没有建模术语」。
    /// 本方法问的是另一件事：**哪几栏真的带回了数据**。
    ///
    /// 两个问句与 [`crate::domain::view`] 的词表**没有任何交集** ——
    /// 往词表里加词不会让这个列表多出一项，也不会少一项。
    ///
    /// 语义：
    /// - `key` 只在 `payload.key` **非空字符串**时算（展示名回退已被 F-D 去掉）；
    /// - `columns` 只要有一项 `name` 非空就算（`properties` 来自 payload）；
    /// - `evidence` 同理（`evidence` 来自 payload），但**逐条**问
    ///   「这一条带不带可复核的依据」—— 见 [`evidence_line_carries_data`]；
    /// - `cardinality` 非空；
    /// - `from_to` 两端都在且都非空（**单端不算** —— 那是残缺声明，不是数据）。
    ///
    /// # ⚠️ REQ-058：`evidence` 这一栏不再问「渲染出来的串非不非空」（F-1）
    ///
    /// 退化值经 [`render_evidence_entry`] 渲染出来**都是非空串**
    /// （`""` 渲染成两个字符 `""`、`{}` 渲染成 `{}`、`null` 渲染成 `null` …）
    /// ⇒ 判 `!e.trim().is_empty()` 等于把**空证据**判成「有依据」，
    /// 而输出里看得见 `证据：{}` —— **看得见的退化骗过了自检**。
    ///
    /// 现在这一栏问的是 [`evidence_line_carries_data`]：
    /// 那是个**保留定界符**的判定，与人类措辞无关，
    /// 判据因此不再随渲染措辞变松变紧。
    pub fn sourced_fields(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.type_key.as_deref().is_some_and(|k| !k.trim().is_empty()) {
            out.push("key");
        }
        if self.columns.iter().any(|c| !c.name.trim().is_empty()) {
            out.push("columns");
        }
        if self.evidence.iter().any(|e| evidence_line_carries_data(e)) {
            out.push("evidence");
        }
        if self.cardinality.as_deref().is_some_and(|c| !c.trim().is_empty()) {
            out.push("cardinality");
        }
        if self.from_type.as_deref().is_some_and(|f| !f.trim().is_empty())
            && self.to_type.as_deref().is_some_and(|t| !t.trim().is_empty())
        {
            out.push("from_to");
        }
        out
    }

    /// 与 [`Self::sourced_fields`] 一一对应的**渲染标记** ——
    /// [`verify_track_separation`] 用它核对「这些栏真的打在那行上了」。
    ///
    /// 刻意做成**子串**，不重排整行：整行重排会与 [`render_review_text`]
    /// 共用同一份格式逻辑，渲染器一改，两边一起错，判据就跟着失效。
    pub fn sourced_markers(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(k) = self.type_key.as_deref().filter(|k| !k.trim().is_empty()) {
            out.push(format!("key={k}"));
        }
        if self.columns.iter().any(|c| !c.name.trim().is_empty()) {
            out.push("列映射：".to_string());
        }
        if self.evidence.iter().any(|e| evidence_line_carries_data(e)) {
            out.push("证据：".to_string());
        }
        if let Some(c) = self.cardinality.as_deref().filter(|c| !c.trim().is_empty()) {
            out.push(format!("基数：{c}"));
        }
        if let (Some(f), Some(t)) = (
            self.from_type.as_deref().filter(|f| !f.trim().is_empty()),
            self.to_type.as_deref().filter(|t| !t.trim().is_empty()),
        ) {
            out.push(format!("两端：{f}→{t}"));
        }
        out
    }
}

/// 一条列映射。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelColumn {
    /// 属性展示名（业务轨那一侧的措辞）。
    pub name: String,
    /// 源库列名（建模轨那一侧的溯源）。
    pub column: String,
}

/// 从**提案条目**投影出建模轨视图。
///
/// ⚠️ **不改判定**：整体是否通过、逐条状态、被拦、悬空关系全部取自
/// 调用方传入的 [`ProposalStatus`]（`review.rs::summarize` 的产物）。
/// 本函数**只做呈现**，不重算、不覆盖任何裁决结果。
///
/// # `extracted_by` 是**提案级**，不是条目级
///
/// REQ-060：归属只存在于 `ProposalRow`，`ProposalItemRow` 没有这一栏。
/// 所以它**只出现一次**，在 [`ReviewTrackView`] 顶层 ——
/// 绝不逐条目重复（那会让 FDE 以为每个条目由不同 agent 抽）。
///
/// 本函数**没收到**归属，于是投影成 [`ProvenanceState::NotProvided`]——
/// **不是**「人工修订」。两者对 FDE 的含义相反，混起来会给出假安心。
pub fn project_review_track(items: &[ItemView], status: &ProposalStatus) -> ReviewTrackView {
    project_review_inner(items, status, ProposalProvenanceView::not_provided())
}

/// [`project_review_track`] + 提案级归属（REQ-060）。
///
/// `extracted_by` 收**提案行**上那个可空 JSONB
/// （[`crate::db::proposal::ProposalRow::extracted_by`]）：
/// `None` = **SQL NULL** = 人工修订（ADR-001）。
///
/// # 为什么归属挂在这里而**不挂**在 `ModelFacing` 上
///
/// [`ModelFacing::sourced_fields`] 的接收者是 `&ModelFacing`，
/// 而 [`audit_fde_channel`] 只逐条读它。归属一旦不进 `ModelFacing`，
/// `premise_holds` 在**类型层面**就与它无关 ——
/// AC-004 要的「多打归属不许让 `premise_holds` 变真」由此成为**结构性质**，
/// 而不是「记得别加进 `sourced_fields`」的约定。
pub fn project_review_track_with_provenance(
    items: &[ItemView],
    status: &ProposalStatus,
    extracted_by: Option<&Value>,
) -> ReviewTrackView {
    project_review_inner(items, status, ProposalProvenanceView::from_extracted_by(extracted_by))
}

/// 两个公开入口的**唯一**实现。
///
/// ⚠️ 归属由调用方**算好后**整个传进来，而不是在这里从 `Option` 推 ——
/// 因为「SQL NULL（人工修订）」与「没提供」两种 `None` 语义**必须**由
/// 调用点自己声明，推不出来。
fn project_review_inner(
    items: &[ItemView],
    status: &ProposalStatus,
    provenance: ProposalProvenanceView,
) -> ReviewTrackView {
    ReviewTrackView {
        // REQ-060：提案级，**只此一份**。
        provenance,
        passed: status.passed,
        blocked: status
            .blocked
            .iter()
            .map(|b| BlockedItem {
                // ⚠️ REQ-069：`target` 与 `reason` 都源自提案方写的字符串
                // （`review.rs` 的 `reason` 里嵌着 `target`）⇒ 同样走转义出口。
                target: escape_payload_text(&b.target),
                reason: b.reason.as_deref().map(escape_payload_text),
            })
            .collect(),
        dangling_relations: status
            .dangling_relations
            .iter()
            .map(|d| escape_payload_text(d))
            .collect(),
        items: items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                // 与 `summarize` 同序产出（同一个 `items` 一次 map），
                // 按下标配对；取不到就落 Pending，**不猜**。
                let v = status.verdicts.get(i);
                ReviewItemView {
                    seq: i as i32 + 1,
                    kind: it.kind,
                    state: v.map(|v| v.state).unwrap_or(EntryState::Pending),
                    reason: v.and_then(|v| v.reason.clone()).as_deref().map(escape_payload_text),
                    biz: BizFacing {
                        // REQ-069：`target` 是提案方原样写进来的字符串
                        // （人工修订的提案可以放任意内容）⇒ 同样走转义出口。
                        display_target: escape_payload_text(&it.target),
                        status: it.biz_status,
                    },
                    model: ModelFacing {
                        // REQ-057（F-D）：**只认 payload.key**，不回退展示名。
                        type_key: payload_key(&it.payload),
                        columns: payload_columns(&it.payload),
                        evidence: payload_evidence(&it.payload),
                        cardinality: payload_str(&it.payload, "cardinality"),
                        from_type: it
                            .from_type
                            .as_deref()
                            .map(escape_payload_text)
                            .or_else(|| payload_str(&it.payload, "from_type")),
                        to_type: it
                            .to_type
                            .as_deref()
                            .map(escape_payload_text)
                            .or_else(|| payload_str(&it.payload, "to_type")),
                        status: it.model_status,
                    },
                }
            })
            .collect(),
    }
}

/// 便捷入口：条目 + 基线类型 → （建模轨投影, 汇总）。
///
/// `baseline_types` 是**基线快照里已有的类型 key** —— 只有存储层知道快照里有什么，
/// 因此领域层不自己读库，由调用方传入。
pub fn project_review_track_with_summary(
    items: &[ItemView],
    baseline_types: &std::collections::BTreeSet<String>,
) -> (ReviewTrackView, ProposalStatus) {
    let status = summarize(items, baseline_types);
    let view = project_review_track(items, &status);
    (view, status)
}

/// [`project_review_track_with_summary`] + 提案级归属（REQ-060）。
pub fn project_review_track_with_summary_and_provenance(
    items: &[ItemView],
    baseline_types: &std::collections::BTreeSet<String>,
    extracted_by: Option<&Value>,
) -> (ReviewTrackView, ProposalStatus) {
    let status = summarize(items, baseline_types);
    let view = project_review_track_with_provenance(items, &status, extracted_by);
    (view, status)
}

// ──────────────────────────────────────────────────────────
// 判据 ①：FDE 通道的豁免前提 —— 「到底有没有真建模数据」（AC-001）
// ──────────────────────────────────────────────────────────

/// FDE 通道自检的结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FdeChannelAudit {
    /// 豁免前提是否成立 —— 「这份投影里**确实**带回了建模数据」。
    pub premise_holds: bool,
    /// 哪些条目的哪些建模栏**确实来自 payload 且非空** —— 形如
    /// `item[1].key` / `item[1].evidence`。
    ///
    /// ⚠️ REQ-056 这一栏是**守门器命中的建模术语**。REQ-057 换掉了：
    /// 那一栏与 [`crate::domain::view`] 的词表耦合，
    /// 而**词表越宽、自检越松**（加个词就能让豁免更容易成立）。
    /// 现在它列的是**字段溯源** —— 加词不会让它多一项。
    ///
    /// 顺带的好处：条目缺哪一栏**直接看得见**
    /// （`item[2].evidence` 在、而 `item[2].key` 不在 ⇒ key 缺失）。
    pub hits: Vec<String>,
}

/// **「FDE 通道不过建模术语守门」这条豁免的执行判据。**
///
/// # 问的是「字段有没有打出来」，不是「文本里有没有某个词」
///
/// `premise_holds` = 「**有某一栏建模字段既来自 payload、又非空**」
/// （[`ModelFacing::sourced_fields`]）。
///
/// ## 为什么不能问「文本里有没有建模术语」
///
/// 那是 REQ-056 的做法，**恒为真**：渲染器对每个条目无条件打 `key=`，
/// 而 `key` ∈ `MODELING_TERMS` ⇒ 必然命中 ⇒ 前提对任何非空提案都成立。
/// 那样 `premise_holds` 实际测的是「提案是否非空」。
///
/// 更要命的是它和词表**反向耦合**：词表越宽 ⇒ 越容易命中 ⇒ 自检越松；
/// 而客户视图那一侧同一个词表是「越宽越严」。
/// **同一份词表把两个方向拧反了。** 现在判据不碰词表，耦合被切断。
///
/// # 为什么不能靠 `review.rs` 的 `type_key()`
///
/// REQ-056 直接用它，于是展示名冒充 key（F-D）。
/// 本判据读的是 [`ModelFacing`] 自己的溯源，**不再经过那个回退**。
///
/// # 与判据 ② 的分工
///
/// - 本函数：「这份投影里到底有没有建模数据」—— 建模栏**整体**退化时报；
/// - [`verify_track_separation`]：「两轨渲染有没有串轨 / 重复 / 漏打」——
///   投影**有**数据但渲染错了时由它报。
///
/// 两者数据源相同、问句不同，不会互相顶掉。
pub fn audit_fde_channel(view: &ReviewTrackView) -> FdeChannelAudit {
    let mut hits = Vec::new();
    for it in &view.items {
        for f in it.model.sourced_fields() {
            hits.push(format!("item[{}].{f}", it.seq));
        }
    }
    FdeChannelAudit { premise_holds: !hits.is_empty(), hits }
}

// ──────────────────────────────────────────────────────────
// 判据 ②：两轨分界（BYPASS_A / BYPASS_C 的判据，AC-003）
// ──────────────────────────────────────────────────────────

/// 核对**渲染结果**与**投影结构**是否一致 —— 返回全部违规（空 vec = 通过）。
///
/// # 为什么要有第二条判据
///
/// [`audit_fde_channel`] 只回答「有没有数据」。
/// 但投影**有**数据、渲染却**串轨 / 重复 / 漏打**时，它一声不吭。
/// QA 的 BYPASS_A（建模字段塞进业务轨行）与
/// BYPASS_C（两轨内容重复）正是这一类。
///
/// # 四条规则（逐条独立可抓）
///
/// ① **前缀认「行首」，不认「整行包含」** —— 一行属于哪一轨由
///    [`line_head_prefix_of`] 判：去掉缩进后**行首**是哪个前缀就是哪一轨；
///    每个条目两轨**各占一行、按序配对**（行数对得上）。
///    抓：两轨混成一行（混进来那轨的行没了 ⇒ 行数对不上）。
///    ⚠️ **REQ-058 删掉了原来那条「任一行不得同时带两个前缀」** ——
///    它与②**对同一行可以给出相反的判决**，而展示名与证据 claim 都是
///    **任意的 payload 文本**：展示名含「建模轨：」时②说「这一行完全正确」、
///    ①说「两轨混成了一行」。①因此**只可能误报**。
///    行首识别之后，两轨真的混成一行时**仍被①的行数抓住**、
///    被②的逐字比对抓住（见本文件的回归测试）。
/// ② **业务轨行只许有它自己的两栏** —— **只剥一次**前缀后，
///    必须**恰好**是 `[display_target, "biz=<status>"]`。
///    抓：BYPASS_A。**不依赖任何词表 / marker 列表** ——
///    这是「token 数量与取值都必须对得上」，不是「有没有某个词」。
///    ⚠️ **REQ-058**：这里原先是 `trim_start_matches`，它会**连续剥重复前缀**
///    （`"业务轨：业务轨：客户"` → `"客户"`），而展示名本身就可以是
///    「业务轨：客户」。现在用 [`str::strip_prefix`]，**只剥一次**。
/// ③ **建模轨行不得是业务轨行的复制** —— 不得与业务轨行逐字相同。抓：BYPASS_C。
/// ④ **payload 来源的栏必须真的打在建模轨行上** —— 每个
///    [`ModelFacing::sourced_markers`] 都要能在该行里找到。
///    抓：渲染漏打 / 漏打某一栏。
pub fn verify_track_separation(view: &ReviewTrackView, rendered: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let lines: Vec<&str> = rendered.lines().collect();

    let biz_lines: Vec<&str> = lines
        .iter()
        .filter(|l| line_head_prefix_of(l) == Some(BIZ_LINE_PREFIX))
        .copied()
        .collect();
    let model_lines: Vec<&str> = lines
        .iter()
        .filter(|l| line_head_prefix_of(l) == Some(MODEL_LINE_PREFIX))
        .copied()
        .collect();

    // ① 行数对得上：一个条目一行，两轨不共行。
    if biz_lines.len() != view.items.len() {
        violations.push(format!(
            "业务轨行数 {} ≠ 条目数 {}（两轨可能被混成了一行）",
            biz_lines.len(),
            view.items.len()
        ));
    }
    if model_lines.len() != view.items.len() {
        violations.push(format!(
            "建模轨行数 {} ≠ 条目数 {}（两轨可能被混成了一行）",
            model_lines.len(),
            view.items.len()
        ));
    }

    for ((it, biz_line), model_line) in
        view.items.iter().zip(biz_lines.iter()).zip(model_lines.iter())
    {
        let (it, biz_line, model_line) = (it, *biz_line, *model_line);

        // ② 业务轨行只许有它自己的两栏。
        //
        // ⚠️ REQ-058（AC-004）：`strip_prefix` **只剥一次**。
        // 原先的 `trim_start_matches` 会**连续剥重复前缀** ——
        // 展示名「业务轨：客户」渲染出的行是 `业务轨：业务轨：客户 · biz=…`，
        // 剥两次之后得到 `客户 · biz=…`，与期望的
        // `业务轨：客户 · biz=…` 对不上 ⇒ **误报**。
        //
        // 剥不到前缀时退回整行 —— 那会让本条**照常报违规**，而不是静默放过。
        let biz_head = biz_line.trim_start();
        let biz_body = biz_head.strip_prefix(BIZ_LINE_PREFIX).unwrap_or(biz_head);
        let want = format!("{} · biz={}", it.biz.display_target, it.biz.status.as_str());
        if biz_body != want {
            violations.push(format!(
                "条目 {} 的业务轨行多了/少了东西（建模栏串进业务轨？）：期望 {want:?}，实际 {biz_body:?}",
                it.seq
            ));
        }

        // ③ 建模轨行不得是业务轨行的复制。
        if model_line == biz_line {
            violations.push(format!("条目 {} 的建模轨行与业务轨行逐字相同（两轨内容重复）", it.seq));
        }

        // ④ payload 来源的栏必须真的打在那行上。
        for m in it.model.sourced_markers() {
            if !model_line.contains(&m) {
                violations.push(format!(
                    "条目 {} 的建模栏 {m:?} 来自 payload 却没打在建模轨行上\n该行：{model_line}",
                    it.seq
                ));
            }
        }
    }

    violations
}

/// 一行的**行首**（去掉缩进后）是哪个已知前缀 —— 本模块**唯一**的前缀识别处。
///
/// # 为什么不能 `contains`
///
/// 展示名（[`BizFacing::display_target`]）与证据 claim 都是
/// **提案方原样放进 payload 的任意文本**。用 `contains` 认轨时，
/// 展示名含「建模轨：」会把**业务轨那一行**算成建模轨行、
/// claim 含「业务轨：」会把**建模轨那一行**算成业务轨行 ——
///
/// - 计数对不上（行数 ≠ 条目数）；
/// - 按序配对整体错位，后续每一条规则都在**核对的不是它该核对的那一行**。
///
/// 实测（REQ-058）：证据 claim 含「业务轨：」⇒ **2 条**误报；
/// 展示名含「建模轨：」⇒ **6 条**误报（payload 带列映射与证据时）。
///
/// # 行首为什么是对的
///
/// 渲染器把缩进与前缀打在**行首**，且前缀后面紧跟的就是本轨的内容 ——
/// 「行首是哪个前缀」是渲染格式**承诺**的性质；
/// 而「整行里有没有出现过这几个词」不是。
///
/// # 为什么归属前缀也在**这一个**函数里（REQ-064 · AC-001）
///
/// [`verify_provenance_scope`] 的规则①曾自己用 `contains` 认归属前缀 ——
/// 那是 F-2 在**同一个文件里的第三处**残留，于是
/// 「claim 里含『提案级抽取归属：』」被误判成一条归属行。
///
/// # REQ-068：规则②的**两处**也归到这里（AC-004 的复发守卫）
///
/// 规则②里还剩两处拿 `contains` 认**两轨前缀**（同一族病的第四、五处），
/// 现在一并改走本函数 ⇒ **文件里再没有第二处用 `contains` 认这三类前缀的实现**
/// （规则④ 分支 (a) 那处 `contains` 是**例外且被论证过的**：
/// 它只在 `prov_lines` 为空时跑，而那时规则①**已经**报了违规，
/// 它**只可能给已经失败的判决补一条更准的理由**，永远不会把「通过」翻成「违规」）。
/// 这条不变量的**自动检测**在 `tests/test_review_view_req068.rs`
/// （⚠️ 那是**源码文本探针**，只验文本、不验语义，效力边界写在那个文件的模块注释里）。
/// ⚠️ 它的**语义兜底只覆盖 `:1039`**：对 `:1046` 完全无感 ——
/// 真实渲染器把归属行固定打在最前，`:1046` 的见证样本（归属行**之前**有含前缀的非条目行）
/// 在真实渲染里**根本不存在**，所以别拿「兜底也抓不到」推出「整套拦不住」：
/// 别名绕过**整套**是抓得住的，只是 `:1046` 那侧**只押在一条构造样本测试上**。
///
/// 现在**不再有第二套前缀识别**：五处调用点共用本函数。
/// 另写一套的代价不是「多几行代码」，而是**两套实现必然漂移**，
/// 而「两套前缀识别不一致」正是 F-2 要消灭的那类 bug。
///
/// ⚠️ 归属**不是一轨**（见 [`PROVENANCE_LINE_PREFIX`]）：它被认出来是为了
/// 让归属判据**只数真正的归属行**，而 [`verify_track_separation`] 比对的是
/// [`BIZ_LINE_PREFIX`] / [`MODEL_LINE_PREFIX`] 两个常量 ⇒ 归属行两边都不落，
/// 分界判据的判决**逐条不变**。
///
/// ⚠️ 三个前缀**两两不互为前缀**（`业务轨：` / `建模轨：` / `提案级抽取归属：`），
/// 所以这个 if 链的**先后顺序不影响结果** ——
/// 这条不变量由 `tests/test_review_view_req064.rs` 钉住，
/// 否则有人调整顺序时会无声地改变判决。
///
/// # REQ-073 · 为什么它是 `#[doc(hidden)] pub`
///
/// **只为让 `tests/` 里的判据与本函数共用同一份实现**。
/// `tests/*.rs` 是集成测试，与本 crate **分开编译**，**看不到 `pub(crate)`**
/// —— 所以「提成 `pub(crate)` 就能共用」是**错的**（QA 在 REQ-073 验收里判过）。
///
/// ⚠️ 它的**唯一**消费者是 `tests/test_review_view_req068.rs`
/// 与 `tests/test_review_view_req064.rs` 里的**判据复刻**。
/// **它不是**对外 API 的一部分，所以 `#[doc(hidden)]` ——
/// 公开符号表里不列它，但它**确实可被外部 crate 调用**。
///
/// # REQ-073 消除了什么 / **没有**消除什么（两件事，务必分开读）
///
/// **已消除 ——「复刻漂移」**：改判据前，这两处**各自写了一份**本函数的近似实现。
/// 判据改了而复刻没改 ⇒ 守卫会**对着过时的认知**判断。
/// 现在复刻已删除，两处直接 `use` 本函数 ⇒ **判据与守卫不可能再分叉**。
///
/// **未消除 ——「文本探针 vs 代码语义」的漂移**：
/// `test_req068_no_second_contains_based_prefix_recogniser` **仍然是源码文本探针**
/// （它读 `review_view.rs` 的**文本**）。共用实现**不会**让它变成语义检查：
/// - 它证明不了本函数的**语义**对，只证明「没有第二处 `contains`」；
/// - **别名绕过**（`let p = BIZ_LINE_PREFIX; line.contains(p)`）它照样抓不到；
/// - 只覆盖 `review_view.rs`，在别的文件里再造一套前缀识别它一声不吭。
///
/// ⇒ 「复刻漂移」这一格关上了；「文本探针」那一格**原样还在**，本条**不声称**关掉它。
#[doc(hidden)]
pub fn line_head_prefix_of(line: &str) -> Option<&'static str> {
    let head = line.trim_start();
    if head.starts_with(BIZ_LINE_PREFIX) {
        Some(BIZ_LINE_PREFIX)
    } else if head.starts_with(MODEL_LINE_PREFIX) {
        Some(MODEL_LINE_PREFIX)
    } else if head.starts_with(PROVENANCE_LINE_PREFIX) {
        Some(PROVENANCE_LINE_PREFIX)
    } else {
        None
    }
}


// ──────────────────────────────────────────────────────────
// 判据 ③：提案级归属的呈现（REQ-060 · AC-002 / AC-003）
// ──────────────────────────────────────────────────────────

/// 核对**提案级归属**的渲染 —— 返回全部违规（空 vec = 通过）。
///
/// # 为什么要有第三条判据
///
/// [`audit_fde_channel`] 答「有没有建模数据」，
/// [`verify_track_separation`] 答「两轨有没有串 / 重 / 漏」——
/// **两条都不看归属**。于是一个把归属**逐条目重复打印**的实现
/// （AC-002 点名的那个误导变体）可以一声不吭地通过它们。
///
/// # 四条规则（逐条独立可抓）
///
/// ① **归属行恰好一行** —— 全文里**行首**带 [`PROVENANCE_LINE_PREFIX`] 的行
///    必须**恰好一条**（认法见 [`line_head_prefix_of`]，不认「整行包含」）。
///    抓：AC-002 的「逐条目重复打印」（N 个条目 ⇒ N 行）。
/// ② **归属行不得粘着一条两轨行，且必须排在第一个条目之前** ——
///    它既不属于业务轨也不属于建模轨；落在条目之后会被读成「最后那个条目的归属」。
///    抓：塞进某一条的行 / 排到条目区里。
///    ⚠️ **REQ-068**：前半句问的是**归属前缀之后的正文**行首是不是某一轨
///    （认法见 [`line_head_prefix_of`]），**不是**「整行包含某个两轨前缀」——
///    后者是 F-2 的形态，`agent=` 这类 payload 文本里带「业务轨：」就会**误报**。
///    判据恒假的坑与理由写在下面那段注释里。
/// ③ **归属行必须明确表态** —— [`ProvenanceState`] 四态各自要打出的
///    **判定词**必须出现（`agent=` / 人工修订 / 未提供 / 形态可疑）。
///    抓：AC-001 的「打空 / 省略 / 打 `None` 字面量」。
/// ④ **归属必须独占一行** —— 抓：REQ-061 的存活变异 M2b「归属只拼进第 1 个条目头行」。
///    形态在 REQ-064 改写过：归属行既已按**行首**认（①），
///    「行首的那一条自己起头」就成了**无条件死代码**；
///    现在盯**反向**形态 —— **全文一条行首归属行都没有**时，
///    归属内容只可能被挂在了别的行上，这时要**说得出它挂在哪里**。
pub fn verify_provenance_scope(view: &ReviewTrackView, rendered: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let lines: Vec<&str> = rendered.lines().collect();
    // ⚠️ REQ-064（AC-001）：**行首**认归属前缀，与两轨判据共用
    // [`line_head_prefix_of`] —— 不新写第二套前缀识别。
    //
    // 原来是 `contains(PROVENANCE_LINE_PREFIX)`，那是 F-2 在**同一个文件里
    // 的第三处**残留：claim / 展示名 / 列名里只要含「提案级抽取归属：」
    // 这几个字，那一行就被数成一条归属行 ⇒ 规则①报「行数为 2」的**误报**。
    let prov_lines: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| line_head_prefix_of(l) == Some(PROVENANCE_LINE_PREFIX))
        .map(|(i, l)| (i, *l))
        .collect();

    // ① 恰好一行 —— 抓「逐条目重复打印」。
    if prov_lines.len() != 1 {
        violations.push(format!(
            "提案级归属行数为 {}，应恰好 1 行（逐条目重复打印 = 误导，会让人以为每个条目由不同 agent 抽）",
            prov_lines.len()
        ));
    }

    if let Some((idx, line)) = prov_lines.first() {
        // ② 不带两轨前缀，且排在所有条目行之前。
        //
        // ⚠️ REQ-068（AC-001）：**不再用 `contains` 认两轨前缀** ——
        // 与两轨判据、规则① 共用 [`line_head_prefix_of`]。
        //
        // ⚠️⚠️ **为什么不能直接写 `line_head_prefix_of(line) == Some(BIZ_LINE_PREFIX)`**
        // —— 那会得到一条**恒假**的判据（「看起来有防护其实没有」）：
        // `line` 取自 `prov_lines`，而 `prov_lines` 本身就是按
        // `line_head_prefix_of(l) == Some(PROVENANCE_LINE_PREFIX)` 筛出来的；
        // 三个前缀**两两不互为前缀**（见 [`line_head_prefix_of`] 的注释与
        // `tests/test_review_view_req064.rs`）⇒ `line_head_prefix_of(line)`
        // **恒等于** `Some(PROVENANCE_LINE_PREFIX)`，判它等于 BIZ / MODEL **永远不成立**。
        //
        // ⇒ 所以这条判据问的是**去掉归属前缀之后的正文**：正文一开头**又是一条轨**
        // ⇒ 说明渲染器把某条轨粘在了归属行的尾巴上。
        //
        // ⚠️⚠️ **当前渲染器产不出这个形态**（实测触发 **0** 次）：
        // [`render_provenance_line`] 四态的 body 首串**恒为**
        // `agent=` / `无（` / `未提供（` / `⚠ ` —— **无一以两轨前缀开头**；
        // 生产路径上 `render_review_text` 的输出直喂本判据（`cli.rs`），
        // 全仓只有 `tests/test_review_view_req068.rs` 里的**字符串手术**造得出这个形态。
        // ⇒ 这条判据在生产上**永不触发**，它是**改版探测**，
        // **不是**「现在正在保护什么」的证据（别把它当成一道现役的防线）。
        // 它仍是旧写法的**严格子集**（纯收紧，见 `tests/test_review_view_req068.rs`）。
        //
        // ⚠️ **REQ-074 · 本处与下面 `:1046` 的保护强度**不对等** —— 别把它们当成一样**
        //
        // 两处都在规则②里、都改成按**行首**认，但它们**能被行为测试杀掉的变异不一样**：
        //
        // | 变异 | 只看渲染结果与判决的语义测试（**不读源码**） |
        // | --- | --- |
        // | **本处**退回 `contains` / 用别名绕过 | 🔴 **红** |
        // | `:1046` 退回 `contains` / 用别名绕过 | 🔴 红，但**只在构造文本上** |
        //
        // **为什么本处有、`:1046` 没有**（差别在**见证样本从哪来**，不在判据形状）：
        // 本处问的是**归属行自己的正文**，而正文里的 `agent=<名>` 是**提案方原样写的
        // 任意字符串** —— `extracted_by` 就能让它带上两轨前缀 ⇒ **真实渲染输出**直接把它
        // 顶到「旧命中 / 新不挂」里 ⇒ 只喂**真实渲染**的语义测试照样分得出新旧。
        //
        // ⚠️ **别把这一行读成「`:1046` 也有同样的保护」** —— 理由紧接在下面 `:1046` 那段。
        let prov_body = line.trim_start().strip_prefix(PROVENANCE_LINE_PREFIX).unwrap_or_default();
        if matches!(line_head_prefix_of(prov_body), Some(BIZ_LINE_PREFIX) | Some(MODEL_LINE_PREFIX)) {
            violations.push(format!(
                "提案级归属行后面直接粘着一条两轨前缀行（归属不是任何一轨的）：{line}"
            ));
        }
        // ⚠️ REQ-068（AC-001）：「第一条条目行」同样只按**行首**认，
        // 不按「整行包含」认 —— 否则 claim / 展示名 / `未通过原因` 里
        // 只要带「业务轨：」，`position` 就会把**那一行**当成第一条条目行
        // ⇒ 归属行被误报成「排在条目行之后」。
        //
        // ⚠️⚠️ **REQ-074 · 本处的保护力曾经是「单点」的** —— 与上面 `:1039` **不对等**。
        //
        // # 为什么语义兜底对**本处**无感（上面那一处有感）
        //
        // 差别**不在判据形状**（两处都是按行首认），而在**见证样本从哪来**：
        // 本处问的是「**第一条条目行**在第几行」，新旧写法**只在**
        // 「归属行**之前**有一行含两轨前缀、而它行首不是轨」时**才分道扬镳**；
        // 而 [`render_review_text`] 把归属行固定打在**所有条目行之前**，
        // 它前面只有一行 `整体通过：是/否`（**不含任何 payload**）⇒
        // **在真实渲染输出上，新旧写法逐条同判** ⇒ 只喂真实渲染的语义测试对本处**完全无感**。
        //
        // ⇒ 本处的见证样本**只能**是**构造文本**。这是**已知边界、不是缺陷**
        // （它守的是「渲染器一改版就踩的坑」），但**护栏不该只有一个点**。
        //
        // # 现在有几道（REQ-074 · AC-002）
        //
        // | 防线 | 性质 | 独立于 |
        // | --- | --- | --- |
        // | `test_req068_no_second_contains_based_prefix_recogniser` | **源码文本探针** | 全部行为测试 |
        // | `..._non_item_line_mentioning_track_prefix_before_provenance_...` | 行为 · **一句手写字面量** | 守卫 |
        // | `test_req074_first_item_detection_ignores_every_payload_derived_non_head_line` | 行为 · **载荷由渲染器落到真实行上** | 上面两条 |
        //
        // ⇒ 「**同时**改掉守卫模式与那条构造样本」**动不到**第三道。
        // 实测（M3 与 M4：M4 = M3 **再叠加**删掉 `..._pure_tightening_sample_counts`
        // 里那条载荷样本）：那个组合下前两条**不再**抓住本处，而第三道**红** ——
        // 且守卫那条的红来自它**自己的自检**（「守卫杀不掉 M1」），
        // **不是**它抓住了本处。
        //
        // ⚠️⚠️ **「M4」有三个口径，查之前先对口径**（REQ-074 · N-1）：
        //
        // | 出处 | 它的 M4 指的是 |
        // | --- | --- |
        // | **本处 + `tests/test_review_view_req068.rs` 模块头 REQ-074 节** | **M3 再叠加**删掉语料里那条载荷样本 |
        // | REQ-068 原始口径（模块头 `:39` / `:474`） | `:1046` 改**混合写法** |
        // | `test_review_view_req070.rs` 的 M4 | M3b 叠加语料外载荷丢失（与本处无关） |
        //
        // 本处与测试模块头那两张表**用的是同一套口径**；REQ-068 原始口径的 M4
        // **仍然有效**（由 `..._non_item_line_...` 覆盖），本条**没有**取代它。
        // 三处的完整口径表都在 `tests/test_review_view_req068.rs` 的模块注释里。
        //
        // # 本处**仍然**抓不到什么（别夸大本条的效力）
        //
        // - **别名绕过里语义未变**的那一种（`let bp = BIZ_LINE_PREFIX;` 之后**仍按行首认**）
        //   —— 语义没变就没有任何东西该红；
        // - **换名的第二套 recogniser**（已登记的 M2′ 边界，见本文件的 REQ-073 小节）。
        //
        // ⚠️ REQ-074 另补了一个方向：本处过去**连「整格删掉 / 恒返回 `None`」都没人管**
        // ⇒ `test_req074_provenance_after_first_item_line_is_still_flagged`（反向变异）。
        if let Some(first_item) = lines
            .iter()
            .position(|l| matches!(line_head_prefix_of(l), Some(BIZ_LINE_PREFIX) | Some(MODEL_LINE_PREFIX)))
        {
            if *idx > first_item {
                violations.push(format!(
                    "提案级归属行（第 {} 行）排在条目行（第 {} 行）之后 —— 会被读成「最后那个条目的归属」",
                    idx + 1,
                    first_item + 1
                ));
            }
        }

        // ③ 四态各自必须打出判定词。
        let must = match &view.provenance.state {
            ProvenanceState::Declared { agent, .. } => {
                vec![format!("agent={agent}")]
            }
            ProvenanceState::HumanRevised => vec!["人工修订".to_string()],
            ProvenanceState::NotProvided => vec!["未提供".to_string()],
            ProvenanceState::Suspected { .. } => vec!["形态可疑".to_string()],
        };
        for m in must {
            if !line.contains(&m) {
                violations.push(format!(
                    "提案级归属行没有明确表态 {m:?}（状态={:?}）—— 打空 / 省略 / 打 `None` 字面量都算违规\n该行：{line}",
                    view.provenance.state
                ));
            }
        }

        // ⚠️ REQ-064：原先这里还有一句「这一行的行首必须就是归属前缀」——
        // 它在旧写法下是**独立**的一条规则；归属行改按**行首**认之后，
        // 它对 `prov_lines` 里的**每一条**都**恒真** ⇒
        // 「判据恒为真 ⇒ 无判别力」。所以它被下面的反向形态取代，不是被删掉。
    }

    // ④ 归属必须**独占一行**（REQ-061 · AC-002 的存活变异 M2b · REQ-064 改写形态）。
    //
    // 抓的正是「归属只拼进第 1 个条目头行」（用 ` · ` 分隔）那个形态。
    // ⚠️ 为什么规则②拦不住它 —— 那两条检查它**都躲得过**：
    //   - 条目头行 `  [1] entity · 状态=…` 自身**不带**两轨前缀；
    //   - 它排在两轨行**之前**（行号更小）⇒ 不违反「不得排在条目之后」。
    // 于是 ①②③ 全部放行（③ 也躲得过：那一行里仍然含 `agent=<值>`）。
    //
    // # 形态为什么改写
    //
    // 旧写法拿「整行含前缀」当归属行，再检查它自己起不带头 ——
    // REQ-064 把归属行改成行首认之后，这两步**塌成同一步**，检查恒为假
    // （`line_head_prefix_of` 给出归属前缀的**唯一路径**就是
    //   `trim_start().starts_with(PROVENANCE_LINE_PREFIX)`）。
    // ⇒ 那句检查被**无条件死代码**取代，现在盯**反向**形态：
    // **全文一条行首归属行都没有** ⇒ 归属内容只可能被挂在了别的行上。
    //
    // ⚠️ 为什么不能「凡非行首出现就算违规」：
    // 那样 claim / 展示名 / 列名里含「提案级抽取归属：」又会被误报 ——
    // 那正是 REQ-064 要消灭的**同一个**误报，只是换了个方向。
    //
    // ⚠️⚠️ **不要**在这里加「有行首归属行时，再查一遍非行首的出现」。
    // 第一轮加过（当时叫分支 (b)），**QA 判 FAIL 并已删除**：
    // 那种规则需要「这一行不可能承载 payload」这种前提，而它**可证伪** ——
    // 那时 `evidence_object_body` 把 claim **逐字** `push_str`（不转义换行），
    // claim 里的 `\n[1] ` 会切出一行 `[1] 提案级抽取归属：…`，
    // 它**每个字节都来自 payload**，却被 `is_item_header_line`（**纯句法**）认成条目头行 ⇒ **误报**。
    // 实测 3 个条目 ⇒ 3 条理由全错的违规，而什么都没发生。
    // ⇒ 「这一行不可能承载 payload」**不是**可用的判据，别再用它。
    //
    // ⚠️ **REQ-069 不改变「分支 (b) 不该加回来」这个结论** ——
    // 它只是把上面那条反例的**根**（换行造行）修掉了。
    // **「反例不存在了」不等于「那条规则变正确了」**：分支 (b) 的问题从来是
    // 它依赖一个**可证伪**的前提；根因被修 ≠ 依赖它的推理就成立。
    // 本条⛔ **不恢复分支 (b)**（BA 已裁定，且 QA 实测它带来的是纯误报）。
    if prov_lines.is_empty() {
        // (a) 全文**没有**行首归属行 ⇒ 归属内容只可能被挂在了别的行上。
        //
        // ⚠️ 这一段里的 `contains` 是**故意的**，且只在 `prov_lines` 为空时才用：
        // 而 `prov_lines` 为空时规则①**已经**报了违规，所以它
        // **只可能给已经失败的判决补一条更准的理由，绝不会把「通过」翻成「违规」**。
        if let Some((idx, line)) =
            lines.iter().enumerate().find(|(_, l)| l.contains(PROVENANCE_LINE_PREFIX))
        {
            let head = line.trim_start();
            let where_ = if is_item_header_line(head) { "条目头行" } else { "别的行" };
            violations.push(format!(
                "提案级归属被拼进了一条{where_}（第 {} 行），而不是独占一行 —— 归属是整份提案的，挂在别的行上会被读成「那一行的归属」\n该行：{line}",
                idx + 1
            ));
        }
    }

    violations
}

/// 一行是不是**条目头行**形态（`  [1] entity · 状态=Accepted`）。
///
/// 只认「行首是 `[` + 纯数字 + `]`」—— 那正是 [`render_review_text`] 打条目头行的方式。
/// 归属行自己永远以 [`PROVENANCE_LINE_PREFIX`] 开头，所以这个判别不会把正常归属误认成条目头行。
///
/// 它的作用**只有一个**：让规则④的违规理由**说得出是哪一种行**，
/// 而不是笼统地说「位置不对」。
///
/// ⚠️ **它只是句法判别，不能当「这一行不可能承载 payload」的证据**：
/// REQ-064 第二轮 QA 判 FAIL 的阻断项就是这个误用。
/// 当时的反例是 claim 里的 `\n[1] ` 会切出一行 `[1] 提案级抽取归属：…` ——
/// [`evidence_object_body`] 当时把 claim **逐字** `push_str`、**不转义换行**。
///
/// ⚠️ **REQ-069 修的是「根」，不是这条注释的推理**：现在换行走
/// [`escape_payload_text`]（LF/CR ⇒ `␊`/`␍`），payload **造不出新行**。
/// ⚠️ 但**本函数仍然是纯句法的**，那条「不能当证据」的理由**没有被本条推翻** ——
/// 它只是不再有已知的 payload 反例。**「没有已知反例」≠「结构上不可能」**：
/// 任何人将来在渲染层加一条不经 [`escape_payload_text`] 的出口，
/// 这条推理立刻重新成立。所以：**别拿本条当「这行不可能承载 payload」的许可证。**
fn is_item_header_line(trimmed_line: &str) -> bool {
    let Some(rest) = trimmed_line.strip_prefix('[') else { return false };
    let Some(close) = rest.find(']') else { return false };
    let seq = &rest[..close];
    !seq.is_empty() && seq.bytes().all(|b| b.is_ascii_digit())
}

// ──────────────────────────────────────────────────────────
// 渲染
// ──────────────────────────────────────────────────────────

/// 业务轨那一行的前缀。
///
/// 它标记了「这一行是业务轨的」——
/// [`verify_track_separation`] 靠它把两行分开数，并核对业务轨行**只**有两栏。
///
/// ⚠️ REQ-057：判据**不再**是「整段文本里有没有建模术语」，
/// 所以这个前缀对判据的贡献从「什么都不做」变成了「定位到具体那一行」。
pub const BIZ_LINE_PREFIX: &str = "业务轨：";

/// 建模轨那一行的前缀。
pub const MODEL_LINE_PREFIX: &str = "建模轨：";

/// payload 里没有 key 时，建模轨 `key=` 那一段打的**缺失标记**。
///
/// ⚠️ REQ-057（F-D）。刻意**不含任何看起来像 key 的东西** ——
/// 宁可让 FDE 看见「这一栏缺了」，也不要给它一个展示名去误当系统标识。
pub const KEY_MISSING_MARK: &str = "（无 key）";

/// 提案级归属那一行的前缀（REQ-060）。
///
/// ⚠️ **刻意不含** [`BIZ_LINE_PREFIX`] / [`MODEL_LINE_PREFIX`]：
/// 归属是**提案级**的，既不属于业务轨也不属于建模轨，
/// 落在任一条轨的行上都会污染两轨分界判据。
pub const PROVENANCE_LINE_PREFIX: &str = "提案级抽取归属：";

/// 归属那一行里说明**这是整份提案的、不是逐条目的**的那半句（REQ-060 · AC-002）。
///
/// ⚠️ 这句话是 AC-002 的**判别力载体** —— 变异「把归属逐条目重复打印」
/// 会被 [`verify_provenance_scope`] 抓住，而不只是靠这句话的存在。
pub const PROVENANCE_SCOPE_NOTE: &str = "整份提案共用一份，不是逐条目的";

/// 把提案级抽取归属渲染成 `review show` 里要打的那一段文本。
///
/// # 三种状态各自**必须**给出明确表述（AC-001 / AC-003）
///
/// - [`ProvenanceState::Declared`] ⇒ 打 `agent=<名>`；
/// - [`ProvenanceState::HumanRevised`] ⇒ 打「人工修订」；
/// - [`ProvenanceState::NotProvided`] ⇒ 打「未提供」；
/// - [`ProvenanceState::Suspected`] ⇒ 打「形态可疑」+ **具体原因** + 原始取值。
///
/// **四种都不可省略**：省掉一行 = FDE 以为「这份提案没有归属信息」，
/// 而实际是「有，但形态可疑」—— 那是**两个相反的结论**。
pub fn render_provenance_line(view: &ReviewTrackView) -> String {
    let body = match &view.provenance.state {
        ProvenanceState::Declared { agent, version, round, failed_groups } => {
            let mut s = format!("agent={agent}");
            s.push_str(&format!(" · version={}", version.as_deref().unwrap_or("（未声明）")));
            if let Some(r) = round {
                s.push_str(&format!(" · 第 {r} 轮"));
            }
            // 失败分组非空 = 这一轮**没有全成功** —— 必须显式说，
            // 否则 FDE 会把「抽出来了」误读成「全抽出来了」。
            if !failed_groups.is_empty() {
                s.push_str(&format!(" · ⚠ 本轮 {} 个分组抽取失败：{}", failed_groups.len(), failed_groups.join("、")));
            }
            s
        }
        ProvenanceState::HumanRevised => "无（人工修订，ADR-001：这份提案的 extracted_by 是 SQL NULL）".to_string(),
        ProvenanceState::NotProvided => {
            // ⚠️ REQ-061（F-3）：这一句是**纯文本终端输出**，终端不渲染 Markdown ——
            // 星号会原样打给 FDE 看。所以措辞里**不许**出现 `**…**` 这类标记；
            // 强调靠措辞本身（「不是」两个字已经足够），不靠 Markdown。
            "未提供（这条路径没有接提案行，无法判断谁抽的 —— 不是「人工修订」）".to_string()
        }
        ProvenanceState::Suspected { reason, raw } => {
            format!("⚠ 形态可疑，不作为归属采信 —— {reason}；原始取值：{raw}")
        }
    };
    format!("{PROVENANCE_LINE_PREFIX}{body}（{PROVENANCE_SCOPE_NOTE}）\n")
}

/// 把建模轨投影渲染成 `review show` 要打的那段文本。
///
/// **两轨各占一行**：业务轨只打展示名与业务轨裁决，
/// 建模轨只打 key / 列映射 / 证据 / 基数 / 两端。
/// 这条「各占一行」的分界就是 AC-003 的判据
/// （由 [`verify_track_separation`] 核对，本函数自己不作数）。
pub fn render_review_text(view: &ReviewTrackView) -> String {
    let mut out = String::new();
    out.push_str(&format!("整体通过：{}\n", if view.passed { "是" } else { "否" }));
    // REQ-060 · 提案级抽取归属 —— **恰好一次**，在所有条目**之前**。
    //
    // ⚠️ 刻意不放在任何条目行里：
    //   - 放进业务轨行 ⇒ 违反 `verify_track_separation` 规则②（该行只许两栏）；
    //   - 放进建模轨行 ⇒ 混入两轨分界，且**看起来像条目级归属**（AC-002 的误导）。
    out.push_str(&render_provenance_line(view));
    for it in &view.items {
        out.push_str(&format!(
            "  [{}] {} · 状态={:?}\n",
            it.seq,
            it.kind.as_str(),
            it.state
        ));
        out.push_str(&format!(
            "      {BIZ_LINE_PREFIX}{} · biz={}\n",
            it.biz.display_target,
            it.biz.status.as_str()
        ));
        // REQ-057（F-D）：`key` 这一栏**始终打出来**，但取值可能不是 key。
        //
        // 缺失时打 `key=（无 key）` 而不是省略 —— 两个理由：
        // ① 让 FDE **看得见这一栏缺了**（省略的话，缺 key 与「本来就没这栏」长得一样）；
        // ② 保住「建模轨那一行必带 `key=`」这条结构性质，
        //    使退化输入下两轨分界依然可判。
        //
        // ⚠️ 绝不打展示名顶替 —— 那正是 F-D：展示名被当成 key，FDE 分不出
        // 「两条提议是不是同一个东西」。
        let key_part = match it.model.type_key.as_deref().map(str::trim) {
            Some(k) if !k.is_empty() => format!("key={k}"),
            _ => format!("key={KEY_MISSING_MARK}"),
        };
        let mut parts: Vec<String> = vec![key_part];
        if !it.model.columns.is_empty() {
            let cols = it
                .model
                .columns
                .iter()
                .map(|c| format!("{}←{}", c.name, c.column))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!("列映射：{cols}"));
        }
        if !it.model.evidence.is_empty() {
            parts.push(format!("证据：{}", it.model.evidence.join("、")));
        }
        if let (Some(f), Some(t)) = (&it.model.from_type, &it.model.to_type) {
            parts.push(format!("两端：{f}→{t}"));
        }
        if let Some(card) = &it.model.cardinality {
            parts.push(format!("基数：{card}"));
        }
        parts.push(format!("model={}", it.model.status.as_str()));
        out.push_str(&format!("      {MODEL_LINE_PREFIX}{}\n", parts.join(" · ")));
        if let Some(reason) = &it.reason {
            out.push_str(&format!("      未通过原因：{reason}\n"));
        }
    }
    if !view.dangling_relations.is_empty() {
        out.push_str(&format!("悬空关系：{:?}\n", view.dangling_relations));
    }
    for b in &view.blocked {
        out.push_str(&format!(
            "被拦：{} —— {}\n",
            b.target,
            b.reason.as_deref().unwrap_or("")
        ));
    }
    out
}

// ──────────────────────────────────────────────────────────
// payload 取值
// ──────────────────────────────────────────────────────────

/// payload 里的**字符串栏**，**已转义行结构字符**（REQ-069）。
///
/// ⚠️ 这是 REQ-069 的**主出口**：任何取自 payload 的字符串都从这里进投影，
/// 于是**所有**读行的判据（[`verify_track_separation`] / [`verify_provenance_scope`]）
/// 一次性免疫，不必给每条判据各加一层检查。
fn payload_str(payload: &Value, field: &str) -> Option<String> {
    payload.get(field).and_then(|v| v.as_str()).map(escape_payload_text)
}

/// payload 里的**类型 key** —— REQ-057（F-D）。
///
/// # 为什么**刻意不用** [`crate::domain::review::ItemView::type_key`]
///
/// `ItemView::type_key()` 在 payload 缺 `key` 时**回退到 `target`（展示名）**。
/// 那是给 `summarize` / `apply` 的**命名空间对齐**用的 ——
/// 两个命名空间必须都能查到同一个东西，回退是有意为之。
///
/// 但**建模轨展示**不能回退：`key=主键服务中心` 里那个「主键服务中心」
/// 是展示名，FDE 会把它当成系统标识，于是「两条提议是不是同一个东西」
/// 判断错了，而且**看上去毫无异样**。
///
/// 所以这里**只认 payload**。缺 `key`、或 `key` 是空串 / 纯空白 ⇒ `None`。
///
/// ⚠️ `review.rs` 是红线文件，**没有也不许改** —— 回退逻辑原样保留给
/// `summarize` / `apply` 用，本函数只在**展示侧**另开一路。
fn payload_key(payload: &Value) -> Option<String> {
    payload
        .get("key")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|k| !k.is_empty())
        // ⚠️ REQ-069：**在 trim / 空值判定之后**才转义（见 [`escape_payload_text`]）。
        // 顺序反了会把「`key = "\n"` ⇒ 空 ⇒ `None`」变成「非空 ⇒ `Some`」，
        // `sourced_fields` 凭空多一栏 —— 那是改既有判据行为，不是本条要的。
        .map(escape_payload_text)
}

/// payload 里的**列映射**。
fn payload_columns(payload: &Value) -> Vec<ModelColumn> {
    payload
        .get("properties")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let name = p.get("name").and_then(|x| x.as_str())?;
                    let column = p.get("column").and_then(|x| x.as_str()).unwrap_or("");
                    Some(ModelColumn {
                        // REQ-069：列名 / 列名同样取自 payload ⇒ 同样要走转义出口。
                        name: escape_payload_text(name),
                        column: escape_payload_text(column),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// payload 里的**证据**，渲染成一行行的人读得懂的话。
///
/// # ⚠️ REQ-070：这是 [`render_evidence_entry`] 的**唯一**调用点，
/// 且它的产出**一律**要过 [`seal_evidence_line`]
///
/// 载荷保护落在这里、而不是渲染器内部，是因为**只有这里同时握着
/// 「原始 `Value`」与「渲染器算好的串」** —— 在渲染器内部加一条提前 `return`
/// （QA 的 C5 / C6）只能改后者，改不了随后按前者做的核对。
///
/// ⚠️ 代价照实说：**改这条链**（把渲染器直接接到 `evidence` 上、删掉封条）
/// 会重新打开那个洞。
/// 「封条还在不在链上」由 `tests/test_review_view_req070.rs` 里的
/// **行为判定**盯着（REQ-076 · AC-001：喂一组见证形状，断言渲染结果里
/// 看得见至少一个载荷原子）；「有没有第二个渲染出口」由同文件的**结构守卫**盯着。
/// ⚠️ 后者仍是**源码文本探针**，照实说它抓不到别名绕过（见本函数上方的边界表）。
///
/// # ⚠️ REQ-077：「一律」是**逐个元素**的一律，不是「渲染过一次」的一律
///
/// 本函数对 `payload["evidence"]` 的**每个元素**各过一次封条 ——
/// 这一维以前**没有任何判据问过**（既有判据每次只把**一条** evidence 包成单项，
/// 索引恒为 0）⇒ 「封条只对第 1 条生效」曾经**九个测试二进制全绿**。
/// 现由同文件的 `test_req070_the_seal_applies_to_every_evidence_entry_not_only_the_first`
/// （REQ-077 · AC-001：一次喂多条，逐条断言**每一条**看得见**它自己独占的**载荷原子）盯着。
/// ⚠️ 那条判据**仍不声称**封死了：见证形状**不穷举**（REQ-066 已证有限枚举不可穷尽），
/// 它要的是**至少一个承重见证**——够用即止，理由见该文件的模块注释。
fn payload_evidence(payload: &Value) -> Vec<String> {
    payload
        .get("evidence")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|ev| seal_evidence_line(ev, render_evidence_entry(ev)))
                .collect()
        })
        .unwrap_or_default()
}

/// 单条证据的渲染。
///
/// 快照层的 `Evidence` 是 `{table, column, claim}`，提案 payload 里也是同一个形状；
/// 但 payload 是**原始 JSON**，形状不保证 —— 认不出三段式就退回紧凑 JSON，
/// **不静默丢掉**（丢掉 = FDE 少一条复核依据，却看不出来）。
///
/// # ⚠️ REQ-058（AC-001）：退化值**保留呈现**，但**不再冒充「有依据」**
///
/// 「不静默丢掉」这条纪律**照旧**—— 退化项照样出现在输出里，
/// FDE 看得见「这里有一条退化的证据」。但它渲染出来的**不再是**一个
/// 看起来像内容的普通串，而是一个 [`DEGENERATE_EVIDENCE_MARK`] 包起来的
/// 保留标记，于是 [`ModelFacing::sourced_fields`] 不会把它算成「有依据」。
///
/// 退化**逐个可区分**：`""` / `"   "` / `{}` / `null` / `[]` / 裸标量 /
/// 有键无值 各自的措辞都**不同**，FDE 看得见「它到底退化成什么」——
/// 统一成一个占位串会把这个问题重新藏起来。
///
/// # ⚠️ REQ-065：逐个可区分**还不够**，还必须**带载荷**
///
/// REQ-058 的 QA 复验实测出：`{"table":"","column":"","claim":""}` 与
/// `{"foo":"","bar":null}` 渲染成**逐字相同**的 `␀有键无值␀` —— 措辞可区分，
/// **载荷被藏起来了**（FDE 知道「它退化了」，却看不到「退化的到底是什么」）。
/// 于是标签 = **类别名 + 载荷指纹**，指纹只由 `payload` 决定
/// （见模块文档的不变量一节与 [`EvidenceDegeneracy::label`]）。
fn render_evidence_entry(v: &Value) -> String {
    // 判据与渲染**共用同一个结论**：退化类别是从**原始 `Value`** 上判出来的
    // （[`evidence_degeneracy`]），不是从渲染结果上倒推的。
    if let Some(d) = evidence_degeneracy(v) {
        // 标签 = **类别名 + 载荷指纹**，两者在 `EvidenceDegeneracy::label` 里拼。
        // ⚠️ 这条路径**自己**不再保证什么：REQ-070 把载荷保证搬到了**消费侧** ——
        // 本函数的产出**一律**要过 [`seal_evidence_line`]（唯一调用点 [`payload_evidence`]），
        // 所以在这里、或在 `label()` 里加几条提前 `return`，载荷都**补得回来**。
        // 措辞同样走转义：payload 的 `␀` 不能原样混进来破坏「以 `␀` 开头且结尾」。
        let label = escape_payload_text(&d.label(v));
        return format!("{DEGENERATE_EVIDENCE_MARK}{label}{DEGENERATE_EVIDENCE_MARK}");
    }
    let raw = match v.as_object() {
        Some(o) => {
            let body = evidence_object_body(o);
            // ⚠️ **基线的兜底分支，不能丢**（QA B-1 阻断）。
            //
            // 三段式认不出来（键不是 table/column/claim，或取值不是字符串）
            // 时退回**紧凑 JSON** —— 上面那句「不静默丢掉」指的就是这里。
            // 少了这一句，`{"other":"x"}` 会渲染成**空串**：输出里只看得见
            // 光秃秃的「证据：」，FDE 既看不出有内容、也看不出缺了什么，
            // **比渲染成占位串更糟**。
            if !body.is_empty() {
                body
            } else {
                v.to_string()
            }
        }
        // 非对象且非退化 ⇒ 紧凑 JSON（「不静默丢掉」的原有出口）。
        None => v.to_string(),
    };
    escape_payload_text(&raw)
}

/// 单条证据渲染结果的**载荷封条**（REQ-070 · AC-001）。
///
/// # 它解决的是哪一格
///
/// REQ-065 的不变量此前**只在「触发形状恰好落在测试语料里」时成立**：
/// 在 [`render_evidence_entry`] 内部加一条提前 `return`（QA 的 C5：某个**语料外**
/// 形状的标签只留类别名；C6：整条连定界符都不打），
/// 载荷就完全消失，而 REQ-065 的语料判据一声不吭。
/// ⇒ **语料外零保护。**
///
/// # 为什么它落在**消费侧**
///
/// 封条拿到的是**「原始 `Value` + 渲染器已经算好的串」**。
/// 渲染器内部无论加几条分支、加几条提前 `return`，都只能改后者 ——
/// **改不了封条随后按前者做的核对**。这是「让 `label()` 除载荷外没有别的返回路径」
/// 之外唯一能覆盖「另开出口」那条路的做法。
///
/// # 它**不枚举任何形状**
///
/// 两条规则对**每一个** `Value` 都成立，与它是否出现在任何一份语料里无关 ——
/// 这正是 AC-001 要的「脱离语料」。
///
/// # 两条规则（**互斥**，各对应一轴）
///
/// | 规则 | 条件 | 动作 | 杀掉 |
/// | --- | --- | --- | --- |
/// | ① 载荷轴 | 这一行**带**定界符 ⇒ 它自称退化标签 | 载荷指纹**补进定界符之内** | **C5** |
/// | ② 冒充轴 | 这一行**不带**定界符 ⇒ 它自称「按原样呈现」，却**连一个载荷原子都没有** | 重新按原样呈现紧凑 JSON | **C6** |
///
/// ## ① 为什么补在定界符**之内**
///
/// 补在外面会把结尾定界符挤掉，那一行就不再是退化标签 ——
/// [`evidence_line_carries_data`] 的判决会跟着变，等于用一条渲染修补
/// 改掉 REQ-058 的判据行为（AC-004 禁止的那类改动）。
///
/// ## ② 为什么判「一个原子都没有」而不是「逐项齐全」
///
/// 「逐项齐全」在基线上**不成立**：非退化对象走 [`evidence_object_body`] 的
/// 三段式摘要（`{"table":"t","column":"c","claim":"x"}` → `t.c：x`），
/// **键名本来就不在输出里** —— 那是 REQ-058 明写的设计。
/// 若按「逐项齐全」封口，这条封条会在基线上到处开火 ⇒ 那就不是封条，是改判据。
///
/// 「一个原子都没有」则是**单向安全**的：基线上每条非退化行都至少含一个
/// 载荷原子（三段式推出 `table`/`column`/`claim` 的**取值**原文，
/// 紧凑 JSON 兜底含全部），所以规则 ② **在基线上恒不触发** ——
/// 它的两个分支只在渲染器真的写了藏载荷的代码时才可达。
///
/// # ⚠️ 它**抓不到**哪几格（照实写，不是免责）
///
/// - **改这条链**：把 [`render_evidence_entry`] 直接接到 `evidence` 上、或删掉封条
///   —— 前者由「渲染调用点数」那条**结构探针**盯着（⚠️ 它认不出别名绕过），
///   后者由 `tests/test_review_view_req070.rs` 里的**行为判定**盯着
///   （REQ-076 · AC-001：封条规则②的承重性，见该文件模块注释的边界表）；
///   ⚠️ 本条**不声称**那条行为判定是现役防线 —— 它只把「静默存活」变成「变红」；
/// - **只藏一半**：行里留**一个**原子（哪怕是个键名）就绕过规则 ② ——
///   「完整呈现」这一层仍由 REQ-065 的语料判据负责，本条不取代它；
/// - **改判据两侧一起改**：把 `evidence_degeneracy` 的分类改成「某形状算退化」
///   又同时让 [`evidence_object_body`] 把它原样吐出 —— 两侧同改，规则 ② 不会响。
fn seal_evidence_line(v: &Value, line: String) -> String {
    let fingerprint = escape_payload_text(&evidence_payload_fingerprint(v));

    // 规则 ①：带定界符 ⇒ 它自称是退化标签 ⇒ 定界符之间必须带得下这个元素的载荷。
    if let Some(inner) = strip_degenerate_marks(&line) {
        if fingerprint.is_empty() || inner.contains(&fingerprint) {
            return line;
        }
        return format!(
            "{DEGENERATE_EVIDENCE_MARK}{inner} {fingerprint}{DEGENERATE_EVIDENCE_MARK}"
        );
    }

    // 规则 ②：没带定界符 ⇒ 它自称是「按原样呈现」。一个**连一个载荷原子都没有**
    // 的串不是呈现、是**判决**：它在替载荷下结论，而结论连载荷本身都没露出来。
    if payload_atoms(v).is_empty() || payload_atoms(v).iter().any(|a| line.contains(a.as_str())) {
        return line;
    }
    escape_payload_text(&v.to_string())
}

/// 这一行是不是**保留定界符包起来的退化标签**，是的话把**里面那部分**交出来。
///
/// ⚠️ 它与 [`evidence_line_carries_data`] 是**同一个判定**（那边取反用）——
/// 两边写成两个实现就等于留了一处可以只改一边的漂移点（REQ-069 AC-002 同理）。
fn strip_degenerate_marks(line: &str) -> Option<&str> {
    if line.chars().count() > 1
        && line.starts_with(DEGENERATE_EVIDENCE_MARK)
        && line.ends_with(DEGENERATE_EVIDENCE_MARK)
    {
        line.strip_prefix(DEGENERATE_EVIDENCE_MARK)
            .and_then(|s| s.strip_suffix(DEGENERATE_EVIDENCE_MARK))
    } else {
        None
    }
}

/// 这个元素的**载荷原子** —— 「一个忠实的呈现里至少得看得见一个」的那个「一个」。
///
/// 原子 = 对象的**键名** + 全部**非空叶子标量**（字符串 / 数字 / 布尔）的字面文本。
///
/// ⚠️ **刻意不含**：`null` 与空容器（`{}` / `[]`）—— 它们「无可藏」，
/// 拿它们当原子会让规则 ② 对空形失灵。
/// ⚠️ **刻意不含**：纯空白字符串（`"   "`）—— 空白有多少本来就说不清
/// （见模块文档「为什么『非空』不是这条不变量」）。
fn payload_atoms(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_payload_atoms(v, &mut out);
    out
}

fn collect_payload_atoms(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            for (k, val) in o {
                out.push(k.clone());
                collect_payload_atoms(val, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|e| collect_payload_atoms(e, out)),
        Value::String(s) => {
            if !s.trim().is_empty() {
                out.push(s.clone());
            }
        }
        Value::Number(_) | Value::Bool(_) => out.push(v.to_string()),
        Value::Null => {}
    }
}

/// 对象形态证据的**三段式**呈现（`表.列：claim`）。
///
/// ⚠️ 空 / 纯空白的 `table`、`column` 当作**没有**——
/// 否则 `{"table":"","column":""}` 会渲染成 `"."`，那是一个**看起来像内容**
/// 的串，实际什么都没说。
///
/// # ⚠️ 这里**逐字** `push_str(claim)` 是**安全的** —— 因为上游有转义
///
/// 本函数自己不转义：它在 [`render_evidence_entry`] 的链路上，而那里
/// 对成品串调 [`escape_payload_text`]（见该函数与 [`render_evidence_entry`]）。
/// ⇒ claim 里的 LF / CR 到渲染结果之前已被换成 `␊` / `␍`，
/// **造不出新的一行**（REQ-069 · AC-001）。
/// ⚠️ 这条「安全」**依赖那条转义链仍然存在** —— 若有人在这里另开一个
/// 未经 [`escape_payload_text`] 的出口，本函数的逐字拼接立刻重新变成漏洞。
fn evidence_object_body(o: &serde_json::Map<String, Value>) -> String {
    let get = |k: &str| o.get(k).and_then(|x| x.as_str());
    let present = |k: &str| get(k).filter(|s| !s.trim().is_empty());
    let mut s = match (present("table"), present("column")) {
        (Some(t), Some(c)) => format!("{t}.{c}"),
        (Some(t), None) => t.to_string(),
        (None, Some(c)) => c.to_string(),
        (None, None) => String::new(),
    };
    if let Some(claim) = get("claim") {
        if !s.is_empty() {
            s.push('：');
        }
        s.push_str(claim);
    }
    s
}

// ──────────────────────────────────────────────────────────
// 退化 evidence 的分类与保留标记（REQ-058 · AC-001 / AC-002）
// ──────────────────────────────────────────────────────────

/// 退化 evidence 渲染成保留标记时用的**定界符**。
///
/// ⚠️ 任何**取自 payload** 的文本在渲染时都会把这个字符**转义**成
/// [`DEGENERATE_EVIDENCE_ESCAPE`]（见 [`escape_payload_text`]）。
/// 于是「以它开头**且**以它结尾」在**结构上**只可能来自本模块的退化出口 ——/
/// [`evidence_line_carries_data`] 判「这一条不携带可复核依据」因此是**精确**的：
/// 既不会放过退化值，也不会误伤一条 claim 恰好长得像标记串的真证据。
///
/// **这不是 parser。** 它不解析任何「给人看的」措辞：
/// 退化类别在 [`evidence_degeneracy`] 里从**原始 `Value`** 上判出来，
/// 渲染与判据共用同一个结论，中间没有第二套「理解文本」的逻辑。
pub const DEGENERATE_EVIDENCE_MARK: char = '\u{2400}';

/// payload 文本里 [`DEGENERATE_EVIDENCE_MARK`] 的转义写法。
const DEGENERATE_EVIDENCE_ESCAPE: char = '\u{2401}';

/// payload 文本里**换行**（LF）在渲染时的转义写法（REQ-069）。
///
/// ⚠️ 与 [`DEGENERATE_EVIDENCE_MARK`] / [`DEGENERATE_EVIDENCE_ESCAPE`]
/// **同一个 Unicode「Control Pictures」区**（U+2400 起）——
/// 所以换行转义是**那套转义机制的扩展**，不是另造一套（见 [`escape_payload_text`]）。
const LINE_FEED_ESCAPE: char = '\u{240A}';

/// payload 文本里**回车**（CR）在渲染时的转义写法（REQ-069）—— 同 [`LINE_FEED_ESCAPE`]。
const CARRIAGE_RETURN_ESCAPE: char = '\u{240D}';

/// 退化成什么 —— **逐个可区分**（AC-001：不许合并成一类）。
///
/// 每个变体在输出里各自有措辞，所以 FDE 看得见「这一条退化成的是什么」。
///
/// # ⚠️ 变体里**不许**再存载荷 —— 载荷统一由 [`evidence_payload_fingerprint`] 拼（REQ-065）
///
/// 各变体**只提供类别名**（[`Self::category`]）。这不只是整洁：
/// 它把「标签带什么」从**每个类别各自的决定**改成
/// **一个 `payload` 的函数**（[`Self::label`]）——
/// 于是**新增退化类别时，走 [`Self::label`] 的默认路径就带得上载荷**。
///
/// ⚠️ 上面那句的**前提是「这条路径上没有别的出口」**（REQ-070 · AC-004）：
/// 在 [`Self::label`] 里给某个形状加一条提前 `return`，或在本函数之外另开出口，
/// 这条路径就**不再是唯一的** —— 那时带不带载荷由那条新出口说了算，
/// 只是产出随后会被 [`seal_evidence_line`] 补回来。
/// **所以这不是「没法漏」的保证，是「漏了会被收口」的结构。**
#[derive(Debug, Clone, PartialEq, Eq)]
enum EvidenceDegeneracy {
    /// `""` —— 空串。注意它**渲染成两个字符**（`serde_json` 的 `Display` 带引号）。
    EmptyString,
    /// `"   "` —— 非空但纯空白。
    BlankString,
    /// `{}` —— 一个键都没有。
    EmptyObject,
    /// `null`。
    Null,
    /// `[]` —— 一个元素都没有。
    EmptyArray,
    /// `[…]` —— **非空**，但里面**每一条**都退化。
    ///
    /// 带着**原始内容**（QA B-1 的同类教训）：退化标签不许把
    /// payload 里实际有的东西藏起来 —— `[1,2]` 渲染成 `全退化数组 [1,2]`，
    /// 不是 `全退化数组`。
    AllDegenerateArray,
    /// 裸标量（`0` / `false` / `42` …）—— 不是 `{table, column, claim}`，
    /// 也不是一句话，拿不出任何可供复核的指向。带着字面量以示区别。
    Scalar,
    /// 有键、但**每个值都是空 / 纯空白 / null** —— 三段式渲染出来什么也不是。
    ///
    /// ⚠️ REQ-065：**必须带出键名**。`{"table":"","column":"","claim":""}`
    /// 与 `{"foo":"","bar":null}` 是**两种不同性质的问题**
    /// （前者知道三段式 schema 却没填值，后者发了无关键），
    /// 渲染成同一个 `␀有键无值␀` ⇒ 提案方**看不出自己漏了什么**。
    KeyWithoutValue,
}

impl EvidenceDegeneracy {
    /// 这一类退化在输出里的**类别名**（**只有名字，不带载荷**）。
    ///
    /// ⚠️ 这里**不许**拼载荷 —— 载荷走 [`Self::label`]，且在 `match` **之外**追加。
    /// 把拼装放回本函数就等于把「藏不藏」交还给**每个类别自己决定**，
    /// 那正是 REQ-065 要从结构上拔掉的东西。
    fn category(&self) -> &'static str {
        match self {
            EvidenceDegeneracy::EmptyString => "空串",
            EvidenceDegeneracy::BlankString => "纯空白",
            EvidenceDegeneracy::EmptyObject => "空对象",
            EvidenceDegeneracy::Null => "null",
            EvidenceDegeneracy::EmptyArray => "空数组",
            EvidenceDegeneracy::AllDegenerateArray => "全退化数组",
            EvidenceDegeneracy::Scalar => "裸值",
            EvidenceDegeneracy::KeyWithoutValue => "有键无值",
        }
    }

    /// 这一类退化在输出里的**完整标签** = 类别名 + 载荷指纹（REQ-065 不变量）。
    ///
    /// # 载荷部分在 [`Self::category`] **之外**追加 —— 这是不变量的**一半**
    ///
    /// ```text
    /// label(v) = category() + 载荷指纹(v)
    /// ```
    ///
    /// 「带什么载荷」只由 `v` 决定（[`evidence_payload_fingerprint`]），
    /// **与是哪一类无关** ⇒ **走本函数默认路径**的新增退化类别自动带上载荷。
    /// ⚠️ **另一半**是「本函数没有别的出口」—— 见下。
    ///
    /// ⚠️ 上面那句是**有条件**的：它要求**本函数没有提前返回的分支**。
    /// 加一条（QA 的 C5 实测：形状落在测试语料之外时，REQ-065 的 4 条判据全绿）
    /// 载荷就会从这里消失 —— 只是 [`seal_evidence_line`] 随后会把它**补回定界符之内**，
    /// 补不回的是**那条分支的存在本身**，所以结构守卫盯着它
    /// （`tests/test_review_view_req070.rs`）。
    ///
    /// # REQ-075：盯着它的那条守卫**换了形态**
    ///
    /// 原来它靠**源码字面量**（`if fingerprint.is_empty() {`）认分支条件，
    /// 而 QA 的 **M9** 实测表明：**逐字保留那个字面量、只把指纹换成影子变量**就能全绿。
    /// ⇒ 那条字面量断言**已删除**，换成**直接调用本函数**的行为扫描
    /// （[`evidence_degeneracy_labels`]，REQ-075 AC-003）：
    /// **凡是改掉本函数输出的写法** —— 提前 `return`、影子变量、别名、
    /// 把拼装挪进 [`Self::category`] —— 都会让扫描网报红。
    ///
    /// ⚠️ **边界照实写**：扫描网的**类别轴**是穷举的（[`EvidenceDegeneracy::ALL`]），
    /// 但**取值轴仍是一份有限语料**。一个「类别已在网里、取值形状语料外」的
    /// 载荷丢失，它**抓不到**（抓不到的原因与 REQ-070 原样：有限枚举不可能穷尽）。
    /// ⇒ 本条**不声称**该格已封堵，详见 `tests/test_review_view_req070.rs` 模块注释。
    fn label(&self, v: &Value) -> String {
        let fingerprint = evidence_payload_fingerprint(v);
        if fingerprint.is_empty() {
            // 规范空形（`""` / `null` / `{}` / `[]`）：类别名已经把它说尽了。
            self.category().to_string()
        } else {
            format!("{} {fingerprint}", self.category())
        }
    }

    /// **全部退化类别** —— [`evidence_degeneracy_labels`] 的扫描轴（REQ-075）。
    ///
    /// ⚠️ **手写的清单**，所以它自己是**一个可以漏的地方**，照实写明：
    ///
    /// - 绝大多数漏登记会被**对账**逼出来：新增的变体只要能被某个 `Value` 判出来，
    ///   它就出现在 [`evidence_degeneracy_category`] 那一侧而不在本清单这一侧
    ///   ⇒ `tests/test_review_view_req070.rs` 的前提自检报红。
    /// - 真漏网的只有「新增了变体，且**任何** `Value` 都判不出它」——
    ///   那时它是死代码，判据压根走不到它。
    ///
    /// ⚠️ **它不是穷举**：新增变体既不进本清单、又能被判出来的情形
    /// （例如分类器被改成只在内联判断里认某形状）不在本条的覆盖里，
    /// 已如实登记为 `tests/test_review_view_req070.rs` 模块注释里的已知边界。
    const ALL: [Self; 8] = [
        Self::EmptyString,
        Self::BlankString,
        Self::EmptyObject,
        Self::Null,
        Self::EmptyArray,
        Self::AllDegenerateArray,
        Self::Scalar,
        Self::KeyWithoutValue,
    ];
}

/// 【REQ-075 · AC-003】把 [`EvidenceDegeneracy`] 的**每一个变体** × 一个 `Value` 摊开。
///
/// # 为什么它是 `#[doc(hidden)] pub`
///
/// REQ-075 要消掉守卫对**源码字面量**的依赖（QA 的 M9：保留 `if fingerprint.is_empty() {`
/// 只把指纹换成影子变量 ⇒ 旧守卫全绿）。新判据**直接调用生产的 [`Self::label`]**，
/// 而 `tests/*.rs` 是集成测试，**看不到 `pub(crate)`** —— 与 [`line_head_prefix_of`]
/// 同一处境，故同样用 `#[doc(hidden)] pub`（REQ-073 的结论）。
///
/// ⚠️ **唯一消费者**是 `tests/test_review_view_req070.rs` 里的守卫，不是对外 API。
///
/// # 它买到什么（照实说，只有一格）
///
/// | | 守卫形态 | 认出别名的能力 |
/// | --- | --- | --- |
/// | REQ-070 原样 | 源码文本探针（挂字面量） | ❌ 保留字面量即绕过（M9 实测） |
/// | 本函数 | **行为扫描**：调生产 `label()` | ✅ 任何**改掉 `label()` 输出**的写法都跑不掉 |
#[doc(hidden)]
pub fn evidence_degeneracy_labels(v: &Value) -> Vec<(&'static str, String)> {
    EvidenceDegeneracy::ALL
        .iter()
        .map(|d| (d.category(), d.label(v)))
        .collect()
}

/// 【REQ-075 · AC-003】分类器 [`evidence_degeneracy`] 对 `v` 的结论（只给**类别名**）。
///
/// 与 [`evidence_degeneracy_labels`] 配对使用：后者是**扫描轴**（全部变体），
/// 本函数是**生产实际会走的那一条**。两者**对账**就是「新类别没漏进扫描网」的检查。
#[doc(hidden)]
pub fn evidence_degeneracy_category(v: &Value) -> Option<&'static str> {
    evidence_degeneracy(v).map(|d| d.category())
}

/// 退化元素的**载荷指纹** —— 标签里必须带出来的那部分原始载荷（REQ-065）。
///
/// # 空串 = 「这个元素压根没有可带的内容」
///
/// 只有 JSON 的**规范空形**落在这里：`""` / `null` / `{}` / `[]`。
/// 它们**各只有一个取值**，类别名已经把它说尽了 ——
/// 不存在「两个不同的载荷渲染成同一个串」的可能，**无可藏**。
/// 其余一律**原样带出**：`"   "`、`[1,2]`、`{"table":"","column":"","claim":""}` …
///
/// # 为什么用**紧凑 JSON**，不是把字段拼成人话
///
/// - **键名与结构一次带全** —— `有键无值` 那一类因此看得见**到底有哪些键**；
/// - **纯空白串带引号**（`"   "`）⇒ 空白有多少、是不是空白都看得见；
/// - ⚠️ `serde_json` 把字符串里的**控制字符**转义成 `\n` / `\t` 两字符序列
///   ⇒ **载荷里的换行造不出第二行**，注不进行结构。
///   （同机制的真问题在**非退化** claim 上，那是 REQ-069，本条不碰。）
///
/// # ⚠️ 刻意**不截断**
///
/// REQ-058 的 `全退化数组` / `裸值` 本来就是全量带出的，
/// 本条**不新增**一条截断策略（那会把「带了多少」变成另一处需要论证的取舍）。
fn evidence_payload_fingerprint(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) if s.is_empty() => String::new(),
        Value::Array(a) if a.is_empty() => String::new(),
        Value::Object(o) if o.is_empty() => String::new(),
        other => other.to_string(),
    }
}

/// 这一条证据**退化成什么了** —— 从**原始 `Value`** 上判（AC-002 的根）。
///
/// `None` = 它带着真实的可复核依据，按三段式 / 紧凑 JSON 正常渲染。
///
/// # 判据（不是「是不是空的」，而是「有没有可复核的指向」）
///
/// - `null` / 空对象 / 空数组 / 空串 / 纯空白：**什么都没说**；
/// - 裸标量：不是引用、也不是一句话，FDE 拿它没法复核；
/// - 对象的**每个值**都空 / 纯空白 / null：同样什么都没说；
/// - 非空数组里**每一条**都退化 ⇒ 整条退化（少一层就漏了同一个洞）。
///
/// ⚠️ **刻意不扩大**：带非空值的对象（如 `{"other":"x"}`、`{"claim":5}`）
/// 与含真证据的数组**继续按原样渲染** —— 它们带着信息，
/// 不是这一条要堵的洞。
fn evidence_degeneracy(v: &Value) -> Option<EvidenceDegeneracy> {
    match v {
        Value::Null => Some(EvidenceDegeneracy::Null),
        // ⚠️ 裸标量**不带字面量**进变体：载荷由 `evidence_payload_fingerprint` 统一追加
        // （REQ-065）—— 变体里存一份、标签里再拼一份 = 两个来源，迟早不一致。
        Value::Bool(_) | Value::Number(_) => Some(EvidenceDegeneracy::Scalar),
        Value::String(s) if s.trim().is_empty() => Some(if s.is_empty() {
            EvidenceDegeneracy::EmptyString
        } else {
            EvidenceDegeneracy::BlankString
        }),
        Value::Array(a) if a.is_empty() => Some(EvidenceDegeneracy::EmptyArray),
        Value::Array(a) if a.iter().all(|e| evidence_degeneracy(e).is_some()) => {
            Some(EvidenceDegeneracy::AllDegenerateArray)
        }
        Value::Object(o) if o.is_empty() => Some(EvidenceDegeneracy::EmptyObject),
        Value::Object(o) if o.values().all(is_blank_value) => {
            Some(EvidenceDegeneracy::KeyWithoutValue)
        }
        _ => None,
    }
}

/// 一个值算不算「空」（空串 / 纯空白 / null）。
fn is_blank_value(v: &Value) -> bool {
    v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty())
}

/// **取自 payload / 提案输入的文本**，进渲染层前**一律**走这一个函数。
///
/// # REQ-058 的定界符转义是本函数的**一个分支**，不是它的邻居
///
/// REQ-058 落地的是 `␀` → `␁`（[`DEGENERATE_EVIDENCE_MARK`] →
/// [`DEGENERATE_EVIDENCE_ESCAPE`]）。那个函数**已被吸收**——
/// 两个转义**在同一个 `match` 里**，且本文件**只有这一个 `fn`**（由
/// `test_req069_one_escape_exit_exists` 钉住：函数定义恰好 1 处，且旧名不存在）。
///
/// # ⚠️ 这条不变量**管到哪一格**（写明是为了不被当成「全查过了」）
///
/// 「所有 payload 文本都走它」**仍然不是**结构性保证 —— 它是**当前这 N 个调用点**的纪律。
/// ⚠️ **反例就在眼前**：将来有人给 [`ModelFacing`] 加一个新栏，
/// 若他绕开本函数取 payload 文本（`.clone()` / `.map(str::to_string)` 都算，
/// ⚠️ 不只是 `.map(String::from)` —— REQ-069 的原守卫只认后一种，
/// QA 用 `.clone()` 实测**零红绕过**），
/// 那栏就**不经**本函数 ⇒ 换行立刻能注入行结构。
/// ⇒ **新增 payload 栏时必须显式走 [`escape_payload_text`]**，本函数不替你保证。
///
/// ## REQ-071 之后的检测面（比上一版宽，**但仍有边界**）
///
/// 上一版这里写的是「**这一格没有自动守卫**」。REQ-071 之后不再准确，改成下表：
///
/// | | 手段 | 覆盖 |
/// | --- | --- | --- |
/// | ① | `test_req069_payload_cannot_add_a_line_anywhere` 的**语料** | 10 位（`payload.key` / `properties[].name` / `.column` / `evidence[].claim` / `.table` / `.column` / `cardinality` / `from_type` / `to_type` / `display_target`），外加同文件另一条测试管的 `provenance.agent` |
/// | ② | `test_req069_every_payload_string_read_goes_through_the_escape` —— **投影值判据** | `ItemView` 里**凡还原得回来的字符串字段** + `payload` 任意深度 + `extracted_by` 的每个字符串。**不认字段名** ⇒ 新增字段自动纳入 |
/// | ③ | `tests/test_review_view_req071.rs` 的**逐位见证** | ② 之外**单独立案**的 7 位：L/M/N/O/P/S/T（REQ-069 QA §2 的位号） |
///
/// ⚠️ **仍然没有任何东西覆盖的**（照实列，不是免责套话）：
///
/// - **② ③ 只在 `ItemView` 与 `extracted_by` 两个输入面成立**。
///   将来若有**第三个输入面**（某个新参数直接带字符串进来），②③ 都不会毒到它
///   ⇒ 那一格**仍然是纪律**。不认字段名 ≠ 不认输入面。
/// - **② 只证「投影里没有裸换行」，不证「换行被转义而不是被删掉」** ——
///   「整段吞掉」也能让 ② 绿。后半句是 ① 的 `␊` 断言在管。两条分工，缺一条都漏。
/// - **③ 里的 N 位（`items[].reason`）是防御性的**：当前 [`summarize`] 产出的
///   `ItemVerdict::reason` 只可能是两个固定串 ⇒ 它在 ② 的毒化下**照不到**
///   （实测 M2-N 只有 ③ 杀得住）。`ProposalStatus` 是 `pub`、投影入口接受任意一份
///   ⇒ 它是真实 API 面，但**今天没有活的喂入路径**。照实登记，不假装它是活的。
/// - **一个类型上装不进任意文本的字段**（枚举判别式之类）落在 ② 的毒化之外 ——
///   那是**对的**：它本来就不可能是换行注入面。
///
/// # 它挡住什么（REQ-069 · AC-001）
///
/// 渲染结果里的「行」是**判据的输入**：[`verify_track_separation`] 数行、
/// [`verify_provenance_scope`] 认归属行、`is_item_header_line` 认条目头行
/// —— 它们全是**纯句法**的。实测：claim / 展示名 / key / 列名 / 基数 / 归属名里
/// 的一个换行，**能凭空造出一整行**，那一行每个字节都来自 payload，
/// 却被判据当成渲染器自己打的结构行 ⇒ 产出**理由全错的误报**。
///
/// # 为什么落在**投影边界**、不是 `render_review_text` 里
///
/// 判据不止读渲染文本：它还拿**投影里的字段**去比对
/// （`sourced_markers()`、`display_target`、归属名 …）。
/// 若只在渲染时转义，投影里仍是原文 ⇒ 标记与行**对不上**
/// ⇒ [`verify_track_separation`] 规则④**误报**。
/// 所以必须在「payload 文本进入投影的那一刻」转义：
/// 渲染与判据从此看到**同一个串**。
///
/// # 为什么在**既有归一化之后**才转义（AC-004）
///
/// 转义放在 trim / 空值过滤**之后**（`payload_key`、`from_extracted_by` 的
/// 空值判定都已在前面跑过）⇒ **既有判据看到的判定输入逐字不变**。
/// 若把转义塞进 `payload_str`，`key = "\n"` 会从「空 ⇒ `None`」变成
/// 「非空 ⇒ `Some`」，`sourced_fields` 凭空多一栏 —— 那就是改了既有判据行为。
///
/// # ⚠️ 不可逆 —— 与 REQ-058 的 N-2 **同一类**边界，不是新问题
///
/// payload 字面含 `␊` 与含 `\n` 输出同形。**这不影响本条的不变量**：
/// 两者都**不是**真换行，都造不出行结构。判据问的是「有没有真换行」，
/// 渲染层给的是「有没有真换行」—— 那个问题仍然是精确的。
/// 反过来说：**本函数不声称「渲染结果能还原原始 payload」**，它只声称
/// 「payload 不能改变渲染结果的行结构」。
fn escape_payload_text(s: &str) -> String {
    if !s.contains(DEGENERATE_EVIDENCE_MARK) && !s.contains('\n') && !s.contains('\r') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            DEGENERATE_EVIDENCE_MARK => out.push(DEGENERATE_EVIDENCE_ESCAPE),
            '\n' => out.push(LINE_FEED_ESCAPE),
            '\r' => out.push(CARRIAGE_RETURN_ESCAPE),
            _ => out.push(c),
        }
    }
    out
}

/// 渲染好的这一条证据**带不带可复核的依据**。
///
/// [`ModelFacing::sourced_fields`] 与 [`ModelFacing::sourced_markers`] 都问它，
/// 两边因此永远同判 —— 判据与渲染标记不会各说各话。
pub(crate) fn evidence_line_carries_data(line: &str) -> bool {
    // 「保留标记」= 定界符包起来的退化措辞。转义保证 payload 文本里
    // 不会出现定界符，所以这个判定是精确的。
    //
    // ⚠️ REQ-070：判据与封条共用 [`strip_degenerate_marks`] —— 同一份判定
    // 只写一次，免得只改一边（那边改了这边没改，判决就开始互相矛盾）。
    strip_degenerate_marks(line).is_none()
}

#[cfg(test)]
mod req070_tests {
    //! REQ-070 · [`super::seal_evidence_line`] 的**直接**判据。
    //!
    //! # 为什么这些断言写在 `src/` 里而不是 `tests/`
    //!
    //! 封条的两条**修复**分支在基线上**不可达** —— 渲染器不犯过错时它们恒不触发。
    //! 放在集成测试里就只能靠「跑变异」间接证明，而变异是 QA 的活。
    //! 这里直接喂**两条不忠实的行**（正是 C5 / C6 渲染器会算出来的那种串），
    //! 于是两条规则各自「会不会开火」成了**可执行**的判据。
    //!
    //! # 两条断言各自的方向（都不可省）
    //!
    //! - [`test_req070_seal_restores_a_payload_hidden_degenerate_label`]：**会修**；
    //! - [`test_req070_seal_is_a_no_op_on_faithful_lines`]：**不会乱修**。
    //!
    //! 只写前者 ⇒ 判据可能是「什么都改」（恒真的修补）；
    //! 只写后者 ⇒ 判据可能是「什么都不改」（恒假的修补）。
    //! 「别把『有测试』当『有防护』」的老坑，两头都得堵。
    use super::*;
    use serde_json::json;

    const MARK: char = DEGENERATE_EVIDENCE_MARK;

    /// 一批**刻意生成**的形状 —— 覆盖退化 / 非退化 / 嵌套 / 混合 / 畸形，
    /// **不依赖任何一份外部语料**（AC-001）。
    fn shapes() -> Vec<(&'static str, Value)> {
        vec![
            ("规范空形/空串", json!("")),
            ("规范空形/null", json!(null)),
            ("规范空形/空对象", json!({})),
            ("规范空形/空数组", json!([])),
            ("纯空白", json!("  \t ")),
            ("裸标量", json!(-0.5)),
            ("全退化数组", json!([1, [2], {"c": ""}])),
            ("有键无值/三段式", json!({"table":"","column":"","claim":""})),
            ("非退化/三段式", json!({"table":"t","column":"c","claim":"x"})),
            ("非退化/两段式", json!({"table":"t","column":"c"})),
            ("非退化/半三段式", json!({"table":"t","semantic":"has"})),
            ("非退化/只有claim", json!({"claim":"只有一句话"})),
            ("非退化/claim非串", json!({"claim":5})),
            ("非退化/值是空容器", json!({"table":{},"column":[]})),
            ("非退化/嵌套对象", json!([{"a":{"b":null}}])),
            ("非退化/键带定界符", json!({"\u{2400}k":"v"})),
            ("非退化/值带换行", json!({"k":"a\nb"})),
            ("非退化/数组混真证据", json!([1,"真证据"])),
            ("非退化/深嵌套", json!({"a":{"b":{"c":{"d":"e"}}}})),
        ]
    }

    /// 【AC-001 · 规则 ①】标签藏了载荷 ⇒ 封条把载荷**补回定界符之内**。
    ///
    /// 这一条**不枚举触发形状**：它喂的是「渲染器算出来的那行字」，
    /// 断言的是封条对**那行字**做了什么 —— 语料在不在与此无关。
    #[test]
    fn test_req070_seal_restores_a_payload_hidden_degenerate_label() {
        // C5 算出来的那一行：带定界符、只留类别名、载荷**完全消失**。
        let v = json!({"a": {"b": {}}});
        let hiding = format!("{MARK}嵌套对象{MARK}");

        // 前提自检：这行**真的**没带载荷（否则下面那条断言可能在恒真地上跑）。
        assert!(
            strip_degenerate_marks(&hiding).is_some(),
            "前提：这行应当是保留标记包起来的退化标签"
        );
        assert!(
            !hiding.contains(&evidence_payload_fingerprint(&v)),
            "前提：这行应当**没有**载荷，否则本测试证明不了任何东西"
        );

        let sealed = seal_evidence_line(&v, hiding);

        // 载荷回来了。
        assert!(
            sealed.contains(&evidence_payload_fingerprint(&v)),
            "封条没有把载荷补回去：{sealed:?}"
        );
        // ⚠️ 且**补在定界符之内** —— 补在外面会把结尾定界符挤掉，
        // 那就不再是退化标签，REQ-058 的判决会跟着变（本条不许改判据行为）。
        let inner = strip_degenerate_marks(&sealed)
            .unwrap_or_else(|| panic!("封条把退化标签的定界符挤掉了：{sealed:?}"));
        assert!(
            inner.contains(&evidence_payload_fingerprint(&v)),
            "载荷被补到了定界符**之外** ⇒ 这一行不再被 evidence_line_carries_data 判成退化"
        );
        assert!(
            !evidence_line_carries_data(&sealed),
            "封条补完载荷后，REQ-058 的「退化不带依据」判决翻了"
        );
    }

    /// 【AC-002 · 规则 ②】非退化行**一个载荷原子都没有** ⇒ 封条把它换回忠实呈现。
    ///
    /// C6 算出来的那一行：冒充「它退化了」，而载荷**完全消失**。
    /// 封条不是把载荷**追加**在判决后面 —— 追加会把那句冒充留在输出里；
    /// 它**整行换回**忠实呈现（与基线对同一形状的渲染逐字相同）。
    #[test]
    fn test_req070_seal_replaces_a_payload_free_non_degenerate_line() {
        let v = json!({"table":{}, "column":[]});
        let verdict_only = "值全是空容器";

        // 前提自检：这一行**真的**一个原子都没露（原子 = 键名 + 非空叶子标量）。
        assert!(
            !payload_atoms(&v).is_empty(),
            "前提：这个形状应当有载荷原子（键名），否则本测试证明不了任何东西"
        );
        assert!(
            !payload_atoms(&v).iter().any(|a| verdict_only.contains(a.as_str())),
            "前提：这行应当一个原子都没露"
        );

        let sealed = seal_evidence_line(&v, verdict_only.to_string());

        assert_eq!(
            sealed,
            escape_payload_text(&v.to_string()),
            "冒充的措辞应当被**整行换掉**，而不是把载荷追加在它后面"
        );
        assert!(
            !sealed.contains(verdict_only),
            "冒充「它退化了」的那句措辞还在输出里"
        );
    }

    /// 【AC-001 · 反向】封条对**忠实**的渲染**一个字都不动**。
    ///
    /// # 为什么这一条比它看起来更重要
    ///
    /// 封条挂在**每一个**证据元素的出口上。若它会在基线上乱改行，
    /// 那它改的就是 REQ-058 / REQ-065 的既有判据行为（AC-004 禁止的那类改动）。
    /// 这里对一批**生成**出来的形状断言 `seal(v, render(v)) == render(v)` ——
    /// 即封条是**恒等**的：它只在渲染器真的写了藏载荷的代码时才动手。
    #[test]
    fn test_req070_seal_is_a_no_op_on_faithful_lines() {
        let mut marked = 0usize;
        let mut unmarked = 0usize;
        let mut changed: Vec<String> = Vec::new();

        for (name, v) in shapes() {
            let faithful = render_evidence_entry(&v);
            let sealed = seal_evidence_line(&v, faithful.clone());
            if sealed != faithful {
                changed.push(format!("[{name}] {faithful:?} → {sealed:?}"));
            }
            if strip_degenerate_marks(&faithful).is_some() {
                marked += 1;
            } else {
                unmarked += 1;
            }
        }

        // 前提自检：这批形状**真的**两头都踩到了（恒真判据的老坑）。
        assert!(
            marked >= 5 && unmarked >= 5,
            "前提：这批形状应当同时覆盖退化（{marked}）与非退化（{unmarked}）两条路，\
             否则「封条不动它」可能只是因为它压根没被考验"
        );
        assert!(
            changed.is_empty(),
            "封条在**忠实**渲染上动了手 ⇒ 它改的是既有判据行为：\n{}",
            changed.join("\n")
        );
    }
}
