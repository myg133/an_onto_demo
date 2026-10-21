//! QA 独立复算（REQ-107 AC-005）：**在基线 worktree（b714f48）**用基线 API 重算
//! 几个客户类出口的字节，写到 `REQ107_QA_OUT`，随后由 PowerShell 独立取 sha256
//! 与入库 golden 比对。
//!
//! ⚠️ 本文件是 QA 的临时工具，只在 `.review-workspace/req107/base` 里存在（未跟踪）。

#[path = "common/req107.rs"]
mod fixture_mod;

use fixture_mod::{overview, sampling, fixture};

const TOKEN: &str = "correct-horse-battery";
const PROJECT: &str = "样例项目";

#[tokio::test]
async fn qa_recompute_outlet_bytes() {
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;

    let Ok(out_dir) = std::env::var("REQ107_QA_OUT") else {
        eprintln!("· REQ107_QA_OUT 未设置 ⇒ 不产出");
        return;
    };
    let dir = std::path::PathBuf::from(out_dir);
    std::fs::create_dir_all(&dir).unwrap();

    let cv = fixture();

    // C1: render_html（基线 API）
    let page = ontok::domain::html_view::render_html(&cv, &sampling());
    std::fs::write(dir.join("C1_html.txt"), page.html.as_bytes()).unwrap();

    // C3: filter_view + to_string_pretty + 尾换行（emit_domain_line 语义）
    let filtered = ontok::domain::property_filter::filter_view(&cv);
    let mut json = serde_json::to_string_pretty(&filtered.view).unwrap();
    json.push('\n');
    std::fs::write(dir.join("C3_json.txt"), json.as_bytes()).unwrap();

    // C4 / C5: 业务总览
    let ov = overview();
    std::fs::write(
        dir.join("C4_overview.txt"),
        ontok::domain::overview::render_overview_text(&ov).as_bytes(),
    )
    .unwrap();
    let mut ovj = serde_json::to_string_pretty(&ov).unwrap();
    ovj.push('\n');
    std::fs::write(dir.join("C5_overview_json.txt"), ovj.as_bytes()).unwrap();

    // H1 / H2: 基线路由（与生产同一条装配函数）
    let st = ontok::server::http::state_from_view(&cv, PROJECT, 2, TOKEN);
    let app = ontok::server::http::build_router(st);
    for (name, uri) in [
        ("H1_index.html", "/".to_string()),
        ("H2_graph.json", format!("/api/graph?project={PROJECT}&version=2")),
    ] {
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(r.status().is_success(), "{name} 必须 200");
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        std::fs::write(dir.join(name), &b).unwrap();
    }
    eprintln!("· QA 独立复算完成：{}", dir.display());
}
