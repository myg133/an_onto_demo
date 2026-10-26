//! REQ-116 · **端到端**：真进程 + 本地假 LLM 端点 + 自造中文语料。
//!
//! 这个文件回答的是「**在没有产品库的机器上**，文档摄取真的按 AC 跑了吗」，
//! 而不是「单元函数算得对不对」（那些在 `src/extract/doc_ingest.rs` 的 `mod tests` 里）：
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | `.pdf` / `.docx` / `.doc` ⇒ exit≠0，**点名格式** + 提示先转文本，且**不出产物** |
//! | AC-002 | 同一语料两次 `--chunks-only` ⇒ 产物 **sha256 相同** |
//! | AC-003 | 伪造 quote ⇒ **丢弃**、JSONL 里一条都不留、`stats.rejected.no_evidence` 计数；<br>反向：合法 quote 的 `span` 必须能在**块原文**里切出同一段字 |
//! | AC-004 | 谓词是文档动词；`belongs_to` 被拒（不出现在 JSONL 里） |
//! | AC-005 | 每条带 `modality`；`future` / `example` 的 `alignable = false` |
//! | AC-006 | 第二次运行 **0 token**、`cache_hits == chunks`、JSONL **逐字节相同** |
//! | AC-007 | `--budget-tokens 1` ⇒ 停 + 报「已用 / 上限 / 停在第几块」+ **不写断言文件** |
//! | AC-008 | 全文件**不连库**：所有用例都 `env_remove(DATABASE_URL)`，且本模块源码里
//!           没有 `sqlx` / `PgPool` / `db::` |
//! | AC-009 | `I10` 登记为**内部轨**、不过发布闸，且没有接到客户视图 |
//!
//! # 为什么用「本地假端点」而不是真模型
//!
//! 真模型会说人话、但**不保证**每次都吐同一份 JSON ⇒ 确定性判据（AC-006）和
//! 证据判据（AC-003）就会随模型版本漂移。假端点是一个 `TcpListener`（标准库，
//! 零新依赖），按请求里的**块原文**回一份固定形状的回答，并**数请求次数**：
//! 不联网、不花钱、不需要真 key、可重复。
//!
//! # 夹具
//!
//! `tests/fixtures/req116/docs/` 是**自造**的中文小样本（4 份，覆盖标题层级 /
//! 列表 / 表格 / 中英混排 / 含「未来规划」「示例」字样）—— 仓里没有业务文档。

mod common;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_ontok-builder");

/// 跑一次要用的环境变量名字（**全部先清掉**，再按用例注入）。
const LLM_ENVS: [&str; 3] = ["ONTOK_LLM_BASE_URL", "ONTOK_LLM_API_KEY", "ONTOK_LLM_MODEL"];

fn crate_root() -> PathBuf {
    common::crate_root()
}

fn docs_dir() -> PathBuf {
    crate_root().join("tests/fixtures/req116/docs")
}

fn formats_dir() -> PathBuf {
    crate_root().join("tests/fixtures/req116/formats")
}

// ──────────────────────────────────────────────────────────
// 假 LLM 端点（标准库 TCP，零依赖）
// ──────────────────────────────────────────────────────────

/// 假端点怎么回答。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 从请求里的**块原文**取前 24 个字符当 quote（逐字命中）。
    /// 另外固定回一条 `belongs_to` 关系（AC-004 的闸要把它拒掉）。
    Valid,
    /// 回一句原文里**没有**的 quote（AC-003 的伪造用例）。
    Forged,
    /// ⭐ N-4：quote = 块原文前缀 + **原文里没有的尾巴**。
    ///
    /// 逐字匹配必须拒；任何「前缀 / 半句 / starts_with 就算过」的放宽都会放行它
    /// ⇒ 这条判据在**集成层**也有牙（QA 的 M2 变异：lib 红 4 条、集成 8 条全绿）。
    PrefixForged,
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

/// 从请求体里取出「这一块」的原文（`messages[1].content` 是一个 JSON 字符串）。
fn chunk_text_of(body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return String::new() };
    let user = v["messages"]
        .as_array()
        .and_then(|a| a.iter().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("");
    serde_json::from_str::<Value>(user)
        .ok()
        .and_then(|u| u["text"].as_str().map(str::to_string))
        .unwrap_or_default()
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
            // 读到「头 + Content-Length 指定的正文」齐了为止
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
            // ⚠️ 只喂**正文**给 JSON 解析：整段 data 含 HTTP 头，直接解析必然失败
            //（第一版就是这么错的：`chunk_text_of` 恒返回空串 ⇒ 所有 quote 都是空的）。
            let body = match header_end {
                Some(pos) => {
                    let end = (pos + 4 + content_len).min(data.len());
                    String::from_utf8_lossy(&data[pos + 4..end]).into_owned()
                }
                None => String::new(),
            };
            let text = chunk_text_of(&body);
            let quote = match mode {
                Mode::Valid => text.chars().take(24).collect::<String>(),
                Mode::Forged => "模型编造的原文片段：客户向供应商采购".to_string(),
                Mode::PrefixForged => {
                    // 前 40 字是**真的**（原文前缀），尾巴是模型补的 —— 逐字匹配必须拒。
                    let mut q: String = text.chars().take(40).collect();
                    q.push_str("（原文没有的后半句）");
                    q
                }
            };
            let content = serde_json::json!({
                "concepts": [
                    {"name": "客户", "aliases": ["顾客"], "description": "下单的主体",
                     "quote": quote, "modality": "fact"},
                    {"name": "积分转赠", "quote": quote, "modality": "future"},
                    {"name": "下单示例", "quote": quote, "modality": "example"}
                ],
                "relations": [
                    {"subject": "客户", "predicate": "下单", "object": "订单",
                     "cardinality": "N:1", "quote": quote, "modality": "fact"},
                    // ⛔ 结构词：必须被 AC-004 的闸拒掉
                    {"subject": "订单", "predicate": "belongs_to", "object": "客户",
                     "quote": quote, "modality": "fact"}
                ]
            })
            .to_string();
            let resp_body = serde_json::json!({
                "id": "chatcmpl-req116",
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
    FakeLlm { base_url: format!("http://127.0.0.1:{port}/v1"), hits, _handle: Some(handle) }
}

// ──────────────────────────────────────────────────────────
// 运行二进制
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

fn run_doc(args: &[String], fake: Option<&FakeLlm>) -> Run {
    let mut cmd = Command::new(BIN);
    cmd.current_dir(crate_root());
    // ⭐ AC-008 的一半：**两个库连接串都清掉**。能跑通 = 本命令真的不连库。
    for k in LLM_ENVS
        .iter()
        .copied()
        .chain(["ONTOK_DATABASE_URL", "ONTOK_TEST_DATABASE_URL"])
    {
        cmd.env_remove(k);
    }
    if let Some(f) = fake {
        cmd.env("ONTOK_LLM_BASE_URL", &f.base_url);
        // ⚠️ 假 key，**不是**任何真实凭据；只用于让 Bearer 头非空
        cmd.env("ONTOK_LLM_API_KEY", "test-only-not-a-real-key");
        cmd.env("ONTOK_LLM_MODEL", "fake-model-req116");
    }
    cmd.args(args);
    let out = cmd.output().expect("跑不动 ontok 二进制");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// 每个用例自己的输出目录（先删干净 —— 缓存文件会让「第一次」变成「第二次」）。
fn out_dir(tag: &str) -> PathBuf {
    let p = crate_root().join("out").join("req116").join(tag);
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("建输出目录");
    p
}

fn ingest_args(input: &Path, out: &Path, stats: &Path) -> Vec<String> {
    vec![
        "doc".into(),
        "ingest".into(),
        "--input".into(),
        input.to_string_lossy().into_owned(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
        "--stats".into(),
        stats.to_string_lossy().into_owned(),
    ]
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

// ──────────────────────────────────────────────────────────
// AC-001 · 格式闸
// ──────────────────────────────────────────────────────────

/// `.pdf` / `.docx` / `.doc` 各跑一次：exit≠0、**点名格式**、提示先转文本、**不出产物**。
/// 另加 `.PDF`（N-6：报错显示**实际后缀**，不折成小写）。
#[test]
fn ac001_unsupported_formats_are_rejected_by_name() {
    for ext in ["pdf", "docx", "doc"] {
        let d = out_dir(&format!("format-{ext}"));
        let out = d.join("assertions.jsonl");
        let stats = d.join("stats.json");
        let input = formats_dir().join(format!("sample.{ext}"));
        let r = run_doc(&ingest_args(&input, &out, &stats), None);
        let all = r.all();
        assert_ne!(r.code, 0, "({ext}) 不支持的格式必须非 0 退出：{all}");
        assert!(all.contains(&format!(".{ext}")), "({ext}) 报错必须点名格式：{all}");
        assert!(
            all.contains("先把它转成 .md/.txt"),
            "({ext}) 必须提示先转文本（转换后记 hash）：{all}"
        );
        assert!(
            all.contains("不会静默跳过"),
            "({ext}) 必须说清「不静默跳过」—— 跳过等于装作处理过：{all}"
        );
        assert!(!out.exists(), "({ext}) 格式被拒时不许产出断言文件");
    }

    // ⭐ N-6：后缀是大写 `.PDF` 时，报错必须显示 `.PDF`（折成小写会让人以为程序认错了）。
    let d = out_dir("format-upper");
    let upper = d.join("sample.PDF");
    std::fs::copy(formats_dir().join("sample.pdf"), &upper).expect("复制大写后缀夹具");
    let out = d.join("assertions.jsonl");
    let r = run_doc(&ingest_args(&upper, &out, &d.join("stats.json")), None);
    assert_ne!(r.code, 0);
    assert!(r.stderr.contains(".PDF"), "必须显示实际后缀 `.PDF`：{}", r.stderr);
    assert!(
        !r.stderr.contains(".pdf（"),
        "不许把 `.PDF` 折成 `.pdf` 再报：{}",
        r.stderr
    );
}

/// ⭐ N-5：隐藏文件**不许静默跳过**（QA 判 fail-open）。
///
/// 裁定 (a)：隐藏项**照常处理、照常报错**。
/// - 隐藏目录里的 `.pdf` ⇒ 非 0 退出并点名格式（⛔ 不能 exit 0 装作跑过）；
/// - 隐藏的 `.md` ⇒ **照常读**（隐藏 ≠ 不是文档）。
#[test]
fn ac001_hidden_files_are_processed_not_silently_skipped() {
    // (a) 隐藏目录里的 `.pdf`
    let d = out_dir("hidden");
    let input = d.join("in");
    std::fs::create_dir_all(input.join(".hidden")).expect("建隐藏目录");
    std::fs::write(input.join(".hidden").join("x.pdf"), b"%PDF-1.4\n").expect("写隐藏 pdf");
    std::fs::write(input.join("visible.md"), "# 可见\n\n客户下单。\n").expect("写可见 md");
    let out = d.join("assertions.jsonl");
    let r = run_doc(&ingest_args(&input, &out, &d.join("stats.json")), None);
    assert_ne!(
        r.code,
        0,
        "隐藏目录里的 .pdf 被静默跳过了（exit 0）—— 那正是 AC-001 禁止的 fail-open：\n{}",
        r.all()
    );
    assert!(r.stderr.contains(".pdf"), "必须点名格式：{}", r.stderr);

    // (b) 隐藏的 `.md` 照常读
    let d2 = out_dir("hidden-md");
    let input2 = d2.join("in");
    std::fs::create_dir_all(&input2).expect("建输入目录");
    std::fs::write(input2.join(".secret.md"), "# 秘密\n\n平台扣费。\n").expect("写隐藏 md");
    let out2 = d2.join("chunks.jsonl");
    let mut args = ingest_args(&input2, &out2, &d2.join("stats.json"));
    args.push("--chunks-only".into());
    let r2 = run_doc(&args, None);
    assert_eq!(r2.code, 0, "隐藏的 .md 必须照常读：{}", r2.stderr);
    let rows = lines_of(&out2);
    assert_eq!(rows.len(), 1, "隐藏的 .md 应产出 1 块：{rows:?}");
    assert_eq!(rows[0]["file"], ".secret.md", "隐藏文件也是文档：{rows:?}");
}

// ──────────────────────────────────────────────────────────
// AC-002 · 分块确定性
// ──────────────────────────────────────────────────────────

/// 同一语料两次 `--chunks-only` ⇒ 产物 sha256 相同；块 id 形状与切片自洽。
#[test]
fn ac002_chunks_only_is_byte_identical_across_runs() {
    let d = out_dir("chunks");
    let a = d.join("chunks-a.jsonl");
    let b = d.join("chunks-b.jsonl");
    let mut args = ingest_args(&docs_dir(), &a, &d.join("unused.json"));
    args.push("--chunks-only".into());
    let r1 = run_doc(&args, None);
    assert_eq!(r1.code, 0, "只分块不需要模型：{}\n{}", r1.stdout, r1.stderr);
    let mut args2 = ingest_args(&docs_dir(), &b, &d.join("unused.json"));
    args2.push("--chunks-only".into());
    let r2 = run_doc(&args2, None);
    assert_eq!(r2.code, 0, "{}", r2.stderr);

    let ha = ontok::extract::sample::sha256_hex(&std::fs::read(&a).expect("读块清单 a"));
    let hb = ontok::extract::sample::sha256_hex(&std::fs::read(&b).expect("读块清单 b"));
    assert_eq!(ha, hb, "同一份语料两次分块必须逐字节相同（AC-002）");

    let lines = lines_of(&a);
    assert!(lines.len() >= 4, "4 份文档至少 4 块：{}", lines.len());
    for l in &lines {
        let id = l["chunk_id"].as_str().expect("chunk_id");
        let (sha, range) = id.split_once(':').expect("chunk_id = sha256(file)[:12]:start-end");
        assert_eq!(sha.len(), 12, "块 id 前 12 位是文件 sha256：{id}");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()), "块 id 前缀必须是 hex：{id}");
        let (s, e) = range.split_once('-').expect("start-end");
        let (s, e): (usize, usize) = (s.parse().unwrap(), e.parse().unwrap());
        assert!(s < e, "块区间必须非空：{id}");
        assert_eq!(l["start"].as_u64().unwrap() as usize, s);
        assert_eq!(l["end"].as_u64().unwrap() as usize, e);
        let text = l["text"].as_str().expect("text");
        assert!(!text.trim().is_empty(), "空块不许出现");
    }
}

// ──────────────────────────────────────────────────────────
// AC-003 / AC-004 / AC-005 · 证据、谓词、模态
// ──────────────────────────────────────────────────────────

/// 合法回答：每条断言的 `span` 必须能在**该块原文**里切出与 `quote` 相同的字；
/// 谓词不是结构词；`future` / `example` 标 `alignable: false`。
///
/// ⚠️ 这里的 span 校验**不复用被测模块的 `locate_quote`** —— 独立地从块清单里
/// 取原文再按 span 切片，免得「判据与被测代码同一个 bug」时两边一起绿。
#[test]
fn ac003_ac004_ac005_evidence_spans_are_verbatim() {
    let fake = spawn_fake(Mode::Valid, 11);
    let d = out_dir("valid");
    let out = d.join("assertions.jsonl");
    let stats = d.join("stats.json");
    let r = run_doc(&ingest_args(&docs_dir(), &out, &stats), Some(&fake));
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);

    // 独立地拿一份块清单（同一份语料的确定性产物）
    let manifest = d.join("chunks.jsonl");
    let mut cargs = ingest_args(&docs_dir(), &manifest, &d.join("unused.json"));
    cargs.push("--chunks-only".into());
    assert_eq!(run_doc(&cargs, None).code, 0);
    let mut chunk_text: HashMap<String, (String, usize)> = HashMap::new();
    for c in lines_of(&manifest) {
        chunk_text.insert(
            c["chunk_id"].as_str().unwrap().to_string(),
            (c["text"].as_str().unwrap().to_string(), c["start"].as_u64().unwrap() as usize),
        );
    }

    let rows = lines_of(&out);
    assert!(!rows.is_empty(), "合法回答必须产出断言：{}", r.stderr);
    let mut future = 0;
    let mut example = 0;
    let mut relations = 0;
    for v in &rows {
        let kind = v["kind"].as_str().expect("kind");
        assert!(kind == "concept" || kind == "relation", "kind 只有这两种：{kind}");
        let ev = &v["evidence"];
        let quote = ev["quote"].as_str().expect("evidence.quote");
        assert!(!quote.trim().is_empty(), "没有 quote 的断言不许进 JSONL：{v}");
        let cid = ev["chunk_id"].as_str().expect("chunk_id");
        let (text, start) = chunk_text.get(cid).expect("evidence.chunk_id 必须是真的块");
        let span = ev["span"].as_array().expect("span");
        let (s, e) = (span[0].as_u64().unwrap() as usize, span[1].as_u64().unwrap() as usize);
        assert!(s >= *start && e <= start + text.len(), "span 必须落在块内：{v}");
        assert_eq!(
            &text[s - start..e - start],
            quote,
            "span 切出来的字必须与 quote 逐字相同（证据可复核）：{v}"
        );
        // 模态
        let m = v["modality"].as_str().expect("modality");
        assert!(
            ["fact", "spec", "future", "example"].contains(&m),
            "modality 必须是四选一：{m}"
        );
        match m {
            "future" => {
                future += 1;
                assert_eq!(v["alignable"], false, "future 必须标不可对齐：{v}");
            }
            "example" => {
                example += 1;
                assert_eq!(v["alignable"], false, "example 必须标不可对齐：{v}");
            }
            _ => assert_eq!(v["alignable"], true, "fact/spec 可对齐：{v}"),
        }
        if kind == "relation" {
            relations += 1;
            let p = v["predicate"].as_str().expect("predicate");
            assert!(!p.trim().is_empty(), "关系必须有动词谓词：{v}");
            assert_ne!(p, "belongs_to", "⛔ 谓词不许退化成结构词（AC-004）：{v}");
        }
    }
    assert!(future > 0 && example > 0, "假端点回了 future/example，产物里必须看得到");
    assert!(relations > 0, "至少要有一条关系断言");

    let s = read_json(&stats);
    assert_eq!(s["assertions"].as_u64().unwrap() as usize, rows.len());
    assert_eq!(s["relations"].as_u64().unwrap() as usize, relations);
    // 每一块都回了一条 belongs_to ⇒ 每条都被拒；future/example **不**被拒（保留但标记）
    assert_eq!(
        s["rejected"]["predicate"].as_u64().unwrap() as usize,
        s["chunks"].as_u64().unwrap() as usize,
        "每块一条 belongs_to 必须被拒：{s}"
    );
    assert_eq!(s["rejected"]["no_evidence"].as_u64().unwrap(), 0);

    // ⭐ N-2：模型**原始输出**里必须看得见 `belongs_to`（过闸之前），
    // 且「原始占比」不是 0 —— 否则 AC-004 是一条恒真判据（QA 判的就是这个）。
    let n = s["chunks"].as_u64().unwrap() as usize;
    assert_eq!(
        s["raw_predicate_counts"]["belongs_to"].as_u64().unwrap() as usize,
        n,
        "每块一条 belongs_to 必须在原始分布里：{s}"
    );
    assert_eq!(
        s["raw_predicate_counts"]["下单"].as_u64().unwrap() as usize,
        n,
        "{s}"
    );
    assert_eq!(s["raw_relations"].as_u64().unwrap() as usize, n * 2, "{s}");
    assert_eq!(
        s["rejected_by_predicate_gate"]["total"].as_u64().unwrap() as usize,
        n,
        "谓词闸丢弃的条数要单独记：{s}"
    );
    assert_eq!(
        s["rejected_by_predicate_gate"]["by_predicate"]["belongs_to"].as_u64().unwrap() as usize,
        n,
        "{s}"
    );
    let raw_share = s["belongs_to_share"]["raw"]["share"].as_f64().unwrap();
    assert!(raw_share > 0.0, "★ N-2：原始占比必须非 0（否则恒真）：{s}");
    assert_eq!(s["belongs_to_share"]["raw"]["count"].as_u64().unwrap() as usize, n);
    assert_eq!(s["belongs_to_share"]["raw"]["total"].as_u64().unwrap() as usize, n * 2);
    assert_eq!(s["belongs_to_share"]["alive"]["count"].as_u64().unwrap(), 0);
    assert_eq!(s["belongs_to_share"]["alive"]["total"].as_u64().unwrap() as usize, n);
    // 报告里两个数都要打出来
    assert!(r.stderr.contains("belongs_to  : 原始"), "屏幕要同时报两个占比：{}", r.stderr);
    assert!(r.stderr.contains("活下来"), "{}", r.stderr);
}

/// ★ 伪造 quote：**丢弃 + 计数**，JSONL 里一条都不留，且绝不「修正 quote 让它对上」。
#[test]
fn ac003_forged_quote_is_dropped_not_fixed() {
    let fake = spawn_fake(Mode::Forged, 7);
    let d = out_dir("forged");
    let out = d.join("assertions.jsonl");
    let stats = d.join("stats.json");
    let r = run_doc(&ingest_args(&docs_dir(), &out, &stats), Some(&fake));
    assert_eq!(r.code, 0, "丢弃是计数，不是运行失败：{}\n{}", r.stdout, r.stderr);

    let body = std::fs::read_to_string(&out).expect("断言文件必须写出（哪怕是空的）");
    assert_eq!(body, "", "伪造的 quote ⇒ 一条断言都不许留下（⛔ 不修正 quote）");

    let s = read_json(&stats);
    assert_eq!(s["assertions"].as_u64().unwrap(), 0);
    assert_eq!(s["concepts"].as_u64().unwrap(), 0);
    assert_eq!(s["relations"].as_u64().unwrap(), 0);
    assert!(
        s["rejected"]["no_evidence"].as_u64().unwrap() > 0,
        "证据对不上必须计数（stats.rejected.no_evidence）：{s}"
    );
    // 每块 3 concept + 1 relation(下单) + 1 belongs_to —— 前 4 条都栽在证据上，
    // belongs_to 先被谓词闸拒（在证据之前判）。
    assert_eq!(
        s["rejected"]["no_evidence"].as_u64().unwrap() as usize,
        s["chunks"].as_u64().unwrap() as usize * 4,
        "{s}"
    );
    assert_eq!(fake.hits() as u64, s["chunks"].as_u64().unwrap());
    // 屏幕上必须看得见（fail-closed 不等于沉默）
    assert!(r.stderr.contains("证据对不上"), "丢弃必须在报告里看得见：{}", r.stderr);
    assert!(r.stderr.contains("alignable=false"), "报告要说清 future/example 不可对齐：{}", r.stderr);
}

/// ⭐ N-4：**前缀 + 原文没有的尾巴** ⇒ 必须丢弃。
///
/// QA 变异 `locate_quote → 前缀匹配` 时，lib 抓到了 4 红、**集成 8 条全绿**——
/// 说明集成层对「放宽为前缀匹配」没有牙。这条就是补那一格的：
///
/// ```text
/// quote = 该块原文的前 40 个字 + "（原文没有的后半句）"
/// 逐字匹配 ⇒ 拒（本条判据）；前缀/半句/starts_with ⇒ 放行（这条立刻红）
/// ```
#[test]
fn ac003_prefix_plus_invented_tail_is_dropped() {
    let fake = spawn_fake(Mode::PrefixForged, 7);
    let d = out_dir("prefix-forged");
    let out = d.join("assertions.jsonl");
    let stats = d.join("stats.json");
    let r = run_doc(&ingest_args(&docs_dir(), &out, &stats), Some(&fake));
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);

    let body = std::fs::read_to_string(&out).expect("断言文件必须写出（哪怕是空的）");
    assert_eq!(
        body, "",
        "「原文前缀 + 原文没有的尾巴」必须整条丢弃 —— 前半句对不是证据（⛔ 不许前缀匹配）"
    );
    let s = read_json(&stats);
    assert_eq!(s["assertions"].as_u64().unwrap(), 0, "{s}");
    // 每块 3 concept + 1 relation 栽在证据上；belongs_to 先被谓词闸拒。
    assert_eq!(
        s["rejected"]["no_evidence"].as_u64().unwrap() as usize,
        s["chunks"].as_u64().unwrap() as usize * 4,
        "{s}"
    );
    // N-2 顺带：被谓词闸拒掉的那条仍然在原始分布里
    assert_eq!(
        s["rejected_by_predicate_gate"]["total"].as_u64().unwrap() as usize,
        s["chunks"].as_u64().unwrap() as usize,
        "{s}"
    );
    assert!(r.stderr.contains("证据对不上"), "{}", r.stderr);
}

// ──────────────────────────────────────────────────────────
// AC-006 · 确定性（缓存）
// ──────────────────────────────────────────────────────────

/// 第二次运行：**0 token**、全部命中缓存、JSONL 逐字节相同。
#[test]
fn ac006_second_run_is_zero_token_and_byte_identical() {
    let fake = spawn_fake(Mode::Valid, 13);
    let d = out_dir("cache");
    let out = d.join("assertions.jsonl");
    let stats = d.join("stats.json");
    let args = ingest_args(&docs_dir(), &out, &stats);

    let r1 = run_doc(&args, Some(&fake));
    assert_eq!(r1.code, 0, "{}\n{}", r1.stdout, r1.stderr);
    let hits1 = fake.hits();
    let s1 = read_json(&stats);
    let chunks = s1["chunks"].as_u64().unwrap() as usize;
    assert!(chunks >= 4, "语料至少 4 块：{s1}");
    assert_eq!(hits1, chunks, "第一次：每块一次调用");
    assert_eq!(s1["cache_hits"].as_u64().unwrap(), 0);
    assert_eq!(s1["calls"].as_u64().unwrap() as usize, chunks);
    assert!(s1["tokens_spent"].as_u64().unwrap() > 0);
    let jsonl1 = std::fs::read(&out).expect("读第一次的 JSONL");

    // 第二次：同一台机器、同一个 out（缓存文件在 `<out>.cache.json`）
    let r2 = run_doc(&args, Some(&fake));
    assert_eq!(r2.code, 0, "{}\n{}", r2.stdout, r2.stderr);
    assert_eq!(fake.hits(), hits1, "第二次**一次都不许调用模型**（AC-006）");
    let s2 = read_json(&stats);
    assert_eq!(s2["tokens_spent"].as_u64().unwrap(), 0, "第二次运行必须是 0 token：{s2}");
    assert_eq!(s2["cache_hits"].as_u64().unwrap() as usize, chunks, "全部命中：{s2}");
    assert_eq!(s2["calls"].as_u64().unwrap(), 0);
    let jsonl2 = std::fs::read(&out).expect("读第二次的 JSONL");
    assert_eq!(jsonl1, jsonl2, "JSONL 必须逐字节相同（AC-006）");
    assert!(
        std::fs::metadata(format!("{}.cache.json", out.display())).is_ok(),
        "缓存文件必须落在 <out>.cache.json"
    );
}

// ──────────────────────────────────────────────────────────
// AC-007 · 预算闸
// ──────────────────────────────────────────────────────────

/// `--budget-tokens 1`：第一次调用后即停，报「已用 / 上限 / 停在第几块」，
/// **不写断言文件**，但**缓存要落盘**（钱已经花了，不许白花）。
#[test]
fn ac007_budget_one_stops_and_writes_no_assertion_file() {
    let fake = spawn_fake(Mode::Valid, 50);
    let d = out_dir("budget");
    let out = d.join("assertions.jsonl");
    let stats = d.join("stats.json");
    let mut args = ingest_args(&docs_dir(), &out, &stats);
    args.extend(["--budget-tokens".into(), "1".into()]);

    let r = run_doc(&args, Some(&fake));
    assert_ne!(r.code, 0, "越预算必须非 0 退出：{}\n{}", r.stdout, r.stderr);
    let all = r.all();
    for needle in ["已用", "上限", "停在第"] {
        assert!(all.contains(needle), "报告里必须有「{needle}」：{all}");
    }
    assert_eq!(fake.hits(), 1, "预算 1 ⇒ 只允许第一次调用");
    assert!(!out.exists(), "AC-007：越预算时**不产出半份 JSONL**");
    assert!(!stats.exists(), "AC-007：越预算时也不写 stats");
    let cache = PathBuf::from(format!("{}.cache.json", out.display()));
    assert!(cache.exists(), "已经花掉的那次回答必须落缓存（否则重跑再付一次）");

    // 放宽预算重跑：第一次那块直接命中，只付剩下的块。
    let mut args2 = ingest_args(&docs_dir(), &out, &stats);
    args2.extend(["--budget-tokens".into(), "100000".into()]);
    let r2 = run_doc(&args2, Some(&fake));
    assert_eq!(r2.code, 0, "{}\n{}", r2.stdout, r2.stderr);
    let s = read_json(&stats);
    assert_eq!(s["cache_hits"].as_u64().unwrap(), 1, "越预算前那一块必须命中缓存：{s}");
    assert_eq!(s["calls"].as_u64().unwrap() as usize, s["chunks"].as_u64().unwrap() as usize - 1);
}

// ──────────────────────────────────────────────────────────
// AC-008 / AC-009 · 不写库、出口登记
// ──────────────────────────────────────────────────────────

/// AC-008：本模块**结构上碰不到库**（没有 sqlx / 连接池 / `db::`），
/// 且上面每个用例都清掉了两个连接串 —— 「不写库」不是靠人记得。
#[test]
fn ac008_doc_ingest_cannot_touch_the_database() {
    // 剥注释再扫：模块头**讨论**了「本模块没有 sqlx」这句话本身不是代码。
    let src = common::strip_comments(&common::read_source("src/extract/doc_ingest.rs"));
    for needle in ["sqlx", "PgPool", "db::"] {
        assert!(
            !src.contains(needle),
            "⛔ doc_ingest 里出现了 `{needle}` —— 本需求不写库（AC-008），缓存走本地文件"
        );
    }
    // `run()` 在连库之前分派 `doc` 子命令（否则上面的用例都会因缺库而失败，
    // 而不是因为格式/证据）。这一条钉住那个提前返回还在。
    let cli = common::read_source("src/cli.rs");
    let at = cli.find("Cmd::Doc { cmd } => return doc_cmd(cmd).await").expect(
        "`run()` 必须在建连接池**之前**分派 `doc` 子命令（否则格式闸会被「连不上库」盖住）",
    );
    let pool_at = cli.find("crate::db::pool::connect(&url)").expect("连接池那一行");
    assert!(at < pool_at, "`doc` 的分派必须在连库之前");
}

/// AC-009：产物登记为**内部出口**（内部轨 ⇒ 不过客户发布闸），且没有接到客户视图。
#[test]
fn ac009_doc_ingest_is_an_internal_outlet_not_a_customer_view() {
    use ontok::domain::pipeline::{outlet, Enforcement, Track};

    let o = outlet("I10").expect("REQ-116 的产物必须登记进 OUTLETS（AC-009）");
    assert_eq!(o.track(), Track::Internal, "文档摄取产物是**内部轨**");
    assert!(!o.track().gated(), "内部轨不过客户发布闸（AC-009）");
    assert_eq!(o.enforcement, Enforcement::InternalOnly);
    assert_eq!(o.path, "src/cli.rs");
    assert!(o.call_site.contains("fs::write"), "登记的写点必须是真的写出点：{:?}", o.call_site);
    assert!(!o.exemption_reason.trim().is_empty(), "豁免理由不许沉默");

    // 没有把它接到任何客户视图 / 客户出口
    for f in ["src/server/http.rs", "src/review_ui/http.rs", "src/mcp/tools.rs", "src/domain/view.rs"] {
        let src = common::read_source(f);
        assert!(
            !src.contains("doc_ingest") && !src.contains("assertions.jsonl"),
            "客户/服务出口 {f} 提到了文档摄取的产物 —— 它不该接到客户视图"
        );
    }
}
