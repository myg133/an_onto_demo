//! **ACP 抽取 adapter** —— 让 `ontok` 能真的把抽取外包给 `mcode acp` 子进程。
//!
//! ADR-002 D3：KH Agent 走 **Agent Client Protocol**，
//! JSON-RPC 2.0 over stdio，由 `ontok` 以**客户端**身份拉起子进程对话。
//!
//! ```text
//! ontok ──stdin/stdout(JSON-RPC)──▶ mcode acp
//!   │  initialize ────────────────────────────▶ 取 agentInfo（身份）
//!   │  session/new ──────────────────────────▶ 取 sessionId
//!   │  session/prompt ───────────────────────▶ ★正文在流式通知里（accumulator）
//!   ◀─────────────────────────────────────────
//! ```
//!
//! # 模块分层（三层，各管各的）
//!
//! | 模块 | 职责 | 依赖 |
//! | --- | --- | --- |
//! | [`protocol`] | JSON-RPC 编解码 + 流式累积 | **零 I/O**，纯函数 |
//! | [`process`] | 子进程生命周期 + 专用线程 | `std::process` / `std::io`，**零 tokio** |
//! | 本模块 | [`ExtractionAgent`] 实现、prompt 构建、响应解析、能力声明 | 上面两层 |
//!
//! 为什么这么切：协议知识全在字节与形状里，子进程只是搬运工。
//! 混在一起就只能靠「真的起一个 `mcode acp`」测协议 —— 而那 11.7 秒、
//! 依赖登录态、CI 不可复现。切开之后，协议层用**录好的真实字节**测（见 `tests/`），
//! adapter 层用 [`ScriptedTransport`] 测，**两者都不起进程**。
//!
//! # ★ 同步/异步：本 adapter **一个 `block_on` 都不写**
//!
//! 背景事实（逐行核实过）：
//!
//! | 事实 | 位置 |
//! | --- | --- |
//! | `ontok-propose` 是 `#[tokio::main] async fn main()` | `src/bin/ontok-propose.rs:39` |
//! | `orchestrate(...)` 在 **async main 内部**被调用 | `src/bin/ontok-propose.rs:229` |
//! | [`ExtractionAgent`] 的四个方法**全是同步** | `src/agent/mod.rs:310-364` |
//!
//! ADR-002 的 **N3** 记的坑就在这儿：同步方法要 I/O 就得 `block_on`，
//! 而 `block_on` 在已有 runtime 上下文里**直接 panic**。
//!
//! ## 选的是 BA 的 A 与 B 的**合成**：`process` 模块的线程模型取 A，I/O 取 B
//!
//! - **A 的机制** —— 子进程活在一根**专用线程**上，`extract_group` 通过 channel 与它往返；
//! - **B 的 I/O** —— `std::process` + `std::io`，**不补 tokio feature，一个依赖都不加**。
//!
//! 为什么不「照 A 做到底」，三条理由：
//!
//! 1. **A 自己也没能真的消灭 `block_on`。** A 要在专用线程里起 tokio runtime，
//!    再用 `block_on` 驱动它 —— `block_on` 这个词照样出现在代码里，
//!    只是「不在已有 runtime 上下文里」。本方案**一个字都不写**，
//!    于是 BA 要求的那条不变量（「不得出现 block_on」）可以**用一条 grep 证明**。
//! 2. **A 要补 `process` / `io-util` / `time` 三个 tokio feature**，
//!    换来的只是把同一段阻塞逻辑写成异步形式。ACP 是一条 stdio 上的
//!    **严格串行**请求/响应流，没有并发可利用，异步在这里是纯开销。
//!    不补 feature ⇒ **不用改 `Cargo.toml`**，也就不用碰依赖。
//! 3. **专用线程解决的是「子进程管理」这一个真问题**：拉起进程、读写 stdio、
//!    捏超时、收尾清孤儿，全都搬到了**不是 runtime worker** 的线程上 ——
//!    子进程这一侧既不借 runtime 的手，也就不需要 runtime 上下文。
//!    ⚠️ **它没有解决、也解决不了「不阻塞调用方」**：一次 `extract_group` 要阻塞
//!    十几秒（实测一轮往返 11.7 秒），这段等待**发生在调用方线程上**
//!    （QA-A 实测：传输模拟 300ms 时，`extract_group` 占用**调用方线程** 300.55ms）。
//!    从 `#[tokio::main]` 的 async main 调进来，占住的就是那根 tokio worker ——
//!    同 worker 上的其它 future 一样会饿死。
//!    **别把这根线程当成「可以放心在 async 上下文里跑」的许可证。**
//!    今天影响为 0，只是因为 adapter 还没接线：`build_agent` 的 `other` 分支
//!    对 `acp` 直接 `bail!`（`src/agent/factory.rs:91`），压根到不了这条路径。
//!
//! 「为什么这根线程是合法的」在 [`process`] 模块的 `ThreadTransport` 上有完整论证。
//!
//! # ★ 能力声明：**静态**，只声明 `reports_usage`
//!
//! 这是本 REQ 唯一需要论证的决定，先给结论，再给依据。
//!
//! ## 结论
//!
//! | 能力 | 声明？ | 理由 |
//! | --- | --- | --- |
//! | [`CAPABILITY_REPORTS_USAGE`] | ✅ 声明 | 我们**确实**上报 ACP 的 `usage_update`（F-2），数据源现成 |
//! | [`CAPABILITY_REBUILDS_PROPOSAL`] | ❌ **不声明** | 候选**承载不下**提案 —— 见下 |
//! | [`CAPABILITY_PRODUCES_PROPOSAL`] | ❌ **绝不声明** | `items` 不是本实现产的（规则管线产的）。声明它**就是撒谎** |
//!
//! ## ★ 为什么「静态」是硬要求（不是风格）
//!
//! 这个实现曾经把「能不能重建」存成一个**运行时**状态（`rebuildable`），
//! 由 `extract_group` 填、由 `capabilities()` 读。那个形状**结构性**地坏：
//!
//! ```text
//! rebuildable 初始 None
//!   → 唯一写入点在 extract_group 内（抽取**之后**）
//!   → 能力闸门在 orchestrate 入口就执行（抽取**之前**）
//!   ⇒ 该分支经 orchestrate 这唯一生产入口不可达
//! ```
//!
//! **判据先于它要核对的数据产生**，就只能读一个「还没发生」的状态。
//! 那样的判据要么恒假（就是这个形状），要么恒真（下一个更糟的形状）。
//! 所以现在 [`AcpAgent::capabilities`] 是个**不读任何字段的纯函数** ——
//! 能力是实现的**固有属性**，不随答案翻转。
//!
//! ### 不变量（写在这里，免得后来人以为是遗漏）
//!
//! > **能力标签描述「这个实现是否具备某能力」，不描述「这一份答案长什么样」。**
//!
//! 让标签跟着答案变，得到的是 REQ-019 点名的**静默假溯源**：
//! 同一份登记、同一套代码，提案能不能产出取决于**这次模型答了什么**。
//! 这与 `source` 字段在本项目栽过的跟头（「声明了却没用上」）是**镜像**：
//! 那次声明了却不用，这次答案一变声明就变 —— 两种都是「标签与事实脱钩」。
//!
//! ### 「补齐字段就自动变对」是必要不充分
//!
//! 曾经的注释说本实现「有一处可以随协议演进自己变对」。**那句是错的**：
//! 补齐 `origin` + `rejected_comments` 只让**判据**变对，
//! 而「该不该声明」是**人的签字**，不是某字段到齐就自动发生的。
//! 真要签字，得把 [`AcpAgent::capabilities`] **显式**改成硬编码的
//! `vec![REBUILDS]` —— 那是一次**可以评审**的改动。
//!
//! ## 逐份答案的核对在**另一处**（抽取后守卫）
//!
//! 「这份答案承不承载得下重建契约」是一个**逐份**的问题，由
//! [`crate::agent::assemble::contract_gaps`] 在**抽取之后**回答，
//! 编排在重建路径上**每组抽完立刻核对**、不合契约就显式失败。
//! 这两件事曾经长在一起（判据住在 adapter 里、能力标签由它推导），
//! 现在**拆开**：能力静态、守卫抽取后。时机对不上，判据就必然是死的。
//!
//! ## 为什么 `rebuilds_proposal` 也是撒谎 —— **不是**因为「格式不稳定」
//!
//! BA 的倾向是「只要你能把 ACP 的输出解析成合法的 `CandidateType`」。
//! 本 REQ 的结论比那更严格，而且**更强**：
//!
//! > 即使模型每次都吐出**完美合法**的 JSON，`CandidateType` 里仍有**两个必填字段**
//! > 是「模型在黑盒里无法报告、而本 adapter 也无法凭空知道」的事实。
//! > 所以问题不是「解析稳不稳」，是**这个契约本身对 LLM 不可满足**。
//!
//! 那两个字段（重建路径 `assemble::entity_item` / `rebuild_outcome` 都对它
//! **`bail!`**）：
//!
//! | 字段 | 它是什么 | 为什么填不出来 |
//! | --- | --- | --- |
//! | `property.origin` | 名字的来源：`Comment` / `WordList` / `Relation` | 这三个值描述的是**规则管线的三条命名路径**（`propose::build_properties`）。模型在黑盒里起了个名字，**没有任何渠道**告诉我们那是照抄注释、从关系推的、还是自己造的 |
//! | `type.rejected_comments` | REQ-005 **质量门槛**的拒用列数 | 本 adapter **根本没有实现那个门槛** —— 它是规则管线的一部分 |
//!
//! 编一个值填进去会怎样，逐一算清楚：
//!
//! - `origin` 随便填 ⇒ `stats.props_via_comment` / `_wordlist` / `_relation`
//!   三个计数全是编的。`assemble` 的模块头原话：真实源库 964 个属性里
//!   **579 个来自列注释**，写错就是 stats.json 在说谎，而 stats.json 是
//!   E2E 判读器读的东西。
//! - `rejected_comments` 填 `Some(0)` ⇒ `stats.props_comment_rejected = 0`，
//!   而真实那次抽取是 **6**。`assemble` 的注释写着「填个默认值发出去 =
//!   **发一份数据在说谎的提案**」。
//!
//! **两处都不是「差一点对」，是全错。** 而它们都不在产物里显眼的位置
//! —— 混在一堆看起来合理的数字中间，**没人查得出它错**。
//!
//! ## 为什么不声明任何产出能力（这是**正确**的行为，不是妥协）
//!
//! 于是 [`ExtractionAgent::capabilities`] 返回**只有** `reports_usage`，
//! [`crate::agent::orchestrate`] 的 `require_proposal_producing` 会
//! **在入口拒绝整轮**，并说清两条出路（该错误信息本身写得很好）。
//!
//! 这个拒绝是**本 REQ 最正确的一个结果**，因为它让缺口**可见**：
//!
//! - 若声明了 `rebuilds_proposal` ⇒ 闸门放行 ⇒ 抽取后守卫在**第一组**不合契约时
//!   就拦下（比 REQ-023 那版「91 组跑完才在阶段 4 炸」早得多），
//!   **钱花完了，提案还是没有**。
//! - 现在不声明 ⇒ **入口就拒**，一分钱不花，错误信息直接指向那几个字段。
//!
//! REQ-022 的验收结论早就点破了这件事：
//!
//! > 「RN-2 加 adapter 后，闸门会从『防呆』变成『逼你签字』。」
//!
//! 这次**没有签字**。可执行的证据有两条：
//! `tests/test_acp_extraction.rs::acp_output_fails_the_rebuild_contract_closed`
//! 拿本 adapter **自己产出的候选**喂给 `assemble::rebuild_outcome`，断言它按设计报错；
//! `tests/test_acp_extraction.rs::capabilities_are_static_and_never_depend_on_the_answer`
//! 钉住「能力不随答案翻转」这条不变量。论证因此是**可执行的**，不是文档里的一句主张。
//!
//! ## 什么时候这个结论会变（写清楚，免得后来人以为是遗漏）
//!
//! 只要 ACP（或未来别的协议）能给出**结构化的、可验证的**来源信息，
//! 或者本 adapter 愿意**明确地**采用一条命名来源策略（并把那条策略
//! 的代价写进 `stats` 的口径说明），`rebuilds_proposal` 就能如实声明 ——
//! 但那是一次**显式的签字**：改 [`AcpAgent::capabilities`] 的硬编码返回值，
//! 并让逐项守卫对真实答案跑绿。**没有「自动变对」这条路。**
//!
//! # ⚠️ ADR-002 · N5：`agentInfo.version` 是 **mcode 二进制版本**，不是模型版本
//!
//! 实测：握手回的是 `{"name":"minimax-code","title":"MiniMax Code","version":"0.5.5"}`。
//! `0.5.5` 是驱动进程自己的版本。把它当模型版本记进 [`AgentIdentity`]，
//! 于是 §8.4 的「agent 与其版本在整个抽取会话内锁定」锁的是**一个与抽取效果无关的数字** ——
//! 模型换了一版，`extracted_by.version` **一个字都没变**，diff 归因全错。
//!
//! 所以本 adapter：
//!
//! - 如实把 `agentInfo.version` 记进 [`AgentIdentity::version`]（**它就是那个值**，不许编）；
//! - 另加 [`AcpAgent::model_note`] 记录**会话级**的配置快照
//!   （`session/new` 回的 `configOptions`），让「这一轮到底用的什么配置」
//!   有一处可查的地方；
//! - 要真正**锁死**模型，需要在建会话时固定 `configOptions.model` 并记录 ——
//!   **请求侧那个参数的形状本次没有实测**（BA 的探测只抓了响应侧），
//!   所以本 REQ **不发**那个字段。发一个没量过的字段，是拿协议错赌字段对。
//!
//! # 怎么造
//!
//! ```no_run
//! use ontok::agent::acp::{AcpAgent, AcpConfig};
//! use ontok::agent::ExtractionAgent; // trait 的方法要 trait 在作用域里
//!
//! # fn main() -> anyhow::Result<()> {
//! // 真的拉起 `mcode acp`（需要登录态）
//! let agent = AcpAgent::connect(AcpConfig::default(), 1)?;
//!
//! // 身份来自**真实握手**，不是硬编码默认值（§8.4）
//! println!("{:?} v{}", agent.identity().agent, agent.identity().version);
//!
//! // 能力声明（见本模块头）：只报 reports_usage
//! println!("{:?}", agent.capabilities());
//! # Ok(()) }
//! ```
//!
//! 测试用 [`AcpAgent::with_transport`] 注入 [`ScriptedTransport`] ——
//! **不起进程、不需要登录、毫秒级**。

pub mod parse;
pub mod process;
pub mod protocol;

pub use parse::{build_prompt, can_rebuild_proposal, contract_gaps, parse_extraction};
pub use process::{
    AcpConfig, AcpProcess, AcpTransport, Handshake, ScriptedTransport, ThreadTransport,
    DEFAULT_TIMEOUT,
};
pub use protocol::{
    AgentInfo, Incoming, NewSessionResult, PromptAccumulator, RpcError, RpcRequest, RpcResponse,
    TurnOutcome, UpdatePayload, UsageSample, STOP_REASON_END_TURN,
};

use std::sync::Mutex;

use anyhow::{bail, Context, Result};

use super::group::{ExtractionGroup, GroupExtraction, GroupFailure};
use super::registry::{is_vague_version, AgentIdentity};
use super::{
    ExtractionAgent, UsageReport, CAPABILITY_REPORTS_USAGE,
};

/// **ACP 抽取 adapter** —— [`ExtractionAgent`] 的第二个实现。
///
pub struct AcpAgent {
    /// ⚠️ **一把 `Mutex` 不是为了并发安全，是为了「一次只有一个调用」写在类型上。**
    ///
    /// [`ExtractionAgent`] 的方法全是 `&self`（MSRV 下没有 dyn-safe 的
    /// `async fn in trait`，也没有 `async-trait` 依赖），
    /// 而 [`AcpTransport::prompt`] 要可变（它推进 request id、累积下一轮的响应）。
    /// 编排是**单线程顺序**跑 91 个组的，所以**不存在真正的竞争** ——
    /// 但把可变的会话状态裸放在一个共享引用后面，
    /// 等于把「此刻只能有一次往返」这个事实寄托在调用方身上。
    /// 有了这把锁，这个事实由类型保证。
    transport: Mutex<Box<dyn AcpTransport>>,
    identity: AgentIdentity,
    /// 会话级的配置快照（`session/new` 回的 `modes` / `configOptions`）——
    /// ADR-002 · N5 的留痕处，见模块头。
    model_note: Option<String>,
    /// 每组一条最新用量（`usage()` 是 `&self` 上的方法，所以要一把锁）。
    usage: Mutex<Vec<UsageReport>>,
    /// 上一次见过的**会话累计**用量 —— 换算成「本组增量」需要它。
    ///
    /// `None` = 这一轮之前从没收到过 `usage_update`（首组）。
    usage_baseline: Mutex<Option<UsageSample>>,
    /// 本实现自报的失败组（[`ExtractionAgent::failed_groups`] 被编排真的读，
    /// 不实现它就是一条死路径）。
    failed: Mutex<Vec<GroupFailure>>,
}

impl AcpAgent {
    /// 真的拉起 `mcode acp` 并握手。
    pub fn connect(cfg: AcpConfig, round: i32) -> Result<Self> {
        let transport = ThreadTransport::spawn(cfg)?;
        Self::with_transport(Box::new(transport), round)
    }

    /// 用任意传输造一个 adapter（测试 / 回放）。
    pub fn with_transport(mut transport: Box<dyn AcpTransport>, round: i32) -> Result<Self> {
        let h = transport.handshake()?;
        let transport = Mutex::new(transport);
        let identity = identity_from_handshake(&h, round)?;
        let model_note = model_note_from(&h);
        Ok(Self {
            transport,
            identity,
            model_note,
            usage: Mutex::new(Vec::new()),
            usage_baseline: Mutex::new(None),
            failed: Mutex::new(Vec::new()),
        })
    }

    /// 会话级配置快照（ADR-002 · **N5**）。
    ///
    /// `agentInfo.version` 是**驱动二进制**的版本，与模型无关；
    /// 想知道「这一轮用的什么模型/配置」，唯一有据可查的地方是这里 ——
    /// 它是 `session/new` 回的、服务端**承认**的配置项。
    pub fn model_note(&self) -> Option<&str> {
        self.model_note.as_deref()
    }

    /// 这个实现**结构性**地承载不下提案 —— 一句话理由，
    /// 而不是「某一次答案的实测结果」。
    ///
    /// 曾经它读运行时状态，于是答出两件**互相矛盾**的事：
    /// 「本实现不具备产出能力」（能力标签）与「上一次恰好承载得下」（本方法）。
    /// 现在判据只有一处、时机只有一处：**抽取之后**由
    /// [`crate::agent::assemble::contract_gaps`] 逐项核对，
    /// 而本实现**今天就该静态地不声明** `rebuilds_proposal`
    /// （[`AcpAgent::capabilities`] 是纯函数，见那里的不变量）。
    ///
    /// 保留这个方法是让「为什么不声明」**可被程序问出来**，而不只是被读到。
    pub fn contract_gap_report(&self) -> Vec<String> {
        vec![
            "本 adapter **今天填不出**重建所需的字段：`property.origin` 与 \
             `type.rejected_comments`（另见 assemble::contract_gaps 的完整判据，\
             它现在还覆盖 `nullable` 与 `links[].evidence`）。\n\
             \n\
             这不是「某一次答案没填好」，是**结构性的**（论证见本模块头）：\n\
             · `origin` 的三个取值是规则管线 build_properties 的三条分支标签，\
               Relation 那支要 known_tables + TableNames 重跑 infer_link 才算得数 —— \
               adapter 手上两者都没有，**没人能验证**，不是「模型不知道」；\n\
             · `rejected_comments` 统计的是 REQ-005 门槛拒用的列，而**被拒的列不会变成\
               任何属性候选** —— 分子分母不在同一集合，信息论上不可能。"
                .to_string(),
        ]
    }
}

fn identity_from_handshake(h: &Handshake, round: i32) -> Result<AgentIdentity> {
    let version = h
        .agent_info
        .version
        .clone()
        .filter(|v| !v.trim().is_empty())
        // ⚠️ ADR-002 · N5：这是 **mcode 二进制**的版本，不是模型版本。
        // 如实记，不加工，也不假装它是模型版本 —— 加工出来的版本号
        // 会让 §8.4 的版本锁定变成一个**与抽取效果无关**的锚点。
        .context(
            "ACP 的 agentInfo 里没有 version —— §8.4 要求 `extracted_by` 记具体版本，\
             编一个（如 \"unknown\"）就是假溯源。**宁可拒绝这次抽取。**",
        )?;
    if is_vague_version(&version) {
        bail!(
            "ACP 报的 agentInfo.version 是 {version:?} —— 模糊版本。\
             §8.4 要求记具体版本号，否则 diff 变了分不清是「本体演进」还是「agent 换人」。\
             \n\
             注意这个字段是 **mcode 二进制**的版本（ADR-002 · N5），不是模型版本。"
        );
    }
    if h.agent_info.name.trim().is_empty() {
        bail!("ACP 的 agentInfo.name 是空的 —— 抽取 agent 必须有可追溯的名字");
    }
    Ok(AgentIdentity {
        agent: h.agent_info.name.clone(),
        version,
        round: round.max(1),
    })
}

fn model_note_from(h: &Handshake) -> Option<String> {
    let cfg = h.session.config_options.as_ref()?;
    if cfg.is_null() {
        return None;
    }
    Some(format!(
        "会话 {} 的 configOptions（服务端自报）：{}",
        h.session.session_id,
        serde_json::to_string(cfg).unwrap_or_else(|e| format!("<序列化失败：{e}>"))
    ))
}

impl ExtractionAgent for AcpAgent {
    /// 身份来自**真实握手**的 `agentInfo`（§8.4），不是硬编码默认值。
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }

    /// ★ 只声明 [`CAPABILITY_REPORTS_USAGE`]。完整论证见**本模块头**。
    ///
    /// # ★ 本方法是**静态**的：能力不随答案翻转
    ///
    /// 这是一个 `&self` 上的**纯函数** —— 它不读任何字段，**没有任何运行时状态**。
    /// 曾经它读一个 `rebuildable: Mutex<Option<bool>>`（由 `extract_group` 填），
    /// 于是长成 `if rebuildable == Some(true) { push(REBUILDS) }`。那个形状是坏的，
    /// 而且是**结构性**地坏：
    ///
    /// ```text
    /// rebuildable 初始 None
    ///   → 唯一写入点在 extract_group 内（抽取**之后**）
    ///   → 能力闸门在 orchestrate 入口就执行（抽取**之前**）
    ///   ⇒ 该分支经 orchestrate 这唯一生产入口不可达
    /// ```
    ///
    /// 判据**先于数据产生**，就只能读一个还没发生的状态 ——
    /// 这样的判据要么恒假（就是今天这样），要么恒真（下一个更糟的形状）。
    ///
    /// # 不变量（不许被「优化」掉）
    ///
    /// > **能力标签描述的是「这个实现是否具备某能力」，不是「这一份答案长什么样」。**
    /// > 一旦让标签跟着答案变，就得到 REQ-019 点名的**静默假溯源**：
    /// > 同一份登记、同一套代码，提案能不能产出取决于**这次模型答了什么**。
    ///
    /// 这正是 `source` 字段在本项目栽过的跟头（「声明了却没用上」）的**镜像**：
    /// 那次是声明了却不用，这次是**答案一变、声明就变**。两种都是「标签与事实脱钩」。
    ///
    /// 「补齐 `origin` + `rejected_comments` 就会自动变对」是**必要不充分** ——
    /// 只有把本方法改写成硬编码的 `vec![REBUILDS]` 才算签字，而签字是**人的决定**，
    /// 不是某个字段到齐就自动发生的。
    ///
    /// 那份答案**逐项**核不承载契约，是**另一个问题**，由
    /// [`crate::agent::assemble::contract_gaps`] 在**抽取之后**回答（守卫）。
    ///
    /// # 绝**不**声明 [`CAPABILITY_PRODUCES_PROPOSAL`]
    ///
    /// 那个标签的意思是「阶段 3+4 的 `items` **就是**规则管线的产物」。
    /// 本 adapter 走的是**重建**路，`items` 由 `assemble::rebuild_outcome`
    /// 从候选搬运而来，不是规则管线的产物。声明它就是 REQ-019 点名的
    /// **静默假溯源**。
    fn capabilities(&self) -> Vec<String> {
        // 我们确实会上报 ACP 的 `usage_update`（F-2），所以这一条是真的。
        // 声明它的**后果**是编排强制要求设预算（`require_budget` 的 fail-closed）
        // —— 对一个真的在烧 token 的 agent，这正是要的。
        vec![CAPABILITY_REPORTS_USAGE.to_string()]
    }

    /// 阶段 1：抽一组 —— **每组一次 `session/prompt`**（规格 §7.1）。
    fn extract_group(&self, group: &ExtractionGroup) -> Result<GroupExtraction> {
        let prompt = build_prompt(group);
        let turn = self
            .transport
            .lock()
            .expect("transport 锁")
            .prompt(&prompt)?;

        // ── 先记账，再解析 ──
        //
        // 顺序不是随意的：模型**已经**为这一组花过钱了。若先解析、解析失败
        // 就直接 return，那笔钱**永远不进账** —— 成本闸门于是少算，
        // 下一组照跑。**漏记的钱比报错更难发现**（账是平的，看不出来）。
        self.record_usage(group, &turn)?;

        let extraction = parse_extraction(&turn.text, group)?;

        Ok(extraction)
    }

    /// 本实现自报的失败组（编排真的读这个方法，不实现就是死路径）。
    fn failed_groups(&self) -> Vec<GroupFailure> {
        self.failed.lock().expect("failed 锁").clone()
    }

    /// 把 ACP 的 `usage_update`（F-2）换算成 REQ-022 的 [`UsageReport`]。
    ///
    /// # 语义：**每次尝试一条增量**，全部相加等于会话真实总量
    ///
    /// ACP 报的是**会话累计**（[`UsageSample::delta_since`] 的完整论证），
    /// 而编排要的是**每条增量**（[`UsageReport::used`] 的契约）。
    /// 直接报会话累计会算出 91 倍 —— 闸门在 `13993/91 ≈ 154` 处就停下，而账是错的。
    /// 同组重试会记**多条**（`attempt` 不同），编排**每条都算**（REQ-050）。
    fn usage(&self) -> Vec<UsageReport> {
        self.usage.lock().expect("usage 锁").clone()
    }
}

impl AcpAgent {
    /// 记一次抽取的用量，并推进基线。
    ///
    /// # 没有 `usage_update` 就**报错**，不报 0（fail-closed）
    ///
    /// 「这一组花了多少」如果答不上来，选项只有三个：报 0（撒谎）、
    /// 不报这一组（漏算，闸门形同虚设）、**让这一组失败**。
    /// 选第三个：钱已经花掉了，但**账目缺口会显式炸出来**，
    /// 而不是变成一份看起来平的账。
    fn record_usage(&self, group: &ExtractionGroup, turn: &TurnOutcome) -> Result<()> {
        let sample = turn.usage.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "ACP 报告了一组 {:?} 的抽取，但**全程没有收到 `usage_update`** —— \
                 这一组花了多少，客户端**无从判断**。\n\
                 \n\
                 为什么必须报错而不是报 0 / 不报：\n\
                 · 报 0 = 把「不知道」说成「免费」，那是账目造假；\n\
                 · 不报这一组 = 成本闸门少算它，**漏掉的闸门是看不见的闸门**，\
                   91 组漏掉一半，闸门照样形同虚设；\n\
                 · 报错 = 缺口显式、进 `failed_groups`、被人看见。\n\
                 \n\
                 这一轮收到了 {chunks} 条 chunk（正文 {chars} 字符），\
                 但其中没有任何 `usage_update`。\
                 对端若改了协议形状（ADR-002 · F-2 的数据源），\
                 请同步更新 `protocol::UpdatePayload::UsageUpdate`。",
                group.group_id,
                chunks = turn.chunks,
                chars = turn.text.len(), // 真实通知数拿不到就不编：TurnOutcome 不带它（真实值在 protocol 层的累积器里），正文字符数不冒充它
            )
        })?;
        let baseline = self.usage_baseline.lock().expect("usage_baseline 锁").clone();
        // ★ REQ-050：按「该组已上报几条」编号，把重试的每一笔分成不同的键。
        // 一直报 0 会让重试的第一笔被下一笔顶掉 —— 闸门按偏低的数放行。
        let attempt = {
            let log = self.usage.lock().expect("usage 锁");
            log.iter()
                .filter(|r| r.group_id == group.group_id)
                .count() as u32
        };
        let report = sample.delta_to_report(baseline.as_ref(), &group.group_id, attempt);
        self.usage.lock().expect("usage 锁").push(report);
        *self.usage_baseline.lock().expect("usage_baseline 锁") = Some(sample);
        Ok(())
    }
}