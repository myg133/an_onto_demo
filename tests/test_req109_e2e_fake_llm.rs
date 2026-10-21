//! REQ-109 · **端到端**：真进程 + 本地假 LLM 端点 + 真 `ai_cache` 表。
//!
//! 这个文件回答的是「协议、缓存、预算**在真实装配下**是否真的生效」，
//! 而不是「单元函数算得对不对」：
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-003 | 第二次运行**请求数不增**（0 次调用）；删掉缓存行后再跑 ⇒ 请求数 +1 |
//! | AC-004 | 两次运行的四个产物 **sha256 逐个相同** |
//! | AC-006 | `--budget-tokens 1` ⇒ 非 0 退出，且**一个产物文件都不写** |
//! | R3 | `--refresh` ⇒ 请求数 +1 |
//!
//! # 为什么用「本地假端点」而不是 `ScriptedTransport`
//!
//! `ScriptedTransport` 只能注入到库函数里；本文件要验的正是**二进制里的接线**
//! （环境变量 → transport → 预算 → 缓存 → 评分 → 落盘）。假端点是一个
//! `TcpListener`（标准库，零新依赖），回一个固定形状的 OpenAI 兼容响应，
//! 并**数请求次数**。它不联网、不花钱、不需要真 key。
//!
//! # 跳过条件
//!
//! 需要 `ONTOK_TEST_DATABASE_URL`（`ai_cache` 要落库）。缺了就**跳过并打印原因**，
//! 与仓里既有的库用例同一做法 —— 「没配库」不等于「测试通过」，但也不该
//! 让一次干净的 `cargo test` 因为没有共享库而红。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_ontok-propose");

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn db_url() -> Option<String> {
    std::env::var("ONTOK_TEST_DATABASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

fn skip(what: &str) -> bool {
    if db_url().is_none() {
        eprintln!("SKIP {what}：未设置 ONTOK_TEST_DATABASE_URL（ai_cache 要落库）");
        return true;
    }
    false
}

// ──────────────────────────────────────────────────────────
// 假 LLM 端点（标准库 TCP，零依赖）
// ──────────────────────────────────────────────────────────

struct FakeLlm {
    base_url: String,
    hits: Arc<AtomicUsize>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl FakeLlm {
    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Drop for FakeLlm {
    fn drop(&mut self) {
        // 线程阻塞在 accept 上；测试进程结束时一并消失。这里只等它「已经不属于我们」。
        if let Some(h) = self.handle.take() {
            drop(h);
        }
    }
}

/// 回一个固定的 chat completion，正文是**合法的判定 JSON**。
fn spawn_fake_llm(content: Value, total_tokens: u64) -> FakeLlm {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地假端点");
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let content = content.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
            let mut data: Vec<u8> = Vec::new();
            let mut buf = [0u8; 2048];
            // 读到「头 + Content-Length 指定的正文」齐了为止
            loop {
                let n = match s.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };
                data.extend_from_slice(&buf[..n]);
                if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&data[..pos]).to_ascii_lowercase();
                    let len = head
                        .split("\r\n")
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if data.len() >= pos + 4 + len {
                        break;
                    }
                }
            }
            counter.fetch_add(1, Ordering::SeqCst);
            let body = serde_json::json!({
                "id": "chatcmpl-req109",
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
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes());
            let _ = s.flush();
        }
    });
    FakeLlm { base_url: format!("http://127.0.0.1:{port}/v1"), hits, handle: None }
}

// ──────────────────────────────────────────────────────────
// 运行二进制
// ──────────────────────────────────────────────────────────

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_propose(args: &[String], envs: &[(&str, String)]) -> Run {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(manifest_dir());
    // 先把 LLM 三个变量清干净，再按参数注入 —— 免得开发机上的真凭据混进测试
    for k in [
        "ONTOK_LLM_BASE_URL",
        "ONTOK_LLM_API_KEY",
        "ONTOK_LLM_MODEL",
    ] {
        cmd.env_remove(k);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.args(args);
    let out = cmd.output().expect("跑不动 ontok-propose 二进制");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// 一次运行要用的输出目录（每个用例独立，互不覆盖）。
fn out_dir(tag: &str) -> PathBuf {
    let p = manifest_dir().join("out").join("req109-e2e").join(tag);
    std::fs::create_dir_all(&p).expect("建输出目录");
    p
}

fn base_args(out: &Path, model_tag: &str) -> Vec<String> {
    let m = manifest_dir();
    vec![
        "--schema".into(),
        m.join("fixtures/schema-req109.json").to_string_lossy().into_owned(),
        "--names".into(),
        m.join("src/extract/table_names.json").to_string_lossy().into_owned(),
        "--out".into(),
        out.join("proposal.json").to_string_lossy().into_owned(),
        "--stats".into(),
        out.join("stats.json").to_string_lossy().into_owned(),
        "--project".into(),
        format!("req109{model_tag}"),
        "--no-sample".into(),
    ]
}

fn llm_envs(fake: &FakeLlm, model: &str) -> Vec<(&'static str, String)> {
    vec![
        ("ONTOK_LLM_BASE_URL", fake.base_url.clone()),
        // ⚠️ 假 key，**不是**任何真实凭据；它只用于让 newman 的 Bearer 头非空
        ("ONTOK_LLM_API_KEY", "test-only-not-a-real-key".to_string()),
        ("ONTOK_LLM_MODEL", model.to_string()),
        (
            "ONTOK_DATABASE_URL",
            db_url().expect("调用方已判过 ONTOK_TEST_DATABASE_URL"),
        ),
    ]
}

/// 四个产物的 sha256（AC-004 的比较口径）。
///
/// ⚠️ **AC-004 与 AC-003 / AC-010 在字面上互斥**（本轮实测，见报告 §AC 偏差）：
/// `ai.cache_hit` 记的是**这一次运行**的事实（第一次 false、第二次 true），
/// 而它按 demand / AC-010 必须落在产物里。所以「四份产物 sha256 逐个相同」
/// 只在**把这一位归一之后**成立。
///
/// 这里两种口径都算：`normalized` 用于「内容相同」的断言，
/// [`differing_lines`] 用来**证明**差异确实只有这一位（不是「差不多」）。
fn artifact_hashes(dir: &Path) -> Vec<(String, String)> {
    artifact_hashes_impl(dir, true)
}

fn raw_artifact_hashes(dir: &Path) -> Vec<(String, String)> {
    artifact_hashes_impl(dir, false)
}

fn artifact_hashes_impl(dir: &Path, normalize: bool) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = ["proposal.json", "stats.json", "proposal-pending.json", "proposal-rejected.json"]
        .iter()
        .filter_map(|n| {
            let p = dir.join(n);
            let bytes = std::fs::read(&p).ok()?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let payload = if normalize { normalize_cache_hit(&text) } else { text };
            Some((n.to_string(), ontok::extract::sample::sha256_hex(payload.as_bytes())))
        })
        .collect();
    v.sort();
    v
}

/// 把 `ai.cache_hit` 的取值抹平成同一个占位 —— 只动这一位，别的字节一个不碰。
fn normalize_cache_hit(text: &str) -> String {
    text.replace("\"cache_hit\": false", "\"cache_hit\": <run>")
        .replace("\"cache_hit\": true", "\"cache_hit\": <run>")
}

/// 两份文件里**取值不同**的行（用于证明「差异只有 cache_hit 一位」）。
fn differing_lines(a: &Path, b: &Path) -> Vec<(String, String)> {
    let ta = std::fs::read_to_string(a).expect("读第一份");
    let tb = std::fs::read_to_string(b).expect("读第二份");
    let la: Vec<&str> = ta.lines().collect();
    let lb: Vec<&str> = tb.lines().collect();
    let mut out = Vec::new();
    for i in 0..la.len().max(lb.len()) {
        let x = la.get(i).copied().unwrap_or("<无此行>");
        let y = lb.get(i).copied().unwrap_or("<无此行>");
        if x != y {
            out.push((x.trim().to_string(), y.trim().to_string()));
        }
    }
    out
}

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).expect("读产物")).expect("产物是合法 JSON")
}

/// 清掉某个测试模型的缓存行（**按 model 删**，不动别人跑出来的缓存）。
fn clear_cache_for(model: &str) {
    let url = db_url().expect("调用方已判过");
    let model = model.to_string();
    let rt = tokio::runtime::Runtime::new().expect("建 runtime");
    rt.block_on(async move {
        let pool = ontok::db::pool::connect(&url).await.expect("连测试库");
        ontok::db::pg::migrate(&pool).await.expect("迁移");
        ontok::db::PgAiCache::new(pool.clone())
            .delete_by_model(&model)
            .await
            .expect("删缓存行");
        pool.close().await;
    });
}

// ──────────────────────────────────────────────────────────
// 用例
// ──────────────────────────────────────────────────────────

/// AC-003 + AC-004：第二次**0 次调用**、四个产物逐字节相同；删缓存后必须重调。
#[test]
fn ac003_ac004_cache_hit_then_reload() {
    if skip("ac003_ac004_cache_hit_then_reload") {
        return;
    }
    let model = "fake-model-req109-cache";
    clear_cache_for(model);

    let fake = spawn_fake_llm(
        serde_json::json!({
            "choice": {"table": "llm_model", "column": "id"},
            "semantic_fit": 0.8,
            "rationale": "模型表以 id 为主键，model_id 列指向模型行",
            "cites": [{"table": "llm_model", "column": "id"}]
        }),
        17,
    );
    let envs = llm_envs(&fake, model);

    // ① 第一次：未命中 ⇒ 一次调用
    let d1 = out_dir("cache-run1");
    let mut a = base_args(&d1, "cache");
    a.extend(["--llm".into(), "--budget-tokens".into(), "100000".into()]);
    let r1 = run_propose(&a, &envs);
    assert_eq!(r1.code, 0, "第一次运行必须成功：{}\n{}", r1.stdout, r1.stderr);
    assert_eq!(fake.hits(), 1, "一条关系 ⇒ 一次调用");
    let h1 = artifact_hashes(&d1);
    assert_eq!(h1.len(), 4, "四份产物都要写出：{h1:?}");

    // 产物里必须带 ai 溯源 + v2 方法（AC-010 的审核界面数据源）
    let pending = read_json(&d1.join("proposal-pending.json"));
    let item = pending["items"].as_array().expect("items").first().expect("一条关系").clone();
    let conf = &item["confidence"];
    assert_eq!(conf["method"], "evidence-scorecard-v2", "AI 开必须用 v2");
    assert_eq!(conf["ai"]["cache_hit"], false);
    assert_eq!(conf["ai"]["tokens"], 17);
    assert_eq!(conf["ai"]["model"], model);
    assert!(
        conf["signals"].as_array().unwrap().iter().any(|s| s["name"] == "semantic_fit"),
        "semantic_fit 必须作为信号进评分卡：{conf}"
    );

    // ② 第二次：全命中 ⇒ **0 次调用**，四个产物逐字节相同
    let d2 = out_dir("cache-run2");
    let mut a2 = base_args(&d2, "cache");
    a2.extend(["--llm".into(), "--budget-tokens".into(), "100000".into()]);
    let r2 = run_propose(&a2, &envs);
    assert_eq!(r2.code, 0, "第二次运行必须成功：{}\n{}", r2.stdout, r2.stderr);
    assert_eq!(fake.hits(), 1, "第二次**一次都不能调**（命中缓存）");
    let pending2 = read_json(&d2.join("proposal-pending.json"));
    let item2 = pending2["items"].as_array().unwrap().first().unwrap().clone();
    assert_eq!(item2["confidence"]["ai"]["cache_hit"], true, "命中要如实标 true");
    assert_eq!(
        artifact_hashes(&d2),
        h1,
        "同输入两次运行：把 ai.cache_hit 归一之后，四份产物必须逐个字节相同（AC-004）"
    );
    // ★ 差异**只许**有 cache_hit 这一位 —— 这一条是「归一」不是「放宽」的证据
    let diff = differing_lines(&d1.join("proposal-pending.json"), &d2.join("proposal-pending.json"));
    assert_eq!(
        diff,
        vec![(
            "\"cache_hit\": false,".to_string(),
            "\"cache_hit\": true,".to_string()
        )],
        "两次运行的差异必须且只能是 ai.cache_hit（AC-003 要求第二次为 true，AC-004 要求字节相同 —— 两条字面上互斥）"
    );
    // 未归一的原始 sha256 因此会差一份文件（pending），其余三份应当**完全相同**
    let raw1 = raw_artifact_hashes(&d1);
    let raw2 = raw_artifact_hashes(&d2);
    let changed: Vec<&String> = raw1
        .iter()
        .zip(raw2.iter())
        .filter(|(a, b)| a.1 != b.1)
        .map(|(a, _)| &a.0)
        .collect();
    assert_eq!(changed, vec!["proposal-pending.json"], "原始 sha256 只该差这一份：{changed:?}");

    // ③ 删掉这个模型的缓存行 ⇒ 第三次必须真的重调（AC-003 的第三条判据）
    clear_cache_for(model);
    let d3 = out_dir("cache-run3");
    let mut a3 = base_args(&d3, "cache");
    a3.extend(["--llm".into(), "--budget-tokens".into(), "100000".into()]);
    let r3 = run_propose(&a3, &envs);
    assert_eq!(r3.code, 0);
    assert_eq!(fake.hits(), 2, "缓存被删掉后必须重新调用一次");
    assert_eq!(artifact_hashes(&d3), h1, "重算得到的产物仍应与第一次逐字节相同");

    clear_cache_for(model);
}

/// R3 / AC-005：`--refresh` 忽略缓存并覆盖（请求数 +1）。
#[test]
fn r3_refresh_forces_one_more_call() {
    if skip("r3_refresh_forces_one_more_call") {
        return;
    }
    let model = "fake-model-req109-refresh";
    clear_cache_for(model);
    let fake = spawn_fake_llm(
        serde_json::json!({
            "choice": {"table": "llm_model", "column": "id"},
            "semantic_fit": 0.7,
            "rationale": "刷新测试用固定答复",
            "cites": [{"table": "llm_model", "column": "id"}]
        }),
        17,
    );
    let envs = llm_envs(&fake, model);

    let d1 = out_dir("refresh-run1");
    let mut a = base_args(&d1, "refresh");
    a.extend(["--llm".into(), "--budget-tokens".into(), "100000".into()]);
    assert_eq!(run_propose(&a, &envs).code, 0);
    assert_eq!(fake.hits(), 1);

    let d2 = out_dir("refresh-run2");
    let mut a2 = base_args(&d2, "refresh");
    a2.extend([
        "--llm".into(),
        "--budget-tokens".into(),
        "100000".into(),
        "--refresh".into(),
    ]);
    let r2 = run_propose(&a2, &envs);
    assert_eq!(r2.code, 0, "{}\n{}", r2.stdout, r2.stderr);
    assert_eq!(fake.hits(), 2, "--refresh 必须真的重新调用");
    assert!(
        r2.stdout.contains("强制刷新") && r2.stdout.contains("1 条"),
        "R3 要求打印「强制刷新 N 条」：{}",
        r2.stdout
    );
    clear_cache_for(model);
}

/// AC-006：预算 1 ⇒ 第一次调用后停，**非 0 退出且一个产物都不写**。
#[test]
fn ac006_budget_one_stops_and_writes_no_artifact() {
    if skip("ac006_budget_one_stops_and_writes_no_artifact") {
        return;
    }
    let model = "fake-model-req109-budget";
    clear_cache_for(model);
    let fake = spawn_fake_llm(
        serde_json::json!({
            "choice": null,
            "semantic_fit": 0.5,
            "rationale": "预算测试用固定答复",
            "cites": []
        }),
        17,
    );
    let envs = llm_envs(&fake, model);

    // 一个全新目录：跑完仍然必须是空的
    let d = manifest_dir().join("out").join("req109-e2e").join("budget-run1");
    let _ = std::fs::remove_dir_all(&d); // 只删本用例自己的输出目录
    std::fs::create_dir_all(&d).expect("建输出目录");

    let mut a = base_args(&d, "budget");
    a.extend(["--llm".into(), "--budget-tokens".into(), "1".into()]);
    let r = run_propose(&a, &envs);
    assert_ne!(r.code, 0, "越预算必须非 0 退出：{}\n{}", r.stdout, r.stderr);
    let all = format!("{}{}", r.stdout, r.stderr);
    for needle in ["已用", "上限", "停在第"] {
        assert!(all.contains(needle), "报告里必须有「{needle}」：{all}");
    }
    assert_eq!(fake.hits(), 1, "预算 1 ⇒ 只允许第一次调用");
    for f in ["proposal.json", "stats.json", "proposal-pending.json", "proposal-rejected.json"] {
        assert!(
            !d.join(f).exists(),
            "AC-006：越预算时不许产出半份可导入的提案（却写下了 {f}）"
        );
    }
    clear_cache_for(model);
}
