//! ⭐ REQ-144 · **access mapping 契约（v1）** —— 「这些东西去哪取、怎么取」的可执行描述。
//!
//! ```text
//! 本体（TTL）            ← 世界由什么构成（类 / 属性 / 关系）         ← REQ-110 / 119
//! access mapping（本模块）← 每个类：哪个 API、哪些参数、响应哪一段、能不能过滤
//! 执行器（`ontok access`）← 按映射取数，返回与 `query serve` 同形状的 JSON-LD
//! ```
//!
//! 依据：`BA/design/2026-10-25-access-mapping-contract.md`（草案 v1）+
//! `BA/demands/REQ-144/demand.md` §三 设计修订（双形态 JSON / JSON-LD）。
//!
//! # 本模块的边界（阶段 0 只读文件；阶段 1 增加**显式开启**的真调用）
//!
//! | 做 | 不做 |
//! | --- | --- |
//! | 契约结构 + 解析（作者格式 JSON） | ⛔ 跨源 join（v1 边界） |
//! | `validate` 的四条有牙检查（+ 结构与本体自洽） | ⛔ 任何 `query serve` 改动（REQ-145） |
//! | `fetch --dry-run-fixture`（分页 / 本地过滤 / JSON-LD / meta） | ⛔ **默认**发网络请求（要 `--allow-network`） |
//! | `publish`：JSON → JSON-LD（RML 风格，供 SPARQL 查） | ⛔ GET 签名 / 非 JSON 请求体 |
//! | `fetch --allow-network`（[`net`] + [`tc3`]）：TC3 签名 → HTTP → 错误分类 | |
//!
//! # 双形态（demand §三 R-1）
//!
//! ```text
//! access.json      ← 作者格式：人写、人审、好 diff（本模块的**唯一输入**）
//! access.jsonld    ← 发布格式：RML 风格，给执行器与 Agent（`publish` 产出）
//! ```
//!
//! # 三层结构 + 每条字段的「为什么在」
//!
//! | 层 | 字段 | 为什么必须显式 |
//! | --- | --- | --- |
//! | `sources[]` | `id` / `kind` / `base_url` / `auth.mode` / `auth.credential_env` | 契约会随本体分发 ⇒ **⛔ 不含任何真凭据**，只写环境变量名 |
//! | `entities[]` | `id` / `class` / `source` / `operation` / `iterator` / `identity` / `iri_template` / `fields` / `request` / `paging` / `capability` / `freshness` / `reviewed_by` / **`fixture`（可选）** | 缺一样，执行器就得**猜**；猜出来的东西没法审（`fixture` 例外：没有它只是「没校准」，不是「能猜」） |
//! | `relations[]` | `predicate` / `from` / `to` / `via` / `cross_source` / `traversable` | 跨源边要能被标记（demand §三 R-4） |
//!
//! # `request` 是**模板**，不是「参数表」（阶段 1 复审后的裁定）
//!
//! ```json
//! "request": {
//!   "template": { "Region": "{region}",
//!                 "Clauses": [ { "Field": "zone", "Values": ["{zone}"] } ] },
//!   "bindings": { "region": { "source": "default" },
//!                 "zone":   { "source": "filter" } },
//!   "paging":   { "offset": "Offset", "limit": "Limit",
//!                 "value_type": "string", "offset_base": 0 }
//! }
//! ```
//!
//! 分页的**类型**（`value_type`：`number` / `string`）、**起点**（`offset_base`：`0` / `1`，
//! 或更清楚的 `"page": "PageNumber"`）、绑定的**值类型 / 形状**
//! （`value_type`：`string` / `integer` / `boolean`；`shape`：`scalar` / `array`）
//! 都是 REQ-149 加的通用表达力 —— **全可选**，不写时行为与旧版逐字节相同。
//!
//! 三种真实存在的请求体形状（顶层标量 / 数组里放对象 / 深层嵌套）**一种代码路径**全覆盖；
//! 执行器只替换字符串值里的 `{name}`，⛔ 不改键名、⛔ 不按厂商或接口名分支。
//! 缺值（`--filter` 没给）按「剪枝」处理（见 [`RequestSpec`] 的文档），⛔ 不发空值。
//! 旧写法 `request.from_filter: {"zone":"Zone"}` 保留兼容，解析时**内部转成模板**。
//!
//! # 十条硬规则落在哪（设计稿 §三，逐条给出代码位置）
//!
//! | # | 规则 | 落点 |
//! | --- | --- | --- |
//! | 1 | ⛔ 不含真凭据 | [`Auth::credential_env`] 只收环境变量名；`validate` 不检查值（它不会贬值） |
//! | 2 | `iri_template` 与 TTL 的 `ontok:instanceIri` 一致 | [`validate::check_iri_templates`]（R3） |
//! | 3 | `pushdown` / `local_only` **必须显式** | [`parse`] 里两者都是必填（空数组也要写） |
//! | 4 | 超出 `pushdown` ⇒ 本地过滤且回执标 `filtered_locally` | [`fetch::dry_run`]（`meta.filtered_locally`） |
//! | 5 | 超 `max_scan_rows` ⇒ 拒绝，⛔ 不许偷偷全量 | [`fetch::dry_run`] 的**取页前**预算（`SCAN_LIMIT_EXCEEDED`） |
//! | 6 | `freshness` 必填且回执要带 | [`parse`] + `meta.freshness` |
//! | 7 | `relations.via.join` 只允许同一 source 内 | [`validate::check_relations`]（跨源 join ⇒ 报错） |
//! | 8 | 未签 entity 不许取数 | [`fetch::dry_run`] 的第一道闸（`UNREVIEWED_MAPPING`） |
//! | 9 | `fixture` **可选**：有它 ⇒ `--dry-run` 必须能跑；没有 ⇒ `validate` 只给 `FIELD_PATH_NOT_CHECKED`（⛔ 不假通过），`--dry-run-fixture` 明确拒绝 | [`fetch`] / [`validate`]（REQ-149 · D） |
//! | 10 | 契约版本只增不减 | [`CONTRACT_VERSION`] + [`parse`] 的版本闸 |
//!
//! # 关于 `identity` 的两种写法
//!
//! 设计稿 v1 的草案把 `select` / `key_path` / `iri_template` 收在 `identity` 里，
//! 而交互契约（`2026-10-25-interaction-contract.md` §3.3）把 `iterator` / `iri_template`
//! 放在 `entity` 顶层。两张纸都对，**同一件事不许有两种写法**才是纪律 ——
//! 所以本实现：
//!
//! - **规范写法（发布态 / 文档 / 示例都用它）**：`iterator` + `iri_template` 在
//!   entity 顶层，`identity.key_path` 只留「哪一列是键」；
//! - **兼容读取（只读，不参与发布）**：若顶层缺 `iterator` 而 `identity.select` 在，
//!   或缺 `iri_template` 而 `identity.iri_template` 在，则回退到嵌套写法；
//!   同理 `paging` 缺省时回退 `request.paging`。
//!
//! 回退是**只增不减**的：老文件继续能读，新文件必须按规范写法写（回退命中会在
//! `warnings` 里点名，⛔ 不静默）。
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - TTL 解析器只认本仓序列化器（`domain::ttl`）的输出形状：`<subject> a owl:Class ;`
//!   + 每行 `<pred> <obj> ;` + 以 `.` 收尾的 Turtle 子集。⛔ **不是通用 Turtle 解析器**
//!   （不认空白节点集合 / `a, b` 对象列表 / 嵌套 `[]`）。
//! - `capability` 只能查**自洽**（重叠 / 显式 / 预算），查不了「腾讯云是不是真支持这个参数」
//!   —— 那需要人签（R6 就是为此存在的）。
//! - 默认路径**不发任何网络请求**：`fetch` 要么 `--dry-run-fixture`（读夹具），
//!   要么 `--allow-network`（真调用）。两个都不给 ⇒ 明确拒绝（`DRY_RUN_REQUIRED`），
//!   ⛔ 不静默降级成「读 fixture 冒充真数据」。
//! - 阶段 1 的**真调用通路没有做过任何真实 API 调用**：判据来自「腾讯云官方签名向量 +
//!   本地假端点」。已经能证明的是「签名对、分页对、错误分类对」，
//!   **不能**证明「某个 customer 的真接口接受这份映射」（那需要只读凭据 + 真响应）。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

pub mod fetch;
pub mod net;
pub mod path;
pub mod publish;
pub mod tc3;
pub mod validate;

/// 契约**基线**版本（v1）。破坏性变更 ⇒ +1（设计稿规则 10）。
///
/// v1 的形状：`operation` 只有 `action` / `version` / `method` / `content_type`，
/// `auth` 只有 `mode` + `credential_env`。⭐ REQ-152 的 v2 是**加法式**的 ——
/// 新字段全部可选，所以 v1 映射照常加载、行为逐字节不变（见 [`CONTRACT_VERSION_V2`]）。
pub const CONTRACT_VERSION: u32 = 1;

/// ⭐ REQ-152 · 契约 v2：`operation` 泛化成 `headers{}` + `vars{}`
/// （厂商概念降级为 **header 模板**，字段名里不再出现任何厂商名词），
/// `auth` 补 `signed_headers` / `timestamp` / `scope` 三项**声明**（仍然只有变量名，⛔ 没有值）。
///
/// **v2 是"允许写"，不是"必须写"**：v2 映射里缺某个新字段 ⇒ 那一项就**不出现**
/// （⛔ 不按 `mode` 猜一个默认值 —— 猜出来的签名头是错的，而错签名比没签名更难查）。
pub const CONTRACT_VERSION_V2: u32 = 2;

/// 本程序认识的契约版本。v1 是基线，v2 是加法 ⇒ **两个都收**。
pub fn contract_version_supported(v: u32) -> bool {
    matches!(v, CONTRACT_VERSION | CONTRACT_VERSION_V2)
}

/// 这个版本起，`@pending.get` **允许**给完整取数配方。
///
/// v1 / 不写版本 ⇒ 旧 4 键（`class` / `filter` / `pushdown` / `ref`），一个不多 ——
/// 老客户端只看这 4 个键，多给的东西它也不认（AC-103 / R2）。
pub fn contract_has_recipe(v: u32) -> bool {
    v >= CONTRACT_VERSION_V2
}

// ══════════════════════════════════════════════════════════════
// 错误码（Agent 要能**按码**决定怎么办 —— 设计稿 §五）
// ══════════════════════════════════════════════════════════════

/// 契约本身不合法（缺必填 / 类型不对 / 版本不认识）。
pub const CODE_SCHEMA: &str = "ACCESS_SCHEMA_INVALID";
/// R3：`iri_template` 与本体 `ontok:instanceIri` 不一致。
pub const CODE_IRI_TEMPLATE: &str = "IRI_TEMPLATE_MISMATCH";
/// R4：`pushdown` 与 `local_only` 重叠。
pub const CODE_CAPABILITY_OVERLAP: &str = "CAPABILITY_OVERLAP";
/// R7：relation 的 `from` / `to` 不在 `entities` 里。
pub const CODE_RELATION_ENDPOINT: &str = "RELATION_ENDPOINT_UNKNOWN";
/// 设计稿 §五既有码：映射路径在 fixture 里找不到。
pub const CODE_FIELD_PATH: &str = "FIELD_PATH_MISSING";
/// 映射的类在本体里根本不存在（IRI 拼不出来就谈不上「与本体一致」）。
pub const CODE_CLASS_NOT_IN_ONTOLOGY: &str = "CLASS_NOT_IN_ONTOLOGY";
/// fixture 文件读不了 / 不是合法 JSON。
pub const CODE_FIXTURE_UNREADABLE: &str = "FIXTURE_UNREADABLE";
/// `iterator` 在 fixture 里选不出任何节点。
pub const CODE_ITERATOR_EMPTY: &str = "ITERATOR_EMPTY";
/// `identity.key_path` 在某一行里取不到（拼不出 IRI）。
pub const CODE_KEY_MISSING: &str = "KEY_PATH_MISSING";
/// R5：需要扫描的行数会超出 `max_scan_rows` ⇒ 拒绝并说明。
pub const CODE_SCAN_LIMIT: &str = "SCAN_LIMIT_EXCEEDED";
/// R6：`reviewed_by` 为空的 entity 不许取数。
pub const CODE_UNREVIEWED: &str = "UNREVIEWED_MAPPING";
/// 过滤条件既不在 `pushdown` 也不在 `local_only`、也不是已声明的字段。
pub const CODE_UNKNOWN_FILTER: &str = "UNKNOWN_FILTER_FIELD";
/// 阶段 0：`fetch` 必须显式选一条通路（`--dry-run-fixture` 或 `--allow-network`）。
pub const CODE_DRY_RUN_REQUIRED: &str = "DRY_RUN_REQUIRED";
/// `--entity` 指的那条映射不存在。
pub const CODE_ENTITY_UNKNOWN: &str = "ENTITY_NOT_IN_MAPPING";

// ── REQ-149 · 「没证据」必须长得像「没证据」 ──────────────────────
//
// 这两条不是错误码（都不让 `validate` 失败），而是**醒目的警告标签**：
// 一个没有 fixture 的实体，字段路径**根本没被校验过** ——
// 把它显示成「通过 / 1/1」和「SKIP 记成 passed」是同一类病。

/// 该实体没有 `fixture` ⇒ 字段路径校验**没做**（⛔ 不是通过）。
pub const CODE_FIELD_PATH_NOT_CHECKED: &str = "FIELD_PATH_NOT_CHECKED";
/// 夹具疑似（或被声明为）由映射键**反向生成** ⇒ 字段路径全过属于自证，无独立证据力。
pub const CODE_FIXTURE_LOOKS_DERIVED: &str = "FIXTURE_LOOKS_DERIVED";
/// 同一个占位符**既进 header 又进 body** ⇒ 通常是映射写重了（真调会报未知参数）。
pub const CODE_DUPLICATE_PLACEMENT: &str = "DUPLICATE_PLACEMENT";
/// `--dry-run-fixture` 需要夹具，而这条 entity 没有声明。
pub const CODE_FIXTURE_REQUIRED: &str = "FIXTURE_REQUIRED";

// ══════════════════════════════════════════════════════════════
// 阶段 1 的网络通路错误码（R4：五种失败各报各的，⛔ 不许一律 failed）
// ══════════════════════════════════════════════════════════════

/// 凭据环境变量没设 / 为空。**在发请求之前**就判（⛔ 不拿 401 当答案）。
pub const CODE_AUTH_MISSING: &str = "AUTH_MISSING";
/// 凭据被拒绝：HTTP 401 / 403 或响应码 `AuthFailure.*`。⛔ 不重试。
pub const CODE_AUTH_REJECTED: &str = "AUTH_REJECTED";
/// 被限流：HTTP 429 或响应码 `RequestLimitExceeded`。✅ 退避重试（有上限）。
pub const CODE_RATE_LIMITED: &str = "RATE_LIMITED";
/// 请求超时（连接 / 整体）。⛔ 不静默成「没数据」。
pub const CODE_UPSTREAM_TIMEOUT: &str = "UPSTREAM_TIMEOUT";
/// 连不上 endpoint（DNS / 代理 / 出网策略）。⛔ 不静默成「没数据」。
pub const CODE_UPSTREAM_UNREACHABLE: &str = "UPSTREAM_UNREACHABLE";
/// 服务端返回业务错误（不是签名问题；带它自己的 `Code` / `Message`）。
pub const CODE_API_ERROR: &str = "API_ERROR";
/// 契约声明的 `auth.mode` 不是 `tc3-hmac-sha256`（阶段 1 只实现这一种）。
pub const CODE_AUTH_MODE_UNSUPPORTED: &str = "AUTH_MODE_UNSUPPORTED";
/// 请求模板里的绑定取不到值（`required` 点名了它）。
pub const CODE_REQUEST_BINDING_MISSING: &str = "REQUEST_BINDING_MISSING";
/// 请求体形状不支持（非 POST / 非 JSON body）；⛔ 不偷偷适配、也不静默降级。
pub const CODE_REQUEST_SHAPE: &str = "REQUEST_SHAPE_UNSUPPORTED";

// ══════════════════════════════════════════════════════════════
// 一处问题 / 一个可行动的错误
// ══════════════════════════════════════════════════════════════

/// `validate` / `parse` 报出的一处问题（**收集全部**再一次性给，⛔ 不是遇到第一条就退）。
///
/// `location` 必须点名「在哪个 entity / 哪个字段」—— AC-001 的判据是
/// 「缺必填 ⇒ 报错**点名**是哪个字段、在哪个 entity」。
#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    /// 机读码（[`CODE_SCHEMA`] 等）。
    pub code: &'static str,
    /// 人读位置：`entities[0]（id=cvm_instance）字段 iri_template`。
    pub location: String,
    /// 一句话说明 + 怎么改（可行动）。
    pub message: String,
}

impl Problem {
    pub fn new(code: &'static str, location: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code, location: location.into(), message: message.into() }
    }
}

/// 取数/发布路径上的**单个**致命错误（`Display` = `码 · 说明`）。
///
/// 与 [`Problem`] 的分工：`Problem` 是**体检**（一次报全），`AccessError` 是**执行**（第一条就停）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessError {
    pub code: &'static str,
    pub message: String,
}

impl AccessError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · {}", self.code, self.message)
    }
}

impl std::error::Error for AccessError {}

// ══════════════════════════════════════════════════════════════
// 契约结构（解析后的形态：字段都已校验过）
// ══════════════════════════════════════════════════════════════

/// 一份 access mapping（作者格式 `access.json` 的解析结果）。
#[derive(Debug, Clone, Serialize)]
pub struct Mapping {
    pub contract_version: u32,
    /// 映射版本 —— `@pending[].get.ref` 里那个 `@v<n>`（交互契约 §3.3）。
    pub mapping_version: u32,
    pub project: String,
    pub ontology_version: Option<u32>,
    pub sources: Vec<Source>,
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub relations: Vec<Relation>,
}

/// 一个数据源（**只放凭据的变量名**，⛔ 不放值）。
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub id: String,
    pub kind: String,
    pub base_url: String,
    pub auth: Option<Auth>,
    pub defaults: BTreeMap<String, Value>,
}

/// 鉴权声明（设计稿规则 1：契约会随本体分发 ⇒ 只写环境变量名）。
///
/// ⭐ REQ-152 · v2 补三项**声明**（全部可选，⛔ 一个都不许猜默认值）：
///
/// | 字段 | 给客户端什么 | 不给时 |
/// | --- | --- | --- |
/// | [`Self::signed_headers`] | **哪些 header 参与签名**（小写，客户端照抄） | 该键不出现（AC-106） |
/// | [`Self::timestamp`] | 时间戳写哪个 header、什么格式 | 同上 |
/// | [`Self::scope`] | scope 由哪几段拼出来（`date` / `service` / `tc3_request`） | 同上 |
///
/// ⛔ 凭据**永远**只有变量名（`credential_env`）；值不出客户边界。
#[derive(Debug, Clone, Serialize)]
pub struct Auth {
    pub mode: String,
    pub credential_env: Vec<String>,
    /// ⭐ v2 · 参与签名的 header 名（**小写**；顺序即映射作者写的顺序 ——
    /// 签名前要排序，但那是实现细节，契约不替客户端排）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_headers: Option<Vec<String>>,
    /// ⭐ v2 · 时间戳写进哪个 header、用什么格式。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<AuthTimestamp>,
    /// ⭐ v2 · scope 的拼法（产品代码只透传，⛔ 不解释每一段是什么意思 ——
    /// 那是算法的事，写在 `docs/AUTH-SPEC.md` 里）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
}

/// ⭐ REQ-152 · 签名时间戳的**声明**（写哪个 header + 什么格式）。
#[derive(Debug, Clone, Serialize)]
pub struct AuthTimestamp {
    /// 放时间戳的 header 名（例：`X-Api-Timestamp`）。
    pub header: String,
    /// 值的格式（`unix-seconds` / `unix-millis` / `rfc3339`）。
    pub format: String,
}

/// 一条「类 ← API 响应节点」的映射。
#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    /// 映射条目 id（`@pending[].get.ref` 的 `access#<id>@v<n>`；不要求等于类名）。
    pub id: String,
    /// 本体里的类**本地名**（`云资源` / `cvm_instance`）。
    pub class: String,
    /// 引用 [`Mapping::sources`] 的 `id`。
    pub source: String,
    /// 人签（ADR-005）。`None` ⇒ 未签 ⇒ `fetch` 拒绝（R6）。
    pub reviewed_by: Option<String>,
    pub operation: Operation,
    /// 响应里**一行实体**的位置（`$.Response.InstanceSet[*]`）。
    pub iterator: String,
    /// 键所在字段（拼 IRI 用；与 [`Entity::iri_template`] 的占位符配合）。
    pub key_path: String,
    /// 实例 IRI 模板（必须与本体的 `ontok:instanceIri` 同形 —— R3）。
    pub iri_template: String,
    pub fields: BTreeMap<String, Field>,
    pub paging: Option<Paging>,
    /// **请求体模板 + 绑定**（`request` 段）—— 见 [`RequestSpec`]。
    ///
    /// ⚠️ REQ-144 阶段 1 复审（用户裁定）：⛔ **不做厂商特例适配**。
    /// 请求体形状千差万别（顶层标量 / 数组对象 / 深层嵌套 / 自定义分页位置），
    /// 用「过滤名 → 参数名」那种映射表只能表达第一种 ⇒ 改成**模板 + 占位符**。
    pub request: RequestSpec,
    pub capability: Capability,
    pub freshness: Freshness,
    /// 离线 fixture。**可选**（REQ-149 · 缺口 D）。
    ///
    /// | 有 `fixture` | `validate` 走原来的字段路径校验 |
    /// | --- | --- |
    /// | **没有** | `validate` **不报错**，改为一条 `FIELD_PATH_NOT_CHECKED` 警告 + 该实体**不出现在** `field_coverage` 里；`fetch --dry-run-fixture` 则**明确拒绝**并点名（`FIXTURE_REQUIRED`） |
    /// | 任意情况 | `fetch --allow-network` 不受影响（真调用不需要夹具） |
    ///
    /// ⛔ 刻意**不**要求「没有夹具也要有一条」：旧口径逼着人从映射反向造夹具，
    /// 于是「1/1 覆盖」变成了自证。宁可显示「未校准」，也不要假证据。
    pub fixture: Option<String>,
    /// 夹具的**来源声明**（可选）：`declared`（独立采集的真实/手工响应）或
    /// `derived`（由映射键反向生成）。缺省 ⇒ 工具按结构做**启发式**判断。
    ///
    /// 声明为 `derived` 时，`validate` 会明确说「这份夹具没有独立证据力」。
    /// 它不是安全边界（作者可以写 `declared`），而是**让人无法在不知情的情况下**
    /// 把自证夹具当成校准过的证据。
    pub fixture_source: Option<FixtureSource>,
}

/// 夹具来源（REQ-149 · 缺口 D 的「可看出来」落点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FixtureSource {
    /// 作者声明：独立采集的响应（工具**不**核实）。
    Declared,
    /// 作者声明：由映射键反向生成 ⇒ 字段路径全过是自证。
    Derived,
}

impl FixtureSource {
    pub fn as_str(self) -> &'static str {
        match self {
            FixtureSource::Declared => "declared",
            FixtureSource::Derived => "derived",
        }
    }
}

/// **请求体模板 + 绑定**：请求体长什么样由作者逐字写，执行器只做占位符替换。
///
/// ```json
/// "request": {
///   "required": ["zone"],
///   "template": { "Region": "{region}",
///                 "Clauses": [ { "Field": "zone", "Values": ["{zone}"] } ] },
///   "bindings": { "region": { "source": "default" },
///                 "zone":   { "source": "filter" } },
///   "paging":   { "offset": "Offset", "limit": "Limit" }
/// }
/// ```
///
/// # 语义（三句话）
///
/// 1. `template` 是**请求体原样**：任意深度嵌套、任意数组，键名一个都不改；
/// 2. 字符串值里的 `{name}` 按 `bindings[name]` 取值替换（⛔ 不替换键名）；
/// 3. `paging.offset` / `.limit` 是**路径**（`Offset` 或 `Page.Number`），由分页器写进去。
///
/// # 分页的三种真实形态（REQ-149 · 缺口 A/B）
///
/// | 接口怎么数 | 契约怎么写 | 第 1 页 / 第 2 页发出的值 |
/// | --- | --- | --- |
/// | **偏移量、0 基**（默认） | `"offset": "Offset"` | `0` / `100` |
/// | **偏移量、1 基**（少见） | 本实现对 `offset_base: 1` 按**页码**解释（见下） | `1` / `2` |
/// | **页码、1 基** | `"page": "PageNumber"`（推荐） | `1` / `2` |
/// | **页码、0 基** | `"page": "PageNumber"` + `"offset_base": 0` | `0` / `1` |
///
/// ⚠️ **`offset_base: 1` 的语义是「页码」，不是「已取条数 + 1」**。
/// 理由：位置值 = 0 时唯一自然的读法是「跳过多少条」；= 1 时唯一自然的读法是「第几页」。
/// 若接口真的要「1 基的条数偏移」，本实现没有表达它 —— 请写成 `page`（如果那其实是页码），
/// 或开需求（⛔ 不要靠猜：猜错的表现是「第 2 页返回第 1 页」）。
/// 想要**不歧义**就写 `page`：`"page": "PageNumber"` 与 `"offset_base": 1` 等价且更清楚。
///
/// `value_type`（`number` 默认 / `string`）决定**写进请求体的 JSON 类型**：
/// 有些接口把 `Offset/Limit` 在文档里定义成 String（`"0"` / `"100"`），发数字会被拒。
/// 它同时作用于 `{offset}` / `{limit}` 两个内建占位符。
///
/// # 绑定的值类型 / 形状（REQ-149 · 缺口 C）
///
/// `bindings.<名>.value_type ∈ {string(默认), integer, boolean}`、`shape ∈ {scalar(默认), array}`。
/// 于是「整数数组」这类筛选能真正下推：`{"source":"filter","value_type":"integer","shape":"array"}`
/// ⇒ `[1, 2]`（不是 `["1"]`）。取值集合封闭；不认识的组合**报错并点名**，⛔ 不静默降级成字符串。
///
/// # 为什么要「模板」而不是「参数表」
///
/// 真调用（只读账号，2026-10-25）暴露：某接口**不接受**顶层过滤参数，只认
/// 「数组里放对象、对象里放值和字段名」的形态。这不是某一个接口的特例：
/// 顶层标量、数组对象、深层嵌套三种形状**都是真实存在的**。
/// 参数表天生表达不了后两种 ⇒ 改成模板后，形状由作者写、代码里**零厂商/接口名词**。
///
/// # 缺值怎么剪枝（`--filter` 没给时）
///
/// 授权一个整值占位符（`"{zone}"` 整个字符串就是占位符）没拿到值 ⇒：
///
/// | 位置 | 行为 |
/// | --- | --- |
/// | 对象的成员 | 丢掉这个键 |
/// | **数组的元素**（该元素子树里只要有一处被剪） | 丢掉整个元素（→ 空数组再丢掉这个键） |
/// | 字符串里**嵌着**占位符（`"a-{x}"`）却没值 | **报错**（⛔ 不写半截字符串） |
///
/// 这条规则让「可选过滤」不必在契约里写条件：没给 `zone` 就整段不出现，
/// 而不是发出一个空值让服务端去猜。
#[derive(Debug, Clone, Default, Serialize)]
pub struct RequestSpec {
    /// 请求体模板（JSON 对象）。`None` ⇒ 走「无模板」的旧口径（顶层参数 + 分页参数）。
    pub template: Option<Value>,
    pub bindings: BTreeMap<String, Binding>,
    /// 必须能取到值的绑定名（缺 ⇒ `REQUEST_BINDING_MISSING`）。
    pub required: Vec<String>,
    /// 分页器把 offset 写到模板里的**哪个路径**（`Offset` / `Page.Number`）。
    ///
    /// `None` ⇒ 用实体级 `paging.offset_param`，再退化为 `Offset`。
    pub paging_offset: Option<String>,
    /// 同上，limit。
    pub paging_limit: Option<String>,
    /// 页码写到哪个路径（`PageNumber`）。与 [`RequestSpec::paging_offset`] **互斥**。
    ///
    /// 有它 ⇒ 分页器发的是**页码**（`已取页数 + offset_base`，
    /// `offset_base` 缺省 1）。见类型级文档的「分页的三种真实形态」。
    pub paging_page: Option<String>,
    /// 分页值的 JSON 类型（`number` 默认 / `string`）。
    pub paging_value_type: PagingValueType,
    /// 位置基（`0` / `1`）。`None` ⇒ 随语义取默认：`offset` 0 基、`page` 1 基。
    ///
    /// ⚠️ `offset_base: 1` 把 `offset` 解释成**页码**（不是「已取条数 + 1」）。
    /// 想要不歧义请写 `page`。
    pub offset_base: Option<u32>,
}

/// 分页值写进请求体时的 JSON 类型（REQ-149 · 缺口 A）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PagingValueType {
    /// 数字（默认，与阶段 0/1 的旧行为逐字节相同）。
    #[default]
    Number,
    /// 字符串（文档把 `Offset/Limit` 定义成 String 的接口）。
    String,
}

impl PagingValueType {
    /// 把一个非负整数按本类型转成 JSON 值。
    pub fn json(self, n: usize) -> Value {
        match self {
            PagingValueType::Number => Value::from(n),
            PagingValueType::String => Value::String(n.to_string()),
        }
    }
}

/// 一个占位符的取值来源 + 值类型 / 形状（REQ-149 · 缺口 C）。
///
/// `value_type` / `shape` 都**可选**；缺省 = `string` + `scalar`（旧行为）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub source: BindingSource,
    /// 值类型。`None` ⇒ `string`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<BindingValueType>,
    /// 形状。`None` ⇒ `scalar`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<BindingShape>,
}

impl Binding {
    /// 只带来源的绑定（旧写法 / 默认值）。
    pub fn new(source: BindingSource) -> Self {
        Self { source, value_type: None, shape: None }
    }

    pub fn value_type(&self) -> BindingValueType {
        self.value_type.unwrap_or_default()
    }

    pub fn shape(&self) -> BindingShape {
        self.shape.unwrap_or_default()
    }

    /// 有没有显式声明类型 / 形状（没有 ⇒ 走**逐字节不变**的旧路径）。
    pub fn has_type_decl(&self) -> bool {
        self.value_type.is_some() || self.shape.is_some()
    }
}

/// 绑定值的类型（封闭集合；REQ-149 · R3）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingValueType {
    /// 字符串（默认）。
    #[default]
    String,
    /// 整数（`"1"` ⇒ `1`；`[1,2]` 可下推整数数组）。
    Integer,
    /// 布尔（`"true"` ⇒ `true`）。
    Boolean,
}

/// 绑定值的形状（封闭集合；REQ-149 · R3）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BindingShape {
    /// 单值（默认）。
    #[default]
    Scalar,
    /// 数组（单值也包成 `[v]`）。
    Array,
}

/// 值从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BindingSource {
    /// 取 `sources[].defaults[占位符名]`（先精确匹配，再大小写不敏感兜底）。
    Default,
    /// 取调用方 `--filter[占位符名]`。
    Filter,
    /// 由分页器提供：`offset` 或 `limit`。
    Paging(PagingValue),
}

/// 分页器提供的两个值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PagingValue {
    Offset,
    Limit,
}

/// API 动作（`ontok:apiAction` + 三个 HTTP 细节）。
///
/// ⭐ REQ-152 · **v2 起厂商概念降级为 header 模板**：`headers` 里写
/// `{"X-Api-Action": "{action}"}`、`vars` 里写 `{"action": "DescribeInstances"}` ——
/// 于是「动作名落到哪个 header」是**映射的数据**，不是产品代码里的分支。
///
/// ⛔ 老字段 `action` / `version` / `content_type` **原样保留**（R2）：v1 映射、
/// 以及仍在用它们的执行器（构建面）不需要改。新字段全部可选 ⇒ 没写就不出现。
#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    /// v1 的动作名。⚠️ v2 起**允许缺**（这时动作名在 `vars` 里、落点由 `headers` 决定）；
    /// v1 映射缺它仍然报错（基线行为不变）。
    pub action: String,
    pub version: Option<String>,
    pub method: String,
    pub content_type: String,
    /// ⭐ v2 · 请求路径（`"/"`）。`None` ⇒ 该键不出现（客户端按 `base_url` 自己决定）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// ⭐ v2 · **header 模板**：名字由映射作者写，值里的 `{name}` 从 `vars` 取。
    ///
    /// ⛔ 产品代码不认任何具体的 header 名 —— 它只负责把这张表原样透出 / 原样填值。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    /// ⭐ v2 · header 模板里占位符的取值（`{"action": "DescribeInstances"}`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<BTreeMap<String, String>>,
}

/// 一个字段映射。
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    /// 相对**一行响应对象**的路径（`Placement.Zone`）。
    pub path: String,
    /// `xsd:` 本地名（`integer` / `boolean` / `dateTime` …）；缺省 ⇒ `string`。
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ty: Option<String>,
}

/// 分页（`rml:source` 之外的、RML 没有的那部分）。
#[derive(Debug, Clone, Serialize)]
pub struct Paging {
    pub offset_param: String,
    pub limit_param: String,
    /// 单页上限（**服务端**的上限；`--limit` 是客户端要多少行，两者不同）。
    pub max_limit: usize,
    /// 总行数路径（`$.Response.TotalCount`）。有它 ⇒ 能算准还要几页。
    pub total_path: Option<String>,
}

/// 能力声明（**必须显式**：不写就等于暗示「什么都能过滤」——规则 3）。
#[derive(Debug, Clone, Serialize)]
pub struct Capability {
    /// API **服务端**支持的过滤条件（逻辑名：`zone` / `ids` / `region`）。
    pub pushdown: Vec<String>,
    /// 只能**本地**过滤的字段（`status` / `cpu` …）。
    pub local_only: Vec<String>,
    /// 一次取数最多扫多少行（超 ⇒ `SCAN_LIMIT_EXCEEDED`，规则 5）。
    pub max_scan_rows: u64,
    pub cost_hint: Option<String>,
}

/// 时点声明（规则 6：运维数字最怕「数字对但时点不对」）。
#[derive(Debug, Clone, Serialize)]
pub struct Freshness {
    pub mode: String,
    pub note: Option<String>,
}

/// 一条关系（v1：⛔ 不做跨源 join）。
#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    pub predicate: String,
    pub from: String,
    pub to: String,
    pub via: Option<Via>,
    /// 跨源边：**不遍历**（demand §三 R-4）。
    pub cross_source: bool,
    pub traversable: bool,
}

/// 关系怎么连上的。
#[derive(Debug, Clone, Serialize)]
pub struct Via {
    /// `join`（同源两实体各拉一批再拼）或 `field`（同一行里一个字段指过去）。
    pub kind: String,
    pub local: Option<String>,
    pub remote: Option<String>,
    pub field: Option<String>,
}

impl Mapping {
    /// 按 id 找一条实体映射；找不到再按类名找（`--entity` 两种都认）。
    pub fn entity(&self, id_or_class: &str) -> Option<&Entity> {
        self.entities
            .iter()
            .find(|e| e.id == id_or_class)
            .or_else(|| self.entities.iter().find(|e| e.class == id_or_class))
    }

    /// 某个 source（`Entity::source` 的解析）。
    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// ⭐ REQ-152 · 这份映射**允许**给完整取数配方吗（`contract_version >= 2`）。
    ///
    /// v1 ⇒ `@pending.get` 只有旧 4 键（AC-103）；这是**一处判定**，
    /// ⛔ 不在每个下游各判一次（判两次就会有一处漏掉）。
    pub fn has_recipe(&self) -> bool {
        contract_has_recipe(self.contract_version)
    }
}

impl Entity {
    /// 这条映射签过没有（R6）。空白字符串**不算**签。
    pub fn reviewed(&self) -> bool {
        self.reviewed_by.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
    }
}

// ══════════════════════════════════════════════════════════════
// 解析（作者格式 JSON → 结构）
// ══════════════════════════════════════════════════════════════

/// 解析一份 `access.json`。**一次报全**：`Err` 里是全部问题（⛔ 不是遇到第一条就退）。
///
/// 为什么不用 `serde` 直接反序列化：`serde_json` 的错误只说「第 N 行第 M 列缺字段」，
/// 而 AC-001 要求**点名是哪个字段、在哪个 entity** —— 那需要自己走一遍取值。
pub fn parse(text: &str) -> Result<Mapping, Vec<Problem>> {
    let root: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            return Err(vec![Problem::new(
                CODE_SCHEMA,
                "<根>",
                format!("不是合法的 JSON：{e}。先确认文件没被截断 / 没多一个逗号。"),
            )])
        }
    };
    parse_value(&root, &mut Vec::new())
}

/// 同样的事，但输入已经是 `Value`（`publish` / 测试复用）。
pub fn parse_value(root: &Value, problems: &mut Vec<Problem>) -> Result<Mapping, Vec<Problem>> {
    let top = match root.as_object() {
        Some(o) => o,
        None => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                "<根>",
                "顶层必须是 JSON 对象（`{ \"contract_version\": 1, \"sources\": [...], ... }`）。",
            ));
            return Err(std::mem::take(problems));
        }
    };

    // ── contract_version（先看版本：版本不认识时，后面的字段名可能整套都变了）──
    //
    // ⭐ REQ-152 · v1 与 v2 **都收**（v2 是加法式：新字段全部可选）。
    // 记下**声明的那个**版本号（⛔ 不统一写成常量）：`publish` 的 `ontok:contractVersion`
    // 与 `@pending.get` 的配方门槛都读它 —— 把 v2 记成 v1 会让配方永远不出现。
    let mut version_ok = true;
    let mut declared_version = CONTRACT_VERSION;
    match opt_u64(top, "contract_version") {
        Some(v) if contract_version_supported(v as u32) => declared_version = v as u32,
        Some(v) => {
            version_ok = false;
            problems.push(Problem::new(
                CODE_SCHEMA,
                "字段 contract_version",
                format!(
                    "契约版本 {v} 不认识（本程序只认 {CONTRACT_VERSION} / {CONTRACT_VERSION_V2}）。\
                     ⛔ 破坏性变更必须升版本号，而不是让读端去猜字段。"
                ),
            ));
        }
        None => {
            version_ok = false;
            problems.push(Problem::new(
                CODE_SCHEMA,
                "字段 contract_version",
                format!(
                    "缺必填整数字段（本程序认 {CONTRACT_VERSION} / {CONTRACT_VERSION_V2}）。"
                ),
            ));
        }
    }

    let project = need_str(top, "project", "<根>", problems);
    let mapping_version = opt_u64(top, "mapping_version").unwrap_or(1) as u32;
    let ontology_version = opt_u64(top, "ontology_version").map(|v| v as u32);

    // ── sources[] ──
    let mut sources = Vec::new();
    if let Some(items) = need_array(top, "sources", "<根>", problems) {
        if items.is_empty() {
            problems.push(Problem::new(
                CODE_SCHEMA,
                "字段 sources",
                "至少要有 1 个数据源（空数组 = 这份映射取不到任何数）。",
            ));
        }
        for (i, item) in items.iter().enumerate() {
            if let Some(s) = parse_source(item, i, problems) {
                sources.push(s);
            }
        }
    }

    // ── entities[] ──
    let mut entities: Vec<Entity> = Vec::new();
    if let Some(items) = need_array(top, "entities", "<根>", problems) {
        if items.is_empty() {
            problems.push(Problem::new(
                CODE_SCHEMA,
                "字段 entities",
                "至少要有 1 条实体映射（空数组 = 本体里的类一个都取不到）。",
            ));
        }
        for (i, item) in items.iter().enumerate() {
            if let Some(e) = parse_entity(item, i, &sources, problems) {
                entities.push(e);
            }
        }
    }

    // ── relations[]（可选；缺省 = 没有关系）──
    let mut relations = Vec::new();
    if let Some(items) = opt_array(top, "relations") {
        for (i, item) in items.iter().enumerate() {
            if let Some(r) = parse_relation(item, i, problems) {
                relations.push(r);
            }
        }
    }

    if !problems.is_empty() {
        return Err(std::mem::take(problems));
    }
    debug_assert!(version_ok);

    Ok(Mapping {
        contract_version: declared_version,
        mapping_version,
        project: project.unwrap_or_default(),
        ontology_version,
        sources,
        entities,
        relations,
    })
}

/// ⭐ REQ-152 · 解析 `sources[].auth`（v1 只认 `mode` / `credential_env`；v2 的三项声明可选）。
///
/// ⛔ 这里**只解声明**：不读环境变量、不碰任何凭据值、不按 `mode` 推默认头。
fn parse_auth(a: &Map<String, Value>, loc: &str, problems: &mut Vec<Problem>) -> Auth {
    let timestamp = match a.get("timestamp") {
        None => None,
        Some(Value::Object(t)) => Some(AuthTimestamp {
            header: need_str(t, "header", &format!("{loc}.timestamp"), problems)
                .unwrap_or_default(),
            format: need_str(t, "format", &format!("{loc}.timestamp"), problems)
                .unwrap_or_default(),
        }),
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc}.timestamp"),
                "必须是对象（`{\"header\": \"…\", \"format\": \"unix-seconds\"}`）—— \
                 一个字符串表达不了「写哪个 header + 什么格式」两件事。",
            ));
            None
        }
    };
    Auth {
        mode: opt_str(a, "mode").unwrap_or_else(|| "none".to_string()),
        credential_env: opt_str_list(a, "credential_env").unwrap_or_default(),
        signed_headers: opt_str_list(a, "signed_headers").filter(|v| !v.is_empty()),
        timestamp,
        scope: opt_str_list(a, "scope").filter(|v| !v.is_empty()),
    }
}

/// ⭐ REQ-152 · 解析 `headers{}` / `vars{}` 这类「名字 → 字符串」表。
///
/// 空对象 / 缺省 ⇒ `None`（**整个键不出现**）；值不是字符串 ⇒ 报错点名（⛔ 不静默丢）。
fn parse_str_map(
    o: &Map<String, Value>,
    key: &str,
    loc: &str,
    problems: &mut Vec<Problem>,
) -> Option<BTreeMap<String, String>> {
    match o.get(key) {
        None => None,
        Some(Value::Object(m)) => {
            let mut out: BTreeMap<String, String> = BTreeMap::new();
            for (k, v) in m {
                match v.as_str() {
                    Some(s) => {
                        out.insert(k.clone(), s.to_string());
                    }
                    None => problems.push(Problem::new(
                        CODE_SCHEMA,
                        format!("{loc} 字段 {key}.{k}"),
                        "必须是字符串。header 模板 / 变量表里每一项都是「名字 → 字符串」，\
                         ⛔ 不支持嵌套（嵌套会让「填值规则」变成一门语言）。",
                    )),
                }
            }
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        }
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 {key}"),
                "必须是 JSON 对象（`{\"名字\": \"值或 {占位符}\"}`）。",
            ));
            None
        }
    }
}

fn parse_source(item: &Value, idx: usize, problems: &mut Vec<Problem>) -> Option<Source> {
    let loc = format!("sources[{idx}]");
    let obj = match item.as_object() {
        Some(o) => o,
        None => {
            problems.push(Problem::new(CODE_SCHEMA, &loc, "必须是 JSON 对象。"));
            return None;
        }
    };
    let id = need_str(obj, "id", &loc, problems)?;
    // `kind` 有默认值（`http-json`）⇒ ⛔ 不用 `need_str`（那会记一条「缺必填」的问题，
    // 于是「没写 kind」变成硬错误 —— 而它本该是一个有默认值的可选声明）。
    let kind = opt_str(obj, "kind").unwrap_or_else(|| "http-json".to_string());
    let base_url = need_str(obj, "base_url", &loc, problems)?;
    let auth = obj
        .get("auth")
        .and_then(|a| a.as_object())
        .map(|a| parse_auth(a, &format!("{loc} 字段 auth"), problems));
    let defaults = obj
        .get("defaults")
        .and_then(|d| d.as_object())
        .map(|d| d.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    Some(Source { id, kind, base_url, auth, defaults })
}

fn parse_entity(
    item: &Value,
    idx: usize,
    sources: &[Source],
    problems: &mut Vec<Problem>,
) -> Option<Entity> {
    let obj = match item.as_object() {
        Some(o) => o,
        None => {
            problems.push(Problem::new(CODE_SCHEMA, format!("entities[{idx}]"), "必须是 JSON 对象。"));
            return None;
        }
    };
    let id = opt_str(obj, "id").unwrap_or_else(|| format!("#{idx}"));
    let loc = format!("entities[{idx}]（id={id}）");

    let class = need_str(obj, "class", &loc, problems);
    let source = need_str(obj, "source", &loc, problems);
    if let Some(sid) = &source {
        if !sources.iter().any(|s| &s.id == sid) {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 source"),
                format!("引用了不存在的数据源 `{sid}`（`sources[].id` 里没有它）。"),
            ));
        }
    }

    // reviewed_by **必须在场**，但可以是 null（未签是合法状态，只是不许取数 —— R6）。
    let reviewed_by = match obj.get("reviewed_by") {
        None => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 reviewed_by"),
                "缺必填字段：即使还没人签也要写 `\"reviewed_by\": null` \
                 （⛔ 不写等于暗示「不用签」，而 R6 要求未签的实体不许取数）。",
            ));
            None
        }
        Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 reviewed_by"),
                "必须是字符串（`\"张三@2026-10-25\"`）或 null。",
            ));
            None
        }
    };

    let operation = obj
        .get("operation")
        .and_then(|o| o.as_object())
        .and_then(|o| {
            let o_loc = format!("{loc} 字段 operation");
            // ⭐ REQ-152 · v2：`action` 可以缺 —— 这时动作名在 `vars` 里、落点由 `headers`
            // 决定。但**既没有 action 也没有 headers** 时仍然报错（v1 行为不变）。
            let headers = parse_str_map(o, "headers", &o_loc, problems);
            let action = match opt_str(o, "action") {
                Some(a) if !a.trim().is_empty() => a,
                _ if headers.is_some() => String::new(),
                _ => need_str(o, "action", &o_loc, problems)?,
            };
            Some(Operation {
                action,
                version: opt_str(o, "version"),
                method: opt_str(o, "method").unwrap_or_else(|| "POST".to_string()),
                content_type: opt_str(o, "content_type")
                    .unwrap_or_else(|| "application/json".to_string()),
                path: opt_str(o, "path"),
                headers,
                vars: parse_str_map(o, "vars", &o_loc, problems),
            })
        });
    if operation.is_none() && !obj.contains_key("operation") {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 operation"),
            "缺必填对象（至少要 `{\"action\":\"SomeAction\"}`；v2 也可只写 \
             `{\"method\":\"POST\",\"headers\":{…},\"vars\":{…}}`）。",
        ));
    }

    // iterator：顶层规范写法，回退设计稿 v1 的 `identity.select`。
    let identity = obj.get("identity").and_then(|v| v.as_object());
    let iterator = match opt_str(obj, "iterator") {
        Some(s) => Some(s),
        None => identity.and_then(|i| opt_str(i, "select")),
    };
    let iterator = iterator.or_else(|| {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 iterator"),
            "缺必填字段：响应里「一行实体」的路径（如 `$.Response.InstanceSet[*]`）。",
        ));
        None
    });

    let key_path = identity
        .and_then(|i| opt_str(i, "key_path"))
        .or_else(|| opt_str(obj, "key_path"));
    let key_path = key_path.or_else(|| {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 identity.key_path"),
            "缺必填字段：「哪一列是键」（拼实例 IRI 用，如 `InstanceId`）。",
        ));
        None
    });

    let iri_template = opt_str(obj, "iri_template")
        .or_else(|| identity.and_then(|i| opt_str(i, "iri_template")));
    let iri_template = iri_template.or_else(|| {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 iri_template"),
            "缺必填字段：实例 IRI 模板（必须与本体的 `ontok:instanceIri` 同形，占位符是 `{id}`）。",
        ));
        None
    });

    // fields：至少要有 1 个（一条不取属性的映射是空映射）。
    let mut fields: BTreeMap<String, Field> = BTreeMap::new();
    match obj.get("fields").and_then(|f| f.as_object()) {
        Some(map) if !map.is_empty() => {
            for (name, spec) in map {
                let path = match spec.as_object().and_then(|o| opt_str(o, "path")) {
                    Some(p) => p,
                    None => {
                        problems.push(Problem::new(
                            CODE_SCHEMA,
                            format!("{loc} 字段 fields.{name}.path"),
                            "缺必填字段（响应里的相对路径，如 `Placement.Zone`）。",
                        ));
                        continue;
                    }
                };
                let ty = spec.as_object().and_then(|o| opt_str(o, "type"));
                fields.insert(name.clone(), Field { path, ty });
            }
        }
        Some(_) => problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 fields"),
            "至少要映射 1 个属性（空对象 = 这条映射取不到任何属性）。",
        )),
        None => problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 fields"),
            "缺必填对象（`{ \"name\": {\"path\": \"InstanceName\"}, ... }`）。",
        )),
    }

    // paging：顶层规范写法，回退设计稿 v1 的 `request.paging`。
    //
    // ⚠️ 两种 `request.paging` 同名不同义，靠**键名**区分（⛔ 不靠猜测）：
    //   - `{offset_param, limit_param, …}` ⇒ 设计稿 v1 的**实体级分页**（本回退）
    //   - `{offset, limit}`                 ⇒ 阶段 1 的**模板分页路径**（见 [`RequestSpec`]）
    let paging_obj = obj.get("paging").and_then(|p| p.as_object()).or_else(|| {
        obj.get("request")
            .and_then(|r| r.as_object())
            .and_then(|r| r.get("paging"))
            .and_then(|p| p.as_object())
            .filter(|p| p.contains_key("offset_param") || p.contains_key("limit_param"))
    });
    let paging = paging_obj.map(|p| {
        let loc2 = format!("{loc} 字段 paging");
        // REQ-149 · 新字段只在 `request.paging` 生效（分页器读的是那里）。
        // 写到实体级 `paging` 里 ⇒ **报错点名**，⛔ 不静默忽略（忽略 = 以为声明了却发旧值）。
        for wrong in ["value_type", "offset_base", "page"] {
            if p.contains_key(wrong) {
                problems.push(Problem::new(
                    CODE_SCHEMA,
                    format!("{loc2}.{wrong}"),
                    "这个字段属于 `request.paging`（分页器读的是那里），不是实体级 `paging`。\
                     实体级 `paging` 只认 `offset_param` / `limit_param` / `max_limit` / `total_path`。\
                     请把它挪进 `request.paging` —— ⛔ 本实现不会静默忽略（否则你以为声明了，发出去的还是旧值）。",
                ));
            }
        }
        Paging {
            offset_param: need_str(p, "offset_param", &loc2, problems)
                .unwrap_or_else(|| "Offset".to_string()),
            limit_param: need_str(p, "limit_param", &loc2, problems)
                .unwrap_or_else(|| "Limit".to_string()),
            max_limit: opt_u64(p, "max_limit").unwrap_or(100) as usize,
            total_path: opt_str(p, "total_path"),
        }
    });
    if let Some(pg) = &paging {
        if pg.max_limit == 0 {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 paging.max_limit"),
                "必须 ≥ 1（0 意味着这个 API 一页都取不到，映射写错了）。",
            ));
        }
    }

    // `request` 段：请求体模板 + 绑定（见 [`RequestSpec`]）；旧写法 `from_filter` 内部转成模板。
    let request = parse_request_spec(obj.get("request"), &loc, problems);

    // capability：三个字段**都必须显式**（设计稿规则 3）。
    let capability = obj
        .get("capability")
        .and_then(|c| c.as_object())
        .map(|c| {
            let loc2 = format!("{loc} 字段 capability");
            Capability {
                pushdown: opt_str_list(c, "pushdown").unwrap_or_else(|| {
                    problems.push(Problem::new(
                        CODE_SCHEMA,
                        format!("{loc2}.pushdown"),
                        "缺必填字段：⛔ 「不写」不等于「都能下推」—— \
                         服务端不支持却写在这里，Agent 会以为筛过了。空数组也要显式写 []。",
                    ));
                    Vec::new()
                }),
                local_only: opt_str_list(c, "local_only").unwrap_or_else(|| {
                    problems.push(Problem::new(
                        CODE_SCHEMA,
                        format!("{loc2}.local_only"),
                        "缺必填字段：只能本地过滤的字段要显式列出来（空数组也要写 []）。",
                    ));
                    Vec::new()
                }),
                max_scan_rows: match opt_u64(c, "max_scan_rows") {
                    Some(v) if v > 0 => v,
                    Some(_) => {
                        problems.push(Problem::new(
                            CODE_SCHEMA,
                            format!("{loc2}.max_scan_rows"),
                            "必须 ≥ 1（0 = 这条映射一行都取不到）。",
                        ));
                        1
                    }
                    None => {
                        problems.push(Problem::new(
                            CODE_SCHEMA,
                            format!("{loc2}.max_scan_rows"),
                            "缺必填整数字段：超了就要拒绝（R5），⛔ 不许默认成无穷大。",
                        ));
                        0
                    }
                },
                cost_hint: opt_str(c, "cost_hint"),
            }
        });
    let capability = capability.unwrap_or_else(|| {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 capability"),
            "缺必填对象（`pushdown` / `local_only` / `max_scan_rows` 三个都必须在）。",
        ));
        Capability {
            pushdown: Vec::new(),
            local_only: Vec::new(),
            max_scan_rows: 0,
            cost_hint: None,
        }
    });

    let freshness = obj
        .get("freshness")
        .and_then(|f| f.as_object())
        .map(|f| {
            let loc2 = format!("{loc} 字段 freshness");
            Freshness {
                mode: need_str(f, "mode", &loc2, problems).unwrap_or_else(|| "unknown".to_string()),
                note: opt_str(f, "note"),
            }
        });
    let freshness = freshness.unwrap_or_else(|| {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 freshness"),
            "缺必填对象（至少 `{\"mode\":\"realtime\"}`）—— 回执要带时点（规则 6）。",
        ));
        Freshness { mode: "unknown".to_string(), note: None }
    });

    // REQ-149 · 缺口 D：`fixture` **可选**。没有它 ⇒ validate 不报错（改为
    // `FIELD_PATH_NOT_CHECKED` 警告 + 不出现在 field_coverage 里），
    // 只有 `fetch --dry-run-fixture` 会明确拒绝。
    let fixture = opt_str(obj, "fixture").filter(|s| !s.trim().is_empty());

    // 来源声明（可选）：declared / derived，封闭取值。
    let fixture_source = match obj.get("fixture_source") {
        None => None,
        Some(Value::Null) => None,
        Some(Value::String(s)) => match s.trim() {
            "declared" => Some(FixtureSource::Declared),
            "derived" => Some(FixtureSource::Derived),
            other => {
                problems.push(Problem::new(
                    CODE_SCHEMA,
                    format!("{loc} 字段 fixture_source"),
                    format!(
                        "只能是 `declared`（独立采集的响应）或 `derived`（由映射键反向生成），\
                         不认识 `{other}`。⛔ 本字段不写也合法（缺省时工具按结构做启发式判断）。"
                    ),
                ));
                None
            }
        },
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 fixture_source"),
                "必须是字符串 `declared` / `derived`（或缺省）。",
            ));
            None
        }
    };
    if fixture_source.is_some() && fixture.is_none() {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 fixture_source"),
            "写了 `fixture_source` 却没有 `fixture` —— 没有夹具就无从谈来源；\
             要么补 `fixture`，要么把 `fixture_source` 去掉。",
        ));
    }

    Some(Entity {
        id,
        class: class?,
        source: source?,
        reviewed_by,
        operation: operation?,
        iterator: iterator?,
        key_path: key_path?,
        iri_template: iri_template?,
        fields,
        paging,
        request,
        capability,
        freshness,
        fixture,
        fixture_source,
    })
}

fn parse_relation(item: &Value, idx: usize, problems: &mut Vec<Problem>) -> Option<Relation> {
    let loc = format!("relations[{idx}]");
    let obj = match item.as_object() {
        Some(o) => o,
        None => {
            problems.push(Problem::new(CODE_SCHEMA, &loc, "必须是 JSON 对象。"));
            return None;
        }
    };
    let predicate = need_str(obj, "predicate", &loc, problems)?;
    let from = need_str(obj, "from", &loc, problems)?;
    let to = need_str(obj, "to", &loc, problems)?;
    let via = obj.get("via").and_then(|v| v.as_object()).map(|v| Via {
        kind: opt_str(v, "kind").unwrap_or_else(|| "field".to_string()),
        local: opt_str(v, "local"),
        remote: opt_str(v, "remote"),
        field: opt_str(v, "field"),
    });
    Some(Relation {
        predicate,
        from,
        to,
        via,
        cross_source: obj.get("cross_source").and_then(|b| b.as_bool()).unwrap_or(false),
        // 缺省 = 可遍历（同源边）。跨源边必须显式声明不可遍历（validate 会查）。
        traversable: obj.get("traversable").and_then(|b| b.as_bool()).unwrap_or(true),
    })
}

// ── `request` 段：模板 + 绑定（见 [`RequestSpec`]）──────────────────

/// 解析 `request` 段。**一次报全**：模板形状、绑定、必填、占位符对账分别报。
fn parse_request_spec(
    spec: Option<&Value>,
    loc: &str,
    problems: &mut Vec<Problem>,
) -> RequestSpec {
    let mut out = RequestSpec::default();
    let obj = match spec {
        None => return out,
        Some(v) => match v.as_object() {
            Some(o) => o,
            None => {
                problems.push(Problem::new(
                    CODE_SCHEMA,
                    format!("{loc} 字段 request"),
                    "必须是对象（`{\"template\":{…},\"bindings\":{…}}`）。",
                ));
                return out;
            }
        },
    };

    // ① 模板：必须是 JSON 对象（非 JSON 形态**报不支持**，⛔ 不偷偷适配）
    if let Some(t) = obj.get("template") {
        match t.as_object() {
            Some(_) => out.template = Some(t.clone()),
            None => problems.push(Problem::new(
                CODE_REQUEST_SHAPE,
                format!("{loc} 字段 request.template"),
                "模板必须是 **JSON 对象**。本实现只签/只发 **POST + JSON** 请求体；\
                 XML body、multipart、application/x-www-form-urlencoded、GET query 签名\
                 都**不支持** —— ⛔ 不偷偷适配，也不静默降级成别的形状。\
                 请把接口的 JSON 请求体原样写成模板。",
            )),
        }
    }

    // ② 绑定
    if let Some(bs) = obj.get("bindings") {
        match bs.as_object() {
            Some(map) => {
                for (name, b) in map {
                    let bloc = format!("{loc} 字段 request.bindings.{name}");
                    if let Some(binding) = parse_binding(b, &bloc, problems) {
                        out.bindings.insert(name.clone(), binding);
                    }
                }
            }
            None => problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 request.bindings"),
                "必须是对象：`{\"zone\":{\"source\":\"filter\"}}`。",
            )),
        }
    }

    // ③ 必填绑定
    if let Some(req) = obj.get("required") {
        match req.as_array() {
            Some(items) => {
                for it in items {
                    match it.as_str() {
                        Some(s) if !s.trim().is_empty() => out.required.push(s.to_string()),
                        _ => problems.push(Problem::new(
                            CODE_SCHEMA,
                            format!("{loc} 字段 request.required"),
                            "必须是**非空字符串**数组（写占位符名，如 [\"zone\"]）。",
                        )),
                    }
                }
            }
            None => problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 request.required"),
                "必须是数组（写占位符名，如 [\"zone\"]）。",
            )),
        }
    }

    // ④ 分页路径 + 类型 + 位置基（REQ-149 · 缺口 A/B）
    if let Some(p) = obj.get("paging") {
        match p.as_object() {
            Some(p) => {
                let ploc = format!("{loc} 字段 request.paging");
                out.paging_offset = opt_str(p, "offset");
                out.paging_limit = opt_str(p, "limit");
                out.paging_page = opt_str(p, "page");

                // `offset` 与 `page` 是对同一件事的两种读法 —— 同时写就是语义打架。
                if out.paging_offset.is_some() && out.paging_page.is_some() {
                    problems.push(Problem::new(
                        CODE_SCHEMA,
                        format!("{ploc}.page"),
                        "`offset` 与 `page` 只能写**一个**：前者是「跳过多少条」，\
                         后者是「第几页」。同时写会让执行器无法判断该发哪一个\
                         （本实现 ⛔ 不按「谁先出现」猜）。",
                    ));
                }

                // 位置基：封闭取值 0 / 1（⛔ 不收任意整数）。
                if let Some(v) = p.get("offset_base") {
                    match v.as_u64() {
                        Some(0) => out.offset_base = Some(0),
                        Some(1) => out.offset_base = Some(1),
                        _ => problems.push(Problem::new(
                            CODE_SCHEMA,
                            format!("{ploc}.offset_base"),
                            format!(
                                "只能是整数 `0` 或 `1`，不认识 {v}。\
                                 `0` = 偏移量从 0 开始（默认）；`1` = 从 1 开始（按页码解释）。
                                 ⛔ 不是「任意基数」—— 本实现只表达真实存在的两种。"
                            ),
                        )),
                    }
                }

                // 分页值的 JSON 类型：封闭取值 number / string。
                if let Some(v) = p.get("value_type") {
                    out.paging_value_type = match v.as_str().map(str::trim) {
                        Some("number") => PagingValueType::Number,
                        Some("string") => PagingValueType::String,
                        other => {
                            problems.push(Problem::new(
                                CODE_SCHEMA,
                                format!("{ploc}.value_type"),
                                format!(
                                    "只能是 `number`（默认）或 `string`，不认识 {}。\
                                     文档把 `Offset/Limit` 定义成 String 的接口要写 `string`\
                                     （否则发数字会被服务端判 `InvalidParameter`）。\
                                     ⛔ 本实现不会替你猜类型。",
                                    match other {
                                        Some(s) => format!("`{s}`"),
                                        None => "非字符串值".to_string(),
                                    }
                                ),
                            ));
                            PagingValueType::Number
                        }
                    };
                }
            }
            None => problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 request.paging"),
                "必须是对象：`{\"offset\":\"Offset\",\"limit\":\"Limit\"}`（值是**路径**）。",
            )),
        }
    }

    // ⑤ 旧写法兼容：`from_filter: {"zone": "Zone"}` ⇒ 内部转成模板 + 绑定
    if let Some(ff) = obj.get("from_filter") {
        match ff.as_object() {
            Some(map) => {
                if out.template.is_some() {
                    problems.push(Problem::new(
                        CODE_SCHEMA,
                        format!("{loc} 字段 request"),
                        "`template` 与 `from_filter` 不能同时写 —— 前者是通用写法，后者只是它的\
                         兼容简写（`{\"zone\":\"Zone\"}` ⇒ `{\"Zone\":\"{zone}\"}`）。",
                    ));
                } else {
                    let mut template = Map::new();
                    for (name, v) in map {
                        let Some(param) = v.as_str() else {
                            problems.push(Problem::new(
                                CODE_SCHEMA,
                                format!("{loc} 字段 request.from_filter.{name}"),
                                "兼容写法只接受字符串（顶层参数名，如 `\"Zone\"`）。\
                                 要拼数组对象 / 深层嵌套请用 `template` + `bindings`。",
                            ));
                            continue;
                        };
                        template
                            .insert(param.to_string(), Value::String(format!("{{{name}}}")));
                        out.bindings
                            .insert(name.clone(), Binding::new(BindingSource::Filter));
                    }
                    out.template = Some(Value::Object(template));
                }
            }
            None => problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 request.from_filter"),
                "必须是对象（`{\"zone\":\"Zone\"}`）。",
            )),
        }
    }

    // ⑥ 占位符 ↔ 绑定 对账：笔误在这里抓，别等到真调用时才炸
    if let Some(t) = &out.template {
        for name in template_placeholders(t) {
            if name == "offset" || name == "limit" {
                // 分页器的两个值是**内建**的，不必显式绑定
                out.bindings.entry(name.clone()).or_insert(Binding::new(
                    BindingSource::Paging(if name == "offset" {
                        PagingValue::Offset
                    } else {
                        PagingValue::Limit
                    }),
                ));
                continue;
            }
            if !out.bindings.contains_key(&name) {
                problems.push(Problem::new(
                    CODE_SCHEMA,
                    format!("{loc} 字段 request.template"),
                    format!(
                        "模板里的占位符 `{{{name}}}` 没有对应绑定 —— \
                         加 `request.bindings.{name}`（`source` 取 `default` / `filter` / `paging`），\
                         或把它改成常量字符串。"
                    ),
                ));
            }
        }
    }

    // ⑦ `required` 必须在 bindings 里，否则永远满足不了
    for name in &out.required {
        if !out.bindings.contains_key(name) {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 request.required"),
                format!("`{name}` 不在 `request.bindings` 里 —— 必填项却没有任何取值来源。"),
            ));
        }
    }

    out
}

/// 解析一个绑定（`{"source":"default"|"filter"|"paging", "from":"offset"|"limit"}`）。
fn parse_binding(spec: &Value, loc: &str, problems: &mut Vec<Problem>) -> Option<Binding> {
    let obj = match spec.as_object() {
        Some(o) => o,
        None => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                loc,
                "必须是对象：`{\"source\":\"default\"|\"filter\"|\"paging\"}`\
                 （`paging` 还要 `\"from\":\"offset\"|\"limit\"`）。",
            ));
            return None;
        }
    };

    let source = opt_str(obj, "source").unwrap_or_default();
    let src = match source.as_str() {
        "default" => BindingSource::Default,
        "filter" => BindingSource::Filter,
        "paging" => {
            let which = opt_str(obj, "from").unwrap_or_else(|| "offset".to_string());
            match which.as_str() {
                "offset" => BindingSource::Paging(PagingValue::Offset),
                "limit" => BindingSource::Paging(PagingValue::Limit),
                other => {
                    problems.push(Problem::new(
                        CODE_SCHEMA,
                        loc,
                        format!(
                            "`source: paging` 的 `from` 只能是 `offset` 或 `limit`，不认识 `{other}`。"
                        ),
                    ));
                    return None;
                }
            }
        }
        "" => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                loc,
                "缺必填字段 `source`（`default` / `filter` / `paging`）—— \
                 不写就不知道这个值从哪来。",
            ));
            return None;
        }
        other => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                loc,
                format!(
                    "`source` 只能是 `default`（取 sources[].defaults）/ `filter`（取调用方 --filter）\
                     / `paging`（由分页器给），不认识 `{other}`。"
                ),
            ));
            return None;
        }
    };
    // REQ-149 · 缺口 C：值类型 / 形状（封闭取值；⛔ 不静默降级）。
    //
    // 只对 `default` / `filter` 生效。`paging` 的类型由 `request.paging.value_type`
    // 控制（它写的是**位置值**，不是过滤值）—— 两处都写就会有两个真相。
    let value_type = match obj.get("value_type") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match s.trim() {
            "string" => Some(BindingValueType::String),
            "integer" => Some(BindingValueType::Integer),
            "boolean" => Some(BindingValueType::Boolean),
            other => {
                problems.push(Problem::new(
                    CODE_SCHEMA,
                    format!("{loc}.value_type"),
                    format!(
                        "只能是 `string`（默认）/ `integer` / `boolean`，不认识 `{other}`。\
                         整数数组筛选（`\"N\"` 位置的数组）写 `\"value_type\":\"integer\"`；\
                         ⛔ 本实现不会把不认识的值静默当成字符串。"
                    ),
                ));
                None
            }
        },
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc}.value_type"),
                "必须是字符串 `string` / `integer` / `boolean`（或省略）。",
            ));
            None
        }
    };
    let shape = match obj.get("shape") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match s.trim() {
            "scalar" => Some(BindingShape::Scalar),
            "array" => Some(BindingShape::Array),
            other => {
                problems.push(Problem::new(
                    CODE_SCHEMA,
                    format!("{loc}.shape"),
                    format!(
                        "只能是 `scalar`（默认）或 `array`，不认识 `{other}`。\
                         `array` ⇒ 单值也包成 `[v]`（多值直接展开）；\
                         ⛔ 本实现不会把标量静默塞进数组。"
                    ),
                ));
                None
            }
        },
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc}.shape"),
                "必须是字符串 `scalar` / `array`（或省略）。",
            ));
            None
        }
    };
    if matches!(src, BindingSource::Paging(_)) && (value_type.is_some() || shape.is_some()) {
        problems.push(Problem::new(
            CODE_SCHEMA,
            loc,
            "`source: paging` 的绑定不能再写 `value_type` / `shape` —— \
             分页值的类型由 `request.paging.value_type` 统一控制。\
             两处都写会出现两个真相（本实现 ⛔ 不挑其中一个）。",
        ));
        return None;
    }

    Some(Binding { source: src, value_type, shape })
}

/// 模板里出现的全部占位符（**只认字符串值**，键名不算）。
fn template_placeholders(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => out.extend(placeholders(s)),
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            Value::Object(map) => map.values().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    walk(v, &mut out);
    out.sort();
    out.dedup();
    out
}

// ── 取值助手（每个都自己记一条**点名**的问题）──────────────────

fn need_str(
    obj: &Map<String, Value>,
    key: &str,
    loc: &str,
    problems: &mut Vec<Problem>,
) -> Option<String> {
    match obj.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(Value::String(_)) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 {key}"),
                "不能是空字符串。",
            ));
            None
        }
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 {key}"),
                "必须是字符串。",
            ));
            None
        }
        None => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 {key}"),
                "缺必填字段。",
            ));
            None
        }
    }
}

fn opt_str(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn opt_u64(obj: &Map<String, Value>, key: &str) -> Option<u64> {
    obj.get(key).and_then(|v| v.as_u64())
}

fn need_array<'a>(
    obj: &'a Map<String, Value>,
    key: &str,
    loc: &str,
    problems: &mut Vec<Problem>,
) -> Option<&'a Vec<Value>> {
    match obj.get(key) {
        Some(Value::Array(a)) => Some(a),
        Some(_) => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 {key}"),
                "必须是数组。",
            ));
            None
        }
        None => {
            problems.push(Problem::new(
                CODE_SCHEMA,
                format!("{loc} 字段 {key}"),
                "缺必填数组。",
            ));
            None
        }
    }
}

fn opt_array<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Vec<Value>> {
    obj.get(key).and_then(|v| v.as_array())
}

fn opt_str_list(obj: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    obj.get(key)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect())
}

// ══════════════════════════════════════════════════════════════
// 本体（TTL）—— 只解出 validate 需要的两样东西
// ══════════════════════════════════════════════════════════════

/// 从 TTL 里解出来的本体事实（v1 只需要「类 → 实例 IRI 模板」）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Ontology {
    /// 本体 IRI（`@prefix :` 去掉尾部 `#`；没有 `:` 前缀时退化为 `owl:Ontology` 的 subject）。
    pub iri: String,
    /// 前缀表（`@prefix p: <iri> .`）。
    pub prefixes: BTreeMap<String, String>,
    /// 类本地名 → 声明。
    pub classes: BTreeMap<String, ClassDecl>,
}

/// 一个类的声明。
#[derive(Debug, Clone, Serialize)]
pub struct ClassDecl {
    /// 类的完整 IRI。
    pub iri: String,
    /// `ontok:instanceIri` 的**原值**（含 `{class}` / `{id}` 占位符）。没写 ⇒ `None`。
    pub instance_iri: Option<String>,
}

impl Ontology {
    /// 按类名取声明：先按本地名，再按完整 IRI，最后按 `:本地名` / 前缀写法。
    pub fn class(&self, name: &str) -> Option<&ClassDecl> {
        let n = name.trim();
        if let Some(c) = self.classes.get(n) {
            return Some(c);
        }
        let stripped = n.strip_prefix(':').unwrap_or(n);
        if let Some(c) = self.classes.get(stripped) {
            return Some(c);
        }
        // 前缀写法 `p:local` / 完整 IRI `<...>`
        let iri = self.resolve(n);
        self.classes.values().find(|c| c.iri == iri)
    }

    /// 把 `p:local` / `<iri>` 解析成完整 IRI（解析不了就原样返回）。
    pub fn resolve(&self, token: &str) -> String {
        let t = token.trim();
        if let Some(inner) = t.strip_prefix('<').and_then(|x| x.strip_suffix('>')) {
            return inner.to_string();
        }
        if let Some((p, local)) = t.split_once(':') {
            if let Some(ns) = self.prefixes.get(p) {
                return format!("{ns}{local}");
            }
        }
        t.to_string()
    }

    /// 类的本地名（`<...>#云资源` → `云资源`；也认 `/` 结尾的命名空间）。
    ///
    /// 顺带认前缀写法：`:云资源` → `云资源`（⛔ 别让冒号混进类名）。
    pub fn local_name(iri: &str) -> &str {
        let s = iri.strip_prefix(':').unwrap_or(iri);
        match s.rfind(['#', '/']) {
            Some(i) => &s[i + 1..],
            None => s,
        }
    }
}

/// 解析 TTL（**只认本仓序列化器的输出形状**，见模块头「已知限制」）。
pub fn parse_ontology(text: &str) -> Ontology {
    let prefixes = parse_prefixes(text);
    let triples = parse_triples(&strip_comments(text));

    // 本体 IRI：优先 `:` 前缀（去掉尾部 `#`），否则取 `X a owl:Ontology` 的 subject。
    let mut iri = prefixes
        .get("")
        .map(|ns| ns.trim_end_matches('#').to_string())
        .unwrap_or_default();
    if iri.is_empty() {
        for (s, p, o) in &triples {
            if p == "a" && o.ends_with("Ontology") {
                iri = s.clone();
                break;
            }
        }
    }

    let mut classes: BTreeMap<String, ClassDecl> = BTreeMap::new();
    // ⚠️ subject 可能是**前缀写法**（`:云资源`）或**完整 IRI**（`<https://…#云资源>`）：
    // 先解析成完整 IRI 再取本地名，否则 `:云资源` 会带着冒号变成类名（`":云资源"`），
    // 而契约里写的 `云资源` 就永远找不到它。
    for (s, p, o) in &triples {
        if p == "a" && (o == "owl:Class" || o.ends_with("#Class")) {
            let iri = resolve_in(&prefixes, s);
            let local = Ontology::local_name(&iri).to_string();
            classes.entry(local).or_insert_with(|| ClassDecl { iri, instance_iri: None });
        }
    }
    // `ontok:instanceIri`：按**解析后的谓词 IRI** 认（前缀叫什么不重要）。
    for (s, p, o) in &triples {
        let pred = resolve_in(&prefixes, p);
        if !pred.ends_with("instanceIri") {
            continue;
        }
        let iri = resolve_in(&prefixes, s);
        let local = Ontology::local_name(&iri).to_string();
        let entry = classes
            .entry(local)
            .or_insert_with(|| ClassDecl { iri, instance_iri: None });
        if entry.instance_iri.is_none() {
            entry.instance_iri = Some(unquote(o));
        }
    }

    Ontology { iri, prefixes, classes }
}

fn resolve_in(prefixes: &BTreeMap<String, String>, token: &str) -> String {
    let t = token.trim();
    if let Some(inner) = t.strip_prefix('<').and_then(|x| x.strip_suffix('>')) {
        return inner.to_string();
    }
    if let Some((p, local)) = t.split_once(':') {
        if let Some(ns) = prefixes.get(p) {
            return format!("{ns}{local}");
        }
    }
    t.to_string()
}

/// 去掉 Turtle 注释（`#` 到行尾），但**保留** `<...>` 与 `"..."` 里的内容。
///
/// ⚠️ 必须认 `<...>`：TTL 的命名空间本身带 `#`（`<https://x#y>`），
/// 把它当注释起点会把整份文件切碎 —— 那种失败是「所有类都找不到」、
/// 看起来像「本体里没写 instanceIri」，与实际原因差得很远。
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    #[derive(PartialEq)]
    enum St {
        Code,
        Iri,
        Str,
        Comment,
    }
    let mut st = St::Code;
    let mut escaped = false;
    for ch in text.chars() {
        match st {
            St::Comment => {
                if ch == '\n' {
                    out.push(ch);
                    st = St::Code;
                }
            }
            St::Str => {
                out.push(ch);
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    st = St::Code;
                }
            }
            St::Iri => {
                out.push(ch);
                if ch == '>' {
                    st = St::Code;
                }
            }
            St::Code => match ch {
                '#' => st = St::Comment,
                '"' => {
                    st = St::Str;
                    out.push(ch);
                }
                '<' => {
                    st = St::Iri;
                    out.push(ch);
                }
                _ => out.push(ch),
            },
        }
    }
    out
}

/// `@prefix p: <iri> .`（`:` 前缀的 key 是空串）。
fn parse_prefixes(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("@prefix ").or_else(|| t.strip_prefix("PREFIX ")) else {
            continue;
        };
        let Some(colon) = rest.find(':') else { continue };
        let name = rest[..colon].trim().to_string();
        let after = rest[colon + 1..].trim();
        let Some(start) = after.find('<') else { continue };
        let Some(end) = after[start..].find('>') else { continue };
        let iri = after[start + 1..start + end].to_string();
        out.insert(name, iri);
    }
    out
}

/// 语句 → 三元组。**按 `;` / `.` 切**（跳过 `<>` 与 `""` 里的标点）。
///
/// ⚠️ 第一段里**同时**住着 subject 与第一个 `pred obj`：
/// `<s> a owl:Class ;` 切出来的第一段是 `"<s> a owl:Class"`，
/// 不是 `"<s>"`。把整段当 subject 会让 `a owl:Class` 这条三元组**丢失**，
/// 于是「本体里一个类都没有」—— 那种失败看起来像「TTL 没写类」，差得很远。
fn parse_triples(text: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for stmt in split_statements(text) {
        let parts = split_objects(&stmt);
        let Some(first) = parts.first().map(|s| s.trim().to_string()) else {
            continue;
        };
        if first.is_empty() {
            continue;
        }
        // subject = 第一个空白之前的记号；余下的部分是该语句的**第一个** `pred obj`。
        let (subject, pairs) = match first.find(char::is_whitespace) {
            Some(at) => {
                let head = first[..at].to_string();
                let tail = first[at..].trim().to_string();
                let mut v: Vec<String> = Vec::new();
                if !tail.is_empty() {
                    v.push(tail);
                }
                v.extend(parts[1..].iter().cloned());
                (head, v)
            }
            None => (first, parts[1..].to_vec()),
        };
        for pair in pairs {
            let Some(pred) = pair.split_whitespace().next() else {
                continue;
            };
            let obj = pair[pred.len()..].trim().to_string();
            out.push((subject.clone(), pred.to_string(), obj));
        }
    }
    out
}

/// 按顶层 `.` 切语句（`<...>` / `"..."` 里的点不算）。
fn split_statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_iri = false;
    let mut in_str = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_str {
            cur.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        if in_iri {
            cur.push(ch);
            if ch == '>' {
                in_iri = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                cur.push(ch);
            }
            '<' => {
                in_iri = true;
                cur.push(ch);
            }
            '.' => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// 按顶层 `;` 切「subject / pred obj / pred obj …」。
fn split_objects(stmt: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_iri = false;
    let mut in_str = false;
    let mut escaped = false;
    for ch in stmt.chars() {
        if in_str {
            cur.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        if in_iri {
            cur.push(ch);
            if ch == '>' {
                in_iri = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                cur.push(ch);
            }
            '<' => {
                in_iri = true;
                cur.push(ch);
            }
            ';' => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// 去掉字面量两侧的引号与语言标签（`"…"@zh` → `…`）。
pub fn unquote(lit: &str) -> String {
    let t = lit.trim();
    let Some(inner) = t.strip_prefix('"') else { return t.to_string() };
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    out
}

// ══════════════════════════════════════════════════════════════
// IRI 模板（R3 的两边都在这里成形）
// ══════════════════════════════════════════════════════════════

/// 把映射里的 `ontok:` 前缀展开成本体的实例命名空间。
///
/// 为什么是 `ontok:` → `<ontology_iri>#` 而不是读 TTL 的 `@prefix ontok:`：
/// 本仓 TTL 里的 `ontok:` 是**模式命名空间**（`https://ontok.local/schema/`），
/// 而 `query serve` 的 JSON-LD `@context` 里 `ontok` 是**实例命名空间**
/// （`<ontology_iri>#`，见 `query::jsonld::ONTOK_PREFIX`）。
/// 两处同名不同义 —— 展开时必须跟 JSON-LD 那一套，否则拼出来的 IRI 与图对不上。
pub fn expand_iri_template(template: &str, ontology_iri: &str) -> String {
    let t = template.trim();
    match t.strip_prefix("ontok:") {
        Some(rest) => format!("{ontology_iri}#{rest}"),
        None => t.to_string(),
    }
}

/// 本体的 `ontok:instanceIri` **按类展开**（`{class}` → 类本地名）。
pub fn expand_ttl_instance_iri(raw: &str, class_local: &str) -> String {
    raw.replace("{class}", class_local)
}

/// IRI 模板里的占位符（`{id}` → `id`）。
pub fn placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else { break };
        out.push(rest[start + 1..start + end].to_string());
        rest = &rest[start + end + 1..];
    }
    out
}

/// 用 `key` 填模板里的 `{id}`（也认 `{class}`）。
///
/// IRI 里不该出现裸的 `/`、`#`、空格（键值来自云 API，理论上是 ID，但⛔ 不假设它干净）：
/// 这里只做**最小**转义（`/` `#` `?` 空格 → `%2F` 之类），保证拼出的 IRI 一定可解析。
pub fn fill_iri_template(template: &str, class_local: &str, key: &str) -> String {
    let safe = escape_iri_component(key);
    let mut out = template.replace("{class}", class_local);
    for ph in placeholders(template) {
        if ph == "class" {
            continue;
        }
        out = out.replace(&format!("{{{ph}}}"), &safe);
    }
    out
}

/// RFC 3987 的最小转义：只处理会破坏 IRI 结构的字符。
fn escape_iri_component(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for ch in key.chars() {
        match ch {
            '/' => out.push_str("%2F"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            ' ' => out.push_str("%20"),
            '%' => out.push_str("%25"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> String {
        // ⚠️ 用 `to_string_pretty`：下面的变异都用 `"\"key\": \"value\","` 这种**带空格**的
        // 锚点做 `replace`，而 `to_string` 是紧凑格式（`"key":"value"`）⇒ 锚点不命中、
        // 变异静默失效、测试变成「什么都没改却断言它是坏的」。
        serde_json::to_string_pretty(&serde_json::json!({
            "contract_version": 1,
            "project": "ops360",
            "sources": [{ "id": "s1", "base_url": "https://x" }],
            "entities": [{
                "id": "cvm_instance",
                "class": "cvm_instance",
                "source": "s1",
                "reviewed_by": "张三@2026-10-25",
                "operation": { "action": "SomeAction" },
                "iterator": "$.Response.InstanceSet[*]",
                "identity": { "key_path": "InstanceId" },
                "iri_template": "ontok:cvm_instance/{id}",
                "fields": { "name": { "path": "InstanceName" } },
                "capability": { "pushdown": [], "local_only": [], "max_scan_rows": 10 },
                "freshness": { "mode": "realtime" },
                "fixture": "f.json"
            }]
        }))
        .expect("测试夹具本身必须是合法 JSON")
    }

    #[test]
    fn parse_minimal_ok() {
        let m = parse(&minimal()).expect("最小契约应当解析成功");
        assert_eq!(m.contract_version, CONTRACT_VERSION);
        assert_eq!(m.mapping_version, 1, "缺省映射版本 = 1");
        assert_eq!(m.entities[0].key_path, "InstanceId");
        assert!(m.entities[0].reviewed());
    }

    // ── `request` 段：模板 + 绑定（F4）────────────────────────────

    /// 往最小契约里塞一个 `request` 段。
    fn with_request(request: serde_json::Value) -> String {
        let mut v: serde_json::Value = serde_json::from_str(&minimal()).unwrap();
        v["entities"][0]["request"] = request;
        serde_json::to_string(&v).unwrap()
    }

    #[test]
    fn request_template_and_bindings_parse() {
        let m = parse(&with_request(serde_json::json!({
            "required": ["zone"],
            "template": {
                "Region": "{region}",
                "Clauses": [ { "Field": "zone", "Values": ["{zone}"] } ]
            },
            "bindings": {
                "region": { "source": "default" },
                "zone":   { "source": "filter" },
                "page":   { "source": "paging", "from": "offset" }
            },
            "paging": { "offset": "Page.Number", "limit": "Page.Size" }
        })))
        .expect("模板契约应当解析成功");

        let spec = &m.entities[0].request;
        let t = spec.template.as_ref().expect("模板必须在");
        assert_eq!(t["Region"], serde_json::json!("{region}"));
        assert_eq!(t["Clauses"][0]["Values"][0], serde_json::json!("{zone}"));
        assert_eq!(spec.bindings["region"].source, BindingSource::Default);
        assert_eq!(spec.bindings["zone"].source, BindingSource::Filter);
        assert_eq!(
            spec.bindings["page"].source,
            BindingSource::Paging(PagingValue::Offset)
        );
        assert_eq!(spec.required, vec!["zone".to_string()]);
        assert_eq!(spec.paging_offset.as_deref(), Some("Page.Number"));
        assert_eq!(spec.paging_limit.as_deref(), Some("Page.Size"));
    }

    /// 旧写法 `from_filter` **内部转成模板**（⛔ 不是另一条运行时代码路径）。
    #[test]
    fn request_from_filter_legacy_converts_to_template() {
        let m = parse(&with_request(serde_json::json!({
            "from_filter": { "zone": "Zone", "ids": "InstanceIds" }
        })))
        .expect("旧写法必须仍然能解析");

        let spec = &m.entities[0].request;
        let t = spec.template.as_ref().expect("旧写法应当被转成模板");
        assert_eq!(t["Zone"], serde_json::json!("{zone}"));
        assert_eq!(t["InstanceIds"], serde_json::json!("{ids}"));
        assert_eq!(spec.bindings["zone"].source, BindingSource::Filter);
        assert_eq!(spec.bindings["ids"].source, BindingSource::Filter);
    }

    #[test]
    fn request_rejects_unsupported_shapes_and_typos_by_name() {
        // ① 非 JSON 模板 ⇒ REQUEST_SHAPE_UNSUPPORTED（⛔ 不偷偷适配）
        let errs = parse(&with_request(serde_json::json!({
            "template": "<Request><Zone>x</Zone></Request>"
        })))
        .unwrap_err();
        assert!(
            errs.iter()
                .any(|p| p.code == CODE_REQUEST_SHAPE
                    && p.location.contains("request.template")),
            "非对象模板必须报 REQUEST_SHAPE_UNSUPPORTED：{errs:?}"
        );

        // ② 占位符没有绑定 ⇒ 解析期就点名（别等真调用）
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Zone": "{zone}" }
        })))
        .unwrap_err();
        assert!(
            errs.iter()
                .any(|p| p.message.contains("占位符 `{zone}` 没有对应绑定")),
            "要点名是哪个占位符没绑定：{errs:?}"
        );

        // ③ source 认不出来 ⇒ 点名三个合法值
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Zone": "{zone}" },
            "bindings": { "zone": { "source": "magic" } }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.message.contains("`source` 只能是")),
            "要点名合法来源：{errs:?}"
        );

        // ④ required 里写了没有绑定的名字
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Zone": "fixed" },
            "required": ["zone"]
        })))
        .unwrap_err();
        assert!(
            errs.iter()
                .any(|p| p.message.contains("`zone` 不在 `request.bindings` 里")),
            "{errs:?}"
        );

        // ⑤ template 与 from_filter 同时写 ⇒ 歧义，拒绝
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Zone": "{zone}" },
            "bindings": { "zone": { "source": "filter" } },
            "from_filter": { "zone": "Zone" }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.message.contains("不能同时写")),
            "{errs:?}"
        );
    }

    // ── REQ-149 · 新字段的封闭取值 / 不支持组合（R3/R4）────────────

    /// 新字段写错 / 写错位置 ⇒ **报错并点名**，⛔ 不静默降级。
    ///
    /// 变异：把 `parse_request_spec` 里任何一处 `problems.push` 删掉
    /// ⇒ 对应的 `unwrap_err()` 会变成 `Ok` ⇒ 这条测试立刻红。
    #[test]
    fn req149_new_fields_are_closed_sets_and_named_on_bad_combinations() {
        // ① 分页 value_type 只认 number / string
        let errs = parse(&with_request(serde_json::json!({
            "paging": { "offset": "Offset", "value_type": "float" }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("request.paging.value_type")
                && p.message.contains("number")),
            "{errs:?}"
        );

        // ② offset 与 page 同时写 ⇒ 语义打架，拒绝
        let errs = parse(&with_request(serde_json::json!({
            "paging": { "offset": "Offset", "page": "PageNumber" }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.message.contains("只能写**一个**")),
            "{errs:?}"
        );

        // ③ offset_base 只认 0 / 1
        let errs = parse(&with_request(serde_json::json!({
            "paging": { "offset": "Offset", "offset_base": 2 }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("offset_base")),
            "{errs:?}"
        );

        // ④ 绑定 value_type 的封闭集合（⛔ 没有 number）
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Status": "{status}" },
            "bindings": { "status": { "source": "filter", "value_type": "number" } }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("bindings.status.value_type")
                && p.message.contains("integer")),
            "{errs:?}"
        );

        // ⑤ 绑定 shape 的封闭集合
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Status": "{status}" },
            "bindings": { "status": { "source": "filter", "shape": "map" } }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("bindings.status.shape")),
            "{errs:?}"
        );

        // ⑥ 分页绑定上再写类型 ⇒ 两个真相，拒绝
        let errs = parse(&with_request(serde_json::json!({
            "template": { "Offset": "{offset}" },
            "bindings": {
                "offset": { "source": "paging", "from": "offset", "value_type": "string" }
            }
        })))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| p.message.contains("request.paging.value_type")),
            "{errs:?}"
        );

        // ⑦ 写到**实体级** `paging` 里 ⇒ 点名「挪进 request.paging」（⛔ 不静默忽略）
        let mut v: serde_json::Value = serde_json::from_str(&minimal()).unwrap();
        v["entities"][0]["paging"] = serde_json::json!({
            "offset_param": "Offset", "limit_param": "Limit", "value_type": "string"
        });
        let errs = parse(&serde_json::to_string(&v).unwrap()).unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("paging.value_type")
                && p.message.contains("request.paging")),
            "{errs:?}"
        );

        // ⑧ fixture_source 的封闭集合
        let bad = minimal().replace(
            "\"fixture\": \"f.json\"",
            "\"fixture\": \"f.json\", \"fixture_source\": \"guessed\"",
        );
        let errs = parse(&bad).unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("fixture_source")
                && p.message.contains("declared")),
            "{errs:?}"
        );
    }

    #[test]
    fn missing_field_names_the_entity_and_field() {
        let bad = minimal().replace("\"iri_template\": \"ontok:cvm_instance/{id}\",", "");
        let errs = parse(&bad).unwrap_err();
        let p = errs
            .iter()
            .find(|p| p.location.contains("iri_template"))
            .unwrap_or_else(|| panic!("必须点名 iri_template：{errs:?}"));
        assert_eq!(p.code, CODE_SCHEMA);
        assert!(p.location.contains("entities[0]"), "位置要点名第几条：{}", p.location);
        assert!(p.location.contains("cvm_instance"), "位置要点名哪个 entity：{}", p.location);
    }

    #[test]
    fn reviewed_by_must_be_present_even_if_null() {
        let bad = minimal().replace("\"reviewed_by\": \"张三@2026-10-25\",", "");
        let errs = parse(&bad).unwrap_err();
        assert!(
            errs.iter().any(|p| p.location.contains("reviewed_by")),
            "缺 reviewed_by 必须点名（⛔ 不写 ≠ 不用签）：{errs:?}"
        );
        // 显式 null 是合法状态（只是不许取数）
        let ok = minimal().replace("\"张三@2026-10-25\"", "null");
        let m = parse(&ok).unwrap();
        assert!(!m.entities[0].reviewed());
    }

    #[test]
    fn capability_must_be_explicit() {
        let bad = minimal().replace("\"local_only\": [],", "");
        let errs = parse(&bad).unwrap_err();
        assert!(errs.iter().any(|p| p.code == CODE_SCHEMA && p.location.contains("local_only")));
    }

    #[test]
    fn source_reference_must_exist() {
        let bad = minimal().replace("\"source\": \"s1\"", "\"source\": \"nope\"");
        let errs = parse(&bad).unwrap_err();
        assert!(errs.iter().any(|p| p.location.contains("source")), "{errs:?}");
    }

    #[test]
    fn contract_version_is_gated() {
        let bad = minimal().replace("\"contract_version\": 1", "\"contract_version\": 9");
        let errs = parse(&bad).unwrap_err();
        assert!(errs.iter().any(|p| p.message.contains("契约版本 9")), "{errs:?}");
    }

    // ── ⭐ REQ-152 · 契约 v2（加法式）：operation 泛化 + auth 补声明 ──────────

    /// 最小契约改一处再序列化（⛔ 不用字符串 replace：pretty 缩进一变锚点就静默失效）。
    fn v2_with(f: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut v: serde_json::Value = serde_json::from_str(&minimal()).unwrap();
        v["contract_version"] = serde_json::json!(CONTRACT_VERSION_V2);
        f(&mut v);
        serde_json::to_string(&v).unwrap()
    }

    /// v2 要能加载，且**记住声明的版本**（`get` 的配方门槛读它）。
    #[test]
    fn contract_v2_is_accepted_and_remembered() {
        let v2 = parse(&v2_with(|_| {})).expect("v2 是加法式：必须能加载");
        assert_eq!(v2.contract_version, CONTRACT_VERSION_V2);
        assert!(v2.has_recipe(), "v2 ⇒ 允许给完整配方");
        assert!(!parse(&minimal()).expect("v1").has_recipe(), "v1 ⇒ 旧 4 键，一个不多");
        assert!(parse(&minimal().replace("\"contract_version\": 1", "\"contract_version\": 3")).is_err());
    }

    /// `headers{}` + `vars{}`：厂商概念只是**数据**，`action` 可以缺（v2）。
    #[test]
    fn operation_headers_and_vars_are_data() {
        let m = parse(&v2_with(|v| {
            v["entities"][0]["operation"] = serde_json::json!({
                "method": "POST", "path": "/",
                "headers": { "X-Api-Target": "{target}", "Content-Type": "application/json" },
                "vars": { "target": "DescribeInstances" }
            });
        }))
        .expect("v2 的 operation 只写 headers/vars 也合法");
        let op = &m.entities[0].operation;
        assert_eq!(op.action, "", "没有 action ⇒ 空串（⛔ 不编一个动作名）");
        assert_eq!(op.path.as_deref(), Some("/"));
        assert_eq!(
            op.headers.as_ref().unwrap().get("X-Api-Target").map(String::as_str),
            Some("{target}"),
            "header 名原样透出：映射写什么就是什么"
        );
        assert_eq!(op.vars.as_ref().unwrap()["target"], "DescribeInstances");

        // v1 仍然要求 `action`（基线行为不变）。
        let mut v: serde_json::Value = serde_json::from_str(&minimal()).unwrap();
        v["entities"][0]["operation"] = serde_json::json!({ "method": "POST" });
        let errs = parse(&serde_json::to_string(&v).unwrap()).unwrap_err();
        assert!(errs.iter().any(|p| p.location.contains("operation")), "{errs:?}");

        // header 模板的值不是字符串 ⇒ 点名，⛔ 不静默丢。
        let errs = parse(&v2_with(|v| {
            v["entities"][0]["operation"] =
                serde_json::json!({ "action": "A", "headers": { "X": 1 } });
        }))
        .unwrap_err();
        assert!(errs.iter().any(|p| p.location.contains("headers.X")), "{errs:?}");
    }

    /// `auth` 的三项 v2 声明：给了就解析，**没给就不出现**（⛔ 不按 mode 猜）。
    #[test]
    fn auth_declarations_are_optional_and_never_inferred() {
        let m = parse(&v2_with(|v| {
            v["sources"][0]["auth"] = serde_json::json!({
                "mode": "tc3-hmac-sha256",
                "credential_env": ["PROVIDER_ACCESS_KEY_ID", "PROVIDER_SECRET_KEY"],
                "signed_headers": ["content-type", "host", "x-api-action"],
                "timestamp": { "header": "X-Api-Timestamp", "format": "unix-seconds" },
                "scope": ["date", "service", "tc3_request"]
            });
        }))
        .expect("v2 + 完整 auth 声明");
        let a = m.sources[0].auth.as_ref().expect("auth");
        assert_eq!(a.signed_headers.as_ref().unwrap().len(), 3);
        let ts = a.timestamp.as_ref().expect("timestamp");
        assert_eq!((ts.header.as_str(), ts.format.as_str()), ("X-Api-Timestamp", "unix-seconds"));
        assert_eq!(a.scope.as_ref().unwrap().len(), 3);
        assert_eq!(
            a.credential_env,
            vec!["PROVIDER_ACCESS_KEY_ID", "PROVIDER_SECRET_KEY"],
            "只有**变量名**"
        );
        assert!(
            !serde_json::to_string(a).unwrap().contains("\"value\""),
            "⛔ 凭据值字段不许出现在契约里"
        );

        // 有 auth、没有三项声明 ⇒ 三个键都不出现（AC-106 在映射层的判据）。
        let m = parse(&v2_with(|v| {
            v["sources"][0]["auth"] = serde_json::json!({ "mode": "m", "credential_env": ["A"] });
        }))
        .expect("v2 + 最小 auth");
        let a = m.sources[0].auth.as_ref().unwrap();
        assert!(a.signed_headers.is_none() && a.timestamp.is_none() && a.scope.is_none());
        let text = serde_json::to_string(a).unwrap();
        assert!(!text.contains("signed_headers") && !text.contains("timestamp"), "{text}");

        // `timestamp` 缺 `header` ⇒ 点名（否则客户端不知道往哪个 header 放时间戳）。
        let errs = parse(&v2_with(|v| {
            v["sources"][0]["auth"] =
                serde_json::json!({ "mode": "m", "timestamp": { "format": "unix-seconds" } });
        }))
        .unwrap_err();
        assert!(
            errs.iter().any(|p| {
                p.location.contains("auth.timestamp") && p.location.contains("header")
            }),
            "{errs:?}"
        );
    }

    #[test]
    fn ontology_parses_class_and_template() {
        let ttl = r#"
@prefix : <https://ontok.local/ops360#> .
@prefix ontok: <https://ontok.local/schema/> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

<https://ontok.local/ops360> a owl:Ontology .

:云资源 a owl:Class ;
    rdfs:label "云资源"@zh ;
    ontok:instanceIri "https://ontok.local/ops360#{class}/{id}" .
"#;
        let o = parse_ontology(ttl);
        assert_eq!(o.iri, "https://ontok.local/ops360");
        let c = o.class("云资源").expect("要能按本地名取到类");
        assert_eq!(c.iri, "https://ontok.local/ops360#云资源");
        assert_eq!(
            c.instance_iri.as_deref(),
            Some("https://ontok.local/ops360#{class}/{id}")
        );
        // 注释里的 `#` 不该把 `<...>` 切开
        assert!(o.prefixes.contains_key("ontok"));
    }

    #[test]
    fn iri_template_expansion_and_fill() {
        assert_eq!(
            expand_iri_template("ontok:cvm_instance/{id}", "https://ontok.local/ops360"),
            "https://ontok.local/ops360#cvm_instance/{id}"
        );
        assert_eq!(
            expand_iri_template("https://x#y/{id}", "https://ontok.local/ops360"),
            "https://x#y/{id}"
        );
        assert_eq!(
            fill_iri_template("https://x#cvm_instance/{id}", "cvm_instance", "ins-1"),
            "https://x#cvm_instance/ins-1"
        );
        assert_eq!(
            fill_iri_template("https://x#{class}/{id}", "云资源", "a/b"),
            "https://x#云资源/a%2Fb",
            "键值里的 `/` 必须转义，否则 IRI 多一层路径"
        );
        assert_eq!(placeholders("a/{id}/b/{class}"), vec!["id", "class"]);
    }

    #[test]
    fn ontology_local_name() {
        assert_eq!(Ontology::local_name("https://x#云资源"), "云资源");
        assert_eq!(Ontology::local_name("https://x/云资源"), "云资源");
        assert_eq!(Ontology::local_name("云资源"), "云资源");
    }
}
