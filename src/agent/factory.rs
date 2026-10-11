//! **按登记造实现** —— 让「换一个抽取 agent」不再需要改生产入口。
//!
//! # 缺陷 F2 的正面回答：接第二个 agent 要改几处
//!
//! REQ-013 建好 trait 之后，生产入口仍然是
//! `RuleBasedAgent::new(identity, names)` + `.propose(...)` —— 构造**具体类型**、
//! 调用**固有方法**。于是「接第二个实现」要改两处生产代码，而上一批注释里
//! 「避免每次接协议都要重写一遍编排」这句话**并没有兑现**。
//!
//! 本文件把「用哪个实现」的决定收到一处：**登记里写的 `protocol` 是什么，
//! 就造哪个实现**。接第二个实现从此是「实现 trait + 在这里加一条分支」，
//! 生产入口（[`crate::bin`] 侧的 `ontok-propose`）**一行都不用改**。
//!
//! # 为什么按 `protocol` 分派，而不是按 agent 名字
//!
//! `protocol` 是 registry 里**已经存在的字段**，且「协议是数据不是代码」
//! （`registry.rs` 模块头）。按它分派意味着将来选了协议、加了 adapter，
//! 这里加一条分支就行，**不用改 trait、不用改登记表结构**。
//! 按 `agent` 名字分派则会把「自研/外部」这种**命名约定**焊进代码。
//!
//! # ★ fail-closed：找不到登记的实现就报错，绝不回落（缺陷 F2 的另一半）
//!
//! 沿用 [`crate::config`]（`require_env` 不提供默认值）与
//! `introspect.rs` 的同一纪律：**缺配置必报错，不静默回落**。
//!
//! 为什么不许回落：回落意味着「我登记了一个 `protocol = "a2a"` 的 agent，
//! adapter 还没写，于是系统悄悄用规则实现跑完了这一轮」——
//! 提案正常产出、统计正常、**没人知道这一轮其实没走登记的那个 agent**。
//! 而 §8.4 的全部价值（版本锁定、diff 归因）都建立在
//! 「`extracted_by` 记的就是真跑的那个」之上。回落一次，那份溯源就是假的，
//! 而且是**静默**的假 —— 这正是本项目吃过七次亏的那一类。
//!
//! 所以本函数返回 `Result`，找不到实现时 [`anyhow::bail!`]，
//! **没有任何 `unwrap_or_else(|_| 造规则实现)` 之类的回落分支**。

use anyhow::{Context, Result};
use std::collections::HashMap;

use super::acp::{AcpAgent, AcpConfig};
use super::llm::{LlmAgent, LlmConfig};
use super::registry::{AgentIdentity, AgentRegistration};
use super::rule_based::RuleBasedAgent;
use super::ExtractionAgent;
use crate::extract::names::TableNames;

/// **按登记造出抽取 agent 的实现**（`Box<dyn ExtractionAgent>`）。
///
/// # 参数
///
/// - `registration` —— agent-registry 的**登记记录**。分派依据是它的
///   `protocol` 字段。
///
///   ⚠️ **身份（agent + 锁定版本）的来源按协议分支不同**（别把这里读成
///   「都来自登记」）：
///
///   | 分支 | [`ExtractionAgent::identity`] 里的 agent/version 来自 |
///   | --- | --- |
///   | `inproc` | **登记**（[`AgentIdentity::from_registration`]）|
///   | `acp` | **握手**（`mcode acp` 自己报的 `agent` / 版本）|
///
///   两条路的 `round` 都由本函数的 `round` 参数给（轮次是**运行**的属性）。
///   登记名与握手名不一致时**不报错**（别名是合法的），只由
///   [`identity_cross_check`] 出告警；原因见该函数的文档。
///
/// - `names` —— 表名翻译表。规则实现的**知识资产**（REQ-003），
///   协议实现用不上时也不碍事（本函数只负责造，**不负责丢**）。
/// - `round` —— 第几轮抽取（§8.4）。轮次是**运行**的属性不是 agent 的属性，
///   所以由调用方给，不从登记里读。
///
/// # 返回
///
/// `Box<dyn ExtractionAgent>` —— 调用点拿到的**只有一个 dyn**，
/// 因此「编排循环」里不可能出现具体类型分支（那正是 F2 要拆掉的东西）。
///
/// # 找不到实现 ⇒ 报错，不回落
///
/// 登记里的 `protocol` 没有对应实现时 [`anyhow::bail!`]。
/// 模块头说明了为什么这条必须是 fail-closed。
pub fn build_agent(
    registration: &AgentRegistration,
    names: TableNames,
    round: i32,
) -> Result<Box<dyn ExtractionAgent>> {
    // 登记本身先过一遍校验：空 agent / 空版本 / 「latest」这类模糊版本
    // 在造实现之前就该被挡住（§8.4）。与 `TableNames::load` / `config::require_env`
    // 同一纪律 —— 坏配置早失败，好过造出一个「能跑但溯源是假的」agent。
    registration
        .validate()
        .with_context(|| format!("agent 登记 {:?} 未通过校验", registration.agent))?;

    match registration.protocol.as_str() {
        // `inproc` = 进程内规则实现（REQ-013 的第一个实现）。
        //
        // `endpoint` 对它没有意义，但**不因为「没用上」就报错** ——
        // 登记里带了端点不构成矛盾（同一实现登记两次，第二次填了端点）。
        // 真正会报错的是**有端点却没有对应实现**，见下面那一支。
        "inproc" => Ok(Box::new(RuleBasedAgent::new(
            AgentIdentity::from_registration(registration, round),
            names,
        ))),

        // ACP = `mcode acp` 子进程协议 adapter（ADR-002 D3）。
        //
        // ⚠️ **端点从登记的 `endpoint` 字段来，不写死**：可执行文件在哪、
        // 带什么参数，是**部署事实**，写死在代码里等于「换个环境就跑不起来，
        // 而报错会指向代码而不是环境」。
        "acp" => {
            let endpoint = registration
                .endpoint
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .with_context(|| {
                    format!(
                        "agent {:?} 登记的协议是 \"acp\"，但 endpoint 是空的 —— \
                         ACP 要拉起一个子进程，可执行文件与参数是**登记的事实**，\
                         不写死在代码里（换个部署环境就得改代码）。\n\
                         请在登记里填形如 \"mcode acp\" 的 endpoint。",
                        registration.agent
                    )
                })?;
            // endpoint 记成一条命令行：第一个词是可执行文件，其余是参数。
            // 不引入第二套「端点语法」—— 那会让登记表多一种要人记的格式，
            // 而这一种读得懂、也说得清（`ps` 里就是这个样子）。
            let mut words = endpoint.split_whitespace();
            let program = words.next().with_context(|| {
                format!(
                    "agent {:?} 的 endpoint {:?} 里没有可执行文件名",
                    registration.agent, endpoint
                )
            })?;
            let args: Vec<String> = words.map(str::to_string).collect();
            let cfg = AcpConfig {
                program: program.to_string(),
                args,
                // cwd 沿用 [`AcpConfig::default`] 的口径（当前工作目录）——
                // 登记里不表达「会话建在哪」，那不是 agent 身份的一部分。
                ..AcpConfig::default()
            };
            // 拉不起来 / 握手不合格 ⇒ **报错**，绝不回落规则实现。
            // `AcpAgent::connect` 自己已经 fail-closed（§8.4 的版本模糊会拒）。
            let agent = AcpAgent::connect(cfg, round).with_context(|| {
                format!(
                    "agent {:?} 登记的协议是 \"acp\"（endpoint {endpoint:?}），\
                     但没能连上它 —— \n\
                     没有回落到进程内规则实现：回落会让 extracted_by 记下\
                     一个**根本没跑过**的 agent，那是静默的假溯源（见本模块头）。",
                    registration.agent
                )
            })?;
            Ok(Box::new(agent))
        }

        // LLM = 内置 LLM 兜底实现，直连 OpenAI 兼容端点（REQ-034 · RN-5）。
        //
        // ⚠️ **同样从登记的 `endpoint` 取 base_url，不写死**，与 ACP 同一纪律：
        // 端点在哪是**部署事实**，写死等于「换个环境就得改代码，
        // 而报错会指向代码而不是环境」。
        //
        // ⚠️ **api key 与 model 从环境变量取**（`LlmConfig::from_env`）——
        // 凭据绝不写死、绝不进登记（登记表是要入库的）。
        //
        // ⚠️ **不做回落**（模块头的 fail-closed 纪律对这一支同样适用）：
        // 登记 `protocol = "llm"` 而环境变量没配 ⇒ **报错**，
        // 绝不悄悄换成规则实现跑完这一轮 —— 那会让 `extracted_by`
        // 记下一个**根本没跑过**的 agent。
        "llm" => {
            let endpoint = registration
                .endpoint
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .with_context(|| {
                    format!(
                        "agent {:?} 登记的协议是 \"llm\"，但 endpoint 是空的 —— \
                         base_url 是**登记的事实**，不写死在代码里（换个部署环境就得改代码）。\n\
                         请在登记里填形如 \"https://…/v1\" 的 endpoint。",
                        registration.agent
                    )
                })?;
            let cfg = LlmConfig::from_env().with_context(|| {
                format!(
                    "agent {:?} 登记的协议是 \"llm\"，但没能从环境变量取到凭据/模型。\n\
                     \n\
                     没有回落到进程内规则实现：回落会让 extracted_by 记下一个\
                     **根本没跑过**的 agent（见本模块头）。\n\
                     要设的变量名见上面的报错；**变量的值不要写进登记、日志或提交**。",
                    registration.agent
                )
            })?;
            let agent = LlmAgent::from_config(cfg, endpoint, &registration.agent, &registration.version, round)
                .with_context(|| {
                    format!(
                        "agent {:?} 登记的协议是 \"llm\"（endpoint {endpoint:?}），但没能造出实现。",
                        registration.agent
                    )
                })?;
            Ok(Box::new(agent))
        }

        // 其余协议（A2A / MCP …）尚未落地。
        //
        // 这里是**唯一**该出现「未实现」提示的地方：协议是登记里明写的事实，
        // 我们据实说「这个协议的实现还没有」，而不是悄悄换个实现跑。
        other => anyhow::bail!(
            "agent {:?} 登记的协议 {other:?} 没有对应实现 —— \
             抽取 agent 的实现由登记决定，查不到就报错，\
             **不静默回落到规则实现**（那会让 extracted_by 记下一个假的溯源）。\
             已实现的协议是 {}。",
            registration.agent,
            implemented_protocols()
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .join(" / "),
        ),
    }
}

/// **可插拔实现的注册表视图** —— `protocol` → 构造函数。
///
/// # 为什么它现在有 2 个条目（REQ-024 之后）
///
/// REQ-023 交付了 [`AcpAgent`] 与它的协议层，但**没有动分派点** ——
/// 那时这张表只有 `inproc`，生产入口造不出 ACP agent。REQ-024 把接线补上：
/// 登记里 `protocol = "acp"` + `endpoint = "mcode acp"` 就能造出实现。
///
/// 保留这层结构的理由：dispatch 表让「多一个实现 = 加一行」成为**惯例**
/// 而不是每次重新设计分派方式。`build_agent` 是**唯一的分派点**（纪律），
/// 这张表是「已实现哪些」的**可断言事实** —— `implemented_protocols().len()`
/// 现在是 2，被 `tests/test_agent_orchestration.rs` 钉住。
///
/// # 注意：**登记里能造出来 ≠ 生产上跑得起来**
///
/// `acp` 这条分派要真的拉起子进程并握手，而
/// [`AcpAgent::capabilities`] 只声明 `reports_usage` ⇒ 编排的
/// `require_proposal_producing` 会在**入口拒绝整轮**。
/// 那是**有意**的（见 [`AcpAgent`] 模块头）：宁可一分钱不花地拒绝，
/// 也不发一份溯源对不上的提案。
/// **进程内规则实现自报的执行者身份**（REQ-059）。
///
/// # 为什么要有这个常量
///
/// 装配层过去要造一条 [`AgentRegistration`] 时，`agent` 字段只能填
/// **调用方随手给的字符串**（`ontok-propose` 的默认值是硬编码的 `"mcode-agent"`），
/// 而 `protocol` 固定填 `"inproc"` —— 那条路径跑的确实是进程内规则实现。
/// 于是「这条本体是谁抽的」记的**从来不是执行者的事实**，而是一个占位串，
/// 却与另一件不同的事（翻译表的产出方）**长得几乎一样**，连版本都直接借用
/// 翻译表的版本号。
///
/// 本常量的值**由代码自己给出**：协议名 + 实现类型名 —— 不是谁「登记」它是
/// 什么，而是**编译进这个二进制的那个实现是什么**。§8.4 要防的是版本漂移，
/// 而版本漂移的前提是「记下来的那个名字**真的**对应跑过的那份代码」。
///
/// ⚠️ 取值刻意带 `protocol:` 前缀：`extracted_by.agent` 里出现 `inproc:` 时，
/// 一眼就能认出它是「按协议分派出来的执行者」而不是某个部署侧登记名，
/// 也就与 registry 里的登记名天然可分辨。
pub const INPROC_EXECUTOR_ID: &str = "inproc:ontok::agent::RuleBasedAgent";

/// **进程内规则实现的自报版本** —— 本 crate 自身的版本号。
///
/// 同样不是调用方填的：跑的是哪个二进制，那个二进制的版本就是它。
/// 「规则引擎的版本」与「翻译表的版本」是两件不同的事（后者是知识资产的
/// 产出方版本），混用会让 diff 归因指向一份并不存在的版本变化。
pub const INPROC_EXECUTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn implemented_protocols() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("inproc", "RuleBasedAgent（进程内规则引擎，零 I/O）");
    m.insert(
        "acp",
        "AcpAgent（mcode acp 子进程，stdio JSON-RPC；端点取自登记的 endpoint）",
    );
    m.insert(
        "llm",
        "LlmAgent（OpenAI 兼容端点 /v1/chat/completions；端点取自登记的 endpoint，凭据/模型取自环境变量）",
    );
    m
}

/// **翻译表来源名与抽取执行者名「读起来像同一个东西」**时返回一段话，否则 [`None`]。
///
/// # 这两条名字是**两件不同的事**
///
/// | 名字 | 说的是 |
/// | --- | --- |
/// | 翻译表的 `agent` | **知识资产（表名翻译表）是谁产出的** |
/// | `extracted_by.agent` | **这份提案的 items 是哪个执行者抽的** |
///
/// 它们落在**同一个提案的同一次输出**里，却各指一件不相干的事。
///
/// # 为什么要判「像同一个」而不只判「完全相等」
///
/// 完全相等是可以绕开的（换个名字就行）。真正出事的是**前缀相同** ——
/// 基线上的实况就是这样：翻译表登记的产出方是
/// `"mcode-agent (MiniMax Code)"`，而 `extracted_by.agent` 记的是
/// `"mcode-agent"`。两个字符串**不相等**，但同一屏输出里读起来就是同一个名字，
/// 于是「这条本体是谁抽的」与「翻译表是谁产的」被当成同一件事。
/// 所以判据按「归一化后互为前缀」判，那才是实测出事的那一种。
///
/// 短名豁免（不足 4 个字符的一侧不参与判定）：翻译表产出方叫 `a` / `L` 这类
/// 短代号时，任何长名都以它开头，那是巧合不是歧义 —— 按歧义拒绝会变成噪声。
pub fn executor_name_collision(table_names_agent: &str, executor: &str) -> Option<String> {
    let norm = |s: &str| s.trim().to_ascii_lowercase();
    let (a, b) = (norm(table_names_agent), norm(executor));
    let hits = if a == b {
        true
    } else if a.len() >= MIN_DISTINGUISHABLE_NAME_LEN && b.starts_with(&a) {
        true
    } else {
        b.len() >= MIN_DISTINGUISHABLE_NAME_LEN && a.starts_with(&b)
    };
    if !hits {
        return None;
    }
    Some(format!(
        "翻译表来源名与抽取执行者名撞了：翻译表登记的产出方是 {table_names_agent:?}，\
         这一轮真正执行的实现是 {executor:?}。\n\
         \n\
         为什么必须拒：这是**两件不相干的事** —— 前者是「表名翻译表这份知识资产由谁产出」，\
         后者是「这份提案的 items 由谁抽」。它们出现在同一屏输出里，一旦读起来像同一个名字，\
         就会把「本体是谁抽的」当成「翻译表是谁产的」，而这正是 §8.4 的版本归因要依据的那件事。\n\
         \n\
         怎么处理：把翻译表的 agent 字段改成一个不会与执行者名互相包含的名字\
         （它应该记**翻译表的产出方**，例如翻译流水线本身的名字），再重跑。",
        table_names_agent = table_names_agent,
        executor = executor,
    ))
}

/// 短于这个长度的名字不参与「互为前缀」判定（见 [`executor_name_collision`]）。
const MIN_DISTINGUISHABLE_NAME_LEN: usize = 4;

/// **登记身份 vs 握手身份的不一致告警**（非致命）—— 不一致时返回一段话，一致时 [`None`]。
///
/// # 为什么需要它：两条路记的是**两个不同来源**的身份
///
/// | 记在哪 | 值来自 | 代码 |
/// | --- | --- | --- |
/// | `extraction_rounds`（轮次台账）| **登记** `agent` / `version` | 调用方在 `build_agent` **之前** [`AgentRegistry::record_round`] |
/// | `extracted_by`（提案元数据）| **握手** `agent` / `version` | `orchestrate` 里 `agent.identity()` → [`crate::agent::stamp_provenance`] |
///
/// `record_round` 在**闸门之前**、握手在闸门之后，两者之间没有任何一致性核对。
/// 所以登记名与握手名不一致时（实测：登记 `mcode-agent`，`mcode acp` 握手报
/// `minimax-code`），`extraction_rounds` 记登记值、`extracted_by` 记握手值，
/// **两处对不上，而且都不报错** —— 查轮次台账的人会以为这一轮是另一个 agent 跑的。
///
/// # 为什么是**告警**而不是 `bail!`
///
/// **别名是合法的**：同一个实现登记成 `mcode-agent`、自己报 `minimax-code`，
/// 这在 adapter 接进来之前是常态（登记名是部署侧的约定，握手名是实现侧的事实）。
/// 在这里报错等于「接协议实现之前必须先把两边名字对齐」，那是**政策**不是**事实**，
/// 由本函数替仓库做决定不合适。所以：让真相**可见**，不误杀。
///
/// # ⚠️ REQ-059：本函数是**探测器**，处置权在调用方 —— 而调用方现在改成**拒绝出产物**
///
/// 上面那段「不报错是有意的」是**本函数**的判断，它只负责**发现**不一致。
/// **生产入口（`ontok-propose`）现在把不一致当致命错误处理**（`bail!`，不出产物），
/// 理由是 §8.4：这个字段进的是**版本快照**，而这一串需求要防的正是版本漂移；
/// 「让人看见但不阻止」等于让人拿着一份**自己也说不清谁抽的**提案继续往下走。
///
/// 所以现在的分工是：**这里判定「对不对得上」**（纯函数、可测、文案点名两边的值），
/// **入口决定「对不上怎么办」**（拒绝，而不是打一行 `⚠` 然后照常出产物）。
/// 别再把 `eprintln!("⚠ {warn}")` 加回生产入口 —— 那正是 REQ-059 要拆掉的那一半。
///
/// # ⚠️ 这条核对在 `inproc` 路径上**曾经恒真**（已修，但值得记住）
///
/// REQ-059 之前，生产入口传进来比的是
/// `AgentIdentity::from_registration(&registration, round)` 与 `agent.identity()`，
/// 而 `inproc` 分支造 `RuleBasedAgent` 时用的身份**正是前者的同源拷贝**
/// （[`build_agent`] 里 `AgentIdentity::from_registration(registration, round)`）。
/// ⇒ 它拿一个值跟它自己比，**恒返回 `None`**，在默认路径上永远不可能触发。
/// 「有告警分支」不等于「这条分支走过」—— 现在默认路径登记的就是真实执行者身份，
/// 核对虽仍无差异可报，但它比的两边不再是同一个凭空值。
///
/// # ⚠️ 已知限制：**改不掉**，且别再试「用握手身份重记一轮」
///
/// 今天是**潜伏**的，不是活的 bug：[`AcpAgent::capabilities`] 只声明
/// `reports_usage` ⇒ `require_proposal_producing` 在**入口**就拒掉整轮，
/// ACP 路径根本走不到阶段 1，也就没有 `extraction_rounds` 与 `extracted_by`
/// 对不上的机会。**今天改掉它是在给一条跑不通的路修路。**
///
/// 曾经考虑过的修法（`record_round` 之后拿真实握手身份再记一轮）**走不通**，
/// 别再试一遍：[`crate::db::AgentRegistry::record_round`] 第一件事就是
/// `self.require(agent, version).await?`，而握手身份（如 `minimax-code 0.5.5`）
/// **不在登记里** ⇒ 直接返回 `VersionNotRegistered`（`src/db/agent_registry.rs:270-271`）。
/// 即「登记一个假的 agent 名去凑对得上」——那会把假身份写进台账，
/// 比现在两处对不上**更糟**。真要修得先决定「以哪边为准」
/// （改登记名 vs 放宽 `record_round` 的登记校验），那是需求决策，不是本函数该定的。
///
/// 所以这里只做**在当前边界内唯一诚实**的事：把不一致说出来。
pub fn identity_cross_check(
    registered: &AgentIdentity,
    handshake: &AgentIdentity,
) -> Option<String> {
    if registered.agent == handshake.agent && registered.version == handshake.version {
        return None;
    }
    Some(format!(
        "抽取身份两处对不上：登记的是 {reg_agent} {reg_version}，\
         这一轮握手报的是 {hs_agent} {hs_version}。\n\
         轮次台账（extraction_rounds）记的是**登记值**（record_round 发生在握手之前），\
         提案元数据（extracted_by）记的是**握手值**。\n\
         本函数只判定「对不对得上」；**生产入口据此拒绝出产物**（REQ-059）——\
         这个字段进的是版本快照，对不上就发出去等于让 §8.4 的版本归因建在一个\
         没人能说清的名字上。\n\
         要跑通就得先决定「以哪边为准」（改登记名，还是放宽 record_round 的登记校验），\
         别靠两处各记各的来蒙混过去。",
        reg_agent = registered.agent,
        reg_version = registered.version,
        hs_agent = handshake.agent,
        hs_version = handshake.version,
    ))
}
