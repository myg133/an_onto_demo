//! REQ-111 · 本地审核界面（`ontok review serve`）—— AC-001 ~ AC-010 的判据。
//!
//! # 为什么行为判据在**真路由**上打
//!
//! 这些判据要的不是「函数返回了什么」，而是「HTTP 面上到底能不能签、签了谁看得见」。
//! 所以一律经 [`build_router`] + `Router::oneshot` 打真实请求 ——
//! 与 `src/server/http.rs` 的 `#[cfg(test)]` 同一个做法（405/401 都能在进程内断言）。
//!
//! # 生产接线与测试接线**同一份**
//!
//! [`prepare`] 是 `serve()` 启动时调的那个函数（解析项目 / 提案 / 基版本 + 三道闸门），
//! 测试**不另搭**一份 `ReviewState`。把闸门放回 `prepare`，这里立刻红。
//!
//! # 为什么两条判据走**真二进制**（`CARGO_BIN_EXE_ontok`）
//!
//! AC-004 / AC-009 说的是「页面签 ⇒ CLI 读得到 / CLI 签 ⇒ 页面读得到」。
//! 在进程内调 `decide_item` 只能证明「同一个函数被调了两次」，证明不了
//! **`review show` 那个真实进程**读得出来。所以这两条 spawn `ontok` 本体，
//! 用它的 stdout / 数据库落点说话。

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::proposal::{NewItem, NewProposal, PgProposalStore, ProposalItemRow};
use ontok::db::snapshot_store::PgStore;
use ontok::domain::review::ItemKind;
use ontok::domain::snapshot::Snapshot;
use ontok::review_ui::http::{build_router, prepare};
use ontok::review_ui::ReviewArgs;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "correct-horse-battery";

// ══════════════════════════════════════════════════════════════
// 夹具：项目 + 未冻结 v1 + 快照 + 一份提案
// ══════════════════════════════════════════════════════════════

struct Fixture {
    pool: PgPool,
    project: String,
    version_id: Uuid,
    proposal_id: Uuid,
}

/// 建一个**全新**项目（撞名响亮失败），v1 + 空快照 + 一份 n 条目的提案。
///
/// 条目构造刻意包含三类：
/// - 实体（无置信度）；
/// - 关系（带 `confidence.score`，供批量档位筛选）；
/// - 另一条实体（供 `--item 2 --track model` 这种单轨判据用）。
async fn fixture(prefix: &str, items: Vec<NewItem>) -> Fixture {
    let pool = test_pool().await;
    common::migrate(&pool).await;

    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let proposals = PgProposalStore::new(pool.clone());

    let p = common::fresh_project(&projects, prefix).await;
    let v1 = projects.new_version(p.id).await.unwrap();
    store.save_snapshot(&Snapshot::new(v1.id, 1)).await.unwrap();

    let prop = proposals
        .create(
            p.id,
            NewProposal { base_version_id: v1.id, round: 1, extracted_by: None },
            &items,
        )
        .await
        .unwrap();

    Fixture { pool, project: p.name, version_id: v1.id, proposal_id: prop.id }
}

fn entity(key: &str, name: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddEntityType,
        target_key: name.to_string(),
        payload: json!({ "key": key, "display_name": name, "properties": [] }),
    }
}

/// 一份**可入库**的置信度（v2：语义项 + AI 溯源成对出现，见 `domain::confidence` 的 A-10 / S-1）。
///
/// ⚠️ 不能随手写 `{"score":0.95}`：`NewItem::confidence` 会拒绝「有分数没证据链」。
/// 这里给的是六个信号全 1.0 的满分证据链，`score` 只改声明值（校验只要求
/// `score ≤ veto 上限`，不要求等于加权和）。
fn confidence(score: f64, band: &str) -> Value {
    json!({
        "score": score,
        "band": band,
        "method": "evidence-scorecard-v2",
        "signals": [
            {"name": "sample_hit_rate", "value": 1.0, "weight": 0.3825, "sampled": 5, "matched": 5},
            {"name": "name_match", "value": 1.0, "weight": 0.17},
            {"name": "not_all_null", "value": 1.0, "weight": 0.1275},
            {"name": "type_compatible", "value": 1.0, "weight": 0.085},
            {"name": "target_is_key", "value": 1.0, "weight": 0.085},
            {"name": "semantic_fit", "value": 1.0, "weight": 0.15}
        ],
        "data_evidence": "sampled",
        "ai": {
            "model": "fake-1",
            "prompt_sha": "deadbeef",
            "tokens": 7,
            "cache_hit": false,
            "rationale": "两端语义匹配"
        }
    })
}

fn relation(from: &str, to: &str, name: &str, score: f64, band: &str) -> NewItem {
    NewItem {
        kind: ItemKind::AddLinkType,
        target_key: name.to_string(),
        payload: json!({
            "key": format!("{from}__{to}"),
            "from_type": from,
            "to_type": to,
            "cardinality": "N:1",
            "semantic": "belongs_to",
            "evidence": [{"table": "t", "column": "c", "claim": "按命名约定推断"}],
            "confidence": confidence(score, band),
        }),
    }
}

fn args(f: &Fixture) -> ReviewArgs {
    ReviewArgs {
        project: f.project.clone(),
        proposal: f.proposal_id.to_string(),
        bind: "127.0.0.1:0".parse().unwrap(),
        token: TOKEN.to_string(),
    }
}

async fn app(f: &Fixture) -> Router {
    let state = prepare(&f.pool, &args(f)).await.expect("夹具应当通过启动闸门");
    build_router(state)
}

/// 打一发请求。`body` 有值 ⇒ POST + JSON。
async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, String) {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&v).unwrap()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = app.clone().oneshot(req).await.expect("oneshot 的错误类型是 Infallible");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 22).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn body_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("不是合法 JSON：{e}\n{s}"))
}

async fn rows(f: &Fixture) -> Vec<ProposalItemRow> {
    PgProposalStore::new(f.pool.clone())
        .items(f.proposal_id)
        .await
        .unwrap()
}

fn item_row<'a>(rows: &'a [ProposalItemRow], seq: i32) -> &'a ProposalItemRow {
    rows.iter().find(|r| r.seq == seq).expect("条目必须存在")
}

/// 真二进制跑一条 CLI 命令，返回 (退出码, stdout, stderr)。
fn run_bin(bin: &str, args: &[&str]) -> (i32, String, String) {
    let url = std::env::var("ONTOK_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("ONTOK_DATABASE_URL"))
        .expect("测试需要 ONTOK_TEST_DATABASE_URL（见 .env.example）");
    let out = std::process::Command::new(bin)
        .args(args)
        .env("ONTOK_DATABASE_URL", url)
        .output()
        .unwrap_or_else(|e| panic!("跑不起 {bin}：{e}"));
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 只绑回环
// ══════════════════════════════════════════════════════════════

/// ⛔ 非回环 ⇒ **拒绝启动**（`serve` 同一条闸，理由更重：这个进程会写）。
#[tokio::test]
async fn ac001_non_loopback_bind_is_refused() {
    let f = fixture("req111_loopback", vec![entity("customer", "客户")]).await;

    for bad in ["0.0.0.0:8788", "[::]:8788"] {
        let mut a = args(&f);
        a.bind = bad.parse().unwrap();
        let err = ontok::review_ui::http::serve(&f.pool, a)
            .await
            .expect_err("非回环地址必须拒绝启动");
        let msg = err.to_string();
        assert!(msg.contains("回环"), "文案要说清只允许回环：{msg}");
        assert!(msg.contains("127.0.0.1"), "要给出可照抄的例子：{msg}");
    }

    // 对照组（纪律 E）：回环地址必须**过得去**这道闸 —— 否则上面两条只是「凡绑必拒」。
    let state = prepare(&f.pool, &args(&f))
        .await
        .expect("回环地址应当通过启动闸门");
    // 路由能真的搭起来（不需要真绑端口）。
    let _ = build_router(state);
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 只服务未冻结提案
// ══════════════════════════════════════════════════════════════

/// 基版本被冻结 ⇒ `prepare` 直接失败，且文案点名 `v1 已冻结`。
#[tokio::test]
async fn ac002_frozen_base_version_refuses_to_start() {
    let f = fixture("req111_frozen", vec![entity("customer", "客户")]).await;
    PgProjectStore::new(f.pool.clone())
        .freeze(f.version_id)
        .await
        .unwrap();

    let err = prepare(&f.pool, &args(&f))
        .await
        .err()
        .expect("已冻结版本的提案不许开审核界面");
    let msg = err.to_string();
    assert!(msg.contains("已冻结"), "文案必须点名已冻结：{msg}");
    assert!(msg.contains("v1"), "文案必须点名是哪一版：{msg}");
    assert_eq!(msg.contains(&f.project), true, "文案要带项目名：{msg}");
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 两个接口都必须鉴权
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac003_both_apis_require_a_token() {
    let f = fixture("req111_auth", vec![entity("customer", "客户")]).await;
    let app = app(&f).await;

    // ① 读：无 token / 错 token ⇒ 401；对 token ⇒ 200。
    assert_eq!(
        call(&app, "GET", "/api/review", None, None).await.0,
        StatusCode::UNAUTHORIZED,
        "GET /api/review 无口令必须 401"
    );
    assert_eq!(
        call(&app, "GET", "/api/review", Some("nope"), None).await.0,
        StatusCode::UNAUTHORIZED,
    );
    let (s, b) = call(&app, "GET", "/api/review", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK, "带正确口令必须 200：{b}");

    // ② 写：无 token ⇒ 401，且**不许**落库。
    let (s, b) = call(
        &app,
        "POST",
        "/api/decide",
        None,
        Some(json!({"item": 1, "track": "biz", "decision": "accept"})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "POST /api/decide 无口令必须 401");
    assert!(!b.contains(TOKEN), "401 正文不许回显口令：{b}");
    assert_eq!(
        item_row(&rows(&f).await, 1).biz_status,
        "pending",
        "401 之后不许有任何写入"
    );

    // ③ 页面壳：免口令可打开（浏览器导航带不了 Authorization），但**不含数据**。
    let (s, html) = call(&app, "GET", "/", None, None).await;
    assert_eq!(s, StatusCode::OK, "页面壳必须免口令可打开");
    assert!(!html.contains(&f.project), "壳里不许出现项目名");
    assert!(!html.contains("客户"), "壳里不许出现条目名");
    assert!(!html.contains(TOKEN), "壳里不许出现口令");
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 双向一致（页面签 ⇒ CLI 读得到；CLI 签 ⇒ 页面读得到）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac004_page_signs_then_the_cli_reads_it_and_back() {
    let f = fixture(
        "req111_two_way",
        vec![entity("customer", "客户"), entity("order", "订单")],
    )
    .await;
    let app = app(&f).await;

    // ── 方向 ①：页面签 ⇒ CLI 读得到 ──
    let (s, b) = call(
        &app,
        "POST",
        "/api/decide",
        Some(TOKEN),
        Some(json!({"item": 1, "track": "biz", "decision": "accept"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "页面签一条必须成功：{b}");
    assert_eq!(body_json(&b)["biz"], json!("accepted"));

    let (code, stdout, stderr) = run_bin(
        env!("CARGO_BIN_EXE_ontok"),
        &["review", "show", "--proposal", &f.proposal_id.to_string(), "--project", &f.project],
    );
    assert_eq!(code, 0, "review show 应当成功：{stderr}");
    assert!(
        stdout.contains("biz=accepted"),
        "页面上签的 biz 决定，`review show` 必须读得到：\n{stdout}"
    );

    // ── 方向 ②：CLI 签 ⇒ 页面读得到 ──
    let (code, stdout, stderr) = run_bin(
        env!("CARGO_BIN_EXE_ontok"),
        &[
            "review", "decide", "--proposal", &f.proposal_id.to_string(),
            "--project", &f.project, "--item", "2", "--track", "model", "--accept",
        ],
    );
    assert_eq!(code, 0, "CLI decide 应当成功：{stderr}\n{stdout}");

    let (s, b) = call(&app, "GET", "/api/review", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    let v = body_json(&b);
    let second = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|it| it["item"] == json!(2))
        .expect("条目 2 必须在列表里");
    assert_eq!(
        second["model"], json!("accepted"),
        "CLI 签的 model 决定，页面必须读得到：{second}"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 幂等覆盖
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac005_signing_twice_overwrites_and_keeps_one_row() {
    let f = fixture("req111_idem", vec![entity("customer", "客户")]).await;
    let app = app(&f).await;

    for d in ["accept", "reject"] {
        let (s, b) = call(
            &app,
            "POST",
            "/api/decide",
            Some(TOKEN),
            Some(json!({"item": 1, "track": "biz", "decision": d})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "连签不该报错（{d}）：{b}");
    }

    let rows = rows(&f).await;
    assert_eq!(rows.len(), 1, "同 item+track 连签**不许**产生第二条记录");
    assert_eq!(item_row(&rows, 1).biz_status, "rejected", "终态必须是最后一次 reject");
    assert_eq!(
        item_row(&rows, 1).biz_via.as_deref(),
        Some("ui"),
        "审计要记页面入口"
    );
    assert!(
        item_row(&rows, 1).biz_decided_at.is_some(),
        "审计要记 decided_at"
    );

    // `review show` 也只反映最后一次。
    let (code, stdout, _) = run_bin(
        env!("CARGO_BIN_EXE_ontok"),
        &["review", "show", "--proposal", &f.proposal_id.to_string(), "--project", &f.project],
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("biz=rejected"), "终态应是 rejected：\n{stdout}");
    assert!(!stdout.contains("biz=accepted"), "旧决定不许残留：\n{stdout}");
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 批量签必须先预览条数（且有 items[] 白名单）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac006_batch_preview_shows_the_count_and_the_whitelist() {
    // 10 条未签：3 条 ≥0.90、4 条 0.75–0.90、3 条无分数。
    let mut items = vec![entity("customer", "客户")];
    for i in 0..3 {
        items.push(relation("customer", "order", &format!("高{i}"), 0.95, "high"));
    }
    for i in 0..4 {
        items.push(relation("customer", "order", &format!("中{i}"), 0.80, "middle"));
    }
    items.push(entity("order", "订单"));
    items.push(entity("product", "产品"));
    items.push(entity("channel", "渠道"));
    let f = fixture("req111_batch", items).await;
    let app = app(&f).await;

    let (s, b) = call(
        &app,
        "GET",
        "/api/preview?track=biz&min_confidence=0.9&scope=unsigned",
        Some(TOKEN),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "预览必须成功：{b}");
    let v = body_json(&b);
    assert_eq!(v["count"], json!(3), "「将接受 3 条」—— 条数必须先算出来：{v}");
    let whitelist: Vec<i64> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_i64().unwrap())
        .collect();
    assert_eq!(whitelist.len(), 3, "白名单条数必须等于 count");
    assert_eq!(v["excluded"]["below_band"], json!(4), "低于档位的要单独报数：{v}");
    assert_eq!(v["excluded"]["no_confidence"], json!(4), "无分数的要单独报数：{v}");

    // 只读：预览**不许**改任何状态。
    assert!(rows(&f).await.iter().all(|r| r.biz_status == "pending" && r.model_status == "pending"));

    // 页面必须把这个数显示出来，且不含任何批量写接口。
    let html = ontok::review_ui::page::REVIEW_HTML;
    assert!(html.contains("将接受 "), "页面必须显示「将接受 N 条」：没找到");
    assert!(html.contains("api/preview"), "页面必须走预览接口");
    assert!(
        !html.contains("api/decide-bulk"),
        "⛔ 页面不许有批量写接口（服务端也没有：批量 = 逐条 POST）"
    );
    // 服务端确实没有批量写入口：未注册路径 ⇒ 405/404，不会落库。
    let (s, _) = call(
        &app,
        "POST",
        "/api/decide-bulk",
        Some(TOKEN),
        Some(json!({"items": [1, 2, 3], "track": "biz", "decision": "accept"})),
    )
    .await;
    assert!(
        s == StatusCode::METHOD_NOT_ALLOWED || s == StatusCode::NOT_FOUND,
        "批量写接口不该存在，实际 {s}"
    );
    // 参数错误也必须说话（不是静默空）。
    let (s, b) = call(&app, "GET", "/api/preview", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "缺 track 应当 400：{b}");
    assert!(b.contains("track"), "400 要说清楚缺什么：{b}");
}

// ══════════════════════════════════════════════════════════════
// AC-007 · 失败必须可见（运行中冻结）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac007_write_after_freeze_is_conflict_with_the_concrete_reason() {
    let f = fixture("req111_freeze_late", vec![entity("customer", "客户")]).await;
    let app = app(&f).await; // 先正常启动（此时未冻结）

    // ⭐ 服务运行期间版本被冻上 —— 启动校验已经过去了，写前校验必须拦住。
    PgProjectStore::new(f.pool.clone())
        .freeze(f.version_id)
        .await
        .unwrap();

    let (s, b) = call(
        &app,
        "POST",
        "/api/decide",
        Some(TOKEN),
        Some(json!({"item": 1, "track": "biz", "decision": "accept"})),
    )
    .await;
    assert!(!s.is_success(), "冻结后写决策必须失败，实际 {s}");
    assert_eq!(s, StatusCode::CONFLICT, "应当是 409（状态冲突）而不是 500");
    let v = body_json(&b);
    let why = v["error"].as_str().expect("错误正文必须带 error 字段");
    assert!(why.contains("已冻结"), "页面必须看得到具体原因：{why}");
    assert!(why.contains("v1"), "原因要点名是哪一版：{why}");
    assert!(
        why.contains(&f.project),
        "原因要点名是哪个项目：{why}"
    );
    // 且**真的没写**。
    assert_eq!(item_row(&rows(&f).await, 1).biz_status, "pending");
}

// ══════════════════════════════════════════════════════════════
// AC-008 · 与 serve 隔离（这个测试进程内能证的那一半）
// ══════════════════════════════════════════════════════════════

/// 审核界面的路由**只有**那一个写点；其余一律 405（与 `serve` 的 405 语义同形，
/// 但**是另一个 Router**）。「`serve` 一行未动」本身由 `git diff` 与
/// `test_graph_server.rs` / `http.rs` 的 405 判据证明，见验证报告。
#[tokio::test]
async fn ac008_review_router_has_exactly_one_write_method() {
    let f = fixture("req111_methods", vec![entity("customer", "客户")]).await;
    let app = app(&f).await;

    for m in ["PUT", "PATCH", "DELETE"] {
        let (s, _) = call(
            &app,
            m,
            "/api/decide",
            Some(TOKEN),
            Some(json!({"item": 1, "track": "biz", "decision": "accept"})),
        )
        .await;
        assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED, "{m} /api/decide 应当 405");
    }
    // GET 到写端点 = 404/405（方法不对，不是「读得到决策」）。
    let (s, _) = call(&app, "GET", "/api/decide", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED, "GET /api/decide 应当 405");
}

// ══════════════════════════════════════════════════════════════
// AC-009 · 审计可区分来源（cli / ui）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn ac009_audit_distinguishes_ui_from_cli() {
    let f = fixture(
        "req111_audit",
        vec![entity("customer", "客户"), entity("order", "订单")],
    )
    .await;
    let app = app(&f).await;

    // 页面签 item 1 / biz。
    let (s, _) = call(
        &app,
        "POST",
        "/api/decide",
        Some(TOKEN),
        Some(json!({"item": 1, "track": "biz", "decision": "accept"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // CLI 签 item 2 / model（真二进制，与用户敲的是同一条命令）。
    let (code, _, stderr) = run_bin(
        env!("CARGO_BIN_EXE_ontok"),
        &[
            "review", "decide", "--proposal", &f.proposal_id.to_string(),
            "--project", &f.project, "--item", "2", "--track", "model", "--accept",
        ],
    );
    assert_eq!(code, 0, "CLI decide 应当成功：{stderr}");

    let rows = rows(&f).await;
    let ui = item_row(&rows, 1);
    assert_eq!(ui.biz_via.as_deref(), Some("ui"), "页面签的必须记 ui");
    assert!(ui.biz_decided_at.is_some(), "ui 决策必须记 decided_at");

    let cli = item_row(&rows, 2);
    assert_eq!(cli.model_via.as_deref(), Some("cli"), "CLI 签的必须记 cli");
    assert!(cli.model_decided_at.is_some(), "cli 决策必须记 decided_at");

    // 没签过的那一轨两栏都是 NULL —— 不是 0 值、不是默认串。
    assert!(ui.model_via.is_none());
    assert!(cli.biz_via.is_none());
}

// ══════════════════════════════════════════════════════════════
// 读接口的内容面（AC 之外的交付要求：条目 + 双轨 + 置信度 + AI 理由 + 证据）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn read_api_carries_both_tracks_confidence_rationale_and_evidence() {
    let f = fixture(
        "req111_read_api",
        vec![
            entity("customer", "客户"),
            relation("customer", "order", "客户→订单", 0.95, "high"),
        ],
    )
    .await;
    let app = app(&f).await;

    let (s, b) = call(&app, "GET", "/api/review", Some(TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    let v = body_json(&b);
    assert_eq!(v["project"], json!(f.project));
    assert_eq!(v["base_version"], json!(1));
    assert_eq!(v["progress"]["total"], json!(2));

    let rel = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|it| it["item"] == json!(2))
        .expect("关系条目必须在列表里");
    // 双轨状态
    assert_eq!(rel["biz"], json!("pending"));
    assert_eq!(rel["model"], json!("pending"));
    // 置信度 + AI 理由（与 `review show --confidence` 同一份 payload）
    assert_eq!(rel["confidence"]["score"], json!(0.95));
    assert_eq!(rel["confidence"]["band"], json!("high"));
    assert_eq!(rel["confidence"]["ai"]["rationale"], json!("两端语义匹配"));
    // 证据
    assert_eq!(rel["evidence"][0]["claim"], json!("按命名约定推断"));
    // 候选定义（FDE 要裁决的原始内容）
    assert_eq!(rel["payload"]["from_type"], json!("customer"));
}

// ══════════════════════════════════════════════════════════════
// 页面壳：自包含 + 批量预览文案
// ══════════════════════════════════════════════════════════════

#[test]
fn page_shell_is_self_contained() {
    let html = ontok::review_ui::page::REVIEW_HTML;
    for bad in ["http://", "https://", "//cdn", "cdn.", "integrity="] {
        assert!(!html.contains(bad), "页面不得引外部资源 —— 可能离线打开：{bad}");
    }
    assert!(html.contains("Bearer"), "页面请求必须带口令");
    assert!(html.contains("api/decide"), "页面必须有签的落点");
    assert!(
        html.contains("先看条数") || html.contains("将接受 "),
        "页面必须说明批量先预览"
    );
}

/// 存储错误 → HTTP 的映射：**没有第 N 个条目**要 404 且说话（不许 500 / 不许静默）。
#[tokio::test]
async fn missing_item_is_a_visible_404() {
    let f = fixture("req111_missing", vec![entity("customer", "客户")]).await;
    let app = app(&f).await;
    let (s, b) = call(
        &app,
        "POST",
        "/api/decide",
        Some(TOKEN),
        Some(json!({"item": 99, "track": "biz", "decision": "accept"})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "不存在的条目应当 404：{b}");
    assert!(b.contains("99"), "要说清是第几个条目：{b}");

    // 参数错：400 + 具体话。
    let (s, b) = call(
        &app,
        "POST",
        "/api/decide",
        Some(TOKEN),
        Some(json!({"item": 1, "track": "业务", "decision": "accept"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(b.contains("biz"), "要说清合法取值：{b}");
}
