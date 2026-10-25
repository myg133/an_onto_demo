//! **REQ-147 · `doc ingest --agent acp`** —— 把「一块一块地问模型」外包给 ACP 子进程。
//!
//! ```text
//! 块 ──prompt（skill 契约 + 输出协议 + 块 JSON）──▶ pi-acp（ACP 子进程）
//!   ◀──────────────── 累积正文（严格 JSON）+ usage_update
//!     ──doc_ingest::parse_extraction + validate（同一套）──▶ 断言
//! ```
//!
//! # 一句话：本模块**只换搬运工，不换判据**
//!
//! | 层 | 谁负责 | 本 REQ 动了吗 |
//! | --- | --- | --- |
//! | 分块 / 指纹 / 预算 / 证据校验 / 丢弃计数 / 渲染 | [`crate::extract::doc_ingest`] | ⛔ **一行没动** |
//! | 传输（一次问答从哪来） | **本模块**（ACP）或 `agent::llm::HttpTransport`（内置） | ✅ 新增 |
//! | 读文件 / 写产物 / 缓存落盘 | `cli::doc_ingest_cmd` | ✅ 加了分支 |
//!
//! 因为 [`crate::extract::doc_ingest::ingest_all`] 是**唯一**的编排，
//! ACP 与内置两条路的 `assertions.jsonl` 形状、证据校验、丢弃计数、
//! 指纹缓存的语义**结构性相同**（R2 / R5 不是靠人盯着，是靠只有一份实现）。
//!
//! # ★ AC-005 为什么在 ACP 上「免费」成立
//!
//! 证据校验的**唯一实现**是 [`crate::extract::doc_ingest::locate_quote`]，
//! 它在 `validate` 里被调用。ACP 拿回来的正文**照样**走那条路 ——
//! 于是「agent 编了一条原文里没有的 quote」的结果还是同一个：整条丢弃 +
//! 计入 `rejected_no_evidence`（本通道在屏幕上叫 `discarded_no_quote`）。
//! 本模块**没有**第二份 quote 判据，也不许加（纪律 H）。
//!
//! # ★ 缓存为什么在 ACP 上仍然确定性（R2 / AC-003）
//!
//! 指纹 = `sha256(块文本 + prompt_sha + model + DOC_AGENT_VERSION)`（`doc_ingest` 的口径）。
//! 本通道把两件事塞进那两项，于是「换 skill」「换 agent 命令」「agent 升版本」
//! 都会让缓存**整体失效**（而不是回放一份旧契约下的回答）：
//!
//! - `prompt_sha` = `sha256(skill 原文 + 输出协议)`（见 [`system_prompt`]）；
//! - `model` = [`model_id`]：`acp:<程序> <参数>@<握手 agent>#<握手版本>`。
//!
//! 缓存里存的是**回答原文**，每次运行照样重新过证据校验 —— 与内置路径同一条纪律。
//!
//! # ★ 失败要响（R3）：本模块**没有**任何回落分支
//!
//! | 构造 | 行为 |
//! | --- | --- |
//! | 程序不存在 / 拉不起来 | [`connect`] 直接 `bail!`，**一个产物都不写** |
//! | 握手不合格（名字/版本为空或模糊） | 同上 |
//! | 单轮超时 / 对端崩了 | 该块 `failed_calls += 1`；全部失败则由 [`require_usable_output`] 拒绝出产物 |
//! | 对端不报 `usage_update` | 该块作废（**不按 0 记账**：0 是「免费」的谎） |
//! | 一条断言都没抽出来 | [`require_usable_output`] 拒绝出产物 |
//!
//! ⛔ **绝不回落到内置 LLM**：`--agent acp` 的产物要么是 ACP 抽的，
//! 要么没有产物。回落会让「这份 JSONL 是谁抽的」变成一句没人能核的话
//! （与 `agent::factory` 模块头同一条论证）。
//!
//! # ⚠️ 实测到的 ACP 侧事实（pi-acp 0.0.34，2026-10-25）
//!
//! 1. **会话首轮会先吐一段「## Skills / ## Extensions」前言**（实测 5501 字符），
//!    正文 JSON 在后面 —— 直接解析必然失败。所以本模块在**第一次真实调用之前**
//!    先发一条固定的 [`WARMUP_PROMPT`] 并**丢掉它的正文**，把那段前言消耗掉
//!    （见 [`AcpChatTransport::warm_up`]）。
//!    代价：一次多余往返（实测 1.7 秒），且**只在真的有一次调用时**发生 ——
//!    全命中缓存的重跑仍然 **0 次调用**（AC-003）。
//! 2. `session/prompt` 的终响应**不带正文**（只有 `stopReason`），正文在
//!    `session/update` 的 `agent_message_chunk` 里 —— 这一层由
//!    `agent::acp::protocol::PromptAccumulator` 负责，本模块拿到的是**拼好的**正文。
//! 3. `usage_update.used` 是**会话累计**，不是本块的增量。直接当本块用量会
//!    把 95 块算成 95 倍，预算闸在第一块就误停。本模块按「累计之差」换算（见
//!    [`AcpChatTransport::complete`]）。
//!
//! # 已知限制（⛔ 不许写成「已支持」）
//!
//! - **skill 是逐块内联进 prompt 的**：每次真调用都带一份 skill 全文
//!   （约 4 KB ≈ 1.5k token）。为什么不「会话开头发一次」：那会让
//!   **全命中缓存的重跑也要发一次调用**，直接破坏 AC-003 的「第二次 0 次调用」。
//! - 换一个 ACP agent 若**每轮**都夹前言，本通道会解析失败并显式报错 ——
//!   ⛔ 不猜、不「从散文里抠一段 JSON」（那会把「回答不是 JSON」变成静默的抽错）。
//! - 单会话跑完一份文档：中途 `ThreadTransport` 一旦粘住（`poisoned`），
//!   后续块全部失败 ⇒ [`require_usable_output`] 拒绝出产物（⛔ 不出半份）。
//! - 本模块**不设** `max_tokens`：ACP 侧没有这个参数，输出上限由对端决定
//!   （内置路径才有 `ONTOK_LLM_MAX_TOKENS`）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::agent::acp::protocol::UsageSample;
use crate::agent::acp::{AcpConfig, AcpTransport, ThreadTransport};
use crate::agent::llm::protocol::{ChatRequest, ChatResponse, Choice, Message_, Usage};
use crate::agent::llm::LlmTransport;
use crate::agent::registry::is_vague_version;
use crate::extract::doc_ingest::DocStats;

/// `--agent` 的**已实现**取值（本 REQ 只有一个）。
pub const AGENT_ACP: &str = "acp";

/// `--agent-skill` 指向目录时，在里面找这个文件名。
///
/// 只认一个名字（而不是「目录里任意 `.md`」）：传一个目录是给人省事的写法，
/// 不是让程序猜「哪一个才是契约」。
pub const SKILL_FILE_NAME: &str = "SKILL.md";

/// 会话**首轮**的预热 prompt —— 正文丢掉，只为消耗掉对端的前言。
///
/// 内容刻意最短、最无副作用：不要求抽取，也不进入任何产物或缓存。
/// 它**不进指纹**（命中缓存时根本不发），所以改它不会让缓存失效。
pub const WARMUP_PROMPT: &str = "只回两个字：就绪";

/// 追加在 skill 原文之后的**输出协议**（本通道的传输要求，不是 skill 的一部分）。
///
/// # 为什么必须单独一段
///
/// `--agent-skill` 给的技能文档（`Deploy/skills/ontok-extract/SKILL.md`）描述的是
/// **抽取契约**（什么算概念 / 什么算关系 / 证据怎么给），它举的输出例子是
/// **逐行 JSONL**。而 [`crate::extract::doc_ingest::parse_extraction`] 只认
/// `{"concepts": [...], "relations": [...]}` 这一个 JSON 对象 —— 两者形状不同。
///
/// 所以本通道必须**显式仲裁**：契约照 skill，**响应形状照本节**。
/// ⛔ 不改 skill 一个字（那是 BA 的交付物，R4：路径由参数传、内容不写死在代码里）。
/// ⛔ 也不放宽解析器去「兼容 JSONL」：那会让两种形状都能进产物，
/// 下游（`doc merge`）就要处理两种输入，而 schema 一致性（R5）当场破掉。
pub const OUTPUT_PROTOCOL: &str = r#"
---

## 输出协议（**本通道的传输要求**，机器会逐字段校验）

> 上一节描述的「逐行 JSONL」是对**产物**的描述。**本通道的响应形状以本节为准** ——
> 你收到的 `user` 消息是**一段**文档原文，你要回的也是**一个** JSON 对象。

你收到的 `user` 消息是一个 JSON：`{"file","chunk_id","heading_path","text","task"}`。
你只处理 `text` 这一段，⛔ 不要读文件、⛔ 不要调用任何工具。

**你的整个回答必须是一个 JSON 对象**（不要 markdown 代码块、不要任何解释文字、不要逐行 JSONL）：

{"concepts": [{"name": "概念名", "aliases": ["别名"], "description": "一句话说明", "quote": "原文片段", "modality": "fact"}],
 "relations": [{"subject": "主体", "predicate": "关系名", "object": "客体", "cardinality": "1:N", "quote": "原文片段", "modality": "fact"}]}

规则（每一条都会**被程序校验**，不合规的条目**整条丢弃**）：

1. `quote` 必须是 `text` 里**逐字出现**的一段原文（一句话或一个分句）。
   ⛔ 不许改写、不许拼接、不许加省略号、不许翻译、不许把标题当引文。
2. `modality` **必填**，只能是 `fact` / `spec` / `future` / `example` 四个之一。
3. `cardinality` 只能是 `1:1` / `1:N` / `N:1` / `N:N`，不确定就省略。
4. `aliases` / `description` 不确定就省略或给空数组。
5. 这一段里没有可抽的内容，就回 `{"concepts": [], "relations": []}`。
6. ⛔ **不要**输出 `span` / `file` / `chunk_id` —— 本通道由程序用 `quote` 在原文里
   定位后回填（你给的偏移量与程序的不一定同源，信错一个就整份证据作废）。
"#;

// ──────────────────────────────────────────────────────────
// skill / 提示词
// ──────────────────────────────────────────────────────────

/// 读 skill（**路径由 `--agent-skill` 给，代码里不写死** —— R4）。
///
/// 传目录 ⇒ 在里面找 [`SKILL_FILE_NAME`]；传文件 ⇒ 直接用。
/// 空文件 / 读不到 ⇒ 报错（⛔ 不回落成「只有输出协议」的空契约：
/// 那等于悄悄把 skill 丢掉了，而屏幕上看不出区别）。
pub fn load_skill(path: &Path) -> Result<String> {
    let file: PathBuf = if path.is_dir() {
        let joined = path.join(SKILL_FILE_NAME);
        if !joined.is_file() {
            bail!(
                "--agent-skill 给的是目录 {}，但里面没有 {SKILL_FILE_NAME}。\n\
                 请直接指向契约文件，或把契约放成 <目录>/{SKILL_FILE_NAME}。",
                path.display()
            );
        }
        joined
    } else {
        path.to_path_buf()
    };
    let text = std::fs::read_to_string(&file).with_context(|| {
        format!(
            "读不到 skill 契约：{}（--agent-skill 必须指向一份存在的 .md 文件）\n\
             ⛔ 不回落成内置提示词：那会让「这次跑的到底是哪个契约」在产物里看不出来。",
            file.display()
        )
    })?;
    if text.trim().is_empty() {
        bail!(
            "skill 契约是空的：{}。\n\
             空契约等于没有约束，而抽出来的每一条都会被当作「按契约抽的」。",
            file.display()
        );
    }
    Ok(text)
}

/// 本通道的 **system 消息** = skill 原文 + [`OUTPUT_PROTOCOL`]。
///
/// 它的 `sha256` 就是缓存指纹里的 `prompt_sha` ⇒ 改 skill 或改输出协议
/// 都会让整份缓存失效（而不是回放一份旧契约下的回答）。
pub fn system_prompt(skill: &str) -> String {
    format!("{}\n{OUTPUT_PROTOCOL}", skill.trim_end())
}

// ──────────────────────────────────────────────────────────
// 命令解析（Windows 的 .cmd / .bat 要经 cmd /c）
// ──────────────────────────────────────────────────────────

/// 把 `--agent-command` 解析成 `(程序, 参数)`，并**在派发前**确认程序找得到。
///
/// # 为什么不是「直接 `Command::new(第一个词)`」
///
/// Windows 上 `std::process::Command` 只给无扩展名的程序补 `.exe`。
/// `pi-acp` 在宿主机上装成 **`pi-acp.cmd`**（npm 的 shim）——
/// `Command::new("pi-acp")` 会以「找不到文件」失败，而那条报错**指向的是
/// spawn，不是「你少写了个扩展名」**，人会以为是 ACP 坏了。
///
/// 所以这里做三件事，全部**确定性、无猜测**：
/// 1. 含路径分隔符 ⇒ 原样用（不再去 PATH 里找）；
/// 2. 否则按 `PATH` 找 `名字`、`名字.exe`、`名字.cmd`、`名字.bat`（顺序固定）；
/// 3. 命中的是 `.cmd` / `.bat` ⇒ 包成 `cmd /c <路径>`（Windows 上那是唯一跑法）。
///
/// 找不到 ⇒ **在这里就报错并点名**（⛔ 不留到 spawn 才炸，也不回落成任何别的程序）。
pub fn resolve_command(line: &str) -> Result<(String, Vec<String>)> {
    let mut words = line.split_whitespace();
    let program = words
        .next()
        .with_context(|| format!("--agent-command 是空的：{line:?}（要形如 \"pi-acp\"）"))?;
    let rest: Vec<String> = words.map(str::to_string).collect();

    let has_sep = program.contains('/') || program.contains('\\');
    let path = if has_sep {
        PathBuf::from(program)
    } else {
        match find_on_path(program) {
            Some(p) => p,
            None => {
                let tried = candidate_names(program).join(" / ");
                bail!(
                    "找不到 ACP 可执行程序 {program:?} —— 在 PATH 里找过：{tried}。\n\
                     \n\
                     ⛔ 没有回落到任何别的实现（回落 = 溯源造假，见 agent::factory 模块头）。\n\
                     要么把它装上/放进 PATH，要么用 --agent-command 给一个绝对路径。"
                );
            }
        }
    };

    if !path.exists() {
        bail!(
            "--agent-command 里的程序不存在：{}（程序名 {program:?}）",
            path.display()
        );
    }

    let path_str = path.display().to_string();
    let lower = path_str.to_ascii_lowercase();
    if cfg!(windows) && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        // `.cmd` / `.bat` 不是可执行映像，`CreateProcess` 直接拒 —— 只能过 `cmd /c`。
        let mut args = vec!["/c".to_string(), path_str];
        args.extend(rest);
        return Ok(("cmd".to_string(), args));
    }
    Ok((path_str, rest))
}

/// 无扩展名时要试的文件名（顺序固定 ⇒ 结果确定）。
fn candidate_names(program: &str) -> Vec<String> {
    if cfg!(windows) {
        // ⚠️ **Windows 上不试「不带扩展名的那个文件」**（实测踩过）：
        // npm 在 `D:\ToolCaches\npm\` 里同时放了 `pi-acp`（**无扩展名的 shell 脚本**，
        // 给 Git Bash 用）与 `pi-acp.cmd`（Windows 的入口）。`CreateProcess` 只认
        // `.exe` / `.cmd` / `.bat`，命中那个无扩展名的文件会报「不是有效的 Win32 应用」
        // —— 一条指向「ACP 坏了」而不是「挑错了文件」的错。所以直接从 `.exe` 开始试。
        vec![
            format!("{program}.exe"),
            format!("{program}.cmd"),
            format!("{program}.bat"),
        ]
    } else {
        vec![program.to_string()]
    }
}

/// 在 `PATH` 里找一个存在的文件（**不执行**它）。
fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    // 先试「原样」（不带扩展名），再试带扩展名的 —— 与 `candidate_names` 同序。
    for name in candidate_names(program) {
        for d in &dirs {
            let p = d.join(&name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

// ──────────────────────────────────────────────────────────
// 传输：ACP ⇄ `LlmTransport`
// ──────────────────────────────────────────────────────────

/// **把 ACP 的一问一答包成 `LlmTransport`** —— 于是 `doc_ingest::ingest_all`
/// 一行都不用改（缓存 / 预算 / 证据校验 / 丢弃计数全部复用）。
///
/// # 用量口径：会话累计 → 本块增量
///
/// ACP 的 `usage_update.used` 是**整个会话**的累计值（实测首轮就 15k+）。
/// 直接当本块用量 ⇒ 95 块各报同一个数，相加是 95 倍，预算闸在第一块就误停。
/// 所以这里存一份 `baseline`，报「本次累计 − 上次累计」。
///
/// 首块没有 baseline ⇒ 按 **0** 起算，于是首块把会话建立 + [`WARMUP_PROMPT`]
/// 的开销一起算进去。这是**故意**的（宁可多算，不可少算 —— 与
/// `agent::acp::UsageSample::delta_since` 同一条论证）。
///
/// # 没有 `usage_update` ⇒ 报错，不报 0
///
/// 报 0 = 把「不知道」说成「免费」，而预算闸正是靠这个数停的。
/// 报错 = 这一块作废、`failed_calls += 1`、全部失败时不出产物（R3）。
pub struct AcpChatTransport {
    inner: Box<dyn AcpTransport>,
    /// 上一次见到的**会话累计**用量（换增量用）。
    baseline: Option<UsageSample>,
    /// 预热做过了吗（每次运行最多一次，且只在真有一次调用时才做）。
    warmed: bool,
    /// 预热那一条**实际发了没有** —— 与调用方**共享**。
    ///
    /// 为什么共享而不是一个 getter：本类型最终被装进 `Box<dyn LlmTransport>`
    /// 交给 `ingest_all`，跑完之后调用方手上只剩那个 trait 对象，读不到具体类型的
    /// 字段。而屏幕上报「这一轮到底预热了没有」必须读得到 ——
    /// 否则「第二次运行 0 次调用」这句话在屏幕上就没有凭据。
    pub warmup: Arc<AtomicBool>,
}

impl AcpChatTransport {
    pub fn new(inner: Box<dyn AcpTransport>) -> Self {
        Self::with_flag(inner, Arc::new(AtomicBool::new(false)))
    }

    /// 用一个**外部可见的旗子**造（屏幕报告靠它回答「预热发了没有」）。
    pub fn with_flag(inner: Box<dyn AcpTransport>, warmup: Arc<AtomicBool>) -> Self {
        Self { inner, baseline: None, warmed: false, warmup }
    }

    /// 会话首轮预热：把对端的「## Skills / ## Extensions」前言**消耗掉**。
    ///
    /// 只在**第一次真调用之前**发生 ⇒ 全命中缓存的重跑不会发它（AC-003 的
    /// 「第二次 0 次调用」因此成立）。
    ///
    /// 预热那一轮的用量**不单独记账**：`baseline` 保持 `None`，
    /// 于是第一块会把「会话建立 + 预热」一起报进本块用量（多算，不少算）。
    fn warm_up(&mut self) -> Result<()> {
        if self.warmed {
            return Ok(());
        }
        self.warmed = true;
        self.inner.prompt(WARMUP_PROMPT).with_context(|| {
            format!(
                "ACP 会话预热失败（发的是固定 prompt {WARMUP_PROMPT:?}，正文丢掉）。\n\
                 预热存在的理由：实测 pi-acp 的**会话首轮**会先吐一段「## Skills」前言，\
                 正文 JSON 在其后 —— 不预热的话第一块必然解析失败。\n\
                 ⛔ 不跳过它硬跑：那只会把一次可修的传输问题表现成「这一块抽不出东西」。"
            )
        })?;
        self.warmup.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// 把 `ChatRequest`（system + user）**确定性**地拼成一段文本。
///
/// ACP 的 `session/prompt` 只收一段文本，没有 role 概念。拼接方式固定
/// （每条消息之间插 `---` 分隔线），于是同一份输入永远得到同一个 prompt ⇒
/// 指纹与回放才有意义。
fn flatten(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

impl LlmTransport for AcpChatTransport {
    fn complete(&mut self, req: &ChatRequest) -> Result<ChatResponse> {
        self.warm_up()?;
        let prompt = flatten(req);
        let turn = self.inner.prompt(&prompt)?;

        // ── 先记账，再交正文（钱已经花了）──
        //
        // 顺序与 `AcpAgent::extract_group` 相同：先拿到用量样本并推进基线，
        // 再让上游去解析。反过来（先解析失败就 return）会让这一笔**永远不进账**，
        // 预算闸少算，而账看起来是平的。
        let sample = turn.usage.ok_or_else(|| {
            anyhow::anyhow!(
                "ACP 回了一轮（正文 {} 字符），但**全程没有收到 `usage_update`** —— \
                 这一块花了多少客户端无从判断。\n\
                 \n\
                 为什么必须报错：报 0 是「把不知道说成免费」，而预算闸正是靠这个数停的；\
                 不报这一块则是漏算，闸门形同虚设。两种都比这一块失败更难查。",
                turn.text.chars().count()
            )
        })?;
        let delta = sample.delta_since(self.baseline.as_ref());
        self.baseline = Some(sample);
        let tokens = delta.used;

        Ok(ChatResponse {
            choices: vec![Choice {
                message: Message_ { content: Some(turn.text), reasoning_content: None },
                // ACP 的 `stopReason` 直接映射到 `finish_reason`：
                // `completion_text` 只对它等于 `"length"` 的情况特判，其余一律当正常。
                finish_reason: Some(turn.stop_reason),
            }],
            // ⚠️ ACP 只报**总量**（不分输入/输出）。把增量记在 `prompt_tokens`、
            // `completion_tokens` 记 0：真正被读的是 `total_tokens()`（账单口径），
            // 拆错了只会让屏幕上的「输入/输出」好看，而账是错的。
            usage: Some(Usage {
                prompt_tokens: tokens,
                completion_tokens: 0,
                total_tokens: Some(tokens),
                completion_tokens_details: None,
            }),
        })
    }
}

// ──────────────────────────────────────────────────────────
// 造连接
// ──────────────────────────────────────────────────────────

/// 一条**已握手**的 ACP 会话 + 如实记下来的身份。
pub struct AcpSession {
    pub transport: AcpChatTransport,
    /// 握手报的 agent 名（**不是**命令行里写的那个字符串）。
    pub agent: String,
    /// 握手报的版本（⚠️ 是**驱动二进制**的版本，不是模型版本 —— ADR-002 · N5）。
    pub version: String,
    /// `session/new` 回的配置快照（想知道「这一轮用的什么模型」就看这里）。
    pub model_note: Option<String>,
    /// 预热那一条发过没有（与 transport 共享同一面旗子）。
    pub warmup: Arc<AtomicBool>,
}

/// 拉起子进程 + 握手，**失败就报错**（⛔ 不回落成内置 LLM —— R3）。
///
/// ⚠️ 错误文本用 `{e:#}` 把**根因链**一并带出来：本命令的入口只打印最外层那句话，
/// 只给「拉不起来」等于让人无从下手（实测就是这样丢掉过
/// 「命中了一个无扩展名的 shell 脚本」这条真原因的）。
pub fn connect(cfg: AcpConfig) -> Result<AcpSession> {
    let mut transport = ThreadTransport::spawn(cfg).map_err(|e| {
        anyhow::anyhow!(
            "ACP 子进程拉不起来 —— ⛔ 没有回落到内置 LLM\
             （`--agent acp` 的产物要么是 ACP 抽的，要么没有产物）。\n\
             根因：{e:#}"
        )
    })?;
    let h = transport.handshake().map_err(|e| {
        anyhow::anyhow!(
            "ACP 握手失败（initialize / session/new）—— ⛔ 没有回落到内置 LLM。\
             检查对端是否真的在讲 Agent Client Protocol。\n\
             根因：{e:#}"
        )
    })?;

    let agent = h.agent_info.name.trim().to_string();
    if agent.is_empty() {
        bail!("ACP 握手报的 agentInfo.name 是空的 —— 抽取 agent 必须有可追溯的名字");
    }
    let version = h
        .agent_info
        .version
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .with_context(|| {
            format!(
                "ACP 握手报的 agentInfo.name = {agent:?}，但**没有 version**。\n\
                 编一个（如 \"unknown\"）就是假溯源：`stats.json` 里那一行会变成一句\
                 没人能核的话，而且和「真的换了个版本」长得一模一样。"
            )
        })?
        .to_string();
    if is_vague_version(&version) {
        bail!(
            "ACP 握手报的 agentInfo.version 是 {version:?}（模糊版本）。\
             请让对端报一个具体版本号，否则「换版本了没有」无从判断。"
        );
    }
    let model_note = h.session.config_options.as_ref().filter(|v| !v.is_null()).map(|v| {
        format!(
            "会话 {} 的 configOptions（服务端自报）：{}",
            h.session.session_id,
            serde_json::to_string(v).unwrap_or_else(|e| format!("<序列化失败：{e}>"))
        )
    });

    // ① 先建旗子，再建 transport —— 两边看到的是同一个值。
    //   （反过来就要从 transport 里再借出来，而它接下来会被装进 Box<dyn …>。）
    let warmup = Arc::new(AtomicBool::new(false));
    let transport = AcpChatTransport::with_flag(Box::new(transport), Arc::clone(&warmup));

    Ok(AcpSession { transport, agent, version, model_note, warmup })
}

/// 进**指纹**的 `model` 字段（`doc_ingest` 指纹的四项之一）。
///
/// 三样都进：命令行、握手名、握手版本。
/// - 命令行：换程序 / 换参数（例如换模型参数）⇒ 缓存整体失效；
/// - 握手名 + 版本：同一个命令行背后换了实现（npm 升级）⇒ 也必须失效。
///
/// ⚠️ 版本是**驱动二进制的版本**（ADR-002 · N5），所以它挡得住「pi 升级了」，
/// 挡不住「pi 背后换了模型」。后者由 `--cache` 与 `stats.model_note` 交代 ——
/// 本 REQ 不做模型锁定（那需要在 `session/new` 里发一个没实测过的字段）。
pub fn model_id(program: &str, args: &[String], agent: &str, version: &str) -> String {
    let line = if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    };
    format!("acp:{line}@{agent}#{version}")
}

// ──────────────────────────────────────────────────────────
// 出产物之前的闸（R3）
// ──────────────────────────────────────────────────────────

/// **ACP 通道的出产物闸**：三种「跑了但没结果」的情况一律拒绝出产物。
///
/// 内置路径没有这道闸（它的失败面已经被 REQ-116 的预算闸覆盖），
/// 而 ACP 多了一整类**传输级**的失败（连不上 / 超时 / 对端不守协议），
/// 那些失败不体现在预算上 —— 不设这道闸，会得到一份 0 字节的
/// `assertions.jsonl` 加一次 exit 0，与「这份文档真的没有概念」一模一样。
///
/// | 判据 | 为什么它是失败而不是空结果 |
/// | --- | --- |
/// | `failed_calls == calls` 且 `calls > 0` | 每一次调用都失败了 ⇒ 这一轮**什么都没抽到** |
/// | `assertions == 0` | 一轮跑完一条都没留下 ⇒ 产物是空的，而下游会当全量吃掉 |
///
/// ⚠️ 预算停在半路**不算**这一类：那条已经在 `cli` 里单独拒绝了（本函数不重复判）。
pub fn require_usable_output(stats: &DocStats, chunks: usize) -> Result<()> {
    if chunks == 0 {
        return Ok(());
    }
    if stats.calls > 0 && stats.failed_calls >= stats.calls {
        bail!(
            "ACP 通道**每一次调用都失败了**（{} / {} 块）—— 本轮**不产出任何断言文件**。\n\
             \n\
             为什么什么都不写：一份 0 条的 JSONL 与「这份文档真的没有概念」\
             在产物里长得一模一样，下游会把它当全量吃掉。\n\
             \n\
             丢弃明细：证据对不上 {} · 模态 {} · 谓词 {} · 字段空 {} · 解析 {}。\n\
             前 3 条原因：{}",
            stats.failed_calls,
            stats.calls,
            stats.rejected_no_evidence,
            stats.rejected_modality,
            stats.rejected_predicate,
            stats.rejected_empty,
            stats.rejected_parse,
            if stats.rejection_samples.is_empty() {
                "（无）".to_string()
            } else {
                stats.rejection_samples.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")
            }
        );
    }
    if stats.assertions == 0 {
        bail!(
            "ACP 通道跑了 {} 块，但**一条断言都没产出** —— 本轮**不产出任何断言文件**。\n\
             \n\
             为什么这算失败：`assertions.jsonl` 的契约里没有「这一轮什么都没抽到」这一栏，\
             一份空文件与「文档真的没有概念」长得一样，而下游会把它当全量吃掉。\n\
             \n\
             丢弃明细：证据对不上 {} · 模态 {} · 谓词 {} · 字段空 {} · 解析 {}。\n\
             前 3 条原因：{}",
            stats.chunks,
            stats.rejected_no_evidence,
            stats.rejected_modality,
            stats.rejected_predicate,
            stats.rejected_empty,
            stats.rejected_parse,
            if stats.rejection_samples.is_empty() {
                "（无）".to_string()
            } else {
                stats.rejection_samples.iter().take(3).cloned().collect::<Vec<_>>().join(" | ")
            }
        );
    }
    Ok(())
}

/// **quote 校验的分子分母**（AC-006 的「quote 校验通过率」）。
///
/// 分子 = 进了 `assertions.jsonl` 的条数（**每一条都过了** `locate_quote`）；
/// 分母 = 分子 + `discarded_no_quote`。
///
/// ⚠️ 这个口径**只数「过了 quote 闸」与「没过 quote 闸」两类**，
/// 不含因为模态 / 谓词 / 空字段被丢的条目 —— 那些压根没走到 quote 校验。
/// 所以它是「quote 这一关的通过率」，不是「抽取的总成功率」。
pub fn quote_pass(stats: &DocStats) -> (usize, usize) {
    let passed = stats.assertions;
    let dropped = stats.rejected_no_evidence;
    (passed, passed + dropped)
}

/// 「quote 校验通过率」的一句话（`discarded_no_quote` 在这里出现 —— 它是
/// `rejected_no_evidence` 在本通道的名字，⛔ 不是第二个计数器）。
pub fn quote_pass_line(stats: &DocStats) -> String {
    let (passed, total) = quote_pass(stats);
    let rate = if total == 0 {
        "（分母 0：没有条目走到 quote 校验）".to_string()
    } else {
        format!("{:.1}%", passed as f64 / total as f64 * 100.0)
    };
    format!(
        "通过 {passed} / {total} = {rate}（丢弃 discarded_no_quote = {}）",
        stats.rejected_no_evidence
    )
}

/// ACP 会话的**默认单轮超时**（秒）。
pub const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 180;

/// `--agent-timeout-secs` → [`Duration`]（0 报错：0 秒的意思是一次都不许跑完）。
pub fn resolve_timeout(secs: Option<u64>) -> Result<Duration> {
    match secs {
        Some(0) => bail!(
            "--agent-timeout-secs 是 0：0 秒的意思是一次往返都不许跑完，\
             那与「不跑」是同一件事。要跑就给一个真实上限（例如 {DEFAULT_AGENT_TIMEOUT_SECS}）。"
        ),
        Some(s) => Ok(Duration::from_secs(s)),
        None => Ok(Duration::from_secs(DEFAULT_AGENT_TIMEOUT_SECS)),
    }
}

/// 当前工作目录（`session/new` 的 `cwd` 必须是一个**真实存在的目录**）。
pub fn session_cwd() -> Result<String> {
    let p = std::env::current_dir().context("拿不到当前工作目录 —— session/new 需要一个真实目录")?;
    Ok(p.display().to_string())
}

// ──────────────────────────────────────────────────────────
// 单测（零网络、零磁盘、不起进程）
// ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::acp::protocol::{AgentInfo, NewSessionResult};
    use crate::agent::acp::{Handshake, ScriptedTransport, TurnOutcome};
    use crate::agent::llm::protocol::Message;
    use crate::extract::doc_ingest::{
        self, ingest_all, MemoryDocCache, Prompts, SourceDoc,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn handshake() -> Handshake {
        Handshake {
            protocol_version: 1,
            agent_info: AgentInfo {
                name: "fake-acp".into(),
                title: None,
                version: Some("9.9.9".into()),
            },
            agent_capabilities: serde_json::json!({}),
            session: NewSessionResult {
                session_id: "s-1".into(),
                config_options: None,
                modes: None,
            },
        }
    }

    fn turn(text: &str, used: u64) -> TurnOutcome {
        TurnOutcome {
            text: text.to_string(),
            chunks: 1,
            usage: Some(UsageSample {
                used,
                size: Some(200_000),
                cost: None,
            }),
            stop_reason: "end_turn".into(),
        }
    }

    fn req() -> ChatRequest {
        ChatRequest {
            model: "whatever".into(),
            messages: vec![Message::system("契约"), Message::user("{\"a\":1}")],
            max_tokens: 0,
        }
    }

    /// skill 与输出协议**都要**在 prompt 里：少一个都会让对端按别的形状回答。
    #[test]
    fn system_prompt_carries_skill_and_protocol() {
        let p = system_prompt("# 我的契约\n\n只抽名词。\n");
        assert!(p.contains("我的契约"), "skill 原文必须在");
        assert!(p.contains("\"concepts\""), "输出协议必须在");
        assert!(p.contains("modality"), "modality 的枚举必须在（validate 会拒掉缺失的）");
        // 剪掉尾部空白后拼装 ⇒ 同一份 skill 永远得到同一个 prompt_sha
        assert_eq!(p, system_prompt("# 我的契约\n\n只抽名词。\n"));
        assert_ne!(p, system_prompt("# 我的契约\n\n只抽动词。\n"));
    }

    /// 指纹的 `model` 项必须把三件事都吃进去。
    #[test]
    fn model_id_covers_command_and_handshake_identity() {
        let base = model_id("pi-acp", &[], "pi-acp", "0.0.34");
        assert_ne!(base, model_id("other-acp", &[], "pi-acp", "0.0.34"), "换程序必须换指纹");
        assert_ne!(base, model_id("pi-acp", &["--x".into()], "pi-acp", "0.0.34"), "换参数必须换指纹");
        assert_ne!(base, model_id("pi-acp", &[], "pi-acp", "0.0.35"), "换版本必须换指纹");
        assert_ne!(base, model_id("pi-acp", &[], "mcode", "0.0.34"), "换实现名必须换指纹");
        assert_eq!(base, model_id("pi-acp", &[], "pi-acp", "0.0.34"));
    }

    /// 空命令 ⇒ 报错（不回落成任何默认程序）。
    #[test]
    fn empty_command_is_rejected() {
        assert!(resolve_command("   ").is_err());
    }

    /// 不存在的程序 ⇒ 报错**并点名**（这是 AC-004 的第一种构造）。
    #[test]
    fn missing_program_is_named() {
        let err = resolve_command("definitely-not-a-real-acp-program-req147")
            .unwrap_err()
            .to_string();
        assert!(err.contains("definitely-not-a-real-acp-program-req147"), "{err}");
        assert!(err.contains("没有回落"), "必须明说不回落：{err}");
    }

    /// 会话累计 → 本块增量：首块按 0 起算，之后是差。
    /// （直接报累计会把 95 块算成 95 倍，预算闸在第一块就误停。）
    #[test]
    fn cumulative_usage_is_converted_to_per_chunk_delta() {
        let inner = ScriptedTransport::new(handshake(), vec![
            Ok(turn("就绪", 1_000)),      // 预热（丢正文）
            Ok(turn("{}", 15_000)),       // 第一块：累计 15k（含会话建立 + 预热）
            Ok(turn("{}", 18_000)),       // 第二块：累计 18k
        ]);
        let mut t = AcpChatTransport::new(Box::new(inner));
        let a = t.complete(&req()).unwrap();
        assert_eq!(a.usage.unwrap().total_tokens(), 15_000, "首块多算（含预热与会话建立）");
        let b = t.complete(&req()).unwrap();
        assert_eq!(b.usage.unwrap().total_tokens(), 3_000, "第二块必须只报增量");
    }

    /// ★ 预热只在**第一次真调用之前**发一次；且正文不进任何产物。
    #[test]
    fn warmup_happens_once_and_its_text_is_discarded() {
        let inner = ScriptedTransport::new(handshake(), vec![
            Ok(turn("## Skills\n- a\n- b\n", 10)),
            Ok(turn("{\"concepts\":[],\"relations\":[]}", 20)),
            Ok(turn("{\"concepts\":[],\"relations\":[]}", 30)),
        ]);
        let mut t = AcpChatTransport::new(Box::new(inner));
        let r1 = t.complete(&req()).unwrap();
        // 预热那一轮的「## Skills」前言**绝不能**变成这一块的正文
        assert_eq!(r1.choices[0].message.content.as_deref(), Some("{\"concepts\":[],\"relations\":[]}"));
        let _ = t.complete(&req()).unwrap();
        assert!(t.warmup.load(Ordering::SeqCst), "预热必须发过一次");
    }

    /// 对端不报 `usage_update` ⇒ 这一块**作废**（不按 0 记账）。
    #[test]
    fn missing_usage_is_an_error_not_zero() {
        let mut no_usage = turn("{}", 0);
        no_usage.usage = None;
        let inner = ScriptedTransport::new(handshake(), vec![Ok(turn("就绪", 5)), Ok(no_usage)]);
        let mut t = AcpChatTransport::new(Box::new(inner));
        let err = t.complete(&req()).unwrap_err().to_string();
        assert!(err.contains("usage_update"), "{err}");
    }

    // ── 端到端（仍是零进程：ScriptedTransport 喂「ACP 的回答」）──

    fn doc(text: &str) -> SourceDoc {
        SourceDoc::from_text("docs/a.md", text)
    }

    fn prompts(skill: &str) -> Prompts {
        Prompts::from_system(system_prompt(skill))
    }

    /// 造一个传输：脚本 = **[预热那轮的「就绪」] + 各块的真实回答**。
    ///
    /// `complete` 第一次被调用会自动消耗第一条（预热），所以用例里
    /// 只需要关心「块怎么答」。
    fn acp_with(answers: Vec<Result<TurnOutcome>>) -> AcpChatTransport {
        let mut all = vec![Ok(turn("就绪", 1_000))];
        all.extend(answers);
        AcpChatTransport::new(Box::new(ScriptedTransport::new(handshake(), all)))
    }

    /// ★ AC-005（ACP 侧）：**编造 quote ⇒ 丢弃 + 计入 `discarded_no_quote`**。
    ///
    /// 判据与内置路径**是同一个**（`locate_quote`）：本通道没有第二份 quote 检查。
    #[test]
    fn ac005_forged_quote_is_dropped_and_counted() {
        let d = doc("# 订单\n\n客户在平台下单，平台随后扣费。\n");
        let chunks = doc_ingest::chunk_document(&d);
        let fake = r#"{"concepts":[{"name":"订单","quote":"客户在平台退款","modality":"fact"}],
                       "relations":[{"subject":"客户","predicate":"下单","object":"订单",
                                     "quote":"客户在平台退款","modality":"fact"}]}"#;
        let mut t = acp_with(vec![Ok(turn(fake, 2_000))]);
        let mut cache = MemoryDocCache::new();
        let (assertions, stats) = ingest_all(
            &mut t, &mut cache, &chunks, "m", &prompts("# 契约\n"), 1_000_000, 0,
        );
        assert!(assertions.is_empty(), "编造的 quote 一条都不许留下：{assertions:?}");
        assert_eq!(stats.rejected_no_evidence, 2, "概念 + 关系各丢一条");
        assert_eq!(stats.failed_calls, 0, "「证据对不上」不是传输失败，别混计");
        let (passed, total) = quote_pass(&stats);
        assert_eq!((passed, total), (0, 2), "quote 校验的分子分母");
        assert!(quote_pass_line(&stats).contains("discarded_no_quote = 2"));
    }

    /// ★ AC-005 正常侧：逐字 quote ⇒ 保留，`span` 必须能在原文里切出同一段字。
    #[test]
    fn ac005_verbatim_quote_survives_with_exact_span() {
        let d = doc("# 订单\n\n客户在平台下单，平台随后扣费。\n");
        let chunks = doc_ingest::chunk_document(&d);
        let fake = r#"{"concepts":[{"name":"订单","quote":"客户在平台下单","modality":"fact"}],
                       "relations":[{"subject":"客户","predicate":"下单","object":"订单",
                                     "quote":"平台随后扣费","modality":"fact"}]}"#;
        let mut t = acp_with(vec![Ok(turn(fake, 2_000))]);
        let mut cache = MemoryDocCache::new();
        let (assertions, stats) = ingest_all(
            &mut t, &mut cache, &chunks, "m", &prompts("# 契约\n"), 1_000_000, 0,
        );
        assert_eq!(assertions.len(), 2, "{assertions:?}");
        assert_eq!(stats.rejected_no_evidence, 0);
        for a in &assertions {
            let e = a.evidence();
            let (s, t2) = (e.span[0], e.span[1]);
            assert_eq!(&d.text[s..t2], e.quote, "span 必须精确指向原文");
        }
        assert!(quote_pass_line(&stats).contains("100.0%"));
    }

    /// ★ AC-003（ACP 侧）：同输入两次 ⇒ **逐字节相同** + 第二次 **0 次调用**。
    #[test]
    fn ac003_second_run_hits_cache_zero_calls_byte_identical() {
        let d = doc("# 订单\n\n客户在平台下单，平台随后扣费。\n");
        let chunks = doc_ingest::chunk_document(&d);
        let fake = r#"{"concepts":[{"name":"订单","quote":"客户在平台下单","modality":"fact"}],
                       "relations":[]}"#;
        let mut cache = MemoryDocCache::new();
        let p = prompts("# 契约\n");
        let mut t1 = acp_with(vec![Ok(turn(fake, 2_000))]);
        let (a1, _) = ingest_all(&mut t1, &mut cache, &chunks, "m", &p, 1_000_000, 0);

        // 第二次：一个「问什么都炸」的传输 —— 真调用就会被抓住
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        let inner = ScriptedTransport::always_answer(handshake(), move |_| {
            h.fetch_add(1, Ordering::SeqCst);
            bail!("命中缓存时绝不该调用 ACP")
        });
        let mut t2 = AcpChatTransport::new(Box::new(inner));
        let (a2, s2) = ingest_all(&mut t2, &mut cache, &chunks, "m", &p, 1_000_000, 0);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "命中缓存必须 0 次调用（含预热）");
        assert_eq!(s2.calls, 0);
        assert_eq!(s2.cache_hits, chunks.len());
        assert_eq!(
            doc_ingest::render_jsonl(&a1).unwrap(),
            doc_ingest::render_jsonl(&a2).unwrap(),
            "JSONL 必须逐字节相同"
        );
    }

    /// R3 的闸：全部调用失败 / 一条都没抽到 ⇒ 拒绝出产物。
    #[test]
    fn gates_refuse_to_produce_output() {
        let d = doc("# 订单\n\n客户下单。\n");
        let chunks = doc_ingest::chunk_document(&d);
        let n = chunks.len();

        // ① 每一次调用都失败
        let mut always_fail = AcpChatTransport::new(Box::new(ScriptedTransport::always_answer(
            handshake(),
            |_| bail!("对端超时"),
        )));
        let mut cache = MemoryDocCache::new();
        let (a, s) = ingest_all(
            &mut always_fail,
            &mut cache,
            &chunks,
            "m",
            &prompts("# 契约\n"),
            1_000_000,
            0,
        );
        assert!(a.is_empty());
        assert!(s.calls > 0 && s.failed_calls == s.calls);
        let err = require_usable_output(&s, n).unwrap_err().to_string();
        assert!(err.contains("每一次调用都失败了"), "{err}");
        assert!(err.contains("不产出任何断言文件"), "{err}");

        // ② 全部答「空抽取」⇒ 0 条断言（不是「文档没有概念」，是这一轮白跑）
        let mut empty = AcpChatTransport::new(Box::new(ScriptedTransport::always_answer(
            handshake(),
            |_| Ok(turn("{\"concepts\":[],\"relations\":[]}", 1_000)),
        )));
        let mut cache2 = MemoryDocCache::new();
        let (a2, s2) = ingest_all(
            &mut empty,
            &mut cache2,
            &chunks,
            "m",
            &prompts("# 契约\n"),
            1_000_000,
            0,
        );
        assert!(a2.is_empty());
        assert_eq!(s2.failed_calls, 0, "解析成功但没抽到东西 —— 不是传输失败");
        let err2 = require_usable_output(&s2, n).unwrap_err().to_string();
        assert!(err2.contains("一条断言都没产出"), "{err2}");

        // ③ 有产物时闸必须放行（防止判据恒红）
        let mut good = AcpChatTransport::new(Box::new(ScriptedTransport::always_answer(
            handshake(),
            |_| {
                Ok(turn(
                    "{\"concepts\":[{\"name\":\"订单\",\"quote\":\"客户下单\",\"modality\":\"fact\"}],\"relations\":[]}",
                    1_000,
                ))
            },
        )));
        let mut cache3 = MemoryDocCache::new();
        let (a3, s3) = ingest_all(
            &mut good,
            &mut cache3,
            &chunks,
            "m",
            &prompts("# 契约\n"),
            1_000_000,
            0,
        );
        assert!(!a3.is_empty(), "前置条件：这一轮真的有产物");
        assert!(require_usable_output(&s3, n).is_ok(), "有产物时必须放行");
    }

    /// `--agent-timeout-secs` 的取值口径（0 报错、缺省 180 秒）。
    #[test]
    fn timeout_flag_is_interpreted() {
        assert_eq!(resolve_timeout(None).unwrap(), Duration::from_secs(180));
        assert_eq!(resolve_timeout(Some(3)).unwrap(), Duration::from_secs(3));
        assert!(resolve_timeout(Some(0)).is_err());
    }
}



