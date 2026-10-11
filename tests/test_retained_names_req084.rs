//! REQ-084 · 客户源库表名「保留项」登记的元测试 —— 登记的守卫
//!
//! # 这层补的是哪一格
//!
//! REQ-084 脱敏后，代码里**仍然保留着 199 处真实表名**：
//! 160 `L` + 19 `L-改判` + 20 `R`，全部已裁定，`O-待裁` 清零。
//! 每一处都必须能回答「**为什么不能改**」。答案集中登记在
//! [`REGISTRY`] —— **单一登记点**，不是散落在注释里的善意。
//!
//! 没有这层的话，「登记」只是一份没人看的文档：改代码的人看不到它，
//! 评审的人也很难逐条核。所以这里把它钉死。
//!
//! # 三道锁
//!
//! | 锁 | 手段 | 抓什么 |
//! | --- | --- | --- |
//! | ① **构建失败** | [`REGISTRY`] 用 `include_str!` | 删掉登记文件 |
//! | ② **实物核对** | [`test_req084_every_registered_name_is_still_literally_at_its_line`] | 改了代码里被登记的名字，却没改登记 |
//! | ③ **反向核对 + 计数下界** | [`test_req084_no_unregistered_real_name_remains_in_the_registered_files`]、[`test_req084_registry_totals_match_the_floor`] | 偷偷删登记行；代码里新添一处真名不登记 |
//!
//! # ⛔ 覆盖边界（**不声称封堵**）
//!
//! | 形态 | 抓得住吗 |
//! | --- | --- |
//! | 删掉本登记文件 | ✅ ① `include_str!` **构建失败** |
//! | 改代码里被登记的名字，不改登记 | ✅ ② 变红 |
//! | 删登记行来掩盖改动 | ✅ ③ 计数下界变红 |
//! | 代码里新添一处真名，不登记 | ✅ ③ 反向核对变红 |
//! | **把本文件也删掉** | ❌ ①②③ 一起消失，回退到「无痕」 |
//! | 把本文件的断言改成恒真 | ❌ |
//! | 同时删本文件与登记文件 | ❌ 与「删掉本文件」同 |
//!
//! ⇒ **递归到此为止**，与 REQ-075 / REQ-081 同一递归终点。
//! 本层**不声称封堵**任何东西；它只把「静默删改保留项」从**无痕**
//! 变成**构建失败 / 变红**。
//!
//! # ⚠️ 本文件**不得**硬编码任何真实表名
//!
//! 全部表名一律从 [`REGISTRY`] 读。原因与 REQ-081 记的是同一条坑：
//! 措辞/字面量写进判据源码里，「判据读到的文本含那句表名」会因为
//! **字面量自己就在这个文件里**而恒真 —— 判据自己给自己作证。
//! ⇒ 表名只存在登记文件里，判据只负责比对。
//!
//! 本文件**不碰数据库**（不 `mod common`），任何环境都能跑。

/// 登记本体。**删掉它 ⇒ 下面这行 `include_str!` 编译失败 ⇒ 构建失败。**
const REGISTRY: &str = include_str!("retained_names.req084.tsv");

// ── 被登记的文件本体 ───────────────────────────────────────────────
// 用 `include_str!` 而不是运行时读盘：判据必须对着**编译进去的那一份**，
// 顺带再钉一道 —— 移动/删除任一源文件 ⇒ 构建失败。
const SRC_PREFLIGHT: &str = include_str!("../src/bin/ontok-gate-preflight.rs");
const SRC_CONCEPT: &str = include_str!("../src/domain/concept.rs");
const SRC_HTML_VIEW: &str = include_str!("../src/domain/html_view.rs");
const SRC_IDENT: &str = include_str!("../src/domain/identifier_shape.rs");
const SRC_REVIEW: &str = include_str!("../src/domain/review.rs");
const SRC_VIEW: &str = include_str!("../src/domain/view.rs");
const SRC_PROPOSE: &str = include_str!("../src/extract/propose.rs");
const T_FK_NAMING: &str = include_str!("test_fk_naming.rs");
const T_MCP_SERVER: &str = include_str!("test_mcp_server.rs");
const T_OVERVIEW: &str = include_str!("test_overview.rs");
const T_PROP_QUALITY: &str = include_str!("test_prop_name_quality.rs");
const T_SNAPSHOT_PATHS: &str = include_str!("test_snapshot_write_paths.rs");
/// 红线文件，`drop_rate.rs` **登记 0 处** —— 它从头到尾没有出现过客户表名。
const T_DROP_RATE: &str = include_str!("drop_rate.rs");

/// (相对仓库根的路径, 文件内容)
fn sources() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/bin/ontok-gate-preflight.rs", SRC_PREFLIGHT),
        ("src/domain/concept.rs", SRC_CONCEPT),
        ("src/domain/html_view.rs", SRC_HTML_VIEW),
        ("src/domain/identifier_shape.rs", SRC_IDENT),
        ("src/domain/review.rs", SRC_REVIEW),
        ("src/domain/view.rs", SRC_VIEW),
        ("src/extract/propose.rs", SRC_PROPOSE),
        ("tests/test_fk_naming.rs", T_FK_NAMING),
        ("tests/test_mcp_server.rs", T_MCP_SERVER),
        ("tests/test_overview.rs", T_OVERVIEW),
        ("tests/test_prop_name_quality.rs", T_PROP_QUALITY),
        ("tests/test_snapshot_write_paths.rs", T_SNAPSHOT_PATHS),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Row {
    kind: String,
    file: String,
    line: usize,
    col: usize,
    name: String,
    crit: String,
    why: String,
}

fn registry() -> Vec<Row> {
    let mut out = Vec::new();
    for (i, raw) in REGISTRY.lines().enumerate() {
        if raw.starts_with('#') || raw.trim().is_empty() || raw.starts_with("KIND\t") {
            continue;
        }
        let f: Vec<&str> = raw.split('\t').collect();
        assert_eq!(
            f.len(),
            7,
            "登记表第 {} 行必须 7 列（KIND/FILE/LINE/COL/NAME/CRIT/WHY），实得 {} 列：\n{}",
            i + 1,
            f.len(),
            raw
        );
        let num = |s: &str, col: &str| -> usize {
            s.parse()
                .unwrap_or_else(|e| panic!("登记表第 {} 行 {} 不是整数 `{}`：{e}", i + 1, col, s))
        };
        out.push(Row {
            kind: f[0].into(),
            file: f[1].into(),
            line: num(f[2], "LINE"),
            col: num(f[3], "COL"),
            name: f[4].into(),
            crit: f[5].into(),
            why: f[6].into(),
        });
    }
    assert!(!out.is_empty(), "登记表是空的 —— 登记没了就等于没有保留项这回事");
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// 数 `name` 在 `hay` 里出现的次数，边界与扫描器口径一致：
/// `(?<![A-Za-z0-9_])…(?![A-Za-z0-9_])`。
///
/// ⛔ **不是** `\b` —— `\b` 在 CJK 旁不成立（`应` 被当作 `\w`），
/// REQ-084 已因此在 rev.1 里错报过一次（682 vs 695）。
fn occurrences(hay: &str, name: &str) -> usize {
    let mut n = 0;
    for (i, _) in hay.match_indices(name) {
        let before_ok = i == 0 || !is_word(hay[..i].chars().next_back().unwrap());
        let end = i + name.len();
        let after_ok = end >= hay.len() || !is_word(hay[end..].chars().next().unwrap());
        if before_ok && after_ok {
            n += 1;
        }
    }
    n
}

/// 登记表里出现过的全部表名（去重、字典序）。
fn distinct_names(rows: &[Row]) -> Vec<&str> {
    let mut v: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// 第 `col` 字节列（1-based）上是否**恰好**是这个表名，且左右词边界成立。
fn name_at(line: &str, col: usize, name: &str) -> bool {
    let Some(i) = col.checked_sub(1) else { return false };
    if i + name.len() > line.len() || !line.is_char_boundary(i) || !line.is_char_boundary(i + name.len()) {
        return false;
    }
    if &line[i..i + name.len()] != name {
        return false;
    }
    let before_ok = i == 0 || !is_word(line[..i].chars().next_back().unwrap());
    let end = i + name.len();
    let after_ok = end >= line.len() || !is_word(line[end..].chars().next().unwrap());
    before_ok && after_ok
}

fn line_of(hay: &str, line: usize) -> &str {    hay.lines().nth(line - 1).unwrap_or_else(|| {
        panic!(
            "登记表指向的行不存在（文件只有 {} 行）—— 多半是有人在被登记的位置上方插/删了行",
            hay.lines().count()
        )
    })
}

/// 登记里出现的每一类，各应有多少处。**数字手写**：它是与登记文件的
/// 独立第二来源，两者相等才有意义（若登记由实物生成，这个等式恒成立，
/// 判据即失效 —— REQ-081 记过同一条权衡）。
const FLOOR_KIND: &[(&str, usize)] = &[
    ("L", 160),
    // ① 第一步判 S、被 `cargo test` 变红证伪后改判 L 的 13 处
    //   （gate-preflight 12 + test_prop_name_quality 1）；
    // ② AC-003「泄漏 oracle」经 BA rev.3 裁定**全留**的 6 处（同在 gate-preflight）。
    ("L-改判", 19),
    ("R-代码", 13),
    ("R-注释", 7),
    // `O-待裁` 已随 rev.3 清零：15 处泄漏 oracle 全部裁定 ——
    // A/B/C 共 9 处两侧同批替换为假名（不再有真名可登记），
    // D 的 6 处全留并入 `L-改判`。保留这条 0 下界，
    // 让「偷偷把泄漏 oracle 塞回来却登记成 O-待裁」变红。
    ("O-待裁", 0),
];

/// 每个文件各应登记多少处（含 `drop_rate.rs` 的 0 —— 它必须**显式**是 0）。
const FLOOR_FILE: &[(&str, usize)] = &[
    ("src/bin/ontok-gate-preflight.rs", 18),
    ("src/domain/concept.rs", 5),
    ("src/domain/html_view.rs", 2),
    ("src/domain/identifier_shape.rs", 10),
    ("src/domain/review.rs", 3),
    ("src/domain/view.rs", 5),
    ("src/extract/propose.rs", 130),
    ("tests/drop_rate.rs", 0),
    ("tests/test_fk_naming.rs", 15),
    ("tests/test_mcp_server.rs", 2),
    ("tests/test_overview.rs", 0),
    ("tests/test_prop_name_quality.rs", 3),
    ("tests/test_snapshot_write_paths.rs", 6),
];

// ── ① ─────────────────────────────────────────────────────────────

#[test]
fn test_req084_registry_totals_match_the_floor() {
    let rows = registry();
    assert_eq!(
        rows.len(),
        199,
        "登记表共 {} 行，应为 199（160 L + 19 L-改判 + 20 R + 0 O-待裁）。\
         删登记行来掩盖改动 ⇒ 这里红。",
        rows.len()
    );
    for (kind, want) in FLOOR_KIND {
        let got = rows.iter().filter(|r| r.kind == *kind).count();
        assert_eq!(got, *want, "`{kind}` 档登记数应为 {want}，实得 {got}");
    }
    for (file, want) in FLOOR_FILE {
        let got = rows.iter().filter(|r| r.file == *file).count();
        assert_eq!(got, *want, "`{file}` 应登记 {want} 处，实得 {got}");
    }
}

#[test]
fn test_req084_registry_rows_are_well_formed_and_unique() {
    let rows = registry();
    let known: Vec<&str> = sources().iter().map(|(f, _)| *f).collect();
    for r in &rows {
        assert!(
            known.contains(&r.file.as_str()),
            "登记指向未被本文件内联的文件 `{}` —— 源文件被移动了？",
            r.file
        );
        assert!(!r.crit.trim().is_empty(), "{}:{} 的「判据」是空的", r.file, r.line);
        assert!(!r.why.trim().is_empty(), "{}:{} 的「理由」是空的", r.file, r.line);
    }
    let mut keys: Vec<(&str, usize, usize, &str)> = rows
        .iter()
        .map(|r| (r.file.as_str(), r.line, r.col, r.name.as_str()))
        .collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    assert_eq!(before, keys.len(), "登记表里有重复行 —— 计数会虚高");
}

// ── ② ─────────────────────────────────────────────────────────────

#[test]
fn test_req084_every_registered_name_is_still_literally_at_its_line() {
    let src = sources();
    for r in registry() {
        let (_, text) = src.iter().find(|(f, _)| *f == r.file).expect("已在上一条查过");
        let line = line_of(text, r.line);
        assert!(
            name_at(line, r.col, &r.name),
            "{}:{}:{} —— 登记说这里保留着表名 `{}`，但该位置上是：\n  {}\n\
             ⇒ 要么有人把它脱敏了（那请改登记并说明理由），要么这行被挪动过、登记过期了。",
            r.file,
            r.line,
            r.col,
            r.name,
            line.trim()
        );
    }
}

// ── ③ ─────────────────────────────────────────────────────────────

#[test]
fn test_req084_no_unregistered_real_name_remains_in_the_registered_files() {
    let rows = registry();
    let names = distinct_names(&rows);

    for (file, text) in sources() {
        let actual: usize = names.iter().map(|n| occurrences(text, n)).sum();
        let registered = rows.iter().filter(|r| r.file == file).count();
        assert_eq!(
            actual, registered,
            "`{file}` 里实际出现 {actual} 处已登记表名，登记表只记了 {registered} 处。\
             多出来的就是**没登记就留在仓库里的客户表名** —— 补登记，或把它脱敏掉。"
        );
    }
}

#[test]
fn test_req084_drop_rate_red_line_file_has_no_real_table_name() {
    // `drop_rate.rs` 从头到尾 0 处客户表名（它是按 schema 文本统计丢弃率的）。
    // 「显式登记为 0」是这条断言存在的意义：有人往红线文件里塞进一个真表名时，
    // 下面这行会红。
    let rows = registry();
    for n in distinct_names(&rows) {
        assert_eq!(
            occurrences(T_DROP_RATE, n),
            0,
            "红线文件 tests/drop_rate.rs 里出现了表名 `{n}` —— 登记说它一个都不该有"
        );
    }
    assert_eq!(
        rows.iter().filter(|r| r.file == "tests/drop_rate.rs").count(),
        0,
        "登记表里不该有 drop_rate.rs 的行"
    );
}

#[test]
fn test_req084_red_line_files_are_registered_not_shadowed() {
    let rows = registry();
    // 红线两文件（view.rs / test_fk_naming.rs）必须**逐条**在登记里，
    // 否则「红线一字未动」就只是一句没人核的话。
    for f in ["src/domain/view.rs", "tests/test_fk_naming.rs"] {
        let got = rows.iter().filter(|r| r.file == f).count();
        assert!(got > 0, "红线文件 `{f}` 在登记表里一处都没有 —— 保留档没被登记");
    }
    // 且它们的每一行都必须是 R 档（红线不因「看起来像载荷」而降级）
    for f in ["src/domain/view.rs", "tests/test_fk_naming.rs"] {
        for r in rows.iter().filter(|r| r.file == f) {
            assert!(
                r.kind.starts_with("R-"),
                "红线文件 `{f}`:{} 被登记成 `{}` —— 红线只能是 R 档",
                r.line,
                r.kind
            );
        }
    }
}
