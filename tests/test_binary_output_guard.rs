//! REQ-078 · **二进制层差分守卫** —— 断言「**判据收到的串 == 打印出去的串**」。
//!
//! # 这个文件补的是哪一格
//!
//! REQ-072 的 QA 用变异 **M2** 证明了一件事（`.review-workspace/req072qa/QA.md` §2）：
//! 把 `src/cli.rs` 里那行 `print!("{text}")` 改成打印前做一次 `str::replace`
//! （**domain 层一行未改**），实测：
//!
//! ```text
//! 全量 53 个测试二进制：passed=738 / failed=4 / ignored=1   ← 既有 738 条**全绿**
//! 退出码实测 = 0
//! stderr 完全为空 —— 三道自检在 stdout 已被攻破时**一声不吭**
//! ```
//!
//! 根因**不是疏忽，是接线方式的必然**：判据收的是 **domain 产物**，
//! 而打印出去的可以是它的**再加工**。两者不是同一个对象 ⇒
//! **打印点的任何加工，对自检结构性不可见。**
//!
//! ⚠️ **本条不承诺封堵这一类**（AC 的范围判断）。做到的是**让它可见**。
//!
//! # 四条守卫各管哪一格
//!
//! | 守卫 | AC | 杀谁 | 机制 |
//! | --- | --- | --- | --- |
//! | [`g1_criteria_receive_exactly_what_is_printed`] | **AC-001** ⭐ | M2（打印点被再加工） | 词法级读 `src/cli.rs`：写出表达式与判据实参**必须是同一个东西** |
//! | [`g2_prefix_is_registered_not_hardcoded_in_tests`] | **AC-002** | 前缀常量漂移 | 前缀的**唯一事实在 `src/cli.rs`**；`tests/**` 里出现前缀字面量即红 |
//! | [`g3_exit_code_policy_is_defined_and_asserted`] | **AC-004** | 「退出码 0 = 渲染没问题」的错觉 | 纯函数 + 源码接线 + 文字政策，三样都断言 |
//! | [`g4_guard_is_not_vacuous`] | **AC-003** ⭐ | **守卫自己恒真** | 判据函数抽成纯函数 ⇒ 用**构造出来的变异**先证明它能红 |
//! | [`g5_every_domain_product_print_site_is_registered`] | **AC-005** | 漏登记的打印点 | 登记表与源码**双向**对账 |
//!
//! # ⭐ AC-001 是灵魂：为什么判据必须是**源码级**的
//!
//! REQ-072 已经有「真实 stdout == 抬头 ++ domain 渲染」的端到端断言，
//! M2 下它**确实红了**。但那断言的是**结果**（stdout 是什么），
//! 不是**接线**（判据收到的是不是同一份东西）。
//!
//! 两者可以**一起漂移**：哪天有人「修 M2」时把端到端断言里的
//! `format!` 也改成带同样加工的形状，两边就一致了 —— 全绿，
//! 而**判据已经瞎了**。⇒ **接线必须是独立被断言的对象**。
//!
//! # 词法级读源码，不是解析 Rust
//!
//! 与 `tests/test_anchor_guard.rs` 同款：不引解析器依赖，
//! **先剥注释**（含可嵌套块注释 / raw string / 字符字面量 vs 生命周期），
//! 再按标识符**词元**匹配。⚠️ 用**词元**而不是子串 ——
//! `json_text` 里含有 `text`，子串匹配会把两个站点混成一个。

mod common;

use std::collections::BTreeMap;

use ontok::cli::{
    self_check_exit_code, DomainPrintSite, EXIT_LEAKED_MODELING_TERMS, EXIT_OK,
    EXIT_OPERATION_FAILED, EXIT_POLICY, EXIT_SELF_CHECK_VIOLATION, DOMAIN_PRINT_SITES,
    REVIEW_SHOW_PREFIX_TEMPLATE, WIRING_CLOSURE, WIRING_NONE, WIRING_SAME_BINDING,
};
use ontok::domain::diff::diff_snapshots;
use ontok::domain::review::{summarize, Decision, ItemKind, ItemView};
use ontok::domain::snapshot::{EntityType, Snapshot};
use uuid::Uuid;

/// 单引号的字符常量。
///
/// ⚠️ 刻意**不**写成 `'\''`：紧挨着 `))` 时 rustc 的词法器会把它吃成别的形状，
/// 报错长得像「缺一个 `(`」—— 与实际问题毫无关系的误导性报错。
const QUOTE: char = '\u{27}';

const CLI_SRC: &str = "src/cli.rs";
const MAIN_SRC: &str = "src/main.rs";

// ══════════════════════════════════════════════════════════════
// 词法底座（照抄 `tests/test_anchor_guard.rs` 的成熟实现）
// ══════════════════════════════════════════════════════════════

#[derive(Clone, Copy, PartialEq)]
enum Lex {
    Code,
    LineComment,
    /// 嵌套深度（Rust 的块注释**可嵌套**）
    BlockComment(usize),
    Str,
    /// raw string，`r#"…"#` 的井号个数
    RawStr(usize),
    Char,
}

/// **词法级**剥注释：只删注释，**保留字符串字面量**（它们是代码）。
///
/// 返回值**行数与行号与输入一一对应**（注释里的换行被保留），
/// 命中时才能报出准确的 `path:line`。
fn strip_comments(src: &str) -> Vec<String> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let (mut i, mut st) = (0usize, Lex::Code);

    while i < cs.len() {
        let c = cs[i];
        match st {
            Lex::Code => match c {
                '/' if cs.get(i + 1) == Some(&'/') => {
                    st = Lex::LineComment;
                    i += 2;
                }
                '/' if cs.get(i + 1) == Some(&'*') => {
                    st = Lex::BlockComment(1);
                    i += 2;
                }
                'r' => {
                    let mut j = i + 1;
                    let mut hashes = 0usize;
                    while cs.get(j) == Some(&'#') {
                        hashes += 1;
                        j += 1;
                    }
                    if cs.get(j) == Some(&'"') {
                        out.extend(cs[i..=j].iter());
                        i = j + 1;
                        st = Lex::RawStr(hashes);
                    } else {
                        out.push(c);
                        i += 1;
                    }
                }
                '"' => {
                    out.push(c);
                    i += 1;
                    st = Lex::Str;
                }
                QUOTE => {
                    let is_char = cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&QUOTE);
                    out.push(c);
                    i += 1;
                    if is_char {
                        st = Lex::Char;
                    }
                }
                _ => {
                    out.push(c);
                    i += 1;
                }
            },
            Lex::LineComment => {
                if c == '\n' {
                    out.push('\n');
                    st = Lex::Code;
                }
                i += 1;
            }
            Lex::BlockComment(depth) => {
                if c == '/' && cs.get(i + 1) == Some(&'*') {
                    out.push(' ');
                    out.push(' ');
                    st = Lex::BlockComment(depth + 1);
                    i += 2;
                } else if c == '*' && cs.get(i + 1) == Some(&'/') {
                    out.push(' ');
                    out.push(' ');
                    st = if depth <= 1 { Lex::Code } else { Lex::BlockComment(depth - 1) };
                    i += 2;
                } else {
                    if c == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
            Lex::Str => {
                if c == '\\' {
                    out.push(c);
                    if let Some(&n) = cs.get(i + 1) {
                        out.push(n);
                    }
                    i += 2;
                } else {
                    out.push(c);
                    if c == '"' {
                        st = Lex::Code;
                    }
                    i += 1;
                }
            }
            Lex::RawStr(hashes) => {
                if c == '"' && (0..hashes).all(|k| cs.get(i + 1 + k) == Some(&'#')) {
                    out.push(c);
                    for _ in 0..hashes {
                        out.push('#');
                    }
                    i += 1 + hashes;
                    st = Lex::Code;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            Lex::Char => {
                out.push(c);
                if c == '\\' {
                    if let Some(&n) = cs.get(i + 1) {
                        out.push(n);
                    }
                    i += 2;
                } else {
                    i += 1;
                    if c == QUOTE || c == '\n' {
                        st = Lex::Code;
                    }
                }
            }
        }
    }

    out.split('\n').map(|s| s.to_string()).collect()
}

// ══════════════════════════════════════════════════════════════
// 提取层（纯函数 —— G-4 拿构造输入直接打它们）
// ══════════════════════════════════════════════════════════════

/// 从 `expr` 里取出**标识符词元**（跳过字符串/字符字面量与 `&`）。
///
/// ⛔ **必须按词元而不是子串**：`json_text` 含 `text`、
/// `mv_json` 含 `json` —— 子串匹配会把两个站点混成一个，
/// 于是「这一格有没有被守住」就成了假问题。
fn ident_tokens(expr: &str) -> Vec<String> {
    let cs: Vec<char> = expr.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let (mut i, mut in_str, mut in_raw, mut hashes, mut in_char) = (0usize, false, false, 0usize, false);
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    while i < cs.len() {
        let c = cs[i];
        if in_raw {
            if c == '"' && (0..hashes).all(|k| cs.get(i + 1 + k) == Some(&'#')) {
                in_raw = false;
                i += 1 + hashes;
            } else {
                i += 1;
            }
            continue;
        }
        if in_str {
            if c == '\\' {
                i += 2;
            } else {
                if c == '"' {
                    in_str = false;
                }
                i += 1;
            }
            continue;
        }
        if in_char {
            if c == '\\' {
                i += 2;
            } else {
                if c == QUOTE {
                    in_char = false;
                }
                i += 1;
            }
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            cur.push(c);
            i += 1;
            continue;
        }
        flush(&mut cur, &mut out);
        if c == '"' {
            in_str = true;
            i += 1;
        } else if c == 'r' {
            // 落到这里说明 `r` 后面不是标识符字符 —— 再看是不是 raw string
            let mut j = i + 1;
            let mut h = 0usize;
            while cs.get(j) == Some(&'#') {
                h += 1;
                j += 1;
            }
            if cs.get(j) == Some(&'"') {
                in_raw = true;
                hashes = h;
                i = j + 1;
            } else {
                i += 1;
            }
        } else if c == QUOTE && (cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&QUOTE)) {
            in_char = true;
            i += 1;
        } else {
            i += 1;
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// `expr` 里**是否提到了** `path`（`page.html` 这种多段路径按连续词元匹配）。
fn mentions_path(expr: &str, path: &str) -> bool {
    let hay = ident_tokens(expr);
    let needle = ident_tokens(path);
    !needle.is_empty()
        && hay.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// `expr` **就是** `path` 本身（不多不少）—— 打印点没有对它做任何加工。
fn is_plain_path(expr: &str, path: &str) -> bool {
    ident_tokens(expr) == ident_tokens(path)
}

/// 从 `open`（指向 `(`）起找到配对的 `)` —— 跳过字符串与字符字面量。
fn match_paren(s: &[char], open: usize) -> Option<usize> {
    let (mut i, mut depth, mut in_str, mut in_char) = (open, 0i32, false, false);
    while i < s.len() {
        let c = s[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == QUOTE {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => i += 1,
            '"' => in_str = true,
            QUOTE if s.get(i + 1) == Some(&'\\') || s.get(i + 2) == Some(&QUOTE) => in_char = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// 在 `s`（切片）里从 `from` 起找下一个不在字面量里的 `ch`。
fn find_plain(s: &[char], from: usize, ch: char) -> Option<usize> {
    let (mut i, mut in_str, mut in_char) = (from, false, false);
    while i < s.len() {
        let c = s[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == QUOTE {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => i += 1,
            '"' => in_str = true,
            QUOTE if s.get(i + 1) == Some(&'\\') || s.get(i + 2) == Some(&QUOTE) => in_char = true,
            _ if c == ch => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// 把 `args` 按**顶层**逗号切成若干段（跳过字面量与嵌套括号）。
fn split_top_level(args: &str) -> Vec<String> {
    let cs: Vec<char> = args.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let (mut start, mut depth, mut in_str, mut in_char) = (0usize, 0i32, false, false);
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == QUOTE {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => i += 1,
            '"' => in_str = true,
            QUOTE if cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&QUOTE) => in_char = true,
            '(' | '{' | '[' => {
                depth += 1;
                i += 1;
            }
            ')' | '}' | ']' => {
                depth -= 1;
                i += 1;
            }
            ',' if depth == 0 => {
                parts.push(cs[start..i].iter().collect());
                start = i + 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    parts.push(cs[start..].iter().collect());
    parts.into_iter().map(|s| s.trim().to_string()).collect()
}

/// 一个闭包：`|参数名| 体`。
#[derive(Debug, Clone, PartialEq)]
struct Closure {
    param: String,
    body: String,
}

/// 从 `emit_domain_text_with(...)` 的实参里解析出闭包。
///
/// 解析不出来就返回 `None` —— 调用方据此**判红**，
/// 而不是「解析不了就当没有闭包」（那正是恒真的来源）。
fn parse_closure(args: &str) -> Option<Closure> {
    let cs: Vec<char> = args.chars().collect();
    let bar = find_plain(&cs, 0, '|')?;
    let bar2 = find_plain(&cs, bar + 1, '|')?;
    let param: String = cs[bar + 1..bar2].iter().collect::<String>().trim().to_string();
    if ident_tokens(&param).len() != 1 {
        return None;
    }
    let brace = cs[bar2 + 1..]
        .iter()
        .position(|c| *c == '{')
        .map(|p| bar2 + 1 + p)?;
    let brace = cs.iter().skip(brace).position(|c| *c == '{')? + brace;
    let mut depth = 0i32;
    let (mut i, mut in_str, mut in_char) = (brace, false, false);
    while i < cs.len() {
        let c = cs[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == QUOTE {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => i += 1,
            '"' => in_str = true,
            QUOTE if cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&QUOTE) => in_char = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(Closure {
                        param,
                        body: cs[brace + 1..i].iter().collect(),
                    });
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

// ══════════════════════════════════════════════════════════════
// 源码扫描层
// ══════════════════════════════════════════════════════════════

/// 会被判据当作「打印 domain 产物」的宏 / 函数。
///
/// ⛔ **这是一份枚举，不是判定** —— 与本仓既有的词表纪律一致：
/// 枚举会漏，但漏的方式是「漏登记的新站点**红**」（G-2/G-5），
/// 而不是「静默通过」。
const DOMAIN_PRODUCERS: &[&str] = &[
    "render_review_text",
    "render_provenance_line",
    "render_html",
    "render_overview_text",
    "build_overview",
    "project_model_track",
    "diff_snapshots",
    "summarize",
    "to_string_pretty",
    "summary",
];

/// 会把产物写向 stdout 的调用**开头**（含 `!` —— `print!`/`println!` 是宏）。
///
/// ⚠️ 这里写的是**带括号的整段**而不是函数名：`print!` 与 `print(` 差一个 `!`，
/// 漏掉它会让扫描**只认得** `emit_*`、一个 `println!` 都找不到 ——
/// 而那种失败是「全绿」，因为少认的那部分正好是不该判红的那部分。
const PRINT_CALLEES: &[&str] = &[
    "print!(",
    "println!(",
    "emit_domain_text(",
    "emit_domain_line(",
    "emit_domain_text_with(",
];

/// 一处打印调用的**位置**与**实参原文**。
#[derive(Debug, Clone)]
struct PrintCall {
    line: usize,
    /// 调用**结束**所在的行（跨行调用要靠它判「判据在不在闭包里」）。
    end_line: usize,
    callee: String,
    args: String,
}

/// 这个打印调用**写出去的那一份**是什么。
///
/// - `emit_domain_*` 系列 ⇒ **第一个实参**（唯一出口，产物就在那儿）；
/// - `print!` / `println!` ⇒ **整个实参表**（前缀与产物混在一张表里）。
fn emitted_expression(call: &PrintCall) -> String {
    if call.callee.starts_with("emit_domain") {
        split_top_level(&call.args)
            .into_iter()
            .next()
            .unwrap_or_default()
    } else {
        call.args.clone()
    }
}

/// 扫 `lines` 里的全部打印调用（按**括号配平**跨行拼装）。
fn scan_prints(lines: &[String]) -> Vec<PrintCall> {
    let mut out: Vec<PrintCall> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        for name in PRINT_CALLEES {
            // 词边界：左邻不得是标识符字符，右邻必须是 `(`。
            let mut from = 0usize;
            while let Some(rel) = line[from..].find(name) {
                let at = from + rel;
                let left_ok = at == 0
                    || !line[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_');
                from = at + 1;
                if !left_ok {
                    continue;
                }
                // 行内配平；跨行就往下拼。
                let mut buf = line[at..].to_string();
                let (lo, mut hi) = (li, li);
                let mut closed: Option<(usize, usize)> = None; // (open_idx, close_idx)
                loop {
                    let cs: Vec<char> = buf.chars().collect();
                    let Some(open_idx) = cs.iter().position(|c| *c == '(') else {
                        break;
                    };
                    match match_paren(&cs, open_idx) {
                        Some(close) => {
                            closed = Some((open_idx, close));
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
                let Some((open_idx, close)) = closed else { continue };
                let cs: Vec<char> = buf.chars().collect();
                out.push(PrintCall {
                    line: lo + 1,
                    end_line: hi + 1,
                    // 去掉结尾的 `(`：`callee` 只留**名字**（`print!` 保留 `!`）。
                    callee: name.trim_end_matches('(').to_string(),
                    args: cs[open_idx + 1..close].iter().collect(),
                });
                break;
            }
        }
    }
    out
}

/// 函数体在 `lines` 里的行区间（闭区间，0 基）。
fn fn_span(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let hit = format!("fn {name}(");
    let start = lines.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with(&hit) || t.starts_with(&format!("async {hit}"))
    })?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| !l.is_empty() && !l.starts_with(char::is_whitespace))
        .map(|(i, _)| i - 1)
        .unwrap_or(lines.len() - 1);
    Some((start, end))
}

fn read_cli() -> Vec<String> {
    strip_comments(&common::source_root::read_source(CLI_SRC))
}

// ══════════════════════════════════════════════════════════════
// G-1 · ⭐ AC-001 —— 判据收到的串 == 打印出去的串
// ══════════════════════════════════════════════════════════════

/// 写出表达式有没有在那个绑定**后面接方法调用**（`text.replace(..)` 那种）。
///
/// ⚠️ 只对 `print!`/`println!` 有意义：宏的实参里前缀与产物混在一张表里，
/// 整体不可能「就是那个绑定」。但**「在它后面接了 `.`」**在任何形态下都是加工。
fn chains_method_on(expr: &str, path: &str) -> bool {
    expr.contains(&format!("{path}."))
}

// ───── REQ-080 F-2 · `same_binding` 族的第二个判别面：重新绑定 ─────
//
// ⛔ 词元相等**单独不足以**判红。REQ-080 QA 的 M2b（实测）构造：
//
// ```text
// let leaked = scan_modeling_terms(&json);   // 判据：未加工
// let json  = json.replace('\n', "");        // ← shadowing，两边仍都叫 `json`
// crate::cli::emit_domain_line(&json);       // 写出：已加工
// ```
//
// `is_plain_path(&json, "json")` 与判据实参的词元比较**双双通过** ⇒
// REQ-078 那五条全绿。真正判红的是端到端字节比对 ——
// 也就是说**防线全落在 e2e 上，e2e 一旦被放松就是零防线**。
//
// ⇒ 这里补的那一格问的是**语义**：「判据读到的那份绑定，在判据与写出之间
// 有没有被重新绑定过」。判据读的是**判据那一行**解析到的绑定，
// 写出读的是**写出那一行**解析到的绑定；两者只要不是同一个绑定，
// 就不可能拿到同一份东西。
//
// # ⚠️ 这一格的**判别面**有多大 —— 已实测，不是推测（REQ-080 QA 终验 N-1）
//
// 这一格**只认 `let` 引入的重绑定**。它**不是穷尽的**，QA 用三个绕过实测过，
// 三个**全部编译通过**（`grep -c '^error'` = 0）且 **G-1 全绿**：
//
// | 绕过 | 构造 | G-1 | e2e |
// | --- | --- | --- | --- |
// | **A** | `let mut json` + **后续赋值**（整个窗口里没有 `let` 词元） | 🟢 7 passed | 🔴 FAILED |
// | **B** | `for json in [..] {` 体内**就是**写出点 | 🟢 7 passed | 🔴 FAILED |
// | **C** | `match` 分支体内重绑定（另一个站点） | 🟢 7 passed | — |
//
// ⚠️ **「同一深度」这个限定救不了 B / C**：`let` 与写出点**就在同一块内**，
// 深度相同是它们的**常态**而不是异常。
//
// ⇒ 我原先在这里写的「`let` 是**穷尽**的：`for x in ..`、`match` 分支、闭包参数的
// 绑定只活在自己的块里，影响不到后面的写出点」**已被证伪**：
// 它只覆盖了「写出点在那个块**之后**」的情形，漏掉了「写出点**在那个块里**」。
//
// ⇒ 所以这一格的诚实表述是：**它抓住「重新 `let`」那一族**（QA 判定它有用、
// 报错定位精确），**抓不住「同一块内、靠别的方式换掉绑定」那一族**。
// 后者仍然只有 e2e 兜着 —— 与 4 个站点 `note` 里登记的边界**一致**，不要改口。

/// 这一行在**字面量之外**净增/净减几个花括号。
fn line_brace_delta(line: &str) -> i32 {
    let cs: Vec<char> = line.chars().collect();
    let (mut i, mut depth, mut in_str, mut in_char) = (0usize, 0i32, false, false);
    while i < cs.len() {
        let c = cs[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == QUOTE {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => i += 1,
            '"' => in_str = true,
            QUOTE if cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&QUOTE) => in_char = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    depth
}

/// 从 `lo`（0 基）累加到 `idx`（0 基，闭区间）为止的**块深度**。
fn block_depth_upto(lines: &[String], lo: usize, idx: usize) -> i32 {
    (lo..=idx.min(lines.len().saturating_sub(1)))
        .map(|i| line_brace_delta(&lines[i]))
        .sum()
}

/// `line` 是不是「用 `let` 把 `root` 这个名字重新绑定了一次」。
///
/// `page.html` 的 `root` 是 `page` —— Rust 里改不了字段本身，
/// 换了 `page` 就等于换了那份东西。
fn lets_rebind_root(line: &str, root: &str) -> bool {
    let t = ident_tokens(line);
    t.windows(2).any(|w| w[0] == "let" && w[1] == root)
        || t.windows(3).any(|w| w[0] == "let" && w[1] == "mut" && w[2] == root)
}

/// `binding` 的**根标识符**（`page.html` → `page`）。
fn binding_root(binding: &str) -> String {
    ident_tokens(binding)
        .first()
        .cloned()
        .unwrap_or_else(|| binding.to_string())
}

/// ⭐ F-2 · 判据与写出之间有没有把那个绑定**重新 `let` 过一次**。
///
/// 返回 `Some(违规行号)`。只看 `(判据行, 写出行)` **开区间**内、
/// 且**块深度与判据行相同**的 `let`：
///
/// - 开区间 ⇒ 判据**之前**的重绑定不影响它（判据读到的就是重绑定后那一份，
///   与写出同一份，这是正确的），判据**之后**的重绑定才切断「同一份」；
/// - 同深度 ⇒ 嵌在别的块里的同名 `let` 影响不到后面的写出点，不该报。
fn rebound_between(
    lines: &[String],
    fn_lo: usize,
    judge_line: usize,
    print_line: usize,
    binding: &str,
) -> Option<usize> {
    let root = binding_root(binding);
    let jd = block_depth_upto(lines, fn_lo, judge_line - 1);
    let mut depth = jd;
    for i in (judge_line)..(print_line - 1) {
        // `judge_line` / `print_line` 都是 1 基，`i` 取的是它们之间的 0 基下标。
        if lets_rebind_root(&lines[i], &root) && depth == jd {
            return Some(i + 1);
        }
        depth += line_brace_delta(&lines[i]);
    }
    None
}

/// 把一个站点该有的**形状**核一遍，返回违规原因（`None` = 通过）。
///
/// 抽成**纯函数**是 AC-003 的前提：G-4 拿构造出来的变异直接打它。
fn check_site(lines: &[String], site: &DomainPrintSite) -> Option<String> {
    let (lo, hi) = fn_span(lines, site.enclosing_fn)
        .unwrap_or_else(|| panic!("`{CLI_SRC}` 里找不到函数 `{}`", site.enclosing_fn));

    let prints: Vec<PrintCall> = scan_prints(lines)
        .into_iter()
        .filter(|c| c.line >= lo + 1 && c.line <= hi + 1)
        .filter(|c| mentions_path(&emitted_expression(c), site.binding))
        .collect();

    if prints.is_empty() {
        return Some(format!(
            "`{}` 里找不到把 `{}` 写出去的打印点 —— 登记表与源码已经漂移，\
             这一格**根本没被守住**",
            site.enclosing_fn, site.binding
        ));
    }

    for p in &prints {
        let emitted = emitted_expression(p);
        if p.callee.starts_with("emit_domain") {
            // ① 走唯一出口的：**写出表达式必须就是那个绑定**（前缀在登记表里，不在这里）。
            //    ⭐ 这一条就是 REQ-072 QA 的 M2：
            //    `emit_domain_text_with(&text.replace('\u{240A}', "\n"), …)` 在这里红。
            if !is_plain_path(&emitted, site.binding) {
                return Some(format!(
                    "{}:{} —— 写出去的**不是** `{}` 本身，而是 `{emitted}`。\
                     打印点对 domain 产物做了加工 ⇒ 判据看不见（这正是 M2 的形状）",
                    CLI_SRC, p.line, site.binding
                ));
            }
        } else if chains_method_on(&emitted, site.binding) {
            return Some(format!(
                "{}:{} —— 打印点对 `{}` 接了方法调用（`{emitted}`）—— \
                 那是在写出之前**再加工**，判据看不见",
                CLI_SRC, p.line, site.binding
            ));
        }

        // ② 接线形态必须与登记表一致 ——
        //    `closure` 靠**类型**保证（闭包只拿得到刚写出去的那个切片）；
        //    `same_binding` 靠**逐词元相等**保证（fail-closed 站点必须先判后写，
        //    判据因而不在闭包里）。形态错了要么退化成约定，要么守卫验的东西变了。
        let want_closure = site.wiring == WIRING_CLOSURE;
        if want_closure && p.callee != "emit_domain_text_with" {
            return Some(format!(
                "{}:{} —— 登记表写的是 `{WIRING_CLOSURE}` 接线（类型保证），\
                 但写出走的是 `{}()`；「同一份」就只剩约定，不剩类型",
                CLI_SRC, p.line, p.callee
            ));
        }
        if !want_closure && p.callee == "emit_domain_text_with" {
            return Some(format!(
                "{}:{} —— 登记表写的是 `{}` 接线，却多包了一层闭包 —— \
                 接线形态与登记不符，守卫正在验的东西与生产实际做的不一致",
                CLI_SRC, p.line, site.wiring
            ));
        }
    }

    // 判据调用：**按它收到的那个绑定**归属到站点。
    // ⚠️ 不能按「函数里出现的全部判据调用」归属 —— `ontology_cmd` 里
    // 三条站点共用一个函数，否则 `scan_modeling_terms(&page.html)`
    // 会被算成 `overview` 的判据，而它压根不判那个东西。
    let judge_calls: Vec<(String, String, usize)> = lines
        .iter()
        .enumerate()
        .filter(|(i, _)| *i >= lo && *i <= hi)
        .flat_map(|(i, l)| {
            site.judged_by
                .iter()
                .filter_map(move |judge| {
                    let at = l.find(&format!("{judge}("))?;
                    let cs: Vec<char> = l.chars().collect();
                    let close = match_paren(&cs, at + judge.len())?;
                    let raw: String = cs[at + judge.len() + 1..close].iter().collect();
                    // ⭐ 取**最后一个**顶层实参：那才是「渲染文本」这一格。
                    // 第一个实参是投影（`&rv` / `&cv`）—— 判据要两样都收，
                    // 而「判据收到的那份**文本**」必须就是写出表达式本身。
                    let text_arg = split_top_level(&raw).pop().unwrap_or_default();
                    Some(((*judge).to_string(), text_arg, i + 1))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    if site.judged_by.is_empty() {
        if site.wiring != WIRING_NONE {
            return Some(format!(
                "[{}] 没有判据却登记了 `{WIRING_NONE}` 之外的接线形态 `{}`",
                site.command, site.wiring
            ));
        }
        return None;
    }

    // `closure` 形：判据实参 == 闭包参数，闭包体里不碰外层绑定，
    //             且判据**必须就在那个闭包里**。
    // `same_binding` 形：判据的文本实参 == 写出表达式（逐词元相等）。
    let mut expected = site.binding.to_string();
    let mut enclosing: Option<(usize, usize)> = None;
    if site.wiring == WIRING_CLOSURE {
        let Some(cl) = prints
            .iter()
            .find(|p| p.callee == "emit_domain_text_with")
            .and_then(|p| parse_closure(&p.args))
        else {
            return Some(format!(
                "`{}` 里 `emit_domain_text_with` 的实参解析不出闭包 —— \
                 守卫**读不懂**那条接线（不是「没有接线」）",
                site.enclosing_fn
            ));
        };
        if mentions_path(&cl.body, site.binding) {
            return Some(format!(
                "`{}` 里 `emit_domain_text_with` 的**闭包体内又提到了** `{}`。\
                 闭包本来只拿得到刚写出去的那一份；它去读外层绑定，\
                 「同一份」就退化成了约定",
                site.enclosing_fn, site.binding
            ));
        }
        expected = cl.param.clone();
        enclosing = prints
            .iter()
            .find(|p| p.callee == "emit_domain_text_with")
            .map(|p| (p.line, p.end_line));
    }

    let judge_calls: Vec<_> = judge_calls
        .into_iter()
        .filter(|(_, arg, _)| mentions_path(arg, &expected))
        .collect();

    if judge_calls.is_empty() {
        return Some(format!(
            "`{}` 里找不到判据 {:?} 收到 `{}` —— 判据这一格是空的，\
             「判据收到的 == 打印出去的」这句话**无从谈起**",
            site.enclosing_fn, site.judged_by, expected
        ));
    }

    for (judge, arg, line) in &judge_calls {
        if !is_plain_path(arg, &expected) {
            return Some(format!(
                "{}:{} —— 判据 `{judge}` 收到的文本实参是 `{arg}`，\
                 不是写出去的那一份（`{expected}`）。判据与打印各拿各的东西 ⇒ \
                 打印点的加工对它结构性不可见",
                CLI_SRC, line
            ));
        }
        if let Some((lo, hi)) = enclosing {
            if *line < lo || *line > hi {
                return Some(format!(
                    "{}:{} —— 判据 `{judge}` 在 `emit_domain_text_with` 的闭包**之外**\
                     （闭包在 {}–{} 行）。挪出去就等于让它重新拿回外层那个 `{}`",
                    CLI_SRC, line, lo, hi, site.binding
                ));
            }
        }
        // ③′ ⭐ REQ-080 F-2 · `same_binding` 族：词元相等**只是必要条件**。
        //     `closure` 族不需要这一格 —— 判据只拿得到闭包参数，
        //     而闭包参数就是刚写出去的那一份（外层 `let` 在它之前还是之后
        //     都改变不了这一点），所以那里换成 `enclosing.is_none()`。
        if enclosing.is_none() {
            for p in prints.iter().filter(|p| p.line > *line) {
                if let Some(at) = rebound_between(lines, lo, *line, p.line, &expected) {
                    return Some(format!(
                        "{}:{} —— `{}` 在判据（{CLI_SRC}:{} 读的是**这一份** `{}`）\
                         与写出（{CLI_SRC}:{} 读的是**那一份**）之间被 `let` **重新绑定**过。\
                         两边仍都叫 `{}` ⇒ 逐词元相等**看不出来** —— \
                         这正是 M2b 的形状（G-4 对四个 `same_binding` 站点逐个构造过）",
                        CLI_SRC, at, expected, line, expected, p.line, expected
                    ));
                }
            }
        }
    }
    None
}

/// ⛔ **孤儿判据**：收到的东西既不是任何登记项的绑定 ⇒ 没有站点认领它。
///
/// 这一条是 `same_binding` 那一族里**唯一非自指的**检查 ——
/// 其余断言都是「绑定 == 绑定」，而这条问的是「有没有判据判了一个没人写出去的东西」
/// （或反之：写出去了却没有判据认领）。
fn orphan_judgements(lines: &[String]) -> Vec<String> {
    // 每个函数里「被判据认领的表达式」全集：
    // `closure` 形认领**闭包参数**，`same_binding` 形认领**绑定本身**。
    let claimed_by_fn: BTreeMap<&str, Vec<String>> = DOMAIN_PRINT_SITES
        .iter()
        .fold(BTreeMap::new(), |mut m, s| {
            let v = m.entry(s.enclosing_fn).or_insert_with(Vec::new);
            if s.wiring == WIRING_CLOSURE {
                let (lo, hi) = fn_span(lines, s.enclosing_fn).unwrap_or((0, 0));
                if let Some(p) = scan_prints(lines)
                    .into_iter()
                    .find(|p| {
                        p.callee == "emit_domain_text_with"
                            && p.line >= lo + 1
                            && p.line <= hi + 1
                            && mentions_path(&emitted_expression(p), s.binding)
                    })
                    .and_then(|p| parse_closure(&p.args))
                {
                    v.push(p.param);
                }
            } else {
                v.push(s.binding.to_string());
            }
            m
        });

    let mut out = Vec::new();
    for site in DOMAIN_PRINT_SITES {
        let Some((lo, hi)) = fn_span(lines, site.enclosing_fn) else {
            continue;
        };
        for (i, l) in lines.iter().enumerate().take(hi + 1).skip(lo) {
            for judge in site.judged_by {
                let Some(at) = l.find(&format!("{judge}(")) else { continue };
                let cs: Vec<char> = l.chars().collect();
                let Some(close) = match_paren(&cs, at + judge.len()) else { continue };
                let raw: String = cs[at + judge.len() + 1..close].iter().collect();
                let arg = split_top_level(&raw).pop().unwrap_or_default();
                let claimed = claimed_by_fn
                    .get(site.enclosing_fn)
                    .map(|v| v.iter().any(|p| is_plain_path(&arg, p)))
                    .unwrap_or(false);
                if !claimed {
                    out.push(format!(
                        "  - {CLI_SRC}:{} `{judge}` 收到 `{arg}`，\
                         `{}` 里没有任何登记项会写出它 —— 判据判了一个没人写出去的东西",
                        i + 1, site.enclosing_fn
                    ));
                }
            }
        }
    }
    out
}

#[test]
fn g1_criteria_receive_exactly_what_is_printed() {
    let lines = read_cli();

    // ── 自检：先证明判据不是空转（G-4 再单独把这条钉一遍）──
    assert!(
        check_site(&lines, &DOMAIN_PRINT_SITES[0]).is_none(),
        "登记表第 0 项在**当前源码**上就不通过：{:?}\n\
         ⇒ 下面的红可能全是构造问题，不是生产问题，先看这条",
        check_site(&lines, &DOMAIN_PRINT_SITES[0])
    );

    let mut bad = Vec::new();
    for site in DOMAIN_PRINT_SITES {
        if let Some(why) = check_site(&lines, site) {
            bad.push(format!("  - [{}] {why}", site.command));
        }
    }
    let orphans = orphan_judgements(&lines);
    if !orphans.is_empty() {
        bad.push(format!("孤儿判据：\n{}", orphans.join("\n")));
    }

    assert!(
        bad.is_empty(),
        "AC-001 未达成：{} 个站点上「判据收到的串 ≠ 打印出去的串」（或读不懂接线）：\n{}",
        bad.len(),
        bad.join("\n")
    );
}

// ══════════════════════════════════════════════════════════════
// G-2 · AC-002 —— 前缀必须进**显式登记表**，不许硬写在测试里
// ══════════════════════════════════════════════════════════════

#[test]
fn g2_prefix_is_registered_not_hardcoded_in_tests() {
    // ⭐ 前缀的**唯一事实在生产侧**（`REVIEW_SHOW_PREFIX_TEMPLATE`）。
    assert!(
        REVIEW_SHOW_PREFIX_TEMPLATE.starts_with("提案 ") && REVIEW_SHOW_PREFIX_TEMPLATE.contains("状态="),
        "登记表里的前缀模板不该是这个形状：{REVIEW_SHOW_PREFIX_TEMPLATE:?}"
    );
    assert!(
        DOMAIN_PRINT_SITES
            .iter()
            .any(|s| s.prefix_template == REVIEW_SHOW_PREFIX_TEMPLATE),
        "登记表里必须有一条以 `REVIEW_SHOW_PREFIX_TEMPLATE` 为前缀的登记项"
    );

    // ⛔ 反向：前缀的**字面原文不得出现在任何测试里** ——
    //    REQ-072 的做法正是 `format!("提案 {alias} 状态={status}\n{domain_text}")`，
    //    常量散落在两处就会漂移（生产改抬头、测试忘了改 ⇒ 假红；反过来 ⇒ 假绿）。
    // 抬头前半段（到第一个占位符为止）与后半段（从 `{status` 之前那一段起）。
    let (head, tail) = {
        let t = REVIEW_SHOW_PREFIX_TEMPLATE;
        let (head, rest) = t.split_once('{').expect("模板里至少有一个占位符");
        let (_, tail) = rest.rsplit_once('=').expect("模板里至少有一个 `{x}=` 形态的占位符");
        // ⚠️ 探针要的是**源码里的字面写法**：模板的运行值里那个换行是真换行，
        //    而测试文件里写的是 `\` + `n`。不还原就永远匹配不上 ——
        //    那是「判据恒假 ⇒ 永远绿」，本条 AC-003 要治的正是它。
        (rust_source_literal(head), format!("={}", rust_source_literal(tail)))
    };

    let mut offenders = Vec::new();
    for f in collect_test_rs() {
        let Ok(raw) = std::fs::read_to_string(&f) else {
            continue;
        };
        for (i, line) in strip_comments(&raw).iter().enumerate() {
            // ⛔ 判据要能认出**写在 `format!` 里**的那一档 ——
            //    只查「行首就是抬头」会漏掉 `let expected = format!("…{alias} …")`，
            //    而那正是 REQ-072 原来的写法。
            //
            // ⚠️ 探针**从登记表派生**而不是在本文件里写死：写死的话
            // 本判据自己就带着前缀原文，G-2 会先把自己判红。
            if line.contains(head.as_str()) && line.contains(tail.as_str()) && line.contains('{') {
                offenders.push(format!("{}:{}  {}", f.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "前缀原文被硬写进了测试（AC-002 ⛔）：\n  - {}\n\
         请改用 `ontok::cli::review_show_prefix(..)` / `REVIEW_SHOW_PREFIX_TEMPLATE`",
        offenders.join("\n  - ")
    );

    // 登记项自己的前缀必须**真的**出现在源码里（防止登记了一个不存在的模板）。
    let cli = read_cli().join("\n");
    for site in DOMAIN_PRINT_SITES {
        if site.prefix_template.is_empty() {
            continue;
        }
        // ⚠️ 比的是**源码里的字面写法**：`REVIEW_SHOW_PREFIX_TEMPLATE` 的**运行值**
        // 里那个 `\n` 是**真换行**，而 `src/cli.rs` 里写的是两个字符 `\` + `n`。
        // 直接拿运行值去 `contains` 会永远红 —— 那是「预期颜色与实测颜色矛盾」，
        // 先怀疑自己的构造（`dispatch/rules.md` 纪律 ③）。
        assert!(
            cli.contains(&rust_source_literal(site.prefix_template)),
            "[{}] 登记的前缀 {:?} 在 {CLI_SRC} 里根本不存在",
            site.command,
            site.prefix_template
        );
    }
}

/// 把一个字符串还原成它在 Rust 源码里的**字面写法**（`\n`、`\\`、`\"`）。
///
/// ⚠️ 为什么需要它：登记表的 `prefix_template` 是**运行值**（换行是真换行），
/// 而 `src/cli.rs` 里写的是**字面量**（`\` + `n` 两个字符）。
/// 两者直接 `contains` 必然对不上，而那个红会指向完全错误的方向。
fn rust_source_literal(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out
}

fn collect_test_rs() -> Vec<std::path::PathBuf> {
    let mut acc = Vec::new();
    fn walk(dir: &std::path::Path, acc: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        items.sort();
        for p in items {
            if p.is_dir() {
                walk(&p, acc);
            } else if p.extension().is_some_and(|e| e == "rs") {
                acc.push(p);
            }
        }
    }
    walk(&common::source_root::crate_root().join("tests"), &mut acc);
    acc
}

// ══════════════════════════════════════════════════════════════
// G-3 · AC-004 —— 退出码政策必须落成**断言**
// ══════════════════════════════════════════════════════════════

#[test]
fn g3_exit_code_policy_is_defined_and_asserted() {
    // ① 纯函数：空 ⇒ 不改退出码；有违规 ⇒ 必须**非零**。
    assert_eq!(
        self_check_exit_code(&[]),
        None,
        "没有违规时不得改退出码（照常 0）"
    );
    assert_eq!(
        self_check_exit_code(&["两轨分界自检：伪造行".to_string()]),
        Some(EXIT_SELF_CHECK_VIOLATION),
        "有自检违规时退出码必须落成 {EXIT_SELF_CHECK_VIOLATION}"
    );
    // 多条也一样 —— 不能因为「多于一条」走别的分支。
    assert_eq!(
        self_check_exit_code(&["a".to_string(), "b".to_string()]),
        Some(EXIT_SELF_CHECK_VIOLATION)
    );

    // ② 几个码必须互不相同，且自检码非零。
    assert_ne!(EXIT_SELF_CHECK_VIOLATION, EXIT_OK);
    assert_ne!(EXIT_SELF_CHECK_VIOLATION, EXIT_OPERATION_FAILED);
    assert_ne!(EXIT_SELF_CHECK_VIOLATION, EXIT_LEAKED_MODELING_TERMS);
    assert_eq!(EXIT_OK, 0);

    // ③ 政策**必须有文字**，且文字里点名了那两个码。
    //    AC-004 的原话是「落成断言**或**明确文字」—— 这里两样都要。
    assert!(
        EXIT_POLICY.contains("退出码政策"),
        "退出码政策得有可引用的抬头"
    );
    assert!(
        EXIT_POLICY.contains(&EXIT_SELF_CHECK_VIOLATION.to_string()),
        "政策文字里必须点名退出码 {EXIT_SELF_CHECK_VIOLATION}"
    );
    assert!(
        EXIT_POLICY.contains("渲染"),
        "政策文字必须说清 0 承载的**渲染**含义"
    );

    // ④ ⭐ 接线：政策不能只停在常量上，`review show` 必须真的调它。
    let cli = read_cli().join("\n");
    assert!(
        cli.contains("self_check_exit_code(&violations)"),
        "{CLI_SRC} 里 `review show` 没有把自检结果交给退出码政策 —— \
         政策写在天上、代码不执行它"
    );
    assert!(
        cli.contains("std::process::exit(code)"),
        "{CLI_SRC} 里自检违规后没有按政策退出"
    );
    assert!(
        cli.contains("EXIT_LEAKED_MODELING_TERMS"),
        "{CLI_SRC} 仍在用裸 `exit(2)` 记泄露 —— 政策与接线会漂"
    );

    // ⑤ 操作性错误仍走 `main.rs` 的 1。
    let main = strip_comments(&common::source_root::read_source(MAIN_SRC)).join("\n");
    assert!(
        main.contains("std::process::exit(1)"),
        "{MAIN_SRC} 应当仍以 1 表示操作性错误"
    );

    // ⑥ ⭐ REQ-080 F-1 · 退出码 0 那一行**不得**声称 stdout 等于渲染。
    //    真 M2（M2d：闭包捕获外层绑定）下退出码实测就是 0，而 stdout 已被攻破 ——
    //    「0 = 成功，且 stdout 逐字节 = 前缀 ++ 渲染」那句话在该场景下为假。
    let zero_line = exit_policy_zero_line(EXIT_POLICY)
        .expect("政策文字里必须有 `0 = …` 那一行 —— 政策不点 0 号码就等于没写 0 的含义");
    assert!(
        !zero_line_claims_render_identity(zero_line),
        "退出码政策把「stdout == 渲染」挂在了 0 号码上，而那一格**不由退出码守**：\
         \n  真 M2（闭包捕获外层绑定）下三道自检合法沉默 ⇒ 退出码实测 0、stdout 已被攻破。\n  \
         0 那一行只许说它真的判到的东西（有没有自检违规）。\n  当前 0 行：{zero_line}"
    );
}

/// `EXIT_POLICY` 里 **`0 = …` 那一行**（1 基行号无关，只取原文）。
fn exit_policy_zero_line(policy: &str) -> Option<&str> {
    policy.lines().map(str::trim).find(|l| {
        l.starts_with("0")
            && l[1..].trim_start().starts_with('=')
    })
}

/// `0 = …` 那一行有没有把「stdout 逐字节等于 domain 渲染」当成 0 承载的含义。
///
/// ⛔ 只看**那一行**：限定语（`⛔ 0 不承载…`）写在别的行是**正确**的写法，
/// 全篇扫一遍会把诚实的限定语一起判红 —— 那是判据恒假。
fn zero_line_claims_render_identity(zero_line: &str) -> bool {
    zero_line.contains("逐字节") || zero_line.contains("++") || zero_line.contains("渲染")
}

// ══════════════════════════════════════════════════════════════
// G-4 · ⭐ AC-003 —— 守卫**自带反空转自检**
// ══════════════════════════════════════════════════════════════

#[test]
fn g4_guard_is_not_vacuous() {
    // ⚠️ 本会话最高发的形态：**恒假 ⇒ 永远绿 ⇒ 看起来像防护其实没有。**
    // 所以每一条判据都先拿**构造出来的**正负样本打一遍。
    // 照抄 `tests/test_anchor_guard.rs:495-534` 的 `hit()` 自检块。

    // ── 纯函数 ①：`is_plain_path` —— 「写出去的是不是那个绑定本身」──
    assert!(
        is_plain_path("&text", "text"),
        "⛔ 直白写法应判为「就是它」"
    );
    assert!(
        !is_plain_path("&text.replace('\\u{240A}', \"\\n\")", "text"),
        "🔴 **M2 本体**必须被判为「不是它」—— 打印点做了加工"
    );
    assert!(
        !is_plain_path("&format!(\"{text}\")", "text"),
        "🔴 包一层 format! 也是加工"
    );
    assert!(
        !is_plain_path("&shown", "text"),
        "🔴 换个变量名也该判为「不是它」"
    );

    // ── 纯函数 ②：`mentions_path` —— 必须按**词元**而不是子串 ──
    assert!(
        !mentions_path("&json_text", "json"),
        "🔴 `json_text` 不该被当成 `json`（子串匹配的经典误判）"
    );
    assert!(mentions_path("&page.html", "page.html"), "多段路径要能匹配");
    assert!(!mentions_path("&page.html_extra", "page.html"), "前缀相同不算");

    // ── 纯函数 ③：`parse_closure` —— 闭包必须**认得出来** ──
    let good = parse_closure("&text, |judged| { verify(&rv, judged) }");
    assert_eq!(
        good,
        Some(Closure {
            param: "judged".into(),
            body: " verify(&rv, judged) ".into()
        }),
        "⛔ 正常形状的闭包必须解析得出参数名与体"
    );
    assert_eq!(
        parse_closure("&text, |a, b| { x }").map(|c| c.param),
        None,
        "🔴 两个参数不是本守卫认得的形状 —— 必须**解析不出**，而不是猜一个"
    );

    // ⭐ 最关键的一条：**拿 M2 的真实形状打整条守卫**。
    // 这一段是本文件存在的理由 —— 若它不红，G-1 就是注释。
    let site = DOMAIN_PRINT_SITES
        .iter()
        .find(|s| s.command == "review show")
        .expect("登记表里必须有 review show");
    let mut mutated = read_cli();
    let victim = mutated
        .iter()
        .position(|l| l.contains("let violations = crate::cli::emit_domain_text_with(&text"))
        .expect("源码里找不到 review show 的接线行 —— 守卫可能已失效");
    mutated[victim] = r#"            let violations = crate::cli::emit_domain_text_with(&text.replace('\u{240A}', "\n"), |judged| {"#.to_string();

    let verdict = check_site(&mutated, site);
    assert!(
        verdict.is_some(),
        "🔴🔴 **M2 变异在守卫下全绿** ⇒ AC-003 未达成，这条守卫不是防线。\n\
         变异：把 `emit_domain_text_with` 的实参换成 `&text.replace('\\u{{240A}}', \"\\n\")`"
    );

    // 另一侧：让**判据**去读加工过的串（打印点干净、判据点被换掉）。
    let mut mutated2 = read_cli();
    let v2 = mutated2
        .iter()
        .position(|l| l.contains("verify_track_separation(&rv, judged)"))
        .expect("找不到判据调用行");
    mutated2[v2] =
        "                for v in crate::domain::review_view::verify_track_separation(&rv, &text) {"
            .to_string();
    assert!(
        check_site(&mutated2, site).is_some(),
        "🔴🔴 判据改回读外层绑定 `&text` 居然全绿 ⇒ G-1 认不出这一侧"
    );

    // 反空转的**反向**：判据在**真实源码**上至少要认得出每一个登记项 ——
    // 一条都认不出，就说明整套扫描在自己的输入上是瞎的。
    let lines = read_cli();
    assert!(
        lines.iter().any(|l| l.contains("emit_domain_text_with(&text")),
        "判据连当前源码里那条接线都找不到 —— 扫描本身失效了"
    );

    // ── ⭐ REQ-080 F-1 · 退出码政策那一格也不能是空壳 ──
    //    旧政策把「stdout == 渲染」挂在 0 号码上，而真 M2 下退出码实测 0。
    //    这里证明**新判据能红**：拿 REQ-078 那一版 0 行的原文去打它。
    assert!(
        zero_line_claims_render_identity(
            "0 = 成功，且 stdout 逐字节 = 已登记前缀 ++ domain 渲染，且三道自检沉默"
        ),
        "🔴 **「0 承载 stdout == 渲染」这个断言判不出来** ⇒ G-3 ⑥ 恒绿 = 不是防线。\n\
         REQ-080 F-1 实测：真 M2（闭包捕获外层绑定）下退出码是 0，而 stdout 已被攻破。"
    );
    assert!(
        !zero_line_claims_render_identity(
            "0 = 1 / 2 / 3 都没触发：三道自检从**它们读到的那份串**上没报出违规"
        ),
        "🔴 如实改写后的 0 行被判成了「仍在声称 stdout == 渲染」⇒ 判据恒红 = 恒假"
    );
    // 提取器本身也要验：不能「随便抓一行都算 0 行」（那会让 ⑥ 恒绿）。
    assert_eq!(
        exit_policy_zero_line("退出码政策：\n  3 = 三道自检报出了东西\n"),
        None,
        "🔴 政策里没有 0 那一行时，提取器却抓到了东西 ⇒ 提取器恒真"
    );
    assert_eq!(
        exit_policy_zero_line("  0 = 没触发\n"),
        Some("0 = 没触发"),
        "🔴 提取器认不出带缩进的 0 行"
    );

    // ── ⭐ REQ-080 F-2 · `same_binding` 族的 M2b：**逐个站点**构造 ──
    //
    // REQ-080 QA 实测：这一族在 shadowing 下 REQ-078 的五条**全绿**，
    // 真正判红的是端到端字节比对 ⇒ 防线全落在 e2e 上，e2e 一旦被放松就是零防线。
    // 现在对**每一个** `same_binding` 站点都插一次 shadowing，
    // 证明 `check_site` 在它身上**真的会红**（⛔ 不是「e2e 抓到了」）。
    let same_binding: Vec<&DomainPrintSite> = DOMAIN_PRINT_SITES
        .iter()
        .filter(|s| s.wiring == WIRING_SAME_BINDING)
        .collect();
    assert_eq!(
        same_binding.len(),
        4,
        "`same_binding` 族已经不是 4 个站点了 —— 下面那 4 段逐站变异必须跟着改，\
         别让它们悄悄缩成 0 段（0 段 = 这一格恒真 = 不是防线）"
    );
    for site in same_binding {
        let base = read_cli();
        let jl = judge_line_for(&base, site).unwrap_or_else(|| {
            panic!(
                "[{}] 在 `{CLI_SRC}` 里找不到判据读到 `{}` 的那一行 —— 变异无从下手",
                site.command, site.binding
            )
        });
        // 反空转的另一侧：**未变异**的源码上必须仍然绿。
        assert!(
            check_site(&base, site).is_none(),
            "🔴 [{}] 在**未变异**的真实源码上就红 ⇒ 下面那次变异红得没有意义",
            site.command
        );
        let mut mutated = base.clone();
        // 插在判据之后、写出之前：shadowing 之后**两边仍都叫同一个名字**
        // ⇒ `is_plain_path` / 逐词元比较全部照样通过，只有新加的那一格能看见。
        //
        // ⚠️ 左边是**根**标识符（`page.html` ⇒ `page`）——
        //    Rust 里改不了字段本身，绑了新的 `page` 就等于换了那份东西；
        //    写成 `let page.html = ..` 那不是合法 Rust（也不构成任何重绑定）。
        let root = binding_root(site.binding);
        mutated.insert(
            jl,
            format!(
                "{}let {root} = {}.replace('\\n', \"\");",
                " ".repeat(16),
                site.binding
            ),
        );
        let verdict = check_site(&mutated, site).unwrap_or_else(|| {
            panic!(
                "🔴🔴 **M2b 变异在 [{}]（`{}`）上全绿** ⇒ 这一族的 G-1 没有判别力，\
                 防线仍然全落在真进程字节比对上（REQ-080 F-2）",
                site.command, site.binding
            )
        });
        assert!(
            verdict.contains("重新绑定"),
            "🔴 [{}] 的 M2b 虽然红了，但红的**不是** F-2 那一格 ⇒ 变异没有命中目标：{verdict}",
            site.command
        );
    }

    // ── ⭐ REQ-080 F-2 · 那一格的**判别面**登记在这里，边界要说得准 ──
    //    只认 `let` 引入的重绑定，且只认判据**之后**、写出**之前**、同一块深度的。
    assert!(lets_rebind_root("let json = json.replace('\\n', \"\");", "json"));
    assert!(lets_rebind_root("let mut page = render();", "page"), "`let mut` 也是重绑定");
    assert!(!lets_rebind_root("let text = render(&json);", "json"), "换个名字绑定不算");
    assert!(!lets_rebind_root("if json { }", "json"), "光提到不算");
    assert_eq!(binding_root("page.html"), "page", "`page.html` 的根是 `page`");
    assert_eq!(binding_root("json_text"), "json_text");

    // ⚠️ 块深度那一格**也**必须有判别力：`line_brace_delta` 若恒返回 0，
    // 嵌套块里的同名 `let` 会假红，而「不进块」这个限定就名存实亡了。
    assert_eq!(line_brace_delta("match &out {"), 1, "开括号要算进去");
    assert_eq!(line_brace_delta("    } else {"), 0, "同层闭合再打开 = 净 0");
    assert_eq!(line_brace_delta("}"), -1, "闭括号要算成负的");
    assert_eq!(
        line_brace_delta(r#"eprintln!("{{}} 字面量里的括号不算");"#),
        0,
        "🔴 字符串字面量里的花括号被当成代码了 ⇒ 深度会算错"
    );
    assert_eq!(line_brace_delta(r#"let c = '}';"#), 0, "字符字面量里的 `}}` 同理");
    let depth_src: Vec<String> = vec![
        "fn f() {".into(),        // 1 → 深度 1
        "    if x {".into(),      // 2 → 深度 2
        "        let page = 1;".into(), // 3 → 深度 2（判据行深度）
        "    }".into(),           // 4 → 深度 1
        "    let page = 2;".into(), // 5 → 深度 1 ← 与判据**不同深度**，不该被算进同一窗口
        "}".into(),
    ];
    assert_eq!(
        block_depth_upto(&depth_src, 0, 2),
        2,
        "块深度累加算错了 ⇒ 「同一深度」这个限定名存实亡"
    );
    assert_eq!(block_depth_upto(&depth_src, 0, 4), 1);
}

/// 这个站点「判据读到绑定」的那一行（**1 基**行号）。
///
/// 与 [`check_site`] 里归属判据调用用的是**同一套口径**
/// （按 `judged_by` 找调用 → 取**最后一个**顶层实参 → 逐词元等于 `binding`），
/// 两边一旦漂移，G-4 那个变异就会插到别的行上去。
fn judge_line_for(lines: &[String], site: &DomainPrintSite) -> Option<usize> {
    let (lo, hi) = fn_span(lines, site.enclosing_fn)?;
    for (i, l) in lines.iter().enumerate().take(hi + 1).skip(lo) {
        for judge in site.judged_by {
            let Some(at) = l.find(&format!("{judge}(")) else { continue };
            let cs: Vec<char> = l.chars().collect();
            let Some(close) = match_paren(&cs, at + judge.len()) else { continue };
            let raw: String = cs[at + judge.len() + 1..close].iter().collect();
            let arg = split_top_level(&raw).pop().unwrap_or_default();
            if is_plain_path(&arg, site.binding) {
                return Some(i + 1);
            }
        }
    }
    None
}

// ══════════════════════════════════════════════════════════════
// G-5 · AC-005 —— 覆盖**所有**打印 domain 产物的站点
// ══════════════════════════════════════════════════════════════

/// 这个打印调用是不是「在打一个 domain 产物」。
///
/// 两个来源：登记表里登记过的绑定，以及 [`DOMAIN_PRODUCERS`] 这张枚举。
/// 两者取并集 ⇒ **新加的站点无论走哪一条都会被认出来**，认不出就红。
fn looks_like_domain_print(call: &PrintCall, bindings: &[&str]) -> bool {
    let emitted = emitted_expression(call);
    bindings.iter().any(|b| mentions_path(&emitted, b))
        || DOMAIN_PRODUCERS
            .iter()
            .any(|p| mentions_path(&emitted, p))
}

#[test]
fn g5_every_domain_product_print_site_is_registered() {
    let lines = read_cli();
    let bindings: Vec<&str> = DOMAIN_PRINT_SITES.iter().map(|s| s.binding).collect();

    // 反空转：判据得先认得出打印调用本身。
    let all_prints = scan_prints(&lines);
    assert!(
        all_prints.len() >= DOMAIN_PRINT_SITES.len(),
        "只认出 {} 处打印调用，少于登记表的 {} 条 —— 扫描口径不对",
        all_prints.len(),
        DOMAIN_PRINT_SITES.len()
    );

    // ① 正向：登记表里每一条，在源码里都得找得到对应打印点（G-1 已核过接线，
    //    这里核的是「**位置**还在」—— 两者不是同一格）。
    let mut missing = Vec::new();
    for site in DOMAIN_PRINT_SITES {
        let (lo, hi) = fn_span(&lines, site.enclosing_fn)
            .unwrap_or_else(|| panic!("找不到函数 {}", site.enclosing_fn));
        let hit = all_prints.iter().any(|c| {
            c.line >= lo + 1
                && c.line <= hi + 1
                && mentions_path(&emitted_expression(c), site.binding)
        });
        if !hit {
            missing.push(format!(
                "  - [{}] `{}` 里已没有打印 `{}` 的站点",
                site.command, site.enclosing_fn, site.binding
            ));
        }
    }
    assert!(missing.is_empty(), "登记表有 {}\n", missing.join("\n"));

    // ② 反向：⛔ **漏登记即红** —— 源码里打印 domain 产物、却不在册的站点。
    let mut unregistered: Vec<String> = Vec::new();
    for call in &all_prints {
        if !looks_like_domain_print(call, &bindings) {
            continue;
        }
        let owner = ["review_cmd", "ontology_cmd", "proposal_cmd", "concept_cmd"]
            .iter()
            .find(|f| {
                fn_span(&lines, f)
                    .map(|(lo, hi)| call.line >= lo + 1 && call.line <= hi + 1)
                    .unwrap_or(false)
            })
            .copied();
        let Some(owner) = owner else { continue };
        let emitted = emitted_expression(call);
        let registered = DOMAIN_PRINT_SITES
            .iter()
            .any(|s| s.enclosing_fn == owner && mentions_path(&emitted, s.binding));
        if !registered {
            unregistered.push(format!(
                "  - {}:{}（`{owner}`）打印了 `{emitted}`，\
                 但不在 `DOMAIN_PRINT_SITES` 里 —— 请登记，或证明它不是 domain 产物",
                CLI_SRC, call.line
            ));
        }
    }
    assert!(
        unregistered.is_empty(),
        "⛔ 有 {} 处打印 domain 产物的站点**未登记**（AC-005）：\n{}",
        unregistered.len(),
        unregistered.join("\n")
    );

    // ③ 未覆盖的必须**写清为什么** —— 「未覆盖」可以是结论，
    //    「未登记的未覆盖」不是。
    let mut no_note = Vec::new();
    for site in DOMAIN_PRINT_SITES.iter().filter(|s| !s.e2e_covered) {
        if site.note.trim().is_empty() {
            no_note.push(format!("  - [{}]", site.command));
        }
    }
    assert!(
        no_note.is_empty(),
        "未覆盖的站点必须写明原因：\n{}",
        no_note.join("\n")
    );

    // ④ 把覆盖度**打进运行日志**（`--nocapture` 下可见）——
    //    「哪些如实登记为未覆盖」必须是一条看得见的结论，不是文件里的一句话。
    let mut by_fn: BTreeMap<&str, Vec<&DomainPrintSite>> = BTreeMap::new();
    for s in DOMAIN_PRINT_SITES {
        by_fn.entry(s.enclosing_fn).or_default().push(s);
    }
    println!("\n===== REQ-078 · 打印点登记表覆盖度 =====");
    for (f, sites) in &by_fn {
        println!("  {f}：");
        for s in sites {
            println!(
                "    [{}] `{}` 前缀={:?} 尾换行={} 接线={} 判据={:?} 端到端字节比对={}",
                s.command,
                s.binding,
                s.prefix_template,
                s.trailing_newline,
                s.wiring,
                s.judged_by,
                if s.e2e_covered { "✅" } else { "⛔ 未覆盖" },
            );
            if !s.e2e_covered {
                println!("           原因：{}", s.note);
            }
        }
    }
    let covered = DOMAIN_PRINT_SITES.iter().filter(|s| s.e2e_covered).count();
    println!(
        "  —— 共 {} 个站点：端到端覆盖 {} 个，如实登记为未覆盖 {} 个",
        DOMAIN_PRINT_SITES.len(),
        covered,
        DOMAIN_PRINT_SITES.len() - covered
    );
    println!("=====================================\n");
    assert!(
        DOMAIN_PRINT_SITES.len() >= 8,
        "登记表只有 {} 条 —— REQ-072 的 grep 面至少是这么宽的，别把范围缩了",
        DOMAIN_PRINT_SITES.len()
    );
}


// ══════════════════════════════════════════════════════════════
// G-6 · ⭐ REQ-080 F-3 —— 「未覆盖」站点的**载荷面声明**必须为真
// ══════════════════════════════════════════════════════════════

/// 载荷面声明的**封闭词表** —— 未覆盖的站点必须在 note 里恰好落一个。
///
/// ⛔ 刻意用**标记**而不是散文：这次出事的形状恰恰是散文
/// （REQ-078 的 note 写「无文本载荷面」，而代码里载荷面明明白白存在）。
/// 散文可以被换一种说法绕过去，标记不能。
const PAYLOAD_SURFACE_YES: &str = "载荷面=有";
const PAYLOAD_SURFACE_NO: &str = "载荷面=无";

/// REQ-078 那句被实测推翻的话（⛔ 不许原样回来）。
const FALSE_NO_PAYLOAD_CLAIMS: &[&str] = &["无载荷面", "无文本载荷面", "没有可注入文本载荷"];

#[test]
fn g6_uncovered_sites_declare_a_payload_surface_that_holds() {
    // ① 未覆盖 ⇒ 必须**显式声明**载荷面（有 / 无，恰好一个）。
    let mut no_declaration = Vec::new();
    for site in DOMAIN_PRINT_SITES.iter().filter(|s| !s.e2e_covered) {
        let (yes, no_) = (
            site.note.contains(PAYLOAD_SURFACE_YES),
            site.note.contains(PAYLOAD_SURFACE_NO),
        );
        if yes == no_ {
            no_declaration.push(format!(
                "  - [{}] note 里要 `{PAYLOAD_SURFACE_YES}` 或 `{PAYLOAD_SURFACE_NO}` 恰好一个\n    {}",
                site.command, site.note
            ));
        }
    }
    assert!(
        no_declaration.is_empty(),
        "如实登记为未覆盖的站点**没有声明载荷面**（AC-005 ③ 的延续）：\n{}",
        no_declaration.join("\n")
    );

    // ② ⛔ REQ-078 那句「无载荷面」**不许**再回来。
    for site in DOMAIN_PRINT_SITES {
        for phrase in FALSE_NO_PAYLOAD_CLAIMS {
            assert!(
                !site.note.contains(phrase),
                "[{}] note 里的「{phrase}」**已被 REQ-080 F-3 实测推翻** —— \
                 载荷面存在，只是没有字节比对断言。两件事不许混成一件。\n    当前 note：{}",
                site.command, site.note
            );
        }
    }

    // ③ ⭐ 声明必须**为真**：声明「有」的那些站点，实测真的有载荷面。
    let declaring: Vec<&str> = DOMAIN_PRINT_SITES
        .iter()
        .filter(|s| s.note.contains(PAYLOAD_SURFACE_YES))
        .map(|s| s.command)
        .collect();
    assert!(
        declaring.contains(&"ontology diff") && declaring.contains(&"proposal apply（未通过清单）"),
        "载荷面证据是**按站点**写的（见下面两段）—— 登记表里声明「{PAYLOAD_SURFACE_YES}」的站点变了，\
         这两段证据就得跟着补。当前声明集合：{declaring:?}"
    );

    // (a) `ontology diff`：`DiffItem.target` 与 `before`/`after` 是**原始 JSON 值**，
    //     `diff.rs` 的 `render()` 对 `Value::String` 是**直接 clone**
    //     ⇒ payload 里的真换行原样进 `summary()`，也就原样进 stdout。
    let payload_newline = "客户\n伪造出来的第二行";
    let mut v1 = Snapshot::new(Uuid::nil(), 1);
    v1.entity_types = vec![EntityType {
        key: "customer".into(),
        display_name: "客户".into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    }];
    let mut v2 = v1.clone();
    v2.entity_types[0].display_name = payload_newline.into();
    let d = diff_snapshots(&v1, &v2);
    let summary = d
        .iter()
        .find(|x| x.field.as_deref() == Some("display_name"))
        .unwrap_or_else(|| {
            panic!("`display_name` 变了就该有一条 Modified —— 没有的话下面那条断言是**空断言**")
        })
        .summary();
    assert!(
        summary.contains('\n'),
        "🔴 [ontology diff] 载荷里的换行**没能**原样走到 `summary()` ⇒ \
         登记表声明的「{PAYLOAD_SURFACE_YES}」已经**过期**，请重测后再改 note。\n\
         实测 summary：{summary:?}"
    );

    // (b) `proposal apply（未通过清单）`：`ItemVerdict.target` 是 `ItemView.target`
    //     的克隆（`src/domain/review.rs:218`），原样来自提案 payload。
    let item = ItemView {
        id: "add_link_type:1".into(),
        kind: ItemKind::AddLinkType,
        target: payload_newline.into(),
        from_type: Some("两端之一：尚未被接受".into()),
        to_type: Some("两端之二：尚未被接受".into()),
        biz_status: Decision::Accepted,
        model_status: Decision::Accepted,
        payload: serde_json::Value::Null,
    };
    let st = summarize(&[item], &std::collections::BTreeSet::new());
    let blocked = st.blocked.first().unwrap_or_else(|| {
        panic!(
            "两端都不在可用集合里 ⇒ 这一条**必须**进 blocked —— 没有的话下面那条断言是**空断言**。\
             实测 applicable={:?} dangling={:?}",
            st.applicable, st.dangling_relations
        )
    });
    assert!(
        blocked.target.contains('\n'),
        "🔴 [proposal apply] 载荷里的换行**没能**原样走到 `ItemVerdict.target` ⇒ \
         登记表声明的「{PAYLOAD_SURFACE_YES}」已经**过期**，请重测后再改 note。\n\
         实测 target：{:?}",
        blocked.target
    );

    // ④ 反空转：把标记抽出来，两侧都要判得出来（否则 ① 恒绿 = 不是防线）。
    assert!(
        "未覆盖：无判据接线。载荷面=无".contains(PAYLOAD_SURFACE_NO)
            && !"未覆盖：无判据接线。载荷面=无".contains(PAYLOAD_SURFACE_YES),
        "🔴 词表判据分不开「有」与「无」⇒ ① 恒绿"
    );
    assert!(
        "载荷面=有".contains(PAYLOAD_SURFACE_YES)
            && !"载荷面=有".contains(PAYLOAD_SURFACE_NO),
        "🔴 词表判据分不开「有」与「无」⇒ ① 恒绿"
    );
    assert!(
        !FALSE_NO_PAYLOAD_CLAIMS
            .iter()
            .any(|p| "载荷面=有".contains(p)),
        "🔴 被禁的散文短语判据认不出 REQ-078 原文 ⇒ ② 恒绿"
    );
    assert!(
        FALSE_NO_PAYLOAD_CLAIMS
            .iter()
            .any(|p| "未覆盖：逐条 summary()，无文本载荷面、无判据接线；字节比对对它无判别力。"
                .contains(p)),
        "🔴 被禁的散文短语认不出 REQ-078 **原句** ⇒ ② 恒绿"
    );
}

// ══════════════════════════════════════════════════════════════
// G-7 · ⭐ REQ-080 F-4 —— 清理后的**复查**必须查得到 schema 名
// ══════════════════════════════════════════════════════════════
//
// 为什么要有这一条：F-4 的运行时证据（`f4_schema_probe_sees_a_schema_that_really_exists`）
// 证明的是**查询本身**看得见一个确实存在的 schema。它证明不了
// `cleanup_schema` **还在调**那个查询 —— 有人把复查那一行删掉，运行时证据照样绿。
//
// ⛔ 所以这里读**源码**：而且是**剥掉注释之后**读 ——
//    否则函数里那句解释「为什么不能是 to_regclass」的注释就会把判据喂饱，
//    那是一条自己判自己绿的断言（本仓最高发的形态）。

/// 「专属 schema 清理」的落点 —— 两处各有一份（REQ-072 / REQ-078 各自沙箱）。
const SCHEMA_CLEANUP_SITES: &[&str] = &[
    "tests/test_review_show_binary_req072.rs",
    "tests/test_domain_print_sites_binary_req078.rs",
];

/// `lines` 里某个顶层函数（**不含**注释行）的体。
fn stripped_fn_body<'a>(lines: &'a [String], name: &str) -> Option<&'a [String]> {
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with(&format!("fn {name}("))
            || l.trim_start().starts_with(&format!("async fn {name}(")))?;
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| l.trim_end() == "}")
        .map(|(i, _)| i)?;
    Some(&lines[start..=end])
}

#[test]
fn g7_schema_cleanup_recheck_queries_pg_namespace_not_to_regclass() {
    for file in SCHEMA_CLEANUP_SITES {
        let lines = strip_comments(&common::source_root::read_source(file));
        let body = stripped_fn_body(&lines, "cleanup_schema")
            .unwrap_or_else(|| panic!("{file} 里找不到 `cleanup_schema` —— 守卫读不懂那条接线"));
        let joined = body.join("\n");
        // 查询既可以**内联**在 `cleanup_schema` 里，也可以**委托**给一个
        // 专用函数（REQ-072 那份就是委托的，好让判定能拆成纯函数被直接断言）。
        // 两种形状都要跟到底，否则换一种写法就被守卫判红 —— 那是假红。
        let probe = if joined.contains("schema_still_there(") {
            stripped_fn_body(&lines, "schema_still_there")
                .unwrap_or_else(|| {
                    panic!(
                        "{file} 的 `cleanup_schema` 调了 `schema_still_there`，\
                         却没有那个函数 —— 守卫跟到底就跟丢了"
                    )
                })
                .join("\n")
        } else {
            joined.clone()
        };
        let both = format!("{joined}\n{probe}");

        assert!(
            probe.contains("pg_namespace"),
            "⛔ {file} 的 `cleanup_schema` 删掉了（或换掉了）那条查 schema 名的查询 —— \
             「`DROP` 报成功但还在」那一格因此又变回**永不触发**（REQ-080 F-4）。\n\
             现在这一格的正文是：\n{probe}"
        );
        assert!(
            !both.contains("to_regclass"),
            "⛔ {file} 的 `cleanup_schema` 里又出现了 `to_regclass` —— 它解析的是**关系名**，\
             对一个确实存在的 schema 恒返回 NULL，这一格因此恒真（REQ-080 F-4）。\n{both}"
        );
        // schema 不存在时这条查询返回**零行** ⇒ `fetch_one` 会把「已删干净」
        // 误报成「复查没跑起来」。这一格错了是把成功报成失败。
        assert!(
            probe.contains("fetch_optional"),
            "⛔ {file} 的 `cleanup_schema` 用了 `fetch_one` —— schema 已被删掉时这条查询返回\
             **零行**（不像 to_regclass 返回一行 NULL）⇒ 会把「收干净了」误报成失败。\n{probe}"
        );
    }

    // ── 反空转：判据**两侧**都要判得出来，且注释喂不饱它 ──
    //    （注释里写一遍查询名就绿 ⇒ 那是一条自己判自己绿的断言）
    let only_in_comment = strip_comments(
        "async fn cleanup_schema() {\n    // 从前是 SELECT to_regclass($1)\n    let q = 1;\n}\n\
         async fn other() {}\n",
    );
    assert!(
        !stripped_fn_body(&only_in_comment, "cleanup_schema")
            .unwrap()
            .join("\n")
            .contains("pg_namespace"),
        "🔴 把 `pg_namespace` 只写在**注释**里就能让这一格绿 ⇒ 判据恒绿"
    );
    // 真正的委托形状（含注释里的 `to_regclass`）必须判红 —— 这一条证明
    // 「注释里的 to_regclass 不算数、正文里的才算」这个口径是分得开的。
    let delegated = strip_comments(
        "async fn cleanup_schema() {\n    /* to_regclass 查的是关系名 */\n    \
         verdict_after_drop(schema_still_there(&pool, schema).await?, schema)\n}\n\
         async fn schema_still_there(p: &P, s: &str) -> Result<Option<String>, String> {\n    \
         sqlx::query_scalar(\"SELECT nspname FROM pg_namespace WHERE nspname = $1\")\n        \
         .fetch_optional(p).await.map_err(|e| e.to_string()).map(|r| r.flatten())\n}\n\
         async fn trailing() {}\n",
    );
    let delegated_probe = stripped_fn_body(&delegated, "schema_still_there")
        .unwrap()
        .join("\n");
    assert!(
        delegated_probe.contains("pg_namespace")
            && delegated_probe.contains("fetch_optional")
            && !format!(
                "{}\n{delegated_probe}",
                stripped_fn_body(&delegated, "cleanup_schema").unwrap().join("\n")
            )
            .contains("to_regclass"),
        "🔴 委托形状（注释里带 to_regclass）判不对 ⇒ G-7 的口径与它自己写的注释矛盾"
    );
    assert!(
        stripped_fn_body(&strip_comments("async fn other() {\n  pg_namespace\n}"), "cleanup_schema")
            .is_none(),
        "🔴 找不到函数时却抓到了一段 ⇒ `stripped_fn_body` 恒真"
    );
}
