//! **文档摄取与断言抽取**（REQ-116）—— 第一步：非结构化文档 → **带证据的断言**。
//!
//! # 一句话
//!
//! 当前本体 82 条关系里 79 条 `belongs_to`、3 条 `references`、基数全 `N:1` ——
//! 因为源库 0 外键，关系只能从 `*_id` 推断。**关系语义在文档里，不在 DDL 里。**
//! 本模块把文档切成确定性的块、让模型只回答「这一段里写明了哪些概念/关系」，
//! 然后**逐字校验它引用的原文片段**；对不上的断言**丢弃并计数**。
//!
//! ```text
//! .md/.txt ──分块（标题+段落，确定性）──▶ 块
//!   块 ──模型（每个块一次调用）──▶ 概念/关系 + quote
//!      ──证据校验（quote 必须在该块原文里逐字找到）──▶ JSONL
//!         对不上 ⇒ 丢弃 + 计数（⛔ 不修正 quote）
//! ```
//!
//! # 分层（照抄 [`crate::extract::ai_semantic`] 的 REQ-109 口径）
//!
//! | 层 | 职责 | I/O |
//! | --- | --- | --- |
//! | 本模块 | 格式闸 / 分块 / 指纹 / 协议校验 / 证据校验 / 缓存抽象 / 预算 / 渲染 | **零**（传输与缓存都是注入的 trait） |
//! | `cli::doc_cmd` | 读文件、写三份产物（JSONL / stats / 本地指纹缓存） | 有 |
//! | [`crate::agent::llm::transport`] | 真 HTTP | 有 |
//!
//! ⇒ **全部 AC 都能用 [`crate::agent::llm::ScriptedTransport`] + 内存缓存跑**，
//! 本模块一次网络都不打、一次库都不碰。
//!
//! # 六条硬约束落在哪里
//!
//! | # | 约束 | 落点 |
//! | --- | --- | --- |
//! | R1 | 证据是硬要求 | [`locate_quote`] + [`ingest_all`]：对不上 ⇒ 丢弃 + 计数 |
//! | R2 | 分块确定性 | [`chunk_document`]：只有标题/段落/字符上限三种边界，无模型参与 |
//! | R3 | 抽取确定性 | 指纹缓存：命中时**不调模型**，回放同一份回答文本 |
//! | R4 | 事实与意见分开 | [`Modality`]：`future` / `example` 标 `alignable: false` |
//! | R5 | 不猜 | 提示词明令「文档没写的关系不要产出”，且本模块**没有任何 schema 输入** |
//! | R6 | 中文友好 | 分块只在标题/段落/超长单元之后断开，⛔ 不按字符数切句 |
//!
//! # 证据校验只有一处实现（纪律 H）
//!
//! [`normalize_newlines`] + [`locate_quote`] 是**唯一**的判据。它的容忍面
//! **只有空白**：CRLF/LF 差异与连续空白差异。⛔ 不做编辑距离、不做前缀匹配、
//! 不做「大小写不敏感」。原因：一旦允许「差不多」，`quote` 就从**证据**退化成**提示**，
//! 而下游（REQ-117 对齐）会拿它当人签的依据。
//!
//! # 指纹缓存：为什么是**文件**而不是 `ai_cache` 表
//!
//! 本 REQ 的硬约束是「不写库」。REQ-109 的 `ai_cache` 表存的是**语义判断**的形状
//! （`CachedOutput`），与这里的「回答原文 + 用量」不是同一种载荷；往同一张表塞
//! 第二种形状会让 `PgAiCache::preload` 读到读不懂的行。所以本模块只定义
//! [`DocCache`] trait + 内存实现，落盘由 `cli::doc_cmd` 写一个**本地 JSON 文件**
//! （默认 `<out>.cache.json`）：
//!
//! - 命令因此**完全不连产品库**（`ontok doc ingest` 不需要 `ONTOK_DATABASE_URL`）；
//! - 「不写库」不是靠人记得，是**结构上做不到**（本模块没有 `sqlx`、没有连接池）；
//! - 指纹口径与 REQ-109 同源（`sha256(块文本 + prompt_sha + model + agent_version)`），
//!   换提示词 / 换模型 / 换抽取逻辑版本都会让缓存整体失效。
//!
//! # 缓存里存的是**回答原文**，不是已校验的断言
//!
//! 这样每次运行都会**重新过一遍证据校验**：缓存只省「模型调用」，
//! 不省「校验」。于是「第二次运行 0 token 且 JSONL 逐字节相同」是**校验结果**
//! 相同，而不是「上次那份没人再看」。
//!
//! # 已知限制（⛔ 不许写成「已支持」）
//!
//! - 只吃 `.md` / `.txt`（UTF-8）。`.pdf` / `.docx` / `.doc` **显式拒绝**，
//!   PDF/Word 解析由外部转换负责。
//! - 标题层级只认 Markdown ATX（`#`…`######`）；纯 `.txt` 没有标题层级，
//!   只按段落 + 字符上限分组。
//! - 多叫法**不去重**（`stats.distinct_concepts` 只是计数，不是实体消解）。
//! - 粒度未经真实业务文档调优（demand 风险 4：先看分布，再调提示词）。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::agent::llm::protocol::Message;
use crate::agent::llm::{completion_text, ChatRequest, LlmTransport};
use crate::extract::sample::sha256_hex;

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 抽取逻辑版本 —— **进指纹**。改了本模块的提示词策略 / 解析 / 校验，
/// 就该让它变，否则旧缓存会被新逻辑误用（与 `ai_semantic::AGENT_VERSION` 同一纪律）。
pub const DOC_AGENT_VERSION: &str = "doc-ingest-1";

/// 指纹算法的版本前缀。**改指纹构成 = 改这里**。
pub const FINGERPRINT_VERSION: &str = "v1";

/// 一个块的字符上限。**只用来决定「下一段要不要另起一块」**，
/// 绝不切开一个段落（R6：中文不按字符数硬切）。
pub const MAX_CHUNK_CHARS: usize = 1200;

/// `--budget-tokens` 省略时的默认上限。
///
/// 为什么给默认值而不是「不设闸」：本命令**总是**调用模型（每次一块），
/// 没有上限时一次全量运行的花费由文档规模决定，而调用方无从预估。
/// 默认值只是「别让一次手滑烧穿」，不是推荐值；要跑大文档请显式给上限。
pub const DEFAULT_BUDGET_TOKENS: u64 = 500_000;

/// 本通道的**默认输出上限**（token）。
///
/// # 为什么不是 [`crate::agent::llm::LlmConfig::DEFAULT_MAX_TOKENS`]（300）
///
/// 300 是给「一条关系判一句话」那种**单条判定**定的量级（REQ-109）。
/// 文档抽取一块要回**多条**断言（概念 + 关系 + 每条的 quote），300 个 token
/// 连推理都不够 —— 实测（`qwen36-27B`，本机）第一次跑就是
/// `finish_reason = "length"`、正文为空、862 个 token 全花在推理上。
///
/// 口径：**只有运维没设 `ONTOK_LLM_MAX_TOKENS` 时**才用这个默认值；
/// 设了就以运维的为准（那是部署事实，不该被代码盖掉）。
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// 只接受这两种扩展名（小写）。
pub const ACCEPTED_EXTENSIONS: &[&str] = &["md", "txt"];

/// **显式拒绝**并点名格式的外来格式（PDF / Word 由外部先转文本）。
pub const UNSUPPORTED_FORMATS: &[&str] = &["pdf", "docx", "doc"];

/// 结构词黑名单：⛔ 关系谓词不许退化成 schema 里的结构词（AC-004 的第一道闸）。
///
/// 只收**英文结构词 + `belongs_to`**：中文动词（「属于」「关联」）不在黑名单里 ——
/// 它们在真实业务文档里可能真的是谓词，误杀比漏杀更贵（AC-004 的分布统计可见）。
pub const STRUCTURAL_PREDICATES: &[&str] = &[
    "belongs_to",
    "belongs-to",
    "belongsto",
    "has_one",
    "has_many",
    "has_many_to_many",
    "references",
    "reference",
    "related_to",
    "is_a",
];

/// 每条断言都必须能在块原文里**逐字**指出来（R1）。
///
/// 提示词在这里只写**规则**，不写任何本次输入：输入走 user 消息，
/// 于是「改提示词」必然改 `prompts.sha256`、必然让缓存整体失效。
pub const SYSTEM_PROMPT: &str = r#"你是文档断言抽取器。你只做一件事：从给定的一段文档原文里，抽出文中**写明**的概念与关系，并为每一条附上原文片段。

输入是一个 JSON：{"file": "文件名", "chunk_id": "块编号", "heading_path": "标题路径", "text": "这一段文档原文", "task": "..."}

输出必须是严格 JSON（不要 markdown 代码块，不要任何解释文字），形状：
{"concepts": [{"name": "概念名", "aliases": ["别名"], "description": "一句话说明", "quote": "原文片段", "modality": "fact"}],
 "relations": [{"subject": "主体", "predicate": "动词", "object": "客体", "cardinality": "1:N", "quote": "原文片段", "modality": "fact"}]}

规则（每一条都会被程序校验，不合规的条目会被整条丢弃）：
1. quote 必须是输入 text 里**逐字出现**的一段原文（一句话或一个分句）。不许改写、不许拼接、不许加省略号、不许翻译。
2. predicate 必须是文档里**写明的动词或动宾短语**（如「下单」「扣费」「订阅」「退款」）。⛔ 不许写 belongs_to / has / references 这类结构词，也不许只写「关联」。
3. 文档**没写**的关系不要产出。⛔ 不许用数据库表结构知识「补全」——你只抽文中写了的东西。
4. modality 只能是这四个之一：
   fact = 陈述事实；spec = 规格 / 约定 / 约束；future = 未来规划 / 尚未实现；example = 示例 / 举例。
   带「将」「计划」「未来」「待」的通常是 future；带「例如」「示例」「比如」的通常是 example。
5. aliases / description / cardinality 不确定就省略或给空数组；cardinality 只能写 1:1、1:N、N:1、N:N。
6. 这一段里没有可抽的内容，就给 {"concepts": [], "relations": []}。
"#;

/// 一条请求里固定的 task 文案（进 user 消息，不进模板哈希）。
const TASK_TEXT: &str = "从这段文档原文里抽出写明了的 concept / relation，按协议返回严格 JSON。";

/// 提示词与它的哈希（构造时算一次，避免「改了模板却没重算哈希」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompts {
    pub system: String,
    pub sha256: String,
}

impl Prompts {
    pub fn standard() -> Self {
        Self::from_system(SYSTEM_PROMPT)
    }

    pub fn from_system(system: impl Into<String>) -> Self {
        let system = system.into();
        let sha256 = sha256_hex(system.as_bytes());
        Self { system, sha256 }
    }
}

impl Default for Prompts {
    fn default() -> Self {
        Self::standard()
    }
}

// ──────────────────────────────────────────────────────────
// 格式闸（AC-001）
// ──────────────────────────────────────────────────────────

/// 不支持的格式 —— **显式拒绝**，⛔ 不静默跳过。
///
/// 「跳过」等于装作处理过：产物里少一份文档，读的人无从知道少了什么。
pub fn unsupported_format_error(ext: &str, path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "不支持的文件格式：.{ext}（{path}）\n\
         \n\
         本命令只吃 .md / .txt（UTF-8 纯文本）。\n\
         请先把它转成 .md/.txt 再跑（转换后请记下 hash）：\n\
         \x20   pandoc \"{path}\" -t markdown -o \"{path}.md\"\n\
         \n\
         ⛔ 不会静默跳过 —— 跳过等于装作处理过（AC-001）。\
         PDF / Word 的解析由外部转换负责，本命令不做。",
        path = path.display()
    )
}

/// 一个待摄取的文件（**已读入、已归一化**）。
///
/// `sha256` 是对**归一化后的文本**算的：同一份文件在 CRLF / LF 两种检出下
/// 得到同一个文件指纹，于是块 id 不随平台漂移（`sha256(file)[:12]:<start>-<end>`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDoc {
    /// 输入里的原始路径（报错定位用）。
    pub path: PathBuf,
    /// 证据里记的文件标识：单文件输入 = 文件名；目录输入 = 相对目录的路径（`/` 分隔）。
    pub rel: String,
    /// `sha256(归一化文本)`。
    pub sha256: String,
    /// 归一化后的全文（CRLF/CR → LF，去掉 BOM）。
    pub text: String,
    /// 原始字节数（stats 用）。
    pub raw_bytes: usize,
}

impl SourceDoc {
    /// 从内存文本造一份（**测试与调用方不需要磁盘**）。
    ///
    /// 归一化口径与 [`load_docs`] 完全一致 —— 两条路不各算一次。
    pub fn from_text(rel: impl Into<String>, text: impl Into<String>) -> Self {
        let rel = rel.into();
        let text = normalize_newlines(&strip_bom(&text.into()));
        let raw_bytes = text.len();
        Self {
            path: PathBuf::from(&rel),
            sha256: sha256_hex(text.as_bytes()),
            rel,
            text,
            raw_bytes,
        }
    }
}

/// 去掉 UTF-8 BOM（`\u{feff}`）。BOM 是**编码标记**，不是原文内容。
fn strip_bom(text: &str) -> String {
    text.strip_prefix('\u{feff}').unwrap_or(text).to_string()
}

/// **换行归一化**：CRLF / 单独 CR → LF。这是证据校验的容忍面之一（R1）。
///
/// 只动换行，不动空格与全角字符 —— 「空白归一化」在 [`locate_quote`] 里按
/// **骨架匹配**做，不在这里改写原文（改写原文会让 span 对不上）。
pub fn normalize_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| !e.trim().is_empty())
}

/// 读入输入（文件或目录），**先判格式再读内容**。
///
/// 判据（AC-001）：
/// - `.md` / `.txt` ⇒ 入；
/// - `.pdf` / `.docx` / `.doc` ⇒ **报错并点名格式**；
/// - 其它扩展名 / 没有扩展名 ⇒ 同样报错（本命令只吃这两种）。
///
/// 目录**递归**；文件按 `rel` 字典序排序（输出确定性的一半）。
pub fn load_docs(input: &Path) -> Result<Vec<SourceDoc>> {
    if !input.exists() {
        bail!("找不到输入：{}（给一个 .md/.txt 文件，或装着它们的目录）", input.display());
    }
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    if input.is_dir() {
        walk_docs(input, input, &mut files)?;
        files.sort_by(|a, b| a.1.cmp(&b.1));
    } else {
        let rel = input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| input.display().to_string());
        check_ext(input)?;
        files.push((input.to_path_buf(), rel));
    }
    if files.is_empty() {
        bail!(
            "输入里一个 .md/.txt 都没有：{}（目录递归过了，空目录不算「跑过了」）",
            input.display()
        );
    }

    let mut docs = Vec::with_capacity(files.len());
    for (path, rel) in files {
        let bytes = std::fs::read(&path)
            .with_context(|| format!("读不到 {}", path.display()))?;
        let raw_bytes = bytes.len();
        let text = String::from_utf8(bytes).map_err(|e| {
            anyhow::anyhow!(
                "{} 不是合法的 UTF-8（第 {} 字节处）：{}\n\
                 本命令只吃 UTF-8 纯文本。请先转成 UTF-8（例如 iconv -f gbk -t utf-8）再跑。",
                path.display(),
                e.utf8_error().valid_up_to(),
                path.display()
            )
        })?;
        let text = normalize_newlines(&strip_bom(&text));
        docs.push(SourceDoc {
            sha256: sha256_hex(text.as_bytes()),
            rel,
            path,
            text,
            raw_bytes,
        });
    }
    Ok(docs)
}

/// 目录递归收集 `.md` / `.txt`。**任何不支持的格式都当场报错**（AC-001）。
///
/// 隐藏文件 / 隐藏目录（`.` 开头，如 `.git` / `.DS_Store`）跳过并在 stderr 说明 ——
/// 它们不是「用户以为会被处理的文档」；但 `.pdf` 这类**显式点名**的格式不会被跳过。
fn walk_docs(root: &Path, dir: &Path, acc: &mut Vec<(PathBuf, String)>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("读不了目录 {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("遍历目录 {} 失败", dir.display()))?;
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            walk_docs(root, &p, acc)?;
            continue;
        }
        check_ext(&p)?;
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        acc.push((p, rel));
    }
    Ok(())
}

/// 单个文件的格式闸（AC-001）。**唯一**一处判「这个扩展名能不能吃」。
fn check_ext(path: &Path) -> Result<()> {
    match extension_of(path) {
        Some(ext) if ACCEPTED_EXTENSIONS.contains(&ext.as_str()) => Ok(()),
        Some(ext) => Err(unsupported_format_error(&ext, path)),
        None => Err(anyhow::anyhow!(
            "无法判断格式（没有扩展名）：{}\n\
             本命令只吃 .md / .txt（UTF-8 纯文本）—— 没有扩展名时无从判断，宁可报错也不猜。",
            path.display()
        )),
    }
}

// ──────────────────────────────────────────────────────────
// 分块（AC-002 / R2 / R6）
// ──────────────────────────────────────────────────────────

/// 一个块 —— 证据的定位单位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Chunk {
    /// 证据里的文件标识（同 [`SourceDoc::rel`]）。
    pub file: String,
    /// `sha256(file)[:12]:<start>-<end>`（字节偏移，针对归一化后的文本）。
    pub chunk_id: String,
    /// 起始字节偏移（含）。
    pub start: usize,
    /// 结束字节偏移（不含）。
    pub end: usize,
    /// 块开始处的标题路径（如 `订单流程 > 支付`），给模型当上下文；⛔ 不进证据校验。
    pub heading_path: String,
    /// **原文切片**（`text[start..end]`）。证据 quote 必须能在这里逐字找到。
    pub text: String,
}

/// 一个「不可再分」的单元：标题行，或一个段落。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Unit {
    start: usize,
    end: usize,
    /// `Some(level)` = 这是一个 ATX 标题行。
    level: Option<usize>,
    heading: Option<String>,
}

/// ATX 标题（`#`…`######` + 空格）。其余一律当正文。
///
/// ⚠️ 只认 Markdown ATX：`.txt` 没有标题层级，只按段落分组（已登记为已知限制）。
fn heading_level(line: &str) -> Option<usize> {
    let t = line.trim_end();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
        Some(hashes)
    } else {
        None
    }
}

/// 把一段文本切成**单元**（标题行 / 段落）。
///
/// 段落 = 连续非空行（内部的单换行原样保留，所以一句话不会被当成两段）。
/// 偏移全部是归一化文本上的字节偏移 —— 与 [`Chunk::start`] / [`Chunk::end`] 同一套。
fn split_units(text: &str) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    // 当前段落的 (start, end)
    let mut para: Option<(usize, usize)> = None;
    let mut offset = 0usize;
    for raw in text.split_inclusive('\n') {
        let line_start = offset;
        offset += raw.len();
        let content = raw.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = content.trim();
        if trimmed.is_empty() {
            if let Some((s, e)) = para.take() {
                units.push(Unit { start: s, end: e, level: None, heading: None });
            }
            continue;
        }
        // 行内第一个非空白字节
        let lead = content.len() - content.trim_start().len();
        let ls = line_start + lead;
        let le = ls + trimmed.len();
        if let Some(level) = heading_level(trimmed) {
            if let Some((s, e)) = para.take() {
                units.push(Unit { start: s, end: e, level: None, heading: None });
            }
            units.push(Unit {
                start: ls,
                end: le,
                level: Some(level),
                heading: Some(trimmed.to_string()),
            });
            continue;
        }
        para = match para {
            Some((s, _)) => Some((s, le)),
            None => Some((ls, le)),
        };
    }
    if let Some((s, e)) = para.take() {
        units.push(Unit { start: s, end: e, level: None, heading: None });
    }
    units
}

/// 标题路径（栈式维护，`A > B`）。每个单元得到「它所在位置的路径」。
fn heading_paths(units: &[Unit]) -> Vec<String> {
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut out = Vec::with_capacity(units.len());
    for u in units {
        if let (Some(level), Some(h)) = (u.level, u.heading.as_deref()) {
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            // 标题自身只留名字（去掉 `#` 与首尾空白）
            let name = h.trim_start_matches('#').trim().to_string();
            stack.push((level, name));
        }
        out.push(
            stack
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>()
                .join(" > "),
        );
    }
    out
}

/// **确定性分块**（R2 / R6）。
///
/// 三种边界，没有第四种（⛔ 不调模型、不按字符数切句）：
/// 1. 下一个单元是 **1–2 级标题** ⇒ 另起一块（标题层级边界）；
/// 2. 再塞一个单元会超过 [`MAX_CHUNK_CHARS`] 字符 ⇒ 另起一块（**只在段落边界**上断）；
/// 3. 文档结束。
///
/// 单个超长段落**不会被切开**：它会独占一块，哪怕超过上限 ——
/// 「切句」比「块大」糟得多（R6）。
pub fn chunk_document(doc: &SourceDoc) -> Vec<Chunk> {
    let text = &doc.text;
    if text.trim().is_empty() {
        return Vec::new();
    }
    let units = split_units(text);
    if units.is_empty() {
        return Vec::new();
    }
    let paths = heading_paths(&units);
    let short = &doc.sha256[..12.min(doc.sha256.len())];

    let mut chunks = Vec::new();
    let mut i = 0usize;
    while i < units.len() {
        let first = i;
        let mut last = i;
        let mut chars = 0usize;
        // 这一块里已经有**正文**了吗？没有的话，连着的标题行要并进同一块 ——
        // 否则 `# 大标题` 会独占一块（一次模型调用只换来 20 个字，还丢了上下文）。
        let mut has_body = false;
        while last < units.len() {
            let u = &units[last];
            let u_chars = text[u.start..u.end].chars().count();
            if last > first {
                // ① 标题层级边界（1–2 级）—— **只在已经有正文之后**才断
                if u.level.is_some_and(|l| l <= 2) && has_body {
                    break;
                }
                // ② 字符上限（只在段落/标题单元之间判）
                if chars + u_chars > MAX_CHUNK_CHARS {
                    break;
                }
            }
            if u.level.is_none() {
                has_body = true;
            }
            chars += u_chars;
            last += 1;
        }
        let start = units[first].start;
        let mut end = units[last - 1].end;
        // 去掉尾部空白（保持切片连续：start/end 仍在归一化文本上）
        while end > start && text.as_bytes()[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        if end > start {
            chunks.push(Chunk {
                file: doc.rel.clone(),
                chunk_id: format!("{short}:{start}-{end}"),
                start,
                end,
                heading_path: paths[first].clone(),
                text: text[start..end].to_string(),
            });
        }
        i = last;
    }
    chunks
}

/// 把一批文档全部分块（文档顺序 = [`load_docs`] 的排序，块顺序 = 文档内顺序）。
pub fn chunk_all(docs: &[SourceDoc]) -> Vec<Chunk> {
    let mut out = Vec::new();
    for d in docs {
        out.extend(chunk_document(d));
    }
    out
}

// ──────────────────────────────────────────────────────────
// 证据校验（AC-003 / R1）—— **唯一实现**
// ──────────────────────────────────────────────────────────

/// 一条断言的证据：文件 / 块 / **原文片段** / 字节区间。
///
/// `span` 是**绝对**字节偏移（归一化文本上的 `[start, end)`），
/// 等于 `块起点 + 片段在块内的偏移` —— 人可以直接 `dd`/切片复核。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub file: String,
    pub chunk_id: String,
    /// 模型给的原文片段（**只去掉首尾空白**，一个字都不改）。
    pub quote: String,
    pub span: [usize; 2],
}

/// 把原文压成「非空白字符 + 每个字符的原始字节位置」。
fn skeleton(text: &str) -> (Vec<char>, Vec<(usize, usize)>) {
    let mut chars = Vec::new();
    let mut pos = Vec::new();
    let mut offset = 0usize;
    for c in text.chars() {
        let len = c.len_utf8();
        if !c.is_whitespace() {
            chars.push(c);
            pos.push((offset, len));
        }
        offset += len;
    }
    (chars, pos)
}

/// ⭐ **证据校验的唯一实现**（AC-003 / 纪律 H）。
///
/// `quote` 必须能在 `chunk_text` 里找到，返回**块内**字节区间 `[start, end)`。
///
/// 容忍面**只有空白**（这是 demand 明写的「CRLF/空白归一化后包含」）：
/// 1. 先做逐字匹配（`find`）—— 绝大多数情况走这条，span 精确；
/// 2. 不中时，把两边都压成「非空白字符骨架」再找 —— 只容忍**空白位置/数量**的差异，
///    仍然要求每一个非空白字符逐字相同。
///
/// ⛔ 不做的事（每一件都会把「证据」降级成「提示」）：
/// 编辑距离、前缀/后缀匹配、大小写折叠、标点归一、折行重排。
pub fn locate_quote(chunk_text: &str, quote: &str) -> Option<(usize, usize)> {
    let q = quote.trim();
    if q.is_empty() {
        return None;
    }
    // ① 逐字
    if let Some(i) = chunk_text.find(q) {
        return Some((i, i + q.len()));
    }
    // ② 空白骨架
    let (hay, pos) = skeleton(chunk_text);
    let (needle, _) = skeleton(q);
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            let start = pos[i].0;
            let last = &pos[i + needle.len() - 1];
            return Some((start, last.0 + last.1));
        }
        i += 1;
    }
    None
}

// ──────────────────────────────────────────────────────────
// 模态（AC-005 / R4）
// ──────────────────────────────────────────────────────────

/// 断言的模态。`future` / `example` **保留在 JSONL 里**（供人看），
/// 但标 `alignable: false` —— 下游（REQ-117）按这一位过滤。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    /// 陈述事实
    Fact,
    /// 规格 / 约定 / 约束
    Spec,
    /// 未来规划（尚未实现）
    Future,
    /// 示例 / 举例
    Example,
}

impl Modality {
    pub fn parse(s: Option<&str>) -> Option<Self> {
        match s?.trim().to_ascii_lowercase().as_str() {
            "fact" => Some(Modality::Fact),
            "spec" | "specification" => Some(Modality::Spec),
            "future" | "plan" | "planned" => Some(Modality::Future),
            "example" | "sample" => Some(Modality::Example),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Fact => "fact",
            Modality::Spec => "spec",
            Modality::Future => "future",
            Modality::Example => "example",
        }
    }

    /// 能进后续对齐吗？`future` / `example` **不能**（R4）。
    pub fn alignable(self) -> bool {
        matches!(self, Modality::Fact | Modality::Spec)
    }
}

// ──────────────────────────────────────────────────────────
// 断言（产物形状）
// ──────────────────────────────────────────────────────────

/// 一条概念断言。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConceptAssertion {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub modality: Modality,
    /// `false` = 不可用于对齐（`future` / `example`）。
    pub alignable: bool,
    pub evidence: Evidence,
}

/// 一条关系断言。`predicate` 是**文档里的动词**（这是本需求存在的理由，AC-004）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationAssertion {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<String>,
    pub modality: Modality,
    pub alignable: bool,
    pub evidence: Evidence,
}

/// JSONL 的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assertion {
    Concept(ConceptAssertion),
    Relation(RelationAssertion),
}

impl Assertion {
    pub fn evidence(&self) -> &Evidence {
        match self {
            Assertion::Concept(c) => &c.evidence,
            Assertion::Relation(r) => &r.evidence,
        }
    }

    pub fn modality(&self) -> Modality {
        match self {
            Assertion::Concept(c) => c.modality,
            Assertion::Relation(r) => r.modality,
        }
    }

    pub fn is_concept(&self) -> bool {
        matches!(self, Assertion::Concept(_))
    }
}

/// `chunk_id` 里的起点（`<sha>:<start>-<end>`）—— 稳定排序用。
fn chunk_start_of(chunk_id: &str) -> usize {
    chunk_id
        .rsplit_once(':')
        .and_then(|(_, r)| r.split_once('-'))
        .and_then(|(s, _)| s.parse::<usize>().ok())
        .unwrap_or(0)
}

/// **稳定排序**：`file → chunk 起点 → 块内序号`（demand 交付物 7 的口径）。
///
/// 用**稳定**排序，所以同一块内的条目保持模型回答里的顺序（序号）。
pub fn sort_assertions(v: &mut [Assertion]) {
    v.sort_by(|a, b| {
        let ea = a.evidence();
        let eb = b.evidence();
        ea.file
            .cmp(&eb.file)
            .then_with(|| chunk_start_of(&ea.chunk_id).cmp(&chunk_start_of(&eb.chunk_id)))
    });
}

// ──────────────────────────────────────────────────────────
// 输出协议（严格 JSON）+ 校验
// ──────────────────────────────────────────────────────────

/// 一块要被模型回答的「原始条目」。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawConcept {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub modality: Option<String>,
}

/// 一块要被模型回答的「原始关系」。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRelation {
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub predicate: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub cardinality: Option<String>,
    #[serde(default)]
    pub quote: String,
    #[serde(default)]
    pub modality: Option<String>,
}

/// 模型对一块的回答（**未校验**）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawExtraction {
    #[serde(default)]
    pub concepts: Vec<RawConcept>,
    #[serde(default)]
    pub relations: Vec<RawRelation>,
}

/// 断言被丢弃的原因（**分类计数** —— R1 要求「丢弃并计数」，只丢不报等于静默）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// 响应不是合法 JSON / 形状不符
    Parse,
    /// 名字 / 主客体为空
    Empty,
    /// 模态缺失或不在枚举内
    Modality,
    /// 谓词空 / 是结构词（AC-004）
    Predicate,
    /// `quote` 缺失，或在该块原文里找不到（**核心**）
    NoEvidence,
}

impl Rejection {
    pub fn as_str(self) -> &'static str {
        match self {
            Rejection::Parse => "解析失败",
            Rejection::Empty => "字段为空",
            Rejection::Modality => "模态不合规",
            Rejection::Predicate => "谓词不合规",
            Rejection::NoEvidence => "证据对不上",
        }
    }
}

/// 去掉可选的 markdown 代码围栏。**只剥围栏、不猜内容**：
/// 剥完必须仍是合法 JSON，否则整块作废（与 `ai_semantic` 同一口径）。
fn strip_code_fence(text: &str) -> &str {
    let t = text.trim();
    if !t.starts_with("```") {
        return t;
    }
    let after = match t.find('\n') {
        Some(i) => &t[i + 1..],
        None => return t,
    };
    match after.rfind("```") {
        Some(i) => after[..i].trim(),
        None => after.trim(),
    }
}

/// 解析模型的回答（**不校验证据** —— 证据校验在 [`validate`] 里，只有一处）。
pub fn parse_extraction(text: &str) -> Result<RawExtraction, String> {
    let body = strip_code_fence(text);
    match serde_json::from_str::<RawExtraction>(body) {
        Ok(r) => Ok(r),
        Err(e) => {
            // 有些模型会把回答包成 {"concepts": …} 之外的形状；这里只做一次明确的报错，
            // 不「尽力猜一个数组出来」——那会把形状错误变成静默的空抽取。
            Err(format!(
                "响应不是预期的 JSON 形状：{e}（截断到 200 字：{}）",
                body.chars().take(200).collect::<String>()
            ))
        }
    }
}

/// 谓词是不是**结构词**（AC-004 的第一道闸）。
///
/// ⛔ 关系谓词不许退化成 `belongs_to` —— 那正是当前本体 79/82 的病。
pub fn is_structural_predicate(predicate: &str) -> bool {
    let p = predicate.trim().to_ascii_lowercase();
    STRUCTURAL_PREDICATES.iter().any(|s| *s == p)
}

/// 一条原始条目的**校验**结果：过，或（原因 + 人读说明）。
fn check(
    chunk: &Chunk,
    quote: &str,
    stats: &mut DocStats,
) -> Option<Evidence> {
    let q = quote.trim();
    if q.is_empty() {
        stats.reject(Rejection::NoEvidence, format!("{}：条目没有 quote", chunk.chunk_id));
        return None;
    }
    match locate_quote(&chunk.text, q) {
        Some((s, e)) => Some(Evidence {
            file: chunk.file.clone(),
            chunk_id: chunk.chunk_id.clone(),
            quote: q.to_string(),
            span: [chunk.start + s, chunk.start + e],
        }),
        None => {
            // ⛔ 这里**绝不**尝试「修一下 quote 让它对上」：那是把证据变成提示。
            stats.reject(
                Rejection::NoEvidence,
                format!(
                    "{}：quote 在该块原文里找不到 —— 「{}」",
                    chunk.chunk_id,
                    q.chars().take(60).collect::<String>()
                ),
            );
            None
        }
    }
}

/// 校验一块的模型回答，产出**可以进 JSONL** 的断言（对不上的直接丢弃 + 计数）。
pub fn validate(chunk: &Chunk, raw: RawExtraction, stats: &mut DocStats) -> Vec<Assertion> {
    let mut out = Vec::new();

    for c in raw.concepts {
        let name = c.name.trim().to_string();
        if name.is_empty() {
            stats.reject(Rejection::Empty, format!("{}：concept.name 为空", chunk.chunk_id));
            continue;
        }
        let Some(modality) = Modality::parse(c.modality.as_deref()) else {
            stats.reject(
                Rejection::Modality,
                format!(
                    "{}：concept「{name}」的 modality = {:?} 不在 {{fact,spec,future,example}} 内",
                    chunk.chunk_id, c.modality
                ),
            );
            continue;
        };
        let Some(evidence) = check(chunk, &c.quote, stats) else { continue };
        let aliases: Vec<String> = c
            .aliases
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty() && *a != name)
            .collect();
        let description = c
            .description
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty());
        stats.concepts += 1;
        stats.concept_names.insert(name.clone());
        out.push(Assertion::Concept(ConceptAssertion {
            name,
            aliases,
            description,
            modality,
            alignable: modality.alignable(),
            evidence,
        }));
    }

    for r in raw.relations {
        let subject = r.subject.trim().to_string();
        let object = r.object.trim().to_string();
        if subject.is_empty() || object.is_empty() {
            stats.reject(
                Rejection::Empty,
                format!("{}：relation 的 subject/object 为空", chunk.chunk_id),
            );
            continue;
        }
        let predicate = r.predicate.trim().to_string();
        if predicate.is_empty() || is_structural_predicate(&predicate) {
            stats.reject(
                Rejection::Predicate,
                format!(
                    "{}：relation {subject} -[{predicate}]-> {object} 的谓词不合规\
                     （空 / 结构词；AC-004 要求文档里的动词）",
                    chunk.chunk_id
                ),
            );
            continue;
        }
        let Some(modality) = Modality::parse(r.modality.as_deref()) else {
            stats.reject(
                Rejection::Modality,
                format!(
                    "{}：relation「{subject}-{predicate}->{object}」的 modality = {:?} 不在枚举内",
                    chunk.chunk_id, r.modality
                ),
            );
            continue;
        };
        let Some(evidence) = check(chunk, &r.quote, stats) else { continue };
        let cardinality = r
            .cardinality
            .map(|c| c.trim().to_string())
            .filter(|c| matches!(c.as_str(), "1:1" | "1:N" | "N:1" | "N:N"));
        stats.relations += 1;
        *stats.predicate_counts.entry(predicate.clone()).or_insert(0) += 1;
        out.push(Assertion::Relation(RelationAssertion {
            subject,
            predicate,
            object,
            cardinality,
            modality,
            alignable: modality.alignable(),
            evidence,
        }));
    }

    out
}

// ──────────────────────────────────────────────────────────
// 请求与指纹（R3）
// ──────────────────────────────────────────────────────────

/// 一个块的抽取请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRequest {
    pub chunk: Chunk,
}

impl ChunkRequest {
    pub fn new(chunk: Chunk) -> Self {
        Self { chunk }
    }

    /// **指纹**：`sha256(块文本 + prompt_sha + model + agent_version)`（demand 原文口径）。
    ///
    /// 四项缺一不可：
    /// - 块文本：不同块不能共用一份缓存；
    /// - `prompt_sha`：**改提示词却复用旧结果**是本仓库点过名的病（REQ-109 R1）；
    /// - `model`：换模型仍复用旧结果 ⇒ 供应商漂移无声；
    /// - `agent_version`：解析/校验逻辑改了仍复用旧结果。
    ///
    /// 分隔符用 `\n` 且**长度前缀**不用 —— 四项里只有块文本可能含换行，
    /// 而 `prompt_sha` / `model` / `agent_version` 都不含换行，所以拼接无歧义。
    pub fn fingerprint(&self, prompt_sha: &str, model: &str, agent_version: &str) -> String {
        let material = format!(
            "{FINGERPRINT_VERSION}\n{}\n{}\n{}\n{}\n",
            self.chunk.text, prompt_sha, model, agent_version
        );
        sha256_hex(material.as_bytes())
    }

    /// user 消息的 JSON（**紧凑序列化**：同输入逐字节相同）。
    fn user_message(&self) -> Result<String> {
        let doc = serde_json::json!({
            "file": self.chunk.file,
            "chunk_id": self.chunk.chunk_id,
            "heading_path": self.chunk.heading_path,
            "text": self.chunk.text,
            "task": TASK_TEXT,
        });
        serde_json::to_string(&doc).context("抽取请求体序列化失败")
    }
}

// ──────────────────────────────────────────────────────────
// 缓存（trait + 内存实现）
// ──────────────────────────────────────────────────────────

/// 缓存里的一条：**模型回答原文** + 当初那次调用的账。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedChunk {
    /// 模型回答的正文（**未校验** —— 每次运行都会重新过证据校验）。
    pub response: String,
    pub model: String,
    pub prompt_sha: String,
    /// 当初那次调用花了多少 token（命中时**不重复计费**，只作溯源）。
    pub tokens: u64,
}

/// 指纹 → [`CachedChunk`]。**同步** trait：编排是同步的，
/// 真实存储层（本地 JSON 文件）在编排**之前/之后**读/写。
pub trait DocCache {
    fn get(&mut self, fingerprint: &str) -> Option<CachedChunk>;
    fn put(&mut self, fingerprint: &str, entry: CachedChunk);
}

/// 内存缓存 —— 测试用，也是文件缓存预加载/回写之间的中转。
#[derive(Debug, Default)]
pub struct MemoryDocCache {
    entries: HashMap<String, CachedChunk>,
    /// 本轮**新写入**的条目（只用来报告「新落盘 N 条」）。
    pub fresh: Vec<(String, CachedChunk)>,
}

impl MemoryDocCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_entries(entries: impl IntoIterator<Item = (String, CachedChunk)>) -> Self {
        Self { entries: entries.into_iter().collect(), fresh: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.fresh.clear();
    }

    pub fn contains(&self, fingerprint: &str) -> bool {
        self.entries.contains_key(fingerprint)
    }

    /// 全部条目（**按指纹排序**，落盘确定性靠它）。
    pub fn sorted(&self) -> Vec<(String, CachedChunk)> {
        let mut v: Vec<(String, CachedChunk)> =
            self.entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}

impl DocCache for MemoryDocCache {
    fn get(&mut self, fingerprint: &str) -> Option<CachedChunk> {
        self.entries.get(fingerprint).cloned()
    }

    fn put(&mut self, fingerprint: &str, entry: CachedChunk) {
        self.entries.insert(fingerprint.to_string(), entry.clone());
        self.fresh.push((fingerprint.to_string(), entry));
    }
}

/// 缓存文件的**磁盘形状**（`cli::doc_cmd` 只负责读写字节，序列化在这里）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheFile {
    pub version: u32,
    pub entries: Vec<CacheEntryJson>,
}

/// 一条缓存条目（JSON 形状，字段名即契约）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntryJson {
    pub fingerprint: String,
    pub response: String,
    pub model: String,
    pub prompt_sha: String,
    pub tokens: u64,
}

pub const CACHE_FILE_VERSION: u32 = 1;

/// 缓存 → JSON（**按指纹排序**，两次运行逐字节相同）。
pub fn cache_to_json(cache: &MemoryDocCache) -> Result<String> {
    let file = CacheFile {
        version: CACHE_FILE_VERSION,
        entries: cache
            .sorted()
            .into_iter()
            .map(|(fingerprint, e)| CacheEntryJson {
                fingerprint,
                response: e.response,
                model: e.model,
                prompt_sha: e.prompt_sha,
                tokens: e.tokens,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).context("缓存序列化失败")
}

/// JSON → 缓存。形状不对 ⇒ **报错**（不静默重建：那会把「缓存坏了」变成一次花钱的正常运行）。
pub fn cache_from_json(text: &str) -> Result<MemoryDocCache> {
    let file: CacheFile =
        serde_json::from_str(text).context("指纹缓存文件读不懂（不是本命令写的形状？）")?;
    if file.version != CACHE_FILE_VERSION {
        bail!(
            "指纹缓存版本是 {}，本命令只认 {CACHE_FILE_VERSION}。\n\
             删掉那个缓存文件重跑即可（会重新调用模型、重新计费），\
             或者给 --cache 指一个别的位置。",
            file.version
        );
    }
    Ok(MemoryDocCache::with_entries(file.entries.into_iter().map(|e| {
        (
            e.fingerprint,
            CachedChunk {
                response: e.response,
                model: e.model,
                prompt_sha: e.prompt_sha,
                tokens: e.tokens,
            },
        )
    })))
}

// ──────────────────────────────────────────────────────────
// 统计
// ──────────────────────────────────────────────────────────

/// 一轮抽取的统计（**失败也要看得见**）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocStats {
    /// 实际送模型的块数（`--limit-chunks` 之后）。
    pub chunks: usize,
    pub assertions: usize,
    pub concepts: usize,
    pub relations: usize,
    pub rejected_no_evidence: usize,
    pub rejected_modality: usize,
    pub rejected_predicate: usize,
    pub rejected_empty: usize,
    pub rejected_parse: usize,
    /// 缓存命中几块（**命中不调用**）
    pub cache_hits: usize,
    /// 真正调用模型几次
    pub calls: usize,
    pub failed_calls: usize,
    /// **本轮**花掉的 token（命中缓存的块不计 —— 那是上一次的钱）
    pub tokens_spent: u64,
    /// 命中缓存里**历史用量**之和（溯源用，不进本轮预算）
    pub tokens_replayed: u64,
    pub budget: u64,
    /// 因预算停止：停在第几块（1-based）
    pub budget_stopped: Option<usize>,
    /// 去重后的概念名集合（只计数；⛔ 不做实体消解）
    pub concept_names: BTreeSet<String>,
    /// 谓词分布（BTreeMap ⇒ 输出顺序确定）
    pub predicate_counts: BTreeMap<String, usize>,
    /// 作废样例（最多 5 条）
    pub rejection_samples: Vec<String>,
}

impl DocStats {
    pub fn rejected(&self) -> usize {
        self.rejected_no_evidence
            + self.rejected_modality
            + self.rejected_predicate
            + self.rejected_empty
            + self.rejected_parse
    }

    /// 去重后概念数（demand 交付物 7 明确要这个数）。
    pub fn distinct_concepts(&self) -> usize {
        self.concept_names.len()
    }

    /// 缓存命中率（分母 = 块数；0 块 ⇒ `None`）。
    pub fn cache_hit_rate(&self) -> Option<f64> {
        if self.chunks == 0 {
            None
        } else {
            Some(self.cache_hits as f64 / self.chunks as f64)
        }
    }

    /// 预算是否被越过。
    ///
    /// 两种都算（与 REQ-109 同一口径）：
    /// - 还有块没跑（`budget_stopped` 有值）；
    /// - 全跑完了，但累计用量**已经超过**上限（最后一次调用把它顶过去了）。
    pub fn over_budget(&self) -> bool {
        self.budget_stopped.is_some() || self.tokens_spent > self.budget
    }

    fn reject(&mut self, why: Rejection, detail: String) {
        match why {
            Rejection::Parse => self.rejected_parse += 1,
            Rejection::Empty => self.rejected_empty += 1,
            Rejection::Modality => self.rejected_modality += 1,
            Rejection::Predicate => self.rejected_predicate += 1,
            Rejection::NoEvidence => self.rejected_no_evidence += 1,
        }
        if self.rejection_samples.len() < 5 {
            self.rejection_samples.push(format!("{}：{detail}", why.as_str()));
        }
    }
}

// ──────────────────────────────────────────────────────────
// 编排 + 预算（R3 / AC-006 / AC-007）
// ──────────────────────────────────────────────────────────

/// 跑一批块。返回 `(断言, 统计)` —— 断言**已排序**、**已过证据校验**。
///
/// # 预算闸的位置
///
/// 在**发起下一次调用之前**判。单次调用的花费在调用前不可知，所以这是
/// 「不再发起」的闸，不是「保证不超」的闸（与 `ai_semantic` 同一条论证）。
///
/// # 命中缓存时
///
/// **不调模型**、`tokens_spent` 不增加（那是上一次的钱），
/// 但回答**照样过** [`parse_extraction`] + [`validate`] —— 缓存省调用，不省校验。
pub fn ingest_all(
    transport: &mut dyn LlmTransport,
    cache: &mut dyn DocCache,
    chunks: &[Chunk],
    model: &str,
    prompts: &Prompts,
    budget: u64,
    max_tokens: u32,
) -> (Vec<Assertion>, DocStats) {
    let mut assertions: Vec<Assertion> = Vec::new();
    let mut stats = DocStats { chunks: chunks.len(), budget, ..Default::default() };

    for (i, chunk) in chunks.iter().enumerate() {
        // ── 预算闸：发起下一次之前判 ──
        if stats.tokens_spent >= budget {
            stats.budget_stopped = Some(i + 1);
            break;
        }

        let req = ChunkRequest::new(chunk.clone());
        let fp = req.fingerprint(&prompts.sha256, model, DOC_AGENT_VERSION);

        // ── 缓存：命中**绝不调用** ──
        if let Some(entry) = cache.get(&fp) {
            stats.cache_hits += 1;
            stats.tokens_replayed += entry.tokens;
            match parse_extraction(&entry.response) {
                Ok(raw) => {
                    let mut got = validate(chunk, raw, &mut stats);
                    assertions.append(&mut got);
                }
                Err(detail) => {
                    // 缓存里是一份当初就解析不了的回答 ⇒ 照样作废（不重复付费）。
                    stats.reject(Rejection::Parse, format!("{}（缓存回放）{detail}", chunk.chunk_id));
                }
            }
            continue;
        }

        let user = match req.user_message() {
            Ok(u) => u,
            Err(e) => {
                stats.failed_calls += 1;
                stats.reject(Rejection::Parse, format!("{} 请求体构造失败：{e:#}", chunk.chunk_id));
                continue;
            }
        };
        let chat = ChatRequest {
            model: model.to_string(),
            messages: vec![Message::system(prompts.system.clone()), Message::user(user)],
            max_tokens,
        };
        stats.calls += 1;
        let resp = match transport.complete(&chat) {
            Ok(r) => r,
            Err(e) => {
                stats.failed_calls += 1;
                // 传输失败 = 这一块**没有回答**（不是「0 个断言」）
                stats.reject(Rejection::Parse, format!("{} 调用失败：{:#}", chunk.chunk_id, e));
                continue;
            }
        };

        // ── 先记账，再解析（钱已经花了）──
        let Some(usage) = resp.usage.as_ref() else {
            stats.failed_calls += 1;
            stats.reject(
                Rejection::Parse,
                format!("{} 的响应没有 usage —— 无法记账，这一块作废", chunk.chunk_id),
            );
            continue;
        };
        let tokens = usage.total_tokens();
        stats.tokens_spent += tokens;

        let text = match completion_text(&resp) {
            Ok(t) => t,
            Err(e) => {
                stats.failed_calls += 1;
                // ⛔ **不写缓存**：截断 / 空正文是**调用参数**（输出上限）或网络的属性，
                // 不是输入文本的属性。把它缓存下来会让「调大 max_tokens 再跑」变成空操作
                // —— 命中一份当初就残缺的回答，而人以为已经修好了。
                stats.reject(Rejection::Parse, format!("{} 正文不可用：{:#}", chunk.chunk_id, e));
                continue;
            }
        };

        // 解析不出 / 校验不过的回答**也写缓存**：同一份回答不该每次重跑再付一次钱
        // （与 REQ-109 的 `CachedOutput::rejected` 同一条论证）。
        cache.put(
            &fp,
            CachedChunk {
                response: text.clone(),
                model: model.to_string(),
                prompt_sha: prompts.sha256.clone(),
                tokens,
            },
        );
        match parse_extraction(&text) {
            Ok(raw) => {
                let mut got = validate(chunk, raw, &mut stats);
                assertions.append(&mut got);
            }
            Err(detail) => stats.reject(Rejection::Parse, format!("{} {detail}", chunk.chunk_id)),
        }
    }

    stats.assertions = assertions.len();
    sort_assertions(&mut assertions);
    (assertions, stats)
}

// ──────────────────────────────────────────────────────────
// 渲染（JSONL / stats.json / 块清单）—— **确定性**
// ──────────────────────────────────────────────────────────

/// 断言 → JSONL（每行一条，末尾换行；**0 条 ⇒ 0 字节**）。
pub fn render_jsonl(assertions: &[Assertion]) -> Result<String> {
    let mut out = String::new();
    for a in assertions {
        out.push_str(&serde_json::to_string(a).context("断言序列化失败")?);
        out.push('\n');
    }
    Ok(out)
}

/// 块清单 → JSONL（`--chunks-only` 的产物；**不调模型**）。
pub fn render_chunks_jsonl(chunks: &[Chunk]) -> Result<String> {
    let mut out = String::new();
    for c in chunks {
        let row = serde_json::json!({
            "file": c.file,
            "chunk_id": c.chunk_id,
            "start": c.start,
            "end": c.end,
            "chars": c.text.chars().count(),
            "heading_path": c.heading_path,
            "text": c.text,
        });
        out.push_str(&serde_json::to_string(&row).context("块清单序列化失败")?);
        out.push('\n');
    }
    Ok(out)
}

/// 一个文件在 stats 里的明细。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatsFile {
    pub file: String,
    pub sha256: String,
    pub bytes: usize,
    pub chunks: usize,
}

/// 丢弃分类的计数（`total` 必须等于四项之和 —— 在 [`render_stats`] 里现算）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatsRejected {
    pub total: usize,
    pub no_evidence: usize,
    pub modality: usize,
    pub predicate: usize,
    pub empty: usize,
    pub parse: usize,
}

/// `stats.json` 的磁盘形状（字段顺序即输出顺序：确定性）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatsReport {
    pub input: String,
    pub files: usize,
    pub chunks: usize,
    pub assertions: usize,
    pub concepts: usize,
    pub relations: usize,
    pub rejected: StatsRejected,
    pub cache_hits: usize,
    pub calls: usize,
    pub failed_calls: usize,
    pub tokens_spent: u64,
    pub tokens_replayed: u64,
    pub distinct_concepts: usize,
    pub predicate_counts: BTreeMap<String, usize>,
    pub budget_limit: u64,
    pub budget_stopped_at: Option<usize>,
    pub truncated: bool,
    pub model: String,
    pub prompt_sha: String,
    pub agent_version: String,
    pub files_detail: Vec<StatsFile>,
    pub rejection_samples: Vec<String>,
}

/// 造 `stats.json` 的内容（pretty JSON，键顺序固定）。
///
/// `truncated` = 「还有块没跑」：`--limit-chunks` 截断了，**或**预算停了。
/// 两种都必须显式标出来 —— 一份看起来完整的半份产物是最坏的形态（AC-007）。
#[allow(clippy::too_many_arguments)]
pub fn render_stats(
    input: &str,
    docs: &[SourceDoc],
    all_chunks: &[Chunk],
    stats: &DocStats,
    limit: Option<usize>,
    model: &str,
    prompt_sha: &str,
) -> Result<String> {
    let mut per_file: BTreeMap<&str, usize> = BTreeMap::new();
    for c in all_chunks {
        *per_file.entry(c.file.as_str()).or_insert(0) += 1;
    }
    let files_detail: Vec<StatsFile> = docs
        .iter()
        .map(|d| StatsFile {
            file: d.rel.clone(),
            sha256: d.sha256.clone(),
            bytes: d.raw_bytes,
            chunks: per_file.get(d.rel.as_str()).copied().unwrap_or(0),
        })
        .collect();
    let report = StatsReport {
        input: input.to_string(),
        files: docs.len(),
        chunks: stats.chunks,
        assertions: stats.assertions,
        concepts: stats.concepts,
        relations: stats.relations,
        rejected: StatsRejected {
            total: stats.rejected(),
            no_evidence: stats.rejected_no_evidence,
            modality: stats.rejected_modality,
            predicate: stats.rejected_predicate,
            empty: stats.rejected_empty,
            parse: stats.rejected_parse,
        },
        cache_hits: stats.cache_hits,
        calls: stats.calls,
        failed_calls: stats.failed_calls,
        tokens_spent: stats.tokens_spent,
        tokens_replayed: stats.tokens_replayed,
        distinct_concepts: stats.distinct_concepts(),
        predicate_counts: stats.predicate_counts.clone(),
        budget_limit: stats.budget,
        budget_stopped_at: stats.budget_stopped,
        truncated: stats.budget_stopped.is_some()
            || limit.is_some_and(|n| all_chunks.len() > n),
        model: model.to_string(),
        prompt_sha: prompt_sha.to_string(),
        agent_version: DOC_AGENT_VERSION.to_string(),
        files_detail,
        rejection_samples: stats.rejection_samples.clone(),
    };
    serde_json::to_string_pretty(&report).context("stats.json 序列化失败")
}

// ──────────────────────────────────────────────────────────
// 预算（AC-007）
// ──────────────────────────────────────────────────────────

/// 预算闸的**唯一判定**：给了 `--budget-tokens` 就必须 > 0；没给就用默认上限。
///
/// 放在本模块而不是 bin 里：这条规则属于「这次抽取能不能开跑」，
/// 与「哪个 CLI 打开了它」无关（与 `ai_semantic::require_ai_budget` 同一条理由）。
pub fn require_budget(budget: Option<u64>) -> Result<u64> {
    match budget {
        Some(0) => bail!(
            "--budget-tokens 是 0：0 的意思是「一次模型调用都不许」，\
             那与「不跑」是同一件事 —— 要跑就给一个真实上限（例如 200000）。"
        ),
        Some(b) => Ok(b),
        None => Ok(DEFAULT_BUDGET_TOKENS),
    }
}

/// 本通道实际发出的输出上限（规则见 [`DEFAULT_MAX_TOKENS`]）。
///
/// `explicitly_set` = 运维有没有显式设 `ONTOK_LLM_MAX_TOKENS`。
/// 显式设了就**不许被代码盖掉**（部署事实），没设才用本通道的默认值。
pub fn resolve_max_tokens(configured: u32, explicitly_set: bool) -> u32 {
    if explicitly_set {
        configured
    } else {
        DEFAULT_MAX_TOKENS
    }
}

// ──────────────────────────────────────────────────────────
// 单测（零网络、零磁盘）
// ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::llm::{ChatResponse, ScriptedTransport};
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn doc(text: &str) -> SourceDoc {
        SourceDoc::from_text("docs/a.md", text)
    }

    fn reply(content: &str, tokens: u64) -> ChatResponse {
        ChatResponse {
            choices: vec![crate::agent::llm::protocol::Choice {
                message: crate::agent::llm::protocol::Message_ {
                    content: Some(content.to_string()),
                    reasoning_content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(crate::agent::llm::protocol::Usage {
                prompt_tokens: tokens.saturating_sub(1),
                completion_tokens: 1,
                total_tokens: Some(tokens),
                completion_tokens_details: None,
            }),
        }
    }

    // ── 分块 ──────────────────────────────────────────────

    /// R2：同文件两次分块 ⇒ 块 id 与内容**逐字**相同。
    #[test]
    fn chunking_is_deterministic() {
        let d = doc("# 订单\n\n客户下单后生成订单。\n\n## 支付\n\n平台扣费。\n");
        let a = chunk_document(&d);
        let b = chunk_document(&d);
        assert_eq!(a, b, "同一份文件两次分块必须逐字相同");
        assert!(!a.is_empty());
        // 块 id 的形状：sha256(file)[:12]:start-end
        for c in &a {
            let (sha, range) = c.chunk_id.split_once(':').expect("块 id 形状");
            assert_eq!(sha.len(), 12);
            let (s, e) = range.split_once('-').unwrap();
            assert_eq!((s.parse::<usize>().unwrap(), e.parse::<usize>().unwrap()), (c.start, c.end));
            assert_eq!(&d.text[c.start..c.end], c.text, "块文本必须是原文切片");
        }
    }

    /// R6：标题层级与段落是边界，**句子不被切开**。
    #[test]
    fn chunking_respects_headings_and_paragraphs() {
        let d = doc("# 一\n\n第一段。\n\n第二段。\n\n# 二\n\n第三段。\n");
        let chunks = chunk_document(&d);
        // 两个 1 级标题 ⇒ 至少两块，且每个块都以标题行开头
        assert!(chunks.len() >= 2, "{chunks:?}");
        assert!(chunks[0].text.starts_with("# 一"));
        assert!(chunks[1].text.starts_with("# 二"));
        // 每个块都是完整句子（以句号收尾），没有从中间切断
        for c in &chunks {
            assert!(c.text.ends_with('。') || c.text.ends_with("# 一") || c.text.ends_with("# 二"));
        }
        assert_eq!(chunks[1].heading_path, "二");
    }

    /// 超长段落**独占一块**、不被切句（宁可块大，不可切句）。
    #[test]
    fn oversized_paragraph_is_never_cut() {
        let long = "这是一句很长的话，".repeat(400) + "结束。";
        let d = doc(&format!("# 长\n\n{long}\n"));
        let chunks = chunk_document(&d);
        assert!(chunks.iter().any(|c| c.text.contains(&long)), "超长段落必须完整保留");
    }

    /// CRLF 归一化后分块结果与 LF 一致（证据校验的容忍面之一）。
    #[test]
    fn crlf_and_lf_chunk_identically() {
        let lf = SourceDoc::from_text("a.md", "# 标题\n\n正文。\n");
        let crlf = SourceDoc::from_text("a.md", "# 标题\r\n\r\n正文。\r\n");
        assert_eq!(lf.text, crlf.text);
        assert_eq!(lf.sha256, crlf.sha256);
        assert_eq!(chunk_all(&[lf]), chunk_all(&[crlf]));
    }

    // ── 证据校验（AC-003）──────────────────────────────────

    /// 逐字命中：span 精确指向原文。
    #[test]
    fn quote_that_appears_verbatim_is_located() {
        let text = "客户在平台下单，平台随后扣费。";
        let (s, e) = locate_quote(text, "平台随后扣费").expect("逐字命中");
        assert_eq!(&text[s..e], "平台随后扣费");
    }

    /// 空白差异（换行/多空格）仍然命中 —— 这是 demand 允许的**唯一**容忍面。
    #[test]
    fn whitespace_differences_are_tolerated() {
        let text = "客户\n在平台   下单";
        let (s, e) = locate_quote(text, "客户 在平台 下单").expect("空白归一化后应命中");
        assert_eq!(&text[s..e], "客户\n在平台   下单");
    }

    /// ★ 伪造的 quote ⇒ **必须**找不到（证据校验有牙）。
    #[test]
    fn forged_quote_is_rejected() {
        let text = "客户在平台下单，平台随后扣费。";
        assert_eq!(locate_quote(text, "客户在平台退款"), None, "改了动词的 quote 不能命中");
        assert_eq!(locate_quote(text, "平台随后退款"), None);
        assert_eq!(locate_quote(text, "客户在平台下单。"), None, "改标点也不行");
        // 空 quote 也不算证据
        assert_eq!(locate_quote(text, "   "), None);
    }

    /// ★ 放宽成「前缀匹配」必须被抓住：quote 是原文的**前缀 + 原文没有的尾巴**。
    #[test]
    fn prefix_only_match_is_not_accepted() {
        let text = "客户在平台下单，平台随后扣费。";
        // 以原文开头，但后半句原文里没有 ⇒ 前缀匹配会误放，逐字匹配必须拒
        assert_eq!(locate_quote(text, "客户在平台下单，平台随后发货"), None);
        // 反过来：原文的前缀本身若真是原文，那是合法证据（防止判据恒红）
        assert!(locate_quote(text, "客户在平台下单").is_some());
    }

    // ── 校验 + 计数（R1 / AC-004 / AC-005）───────────────

    fn chunk_of(text: &str) -> Chunk {
        chunk_document(&doc(text)).remove(0)
    }

    fn raw(v: Value) -> RawExtraction {
        serde_json::from_value(v).expect("测试用回答形状合法")
    }

    /// 正常条目：过；`future` ⇒ `alignable: false`（AC-005）。
    #[test]
    fn valid_items_pass_and_future_is_not_alignable() {
        let c = chunk_of("# 订阅\n\n用户订阅套餐，未来将支持按月扣费。\n");
        let mut stats = DocStats::default();
        let got = validate(
            &c,
            raw(serde_json::json!({
                "concepts": [{"name": "套餐", "quote": "用户订阅套餐", "modality": "fact"}],
                "relations": [
                    {"subject": "用户", "predicate": "订阅", "object": "套餐",
                     "quote": "用户订阅套餐", "modality": "fact"},
                    {"subject": "平台", "predicate": "扣费", "object": "用户",
                     "quote": "未来将支持按月扣费", "modality": "future"}
                ]
            })),
            &mut stats,
        );
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(stats.rejected(), 0);
        assert!(got[0].is_concept());
        let future = got.iter().find(|a| a.modality() == Modality::Future).expect("future 条目应保留");
        assert!(!future_is_alignable(future), "future 必须标 alignable=false");
        // 事实条目可对齐
        assert!(got.iter().filter(|a| a.modality() == Modality::Fact).all(alignable));
    }

    fn future_is_alignable(a: &Assertion) -> bool {
        match a {
            Assertion::Concept(c) => c.alignable,
            Assertion::Relation(r) => r.alignable,
        }
    }

    fn alignable(a: &Assertion) -> bool {
        future_is_alignable(a)
    }

    /// ★ 伪造 quote ⇒ 丢弃 + 计数，且**不进**返回的断言。
    #[test]
    fn forged_quotes_are_dropped_and_counted() {
        let c = chunk_of("# 订单\n\n客户在平台下单。\n");
        let mut stats = DocStats::default();
        let got = validate(
            &c,
            raw(serde_json::json!({
                "concepts": [{"name": "订单", "quote": "客户在平台退款", "modality": "fact"}],
                "relations": [{"subject": "客户", "predicate": "下单", "object": "订单",
                               "quote": "客户在平台退款", "modality": "fact"}]
            })),
            &mut stats,
        );
        assert!(got.is_empty(), "伪造证据的条目一条都不许留下：{got:?}");
        assert_eq!(stats.rejected_no_evidence, 2);
        assert_eq!(stats.rejected(), 2);
    }

    /// 谓词退化成结构词 ⇒ 丢弃（AC-004）。
    #[test]
    fn structural_predicates_are_rejected() {
        let c = chunk_of("# 关系\n\n订单属于客户。\n");
        let mut stats = DocStats::default();
        let got = validate(
            &c,
            raw(serde_json::json!({
                "relations": [{"subject": "订单", "predicate": "belongs_to", "object": "客户",
                               "quote": "订单属于客户", "modality": "fact"}]
            })),
            &mut stats,
        );
        assert!(got.is_empty());
        assert_eq!(stats.rejected_predicate, 1);
    }

    /// 模态缺失/越界 ⇒ 丢弃（R4：模态不是可选项）。
    #[test]
    fn missing_or_unknown_modality_is_rejected() {
        let c = chunk_of("# 订单\n\n客户下单。\n");
        let mut stats = DocStats::default();
        let got = validate(
            &c,
            raw(serde_json::json!({
                "concepts": [
                    {"name": "订单", "quote": "客户下单", "modality": "opinion"},
                    {"name": "客户", "quote": "客户下单"}
                ]
            })),
            &mut stats,
        );
        assert!(got.is_empty());
        assert_eq!(stats.rejected_modality, 2);
    }

    /// 代码围栏会被剥掉；形状不对则整块作废（不猜）。
    #[test]
    fn fenced_json_is_stripped_but_garbage_is_rejected() {
        let fenced = "```json\n{\"concepts\":[],\"relations\":[]}\n```";
        assert!(parse_extraction(fenced).is_ok());
        assert!(parse_extraction("[1,2,3]").is_err());
        assert!(parse_extraction("好的，我来抽取：").is_err());
    }

    // ── 指纹 / 缓存 / 预算 ────────────────────────────────

    #[test]
    fn fingerprint_covers_all_four_inputs() {
        let req = ChunkRequest::new(chunk_of("# 订单\n\n客户下单。\n"));
        let base = req.fingerprint("p1", "m1", "v1");
        assert_ne!(base, req.fingerprint("p2", "m1", "v1"), "换提示词必须换指纹");
        assert_ne!(base, req.fingerprint("p1", "m2", "v1"), "换模型必须换指纹");
        assert_ne!(base, req.fingerprint("p1", "m1", "v2"), "换逻辑版本必须换指纹");
        let other = ChunkRequest::new(chunk_of("# 支付\n\n平台扣费。\n"));
        assert_ne!(base, other.fingerprint("p1", "m1", "v1"), "换块必须换指纹");
        assert_eq!(base, req.fingerprint("p1", "m1", "v1"));
    }

    fn one_chunk_doc() -> Vec<Chunk> {
        chunk_document(&doc("# 订单\n\n客户下单，平台扣费。\n"))
    }

    /// ★ R3 / AC-006：第二次运行**0 次调用**，断言完全相同。
    #[test]
    fn second_run_hits_cache_and_never_calls() {
        let chunks = one_chunk_doc();
        let prompts = Prompts::standard();
        let answer = r#"{"concepts":[{"name":"订单","quote":"客户下单","modality":"fact"}],
                         "relations":[{"subject":"客户","predicate":"下单","object":"订单","quote":"客户下单","modality":"fact"}]}"#;
        let mut transport = ScriptedTransport::new(vec![Ok(reply(answer, 17))]);
        let mut cache = MemoryDocCache::new();

        let (a1, s1) = ingest_all(&mut transport, &mut cache, &chunks, "m", &prompts, 100, 64);
        assert_eq!(s1.calls, 1);
        assert_eq!(s1.tokens_spent, 17);
        assert_eq!(s1.cache_hits, 0);
        assert_eq!(a1.len(), 2, "{a1:?}");

        // 第二次：同一个缓存、一个「问什么都失败」的传输 ⇒ 若真调用了就会失败
        let mut failing = ScriptedTransport::always_answer(|_| {
            anyhow::bail!("命中缓存时绝不该调用模型")
        });
        let (a2, s2) = ingest_all(&mut failing, &mut cache, &chunks, "m", &prompts, 100, 64);
        assert_eq!(failing.call_count(), 0, "命中缓存必须 0 次调用");
        assert_eq!(s2.cache_hits, 1);
        assert_eq!(s2.tokens_spent, 0, "第二次运行 0 token");
        assert_eq!(s2.tokens_replayed, 17, "历史用量如实回放（不进本轮支出）");
        assert_eq!(a1, a2, "两次运行的断言必须逐字相同");
        assert_eq!(
            render_jsonl(&a1).unwrap(),
            render_jsonl(&a2).unwrap(),
            "JSONL 必须逐字节相同"
        );
    }

    /// AC-007：`--budget-tokens 1` 的实际行为 —— 第一次调用后即停，
    /// 报告「停在第 2 块」，且 `over_budget()` 为真（调用方据此不出产物）。
    #[test]
    fn budget_one_stops_after_the_first_chunk() {
        // 三块：三个 1 级标题，每块都很短
        let chunks = chunk_document(&doc("# 一\n\n甲。\n\n# 二\n\n乙。\n\n# 三\n\n丙。\n"));
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        let prompts = Prompts::standard();
        let answer = r#"{"concepts":[],"relations":[]}"#;
        let mut transport = ScriptedTransport::new(vec![
            Ok(reply(answer, 9)),
            Ok(reply(answer, 9)),
            Ok(reply(answer, 9)),
        ]);
        let mut cache = MemoryDocCache::new();
        let (_, stats) = ingest_all(&mut transport, &mut cache, &chunks, "m", &prompts, 1, 64);
        assert_eq!(stats.calls, 1, "预算 1 ⇒ 只允许第一次调用");
        assert_eq!(stats.tokens_spent, 9);
        assert_eq!(stats.budget_stopped, Some(2), "停在第 2 块之前");
        assert!(stats.over_budget(), "越过预算必须为真（调用方据此不出产物）");
    }

    /// 缓存文件：排序落盘 + 读回来等价；版本不对 ⇒ 报错（不静默重建）。
    #[test]
    fn cache_file_roundtrip_is_sorted_and_version_checked() {
        let mut cache = MemoryDocCache::new();
        cache.put("bbb", CachedChunk { response: "{}".into(), model: "m".into(), prompt_sha: "p".into(), tokens: 2 });
        cache.put("aaa", CachedChunk { response: "[]".into(), model: "m".into(), prompt_sha: "p".into(), tokens: 3 });
        let json = cache_to_json(&cache).unwrap();
        assert!(json.find("\"aaa\"").unwrap() < json.find("\"bbb\"").unwrap(), "必须按指纹排序");
        let back = cache_from_json(&json).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back.contains("aaa") && back.contains("bbb"));
        // 常见误输入：直接塞一个空对象 ⇒ 报错而不是「当空缓存」
        assert!(cache_from_json("{}").is_err());
        assert!(cache_from_json("[1,2]").is_err());
        // 版本不对
        let bad = json.replace("\"version\": 1", "\"version\": 99");
        assert!(cache_from_json(&bad).is_err());
    }

    /// 传输失败：这一块**没有回答**（不是「0 个断言」），且失败要被计数。
    #[test]
    fn transport_failure_is_counted_not_silently_zero() {
        let chunks = one_chunk_doc();
        let prompts = Prompts::standard();
        let mut transport = ScriptedTransport::always_answer(|_| anyhow::bail!("连不上"));
        let mut cache = MemoryDocCache::new();
        let (assertions, stats) =
            ingest_all(&mut transport, &mut cache, &chunks, "m", &prompts, 100, 64);
        assert!(assertions.is_empty());
        assert_eq!(stats.failed_calls, 1);
        assert_eq!(stats.rejected_parse, 1);
        assert_eq!(stats.cache_hits, 0);
        assert!(cache.is_empty(), "传输失败不许进缓存（下次要能重试）");
    }

    /// ★ 截断的正文**不许进缓存** —— 否则「调大 max_tokens 再跑」是空操作。
    #[test]
    fn truncated_answer_is_not_cached_so_raising_the_cap_can_retry() {
        use crate::agent::llm::protocol::{Choice, Message_, Usage};
        let chunks = one_chunk_doc();
        let prompts = Prompts::standard();
        let truncated = ChatResponse {
            choices: vec![Choice {
                message: Message_ { content: None, reasoning_content: Some("思考中…".into()) },
                finish_reason: Some("length".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 100,
                completion_tokens: 200,
                total_tokens: Some(300),
                completion_tokens_details: None,
            }),
        };
        let mut transport = ScriptedTransport::new(vec![Ok(truncated)]);
        let mut cache = MemoryDocCache::new();
        let (_, s1) = ingest_all(&mut transport, &mut cache, &chunks, "m", &prompts, 1000, 64);
        assert_eq!(s1.failed_calls, 1);
        assert_eq!(s1.tokens_spent, 300, "钱已经花了，账要记");
        assert!(cache.is_empty(), "截断的回答不许进缓存");

        // 第二次（比如运维调大了输出上限）：必须**真的再调一次**
        let mut transport2 = ScriptedTransport::new(vec![Ok(reply(
            r#"{"concepts":[],"relations":[]}"#,
            5,
        ))]);
        let (_, s2) = ingest_all(&mut transport2, &mut cache, &chunks, "m", &prompts, 1000, 64);
        assert_eq!(s2.calls, 1, "上一次是截断，不许被当成已完成的回答");
        assert_eq!(cache.len(), 1);
    }

    /// 本通道的输出上限默认值比 REQ-109 的单条判定大；运维显式设了则以它为准。
    #[test]
    fn max_tokens_default_is_doc_specific_but_env_wins() {
        assert_eq!(resolve_max_tokens(300, false), DEFAULT_MAX_TOKENS);
        assert_eq!(resolve_max_tokens(8192, true), 8192);
        assert!(
            DEFAULT_MAX_TOKENS > crate::agent::llm::LlmConfig::DEFAULT_MAX_TOKENS,
            "文档抽取一块要回多条断言，300 个 token 不够（本机实测：截断）"
        );
    }

    // ── 格式闸（AC-001）──────────────────────────────────

    #[test]
    fn unsupported_formats_are_named() {
        for ext in UNSUPPORTED_FORMATS {
            let err = unsupported_format_error(ext, Path::new(&format!("x.{ext}"))).to_string();
            assert!(err.contains(&format!(".{ext}")), "必须点名格式：{err}");
            assert!(err.contains("先把它转成 .md/.txt"), "必须提示先转文本：{err}");
        }
        assert!(check_ext(Path::new("a.md")).is_ok());
        assert!(check_ext(Path::new("a.TXT")).is_ok(), "扩展名大小写不敏感");
        assert!(check_ext(Path::new("a.pdf")).is_err());
        assert!(check_ext(Path::new("a.docx")).is_err());
        assert!(check_ext(Path::new("a.doc")).is_err());
    }

    /// 统计渲染：字段齐全、`truncated` 标得出来、键顺序稳定。
    #[test]
    fn stats_report_is_complete_and_stable() {
        let d = doc("# 订单\n\n客户下单。\n");
        let chunks = chunk_all(std::slice::from_ref(&d));
        let mut stats = DocStats { chunks: chunks.len(), budget: 100, ..Default::default() };
        stats.concepts = 1;
        stats.assertions = 1;
        stats.concept_names.insert("订单".into());
        let a = render_stats("docs", &[d.clone()], &chunks, &stats, None, "m", "p").unwrap();
        let b = render_stats("docs", &[d], &chunks, &stats, None, "m", "p").unwrap();
        assert_eq!(a, b, "同输入两次渲染必须逐字节相同");
        for needle in [
            "\"files\"", "\"chunks\"", "\"assertions\"", "\"rejected\"", "\"cache_hits\"",
            "\"tokens_spent\"", "\"distinct_concepts\"", "\"predicate_counts\"", "\"truncated\": false",
        ] {
            assert!(a.contains(needle), "stats.json 缺 {needle}：{a}");
        }
        // --limit-chunks 截断 ⇒ truncated: true（半份产物必须标出来）
        let c = render_stats("docs", &[], &chunks, &stats, Some(0), "m", "p").unwrap();
        assert!(c.contains("\"truncated\": true"));
    }

    /// 计数用的原子量：确认 `ingest_all` 的调用次数与 token 与传输层一致。
    #[test]
    fn calls_and_tokens_are_accounted_from_the_transport() {
        let chunks = chunk_document(&doc("# 一\n\n甲。\n\n# 二\n\n乙。\n"));
        assert_eq!(chunks.len(), 2);
        let prompts = Prompts::standard();
        let seen = Arc::new(AtomicUsize::new(0));
        let s = Arc::clone(&seen);
        let mut transport = ScriptedTransport::always_answer(move |_| {
            s.fetch_add(1, Ordering::SeqCst);
            Ok(reply(r#"{"concepts":[],"relations":[]}"#, 5))
        });
        let mut cache = MemoryDocCache::new();
        let (_, stats) = ingest_all(&mut transport, &mut cache, &chunks, "m", &prompts, 1000, 64);
        assert_eq!(seen.load(Ordering::SeqCst), 2);
        assert_eq!(stats.calls, 2);
        assert_eq!(stats.tokens_spent, 10);
        assert_eq!(cache.len(), 2, "每块一条缓存");
    }
}
