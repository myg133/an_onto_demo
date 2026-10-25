//! REQ-144 · **阶段 1 真调用通路**的离线判据（本地假端点，⛔ 不联网、⛔ 不用真凭据）。
//!
//! # 这份文件治的是什么
//!
//! 签名「对不对」由 `src/access/tc3.rs` 的**官方文档向量**钉住（那是最硬的判据）；
//! 这里钉的是签名**之外**的那条链：`--allow-network` 下的分页 / 重试 / 错误分类 /
//! 「签的 payload 就是发的 body」。
//!
//! | 判据 | 期望 |
//! | --- | --- |
//! | [`normal_two_pages_with_all_signed_headers`] | 2 页 ⇒ `meta.pages=2`，六个头齐全，签的 body == 发的 body |
//! | [`rate_limited_once_then_success`] | 429 一次 ⇒ 退避重试 1 次后成功（`retries=1`） |
//! | [`auth_rejected_is_not_retried`] | 401 ⇒ `AUTH_REJECTED`，**只发了 1 次** |
//! | [`missing_iterator_path_is_a_contract_error`] | 没有 `$.Response.InstanceSet` ⇒ `FIELD_PATH_MISSING` |
//! | [`timeout_is_reported_not_silently_empty`] | 服务端不响应 ⇒ `UPSTREAM_TIMEOUT`，⛔ 不返回空结果 |
//! | [`auth_missing_is_decided_before_any_request`] | 环境变量缺 ⇒ `AUTH_MISSING` |
//! | [`credentials_never_reach_debug_or_display`] | `Debug` 里没有 SecretKey |
//! | [`cli_network_requires_the_flag_and_the_env`] | CLI：两个开关都不给 ⇒ `DRY_RUN_REQUIRED`；给了但没凭据 ⇒ `AUTH_MISSING` |
//! | [`cli_fixture_path_has_no_network_receipt`] | 阶段 0 输出里**没有** `meta.network` 这一栏 |
//!
//! # 为什么用 `std::net::TcpListener` 手写假端点
//!
//! 本需求的硬约束是 **⛔ 不新增依赖**。起一个真 HTTP 服务器（axum / hyper 都在树里，
//! 但那是**服务端**框架，用它就得把「测试夹具」和「产品代码」绑在一起）——
//! 而这里只要「按脚本回几个字节」，手写 60 行就够，且**每个字节都可控**：
//! 超时用例要的就是「accept 之后不说话」，用框架反而要绕。

mod common;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ontok::access::fetch::parse_filter;
use ontok::access::net::{self, Credentials, HttpTimeouts, NetworkOptions, RetryPolicy};
use ontok::access::{parse, parse_ontology, Mapping};

/// 测试用占位凭据（腾讯云文档 / SDK 示例里那对公开值，**不是真凭据**）。
const ID: &str = concat!("AKID", "z8krbsJ5yKBZQpn74WFkmLPx3EXAMPLE");
const KEY: &str = concat!("Gu5t9xGARNpq86cd98joQYCN3", "EXAMPLE");
/// 钉死时间戳：⛔ 不许 `now()` 参与（那样每次跑出来的签名都不一样，等于没对拍）。
const TS: i64 = 1620000000;

// ══════════════════════════════════════════════════════════════
// 假端点（手写最小 HTTP/1.1）
// ══════════════════════════════════════════════════════════════

/// 服务端「怎么回」的脚本（每个连接消费一条）。
#[derive(Debug, Clone)]
enum Scripted {
    Respond(u16, String),
    /// accept 之后**不说话**（用来看客户端是否明确报超时）。
    Hang,
}

/// 记录下来的一个请求。
#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

impl Recorded {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }
}

struct FakeServer {
    addr: String,
    seen: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl FakeServer {
    fn start(script: Vec<Scripted>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("假端点应当能绑定 127.0.0.1:0");
        let addr = listener.local_addr().expect("local_addr").to_string();
        listener.set_nonblocking(true).expect("nonblocking");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let seen2 = Arc::clone(&seen);
        let stop2 = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            let mut script = script.into_iter();
            // `Hang` 的连接**不回、也不关**（挂着），否则客户端看到的是
            // 「连接被中止」而不是「超时」—— 那测的就不是超时那条判据了。
            let mut parked: Vec<TcpStream> = Vec::new();
            while !stop2.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut sock, _)) => {
                        // ⚠️ Windows 上 `accept` 出来的 socket **继承监听 socket 的非阻塞模式**：
                        // 不显式改回阻塞，`read` 会在数据还没到时立刻 WouldBlock，
                        // 于是假端点「什么都没回就把连接关了」—— 客户端看到的是
                        // WSAECONNABORTED（10053），而不是超时/正常响应。
                        let _ = sock.set_nonblocking(false);
                        let Some(req) = read_request(&mut sock) else {
                            continue;
                        };
                        seen2.lock().expect("seen").push(req);
                        match script.next() {
                            Some(Scripted::Respond(code, body)) => {
                                write_response(&mut sock, code, &body);
                                // ⚠️ 写完之后**不立刻 close**：Windows 上「写完就 close」
                                // 会在客户端还没读完时发 RST（WSAECONNABORTED 10053），
                                // 于是测的就不是我们要测的那条判据了。
                                // 响应里已经带 `Connection: close` ⇒ 客户端不会复用这条连接。
                                parked.push(sock);
                            }
                            Some(Scripted::Hang) => {
                                parked.push(sock);
                            }
                            None => write_response(
                                &mut sock,
                                500,
                                r#"{"Response":{"Error":{"Code":"FakeScriptExhausted","Message":"脚本用完了"}}}"#,
                            ),
                        }
                    }
                    Err(_) => thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Self {
            addr,
            seen,
            stop,
            handle: Some(handle),
        }
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
        let content_len = header_value(&head, "content-length")
            .and_then(|v| v.trim().parse::<usize>().ok())
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
        let mut lines = head.lines();
        let first = lines.next().unwrap_or("").to_string();
        let mut parts = first.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();
        let mut headers = BTreeMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        return Some(Recorded {
            method,
            path,
            headers,
            body,
        });
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn header_value(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

fn write_response(sock: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    let _ = sock.write_all(resp.as_bytes());
    let _ = sock.flush();
}

// ══════════════════════════════════════════════════════════════
// 夹具：契约 / 本体 / 选项
// ══════════════════════════════════════════════════════════════

fn root() -> PathBuf {
    common::crate_root()
}

/// 一份指向假端点的契约（结构与 `fixtures/access-cvm.json` 同形，只把 base_url 换掉）。
fn mapping_text(base_url: &str, max_scan_rows: u64) -> String {
    mapping_text_with_request(base_url, max_scan_rows, "{}")
}

/// 同上，但可以指定整个 `request` 段（模板 / 绑定 / 必填 / 分页路径）。
fn mapping_text_with_request(
    base_url: &str,
    max_scan_rows: u64,
    request_json: &str,
) -> String {
    format!(
        r#"{{
  "contract_version": 1,
  "mapping_version": 2,
  "project": "req144-network",
  "sources": [{{
    "id": "fake",
    "kind": "http-json",
    "base_url": "{base_url}",
    "auth": {{ "mode": "tc3-hmac-sha256", "credential_env": ["ONTOK_TEST_SECRET_ID_144", "ONTOK_TEST_SECRET_KEY_144"] }},
    "defaults": {{ "Region": "ap-guangzhou" }}
  }}],
  "entities": [{{
    "id": "cvm_instance",
    "class": "cvm_instance",
    "source": "fake",
    "reviewed_by": "qa@req144 (2026-10-25)",
    "operation": {{ "action": "DescribeInstances", "version": "2017-03-12", "method": "POST", "content_type": "application/json; charset=utf-8" }},
    "iterator": "$.Response.InstanceSet[*]",
    "identity": {{ "key_path": "InstanceId" }},
    "iri_template": "ontok:cvm_instance/{{id}}",
    "fields": {{ "name": {{ "path": "InstanceName" }}, "status": {{ "path": "InstanceState" }} }},
    "request": {request_json},
    "paging": {{ "offset_param": "Offset", "limit_param": "Limit", "max_limit": 2, "total_path": "$.Response.TotalCount" }},
    "capability": {{ "pushdown": ["zone", "ids"], "local_only": ["status"], "max_scan_rows": {max_scan_rows} }},
    "freshness": {{ "mode": "realtime" }},
    "fixture": "unused-in-network-path.json"
  }}]
}}"#
    )
}

/// 旧写法 `request.from_filter`（内部会被转成模板）。
fn mapping_text_with_from_filter(
    base_url: &str,
    max_scan_rows: u64,
    from_filter_json: &str,
) -> String {
    mapping_text_with_request(
        base_url,
        max_scan_rows,
        &format!("{{ \"from_filter\": {from_filter_json} }}"),
    )
}

fn ontology_iri() -> String {
    let text = std::fs::read_to_string(root().join("fixtures/access-ontology.ttl"))
        .expect("示例本体必须在 fixtures/ 里");
    parse_ontology(&text).iri
}

fn temp_mapping(tag: &str, base_url: &str, max_scan_rows: u64) -> (PathBuf, Mapping) {
    temp_mapping_with_request(tag, base_url, max_scan_rows, "{}")
}

fn temp_mapping_with_request(
    tag: &str,
    base_url: &str,
    max_scan_rows: u64,
    request_json: &str,
) -> (PathBuf, Mapping) {
    let dir = std::env::temp_dir().join(format!("ontok-req144-net-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("access.json");
    let text = mapping_text_with_request(base_url, max_scan_rows, request_json);
    std::fs::write(&path, &text).expect("写临时契约");
    let mapping = parse(&text).expect("临时契约应当能解析");
    (path, mapping)
}

fn temp_mapping_with_from_filter(
    tag: &str,
    base_url: &str,
    max_scan_rows: u64,
    from_filter_json: &str,
) -> (PathBuf, Mapping) {
    temp_mapping_with_request(
        tag,
        base_url,
        max_scan_rows,
        &format!("{{ \"from_filter\": {from_filter_json} }}"),
    )
}

fn options(limit: usize) -> NetworkOptions {
    NetworkOptions {
        filter: BTreeMap::new(),
        limit,
        timestamp: Some(TS),
        service: Some("cvm".to_string()),
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

fn page(ids: &[&str], total: usize) -> String {
    let rows: Vec<String> = ids
        .iter()
        .map(|id| {
            format!(
                r#"{{"InstanceId":"{id}","InstanceName":"name-{id}","InstanceState":"RUNNING","Placement":{{"Zone":"ap-guangzhou-2"}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"Response":{{"TotalCount":{total},"InstanceSet":[{}],"RequestId":"fake-request"}}}}"#,
        rows.join(",")
    )
}

fn err_code<T>(r: Result<T, ontok::access::AccessError>) -> ontok::access::AccessError {
    match r {
        Ok(_) => panic!("这里应当报错，但它成功了"),
        Err(e) => e,
    }
}

/// 用**同一个** tc3 实现重算期望的 `Authorization`。
///
/// ⚠️ 这不是「自己证明自己」：签名**绝对正确性**由 `src/access/tc3.rs` 的官方文档向量钉住；
/// 这里证明的是**传输层的自洽** —— 发出去的 body 与签进去的 payload 是同一串、
/// 头与签名里的 `host`/`service`/`timestamp` 一致（这正是「签一份、发另一份」那类 bug 的检测点）。
fn expected_authorization(payload: &str, host: &str) -> String {
    ontok::access::tc3::sign(&ontok::access::tc3::SignInput {
        secret_id: ID,
        secret_key: KEY,
        service: "cvm",
        host,
        action: "DescribeInstances",
        timestamp: TS,
        payload,
        content_type: "application/json; charset=utf-8",
        method: "POST",
        canonical_uri: "/",
        canonical_query: "",
    })
    .expect("签名应当成功")
    .authorization
}

// ══════════════════════════════════════════════════════════════
// 用例 1 · 正常返回两页
// ══════════════════════════════════════════════════════════════

#[test]
fn normal_two_pages_with_all_signed_headers() {
    let server = FakeServer::start(vec![
        Scripted::Respond(200, page(&["ins-1", "ins-2"], 3)),
        Scripted::Respond(200, page(&["ins-3"], 3)),
    ]);
    let (_path, mapping) = temp_mapping("pages", &server.url(), 5000);

    let doc = net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    )
    .expect("两页正常返回应当成功");

    assert_eq!(doc.meta.pages, 2, "3 行 / 每页 2 ⇒ 2 页");
    assert_eq!(doc.meta.rows_scanned, 3);
    assert_eq!(doc.graph.len(), 3, "三台实例应当组装成三个节点");
    assert!(!doc.meta.truncated, "--limit 50 大于总行数 ⇒ 不该标截断");

    let rec = doc
        .meta
        .network
        .as_ref()
        .expect("真调用必须有 meta.network");
    assert_eq!(rec.attempts, 2, "两页 = 两次 HTTP 尝试");
    assert_eq!(rec.retries, 0, "没有限流 ⇒ 不该有重试");
    assert_eq!(rec.http_status, 200);
    assert_eq!(rec.service, "cvm");
    assert_eq!(rec.signed_headers, "content-type;host;x-tc-action");
    assert_eq!(rec.host, server.addr);

    let seen = server.seen();
    assert_eq!(seen.len(), 2, "服务端应当收到两次请求");

    // 六个头必须齐全（缺一个服务端就拒）
    for (i, req) in seen.iter().enumerate() {
        assert_eq!(req.method, "POST", "第 {i} 个请求的方法");
        assert_eq!(req.path, "/");
        assert_eq!(
            req.header("content-type"),
            Some("application/json; charset=utf-8")
        );
        assert_eq!(req.header("host"), Some(server.addr.as_str()));
        assert_eq!(req.header("x-tc-action"), Some("DescribeInstances"));
        assert_eq!(req.header("x-tc-version"), Some("2017-03-12"));
        assert_eq!(req.header("x-tc-timestamp"), Some(TS.to_string().as_str()));
        assert_eq!(req.header("x-tc-region"), Some("ap-guangzhou"));
        let auth = req
            .header("authorization")
            .expect("必须有 Authorization 头");
        assert!(
            auth.starts_with(&format!(
                "TC3-HMAC-SHA256 Credential={ID}/2021-05-03/cvm/tc3_request"
            )),
            "Authorization 的 Credential 段不对：{auth}"
        );
        // ⭐ 签的 payload 必须**就是**发出去的 body
        assert_eq!(
            auth,
            expected_authorization(&req.body, &server.addr),
            "第 {i} 页的签名与它自己发的 body 对不上"
        );
    }

    // 分页参数真的在变（Offset 0 / 2），不是把第一页发了两遍
    assert!(
        seen[0].body.contains(r#""Offset":0"#),
        "第 1 页 body：{}",
        seen[0].body
    );
    assert!(
        seen[1].body.contains(r#""Offset":2"#),
        "第 2 页 body：{}",
        seen[1].body
    );
    assert!(
        seen[0].body.contains(r#""Limit":2"#),
        "每页上限来自 paging.max_limit：{}",
        seen[0].body
    );
    // Region 是**头**，不该塞进 body
    assert!(
        !seen[0].body.contains("Region"),
        "Region 不该进请求体：{}",
        seen[0].body
    );
}

// ══════════════════════════════════════════════════════════════
// 用例 1b · F4：**通用请求模板**（任意 JSON 形状 + 占位符绑定）
//
// 用户裁定：⛔ 不做厂商特例适配。请求体形状由作者逐字写在 `request.template` 里，
// 执行器只做占位符替换 ⇒ 顶层标量 / 数组对象 / 深层嵌套**一种代码路径**全覆盖。
// ══════════════════════════════════════════════════════════════

/// 形状 1 · 顶层标量 + 顶层数组（`{"Region":"x","Ids":["a","b"]}`）。
#[test]
fn template_renders_top_level_scalars_and_arrays() {
    let server = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-top",
        &server.url(),
        5000,
        r#"{ "template": { "Region": "{region}", "Ids": "{ids}" },
             "bindings": { "region": { "source": "default" }, "ids": { "source": "filter" } } }"#,
    );

    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"ids":["i-1","i-2"]}"#).unwrap();
    net::fetch_network(&mapping, &ontology_iri(), "cvm_instance", &opts, &creds())
        .expect("顶层标量 + 数组应当成功");

    // `Region` 同时出现在 defaults 与模板里 ⇒ 模板里那份进 body（⛔ 不悄悄改作者写的形状）
    assert_eq!(
        server.seen()[0].body,
        r#"{"Ids":["i-1","i-2"],"Limit":2,"Offset":0,"Region":"ap-guangzhou"}"#,
        "模板原样 + 分页路径"
    );
}

/// 形状 2 · 数组里放对象（`{"Clauses":[{"Field":"zone","Values":["y"]}]}`）。
#[test]
fn template_renders_array_of_objects() {
    let server = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-arr",
        &server.url(),
        5000,
        r#"{ "template": { "Clauses": [ { "Field": "zone", "Values": ["{zone}"] } ] },
             "bindings": { "zone": { "source": "filter" } } }"#,
    );

    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"zone":"ap-guangzhou-5"}"#).unwrap();
    net::fetch_network(&mapping, &ontology_iri(), "cvm_instance", &opts, &creds())
        .expect("数组对象应当成功");

    assert_eq!(
        server.seen()[0].body,
        r#"{"Clauses":[{"Field":"zone","Values":["ap-guangzhou-5"]}],"Limit":2,"Offset":0}"#,
        "数组对象按模板拼，值进 Values"
    );
}

/// 形状 3 · 深层嵌套（`{"Cond":{"Rules":[{"Metric":"m","Value":"v"}]}}`）。
#[test]
fn template_renders_deep_nesting() {
    let server = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-deep",
        &server.url(),
        5000,
        r#"{ "template": { "Cond": { "Rules": [ { "Metric": "m", "Value": "{zone}" } ] } },
             "bindings": { "zone": { "source": "filter" } } }"#,
    );

    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"zone":"v9"}"#).unwrap();
    net::fetch_network(&mapping, &ontology_iri(), "cvm_instance", &opts, &creds())
        .expect("深层嵌套应当成功");

    assert_eq!(
        server.seen()[0].body,
        r#"{"Cond":{"Rules":[{"Metric":"m","Value":"v9"}]},"Limit":2,"Offset":0}"#,
        "三层嵌套按模板原样拼"
    );
}

/// ⭐ **变异判据**：把模板里的嵌套拆掉一层，body 必须跟着变
/// ⇒ 证明它是**真按模板拼**的，不是「某个特例分支恰好长这样」。
#[test]
fn flattening_one_level_changes_the_body() {
    let deep = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_p1, m_deep) = temp_mapping_with_request(
        "mut-deep",
        &deep.url(),
        5000,
        r#"{ "template": { "Cond": { "Rules": [ { "Metric": "m", "Value": "{zone}" } ] } },
             "bindings": { "zone": { "source": "filter" } } }"#,
    );

    let flat = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_p2, m_flat) = temp_mapping_with_request(
        "mut-flat",
        &flat.url(),
        5000,
        r#"{ "template": { "Cond": { "Metric": "m", "Value": "{zone}" } },
             "bindings": { "zone": { "source": "filter" } } }"#,
    );

    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"zone":"v9"}"#).unwrap();
    net::fetch_network(&m_deep, &ontology_iri(), "cvm_instance", &opts, &creds()).unwrap();
    net::fetch_network(&m_flat, &ontology_iri(), "cvm_instance", &opts, &creds()).unwrap();

    let body_deep = &deep.seen()[0].body;
    let body_flat = &flat.seen()[0].body;
    assert_eq!(
        body_deep,
        r#"{"Cond":{"Rules":[{"Metric":"m","Value":"v9"}]},"Limit":2,"Offset":0}"#
    );
    assert_eq!(
        body_flat,
        r#"{"Cond":{"Metric":"m","Value":"v9"},"Limit":2,"Offset":0}"#
    );
    assert_ne!(body_deep, body_flat, "拆掉一层嵌套 ⇒ body 必须跟着变");
}

/// 可选过滤：没给 `--filter` ⇒ 带占位符的**数组元素整段消失**（⛔ 不发空值让服务端去猜）。
#[test]
fn missing_optional_filter_prunes_its_array_element() {
    let server = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-prune",
        &server.url(),
        5000,
        r#"{ "template": { "Region": "{region}",
                           "Clauses": [ { "Field": "zone", "Values": ["{zone}"] } ] },
             "bindings": { "region": { "source": "default" }, "zone": { "source": "filter" } } }"#,
    );

    // 不给 filter ⇒ Clauses 整段被剪掉，只剩 Region + 分页
    net::fetch_network(&mapping, &ontology_iri(), "cvm_instance", &options(50), &creds())
        .expect("可选过滤缺省时应当成功");

    assert_eq!(
        server.seen()[0].body,
        r#"{"Limit":2,"Offset":0,"Region":"ap-guangzhou"}"#,
        "缺省的可选过滤必须整段剪掉，⛔ 不留下空数组/空字符串"
    );
}

/// `required` 点名的绑定取不到值 ⇒ `REQUEST_BINDING_MISSING`（⛔ 不发半截请求）。
#[test]
fn required_binding_missing_is_an_error() {
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-required",
        "http://127.0.0.1:1",
        5000,
        r#"{ "required": ["zone"],
             "template": { "Clauses": [ { "Field": "zone", "Values": ["{zone}"] } ] },
             "bindings": { "zone": { "source": "filter" } } }"#,
    );

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_REQUEST_BINDING_MISSING, "{err}");
    assert!(err.message.contains("`zone`"), "要点名是哪个绑定：{err}");
    assert!(err.message.contains("--filter"), "要给出改法：{err}");
}

/// 嵌在文本里的占位符：缺值 ⇒ **报错**（⛔ 不写半截字符串）；多值 ⇒ 也报错（⛔ 不拼逗号）。
#[test]
fn embedded_placeholder_never_renders_half_a_string() {
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-embed",
        "http://127.0.0.1:1",
        5000,
        r#"{ "template": { "Tag": "id-{ids}" }, "bindings": { "ids": { "source": "filter" } } }"#,
    );

    // ① 没给值 ⇒ 报错，而不是发出 "id-"
    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    ));
    assert_eq!(err.code, ontok::access::CODE_REQUEST_BINDING_MISSING, "{err}");
    assert!(err.message.contains("嵌在文本里"), "{err}");

    // ② 多值 ⇒ 也报错（数组嵌不进字符串）
    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"ids":["a","b"]}"#).unwrap();
    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &opts,
        &creds(),
    ));
    assert_eq!(err.code, ontok::access::CODE_REQUEST_BINDING_MISSING, "{err}");
    assert!(err.message.contains("数组/对象"), "要说清为什么嵌不进去：{err}");
}

/// 不支持的请求形状（非 POST）⇒ **报不支持并点名**，⛔ 不偷偷适配。
#[test]
fn unsupported_body_shape_is_named_not_adapted() {
    let (_path, mapping) = temp_mapping_with_request(
        "tpl-shape",
        "http://127.0.0.1:1",
        5000,
        r#"{ "template": { "Zone": "{zone}" }, "bindings": { "zone": { "source": "filter" } } }"#,
    );

    // 把 method 改成 GET（其余不变）⇒ 应当在**发请求之前**就报 REQUEST_SHAPE_UNSUPPORTED
    let mut m = mapping.clone();
    m.entities[0].operation.method = "GET".to_string();
    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"zone":"z"}"#).unwrap();
    let err = err_code(net::fetch_network(&m, &ontology_iri(), "cvm_instance", &opts, &creds()));

    assert_eq!(err.code, ontok::access::CODE_REQUEST_SHAPE, "{err}");
    assert!(err.message.contains("GET"), "要点名契约里写的是什么：{err}");
    assert!(err.message.contains("POST"), "要给出改法：{err}");
}

/// 旧写法 `"zone": "Zone"` **保留兼容**（内部转成模板，⛔ 不是另一条运行时路径）。
#[test]
fn legacy_scalar_from_filter_still_works() {
    let server = FakeServer::start(vec![Scripted::Respond(200, page(&["ins-1"], 1))]);
    let (_path, mapping) = temp_mapping_with_from_filter(
        "legacyfilter",
        &server.url(),
        5000,
        r#"{ "zone": "Zone" }"#,
    );

    let mut opts = options(50);
    opts.filter = parse_filter(r#"{"zone":"ap-guangzhou-5"}"#).unwrap();
    net::fetch_network(&mapping, &ontology_iri(), "cvm_instance", &opts, &creds())
        .expect("旧写法必须仍然能跑");

    assert_eq!(
        server.seen()[0].body,
        r#"{"Limit":2,"Offset":0,"Zone":"ap-guangzhou-5"}"#,
        "旧写法就是顶层标量参数"
    );
}

// ══════════════════════════════════════════════════════════════
// 用例 2 · 429 一次后成功（退避重试 1 次）
// ══════════════════════════════════════════════════════════════

#[test]
fn rate_limited_once_then_success() {
    let server = FakeServer::start(vec![
        Scripted::Respond(
            429,
            r#"{"Response":{"Error":{"Code":"RequestLimitExceeded","Message":"请求超过频率限制"}}}"#
                .to_string(),
        ),
        Scripted::Respond(200, page(&["ins-1", "ins-2"], 2)),
    ]);
    let (_path, mapping) = temp_mapping("ratelimit", &server.url(), 5000);

    let doc = net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    )
    .expect("429 一次后应当退避重试并成功");

    let rec = doc.meta.network.as_ref().expect("meta.network");
    assert_eq!(
        rec.retries,
        1,
        "应当**恰好**重试 1 次（不是 0 次，也不是 2 次）；rec={rec:?} 服务端收到 {} 次",
        server.seen().len()
    );
    assert_eq!(rec.attempts, 2);
    assert_eq!(rec.http_status, 200, "最后一次是成功的那次");
    assert_eq!(doc.graph.len(), 2);
    assert_eq!(
        server.seen().len(),
        2,
        "服务端应当看到 2 次请求（第 1 次被限流）"
    );
}

// ══════════════════════════════════════════════════════════════
// 用例 3 · 401 不重试
// ══════════════════════════════════════════════════════════════

#[test]
fn auth_rejected_is_not_retried() {
    let server = FakeServer::start(vec![Scripted::Respond(
        401,
        r#"{"Response":{"Error":{"Code":"AuthFailure.SignatureFailure","Message":"签名校验失败"}}}"#
            .to_string(),
    )]);
    let (_path, mapping) = temp_mapping("auth", &server.url(), 5000);

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_AUTH_REJECTED, "{err}");
    assert_eq!(
        server.seen().len(),
        1,
        "⛔ 401 不许重试（服务端只该看到 1 次请求）"
    );
    // 报错要**可行动**：说清这不是重试能解决的
    assert!(err.message.contains("不重试"), "{err}");
    assert!(
        err.message.contains("5 分钟"),
        "要提示时钟偏差这个高频原因：{err}"
    );
}

// ══════════════════════════════════════════════════════════════
// 用例 4 · 响应缺 iterator 节点
// ══════════════════════════════════════════════════════════════

#[test]
fn missing_iterator_path_is_a_contract_error() {
    let server = FakeServer::start(vec![Scripted::Respond(
        200,
        // `Response` 在、`TotalCount` 在，但**没有** `InstanceSet`
        r#"{"Response":{"TotalCount":1,"RequestId":"fake-request"}}"#.to_string(),
    )]);
    let (_path, mapping) = temp_mapping("missingpath", &server.url(), 5000);

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_FIELD_PATH, "{err}");
    assert!(
        err.message.contains("$.Response.InstanceSet[*]"),
        "要点名是哪条路径：{err}"
    );
    assert!(
        err.message.contains("契约写错了"),
        "要说清责任在契约，不是「没数据」：{err}"
    );
    assert_eq!(server.seen().len(), 1, "契约错 ⇒ ⛔ 不重试");
}

/// 对照组：容器是**空数组**且 `TotalCount=0` ⇒ 这是**合法的空结果**，不是契约错。
#[test]
fn genuinely_empty_result_is_not_an_error() {
    let server = FakeServer::start(vec![Scripted::Respond(
        200,
        r#"{"Response":{"TotalCount":0,"InstanceSet":[],"RequestId":"fake-request"}}"#.to_string(),
    )]);
    let (_path, mapping) = temp_mapping("empty", &server.url(), 5000);

    let doc = net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    )
    .expect("0 条是合法结果");
    assert_eq!(doc.meta.rows_scanned, 0);
    assert_eq!(doc.meta.pages, 1, "确实发了一页请求");
    assert!(doc.graph.is_empty());
}

// ══════════════════════════════════════════════════════════════
// 用例 5 · 超时
// ══════════════════════════════════════════════════════════════

#[test]
fn timeout_is_reported_not_silently_empty() {
    let server = FakeServer::start(vec![Scripted::Hang, Scripted::Hang, Scripted::Hang]);
    let (_path, mapping) = temp_mapping("timeout", &server.url(), 5000);

    let mut opts = options(50);
    opts.timeouts = HttpTimeouts {
        connect: Duration::from_millis(150),
        global: Duration::from_millis(150),
    };
    opts.retry = RetryPolicy {
        max_attempts: 2,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(10),
    };

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &opts,
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_UPSTREAM_TIMEOUT, "{err}");
    assert!(err.message.contains("超时"), "必须明确说超时：{err}");
    assert!(
        err.message.contains("不返回空结果"),
        "⛔ 不许把超时静默成没数据：{err}"
    );
    assert!(
        !server.seen().is_empty(),
        "至少发过一次请求（否则这条判据测的是别的东西）"
    );
}

// ══════════════════════════════════════════════════════════════
// 用例 5b · 连不上（端口没开）⇒ UPSTREAM_UNREACHABLE（⛔ 不是 TIMEOUT）
// ══════════════════════════════════════════════════════════════

#[test]
fn connection_refused_is_unreachable_not_timeout() {
    // `127.0.0.1:1` 上没有监听 ⇒ 立刻 ConnectionRefused（不会等到超时）
    let (_path, mapping) = temp_mapping("unreachable", "http://127.0.0.1:1", 5000);

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_UPSTREAM_UNREACHABLE, "{err}");
    assert_ne!(
        err.code,
        ontok::access::CODE_UPSTREAM_TIMEOUT,
        "⛔ 连不上不许报成超时（改法完全不同）：{err}"
    );
    assert!(
        err.message.contains("安全组") || err.message.contains("端口"),
        "要指出该查端口 / 安全组：{err}"
    );
    assert!(
        !err.message.contains("--timeout-secs"),
        "⛔ 不可达不该建议调超时（治不好端口没开）：{err}"
    );
}

// ══════════════════════════════════════════════════════════════
// 用例 5c · 业务错误（4xx / 5xx）⇒ API_ERROR
// ══════════════════════════════════════════════════════════════

#[test]
fn api_error_4xx_is_reported_without_retry() {
    let server = FakeServer::start(vec![Scripted::Respond(
        400,
        r#"{"Response":{"Error":{"Code":"InvalidParameterValue","Message":"Zone 不合法"}}}"#
            .to_string(),
    )]);
    let (_path, mapping) = temp_mapping("api4xx", &server.url(), 5000);

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &options(50),
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_API_ERROR, "{err}");
    assert!(err.message.contains("InvalidParameterValue"), "要带服务端自己的码：{err}");
    assert!(err.message.contains("业务错误"), "{err}");
    assert_eq!(server.seen().len(), 1, "4xx 是请求本身不合法 ⇒ ⛔ 不重试");
}

#[test]
fn api_error_5xx_is_retried_with_a_cap_then_reported() {
    let server = FakeServer::start(vec![
        Scripted::Respond(503, "{}".to_string()),
        Scripted::Respond(503, "{}".to_string()),
    ]);
    let (_path, mapping) = temp_mapping("api5xx", &server.url(), 5000);

    let mut opts = options(50);
    opts.retry = RetryPolicy {
        max_attempts: 2,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(10),
    };

    let err = err_code(net::fetch_network(
        &mapping,
        &ontology_iri(),
        "cvm_instance",
        &opts,
        &creds(),
    ));

    assert_eq!(err.code, ontok::access::CODE_API_ERROR, "{err}");
    assert!(err.message.contains("5xx"), "要说清这是服务端 5xx：{err}");
    assert!(err.message.contains("已尝试 2 次"), "重试上限要写进消息：{err}");
    assert_eq!(server.seen().len(), 2, "5xx 可重试，但只到上限");
}

// ══════════════════════════════════════════════════════════════
// 案 6 · 凭据：缺 / 不泄露
// ══════════════════════════════════════════════════════════════

#[test]
fn auth_missing_is_decided_before_any_request() {
    let (_path, mapping) = temp_mapping("authmissing", "http://127.0.0.1:1", 5000);
    let source = mapping.source("fake").expect("source 在契约里");

    // 注入的 lookup 什么都不返回 ⇒ AUTH_MISSING（⛔ 不读全局环境，也就不会互相干扰）
    let err = err_code(Credentials::resolve(source, &|_| None));
    assert_eq!(err.code, ontok::access::CODE_AUTH_MISSING, "{err}");
    // 报错要点名**变量名**（契约声明的 + 官方标准名），⛔ 不回显任何值
    assert!(err.message.contains("ONTOK_TEST_SECRET_ID_144"), "{err}");
    assert!(err.message.contains("TENCENTCLOUD_SECRET_ID"), "{err}");
    assert!(err.message.contains("TENCENTCLOUD_SECRET_KEY"), "{err}");
    assert!(
        err.message.contains("--dry-run-fixture"),
        "要给出「没有凭据怎么办」：{err}"
    );
}

#[test]
fn credentials_come_from_the_declared_env_names_first() {
    let (_path, mapping) = temp_mapping("envnames", "http://127.0.0.1:1", 5000);
    let source = mapping.source("fake").expect("source 在契约里");

    // 契约声明的名字优先
    let creds = Credentials::resolve(source, &|name| match name {
        "ONTOK_TEST_SECRET_ID_144" => Some("id-from-contract".to_string()),
        "ONTOK_TEST_SECRET_KEY_144" => Some("key-from-contract".to_string()),
        _ => None,
    })
    .expect("声明的变量名有值 ⇒ 应当成功");
    assert_eq!(creds.secret_id(), "id-from-contract");

    // 声明的名字没设 ⇒ 回落到官方标准名
    let creds = Credentials::resolve(source, &|name| match name {
        "TENCENTCLOUD_SECRET_ID" => Some("id-canonical".to_string()),
        "TENCENTCLOUD_SECRET_KEY" => Some("key-canonical".to_string()),
        _ => None,
    })
    .expect("标准名有值 ⇒ 应当成功");
    assert_eq!(creds.secret_id(), "id-canonical");

    // 空白字符串**不算**设了（否则会拿一个空 key 去签名，然后在服务端报签名失败）
    let err = err_code(Credentials::resolve(source, &|_| Some("   ".to_string())));
    assert_eq!(err.code, ontok::access::CODE_AUTH_MISSING, "{err}");
}

#[test]
fn auth_mode_must_be_tc3_hmac_sha256() {
    // 把契约的 `auth.mode` 换成别的（阶段 1 只实现 TC3）⇒ **明确拒绝**，⛔ 不猜、不静默降级
    let text = mapping_text("http://127.0.0.1:1", 5000)
        .replace("\"tc3-hmac-sha256\"", "\"none\"");
    let mapping = parse(&text).expect("契约结构仍然合法（只是签名方式不支持）");
    let source = mapping.source("fake").expect("source 在契约里");

    let err = err_code(Credentials::resolve(source, &|name| {
        Some(format!("value-of-{name}"))
    }));
    assert_eq!(err.code, ontok::access::CODE_AUTH_MODE_UNSUPPORTED, "{err}");
    assert!(
        err.message.contains("tc3-hmac-sha256"),
        "要指出唯一支持的签名方式：{err}"
    );
    assert!(err.message.contains("none"), "要点名契约里写的是什么：{err}");
}

#[test]
fn credentials_never_reach_debug_or_display() {
    let c = Credentials::new("AKID-example-id", "super-secret-key");
    let text = format!("{c:?}");
    assert!(
        !text.contains("super-secret-key"),
        "Debug 里不许出现 SecretKey：{text}"
    );
    assert!(
        !text.contains("AKID-example-id"),
        "Debug 里也不打 SecretId：{text}"
    );
    assert!(text.contains("<redacted>"), "{text}");
}

// ══════════════════════════════════════════════════════════════
// 案 7 · CLI 闸门（开关 / 凭据 / 阶段 0 输出不变）
// ══════════════════════════════════════════════════════════════

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ontok")
}

/// 跑一次 CLI，返回 (退出码, stdout, stderr)。
fn run_cli(args: &[&str], extra_env: &[(&str, &str)]) -> (i32, String, String) {
    let mut cmd = std::process::Command::new(bin());
    cmd.args(args);
    // ⛔ 不让父进程环境里的真凭据漏进测试
    for k in [
        "TENCENT_SECRET_ID",
        "TENCENT_SECRET_KEY",
        "TENCENTCLOUD_SECRET_ID",
        "TENCENTCLOUD_SECRET_KEY",
    ] {
        cmd.env_remove(k);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("应当能启动 ontok");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn cli_network_requires_the_flag_and_the_env() {
    let (path, _mapping) = temp_mapping("cli", "http://127.0.0.1:1", 5000);
    let p = path.to_string_lossy().to_string();
    let ttl = root()
        .join("fixtures/access-ontology.ttl")
        .to_string_lossy()
        .to_string();

    // ① 两个开关都不给 ⇒ DRY_RUN_REQUIRED（⛔ 不联网、也不冒充 fixture 通路）
    let (code, _o, e) = run_cli(
        &[
            "access",
            "fetch",
            "--mapping",
            &p,
            "--ontology",
            &ttl,
            "--entity",
            "cvm_instance",
        ],
        &[],
    );
    assert_ne!(code, 0);
    assert!(e.contains("DRY_RUN_REQUIRED"), "stderr：{e}");

    // ② 给了 --allow-network 但环境里没凭据 ⇒ AUTH_MISSING。
    //    ⚠️ base_url 指向 127.0.0.1:1（必然拒绝连接）—— 若它偷偷发了请求，
    //    这里会变成 UPSTREAM_UNREACHABLE。断言是 AUTH_MISSING ⇒ **证明没发请求**。
    let (code, _o, e) = run_cli(
        &[
            "access",
            "fetch",
            "--mapping",
            &p,
            "--ontology",
            &ttl,
            "--entity",
            "cvm_instance",
            "--allow-network",
        ],
        &[],
    );
    assert_ne!(code, 0);
    assert!(e.contains("AUTH_MISSING"), "stderr：{e}");
    assert!(
        !e.contains("UPSTREAM_UNREACHABLE") && !e.contains("UPSTREAM_TIMEOUT"),
        "凭据缺失必须在**发请求之前**判定，实际 stderr：{e}"
    );

    // ③ 两个开关互斥（clap 直接拒，退出码 2）
    let (code, _o, e) = run_cli(
        &[
            "access",
            "fetch",
            "--mapping",
            &p,
            "--ontology",
            &ttl,
            "--entity",
            "cvm_instance",
            "--dry-run-fixture",
            "--allow-network",
        ],
        &[],
    );
    assert_eq!(code, 2, "clap 的参数冲突应当是 2：{e}");
}

/// ⭐ F5 · `--dump-raw`：把**服务端原文**（第一页）逐字节落盘，供替换合成 fixture。
///
/// 判据是「落盘文件 == 假端点回的那串字节」——⛔ 不是「重新序列化后相等」：
/// 真实响应的 `\uXXXX` 转义、键序、空白都要原样保留，否则脱敏/替换时会对不上。
#[test]
fn dump_raw_writes_first_page_verbatim() {
    let raw = page(&["ins-1", "ins-2"], 2);
    let server = FakeServer::start(vec![Scripted::Respond(200, raw.clone())]);
    let (map_path, _mapping) = temp_mapping("dumprun", &server.url(), 5000);
    let ttl = root().join("fixtures/access-ontology.ttl");
    let out = std::env::temp_dir().join(format!(
        "ontok-req144-dump-raw-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&out);

    let (code, _stdout, stderr) = run_cli(
        &[
            "access",
            "fetch",
            "--mapping",
            &map_path.to_string_lossy(),
            "--ontology",
            &ttl.to_string_lossy(),
            "--entity",
            "cvm_instance",
            "--limit",
            "50",
            "--allow-network",
            "--dump-raw",
            &out.to_string_lossy(),
        ],
        &[
            ("ONTOK_TEST_SECRET_ID_144", "placeholder-id-not-a-real-credential"),
            ("ONTOK_TEST_SECRET_KEY_144", "placeholder-key-not-a-real-credential"),
        ],
    );

    assert_eq!(code, 0, "stderr：{stderr}");
    let written = std::fs::read_to_string(&out).expect("--dump-raw 应当落盘");
    assert_eq!(written, raw, "落盘的必须是**服务端原文**（⛔ 不重新序列化）");
    assert!(
        !written.contains("placeholder-key-not-a-real-credential"),
        "⛔ 凭据不许进落盘文件"
    );
    assert!(stderr.contains("原始响应体已落盘"), "stderr 要说清落了什么：{stderr}");
    let _ = std::fs::remove_file(&out);
}

/// `--dump-raw` 只对真调用有意义：离线通路给它是**误用** ⇒ 明确拒绝（⛔ 不静默忽略）。
#[test]
fn dump_raw_requires_allow_network() {
    let (map_path, _mapping) = temp_mapping("dumprm", "http://127.0.0.1:1", 5000);
    let ttl = root().join("fixtures/access-ontology.ttl");
    let out = std::env::temp_dir().join(format!(
        "ontok-req144-dump-raw-bad-{}.json",
        std::process::id()
    ));

    let (code, _stdout, stderr) = run_cli(
        &[
            "access",
            "fetch",
            "--mapping",
            &map_path.to_string_lossy(),
            "--ontology",
            &ttl.to_string_lossy(),
            "--entity",
            "cvm_instance",
            "--dry-run-fixture",
            "--dump-raw",
            &out.to_string_lossy(),
        ],
        &[],
    );

    assert_ne!(code, 0);
    assert!(
        stderr.contains("--dump-raw") && stderr.contains("--allow-network"),
        "要说清它只对真调用有意义：{stderr}"
    );
    assert!(!out.exists(), "⛔ 误用时不写文件");
}

#[test]
fn cli_fixture_path_has_no_network_receipt() {
    // 阶段 0 的输出里**不该**出现 `network` 这一栏（R1：逐字节不变的另一半证明）
    let (code, stdout, stderr) = run_cli(
        &[
            "access",
            "fetch",
            "--mapping",
            &root().join("fixtures/access-cvm.json").to_string_lossy(),
            "--ontology",
            &root()
                .join("fixtures/access-ontology.ttl")
                .to_string_lossy(),
            "--entity",
            "cvm_instance",
            "--filter",
            r#"{"status":"STOPPED"}"#,
            "--limit",
            "50",
            "--dry-run-fixture",
        ],
        &[],
    );
    assert_eq!(code, 0, "stderr：{stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("stdout 必须是 JSON-LD");
    assert!(
        v["meta"].get("network").is_none(),
        "fixture 通路不该出现 meta.network：{}",
        v["meta"]
    );
    // 三项能力回执仍在（AC-008 不退化）
    for k in ["filtered_locally", "freshness", "capability_warnings"] {
        assert!(v["meta"].get(k).is_some(), "meta 缺 {k}");
    }
}
