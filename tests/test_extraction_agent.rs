//! REQ-013 · KH Agent 抽象接口 + agent-registry
//!
//! # 这个文件钉住什么
//!
//! | 判据 | 为什么要有 |
//! | --- | --- |
//! | **包成 agent 后提案逐字节不变** | 抽象层最大的风险是「加了层、行为漂了」。没有这条断言，漂了也没人知道 |
//! | `drive_group` 重试与显式失败 | 设计点⑤「不静默跳过」——失败必须能表达、能重试、能指名道姓 |
//! | `verify_mappings` 能查出假声明 | 设计点③「证据必须机器可验证」——声明必须是 `(表,列)` 二元组才查得了 |
//! | registry 查不到就报错 | fail-closed（`config.rs` / `introspect.rs` 同一纪律）：不许回落默认值 |
//! | 版本锁死不许多登记 | §8.4：同键重登记且内容不同必须报错，否则 diff 归因失效 |
//! | registry 源码无破坏性 SQL | 共享本体库**只允许 INSERT**——这条纪律得钉死，否则迟早有人加 TRUNCATE |
//! | `src/domain/` 零 I/O | AC-002 的 grep 判据，防止 agent 客户端/DB 访问渗进领域层 |
//! | CLI 两条路径产出逐字节一致 | 溯源改成走 registry 之后，默认路径不许悄悄变 |

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use common::{crate_root, migrate};
use common::test_pool;
use ontok::agent::group::{
    verify_mappings, CandidateProperty, CandidateType, ColumnRef, DecisionList, ExistingType,
    ExtractionGroup, GroupExtraction, GroupFailure, GroupSource,
};
use ontok::agent::registry::{is_vague_version, AgentIdentity, AgentRegistration};
use ontok::agent::{drive_group, ExtractionAgent, RuleBasedAgent};
use ontok::db::agent_registry::RegistryError;
use ontok::db::PgAgentRegistry;
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
  "source": "req013-test",
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

/// 一份小而有关系的 schema：`t_dev` 有一列 `customer_id` 指向 `t_customer`
/// （零外键库靠 `*_id` 命名约定推关系），另有一列**表里没有**的 `sn`
/// 供「假声明」测试用。
fn schema() -> SchemaReport {
    let tables = vec![
        table(
            "t_dev",
            Some("设备表"),
            vec![
                col("id", None),
                col("sn", Some("序列号")),
                col("model", Some("型号")),
                col("t_customer_id", Some("客户标识")),
            ],
        ),
        table(
            "t_customer",
            Some("客户表"),
            vec![col("id", None), col("name", Some("客户名称"))],
        ),
    ];
    SchemaReport {
        server_version: "16.14".into(),
        transaction_read_only: "on".into(),
        schema: "public".into(),
        table_count: tables.len(),
        column_count: tables.iter().map(|t| t.columns.len()).sum(),
        commented_column_count: tables.iter().map(|t| t.commented_count).sum(),
        foreign_key_count: 0,
        tables,
        introspected_at: "2026-10-07T10:00:00Z".into(),
    }
}

fn names() -> TableNames {
    TableNames::load(NAMES_JSON).expect("翻译表加载失败")
}

/// 按**表**切一组（设计点②：一表一组，不做跨表聚类）。
///
/// `context_tables` 取**全库表名** —— 这正是 REQ-014 · F1 补上的那一份：
/// 阶段 0 分组时把全局表清单一起带下来，让组外的 FK 目标仍然「真实存在」。
/// 只放名字不放 schema：上下文是**存在性判据**，不是第二份待抽的库结构。
fn group_of(
    schema: &SchemaReport,
    tables: &[&str],
    existing: Vec<ExistingType>,
) -> ExtractionGroup {
    ExtractionGroup {
        group_id: ontok::agent::derive_group_id(tables[0]),
        source: GroupSource {
            schema: schema.schema.clone(),
            introspected_at: schema.introspected_at.clone(),
            server_version: schema.server_version.clone(),
            transaction_read_only: schema.transaction_read_only.clone(),
            foreign_key_count: schema.foreign_key_count,
        },
        schema: schema
            .tables
            .iter()
            .filter(|t| tables.contains(&t.name.as_str()))
            .cloned()
            .collect(),
        context_tables: schema.tables.iter().map(|t| t.name.clone()).collect(),
        samples: Vec::new(),
        constraints: DecisionList::default(),
        domain_hints: vec!["做 SaaS 计费的厂商".into()],
        existing_types: existing,
    }
}

/// 从阶段 1 的产物里取某个 (类型, 列) 的属性名。
fn prop_name(ex: &GroupExtraction, type_key: &str, column: &str) -> String {
    ex.types
        .iter()
        .find(|t| t.key == type_key)
        .unwrap_or_else(|| panic!("候选里没有类型 {type_key}"))
        .properties
        .iter()
        .find(|p| p.source.column == column)
        .unwrap_or_else(|| panic!("{type_key} 没有列 {column} 对应的属性"))
        .name
        .clone()
}

// ──────────────────────────────────────────────────────────
// 判据一：包成 agent 之后，提案**逐字节不变**
//
// ★ REQ-020：原在这里的两条断言已迁进 crate 内的
// `src/agent/rule_based.rs` 的 `mod tests` —— 它们调用的
// `RuleBasedAgent::propose` 本 REQ 收窄成了 `pub(crate)`
// （「捏一个身份盖章」是造假溯源的机器，论证见该方法的文档）。
// 两条都是零 I/O 的纯函数断言，搬进 crate **判据强度一个字没减**。
// ──────────────────────────────────────────────────────────

// ──────────────────────────────────────────────────────────
// 判据二：抽象真的能用（trait 可 dyn 化 + 阶段 1 跑得通）
// ──────────────────────────────────────────────────────────

/// 证明它是**抽象**而不是一个只有一个实现的壳：
/// 第二个实现（一个恒失败的假 agent）能塞进同一个 trait 里。
struct AlwaysFails {
    identity: AgentIdentity,
    calls: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl ExtractionAgent for AlwaysFails {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        anyhow::bail!("模拟协议超时")
    }
}

struct FailsThenSucceeds {
    identity: AgentIdentity,
    calls: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

impl ExtractionAgent for FailsThenSucceeds {
    fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    fn extract_group(&self, _g: &ExtractionGroup) -> anyhow::Result<GroupExtraction> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            anyhow::bail!("第一次不该出现的错")
        }
        Ok(GroupExtraction {
            types: Vec::new(),
            links: Vec::new(),
        })
    }
}

#[test]
fn test_trait_is_dyn_usable() {
    let a = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());
    let dyns: Vec<Box<dyn ExtractionAgent>> = vec![
        Box::new(a),
        Box::new(AlwaysFails {
            identity: AgentIdentity::legacy_cli("vendor", "2.3.1"),
            calls: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        }),
    ];
    assert_eq!(dyns.len(), 2);
    assert_eq!(dyns[0].identity().agent, "kh-core");
    assert_eq!(dyns[1].identity().version, "2.3.1");
}

/// 设计点⑤：**失败必须显式**，不能静默跳过。
/// 恒失败的 agent ⇒ `Failed`，带组标识、表名、尝试次数、原因。
#[test]
fn test_drive_group_surfaces_failure_with_group_id_and_reason() {
    let report = schema();
    let group = group_of(&report, &["t_dev"], vec![]);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let agent = AlwaysFails {
        identity: AgentIdentity::legacy_cli("vendor-extractor", "2.3.1"),
        calls: calls.clone(),
    };

    let outcome = drive_group(&agent, &group, 2);
    let f: &GroupFailure = outcome.failure().expect("恒失败的 agent 必须报显式失败");
    assert_eq!(f.group_id, "table:t_dev", "失败必须指名道姓是哪一组");
    assert_eq!(f.table, "t_dev");
    assert_eq!(f.attempts, 2);
    assert!(
        f.reason.contains("模拟协议超时"),
        "原因必须带最后一次的真实信息，实际：{}",
        f.reason
    );
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "必须真的重试了"
    );
    assert!(outcome.ok().is_none());
}

/// 失败不参与收敛判定的载体是 `extracted_by.failed_groups`
/// —— 所以失败信息得能被序列化成提案元数据的形状。
#[test]
fn test_failed_group_serializes_into_proposal_metadata_shape() {
    let f = GroupFailure {
        group_id: "table:t_dev".into(),
        table: "t_dev".into(),
        attempts: 2,
        reason: "模拟协议超时".into(),
    };
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["group_id"], json!("table:t_dev"));
    assert_eq!(v["table"], json!("t_dev"));
    assert_eq!(v["attempts"], json!(2));
    assert_eq!(v["reason"], json!("模拟协议超时"));
}

/// 重试到成功就**不许**报失败（设计点⑤的反面：别把成功也标成失败）。
#[test]
fn test_drive_group_retries_then_succeeds() {
    let report = schema();
    let group = group_of(&report, &["t_dev"], vec![]);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let agent = FailsThenSucceeds {
        identity: AgentIdentity::legacy_cli("vendor-extractor", "2.3.1"),
        calls: calls.clone(),
    };
    let outcome = drive_group(&agent, &group, 2);
    assert!(outcome.failure().is_none(), "重试后成功不该报失败");
    assert!(outcome.ok().is_some());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
}

/// `attempts = 0` 必须被夹到 1 —— 否则「重试 0 次」等于**根本没抽过**
/// 却返回了失败，看起来像「抽过了但失败」，那是另一种静默。
#[test]
fn test_drive_group_clamps_zero_attempts_to_one() {
    let report = schema();
    let group = group_of(&report, &["t_dev"], vec![]);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let agent = AlwaysFails {
        identity: AgentIdentity::legacy_cli("vendor-extractor", "2.3.1"),
        calls: calls.clone(),
    };
    let outcome = drive_group(&agent, &group, 0);
    assert_eq!(outcome.failure().unwrap().attempts, 1);
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "至少要真的调一次，不能一次都不调就报失败"
    );
}

/// 阶段 1 的输出形状：候选类型 + 「属性 ← 来源列」映射，且**没有 Link**
/// （规格 §7.1 把 Link 放在阶段 3）。
#[test]
fn test_extract_group_emits_candidates_with_column_mappings_and_no_links() {
    let report = schema();
    let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());
    let group = group_of(&report, &["t_dev", "t_customer"], vec![]);

    let ex = agent.extract_group(&group).expect("阶段 1 抽取失败");

    let dev = ex
        .types
        .iter()
        .find(|t| t.key == "t_dev")
        .expect("t_dev 应在候选里");
    let sn = dev
        .properties
        .iter()
        .find(|p| p.source.column == "sn")
        .expect("sn 应有映射");
    assert_eq!(sn.source.table, "t_dev");
    assert_eq!(sn.name, "序列号", "属性名应来自列注释");
    assert!(sn.nullable == Some(true));

    // 关系**不许**出现在阶段 1（那是阶段 3 的活）
    let legacy = extract(&report, &names(), "p", "a", "1").unwrap();
    assert!(
        legacy
            .proposal
            .items
            .iter()
            .any(|i| i["kind"] == "add_link_type"),
        "前提：全量路径确实会产出 Link，否则这条断言没意义"
    );
    let ex_json = serde_json::to_string(&ex).unwrap();
    assert!(
        !ex_json.contains("add_link_type"),
        "阶段 1 产物里不许有 Link"
    );
}

/// 每条声明都去 schema 里核过 —— 这是设计点③「机器可验证」的意思。
#[test]
fn test_verify_mappings_accepts_real_declarations() {
    let report = schema();
    let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());
    let group = group_of(&report, &["t_dev", "t_customer"], vec![]);
    let ex = agent.extract_group(&group).unwrap();
    assert_eq!(
        verify_mappings(&ex, &group),
        Vec::<ontok::agent::group::MappingViolation>::new(),
        "规则实现产出的声明必须全部核得过 —— 核不过说明形状与 schema 对不上"
    );
}

/// 假声明必须被抓出来：**表不存在** / **列不存在** / **已有类型 key 打错** / **命中禁用词**。
///
/// 四条各自断，因为它们是四种不同的错；合成一条断言的话，
/// 变异掉其中一条会让另外三条「陪跑」成假绿。
#[test]
fn test_verify_mappings_catches_fake_declarations() {
    let report = schema();
    let group = group_of(
        &report,
        &["t_dev"],
        vec![ExistingType {
            key: "t_customer".into(),
            display_name: "客户".into(),
        }],
    );
    use ontok::agent::group::MappingViolation as V;

    // ① 表不存在
    let mut ex = GroupExtraction {
        links: Vec::new(),
        types: vec![CandidateType {
            key: "t_dev".into(),
            display_name: "设备".into(),
            is_new: true,
            properties: vec![CandidateProperty {
                name: "序列号".into(),
                source: ColumnRef::new("t_ghost", "sn"),
                nullable: None,
                note: None,
                origin: None,
            }],
            description: None,
            aliases: Vec::new(),
            evidence: Vec::new(),
            rejected_comments: None,
        }],
    };
    assert_eq!(
        verify_mappings(&ex, &group),
        vec![V::UnknownTable {
            type_key: "t_dev".into(),
            property: "序列号".into(),
            table: "t_ghost".into(),
        }]
    );

    // ② 表在、列不存在
    ex.types[0].properties[0].source = ColumnRef::new("t_dev", "no_such_column");
    assert_eq!(
        verify_mappings(&ex, &group),
        vec![V::UnknownColumn {
            type_key: "t_dev".into(),
            property: "序列号".into(),
            table: "t_dev".into(),
            column: "no_such_column".into(),
        }]
    );

    // ③ 没有来源列（声明为空 ⇒ 证据不可核对）
    ex.types[0].properties[0].source = ColumnRef::new("", "");
    assert_eq!(
        verify_mappings(&ex, &group),
        vec![V::NoSource {
            type_key: "t_dev".into(),
            property: "序列号".into(),
        }]
    );

    // ④ 声称「映射到已有类型」但 key 不在清单里
    ex.types[0].properties[0].source = ColumnRef::new("t_dev", "sn");
    ex.types[0].is_new = false;
    ex.types[0].key = "t_typo".into();
    assert_eq!(
        verify_mappings(&ex, &group),
        vec![V::UnknownExistingType {
            type_key: "t_typo".into(),
        }]
    );

    // ⑤ 命中禁用词
    let mut g2 = group.clone();
    g2.constraints.forbidden_terms = vec!["内部".into()];
    let mut ex2 = ex.clone();
    ex2.types[0].key = "t_dev".into();
    ex2.types[0].is_new = true;
    ex2.types[0].properties[0].name = "内部序列号".into();
    assert_eq!(
        verify_mappings(&ex2, &g2),
        vec![V::ForbiddenTerm {
            type_key: "t_dev".into(),
            property: "内部序列号".into(),
            term: "内部".into(),
        }]
    );
}

/// 设计点①：输入里必须有**已有类型清单**，输出里必须有「这是不是新类型」。
/// 缺了「已有类型清单」这个字段，agent 就只能自由命名（幻觉的来源）。
#[test]
fn test_agent_input_carries_existing_type_list_and_output_marks_new_type() {
    let report = schema();
    let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());

    // 组自足（清单为空）⇒ 提的类型全是**新类型**
    let g_new = group_of(&report, &["t_dev"], vec![]);
    let ex = agent.extract_group(&g_new).unwrap();
    assert!(
        ex.types.iter().all(|t| t.is_new),
        "清单为空时提的类型应全标为新类型"
    );

    // 清单里已有 t_dev ⇒ 同一张表这次是**映射到已有类型**
    let g_known = group_of(
        &report,
        &["t_dev"],
        vec![ExistingType {
            key: "t_dev".into(),
            display_name: "设备".into(),
        }],
    );
    let ex2 = agent.extract_group(&g_known).unwrap();
    let dev = ex2.types.iter().find(|t| t.key == "t_dev").unwrap();
    assert!(!dev.is_new, "key 命中已有类型清单时应标为映射而非新建");
}

// ──────────────────────────────────────────────────────────
// 判据七：REQ-014 · 作用域 vs 上下文（缺陷 F1）
//
// 上一批栽在这里：REQ-013 自述「阶段 1 与阶段 3/4 今天无分歧」，
// QA-B 实测证伪 —— 同一列 `t_dev.t_customer_id`，
// 全作用域是「所属客户」（关系推导），一表一组是「客户标识」（注释回落）。
// 更糟的是**零测试发现**：22/22 全绿。
// 下面三条把这件事钉死：① 阶段 1 必须给出与阶段 3/4 相同的名字；
// ② 上下文**不许**把作用域撑宽（设计点②）；③ `counts` 仍按本组算。
// ──────────────────────────────────────────────────────────

/// **这条是 F1 的判据**：`t_dev` 单独成组（作用域 1 张表），
/// FK 目标 `t_customer` 在**组外** —— 属性名仍必须是关系推导的「所属客户」，
/// 而**不是**回落成注释里的「客户标识」。
///
/// 三段断言各断一件事：阶段 1 与阶段 3/4 对齐（真 bug 的正面）、
/// 去掉上下文就回落（证明结论**真的**由上下文解出，不是碰巧）、
/// 阶段 3/4 本身没漂（对照组）。
#[test]
fn test_fk_target_outside_group_scope_still_yields_relation_name() {
    let report = schema();
    let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());

    // 一表一组 —— 设计点② 的真实分组形态
    let g = group_of(&report, &["t_dev"], vec![]);
    assert_eq!(g.schema.len(), 1, "前提：作用域只有 t_dev 一张表");
    assert!(
        !g.schema.iter().any(|t| t.name == "t_customer"),
        "前提：FK 目标确实在**组外** —— 否则这条断言测不到 F1"
    );
    assert!(
        g.context_tables.iter().any(|t| t == "t_customer"),
        "前提：上下文清单里有组外的 FK 目标"
    );

    // ① 阶段 1：作用域只有 t_dev，属性名仍走关系推导
    let ex = agent.extract_group(&g).expect("阶段 1 抽取失败");
    assert_eq!(
        prop_name(&ex, "t_dev", "t_customer_id"),
        "所属客户",
        "FK 目标在组外也必须解出关系 —— 回落成注释「客户标识」就是 F1 本身"
    );

    // 对照组：阶段 3/4 的全量路径必须给出**同一个**名字（F1 的本质是两边不一致）
    let full = extract(&report, &names(), "p", "a", "1").unwrap();
    let full_name = full
        .proposal
        .items
        .iter()
        .find(|i| i["kind"] == "add_entity_type" && i["payload"]["key"] == "t_dev")
        .and_then(|i| i["payload"]["properties"].as_array())
        .and_then(|ps| ps.iter().find(|p| p["column"] == "t_customer_id"))
        .and_then(|p| p["name"].as_str())
        .expect("全量路径应有 t_dev.t_customer_id 这个属性");
    assert_eq!(
        full_name, "所属客户",
        "前提：阶段 3/4 全量路径给的是关系推导名（否则 ① 的判据选错了）"
    );

    // ② 反向对照：上下文里去掉组外的 FK 目标 ⇒ 关系确实推不出来 ⇒ 名字回落。
    // 这段证明 ① 不是「碰巧对」——它真的靠上下文解出来的。
    let mut g_no_ctx = g.clone();
    g_no_ctx.context_tables.retain(|t| t != "t_customer");
    let ex_no_ctx = agent.extract_group(&g_no_ctx).unwrap();
    assert_eq!(
        prop_name(&ex_no_ctx, "t_dev", "t_customer_id"),
        "客户标识",
        "没有上下文时关系推不出来，回落成注释 —— 这正是 F1 的成因，也证明上下文是有承重的"
    );
}

/// **上下文不许把作用域撑宽**（设计点②）。
///
/// 上下文是给 FK 消歧的**存在性判据**，不是第二份待抽的库结构。
/// 补上下文的同时最容易顺手把组外表也抽了 —— 那就等于用「多带几张表」
/// 绕开了「哪些表属于同一业务域正是要判断的事」这个循环依赖。
#[test]
fn test_context_tables_do_not_widen_the_extraction_scope() {
    let report = schema();
    let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());
    let g = group_of(&report, &["t_dev"], vec![]);

    let ex = agent.extract_group(&g).unwrap();
    assert_eq!(
        ex.types.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(),
        vec!["t_dev"],
        "上下文里有 t_customer，但作用域只有 t_dev —— 产物里就不许冒出 t_customer"
    );
}

/// `to_schema_report()` 的 `counts` **仍按本组算**，且**不含上下文表**。
///
/// 两个数字为什么是本组而不是「本组 + 上下文」：抽取器只遍历 `tables`，
/// 实体 / 属性 / 统计全部按那一批表产生（`stats.tables_seen` 同源）。
/// 把上下文表算进 `table_count`，这个数就会声称覆盖了一批**从没被遍历过**的表，
/// 于是 `翻译覆盖 = N / tables_seen` 这类比值的分母里混进没看过的表 —— 单组统计当场说谎。
#[test]
fn test_group_counts_cover_the_group_scope_only() {
    let report = schema();
    let g = group_of(&report, &["t_dev"], vec![]);
    let r = g.to_extraction_input();

    assert_eq!(r.scope.tables.len(), 1, "作用域只有 t_dev");
    assert!(
        r.context_tables.len() > r.scope.tables.len(),
        "前提：上下文比作用域大，这条断言才测得到东西"
    );

    assert_eq!(r.scope.table_count, 1, "table_count 按本组算，不含上下文表");
    assert_eq!(r.scope.column_count, g.schema[0].columns.len());
    assert_eq!(r.scope.commented_column_count, 3, "t_dev 上 3 列有注释");

    // 上下文确实在输入里 —— 上面那些数字不是「上下文没接上」的假象
    assert!(r.context_tables.contains(&"t_customer".to_string()));
}

// ──────────────────────────────────────────────────────────
// 判据八：REQ-014 · key 稳定从注释变成机制（缺陷 F4）
// ──────────────────────────────────────────────────────────

/// 声称「新类型」却占了个**已存在**的 key ⇒ 必须报出来。
///
/// REQ-013 只查了镜像的另一半（`is_new=false` 却不在清单里），
/// 这一支当时是**静默通过**的 —— 那正是「key 稳定」沦为注释的原因。
#[test]
fn test_existing_key_declared_as_new_is_reported() {
    use ontok::agent::group::MappingViolation as V;
    let report = schema();
    let group = group_of(
        &report,
        &["t_dev"],
        vec![ExistingType {
            key: "t_dev".into(),
            display_name: "设备".into(),
        }],
    );
    let ex = GroupExtraction {
        links: Vec::new(),
        types: vec![CandidateType {
            key: "t_dev".into(),
            display_name: "设备".into(),
            // key 明明已在清单里，却声称是新类型
            is_new: true,
            properties: vec![CandidateProperty {
                name: "序列号".into(),
                source: ColumnRef::new("t_dev", "sn"),
                nullable: None,
                note: None,
                origin: None,
            }],
            description: None,
            aliases: Vec::new(),
            evidence: Vec::new(),
            rejected_comments: None,
        }],
    };
    assert_eq!(
        verify_mappings(&ex, &group),
        vec![V::ExistingKeyDeclaredAsNew {
            type_key: "t_dev".into()
        }],
        "占用已存在的 key 必须被报出来，不能静默当成新类型"
    );
}

/// 已有类型的 `display_name` 被改动 ⇒ 必须报出来（设计点④）。
///
/// 报的不是「不许改名」——④ 明写「改名只改 `display_name`」，
/// 改名是合法的。这里要保证的是**改名可见**：它是该进版本历史的一次显式事件，
/// 而不是每轮重跑时悄悄漂掉的副作用。key 不变而显示名变，正是「改名」；
/// key 变了则是「删一个加一个」，由上面那条 `UnknownExistingType` 管。
#[test]
fn test_existing_display_name_change_is_reported_and_unchanged_name_is_quiet() {
    use ontok::agent::group::MappingViolation as V;
    let report = schema();
    let group = group_of(
        &report,
        &["t_dev"],
        vec![ExistingType {
            key: "t_dev".into(),
            display_name: "设备".into(),
        }],
    );
    let mk = |display_name: &str, is_new: bool| GroupExtraction {
        links: Vec::new(),
        types: vec![CandidateType {
            key: "t_dev".into(),
            display_name: display_name.into(),
            is_new,
            properties: vec![CandidateProperty {
                name: "序列号".into(),
                source: ColumnRef::new("t_dev", "sn"),
                nullable: None,
                note: None,
                origin: None,
            }],
            description: None,
            aliases: Vec::new(),
            evidence: Vec::new(),
            rejected_comments: None,
        }],
    };

    // ① key 命中、显示名不同 ⇒ 报「改名」
    assert_eq!(
        verify_mappings(&mk("Dev", false), &group),
        vec![V::ExistingDisplayNameChanged {
            type_key: "t_dev".into(),
            declared: "Dev".into(),
            known: "设备".into(),
        }],
        "key 稳定、显示名被改 —— 改名合法但必须**可见**，不能静默漂移"
    );

    // ② key 命中、显示名一致 ⇒ 安静（key 不变就是同一个类型，不是新东西）
    assert_eq!(
        verify_mappings(&mk("设备", false), &group),
        Vec::<V>::new(),
        "key 与显示名都对得上，不该报任何违规"
    );

    // ③ 规则实现走的就是 ② 那条路：既有清单命中时必须安静
    let agent = RuleBasedAgent::new(AgentIdentity::legacy_cli("kh-core", "0.1.0"), names());
    let ex = agent.extract_group(&group).unwrap();
    assert_eq!(
        verify_mappings(&ex, &group),
        Vec::<V>::new(),
        "规则实现按 key 命中已有类型、显示名沿用翻译表 —— 不该被判成改名"
    );
}

// ──────────────────────────────────────────────────────────
// 判据三：版本锁定（§8.4「具体版本号，不是『最新版』」)
// ──────────────────────────────────────────────────────────

#[test]
fn test_vague_version_is_rejected() {
    for v in [
        "latest", "LATEST", " newest ", "current", "main", "master", "dev", "nightly",
    ] {
        assert!(is_vague_version(v), "「{v}」不算具体版本，必须被拒");
        let r = AgentRegistration {
            agent: "kh-core".into(),
            version: v.to_string(),
            protocol: "inproc".into(),
            endpoint: None,
            capabilities: vec![],
            registered_at: "2026-10-07T10:00:00Z".into(),
        };
        assert!(
            r.validate().is_err(),
            "登记一个叫「{v}」的版本必须被拒 —— 否则 diff 归因整体失效（§8.4）"
        );
    }
    for v in ["0.1.0", "2.3.1", "2026.10.07", "kh-core-v4"] {
        assert!(!is_vague_version(v), "「{v}」是具体版本，应放行");
    }
}

#[test]
fn test_registration_rejects_empty_agent_and_protocol() {
    let base = AgentRegistration {
        agent: "kh-core".into(),
        version: "0.1.0".into(),
        protocol: "inproc".into(),
        endpoint: None,
        capabilities: vec![],
        registered_at: "2026-10-07T10:00:00Z".into(),
    };
    assert!(base.validate().is_ok());

    let mut bad = base.clone();
    bad.agent = "  ".into();
    assert!(bad.validate().is_err(), "登记名为空必须被拒");

    let mut bad = base.clone();
    bad.protocol = "".into();
    assert!(
        bad.validate().is_err(),
        "协议为空必须被拒 —— 协议是必填数据"
    );

    let mut bad = base.clone();
    bad.version = "".into();
    assert!(bad.validate().is_err(), "版本为空必须被拒");
}

// ──────────────────────────────────────────────────────────
// 判据四：registry 的 DB 行为（fail-closed + 版本锁定 + 记迭代）
// ──────────────────────────────────────────────────────────

fn reg(agent: &str, version: &str, protocol: &str, endpoint: Option<&str>) -> AgentRegistration {
    AgentRegistration {
        agent: agent.to_string(),
        version: version.to_string(),
        protocol: protocol.to_string(),
        endpoint: endpoint.map(|s| s.to_string()),
        capabilities: vec!["group_extract".into()],
        registered_at: "2026-10-07T10:00:00Z".into(),
    }
}

#[tokio::test]
async fn test_registry_register_require_and_fail_closed() {
    let pool = test_pool().await;
    migrate(&pool).await;
    let registry = PgAgentRegistry::new(&pool);
    let name = common::unique_project("agent");

    // 查不到 ⇒ 报错，**不回落默认值**（config.rs / introspect.rs 同一纪律）
    let e = registry
        .require(&format!("{name}_ghost"), "1.0.0")
        .await
        .unwrap_err();
    assert!(
        matches!(e, RegistryError::VersionNotRegistered { .. }),
        "查不到必须报 VersionNotRegistered，实际：{e:?}"
    );
    let e = registry
        .require_any(&format!("{name}_ghost"))
        .await
        .unwrap_err();
    assert!(
        matches!(e, RegistryError::AgentNotRegistered { .. }),
        "查不到必须报 AgentNotRegistered，实际：{e:?}"
    );

    assert!(registry
        .register(&reg(&name, "0.1.0", "inproc", None))
        .await
        .unwrap());
    let got = registry.require(&name, "0.1.0").await.unwrap();
    assert_eq!(got.agent, name);
    assert_eq!(got.version, "0.1.0");
    assert_eq!(got.protocol, "inproc");
    assert_eq!(got.endpoint, None);
    assert_eq!(got.capabilities, vec!["group_extract".to_string()]);

    // 「协议/端点是数据」—— 换个协议登记，类型一个字都不用改
    assert!(registry
        .register(&reg(
            &name,
            "0.2.0",
            "a2a",
            Some("https://example.invalid/agent")
        ))
        .await
        .unwrap());
    let got2 = registry.require(&name, "0.2.0").await.unwrap();
    assert_eq!(got2.protocol, "a2a");
    assert_eq!(
        got2.endpoint.as_deref(),
        Some("https://example.invalid/agent")
    );

    // 登记不合法的东西必须被拒（fail-closed 在入口就拦）
    let e = registry
        .register(&reg(&name, "latest", "inproc", None))
        .await
        .unwrap_err();
    assert!(
        matches!(e, RegistryError::Invalid(_)),
        "模糊版本必须在登记时被拒，实际：{e:?}"
    );
}

/// **版本锁定是 registry 的硬职责**（规格 §4.1 / §8.4）。
/// 同键重登记且内容不同 ⇒ 报错；内容相同 ⇒ 幂等重放。
#[tokio::test]
async fn test_registry_version_lock_rejects_silent_rewrite() {
    let pool = test_pool().await;
    migrate(&pool).await;
    let registry = PgAgentRegistry::new(&pool);
    let name = common::unique_project("agent");

    registry
        .register(&reg(&name, "1.0.0", "inproc", None))
        .await
        .unwrap();

    // 同内容重放 ⇒ 幂等，不报错也不新增
    assert!(
        !registry
            .register(&reg(&name, "1.0.0", "inproc", None))
            .await
            .unwrap(),
        "同内容重放应判为幂等（返回 false = 没新建）"
    );

    // 同键、换协议 ⇒ 必须报 VersionLocked
    let e = registry
        .register(&reg(
            &name,
            "1.0.0",
            "a2a",
            Some("https://example.invalid/x"),
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(e, RegistryError::VersionLocked { .. }),
        "同键改内容必须报 VersionLocked（偷偷换版本会让 diff 归因失效），实际：{e:?}"
    );
    let msg = e.to_string();
    assert!(msg.contains("新版本号"), "报错要告诉人怎么改，实际：{msg}");

    // 锁死之后，原记录**没被改写** —— 这条才是「锁定」的关键
    let got = registry.require(&name, "1.0.0").await.unwrap();
    assert_eq!(got.protocol, "inproc", "失败的那次登记不许留下痕迹");
    assert_eq!(got.endpoint, None);
}

/// REQ-025 · 并发登记同键：**不得**漏出裸的 `Db(...)`。
///
/// # 这条钉住什么
///
/// `register` 曾经是「先 `find()` 查一遍，没有再 `INSERT`」，两步之间没有锁。
/// 两个连接并发登记**同一 `(agent, version)`** 时两边都可能 `find()` 到 `None`，
/// 随后一个 INSERT 成功、另一个撞裸的唯一约束 ⇒ 调用方收到的是
/// `RegistryError::Db(unique constraint)`，而不是本该给的 `VersionLocked`。
/// 于是一条「版本锁定被绕过」的现场，在并发路径上被翻译成了一句「数据库错误」——
/// **错误变体说假话**。
///
/// # 怎么造出真并发
///
/// `JoinSet` 起 `n` 个 task，每个 task 自带一条池连接（池上限 5 ⇒ 每次 5 条真并行），
/// 全部登记**同一个 `(agent, version)`**。竞争窗口在**数据库侧**
/// （`find()` 返回之后、`INSERT` 落库之前），所以不需要在 Rust 侧对齐起跑线：
/// 同一个 key 上的并发 INSERT 必然有人撞唯一约束。
/// 多跑几轮是为了让「窗口恰好没撞上」的概率低到可以忽略。
///
/// ⚠️ 「每次 5 条真并行」靠的是**池宽**，不是 task 数：池若只有 1 条连接，
/// 8 个 task 就串行执行，数据库侧的并发窗口根本不存在，
/// 这条测试会退化成「永远通过」的假命题。所以走 `test_pool_with(5)`（REQ-030）。
#[tokio::test]
async fn test_registry_concurrent_same_key_never_leaks_raw_db_error() {
    let pool = std::sync::Arc::new(common::test_pool_with(5).await);
    migrate(&pool).await;

    // ① 同键、**内容各不相同** ⇒ 恰好一个写入者，其余全是 VersionLocked。
    for round in 0..4 {
        let agent = common::unique_project("race_diff");
        let r = race_register(&pool, &agent, 8, false).await;
        assert_eq!(
            r.raw.len(),
            0,
            "同键并发登记漏出了裸 Db 错误（第 {round} 轮）：{:?}",
            r.raw
        );
        assert_eq!(r.created, 1, "同键并发只能有一个写入者（第 {round} 轮）");
        assert_eq!(r.locked, 7, "其余全部必须报 VersionLocked（第 {round} 轮）");
        assert_eq!(r.replayed, 0, "内容各不相同，不该有幂等重放（第 {round} 轮）");
        // 只有一个行，且内容是胜出者的 —— 失败的那些次不许留下痕迹
        let got = PgAgentRegistry::new(&pool)
            .require(&agent, "0.1.0")
            .await
            .unwrap();
        assert!(got.endpoint.is_some(), "胜出者的 endpoint 应留在库里");
    }

    // ② 同键、**内容完全相同** ⇒ 一个写入者 + 其余幂等重放。
    //    这条是「别把并发一律报 VersionLocked」的护栏：幂等是明文语义。
    for round in 0..2 {
        let agent = common::unique_project("race_same");
        let r = race_register(&pool, &agent, 8, true).await;
        assert_eq!(r.raw.len(), 0, "同内容并发漏出了裸 Db 错误：{:?}", r.raw);
        assert_eq!(r.created, 1, "同键并发只能有一个写入者（第 {round} 轮）");
        assert_eq!(r.replayed, 7, "同内容重登记是幂等，不许报错（第 {round} 轮）");
        assert_eq!(r.locked, 0, "同内容不该报 VersionLocked（第 {round} 轮）");
    }
}

/// 一轮并发的结果分类。`raw` 里装的是**不该出现**的那些错误。
#[derive(Default)]
struct RaceOutcome {
    created: usize,
    replayed: usize,
    locked: usize,
    raw: Vec<String>,
}

/// 起 `n` 个并发 task 登记**同一个 `(agent, version)`**，按结果分类。
///
/// `same_content = true` ⇒ 每个 task 用同一个 endpoint（考验幂等路径）；
/// `false` ⇒ endpoint 各自不同（考验 `VersionLocked` 路径）。
async fn race_register(
    pool: &std::sync::Arc<sqlx::PgPool>,
    agent: &str,
    n: usize,
    same_content: bool,
) -> RaceOutcome {
    let mut set = tokio::task::JoinSet::new();
    for i in 0..n {
        let pool = pool.clone();
        let agent = agent.to_string();
        set.spawn(async move {
            let endpoint = if same_content {
                "shared-endpoint".to_string()
            } else {
                format!("endpoint-{i}")
            };
            let r = reg(&agent, "0.1.0", "inproc", Some(&endpoint));
            PgAgentRegistry::new(&pool).register(&r).await
        });
    }

    let mut out = RaceOutcome::default();
    while let Some(joined) = set.join_next().await {
        match joined.expect("登记 task panic 了") {
            Ok(true) => out.created += 1,
            Ok(false) => out.replayed += 1,
            Err(RegistryError::VersionLocked { .. }) => out.locked += 1,
            Err(other) => out.raw.push(format!("{other:?} / 显示为「{other}」")),
        }
    }
    out
}

/// 记迭代（§4.1「记录每轮迭代」）：重复同一轮幂等。
#[tokio::test]
async fn test_registry_records_rounds_idempotently() {
    let pool = test_pool().await;
    migrate(&pool).await;
    let registry = PgAgentRegistry::new(&pool);
    let name = common::unique_project("agent");
    registry
        .register(&reg(&name, "3.1.4", "inproc", None))
        .await
        .unwrap();

    assert_eq!(registry.max_round(&name, "3.1.4").await.unwrap(), 0);
    assert!(registry
        .record_round(&name, "3.1.4", 1, Some("public"))
        .await
        .unwrap());
    assert!(
        !registry
            .record_round(&name, "3.1.4", 1, Some("public"))
            .await
            .unwrap(),
        "同一轮重放必须幂等"
    );
    assert!(registry
        .record_round(&name, "3.1.4", 2, None)
        .await
        .unwrap());
    assert_eq!(registry.max_round(&name, "3.1.4").await.unwrap(), 2);

    // 轮次 ≥1，且必须指向已登记的 agent
    assert!(matches!(
        registry
            .record_round(&name, "3.1.4", 0, None)
            .await
            .unwrap_err(),
        RegistryError::BadRound(0)
    ));
    assert!(matches!(
        registry.require(&name, "9.9.9").await.unwrap_err(),
        RegistryError::VersionNotRegistered { .. }
    ));
    assert!(matches!(
        registry
            .record_round(&name, "9.9.9", 1, None)
            .await
            .unwrap_err(),
        RegistryError::VersionNotRegistered { .. }
    ));
}

/// 身份解析：agent + 锁定版本 + 轮次 ⇒ [`AgentIdentity`]，
/// **不回落默认值**。round < 1 直接拒。
#[tokio::test]
async fn test_registry_resolve_identity_is_fail_closed() {
    let pool = test_pool().await;
    migrate(&pool).await;
    let registry = PgAgentRegistry::new(&pool);
    let name = common::unique_project("agent");
    registry
        .register(&reg(
            &name,
            "2.3.1",
            "a2a",
            Some("https://example.invalid/a"),
        ))
        .await
        .unwrap();

    let id = registry.resolve_identity(&name, None, 7).await.unwrap();
    assert_eq!(
        id,
        AgentIdentity {
            agent: name.clone(),
            version: "2.3.1".into(),
            round: 7
        }
    );

    let id = registry
        .resolve_identity(&name, Some("2.3.1"), 1)
        .await
        .unwrap();
    assert_eq!(id.round, 1);

    assert!(matches!(
        registry
            .resolve_identity(&name, Some("0.0.1"), 1)
            .await
            .unwrap_err(),
        RegistryError::VersionNotRegistered { .. }
    ));
    assert!(matches!(
        registry.resolve_identity(&name, None, 0).await.unwrap_err(),
        RegistryError::BadRound(0)
    ));
}

// ──────────────────────────────────────────────────────────
// 判据五：分层纪律（AC-002）与共享库纪律
// ──────────────────────────────────────────────────────────

/// AC-002 的原判据：`grep -rn 'sqlx\|PgStore\|db::' src/domain/` 必须无输出。
/// REQ-013 加了 agent 模块，**必须没有把它渗进 domain**。
#[test]
fn test_domain_layer_stays_io_free() {
    let dir = crate_root().join("src/domain");
    let mut offenders: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .expect("读不到 src/domain")
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let l = line.trim_start();
            // 模块头的注释里写着这条纪律本身，注释行不算
            if l.starts_with("//") || l.starts_with("/*") || l.starts_with('*') {
                continue;
            }
            for needle in ["sqlx", "PgStore", "db::"] {
                if l.contains(needle) {
                    offenders.push(format!("{}:{}: {line}", path.display(), i + 1));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "src/domain/ 出现 I/O 痕迹：\n{}",
        offenders.join("\n")
    );
}

/// 共享本体库**只允许 INSERT**：registry 的源码里不许有破坏性 SQL。
///
/// 匹配的是「动词 + 关键字」的组合，不是裸词 —— 因为本文件的**注释**里
/// 写着「没有 DELETE / TRUNCATE / UPDATE 语句」，裸词匹配会把自己判成违规。
#[test]
fn test_registry_source_has_no_destructive_sql() {
    let src = include_str!("../src/db/agent_registry.rs");
    for bad in [
        "DELETE FROM",
        "delete from",
        "TRUNCATE TABLE",
        "truncate table",
        "DROP TABLE",
        "drop table",
        "UPDATE extraction_",
        "update extraction_",
    ] {
        assert!(
            !src.contains(bad),
            "registry 源码里出现了破坏性 SQL「{bad}」—— 共享本体库只允许 INSERT"
        );
    }
}

// ──────────────────────────────────────────────────────────
// 判据六：CLI 两条路径逐字节一致
// ──────────────────────────────────────────────────────────

static CLI_SCRATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// CLI 测试的**专属**暂存目录。
///
/// 三个 CLI 测试原先共用一个 `cli-test/`，并行跑时互相踩：
/// 一个测试正在 `std::fs::write(schema.json)`，另一个同时去读它，
/// 拿到半截文件 → `schema JSON 解析失败`。
/// （这个 race 是被一次全量 `cargo test --no-fail-fast` 逮到的，
/// 单独跑该 binary 时未必复现。）
/// 一测试一目录 + pid + 序号，撞不了车。
fn cli_scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("..")
        .join(".review-workspace")
        .join("req013")
        .join("cli-test")
        .join(format!(
            "{}-{}-{}",
            tag,
            std::process::id(),
            CLI_SCRATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

/// 把测试用的 schema + 翻译表写进专属目录，返回两者路径。
fn cli_inputs(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let schema_path = dir.join("schema.json");
    std::fs::write(&schema_path, serde_json::to_string_pretty(&schema()).unwrap()).unwrap();
    let names_path = dir.join("names.json");
    std::fs::write(&names_path, NAMES_JSON).unwrap();
    (schema_path, names_path)
}

/// CLI 的两条路径：
/// - 默认（不带 `--agent-registry`）⇒ 不连库，`extracted_by` 记**真正跑起来的实现**
/// - 带 `--agent-registry` ⇒ 连库查登记，**fail-closed**
///
/// ⚠️ REQ-059 起断言分成两半：两条路径的 `items` 必须逐字节相同
/// （REQ-013 的 registry 改造没改抽取行为），而 `extracted_by` 的两栏
/// **必须可分辨** —— 过去它们「相同」，是因为默认路径记的是编造占位。
#[tokio::test]
async fn test_cli_registry_path_is_byte_identical_when_registry_says_the_same() {
    let pool = test_pool().await;
    migrate(&pool).await;
    let registry = PgAgentRegistry::new(&pool);
    // 与 CLI 的默认值完全一致：agent=mcode-agent、version=翻译表版本、round=1。
    // 重复登记是**幂等重放**（同键同内容 ⇒ register 返回 false），所以这个测试可重复跑。
    registry
        .register(&reg("mcode-agent", "2026.10.07", "inproc", None))
        .await
        .unwrap();

    let scratch = cli_scratch("byte-identical");
    let (schema_path, names_path) = cli_inputs(&scratch);

    let run = |extra: &[&str], tag: &str| -> String {
        let out = scratch.join(format!("proposal-{tag}.json"));
        let stats = scratch.join(format!("stats-{tag}.json"));
        let mut cmd = propose_bin();
        cmd.args([
            "--schema",
            schema_path.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stats",
            stats.to_str().unwrap(),
            "--project",
            "cli-probe",
        ])
        .args(extra)
        // 子进程要能自己连库（registry 路径）
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect(NEED_TEST_DB),
        );
        let o = cmd.output().expect("跑 ontok-propose 失败");
        assert!(
            o.status.success(),
            "ontok-propose {:?} 失败：{}",
            extra,
            String::from_utf8_lossy(&o.stderr)
        );
        std::fs::read_to_string(&out).expect("读不到产出提案")
    };

    let legacy = run(&[], "legacy");
    let via_registry = run(
        &[
            "--agent-registry",
            "mcode-agent",
            "--agent-registry-version",
            "2026.10.07",
        ],
        "registry",
    );

    // ⚠️ REQ-059 推翻了本测试原先的前提 —— 两条路径**不再**整份逐字节相同。
    //
    // 原判据「registry 登记成与默认值一致时，两份 proposal.json 逐字节相同」，
    // 成立的前提是「CLI 的默认值 = mcode-agent + 翻译表版本」。
    // 那正是本需求要修的缺陷：默认路径记的是**编造占位**，与 registry
    // 登记值「碰巧一样」只是因为登记时照着占位抄的。
    //
    // 修完之后：默认路径记**真正跑起来的实现**，registry 路径记**库里的登记**，
    // 两者**理应不同**。仍然逐字节一致的判据收窄成**与身份来源无关的内容**。
    let legacy_v: serde_json::Value = serde_json::from_str(&legacy).unwrap();
    let reg_v: serde_json::Value = serde_json::from_str(&via_registry).unwrap();
    assert_eq!(
        legacy_v["items"], reg_v["items"],
        "★ registry 路径与默认路径的 items 必须逐字节一致 —— 这次重构不许改抽取行为"
    );
    assert_eq!(legacy_v["round"], reg_v["round"]);
    assert_eq!(
        legacy_v["extracted_by"]["source"], reg_v["extracted_by"]["source"]
    );
    // 归属两栏：默认路径 = 执行者自报，registry 路径 = 库里登记。
    assert_eq!(
        legacy_v["extracted_by"]["agent"],
        json!(ontok::agent::factory::INPROC_EXECUTOR_ID),
        "默认路径的 extracted_by.agent 必须是真正跑起来的实现，不是 mcode-agent 那个占位"
    );
    assert_eq!(
        legacy_v["extracted_by"]["version"],
        json!(ontok::agent::factory::INPROC_EXECUTOR_VERSION),
        "默认路径的 extracted_by.version 不许再借用翻译表/登记的版本号"
    );
    assert_eq!(reg_v["extracted_by"]["agent"], json!("mcode-agent"));
    assert_ne!(
        legacy_v["extracted_by"]["agent"], reg_v["extracted_by"]["agent"],
        "★ 两条路径的 agent 必须可分辨（相同 = 默认路径又在冒充登记值）"
    );

    // REQ-019：第 2 轮起 CLI 会连库载入基版本的「已有类型清单」，
    // 没有基版本就**直接失败**（不退回空清单 —— 那会让 key 双向核对
    // 静默不可达）。所以跑 `--round 3` 之前先给它一个基版本。
    // 只 INSERT（建 project + 建版本），共享库不清不删。
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    // ⚠️ **本文件唯一故意保留 `get_or_create` 的测试。**
    //
    // 这里 `cli-probe` 是**写死的固定名**，不是 `unique_project` 产物 ——
    // 它必须与上面子进程 CLI 的 `--project cli-probe` 指向同一行。
    // CLI 自己内部就是 `projects.get_or_create(pname)`（`src/cli.rs`），
    // 本测试这段只是**镜像** CLI 的建项目路径，好在同一项目上先垫一个基版本。
    //
    // 与 REQ-026「测试侧一律走 create、撞名即失败」的关系：
    // 那条针对的是 **`unique_project` 的 pid 复用撞名**（会静默读到别人的脏数据）。
    // 本例是**另一个形状**：固定名 + 跨进程（测试进程 ↔ CLI 子进程），
    // 跨轮次复用是**刻意的**（每次跑 `--round 3` 都要在同一项目上垫基版本）。
    // 改成 `create` 不会暴露什么 bug，只会让本测试**从第二轮起永久失败**。
    if projects.get_by_name("cli-probe").await.unwrap().is_none() {
        let p = projects.get_or_create("cli-probe").await.unwrap();
        projects.new_version(p.id).await.unwrap();
    } else {
        let p = projects.get_by_name("cli-probe").await.unwrap().unwrap();
        if projects.latest_version(p.id).await.unwrap().is_none() {
            projects.new_version(p.id).await.unwrap();
        }
    }

    // `--round` 真的进了产出（证明 round 不再是硬编码 1）
    let r3 = run(
        &[
            "--agent-registry",
            "mcode-agent",
            "--agent-registry-version",
            "2026.10.07",
            "--round",
            "3",
        ],
        "round3",
    );
    let v: serde_json::Value = serde_json::from_str(&r3).unwrap();
    assert_eq!(v["round"], json!(3), "round 应来自 --round 而不是硬编码 1");
    assert_eq!(v["extracted_by"]["round"], json!(3));
    assert_eq!(v["extracted_by"]["agent"], json!("mcode-agent"));
    assert_eq!(v["extracted_by"]["version"], json!("2026.10.07"));
    assert_ne!(
        legacy, r3,
        "round 变了就该有字节差异 —— 否则断言 2 没有意义"
    );
}

/// fail-closed 在 CLI 层的表现：查不到的 agent **必须失败退出**，
/// 而且**不许留下半个产出文件**（fail-open 会让人拿着残缺提案继续跑）。
#[test]
fn test_cli_unknown_registry_agent_fails_closed_without_writing_output() {
    let scratch = cli_scratch("fail-closed");
    let (schema_path, names_path) = cli_inputs(&scratch);
    let out = scratch.join("must-not-exist.json");
    let stats = scratch.join("must-not-exist-stats.json");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&stats);

    let o = propose_bin()
        .args([
            "--schema",
            schema_path.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--stats",
            stats.to_str().unwrap(),
            "--agent-registry",
            "definitely-not-registered-agent-xyz",
        ])
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect(NEED_TEST_DB),
        )
        .output()
        .expect("跑 ontok-propose 失败");

    assert!(
        !o.status.success(),
        "查不到的 agent 必须失败退出（fail-closed）"
    );
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("没有") && err.contains("登记"),
        "报错要说清「registry 里没有这个 agent」，实际 stderr：{err}"
    );
    assert!(
        !out.exists(),
        "失败的抽取不许留下提案文件 —— 半个产出比没有产出更危险"
    );
    assert!(!stats.exists(), "失败的抽取不许留下统计文件");
}

/// 两套溯源来源并存 ⇒ 拒绝（§8.4：溯源不能靠猜）。
#[test]
fn test_cli_rejects_registry_plus_explicit_agent() {
    let scratch = cli_scratch("conflict");
    let (schema_path, names_path) = cli_inputs(&scratch);

    let o = propose_bin()
        .args([
            "--schema",
            schema_path.to_str().unwrap(),
            "--names",
            names_path.to_str().unwrap(),
            "--out",
            scratch.join("conflict.json").to_str().unwrap(),
            "--stats",
            scratch.join("conflict-stats.json").to_str().unwrap(),
            "--agent-registry",
            "whatever",
            "--agent",
            "mcode-agent",
        ])
        .env(
            "ONTOK_DATABASE_URL",
            ontok::config::test_database_url().expect(NEED_TEST_DB),
        )
        .output()
        .expect("跑 ontok-propose 失败");

    assert!(!o.status.success(), "两套溯源来源并存必须被拒");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("不能同时给"), "报错要说清冲突，实际：{err}");
}

/// REQ-152：构建面命令已并入 **`ontok-builder`**（子命令 `propose`）。
///
/// 参数、产物与退出码一个字节都没变 —— 只是二进制名字变了、多一层子命令。
/// 返回 `Command` 而不是 `&str`: 调用点大多是 `Command::new(..).arg(..)` 的链，
/// 换成字符串就得在每个调用点各插一行 `.arg("propose")`（漏一个就红得莫名其妙）。
fn propose_bin() -> std::process::Command {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_ontok-builder"));
    c.arg("propose");
    c
}
