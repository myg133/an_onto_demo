//! REQ-107 · AC-007：建模轨豁免必须是**显式声明**（TC-021 / TC-022）。
//!
//! # 要治的是什么
//!
//! 收口前的「豁免」是**推导出来的**：「因为它没调闸，所以它是建模轨」。
//! 那是 fail-open：漏接一个客户出口和「一条建模出口」在代码里长得一模一样。
//!
//! 现在轨道是 `OutletId` 里的**数据**（`Track::Modeling`），
//! 「拦不拦」由 `Track::gated()` 决定 —— 误标时**行为会变**（被闸拦住）。
//!
//! # ⚠️ 如实登记的边界
//!
//! D2–D8 目前**只做登记与站点标注**，没有改道经管线写出（理由见
//! `.feature/design-req107.md` §四：改道会同时打穿 REQ-072/078 的两套既有守卫）。
//! 误标的**行为可观察性**由 D1（真走管线）与下面的 TC-022 证明。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`（REQ-107 A7-1）。

mod common;

use ontok::domain::pipeline::{render, OutletId, Product, Track};
use ontok::domain::pipeline::{outlet, OUTLETS};

/// 出口层不该出现的判据直调（与 AC-003 的否定式判据同一张名单）。
const CRITERION_CALLS: &[&str] = &[
    "scan_modeling_terms(",
    "filter_view(",
    "has_raw_identifier(",
    "sanitize_display(",
    "tools::guard(",
];

fn criterion_hits(body: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        if line.contains("fn scan_modeling_terms(") {
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

/// 取一个函数的整个函数体（按花括号深度）。
fn fn_body(src: &str, name: &str) -> String {
    let hit = format!("fn {name}(");
    let Some(start) = src.find(&hit) else {
        return String::new();
    };
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
            break;
        }
    }
    out
}

// ══════════════════════════════════════════════════════════════
// TC-021 · 10 个建模出口在登记表里显式声明 track = Modeling
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_modeling_track_is_explicitly_declared() {
    // ① 恰好 10 条，ID 集合 == {D1..D10}（D9/D10 = REQ-110 的 TTL 导出）。
    let ids: Vec<&str> = OUTLETS
        .iter()
        .filter(|o| o.track() == Track::Modeling)
        .map(|o| o.id)
        .collect();
    let mut got = ids.clone();
    got.sort_unstable();
    let mut want: Vec<String> = (1..=10).map(|i| format!("D{i}")).collect();
    // 与 `got.sort_unstable()` 同一口径（字典序：D10 排在 D2 前面）。
    want.sort();
    let got: Vec<String> = got.iter().map(|s| s.to_string()).collect();
    assert_eq!(got, want, "建模轨必须是 D1..D10 这 10 条（多一条少一条都红）");

    // ② 每条 needle 必须在源码里命中（登记与源码不许漂移）。
    let mut sources: std::collections::BTreeMap<&str, String> = std::collections::BTreeMap::new();
    for rel in ["src/cli.rs", "src/server/http.rs", "src/mcp/serve.rs"] {
        sources.insert(rel, common::read_source(rel));
    }
    let mut missing = Vec::new();
    for o in OUTLETS.iter().filter(|o| o.track() == Track::Modeling) {
        let src = sources.get(o.path).expect("登记表里的 path 必须在被扫范围里");
        if !src.contains(o.call_site) {
            missing.push(format!("{}: {:?} 不在 {} 里", o.id, o.call_site, o.path));
        }
    }
    assert!(missing.is_empty(), "建模轨的站点标注漂移了：{missing:?}");

    // ③ 建模轨所在的函数体里**零判据直调**（豁免是被声明的，不是被漏掉的）。
    //    `ontology_cmd` 里同时住着 C1–C7 与 D1/D9/D10 —— 所以这一格实际验的是
    //    「出口层整体没有判据直调」，与 AC-003 同源。
    let cli = common::strip_comments(&sources["src/cli.rs"]);
    let cli = cli.split("#[cfg(test)]").next().unwrap_or("").to_string();
    for fname in ["ontology_cmd", "proposal_cmd", "review_cmd", "propose_cmd"] {
        let body = fn_body(&cli, fname);
        if body.is_empty() {
            continue;
        }
        let hits = criterion_hits(&body);
        assert!(
            hits.is_empty(),
            "`{fname}` 里出现了判据直调（建模轨应当零判据）：{hits:?}"
        );
    }

    // ④ 每条 Modeling 都必须带**非空**豁免理由（豁免不许沉默）。
    for o in OUTLETS.iter().filter(|o| o.track() == Track::Modeling) {
        assert!(
            !o.exemption_reason.trim().is_empty(),
            "建模出口 {} 没有写豁免依据 —— 那就又回到「因为它没调闸所以它是建模轨」",
            o.id
        );
    }

    // ⑤ D1 是**真走管线**的那一条（轨道声明真的参与运行时决策）。
    let d1 = outlet("D1").expect("登记表里必须有 D1");
    assert_eq!(
        d1.enforcement,
        ontok::domain::pipeline::Enforcement::Pipeline,
        "D1 必须真的经管线写出 —— 否则「误标 ⇒ 行为变」无从观察"
    );
}

// ══════════════════════════════════════════════════════════════
// TC-022 · 把建模出口误标成客户类 ⇒ 行为要变
// ══════════════════════════════════════════════════════════════

#[test]
fn test_req107_mislabeling_a_modeling_outlet_turns_it_red() {
    // 一份**建模轨产物**的同构替身（`project_model_track` 的输出形状：
    // 建模术语是它要被裁决的内容）。⛔ 不引真实快照，避免依赖数据库。
    let mv = serde_json::json!({
        "entity_types": [
            { "key": "sample_entity", "cardinality": "1:N", "properties": [
                { "name": "样例属性", "key": "sample_key" }
            ]}
        ]
    });
    let text = serde_json::to_string_pretty(&mv).unwrap();

    // 前提自检（纪律 E）：它**确实**含建模术语，否则下面两条是空转。
    assert!(
        !ontok::domain::view::scan_modeling_terms(&text).is_empty(),
        "前提不成立：这份建模轨替身居然不含任何守门词 ⇒ 本测试是空转"
    );

    // ② 声明为 Modeling ⇒ 原样输出（建模术语在这一轨是**内容**，不是泄露）。
    let ok = render(OutletId::D1, Product::Text(text.clone())).expect("建模轨必须放行");
    assert!(ok.as_str().contains("key"), "建模轨的输出必须含建模字段");
    assert_eq!(ok.as_str(), text, "建模轨必须**逐字节**原样输出（不剔不改名）");

    // ③ 同一份产物、把 spec 误标成 Customer ⇒ 必须被拦，且错误正文不含具体词条。
    //    （QA 变异 #4 的原形态 `spec.track = Track::Internal` 在这一版**写不出来**；
    //     源码侧另有 `tests/test_req107_source_enumeration.rs` 做否定式判据。）
    let err = render(OutletId::C3, Product::Text(text.clone()))
        .expect_err("同一份产物走客户轨必须被拦");
    assert!(err.is_leak(), "误标的失败必须是「发布闸命中」");
    assert!(
        !err.message().contains("主键") && !err.message().contains("基数"),
        "错误文案不许回显具体词条（那等于从错误信息里开后门）：{}",
        err.message()
    );

    // ③′ 换 ID **不能关闸**：客户类产物不许从建模轨入口发布（第 2 轮的运行时不变式）。
    let cv = ontok::domain::view::CustomerView { objects: vec![], relations: vec![] };
    let mismatch = render(OutletId::D1, Product::CustomerJson(&cv))
        .expect_err("客户类产物不许从建模轨出口发布");
    assert!(!mismatch.is_leak(), "这一条是「形态不相容」，不是闸命中");

    // ④ 反向：干净产物在客户轨必须放行（证明上面那条不是「客户轨恒红」）。
    let clean = serde_json::to_string_pretty(&serde_json::json!({ "name": "样例客户" })).unwrap();
    assert!(
        render(OutletId::C3, Product::Text(clean)).is_ok(),
        "干净的客户轨产物必须放行 —— 否则「误标必红」证明的是「客户轨恒红」"
    );
}
