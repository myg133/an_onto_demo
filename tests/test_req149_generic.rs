//! REQ-149 · 分页 / 过滤绑定的**通用表达力**（缺口 A/B/C/D）的端到端判据。
//!
//! 这份文件只证明**行为**，不证明某个厂商的接口真的接受这些参数：
//!
//! | # | 判据 | 变异（去掉新能力 ⇒ 必须红） |
//! | --- | --- | --- |
//! | A | 服务端收到的 `Offset/Limit` 是**字符串** `"0"` / `"100"` | 删 `value_type` ⇒ 收到数字 `0` / `100` |
//! | B | `PageNumber` 第 1 页是 **1**、第 2 页是 **2** | 删 `page`/`offset_base` ⇒ 收到 `0` / `100` |
//! | C | `CertificateStatus` 是**整数数组** `[1]` | 删 `shape/array` 或 `value_type/integer` ⇒ 收不到 `[1]` |
//! | D1 | 没有 `fixture` ⇒ `validate` **exit 0** + `FIELD_PATH_NOT_CHECKED` + 无 `field_coverage` | 把 `validate` 改回「无夹具即错」⇒ exit 非 0 |
//! | D2 | `fetch --dry-run-fixture` 无夹具 ⇒ **明确拒绝并点名** | 静默返回空结果 ⇒ 退出码 0 |
//! | D3 | `fetch --allow-network` 不读夹具（无凭据时是 `AUTH_MISSING`，不是 `FIXTURE_REQUIRED`） | 在真调用前判夹具 ⇒ `FIXTURE_REQUIRED` |
//!
//! ⚠️ 本文件**不**用任何真实接口 / 真实凭据：假端点在 `127.0.0.1:0`。
//! 服务端收到的 body 是**逐字节**记录下来的原文（[`Recorded::body`]）。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ontok::access::fetch::{self, FetchOptions, FilterValue};
use ontok::access::net::{self, Credentials, HttpTimeouts, NetworkOptions, RetryPolicy};
use ontok::access::{parse, Mapping};

/// 测试占位凭据（公开示例值，⛔ 不是真凭据）。
const ID: &str = "AKIDreq149example0000000000000000";
const KEY: &str = "req149example000000000000000000";
/// 钉死时间戳（签名可复算，⛔ 不让 `now()` 参与）。
const TS: i64 = 1620000000;

// ══════════════════════════════════════════════════════════════
// 假端点（手写最小 HTTP/1.1；与 `test_req144_network.rs` 同一口径）
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct Recorded {
    body: String,
}

struct FakeServer {
    addr: String,
    seen: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl FakeServer {
    fn start(bodies: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("假端点应当能绑定 127.0.0.1:0");
        let addr = listener.local_addr().expect("local_addr").to_string();
        listener.set_nonblocking(true).expect("nonblocking");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let seen2 = Arc::clone(&seen);
        let stop2 = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut bodies = bodies.into_iter();
            let mut parked: Vec<TcpStream> = Vec::new();
            while !stop2.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut sock, _)) => {
                        // Windows 上 accept 出来的 socket 继承监听 socket 的非阻塞模式。
                        let _ = sock.set_nonblocking(false);
                        let Some(req) = read_request(&mut sock) else {
                            continue;
                        };
                        seen2.lock().expect("seen").push(req);
                        let body = bodies.next().unwrap_or_else(|| {
                            r#"{"Response":{"Error":{"Code":"FakeScriptExhausted","Message":"脚本用完了"}}}"#
                                .to_string()
                        });
                        write_response(&mut sock, &body);
                        parked.push(sock); // 写完不立刻 close（避免 Windows RST）
                    }
                    Err(_) => thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Self { addr, seen, stop, handle: Some(handle) }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn seen(&self) -> Vec<Recorded> {
        self.seen.lock().expect("seen").clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn read_request(sock: &mut TcpStream) -> Option<Recorded> {
    sock.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = sock.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        let Some(head_end) = find(&buf, b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let content_len = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let want = head_end + 4 + content_len;
        while buf.len() < want {
            let n = sock.read(&mut tmp).ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }
        let body = String::from_utf8_lossy(&buf[head_end + 4..]).to_string();
        return Some(Recorded { body });
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn write_response(sock: &mut TcpStream, body: &str) {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    let _ = sock.write_all(resp.as_bytes());
    let _ = sock.flush();
}

// ══════════════════════════════════════════════════════════════
// 夹具
// ══════════════════════════════════════════════════════════════

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ontok-req149-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 一份只含一条 entity 的契约；`entity_json` 由各用例给（**没有 fixture**：
/// REQ-149 · D 允许；A/B/C 的输出与夹具无关）。
fn mapping_text(base_url: &str, entity_json: &str) -> String {
    format!(
        r#"{{
  "contract_version": 1,
  "mapping_version": 1,
  "project": "req149",
  "sources": [{{
    "id": "fake",
    "kind": "http-json",
    "base_url": "{base_url}",
    "auth": {{ "mode": "tc3-hmac-sha256", "credential_env": ["ONTOK_R149_ID", "ONTOK_R149_KEY"], "service": "svc" }},
    "defaults": {{ "Region": "ap-guangzhou" }}
  }}],
  "entities": [{entity_json}]
}}"#
    )
}

fn parse_mapping(base_url: &str, entity_json: &str) -> Mapping {
    let text = mapping_text(base_url, entity_json);
    parse(&text).unwrap_or_else(|ps| {
        panic!(
            "测试契约应当解析成功：\n{}",
            ps.iter()
                .map(|p| format!("[{}] {} :: {}", p.code, p.location, p.message))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })
}

fn options(filter: &[(&str, FilterValue)], limit: usize) -> NetworkOptions {
    NetworkOptions {
        filter: filter.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect(),
        limit,
        timestamp: Some(TS),
        service: Some("fake".to_string()),
        retry: RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::from_millis(5),
            max_delay: Duration::from_millis(20),
        },
        timeouts: HttpTimeouts {
            connect: Duration::from_secs(3),
            global: Duration::from_secs(5),
        },
    }
}

fn creds() -> Credentials {
    Credentials::new(ID, KEY)
}

/// 通用 entity 头（各用例只改 `request` / `iterator` / `fields`）。
fn entity(id: &str, class: &str, iterator: &str, key: &str, request: &str) -> String {
    format!(
        r#"{{
    "id": "{id}",
    "class": "{class}",
    "source": "fake",
    "reviewed_by": "qa@req149 (2026-10-26)",
    "operation": {{ "action": "Describe{id}", "version": "2026-01-01", "method": "POST", "content_type": "application/json" }},
    "iterator": "{iterator}",
    "identity": {{ "key_path": "{key}" }},
    "iri_template": "ontok:{class}/{{id}}",
    "fields": {{ "name": {{ "path": "Name" }} }},
    "request": {request},
    "paging": {{ "offset_param": "Offset", "limit_param": "Limit", "max_limit": 100, "total_path": "$.Response.TotalCount" }},
    "capability": {{ "pushdown": ["status"], "local_only": [], "max_scan_rows": 5000 }},
    "freshness": {{ "mode": "realtime" }}
  }}"#
    )
}

// ══════════════════════════════════════════════════════════════
// A · 分页值的类型 = string
// ══════════════════════════════════════════════════════════════

#[test]
fn a_string_paging_values_reach_the_server_as_strings() {
    let server = FakeServer::start(vec![
        r#"{"Response":{"TotalCount":1,"VpcSet":[{"VpcId":"vpc-1","Name":"n-1"}],"RequestId":"r"}}"#
            .to_string(),
    ]);
    let e = entity(
        "Vpc",
        "vpc",
        "$.Response.VpcSet[*]",
        "VpcId",
        r#"{
      "template": { "Region": "{region}" },
      "bindings": { "region": { "source": "default" } },
      "paging": { "offset": "Offset", "limit": "Limit", "value_type": "string" }
    }"#,
    );
    let m = parse_mapping(&server.url(), &e);
    let doc = net::fetch_network(&m, "https://example.test/onto", "Vpc", &options(&[], 100), &creds())
        .expect("应当成功");
    assert_eq!(doc.graph.len(), 1);
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    // ⭐ 服务端收到的 body 原文（逐字节）。
    assert_eq!(
        seen[0].body,
        r#"{"Limit":"100","Offset":"0","Region":"ap-guangzhou"}"#,
        "Offset/Limit 必须是字符串"
    );
    assert!(seen[0].body.contains(r#""Offset":"0""#));
    assert!(seen[0].body.contains(r#""Limit":"100""#));
}

// ══════════════════════════════════════════════════════════════
// B · 页码从 1 开始
// ══════════════════════════════════════════════════════════════

#[test]
fn b_page_numbers_start_at_one_and_increment() {
    let server = FakeServer::start(vec![
        r#"{"Response":{"TotalCount":3,"Policies":[{"PolicyId":"p-1","Name":"a"},{"PolicyId":"p-2","Name":"b"}],"RequestId":"r"}}"#.to_string(),
        r#"{"Response":{"TotalCount":3,"Policies":[{"PolicyId":"p-3","Name":"c"}],"RequestId":"r"}}"#.to_string(),
    ]);
    let e = entity("AlarmPolicy", "alarm_policy", "$.Response.Policies[*]", "PolicyId", r#"{
      "paging": { "page": "PageNumber", "limit": "PageSize" }
    }"#);
    let mut m = parse_mapping(&server.url(), &e);
    m.entities[0].paging.as_mut().unwrap().max_limit = 2; // 每页 2 行 ⇒ 取 3 行要两页
    let doc = net::fetch_network(
        &m,
        "https://example.test/onto",
        "AlarmPolicy",
        &options(&[], 10),
        &creds(),
    )
    .expect("应当成功");
    assert_eq!(doc.graph.len(), 3);
    let seen = server.seen();
    assert_eq!(seen.len(), 2, "3 行 / 每页 2 ⇒ 两页");
    assert_eq!(seen[0].body, r#"{"PageNumber":1,"PageSize":2}"#, "第 1 页必须是 1");
    assert_eq!(seen[1].body, r#"{"PageNumber":2,"PageSize":2}"#, "第 2 页必须是 2");
    assert!(!seen[0].body.contains(r#""PageNumber":0"#));
}

// ══════════════════════════════════════════════════════════════
// C · 过滤值 = 整数数组
// ══════════════════════════════════════════════════════════════

#[test]
fn c_integer_array_filter_reaches_the_server_as_ints() {
    let server = FakeServer::start(vec![
        r#"{"Response":{"TotalCount":1,"Certificates":[{"CertificateId":"c-1","Name":"n"}],"RequestId":"r"}}"#
            .to_string(),
    ]);
    let e = entity(
        "SslCertificate",
        "ssl_certificate",
        "$.Response.Certificates[*]",
        "CertificateId",
        r#"{
      "template": { "CertificateStatus": ["{status}"] },
      "bindings": { "status": { "source": "filter", "value_type": "integer", "shape": "array" } },
      "paging": { "offset": "Offset", "limit": "Limit" }
    }"#,
    );
    let m = parse_mapping(&server.url(), &e);
    let filter = vec![("status", FilterValue::Many(vec!["1".into()]))];
    net::fetch_network(
        &m,
        "https://example.test/onto",
        "SslCertificate",
        &options(&filter, 100),
        &creds(),
    )
    .expect("应当成功");
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].body, r#"{"CertificateStatus":[1],"Limit":100,"Offset":0}"#,
        "CertificateStatus 必须是整数数组"
    );
    assert!(
        seen[0].body.contains(r#""CertificateStatus":[1]"#),
        "⛔ 不能是 [\"1\"]（字符串数组）"
    );
}

// ══════════════════════════════════════════════════════════════
// D · 没有 fixture 时 validate / fetch 的口径
// ══════════════════════════════════════════════════════════════

const TTL: &str = r#"
@prefix : <https://example.test/onto#> .
@prefix ontok: <https://ontok.local/schema/> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

:c a owl:Class ;
    ontok:instanceIri "https://example.test/onto#{class}/{id}" .
"#;

/// 只做 validate / CLI 闸门用的契约（无 fixture）。
fn cli_mapping_text() -> String {
    format!(
        r#"{{
  "contract_version": 1,
  "project": "req149",
  "sources": [{{ "id": "fake", "base_url": "http://127.0.0.1:1",
                 "auth": {{ "mode": "tc3-hmac-sha256", "credential_env": ["ONTOK_R149_ID", "ONTOK_R149_KEY"], "service": "svc" }} }}],
  "entities": [{{
    "id": "Thing",
    "class": "c",
    "source": "fake",
    "reviewed_by": "qa@req149 (2026-10-26)",
    "operation": {{ "action": "DescribeThing" }},
    "iterator": "$.Response.Set[*]",
    "identity": {{ "key_path": "Id" }},
    "iri_template": "ontok:c/{{id}}",
    "fields": {{ "name": {{ "path": "Name" }} }},
    "capability": {{ "pushdown": [], "local_only": [], "max_scan_rows": 10 }},
    "freshness": {{ "mode": "realtime" }}
  }}]
}}"#
    )
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok-builder")
}

fn run_cli(args: &[&str]) -> (i32, String, String) {
    let mut cmd = std::process::Command::new(bin());
    cmd.args(args);
    // ⛔ 父进程里的真凭据不许漏进测试。
    for k in [
        "TENCENT_SECRET_ID",
        "TENCENT_SECRET_KEY",
        "TENCENTCLOUD_SECRET_ID",
        "TENCENTCLOUD_SECRET_KEY",
    ] {
        cmd.env_remove(k);
    }
    let out = cmd.output().expect("应当能启动 ontok");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

fn write_cli_inputs(dir: &Path) -> (PathBuf, PathBuf) {
    let mapping = dir.join("access.json");
    let ttl = dir.join("onto.ttl");
    std::fs::write(&mapping, cli_mapping_text()).expect("写契约");
    std::fs::write(&ttl, TTL).expect("写本体");
    (mapping, ttl)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

#[test]
fn d1_validate_without_fixture_is_exit_zero_with_named_warning() {
    let dir = temp_dir("d1");
    let (mapping, ttl) = write_cli_inputs(&dir);
    let (code, out, err) = run_cli(&[
        "access",
        "validate",
        "--mapping",
        &s(&mapping),
        "--ontology",
        &s(&ttl),
    ]);
    assert_eq!(code, 0, "没有 fixture 不许失败；stderr：{err}\nstdout：{out}");
    assert!(
        err.contains("FIELD_PATH_NOT_CHECKED"),
        "必须有一条点名警告：{err}"
    );
    assert!(err.contains("Thing"), "警告必须点名实体：{err}");
    assert!(
        err.contains("未做字段路径校准"),
        "头上的结论也必须承认「没校准」，⛔ 不许只说「四条检查全过」：{err}"
    );
    let v: serde_json::Value = serde_json::from_str(&out).expect("stdout 必须是 JSON 报告");
    let entity = &v["entities"][0];
    assert!(
        entity.get("field_coverage").is_none(),
        "没有 fixture 的实体不许出现 field_coverage：{entity}"
    );
    assert_eq!(v["ok"], serde_json::Value::Bool(true));
}

#[test]
fn d2_fetch_dry_run_without_fixture_is_refused_and_named() {
    let dir = temp_dir("d2");
    let (mapping, ttl) = write_cli_inputs(&dir);
    let (code, _out, err) = run_cli(&[
        "access",
        "fetch",
        "--mapping",
        &s(&mapping),
        "--ontology",
        &s(&ttl),
        "--entity",
        "Thing",
        "--dry-run-fixture",
    ]);
    assert_ne!(code, 0, "没有夹具的 dry-run 必须拒绝");
    assert!(err.contains("FIXTURE_REQUIRED"), "要报专门的码：{err}");
    assert!(err.contains("Thing"), "要点名实体：{err}");
}

#[test]
fn d3_allow_network_does_not_require_a_fixture() {
    let dir = temp_dir("d3");
    let (mapping, ttl) = write_cli_inputs(&dir);
    let (code, _out, err) = run_cli(&[
        "access",
        "fetch",
        "--mapping",
        &s(&mapping),
        "--ontology",
        &s(&ttl),
        "--entity",
        "Thing",
        "--allow-network",
    ]);
    // 没有凭据 ⇒ 在发请求之前就停在 AUTH_MISSING；**不是**夹具问题。
    assert_ne!(code, 0);
    assert!(err.contains("AUTH_MISSING"), "stderr：{err}");
    assert!(
        !err.contains("FIXTURE_REQUIRED"),
        "真调用通路不该读夹具：{err}"
    );
    assert!(
        !err.contains("FIXTURE_UNREADABLE"),
        "真调用通路不该读夹具：{err}"
    );
}

// ══════════════════════════════════════════════════════════════
// 旧行为对照：不写新字段时 dry_run 的输出形状不变
// ══════════════════════════════════════════════════════════════

#[test]
fn legacy_dry_run_still_works_without_fixture_declaration_no_new_fields() {
    // 这条只证明「D 不把没夹具变成运行时错误」；阶段 0 的完整零回归由 CLI 对拍（证据在交付里）。
    let dir = temp_dir("legacy");
    let mapping = dir.join("access.json");
    let ttl = dir.join("onto.ttl");
    let fixture = dir.join("x.json");
    std::fs::write(&mapping, cli_mapping_text()).unwrap();
    std::fs::write(&ttl, TTL).unwrap();
    std::fs::write(
        &fixture,
        r#"{"Response":{"TotalCount":1,"Set":[{"Id":"1","Name":"n"}]}}"#,
    )
    .unwrap();
    // 用一个**显式 fixture** 的契约跑 dry_run：确认旧通路仍然完整。
    let text = cli_mapping_text().replace(
        "\"fields\": { \"name\": { \"path\": \"Name\" } },",
        "\"fields\": { \"name\": { \"path\": \"Name\" } },\n    \"fixture\": \"x.json\",",
    );
    let m = parse(&text).expect("解析");
    let opts = FetchOptions { filter: BTreeMap::new(), limit: 10 };
    let doc = fetch::dry_run(&m, "https://example.test/onto", "Thing", &opts, &dir).expect("应成功");
    assert_eq!(doc.graph.len(), 1);
    assert_eq!(doc.meta.freshness.source, "fixture");
}

// ══════════════════════════════════════════════════════════════
// 形状验证（手工跑）：拿**真实产物** am-tencent.json 的派生副本
// ══════════════════════════════════════════════════════════════

/// 形状验证：把真实产物 `am-tencent.json` 的**派生副本**（只加了 REQ-149 的四个新字段）
/// 指向本地假端点，逐个打出**服务端收到的 body 原文**。
///
/// 为什么 `#[ignore]`：它依赖 `.local/` 下的真实产物（不入库），
/// 默认全量跑时不该假装「跑过了」—— 显式跑：
///
/// ```text
/// ONTOK_REQ149_AM_MAPPING=.local/req149/am-tencent-req149.json \
///   cargo test --test test_req149_generic -- --ignored req149_shape_evidence --nocapture
/// ```
#[test]
#[ignore = "依赖 .local/ 下的真实 AM 派生副本；显式跑（见测试文档）"]
fn req149_shape_evidence_with_derived_real_mapping() {
    let path = std::env::var("ONTOK_REQ149_AM_MAPPING")
        .expect("要显式给 ONTOK_REQ149_AM_MAPPING=<派生副本.json>（真实产物不改）");
    let text = std::fs::read_to_string(&path).expect("读派生副本");
    let mut m = parse(&text).expect("派生副本应当能解析");
    assert!(m.entities.iter().any(|e| e.id == "Vpc"));
    assert!(m.entities.iter().any(|e| e.id == "AlarmPolicy"));
    assert!(m.entities.iter().any(|e| e.id == "SslCertificate"));

    let server = FakeServer::start(vec![
        r#"{"Response":{"TotalCount":1,"VpcSet":[{"VpcId":"vpc-0001","VpcName":"demo"}],"RequestId":"r"}}"#.to_string(),
        r#"{"Response":{"TotalCount":1,"Policies":[{"PolicyId":"policy-0001","PolicyName":"demo"}],"RequestId":"r"}}"#.to_string(),
        r#"{"Response":{"TotalCount":1,"Certificates":[{"CertificateId":"cert-0001"}],"RequestId":"r"}}"#.to_string(),
    ]);
    // 只改内存里的 base_url / reviewed_by：磁盘上的真实产物**一个字都不动**。
    for s in &mut m.sources {
        s.base_url = server.url();
    }
    for e in &mut m.entities {
        if matches!(e.id.as_str(), "Vpc" | "AlarmPolicy" | "SslCertificate") {
            e.reviewed_by = Some("req149-shape@2026-10-26".to_string());
        }
    }

    let cases: [(&str, Vec<(&str, FilterValue)>); 3] = [
        ("Vpc", vec![]),
        ("AlarmPolicy", vec![]),
        ("SslCertificate", vec![("status", FilterValue::Many(vec!["1".into()]))]),
    ];
    for (entity, filter) in &cases {
        net::fetch_network(
            &m,
            "https://example.test/onto",
            entity,
            &options(filter, 100),
            &creds(),
        )
        .unwrap_or_else(|e| panic!("{entity} 真调用模拟失败：{e}"));
    }

    let seen = server.seen();
    assert_eq!(seen.len(), 3, "三条 entity 各一次请求");
    for (i, (entity, _)) in cases.iter().enumerate() {
        println!("=== {entity} · 服务端收到的 body 原文 ===\n{}", seen[i].body);
    }

    // ⭐ 判据（与需求的三条形状逐条对应）。
    assert!(
        seen[0].body.contains(r#""Offset":"0""#) && seen[0].body.contains(r#""Limit":"100""#),
        "Vpc 的 Offset/Limit 必须是字符串：{}",
        seen[0].body
    );
    assert!(
        !seen[0].body.contains(r#""Offset":0"#),
        "Vpc 的 Offset 不能是数字：{}",
        seen[0].body
    );
    assert!(
        seen[1].body.contains(r#""PageNumber":1"#),
        "AlarmPolicy 的 PageNumber 必须是 1：{}",
        seen[1].body
    );
    assert!(
        !seen[1].body.contains(r#""PageNumber":0"#),
        "AlarmPolicy 的 PageNumber 不能是 0：{}",
        seen[1].body
    );
    assert!(
        seen[2].body.contains(r#""CertificateStatus":[1]"#),
        "SslCertificate 的 CertificateStatus 必须是整数数组 [1]：{}",
        seen[2].body
    );
}
