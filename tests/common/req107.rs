//! REQ-107 · **golden 比对用的合成夹具**（AC-005）。
//!
//! # 为什么单独一个子模块
//!
//! 同一份夹具要同时喂给两条判据：
//!
//! 1. `tests/test_req107_zero_visible_change.rs` —— **基线兼容**的复刻版
//!    （在 `b714f48` 的 worktree 里也能编译、也能产出 golden）；
//! 2. `tests/test_req107_pipeline_bytes.rs` —— 收口后的**生产**管线产出必须
//!    逐字节等于同一份 golden。
//!
//! 两条判据若各写一份夹具，夹具一漂就会「两边都对、其实不是同一份数据」
//! （纪律 H 的忠实重复实现）。⇒ 夹具只有这一份。
//!
//! # ⛔ 合成夹具纪律（REQ-104 栽过）
//!
//! **不得含真实客户表名 / 中文名 / 列名 / 列注释。** 本文件里：
//! - 对象名一律 `样例…` 开头；
//! - 源库标识符一律 `wg_` 前缀的合成 token（`wg_order_ref` / `wg_col_ref`）；
//! - `Key` 是**被测的建模术语字面量本身**，换掉夹具就废了（保留，见
//!   `tests/test_property_filter.rs` 的约定）。
//!
//! # 夹具刻意覆盖的四条路径
//!
//! | 现象 | 夹具里的载体 | 走哪条路 |
//! | --- | --- | --- |
//! | HTML 改名（`客户编号ID` → `客户编号`） | 属性名 `客户编号ID` | `sanitize_display` |
//! | 整条属性剔除 | 属性名 `样例关联的 Key 凭据` | 两套引擎都命中 |
//! | 对象说明整段隐去 | 对象说明 `来源 wg_order_ref 的订单` | snake_case 形态 |
//! | 只隐去说明 | 属性 `样例属性说明` 的说明 `对应 wg_col_ref` | 名字干净、说明脏 |
//!
//! 覆盖它们是为了让 golden **有内容可比**：一份全是干净数据的夹具，
//! 「零变化」这句话几乎不可能被打破。

#![allow(dead_code)]

use ontok::domain::concept::ConceptGroup;
use ontok::domain::overview::{build_overview, Overview};
use ontok::domain::view::{CustomerView, EntityView, PropertyView, RelationView};
use std::collections::BTreeMap;

/// 抽样高亮清单（判读手册 §3.1 的形态；只影响 ★ 标记）。
pub fn sampling() -> Vec<String> {
    vec!["样例客户".to_string()]
}

/// 一份覆盖四条前处理路径的客户视图。
pub fn fixture() -> CustomerView {
    CustomerView {
        objects: vec![
            EntityView {
                name: "样例客户".to_string(),
                note: Some("样例客户这张表的用途说明".to_string()),
                belongs_to: None,
                aliases: vec!["样例甲方".to_string()],
                properties: vec![
                    PropertyView {
                        name: "客户编号ID".to_string(),
                        note: None,
                        required: true,
                        sample: Some("样例-001".to_string()),
                    },
                    PropertyView {
                        name: "样例关联的 Key 凭据".to_string(),
                        note: None,
                        required: false,
                        sample: None,
                    },
                    PropertyView {
                        name: "下单时间".to_string(),
                        note: Some("样例客户第一次下单的时间".to_string()),
                        required: false,
                        sample: None,
                    },
                ],
            },
            EntityView {
                name: "样例订单".to_string(),
                // 表注释原文形态（合成 token）：前处理会整段隐去。
                note: Some("来源 wg_order_ref 的订单".to_string()),
                belongs_to: Some("样例客户".to_string()),
                aliases: vec![],
                properties: vec![
                    PropertyView {
                        name: "金额".to_string(),
                        note: None,
                        required: true,
                        sample: Some("100".to_string()),
                    },
                    PropertyView {
                        name: "样例属性说明".to_string(),
                        // 名字干净、说明脏 ⇒ 只隐去说明。
                        note: Some("对应 wg_col_ref".to_string()),
                        required: false,
                        sample: None,
                    },
                ],
            },
        ],
        relations: vec![
            RelationView {
                from: "样例客户".to_string(),
                to: "样例订单".to_string(),
                how_many: "一对多".to_string(),
                meaning: "拥有".to_string(),
            },
            RelationView {
                from: "样例订单".to_string(),
                to: "样例客户".to_string(),
                how_many: "多对一".to_string(),
                meaning: "属于".to_string(),
            },
        ],
    }
}

/// 概念组（`build_overview` 的入参）。
pub fn concept_groups() -> Vec<ConceptGroup> {
    vec![ConceptGroup {
        key: "sample-customer".to_string(),
        display_name: "样例客户概念".to_string(),
        members: vec!["e0".to_string(), "e1".to_string()],
        scope_hint: Some("样例客户与订单的范围".to_string()),
        low_confidence: None,
        extracted_by: None,
        evidence: vec![],
    }]
}

/// 实体 key → 展示名（生产侧由 `snapshot_to_view` 得到；合成夹具直接编号）。
pub fn names() -> BTreeMap<String, String> {
    let cv = fixture();
    cv.objects
        .iter()
        .enumerate()
        .map(|(i, o)| (format!("e{i}"), o.name.clone()))
        .collect()
}

/// 业务总览（`build_overview` 是生产函数，不手搓）。
pub fn overview() -> Overview {
    build_overview(&fixture(), &concept_groups(), &names())
}

/// 13 份 golden 的**文件名**（C2 复用 C1；H3 两份；M1–M5 各一份）。
pub const GOLDEN_FILES: &[(&str, &str)] = &[
    ("C1", "C1_html.txt"),
    ("C3", "C3_json.txt"),
    ("C4", "C4_overview.txt"),
    ("C5", "C5_overview_json.txt"),
    ("H1", "H1_index.html"),
    ("H2", "H2_graph.json"),
    ("H3a", "H3_node_1.json"),
    ("H3b", "H3_node_2.json"),
    ("M1", "M1.json"),
    ("M2", "M2.json"),
    ("M3", "M3.json"),
    ("M4", "M4.json"),
    ("M5", "M5.json"),
];

/// MCP 五条调用的实参（**固定**，否则 golden 不可复现）。
pub const MCP_CALLS: &[(&str, &str, &str)] = &[
    ("M1", "list_concepts", "{}"),
    ("M2", "describe_concept", r#"{"name":"样例客户概念"}"#),
    ("M3", "describe_object", r#"{"name":"样例客户"}"#),
    ("M4", "list_relations", r#"{"object":"样例客户"}"#),
    ("M5", "search_objects", r#"{"query":"样例"}"#),
];
