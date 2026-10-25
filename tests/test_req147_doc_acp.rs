//! REQ-147 · **端到端**：`ontok doc ingest --agent acp`（真进程 + 假 ACP agent）。
//!
//! 这个文件回答的是「**不给内置 LLM、只给一个 ACP 子进程**时，本命令真的按 AC 跑了吗」，
//! 而不是「函数算得对不对」（那些在 `src/extract/doc_acp.rs` 的 `mod tests` 里）：
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | 不给 `--agent` ⇒ 走的仍是**内置 LLM**（报错点名 `ONTOK_LLM_BASE_URL`，一个字都不提 acp）|
//! | AC-002 | ACP 产物是**同一份 schema**：每行都有 `kind` / `evidence{file,chunk_id,quote,span}` / `modality` / `alignable` |
//! | AC-003 | 同一输入跑两次 ⇒ JSONL **sha256 相同** + 第二次 **0 次 prompt**（假 agent 的日志为空）|
//! | AC-004 | ①程序不存在 ②单轮超时 ③预算超限 ⇒ **各 exit≠0 且不出断言文件** |
//! | AC-005 | 编造的 quote ⇒ 该条**被丢** + `discarded_no_quote` 计数（变异：正常模式下该计数必须为 0）|
//!
//! # 为什么用 Node 写的假 agent
//!
//! 真的 `pi-acp` 一次往返 4~50 秒、依赖登录态、输出随模型漂移 —— 拿它当 CI 判据，
//! 得到的是「本地过、CI 挂」那种测试。假 agent 讲同一套 JSON-RPC（含**会话首轮前言**
//! 与**累计 usage**这两个真实特性），毫秒级、可复现、离线。
//!
//! ⛔ 本文件**不测协议**（协议在 `tests/test_acp_protocol.rs` 用录好的真实字节测）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_ontok");

/// 会把命令变成「内置 LLM」的环境变量 —— 每个用例都**先清干净**。
const LLM_ENVS: [&str; 5] = [
    "ONTOK_LLM_BASE_URL",
    "ONTOK_LLM_API_KEY",
    "ONTOK_LLM_MODEL",
    "ONTOK_LLM_MAX_TOKENS",
    "ONTOK_LLM_TIMEOUT_SECS",
];

fn crate_root() -> PathBuf {
    common::crate_root()
}

fn fake_acp_js() -> PathBuf {
    crate_root().join("tests/fixtures/req147/fake-acp.js")
}

/// Node 在不在？不在就**显式跳过**并说清原因（⛔ 不静默当作通过）。
fn require_node() -> bool {
    match Command::new("node").arg("--version").output() {
        Ok(o) if o.status.success() => true,
        _ => {
            eprintln!(
                "SKIP：本用例需要 `node`（假 ACP agent 用 Node 写）。\
                 它不是本 crate 的依赖，只在测试期用；装不上就跳过，\
                 ⛔ 不把它伪装成通过。"
            );
            false
        }
    }
}

/// 每个用例自己的输出目录（先删干净 —— 缓存文件会让「第一次」变成「第二次」）。
fn out_dir(tag: &str) -> PathBuf {
    let p = crate_root().join("out").join("req147").join(tag);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建输出目录");
    p
}

/// 写一份小语料（**自造**，不引入业务文档）。
fn write_corpus(dir: &Path) -> PathBuf {
    std::fs::create_dir_all(dir).expect("建语料目录");
    std::fs::write(
        dir.join("a.md"),
        "# 采集约束\n\n只读采集云证书与公网域名 TLS 握手结果。\n\n仅允许执行 Describe、List 类命令。\n",
    )
    .expect("写语料");
    dir.to_path_buf()
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn all(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

fn run(args: &[String], extra_env: &[(&str, String)]) -> Run {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(crate_root());
    for k in LLM_ENVS
        .iter()
        .copied()
        .chain(["ONTOK_DATABASE_URL", "ONTOK_TEST_DATABASE_URL"])
    {
        cmd.env_remove(k);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.args(args);
    let out = cmd.output().expect("跑不动 ontok 二进制");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// 一条标准调用：`--agent acp` + 假 agent 命令 + 夹具契约。
///
/// 夹具的**行为**（正常 / 混合 / 空 / 卡死）不走命令行，走环境变量
/// `FAKE_ACP_MODE` / `FAKE_ACP_LOG` —— 它们是**对端**的属性，不是本命令的参数。
fn acp_args(input: &Path, out: &Path, stats: &Path, cache: &Path) -> Vec<String> {
    vec![
        "doc".into(),
        "ingest".into(),
        "--input".into(),
        input.to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
        "--stats".into(),
        stats.to_string_lossy().into_owned(),
        "--cache".into(),
        cache.to_string_lossy().into_owned(),
        "--agent".into(),
        "acp".into(),
        "--agent-command".into(),
        format!("node {}", fake_acp_js().display()),
        "--agent-skill".into(),
        skill_fixture().to_string_lossy().into_owned(),
        "--budget-tokens".into(),
        "1000000".into(),
    ]
}

/// 夹具用的小契约（**通用**、不含任何行业/项目名词）。
fn skill_fixture() -> PathBuf {
    crate_root().join("tests/fixtures/req147/skill-fixture.md")
}

/// 跑一把假 agent：`mode` 决定它怎么答，`log` 记它收到几条 prompt。
fn run_fake(args: Vec<String>, mode: &str, log: &Path) -> Run {
    run(
        &args,
        &[
            ("FAKE_ACP_MODE", mode.to_string()),
            ("FAKE_ACP_LOG", log.to_string_lossy().into_owned()),
        ],
    )
}

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).expect("读产物"))
        .unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", p.display()))
}

fn lines_of(p: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(p).expect("读 JSONL");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("每一行都是合法 JSON"))
        .collect()
}

fn sha_of(p: &Path) -> String {
    let bytes = std::fs::read(p).expect("读文件");
    // 与 `extract::sample::sha256_hex` 同口径（这里只比相等，不解释它）
    ontok::extract::sample::sha256_hex(&bytes)
}

/// 假的内置 LLM 端点（标准库 TCP，零依赖）—— **只为了证明它一次都没被调用**。
///
/// # 为什么 AC-004a 需要一个「能用的」内置端点
///
/// 「⛔ 不许静默回退内置 LLM」这条约束只有在**内置那侧确实可用**时才测得出来：
/// 如果内置也必然失败，那么「回退了」和「没回退」在退出码与产物上**长得一样**，
/// 判据就成了空话（这正是本项目反复点名的「恒真判据」）。
///
/// 所以这里起一个**会成功**的假端点，并数它的请求数：
/// 静默回退一旦存在，`hits` 必然 > 0，而本用例断言它**恒为 0**。
struct FakeLlm {
    base_url: String,
    hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl FakeLlm {
    fn hits(&self) -> usize {
        self.hits.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn spawn_fake_llm() -> FakeLlm {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定假 LLM 端点");
    let port = listener.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            s.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();
            let mut data: Vec<u8> = Vec::new();
            let mut buf = [0u8; 4096];
            let mut header_end: Option<usize> = None;
            let mut content_len = 0usize;
            loop {
                let n = match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                data.extend_from_slice(&buf[..n]);
                if header_end.is_none() {
                    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos);
                        let head = String::from_utf8_lossy(&data[..pos]).to_ascii_lowercase();
                        content_len = head
                            .split("\r\n")
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                    }
                }
                if let Some(pos) = header_end {
                    if data.len() >= pos + 4 + content_len {
                        break;
                    }
                }
            }
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // 只解析正文（含 HTTP 头直接解析必然失败 —— REQ-116 第一版就这么错过）
            let body = match header_end {
                Some(pos) => String::from_utf8_lossy(
                    &data[pos + 4..(pos + 4 + content_len).min(data.len())],
                )
                .into_owned(),
                None => String::new(),
            };
            let chunk_text = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v["messages"]
                        .as_array()
                        .and_then(|a| a.iter().find(|m| m["role"] == "user"))
                        .and_then(|m| m["content"].as_str())
                        .and_then(|c| serde_json::from_str::<Value>(c).ok())
                        .and_then(|u| u["text"].as_str().map(str::to_string))
                })
                .unwrap_or_default();
            let quote: String = chunk_text.chars().take(20).collect();
            let content = serde_json::json!({
                "concepts": [{"name": "内置概念", "quote": quote, "modality": "fact"}],
                "relations": []
            })
            .to_string();
            let resp_body = serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content},
                             "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 900, "completion_tokens": 100, "total_tokens": 1000}
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    FakeLlm { base_url: format!("http://127.0.0.1:{port}/v1"), hits }
}

// ──────────────────────────────────────────────────────────
// AC-001 · 默认路径没被换掉
// ──────────────────────────────────────────────────────────

/// 不给 `--agent` ⇒ 仍然走内置 LLM：报错里点名的是 **LLM 三件套**，
/// 一个字都不提 `acp`（这正是 R1「默认关闭」在**行为**上的证据）。
#[test]
fn ac001_without_agent_still_goes_to_builtin_llm() {
    let o = out_dir("ac001");
    let corpus = write_corpus(&o.join("docs"));
    let r = run(
        &[
            "doc".into(),
            "ingest".into(),
            "--input".into(),
            corpus.to_string_lossy().into_owned(),
            "--out".into(),
            o.join("a.jsonl").to_string_lossy().into_owned(),
        ],
        &[],
    );
    assert_ne!(r.code, 0, "没配 LLM 三件套 ⇒ 必须报错");
    let all = r.all();
    assert!(all.contains("ONTOK_LLM_BASE_URL"), "必须点名缺的那个变量：{all}");
    assert!(
        !all.contains("--agent-command") && !all.contains("agent-skill"),
        "默认路径不该出现 ACP 的参数名：{all}"
    );
    assert!(!o.join("a.jsonl").exists(), "失败时不许出产物");
}

// ──────────────────────────────────────────────────────────
// AC-002 / AC-003 / AC-005 · 真进程跑通
// ──────────────────────────────────────────────────────────

/// AC-002 + AC-005 正常侧 + AC-003：跑两次。
///
/// | 判据 | 为什么这么判 |
/// | --- | --- |
/// | 每行都有 `kind`/`evidence{…}`/`modality`/`alignable` | 与内置通道**同一份** `render_jsonl` ⇒ schema 一致是结构性的，这条只是把它钉住 |
/// | 两次 sha256 相同 | R2 的确定性 |
/// | 假 agent 的日志第二次为**空** | 「0 次调用」不是看屏幕上的数字，是看对端收到了几条 prompt |
#[test]
fn ac002_ac003_acp_ingest_is_schema_identical_and_byte_stable() {
    if !require_node() {
        return;
    }
    let o = out_dir("ac002_003");
    let corpus = write_corpus(&o.join("docs"));
    let (out, stats, cache) = (o.join("a.jsonl"), o.join("s.json"), o.join("c.json"));
    let log1 = o.join("calls1.log");
    let log2 = o.join("calls2.log");

    let r1 = run_fake(acp_args(&corpus, &out, &stats, &cache), "valid", &log1);
    assert_eq!(r1.code, 0, "第一次必须成功：{}", r1.all());

    // ── AC-002：schema ──
    let rows = lines_of(&out);
    assert!(!rows.is_empty(), "必须有断言：{}", r1.all());
    for row in &rows {
        let kind = row["kind"].as_str().expect("每行都有 kind");
        assert!(kind == "concept" || kind == "relation", "未知 kind：{kind}");
        let ev = &row["evidence"];
        for f in ["file", "chunk_id", "quote", "span"] {
            assert!(!ev[f].is_null(), "evidence.{f} 必须存在：{row}");
        }
        assert!(ev["span"].as_array().is_some_and(|a| a.len() == 2), "span 必须是两元数组");
        let m = row["modality"].as_str().expect("modality 必填");
        assert!(
            ["fact", "spec", "future", "example"].contains(&m),
            "modality 越界：{m}"
        );
        assert!(row["alignable"].is_boolean(), "alignable 必须显式给布尔");
        if kind == "concept" {
            assert!(!row["name"].as_str().unwrap_or("").is_empty());
        } else {
            for f in ["subject", "predicate", "object"] {
                assert!(!row[f].as_str().unwrap_or("").is_empty(), "relation.{f} 不能空");
            }
        }
    }

    // ── AC-005 正常侧：一条都没被丢（变异判据的另一半）──
    let s1 = read_json(&stats);
    assert_eq!(s1["rejected"]["no_evidence"], 0, "正常回答不该有丢弃：{s1}");
    assert!(
        r1.stderr.contains("discarded_no_quote = 0"),
        "屏幕上必须报 quote 校验：{}",
        r1.stderr
    );

    // ── AC-003：第二次全命中 ⇒ 0 次 prompt + 逐字节相同 ──
    let sha_first = sha_of(&out);
    let r2 = run_fake(acp_args(&corpus, &out, &stats, &cache), "valid", &log2);
    assert_eq!(r2.code, 0, "第二次必须成功：{}", r2.all());
    assert_eq!(sha_of(&out), sha_first, "两次运行的 JSONL 必须**逐字节相同**");
    let calls2 = std::fs::read_to_string(&log2).unwrap_or_default();
    assert!(
        calls2.trim().is_empty(),
        "第二次运行必须是 **0 次 prompt**（含预热），实际收到了：{calls2}"
    );
    let s2 = read_json(&stats);
    assert_eq!(s2["calls"], 0, "stats.calls 必须为 0");
    assert_eq!(s2["cache_hits"], s2["chunks"], "每一块都必须命中缓存");
}

// ──────────────────────────────────────────────────────────
// AC-005 · 变异：编造的 quote
// ──────────────────────────────────────────────────────────

/// ★ **AC-005 的变异**：让 agent 回一条原文里**没有**的 quote ⇒
/// 该条必须被丢，且 `discarded_no_quote`（= `stats.rejected.no_evidence`）恰好 +1/块。
///
/// 这一条是本需求的核心价值：没有它，ACP 就只是「更慢的内置」。
#[test]
fn ac005_forged_quote_from_acp_agent_is_dropped_and_counted() {
    if !require_node() {
        return;
    }
    let o = out_dir("ac005");
    let corpus = write_corpus(&o.join("docs"));
    let (out, stats, cache) = (o.join("a.jsonl"), o.join("s.json"), o.join("c.json"));
    let log = o.join("calls.log");

    let r = run_fake(acp_args(&corpus, &out, &stats, &cache), "mixed", &log);
    assert_eq!(r.code, 0, "混合模式必须能跑完（有概念活下来）：{}", r.all());

    let rows = lines_of(&out);
    assert!(!rows.is_empty(), "概念必须留下来（否则本用例测不到「只丢那一条」）");
    // 夹具在每一块里都回一条**编造的** relation ⇒ 一条都不许进产物
    for row in &rows {
        assert_eq!(row["kind"], "concept", "编造的 relation 不许进 JSONL：{row}");
        assert!(
            !row["evidence"]["quote"].as_str().unwrap_or("").contains("客户向供应商采购"),
            "编造的 quote 一个字都不许出现：{row}"
        );
    }
    let s = read_json(&stats);
    assert_eq!(
        s["rejected"]["no_evidence"], s["chunks"],
        "每一块的那条编造 relation 都要被丢并计数：{s}"
    );
    assert!(
        r.stderr.contains("discarded_no_quote = "),
        "屏幕上必须报这个计数：{}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("discarded_no_quote = 0"),
        "变异必须让计数**不为 0**：{}",
        r.stderr
    );
}

// ──────────────────────────────────────────────────────────
// AC-004 · 三种失败各一次，且**不出产物**
// ──────────────────────────────────────────────────────────

/// ① `--agent-command` 指向**不存在**的程序 ⇒ exit≠0、点名、不出产物、明说不回落。
///
/// ★ 内置 LLM **在这一轮是能用的**（假端点会成功）——
/// 于是「静默回退」一旦存在，退出码会变 0、产物会出现、`hits` 会 > 0，
/// 三条断言同时红。这是「⛔ 不许静默回退」这条约束**唯一的**有效判据。
#[test]
fn ac004a_missing_agent_program_exits_nonzero_without_output() {
    let o = out_dir("ac004a");
    let corpus = write_corpus(&o.join("docs"));
    let out = o.join("a.jsonl");
    let fake = spawn_fake_llm();
    let r = run(
        &[
            "doc".into(),
            "ingest".into(),
            "--input".into(),
            corpus.to_string_lossy().into_owned(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
            "--agent".into(),
            "acp".into(),
            "--agent-command".into(),
            "definitely-not-a-real-acp-program-req147".into(),
            "--agent-skill".into(),
            skill_fixture().to_string_lossy().into_owned(),
        ],
        &[
            ("ONTOK_LLM_BASE_URL", fake.base_url.clone()),
            ("ONTOK_LLM_API_KEY", "test-only-not-a-real-key".into()),
            ("ONTOK_LLM_MODEL", "fake-model-req147".into()),
        ],
    );
    assert_ne!(r.code, 0, "连不上必须 exit≠0：{}", r.all());
    let all = r.all();
    assert!(all.contains("definitely-not-a-real-acp-program-req147"), "必须点名程序：{all}");
    assert!(all.contains("没有回落"), "必须明说不回落内置 LLM：{all}");
    assert!(!out.exists(), "失败时**一个断言文件都不许有**");
    assert_eq!(
        fake.hits(),
        0,
        "⛔ **一次内置 LLM 都不许调用** —— 调了就说明发生了静默回退（那正是 R3 禁止的）"
    );
}

/// ② 单轮超时 ⇒ exit≠0、**不产出断言文件**（⛔ 不是「0 条断言的成功」）。
#[test]
fn ac004b_agent_timeout_exits_nonzero_without_output() {
    if !require_node() {
        return;
    }
    let o = out_dir("ac004b");
    let corpus = write_corpus(&o.join("docs"));
    let out = o.join("a.jsonl");
    let log = o.join("calls.log");
    let mut args = acp_args(&corpus, &out, &o.join("s.json"), &o.join("c.json"));
    // 把默认 180 秒压到 1 秒：夹具会**永不**回终响应，必须由客户端自己的闸失败
    args.push("--agent-timeout-secs".into());
    args.push("1".into());
    let r = run_fake(args, "hang", &log);
    assert_ne!(r.code, 0, "超时必须 exit≠0：{}", r.all());
    assert!(!out.exists(), "超时**不许**产出断言文件（半份 JSONL 最坏的形态）");
    let all = r.all();
    assert!(
        all.contains("没有收到终响应") || all.contains("每一次调用都失败了") || all.contains("超时"),
        "必须说清是超时：{all}"
    );
}

/// ③ 预算超限 ⇒ exit≠0、报「已用 / 上限」、**不产出半份**。
#[test]
fn ac004c_budget_exceeded_exits_nonzero_without_output() {
    if !require_node() {
        return;
    }
    let o = out_dir("ac004c");
    let corpus = write_corpus(&o.join("docs"));
    let out = o.join("a.jsonl");
    let log = o.join("calls.log");
    let mut args = acp_args(&corpus, &out, &o.join("s.json"), &o.join("c.json"));
    // 夹具每轮报 +1000（累计从 2000 起）⇒ 上限 1500 一定在第一块之后就越过
    let pos = args.iter().position(|a| a == "--budget-tokens").expect("acp_args 给了预算");
    args[pos + 1] = "1500".into();
    let r = run_fake(args, "valid", &log);
    assert_ne!(r.code, 0, "越预算必须 exit≠0：{}", r.all());
    assert!(!out.exists(), "越预算**不许**产出断言文件");
    let all = r.all();
    assert!(all.contains("预算超限"), "必须报预算超限：{all}");
    assert!(all.contains("已用") && all.contains("上限"), "必须报「已用 / 上限」：{all}");
}

