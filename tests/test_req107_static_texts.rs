//! REQ-107 · AC-008：`tools/list` 与 `initialize` 的**静态文本**（TC-023 / TC-025）。
//!
//! # 洞是什么
//!
//! - `tools/list` 的 `description` / `title` 从**真实响应体**发给客户端与下游 LLM，
//!   而收口前这两个字段**连断言都没有**（`tests/test_mcp_server.rs` 只查
//!   `name` 与 `inputSchema`）；
//! - `initialize` 的 `instructions` **模板正文**从不过闸 —— `tools::instructions`
//!   只扫**项目名**（那是动态的），模板正文（静态的）没人扫。
//!
//! # 裁定：**运行时闸**（AC-008 的二选一）
//!
//! `tools/list` 与 `initialize` 的整段 result 都经 `pipeline::publish_value`
//! （`OutletId::Z2` / `Z3`）。理由见 `.feature/design-req107.md` §五：
//! ① 「MCP 的全部外送」真的收在一个函数上，AC-003 的漏接判据自动覆盖它们；
//! ② 静态文本今天干净 ⇒ 零可见变化；③ 测试守卫**枚举不了** Z3 里的动态部分
//! （项目名 + 版本号），只做测试守卫等于只守了一半。
//!
//! **仍然补断言**（运行时闸只保证「不干净就不发」，不保证「发出了该有的东西」——
//! 字段被删是静默的）。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

mod common;

use ontok::domain::concept::ConceptGroup;
use ontok::domain::view::{CustomerView, EntityView, PropertyView};
use ontok::mcp::tools::{self, OntologyContext};
use std::collections::BTreeMap;

const TEMPLATE_MARKER: &str = "该版本已冻结";
const CONFIRMATION_MARKER: &str = "confirmation";

fn ctx() -> OntologyContext {
    let view = CustomerView {
        objects: vec![EntityView {
            name: "样例客户".to_string(),
            note: Some("样例说明".to_string()),
            belongs_to: None,
            aliases: vec![],
            properties: vec![PropertyView {
                name: "样例属性".to_string(),
                note: None,
                required: false,
                sample: None,
            }],
        }],
        relations: vec![],
    };
    let names: BTreeMap<String, String> =
        vec![("e0".to_string(), "样例客户".to_string())].into_iter().collect();
    let groups = vec![ConceptGroup {
        key: "sample".to_string(),
        display_name: "样例概念".to_string(),
        members: vec!["e0".to_string()],
        scope_hint: Some("样例概念的范围".to_string()),
        low_confidence: None,
        extracted_by: None,
        evidence: vec![],
    }];
    let overview = ontok::domain::overview::build_overview(&view, &groups, &names);
    let mut confirmation = BTreeMap::new();
    for g in &groups {
        confirmation.insert(g.display_name.clone(), tools::confirmation_of(g));
    }
    OntologyContext {
        project: "样例项目".to_string(),
        version: 2,
        frozen_at: "2026-01-01T00:00:00Z".to_string(),
        view,
        overview,
        names,
        confirmation,
    }
}

fn reply(ctx: &OntologyContext, line: &str) -> serde_json::Value {
    let out = ontok::mcp::serve::handle_line(ctx, line).expect("请求必须有响应");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("响应不是合法 JSON：{e}\n{out}"))
}

// ══════════════════════════════════════════════════════════════
// TC-023 · `description` / `title` 的断言（今天零断言的那两格）
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_tools_list_descriptions_and_titles_are_asserted() {
    let catalog = tools::catalog();
    assert_eq!(catalog.len(), 5, "MCP 工具必须是 5 个（M1–M5）");

    for t in &catalog {
        // ① 非空 + 长度下界。下界取 20：少于 20 字不可能说清「用它回答什么」，
        //    而一个说不出用途的 description 对下游 LLM 是纯噪声。
        assert!(
            !t.description.trim().is_empty(),
            "工具 {} 的 description 是空的",
            t.name
        );
        assert!(
            t.description.chars().count() >= 20,
            "工具 {} 的 description 太短（{} 字 < 20）—— 它说不清「用它回答什么」：{}",
            t.name,
            t.description.chars().count(),
            t.description
        );
        assert!(!t.title.trim().is_empty(), "工具 {} 的 title 是空的", t.name);

        // ② ⭐ 今天**完全没有**的那一格：静态文本零守门词。
        let hits = ontok::domain::view::scan_modeling_terms(t.description);
        assert!(
            hits.is_empty(),
            "工具 {} 的 description 带建模术语 {hits:?}：{}",
            t.name,
            t.description
        );
        let hits = ontok::domain::view::scan_modeling_terms(t.title);
        assert!(
            hits.is_empty(),
            "工具 {} 的 title 带建模术语 {hits:?}：{}",
            t.name,
            t.title
        );
    }

    // ③ 名字集合恰好是这五个。
    let mut names: Vec<&str> = catalog.iter().map(|t| t.name).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "describe_concept",
            "describe_object",
            "list_concepts",
            "list_relations",
            "search_objects"
        ]
    );

    // ④ 响应半：`tools/list` 的真实响应体必须**逐字**带着同一份文本 ——
    //    防 `serve.rs` 那个 `json!` 映射把字段写错/丢掉（Rust 编译器看不见 JSON 键名）。
    let r = reply(&ctx(), r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
    let got = r["result"]["tools"].as_array().expect("tools 必须是数组");
    assert_eq!(got.len(), catalog.len());
    for (t, g) in catalog.iter().zip(got.iter()) {
        assert_eq!(g["description"].as_str(), Some(t.description), "{} 的 description 在响应里变了", t.name);
        assert_eq!(g["title"].as_str(), Some(t.title), "{} 的 title 在响应里变了", t.name);
    }
}

// ══════════════════════════════════════════════════════════════
// TC-025 · `initialize` 的 instructions **模板正文**被守卫
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_initialize_instructions_template_body_is_guarded() {
    let c = ctx();
    let text = tools::instructions(&c);

    // ① 整段（模板正文 + 项目名 + 版本）零守门词。
    let hits = ontok::domain::view::scan_modeling_terms(&text);
    assert!(hits.is_empty(), "instructions 带建模术语 {hits:?}：{text}");

    // ② 前提自检：模板正文**还在**。若哪天模板被清空，第 ① 步会恒绿（纪律 E）。
    assert!(
        text.contains(TEMPLATE_MARKER),
        "instructions 里找不到模板正文的固定句子「{TEMPLATE_MARKER}」—— \
         模板被清空的话，上面的「零守门词」是空转：{text}"
    );
    assert!(
        text.contains(CONFIRMATION_MARKER),
        "instructions 里找不到「{CONFIRMATION_MARKER}」—— 下游 LLM 就不知道该带限定：{text}"
    );
    assert!(
        text.contains(&format!("第 {} 版", c.version)),
        "instructions 必须点名版本：{text}"
    );
    assert!(text.contains(&c.project), "干净项目名必须原样出现：{text}");

    // ③ 项目名带守门词时，整段仍然干净（`tools::instructions` 的既有处置）。
    let mut dirty = ctx();
    dirty.project = "外键登记表".to_string();
    let dirty_text = tools::instructions(&dirty);
    assert!(
        ontok::domain::view::scan_modeling_terms(&dirty_text).is_empty(),
        "项目名带守门词时 instructions 漏了：{dirty_text}"
    );

    // ④ 响应半：`initialize` 的整段 result 走运行时闸（`OutletId::Z3`）。
    let r = reply(
        &ctx(),
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
    );
    let instructions = r["result"]["instructions"]
        .as_str()
        .expect("initialize 必须带 instructions");
    assert_eq!(instructions, tools::instructions(&ctx()), "响应里的 instructions 与生产函数不一致");
    assert!(
        r["result"]["serverInfo"]["name"].as_str().is_some(),
        "serverInfo 也在这条外送路径上"
    );

    // ⑤ 结构半：Z2 / Z3 必须真的在管线调用点上（不是「登记了但没接线」）。
    let serve = common::strip_comments(&common::read_source("src/mcp/serve.rs"));
    let serve_code = serve.split("#[cfg(test)]").next().unwrap_or("").to_string();
    for (branch, spec) in [
        ("\"tools/list\" =>", "OutletId::Z2"),
        ("\"initialize\" =>", "OutletId::Z3"),
    ] {
        let at = serve_code
            .find(branch)
            .unwrap_or_else(|| panic!("serve.rs 里找不到分支 {branch}"));
        let tail = &serve_code[at..];
        let end = tail[1..]
            .find("\n        \"")
            .map(|n| n + 1)
            .unwrap_or(tail.len());
        assert!(
            tail[..end].contains(spec),
            "{branch} 分支里没有管线调用点 `{spec}` —— AC-008 的二选一落空了"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// Z4 · `INDEX_HTML` 模板：不单列运行时闸，但被 H1 的整页扫描 + 测试守卫覆盖
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_z4_index_template_is_covered_by_tests_and_the_homepage_gate() {
    // ① 模板本体（渲染后）零守门词 —— 既有 `test_graph_server.rs` 也钉这一条，
    //    这里再钉一次是为了让「Z4 由测试守卫覆盖」这句话在本文件里可核对。
    let html = ontok::server::page::index_html("样例项目", 2).expect("干净项目名必须出页面");
    let hits = ontok::domain::view::scan_modeling_terms(&html);
    assert!(hits.is_empty(), "INDEX_HTML 模板带建模术语 {hits:?}");

    // ② Z4 **不占出口位**（AC §七 A1），但必须被**显式登记**在模板守卫表里。
    assert!(
        ontok::domain::pipeline::STATIC_TEMPLATES
            .iter()
            .any(|(path, name)| *path == "src/server/page.rs" && *name == "INDEX_HTML"),
        "Z4 没有被登记进 `STATIC_TEMPLATES` —— 「它由测试守卫覆盖」就成了一句空话"
    );

    // ③ 运行时侧：H1 的管线对**渲染后的整页**过闸（模板本体因此被覆盖）。
    let http = common::strip_comments(&common::read_source("src/server/http.rs"));
    let start = http.find("async fn index(").expect("找不到 index");
    let tail = &http[start..];
    let end = tail[1..].find("\nasync fn ").map(|n| n + 1).unwrap_or(tail.len());
    let index_fn = &tail[..end];
    assert!(
        index_fn.contains("OutletId::H1"),
        "H1 没有经由管线：{index_fn}"
    );
    assert!(
        index_fn.contains("Product::Text("),
        "H1 必须把**渲染后的整页**交给管线过闸（那是 Z4 的运行时覆盖）：{index_fn}"
    );
}
