//! ⭐ REQ-162 · **本地评审台**的验收判据（进程内路由 + 一条**真进程** AC-007）。
//!
//! | AC | 判据 | 在哪 |
//! | --- | --- | --- |
//! | AC-001 | 无口令：`/` 与 `/api/*` 都不带凭据；非回环 `--bind` 启动即拒 | 本文件 + `console serve --help` |
//! | AC-002 | 无项目：只有 `--dir`；页面没有项目/版本选择器 | 本文件 |
//! | AC-003 | 草稿只读：decide / finalize 前后 `out/proposal.json` 字节不变 | 本文件 |
//! | AC-004 | 裁决持久：重新 `LocalConsole::open` 后已签条目仍在（读 `decisions.json`） | 本文件 |
//! | AC-005 | 定版门禁 + 冻结：`pending>0` ⇒ 400；定版后旧版本不随草稿变 | 本文件 |
//! | AC-006 | 导出的是定版：包里 `ontology/snapshot.json` == `versions/vN/snapshot.json`；`bundle verify` ok | 本文件 + AC-007 |
//! | AC-007 | **零库依赖**：清空 `ONTOK_DATABASE_URL` / `ONTOK_TEST_DATABASE_URL` / `ONTOK_SECRET_KEY`，起真进程走完全流程 | [`ac007_local_review_desk_runs_without_any_db_env`] |
//! | AC-008 | 页面 ≤300 KB、零外部资源、无 `localStorage` | 本文件 |
//!
//! ⚠️ 这里**没有一条 SQL、没有一个真库**：整个文件跑在临时目录上（这正是本单的验收面）。

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::console::local::{self, LocalConsole};
use ontok::console::page::{CONSOLE_HTML, CSP};
use serde_json::Value;
use tower::ServiceExt;

// ══════════════════════════════════════════════════════════════
// 夹具：一个工作目录 + 一份四条的草稿
// ══════════════════════════════════════════════════════════════

/// 草稿：两条实体 + 一条 **doc-origin** 关系（reviewed_by 是 null，要靠人签）+ 一条待拒的实体。
const DRAFT: &str = r#"{
  "project": "anspire_real",
  "base_version": 1,
  "extracted_by": {"agent":"demo-extractor","version":"0.1.0","round":1,"failed_groups":[]},
  "items": [
    {"kind":"add_entity_type","target":"客户",
     "payload":{"key":"customer","display_name":"客户","properties":[
       {"name":"客户编号","column":"customer_no","nullable":false}]}},
    {"kind":"add_entity_type","target":"订单",
     "payload":{"key":"order","display_name":"订单"}},
    {"kind":"add_link_type","target":"客户→订单",
     "payload":{"key":"customer_has_order","from_type":"customer","to_type":"order",
                "cardinality":"1:N","semantic":"has","origin":"doc","band":"medium",
                "reviewed_by":null,
                "evidence":[{"table":"t_order","column":"customer_id","claim":"订单挂客户"}]}},
    {"kind":"add_entity_type","target":"发票",
     "payload":{"key":"invoice","display_name":"发票"}}
  ]
}"#;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req162-it-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(d.join("out")).expect("建临时工作目录");
    std::fs::write(d.join("out/proposal.json"), DRAFT).expect("写草稿夹具");
    d
}

fn service(dir: &Path) -> Arc<LocalConsole> {
    Arc::new(LocalConsole::open(dir).expect("打开工作目录"))
}

struct Reply {
    status: StatusCode,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

async fn call(app: axum::Router, req: Request<Body>) -> Reply {
    let res = app.oneshot(req).await.expect("oneshot");
    let status = res.status();
    let headers: Vec<(String, String)> = res
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).to_string(),
            )
        })
        .collect();
    let body = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读 body")
        .to_vec();
    Reply {
        status,
        headers,
        body,
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn post(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn app_of(svc: &Arc<LocalConsole>) -> axum::Router {
    local::build_router(Arc::clone(svc))
}

/// 把草稿里的每一条按给定两轨签掉（key 从 `/api/draft` 读）。
async fn sign_all(svc: &Arc<LocalConsole>, biz: &str, model: &str) -> Vec<String> {
    let app = app_of(svc).await;
    let draft = call(app.clone(), get("/api/draft")).await;
    assert_eq!(draft.status, StatusCode::OK, "{}", draft.text());
    let keys: Vec<String> = draft.json()["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["key"].as_str().expect("key").to_string())
        .collect();
    for key in &keys {
        let body = serde_json::json!({"key": key, "biz": biz, "model": model}).to_string();
        let r = call(app.clone(), post("/api/decide", &body)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    }
    keys
}

// ══════════════════════════════════════════════════════════════
// AC-001/002 · state / draft 契约（无口令、无项目）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn state_reports_the_directory_draft_and_progress_without_any_credential() {
    let dir = temp_dir("state");
    let svc = service(&dir);
    let app = app_of(&svc).await;

    // 没有 Authorization 头：仍然 200（AC-001）。
    let r = call(app.clone(), get("/api/state")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    let v = r.json();
    assert!(v["dir"]
        .as_str()
        .unwrap()
        .contains("ontok-req162-it-state"));
    assert_eq!(v["draft"]["exists"], true);
    assert_eq!(v["draft"]["items"], 4, "草稿四条");
    assert_eq!(v["draft"]["signed"], 0);
    assert_eq!(v["draft"]["pending"], 4);
    assert_eq!(v["draft"]["stale"], false, "还没签过 ⇒ 不算过期");
    assert!(v["latest_version"].is_null(), "还没有定版");
    assert!(v["bundle"].is_null(), "还没有包");

    // 空目录：draft.exists=false（页面据此给「先跑 propose」的提示）。
    let empty = empty_dir("empty");
    let empty_svc = service(&empty);
    let r = call(app_of(&empty_svc).await, get("/api/state")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["draft"]["exists"], false);
}

/// 空工作目录（没有草稿，也没有 `.ontok/`）。
fn empty_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req162-it-empty-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(d.join("out")).expect("建空工作目录");
    d
}

#[tokio::test]
async fn draft_returns_every_item_with_its_current_decision_and_progress() {
    let dir = temp_dir("draft");
    let svc = service(&dir);
    let app = app_of(&svc).await;

    let r = call(app.clone(), get("/api/draft")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    let v = r.json();
    assert_eq!(v["progress"]["total"], 4);
    assert_eq!(v["progress"]["pending"], 4);
    assert!(v["proposal"]["items"].as_array().unwrap().len() == 4, "要带草稿全文");
    let first = &v["items"][0];
    assert_eq!(first["kind"], "add_entity_type");
    assert_eq!(first["target"], "客户");
    assert_eq!(first["biz"], "pending");
    assert_eq!(first["model"], "pending");
    assert_eq!(first["signed"], false);
    // key 形状 `seq:kind:target`（与库版 ItemView::id 同一个命名法）。
    assert_eq!(first["key"], "1:add_entity_type:客户");

    // 没有草稿 ⇒ 404 + 可执行的提示（人话 + 改法）。
    let empty = temp_dir("draft-missing");
    std::fs::remove_file(empty.join("out/proposal.json")).unwrap();
    let r = call(app_of(&service(&empty)).await, get("/api/draft")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let v = r.json();
    assert_eq!(v["code"], "DRAFT_MISSING");
    assert!(v["message"].as_str().unwrap().contains("proposal.json"));
    assert!(v["hint"].as_str().unwrap().contains("propose"), "{}", v);
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 裁决持久 + 两轨都要有值
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn a_decision_needs_both_tracks_and_survives_a_restart() {
    let dir = temp_dir("decide");
    let svc = service(&dir);
    let app = app_of(&svc).await;
    let key = "1:add_entity_type:客户";

    // 只签业务轨 ⇒ 仍是 pending（两轨都要有值）。
    let r = call(
        app.clone(),
        post("/api/decide", &format!(r#"{{"key":"{key}","biz":"accepted"}}"#)),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    let v = r.json();
    assert_eq!(v["signed"], false);
    assert_eq!(v["progress"]["signed"], 0);
    assert_eq!(v["progress"]["pending"], 4);

    // 再签建模轨 ⇒ signed。
    let r = call(
        app.clone(),
        post("/api/decide", &format!(r#"{{"key":"{key}","model":"accepted"}}"#)),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    assert_eq!(r.json()["signed"], true);
    assert_eq!(r.json()["progress"]["signed"], 1);

    // 文件真的落盘了：`decisions.json` 有这条 + 有草稿 sha。
    let raw = std::fs::read(dir.join(".ontok/decisions.json")).expect("decisions.json 要落盘");
    let decisions: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(decisions["items"][key]["biz"], "accepted");
    assert_eq!(decisions["items"][key]["model"], "accepted");
    assert!(decisions["items"][key]["decided_at"].as_str().unwrap().len() > 10);
    assert_eq!(
        decisions["proposal_sha256"].as_str().unwrap().len(),
        64,
        "草稿 sha256 要记下来（stale 判据）"
    );

    // 「重开页面」= 重新 open 同一个目录：裁决仍在（AC-004）。
    let again = service(&dir);
    let r = call(app_of(&again).await, get("/api/draft")).await;
    let item = &r.json()["items"][0];
    assert_eq!(item["biz"], "accepted");
    assert_eq!(item["model"], "accepted");
    assert_eq!(item["signed"], true);

    // 不认识的 key / 非法值 / 未知字段 ⇒ 400/404 + 点名。
    let r = call(
        app.clone(),
        post("/api/decide", r#"{"key":"nope","biz":"accepted"}"#),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.json()["code"], "ITEM_NOT_FOUND");
    let r = call(
        app.clone(),
        post("/api/decide", &format!(r#"{{"key":"{key}","biz":"maybe"}}"#)),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["code"], "BAD_DECISION");
    let r = call(
        app.clone(),
        post("/api/decide", &format!(r#"{{"key":"{key}","biz":"accepted","nonsense":1}}"#)),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "未知字段要拒");
}

// ══════════════════════════════════════════════════════════════
// AC-003/005 · 草稿只读 + 定版门禁 + 冻结
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn finalize_is_gated_on_pending_and_freezes_a_snapshot() {
    let dir = temp_dir("finalize");
    let before = std::fs::read(dir.join("out/proposal.json")).unwrap();
    let svc = service(&dir);
    let app = app_of(&svc).await;

    // 一条都没签 ⇒ 400（门禁），且不落任何定版。
    let r = call(app.clone(), post("/api/finalize", "")).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["code"], "DRAFT_NOT_READY");
    assert!(!dir.join(".ontok/versions/v1").exists(), "门禁拒绝时零产物");

    // 签三条、故意留第四条 pending ⇒ 仍然 400，且点名还差几条。
    let keys = sign_all_partial(&svc).await;
    let r = call(app.clone(), post("/api/finalize", "")).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["code"], "DRAFT_NOT_READY");
    assert!(r.json()["message"].as_str().unwrap().contains("1"), "{}", r.text());

    // 把最后一条**拒绝**掉（不是接受）⇒ 签完，可以定版。
    let last = &keys[3];
    let r = call(
        app.clone(),
        post(
            "/api/decide",
            &format!(r#"{{"key":"{last}","biz":"rejected","model":"rejected"}}"#),
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    let r = call(app.clone(), post("/api/finalize", "")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    let v = r.json();
    assert_eq!(v["seq"], 1);
    assert_eq!(v["items"], 3, "三条 accepted 进定版，被拒的那条不进");
    assert_eq!(v["snapshot_sha256"].as_str().unwrap().len(), 64);

    // 定版产物落盘（逐字节快照 + meta 五栏）。
    let snapshot_bytes = std::fs::read(dir.join(".ontok/versions/v1/snapshot.json"))
        .expect("snapshot.json 要落盘");
    let meta: Value = serde_json::from_slice(
        &std::fs::read(dir.join(".ontok/versions/v1/meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(meta["seq"], 1);
    assert_eq!(meta["items"], 3);
    assert_eq!(meta["snapshot_sha256"], v["snapshot_sha256"]);
    assert!(meta["created_at"].as_str().unwrap().contains('T'));
    assert_eq!(meta["proposal_sha256"].as_str().unwrap().len(), 64);
    let snapshot: Value = serde_json::from_slice(&snapshot_bytes).unwrap();
    let entity_keys: Vec<&str> = snapshot["entity_types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["key"].as_str().unwrap())
        .collect();
    assert!(entity_keys.contains(&"customer") && entity_keys.contains(&"order"));
    assert!(!entity_keys.contains(&"invoice"), "被拒绝的条目不许进定版");
    assert_eq!(snapshot["link_types"].as_array().unwrap().len(), 1);
    assert_eq!(
        snapshot["link_types"][0]["key"], "customer_has_order",
        "doc-origin 关系在人工签过之后必须进快照（reviewed_by 由评审台补在内存里）"
    );

    // 草稿一个字节都没变（AC-003）。
    assert_eq!(std::fs::read(dir.join("out/proposal.json")).unwrap(), before);

    // 草稿再变，旧定版不跟着变（AC-005 的「冻结」半边）。
    std::fs::write(dir.join("out/proposal.json"), DRAFT.replace("客户", "客户（改）")).unwrap();
    assert_eq!(
        std::fs::read(dir.join(".ontok/versions/v1/snapshot.json")).unwrap(),
        snapshot_bytes,
        "旧定版一个字节都不许跟着草稿变"
    );
    // 而 state 会亮 stale（裁决是对旧草稿签的）。
    let r = call(app.clone(), get("/api/state")).await;
    assert_eq!(r.json()["draft"]["stale"], true);
    // 旧版本仍在列表里。
    let r = call(app.clone(), get("/api/versions")).await;
    assert_eq!(r.json()["versions"][0]["seq"], 1);
}

/// 签前三条（全接受），第四条留 pending。
async fn sign_all_partial(svc: &Arc<LocalConsole>) -> Vec<String> {
    let app = app_of(svc).await;
    let draft = call(app.clone(), get("/api/draft")).await;
    let keys: Vec<String> = draft.json()["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["key"].as_str().unwrap().to_string())
        .collect();
    for key in keys.iter().take(3) {
        let body = serde_json::json!({"key": key, "biz": "accepted", "model": "accepted"}).to_string();
        let r = call(app.clone(), post("/api/decide", &body)).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    }
    keys
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 导出的是定版（包里快照 == 定版快照；下载是原始字节）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn bundle_is_only_from_a_final_version_and_carries_it_byte_for_byte() {
    let dir = temp_dir("bundle");
    let svc = service(&dir);
    let app = app_of(&svc).await;

    // 没有定版 ⇒ 400（草稿不能直接打包）。
    let r = call(app.clone(), post("/api/bundle", "")).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["code"], "NO_VERSION");

    sign_all(&svc, "accepted", "accepted").await;
    let r = call(app.clone(), post("/api/finalize", "")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());

    let r = call(app.clone(), post("/api/bundle", r#"{"seq":1}"#)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    let v = r.json();
    let file = v["file"].as_str().unwrap().to_string();
    assert!(file.ends_with("-v1.ontokbundle"), "{file}");
    let expected_path = dir.join(".ontok/bundles").join(&file);
    let disk = std::fs::read(&expected_path).expect("包要落盘");
    assert_eq!(v["bytes"].as_u64().unwrap() as usize, disk.len());

    // 包里 `ontology/snapshot.json` 与定版 `snapshot.json` **逐字节**相同（AC-006）。
    let entries = ontok::bundle::zip::read(&disk).expect("读包");
    let in_pack = entries
        .iter()
        .find(|e| e.path == ontok::bundle::SNAPSHOT_PATH)
        .expect("包里要有快照")
        .data
        .clone();
    assert_eq!(
        in_pack,
        std::fs::read(dir.join(".ontok/versions/v1/snapshot.json")).unwrap(),
        "包内快照必须与定版 snapshot.json 逐字节相同"
    );
    // 包本身通过自检，且 manifest 的项目名来自目录名、版本号 = 定版号。
    let report = ontok::bundle::verify(&disk);
    assert!(report.ok, "包必须 ok：{:?}", report.problems);
    assert_eq!(report.summary.project.as_deref(), Some(local::project_name_of(&dir).as_str()));
    assert_eq!(report.summary.ontology_seq, Some(1));

    // 下载：application/octet-stream + 原始字节（⛔ 不套 JSON）。
    let r = call(app.clone(), get("/api/bundle/download?seq=1")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.header("content-type"),
        Some("application/octet-stream"),
        "下载必须是原始字节"
    );
    assert!(
        r.header("content-disposition")
            .unwrap_or_default()
            .contains(&file),
        "下载要给出文件名"
    );
    assert_eq!(r.body, disk, "下载的字节必须就是落盘的那份包");

    // state 里能看到这个包；版本列表给 seq/时间/条数/sha。
    let r = call(app.clone(), get("/api/state")).await;
    assert_eq!(r.json()["bundle"]["file"], file);
    assert_eq!(r.json()["bundle"]["bytes"], disk.len());
    let r = call(app.clone(), get("/api/versions")).await;
    let versions = r.json()["versions"].as_array().unwrap().clone();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0]["seq"], 1);
    assert_eq!(versions[0]["items"], 4);
}

#[tokio::test]
async fn stale_decisions_are_reported_and_finalize_refuses() {
    let dir = temp_dir("stale");
    let svc = service(&dir);
    // 手写一份「给旧草稿签的」裁决：sha 全是 0，两轨都接受。
    let draft = call(app_of(&svc).await, get("/api/draft")).await.json();
    let mut items = serde_json::Map::new();
    for item in draft["items"].as_array().unwrap() {
        items.insert(
            item["key"].as_str().unwrap().to_string(),
            serde_json::json!({"biz":"accepted","model":"accepted","reason":"","decided_at":"2026-01-01T00:00:00Z"}),
        );
    }
    let decisions = serde_json::json!({"proposal_sha256": "0".repeat(64), "items": items});
    std::fs::create_dir_all(dir.join(".ontok")).unwrap();
    std::fs::write(
        dir.join(".ontok/decisions.json"),
        serde_json::to_vec_pretty(&decisions).unwrap(),
    )
    .unwrap();

    let app = app_of(&svc).await;
    let r = call(app.clone(), get("/api/state")).await;
    assert_eq!(r.json()["draft"]["stale"], true, "sha 不符必须 stale");
    let r = call(app.clone(), post("/api/finalize", "")).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["code"], "DECISIONS_STALE");
    assert!(r.json()["hint"].as_str().unwrap().contains("不会自动清空"));

    // 重新签一条 = 把裁决基准更新到当前草稿（人的显式动作），之后 stale 消失。
    let key = draft["items"][0]["key"].as_str().unwrap();
    let r = call(
        app.clone(),
        post(
            "/api/decide",
            &format!(r#"{{"key":"{key}","biz":"accepted","model":"accepted"}}"#),
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(app.clone(), get("/api/state")).await;
    assert_eq!(r.json()["draft"]["stale"], false);
}

// ══════════════════════════════════════════════════════════════
// 旧库版端点必须一条都不在（console serve 的进程里）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn every_old_db_backed_endpoint_is_gone() {
    let dir = temp_dir("old-routes");
    let app = app_of(&service(&dir)).await;
    for uri in [
        "/api/projects",
        "/api/projects/demo",
        "/api/projects/demo/bundles",
        "/api/projects/demo/bundles/b1",
        "/api/projects/demo/key",
        "/api/projects/demo/database",
        "/api/projects/demo/database/test",
        "/api/import",
        "/api/apply",
        "/api/freeze",
        "/api/export/ttl",
        "/api/r2rml",
        "/api/audit",
        "/api/review",
        "/api/config/exclusions",
        "/api/sparql",
        "/api/catalog",
    ] {
        let r = call(app.clone(), get(uri)).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{uri} 必须 404（旧库版端点已删）");
        assert_eq!(r.json()["code"], "NOT_FOUND", "{uri}");
        // 不是「401 因为没口令」—— 本机评审台根本没有口令这一层。
        assert_ne!(r.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
    for uri in [
        "/api/projects",
        "/api/import",
        "/api/apply",
        "/api/freeze",
        "/api/r2rml",
        "/api/review",
    ] {
        let r = call(app.clone(), post(uri, "{}")).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "POST {uri} 必须 404");
    }
}

// ══════════════════════════════════════════════════════════════
// AC-008 · 页面：一屏两栏、零外部资源、零浏览器状态
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn the_page_is_one_screen_two_columns_with_no_browser_state() {
    let dir = temp_dir("page");
    let svc = service(&dir);
    let r = call(app_of(&svc).await, get("/")).await;
    assert_eq!(r.status, StatusCode::OK, "`GET /` 必须匿名可打开");
    assert_eq!(r.body, CONSOLE_HTML.as_bytes(), "GET / 必须逐字节就是页面常量");
    assert!(r.header("content-type").unwrap_or_default().starts_with("text/html"));
    assert_eq!(r.header("content-security-policy"), Some(CSP));
    assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"));

    // 体积 / 零外部资源（AC-008）。
    assert!(
        CONSOLE_HTML.len() < 300 * 1024,
        "单文件必须 ≤300 KB，实际 {}",
        CONSOLE_HTML.len()
    );
    assert!(CONSOLE_HTML.len() > 10 * 1024, "页面太小，可能被截断");
    for needle in [
        "<script src=",
        "<link ",
        "<link>",
        "https://",
        "http://",
        "@import",
        "integrity=",
        "localStorage",
        "sessionStorage",
        "indexedDB",
        "document.cookie",
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
        "XMLHttpRequest",
        "form action=",
        "<input",
    ] {
        assert!(
            !CONSOLE_HTML.contains(needle),
            "页面里出现了 {needle:?}（AC-008：零外部资源 / 零浏览器状态 / 只 textContent）"
        );
    }

    // 一屏两栏：左边评审、右边导出。
    for needle in [
        "评审（核心）",
        "导出",
        "全选接受",
        "全选拒绝",
        "定版",
        "导出构建包",
        "业务",
        "建模",
        "本机 · 无口令",
        "confirm(",
        "定版后不可改，要改就再定一版",
        "textContent",
    ] {
        assert!(CONSOLE_HTML.contains(needle), "页面缺 {needle:?}");
    }
    assert!(
        CONSOLE_HTML.matches("textContent").count() >= 5,
        "用户数据必须只走 textContent"
    );
    // 只有一个 fetch 调用点（请求层）；下载用相对路径的 <a href>。
    assert_eq!(CONSOLE_HTML.matches("fetch(").count(), 1, "只应有一个 fetch 调用点");
    assert!(CONSOLE_HTML.contains("/api/state"), "页面要读 state");
    assert!(CONSOLE_HTML.contains("/api/decide"), "页面要写裁决");
    assert!(CONSOLE_HTML.contains("/api/finalize"), "页面要定版");
    assert!(CONSOLE_HTML.contains("/api/bundle"), "页面要导出包");
    // 旧库版端点一条都不许被页面消费。
    for gone in [
        "/api/projects",
        "/api/import",
        "/api/apply",
        "/api/audit",
        "/api/r2rml",
        "/api/config/",
        "api/sparql",
        "api/catalog",
    ] {
        assert!(!CONSOLE_HTML.contains(gone), "页面还在调旧端点 {gone:?}");
    }
    // 壳内零数据：夹具里的名字 / token / 路径一个都不在页面常量里。
    assert!(!CONSOLE_HTML.contains("anspire_real"), "壳里不许出现项目名");
    assert!(!CONSOLE_HTML.contains("绝密对象"), "壳里不许出现对象名");

    // 源码级：常量名与 CSP 都是登记过的那份（防「测试读 A、线上发 B」）。
    let src = common::read_source("src/console/page.rs");
    assert!(src.contains("pub const CONSOLE_HTML"), "页面常量必须叫 CONSOLE_HTML");
    assert!(src.contains("pub const CSP"), "CSP 必须是登记过的常量");
}

#[tokio::test]
async fn the_page_body_contains_no_data_from_the_workdir() {
    let dir = temp_dir("page-zero");
    std::fs::write(
        dir.join("out/proposal.json"),
        DRAFT.replace("\"客户\"", "\"绝密对象\""),
    )
    .unwrap();
    let svc = service(&dir);
    let app = app_of(&svc).await;
    // 数据在 `/api/state` / `/api/draft` 里（那是拿到数据的地方）…
    let r = call(app.clone(), get("/api/state")).await;
    assert_eq!(r.json()["draft"]["items"], 4);
    let r = call(app.clone(), get("/api/draft")).await;
    assert!(r.text().contains("绝密对象"), "草稿内容走 /api/draft");
    // …但 `GET /` 仍然是零数据的常量。
    let r = call(app, get("/")).await;
    assert!(!r.text().contains("绝密对象"), "页面壳里不许出现业务数据");
    assert_eq!(r.body, CONSOLE_HTML.as_bytes());
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 非回环启动即拒 + `--help` 里没有旧参数
// ══════════════════════════════════════════════════════════════

#[test]
fn the_cli_has_dir_and_no_token_project_or_schema() {
    let out = Command::new(bin())
        .args(["console", "serve", "--help"])
        .output()
        .expect("起不了 ontok-builder");
    assert!(out.status.success(), "--help 必须 0 退出");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    for needle in ["--dir", "--bind", "--no-browser", "127.0.0.1:8899", "无口令"] {
        assert!(text.contains(needle), "--help 里缺 {needle:?}：\n{text}");
    }
    for gone in ["--tokens", "--project", "--schema", "--audit-out", "--allow-public"] {
        assert!(
            !text.contains(gone),
            "--help 里不该再有 {gone:?}（REQ-162 去口令/去项目）：\n{text}"
        );
    }
}

#[test]
fn a_non_loopback_bind_is_refused_before_serving() {
    let dir = temp_dir("bind");
    let out = Command::new(bin())
        .args([
            "console",
            "serve",
            "--dir",
            dir.to_str().unwrap(),
            "--bind",
            "0.0.0.0:0",
            "--no-browser",
        ])
        .output()
        .expect("起不了 ontok-builder");
    assert!(!out.status.success(), "非回环地址必须启动即失败");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("只绑回环"), "要说清为什么拒：\n{err}");
    assert!(err.contains("没有 `--allow-public`"), "要说清没有那个开关：\n{err}");
}

// ══════════════════════════════════════════════════════════════
// AC-007 · 真进程：清空三个环境变量，走完 draft → decide → finalize → bundle
// ══════════════════════════════════════════════════════════════

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

struct RawReply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl RawReply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// 手写一条最小 HTTP/1.1 请求（不引 HTTP 客户端：判据只看字节）。
fn raw_http(addr: std::net::SocketAddr, method: &str, path: &str, body: Option<&[u8]>) -> RawReply {
    let mut s = TcpStream::connect(addr).expect("连不上刚绑好的端口");
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    if let Some(b) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).expect("写请求头");
    if let Some(b) = body {
        s.write_all(b).expect("写请求体");
    }
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("读响应");

    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("响应里没有头/体分隔");
    let head_text = String::from_utf8_lossy(&buf[..split]).to_string();
    let body = buf[split + 4..].to_vec();
    let mut lines = head_text.lines();
    let status: u16 = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .expect("状态行读不出");
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    RawReply {
        status,
        headers,
        body,
    }
}

fn env_state(name: &str) -> String {
    match std::env::var(name) {
        Ok(_) => "<set（父进程有值；子进程用 env_remove 清掉）>".to_string(),
        Err(_) => "<unset>".to_string(),
    }
}

/// ⭐ **AC-007 的硬判据**：清空 `ONTOK_DATABASE_URL` / `ONTOK_TEST_DATABASE_URL` /
/// `ONTOK_SECRET_KEY` 之后，真起一个 `ontok-builder console serve`，走完
/// `draft → decide → finalize → bundle`，并用**同一个二进制**的 `bundle verify`
/// 读出 `ok:true`（退出码 0）。
#[test]
fn ac007_local_review_desk_runs_without_any_db_env() {
    println!(
        "AC-007 · 父进程环境：ONTOK_DATABASE_URL={} · ONTOK_TEST_DATABASE_URL={} · ONTOK_SECRET_KEY={}",
        env_state("ONTOK_DATABASE_URL"),
        env_state("ONTOK_TEST_DATABASE_URL"),
        env_state("ONTOK_SECRET_KEY"),
    );

    let dir = temp_dir("ac007");
    let mut child = Command::new(bin())
        .args([
            "console",
            "serve",
            "--dir",
            dir.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
            "--no-browser",
        ])
        // ⭐ 关键：三个环境变量都**从子进程的环境里删掉**（不是设成空串）。
        .env_remove("ONTOK_DATABASE_URL")
        .env_remove("ONTOK_TEST_DATABASE_URL")
        .env_remove("ONTOK_SECRET_KEY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("起不了 ontok-builder console serve");

    // 从启动横幅里读**真实**端口（`:0` = 内核分配）。
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if tx.send(line.unwrap_or_default()).is_err() {
                break;
            }
        }
    });
    let addr = loop {
        match rx.recv_timeout(Duration::from_secs(90)) {
            Ok(line) => {
                if let Some(rest) = line.split("http://").nth(1) {
                    let authority = rest.split('/').next().unwrap_or("");
                    if let Ok(addr) = authority.parse::<std::net::SocketAddr>() {
                        println!("AC-007 · 子进程已就绪（未设置任何库/密钥变量）：http://{addr}/");
                        break addr;
                    }
                }
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "等服务启动超时（{e}）—— 这条判据要证「没有库也能起」，\
                     起不来就是 AC-007 不成立。子进程 stderr 见上。"
                );
            }
        }
    };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // ① state / draft
        let r = raw_http(addr, "GET", "/api/state", None);
        assert_eq!(r.status, 200, "state: {}", String::from_utf8_lossy(&r.body));
        assert_eq!(r.json()["draft"]["exists"], true);
        assert_eq!(r.json()["draft"]["items"], 4);
        let r = raw_http(addr, "GET", "/api/draft", None);
        assert_eq!(r.status, 200);
        let keys: Vec<String> = r.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["key"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys.len(), 4);

        // ② decide（两轨都 accepted）
        for key in &keys {
            let body = serde_json::json!({"key": key, "biz":"accepted", "model":"accepted"})
                .to_string();
            let r = raw_http(addr, "POST", "/api/decide", Some(body.as_bytes()));
            assert_eq!(r.status, 200, "decide {key}: {}", String::from_utf8_lossy(&r.body));
        }
        let r = raw_http(addr, "GET", "/api/state", None);
        assert_eq!(r.json()["draft"]["signed"], 4);
        assert_eq!(r.json()["draft"]["pending"], 0);

        // ③ finalize
        let r = raw_http(addr, "POST", "/api/finalize", Some(b"{}"));
        assert_eq!(r.status, 200, "finalize: {}", String::from_utf8_lossy(&r.body));
        assert_eq!(r.json()["seq"], 1);
        assert_eq!(r.json()["items"], 4);

        // ④ bundle + download
        let r = raw_http(addr, "POST", "/api/bundle", Some(b"{}"));
        assert_eq!(r.status, 200, "bundle: {}", String::from_utf8_lossy(&r.body));
        let bundle_name = r.json()["file"].as_str().unwrap().to_string();
        let r = raw_http(addr, "GET", "/api/bundle/download?seq=1", None);
        assert_eq!(r.status, 200);
        assert_eq!(
            r.headers
                .iter()
                .find(|(k, _)| k == "content-type")
                .map(|(_, v)| v.as_str()),
            Some("application/octet-stream")
        );
        let exported = dir.join(&bundle_name);
        std::fs::write(&exported, &r.body).expect("写导出的包");
        let on_disk = std::fs::read(dir.join(".ontok/bundles").join(&bundle_name))
            .expect("包应落在 .ontok/bundles/");
        assert_eq!(r.body, on_disk, "下载的字节就是落盘的包");

        // ⑤ `bundle verify` 读出 ok:true（退出码 0）—— 也是**同一个二进制**，同样清空变量。
        let verify = Command::new(bin())
            .args(["bundle", "verify", "--file", exported.to_str().unwrap()])
            .env_remove("ONTOK_DATABASE_URL")
            .env_remove("ONTOK_TEST_DATABASE_URL")
            .env_remove("ONTOK_SECRET_KEY")
            .output()
            .expect("跑不了 bundle verify");
        let stdout = String::from_utf8_lossy(&verify.stdout).to_string();
        let stderr = String::from_utf8_lossy(&verify.stderr).to_string();
        println!("AC-007 · bundle verify 退出码 = {:?}", verify.status.code());
        println!("AC-007 · bundle verify stdout =\n{stdout}");
        assert!(verify.status.success(), "bundle verify 必须 0：\n{stderr}\n{stdout}");
        assert!(
            stdout.contains("\"ok\": true"),
            "bundle verify 的 JSON 必须 ok:true：\n{stdout}"
        );
        assert!(stderr.contains("✓ 通过"), "人话报告要说通过：\n{stderr}");
        println!(
            "AC-007 · 全流程 OK：draft(4) → decide(4) → finalize v1 → bundle {}（{} B）",
            bundle_name,
            r.body.len()
        );
    }));

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
