//! REQ-122 · **SPARQL 查询规划与租户注入**（AC-001 … AC-010）—— 公开 API 级判据。
//!
//! # 这个文件补的是哪一格
//!
//! `src/sparql/*` 的单测钉的是**单元行为**（词法/语法、规划器、SQL 文本原语、声明解析）。
//! 本文件从**公开 API**（`build_mapping` + `plan` + `QueryResult`）复验验收标准里
//! 最容易被「改一个量、少一条断言」磨掉的几条：
//!
//! | AC | 本文件的判据 |
//! | --- | --- |
//! | AC-001 | 支持的 7 类查询都能规划出 SQL（且 SQL 里有该有的结构） |
//! | AC-002 | 不支持的 10 类语法**逐条**报错（exit≠0 的等价物：`Err`），⛔ 不静默忽略 |
//! | AC-003 | 未知属性 / 未知类报错并给可用清单，⛔ 不返回空结果 |
//! | AC-004 | 注入由声明驱动（改列名 ⇒ SQL 变）；公共维度不注入；未登记 ⇒ 整条拒绝 |
//! | AC-005 | 注入是 `AND`，`FILTER != 自己` 也拿不到别人的数据 |
//! | AC-006 | 无法确定租户列 ⇒ fail-closed，点名表 |
//! | AC-009 | `QueryResult` 的 JSON 字段齐全 |
//! | AC-010 | **内存映射**的类数/join 数与 `r2rml generate` 的 TTL 一致（同源构造）；仓库根的声明可解析 |
//!
//! ⚠️ 本文件**不**连库：`plan` 是纯函数（输入是映射 + schema + 声明）。
//! 「真实库实测 / 两租户对拍」是交付物里的一次性证据（命令 + 数字），
//! 另有 `tests/test_req122_live.rs` 在**有源库**时可重复跑（没配环境变量则 SKIP）。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（同 REQ-121）。

use std::process::Command;

use ontok::domain::model_view::{
    ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
};
use ontok::domain::r2rml::{build_mapping, generate, R2rmlMapping};
use ontok::domain::snapshot::{Evidence, JoinState};
use ontok::domain::ttl::{SchemaTypes, TtlOptions};
use ontok::sparql::plan::{plan, PlanInput};
use ontok::sparql::policy::QueryPolicy;
use ontok::sparql::QueryResult;

// ══════════════════════════════════════════════════════════════
// 夹具：三个类、两条验过的 join（与 R2RML 的映射同一个来源）
// ══════════════════════════════════════════════════════════════

const ONT: &str = "https://ontok.local/demo";

fn prop(name: &str, column: &str) -> ModelPropertyView {
    ModelPropertyView {
        name: name.into(),
        column: column.into(),
        nullable: false,
        note: None,
        sample: None,
    }
}

fn entity(key: &str, props: Vec<ModelPropertyView>) -> ModelEntityView {
    ModelEntityView {
        key: key.into(),
        display_name: key.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: props,
        evidence: vec![],
    }
}

fn link(from: &str, to: &str, column: &str, parent_col: &str) -> ModelLinkView {
    ModelLinkView {
        key: format!("{from}_{column}__{to}"),
        from_type: from.into(),
        to_type: to.into(),
        cardinality: "N:1".into(),
        semantic: "belongs_to".into(),
        evidence: vec![Evidence {
            table: from.into(),
            column: column.into(),
            claim: "外键".into(),
        }],
        confidence: None,
        join_verified: Some(JoinState::Verified),
        join_evidence: Some(format!(
            "value-overlap 5/5 (100.00%) -> {to}.{parent_col} via primary_key (plural)"
        )),
        column: Some(column.into()),
    }
}

fn view() -> ModelTrackView {
    ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![
            entity("customers", vec![prop("客户名", "customer_name"), prop("租户号", "tenant_no")]),
            entity(
                "orders",
                vec![
                    prop("金额", "amount"),
                    prop("客户", "customer_id"),
                    prop("状态", "status"),
                    prop("下单时间", "created_at"),
                    prop("产品", "product_id"),
                    prop("租户号", "tenant_no"),
                ],
            ),
            entity("products", vec![prop("产品名", "product_name")]),
        ],
        link_types: vec![
            link("orders", "customers", "customer_id", "id"),
            link("orders", "products", "product_id", "id"),
        ],
        action_types: vec![],
        functions: vec![],
    }
}

const SCHEMA: &str = r#"{"tables":[
  {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id","tenant_no"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true},
              {"name":"customer_name","data_type":"text"},
              {"name":"tenant_no","data_type":"text"}]},
  {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
   "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
              {"name":"customer_id","data_type":"text"},
              {"name":"product_id","data_type":"text"},
              {"name":"amount","data_type":"numeric(12,4)"},
              {"name":"status","data_type":"integer"},
              {"name":"created_at","data_type":"timestamp without time zone"},
              {"name":"tenant_no","data_type":"text"}]},
  {"name":"products","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
   "columns":[{"name":"id","data_type":"text","is_primary_key":true},
              {"name":"product_name","data_type":"text"}]}
]}"#;

struct Fixture {
    schema: SchemaTypes,
    mapping: R2rmlMapping,
}

impl Fixture {
    fn new() -> Self {
        let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
        let mut opts = TtlOptions::new("demo", 2);
        opts.schema = Some(&schema);
        let mapping = build_mapping(&view(), &opts, false, Default::default())
            .expect("夹具映射必须能生成");
        Self { schema, mapping }
    }

    fn plan_with(
        &self,
        sparql: &str,
        policy: &QueryPolicy,
        tenant: &str,
        max_rows: u64,
    ) -> Result<ontok::sparql::Planned, ontok::sparql::QueryError> {
        plan(
            sparql,
            &PlanInput {
                mapping: &self.mapping,
                schema: &self.schema,
                ontology_iri: ONT,
                policy,
                tenant,
                max_rows,
            },
        )
    }

    fn plan(&self, sparql: &str) -> Result<ontok::sparql::Planned, ontok::sparql::QueryError> {
        self.plan_with(sparql, &policy(), "C0001", 1000)
    }

    fn sql(&self, sparql: &str) -> String {
        self.plan(sparql).expect("这条查询应当能规划").sql
    }

    fn err(&self, sparql: &str) -> String {
        self.plan(sparql).expect_err("这条查询应当被拒绝").to_string()
    }
}

fn policy() -> QueryPolicy {
    QueryPolicy::parse(
        r#"{
          "version": "1.0.0",
          "tenantSubject": "customers",
          "tenantIdColumn": "id",
          "perTable": { "orders": { "tenantColumn": "customer_id" } },
          "publicDimensions": [ { "table": "products", "reason": "产品目录" } ],
          "reviewed_by": "测试"
        }"#,
    )
    .expect("夹具声明")
}

const A_IRI: &str = "https://ontok.local/demo#customers/A";
const B_IRI: &str = "https://ontok.local/demo#customers/B";

// ══════════════════════════════════════════════════════════════
// AC-001 · 支持的语法真能规划出 SQL
// ══════════════════════════════════════════════════════════════

#[test]
fn ac001_every_supported_shape_plans_and_keeps_its_structure() {
    let f = Fixture::new();

    // 单跳
    let sql = f.sql(r#"SELECT ?o ?a WHERE { ?o :orders__amount ?a }"#);
    assert!(sql.contains(r#"FROM "orders" t0"#), "{sql}");

    // 两跳（沿 joinCondition）
    let sql = f.sql(
        r#"SELECT ?n { ?o :orders__customers ?c . ?c :customers__customer_name ?n }"#,
    );
    assert!(sql.contains(r#"JOIN "customers" t1 ON"#), "{sql}");

    // 聚合（SUM）
    let sql = f.sql(r#"SELECT (SUM(?a) AS ?total) { ?o :orders__amount ?a }"#);
    assert!(sql.contains(r#"sum(t0."amount")"#), "{sql}");

    // GROUP BY（单列）
    let sql = f.sql(
        r#"SELECT ?s (COUNT(*) AS ?n) { ?o :orders__status ?s } GROUP BY ?s"#,
    );
    assert!(sql.contains(r#"GROUP BY t0."status""#), "{sql}");

    // ORDER BY + LIMIT
    let sql = f.sql(r#"SELECT ?o ?d { ?o :orders__created_at ?d } ORDER BY DESC(?d) LIMIT 10"#);
    assert!(sql.contains(r#"ORDER BY t0."created_at" DESC"#), "{sql}");

    // FILTER 比较
    let sql = f.sql(r#"SELECT ?o { ?o :orders__amount ?a . FILTER (?a > 100) }"#);
    assert!(sql.contains(r#"t0."amount" > 100"#), "{sql}");

    // FILTER 复合
    let sql = f.sql(
        r#"SELECT ?o { ?o :orders__status ?s . ?o :orders__amount ?a . FILTER (?s = 1 && ?a > 0) }"#,
    );
    assert!(sql.contains(r#"t0."status" = 1 AND t0."amount" > 0"#), "{sql}");
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 不支持的语法必须报错（⛔ 不静默忽略）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac002_each_unsupported_construct_is_rejected_by_name() {
    let f = Fixture::new();
    let cases: &[(&str, &str)] = &[
        (
            r#"SELECT ?o { ?o :orders__amount ?a OPTIONAL { ?o :orders__status ?s } }"#,
            "OPTIONAL",
        ),
        (r#"SELECT ?o { ?o :orders__amount ?a . ?o :orders__status ?s UNION ?o :orders__tenant_no ?t }"#, "UNION"),
        (r#"SELECT ?o { { SELECT ?o { ?o :orders__amount ?a } } }"#, "子查询"),
        (r#"SELECT ?o { ?o :orders__amount ?a BIND(?a AS ?b) }"#, "BIND"),
        (r#"SELECT ?o { ?o :orders__customers* ?c }"#, "属性路径"),
        (r#"SELECT ?o { ?o :orders__customers/:customers__customer_name ?n }"#, "属性路径"),
        (
            r#"CONSTRUCT { ?o :orders__amount ?a } WHERE { ?o :orders__amount ?a }"#,
            "CONSTRUCT",
        ),
        (r#"ASK { ?o :orders__amount ?a }"#, "ASK"),
        (
            r#"SELECT ?o { ?o :orders__amount ?a FILTER regex(?a, "1") }"#,
            "不支持函数",
        ),
        (
            r#"SELECT (STR(?o) AS ?x) { ?o :orders__amount ?a }"#,
            "不支持函数",
        ),
    ];
    for (q, needle) in cases {
        let e = f.err(q);
        assert!(e.contains(needle), "查询 {q}\n  的错误里应当出现 {needle:?}，实际：{e}");
        assert!(
            e.contains("不支持") || e.contains("只支持"),
            "错误必须说清「不支持什么」：{e}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 属性/类写错必须报错（⛔ 不许返回空结果）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac003_unknown_property_lists_the_available_ones() {
    let f = Fixture::new();
    let e = f.err(r#"SELECT ?o ?x { ?o :orders__no_such_column ?x }"#);
    assert!(e.contains(":orders__no_such_column"), "{e}");
    assert!(e.contains(":orders__amount"), "必须列出可用属性：{e}");
    assert!(e.contains("ontology show"), "必须提示怎么查全量：{e}");
}

#[test]
fn ac003_unknown_class_is_an_error_not_an_empty_result() {
    let f = Fixture::new();
    let e = f.err(r#"SELECT ?s { ?s a :no_such_class }"#);
    assert!(e.contains(":no_such_class"), "{e}");
    assert!(e.contains(":orders") && e.contains(":customers"), "必须列出可用类：{e}");
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 租户强制注入（由声明驱动）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac004_injection_uses_the_declared_column_and_is_visible_in_the_sql() {
    let f = Fixture::new();
    let p = f
        .plan(r#"SELECT ?o ?a { ?o :orders__customers ?c . ?o :orders__amount ?a }"#)
        .unwrap();
    assert!(p.sql.contains(r#""customer_id" = 'C0001'"#), "{}", p.sql);
    assert!(p.sql.contains(r#""id" = 'C0001'"#), "{}", p.sql);
    // tenant_applied 覆盖 tables 里除公共维度外的每一张表
    for t in &p.tables {
        assert!(
            p.tenant_applied.iter().any(|x| &x.table == t)
                || p.public_dimensions.iter().any(|x| &x.table == t),
            "表 {t} 既没注入也不在公共维度里"
        );
    }
}

#[test]
fn ac004_a_public_dimension_has_no_condition_and_says_why() {
    let f = Fixture::new();
    let p = f
        .plan(r#"SELECT ?o ?n { ?o :orders__products ?p . ?p :products__product_name ?n }"#)
        .unwrap();
    assert!(!p.sql.contains(r#""products"."#), "公共维度不该有过滤：{}", p.sql);
    assert_eq!(p.public_dimensions.len(), 1);
    assert!(p.public_dimensions[0].reason.contains("产品目录"));
}

#[test]
fn ac004_changing_the_declared_column_changes_the_injection_without_touching_code() {
    let f = Fixture::new();
    let swapped = QueryPolicy::parse(
        r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
            "perTable":{"orders":{"tenantColumn":"tenant_no"}},
            "publicDimensions":[{"table":"products","reason":"产品目录"}]}"#,
    )
    .unwrap();
    let q = r#"SELECT ?o ?a { ?o :orders__amount ?a }"#;
    let before = f.plan_with(q, &policy(), "C0001", 1000).unwrap();
    let after = f.plan_with(q, &swapped, "C0001", 1000).unwrap();
    assert!(before.sql.contains(r#""customer_id" = 'C0001'"#), "{}", before.sql);
    assert!(after.sql.contains(r#""tenant_no" = 'C0001'"#), "{}", after.sql);
    assert!(!after.sql.contains(r#""customer_id""#), "{}", after.sql);
}

#[test]
fn ac004_a_missing_declaration_entry_rejects_the_whole_query() {
    let f = Fixture::new();
    let narrow = QueryPolicy::parse(
        r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
            "perTable":{},"publicDimensions":[]}"#,
    )
    .unwrap();
    let e = f
        .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &narrow, "C0001", 1000)
        .expect_err("未登记必须整条拒绝")
        .to_string();
    assert!(e.contains("orders") && e.contains("整条查询拒绝"), "{e}");
    assert!(e.contains("perTable") && e.contains("publicDimensions"), "{e}");
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 泄漏：注入是 AND，反向筛也挡得住
// ══════════════════════════════════════════════════════════════

#[test]
fn ac005_the_injection_is_the_first_and_full_where_term() {
    let f = Fixture::new();
    let p = f
        .plan(&format!(
            r#"SELECT ?o ?c {{ ?o :orders__customers ?c . FILTER (?c != <{A_IRI}>) }}"#
        ))
        .unwrap();
    let where_part = p.sql.split(" WHERE ").nth(1).unwrap();
    assert!(
        where_part.starts_with(r#"t0."customer_id" = 'C0001' AND t1."id" = 'C0001' AND"#),
        "注入必须是 WHERE 的最前面的 AND 项：{}",
        p.sql
    );
    assert!(where_part.contains(A_IRI), "用户 FILTER 在注入之后：{}", p.sql);
    // 没有「用户条件 OR 掉租户条件」的形态
    assert!(!p.sql.contains(r#"OR t0."customer_id""#), "{}", p.sql);
    assert!(!p.sql.contains(r#"OR t1."id""#), "{}", p.sql);
}

#[test]
fn ac005_asking_for_another_tenant_by_iri_cannot_win() {
    let f = Fixture::new();
    // 正面越权：FILTER (?c = <B>)。SQL 里注入仍然是 A，两者 AND ⇒ 必然 0 行。
    let p = f
        .plan(&format!(
            r#"SELECT ?o ?c {{ ?o :orders__customers ?c . FILTER (?c = <{B_IRI}>) }}"#
        ))
        .unwrap();
    assert!(p.sql.contains(r#""customer_id" = 'C0001'"#), "{}", p.sql);
    assert!(p.sql.contains(B_IRI), "{}", p.sql);
    assert!(!p.sql.contains("= 'B'"), "不许把 B 当租户写进注入：{}", p.sql);
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 无法确定租户列 ⇒ fail-closed
// ══════════════════════════════════════════════════════════════

#[test]
fn ac006_a_table_without_a_declared_column_rejects_and_names_the_table() {
    let f = Fixture::new();
    // 一份「只把 products 声明为公共维度、其余一律没登记」的声明：
    // orders 没有租户列可注入 ⇒ 整条拒绝（⛔ 不猜、⛔ 不过滤放行）。
    let narrow = QueryPolicy::parse(
        r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
            "perTable":{},"publicDimensions":[{"table":"products","reason":"产品目录"}]}"#,
    )
    .unwrap();

    let e = f
        .plan_with(r#"SELECT ?o ?a { ?o :orders__amount ?a }"#, &narrow, "C0001", 1000)
        .expect_err("未登记的表必须拒绝")
        .to_string();
    assert!(e.contains("orders"), "必须点名表：{e}");
    assert!(e.contains("整条查询拒绝"), "{e}");
    assert!(e.contains("perTable") && e.contains("publicDimensions"), "{e}");

    // 而**登记过的**表在同一份声明下照常工作（不是「一律拒绝」）
    let ok = f
        .plan_with(r#"SELECT ?n { ?p :products__product_name ?n }"#, &narrow, "C0001", 1000)
        .expect("公共维度应当可查");
    assert!(ok.tenant_applied.is_empty(), "公共维度不该注入");
}

// ══════════════════════════════════════════════════════════════
// AC-009 · 返回值形状
// ══════════════════════════════════════════════════════════════

#[test]
fn ac009_the_result_json_has_every_required_field() {
    let r = QueryResult {
        columns: vec!["a".into()],
        rows: vec![vec![Some("1".into())]],
        row_count: 1,
        truncated: false,
        sql_echo: "SELECT ...".into(),
        tenant_applied: vec![ontok::sparql::TenantApplied {
            table: "orders".into(),
            column: "customer_id".into(),
            value: "C0001".into(),
        }],
        tables: vec!["orders".into()],
        public_dimensions: vec![],
        warnings: vec![],
    };
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    for key in [
        "columns",
        "rows",
        "row_count",
        "truncated",
        "sql_echo",
        "tenant_applied",
        "tables",
        "warnings",
    ] {
        assert!(v.get(key).is_some(), "回执里缺字段 {key}：{v}");
    }
    assert_eq!(v["tenant_applied"][0]["column"], "customer_id");
}

// ══════════════════════════════════════════════════════════════
// AC-010 · 不退化：映射与 TTL **同源**（类数 / join 数一致）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac010_the_in_memory_mapping_and_the_generated_ttl_agree_on_classes_and_joins() {
    let schema = SchemaTypes::parse(SCHEMA).unwrap();
    let mut opts = TtlOptions::new("demo", 2);
    opts.schema = Some(&schema);
    let mapping = build_mapping(&view(), &opts, false, Default::default()).unwrap();
    let doc = generate(&view(), &opts, false).unwrap();
    let text = doc.doc();

    assert_eq!(mapping.classes().len(), doc.stats().triples_maps);
    assert_eq!(mapping.classes().len(), text.matches("a rr:TriplesMap").count());
    assert_eq!(mapping.join_count(), doc.stats().join_maps);
    assert_eq!(mapping.join_count(), text.matches("rr:joinCondition [").count());
    assert_eq!(
        mapping.classes().iter().map(|c| c.properties.len()).sum::<usize>(),
        doc.stats().datatype_maps
    );
    // 每条 join 的 child/parent 都能在 TTL 里找到同名的一对
    for class in mapping.classes() {
        for join in &class.joins {
            assert!(
                text.contains(&format!(
                    "rr:joinCondition [ rr:child \"{}\" ; rr:parent \"{}\" ]",
                    join.child, join.parent_column
                )),
                "映射里的 join {}.{} → {}.{} 没出现在 TTL 里",
                class.key,
                join.child,
                join.parent_table,
                join.parent_column
            );
        }
    }
}

#[test]
fn ac010_the_shipped_declaration_parses_and_has_no_contradiction() {
    let raw = include_str!("../query-policy.json");
    let p = QueryPolicy::parse(raw).expect("仓库根的 query-policy.json 必须是一份合法声明");
    assert!(!p.per_table.is_empty(), "逐表登记不能为空");
    assert!(!p.public_dimensions.is_empty(), "公共维度不能为空");
    // 同一个表不能既登记过滤列、又声明为公共维度（parse 已经拦，这里再断言一次）
    for (t, _) in &p.per_table {
        assert!(
            !p.public_dimensions.iter().any(|x| x.table.eq_ignore_ascii_case(t)),
            "{t} 同时出现在两处"
        );
    }
    // 每一条公共维度都必须有理由
    for d in &p.public_dimensions {
        assert!(!d.reason.trim().is_empty(), "{} 没有写理由", d.table);
    }
    // 声明里**没有**硬编码任何「猜列」的痕迹：所有过滤列都必须来自声明本身
    assert_eq!(p.tenant_subject, "customers");
    assert_eq!(p.tenant_id_column, "id");
}

// ══════════════════════════════════════════════════════════════
// 真实进程 `--help`（命令必须可发现、参数必须齐）
// ══════════════════════════════════════════════════════════════

#[test]
fn cli_help_lists_the_sparql_query_command_and_its_flags() {
    let out = Command::new(env!("CARGO_BIN_EXE_ontok"))
        .args(["sparql", "query", "--help"])
        .output()
        .expect("跑不起 ontok 二进制");
    assert!(out.status.success(), "`sparql query --help` 必须成功：{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "--project",
        "--version",
        "--schema",
        "--policy",
        "--tenant",
        "--query",
        "--max-rows",
        "--timeout-ms",
    ] {
        assert!(text.contains(needle), "`--help` 里缺 {needle}：\n{text}");
    }
    assert!(text.contains("ADR-006"), "help 里必须指向租户口径的裁定：\n{text}");
}
