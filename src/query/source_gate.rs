//! REQ-145 · **源闸（`query serve --access-mapping <file>`）** —— API 源声明的加载与判定。
//!
//! # 为什么这个模块叫 `source_gate` 而不是 `access`
//!
//! 仓里已经有一个 `ontok::access`（REQ-144：**API 源的映射契约** —— 那是资产/格式）；
//! 本模块干的是另一件事：**判定「这条查询该走哪个源」**，走不通就拦下来
//! （`NOT_DB_SOURCE` / `CROSS_SOURCE_UNSUPPORTED`）。两个都叫 `access` 编译不冲突，
//! 但两个都叫 `access` 的模块并排出现时（契约 vs 判定），人会愣 ——
//! 所以这里按**职责**命名；契约那个名字不动（它贴切）。
//!
//! # 一句话
//!
//! 平台自己算得完的（库源）算完；**算不了的（api 源）显式标出来并给取数指引**。
//! 这份模块回答三个问题：
//!
//! | # | 问题 | 用在哪 |
//! | --- | --- | --- |
//! | ① | 哪些类来自 API？ | TTL 的 `ontok:sourceKind "api"`、`catalog`/`describe` 的 `access` |
//! | ② | 这条查询会不会碰到它们？ | `@pending` / `NOT_DB_SOURCE` / `CROSS_SOURCE_UNSUPPORTED` |
//! | ③ | 取哪一条、按什么取？ | `@pending[].get`（`class` / `filter` / `pushdown` / `ref`） |
//!
//! # ⛔ 本模块不做什么
//!
//! - **不调 API**：凭据不出客户边界（需求「不做」第 1 条）。平台只给**取数指引**。
//! - **不写回**：拿到的数据不落库（契约 §四 规则 5）。
//! - **不做跨源 join / 联邦**：跨源一律 `CROSS_SOURCE_UNSUPPORTED`，由 Agent 拆两段（契约 §七）。
//! - **不往 TTL 里写端点 / 参数 / 凭据**：TTL 只拿 [`crate::domain::ttl::ApiClasses`]，
//!   结构上就没有别的东西可漏（需求 §1 的禁令）。
//!
//! # 映射文件（最小形状）
//!
//! ```json
//! { "contract_version": 1, "mapping_version": 2,
//!   "entities": [
//!     { "id": "Customer", "class": "customers",
//!       "capability": { "pushdown": ["ids"] }, "freshness": "realtime" } ] }
//! ```
//!
//! ⭐ REQ-152 · **`freshness` 收两种写法**（一份 schema，两个解析器不再打架）：
//! 上面那种字符串，或 access 契约 / 全量 AM 的对象
//! `"freshness": { "mode": "realtime", "note": "…" }`。两种都取 `mode` 归一
//! （见 [`RawFreshness`]）。所以 `--access-mapping` 可以直接吃**全量 AM**，
//! 不再需要 `Deploy/.../gen-serve-mapping.py` 造一份投影。
//!
//! - `mapping_version` **必填**（`meta.mapping_version` 与 `get.ref` 的 `@v<n>` 都取它，
//!   用于「映射与本体版本对得上」的追溯 —— 契约规则 3）；
//! - `entities[].class` 是**本体里的类 key**（= 快照 `entity_types.key`，`catalog` 的 `table`）；
//!   它必须在**这一版本体里真实存在**，否则拒绝启动（⛔ 不静默忽略一条映射）；
//! - `entities[].id` 是 access 契约里的条目名（`get.ref` = `access#<id>@v<version>`）；
//! - `capability.pushdown` ⇒ `access.pushdown_fields` / `get.pushdown`（省略 = `[]`，
//!   ⛔ 不替调用方猜一个能力）；
//! - `freshness` 可选（省略 ⇒ `access` 里不出现这个键，⛔ 不编一个「实时」）。
//!   对象写法里没有 `mode` ⇒ **与省略同义**（同样不编）。
//!
//! 契约里其余字段（`source` / `operation` / `iterator` / `iri_template` / `fields`）
//! 本模块**读进来就丢**：它们只在 access 契约里生效，平台既不执行也不转发
//! （把 `operation` 写进 `@pending` 等于把「怎么直连我们数据源」交给调用方 —— 契约规则 6）。
//!
//! # 跨源边怎么来的（**不写在映射文件里**）
//!
//! 边是**本体里的事实**，源是**映射里的事实**；两者一交叉就知道哪条边跨源：
//! 「两端一条在 api、一条在库」⇒ 跨源。让映射文件再列一遍边，等于把同一件事写两份
//! （本体改了映射不会跟着改，而且不会报错）。所以这里由 [`AccessMapping::bind`]
//! 从**本体**推导，映射文件里**没有**边的字段。
//!
//! # 一条查询落到哪一类（[`SourceCheck`]）
//!
//! | 查询里出现 | 判定 | 两种格式的行为 |
//! | --- | --- | --- |
//! | 只有库源类 | [`SourceCheck::DbOnly`] | 正常执行（`meta.complete: true`） |
//! | api 类被**按类点名**（`?c a :customers` / 它的属性） | [`SourceCheck::Api`] | jsonld ⇒ `@graph: []` + `@pending`（**带** `get`）；rows ⇒ **`NOT_DB_SOURCE`** |
//! | 只走**跨源边**（`?o :order__customers ?c`，没有点名 api 类） | [`SourceCheck::Api`]（`cross_source_edge`） | jsonld ⇒ `@pending`（**不带** `get`，但带 `ref` + `local_hint` + 三步 `hint`）；rows ⇒ `NOT_DB_SOURCE` |
//! | 跨源边**还被要求往下走**（点名的 api 类 / 它的属性） | [`SourceCheck::CrossSource`] | 两种格式都 ⇒ **`CROSS_SOURCE_UNSUPPORTED`** |
//!
//! ⭐ REQ-151 · `Api` 还带一个 `needs_db`（**混合查询** = 同时点了库类 / 走了跨源边）。
//! 普通模式不看它（REQ-145 的「混合 ⇒ 整体 pending」不变）；**本体服务模式**
//! （`--ontology-only`）只有纯 api（`needs_db=false`）才给 `@pending`，混合 ⇒
//! 503 `NO_DATA_SOURCE`（那个部署确实没有库，给一半指引会误导调用方以为数据齐了）。
//!
//! 「不带 `get`」的依据是契约规则 4：跨源边给了能力也**查不了**（平台不做跨源 join），
//! 给了比不给更糟 —— 所以只说明「这里少了一块、要拆两段」，不给一条走不通的指令。
//!
//! # ⚠️ 已知限制（写死，别当成 bug）
//!
//! | # | 限制 | 为什么现在这样 |
//! | --- | --- | --- |
//! | 1 | **跨源边不下发 `get`**（只给 `ref` + `local_hint`） | 一条「带 ids 的 `get`」需要规划器多一条「只读 child 列」的通路；本期由 Agent 自己按 `local_hint.get_ids_by` 去取（阶段 2 再做） |
//! | 2 | **一条查询同时涉及库源实体与 API 类 ⇒ 整个查询 pending、不执行任何 SQL**，`@graph` 恒空 | 部分求值要拆查询计划（阶段 2）。⛔ 不塞半份库源结果伪装成「算完了」——半个答案必须有 `complete:false` 兜着 |
//! | 3 | `local_hint.get_ids_by` **不含租户条件** | 租户口径属于调用方自己（也避免把租户 id 写进半个答案）；`hint` 里明说「按你的租户过滤条件收窄」 |
//! | 4 | 跨源边的 FK 只在「库是 child」时才有 `local_hint.uses_column` | 反过来的形状（api 类是 child）里 FK 值在**对方的数据**里，本库没有可查的列 —— ⛔ 不编一个列名 |
//! | 5 | 客户侧部署**不加载映射** ⇒ 天然没有 `@pending` / `get` | 契约规则 6：`get` 只给内部调用者；这里不需要额外的身份裁剪 |
//! | 6 | **客户视图按 `display_name` 匹配 api 类** ⇒ `display_name` 为空的 api 类**漏标** `ontok:sourceKind "api"` | 客户视图的类身份就是人读名（它结构上没有 key）；没有名字就没有可匹配的东西。⚠️ 这是**漏标不是错标**：下游会把那个类当库源去查，然后收到 `NOT_DB_SOURCE`（jsonld ⇒ `@pending`），但它在 TTL 里看起来像库源 —— 建模视图（按 key 匹配）**不受影响** |

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::model_view::ModelTrackView;
use crate::domain::r2rml::R2rmlMapping;
use crate::domain::ttl::{local_model, model_link_locals, ApiClasses};
use crate::sparql::query::{ObjectTerm, Query};

/// 映射文件里 `entities[].class` 对不上本体时，报错里带几个相近名（与 `catalog` 同一口径）。
const NEAREST_LIMIT: usize = 5;

/// 为什么这一块数据拿不到（`@pending[].reason`）。
///
/// 两个取值是**契约里的稳定字符串**（Agent 会按它分支），⛔ 不是人话：
/// - `source_is_api`：这个类整个在 API 源；按类取数（带 `get`）；
/// - `cross_source_edge`：只能经一条跨源边看到它；**不给** `get`（规则 4）。
pub const REASON_SOURCE_IS_API: &str = "source_is_api";
pub const REASON_CROSS_SOURCE_EDGE: &str = "cross_source_edge";

// ══════════════════════════════════════════════════════════════
// 映射文件
// ══════════════════════════════════════════════════════════════

/// 一条 api 类声明（映射文件 `entities[]` 里本模块真正用到的部分）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entity {
    /// access 契约里的条目名（`get.ref` 取它）。
    pub id: String,
    /// 本体里的类 key。
    pub class: String,
    /// 可下推的过滤字段（`capability.pushdown`；省略 = 空）。
    pub pushdown: Vec<String>,
    /// 数据新鲜度（可选；省略 ⇒ `access` 里不出现这个键）。
    pub freshness: Option<String>,
}

impl Entity {
    /// `get.ref` = `access#<id>@v<mapping_version>`（契约规则 3：可追溯到版本）。
    pub fn reference(&self, mapping_version: u64) -> String {
        format!("access#{}@v{mapping_version}", self.id)
    }
}

/// 一条**跨源边**（本体里的边 + 映射里的源 ⇒ 交叉推出）。
///
/// 为什么要有这个结构（而不只是「谓词 → (from, to)」）：`@pending` 那条要给 Agent
/// **可行动的**取数指引 —— 「库侧哪一列有 FK 值」（[`Self::local_column`]）。
/// 只给类名等于让它去猜列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossEdge {
    /// 本体里的**前向**谓词本地名（`order__customers`）。
    pub predicate: String,
    /// 边的两端（本体 key，child → parent）。
    pub from: String,
    pub to: String,
    /// 库侧 / api 侧的类 key（一库一 api，由映射决定）。
    pub db_key: String,
    pub api_key: String,
    /// 库侧的 FK 列 —— **只有「库是 child」时才有**。
    ///
    /// 反过来的形状（api 类是 child、库是 parent）里，FK 值在对方的**数据**里，
    /// 本库没有可查的列 ⇒ `None`（⛔ 不编一个列名）。
    pub local_column: Option<String>,
}

/// 映射文件本身（已解析、**还没**与本体对账）。
#[derive(Debug, Clone)]
pub struct AccessMapping {
    mapping_version: u64,
    /// ⭐ REQ-152 · 契约版本（缺省按 1）：`>= 2` 才允许下发完整配方（AC-103）。
    contract_version: u64,
    /// 单个类最多声明一次（重复 ⇒ 加载期报错）；按类 key 升序。
    entities: Vec<Entity>,
    /// 类 key → `entities` 下标（唯一索引）。
    by_class: BTreeMap<String, usize>,
    /// ⭐ REQ-152 · 类 key → 取数配方（**只有 v2 映射才有**；v1 ⇒ 表是空的）。
    ///
    /// 为什么放在这里而不是 [`Entity`] 里：`Entity` 要参与排序 / 去重（`Ord + Eq`），
    /// 而配方里有 `serde_json::Value`（原样透出的 `operation` / `request`），
    /// 它既没有 `Ord` 也没有 `Eq` —— 硬塞进去会把 `Entity` 的派生全拆掉。
    recipes: BTreeMap<String, Recipe>,
    /// TTL 用的 api 类集合（`bind` 之后才有值：人读名要从本体取）。
    api: ApiClasses,
    /// ⭐ REQ-151 · 本体里**不属于 api 源**的类本地名（`bind` 时从 model 一次算好）。
    ///
    /// 用途只有一个：判一条「碰到了 api 类」的查询**是不是混合查询**（还点了库类）。
    /// 这个事实在本体侧（`model.entity_types`）而不是映射文件里 —— 映射只声明 api 类，
    /// 「其余类默认在库」是它与本体对账后**推出来**的，⛔ 不要求映射再列一遍库类。
    ///
    /// `normal` 模式下它**不参与**任何判定（`needs_db` 只在本体模式的闸门里看，见
    /// [`SourceCheck::needs_db`]）—— 于是 REQ-145 的既有行为逐字节不变（R1）。
    db_locals: BTreeSet<String>,
    /// 跨源边的谓词本地名 → 边。**前向与反向都登记**（`order__customers` 与
    /// `customers__order` 指的是同一条边）—— 只登记前向的话，用反向谓词写的跨源
    /// 遍历会**漏判**，然后被当成库源查询真的去跑 SQL。
    cross_edges: BTreeMap<String, CrossEdge>,
}

/// 解析失败的形状（`Result<_, String>` 的 `String` 是给运维看的可行动消息）。
#[derive(Debug, Deserialize)]
struct RawMapping {
    #[serde(default)]
    mapping_version: Option<u64>,
    #[serde(default)]
    contract_version: Option<u64>,
    #[serde(default)]
    sources: Vec<SourceDecl>,
    #[serde(default)]
    entities: Vec<RawEntity>,
}

/// ⭐ REQ-152 · `sources[]`（本模块只用三样：`id` / `base_url` / `auth` 的**声明**
/// —— 凭据值一个都不读，也没有地方可放）。
#[derive(Debug, Deserialize)]
struct SourceDecl {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    auth: Option<RawAuth>,
    /// ⭐ REQ-152 · CP4 · 源级默认值（`source=default` 的绑定从这里取值）。
    ///
    /// **原样**透出到 `get.request.defaults`：键名 / 大小写 / 嵌套结构一个字符不改
    /// （真实映射里 `{"Region":…,"module":…}` 两个键的大小写本来就不一致）。
    /// ⛔ 这里不做任何解释 —— 哪个占位符该填哪个键，是客户端与映射作者之间的事。
    #[serde(default)]
    defaults: Option<serde_json::Value>,
}

/// ⭐ REQ-152 · `sources[].auth`。v1 只认前两项；v2 的三项声明**可选**
/// （缺 ⇒ `get.auth` 里就没有那个键，⛔ 不按 `mode` 猜默认值 —— AC-106）。
#[derive(Debug, Deserialize)]
struct RawAuth {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    credential_env: Vec<String>,
    #[serde(default)]
    signed_headers: Option<Vec<String>>,
    #[serde(default)]
    timestamp: Option<RawTimestamp>,
    #[serde(default)]
    scope: Option<Vec<String>>,
}

/// ⭐ REQ-152 · 时间戳声明（写哪个 header + 什么格式）。两项缺一 ⇒ 不透出（不编）。
#[derive(Debug, Deserialize)]
struct RawTimestamp {
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEntity {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    capability: Option<RawCapability>,
    #[serde(default)]
    freshness: Option<RawFreshness>,
    /// ⭐ REQ-152 · 指向 [`RawMapping::sources`] 的 `id`（配方里 `source` 段要用）。
    #[serde(default)]
    source: Option<String>,
    /// ⭐ REQ-152 · `operation` 段：**原样透出**给 `get.operation`（产品代码不认识里面任何名字）。
    #[serde(default)]
    operation: Option<serde_json::Value>,
    /// ⭐ REQ-152 · `request` 段（模板 / 必填 / 绑定 / 分页路径）：**原样透出**。
    #[serde(default)]
    request: Option<serde_json::Value>,
    #[serde(default)]
    iterator: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    iri_template: Option<String>,
    #[serde(default)]
    fields: Option<serde_json::Value>,
}

/// ⭐ REQ-152 · `freshness` 的**两种写法**（一份 schema，两个解析器不再打架）。
///
/// | 写法 | 谁在用 | 例 |
/// | --- | --- | --- |
/// | 字符串 | `query serve` 的历史形状（投影脚本产的那份） | `"freshness": "realtime"` |
/// | 对象 | access 契约 / 部署侧那份**已签的全量 AM** | `"freshness": {"mode": "realtime", "note": "…"}` |
///
/// **归一规则**：两种都收，统一取 `mode`（字符串写法就是它自己）。
/// 对象里没有 `mode`（或为空串）⇒ 与「省略 freshness」同义 —— ⛔ 不编一个「实时」。
/// `note` **读进来就丢**：它是给人看的说明，不参与任何判定（阶段 2 才可能用）。
///
/// 为什么放宽在这里而不是让部署侧继续造投影文件：投影脚本
/// （`Deploy/.../gen-serve-mapping.py`）是「同一份契约两个解析器」的补丁，
/// 它自己就是漂移源 —— 全量 AM 里新增一个字段，投影那份不会跟着变。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawFreshness {
    Text(String),
    Object {
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        #[allow(dead_code)]
        note: Option<String>,
    },
}

impl RawFreshness {
    /// 归一成 serve 侧唯一形状：`Option<非空 mode>`。
    fn into_mode(&self) -> Option<String> {
        let raw = match self {
            RawFreshness::Text(s) => Some(s.clone()),
            RawFreshness::Object { mode, .. } => mode.clone(),
        };
        raw.map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Deserialize)]
struct RawCapability {
    #[serde(default)]
    pushdown: Vec<String>,
}

/// ⭐ REQ-152 · 单类 `get` 的**体积上限**（序列化后字节数；超 ⇒ 加载期报错，不是静默瘦身）。
///
/// 为什么要有上限：`fields` 是可以写成几百条的长表，配方的体积会跟着映射作者的
/// 手速长；一个 200 KB 的 `@pending` 挂在每个响应里，真正的问题（字段映射过肥）
/// 会被"响应慢"这个表象盖住。12 KB 是「够用且看得出异常」的口径。
const MAX_GET_RECIPE_BYTES: usize = 12 * 1024;

/// ⭐ REQ-152 · 一条 `get` 的**取数配方**（`contract_version >= 2` 才下发）。
///
/// 每一段都是 `Option`：**映射里没有的项 ⇒ 整个键不出现**（⛔ 不塞空对象）。
/// 序列化顺序 = 声明顺序（`jsonld::PendingGet` 用 `#[serde(flatten)]` 接在旧 4 键之后）。
///
/// | 段 | 从映射哪里来 | 产品代码做什么 |
/// | --- | --- | --- |
/// | `source` | `entities[].source` → `sources[].{base_url,auth.mode}` | 只取这两个值 |
/// | `operation` | `entities[].operation` | **原样透出**（不认识任何 header 名） |
/// | `request` | `entities[].request` + 所属源的 `sources[].defaults` | **原样透出**（模板 / 必填 / 绑定 / 分页路径；CP4 起多一个 `defaults`，仅当源写了才出现） |
/// | `iterator` | `entities[].iterator` | 原样透出 |
/// | `fields` | `entities[].fields` | 摊平成 `属性 → 响应字段路径` |
/// | `auth` | `sources[].auth` 的声明 | 只透传（变量名 / 头名 / scope 段名） |
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Recipe {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<RecipeSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<RecipeAuth>,
}

/// `get.source`：打到哪儿、用哪种鉴权**模式**（模式名是给客户端选实现的，不是给代码分支的）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecipeSource {
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
}

/// `get.auth`：**只有声明**（变量名 / 头名 / 段名）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RecipeAuth {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub credential_env: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_headers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<RecipeTimestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Vec<String>>,
}

/// `get.auth.timestamp`：时间戳写哪个 header、什么格式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecipeTimestamp {
    pub header: String,
    pub format: String,
}

impl Recipe {
    /// 空配方（v1 / 缺一切）—— `PendingGet` 摊平它之后**一个键都不多**。
    pub fn is_empty(&self) -> bool {
        self.source.is_none()
            && self.operation.is_none()
            && self.request.is_none()
            && self.iterator.is_none()
            && self.fields.is_none()
            && self.auth.is_none()
    }
}

/// 从 `entities[]` 的一条 + `sources[]` 造配方（⭐ REQ-152）。
///
/// ⚠️ 这里**只搬数据**：不查表、不推默认值、不因为某个 `mode` 就补一组签名头 ——
/// 猜出来的 header 名是**错的签名**，比没有签名更难查（AC-106）。
fn build_recipe(
    e: &RawEntity,
    sources: &BTreeMap<String, &SourceDecl>,
) -> Recipe {
    // `source`：只有实体指了源、且那个源写了 `base_url` 才给（⛔ 不编一个地址）。
    let src = e
        .source
        .as_deref()
        .and_then(|id| sources.get(id).copied());
    let source = src.and_then(|s| {
        s.base_url.as_ref().map(|base_url| RecipeSource {
            base_url: base_url.clone(),
            auth_mode: s.auth.as_ref().and_then(|a| a.mode.clone()),
        })
    });
    // `auth`：只透出声明；三项各自缺 ⇒ 各自不出现。
    let auth = src.and_then(|s| s.auth.as_ref()).map(|a| RecipeAuth {
        credential_env: a.credential_env.clone(),
        signed_headers: a.signed_headers.clone(),
        timestamp: a
            .timestamp
            .as_ref()
            .and_then(|t| match (&t.header, &t.format) {
                (Some(header), Some(format)) => Some(RecipeTimestamp {
                    header: header.clone(),
                    format: format.clone(),
                }),
                _ => None,
            }),
        scope: a.scope.clone(),
    });
    // `fields`：摊平成 `属性 → 响应字段路径`（值不是 `{path}` 的条目跳过 —— 那是坏映射，
    // 由 `access validate` 负责报错，本模块只负责把**能用**的部分透出）。
    let fields = e.fields.as_ref().and_then(|f| f.as_object()).map(|map| {
        map.iter()
            .filter_map(|(k, v)| {
                v.get("path")
                    .and_then(|p| p.as_str())
                    .map(|p| (k.clone(), p.to_string()))
            })
            .collect::<BTreeMap<String, String>>()
    });
    Recipe {
        source,
        operation: e.operation.clone(),
        request: with_defaults(e.request.clone(), src.and_then(|s| s.defaults.clone())),
        iterator: e.iterator.clone(),
        fields: fields.filter(|f| !f.is_empty()),
        // `credential_env` 全空且三项也没有 ⇒ 整个 `auth` 不出现。
        auth: auth.filter(|a| {
            !a.credential_env.is_empty()
                || a.signed_headers.is_some()
                || a.timestamp.is_some()
                || a.scope.is_some()
        }),
    }
}

/// ⭐ REQ-152 · CP4 · `sources[].defaults` → `get.request.defaults`（**只搬不改**）。
///
/// 为什么挂在 `request` 下：`bindings.<名>.source = "default"` 只说「从默认值取」，
/// 而值本身在 `sources[].defaults` 里 —— 不把它透出来，客户端拿到 `{module}` 也
/// 不知道填什么（实测：`alarm_policy` 因此报 `MissingParameter`）。
///
/// 口径：
/// - 源**没有** `defaults` ⇒ 整个键不出现（⛔ 不塞 `{}`、⛔ 不按占位符名编一个）；
/// - 映射写了什么就透什么：键名 / 大小写 / 嵌套结构**原样**（⛔ 不合并进 `template`、
///   ⛔ 不解释成「某个 header 的值」）；
/// - `request` 本身就缺、但有 defaults ⇒ 给 `{"defaults": …}`；
/// - `request` 不是对象（坏映射）⇒ 原样留着，⛔ 不静默改写（`access validate` 负责报错）。
fn with_defaults(
    request: Option<serde_json::Value>,
    defaults: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    let Some(defaults) = defaults else { return request };
    match request {
        None => Some(serde_json::json!({ "defaults": defaults })),
        Some(serde_json::Value::Object(mut obj)) => {
            obj.insert("defaults".to_string(), defaults);
            Some(serde_json::Value::Object(obj))
        }
        other => other,
    }
}

impl AccessMapping {
    /// 解析映射文件。**只做形状校验**（字段缺失 / 重复 / 空串）；与本体对不对得上在
    /// [`Self::bind`] 里判 —— 两步分开是因为「文件坏了」与「文件与本体不一致」是两类
    /// 不同的运维动作（改文件 vs 改映射版本）。
    pub fn parse(raw: &str) -> Result<Self, String> {
        let doc: RawMapping = serde_json::from_str(raw)
            .map_err(|e| format!("不是合法的 JSON（{e}）。形状见 `query serve --help` 的 `--access-mapping`。"))?;
        let mapping_version = doc.mapping_version.ok_or_else(|| {
            "缺 `mapping_version`。它是 `meta.mapping_version` 与 `@pending[].get.ref` 的 `@v<n>`\
             （契约规则 3：映射是人签的，要能追到版本）；⛔ 不替你编一个。"
                .to_string()
        })?;
        if mapping_version == 0 {
            return Err("`mapping_version` 是 0。版本从 1 起（0 会被读成「没有版本」）。".to_string());
        }
        // ⭐ REQ-152 · 契约版本：缺省 = 1（旧形状）。`>= 2` 才允许下发完整配方。
        let contract_version = doc.contract_version.unwrap_or(1);
        if contract_version == 0 {
            return Err("`contract_version` 是 0。契约版本从 1 起。".to_string());
        }
        if doc.entities.is_empty() {
            // 「加载了映射但一条 api 类都没有」是一个可用的状态（全是库源），但要说出来 ——
            // 否则运维会以为映射生效了。这里不当作错误，只由调用方打一行提示。
        }

        // ⭐ REQ-152 · `sources[]` 只留三样（`id` → 声明）；没有 `id` 的源没人引用得上。
        let mut sources: BTreeMap<String, &SourceDecl> = BTreeMap::new();
        for s in &doc.sources {
            if let Some(id) = s.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                sources.insert(id.to_string(), s);
            }
        }
        let with_recipe = contract_version >= 2;
        let mut recipes: BTreeMap<String, Recipe> = BTreeMap::new();
        // 配方超限的类（加载期一次性报全，⛔ 不静默瘦身 —— 瘦身等于悄悄改契约）。
        let mut oversized: Vec<(String, usize)> = Vec::new();

        let mut entities: Vec<Entity> = Vec::with_capacity(doc.entities.len());
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for (i, e) in doc.entities.iter().enumerate() {
            let id = e
                .id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    format!(
                        "entities[{i}] 缺 `id`（access 契约里的条目名，例：\"Customer\"）。\
                         `@pending[].get.ref` = `access#<id>@v<mapping_version>`，没有它调用方就追不回条目。"
                    )
                })?
                .to_string();
            let class = e
                .class
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    format!(
                        "entities[{i}]（id={id:?}）缺 `class` —— 要写**本体里的类 key**\
                         （= `catalog` 里 class 项的 `table`，例：\"customers\"）。"
                    )
                })?
                .to_string();
            if let Some(prev) = seen.get(&class) {
                return Err(format!(
                    "类 {class:?} 被声明了两次（id={prev:?} 与 id={id:?}）：\
                     一个类只能有一个数据源，否则「这条数据到底信谁」没有答案。"
                ));
            }
            seen.insert(class.clone(), id.clone());
            let mut pushdown = e
                .capability
                .as_ref()
                .map(|c| c.pushdown.clone())
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<String>>();
            pushdown.sort();
            pushdown.dedup();
            let freshness = e.freshness.as_ref().and_then(RawFreshness::into_mode);
            // ⭐ REQ-152 · v2 才建配方；v1 ⇒ 表里一条都没有（`get` 一个键都不多）。
            if with_recipe {
                let recipe = build_recipe(e, &sources);
                if !recipe.is_empty() {
                    let bytes = serde_json::to_string(&recipe).map(|s| s.len()).unwrap_or(0);
                    if bytes > MAX_GET_RECIPE_BYTES {
                        oversized.push((class.clone(), bytes));
                    }
                    recipes.insert(class.clone(), recipe);
                }
            }
            entities.push(Entity {
                id,
                class,
                pushdown,
                freshness,
            });
        }
        entities.sort_by(|a, b| a.class.cmp(&b.class));

        if !oversized.is_empty() {
            let list = oversized
                .iter()
                .map(|(c, n)| format!("{c}（{n} B）"))
                .collect::<Vec<_>>()
                .join("、");
            return Err(format!(
                "有 {} 个类的 `@pending.get` 配方超过 {MAX_GET_RECIPE_BYTES} B：{list}。\
                 通常是 `entities[].fields` 过肥 —— 几十 KB 的字段表挂在每个 `@pending` 里，\
                 调用方既读不完也缓存不住。请只映射真正要用的属性。\
                 ⛔ 本实现不会替你截断（截断等于悄悄改契约）。",
                oversized.len()
            ));
        }

        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        for (i, e) in entities.iter().enumerate() {
            by_class.insert(e.class.clone(), i);
        }

        Ok(Self {
            mapping_version,
            contract_version,
            entities,
            by_class,
            recipes,
            api: ApiClasses::default(),
            db_locals: BTreeSet::new(),
            cross_edges: BTreeMap::new(),
        })
    }

    /// 与**这一版本体**对账：类必须存在；把 api 类的 key + 人读名交给 TTL；
    /// 顺手从本体推导跨源边（见模块头）。
    ///
    /// 失败 ⇒ 调用方**拒绝启动**（fail-closed）：一份「一半对不上」的映射最坏的后果是
    /// 平台以为某类在库、其实在 API —— 那正是本需求要消灭的静默错误。
    pub fn bind(mut self, model: &ModelTrackView) -> Result<Self, String> {
        let keys: BTreeSet<&str> = model.entity_types.iter().map(|e| e.key.as_str()).collect();
        let mut missing: Vec<String> = Vec::new();
        for e in &self.entities {
            if !keys.contains(e.class.as_str()) {
                missing.push(e.class.clone());
            }
        }
        if let Some(first) = missing.first() {
            let mut near = nearest(&keys, first);
            near.sort();
            near.dedup();
            let hint = if near.is_empty() {
                "用 `GET /api/catalog` 的 class 项（`table` 字段）抄类 key。".to_string()
            } else {
                format!("最接近的几个：{}。", near.join("、"))
            };
            return Err(format!(
                "映射里有 {} 个类在这一版本体里不存在（第一个：{first:?}）。{hint}\n  \
                 ⛔ 这类条目会让平台以为「某个真在 API 的类可以查库」—— 所以拒绝整份映射，\
                 而不是忽略那一行。",
                missing.len()
            ));
        }

        // 人读名（客户视图按名字匹配）：从本体取 display_name。
        let mut names: BTreeSet<String> = BTreeSet::new();
        for e in &self.entities {
            if let Some(ent) = model.entity_types.iter().find(|x| x.key == e.class) {
                if !ent.display_name.trim().is_empty() {
                    names.insert(ent.display_name.clone());
                }
            }
        }
        self.api = ApiClasses::new(self.entities.iter().map(|e| e.class.clone()), names);

        // ⭐ REQ-151 · 「其余类默认在库」= 本体里存在、但**不在**这份 api 映射里的类。
        // 存本地名（SPARQL 里写出来的那个），判定时只与谓词 / 类词法比较，⛔ 不做字符串匹配。
        let api_locals: BTreeSet<String> = self
            .entities
            .iter()
            .map(|e| local_model(&e.class))
            .collect();
        self.db_locals = model
            .entity_types
            .iter()
            .map(|e| local_model(&e.key))
            .filter(|local| !api_locals.contains(local))
            .collect();

        // 跨源边：本体里**真实存在**的边，两端一条在 api、一条在库 ⇒ 跨源。
        let mut links: Vec<&crate::domain::model_view::ModelLinkView> =
            model.link_types.iter().collect();
        links.sort_by(|a, b| {
            a.key
                .cmp(&b.key)
                .then_with(|| a.from_type.cmp(&b.from_type))
                .then_with(|| a.to_type.cmp(&b.to_type))
        });
        let (forward, inverse) = model_link_locals(&links);
        for (i, l) in links.iter().enumerate() {
            let from_api = self.by_class.contains_key(&l.from_type);
            let to_api = self.by_class.contains_key(&l.to_type);
            if from_api == to_api {
                continue; // 两端同源（都在库 / 都在 api）⇒ 不是跨源边
            }
            let (db_key, api_key) = if from_api {
                (l.to_type.clone(), l.from_type.clone())
            } else {
                (l.from_type.clone(), l.to_type.clone())
            };
            let edge = CrossEdge {
                predicate: forward[i].clone(),
                from: l.from_type.clone(),
                to: l.to_type.clone(),
                db_key,
                api_key,
                // FK 列只在本体给的 child 上；**库必须是 child** 才算「本库有这列」。
                local_column: if from_api { None } else { l.column.clone() },
            };
            // 前向与反向都登记同一条边（同一个 `CrossEdge`）——
            // 否则用 `:customers__order` 写的遍历会漏判成「库源查询」。
            self.cross_edges.insert(forward[i].clone(), edge.clone());
            self.cross_edges.insert(inverse[i].clone(), edge);
        }
        Ok(self)
    }

    pub fn mapping_version(&self) -> u64 {
        self.mapping_version
    }

    /// ⭐ REQ-152 · 契约版本（`>= 2` 才允许下发完整配方）。
    pub fn contract_version(&self) -> u64 {
        self.contract_version
    }

    /// ⭐ REQ-152 · 某个类的取数配方（v1 映射 ⇒ 恒 `None`）。
    pub fn recipe(&self, class_key: &str) -> Option<&Recipe> {
        self.recipes.get(class_key)
    }

    /// api 类集合（交给 TTL；只含「是 / 不是」，不含任何接口细节）。
    pub fn api_classes(&self) -> &ApiClasses {
        &self.api
    }

    /// 这个类是不是 api 源。
    pub fn is_api(&self, class_key: &str) -> bool {
        self.by_class.contains_key(class_key)
    }

    pub fn entity(&self, class_key: &str) -> Option<&Entity> {
        self.by_class.get(class_key).map(|i| &self.entities[*i])
    }

    /// api 类：`{id, class, pushdown, freshness}`（catalog / describe 用）。
    pub fn api_entities(&self) -> &[Entity] {
        &self.entities
    }

    /// **跨源边的条数**（启动横幅用；边本身由 [`Self::bind`] 从本体派生）。
    ///
    /// ⚠️ 数的是**本体里的边**（前向谓词去重），⛔ 不是 `cross_edges` 的条目数 ——
    /// 那里面前向 + 反向各登记了一条（判定遍历要用），直接 `len()` 会得到**方向数**
    /// （本部署 40 条边 = 80 个方向）。把方向数当边数报出去，排查的人会对不上。
    pub fn cross_source_edge_count(&self) -> usize {
        self.cross_edges
            .values()
            .map(|e| e.predicate.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    /// 跨源边的**方向数**（前向 + 反向；横幅里跟在边数后面，两个数都写得清）。
    pub fn cross_source_direction_count(&self) -> usize {
        self.cross_edges.len()
    }

    /// 类 key 的本地名（SPARQL / IRI / `@type` 用的那一个）。
    fn class_local(&self, class_key: &str) -> String {
        local_model(class_key)
    }

    // ── 查询前判定 ────────────────────────────────────────────

    /// **这条查询落到哪一类**（纯函数：只读 AST + 映射 + 本体 IRI）。
    ///
    /// 判据全部来自**写出来的三元组模式**（⛔ 不做字符串匹配）：
    /// - `?x a :C` ⇒ 点名了类 `C`；
    /// - `?x :C__col …` ⇒ 点名了类 `C` 的属性；
    /// - `?x :C__D ?y` 且这条边是跨源边 ⇒ 走了跨源边。
    ///
    /// `pk_of(class_key)` 给出该类的实例标识列（用来从字面量里认出 `filter.ids`）；
    /// 给不出 ⇒ `filter: {}`（契约规则 7 的形状），⛔ 不猜 id。
    pub fn check(
        &self,
        q: &Query,
        ontology_iri: &str,
        r2rml: Option<&R2rmlMapping>,
    ) -> SourceCheck {
        // ⭐ REQ-151 · 这条查询**还点了库类**吗（混合查询的判据；只给本体模式的闸门用）。
        // 依据是本体的类集合（`db_locals`）+ 写出来的三元组模式，⛔ 不做字符串拼接后匹配。
        let mut db_hits = false;
        let mut api_hits: BTreeSet<String> = BTreeSet::new(); // 类 key
        let mut cross_hits: Vec<CrossEdge> = Vec::new(); // 走过的跨源边

        let class_iri_prefix = format!("{ontology_iri}#");
        for p in &q.patterns {
            let pred_local = local_of(&p.predicate);
            if pred_local == "type" {
                if let ObjectTerm::Iri(iri) = &p.object {
                    if let Some(local) = iri.strip_prefix(&class_iri_prefix) {
                        if let Some(key) = self.key_of_local(local) {
                            api_hits.insert(key.to_string());
                        } else if self.db_locals.contains(local) {
                            db_hits = true;
                        }
                    }
                }
                continue;
            }
            if let Some(edge) = self.cross_edges.get(pred_local) {
                // 同一条边可能同时被前向与反向谓词写到 —— 只记一次。
                if !cross_hits.iter().any(|e| e.predicate == edge.predicate) {
                    cross_hits.push(edge.clone());
                }
                // 这条边的一端是 api —— 但它是否「被要求往下走」要看有没有点名（下面判）。
                continue;
            }
            // 普通属性谓词：`<类>__<列>`，类在 api ⇒ 点名了 api 类；在库 ⇒ 混合。
            if let Some((owner, _rest)) = pred_local.split_once("__") {
                if let Some(key) = self.key_of_local(owner) {
                    api_hits.insert(key.to_string());
                } else if self.db_locals.contains(owner) {
                    db_hits = true;
                }
            }
        }

        // 「跨源边还要求往下走」= 跨源边涉及的那个 api 类**还被点名**（类 / 属性）。
        for edge in &cross_hits {
            if api_hits.contains(&edge.api_key) {
                return SourceCheck::CrossSource {
                    predicate: edge.predicate.clone(),
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                };
            }
        }

        if !api_hits.is_empty() || !cross_hits.is_empty() {
            let mut pendings: Vec<ApiPending> = Vec::new();
            // 走跨源边看到的 api 类：**不给** `get`（规则 4：给了也查不了），
            // 但**要给** `ref` + `local_hint`（R-1：否则 Agent 拿不到映射、也不知道
            // 库侧哪一列有 FK 值）。
            for edge in &cross_hits {
                if let Some(p) = self.cross_pending(edge) {
                    pendings.push(p);
                }
            }
            // 按类点名的 api 类：**带** `get`（可能一条 per id）。
            for key in &api_hits {
                let ids = ids_of(q, key, r2rml);
                if ids.is_empty() {
                    if let Some(p) = self.pending_for(key, None, r2rml) {
                        pendings.push(p);
                    }
                } else {
                    for id in ids {
                        if let Some(p) = self.pending_for(key, Some(id), r2rml) {
                            pendings.push(p);
                        }
                    }
                }
            }
            pendings.sort_by(|a, b| {
                a.class_key
                    .cmp(&b.class_key)
                    .then_with(|| a.id.cmp(&b.id))
                    .then_with(|| a.reason.cmp(b.reason))
            });
            // 同一个类可能同时被「点名」与「跨源边」看到 —— 去重（key + id + reason）。
            pendings.dedup_by(|a, b| {
                a.class_key == b.class_key && a.id == b.id && a.reason == b.reason
            });
            return SourceCheck::Api {
                pending: pendings,
                // 跨源边的一端在库 ⇒ 这条查询**也要库**（哪怕本模式不执行 SQL）。
                needs_db: db_hits || !cross_hits.is_empty(),
            };
        }
        SourceCheck::DbOnly
    }

    /// 本地名 → 类 key（api 类才有；库源类不需要反查）。
    fn key_of_local(&self, local: &str) -> Option<&str> {
        self.entities
            .iter()
            .find(|e| self.class_local(&e.class) == local)
            .map(|e| e.class.as_str())
    }

    /// **按类点名**的那一条：带 `get`（含 `ids` / `pushdown` / `ref`）。
    fn pending_for(
        &self,
        class_key: &str,
        id: Option<String>,
        r2rml: Option<&R2rmlMapping>,
    ) -> Option<ApiPending> {
        let entity = self.entity(class_key)?;
        let class_local = self.class_local(class_key);
        let _ = r2rml;
        Some(ApiPending {
            class_key: class_key.to_string(),
            class_local,
            id: id.clone(),
            reason: REASON_SOURCE_IS_API,
            reference: None, // 按类点名时 `ref` 在 `get` 里（不重复）
            local_hint: None,
            hint: None,
            get: Some(GetInstruction {
                class: self.class_local(class_key),
                ids: id.into_iter().collect(),
                pushdown: entity.pushdown.clone(),
                reference: entity.reference(self.mapping_version),
                // ⭐ REQ-152 · v1 映射这里恒 `None` ⇒ `get` 只有旧 4 键（AC-103）。
                recipe: self.recipes.get(class_key).cloned(),
            }),
        })
    }

    /// **跨源边**的那一条：⛔ 无 `get`（规则 4），但 `ref` + `local_hint` + 三步 `hint` 都要有。
    fn cross_pending(&self, edge: &CrossEdge) -> Option<ApiPending> {
        let entity = self.entity(&edge.api_key)?;
        let class_local = self.class_local(&edge.api_key);
        let reference = entity.reference(self.mapping_version);
        // 库侧的 FK 列与可执行的取数语句（R-1 的硬要求：Agent 得知道查哪一列）。
        let local_hint = Some(LocalHint {
            uses_column: edge.local_column.clone(),
            get_ids_by: edge.local_column.as_ref().map(|col| {
                format!(
                    "SELECT DISTINCT {col:?} FROM {:?}",
                    edge.from // 本体里 child 侧的 key 就是表名
                )
            }),
            note: if edge.local_column.is_some() {
                None
            } else {
                Some("这条边的 FK 在 API 侧：本库没有可取的列，只能按 api 类自己取")
            },
        });
        let hint = Some(match (&edge.local_column, &reference) {
            (Some(col), r) => format!(
                "① 先在库侧取 ID：`SELECT DISTINCT {col:?} FROM {:?}`（按你的租户过滤条件收窄）；\
                 ② 再按这些 ID 调 {r} 指向的映射取数据；\
                 ③ 把两段结果拼起来 —— 平台不做跨源 join。",
                edge.from
            ),
            (None, r) => format!(
                "① 这条边的 FK 在 API 侧，本库没有可取的列；\
                 ② 按 {r} 指向的映射取该类数据（先按类级 pending 的口径取）；\
                 ③ 把两段结果拼起来 —— 平台不做跨源 join。"
            ),
        });
        Some(ApiPending {
            class_key: edge.api_key.clone(),
            class_local,
            id: None,
            reason: REASON_CROSS_SOURCE_EDGE,
            reference: Some(reference),
            local_hint,
            hint,
            // ⛔ 规则 4：跨源边不下发 `get`。
            get: None,
        })
    }
}

/// 一条 `@pending`（**不进 `@graph`**，契约规则 2）。
///
/// ⚠️ REQ-152 起不再派生 `Eq`：`get` 的配方里有 `serde_json::Value`（原样透出的
/// `operation` / `request`），它只有 `PartialEq`；`Eq` 在这里没有使用者。
#[derive(Debug, Clone, PartialEq)]
pub struct ApiPending {
    /// 本体里的类 key（`unresolved_classes` 报的是本地名，见 [`Self::class_local`]）。
    pub class_key: String,
    /// SPARQL / `@type` 用的本地名（`ontok:<local>`）。
    pub class_local: String,
    /// 认出来的实例标识（`None` = 查询没给 id ⇒ 类级 pending，`filter: {}`）。
    pub id: Option<String>,
    /// [`REASON_SOURCE_IS_API`] / [`REASON_CROSS_SOURCE_EDGE`]。
    pub reason: &'static str,
    /// ⭐ R-1 · **跨源边那一条才有的 `ref`**（`access#<条目>@v<版本>`）。
    ///
    /// 按类点名时 `ref` 在 `get` 里（同一个值，不重复给）；跨源边**没有 `get`**，
    /// 所以必须在这里给 —— 否则 Agent 拿不到映射。
    pub reference: Option<String>,
    /// ⭐ R-1 · 跨源边那一条才有的**库侧取数提示**（哪一列有 FK 值）。
    pub local_hint: Option<LocalHint>,
    /// ⭐ R-1 · 跨源边那一条才有的**三步**人话提示。
    pub hint: Option<String>,
    /// 取数指引；跨源边为 `None`（规则 4）。
    pub get: Option<GetInstruction>,
}

/// ⭐ R-1 · `@pending[].local_hint` ——「ID 在库侧哪儿」。
///
/// 为什么必须有：`@pending` 说「这一块没算」，Agent 要自己去把 ID 捞出来；
/// 不给列名，它就只能猜一个列，然后拿猜的列去查 —— 那正是本需求要消灭的形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalHint {
    /// 库侧哪一列存着 FK 值（例：`customer_id`）。
    ///
    /// `None` = 这条边的 FK 在 **API 侧**（api 类是 child）：本库没有可取的列。
    pub uses_column: Option<String>,
    /// 可直接执行的取数语句（`SELECT DISTINCT <列> FROM <子表>`）。
    ///
    /// ⚠️ 不含 `WHERE`：租户过滤条件属于调用方自己的口径（且不该把租户 id 写进
    /// 半个答案里）。`hint` 里明说「按你的租户过滤条件收窄」。
    pub get_ids_by: Option<String>,
    /// `uses_column = None` 时的解释（⛔ 不编一个列名）。
    pub note: Option<&'static str>,
}

/// `@pending[].get` ——「这一块怎么自己取」。
#[derive(Debug, Clone, PartialEq)]
pub struct GetInstruction {
    /// 要取哪个类（SPARQL 侧的名字）。
    pub class: String,
    /// 已知的实例标识（空 = 平台不知道，按类取）。
    pub ids: Vec<String>,
    /// 可下推的过滤字段（来自 `capability.pushdown`；空 = 映射没说）。
    pub pushdown: Vec<String>,
    /// `access#<id>@v<mapping_version>`。
    pub reference: String,
    /// ⭐ REQ-152 · 取数配方（**契约 v2** 才有；v1 ⇒ `None`）。
    ///
    /// 它被 `jsonld::PendingGet` 用 `#[serde(flatten)]` 接在旧 4 键之后，
    /// 于是「v1 ⇒ 一个键都不多」是**结构**上的事，不靠在哪里判一次版本。
    pub recipe: Option<Recipe>,
}

/// 一条查询的**源判定**（[`AccessMapping::check`] 的返回值）。
#[derive(Debug, Clone, PartialEq)]
pub enum SourceCheck {
    /// 只碰库源 ⇒ 正常执行。
    DbOnly,
    /// 碰到 api 源 ⇒ 不执行 SQL，改为 pending（jsonld）/ 报错（rows）。
    Api {
        pending: Vec<ApiPending>,
        /// ⭐ REQ-151 · 这条查询**除了 api 源之外还要库**（混合：点了库类 / 走了跨源边）。
        ///
        /// 普通模式**不看**它（那里的混合查询本来就走 pending，`@graph` 恒空）；
        /// 本体模式只在**纯 api**（`false`）时给 `@pending`，`true` ⇒ 503 `NO_DATA_SOURCE`。
        /// 为什么以「字段」而不是「另一个枚举分支」表达：它和 pending 是**同一个判定**
        /// 的两个事实，分成两个分支会让 `pending()` 的调用点各判一次。
        needs_db: bool,
    },
    /// 需要跨源遍历 ⇒ 两种格式都报 `CROSS_SOURCE_UNSUPPORTED`。
    CrossSource {
        predicate: String,
        from: String,
        to: String,
    },
}

impl SourceCheck {
    pub fn is_db_only(&self) -> bool {
        matches!(self, SourceCheck::DbOnly)
    }

    /// ⭐ REQ-151 · 这条查询要拿到完整答案，是否需要**库源**。
    ///
    /// `DbOnly` / `CrossSource` 恒 `true`；`Api` 看混合判定。
    pub fn needs_db(&self) -> bool {
        match self {
            SourceCheck::DbOnly | SourceCheck::CrossSource { .. } => true,
            SourceCheck::Api { needs_db, .. } => *needs_db,
        }
    }

    /// 这次查询要不要报 `CROSS_SOURCE_UNSUPPORTED`。
    pub fn cross_source(&self) -> Option<(&str, &str, &str)> {
        match self {
            SourceCheck::CrossSource { predicate, from, to } => {
                Some((predicate.as_str(), from.as_str(), to.as_str()))
            }
            _ => None,
        }
    }

    pub fn pending(&self) -> &[ApiPending] {
        match self {
            SourceCheck::Api { pending, .. } => pending,
            _ => &[],
        }
    }
}

// ══════════════════════════════════════════════════════════════
// 辅助
// ══════════════════════════════════════════════════════════════

/// 谓词 IRI → 本地名。与 planner 的 `predicate_local` **同一口径**（`#` 优先，其次 `/`）。
fn local_of(iri: &str) -> &str {
    match iri.rfind('#') {
        Some(i) => &iri[i + 1..],
        None => iri.rsplit('/').next().unwrap_or(iri),
    }
}

/// 查询里**按字面量点名**的实例标识：`?x :C__<pk> "v"` 的 `v`（升序去重）。
///
/// 依据是**映射里的主键列**（⛔ 不拿 `id` 猜）：主键列的属性本地名由
/// [`R2rmlMapping`] 给出，只有它才代表「这个类的一个实例」。
fn ids_of(q: &Query, class_key: &str, r2rml: Option<&R2rmlMapping>) -> Vec<String> {
    let Some(mapping) = r2rml else { return Vec::new() };
    let Some(class) = mapping.class(class_key) else { return Vec::new() };
    let pk_predicate = class
        .properties
        .iter()
        .find(|p| p.column == class.pk)
        .map(|p| p.local.clone());
    let Some(pk_predicate) = pk_predicate else { return Vec::new() };
    let mut out: BTreeSet<String> = BTreeSet::new();
    for p in &q.patterns {
        if local_of(&p.predicate) != pk_predicate {
            continue;
        }
        match (&p.subject, &p.object) {
            // `?c :customers__id "7d49…"` / `:customers/7d49… :customers__id "…"`
            (_, ObjectTerm::Literal(lit)) => {
                out.insert(lit.text.clone());
            }
            // `?c :customers__id ?cid . FILTER(?cid = "…")` —— 变量的字面量约束在 FILTER 里。
            (_, ObjectTerm::Var(v)) => {
                for value in filter_literals(q, v) {
                    out.insert(value);
                }
            }
            _ => {}
        }
    }
    out.into_iter().collect()
}

/// `FILTER(?v <op> "字面量")` / `FILTER("字面量" <op> ?v)` 里的字面量（只认 `=`）。
fn filter_literals(q: &Query, var: &str) -> Vec<String> {
    use crate::sparql::query::{CmpOp, Expr, Operand};
    fn walk(e: &Expr, var: &str, out: &mut Vec<String>) {
        match e {
            Expr::Or(a, b) | Expr::And(a, b) => {
                walk(a, var, out);
                walk(b, var, out);
            }
            Expr::Not(a) => walk(a, var, out),
            Expr::Cmp(CmpOp::Eq, a, b) => match (a, b) {
                (Operand::Var(v), Operand::Literal(l)) | (Operand::Literal(l), Operand::Var(v))
                    if v == var =>
                {
                    out.push(l.text.clone())
                }
                _ => {}
            },
            Expr::Cmp(..) => {}
        }
    }
    let mut out = Vec::new();
    for f in &q.filters {
        walk(f, var, &mut out);
    }
    out
}

/// 名字相近的几个类 key（报错用；与 `catalog::nearest` 同一条口径：编辑距离 ≤ 3）。
fn nearest<'a>(keys: &BTreeSet<&'a str>, target: &str) -> Vec<String> {
    let mut out: Vec<(usize, String)> = keys
        .iter()
        .map(|k| (levenshtein(target, k), (*k).to_string()))
        .filter(|(d, _)| *d <= 3)
        .collect();
    out.sort();
    out.into_iter().take(NEAREST_LIMIT).map(|(_, k)| k).collect()
}

/// 编辑距离（只用于「你是不是打错了一个字」；小写化后比）。
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparql::query::parse;

    fn model() -> ModelTrackView {
        use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelTrackView};
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
        // 三个类：`order` / `customers`（将是 api）/ `balances`；一条 order→customers 的边。
        ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                entity("order", "订单"),
                entity("customers", "客户"),
                entity("balances", "余额"),
            ],
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

    fn mapping() -> AccessMapping {
        AccessMapping::parse(
            r#"{"contract_version":1,"mapping_version":2,
                "entities":[{"id":"Customer","class":"customers",
                             "capability":{"pushdown":["ids"]},"freshness":"realtime"}]}"#,
        )
        .expect("parse")
        .bind(&model())
        .expect("bind")
    }

    #[test]
    fn parse_requires_mapping_version_and_class() {
        let e = AccessMapping::parse(r#"{"entities":[]}"#).expect_err("缺 mapping_version");
        assert!(e.contains("mapping_version"), "{e}");
        let e = AccessMapping::parse(r#"{"mapping_version":2,"entities":[{"class":"customers"}]}"#)
            .expect_err("缺 id");
        assert!(e.contains("缺 `id`"), "{e}");
        let e = AccessMapping::parse(r#"{"mapping_version":2,"entities":[{"id":"C"}]}"#)
            .expect_err("缺 class");
        assert!(e.contains("缺 `class`"), "{e}");
    }

    #[test]
    fn parse_rejects_duplicate_class() {
        let e = AccessMapping::parse(
            r#"{"mapping_version":2,"entities":[
                 {"id":"A","class":"customers"},{"id":"B","class":"customers"}]}"#,
        )
        .expect_err("重复类");
        assert!(e.contains("两次"), "{e}");
    }

    #[test]
    fn bind_rejects_a_class_that_is_not_in_the_ontology() {
        let e = AccessMapping::parse(
            r#"{"mapping_version":2,"entities":[{"id":"C","class":"customerz"}]}"#,
        )
        .expect("parse")
        .bind(&model())
        .expect_err("类不存在");
        assert!(e.contains("不存在"), "{e}");
        assert!(e.contains("customers"), "要给相近项：{e}");
    }

    #[test]
    fn bind_derives_cross_source_edges_from_the_ontology() {
        let m = mapping();
        // order(db) → customers(api) 就是跨源边；映射文件里**没有**边的字段。
        assert!(m.cross_edges.contains_key("order__customers"), "{:?}", m.cross_edges);
        assert!(m.is_api("customers"));
        assert!(!m.is_api("order"));
        assert!(m.api_classes().is_api_key("customers"));
        assert!(m.api_classes().is_api_name("客户"));
    }

    fn check_of(m: &AccessMapping, sparql: &str) -> SourceCheck {
        let q = parse(sparql, "https://ontok.local/demo").expect("parse query");
        m.check(&q, "https://ontok.local/demo", None)
    }

    #[test]
    fn a_db_only_query_is_db_only() {
        let m = mapping();
        assert!(check_of(&m, "SELECT ?o WHERE { ?o a :order }").is_db_only());
    }

    #[test]
    fn naming_an_api_class_yields_a_pending_with_get() {
        let m = mapping();
        let c = check_of(&m, "SELECT ?c WHERE { ?c a :customers }");
        let pending = c.pending();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].reason, REASON_SOURCE_IS_API);
        let get = pending[0].get.as_ref().expect("按类点名要给 get");
        assert_eq!(get.class, "customers");
        assert!(get.ids.is_empty(), "没给 id 就是类级 pending");
        assert_eq!(get.pushdown, vec!["ids".to_string()]);
        assert_eq!(get.reference, "access#Customer@v2");
    }

    #[test]
    fn an_api_class_property_also_counts_as_naming_it() {
        let m = mapping();
        let c = check_of(&m, "SELECT ?c WHERE { ?c :customers__customer_name ?n }");
        assert_eq!(c.pending().len(), 1, "{c:?}");
    }

    #[test]
    fn a_bare_cross_source_edge_yields_a_pending_without_get() {
        let m = mapping();
        let c = check_of(&m, "SELECT ?o ?c WHERE { ?o a :order . ?o :order__customers ?c }");
        let pending = c.pending();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].reason, REASON_CROSS_SOURCE_EDGE);
        assert!(pending[0].get.is_none(), "规则 4：跨源边不下发 get");
        // ⭐ R-1 · 没有 `get` 就必须有 `ref`（否则拿不到映射）…
        assert_eq!(
            pending[0].reference.as_deref(),
            Some("access#Customer@v2"),
            "跨源边那一条必须带 ref"
        );
        // …和 `local_hint`（否则 Agent 只能猜「哪一列有 FK 值」）。
        let hint = pending[0].local_hint.as_ref().expect("跨源边必须带 local_hint");
        assert_eq!(hint.uses_column.as_deref(), Some("customer_id"), "{hint:?}");
        assert_eq!(
            hint.get_ids_by.as_deref(),
            Some("SELECT DISTINCT \"customer_id\" FROM \"order\""),
            "取数语句要能直接照着写"
        );
        // 三步提示：取 ID → 按 ref 取 → 拼结果。
        let text = pending[0].hint.as_deref().expect("跨源边必须带三步 hint");
        assert!(text.contains("①") && text.contains("②") && text.contains("③"), "{text}");
        assert!(text.contains("cross_source") || text.contains("不做跨源 join"), "{text}");
    }

    /// 反向谓词写出来的跨源边也要被认出来（`customers__order` 是同一条边）。
    ///
    /// 变异：`bind` 只登记前向谓词 ⇒ 本测试红（那条遍历会被当成库源查询真的去跑 SQL）。
    #[test]
    fn the_inverse_predicate_of_a_cross_source_edge_is_recognised_too() {
        let m = mapping();
        let c = check_of(&m, "SELECT ?o ?c WHERE { ?c :customers__order ?o }");
        assert_eq!(c.pending().len(), 1, "{c:?}");
        assert_eq!(c.pending()[0].reason, REASON_CROSS_SOURCE_EDGE);
        let walk = check_of(
            &m,
            "SELECT ?n ?o WHERE { ?c :customers__order ?o . ?c :customers__customer_name ?n }",
        );
        assert!(walk.cross_source().is_some(), "反向谓词也要能判出跨源遍历：{walk:?}");
    }

    #[test]
    fn asking_the_api_side_of_a_cross_source_edge_is_cross_source_unsupported() {
        let m = mapping();
        let c = check_of(
            &m,
            "SELECT ?o ?n WHERE { ?o a :order . ?o :order__customers ?c . ?c :customers__customer_name ?n }",
        );
        let (predicate, from, to) = c.cross_source().expect("应当报跨源");
        assert_eq!(predicate, "order__customers");
        assert_eq!((from, to), ("order", "customers"));
    }

    // ── ⭐ REQ-151 · 「混合查询」判定（只给本体模式的闸门用）────────

    /// 纯 api 查询：`needs_db = false` ⇒ 本体模式可以给 `@pending`。
    #[test]
    fn a_pure_api_query_does_not_need_the_db() {
        let m = mapping();
        let c = check_of(&m, "SELECT ?c WHERE { ?c a :customers }");
        assert_eq!(c.pending().len(), 1, "{c:?}");
        assert!(!c.needs_db(), "只有 api 类 ⇒ 不需要库：{c:?}");
    }

    /// 混合查询（同时点库类与 api 类）：仍是 `Api`（普通模式行为不变），但 `needs_db = true`。
    ///
    /// 变异：让 `db_locals` 恒空 ⇒ 本测试红（本体模式会把混合查询当成纯 api，返回 200）。
    #[test]
    fn a_mixed_query_is_api_but_needs_the_db() {
        let m = mapping();
        let c = check_of(
            &m,
            "SELECT ?o ?c WHERE { ?o a :order . ?c a :customers }",
        );
        assert_eq!(c.pending().len(), 1, "{c:?}");
        assert!(c.needs_db(), "点了库类 ⇒ 混合查询：{c:?}");
    }

    /// 只走跨源边（没有点名库类）：一端在库 ⇒ 也要库。
    #[test]
    fn a_cross_source_edge_needs_the_db() {
        let m = mapping();
        let c = check_of(&m, "SELECT ?o ?c WHERE { ?o :order__customers ?c }");
        assert_eq!(c.pending().len(), 1, "{c:?}");
        assert!(c.needs_db(), "跨源边的一端在库：{c:?}");
    }

    /// 库类属性写成 `<类>__<列>` 也要被认成「点了库类」。
    #[test]
    fn a_db_class_property_counts_as_touching_the_db() {
        let m = mapping();
        let c = check_of(
            &m,
            "SELECT ?c ?n WHERE { ?c a :customers . ?c :customers__customer_name ?n . ?o :order__id ?x }",
        );
        assert!(c.needs_db(), "{c:?}");
    }
}
