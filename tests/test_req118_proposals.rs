//! REQ-118 · **端到端**：真进程 + 本地假 LLM 端点 + 夹具 schema。
//!
//! 这个文件回答的是「**没有产品库**的机器上，三条提案命令真的按 AC 跑了吗」：
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | 产物每一条都有 `claim/evidence/confidence/reason/decided_by/reviewed_by`，`reviewed_by = null` |
//! | AC-002 | `--as-names` 产物被 `ontok-propose --names` **直接消费**（真进程） |
//! | AC-003 | `exclude suggest` 产物被 `ontok-propose --exclude` **直接消费**（真进程） |
//! | AC-005 | 第二次运行：假端点**一次都没被再调**、文件 sha256 **相同** |
//! | AC-006 | `--budget-tokens 1` ⇒ exit≠0、**不产半份文件**、报「已用/上限/停在第几项」 |
//! | AC-007 | `exclude` 的 category 全部落在 REQ-113 的七类里；`concept` 的两种来源在单测里 |
//! | AC-008 | 两条命令都 `env_remove(ONTOK_DATABASE_URL / ONTOK_TEST_DATABASE_URL)` 仍跑通；源码里没有 `sqlx`/`PgPool` |
//!
//! # 为什么用「本地假端点」
//!
//! 真模型不保证每次吐同一份 JSON ⇒ 确定性判据（AC-005）与丢弃判据会随模型版本漂移。
//! 假端点是一个 `TcpListener`（标准库、零新依赖），按请求里的**表清单**回固定形状的回答，
//! 并**数请求次数**：不联网、不花钱、不需要真 key、可重复。
//!
//! # 夹具
//!
//! `tests/fixtures/req118/schema.json` 是自造的 4 张表：两张业务表（有注释）、
//! 一张迁移簿记表、一张 CDC 残留表 —— 刚好覆盖「保留 / 排除」两条路径。

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_ontok");
const BIN_PROPOSE: &str = env!("CARGO_BIN_EXE_ontok-propose");
const LLM_ENVS: [&str; 3] = ["ONTOK_LLM_BASE_URL", "ONTOK_LLM_API_KEY", "ONTOK_LLM_MODEL"];
const DB_ENVS: [&str; 2] = ["ONTOK_DATABASE_URL", "ONTOK_TEST_DATABASE_URL"];

fn crate_root() -> PathBuf {
    common::crate_root()
}

fn fixture_schema() -> PathBuf {
    crate_root().join("tests/fixtures/req118/schema.json")
}

// ──────────────────────────────────────────────────────────
// 假 LLM 端点
// ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Name,
    Exclude,
}

struct FakeLlm {
    base_url: String,
    hits: Arc<AtomicUsize>,
    _handle: Option<std::thread::JoinHandle<()>>,
}

impl FakeLlm {
    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// 从请求体里取出这一批的 `tables`（`messages[1].content` 是一个 JSON 字符串）。
fn tables_of(body: &str) -> Vec<Value> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let user = v["messages"]
        .as_array()
        .and_then(|a| a.iter().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    serde_json::from_str::<Value>(user)
        .ok()
        .and_then(|u| u.get("tables").and_then(|t| t.as_array()).cloned())
        .unwrap_or_default()
}

fn name_answer(tables: &[Value]) -> String {
    let proposals: Vec<Value> = tables
        .iter()
        .map(|t| {
            let table = t["table"].as_str().unwrap_or("");
            let (name, conf, extra) = match table {
                "customers" => ("客户", 0.93, "表注释：客户主档"),
                "orders" => ("订单", 0.91, "表注释：订单主表"),
                "alembic_version" => ("迁移版本台账", 0.75, "列 version_num（迁移版本号）"),
                "_volc_dts_pg_ddl_abc" => ("同步临时表", 0.50, "表名 _volc_dts_pg_ddl_abc"),
                _ => ("未知表", 0.40, "表名"),
            };
            serde_json::json!({
                "subject": table,
                "name": name,
                "claim": format!("这张表表达的业务对象是「{name}」"),
                "evidence": [format!("表名 {table}"), extra],
                "confidence": conf,
                "reason": if conf < 0.6 { "表名来自同步工具且无注释，信息不足" } else { "表注释与列注释一致" }
            })
        })
        .collect();
    serde_json::json!({"proposals": proposals}).to_string()
}

fn exclude_answer(tables: &[Value]) -> String {
    let proposals: Vec<Value> = tables
        .iter()
        .map(|t| {
            let table = t["table"].as_str().unwrap_or("");
            match table {
                "_volc_dts_pg_ddl_abc" => serde_json::json!({
                    "subject": table, "exclude": true, "category": "cdc_residue",
                    "claim": "这是同步工具生成的临时 DDL 表",
                    "evidence": [format!("表名 {table}"), "列 tag / ddl / txid 都是同步元数据"],
                    "confidence": 0.97,
                    "reason": "表名是同步服务的临时 DDL，列全部是同步元数据，没有业务含义"
                }),
                "alembic_version" => serde_json::json!({
                    "subject": table, "exclude": true, "category": "migration_bookkeeping",
                    "claim": "这是迁移框架的版本记账表",
                    "evidence": ["列 version_num（迁移版本号）"],
                    "confidence": 0.99,
                    "reason": "迁移工具的版本记账表，客户业务里没有对应的事物"
                }),
                "customers" => serde_json::json!({
                    "subject": table, "exclude": false,
                    "claim": "这是客户业务对象", "evidence": ["表注释：客户主档"],
                    "confidence": 0.92, "reason": "表注释明确写客户主档，列注释也是客户语义"
                }),
                "orders" => serde_json::json!({
                    "subject": table, "exclude": false,
                    "claim": "这是订单业务对象", "evidence": ["表注释：订单主表"],
                    "confidence": 0.9, "reason": "表注释明确写订单主表，含订单金额等业务列"
                }),
                _ => serde_json::json!({
                    "subject": table, "exclude": false,
                    "claim": "拿不准，保留", "evidence": [format!("表名 {table}")],
                    "confidence": 0.4, "reason": "信息不足以判断，按规则保留并标低置信"
                }),
            }
        })
        .collect();
    serde_json::json!({"proposals": proposals}).to_string()
}

fn spawn_fake(mode: Mode, total_tokens: u64) -> FakeLlm {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地假端点");
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            s.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();
            let mut data: Vec<u8> = Vec::new();
            let mut buf = [0u8; 4096];
            let mut header_end: Option<usize> = None;
            let mut content_len = 0usize;
            loop {
                let n = match s.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
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
            counter.fetch_add(1, Ordering::SeqCst);
            let body = match header_end {
                Some(pos) => {
                    let end = (pos + 4 + content_len).min(data.len());
                    String::from_utf8_lossy(&data[pos + 4..end]).into_owned()
                }
                None => String::new(),
            };
            let tables = tables_of(&body);
            let content = match mode {
                Mode::Name => name_answer(&tables),
                Mode::Exclude => exclude_answer(&tables),
            };
            let resp_body = serde_json::json!({
                "id": "chatcmpl-req118",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": content},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": total_tokens.saturating_sub(1),
                    "completion_tokens": 1,
                    "total_tokens": total_tokens
                }
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                resp_body.len(),
                resp_body
            );
            let _ = s.write_all(resp.as_bytes());
            let _ = s.flush();
        }
    });
    FakeLlm {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        hits,
        _handle: Some(handle),
    }
}

// ──────────────────────────────────────────────────────────
// 跑进程
// ──────────────────────────────────────────────────────────

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

fn run_with(bin: &str, args: &[String], fake: Option<&FakeLlm>) -> Run {
    let mut cmd = Command::new(bin);
    cmd.current_dir(crate_root());
    // AC-008：两个库连接串都清掉。能跑通 = 提案命令真的不连库。
    for k in LLM_ENVS.iter().copied().chain(DB_ENVS) {
        cmd.env_remove(k);
    }
    if let Some(f) = fake {
        cmd.env("ONTOK_LLM_BASE_URL", &f.base_url);
        // ⚠️ 假 key，**不是**任何真实凭据；只用于让 Bearer 头非空。
        cmd.env("ONTOK_LLM_API_KEY", "test-only-not-a-real-key");
        cmd.env("ONTOK_LLM_MODEL", "fake-model-req118");
    }
    cmd.args(args);
    let out = cmd.output().expect("跑不动二进制");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn run(args: &[String], fake: Option<&FakeLlm>) -> Run {
    run_with(BIN, args, fake)
}

fn out_dir(tag: &str) -> PathBuf {
    let p = crate_root().join("out").join("req118").join(tag);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建输出目录");
    p
}

fn name_args(out: &Path) -> Vec<String> {
    vec![
        "name".into(),
        "suggest".into(),
        "--schema".into(),
        fixture_schema().to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
    ]
}

fn exclude_args(out: &Path) -> Vec<String> {
    vec![
        "exclude".into(),
        "suggest".into(),
        "--schema".into(),
        fixture_schema().to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
    ]
}

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display())))
        .unwrap_or_else(|e| panic!("{} 不是合法 JSON：{e}", p.display()))
}

/// 文件的**原始字节**（AC-005 的判据比「sha256 相同」更强：逐字节相等）。
fn bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()))
}

// ──────────────────────────────────────────────────────────
// AC-001 / AC-007 / AC-009：形状
// ──────────────────────────────────────────────────────────

#[test]
fn name_suggest_produces_adr_proposal_shape_with_null_reviewed_by() {
    let fake = spawn_fake(Mode::Name, 37);
    let d = out_dir("name_shape");
    let out = d.join("names.proposal.json");
    let r = run(&name_args(&out), Some(&fake));
    assert_eq!(r.code, 0, "name suggest 必须成功：{}", r.all());
    assert!(r.stderr.contains("✓ 名字提案"), "要有回执：{}", r.stderr);

    let v = read_json(&out);
    assert_eq!(v["kind"], "name");
    assert_eq!(v["source"], "ai", "AC-007");
    assert_eq!(v["reviewed_by"], Value::Null, "P2：提案永不权威");
    assert_eq!(v["decided_by"], "ai:fake-model-req118");
    let ps = v["proposals"].as_array().expect("proposals 数组");
    assert_eq!(ps.len(), 4, "4 张表都要有提案：{ps:?}");
    for p in ps {
        for k in ["claim", "evidence", "confidence", "reason", "decided_by", "reviewed_by"] {
            assert!(p.get(k).is_some(), "AC-001：每条都要有 {k}");
        }
        assert_eq!(p["reviewed_by"], Value::Null);
        assert!(!p["evidence"].as_array().unwrap().is_empty(), "AC-009：evidence 不能空");
        assert!(!p["reason"].as_str().unwrap().trim().is_empty());
        assert!(p["confidence"].as_f64().unwrap() >= 0.0);
    }
    let low = ps
        .iter()
        .find(|p| p["subject"] == "_volc_dts_pg_ddl_abc")
        .unwrap();
    assert_eq!(low["low_confidence"], true, "R4：0.50 < 0.6 必须标低置信");
    assert_eq!(v["coverage"]["named"], 4);
    assert_eq!(v["coverage"]["missing"].as_array().unwrap().len(), 0);
    // AC-005 的另一半：运行经济学**不进文件**（否则第二次不可能逐字节相同）
    assert!(v["stats"].get("tokens_spent").is_none(), "token 不该进提案文件");
    assert!(v["stats"].get("cache_hits").is_none());
}

#[test]
fn exclude_suggest_decisions_use_the_seven_categories_and_null_reviewed_by() {
    let fake = spawn_fake(Mode::Exclude, 41);
    let d = out_dir("exclude_shape");
    let out = d.join("excl.proposal.json");
    let r = run(&exclude_args(&out), Some(&fake));
    assert_eq!(r.code, 0, "exclude suggest 必须成功：{}", r.all());

    let v = read_json(&out);
    assert_eq!(v["source"], "ai");
    assert_eq!(v["reviewed_by"], Value::Null);
    let ps = v["proposals"].as_array().unwrap();
    assert_eq!(ps.len(), 4, "每张表都要有显式判断（保留也是判断）");
    for p in ps {
        for k in ["claim", "evidence", "confidence", "reason", "decided_by", "reviewed_by"] {
            assert!(p.get(k).is_some(), "AC-001：每条都要有 {k}");
        }
        if p["exclude"] == true {
            let c = p["category"].as_str().expect("排除项必须有 category");
            assert!(
                [
                    "cdc_residue",
                    "migration_bookkeeping",
                    "backup_snapshot",
                    "internal_pipeline",
                    "report_intermediate",
                    "deprecated",
                    "out_of_scope"
                ]
                .contains(&c),
                "AC-003 / R3：category 必须是七类之一，收到 {c:?}"
            );
        }
    }
    let entries = v["exclude"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "两张噪声表被排除");
    assert_eq!(v["counts"]["judged"], 4);
    assert_eq!(v["counts"]["excluded"], 2);
    assert_eq!(v["counts"]["kept"], 2);
}

// ──────────────────────────────────────────────────────────
// AC-005：缓存
// ──────────────────────────────────────────────────────────

#[test]
fn name_suggest_second_run_is_zero_token_and_byte_identical() {
    let fake = spawn_fake(Mode::Name, 37);
    let d = out_dir("name_cache");
    let out = d.join("names.proposal.json");
    let r1 = run(&name_args(&out), Some(&fake));
    assert_eq!(r1.code, 0, "第一次：{}", r1.all());
    let hits1 = fake.hits();
    let sha1 = bytes(&out);
    assert!(hits1 > 0, "第一次必须真的调用模型");

    let r2 = run(&name_args(&out), Some(&fake));
    assert_eq!(r2.code, 0, "第二次：{}", r2.all());
    assert_eq!(fake.hits(), hits1, "第二次必须 0 次调用（P3：命中绝不调用）");
    assert_eq!(bytes(&out), sha1, "AC-005：第二次输出逐字节相同");
    assert!(r2.stderr.contains("命中 1 / 1 批"), "回执要报命中：{}", r2.stderr);
    assert!(r2.stderr.contains("本轮 0 / 上限"), "回执要报 0 token：{}", r2.stderr);
}

#[test]
fn exclude_suggest_second_run_is_zero_token_and_byte_identical() {
    let fake = spawn_fake(Mode::Exclude, 41);
    let d = out_dir("exclude_cache");
    let out = d.join("excl.proposal.json");
    assert_eq!(run(&exclude_args(&out), Some(&fake)).code, 0);
    let hits1 = fake.hits();
    let sha1 = bytes(&out);
    assert_eq!(run(&exclude_args(&out), Some(&fake)).code, 0);
    assert_eq!(fake.hits(), hits1, "0 次新调用");
    assert_eq!(bytes(&out), sha1, "逐字节相同");
}

// ──────────────────────────────────────────────────────────
// AC-006：预算闸
// ──────────────────────────────────────────────────────────

#[test]
fn budget_one_stops_and_writes_no_proposal_file() {
    let fake = spawn_fake(Mode::Name, 37);
    let d = out_dir("name_budget");
    let out = d.join("names.proposal.json");
    let mut args = name_args(&out);
    args.extend(["--budget-tokens".into(), "1".into()]);
    let r = run(&args, Some(&fake));
    assert_ne!(r.code, 0, "越预算必须 exit≠0：{}", r.all());
    assert!(!out.exists(), "AC-006：不产半份文件");
    let all = r.all();
    assert!(all.contains("预算超限"), "要报预算超限：{all}");
    assert!(all.contains("已用") || all.contains("37"), "要报已用：{all}");
    assert!(all.contains("停在第"), "要报停在第几项：{all}");
    assert!(
        d.join("names.proposal.json.cache.json").exists(),
        "已经花掉的钱对应的回答必须落盘（下次重跑不重复计费）"
    );
}

#[test]
fn exclude_budget_one_stops_and_writes_no_proposal_file() {
    let fake = spawn_fake(Mode::Exclude, 41);
    let d = out_dir("exclude_budget");
    let out = d.join("excl.proposal.json");
    let mut args = exclude_args(&out);
    args.extend(["--budget-tokens".into(), "1".into()]);
    let r = run(&args, Some(&fake));
    assert_ne!(r.code, 0, "越预算必须 exit≠0：{}", r.all());
    assert!(!out.exists(), "AC-006：不产半份文件");
    assert!(r.all().contains("停在第"), "要报停在第几项：{}", r.all());
}

// ──────────────────────────────────────────────────────────
// AC-002 / AC-003：与既有流程兼容（真跑 propose）
// ──────────────────────────────────────────────────────────

fn run_propose(extra: &[String]) -> Run {
    let mut args: Vec<String> = vec![
        "--schema".into(),
        fixture_schema().to_string_lossy().into_owned(),
        "--project".into(),
        "req118-fixture".into(),
    ];
    args.extend(extra.iter().cloned());
    run_with(BIN_PROPOSE, &args, None)
}

#[test]
fn as_names_is_consumable_by_ontok_propose_names() {
    let fake = spawn_fake(Mode::Name, 37);
    let d = out_dir("name_as_names");
    let out = d.join("names.proposal.json");
    let names = d.join("table_names.json");
    let mut args = name_args(&out);
    args.extend(["--as-names".into(), names.to_string_lossy().into_owned()]);
    let r = run(&args, Some(&fake));
    assert_eq!(r.code, 0, "{}", r.all());

    let nv = read_json(&names);
    assert_eq!(nv["agent"], "ai:fake-model-req118");
    assert_eq!(nv["_proposal"]["reviewed_by"], Value::Null, "P2 溯源也要 null");
    assert!(nv["_proposal"]["sha256"].as_str().unwrap().len() == 64);
    assert_eq!(nv["_coverage"]["named"], 4);

    let proposal = d.join("proposal.json");
    let stats = d.join("stats.json");
    let r2 = run_propose(&[
        "--names".into(),
        names.to_string_lossy().into_owned(),
        "--out".into(),
        proposal.to_string_lossy().into_owned(),
        "--stats".into(),
        stats.to_string_lossy().into_owned(),
        "--no-sample".into(),
    ]);
    assert_eq!(r2.code, 0, "AC-002：--as-names 产物必须能被 propose --names 直接消费：{}", r2.all());
    let pv = read_json(&proposal);
    let keys: Vec<&str> = pv["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .filter_map(|i| i["payload"]["key"].as_str())
        .collect();
    assert!(keys.contains(&"customers"), "提案里要有 customers：{keys:?}");
    assert!(keys.contains(&"orders"), "提案里要有 orders：{keys:?}");
    // 名字确实被用上了（不是又退回英文 key）
    let names_in_proposal: Vec<&str> = pv["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .filter_map(|i| i["payload"]["display_name"].as_str())
        .collect();
    assert!(names_in_proposal.contains(&"客户"), "display_name 应为提案里的中文名：{names_in_proposal:?}");
}

#[test]
fn exclude_proposal_is_consumed_by_ontok_propose_exclude() {
    let fake = spawn_fake(Mode::Exclude, 41);
    let d = out_dir("exclude_consume");
    let out = d.join("excl.proposal.json");
    assert_eq!(run(&exclude_args(&out), Some(&fake)).code, 0);

    // 还需要一份可用的名字表：用名字提案的 --as-names 产（不喂的话 propose 会缺显示名）。
    let names = d.join("table_names.json");
    let nout = d.join("names.proposal.json");
    let mut nargs = name_args(&nout);
    nargs.extend(["--as-names".into(), names.to_string_lossy().into_owned()]);
    let nfake = spawn_fake(Mode::Name, 37);
    assert_eq!(run(&nargs, Some(&nfake)).code, 0);

    let proposal = d.join("proposal.json");
    let stats = d.join("stats.json");
    let r = run_propose(&[
        "--names".into(),
        names.to_string_lossy().into_owned(),
        "--exclude".into(),
        out.to_string_lossy().into_owned(),
        "--out".into(),
        proposal.to_string_lossy().into_owned(),
        "--stats".into(),
        stats.to_string_lossy().into_owned(),
        "--no-sample".into(),
    ]);
    assert_eq!(r.code, 0, "AC-003：exclude 产物必须能被 propose --exclude 直接消费：{}", r.all());
    let sv = read_json(&stats);
    assert_eq!(sv["tables_seen"], 4);
    assert_eq!(sv["tables_excluded"], 2);
    assert_eq!(sv["tables_extracted"], 2);
    let pv = read_json(&proposal);
    let keys: Vec<&str> = pv["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "add_entity_type")
        .filter_map(|i| i["payload"]["key"].as_str())
        .collect();
    assert!(!keys.contains(&"alembic_version"), "被排除的表不许进提案：{keys:?}");
    assert!(!keys.contains(&"_volc_dts_pg_ddl_abc"), "被排除的表不许进提案：{keys:?}");
    assert!(keys.contains(&"customers"));
}

// ──────────────────────────────────────────────────────────
// AC-008：不写库 / 不冒充权威（结构判据）
// ──────────────────────────────────────────────────────────

#[test]
fn proposal_sources_have_no_database_write_path() {
    // 与 REQ-116 的 AC-008 同一条判据：源码里没有连接池 / 没有 db 模块引用，
    // 「不写库」就不是靠人记得，而是**结构上做不到**。
    for f in [
        "src/extract/proposal.rs",
        "src/extract/name_suggest.rs",
        "src/extract/exclude_suggest.rs",
    ] {
        let text = std::fs::read_to_string(crate_root().join(f)).expect("读源码");
        for banned in ["sqlx", "PgPool", "PgStore", "db::apply", "INSERT INTO", "UPDATE "] {
            assert!(
                !text.contains(banned),
                "{f} 里出现了 {banned:?} —— 提案模块必须零写库能力"
            );
        }
    }
}

#[test]
fn commands_refuse_to_run_without_out_and_without_llm_env() {
    let d = out_dir("negative");
    let out = d.join("names.proposal.json");
    // 缺 --out：clap 直接 exit 2（P1：必须落盘）
    let r = run(
        &[
            "name".into(),
            "suggest".into(),
            "--schema".into(),
            fixture_schema().to_string_lossy().into_owned(),
        ],
        None,
    );
    assert_ne!(r.code, 0);
    assert!(r.all().contains("--out"), "要提示缺 --out：{}", r.all());

    // 有 --out 但缺 LLM 三件套：必须 exit≠0 并点名缺哪个变量，且不写产物
    let r = run(&name_args(&out), None);
    assert_ne!(r.code, 0);
    assert!(r.all().contains("ONTOK_LLM_BASE_URL"), "要点名缺的变量：{}", r.all());
    assert!(!out.exists(), "没跑成就不许出产物");
}

#[test]
fn help_lists_the_three_decision_commands() {
    for args in [
        vec!["name".to_string(), "suggest".to_string(), "--help".to_string()],
        vec!["exclude".to_string(), "suggest".to_string(), "--help".to_string()],
        vec!["concept".to_string(), "suggest".to_string(), "--help".to_string()],
    ] {
        let r = run(&args, None);
        assert_eq!(r.code, 0, "{args:?} --help 必须成功：{}", r.all());
    }
    let r = run(&["concept".into(), "suggest".into(), "--help".into()], None);
    assert!(r.all().contains("--ai"), "concept suggest 的 help 必须写出 --ai");
    assert!(r.all().contains("source = \"rule\"") || r.all().contains("算法版"), "要写清降级标注");
}

