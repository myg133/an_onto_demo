//! ⭐ 守卫：`include_str!("../../X")` 引用的仓库根文件**必须**在 Dockerfile 里 COPY。
//!
//! # 为什么需要这条
//!
//! 真实故障（REQ-157）：新增 `src/domain/relation_verbs.rs` 里
//! `include_str!("../../relation-verbs.json")`，本地 `cargo build` **绿**，
//! 但 `podman build` 在 `RUN cargo build` 阶段红：
//! `couldn't read src/domain/../../relation-verbs.json`。
//!
//! 原因：镜像构建上下文只 `COPY src ./src`，仓库根的那个文件没进去。
//! **本地测试永远抓不到这一类**，只有真打镜像会炸 —— 所以用静态对拍把它变成单测。

use std::collections::BTreeSet;
use std::path::Path;

mod common;

/// 仓库根。
///
/// ⚠️ **不许自己写 `env!("CARGO_MANIFEST_DIR")`** —— REQ-047 的 G-1 守卫
/// （`tests/test_anchor_guard.rs`）会在词法层扫 `tests/**/*.rs`，
/// 全仓只允许 `tests/common/source_root.rs` 里那一处（带 `ANCHOR-ALLOW` 标记）。
/// 第一版我就在这里自己写了一份，结果全量多红 1 条（`no_bare_env_manifest_dir_outside_anchor_file`）。
fn crate_root() -> std::path::PathBuf {
    common::source_root::crate_root()
}

/// `src/**/*.rs` 里所有 `include_str!("../../<name>")`（仓库根文件）的名字。
fn root_includes() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![crate_root().join("src")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                for line in text.lines() {
                    let Some(pos) = line.find("include_str!(\"../../") else { continue };
                    let rest = &line[pos + "include_str!(\"../../".len()..];
                    let Some(end) = rest.find('"') else { continue };
                    out.insert(rest[..end].to_string());
                }
            }
        }
    }
    out
}

#[test]
fn every_root_include_str_is_copied_into_the_image_build_context() {
    let includes = root_includes();
    assert!(
        !includes.is_empty(),
        "没扫到任何 `include_str!(\"../../…\")` —— 扫描逻辑坏了，这条守卫会变成空转"
    );

    let dockerfile = Path::new(&crate_root()).join("Dockerfile");
    let text = std::fs::read_to_string(&dockerfile)
        .unwrap_or_else(|e| panic!("读不到 Dockerfile：{e}"));

    let mut missing = Vec::new();
    for name in &includes {
        // 认 `COPY <name>` 或 `COPY <name> <dest>`，行首可有空白
        let copied = text.lines().any(|l| {
            let l = l.trim_start();
            let Some(rest) = l.strip_prefix("COPY ") else { return false };
            rest.split_whitespace().next() == Some(name.as_str())
        });
        if !copied {
            missing.push(name.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "❌ 这些仓库根文件被 `include_str!(\"../../…\")` 引用，但 Dockerfile **没有 COPY**：{missing:?}\n\
         ⇒ 本地 `cargo build` 会绿，`podman build` 会在 `RUN cargo build` 阶段红：\n\
         `couldn't read src/…/../../<文件>: No such file or directory`。\n\
         修法：在 Dockerfile 的 `COPY src ./src` 附近补一行 `COPY <文件> ./`。\n\
         （真实事故：REQ-157 的 relation-verbs.json —— 就是这个坑。）"
    );
}

#[test]
fn the_scan_would_have_caught_the_real_incident() {
    // 反空转：relation-verbs.json 与 query-policy.example.json 都必须在扫描结果里
    let includes = root_includes();
    for expected in ["relation-verbs.json", "query-policy.example.json"] {
        assert!(
            includes.contains(expected),
            "扫描结果里没有 {expected}：{includes:?} —— 扫描逻辑漏了它，守卫就是空的"
        );
    }
}
