//! REQ-126 · 客户门户的**真库实测**（AC-001 的租户对拍 / AC-004 的覆盖警告 / AC-010 的截断）。
//!
//! # 为什么必须连真库
//!
//! - 「两个真实租户对拍、交集为空」只能在**有数据**的源库上判；
//! - 「`coverage: main_only` 的告警可见」需要一张**真的**标了 main_only 的表
//!   （本仓库的 `query-policy.json` 里是 `deductions`）；
//! - 「截断要可见」需要一条**超过上限**的结果（真库的用量明细才有那么多行）。
//!
//! # 缺环境变量时**响亮地跳过**（沿用 REQ-122/123/124 的口径）
//!
//! 不静默：打印 `*** SKIP: <测试名> -- missing <ENV> ***`（直接写 fd 2，
//! 因为 libtest 会捕获测试线程上的 `eprintln!`）。
//!
//! # 需要哪些环境变量
//!
//! | 变量 | 用途 |
//! | --- | --- |
//! | `ONTOK_DATABASE_URL`（或 `ONTOK_TEST_DATABASE_URL`） | 元数据库 |
//! | `ONTOK_SOURCE_DATABASE_URL` | 源库（**只读**） |
//! | `ONTOK_REQ126_TENANT_A` / `ONTOK_REQ126_TENANT_B` | 两个**有数据**的真实租户 id |
//! | `ONTOK_REQ126_PROJECT` / `ONTOK_REQ126_VERSION` | 已冻结的项目 / 版本，默认 `anspire_r2rml_excl` / `2` |
//! | `ONTOK_REQ126_SCHEMA` / `ONTOK_REQ126_POLICY` | 可省；默认仓库根的 `code/out/schema.json` 与 `query-policy.json` |
//!
//! ⚠️ 租户 id 走环境变量而不是写死：它们是**客户数据**。判据要的是「两个真的有数据的
//! 租户」，不是「这两个 UUID」—— 换个库照样能跑，且不必把客户 id 提交进仓库。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（与 REQ-107/122/123/124 的现场判据同一口径）。

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::portal::serve::{load, ServeArgs};
use ontok::portal::PortalService;
use serde_json::Value;
use tower::ServiceExt;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// 缺哪个变量就**响亮地**说，然后 `None`（测试提前返回 = SKIP）。
fn missing(name: &str, test: &str) {
    use std::io::Write as _;
    let mut err = std::io::stderr();
    let _ = writeln!(err, "*** SKIP: {test} -- missing {name} ***");
}

fn meta_url() -> Option<String> {
    env("ONTOK_DATABASE_URL").or_else(|| env("ONTOK_TEST_DATABASE_URL"))
}

fn repo_root() -> PathBuf {
    common::crate_root()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(common::crate_root)
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req126-live-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

const TOKEN_A: &str = "LIVE-PORTAL-TOKEN-aaaaaaaaaaaaaaaa";
const TOKEN_B: &str = "LIVE-PORTAL-TOKEN-bbbbbbbbbbbbbbbbbbbb";

/// 一份能真起门户的参数（审计写临时文件，便于事后查）。
struct Live {
    svc: Arc<PortalService>,
    audit: PathBuf,
}

fn args_for(tag: &str, tenant_a: &str, tenant_b: &str, questions: &Path, max_rows_b: u64) -> (ServeArgs, PathBuf) {
    let dir = temp_dir(tag);
    let tokens = dir.join("portal-tokens.json");
    std::fs::write(
        &tokens,
        format!(
            r#"{{"version":"1.0.0","tokens":[
                {{"label":"A","token":"{TOKEN_A}","kind":"portal","tenant":"{tenant_a}","max_rows":200,"max_concurrency":2}},
                {{"label":"B","token":"{TOKEN_B}","kind":"portal","tenant":"{tenant_b}","max_rows":{max_rows_b},"max_concurrency":1}}
            ]}}"#
        ),
    )
    .expect("写凭据表");
    let audit = dir.join("portal-audit.log");
    let schema = env("ONTOK_REQ126_SCHEMA")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("code").join("out").join("schema.json"));
    // 声明在工作树根（`feature-*` 里有一份），schema 在上一级的 `code/out/` —— 两处都试。
    let policy = env("ONTOK_REQ126_POLICY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let local = common::crate_root().join("query-policy.json");
            if local.is_file() {
                local
            } else {
                repo_root().join("query-policy.json")
            }
        });
    let args = ServeArgs {
        bind: "127.0.0.1:8900".into(),
        tokens,
        project: env("ONTOK_REQ126_PROJECT").unwrap_or_else(|| "anspire_r2rml_excl".into()),
        version: env("ONTOK_REQ126_VERSION")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(2),
        schema,
        policy,
        questions: questions.to_path_buf(),
        allow_public: false,
        audit_out: Some(audit.clone()),
        timeout_ms: 15_000,
        base: "https://ontok.local/".into(),
        include_inherit: false,
    };
    (args, audit)
}

async fn live(tag: &str, tenant_a: &str, tenant_b: &str, questions: &Path, max_rows_b: u64) -> Option<Live> {
    let Some(meta) = meta_url() else {
        missing("ONTOK_DATABASE_URL", tag);
        return None;
    };
    if env("ONTOK_SOURCE_DATABASE_URL").is_none() {
        missing("ONTOK_SOURCE_DATABASE_URL", tag);
        return None;
    }
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect(&meta)
        .await
        .expect("连元数据库");
    let (args, audit) = args_for(tag, tenant_a, tenant_b, questions, max_rows_b);
    let svc = load(&pool, &args).await.expect("门户应当能载入（问题声明全合法）");
    Some(Live {
        svc: Arc::new(svc),
        audit,
    })
}

struct Reply {
    status: StatusCode,
    body: String,
}

async fn call(app: axum::Router, req: Request<Body>) -> Reply {
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 16 * 1024 * 1024)
        .await
        .expect("读 body");
    Reply {
        status,
        body: String::from_utf8_lossy(&bytes).to_string(),
    }
}

fn post_q(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("{}"))
        .unwrap()
}


// ══════════════════════════════════════════════════════════════
// N-1 · **真值对拍**（取代「两个租户行值交集」这条会假红的判据）
// ══════════════════════════════════════════════════════════════
//
// # 为什么不能用「A 的行 ∩ B 的行 = ∅」
//
// 两个租户的余额**都是 `0.00000000`** 时，值级交集非空 ⇒ 判据假红。
// 而它恰恰不是泄漏：两边本来就各有自己的那一行 0。一条会假红的判据比没有判据更糟
// —— 它会训练人「看到红就换一组数据」，最后真泄漏没人看。
//
// # 换成什么：**本租户的真值**（直接 SQL 算出来，只读）
//
// 对每个租户、每条问题：
// 1. 用**手写 SQL**（不复用门户的规划器）从源库算出该租户的真值集合（多重集）；
// 2. 断言**回答 ⊆ 本租户真值**（每一行都能由他自己的记录解释）；
// 3. 未截断时断言**回答 == 真值**（少行也是错：那是「查漏了」）；
// 4. ⛔ 回答里出现「本租户真值里根本没有的行」⇒ 红（那就是别的租户的数据）。
//
// ⇒ 「两边都为 0」自然通过（他自己的真值也是 0）；真泄漏仍然被抓
// （见 `n1_a_hardcoded_tenant_is_caught`）。
//
// ⚠️ 已知边界（如实登记）：如果泄漏来的行**在值上恰好等于**本租户自己的某一行，
// 从回答里**不可能**分辨出来（投影里没有身份列）。这条边界写在报告里，⛔ 不装作已封堵。

/// 交付的 5 条问题 → **手写真值 SQL**（`$1` = 租户）。
///
/// 列顺序必须与问题声明的投影一致（逐行对拍）。`IS NOT NULL` 与规划器的口径相同，
/// 但**这条 SQL 不经过门户的规划器** —— 那正是它作为「真值」的意义。
const TRUTH_SQL: &[(&str, &str)] = &[
    (
        "charges_by_currency",
        "SELECT currency::text, sum(deduction_amount)::text FROM deductions \
         WHERE customer_id = $1 AND currency IS NOT NULL AND deduction_amount IS NOT NULL \
         GROUP BY currency",
    ),
    (
        "my_balance",
        "SELECT recharge_balance::text FROM balances \
         WHERE customer_id = $1 AND recharge_balance IS NOT NULL",
    ),
    (
        "usage_by_model",
        "SELECT model_name::text, sum(usage_amount)::text FROM usage_details \
         WHERE customer_id = $1 AND model_name IS NOT NULL AND usage_amount IS NOT NULL \
         GROUP BY model_name",
    ),
    (
        "my_subscriptions",
        "SELECT status::text, billing_cycle::text, expire_at::text FROM customer_subscriptions \
         WHERE customer_id = $1 AND status IS NOT NULL AND billing_cycle IS NOT NULL \
           AND expire_at IS NOT NULL",
    ),
    (
        "my_api_keys",
        "SELECT name::text FROM licenses WHERE customer_id = $1 AND name IS NOT NULL",
    ),
];

/// 用直接 SQL 算出某租户在某条问题上的**真值**（只读；走服务自己的只读池）。
async fn truth_rows(
    pool: &sqlx::postgres::PgPool,
    qid: &str,
    tenant: &str,
) -> Vec<Vec<String>> {
    let (_, sql) = TRUTH_SQL
        .iter()
        .find(|(id, _)| *id == qid)
        .unwrap_or_else(|| panic!("没有为 {qid} 登记真值 SQL —— 新增问题必须同时登记"));
    let rows = sqlx::query(sql)
        .bind(tenant)
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| panic!("真值 SQL 跑不动（{qid}）：{e}"));
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        use sqlx::Row as _;
        let mut cells = Vec::new();
        for i in 0..row.len() {
            let v: Option<String> = row.try_get(i).unwrap_or(None);
            cells.push(v.unwrap_or_default());
        }
        out.push(cells);
    }
    out
}

/// 单元格归一化：能当数字看的按数字比（`0.00011800` 与 `0.000118` 是同一个数），
/// 其余按字符串比（时间戳 / 币种 / 模型名）。
fn norm_cell(raw: &str) -> String {
    let t = raw.trim();
    match t.parse::<f64>() {
        Ok(v) if v == 0.0 => "0".to_string(),
        Ok(v) => format!("{v:.12e}"),
        Err(_) => t.to_string(),
    }
}

fn multiset(rows: &[Vec<String>]) -> std::collections::BTreeMap<Vec<String>, usize> {
    let mut m = std::collections::BTreeMap::new();
    for r in rows {
        *m.entry(r.iter().map(|c| norm_cell(c)).collect()).or_insert(0) += 1;
    }
    m
}

/// **判据本体**：回答能不能由本租户自己的数据解释？
///
/// - 每一行的出现次数 ≤ 本租户真值里的次数（多重集包含）；
/// - 未截断时还要求**相等**（少行 = 查漏了，也是错）。
pub fn explains_tenant(
    answer: &[Vec<String>],
    truth: &[Vec<String>],
    truncated: bool,
) -> Result<(), String> {
    let got = multiset(answer);
    let want = multiset(truth);
    let mut foreign: Vec<String> = Vec::new();
    for (row, n) in &got {
        let own = want.get(row).copied().unwrap_or(0);
        if *n > own {
            foreign.push(format!(
                "{}（回答里 {} 行，本租户真值只有 {} 行）",
                row.join(" | "),
                n,
                own
            ));
        }
    }
    if !foreign.is_empty() {
        return Err(format!(
            "回答里有**不属于本租户**的行（无法由该租户自己的记录解释）：\n  - {}",
            foreign.join("\n  - ")
        ));
    }
    if !truncated {
        let mut missing: Vec<String> = Vec::new();
        for (row, n) in &want {
            let seen = got.get(row).copied().unwrap_or(0);
            if seen < *n {
                missing.push(format!("{}（真值 {} 行，回答里 {} 行）", row.join(" | "), n, seen));
            }
        }
        if !missing.is_empty() {
            return Err(format!(
                "回答**少了**本租户的数据（未截断却没给全）：\n  - {}",
                missing.join("\n  - ")
            ));
        }
    }
    Ok(())
}

/// 回答的 `rows` + `truncated`。
fn rows_and_truncated(body: &str) -> (Vec<Vec<String>>, bool) {
    let v: Value = serde_json::from_str(body).expect("回答是 JSON");
    let truncated = v["truncated"].as_bool().unwrap_or(false);
    let mut out = Vec::new();
    if let Some(rows) = v["rows"].as_array() {
        for r in rows {
            let cells: Vec<String> = r
                .as_array()
                .map(|cs| {
                    cs.iter()
                        .map(|c| {
                            // 门户把 null 渲染成 `null`（JSON），测试里当空串处理。
                            match c {
                                Value::Null => String::new(),
                                other => other.as_str().unwrap_or("").to_string(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            out.push(cells);
        }
    }
    (out, truncated)
}

fn tenants(test: &str) -> Option<(String, String)> {
    let a = env("ONTOK_REQ126_TENANT_A").or_else(|| {
        missing("ONTOK_REQ126_TENANT_A", test);
        None
    })?;
    let b = env("ONTOK_REQ126_TENANT_B").or_else(|| {
        missing("ONTOK_REQ126_TENANT_B", test);
        None
    })?;
    Some((a, b))
}

// ══════════════════════════════════════════════════════════════
// AC-001 / AC-004 · 两个真实租户对拍
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac001_two_real_tenants_see_disjoint_data_and_no_internals() {
    let Some((tenant_a, tenant_b)) = tenants("ac001_two_real_tenants") else {
        return;
    };
    let questions = common::crate_root().join("portal-questions.json");
    if !questions.is_file() {
        missing("portal-questions.json", "ac001_two_real_tenants");
        return;
    }
    let Some(l) = live("disjoint", &tenant_a, &tenant_b, &questions, 200).await else {
        return;
    };
    let ids = l.svc.questions.ids().iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(ids.len(), 5, "交付物是 5 条问题：{ids:?}");

    let mut nonempty_pairs = 0usize;
    for id in &ids {
        let a = call(
            ontok::portal::http::build_router(Arc::clone(&l.svc)),
            post_q(&format!("/q/{id}"), TOKEN_A),
        )
        .await;
        let b = call(
            ontok::portal::http::build_router(Arc::clone(&l.svc)),
            post_q(&format!("/q/{id}"), TOKEN_B),
        )
        .await;
        assert_eq!(a.status, StatusCode::OK, "{id} (A)：{}", a.body);
        assert_eq!(b.status, StatusCode::OK, "{id} (B)：{}", b.body);

        // ⛔ 回答里不许出现 SQL / 表名 / 列名 / 属性 IRI / 项目名。
        //
        // ⚠️ 判据只看**结构性字段**（列名 / 对象 / 警告 / 问题说明 / limits）；
        // ⛔ 不看 `rows` 的**值**：客户的业务数据里出现 "anspire" 这种字面量
        // （例如某个 API Key 的名字就叫 `anspire-llm=gateway-key`）不是泄漏 ——
        // 那是他自己的数据。身份泄漏（实例 IRI）另外判，见下面。
        for (label, body) in [("A", &a.body), ("B", &b.body)] {
            let mut v: Value = serde_json::from_str(body).unwrap();
            let rows = v["rows"].clone();
            if let Some(obj) = v.as_object_mut() {
                obj.remove("rows");
            }
            let meta = v.to_string().to_lowercase();
            for forbidden in [
                "sql_echo", "select", " from ", "t0.", "https://", "tenant_applied",
                "deductions", "balances", "usage_details", "customer_subscriptions",
                "licenses", "customer_id", "main_customer_id", "anspire",
            ] {
                assert!(
                    !meta.contains(forbidden),
                    "{id} ({label}) 的结构字段里出现了 {forbidden:?}：{body}"
                );
            }
            // 结果值里不许出现本体 IRI（那是身份，不是业务数据）。
            let cell_text = rows.to_string();
            assert!(
                !cell_text.contains("https://") && !cell_text.contains("ontok.local"),
                "{id} ({label}) 的结果值里有 IRI：{rows}"
            );
            // 每条回答都必须解释「涉及哪些业务对象」（AC-004-1）。
            assert!(
                v["objects"].as_array().is_some_and(|o| !o.is_empty()),
                "{id} ({label}) 没有 objects：{body}"
            );
            for o in v["objects"].as_array().unwrap() {
                assert!(o["name"].as_str().is_some_and(|n| !n.is_empty()), "{body}");
                assert!(
                    !o["name"].as_str().unwrap().contains("_"),
                    "业务名里出现了下划线（像是表名）：{body}"
                );
            }
        }

        // ⭐ N-1：**真值对拍**（不是「两个租户行值交集」—— 那会在「两边都为 0」时假红）。
        let pool = l.svc.source.clone();
        let truth_a = truth_rows(&pool, id, &tenant_a).await;
        let truth_b = truth_rows(&pool, id, &tenant_b).await;
        let (ra, ta) = rows_and_truncated(&a.body);
        let (rb, tb) = rows_and_truncated(&b.body);
        explains_tenant(&ra, &truth_a, ta).unwrap_or_else(|e| {
            panic!("{id} (A) 的回答无法由 A 自己的数据解释：{e}\n回答={ra:?}\n真值={truth_a:?}")
        });
        explains_tenant(&rb, &truth_b, tb).unwrap_or_else(|e| {
            panic!("{id} (B) 的回答无法由 B 自己的数据解释：{e}\n回答={rb:?}\n真值={truth_b:?}")
        });
        if !ra.is_empty() && !rb.is_empty() {
            nonempty_pairs += 1;
        }
        eprintln!(
            "· {id}: A {} 行（真值 {} 行）/ B {} 行（真值 {} 行）/ 各自 ⊆ 自己的真值 ✔",
            ra.len(),
            truth_a.len(),
            rb.len(),
            truth_b.len()
        );
    }
    // 至少有一条问题**两边都有数据**（否则上面的对拍是空转）。
    assert!(
        nonempty_pairs >= 1,
        "两个租户没有任何一条问题同时有数据 —— 这次对拍证明不了什么"
    );

    // AC-009：审计行数与执行次数一致，且无口令明文、无 SQL。
    let log = std::fs::read_to_string(&l.audit).unwrap_or_default();
    assert_eq!(
        log.lines().filter(|l| l.contains("code=OK")).count(),
        ids.len() * 2,
        "审计行数应当 = 执行次数：\n{log}"
    );
    for line in log.lines() {
        assert!(!line.contains(TOKEN_A) && !line.contains(TOKEN_B), "{line}");
        assert!(!line.contains("SELECT") && !line.contains("FROM "), "{line}");
    }
}

/// ⭐ N-1 的反向证据：**真泄漏必须被抓**。
///
/// 「把某条问题的 tenant 写死成别人」在外部**可观测**的形态，就是「A 的口令拿到
/// B 的数据」—— 所以这里把口令表换绑（`TOKEN_A → tenant_b`）来构造同一形态，
/// 再用 A 的真值去判：必须**红**。
///
/// 反向自证（防「对谁都红」）：同一份回答拿去对 **B 的真值**必须**绿**。
#[tokio::test]
async fn n1_a_hardcoded_tenant_is_caught() {
    let Some((tenant_a, tenant_b)) = tenants("n1_mutant") else {
        return;
    };
    let questions = common::crate_root().join("portal-questions.json");
    if !questions.is_file() {
        missing("portal-questions.json", "n1_mutant");
        return;
    }
    // ⚠️ 参数顺序**故意反着传**：TOKEN_A 现在绑的是 tenant_b。
    let Some(l) = live("mutant", &tenant_b, &tenant_a, &questions, 200).await else {
        return;
    };
    let pool = l.svc.source.clone();
    let truth_a = truth_rows(&pool, "my_balance", &tenant_a).await;
    let truth_b = truth_rows(&pool, "my_balance", &tenant_b).await;
    if truth_a == truth_b {
        // 两个租户的余额恰好相同 ⇒ 从回答里**不可能**分辨（投影里没有身份列）。
        // 这不是判据的错，是「值级证据」的固有上限 —— 响亮地说，不假装通过。
        eprintln!(
            "*** SKIP: n1 的突变判据 -- 两个租户的余额真值相同（{truth_a:?}），\
             值级判据无法分辨 ***"
        );
        return;
    }

    let r = call(
        ontok::portal::http::build_router(Arc::clone(&l.svc)),
        post_q("/q/my_balance", TOKEN_A),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (rows, truncated) = rows_and_truncated(&r.body);
    let verdict = explains_tenant(&rows, &truth_a, truncated);
    assert!(
        verdict.is_err(),
        "⛔ 写死成别人的租户没有被抓到：回答={rows:?}，A 的真值={truth_a:?}"
    );
    let msg = verdict.unwrap_err();
    assert!(msg.contains("不属于本租户"), "错误话术要说得清：{msg}");
    eprintln!("· 突变被抓：{msg}");
    // 反向自证：同一份回答对 **B** 的真值能解释（否则这条判据是「对谁都红」）。
    assert!(
        explains_tenant(&rows, &truth_b, truncated).is_ok(),
        "同一份回答对 B 的真值也解释不了 ⇒ 判据是空转的（对谁都红）"
    );
}

// ══════════════════════════════════════════════════════════════
// N-1 · 判据本体的单测（不连库；把「假红」与「真泄漏」都钉住）
// ══════════════════════════════════════════════════════════════

fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
    cells
        .iter()
        .map(|r| r.iter().map(|c| c.to_string()).collect())
        .collect()
}

/// ⭐ QA 的假红案例：两个租户的余额**都是 0.00000000** ⇒ 必须**通过**。
#[test]
fn n1_the_judge_does_not_false_red_when_both_tenants_are_zero() {
    let answer_a = rows(&[&["0.00000000"]]);
    let truth_a = rows(&[&["0.00000000"]]);
    let truth_b = rows(&[&["0.00000000"]]);
    assert!(
        explains_tenant(&answer_a, &truth_a, false).is_ok(),
        "两边都为 0 时，值级交集非空 —— 旧判据会假红，新判据必须通过"
    );
    // 旧判据的形态（值交集）对同一组数据是「非空」—— 留证：那正是被替换掉的写法。
    assert!(answer_a.iter().any(|r| truth_b.contains(r)));
}

/// ⭐ 真泄漏：回答里有一行是**别人**的 ⇒ 必须红，并点名那一行。
#[test]
fn n1_a_foreign_row_is_caught() {
    let answer = rows(&[&["85.95334115"]]);
    let truth = rows(&[&["191.06910719"]]);
    let e = explains_tenant(&answer, &truth, false).unwrap_err();
    assert!(e.contains("不属于本租户"), "{e}");
    // 判决里要能看见**那一行**（数字按归一化后的写法打印，便于比对）。
    assert!(e.contains(&norm_cell("85.95334115")), "{e}");
}

/// 多出来的一行（本租户只有一行，回答给了两行）⇒ 也必须红（多重集计数）。
#[test]
fn n1_a_leaked_extra_row_is_caught_even_when_the_value_exists() {
    let answer = rows(&[&["0"], &["0"]]);
    let truth = rows(&[&["0"]]);
    let e = explains_tenant(&answer, &truth, false).unwrap_err();
    assert!(e.contains("不属于本租户"), "{e}");
}

/// 截断时允许是**前缀**（少行不算错），但⛔ 仍不许有别人的行。
#[test]
fn n1_a_truncated_answer_may_be_a_prefix_but_never_foreign() {
    let truth = rows(&[&["1.0"], &["2.0"], &["3.0"]]);
    assert!(explains_tenant(&rows(&[&["1.0"]]), &truth, true).is_ok());
    assert!(explains_tenant(&rows(&[&["9.0"]]), &truth, true).is_err());
    // 未截断就不许少。
    let e = explains_tenant(&rows(&[&["1.0"]]), &truth, false).unwrap_err();
    assert!(e.contains("少了"), "{e}");
}

/// 数字按**数值**比：`0.00011800` 与 `0.000118` 是同一个数。
#[test]
fn n1_numbers_are_compared_numerically_not_textually() {
    assert!(explains_tenant(&rows(&[&["0.00011800"]]), &rows(&[&["0.000118"]]), false).is_ok());
    assert!(explains_tenant(&rows(&[&["0"]]), &rows(&[&["0.00000000"]]), false).is_ok());
    // 文本列仍然逐字比。
    assert!(explains_tenant(&rows(&[&["RMB"]]), &rows(&[&["USD"]]), false).is_err());
}

/// ⭐ AC-004：查 `deductions` 那条（`main_only`）⇒ **覆盖警告可见**，且不含列名。
#[tokio::test]
async fn ac004_the_main_only_coverage_warning_is_visible() {
    let Some((tenant_a, tenant_b)) = tenants("ac004_coverage") else {
        return;
    };
    let questions = common::crate_root().join("portal-questions.json");
    if !questions.is_file() {
        missing("portal-questions.json", "ac004_coverage");
        return;
    }
    let Some(l) = live("coverage", &tenant_a, &tenant_b, &questions, 200).await else {
        return;
    };
    let r = call(
        ontok::portal::http::build_router(Arc::clone(&l.svc)),
        post_q("/q/charges_by_currency", TOKEN_A),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let v: Value = serde_json::from_str(&r.body).unwrap();
    let warnings = v["warnings"].as_array().cloned().unwrap_or_default();
    let text = warnings
        .iter()
        .filter_map(|w| w.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("只覆盖主账号"),
        "扣费这条必须显著提示覆盖范围：{text}"
    );
    assert!(text.contains("扣费明细"), "警告必须点名**业务对象**：{text}");
    assert!(
        !text.contains("main_customer_id") && !text.contains("customer_id"),
        "警告里漏出了列名：{text}"
    );
    assert_eq!(
        v["objects"][0]["coverage"], "main_only",
        "对象上的覆盖范围也要说清：{}",
        r.body
    );
}

// ══════════════════════════════════════════════════════════════
// AC-010 · 行数上限（截断必须可见）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac010_a_truncated_answer_says_so() {
    let Some((tenant_a, tenant_b)) = tenants("ac010_truncation") else {
        return;
    };
    // 一条**必然超过上限**的问题（只用于本地判据；不进交付的 5 条）。
    let dir = temp_dir("limitq");
    let q = dir.join("portal-questions.json");
    std::fs::write(
        &q,
        r#"{
          "version": "1.0.0",
          "reviewed_by": "真库判据@2026-10-23",
          "questions": [
            {"id": "my_usage_timeline",
             "title": "我的用量时间线",
             "sparql": "SELECT ?at WHERE { ?u <https://ontok.local/anspire_r2rml_excl#usage_details__usage_at> ?at . }",
             "note": "按调用时间逐条列出。"}
          ]
        }"#,
    )
    .expect("写问题声明");
    let Some(l) = live("truncation", &tenant_a, &tenant_b, &q, 5).await else {
        return;
    };
    // B 的口令上限是 5（A 是 200）—— 用 B 保证截断。
    let r = call(
        ontok::portal::http::build_router(Arc::clone(&l.svc)),
        post_q("/q/my_usage_timeline", TOKEN_B),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let v: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["limits"]["max_rows"], 5, "上限必须来自口令：{}", r.body);
    let rows = v["rows"].as_array().cloned().unwrap_or_default();
    if !rows.is_empty() && v["truncated"] == Value::Bool(true) {
        assert_eq!(rows.len(), 5, "截断时只显示前 5 行");
        let text = v["warnings"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("只显示前 5 行"), "截断必须有可见提示：{text}");
        eprintln!("· 截断实测：rows=5 truncated=true warning={text}");
    } else {
        // 这个库的 B 租户没有够多的用量明细 ⇒ 判据不成立，**响亮地**说，但不算失败
        // （截断的另一半由 `portal::answer_of` 的单测钉住：truncated ⇒ 有警告）。
        eprintln!(
            "*** SKIP: ac010 的截断实测 -- 这个库的租户 B 只有 {} 行用量明细（< 5）***",
            rows.len()
        );
    }
}

// ══════════════════════════════════════════════════════════════
// AC-010 · 并发闸（真库：N+1 ⇒ N 个 200 + 1 个 429；按口令隔离）
// ══════════════════════════════════════════════════════════════

/// ⭐ 同一口令并发 N+1 ⇒ **恰好 N 个 200 + 1 个 429**。
///
/// 为什么这是确定性的（不是碰运气）：`tokio::join!` 把三个 future 放在**同一个任务**
/// 里轮转，每个 future 至少会被 poll 一次；而名额是在 `run_question` 的**第一个
/// 同步段**里拿的（早于任何 `await`）—— 于是第 3 个 future 第一次被 poll 时，
/// 前两个已经在跑（真实查询 ~150 ms，远长于一次轮转）。
#[tokio::test]
async fn ac010_three_concurrent_requests_on_a_cap_2_token_are_2x200_plus_1x429() {
    let Some((tenant_a, tenant_b)) = tenants("ac010_concurrent") else {
        return;
    };
    let questions = common::crate_root().join("portal-questions.json");
    if !questions.is_file() {
        missing("portal-questions.json", "ac010_concurrent");
        return;
    }
    let Some(l) = live("concurrent", &tenant_a, &tenant_b, &questions, 200).await else {
        return;
    };
    let svc = Arc::clone(&l.svc);
    let app = || ontok::portal::http::build_router(Arc::clone(&svc));
    let r1 = call(app(), post_q("/q/my_balance", TOKEN_A));
    let r2 = call(app(), post_q("/q/my_balance", TOKEN_A));
    let r3 = call(app(), post_q("/q/my_balance", TOKEN_A));
    let (a, b, c) = tokio::join!(r1, r2, r3);
    let mut ok = 0;
    let mut too_many = 0;
    for r in [&a, &b, &c] {
        match r.status {
            StatusCode::OK => ok += 1,
            StatusCode::TOO_MANY_REQUESTS => too_many += 1,
            other => panic!("意外状态 {other}：{}", r.body),
        }
    }
    assert_eq!(
        (ok, too_many),
        (2, 1),
        "A 的上限是 2 ⇒ 3 个并发必须恰好 2 个 200 + 1 个 429\
         （a={:?} b={:?} c={:?}）",
        a.status,
        b.status,
        c.status
    );
    // 429 的正文是标准错误契约，且不含内部标识。
    let rejected = [&a, &b, &c]
        .into_iter()
        .find(|r| r.status == StatusCode::TOO_MANY_REQUESTS)
        .unwrap();
    let v: Value = serde_json::from_str(&rejected.body).unwrap();
    assert_eq!(v["code"], "TOO_MANY_CONCURRENT");
    assert!(!rejected.body.contains("balances") && !rejected.body.contains("SELECT"));
    // 审计：429 也留一行。
    let log = std::fs::read_to_string(&l.audit).unwrap_or_default();
    assert!(
        log.lines().filter(|l| l.contains("code=TOO_MANY_CONCURRENT")).count() >= 1,
        "审计里没有 429 行：\n{log}"
    );
    assert_eq!(svc.limiter.inflight(&svc.tokens.find(TOKEN_A).unwrap().digest), 0);
    eprintln!("· 并发实测：A(cap=2) 三连击 ⇒ 200×{ok} / 429×{too_many}");
}

/// ⭐ A 打满时 **B 照样能取数**（名额按口令隔离，真库）。
#[tokio::test]
async fn ac010_a_saturated_token_does_not_starve_another_token_on_real_data() {
    let Some((tenant_a, tenant_b)) = tenants("ac010_isolation") else {
        return;
    };
    let questions = common::crate_root().join("portal-questions.json");
    if !questions.is_file() {
        missing("portal-questions.json", "ac010_isolation");
        return;
    }
    let Some(l) = live("iso", &tenant_a, &tenant_b, &questions, 200).await else {
        return;
    };
    // 占满 A（cap=2）。
    let a = l.svc.tokens.find(TOKEN_A).unwrap().digest.clone();
    let limit = l.svc.tokens.find(TOKEN_A).unwrap().max_concurrency;
    let p1 = l.svc.limiter.acquire(&a, limit).expect("A 名额 1");
    let p2 = l.svc.limiter.acquire(&a, limit).expect("A 名额 2");

    let ra = call(
        ontok::portal::http::build_router(Arc::clone(&l.svc)),
        post_q("/q/my_balance", TOKEN_A),
    )
    .await;
    assert_eq!(ra.status, StatusCode::TOO_MANY_REQUESTS, "A 应当 429：{}", ra.body);

    let rb = call(
        ontok::portal::http::build_router(Arc::clone(&l.svc)),
        post_q("/q/my_balance", TOKEN_B),
    )
    .await;
    assert_eq!(rb.status, StatusCode::OK, "B 不该被 A 拖住：{}", rb.body);
    let vb: Value = serde_json::from_str(&rb.body).unwrap();
    assert!(
        vb["objects"].as_array().is_some_and(|o| !o.is_empty()),
        "B 的回答仍然要解释对象：{}",
        rb.body
    );

    drop(p1);
    drop(p2);
    let ra2 = call(
        ontok::portal::http::build_router(Arc::clone(&l.svc)),
        post_q("/q/my_balance", TOKEN_A),
    )
    .await;
    assert_eq!(ra2.status, StatusCode::OK, "名额还回去后 A 应当能取数：{}", ra2.body);
    eprintln!("· 隔离实测：A 占满 ⇒ A 429 / B 200 / A 释放后 200");
}
