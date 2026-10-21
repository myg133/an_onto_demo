//! REQ-093 · 只读图服务的**接线层**判据。
//!
//! ## 为什么这里只有「接线」判据，行为判据却在 `src/server/http.rs` 里
//!
//! 405 / 401 / 守门器那几条是**行为**，要拿到 `AppState`（私有字段）
//! 才能构造，所以它们住在 `src/server/http.rs` 的 `#[cfg(test)]` 里，
//! 用 `Router::oneshot` 在进程内打真实请求。
//!
//! 本文件守的是**另两条**行为判据够不着的东西：
//!
//! 1. **`server/` 里没有任何写本体的接线**（AC-003 的第二层）——
//!    405 只能证明「路由不接」，证明不了「代码没去调 apply」。
//! 2. **`domain/` 零 I/O 没被本需求破掉**（AC-005）——
//!    `graph.rs` 是本需求往 `domain/` 新增的文件，它一旦沾上存储层就破功。
//!
//! ## 判据跑在**剥掉注释之后**的代码上
//!
//! 与 `test_read_only_guard.rs` 同一条纪律：REQ-002 复审栽过的形态正是
//! 「守卫被删掉、字符串留在注释里」，裸 grep 挡不住它。
//! `the_stripper_is_not_vacuous` 是这条判据自身的自检。
//!
//! ## ⚠️ 本文件这些是**源码扫描型守卫** —— 取变异证据的方式与别处不同
//!
//! 这里的判据用 `read_source` 在**运行时读工作区文件**，源码**没有**被
//! 编译进测试二进制。于是：
//!
//! - **测试二进制的 sha256 不会因为改动被扫文件而变化。**
//!   QA 的变异 B1 / B2 实测两者二进制 sha256 **完全相同**（`b8884eb2…`），
//!   而变异确实生效了（判据放行了）。
//! - ⇒ 给这类守卫取变异证据，要取的是**被扫文件的工作区 sha256**
//!   （`sha256sum <被扫文件>`），不是二进制 sha256。
//!   否则「二进制没变」会被误读成「测试压根没跑过变异」。
//!
//! ## ⚠️ 而且它们是**拼写黑名单**，不是语义判据（未封堵的已知限制）
//!
//! QA 变异 B2：把一条真实的 `delete from snapshots` 改写成
//! `query_scalar` + 小写 ⇒ **全绿放行**，连既有的 `test_no_write_path`
//! 与 `test_read_only_guard` 也一并放行。
//! ⇒ 判据认的是**那几个词的拼写**，不是「这是一条写语句」。
//! 加固另立需求；详见 `src/server/mod.rs` 的「已知限制 · P2」。

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

/// 判据自身的自检（纪律 E：扫出「零」先怀疑扫描器）。
#[test]
fn the_stripper_is_not_vacuous() {
    let out = strip_rust_comments("// apply_proposal(x)\n/* freeze() */\nlet y = 1;\n");
    assert!(
        !out.contains("apply_proposal") && !out.contains("freeze"),
        "注释没被剥掉 —— 下面的接线判据会退化成存在性 grep：{out}"
    );
    assert!(out.contains("let y = 1;"), "真代码不该被误剥：{out}");
    let kept = strip_rust_comments("f(\"apply_proposal\")");
    assert!(kept.contains("apply_proposal"), "字符串字面量不该被剥：{kept}");
}

// ── AC-003 · 只读：无任何从 HTTP 写到本体的路径 ──────────

/// ⛔ `server/` 下不许出现任何**写本体的入口**。
///
/// 这条抓的是 405 抓不到的那一层：路由确实只接 GET，
/// 但代码若在别处调了 apply / freeze / 裁决，那仍是条写路径。
#[test]
fn server_has_no_write_path_to_the_ontology() {
    for name in ["mod.rs", "http.rs", "page.rs"] {
        let code = strip_rust_comments(&read_source(&format!("src/server/{name}")));
        for verb in [
            "apply_proposal",
            "save_snapshot",
            "set_decision",
            "freeze",
            "create_proposal",
            "set_extracted_by",
            "add_concept_group",
            "migrate",
            "UPDATE ",
            "INSERT ",
            "DELETE FROM",
            "sqlx::query(",
        ] {
            assert!(
                !code.contains(verb),
                "只读图服务不得有写本体的路径：src/server/{name} 里出现了 `{verb}`"
            );
        }
    }
}

/// `server/` 只允许读这三条 —— 多一条都该说清为什么。
#[test]
fn server_only_reads_three_snapshot_methods() {
    let code = strip_rust_comments(&read_source("src/server/http.rs"));
    for allowed in ["get_by_name", "version_by_seq", "get_snapshot"] {
        assert!(code.contains(allowed), "预期会用到只读方法 {allowed}");
    }
}

// ── AC-005 · domain/ 零 I/O ─────────────────────────────

/// 基线 `0455523` 的命中数。**9 处，全部在注释里。**
///
/// 这个数字是**钉死的常量**，不是现算的：原 AC 写的是「grep 必须无输出」，
/// 但那 9 处在**基线上就已经存在**（`apply.rs` 1 / `snapshot.rs` 2 /
/// `mod.rs` 1 / `review_view.rs` 5），判据从写下起就不成立。
/// 复核方式是对每个文件跑 `git show HEAD:<path> | grep …`（纪律 K：读 blob，
/// 不读工作区），基线与工作区**逐文件相等**。
const BASELINE_COMMENT_HITS: usize = 9;

/// 判据跑在**剥掉注释之后**的代码上（REQ-002 复审栽过的形态是
/// 「守卫被删掉、字符串留在注释里」，裸 grep 挡不住它）。
///
/// 本测试**同时**钉两个数，两个都要：
///
/// 1. **代码里命中 = 0** —— 这才是「零 I/O」真正要表达的事；
/// 2. **连注释一起命中 = 9（不增不减）** —— 不许新增。
///    只判第 1 条会漏掉「有人往注释里写了新的存储层依赖名」；
///    只判第 2 条则会被注释里的既有写法永久卡死。
///
/// ⚠️ 将来若有人**正当地**增删了某个 domain 文件里提到这三个词的注释，
/// 本测试会红 —— 那是**设计如此**：那一步必须有人明确决定，
/// 并把常量一并更新，而不是悄悄漂过去。
#[test]
fn domain_layer_stays_io_free() {
    let tokens = ["sqlx", "PgStore", "db::"];
    let dir = common::crate_root().join("src/domain");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("读不到 src/domain/：{e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "src/domain/ 一个 .rs 都没有 —— 判据本身可疑");

    let mut code_hits: Vec<String> = Vec::new();
    // ⚠️ 按**行**计，不按「行×词元」计 —— 必须与
    // `grep -rn 'sqlx\|PgStore\|db::' src/domain/` 的口径**一模一样**，
    // 否则这个常量和交付时贴的 grep 原始输出对不上，判据自己就成了两把尺。
    // （实测有 2 行同时含两个词元：按行算是 9，按行×词元算是 11。）
    let mut comment_hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for f in &files {
        let rel = f.strip_prefix(common::crate_root()).unwrap().to_string_lossy();
        let raw = read_source(&rel);
        for (i, line) in raw.lines().enumerate() {
            if tokens.iter().any(|t| line.contains(t)) {
                comment_hits.insert(format!("{}:{}", f.display(), i + 1));
            }
        }
        let code = strip_rust_comments(&raw);
        for t in tokens {
            if code.contains(t) {
                code_hits.push(format!("{}: {t}", f.display()));
            }
        }
    }

    assert!(
        code_hits.is_empty(),
        "domain 层必须零 I/O：剥掉注释后仍出现存储层引用 → {code_hits:?}"
    );
    assert_eq!(
        comment_hits.len(),
        BASELINE_COMMENT_HITS,
        "连注释在内的**行数**应与基线 0455523 一致（{} 行，不增不减）。\
         当前 {} 行。多/少 {}。命中明细：\n{}",
        BASELINE_COMMENT_HITS,
        comment_hits.len(),
        comment_hits.len().abs_diff(BASELINE_COMMENT_HITS),
        comment_hits.iter().cloned().collect::<Vec<_>>().join("\n")
    );
}

/// ⛔ `graph.rs` 必须真的**不筛东西**。
///
/// 它是客户视图的第二个消费者；一旦在这里加过滤，
/// 「图 = 客户视图」这个不变量就断了，而 AC-001 的比对会**跟着一起绿**
/// —— 两份被同一个过滤器筛过的数据永远相等（纪律 H）。
#[test]
fn graph_projection_does_not_filter() {
    let code = strip_rust_comments(&read_source("src/domain/graph.rs"));
    // 投影里只允许「建索引」用的过滤（`filter(|x| …)` 出现在测试里是允许的，
    // 所以判据跑在测试模块**之前**的代码上）。
    let body = code.split("#[cfg(test)]").next().unwrap_or("");
    for bad in ["usable_as_human_name", ".filter(", "scan_modeling_terms"] {
        assert!(
            !body.contains(bad),
            "graph.rs 不该过滤或自查 —— 那是 view.rs 与 HTTP 层的职责：出现了 `{bad}`"
        );
    }
}

// ── 前端：零外部依赖 + 自己过守门器 ─────────────────────

/// ⛔ 页面不许引任何外部资源（离线可看是硬要求）。
#[test]
fn the_page_pulls_in_nothing_from_outside() {
    let html = ontok::server::page::index_html();
    for bad in ["http://", "https://", "//cdn", "cdn.", "integrity="] {
        assert!(
            !html.contains(bad),
            "页面不得引外部资源 —— 判读人可能离线打开。出现了 `{bad}`"
        );
    }
}

/// 页面源码自己也过守门器（与 `cli.rs` 对 `--html` 的处置同一条）。
///
/// ⛔ 注意「证据」是守门词：页面里一律改说「说明」。
/// 这条测试会抓住任何人不小心把建模黑话写进 UI 文案。
#[test]
fn the_page_itself_has_no_modeling_terms() {
    let html = ontok::server::page::index_html();
    let hits = ontok::domain::view::scan_modeling_terms(&html);
    assert!(hits.is_empty(), "页面泄露了建模术语：{hits:?}");
}

/// ⭐ REQ-110 · **壳里不许有作用域**（AC-005 的模板层那一半）。
///
/// 收口前这条叫 `the_page_carries_its_scope`，断言「项目名要出现在页面上」——
/// 那正是 `/` 不能免 token 的原因之一（页面壳本身是一条数据出口）。
/// 现在作用域改走 gated API，所以判据**反过来**：
///
/// 1. 模板里没有任何项目名 / 版本号（没有占位符、也没有人再往里填）；
/// 2. 模板确实从 `api/scope` 取作用域（否则「零数据」就变成「没数据可用」，
///    页面永远画不出图 —— 那是另一种坏法，必须一起钉住）。
#[test]
fn the_shell_carries_no_scope_and_fetches_it_from_a_gated_api() {
    let html = ontok::server::page::index_html();
    assert!(!html.contains("demo3"), "页面壳里出现了项目名 —— 它必须走 gated API");
    assert!(!html.contains("%%PROJECT%%"), "占位符没被替换干净");
    assert!(!html.contains("%%VERSION%%"), "占位符没被替换干净");
    assert!(
        html.contains("api/scope"),
        "壳必须从 gated API 取作用域，否则页面永远画不出图"
    );
    assert!(
        html.contains("Authorization"),
        "取作用域的请求必须带口令"
    );
    assert!(
        html.contains("api/ontology.ttl"),
        "REQ-110：页面必须有「导出 TTL」的落点"
    );
}

// ── 红线文件：不得被本需求改动 ──────────────────────────
//
// ⚠️ **本文件证明不了「没被改动」** —— 那要看 diff。
// 这里能证明的只有一件弱得多的事：这些文件**还在、还读得到内容**
// （删掉一个红线文件会让 `read_source` 报错）。
// 真正的零触碰证据是交付时那份 `git diff --numstat`。

#[test]
fn red_line_files_are_still_present_and_readable() {
    for f in [
        "src/domain/view.rs",
        "src/domain/review_view.rs",
        "src/extract/names.rs",
        "src/extract/propose.rs",
        "src/mcp/serve.rs",
        "tests/drop_rate.rs",
        "tests/test_fk_naming.rs",
    ] {
        let s = read_source(f);
        assert!(!s.is_empty(), "红线文件 {f} 读不到内容");
    }
}
