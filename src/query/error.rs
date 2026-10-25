//! REQ-123 · **对外错误契约** —— 机器可读 `code` + 人可读 `message` + **可行动 `hint`**。
//!
//! # 为什么单独一层（而不是把 `QueryError` 直接扔给调用方）
//!
//! REQ-122 的 [`QueryError`] 是**一句话**（给人读的、带表名列名的诊断），
//! 它没有「机器可分类的 code」，也没有稳定的 hint 边界。数字员工要做的判断是
//! 「换个查询重试」还是「找运维」——那是**两类动作**，靠正则去猜一句话里的中文
//! 是不该写进调用方的。
//!
//! 所以本层做两件事：① 把 `QueryError` 归到有限个 code；② 给每个 code 一条
//! **可执行**的 hint（AC-006：`UNKNOWN_PROPERTY` 要列出可用属性、
//! `TENANT_UNRESOLVED` 要指出哪张表没登记 + 改哪个声明）。
//!
//! # ⛔ 不在这里过 `safe_error`
//!
//! `mcp::tools::safe_error` 会把含建模术语的**整句**换成通用说明。那对客户视图
//! 是对的（词条就是泄漏），但在这里会把「属性 :orders__amount 不存在，它有的属性：
//! …」换掉 —— 而那句话正是 AC-006 要求的「可行动 hint」。
//! 查询服务是**内部轨**（Agent 面），错误必须**说清**；真正的隔离保证是
//! 租户注入 + 只读 + 行数上限，不是词汇闸。
//!
//! # 已知边界（⛔ 不写成「已封堵」）
//!
//! [`ApiError::from_query_error`] 的分类是**按 planner 消息的固定前缀**做的
//! （`QueryError` 今天只有一个 `message` 字段，没有 code）。消息措辞一改，分类就会
//! 悄悄漂到 `SPARQL_UNSUPPORTED`。判据：本文件末尾 `mod tests` 用**真实的 planner**
//! 造出四类错误并断言 code —— 改措辞不更新这里会**立刻红**。

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::sparql::QueryError;

/// v1 支持的语法清单（`SPARQL_UNSUPPORTED` 的 hint 入口，只此一份）。
pub const SUPPORTED_SYNTAX: &str = "v1 只支持：一条 SELECT + 基本图模式（1–3 跳）+ \
    FILTER（= != < > <= >= && || ! 与括号）+ LIMIT/OFFSET + 单列 ORDER BY + \
    COUNT/SUM/MIN/MAX/AVG + 单列 GROUP BY；不支持 OPTIONAL / UNION / 子查询 / BIND / \
    属性路径 / 函数 / CONSTRUCT / ASK / INSERT / DELETE。";

/// 一次失败的**完整**描述：HTTP 状态 + 机器码 + 两句话。
///
/// 字段即白名单：没有 `sql`、没有 `token`、没有内部路径 —— 错误正文会直接到调用方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: StatusCode,
    /// 机器可读：`TENANT_MISMATCH` / `UNKNOWN_PROPERTY` / …
    pub code: &'static str,
    /// 人可读：错在哪（一句）。
    pub message: String,
    /// **可行动**：下一步怎么办（非空，见 AC-006）。
    pub hint: String,
    /// 相近项（只有 `UNKNOWN_NAME` 用；空则不序列化该字段）。
    pub suggestions: Vec<String>,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            hint: hint.into(),
            suggestions: Vec::new(),
        }
    }

    /// 对外的 JSON 形状（**唯一**构造点；REST 与 MCP 都从这里取）。
    ///
    /// 键序由 serde_json 的 `Map`（BTreeMap）决定 ⇒ 两个协议逐字节相同。
    pub fn body(&self) -> Value {
        let mut v = json!({
            "code": self.code,
            "message": self.message,
            "hint": self.hint,
        });
        if !self.suggestions.is_empty() {
            v["suggestions"] = json!(self.suggestions);
        }
        v
    }

    /// 序列化后的正文（MCP 的 `content[0].text` 用它；REST 用同一个 `Value`）。
    pub fn json(&self) -> String {
        serde_json::to_string(&self.body()).unwrap_or_else(|_| {
            // `Value` 恒可序列化；真出错也要给一句**可用**的话，而不是 panic。
            r#"{"code":"INTERNAL","message":"错误正文生成失败","hint":"联系维护者"}"#.to_string()
        })
    }

    // ── 下面是全部错误码的**唯一**构造点 ────────────────────────

    /// 没有 / 错了 token。⛔ 不透露「token 存不存在」「差在哪一位」。
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "没有有效的凭据。",
            "把静态令牌放在 `Authorization: Bearer <token>` 头里。\
             ⛔ 不接受 query string（`?token=`）或 cookie —— 放在那里一律不生效。",
        )
    }

    /// 请求体/参数不合法（缺字段、类型不对、多了不认识的字段）。
    pub fn bad_request(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", message, hint)
    }

    /// ⭐ REQ-124 · **通用构造点**（同一个错误契约，code/status 由端点语义决定）。
    ///
    /// 控制面（`console`）的 code 集合与查询面不同（`TOKEN_KIND_FORBIDDEN` /
    /// `PROJECT_MISMATCH` / `ALREADY_FROZEN` / `VALIDATION_FAILED` …），
    /// 但**正文形状必须一模一样**：`{code, message, hint}`。让控制面自己拼一份
    /// JSON 等于第二个错误契约 —— 那正是 AC-007「错误可行动」要避免的分叉。
    ///
    /// `code` 是 `&'static str`：错误码是**端点语义的常量**，不是运行时字符串。
    pub fn with_code(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(status, code, message, hint)
    }

    /// 请求体里的 `tenant` 与 token 绑定的租户不一致（R1，HTTP 403）。
    pub fn tenant_mismatch(bound: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "TENANT_MISMATCH",
            "请求体里的 tenant 与凭证绑定的租户不一致。",
            format!(
                "租户由凭证决定（R1）：你这张 token 绑定的是 {bound}。\
                 正常调用**不需要**在请求体里写 tenant；去掉这个字段即可，\
                 要访问别的租户请换一张绑定该租户的 token。"
            ),
        )
    }

    /// 同一 token（或全局）同时在跑的查询太多。
    pub fn too_many_concurrent(token_limit: usize, global_limit: usize) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "TOO_MANY_CONCURRENT",
            "同时进行的查询太多，本次没有执行。",
            format!(
                "同一 token 的并发上限是 {token_limit}（服务全局上限 {global_limit}）。\
                 等前面的查询结束后重试（串行调用最稳）；要更高的并发请让运维调大 tokens.json \
                 里这张 token 的 `max_concurrency`。"
            ),
        )
    }

    /// 同一 token（或全局）打开的 SSE 流太多。
    pub fn too_many_streams(token_limit: usize, global_limit: usize, idle_ms: u64) -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "TOO_MANY_STREAMS",
            "同时打开的 SSE 流太多，本次没有建立。",
            format!(
                "同一 token 的 SSE 流上限是 {token_limit}（全局上限 {global_limit}）。\
                 先关掉不用的流；服务端会在空闲 {idle_ms} ms 后主动断开超过空闲超时的流。"
            ),
        )
    }

    /// 查询超时（服务端 `statement_timeout` 或客户端兜底）。
    pub fn timeout(timeout_ms: u64) -> Self {
        Self::new(
            StatusCode::GATEWAY_TIMEOUT,
            "TIMEOUT",
            format!("查询超时（{timeout_ms} ms），已中止。"),
            "缩小范围重试：加 FILTER / 更具体的 join / 更小的 LIMIT；\
             或让运维调大服务的 `--timeout-ms`。",
        )
    }

    /// `describe?name=` 找不到那个类/属性。
    pub fn unknown_name(name: &str, suggestions: Vec<String>) -> Self {
        let hint = if suggestions.is_empty() {
            "名字要写**类本地名**（catalog 里 class 项的 `name`，如 `:customers`）\
             或**属性本地名**（如 `:customers__id`）。用 GET /api/catalog 翻页看全部名字，\
             或 MCP 的 `catalog` 工具。"
                .to_string()
        } else {
            format!(
                "名字要写**类本地名**（如 `:customers`）或**属性本地名**（如 `:customers__id`）。\
                 最接近的几个：{}。用 GET /api/catalog 翻页看全部名字。",
                suggestions.join("、")
            )
        };
        let mut e = Self::new(
            StatusCode::NOT_FOUND,
            "UNKNOWN_NAME",
            format!("没有叫 {name:?} 的类或属性。"),
            hint,
        );
        e.suggestions = suggestions;
        e
    }

    /// 服务内部错误。正文**只说一句话**，细节进 stderr。
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            message,
            "把出现的时间与这次调用的 code 报给维护者；这不是你的查询写法问题。",
        )
    }

    /// 参数/方法不支持（MCP 的 `-32602` 用它，正文同 REST 的错误契约）。
    pub fn bad_tool_params(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "BAD_REQUEST", message, hint)
    }

    /// ⭐ REQ-136 · **本部署没有数据源**（`--ontology-only`：只提供本体，不接业务库）。
    ///
    /// 为什么是 **503** 而不是 200 空结果 / 500：
    ///
    /// | 码 | 调用方会读成 | 对不对 |
    /// | --- | --- | --- |
    /// | 200 + `rows: []` | 「查了，没有数据」 | ❌ 把「查不了」说成「查不到」 |
    /// | 500 | 「服务坏了，重试/报障」 | ❌ 服务是好的，只是这个部署没有库 |
    /// | **503 + `NO_DATA_SOURCE`** | 「这个能力在当前部署里不存在」 | ✅ |
    ///
    /// hint 必须让调用方**不要重试**（R4 / 需求风险 3）：503 常被当成瞬时故障，
    /// 而这里重试一万次也还是同一句话。REST 与 MCP 用的是**同一个**函数与同一份
    /// `message` / `hint`（两个协议的错误正文逐字相同）。
    pub fn no_data_source() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "NO_DATA_SOURCE",
            "本部署只提供本体，未接业务库 —— 没有可查询的数据源，SPARQL / 取样都不可用。",
            "这个部署是**本体服务模式**（`query serve --ontology-only`）：只有本体（类 / 关系 / 属性 / TTL），\
             **没有接业务库**。⛔ **重试无效**（这不是暂时故障，也不是你的查询写法问题）。\n  \
             要数据请：① 继续用 GET /api/catalog、GET /api/describe、GET /api/ontology.ttl（或 MCP 的 \
             catalog / describe / resources/read）；② 需要真查数据时，让运维在有源库的环境里以**普通模式**\
             （`ONTOK_SOURCE_DATABASE_URL` + `--schema` + `--policy`，不带 `--ontology-only`）另起一个查询服务。",
        )
    }

    /// ⭐ REQ-145 · **查询打了 API 源的类**，而 `format=rows` 挂不住 `@pending`（R5）。
    ///
    /// 为什么是 **400 + 明确报错**，而不是 200 空表：表挂不住「少了一块」这件事，
    /// 空表会被调用方读成「查了，这个类没有数据」—— 那正是本需求要消灭的静默错误
    /// （契约 §六 的 `NOT_DB_SOURCE`：Agent 的动作是**改查 catalog 看 `access.kind`**）。
    ///
    /// hint 必须给出**两条**可走的路：① 换 `format=jsonld` 拿 `@pending` + `get`；
    /// ② 按 `get.class` / `get.filter` 去取，或绕开这些类重写查询。
    pub fn not_db_source(classes: &[String]) -> Self {
        let named = if classes.is_empty() {
            "这次查询点名的类".to_string()
        } else {
            classes.join("、")
        };
        Self::new(
            StatusCode::BAD_REQUEST,
            "NOT_DB_SOURCE",
            format!(
                "查询落在 **API 源**上（{named}）：这些类的数据不在本库，平台**不执行** API 调用，\
                 行回执（`format=rows`）无法表达「少了一块」。"
            ),
            "① 改要 **JSON-LD**：`?format=jsonld` —— 会拿到 `@graph: []` + `@pending[]`\
             （每条带 `get.class` / `get.filter` / `get.pushdown` / `get.ref`），\n  \
             ② 再按 `get` 去取（凭据由你管）：`filter.ids` 里就是平台从查询里认出来的实例标识，\n  \
             ③ 平台**不做**跨源 join：需要库源 + API 源一起答时，拆成两段查（先库源、再按 `get` 取），\n  \
             ④ 想知道哪些类要这么走：`GET /api/catalog` / `GET /api/describe` 看 `access.kind`\
             （`db` ⇒ `queryable_by:[\"sparql\"]`；`api` ⇒ `queryable_by:[\"fetch\"]`）。\n  \
             ⛔ 重试无效：这不是瞬时故障，也不是你的查询写法问题。",
        )
    }

    /// ⭐ REQ-145 · **查询需要跨源遍历**（一条边两端一条在库、一条在 API）。
    ///
    /// 契约规则 4 的落地：跨源边**不下发 `get`**、也**不执行**（平台不做跨源 join，
    /// 契约 §七「谁都不做」）。Agent 的动作是**拆成两段**（契约 §八 第 5 条：别重试）。
    pub fn cross_source_unsupported(predicate: &str, from: &str, to: &str) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "CROSS_SOURCE_UNSUPPORTED",
            format!(
                "查询需要**跨源遍历**：边 `{predicate}` 一端在库源（`{from}`）、另一端在 API 源\
                 （`{to}`），平台不执行跨源 join。"
            ),
            format!(
                "拆成两段：① 先只查库源那一段（只写 `:{from}` 的类 / 属性，不写 `:{predicate}`），\
                 拿到本地的实例标识；\n  \
                 ② 再对 `{to}` 单独查（`?format=jsonld` 会给你 `@pending[].get`，按 `filter.ids` 去取），\
                 由你在答案里拼起来。\n  \
                 ⛔ 别重试同一条查询：这条边平台**永远不会**执行（需求「不做跨源 join / 联邦」）。\n  \
                 想确认哪些类来自 API：`GET /api/catalog` 看 `access.kind`。"
            ),
        )
    }

    /// 把 planner 的错误归到有限个 code（见模块头的已知边界）。
    ///
    /// 归不出来的一律 `SPARQL_UNSUPPORTED`（语法/子集问题最多），并给出支持清单 ——
    /// **⛔ 不许归成 INTERNAL**：那会让调用方以为服务坏了，而实际上多数是写法问题。
    pub fn from_query_error(e: QueryError) -> Self {
        let msg = e.into_message();
        let (head, tail) = split_head_tail(&msg);
        let hint_tail = |extra: &str| match &tail {
            Some(t) => format!("{t}\n{extra}"),
            None => extra.to_string(),
        };

        if msg.contains("查询超时") || msg.contains("statement timeout") {
            // ⚠️ `run_query` 会给执行期错误追加 `SQL: …`（对 PG 报错是可行动的，
            // 但超时的原因不在 SQL 文本上）。超时的 hint 只给动作，不贴 SQL：
            // 调用方拿到的信息里，`sql_echo` 只在**成功**回执里才有意义。
            return Self::new(
                StatusCode::GATEWAY_TIMEOUT,
                "TIMEOUT",
                head,
                "缩小范围重试（加 FILTER / 更具体的 join / 更小的 LIMIT），\
                 或让运维调大服务的 `--timeout-ms`。",
            );
        }
        if msg.contains("租户过滤无法确定") {
            // AC-006：必须指出**哪张表没登记** + 改哪个声明。
            return Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "TENANT_UNRESOLVED",
                head,
                hint_tail(
                    "这是**部署侧的声明缺口**（不是你的查询写法问题）：\
                     在 query-policy.json 的 `perTable` 里给这张表登记 `tenantColumn`，\
                     或在 `publicDimensions` 里登记并写 `reason`；改完**重启**查询服务。\
                     ⛔ 服务不会「猜一个列」或「不过滤放行」（fail-closed）。",
                ),
            );
        }
        if msg.contains("属性 :") && msg.contains("不存在") {
            return Self::new(
                StatusCode::BAD_REQUEST,
                "UNKNOWN_PROPERTY",
                head,
                hint_tail(
                    "用 GET /api/catalog 翻页看全部属性（`kind: \"property\"` 的 `name`），\
                     或 MCP 的 `catalog` / `describe` 工具。",
                ),
            );
        }
        if msg.contains("没有类 :") {
            return Self::new(
                StatusCode::BAD_REQUEST,
                "UNKNOWN_CLASS",
                head,
                hint_tail("用 GET /api/catalog 翻页看全部类（`kind: \"class\"` 的 `name`）。"),
            );
        }
        if msg.contains("SQL 执行失败") {
            return Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "QUERY_FAILED",
                head,
                hint_tail(
                    "SQL 已按你的查询生成但执行失败（不是语法问题）：把 code 与时间报给维护者；\
                     也可以缩小范围重试一次。",
                ),
            );
        }

        // 兜底：语法 / 子集问题。hint 必须给出「支持什么」的入口。
        Self::new(
            StatusCode::BAD_REQUEST,
            "SPARQL_UNSUPPORTED",
            head,
            hint_tail(&format!(
                "{SUPPORTED_SYNTAX}\n需要看有哪些类/属性：GET /api/catalog 或 MCP 的 `catalog` 工具。"
            )),
        )
    }
}

/// 把一条多行诊断拆成「第一句」+「其余」。
///
/// planner 的消息都是 `第一句\n  可执行清单` 的形状（见 `sparql::plan`），
/// 于是 `message` 是「错在哪」、`hint` 是「怎么办」—— ⛔ 不丢任何一行。
fn split_head_tail(msg: &str) -> (String, Option<String>) {
    match msg.split_once('\n') {
        Some((head, tail)) => {
            let tail = tail.trim();
            (
                head.trim().to_string(),
                if tail.is_empty() {
                    None
                } else {
                    Some(tail.to_string())
                },
            )
        }
        None => (msg.trim().to_string(), None),
    }
}

// ══════════════════════════════════════════════════════════════
// 判据
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparql::plan::PlanInput;
    use crate::sparql::policy::QueryPolicy;
    use crate::domain::model_view::{
        ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
    };
    use crate::domain::r2rml::build_mapping;
    use crate::domain::snapshot::{Evidence, JoinState};
    use crate::domain::ttl::{SchemaTypes, TtlOptions};

    const ONT: &str = "https://ontok.local/demo";

    /// 一个最小夹具：`customers` + `orders`（orders 登记了租户列）。
    fn fixture() -> (SchemaTypes, crate::domain::r2rml::R2rmlMapping) {
        let schema = SchemaTypes::parse(
            r#"{"tables":[
              {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
               "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                          {"name":"customer_name","data_type":"text"}]},
              {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
               "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
                          {"name":"customer_id","data_type":"text"},
                          {"name":"amount","data_type":"numeric(12,4)"}]}
            ]}"#,
        )
        .expect("夹具 schema");
        let prop = |n: &str, c: &str| ModelPropertyView {
            name: n.into(),
            column: c.into(),
            nullable: false,
            note: None,
            sample: None,
        };
        let entity = |k: &str, props: Vec<ModelPropertyView>| ModelEntityView {
            key: k.into(),
            display_name: k.into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: props,
            evidence: vec![],
        };
        let link = ModelLinkView {
            key: "orders_customer_id__customers".into(),
            from_type: "orders".into(),
            to_type: "customers".into(),
            cardinality: "N:1".into(),
            semantic: "belongs_to".into(),
            evidence: vec![Evidence {
                table: "orders".into(),
                column: "customer_id".into(),
                claim: "外键".into(),
            }],
            confidence: None,
            join_verified: Some(JoinState::Verified),
            join_evidence: Some(
            "value-overlap 5/5 (100.00%) -> customers.id via primary_key (plural)".into(),
        ),
            column: Some("customer_id".into()),
        };
        let view = ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                entity("customers", vec![prop("客户名", "customer_name")]),
                entity(
                    "orders",
                    vec![prop("金额", "amount"), prop("客户", "customer_id")],
                ),
            ],
            link_types: vec![link],
            action_types: vec![],
            functions: vec![],
        };
        let mut opts = TtlOptions::new("demo", 2);
        opts.schema = Some(&schema);
        let mapping = build_mapping(&view, &opts, false, Default::default()).expect("映射");
        (schema, mapping)
    }

    /// 用**真实的 planner** 造错误 → 断言 code（改措辞不更新本表 ⇒ 立刻红）。
    #[test]
    fn planner_errors_map_to_the_declared_codes() {
        let (schema, mapping) = fixture();
        let policy = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id"}},"publicDimensions":[]}"#,
        )
        .expect("声明");
        let plan_err = |sparql: &str| {
            crate::sparql::plan::plan(
                sparql,
                &PlanInput {
                    mapping: &mapping,
                    schema: &schema,
                    ontology_iri: ONT,
                    policy: &policy,
                    tenant: "C0001",
                    max_rows: 100,
                },
            )
            .expect_err("这条查询应当报错")
        };

        // ① 未知属性
        let e = ApiError::from_query_error(plan_err(
            r#"SELECT ?o ?x WHERE { ?o :orders__nope ?x }"#,
        ));
        assert_eq!(e.code, "UNKNOWN_PROPERTY", "{}", e.message);
        assert!(e.hint.contains(":orders__amount"), "hint 必须列出可用属性：{}", e.hint);

        // ② 未知类
        let e = ApiError::from_query_error(plan_err(
            r#"SELECT ?x WHERE { ?x a :nonexistent }"#,
        ));
        assert_eq!(e.code, "UNKNOWN_CLASS", "{}", e.message);
        assert!(e.hint.contains("customers"), "hint 必须列出可用类：{}", e.hint);

        // ③ 不支持的语法（UNION）
        let e = ApiError::from_query_error(plan_err(
            r#"SELECT ?x WHERE { { ?x :orders__amount ?a } UNION { ?x a :customers } }"#,
        ));
        assert_eq!(e.code, "SPARQL_UNSUPPORTED", "{}", e.message);
        assert!(e.hint.contains("OPTIONAL"), "hint 必须给支持清单：{}", e.hint);
    }

    /// `TENANT_UNRESOLVED` 的 hint 必须点名「哪张表没登记 + 改哪个声明」。
    ///
    /// 造法：把声明改成**不登记 orders**，然后查 orders 的属性 ⇒ planner 报
    /// 「租户过滤无法确定：表 orders …」。
    #[test]
    fn tenant_unresolved_names_the_table_and_the_declaration() {
        let (schema, mapping) = fixture();
        let policy = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{},"publicDimensions":[]}"#,
        )
        .expect("声明");
        let err = crate::sparql::plan::plan(
            r#"SELECT ?o ?a WHERE { ?o :orders__amount ?a }"#,
            &PlanInput {
                mapping: &mapping,
                schema: &schema,
                ontology_iri: ONT,
                policy: &policy,
                tenant: "C0001",
                max_rows: 100,
            },
        )
        .expect_err("未登记的表必须被拒绝");
        let e = ApiError::from_query_error(err);
        assert_eq!(e.code, "TENANT_UNRESOLVED", "{}", e.message);
        assert!(e.message.contains("orders"), "message 必须点名表：{}", e.message);
        assert!(
            e.hint.contains("perTable") && e.hint.contains("publicDimensions"),
            "hint 必须指出改哪个声明：{}",
            e.hint
        );
    }

    /// ⭐ AC-006 的最小判据：**每个** code 的 hint 都非空，且不是「出错了」。
    #[test]
    fn every_code_has_an_actionable_hint() {
        let all = [
            ApiError::unauthorized(),
            ApiError::bad_request(
                "请求体里多了不认识的字段。",
                "可用字段只有 query / max_rows / tenant（tenant 只用于核对）。",
            ),
            ApiError::tenant_mismatch("C0001"),
            ApiError::too_many_concurrent(1, 2),
            ApiError::too_many_streams(1, 2, 60_000),
            ApiError::timeout(10_000),
            ApiError::unknown_name(":nope", vec![":customers".into()]),
            ApiError::unknown_name(":nope", vec![]),
            ApiError::internal("执行时出了点问题。"),
            ApiError::bad_tool_params(
                "arguments 不是 JSON 对象。",
                "catalog 的参数形如 {\"page\": 1, \"page_size\": 50}。",
            ),
        ];
        for e in all {
            assert!(!e.hint.trim().is_empty(), "{} 没有 hint", e.code);
            assert!(e.hint.len() >= 12, "{} 的 hint 太短，不可执行：{}", e.code, e.hint);
            assert!(!e.message.contains("出错了"), "{} 的 message 是废话", e.code);
            // body 里三个键恒在（调用方可以无条件读）
            let b = e.body();
            assert!(b.get("code").is_some());
            assert!(b.get("message").is_some());
            assert!(b.get("hint").is_some());
        }
        // `TENANT_MISMATCH` 的 hint 必须说明「租户由凭证决定」。
        let m = ApiError::tenant_mismatch("C0001");
        assert!(m.hint.contains("租户由凭证决定"), "{}", m.hint);
        assert_eq!(m.body()["code"], "TENANT_MISMATCH");
    }
}
