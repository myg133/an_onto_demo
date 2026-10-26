//! REQ-107 · AC-009：红线零触碰（TC-027）。
//!
//! # 本文件证明**什么**、**不**证明什么
//!
//! 证明的：工作区与 `develop` 的差异里**没有**那四个红线文件、
//! `mcp::tools::guard` 的实现没有被改、`property_filter.rs` 的 G1–G4 三段逐字节未变、
//! 没有新增 `#[ignore]`、没有删除测试文件。
//!
//! ⛔ **不证明**「历史上没改过又改回来」—— 那只能看 PR 的逐 commit diff（人工项）。
//!
//! # 为什么可以调 `git`
//!
//! 这是**开发机判据**（不是运行时分判据）：本仓所有开发机都有 git，
//! 而「红线零触碰」这件事的唯一事实源就是 git。⛔ 不用 `git show HEAD:…`
//! 取「入库版」时也不要读工作区（纪律 K）—— 这里两条都用：`develop` 取基线、
//! 工作区取现状。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

mod common;

use std::process::Command;

/// 在 crate 根上跑一条 git 命令，返回 stdout（失败即 panic，附 stderr）。
fn git(args: &[&str]) -> String {
    let root = common::crate_root();
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("跑不起 git（本判据依赖 git）：{e}"));
    assert!(
        out.status.success(),
        "git {:?} 失败：\n{}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// 取 `text` 里从 `from` 到 `to`（不含）之间的片段；找不到锚点即 panic。
fn section(text: &str, from: &str, to: &str) -> String {
    let a = text
        .find(from)
        .unwrap_or_else(|| panic!("找不到区段起点 {from:?} —— 文档结构变了，判据要跟着改"));
    let b = text[a..]
        .find(to)
        .map(|n| a + n)
        .unwrap_or_else(|| panic!("找不到区段终点 {to:?}"));
    text[a..b].to_string()
}

#[test]
fn test_req107_red_lines_and_guard_are_untouched() {
    // ① 红线四文件：与 `develop` 的差异必须为空 —— **REQ-156 的例外是点名的、且只允许追加**。
    //
    //   REQ-156 授权：`src/domain/view.rs` 允许**追加** `semantic_cn` 的中文映射分支
    //   （以及配套的 `semantic_label` 助手与单测）。⛔ 不许改/删既有行。
    //   ⚠️ 这条 numstat 只证明「没删」；「追加的内容没顺手改语义」由
    //   `tests/test_req156_semantic_cn_invariants.rs` 的四条不变式钉住 ——
    //   白名单 + 专属守卫，两道一起才成立（不能只把文件名加进白名单）。
    let numstat = git(&[
        "diff",
        "--numstat",
        "develop",
        "--",
        "tests/drop_rate.rs",
        "tests/test_fk_naming.rs",
        "src/domain/review_view.rs",
    ]);
    assert!(
        numstat.trim().is_empty(),
        "⛔ 红线文件被改动了（AC-009：禁改）：\n{numstat}"
    );

    // ⭐ REQ-156 · 唯一获准的第四个红线文件：只准**追加**，不准删改。
    let view_numstat = git(&["diff", "--numstat", "develop", "--", "src/domain/view.rs"]);
    if !view_numstat.trim().is_empty() {
        let fields: Vec<&str> = view_numstat.split_whitespace().collect();
        assert_eq!(
            fields.len(),
            3,
            "src/domain/view.rs 的 numstat 形状不对：{view_numstat:?}"
        );
        assert_eq!(fields[2], "src/domain/view.rs", "numstat 指向了别的文件：{view_numstat:?}");
        assert_eq!(
            fields[1], "0",
            "⛔ REQ-156 只授权**追加** src/domain/view.rs（删除行数必须为 0）：\n{view_numstat}"
        );
        let added: u32 = fields[0]
            .parse()
            .unwrap_or_else(|_| panic!("读不出新增行数：{view_numstat:?}"));
        assert!(
            added > 0,
            "view.rs 有 diff 却没有新增行 —— numstat 读错了：{view_numstat:?}"
        );
    }

    // ② 守门器实现不许动：`src/mcp/tools.rs` 的新增/删除行里不得出现实现关键串。
    //    ⚠️ 允许 `guard` 被**调用**（本需求改的是调用点），所以只禁「实现那几行」。
    let diff = git(&["diff", "develop", "--", "src/mcp/tools.rs"]);
    let mut offenders = Vec::new();
    for line in diff.lines() {
        if !(line.starts_with('+') || line.starts_with('-')) {
            continue;
        }
        if line.starts_with("+++") || line.starts_with("---") {
            continue;
        }
        for bad in [
            "fn guard(",
            "已拒绝外送",
            "scan_modeling_terms(&text)",
            "这一问不给数据：内容里出现了",
        ] {
            if line.contains(bad) {
                offenders.push(format!("  {}（含 {bad:?}）", line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "⛔ `mcp::tools::guard` 的实现被改动了（AC-009）：\n{}",
        offenders.join("\n")
    );

    // ③ G1–G4 三段的措辞**逐字节**未变（AC §七 A4 允许在 ①–③ **之后**追加一段，
    //    所以这里比的是 ①→②、②→③、③→「以上三条均未封堵」三段）。
    let base = git(&["show", "develop:src/domain/property_filter.rs"]);
    let cur = common::read_source("src/domain/property_filter.rs");
    for (from, to) in [
        ("//! ## ①", "//! ## ②"),
        ("//! ## ②", "//! ## ③"),
        ("//! ## ③", "//! ⛔ 以上三条"),
    ] {
        assert_eq!(
            section(&cur, from, to),
            section(&base, from, to),
            "`src/domain/property_filter.rs` 的 {from:?} 区段被改动了 —— \
             G1–G4 的措辞一个字都不许动（AC-009 / A4）"
        );
    }
    // 反空转：三段都必须真的取到内容（否则上面是「空 == 空」）。
    let s = section(&cur, "//! ## ①", "//! ## ②");
    assert!(s.len() > 200, "区段① 只有 {} 字节 —— 锚点大概取错了", s.len());

    // ④ 不许新增 `#[ignore]`。
    //
    // ⚠️ 只认**真正的属性**：新增行去掉行首 `+` 与空白后必须以 `#[ignore]` 开头。
    //    否则本文件/别的测试里「讨论这件事」的那几行（注释、字符串、`.contains("…")`）
    //    会把自己判红 —— 判据恒红 = 恒假。
    let attr = concat!("#", "[ignore]");
    let tdiff = git(&["diff", "develop", "--", "tests"]);
    let ignores: Vec<&str> = tdiff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .filter(|l| {
            l.trim_start_matches('+').trim_start().starts_with(attr)
        })
        .collect();
    assert!(
        ignores.is_empty(),
        "⛔ 新增了 `#[ignore]`（AC-009：不许 ignore）：{ignores:?}"
    );
    // 反空转：这条判据得认得出**真的**属性行（否则它永远绿）。
    let probe = format!("+    {attr}");
    assert!(
        probe.trim_start_matches('+').trim_start().starts_with(attr),
        "判据认不出真正的 `#[ignore]` 属性行"
    );
    let comment_probe = "+/// 讨论一下 #[ignore] 这件事";
    assert!(
        !comment_probe.trim_start_matches('+').trim_start().starts_with(attr),
        "注释里提到 `#[ignore]` 被误判成属性"
    );

    // ⑤ 不许删除测试文件。
    //
    // ⚠️ **REQ-155 的例外是点名的**（需求 §三①「测试」一行要求删掉门户的两份测试文件：
    // `tests/test_req126_portal.rs` / `tests/test_req126_live.rs` —— 它们整份都在测
    // REQ-126 的客户门户，门户已删除，留着必然编译不过）。
    // 除这两个名字之外，**任何**测试文件的删除仍然红 —— 判据只是窄化，不是放宽。
    const REQ155_AUTHORIZED_DELETIONS: [&str; 2] = [
        "tests/test_req126_portal.rs",
        "tests/test_req126_live.rs",
    ];
    let deleted = git(&[
        "diff",
        "develop",
        "--diff-filter=D",
        "--name-only",
        "--",
        "tests",
    ]);
    let unexpected: Vec<&str> = deleted
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !REQ155_AUTHORIZED_DELETIONS.contains(l))
        .collect();
    assert!(
        unexpected.is_empty(),
        "⛔ 删除了测试文件（AC-009：不许删测试；REQ-155 只授权删门户的两份）：\n{}",
        unexpected.join("\n")
    );
}

/// 判据自身的自检：`section` 与 `git` 都不能是空转。
#[test]
fn the_red_line_scanners_are_not_vacuous() {
    let text = "AAA\n//! ## ①\n中间\n//! ## ②\nBBB\n";
    assert_eq!(section(text, "//! ## ①", "//! ## ②"), "//! ## ①\n中间\n");
    // 锚点不存在时必须 panic，而不是返回空串（返回空串会让「空 == 空」恒绿）。
    let r = std::panic::catch_unwind(|| section(text, "//! ## ④", "//! ## ⑤"));
    assert!(r.is_err(), "`section` 在锚点缺失时返回了值 —— 判据会恒绿");

    // `git` 必须真的能读到 develop 分支上的内容。
    let v = git(&["show", "develop:Cargo.toml"]);
    assert!(v.contains("name = \"ontok\""), "读不到 develop 上的 Cargo.toml：{v}");
}
