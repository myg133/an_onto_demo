//! REQ-010 · **过渡初始声明**的契约。
//!
//! 这份文件（`src/domain/concept_groups.transition.json`）是**过渡声明，
//! 不是结论**。测试要守住的不是「这些分组对不对」—— 那要人来判 ——
//! 而是**它作为一份声明的自洽性与可评审性**：
//!
//! 1. 「改内容必须改版本」的契约成立（指纹对得上）；
//! 2. 单归属：没有实体归属两个概念（否则总览的关系重定向无解）；
//! 3. 成员全是真实源表名，且没有把表重复计入；
//! 4. **该分开的确分开了** —— 尤其是「客户」与「客户订阅」这类
//!    「合并错了会误导客户」的组合；
//! 5. 每个概念都带说明与理由，**包括显式钉住的例外**；
//! 6. 它**还没有被业务确认**（过渡声明的常态，别把它写成已确认）。

use ontok::domain::concept::{validate_groups, ConceptDeclaration, ConceptProblem};
use ontok::domain::overview::{build_overview, render_overview_text};
use ontok::domain::view::{scan_modeling_terms, CustomerView, EntityView, PropertyView, RelationView};
use std::collections::{BTreeMap, BTreeSet};

/// 用 `include_str!` 而不是运行时读文件 —— 文件不在，**编译就失败**。
const DECL_JSON: &str = include_str!("../src/domain/concept_groups.transition.json");

fn decl() -> ConceptDeclaration {
    ConceptDeclaration::parse(DECL_JSON).expect("过渡声明必须能解析")
}

#[test]
fn test_transition_declaration_satisfies_its_own_contract() {
    // 「改内容必须改版本」：有人手改了 groups 却没升 version ⇒ 这里红
    let d = decl();
    assert_eq!(d.verify(), Ok(()), "过渡声明必须满足「改内容必须改版本」");
    assert!(!d.history.is_empty());
    assert_eq!(d.history.last().unwrap().version, d.version);
}

#[test]
fn test_transition_declaration_is_not_claimed_as_confirmed() {
    let d = decl();
    assert!(d.confirmed_by.is_none(), "过渡声明**不能**自称已确认");
    assert!(d.rule.contains("过渡"), "rule 必须写明这是过渡声明：{}", d.rule);
}

#[test]
fn test_every_group_has_a_reason_and_provenance() {
    let d = decl();
    for g in &d.groups {
        assert!(
            g.scope_hint.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false),
            "概念「{}」没有说明 —— 评审者无从判断", g.display_name
        );
        assert!(!g.evidence.is_empty(), "概念「{}」没有证据", g.display_name);
        let eb = g.extracted_by.as_ref().expect("每个概念都要记声明来源（§8.4）");
        assert!(!eb.agent.is_empty() && !eb.version.is_empty());
    }
}

/// 成员必须全在本项目实体集合内（硬约束①）。
#[test]
fn test_members_are_real_tables_and_single_assigned() {
    let d = decl();
    // 91 张源表（真实源库 public）。这里是**表名全集**的替身：
    // 成员就是表名（抽取侧 key = 表名），所以拿表名当「已知实体集合」判。
    let known: BTreeSet<String> = SOURCE_TABLES.iter().map(|s| s.to_string()).collect();
    let problems = validate_groups(&d.groups, &known);
    assert_eq!(
        problems,
        vec![],
        "过渡声明自身必须过三条硬约束（悬空成员/嵌套/重叠）—— 它是要进提案通道的"
    );
}

#[test]
fn test_no_table_is_in_two_concepts() {
    // 单归属是「重叠被拒」的另一面。单独再判一次，
    // 免得将来有人放宽 validate_groups 时没人发现这份文件已经重叠了。
    let d = decl();
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for g in &d.groups {
        for m in &g.members {
            if let Some(prev) = seen.insert(m.as_str(), g.key.as_str()) {
                panic!("{m} 同时属于 {prev} 与 {}", g.key);
            }
        }
    }
    assert_eq!(seen.len(), 67, "过渡声明覆盖 67 / 91 张表");
}

/// **该分开的确分开了** —— 这是这份声明最容易犯、也最误导客户的错。
#[test]
fn test_semantically_distinct_objects_are_pinned_separate() {
    let d = decl();
    let home: BTreeMap<&str, &str> = d
        .groups
        .iter()
        .flat_map(|g| g.members.iter().map(move |m| (m.as_str(), g.display_name.as_str())))
        .collect();

    // 「客户」不得吞掉这些 —— 客户会说「我的订阅信息呢」
    for (table, concept) in [
        ("t023", "客户订阅"),
        ("t022", "客户折扣返利"),
        ("t018", "客户意向"),
        ("t016", "客户账号绑定"),
    ] {
        assert_eq!(
            home.get(table).copied(),
            Some(concept),
            "{table} 必须单独成概念，自动并进「客户」是**误导**客户"
        );
    }
    assert_eq!(
        home.values().filter(|c| **c == "客户").count(),
        6,
        "「客户」这一组只含客户主档 + 5 张客户侧配置表"
    );

    // 反过来，「客户」这一组确实覆盖了客户侧的那些表（规则如此，但要显眼）
    assert_eq!(home.get("t025").copied(), Some("客户"));
    assert_eq!(home.get("t021").copied(), Some("客户"));
    assert_eq!(home.get("t024").copied(), Some("客户"));

    // 同前缀但不同业务域的 llm_* 必须拆开
    assert_eq!(home.get("t052").copied(), Some("大模型"));
    assert_eq!(home.get("t055").copied(), Some("大模型服务商"));
    assert_eq!(home.get("t051").copied(), Some("鉴权缓存"));
    assert_ne!(
        home.get("t052").copied(),
        home.get("t055").copied(),
        "大模型与服务商不能合成一个概念"
    );
}

/// 过渡声明的**已知弱点**必须写在文件里，不许只留在我的脑子里。
#[test]
fn test_low_confidence_groups_say_so_in_their_own_scope_hint() {
    let d = decl();
    // 「客户」这一组把 5 张配置表并进来了 —— 业务人员可能会反对，
    // 文件里必须自己承认这一点（它是本声明最需要人来拍板的一条）
    let c = d.groups.iter().find(|g| g.key == "concept_customer").expect("应有「客户」概念");
    let hint = c.scope_hint.clone().unwrap_or_default();
    assert!(
        hint.contains("确认") && hint.contains("客户"),
        "「客户」组的说明必须写明「待业务确认」并点出这是最需要确认的一条：{hint}"
    );
    // 低置信的几组也各自带警告
    for k in ["concept_agent", "concept_instance", "concept_internal", "concept_license", "concept_service"] {
        let g = d.groups.iter().find(|g| g.key == k).expect(k);
        let hint = g.scope_hint.clone().unwrap_or_default();
        assert!(hint.contains("低置信") || hint.contains("确认"), "{k} 的说明没写明不确定性：{hint}");
    }
}

/// 总览里不许出现表名 —— 声明里的 `evidence.table` 不得泄进客户读到的文本。
#[test]
fn test_declaration_evidence_never_reaches_customer_facing_text() {
    let d = decl();
    // 用一张最小的客户视图，只挂「套餐」那 3 个成员
    let view = CustomerView {
        objects: vec![
            obj("套餐模板", &["模板编号"]),
            obj("套餐产品", &["产品编号"]),
            obj("套餐权益", &["权益编号"]),
        ],
        relations: vec![],
    };
    let names = BTreeMap::from([
        ("t067".to_string(), "套餐模板".to_string()),
        ("t066".to_string(), "套餐产品".to_string()),
        ("t065".to_string(), "套餐权益".to_string()),
    ]);
    let o = build_overview(&view, &d.groups, &names);
    let text = render_overview_text(&o);
    let json = serde_json::to_string(&o).unwrap();
    // 91 张表名一个都不许出现在客户读到的输出里
    for t in SOURCE_TABLES {
        assert!(!text.contains(t), "表名 {t} 泄进总览文本");
        assert!(!json.contains(t), "表名 {t} 泄进总览 JSON");
    }
    // 文本里连下划线都不该有（那是源库标识符的形态）
    assert!(!text.contains('_'), "源库标识符形态泄进总览文本：\n{text}");
    assert!(scan_modeling_terms(&text).is_empty());
    assert!(scan_modeling_terms(&json).is_empty());

    // 顺带证明「成员查不到 ⇒ 整张卡不上总览并报出来」，而不是静默少一张卡
    assert_eq!(o.stats.concept_count, 1, "只有「套餐」那 3 个成员在视图里");
    assert_eq!(o.stats.concepts_dropped, 25);
    assert!(o.notes.iter().any(|n| n.contains("都没进客户视图")));
}

/// 声明里的概念名必须全部能上客户视图（不得退化、不得纯英文）。
#[test]
fn test_all_declared_concept_names_are_customer_facing_safe() {
    let d = decl();
    let known: BTreeSet<String> = SOURCE_TABLES.iter().map(|s| s.to_string()).collect();
    for g in &d.groups {
        assert!(
            ontok::domain::overview::concept_name_is_usable(&g.display_name, &known),
            "概念「{}」（key={}）的名字不能给客户看",
            g.display_name, g.key
        );
    }
}

fn obj(name: &str, props: &[&str]) -> EntityView {
    EntityView {
        name: name.into(),
        note: None,
        belongs_to: None,
        aliases: vec![],
        properties: props
            .iter()
            .map(|p| PropertyView { name: (*p).into(), note: None, required: false, sample: None })
            .collect(),
    }
}

/// 真实源库 `public` 的 91 张表（取自 `ontok-introspect` 的输出）。
/// 抄在这里而不是读文件：测试必须**离线**能跑，且这张名单本身就是
/// 「成员必须是真实表」这一条的判据。
const SOURCE_TABLES: &[&str] = &[
    "t001", "t002", "t003", "t004", "t005",
    "t006", "t007", "t008", "t009",
    "t010", "t011", "t012", "t013",
    "t014", "t015", "t016", "t017",
    "t018", "t019", "t020",
    "t021", "t022", "t023",
    "t024", "t025", "t026", "t027",
    "t028", "t029", "t030", "t031",
    "t032", "t033", "t034", "t035", "t036",
    "t037", "t038", "t039", "t040",
    "t041", "t042", "t043", "t044",
    "t045", "t046", "t047", "t048",
    "t049", "t050", "t051",
    "t052", "t053", "t054", "t055",
    "t056", "t057", "t058",
    "t059", "t060", "t061", "t062",
    "t063", "t064", "t065", "t066", "t067",
    "t068", "t069", "t070", "t071",
    "t072", "t073", "t074", "t075", "t076",
    "t077", "t078", "t079", "t080",
    "t081", "t082", "t083",
    "t084", "t085", "t086", "t087",
    "t088", "t089", "t090", "t091",
];

#[test]
fn test_source_table_list_is_complete() {
    // 名单本身也要对：漏一张表，那张表就永远不会被「成员必须真实」抓到
    let mut seen = BTreeSet::new();
    for t in SOURCE_TABLES {
        assert!(seen.insert(*t), "名单里有重复：{t}");
    }
    assert_eq!(seen.len(), 91, "真实源库是 91 张表");
    // 声明覆盖的成员必须是名单的子集
    let d = decl();
    for g in &d.groups {
        for m in &g.members {
            assert!(seen.contains(m.as_str()), "成员 {m} 不在 91 张表名单里");
        }
    }
    let covered: BTreeSet<&str> = d.groups.iter().flat_map(|g| g.members.iter().map(|m| m.as_str())).collect();
    assert_eq!(covered.len(), 67);
    assert!(!covered.contains("t064"), "单张表不构成概念，订单应留在未归组清单");
    assert!(!covered.contains("t072"), "报表不并进业务概念");
    let _ = ConceptProblem::EmptyGroup { group: String::new() };
}
