//! REQ-107 · 第 2 轮 · **源码 → 登记表**方向的枚举判据（A11 R-A / R-B / R-C / R-D + B5）。
//!
//! # 这份文件治的是什么
//!
//! 第 1 轮的判据只做了**一个方向**：拿 `OUTLETS` 表逐条去源码里找 needle。
//! 于是「**换个名字/换个文件/新加一条**」全都绿 —— QA 用 12 次变异实测出 5 种绕过，
//! 其中「新增 `/api/raw` 路由、零闸、不入表」正是 AC-003 逐字点名的场景。
//!
//! 第 2 轮把方向反过来：**先枚举源码，再逐个对账登记表**。
//!
//! | 判据 | 枚举什么 | 落在哪里 |
//! | --- | --- | --- |
//! | T1 | `src/` **全目录**（递归，新文件自动纳入）里的客户可见写点 | `OUTLETS.call_site` 或显式「非客户」白名单 |
//! | T2 | 每个 `emit_domain_text/line(` 的实参 | 必须是**由管线入口直接赋值**的标识符，且赋值在写出之前 |
//! | T3 | 出口文件里每一处 `OutletId::` 记号 | `OUTLET_ID_TOKENS`（文件 × 记号 × 次数） |
//! | T4 | 全仓所有返回客户产物的 `pub fn` | 恰好 `render` / `publish_value` 两个 |
//! | T5 | 三个出口文件里的每一个 `fn` 定义 | `OUTLET_FNS`（逐个登记 + 用途） |
//! | T6 | 全仓所有 `mod x;` 声明 | `MODULES`（新增模块即红） |
//!
//! ⛔ **不许写死文件清单**：T1/T4/T6 都从 `src/` 目录**现场遍历**，
//! 新加的 `.rs` 自动进扫描范围（大多数情况下它没有登记 ⇒ 红）。
//! ⛔ **枚举前必须剥注释**：`tests/common::strip_comments`（词法级，保留字符串字面量），
//! 否则 `src/server/http.rs` 文档注释里的 `Json(` 示例会把「摘掉真实写点」判成绿。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。
//!
//! # 已知边界（⛔ 不许改写成「已封堵」）
//!
//! - 枚举认的是**构造拼写**（`println!` / `emit_domain_*` / `Json(` / `Html(` /
//!   `encode_result(` / `fs::write(`）。用别的机制绕过（自建 `io::stdout()`、
//!   第三方日志宏）**抓不到**；补强在类型层（写出函数只收 `OutletRender`，
//!   裸 `&str` 编译不过）。
//! - T2 只看「管线赋值 ↔ 写出」之间的窗口；「在管线调用**之前**用一个本地判据
//!   决定要不要调管线」这种形态由 T5（fn 登记）与 T1（新写点）侧证，
//!   **闭包形式的本地判据**不在 T5 覆盖内（诚实登记，见交付物 §7）。

mod common;

use common::{read_source, strip_comments};
use std::collections::{BTreeMap, BTreeSet};

// ══════════════════════════════════════════════════════════════
// 公共设施
// ══════════════════════════════════════════════════════════════

/// 三个**出口文件**（客户可见字节从这三个文件出去）。
const OUTLET_FILES: &[&str] = &["src/cli.rs", "src/server/http.rs", "src/mcp/serve.rs"];

/// 管线模块 —— **机制本体**，不参与写点枚举（它是登记表与写出函数的定义处）。
/// 另有 T7 断言它一行 stdout 宏都没有。
const PIPELINE_SRC: &str = "src/domain/pipeline.rs";

fn walk_rs(dir: &std::path::Path, acc: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_rs(&p, acc);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            acc.push(p);
        }
    }
}

/// `src/` 下**全部** `.rs`（相对路径，`/` 分隔，按字典序）。
///
/// ⛔ 不写死文件清单：新文件自动进来，然后因为「没有登记」而变红。
fn src_files() -> Vec<String> {
    let root = common::crate_root();
    let mut files = Vec::new();
    walk_rs(&root.join("src"), &mut files);
    files.sort();
    files
        .iter()
        .map(|p| {
            p.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

/// 生产代码（剥注释 + 砍掉 `#[cfg(test)]` 之后的部分）。
fn prod(rel: &str) -> String {
    let raw = strip_comments(&read_source(rel));
    raw.split("#[cfg(test)]").next().unwrap_or("").to_string()
}

/// 把**字符串字面量内容**抹成空格（保留引号与换行 ⇒ 行号与列宽都不变）。
///
/// 为什么写点枚举必须抹掉字符串内容：`src/cli.rs` 的退出码政策是一段**多行字符串**，
/// 里面原样写着 `emit_domain_text_with(&text.replace(..), |judged| …)` 作为**例子**；
/// 不抹掉就会把「文档里的示例」当成一处真实写点（假红），
/// 而 A11 R-A 要求枚举前**必须剥注释**，同理字符串内容也不是代码。
fn blank_strings(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    #[derive(PartialEq)]
    enum St {
        Code,
        Str,
        Raw(usize),
        Char,
    }
    let mut st = St::Code;
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    while i < cs.len() {
        let c = cs[i];
        match st {
            St::Code => {
                if c == 'r' {
                    let mut j = i + 1;
                    let mut h = 0usize;
                    while cs.get(j) == Some(&'#') {
                        h += 1;
                        j += 1;
                    }
                    if cs.get(j) == Some(&'"') {
                        for k in i..=j {
                            out.push(cs[k]);
                        }
                        i = j + 1;
                        st = St::Raw(h);
                        continue;
                    }
                }
                if c == '"' {
                    out.push(c);
                    i += 1;
                    st = St::Str;
                    continue;
                }
                if c == '\'' {
                    let closes = cs.get(i + 2) == Some(&'\'')
                        || (cs.get(i + 1) == Some(&'\\')
                            && cs[i + 2..].iter().take(12).any(|x| *x == '\''));
                    out.push(c);
                    i += 1;
                    if closes {
                        st = St::Char;
                    }
                    continue;
                }
                out.push(c);
                i += 1;
            }
            St::Str => {
                if c == '\\' {
                    out.push(' ');
                    i += 1;
                    if let Some(&n) = cs.get(i) {
                        out.push(blank(n));
                        i += 1;
                    }
                } else if c == '"' {
                    out.push('"');
                    i += 1;
                    st = St::Code;
                } else {
                    out.push(blank(c));
                    i += 1;
                }
            }
            St::Raw(h) => {
                if c == '"' {
                    let mut j = i + 1;
                    let mut seen = 0usize;
                    while seen < h && cs.get(j) == Some(&'#') {
                        seen += 1;
                        j += 1;
                    }
                    if seen == h {
                        out.push('"');
                        i = j;
                        st = St::Code;
                    } else {
                        out.push(blank(c));
                        i += 1;
                    }
                } else {
                    out.push(blank(c));
                    i += 1;
                }
            }
            St::Char => {
                if c == '\\' {
                    out.push(' ');
                    i += 1;
                    if let Some(&n) = cs.get(i) {
                        out.push(blank(n));
                        i += 1;
                    }
                } else if c == '\'' {
                    out.push('\'');
                    i += 1;
                    st = St::Code;
                } else {
                    out.push(blank(c));
                    i += 1;
                }
            }
        }
    }
    out
}

/// 一个源文件的两个视图：**给人看的**（剥注释、字符串原样）与**给扫描器看的**
/// （再抹掉字符串内容）。两者行号严格对齐。
struct Src {
    display: String,
    code: String,
}

impl Src {
    fn load(rel: &str) -> Self {
        let display = prod(rel);
        let code = blank_strings(&display);
        Self { display, code }
    }
}

/// 函数体所在的行区间（`[lo, hi]`，0 基；按花括号配平）。
fn fn_range(lines: &[String], marker: &str) -> Option<(usize, usize)> {
    let at = lines.iter().position(|l| l.contains(marker))?;
    let mut depth = 0i32;
    let mut opened = false;
    for (i, l) in lines.iter().enumerate().skip(at) {
        depth += l.matches('{').count() as i32;
        depth -= l.matches('}').count() as i32;
        if l.contains('{') {
            opened = true;
        }
        if opened && depth <= 0 {
            return Some((at, i));
        }
    }
    None
}

/// 词边界：`at` 前面那个字符不得是标识符字符。
fn boundary_ok(line: &str, at: usize) -> bool {
    !line[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

// ══════════════════════════════════════════════════════════════
// T1 · 写点枚举（R-A）
// ══════════════════════════════════════════════════════════════

/// 客户可见写出的**构造拼写**。
///
/// ⚠️ 用**带括号**的整段：`print!` 与 `print(` 差一个 `!`，漏掉它会让扫描
/// 只认得 `emit_domain_*` —— 那种失败是「全绿」，因为少认的正好是不该判红的那部分。
const WRITE_CONSTRUCTS: &[&str] = &[
    "println!(",
    "print!(",
    "emit_domain_text(",
    "emit_domain_line(",
    "emit_domain_text_with(",
    "write_stdout(",
    "Json(",
    "Html(",
    "fs::write(",
    "encode_result(",
    "tool_call_result(",
];

/// 写出机制**本体**（定义处）：这些函数体里的 `write_stdout(` / `encode_result(`
/// 是「怎么写出」，不是「某条出口写了什么」，不参与枚举。
const WRITER_FNS: &[&str] = &[
    "fn emit_domain_text",
    "fn emit_domain_line",
    "fn emit_domain_text_with",
    "fn write_stdout",
    "pub fn encode_result",
    "pub fn tool_call_result",
];

/// 一处写点：行号（1 基）+ 命中的构造 + **窗口文本**（本行 + 下一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct WritePoint {
    line: usize,
    construct: &'static str,
    text: String,
}

/// ⭐ **纯函数**：枚举一段源码里的客户可见写点。
///
/// `code` = 抹掉字符串内容后的视图（**检测**用）；
/// `display` = 剥注释但字符串原样（**给白名单/登记 needle 匹配**用）。
/// 两者行号严格对齐。
fn write_points(code: &str, display: &str) -> Vec<WritePoint> {
    let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
    let shown: Vec<String> = display.lines().map(|s| s.to_string()).collect();
    // 机制本体的函数体：整段跳过。
    let mut skip: Vec<(usize, usize)> = Vec::new();
    for m in WRITER_FNS {
        if let Some((lo, hi)) = fn_range(&lines, m) {
            skip.push((lo, hi));
        }
    }
    // 第一遍：找出命中的行（0 基）。
    let mut hits: Vec<(usize, &'static str)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if skip.iter().any(|(lo, hi)| i >= *lo && i <= *hi) {
            continue;
        }
        'outer: for c in WRITE_CONSTRUCTS {
            let mut from = 0usize;
            while let Some(rel) = line[from..].find(c) {
                let at = from + rel;
                from = at + 1;
                if !boundary_ok(line, at) {
                    continue;
                }
                hits.push((i, c));
                break 'outer;
            }
        }
    }
    // 第二遍：窗口 = 本行 + 继续行（括号没配平时往下拼），
    // **但绝不吃进下一个写点**（否则相邻两处写点会互相「顶替」登记）。
    let mut out = Vec::new();
    for (idx, (i, construct)) in hits.iter().enumerate() {
        let next_hit = hits.get(idx + 1).map(|(j, _)| *j).unwrap_or(lines.len());
        let mut end = *i;
        let mut buf = shown.get(*i).cloned().unwrap_or_default();
        let mut open = paren_delta(&buf);
        while open > 0 && end + 1 < next_hit && end + 1 < shown.len() && end + 1 < *i + 3 {
            end += 1;
            buf.push('\n');
            buf.push_str(shown.get(end).map(|s| s.as_str()).unwrap_or(""));
            open += paren_delta(shown.get(end).map(|s| s.as_str()).unwrap_or(""));
        }
        out.push(WritePoint {
            line: i + 1,
            construct,
            text: buf,
        });
    }
    out
}

/// 括号净增量（`(` 减 `)`），用来判「这条写出语句是不是还没写完」。
fn paren_delta(s: &str) -> i32 {
    s.chars().fold(0, |acc, c| match c {
        '(' => acc + 1,
        ')' => acc - 1,
        _ => acc,
    })
}

/// 非客户写点白名单：**(文件, needle, 期望命中数, 理由)**。
///
/// `needle` 必须出现在写点的**窗口文本**里，且命中数必须**恰好等于** `expected`：
/// - 少一个 ⇒ 白名单过期（有人删了这个写点却没删登记）；
/// - 多一个 ⇒ 有人**照着白名单又抄了一个** ⇒ 红。
///
/// ⛔ 每一条都必须写清「为什么不是客户可见字节」——「未登记的非客户写点」不是结论。
const NON_CUSTOMER_WRITES: &[(&str, &str, usize, &str)] = &[
    // ── CLI：建模轨 / 内部回执（口径同 D2–D8 / I1–I9，逐条说明「为什么不是客户视图」）──
    ("src/cli.rs", "聚类规则：{}", 1, "`concept import` 的内部回执：打的是声明的聚类规则名（运维面），不含客户视图字段"),
    ("src/cli.rs", "业务确认：{who}", 1, "`concept import` 回执：业务确认人（运维/建模面）"),
    ("src/cli.rs", "业务尚未确认", 1, "`concept import` 回执：一句固定提示，零数据"),
    ("src/cli.rs", r"[{}] {} {}", 2, "两处 `[seq] kind target_key` 逐条回执：`target_key` 是建模 key 本身，属建模轨内容（同 D5/I1 的豁免理由）"),
    ("src/cli.rs", "下一步：ontok review show", 1, "`concept import` 的下一步提示：固定文案 + 项目名（不是客户视图字段）"),
    ("src/cli.rs", r"当前 version\t{}", 1, "`concept digest` 的版本号回执（同 I2 的指纹/版本/契约三行）"),
    ("src/cli.rs", r"契约\t{}", 1, "`concept digest` 的契约校验行（内部校验工具输出）"),
    ("src/cli.rs", "本提案无 extracted_by", 1, "`propose import` 的固定提示（零数据）"),
    ("src/cli.rs", "抽取来源：{}", 1, "`propose import` 的抽取来源（agent 名 + 版本，运维面）"),
    ("src/cli.rs", "review_show_prefix(&proposal, &row.status)", 1, "`review show` 的**抬头**（提案别名 + 状态），正文由 D2 的闭包式出口写出"),
    ("src/cli.rs", "提案未通过：可应用", 1, "`proposal apply` 的汇总回执（条数），与 D6 的逐条被拦清单同源"),
    ("src/cli.rs", "已写出客户视图页面", 1, "C2（`--out`）写文件之后的**回执**：路径 / 对象数 / 抽样数，不是客户字节本身"),
    ("src/cli.rs", "x.summary()", 1, "`ontology diff` 的逐条变化行（I9 的子行；diff 已裁定归建模轨，R1/R3）"),
    // ── HTTP：错误通道（正文都过 `safe_error` 或为固定文案）──
    ("src/server/http.rs", "(self.status(), Json(json!({ \"error\": msg })))", 1, "`ApiError::into_response`：`msg` 来自 `ApiError::message()`，已经过 `safe_error`"),
    ("src/server/http.rs", "(status, Json(json!({ \"error\": msg })))", 1, "`err()` 助手：调用方传的是固定文案或 Z1 的 `safe_error` 产物；这是**错误通道**的唯一构造点"),
    ("src/server/http.rs", "这里只读：只能看", 1, "405 的固定文案（零数据）"),
    ("src/server/http.rs", "没有可用的访问口令", 1, "401 的固定文案（不回显令牌，REQ-106 已钉）"),
    // ── MCP：协议形状（空结果 / isError 包装，不含客户数据正文）──
    ("src/mcp/serve.rs", "p::encode_result(id, p::empty_result())", 2, "`notifications/initialized` 与 `ping`：协议空结果，零数据"),
    ("src/mcp/serve.rs", "p::tool_call_error_result(e.message())", 1, "`tools/call` 命中发布闸 ⇒ `isError`：正文是 `PipelineError::message()`（按设计不含词条）"),
    ("src/mcp/serve.rs", "p::tool_call_error_result(&tools::safe_error", 1, "`tools/call` 的业务 NotFound ⇒ 错误结果：正文经 `safe_error`"),
];

/// 整文件豁免：**只允许**「不承载客户视图」的文件（运维二进制 / 响应构造器）。
///
/// ⛔ 这里刻意用**精确路径**而不是目录前缀：新增一个 `src/bin/evil.rs` 也必须被看见。
const NON_OUTLET_FILES: &[(&str, &str)] = &[
    ("src/mcp/protocol.rs", "MCP 响应**构造器**：`encode_result` / `tool_call_result` 返回 `String`，自己不写任何 fd；真正的写出在 `mcp/serve.rs` 的 `dispatch` 返回值上"),
    ("src/bin/ontok-gate-preflight.rs", "运维预检二进制（A8：本机 unix-only，跑测试时临时挪出）；不属口径 D 的 33 条出口"),
    ("src/bin/ontok-introspect.rs", "运维二进制：introspect 回执"),
    ("src/bin/ontok-propose.rs", "运维二进制：提案导入回执"),
    ("src/bin/ontok-translate-names.rs", "运维二进制：改名批处理回执"),
];

/// 登记表里某个文件的全部 needle。
fn registered_needles(path: &str) -> Vec<&'static str> {
    ontok::domain::pipeline::OUTLETS
        .iter()
        .filter(|o| o.path == path)
        .map(|o| o.call_site)
        .collect()
}

/// ⭐ **纯函数**：给定「文件 → 生产代码」与两张白名单，返回**未登记的写点**。
fn unregistered_write_points(files: &BTreeMap<String, Src>) -> (Vec<String>, Vec<String>) {
    let mut unregistered = Vec::new();
    let mut stale_allow = Vec::new();
    // 白名单命中计数（按 (path, needle)）。
    let mut allow_hits: BTreeMap<(&str, &str), usize> = BTreeMap::new();

    for (rel, src) in files {
        let points = write_points(&src.code, &src.display);
        if points.is_empty() {
            continue;
        }
        if NON_OUTLET_FILES.iter().any(|(p, _)| p == rel) {
            continue;
        }
        let needles = registered_needles(rel);
        for p in points {
            if needles.iter().any(|n| p.text.contains(n)) {
                continue;
            }
            let mut covered = false;
            for (path, needle, _, _) in NON_CUSTOMER_WRITES {
                if path == rel && p.text.contains(needle) {
                    *allow_hits.entry((path, needle)).or_insert(0) += 1;
                    covered = true;
                }
            }
            if !covered {
                unregistered.push(format!(
                    "  - {rel}:{} `{}` —— 既不在 OUTLETS 登记里，也不在非客户白名单里\n      {}",
                    p.line,
                    p.construct,
                    p.text.replace('\n', " ⏎ ")
                ));
            }
        }
    }

    // 白名单双向：每条的命中数必须恰好等于期望值。
    for (path, needle, expected, _) in NON_CUSTOMER_WRITES {
        let got = allow_hits.get(&(*path, *needle)).copied().unwrap_or(0);
        if got != *expected {
            stale_allow.push(format!(
                "  - {path} 的 `{needle}` 期望命中 {expected} 次，实际 {got} 次"
            ));
        }
    }
    // ⚠️ 刻意**不**要求整文件豁免的路径必须存在：A8 绕行会临时把
    // `src/bin/ontok-gate-preflight.rs` 挪出 `src/`（验收方式里写死的环境事实）。
    // 代价是「删了源码、白名单还在」这一格看不见 —— 已登记为已知边界。
    // 登记表这一侧：needle 必须真的在**剥过注释的**源码里（N1 的修法）。
    for o in ontok::domain::pipeline::OUTLETS {
        match files.get(o.path) {
            Some(s) if s.display.contains(o.call_site) => {}
            Some(_) => unregistered.push(format!(
                "  - {}: 登记 needle {:?} 在 {} 的**代码**里找不到（注释里的不算）",
                o.id, o.call_site, o.path
            )),
            None => unregistered.push(format!("  - {}: 找不到 {}", o.id, o.path)),
        }
    }
    (unregistered, stale_allow)
}

fn all_src_code() -> BTreeMap<String, Src> {
    let mut m = BTreeMap::new();
    for rel in src_files() {
        if rel == PIPELINE_SRC {
            continue;
        }
        m.insert(rel.clone(), Src::load(&rel));
    }
    m
}

#[test]
fn t1_every_customer_write_point_is_registered() {
    let files = all_src_code();
    assert!(!files.is_empty(), "src/ 一个 .rs 都没扫到 —— 扫描器自己坏了");

    // 反空转：扫描器必须认得出**今天不存在**的那几种形状（QA 变异的四种绕过）。
    let synthetic = [
        (
            "async fn get_raw(State(s): State<AppState>) -> Json<Value> { Json(v) }",
            "Json(",
        ),
        (r#"fn x() { println!("{}", serde_json::to_string_pretty(&cv)?); }"#, "println!("),
        ("fn x() { crate::cli::emit_domain_line(&raw); }", "emit_domain_line("),
        ("fn x() { std::fs::write(p, raw)?; }", "fs::write("),
        ("fn x() { p::encode_result(id, raw) }", "encode_result("),
    ];
    for (src, want) in synthetic {
        let got = write_points(&blank_strings(src), src);
        assert!(
            got.iter().any(|p| p.construct == want),
            "写点扫描器认不出 `{want}` ⇒ 下面那圈「全部登记」是空转：{got:?}"
        );
    }
    // 反向：干净片段必须 0 命中（防判据恒红）。
    assert!(
        write_points("fn x() { let a = 1 + 1; }", "fn x() { let a = 1 + 1; }").is_empty(),
        "扫描器在无关代码上命中了 —— 判据会恒红"
    );
    // 机制本体不算写点：`write_stdout` 的定义与内部调用不该被当成出口。
    assert!(
        write_points("fn write_stdout(text: &str) { out.write_all(text.as_bytes()); }", "").is_empty(),
        "写出函数的**定义体**被当成了出口写点 ⇒ 每个出口文件都会恒红"
    );

    let (unregistered, stale) = unregistered_write_points(&files);
    assert!(stale.is_empty(), "⛔ 白名单过期：\n{}", stale.join("\n"));
    assert!(
        unregistered.is_empty(),
        "⛔ 有 {} 处客户可见写点**没有登记**（AC-003：新增出口必须当场红）：\n{}",
        unregistered.len(),
        unregistered.join("\n")
    );
}

// ══════════════════════════════════════════════════════════════
// T2 · 写出的实参必须是管线产物（R-D）
// ══════════════════════════════════════════════════════════════

/// 客户字节写出函数 —— 实参**必须**是管线产物的名字。
const CUSTOMER_WRITERS: &[&str] = &["emit_domain_text(", "emit_domain_line("];

/// 是不是一个裸标识符（`html` / `json` / `json_text`）。
fn plain_ident(s: &str) -> Option<&str> {
    let t = s.trim();
    let ok = !t.is_empty()
        && t.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then_some(t)
}

/// ⭐ **纯函数**：找出「写出实参不是管线产物」或「产物在写出前被加工/被判过」的地方。
fn bad_customer_writes(code: &str) -> Vec<String> {
    let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
    let mut out = Vec::new();

    // 先定位所有客户写出的**行与实参**。
    let mut sites: Vec<(usize, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        for w in CUSTOMER_WRITERS {
            let Some(at) = line.find(w) else { continue };
            if !boundary_ok(line, at) {
                continue;
            }
            if line.trim_start().starts_with("fn ") || line.trim_start().starts_with("pub fn ") {
                continue;
            }
            // 取配对括号内的实参（跨行时往下拼）。
            let mut buf = line[at..].to_string();
            let mut hi = i;
            loop {
                let open = buf.find('(').unwrap();
                let mut depth = 0i32;
                let mut close = None;
                for (k, c) in buf.chars().enumerate().skip(open) {
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(k);
                            break;
                        }
                    }
                }
                match close {
                    Some(k) => {
                        sites.push((i, buf[open + 1..k].to_string()));
                        break;
                    }
                    None => {
                        hi += 1;
                        if hi >= lines.len() {
                            break;
                        }
                        buf.push('\n');
                        buf.push_str(&lines[hi]);
                    }
                }
            }
        }
    }

    for (line_idx, arg) in sites {
        let Some(ident) = plain_ident(&arg) else {
            out.push(format!(
                "  - 第 {} 行的写出实参 `{}` **不是裸标识符** —— 只能把管线产物本身交出去",
                line_idx + 1,
                arg.replace('\n', " ")
            ));
            continue;
        };
        // 找这个标识符的 `let` 赋值（从写出往上找最近的一处）。
        let pattern = format!("let {ident} = ");
        let assign = lines
            .iter()
            .enumerate()
            .take(line_idx)
            .rev()
            .find(|(_, l)| l.trim_start().starts_with(&pattern));
        let Some((assign_idx, first)) = assign else {
            out.push(format!(
                "  - 第 {} 行的写出实参 `{ident}` 在函数里找不到 `let {ident} = …` 的绑定",
                line_idx + 1
            ));
            continue;
        };
        // 把赋值语句拼完整（可能跨行），判它是不是管线入口。
        let mut rhs = first[first.find('=').unwrap() + 1..].to_string();
        let mut k = assign_idx;
        while !rhs.contains(';') && k + 1 < lines.len() {
            k += 1;
            rhs.push(' ');
            rhs.push_str(&lines[k]);
        }
        let is_pipeline = rhs.contains("publish_customer(")
            || rhs.contains("pipeline::render(")
            || rhs.contains("pipeline::publish_value(");
        if !is_pipeline {
            out.push(format!(
                "  - 第 {} 行的写出实参 `{ident}` 绑定自 `{}` —— **不是管线产物**\
                 （出口自己拼了一份字节，然后假装它过了闸）",
                line_idx + 1,
                rhs.trim().lines().next().unwrap_or("")
            ));
            continue;
        }
        // 赋值与写出之间：产物只许被**消费**，不许被再加工/再判定。
        for (i, l) in lines.iter().enumerate().take(line_idx).skip(k + 1) {
            let mut rest = l.as_str();
            while let Some(at) = rest.find(ident) {
                if !boundary_ok(rest, at) {
                    rest = &rest[at + 1..];
                    continue;
                }
                let tail = rest[at + ident.len()..].trim_start();
                // 允许的消费：纯访问器（`dropped_object_count` 之类，返回值不是字节），
                // 以及 `.as_str()` 只许出现在 **`fs::write(` / `err(` 的实参位**。
                // ⛔ `local_ok(json.as_str())` 这种「把产物交给另一个函数判一遍」不算消费。
                let allowed = tail.starts_with(".dropped_object_count()")
                    || tail.starts_with(".report()")
                    || tail.starts_with(".outlet()")
                    || (tail.starts_with(".as_str()")
                        && (l.contains("fs::write(") || l.contains("err(")));
                if !allowed {
                    out.push(format!(
                        "  - 第 {} 行的管线产物 `{ident}` 在写出之前被用在了别处：`{}`\
                         —— 那是一次「出口自己又判了一遍 / 又加工了一遍」",
                        i + 1,
                        l.trim()
                    ));
                }
                rest = &rest[at + ident.len()..];
            }
        }
    }
    out
}

#[test]
fn t2_customer_writes_take_pipeline_products_only() {
    // 反空转：四种坏形状必须被判出来。
    let bad = [
        // QA 变异 #5：先写未过闸的字节。
        "fn x() {\n    let raw = serde_json::to_string_pretty(&cv)?;\n    crate::cli::emit_domain_line(raw);\n}",
        // 先写后判：手拼的串直接交出去。
        "fn x() {\n    crate::cli::emit_domain_line(&raw);\n}",
        // 保留管线调用、但写出的绑定点不是它。
        "fn x() {\n    let json = publish_customer(OUTLET, Product::Json(&cv));\n    let out = serde_json::to_string(&cv)?;\n    crate::cli::emit_domain_line(out);\n}",
        // 保留管线调用、在写出之前对产物再判一次（B5 的形态）。
        "fn x() {\n    let json = publish_customer(OUTLET, Product::Json(&cv));\n    if !local_ok(json) { return Ok(()); }\n    crate::cli::emit_domain_line(json);\n}",
    ];
    for src in bad {
        assert!(
            !bad_customer_writes(src).is_empty(),
            "T2 认不出这种形状 ⇒ 这一格是空的：\n{src}"
        );
    }
    // 反向：正解形状必须 0 命中。
    let good = "fn x() {\n    let json = publish_customer(\n        crate::domain::pipeline::OutletId::C3,\n        crate::domain::pipeline::Product::CustomerJson(&cv),\n    );\n    crate::cli::emit_domain_line(json);\n}";
    assert!(
        bad_customer_writes(good).is_empty(),
        "正解形状被判红了 ⇒ 判据不可用：{:?}",
        bad_customer_writes(good)
    );

    // 实物：三个出口文件 + 全仓（`emit_domain_*` 只在 cli.rs 出现）。
    let mut all: Vec<String> = Vec::new();
    for rel in OUTLET_FILES {
        all.extend(bad_customer_writes(&prod(rel)));
    }
    assert!(
        all.is_empty(),
        "⛔ 出口层有 {} 处写出不是「管线产物本身」：\n{}",
        all.len(),
        all.join("\n")
    );
}

// ══════════════════════════════════════════════════════════════
// T3 · 出口 ID 记号枚举（R-B）
// ══════════════════════════════════════════════════════════════

/// 每个出口文件里**允许出现**的 `OutletId::` 记号与次数。
///
/// 这是「源码 → 登记表」的那一半：新增一条管线调用、或把某处的 ID 换成
/// 另一条（`C3` → `D1`，用建模轨绕过客户闸）都会让次数对不上 ⇒ 红。
///
/// ⛔ QA 变异 #4（`spec.track = Track::Internal`）在这一版**写不出来**：
/// `OutletSpec` 的字段全私有、无公开构造器，`render` 只收 `OutletId`。
const OUTLET_ID_TOKENS: &[(&str, &str, usize)] = &[
    ("src/cli.rs", "OutletId::C1", 1),

    ("src/cli.rs", "OutletId::C3", 1),
    ("src/cli.rs", "OutletId::C4", 1),
    ("src/cli.rs", "OutletId::C5", 1),
    ("src/cli.rs", "OutletId::D1", 1),
    ("src/server/http.rs", "OutletId::H1", 1),
    ("src/server/http.rs", "OutletId::H2", 1),
    ("src/server/http.rs", "OutletId::H3", 1),
    ("src/server/http.rs", "OutletId::Z1", 1),
    ("src/mcp/serve.rs", "OutletId::McpTools", 1),
    ("src/mcp/serve.rs", "OutletId::Z2", 1),
    ("src/mcp/serve.rs", "OutletId::Z3", 1),
];

/// ⭐ **纯函数**：文件里出现的全部 `OutletId::<X>` 记号（计数）。
fn id_tokens(code: &str) -> BTreeMap<String, usize> {
    const PREFIX: &str = "OutletId::";
    let mut m: BTreeMap<String, usize> = BTreeMap::new();
    for line in code.lines() {
        let mut rest = line;
        while let Some(at) = rest.find(PREFIX) {
            if !boundary_ok(rest, at) {
                rest = &rest[at + 1..];
                continue;
            }
            let tail = &rest[at + PREFIX.len()..];
            let name: String = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                *m.entry(format!("{PREFIX}{name}")).or_insert(0) += 1;
            }
            rest = tail;
        }
    }
    m
}

#[test]
fn t3_outlet_id_tokens_are_registered() {
    // 反空转。
    let got = id_tokens("f(crate::domain::pipeline::OutletId::C3, x); f(OutletId::D1);");
    assert_eq!(got.get("OutletId::C3"), Some(&1), "{got:?}");
    assert_eq!(got.get("OutletId::D1"), Some(&1), "{got:?}");
    assert!(id_tokens("let NotOutletId = 1;").is_empty(), "词边界失效");

    let mut bad = Vec::new();
    for rel in OUTLET_FILES {
        let got = id_tokens(&prod(rel));
        let want: BTreeMap<String, usize> = OUTLET_ID_TOKENS
            .iter()
            .filter(|(p, _, _)| p == rel)
            .map(|(_, t, n)| (t.to_string(), *n))
            .collect();
        for (tok, n) in &got {
            match want.get(tok) {
                Some(w) if w == n => {}
                Some(w) => bad.push(format!(
                    "  - {rel}: `{tok}` 出现 {n} 次，登记的是 {w} 次\
                     （换 ID / 多加一处管线调用都会落到这里）"
                )),
                None => bad.push(format!(
                    "  - {rel}: `{tok}` **没有登记在 OUTLET_ID_TOKENS 里** —— \
                     新增出口必须先登记它的身份"
                )),
            }
        }
        for (tok, n) in &want {
            if !got.contains_key(tok) {
                bad.push(format!("  - {rel}: 登记的 `{tok}`（{n} 次）在源码里一次都没有"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "⛔ 出口 ID 记号与登记表不一致：\n{}",
        bad.join("\n")
    );

    // ⭐ R-B 的类型层那一半：全仓**没有**任何 `pub fn` 返回 `OutletSpec`
    // （`spec_of` 已私有）⇒ 出口连一个规格值都拿不到，更谈不上去改它的轨道。
    let mut leaks = Vec::new();
    for rel in src_files() {
        let code = prod(&rel);
        for line in code.lines() {
            let t = line.trim_start();
            let public = t.starts_with("pub fn ") || t.starts_with("pub const fn ");
            if public && t.contains("-> OutletSpec") {
                leaks.push(format!("  - {rel}: {t}"));
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "⛔ 有 `pub fn` 返回 `OutletSpec` —— 出口就能拿到一个可读的规格值：\n{}",
        leaks.join("\n")
    );
    assert!(
        !prod(PIPELINE_SRC).contains("pub const fn spec_of("),
        "`spec_of` 必须是模块私有的 —— 公开它等于把「轨道查表」还给了调用方"
    );
}

// ══════════════════════════════════════════════════════════════
// T4 · 客户产物的公开构造器枚举（R-C / B4）
// ══════════════════════════════════════════════════════════════

/// 客户产物的类型名。
const GUARDED_TYPES: &[&str] = &["OutletRender", "GuardedValue"];

/// ⭐ **纯函数**：枚举「返回客户产物的**公开**函数」的签名（不含私有 `build`）。
///
/// 返回 `名称 -> 所在行号`。签名可能跨行，所以看的是「`fn` 那一行 + 下两行」。
fn public_factories(code: &str) -> Vec<(String, usize)> {
    let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let public = t.starts_with("pub fn ")
            || t.starts_with("pub const fn ")
            || t.starts_with("pub async fn ");
        if !public {
            continue;
        }
        let mut sig = t.to_string();
        for k in 1..3 {
            if let Some(n) = lines.get(i + k) {
                sig.push(' ');
                sig.push_str(n.trim());
            }
        }
        let returns_guarded = GUARDED_TYPES.iter().any(|ty| {
            let ret = format!("-> {ty}");
            let ret_res = format!("-> Result<{ty}");
            sig.contains(&ret) || sig.contains(&ret_res) || sig.contains(&format!("-> Self"))
        });
        if !returns_guarded {
            continue;
        }
        // `-> Self` 只有在 impl 里才有意义：判它是不是那两个类型的 impl。
        if sig.contains("-> Self") && !sig.contains("OutletRender") && !sig.contains("GuardedValue")
        {
            // 往上找最近的 `impl <Type>`。
            let in_guarded_impl = lines[..=i]
                .iter()
                .rev()
                .find(|l| l.trim_start().starts_with("impl "))
                .map(|l| GUARDED_TYPES.iter().any(|ty| l.contains(ty)))
                .unwrap_or(false);
            if !in_guarded_impl {
                continue;
            }
        }
        let name = t
            .split("fn ")
            .nth(1)
            .and_then(|s| s.split('(').next())
            .unwrap_or("?")
            .trim()
            .to_string();
        out.push((name, i + 1));
    }
    out
}

#[test]
fn t4_only_one_public_factory_per_guarded_type() {
    // 反空转：QA 变异 #2 的两种拼写都必须被认出来（⛔ 不是只匹配 `pub fn new(`）。
    let craft = "impl OutletRender {\n    pub fn craft(text: String) -> Self {\n        Self { text }\n    }\n}";
    assert_eq!(
        public_factories(craft),
        vec![("craft".to_string(), 2)],
        "换一个名字的公开构造器认不出来 ⇒ 这一格是靠拼写挡的"
    );
    let newish = "impl GuardedValue {\n    pub const fn new(text: String) -> GuardedValue {\n        todo!()\n    }\n}";
    assert_eq!(public_factories(newish).len(), 1);
    // 私有构造器不算（管线内部要能造）。
    assert!(
        public_factories("impl OutletRender {\n    fn build(text: String) -> Self { todo!() }\n}")
            .is_empty(),
        "私有 `fn build` 被算成公开构造器 ⇒ 判据会恒红"
    );
    // 自由函数也要认。
    assert_eq!(
        public_factories("pub fn forged(text: &str) -> Result<OutletRender, E> { todo!() }").len(),
        1
    );

    // 实物：全仓 `pub fn` 返回这两个类型的，**恰好**是管线的两个入口。
    let mut got: Vec<(String, String)> = Vec::new();
    for rel in src_files() {
        if rel == PIPELINE_SRC {
            // 管线自己的入口也一起枚举（它们是**唯一**允许的两个）。
        }
        for (name, line) in public_factories(&prod(&rel)) {
            got.push((rel.clone(), format!("{name}@{line}")));
        }
    }
    let mut names: Vec<String> = got.iter().map(|(_, n)| n.split('@').next().unwrap().to_string()).collect();
    names.sort();
    names.dedup();
    assert_eq!(
        names,
        vec!["publish_value".to_string(), "render".to_string()],
        "全仓返回客户产物的 `pub fn` 应当恰好是 `render` / `publish_value` 两个，\
         实际：{got:?}\n（`publish_error_text` 这个「任意 &str → 可写出产物」的工厂已在第 2 轮删除）"
    );
    // 每条都必须落在管线模块里（谁敢在出口文件里开工厂就红）。
    for (rel, n) in &got {
        assert_eq!(rel, PIPELINE_SRC, "客户产物的公开工厂出现在 {rel}: {n}");
    }
}

// ══════════════════════════════════════════════════════════════
// T5 · 出口文件的 fn 定义枚举（B5：不许有第二个判据实现）
// ══════════════════════════════════════════════════════════════

/// ⭐ **纯函数**：一段源码里所有**函数定义**的名字。
fn fn_definitions(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in code.lines() {
        let t = line.trim_start();
        let at = if let Some(a) = t.find("fn ") {
            if a == 0 || t[..a].trim().is_empty() {
                a + 3
            } else if t.starts_with("pub fn ")
                || t.starts_with("pub(crate) fn ")
                || t.starts_with("pub const fn ")
                || t.starts_with("pub async fn ")
                || t.starts_with("async fn ")
                || t.starts_with("const fn ")
            {
                a + 3
            } else {
                continue;
            }
        } else {
            continue;
        };
        let name: String = t[at..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}

/// 三个出口文件里**允许存在**的函数：(文件, 函数名, 用途)。
///
/// ⛔ 新增一个函数（尤其是 `fn local_ok(s) -> bool` 这种）必须在这里登记；
/// 「出口文件里悄悄多了一个判据」是本需求 B5 那一格。
const OUTLET_FNS: &[(&str, &str, &str)] = &[
    ("src/cli.rs", "install_broken_pipe_guard", "运维辅助"),
    ("src/cli.rs", "self_check_exit_code", "运维辅助"),
    ("src/cli.rs", "review_show_prefix", "运维辅助"),
    ("src/cli.rs", "emit_domain_text", "写出机制（类型层只收 OutletRender）"),
    ("src/cli.rs", "emit_domain_line", "写出机制（类型层只收 OutletRender）"),
    ("src/cli.rs", "emit_domain_text_with", "写出机制（类型层只收 OutletRender）"),
    ("src/cli.rs", "write_stdout", "写出机制（类型层只收 OutletRender）"),
    ("src/cli.rs", "publish_customer", "管线适配器（Err → 退出码）"),
    ("src/cli.rs", "run", "入口分发"),
    ("src/cli.rs", "concept_cmd", "子命令 handler"),
    ("src/cli.rs", "project_cmd", "子命令 handler"),
    ("src/cli.rs", "version_cmd", "子命令 handler"),
    ("src/cli.rs", "propose_cmd", "子命令 handler"),
    ("src/cli.rs", "review_cmd", "子命令 handler"),
    ("src/cli.rs", "proposal_cmd", "子命令 handler"),
    ("src/cli.rs", "ontology_cmd", "子命令 handler"),
    ("src/cli.rs", "resolve_proposal", "运维辅助"),
    ("src/cli.rs", "newest_proposal_project", "运维辅助"),
    ("src/cli.rs", "resolve_proposal_arg", "运维辅助"),
    ("src/cli.rs", "proposal_alias", "运维辅助"),
    ("src/cli.rs", "snapshot_to_view", "运维辅助"),
    ("src/server/http.rs", "load_frozen_view", "HTTP 路由/装配"),
    ("src/server/http.rs", "status", "HTTP 路由/装配"),
    ("src/server/http.rs", "message", "HTTP 路由/装配"),
    ("src/server/http.rs", "into_response", "HTTP 路由/装配"),
    ("src/server/http.rs", "err", "HTTP 路由/装配"),
    ("src/server/http.rs", "publish_json", "HTTP 路由/装配"),
    ("src/server/http.rs", "check_scope", "HTTP 路由/装配"),
    ("src/server/http.rs", "get_graph", "HTTP 路由/装配"),
    ("src/server/http.rs", "get_node", "HTTP 路由/装配"),
    ("src/server/http.rs", "index", "HTTP 路由/装配"),
    ("src/server/http.rs", "read_only_not_allowed", "HTTP 路由/装配"),
    ("src/server/http.rs", "normalize_405", "HTTP 路由/装配"),
    ("src/server/http.rs", "fallback", "HTTP 路由/装配"),
    ("src/server/http.rs", "require_token", "HTTP 路由/装配"),
    ("src/server/http.rs", "token_matches", "HTTP 路由/装配"),
    ("src/server/http.rs", "build_router", "HTTP 路由/装配"),
    ("src/server/http.rs", "state_from_view", "HTTP 路由/装配"),
    ("src/server/http.rs", "serve", "HTTP 路由/装配"),
    ("src/mcp/serve.rs", "prepare", "MCP 分发/装配"),
    ("src/mcp/serve.rs", "serve", "MCP 分发/装配"),
    ("src/mcp/serve.rs", "handle_line", "MCP 分发/装配"),
    ("src/mcp/serve.rs", "dispatch", "MCP 分发/装配"),

];

#[test]
fn t5_outlet_files_declare_every_function() {
    // 反空转：QA 变异 #1 的写法必须被枚举出来。
    let got = fn_definitions("fn local_ok(s: &str) -> bool { s.contains(\"key\") }");
    assert_eq!(got, vec!["local_ok".to_string()], "{got:?}");
    assert_eq!(
        fn_definitions("    pub async fn index(State(s): State<AppState>) -> Html<String> {")
            .len(),
        1,
        "`pub async fn` 认不出来"
    );
    assert!(fn_definitions("let f = |x| x;").is_empty(), "闭包不是 fn 定义");

    let mut bad = Vec::new();
    for rel in OUTLET_FILES {
        let got = fn_definitions(&prod(rel));
        let want: BTreeSet<&str> = OUTLET_FNS
            .iter()
            .filter(|(p, _, _)| p == rel)
            .map(|(_, n, _)| *n)
            .collect();
        for name in &got {
            if !want.contains(name.as_str()) {
                bad.push(format!(
                    "  - {rel}: `fn {name}` **没有登记在 OUTLET_FNS 里** —— \
                     出口文件里出现新函数（例如一份本地判据 `fn local_ok(s) -> bool`）\
                     必须显式登记它的用途"
                ));
            }
        }
        for name in &want {
            if !got.iter().any(|g| g == name) {
                bad.push(format!("  - {rel}: 登记的 `fn {name}` 在源码里不存在"));
            }
        }
    }
    assert!(bad.is_empty(), "⛔ 出口函数登记表与源码不一致：\n{}", bad.join("\n"));
}

// ══════════════════════════════════════════════════════════════
// T6 · 模块枚举（B5：新增模块即红）
// ══════════════════════════════════════════════════════════════

/// ⭐ **纯函数**：`mod x;` 声明（不含 `mod tests {}` 这类内联模块）。
fn module_decls(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in code.lines() {
        let t = line.trim_start();
        let t = t.strip_prefix("pub ").unwrap_or(t);
        let Some(rest) = t.strip_prefix("mod ") else { continue };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        // `mod x;` 才算（`mod x {` 是内联模块，不产生新文件）。
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with(';') {
            out.push(name);
        }
    }
    out
}

/// 全仓 `mod x;` 的登记表（文件 → 模块名）。
///
/// 新增一个模块（例如把一份自造判据拆到 `src/domain/local.rs`）必须在这里登记。
const MODULES: &[(&str, &str)] = &[
    ("src/lib.rs", "agent"),
    ("src/lib.rs", "cli"),
    ("src/lib.rs", "config"),
    ("src/lib.rs", "db"),
    ("src/lib.rs", "domain"),
    ("src/lib.rs", "extract"),
    ("src/lib.rs", "mcp"),
    ("src/lib.rs", "server"),
    ("src/agent/mod.rs", "acp"),
    ("src/agent/mod.rs", "assemble"),
    ("src/agent/mod.rs", "factory"),
    ("src/agent/mod.rs", "group"),
    ("src/agent/mod.rs", "llm"),
    ("src/agent/mod.rs", "orchestrate"),
    ("src/agent/mod.rs", "registry"),
    ("src/agent/mod.rs", "rule_based"),
    ("src/agent/acp/mod.rs", "parse"),
    ("src/agent/acp/mod.rs", "process"),
    ("src/agent/acp/mod.rs", "protocol"),
    ("src/agent/llm/mod.rs", "protocol"),
    ("src/agent/llm/mod.rs", "transport"),
    ("src/db/mod.rs", "agent_registry"),
    ("src/db/mod.rs", "apply"),
    ("src/db/mod.rs", "pg"),
    ("src/db/mod.rs", "pool"),
    ("src/db/mod.rs", "project"),
    ("src/db/mod.rs", "proposal"),
    ("src/db/mod.rs", "snapshot_store"),
    ("src/domain/mod.rs", "apply"),
    ("src/domain/mod.rs", "concept"),
    ("src/domain/mod.rs", "diff"),
    ("src/domain/mod.rs", "graph"),
    ("src/domain/mod.rs", "hierarchy"),
    ("src/domain/mod.rs", "html_view"),
    ("src/domain/mod.rs", "identifier_shape"),
    ("src/domain/mod.rs", "model_view"),
    ("src/domain/mod.rs", "overview"),
    ("src/domain/mod.rs", "pipeline"),
    ("src/domain/mod.rs", "property_filter"),
    ("src/domain/mod.rs", "review"),
    ("src/domain/mod.rs", "review_view"),
    ("src/domain/mod.rs", "snapshot"),
    ("src/domain/mod.rs", "view"),
    ("src/extract/mod.rs", "cardinality"),
    ("src/extract/mod.rs", "introspect"),
    ("src/extract/mod.rs", "names"),
    ("src/extract/mod.rs", "propose"),
    ("src/mcp/mod.rs", "protocol"),
    ("src/mcp/mod.rs", "serve"),
    ("src/mcp/mod.rs", "tools"),
    ("src/server/mod.rs", "http"),
    ("src/server/mod.rs", "page"),
];

#[test]
fn t6_every_module_is_registered() {
    assert_eq!(module_decls("pub mod pipeline;"), vec!["pipeline".to_string()]);
    assert!(module_decls("mod tests {").is_empty(), "内联模块不该算");

    let mut bad = Vec::new();
    for rel in src_files() {
        let code = prod(&rel);
        for name in module_decls(&code) {
            let ok = MODULES.iter().any(|(p, m)| *p == rel && *m == name);
            if !ok {
                bad.push(format!("  - {rel}: `mod {name};` 没有登记"));
            }
        }
    }
    for (p, m) in MODULES {
        let code = prod(p);
        if !module_decls(&code).iter().any(|g| g == m) {
            bad.push(format!("  - {p}: 登记的 `mod {m};` 在源码里不存在"));
        }
    }
    assert!(bad.is_empty(), "⛔ 模块登记表与源码不一致：\n{}", bad.join("\n"));
}

// ══════════════════════════════════════════════════════════════
// T7 · 管线模块自己不许往 stdout 写
// ══════════════════════════════════════════════════════════════

#[test]
fn t7_pipeline_module_never_writes_stdout() {
    /// 按**词边界**找 stdout 宏：`eprintln!(` 不能算成 `println!(`（子串陷阱）。
    fn hits(code: &str, needle: &str) -> usize {
        let mut n = 0;
        for line in code.lines() {
            let mut from = 0usize;
            while let Some(rel) = line[from..].find(needle) {
                let at = from + rel;
                from = at + 1;
                if boundary_ok(line, at) {
                    n += 1;
                }
            }
        }
        n
    }
    let code = prod(PIPELINE_SRC);
    for bad in ["println!(", "print!(", "write_stdout("] {
        assert_eq!(
            hits(&code, bad),
            0,
            "管线模块里出现了 `{bad}` —— 管线只产出「可写出的产物」，\
             写出是出口的事（否则闸与写出的时序又回到同一个函数里）"
        );
    }
    // 反空转：`eprintln!` 确实存在，但不该被算成 stdout（否则判据恒红）。
    assert_eq!(hits(&code, "println!("), 0);
    assert_eq!(hits("eprintln!(\"x\");", "println!("), 0, "子串陷阱：`eprintln!` 被算成 stdout");
    assert_eq!(hits("println!(\"x\");", "println!("), 1);
    // 反空转：确认我们读到的确实是管线模块（而不是一个空文件）。
    assert!(code.contains("pub fn render("), "读不到管线模块的入口 ⇒ 上面那圈是空转");
    // stderr 报告是允许的（运维面），但只有 DropReport 一处。
    assert_eq!(
        code.matches("eprintln!(").count(),
        1,
        "管线里的 stderr 写出应当只有 `DropReport::emit` 一处"
    );
}

