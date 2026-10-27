//! REQ-169 · `@pending.get.auth` 必须带**签名服务名**（`service`）。
//!
//! # 为什么这条不改就是废配方
//!
//! `query serve` 在 v2 契约下已经把配方摊平进 `@pending[].get`，但 `auth` 里**没有**
//! `service`。取数脚本的 tc3 签名器是 **fail-closed** 的（⛔ 不从 host 猜 service），
//! 于是数字员工拿着这份配方**签不出能被接受的请求** —— 只差服务端把它发出来这一环。
//!
//! | # | 判据 | 层 |
//! | --- | --- | --- |
//! | **AC-001** | v2 AM（含 `auth.service`）⇒ `@pending[].get.auth.service` 存在且**等于 AM 的值** | 真进程 + 真库 |
//! | **AC-002** | v1 AM ⇒ `@pending.get` **仍然只有 4 键**（class/filter/pushdown/ref），一个都不多 | 契约层 + 真进程 |
//! | **AC-003** | v2 AM 但 `auth.service` 缺失 ⇒ 配方里**没有** `service` 键（⛔ 不塞空串、不从 `base_url` 猜） | 契约层 + 真进程 |
//! | **AC-004** | 把真回执里的 `@pending.get` 原样喂 `access_fetch.py --recipe - --format rows --check` ⇒ 不再因缺 service 被拒 | 端到端（真脚本） |
//!
//! # 红绿自证
//!
//! 把 `build_recipe()` 里填 `service` 的那几行去掉 ⇒ **AC-001 红**（`auth.service` 不在）
//! 且 **AC-004 红**（真回执里没有 service ⇒ 脚本判 `结果：拒绝` / 点名 `auth.service`）。
//! AC-002 / AC-003 保持绿 —— 它们判的是「不多出键」，与本改动**方向相反**的那一半。
//!
//! # 环境
//!
//! | 变量 | 用途 | 缺了怎样 |
//! | --- | --- | --- |
//! | `ONTOK_TEST_DATABASE_URL`（或 `ONTOK_DATABASE_URL`） | 元数据库（项目 / 版本 / 快照） | **响亮跳过**（契约层的 4 条照跑） |
//! | `ONTOK_SECRET_KEY` | `query serve` 起进程的写面密钥闸门（单项目模式无条件加载） | **响亮跳过** |
//! | `ONTOK_ACCESS_FETCH` | `access_fetch.py` 的路径（覆盖默认的 `../Deploy/...`） | 跳过 AC-004（⛔ 不假装通过） |
//!
//! ⚠️ 真进程的几条都起**真二进制**（`CARGO_BIN_EXE_ontok`）：`@pending` 的字节是
//! 「进程怎么装配服务」的产物，在进程内测不到那一层。

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::model_view::{ModelEntityView, ModelLinkView, ModelTrackView};
use ontok::domain::snapshot::{EntityType, Snapshot};
use ontok::query::source_gate::{AccessMapping, SourceCheck};
use ontok::sparql::query::parse as parse_query;
use serde_json::{json, Value};
use sqlx::postgres::PgPool;

/// 查询 key（≥ 24 字符；判据只要「有且形状对」，⛔ 不是真凭据）。
const TOK: &str = "FAKE-REQ169-QUERY-aaaaaaaaaaaaaaaaaaaa";

/// 本体里的类 key（= AM `entities[].class`）。
const CLASS: &str = "alarm_policy";
/// AM 里写的服务名。
///
/// ⚠️ **刻意选一个与 `base_url` 首段不同的值**：夹具的 `base_url` 是
/// `https://monitor.tencentcloudapi.com`（首段 `monitor`），而 tc3 的 service 是 `cvm`。
/// 于是「配方里出现 `cvm`」只可能来自 `sources[].auth.service` —— 任何「从 host 猜」的
/// 实现都会给出 `monitor`，本文件的断言立刻红。
const SERVICE: &str = "cvm";
/// 第二个值：证明配方是**逐字搬运**，不是按 mode 填的某个常量。
const SERVICE_ALT: &str = "monitor";
/// `base_url` 的首段（「猜出来的 service」会长这样）。
const HOST_SEGMENT: &str = "monitor";

// ══════════════════════════════════════════════════════════════
// 夹具：两版 AM + 一条「只看契约」的取配方辅助
// ══════════════════════════════════════════════════════════════

/// 一份最小 AM。
///
/// - `contract_version = 1` ⇒ 服务端**不建配方**（`Recipe` 全 `None`）⇒ `get` 只有 4 键；
/// - `contract_version = 2` ⇒ 配方摊平进 `get`；
/// - `service = None` ⇒ **不写** `sources[].auth.service`（AC-003 的夹具）。
fn am(contract_version: u32, service: Option<&str>) -> String {
    let service_field = match service {
        Some(s) => format!(r#","service":"{s}""#),
        None => String::new(),
    };
    format!(
        r#"{{
          "contract_version": {contract_version},
          "mapping_version": 3,
          "sources": [{{
            "id": "monitor",
            "base_url": "https://monitor.tencentcloudapi.com",
            "auth": {{
              "mode": "tc3-hmac-sha256",
              "credential_env": ["TENCENT_SECRET_ID", "TENCENT_SECRET_KEY"],
              "signed_headers": ["content-type", "host", "x-tc-action"],
              "timestamp": {{ "header": "X-TC-Timestamp", "format": "unix-seconds" }},
              "scope": ["date", "service", "tc3_request"]{service_field}
            }},
            "defaults": {{ "Region": "ap-guangzhou", "module": "monitor" }}
          }}],
          "entities": [{{
            "id": "AlarmPolicy",
            "class": "{CLASS}",
            "source": "monitor",
            "capability": {{ "pushdown": ["ids"] }},
            "freshness": "realtime",
            "operation": {{
              "action": "DescribeAlarmPolicies",
              "version": "2018-07-24",
              "method": "POST",
              "content_type": "application/json"
            }},
            "request": {{
              "required": ["module"],
              "template": {{ "Module": "{{module}}" }},
              "bindings": {{ "module": {{ "source": "default" }} }},
              "paging": {{ "offset": "Offset", "limit": "Limit", "value_type": "number" }}
            }},
            "iterator": "$.Response.Policies[*]",
            "fields": {{ "name": {{ "path": "PolicyName" }} }}
          }}]
        }}"#
    )
}

/// 只跑契约层（不起进程、不连库）：`parse` → `bind` → `check` ⇒ 第 0 条 pending 的 `get`。
fn pending_get(mapping: &str) -> ontok::query::source_gate::GetInstruction {
    let model = model();
    let m = AccessMapping::parse(mapping)
        .expect("AM 形状合法")
        .bind(&model)
        .expect("AM 里的类必须在这一版本体里");
    let q = parse_query(&format!("SELECT ?c WHERE {{ ?c a :{CLASS} }}"), IRI).expect("查询合法");
    let c = m.check(&q, IRI, None);
    assert!(matches!(c, SourceCheck::Api { .. }), "{c:?}");
    c.pending()[0].get.clone().expect("按类点名必须给 get")
}

/// 把服务端那份 `get` 摊平成**线上形状**（`jsonld::PendingGet`）再序列化。
///
/// ⚠️ 必须走这个类型：AC-002 / AC-003 判的是**摊平之后**的键集合
/// （`#[serde(flatten)]` + `skip_serializing_if` 的合力），⛔ 不是 `GetInstruction` 的
/// Rust 字段 —— 后者没有 `Serialize`，也不代表客户端看到的东西。
fn wire_get(get: &ontok::query::source_gate::GetInstruction) -> Value {
    let pg = ontok::query::jsonld::PendingGet {
        class: get.class.clone(),
        filter: ontok::query::jsonld::PendingFilter { ids: get.ids.clone() },
        pushdown: get.pushdown.clone(),
        reference: get.reference.clone(),
        recipe: get.recipe.clone().unwrap_or_default(),
    };
    serde_json::to_value(&pg).expect("序列化")
}

const IRI: &str = "https://ontok.local/demo";

fn model() -> ModelTrackView {
    ModelTrackView {
        seq: 1,
        extracted_by: None,
        entity_types: vec![ModelEntityView {
            key: CLASS.to_string(),
            display_name: "告警策略".to_string(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: vec![],
            evidence: vec![],
        }],
        link_types: Vec::<ModelLinkView>::new(),
        action_types: vec![],
        functions: vec![],
    }
}

/// 序列化后的**键名列表**（摊平之后调用的那个形状）。
fn keys_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .unwrap_or_else(|| panic!("不是 JSON 对象：{v}"))
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

/// v1 的四个旧键（顺序无关，比较前排序）。
fn legacy_keys() -> Vec<String> {
    ["class", "filter", "pushdown", "ref"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

// ══════════════════════════════════════════════════════════════
// 契约层（不需要库 / 不需要进程）
// ══════════════════════════════════════════════════════════════

/// **AC-002（契约层）** · v1 ⇒ `get` 摊平后**恰好 4 键**，逐键比对。
#[test]
fn ac002_contract_v1_get_has_exactly_the_four_legacy_keys() {
    let get = pending_get(&am(1, Some(SERVICE)));
    assert!(get.recipe.is_none(), "v1 不建配方（AC-103）");
    let v = wire_get(&get);
    assert_eq!(
        keys_of(&v),
        legacy_keys(),
        "v1 的 `@pending.get` 必须逐键等于旧 4 键（一个都不多）：{v}"
    );
    // 夹具**确实**写了 service —— 于是这条断言不是「因为没写所以没出现」的空命题。
    assert!(am(1, Some(SERVICE)).contains(r#""service":"cvm""#));
    // 而且它与 `base_url` 首段**不同**：这条 v1 断言里若出现 `monitor`，就说明有人在猜。
    assert_ne!(SERVICE, HOST_SEGMENT);
    let text = serde_json::to_string(&v).unwrap();
    assert!(!text.contains("\"auth\""), "{text}");
    assert!(!text.contains("\"service\""), "{text}");
}

/// **AC-003（契约层）** · v2 + 没写 `service` ⇒ `auth` 里**没有**这个键。
#[test]
fn ac003_contract_v2_without_service_has_no_service_key() {
    let get = pending_get(&am(2, None));
    let auth = get
        .recipe
        .as_ref()
        .and_then(|r| r.auth.as_ref())
        .expect("v2 有 auth 声明（credential_env 等都在）");
    assert!(
        auth.service.is_none(),
        "AM 没写 service ⇒ 配方里必须没有它（⛔ 不塞空串、⛔ 不从 host 猜）"
    );
    let v = serde_json::to_value(auth).expect("序列化");
    assert!(
        !keys_of(&v).contains(&"service".to_string()),
        "键里不该出现 service：{v}"
    );
    let text = serde_json::to_string(auth).unwrap();
    // ⚠️ 不能拿 `contains("service")` 判：`scope` 里**本来就有** `"service"` 这一段名
    // （scope 段名 ≠ auth.service）。要判的是**键**。
    assert!(
        !text.contains(r#""service":"#),
        "auth 里出现了 service **键**：{text}"
    );
    // 其余四项照旧（`service` 是**加法**，不是替换）。
    for k in ["credential_env", "signed_headers", "timestamp", "scope"] {
        assert!(text.contains(k), "缺 {k}：{text}");
    }
}

/// **AC-001（契约层）** · v2 + 写了 `service` ⇒ 配方里**逐字**是那个值（不是按 mode 编的常量）。
#[test]
fn ac001_contract_v2_carries_service_verbatim() {
    for want in [SERVICE, SERVICE_ALT] {
        let get = pending_get(&am(2, Some(want)));
        let auth = get
            .recipe
            .as_ref()
            .and_then(|r| r.auth.as_ref())
            .expect("v2 有 auth");
        assert_eq!(
            auth.service.as_deref(),
            Some(want),
            "配方里的 service 必须是 AM 里写的那个值（⛔ 不从 base_url 推、⛔ 不按 mode 填常量）"
        );
        let v = serde_json::to_value(auth).unwrap();
        assert_eq!(v["service"], want);
    }
    // 「没写」与「写了」是两件事：同一份 AM 去掉 service ⇒ None。
    let without = pending_get(&am(2, None));
    assert!(without
        .recipe
        .expect("v2 有配方")
        .auth
        .expect("有 auth")
        .service
        .is_none());
}

/// **AC-003 的边界** · 源**只写了** `service`（没 credential_env / 头 / scope）⇒
/// `auth` 仍然出现，且只带那一个键（⛔ 不是整段被过滤掉）。
///
/// 为什么单列一条：`auth` 原来是「四项全空 ⇒ 整段不出现」，加了 `service` 之后，
/// 漏掉这条并有**两**个后果：少一个键（AC-001 红）+ 整段消失（下游连 mode 都读不到）。
#[test]
fn ac003_a_source_that_only_declares_service_still_emits_auth() {
    let raw = am(2, Some(SERVICE));
    let mut doc: Value = serde_json::from_str(&raw).unwrap();
    doc["sources"][0]["auth"] = json!({ "service": SERVICE });
    let get = pending_get(&serde_json::to_string(&doc).unwrap());
    let auth = get
        .recipe
        .expect("v2 有配方")
        .auth
        .expect("只写 service 也要把 auth 发出来");
    assert_eq!(auth.service.as_deref(), Some(SERVICE));
    let v = serde_json::to_value(&auth).unwrap();
    assert_eq!(keys_of(&v), vec!["service".to_string()], "{v}");
}

// ══════════════════════════════════════════════════════════════
// 真进程 + 真库
// ══════════════════════════════════════════════════════════════

fn ontok_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ontok"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ontok-req169-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&d).expect("建临时目录");
    d
}

fn secret_key() -> String {
    // 64 位十六进制（32 字节）。⛔ 不写死真密钥 —— 判据只要「有且形状对」。
    "ab".repeat(32)
}

/// 一个「可服务」的最小项目：一条 api 类 + 已冻结的第 1 版。
struct Live {
    pool: PgPool,
    project: String,
    version: i32,
    dir: PathBuf,
}

/// 起库夹具。缺环境 ⇒ **响亮跳过**并返回 `None`（⛔ 不假装通过）。
async fn live(tag: &str, test: &str) -> Option<Live> {
    if ontok::config::test_database_url().is_err() {
        common::skip::missing("ONTOK_TEST_DATABASE_URL", test);
        return None;
    }
    // `query serve` 单项目模式**无条件**加载写面密钥（缺 ⇒ 拒绝启动），所以这里先判。
    if std::env::var("ONTOK_SECRET_KEY")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        common::skip::missing("ONTOK_SECRET_KEY", test);
        return None;
    }
    let pool = common::test_pool().await;
    common::migrate(&pool).await;

    let projects = PgProjectStore::new(pool.clone());
    let project = common::unique_project(&format!("req169{tag}"));
    let p = common::new_project(&projects, &project).await;
    let v = projects.new_version(p.id).await.expect("新版本");

    let mut snap = Snapshot::new(v.id, v.seq);
    snap.entity_types = vec![EntityType {
        key: CLASS.to_string(),
        display_name: "告警策略".to_string(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    }];
    PgStore::new(pool.clone())
        .save_snapshot(&snap)
        .await
        .expect("存快照");
    // 单项目模式的 `query serve` 只服务**已冻结**的版本（与 `serve` 同一条闸门）。
    projects.freeze(v.id).await.expect("冻结");

    Some(Live { pool, project, version: v.seq, dir: temp_dir(tag) })
}

async fn cleanup(live: &Live) {
    let _ = sqlx::query("DELETE FROM projects WHERE name = $1")
        .bind(&live.project)
        .execute(&live.pool)
        .await;
    let _ = std::fs::remove_dir_all(&live.dir);
}

struct Server {
    child: Child,
    port: u16,
    stderr: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    /// 进程的 stderr（起不来时用它给出**人话**，而不是一句「连不上端口」）。
    fn stderr_text(&self) -> String {
        std::fs::read_to_string(&self.stderr).unwrap_or_default()
    }
}

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("要一个空闲端口");
    let p = l.local_addr().expect("addr").port();
    drop(l);
    p
}

fn tokens_json() -> String {
    format!(r#"{{"version":"1.0.0","tokens":[{{"label":"agent","token":"{TOK}"}}]}}"#)
}

/// 起 **真进程**：`ontok query serve --ontology-only --project … --version … --access-mapping …`。
///
/// `--ontology-only`：判据不需要客户源库（纯 api 类的 `@pending` 不碰库）。
fn spawn(live: &Live, am_text: &str) -> Server {
    let am_path = live.dir.join("access-mapping.json");
    std::fs::write(&am_path, am_text).expect("写 AM");
    let tokens = live.dir.join("tokens.json");
    std::fs::write(&tokens, tokens_json()).expect("写 token 表");
    let stderr_path = live.dir.join("serve.stderr.log");
    let port = free_port();

    let child = Command::new(ontok_bin())
        .args([
            "query",
            "serve",
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--tokens",
            tokens.to_str().unwrap(),
            "--ontology-only",
            "--project",
            &live.project,
            "--version",
            &live.version.to_string(),
            "--access-mapping",
            am_path.to_str().unwrap(),
        ])
        .env("ONTOK_DATABASE_URL", ontok::config::test_database_url().unwrap())
        .env("ONTOK_SECRET_KEY", secret_key())
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::File::create(&stderr_path).expect("建 stderr 日志"),
        ))
        .spawn()
        .expect("起 ontok 子进程");

    let srv = Server { child, port, stderr: stderr_path };
    wait_ready(&srv);
    srv
}

fn wait_ready(srv: &Server) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        if let Ok((200, _)) = std::panic::catch_unwind(|| {
            http(srv.port, "GET", "/api/health", &Vec::new(), &[])
        }) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "服务在 30 秒内没有就绪（端口 {}）。stderr：\n{}",
        srv.port,
        srv.stderr_text()
    );
}

/// 裸 HTTP/1.1（`Connection: close`）：返回 `(状态码, 正文)`。
fn http(port: u16, method: &str, path: &str, headers: &[(String, String)], body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("连上服务");
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
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
    (status, if chunked { dechunk(raw) } else { raw.to_vec() })
}

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

/// 打一条「按类点名」的查询，拿回 JSON-LD 回执（`@pending` 在里面）。
fn sparql_jsonld(srv: &Server) -> Value {
    let body = json!({ "query": format!("SELECT ?c WHERE {{ ?c a :{CLASS} }}") }).to_string();
    let headers = vec![
        ("Authorization".to_string(), format!("Bearer {TOK}")),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    let (status, raw) = http(
        srv.port,
        "POST",
        "/api/sparql?format=jsonld",
        &headers,
        body.as_bytes(),
    );
    let text = String::from_utf8_lossy(&raw).to_string();
    assert_eq!(
        status, 200,
        "纯 api 类的 jsonld 查询应当 200 且带 @pending。stderr：\n{}\nbody：{text}",
        srv.stderr_text()
    );
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("不是 JSON（{e}）：{text}"))
}

/// 回执里的 `@pending[0].get`。
fn pending_get_of(receipt: &Value) -> Value {
    let pending = receipt["@pending"]
        .as_array()
        .unwrap_or_else(|| panic!("回执里没有 @pending 数组：{receipt}"));
    assert_eq!(pending.len(), 1, "只点名了一个类：{receipt}");
    pending[0]["get"].clone()
}

/// **AC-001（真进程 + 真库）** · v2 AM（含 `service`）⇒ `@pending[0].get.auth.service`
/// **存在且等于 AM 里的值**。
#[tokio::test]
async fn ac001_live_v2_pending_get_carries_auth_service() {
    let test = "ac001_live_v2_pending_get_carries_auth_service";
    let Some(live) = live("a1", test).await else { return };
    let srv = spawn(&live, &am(2, Some(SERVICE)));

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    let auth = get
        .get("auth")
        .unwrap_or_else(|| panic!("v2 的 get 必须有 auth：{get}"));
    assert_eq!(
        auth["service"], SERVICE,
        "`@pending[0].get.auth.service` 必须等于 AM 里写的值 —— \
         没有它，数字员工签 tc3 必被拒（这就是本需求的全部）：{auth}"
    );
    // 反证：「从 host 猜」的实现会给出 `monitor`（`base_url` 首段），而不是 AM 里的 `cvm`。
    assert_ne!(
        auth["service"].as_str(),
        Some(HOST_SEGMENT),
        "配方里的 service 等于 base_url 首段 ⇒ 它是猜出来的，不是 AM 声明的：{auth}"
    );
    // 其余四项也在（加法不是替换）。
    for k in ["credential_env", "signed_headers", "timestamp", "scope"] {
        assert!(auth.get(k).is_some(), "缺 {k}：{auth}");
    }
    eprintln!("· AC-001 · @pending[0].get.auth = {auth}");
    eprintln!("· AC-001 · @pending[0].get 的键 = {:?}", keys_of(&get));

    drop(srv);
    cleanup(&live).await;
}

/// **AC-002（真进程 + 真库）** · v1 AM ⇒ `@pending.get` 仍然**只有 4 键**（逐键比对）。
#[tokio::test]
async fn ac002_live_v1_pending_get_has_only_the_four_legacy_keys() {
    let test = "ac002_live_v1_pending_get_has_only_the_four_legacy_keys";
    let Some(live) = live("a2", test).await else { return };
    // 契约 v1 但源里**照样写着** service：证明「4 键」不是靠夹具里没有 service 混过去的。
    let srv = spawn(&live, &am(1, Some(SERVICE)));

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    assert_eq!(
        keys_of(&get),
        legacy_keys(),
        "v1 的 `@pending.get` 必须逐键等于那 4 个（AC-103 不回退）：{get}"
    );
    let text = serde_json::to_string(&get).unwrap();
    assert!(!text.contains("service"), "{text}");
    assert!(!text.contains("auth"), "{text}");
    eprintln!("· AC-002 · v1 @pending[0].get = {get}");

    drop(srv);
    cleanup(&live).await;
}

/// **AC-003（真进程 + 真库）** · v2 AM 但 `auth.service` 缺失 ⇒ 配方里**没有**这个键。
#[tokio::test]
async fn ac003_live_v2_without_service_has_no_service_key() {
    let test = "ac003_live_v2_without_service_has_no_service_key";
    let Some(live) = live("a3", test).await else { return };
    let srv = spawn(&live, &am(2, None));

    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    let auth = get
        .get("auth")
        .unwrap_or_else(|| panic!("v2 的 get 必须有 auth：{get}"));
    assert!(
        auth.get("service").is_none(),
        "AM 没写 service ⇒ 配方里不许出现它（⛔ 不塞空串、⛔ 不从 host 猜）：{auth}"
    );
    // ⚠️ 判**键**而不是子串：`scope` 里本来就有 `"service"` 这个段名（段名 ≠ auth.service）。
    let text = serde_json::to_string(&auth).unwrap();
    assert!(!text.contains(r#""service":"#), "{text}");
    eprintln!("· AC-003 · 无 service 的 auth = {auth}");

    drop(srv);
    cleanup(&live).await;
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 端到端：真回执里的配方 ⇒ 真脚本的 `--check`
// ══════════════════════════════════════════════════════════════

/// `access_fetch.py` 在哪：环境变量优先，否则取**部署树里的那一份**
/// （`<仓库根>/../Deploy/skills/ontok-query/scripts/access_fetch.py`）。
fn access_fetch() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ONTOK_ACCESS_FETCH") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let root = common::crate_root();
    let p = root
        .parent()
        .unwrap_or(&root)
        .join("Deploy")
        .join("skills")
        .join("ontok-query")
        .join("scripts")
        .join("access_fetch.py");
    p.is_file().then_some(p)
}

/// 找一个能跑的 Python（`python` / `py`）。找不到 ⇒ 返回 `None`。
fn python() -> Option<&'static str> {
    for exe in ["python", "py"] {
        if Command::new(exe)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(exe);
        }
    }
    None
}

/// **AC-004** · 把**真进程**发出的 `@pending.get` 原样（从 stdin，`--recipe -`）喂给
/// 部署脚本的 `--check`：不再因缺 `service` 被拒。
///
/// 判据不是「脚本 exit 0」（那可能只是没检查到），而是三件事同时成立：
/// ① exit 0；② 输出里**没有** `am.invalid` / 缺 `auth.service` 的拒绝原文；
/// ③ 输出里认出了那份配方的来源（`pending-recipe`）—— 证明它真的读了 stdin 那段配方。
#[tokio::test]
async fn ac004_live_recipe_feeds_access_fetch_check() {
    let test = "ac004_live_recipe_feeds_access_fetch_check";
    let Some(live) = live("a4", test).await else { return };
    let Some(script) = access_fetch() else {
        common::skip::missing(
            "Deploy/skills/ontok-query/scripts/access_fetch.py（可用 ONTOK_ACCESS_FETCH 指定）",
            test,
        );
        cleanup(&live).await;
        return;
    };
    let Some(py) = python() else {
        common::skip::missing("可执行的 Python（python / py）", test);
        cleanup(&live).await;
        return;
    };

    let srv = spawn(&live, &am(2, Some(SERVICE)));
    let receipt = sparql_jsonld(&srv);
    let get = pending_get_of(&receipt);
    // 前提：这一段确实带了 service（否则本用例测的就不是 AC-001 的产物）。
    assert_eq!(get["auth"]["service"], SERVICE, "{get}");
    // 「原样」= 把服务端发的那一段 `get` 序列化后直接当配方文件的内容。
    let recipe_json = serde_json::to_string(&get).expect("序列化配方");
    eprintln!("· AC-004 · 喂给脚本的配方 = {recipe_json}");

    let mut child = Command::new(py)
        .arg(&script)
        .args(["--recipe", "-", "--format", "rows", "--check"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("起 access_fetch.py");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(recipe_json.as_bytes())
        .expect("把配方写进 stdin");
    let out = child.wait_with_output().expect("等脚本结束");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    eprintln!("· AC-004 · exit = {:?}", out.status.code());
    eprintln!("· AC-004 · stdout:\n{stdout}");
    if !stderr.trim().is_empty() {
        eprintln!("· AC-004 · stderr:\n{stderr}");
    }

    let all = format!("{stdout}\n{stderr}");
    assert!(
        out.status.success(),
        "带着 service 的配方不该被 `--check` 拒（exit {:?}）：\n{all}",
        out.status.code()
    );
    // ⚠️ 不能拿 `contains("am.invalid")` / `contains("auth.service")` 判：`--check` 的
    // **图例**里本来就写着「缺则 am.invalid」「必填：… service」—— 那是说明文字，不是拒绝。
    // 判据用脚本自己的**结论行**与**错误符号**：只有真 error 才会打 `✗` 与「结果：拒绝」。
    for bad in ["结果：拒绝", "✗"] {
        assert!(
            !all.contains(bad),
            "`--check` 判了拒绝（出现 {bad:?}）—— 配方没把 service 带过去：\n{all}"
        );
    }
    assert!(
        all.contains("结果：通过"),
        "`--check` 没有给出「通过」的结论行：\n{all}"
    );
    assert!(
        all.contains("service") && all.contains("必填"),
        "脚本的 auth 键清单里应当点名 service 是必填（证明这条判据不是空跑）：\n{all}"
    );
    assert!(
        all.contains("pending-recipe"),
        "脚本没有把这份配方当配方读（没看到 pending-recipe）：\n{all}"
    );

    drop(srv);
    cleanup(&live).await;
}
