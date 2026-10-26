//! REQ-152 · 契约 v2：`@pending.get` 给**够用的取数配方**（AC-101 / AC-103 / AC-105 / AC-106 / AC-107）。
//!
//! 这里量的是**契约层**的三件事（不起服务、不连库）：
//!
//! | # | 断言 | 对应 AC |
//! | --- | --- | --- |
//! | ① | v1 / 不写版本 ⇒ `get` **只有旧 4 键**，配方段一个不出现 | AC-103 / R2 |
//! | ② | v2 ⇒ 六段配方都在，且 `headers` 里写什么就透出什么（**零厂商判断**） | AC-101 / AC-105 |
//! | ③ | 映射没给的项 ⇒ 那个键**不存在**（⛔ 不按 `mode` 猜默认值） | AC-106 |
//! | ④ | 单类配方 > 12 KB ⇒ **加载期报错并点名**（⛔ 不静默截断） | AC-107 |
//!
//! ⚠️ 夹具里刻意用两家云的 header 名（`X-Amz-*` / `X-TC-*`）：它们只出现在**映射**里，
//! 产品代码（`src/query/`、`src/access/`）里一个都没有 —— 这正是 AC-105 的形态。

use ontok::domain::model_view::{ModelEntityView, ModelLinkView, ModelTrackView};
use ontok::query::source_gate::{AccessMapping, SourceCheck};
use ontok::sparql::query::parse as parse_query;

const IRI: &str = "https://ontok.local/demo";

fn model() -> ModelTrackView {
    let entity = |key: &str, name: &str| ModelEntityView {
        key: key.to_string(),
        display_name: name.to_string(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    };
    ModelTrackView {
        seq: 2,
        extracted_by: None,
        entity_types: vec![entity("order", "订单"), entity("customers", "客户")],
        link_types: vec![ModelLinkView {
            key: "order_customer_id__customers".to_string(),
            from_type: "order".to_string(),
            to_type: "customers".to_string(),
            cardinality: "N:1".to_string(),
            semantic: "belongs_to".to_string(),
            evidence: vec![],
            confidence: None,
            join_verified: None,
            join_evidence: None,
            column: Some("customer_id".to_string()),
        }],
        action_types: vec![],
        functions: vec![],
    }
}

/// 一份**最小 v2 映射**（非腾讯形状：`X-Amz-Target` + `aws-sigv4`）。
fn v2_mapping(contract_version: Option<u32>, with_auth_decl: bool) -> String {
    let cv = match contract_version {
        Some(v) => format!("\"contract_version\": {v},"),
        None => String::new(),
    };
    let auth_extra = if with_auth_decl {
        r#",
           "signed_headers": ["content-type", "host", "x-amz-target"],
           "timestamp": { "header": "X-Amz-Date", "format": "rfc3339" },
           "scope": ["date", "region", "service", "aws4_request"]"#
    } else {
        ""
    };
    format!(
        r#"{{{cv}
          "mapping_version": 7,
          "sources": [{{
            "id": "s1",
            "base_url": "https://ec2.example.com",
            "auth": {{ "mode": "aws-sigv4", "credential_env": ["AWS_ACCESS_KEY_ID"]{auth_extra} }},
            "defaults": {{ "region": "us-east-1" }}
          }}],
          "entities": [{{
            "id": "Customer", "class": "customers", "source": "s1",
            "capability": {{ "pushdown": ["ids"] }},
            "operation": {{
              "method": "POST", "path": "/",
              "headers": {{ "X-Amz-Target": "{{target}}", "Content-Type": "application/json" }},
              "vars": {{ "target": "DescribeCustomers" }}
            }},
            "request": {{
              "required": ["region"],
              "template": {{ "Region": "{{region}}" }},
              "bindings": {{ "region": {{ "source": "default" }} }},
              "paging": {{ "offset": "NextToken", "limit": "MaxResults" }}
            }},
            "iterator": "$.Response.CustomerSet[*]",
            "fields": {{ "name": {{ "path": "CustomerName" }} }},
            "freshness": "realtime"
          }}]
        }}"#
    )
}

fn pending_get(mapping: &str) -> ontok::query::source_gate::GetInstruction {
    let m = AccessMapping::parse(mapping).expect("解析").bind(&model()).expect("对账");
    let q = parse_query("SELECT ?c WHERE { ?c a :customers }", IRI).expect("查询");
    let c = m.check(&q, IRI, None);
    assert!(matches!(c, SourceCheck::Api { .. }), "{c:?}");
    c.pending()[0].get.clone().expect("按类点名必须给 get")
}

/// ① v1（或不写版本）⇒ **只有旧 4 键**，配方段一个都没有。
#[test]
fn contract_v1_has_only_the_four_legacy_keys() {
    for cv in [None, Some(1)] {
        let get = pending_get(&v2_mapping(cv, true));
        assert_eq!(get.class, "customers");
        assert_eq!(get.reference, "access#Customer@v7");
        assert_eq!(get.pushdown, vec!["ids".to_string()]);
        assert!(
            get.recipe.is_none(),
            "contract_version={cv:?} 不该有配方（AC-103：旧 4 键，一个不多）"
        );
        // 摊平后的形状就是 4 个键 —— 与 v1 基线逐字节同形。
        let text = serde_json::to_string(&serde_json::json!({
            "class": get.class, "filter": {"ids": get.ids}, "pushdown": get.pushdown, "ref": get.reference
        }))
        .unwrap();
        assert!(!text.contains("operation") && !text.contains("auth"), "{text}");
    }
}

/// ② v2 ⇒ 六段都在；`headers` **逐字透出**（写 `X-Amz-Target` 就回 `X-Amz-Target`）。
#[test]
fn contract_v2_emits_the_whole_recipe_verbatim() {
    let get = pending_get(&v2_mapping(Some(2), true));
    let r = get.recipe.as_ref().expect("v2 必须有配方");
    let src = r.source.as_ref().expect("source");
    assert_eq!(src.base_url, "https://ec2.example.com");
    assert_eq!(src.auth_mode.as_deref(), Some("aws-sigv4"));

    let op = r.operation.as_ref().expect("operation").as_object().unwrap();
    assert_eq!(op["method"], "POST");
    assert_eq!(op["path"], "/");
    assert_eq!(
        op["headers"]["X-Amz-Target"], "{target}",
        "header 名与模板必须原样透出（⛔ 产品代码不认识任何具体名字）"
    );
    assert_eq!(op["vars"]["target"], "DescribeCustomers");

    let req = r.request.as_ref().expect("request").as_object().unwrap();
    assert_eq!(req["template"]["Region"], "{region}");
    assert_eq!(req["required"][0], "region");
    assert_eq!(req["paging"]["offset"], "NextToken");

    assert_eq!(r.iterator.as_deref(), Some("$.Response.CustomerSet[*]"));
    assert_eq!(
        r.fields.as_ref().unwrap()["name"], "CustomerName",
        "fields 摊平成 `属性 → 响应字段`"
    );

    // 序列化后的键名与顺序：旧 4 键在前，配方段依次接上。
    let text = serde_json::to_string(r).unwrap();
    assert!(
        text.starts_with(r#"{"source":{"base_url":"https://ec2.example.com","auth_mode":"aws-sigv4"}"#),
        "{text}"
    );
    for key in ["\"operation\"", "\"request\"", "\"iterator\"", "\"fields\"", "\"auth\""] {
        assert!(text.contains(key), "缺 {key}：{text}");
    }
}

/// ③ 映射没给 `signed_headers` ⇒ `get.auth` 里**没有这个键**（⛔ 不按 `mode` 猜）。
#[test]
fn missing_auth_declarations_are_absent_not_inferred() {
    let get = pending_get(&v2_mapping(Some(2), false));
    let auth = get
        .recipe
        .as_ref()
        .and_then(|r| r.auth.as_ref())
        .expect("有 auth 声明（credential_env）");
    assert_eq!(auth.credential_env, vec!["AWS_ACCESS_KEY_ID".to_string()]);
    assert!(auth.signed_headers.is_none(), "⛔ 不许按 mode 猜签名头");
    assert!(auth.timestamp.is_none());
    assert!(auth.scope.is_none());
    let text = serde_json::to_string(auth).unwrap();
    assert!(
        !text.contains("signed_headers") && !text.contains("timestamp") && !text.contains("scope"),
        "{text}"
    );
}

/// ④ 配方超过 12 KB ⇒ 加载期报错**点名那个类**（⛔ 不静默截断、不悄悄瘦身）。
#[test]
fn an_oversized_recipe_is_a_named_load_error() {
    let mut doc: serde_json::Value = serde_json::from_str(&v2_mapping(Some(2), true)).unwrap();
    let fat: serde_json::Map<String, serde_json::Value> = (0..600)
        .map(|i| (format!("prop_{i:03}"), serde_json::json!({ "path": format!("Field{i:03}") })))
        .collect();
    doc["entities"][0]["fields"] = serde_json::Value::Object(fat);
    let err = AccessMapping::parse(&serde_json::to_string(&doc).unwrap())
        .expect_err("600 个字段的配方一定超 12 KB");
    assert!(err.contains("12288"), "要说清上限：{err}");
    assert!(err.contains("customers"), "要点名是哪个类：{err}");
    assert!(err.contains("fields"), "要指出多半是 fields 过肥：{err}");
}
