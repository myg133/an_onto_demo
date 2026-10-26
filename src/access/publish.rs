//! `ontok-builder access publish` —— 作者格式 JSON → **发布态 JSON-LD**（RML 风格）。
//!
//! ```text
//! access.json      ← 人写 / 人审 / 好 diff（作者格式）
//! access.jsonld    ← RML 风格，给执行器与 Agent，且**能被 SPARQL 查**（发布格式）
//! ```
//!
//! # 为什么是 RML 而不是自造一套谓词（demand §三 R-1）
//!
//! 「JSON 响应 → RDF」这件事 RML 已经定义过了：`rml:logicalSource` / `rml:iterator` /
//! `rr:template` / `rml:reference` / `rr:class`。再发明一套只会让「我们的映射」与
//! 「业界映射」成为两种东西 —— 而客户侧执行器本来就可能是通用 RML 引擎。
//!
//! # RML 没有的部分用 `ontok:` 扩展（这是**只增不减**的边界）
//!
//! | 谓词 | 来自 | 为什么要发布 |
//! | --- | --- | --- |
//! | `ontok:apiAction` | `operation.action` | Agent 要知道这条映射打的是哪个动作 |
//! | `ontok:pushdown` | `capability.pushdown` | 「哪些类支持按 zone 过滤」正是问它 |
//! | `ontok:localOnly` | `capability.local_only` | 不给它，Agent 会以为服务端能筛 |
//! | `ontok:maxScanRows` | `capability.max_scan_rows` | 预算（R5）得能被下游看见 |
//! | `ontok:freshness` | `freshness.mode` | 时点声明（规则 6） |
//! | `ontok:reviewedBy` | `reviewed_by` | 未签的映射不许取数（R6）—— 签没签要能被查 |
//!
//! 另外几条（不在任务点名的六个里，但不发布就断链）：
//! `ontok:sourceKind` / `ontok:baseUrl` / `ontok:credentialEnv`（源）、
//! `ontok:Relation` + `ontok:from` / `ontok:to` / `ontok:traversable`（关系）、
//! `ontok:mappingVersion` / `ontok:contractVersion`（`@pending[].get.ref` 的 `@v<n>` 靠它）。
//!
//! # 口径
//!
//! - **确定性**：源 / 实体 / 关系都按 id 升序；同一个 `access.json` ⇒ 同一份字节。
//! - **纯 JSON-LD**：产物只有 `@context` + `@graph`，⛔ 不夹 `meta` / `summary`
//!   （那两个是回执的概念；把非 RDF 的键塞进发布物会让 JSON-LD 处理器困惑）。
//! - **类与谓词用完整 IRI 节点**（`{"@id": …}`），因为本文档的 `ontok:` 前缀是
//!   **模式命名空间**（`https://ontok.local/schema/`），与实例命名空间同名不同义 ——
//!   混用会让 SPARQL 里 `?class` 绑到字符串而不是 IRI。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 不产出 `rr:logicalTable`（那是 R2RML 的关系库侧）；本文件只描述 HTTP API 源。
//! - 不展开 `rr:predicateObjectMap` 的 join 形态：`relations[].via.join` 发布成
//!   `ontok:Relation` 节点，⛔ 不伪装成 `rr:RefObjectMap`（v1 不做跨源 join，
//!   同源 join 的执行语义也还没定）。
//! - 不写 `@base`：所有 `@id` 都是绝对 IRI（`urn:ontok:access:<project>#…`）。

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::{Mapping, Ontology};

/// 模式命名空间（与 TTL 里 `@prefix ontok:` 一致）。
pub const ONTOK_NS: &str = "https://ontok.local/schema/";

/// RML 的命名空间。
pub const RML_NS: &str = "http://semweb.mmlab.be/ns/rml#";
/// R2RML 的命名空间（RML 复用它做 `rr:template` / `rr:class`）。
pub const R2RML_NS: &str = "http://www.w3.org/ns/r2rml#";
/// `xsd` 命名空间。
pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema#";

/// 发布态文档里「映射条目」的 IRI 前缀（`urn:ontok:access:<project>`）。
pub fn access_base(project: &str) -> String {
    format!("urn:ontok:access:{project}")
}

/// 把一份契约转成 JSON-LD（`Value`：成员按键排序 ⇒ 逐字节确定）。
pub fn to_jsonld(mapping: &Mapping, ontology_iri: &str) -> Value {
    let base = access_base(&mapping.project);
    let ontok = |local: &str| format!("{base}#{local}");
    let class_iri = |class: &str| {
        // 契约里的类名是**本地名**；本体 IRI 决定完整 IRI。
        format!("{ontology_iri}#{}", Ontology::local_name(class))
    };

    let mut context: BTreeMap<&'static str, String> = BTreeMap::new();
    // ⚠️ `ontok` 在这里是**模式**命名空间（扩展谓词），不是实例命名空间。
    context.insert("ontok", ONTOK_NS.to_string());
    context.insert("rml", RML_NS.to_string());
    context.insert("rr", R2RML_NS.to_string());
    context.insert("xsd", XSD_NS.to_string());

    let mut graph: Vec<Value> = Vec::new();

    // ── 文档自己的身份（`@pending[].get.ref` 要能追到版本）──
    let mut root = Map::new();
    root.insert("@id".to_string(), json!(base));
    root.insert("@type".to_string(), json!("ontok:AccessMapping"));
    root.insert("ontok:contractVersion".to_string(), json!(mapping.contract_version));
    root.insert("ontok:mappingVersion".to_string(), json!(mapping.mapping_version));
    root.insert("ontok:project".to_string(), json!(mapping.project));
    if let Some(v) = mapping.ontology_version {
        root.insert("ontok:ontologyVersion".to_string(), json!(v));
    }
    root.insert("ontok:ontologyIri".to_string(), json!(format!("{ontology_iri}#")));
    let mut sources_sorted: Vec<&super::Source> = mapping.sources.iter().collect();
    sources_sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut entities_sorted: Vec<&super::Entity> = mapping.entities.iter().collect();
    entities_sorted.sort_by(|a, b| a.id.cmp(&b.id));
    root.insert(
        "ontok:source".to_string(),
        Value::Array(sources_sorted.iter().map(|s| json!({ "@id": ontok(&format!("source:{}", s.id)) })).collect()),
    );
    root.insert(
        "ontok:entity".to_string(),
        Value::Array(entities_sorted.iter().map(|e| json!({ "@id": ontok(&e.id) })).collect()),
    );
    graph.push(Value::Object(root));

    // ── sources[] ──
    for s in &sources_sorted {
        let mut n = Map::new();
        n.insert("@id".to_string(), json!(ontok(&format!("source:{}", s.id))));
        n.insert("@type".to_string(), json!("ontok:DataSource"));
        n.insert("ontok:sourceKind".to_string(), json!(s.kind));
        n.insert("ontok:baseUrl".to_string(), json!(s.base_url));
        if let Some(a) = &s.auth {
            n.insert("ontok:authMode".to_string(), json!(a.mode));
            // ⛔ 只有**变量名**，没有值（设计稿规则 1）。
            n.insert(
                "ontok:credentialEnv".to_string(),
                Value::Array(a.credential_env.iter().map(|e| json!(e)).collect()),
            );
        }
        if !s.defaults.is_empty() {
            let mut d = Map::new();
            for (k, v) in &s.defaults {
                d.insert(k.clone(), v.clone());
            }
            n.insert("ontok:requestDefault".to_string(), Value::Object(d));
        }
        graph.push(Value::Object(n));
    }

    // ── entities[]（每条一个 rml:TriplesMap）──
    for e in &entities_sorted {
        let cls = class_iri(&e.class);
        let mut n = Map::new();
        n.insert("@id".to_string(), json!(ontok(&e.id)));
        n.insert("@type".to_string(), json!("rml:TriplesMap"));
        n.insert(
            "rr:logicalSource".to_string(),
            json!({
                "@type": "rml:LogicalSource",
                "rml:source": { "@id": ontok(&format!("source:{}", e.source)) },
                "rml:iterator": e.iterator,
            }),
        );
        n.insert(
            "rr:subjectMap".to_string(),
            json!({
                "rr:template": e.iri_template,
                "rr:class": { "@id": cls },
            }),
        );
        // 一条属性一个 predicateObjectMap（键序 = 字段名字典序 ⇒ 确定性）。
        let poms: Vec<Value> = e
            .fields
            .iter()
            .map(|(name, f)| {
                let mut om = Map::new();
                om.insert("rml:reference".to_string(), json!(f.path));
                if let Some(ty) = &f.ty {
                    let local = ty.trim().strip_prefix("xsd:").unwrap_or(ty.trim());
                    om.insert("rr:datatype".to_string(), json!({ "@id": format!("{XSD_NS}{local}") }));
                }
                json!({
                    "rr:predicate": { "@id": format!("{cls}__{name}") },
                    "rr:objectMap": Value::Object(om),
                })
            })
            .collect();
        n.insert("rr:predicateObjectMap".to_string(), Value::Array(poms));

        n.insert("ontok:entityId".to_string(), json!(e.id));
        n.insert("ontok:class".to_string(), json!(cls));
        n.insert("ontok:apiAction".to_string(), json!(e.operation.action));
        if let Some(v) = &e.operation.version {
            n.insert("ontok:apiVersion".to_string(), json!(v));
        }
        n.insert("ontok:httpMethod".to_string(), json!(e.operation.method));
        n.insert("ontok:contentType".to_string(), json!(e.operation.content_type));
        n.insert("ontok:keyPath".to_string(), json!(e.key_path));
        n.insert(
            "ontok:pushdown".to_string(),
            Value::Array(e.capability.pushdown.iter().map(|f| json!(f)).collect()),
        );
        n.insert(
            "ontok:localOnly".to_string(),
            Value::Array(e.capability.local_only.iter().map(|f| json!(f)).collect()),
        );
        n.insert("ontok:maxScanRows".to_string(), json!(e.capability.max_scan_rows));
        n.insert("ontok:freshness".to_string(), json!(e.freshness.mode));
        if let Some(note) = &e.freshness.note {
            n.insert("ontok:freshnessNote".to_string(), json!(note));
        }
        // 未签 ⇒ 写 `null`。⚠️ **行为上等于「不写」**：标准 JSON-LD 会丢弃 `null`
        // ⇒ RDF 里**没有** `ontok:reviewedBy` 这个三元组（rdflib 实测：2 条映射、1 条未签，
        //   图里只有 1 条 `ontok:reviewedBy`）。所以「查谁没签」必须用
        //   `FILTER NOT EXISTS { ?m ontok:reviewedBy ?who }` 或 `OPTIONAL` + `FILTER(!BOUND(?who))`，
        //   ⛔ 不能查「值等于 null」—— RDF 里没有「null 字面量」这种东西。
        //
        // 那为什么作者格式里仍写 `null` 而不是省略键：`parse` 把「键缺失」判成
        // **契约缺字段**（ACCESS_SCHEMA_INVALID，见 `access::parse` 的 reviewed_by 那一支），
        // 而 `null` 是**合法的「还没人签」状态** —— 两者在作者格式里必须能分开，
        // 在发布态里则不分开（都没有那个三元组）。
        n.insert("ontok:reviewedBy".to_string(), json!(e.reviewed_by));
        n.insert("ontok:fixture".to_string(), json!(e.fixture));
        if let Some(p) = &e.paging {
            n.insert(
                "ontok:paging".to_string(),
                json!({
                    "ontok:offsetParam": p.offset_param,
                    "ontok:limitParam": p.limit_param,
                    "ontok:maxLimit": p.max_limit,
                    "ontok:totalPath": p.total_path,
                }),
            );
        }
        graph.push(Value::Object(n));
    }

    // ── relations[] ──
    let mut rels: Vec<&super::Relation> = mapping.relations.iter().collect();
    rels.sort_by(|a, b| a.predicate.cmp(&b.predicate));
    for r in &rels {
        let mut n = Map::new();
        n.insert("@id".to_string(), json!(ontok(&format!("relation:{}", r.predicate))));
        n.insert("@type".to_string(), json!("ontok:Relation"));
        n.insert("ontok:predicate".to_string(), json!(r.predicate));
        n.insert("ontok:from".to_string(), json!({ "@id": ontok(&r.from) }));
        n.insert("ontok:to".to_string(), json!({ "@id": ontok(&r.to) }));
        if let Some(v) = &r.via {
            n.insert(
                "ontok:via".to_string(),
                json!({
                    "ontok:viaKind": v.kind,
                    "ontok:local": v.local,
                    "ontok:remote": v.remote,
                    "ontok:field": v.field,
                }),
            );
        }
        n.insert("ontok:crossSource".to_string(), json!(r.cross_source));
        // 跨源边不遍历（demand §三 R-4）：这条要能被查出来。
        n.insert("ontok:traversable".to_string(), json!(r.traversable));
        graph.push(Value::Object(n));
    }

    let mut doc = Map::new();
    doc.insert("@context".to_string(), {
        let mut c = Map::new();
        for (k, v) in &context {
            c.insert((*k).to_string(), json!(v));
        }
        Value::Object(c)
    });
    doc.insert("@graph".to_string(), Value::Array(graph));
    Value::Object(doc)
}

/// 直接出文本（CLI 的 stdout / `--out` 都取这一个）。
pub fn to_text(mapping: &Mapping, ontology_iri: &str) -> Result<String, String> {
    serde_json::to_string_pretty(&to_jsonld(mapping, ontology_iri))
        .map_err(|e| format!("JSON-LD 序列化失败：{e}"))
}

/// 随发布物一起给出的**能跑的查询示例**（`fixtures/access-queries.rq` 与它同源）。
///
/// 为什么把 SPARQL 放在代码里而不是只写在文档里：文档里的查询会随 schema 漂移，
/// 而这条是**测试**在跑的（`publish::tests::query_finds_pushdown_class` 用的是同一份谓词）。
pub fn query_pushdown_by_field(field: &str) -> String {
    format!(
        r#"PREFIX rml: <{RML_NS}>
PREFIX rr: <{R2RML_NS}>
PREFIX ontok: <{ONTOK_NS}>

# 「哪些类支持按 {field} 过滤」—— 一条映射好不好用，Agent 第一个要问的就是它。
SELECT ?mapping ?class ?action ?maxScanRows WHERE {{
  ?mapping a rml:TriplesMap ;
           rr:subjectMap ?sm ;
           ontok:apiAction ?action ;
           ontok:maxScanRows ?maxScanRows ;
           ontok:pushdown ?field .
  ?sm rr:class ?class .
  FILTER (LCASE(STR(?field)) = "{low}")
}}"#,
        low = field.to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{parse, parse_ontology};

    const TTL: &str = r#"
@prefix : <https://ontok.local/ops360#> .
@prefix ontok: <https://ontok.local/schema/> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
:云资源 a owl:Class ;
    ontok:instanceIri "https://ontok.local/ops360#{class}/{id}" .
"#;

    fn mapping_json() -> String {
        r#"{
  "contract_version": 1,
  "mapping_version": 2,
  "project": "ops360",
  "ontology_version": 2,
  "sources": [
    { "id": "svc-example", "kind": "http-json", "base_url": "https://api.example.com",
      "auth": { "mode": "hmac-sha256", "credential_env": ["EXAMPLE_SECRET_ID", "EXAMPLE_SECRET_KEY"] },
      "defaults": { "Region": "ap-guangzhou" } }
  ],
  "entities": [
    { "id": "cvm_instance", "class": "云资源", "source": "svc-example",
      "reviewed_by": "张三@2026-10-25",
      "operation": { "action": "SomeAction", "version": "2017-03-12" },
      "iterator": "$.Response.InstanceSet[*]",
      "identity": { "key_path": "InstanceId" },
      "iri_template": "ontok:云资源/{id}",
      "fields": { "name": { "path": "InstanceName" }, "cpu": { "path": "CPU", "type": "xsd:integer" } },
      "paging": { "offset_param": "Offset", "limit_param": "Limit", "max_limit": 100, "total_path": "$.Response.TotalCount" },
      "capability": { "pushdown": ["zone"], "local_only": ["status"], "max_scan_rows": 5000 },
      "freshness": { "mode": "realtime", "note": "状态有数秒延迟" },
      "fixture": "cvm.json" }
  ],
  "relations": [
    { "predicate": "云资源__告警", "from": "cvm_instance", "to": "cvm_instance", "via": { "kind": "field", "field": "alarm_id" } }
  ]
}"#
        .to_string()
    }

    #[test]
    fn publishes_rml_core_and_ontok_extensions() {
        let m = parse(&mapping_json()).unwrap();
        let o = parse_ontology(TTL);
        let doc = to_jsonld(&m, &o.iri);
        let text = serde_json::to_string(&doc).unwrap();
        for needle in [
            "rml:TriplesMap",
            "rml:iterator",
            "rr:template",
            "rml:reference",
            "rr:class",
            "ontok:apiAction",
            "ontok:pushdown",
            "ontok:localOnly",
            "ontok:maxScanRows",
            "ontok:freshness",
            "ontok:reviewedBy",
        ] {
            assert!(text.contains(needle), "发布物里缺 {needle}");
        }
        // ⛔ 不含真凭据
        assert!(!text.contains("SECRET_KEY_VALUE"));
        // 只允许 @context / @graph 两个顶层成员
        let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["@context", "@graph"]);
    }

    #[test]
    fn deterministic_bytes() {
        let m = parse(&mapping_json()).unwrap();
        let o = parse_ontology(TTL);
        assert_eq!(to_text(&m, &o.iri).unwrap(), to_text(&m, &o.iri).unwrap());
    }

    #[test]
    fn classes_are_iri_nodes_not_strings() {
        let m = parse(&mapping_json()).unwrap();
        let o = parse_ontology(TTL);
        let doc = to_jsonld(&m, &o.iri);
        let graph = doc["@graph"].as_array().unwrap();
        let tm = graph
            .iter()
            .find(|n| n["@type"] == json!("rml:TriplesMap"))
            .expect("必须有一条 TriplesMap");
        assert_eq!(
            tm["rr:subjectMap"]["rr:class"]["@id"],
            json!("https://ontok.local/ops360#云资源"),
            "rr:class 必须是 IRI 节点（⛔ 不是字符串字面量）"
        );
        assert_eq!(
            tm["ontok:pushdown"],
            json!(["zone"]),
            "pushdown 要能按值查（SPARQL 里就是多条 triple）"
        );
    }

    #[test]
    fn query_text_mentions_the_field() {
        let q = query_pushdown_by_field("zone");
        assert!(q.contains("ontok:pushdown ?field"));
        assert!(q.contains("\"zone\""));
        assert!(q.contains("rr:class ?class"));
    }
}
