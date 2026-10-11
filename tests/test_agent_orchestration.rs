//! REQ-015 · 把 agent 抽象层**真正接到生产路径**（缺陷 F2 + F3）
//!
//! # 这个文件钉住什么
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | factory 按登记造实现，查不到**报错不回落** | 回落 = 登记了 a2a 却悄悄用规则跑完一轮，`extracted_by` 就是假溯源 |
//! | 编排**一表一组**、逐组 `drive_group` | 规格 §7.1 阶段 1「每组一次调用」；不进分组等于没接通 |
//! | 重试次数/失败组**由 `drive_group` 产生** | 绕开它 = 编排自己发明一套重试，设计点⑤的「显式失败」就漏了 |
//! | **阶段 2 在代码侧核对** agent 的映射声明 | 规格架构红线：证据核对不可外包。摘掉 `verify_mappings` 就是把红线拆了 |
//! | `failed_groups()` **真的被读取**并汇总 | 不读 = 死方法，agent 自报的失败被静默吞掉 |
//! | 生产入口**没有具体类型分支** | 缺陷 F2 的原话：接第二个 agent 要改 `ontok-propose.rs` |
//! | CLI 产出**逐字节不变** | 加了一层抽象最大的风险是行为漂移；没这条断言，漂了也没人知道 |

mod common;
use common::crate_root;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ontok::agent::{CAPABILITY_REBUILDS_PROPOSAL};
use ontok::agent::factory::identity_cross_check;
use ontok::agent::orchestrate::{
    orchestrate, GroupViolation, OrchestrationInput, DEFAULT_ATTEMPTS,
};
use ontok::agent::registry::{AgentIdentity, AgentRegistration};
use ontok::agent::{
    build_agent, implemented_protocols, verify_mappings, CandidateProperty, CandidateType,
    ColumnRef, ExistingType, ExtractionAgent, ExtractionGroup, GroupExtraction, GroupFailure,
    MappingViolation, CAPABILITY_PRODUCES_PROPOSAL,
};
use ontok::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};
use ontok::extract::names::TableNames;
use ontok::extract::propose::extract;
use serde_json::json;

// ──────────────────────────────────────────────────────────
// 造数据
// ──────────────────────────────────────────────────────────

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req015-test",
  "names": {
    "t_dev": "设备",
    "t_customer": "客户"
  }
}"#;

fn col(name: &str, comment: Option<&str>) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        data_type: "text".into(),
        nullable: true,
        comment: comment.map(|s| s.to_string()),
        is_primary_key: false,
    }
}

fn table(name: &str, comment: Option<&str>, columns: Vec<ColumnMeta>) -> TableMeta {
    let commented_count = columns.iter().filter(|c| c.comment.is_some()).count();
    TableMeta {
        name: name.to_string(),
        comment: comment.map(|s| s.to_string()),
        column_count: columns.len(),
        commented_count,
        columns,
    }
}

/// 三张表的小 schema：`t_dev` 有一列 `customer_id` 指向 `t_customer`
/// （零外键库靠 `*_id` 命名约定推关系 —— 这条关系**跨组**，
/// 用来钉住「上下文随组走但作用域不随之变宽」）。
fn schema() -> SchemaReport {
    let tables = vec![
        table(
            "t_dev",
            Some("设备表"),
            vec![
                col("id", None),
                col("sn", Some("序列号")),
                col("customer_id", Some("客户ID")),
            ],
        ),
        table(
            "t_customer",
            Some("客户表"),
            vec![col("id", None), col("code", Some("客户编码"))],
        ),
        table("t_region", Some("区域表"), vec![col("id", None)]),
    ];
    let column_count: usize = tables.iter().map(|t| t.column_count).sum();
    SchemaReport {
        server_version: "PostgreSQL 17.2".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: tables.len(),
        column_count,
        commented_column_count: 3,
        foreign_key_count: 0,
        introspected_at: "2026-10-07T10:00:00Z".into(),
        tables,
    }
}

fn names() -> TableNames {
    TableNames::load(NAMES_JSON).expect("翻译表应可载入")
}

fn reg(agent: &str, version: &str, protocol: &str) -> AgentRegistration {
    AgentRegistration {
        agent: agent.to_string(),
        version: version.to_string(),
        protocol: protocol.to_string(),
        endpoint: None,
        // 与 tests/test_extraction_agent.rs 的登记**同内容** ——
        // 共享本体库上同键同内容是幂等重放，内容不同才会 VersionLocked。
        // 这里必须一致，否则两个测试文件互相把对方顶掉。
        capabilities: vec!["group_extract".into()],
        registered_at: "2026-10-07T10:00:00Z".into(),
    }
}

fn identity() -> AgentIdentity {
    AgentIdentity {
        agent: "mcode-agent".into(),
        version: "2026.10.07".into(),
        round: 1,
    }
}

/// **替身默认声明 `produces_proposal`**（REQ-019 的闸门）。
///
/// REQ-019 起编排会拒绝「没声明它的 agent」—— 而下面这些替身要走到
/// 阶段 2 / 阶段 3+4 才能被测到，所以必须显式声明。
/// 声明**不等于**它们真的产出提案：这只是让它们拿到与规则实现同样的通行证，
/// 这一点在本文件里是事实（规则管线跑的就是规则），判别力由
/// [`test_orchestrate_refuses_agent_that_cannot_produce_the_proposal`] 守。
fn produces_proposal() -> Vec<String> {
    vec![CAPABILITY_PRODUCES_PROPOSAL.to_string()]
}

// ──────────────────────────────────────────────────────────
// 测试替身（**不是**生产实现，不构成「已有多个实现」）
// ──────────────────────────────────────────────────────────

struct AlwaysFails {
    identity: AgentIdentity,
    calls: Arc<AtomicU32>,
}

impl ExtractionAgent for AlwaysFails {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        produces_proposal()
    }
    fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("模拟协议超时")
    }
}

/// 第一次失败、之后成功 —— 用来钉住「重试真的发生在 `drive_group` 里」。
struct FailsThenSucceeds {
    identity: AgentIdentity,
    calls: Arc<AtomicU32>,
}

impl ExtractionAgent for FailsThenSucceeds {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        produces_proposal()
    }
    fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            anyhow::bail!("第一次不该出现的错")
        }
        Ok(GroupExtraction {
            types: Vec::new(),
            links: Vec::new(),
        })
    }
}

/// **撒谎的 agent**：声明一条表里**根本没有**的来源列。
///
/// 这是缺陷 F3 的靶子 —— 阶段 1 的产出只是「声明」，属不属实由代码核对。
struct LyingAgent {
    identity: AgentIdentity,
}

impl ExtractionAgent for LyingAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        produces_proposal()
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        Ok(GroupExtraction {
            links: Vec::new(),
            types: vec![CandidateType {
                key: table.clone(),
                display_name: "撒谎的类型".into(),
                is_new: true,
                properties: vec![CandidateProperty {
                    name: "凭空捏造的序列号".into(),
                    source: ColumnRef::new(&table, "no_such_column"),
                    nullable: None,
                    note: None,
                    origin: None,
                }],
                description: None,
                aliases: Vec::new(),
                evidence: Vec::new(),
                rejected_comments: None,
            }],
        })
    }
}

/// 抽「成功」了，但**自报**有一组失败 —— 编排必须把它读出来。
struct OkButSelfReportsFailure {
    identity: AgentIdentity,
}

impl ExtractionAgent for OkButSelfReportsFailure {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn capabilities(&self) -> Vec<String> {
        produces_proposal()
    }
    fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        Ok(GroupExtraction {
            types: Vec::new(),
            links: Vec::new(),
        })
    }
    fn failed_groups(&self) -> Vec<GroupFailure> {
        vec![GroupFailure {
            group_id: "table:t_dev".into(),
            table: "t_dev".into(),
            attempts: 1,
            reason: "agent 内部只出了一半结果".into(),
        }]
    }
}

// ──────────────────────────────────────────────────────────
// 判据一：factory 按登记造实现，且 fail-closed
// ──────────────────────────────────────────────────────────

#[test]
fn test_factory_builds_dyn_from_registered_protocol() {
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), names(), 1)
        .expect("inproc 有实现，应当造得出");
    // 拿到的是 dyn：身份来自登记，不是硬编码默认值（§8.4）
    assert_eq!(agent.identity().agent, "mcode-agent");
    assert_eq!(agent.identity().version, "2026.10.07");
    assert_eq!(agent.identity().round, 1);
}

/// ★ fail-closed：**没有登记的实现 ⇒ 报错，绝不回落规则实现**。
///
/// 这是缺陷 F2 真正要治的病。回落一次，用户登记了 `protocol = "a2a"`、
/// adapter 还没写，系统却悄悄用规则实现跑完了这一轮，提案正常产出、
/// 统计正常，**没人知道这一轮没走登记的那个 agent** ——
/// `extracted_by` 成了假溯源，而且静默。
#[test]
fn test_factory_fails_closed_on_unknown_protocol_instead_of_falling_back() {
    let err = build_agent(&reg("vendor-extractor", "1.0.0", "a2a"), names(), 1)
        .err()
        .expect("没有登记实现的协议必须报错，不许回落规则实现");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("a2a"),
        "错误信息要指名道姓说哪个协议没有实现，实际：{msg}"
    );
    assert!(
        msg.contains("inproc"),
        "错误信息要点明进程内实现才是已实现的（对照 `implemented_protocols`），实际：{msg}"
    );
}

/// 模糊版本（「latest」）在**造实现之前**就该被挡住（§8.4）。
#[test]
fn test_factory_rejects_vague_version_before_building() {
    let err = build_agent(&reg("mcode-agent", "latest", "inproc"), names(), 1)
        .err()
        .expect("模糊版本必须报错");
    assert!(
        format!("{err:#}").contains("模糊"),
        "错误信息要说清是模糊版本，实际：{err:#}"
    );
}

/// 已实现的协议**有 3 个**（REQ-034 起内置 LLM 兜底也接进了分派点）
/// —— 这个数字是事实，钉住它，免得哪天有人把「造得出来」与「跑得完整轮」混成一件事。
///
/// ⚠️ 这个断言**被本文件自己改过两次**，每次都只因**实况变了**：
/// · REQ-023 时是 `len() == 1`，编码的是「协议 adapter 属 RN-2」这条当时为真的事实；
/// · REQ-024 接入 ACP 后改成 `len() == 2`；
/// · **REQ-034** 接入内置 LLM 兜底（`protocol = "llm"`）后改成 `len() == 3`。
///
/// ⚠️ **它钉的是「已接线几个」，不是「承诺几个」**。
/// `mod.rs` 的模块头明确说过：规格 §7.1 只许诺「**可插拔**」，**未列举个数**，
/// 且曾把这一行拔高成「≥3（规则 / 外部协议 / LLM 兜底）」后被 QA 独立核实为**假陈述**
/// 并删除（REQ-025）。所以这里断言 3，是因为**本提交确实接了 3 个**，
/// 不是因为规格欠了 3 个 —— 别把它读成后者。
#[test]
fn test_three_protocols_are_implemented_and_the_factory_knows_all_of_them() {
    let m = implemented_protocols();
    assert_eq!(
        m.len(),
        3,
        "截至本提交实现了 inproc（规则）、acp（子进程协议）、llm（OpenAI 兼容端点）三个。\
         注意：这个数字钉的是**已接线几个**，不是规格许诺了几个\
         （§7.1 只许诺「可插拔」，未列举个数）。实际：{m:?}"
    );
    assert!(m.contains_key("inproc"), "规则实现必须在，实际：{m:?}");
    assert!(
        m.contains_key("acp"),
        "ACP adapter 已接进分派点（RN-2 收尾），实际：{m:?}"
    );
    assert!(
        m.contains_key("llm"),
        "内置 LLM 兜底已接进分派点（REQ-034 · RN-5），实际：{m:?}"
    );
}

/// 「造得出」**不等于**「跑得完整轮」—— 两者被这条测试分开记账。
///
/// ACP 这条分派会真的拉起子进程；造不出来时必须**报错**而不是回落规则实现
/// （回落 = `extracted_by` 记下一个根本没跑过的 agent）。
#[test]
fn test_factory_refuses_to_build_an_acp_agent_without_a_usable_endpoint() {
    // 空 endpoint ⇒ fail-closed，且错误要指名是 endpoint 缺了。
    let mut r = reg("mcode-agent", "2026.10.07", "acp");
    r.endpoint = None;
    let err = build_agent(&r, names(), 1)
        .err()
        .expect("登记了 acp 却没给 endpoint ⇒ 必须报错，不许猜一个可执行文件");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("endpoint"),
        "要说清缺的是 endpoint，实际：{msg}"
    );
    assert!(
        msg.contains("mcode-agent"),
        "要指名道姓哪个 agent 被拒，实际：{msg}"
    );

    // 空白串也当「没给」—— 登记里填了三个空格不是配置。
    let mut r2 = reg("mcode-agent", "2026.10.07", "acp");
    r2.endpoint = Some("   ".to_string());
    assert!(
        build_agent(&r2, names(), 1).is_err(),
        "空白 endpoint 等于没给，不许当成合法端点"
    );
}

/// ★ fail-closed：**可执行文件不存在** ⇒ 报错，绝不回落规则实现。
///
/// # 为什么这条值得单独测（上一轮我判它是「测它要真起子进程，与测试纪律冲突」—— 判错了）
///
/// 那个理由是错的，而且错在**机制**上：
///
/// - **不存在的可执行文件在 [`Command::spawn`] 的 `fork` 之前就失败** ——
///   shell 层找不到它，`spawn` 直接返回 `Err(NotFound)`。**零子进程成本**。
/// - `/bin/false` 才真的起一个进程，但它**立刻退出**：stdout 立刻 EOF ⇒
///   握手读不到 `initialize` 的应答 ⇒ fail-closed。<1s、无依赖、
///   不需要登录态、CI 可复现。
///
/// 两条**都不碰 `mcode acp`**（可执行文件是我们自己指的），所以既不花钱、
/// 也不依赖那个二进制在不在。这正是「造得出 ≠ 跑得完整轮」这条纪律的最后一格：
/// 连**拉不起来**都得 fail-closed，否则用户登记了一个坏端点，
/// 系统会拿别的东西把这一轮跑完，而 `extracted_by` 记的是那个**没跑过的** agent。
#[test]
fn test_factory_fails_closed_when_the_acp_executable_does_not_exist() {
    let mut r = reg("mcode-agent", "2026.10.07", "acp");
    // 指向一个**确定不存在**的路径：名字带仓库专属前缀，避免撞上真有的东西。
    r.endpoint = Some("/nonexistent/ontok-acp-no-such-executable".to_string());

    let err = build_agent(&r, names(), 1)
        .err()
        .expect("★ 端点指到一个不存在的可执行文件 ⇒ 必须报错，不许回落规则实现");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("mcode-agent"),
        "要指名道姓哪个 agent 拉不起来（错误要指向部署事实，不是指向代码），实际：{msg}"
    );
    assert!(
        msg.contains("acp"),
        "要说清是 acp 这条分派出的问题，实际：{msg}"
    );
    assert!(
        msg.contains("回落到进程内规则实现"),
        "★ 错误信息必须说清「没有回落」—— 回落是这里唯一不可接受的分支，实际：{msg}"
    );
}

/// ★ fail-closed：**可执行文件起得来但立刻退出**（握手不合格）⇒ 同样报错。
///
/// 与上一条配对，覆盖 ACP 分派的**两种**失败：
/// `spawn` 失败（文件不存在）与握手失败（跑起来了但不按协议应答）。
/// 只测前者的话，「进程起得来 ⇒ 身份就是真的」这个**危险假设**没人钉。
#[test]
fn test_factory_fails_closed_when_the_acp_process_dies_before_the_handshake() {
    let mut r = reg("mcode-agent", "2026.10.07", "acp");
    // `/bin/false` 起得来、立刻退出、stdout 立刻 EOF ⇒ 握手拿不到应答。
    // 它不依赖 mcode、不依赖网络、不需要登录态。
    r.endpoint = Some("/bin/false".to_string());

    let err = build_agent(&r, names(), 1)
        .err()
        .expect("★ 进程起得来但握手不合格 ⇒ 同样必须报错，不许回落规则实现");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("回落到进程内规则实现"),
        "★ 握手失败也必须说清「没有回落」，实际：{msg}"
    );
}

/// ★ 登记身份 vs 握手身份不一致 ⇒ **告警**（不报错），一致 ⇒ 静默。
///
/// # 为什么不测成 `bail!`
///
/// 别名是**合法的**：登记名是部署侧的约定（`mcode-agent`），握手名是实现侧的
/// 事实（`minimax-code`）。在这里报错等于仓库替人定了「必须先把两边名字对齐」
/// 这条政策。所以判据是：**不一致要说出来，一致不许啰嗦**。
///
/// 少一条都会出事：不一致却静默 ⇒ `extraction_rounds` 记登记值、
/// `extracted_by` 记握手值，两处对不上且没人知道（这正是本函数存在的理由）。
#[test]
fn test_identity_cross_check_warns_on_a_fork_but_stays_quiet_when_aligned() {
    let registered = identity(); // mcode-agent 2026.10.07

    // 一致 ⇒ None（不许对每一轮都喊一遍狼来了）。
    assert!(
        identity_cross_check(&registered, &registered).is_none(),
        "身份一致时不该有任何告警（否则日志噪声会把它真正有用的那次淹掉）"
    );

    // 实测过的分叉形状：登记名与握手名/版本都不同。
    let handshake = AgentIdentity {
        agent: "minimax-code".into(),
        version: "0.5.5".into(),
        round: 1,
    };
    let warn = identity_cross_check(&registered, &handshake)
        .expect("★ 身份对不上必须告警，否则两处溯源错开且静默");

    // 告警要**两个值都点名** —— 运维得能据此判断该改哪一边，
    // 只说「对不上」等于把判断题原样退回去。
    for needle in ["mcode-agent", "2026.10.07", "minimax-code", "0.5.5"] {
        assert!(
            warn.contains(needle),
            "告警必须点名 {needle:?}（只说「对不上」等于没告诉运维该看哪一边），实际：{warn}"
        );
    }
    assert!(
        warn.contains("extraction_rounds") && warn.contains("extracted_by"),
        "★ 告警要说清**错开的是哪两处**，否则运维不知道去查哪里，实际：{warn}"
    );
}

/// ★ 交叉核对**不许被摘掉**，且**不许退回「只告警」**（变异自证的靶子 · REQ-059 加强）。
///
/// `identity_cross_check` 是个纯函数，测试它只能证明「函数算得对」，
/// 证明不了「生产入口**真的调了它**」。而后者才是会出事的那一半：
/// 函数留着、调用点删掉 ⇒ 测试全绿、线上静默。
///
/// 所以补一条**源码断言**（本仓已有先例，见
/// `test_registry_source_has_no_destructive_sql` 与
/// `test_production_entry_has_no_concrete_agent_type_branch`）：钉住
/// `ontok-propose.rs` 里那两样东西同时在 —— 调用了函数、且不一致时**拒绝出产物**。
///
/// ⚠️ REQ-059 把处置从「`eprintln` 打一行 ⚠ 然后照常出产物」改成**致命**：
/// `extracted_by` 进的是版本快照（§8.4 要防版本漂移），
/// 「让人看见但不阻止」等于发一份自己都说不清谁抽的提案出去。
/// 下面的断言因此从「必须打出来」翻成「必须**拦住**」。
///
/// ⚠️ **这条断言曾经假通过过一次，值得记下来**：REQ-059 改完之后本测试**仍然是绿的**，
/// 因为生产入口里新写的**注释**恰好含有 `eprintln!("⚠ {warn}")` 这段字面量，
/// 而断言用的是 `src.contains(..)` —— **散文里写着**不等于**代码里还在**。
/// 所以现在断言同时钉住「不许再出现那段字面量」，注释里也就不许复述它了。
#[test]
fn test_the_production_entry_still_reports_the_identity_fork() {
    let src = include_str!("../src/bin/ontok-propose.rs");
    assert!(
        src.contains("identity_cross_check("),
        "★ 生产入口不再调用交叉核对了 —— 身份错开会变成静默的（函数还在，只是没人调）"
    );
    // 不一致 ⇒ 必须**拒绝出产物**。判据：交叉核对之后的那段必须落到 bail!，
    // 且**不再有**「打一行告警就往下走」的写法。
    let check = src
        .find("identity_cross_check(")
        .expect("生产入口仍调用交叉核对");
    let after = &src[check..(check + 600).min(src.len())];
    assert!(
        after.contains("anyhow::bail!") || after.contains("bail!("),
        "★ 身份对不上必须**拒绝出产物**（bail），不是打一行告警然后照发 —— \
         extracted_by 进版本快照，对不上就发等于让 §8.4 的版本归因建在假名字上"
    );
    assert!(
        !src.contains("⚠ {warn}"),
        "★ 生产入口不许再有「用 eprintln 打一行身份告警」的写法（REQ-059 起一律 bail）"
    );
    // 交叉核对必须在**造出 agent 之后**才有意义：握手身份那会儿才有。
    let build = src.find("build_agent(&registration").expect("生产入口仍经 build_agent 造实现");
    assert!(
        build < check,
        "★ 交叉核对必须在 build_agent **之后** —— 握手身份在握手完成前还不存在，\
         放在前面会拿登记身份跟自己比，得到一个恒为「一致」的假阴性"
    );
}

// ──────────────────────────────────────────────────────────
// 判据二：编排真的按表分组、逐组走 drive_group
// ──────────────────────────────────────────────────────────

#[test]
fn test_orchestrate_runs_one_group_per_table() {
    let report = schema();
    let n = names();
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), n.clone(), 1).unwrap();
    let input = OrchestrationInput::new(&n, "extracted");

    let run = orchestrate(&*agent, &report, &input).expect("编排应成功");

    assert_eq!(run.stage.groups, report.tables.len(), "一表一组（设计点②）");
    assert_eq!(run.stage.groups_ok, report.tables.len(), "规则实现不该有失败组");
    assert!(run.stage.failed_groups.is_empty(), "规则实现恒成功");
}

/// ★ 失败组**必须**由编排汇总，并指名道姓（设计点⑤「不静默跳过」）。
#[test]
fn test_orchestrate_records_every_failed_group_with_group_id_and_reason() {
    let report = schema();
    let n = names();
    let calls = Arc::new(AtomicU32::new(0));
    let agent = AlwaysFails { identity: identity(), calls: calls.clone() };
    let input = OrchestrationInput::new(&n, "extracted");

    let run = orchestrate(&agent, &report, &input).expect("编排本身不该失败");

    assert_eq!(run.stage.groups_ok, 0);
    assert_eq!(
        run.stage.failed_groups.len(),
        report.tables.len(),
        "每一组失败都要显式记下来，不许静默跳过"
    );
    for f in &run.stage.failed_groups {
        assert!(f.group_id.starts_with("table:"), "组 id 要能指名：{:?}", f);
        assert_eq!(f.reason, "模拟协议超时", "要带最后一次失败的原因");
    }
    // 失败的组也必须进提案元数据（REQ-002 的 `extracted_by.failed_groups`）
    let fb = &run.outcome.proposal.extracted_by.failed_groups;
    assert_eq!(fb.len(), report.tables.len(), "失败组要进 extracted_by");
    assert_eq!(fb[0]["group_id"], json!(run.stage.failed_groups[0].group_id));
}

/// ★ 判别「编排绕开 `drive_group`」：重试次数只能由 `drive_group` 产生。
///
/// `attempts = 3` 时每个组应该被调用 **3** 次。若编排绕开 `drive_group`
/// 直接调 `extract_group`（或自己发重试），调用数会变成 1，
/// `GroupFailure.attempts` 也会变成 1 —— 这条断言当场失败。
#[test]
fn test_retry_count_comes_from_drive_group_not_from_a_private_loop() {
    let report = schema();
    let n = names();
    let calls = Arc::new(AtomicU32::new(0));
    let agent = AlwaysFails { identity: identity(), calls: calls.clone() };
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.attempts = 3;

    let run = orchestrate(&agent, &report, &input).expect("编排本身不该失败");

    assert_eq!(
        calls.load(Ordering::SeqCst) as usize,
        report.tables.len() * 3,
        "每组 3 次重试 ⇒ 总调用次数必须是 组数 × 3（证明走的是 drive_group 的重试）"
    );
    for f in &run.stage.failed_groups {
        assert_eq!(
            f.attempts, 3,
            "attempts 必须由 drive_group 如实记录（绕开它就会记成 1）"
        );
    }
}

/// 第一次失败、之后成功 ⇒ `attempts = 2` 时**全部成功**。
///
/// 绕开 `drive_group`（只调一次 `extract_group`）会把这些组记成失败。
#[test]
fn test_retry_recovers_a_flaky_agent_inside_orchestration() {
    let report = schema();
    let n = names();
    let calls = Arc::new(AtomicU32::new(0));
    let agent = FailsThenSucceeds { identity: identity(), calls };
    let mut input = OrchestrationInput::new(&n, "extracted");
    input.attempts = 2;

    let run = orchestrate(&agent, &report, &input).expect("编排本身不该失败");

    assert_eq!(run.stage.groups_ok, report.tables.len(), "第二次尝试应当成功");
    assert!(
        run.stage.failed_groups.is_empty(),
        "重试救回来的组不该再记成失败，实际：{:?}",
        run.stage.failed_groups
    );
}

/// `failed_groups()` 真的被读取并汇总（否则它就是死方法）。
#[test]
fn test_agent_reported_failed_groups_are_read_and_merged() {
    let report = schema();
    let n = names();
    let agent = OkButSelfReportsFailure { identity: identity() };
    let input = OrchestrationInput::new(&n, "extracted");

    let run = orchestrate(&agent, &report, &input).expect("编排本身不该失败");

    // agent 每一组都返回 Ok，但自报 t_dev 失败 —— 编排必须把它读出来
    let reported: Vec<&GroupFailure> = run
        .stage
        .failed_groups
        .iter()
        .filter(|f| f.group_id == "table:t_dev")
        .collect();
    assert_eq!(
        reported.len(),
        1,
        "agent 自报的失败组必须被汇总（不读 failed_groups() 就是死方法），实际汇总：{:?}",
        run.stage.failed_groups
    );
    assert_eq!(reported[0].reason, "agent 内部只出了一半结果");
    assert_eq!(
        run.outcome.proposal.extracted_by.failed_groups
            .iter()
            .filter(|v| v["group_id"] == json!("table:t_dev"))
            .count(),
        1,
        "自报的失败组也要进提案元数据"
    );
}

/// 同一组**不会**在 `failed_groups` 里出现两条（`drive_group` 的失败与
/// agent 自报的去重合并）。
#[test]
fn test_failed_groups_are_deduplicated_by_group_id() {
    let report = SchemaReport { tables: vec![schema().tables[0].clone()], ..schema() };
    let n = names();
    let calls = Arc::new(AtomicU32::new(0));
    // 既在 drive_group 里失败，又自报同一组失败
    struct Both {
        identity: AgentIdentity,
        calls: Arc<AtomicU32>,
    }
    impl ExtractionAgent for Both {
        fn identity(&self) -> &AgentIdentity {
            &self.identity
        }
        fn capabilities(&self) -> Vec<String> {
            produces_proposal()
        }
        fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("超时")
        }
        fn failed_groups(&self) -> Vec<GroupFailure> {
            vec![GroupFailure {
                group_id: "table:t_dev".into(),
                table: "t_dev".into(),
                attempts: 1,
                reason: "agent 自己也报了".into(),
            }]
        }
    }
    let agent = Both { identity: identity(), calls };
    let input = OrchestrationInput::new(&n, "extracted");

    let run = orchestrate(&agent, &report, &input).unwrap();
    assert_eq!(
        run.stage.failed_groups.len(),
        1,
        "同一组不该记两条，实际：{:?}",
        run.stage.failed_groups
    );
}

// ──────────────────────────────────────────────────────────
// 判据三：阶段 2 的证据核对在代码侧，且接在编排里（缺陷 F3）
// ──────────────────────────────────────────────────────────

/// ★ 缺陷 F3 的核心判据：agent 声明了什么，**代码去核对**，
/// 核不过去必须**显式报出来**（只报错不丢弃）。
///
/// 摘掉编排里的 `verify_mappings` ⇒ `violations` 为空 ⇒ 本测试失败。
#[test]
fn test_orchestrate_verifies_agent_declarations_in_code() {
    let report = schema();
    let n = names();
    let agent = LyingAgent { identity: identity() };
    let input = OrchestrationInput::new(&n, "extracted");

    let run = orchestrate(&agent, &report, &input).expect("编排本身不该失败");

    assert_eq!(
        run.stage.violations.len(),
        report.tables.len(),
        "每一组的假声明都必须被代码核对出来，实际：{:?}",
        run.stage.violations
    );
    for v in &run.stage.violations {
        assert!(
            matches!(v.violation, MappingViolation::UnknownColumn { .. }),
            "核出来的应是「表在、列不在」，实际：{:?}",
            v.violation
        );
        assert!(v.group_id.starts_with("table:"), "违规要能定位到组：{:?}", v);
    }
    // 只报错**不丢弃**：违规项不改变提案本体 —— 删不删是人的决定
    assert_eq!(
        run.outcome.proposal.items
            .iter()
            .filter(|i| i["kind"] == "add_entity_type")
            .count(),
        report.tables.len(),
        "阶段 2 只报错不丢弃 —— 提案实体数不受违规项影响"
    );
}

/// 编排里的阶段 2 与独立调 [`verify_mappings`] **结论一致** ——
/// 证明接上去的确实是那个纯函数，不是另写了一份弱化版。
#[test]
fn test_orchestration_uses_the_same_verify_mappings_semantics() {
    let report = schema();
    let n = names();
    let agent = LyingAgent { identity: identity() };
    let input = OrchestrationInput::new(&n, "extracted");
    let run = orchestrate(&agent, &report, &input).unwrap();

    let group = ExtractionGroup {
        group_id: "table:t_dev".into(),
        source: ontok::agent::GroupSource {
            schema: "public".into(),
            introspected_at: "2026-10-07T10:00:00Z".into(),
            server_version: "PostgreSQL 17.2".into(),
            transaction_read_only: "on".into(),
            foreign_key_count: 0,
        },
        schema: vec![report.tables[0].clone()],
        context_tables: report.tables.iter().map(|t| t.name.clone()).collect(),
        samples: vec![],
        constraints: Default::default(),
        domain_hints: vec![],
        existing_types: vec![],
    };
    let standalone = verify_mappings(
        &LyingAgent { identity: identity() }
            .extract_group(&group)
            .unwrap(),
        &group,
    );
    // 按**组**比：schema 有 3 张表 ⇒ 编排产出 3 组的违规，
    // 独立调用只核了 t_dev 这一组 ⇒ 1 条。比总数只会得到假失败。
    let from_orch = run
        .stage
        .violations
        .iter()
        .filter(|v| v.group_id == "table:t_dev")
        .count();
    assert_eq!(
        from_orch, standalone.len(),
        "编排里 t_dev 这一组的阶段 2 结论必须与独立调用 verify_mappings 同口径"
    );
}

/// 规则实现在这份真实形状的 schema 上**一条违规都没有** ——
///
/// 这是个**阴性**结论，但必须钉住：将来真出现违规时，
/// 才分得清「是新引入的」还是「一直都有」。
/// （生产 schema 上实测同为 0 条，见报告。）
#[test]
fn test_rule_based_agent_declarations_pass_verification() {
    let report = schema();
    let n = names();
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), n.clone(), 1).unwrap();
    let input = OrchestrationInput::new(&n, "extracted");
    let run = orchestrate(&*agent, &report, &input).unwrap();
    assert_eq!(run.stage.groups_ok, 3);
    assert!(
        run.stage.violations.is_empty(),
        "规则实现的映射声明应当条条核得过，实际违规：{:?}",
        run.stage.violations
    );
}

// ──────────────────────────────────────────────────────────
// 判据四：生产入口不再有具体类型分支 + 产出逐字节不变
// ──────────────────────────────────────────────────────────

/// 缺陷 F2 的原话是「接第二个 agent 实现要改 `ontok-propose.rs`」。
/// 那就得先保证这个文件**里没有**可以分派具体类型的地方。
#[test]
fn test_production_entry_has_no_concrete_agent_type_branch() {
    let src = include_str!("../src/bin/ontok-propose.rs");
    let mut offenders = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let l = line.trim_start();
        // 注释行不算：模块头在解释「以前怎么写的」，那是有意留的记录
        if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') {
            continue;
        }
        for needle in ["RuleBasedAgent", "propose::extract(", ".propose("] {
            if l.contains(needle) {
                offenders.push(format!("{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "生产入口出现了具体类型/固有方法调用（应只经 build_agent + orchestrate）：\n{}",
        offenders.join("\n")
    );
}

// ──────────────────────────────────────────────────────────
// 判据四之二：REQ-020 —— 能造「假溯源 Proposal」的入口不许导出
// ──────────────────────────────────────────────────────────

/// 找一行**函数声明**（跳过注释行），返回 `true` 表示它带 `pub` 导出。
///
/// 为什么要跳注释：这几个函数的文档里**大量**引用它们自己的名字
/// （「本函数把 agent 原样写进 extracted_by…」），按裸文本搜会全中。
fn declares_pub_fn(src: &str, needle: &str) -> Option<bool> {
    for line in src.lines() {
        let l = line.trim_start();
        if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') {
            continue;
        }
        if l.contains(needle) {
            // 判「导出」要看**首 token 是不是裸 `pub`**：`pub(crate)` 的首 token
            // 是 `pub(crate)`，不是 `pub`。用 `starts_with("pub")` 会把
            // `pub(crate)` 一并算成导出，那这条判据就永远红/永远绿都没意义。
            let first = l.split_whitespace().next().unwrap_or("");
            return Some(first == "pub");
        }
    }
    None
}

/// ★ REQ-020：两个入口**不许**出现在 crate 的公开 API 上。
///
/// 这两个函数的共同形状是：把**调用方给的任意字符串**盖进
/// `proposal.extracted_by`，而 `items` 永远由**本地规则管线**产出。
/// 于是从 crate 外调用它们就能造出与 `items` 真实来源不一致的 `Proposal` ——
/// QA 实测 `extract(&report, &names, "p", "qa-outsider-agent", "9.9.9")`
/// 会把 `qa-outsider-agent` 写进 `extracted_by` 而全程不过能力闸门。
///
/// 所以收紧**可见性**，让这条路在**编译期**就不成立：
///
/// - `propose::extract_with_context` —— REQ-020 前是 `pub`，但实测
///   crate 外**零调用点**（生产两处都在 crate 内），收窄零成本；
/// - `RuleBasedAgent::propose` —— REQ-015 起生产就不调（生产走
///   `build_agent` + `orchestrate`），本 REQ 直接 `#[cfg(test)]`，
///   生产构建里这个方法**根本不存在**。
///
/// # 为什么不把 `propose::extract` 也一并断言掉
///
/// 它**必须**暂时留 `pub`：`tests/drop_rate.rs` 与 `tests/test_fk_naming.rs`
/// 两个**由他人维护、本 REQ 不得改动**的集成测试从 crate 外调用它
/// （丢弃率 / 外键命名的回归门禁）。那是 REQ-020 的**未闭合项**，
/// 已在 `src/extract/propose.rs` 的文档里记录在案 ——
///
/// **故意不在此断言 `extract` 是 `pub` 或不是 `pub`**：哪天那两个文件
/// 的所有权交回来、这条判据该被撤掉，而不是反过来把它钉死成「必须 pub」。
#[test]
fn test_proposal_forging_entries_are_not_exported_from_the_crate() {
    let mut offenders = Vec::new();

    match declares_pub_fn(include_str!("../src/extract/propose.rs"), "fn extract_with_context(") {
        None => offenders.push("src/extract/propose.rs：找不到 extract_with_context 的声明 —— 是不是被改名了？"),
        Some(true) => offenders.push(
            "src/extract/propose.rs：extract_with_context 又是 pub 了 —— 它会把调用方给的 \
             agent/agent_version 原样写进 extracted_by，crate 外拿得到就等于留了一台盖章机",
        ),
        Some(false) => {}
    }

    match declares_pub_fn(include_str!("../src/agent/rule_based.rs"), "fn propose(&self") {
        None => offenders.push("src/agent/rule_based.rs：找不到 RuleBasedAgent::propose 的声明 —— 是不是被改名了？"),
        Some(true) => offenders.push(
            "src/agent/rule_based.rs：RuleBasedAgent::propose 又是 pub 了 —— 构造实例 + 自带随手捏的身份 \
             就能盖章，而能力闸门只装在 orchestrate 上",
        ),
        Some(false) => {}
    }

    assert!(
        offenders.is_empty(),
        "能造假溯源的入口被重新导出了：\n{}",
        offenders.join("\n")
    );
}

/// `RuleBasedAgent::propose` 在**生产构建**里必须不存在。
///
/// 与上一条互补：上一条只看可见性，这一条要求它整段被 `#[cfg(test)]` 拿掉。
/// 理由是它自 REQ-015 起就没有生产调用方（生产走 `build_agent` + `orchestrate`），
/// 留着它在生产二进制里既是死代码，也是一台现成的「捏身份 + 盖章」机。
#[test]
fn test_rule_based_propose_is_absent_from_production_builds() {
    let src = include_str!("../src/agent/rule_based.rs");
    let mut offenders = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let l = line.trim_start();
        if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') {
            continue;
        }
        if l.contains("fn propose(&self") {
            // 声明行的**上一行**必须带 `#[cfg(test)]`。
            let guarded = src
                .lines()
                .take(i)
                .filter(|p| {
                    let t = p.trim();
                    !t.is_empty() && !t.starts_with("//")
                })
                .last()
                .map(|p| p.trim() == "#[cfg(test)]")
                .unwrap_or(false);
            if !guarded {
                offenders.push(format!("{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "RuleBasedAgent::propose 又回到生产构建里了（应整体 #[cfg(test)]）：\n{}",
        offenders.join("\n")
    );
}

/// 编排入口的签名**必须**是 `&dyn ExtractionAgent` —— 参数写成具体类型
/// 或泛型，调用方就有机会传进任何东西，分支就会回来。
#[test]
fn test_orchestrate_takes_a_trait_object_not_a_concrete_type() {
    let src = include_str!("../src/agent/orchestrate.rs");
    let sig = src
        .lines()
        .find(|l| l.trim_start().starts_with("agent: &"))
        .expect("找得到 orchestrate 的 agent 形参");
    assert!(
        sig.contains("&dyn ExtractionAgent"),
        "agent 形参必须是 &dyn ExtractionAgent，实际：{sig}"
    );
}

/// 加了分组编排之后，提案**逐字节不变** —— 这是本次改动最大的风险点。
#[test]
fn test_orchestration_output_is_byte_identical_to_legacy_full_pass() {
    let report = schema();
    let n = names();
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), n.clone(), 1).unwrap();
    let input = OrchestrationInput::new(&n, "extracted");
    let orchestrated = orchestrate(&*agent, &report, &input).unwrap().outcome;

    let legacy = extract(&report, &n, "extracted", "mcode-agent", "2026.10.07").unwrap();

    assert_eq!(
        serde_json::to_string_pretty(&orchestrated.proposal).unwrap(),
        serde_json::to_string_pretty(&legacy.proposal).unwrap(),
        "分组编排后的 proposal.json 必须与既有全量路径逐字节一致"
    );
    assert_eq!(
        serde_json::to_string_pretty(&orchestrated.stats).unwrap(),
        serde_json::to_string_pretty(&legacy.stats).unwrap(),
        "统计口径必须不变（全局作用域，不是组级求和）"
    );
}

/// 组级统计**不进** `stats.json` —— 组级口径冒充全局口径就是 `group.rs`
/// 里点名的「单组统计当场说谎」。这里钉住：分组后 `tables_seen` 仍是全库。
#[test]
fn test_global_stats_keep_global_scope_after_grouping() {
    let report = schema();
    let n = names();
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), n.clone(), 1).unwrap();
    let input = OrchestrationInput::new(&n, "extracted");
    let run = orchestrate(&*agent, &report, &input).unwrap();

    assert_eq!(run.stage.groups, 3, "确实按表分了 3 组");
    assert_eq!(
        run.outcome.stats.tables_seen,
        report.tables.len(),
        "全局 tables_seen 仍是全库作用域，不能变成组级数字"
    );
    assert_eq!(
        run.outcome.proposal.extracted_by.failed_groups.len(),
        0,
        "规则实现恒成功 ⇒ failed_groups 为空（序列化结果与 REQ-013 之前一致）"
    );
}

/// CLI 端到端：真实二进制跑出来的**内容**与既有全量路径逐字节一致。
///
/// ⚠️ REQ-059 起「逐字节一致」**只对 `items` 成立**，`extracted_by` 的
/// `agent` / `version` 两个字段**必然不同** —— 因为 CLI 默认路径过去记的是
/// 调用方随手填的占位（`mcode-agent`），而这一条真正跑的是进程内规则实现。
/// 把那两个字段也按「一致」断言，等于把「产物里躺着一个假名」钉成契约。
#[test]
fn test_cli_end_to_end_output_is_byte_identical_to_legacy_extract() {
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req015")
        .join("cli-e2e")
        .join(format!("{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let schema_path = dir.join("schema.json");
    let names_path = dir.join("names.json");
    std::fs::write(&schema_path, serde_json::to_string_pretty(&schema()).unwrap()).unwrap();
    std::fs::write(&names_path, NAMES_JSON).unwrap();

    let out = dir.join("proposal.json");
    let stats = dir.join("stats.json");
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-propose"))
        .args([
            "--schema",
            schema_path.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stats",
            stats.to_str().unwrap(),
            "--project",
            "extracted",
        ])
        .output()
        .expect("跑 ontok-propose 失败");
    assert!(
        o.status.success(),
        "ontok-propose 失败：{}",
        String::from_utf8_lossy(&o.stderr)
    );

    let n = names();
    let legacy = extract(&schema(), &n, "extracted", "mcode-agent", "2026.10.07").unwrap();

    // 统计**完全**不受 REQ-059 影响（它不记归属）—— 整份逐字节比。
    assert_eq!(
        std::fs::read_to_string(&stats).unwrap(),
        serde_json::to_string_pretty(&legacy.stats).unwrap()
    );

    let got: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();

    // 提案的**内容**逐字节一致：`items` 一个字节都不许动。
    assert_eq!(
        got["items"],
        serde_json::to_value(&legacy.proposal.items).unwrap(),
        "★ items 是提案的实质内容，REQ-059 只改「记什么」，不许改「抽什么」"
    );
    assert_eq!(got["round"], serde_json::to_value(&legacy.proposal.round).unwrap());
    assert_eq!(
        got["extracted_by"]["source"],
        serde_json::to_value(&legacy.proposal.extracted_by.source).unwrap()
    );
    assert_eq!(
        got["extracted_by"]["failed_groups"],
        serde_json::to_value(&legacy.proposal.extracted_by.failed_groups).unwrap()
    );
    assert_eq!(got["extracted_by"]["round"], serde_json::json!(1));

    // 而归属两栏必须记**执行者**，不是那个随手填的占位。
    assert_eq!(
        got["extracted_by"]["agent"],
        serde_json::json!(ontok::agent::factory::INPROC_EXECUTOR_ID),
        "★ 默认路径的 extracted_by.agent 必须是真正跑起来的那个实现的自报名"
    );
    assert_eq!(
        got["extracted_by"]["version"],
        serde_json::json!(ontok::agent::factory::INPROC_EXECUTOR_VERSION),
        "★ extracted_by.version 必须是执行者自己的版本，\
         **不许**再借用翻译表的版本号（REQ-059 删掉了那个顶替）"
    );
    // 阶段 1/2 的执行结果必须被报出来（设计点⑤：不静默跳过）
    let so = String::from_utf8_lossy(&o.stdout);
    assert!(
        so.contains("阶段1/2"),
        "CLI 必须报出阶段 1/2 的分组执行结果，实际 stdout：{so}"
    );
}

/// CLI 的 registry 路径与默认路径：**内容**逐字节一致，**归属**可分辨
/// （REQ-013 的既有判据；REQ-059 把「整份逐字节一致」收窄成「`items` 一致」）。
///
/// 收窄的理由见下面注释：REQ-059 修掉的正是「默认路径与 registry 路径产出**完全一样**
/// 的归属」这件事 —— 它们过去一样，是因为默认路径记的是编造占位。
/// 一致性继续只对**与身份来源无关的内容**断言，那才是本判据真正要护的东西。
#[tokio::test]
async fn test_cli_default_and_registry_paths_remain_byte_identical() {
    let pool = common::test_pool().await;
    common::migrate(&pool).await;
    let registry = ontok::db::PgAgentRegistry::new(&pool);
    registry
        .register(&reg("mcode-agent", "2026.10.07", "inproc"))
        .await
        .expect("登记 agent");

    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req015")
        .join("cli-paths")
        .join(format!("{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&dir).unwrap();
    let schema_path = dir.join("schema.json");
    let names_path = dir.join("names.json");
    std::fs::write(&schema_path, serde_json::to_string_pretty(&schema()).unwrap()).unwrap();
    std::fs::write(&names_path, NAMES_JSON).unwrap();

    let run = |extra: &[&str], tag: &str| -> String {
        let out = dir.join(format!("proposal-{tag}.json"));
        let stats = dir.join(format!("stats-{tag}.json"));
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-propose"));
        cmd.args([
            "--schema",
            schema_path.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stats",
            stats.to_str().unwrap(),
        ])
        .args(extra)
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect("缺 ONTOK_TEST_DATABASE_URL"),
        );
        let o = cmd.output().expect("跑 ontok-propose 失败");
        assert!(
            o.status.success(),
            "ontok-propose {:?} 失败：{}",
            extra,
            String::from_utf8_lossy(&o.stderr)
        );
        std::fs::read_to_string(&out).unwrap()
    };

    let default_path = run(&[], "legacy");
    let via_registry = run(
        &[
            "--agent-registry",
            "mcode-agent",
            "--agent-registry-version",
            "2026.10.07",
        ],
        "registry",
    );

    // ⚠️ REQ-059 推翻了本测试原先的前提。
    //
    // 原判据是「registry 登记成与默认值一致时，两条路径的 proposal.json 逐字节相同」
    // —— 而「默认值一致」的前提正是**默认路径记的是一个编造占位**
    // （`mcode-agent` / 翻译表版本 `1.2.0`）。REQ-059 修掉的恰恰是这一点：
    // 默认路径现在记**真正跑起来的那个实现**（`inproc:…` / 本 crate 版本），
    // registry 路径记**库里登记的**身份。两边**理应不同**。
    //
    // 保留下来的是这条判据真正想护住的东西：**REQ-013 的 registry 改造
    // 没有改动抽取行为** —— 所以逐字节一致的判据从「整份提案」收窄成
    // 「提案的**内容** `items` + 轮次 + 失败组」，这三样与身份来源无关。
    let default_v: serde_json::Value = serde_json::from_str(&default_path).unwrap();
    let reg_v: serde_json::Value = serde_json::from_str(&via_registry).unwrap();

    assert_eq!(
        default_v["items"], reg_v["items"],
        "★ registry 路径与默认路径的 items 必须逐字节一致 —— 这次重构不许改抽取行为"
    );
    assert_eq!(default_v["round"], reg_v["round"]);
    assert_eq!(
        default_v["extracted_by"]["source"], reg_v["extracted_by"]["source"]
    );
    assert_eq!(
        default_v["extracted_by"]["failed_groups"], reg_v["extracted_by"]["failed_groups"]
    );

    // 而归属两栏**必须不同**，且各自说清自己记的是什么。
    assert_eq!(
        default_v["extracted_by"]["agent"],
        serde_json::json!(ontok::agent::factory::INPROC_EXECUTOR_ID),
        "默认路径必须记真正跑起来的那个实现"
    );
    assert_eq!(
        reg_v["extracted_by"]["agent"],
        serde_json::json!("mcode-agent"),
        "registry 路径必须记库里登记的那个名字"
    );
    assert_ne!(
        default_v["extracted_by"]["agent"], reg_v["extracted_by"]["agent"],
        "★ 两条路径的 agent 必须可分辨 —— 相同就说明默认路径又在冒充 registry 的登记值"
    );
    assert_ne!(
        default_v["extracted_by"]["version"], reg_v["extracted_by"]["version"],
        "★ 两条路径的 version 必须可分辨 —— 默认路径不许再借用 registry 登记的版本"
    );
}

/// `drive_group` 仍然被编排用着（编排层的回归护栏：
/// 如果哪天有人把编排改成自己 `for` 一遍 `extract_group`，这条会先响）。
#[test]
fn test_orchestration_source_calls_drive_group_and_verify_mappings() {
    let src = include_str!("../src/agent/orchestrate.rs");
    let body = src
        .split("pub fn orchestrate")
        .nth(1)
        .expect("找得到 orchestrate 函数体");
    assert!(
        body.contains("drive_group("),
        "编排必须调 drive_group（重试与显式失败是它的职责）"
    );
    assert!(
        body.contains("verify_mappings("),
        "编排必须调 verify_mappings（阶段 2 是代码侧红线，不可外包）"
    );
    assert!(
        body.contains("agent.failed_groups()"),
        "编排必须读 agent.failed_groups()（不读就是死方法）"
    );
    // 编排里不许出现 `extract_group(` 的直接调用 —— 那是绕开重试的路
    let direct: Vec<&str> = src
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with('*') && t.contains(".extract_group(")
        })
        .collect();
    assert!(
        direct.is_empty(),
        "编排不得绕过 drive_group 直接调 extract_group：{direct:?}"
    );
}

// ──────────────────────────────────────────────────────────
// 判据五：零 I/O 纪律没被破（agent 层不许有 sqlx）
// ──────────────────────────────────────────────────────────

#[test]
fn test_agent_modules_have_no_direct_db_access() {
    let dir = crate_root().join("src/agent");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("读不到 src/agent").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let l = line.trim_start();
            if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') {
                continue;
            }
            for needle in ["sqlx::", "PgPool", "db::agent_registry", "db::pool"] {
                if l.contains(needle) {
                    offenders.push(format!("{}:{}: {line}", path.display(), i + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "src/agent/ 出现直接 DB 访问（装配层的事，抽取器不该自己连库）：\n{}",
        offenders.join("\n")
    );
}

// ──────────────────────────────────────────────────────────
// 判据六：REQ-019 · 假溯源守卫（**挡住**，不是造对）
// ──────────────────────────────────────────────────────────

/// **第三方模型 agent**：候选提得很多，但**声明不了** `produces_proposal`
/// —— 阶段 3+4 只会用本地规则管线产出 items，盖不上它的名。
///
/// 这正是 QA 自造的那个 `VendorModelAgent` 的等价物：
/// 声明 8 条违规候选，产出却与规则 agent 逐字节相同。
struct VendorModelAgent {
    identity: AgentIdentity,
}

impl VendorModelAgent {
    fn new(agent: &str) -> Self {
        VendorModelAgent {
            identity: AgentIdentity {
                agent: agent.to_string(),
                version: "2026.10.08".into(),
                round: 2,
            },
        }
    }
}

impl ExtractionAgent for VendorModelAgent {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    /// 只声明「分组抽取」—— 它确实做到了。**没有**声明产出即提案。
    fn capabilities(&self) -> Vec<String> {
        vec!["group_extract".to_string()]
    }
    fn extract_group(&self, g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        let table = g.schema[0].name.clone();
        Ok(GroupExtraction {
            links: Vec::new(),
            types: vec![CandidateType {
                key: table.clone(),
                display_name: "模型编出来的类型".into(),
                is_new: true,
                properties: (0..8)
                    .map(|i| CandidateProperty {
                        name: format!("编出来的属性{i}"),
                        source: ColumnRef::new(&table, "no_such_column"),
                        nullable: None,
                        note: None,
                        origin: None,
                    })
                    .collect(),
                description: None,
                aliases: Vec::new(),
                evidence: Vec::new(),
                rejected_comments: None,
            }],
        })
    }
}

/// ★ 核心判据：**产不出提案的 agent 一律被拒绝**，而不是发一份盖了错章的提案。
///
/// 拿掉 `require_proposal_producing(agent)?` 这一行 ⇒ `orchestrate` 返回
/// `Ok` 且 items 是规则产出的、`extracted_by.agent` 是 `vendor-model`
/// —— 假溯源成立，本测试立刻失败。
#[test]
fn test_orchestrate_refuses_agent_that_cannot_produce_the_proposal() {
    let report = schema();
    let n = names();
    let agent = VendorModelAgent::new("vendor-model");
    let input = OrchestrationInput::new(&n, "extracted");

    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("产不出提案的 agent 必须被拒绝 —— 不许发溯源是假的提案");
    let msg = format!("{err:#}");

    // 拒绝的信息必须说人话：为什么拒 / 缺什么 / 现在声明了什么 / 怎么才能跑通
    assert!(msg.contains("vendor-model"), "要指名道姓哪个 agent 被拒，实际：{msg}");
    assert!(
        msg.contains(CAPABILITY_PRODUCES_PROPOSAL),
        "要说清缺哪个能力，实际：{msg}"
    );
    assert!(
        msg.contains("group_extract"),
        "要回显它**实际**声明了什么，让人能看出缺的是哪一项，实际：{msg}"
    );
    assert!(
        msg.contains("静默") && msg.contains("failed_groups"),
        "要说清放行的后果（假溯源 + 掩盖抽取失败），实际：{msg}"
    );
    // ★ REQ-022：第二条路现在**已经存在**了（重建路径），
    // 所以这里盯的不再是「有没有说 RN-2」，而是**两条路各自的落地办法**：
    // 规则实现声明 `produces_proposal`；自建实现把契约补齐后声明 `rebuilds_proposal`。
    // 消息里若继续写「属 RN-2 范围」，就是在指向一件已经做完的事 ——
    // 过期指引比没有指引更糟（人会照着去找一条不存在的路）。
    assert!(
        msg.contains("inproc") && msg.contains(CAPABILITY_REBUILDS_PROPOSAL),
        "要给**两条可执行的路**（换规则实现 / 契约补齐后走重建路径），实际：{msg}"
    );
}

/// 被拒时**一行业务代码都没跑**：失败组一个都没产生、提案根本没生成。
///
/// 这条钉的是「闸门在阶段 3+4 **之前**」—— 若有人把闸门挪到产完之后
/// 再校验，`stage` 就会先跑一遍（本测试会看到 groups/groups_ok）。
#[test]
fn test_refusal_happens_before_any_extraction_work_is_done() {
    let report = schema();
    let n = names();
    let agent = VendorModelAgent::new("vendor-model");
    let input = OrchestrationInput::new(&n, "extracted");

    let err = orchestrate(&agent, &report, &input).err().expect("闸门必须拒绝");
    let msg = format!("{err:#}");
    assert!(
        !msg.contains("阶段 3+4 全量组装失败"),
        "闸门必须早于阶段 3+4，实际错误链：{msg}"
    );
}

/// 生产路径没被闸门误伤：规则实现声明了那个能力，且编排跑得通。
#[test]
fn test_rule_based_agent_is_admitted_by_the_gate() {
    let report = schema();
    let n = names();
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), n.clone(), 1).unwrap();
    assert!(
        agent
            .capabilities()
            .iter()
            .any(|c| c == CAPABILITY_PRODUCES_PROPOSAL),
        "规则实现必须声明 produces_proposal —— 它的产出**就是** items（否则生产路径直接不可用）"
    );
    let input = OrchestrationInput::new(&n, "extracted");
    let run = orchestrate(&*agent, &report, &input).expect("规则实现应当过闸");
    assert_eq!(run.stage.groups_ok, 3);
}

/// ★ 判据是**能力标签**，不是 `protocol`。
///
/// 源级检查：`orchestrate.rs` 的**代码行**里不许出现 `"inproc"`、
/// `protocol`、`RuleBasedAgent` —— 按协议硬编码等于把「哪个协议碰巧能用」
/// 焊进代码，而这条判据问的是「items 是不是这个实现自己产的」这个实现层事实。
#[test]
fn test_gate_is_data_driven_not_keyed_on_protocol_or_concrete_type() {
    let src = include_str!("../src/agent/orchestrate.rs");
    // 闸门的**判定段** = 函数开头到 `anyhow::bail!` 之前。
    // （bail! 之后是给人看的错误消息，里面当然会提到 `protocol="inproc"` ——
    //   那是「怎么才能跑通」的指引，不是分派分支。两者必须分开看。）
    let gate = src
        .split("fn require_proposal_producing")
        .nth(1)
        .expect("找得到闸门函数")
        .split("anyhow::bail!")
        .next()
        .expect("闸门函数里有 bail!");

    let mut offenders = Vec::new();
    for (i, line) in gate.lines().enumerate() {
        let l = line.trim_start();
        if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') || l.is_empty() {
            continue;
        }
        for needle in ["\"inproc\"", "protocol", "RuleBasedAgent", "agent as "] {
            if l.contains(needle) {
                offenders.push(format!("判定段 {}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "闸门必须只认能力标签，不能按协议/具体类型分派：\n{}",
        offenders.join("\n")
    );
    assert!(
        gate.contains("capabilities()"),
        "闸门必须问 agent 的能力标签"
    );

    // 整个编排模块（代码行）里也不该有 `protocol` 字段的读取 ——
    // 判据是数据，不是登记表的某列。
    let mut elsewhere = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let l = line.trim_start();
        if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') || l.is_empty() {
            continue;
        }
        if l.contains(".protocol") || l.contains("registration") {
            elsewhere.push(format!("{}: {}", i + 1, line.trim()));
        }
    }
    assert!(
        elsewhere.is_empty(),
        "编排不得读登记表的 protocol/registration 来分派：\n{}",
        elsewhere.join("\n")
    );
}

/// 能力闸门是真的接在 `orchestrate` 入口上的（回归护栏）。
#[test]
fn test_orchestrate_calls_the_capability_gate_first() {
    let src = include_str!("../src/agent/orchestrate.rs");
    let body = src
        .split("pub fn orchestrate")
        .nth(1)
        .expect("找得到 orchestrate 函数体");
    assert!(
        body.contains("require_proposal_producing(agent)?"),
        "编排入口必须先过能力闸门"
    );
}

// ──────────────────────────────────────────────────────────
// 判据七：REQ-019 · 重试在生产路径上真的开着
// ──────────────────────────────────────────────────────────

/// 默认尝试次数 **> 1**。钉 1 等于在生产路径上关掉规格 §7.1 设计点⑤。
#[test]
fn test_default_attempts_is_greater_than_one() {
    let n = names();
    let input = OrchestrationInput::new(&n, "extracted");
    assert_eq!(input.attempts, DEFAULT_ATTEMPTS, "默认值必须来自常量，不是各处硬编码");
    assert!(
        input.attempts > 1,
        "默认必须 >1 —— 规格要求「重试 1–2 次，仍失败才标记抽取失败」，默认 1 等于零重试"
    );
}

/// 生产入口**不许**再把 attempts 钉死 —— 上一轮正是这一行让重试等于关闭。
#[test]
fn test_production_entry_no_longer_pins_attempts_to_one() {
    let src = include_str!("../src/bin/ontok-propose.rs");
    let mut offenders = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let l = line.trim_start();
        if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') {
            continue;
        }
        if l.contains("attempts = 1") || l.contains("attempts: u32 = 1") {
            offenders.push(format!("{}: {}", i + 1, line.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "生产入口不许把 attempts 钉成 1（重试在生产路径上就等于关闭）：\n{}",
        offenders.join("\n")
    );
}

// ──────────────────────────────────────────────────────────
// 判据八：REQ-019 · key 双向核对在生产上真的跑得到
// ──────────────────────────────────────────────────────────

/// ★ 第 2 轮：清单非空 ⇒ `ExistingKeyDeclaredAsNew` / `ExistingDisplayNameChanged`
/// 从「不可达」变成「真的会报」。
///
/// 同一个 agent、同一份 schema，只改 `existing_types`：
/// 第 1 轮（清单空）0 违规，第 2 轮（基版本里 `t_dev` 的显示名不同）报改名。
/// 这条同时钉住 `build_groups` 真的把清单传进了每一组。
#[test]
fn test_round_two_existing_types_make_key_checks_reachable() {
    let report = schema();
    let n = names();
    let agent = build_agent(&reg("mcode-agent", "2026.10.07", "inproc"), n.clone(), 1).unwrap();

    // 第 1 轮：没有基版本 ⇒ 清单为空 ⇒ key 全部算「新类型」，无违规
    let mut input = OrchestrationInput::new(&n, "extracted");
    let run1 = orchestrate(&*agent, &report, &input).unwrap();
    assert!(
        run1.stage.violations.is_empty(),
        "清单为空时不该有 key 违规，实际：{:?}",
        run1.stage.violations
    );

    // 第 2 轮：基版本里 t_dev 的显示名是「设备台账」，这一轮抽出来是别的名字
    input.existing_types = vec![ExistingType {
        key: "t_dev".into(),
        display_name: "设备台账".into(),
    }];
    let run2 = orchestrate(&*agent, &report, &input).unwrap();

    let renamed: Vec<&GroupViolation> = run2
        .stage
        .violations
        .iter()
        .filter(|v| {
            matches!(
                v.violation,
                MappingViolation::ExistingDisplayNameChanged { .. }
            )
        })
        .collect();
    assert_eq!(
        renamed.len(),
        1,
        "第 2 轮 key 被改显示名必须被检出（REQ-014 的双向核对，生产上第一次真跑），实际：{:?}",
        run2.stage.violations
    );
    assert_eq!(renamed[0].group_id, "table:t_dev");
    assert_eq!(renamed[0].table, "t_dev");
    match &renamed[0].violation {
        MappingViolation::ExistingDisplayNameChanged { type_key, declared, known } => {
            assert_eq!(type_key, "t_dev");
            assert_eq!(known, "设备台账", "基版本里的显示名要如实回显");
            assert_ne!(declared, known, "两个名字确实不同才报得出");
        }
        other => panic!("不该是别的违规：{other:?}"),
    }

    // 显示名一致 ⇒ 同一份清单不该报改名（阴性对照：别把闸门写成永远报警）
    input.existing_types[0].display_name = extract(&report, &n, "extracted", "mcode-agent", "2026.10.07")
        .unwrap()
        .proposal
        .items
        .iter()
        .find(|i| i["kind"] == "add_entity_type" && i["payload"]["key"] == json!("t_dev"))
        .expect("提案里应有 t_dev")["payload"]["display_name"]
        .as_str()
        .unwrap()
        .to_string();
    let run3 = orchestrate(&*agent, &report, &input).unwrap();
    assert!(
        run3.stage.violations.is_empty(),
        "显示名一致时不该报改名，实际：{:?}",
        run3.stage.violations
    );
}
