//! ⭐ REQ-176 · **部署级业务词白名单**（`--term-allowlist <file>`）。
//!
//! # 它解决的是「闸门分不清业务词与建模术语」
//!
//! 发布闸的判据只有一份：`domain::view::scan_modeling_terms`。它按**词**判，
//! 分不清「建模术语泄漏」与「客户业务词恰好同名」—— 审计/取证域的实体叫
//! `evidence` 是合理的，但闸会拒绝整份客户视图（项目因此 500 无法加载）。
//!
//! 本模块**不**碰判据：`scan_modeling_terms` 的返回值一字不变（它还有别的消费者）。
//! 它在**闸门层**做减法：`hits - allowlist`，剩下的为空才放行。
//!
//! # 为什么配置是**进程级**的
//!
//! 白名单由部署方（运维）给，一个进程一个文件 —— 与 `--tokens` 同级。
//! 闸门（`domain::pipeline::gate_text` / `mcp::tools::guard`）是**自由函数**，
//! 没有「命令上下文」可传；把它做成进程级配置既符合部署语义，也不用改整条管线的签名。
//!
//! # 硬要求：放宽必须**看得见**
//!
//! - 启动时打印一行：文件、加载了几个词、分别是哪些；
//! - **每次**豁免打印一行：出口 + 被豁免的词（⛔ 不许静默放行）；
//! - 文件不存在 / 读不出 / 某行不合法（含空白、控制字符、超长）⇒ **启动失败**
//!   （fail-closed，消息点名文件与行号）；⛔ 不许当作空表继续跑；
//! - 白名单里塞了词表外的词 ⇒ 启动**警告**「这行没意义」，但**不失败**（防手滑）。
//!
//! 日志只打**词条**，⛔ 不打客户正文 —— 与发布闸的既有纪律一致。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 单个词条的长度上限（字符数）。
///
/// 词表里的词最长是 `primary_key` / `extracted_by`（11 个字符），
/// 中文业务词更短。64 足够宽（不会误伤正常业务词），又能挡住
/// 「一行贴进来一整段 JSON / 一整份文档」——那是配置事故，不是词。
pub const MAX_TERM_CHARS: usize = 64;

/// 已加载的白名单（不可变值）。**判据不在这里**，这里只有「哪些词被部署方放行了」。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TermAllowlist {
    /// 文件顺序（去重后），只用于启动回执。
    entries: Vec<String>,
    /// 匹配用的规范化形式（ASCII 小写）。`scan_modeling_terms` 返回的是词表原文
    /// （英文全小写），所以这里小写化后按等值匹配。
    canonical: BTreeSet<String>,
}

/// 一次加载的产物：白名单 + 需要打印的警告（词表外的词）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    pub allowlist: TermAllowlist,
    /// 每条都是**人话**（含文件与行号），由调用方打到 stderr。
    pub warnings: Vec<String>,
}

impl TermAllowlist {
    /// 解析一份白名单文本。`source` 只用于报错点名（不读文件）。
    ///
    /// 格式：每行一个词；`#` 开头为注释；空行忽略；两侧 trim。
    /// 不合法（词条中间含空白 / 控制字符，或超过 [`MAX_TERM_CHARS`]）⇒ `Err`，
    /// 消息**点名文件与行号**（AC-004）。词表外的词 ⇒ 只进 `warnings`（AC-006）。
    pub fn parse(text: &str, source: &Path) -> Result<Loaded, String> {
        let mut entries: Vec<String> = Vec::new();
        let mut canonical: BTreeSet<String> = BTreeSet::new();
        let mut warnings: Vec<String> = Vec::new();

        for (idx, raw) in text.lines().enumerate() {
            let line_no = idx + 1;
            let term = raw.trim();
            if term.is_empty() || term.starts_with('#') {
                continue;
            }
            let chars = term.chars().count();
            if chars > MAX_TERM_CHARS {
                return Err(format!(
                    "--term-allowlist 文件 {} 第 {} 行不合法：词条超长（{} 个字符 > 上限 {}）\
                     —— 一行只能一个词，⛔ 不要把整段正文贴进来",
                    source.display(),
                    line_no,
                    chars,
                    MAX_TERM_CHARS
                ));
            }
            if term.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!(
                    "--term-allowlist 文件 {} 第 {} 行不合法：词条中间含空白 / 控制字符\
                     （一行只能一个词，注释只能整行以 # 开头）：{:?}",
                    source.display(),
                    line_no,
                    term
                ));
            }

            let canon = term.to_lowercase();
            if !canonical.insert(canon) {
                // 同一个词写两遍不是错误：去重即可（它不改变语义）。
                continue;
            }
            // ⭐ AC-006：词表外的词对闸门毫无作用 —— 多半是手滑。**警告不失败**，
            // 因为白名单是部署配置，不该因为「多写了一个词」把服务挡在门外。
            if !is_known_term(term) {
                warnings.push(format!(
                    "⚠ --term-allowlist 文件 {} 第 {} 行 {:?} 不在建模术语词表里 —— \
                     这行没意义（闸门永远不会命中它），仍会加载但不影响任何判定",
                    source.display(),
                    line_no,
                    term
                ));
            }
            entries.push(term.to_string());
        }

        Ok(Loaded {
            allowlist: TermAllowlist { entries, canonical },
            warnings,
        })
    }

    /// 读文件 + 解析。读不出 ⇒ `Err`（消息点名路径），⛔ 不当作空表。
    pub fn load(path: &Path) -> Result<Loaded, String> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "读不到 --term-allowlist 文件 {}：{e} —— ⛔ 不当作「没有白名单」继续跑\
                 （那会让本意是放宽的部署以为自己在放宽，实际仍在拒绝）",
                path.display()
            )
        })?;
        Self::parse(&text, path)
    }

    /// 加载了几个词。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按文件顺序的词条（启动回执用）。
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// 这个命中词是否被部署方放行。英文按小写等值（判据返回的是词表原文）。
    pub fn allows(&self, hit: &str) -> bool {
        self.canonical.contains(&hit.to_lowercase())
    }

    /// `hits - allowlist`。返回 `(剩下的, 被豁免的)` —— 排序保持输入顺序。
    pub fn retain_unexempt(&self, hits: Vec<String>) -> (Vec<String>, Vec<String>) {
        let mut kept = Vec::with_capacity(hits.len());
        let mut exempted = Vec::new();
        for h in hits {
            if self.allows(&h) {
                exempted.push(h);
            } else {
                kept.push(h);
            }
        }
        (kept, exempted)
    }

    /// 启动回执（AC §二.3 的「加载了几个词、分别是哪些」）。
    pub fn startup_line(&self, path: &Path) -> String {
        let words = if self.entries.is_empty() {
            "无".to_string()
        } else {
            self.entries
                .iter()
                .map(|w| w.as_str())
                .collect::<Vec<_>>()
                .join("、")
        };
        format!(
            "· --term-allowlist {}：加载 {} 个词（{}）—— 只在闸门层做减法，\
             判据 scan_modeling_terms 不变",
            path.display(),
            self.entries.len(),
            words
        )
    }
}

/// 词表里有没有这个词（用于 AC-006 的「没意义」警告）。
///
/// 只读 `MODELING_TERMS`（英文 + 中文都在里面）；⛔ 不复制词表、不改词表。
fn is_known_term(term: &str) -> bool {
    crate::domain::view::MODELING_TERMS
        .iter()
        .any(|t| t.eq_ignore_ascii_case(term))
}

// ══════════════════════════════════════════════════════════════
// 进程级安装（部署级配置；与 `--tokens` 同级）
// ══════════════════════════════════════════════════════════════

/// 进程内**只装一次**。`None` = 没给 `--term-allowlist` ⇒ 闸门行为与今天逐字节一致。
static ACTIVE: OnceLock<TermAllowlist> = OnceLock::new();

/// 已安装的白名单文件名（只用于诊断；⛔ 不参与判定）。
static ACTIVE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 读文件 → 打印启动回执与警告 → 安装成进程级配置。**失败即启动失败**。
pub fn install_from_path(path: &Path) -> anyhow::Result<()> {
    let loaded = TermAllowlist::load(path).map_err(|e| anyhow::anyhow!("{e}"))?;
    for w in &loaded.warnings {
        eprintln!("{w}");
    }
    eprintln!("{}", loaded.allowlist.startup_line(path));
    ACTIVE
        .set(loaded.allowlist)
        .map_err(|_| anyhow::anyhow!("--term-allowlist 是进程级配置，只允许安装一次"))?;
    let _ = ACTIVE_PATH.set(path.to_path_buf());
    Ok(())
}

/// 有没有装白名单（诊断用）。
pub fn is_active() -> bool {
    ACTIVE.get().is_some()
}

/// 已安装的文件（诊断用）。
pub fn active_path() -> Option<&'static Path> {
    ACTIVE_PATH.get().map(|p| p.as_path())
}

/// **闸门层的唯一过滤点**：`hits - allowlist`，并把豁免**逐次**留痕。
///
/// - 没装白名单 ⇒ 原样返回（⛔ 一个字节的默认行为都不变）；
/// - 装了 ⇒ 每一条被豁免的词都会出现在一行 stderr 里，**点名出口**；
/// - 返回空 = 放行，非空 = 按既有逻辑拒绝（错误消息只会列**剩下的**词）。
pub fn filter_hits(outlet: &'static str, hits: Vec<String>) -> Vec<String> {
    let Some(allowlist) = ACTIVE.get() else {
        return hits;
    };
    let (kept, exempted) = allowlist.retain_unexempt(hits);
    if !exempted.is_empty() {
        eprintln!(
            "· 发布闸豁免（--term-allowlist）：出口 {} 豁免词条 {:?} —— \
             这是部署方**显式**放宽的业务词，不是闸门失效",
            outlet, exempted
        );
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        PathBuf::from("terms.txt")
    }

    /// 正常文件：注释 / 空行 / 两侧空白都被吃掉，顺序保留。
    #[test]
    fn comments_blanks_and_surrounding_whitespace_are_ignored() {
        let text = "\n# 审计域业务词\n  evidence  \n\n证据\n#tail\n";
        let loaded = TermAllowlist::parse(text, &path()).expect("合法文件不该失败");
        assert_eq!(loaded.allowlist.entries(), ["evidence", "证据"]);
        assert_eq!(loaded.allowlist.len(), 2);
        assert!(loaded.warnings.is_empty(), "两个词都在词表里：{:?}", loaded.warnings);
    }

    /// AC-006：词表外的词只警告，不失败；消息必须说「这行没意义」。
    #[test]
    fn unknown_terms_warn_but_do_not_fail() {
        let loaded = TermAllowlist::parse("evidence\ninvoice_no\n", &path())
            .expect("词表外的词不许让启动失败");
        assert_eq!(loaded.allowlist.len(), 2, "词表外的词也要加载（警告而已）");
        assert_eq!(loaded.warnings.len(), 1);
        let w = &loaded.warnings[0];
        assert!(w.contains("第 2 行"), "警告要点名行号：{w}");
        assert!(w.contains("invoice_no"), "警告要点名那个词：{w}");
        assert!(w.contains("这行没意义"), "AC-006 的原文：{w}");
    }

    /// AC-004：词条中间有空白 ⇒ `Err`，点名文件与行号。
    #[test]
    fn a_line_with_inner_whitespace_is_a_fatal_error_with_file_and_line() {
        let e = TermAllowlist::parse("evidence\nevi dence\n", &path())
            .expect_err("含空白的行必须让启动失败");
        assert!(e.contains("terms.txt"), "要点名文件：{e}");
        assert!(e.contains("第 2 行"), "要点名行号：{e}");
        assert!(e.contains("空白"), "要说清为什么不合法：{e}");
    }

    /// AC-004：超长行 ⇒ `Err`，点名行号（防「整段正文贴进来」）。
    #[test]
    fn an_overlong_line_is_a_fatal_error_with_the_line_number() {
        let long = "e".repeat(MAX_TERM_CHARS + 1);
        let e = TermAllowlist::parse(&format!("evidence\n{long}\n"), &path())
            .expect_err("超长行必须让启动失败");
        assert!(e.contains("第 2 行"), "要点名行号：{e}");
        assert!(e.contains("超长"), "要说清原因：{e}");
    }

    /// 文件不存在 ⇒ `Err`（点名路径），⛔ 不许静默当空表。
    #[test]
    fn a_missing_file_is_an_error_not_an_empty_allowlist() {
        let missing = std::env::temp_dir().join(format!(
            "ontok-req176-missing-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let e = TermAllowlist::load(&missing).expect_err("不存在的文件必须失败");
        assert!(e.contains(&missing.display().to_string()), "要点名路径：{e}");
    }

    /// 减法语义：`hits - allowlist`，剩下的保持输入顺序；英文大小写不敏感。
    #[test]
    fn filtering_removes_only_allowlisted_hits() {
        let loaded = TermAllowlist::parse("Evidence\n", &path()).unwrap();
        let (kept, exempted) = loaded
            .allowlist
            .retain_unexempt(vec!["主键".into(), "evidence".into(), "key".into()]);
        assert_eq!(kept, ["主键", "key"]);
        assert_eq!(exempted, ["evidence"]);
        assert!(loaded.allowlist.allows("EVIDENCE"), "英文比对应当忽略大小写");
        assert!(!loaded.allowlist.allows("证据"), "没写的词不许被放行");
    }

    /// 空文件 = 加载 0 个词，启动行必须明说「0 个」而不是看起来像没给。
    #[test]
    fn an_empty_file_loads_zero_terms_and_says_so() {
        let loaded = TermAllowlist::parse("# 全是注释\n\n", &path()).unwrap();
        assert!(loaded.allowlist.is_empty());
        let line = loaded.allowlist.startup_line(&path());
        assert!(line.contains("加载 0 个词"), "{line}");
    }
}
