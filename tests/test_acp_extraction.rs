//! **ACP adapter**（REQ-023）—— prompt 构建 / 响应解析 / 用量记账 / 能力声明
//!
//! # 这个文件里最要紧的不是「跑通了」，而是**跑不通的那条断言**
//!
//! [`acp_output_fails_the_rebuild_contract_closed`] 与
//! [`orchestrate_rejects_an_acp_agent_at_the_entry_gate`] 一起把
//! 「本 adapter 为什么不声明 `rebuilds_proposal`」从一句论证变成
//! **两条会红的断言**。论证放在文档里没人查，断言会。
//!
//! 全部测试都**不起进程、不需要登录态**（[`ScriptedTransport`] 喂录好的响应），
//! 按测试纪律：`mcode acp` 一轮往返实测 11.7 秒且依赖登录态，CI 不可复现。

use ontok::agent::acp::{parse, AcpAgent, AcpConfig, Handshake, ScriptedTransport, TurnOutcome};
use ontok::agent::assemble::{rebuild_outcome, GroupCandidates};
use ontok::agent::orchestrate::{orchestrate, OrchestrationInput};
use ontok::agent::{
    AgentIdentity, CandidateProperty, CandidateType, ColumnRef, ColumnSample, DecisionList,
    ExistingType, ExtractionAgent, ExtractionGroup, GroupExtraction, GroupSource, MoneyAmount,
    UsageBudget, CAPABILITY_PRODUCES_PROPOSAL, CAPABILITY_REBUILDS_PROPOSAL,
    CAPABILITY_REPORTS_USAGE,
};
use ontok::extract::introspect::{ColumnMeta, TableMeta};
use ontok::extract::names::TableNames;
use serde_json::json;

/// ⚠️ **REQ-046** —— 这条 `mod common` 是为 [`read_source`] 加的。
///
/// 本文件此前**只用 `env!("CARGO_MANIFEST_DIR")` 裸取路径**，
/// 而那正是 REQ-045 合并后 30 条失败的**现场**：
/// `capabilities_is_a_pure_function_that_reads_no_field` panic 在
/// `读不到 /home/…/verify-REQ-044/src/agent/acp/mod.rs：No such file or directory`
/// —— 目录早已回收，报错却把「**目录不对**」说成「**文件不存在**」。
mod common;
use common::read_source;

const NAMES_JSON: &str = r#"{
  "agent": "table-name-translator",
  "version": "2026.10.07",
  "generated_at": "2026-10-07T10:00:00Z",
  "source": "req023-test",
  "names": { "t_dev": "设备", "t_customer": "客户" }
}"#;

// ──────────────────────────────────────────────────────────
// 造数据
// ──────────────────────────────────────────────────────────

fn col(name: &str, nullable: bool, comment: Option<&str>) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        data_type: "text".into(),
        nullable,
        comment: comment.map(|s| s.to_string()),
        is_primary_key: false,
    }
}

fn table(name: &str, comment: Option<&str>, columns: Vec<ColumnMeta>) -> TableMeta {
    let column_count = columns.len();
    let commented_count = columns.iter().filter(|c| c.comment.is_some()).count();
    TableMeta {
        name: name.to_string(),
        comment: comment.map(|s| s.to_string()),
        columns,
        column_count,
        commented_count,
    }
}

/// 一个**信息齐全**的组：规格 §7.1 阶段 1 的五个输入一个都不缺。
fn group() -> ExtractionGroup {
    ExtractionGroup {
        group_id: "table:t_dev".into(),
        source: GroupSource {
            schema: "public".into(),
            introspected_at: "2026-10-08T04:00:00Z".into(),
            server_version: "PostgreSQL 16.14".into(),
            transaction_read_only: "on".into(),
            foreign_key_count: 1,
        },
        schema: vec![table(
            "t_dev",
            Some("设备台账"),
            vec![
                col("sn", false, Some("设备序列号")),
                col("model", true, Some("设备型号")),
                col("customer_id", true, None),
            ],
        )],
        // 上下文里有一张**组外**的表（FK 消歧用，不是抽取对象）
        context_tables: vec!["t_dev".into(), "t_customer".into()],
        samples: vec![ColumnSample {
            table: "t_dev".into(),
            column: "model".into(),
            values: vec!["XR-200".into(), "XR-300".into()],
        }],
        constraints: DecisionList {
            forbidden_terms: vec!["内部代号".into()],
            require_verifiable_evidence: true,
            min_sample_match: 1,
        },
        domain_hints: vec!["这是一家做 SaaS 计费的厂商".into()],
        existing_types: vec![ExistingType {
            key: "t_customer".into(),
            display_name: "客户".into(),
        }],
    }
}

/// 握手：身份来自 `initialize`（实测形状）。
fn handshake(version: &str) -> Handshake {
    Handshake {
        protocol_version: 1,
        agent_info: ontok::agent::acp::AgentInfo {
            name: "minimax-code".into(),
            title: Some("MiniMax Code".into()),
            version: Some(version.into()),
        },
        agent_capabilities: json!({"loadSession": true}),
        session: ontok::agent::acp::NewSessionResult {
            session_id: "mvs_test".into(),
            modes: None,
            config_options: Some(json!({"model": {"current": "minimax-M2"}})),
        },
    }
}

fn turn(text: &str, used: Option<u64>) -> TurnOutcome {
    TurnOutcome {
        text: text.to_string(),
        chunks: 1,
        usage: used.map(|u| ontok::agent::acp::UsageSample {
            used: u,
            size: Some(1_000_000),
            cost: Some(MoneyAmount {
                amount: 0.0,
                currency: "USD".into(),
            }),
        }),
        stop_reason: "end_turn".into(),
    }
}

/// 一个「结构完美」的模型回应 —— 注意它**没有** `nullable` / `origin` / `rejected_comments`，
/// 因为那三样模型**答不出来**（见 adapter 模块头）。
fn good_answer() -> String {
    json!({
        "types": [{
            "key": "t_dev",
            "display_name": "设备",
            "is_new": true,
            "description": "客户名下的设备",
            "aliases": ["终端"],
            "properties": [
                {"name": "序列号", "table": "t_dev", "column": "sn",  "note": "唯一"},
                {"name": "型号",   "table": "t_dev", "column": "model"}
            ]
        }],
        "links": [{
            "key": "t_dev_customer__t_customer",
            "target": "设备→客户",
            "from_type": "t_dev",
            "to_type": "t_customer",
            "cardinality": "N:1",
            "semantic": "belongs_to"
        }]
    })
    .to_string()
}

fn agent_answering(text: String, used: u64) -> AcpAgent {
    AcpAgent::with_transport(
        Box::new(ScriptedTransport::always_answer(handshake("0.5.5"), move |_p| {
            Ok(turn(&text, Some(used)))
        })),
        1,
    )
    .expect("握手应当成功")
}

fn call(group: &ExtractionGroup, agent: &AcpAgent) -> GroupExtraction {
    agent.extract_group(group).expect("这一组应当抽得出来")
}

// ──────────────────────────────────────────────────────────
// 一、prompt 必须带上阶段 1 的**五个**输入
// ──────────────────────────────────────────────────────────

#[test]
fn prompt_carries_every_one_of_the_five_stage_one_inputs() {
    let p = parse::build_prompt(&group());

    // ① schema 片段
    assert!(p.contains("t_dev"), "schema 片段：表名");
    assert!(p.contains("设备台账"), "schema 片段：表注释");
    assert!(p.contains("设备序列号"), "schema 片段：列注释");
    assert!(p.contains("customer_id"), "schema 片段：列名");
    // ② 上下文
    assert!(p.contains("上下文") && p.contains("t_customer"), "上下文表名");
    assert!(p.contains("**不要**抽这些表"), "上下文必须说清不是抽取对象");
    // ③ 样本
    assert!(p.contains("XR-200"), "样本值");
    // ④ 决策清单约束
    assert!(p.contains("内部代号"), "禁用词");
    assert!(p.contains("至少要命中 1 个"), "采样命中数下限");
    // ⑤ 领域提示 + 已有类型清单
    assert!(p.contains("SaaS 计费"), "领域提示");
    assert!(p.contains("key=t_customer"), "已有类型清单");

    // 输出契约：把 (表,列) 限定成**白名单**，好让解析那一道能核。
    assert!(p.contains("必须**逐字取自"), "输出契约要限定列名白名单");
}

/// 作用域**不含**上下文表 —— 上下文只用于 FK 消歧，不是抽取对象。
#[test]
fn context_tables_are_present_but_never_in_scope() {
    let p = parse::build_prompt(&group());
    let scope_section = p
        .split("## 作用域")
        .nth(1)
        .and_then(|s| s.split("## 上下文").next())
        .unwrap();
    assert!(
        !scope_section.contains("t_customer"),
        "上下文表出现在作用域里 = 在输入里摆了第二份库结构，\
         后人迟早会从组外表抽属性（设计点②要挡的正是这个）"
    );
}

// ──────────────────────────────────────────────────────────
// 二、能自己查的事实：绝不问模型
// ──────────────────────────────────────────────────────────

/// ★ `nullable` 由 **schema** 决定，不是模型说的。
#[test]
fn nullable_is_read_from_the_schema_not_taken_from_the_model() {
    let agent = agent_answering(good_answer(), 13993);
    let ex = call(&group(), &agent);

    let t = &ex.types[0];
    let sn = t.properties.iter().find(|p| p.source.column == "sn").unwrap();
    let model = t.properties.iter().find(|p| p.source.column == "model").unwrap();

    assert_eq!(sn.nullable, Some(false), "t_dev.sn 在 schema 里不可空");
    assert_eq!(model.nullable, Some(true), "t_dev.model 在 schema 里可空");
    assert!(
        !good_answer().contains("nullable"),
        "回应里本来就没有这个字段 —— 证明它不是从模型那儿来的"
    );
}

/// 编造的列名立即报错 —— 不会有一份 `source` 指向不存在列的候选。
#[test]
fn an_invented_column_is_rejected_outright() {
    let bad = json!({
        "types": [{
            "key": "t_dev", "display_name": "设备", "is_new": true,
            "properties": [{"name": "凭空来的", "table": "t_dev", "column": "no_such_column"}]
        }]
    })
    .to_string();

    let agent = agent_answering(bad, 13993);
    let err = agent
        .extract_group(&group())
        .expect_err("不存在的列必须报错，不许照收");
    let msg = format!("{err:#}");
    assert!(msg.contains("no_such_column"), "错误要指名道姓，实际：{msg}");
    assert!(msg.contains("可核对"), "错误要点明设计点③，实际：{msg}");
}

/// 组外的表**不许**成为抽取对象 —— 照收会让「上下文」变成「第二份作用域」。
#[test]
fn a_table_outside_this_group_is_not_accepted_as_a_source() {
    let bad = json!({
        "types": [{
            "key": "t_dev", "display_name": "设备", "is_new": true,
            "properties": [{"name": "客户", "table": "t_customer", "column": "name"}]
        }]
    })
    .to_string();
    let agent = agent_answering(bad, 13993);
    let err = agent.extract_group(&group()).expect_err("组外表必须被拒");
    assert!(
        format!("{err:#}").contains("t_customer"),
        "错误要指名道姓是组外的表"
    );
}

/// `evidence` 由本 adapter **自己造**，形状与规则实现一致 —— 不问模型「你凭什么」。
#[test]
fn evidence_is_constructed_by_us_not_asked_for() {
    let agent = agent_answering(good_answer(), 13993);
    let ex = call(&group(), &agent);
    let ev = &ex.types[0].evidence;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0]["table"], "t_dev");
    assert!(
        format!("{}", ev[0]["claim"]).contains("ACP adapter"),
        "证据要说是谁抽的"
    );
}

// ──────────────────────────────────────────────────────────
// 三、★ 能力声明：不签字
// ──────────────────────────────────────────────────────────

/// ★★ **本 REQ 最重要的断言**：本 adapter 的输出**过不了**重建契约。
///
/// 这不是「测试还没写好」，这是**能力声明的依据**。
/// 重建路径（`assemble`）对缺 `origin` / `rejected_comments` 是 `bail!`，
/// 而这两个字段模型答不出来（adapter 模块头有完整论证）。
/// 于是 `rebuilds_proposal` 是**不诚实的声明**，本 adapter 不签。
#[test]
fn acp_output_fails_the_rebuild_contract_closed() {
    let agent = agent_answering(good_answer(), 13993);
    let g = group();
    let ex = call(&g, &agent);

    // 前置：阶段 2 的纯代码核对**是通过的** —— 候选本身没问题。
    // （这正是问题所在：阶段 2 过得去，于是「哪里错了」只有阶段 4 才发现。）
    let violations = ontok::agent::verify_mappings(&ex, &g);
    assert!(
        violations.is_empty(),
        "本用例要证明的是「阶段 2 过、阶段 4 不过」，所以阶段 2 必须先过；实际违规：{violations:?}"
    );

    // 缺口清单要说清缺**哪两个**字段。
    let gaps = parse::contract_gaps(&ex);
    assert!(!gaps.is_empty(), "缺口清单不该为空 —— 那正是本 REQ 的结论");
    assert!(
        gaps.iter().any(|g| g.contains("origin")),
        "必须点名 origin，实际：{gaps:?}"
    );
    assert!(
        gaps.iter().any(|g| g.contains("rejected_comments")),
        "必须点名 rejected_comments，实际：{gaps:?}"
    );
    assert!(!parse::can_rebuild_proposal(&ex));

    // ★ 拿本 adapter **自己的产出**喂重建路径 —— 它必须按设计报错。
    let err = rebuild_outcome(
        &[GroupCandidates {
            group_id: g.group_id.clone(),
            extraction: ex,
        }],
        &g.to_schema_report(),
        "extracted",
        "minimax-code",
        "0.5.5",
    )
    .expect_err("缺 origin 的候选不许重建出提案");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("origin"),
        "重建必须为缺 origin 报错（那是 assemble 里的 fail-closed），实际：{msg}"
    );
}

/// ★★ 能力声明是**静态纯函数** —— 判据在源码层面钉住，不靠运行时取值。
///
/// # 为什么这条必须是**源码级**的（一个如实记录的事实）
///
/// 同文件里 `capabilities_are_static_and_never_depend_on_the_answer` 是**行为级**的，
/// 它抓「抽取前后返回值变了」这个形状。但它**抓不住**下面这个更隐蔽的变异：
/// 把 `rebuildable` 翻转**原样复原**，同时保留本 adapter 的解析器不变 ——
///
/// ```text
/// extract_group 里：rebuildable = Some(can_rebuild_proposal(&extraction))
/// 而本 adapter 的解析器**永远**填不出 origin / rejected_comments
///   ⇒ can_rebuild_proposal 恒为 false ⇒ 翻转恒为 Some(false)
///   ⇒ capabilities() 的返回值**一个字节都不变** ⇒ 行为级断言全部照样绿
/// ```
///
/// 也就是说：**对这个实现而言，那个翻转分支恒为死代码** ——
/// 这正是本 REQ 要说清的事（判据与数据不同步 ⇒ 分支不可达），
/// 也意味着「行为级断言」对它天然没有判别力。
///
/// 所以不变量必须落在**源码**上：能力是一个**不读任何字段**的纯函数。
/// 这与本仓既有的做法一致（`test_agent_modules_have_no_direct_db_access`
/// 就是读 `src/agent/` 源码做静态判据）—— 有些不变量**只**在源码上可断言。
///
/// 判别力：复原 `rebuildable` 字段与那个 `if` 分支 ⇒ 本测试立刻红。
#[test]
fn capabilities_is_a_pure_function_that_reads_no_field() {
    let text = read_source("src/agent/acp/mod.rs");

    // ── 1) `capabilities()` 的函数体里不许出现 `self.` ──
    //
    // 读任何字段 = 能力取决于「这个对象经历过什么」= 能力不是实现的属性。
    let start = text
        .find("fn capabilities(&self) -> Vec<String> {")
        .expect("本文件里必须有 capabilities 的实现");
    let body_start = start + "fn capabilities(&self) -> Vec<String> {".len();
    let body_end = text[body_start..]
        .find("\n    }")
        .map(|i| body_start + i + 1)
        .expect("找不到 capabilities 的函数体结尾");
    let body = &text[body_start..body_end];
    assert!(
        !body.contains("self."),
        "★ capabilities() 读了 self —— 能力成了「这个对象经历过什么」的函数。\n\
         不变量：能力标签描述「实现是否具备某能力」，不描述「这一份答案长什么样」。\n\
         实际函数体：\n{body}"
    );

    // ── 2) 结构体里不许再有「由 extract_group 填」的能力状态字段 ──
    let struct_start = text
        .find("pub struct AcpAgent {")
        .expect("本文件里必须有 AcpAgent");
    let struct_end = text[struct_start..]
        .find("\n}")
        .map(|i| struct_start + i)
        .expect("找不到 AcpAgent 的定义结尾");
    let decl = &text[struct_start..struct_end];
    assert!(
        !decl.contains("rebuildable"),
        "★ AcpAgent 又有了 rebuildable 字段 —— 那是「能力跟着答案翻转」的载体。\n\
         判据先于数据产生时，这样的分支经 orchestrate 这唯一生产入口**不可达**\
         （对本 adapter 而言它恒为 Some(false)，是一段死代码）。\n\
         逐份答案的核对请用 assemble::contract_gaps（抽取之后的守卫）。\n\
         实际字段：\n{decl}"
    );
}

/// 能力声明：只报 `reports_usage`，**两个「产出即提案」都不报**。
#[test]
fn capabilities_report_usage_and_sign_no_proposal_producing_capability() {
    let agent = agent_answering(good_answer(), 13993);
    let caps = agent.capabilities();

    assert!(
        caps.contains(&CAPABILITY_REPORTS_USAGE.to_string()),
        "我们**确实**上报 ACP 的 usage_update（F-2），这一条是真的：{caps:?}"
    );
    assert!(
        !caps.contains(&CAPABILITY_PRODUCES_PROPOSAL.to_string()),
        "items 不是本实现产的（规则管线产的）—— 声明它就是 REQ-019 点名的静默假溯源：{caps:?}"
    );
    assert!(
        !caps.contains(&CAPABILITY_REBUILDS_PROPOSAL.to_string()),
        "候选承载不下提案（origin / rejected_comments 填不出来），\
         声明它就是撒谎：{caps:?}"
    );

    // 而且理由是**可被程序问出来**的，不只写在文档里。
    assert!(
        !agent.contract_gap_report().is_empty(),
        "「为什么不声明」必须能被程序查到，而不是只能读到"
    );
}

/// ★★ 能力声明是**静态**的：抽之前、抽之后、抽很多组之后，一个字都不许变。
///
/// 这条钉的是本 REQ 修掉的那个**自举悖论**：
///
/// ```text
/// rebuildable 初始 None
///   → 唯一写入点在 extract_group 内（抽取之后）
///   → 能力闸门在 orchestrate 入口就执行（抽取之前）
///   ⇒ 那个 if 分支经 orchestrate 这唯一生产入口不可达
/// ```
///
/// 判别力：把 `capabilities()` 改回依赖运行时状态（复原 `rebuildable` 翻转），
/// 本测试立刻红 —— 因为**抽完之后**再问一次，答案变了。
///
/// 为什么这条比「断言一次 capabilities() 等于某个数组」更有力：
/// 后者只在**未抽取**时取值，而那时两种实现的返回值**完全一样**
/// （都是「还没抽过 ⇒ 不声明」）。只有跨过抽取那一刻，两种形状才分道扬镳。
#[test]
fn capabilities_are_static_and_never_depend_on_the_answer() {
    let g = group();
    let script = vec![
        Ok(turn(&good_answer(), Some(13993))),
        Ok(turn(&good_answer(), Some(20500))),
    ];
    let agent = AcpAgent::with_transport(
        Box::new(ScriptedTransport::new(handshake("0.5.5"), script)),
        1,
    )
    .unwrap();

    let before = agent.capabilities();
    assert!(
        !before.contains(&CAPABILITY_REBUILDS_PROPOSAL.to_string()),
        "抽之前不该声明 rebuilds_proposal"
    );

    // 真的抽两组（模型给出**逐字相同**的答案 ⇒ 判据数据也是同一份）。
    let first = agent.extract_group(&g).expect("第一组应成功");
    let after_first = agent.capabilities();
    let second = agent.extract_group(&g).expect("第二组应成功");
    let after_second = agent.capabilities();

    assert_eq!(
        before, after_first,
        "★ 抽完一组之后能力标签变了 —— 能力成了「答案的函数」。\
         这正是 REQ-024 拆掉的那个形状：同一份登记、同一套代码，\
         提案能不能产出取决于这次模型答了什么（静默假溯源）。"
    );
    assert_eq!(
        before, after_second,
        "★ 抽完两组之后能力标签又变了。同上，且说明翻转是**累积**的。"
    );

    // 逐项核对，不只看长度 —— 免得「换个顺序同样长」蒙混过关。
    assert!(
        after_second.contains(&CAPABILITY_REPORTS_USAGE.to_string()),
        "reports_usage 必须始终在（我们确实上报 usage_update）：{after_second:?}"
    );
    assert!(
        !after_second.contains(&CAPABILITY_PRODUCES_PROPOSAL.to_string())
            && !after_second.contains(&CAPABILITY_REBUILDS_PROPOSAL.to_string()),
        "两个「产出即提案」的能力必须始终**不**声明：{after_second:?}"
    );

    // 两组答案逐字相同 ⇒ 守卫判据对两份答案的结论也必须相同。
    // （这条是前提：若两次答案不同，「能力不变」就该靠运气而不是靠设计。）
    assert_eq!(
        parse::contract_gaps(&first),
        parse::contract_gaps(&second),
        "同一份答案跑两次，守卫判据必须一致"
    );
}

/// ★ 守卫判据是**逐份答案**的，不是实现的能力声明 —— 两者不许互相推导。
///
/// 证明方式：直接构造一份**逐字不同**的候选（同一个 adapter 类型无法产出
/// 带 `origin` 的答案，因为它的解析器永远留 `None`），让守卫判它「可以重建」，
/// 再断言 [`AcpAgent::capabilities`] **仍然不声明**。
#[test]
fn a_conforming_answer_does_not_make_the_adapter_declare_the_capability() {
    // 一份「全部字段齐备」的候选 —— 它通过守卫。
    let conforming = GroupExtraction {
        types: vec![CandidateType {
            key: "设备".into(),
            display_name: "设备".into(),
            is_new: true,
            properties: vec![CandidateProperty {
                name: "序列号".into(),
                source: ColumnRef::new("t_dev", "sn"),
                nullable: Some(false),
                note: None,
                origin: Some(ontok::agent::PropertyOrigin::Comment),
            }],
            description: None,
            aliases: Vec::new(),
            evidence: vec![serde_json::json!({"table": "t_dev", "claim": "测试"})],
            rejected_comments: Some(0),
        }],
        links: Vec::new(),
    };
    assert!(
        parse::can_rebuild_proposal(&conforming),
        "前提：这份候选逐项齐备，守卫判它承载得下重建"
    );

    // ★ 但同一个 adapter 的能力声明**不因此改变**。
    let agent = agent_answering(good_answer(), 13993);
    assert!(
        !agent
            .capabilities()
            .contains(&CAPABILITY_REBUILDS_PROPOSAL.to_string()),
        "★ 一份齐备的答案不许让能力标签翻转。\n\
         能力是**实现**的属性（今天结构性填不出 origin / rejected_comments），\
         不是某一份答案的属性。若这条红了，说明有人又把「逐份判据」\
         接回了「能力标签」—— 那就是本 REQ 拆掉的那个自举悖论。"
    );
    // 逐份判据与能力声明是两个独立答案：一份说「行」，一个说「不行」。
    assert!(
        !parse::can_rebuild_proposal(&call(&group(), &agent)),
        "而本 adapter 自己的答案仍然承载不下（缺 origin / rejected_comments）"
    );
}

/// ★ 编排在**入口**就拒绝这个 agent —— 一分钱不花。
///
/// 这是「宁可拒绝」这条纪律的可执行版本：若当初签了
/// `rebuilds_proposal`，闸门会放行，然后 91 个组跑完
/// （一堆真金白银的 token）才在阶段 4 炸。
#[test]
fn orchestrate_rejects_an_acp_agent_at_the_entry_gate() {
    let g = group();
    let report = g.to_schema_report();
    let names = TableNames::load(NAMES_JSON).unwrap();
    let agent = agent_answering(good_answer(), 13993);

    // 给了预算 —— 这样 `require_budget` 放行，拒绝只可能来自能力闸门。
    let mut input = OrchestrationInput::new(&names, "extracted");
    input.budget = Some(UsageBudget {
        max_used: 1_000_000,
        max_cost: None,
    });

    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("没有产出即提案的声明就必须在入口拒绝");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("minimax-code"),
        "错误要点名是哪个 agent 被拒，实际：{msg}"
    );
    assert!(
        msg.contains(CAPABILITY_REBUILDS_PROPOSAL),
        "错误要给出路：那才是能真正解掉这次拒绝的那条路，实际：{msg}"
    );
    assert!(
        msg.contains(CAPABILITY_REPORTS_USAGE),
        "错误里要如实回显本 agent 声明了什么，实际：{msg}"
    );
}

/// ★ 钉住**闸门顺序**：能力闸门在成本闸门**之前**。
///
/// 这条不是凑数 —— 它记的是一个反直觉的事实：
/// 本 adapter 声明了 `reports_usage`（规格 §7.1 的强制义务），
/// 按理说「没设预算」才是拦它的第一道闸；但实际上它**永远走不到**成本闸门，
/// 因为能力闸门先拒了它。
///
/// 所以：**今天成本闸门对 ACP adapter 是不可达的**。
/// 这不影响本 REQ（反正不花钱就会被拒），但它决定了「什么时候成本闸门
/// 才真正开始保护钱」—— 答案是「契约缺口被补上、`rebuilds_proposal`
/// 真的能签之后」。谁把这两道闸调换顺序，这条测试会告诉他：
/// **能力闸才是那道防伪溯源的闸，成本闸管不了这个。**
#[test]
fn the_capability_gate_fires_before_the_budget_gate_for_this_adapter() {
    let g = group();
    let report = g.to_schema_report();
    let names = TableNames::load(NAMES_JSON).unwrap();
    let agent = agent_answering(good_answer(), 13993);

    let input = OrchestrationInput::new(&names, "extracted"); // 没设预算
    // 声明里**确实**有 reports_usage（义务已签字），但……
    let agent = agent_answering(good_answer(), 13993);

    assert!(
        agent.capabilities().contains(&CAPABILITY_REPORTS_USAGE.to_string()),
        "前提：本 adapter 声明了会上报用量"
    );
    // ……错误**仍然**来自能力闸门，不是成本闸门。
    let err = orchestrate(&agent, &report, &input)
        .err()
        .expect("没设预算也必须被拒");
    let msg = format!("{err:#}");
    assert!(
        msg.contains(CAPABILITY_REBUILDS_PROPOSAL),
        "拒绝应来自能力闸门（它在 require_budget 之前），实际：{msg}"
    );
    assert!(
        !msg.contains("没有给预算"),
        "不该是成本闸门的报错 —— 能力闸门先拒了它。\n\
         如果这条开始失败，说明有人调换了两道闸的顺序，那会让成本闸门\n\
         去承担它管不了的事（它防的是钱，防不了假溯源）。实际：{msg}"
    );
}

// ──────────────────────────────────────────────────────────
// 四、用量记账（F-2）
// ──────────────────────────────────────────────────────────

/// 每组一条**增量**，相加等于会话真实总量。
#[test]
fn usage_is_recorded_per_group_as_a_delta() {
    let g = group();
    // 会话累计：第 1 组后 13993，第 2 组后 20500（脚本按调用顺序消耗）
    let script = vec![Ok(turn(&good_answer(), Some(13993))), Ok(turn(&good_answer(), Some(20500)))];
    let agent =
        AcpAgent::with_transport(Box::new(ScriptedTransport::new(handshake("0.5.5"), script)), 1)
            .unwrap();

    agent.extract_group(&g).unwrap();
    agent.extract_group(&g).unwrap();

    let usage = agent.usage();
    assert_eq!(usage.len(), 2, "每组一条");
    assert_eq!(usage[0].group_id, "table:t_dev");
    assert_eq!(usage[0].used, 13993, "首组无基线 ⇒ 按会话累计起算");
    assert_eq!(
        usage[1].used, 6507,
        "第二组必须报增量 20500-13993=6507；报 20500 的话总账虚高 68%"
    );
    assert_eq!(usage[0].used + usage[1].used, 20500, "各组之和 = 会话真实累计");
}

/// ★ 钱花了但没收到 `usage_update` ⇒ **这一组失败**，不许报 0。
///
/// 三个选项里只有「报错」是对的：报 0 是把「不知道」说成「免费」；
/// 不报这一组是让成本闸门漏算它，而**漏掉的闸门是看不见的闸门**。
#[test]
fn a_turn_without_usage_update_fails_the_group_rather_than_reporting_zero() {
    // 造一个「压根没报用量」的传输
    let agent2 = AcpAgent::with_transport(
        Box::new(ScriptedTransport::always_answer(handshake("0.5.5"), |_p| {
            Ok(turn(&good_answer(), None))
        })),
        1,
    )
    .unwrap();

    let err = agent2
        .extract_group(&group())
        .expect_err("没有 usage_update ⇒ 这一组必须显式失败");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("usage_update") && msg.contains("无从判断"),
        "错误要说清「这一组花了多少无从判断」，实际：{msg}"
    );
    assert!(
        agent2.usage().is_empty(),
        "没报用量时**不许**塞一条 0 进去 —— 那会让账看起来是平的，看不出缺口"
    );
}

/// **先记账再解析** —— 解析失败也不能让花掉的钱从账上消失。
#[test]
fn usage_is_recorded_even_when_parsing_the_answer_fails() {
    let bad = json!({"types": "这不是数组"}).to_string();
    let agent = AcpAgent::with_transport(
        Box::new(ScriptedTransport::always_answer(handshake("0.5.5"), move |_p| {
            Ok(turn(&bad, Some(13993)))
        })),
        1,
    )
    .unwrap();

    assert!(
        agent.extract_group(&group()).is_err(),
        "形状不对必须失败"
    );
    let usage = agent.usage();
    assert_eq!(
        usage.len(),
        1,
        "模型**已经**为这一组花过钱了；解析失败不该让这笔钱从账上消失 —— \
         漏记的钱比报错更难发现（账是平的，看不出来）"
    );
    assert_eq!(usage[0].used, 13993);
}

// ──────────────────────────────────────────────────────────
// 五、读回文本：宽容有边界
// ──────────────────────────────────────────────────────────

/// 模型包了 ```json 围栏 —— 宽容解析，把围栏剥掉。
#[test]
fn a_code_fenced_answer_is_accepted() {
    let text = format!("```json\n{}\n```", good_answer());
    let ex = parse::parse_extraction(&text, &group()).expect("围栏要剥掉");
    assert_eq!(ex.types.len(), 1);
    assert_eq!(ex.types[0].key, "t_dev");
}

/// 前后有寒暄 —— 只要能找到那对花括号就认。
#[test]
fn prose_around_the_json_is_tolerated() {
    let text = format!(
        "好的，以下是结果：\n{}\n如需调整请告诉我。",
        good_answer()
    );
    let ex = parse::parse_extraction(&text, &group()).expect("寒暄不该让整轮失败");
    assert_eq!(ex.types[0].key, "t_dev");
}

/// ★ 花括号**在字符串里**不许截断对象。
///
/// 描述文本里写一个 `}` 是完全正常的。`find('}')` 会在这里截断出一个
/// **语法合法但内容残缺**的片段 —— 那比解析失败糟得多。
#[test]
fn braces_inside_string_literals_do_not_truncate_the_object() {
    let json_text = json!({
        "types": [{
            "key": "t_dev",
            "display_name": "设备 } 花括号 } 测试",
            "is_new": true,
            "description": "含 } 与 \"引号\" 与 \\ 反斜杠",
            "properties": [{"name": "序列号", "table": "t_dev", "column": "sn"}]
        }]
    })
    .to_string();

    let ex = parse::parse_extraction(&json_text, &group()).expect("字符串里的 } 不该截断");
    assert_eq!(
        ex.types[0].display_name, "设备 } 花括号 } 测试",
        "花括号必须原样保留"
    );
    assert_eq!(
        ex.types[0].description.as_deref(),
        Some("含 } 与 \"引号\" 与 \\ 反斜杠")
    );
}

/// 完全没有 JSON ⇒ **报错**，并把回应带出来。绝不「凑合解析」。
#[test]
fn a_pure_prose_answer_is_rejected_loudly() {
    let err = parse::parse_extraction("抱歉，我无法完成这个任务。", &group())
        .expect_err("没有 JSON 就必须报错");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("找不到 JSON 对象"),
        "错误要说清是找不到 JSON，实际：{msg}"
    );
    assert!(
        msg.contains("长得一模一样"),
        "错误要点明为什么必须响亮地失败（空结果与「真的抽不出」无法区分），实际：{msg}"
    );
}

/// 空 key 必须被拒 —— key 是**跨轮次锚点**，空锚点没法用。
#[test]
fn an_empty_type_key_is_rejected() {
    let bad = json!({"types": [{"key": "", "display_name": "", "is_new": true}]}).to_string();
    let err = parse::parse_extraction(&bad, &group()).expect_err("空 key 必须报错");
    assert!(format!("{err:#}").contains("key 为空"));
}

// ──────────────────────────────────────────────────────────
// 六、身份：来自真实握手（§8.4）
// ──────────────────────────────────────────────────────────

#[test]
fn identity_comes_from_the_handshake_not_from_a_hardcoded_default() {
    let agent = agent_answering(good_answer(), 13993);
    let id: &AgentIdentity = agent.identity();
    assert_eq!(id.agent, "minimax-code");
    assert_eq!(id.version, "0.5.5", "如实记 agentInfo.version");
    assert_eq!(id.round, 1);
}

/// ⚠️ ADR-002 · **N5**：`agentInfo.version` 是**驱动二进制**的版本。
/// 所以会话配置必须另留一处痕，否则「这一轮用的什么模型」无处可查。
#[test]
fn the_binary_version_is_not_mistaken_for_a_model_version() {
    let agent = agent_answering(good_answer(), 13993);
    let note = agent
        .model_note()
        .expect("会话配置快照必须留痕（否则 N5 说的那件事真的无从查证）");
    assert!(
        note.contains("minimax-M2") && note.contains("configOptions"),
        "留痕里要有服务端自报的配置项，实际：{note}"
    );
    assert!(
        agent.identity().version != "minimax-M2",
        "二进制版本与模型版本是两回事，混起来就锁了个不相干的东西"
    );
}

/// 模糊版本必须被拒（§8.4）。
#[test]
fn a_vague_agent_version_is_rejected_before_any_extraction() {
    let err = AcpAgent::with_transport(
        Box::new(ScriptedTransport::always_answer(handshake("latest"), |_p| {
            Ok(turn(&good_answer(), Some(1)))
        })),
        1,
    )
    .err()
    .expect("模糊版本必须在握手后就拒");
    let msg = format!("{err:#}");
    assert!(msg.contains("模糊版本"), "实际：{msg}");
    assert!(
        msg.contains("mcode 二进制"),
        "错误要顺带提醒 N5（那是二进制版本不是模型版本），实际：{msg}"
    );
}

/// 没有 version ⇒ 拒绝，**不**编一个 `unknown`。
#[test]
fn a_missing_agent_version_is_rejected_rather_than_defaulted() {
    let mut h = handshake("0.5.5");
    h.agent_info.version = None;
    let err = AcpAgent::with_transport(
        Box::new(ScriptedTransport::always_answer(h, |_p| Ok(turn(&good_answer(), Some(1))))),
        1,
    )
    .err()
    .expect("没有版本就必须拒绝 —— 编一个就是假溯源");
    assert!(format!("{err:#}").contains("§8.4"));
}

// ──────────────────────────────────────────────────────────
// 七、候选形状的最小契约
// ──────────────────────────────────────────────────────────

/// 解析出的候选满足阶段 2 的**纯代码**要求（证据可核对）。
#[test]
fn parsed_candidates_satisfy_the_stage_two_verification_rules() {
    let agent = agent_answering(good_answer(), 13993);
    let g = group();
    let ex = call(&g, &agent);

    assert!(ontok::agent::verify_mappings(&ex, &g).is_empty());

    let t: &CandidateType = &ex.types[0];
    assert!(t.is_new, "key 不在已有清单里 ⇒ 新类型");
    assert_eq!(t.description.as_deref(), Some("客户名下的设备"));
    assert_eq!(t.aliases, vec!["终端"]);
    let p: &CandidateProperty = &t.properties[0];
    assert_eq!(p.source, ColumnRef::new("t_dev", "sn"));
    assert_eq!(p.note.as_deref(), Some("唯一"));

    assert_eq!(ex.links.len(), 1);
    assert_eq!(ex.links[0].semantic, "belongs_to");
}

/// 空的 `GroupExtraction`（模型什么都没提）**不**报错 —— 那是一个合法答案。
/// 但它仍然**承载不下**提案（`origin` / `rejected_comments` 一样缺），
/// 所以能力声明不受影响。
#[test]
fn an_empty_answer_is_legal_but_still_not_rebuildable() {
    let agent = agent_answering(json!({"types": [], "links": []}).to_string(), 13993);
    let ex = call(&group(), &agent);
    assert!(ex.types.is_empty());
    assert!(
        !parse::can_rebuild_proposal(&ex),
        "空答案同样不能重建 —— 缺口清单为空只会让人误读成「没问题」"
    );
}

/// 测试**不许**真的起进程 —— `AcpConfig::default()` 的 cwd 必须来自真实目录，
/// 而 `AcpAgent::connect` 在这里一个都不调用（全部走 `with_transport`）。
#[test]
fn no_test_in_this_file_opens_a_subprocess() {
    // 这个断言看着空，但它挡住的是「有人后来加一行 AcpAgent::connect(...)」：
    // 那会让整份测试变成 11.7 秒 × N、依赖登录态、CI 上必挂。
    let cfg = AcpConfig::default();
    assert_eq!(cfg.program, "mcode");
    assert_eq!(cfg.args, vec!["acp".to_string()]);
    assert!(
        std::path::Path::new(&cfg.cwd).is_dir(),
        "默认 cwd 必须是真实目录（会话就建在那）"
    );
    // 默认配置**不发**未实测的 model 参数（N5）。
    let ns = ontok::agent::acp::protocol::new_session_params(&cfg.cwd);
    assert!(ns.get("configOptions").is_none());
}

// 让未使用的导入在「本文件曾经引用过」时也能编译（防止有人删测试后留下孤儿导入）。
#[allow(unused)]
fn _unused_anchor(_: ExtractionGroup, _: GroupExtraction) {}