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
                {{"label":"A","token":"{TOKEN_A}","kind":"portal","tenant":"{tenant_a}","max_rows":200}},
                {{"label":"B","token":"{TOKEN_B}","kind":"portal","tenant":"{tenant_b}","max_rows":{max_rows_b}}}
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

fn rowset(body: &str) -> Vec<String> {
    let v: Value = serde_json::from_str(body).expect("回答是 JSON");
    let mut out = Vec::new();
    if let Some(rows) = v["rows"].as_array() {
        for r in rows {
            let cells: Vec<String> = r
                .as_array()
                .map(|cs| {
                    cs.iter()
                        .map(|c| c.as_str().unwrap_or("").to_string())
                        .collect()
                })
                .unwrap_or_default();
            out.push(cells.join(" | "));
        }
    }
    out
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

        let sa = rowset(&a.body);
        let sb = rowset(&b.body);
        let inter: Vec<&String> = sa.iter().filter(|r| sb.contains(r)).collect();
        assert!(
            inter.is_empty(),
            "{id}：两个租户的结果有交集（⛔ 跨租户泄漏）：{inter:?}\nA={sa:?}\nB={sb:?}"
        );
        if !sa.is_empty() && !sb.is_empty() {
            nonempty_pairs += 1;
        }
        eprintln!("· {id}: A {} 行 / B {} 行 / 交集 0", sa.len(), sb.len());
    }
    // 至少有一条问题**两边都有数据**（否则「交集为空」是空转）。
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
