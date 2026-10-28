//! ⭐ REQ-159 P1b · **多项目模式**（一套部署 · 多套本体）的真库实测。
//!
//! # 这一刀只做两件事
//!
//! 1. 多项目解析：`Authorization: Bearer <key>` → `token.project` →
//!    `projects.active_version_id` → 按 **version_id** 缓存 / 懒加载快照；
//! 2. token 绑项目：`tokens.json` 的 `version: 2` 多一个 `project`，多项目模式下**每条必填**。
//!
//! # 判据（能红能绿）
//!
//! | 判据 | 测的是什么 | 怎么变红 |
//! | --- | --- | --- |
//! | AC-b1 | 单项目模式对同一请求的响应体与**基线二进制**逐字节相同（sha256 相等） | 动一处响应构造 / 多一个字段就红 |
//! | AC-b2 | 两个项目两把 key ⇒ 同一端点各返**自己**的 catalog（类数 2 vs 5） | 把 `token.project` 换成别的（如 tenant）就红 |
//! | AC-b3 | 项目没有 `active_version_id` ⇒ **503 `project.no_active_version`** | 返回 404 / 200 空目录就红 |
//! | AC-b4 | 多项目模式 + token 缺 `project` ⇒ **启动即失败并点名** | 放宽成「缺了也能起」就红 |
//! | AC-b5 | `version: 1` 的表在**单项目模式**下仍能启动 | 只认 2.x 就红 |
//! | AC-b6 | `activate` 换版本后（这里直接改库）**下一次请求**就是新版本 | 把 `active_version_id` 改成启动时读一次就红 |
//! | AC-b7 | 两个项目的快照**不是同一个** `Arc`（指针比较） | 缓存只留一份 / 键写成常量就红 |
//! | ⑤-快照缺失 | `active_version_id` 指向不存在的快照 ⇒ **500 `project.snapshot_missing`** | 静默 404 就红 |
//!
//! # 环境
//!
//! - `ONTOK_TEST_DATABASE_URL`（元数据库）**必需** —— 缺了 `common::test_pool` 会响亮失败；
//! - `ONTOK_BASELINE_BIN`（基线 `ontok` 二进制，如 `a4faf2d` 编出来的那份）**可选**：
//!   给了才跑 AC-b1，没给就**响亮跳过**（打印原因，不算红）。
//!
//! 夹具项目名带进程号 + 时间戳，结束时删掉（`ON DELETE CASCADE` 覆盖版本与快照表）。

mod common;

/// 测试库不可用时的统一说明：**可能是没设，也可能是指向了非专用库**（防污染闸会拒）。
const NEED_TEST_DB: &str = "ONTOK_TEST_DATABASE_URL 不可用：没设，或指向了非专用测试库（库名需含 test/scratch/tmp/ci/dev）";

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::snapshot::{EntityType, Property, Snapshot};
use ontok::domain::ttl::SchemaTypes;
use ontok::query::audit::AuditSink;
use ontok::query::http::build_router_multi;
use ontok::query::multi::{MultiProject, NO_ACTIVE_VERSION};
use ontok::query::serve::Deployment;
use ontok::query::tokens::TokenTable;
use ontok::query::ServiceMode;
use ontok::sparql::policy::QueryPolicy;
use serde_json::Value;
use sqlx::postgres::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN_A: &str = "FAKE-REQ159B-CLOUD-aaaaaaaaaaaaaaaa";
const TOKEN_B: &str = "FAKE-REQ159B-BILLING-bbbbbbbbbbbb";
/// 绑到一个**还没有生效版本**的项目上（AC-b3）。
const TOKEN_C: &str = "FAKE-REQ159B-PENDING-cccccccccccc";

/// 本体服务模式的占位租户声明（与 `query::serve::NO_TENANT_POLICY` 同一个形状）。
const NO_TENANT: &str = r#"{"version":"1.0.0","tenantSubject":"__no_tenant__",
    "tenantIdColumn":"__no_tenant__","perTable":{},"publicDimensions":[]}"#;

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req159b-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

/// 每个测试用**自己的**项目名（共享库上并行跑不会互相踩）。
fn unique(tag: &str) -> String {
    format!(
        "req159b-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn entity(key: &str) -> EntityType {
    EntityType {
        key: key.into(),
        display_name: format!("{key} 的中文名"),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![Property {
            name: "编号".into(),
            column: "id".into(),
            nullable: false,
            note: None,
            sample: None,
        }],
        evidence: vec![],
    }
}

fn snapshot(version_id: Uuid, seq: i32, classes: &[&str]) -> Snapshot {
    let mut s = Snapshot::new(version_id, seq);
    s.entity_types = classes.iter().map(|k| entity(k)).collect();
    s
}

/// 建项目 + 若干版本（每个版本一份快照），返回派生出的版本 id。
///
/// **不冻结、不激活**：调用方自己决定激活哪一版（AC-b3 需要「建了但没激活」）。
async fn make_project(pool: &PgPool, name: &str, versions: &[&[&str]]) -> Vec<Uuid> {
    let projects = PgProjectStore::new(pool.clone());
    let store = PgStore::new(pool.clone());
    let p = projects.get_or_create(name).await.expect("建项目");
    let mut ids = Vec::new();
    for classes in versions {
        let v = projects.new_version(p.id).await.expect("新版本");
        store
            .save_snapshot(&snapshot(v.id, v.seq, classes))
            .await
            .expect("存快照");
        ids.push(v.id);
    }
    ids
}

async fn activate(pool: &PgPool, name: &str, version_id: Uuid) {
    let projects = PgProjectStore::new(pool.clone());
    let p = projects.get_by_name(name).await.expect("读项目").expect("项目在");
    projects.activate(p.id, version_id).await.expect("激活");
}

/// 直接改库里的生效指针（模拟「`activate` 在**另一个进程**里执行」—— AC-b6 的关键）。
async fn point_at(pool: &PgPool, name: &str, version_id: Option<Uuid>) {
    sqlx::query("UPDATE projects SET active_version_id = $2 WHERE name = $1")
        .bind(name)
        .bind(version_id)
        .execute(pool)
        .await
        .expect("改生效指针");
}

async fn cleanup(pool: &PgPool, names: &[String]) {
    for n in names {
        // `ontology_versions` / 快照五表都是 ON DELETE CASCADE。
        let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(n)
            .execute(pool)
            .await;
    }
}

/// 多项目模式的部署级输入（**本体服务模式**：不连业务库 ⇒ 判据不需要源库）。
fn deployment() -> Deployment {
    Deployment {
        mode: ServiceMode::OntologyOnly,
        schema: SchemaTypes::default(),
        schema_path: None,
        policy: QueryPolicy::parse(NO_TENANT).expect("占位声明"),
        base: "https://ontok.local/".into(),
        include_inherit: false,
        access_raw: None,
        access_path: None,
        tenancy: None,
        timeout_ms: 10_000,
        sse_idle_timeout_ms: 60_000,
        sse_max_connections: 4,
        max_concurrent_queries: 2,
        source: None,
        facts: None,
    }
}

/// 一张多项目模式的 token 表（**每条都绑了项目** ⇒ 能通过启动闸门）。
fn tokens(pairs: &[(&str, &str)]) -> TokenTable {
    let entries: Vec<String> = pairs
        .iter()
        .map(|(t, p)| format!(r#"{{"token":"{t}","project":"{p}","label":"{p}"}}"#))
        .collect();
    TokenTable::parse_multi(
        &format!(r#"{{"version":2,"tokens":[{}]}}"#, entries.join(",")),
        ServiceMode::OntologyOnly,
    )
    .expect("多项目模式的 token 表")
}

fn registry(pool: &PgPool, pairs: &[(&str, &str)]) -> Arc<MultiProject> {
    Arc::new(MultiProject::new(
        pool.clone(),
        tokens(pairs),
        deployment(),
        AuditSink::stdout_only(),
    ))
}

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value, String) {
    let res = app.clone().oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 8 * 1024 * 1024)
        .await
        .expect("读 body");
    let text = String::from_utf8_lossy(&bytes).to_string();
    let v = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, v, text)
}

async fn get(app: &axum::Router, uri: &str, token: Option<&str>) -> (StatusCode, Value, String) {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION.as_str(), format!("Bearer {t}"));
    }
    call(app, b.body(Body::empty()).unwrap()).await
}

// ══════════════════════════════════════════════════════════════
// AC-b2 · 两把 key ⇒ 两份 catalog（不串）
// ══════════════════════════════════════════════════════════════

/// **AC-b2** · 多项目模式下一个进程服务两套本体：同一个端点 `GET /api/catalog`，
/// 换 key 就换项目（类数 2 vs 5，一眼能看出不是同一份）。
///
/// 变异：把「项目」从 `token.project` 换成别的（例如拿 `token.tenant` 顶）⇒
/// 两张表都没有 tenant（本体模式），两把 key 解析到同一个项目 ⇒ 类数相同 ⇒ 本测试红。
#[tokio::test]
async fn b2_two_keys_return_their_own_catalog() {
    let pool = common::test_pool().await;
    let name_a = unique("cloud");
    let name_b = unique("billing");
    let mut names = vec![name_a.clone(), name_b.clone()];

    let a1 = make_project(&pool, &name_a, &[&["vm", "disk"]]).await;
    let b1 = make_project(&pool, &name_b, &[&["bill", "item", "price", "period", "account"]]).await;
    activate(&pool, &name_a, a1[0]).await;
    activate(&pool, &name_b, b1[0]).await;

    let multi = registry(&pool, &[(TOKEN_A, &name_a), (TOKEN_B, &name_b)]);
    let app = build_router_multi(Arc::clone(&multi));

    let (sa, ca, _) = get(&app, "/api/catalog?page=1&page_size=50", Some(TOKEN_A)).await;
    let (sb, cb, _) = get(&app, "/api/catalog?page=1&page_size=50", Some(TOKEN_B)).await;
    assert_eq!(sa, StatusCode::OK, "{ca}");
    assert_eq!(sb, StatusCode::OK, "{cb}");

    // 项目名与版本都各是各的。
    assert_eq!(ca["project"], name_a.as_str(), "{ca}");
    assert_eq!(cb["project"], name_b.as_str(), "{cb}");
    assert_eq!(ca["version"], 1, "第一版：{ca}");
    assert_eq!(cb["version"], 1, "{cb}");

    // 类数不同 ⇒ 证明不是同一份快照（AC-b2 的判据）。
    assert_eq!(ca["counts"]["classes"], 2, "{ca}");
    assert_eq!(cb["counts"]["classes"], 5, "{cb}");
    eprintln!(
        "· AC-b2 key A → project={} v{} classes={} · key B → project={} v{} classes={}",
        ca["project"], ca["version"], ca["counts"]["classes"],
        cb["project"], cb["version"], cb["counts"]["classes"],
    );
    let keys_a: Vec<&str> = ca["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["table"].as_str().unwrap_or(""))
        .collect();
    let keys_b: Vec<&str> = cb["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["table"].as_str().unwrap_or(""))
        .collect();
    assert!(keys_a.contains(&"vm") && !keys_a.contains(&"bill"), "{keys_a:?}");
    assert!(keys_b.contains(&"bill") && !keys_b.contains(&"vm"), "{keys_b:?}");

    // 同一把 key 连问两次 ⇒ 还是自己那套（缓存命中不串）。
    let (_, ca2, _) = get(&app, "/api/catalog?page=1&page_size=50", Some(TOKEN_A)).await;
    assert_eq!(ca2["project"], name_a.as_str(), "{ca2}");
    assert_eq!(ca2["counts"]["classes"], 2, "{ca2}");

    cleanup(&pool, &mut names).await;
}

// ══════════════════════════════════════════════════════════════
// AC-b3 · 没有生效版本 ⇒ 503 project.no_active_version
// ══════════════════════════════════════════════════════════════

/// **AC-b3** · 项目建好了、包里也导入过，但**没有人 activate** ⇒
/// **503 + `code=project.no_active_version`**（⛔ 不是 404、⛔ 不是 200 空目录）。
#[tokio::test]
async fn b3_a_project_without_an_active_version_is_503() {
    let pool = common::test_pool().await;
    let name_c = unique("pending");
    let mut names = vec![name_c.clone()];
    // 有版本、有快照，但**不激活** ⇒ active_version_id 是 NULL。
    make_project(&pool, &name_c, &[&["vm"]]).await;

    let multi = registry(&pool, &[(TOKEN_C, &name_c)]);
    let app = build_router_multi(Arc::clone(&multi));

    let (s, body, text) = get(&app, "/api/catalog", Some(TOKEN_C)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{text}");
    assert_eq!(body["code"], NO_ACTIVE_VERSION, "{text}");
    assert!(
        body["message"].as_str().unwrap_or("").contains(&name_c),
        "要点名是哪个项目：{text}"
    );
    assert!(
        body["hint"].as_str().unwrap_or("").contains("activate"),
        "hint 要说清怎么让它上线：{text}"
    );
    eprintln!(
        "· AC-b3 项目 {name_c} 没有 active_version_id ⇒ {s} code={} · message={}",
        body["code"], body["message"]
    );

    // ⛔ 同一把 key 打别的端点也是同一个结论（不是某个端点忘了判）。
    let (s2, body2, _) = get(&app, "/api/describe?name=vm", Some(TOKEN_C)).await;
    assert_eq!(s2, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body2["code"], NO_ACTIVE_VERSION);

    // 激活之后**下一次请求**就好了（不需要重启）—— 这正是 AC-b6 的形状。
    let projects = PgProjectStore::new(pool.clone());
    let p = projects.get_by_name(&name_c).await.unwrap().unwrap();
    let v = projects.latest_version(p.id).await.unwrap().unwrap();
    activate(&pool, &name_c, v.id).await;
    let (s3, body3, text3) = get(&app, "/api/catalog", Some(TOKEN_C)).await;
    assert_eq!(s3, StatusCode::OK, "{text3}");
    assert_eq!(body3["counts"]["classes"], 1, "{text3}");

    cleanup(&pool, &mut names).await;
}

// ══════════════════════════════════════════════════════════════
// AC-b6 · 换 active_version_id ⇒ 下一次请求就是新版本（没有陈旧缓存）
// ══════════════════════════════════════════════════════════════

/// **AC-b6** · `activate` 在**另一个进程**里执行（本用例直接改库模拟），
/// `query serve` 收不到任何通知 —— 但**下一次请求**必须返回新版本的内容。
///
/// 为什么能做到：缓存的键是 **version_id**（不是 project）。指针一变，查表的键就变，
/// 自然落到新快照。
///
/// 变异：把 `active_version_id` 改成「启动时读一次」（例如缓存在注册表里）⇒
/// 第二次请求仍然是 v1 的 2 个类 ⇒ 本测试红。
#[tokio::test]
async fn b6_a_new_active_version_is_visible_on_the_very_next_request() {
    let pool = common::test_pool().await;
    let name = unique("switch");
    let mut names = vec![name.clone()];
    // v1: 2 个类；v2: 4 个类（内容不同 ⇒ 一眼看出换没换）。
    let ids = make_project(&pool, &name, &[&["vm", "disk"], &["vm", "disk", "zone", "image"]]).await;
    activate(&pool, &name, ids[0]).await;

    let multi = registry(&pool, &[(TOKEN_A, &name)]);
    let app = build_router_multi(Arc::clone(&multi));

    let (s1, c1, _) = get(&app, "/api/catalog?page=1&page_size=50", Some(TOKEN_A)).await;
    assert_eq!(s1, StatusCode::OK, "{c1}");
    assert_eq!(c1["version"], 1, "{c1}");
    assert_eq!(c1["counts"]["classes"], 2, "{c1}");

    // ── 另一个进程里发生的事：activate v2（= 冻结 + 挪指针）────
    // 这里拆成两步是为了让「指针变了」这一件事**独立**可测：
    // 冻结是版本自己的状态，`service_for` 每请求只读指针。
    PgProjectStore::new(pool.clone())
        .freeze(ids[1])
        .await
        .expect("冻结 v2");
    point_at(&pool, &name, Some(ids[1])).await;

    let (s2, c2, text2) = get(&app, "/api/catalog?page=1&page_size=50", Some(TOKEN_A)).await;
    assert_eq!(s2, StatusCode::OK, "{text2}");
    assert_eq!(c2["version"], 2, "下一次请求就该是新版本：{text2}");
    assert_eq!(c2["counts"]["classes"], 4, "{text2}");
    eprintln!(
        "· AC-b6 换指针前 v1 classes=2 · 换指针后**下一次请求** v{} classes={}（无重启、无跨进程通知）",
        c2["version"], c2["counts"]["classes"]
    );

    // 回滚回 v1 也一样（旧快照还在缓存里，但那不影响正确性）。
    point_at(&pool, &name, Some(ids[0])).await;
    let (_, c3, _) = get(&app, "/api/catalog?page=1&page_size=50", Some(TOKEN_A)).await;
    assert_eq!(c3["version"], 1, "{c3}");
    assert_eq!(c3["counts"]["classes"], 2, "{c3}");

    cleanup(&pool, &mut names).await;
}

// ══════════════════════════════════════════════════════════════
// AC-b7 · 两个项目**不共享**同一个快照对象
// ══════════════════════════════════════════════════════════════

/// **AC-b7** · 缓存按 **version_id** 分键：两个项目各自一份 `Arc<QueryService>`，
/// 指针不同（`Arc::ptr_eq == false`），类数也不同。
///
/// 变异：把缓存键写成常量 / 只留一份 ⇒ 第二个项目拿到第一份的对象 ⇒ 指针相等 ⇒ 红。
#[tokio::test]
async fn b7_two_projects_never_share_the_same_snapshot_object() {
    let pool = common::test_pool().await;
    let name_a = unique("iso-a");
    let name_b = unique("iso-b");
    let mut names = vec![name_a.clone(), name_b.clone()];
    let a1 = make_project(&pool, &name_a, &[&["vm", "disk"]]).await;
    let b1 = make_project(&pool, &name_b, &[&["bill", "item", "price"]]).await;
    activate(&pool, &name_a, a1[0]).await;
    activate(&pool, &name_b, b1[0]).await;

    let multi = registry(&pool, &[(TOKEN_A, &name_a), (TOKEN_B, &name_b)]);
    let app = build_router_multi(Arc::clone(&multi));
    let (_, _, _) = get(&app, "/api/catalog", Some(TOKEN_A)).await;
    let (_, _, _) = get(&app, "/api/catalog", Some(TOKEN_B)).await;

    assert_eq!(multi.cached_versions(), 2, "两个版本各一份");
    let ids = multi.cached_ids();
    assert!(ids.contains(&a1[0]) && ids.contains(&b1[0]), "{ids:?}");
    let sa = multi.cached_service(a1[0]).expect("A 在缓存里");
    let sb = multi.cached_service(b1[0]).expect("B 在缓存里");
    assert!(
        !Arc::ptr_eq(&sa, &sb),
        "两个项目**不是**同一个 Arc（缓存不许共用一份快照）"
    );
    // 内容也确实不同（类数不同 = 两份快照）。
    assert_eq!(sa.project, name_a);
    assert_eq!(sb.project, name_b);
    assert_eq!(sa.version, 1);
    assert_eq!(sb.version, 1);
    assert_ne!(
        sa.catalog_items.len(),
        sb.catalog_items.len(),
        "两套本体的目录条目数不同（这才是「没串」的证据）"
    );
    eprintln!(
        "· AC-b7 缓存 {} 份：A={:p}（{} 条目录）· B={:p}（{} 条目录）· ptr_eq={}",
        multi.cached_versions(),
        Arc::as_ptr(&sa),
        sa.catalog_items.len(),
        Arc::as_ptr(&sb),
        sb.catalog_items.len(),
        Arc::ptr_eq(&sa, &sb),
    );

    cleanup(&pool, &mut names).await;
}

// ══════════════════════════════════════════════════════════════
// ⑤ · 生效指针指向一个**装不上**的版本 ⇒ 500（不是 404、不是旧版本）
// ══════════════════════════════════════════════════════════════

/// **数据不一致 ⇒ 500**：生效指针指向一个**未冻结**的版本（库里手改 / 半截迁移）时，
/// ⛔ 不许静默 404（会被读成「没这个项目」）、⛔ 也不许退回去服务旧版本
/// （那会让你以为看到的是当前生效版本）。
///
/// ⚠️ 为什么不是 `project.snapshot_missing`：当前 schema 给 `projects.active_version_id`
/// 加了外键（`REFERENCES ontology_versions(id)`，无 `ON DELETE`），所以**指针不可能指向
/// 不存在的版本行**；而「版本行在、快照五表空」在 `PgStore::get_snapshot` 里读出来是
/// **一份空快照**（不是 `None`）。⇒ `snapshot_missing` 在本 schema 下**不可达**，
/// 它是防御分支（将来去掉外键、或快照行被单独删掉时才会用上）。它的错误形状由
/// `query::multi` 的单测钉住（`snapshot_missing_is_a_500_not_a_404`）。
///
/// 这一条测的是**唯一可达**的那类「指针指向装不上的版本」：未冻结。
#[tokio::test]
async fn a_pointer_to_an_unfrozen_version_is_500_not_404() {
    let pool = common::test_pool().await;
    let name = unique("dangling");
    let mut names = vec![name.clone()];
    let ids = make_project(&pool, &name, &[&["vm"], &["vm", "disk"]]).await;
    activate(&pool, &name, ids[0]).await;
    // 指针直接挪到**未冻结**的 v2（`activate` 会顺手冻结，这里刻意绕过它）。
    point_at(&pool, &name, Some(ids[1])).await;

    let multi = registry(&pool, &[(TOKEN_A, &name)]);
    let app = build_router_multi(Arc::clone(&multi));
    let (s, body, text) = get(&app, "/api/catalog", Some(TOKEN_A)).await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR, "{text}");
    assert_eq!(body["code"], "project.load_failed", "{text}");
    assert!(
        body["message"].as_str().unwrap_or("").contains("冻结"),
        "要说清是「未冻结」这一类不一致：{text}"
    );
    // ⛔ 明确不是 404（那会被读成「这个 key 没权限 / 项目不存在」）。
    assert_ne!(s, StatusCode::NOT_FOUND);

    cleanup(&pool, &mut names).await;
}

// ══════════════════════════════════════════════════════════════
// 启动闸门（真二进制）
// ══════════════════════════════════════════════════════════════

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn db_url() -> String {
    ontok::config::test_database_url().expect(NEED_TEST_DB)
}

/// **AC-b4** · 多项目模式（**不给** `--project`）+ 一条没绑项目的 token ⇒
/// **启动即失败**（退出码非 0），stderr 里**点名第几条 / 哪个 label** 并给两条修法。
///
/// 变异：把 `parse_multi` 换成 `parse_for_mode`（缺 project 也放行）⇒
/// 进程会起来并卡住等请求 ⇒ 本测试拿不到非 0 退出码 ⇒ 红。
#[test]
fn b4_multi_mode_refuses_to_start_when_a_token_has_no_project() {
    let dir = temp_dir("gate");
    let tokens = dir.join("tokens.json");
    std::fs::write(
        &tokens,
        r#"{"version":2,"tokens":[
            {"label":"ops-assistant","token":"FAKE-REQ159B-OK-aaaaaaaaaaaaaaaa","project":"cloud-resource"},
            {"label":"orphan-key","token":"FAKE-REQ159B-ORPHAN-bbbbbbbbbbbb"}
        ]}"#,
    )
    .expect("写 token 表");

    let out = std::process::Command::new(ontok_bin())
        .args([
            "query",
            "serve",
            "--bind",
            "127.0.0.1:0",
            "--tokens",
            tokens.to_str().unwrap(),
            "--ontology-only",
        ])
        .env("ONTOK_DATABASE_URL", db_url())
        .output()
        .expect("跑 ontok");

    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        !out.status.success(),
        "缺 project 的 token 必须让启动失败（exit={:?}）：\n{stderr}",
        out.status
    );
    assert!(stderr.contains("第 2 条"), "要点名第几条：\n{stderr}");
    assert!(stderr.contains("orphan-key"), "要点名 label：\n{stderr}");
    assert!(stderr.contains("没有 project"), "{stderr}");
    assert!(stderr.contains("--project"), "要给出两条修法：\n{stderr}");
    // ⛔ 不许提到「已经就绪」——启动闸门必须在 bind / 加载快照之前。
    assert!(!stderr.contains("就绪"), "拒绝了就不该打就绪横幅：\n{stderr}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **AC-b5** · `version: 1`（老格式，无 `project`）在**单项目模式**下仍然能启动。
///
/// 判据是「起得来 + 能应答」，不是「解析函数返回 Ok」：这条防的正是
/// 「文件能读、但启动路径另有闸门」。
#[tokio::test]
async fn b5_a_version_one_token_table_still_starts_in_single_project_mode() {
    let pool = common::test_pool().await;
    let name = unique("v1compat");
    let mut names = vec![name.clone()];
    let ids = make_project(&pool, &name, &[&["vm", "disk"]]).await;
    activate(&pool, &name, ids[0]).await;

    let dir = temp_dir("v1");
    let tokens = dir.join("tokens.json");
    // ⚠️ 本体服务模式的 token **不能带 tenant**（REQ-137），且这是 **v1** 格式（没有 project）。
    std::fs::write(
        &tokens,
        r#"{"version":"1.0.0","tokens":[
            {"label":"legacy-agent","token":"FAKE-REQ159B-LEGACY-aaaaaaaaaaaa"}
        ]}"#,
    )
    .expect("写 token 表");

    let port = free_port();
    let mut srv = spawn_server(
        &ontok_bin(),
        port,
        &tokens,
        &["--project", &name, "--version", "1", "--ontology-only"],
    );
    wait_ready(port);
    let (status, body) = http(
        port,
        "GET",
        "/api/catalog?page=1&page_size=50",
        &[("Authorization", "Bearer FAKE-REQ159B-LEGACY-aaaaaaaaaaaa")],
        &[],
    );
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let v: Value = serde_json::from_slice(&body).expect("catalog JSON");
    assert_eq!(v["project"], name.as_str(), "{v}");
    assert_eq!(v["counts"]["classes"], 2, "{v}");
    srv.stop();

    cleanup(&pool, &mut names).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// AC-b1 · 单项目模式与基线**逐字节一致**
// ══════════════════════════════════════════════════════════════

/// **AC-b1** · 同一个部署（同一项目 / 同一版 / 同一份 token 表 / 同一个 `--ontology-only`），
/// **基线二进制**（`a4faf2d`）与新二进制的响应体 **sha256 相等**。
///
/// 覆盖的端点：`/api/health`（免 token）、`/api/catalog`、`/api/describe`、
/// `/api/ontology.ttl`（两视图）、`POST /api/sparql`（本体模式 ⇒ 503 的正文也在比对里）、
/// `POST /mcp`（JSON-RPC `initialize` 与 `tools/call`）。
///
/// ⚠️ 基线二进制由 `ONTOK_BASELINE_BIN` 指定（编出来那份 `ontok.exe`）。
/// 没给 ⇒ **响亮跳过**（打印原因）：这条判据没有基线就没法判，⛔ 不假装通过。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b1_single_project_mode_is_byte_identical_to_the_baseline() {
    let Some(baseline) = std::env::var("ONTOK_BASELINE_BIN").ok().map(PathBuf::from) else {
        eprintln!(
            "⏭ 跳过 AC-b1：没有 ONTOK_BASELINE_BIN（基线 ontok 二进制）。\
             设成 `a4faf2d` 编出来的 ontok.exe 之后重跑本用例。"
        );
        return;
    };
    assert!(baseline.is_file(), "ONTOK_BASELINE_BIN 不是文件：{baseline:?}");

    let pool = common::test_pool().await;
    let name = unique("b1");
    let mut names = vec![name.clone()];
    let ids = make_project(&pool, &name, &[&["vm", "disk"]]).await;
    activate(&pool, &name, ids[0]).await;

    let dir = temp_dir("b1");
    let tokens = dir.join("tokens.json");
    // **v1 格式**（老部署就是这一份）：单项目模式下必须与基线一样能用（AC-b5 同一条）。
    std::fs::write(
        &tokens,
        r#"{"version":"1.0.0","tokens":[
            {"label":"b1-agent","token":"FAKE-REQ159B-B1-aaaaaaaaaaaaaaaa","max_rows":10}
        ]}"#,
    )
    .expect("写 token 表");

    let port_new = free_port();
    let port_old = free_port();
    let mut new_srv = spawn_server(
        &ontok_bin(),
        port_new,
        &tokens,
        &["--project", &name, "--version", "1", "--ontology-only"],
    );
    let mut old_srv = spawn_server(
        &baseline,
        port_old,
        &tokens,
        &["--project", &name, "--version", "1", "--ontology-only"],
    );
    wait_ready(port_new);
    wait_ready(port_old);

    let auth = [("Authorization", "Bearer FAKE-REQ159B-B1-aaaaaaaaaaaaaaaa")];
    let mcp = [
        ("Authorization", "Bearer FAKE-REQ159B-B1-aaaaaaaaaaaaaaaa"),
        ("Content-Type", "application/json"),
    ];
    let sparql_body = br#"{"query":"SELECT ?s WHERE { ?s a :vm }"}"#.to_vec();
    let mcp_init = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_vec();
    let mcp_call = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"catalog","arguments":{"page":1,"page_size":50}}}"#.to_vec();

    // (说明, method, path, headers, body)
    let cases: Vec<(&str, &str, &str, &[(&str, &str)], Vec<u8>)> = vec![
        ("health", "GET", "/api/health", &[], vec![]),
        ("catalog", "GET", "/api/catalog?page=1&page_size=50", &auth, vec![]),
        ("describe", "GET", "/api/describe?name=vm", &auth, vec![]),
        ("ttl-customer", "GET", "/api/ontology.ttl", &auth, vec![]),
        ("ttl-model", "GET", "/api/ontology.ttl?view=model", &auth, vec![]),
        ("sparql", "POST", "/api/sparql", &mcp, sparql_body),
        ("mcp-initialize", "POST", "/mcp", &mcp, mcp_init),
        ("mcp-catalog", "POST", "/mcp", &mcp, mcp_call),
        ("401-no-token", "GET", "/api/catalog", &[], vec![]),
        ("404", "GET", "/api/nope", &auth, vec![]),
    ];

    let mut compared = 0usize;
    for (what, method, path, headers, body) in cases {
        let (s_old, b_old) = http(port_old, method, path, headers, &body);
        let (s_new, b_new) = http(port_new, method, path, headers, &body);
        assert_eq!(s_old, s_new, "[{what}] 状态码变了：基线 {s_old} / 现在 {s_new}");
        let d_old = ontok::query::audit::sha256_hex(&b_old);
        let d_new = ontok::query::audit::sha256_hex(&b_new);
        assert_eq!(
            d_old,
            d_new,
            "[{what}] 响应体**不再逐字节相同**（AC-b1 / R6 破了）：\n  基线 {s_old} {d_old} ({} 字节)\n  现在 {s_new} {d_new} ({} 字节)\n  基线体：{}\n  现在体：{}",
            b_old.len(),
            b_new.len(),
            String::from_utf8_lossy(&b_old),
            String::from_utf8_lossy(&b_new),
        );
        compared += 1;
        eprintln!("· [{what}] {method} {path} ⇒ {s_new} · sha256:{d_new}");
    }
    assert_eq!(compared, 10, "十个请求都要真的比过");
    assert!(compared >= 10);

    new_srv.stop();
    old_srv.stop();
    cleanup(&pool, &mut names).await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════
// 起子进程 / 裸 HTTP（判据只用标准库：⛔ 不为测试引依赖）
// ══════════════════════════════════════════════════════════════

struct Server {
    child: std::process::Child,
}

impl Server {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("要一个空闲端口");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

fn spawn_server(bin: &Path, port: u16, tokens: &Path, extra: &[&str]) -> Server {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(["query", "serve", "--bind", &format!("127.0.0.1:{port}")])
        .arg("--tokens")
        .arg(tokens)
        .args(extra)
        .env("ONTOK_DATABASE_URL", db_url())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = cmd.spawn().expect("起 ontok 子进程");
    Server { child }
}

fn wait_ready(port: u16) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if let Ok((200, _)) = std::panic::catch_unwind(|| http(port, "GET", "/api/health", &[], &[])) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("服务在 30 秒内没有就绪（端口 {port}）");
}

/// 裸 HTTP/1.1（`Connection: close`）：返回 `(状态码, 正文)`。
///
/// 为什么不用 ureq：这条判据比的是**字节**，裸 socket 拿到的就是线上那串字节
/// （没有中间库做解码 / 规范化），也就不用为一个判据再引一个测试依赖。
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("连上服务");
    s.set_read_timeout(Some(Duration::from_secs(20)))
        .expect("设读超时");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).expect("写请求头");
    s.write_all(body).expect("写请求体");
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("读响应");

    let pos = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("响应头结束标记");
    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .expect("状态行");
    let raw = &buf[pos + 4..];
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    (status, if chunked { dechunk(raw) } else { raw.to_vec() })
}

/// 拆 chunked（axum 对流式响应会用；JSON 一般带 Content-Length，两种都认）。
fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = raw.windows(2).position(|w| w == b"\r\n") else {
            break;
        };
        let size_txt = String::from_utf8_lossy(&raw[..eol]).to_string();
        let size = usize::from_str_radix(size_txt.trim().split(';').next().unwrap_or("0"), 16)
            .unwrap_or(0);
        raw = &raw[eol + 2..];
        if size == 0 {
            break;
        }
        out.extend_from_slice(&raw[..size.min(raw.len())]);
        raw = &raw[(size + 2).min(raw.len())..];
    }
    out
}
