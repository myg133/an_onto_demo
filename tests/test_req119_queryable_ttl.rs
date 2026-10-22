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
    range_stats, serialize_customer, serialize_model, tenancy_report, TenancyDecl, TtlOptions,
    DEFAULT_BASE,
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
    }
}

fn opts() -> TtlOptions<'static> {
    TtlOptions {
        project: "demo",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: None,
    }
}

fn opts_with<'a>(decl: &'a TenancyDecl) -> TtlOptions<'a> {
    TtlOptions {
        project: "demo",
        version: 3,
        base: DEFAULT_BASE,
        action_type_count: 0,
        tenancy: Some(decl),
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
// AC-010 · 文档
// ══════════════════════════════════════════════════════════════

#[test]
fn ac010_module_header_documents_the_three_conventions() {
    let src = common::read_source("src/domain/ttl.rs");
    for keyword in ["ontok:instanceIri", "反向属性", "xsd:decimal", "{class}", "{id}"] {
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
