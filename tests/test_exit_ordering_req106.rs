//! REQ-106 · 出口**时序**与**口径**的判据。
//!
//! # 这个文件守的是哪两件事
//!
//! REQ-106 修的是两个出口缺陷，**都不是**「值不对」，而是**顺序**与**口径**：
//!
//! | 洞 | 位置 | 坏在哪 |
//! | --- | --- | --- |
//! | H1 / AC-001 | `src/cli.rs` 的 `ontology show` JSON 出口 | **先写出、后判定** ⇒ 命中时完整数据已落 stdout，`exit 2` 只是事后信号 |
//! | H2 / AC-003 | `src/server/page.rs` 的首页 | 项目名**不过闸**且**不转义** ⇒ 与 MCP 侧口径不一 + 可注入 |
//!
//! **行为**判据（首页返回什么、页面被不被改坏）在
//! `src/server/http.rs` 的 `#[cfg(test)]` 里 —— 那里能拿到 `AppState`
//! 并用 `Router::oneshot` 打真实请求。
//!
//! ## 判据跑在**剥掉注释之后**的代码上
//!
//! 与 `test_read_only_guard.rs` / `test_graph_server.rs` 同一条纪律：
//! REQ-002 复审栽过的形态正是「守卫被删掉、字符串留在注释里」，裸 grep 挡不住它。
//! `the_stripper_is_not_vacuous` 是这条判据自身的自检。
//!
//! ## ⚠️ 这些是**源码扫描型守卫** —— 取变异证据的方式与别处不同
//!
//! 判据用 `read_source` 在**运行时读工作区文件**，源码**没有**被编译进测试二进制。
//! 所以给它们取变异证据，要取**被扫文件的工作区 sha256**
//! （`sha256sum src/cli.rs` / `src/server/page.rs`），不是二进制 sha256。
//!
//! ## ⚠️ 已知限制 · **未封堵** —— 本文件的守卫是**拼写级**，不是语义级
//!
//! **这是纪律 H 的正面登记，不是免责声明。** 纪律 H 说的是：
//! 「两份等价实现比出来永远相等 ⇒ 永远绿」。本文件认的正是**那几处拼写**：
//! `emit_domain_line(&json)`、`scan_modeling_terms(&json)`、
//! `if !leaked.is_empty()`、`mcp::tools::guard`、`var who = { project: %%PROJECT%%`。
//!
//! 逐条说清 AC-001 / AC-002 / AC-003 各自**抓得住**与**抓不住**什么：
//!
//! | AC | 守卫 | 抓得住 | 抓不住 |
//! | --- | --- | --- | --- |
//! | AC-001 | `the_json_exit_judges_before_it_writes`、`the_json_exit_writes_outside_the_refusal_branch` | **改回旧写法**（先写后判、把写出挪进拒发分支） | 换用另一个等价的写 stdout 的函数/闭包，只改那几处拼写就绕过 |
//! | AC-002 | `the_leak_listing_never_goes_to_stdout` | **改回旧写法**（命中清单回到 `println!`） | 把清单拼进别的 `println!` 变体、或换个等价的 stdout 写法 |
//! | AC-003 | `the_homepage_gate_calls_the_mcp_guard_itself`、`the_homepage_does_not_carry_its_own_criterion`、`the_project_placeholder_occupies_a_whole_js_literal` | **改回旧写法**（不过闸 / 自抄一份判据 / 占位符退回裸引号） | 判据等价重写成自造 wrapper 而保留那几个拼写 —— 此时本文件**仍会全绿**，而两份判据可能已经漂了 |
//!
//! ⛔ **因此本文件不主张、也不构成 AC-001 / AC-002 / AC-003 的完整覆盖。**
//! 能证明的只有一句话：「这几处**时序**与**口径**没有被悄悄改回旧写法」。
//! 不能证明「不存在另一种绕过写法」。加固另立需求。
//!
//! ⚠️ 顺带说明：这**不影响**变异证据的有效性 —— 纪律 G 要证明的是
//! 「守卫在它认的那类改动上真的会红」，而三条变异正是那类改动（M1/M2/M3 各自转红）。

mod common;

use common::read_source;

/// 去掉 Rust 注释（块注释支持嵌套，与 `test_read_only_guard.rs` 同一份算法）。
fn strip_rust_comments(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(b.len());
    let mut block = 0usize;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if block > 0 {
            if c == '/' && next == Some('*') {
                block += 1;
                i += 2;
            } else if c == '*' && next == Some('/') {
                block -= 1;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            block = 1;
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 被扫文件的**测试模块之前**那部分（判据不许被测试代码里的字符串喂饱）。
fn production_code(rel: &str) -> String {
    let code = strip_rust_comments(&read_source(rel));
    code.split("#[cfg(test)]").next().unwrap_or("").to_string()
}

/// 判据自身的自检（纪律 E：扫出「零」先怀疑扫描器）。
#[test]
fn the_stripper_is_not_vacuous() {
    let out = strip_rust_comments("// scan_modeling_terms(x)\n/* emit_domain_line */\nlet y = 1;\n");
    assert!(!out.contains("scan_modeling_terms"), "行注释没被剥掉");
    assert!(!out.contains("emit_domain_line"), "块注释没被剥掉");
    assert!(out.contains("let y = 1;"), "剥注释把真代码也剥了");
}

// ══════════════════════════════════════════════════════════════
// AC-001 · JSON 出口必须**先判后写**
//
// ⚠️ REQ-107 迁移说明（本文件是本轮**第二个**被改写的既有守卫，第一个是
// `tests/test_property_filter.rs` 的 `all_three_outlets_call_the_one_shared_function`）：
//
// 收口前这两条判据钉的是 `src/cli.rs` 里的
// `let leaked = scan_modeling_terms(&json);` 这一行 —— 而 REQ-107 的 AC-001
// 要求出口文件里**没有任何判据直调**（判据收进 `src/domain/pipeline.rs`）。
// ⇒ 锚点**随代码迁移**到管线，断言的内容（判定在写出之前、产物不在拒发分支内）
// 一个字没放宽。旧锚点在收口后**不存在**，继续钉它会得到「找不到 anchor」式的假红。
// ══════════════════════════════════════════════════════════════

/// 管线模块的**生产代码**（剥注释、去测试模块）。
fn pipeline_code() -> String {
    production_code("src/domain/pipeline.rs")
}

/// 取 `render` 里某一个 arm 的片段（从 arm 头到下一个 `Product::` arm 头）。
fn arm_slice<'a>(src: &'a str, arm: &str) -> &'a str {
    let from = src
        .find(arm)
        .unwrap_or_else(|| panic!("管线里找不到 arm `{arm}` —— 收口后的结构变了，判据必须跟着改"))
        + arm.len();
    let rest = &src[from..];
    match rest.find("Product::") {
        Some(n) => &rest[..n],
        None => rest,
    }
}

/// ⛔ 客户类出口的三步顺序必须是 **判定 → 不通过则拒发 → 通过才产出可写出的产物**。
///
/// 收口前这里是「`let leaked = scan_modeling_terms(&json);` 之后才是
/// `emit_domain_line(&json)`」；收口后判据与写出都在管线里，判据变成
/// 「闸调用必须在产物构造之前」—— 「先判后写」这句话的落点没变。
#[test]
fn the_json_exit_judges_before_it_writes() {
    let src = pipeline_code();
    let arm = arm_slice(&src, "Product::CustomerJson(view) => {");

    let gate = arm
        .find("gate_text(spec, &text)?")
        .unwrap_or_else(|| panic!("JSON arm 里找不到闸调用 `gate_text(spec, &text)?` —— 判定不见了"));
    let build = arm
        .find("Ok(OutletRender::build(")
        .unwrap_or_else(|| panic!("JSON arm 里找不到产物构造 —— 「写出」那一步不见了"));

    assert!(
        gate < build,
        "AC-001 被破坏：产物在判定之前就构造出来了（构造位置 {build} < 判定位置 {gate}）\
         —— 命中建模术语时出口已经拿到一份可写出的字节"
    );
}

/// AC-001 的**另一半**：产物构造必须发生在拒发分支**之外**。
///
/// ⛔ 只靠「谁在前」不够：有人可以把产物构造挪进 `if !hits.is_empty() { … }`
/// **块内部**，字面量上看着仍然是「判定在前」。
/// 判据用**缩进**钉住块归属 —— 产物构造那一行不得比闸那一行更深。
#[test]
fn the_json_exit_writes_outside_the_refusal_branch() {
    let src = pipeline_code();
    for arm in [
        "Product::CustomerJson(view) => {",
        "Product::CustomerHtml { view, sampling } => {",
    ] {
        let body = arm_slice(&src, arm);
        let gate_at = body
            .find("gate_text(spec, &")
            .unwrap_or_else(|| panic!("{arm} 里找不到闸调用"));
        let build_at = body
            .find("Ok(OutletRender::build(")
            .unwrap_or_else(|| panic!("{arm} 里找不到产物构造"));

        let gate_indent = line_indent(body, gate_at);
        let build_indent = line_indent(body, build_at);
        assert!(
            build_indent <= gate_indent,
            "{arm}：产物构造那一行缩进 {build_indent} 比闸那一行 {gate_indent} 更深 —— \
             它被挪进了拒发分支的块内部（那样这一条臂永远不会产出字节）"
        );
    }
}

/// 某一处偏移量**所在行**的行首缩进宽度（按字节，中文注释不影响：已被剥掉）。
fn line_indent(body: &str, off: usize) -> usize {
    let start = body[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let seg = &body[start..off];
    seg.len() - seg.trim_start().len()
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 诊断一律走 stderr
// ══════════════════════════════════════════════════════════════

/// ⛔ 命中清单**不许**进 stdout。
///
/// 清单里就是建模黑话本身（`外键` / `基数` / `主键` / `证据`）；
/// 写进 stdout 等于把它顺着 `> out.json` 一起发出去 ——
/// 闸门挡住了正文，却从诊断里漏了后门。
#[test]
fn the_leak_listing_never_goes_to_stdout() {
    let body = production_code("src/cli.rs");
    assert!(
        !body.contains(r#"println!("\n⚠ 客户视图泄露了建模术语"#),
        "命中清单回到 stdout 了 —— 守门词会顺着重定向一起发出去"
    );
    assert!(
        body.contains(r#"eprintln!("⚠ 客户视图泄露了建模术语"#),
        "命中清单应当走 stderr"
    );
}

// ══════════════════════════════════════════════════════════════
// AC-003 · 首页与 MCP 侧**同一道**判据（纪律 H）
// ══════════════════════════════════════════════════════════════

/// ⛔ 首页的项目名闸门必须是 `mcp::tools::guard` **本身**，不是抄一份。
///
/// 抄一份就会漂：MCP 那条路放过的东西，这条路可能挡住（或反过来）。
/// 这正是本 REQ 修的那个不一致的形状。
#[test]
fn the_homepage_gate_calls_the_mcp_guard_itself() {
    let body = production_code("src/server/page.rs");
    assert!(
        body.contains("mcp::tools::guard"),
        "首页项目名必须过 `mcp::tools::guard` 本身 —— 那是全仓唯一一份判据"
    );
}

/// ⛔ `page.rs` 不得**自己**再抄一份判据。
///
/// 命中判据就是 `scan_modeling_terms`（`mcp::tools::guard` 内部用的那个）。
/// 首页若直接调它，就等于多了一条判据 —— 与 MCP 那条**可能**不一致，
/// 而「可能」正是纪律 H 要消灭的东西。
#[test]
fn the_homepage_does_not_carry_its_own_criterion() {
    let body = production_code("src/server/page.rs");
    assert!(
        !body.contains("scan_modeling_terms"),
        "page.rs 自己调了 `scan_modeling_terms` —— 那是第二份判据，\
         必须一律走 `mcp::tools::guard`"
    );
}

/// AC-003 的**另一半**：项目名必须**安全转义**进 JS 字面量。
///
/// ⛔ 判据认的是「占位符两侧没有裸引号」这件事：
/// REQ-106 之前模板是 `project: "%%PROJECT%%"` —— 引号在**模板**里，
/// 于是替换进去的只能是「裸内容」，转义无处安放：项目名里的 `"`
/// 必然与模板那对引号配对错位，把 `var who = {…}` 的结构改坏。
/// 现在占位符占的是**整个字面量**，转义才有地方落。
#[test]
fn the_project_placeholder_occupies_a_whole_js_literal() {
    let body = production_code("src/server/page.rs");
    assert!(
        body.contains(r#"var who = { project: %%PROJECT%%, version: %%VERSION%% };"#),
        "模板里的 `%%PROJECT%%` 必须占**整个** JS 字面量（两侧不该再有裸引号）"
    );
    assert!(
        !body.contains(r#"{ project: "%%PROJECT%%""#),
        "模板里还留着裸引号 —— 项目名里的 `\"` 会与它配对错位、改坏脚本结构"
    );
}
