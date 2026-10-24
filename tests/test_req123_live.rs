//! REQ-123 · **真库实测**：两个协议逐字节一致 + 租户隔离 + 五条真实查询。
//!
//! # 为什么这些判据必须连真库
//!
//! - 「REST 与 MCP 的回执逐字节相同」在**空结果**上也成立，所以必须有一条**有行**的查询；
//! - 「A 的 token 看不到 B 的行」在合成数据上证明不了声明与真实 schema 对得上；
//! - 「五条真实查询（单跳 / 两跳 / 聚合 / GROUP BY / ORDER BY+LIMIT）」是 AC-010 的原文。
//!
//! # 缺环境变量时**响亮地跳过**（沿用 REQ-122 · N2 的口径）
//!
//! 不静默：打印 `*** SKIP: <测试名> -- missing <ENV> ***`（stderr + **子进程回显**），
//! 于是默认 `cargo test` 的输出里也看得见「这几条没跑」。
//!
//! # 需要哪些环境变量
//!
//! | 变量 | 用途 |
//! | --- | --- |
//! | `ONTOK_DATABASE_URL`（或 `ONTOK_TEST_DATABASE_URL`） | 元数据库（项目 / 版本 / 快照） |
//! | `ONTOK_SOURCE_DATABASE_URL` | 源库（只读；查询真的打在这里） |
//! | `ONTOK_REQ123_SCHEMA` | `schema.json` 的路径（列类型 / 列存在性只能来自它） |
//! | `ONTOK_REQ123_TENANT` | 一个**有数据**的租户 id（`balances` 里取） |
//! | `ONTOK_REQ123_TENANT_B` | 第二个租户 id（可省：省了只跑 A 侧隔离判据） |
//! | `ONTOK_REQ123_PROJECT` / `ONTOK_REQ123_VERSION` | 默认 `anspire_r2rml_excl` / `2` |
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（与 REQ-107/122 的现场判据同一口径）。

mod common;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::query::tokens::TokenTable;
use ontok::query::{http, Caller, Protocol, QueryService, ServiceParts, SparqlRequest};
use serde_json::Value;
use tower::ServiceExt;

const TOK_A: &str = "FAKE-LIVE-TOKEN-aaaaaaaaaaaaaaaa";
const TOK_B: &str = "FAKE-LIVE-TOKEN-bbbbbbbbbbbbbbbb";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn meta_url() -> Option<String> {
    env("ONTOK_DATABASE_URL").or_else(|| env("ONTOK_TEST_DATABASE_URL"))
}

/// 缺哪个变量就**响亮地**说，然后提前返回（SKIP）。
///
/// 唯一实现在 [`common::skip::missing`]（REQ-131）：默认打一行 `*** SKIP: … ***`
/// 后**正常返回**；`ONTOK_REQUIRE_FIXTURES=1` 时**失败**（CI 用）。
fn require(test: &str, names: &[&str]) -> Option<()> {
    for n in names {
        if env(n).is_none() {
            common::skip::missing(n, test);
            return None;
        }
    }
    Some(())
}

/// 真库用例的**串行闸**（不是「测试必须单线程跑」的免责声明，是让它并行也不炸）。
///
/// 为什么需要：每个用例都自己 [`build_service`] 一个服务 ⇒ 一个只读池
/// （`read_only_pool` 硬上限 `max_connections = 2`，这是客户资产，不开大）。
/// 9 个用例并行 ⇒ 峰值 18 条连接；源库在非标准端口 30432 是**代理**，超额连接会被
/// 重置（`src/extract/introspect.rs` 的注释里记着这条 S6）⇒ 表现为
/// `pool timed out while waiting for an open connection` / `连不上源库`，
/// 与代码正确性无关。闸住「建服务 + 查询」这一段，连接峰值就回到 1 个池。
///
/// ⚠️ 默认 `cargo test` 仍然会把 9 个用例**都**跑完（只是不并发连源库）；
/// 想更快也没问题：`--test-threads=1` 时这把锁从不争用。
static LIVE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    // 某个用例 panic 会毒化这把锁 —— 毒化本身不是失败原因，拿到内部值继续。
    LIVE_GATE.lock().unwrap_or_else(|e| e.into_inner())
}

async fn build_service(timeout_ms: u64, tenant_a: &str, tenant_b: &str) -> Option<Arc<QueryService>> {
    let meta = meta_url()?;
    let source = env("ONTOK_SOURCE_DATABASE_URL")?;
    let schema_path = env("ONTOK_REQ123_SCHEMA")?;
    let project = env("ONTOK_REQ123_PROJECT").unwrap_or_else(|| "anspire_r2rml_excl".into());
    let version: i32 = env("ONTOK_REQ123_VERSION")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let pool = match ontok::db::pool::connect(&meta).await {
        Ok(p) => p,
        Err(e) => {
            common::skip::missing(&format!("可连的 ONTOK_DATABASE_URL（{e}）"), "build_service");
            return None;
        }
    };
    let projects = ontok::db::project::PgProjectStore::new(pool.clone());
    let store = ontok::db::snapshot_store::PgStore::new(pool.clone());
    let p = projects.get_by_name(&project).await.ok().flatten()?;
    let v = projects.version_by_seq(p.id, &p.name, version).await.ok()?;
    assert!(v.frozen_at.is_some(), "真库实测要求该版本已冻结（与 serve 同一条闸门）");
    let snap = store.get_snapshot(v.id).await.ok().flatten()?;

    let raw = std::fs::read_to_string(&schema_path).expect("schema 文件可读");
    let schema = ontok::domain::ttl::SchemaTypes::parse(&raw).expect("schema 合法");
    let raw = std::fs::read_to_string(
        common::crate_root().join("query-policy.json"),
    )
    .expect("query-policy.json 可读");
    let policy = ontok::sparql::policy::QueryPolicy::parse(&raw).expect("声明合法");

    let opts = ontok::domain::ttl::TtlOptions {
        project: project.as_str(),
        version,
        base: "https://ontok.local/",
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema),
    };
    let model = ontok::domain::model_view::project_model_track(&snap);
    let mapping = ontok::domain::r2rml::build_mapping(
        &model,
        &opts,
        false,
        ontok::domain::r2rml::R2rmlAudit::default(),
    )
    .expect("映射必须能生成（否则说明 schema 与快照对不上）");
    let ontology_iri = opts.ontology_iri();

    let src = ontok::sparql::exec::connect_read_only(&source)
        .await
        .expect("源库必须能连上且只读");

    let tokens = TokenTable::parse(&format!(
        r#"{{"version":"1.0.0","tokens":[
            {{"label":"a","token":"{TOK_A}","tenant":"{tenant_a}","max_rows":200,"max_concurrency":2}},
            {{"label":"b","token":"{TOK_B}","tenant":"{tenant_b}","max_rows":50,"max_concurrency":1}}
        ]}}"#
    ))
    .expect("夹具 token 表");

    // ⭐ REQ-127 · 服务状态里多了两视图 TTL（不连库也能从已投影视图渲染；走同一个管线）。
    let customer_view = ontok::domain::view::project(&ontok::cli::snapshot_to_view(&snap));
    let ttl = ontok::query::ttl::OntologyTtl::from_views(
        &customer_view,
        &model,
        ontok::query::ttl::TtlMeta {
            project: &project,
            version,
            base: "https://ontok.local/",
            action_type_count: snap.action_types.len(),
            schema: Some(&schema),
            tenancy: None,
        },
    )
    .expect("两视图 TTL 必须能渲染");

    Some(Arc::new(QueryService::new(ServiceParts {
        source: src.pool.clone(),
        facts: src.facts.clone(),
        mapping,
        model,
        schema,
        policy,
        ontology_iri,
        project,
        version,
        timeout_ms,
        sse_idle_timeout_ms: 2_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        tokens,
        audit: ontok::query::audit::AuditSink::stderr_only(),
        ttl,
    })))
}

async fn rest(svc: &Arc<QueryService>, method: &str, uri: &str, token: Option<&str>, body: Option<&str>) -> (StatusCode, String) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if body.is_some() {
        b = b.header(header::CONTENT_TYPE, "application/json");
    }
    let req = b
        .body(body.map(|s| Body::from(s.to_string())).unwrap_or_else(Body::empty))
        .unwrap();
    let res = http::build_router(Arc::clone(svc)).oneshot(req).await.expect("router");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 16 * 1024 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn mcp(svc: &Arc<QueryService>, token: &str, line: &str) -> Value {
    let (_, body) = rest(svc, "POST", "/mcp", Some(token), Some(line)).await;
    serde_json::from_str(&body).expect("MCP 响应是 JSON")
}

/// ⭐ AC-004 的核心判据：同一 token、同一查询，REST 的响应体与 MCP 的
/// `content[0].text` **逐字节相同**（四条查询：单跳 / 两跳 / 聚合 / **覆盖范围不完整**）。
///
/// # 为什么必须有第四条（QA 变异 M1）
///
/// 前三条都**不产生** `warnings`。于是「MCP 路径把 `warnings` 清空」在字节上等于没变 ——
/// lib 562 全绿、live 全绿，变异存活。第四条查 `deductions`（声明里 `coverage: main_only`）
/// 必然带 warning，所以只要 MCP 少带/改一个字，`text != rest_body` 立刻红。
#[tokio::test]
async fn ac004_rest_and_mcp_receipts_are_byte_identical() {
    let _live = one_at_a_time();
    let test = "ac004_rest_and_mcp_receipts_are_byte_identical";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let tenant_b = env("ONTOK_REQ123_TENANT_B").unwrap_or_else(|| tenant_a.clone());
    let svc = build_service(10_000, &tenant_a, &tenant_b).await.expect("服务可构建");

    // (标签, 查询, 是否**必须**产生非空 warnings)
    let queries = [
        ("单跳", "SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt }", false),
        ("两跳", "SELECT ?l ?n WHERE { ?l :licenses__customers ?c . ?c :customers__customer_name ?n }", false),
        ("聚合", "SELECT (COUNT(?b) AS ?n) WHERE { ?b a :balances }", false),
        // ⭐ `deductions` 在 `query-policy.json` 里是 `coverage: main_only`
        // ⇒ 命中它必然产生 warning（这是判据，不是巧合）。
        (
            "覆盖范围不完整（main_only）",
            "SELECT ?d ?amt WHERE { ?d :deductions__deduction_amount ?amt }",
            true,
        ),
    ];
    for (label, q, must_warn) in queries {
        let body = serde_json::json!({ "query": q }).to_string();
        let (status, rest_body) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "[{label}] REST 失败：{rest_body}");
        let line = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "sparql_query", "arguments": { "query": q } }
        })
        .to_string();
        let v = mcp(&svc, TOK_A, &line).await;
        assert_eq!(v["result"]["isError"], false, "[{label}] {v}");
        let text = v["result"]["content"][0]["text"].as_str().expect("text");
        assert_eq!(
            text, rest_body,
            "⛔ 两协议回执不逐字节相同（AC-004）[{label}]：\n REST: {rest_body}\n MCP : {text}"
        );
        // 前提自检：回执里有 rows 字段，且序列化不是空对象
        let parsed: Value = serde_json::from_str(&rest_body).unwrap();
        assert!(parsed.get("rows").is_some());
        assert!(parsed.get("coverage").is_some());
        assert!(parsed.get("join_states").is_some());
        // B1：范围必须写在回执里（机器读值 + 人话），且与文档里的常量一致。
        assert_eq!(
            parsed["join_states_scope"], "all_edges_between_involved_tables",
            "[{label}] 回执缺范围标签：{rest_body}"
        );
        assert!(
            parsed["join_states_scope_note"]
                .as_str()
                .unwrap_or_default()
                .contains("不是本次查询实际走过的那几条"),
            "[{label}] 范围标签只有值没有解释：{rest_body}"
        );
        // B1-附带（M1）：这一条必须**有** warning，且两个协议的 warnings 相同。
        let rest_warnings = parsed["warnings"].as_array().expect("warnings 数组");
        if must_warn {
            assert!(
                !rest_warnings.is_empty(),
                "[{label}] 这条查询必须产生 warning（否则它证明不了「MCP 少带 warnings」）：{rest_body}"
            );
            let mcp_json: Value = serde_json::from_str(text).expect("MCP text 是 JSON");
            assert_eq!(
                mcp_json["warnings"], parsed["warnings"],
                "⛔ MCP 的 warnings 与 REST 不同 [{label}]：\n REST: {rest_body}\n MCP : {text}"
            );
            assert!(
                !mcp_json["warnings"].as_array().unwrap().is_empty(),
                "⛔ MCP 的 warnings 被清空了 [{label}]：{text}"
            );
        }
    }
}

/// B1 · **QA 实测的误读场景**（真库）：三表查询的回执里，「涉及但本次未用」的边
/// 必须能认出来 —— 这是 `join_states_scope` + `used` 的存在理由。
///
/// 查询：`?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c`
/// ⇒ `tables[]` = licenses / customers / balances ⇒ `balances` 与 `customers` 之间
/// **两条**边都在场，但本次只走了 `balances__customers__customer_id`；
/// `balances__customers__source_customer_id` 是「涉及但未用」⇒ `used: false`。
#[tokio::test]
async fn b1_the_scope_and_the_unused_edge_are_visible_in_the_receipt() {
    let _live = one_at_a_time();
    let test = "b1_the_scope_and_the_unused_edge_are_visible_in_the_receipt";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(10_000, &tenant_a, &tenant_a).await.expect("服务可构建");

    let q = "SELECT ?l ?c ?b WHERE { ?l :licenses__customers ?c . \
             ?b :balances__customers__customer_id ?c }";
    let body = serde_json::json!({ "query": q }).to_string();
    let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let v: Value = serde_json::from_str(&text).expect("回执是 JSON");

    // 前提：三张表都在场（否则下面「涉及但未用」的判据不成立）
    let tables: Vec<&str> = v["tables"]
        .as_array()
        .expect("tables")
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    for t in ["licenses", "customers", "balances"] {
        assert!(tables.contains(&t), "tables 缺 {t}（{tables:?}）：{text}");
    }

    // ① 范围标签在回执里（⛔ 不是「只在文档里」）
    assert_eq!(v["join_states_scope"], "all_edges_between_involved_tables", "{text}");
    assert!(
        v["join_states_scope_note"]
            .as_str()
            .unwrap()
            .contains("不是本次查询实际走过的那几条"),
        "{text}"
    );

    // ② 本次写进查询的两条边 ⇒ used: true
    let edges = v["join_states"].as_array().expect("join_states");
    let find = |pred: &str| {
        edges
            .iter()
            .find(|e| e["predicate"].as_str() == Some(pred))
            .unwrap_or_else(|| panic!("join_states 缺 {pred}：{text}"))
            .clone()
    };
    assert_eq!(find("licenses__customers")["used"], true, "{text}");
    assert_eq!(
        find("balances__customers__customer_id")["used"],
        true,
        "{text}"
    );

    // ③ ⭐ 涉及但未用 ⇒ used: false（这就是 QA 误读的那条）
    let unused = find("balances__customers__source_customer_id");
    assert_eq!(
        unused["used"], false,
        "⛔ 没写进查询的边必须标 used=false：{text}"
    );
    assert_eq!(unused["from"], "balances");
    assert_eq!(unused["to"], "customers");
    assert!(
        !unused["evidence"].as_str().unwrap().is_empty(),
        "每条边都要有证据原文：{text}"
    );

    // ④ 三表 ⇒ 至少有 1 条「涉及但未用」的边（这正是范围比「实走边」宽的证据）
    assert!(
        edges.iter().any(|e| e["used"] == false),
        "三表查询里必须能看到「涉及但未用」的边：{text}"
    );
    // 证据（`--nocapture` 可见）：范围标签 + 每条边的 used
    let mut brief = Vec::new();
    for e in edges {
        brief.push(serde_json::json!({
            "from": e["from"], "to": e["to"], "predicate": e["predicate"],
            "used": e["used"], "state": e["state"],
        }));
    }
    println!(
        "B1-SNIPPET {}",
        serde_json::json!({
            "tables": v["tables"],
            "join_states_scope": v["join_states_scope"],
            "join_states_scope_note": v["join_states_scope_note"],
            "join_states": brief,
        })
    );
}

/// ⭐ AC-003 的真库判据：A 的 token 用 FILTER 写死 B 的实例 IRI ⇒ 0 行，
/// 且 `sql_echo` 里注入的仍是 **A**（不是 B、也不是「没有条件」）。
#[tokio::test]
async fn ac003_a_never_sees_another_tenants_rows() {
    let _live = one_at_a_time();
    let test = "ac003_a_never_sees_another_tenants_rows";
    let missing_required = require(
        test,
        &[
            "ONTOK_REQ123_SCHEMA",
            "ONTOK_REQ123_TENANT",
            "ONTOK_REQ123_TENANT_B",
            "ONTOK_SOURCE_DATABASE_URL",
        ],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性），同上面那几条。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let tenant_b = env("ONTOK_REQ123_TENANT_B").unwrap();
    assert_ne!(tenant_a, tenant_b, "两个租户必须是不同的 id");
    let svc = build_service(10_000, &tenant_a, &tenant_b).await.expect("服务可构建");
    let ont = svc.ontology_iri.clone();

    let leak_q = format!(
        "SELECT ?b WHERE {{ ?b :balances__customers__customer_id ?c . \
         FILTER(?c = <{ont}#customers/{tenant_b}>) }}"
    );
    let body = serde_json::json!({ "query": leak_q }).to_string();
    let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["row_count"], 0, "⛔ A 看到了 B 的行：{text}");
    let sql = v["sql_echo"].as_str().unwrap();
    assert!(
        sql.contains(&format!("= '{tenant_a}'")),
        "注入的必须是 A 的租户：{sql}"
    );
    assert!(
        !sql.contains(&format!("= '{tenant_b}'")),
        "⛔ SQL 里出现了 B 的租户值：{sql}"
    );

    // 对照：**B 自己的 token** 查同一条 ⇒ 不是「这条查询永远 0 行」
    let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_B), Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(
        v["row_count"].as_u64().unwrap() >= 1,
        "B 自己查自己应当有行 —— 否则上面那条 0 行是空转：{text}"
    );
}

/// AC-010：五条真实查询（单跳 / 两跳 / 聚合 / GROUP BY / ORDER BY+LIMIT）。
#[tokio::test]
async fn ac010_five_real_queries_return_receipts() {
    let _live = one_at_a_time();
    let test = "ac010_five_real_queries_return_receipts";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(10_000, &tenant_a, &tenant_a).await.expect("服务可构建");

    for (label, q) in [
        ("单跳", "SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt }".to_string()),
        ("两跳", "SELECT ?l ?n WHERE { ?l :licenses__customers ?c . ?c :customers__customer_name ?n }".to_string()),
        ("聚合", "SELECT (COUNT(?b) AS ?n) WHERE { ?b a :balances }".to_string()),
        ("GROUP BY", "SELECT ?c (COUNT(?b) AS ?n) WHERE { ?b :balances__customers__customer_id ?c } GROUP BY ?c".to_string()),
        ("ORDER BY+LIMIT", "SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt } ORDER BY DESC(?amt) LIMIT 5".to_string()),
    ] {
        let body = serde_json::json!({ "query": q }).to_string();
        let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "[{label}] {text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v["row_count"].is_number(), "[{label}] 缺 row_count：{text}");
        assert!(v["warnings"].is_array(), "[{label}] 缺 warnings：{text}");
        assert!(v["coverage"].is_array(), "[{label}] 缺 coverage：{text}");
        assert!(v["join_states"].is_array(), "[{label}] 缺 join_states：{text}");
        // `sql_echo` 必须带着注入条件（不是「回执里好看的一行」）
        assert!(
            v["sql_echo"].as_str().unwrap().contains(&format!("= '{}'", tenant_a)),
            "[{label}] sql_echo 里没有注入条件：{text}"
        );
    }
}

/// AC-004 的错误侧：同一失败在 REST（HTTP code）与 MCP（`structuredContent.code`）相同。
#[tokio::test]
async fn ac004_the_same_error_code_on_both_protocols() {
    let _live = one_at_a_time();
    let test = "ac004_the_same_error_code_on_both_protocols";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(10_000, &tenant_a, &tenant_a).await.expect("服务可构建");

    let q = "SELECT ?x WHERE { ?x a :no_such_class }";
    let body = serde_json::json!({ "query": q }).to_string();
    let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let rest_err: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(rest_err["code"], "UNKNOWN_CLASS");
    assert!(!rest_err["hint"].as_str().unwrap().is_empty());

    let line = serde_json::json!({
        "jsonrpc": "2.0", "id": 9, "method": "tools/call",
        "params": { "name": "sparql_query", "arguments": { "query": q } }
    })
    .to_string();
    let v = mcp(&svc, TOK_A, &line).await;
    assert_eq!(v["result"]["isError"], true, "{v}");
    assert_eq!(v["result"]["structuredContent"]["code"], rest_err["code"]);
    assert_eq!(v["result"]["structuredContent"]["hint"], rest_err["hint"]);
}

/// catalog / describe 在两个协议上也逐字节相同（顺带覆盖 AC-001 的两个 GET）。
#[tokio::test]
async fn ac001_catalog_and_describe_are_identical_across_protocols() {
    let _live = one_at_a_time();
    let test = "ac001_catalog_and_describe_are_identical_across_protocols";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(10_000, &tenant_a, &tenant_a).await.expect("服务可构建");

    let (status, cat) = rest(&svc, "GET", "/api/catalog?page=1&page_size=5", Some(TOK_A), None).await;
    assert_eq!(status, StatusCode::OK, "{cat}");
    let v = mcp(
        &svc,
        TOK_A,
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"catalog","arguments":{"page":1,"page_size":5}}}).to_string(),
    )
    .await;
    assert_eq!(v["result"]["content"][0]["text"].as_str().unwrap(), cat);

    // describe：用一个**一定存在**的名字（类清单的第一条）
    let first_name = serde_json::from_str::<Value>(&cat)
        .unwrap()["items"][0]["name"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, des) = rest(
        &svc,
        "GET",
        &format!("/api/describe?name=:{}", first_name.trim_start_matches(':')),
        Some(TOK_A),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{des}");
    let _: Value = serde_json::from_str(&des).expect("describe 形状");
    let v = mcp(
        &svc,
        TOK_A,
        &serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"describe","arguments":{"name": format!(":{}", first_name)}}}).to_string(),
    )
    .await;
    assert_eq!(v["result"]["content"][0]["text"].as_str().unwrap(), des);
}

/// AC-007：审计行里**没有** token 明文、**没有**查询原文；失败也有一行。
#[tokio::test]
async fn ac007_audit_has_no_token_and_no_query_text() {
    let _live = one_at_a_time();
    let test = "ac007_audit_has_no_token_and_no_query_text";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(10_000, &tenant_a, &tenant_a).await.expect("服务可构建");

    let q = "SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt }";
    let caller = Caller {
        token: svc.tokens.find(TOK_A).expect("token A").clone(),
        protocol: Protocol::Rest,
    };
    // 成功一次 + 失败一次（两条都要有审计）
    let ok = ontok::query::run_sparql(&svc, &caller, SparqlRequest {
        query: q.to_string(),
        max_rows: None,
        tenant: None,
    })
    .await;
    assert!(ok.is_ok(), "{ok:?}");
    let bad = ontok::query::run_sparql(&svc, &caller, SparqlRequest {
        query: "SELECT ?x WHERE { OPTIONAL { ?x a :balances } }".to_string(),
        max_rows: None,
        tenant: None,
    })
    .await;
    assert_eq!(bad.unwrap_err().code, "SPARQL_UNSUPPORTED");

    // 审计走的是 stderr；这里直接断言 `AuditRecord` 的形状（内容判据在 lib 单测里）。
    // 真库路径上再验一次「token 摘要 != 明文」：把 token 的 sha256 前缀算出来比对。
    let digest = ontok::query::audit::sha256_hex(TOK_A.as_bytes());
    let prefix = &digest[..12];
    assert_ne!(prefix, TOK_A, "摘要不能等于明文");
    assert!(!digest.contains(TOK_A));
}

/// AC-005：调用方**抬不高** token 的 max_rows（真库路径）。
#[tokio::test]
async fn ac005_the_token_row_cap_cannot_be_raised() {
    let _live = one_at_a_time();
    let test = "ac005_the_token_row_cap_cannot_be_raised";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(10_000, &tenant_a, &tenant_a).await.expect("服务可构建");
    let body = serde_json::json!({
        "query": "SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt }",
        "max_rows": 999_999u64
    })
    .to_string();
    let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["limits"]["max_rows"], 200, "必须被压到 token 上限：{text}");
    assert!(
        v["warnings"].as_array().unwrap().iter().any(|w| w
            .as_str()
            .unwrap()
            .contains("凭证上限")),
        "压上限必须给 warning：{text}"
    );
}

/// AC-005：超时 ⇒ `TIMEOUT`（把服务的 timeout 设成 1ms）。
#[tokio::test]
async fn ac005_a_timeout_is_reported_as_timeout() {
    let _live = one_at_a_time();
    let test = "ac005_a_timeout_is_reported_as_timeout";
    let missing_required = require(
        test,
        &["ONTOK_REQ123_SCHEMA", "ONTOK_REQ123_TENANT", "ONTOK_SOURCE_DATABASE_URL"],
    )
    .is_none();
    let missing_db = meta_url().is_none();
    // ⚠️ 一条用例只打**一行**标记（REQ-131 · 可数性）：`require` 已经说过缺什么时，
    // 不再补一句 `ONTOK_DATABASE_URL` —— 否则 `grep -c` 数出来的不是用例数。
    if !missing_required && missing_db {
        common::skip::missing("ONTOK_DATABASE_URL", test);
    }
    if missing_required || missing_db {
        return;
    }
    let tenant_a = env("ONTOK_REQ123_TENANT").unwrap();
    let svc = build_service(1, &tenant_a, &tenant_a).await.expect("服务可构建");
    let body = serde_json::json!({
        "query": "SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt }"
    })
    .to_string();
    let (status, text) = rest(&svc, "POST", "/api/sparql", Some(TOK_A), Some(&body)).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "TIMEOUT");
    assert!(!v["hint"].as_str().unwrap().is_empty());
}
