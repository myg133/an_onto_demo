//! REQ-135 · **Prometheus 文本格式合法性**（AC-002，逐条断言）。
//!
//! 本机没有 `promtool`（需求风险 4），所以判据是**手写校验器**
//! [`ontok::metrics::validate_exposition`] —— 不是「跳过」：
//!
//! 1. 三个服务各渲染一份 → 全部通过校验器；
//! 2. 六种「手写格式会犯的错」各喂一次 → 校验器必须**逐条**报出来（反空转）；
//! 3. 若给了 `ONTOK_REQ135_METRICS_FILE`（真服务 `curl /metrics` 的抓取结果），
//!    对**真实输出**再跑一次同样的校验。

use ontok::metrics::{
    Metrics, VIEW_CUSTOMER, VIEW_MODEL,
};

/// 三个服务各产一份「有流量之后」的 /metrics 正文。
fn exercised(service: &str) -> (Metrics, String) {
    let m = match service {
        "ontok-query" => Metrics::for_query(),
        "ontok-console" => Metrics::for_console(),
        "ontok-portal" => Metrics::for_portal(),
        other => panic!("不认识的服务 {other}"),
    };
    m.observe_request("/api/health", "200", std::time::Duration::from_millis(2));
    m.observe_request("/metrics", "200", std::time::Duration::from_millis(1));
    m.observe_request("/api/sparql", "403", std::time::Duration::from_millis(120));
    m.rows(5);
    m.note_denial_code(match service {
        "ontok-query" => "TENANT_MISMATCH",
        "ontok-console" => "TOKEN_KIND_FORBIDDEN",
        _ => "TENANT_FROM_CLIENT",
    });
    m.set_ttl_bytes(VIEW_CUSTOMER, 4_321);
    m.set_ttl_bytes(VIEW_MODEL, 12_345);
    m.set_mapping_joins(9);
    let text = m.render();
    (m, text)
}

#[test]
fn all_three_services_render_valid_exposition() {
    for service in ["ontok-query", "ontok-console", "ontok-portal"] {
        let (_, text) = exercised(service);
        if let Err(errs) = ontok::metrics::validate_exposition(&text) {
            panic!(
                "{service} 的 /metrics 不合法（{} 处）：\n{}\n\n----- 原文 -----\n{text}",
                errs.len(),
                errs.join("\n")
            );
        }
        // 每个服务都必须恰好有六个指标族（少一个 = 忘了接线）。
        for name in [
            "ontok_requests_total",
            "ontok_request_duration_seconds",
            "ontok_rows_returned_total",
            "ontok_tenant_denied_total",
            "ontok_ttl_bytes",
            "ontok_mapping_joins",
        ] {
            assert!(
                text.contains(&format!("# HELP {name} ")),
                "{service} 缺 # HELP {name}"
            );
            assert!(
                text.contains(&format!("# TYPE {name} ")),
                "{service} 缺 # TYPE {name}"
            );
        }
        // 直方图必须真的有三件套（AC-002 的点名判据）。
        for suffix in ["_bucket", "_sum", "_count"] {
            assert!(
                text.contains(&format!("ontok_request_duration_seconds{suffix}{{")),
                "{service} 的直方图缺 {suffix}"
            );
        }
        assert!(
            text.contains("ontok_request_duration_seconds_bucket{") && text.contains("le=\"+Inf\""),
            "{service} 的直方图必须以 +Inf 收尾"
        );
    }
}

#[test]
fn the_validator_catches_each_hand_written_format_error() {
    let (_, good) = exercised("ontok-query");
    let cases: Vec<(&str, String)> = vec![
        ("缺 # TYPE", good.replace("# TYPE ontok_mapping_joins gauge\n", "")),
        (
            "类型写错",
            good.replace("# TYPE ontok_ttl_bytes gauge", "# TYPE ontok_ttl_bytes counter"),
        ),
        (
            "取值不是数字",
            good.replace("ontok_mapping_joins 9", "ontok_mapping_joins nine"),
        ),
        (
            "标签没有引号",
            good.replace("view=\"customer\"", "view=customer"),
        ),
        (
            "标签集合不对",
            good.replace("ontok_mapping_joins 9", "ontok_mapping_joins{tenant=\"X\"} 9"),
        ),
        (
            "同一组标签出现两次",
            good.replace("ontok_mapping_joins 9", "ontok_mapping_joins 9\nontok_mapping_joins 10"),
        ),
        (
            "直方图缺 _sum",
            good.replace(
                "ontok_request_duration_seconds_sum{service=\"ontok-query\",endpoint=\"/api/sparql\"}",
                "ontok_request_duration_seconds_summary{service=\"ontok-query\",endpoint=\"/api/sparql\"}",
            ),
        ),
    ];
    for (label, broken) in cases {
        let errs = ontok::metrics::validate_exposition(&broken).expect_err(&format!(
            "这种坏格式必须被拒（{label}）：\n{broken}"
        ));
        assert!(!errs.is_empty(), "{label}：报错清单不许是空的");
    }
}

/// 真实抓取的输出（若有）：`curl -s localhost:PORT/metrics > /tmp/metrics.txt`。
///
/// 缺这个变量时**响亮地跳过**（与 REQ-131 的口径一致：默认不失败，但要说出来）。
#[test]
fn the_captured_live_output_is_valid_too() {
    let Ok(path) = std::env::var("ONTOK_REQ135_METRICS_FILE") else {
        eprintln!(
            "*** SKIP: the_captured_live_output_is_valid_too -- missing ONTOK_REQ135_METRICS_FILE ***"
        );
        return;
    };
    let text = std::fs::read_to_string(&path).expect("读抓取文件");
    assert!(
        text.contains("# TYPE ontok_requests_total counter"),
        "{path} 看起来不是 /metrics 的输出"
    );
    if let Err(errs) = ontok::metrics::validate_exposition(&text) {
        panic!("真实抓取的 /metrics 不合法：\n{}", errs.join("\n"));
    }
}
