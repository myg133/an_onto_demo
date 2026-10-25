//! REQ-119 · **本体可查询化**的测设层判据（AC-001 ~ AC-010）。
//!
//! # 判据分工
//!
//! | AC | 判据 | 本文件 |
//! | --- | --- | --- |
//! | AC-001 | 52 条对象属性 ⇒ 52 条反向 + 104 条 `owl:inverseOf`；domain/range 对调 | `ac001_*` |
//! | AC-002 | xsd range 按列类型映射；失败 ⇒ string + 计数 | `ac002_*` / ttl.rs 单测 |
//! | AC-003 | 每个类一条 `ontok:instanceIri`，占位符有文档 | `ac003_*` |
//! | AC-004 | 租户标注只在建模视图；未签不落标注 | `ac004_*` |
//! | AC-005 | 无声明 ⇒ 文件头 + `tenantUnannotated` + 警告 | `ac005_*` |
//! | AC-006 | 同版本两次导出逐字节相同 | `ac006_*` |
//! | AC-007 | 客户视图 vs 建模视图四项核对 | `ac007_*` |
//! | AC-008 | 体积 ≤ 40% 增长（真库实测） | `live_*` |
//! | AC-009 | Agent 只读 TTL 写 SPARQL（人工判） | 交付报告（不在测试里假装自动判） |
//! | AC-010 | 三份文档在 `ttl.rs` 模块头 | `ac010_*` |

mod common;

use ontok::domain::model_view::{
    ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
};
use ontok::domain::ttl::{
    parse_allowed_values, range_stats, serialize_customer, serialize_model, tenancy_report,
    xsd_for_sql_type, SchemaTypes, TenancyDecl, TtlOptions, DEFAULT_BASE,
};
use ontok::domain::view::{CustomerView, EntityView, PropertyView, RelationView};

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn customer_view() -> CustomerView {
    CustomerView {
        objects: vec![
            EntityView {
                name: "订单".into(),
                note: Some("一张订单".into()),
                belongs_to: None,
                aliases: vec![],
                properties: vec![
                    PropertyView {
                        name: "下单时间".into(),
                        note: None,
                        required: true,
                        sample: None,
                    },
                    PropertyView {
                        name: "金额".into(),
                        note: None,
                        required: true,
                        sample: None,
                    },
                    PropertyView {
                        name: "是否启用".into(),
                        note: None,
                        required: true,
                        sample: None,
                    },
                    PropertyView {
                        name: "星云常数".into(),
                        note: None,
                        required: true,
                        sample: None,
                    },
                ],
            },
            EntityView {
                name: "客户".into(),
                note: None,
                belongs_to: None,
                aliases: vec![],
                properties: vec![PropertyView {
                    name: "余额".into(),
                    note: None,
                    required: true,
                    sample: None,
                }],
            },
        ],
        relations: vec![
            RelationView {
                from: "订单".into(),
                to: "客户".into(),
                how_many: "多对一".into(),
                meaning: "归属于".into(),
            },
            // 反向对：用于钉「反向名撞正向名」的 `__inverse` 后缀。
            RelationView {
                from: "客户".into(),
                to: "订单".into(),
                how_many: "一对多".into(),
                meaning: "拥有".into(),
            },
        ],
    }
}

fn prop(name: &str, column: &str) -> ModelPropertyView {
    ModelPropertyView {
        name: name.into(),
        column: column.into(),
        nullable: true,
        note: None,
        sample: None,
    }
}

fn model_view() -> ModelTrackView {
    ModelTrackView {
        seq: 3,
        extracted_by: None,
        entity_types: vec![
            ModelEntityView {
                key: "customers".into(),
                display_name: "客户".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![
                    prop("所属客户", "customer_id"),
                    prop("余额", "balance"),
                    prop("创建时间", "created_at"),
                    prop("是否删除", "is_deleted"),
                    prop("金额", "amount"),
                    prop("莫名列", "zzz_unknown"),
                ],
                evidence: vec![],
            },
            ModelEntityView {
                key: "deductions".into(),
                display_name: "扣费明细".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![prop("所属客户", "customer_id"), prop("金额", "amount")],
                evidence: vec![],
            },
            ModelEntityView {
                key: "order".into(),
                display_name: "订单".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![prop("所属客户", "customer_id")],
                evidence: vec![],
            },
            ModelEntityView {
                key: "customer_subscriptions".into(),
                display_name: "客户订阅".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![prop("所属客户", "customer_id")],
                evidence: vec![],
            },
        ],
        link_types: vec![
            link("deductions_customer", "deductions", "customers"),
            link("order_subscription", "order", "customer_subscriptions"),
            // 反向对：它的正向名会被前一条的反向名占用。
            link("subscription_order", "customer_subscriptions", "order"),
        ],
        action_types: vec![],
        functions: vec![],
    }
}

fn link(key: &str, from: &str, to: &str) -> ModelLinkView {
    ModelLinkView {
        key: key.into(),
        from_type: from.into(),
        to_type: to.into(),
        cardinality: "N:1".into(),
        semantic: "belongs_to".into(),
        evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
        column: None,
    }
}

fn opts() -> TtlOptions<'static> {
    TtlOptions {
        project: "demo",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: None,
        schema: None,
        api_classes: None,
    }
}

fn opts_with<'a>(decl: &'a TenancyDecl) -> TtlOptions<'a> {
    TtlOptions {
        project: "demo",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: Some(decl),
        schema: None,
        api_classes: None,
    }
}

fn block_of<'a>(doc: &'a str, subject: &str) -> &'a str {
    let start = doc.find(subject).unwrap_or_else(|| panic!("找不到 {subject}"));
    let rest = &doc[start..];
    let end = rest.find(" .\n").map(|i| i + 3).unwrap_or(rest.len());
    &rest[..end]
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 反向属性完整且对称
// ══════════════════════════════════════════════════════════════

#[test]
fn ac001_every_object_property_gets_an_inverse_and_two_inverse_of() {
    let doc = serialize_model(&model_view(), &opts());
    let d = doc.doc();
    let links = model_view().link_types.len();
    assert_eq!(d.matches("owl:ObjectProperty").count(), links * 2, "正向 + 反向：{d}");
    assert_eq!(d.matches("owl:inverseOf").count(), links * 2, "双向各一条：{d}");

    // 抽取一对：正反 domain/range 必须对调。
    let fwd = block_of(d, ":deductions__customers a ");
    let inv = block_of(d, ":customers__deductions a ");
    assert!(fwd.contains("rdfs:domain :deductions"), "{fwd}");
    assert!(fwd.contains("rdfs:range :customers"), "{fwd}");
    assert!(fwd.contains("owl:inverseOf :customers__deductions"), "{fwd}");
    assert!(inv.contains("rdfs:domain :customers"), "{inv}");
    assert!(inv.contains("rdfs:range :deductions"), "{inv}");
    assert!(inv.contains("owl:inverseOf :deductions__customers"), "{inv}");
}

#[test]
fn ac001_reverse_pair_uses_the_documented_inverse_suffix() {
    let d = serialize_model(&model_view(), &opts()).into_doc();
    // `order → customer_subscriptions` 与 `customer_subscriptions → order` 同时存在：
    // 后者的反向名 `order__customer_subscriptions` 已是一条正向属性 ⇒ 加 `__inverse`。
    assert!(d.contains(":order__customer_subscriptions a "), "{d}");
    assert!(d.contains(":order__customer_subscriptions__inverse a "), "{d}");
    // 两条关系 ⇒ 4 条 inverseOf，且都指向真实存在的块。
    assert_eq!(d.matches("owl:inverseOf").count(), 6, "{d}");
}

// ══════════════════════════════════════════════════════════════
// AC-002 · xsd range
// ══════════════════════════════════════════════════════════════

#[test]
fn ac002_numeric_and_temporal_columns_get_real_xsd_types() {
    let d = serialize_model(&model_view(), &opts()).into_doc();
    assert!(block_of(&d, ":customers__amount a ").contains("rdfs:range xsd:decimal"), "{d}");
    assert!(block_of(&d, ":customers__balance a ").contains("rdfs:range xsd:decimal"), "{d}");
    assert!(
        block_of(&d, ":customers__created_at a ").contains("rdfs:range xsd:dateTime"),
        "{d}"
    );
    assert!(block_of(&d, ":customers__is_deleted a ").contains("rdfs:range xsd:boolean"), "{d}");
    assert!(block_of(&d, ":customers__customer_id a ").contains("rdfs:range xsd:string"), "{d}");
    assert_eq!(d.matches("rdfs:range rdfs:Literal").count(), 0, "不再一律 Literal：{d}");
}

#[test]
fn ac002_unmapped_property_is_string_but_marked_and_listed() {
    let d = serialize_model(&model_view(), &opts()).into_doc();
    let unknown = block_of(&d, ":customers__zzz_unknown a ");
    assert!(unknown.contains("rdfs:range xsd:string"), "{unknown}");
    assert!(unknown.contains("ontok:rangeUnmapped true"), "{unknown}");
    let stats = range_stats(&d);
    assert_eq!(stats.get("literal"), 0);
    assert_eq!(stats.unmapped, vec!["莫名列".to_string()]);
    // 总数 == 属性条数（每一条都有且只有一个 range）。
    assert_eq!(
        stats.get("decimal")
            + stats.get("integer")
            + stats.get("dateTime")
            + stats.get("date")
            + stats.get("boolean")
            + stats.get("string"),
        10,
        "{stats:?}"
    );
}

#[test]
fn ac002_customer_view_also_gets_types_from_business_names() {
    let d = serialize_customer(&customer_view(), &opts()).into_doc();
    assert!(block_of(&d, ":订单__金额 a ").contains("rdfs:range xsd:decimal"), "{d}");
    assert!(block_of(&d, ":订单__下单时间 a ").contains("rdfs:range xsd:dateTime"), "{d}");
    assert!(block_of(&d, ":订单__是否启用 a ").contains("rdfs:range xsd:boolean"), "{d}");
    assert!(block_of(&d, ":客户__余额 a ").contains("rdfs:range xsd:decimal"), "{d}");
    assert!(block_of(&d, ":订单__星云常数 a ").contains("ontok:rangeUnmapped true"), "{d}");
}

// ══════════════════════════════════════════════════════════════
// AC-003 · IRI 模板
// ══════════════════════════════════════════════════════════════

#[test]
fn ac003_every_class_has_a_deterministic_instance_iri_template() {
    let c = serialize_customer(&customer_view(), &opts()).into_doc();
    let m = serialize_model(&model_view(), &opts()).into_doc();
    for (track, d, classes) in [
        ("客户视图", c.as_str(), customer_view().objects.len()),
        ("建模视图", m.as_str(), model_view().entity_types.len()),
    ] {
        assert_eq!(
            d.matches("ontok:instanceIri").count(),
            classes,
            "{track}：每个类一条模板"
        );
        assert!(
            d.contains("ontok:instanceIri \"https://ontok.local/demo#{class}/{id}\""),
            "{track}：模板必须由 base+project 派生"
        );
        assert!(!d.contains("2026-"), "{track}：模板不许含时间");
    }
}

// ══════════════════════════════════════════════════════════════
// AC-004 / AC-005 · 租户标注与未标注警告
// ══════════════════════════════════════════════════════════════

#[test]
fn ac004_signed_tenancy_marks_the_declared_column_only() {
    let decl = TenancyDecl::parse(include_str!("../tenancy.json")).expect("样例声明必须合法");
    let m = serialize_model(&model_view(), &opts_with(&decl)).into_doc();
    // 4 个实体都有 customer_id ⇒ 4 个 tenantKey + 4 个 tenantColumn。
    assert_eq!(m.matches("ontok:tenantKey true").count(), 4, "{m}");
    assert_eq!(m.matches("ontok:tenantColumn \"customer_id\"").count(), 4, "{m}");
    // 未声明的列不许被标。
    assert!(
        !block_of(&m, ":customers__amount a ").contains("tenantKey"),
        "amount 不是租户键：{m}"
    );
    assert!(m.contains("ontok:tenancyReviewedBy"), "{m}");
    assert!(!m.contains("ontok:tenantUnannotated"), "已签且命中：{m}");
    assert!(tenancy_report(&model_view(), Some(&decl)).warning().is_none());
}

#[test]
fn ac004_tenant_annotations_never_appear_in_the_customer_view() {
    let decl = TenancyDecl::parse(include_str!("../tenancy.json")).expect("样例声明必须合法");
    let c = serialize_customer(&customer_view(), &opts_with(&decl)).into_doc();
    assert_eq!(c.to_lowercase().matches("tenant").count(), 0, "客户视图不许有 tenant*：{c}");
}

#[test]
fn ac004_unsigned_tenancy_is_refused_but_visible() {
    let decl = TenancyDecl::parse(r#"{"columns":["customer_id"],"reviewed_by":null}"#)
        .expect("合法 JSON");
    let m = serialize_model(&model_view(), &opts_with(&decl)).into_doc();
    assert!(!m.contains("ontok:tenantKey true"), "未签不落标注：{m}");
    assert!(m.contains("ontok:tenancyReviewed false"), "{m}");
    assert!(m.contains("ontok:tenantUnannotated true"), "{m}");
    assert!(m.contains("# · 租户键：未签"), "文件头必须体现未签：{m}");
}

#[test]
fn ac005_missing_tenancy_has_all_three_marks() {
    let model = model_view();
    let doc = serialize_model(&model, &opts());
    let d = doc.doc();
    assert!(d.contains("# · 租户键：未标注"), "① 文件头：{d}");
    assert!(d.contains("ontok:tenantUnannotated true"), "② 本体节点：{d}");
    let warning = tenancy_report(&model, None).warning().expect("③ 必须有 stderr 警告");
    assert!(warning.contains("不能直接给数字员工用"), "{warning}");
}

// ══════════════════════════════════════════════════════════════
// AC-006 / AC-007 · 确定性、两版视图
// ══════════════════════════════════════════════════════════════

#[test]
fn ac006_two_exports_are_byte_identical_and_order_independent() {
    let a = serialize_model(&model_view(), &opts()).into_doc();
    let b = serialize_model(&model_view(), &opts()).into_doc();
    assert_eq!(a, b, "两次导出必须逐字节相同");

    // 输入顺序打乱 ⇒ 输出必须一样（挡 HashMap 迭代序）。
    let mut rev = model_view();
    rev.entity_types.reverse();
    for e in &mut rev.entity_types {
        e.properties.reverse();
    }
    rev.link_types.reverse();
    assert_eq!(a, serialize_model(&rev, &opts()).into_doc(), "输出依赖了输入顺序");

    let ca = serialize_customer(&customer_view(), &opts()).into_doc();
    let mut cr = customer_view();
    cr.objects.reverse();
    for o in &mut cr.objects {
        o.properties.reverse();
    }
    cr.relations.reverse();
    assert_eq!(ca, serialize_customer(&cr, &opts()).into_doc(), "客户视图输出依赖了输入顺序");
}

#[test]
fn ac007_customer_has_the_capabilities_but_no_tenancy() {
    let decl = TenancyDecl::parse(include_str!("../tenancy.json")).expect("合法");
    let c = serialize_customer(&customer_view(), &opts_with(&decl)).into_doc();
    let m = serialize_model(&model_view(), &opts_with(&decl)).into_doc();
    for (name, d) in [("客户视图", &c), ("建模视图", &m)] {
        assert!(d.contains("owl:inverseOf"), "{name} 缺反向属性");
        assert!(d.contains("rdfs:range xsd:"), "{name} 缺 xsd range");
        assert!(d.contains("ontok:instanceIri"), "{name} 缺 IRI 模板");
    }
    assert!(!c.contains("tenantKey") && !c.contains("tenantColumn"), "客户视图不许有租户标注");
    assert!(m.contains("tenantKey") && m.contains("tenantColumn"), "建模视图必须有租户标注");
}

// ══════════════════════════════════════════════════════════════
// A-1 · 真实列类型（--schema）
// ══════════════════════════════════════════════════════════════

/// 一份最小的 `ontok-introspect` schema（故意与启发式对着干）：
/// `amount` 是 varchar（启发式会当 decimal）、`status` 是 integer（启发式会当 string）、
/// `version_number` 是 varchar（启发式会当 integer）、`point_change` 是 numeric。
const SCHEMA_FIXTURE: &str = r#"{"tables":[
  {"name":"customers","columns":[
    {"name":"customer_id","data_type":"character varying"},
    {"name":"balance","data_type":"numeric"},
    {"name":"created_at","data_type":"timestamp without time zone"},
    {"name":"is_deleted","data_type":"integer"},
    {"name":"amount","data_type":"character varying"},
    {"name":"status","data_type":"integer"},
    {"name":"version_number","data_type":"character varying"},
    {"name":"point_change","data_type":"numeric"}
  ]},
  {"name":"deductions","columns":[
    {"name":"customer_id","data_type":"character varying"},
    {"name":"amount","data_type":"numeric"}
  ]},
  {"name":"order","columns":[{"name":"customer_id","data_type":"character varying"}]},
  {"name":"customer_subscriptions","columns":[{"name":"customer_id","data_type":"character varying"}]}
]}"#;

fn schema_model() -> ModelTrackView {
    let mut v = model_view();
    for (name, column) in [
        ("状态", "status"),
        ("版本号", "version_number"),
        ("点数变化量", "point_change"),
    ] {
        v.entity_types[0].properties.push(prop(name, column));
    }
    v
}

fn schema_opts<'a>(schema: &'a SchemaTypes) -> TtlOptions<'a> {
    TtlOptions {
        project: "demo",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: None,
        schema: Some(schema),
        api_classes: None,
    }
}

#[test]
fn a1_schema_types_override_heuristic_per_column() {
    let schema = SchemaTypes::parse(SCHEMA_FIXTURE).expect("schema 合法");
    let doc = serialize_model(&schema_model(), &schema_opts(&schema));
    let d = doc.doc();

    // 真实类型压过启发式（这四条都是启发式会判错的形态）。
    assert!(block_of(d, ":customers__amount a ").contains("rdfs:range xsd:string"), "{d}");
    assert!(block_of(d, ":customers__status a ").contains("rdfs:range xsd:integer"), "{d}");
    assert!(
        block_of(d, ":customers__version_number a ").contains("rdfs:range xsd:string"),
        "{d}"
    );
    assert!(
        block_of(d, ":customers__point_change a ").contains("rdfs:range xsd:decimal"),
        "{d}"
    );
    for col in ["amount", "status", "version_number", "point_change", "created_at"] {
        assert!(
            block_of(d, &format!(":customers__{col} a ")).contains("ontok:rangeSource \"schema\""),
            "{col} 必须标 schema 来源：{d}"
        );
    }
    // 没匹配上的列：启发式 + 未映射 + 计数。
    let unknown = block_of(d, ":customers__zzz_unknown a ");
    assert!(unknown.contains("ontok:rangeSource \"heuristic\""), "{unknown}");
    assert!(unknown.contains("ontok:rangeUnmapped true"), "{unknown}");
    let stats = range_stats(d);
    assert_eq!(stats.source("schema"), 12, "{stats:?}");
    assert_eq!(stats.source("heuristic"), 1, "{stats:?}");
    assert_eq!(stats.heuristic, vec!["莫名列".to_string()]);
    assert!(d.contains("ontok:rangeSourceSchema 12"), "{d}");
    assert!(d.contains("ontok:rangeSourceHeuristic 1"), "{d}");
    assert!(d.contains("# · xsd 类型来源：真实列类型"), "{d}");
}

#[test]
fn a1_no_schema_has_all_three_marks() {
    let doc = serialize_model(&model_view(), &opts());
    let d = doc.doc();
    assert!(d.contains("# · xsd 类型来源：**启发式**（未读真实列类型"), "① 文件头：{d}");
    assert!(d.contains("ontok:rangeSourceHeuristic 10"), "② 本体节点计数：{d}");
    assert!(
        block_of(d, ":customers__amount a ").contains("ontok:rangeSource \"heuristic\""),
        "③ 属性来源标记：{d}"
    );
    let stats = range_stats(d);
    assert_eq!(stats.source("heuristic"), 10, "{stats:?}");
    assert_eq!(stats.source("schema"), 0, "{stats:?}");
}

#[test]
fn a1_schema_parse_is_fail_closed_and_normalizes_names() {
    assert!(SchemaTypes::parse("{not json").is_err(), "坏 JSON 必须报错");
    let s = SchemaTypes::parse(
        r#"{"tables":[{"name":" Customers ","columns":[{"name":" Amount ","data_type":"NUMERIC(10,2)"}]}]}"#,
    )
    .expect("合法");
    assert_eq!(s.data_type("customers", "amount"), Some("numeric(10,2)"));
    assert_eq!(s.data_type("CUSTOMERS", "AMOUNT"), Some("numeric(10,2)"));
    assert!(s.data_type("customers", "missing").is_none());
}

#[test]
fn a2_collision_suffix_keeps_the_required_52_and_104_counts() {
    // 52 条对象属性：50 条链式 + 一对互为反向（t50→t51 / t51→t50）。
    let mut v = ModelTrackView {
        seq: 3,
        extracted_by: None,
        entity_types: (0..52)
            .map(|i| ModelEntityView {
                key: format!("t{i}"),
                display_name: format!("T{i}"),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            })
            .collect(),
        link_types: Vec::new(),
        action_types: vec![],
        functions: vec![],
    };
    for i in 0..50 {
        v.link_types.push(link(&format!("l{i}"), &format!("t{i}"), &format!("t{}", i + 1)));
    }
    v.link_types.push(link("l50", "t50", "t51"));
    v.link_types.push(link("l51", "t51", "t50"));
    assert_eq!(v.link_types.len(), 52);

    let d = serialize_model(&v, &opts()).into_doc();
    assert_eq!(d.matches("owl:ObjectProperty").count(), 104, "52 正 + 52 反");
    assert_eq!(d.matches("owl:inverseOf").count(), 104, "双向各一条");
    // 命名冲突：`t51→t50` 的正向名被 `t50→t51` 的反向名占用 ⇒ 加 `__inverse`。
    assert!(d.contains(":t51__t50 a "), "{d}");
    assert!(d.contains(":t51__t50__inverse a "), "冲突必须加后缀：{d}");
    assert!(
        d.contains("owl:inverseOf :t51__t50__inverse"),
        "正向必须指到带后缀的反向：{d}"
    );
}

// ══════════════════════════════════════════════════════════════
// B-1 / B-2 / B-3 · 替身实验抓到的三处
// ══════════════════════════════════════════════════════════════

#[test]
fn b1_n_to_one_is_functional_and_one_to_n_is_inverse_functional() {
    let d = serialize_model(&model_view(), &opts()).into_doc();
    assert_eq!(
        d.matches("a owl:FunctionalProperty, owl:ObjectProperty").count(),
        3,
        "3 条 N:1 正向必须全是 Functional"
    );
    assert_eq!(
        d.matches("a owl:InverseFunctionalProperty, owl:ObjectProperty").count(),
        3,
        "3 条 1:N 反向必须全是 InverseFunctional"
    );
    assert!(
        block_of(&d, ":deductions__customers a ").contains("a owl:FunctionalProperty"),
        "N:1（deductions → customers）必须 Functional：{d}"
    );
    assert!(
        block_of(&d, ":customers__deductions a ").contains("a owl:InverseFunctionalProperty"),
        "1:N（反向）必须 InverseFunctional：{d}"
    );

    // 客户视图同一口径：多对一 ⇒ Functional；一对多 ⇒ InverseFunctional。
    let c = serialize_customer(&customer_view(), &opts()).into_doc();
    assert!(block_of(&c, ":订单__客户 a ").contains("a owl:FunctionalProperty"), "{c}");
    assert!(
        block_of(&c, ":客户__订单 a ").contains("a owl:InverseFunctionalProperty"),
        "{c}"
    );
}

const SCHEMA_PK: &str = r#"{"tables":[
  {"name":"customers","primary_key":"customer_id","columns":[
    {"name":"customer_id","data_type":"character varying","is_primary_key":true},
    {"name":"amount","data_type":"numeric"}
  ]},
  {"name":"deductions","primary_key":"id","columns":[
    {"name":"id","data_type":"character varying","is_primary_key":true},
    {"name":"amount","data_type":"numeric"}
  ]},
  {"name":"order","primary_key":"id","columns":[
    {"name":"id","data_type":"character varying","is_primary_key":true}
  ]},
  {"name":"customer_subscriptions","primary_key":"id","columns":[
    {"name":"id","data_type":"character varying","is_primary_key":true}
  ]}
]}"#;

#[test]
fn b2_primary_key_is_exported_even_when_extraction_dropped_it() {
    let schema = SchemaTypes::parse(SCHEMA_PK).expect("schema 合法");
    let d = serialize_model(&model_view(), &schema_opts(&schema)).into_doc();

    // 每个类一条主键列；84 条类在真库上的判据在 live 测试里。
    assert_eq!(d.matches("ontok:primaryKeyColumn").count(), 4, "{d}");
    assert_eq!(d.matches("ontok:isPrimaryKey true").count(), 4, "{d}");
    assert!(
        block_of(&d, ":customers a owl:Class").contains("ontok:primaryKeyColumn \"customer_id\""),
        "{d}"
    );
    // 抽取侧没有 `id` 属性 ⇒ 必须补一条，否则 FK 串无法还原成 IRI。
    let pk = block_of(&d, ":deductions__id a ");
    assert!(pk.contains("rdfs:range xsd:string"), "{pk}");
    assert!(pk.contains("ontok:column \"id\""), "{pk}");
    assert!(pk.contains("ontok:nullable false"), "{pk}");
    assert!(pk.contains("ontok:isPrimaryKey true"), "{pk}");
    assert!(pk.contains("ontok:rangeSource \"schema\""), "{pk}");
    // 已在属性清单里的主键不重复造块，只加标记。
    assert_eq!(d.matches(":deductions__id a owl:DatatypeProperty").count(), 1, "{d}");
    assert!(
        block_of(&d, ":customers__customer_id a ").contains("ontok:isPrimaryKey true"),
        "{d}"
    );
}

#[test]
fn b3_allowed_values_come_only_from_explicit_comments() {
    assert_eq!(
        parse_allowed_values("状态(0-未开始,1-活动中,2-已结束,3-已暂停,-1-已删除)").as_deref(),
        Some("0=未开始;1=活动中;2=已结束;3=已暂停;-1=已删除")
    );
    assert_eq!(
        parse_allowed_values("扣减币种：实际扣减的币种（POINT/RMB/USD）").as_deref(),
        Some("POINT;RMB;USD")
    );
    assert_eq!(
        parse_allowed_values("充值币种: RMB-人民币, USD-美元, POINT-免费点数").as_deref(),
        Some("RMB=人民币;USD=美元;POINT=免费点数")
    );
    assert_eq!(
        parse_allowed_values("计费周期：monthly/quarterly/semi_yearly/yearly").as_deref(),
        Some("monthly;quarterly;semi_yearly;yearly")
    );
    // ⛔ 解析不出就不导出：举例 / IPv4 兼容 / 单位 / NULL 伪码全部拒绝。
    for bad in [
        "模型系列(如: GPT, Claude, GLM)",
        "最后一次登录IP地址（兼容IPv4/IPv6）",
        "客户模型级TPM覆盖配置，保存原始tokens/minute",
        "客户ID；NULL=全局放行；非NULL=仅对该客户放行",
        "运行状态：1-正常运行，其他值待定义",
    ] {
        assert_eq!(parse_allowed_values(bad), None, "不该从 `{bad}` 编出值域");
    }

    let mut v = model_view();
    v.entity_types[0].properties.push(ModelPropertyView {
        name: "状态".into(),
        column: "status".into(),
        nullable: false,
        note: Some("状态(0-未开始,1-活动中,2-已结束)".into()),
        sample: None,
    });
    let d = serialize_model(&v, &opts()).into_doc();
    assert!(
        block_of(&d, ":customers__status a ")
            .contains("ontok:allowedValue \"0=未开始;1=活动中;2=已结束\""),
        "{d}"
    );
    assert_eq!(d.matches("ontok:allowedValue").count(), 1, "{d}");
}

// ══════════════════════════════════════════════════════════════
// AC-010 · 文档
// ══════════════════════════════════════════════════════════════

#[test]
fn ac010_module_header_documents_the_three_conventions() {
    let src = common::read_source("src/domain/ttl.rs");
    for keyword in [
        "ontok:instanceIri",
        "反向属性",
        "xsd:decimal",
        "{class}",
        "{id}",
        "--schema",
        "ontok:rangeSource",
        "xsd_for_sql_type",
        "ontok:primaryKeyColumn",
        "ontok:allowedValue",
    ] {
        assert!(src.contains(keyword), "ttl.rs 模块头缺少 `{keyword}` 的说明");
    }
}

// ══════════════════════════════════════════════════════════════
// 真库（anspire_grouped v3 已冻结）：计数 / 体积 / 确定性
// ══════════════════════════════════════════════════════════════

/// 升级前客户视图的字节数（REQ-110 基线）。AC-008：增长 ≤ 40%。
const BASELINE_CUSTOMER_BYTES: usize = 220_696;

#[tokio::test]
async fn live_grouped_v3_counts_and_volume_are_within_the_contract() {
    use ontok::db::project::PgProjectStore;
    use ontok::db::snapshot_store::PgStore;

    // 没有测试库就**明确**跳过（不是假绿：报告里写清是环境缺失）。
    if std::env::var(ontok::config::TEST_DATABASE_URL_ENV).is_err() {
        eprintln!("· 缺 ONTOK_TEST_DATABASE_URL ⇒ 跳过真库计数（结构判据仍在上面的测试里跑）");
        return;
    }
    let pool = common::test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let Some(p) = ps.get_by_name("anspire_grouped").await.expect("查项目失败") else {
        eprintln!("· 库里没有 `anspire_grouped` ⇒ 跳过真库计数");
        return;
    };
    let Ok(v) = ps.version_by_seq(p.id, &p.name, 3).await else {
        eprintln!("· `anspire_grouped` 没有 v3 ⇒ 跳过");
        return;
    };
    let snap = store.get_snapshot(v.id).await.expect("读快照失败").expect("v3 应当有快照");
    let model = ontok::domain::model_view::project_model_track(&snap);
    let cv = ontok::domain::view::project(&ontok::cli::snapshot_to_view(&snap));

    let link_count = model.link_types.len();
    assert_eq!(link_count, 52, "AC-001 的数据前提：52 条对象属性");

    let o = TtlOptions {
        project: "anspire_grouped",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: None,
        api_classes: None,
    };
    let m = serialize_model(&model, &o).into_doc();
    let c = serialize_customer(&cv, &o).into_doc();

    // AC-001：52 条反向属性 + 104 条 owl:inverseOf（双向）。
    assert_eq!(m.matches("owl:ObjectProperty").count(), link_count * 2, "建模视图反向属性数");
    assert_eq!(c.matches("owl:ObjectProperty").count(), link_count * 2, "客户视图反向属性数");
    assert_eq!(m.matches("owl:inverseOf").count(), link_count * 2, "建模视图 inverseOf");
    assert_eq!(c.matches("owl:inverseOf").count(), link_count * 2, "客户视图 inverseOf");

    // AC-002：一条 rdfs:Literal 都不剩；数值列必须成规模存在。
    assert_eq!(m.matches("rdfs:range rdfs:Literal").count(), 0, "建模视图仍有 Literal");
    assert_eq!(c.matches("rdfs:range rdfs:Literal").count(), 0, "客户视图仍有 Literal");
    let ms = range_stats(&m);
    let cs = range_stats(&c);
    assert!(ms.numeric() >= 100, "数值列太少：{ms:?}");
    assert!(cs.numeric() >= 100, "数值列太少：{cs:?}");
    assert_eq!(ms.unmapped.len(), 1, "本版真实数据只有 `verify` 认不出：{ms:?}");

    // AC-003：每个类都有模板。
    assert_eq!(m.matches("ontok:instanceIri").count(), model.entity_types.len());
    assert_eq!(c.matches("ontok:instanceIri").count(), cv.objects.len());

    // AC-005 / AC-007：建模视图有未标注标记，客户视图连 tenant 都没有。
    assert!(m.contains("ontok:tenantUnannotated true"));
    assert_eq!(c.to_lowercase().matches("tenant").count(), 0);

    // AC-008：体积 ≤ 40% 增长。
    let bytes = c.len();
    let limit = BASELINE_CUSTOMER_BYTES * 140 / 100;
    assert!(
        bytes <= limit,
        "客户视图 {} 字节，超过基线 {} 字节的 140%（上限 {}）",
        bytes,
        BASELINE_CUSTOMER_BYTES,
        limit
    );
    eprintln!(
        "REQ-119 真库实测：客户视图 {} 字节（基线 {}，增长 {:.1}%）· 建模视图 {} 字节",
        bytes,
        BASELINE_CUSTOMER_BYTES,
        (bytes as f64 - BASELINE_CUSTOMER_BYTES as f64) / BASELINE_CUSTOMER_BYTES as f64 * 100.0,
        m.len()
    );

    // AC-006：两次导出逐字节相同。
    assert_eq!(m, serialize_model(&model, &o).into_doc(), "建模视图两次导出不一致");
    assert_eq!(c, serialize_customer(&cv, &o).into_doc(), "客户视图两次导出不一致");
}

/// A-1 的**最终判据**：给真实 schema 后，870 条数据类型属性的 range 与
/// `schema.json` 的 `data_type` **逐条一致**（0 条启发式、0 条不一致）。
///
/// schema 路径优先读 `ONTOK_SCHEMA_JSON`，否则用仓库外的 `../code/out/schema.json`；
/// 文件不存在 ⇒ 明确跳过（不是假绿：报告里写清是环境缺失）。
#[tokio::test]
async fn live_schema_makes_all_ranges_match_real_data_types() {
    use ontok::db::project::PgProjectStore;
    use ontok::db::snapshot_store::PgStore;
    use ontok::domain::ttl::local_model;

    if std::env::var(ontok::config::TEST_DATABASE_URL_ENV).is_err() {
        eprintln!("· 缺 ONTOK_TEST_DATABASE_URL ⇒ 跳过 A-1 真库逐条比对");
        return;
    }
    let schema_path = std::env::var("ONTOK_SCHEMA_JSON")
        .unwrap_or_else(|_| "../code/out/schema.json".to_string());
    let Ok(raw) = std::fs::read_to_string(&schema_path) else {
        eprintln!("· 读不到 {schema_path} ⇒ 跳过 A-1 逐条比对（设置 ONTOK_SCHEMA_JSON）");
        return;
    };
    let schema = SchemaTypes::parse(&raw).expect("schema.json 合法");

    let pool = common::test_pool().await;
    let ps = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let Some(p) = ps.get_by_name("anspire_grouped").await.expect("查项目失败") else {
        eprintln!("· 库里没有 `anspire_grouped` ⇒ 跳过");
        return;
    };
    let Ok(v) = ps.version_by_seq(p.id, &p.name, 3).await else {
        eprintln!("· 没有 v3 ⇒ 跳过");
        return;
    };
    let snap = store.get_snapshot(v.id).await.expect("读快照失败").expect("v3 应当有快照");
    let model = ontok::domain::model_view::project_model_track(&snap);
    let o = TtlOptions {
        project: "anspire_grouped",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema),
        api_classes: None,
    };
    let m = serialize_model(&model, &o).into_doc();
    let stats = range_stats(&m);
    let total: usize = model.entity_types.iter().map(|e| e.properties.len()).sum();
    assert_eq!(total, 870, "数据前提：870 条数据类型属性");
    // B-2：主键通常被抽取侧剥掉 ⇒ 由 schema 补 82 条；全部仍是 schema 来源。
    let datatype_props = m.matches("a owl:DatatypeProperty").count();
    assert_eq!(datatype_props, 952, "870 + 补出的 82 条主键属性");
    assert_eq!(stats.source("schema"), datatype_props, "所有属性都必须来自真实 schema：{stats:?}");
    assert_eq!(stats.source("heuristic"), 0, "schema 模式不许回退启发式：{stats:?}");
    assert_eq!(stats.unmapped.len(), 0, "schema 模式不应有未映射：{stats:?}");

    // B-2：84 个类都有单列主键；`id` 属性 82 条（另 2 个类主键不叫 id）。
    assert_eq!(m.matches("ontok:primaryKeyColumn").count(), 84, "每个类一条主键列声明");
    assert_eq!(m.matches("ontok:isPrimaryKey true").count(), 84, "84 条主键属性");
    let id_props = m.matches("__id a owl:DatatypeProperty").count();
    assert_eq!(id_props, 82, "82 个类的主键列名是 `id`：{id_props}");

    // B-1：52 条 N:1 全部 Functional；52 条反向（1:N）全部 InverseFunctional。
    let fwd_functional = m.matches("a owl:FunctionalProperty, owl:ObjectProperty").count();
    let inv_functional = m.matches("a owl:InverseFunctionalProperty, owl:ObjectProperty").count();
    assert_eq!(fwd_functional, 52, "N:1 正向必须 Functional");
    assert_eq!(inv_functional, 52, "1:N 反向必须 InverseFunctional");

    // B-3：注释里能确定性解析出的值域列。
    assert_eq!(m.matches("ontok:allowedValue").count(), 104, "值域列数");

    let mut mismatched: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for e in &model.entity_types {
        for prop in &e.properties {
            let column = if prop.column.trim().is_empty() {
                prop.name.trim()
            } else {
                prop.column.trim()
            };
            let Some(sql) = schema.data_type(&e.key, column) else {
                missing.push(format!("{}.{}", e.key, column));
                continue;
            };
            let want = xsd_for_sql_type(sql)
                .unwrap_or_else(|| panic!("schema 类型认不出：{}.{} = {sql}", e.key, column));
            let subject = format!(":{}__{}", local_model(&e.key), local_model(column));
            let block = block_of(&m, &format!("{subject} a "));
            if !block.contains(&format!("rdfs:range {}", want.iri())) {
                mismatched.push(format!(
                    "{}.{} | 真实 {sql} | 期望 {} | 实际块：{}",
                    e.key,
                    column,
                    want.iri(),
                    block.lines().find(|l| l.contains("rdfs:range")).unwrap_or("<无>")
                ));
            }
        }
    }
    assert!(missing.is_empty(), "schema 缺列：{missing:?}");
    assert!(
        mismatched.is_empty(),
        "A-1 失败：{} 条与真实类型不一致（前 20 条）：\n{}",
        mismatched.len(),
        mismatched.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
    eprintln!(
        "A-1 实测：{total} 条属性 · schema {} 条 · heuristic {} 条 · 不一致 0 条",
        stats.source("schema"),
        stats.source("heuristic")
    );
}
