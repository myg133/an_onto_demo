//! REQ-081 · **守卫的守卫 · 第二层** —— 让「删掉 REQ-075 那个元测试」也变成**构建失败**。
//!
//! @guard-manifest: REQ-081
//!
//! 测试名带 `test_req081_` 前缀。
//!
//! # 本条补的是哪一格
//!
//! REQ-075 的模块注释自己写明了递归终点（`:33` 那两行）：
//!
//! > 「本文件自己也能被删 ⇒ 删掉它 ⇒ 下面两条 `include_str!` 的编译错误一起消失
//! > ⇒ 守卫的删除重新变成**无痕**」
//!
//! BA 已实测：**只删元测试跑全量 ⇒ `exit=0`、只有数目标才看得见**。
//! ⇒ 本层补的就是这一格：**REQ-075 与本文件互相钉住**，任一消失 ⇒ **构建失败**。
//!
//! # 本文件的四条边界（⛔ 不许被后来人改写成「已封堵」）
//!
//! | 事实 | 后果 |
//! | --- | --- |
//! | **本文件自己也能被删** | 删掉它 ⇒ REQ-075 的 `include_str!` 编译失败（**构建失败**，不是变红） |
//! | **本文件与 REQ-075 一起删** | 两处 `include_str!` 一起消失 ⇒ 仍然 exit 0，**没有任何一条测试会红** |
//! | **基线文件也能被删** | 删掉它 ⇒ 本文件 `include_str!` 编译失败（**构建失败**） |
//! | **本文件自己也能被改空** | 把断言改成恒真 ⇒ 与删掉等价；**断言数下界只抓得住「删断言」，抓不住「换一个同样强的断言」** |
//!
//! ⇒ **递归到此为止**：这一层与 REQ-075 是**互钉**的两层，再往上一层它自己也可以被删。
//! 本层 **不声称封堵** 任何东西；它只把「静默删除守卫」从**无痕**变成
//! **编译报错 / 清单与实物对不上**。
//!
//! # 清单的真相来源：**由实物生成**，不是手写
//!
//! | 东西 | 唯一来源 |
//! | --- | --- |
//! | 哪些文件是守卫 | `tests/**/*.rs` 里带标记行 [`META_MARKER`] 的文件（**扫出来的**） |
//! | 每条守卫测试叫什么 | 那些文件里 trim 后恰为 `#[test]` 的属性之后那条顶层 `fn`（**扫出来的**） |
//! | 每条守卫测试有多强 | 同一条 `fn` 体里 `assert_eq!` / `assert_ne!` / `assert!` 的出现次数（**数出来的**） |
//! | 人读的那一份 | [`BASELINE`]（由实物**机械生成**，判据失败时原样给出新内容） |
//!
//! ⚠️ 与 REQ-075 既有 `INVENTORY` 的分工：REQ-075 那一层登记**行为守卫**
//! （REQ-068 / REQ-070），本层登记**元守卫层**（REQ-075 / REQ-081）。
//! 两份登记**互不重叠**，合起来覆盖整条守卫链。
//!
//! ⚠️ **为什么没有把 REQ-075 的 `INVENTORY` 换成本基线**（BA 的范围判断 ⛔3「不改既有判据逻辑」）：
//! `test_req075_guard_test_counts_match_the_inventory` 的逻辑是
//! 「文件里 `#[test]` 的条数 == 清单里登记的条数」。若清单**由实物生成**，
//! 这个等式**恒成立** ⇒ 那条判据变成恒真、就此失效。
//! ⇒ 「由实物生成」与「登记是硬要求」在**同一条判据上**不可兼得；这里选保住既有判据。
//!
//! # 「防改弱」的覆盖边界（⛔ 不声称穷尽）
//!
//! 本层用**断言宏计数下界**（floor）钉住每条守卫测试的强度。
//!
//! | 形态 | 抓得住吗 |
//! | --- | --- |
//! | 删掉一条 `assert_eq!` / `assert!` | ✅ 计数掉到下界以下 |
//! | `assert_eq!(a, b)` → `assert!(true)` | ✅ `assert_eq!` 计数掉 1 |
//! | 把守卫测试整个删空 | ✅ 进「实物里没有」那一类 |
//! | `assert!(x == y)` → `assert!(x == y \|\| true)` | ❌ **抓不到**（宏个数不变） |
//! | 把断言挪进一个被改弱的辅助函数 | ❌ **抓不到** |
//! | `assert_eq!(compute(), 3)` 而 `compute()` 被改坏 | ❌ **抓不到** |
//! | 改低本文件自己的判据门槛 | ❌ **抓不到**（这就是上面那行「本文件自己也能被改空」） |
//!
//! ⇒ 有限枚举抓不完「改弱」，**这一层不是封堵**。

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ══════════════════════════════════════════════════════════
// 编译期锚点 —— AC-001 里「构建失败」那一格就靠它们
// ══════════════════════════════════════════════════════════

/// REQ-075 那个元测试 —— **本文件是它的下一层**。
/// 它被删 ⇒ 本二进制**编译失败**（E0463），这不是「一条红」。
const GUARD_REQ075: &str = include_str!("test_review_view_req075.rs");

/// **本文件自己**。写在这里是**故意**的：REQ-075 那边有一份对称的
/// `include_str!` 指着它 ⇒ 删掉本文件 ⇒ **REQ-075 的二进制编译失败**。
const GUARD_REQ081: &str = include_str!("test_guard_manifest_req081.rs");

/// 登记基线 —— 人读的那一份。删掉它 ⇒ 本二进制**编译失败**。
const BASELINE: &str = include_str!("guard_baseline.req081.txt");

/// 守卫文件的**机器可识别标记**（模块文档里的一行）。
///
/// 为什么必须是**实物上的标记**而不是文件名规律：`tests/` 下有一整排
/// `test_review_view_req0NN.rs`，其中大半（req064/065/069/071…）**不是守卫**。
/// 按名字扫会把它们一起拖进来。
const META_MARKER: &str = "//! @guard-manifest: REQ-081";

// ══════════════════════════════════════════════════════════
// 词法层：把注释与字符串内容**涂成空格**（保留长度与换行）
// ══════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
enum Lex {
    Code,
    Line,
    Block(usize),
    Str,
    RawStr(usize),
    CharLit,
}

/// 涂掉**注释**与**字符串/字符字面量的内容**，引号与换行保留。
///
/// # 为什么必须做到字面量这一层
///
/// 只剥注释是不够的：守卫测试体里满是 `format!("…assert_eq!…")` 这类**字符串**，
/// 数它们就是**恒假/恒真**的来源（本会话已被此坑两次：REQ-065 的注释占比 56%）。
/// 涂掉内容后，`{` / `}` 的深度统计与宏计数看到的**只有代码**。
fn blank_non_code(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = cs.clone();
    let (mut i, mut st) = (0usize, Lex::Code);

    while i < cs.len() {
        let c = cs[i];
        match st {
            Lex::Code => match c {
                '/' if cs.get(i + 1) == Some(&'/') => {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    st = Lex::Line;
                    i += 2;
                }
                '/' if cs.get(i + 1) == Some(&'*') => {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    st = Lex::Block(1);
                    i += 2;
                }
                'r' => {
                    // r"…" / r#"…"# / r##"…"##
                    let mut j = i + 1;
                    let mut hashes = 0usize;
                    while cs.get(j) == Some(&'#') {
                        hashes += 1;
                        j += 1;
                    }
                    if cs.get(j) == Some(&'"') {
                        i = j + 1;
                        st = Lex::RawStr(hashes);
                    } else {
                        i += 1;
                    }
                }
                '"' => {
                    st = Lex::Str;
                    i += 1;
                }
                '\'' => {
                    // 字符字面量 `'x'` / `'\…'`；否则是生命周期 `&'a T`，按普通字符放过。
                    let is_char = cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&'\'');
                    if is_char {
                        st = Lex::CharLit;
                    }
                    i += 1;
                }
                _ => i += 1,
            },
            Lex::Line => {
                if c != '\n' {
                    out[i] = ' ';
                }
                if c == '\n' {
                    st = Lex::Code;
                }
                i += 1;
            }
            Lex::Block(depth) => {
                if c == '/' && cs.get(i + 1) == Some(&'*') {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    st = Lex::Block(depth + 1);
                    i += 2;
                } else if c == '*' && cs.get(i + 1) == Some(&'/') {
                    out[i] = ' ';
                    out[i + 1] = ' ';
                    st = if depth <= 1 { Lex::Code } else { Lex::Block(depth - 1) };
                    i += 2;
                } else {
                    if c != '\n' {
                        out[i] = ' ';
                    }
                    i += 1;
                }
            }
            Lex::Str => {
                if c == '\\' {
                    if cs.get(i + 1).is_some() {
                        out[i] = ' ';
                        out[i + 1] = ' ';
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else if c == '"' {
                    st = Lex::Code;
                    i += 1;
                } else {
                    if c != '\n' {
                        out[i] = ' ';
                    }
                    i += 1;
                }
            }
            Lex::RawStr(hashes) => {
                if c == '"' && (0..hashes).all(|k| cs.get(i + 1 + k) == Some(&'#')) {
                    i += 1 + hashes;
                    st = Lex::Code;
                } else {
                    if c != '\n' {
                        out[i] = ' ';
                    }
                    i += 1;
                }
            }
            Lex::CharLit => {
                if c == '\\' {
                    if cs.get(i + 1).is_some() {
                        out[i] = ' ';
                        out[i + 1] = ' ';
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else if c == '\'' {
                    st = Lex::Code;
                    i += 1;
                } else {
                    if c != '\n' {
                        out[i] = ' ';
                    }
                    i += 1;
                }
            }
        }
    }

    out.into_iter().collect()
}

/// 数「独立出现」的 `needle` 次数 —— 前一个字符**不得**是标识符字符。
///
/// 这一步是 `assert_eq!` 不被 `debug_assert_eq!`、`assert_ne!` 不被 `assert!` 顺带数的关键。
fn count_macro(hay: &str, needle: &str) -> usize {
    let chars: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return 0;
    }
    let mut hits = 0usize;
    let mut i = 0usize;
    while i + n.len() <= chars.len() {
        if chars[i..i + n.len()] == n[..] {
            let boundary = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
            if boundary {
                hits += 1;
                i += n.len();
                continue;
            }
        }
        i += 1;
    }
    hits
}

// ══════════════════════════════════════════════════════════
// 解析层：从**实物**抽出「守卫测试 + 它的断言强度」
// ══════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedTest {
    name: String,
    eq: usize,
    ne: usize,
    plain: usize,
}

impl ParsedTest {
    fn strengths(&self) -> [usize; 3] {
        [self.eq, self.ne, self.plain]
    }
}

/// `#[test]` 之后那条 `fn` 的名字，以及**函数体首行**（含 `{` 的那行）下标。
///
/// ⚠️ 注释已被涂掉 ⇒ `#[test]` 与 `fn` 之间夹多少注释都不影响。
/// 签名跨行也认（拼到出现 `{` 或 `;` 为止）。
fn fn_head_after(lines: &[&str], start: usize) -> Option<(String, usize)> {
    let mut sig = String::new();
    let mut k = start;
    while k < lines.len() && k - start <= 16 {
        let l = lines[k].trim();
        if !sig.is_empty() {
            sig.push(' ');
        }
        sig.push_str(l);
        if l.contains('{') || l.contains(';') {
            break;
        }
        k += 1;
    }
    if k >= lines.len() {
        return None;
    }
    let rest = sig.trim().strip_prefix("fn ")?;
    let open = rest.find('(')?;
    let name = rest[..open].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), k))
}

/// 那个 `fn` 的**闭合大括号**所在行下标。
///
/// 用花括号深度而不是「下一行以 `}` 开头」：字符串内容已涂掉，深度是**精确**的，
/// 不会被 `format!("}}")` 之类带偏。
fn body_end(lines: &[&str], head: usize) -> usize {
    let mut depth: i64 = 0;
    let mut started = false;
    for idx in head..lines.len() {
        for ch in lines[idx].chars() {
            if ch == '{' {
                depth += 1;
                started = true;
            } else if ch == '}' {
                depth -= 1;
            }
        }
        if started && depth <= 0 {
            return idx;
        }
    }
    lines.len().saturating_sub(1)
}

/// 从一份守卫源码里抽出**全部** `#[test]` 及其断言强度（按出现顺序）。
fn parse_guard_tests(src: &str) -> Vec<ParsedTest> {
    let blank = blank_non_code(src);
    let lines: Vec<&str> = blank.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].trim() == "#[test]" {
            if let Some((name, head)) = fn_head_after(&lines, i + 1) {
                let end = body_end(&lines, head).min(lines.len() - 1);
                let body: String = lines[head..=end].join("\n");
                out.push(ParsedTest {
                    name,
                    eq: count_macro(&body, "assert_eq!"),
                    ne: count_macro(&body, "assert_ne!"),
                    plain: count_macro(&body, "assert!"),
                });
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ══════════════════════════════════════════════════════════
// 实物扫描：哪些文件是守卫
// ══════════════════════════════════════════════════════════

fn collect_rs(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort(); // 稳定 ⇒ 报错顺序可复现
    for p in items {
        if p.is_dir() {
            collect_rs(&p, acc);
        } else if p.extension().is_some_and(|e| e == "rs") {
            acc.push(p);
        }
    }
}

/// `tests/**/*.rs` 的全部源码（键是**相对 `tests/`** 的路径，用 `/` 分隔）。
fn all_test_sources() -> BTreeMap<String, String> {
    let tests_root = common::crate_root().join("tests");
    let mut acc = BTreeMap::new();
    let mut files = Vec::new();
    collect_rs(&tests_root, &mut files);
    for f in files {
        let Ok(raw) = std::fs::read_to_string(&f) else {
            continue;
        };
        let rel = f
            .strip_prefix(&tests_root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        acc.insert(rel, raw);
    }
    acc
}

/// 带 [`META_MARKER`] 标记的那几份（守卫文件集合 —— **扫出来的**，不是手写的）。
fn guard_sources() -> BTreeMap<String, String> {
    all_test_sources()
        .into_iter()
        .filter(|(_, v)| v.lines().any(|l| l.trim() == META_MARKER))
        .collect()
}

fn derive(srcs: &BTreeMap<String, String>) -> BTreeMap<String, Vec<ParsedTest>> {
    srcs.iter()
        .map(|(k, v)| (k.clone(), parse_guard_tests(v)))
        .collect()
}

// ══════════════════════════════════════════════════════════
// 登记基线（人读的那一份）
// ══════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct RegTest {
    name: String,
    floors: [usize; 3],
}

#[derive(Debug, Clone)]
struct RegFile {
    file: String,
    tests: Vec<RegTest>,
}

#[derive(Debug, Clone, Default)]
struct Registry {
    files: Vec<RegFile>,
    /// `(守卫文件名, id, 必须逐字在位的措辞)`
    phrases: Vec<(String, String, String)>,
    /// `(守卫文件名, **不得**出现在那份文件里的过度声明措辞)`
    banned: Vec<(String, String)>,
}

fn parse_baseline(text: &str) -> Registry {
    let mut reg = Registry::default();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = l.split('|').collect();
        match f.as_slice() {
            ["FILE", file] => reg.files.push(RegFile {
                file: (*file).to_string(),
                tests: Vec::new(),
            }),
            ["TEST", file, name, eq, ne, plain] => {
                let t = RegTest {
                    name: (*name).to_string(),
                    floors: [
                        eq.parse().unwrap_or(0),
                        ne.parse().unwrap_or(0),
                        plain.parse().unwrap_or(0),
                    ],
                };
                match reg.files.iter_mut().find(|x| x.file == *file) {
                    Some(rf) => rf.tests.push(t),
                    None => reg.files.push(RegFile {
                        file: (*file).to_string(),
                        tests: vec![t],
                    }),
                }
            }
            ["PHRASE", file, id, text] => {
                reg.phrases.push(((*file).to_string(), (*id).to_string(), (*text).to_string()))
            }
            ["BANNED", file, text] => {
                reg.banned.push(((*file).to_string(), (*text).to_string()))
            }
            _ => {}
        }
    }
    reg
}

// ══════════════════════════════════════════════════════════
// 三条判据（各自独立，便于自检逐条证明它们不是空转）
// ══════════════════════════════════════════════════════════

/// **判据一 · 清单 vs 实物**：文件集合与测试名集合必须逐条相等。
fn inventory_problems(
    derived: &BTreeMap<String, Vec<ParsedTest>>,
    reg: &Registry,
) -> Vec<String> {
    let mut out = Vec::new();
    let dnames: BTreeSet<&String> = derived.keys().collect();
    let rnames: BTreeSet<&String> = reg.files.iter().map(|f| &f.file).collect();

    for f in dnames.difference(&rnames) {
        out.push(format!(
            "`{f}` 带 `{META_MARKER}` 标记但**没登记**进基线 —— \
             守卫在没人知情的情况下变动 = 无痕变动，请回来登记"
        ));
    }
    for f in rnames.difference(&dnames) {
        out.push(format!(
            "基线登记了 `{f}`，但 `tests/` 里**找不到带标记的文件** ⇒ \
             它被删了 / 被改名了，或它的标记行被删了"
        ));
    }

    for rf in &reg.files {
        let Some(tests) = derived.get(&rf.file) else {
            continue;
        };
        let dt: BTreeMap<&str, &ParsedTest> = tests.iter().map(|t| (t.name.as_str(), t)).collect();
        let rt: BTreeMap<&str, &RegTest> = rf.tests.iter().map(|t| (t.name.as_str(), t)).collect();
        for name in dt.keys() {
            if !rt.contains_key(name) {
                out.push(format!("`{}`: `{name}` 实物里有、基线里没有 ⇒ 请回来登记", rf.file));
            }
        }
        for name in rt.keys() {
            if !dt.contains_key(name) {
                out.push(format!(
                    "`{}`: 基线登记了 `{name}`，实物里**已经找不到了** ⇒ \
                     那条守卫测试被删了或被改名了",
                    rf.file
                ));
            }
        }
    }
    out
}

/// **判据二 · 防改弱**：每条守卫测试的断言宏个数不得低于登记的下界。
fn weakness_problems(
    derived: &BTreeMap<String, Vec<ParsedTest>>,
    reg: &Registry,
) -> Vec<String> {
    const LABELS: [&str; 3] = ["assert_eq!", "assert_ne!", "assert!"];
    let mut out = Vec::new();
    for rf in &reg.files {
        let Some(tests) = derived.get(&rf.file) else {
            continue;
        };
        let dt: BTreeMap<&str, &ParsedTest> = tests.iter().map(|t| (t.name.as_str(), t)).collect();
        for rt in &rf.tests {
            let Some(d) = dt.get(rt.name.as_str()) else {
                continue; // 「被删掉」由判据一报，不重复
            };
            for k in 0..3 {
                if d.strengths()[k] < rt.floors[k] {
                    out.push(format!(
                        "`{}`: `{name}` 的 {label} 数从登记的 {floor} 掉到 {now} ⇒ \
                         **断言被删掉或被改弱**",
                        rf.file,
                        name = rt.name,
                        label = LABELS[k],
                        floor = rt.floors[k],
                        now = d.strengths()[k]
                    ));
                }
            }
        }
    }
    out
}

/// **判据三 · 边界措辞**：元守卫必须**仍然如实标注**递归终点与未封堵。
///
/// ⚠️ 措辞写在 [`BASELINE`] 里、**不在本文件** —— 写在本文件的话，
/// `GUARD_REQ081.contains(措辞)` 会因为**字面量自己就在文件里**而恒真（REQ-075 已踩过这个坑）。
fn phrase_problems(srcs: &BTreeMap<String, String>, reg: &Registry) -> Vec<String> {
    let mut out = Vec::new();
    for (file, id, text) in &reg.phrases {
        match srcs.get(file) {
            None => out.push(format!("`{file}` 不在守卫集合里 ⇒ `{id}` 无从核对")),
            Some(src) => {
                if !src.contains(text.as_str()) {
                    out.push(format!(
                        "`{file}` 里找不到必须如实标注的措辞 `{id}` = {text:?} —— \
                         ⚠️ 没有标注 ≠ 问题变小，只等于**没人知道缺口还在**"
                    ));
                }
            }
        }
    }
    out
}

/// 判据失败时给出的**可直接贴回**的基线正文。
fn render_baseline(derived: &BTreeMap<String, Vec<ParsedTest>>, reg: &Registry) -> String {
    let mut out = String::from("# REQ-081 · 守卫清点基线（由实物生成 —— 原样贴回本文件，保留上方注释头）\n");
    for (file, tests) in derived {
        out.push_str(&format!("FILE|{file}\n"));
        for t in tests {
            let [eq, ne, plain] = t.strengths();
            out.push_str(&format!("TEST|{file}|{}|{eq}|{ne}|{plain}\n", t.name));
        }
    }
    for (file, id, text) in &reg.phrases {
        out.push_str(&format!("PHRASE|{file}|{id}|{text}\n"));
    }
    for (file, text) in &reg.banned {
        out.push_str(&format!("BANNED|{file}|{text}\n"));
    }
    out
}

// ══════════════════════════════════════════════════════════
// AC-004 · 判别力自证 —— 证明上面三条判据**不是空转**
// ══════════════════════════════════════════════════════════

/// 合成夹具。⚠️ 它刻意同时含：注释里的断言、字符串里的断言与花括号、
/// 生命周期 `'a`、raw string —— 用来证明数的是**代码**不是文本。
/// ⚠️ 外层用 `r##"…"##` 而不是 `r#"…"#`：夹具里**内嵌**了一处 `r#"…"#`，
/// 两者用同一个定界符会在内层开头就被提前闭合（这会让夹具本身被腰斩）。
const FIXTURE_A: &str = r##"//! @guard-manifest: REQ-081
//! 合成夹具 A
#![allow(dead_code)]

/// 生命周期不该被当成字符字面量：x: &'a str
const KEEP: &str = "assert_eq!(不该被数)";

#[test]
fn test_req081_fixture_one() {
    let a = 1;
    assert_eq!(a, 1, "带说明的精确比较");
    assert_ne!(a, 2);
    assert!(a > 0, "{} 里的花括号", "被数的是花括号深度");
    debug_assert_eq!(a, 1);
}

#[test]
fn test_req081_fixture_two() {
    // 注释里的 assert_eq!(x, y) 不该被数进去
    /* 块注释里的 assert!(z) 也不该 */
    let b = r#"raw string 里的 assert_ne! 和 } 都不该被数"#;
    assert!(b.contains('a'));
}
"##;

/// 夹具 B：声明了 `mod hidden_guards;`。
const FIXTURE_B: &str = r#"//! @guard-manifest: REQ-081

mod hidden_guards;

#[test]
fn test_req081_fixture_one() {
    assert!(true);
}
"#;

/// 夹具 B 的配套件之一：那份文件里**有** `#[test]` ⇒ `mod` 真的把测试藏起来了。
const FIXTURE_HIDDEN: &str =
    "//! 配套夹具\n#[test]\nfn test_req081_fixture_hidden() {\n    assert!(true);\n}\n";

/// 夹具 B 的配套件之二：那份文件里**没有** `#[test]` ⇒ 不算藏（`mod common;` 就属这一类）。
const FIXTURE_PLAIN: &str = "//! 配套夹具\npub fn helper() -> u8 { 1 }\n";

fn fixture_map() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("fixture_a.rs".to_string(), FIXTURE_A.to_string());
    m
}

fn reg_one_test() -> Registry {
    // 与夹具 A 的 `test_req081_fixture_one` 完全对齐（floor = 实测值）
    let mut reg = Registry::default();
    reg.files.push(RegFile {
        file: "fixture_a.rs".to_string(),
        tests: vec![RegTest {
            name: "test_req081_fixture_one".to_string(),
            floors: [1, 1, 1],
        }],
    });
    reg
}

/// 【AC-004】三条判据**各自**在合成夹具上**证明自己会响**。
///
/// # 为什么要这一条
///
/// 判据恒返回空 ⇒ 永远绿 ⇒ 那正是「看起来有守卫、实则不拦任何东西」。
/// 照抄 `test_anchor_guard.rs::no_bare_env_manifest_dir_outside_anchor_file`
/// 里的 `hit()` 自检先例：**先证明判据能判红，再让它去判实物。**
#[test]
fn test_req081_every_criterion_is_proven_to_fire_on_synthetic_fixtures() {
    // ── 解析层：数的是**代码**，不是文本 ──────────────────────
    let parsed = parse_guard_tests(FIXTURE_A);
    assert_eq!(parsed.len(), 2, "夹具 A 里恰好两条 `#[test]`");
    let one = parsed.iter().find(|t| t.name == "test_req081_fixture_one").expect("夹具里那条");
    // `debug_assert_eq!` **不得**被数进 `assert_eq!`（词法边界那一步）
    assert_eq!((one.eq, one.ne, one.plain), (1, 1, 1), "带边界/带块的计数");
    let two = parsed.iter().find(|t| t.name == "test_req081_fixture_two").expect("夹具里那条");
    assert_eq!(
        (two.eq, two.ne, two.plain),
        (0, 0, 1),
        "🔴 注释里 / 字符串里的断言宏**不得**被数进去（否则 floor 是假的）"
    );
    // raw string 里的 `}` 不得把函数体的花括号深度带偏
    assert!(two.name.ends_with("two"));

    // ── 判据一：文件/测试集合对不上时**必须**报 ──────────────
    assert!(
        inventory_problems(&derive(&fixture_map()), &reg_one_test())
            .iter()
            .any(|p| p.contains("test_req081_fixture_two")),
        "🔴 实物里多出一条没登记的守卫测试，判据一必须报出来"
    );
    // 反向：删掉一条（实物里没有）⇒ 也必须报
    let mut shrunk = reg_one_test();
    shrunk.files[0].tests.push(RegTest {
        name: "test_req081_fixture_gone".to_string(),
        floors: [1, 1, 1],
    });
    assert!(
        inventory_problems(&derive(&fixture_map()), &shrunk)
            .iter()
            .any(|p| p.contains("test_req081_fixture_gone") && p.contains("找不到了")),
        "🔴 登记的守卫测试在实物里消失，判据一必须报出来"
    );
    // 标记被删 ⇒ 整个文件掉出守卫集合 ⇒ 必须报
    assert!(
        inventory_problems(&derive(&BTreeMap::new()), &reg_one_test())
            .iter()
            .any(|p| p.contains("fixture_a.rs") && p.contains("找不到带标记的文件")),
        "🔴 标记行被删 ⇒ 文件静默掉出守卫集合，判据一必须报出来"
    );

    // ── 判据二：`assert_eq!` → `assert!(true)` 必须被抓 ────────
    let mut weak_srcs = BTreeMap::new();
    weak_srcs.insert("fixture_a.rs".to_string(), FIXTURE_B.to_string());
    let weak = weakness_problems(&derive(&weak_srcs), &reg_one_test());
    assert!(
        weak.iter().any(|p| p.contains("assert_eq!") && p.contains("test_req081_fixture_one")),
        "🔴 把 `assert_eq!` 换成 `assert!(true)`（或直接删掉）必须被抓到，实测：{weak:?}"
    );
    assert!(
        weakness_problems(&derive(&fixture_map()), &reg_one_test()).is_empty(),
        "未变异时判据二必须**不报**（否则它恒红，等于没有）"
    );

    // ── `mod` 判据：**真的藏了 `#[test]`** 时必须被抓 ─────────
    let mut b_only = BTreeMap::new();
    b_only.insert("fixture_b.rs".to_string(), FIXTURE_B.to_string());
    let mut with_hidden = b_only.clone();
    with_hidden.insert("hidden_guards.rs".to_string(), FIXTURE_HIDDEN.to_string());
    assert!(
        !hidden_test_mods(&b_only, &with_hidden).is_empty(),
        "🔴 `mod` 指过去的那份文件里有 `#[test]` ⇒ 判据必须抓出来（否则删 `mod` 就能静默摘掉守卫）"
    );
    let mut with_plain = b_only.clone();
    with_plain.insert("hidden_guards.rs".to_string(), FIXTURE_PLAIN.to_string());
    assert!(
        hidden_test_mods(&b_only, &with_plain).is_empty(),
        "⚠️ 那份文件里**没有** `#[test]` ⇒ 不该报（本仓的 `mod common;` 就属这一类）"
    );
    assert!(
        hidden_test_mods(&fixture_map(), &fixture_map()).is_empty(),
        "⚠️ 夹具 A 没有 `mod` ⇒ 判据在它上面必须**不报**"
    );

    // ── 判据三：措辞不在文件里必须被抓 ──────────────────────
    let mut reg_phrase = reg_one_test();
    // ⚠️ 先用一句**夹具里确实有**的措辞证明「在位时不报」——
    //    否则这一格会把「恒假」误当成「通过」。
    reg_phrase.phrases.push((
        "fixture_a.rs".to_string(),
        "present".to_string(),
        "合成夹具 A".to_string(),
    ));
    assert!(
        phrase_problems(&fixture_map(), &reg_phrase).is_empty(),
        "夹具 A 里**确实**写着 `合成夹具 A` ⇒ 判据三在它上面必须不报，实测：{:?}",
        phrase_problems(&fixture_map(), &reg_phrase)
    );
    // 再换成一句**夹具里没有**的 ⇒ 必须报
    reg_phrase.phrases[0].2 = "本文件自己也能被删".to_string();
    assert_eq!(
        phrase_problems(&fixture_map(), &reg_phrase).len(),
        1,
        "🔴 实物里没有那句措辞 ⇒ 判据三必须报出来"
    );

    // ── 「过度声明」判据：源码里出现了禁用措辞必须被抓 ────────
    // ⚠️⚠️ 这里**刻意用一句合成串**而不是基线里登记的真禁用措辞：
    // 若把真措辞原样写在本文件里，它就会出现在**被扫描的那份源码**中
    // ⇒ 判据立刻变成恒红（本条第一版就是这么把自己判红的第二次）。
    // 本格要证明的是「禁用清单**确实被查了**」，不是那几个具体词。
    const SYNTH_BANNED: &str = "⟪合成过度声明探针⟫";
    let mut over_srcs = fixture_map();
    over_srcs.insert(
        "fixture_a.rs".to_string(),
        format!("{FIXTURE_A}\n//! {SYNTH_BANNED}\n"),
    );
    let banned = vec![("fixture_a.rs".to_string(), SYNTH_BANNED.to_string())];
    let hits: Vec<String> = over_srcs
        .iter()
        .filter_map(|(f, v)| banned.iter().find(|(bf, p)| bf == f && v.contains(p.as_str())).map(|(_, p)| p.clone()))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "🔴 守卫源码里出现了禁用措辞 ⇒ 「不许过度声明」这一格必须报出来，实测：{hits:?}"
    );
}

/// 剥掉 `mod` 前面可能有的可见性修饰。
fn strip_mod_vis(t: &str) -> Option<&str> {
    ["pub(crate) mod ", "pub mod ", "mod "]
        .into_iter()
        .find_map(|p| t.strip_prefix(p))
}

/// 守卫文件里「**真的会把 `#[test]` 藏起来**」的 `mod` 声明。
///
/// # 为什么不是「见到 `mod` 就报」
///
/// 第一版这里写的是「守卫文件里不许有任何 `mod` 声明」，**实测误报**：
/// 本文件自己要写 `mod common;`（判据读源码必须走 `tests/common` 的 `crate_root()`）。
/// ⇒ 那样等于逼判据为了过自己而别用公共设施，是**放宽不了的假阳性**。
///
/// # 现在钉的是「藏得住测试」这一个事实
///
/// - `mod NAME;` ⇒ 解析到同目录的 `NAME.rs` / `NAME/mod.rs`，那份文件里有 `#[test]` 才算；
/// - `mod NAME { … }` ⇒ 那个块里有 `#[test]` 才算。
///
/// ⇒ `mod common;`（那份文件里**没有** `#[test]`）不报；
///   「把守卫搬进 `mod guards { … }` 然后删掉 `mod`」报。
fn hidden_test_mods(
    guards: &BTreeMap<String, String>,
    all: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (file, src) in guards {
        let blank = blank_non_code(src);
        let lines: Vec<&str> = blank.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            if let Some(rest) = strip_mod_vis(lines[i].trim()) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let tail = rest[name.len()..].trim_start();
                if tail.starts_with(';') {
                    if let Some(target) = resolve_mod(file, &name, all) {
                        if target.lines().any(|l| l.trim() == "#[test]") {
                            out.push(format!("{file}: `mod {name};` 指过去的那份文件里有 `#[test]`"));
                        }
                    }
                } else if tail.starts_with('{') {
                    let end = body_end(&lines, i).min(lines.len() - 1);
                    if lines[i..=end].iter().any(|l| l.trim() == "#[test]") {
                        out.push(format!("{file}: `mod {name} {{ … }}` 里有 `#[test]`"));
                    }
                    i = end;
                }
            }
            i += 1;
        }
    }
    out
}

/// 从 `from_rel` 出发解析 `mod NAME;` 指向的那份文件。
fn resolve_mod<'a>(
    from_rel: &str,
    name: &str,
    all: &'a BTreeMap<String, String>,
) -> Option<&'a String> {
    let dir = from_rel
        .rsplit_once('/')
        .map(|(d, _)| format!("{d}/"))
        .unwrap_or_default();
    all.get(&format!("{dir}{name}.rs"))
        .or_else(|| all.get(&format!("{dir}{name}/mod.rs")))
}

// ══════════════════════════════════════════════════════════
// AC-001 · 判据一：登记的守卫清单必须与**实物**逐条相符
// ══════════════════════════════════════════════════════════

/// 【AC-001】基线登记的**守卫文件集合**与**每条守卫测试的名字**必须与实物逐条相等。
///
/// # 它抓的是
///
/// | 变异 | 结果 |
/// | --- | --- |
/// | 删掉**整条**守卫测试 | 🔴 本条红（实物里有、基线里没有 ⇒ 反向那一格） |
/// | 给守卫文件**改名** | 🔴 红（名字对不上） |
/// | 守卫文件被**删掉** | 🔴 红 **且** 本二进制**编译失败**（上面三条 `include_str!`） |
/// | 标记行被删 | 🔴 红（文件静默掉出守卫集合） |
/// | **新增**守卫测试不登记 | 🔴 红 |
///
/// # ⚠️ 它抓不到的
///
/// 删掉**本文件自己**（⇒ 真的构建失败，但那来自 REQ-075 的对称 `include_str!`，
/// 不来自本条）；把本条断言改成恒真（与删掉等价）。
#[test]
fn test_req081_the_registered_guard_inventory_matches_the_physical_tree() {
    let srcs = guard_sources();
    assert!(
        !srcs.is_empty(),
        "🔴 判据空转：`tests/` 里一个带 `{META_MARKER}` 标记的文件都没扫到 —— \
         本文件自己就该带标记 ⇒ 标记被删了，或扫描没生效"
    );

    let derived = derive(&srcs);
    let reg = parse_baseline(BASELINE);
    // ⚠️ 前提自检**并进**问题列表，而不是单独一条 assert：
    // 单独 assert 会把「基线还是空的（首次生成）」也变成**看不到重生成块**的死路。
    let mut problems = inventory_problems(&derived, &reg);
    if reg.files.len() < 2 {
        problems.push(format!(
            "基线里登记的守卫文件只有 {} 个 ⇒ 本层至少要钉住 REQ-075 与本文件",
            reg.files.len()
        ));
    }
    assert!(
        problems.is_empty(),
        "守卫清单与实物对不上（{n} 处）：\n  - {p}\n\n\
         这是「清单与实物漂移」—— 请把下面这段**原样**贴回 `tests/guard_baseline.req081.txt`\n\
         （登记是硬要求，但**由实物机械生成**，不是手打）：\n\n{b}",
        n = problems.len(),
        p = problems.join("\n  - "),
        b = render_baseline(&derived, &reg)
    );
}

// ══════════════════════════════════════════════════════════
// AC-003 · 判据二：守卫被**改弱**也要响
// ══════════════════════════════════════════════════════════

/// 【AC-003】每条守卫测试的**断言宏个数**不得低于登记的下界。
///
/// # ⛔ 覆盖边界（照抄模块注释的表，别读成「全封住了」）
///
/// 抓得住：删断言、`assert_eq!` → `assert!(true)`、把测试删空。
/// **抓不住**：`assert!(x == y || true)`、把断言挪进被改弱的辅助函数、
/// 被断言的那个**函数**本身被改坏。
///
/// 换句话说：**有限枚举抓不完「改弱」**，本条不是封堵。
#[test]
fn test_req081_guard_tests_have_not_been_weakened() {
    let srcs = guard_sources();
    let derived = derive(&srcs);
    let reg = parse_baseline(BASELINE);
    let problems = weakness_problems(&derived, &reg);

    // 前提自检：下界不得**全空**。全 0 的下界 = 恒真的判据。
    let floors_sum: usize = reg
        .files
        .iter()
        .flat_map(|f| f.tests.iter())
        .map(|t| t.floors.iter().sum::<usize>())
        .sum();
    assert!(
        floors_sum > 0,
        "🔴 判据空转：基线里所有断言数下界都是 0 ⇒ 「防改弱」这一格等于没写"
    );

    assert!(
        problems.is_empty(),
        "{n} 条守卫测试**被改弱或断言被删掉了**：\n  - {p}\n\n\
         ⛔ 抓不到的那些形态见 `tests/test_guard_manifest_req081.rs` 的模块注释表；\
         **本条不是封堵**。",
        n = problems.len(),
        p = problems.join("\n  - ")
    );
}

// ══════════════════════════════════════════════════════════
// AC-001 · 判据四：`mod` 声明这一格
// ══════════════════════════════════════════════════════════

/// 【AC-001 · `mod` 声明】守卫测试必须**都是顶层 `#[test]` fn**，不许藏在 `mod` 后面。
///
/// # 现状与为什么还要这条
///
/// 本仓四个守卫文件目前**没有任何一处 `mod` 藏着 `#[test]`** ⇒ 「删掉一条 `mod` 声明」
/// 这一格今天**等价于**「删掉整个文件」（已由 `include_str!` 覆盖）。
///
/// 那为什么还要写这条？因为它是一枚**绊线**：哪天有人把守卫测试搬进
/// `mod guards { … }`，删 `mod` 就能把一整批守卫**静默摘掉**而本层毫无反应。
/// ⇒ 本条在那一刻变红，逼他回来改判据，而不是让护栏悄悄少一块。
///
/// ⚠️ 判的是「**真的藏了 `#[test]`**」，不是「出现了 `mod`」——
/// 本文件自己就得写 `mod common;`，粗判会把自己判红（第一版的实测误报，见
/// [`hidden_test_mods`] 的注释）。
#[test]
fn test_req081_guard_tests_are_top_level_not_hidden_behind_a_mod() {
    let all = all_test_sources();
    let guards: BTreeMap<String, String> = all
        .iter()
        .filter(|(_, v)| v.lines().any(|l| l.trim() == META_MARKER))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert!(!guards.is_empty(), "🔴 判据空转：没扫到守卫文件");

    let offenders = hidden_test_mods(&guards, &all);

    assert!(
        offenders.is_empty(),
        "守卫测试被藏在 `mod` 后面：\n  - {p}\n\
         本层按「顶层 `#[test]` fn」清点 ⇒ `mod` 会让守卫被摘掉时本层**毫无反应**。\n\
         如果确实需要分组，请先回来改这一层判据。",
        p = offenders.join("\n  - ")
    );
}

// ══════════════════════════════════════════════════════════
// AC-001 · 递归问题：钉住那两条 `include_str!` 不是空话
// ══════════════════════════════════════════════════════════

/// 【AC-001 · 递归】本层与 REQ-075 **互钉**：三份锚点都必须是**真的那三份文本**。
///
/// # 递归问题的实测答案（AC-001 明确要求「明确回答」）
///
/// | 删掉谁 | 实测结果 |
/// | --- | --- |
/// | **REQ-075 那个元测试文件** | **构建失败**（本文件 `include_str!` 报 E0463）—— **M3 实测** |
/// | **本文件自己** | **构建失败**（REQ-075 那边有一份对称的 `include_str!`） |
/// | **基线文件** | **构建失败**（本文件 `include_str!`） |
/// | **三个一起删** | exit 0，**没有一条测试会红** ⇒ 递归到此为止 |
///
/// # 为什么这一条还要存在
///
/// `include_str!` 只保证「编译期有那份文本」，不保证「**指对了**文件」。
/// 照 REQ-075 `test_req075_this_meta_test_really_reads_the_guard_files`
/// 的做法：把「读到的确实是那一份」钉成**可执行断言**。
#[test]
fn test_req081_the_compile_time_anchors_really_are_the_guard_files() {
    // 三份锚点都不是空的 —— 空文件会让下面的 `contains` 在空跑的路上假装通过
    for (name, text) in [
        ("test_review_view_req075.rs", GUARD_REQ075),
        ("test_guard_manifest_req081.rs", GUARD_REQ081),
        ("guard_baseline.req081.txt", BASELINE),
    ] {
        assert!(
            text.len() > 500,
            "锚点 `{name}` 只剩 {} 字节 ⇒ 它被清空了，「锚点还在」必须重新确认",
            text.len()
        );
    }

    // 「指对了」：逐字核对模块文档首行（REQ-075 自己那套做法）
    assert!(
        GUARD_REQ075.contains("//! REQ-075 · **守卫的守卫**"),
        "读到的 REQ-075 文本首行不对 ⇒ [`include_str!`] 指到了别的东西，\
         本层在钉一份**没在跑**的守卫"
    );
    assert!(
        GUARD_REQ081.contains("//! REQ-081 · **守卫的守卫 · 第二层**"),
        "读到的 REQ-081 文本首行不对 ⇒ [`include_str!`] 指到了别的东西"
    );
    // 互钉的那一格：REQ-075 里**必须**有指着本文件的锚点。
    // 少了它，删掉本文件就只剩 exit 0 —— 那正是 AC-001 要治的那一格。
    assert!(
        GUARD_REQ075.contains("include_str!(\"test_guard_manifest_req081.rs\")"),
        "🔴 REQ-075 里没有指向本文件的 `include_str!` ⇒ **删掉本文件会重新变成无痕**。\n\
         请在 `tests/test_review_view_req075.rs` 里补上那一行。"
    );
}

// ══════════════════════════════════════════════════════════
// AC-004 · 边界措辞（⛔ 不许被改写成「已封堵」）
// ══════════════════════════════════════════════════════════

/// 【AC-004】元守卫必须**仍然如实标注**递归终点与「未封堵」。
///
/// 照 REQ-075 `test_req075_guard_docs_still_mark_the_alias_boundary_as_unclosed`
/// 的做法：钉**具体几串**措辞，而不是「不许有强断言」（后者没法执行）。
#[test]
fn test_req081_the_recursion_limit_is_still_declared_honestly() {
    let srcs = guard_sources();
    assert!(!srcs.is_empty(), "🔴 判据空转：没扫到守卫文件");

    let reg = parse_baseline(BASELINE);
    assert!(
        !reg.phrases.is_empty(),
        "🔴 判据空转：基线里一条 `PHRASE` 都没有 ⇒ 「如实标注」这一格等于没写"
    );
    assert!(
        !reg.banned.is_empty(),
        "🔴 判据空转：基线里一条 `BANNED` 都没有 ⇒ 「不许过度声明」这一格等于没写"
    );

    // ⛔ 过度声明：这几串**不得**出现。
    //
    // ⚠️⚠️ **为什么不把禁用措辞写成源码字面量**（本条第一版就是这么把自己判红的）：
    // 一条「某串话不得出现在守卫文件里」的判据，只要它自己或任何一份守卫文件里
    // **提到过**那串话，它就恒红。REQ-075 已经踩过一次：
    // 它的 `BANNED` 列表里写着 `本条已封堵别名绕过`，而「⛔ 不许写『本条已封堵别名绕过』」
    // 那句警告**也**在同一份文件里 ⇒ 「全文找那串话」必然命中。
    //
    // ⇒ 做法：**禁用措辞存放在基线文件里**（`tests/**/*.rs` 之外的 `.txt`），
    //   扫描的守卫源码里**永远不会有它的字面量**，判据因此不自指。
    //   代价：选词要避开**已经存在**于守卫文件里的串（否则是假阳性）。
    let mut over = Vec::new();
    for (file, phrase) in &reg.banned {
        match srcs.get(file) {
            None => over.push(format!("{file} 不在守卫集合里（无法核对 `{phrase}`）")),
            Some(src) => {
                if src.contains(phrase.as_str()) {
                    over.push(format!("{file}: 出现了过度声明 {phrase:?}"))
                }
            }
        }
    }
    assert!(
        over.is_empty(),
        "守卫文件里出现了**过度声明**：\n  - {}\n\
         ⛔ 本仓已两次判过这类说法不成立。别把这一层读成封堵。",
        over.join("\n  - ")
    );

    let problems = phrase_problems(&srcs, &reg);
    assert!(
        problems.is_empty(),
        "如实标注不见了：\n  - {}\n\
         ⚠️ 没有标注 ≠ 问题变小，只等于**没人知道缺口还在**。",
        problems.join("\n  - ")
    );
}