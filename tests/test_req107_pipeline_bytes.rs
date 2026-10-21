//! REQ-107 · AC-005 的**生产侧**半边：管线产出必须逐字节等于同一份 golden。
//!
//! # 为什么这条必须与 `test_req107_zero_visible_change.rs` 并存
//!
//! 那一份是**基线兼容**的（要在 `b714f48` 的 worktree 里跑生成器），所以它用
//! 基线就有的函数**复刻**各出口 —— 复刻与生产可能分叉（纪律 H）。
//! 这一份跑在**收口之后**的代码上，直接调**生产管线**：
//! 管线产出 ≠ golden ⇒ 红。两条合起来才把「零可见变化」钉在**生产**上。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

mod common;

use common::req107::{concept_groups, fixture, names, overview, sampling, GOLDEN_FILES, MCP_CALLS};
use ontok::domain::pipeline::{render, OutletId, Product};

const TOKEN: &str = "correct-horse-battery";
const PROJECT: &str = "样例项目";

fn golden(name: &str) -> Vec<u8> {
    // ⚠️ 走锚点（`tests/test_anchor_guard.rs` G-1 禁止裸 `env!("CARGO_MANIFEST_DIR")`）。
    let dir = common::crate_root()
        .join("tests")
        .join("fixtures")
        .join("req107")
        .join("golden");
    std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("读不到 golden {name}: {e}"))
}

fn assert_same(id: &str, name: &str, want: &[u8], got: &[u8]) {
    if want == got {
        return;
    }
    let at = want
        .iter()
        .zip(got.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| want.len().min(got.len()));
    let lo = at.saturating_sub(40);
    let ctx = |s: &[u8]| String::from_utf8_lossy(&s[lo..(at + 40).min(s.len())]).to_string();
    panic!(
        "出口 {id}（{name}）的**管线产出**与 golden 不同 —— 这就是一次可见变化。\n\
         第一处差异：偏移 {at}（golden 长 {} / 现在长 {}）\n\
         golden: {:?}\n\
         now   : {:?}",
        want.len(),
        got.len(),
        ctx(want),
        ctx(got)
    );
}

#[tokio::test]
async fn test_req107_pipeline_bytes_equal_golden() {
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;

    let cv = fixture();

    // ── C1 / C2 ──
    let c1 = render(
        OutletId::C1,
        Product::CustomerHtml { view: &cv, sampling: &sampling() },
    )
    .expect("干净夹具必须过闸")
    .into_text();
    assert_same("C1", "C1_html.txt", &golden("C1_html.txt"), c1.as_bytes());

    // ── C3 ──
    //
    // ⚠️ 出口的 stdout 字节 = 产物 ++ 尾换行（`emit_domain_line` 的 `println!` 语义）。
    //    管线产物本身不带那个换行 —— golden 记的是**出口字节**，所以这里补上。
    let c3 = render(OutletId::C3, Product::CustomerJson(&cv))
        .expect("干净夹具必须过闸")
        .into_text();
    assert_same(
        "C3",
        "C3_json.txt",
        &golden("C3_json.txt"),
        format!("{c3}\n").as_bytes(),
    );

    // ── C4 / C5 ──
    let ov = overview();
    let c4 = render(OutletId::C4, Product::Overview(&ov))
        .expect("干净总览必须过闸")
        .into_text();
    assert_same("C4", "C4_overview.txt", &golden("C4_overview.txt"), c4.as_bytes());
    let c5 = render(OutletId::C5, Product::Overview(&ov))
        .expect("干净总览必须过闸")
        .into_text();
    assert_same(
        "C5",
        "C5_overview_json.txt",
        &golden("C5_overview_json.txt"),
        format!("{c5}\n").as_bytes(),
    );

    // ── H1 / H2 / H3：走**真实路由**（响应体就是出口字节）──
    let st = ontok::server::http::state_from_view(&cv, PROJECT, 2, TOKEN, 0);
    let app = ontok::server::http::build_router(st.clone());
    let get = |uri: String| {
        let app = app.clone();
        async move {
            let r = app
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = r.status();
            let body = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
            (status, body.to_vec())
        }
    };

    let (s, b) = get("/".to_string()).await;
    assert!(s.is_success());
    assert_same("H1", "H1_index.html", &golden("H1_index.html"), &b);

    let (s, b) = get(format!("/api/graph?project={PROJECT}&version=2")).await;
    assert!(s.is_success());
    assert_same("H2", "H2_graph.json", &golden("H2_graph.json"), &b);

    let filtered = ontok::domain::property_filter::filter_view(&cv);
    let keys: Vec<String> = ontok::domain::graph::project(&filtered.view)
        .nodes
        .iter()
        .map(|n| n.key.clone())
        .collect();
    for (id, key, file) in [
        ("H3a", &keys[0], "H3_node_1.json"),
        ("H3b", &keys[1], "H3_node_2.json"),
    ] {
        let (s, b) = get(format!(
            "/api/node?project={PROJECT}&version=2&id={}",
            urlencode(key)
        ))
        .await;
        assert!(s.is_success());
        assert_same(id, file, &golden(file), &b);
    }

    // ── M1–M5 ──
    let mut confirmation = std::collections::BTreeMap::new();
    for g in concept_groups() {
        confirmation.insert(g.display_name.clone(), ontok::mcp::tools::confirmation_of(&g));
    }
    let ctx = ontok::mcp::tools::OntologyContext {
        project: PROJECT.to_string(),
        version: 2,
        frozen_at: "2026-01-01T00:00:00Z".to_string(),
        view: filtered.view.clone(),
        overview: ov.clone(),
        names: names(),
        confirmation,
    };
    for (id, tool, args) in MCP_CALLS {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#
        );
        let resp = ontok::mcp::serve::handle_line(&ctx, &line).expect("请求必须有响应");
        let name = GOLDEN_FILES
            .iter()
            .find(|(gid, _)| gid == id)
            .map(|(_, n)| *n)
            .unwrap();
        assert_same(id, name, &golden(name), resp.as_bytes());
    }
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
