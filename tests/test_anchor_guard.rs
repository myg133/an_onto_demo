//! REQ-047 · **判据守卫三层** —— 目标是「**堵住今天的修复被无声还原**」，
//! 不是「防止将来写坏代码」。
//!
//! # 为什么这三条判据必须存在（不是「防御性编程」）
//!
//! REQ-046 修好了「判据锚点依赖编译期目录」，QA 实跑发现**两条存活变异**：
//!
//! | 变异 | 做什么 | 全量结果 |
//! | --- | --- | --- |
//! | **M1** | 删掉 `candidate_roots()` 的 cwd 分支，锚点退回编译期常量（**= 还原 REQ-045**） | **536/0/1，退出码 0** 🔴 |
//! | **M2** | 类别 B 抬头替换成类别 A 文案（两类合并） | **536/0/1，退出码 0** 🔴 |
//!
//! ⚠️ **M1 比原始事故更危险**：原始事故至少**红 30 条**；M1 是**全绿** ——
//! 修复被还原后没有任何信号。两条都**不是等价变异**，理由见各自判据的注释。
//!
//! # 三层分工
//!
//! | 层 | 判据 | 杀谁 | 机制 |
//! | --- | --- | --- | --- |
//! | **G-2 行为层** | [`anchor_order_is_cwd_first_and_walk_is_bounded`] | **M1** | 断言**来源顺序**与**上界**，与取值无关 |
//! | **G-3 契约层** | [`two_error_categories_stay_distinct`] | **M2** | 两段抬头抽成**纯函数**，断言互不相等 + 判别短语 |
//! | **G-1 语法层** | [`no_bare_env_manifest_dir_outside_anchor_file`] | 新增裸调用 | **词法级**扫 `tests/**/*.rs` 的**代码行** |
//!
//! # 全部三条都**不碰进程级全局**
//!
//! 本文件**一处 `set_current_dir` 都没有**。它会污染同二进制内的并行测试，
//! 而事故恰恰是「在别的目录跑」的语义 —— 用它来测反而会把测试环境搞脏。
//! G-2/G-3 因此把被测逻辑抽成**吃参数的纯函数**
//! （[`walk_up_from`] / [`order_candidates`] / 两个抬头函数），
//! 拿**构造出来的路径**断言，**不需要真的换目录**。
//! ⇒ 独立测试二进制**只在 G-1 需要**（它要遍历 `tests/` 目录），
//! 而 G-1 不碰全局状态，两者互不冲突。

mod common;

use common::source_root::{
    candidate_roots, header_discriminators, header_mismatch, header_missing_file, order_candidates,
    walk_up_from, RootOrigin, MAX_WALK_UP,
};
use std::path::{Path, PathBuf};

// ══════════════════════════════════════════════════════════════
// G-2 · 行为层：锚点顺序 + 上界
// ══════════════════════════════════════════════════════════════

/// **杀 M1**：锚点必须**先试运行时 cwd**、编译期常量只做兜底，且上溯**有上界**。
///
/// # 为什么断言「顺序」而不是「取值」
///
/// `cargo test` 起的测试进程 **cwd 恒等于包根** ⇒
/// `crate_root_from_cwd()` 与 `BUILD_MANIFEST_DIR` 在**正常路径上取值相同**
/// （`source_root.rs` 文件头自己就这么论证）。⇒ **任何逐行/逐值相等的断言
/// 在 M1 下照样全绿 —— 这正是 M1 存活的原因**。
///
/// 所以这里断言的是**与取值无关的性质**：候选列表里**来源标签的次序**。
/// `RootOrigin` 是 REQ-047 为此加的 —— 没有它，「顺序」在取值相同时不可观测。
///
/// ⚠️ 归一化后做**顺序断言**（比较 `find()` 下标），**不做逐行相等**。
#[test]
fn anchor_order_is_cwd_first_and_walk_is_bounded() {
    // ── (a) 纯函数：两个候选**取值不同**时的次序 ────────────────
    // 用假路径 ⇒ 两条候选的路径**真的不同** ⇒ `find()` 下标可比。
    let fake_cwd = PathBuf::from("/__fake_cwd_root__");
    let ordered = order_candidates(Some(fake_cwd.clone()), true);

    assert_eq!(
        ordered.len(),
        2,
        "cwd 与编译期目录都在时，候选应当有两条"
    );
    assert_eq!(ordered[0].origin, RootOrigin::Cwd, "第 0 条必须是运行时 cwd");
    assert_eq!(
        ordered[1].origin,
        RootOrigin::BuildManifest,
        "编译期常量只做**兜底**，排第二"
    );

    // 顺序断言：cwd 的下标必须严格小于编译期兜底的下标。
    let i_cwd = ordered
        .iter()
        .position(|c| c.path == fake_cwd)
        .expect("cwd 候选应当在列表里");
    let i_build = ordered
        .iter()
        .position(|c| c.origin == RootOrigin::BuildManifest)
        .expect("编译期兜底应当在列表里");
    assert!(
        i_cwd < i_build,
        "cwd 必须在编译期常量**之前**被试（M1 就是把它删掉或挪后）"
    );

    // ── (b) 纯函数：cwd 定位不到时**不得凭空造出 Cwd 条目** ──────
    let no_cwd = order_candidates(None, true);
    assert_eq!(no_cwd.len(), 1);
    assert_eq!(no_cwd[0].origin, RootOrigin::BuildManifest);

    // ── (c) 真实接线 ─────────────────────────────────────────
    // ⚠️ 这一段才是**真正杀 M1** 的地方：M1 改的是 `candidate_roots()`
    // 里的 cwd 分支，(a)/(b) 是纯函数、直接传参，**碰不到那处接线**。
    let real = candidate_roots();
    assert!(
        !real.is_empty(),
        "cargo test 下 cwd 就是包根，crate 根必然定位得到；\
         若为空说明锚点定位整体坏了"
    );
    assert_eq!(
        real[0].origin,
        RootOrigin::Cwd,
        "🔴 M1 在这里被杀：真实候选的第 0 条必须来自运行时 cwd。\
         若它是 BuildManifest，说明 cwd 分支被删/被挪到兜底之后 —— \
         锚点已退回编译期常量，等于把 REQ-046 还原掉了"
    );
    if let Some(i) = real.iter().position(|c| c.origin == RootOrigin::Cwd) {
        assert_eq!(i, 0, "Cwd 候选一旦存在，就必须排在最前");
    }

    // ── (d) 上溯有上界 ───────────────────────────────────────
    assert!(
        (1..=32).contains(&MAX_WALK_UP),
        "MAX_WALK_UP 必须是个小而明确的上界（实际 {MAX_WALK_UP}）；\
         调大等于把「上溯无界」这个老洞重新打开"
    );

    // 构造一条深路径，证明**上界是起作用的**，而不只是一个好看的常量。
    // 起点深 11 层；两个「假 crate 根」：深度 5 处与文件系统根 `/`。
    let start = Path::new("/a/b/c/d/e/f/g/h/i/j/k");
    let fake_root = Path::new("/a/b/c/d/e/f");
    let root_dir = Path::new("/");
    let is_any_fake = |p: &Path| p == fake_root || p == root_dir;

    // 上界之内：够得着深度 5 的那个。
    assert_eq!(
        walk_up_from(start, MAX_WALK_UP, is_any_fake),
        Some(fake_root.to_path_buf()),
        "MAX_WALK_UP={MAX_WALK_UP} 之内应当够得到深度 5 的根"
    );
    // 上界之外：**够不着** —— 这才是「有上界」的可观测后果。
    assert_eq!(
        walk_up_from(start, 3, is_any_fake),
        None,
        "上界 3 时不该够到深度 5 的根"
    );
    // 若**没有**上界（usize::MAX），它会一路爬到**文件系统根** `/`
    // （`/` 在深度 11，超出 MAX_WALK_UP=8）—— 证明上界**真的在拦**，
    // 而不是恰好没触发：上界之内返回 None，去掉上界就返回 Some("/")。
    let is_fs_root = |p: &Path| p == Path::new("/");
    assert_eq!(
        walk_up_from(start, MAX_WALK_UP, is_fs_root),
        None,
        "有上界时不该爬到文件系统根"
    );
    assert_eq!(
        walk_up_from(start, usize::MAX, is_fs_root),
        Some(PathBuf::from("/")),
        "无上界时会爬到 `/`：这正是 MAX_WALK_UP 存在的理由"
    );
}

// ══════════════════════════════════════════════════════════════
// G-3 · 契约层：两类报错互不相等
// ══════════════════════════════════════════════════════════════

/// **杀 M2**：类别 A 与类别 B 的抬头必须**互不相等**，且各自带着自己的**判别短语**。
///
/// # 为什么 M2 能活
///
/// REQ-046 把两类报错分开了，但**全仓零可执行断言**去区分它们，
/// 且 panic 分支在正常路径下永不进入。⇒ 把 B 的抬头整个换成 A 的文案
/// （即「两类合并」，**回事故里最有害的那条**）是 536/0/1。
///
/// # 本判据怎么绕开 `set_current_dir`
///
/// **不绕、不用**。两段抬头已抽成**不吃任何环境状态的纯函数**
/// （`header_missing_file()` / `header_mismatch()`），测试直接调用即可，
/// **不需要切换目录、不需要独立二进制、也不需要碰进程级全局**。
#[test]
fn two_error_categories_stay_distinct() {
    let a = header_missing_file();
    let b = header_mismatch();

    // ① 两者互不相等
    assert_ne!(
        a, b,
        "🔴 M2 在这里被杀：两类抬头合并成同一段 ⇒ 「目录不对」被读成「文件被删了」"
    );

    // ② 各含自己的判别短语
    let a_phrase = header_discriminators()
        .iter()
        .find(|(tag, _)| *tag == "A")
        .map(|(_, p)| *p)
        .expect("应登记类别 A 的判别短语");
    let b_phrase = header_discriminators()
        .iter()
        .find(|(tag, _)| *tag == "B")
        .map(|(_, p)| *p)
        .expect("应登记类别 B 的判别短语");

    assert!(a.contains(a_phrase), "类别 A 抬头须含判别短语「{a_phrase}」");
    assert!(
        b.contains(b_phrase),
        "类别 B 抬头须含判别短语「{b_phrase}」"
    );

    // ③ 判别短语**互不串台** —— 这一条直接钉死 M2
    assert!(
        !a.contains(b_phrase),
        "类别 A 的抬头里不该出现类别 B 的判别短语「{b_phrase}」"
    );
    assert!(
        !b.contains(a_phrase),
        "🔴 M2 在这里也被杀：类别 B 的抬头里出现了类别 A 的判别短语「{a_phrase}」，\
         说明两类被合并了"
    );

    // ④ 两段抬头都应是「抬头」形状：以 lead 句 + 【失败类型】开头，以换行收尾
    for (tag, h) in [("A", a), ("B", b)] {
        assert!(
            h.contains("【失败类型】"),
            "类别 {tag} 抬头须含【失败类型】小节"
        );
        assert!(h.ends_with('\n'), "类别 {tag} 抬头须以换行收尾");
    }
}

// ══════════════════════════════════════════════════════════════
// G-1 · 语法层：禁止新增裸 env!("CARGO_MANIFEST_DIR")
// ══════════════════════════════════════════════════════════════

/// 允许出现裸编译期锚点的**唯一**文件（相对 crate 根）。
const ANCHOR_FILE: &str = "tests/common/source_root.rs";
/// 那一行必须带的标记。
const ANCHOR_MARKER: &str = "ANCHOR-ALLOW";

/// 判定用的**针**。**刻意拆成四段拼** —— 若把它原样写在本文件里，
/// 本文件自己就会被 G-1 扫成违规（见下方「自检」：字符串字面量**不是**注释，
/// 词法器不会把它剥掉）。拼起来既让判据自洽，也顺带证明判据不是靠豁免自己过的。
fn needle() -> String {
    ["env", "!", "(", "\"CARGO_MANIFEST_DIR\""].concat()
}

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
/// # 为什么裸 grep 不够（本会话已因此误报两次）
///
/// 一次扫出「36 个残留」实为 0（命中的是注释里记录封口历史的字面量）；
/// 一次把注释里的字面量当成代码。⇒ 判据必须**先把注释剥掉**。
///
/// 要剥干净，就必须处理这些：
/// - `//` 行注释、**可嵌套**的 `/* */` 块注释；
/// - `'` 到底是**字符字面量**（`'x'`）还是**生命周期**（`&'a T`）——
///   误判成字符会让后面的 `//` 被当成字符串内容，剥不掉；
/// - raw string `r"…"` / `r#"…"#`，它的收尾是 `"` + 若干 `#`；
/// - 字符串里的 `\n\` 续行（本仓 `source_root.rs` 全靠它拼多行报错）。
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
                    // r"…" / r#"…"# / r##"…"##
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
                '\'' => {
                    // 字符字面量：`'x'` 或 `'\…'`；否则是生命周期/标签，当普通字符。
                    let is_char = cs.get(i + 1) == Some(&'\\') || cs.get(i + 2) == Some(&'\'');
                    if is_char {
                        out.push(c);
                        i += 1;
                        st = Lex::Char;
                    } else {
                        out.push(c);
                        i += 1;
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
                    // 注释里的换行要**保留**，否则行号会错位
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
                    if c == '\'' {
                        st = Lex::Code;
                    } else if c == '\n' {
                        st = Lex::Code; // 畸形输入，别把后面全吞掉
                    }
                }
            }
        }
    }

    out.split('\n').map(|s| s.to_string()).collect()
}

/// **全文**去空白（跨行归一），并记录每个字符来自第几行。
///
/// 为什么必须**全文**而不是逐行：实参跨行的写法
/// （`env!(\n    "CARGO_MANIFEST_DIR"\n)`）逐行看，命中行上只有 `env!(`、
/// 没有实参 ⇒ **漏报**。把换行也当空白去掉，两半就重新接上了。
fn squeeze_indexed(lines: &[String]) -> (Vec<char>, Vec<usize>) {
    let mut chars = Vec::new();
    let mut owner = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        for c in line.chars() {
            if c.is_whitespace() {
                continue;
            }
            chars.push(c);
            owner.push(li);
        }
    }
    (chars, owner)
}

/// 找出所有「以编译期锚点常量为实参的 `env!`」所在的**行号**（0 基）。
///
/// # 为什么查「`env!(` + 实参窗口」而不是查一整条固定子串
///
/// 查 `env!("CARGO_MANIFEST_DIR"` 会被 **raw string** 写法绕过
/// （`env!(r#"CARGO_MANIFEST_DIR"#)`）—— 而 raw string 在本仓判据里**确实出现过**
/// （REQ-045 专门登记过这条局限）。改成「`env!(` 之后一个小窗口里有没有那个常量名」
/// 即可一并覆盖普通字符串、raw string、跨行、尾逗号与 `option_env!`。
fn anchor_hit_lines(lines: &[String]) -> Vec<usize> {
    /// 实参窗口长度：足以覆盖 `r###"CARGO_MANIFEST_DIR"###` 这类写法
    const ARG_WINDOW: usize = 40;
    const HEAD: [char; 5] = ['e', 'n', 'v', '!', '('];

    let (sq, owner) = squeeze_indexed(lines);
    let mut hits = Vec::new();
    if sq.len() < HEAD.len() {
        return hits;
    }
    for i in 0..=sq.len() - HEAD.len() {
        if sq[i..i + HEAD.len()] != HEAD {
            continue;
        }
        let start = i + HEAD.len();
        let end = (start + ARG_WINDOW).min(sq.len());
        if sq[start..end].iter().collect::<String>().contains("CARGO_MANIFEST_DIR") {
            hits.push(owner.get(i).copied().unwrap_or(0));
        }
    }
    hits
}

/// 该行（含注释）经「剥注释 → 去空白」后，**代码部分**是否含裸编译期锚点。
fn code_line_hits(line: &str) -> bool {
    // `split('\n')` 恒返回至少一个元素，故 `.first()` 必为 `Some`
    !anchor_hit_lines(&strip_comments(line)).is_empty()
}

/// 递归收集 `tests/` 下的 `.rs` 文件（相对 crate 根的路径）。
fn collect_test_sources(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort(); // 稳定 ⇒ 报错顺序可复现
    for p in items {
        if p.is_dir() {
            collect_test_sources(&p, acc);
        } else if p.extension().is_some_and(|e| e == "rs") {
            acc.push(p);
        }
    }
}

/// **G-1**：裸 `env!("CARGO_MANIFEST_DIR")` 只许出现在 [`ANCHOR_FILE`]
/// 且该行须带 [`ANCHOR_MARKER`] 标记。
///
/// # 为什么必须**词法级**
///
/// 裸 `grep 'CARGO_MANIFEST_DIR'` 会被三样东西绕过/误报：
/// 1. `env!( "…" )` 的**空格**、换行、尾逗号 —— 归一化空白后一并归一；
/// 2. `std::env::var(...)` / `option_env!(...)`；
/// 3. **注释**里的字面量 —— 本会话已因此误报两次，**必须先剥注释**。
#[test]
fn no_bare_env_manifest_dir_outside_anchor_file() {
    let needle = needle();
    let root = common::crate_root();
    let tests_dir = root.join("tests");
    assert!(
        tests_dir.is_dir(),
        "找不到 tests/ 目录（crate 根 = {}）",
        root.display()
    );

    // ── 自检：证明判据不是空转 ─────────────────────────────────
    // 这几条**必须**被判成命中 / 不命中。若判据恒返回空，
    // 它会「全绿」—— 那就是 M1 的同一种病：看起来有守卫、实则不拦任何东西。
    let hit = |s: &str| code_line_hits(s);
    assert!(hit(&format!("const A: &str = {needle};")), "直白写法应命中");
    assert!(
        hit(&format!("const B: &str = {};", needle.replace("(", " ( "))),
        "带空格应命中（空白归一化）"
    );
    assert!(
        hit(&format!("const C: &str = {needle}, 1);")),
        "带尾逗号/多余实参应命中"
    );
    assert!(
        hit(&format!("const D: &str = option_{needle};")),
        "option_env! 是同一个洞，应命中"
    );
    assert!(
        hit(&format!(
            "const E: &str = {};",
            needle.replace("\"CARGO", "r#\"CARGO").replace("DIR\"", "DIR\"#")
        )),
        "raw string 写法应命中（本仓判据里出现过这种写法）"
    );
    assert!(
        hit(&format!(
            "const F: &str = {});",
            needle.replace(
                "(\"CARGO_MANIFEST_DIR\"",
                "(\n    // 实参说明：跨行 + 实参内部带注释\n    \"CARGO_MANIFEST_DIR\""
            )
        )),
        "实参跨行、且实参内部带注释，应命中"
    );
    assert!(
        !hit(&format!("// {needle}")),
        "🔴 注释里的字面量**必须**被剥掉（本会话误报过两次）"
    );
    assert!(
        !hit(&format!("/* {needle} */")),
        "🔴 块注释里的字面量**必须**被剥掉"
    );
    assert!(
        !hit(&format!("/* 外层 /* {} */ 仍在内层 */", needle.replace("(", " ("))),
        "🔴 嵌套块注释必须整体剥掉"
    );

    // ── 实扫 ─────────────────────────────────────────────────
    let mut files = Vec::new();
    collect_test_sources(&tests_dir, &mut files);
    assert!(!files.is_empty(), "没扫到任何 .rs，判据就是空转");

    let mut violations: Vec<String> = Vec::new();
    let mut allowed_hits = 0usize;

    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        let rel = f
            .strip_prefix(&root)
            .unwrap_or(f.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let stripped = strip_comments(&src);
        let raw_lines: Vec<&str> = src.lines().collect();

        for idx in anchor_hit_lines(&stripped) {
            // 行号从整篇文件扫出来，与 `src.lines()` 对齐
            let raw = raw_lines.get(idx).copied().unwrap_or("");
            if rel == ANCHOR_FILE && raw.contains(ANCHOR_MARKER) {
                allowed_hits += 1;
            } else {
                violations.push(format!(
                    "{rel}:{}  —— 裸编译期锚点只允许出现在 {ANCHOR_FILE} 且该行带 \
                     // {ANCHOR_MARKER}；请改走 `common::source_root::read_source` / \
                     `crate_root`（运行时定位）",
                    idx + 1
                ));
            }
        }
    }

    // 反空转：判据必须**至少**认得那条被允许的锚点行。
    assert!(
        allowed_hits >= 1,
        "🔴 判据空转：一条 {ANCHOR_MARKER} 都没认出来，G-1 等于没写"
    );

    assert!(
        violations.is_empty(),
        "发现 {} 处未获准的裸编译期锚点：\n  - {}",
        violations.len(),
        violations.join("\n  - ")
    );
}