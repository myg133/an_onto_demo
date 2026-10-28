//! REQ-122 · **泄漏判据（AC-005）/ 只读判据（AC-008）** —— 两层，缺库才 SKIP。
//!
//! # 两层判据，为什么不能只有一层
//!
//! | 层 | 需要什么 | 证明什么 |
//! | --- | --- | --- |
//! | ① **真源库对拍** | `ONTOK_SOURCE_DATABASE_URL` + 元数据库 + 两个真实租户 | 在**真实数据 + 真实映射 + 真实声明**下，A 的结果里一行 B 的数据都没有 |
//! | ② **测试库内两租户** | 只需要 `ONTOK_TEST_DATABASE_URL`（常规 CI 配置） | 走完整 `run_query` 路径（规划 → 注入 → 只读执行），A 查不到 B；且 `sql_echo` 原样再执行**行集不变** |
//!
//! ② 是 REQ-122 · N2 补的：原先只有 ①，而 ① 缺环境就静静 SKIP、`cargo test`
//! 照样「通过」—— 最重要的一条判据在 CI 上很可能**根本没跑**。
//! 现在 ② 只需要测试库，覆盖率不再取决于有没有客户源库。
//!
//! # SKIP 是**响亮的**（N2）
//!
//! 缺库时打印一行 `⚠ SKIP · <测试名>：<缺什么>`，并且**直接写控制台设备**
//! （绕过 libtest 对测试线程 stdout/stderr 的捕获）—— 默认的 `cargo test`
//! 输出里就能看见，不是只有 `--nocapture` 才看得见。
//! ⛔ 「没跑」不许看起来像「通过」。
//!
//! # ① 怎么跑
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
//!
//! # ② 怎么跑
//!
//! ```text
//! export ONTOK_TEST_DATABASE_URL="postgresql://…（测试库，可建 schema）"
//! cargo test --test test_req122_live
//! ```
//!
//! ② 自己建一个**专属 schema**（`t_req122_leak_<pid>_<n>`）+ 两张表 + 两个租户的
//! 夹具数据，跑完 `DROP SCHEMA … CASCADE` 并**复查确实没了**；
//! ⛔ 不碰 public、不 DELETE/TRUNCATE 任何共享表。
//!
//! # 它**不**证明什么
//!
//! - ① 不证明「换个客户库也对」—— 证据强度就是**这一个库 + 这一份声明**；
//! - ② 的夹具是**我们自己造的**，它证明的是「机制在真库上真的按声明过滤」，
//!   不证明客户库里的历史脏数据也守得住（那是 ① 的事）；
//! - 都不证明没有别的泄漏路径（例如将来的 HTTP 端点，见 REQ-123）。

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use std::collections::BTreeSet;

use sqlx::postgres::PgPool;
use sqlx::Row;

use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::model_view::{
    ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
};
use ontok::domain::r2rml::{build_mapping, R2rmlMapping};
use ontok::domain::snapshot::{Evidence, JoinState};
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

/// **响亮的 SKIP**（REQ-122 · N2）。
///
/// 四重可见，缺一都不够：
/// 1. `⚠ SKIP` 字样 + **测试名**（取运行时线程名 —— libtest 用测试名命名测试线程）；
/// 2. 写 stderr（`--nocapture` / 失败时可见）；
/// 3. 直接写控制台设备（Windows `CONOUT$` / Unix `/dev/tty`）—— 有人在终端时看得见；
/// 4. **子进程回显**（[`echo_uncaptured`]）：libtest 的捕获是**测试线程的
///    Rust 层 output capture**，子进程的 stdout 直接继承本进程的真实句柄 ⇒
///    连**默认**（不带 `--nocapture`）的 `cargo test`/CI 日志里都能看见。
///
/// ⛔ 只做 1/2 就是老问题：默认 `cargo test` 把通过测试的输出全吞掉，
/// 「静默 SKIP 且算 pass」照旧。
fn skip(reason: &str) {
    let name = std::thread::current()
        .name()
        .unwrap_or("<未命名测试线程>")
        .to_string();
    let detail = format!(
        "\n\
         ⚠ SKIP · {name}\n  \
         原因：{reason}\n  \
         ⇒ 这条判据**没有执行**（不是通过）。AC-005 泄漏 / AC-008 只读是 REQ-122 最重要的判据；\n    \
         请按 tests/test_req122_live.rs 文件头补上环境变量后重跑。\n"
    );
    // ② stderr（--nocapture / 失败时可见）
    eprintln!("{detail}");
    // ③ 控制台设备（有人在终端时可见；拿不到就算了）
    for dev in ["CONOUT$", "/dev/tty"] {
        if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(dev) {
            use std::io::Write;
            let _ = f.write_all(detail.as_bytes());
            let _ = f.flush();
            break;
        }
    }
    // ④ 子进程回显 —— 这条才保证**默认** `cargo test` 的输出里有它。
    //    只用 ASCII + 不用 shell 元字符，避开控制台代码页与 cmd 解析的坑；
    //    于是这里**不塞中文 reason**，只点名缺了哪几个环境变量。
    let envs: Vec<&str> = reason
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with("ONTOK_") || w.starts_with("REQ122_"))
        .collect();
    let missing = if envs.is_empty() {
        "required environment".to_string()
    } else {
        envs.join(",")
    };
    let ascii = format!(
        "*** SKIP: {name} -- missing {missing} -- the criterion was NOT executed, this is NOT a pass ***"
    );
    echo_uncaptured(&ascii);
}

/// 把一行文字写到**不受 libtest 捕获**的地方。
///
/// 原理：libtest 的捕获是当前测试线程里的 `io::set_output_capture`（Rust 层），
/// 而子进程用的是本进程**继承下去的真实 stdin/stdout/stderr 句柄** ——
/// 于是子进程打印的内容直接进终端 / CI 日志，与 `--nocapture` 无关。
///
/// ⛔ 传给 shell 的文字必须是 ASCII、不含 shell 元字符（见 [`skip`] 的组装）。
fn echo_uncaptured(line: &str) {
    debug_assert!(line.is_ascii(), "回显内容必须是 ASCII：{line}");
    debug_assert!(
        !line.contains(['&', '|', '<', '>', '(', ')', '"', '^']),
        "回显内容不许含 shell 元字符：{line}"
    );
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        c.raw_arg("/C").raw_arg(format!("echo {line}"));
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg("printf '%s\\n' \"$1\"").arg("sh").arg(line);
        c
    };
    let _ = cmd.status();
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

// ══════════════════════════════════════════════════════════════
// ① 真源库对拍
// ══════════════════════════════════════════════════════════════

struct Ctx {
    mapping: R2rmlMapping,
    schema: SchemaTypes,
    policy: QueryPolicy,
    iri: String,
    src: PgPool,
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
        api_classes: None,
    };
    let mapping =
        build_mapping(&model, &opts, false, Default::default()).expect("映射必须生成");
    let iri = ontology_iri(&name);

    let src = exec::connect_read_only(&src_url).await.expect("连源库（只读）").pool;
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
        skip("缺 REQ122_TENANT_A / REQ122_TENANT_B（真实租户 id 不进仓库）");
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

/// I-1 · 真实声明里的 `coverage: main_only` 必须在真实回执里出现（且只在命中时）。
#[tokio::test]
async fn i1_partial_coverage_is_announced_on_real_data() {
    let Some((a, _b)) = tenants() else {
        skip("缺 REQ122_TENANT_A / REQ122_TENANT_B（真实租户 id 不进仓库）");
        return;
    };
    let Some(ctx) = setup().await else {
        skip("缺 ONTOK_DATABASE_URL / ONTOK_SOURCE_DATABASE_URL / schema / 声明");
        return;
    };

    // 命中 main_only 表 ⇒ 点名警告（含 coverage 值与声明写的短句）
    let res = run(&ctx, &a, "SELECT ?b WHERE { ?b a :dim_customer_binding }").await;
    let hit: Vec<&String> = res
        .warnings
        .iter()
        .filter(|w| w.contains("dim_customer_binding") && w.contains("main_only"))
        .collect();
    assert_eq!(hit.len(), 1, "main_only 表命中时必须警告：{:?}", res.warnings);
    assert!(hit[0].contains("主账号看不到"), "警告正文来自声明：{}", hit[0]);

    // 同一次查询里 complete 的表不该被报
    let res = run(&ctx, &a, "SELECT ?u WHERE { ?u a :usage_details }").await;
    assert!(
        !res.warnings.iter().any(|w| w.contains("覆盖范围")),
        "complete 的表不该出现覆盖范围警告：{:?}",
        res.warnings
    );
    ctx.src.close().await;
}

#[tokio::test]
async fn ac008_the_session_is_read_only() {
    let Some(src_url) = env_nonempty("ONTOK_SOURCE_DATABASE_URL") else {
        skip("缺 ONTOK_SOURCE_DATABASE_URL —— AC-008 只读判据未执行");
        return;
    };
    let conn = exec::connect_read_only(&src_url).await.expect("只读连接");
    let pool = &conn.pool;
    let ro: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(ro, "on");
    // 回执里的值必须是**读回来的**同一个（N3：不是写死的 on）
    assert_eq!(conn.facts.transaction_read_only, ro);
    let scs: String = sqlx::query_scalar("SHOW standard_conforming_strings")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(scs, "on");
    assert_eq!(conn.facts.standard_conforming_strings, scs);
    // 只读会话里写操作必须失败（不是「我们没写」，而是「写不了」）
    let err = sqlx::query("CREATE TEMP TABLE req122_should_fail (x int)")
        .execute(pool)
        .await
        .expect_err("只读会话里建表必须失败");
    let msg = err.to_string();
    assert!(
        msg.contains("read-only") || msg.contains("只读"),
        "错误里应当说清只读：{msg}"
    );
    conn.pool.close().await;
}

// ══════════════════════════════════════════════════════════════
// ② 测试库内两租户（只需要 ONTOK_TEST_DATABASE_URL）
// ══════════════════════════════════════════════════════════════

const TENANT_A: &str = "CUST-A";
const TENANT_B: &str = "CUST-B";
const LEASE_A: [&str; 2] = ["ORD-A1", "ORD-A2"];
const LEASE_B: [&str; 2] = ["ORD-B1", "ORD-B2"];

/// 一个**专属** schema：建、用、删都在这里，⛔ 不碰 public 的任何表。
struct Sandbox {
    schema: String,
    /// 带 `search_path=<schema>` 的连接串（测试库；建表用可写池，查询用只读池）。
    url: String,
    admin: PgPool,
}

fn scoped_url(base: &str, schema: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}options=-c%20search_path%3D{schema}")
}

impl Sandbox {
    async fn new(base: &str) -> Sandbox {
        // 名字带 pid + 纳秒：并发的测试进程/同一进程的多条用例都不撞名。
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let schema = format!("t_req122_leak_{}_{}", std::process::id(), nanos);
        let admin = ontok::db::pool::connect(base).await.expect("连测试库（建 schema）");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("建专属 schema 失败");
        let url = scoped_url(base, &schema);
        Sandbox { schema, url, admin }
    }

    /// 可写池（**只在造夹具时**用；查询走只读池）。
    async fn writable(&self) -> PgPool {
        ontok::db::pool::connect(&self.url).await.expect("连测试库（造夹具）")
    }

    /// 删掉自己的 schema 并**复查**（⛔ 不静默吞失败 —— REQ-078 F-3 的口径）。
    async fn cleanup(&self) -> Result<(), String> {
        if !self.schema.starts_with("t_req122_leak_")
            || !self
                .schema
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(format!("schema 名 `{}` 不在白名单内 —— 拒绝清理", self.schema));
        }
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema))
            .execute(&self.admin)
            .await
            .map_err(|e| format!("`DROP SCHEMA … CASCADE` 失败：{e}"))?;
        let still: Option<String> = sqlx::query_scalar(
            "SELECT nspname FROM pg_namespace WHERE nspname = $1",
        )
        .bind(&self.schema)
        .fetch_optional(&self.admin)
        .await
        .map_err(|e| format!("复查 schema 是否删净时查询失败：{e}"))?;
        match still {
            None => Ok(()),
            Some(s) => Err(format!("`DROP SCHEMA` 报成功但 {s} 还在 —— 清理没生效")),
        }
    }
}

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

/// 夹具的 schema（列名/类型与下面 `CREATE TABLE` **逐列对应**）。
fn fixture_schema() -> SchemaTypes {
    let doc = serde_json::json!({
        "tables": [
            {
                "name": "customers",
                "primary_key": "id",
                "composite_primary_key": [],
                "unique_columns": ["id"],
                "columns": [
                    {"name": "id", "data_type": "text", "is_primary_key": true},
                    {"name": "customer_name", "data_type": "text"}
                ]
            },
            {
                "name": "orders",
                "primary_key": "order_id",
                "composite_primary_key": [],
                "unique_columns": ["order_id"],
                "columns": [
                    {"name": "order_id", "data_type": "text", "is_primary_key": true},
                    {"name": "customer_id", "data_type": "text"},
                    {"name": "amount", "data_type": "numeric(12,2)"}
                ]
            }
        ]
    });
    SchemaTypes::parse(&doc.to_string()).expect("夹具 schema 必须合法")
}

fn fixture_mapping(schema: &SchemaTypes) -> R2rmlMapping {
    let view = ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![
            entity("customers", vec![prop("客户名", "customer_name")]),
            entity(
                "orders",
                vec![prop("金额", "amount"), prop("客户", "customer_id")],
            ),
        ],
        link_types: vec![link("orders", "customers", "customer_id", "id")],
        action_types: vec![],
        functions: vec![],
    };
    let mut opts = TtlOptions::new("demo", 2);
    opts.schema = Some(schema);
    build_mapping(&view, &opts, false, Default::default()).expect("夹具映射必须能生成")
}

/// 夹具声明：`orders` 按 `customer_id` 过滤；`customers` 是租户主体（按 `id`）。
fn fixture_policy() -> QueryPolicy {
    QueryPolicy::parse(
        r#"{
          "version": "1.0.0",
          "tenantSubject": "customers",
          "tenantIdColumn": "id",
          "perTable": { "orders": { "tenantColumn": "customer_id" } },
          "publicDimensions": [],
          "reviewed_by": "REQ-122 测试"
        }"#,
    )
    .expect("夹具声明必须合法")
}

/// 测试库夹具上的一次查询（把「映射 + schema + 声明 + 连接」收在一起，
/// 免得每条断言都重复一遍 `PlanInput` 的字段）。
struct Harness<'a> {
    mapping: &'a R2rmlMapping,
    schema: &'a SchemaTypes,
    policy: &'a QueryPolicy,
    iri: &'a str,
    pool: &'a PgPool,
}

impl Harness<'_> {
    async fn run(&self, tenant: &str, sparql: &str) -> QueryResult {
        let input = PlanInput {
            mapping: self.mapping,
            schema: self.schema,
            ontology_iri: self.iri,
            policy: self.policy,
            tenant,
            max_rows: 1000,
        };
        run_query(self.pool, &input, sparql, 10_000)
            .await
            .unwrap_or_else(|e| panic!("查询失败：{e}"))
    }
}

/// **N2 的主判据**：只配测试库（没有客户源库）也能证明「A 查不到 B」。
///
/// 走的是**真实**路径：`run_query` = 声明驱动规划 → 逐表注入 → `sql_echo` → 只读执行。
/// 夹具里两个租户各有 2 张订单；A 的查询结果必须恰好是 A 的 2 张，与 B 的交集为空。
/// 另外两条判据：
/// - `sql_echo` 原样再执行**行集不变**（N1：echo 就是执行的那条 SQL，端到端）；
/// - 反向 / 正面越权 FILTER 都是 0 行（注入是 AND）。
#[tokio::test]
async fn ac005_testdb_a_tenant_never_sees_another_tenants_rows() {
    let Some(base) = env_nonempty("ONTOK_TEST_DATABASE_URL") else {
        skip(NEED_TEST_DB);
        return;
    };

    let sb = Sandbox::new(&base).await;
    let writable = sb.writable().await;

    // ── 夹具：两张表、两个租户、各 2 行 ───────────────────────
    for sql in [
        "CREATE TABLE customers (id text PRIMARY KEY, customer_name text NOT NULL)",
        "CREATE TABLE orders (order_id text PRIMARY KEY, customer_id text NOT NULL, \
         amount numeric(12,2) NOT NULL)",
        "INSERT INTO customers (id, customer_name) VALUES ('CUST-A','甲客户'),('CUST-B','乙客户')",
        "INSERT INTO orders (order_id, customer_id, amount) VALUES \
         ('ORD-A1','CUST-A',100),('ORD-A2','CUST-A',250),\
         ('ORD-B1','CUST-B',900),('ORD-B2','CUST-B',5)",
    ] {
        sqlx::query(sql)
            .execute(&writable)
            .await
            .unwrap_or_else(|e| panic!("夹具 SQL 失败：{sql}\n{e}"));
    }

    let schema = fixture_schema();
    let mapping = fixture_mapping(&schema);
    let policy = fixture_policy();
    let iri = "https://ontok.local/demo";
    let readonly = exec::connect_read_only(&sb.url)
        .await
        .expect("测试库上也要能拿到只读连接（search_path 已限定在专属 schema）");
    let h = Harness {
        mapping: &mapping,
        schema: &schema,
        policy: &policy,
        iri,
        pool: &readonly.pool,
    };

    // ── ① 单表：A 只拿到 A 的两张订单 ─────────────────────────
    let q = "SELECT ?o ?a WHERE { ?o :orders__amount ?a }";
    let res_a = h.run(TENANT_A, q).await;
    let res_b = h.run(TENANT_B, q).await;
    let a_ids = ids(&res_a, 0);
    let b_ids = ids(&res_b, 0);
    assert_eq!(
        a_ids,
        LEASE_A.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
        "A 必须恰好拿到自己的两行：{}",
        res_a.sql_echo
    );
    assert_eq!(
        b_ids,
        LEASE_B.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>(),
        "B 必须恰好拿到自己的两行：{}",
        res_b.sql_echo
    );
    assert!(a_ids.is_disjoint(&b_ids), "⛔ 泄漏：两个租户的行集相交了");

    // ── ② echo 就是执行的 SQL（端到端）：原样再执行，行集必须逐行相同 ──
    assert!(
        res_a.sql_echo.contains(r#"WHERE t0."customer_id" = 'CUST-A'"#),
        "echo 的 WHERE 最前面必须是注入条件：{}",
        res_a.sql_echo
    );
    assert!(
        res_a.sql_echo.contains("LIMIT 1001 /* REQ-122: 上限 1000 行 + 1 行探针"),
        "echo 必须带 LIMIT N+1 探针：{}",
        res_a.sql_echo
    );
    assert!(
        res_a.sql_echo.trim_end().ends_with("探针不进结果 */"),
        "{}",
        res_a.sql_echo
    );
    let echoed = run_verbatim(&readonly.pool, &res_a.sql_echo, res_a.columns.len()).await;
    assert_eq!(
        echoed, res_a.rows,
        "把 sql_echo 原样再执行必须得到同一批行 —— 回执与执行漂移了"
    );

    // ── ③ 两跳：两张表都要被注入，结果仍不含 B ────────────────
    let res = h.run(TENANT_A, "SELECT ?o ?c WHERE { ?o :orders__customers ?c }").await;
    assert_eq!(
        res.tenant_applied.len(),
        2,
        "orders 与 customers 都要注入：{:?}",
        res.tenant_applied
    );
    for t in &res.tenant_applied {
        assert!(
            res.sql_echo.contains(&format!("\"{}\" = '{}'", t.column, t.value)),
            "表 {} 的注入没出现在 echo 里：{}",
            t.table,
            res.sql_echo
        );
    }
    for row in &res.rows {
        let c = row[1].clone().unwrap_or_default();
        assert_eq!(id_of(&c), TENANT_A, "行里出现了别的客户：{c}");
    }

    // ── ④ 反向筛（本租户之外的全排除）与正面越权（直接点名 B）⇒ 都 0 行 ──
    let back = h
        .run(
            TENANT_A,
            &format!(
                "SELECT ?o ?c WHERE {{ ?o :orders__customers ?c . FILTER (?c != <{iri}#customers/{TENANT_A}>) }}"
            ),
        )
        .await;
    assert_eq!(back.row_count, 0, "反向筛必须被注入挡住：{}", back.sql_echo);

    let over = h
        .run(
            TENANT_A,
            &format!(
                "SELECT ?o ?c WHERE {{ ?o :orders__customers ?c . FILTER (?c = <{iri}#customers/{TENANT_B}>) }}"
            ),
        )
        .await;
    assert_eq!(over.row_count, 0, "查别人必须 0 行：{}", over.sql_echo);

    // ── 收尾：删自己的 schema 并复查 ──────────────────────────
    readonly.pool.close().await;
    writable.close().await;
    sb.cleanup().await.expect("清理专属 schema 失败");
    sb.admin.close().await;
}

/// 把回执里的 `sql_echo` **原样**执行一次（不经过规划器），把行解码成文本。
async fn run_verbatim(pool: &PgPool, sql: &str, columns: usize) -> Vec<Vec<Option<String>>> {
    let rows = sqlx::query(sql)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| panic!("回执里的 sql_echo 原样执行失败：{e}\nSQL: {sql}"));
    rows.iter()
        .map(|r| {
            (0..columns)
                .map(|i| {
                    r.try_get::<Option<String>, _>(i)
                        .unwrap_or_else(|e| panic!("取第 {} 列失败：{e}", i + 1))
                })
                .collect()
        })
        .collect()
}
