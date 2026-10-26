//! REQ-123 · **查询服务对外（REST + 远程 MCP）** —— 一个进程、两种协议、**一份鉴权**。
//!
//! ```text
//! POST /api/sparql     ─┐
//! GET  /api/catalog     ├─▶ token → tenant ─▶ REQ-122 的 run_query() ─▶ SQL
//! GET  /api/describe    ┤        │
//! POST /mcp (JSON-RPC)  ┤        └─▶ 限额 / 审计 / 错误契约
//! GET  /mcp (SSE)      ─┘
//! ```
//!
//! # ⭐ REQ-136 · 两种模式（[`ServiceMode`]），一套适配器
//!
//! | | [`ServiceMode::Query`]（默认） | [`ServiceMode::OntologyOnly`]（`--ontology-only`） |
//! | --- | --- | --- |
//! | 源库 | 启动时连、读回只读会话 | **`None`**（启动时不连、任何请求也不连 —— R2） |
//! | schema / R2RML 映射 | 必须有 | **`None`**（不做对账，不来自动造 schema） |
//! | `catalog` / `describe` | 只列映射里可查的类/关系 | 直接读快照（类/关系**全列**，R3） |
//! | `/api/ontology.ttl` | 同 CLI 导出 | 同 CLI 导出（客户视图与无 `--schema` 的导出逐字节相同） |
//! | `sparql_query` / `sample` | 真查 | **需要库的查询** ⇒ 503 `NO_DATA_SOURCE`（R4；⛔ 不是 200 空结果、⛔ 不是 500）；**纯 api 类**（加载了 `--access-mapping`）⇒ 与普通模式同一份 `@pending`（jsonld 200 / rows 400 `NOT_DB_SOURCE`） |
//!
//! ⭐ REQ-151 · 本体模式的「恒 503」被收窄成「**需要库**才 503」：只碰 api 源的查询
//! 本来就不需要库，它要的是「你去哪取」（`@pending` + `get`）。混合查询（库类 + api 类）
//! 仍 503 —— 它确实需要库。判据与渲染走**同一条**路径（`source_gate` + `PendingAnswer`），
//! ⛔ 没有第二份 `@pending` 渲染。
//!
//! 分叉只有两处：**数据源有没有**（[`run_sparql`] 的第一段）与
//! **目录的过滤**（[`catalog::build_items`] 的 `mapping: Option<…>`）。
//! 鉴权 / 限额 / 审计 / 错误契约 / TTL / 分页**完全共用** —— 于是「REST 拦了、MCP 忘了拦」
//! 在这两个模式下都没有发生的位置。
//!
//! # 纪律 H：两个协议只是**适配器**
//!
//! | 事 | 唯一实现 |
//! | --- | --- |
//! | 鉴权（Bearer → token 表 → tenant） | [`http::require_token`]（一个中间件，挂在 `/api/*` 与 `/mcp` 上） |
//! | 查询（规划 / 注入 / 只读执行） | [`crate::sparql::run_query`]（REQ-122 的同一个函数，⛔ 不另写） |
//! | 回执构造 | `receipt_of()`（唯一构造函数）+ [`QueryService::join_states`]（含范围标签 [`JOIN_STATES_SCOPE_ALL_EDGES`]） |
//! | 回执序列化 | [`SparqlReceipt::serialize`]（REST 走 `Json`、MCP 走 `content[0].text`，**同一个函数**） |
//! | 限额 / 审计 / 错误码 | [`run_sparql`]、[`audit`]、[`error`]（两个协议都经过同一段代码） |
//!
//! 判据（AC-004）：同一 token、同一查询，REST 与 MCP 的回执**序列化后逐字节相同**
//! （`tests/test_req123_live.rs` 对拍 sha256）。
//!
//! # 租户只能来自 token（R1）
//!
//! [`Caller`] 里只有从 token 表查出来的 [`tokens::TokenEntry`]；请求体里的
//! `tenant` 只用于**比对**（[`run_sparql`] 的第一段），不一致 ⇒ 403 `TENANT_MISMATCH`。
//! 规划器拿到的 `PlanInput::tenant` **只可能**是 `entry.tenant` —— 类型上就没有第二条路。
//!
//! # 回执扩展（AC-007 / 需求 7）
//!
//! 在 REQ-122 的九个字段之后追加五项：
//!
//! - `coverage[]`：查询涉及的每张表 → `complete` / `main_only`（来自声明，⛔ 不推断）；
//! - `join_states[]`：**涉及的表之间**的全部映射边 → `true` / `inherit` / `false` +
//!   证据原文 + `used`（这条边本次有没有被查询用到）；
//! - `join_states_scope`：`join_states[]` 的**范围标签**（机器可读，恒为
//!   [`JOIN_STATES_SCOPE_ALL_EDGES`] = `all_edges_between_involved_tables`）；
//! - `join_states_scope_note`：同一件事的**人话** —— 让只读回执的调用方（尤其是 AI）
//!   不必翻文档就知道该把 `join_states[]` 读成什么；
//! - `limits`：本次实际生效的 `max_rows` / `timeout_ms`（调用方看得出上限被谁压的）。
//!
//! ## `join_states[]` 的范围（⛔ 别读成「本次查询走到的边」）
//!
//! 范围是**「涉及的表之间的全部映射边」**：只要一张表出现在 `tables[]` 里，声明/映射里
//! 它与另一张同样在场的表之间的**每一条** join 都会列出来 —— **不论这次查询有没有用到它**。
//!
//! 三表例子（真实查询，QA 实测过的误读场景）：
//!
//! ```sparql
//! SELECT ?l ?c ?b WHERE { ?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c }
//! ```
//!
//! `tables[]` = `licenses` / `customers` / `balances`，于是
//! `balances --balances__customers__source_customer_id--> customers` 也会出现在
//! `join_states[]` 里（两张表都在场），尽管**本次根本没走到它**；这种条目 `used: false`。
//!
//! `used` 是怎么判的：planner 只会为**查询里写出来的**关系谓词生成 join
//! （`connect_nodes` 只在已有的边上选，⛔ 不造隐式笛卡尔积），所以「该边的谓词（或它的
//! 反向谓词）出现在查询里」⇔「这条边被 plan 用到了」。判定走的是**再解析一遍查询**
//! （[`QueryService::used_join_predicates`]），不是字符串猜。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - `join_states[]` 有**冗余条目**（多表多边时，涉及但没走的边也在里面）：这是 v1 的
//!   范围口径（BA 裁定 J-2），不是 bug —— 靠 `used` 与 `join_states_scope` 区分。
//!   要做到「只列走过的边」得让 `Planned` 暴露用过的边（要动 REQ-122 的结构），后续需求。
//! - token 表**启动时读一次**（不热加载）；改 token 要重启。
//! - 审计的 token 摘要不加盐（见 [`audit`] 的已知边界）。

pub mod audit;
// ⭐ REQ-145 · access 映射（`--access-mapping`）：哪些类来自 API、跨源边在哪、查询落到哪一类。
pub mod source_gate;
pub mod catalog;
pub mod error;
pub mod http;
// ⭐ REQ-140 · SPARQL 结果 → JSON-LD 上下文子图（纯函数；零 I/O、零依赖）。
pub mod jsonld;
pub mod serve;
pub mod tokens;
// ⭐ REQ-127 · 整份 OWL（TTL）的两视图：启动时渲染一次并缓存（客户 / 建模）。
pub mod ttl;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;

use crate::sparql::exec::SessionFacts;
use crate::sparql::plan::{PlanInput, PublicDimensionUsed, TenantApplied};
use crate::sparql::policy::{QueryPolicy, TableDecision};
use crate::sparql::QueryResult;

use audit::{digest_prefix, sha256_hex, AuditRecord, AuditSink};
use error::ApiError;
use tokens::{TokenEntry, TokenTable};

/// SPARQL 语法版本（健康检查与错误 hint 里出现；与 REQ-122 的 v1 子集同一份）。
pub const SYNTAX_VERSION: &str = "sparql-v1";

/// 调用来自哪个协议（审计里只有这两个值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Rest,
    Mcp,
}

impl Protocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Protocol::Rest => "rest",
            Protocol::Mcp => "mcp",
        }
    }
}

/// ⭐ REQ-136 · **服务的两种模式**（启动时按 `--ontology-only` 定，运行期不变）。
///
/// | 模式 | 源库 | schema / R2RML 对账 | catalog / describe | SPARQL |
/// | --- | --- | --- | --- | --- |
/// | [`ServiceMode::Query`] | ✅ 必须连 | ✅ 必须 | 只列**可查**的类/关系 | 真查 |
/// | [`ServiceMode::OntologyOnly`] | ⛔ **一处都不连** | ⛔ 不做 | 直接读快照（关系照列） | 需要库 ⇒ **503** `NO_DATA_SOURCE`；纯 api 类（有映射）⇒ `@pending`（REQ-151） |
///
/// 类型上是**一个枚举**而不是两个 `bool`：「既接库又不接库」写不出来。
/// 两个模式共用的部分（鉴权 / 限额 / 审计 / 错误契约 / TTL / 目录构建）是同一段代码，
/// 分叉只在**过滤**（见 [`catalog::build_items`]）与**数据源有无**（见 [`run_sparql`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceMode {
    /// 普通模式：有源库，做 R2RML 对账，SPARQL 真查（REQ-123 起的既有行为）。
    Query,
    /// 本体服务模式：不接业务库、不做 schema 对账；catalog/describe 直接读快照。
    OntologyOnly,
}

impl ServiceMode {
    /// 对外出现在 `/api/health` 与启动横幅里的字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            ServiceMode::Query => "query",
            ServiceMode::OntologyOnly => "ontology-only",
        }
    }

    /// 本体服务模式？（数据源 / 对账相关分支的唯一判据）
    pub const fn is_ontology_only(self) -> bool {
        matches!(self, ServiceMode::OntologyOnly)
    }

    /// 横幅里那个人读的一行（需求风险 2：该模式被误用于有库项目时，横幅要说清）。
    ///
    /// 普通模式返回 `None`：既有横幅**一个字节都不改**（AC-006 的「零行为变化」从日志
    /// 也照做）；本体模式多印一行 `mode=ontology-only（不接业务库）`（AC-001 的判据）。
    pub const fn banner_line(self) -> Option<&'static str> {
        match self {
            ServiceMode::Query => None,
            ServiceMode::OntologyOnly => Some(
                "· 模式：mode=ontology-only（不接业务库）· 无数据源 · SPARQL 需要库 ⇒ 503 NO_DATA_SOURCE；\
                 纯 api 类（有 --access-mapping）⇒ @pending 取数指引 · 取样恒 503\n",
            ),
        }
    }
}

/// 一次已鉴权的调用：**token 表里那一行** + 协议。
///
/// 这是规划器能看到的全部身份信息 —— ⛔ 没有「请求体里的 tenant」这条路。
#[derive(Debug, Clone)]
pub struct Caller {
    pub token: TokenEntry,
    pub protocol: Protocol,
}

// ══════════════════════════════════════════════════════════════
// 请求 DTO（REST 体与 MCP arguments **同一套解析**）
// ══════════════════════════════════════════════════════════════

/// `POST /api/sparql` 的 body / MCP `sparql_query` 的 arguments。
///
/// `tenant` 字段**只为比对**而存在：值必须等于 token 绑定的租户，否则 403。
/// `deny_unknown_fields`：拼错字段名（`"max_row"`）会报 400 并列出可用字段，
/// ⛔ 不静默忽略（静默忽略会让调用方以为限额生效了）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparqlRequest {
    pub query: String,
    #[serde(default)]
    pub max_rows: Option<u64>,
    #[serde(default)]
    pub tenant: Option<String>,
    /// ⭐ REQ-140 / REQ-142 · 回执格式（`jsonld`(默认) / `rows`）。⚠️ 它**只**决定渲染，
    /// ⛔ 不参与规划 / 注入 / 限额 / 审计的任何判断。
    ///
    /// REST 侧还有两个入口（`?format=` 与 `Accept`），三者的优先级见
    /// [`crate::query::http`] 的 `sparql_format`：**显式 `?format=` > 本字段 > `Accept` 头**；
    /// 三处都没给线索时用 [`QueryFormat::default`]（REQ-142 起 = `jsonld`，即
    /// [`crate::query::DEFAULT_FORMAT`]）。要行回执：`?format=rows` /
    /// `Accept: application/json` / 本字段 `"rows"`（三选一）。
    #[serde(default)]
    pub format: Option<QueryFormat>,
}

/// ⭐ REQ-140 / REQ-142 · 回执格式：`jsonld`（**默认**，上下文子图）/ `rows`（REQ-122 的行回执）。
///
/// ⭐ REQ-142 · **默认翻转**：三处都没写线索时返回 JSON-LD（[`DEFAULT_FORMAT`]），
/// 因为数字员工要的是图。`rows` 这条路**没有删**：显式写 `format`（REST 的 body 或
/// `?format=rows`、MCP 的 `format:"rows"`）或 `Accept: application/json` 即可，字节与
/// REQ-122/123 的行回执**逐字节相同**（R2）。
///
/// 这个类型只决定「取到行之后怎么渲染」，不进规划器、不进限额、不进审计。
///
/// REST 与 MCP **共用这一个类型与这一条解析**（[`QueryFormat::parse`]）：两个协议
/// 各写一份「认哪些字符串」就会出现「REST 认、MCP 不认」这种静默分歧。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryFormat {
    /// 行回执（`columns` / `rows` / …）。REQ-122 起的行为，⛔ 一个字节都不改。
    Rows,
    /// JSON-LD 上下文子图（`@context` / `@graph` / `summary` / `meta`）。
    /// ⭐ REQ-142 · 协议的**默认**输出（R1：REST 与 MCP 都是）。
    #[default]
    JsonLd,
}

/// ⭐ REQ-142 · 三个入口（body / `?format=` / `Accept`）**都没给线索**时的默认格式。
///
/// 它就是 [`QueryFormat::default`] 的具名别名：默认值**只有一处**（枚举上的
/// `#[default]`），MCP 的 `format` 省略路径与 REST 的 `Accept` 兜底读的是同一个值
/// —— 于是「REST 默认图、MCP 默认表」这种分歧在结构上没有发生的位置（R1）。
pub const DEFAULT_FORMAT: QueryFormat = QueryFormat::JsonLd;

impl QueryFormat {
    /// 字符串 → 格式（REST 的 `?format=` 用它；MCP 走 serde）。
    ///
    /// ⛔ 不做前缀 / 大小写 / 近义词匹配：`?view=` 的口径是「拼错不静默回退」，
    /// 这里同样 —— `format=json-ld` 会 400 并列出两个合法值。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "rows" => Some(QueryFormat::Rows),
            "jsonld" => Some(QueryFormat::JsonLd),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            QueryFormat::Rows => "rows",
            QueryFormat::JsonLd => "jsonld",
        }
    }

    pub const fn is_jsonld(self) -> bool {
        matches!(self, QueryFormat::JsonLd)
    }
}

impl SparqlRequest {
    /// 从 `serde_json::Value` 解析（两个协议都走这里）。
    pub fn from_value(v: &serde_json::Value) -> Result<Self, ApiError> {
        let obj = v.as_object().ok_or_else(|| {
            ApiError::bad_request(
                "请求体不是 JSON 对象。",
                "body 形如 {\"query\": \"SELECT ?s WHERE { ?s a :customers }\", \"max_rows\": 100}。",
            )
        })?;
        let req: Self = serde_json::from_value(serde_json::Value::Object(obj.clone()))
            .map_err(|e| {
                ApiError::bad_request(
                    format!("请求体不合法：{e}"),
                    "可用字段只有 `query`（必填，字符串）、`max_rows`（可选，正整数）、\
                     `tenant`（可选，**只用于核对**，必须等于凭证绑定的租户）、\
                     `format`（可选，`jsonld`(默认) / `rows`：前者返回 JSON-LD 上下文子图，\
                     后者返回行回执表）。\
                     租户由凭证决定，正常调用不需要写 tenant。",
                )
            })?;
        if req.query.trim().is_empty() {
            return Err(ApiError::bad_request(
                "`query` 是空的。",
                "写一条 SPARQL SELECT，例如 `SELECT ?s WHERE { ?s a :customers }`；\
                 不确定有哪些类/属性时先 GET /api/catalog。",
            ));
        }
        Ok(req)
    }
}

/// `GET /api/catalog` 的 query / MCP `catalog` 的 arguments。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRequest {
    #[serde(default)]
    pub page: Option<usize>,
    #[serde(default)]
    pub page_size: Option<usize>,
}

impl CatalogRequest {
    pub fn from_value(v: &serde_json::Value) -> Result<Self, ApiError> {
        if v.is_null() {
            return Ok(Self::default());
        }
        let obj = v.as_object().ok_or_else(|| {
            ApiError::bad_request(
                "arguments 不是 JSON 对象。",
                "catalog 的参数形如 {\"page\": 1, \"page_size\": 50}。",
            )
        })?;
        serde_json::from_value(serde_json::Value::Object(obj.clone())).map_err(|e| {
            ApiError::bad_request(
                format!("catalog 参数不合法：{e}"),
                "可用参数只有 `page`（从 1 起）与 `page_size`（1–200，默认 50）。",
            )
        })
    }

    /// 校验 + 取默认值。
    pub fn normalized(&self) -> Result<(usize, usize), ApiError> {        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(catalog::DEFAULT_PAGE_SIZE);
        if page == 0 {
            return Err(ApiError::bad_request(
                "page 从 1 起，收到 0。",
                "第一页是 `page=1`；要更多就依次加一。`has_more=false` 表示已经到底。",
            ));
        }
        if page_size == 0 || page_size > catalog::MAX_PAGE_SIZE {
            return Err(ApiError::bad_request(
                format!("page_size 必须在 1–{} 之间，收到 {page_size}。", catalog::MAX_PAGE_SIZE),
                format!("用 `page_size={}`（默认）一次看 50 条；上限是 {MAX}。", catalog::DEFAULT_PAGE_SIZE, MAX = catalog::MAX_PAGE_SIZE),
            ));
        }
        Ok((page, page_size))
    }

    /// REST 的 query string → 同一个 DTO（⛔ 不认不认识的参数：拼错要报，不许静默）。
    pub fn from_query(
        q: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, ApiError> {
        let mut req = Self::default();
        for (k, v) in q {
            match k.as_str() {
                "page" => {
                    req.page = Some(v.trim().parse().map_err(|_| {
                        ApiError::bad_request(
                            format!("page 要写一个正整数，收到 {v:?}。"),
                            "第一页是 `page=1`；`has_more=false` 表示已经到底。",
                        )
                    })?)
                }
                "page_size" => {
                    req.page_size = Some(v.trim().parse().map_err(|_| {
                        ApiError::bad_request(
                            format!("page_size 要写一个正整数，收到 {v:?}。"),
                            "`page_size` 默认 50，上限 200。",
                        )
                    })?)
                }
                other @ ("token" | "access_token" | "api_key") => {
                    return Err(ApiError::bad_request(
                        format!("query string 里不要放 {other}。"),
                        "令牌只认 `Authorization: Bearer <token>` 头；\
                         放在 query string 里**不生效**，服务端也不会读取它（R2）。",
                    ))
                }
                other => {
                    return Err(ApiError::bad_request(
                        format!("不认识的查询参数 {other:?}。"),
                        "catalog 只接受 `page`（从 1 起）与 `page_size`（1–200）。",
                    ))
                }
            }
        }
        Ok(req)
    }
}

/// `GET /api/describe` 的 query / MCP `describe` 的 arguments。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribeRequest {
    #[serde(default)]
    pub name: Option<String>,
}

impl DescribeRequest {
    pub fn from_value(v: &serde_json::Value) -> Result<Self, ApiError> {
        if v.is_null() {
            return Ok(Self::default());
        }
        let obj = v.as_object().ok_or_else(|| {
            ApiError::bad_request(
                "arguments 不是 JSON 对象。",
                "describe 的参数形如 {\"name\": \":customers\"}。",
            )
        })?;
        serde_json::from_value(serde_json::Value::Object(obj.clone())).map_err(|e| {
            ApiError::bad_request(
                format!("describe 参数不合法：{e}"),
                "可用参数只有 `name`（类本地名如 `:customers`，或属性本地名如 `:customers__id`）。",
            )
        })
    }

    pub fn required_name(&self) -> Result<&str, ApiError> {
        match self.name.as_deref().map(str::trim) {
            Some(n) if !n.is_empty() => Ok(n),
            _ => Err(ApiError::bad_request(
                "describe 缺 name。",
                "带上要查的名字：`GET /api/describe?name=:customers`，\
                 属性写 `:customers__id`；不知道名字就先 GET /api/catalog。",
            )),
        }
    }

    /// REST 的 query string → 同一个 DTO（只认 `name`，拼错要报）。
    pub fn from_query(
        q: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, ApiError> {
        let mut req = Self::default();
        for (k, v) in q {
            match k.as_str() {
                "name" => req.name = Some(v.clone()),
                other @ ("token" | "access_token" | "api_key") => {
                    return Err(ApiError::bad_request(
                        format!("query string 里不要放 {other}。"),
                        "令牌只认 `Authorization: Bearer <token>` 头；\
                         放在 query string 里**不生效**，服务端也不会读取它（R2）。",
                    ))
                }
                other => {
                    return Err(ApiError::bad_request(
                        format!("不认识的查询参数 {other:?}。"),
                        "describe 只接受 `name`（类本地名如 `:customers`，或属性本地名如 `:customers__id`）。",
                    ))
                }
            }
        }
        Ok(req)
    }
}

/// MCP `sample` 的 arguments。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRequest {
    /// 类本地名（`:customers`）或属性本地名（`:customers__id`）。
    pub name: String,
    #[serde(default)]
    pub limit: Option<u64>,
}

impl SampleRequest {
    pub fn from_value(v: &serde_json::Value) -> Result<Self, ApiError> {
        let obj = v.as_object().ok_or_else(|| {
            ApiError::bad_request(
                "arguments 不是 JSON 对象。",
                "sample 的参数形如 {\"name\": \":customers__id\", \"limit\": 5}。",
            )
        })?;
        let req: Self = serde_json::from_value(serde_json::Value::Object(obj.clone())).map_err(|e| {
            ApiError::bad_request(
                format!("sample 参数不合法：{e}"),
                "可用参数只有 `name`（类/属性本地名）与 `limit`（1–token 上限，默认 10）。",
            )
        })?;
        if req.name.trim().is_empty() {
            return Err(ApiError::bad_request(
                "sample 缺 name。",
                "name 写类本地名（`:customers`）或属性本地名（`:customers__id`）。",
            ));
        }
        Ok(req)
    }
}

// ══════════════════════════════════════════════════════════════
// 回执（REQ-122 的九个字段 + REQ-123 的三项扩展）
// ══════════════════════════════════════════════════════════════

/// 一张表的**覆盖面**（`complete` / `main_only`），来自声明。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CoverageEntry {
    pub table: String,
    pub coverage: &'static str,
}

/// 一条 join 边的**证据等级**（`true` / `inherit` / `false`）与**本次是否用到**。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct JoinStateEntry {
    pub from: String,
    pub to: String,
    pub predicate: String,
    /// 这条边**本次查询用到了吗**（`false` = 涉及但未用，见 [`JOIN_STATES_SCOPE_NOTE`]）。
    ///
    /// 判据见 [`QueryService::used_join_predicates`]：该边的谓词（或它的反向谓词）
    /// 是否出现在本次查询里。
    pub used: bool,
    pub state: &'static str,
    /// 证据原文（`value-overlap 5/5 (100.00%) -> customers.id …`）。
    pub evidence: String,
}

/// `join_states[]` 的范围标签（**机器可读**）。⛔ 不要改成暗示「实走边」的名字或值。
///
/// 含义：`join_states[]` = 「查询涉及的表之间」的**全部**映射边。
pub const JOIN_STATES_SCOPE_ALL_EDGES: &str = "all_edges_between_involved_tables";

/// [`JOIN_STATES_SCOPE_ALL_EDGES`] 的**人话**版本（随回执一起返回）。
///
/// 存在的理由：MCP 的 Agent 只看回执，不看模块文档 —— 范围必须写在它看得见的地方。
pub const JOIN_STATES_SCOPE_NOTE: &str = "join_states[] 是「查询涉及的表之间」的全部映射边（来自声明/映射），\
不是本次查询实际走过的那几条；每条边的 used=false 表示「涉及但未用」，used=true 表示该边的谓词写进了查询。";

/// 本次实际生效的限额（调用方看得出「上限是谁压下来的」）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LimitInfo {
    /// 本次执行的 `max_rows`（= min(请求值, token 上限)）。
    pub max_rows: u64,
    /// 本凭证的 `max_rows` 上限（**调用方抬不高**的那条线）。
    pub token_max_rows: u64,
    pub timeout_ms: u64,
}

/// 查询回执。字段顺序即序列化顺序（REST 与 MCP 用**同一个**类型与序列化函数）。
#[derive(Debug, Clone, Serialize)]
pub struct SparqlReceipt {
    // ── REQ-122 的九个字段（顺序不动）──
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    pub row_count: usize,
    pub truncated: bool,
    pub sql_echo: String,
    pub tenant_applied: Vec<TenantApplied>,
    pub tables: Vec<String>,
    pub public_dimensions: Vec<PublicDimensionUsed>,
    pub warnings: Vec<String>,
    // ── REQ-123 的扩展 ──
    /// 每张表 → `complete` / `main_only`（公共维度记 `complete`，并在
    /// `public_dimensions[]` 里同时出现，两栏合看即可区分）。
    pub coverage: Vec<CoverageEntry>,
    /// 涉及的表之间的**全部**映射边（⛔ **不是**「本次查询走到的边」，见 `join_states_scope`）。
    pub join_states: Vec<JoinStateEntry>,
    /// `join_states[]` 的范围（恒为 [`JOIN_STATES_SCOPE_ALL_EDGES`]，不随查询变化）。
    pub join_states_scope: &'static str,
    /// 范围的**人话**说明（[`JOIN_STATES_SCOPE_NOTE`]；与上一个字段同义）。
    pub join_states_scope_note: &'static str,
    pub limits: LimitInfo,
    // ── ⭐ REQ-145 的扩展（**只有加载了 access 映射才有**）──
    /// 这份结果**完整吗**：`false` ⇒ 至少有一条 `@pending`（契约规则 1）。
    ///
    /// ⚠️ `None` = 这个部署没加载 `--access-mapping` ⇒ 字段不出现（R1：与基线逐字节相同）。
    /// `Some(true)` = 查询只落在库源（或全空结果）；`Some(false)` = 有 api 源没取。
    ///
    /// 不变式（AC-007）：`complete == unresolved_classes.is_empty()`，即
    /// **`complete == @pending.is_empty()`** —— 一个字段说了算，不靠调用方推断。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complete: Option<bool>,
    /// 本次**没算进去**的类（SPARQL 侧的本地名，升序去重）。
    ///
    /// 表挂不住 `@pending` 的明细（行没有图结构），所以只报「哪几类缺了」；
    /// 明细（`get` / `ids` / `ref`）只在 `format=jsonld` 的 `@pending` 里给。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved_classes: Option<Vec<String>>,
}

impl SparqlReceipt {
    /// **唯一**的序列化函数。
    ///
    /// ⛔ 两个协议都不许自己 `json!` 拼回执：REST 的响应体与 MCP 的
    /// `content[0].text` 都必须逐字节来自这里（AC-004 的判据就是 sha256 相等）。
    pub fn serialize(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|e| {
            // 回执里全是 String / Vec，理论上不会失败；真失败也不许 panic。
            format!("{{\"error\":\"回执序列化失败：{e}\"}}")
        })
    }
}

/// 谓词 IRI → 本地名（`…/ontok.local/anspire_r2rml_excl#licenses__customers` →
/// `licenses__customers`）。与 planner 的 `local_of` 同一个口径（`#` 优先，其次 `/`）。
fn predicate_local(iri: &str) -> &str {
    match iri.rfind('#') {
        Some(i) => &iri[i + 1..],
        None => iri.rsplit('/').next().unwrap_or(iri),
    }
}

// ══════════════════════════════════════════════════════════════
// 服务
// ══════════════════════════════════════════════════════════════

/// 服务状态（启动时一次性载入，运行期不再查元数据库）。
pub struct QueryService {
    /// ⭐ REQ-136 · 哪个模式（启动时定；`Option` 字段的解释权都在它手里）。
    pub mode: ServiceMode,
    /// **源库**只读连接池（REQ-122 的 `connect_read_only`）。
    ///
    /// `None` = 本体服务模式：**启动时就没连过**（不是连失败），运行期也没有任何分支会去连
    /// （[`run_sparql`] 在碰它之前就返回 503 —— 类型上 `Option` 让「忘了判」变成编译错误）。
    pub source: Option<PgPool>,
    /// 只读会话的**读回事实**（启动横幅打印它，⛔ 不写死）。
    ///
    /// 与 [`Self::source`] 同生共死：没有会话就没有可读回的事实，`None` 时横幅改印
    /// 「无数据源」而不是编一个 `on`。
    pub facts: Option<SessionFacts>,
    /// R2RML 映射（`None` = 本体服务模式：没有 schema 可对账，也就没有映射）。
    ///
    /// ⛔ 不是「映射建失败了」：本模式**刻意不建**（需求「不做自动造 schema、不猜 join」）。
    pub mapping: Option<crate::domain::r2rml::R2rmlMapping>,
    pub model: crate::domain::model_view::ModelTrackView,
    /// 真实列类型（本体模式恒为空 schema：`SchemaTypes::default()`，⛔ 不读文件、不猜类型）。
    pub schema: crate::domain::ttl::SchemaTypes,
    pub policy: QueryPolicy,
    pub ontology_iri: String,
    pub project: String,
    pub version: i32,
    /// 单次查询超时（毫秒；服务端 `statement_timeout` + 客户端兜底都按它）。
    pub timeout_ms: u64,
    /// SSE 空闲超时（毫秒）。
    pub sse_idle_timeout_ms: u64,
    /// 全局 SSE 连接上限。
    pub sse_max_connections: usize,
    /// 全局并发查询上限（应与源库连接池大小匹配）。
    pub max_concurrent_queries: usize,
    pub tokens: TokenTable,
    pub audit: AuditSink,
    /// ⭐ REQ-127 · 两视图的 TTL（启动时算好；版本冻结 ⇒ 内容不变）。
    pub ttl: crate::query::ttl::OntologyTtl,
    /// ⭐ REQ-135 · `/metrics` 的注册表（进程私有；测试各自一份 ⇒ 断言不互相干扰）。
    pub metrics: crate::metrics::Metrics,
    /// 目录条目（启动时拍平一次，翻页只做切片）。
    pub catalog_items: Vec<catalog::CatalogItem>,
    /// ⭐ REQ-145 · access 映射（`--access-mapping` 的产物）。
    ///
    /// `None` = 这个部署没加载映射 ⇒ **与基线逐字节相同**（R1）：没有 `access` 字段、
    /// 没有 `@pending`、没有 `complete`、没有 `mapping_version`；api 类照常按库源处理。
    /// 客户侧部署天然不加载它 ⇒ 也就拿不到 `get`（R6，不做额外身份裁剪）。
    pub access: Option<source_gate::AccessMapping>,
    limiter: Arc<Limiter>,
}

/// 构造参数（`serve` 与测试共用**这一个**构造器 —— 不另搭一份接线）。
pub struct ServiceParts {
    pub mode: ServiceMode,
    pub source: Option<PgPool>,
    pub facts: Option<SessionFacts>,
    pub mapping: Option<crate::domain::r2rml::R2rmlMapping>,
    pub model: crate::domain::model_view::ModelTrackView,
    pub schema: crate::domain::ttl::SchemaTypes,
    pub policy: QueryPolicy,
    pub ontology_iri: String,
    pub project: String,
    pub version: i32,
    pub timeout_ms: u64,
    pub sse_idle_timeout_ms: u64,
    pub sse_max_connections: usize,
    pub max_concurrent_queries: usize,
    pub tokens: TokenTable,
    pub audit: AuditSink,
    /// ⭐ REQ-127 · 两视图的 TTL（`query::ttl::OntologyTtl::prepare` 的产物）。
    pub ttl: crate::query::ttl::OntologyTtl,
    /// ⭐ REQ-145 · access 映射（可选；`None` = 没加载 `--access-mapping`）。
    pub access: Option<source_gate::AccessMapping>,
}

impl QueryService {
    pub fn new(p: ServiceParts) -> Self {
        // ⭐ REQ-136 · 目录的**唯一**构建点：把快照（model）与映射（可选）一起交出去，
        // 两个模式在 `catalog::build_items` 内部只在**过滤**处分叉（纪律 H）。
        // ⭐ REQ-145 · access 映射（可选）只**追加** `access` 字段 / 让 api 类也列出来；
        // `None` 时逐字节不变（R1）。
        let catalog_items = catalog::build_items(&p.model, p.mapping.as_ref(), p.access.as_ref());
        // ⭐ REQ-135 · 两个启动即确定的仪表：映射规模（joinCondition 数）与两视图字节数。
        // 它们随「版本冻结」而固定 —— 只在 `new` 里设一次（运行期不会变）。
        // ⭐ REQ-136 · 本体服务模式**没有映射** ⇒ `ontok_mapping_joins` 记 0（这是事实：
        // 那个部署不做对账，join 数不存在，⛔ 不是「0 条 join 的映射」）。
        let metrics = crate::metrics::Metrics::for_query();
        metrics.set_mapping_joins(p.mapping.as_ref().map_or(0, |m| m.join_count()) as u64);
        metrics.set_ttl_bytes(
            crate::metrics::VIEW_CUSTOMER,
            p.ttl.customer().bytes() as u64,
        );
        metrics.set_ttl_bytes(crate::metrics::VIEW_MODEL, p.ttl.model().bytes() as u64);

        Self {
            mode: p.mode,
            source: p.source,
            facts: p.facts,
            mapping: p.mapping,
            model: p.model,
            schema: p.schema,
            policy: p.policy,
            ontology_iri: p.ontology_iri,
            project: p.project,
            version: p.version,
            timeout_ms: p.timeout_ms,
            sse_idle_timeout_ms: p.sse_idle_timeout_ms,
            sse_max_connections: p.sse_max_connections,
            max_concurrent_queries: p.max_concurrent_queries,
            tokens: p.tokens,
            audit: p.audit,
            ttl: p.ttl,
            metrics,
            catalog_items,
            access: p.access,
            limiter: Arc::new(Limiter::default()),
        }
    }

    /// ⭐ REQ-151 · **业务查询唯一**能拿到源库连接池的入口。
    ///
    /// 为什么要有这个函数（而不是各处直接 `svc.source.as_ref()`）：REQ-136 的
    /// 「本体模式绝不碰源库」原来是靠「⓪ 步早返回」这个**顺序**保证的。REQ-151 把闸门
    /// 挪到源判定之后，顺序不再是保证 —— 于是把判据收进这里：**模式是唯一开关**，
    /// 本体模式恒 `None`，哪怕 `source` 字段里被误塞了一个池也拿不出来。
    ///
    /// 类型上仍然返回 `Option`：调用方拿到 `None` 只能走 503 / pending，
    /// ⛔ 没有「先 unwrap 再说」的位置。
    pub fn source_pool(&self) -> Option<&PgPool> {
        if self.mode.is_ontology_only() {
            return None;
        }
        self.source.as_ref()
    }

    /// 查 join 证据：**涉及的表之间**的**全部**映射边（范围见模块头，⛔ 不是「实走边」）。
    ///
    /// `used` 由 `used_predicates`（= [`Self::used_join_predicates`] 的结果）判定：
    /// 该边的谓词或其反向谓词在本次查询里出现过 ⇒ `true`，否则 `false`（涉及但未用）。
    pub fn join_states(&self, tables: &[String], used_predicates: &[String]) -> Vec<JoinStateEntry> {
        let mut out: Vec<JoinStateEntry> = Vec::new();
        // ⭐ REQ-136 · 本体模式没有映射 ⇒ 没有「映射边」这个概念（回执本身也不可达：SPARQL 503）。
        let Some(mapping) = self.mapping.as_ref() else {
            return out;
        };
        for t in tables {
            let Some(class) = mapping.class(t) else { continue };
            for j in &class.joins {
                if !tables.iter().any(|x| x == &j.child_table) {
                    continue;
                }
                if !tables.iter().any(|x| x == &j.parent_table) {
                    continue;
                }
                if out.iter().any(|e| {
                    e.from == j.child_table && e.to == j.parent_table && e.predicate == j.predicate_local
                }) {
                    continue;
                }
                let used = used_predicates
                    .iter()
                    .any(|p| p == &j.predicate_local || p == &j.inverse_local);
                out.push(JoinStateEntry {
                    from: j.child_table.clone(),
                    to: j.parent_table.clone(),
                    predicate: j.predicate_local.clone(),
                    used,
                    state: j.state.as_str(),
                    evidence: j.evidence.clone(),
                });
            }
        }
        out.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| a.predicate.cmp(&b.predicate))
        });
        out
    }

    /// 本次查询**写到的**关系谓词本地名（含反向写法）—— 只用来判 [`JoinStateEntry::used`]。
    ///
    /// 依据：planner 只为**查询里写出来的**关系谓词建 join（`connect_nodes` 只在已有的
    /// 边上选，⛔ 不造隐式笛卡尔积），所以「谓词出现在查询里」⇔「这条边被用到」。
    ///
    /// 走**再解析一遍**（[`crate::sparql::query::parse`]，与 planner 同一个解析器），
    /// ⛔ 不是对查询文本做子串匹配（那会把字面量里的同名字符串也算进来）。
    ///
    /// 解析失败（理论上不可能：同一条查询刚刚跑成功）⇒ 空集 ⇒ 全部 `used: false`：
    /// 保守方向 —— 宁可说「这条边没被用到」，也不虚报「用到了」。
    pub fn used_join_predicates(&self, sparql: &str) -> Vec<String> {
        let Ok(q) = crate::sparql::query::parse(sparql, &self.ontology_iri) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for pat in &q.patterns {
            let local = predicate_local(&pat.predicate);
            if local.is_empty() || local == "type" {
                continue;
            }
            if !out.iter().any(|x| x == local) {
                out.push(local.to_string());
            }
        }
        out
    }

    fn acquire_query(&self, entry: &TokenEntry) -> Result<Slot, ApiError> {
        self.limiter.acquire_query(
            &entry.digest,
            entry.max_concurrency,
            self.max_concurrent_queries,
        )
    }

    pub(crate) fn acquire_sse(&self, entry: &TokenEntry) -> Result<Slot, ApiError> {
        self.limiter.acquire_sse(
            &entry.digest,
            entry.max_concurrency,
            self.sse_max_connections,
            self.sse_idle_timeout_ms,
        )
    }

    /// 判据专用：按 token 原文占一个**查询**名额（模拟「一条查询正在跑」）。
    ///
    /// ⛔ 只编进测试：生产代码没有「不经过适配器就占名额」的路径。
    #[cfg(test)]
    pub(crate) fn occupy_query_slot(&self, token: &str) -> Slot {
        let entry = self.tokens.find(token).expect("夹具 token").clone();
        self.acquire_query(&entry).expect("夹具名额")
    }

    /// 判据专用：按 token 原文占一个 **SSE** 名额。
    #[cfg(test)]
    pub(crate) fn occupy_sse_slot(&self, token: &str) -> Slot {
        let entry = self.tokens.find(token).expect("夹具 token").clone();
        self.acquire_sse(&entry).expect("夹具名额")
    }
}

// ══════════════════════════════════════════════════════════════
// 限额（每 token + 全局）
// ══════════════════════════════════════════════════════════════

/// 进程内计数器。**每 token 上限**是产品口径；**全局上限**对齐源库连接池，
/// 免得调用排队排到 `acquire_timeout` 才报一个不可行动的「取不到连接」。
#[derive(Debug, Default)]
struct Limiter {
    inflight: Mutex<HashMap<String, usize>>,
    inflight_total: Mutex<usize>,
    sse: Mutex<HashMap<String, usize>>,
    sse_total: Mutex<usize>,
}

/// 一次占用的凭据。`Drop` 负责归还（panic 也不泄漏名额）。
#[derive(Debug)]
pub(crate) struct Slot {
    limiter: Arc<Limiter>,
    key: String,
    kind: SlotKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Query,
    Sse,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.limiter.release(&self.key, self.kind);
    }
}

/// 中毒的锁不该让服务停摆：计数器的语义是「尽力」，取回内部值继续。
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

impl Limiter {
    /// `self: &Arc<Self>`：`Slot` 必须拿到同一份计数的 `Arc`，
    /// 而 `&self` 拿不到外层的 `Arc` —— 让调用方直接传引用计数是最不容易写错的做法。
    fn acquire_query(
        self: &Arc<Self>,
        key: &str,
        token_limit: usize,
        global_limit: usize,
    ) -> Result<Slot, ApiError> {
        let mut total = lock(&self.inflight_total);
        if *total >= global_limit {
            return Err(ApiError::too_many_concurrent(token_limit, global_limit));
        }
        let mut per = lock(&self.inflight);
        let cur = per.get(key).copied().unwrap_or(0);
        if cur >= token_limit {
            return Err(ApiError::too_many_concurrent(token_limit, global_limit));
        }
        *total += 1;
        per.insert(key.to_string(), cur + 1);
        Ok(Slot {
            limiter: Arc::clone(self),
            key: key.to_string(),
            kind: SlotKind::Query,
        })
    }

    fn acquire_sse(
        self: &Arc<Self>,
        key: &str,
        token_limit: usize,
        global_limit: usize,
        idle_ms: u64,
    ) -> Result<Slot, ApiError> {
        let mut total = lock(&self.sse_total);
        if *total >= global_limit {
            return Err(ApiError::too_many_streams(token_limit, global_limit, idle_ms));
        }
        let mut per = lock(&self.sse);
        let cur = per.get(key).copied().unwrap_or(0);
        if cur >= token_limit {
            return Err(ApiError::too_many_streams(token_limit, global_limit, idle_ms));
        }
        *total += 1;
        per.insert(key.to_string(), cur + 1);
        Ok(Slot {
            limiter: Arc::clone(self),
            key: key.to_string(),
            kind: SlotKind::Sse,
        })
    }

    fn release(&self, key: &str, kind: SlotKind) {        match kind {
            SlotKind::Query => {
                let mut per = lock(&self.inflight);
                if let Some(v) = per.get_mut(key) {
                    *v = v.saturating_sub(1);
                    if *v == 0 {
                        per.remove(key);
                    }
                }
                let mut total = lock(&self.inflight_total);
                *total = total.saturating_sub(1);
            }
            SlotKind::Sse => {
                let mut per = lock(&self.sse);
                if let Some(v) = per.get_mut(key) {
                    *v = v.saturating_sub(1);
                    if *v == 0 {
                        per.remove(key);
                    }
                }
                let mut total = lock(&self.sse_total);
                *total = total.saturating_sub(1);
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 适配器（REST 与 MCP **都只调这里**）
// ══════════════════════════════════════════════════════════════

/// ⭐ REQ-140 · 一次查询的**完整产物** = 行回执 + 列的 RDF 身份（`var → 类 / 谓词 / xsd`）。
///
/// 为什么要多带一个 `projection`：JSON-LD 渲染需要「这一列是实体还是字面量、谓词叫什么」
/// 这三件在**规划期**才知道的事。它**不是**第二个回执 —— `receipt` 就是
/// [`run_sparql`] 返回的那一个（`format=rows` 的字节由它决定，⛔ 与这里无关）。
///
/// 两者同生共死：`projection` 只从 `Planned` 来（[`crate::sparql::run_query`]），
/// 所以「列名 → 谓词」永远与**真正执行的那条 SQL** 同源，⛔ 不从返回的列名反猜。
#[derive(Debug, Clone)]
pub struct SparqlOutcome {
    pub receipt: SparqlReceipt,
    pub projection: Vec<crate::sparql::ProjectedColumn>,
}

/// ⭐ REQ-145 · 一次查询的**源判定结果**（`format` 不参与这个决定）。
///
/// 为什么是一个枚举而不是「在回执里加一个字段」：`@pending` 是**顶层**的、与 `@graph`
/// 并列的另一种东西（契约 §九 第 1 点），它挂不到「一行一条」的行回执上。两种格式
/// 从**同一个**判定出发，各自表达同一件事：
///
/// | 判定 | `format=jsonld` | `format=rows` |
/// | --- | --- | --- |
/// | [`SparqlAnswer::Rows`] | `@graph` + `meta.complete: true` | 行回执 + `complete: true` |
/// | [`SparqlAnswer::Pending`] | `@graph: []` + `@pending` + `meta.complete: false` | **`NOT_DB_SOURCE`**（⛔ 不返回空表） |
#[derive(Debug, Clone)]
pub enum SparqlAnswer {
    /// 库源能算完：回执 + 列的 RDF 身份（JSON-LD 用）。
    Rows(SparqlOutcome),
    /// 只碰 api 源：**一行 SQL 都没执行**，产物是「半成品 + 说明书」。
    Pending(PendingAnswer),
}

/// `@pending` 的**服务端事实**（渲染成 JSON-LD 的形状在 [`jsonld`]）。
#[derive(Debug, Clone)]
pub struct PendingAnswer {
    /// 空回执（0 行 / 无列）—— 与行回执**同一个类型**，所以 `meta` 的形状不会漂。
    /// 它带 `complete: Some(false)` + `unresolved_classes: Some([...])`。
    pub receipt: SparqlReceipt,
    /// 一条一条待取（**不进 `@graph`**，契约规则 2）。
    pub pending: Vec<source_gate::ApiPending>,
    /// 加载的映射版本（`meta.mapping_version`）。
    pub mapping_version: u64,
}

impl PendingAnswer {
    /// `meta.unresolved_classes`：这次没算进去的类（本地名，升序去重）。
    pub fn unresolved_classes(&self) -> Vec<String> {
        self.receipt.unresolved_classes.clone().unwrap_or_default()
    }
}

/// 一条 SPARQL 查询（**唯一**入口：REST `POST /api/sparql` 与 MCP `sparql_query`）。
///
/// 顺序是刻意的（最便宜的失败先报，且**每个失败都留一条审计**）：
/// ⓪ 源判定 + 有没有数据源 → ① 租户比对 → ② `max_rows` 收口 → ③ 并发名额 →
/// ④ 规划 + 注入 + 执行 → ⑤ 回执。
///
/// ⭐ REQ-136 · 第 ⓪ 步是本体服务模式对**需要库**的查询的出口（503 `NO_DATA_SOURCE`）；
/// 它仍在**碰任何身份 / 限额 / 连接之前**返回，所以「该模式不许任何请求尝试连源库」是
/// 一个**结构事实**：`svc.source` 是 `Option`，而唯一能拿到池的入口
/// [`QueryService::source_pool`] 在模式上恒 `None`。
///
/// ⭐ REQ-151 · 但 ⓪ 不再「恒 503」：它先做源判定（纯函数，只读 AST + 映射），只拦
/// **需要库**的查询。纯 api 类的查询继续往下走，在 ③.5 用**同一份判定**产出 `@pending`
/// —— 与普通模式同一条渲染路径（R5）。
///
/// ⭐ REQ-140 · `format` **不在这里**判：它只影响渲染，[`run_sparql_ex`] 把
/// 「行回执 + 列的 RDF 身份」一起交出去，由 `http` 层按格式二选一序列化。
/// 于是「格式」不会渗进规划 / 注入 / 限额 / 审计的任何分支（AC-009 的结构保证）。
///
/// ⭐ REQ-145 · **行回执的契约是「要么完整、要么报错」**：只碰 api 源时这里直接给
/// `NOT_DB_SOURCE`（R5：⛔ 不返回 `rows: []` —— 空表会被读成「查了，没有数据」）。
/// 要 `@pending` 的明细走 [`run_sparql_ex`]（JSON-LD 那条路）。
pub async fn run_sparql(
    svc: &QueryService,
    caller: &Caller,
    req: SparqlRequest,
) -> Result<SparqlReceipt, ApiError> {
    // ⭐ REQ-145 · R-2：审计的 `code` 必须是**客户端实际收到的那个** —— 行回执这条路
    // 收到的是 `NOT_DB_SOURCE`，所以这里记 `NOT_DB_SOURCE`（⛔ 不是内部状态名）。
    // 「这是半个答案」由独立的 `pending=true` 表达；一次调用仍然只有**一行**审计。
    let started = Instant::now();
    // 审计要的是**查询哈希**（⛔ 不是原文）；借一份出来，事件函数会把 `req` 吃掉。
    let query = req.query.clone();
    match run_sparql_event(svc, caller, "sparql", req).await? {
        SparqlAnswer::Rows(out) => Ok(out.receipt),
        SparqlAnswer::Pending(p) => {
            let e = ApiError::not_db_source(&p.unresolved_classes());
            svc.metrics.note_denial_code(e.code);
            audit_pending(svc, "sparql", caller, Some(&query), started, e.code);
            Err(e)
        }
    }
}

/// ⭐ REQ-140 · 同 [`run_sparql`]，但把「列的 RDF 身份」也带出来（JSON-LD 用）。
///
/// 与 [`run_sparql`] **同一个实现**（前者就是它的 `.receipt`）—— 两个函数名只是
/// 「要不要 projection」的区别，⛔ 不存在第二条查询链路。
///
/// ⭐ REQ-145 · 区别还有两条：**这里能拿到 [`SparqlAnswer::Pending`]**（JSON-LD 挂得住
/// `@pending`），而行回执那条路必须报错；并且 pending 的审计记 `code=OK`
/// （客户端拿到的是 **200**）`+ pending=true`。
pub async fn run_sparql_ex(
    svc: &QueryService,
    caller: &Caller,
    req: SparqlRequest,
) -> Result<SparqlAnswer, ApiError> {
    let started = Instant::now();
    let query = req.query.clone();
    let answer = run_sparql_event(svc, caller, "sparql", req).await?;
    if let SparqlAnswer::Pending(_) = &answer {
        // 客户端收到 200（半成品 + 说明书）⇒ 审计必须写 `code=OK`。
        audit_pending(svc, "sparql", caller, Some(&query), started, "OK");
    }
    Ok(answer)
}

async fn run_sparql_event(
    svc: &QueryService,
    caller: &Caller,
    event: &'static str,
    req: SparqlRequest,
) -> Result<SparqlAnswer, ApiError> {
    let started = Instant::now();

    // ⓪ ⭐ REQ-151 · 本体服务模式的闸门（位置：**在源判定之后**，仍早于租户/限额/连接）。
    //
    // ⭐ REQ-136 的原始意图是「该模式不许任何请求碰源库」，把它落成「恒 503」是**过度拦截**：
    // 只碰 api 源的查询本来就不需要库，它要的是「你去哪取」（`@pending` + `get`）。
    // 于是这里先做**源判定**（纯函数：只读 AST + 映射，⛔ 不碰连接、不解析 SQL），只有
    // **需要库**的查询（`DbOnly` / `CrossSource` / 混合 `Api{needs_db:true}` / 没有映射 /
    // 解析失败）才保留原来的 503。判定结果留给 ③.5 —— 同一条查询**只解析一次**。
    //
    // 闸门仍放在 ① 之前：需要库的查询因此与 `9dfb8e7` 的响应**逐字节相同**（R2），
    // 不会先撞上 tenant / max_rows / 并发这些更晚出现的答案。
    let early_verdict: Option<Option<source_gate::SourceCheck>> = if svc.mode.is_ontology_only() {
        let check = svc
            .access
            .as_ref()
            .and_then(|access| source_check(svc, access, &req.query));
        // 只有「纯 api」放行；`needs_db` 为 true 或判不出来 ⇒ 这条查询要库。
        let pure_api = check
            .as_ref()
            .is_some_and(|v| matches!(v, source_gate::SourceCheck::Api { needs_db: false, .. }));
        if !pure_api {
            let e = ApiError::no_data_source();
            // 审计照样留一行（「谁在这个部署上试过查数据」是一个可数的事实）。
            audit_call(svc, event, caller, Some(&req.query), None, started, e.code, None);
            return Err(e);
        }
        Some(check)
    } else {
        None
    };

    // ① 租户只能来自 token：请求体里的 tenant 只**用来核对**。
    //
    // ⭐ REQ-137 · 走到这里租户一定 `Some`：本体模式里只有**纯 api** 查询能下来，
    // 而它们的 token 本身不带 tenant（`parse_for_mode` 强制缺省）；普通模式的 token
    // 解析阶段强制要求 tenant（缺失/空串都拒绝）。`unwrap_or_default()` 只是让类型闭合，
    // ⛔ 它不是「给本体模式编一个空租户」—— 那条路根本到不了这里。
    if let Some(claim) = req.tenant.as_deref() {
        let bound = caller.token.tenant.as_deref().unwrap_or_default();
        if claim.trim() != bound {
            let e = ApiError::tenant_mismatch(bound);
            // ⭐ REQ-135 · 越权信号（AC-004 的核心）：请求体带别人的租户 ⇒ 计数 +1。
            svc.metrics.note_denial_code(e.code);
            audit_call(svc, event, caller, Some(&req.query), None, started, e.code, None);
            return Err(e);
        }
    }

    // ② max_rows：token 上限是**硬上限**，调用方只能往下压。
    let mut warnings: Vec<String> = Vec::new();
    let effective = match req.max_rows {
        Some(0) => {
            let e = ApiError::bad_request(
                "max_rows 是 0。",
                "0 行等于「什么都没查」，与「查了没有」会混淆。要查就给 ≥1，\
                 或者干脆不写这个字段（默认用凭证上限）。",
            );
            audit_call(svc, event, caller, Some(&req.query), None, started, e.code, None);
            return Err(e);
        }
        Some(n) if n > caller.token.max_rows => {
            warnings.push(format!(
                "请求的 max_rows={n} 超过本凭证上限 {}，已按 {} 执行 —— \
                 凭证上限**不能**由调用方抬高（要更多请让运维调 tokens.json）",
                caller.token.max_rows, caller.token.max_rows
            ));
            caller.token.max_rows
        }
        Some(n) => n,
        None => caller.token.max_rows,
    };

    // ③ 并发名额（RAII：出错 / panic 都会归还）。
    let slot = match svc.acquire_query(&caller.token) {
        Ok(s) => s,
        Err(e) => {
            audit_call(svc, event, caller, Some(&req.query), None, started, e.code, None);
            return Err(e);
        }
    };

    // ③.5 ⭐ REQ-145 · **源判定**：这条查询会不会碰到 api 源？
    //
    // 位置在「拿名额之后、规划之前」：规划器会真的去解析类名 / 建 SQL，而一条只碰 api 源
    // 的查询**一行 SQL 都不该执行**（库里的那份副本不是事实源）。判据只读 AST + 映射
    // （⛔ 不做字符串匹配），见 `source_gate::AccessMapping::check`。
    //
    // ⭐ REQ-151 · 本体模式下这一步**已经做过**（⓪ 的闸门），这里直接复用同一个结果：
    // 一条查询只解析一次，闸门的放行条件与这里的分流条件不可能漂。
    let access = svc.access.as_ref();
    let verdict = match early_verdict {
        // 本体模式：⓪ 已经算过（同一个纯函数的结果）—— 不重复解析。
        Some(v) => v,
        None => access.and_then(|access| source_check(svc, access, &req.query)),
    };
    if let Some(check) = verdict {
        match check {
            source_gate::SourceCheck::DbOnly => {}
            source_gate::SourceCheck::CrossSource { predicate, from, to } => {
                let e = ApiError::cross_source_unsupported(&predicate, &from, &to);
                svc.metrics.note_denial_code(e.code);
                audit_call(svc, event, caller, Some(&req.query), None, started, e.code, None);
                drop(slot);
                return Err(e);
            }
            // ⭐ REQ-151 · 本体模式**只有纯 api** 能到这一支（⓪ 的闸门已把 `needs_db` 拦掉）。
            // `needs_db` 在普通模式不参与判定 —— REQ-145 的「混合 ⇒ 整体 pending」不变。
            source_gate::SourceCheck::Api { pending, .. } => {
                // ⚠️ R-2：**这里不写审计**。审计的 `code` 必须是客户端实际收到的那个，
                // 而「客户端收到什么」只有调用方知道（rows ⇒ 400 `NOT_DB_SOURCE`，
                // jsonld ⇒ 200 `OK`）—— 于是 `run_sparql` / `run_sparql_ex` 各自记一行
                // （都带 `pending=true`）。一次调用仍然只有**一行**审计。
                let mut classes: Vec<String> =
                    pending.iter().map(|p| p.class_local.clone()).collect();
                classes.sort();
                classes.dedup();
                let receipt = SparqlReceipt {
                    // 一行 SQL 都没执行 ⇒ 没有列、没有行、没有 SQL 可回声。
                    columns: Vec::new(),
                    rows: Vec::new(),
                    row_count: 0,
                    truncated: false,
                    sql_echo: String::new(),
                    tenant_applied: Vec::new(),
                    tables: Vec::new(),
                    public_dimensions: Vec::new(),
                    warnings,
                    coverage: Vec::new(),
                    join_states: Vec::new(),
                    join_states_scope: JOIN_STATES_SCOPE_ALL_EDGES,
                    join_states_scope_note: JOIN_STATES_SCOPE_NOTE,
                    limits: LimitInfo {
                        max_rows: effective,
                        token_max_rows: caller.token.max_rows,
                        timeout_ms: svc.timeout_ms,
                    },
                    // 契约规则 1：有 `@pending` ⇒ 必为 `false`；与 `unresolved_classes`
                    // 同源（同一个 `pending` 切片算出来的），满足 AC-007 的不变式。
                    complete: Some(false),
                    unresolved_classes: Some(classes),
                };
                drop(slot);
                // `pending` 只可能来自加载了映射的部署 ⇒ `access` 一定在（这里仍不 unwrap：
                // 缺了也只是 `mapping_version=0`，⛔ 不让一条只读查询 panic 成 500）。
                let mapping_version = access.map_or(0, |a| a.mapping_version());
                return Ok(SparqlAnswer::Pending(PendingAnswer {
                    receipt,
                    pending,
                    mapping_version,
                }));
            }
        }
    }

    // ④ 规划 + 注入 + 只读执行（REQ-122 的**同一个** `run_query`）。
    //
    // ⭐ REQ-151 · 源库连接池**只能**从 [`QueryService::source_pool`] 拿：它在本体模式恒
    // `None`（判据是 `mode`，⛔ 不是「调用点记得先看 ⓪」）。于是「本体模式绝不碰源库」
    // 不依赖闸门的位置 —— 即使将来有人改动了上面的顺序，这里也拿不到池子，只能答 503。
    // 上面 ⓪ 已经保证纯 api 查询走 pending；两个 `Option` 同时为 `None` 的状态只属于
    // 本体模式，真发生了也不 panic 成 500，而是按同一个 503 契约回答。
    let (Some(mapping), Some(source)) = (svc.mapping.as_ref(), svc.source_pool()) else {
        let e = ApiError::no_data_source();
        audit_call(svc, event, caller, Some(&req.query), None, started, e.code, None);
        return Err(e);
    };
    let input = PlanInput {
        mapping,
        schema: &svc.schema,
        ontology_iri: &svc.ontology_iri,
        policy: &svc.policy,
        tenant: caller.token.tenant.as_deref().unwrap_or_default(),
        max_rows: effective,
    };
    let result = crate::sparql::run_query(source, &input, &req.query, svc.timeout_ms).await;
    drop(slot);

    match result {
        Ok(r) => {
            let (receipt, projection) =
                receipt_of(svc, r, effective, &caller.token, warnings, &req.query);
            // ⭐ REQ-135 · 真的返回给调用方的行数（截断前；`truncated` 另有字段表达）。
            svc.metrics.rows(receipt.row_count as u64);
            audit_call(
                svc,
                event,
                caller,
                Some(&req.query),
                Some(receipt.row_count),
                started,
                "OK",
                None,
            );
            Ok(SparqlAnswer::Rows(SparqlOutcome { receipt, projection }))
        }
        Err(e) => {
            let err = ApiError::from_query_error(e);
            // ⭐ REQ-135 · `TENANT_UNRESOLVED`（表没登记 ⇒ fail-closed）也是越权信号之一：
            // 调用点拿不到「为什么」，所以按**错误码**在一个地方翻译（`metrics::denied_reason`）。
            svc.metrics.note_denial_code(err.code);
            audit_call(
                svc,
                event,
                caller,
                Some(&req.query),
                None,
                started,
                err.code,
                None,
            );
            Err(err)
        }
    }
}

/// ⭐ REQ-145 · 解析这条查询并按 access 映射判定源。
///
/// 解析失败 ⇒ `None`（不在这里报错）：语法错的查询交给**真正的**规划器去报，
/// 那样错误码与 hint 只有一处来源（`SPARQL_UNSUPPORTED` 的支持清单）。
fn source_check(
    svc: &QueryService,
    access: &source_gate::AccessMapping,
    sparql: &str,
) -> Option<source_gate::SourceCheck> {
    let q = crate::sparql::query::parse(sparql, &svc.ontology_iri).ok()?;
    Some(access.check(&q, &svc.ontology_iri, svc.mapping.as_ref()))
}

/// `QueryResult` → 回执（**唯一**的回执构造函数）。
///
/// ⛔ 不在这里改任何 REQ-122 字段的值；只追加 `coverage` / `join_states`（+ 范围标签）/
/// `limits` 与「上限被压」的 warning。
///
/// `sparql` 只用于判 `join_states[].used`（[`QueryService::used_join_predicates`]）——
/// 回执的范围口径不随它变化（恒为 [`JOIN_STATES_SCOPE_ALL_EDGES`]）。
///
/// ⭐ REQ-140 · 返回值多一个 `projection`（列的 `var → 类 / 谓词 / xsd`）：
/// 它**原样**来自 `QueryResult`（也就是 `Planned`），⛔ 不在这里重新推导 ——
/// 回执的字段与字节一个都没变，JSON-LD 只是多拿了这一份「列的 RDF 身份」。
fn receipt_of(
    svc: &QueryService,
    r: QueryResult,
    effective: u64,
    token: &TokenEntry,
    mut warnings: Vec<String>,
    sparql: &str,
) -> (SparqlReceipt, Vec<crate::sparql::ProjectedColumn>) {
    warnings.extend(r.warnings.iter().cloned());
    let coverage = r
        .tables
        .iter()
        .filter_map(|t| match svc.policy.decide(t) {
            TableDecision::Filter { coverage, .. } => Some(CoverageEntry {
                table: t.clone(),
                coverage: coverage.as_str(),
            }),
            // 公共维度：对所有租户可见 ⇒ 覆盖面完整；它与 `public_dimensions[]`
            // 里的同一条合看就能区分「没过滤」与「过滤得很干净」。
            TableDecision::Public { .. } => Some(CoverageEntry {
                table: t.clone(),
                coverage: "complete",
            }),
            // planner 已经对未登记表 fail-closed 过；这里只是防御性跳过。
            TableDecision::Unregistered => None,
        })
        .collect();
    let used_predicates = svc.used_join_predicates(sparql);
    let join_states = svc.join_states(&r.tables, &used_predicates);
    let receipt = SparqlReceipt {
        columns: r.columns,
        rows: r.rows,
        row_count: r.row_count,
        truncated: r.truncated,
        sql_echo: r.sql_echo,
        tenant_applied: r.tenant_applied,
        tables: r.tables,
        public_dimensions: r.public_dimensions,
        warnings,
        coverage,
        join_states,
        join_states_scope: JOIN_STATES_SCOPE_ALL_EDGES,
        join_states_scope_note: JOIN_STATES_SCOPE_NOTE,
        limits: LimitInfo {
            max_rows: effective,
            token_max_rows: token.max_rows,
            timeout_ms: svc.timeout_ms,
        },
        // ⭐ REQ-145 · 走到这里说明这条查询**只落在库源**（api 源在 ③.5 就分流了）
        // ⇒ 结果完整、没有未解析的类。`None` = 没加载映射 ⇒ 两个字段都不出现（R1）。
        complete: svc.access.as_ref().map(|_| true),
        unresolved_classes: svc.access.as_ref().map(|_| Vec::new()),
    };
    (receipt, r.projection)
}

/// `catalog`（REST `GET /api/catalog` 与 MCP `catalog`）。
pub fn run_catalog(
    svc: &QueryService,
    caller: &Caller,
    req: &CatalogRequest,
) -> Result<catalog::CatalogPage, ApiError> {
    let started = Instant::now();
    let (page, page_size) = match req.normalized() {
        Ok(v) => v,
        Err(e) => {
            audit_call(svc, "catalog", caller, None, None, started, e.code, None);
            return Err(e);
        }
    };
    let out = catalog::page(svc, page, page_size);
    audit_call(
        svc,
        "catalog",
        caller,
        None,
        Some(out.items.len()),
        started,
        "OK",
        None,
    );
    Ok(out)
}

/// `describe`（REST `GET /api/describe` 与 MCP `describe`）。
pub fn run_describe(
    svc: &QueryService,
    caller: &Caller,
    req: &DescribeRequest,
) -> Result<catalog::Description, ApiError> {
    let started = Instant::now();
    let name = match req.required_name() {
        Ok(n) => n,
        Err(e) => {
            audit_call(svc, "describe", caller, None, None, started, e.code, None);
            return Err(e);
        }
    };
    match catalog::describe(svc, name) {
        Ok(d) => {
            audit_call(svc, "describe", caller, None, Some(1), started, "OK", None);
            Ok(d)
        }
        Err(e) => {
            audit_call(svc, "describe", caller, None, None, started, e.code, None);
            Err(e)
        }
    }
}

/// `sample`（MCP `sample`）—— **不另开一条数据路径**：它把名字翻译成一条 SPARQL，
/// 再走 [`run_sparql_event`]（同一套注入 / 限额 / 审计）。
///
/// ⭐ REQ-136 · `sample` 的本质是**取数据**（它只是「由服务端代写一条 SELECT」），
/// 所以本体服务模式下它与 `sparql_query` 同一个 503 `NO_DATA_SOURCE`
/// （⛔ 不返回空样例：那会被读成「这个类没有实例」，而事实是「这个部署没有库」）。
/// 工具本身仍然出现在 `tools/list` 里（AC-008 的第一半判的是**清单**）。
pub async fn run_sample(
    svc: &QueryService,
    caller: &Caller,
    req: SampleRequest,
) -> Result<SparqlReceipt, ApiError> {
    if svc.mode.is_ontology_only() {
        let e = ApiError::no_data_source();
        audit_call(svc, "sample", caller, None, None, Instant::now(), e.code, None);
        return Err(e);
    }
    let limit = req.limit.unwrap_or(10);
    if limit == 0 {
        let e = ApiError::bad_request(
            "sample 的 limit 是 0。",
            "要取样就给 ≥1（默认 10，且不会超过本凭证的 max_rows）。",
        );
        audit_call(svc, "sample", caller, None, None, Instant::now(), e.code, None);
        return Err(e);
    }
    let query = match sample_query(svc, &req.name, limit) {
        Ok(q) => q,
        Err(e) => {
            audit_call(svc, "sample", caller, None, None, Instant::now(), e.code, None);
            return Err(e);
        }
    };
    // ⭐ REQ-145 · `sample` 也是**行回执**（表），所以对「只碰 api 源」的回答与
    // `format=rows` 一致：`NOT_DB_SOURCE`。取样取自库里的那份副本恰恰是最容易骗人的
    // （副本可能一周没更新）—— ⛔ 不给它开一条旁路。
    // ⚠️ R-2：`sample` 也是行回执 ⇒ 客户端收到 `NOT_DB_SOURCE`，审计就记 `NOT_DB_SOURCE`
    // （+ `pending=true`），与主查询那条路同一个口径。
    let started = Instant::now();
    let query_for_audit = query.clone();
    match run_sparql_event(
        svc,
        caller,
        "sample",
        SparqlRequest {
            query,
            max_rows: Some(limit.min(caller.token.max_rows)),
            tenant: None,
            // ⭐ REQ-140 · `sample` 恒是行回执（JSON-LD 只有 `sparql_query` 一个入口，
            // 需求表里也没有 `sample` 那一格）。
            format: Some(QueryFormat::Rows),
        },
    )
    .await?
    {
        SparqlAnswer::Rows(out) => Ok(out.receipt),
        SparqlAnswer::Pending(p) => {
            let e = ApiError::not_db_source(&p.unresolved_classes());
            svc.metrics.note_denial_code(e.code);
            audit_pending(svc, "sample", caller, Some(&query_for_audit), started, e.code);
            Err(e)
        }
    }
}

/// 名字 → 一条**安全的** SPARQL（先校验名字存在，⛔ 不把用户输入直接拼进查询）。
fn sample_query(svc: &QueryService, raw: &str, limit: u64) -> Result<String, ApiError> {
    let name = raw.trim();
    let local = name.strip_prefix(':').unwrap_or(name);
    if local.is_empty() {
        return Err(ApiError::bad_request(
            "sample 的 name 是空的。",
            "name 写类本地名（`:customers`）或属性本地名（`:customers__id`）。",
        ));
    }
    // ⭐ REQ-136 · 没有映射 ⇒ 没有可翻译的名字（本体模式下 `run_sample` 已经先 503 了，
    // 这里只是防御：⛔ 不 panic 成 500）。
    let Some(mapping) = svc.mapping.as_ref() else {
        return Err(ApiError::no_data_source());
    };
    // 名字必须能在映射里找到 —— 这既是「可行动」提示，也是拼接的安全边界
    // （本地名只含字母/数字/下划线，⛔ 不接受任意字符串）。
    if mapping.class_by_local(local).is_some() || mapping.class(local).is_some() {
        let class = mapping
            .class_by_local(local)
            .or_else(|| mapping.class(local))
            .expect("上面刚判过");
        return Ok(format!(
            "SELECT ?s WHERE {{ ?s a :{} }} LIMIT {}",
            class.class_local, limit
        ));
    }
    if let Some((_class_key, prop)) = find_property(svc, local) {
        return Ok(format!(
            "SELECT ?v WHERE {{ ?s :{} ?v }} LIMIT {}",
            prop.local, limit
        ));
    }
    Err(ApiError::unknown_name(name, Vec::new()))
}

/// 属性查找与 `catalog::describe` 同一口径（避免两处解析出不同答案）。
fn find_property<'a>(
    svc: &'a QueryService,
    local: &str,
) -> Option<(&'a str, &'a crate::domain::r2rml::MappedProperty)> {
    for c in svc.mapping.as_ref()?.classes() {
        if let Some(p) = c.property(local) {
            return Some((c.key.as_str(), p));
        }
    }
    None
}

// ══════════════════════════════════════════════════════════════
// 审计（每个出口都调**这一个**函数）
// ══════════════════════════════════════════════════════════════

/// 记一条审计。`rows = None` 表示失败/无行；`code` 是 `OK` 或错误码。
#[allow(clippy::too_many_arguments)]
pub fn audit_call(
    svc: &QueryService,
    event: &'static str,
    caller: &Caller,
    query: Option<&str>,
    rows: Option<usize>,
    started: Instant,
    code: &'static str,
    note: Option<&'static str>,
) {
    let rec = AuditRecord {
        ts: AuditRecord::now_ts(),
        event,
        proto: caller.protocol.as_str(),
        token_digest: Some(caller.token.audit_digest()),
        // ⭐ REQ-137 · 本体模式的凭证没有租户 ⇒ `None` ⇒ 审计写 `tenant=-`（R3）。
        tenant: caller.token.tenant.clone(),
        query_digest: query.map(|q| {
            format!(
                "sha256:{}",
                digest_prefix(&sha256_hex(q.as_bytes()), 16)
            )
        }),
        rows,
        elapsed_ms: started.elapsed().as_millis(),
        code,
        view: None,
        bytes: None,
        note,
        // ⭐ REQ-145 · 走这条函数的一定不是「半个答案」；那种调用走 `audit_pending`。
        pending: false,
    };
    svc.audit.record(&rec);
}

/// ⭐ REQ-145 · **「半个答案」那一条**（`@pending` 非空 / 行回执被拒）。
///
/// 与 [`audit_call`] 只差一个字段：`pending=true`。为什么单独一个函数而不是加一个
/// `bool` 参数：`audit_call` 有 8 个参数且全是位置参数，再塞一个布尔值等于让每个调用点
/// 都要记住「第 9 个是什么」；而「这次只返回一半」是**少数派**，给它一个名字更省事。
///
/// `rows` 恒 `Some(0)`：这一路**一行 SQL 都没执行**，0 是事实（不是「失败所以未知」）。
pub fn audit_pending(
    svc: &QueryService,
    event: &'static str,
    caller: &Caller,
    query: Option<&str>,
    started: Instant,
    code: &'static str,
) {
    let rec = AuditRecord {
        ts: AuditRecord::now_ts(),
        event,
        proto: caller.protocol.as_str(),
        token_digest: Some(caller.token.audit_digest()),
        tenant: caller.token.tenant.clone(),
        query_digest: query.map(|q| {
            format!(
                "sha256:{}",
                digest_prefix(&sha256_hex(q.as_bytes()), 16)
            )
        }),
        rows: Some(0),
        elapsed_ms: started.elapsed().as_millis(),
        // ⚠️ R-2：`code` 必须是**客户端实际收到的那个**（`OK` 或 `NOT_DB_SOURCE`），
        // ⛔ 不许在这里写 `API_PENDING` —— 「这是半个答案」由 `pending=true` 表达。
        code,
        view: None,
        bytes: None,
        note: None,
        pending: true,
    };
    svc.audit.record(&rec);
}

/// ⭐ REQ-127 · TTL 出口的审计（成功 / 304 / 400 都走这一条）。
///
/// 比 [`audit_call`] 多两个字段：`view`（customer / model）与 `bytes`（响应正文字节数）。
/// 401 走 [`audit_anonymous`]（没有身份可用，但有 `view`）。
pub fn audit_ttl(
    svc: &QueryService,
    caller: &Caller,
    view: crate::query::ttl::TtlView,
    bytes: Option<usize>,
    started: Instant,
    code: &'static str,
) {
    let rec = AuditRecord {
        ts: AuditRecord::now_ts(),
        event: "ttl",
        proto: caller.protocol.as_str(),
        token_digest: Some(caller.token.audit_digest()),
        // ⭐ REQ-137 · 同 [`audit_call`]：本体模式没有租户 ⇒ `tenant=-`。
        tenant: caller.token.tenant.clone(),
        query_digest: None,
        rows: None,
        elapsed_ms: started.elapsed().as_millis(),
        code,
        view: Some(view.as_str()),
        bytes,
        note: None,
        // ⭐ REQ-145 · 不是半个答案（那个事实由 audit_pending 写 pending=true）。
        pending: false,
    };
    svc.audit.record(&rec);
}

/// 没有身份时的那一条审计（`/api/health` 与鉴权失败）。
///
/// 字段里没有 token / tenant / query —— 它们**此刻不存在**。事件名仍然记录，
/// 于是「401 有多少次」是一个能数出来的事实，而不是一段沉默。
///
/// `view` 只有 TTL 端点会传（401/400 也记「你要的是哪个视图」）。
pub fn audit_anonymous(
    svc: &QueryService,
    event: &'static str,
    proto: Protocol,
    view: Option<&'static str>,
    code: &'static str,
    started: Instant,
) {
    let rec = AuditRecord {
        ts: AuditRecord::now_ts(),
        event,
        proto: proto.as_str(),
        token_digest: None,
        tenant: None,
        query_digest: None,
        rows: None,
        elapsed_ms: started.elapsed().as_millis(),
        code,
        view,
        bytes: None,
        note: None,
        // ⭐ REQ-145 · 不是半个答案（那个事实由 audit_pending 写 pending=true）。
        pending: false,
    };
    svc.audit.record(&rec);
}

// ══════════════════════════════════════════════════════════════
// 测试夹具（只编进测试；见 `catalog` 的判据也用这一份）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
pub(crate) fn test_fixture() -> (
    crate::domain::ttl::SchemaTypes,
    crate::domain::r2rml::R2rmlMapping,
    crate::domain::model_view::ModelTrackView,
) {
    use crate::domain::model_view::{
        ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView,
    };
    use crate::domain::snapshot::{Evidence, JoinState};
    use crate::domain::ttl::{SchemaTypes, TtlOptions};

    const SCHEMA: &str = r#"{"tables":[
      {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"customer_name","data_type":"text"},
                  {"name":"status","data_type":"text"}]},
      {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
       "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
                  {"name":"customer_id","data_type":"text"},
                  {"name":"amount","data_type":"numeric(12,4)"}]}
    ]}"#;

    let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
    let prop = |n: &str, c: &str, note: Option<&str>| ModelPropertyView {
        name: n.into(),
        column: c.into(),
        nullable: false,
        note: note.map(str::to_string),
        sample: None,
    };
    let entity = |k: &str, props: Vec<ModelPropertyView>| ModelEntityView {
        key: k.into(),
        display_name: k.into(),
        description: Some(format!("{k} 的业务说明")),
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
            entity(
                "customers",
                vec![
                    prop("客户名", "customer_name", None),
                    prop("状态(0-停用,1-正常)", "status", Some("状态(0-停用,1-正常)")),
                ],
            ),
            entity(
                "orders",
                vec![prop("金额", "amount", None), prop("客户", "customer_id", None)],
            ),
        ],
        link_types: vec![link],
        action_types: vec![],
        functions: vec![],
    };
    let mut opts = TtlOptions::new("demo", 2);
    opts.schema = Some(&schema);
    let mapping = crate::domain::r2rml::build_mapping(&view, &opts, false, Default::default())
        .expect("夹具映射");
    (schema, mapping, view)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `SparqlReceipt` 的序列化顺序 = 字段声明顺序（两个协议共用 ⇒ 逐字节相同的前提）。
    #[test]
    fn receipt_serialization_is_stable_and_ordered() {
        let receipt = SparqlReceipt {
            columns: vec!["s".into()],
            rows: vec![vec![Some("x".into())]],
            row_count: 1,
            truncated: false,
            sql_echo: "SELECT 1".into(),
            tenant_applied: vec![],
            tables: vec![],
            public_dimensions: vec![],
            warnings: vec![],
            coverage: vec![],
            join_states: vec![],
            join_states_scope: JOIN_STATES_SCOPE_ALL_EDGES,
            join_states_scope_note: JOIN_STATES_SCOPE_NOTE,
            limits: LimitInfo {
                max_rows: 10,
                token_max_rows: 10,
                timeout_ms: 1000,
            },
            // ⭐ REQ-145 · `None` = 没加载映射 ⇒ 这两个字段不进正文（R1 的字节判据靠它）。
            complete: None,
            unresolved_classes: None,
        };
        let a = receipt.serialize();
        let b = receipt.serialize();
        assert_eq!(a, b);
        // 顺序钉住：columns 在 rows 前面，warnings 在 coverage 前面，limits 最后。
        let pos = |k: &str| a.find(k).unwrap_or_else(|| panic!("缺 {k}: {a}"));
        assert!(pos("\"columns\"") < pos("\"rows\""));
        assert!(pos("\"warnings\"") < pos("\"coverage\""));
        assert!(pos("\"coverage\"") < pos("\"join_states\""));
        assert!(pos("\"join_states\"") < pos("\"join_states_scope\""));
        assert!(pos("\"join_states_scope\"") < pos("\"join_states_scope_note\""));
        assert!(pos("\"join_states_scope_note\"") < pos("\"limits\""));
        assert!(a.trim_end().ends_with('}'));
        // 范围标签本身：机器读值 + 人话说明都在回执里（⛔ 不是只在文档里）。
        let v: serde_json::Value = serde_json::from_str(&a).expect("回执是 JSON");
        assert_eq!(v["join_states_scope"], "all_edges_between_involved_tables");
        assert!(v["join_states_scope_note"]
            .as_str()
            .unwrap()
            .contains("全部映射边"));
    }

    /// B1：`join_states[]` 的范围**必须**在回执里标清 —— 三表查询里出现的
    /// 「涉及但未用」的边要能被认出来（`used: false`），而不是被读成「本次走到的边」。
    #[test]
    fn the_scope_label_is_not_the_traversed_edges() {
        // 值本身是判据（BA 裁定 J-2 的原文），改字即红。
        assert_eq!(
            JOIN_STATES_SCOPE_ALL_EDGES,
            "all_edges_between_involved_tables"
        );
        // 人话说明必须明说「不是本次实际走过的那几条」——否则只加字段不解释。
        assert!(
            JOIN_STATES_SCOPE_NOTE.contains("不是本次查询实际走过的那几条"),
            "{JOIN_STATES_SCOPE_NOTE}"
        );
        assert!(
            JOIN_STATES_SCOPE_NOTE.contains("used=false"),
            "{JOIN_STATES_SCOPE_NOTE}"
        );
        // 谓词 IRI → 本地名（`used` 的判定靠它）。
        assert_eq!(
            predicate_local("https://ontok.local/anspire_r2rml_excl#licenses__customers"),
            "licenses__customers"
        );
        assert_eq!(predicate_local("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"), "type");
    }

    /// 请求解析：`tenant` 是唯一允许的「额外」字段，拼错字段名要报 400 而不是忽略。
    #[test]
    fn sparql_request_parsing_is_strict_but_accepts_the_tenant_field() {
        let ok = SparqlRequest::from_value(&serde_json::json!({
            "query": "SELECT ?s WHERE { ?s a :customers }",
            "max_rows": 5,
            "tenant": "C0001"
        }))
        .expect("合法 body");
        assert_eq!(ok.max_rows, Some(5));
        assert_eq!(ok.tenant.as_deref(), Some("C0001"));

        let e = SparqlRequest::from_value(&serde_json::json!({"query": "SELECT ?s WHERE { ?s ?p ?o }", "max_row": 5}))
            .unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
        assert!(e.message.contains("max_row"), "{}", e.message);

        let e = SparqlRequest::from_value(&serde_json::json!({"query": "   "})).unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
        assert!(e.hint.contains("SELECT"), "{}", e.hint);

        let e = SparqlRequest::from_value(&serde_json::json!("not an object")).unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
    }

    /// ⭐ REQ-140 / REQ-142 · `format` 字段：省略 = **jsonld**（REQ-142 翻转）；
    /// `rows` 显式要行回执；拼错 **400**（⛔ 不回退）。
    ///
    /// MCP 的 `sparql_query` 与 REST 的 body 走**同一个** `from_value`，所以这一条
    /// 同时钉住「MCP 入口认哪些值」。
    #[test]
    fn the_format_field_is_rows_by_default_and_rejects_typos() {
        let base = serde_json::json!({"query": "SELECT ?s WHERE { ?s a :customers }"});

        let none = SparqlRequest::from_value(&base).expect("不写 format");
        assert_eq!(
            none.format, None,
            "不写就是 None（下游 unwrap_or_default ⇒ jsonld，REQ-142）"
        );
        // ⭐ REQ-142 · 默认值只有一处：枚举上的 `#[default]`。它就是 MCP 省略 format 时
        // 的实际取值（`unwrap_or_default`），也是 REST `Accept` 兜底的取值。
        assert_eq!(QueryFormat::default(), QueryFormat::JsonLd);
        assert_eq!(DEFAULT_FORMAT, QueryFormat::JsonLd);
        assert!(none.format.unwrap_or_default().is_jsonld());

        let rows = SparqlRequest::from_value(
            &serde_json::json!({"query": "SELECT ?s WHERE { ?s a :customers }", "format": "rows"}),
        )
        .expect("format=rows");
        assert_eq!(rows.format, Some(QueryFormat::Rows));

        let jsonld = SparqlRequest::from_value(
            &serde_json::json!({"query": "SELECT ?s WHERE { ?s a :customers }", "format": "jsonld"}),
        )
        .expect("format=jsonld");
        assert_eq!(jsonld.format, Some(QueryFormat::JsonLd));
        assert!(jsonld.format.unwrap().is_jsonld());
        assert_eq!(QueryFormat::JsonLd.as_str(), "jsonld");

        // ⛔ 近义词 / 大小写 / 拼错都不认（与 `?view=` 同一条口径）
        for bad in ["json-ld", "JSONLD", "json", "graph", ""] {
            let e = SparqlRequest::from_value(&serde_json::json!({
                "query": "SELECT ?s WHERE { ?s a :customers }",
                "format": bad
            }))
            .err()
            .unwrap_or_else(|| panic!("format={bad:?} 不该被接受"));
            assert_eq!(e.code, "BAD_REQUEST", "format={bad:?}");
            assert!(e.hint.contains("jsonld"), "hint 要列出合法值：{}", e.hint);
        }
        assert_eq!(QueryFormat::parse("  jsonld  "), Some(QueryFormat::JsonLd));
        assert_eq!(QueryFormat::parse("json-ld"), None);
    }

    #[test]
    fn catalog_and_describe_params_are_validated() {
        let (p, s) = CatalogRequest::from_value(&serde_json::json!({"page": 2, "page_size": 10}))
            .unwrap()
            .normalized()
            .unwrap();
        assert_eq!((p, s), (2, 10));

        let e = CatalogRequest::from_value(&serde_json::json!({"page": 0}))
            .unwrap()
            .normalized()
            .unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");

        let e = CatalogRequest::from_value(&serde_json::json!({"page_size": 201}))
            .unwrap()
            .normalized()
            .unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");

        let e = DescribeRequest::from_value(&serde_json::json!({}))
            .unwrap()
            .required_name()
            .unwrap_err();
        assert_eq!(e.code, "BAD_REQUEST");
        assert!(e.hint.contains("/api/describe"), "{}", e.hint);
    }

    /// 回执扩展：`coverage` 与 `join_states` 都来自**声明 / 映射**，不是编的。
    #[tokio::test]
    async fn receipt_extension_reads_the_declaration_and_the_mapping() {
        use crate::sparql::policy::QueryPolicy;
        let (schema, mapping, model) = test_fixture();
        let policy = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id","coverage":"main_only",
                "coverageNote":"只覆盖主账号","reason":"子账号数据不在主账号结果里"}},
                "publicDimensions":[]}"#,
        )
        .expect("声明");
        // IRI 基要先算出来（`opts` 借了 `schema`，之后 `schema` 要 move 进服务）。
        let ontology_iri = {
            let mut opts = crate::domain::ttl::TtlOptions::new("demo", 2);
            opts.schema = Some(&schema);
            opts.ontology_iri()
        };
        let svc = QueryService::new(ServiceParts {
            mode: ServiceMode::Query,
            // 懒连接：本测试**不连库**（只看回执扩展的构造）。
            source: Some(
                sqlx::postgres::PgPoolOptions::new()
                    .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
                    .expect("lazy pool"),
            ),
            facts: Some(SessionFacts {
                transaction_read_only: "on".into(),
                standard_conforming_strings: "on".into(),
            }),
            mapping: Some(mapping),
            model,
            schema,
            policy,
            ontology_iri,
            project: "demo".into(),
            version: 2,
            timeout_ms: 1000,
            sse_idle_timeout_ms: 1000,
            sse_max_connections: 1,
            max_concurrent_queries: 1,
            tokens: TokenTable::default(),
            audit: AuditSink::stdout_only(),
            ttl: crate::query::ttl::fixture(),
            access: None,
        });

        // 直接构造一个 QueryResult（等价于 run_query 的成功返回）来验证回执扩展。
        let r = QueryResult {
            columns: vec!["n".into()],
            rows: vec![vec![Some("A".into())]],
            row_count: 1,
            truncated: false,
            sql_echo: "SELECT 1".into(),
            tenant_applied: vec![],
            tables: vec!["orders".into(), "customers".into()],
            public_dimensions: vec![],
            warnings: vec![],
            // ⭐ REQ-140 · 列的 RDF 身份（本判据只看回执，给空集即可）。
            projection: vec![],
        };
        let token = TokenEntry {
            token: "x".repeat(24),
            digest: "d".repeat(64),
            tenant: Some("C0001".into()),
            max_rows: 10,
            max_concurrency: 1,
            label: None,
        };
        let receipt = receipt_of(&svc, r, 10, &token, vec![], "").0;
        assert_eq!(
            receipt.coverage,
            vec![
                CoverageEntry { table: "orders".into(), coverage: "main_only" },
                CoverageEntry { table: "customers".into(), coverage: "complete" },
            ]
        );
        assert_eq!(receipt.join_states.len(), 1, "{:?}", receipt.join_states);
        assert_eq!(receipt.join_states[0].from, "orders");
        assert_eq!(receipt.join_states[0].to, "customers");
        assert_eq!(receipt.join_states[0].state, "true");
        assert!(receipt.join_states[0].evidence.contains("value-overlap"));
        // 空查询（= 什么都没写）⇒ 「涉及但未用」，而不是含糊的默认。
        assert!(!receipt.join_states[0].used);
        // 范围标签恒在（⛔ 不随查询/结果变化）。
        assert_eq!(receipt.join_states_scope, JOIN_STATES_SCOPE_ALL_EDGES);
        assert_eq!(receipt.join_states_scope_note, JOIN_STATES_SCOPE_NOTE);
        assert_eq!(receipt.limits.max_rows, 10);
        assert_eq!(receipt.limits.token_max_rows, 10);

        // 同一条边，查询里**真的写了**这个谓词 ⇒ `used: true`（两种方向都认）。
        let join = svc
            .mapping
            .as_ref()
            .expect("夹具映射")
            .class("orders")
            .expect("夹具订单类")
            .joins
            .first()
            .expect("夹具 join")
            .clone();
        for pred in [&join.predicate_local, &join.inverse_local] {
            let r = QueryResult {
                columns: vec!["n".into()],
                rows: vec![],
                row_count: 0,
                truncated: false,
                sql_echo: "SELECT 1".into(),
                tenant_applied: vec![],
                tables: vec!["orders".into(), "customers".into()],
                public_dimensions: vec![],
                warnings: vec![],
                projection: vec![],
            };
            let q = format!("SELECT ?a ?b WHERE {{ ?a :{pred} ?b }}");
            let receipt = receipt_of(&svc, r, 10, &token, vec![], &q).0;
            assert_eq!(receipt.join_states.len(), 1, "{q}: {:?}", receipt.join_states);
            assert!(receipt.join_states[0].used, "写了 :{pred} 就是用过：{q}");
        }
    }

    /// `sample` 只把**存在**的名字翻成查询；不存在 ⇒ UNKNOWN_NAME（不拼任意字符串）。
    #[tokio::test]
    async fn sample_only_builds_queries_for_known_names() {
        use crate::sparql::policy::QueryPolicy;
        let (schema, mapping, model) = test_fixture();
        let policy = QueryPolicy::parse(
            r#"{"version":"1.0.0","tenantSubject":"customers","tenantIdColumn":"id",
                "perTable":{"orders":{"tenantColumn":"customer_id"}},"publicDimensions":[]}"#,
        )
        .expect("声明");
        let ontology_iri = {
            let mut opts = crate::domain::ttl::TtlOptions::new("demo", 2);
            opts.schema = Some(&schema);
            opts.ontology_iri()
        };
        let svc = QueryService::new(ServiceParts {
            mode: ServiceMode::Query,
            source: Some(
                sqlx::postgres::PgPoolOptions::new()
                    .connect_lazy("postgresql://nobody@127.0.0.1:1/none")
                    .expect("lazy pool"),
            ),
            facts: Some(SessionFacts {
                transaction_read_only: "on".into(),
                standard_conforming_strings: "on".into(),
            }),
            mapping: Some(mapping),
            model,
            schema,
            policy,
            ontology_iri,
            project: "demo".into(),
            version: 2,
            timeout_ms: 1000,
            sse_idle_timeout_ms: 1000,
            sse_max_connections: 1,
            max_concurrent_queries: 1,
            tokens: TokenTable::default(),
            audit: AuditSink::stdout_only(),
            ttl: crate::query::ttl::fixture(),
            access: None,
        });

        let q = sample_query(&svc, ":customers", 5).expect("已知类");
        assert_eq!(q, "SELECT ?s WHERE { ?s a :customers } LIMIT 5");
        let q = sample_query(&svc, ":orders__amount", 3).expect("已知属性");
        assert_eq!(q, "SELECT ?v WHERE { ?s :orders__amount ?v } LIMIT 3");
        let e = sample_query(&svc, ":evil } INSERT { ?s ?p ?o", 3).unwrap_err();
        assert_eq!(e.code, "UNKNOWN_NAME");
    }

    /// 限额：并发占用满 ⇒ 429；`Drop` 归还后又能拿到（不泄漏名额）。
    #[test]
    fn the_limiter_counts_per_token_and_returns_slots_on_drop() {
        let limiter = Arc::new(Limiter::default());
        let a = limiter.acquire_query("tokenA", 1, 2).expect("第一个名额");
        let e = limiter.acquire_query("tokenA", 1, 2).unwrap_err();
        assert_eq!(e.code, "TOO_MANY_CONCURRENT");
        assert!(!e.hint.trim().is_empty());
        // 另一个 token 不受影响
        let b = limiter.acquire_query("tokenB", 1, 2).expect("另一个 token 的名额");
        // 全局名额用完
        let e = limiter.acquire_query("tokenC", 1, 2).unwrap_err();
        assert_eq!(e.code, "TOO_MANY_CONCURRENT");
        drop(a);
        let c = limiter.acquire_query("tokenC", 1, 2).expect("归还后可以再拿");
        drop((b, c));
        // SSE 与查询是两套计数（互不占名额）
        let s1 = limiter.acquire_sse("tokenA", 1, 1, 1000).expect("SSE 名额");
        assert_eq!(
            limiter.acquire_sse("tokenA", 1, 1, 1000).unwrap_err().code,
            "TOO_MANY_STREAMS"
        );
        drop(s1);
        let _ = limiter.acquire_query("tokenA", 1, 1).expect("查询名额仍可用");
    }
}
