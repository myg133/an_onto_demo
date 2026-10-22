//! **REQ-027 · `mcp-server` 的协议契约与三条硬约束**。
//!
//! # 测试纪律：不许连真的 MCP 客户端
//!
//! 真客户端要装插件、要登录态、慢、CI 上不可复现。所以这一批
//! **喂假的 JSON-RPC 字节**，断言**响应的形状** —— 与 ACP 那批
//! （`test_acp_protocol.rs`）同一套做法。
//!
//! # 「标准 MCP」是怎么判定的 —— 不是靠感觉
//!
//! 本文件里的 `LINE_INIT_*` / `LINE_TOOLS_LIST` / `LINE_TOOLS_CALL`
//! **逐字段照抄 MCP 2025-06-18 规范原文**（Lifecycle / Tools / Transports 三章），
//! 而不是照抄「我们自己的输出」—— 后者只能证明自洽，证明不了是标准。
//! 每一处规范依据写在断言旁边的注释里。

mod common;
use common::test_pool;
use ontok::db::project::PgProjectStore;
use ontok::db::snapshot_store::PgStore;
use ontok::domain::concept::ConceptGroup;
use ontok::domain::snapshot::{EntityType, ExtractedBy, LinkType, Property, Snapshot};
use ontok::mcp::protocol as p;
use ontok::mcp::tools::{self, OntologyContext};
use uuid::Uuid;

fn entity(key: &str, name: &str) -> EntityType {
    EntityType {
        key: key.into(),
        display_name: name.into(),
        description: None,
        aliases: vec![],
        parent: None,
        disjoint_with: vec![],
        properties: vec![],
        evidence: vec![],
    }
}

fn entity_with_prop(key: &str, name: &str, column: &str) -> EntityType {
    let mut e = entity(key, name);
    e.properties = vec![Property {
        name: String::new(),
        column: column.into(),
        nullable: true,
        note: None,
        sample: None,
    }];
    e
}

/// 带 `note`（= 数据库**列注释原文**）的属性。
///
/// ⚠️ `note` 取的是**真实 E2E 产物里的原文**，不是编的：
/// `t034.redeemed_by` 与 `t055.model_id` 两列的注释
/// 在 `code/out/e2e-schema.json` 里可核。B1 就是被这两条原文击穿的 ——
/// 用编造的串测不出来，因为编造的人会照着判据挑一个干净的串。
fn prop_with_note(column: &str, note: &str) -> Property {
    Property {
        name: String::new(),
        column: column.into(),
        nullable: true,
        note: Some(note.into()),
        sample: None,
    }
}

/// 真实 E2E 产物里那两条**带标识符**的列注释原文。
/// 见 `code/out/e2e-schema.json` 的 `tables[].columns[].comment`。
const REAL_LEAKY_NOTE_DOTTED: &str = "关联模型ID(对应t052.model_id)";
const REAL_LEAKY_NOTE_SNAKE: &str = "核销人customer_id";
/// 一条**干净**的真实注释 —— 用来证明清洗不是「把 note 整个丢掉」。
const REAL_CLEAN_NOTE: &str = "子账号ID";

/// 一份**由规则声明**（`extracted_by: Some`）的概念组 —— 也就是
/// `concept_groups.transition.json` 今天的实况：**未经业务确认**。
fn unconfirmed_group(key: &str, display: &str, members: &[&str], hint: &str) -> ConceptGroup {
    ConceptGroup {
        key: key.into(),
        display_name: display.into(),
        members: members.iter().map(|s| s.to_string()).collect(),
        scope_hint: Some(hint.into()),
        // REQ-028 · F4：判据是**这个字段**，不是 `hint` 的文字。
        // 这里如实置 None（「没声明过」），于是下游拿到的
        // `low_confidence` 恒为 false —— 与今天真实声明的行为一致。
        low_confidence: None,
        extracted_by: Some(ExtractedBy {
            agent: "prefix-cluster-seed".into(),
            version: "0.1.0".into(),
            round: 1,
            failed_groups: vec![],
            source: None,
        }),
        evidence: vec![],
    }
}

fn base_snapshot(vid: Uuid) -> Snapshot {
    let mut s = Snapshot::new(vid, 1);
    s.entity_types = vec![
        entity_with_prop("customer", "客户", "customer_name"),
        entity_with_prop("orders", "订单", "order_no"),
    ];
    s.link_types = vec![LinkType {
        key: "order_to_customer".into(),
        from_type: "orders".into(),
        to_type: "customer".into(),
        cardinality: "N:1".into(),
        semantic: "belongs_to".into(),
        evidence: vec![],
        confidence: None,
        join_verified: None,
        join_evidence: None,
    }];
    s.concept_groups = vec![unconfirmed_group(
        "concept_customer",
        "客户",
        &["customer"],
        "⚠ 客户主档。这是**最需要业务确认的一条**",
    )];
    s
}

/// 建一份**已冻结**的快照并返回可 serve 的上下文。
async fn frozen_context() -> (PgStore, OntologyContext) {
    let pool = test_pool().await;
    common::migrate(&pool).await;
    let store = PgStore::new(pool.clone());
    let ps = PgProjectStore::new(pool.clone());
    let p = common::fresh_project(&ps, "mcp").await;
    let v = ps.new_version(p.id).await.unwrap();

    let snap = base_snapshot(v.id);
    store.save_snapshot(&snap).await.unwrap();
    ps.freeze(v.id).await.unwrap(); // ← 冻结闸门要求非 NULL

    let args = ontok::mcp::ServerArgs { project: p.name.clone(), version: 1 };
    let ctx = ontok::mcp::prepare(&pool, &args).await.expect("已冻结的版本必须能起服务");
    (store, ctx)
}

/// 一份**带真实脏 `note`** 的冻结快照 —— B1 的判别力所在。
///
/// 复刻真实 E2E 产物的两个对象：
/// - `大模型服务商`（源表 `t055`）的 `model_id` 列，
///   注释原文 `关联模型ID(对应t052.model_id)`
/// - `兑换码`（源表 `t034`）的 `redeemed_by` 列，
///   注释原文 `核销人customer_id`
///
/// ⚠️ 属性名给的是**中文**（真实抽取侧就是这么给的）—— 因为
/// `view::project` 对**认不出**的列名会兜底 `humanize_column`，
/// 而 `model_id`/`redeemed_by` 不在词表里会被**整个丢掉**，
/// 那样就构造不出「note 里有标识符」的场景，测试会假绿。
async fn leaky_note_context() -> (PgStore, OntologyContext) {
    let pool = test_pool().await;
    common::migrate(&pool).await;
    let store = PgStore::new(pool.clone());
    let ps = PgProjectStore::new(pool.clone());
    let p = common::fresh_project(&ps, "mcpnote").await;
    let v = ps.new_version(p.id).await.unwrap();

    // ⚠️ key 用**真实源表名**、display_name 用中文 —— 与真实抽取产物同构。
    // 若把中文名同时当 key，`usable_as_human_name` 会因为「值 == key」把整个对象
    // 剔掉，那样 `describe_object` 只会回 NotFound，下面的断言**全部空过**（假绿）。
    let mut provider = entity("t055", "大模型服务商");
    provider.properties = vec![{
        let mut pr = prop_with_note("model_id", REAL_LEAKY_NOTE_DOTTED);
        pr.name = "关联模型编号".into(); // ← 抽取侧给的中文名
        pr
    }];
    let mut gift = entity("t034", "兑换码");
    gift.properties = vec![{
        let mut pr = prop_with_note("redeemed_by", REAL_LEAKY_NOTE_SNAKE);
        pr.name = "核销人".into();
        pr
    }];
    let mut sub = entity("t085", "子账号额度");
    sub.properties = vec![{
        let mut pr = prop_with_note("sub_customer_id", REAL_CLEAN_NOTE);
        pr.name = "子账号".into();
        pr
    }];

    let mut snap = base_snapshot(v.id);
    snap.entity_types.extend([provider, gift, sub]);
    store.save_snapshot(&snap).await.unwrap();
    ps.freeze(v.id).await.unwrap();

    let args = ontok::mcp::ServerArgs { project: p.name.clone(), version: 1 };
    let ctx = ontok::mcp::prepare(&pool, &args).await.expect("已冻结的版本必须能起服务");
    (store, ctx)
}

// ──────────────────────────────────────────────────────────
// ① 标准 MCP 协议形状（照抄规范原文，不照抄我们自己的输出）
// ──────────────────────────────────────────────────────────

/// 规范 Lifecycle 章 `initialize` 请求原文（`protocolVersion` 是**字符串**）。
const LINE_INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{"roots":{"listChanged":true},"sampling":{},"elicitation":{}},"clientInfo":{"name":"ExampleClient","title":"Example Client Display Name","version":"1.0.0"}}}"#;

/// 规范 Tools 章 `tools/list` 请求原文。
const LINE_TOOLS_LIST: &str =
    r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"cursor":"optional-cursor-value"}}"#;

/// 规范 Tools 章 `tools/call` 请求原文。
const LINE_TOOLS_CALL: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_weather","arguments":{"location":"New York"}}}"#;

fn reply(ctx: &OntologyContext, line: &str) -> serde_json::Value {
    let out = ontok::mcp::serve::handle_line(ctx, line).expect("请求必须有响应");
    serde_json::from_str(&out).expect("响应必须是合法 JSON")
}

#[tokio::test]
async fn initialize_echoes_the_spec_requested_protocol_version() {
    let (_s, ctx) = frozen_context().await;
    let r = reply(&ctx, LINE_INIT);

    // 规范：服务端支持请求的版本就 MUST 回**同一个**
    assert_eq!(r["jsonrpc"], "2.0");
    assert_eq!(r["id"], 1);
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
}

#[tokio::test]
async fn initialize_declares_the_tools_capability_must_be_declared() {
    let (_s, ctx) = frozen_context().await;
    let r = reply(&ctx, LINE_INIT);

    // 规范 Tools 章：支持 tools 的服务器 **MUST** 声明 tools 能力
    assert!(
        r["result"]["capabilities"]["tools"].is_object(),
        "tools 能力必须声明：{}",
        r["result"]["capabilities"]
    );
    // 规范 Lifecycle：必须带 serverInfo
    assert_eq!(r["result"]["serverInfo"]["name"], "ontok");
    // 不声明自己不做的能力 —— 声明了客户端就会真的来发
    for absent in ["resources", "prompts", "logging"] {
        assert!(
            r["result"]["capabilities"].get(absent).is_none(),
            "我们不实现 {absent}，就不该声明它"
        );
    }
}

#[tokio::test]
async fn version_negotiation_falls_back_to_our_latest_when_unsupported() {
    let (_s, ctx) = frozen_context().await;
    // 规范 Lifecycle：「否则 MUST 回一个自己支持的（SHOULD 是最新的）」
    let line = r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{"protocolVersion":"1.0.0"}}"#;
    let r = reply(&ctx, line);
    assert_eq!(r["result"]["protocolVersion"], p::PROTOCOL_VERSION);
    assert!(p::SUPPORTED_PROTOCOL_VERSIONS.contains(&r["result"]["protocolVersion"].as_str().unwrap()));
}

#[tokio::test]
async fn tools_list_shape_matches_the_spec() {
    let (_s, ctx) = frozen_context().await;
    let r = reply(&ctx, LINE_TOOLS_LIST);

    // 规范 Tools 章：`result.tools` 是数组，每项有 name/description/inputSchema
    let tools = r["result"]["tools"].as_array().expect("tools 必须是数组");
    assert!(tools.len() >= 5, "§9.5 要求至少 5 个工具，实际 {}", tools.len());
    for t in tools {
        assert!(t["name"].is_string(), "每个工具必须有 name：{t}");
        assert!(t["description"].is_string(), "每个工具必须有 description：{t}");
        // 规范：inputSchema 是 JSON Schema，必须是 object 型
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
    }
}

#[tokio::test]
async fn tools_call_result_has_content_and_structured_content() {
    let (_s, ctx) = frozen_context().await;
    let line = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_concepts","arguments":{}}}"#;
    let r = reply(&ctx, line);

    // 规范 Tools 章：结果在 `content` 数组里，元素是 typed content block
    let content = r["result"]["content"].as_array().expect("content 必须是数组");
    assert_eq!(content[0]["type"], "text");
    assert!(content[0]["text"].is_string());
    assert_eq!(r["result"]["isError"], false);
    // 规范：返回结构化内容的工具 SHOULD 同时给 structuredContent
    assert!(r["result"]["structuredContent"].is_object());
}

#[tokio::test]
async fn unknown_method_is_json_rpc_method_not_found() {
    let (_s, ctx) = frozen_context().await;
    let r = reply(&ctx, r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#);
    // 规范：方法不存在 ⇒ -32601
    assert_eq!(r["error"]["code"], p::METHOD_NOT_FOUND);
    assert_eq!(r["error"]["code"], -32601);
}

#[tokio::test]
async fn unknown_tool_is_invalid_params() {
    let (_s, ctx) = frozen_context().await;
    let r = reply(&ctx, LINE_TOOLS_CALL); // 名字是 get_weather，不是我们的工具
    // 规范 Tools 章「Error Handling」原文给的例子就是 -32602
    assert_eq!(r["error"]["code"], p::INVALID_PARAMS);
    assert_eq!(r["error"]["code"], -32602);
}

#[tokio::test]
async fn malformed_json_is_parse_error_with_null_id() {
    let (_s, ctx) = frozen_context().await;
    let out = ontok::mcp::serve::handle_line(&ctx, "{ 这不是 JSON").unwrap();
    let r: serde_json::Value = serde_json::from_str(&out).unwrap();
    // JSON-RPC 2.0：解析失败必须回 id:null 的错误，**不许不回**
    assert_eq!(r["error"]["code"], p::PARSE_ERROR);
    assert!(r["id"].is_null());
}

#[tokio::test]
async fn notifications_get_no_response() {
    let (_s, ctx) = frozen_context().await;
    // 规范 Lifecycle：第三步是 `notifications/initialized`
    assert!(
        ontok::mcp::serve::handle_line(&ctx, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none(),
        "通知有响应就是协议违规"
    );
}

#[tokio::test]
async fn response_is_exactly_one_line_without_embedded_newline() {
    let (_s, ctx) = frozen_context().await;
    let out = ontok::mcp::serve::handle_line(&ctx, LINE_TOOLS_LIST).unwrap();
    // 规范 Transports：stdio 消息 MUST NOT contain embedded newlines
    assert!(!out.contains('\n'), "响应里不许有裸换行（会破坏行分隔）");
    assert!(!out.contains('\r'));
}

#[tokio::test]
async fn string_ids_are_echoed_unchanged() {
    let (_s, ctx) = frozen_context().await;
    // JSON-RPC 2.0 允许字符串 id；收窄成 i64 会让这类客户端对不上
    let r = reply(&ctx, r#"{"jsonrpc":"2.0","id":"abc-123","method":"ping"}"#);
    assert_eq!(r["id"], "abc-123");
}

// ──────────────────────────────────────────────────────────
// ② 硬要求 · 冻结闸（fail-closed）
// ──────────────────────────────────────────────────────────

#[tokio::test]
async fn unfrozen_version_is_refused_outright() {
    let pool = test_pool().await;
    common::migrate(&pool).await;
    let store = PgStore::new(pool.clone());
    let ps = PgProjectStore::new(pool.clone());
    let p = common::fresh_project(&ps, "mcpunfrozen").await;
    let v = ps.new_version(p.id).await.unwrap();
    store.save_snapshot(&base_snapshot(v.id)).await.unwrap();
    // **刻意不 freeze**

    let args = ontok::mcp::ServerArgs { project: p.name.clone(), version: 1 };
    let err = ontok::mcp::prepare(&pool, &args)
        .await
        .err()
        .expect("未冻结的版本必须被拒绝启动（fail-closed）");
    let msg = format!("{err:#}");
    assert!(msg.contains("冻结"), "错误要说清为什么：{msg}");
}

#[tokio::test]
async fn serving_reports_which_frozen_version_it_holds() {
    let (_s, ctx) = frozen_context().await;
    assert!(ctx.source_block()["frozen"].as_bool().unwrap());
    assert!(!ctx.frozen_at.is_empty(), "必须报出冻结时间");
}

// ──────────────────────────────────────────────────────────
// ③ 硬要求 · confirmation_status 标记
// ──────────────────────────────────────────────────────────

#[tokio::test]
async fn machine_declared_group_is_marked_unconfirmed() {
    let (_s, ctx) = frozen_context().await;
    let v = tools::call(&ctx, "list_concepts", &serde_json::json!({})).unwrap();

    let concepts = v["concepts"].as_array().unwrap();
    let c = concepts.iter().find(|c| c["name"] == "客户").expect("「客户」应在列");
    // 过渡声明 confirmed_by=null ⇒ 未经业务确认，**必须**如实标出
    assert_eq!(c["confirmation"]["status"], "unconfirmed");
    assert_eq!(c["confirmation"]["low_confidence"], false);
    assert!(c["confirmation"]["note"].is_string());
}

/// REQ-028 · F4：`low_confidence` 的判据是**结构化字段**，不是 `scope_hint` 文字。
///
/// # 这条测试取代了旧版 `low_confidence_hint_in_scope_becomes_a_flag`
///
/// 旧版断言「说明里写了『低置信』⇒ flag 为 true」，那正是被本 REQ 拔掉的
/// fail-open：`scope_hint` 是给人看的文字，谁改个措辞（「低置信」→「存疑」）
/// 标记就静默消失，**没有任何测试会红**。
///
/// 新版断言**两件事**，缺一不可：
/// 1. 说明文字**不再**能决定 flag（哪怕一字不差写着「低置信」）；
/// 2. 结构化字段 `Some(true)` **能**决定 flag（哪怕说明里一个字都没有）。
#[tokio::test]
async fn low_confidence_reads_the_structured_field_not_the_wording() {
    // ① 文字照旧写着「低置信」，但字段是 None（「没声明过」）⇒ false。
    //    这条在旧实现下会返回 true —— 它就是本次修复的判别点。
    let worded = unconfirmed_group(
        "concept_customer",
        "客户",
        &["customer"],
        "⚠ 低置信：权限和限流可能该分开",
    );
    assert_eq!(worded.low_confidence, None, "本用例刻意不声明结构化字段");
    assert!(
        !tools::confirmation_of(&worded).low_confidence,
        "说明文字里写了「低置信」**不得**再翻转 flag —— 判据是字段（REQ-028 · F4）"
    );

    // ② 字段声明了 true，而说明里**完全没有**低置信措辞 ⇒ true。
    let mut declared = unconfirmed_group("concept_customer", "客户", &["customer"], "客户与权限");
    declared.low_confidence = Some(true);
    assert!(
        tools::confirmation_of(&declared).low_confidence,
        "结构化字段是判据，说明措辞不该有约束力"
    );
}

/// REQ-028 · F4（端到端）：结构化字段必须**穿过快照往返**到达服务进程。
///
/// # 为什么必须单独测这一跳
///
/// `mcp::prepare` 读的是**快照里的组**（`snapshot_store::load_concept_groups`），
/// 不是 `concept_groups.transition.json`。所以只在 `ConceptGroup` 上加字段
/// 是不够的 —— 字段在「快照写库 → 读回」这一跳上恒为 NULL，
/// 于是一切低置信组在服务进程里都变成 `low_confidence: false`：
/// **换了个更好看的壳，fail-open 一模一样**。
///
/// 本测试走完整条链路（`save_snapshot` → `freeze` → `prepare` → `tools::call`），
/// 所以它会红在两种回归上：
///   - 建表/补丁漏了这一列（读回 NULL）；`or
///   - INSERT/SELECT 漏了绑定。
#[tokio::test]
async fn low_confidence_survives_the_snapshot_round_trip() {
    let pool = test_pool().await;
    common::migrate(&pool).await;
    let store = PgStore::new(pool.clone());
    let ps = PgProjectStore::new(pool.clone());
    let p = common::fresh_project(&ps, "mcplowconf").await;
    let v = ps.new_version(p.id).await.unwrap();

    let mut snap = base_snapshot(v.id);
    // 第 2 组：**声明为低置信**，且说明里刻意**不写**「低置信」二字 ——
    // 这样断言为 true 时，唯一可能的来源就是结构化字段本身。
    let mut low = unconfirmed_group("concept_low", "低置信组", &["orders"], "这一组拿不准");
    low.low_confidence = Some(true);
    snap.concept_groups.push(low);

    store.save_snapshot(&snap).await.unwrap();
    // 往返本身就要无损：读回来的字段必须还是 Some(true)
    let back = store.get_snapshot(v.id).await.unwrap().expect("快照应存在");
    assert!(
        back.concept_groups.iter().any(|g| g.key == "concept_low" && g.low_confidence == Some(true)),
        "结构化字段必须穿过快照往返（建表/INSERT/SELECT 任一漏掉都会红）"
    );

    ps.freeze(v.id).await.unwrap();
    let args = ontok::mcp::ServerArgs { project: p.name.clone(), version: 1 };
    let ctx = ontok::mcp::prepare(&pool, &args).await.expect("已冻结的版本必须能起服务");

    let out = tools::call(&ctx, "list_concepts", &serde_json::json!({})).unwrap();
    let concepts = out["concepts"].as_array().unwrap();
    let c = concepts.iter().find(|c| c["name"] == "低置信组").expect("「低置信组」应在列");
    assert_eq!(
        c["confirmation"]["low_confidence"], true,
        "服务进程必须看到结构化字段 —— 若读回 NULL，这就是一个换壳的 fail-open"
    );
    // 对照：未声明的那一组必须仍是 false（不能被顺手回填成 true）
    let other = concepts.iter().find(|c| c["name"] == "客户").expect("「客户」应在列");
    assert_eq!(other["confirmation"]["low_confidence"], false);
}

#[tokio::test]
async fn manually_revised_group_is_marked_confirmed() {
    // ADR-001：`extracted_by: None` = 人工修订。与「未经确认」是两件事。
    let mut g = unconfirmed_group("concept_customer", "客户", &["customer"], "");
    g.extracted_by = None;
    assert_eq!(tools::confirmation_of(&g).status, "confirmed");
}

#[tokio::test]
async fn marking_does_not_hide_unconfirmed_groups() {
    // BA 裁定：**标记**而不是过滤。过滤会把未归组对象连同真实结构一起藏掉。
    let (_s, ctx) = frozen_context().await;
    let v = tools::call(&ctx, "list_concepts", &serde_json::json!({})).unwrap();
    assert!(
        !v["concepts"].as_array().unwrap().is_empty(),
        "未确认的分组必须**仍然出现在**结果里，只是带标记"
    );
}

// ──────────────────────────────────────────────────────────
// ④ 硬要求 · 守门器（AC-005）
// ──────────────────────────────────────────────────────────

#[tokio::test]
async fn guard_rejects_payloads_carrying_modeling_terms() {
    // 判别力所在：把 `guard` 改成永远 Ok ⇒ 本测试立刻变红。
    //
    // ⚠️ 注意这里断言的是**建模词汇**（外键/基数/主键/evidence/key/`N:1`/
    // 未翻译的语义标识符），**不是**裸源表名 —— 守门器抓不到后者，
    // 见下一个测试。两层防线分开断言，混在一起会得出「守门器能拦标识符」
    // 的错误结论。
    for bad in [
        r#"{"x":"看这个外键"}"#,
        r#"{"x":"基数是 N:1"}"#,
        r#"{"x":"主键"}"#,
        r#"{"x":"evidence"}"#,
        r#"{"key":"customer"}"#,
        r#"{"meaning":"belongs_to"}"#,
    ] {
        let v: serde_json::Value = serde_json::from_str(bad).unwrap();
        assert!(
            tools::guard(&v).is_err(),
            "含建模术语的返回值必须被守门器拒绝：{bad}"
        );
    }
}

/// ~~守门器抓不到裸源表名~~ —— **这条测试已删除**（B3 · QA-B 判定，BA 采纳）。
///
/// # 为什么删，而不是「把断言方向反过来」
///
/// 原断言是 `assert!(tools::guard(&v).is_ok())`，
/// 断言「守门器**抓不到**裸表名」。三条理由：
///
/// 1. **断言方向与产品意图相反**：它一旦变红（= 守门器修好了），
///    修复者会本能地**改断言让它变绿**，而不是接受修复。
/// 2. **它锁的不是「已知事实」，是「防线顺序」**：项目纪律是
///    「约束要么落在代码里，要么就不存在」—— 锁顺序等于把顺序本身
///    固化成契约。
/// 3. **它的前提已被 B1 证伪**：它成立的前提是「投影层完整挡住标识符」，
///    而 B1 用真实二进制在 92 条 `describe_object` 响应里找到了泄漏
///    （`关联模型ID(对应t052.model_id)` / `核销人customer_id`）。
///    在一个前提被证伪的测试上固化缺陷，是把错误的架构判断写进测试套件。
///
/// # 标识符防线的**现行**归属
///
/// 标识符防线现在**同时**有两道，且都有断言：
/// - 自由文本 → [`ontok::mcp::tools::scrub_identifiers`]（形态判定，见下面两条测试）
/// - 结构化字段 → 投影层不序列化它们（见 `raw_identifiers_never_reach_the_output_structurally`）
///
/// 守门器**不**负责标识符（它按词元切分，保留下划线），
/// 这是**设计**，不是缺陷 —— 所以不需要也不应该有测试去断言这件事。
///
/// `#[ignore]` + `todo!()` 而不是删了就没了：留个位置提醒后来者
/// 别再把这条反向断言加回来，并把**剩余**缺口列清楚。
#[test]
#[ignore = "缺口清单，不是可运行断言"]
fn todo_identifier_defenses_have_own_assertions() {
    todo!(
        "标识符缺口清单（B1 修完后的**剩余**缺口）：\n\
         1. 守门器 `scan_modeling_terms` 抓不到 snake_case / 点路径标识符 ——\n\
        \x20   这是**设计**（它守的是建模词汇，不是标识符形态）。\n\
        \x20   标识符由 `tools::scrub_identifiers` 按形态负责。\n\
        \x20   ⚠️ **更正**：原文此处写的是「**不要**在这里加断言」—— **那句话是错的**，\n\
        \x20   而且它**正是 REQ-029 · F1 这个洞的成因**：照它办事，MCP 侧退回内联副本时\n\
        \x20   全仓一个测试都不红（`tools.rs` 的单测走 `use super::*`，打到的是**本地**\n\
        \x20   同名函数）。当初这么写，大概是不想在集成测试里重复单测已有的断言；\n\
        \x20   但**重复的是「判据值」，不重复的是「谁来调」** —— 后者单测结构上测不到。\n\
        \x20   **接线**断言见本文件下方的 `mcp_tools_actually_calls_the_shared_criterion`。\n\
         2. `sample`（样例值）**未过**标识符清洗 —— 它是**数据值**不是说明。\n\
        \x20   当前 fixture 的 `sample` 全为空，**未观察到**命中形态判据的样例值。\n\
        \x20   ⚠️ 「未观察到」**不等于**「不可达」：空集推不出不可达。\n\
        \x20   一旦样例值真的带上了标识符，这里需要补一道。\n\
         3. ~~两份判据漂移~~ **REQ-029 已修**：判据合并到 `domain::identifier_shape`，\
        \x20   页面层与 MCP 层共用同一份，两份副本都已删除。仍然分开的只有两点：\
        \x20   ① 字面量 `key` —— 页面层判据含它、MCP 侧不含（MCP 侧 `key` 由\
        \x20   `scan_modeling_terms` 兜住，**不是漏网**）。\n\
        \x20   ⚠️ **准确的后果不是「页面层更全面所以更好」**（QA · REQ-029 F2 纠正）：\n\
        \x20   页面层对 **`note` 也用含 `key` 字面量的判据**，且命中动作是**整条丢弃**\n\
        \x20   （`domain/html_view.rs` 的 `sanitize_display(note)`）。所以「`key` 只进页面层」\n\
        \x20   **只是把误伤挪了地方，并没有缩小误伤面** —— `license key` 这类误伤**在 note 上\n\
        \x20   仍然存在**，且动作**更重**（整条说明消失 > 局部替换成 `…`）。\n\
        \x20   当前语料实测 0 例（`license key` 是**真实**列注释原文，一旦出现就会命中）。\n\
        \x20   **本轮不改行为**（属既有行为，超出 REQ-029 范围），此处如实登记。\n\
        \x20   ② 命中后动作 —— 页面层整条丢弃 / MCP 侧局部替换成 `…`（**有意**）。\
         4. 形态判据**误伤样例值 / JSON 字段名**：形态分不出「列名」与「值」，\n\
        \x20   `功能编码，如 jd_api`（样例值）、`供应商配置(base_url, api_key等)`\n\
        \x20   （JSON 字段名）都被隐去 —— 真实 E2E 命中 6 条里这 2 条属误伤。\n\
        \x20   这是 fail-closed 方向的**已知代价**（见 `tools.rs` 的 B1 误伤面测试注释）。\n\
         5. 概念工具的 `summary` **原样发出**，**未过** `scrub_identifiers`：\n\
        \x20   `list_concepts` 发 `c.summary`、`describe_concept` 发 `card.summary`，\n\
        \x20   **两处都漏**。它与 `note` 同为自由文本，防线却只覆盖了 `note`。\n\
         6. ~~形态判据把小数当点路径抓~~ **REQ-029 已修**：判据现在要求\
        \x20   「段里至少有一个字母」，`0.8`/`3.14`/`2024.10`/`1_000_000` 全部放行。\
        \x20   残留：`U.S.`/`i.e.`/`No.1`/`v1.2` 这类「数字+字母」仍会命中\n\
        \x20   （收紧要走「**段长 ≥2**」那条轴，而那会把 `a.b` 也放过去 = 削弱闸门；\n\
        \x20   ⚠️ **别与「每段 vs 至少一段」的字母覆盖轴搞混**：「每段都含字母」**保得住**\n\
        \x20   `a.b`（两段都有字母），伤 `a.b` 的只有段长那条 —— 见\n\
        \x20   `domain::identifier_shape` 模块注释「残留风险」一节的对照表）；\
        \x20   真实语料实测命中 0 例，属**潜在**风险。\
         7. `llm_provider_back_20260701` 这类**带日期后缀的真实表名**：\
        \x20   判据取「至少一段含字母」正是为了不漏它；若改成「每段都含字母」\
        \x20   会在真实语料里漏掉这张表（91 表实测漏 1）。"
    )
}

/// ⚠️ **REQ-029 · F1：钉住 MCP 侧真的在调共享判据**（页面侧的对应那条在
/// `domain::identifier_shape` 的 `html_view_actually_calls_the_shared_criterion`）。
///
/// # 为什么这条**必须**待在 `tests/` 下，而不能塞进 `src/mcp/tools.rs` 的单测
///
/// `tools.rs` 的 `mod tests` 写的是 `use super::*`。一旦有人把共享判据
/// **内联回 MCP 侧**（把第 263 行的 `pub use crate::domain::identifier_shape::scrub_identifiers`
/// 换成一份本地 `fn scrub_identifiers`），`super::*` 会把那条**本地同名函数**
/// 带进测试里 —— 被测对象悄悄换人，`tools.rs` 单测那 16 条断言**全绿**，
/// 全仓一个测试都不红。判据分叉当初就是这样回来的。
///
/// 这条测试住在 `tests/` 里，**只能**通过 crate 的**公开路径**
/// `ontok::mcp::tools::scrub_identifiers` 调它。那条路径**没有第二个解析目标**：
/// 它要么是那份 `pub use`，要么就是内联副本。MCP 侧退回本地副本的那一刻，
/// 这里立刻变红 —— 这就是「判据漂移在 MCP 侧也杀得掉」的证据。
///
/// ⚠️ 注意这**不是**「再测一遍判据值」：`decimals_and_dates_are_not_identifiers`
/// 那类单测只证明**函数本身**对，证明不了**调用方有没有在调它**（M2 的教训）。
/// 所以这里断言的是**接线**：出口拿到的行为必须与 `domain::identifier_shape` 一致。
#[test]
fn mcp_tools_actually_calls_the_shared_criterion() {
    // ① REQ-029 主项：小数 / 日期 / 千分位原样放过。
    //    ⚠️ 这 4 条正是「退回内联副本（旧判据）」时的**杀死点** ——
    //    旧判据不要求任何一段含字母，`0.8` / `2024.10` / `1_000_000` 全部命中。
    for keep in [
        "折扣 0.8 折",
        "见 2024.10 的公告",
        "生效 3.14 之后",
        "金额 1_000_000 元",
    ] {
        assert_eq!(
            ontok::mcp::tools::scrub_identifiers(keep),
            keep,
            "MCP 侧出口必须走共享判据，否则小数/日期会被静默吃掉：{keep}"
        );
    }
    // ② 真标识符仍然被抓 —— 防止「接线测试」变成「放水测试」。
    //    这几条旧判据也命中，它们的作用是证明变异**只杀在①上**（方向正确）。
    for (raw, want) in [
        ("关联模型ID(对应t052.model_id)", "关联模型ID(对应…)"),
        ("核销人customer_id", "核销人…"),
        ("指向 billing_account", "指向 …"),
        // 中文前缀粘点路径：页面层曾因此漏抓，MCP 侧要确认没漏
        ("对应abc.def", "对应…"),
        // 真实表名（主仓 e2e-schema.json 里有），「至少一段含字母」正是为了不漏它
        ("llm_provider_back_20260701", "…"),
    ] {
        assert_eq!(
            ontok::mcp::tools::scrub_identifiers(raw),
            want,
            "真标识符不该被放过去：{raw}"
        );
    }
    // ③ MCP 侧**不**并入 `key` 字面量（并进去会把真实列注释 `license key` 吃成 `license …`）。
    assert_eq!(
        ontok::mcp::tools::scrub_identifiers("license key"),
        "license key"
    );
}

#[tokio::test]
async fn raw_identifiers_never_reach_the_output_structurally() {
    // 第一道防线（结构性）：我们只发 `view::project` 的产物，
    // `project()` 投影时已经把 `key`/`evidence`/`ordinal` 丢掉了。
    // 若有人绕过投影直接序列化原始快照，这个测试立刻变红。
    let (_s, ctx) = frozen_context().await;

    let line = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe_object","arguments":{"name":"订单"}}}"#;
    let r = reply(&ctx, line);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();

    // 快照里真实存在的源标识符，一个都不许**作为取值**出现在给客户端的文本里
    for leaked in ["orders", "order_no", "N:1"] {
        assert!(
            !text.contains(leaked),
            "源标识符 {leaked} 泄漏进了 MCP 返回值：{text}"
        );
    }
    // 同时确实给了客户看得懂的东西
    assert!(text.contains("订单"), "投影后的中文名必须在：{text}");

    // ⚠️ `belongs_to` 是 `EntityView` **自己的字段名**（`ontology show` 也发它），
    // `view.rs` 明写守门器**不许**把它当语义泄漏 —— 否则每一份视图都误杀。
    // 所以断言要精确：它只能作为**键名**出现，绝不能作为关系语义的**取值**出现。
    assert!(
        !text.contains("\"relation\": \"belongs_to\"")
            && !text.contains("\"relation\":\"belongs_to\""),
        "关系语义必须翻成人话，不能原样透出 belongs_to：{text}"
    );
    assert!(
        text.contains("归属于"),
        "belongs_to 应当已被翻成中文「归属于」：{text}"
    );

    // ── note 层（B1 的覆盖面扩展）──────────────────────────
    //
    // 上面这些 `orders`/`order_no` **全部来自 key 层**（结构化字段），
    // 它们证明不了 `note` 那一路也干净 —— 而 B1 实测泄漏恰恰在 `note`。
    // 所以这里用**真实 E2E 列注释原文**做输入，不是 key 层的串。
    //
    // 这半段就是「把 `raw_identifiers_never_reach_the_output_structurally`
    // 从只测 key 层扩到 note 层」—— 同一个测试，两层防线。
    let (_s2, ctx2) = leaky_note_context().await;
    for (obj, leaked) in [
        ("大模型服务商", "t052.model_id"),
        ("大模型服务商", "t052"),
        ("大模型服务商", "model_id"),
        ("兑换码", "customer_id"),
    ] {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"describe_object","arguments":{{"name":"{obj}"}}}}}}"#
        );
        let r = reply(&ctx2, &line);
        // ⚠️ **先证明这一问真有答案**，再谈泄漏。
        // 否则对象被投影剔掉时响应是 NotFound，里面当然没有标识符 ——
        // 断言会**空过**（假绿）。这正是本测试第一版的失败方式。
        assert_eq!(
            r["result"]["isError"], false,
            "对象 {obj} 必须真的能被查到，否则下面的泄漏断言是空过：{r}"
        );
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            !text.contains(leaked),
            "note 里的源标识符 {leaked} 泄漏进了 MCP 返回值（obj={obj}）：{text}"
        );
    }

    // ⚠️ 反过来也必须成立：**不许**用「把 note 整个丢掉」来绕过。
    // 干净的 note 原样透出，脏的 note 只被**局部**替换。
    let line = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe_object","arguments":{"name":"兑换码"}}}"#;
    let r = reply(&ctx2, line);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("核销人"),
        "清洗后必须**保留** note 里的业务文字「核销人」，不能整条丢掉：{text}"
    );
    let line = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe_object","arguments":{"name":"子账号额度"}}}"#;
    let r = reply(&ctx2, line);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains(REAL_CLEAN_NOTE),
        "干净的 note 必须原样保留：{text}"
    );
}

#[tokio::test]
async fn guard_lets_clean_payloads_through() {
    let v = serde_json::json!({"name": "客户", "properties": ["名称", "等级"]});
    assert!(tools::guard(&v).is_ok());
}

#[tokio::test]
async fn guard_error_message_does_not_repeat_the_offending_terms() {
    // 闸门挡住正文却从错误信息里漏后门 —— 那等于没挡。
    let v: serde_json::Value = serde_json::from_str(r#"{"x":"外键"}"#).unwrap();
    let reason = tools::guard(&v).unwrap_err();
    assert!(!reason.contains("外键"), "命中词条不许出现在返回给客户端的原因里：{reason}");
    assert!(reason.contains("术语"), "要说清是什么类型的失败：{reason}");
}

#[tokio::test]
async fn guard_is_wired_into_the_serving_path_not_just_a_helper() {
    // 造一个**故意脏**的上下文（正常数据不会这样），
    // 证明守门器真的挂在 `tools/call` 的出口上。
    // ⚠️ REQ-084：`t021` 是假名，与 768/770 的输入侧、776 的判据侧是同一个串，
    // **三处必须同批改**。脏性仍由 `外键表`（建模术语）承担，与表名无关。
    let (_s, mut ctx) = frozen_context().await;
    ctx.view.objects[0].name = "t021 外键表".into();

    let line = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"describe_object","arguments":{"name":"t021 外键表"}}}"#;
    let r = reply(&ctx, line);

    // fail-closed：给「不给数据」，而不是「给一份看着干净的数据」
    assert_eq!(r["result"]["isError"], true, "脏数据必须被拒绝外送");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!text.contains("t021"), "脏数据不许出现在响应里：{text}");
    assert!(!text.contains("外键"), "命中词条也不许出现在响应里：{text}");
}

// ──────────────────────────────────────────────────────────
// ⑤ 五个客户级业务问题都能答（§9.5）
// ──────────────────────────────────────────────────────────

#[tokio::test]
async fn five_customer_level_questions_all_get_answered() {
    let (_s, ctx) = frozen_context().await;

    // ① 「这系统里到底有哪几样东西？」
    let a = tools::call(&ctx, "list_concepts", &serde_json::json!({})).unwrap();
    assert!(!a["concepts"].as_array().unwrap().is_empty());

    // ② 「客户这个概念管什么？」
    let b = tools::call(&ctx, "describe_concept", &serde_json::json!({"name":"客户"})).unwrap();
    assert_eq!(b["name"], "客户");

    // ③ 「订单这个对象有哪些信息？」
    let c = tools::call(&ctx, "describe_object", &serde_json::json!({"name":"订单"})).unwrap();
    assert!(!c["properties"].as_array().unwrap().is_empty());

    // ④ 「订单和什么有关系？」
    let d = tools::call(&ctx, "list_relations", &serde_json::json!({"object":"订单"})).unwrap();
    assert_eq!(d["count"], 1);

    // ⑤ 「有没有跟客户有关的东西？」
    let e = tools::call(&ctx, "search_objects", &serde_json::json!({"query":"客户"})).unwrap();
    assert!(!e["concepts"].as_array().unwrap().is_empty());

    // 六问全部返回 ⇒ 且全部过守门器
    for v in [&a, &b, &c, &d, &e] {
        assert!(tools::guard(v).is_ok(), "工具返回值必须过守门器");
    }
}

/// 这三条**必须一起看** —— 它们守的是**同一个**不变量：
/// 「客户端传进来的字符串，一个字都不许原样出现在响应里」。
///
/// # 为什么拆成三条（QA-A 的判定，本轮采纳）
///
/// 原来只有 `NotFound` 一条。**只测一条**不是「少写两行」，
/// 是**测试设计问题**：三条错误路径长得几乎一样，
/// 只钉住一条就等于宣布「另外两条不用管」——
/// 后门正是从另外两条溜过去的（`没有这个工具：外键表` /
/// `不支持的方法：外键表/normalize` 原样回显）。
///
/// 协议细节各不相同，所以**分成三条测**（各自的响应形状本来就不同），
/// 但**共用一个不变量**。以后再加错误路径，照着这三条的样子补一条。
#[tokio::test]
async fn error_echoing_client_input_also_passes_the_guard() {
    // 路径①：业务上取不到（`ToolError::NotFound`）—— 响应是 tool result，`isError: true`
    let (_s, ctx) = frozen_context().await;
    let line = r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"describe_object","arguments":{"name":"外键表 不存在的东西"}}}"#;
    let r = reply(&ctx, line);

    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        !text.contains("外键"),
        "错误文案把客户端传入的建模术语原样回显了：{text}"
    );
    // 仍然要说清这是「没找到」，而不是含糊其辞
    assert!(text.contains("术语") || text.contains("给不出"), "错误文案要说清原因：{text}");
}

#[tokio::test]
async fn unknown_tool_name_is_not_echoed_back_verbatim() {
    // 路径②：未知工具（`serve.rs` 的目录检查）—— 响应是 **JSON-RPC error**，不是 tool result。
    //
    // ⚠️ 这条路径在 `tools::call` **之外**（目录里没有 ⇒ 直接回协议错误），
    // 所以 `call` 层的任何处理都够不着它 —— 必须由 `serve.rs` 自己过 `safe_error`。
    // 漏掉它时的实测输出是 `没有这个工具：外键表`。
    let (_s, ctx) = frozen_context().await;
    let line = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"外键表","arguments":{}}}"#;
    let r = reply(&ctx, line);

    let msg = r["error"]["message"].as_str().unwrap_or_else(|| {
        panic!("未知工具必须是 JSON-RPC 错误，实际响应：{r}")
    });
    assert!(
        !msg.contains("外键"),
        "未知工具的错误消息把客户端传入的建模术语原样回显了：{msg}"
    );
    // 协议码不变 —— 消毒文案不许改协议语义（规范 Tools 章给的就是 -32602）
    assert_eq!(r["error"]["code"], p::INVALID_PARAMS);
    // 命中守门词时 `safe_error` **整条替换**文案（不是只摘词），
    // 所以只能断言「说清了是术语问题」，不能断言原文里的「工具」二字还在。
    assert!(
        msg.contains("术语") || msg.contains("给不出"),
        "错误文案要说清原因：{msg}"
    );
}

#[tokio::test]
async fn unknown_method_name_is_not_echoed_back_verbatim() {
    // 路径③：未知方法（`dispatch` 的兜底臂）—— 同样是 **JSON-RPC error**。
    //
    // ⚠️ 这条路径离工具层**更远**：它连 `tools/call` 都没进。
    // 漏掉它时的实测输出是 `不支持的方法：外键表/normalize`。
    let (_s, ctx) = frozen_context().await;
    let line = r#"{"jsonrpc":"2.0","id":10,"method":"外键表/normalize","params":{}}"#;
    let r = reply(&ctx, line);

    let msg = r["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("未知方法必须是 JSON-RPC 错误，实际响应：{r}"));
    assert!(
        !msg.contains("外键"),
        "未知方法的错误消息把客户端传入的建模术语原样回显了：{msg}"
    );
    // 协议码不变（规范 Tools 章「Error Handling」：方法不认识 ⇒ -32601）
    assert_eq!(r["error"]["code"], p::METHOD_NOT_FOUND);
    assert!(
        msg.contains("术语") || msg.contains("给不出"),
        "错误文案要说清原因：{msg}"
    );
}

#[tokio::test]
async fn instructions_do_not_leak_a_modeling_term_project_name() {
    // 项目名会进 LLM 的**系统提示**（`initialize` 的 `instructions`），
    // 与 `tools/call` 的返回值是同一条外送路径。
    //
    // ⚠️ 而项目名**不受快照投影约束** —— 它是 `--project` 启动参数，
    // 走的是另一条路，所以投影层那道防线够不着它。
    let (_s, mut ctx) = frozen_context().await;
    ctx.project = "外键表项目".into();

    let text = tools::instructions(&ctx);
    assert!(
        !text.contains("外键"),
        "instructions 把项目名里的建模术语原样送进了 LLM 系统提示：{text}"
    );
    // 干净的项目名必须照常出现（不许一刀切全隐去）
    let (_s2, ctx2) = frozen_context().await;
    let clean = tools::instructions(&ctx2);
    assert!(
        clean.contains(&ctx2.project),
        "干净的项目名必须原样出现在 instructions 里：{clean}"
    );
}

#[tokio::test]
async fn every_catalogued_tool_is_actually_implemented() {
    let (_s, ctx) = frozen_context().await;
    // 目录与 `call` 的分发臂是**两处**。两处漂移的后果很具体：
    // 目录里 advertised 但分发臂没有 ⇒ 客户端看得见、点下去就报错。
    for t in tools::catalog() {
        // 空参数也要能进分发（走到「缺参数」是 BadParams，不是 Unknown）
        let got = tools::call(&ctx, t.name, &serde_json::json!({}));
        assert!(
            !matches!(got, Err(tools::ToolError::Unknown(_))),
            "工具 {} 在目录里 advertised，但分发臂里没有实现",
            t.name
        );
        // 工具名本身也得干净 —— 名字会原样进 LLM 的提示词
        assert!(
            ontok::domain::view::scan_modeling_terms(t.name).is_empty(),
            "工具名 {} 本身带建模术语",
            t.name
        );
        assert!(
            ontok::domain::view::scan_modeling_terms(&serde_json::to_string(&t.input_schema).unwrap())
                .is_empty(),
            "工具 {} 的 inputSchema 带建模术语（参数名会进返回值）",
            t.name
        );
    }
}

#[tokio::test]
async fn missing_object_is_a_tool_error_not_a_protocol_error() {
    let (_s, ctx) = frozen_context().await;
    // 连接是好的，只是这一问没答案 ⇒ 规范说的「tool execution error」
    let r = reply(
        &ctx,
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"describe_object","arguments":{"name":"不存在的东西"}}}"#,
    );
    assert_eq!(r["result"]["isError"], true);
    assert!(r["error"].is_null(), "业务查不到不是协议错误");
}