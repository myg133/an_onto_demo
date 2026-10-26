//! REQ-107 · 出口收口的**结构判据**（TC-001 ~ TC-012）。
//!
//! # 这份文件守的是什么
//!
//! 本需求的核心不是「新管线更整齐」，而是 **「漏接一个出口时，门禁必须立刻红」**。
//! 所以这里的判据有三层，缺一不可：
//!
//! 1. **源码否定式**（TC-001 / TC-009 / TC-010）：出口文件里对判据的**直调点数为 0**。
//!    正向判据（「这三个文件调了共享函数」）钉不住 QA 变异 **M3** 的形状
//!    ——「保留共享调用、另加一份真在生效的本地判据」实测 9 条全绿
//!    （登记在 `src/domain/property_filter.rs` 的 ③ 段）。
//! 2. **类型层**（TC-008）：可写出的产物只能由管线构造（`compile_fail` doctest +
//!    `cargo test --doc`）。这是唯一能挡住「换一种写法绕过」的判据。
//! 3. **登记表双向对账**（TC-004/005/006/007）：33 条口径、ID 集合、needle 命中。
//!
//! # ⛔ 本文件**不是**元守卫层
//!
//! **刻意不加** `//! @guard-manifest: REQ-081`：加了就必须同步
//! `tests/guard_baseline.req081.txt`，而那超出本需求范围（REQ-107 A7-1）。
//!
//! # ⚠️ 已知边界（⛔ 不许改写成「已封堵」）
//!
//! - 源码扫描型判据认的是**拼写**：自造一个全新命名的本地判据
//!   （`fn local_ok(s) -> bool { s.contains("key") }`）**抓不到**。
//!   补强在 TC-008 的类型层与 TC-012 的「手搓新出口」那一格。
//! - needle 式对账抓得住**漂移**，抓不住**语义等价改写**。

mod common;

use common::{read_source, strip_comments};
use ontok::domain::pipeline::{outlet_count, Track, OUTLETS};
use std::collections::BTreeMap;

/// 三个**出口文件** —— 它们只许声明「格式 + 轨道」，不许自己调判据。
const OUTLET_FILES: &[&str] = &["src/cli.rs", "src/server/http.rs", "src/mcp/serve.rs"];

/// 管线的**唯一**所在。
const PIPELINE_SRC: &str = "src/domain/pipeline.rs";

/// 出口层**不许**出现的判据直调。
///
/// ⚠️ 用 `tools::guard(` 而不是裸 `guard(`：后者会把 `install_broken_pipe_guard()`
/// 这类无关调用算成命中（实测 `src/cli.rs` 里有两处）⇒ 判据恒红 = 恒假。
/// 这是 TC-001 用例原文的已知缺陷，交付物「异议」里已登记。
const CRITERION_CALLS: &[&str] = &[
    "scan_modeling_terms(",
    "filter_view(",
    "has_raw_identifier(",
    "sanitize_display(",
    "tools::guard(",
];

/// 剥注释 + 只留**测试模块之前**的生产代码。
fn prod(rel: &str) -> String {
    let raw = strip_comments(&read_source(rel));
    raw.split("#[cfg(test)]").next().unwrap_or("").to_string()
}

/// ⭐ **纯函数**：数出一段源码里的判据直调点（行号 1 基 + 命中的名字）。
///
/// 抽成纯函数是为了能**用合成输入自证判据有判别力**（TC-010 第 2/4 步）——
/// 只对真实源码断言 0 的判据无法区分「真的没有」与「扫描器自己坏了」。
fn criterion_sites(src: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        // 跳过 `fn xxx(` 的定义行：定义不是调用点（TC-003 单独管定义数）。
        let is_def = line.contains("fn scan_modeling_terms(")
            || line.contains("fn filter_view(")
            || line.contains("fn has_raw_identifier(")
            || line.contains("fn sanitize_display(")
            || line.contains("fn guard(");
        if is_def {
            continue;
        }
        for c in CRITERION_CALLS {
            if line.contains(c) {
                out.push((i + 1, *c));
            }
        }
    }
    out
}

// ══════════════════════════════════════════════════════════════
// TC-001 · 出口文件里判据直调点数 == 0
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_outlet_files_have_zero_criterion_call_sites() {
    let mut bad: Vec<String> = Vec::new();
    for rel in OUTLET_FILES {
        let src = prod(rel);
        for (line, name) in criterion_sites(&src) {
            bad.push(format!("  - {rel}:{line}  `{name}`"));
        }
    }
    assert!(
        bad.is_empty(),
        "⛔ 出口文件里还有 {} 处判据直调 —— 剔除/清洗/闸只许经由 `{PIPELINE_SRC}`：\n{}",
        bad.len(),
        bad.join("\n")
    );

    // 反空转（纪律 E）：判据必须认得出**今天已经不存在**的那种形状。
    assert_eq!(
        criterion_sites("let leaked = scan_modeling_terms(&json);").len(),
        1,
        "判据认不出 `scan_modeling_terms(` ⇒ 上面那圈「0 命中」是空转"
    );
    assert_eq!(
        criterion_sites("let f = crate::domain::property_filter::filter_view(&cv);").len(),
        1
    );
    // `install_broken_pipe_guard()` 这种无关词**不许**被算成判据（否则恒红）。
    assert!(criterion_sites("install_broken_pipe_guard();").is_empty());
}

// ══════════════════════════════════════════════════════════════
// TC-002 · 出口层只声明「格式 + 轨道」，不许自带开关
// ══════════════════════════════════════════════════════════════

/// 取一个函数的**整个函数体**（按花括号深度；含多行签名）。
fn fn_body<'a>(src: &'a str, name: &str) -> Option<String> {
    let hit = format!("fn {name}(");
    let start = src
        .find(&hit)
        .or_else(|| src.find(&format!("async {hit}")))?;
    let mut depth = 0i32;
    let mut opened = false;
    let mut out = String::new();
    for line in src[start..].lines() {
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if line.contains('{') {
            opened = true;
        }
        out.push_str(line);
        out.push('\n');
        if opened && depth <= 0 {
            return Some(out);
        }
    }
    None
}

#[test]
fn test_req107_outlet_only_declares_format_and_track() {
    // ① 管线入口的签名里不许有「要不要过闸 / 要不要剔除」这类开关。
    let pipe = prod(PIPELINE_SRC);
    for entry in ["pub fn render(", "pub fn publish_value("] {
        let at = pipe
            .find(entry)
            .unwrap_or_else(|| panic!("管线里找不到入口 `{entry}`"));
        let sig_end = pipe[at..].find('{').map(|n| at + n).unwrap_or(pipe.len());
        let sig = &pipe[at..sig_end];
        for forbidden in [
            "skip_guard",
            "bypass",
            "no_guard",
            "escape_guard",
            "guard: bool",
            "filter: bool",
        ] {
            assert!(
                !sig.contains(forbidden),
                "管线入口 `{entry}` 的签名里出现了 `{forbidden}` —— \
                 那等于把「拦不拦」的决定权还给出口"
            );
        }
    }

    // ② 出口函数体里不许再有「自己判红 → 自己退出」的分支。
    //    ⚠️ 必须按**函数体**判，不按整文件：`EXIT_LEAKED_MODELING_TERMS` 的定义
    //        与政策文案本来就在 `src/cli.rs`（`:355` / `:370`），按整文件判会恒红。
    let cases: &[(&str, &str)] = &[
        ("src/cli.rs", "ontology_cmd"),
        ("src/server/http.rs", "get_graph"),
        ("src/server/http.rs", "get_node"),
        ("src/server/http.rs", "index"),
        ("src/mcp/serve.rs", "dispatch"),
    ];
    let mut bad = Vec::new();
    for (rel, fname) in cases {
        let src = prod(rel);
        let body = fn_body(&src, fname)
            .unwrap_or_else(|| panic!("{rel} 里找不到 `fn {fname}(` —— 判据锚点失效了"));
        for forbidden in [
            "EXIT_LEAKED_MODELING_TERMS",
            "std::process::exit(",
            "if !leaked.is_empty()",
            "if !leaked_json.is_empty()",
        ] {
            if body.contains(forbidden) {
                bad.push(format!("  - {rel}::{fname} 里有 `{forbidden}`"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "⛔ 出口函数体里还有 {} 处「自己决定拦不拦」的分支（判红决策必须在管线内）：\n{}",
        bad.len(),
        bad.join("\n")
    );

    // ③ 出口必须把「轨道 + 格式」作为**声明**传进去，而不是自己拿布尔开关。
    for rel in OUTLET_FILES {
        let src = prod(rel);
        assert!(
            src.contains("OutletId::"),
            "{rel} 里没有任何 `OutletId::` —— 出口没有声明自己是谁，那它一定在自决"
        );
    }
    // 登记表里每条都必须把轨道与格式**都**填好（类型层：枚举，不是字符串开关）。
    for o in OUTLETS {
        let _ = (o.track(), o.format());
    }
}

// ══════════════════════════════════════════════════════════════
// TC-003 · 不许抄第二份判据（纪律 H）
// ══════════════════════════════════════════════════════════════

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

fn src_files() -> Vec<(String, String)> {
    let root = common::crate_root();
    let mut files = Vec::new();
    walk_rs(&root.join("src"), &mut files);
    files.sort();
    files
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel.clone(), read_source(&rel))
        })
        .collect()
}

/// 某个函数**定义**出现在哪几个文件里（只认 `fn name(` 的定义行，不认调用）。
fn fn_definitions(files: &[(String, String)], name: &str) -> Vec<String> {
    let needle = format!("fn {name}(");
    files
        .iter()
        .filter(|(_, src)| {
            src.lines().any(|l| {
                let t = l.trim_start();
                t.starts_with(&needle) || t.starts_with(&format!("pub {needle}"))
            })
        })
        .map(|(rel, _)| rel.clone())
        .collect()
}

#[test]
fn test_req107_no_second_criterion_implementation() {
    let files = src_files();
    assert!(!files.is_empty(), "src/ 一个 .rs 都没扫到 —— 判据本身可疑");

    // ① 守门器本体：全仓恰好一处，且在 `mcp/tools.rs`。
    let guards = fn_definitions(&files, "guard");
    assert_eq!(
        guards,
        vec!["src/mcp/tools.rs".to_string()],
        "`fn guard` 的定义必须恰好一处且在 src/mcp/tools.rs（AC-001：⛔ 不许把 guard 抄一份）"
    );

    // ② 术语判据本体：全仓恰好一处。
    let scans = fn_definitions(&files, "scan_modeling_terms");
    assert_eq!(
        scans,
        vec!["src/domain/view.rs".to_string()],
        "`fn scan_modeling_terms` 的定义必须恰好一处且在 src/domain/view.rs"
    );

    // ③ 错误文案闸：恰好一处。
    let safe = fn_definitions(&files, "safe_error");
    assert_eq!(
        safe,
        vec!["src/mcp/tools.rs".to_string()],
        "`fn safe_error` 的定义必须恰好一处"
    );

    // ④ 标识符形态判据：允许 2 处，但 identifier_shape 之外那处必须是**逐字委托**。
    let idents = fn_definitions(&files, "has_raw_identifier");
    assert!(
        !idents.is_empty() && idents.len() <= 2,
        "`fn has_raw_identifier` 的定义数是 {}（期望 1~2）—— 多出来的那份很可能带逻辑：{idents:?}",
        idents.len()
    );
    for rel in &idents {
        if rel == "src/domain/identifier_shape.rs" {
            continue;
        }
        let (_, src) = files.iter().find(|(r, _)| r == rel).unwrap();
        let body = fn_body(&src.replace("//", "\n//"), "has_raw_identifier")
            .unwrap_or_else(|| panic!("{rel} 里取不到 has_raw_identifier 的函数体"));
        // 去掉签名与花括号，只留语句。
        let inner: String = body
            .lines()
            .skip(1)
            .filter(|l| !l.trim().starts_with('}'))
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            inner.replace(' ', ""),
            "identifier_shape::has_raw_identifier(s)".replace(' ', ""),
            "{rel} 里的 `has_raw_identifier` 不是逐字委托，而是一份**带逻辑的副本**：{inner}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// TC-004 / TC-005 / TC-006 · 13 条出口逐条接线
// ══════════════════════════════════════════════════════════════

/// ⭐ **纯函数**：登记表里哪些条目的 call_site 在源码里找不到（空 = 全部接线到位）。
fn missing_call_sites(files: &BTreeMap<&str, String>) -> Vec<String> {
    let mut out = Vec::new();
    for o in OUTLETS {
        let Some(src) = files.get(o.path) else {
            out.push(format!("{}: 判定用的源文件 {} 不在表里", o.id, o.path));
            continue;
        };
        if !src.contains(o.call_site) {
            out.push(format!(
                "{}: 管线/写出点 {:?} 在 {} 里找不到",
                o.id, o.call_site, o.path
            ));
        }
    }
    out
}

fn outlet_sources() -> BTreeMap<&'static str, String> {
    let mut m = BTreeMap::new();
    for rel in OUTLET_FILES {
        m.insert(*rel, read_source(rel));
    }
    // ⭐ REQ-111 · 审核界面**不在** `OUTLET_FILES`（它不是客户出口、不走管线），
    // 但它往登记表里新增了 5 条出口（R1–R5）。登记表对账必须连它一起查 ——
    // 否则「新增出口的 needle 漂移」在这张表上永远看不见（这正是「漏接即红」
    // 那条判据要防的形态）。`tests/test_req107_source_enumeration.rs` 的 T1
    // 另从 `src/` 全目录枚举写点，两边合起来才是双向的。
    m.insert("src/review_ui/http.rs", read_source("src/review_ui/http.rs"));
    // ⭐ REQ-123 · 查询服务（REST / MCP / SSE / 审计）也不在 `OUTLET_FILES`
    // （不是客户出口、不走管线），但它在登记表里新增了 4 条内部轨出口（I14–I17）。
    // 同一个理由：needle 漂移必须在这张表上看得见。
    m.insert("src/query/http.rs", read_source("src/query/http.rs"));
    m.insert("src/query/audit.rs", read_source("src/query/audit.rs"));
    // ⭐ REQ-124 · 控制面也不在 `OUTLET_FILES`（管理通道，不是客户出口），
    // 但它在登记表里新增了 4 条内部轨出口（I18–I21）。同一个理由：needle 漂移要看得见。
    m.insert("src/console/http.rs", read_source("src/console/http.rs"));
    m.insert("src/console/decl.rs", read_source("src/console/decl.rs"));
    // ⭐ REQ-135 · `/metrics` 的**唯一**响应构造点（I28）在 `src/metrics.rs`。
    // 三个 http.rs 只调 `Metrics::http_response`，正文不在那里拼 —— 所以 needle
    // 对账必须落在 metrics.rs 上（否则「正文换了个写点」这张表看不见）。
    m.insert("src/metrics.rs", read_source("src/metrics.rs"));
    // ⭐ REQ-155 · Studio 页面壳的唯一响应构造点（I32）在 `src/studio/mod.rs`。
    // 三个资源（HTML / CSS / JS）都是 `include_str!` 常量、都调 `studio_response`，
    // 所以 needle 对账落在那个函数定义上 —— 「壳里混进一条运行期字符串」在这张表上看得见。
    m.insert("src/studio/mod.rs", read_source("src/studio/mod.rs"));
    m
}

#[test]
fn test_req107_cli_five_customer_outlets_are_wired() {
    let files = outlet_sources();
    let cli = &files["src/cli.rs"];

    // ① C1–C5 的五个写出点/管线调用点都在。
    for id in ["C1", "C2", "C3", "C4", "C5"] {
        let o = OUTLETS.iter().find(|o| o.id == id).unwrap();
        assert!(
            cli.contains(o.call_site),
            "{id} 的调用点 {:?} 在 src/cli.rs 里找不到",
            o.call_site
        );
        assert_eq!(o.track(), Track::Customer, "{id} 的轨道必须是客户");
        let want = match id {
            "C1" | "C2" => ontok::domain::pipeline::Format::Html,
            "C3" | "C5" => ontok::domain::pipeline::Format::Json,
            _ => ontok::domain::pipeline::Format::Text,
        };
        assert_eq!(o.format(), want, "{id} 的格式声明不对");
    }

    // ② C1（stdout）与 C2（--out 文件）必须**共用同一份产物**：同一个变量名 `html`，
    //    而且那个 `html` 就是管线产物本身（第 2 轮：`let html = publish_customer(…)`，
    //    不再有「先 `.as_str()` 派生一份再写」的中间形态）。
    assert!(
        cli.contains("crate::cli::emit_domain_text(html)")
            && cli.contains("std::fs::write(p, html.as_str())"),
        "C1/C2 必须写**同一份** `html`（`--out` 分支与 stdout 分支引用同一个变量）"
    );
    assert!(
        cli.contains("let html = publish_customer("),
        "`html` 必须**直接**由管线入口赋值（写出函数只收 `OutletRender`）—— \
         否则「先写后判」又在出口层活过来了"
    );
}

#[tokio::test]
async fn test_req107_http_three_outlets_are_wired() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    let files = outlet_sources();
    for id in ["H1", "H2", "H3"] {
        let o = OUTLETS.iter().find(|o| o.id == id).unwrap();
        assert!(
            files["src/server/http.rs"].contains(o.call_site),
            "{id} 的管线调用点 {:?} 在 src/server/http.rs 里找不到",
            o.call_site
        );
    }
    assert_eq!(
        OUTLETS.iter().find(|o| o.id == "H3").unwrap().granularity(),
        ontok::domain::pipeline::Granularity::PerNode,
        "H3（/api/node）必须是**逐节点**粒度（卡点②的裁定）"
    );

    // 行为半：干净视图 ⇒ 三条都 200。
    let clean = fixture_view(false);
    let st = ontok::server::http::state_from_view(&clean, "样例项目", 2, "correct-horse-battery", 0);
    let app = ontok::server::http::build_router(st.clone());
    let ids: Vec<String> = st_graph_keys(&clean);

    for uri in [
        "/".to_string(),
        "/api/graph?project=样例项目&version=2".to_string(),
        format!(
            "/api/node?project=样例项目&version=2&id={}",
            urlencode(&ids[0])
        ),
    ] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .header(header::AUTHORIZATION, "Bearer correct-horse-battery")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "干净视图下 {uri} 应当 200");
    }

    // 反向半：视图里有一个**脏对象名** ⇒ /api/graph 整份拒发（500），
    // 而 /api/node 取那个**干净**对象仍然 200（逐节点粒度的证据）。
    let dirty = fixture_view(true);
    let st2 = ontok::server::http::state_from_view(&dirty, "样例项目", 2, "correct-horse-battery", 0);
    let app2 = ontok::server::http::build_router(st2.clone());
    let keys2 = st_graph_keys(&dirty);

    let r = app2
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/graph?project=样例项目&version=2")
                .header(header::AUTHORIZATION, "Bearer correct-horse-battery")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "/api/graph 是**整份**粒度的出口：一个脏节点必须整份拒发"
    );

    let clean_key = keys2
        .iter()
        .find(|k| !k.contains("主键"))
        .expect("夹具里必须有一个干净节点");
    let r = app2
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/node?project=样例项目&version=2&id={}",
                    urlencode(clean_key)
                ))
                .header(header::AUTHORIZATION, "Bearer correct-horse-battery")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::OK,
        "/api/node 是**逐节点**粒度的出口：一个坏节点不许废掉别的节点（REQ-104 修的就是这个）"
    );
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn st_graph_keys(view: &ontok::domain::view::CustomerView) -> Vec<String> {
    ontok::domain::graph::project(view)
        .nodes
        .iter()
        .map(|n| n.key.clone())
        .collect()
}

/// 合成客户视图（⛔ 不含真实客户表名/列名；`主键` 是守门词本体）。
fn fixture_view(dirty: bool) -> ontok::domain::view::CustomerView {
    use ontok::domain::view::{CustomerView, EntityView, PropertyView, RelationView};
    let obj = |name: &str| EntityView {
        name: name.to_string(),
        note: Some("样例说明".to_string()),
        belongs_to: None,
        aliases: vec![],
        properties: vec![PropertyView {
            name: "样例属性".to_string(),
            note: None,
            required: false,
            sample: None,
        }],
    };
    let mut objects = vec![obj("样例客户")];
    let mut relations = vec![];
    if dirty {
        // 「主键」不是标识符形态 ⇒ 前处理剔不掉 ⇒ 一定会到达发布闸。
        // 关系也挂在脏对象上：`list_relations` 才有一条会命中闸的数据。
        objects.push(obj("主键台账"));
        relations.push(RelationView {
            from: "主键台账".to_string(),
            to: "样例客户".to_string(),
            how_many: "一对多".to_string(),
            meaning: "拥有".to_string(),
        });
    }
    CustomerView { objects, relations }
}

#[test]
fn test_req107_mcp_five_tools_share_one_pipeline_call() {
    use ontok::mcp::tools;

    let files = outlet_sources();
    let serve = &files["src/mcp/serve.rs"];

    // ① 结构半：`tools/call` 分支里判据直调 == 0，管线调用点 >= 1。
    let branch_start = serve
        .find("\"tools/call\" =>")
        .expect("serve.rs 里必须有 tools/call 分支");
    let branch = &serve[branch_start..];
    assert!(
        criterion_sites(branch).is_empty(),
        "`tools/call` 分支里还有判据直调：{:?}",
        criterion_sites(branch)
    );
    assert!(
        branch.contains("OutletId::McpTools"),
        "`tools/call` 分支里找不到管线调用点 `OutletId::McpTools`"
    );

    // ② 行为半。每条工具用**能取到脏数据**的实参（不传脏实参就等于没测到闸）。
    let ctx_dirty = ctx(fixture_view(true));
    let dirty_calls: [(&str, &str); 5] = [
        ("list_concepts", "{}"),
        ("describe_concept", r#"{"name":"样例概念"}"#),
        ("describe_object", r#"{"name":"主键台账"}"#),
        ("list_relations", "{}"),
        ("search_objects", r#"{"query":"主键"}"#),
    ];
    for (name, args) in dirty_calls {
        let out = tools_call(&ctx_dirty, name, args);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["result"]["isError"], true,
            "脏视图下 {name} 必须 fail-closed（isError:true），实际：{out}"
        );
        assert!(
            !out.contains("主键"),
            "脏视图下 {name} 的响应里不许带出守门词：{out}"
        );
    }

    // ③ 对照组（纪律 E）：干净视图下 5 条都必须有数据 ——
    //    「5 条全拒」在没有对照组时可能只是「所有调用都坏了」。
    let ctx_clean = ctx(fixture_view(false));
    let clean_calls: [(&str, &str); 5] = [
        ("list_concepts", "{}"),
        ("describe_concept", r#"{"name":"样例概念"}"#),
        ("describe_object", r#"{"name":"样例客户"}"#),
        ("list_relations", "{}"),
        ("search_objects", r#"{"query":"样例"}"#),
    ];
    for (name, args) in clean_calls {
        let out = tools_call(&ctx_clean, name, args);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v["result"]["isError"], false,
            "干净视图下 {name} 必须有数据（没有对照组的「全拒」可能只是全坏了）：{out}"
        );
    }

    // ④ 目录里就是这五个（集合相等：多一个少一个都红）。
    let mut got: Vec<&str> = tools::catalog().iter().map(|t| t.name).collect();
    got.sort_unstable();
    let want = [
        "describe_concept",
        "describe_object",
        "list_concepts",
        "list_relations",
        "search_objects",
    ];
    assert_eq!(got, want, "MCP 工具集合变了（M1–M5 的口径要对账）");
}

/// 发一条 `tools/call`（走**生产**的 `handle_line`）。
fn tools_call(ctx: &ontok::mcp::tools::OntologyContext, name: &str, args: &str) -> String {
    let line = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#
    );
    ontok::mcp::serve::handle_line(ctx, &line).expect("请求必须有响应")
}

/// 用**生产函数**装配一份 `OntologyContext`（不是手搓字段的复刻品）。
fn ctx(view: ontok::domain::view::CustomerView) -> ontok::mcp::tools::OntologyContext {
    use ontok::domain::concept::ConceptGroup;
    use std::collections::BTreeMap;

    let names: BTreeMap<String, String> = view
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| (format!("e{i}"), o.name.clone()))
        .collect();
    // 概念组把**全部**对象都收进来：脏对象若不在组里，`describe_concept` 就取不到它，
    // 那一条工具等于没测到闸（假绿）。
    let groups = vec![ConceptGroup {
        key: "sample-concept".to_string(),
        display_name: "样例概念".to_string(),
        members: names.keys().cloned().collect(),
        scope_hint: Some("样例概念的范围".to_string()),
        low_confidence: None,
        extracted_by: None,
        evidence: vec![],
    }];
    let overview = ontok::domain::overview::build_overview(&view, &groups, &names);
    let mut confirmation = BTreeMap::new();
    for g in &groups {
        confirmation.insert(
            g.display_name.clone(),
            ontok::mcp::tools::confirmation_of(g),
        );
    }
    ontok::mcp::tools::OntologyContext {
        project: "样例项目".to_string(),
        version: 2,
        frozen_at: "2026-01-01T00:00:00Z".to_string(),
        view,
        overview,
        names,
        confirmation,
    }
}

// ══════════════════════════════════════════════════════════════
// TC-007 · 登记表与 33 条口径双向对账
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_outlet_registry_matches_the_declared_baseline() {
    // ① 条数：口径 = 33（REQ-107）+ 6（REQ-110 的 TTL 出口 C6/C7/H4/H5/D9/D10）
    //        + 5（REQ-111 的审核界面出口 R1–R5）+ 1（REQ-116 的文档摄取产物 I10）
    //        + 2（REQ-121 的 R2RML 映射出口 I11/I12）
    //        + 1（REQ-122 的 SPARQL 查询回执出口 I13）
    //        + 4（REQ-123 的查询服务出口 I14–I17）
    //        + 4（REQ-124 的控制面出口 I18–I21）
    //        + 1（REQ-125 的工作台页面壳出口 I22）
    //        + 1（REQ-127 的查询服务 OWL TTL 出口 I25）
    //        + 2（REQ-129 的 `doc merge` 合并提案 I26 / `doc sign` 签字产物 I27）
    //        + 1（REQ-135 的 `/metrics` 指标响应 I28）
    //        + 3（REQ-144 的 access mapping：`validate` 报告 I29 / `fetch` JSON-LD I30 /
    //             `publish` 发布物 I31）
    //        + 1（REQ-155 的 Studio 页面壳 I32：三份编译期常量、零数据）
    //        ⚠️ REQ-155 删除客户门户：REQ-126 的 I23/I24 已移除 ⇒ 67 - 2 = 65。
    assert_eq!(
        OUTLETS.len(),
        65,
        "33（REQ-107）+ 6（REQ-110 TTL 出口）+ 5（REQ-111 审核界面出口）\
         + 1（REQ-116 文档摄取产物）+ 2（REQ-121 R2RML 出口）+ 1（REQ-122 SPARQL 回执）\
         + 4（REQ-123 REST / MCP / SSE / 审计）+ 4（REQ-124 控制面）\
         + 1（REQ-125 工作台页面壳）+ 1（REQ-127 OWL TTL 响应）\
         + 2（REQ-129 doc merge 合并提案 / doc sign 签字产物）\
         + 1（REQ-135 /metrics 指标响应）\
         + 3（REQ-144 access validate / fetch / publish）\
         + 1（REQ-155 Studio 页面壳）\
         - 2（REQ-155 删除客户门户：I23/I24）"
    );

    // ② 分组：客户 17 / 建模 10 / 内部 17 / 静态 2 / 错误路径 1。
    //
    // ⚠️ 与 TC-007 用例原文的差异（**AC/用例自身缺陷**，纪律 S）：
    // 原文写 `Static == 3`（Z2,Z3,Z4）+ ErrorPath 1 ⇒ 13+8+9+3+1 = **34**，
    // 与它自己断言的 `OUTLETS.len() == 33` 矛盾。按 A1 的裁定（Z4 不是出口）
    // 这里取 Static == 2，Z4 单列 `STATIC_TEMPLATES`。已在交付物「异议」登记。
    // REQ-110 把客户 13→17、建模 8→10（+6 条 TTL 出口），其余分组不动。
    // REQ-111 只动内部 9→14（R1–R5）：审核界面是**内部**工具（只绑回环 + 口令），
    // 既不是客户出口、也不是「建模视图」那 10 条 —— 改建模轨会放宽
    // `test_req107_track_manifest.rs` 的「恰好 D1–D10」不变量。
    // REQ-116 内部 14→15（I10）：文档摄取的产物写出点，**内部轨**（不写库、不改本体、不出图）。
    // REQ-121 内部 15→17（I11/I12）：R2RML 映射的 stdout / `--out` 两个写点
    // （同一份 Turtle 字节，两个落点 —— 与 C6/C7 同一条口径）。
    // REQ-122 内部 17→18（I13）：`sparql query` 的 JSON 回执（查询结果不是视图；
    // 隔离由声明驱动的租户注入 + 只读 + 行数上限保证，不是词汇闸）。
    // REQ-123 内部 18→22（I14–I17）：查询服务的 REST JSON / MCP JSON-RPC / SSE / 审计
    // （同一条口径：Agent 面的数据与通道，不是客户视图）。
    // REQ-124 内部 22→26（I18–I21）：控制面的 JSON / TTL / R2RML / 声明落盘
    // （管理通道，只有 admin token 能到；内容侧的闸在生成那一侧）。
    // REQ-125 内部 26→27（I22）：工作台的**页面壳**（零数据、匿名可达的 HTML 骨架）。
    // REQ-127 内部 29→30（I25）：查询服务的整份 OWL TTL 响应（C6/D9 的同一份字节，送达侧）。
    // REQ-129 内部 30→32（I26/I27）：`doc merge` 的合并提案 + `doc sign` 的签字产物
    // （都是候选提案，不是客户视图；物理写点仍复用 I10 的唯一 `fs::write`，
    // 这两条钉的是**新命令的写出调用**）。
    // REQ-135 内部 32→33（I28）：两个服务共用的 `/metrics` Prometheus 文本响应
    // （匿名可达但零业务数据；标签值类型层固定为 `&'static str`）。
    // REQ-144 内部 33→36（I29–I31）：access mapping 的三条写点
    // （`validate` 报告 / `fetch` JSON-LD / `publish` 发布物）—— 全是 FDE 与 Agent 面的
    // 内部产物，客户视图零读取；⛔ 阶段 0 不发网络请求，产物也⛔ 不含凭据值。
    // ⚠️ REQ-155 删除客户门户：REQ-126 的 I23/I24 随之移除 ⇒ 内部 36→34；同需求 P2 再加
    // I32（Studio 页面壳，匿名可达、零数据）⇒ 内部 34→35、总条数 64→65。
    assert_eq!(outlet_count(Track::Customer), 17);
    assert_eq!(outlet_count(Track::Modeling), 10);
    assert_eq!(outlet_count(Track::Internal), 35);
    assert_eq!(outlet_count(Track::Static), 2);
    assert_eq!(outlet_count(Track::ErrorPath), 1);

    // ③ ID 集合双向相等（多一条少一条都红）+ 每条都有 path / call_site。
    let mut ids: Vec<&str> = OUTLETS.iter().map(|o| o.id).collect();
    ids.sort_unstable();
    let total = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), total, "ID 必须唯一");

    let mut want: Vec<String> = Vec::new();
    want.extend((1..=7).map(|i| format!("C{i}")));
    want.extend((1..=5).map(|i| format!("H{i}")));
    want.extend((1..=5).map(|i| format!("M{i}")));
    want.extend((1..=10).map(|i| format!("D{i}")));
    // REQ-121 · I11/I12（R2RML 映射：stdout / `--out`）；REQ-122 · I13（SPARQL 回执）；
    // REQ-123 · I14–I17（查询服务：REST / MCP / SSE / 审计）；
    // REQ-124 · I18–I21（控制面：JSON / TTL / R2RML / 声明落盘）；
    // REQ-125 · I22（工作台页面壳：匿名可达、零数据）；
    // REQ-127 · I25（查询服务的整份 OWL TTL 响应）
    // REQ-129 · I26/I27（`doc merge` 的合并提案 / `doc sign` 的签字产物）
    // REQ-135 · I28（两个服务共用的 `/metrics` Prometheus 文本响应）
    // REQ-144 · I29–I31（access mapping：`validate` 报告 / `fetch` JSON-LD / `publish` 发布物）
    // ⭐ REQ-155 · I32（Studio 页面壳：匿名可达、零数据的三份静态常量）
    // ⚠️ REQ-155 删除了 REQ-126 的 I23/I24（客户门户）⇒ 编号留空，不回填。
    want.extend(
        (1..=32)
            .filter(|i| *i != 23 && *i != 24)
            .map(|i| format!("I{i}")),
    );
    // REQ-111 · 本地审核界面（内部轨）：读 / 写回执 / 批量预览 / 页面壳 / 错误通道。
    want.extend((1..=5).map(|i| format!("R{i}")));
    want.push("Z1".into());
    want.push("Z2".into());
    want.push("Z3".into());
    want.sort();
    let got: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    assert_eq!(got, want, "ID 集合与 AC 的编号集合必须**双向**相等");

    for o in OUTLETS {
        assert!(!o.path.is_empty(), "{} 没有登记 path", o.id);
        assert!(!o.call_site.is_empty(), "{} 没有登记 call_site", o.id);
    }

    // ④ 既有 `DOMAIN_PRINT_SITES`（9 条）**不是**本表的替代品。
    //
    // REQ-121 把 8→9：`r2rml generate` 的 stdout 是**新的 domain 打印点**
    // （不登记就是「新增写点没人管」）。
    assert_eq!(
        ontok::cli::DOMAIN_PRINT_SITES.len(),
        9,
        "`DOMAIN_PRINT_SITES` 是 REQ-078 的**打印点**登记表（9 条），\
         与 REQ-107 的出口表不是同一张表；条数变了说明有人拿它冒名顶替"
    );
    assert_ne!(
        ontok::cli::DOMAIN_PRINT_SITES.len(),
        OUTLETS.len(),
        "两张表条数相同会让「拿旧表顶替」变得不可见"
    );
}

// ══════════════════════════════════════════════════════════════
// TC-008 · 类型层：管线外构造不出可写出的产物
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_type_layer_blocks_off_pipeline_construction() {
    // ① 全仓必须有至少一条 `compile_fail` doctest，且它**不在 tests/ 里**、
    //    代码块里出现包装类型名与一次字面量构造。
    //
    // ⚠️ 必须先把 `//!` / `///` 前缀剥掉：doctest 的围栏写在文档注释里，
    //    直接按 ` ```compile_fail ` 找会一条都找不到（判据恒红/恒绿）。
    fn doc_text(src: &str) -> String {
        src.lines()
            .map(|l| {
                let t = l.trim_start();
                let t = t
                    .strip_prefix("//!")
                    .or_else(|| t.strip_prefix("///"))
                    .unwrap_or(t);
                t.trim_start()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    let mut found = Vec::new();
    for (rel, src) in src_files() {
        let doc = doc_text(&src);
        let mut in_block = false;
        let mut block = String::new();
        for line in doc.lines() {
            if line.trim_start().starts_with("```compile_fail") {
                in_block = true;
                block.clear();
                continue;
            }
            if in_block && line.trim_start().starts_with("```") {
                in_block = false;
                if block.contains("OutletRender") && block.contains("OutletRender {") {
                    found.push(rel.clone());
                }
                continue;
            }
            if in_block {
                block.push_str(line);
                block.push('\n');
            }
        }
    }
    assert!(
        !found.is_empty(),
        "全仓找不到一条「字面量构造 OutletRender 必须编不过」的 `compile_fail` doctest —— \
         类型层判据不存在（`cargo test --doc` 也守不住它）"
    );

    // ② 包装类型不得有 `pub` 字段、不得有公开构造器。
    let pipe = read_source(PIPELINE_SRC);
    let start = pipe.find("pub struct OutletRender").expect("管线里必须有 OutletRender");
    let end = start + pipe[start..].find("\n}").expect("取不到结构体结尾");
    let body = &pipe[start..end];
    for line in body.lines().skip(1) {
        assert!(
            !line.trim_start().starts_with("pub "),
            "`OutletRender` 有 `pub` 字段：{line} —— 出口就能字面量构造它了"
        );
    }
    assert!(
        !pipe.contains("pub fn new("),
        "管线里出现了 `pub fn new(` —— 公开构造器会直接绕过闸（`pub fn` 全枚举见 `test_req107_source_enumeration.rs`）"
    );

    // ③ 反空转：**在 crate 内**构造是合法的（`OutletRender::build` 就在做这件事），
    //    所以上面那条「编不过」是**私有性**造成的，不是路径写错造成的。
    assert!(
        pipe.contains("fn build(") && pipe.contains("OutletRender {"),
        "管线内部没有构造点 ⇒ 上面那条 compile_fail 可能只是「类型不存在」"
    );
}

// ══════════════════════════════════════════════════════════════
// TC-009 · 全仓否定式：判据直调只许出现在管线 / 守门器 / 测试
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_no_second_judgement_outside_pipeline() {
    let files = src_files();

    // ① 术语判据 `scan_modeling_terms(` 的**调用点**只许在：
    //    - 管线本体；
    //    - 守门器本体（`mcp::tools` 里 `guard` / `safe_error` 调它）。
    //    （`tests/` 不在本扫描范围 —— 这个函数只看 `src/`。）
    //
    // ⚠️ 刻意**不**把 `has_raw_identifier(` / `sanitize_display(` 放进全仓名单：
    //    它们的**定义与内部使用**本来就在 `identifier_shape` / `html_view` /
    //    `property_filter` 里（那是判据侧，由 REQ-103 管）。
    //    对这三个名字的「出口侧为 0」判据在 TC-001（只看三个出口文件）里。
    const ALLOWED: &[&str] = &["src/domain/pipeline.rs", "src/mcp/tools.rs"];
    let mut out = Vec::new();
    for (rel, src) in &files {
        if ALLOWED.contains(&rel.as_str()) {
            continue;
        }
        let code = strip_comments(src).split("#[cfg(test)]").next().unwrap_or("").to_string();
        for (i, line) in code.lines().enumerate() {
            if line.contains("scan_modeling_terms(") && !line.contains("fn scan_modeling_terms(") {
                out.push(format!("  - {rel}:{}", i + 1));
            }
        }
    }
    assert!(
        out.is_empty(),
        "⛔ 全仓有 {} 处「管线/守门器之外的术语判据直调」：\n{}",
        out.len(),
        out.join("\n")
    );

    // ② `filter_view(` 只许在管线里（定义处除外）。
    let mut fv = Vec::new();
    for (rel, src) in &files {
        if rel == "src/domain/property_filter.rs" || rel == "src/domain/pipeline.rs" {
            continue;
        }
        let code = strip_comments(src).split("#[cfg(test)]").next().unwrap_or("").to_string();
        for (i, line) in code.lines().enumerate() {
            if line.contains("filter_view(") {
                fv.push(format!("  - {rel}:{}", i + 1));
            }
        }
    }
    assert!(
        fv.is_empty(),
        "`filter_view(` 只许在管线模块里被**调用**（定义处除外），实际：\n{}",
        fv.join("\n")
    );
}

// ══════════════════════════════════════════════════════════════
// TC-010 · ⭐ M3 专项：保留共享调用 + 另加一份本地判据 必须红
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_m3_keeps_shared_call_and_adds_local_criterion() {
    // ② 自证：喂一段与**收口前** `src/cli.rs` 同形的合成串。
    let m3_like = r#"
crate::domain::property_filter::filter_view(&cv);
let leaked = crate::domain::view::scan_modeling_terms(&json);
if !leaked.is_empty() { std::process::exit(2); }
"#;
    let hits = criterion_sites(m3_like);
    assert_eq!(
        hits.len(),
        2,
        "M3 形状应当命中 2 处（filter_view + scan_modeling_terms），实际 {hits:?}"
    );
    assert!(!hits.is_empty());

    // ④ 反向自证：只有管线调用的串必须 0 命中（防判据被改成恒红）。
    let clean = "let rendered = crate::domain::pipeline::render(OutletId::C3, Product::CustomerJson(&cv));";
    assert_eq!(criterion_sites(clean).len(), 0);

    // ③ 实物：三个出口文件各自 0 命中（与 TC-001 同源，但这里逐文件报数）。
    for rel in OUTLET_FILES {
        let src = prod(rel);
        let sites = criterion_sites(&src);
        assert!(
            sites.is_empty(),
            "{rel} 里还有 {} 处判据直调：{:?}",
            sites.len(),
            sites
        );
    }
}

// ══════════════════════════════════════════════════════════════
// TC-011 / TC-012 · 漏接即红：两种形态的**可执行**证明
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_removing_an_outlet_breaks_the_registry() {
    // 基线：真实源码上 0 缺失。
    assert!(
        missing_call_sites(&outlet_sources()).is_empty(),
        "基线就不成立：{:?}",
        missing_call_sites(&outlet_sources())
    );

    // TC-011 的形态：把 C3 的写出行摘掉 ⇒ 登记表对账必须红。
    let mut files = outlet_sources();
    let cli = files.remove("src/cli.rs").unwrap();
    let mutated = cli.replace("crate::cli::emit_domain_line(json);", "");
    assert_ne!(mutated, cli, "变异没生效 —— 摘除锚点写错了");
    files.insert("src/cli.rs", mutated);
    let missing = missing_call_sites(&files);
    assert!(
        missing.iter().any(|m| m.starts_with("C3:")),
        "把 C3 的写出点摘掉后登记表**没有**报缺 —— 「漏接即红」在这一格不成立：{missing:?}"
    );
}

#[test]
fn test_req107_a_hand_rolled_new_customer_exit_would_be_caught() {
    // TC-012 的形态：在 `ontology_cmd` 里手搓一条新出口（不经管线、不调判据）。
    // 判据：该函数里的 stdout 写出点，只要摸了「domain 产物」就必须出现在登记表里。
    const PRODUCERS: &[&str] = &[
        "to_string_pretty",
        "render_html",
        "render_overview_text",
        "build_overview",
        "project_model_track",
        "filter_view",
    ];

    fn unregistered_writes(src: &str, bindings: &[&str]) -> Vec<String> {
        let Some(body) = fn_body(src, "ontology_cmd") else {
            return vec!["找不到 ontology_cmd".to_string()];
        };
        let mut out = Vec::new();
        for (i, line) in body.lines().enumerate() {
            let writes = line.contains("println!(")
                || line.contains("print!(")
                || line.contains("emit_domain_text(")
                || line.contains("emit_domain_line(");
            if !writes {
                continue;
            }
            let touches_domain = PRODUCERS.iter().any(|p| line.contains(p));
            if !touches_domain {
                continue;
            }
            if bindings.iter().any(|b| line.contains(b)) {
                continue;
            }
            out.push(format!("ontology_cmd:{}  {line}", i + 1));
        }
        out
    }

    let bindings: Vec<&str> = OUTLETS
        .iter()
        .filter(|o| o.path == "src/cli.rs")
        .map(|o| o.call_site)
        .collect();
    let cli = read_source("src/cli.rs");

    // 基线：真实源码里没有「未登记的手搓出口」。
    assert!(
        unregistered_writes(&cli, &bindings).is_empty(),
        "基线就不成立：{:?}",
        unregistered_writes(&cli, &bindings)
    );

    // 变异：插一条手搓出口。
    let anchor = "crate::cli::emit_domain_line(json);";
    let at = cli.find(anchor).expect("找不到 C3 的写出行");
    let mut mutated = String::with_capacity(cli.len() + 80);
    mutated.push_str(&cli[..at + anchor.len()]);
    mutated.push_str("\n            println!(\"{}\", serde_json::to_string_pretty(&cv)?);");
    mutated.push_str(&cli[at + anchor.len()..]);

    let bad = unregistered_writes(&mutated, &bindings);
    assert!(
        !bad.is_empty(),
        "手搓一条新客户出口后判据**没有**报 —— TC-012 的「漏接即红」在这一格不成立"
    );
}
