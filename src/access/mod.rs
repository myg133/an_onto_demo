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
//! # 本模块的边界（阶段 0）
//!
//! | 做 | 不做 |
//! | --- | --- |
//! | 契约结构 + 解析（作者格式 JSON） | ⛔ 真实 HTTP 调用（阶段 1） |
//! | `validate` 的四条有牙检查（+ 结构与本体自洽） | ⛔ TC3 签名（阶段 1） |
//! | `fetch --dry-run-fixture`（分页 / 本地过滤 / JSON-LD / meta） | ⛔ 跨源 join（v1 边界） |
//! | `publish`：JSON → JSON-LD（RML 风格，供 SPARQL 查） | ⛔ 任何 `query serve` 改动（REQ-145） |
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
//! | `entities[]` | `id` / `class` / `source` / `operation` / `iterator` / `identity` / `iri_template` / `fields` / `paging` / `capability` / `freshness` / `reviewed_by` / `fixture` | 缺一样，执行器就得**猜**；猜出来的东西没法审 |
//! | `relations[]` | `predicate` / `from` / `to` / `via` / `cross_source` / `traversable` | 跨源边要能被标记（demand §三 R-4） |
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
//! | 9 | 有 `fixture` ⇒ `--dry-run` 必须能跑 | 阶段 0 **只**有这条通路（[`fetch`]） |
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
//! - 阶段 0 **不发任何网络请求**：`fetch` 只有 `--dry-run-fixture` 一条通路，
//!   省略它会被明确拒绝（`DRY_RUN_REQUIRED`），⛔ 不静默降级成「读 fixture 冒充真数据」。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

pub mod fetch;
pub mod path;
pub mod publish;
pub mod validate;

/// 契约版本。破坏性变更 ⇒ +1（设计稿规则 10）。
pub const CONTRACT_VERSION: u32 = 1;

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
/// 阶段 0：`fetch` 必须显式 `--dry-run-fixture`（⛔ 不发网络请求）。
pub const CODE_DRY_RUN_REQUIRED: &str = "DRY_RUN_REQUIRED";
/// `--entity` 指的那条映射不存在。
pub const CODE_ENTITY_UNKNOWN: &str = "ENTITY_NOT_IN_MAPPING";

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
#[derive(Debug, Clone, Serialize)]
pub struct Auth {
    pub mode: String,
    pub credential_env: Vec<String>,
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
    /// 过滤名 → 请求参数名（`{"zone":"Zone"}`）。缺省 ⇒ 过滤名即参数名。
    pub pushdown_requests: BTreeMap<String, String>,
    pub capability: Capability,
    pub freshness: Freshness,
    /// 离线 fixture（阶段 0 **必填**：没有它这条映射无法被验）。
    pub fixture: String,
}

/// API 动作（`ontok:apiAction` + 三个 HTTP 细节）。
#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub action: String,
    pub version: Option<String>,
    pub method: String,
    pub content_type: String,
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
    let mut version_ok = true;
    match opt_u64(top, "contract_version") {
        Some(v) if v as u32 == CONTRACT_VERSION => {}
        Some(v) => {
            version_ok = false;
            problems.push(Problem::new(
                CODE_SCHEMA,
                "字段 contract_version",
                format!(
                    "契约版本 {v} 不认识（本程序只认 {CONTRACT_VERSION}）。\
                     ⛔ 破坏性变更必须升版本号，而不是让读端去猜字段。"
                ),
            ));
        }
        None => {
            version_ok = false;
            problems.push(Problem::new(
                CODE_SCHEMA,
                "字段 contract_version",
                format!("缺必填整数字段（本程序认 {CONTRACT_VERSION}）。"),
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
        contract_version: CONTRACT_VERSION,
        mapping_version,
        project: project.unwrap_or_default(),
        ontology_version,
        sources,
        entities,
        relations,
    })
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
    let auth = obj.get("auth").and_then(|a| a.as_object()).map(|a| Auth {
        mode: opt_str(a, "mode").unwrap_or_else(|| "none".to_string()),
        credential_env: opt_str_list(a, "credential_env").unwrap_or_default(),
    });
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
            let action = need_str(o, "action", &format!("{loc} 字段 operation"), problems)?;
            Some(Operation {
                action,
                version: opt_str(o, "version"),
                method: opt_str(o, "method").unwrap_or_else(|| "POST".to_string()),
                content_type: opt_str(o, "content_type")
                    .unwrap_or_else(|| "application/json".to_string()),
            })
        });
    if operation.is_none() && !obj.contains_key("operation") {
        problems.push(Problem::new(
            CODE_SCHEMA,
            format!("{loc} 字段 operation"),
            "缺必填对象（至少要 `{\"action\":\"DescribeInstances\"}`）。",
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
    let paging_obj = obj
        .get("paging")
        .and_then(|p| p.as_object())
        .or_else(|| {
            obj.get("request")
                .and_then(|r| r.as_object())
                .and_then(|r| r.get("paging"))
                .and_then(|p| p.as_object())
        });
    let paging = paging_obj.map(|p| {
        let loc2 = format!("{loc} 字段 paging");
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

    // 过滤名 → 请求参数名（`request.from_filter`）。
    let pushdown_requests = obj
        .get("request")
        .and_then(|r| r.as_object())
        .and_then(|r| r.get("from_filter"))
        .and_then(|f| f.as_object())
        .map(|f| {
            f.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

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

    let fixture = need_str(obj, "fixture", &loc, problems).or_else(|| {
        // need_str 已经记了一条；这里只保证返回 None 时不再重记。
        None
    });

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
        pushdown_requests,
        capability,
        freshness,
        fixture: fixture?,
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
                "operation": { "action": "DescribeInstances" },
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
