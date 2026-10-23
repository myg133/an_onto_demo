//! REQ-122 · **真实源库对拍**（AC-005 泄漏测试 / AC-008 只读）—— 有源库才跑。
//!
//! # 这条判据为什么必须连真库
//!
//! 「A 查不到 B 的数据」不可能由构造样本证明 —— 构造样本只能证明「我写了一个会加
//! `AND tenant = ?` 的函数」，证明不了「在真实数据 + 真实映射 + 真实声明下，
//! A 的结果里**一行** B 的数据都没有」。所以这里连真库、用**两个真实租户**逐行核对 id。
//!
//! # 怎么跑
//!
//! ```text
//! export ONTOK_DATABASE_URL="postgresql://…（元数据库，只读）"
//! export ONTOK_SOURCE_DATABASE_URL="postgresql://…（源库，只读账号）"
//! export REQ122_TENANT_A="<客户 id>"
//! export REQ122_TENANT_B="<另一个客户 id>"
//! cargo test --test test_req122_live -- --nocapture
//! ```
//!
//! 连接串与**租户 id 都只从环境变量读**，代码与文档里都没有它们 ——
//! 真实客户 id 与口令一样不进仓库（同 REQ-084 N1 的口径）。
//! 缺任何一个变量 ⇒ 打印一行 `SKIP` 并返回（不用 `#[ignore]`：那不是「不跑」，
//! 而是「这台机器没有源库可跑」，两者透明度不同）。
//!
//! # 它**不**证明什么
//!
//! - 不证明「换个客户库也对」—— 本判据的证据强度就是**这一个库 + 这一份声明**；
//! - 不证明没有别的泄漏路径（例如将来的 HTTP 端点，见 REQ-123）。

use std::collections::BTreeSet;

use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::r2rml::build_mapping;
use ontok::domain::ttl::{SchemaTypes, TtlOptions};
use ontok::sparql::plan::PlanInput;
use ontok::sparql::policy::QueryPolicy;
use ontok::sparql::{exec, run_query, QueryResult};

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// `(A, B)` 两个真实租户 id；缺一个就 `None`。
fn tenants() -> Option<(String, String)> {
    Some((env_nonempty("REQ122_TENANT_A")?, env_nonempty("REQ122_TENANT_B")?))
}

fn project() -> String {
    env_nonempty("REQ122_PROJECT").unwrap_or_else(|| "anspire_r2rml_excl".to_string())
}

fn version() -> i32 {
    env_nonempty("REQ122_VERSION")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2)
}

fn schema_path() -> String {
    env_nonempty("REQ122_SCHEMA").unwrap_or_else(|| "../code/out/schema.json".to_string())
}

fn policy_path() -> String {
    env_nonempty("REQ122_POLICY").unwrap_or_else(|| "query-policy.json".to_string())
}

fn skip(reason: &str) {
    println!("SKIP · {reason} —— 本判据需要真实源库与两个真实租户（见文件头）");
}

/// 取某个实例 IRI 的 id 部分（结果里实例列是完整 IRI）。
fn id_of(iri: &str) -> String {
    iri.rsplit('/').next().unwrap_or(iri).to_string()
}

fn ids(res: &QueryResult, col: usize) -> BTreeSet<String> {
    res.rows
        .iter()
        .filter_map(|r| r.get(col).cloned().flatten())
        .map(|v| id_of(&v))
        .collect()
}

struct Ctx {
    mapping: ontok::domain::r2rml::R2rmlMapping,
    schema: SchemaTypes,
    policy: QueryPolicy,
    iri: String,
    src: sqlx::postgres::PgPool,
}

async fn setup() -> Option<Ctx> {
    let meta_url = env_nonempty("ONTOK_DATABASE_URL")?;
    let src_url = env_nonempty("ONTOK_SOURCE_DATABASE_URL")?;
    let schema_raw = std::fs::read_to_string(schema_path())
        .map_err(|e| println!("SKIP · 读不出 schema：{e}"))
        .ok()?;
    let policy_raw = std::fs::read_to_string(policy_path())
        .map_err(|e| println!("SKIP · 读不出声明：{e}"))
        .ok()?;
    let policy = QueryPolicy::parse(&policy_raw).expect("query-policy.json 必须合法");
    let schema = SchemaTypes::parse(&schema_raw).expect("schema.json 必须合法");

    let meta = ontok::db::pool::connect(&meta_url).await.expect("连元数据库");
    let projects = PgProjectStore::new(meta.clone());
    let store = PgStore::new(meta);
    let name = project();
    let p = projects.get_by_name(&name).await.expect("查项目").expect("项目不存在");
    let v = projects.version_by_seq(p.id, &p.name, version()).await.expect("查版本");
    let snap = store.get_snapshot(v.id).await.expect("读快照").expect("没有快照");
    let model = ontok::domain::model_view::project_model_track(&snap);

    let opts = TtlOptions {
        project: name.as_str(),
        version: version(),
        base: "https://ontok.local/",
        action_type_count: snap.action_types.len(),
        tenancy: None,
        schema: Some(&schema),
    };
    let mapping =
        build_mapping(&model, &opts, false, Default::default()).expect("映射必须生成");
    let iri = ontology_iri(&name);

    let src = exec::connect_read_only(&src_url).await.expect("连源库（只读）");
    Some(Ctx { mapping, schema, policy, iri, src })
}

/// 与 CLI 同一条口径的本体 IRI：`<base><local_model(project)>`。
///
/// 用的是**公开**的 `local_model`（与 TTL / R2RML 同一个），⛔ 不在测试里手写一份。
fn ontology_iri(project: &str) -> String {
    format!("https://ontok.local/{}", ontok::domain::ttl::local_model(project))
}

async fn truth(ctx: &Ctx, table: &str, tenant: &str) -> BTreeSet<String> {
    let sql = format!("SELECT id FROM public.\"{table}\" WHERE customer_id = $1");
    let rows: Vec<(String,)> = sqlx::query_as(&sql)
        .bind(tenant)
        .fetch_all(&ctx.src)
        .await
        .unwrap_or_else(|e| panic!("对拍 SQL 失败：{e}"));
    rows.into_iter().map(|r| r.0).collect()
}

async fn run(ctx: &Ctx, tenant: &str, sparql: &str) -> QueryResult {
    let input = PlanInput {
        mapping: &ctx.mapping,
        schema: &ctx.schema,
        ontology_iri: &ctx.iri,
        policy: &ctx.policy,
        tenant,
        max_rows: 5000,
    };
    run_query(&ctx.src, &input, sparql, 10_000)
        .await
        .unwrap_or_else(|e| panic!("查询失败：{e}"))
}

#[tokio::test]
async fn ac005_a_tenant_never_sees_another_tenants_rows() {
    let Some((a, b)) = tenants() else {
        skip("缺 REQ122_TENANT_A / REQ122_TENANT_B");
        return;
    };
    let Some(ctx) = setup().await else {
        skip("缺 ONTOK_DATABASE_URL / ONTOK_SOURCE_DATABASE_URL / schema / 声明");
        return;
    };

    // ① 主查询不带任何客户条件 ⇒ 只返回本租户；与 B 的 id 集合交集必须为空
    let q_plain = "SELECT ?u ?c WHERE { ?u :usage_details__customers ?c }";
    let own_a = truth(&ctx, "usage_details", &a).await;
    let own_b = truth(&ctx, "usage_details", &b).await;
    assert!(!own_a.is_empty() && !own_b.is_empty(), "两个租户都要有数据才算对拍");

    for (tenant, other_ids) in [(&a, &own_b), (&b, &own_a)] {
        let res = run(&ctx, tenant, q_plain).await;
        let got = ids(&res, 0);
        let expected = truth(&ctx, "usage_details", tenant).await;
        assert_eq!(got, expected, "行集必须与直接 SQL 一致");
        assert!(got.is_disjoint(other_ids), "⛔ 泄漏：出现了另一租户的行");
        assert!(
            res.tenant_applied.iter().any(|t| t.table == "usage_details"),
            "usage_details 必须被注入"
        );
        // 每一行的客户列都必须是本租户
        for row in &res.rows {
            let c = row[1].clone().unwrap_or_default();
            assert_eq!(id_of(&c), *tenant, "行里出现了别的客户：{c}");
        }
    }

    // ② 反向筛：FILTER (?c != 本租户) ⇒ 注入是 AND ⇒ 0 行
    let res = run(
        &ctx,
        &a,
        &format!(
            "SELECT ?u ?c WHERE {{ ?u :usage_details__customers ?c . FILTER (?c != <{}#customers/{a}>) }}",
            ctx.iri
        ),
    )
    .await;
    assert_eq!(res.row_count, 0, "反向筛必须被注入挡住，实际：{}", res.sql_echo);

    // ③ 正面越权：FILTER (?c = B) ⇒ 0 行
    let res = run(
        &ctx,
        &a,
        &format!(
            "SELECT ?u ?c WHERE {{ ?u :usage_details__customers ?c . FILTER (?c = <{}#customers/{b}>) }}",
            ctx.iri
        ),
    )
    .await;
    assert_eq!(res.row_count, 0, "查别人必须 0 行，实际：{}", res.sql_echo);

    // ④ 两跳：每跳涉及的客户表都被过滤
    let res = run(
        &ctx,
        &a,
        "SELECT ?u ?s ?t WHERE { ?u :usage_details__customer_subscriptions ?s . \
         ?s :customer_subscriptions__package_templates ?t }",
    )
    .await;
    assert!(res.tenant_applied.iter().any(|t| t.table == "customer_subscriptions"));
    assert!(ids(&res, 0).is_disjoint(&own_b), "两跳的结果里出现了另一租户的用量");

    // ⑤ 1:1 表：只返回本租户那一行（balances 的 id 是余额行自己的 id，
    //    判据要看**客户列**，不是余额行的 id）
    let res = run(
        &ctx,
        &a,
        "SELECT ?b ?c WHERE { ?b :balances__customers__customer_id ?c . ?b :balances__recharge_balance ?x }",
    )
    .await;
    assert_eq!(res.row_count, 1, "balances 对每个租户恰好一行：{}", res.sql_echo);
    assert_eq!(id_of(res.rows[0][1].as_deref().unwrap()), a);
    ctx.src.close().await;
}

#[tokio::test]
async fn ac008_the_session_is_read_only() {
    let Some(src_url) = env_nonempty("ONTOK_SOURCE_DATABASE_URL") else {
        skip("缺 ONTOK_SOURCE_DATABASE_URL");
        return;
    };
    let pool = exec::connect_read_only(&src_url).await.expect("只读连接");
    let ro: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ro, "on");
    let scs: String = sqlx::query_scalar("SHOW standard_conforming_strings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scs, "on");
    // 只读会话里写操作必须失败（不是「我们没写」，而是「写不了」）
    let err = sqlx::query("CREATE TEMP TABLE req122_should_fail (x int)")
        .execute(&pool)
        .await
        .expect_err("只读会话里建表必须失败");
    let msg = err.to_string();
    assert!(
        msg.contains("read-only") || msg.contains("只读"),
        "错误里应当说清只读：{msg}"
    );
    pool.close().await;
}
