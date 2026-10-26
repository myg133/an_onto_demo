//! REQ-152 · **两个 bin 的结构守卫**（服务面 / 构建面）。
//!
//! 这里的每一条都对应 AC-006 的一个判据，但判据是**结构式**的、能脱离编译产物跑：
//!
//! | 本文件的测试 | 对应的 AC-006 判据 |
//! | --- | --- |
//! | [`cargo_declares_exactly_two_bins`] | `target/release` 里只有 `ontok` / `ontok-builder`；四个老 bin 不再产出 |
//! | [`service_entry_does_not_reference_the_builder_cli`] | `strings target/release/ontok \| grep -c X-TC-Action` ⇒ 0（可达图里没有执行器） |
//! | [`builder_entry_merges_the_four_tools_as_subcommands`] | `ontok-builder introspect/propose/translate-names/gate-preflight` 存在 |
//! | [`dockerfile_copies_only_ontok`] | `podman run --rm <img> ls /usr/local/bin` ⇒ 只有 `ontok` |
//!
//! ⚠️ **这不是替换**实机判据：`strings` / `podman` 那两条仍要真跑。本文件守的是
//! 「改代码时不会悄悄把执行器接回服务面」—— 它比 `strings` 早红，而且不依赖构建产物。

use std::path::{Path, PathBuf};

// ⚠️ 读源码必须走 `common::source_root`（**运行时**定位 crate 根）。
// `env!("CARGO_MANIFEST_DIR")` 是**裸编译期锚点**，`tests/test_anchor_guard.rs`
// 会当场判红（只允许出现在 `tests/common/source_root.rs` 且带 `// ANCHOR-ALLOW`）。
mod common;

fn root() -> PathBuf {
    common::crate_root()
}

fn read(rel: &str) -> String {
    let p = root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}: {e}", p.display()))
}

/// 只保留**代码行**（去掉 `//` 开头的注释行、块注释与 `/* */`）。
///
/// 为什么要剥：服务面的模块头里**写着**「不许引用 `crate::cli`」这句话本身，
/// 直接扫全文会把说明当成违规（假红）。剥注释只解决这一格；
/// 字符串里的同名词仍会被算命中 —— 那是故意的（宁可多报）。
fn code_lines(text: &str) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in text.lines() {
        let t = line.trim_start();
        if in_block {
            if let Some(at) = t.find("*/") {
                in_block = false;
                out.push_str(&t[at + 2..]);
                out.push('\n');
            }
            continue;
        }
        if t.starts_with("/*") {
            if !t.contains("*/") {
                in_block = true;
            }
            continue;
        }
        if t.starts_with("//") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn cargo_declares_exactly_two_bins() {
    let toml = read("Cargo.toml");
    let bins: Vec<&str> = toml
        .lines()
        .filter(|l| l.trim_start().starts_with("[[bin]]"))
        .collect();
    assert_eq!(
        bins.len(),
        2,
        "R6：`Cargo.toml` 的 `[[bin]]` 必须恰好两个（服务面 / 构建面）；实际 {} 个\n{toml}",
        bins.len()
    );
    assert!(
        toml.contains("name = \"ontok\""),
        "缺服务面 bin：`name = \"ontok\"`"
    );
    assert!(
        toml.contains("name = \"ontok-builder\""),
        "缺构建面 bin：`name = \"ontok-builder\"`"
    );
    // ⛔ 老命令名不保留别名（用户拍板）：`[[bin]]` 里也不许再出现它们。
    for old in [
        "ontok-introspect",
        "ontok-propose",
        "ontok-translate-names",
        "ontok-gate-preflight",
    ] {
        assert!(
            !toml.contains(&format!("name = \"{old}\"")),
            "老的 bin 名还被声明成 `[[bin]]`：{old}（R5：不保留别名）"
        );
    }
    // `src/bin/` 下还留着四个工具的源文件 ⇒ 必须关掉自动发现，否则它们又会各自成 bin。
    assert!(
        toml.contains("autobins = false"),
        "`src/bin/` 里仍有可能被自动发现的文件 ⇒ 必须写 `autobins = false`，\
         否则 `cargo build --bins` 会重新产出 ontok-introspect / ontok-propose / …"
    );
}

/// 服务面的三个源文件（入口 + 服务 CLI + 共用的 EPIPE hook）**不许**引用构建面或执行器。
///
/// 这条守的是 R1「服务面零签名代码」的**结构面**：链接器按可达符号收对象，
/// 只要这里干净，`access::net` / `access::tc3` / `ureq` 就不会进 `ontok`。
const SERVICE_FILES: &[&str] = &["src/main.rs", "src/service.rs", "src/pipe_guard.rs"];

/// 服务面不许出现的引用（前两条是构建面入口，后四条是执行器）。
const FORBIDDEN_IN_SERVICE: &[&str] = &[
    "ontok::cli",
    "crate::cli",
    "access::net",
    "access::tc3",
    "access::fetch",
    "ureq",
];

#[test]
fn service_entry_does_not_reference_the_builder_cli() {
    for rel in SERVICE_FILES {
        assert!(root().join(rel).exists(), "服务面文件不见了：{rel}");
        let code = code_lines(&read(rel));
        for bad in FORBIDDEN_IN_SERVICE {
            assert!(
                !code.contains(bad),
                "{rel} 的代码里出现了 `{bad}` —— 服务面不许引用构建面 / 执行器。\n\
                 后果：`strings target/release/ontok | grep -c 'X-TC-Action'` 会从 0 变成非 0\
                 （AC-006），产品二进制里又带上签名代码。"
            );
        }
    }
    // 反向自检：`src/cli.rs` **必须**真的还引用执行器（否则上面那圈是因为「名字打错了」而恒绿）。
    let builder = code_lines(&read("src/cli.rs"));
    assert!(
        builder.contains("access::net"),
        "前提不成立：`src/cli.rs` 里已经找不到 `access::net` ⇒ 上面那条「服务面不引用」\
         可能是拿一个不存在的名字在比（恒真）"
    );
}

#[test]
fn builder_entry_merges_the_four_tools_as_subcommands() {
    let src = read("src/bin/ontok-builder.rs");
    for (file, module) in [
        ("ontok-introspect.rs", "introspect"),
        ("ontok-propose.rs", "propose"),
        ("ontok-translate-names.rs", "translate_names"),
        ("ontok-gate-preflight.rs", "gate_preflight"),
    ] {
        assert!(
            src.contains(&format!("#[path = \"{file}\"]")),
            "`ontok-builder` 没有把 {file} 挂成模块（`#[path = \"{file}\"]`）"
        );
        assert!(
            src.contains(&format!("mod {module};")),
            "`ontok-builder` 没有声明 `mod {module};`"
        );
        assert!(
            root().join("src/bin").join(file).exists(),
            "四个工具的源文件必须留在原路径（`include_str!` 与「保留名」判据按精确路径钉着）：{file}"
        );
    }
    // 子命令名必须是任务书里的四个（kebab-case 由 clap 从变体名生成）。
    for variant in ["Introspect", "Propose", "TranslateNames", "GatePreflight"] {
        assert!(
            src.contains(variant),
            "`ontok-builder` 缺子命令变体 `{variant}`（对应 {}）",
            match variant {
                "Introspect" => "introspect",
                "Propose" => "propose",
                "TranslateNames" => "translate-names",
                _ => "gate-preflight",
            }
        );
    }
}

#[test]
fn dockerfile_copies_only_ontok() {
    let df = read("Dockerfile");
    let copies: Vec<&str> = df
        .lines()
        .filter(|l| l.trim_start().starts_with("COPY --from=build"))
        .filter(|l| l.contains("/usr/local/bin/"))
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "镜像里只该 COPY 一个二进制（服务面 `ontok`）；实际 {} 个：{copies:?}",
        copies.len()
    );
    assert!(
        copies[0].contains("/target/release/ontok "),
        "COPY 的必须是 `/src/target/release/ontok`：{}",
        copies[0]
    );
    // ⚠️ 只看 **COPY 行**：Dockerfile 的注释里**点着名**说「不再 COPY ontok-introspect …」，
    //    那是说明，不是行为（扫全文会把说明本身判成违规 —— 假红）。
    for line in df.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            continue;
        }
        for old in [
            "ontok-introspect",
            "ontok-propose",
            "ontok-translate-names",
        ] {
            assert!(
                !t.contains(old),
                "`Dockerfile` 的**有效指令**里仍提到 {old} —— 客户镜像不该带 FDE 本机工具：{line}"
            );
        }
    }
}

/// 四个工具的源文件**仍然存在**（它们只是不再是独立 bin）。
///
/// 这条看着像废话，其实是给 `autobins = false` 兜底：有人嫌「留着四个 .rs 太乱」
/// 把它们删了，`ontok-builder` 立刻编译失败 —— 但在那之前，这条会先红。
#[test]
fn the_four_tool_sources_are_still_modules_of_the_builder() {
    let bin_dir = root().join("src/bin");
    for f in [
        "ontok-introspect.rs",
        "ontok-propose.rs",
        "ontok-translate-names.rs",
        "ontok-gate-preflight.rs",
        "ontok-builder.rs",
    ] {
        let p: &Path = &bin_dir.join(f);
        assert!(p.exists(), "缺文件：{}", p.display());
    }
}
