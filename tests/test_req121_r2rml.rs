//! REQ-121 · **R2RML 映射生成**（AC-001 … AC-012）—— 纯函数级判据 + 真实进程 `--help`。
//!
//! # 这个文件补的是哪一格
//!
//! `src/domain/r2rml.rs` 的单测钉的是**单元行为**；本文件从**公开 API** 复验一遍
//! 验收标准里最容易被「改一个量、少一条断言」磨掉的几条：
//!
//! | AC | 本文件的判据 |
//! | --- | --- |
//! | AC-001 | 每个 TriplesMap 三件套齐全（logicalTable / subjectMap / ≥1 POM） |
//! | AC-002 | `rr:template` 由 `ontok:instanceIri` 展开（`{id}` → 主键列），且与 TTL 同源 |
//! | AC-003 | `rr:datatype` 与 TTL 的 `rdfs:range` **逐条**相等 |
//! | AC-004 | `true` 边 → joinCondition（child / parent） |
//! | AC-005 | `inherit` 默认不生成；`--include-inherit` 生成且带 `ontok:joinState` |
//! | AC-006 | `false` 边任何模式都不生成 join；文件尾列候选 + 原因 |
//! | AC-007 | 已签租户声明的类写出 `ontok:tenantColumn` |
//! | AC-008 | 对账失败**点名**（表.列）且不产出文档 |
//! | AC-010 | 两次生成逐字节相同 |
//! | AC-011 | 复合主键类跳过并列入清单 |
//!
//! ⚠️ 本文件**不**连库：`generate` 是纯函数（输入是建模视图 + schema）。
//! 「真实库实测」是交付物里的一次性证据（命令 + 数字），不是这里的恒常判据 ——
//! 那需要一套专属 schema 夹具（连 `ancestors` 表都要造），成本与本文件要守的东西不成比例。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（同 REQ-107 的 A7-1）。

use std::process::Command;

use ontok::domain::model_view::{
    ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
};
use ontok::domain::r2rml::{generate, R2rmlDocument, R2rmlError, R2rmlStats};
use ontok::domain::snapshot::{Evidence, JoinState};
use ontok::domain::ttl::{serialize_model, SchemaTypes, TenancyDecl, TtlOptions};

// ══════════════════════════════════════════════════════════════
// 夹具：两个类、三种证据等级各一条边
// ══════════════════════════════════════════════════════════════

fn view() -> ModelTrackView {
    ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![
            ModelEntityView {
                key: "orders".into(),
                display_name: "订单".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![
                    ModelPropertyView {
                        name: "金额".into(),
                        column: "amount".into(),
                        nullable: false,
                        note: None,
                        sample: None,
                    },
                    ModelPropertyView {
                        name: "客户".into(),
                        column: "customer_id".into(),
                        nullable: false,
                        note: None,
                        sample: None,
                    },
                ],
                evidence: vec![],
            },
            ModelEntityView {
                key: "customers".into(),
                display_name: "客户".into(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![ModelPropertyView {
                    name: "客户号".into(),
                    column: "customer_no".into(),
                    nullable: false,
                    note: None,
                    sample: None,
                }],
                evidence: vec![],
            },
        ],
        link_types: vec![
            ModelLinkView {
                key: "orders_customer_id__customers".into(),
                from_type: "orders".into(),
                to_type: "customers".into(),
                cardinality: "N:1".into(),
                semantic: "belongs_to".into(),
                evidence: vec![Evidence {
                    table: "orders".into(),
                    column: "customer_id".into(),
                    claim: "外键".into(),
                }],
                confidence: None,
                join_verified: Some(JoinState::Verified),
                join_evidence: Some(
                    "value-overlap 5/5 (100.00%) -> customers.id via primary_key (plural)".into(),
                ),
                column: Some("customer_id".into()),
            },
            ModelLinkView {
                key: "orders_license_id__licenses".into(),
                from_type: "orders".into(),
                to_type: "customers".into(),
                cardinality: "N:1".into(),
                semantic: "belongs_to".into(),
                evidence: vec![Evidence {
                    table: "orders".into(),
                    column: "license_id".into(),
                    claim: "外键".into(),
                }],
                confidence: None,
                join_verified: Some(JoinState::Inherited),
                join_evidence: Some(
                    "inherited from license_quota.license_id (verified 5/5) -> customers.id via primary_key (plural)"
                        .into(),
                ),
                column: Some("license_id".into()),
            },
            ModelLinkView {
                key: "orders_amount__customers".into(),
                from_type: "orders".into(),
                to_type: "customers".into(),
                cardinality: "N:1".into(),
                semantic: "belongs_to".into(),
                evidence: vec![],
                confidence: None,
                join_verified: Some(JoinState::NamingOnly),
                join_evidence: Some("insufficient_sample: 有效样本 1 行 < 3 条下限".into()),
                column: Some("amount".into()),
            },
        ],
        action_types: vec![],
        functions: vec![],
    }
}

const SCHEMA: &str = r#"{"tables":[
    {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
     "columns":[{"name":"order_id","data_type":"bigint","is_primary_key":true},
                {"name":"customer_id","data_type":"bigint"},
                {"name":"license_id","data_type":"bigint"},
                {"name":"amount","data_type":"numeric(10,2)"}]},
    {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id","customer_no"],
     "columns":[{"name":"id","data_type":"bigint","is_primary_key":true},
                {"name":"customer_no","data_type":"text"}]}
]}"#;

fn schema() -> SchemaTypes {
    SchemaTypes::parse(SCHEMA).expect("夹具 schema 必须能解析")
}

fn opts<'a>(schema: &'a SchemaTypes, tenancy: Option<&'a TenancyDecl>) -> TtlOptions<'a> {
    let mut o = TtlOptions::new("demo", 2);
    o.schema = Some(schema);
    o.tenancy = tenancy;
    o
}

fn stats() -> R2rmlStats {
    gen_doc(false).expect("generate").stats().clone()
}

/// 夹具上的生成（`schema` / `view` 都是临时值，所以在函数里持有它们）。
fn gen_doc(include_inherit: bool) -> Result<R2rmlDocument, R2rmlError> {
    let s = schema();
    let v = view();
    generate(&v, &opts(&s, None), include_inherit)
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 每个类一个 TriplesMap，三件套齐全，没有空壳
// ══════════════════════════════════════════════════════════════

#[test]
fn ac001_every_triples_map_has_the_required_three_parts() {
    let doc = gen_doc(false).expect("generate");
    let text = doc.doc();
    assert_eq!(doc.stats().triples_maps, 2);
    assert_eq!(text.matches("a rr:TriplesMap").count(), 2);
    // 三件套：logicalTable（含 tableName）+ subjectMap（template + class）+ ≥1 POM
    for table in ["orders", "customers"] {
        let block = text
            .split(&format!("map:TM_{table} "))
            .nth(1)
            .unwrap_or_else(|| panic!("找不到 {table} 的 TriplesMap"));
        // 以「下一条 TriplesMap」或文件尾为界
        let block = block.split("\nmap:TM_").next().unwrap_or(block);
        let block = block.split("\n# ──").next().unwrap_or(block);
        assert!(block.contains(&format!("rr:logicalTable [ rr:tableName \"{table}\" ]")), "{block}");
        assert!(block.contains("rr:subjectMap ["), "{block}");
        assert!(block.contains("rr:template "), "{block}");
        assert!(block.contains(&format!("rr:class :{table}")), "{block}");
        assert!(block.contains("rr:predicateObjectMap ["), "空壳 TriplesMap：{block}");
    }
    // 数据类型 POM 数 = orders(id 补出来的 + amount + customer_id) + customers(id + customer_no)
    assert_eq!(doc.stats().datatype_maps, 5, "{:?}", doc.stats());
}

// ══════════════════════════════════════════════════════════════
// AC-002 · subject template 由 ontok:instanceIri 展开
// ══════════════════════════════════════════════════════════════

#[test]
fn ac002_subject_template_is_the_instance_iri_with_the_pk_column() {
    let doc = gen_doc(false).expect("generate");
    let text = doc.doc();
    // `{id}` 被换成**主键列名**（orders 的主键是 order_id，不是 id）
    assert!(
        text.contains("rr:template \"https://ontok.local/demo#orders/{order_id}\""),
        "template 必须由 ontok:instanceIri 展开：{text}"
    );
    assert!(text.contains("rr:template \"https://ontok.local/demo#customers/{id}\""));
    // 展开的来源就是 TTL 里那一条（同一个函数）
    let s = schema();
    let ttl = serialize_model(&view(), &opts(&s, None));
    assert!(ttl
        .doc()
        .contains("ontok:instanceIri \"https://ontok.local/demo#{class}/{id}\""));
    // 所有 template 里的占位符都是**列名**（不是 `{class}` / `{id}` 这种抽象位）
    assert!(!text.contains("{class}"), "未展开的 {{class}} 漏进了 mapping：{text}");
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 数据类型属性 → 列 + xsd，与 TTL 的 rdfs:range 一致
// ══════════════════════════════════════════════════════════════

#[test]
fn ac003_datatypes_are_identical_to_the_ttl_ranges() {
    let s = schema();
    let o = opts(&s, None);
    let doc = generate(&view(), &o, false).expect("generate");
    let ttl = serialize_model(&view(), &o);
    let mut checked = 0usize;
    for (prop, want) in [
        ("orders__amount", "xsd:decimal"),
        ("orders__customer_id", "xsd:integer"),
        ("orders__order_id", "xsd:integer"),
        ("customers__customer_no", "xsd:string"),
    ] {
        let ttl_block = ttl
            .doc()
            .split(&format!(":{prop} "))
            .nth(1)
            .unwrap_or_else(|| panic!("TTL 里找不到 {prop}"));
        assert!(
            ttl_block.lines().take(8).any(|l| l.contains(&format!("rdfs:range {want}"))),
            "{prop} 的 TTL range 不是 {want}：{ttl_block}"
        );
        let r2_block = doc
            .doc()
            .split(&format!("rr:predicate :{prop} ;"))
            .nth(1)
            .unwrap_or_else(|| panic!("R2RML 里找不到 {prop}"));
        assert!(
            r2_block.lines().take(4).any(|l| l.contains(&format!("rr:datatype {want}"))),
            "{prop} 的 rr:datatype 不是 {want}：{r2_block}"
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

// ══════════════════════════════════════════════════════════════
// AC-004 / AC-005 / AC-006 · 三态
// ══════════════════════════════════════════════════════════════

#[test]
fn ac004_verified_edge_becomes_a_join_condition_with_evidence_comment() {
    let doc = gen_doc(false).expect("generate");
    let text = doc.doc();
    assert_eq!(doc.stats().verified_edges, 1);
    assert_eq!(doc.stats().join_conditions(), 1);
    assert!(text.contains("rr:joinCondition [ rr:child \"customer_id\" ; rr:parent \"id\" ]"));
    assert!(text.contains("rr:parentTriplesMap map:TM_customers"));
    assert!(
        text.contains(
            "# joinEvidence: value-overlap 5/5 (100.00%) -> customers.id via primary_key (plural)"
        ),
        "AC-004 要求每条 join 旁有一行注释写明证据：{text}"
    );
}

#[test]
fn ac005_inherit_is_off_by_default_and_flagged_when_asked() {
    let s = schema();
    let o = opts(&s, None);
    let default = generate(&view(), &o, false).expect("generate");
    assert_eq!(default.stats().join_conditions(), 1, "默认只生成 true 的那一条");
    assert!(!default.doc().contains("ontok:joinState \"inherit\""));

    let with = generate(&view(), &o, true).expect("generate");
    assert_eq!(with.stats().join_conditions(), 2);
    assert_eq!(with.stats().inherited_emitted, 1);
    assert!(with.doc().contains("ontok:joinState \"inherit\""));
    assert!(with
        .doc()
        .contains("ontok:joinEvidence \"inherited from license_quota.license_id (verified 5/5) -> customers.id via primary_key (plural)\""));
    // 默认那次必须**把它留在文件尾**（不是悄悄消失）
    assert!(default.doc().contains("# · orders → customers（child orders.license_id）"));
}

#[test]
fn ac006_false_edges_never_produce_a_join_in_either_mode() {
    for include_inherit in [false, true] {
        let doc = gen_doc(include_inherit).expect("generate");
        assert!(
            !doc.doc().contains("rr:child \"amount\""),
            "false 边生成了 join（include_inherit={include_inherit}）"
        );
        assert!(
            doc.doc().contains(
                "# · orders → customers（child orders.amount）：false（仅名字推断，未验）：insufficient_sample: 有效样本 1 行 < 3 条下限"
            ),
            "false 边必须连同原因写进文件尾：{}",
            doc.doc()
        );
        assert_eq!(doc.stats().naming_only_edges, 1);
        assert_eq!(doc.stats().unverified_edges, 0, "有证据的 false 不是「未标」");
    }
}

/// AC-006 的**另一半**：旧版布尔 `false`（没有 `joinEvidence`）是「未标」，
/// ⛔ 不算 `false` —— 否则读成「验过但失败」。
///
/// 现实里这就是 `anspire_grouped v3`（REQ-119 快照）的 52 条边。
#[test]
fn ac006_legacy_boolean_false_without_evidence_is_reported_as_unlabeled() {
    let s = schema();
    let mut v = view();
    // 只改第三条（`orders_amount__customers`）的状态：布尔 false、无证据。
    v.link_types[2].join_verified = Some(JoinState::NamingOnly);
    v.link_types[2].join_evidence = None;

    let doc = generate(&v, &opts(&s, None), false).expect("generate");
    assert_eq!(doc.stats().naming_only_edges, 0, "{:?}", doc.stats());
    assert_eq!(doc.stats().unverified_edges, 1, "{:?}", doc.stats());
    assert!(
        doc.doc().contains("未标（旧版布尔 false 没有 joinEvidence"),
        "{}",
        doc.doc()
    );
    assert!(!doc.doc().contains("rr:child \"amount\""));
}

// ══════════════════════════════════════════════════════════════
// AC-007 · 租户列
// ══════════════════════════════════════════════════════════════

#[test]
fn ac007_tenant_column_lands_on_the_triples_map_only_after_signature() {
    let s = schema();
    let v = view();
    let unsigned = TenancyDecl::parse(r#"{"columns":["customer_id"],"reviewed_by":""}"#).unwrap();
    let doc = generate(&v, &opts(&s, Some(&unsigned)), false).expect("generate");
    assert!(!doc.doc().contains("ontok:tenantColumn"), "未签声明不许落标注");

    let signed =
        TenancyDecl::parse(r#"{"columns":["customer_id"],"reviewed_by":"安全负责人"}"#).unwrap();
    let doc = generate(&v, &opts(&s, Some(&signed)), false).expect("generate");
    assert!(doc.doc().contains("ontok:tenantColumn \"customer_id\""));
    assert!(doc.doc().contains("# 租户列：已标注（1 个类 / 1 个列）"), "{}", doc.doc());
}

// ══════════════════════════════════════════════════════════════
// AC-008 · 与 schema 对账（点名）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac008_schema_reconciliation_names_every_failure_and_emits_no_document() {
    // ① 表不存在
    let no_table = SchemaTypes::parse(
        r#"{"tables":[{"name":"orders","primary_key":"order_id","unique_columns":["order_id"],
            "columns":[{"name":"order_id","data_type":"bigint"},
                       {"name":"customer_id","data_type":"bigint"},
                       {"name":"amount","data_type":"numeric"}]}]}"#,
    )
    .unwrap();
    let err = generate(&view(), &opts(&no_table, None), false).unwrap_err();
    assert!(err.failures().iter().any(|m| m.contains("表 customers 不在 schema.json 里")), "{err}");

    // ② 列不存在（点名到 表.列）
    let no_column = SchemaTypes::parse(
        r#"{"tables":[
            {"name":"orders","primary_key":"order_id","unique_columns":["order_id"],
             "columns":[{"name":"order_id","data_type":"bigint"},
                        {"name":"customer_id","data_type":"bigint"}]},
            {"name":"customers","primary_key":"id","unique_columns":["id"],
             "columns":[{"name":"id","data_type":"bigint"},
                        {"name":"customer_no","data_type":"text"}]}]}"#,
    )
    .unwrap();
    let err = generate(&view(), &opts(&no_column, None), false).unwrap_err();
    assert!(err.to_string().contains("表 orders 缺列 amount"), "{err}");
    // 失败文案必须带「未写出任何文件」的确定性结论（半份映射比没有更危险）
    assert!(err.to_string().contains("未写出任何文件"), "{err}");
}

// ══════════════════════════════════════════════════════════════
// AC-008 · 对账的三条细粒度 guard（H-1：删掉任一条必须红）
//
// 上面 `ac008_*` 钉的是「表不在 schema / 数据类型列不存在」；这三条钉的是
// **join 的两个端点**：`rr:child` 在子表、`rr:parent` 在父表、证据表 == 关系终点。
// 三者共用一个 `Err`，所以只有各自的夹具能证明**每一条**都在起作用 ——
// 少了哪条，只是失败文案换一句，或者干脆静默生成一条连错表的 join。
// ══════════════════════════════════════════════════════════════

/// guard ①：`rr:child` 必须存在于**子表**。
///
/// 夹具：快照说这条 join 靠 `orders.ghost_child` 连接，但 schema 里 orders 没有这一列。
#[test]
fn ac008_a_child_column_absent_from_the_child_table_is_a_named_failure() {
    let s = schema();
    let mut v = view();
    v.link_types[0].column = Some("ghost_child".into());

    let err = generate(&v, &opts(&s, None), false).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("表 orders 缺子列 ghost_child"),
        "必须点名「表 X 缺子列 Y」：{text}"
    );
    assert!(
        text.contains("rr:child"),
        "失败文案必须点明是哪一条 join 条件（rr:child）：{text}"
    );
    assert!(
        err.failures().iter().any(|m| m.contains("orders_customer_id__customers")),
        "必须点名到关系 key：{text}"
    );
}

/// guard ②：`rr:parent` 必须存在于**父表**。
///
/// 夹具：证据说连 `customers.ghost_parent`（唯一列），但 customers 没有这一列。
/// ⚠️ 这条故意让「缺列」比「不唯一」先命中 —— 若删掉 guard ②，失败会退化成
/// 「不是主键也不是唯一列」，断言照样红：证明这条 guard 才是点名「缺父列」的那一条。
#[test]
fn ac008_a_parent_column_absent_from_the_parent_table_is_a_named_failure() {
    let s = schema();
    let mut v = view();
    v.link_types[0].join_evidence = Some(
        "value-overlap 5/5 (100.00%) -> customers.ghost_parent via unique_column (exact)".into(),
    );

    let err = generate(&v, &opts(&s, None), false).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("表 customers 缺父列 ghost_parent"),
        "必须点名「表 X 缺父列 Y」：{text}"
    );
    assert!(
        text.contains("rr:parent"),
        "失败文案必须点明是哪一条 join 条件（rr:parent）：{text}"
    );
    assert!(
        err.failures().iter().any(|m| m.contains("orders_customer_id__customers")),
        "必须点名到关系 key：{text}"
    );
}

/// guard ③：**证据表 == 关系终点表**。
///
/// 夹具：关系终点是 `customers`，证据却写 `-> licenses.id`。
/// 若删掉这条 guard，`has_column(customers, "id")` 与唯一性检查都会通过 ——
/// 于是一条「审核时看的是 licenses、生成时连的是 customers」的 join 会静默落盘。
#[test]
fn ac008_join_evidence_pointing_at_a_different_table_than_the_edge_target_is_a_named_failure() {
    let s = schema();
    let mut v = view();
    v.link_types[0].join_evidence =
        Some("value-overlap 5/5 (100.00%) -> licenses.id via primary_key (plural)".into());

    let err = generate(&v, &opts(&s, None), false).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("证据指向 licenses"),
        "必须点名证据指向的表：{text}"
    );
    assert!(
        text.contains("关系终点是 customers"),
        "必须点名关系实际的终点表：{text}"
    );
    assert!(
        err.failures().iter().any(|m| m.contains("orders_customer_id__customers")),
        "必须点名到关系 key：{text}"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-010 / AC-011
// ══════════════════════════════════════════════════════════════

#[test]
fn ac010_two_runs_are_byte_identical() {
    let s = schema();
    let o = opts(&s, None);
    let a = generate(&view(), &o, true).expect("generate");
    let b = generate(&view(), &o, true).expect("generate");
    assert_eq!(a.doc(), b.doc());
    // 结构统计也要一致（两份文档不可能「字节相同但统计不同」—— 这里显式钉住）
    assert_eq!(a.stats(), b.stats());
    // 生成器不许落时间：文件头里没有「生成于 / 时间戳」这类会每天变一个字节的行。
    assert!(!a.doc().contains("生成本文件"), "文件头不该有生成时间");
    assert!(!a.doc().contains("dcterms:created"), "⛔ 不许写创建时间（两次导出会差一个字节）");
}

#[test]
fn ac011_composite_primary_key_class_is_skipped_and_listed() {
    let mut v = view();
    v.entity_types.push(ModelEntityView {
        key: "audit_logs".into(),
        display_name: "审计日志".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    });
    let s = SchemaTypes::parse(
        r#"{"tables":[
            {"name":"orders","primary_key":"order_id","unique_columns":["order_id"],
             "columns":[{"name":"order_id","data_type":"bigint"},
                        {"name":"customer_id","data_type":"bigint"},
                        {"name":"license_id","data_type":"bigint"},
                        {"name":"amount","data_type":"numeric"}]},
            {"name":"customers","primary_key":"id","unique_columns":["id"],
             "columns":[{"name":"id","data_type":"bigint"},
                        {"name":"customer_no","data_type":"text"}]},
            {"name":"audit_logs","primary_key":null,"composite_primary_key":["tenant","subject_id"],
             "unique_columns":[],"columns":[{"name":"tenant","data_type":"text"},
                                            {"name":"subject_id","data_type":"bigint"}]}]}"#,
    )
    .unwrap();
    let doc = generate(&v, &opts(&s, None), false).expect("generate");
    assert_eq!(doc.stats().triples_maps, 2);
    assert_eq!(
        doc.stats().skipped_classes,
        vec![("audit_logs".to_string(), "复合主键（tenant、subject_id）".to_string())]
    );
    assert!(doc
        .doc()
        .contains("# ── 未生成清单：复合主键 / 无单列主键（R2RML 模板要拼多列，本轮不做）──"));
    assert!(doc.doc().contains("# · audit_logs —— 复合主键（tenant、subject_id）"));
}

// ══════════════════════════════════════════════════════════════
// 结构：统计与文档自洽 + 真实进程 `--help`
// ══════════════════════════════════════════════════════════════

#[test]
fn structural_counts_agree_with_the_document() {
    let doc = gen_doc(true).expect("generate");
    let st = doc.stats();
    assert_eq!(st.triples_maps, doc.doc().matches("a rr:TriplesMap").count());
    assert_eq!(st.join_maps, doc.doc().matches("rr:joinCondition [").count());
    assert_eq!(st.datatype_maps, doc.doc().matches("rr:datatype ").count());
    assert_eq!(st.join_maps, st.join_conditions(), "一条 join 恰好一个条件");
}

#[test]
fn cli_help_lists_the_generate_command_and_its_required_schema_flag() {
    let out = Command::new(env!("CARGO_BIN_EXE_ontok-builder"))
        .args(["r2rml", "generate", "--help"])
        .output()
        .expect("跑不起 ontok 二进制");
    assert!(out.status.success(), "`r2rml generate --help` 必须成功：{:?}", out);
    let text = String::from_utf8_lossy(&out.stdout);
    for needle in ["--project", "--version", "--schema", "--out", "--include-inherit"] {
        assert!(text.contains(needle), "`--help` 里缺 {needle}：\n{text}");
    }
    assert!(
        text.contains("必填") || text.contains("required"),
        "`--schema` 是必填项，help 里必须说清：\n{text}"
    );
}
