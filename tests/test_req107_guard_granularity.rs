//! REQ-107 · 三个卡点的**行为差异**判据（TC-013 / TC-014 / TC-015）。
//!
//! # 这三条今天都是绿的 —— 它们的价值不是抓 bug
//!
//! 价值是让「静默改口径」当场变红：AC-004 要的是**裁定 + 理由 + 被放弃方案的代价**
//! （文本在 `.feature/design-req107.md` 与交付物里），而这里钉的是
//! **裁定之后的行为不能被悄悄改掉**。
//!
//! | 卡点 | 裁定 | 本文件钉的行为 |
//! | --- | --- | --- |
//! | ① 改名口径 | JSON **不改名** | HTML 显示「客户编号」，JSON 仍是「客户编号ID」 |
//! | ② 闸粒度 | 双粒度，**由 spec 声明** | `/api/graph` 整份拒、`/api/node` 逐节点 |
//! | ③ stderr 报告 | 两级 + 形状唯一 | 正文只报数量；stderr 报原名；形状来自 `DropReport` |
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

mod common;

use ontok::domain::pipeline::{render, DropReport, OutletId, Product};
use ontok::domain::view::{CustomerView, EntityView, PropertyView, RelationView};

// ══════════════════════════════════════════════════════════════
// 夹具（⛔ 不含真实客户表名/列名/中文名；一律 `样例` 开头）
// ══════════════════════════════════════════════════════════════

fn view_with(prop_name: &str) -> CustomerView {
    CustomerView {
        objects: vec![EntityView {
            name: "样例客户".to_string(),
            note: Some("样例说明".to_string()),
            belongs_to: None,
            aliases: vec![],
            properties: vec![PropertyView {
                name: prop_name.to_string(),
                note: None,
                required: false,
                sample: None,
            }],
        }],
        relations: vec![],
    }
}

/// 两个对象：干净 `样例客户`、脏 `主键台账`（守门词本体，且**不是**标识符形态
/// ⇒ `filter_view` 剔不掉，见 `tests/test_property_filter.rs` 的 AC-002 对照组）。
fn two_objects(relations: bool) -> CustomerView {
    let obj = |name: &str| EntityView {
        name: name.to_string(),
        note: None,
        belongs_to: None,
        aliases: vec![],
        properties: vec![PropertyView {
            name: "样例属性".to_string(),
            note: None,
            required: false,
            sample: None,
        }],
    };
    CustomerView {
        objects: vec![obj("样例客户"), obj("主键台账")],
        relations: if relations {
            vec![RelationView {
                from: "主键台账".to_string(),
                to: "样例客户".to_string(),
                how_many: "一对多".to_string(),
                meaning: "拥有".to_string(),
            }]
        } else {
            vec![]
        },
    }
}

fn html_of(cv: &CustomerView) -> String {
    render(
        OutletId::C1,
        Product::CustomerHtml { view: cv, sampling: &[] },
    )
    .expect("干净夹具必须过闸")
    .into_text()
}

fn json_of(cv: &CustomerView) -> String {
    render(OutletId::C3, Product::CustomerJson(cv))
        .expect("干净夹具必须过闸")
        .into_text()
}

// ══════════════════════════════════════════════════════════════
// 卡点① · 改名口径：HTML 改名、JSON 不改名
// ══════════════════════════════════════════════════════════════

/// 同一份 cv：HTML 经 `sanitize_display` 摘掉 `ID` 记号，JSON 只过 `filter_view`
/// （**不改名**）—— 两个出口对同一属性给出两个名字。
///
/// **裁定：JSON 保持原名**（理由见设计文档 §三·①）。若哪天有人把 JSON 也接上
/// `strip_id_token`，本条第 4 步当场红 —— 那正是「静默改口径」的样子。
#[test]
fn test_req107_cardpoint1_rename_divergence_between_html_and_json() {
    let cv = view_with("客户编号ID");
    let page = html_of(&cv);
    let json = json_of(&cv);

    assert!(
        !page.contains("客户编号ID"),
        "HTML 出口应当摘掉 ID 记号（sanitize_display）"
    );
    assert!(page.contains("客户编号"), "HTML 出口应当留着业务名：{page}");
    assert!(
        json.contains("客户编号ID"),
        "**裁定：JSON 不改名** —— 它是数据通道，名字是**标识**不是文案：{json}"
    );

    // 两个出口不可能同时「都改名」或「都不改名」。
    let html_renamed = !page.contains("客户编号ID");
    let json_renamed = !json.contains("客户编号ID");
    assert_ne!(
        html_renamed, json_renamed,
        "两个出口的改名口径必须**显式不同**（卡点①的裁定：HTML 改名 / JSON 不改名）"
    );
}

// ══════════════════════════════════════════════════════════════
// 卡点② · 闸粒度：整份 vs 逐节点（三个断言缺一不可）
// ══════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_req107_cardpoint2_whole_document_vs_per_node_granularity() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    let cv = two_objects(true);
    let keys: Vec<String> = ontok::domain::graph::project(&cv)
        .nodes
        .iter()
        .map(|n| n.key.clone())
        .collect();
    let dirty_key = keys.iter().find(|k| k.contains("主键")).unwrap().clone();
    let clean_key = keys.iter().find(|k| !k.contains("主键")).unwrap().clone();

    let st = ontok::server::http::state_from_view(&cv, "样例项目", 2, "correct-horse-battery", 0);
    let app = ontok::server::http::build_router(st);
    let get = |uri: String| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .uri(uri)
                    .header(header::AUTHORIZATION, "Bearer correct-horse-battery")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };
    let body = |r: axum::response::Response| async move {
        String::from_utf8_lossy(&axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap())
            .to_string()
    };

    // ② `/api/graph` 是**整份**粒度：一个坏节点 ⇒ 整份 500，且不顺带发出干净数据。
    let r = get("/api/graph?project=样例项目&version=2".to_string()).await;
    assert_eq!(
        r.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "`/api/graph` 是整份粒度的出口：一份坏数据必须整份拒发"
    );
    let b = body(r).await;
    assert!(!b.contains("主键"), "拒发时不许把守门词发出去：{b}");
    assert!(
        !b.contains("样例客户"),
        "拒发时不许顺带把干净数据发出去：{b}"
    );

    // ③ 干净节点 `/api/node` ⇒ **200**。这才是「逐节点」的证据：
    //    一个坏节点没有废掉整页（REQ-104 修的就是这个）。
    let r = get(format!(
        "/api/node?project=样例项目&version=2&id={}",
        urlencode(&clean_key)
    ))
    .await;
    assert_eq!(
        r.status(),
        StatusCode::OK,
        "`/api/node` 是逐节点粒度的出口：干净节点必须打得开"
    );
    assert!(body(r).await.contains("样例客户"));

    // ④ 脏节点 `/api/node` ⇒ 500，且不发守门词。
    let r = get(format!(
        "/api/node?project=样例项目&version=2&id={}",
        urlencode(&dirty_key)
    ))
    .await;
    assert_eq!(
        r.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "脏节点自己必须被拒（逐节点 ≠ 放过脏节点）"
    );
    let b = body(r).await;
    assert!(!b.contains("主键"), "拒发时不许把守门词发出去：{b}");

    // ⑤ 「整份」侧的无 DB 近似证据：若统一到整份，C3 侧必然拒发。
    let filtered = ontok::domain::property_filter::filter_view(&cv);
    let whole = serde_json::to_string_pretty(&filtered.view).unwrap();
    assert!(
        !ontok::domain::view::scan_modeling_terms(&whole).is_empty(),
        "前提自检：这份视图剔完仍然脏 —— 否则上面几条的差异无从谈起"
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

// ══════════════════════════════════════════════════════════════
// 卡点③ · stderr 报告形状：正文只报数量，stderr 报原名
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_cardpoint3_report_shape_count_vs_raw_names() {
    use ontok::domain::property_filter::filter_view;

    // `Key` 命中 `identifier_shape::is_modeling_literal` ⇒ 整条属性剔除。
    let cv = view_with("样例关联的 Key 凭据");

    // ② 页面正文：**只报数量，绝不报原名**（`html_view.rs` 的既有措辞）。
    let page = html_of(&cv);
    assert!(
        !page.contains("样例关联的 Key 凭据"),
        "页面正文不许出现被剔掉的原名（那等于把技术串再泄一次）"
    );
    assert!(
        page.contains("另有 1 项"),
        "页面正文必须**报数量**（静默剔除是本项目最贵的 fail-open）：{page}"
    );

    // ③ 运维面（stderr）：带原名 —— 运维要靠它定位到具体属性。
    let filtered = filter_view(&cv);
    assert!(
        filtered.report().contains("样例关联的 Key 凭据"),
        "`FilteredView::report()` 必须带原名（运维面）：{}",
        filtered.report()
    );

    // ④ `RenderedPage.skipped_props` 同样带原名（CLI 收口前把它打到 stderr）。
    let page_struct = ontok::domain::html_view::render_html(&cv, &[]);
    assert!(
        page_struct
            .skipped_props
            .iter()
            .any(|s| s.contains("样例关联的 Key 凭据")),
        "`RenderedPage.skipped_props` 必须带原名：{:?}",
        page_struct.skipped_props
    );

    // ⑤ ⭐ 形状统一的可机判形式：所有出口的 stderr 报告**只经** `DropReport::stderr_line()`。
    let rendered = render(
        OutletId::C3,
        Product::CustomerJson(&cv),
    )
    .expect("这份视图剔完必须干净");
    let line = rendered.report().stderr_line();
    assert!(
        line.starts_with(ontok::domain::pipeline::DROP_REPORT_PREFIX),
        "报告抬头必须来自唯一常量 `DROP_REPORT_PREFIX`：{line}"
    );
    assert!(line.contains("共剔除 1 项"), "报告必须报总数：{line}");
    assert!(line.contains("整条属性 1"), "报告必须分类计数：{line}");
    assert!(
        line.contains("样例关联的 Key 凭据"),
        "运维面报告必须带原名：{line}"
    );
    // 空报告不产文本（否则每个干净出口都会刷一行噪声）。
    assert!(DropReport::default().stderr_line().is_empty());
    assert!(DropReport::default().count(ontok::domain::pipeline::DropKind::Object) == 0);
    let _ = common::crate_root();
}

// ══════════════════════════════════════════════════════════════
// 附加：`ontology overview` 的两条形态**都**过闸（既有过度拦截，原样保留）
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_overview_gates_both_formats() {
    use ontok::domain::concept::ConceptGroup;
    use std::collections::BTreeMap;

    let cv = view_with("样例属性");
    let names: BTreeMap<String, String> = cv
        .objects
        .iter()
        .enumerate()
        .map(|(i, o)| (format!("e{i}"), o.name.clone()))
        .collect();
    let groups = vec![ConceptGroup {
        key: "c".to_string(),
        display_name: "样例概念".to_string(),
        members: vec!["e0".to_string()],
        scope_hint: None,
        low_confidence: None,
        extracted_by: None,
        evidence: vec![],
    }];
    let ov = ontok::domain::overview::build_overview(&cv, &groups, &names);

    // 干净总览：两条形态都过。
    assert!(render(OutletId::C4, Product::Overview(&ov)).is_ok());
    assert!(render(OutletId::C5, Product::Overview(&ov)).is_ok());

    // 脏总览：两条形态**都**拒（收口前的过度拦截原样保留 —— AC-005 要求零可见变化）。
    let dirty = CustomerView {
        objects: vec![EntityView {
            name: "主键台账".to_string(),
            note: None,
            belongs_to: None,
            aliases: vec![],
            properties: vec![],
        }],
        relations: vec![],
    };
    let names2: BTreeMap<String, String> =
        vec![("e0".to_string(), "主键台账".to_string())].into_iter().collect();
    let ov2 = ontok::domain::overview::build_overview(&dirty, &[], &names2);
    assert!(
        render(OutletId::C4, Product::Overview(&ov2)).is_err(),
        "文本形态命中 ⇒ 必须拒发"
    );
    assert!(
        render(OutletId::C5, Product::Overview(&ov2)).is_err(),
        "JSON 形态命中 ⇒ 必须拒发（两条形态都判，是既有行为）"
    );
}
