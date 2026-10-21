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
//! | T7 | 管线模块自己的 stdout 宏 | 零命中（写出是出口的事） |
//! | T8 | 每条 `Enforcement::Pipeline` 的**证据**（第 3 轮 F1） | 文件里必须存在携带 `OutletId::<X>` 的**管线调用**，且该调用与登记的写点**在同一个函数**里（或写点所在函数正是该调用的适配器） |
//! | T9 | `Json(` / `Html(` 响应体的实参（第 3 轮 F4） | 必须是 `<绑定>.into_value()` / `<绑定>.into_text()`，且该绑定的**初始化式本身**就是管线调用（第 4 轮 B3②：不再接受「上游 20 行内出现过」） |
//! | T10 | 全仓所有 `fn into_value` / `fn into_text` 的**定义**（第 4 轮 B3①） | 只许在管线模块里（本地 newtype 不能伪造 `.into_value()` 这个形状） |
//!
//! # 第 4 轮（`qa-review-3.md` / `acceptance.md §七 A14`）：B1–B5
//!
//! | 项 | 病 | 这一版的封法 |
//! | --- | --- | --- |
//! | **B1** | `Err` 通道：`Result<_, Response>` 的 `Err` 本来就是 `Response` ⇒ `return Err((StatusCode::OK, raw).into_response())` **4 行、签名不动**就吐客户图 | `.into_response(` 进 [`FORBIDDEN_MECHANISMS`]，五处错误通道用 [`INTO_RESPONSE_SITES`] **逐字钉住**（形状一改 / 多一处 / 少一处都红）；另禁 `StatusCode::OK`（Err 通道不许回 2xx） |
//! | **B2** | [`prod()`] 在第一个 `#[cfg(test)]` 处**截断** ⇒ `http.rs` 文件尾部的生产代码不可见 | 改成**按花括号配对跳过** `#[cfg(test)]` 修饰的区间（区间抹成空行，行号 1:1） |
//! | **B3** | 本地 newtype 伪造 `.into_value()` + `if false` 里的死调用 ⇒ 73/0 全绿 | ① T10（定义只在管线模块）；② T9 要求绑定初始化式本身就是管线调用 |
//! | **B4** | 内联 `mod x { fn get_graph(…) }` 不被 `t6` 看见 | **方案 A**：内联 `mod` 的内容**展开纳入扫描**（`prod()` 不截断 + 全按文件文本扫），不额外禁写；理由见交付报告 §12.3 |
//! | **B5** | `OUTLET_FNS` 是 `(文件, 函数名)` 集合 ⇒ 同名第二个函数免登记 | T5 里**同名重复定义即红**（[`duplicate_fns`]）—— 这也让 T8 的「按名字比同函数」重新成立 |
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
//!   `encode_result(` / `fs::write(`）。**第 3 轮**已把「绕开全部构造」的三条路
//!   （`std::io::stdout` / `write_all(` / `Response::builder` / `.body(` / `Body::from(`）
//!   做成**否定式禁调**（[`FORBIDDEN_MECHANISMS`]，T1 内断言）——
//!   它们不再「全绿」，而是**一出现就红**。**第 4 轮**又把 `.into_response(` 与
//!   `StatusCode::OK` 加了进去（B1）。
//! - T2 只看「管线赋值 ↔ 写出」之间的窗口；「在管线调用**之前**用一个本地判据
//!   决定要不要调管线」这种形态由 T5（fn 登记）与 T1（新写点）侧证，
//!   **闭包形式的本地判据**不在 T5 覆盖内（诚实登记，见交付物 §12.7）。
//! - `prod()` 跳过的是 `#[cfg(test)]` **区间**；`#[cfg(not(test))]` / `cfg_attr` 不在此列。
//! - B1 的残余：`err(status, &raw)` 一类「错误通道携带任意文本」的形态**仍可表达**
//!   （`err` 的第二个参数是 `&str`）—— 见交付报告 §12.7 的 P-2 残余，**不是已封堵**。

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

/// 生产代码（剥注释 + **跳过** `#[cfg(test)]` 修饰的区间）。
///
/// # ⛔ 第 4 轮 B2：这里曾经是「截断」，不是「跳过」
///
/// 第 2–3 轮写的是 `raw.split("#[cfg(test)]").next()` ——
/// **从第一个 `#[cfg(test)]` 起全部丢掉**。
/// `src/server/http.rs` 的测试模块从 572 行一直到文件末尾（1246 行），
/// 于是「把 handler 追加到文件末尾 + 一行 `.route(...)`」（5 行、零登记）
/// 对 `t1` / `t3` / `t5` / `t8` / `t9` **全部不可见** ⇒ 13 目标 73/0 全绿。
/// 这是 A13 明令禁止的「判据看着绿、其实没人管」的真空（`qa-review-3.md` §7-B2）。
///
/// # 现在的口径：花括号配对，跳过**区间**
///
/// - 只认 `#[cfg(test)]`（以及 `#[cfg(all(test, …))]`）**属性行**；
///   从属性行起，按花括号配平找到它修饰的那个 item（`mod` / `impl` / `fn` …）的**结束行**；
/// - 区间**抹成空行**（⛔ 不是删行）：`display` / `code` 两个视图的行号必须与源文件 1:1，
///   `t8` 的「同函数」与 `t9` 的绑定定位都靠行号，删行会让所有报错行号漂移；
/// - 配平用的是 [`blank_strings`] 视图（字符串里的 `{` / `}` 不算），
///   否则测试模块里的一句 `"{}"` 就能把配平带偏。
/// - 多个 `#[cfg(test)]` 模块、以及**嵌套**花括号（`mod` 里再套 `mod` / 闭包 / 结构体）
///   都由配平处理，⛔ 不是「找第一个 `#[cfg(test)]` 就截断」，⛔ 也不是正则。
fn prod(rel: &str) -> String {
    let display = strip_comments(&read_source(rel));
    let lines: Vec<&str> = display.lines().collect();
    let masked = blank_strings(&display);
    let view: Vec<&str> = masked.lines().collect();
    let mut skip = vec![false; lines.len()];

    let mut i = 0usize;
    while i < view.len() {
        let t = view[i].trim_start();
        let is_cfg_test = t.starts_with("#[cfg(test)]")
            || t.starts_with("#[cfg(all(test,")
            || t.starts_with("#[cfg(all(test)]");
        if !is_cfg_test {
            i += 1;
            continue;
        }
        // 从属性行起配平花括号，直到「见过 `{` 且深度回到 0」。
        let mut depth = 0i32;
        let mut opened = false;
        let mut end = None;
        for (k, l) in view.iter().enumerate().skip(i) {
            depth += l.matches('{').count() as i32;
            depth -= l.matches('}').count() as i32;
            if l.contains('{') {
                opened = true;
            }
            if opened && depth <= 0 {
                end = Some(k);
                break;
            }
        }
        match end {
            // 区间抹空（含属性行与被修饰 item 的结束行）。
            Some(e) => {
                for s in skip.iter_mut().take(e + 1).skip(i) {
                    *s = true;
                }
                i = e + 1;
            }
            // 没有花括号（`#[cfg(test)] use …;` 之类）：只抹属性行，继续往后扫。
            None => {
                skip[i] = true;
                i += 1;
            }
        }
    }

    let mut out = String::with_capacity(display.len());
    for (i, l) in lines.iter().enumerate() {
        if !skip[i] {
            out.push_str(l);
        }
        out.push('\n');
    }
    out
}

/// 一个字符串里**内联**（`mod x { … }`）与**声明**（`mod x;`）模块的名字。
///
/// 第 4 轮 B4 选了**方案 A**（内联 mod 的内容**展开纳入扫描**，见 §12）：
/// `prod()` 不再截断、`write_points` / `fn_definitions` / `id_tokens` 都是**按文件文本**
/// 扫的，所以内联 `mod` 里的写点与函数本来就在扫描范围内 ——
/// 这条函数只用于**反空转**（证明内联 mod 确实被当成同文件，而不是被当成注释跳过）。
fn inline_mods(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in code.lines() {
        let t = line.trim_start();
        let t = t.strip_prefix("pub ").unwrap_or(t);
        let Some(rest) = t.strip_prefix("mod ") else { continue };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with('{') {
            out.push(name);
        }
    }
    out
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

// ══════════════════════════════════════════════════════════════
// 第 3 轮公共设施（F1–F4）
// ══════════════════════════════════════════════════════════════

/// **domain 生产者**：一段写点窗口里出现这些调用，说明它在**构造**客户视图。
///
/// F2（`qa-review-2.md` §7.1-B2 / §9-2）：`NON_CUSTOMER_WRITES` 是**自证清单** ——
/// 谁都能加一行 needle + 计数，把一处「打客户 JSON 的 `println!`」登记成「非客户」。
/// 判据改成：白名单命中的写点窗口**不得**含 domain 生产者（见
/// [`whitelist_smuggles_domain_product`]）。
const DOMAIN_PRODUCERS: &[&str] = &[
    "to_string_pretty(",
    "to_value(",
    "render_html(",
    "render_overview_text(",
    "filter_view(",
    "encode_result(",
];

/// F2 的**纯判据**：白名单命中的写点窗口里是否夹带了 domain 生产者。
///
/// `construct` 是这条写点自己的构造拼写 —— 若它本身就是 `encode_result(`，
/// 那「窗口里含 `encode_result(`」是**这条写点自己**，不算夹带。
fn whitelist_smuggles_domain_product<'a>(construct: &str, window: &'a str) -> Option<&'static str> {
    DOMAIN_PRODUCERS
        .iter()
        .copied()
        .find(|p| *p != construct && window.contains(p))
}

/// F3 / F4：出口文件里**禁止**出现的、绕开全部已登记构造的写出机制。
///
/// - **F3**（`qa-review-2.md` §3.6-5d）：`std::io::stdout().lock().write_all(raw)`
///   曾经 71 passed 全绿 —— REQ-106 的语义病（未过闸字节先落 stdout）仍可表达。
/// - **F4**（§4.6-M10b）：`Response::builder().status(..).body(Body::from(raw))`
///   曾经全绿，而且**顺手绕过 `check_scope`**。
///
/// ⇒ 第 3 轮把它们做成**否定式禁调**：不是「枚举到了才登记」，而是「出现即红」。
const FORBIDDEN_MECHANISMS: &[(&str, &str)] = &[
    ("std::io::stdout", "F3：自开一条 stdout，绕开 `emit_domain_*`（REQ-106 的原始病）"),
    ("write_all(", "F3：裸 fd 写 —— 与 `std::io::stdout` 同一条绕过路径"),
    ("Response::builder", "F4：响应体由 `Response::builder` 手搓 ⇒ 不在 `Json(`/`Html(` 写点枚举内"),
    (".body(", "F4：`.body(…)` 直收裸串 —— 同上"),
    ("Body::from(", "F4：`Body::from(raw)` 由裸串构造响应体 —— 同上"),
    (
        "into_response(",
        "B1：`Err` 通道自造响应体 —— `Result<_, Response>` 的 `Err` 本来就是 `Response`，\
         于是 `return Err((StatusCode::OK, raw).into_response())` 四行、签名不动就吐客户图",
    ),
    (
        "StatusCode::OK",
        "B1：`Err` 通道不许回 2xx —— 200 只许来自 `Ok(<管线产物>)` 那条分支",
    ),
];

/// B1 · `into_response(` 的**逐字钉住**清单：`(文件, 语句窗口里必须出现的 needle, 期望命中数)`。
///
/// 为什么不是「整函数豁免」：`into_response(` 是「把某个东西变成 `Response`」的**总闸门**，
/// 谁都能拿它拼响应体。今天它只出现在五处**错误通道**里（正文都是 `{"error": …}`，
/// 不含任何客户字节）。这里把五处**逐字**钉住：
///
/// - 改掉其中任何一处的形状（例如把 `msg` 换成裸串）⇒ needle 找不到 ⇒ 红；
/// - 在别处新加一处 ⇒ 「没被任何 needle 认领」⇒ 红；
/// - 把它们挪进别的函数、或让同一个 needle 出现两次 ⇒ 计数对不上 ⇒ 红。
///
/// ⚠️ 禁调 needle 是 **`into_response(`**（**不带前导点**）：于是
/// `x.into_response()`、`IntoResponse::into_response(x)`、`<T as IntoResponse>::into_response(x)`
/// 都是同一格 —— 只封 `.into_response(` 那个拼写等于留三种别名。
/// **函数定义行**（`fn into_response(self) -> Response`）不算调用，单独跳过
/// （见 [`forbidden_mechanisms`] 里的 `is_fn_def`）。
///
/// ⛔ 这张表**不是**「已封住」的声明：见交付报告 §12.7 的 P-2 残余
/// （`err(status, &raw)` 一类「错误通道携带任意文本」的形态仍可表达，已登记）。
const INTO_RESPONSE_SITES: &[(&str, &str, usize)] = &[
    (
        "src/server/http.rs",
        r#"(self.status(), Json(json!({ "error": msg })))"#,
        1,
    ),
    (
        "src/server/http.rs",
        r#"(status, Json(json!({ "error": msg })))"#,
        1,
    ),
    ("src/server/http.rs", "StatusCode::METHOD_NOT_ALLOWED.into_response()", 1),
    // 405 的固定文案（与 `NON_CUSTOMER_WRITES` 里那条同源）：它上面的 `(` 元组以 `.into_response()` 收尾。
    ("src/server/http.rs", "这里只读：只能看", 1),
    // 401 的固定文案：同上。
    ("src/server/http.rs", "没有可用的访问口令", 1),
];

/// B1 的语句窗口：命中行 + 往上 3 行。
///
/// 405 / 401 的 `.into_response()` 写在**续行**上，它的「被转换的表达式」在前 3 行内；
/// 窗口刻意这么短，是为了不让相邻的上一处 `into_response(`（`err` 与
/// `impl IntoResponse for ApiError` 只隔 5 行）互相顶替 pinned needle。
const INTO_RESPONSE_WINDOW: usize = 3;

/// 机制级豁免：**传输**函数里的 fd 写出（与 [`WRITER_FNS`] 同理）。
///
/// `mcp/serve.rs` 的 `write_mcp_line()` 把**已经编码好的响应行**写到 stdout —— 它是
/// MCP 的传输层，不是一条客户出口（它自己与 `serve` 都在 `OUTLET_FNS` 里登记）。
/// ⚠️ 这是**唯一**的 F3 豁免，且精确到「文件 + 函数」。
/// 第 3 轮之前豁免的是整条 `serve()`（函数体很大，往里加一行裸写不会被抓）；
/// 现在传输写被收进这一个小函数，`serve()` 的函数体受 F3 约束。
const TRANSPORT_FNS: &[(&str, &str)] = &[("src/mcp/serve.rs", "fn write_mcp_line")];

/// 取出 `construct` 之后**配对括号内**的实参（跨行时由调用方把窗口拼好）。
fn paren_arg(text: &str, construct: &str) -> Option<String> {
    let at = text.find(construct)?;
    let start = at + construct.len(); // `construct` 以 `(` 结尾
    let mut depth = 1usize;
    for (off, ch) in text[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + off].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// 这一行是不是**函数定义**行（`fn` / `pub fn` / `async fn` / `pub(crate) fn` …）。
fn is_fn_def(line: &str) -> bool {
    let t = line.trim_start();
    [
        "pub(crate) fn ",
        "pub async fn ",
        "pub const fn ",
        "pub fn ",
        "async fn ",
        "const fn ",
        "fn ",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

/// 函数定义行上的函数名。
fn fn_name_of(line: &str) -> Option<String> {
    let at = line.find("fn ")? + 3;
    let name: String = line[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// 从第 `at` 行开始的函数体行区间（0 基，含首尾；按花括号配平）。
fn fn_range_at(lines: &[String], at: usize) -> Option<(usize, usize)> {
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

/// 第 `idx` 行（0 基）**所属**的函数名：向上找最近的 `fn` 定义，再按花括号配平确认它包含 `idx`。
///
/// 这是 F1 的「同一个函数」判据与 [`pipeline_calls`] 的定位基础。
fn enclosing_fn(lines: &[String], idx: usize) -> Option<String> {
    let mut k = idx;
    loop {
        if is_fn_def(&lines[k]) {
            if let Some((lo, hi)) = fn_range_at(lines, k) {
                if idx >= lo && idx <= hi {
                    return fn_name_of(&lines[k]);
                }
            }
        }
        if k == 0 {
            return None;
        }
        k -= 1;
    }
}

/// 词边界（右侧）：`at` 处的字符之后不得紧跟标识符字符（`StatusCode::OKAY` 不算 `OK`）。
fn boundary_ok_after(line: &str, at: usize) -> bool {
    !line[at..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// 扫描一处源码里的**禁调机制**（F3/F4 + B1），跳过写出机制与传输层函数的函数体。
///
/// ⭐ 第 4 轮 B1：`.into_response(` 与 `StatusCode::OK` 也进了禁调集合。
/// `.into_response(` 只有 [`INTO_RESPONSE_SITES`] 里**逐字钉住**的五处（全是错误通道）例外，
/// 且钉住的每条命中数必须**恰好**等于期望值（双向，同 `NON_CUSTOMER_WRITES` 的口径）。
fn forbidden_mechanisms(rel: &str, code: &str) -> Vec<String> {
    let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
    let mut skip: Vec<(usize, usize)> = Vec::new();
    for m in WRITER_FNS {
        if let Some((lo, hi)) = fn_range(&lines, m) {
            skip.push((lo, hi));
        }
    }
    for (file, m) in TRANSPORT_FNS {
        if *file == rel {
            if let Some((lo, hi)) = fn_range(&lines, m) {
                skip.push((lo, hi));
            }
        }
    }
    // pinned needle 的命中计数（B1 的双向那一半）。
    let mut pin_hits: BTreeMap<&str, usize> = BTreeMap::new();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if skip.iter().any(|(lo, hi)| i >= *lo && i <= *hi) {
            continue;
        }
        for (needle, why) in FORBIDDEN_MECHANISMS {
            // ── B1 · `into_response(`：**不带前导点**，也**不做词边界检查** ──
            // 三种别名必须同罪：`raw.into_response()`（接收者是标识符，`boundary_ok` 会漏）、
            // `IntoResponse::into_response(x)`、`<T as IntoResponse>::into_response(x)`。
            // 唯一例外：**函数定义行**（`fn into_response(self) -> Response`）。
            if *needle == "into_response(" {
                let mut hits = 0usize;
                let mut from = 0usize;
                while let Some(off) = line[from..].find(needle) {
                    let at = from + off;
                    from = at + 1;
                    // 唯一例外：**函数定义**那一处（`fn into_response(self)`）——
                    // 逐处判，不整行判（一行里可能既有 `fn x()` 又有调用）。
                    if line[..at].trim_end().ends_with("fn") {
                        continue;
                    }
                    hits += 1;
                }
                if hits == 0 {
                    continue;
                }
                let lo = i.saturating_sub(INTO_RESPONSE_WINDOW);
                let window = lines[lo..=i].join("\n");
                if let Some((_, pinned, _)) = INTO_RESPONSE_SITES
                    .iter()
                    .find(|(f, n, _)| *f == rel && window.contains(n))
                {
                    *pin_hits.entry(pinned).or_insert(0) += hits;
                } else {
                    out.push(format!(
                        "  - {rel}:{} `{needle}` —— {why}\n      （也不在 `INTO_RESPONSE_SITES` \
                         钉住的错误通道里 ⇒ 这是一处**新造的响应体通道**）",
                        i + 1
                    ));
                }
                continue;
            }
            // ── B1 · `StatusCode::OK`：Err 通道不许回 2xx ──
            if *needle == "StatusCode::OK" {
                let mut from = 0usize;
                while let Some(off) = line[from..].find(needle) {
                    let at = from + off;
                    from = at + 1;
                    if !boundary_ok(line, at) || !boundary_ok_after(line, at + needle.len()) {
                        continue;
                    }
                    out.push(format!("  - {rel}:{} `{needle}` —— {why}", i + 1));
                }
                continue;
            }
            let mut from = 0usize;
            while let Some(off) = line[from..].find(needle) {
                let at = from + off;
                from = at + 1;
                if !boundary_ok(line, at) {
                    continue;
                }
                out.push(format!("  - {rel}:{} `{needle}` —— {why}", i + 1));
            }
        }
    }
    // B1 的双向：钉住的每条错误通道**必须**原样出现，且次数相等。
    for (file, pinned, expected) in INTO_RESPONSE_SITES {
        if *file != rel {
            continue;
        }
        let got = pin_hits.get(pinned).copied().unwrap_or(0);
        if got != *expected {
            out.push(format!(
                "  - {rel} 的错误通道 `{pinned}` 期望被 `.into_response(` 命中 {expected} 次，\
                 实际 {got} 次 —— 形状被改过（或有人把一处 `.into_response(` 挪了位置）"
            ));
        }
    }
    out
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
    // ── REQ-108 · 置信度的两处运维面输出（都不是客户视图）──
    (
        "src/cli.rs",
        "{band_lines}",
        1,
        "`review show --confidence` 的分档表：档位 / 分数 / 关系名 / 证据摘要 / veto 理由。\
         分数是**内部质量指标**，客户视图逐字不读（AC-010）；这一段是审核者显式带开关才输出的",
    ),
    (
        "src/cli.rs",
        "{report_lines}",
        1,
        "`confidence calibrate` 的分箱表：分数区间 / 条数 / 人工接受数 / 接受率 / 平均分。\
         运维面统计量，零客户视图字段（条目名都不打）",
    ),
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

/// ⭐ **纯函数**：给定「文件 → 生产代码」与两张白名单，返回**未登记的写点**、
/// **过期的白名单**，以及**夹带了 domain 生产者**的白名单命中（F2）。
fn unregistered_write_points(
    files: &BTreeMap<String, Src>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut unregistered = Vec::new();
    let mut stale_allow = Vec::new();
    let mut smuggled = Vec::new();
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
                    // ⭐ F2：白名单不是「填一行就合法」——窗口里夹带 domain 生产者
                    // 就说明这是**客户视图的构造**，登记成「非客户」不算数。
                    if let Some(prod) = whitelist_smuggles_domain_product(p.construct, &p.text) {
                        smuggled.push(format!(
                            "  - {rel}:{} `{}`（白名单 needle `{needle}`）的窗口里含 domain 生产者 `{prod}`\n      {}",
                            p.line,
                            p.construct,
                            p.text.replace('\n', " ⏎ ")
                        ));
                    }
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
    (unregistered, stale_allow, smuggled)
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

    let (unregistered, stale, smuggled) = unregistered_write_points(&files);
    assert!(stale.is_empty(), "⛔ 白名单过期：\n{}", stale.join("\n"));
    // ⭐ F2（第 3 轮）：白名单 needle 的写点窗口**不得**含 domain 生产者。
    // 反空转：QA 的 M7 形态（`println!` 打客户 JSON + 白名单一行）必须被判出来。
    assert_eq!(
        whitelist_smuggles_domain_product(
            "println!(",
            r#"println!("{}", serde_json::to_string_pretty(&*state.graph))"#
        ),
        Some("to_string_pretty("),
        "白名单夹带 domain 生产者的判据认不出来 ⇒ M7 仍然全绿"
    );
    assert_eq!(
        whitelist_smuggles_domain_product("encode_result(", "p::encode_result(id, p::empty_result())"),
        None,
        "`encode_result(` 是这条写点**自己的构造拼写**，不该被算成夹带 ⇒ 判据会恒红"
    );
    assert_eq!(
        whitelist_smuggles_domain_product("Json(", r#"(status, Json(json!({ "error": msg })))"#),
        None,
        "错误通道的 `json!` 被算成 domain 生产者 ⇒ 判据会恒红"
    );
    assert!(
        smuggled.is_empty(),
        "⛔ 有 {} 处白名单把「domain 生产者」登记成了非客户写点（F2：白名单 = 合法绕过）：\n{}",
        smuggled.len(),
        smuggled.join("\n")
    );

    // ⭐ F3 / F4（第 3 轮）+ B1（第 4 轮）：出口文件里**禁止**绕开全部已登记构造的写出机制。
    // 反空转：每一种形态必须被认出来（它们曾经各自 71–73 passed 全绿）。
    let probes = [
        (
            "fn x() { let mut o = std::io::stdout().lock(); o.write_all(raw.as_bytes()); }",
            "std::io::stdout",
        ),
        ("fn x() { std::io::stdout().write_all(raw.as_bytes()); }", "write_all("),
        ("fn x() { Response::builder().body(Body::from(raw)); }", "Response::builder"),
        ("fn x() { Response::builder().body(Body::from(raw)); }", ".body("),
        ("fn x() { Response::builder().body(Body::from(raw)); }", "Body::from("),
        // ── B1（qa-review-3.md §7）：`Err` 通道的四行形态 ──
        // `Result<_, Response>` 的 `Err` 本来就是 `Response`，签名一字不动就能吐客户图。
        (
            "fn x() { return Err((StatusCode::OK, raw).into_response()); }",
            "into_response(",
        ),
        (
            "fn x() { return Err((StatusCode::BAD_REQUEST, raw).into_response()); }",
            "into_response(",
        ),
        // 接收者是**标识符**（`String: IntoResponse`）—— 词边界陷阱：
        // `boundary_ok` 会把 `raw.into_response(` 当成「不是调用」，这一格必须单独钉。
        ("fn x() { return Err(raw.into_response()); }", "into_response("),
        // ⭐ 别名形态（第 4 轮的封法特意**不带前导点**）：UFCS 两种写法同罪。
        (
            "fn x() { return Err(IntoResponse::into_response((StatusCode::OK, raw))); }",
            "into_response(",
        ),
        (
            "fn x() { return Err(<String as IntoResponse>::into_response(raw)); }",
            "into_response(",
        ),
        ("fn x() { let ok = StatusCode::OK; }", "StatusCode::OK"),
    ];
    for (src, want) in probes {
        let got = forbidden_mechanisms("src/cli.rs", src);
        assert!(
            got.iter().any(|m| m.contains(want)),
            "禁调扫描器认不出 `{want}` ⇒ F3/F4/B1 那几条又回到「全绿」：{got:?}"
        );
    }
    // 反向：`fn into_response(self) -> Response` 是 **`IntoResponse` 的实现**，不是调用，
    // 不许误伤（否则任何 impl 都会恒红）。
    assert!(
        forbidden_mechanisms(
            "src/cli.rs",
            "impl IntoResponse for E {\n    fn into_response(self) -> Response {\n        todo!()\n    }\n}"
        )
        .is_empty(),
        "`fn into_response` 的**定义行**被当成调用 ⇒ 判据会恒红"
    );
    // 反向：`StatusCode::OKAY` 这类**不是** `OK` 的记号不许误伤（词边界右侧）。
    assert!(
        forbidden_mechanisms("src/cli.rs", "fn x() { let a = StatusCode::OKAY; }").is_empty(),
        "右侧词边界失效：`StatusCode::OKAY` 被当成 `StatusCode::OK` ⇒ 判据会恒红"
    );
    // 反向：今天那五处**错误通道**的 `.into_response(` 必须**不**被判红（否则判据恒红）。
    // 反探针用**真实源码**：合成片段做不到「五处都在、且互不顶替」。
    assert!(
        forbidden_mechanisms("src/server/http.rs", &prod("src/server/http.rs")).is_empty(),
        "真实的五处错误通道被当成绕过 ⇒ 判据会恒红：{:?}",
        forbidden_mechanisms("src/server/http.rs", &prod("src/server/http.rs"))
    );
    // 但它们被**逐字钉住**：形状一改就红。
    assert!(
        !forbidden_mechanisms(
            "src/server/http.rs",
            "fn err(status: StatusCode, msg: &str) -> Response {\n    (status, msg).into_response()\n}"
        )
        .is_empty(),
        "`err()` 被改成直收裸串仍然全绿 ⇒ B1 的「逐字钉住」是空转"
    );
    assert!(
        !forbidden_mechanisms(
            "src/server/http.rs",
            "fn err(status: StatusCode, msg: &str) -> Response {\n    (status, Json(json!({ \"error\": msg }))).into_response()\n}\nfn extra(m: &str) -> Response {\n    (StatusCode::BAD_REQUEST, m).into_response()\n}"
        )
        .is_empty(),
        "同一文件里多出一处未被钉住的 `.into_response(` 仍然全绿 ⇒ B1 只封了一个函数"
    );
    // 反向：写出机制本体 / MCP 传输层的 fd 写出必须放行（否则判据恒红）。
    assert!(
        forbidden_mechanisms(
            "src/cli.rs",
            "fn write_stdout(text: &str) { let mut out = std::io::stdout(); out.write_all(text.as_bytes()); }"
        )
        .is_empty(),
        "`write_stdout` 的函数体被当成绕过 ⇒ 判据会恒红"
    );
    assert!(
        forbidden_mechanisms(
            "src/mcp/serve.rs",
            "fn write_mcp_line(resp: &str) -> R { let mut o = std::io::stdout(); o.write_all(resp.as_bytes()) }"
        )
        .is_empty(),
        "MCP 传输层的 stdout 写出被当成绕过 ⇒ 判据会恒红"
    );
    // 反向：传输写被收进 `write_mcp_line` 之后，**`serve()` 的函数体**必须重新受约束
    // （第 3 轮之前整条 `serve()` 都在豁免里，往里加一行裸写不会被抓）。
    assert!(
        !forbidden_mechanisms(
            "src/mcp/serve.rs",
            "async fn serve(x: X) { let o = raw; std::io::stdout().write_all(o.as_bytes()); }"
        )
        .is_empty(),
        "`serve()` 又变成豁免区 ⇒ F3 在 MCP 传输层有一个大洞"
    );
    let mut forbidden = Vec::new();
    for rel in OUTLET_FILES {
        forbidden.extend(forbidden_mechanisms(rel, &prod(rel)));
    }
    assert!(
        forbidden.is_empty(),
        "⛔ 出口文件里出现了**禁用的写出机制**（F3/F4：绕开全部已登记构造）：\n{}",
        forbidden.join("\n")
    );
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
    // REQ-108 · 置信度：审核界面与校准命令（都不写客户视图字节）
    (
        "src/cli.rs",
        "render_confidence_table",
        "运维辅助（`review show --confidence` 的分档表渲染，纯文本、零客户字段）",
    ),
    ("src/cli.rs", "confidence_cmd", "子命令 handler（`confidence calibrate`）"),
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
    ("src/mcp/serve.rs", "write_mcp_line", "MCP **传输层**的 fd 写出（F3 的唯一豁免，精确到函数）"),

];

/// ⭐ **纯函数**：一段源码里**重名**的函数定义（第 4 轮 B5）。
///
/// 登记粒度是 `(文件, 函数名)`：`get_graph` 登记过一次之后，第二个同名函数
/// **不需要任何新登记**，`t8` 的「同函数」按名字比也会把它当成同一个函数
/// （`qa-review-3.md` §4.6-d：内联 `mod qa_shadow { fn get_graph(…) }` ⇒ 73/0 全绿）。
/// 函数名在一个文件里唯一，是「名字 ⇔ 函数」这条推理成立的前提。
fn duplicate_fns(names: &[String]) -> Vec<String> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for n in names {
        *seen.entry(n.as_str()).or_insert(0) += 1;
    }
    seen.into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(n, count)| format!("`fn {n}` 定义了 {count} 次"))
        .collect()
}

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

    // ⭐ B4 / B5（第 4 轮）：内联 `mod x { … }` 里的函数**同样**在这个文件的函数清单里
    // （方案 A：展开纳入扫描），于是同名第二个定义会被下面那圈判出来。
    let shadow = "mod qa_shadow {\n    use super::*;\n    pub async fn get_graph(s: S) -> R { todo!() }\n}\nfn get_graph(s: S) -> R { todo!() }";
    let got_shadow = fn_definitions(shadow);
    assert_eq!(
        got_shadow,
        vec!["get_graph".to_string(), "get_graph".to_string()],
        "内联 mod 里的函数没被枚举 ⇒ B4/B5 又回到「同名免登记」：{got_shadow:?}"
    );
    assert_eq!(inline_mods(shadow), vec!["qa_shadow".to_string()]);
    assert_eq!(
        duplicate_fns(&got_shadow),
        vec!["`fn get_graph` 定义了 2 次".to_string()],
        "重名函数认不出来 ⇒ 这条判据是空转"
    );
    assert!(
        duplicate_fns(&fn_definitions("fn a() {}\nfn b() {}")).is_empty(),
        "不重名的函数被算成重名 ⇒ 判据会恒红"
    );

    let mut bad = Vec::new();
    for rel in OUTLET_FILES {
        let got = fn_definitions(&prod(rel));
        let want: BTreeSet<&str> = OUTLET_FNS
            .iter()
            .filter(|(p, _, _)| p == rel)
            .map(|(_, n, _)| *n)
            .collect();
        // ⭐ B5：同名函数重复定义必须红 —— 否则 `OUTLET_FNS` 的 (文件, 名字) 粒度
        // 会让第二个 `get_graph` 免登记，`t8` 的「同函数」也会认错。
        for dup in duplicate_fns(&got) {
            bad.push(format!(
                "  - {rel}: {dup} —— 登记粒度是 (文件, 函数名)，重名函数等于**免登记**（B5）"
            ));
        }
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
    // REQ-108 · 证据评分卡（纯函数，AC-004 / AC-005）
    ("src/domain/mod.rs", "confidence"),
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
    // REQ-108 · 只读确定性采样器（AC-002 / AC-003）
    ("src/extract/mod.rs", "sample"),
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

// ══════════════════════════════════════════════════════════════
// T8 · 每条 `Enforcement::Pipeline` 必须有**证据**（第 3 轮 F1）
// ══════════════════════════════════════════════════════════════

/// 管线入口的**调用拼写**（`publish_customer` 是 CLI 的适配器，内部再调 `pipeline::render`）。
const PIPELINE_ENTRIES: &[&str] = &[
    "publish_customer(",
    "publish_json(",
    "publish_value(",
    "pipeline::render(",
];

/// 一处管线调用点：行号（1 基）、callee、实参里的 `OutletId::<X>` 记号、所在函数。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PipelineCall {
    line: usize,
    callee: &'static str,
    token: Option<String>,
    enclosing: Option<String>,
}

/// ⭐ **纯函数**：枚举一段源码里的管线调用点（函数**定义**行不算调用）。
fn pipeline_calls(code: &str) -> Vec<PipelineCall> {
    let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if is_fn_def(line) {
            continue;
        }
        for callee in PIPELINE_ENTRIES {
            let Some(at) = line.find(callee) else { continue };
            if !boundary_ok(line, at) {
                continue;
            }
            // 实参窗口：按括号配平，最多向下 8 行（登记表里的调用都不超过 4 行）。
            let mut buf = line[at..].to_string();
            let mut k = i;
            while paren_delta(&buf) > 0 && k + 1 < lines.len() && k + 1 <= i + 8 {
                k += 1;
                buf.push('\n');
                buf.push_str(&lines[k]);
            }
            // `id_tokens` 的键带 `OutletId::` 前缀，这里只留记号名。
            let token = id_tokens(&buf)
                .keys()
                .next()
                .map(|t| t.trim_start_matches("OutletId::").to_string());
            out.push(PipelineCall {
                line: i + 1,
                callee,
                token,
                enclosing: enclosing_fn(&lines, i),
            });
            break;
        }
    }
    out
}

/// 出口 id → **证据记号**（`OutletId::<T>` 里的 `T`）。
///
/// ⛔ 这里**不是**一张可扩展的映射表：除两个显式共享组外，证据记号**就是出口自己的
/// id**。于是「新登记一条 `H4`」**不会**自动借到 `H2` 的记号 —— 它必须自带
/// `OutletId::H4`（枚举里真有这个变体、且真被管线调用）。
///
/// 两个共享组是**写死**的，理由在代码里：
/// - `C2`（`--out` 写文件）与 `C1` 写**同一份产物**（`call_site` 注释与 `cli.rs`
///   的 `let html = publish_customer(OutletId::C1, ..)` 都是证据）；
/// - `M1`–`M5` 五个工具**共用一次** `publish_value(OutletId::McpTools, ..)`。
fn evidence_token(id: &str) -> String {
    match id {
        "C2" => "C1".to_string(),
        "M1" | "M2" | "M3" | "M4" | "M5" => "McpTools".to_string(),
        other => other.to_string(),
    }
}

#[test]
fn t8_pipeline_declarations_have_evidence() {
    use ontok::domain::pipeline::{Enforcement, OUTLETS};

    // 反空转 ①：新出口 id **不会**自动借到别人的记号（这是 M6b 的命门）。
    assert_eq!(evidence_token("H4"), "H4", "新出口 id 必须自带同名证据");
    assert_eq!(evidence_token("M6"), "M6", "只有登记在册的 M1–M5 才共享 `McpTools`");
    // 反空转 ②：两个显式共享组。
    assert_eq!(evidence_token("C2"), "C1");
    assert_eq!(evidence_token("M3"), "McpTools");
    // 反空转 ③：跨行调用也要取得到实参里的记号；定义行不是调用。
    let calls = pipeline_calls(
        "fn x() {\n    publish_json(\n        OutletId::H2,\n        value,\n    )\n}",
    );
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].token.as_deref(), Some("H2"), "{calls:?}");
    assert_eq!(calls[0].enclosing.as_deref(), Some("x"), "{calls:?}");
    assert!(
        pipeline_calls("fn publish_json(id: OutletId, v: Value) -> X { todo!() }").is_empty(),
        "函数定义被当成了调用点 ⇒ 这条判据会空转"
    );

    let mut bad = Vec::new();
    // ① 客户出口只许在三个出口文件里（否则下面的逐文件对账会漏掉它）。
    for o in OUTLETS.iter().filter(|o| o.enforcement == Enforcement::Pipeline) {
        if !OUTLET_FILES.contains(&o.path) {
            bad.push(format!(
                "  - {}: `Enforcement::Pipeline` 的出口落在 {} —— 客户可见字节只从三个出口文件出去",
                o.id, o.path
            ));
        }
    }
    // ② 逐文件：登记的每条 Pipeline 都要有「调用 + 同函数写点」两件证据。
    for rel in OUTLET_FILES {
        let src = Src::load(rel);
        let lines: Vec<String> = src.code.lines().map(|s| s.to_string()).collect();
        let calls = pipeline_calls(&src.code);
        let mut expected: BTreeSet<String> = BTreeSet::new();
        for o in OUTLETS
            .iter()
            .filter(|o| o.enforcement == Enforcement::Pipeline && o.path == *rel)
        {
            let token = evidence_token(o.id);
            expected.insert(token.clone());
            let Some(call) = calls.iter().find(|c| c.token.as_deref() == Some(token.as_str()))
            else {
                bad.push(format!(
                    "  - {}（{rel}）声明了 `Enforcement::Pipeline`，但 {rel} 里找不到携带 \
                     `OutletId::{token}` 的**管线调用** —— 「填表」不是接线（F1）",
                    o.id
                ));
                continue;
            };
            // 写点所在函数：登记 needle 在**展示视图**里的第一处。
            let Some(wp_idx) = src.display.lines().position(|l| l.contains(o.call_site)) else {
                continue; // T1 已经报「登记的 needle 在代码里找不到」
            };
            let wp_fn = enclosing_fn(&lines, wp_idx);
            let same_fn = wp_fn.is_some() && wp_fn == call.enclosing;
            // 适配器形态：写点所在函数**正是**这条管线调用的被调者
            // （HTTP 的 `publish_json` / `publish_value` 把产物交给 `Json(..)`）。
            let via_adapter = wp_fn
                .as_deref()
                .is_some_and(|f| f == call.callee.trim_end_matches('('));
            if !same_fn && !via_adapter {
                bad.push(format!(
                    "  - {}（{rel}）的写点在第 {} 行的 `{}` 里，但携带 `OutletId::{token}` 的管线调用在 \
                     第 {} 行的 `{}` 里 —— 声明与证据不在同一条路径上（F1）",
                    o.id,
                    wp_idx + 1,
                    wp_fn.as_deref().unwrap_or("?"),
                    call.line,
                    call.enclosing.as_deref().unwrap_or("?")
                ));
            }
        }
        // ③ 反向：文件里每一处**携带记号**的管线调用都必须有 Pipeline 条目认领
        //    （多调一次也是绕过：那是一次没人登记的闸调用）。
        for c in &calls {
            if let Some(tok) = &c.token {
                if !expected.contains(tok) {
                    bad.push(format!(
                        "  - {rel}:{} `{}` 携带 `OutletId::{tok}`，但登记表里没有一条 Pipeline 出口需要它",
                        c.line, c.callee
                    ));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "⛔ `Enforcement::Pipeline` 是声明，不是证据（F1：老实填表即合法）：\n{}",
        bad.join("\n")
    );
}

// ══════════════════════════════════════════════════════════════
// T9 · 客户响应体必须是管线产物（第 3 轮 F4）
// ══════════════════════════════════════════════════════════════

/// 响应体实参的**唯一合法形状**：`<绑定>.into_value()` / `<绑定>.into_text()`。
fn response_arg_product(arg: &str) -> Option<&str> {
    let arg = arg.trim();
    arg.strip_suffix(".into_value()")
        .or_else(|| arg.strip_suffix(".into_text()"))
}

/// 一个表达式**开头**就是一次管线调用吗（第 4 轮 B3 ②）。
///
/// 只看**头**：`let rendered = …` 的初始化式、或 `match …` 的被匹配表达式，
/// 必须**本身**是 `publish_value(…)` / `publish_json(…)` / `publish_customer(…)` /
/// `pipeline::render(…)` 之一。
///
/// ⛔ 第 3 轮的做法是「往上 20 行内出现过管线调用」—— 纯文本包含，
/// `if false { publish_value(…) }` 的死调用、甚至字符串字面量都能满足
/// （`qa-review-3.md` §4.3 的 a′ 因此 73/0 全绿）。
fn pipeline_expr_head(expr: &str) -> Option<&'static str> {
    let t0 = expr.trim_start();
    let t = t0.strip_prefix('&').map(str::trim_start).unwrap_or(t0);
    // `crate::domain::pipeline::publish_value(` 这层模块路径先剥掉
    // （剥掉之后 `render(` 就是 `pipeline::render(`，仍要求是本模块的那个入口）。
    if let Some(rest) = t.strip_prefix("crate::domain::pipeline::") {
        if rest.starts_with("render(") {
            return Some("pipeline::render(");
        }
        return PIPELINE_ENTRIES.iter().copied().find(|c| rest.starts_with(c));
    }
    PIPELINE_ENTRIES.iter().copied().find(|c| t.starts_with(c))
}

/// 第 `at` 行（0 基）**所属函数**的起始行（0 基）。
///
/// `product_origin` 必须只在**同一个函数**里找绑定：`check_scope` 里也有一处
/// `let rendered = match rendered { … };`（Z1 的错误文案），跨函数找会把
/// `index` 的 `Ok(rendered) => Ok(Html(rendered.into_text()))` 配到它头上。
fn enclosing_fn_lo(lines: &[String], at: usize) -> usize {
    let mut k = at;
    loop {
        if is_fn_def(&lines[k]) {
            if let Some((lo, hi)) = fn_range_at(lines, k) {
                if at >= lo && at <= hi {
                    return lo;
                }
            }
        }
        if k == 0 {
            return 0;
        }
        k -= 1;
    }
}

/// `ident` 这个产物绑定的**初始化式**（第 4 轮 B3 ②）。
///
/// 返回 `Ok(())` 当且仅当初始化式**本身就是**一次管线调用：
/// - `let <ident> = <pipeline call>(…)`；或
/// - `match <pipeline call>(…) { Ok(<ident>) => … }`（今天 `http.rs` 的两处都是这种）。
///
/// 已知边界：只找**最近**的 `match`，不跟 `impl` / `mod` 路径（`t5` 已保证函数名唯一，
/// 所以「名字 ⇔ 函数」成立）；嵌套 `match` 里再写响应体这一格由人工评审兜。
fn product_origin(lines: &[String], ident: &str, at: usize) -> Result<(), String> {
    // 只在**同一个函数**里找（跨函数会捡到别的函数的同名绑定）。
    let fn_lo = enclosing_fn_lo(lines, at);
    // ① `let <ident> = <初始化式>`（可能跨行，拼到分号配平为止）。
    let let_pat = format!("let {ident} =");
    let let_at = (fn_lo..at).rev().find(|i| {
        lines
            .get(*i)
            .is_some_and(|l| l.trim_start().starts_with(&let_pat))
    });
    if let Some(i) = let_at {
        let mut rhs = lines[i]
            .split_once('=')
            .map(|(_, r)| r.to_string())
            .unwrap_or_default();
        let mut k = i;
        while !rhs.contains(';') && k + 1 < at {
            k += 1;
            rhs.push(' ');
            rhs.push_str(lines.get(k).map(|s| s.as_str()).unwrap_or(""));
        }
        return match pipeline_expr_head(&rhs) {
            Some(_) => Ok(()),
            None => Err(format!(
                "的 `let {ident} = …` 初始化式**不是管线调用**（是 `{}`）—— \
                 第 {} 行",
                rhs.trim().lines().next().unwrap_or("").trim(),
                i + 1
            )),
        };
    }
    // ② `match <初始化式> { Ok(<ident>) => … }`。
    let match_at = (fn_lo..at).rev().find(|i| {
        lines
            .get(*i)
            .is_some_and(|l| l.trim_start().starts_with("match "))
    });
    let Some(m) = match_at else {
        return Err(format!(
            "在往上找不到 `let {ident} = …` 或 `match … {{ Ok({ident}) => … }}` 的绑定"
        ));
    };
    let mut expr = lines[m].trim_start().to_string();
    let mut k = m;
    while !expr.contains('{') && k + 1 < at {
        k += 1;
        expr.push(' ');
        expr.push_str(lines.get(k).map(|s| s.as_str()).unwrap_or(""));
    }
    let scrutinee = expr
        .split_once("match ")
        .map(|(_, rest)| rest)
        .unwrap_or("")
        .split('{')
        .next()
        .unwrap_or("")
        .to_string();
    match pipeline_expr_head(&scrutinee) {
        Some(_) => Ok(()),
        None => Err(format!(
            "绑定自第 {} 行的 `match {}` —— 被匹配的表达式**不是管线调用**（B3②）",
            m + 1,
            scrutinee.trim()
        )),
    }
}

#[test]
fn t9_customer_response_bodies_come_from_the_pipeline() {
    // 反空转：三条形状（M6b / M10b 的手搓响应体）必须认不出来是产物。
    assert_eq!(response_arg_product("rendered.into_value()"), Some("rendered"));
    assert_eq!(response_arg_product("rendered.into_text()"), Some("rendered"));
    assert_eq!(
        response_arg_product("serde_json::to_value(&*state.graph)"),
        None,
        "手搓的客户 JSON 被当成了管线产物 ⇒ M6b 仍然全绿"
    );
    assert_eq!(response_arg_product(r#"json!({ "error": msg })"#), None);
    assert_eq!(response_arg_product(r#"serde_json::to_string_pretty(node).unwrap_or_default()"#), None);

    // ⭐ B3 ②（第 4 轮）：产物的**初始化式本身**必须是管线调用。
    let l = |s: &str| s.lines().map(|x| x.to_string()).collect::<Vec<_>>();
    assert!(
        product_origin(
            &l("fn x() {\n    let rendered = publish_value(OutletId::H2, v);\n    Ok(Json(rendered.into_value()))\n}"),
            "rendered",
            2
        )
        .is_ok(),
        "`let <id> = publish_value(…)` 被当成「不是管线产物」⇒ 判据会恒红"
    );
    assert!(
        product_origin(
            &l("fn x() {\n    match crate::domain::pipeline::render(id, p) {\n        Ok(rendered) => Ok(Json(rendered.into_value())),\n    }\n}"),
            "rendered",
            2
        )
        .is_ok(),
        "`match render(…) {{ Ok(<id>) => … }}` 被当成「不是管线产物」⇒ 判据会恒红"
    );
    // QA a′ 的形态：死调用 + 本地包装的初始化式 ⇒ 必须判红。
    assert!(
        product_origin(
            &l("fn x() {\n    if false { let _ = publish_value(OutletId::H4, serde_json::json!({})); }\n    let rendered = QaRaw(serde_json::to_value(&*state.graph));\n    Ok(Json(rendered.into_value()))\n}"),
            "rendered",
            3
        )
        .is_err(),
        "死调用 + 本地包装仍被当成管线产物 ⇒ a′ 仍然全绿（B3②）"
    );
    assert!(
        product_origin(
            &l("fn x() {\n    match Ok::<QaRaw, Response>(QaRaw(raw)) {\n        Ok(rendered) => Ok(Json(rendered.into_value())),\n    }\n}"),
            "rendered",
            2
        )
        .is_err(),
        "`match Ok::<QaRaw, _>(…)` 被当成管线调用 ⇒ a′ 仍然全绿（B3②）"
    );
    assert!(
        product_origin(
            &l("fn x() {\n    let rendered = if false { publish_value(OutletId::H4, v) } else { raw };\n    Ok(Json(rendered.into_value()))\n}"),
            "rendered",
            2
        )
        .is_err(),
        "把管线调用藏在 `if false` 里当初始化式仍然全绿 ⇒ B3② 只看「出现过」"
    );

    let mut bad = Vec::new();
    let mut checked = 0usize;
    for rel in OUTLET_FILES {
        let src = Src::load(rel);
        let lines: Vec<String> = src.display.lines().map(|s| s.to_string()).collect();
        // ⛔ 不看登记表：**每一处** `Json(` / `Html(` 写点都要过这一关（除错误通道白名单）。
        // 只看「登记 needle 里含 Json( 的条目」会漏掉「手搓路由 + 登记 needle 不含 Json(」
        // 这一格（QA-M11 实测）。
        for p in write_points(&src.code, &src.display) {
            if !(p.construct == "Json(" || p.construct == "Html(") {
                continue;
            }
            let whitelisted = NON_CUSTOMER_WRITES
                .iter()
                .any(|(path, needle, _, _)| *path == *rel && p.text.contains(needle));
            if whitelisted {
                continue;
            }
            checked += 1;
            let Some(arg) = paren_arg(&p.text, p.construct) else {
                bad.push(format!(
                    "  - {rel}:{} `{}` 的实参取不出来",
                    p.line, p.construct
                ));
                continue;
            };
            let Some(ident) = response_arg_product(&arg) else {
                bad.push(format!(
                    "  - {rel}:{} 的响应体是 `{}` —— **不是管线产物**（F4：只能是 \
                     `<绑定>.into_value()` / `<绑定>.into_text()`）",
                    p.line,
                    arg.trim().replace('\n', " ")
                ));
                continue;
            };
            let this_line = lines.get(p.line - 1).map(|s| s.as_str()).unwrap_or("");
            if !this_line.contains(&format!("Ok({ident})")) {
                bad.push(format!(
                    "  - {rel}:{} 的 `{ident}` 不在 `Ok({ident})` 分支里 —— 拿不到管线产物",
                    p.line
                ));
                continue;
            }
            // ⭐ B3 ②（第 4 轮）：这个 `Ok(..)` 分支的**初始化式本身**必须是管线调用。
            // ⛔ 第 3 轮这里是「往上 20 行内出现过管线调用」—— 死调用/字符串字面量都能满足。
            if let Err(why) = product_origin(&lines, ident, p.line - 1) {
                bad.push(format!("  - {rel}:{} 的 `{ident}` {why}", p.line));
            }
        }
    }
    // 反空转：干净树上必须真的检查到「客户响应体」那几处（今天 http.rs 是 2 处：
    // H1 整页的 `Html(` + H2/H3 共用的 `publish_json` 里的 `Json(`；
    // 其余 3 处 `Json(` 是错误通道白名单）。
    assert!(
        checked >= 2,
        "只检查到 {checked} 处响应体 ⇒ 白名单把客户响应体也放行了，这条判据在空转"
    );
    assert!(
        bad.is_empty(),
        "⛔ 客户响应体不是管线产物（F4：`Response::builder().body(raw)` 的同类形态）：\n{}",
        bad.join("\n")
    );
}

// ══════════════════════════════════════════════════════════════
// T10 · `into_value` / `into_text` 只许定义在管线模块（第 4 轮 B3 ①）
// ══════════════════════════════════════════════════════════════

/// ⭐ **纯函数**：一段源码里 `into_value` / `into_text` 的**定义**（不是调用）。
///
/// 只认 `fn into_value` / `pub fn into_text` / trait 里的方法签名；
/// `rendered.into_value()` 这种**调用**不算（`line[..at]` 不以 `fn` 结尾）。
fn guarded_consumers(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in code.lines() {
        for name in ["into_value", "into_text"] {
            let mut from = 0usize;
            while let Some(off) = line[from..].find(name) {
                let at = from + off;
                from = at + 1;
                if !boundary_ok(line, at) || !boundary_ok_after(line, at + name.len()) {
                    continue;
                }
                if line[..at].trim_end().ends_with("fn") {
                    out.push(name.to_string());
                }
            }
        }
    }
    out
}

#[test]
fn t10_into_value_is_defined_only_in_the_pipeline_module() {
    // 反空转：QA a′ 的本地 newtype 必须被认出来。
    let forged = "struct QaRaw(serde_json::Value);\nimpl QaRaw {\n    fn into_value(self) -> serde_json::Value { self.0 }\n}";
    assert_eq!(
        guarded_consumers(forged),
        vec!["into_value".to_string()],
        "本地 newtype 的 `fn into_value` 认不出来 ⇒ B3① 空转"
    );
    assert_eq!(
        guarded_consumers("trait LocalShape { fn into_text(&self) -> String; }"),
        vec!["into_text".to_string()],
        "trait 里的 `fn into_text` 认不出来（换个名字定义同名方法就能绕）"
    );
    assert!(
        guarded_consumers("let v = rendered.into_value();").is_empty(),
        "**调用**被算成了**定义** ⇒ 判据会恒红"
    );

    // 实测：管线模块里恰好有这两个消费方法（否则下面那圈「管线外为 0」是空转）。
    let pipe = guarded_consumers(&prod(PIPELINE_SRC));
    assert!(
        pipe.contains(&"into_value".to_string()) && pipe.contains(&"into_text".to_string()),
        "管线模块里读不到 `into_value` / `into_text` ⇒ 这条判据在空转：{pipe:?}"
    );

    let mut bad = Vec::new();
    for rel in src_files() {
        if rel == PIPELINE_SRC {
            continue;
        }
        for name in guarded_consumers(&prod(&rel)) {
            bad.push(format!("  - {rel}: 定义了 `fn {name}`"));
        }
    }
    assert!(
        bad.is_empty(),
        "⛔ 管线模块**之外**定义了 `into_value` / `into_text`（B3：本地 newtype 一行就能\
         伪造 t9 要的那个形状）：\n{}",
        bad.join("\n")
    );
}

