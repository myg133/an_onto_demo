//! REQ-115 · **概念分组与图上的归并** —— 不依赖数据库的判据。
//!
//! 覆盖：
//!
//! | AC | 判据 |
//! | --- | --- |
//! | AC-001 | `/api/graph` 带 `groups`，节点 `group` 等于某组标识或 `null` |
//! | AC-002 | 折叠/展开的实现位置存在；折叠态节点数 = 组数 + 未分组数（Node 探针另测） |
//! | AC-003 | 无组 ⇒ `groups: []` + 每个节点 `group: null`，**没有默认组** |
//! | AC-006 | 页面只消费 `groups`，源码里没有分组算法 |
//! | AC-009 | 折叠/展开的函数体里没有网络调用（纯前端状态） |
//!
//! ⚠️ **AC-001 的 `key` 在线上叫 `id`**：`key` 在发布闸的词表里（`MODELING_TERMS`），
//! 写成线上键名会让每一次带分组的 `/api/graph` 被闸拒发。本文件里的
//! `why_id_not_key_*` 把这条裁定钉住 —— 它证明的不是「我们喜欢 id」，
//! 而是「写成 key 就会被拒」。
//!
//! ⚠️ 真实库的往返（suggest → import → 签 → apply → 图上出现）在
//! `.feature/verification-report.md` 里贴输出；它需要数据库，不属于本文件。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use ontok::domain::concept::ConceptGroup;
use ontok::domain::graph::{self, DeclaredGroup, GraphProjection};
use ontok::domain::snapshot::{EntityType, Snapshot};
use ontok::domain::view::{CustomerView, EntityView, PropertyView, RelationView};
use ontok::server::http::{build_router, state_from_view, state_from_view_with_groups, AppState};
use serde_json::{json, Value};
use tower::ServiceExt;

const TOKEN: &str = "correct-horse-battery";
const PROJECT: &str = "样例项目";
const SCOPE: &str = "/api/graph?project=%E6%A0%B7%E4%BE%8B%E9%A1%B9%E7%9B%AE&version=2";

// ──────────────────────────────────────────────────────────
// 夹具：四个对象（甲→乙 一条关系）+ 两个概念组
// ──────────────────────────────────────────────────────────

fn obj(name: &str, parent: Option<&str>) -> EntityView {
    EntityView {
        name: name.to_string(),
        note: None,
        belongs_to: parent.map(str::to_string),
        aliases: vec![],
        properties: vec![PropertyView {
            name: "名称".to_string(),
            note: None,
            required: true,
            sample: None,
        }],
    }
}

fn view() -> CustomerView {
    CustomerView {
        objects: vec![
            obj("甲", None),
            obj("乙", Some("甲")),
            obj("丙", None),
            obj("丁", None),
        ],
        relations: vec![RelationView {
            from: "甲".to_string(),
            to: "乙".to_string(),
            how_many: "一对多".to_string(),
            meaning: "拥有".to_string(),
        }],
    }
}

/// 权威层的声明（成员 = **展示名**，这是 `graph::declared_groups` 换算后的形状）。
///
/// `scope_hint` 是**客户可见的组说明**（C-3，与业务总览的 `summary` 同源）。
fn declared() -> Vec<DeclaredGroup> {
    vec![
        DeclaredGroup {
            key: "g-甲乙".to_string(),
            display_name: "甲乙一体".to_string(),
            members: vec!["甲".to_string(), "乙".to_string()],
            scope_hint: Some("样例范围".to_string()),
        },
        DeclaredGroup {
            key: "g-丙".to_string(),
            display_name: "丙单独".to_string(),
            members: vec!["丙".to_string()],
            scope_hint: None,
        },
    ]
}

fn grouped_state() -> AppState {
    state_from_view_with_groups(&view(), &declared(), PROJECT, 2, TOKEN, 0)
}

/// 打一发带口令的 GET，返回 `(状态, JSON, 原始正文)`。
async fn get(state: AppState, uri: &str, token: Option<&str>) -> (StatusCode, Value, String) {
    let mut b = Request::builder().method("GET").uri(uri);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let r = build_router(state)
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .expect("oneshot 的错误类型是 Infallible");
    let status = r.status();
    let body = axum::body::to_bytes(r.into_body(), 1 << 20)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body).to_string();
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, value, text)
}

fn node_group(v: &Value, id: &str) -> Option<String> {
    v["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|n| n["id"] == id)
        .and_then(|n| n["group"].as_str().map(str::to_string))
}

// ── AC-001 · 图接口返回权威分组 ──────────────────────────

#[tokio::test]
async fn groups_and_node_membership_are_on_the_wire() {
    let (status, v, body) = get(grouped_state(), SCOPE, Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK, "带干净分组的数据必须 200：{body}");

    let groups = v["groups"].as_array().expect("groups 必须是数组");
    assert_eq!(groups.len(), 2, "两个声明组必须都出现：{groups:?}");
    assert_eq!(groups[0]["id"], "g-甲乙");
    assert_eq!(groups[0]["display_name"], "甲乙一体");
    assert_eq!(groups[0]["members"], json!(["甲", "乙"]));
    assert_eq!(groups[1]["id"], "g-丙");

    // ⭐ C-3：`scope_hint` 是客户可见的组说明（与业务总览 summary 同源），图上带它；
    //    它之所以安全，是因为导入时已过客户闸（C-1）。
    assert_eq!(groups[0]["scope_hint"], "样例范围");
    assert!(
        groups[1].get("scope_hint").is_none(),
        "没写说明就不写这一栏（不凭空造）"
    );

    // 每个节点**都有** group 字段；值的集合 ⊆ 组标识 ∪ {null}
    let ids: Vec<&str> = groups.iter().filter_map(|g| g["id"].as_str()).collect();
    for n in v["nodes"].as_array().expect("nodes") {
        let g = n.get("group").expect("AC-001：每个节点都必须带 group（无组时是 null）");
        if let Some(id) = g.as_str() {
            assert!(ids.contains(&id), "node.group={id:?} 不是任何组的标识");
        } else {
            assert!(g.is_null(), "group 只能是组标识或 null，收到 {g:?}");
        }
    }
    assert_eq!(node_group(&v, "甲").as_deref(), Some("g-甲乙"));
    assert_eq!(node_group(&v, "乙").as_deref(), Some("g-甲乙"));
    assert_eq!(node_group(&v, "丙").as_deref(), Some("g-丙"));
    assert_eq!(node_group(&v, "丁"), None, "丁没有组 ⇒ null（不是被塞进某个默认组）");

    // ⚠️ 线上字段叫 id 不叫 key：body 里出现字面键 "key" 就说明有人改回去了
    assert!(
        !body.contains("\"key\""),
        "带分组的响应里出现了线上键 \"key\" —— 它会被发布闸拒发（见 why_id_not_key_*）"
    );
}

#[tokio::test]
async fn node_group_always_equals_one_of_the_group_ids() {
    let (_, v, _) = get(grouped_state(), SCOPE, Some(TOKEN)).await;
    let groups = v["groups"].as_array().expect("groups");
    for n in v["nodes"].as_array().expect("nodes") {
        match &n["group"] {
            Value::Null => {}
            Value::String(id) => assert!(
                groups.iter().any(|g| g["id"] == *id),
                "节点 {} 的 group={id} 在 groups 里找不到（AC-001 要求必须对得上）",
                n["id"]
            ),
            other => panic!("group 只能是字符串或 null，收到 {other:?}"),
        }
    }
}

// ── AC-003 · 无组时如实显示 ─────────────────────────────

#[tokio::test]
async fn no_groups_is_an_empty_array_plus_null_membership() {
    let state = state_from_view_with_groups(&view(), &[], PROJECT, 2, TOKEN, 0);
    let (status, v, _) = get(state, SCOPE, Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["groups"], json!([]), "「没有分组」必须写成 []，不是缺字段、更不是默认组");
    for n in v["nodes"].as_array().expect("nodes") {
        assert!(
            n.get("group").map(Value::is_null).unwrap_or(false),
            "无组时每个节点都必须显式写 group: null（AC-001），收到 {:?}",
            n.get("group")
        );
    }
}

// ── AC-008 的兼容半边：旧投影的字节形状不许漂 ────────────

#[tokio::test]
async fn legacy_projection_keeps_the_frozen_h2_shape() {
    // `state_from_view` 是 REQ-107 的 golden 形状：groups=缺席、节点无 group 字段。
    // 它存在**只为**让 H2_graph.json 逐字节不变；生产走 `state_from_view_with_groups`。
    let state = state_from_view(&view(), PROJECT, 2, TOKEN, 0);
    let (status, v, _) = get(state, SCOPE, Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(v.get("groups").is_none(), "旧形状多出 groups 字段 ⇒ H2 golden 会红");
    for n in v["nodes"].as_array().expect("nodes") {
        assert!(n.get("group").is_none(), "旧形状多出 group 字段 ⇒ H2 golden 会红");
    }
    // 顶层键集合必须恰好是 edges / nodes（serde_json::Value 按键排序）
    let top: Vec<&String> = v.as_object().expect("对象").keys().collect();
    assert_eq!(top, vec!["edges", "nodes"], "顶层键集合变了：{top:?}");
}

// ── AC-001 的裁定：为什么线上叫 id 而不是 key ────────────

#[test]
fn why_id_not_key_the_guard_rejects_a_literal_key_field() {
    let hits = ontok::domain::view::scan_modeling_terms(r#"{"groups":[{"key":"g1"}]}"#);
    assert!(
        !hits.is_empty(),
        "发布闸居然放过了字面键 \"key\" —— 那说明词表/分词被改了；\
         本需求正是因为它才把线上字段定成 id"
    );
    // 反过来：线上真实形状必须过闸（否则每一次带分组的响应都会 500）
    let ok = r#"{"groups":[{"id":"g1","display_name":"甲乙一体","members":["甲"]}]}"#;
    assert!(
        ontok::domain::view::scan_modeling_terms(ok).is_empty(),
        "线上分组形状本身过不了闸：{:?}",
        ontok::domain::view::scan_modeling_terms(ok)
    );
}

// ── 声明 → 图的换算（两级：实体 key → 展示名 → 节点标识）──

#[test]
fn declared_groups_maps_entity_keys_to_display_names() {
    let snap = Snapshot {
        version_id: uuid::Uuid::nil(),
        seq: 2,
        extracted_by: None,
        entity_types: vec![
            EntityType {
                key: "e0".to_string(),
                display_name: "甲".to_string(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
            EntityType {
                key: "e1".to_string(),
                display_name: "乙".to_string(),
                description: None,
                aliases: vec![],
                parent: None,
                disjoint_with: vec![],
                properties: vec![],
                evidence: vec![],
            },
        ],
        link_types: vec![],
        action_types: vec![],
        functions: vec![],
        concept_groups: vec![ConceptGroup {
            key: "g-甲乙".to_string(),
            display_name: "甲乙一体".to_string(),
            members: vec!["e0".to_string(), "e1".to_string(), "e-ghost".to_string()],
            scope_hint: Some("样例范围".to_string()),
            low_confidence: None,
            extracted_by: None,
            evidence: vec![],
        }],
    };
    let got = graph::declared_groups(&snap);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].key, "g-甲乙");
    assert_eq!(got[0].display_name, "甲乙一体");
    // 实体 key 换成展示名；快照里没有的成员**不臆造**（它会在投影里被跳过）
    assert_eq!(got[0].members, vec!["甲".to_string(), "乙".to_string()]);
    // ⭐ C-3：`scope_hint` 与业务总览的 `summary` 同源 —— 图这一侧照搬。
    assert_eq!(got[0].scope_hint.as_deref(), Some("样例范围"));
}

#[test]
fn projection_is_deterministic_and_groups_are_in_declaration_order() {
    let a = graph::project_grouped(&view(), &declared());
    let b = graph::project_grouped(&view(), &declared());
    assert_eq!(a, b, "同一份声明两次投影必须完全一致");
    let groups = a.groups.as_ref().expect("接通了通道");
    let keys: Vec<&str> = groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(keys, vec!["g-甲乙", "g-丙"], "组的顺序 = 声明顺序（不排序、不重排）");
}

#[test]
fn a_declared_group_without_visible_members_stays_visible_but_empty() {
    // 声明里有一个客服视图看不见的成员：组**仍然在**（不静默消失），
    // 但 members 只列图上看得见的 —— 页面上数出来的 N 与点得到的对象永远一致。
    let d = vec![DeclaredGroup {
        key: "g-空".to_string(),
        display_name: "看不见的组".to_string(),
        members: vec!["不存在".to_string()],
        scope_hint: None,
    }];
    let p: GraphProjection = graph::project_grouped(&view(), &d);
    let groups = p.groups.as_ref().expect("接通了通道");
    assert_eq!(groups.len(), 1, "组不许因为成员不可见就消失");
    assert!(groups[0].members.is_empty());
    for n in &p.nodes {
        assert_eq!(n.group, Some(None), "没有成员可见 ⇒ 没有任何节点被归进去");
    }
}

#[tokio::test]
async fn a_group_with_no_visible_member_is_not_rendered_as_a_default_group() {
    let d = vec![DeclaredGroup {
        key: "g-空".to_string(),
        display_name: "看不见的组".to_string(),
        members: vec!["不存在".to_string()],
        scope_hint: None,
    }];
    let state = state_from_view_with_groups(&view(), &d, PROJECT, 2, TOKEN, 0);
    let (status, v, _) = get(state, SCOPE, Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["groups"].as_array().map(Vec::len), Some(1));
    assert_eq!(v["groups"][0]["members"], json!([]));
    for n in v["nodes"].as_array().expect("nodes") {
        assert!(n["group"].is_null(), "空组不许把对象「默认」收进去");
    }
}

// ── C-1 · 组字段在**导入时**过客户闸（display_name / scope_hint / key）──

#[test]
fn group_fields_are_checked_before_import_not_when_serving() {
    use ontok::domain::concept::{scan_group_names, ConceptGroup};

    let clean = ConceptGroup {
        key: "api_entitlement".to_string(),
        display_name: "API 密钥与授权".to_string(),
        members: vec!["licenses".to_string()],
        scope_hint: Some("客户手上有什么凭证、以及未持凭证的试用请求".to_string()),
        low_confidence: None,
        extracted_by: None,
        evidence: vec![],
    };
    assert_eq!(
        scan_group_names(std::slice::from_ref(&clean)),
        vec![],
        "三个客户可见字段都干净时必须放行"
    );

    // ① display_name 命中
    let mut bad = clean.clone();
    bad.display_name = "未持 Key 的权益".to_string();
    let hits = scan_group_names(&[bad]);
    assert_eq!(hits.len(), 1, "display_name 里的 Key 必须被抓住：{hits:?}");
    assert_eq!(
        hits[0].to_string(),
        "组 `api_entitlement` 的 display_name 含建模术语 `key`",
        "报错要点名组 + 字段 + 词条"
    );

    // ② scope_hint 命中（QA 阻断的那一句；它也是客户可见的组说明）
    let mut bad = clean.clone();
    bad.scope_hint = Some("客户手上有什么凭证、以及未持 Key 的试用请求".to_string());
    let hits = scan_group_names(&[bad]);
    assert_eq!(hits.len(), 1, "scope_hint 里的 Key 必须被抓住：{hits:?}");
    assert_eq!(hits[0].field, "scope_hint");
    assert_eq!(
        hits[0].to_string(),
        "组 `api_entitlement` 的 scope_hint 含建模术语 `key`"
    );

    // ③ 两者都干净 ⇒ 放行（C-1 的第三种实测）
    assert_eq!(scan_group_names(&[clean]), vec![]);
}

// ── AC-006 · 页面不许自己算分组 ─────────────────────────

#[test]
fn the_page_consumes_groups_and_contains_no_grouping_algorithm() {
    let html = ontok::server::page::index_html();
    for need in [
        "DATA.groups",
        "function unitsOf(",
        "function renderedCount(",
        "function setFold(",
        "function foldAll(",
        "function relayout(",
        "grpinfo",
    ] {
        assert!(html.contains(need), "页面缺少 REQ-115 的分组渲染落点：{need}");
    }
    // ⭐ C-3：`scope_hint` 是客户可见的组说明 —— 页面照常显示（导入时已过闸）。
    assert!(
        html.contains("g.scope_hint"),
        "页面应当显示客户可见的组说明（C-3）"
    );
    let lowered = html.to_lowercase();
    for bad in ["louvain", "modularity", "community", "cluster", "greedy"] {
        assert!(
            !lowered.contains(bad),
            "页面里出现了分组算法标识 `{bad}` —— AC-006 只许消费 groups，不许自己算"
        );
    }
    assert!(
        html.len() < 80 * 1024,
        "页面必须 < 80KB（当前 {} 字节）",
        html.len()
    );
}

// ── AC-009 · 折叠不改数据（页面侧：连网络调用都没有）────

#[test]
fn folding_never_talks_to_the_server() {
    let html = ontok::server::page::index_html();
    let at = html.find("function setFold(").expect("setFold");
    let end = html.find("function updateGroupInfo(").expect("updateGroupInfo");
    let block = &html[at..end];
    assert!(!block.is_empty());
    for bad in ["fetch(", "ask(", "api/", "XMLHttpRequest"] {
        assert!(
            !block.contains(bad),
            "折叠/展开的实现里出现了 `{bad}` —— 折叠必须是纯前端状态（AC-009）"
        );
    }
    // 页面只发 GET（三个 gated 接口）；任何写方法都意味着折叠可能动到数据
    for bad in ["method: \"POST\"", "method: \"PUT\"", "method: \"DELETE\"", "method: \"PATCH\""] {
        assert!(!html.contains(bad), "页面出现了写方法：{bad}");
    }
}

// ── AC-007 的重跑：无 token 仍 401（分组接通不放松鉴权）──

#[tokio::test]
async fn the_grouped_graph_is_still_gated() {
    let (status, _, body) = get(grouped_state(), SCOPE, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "无口令必须 401");
    assert!(!body.contains("甲乙一体"), "401 正文不得回显任何分组内容");
}
