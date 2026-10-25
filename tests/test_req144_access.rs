//! REQ-144 · **access mapping 阶段 0** 的端到端判据（AC-001 ~ AC-008 + 一条**额外自证**）。
//!
//! # 这份文件治的是什么
//!
//! `src/access/` 里的单测各自盯一个函数；这里盯的是**契约 → 检查 → 取数 → 发布**
//! 这条链在**真实夹具**上跑通，而且**该红的地方真的红**。
//!
//! | 判据 | 对应 AC | 红的方式 |
//! | --- | --- | --- |
//! | [`ac001_contract_parses_and_missing_field_is_named`] | AC-001 | 删一个必填字段，报错必须点名 entity + 字段 |
//! | [`ac002_four_checks_have_teeth`] | AC-002 | 四条各有一次「造坏 ⇒ 必红」 |
//! | [`ac003_dry_run_never_touches_the_network`] | AC-003 | 端点写成 `.invalid` 的域名也能跑 |
//! | [`ac004_paging_and_local_filter`] | AC-004 | 250 行 / 每页 100 ⇒ 3 页；本地过滤要见 `filtered_locally` |
//! | [`ac005_scan_limit_is_refused_without_partial_result`] | AC-005 | 超预算 ⇒ `SCAN_LIMIT_EXCEEDED` |
//! | [`ac006_unreviewed_entity_is_refused`] | AC-006 | `reviewed_by: null` ⇒ `UNREVIEWED_MAPPING` |
//! | [`ac007_iri_template_matches_the_ontology`] | AC-007 | `@id` 与本体的 `ontok:instanceIri` 逐字同形 |
//! | [`ac008_meta_carries_the_five_fields`] | AC-008 | 五个字段一个都不能少 |
//! | [`extra_published_jsonld_is_queryable`] | **额外自证**（⛔ 不是 AC-009） | 发布物里 RML 核心 + `ontok:` 扩展都在，能按 pushdown 查 |
//!
//! ⚠️ **AC-009 是「不退化 / 零依赖」，不是「可查性」**（`acceptance.md` 原文）。
//! 那一条由**既有判据**覆盖，不在本文件里重复实现：
//!
//! - 四红线 / `tools.rs` / `guard()` / `property_filter.rs` 逐字节未变 ⇒ `tests/test_req107_red_lines.rs`；
//! - 新出口必须登记（I29/I30/I31）⇒ `tests/test_req107_source_enumeration.rs` + `test_req107_outlet_pipeline.rs`；
//! - 零新增依赖 ⇒ `git diff develop -- Cargo.toml` 为空（`Cargo.lock` 同理）。
//!
//! # 为什么这些判据**不连库**
//!
//! access 契约是**本地资产**：解析 / 校验 / 取数 / 发布全在文件上做（阶段 0 不发网络请求）。
//! 判据若需要库，CI 就跑不了 —— 而「映射写歪了」正是最该在 CI 里被挡住的事。
//!
//! ⚠️ 本文件**不加** `//! @guard-manifest: REQ-081`。

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use ontok::access::fetch::{self, FetchOptions, FilterValue};
use ontok::access::validate::validate;
use ontok::access::{parse, parse_ontology, Problem};
use serde_json::{json, Value};

/// 仓库根（运行期定位，⛔ 不用编译期常量 —— 与 REQ-047 同一条纪律）。
fn root() -> PathBuf {
    common::crate_root()
}

fn fixtures() -> PathBuf {
    root().join("fixtures")
}

/// 读示例契约（`fixtures/access-cvm.json`）。
fn mapping_text() -> String {
    std::fs::read_to_string(fixtures().join("access-cvm.json")).expect("示例契约必须在 fixtures/ 里")
}

/// 读示例本体（`fixtures/access-ontology.ttl`）。
fn ontology_text() -> String {
    std::fs::read_to_string(fixtures().join("access-ontology.ttl")).expect("示例本体必须在 fixtures/ 里")
}

fn setup() -> (ontok::access::Mapping, ontok::access::Ontology) {
    (
        parse(&mapping_text()).expect_or_panic("示例契约应当解析成功"),
        parse_ontology(&ontology_text()),
    )
}

/// 让 `Result<_, Vec<Problem>>` 也能 `.expect(..)`（`Vec<Problem>` 不是 `Debug` 友好的错误类型，
/// 但它是 `Debug` 的 —— 这里只是给出一句人话）。
trait ExpectOrPanic<T> {
    fn expect_or_panic(self, msg: &str) -> T;
}

impl<T> ExpectOrPanic<T> for Result<T, Vec<Problem>> {
    fn expect_or_panic(self, msg: &str) -> T {
        match self {
            Ok(v) => v,
            Err(ps) => panic!(
                "{msg}\n{}",
                ps.iter()
                    .map(|p| format!("  [{}] {} :: {}", p.code, p.location, p.message))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }
}

/// 改一处 JSON 文本里的片段（造变异用；改不到就 panic —— ⛔ 不许静默无效）。
fn mutate(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "变异锚点不存在：{from:?}");
    text.replacen(from, to, 1)
}

fn run_fetch(
    m: &ontok::access::Mapping,
    o: &ontok::access::Ontology,
    entity: &str,
    filter: &[(&str, FilterValue)],
    limit: usize,
) -> Result<fetch::JsonLdDoc, ontok::access::AccessError> {
    let opts = FetchOptions {
        filter: filter.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect::<BTreeMap<_, _>>(),
        limit,
    };
    fetch::dry_run(m, &o.iri, entity, &opts, &fixtures())
}

// ══════════════════════════════════════════════════════════════
// AC-001 · 契约能解析；缺必填 ⇒ 点名
// ══════════════════════════════════════════════════════════════

#[test]
fn ac001_contract_parses_and_missing_field_is_named() {
    // 成功那一份
    let m = parse(&mapping_text()).expect_or_panic("示例契约必须解析成功");
    assert_eq!(m.sources.len(), 2);
    assert_eq!(m.entities.len(), 2);
    assert_eq!(m.relations.len(), 1);
    assert_eq!(m.contract_version, 1);
    assert_eq!(m.mapping_version, 2);
    assert!(m.entity("cvm_instance").is_some(), "`--entity` 要能按 id 找到它");

    // 失败那一份：删掉 cvm_instance 的 iri_template
    let bad = mutate(
        &mapping_text(),
        "\"iri_template\": \"ontok:cvm_instance/{id}\",",
        "",
    );
    let errs = parse(&bad).expect_err("缺必填必须报错");
    let p = errs
        .iter()
        .find(|p| p.location.contains("iri_template"))
        .unwrap_or_else(|| panic!("必须点名 iri_template：{errs:?}"));
    // 点名到「哪条 entity」+「哪个字段」—— AC-001 的判据就在这两点上。
    assert!(p.location.contains("entities[0]"), "{}", p.location);
    assert!(p.location.contains("cvm_instance"), "{}", p.location);
    assert_eq!(p.code, "ACCESS_SCHEMA_INVALID");
}

// ══════════════════════════════════════════════════════════════
// AC-002 · 四条检查各有牙（每条都真的红一次）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac002_four_checks_have_teeth() {
    let (m, o) = setup();

    // 基线：干净的契约必须过（否则下面四条「红」可能是别的原因）。
    let ok = validate(&m, &o, &fixtures());
    assert!(ok.ok, "基线契约应当通过：{:?}", ok.errors);

    // ① R3：改 iri_template
    let bad = mutate(&mapping_text(), "ontok:cvm_instance/{id}", "ontok:云主机/{id}");
    let r = validate(&parse(&bad).unwrap(), &o, &fixtures());
    assert!(
        r.errors.iter().any(|p| p.code == "IRI_TEMPLATE_MISMATCH"),
        "改 template 必须红：{:?}",
        r.errors
    );

    // ② R4：同一字段写两处
    let bad = mutate(&mapping_text(), "\"local_only\": [\"status\", \"cpu\"]", "\"local_only\": [\"status\", \"zone\"]");
    let r = validate(&parse(&bad).unwrap(), &o, &fixtures());
    let p = r
        .errors
        .iter()
        .find(|p| p.code == "CAPABILITY_OVERLAP")
        .expect("同一字段写两处必须红");
    assert!(p.message.contains("zone"), "{}", p.message);

    // ③ R7：关系指向不存在的类
    let bad = mutate(&mapping_text(), "\"to\": \"cvm_instance\"", "\"to\": \"cvm_disk\"");
    let r = validate(&parse(&bad).unwrap(), &o, &fixtures());
    let p = r
        .errors
        .iter()
        .find(|p| p.code == "RELATION_ENDPOINT_UNKNOWN")
        .expect("端点不在 entities 里必须红");
    assert!(p.message.contains("cvm_disk"), "{}", p.message);

    // ④ 字段路径：写个 fixture 里不存在的路径
    let bad = mutate(&mapping_text(), "\"path\": \"Placement.Zone\"", "\"path\": \"Placement.ZoneName\"");
    let r = validate(&parse(&bad).unwrap(), &o, &fixtures());
    let p = r
        .errors
        .iter()
        .find(|p| p.code == "FIELD_PATH_MISSING")
        .expect("路径在 fixture 里不存在必须红");
    assert!(p.location.contains("fields.zone.path"), "{}", p.location);
    assert!(p.message.contains("250 行"), "要说清查了多少行：{}", p.message);
}

// ══════════════════════════════════════════════════════════════
// AC-003 · `--dry-run-fixture` 不发任何网络请求
// ══════════════════════════════════════════════════════════════

#[test]
fn ac003_dry_run_never_touches_the_network() {
    // 端点换成**不可解析**的域名：只要实现里有任何一次 HTTP，这里就会失败。
    let bad = mutate(&mapping_text(), "https://cvm.tencentcloudapi.com", "https://cvm.invalid");
    let m = parse(&bad).expect_or_panic("换域名不该影响解析");
    let o = parse_ontology(&ontology_text());
    let doc = run_fetch(&m, &o, "cvm_instance", &[], 3).expect("不联网也必须能跑通");
    assert_eq!(doc.graph.len(), 3);
    // 而且回执要明说来源是夹具（⛔ 不假装是现场数据）。
    assert_eq!(doc.meta.freshness.source, "fixture");
}

// ══════════════════════════════════════════════════════════════
// AC-004 · 分页 + 本地过滤（核心语义）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac004_paging_and_local_filter() {
    let (m, o) = setup();

    // 250 行、`max_limit = 100` ⇒ 3 页（100 / 100 / 50）。
    let doc = run_fetch(&m, &o, "cvm_instance", &[], 1000).expect("应当成功");
    assert_eq!(doc.meta.rows_scanned, 250);
    assert_eq!(doc.meta.pages, 3, "250 行 / 每页 100 ⇒ 3 页");
    assert_eq!(doc.meta.rows_returned, 250);
    assert!(doc.meta.filtered_locally.is_empty(), "没有过滤条件时不该标本地过滤");

    // `status` 在 `local_only` 里 ⇒ 必须出现在 `filtered_locally`，且结果**已按它筛过**。
    let stop = vec![("status", FilterValue::One("STOPPED".into()))];
    let doc = run_fetch(&m, &o, "cvm_instance", &stop, 200).expect("应当成功");
    assert_eq!(doc.meta.filtered_locally, vec!["status".to_string()]);
    assert_eq!(doc.meta.rows_scanned, 250, "本地过滤要扫完才能确定结果");
    assert_eq!(doc.meta.rows_matched, 83, "fixture 里 STOPPED 恰好 83 行（i%3==0）");
    assert_eq!(doc.meta.rows_returned, 83);
    assert!(!doc.meta.truncated);
    for node in &doc.graph {
        assert_eq!(node["ontok:cvm_instance__status"], json!("STOPPED"));
    }

    // `--limit` 截断：给多少返多少，且 `truncated` 必须为真。
    let doc = run_fetch(&m, &o, "cvm_instance", &stop, 50).expect("应当成功");
    assert_eq!(doc.meta.rows_returned, 50);
    assert!(doc.meta.truncated, "limit < 命中数 ⇒ truncated 必须为 true");

    // `zone` 在 `pushdown` 里 ⇒ 是**服务端**筛的，⛔ 不进 `filtered_locally`。
    let zone = vec![("zone", FilterValue::One("ap-guangzhou-3".into()))];
    let doc = run_fetch(&m, &o, "cvm_instance", &zone, 200).expect("应当成功");
    assert!(doc.meta.filtered_locally.is_empty(), "下推不计入本地过滤");
    assert_eq!(doc.meta.rows_scanned, 125, "下推后服务端只返回 125 行");
    assert_eq!(doc.meta.pages, 2, "125 行 / 每页 100 ⇒ 2 页");
}

// ══════════════════════════════════════════════════════════════
// AC-005 · 超 `max_scan_rows` ⇒ 拒绝（⛔ 不给部分结果）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac005_scan_limit_is_refused_without_partial_result() {
    let bad = mutate(&mapping_text(), "\"max_scan_rows\": 5000", "\"max_scan_rows\": 120");
    let m = parse(&bad).expect_or_panic("改预算不该影响解析");
    let o = parse_ontology(&ontology_text());
    // ⚠️ 不用 `.expect_err()`：`JsonLdDoc` 的 `Debug` 会把 250 个节点全打出来，
    // 红行会被淹掉 —— 变红时「它居然返回了 N 行」才是要看的。
    let err = match run_fetch(&m, &o, "cvm_instance", &[], 250) {
        Ok(doc) => panic!(
            "超预算居然返回了：扫 {} 行 / {} 页、返回 {} 行（预算 120）",
            doc.meta.rows_scanned, doc.meta.pages, doc.meta.rows_returned
        ),
        Err(e) => e,
    };
    assert_eq!(err.code, "SCAN_LIMIT_EXCEEDED");
    // 判据：**取页前**就停（第 1 页 100 行已取，第 2 页会让合计到 200 > 120）。
    assert!(err.message.contains("已扫 100 行"), "{}", err.message);
    assert!(err.message.contains("预算只有 120"), "{}", err.message);
}

// ══════════════════════════════════════════════════════════════
// AC-006 · 未签 entity 拒绝取数（R6）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac006_unreviewed_entity_is_refused() {
    let bad = mutate(
        &mapping_text(),
        "\"reviewed_by\": \"fde@ops360 (2026-10-25)\"",
        "\"reviewed_by\": null",
    );
    let m = parse(&bad).expect_or_panic("null 是合法状态");
    let o = parse_ontology(&ontology_text());

    let err = match run_fetch(&m, &o, "cvm_instance", &[], 5) {
        Ok(doc) => panic!(
            "未签的映射居然取到数了：扫 {} 行、返回 {} 行",
            doc.meta.rows_scanned, doc.meta.rows_returned
        ),
        Err(e) => e,
    };
    assert_eq!(err.code, "UNREVIEWED_MAPPING");

    // 但**另一条签过的** entity 照常能取 —— 拒绝是逐条 entity 的，不是整份契约作废。
    let ok = run_fetch(&m, &o, "alarm", &[], 5).expect("alarm 仍已签，应当能取");
    assert_eq!(ok.graph.len(), 3);

    // 签上之后就能取（这是 R6 的另一半：不是「永远不行」）。
    let signed = mutate(&bad, "\"reviewed_by\": null", "\"reviewed_by\": \"张三@2026-10-26\"");
    let m = parse(&signed).unwrap();
    let doc = run_fetch(&m, &o, "cvm_instance", &[], 2).expect("签上之后必须能取");
    assert_eq!(doc.graph.len(), 2);
}

// ══════════════════════════════════════════════════════════════
// AC-007 · `iri_template` 与本体 `ontok:instanceIri` 一致（R3 端到端）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac007_iri_template_matches_the_ontology() {
    let (m, o) = setup();
    let report = validate(&m, &o, &fixtures());
    assert!(report.ok, "{:?}", report.errors);

    // 两边的字符串**逐字**摆出来（这就是判据本身）。
    let cvm = report
        .entities
        .iter()
        .find(|e| e.id == "cvm_instance")
        .expect("报告里必须有 cvm_instance");
    assert_eq!(
        cvm.ttl_instance_iri.as_deref(),
        Some("https://ontok.local/ops360#{class}/{id}")
    );
    assert_eq!(
        cvm.ttl_expanded.as_deref(),
        Some("https://ontok.local/ops360#cvm_instance/{id}")
    );
    assert_eq!(
        cvm.expanded_iri_template,
        "https://ontok.local/ops360#cvm_instance/{id}",
        "access.json 展开后的模板必须与本体的模板同形"
    );

    // 端到端：真取一条，`@id` 必须**由同一条模板**拼出来。
    let doc = run_fetch(&m, &o, "cvm_instance", &[], 1).expect("应当成功");
    let id = doc.graph[0]["@id"].as_str().unwrap();
    assert_eq!(id, "https://ontok.local/ops360#cvm_instance/ins-0001");
    assert_eq!(
        id.replace("ins-0001", "{id}"),
        cvm.ttl_expanded.clone().unwrap(),
        "把键换回占位符之后，`@id` 与本体的模板逐字相同"
    );
    assert_eq!(doc.graph[0]["@type"], json!("ontok:cvm_instance"));
}

// ══════════════════════════════════════════════════════════════
// AC-008 · `meta` 五项齐全（R2）
// ══════════════════════════════════════════════════════════════

#[test]
fn ac008_meta_carries_the_five_fields() {
    let (m, o) = setup();
    let stop = vec![("status", FilterValue::One("RUNNING".into()))];
    let doc = run_fetch(&m, &o, "cvm_instance", &stop, 10).expect("应当成功");
    let text = doc.to_text();
    let v: Value = serde_json::from_str(&text).expect("必须是合法 JSON");

    // 与 `query serve` 同形状：@context / @graph / summary / meta
    for key in ["@context", "@graph", "summary", "meta"] {
        assert!(v.get(key).is_some(), "缺顶层成员 {key}");
    }
    assert!(v["@graph"].is_array(), "@graph 必须是数组（空也是 []）");
    assert_eq!(v["@context"]["ontok"], "https://ontok.local/ops360#");

    for key in [
        "rows_scanned",
        "filtered_locally",
        "freshness",
        "capability_warnings",
        "truncated",
    ] {
        assert!(v["meta"].get(key).is_some(), "meta 缺 {key}");
    }
    // `freshness` 要能回答「哪个时点」；`filtered_locally` 要真的列出被本地筛的字段。
    assert_eq!(v["meta"]["freshness"]["mode"], "realtime");
    assert_eq!(v["meta"]["freshness"]["source"], "fixture");
    assert!(v["meta"]["freshness"]["fetched_at"].is_string());
    assert_eq!(v["meta"]["filtered_locally"], json!(["status"]));
    // 能力提醒要说明「为什么本地筛」（Agent 不能以为服务端筛过了）。
    let warns = v["meta"]["capability_warnings"].as_array().unwrap();
    assert!(
        warns.iter().any(|w| w.as_str().unwrap_or("").contains("status")),
        "{warns:?}"
    );
}

// ══════════════════════════════════════════════════════════════
// 额外自证 · `publish` 的 JSON-LD 可被 SPARQL 查
//
// ⚠️ ⛔ **不是 AC-009**：`acceptance.md` 的 AC-009 是「不退化 / 零依赖」。
// 这一条是任务书「输出的 JSON-LD 能被 SPARQL 查」那半句的自证，
// 真 SPARQL 引擎跑过的查询在 `fixtures/access-queries.rq`。
// ══════════════════════════════════════════════════════════════

#[test]
fn extra_published_jsonld_is_queryable() {
    let (m, o) = setup();
    let doc = ontok::access::publish::to_jsonld(&m, &o.iri);
    let text = serde_json::to_string(&doc).unwrap();

    // RML 的四个核心谓词 + `ontok:` 的六个扩展，一个都不能少（任务书逐条点过名）。
    for needle in [
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

    // 只允许 @context / @graph（放别的非 RDF 键进去会让处理器困惑）。
    let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["@context", "@graph"]);

    // 查询形状：TriplesMap → subjectMap → rr:class；`ontok:pushdown` 按值绑定。
    // （真 SPARQL 引擎跑过一遍的查询见 `fixtures/access-queries.rq`；
    //   这里不引 RDF 依赖，只断言**被查的那几条三元组**确实存在。）
    let graph = doc["@graph"].as_array().unwrap();
    let tm: Vec<&Value> = graph
        .iter()
        .filter(|n| n["@type"] == json!("rml:TriplesMap"))
        .collect();
    assert_eq!(tm.len(), 2, "每条 entity 一个 TriplesMap");
    let hit: Vec<&Value> = tm
        .iter()
        .copied()
        .filter(|n| {
            n["rr:subjectMap"]["rr:class"]["@id"]
                == json!("https://ontok.local/ops360#cvm_instance")
                && n["ontok:pushdown"]
                    .as_array()
                    .map(|a| a.contains(&json!("zone")))
                    .unwrap_or(false)
        })
        .collect();
    assert_eq!(hit.len(), 1, "「哪些类支持按 zone 过滤」应当恰好命中 cvm_instance");
    assert_eq!(hit[0]["ontok:maxScanRows"], json!(5000));
    assert_eq!(hit[0]["ontok:apiAction"], json!("DescribeInstances"));
    // 发布物里 ⛔ 不许出现凭据值（只有环境变量名）。
    assert!(text.contains("TENCENT_SECRET_ID"));
    assert!(!text.contains("TENCENT_SECRET_KEY_VALUE"));
}
