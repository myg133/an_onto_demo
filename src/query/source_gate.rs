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
//! - `mapping_version` **必填**（`meta.mapping_version` 与 `get.ref` 的 `@v<n>` 都取它，
//!   用于「映射与本体版本对得上」的追溯 —— 契约规则 3）；
//! - `entities[].class` 是**本体里的类 key**（= 快照 `entity_types.key`，`catalog` 的 `table`）；
//!   它必须在**这一版本体里真实存在**，否则拒绝启动（⛔ 不静默忽略一条映射）；
//! - `entities[].id` 是 access 契约里的条目名（`get.ref` = `access#<id>@v<version>`）；
//! - `capability.pushdown` ⇒ `access.pushdown_fields` / `get.pushdown`（省略 = `[]`，
//!   ⛔ 不替调用方猜一个能力）；
//! - `freshness` 可选（省略 ⇒ `access` 里不出现这个键，⛔ 不编一个「实时」）。
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

use serde::Deserialize;

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
    /// 单个类最多声明一次（重复 ⇒ 加载期报错）；按类 key 升序。
    entities: Vec<Entity>,
    /// 类 key → `entities` 下标（唯一索引）。
    by_class: BTreeMap<String, usize>,
    /// TTL 用的 api 类集合（`bind` 之后才有值：人读名要从本体取）。
    api: ApiClasses,
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
    entities: Vec<RawEntity>,
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
    freshness: Option<String>,
    // 下面这些在契约里存在、本模块**不解释**（读进来就丢，见模块头）。
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    operation: Option<serde_json::Value>,
    #[serde(default)]
    #[allow(dead_code)]
    iterator: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    iri_template: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    fields: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawCapability {
    #[serde(default)]
    pushdown: Vec<String>,
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
        if let Some(cv) = doc.contract_version {
            if cv == 0 {
                return Err("`contract_version` 是 0。契约版本从 1 起。".to_string());
            }
        }
        if doc.entities.is_empty() {
            // 「加载了映射但一条 api 类都没有」是一个可用的状态（全是库源），但要说出来 ——
            // 否则运维会以为映射生效了。这里不当作错误，只由调用方打一行提示。
        }

        let mut entities: Vec<Entity> = Vec::with_capacity(doc.entities.len());
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        for (i, e) in doc.entities.into_iter().enumerate() {
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
                .map(|c| c.pushdown)
                .unwrap_or_default()
                .into_iter()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect::<Vec<String>>();
            pushdown.sort();
            pushdown.dedup();
            let freshness = e
                .freshness
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            entities.push(Entity {
                id,
                class,
                pushdown,
                freshness,
            });
        }
        entities.sort_by(|a, b| a.class.cmp(&b.class));

        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        for (i, e) in entities.iter().enumerate() {
            by_class.insert(e.class.clone(), i);
        }

        Ok(Self {
            mapping_version,
            entities,
            by_class,
            api: ApiClasses::default(),
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
            // 普通属性谓词：`<类>__<列>`，类在 api ⇒ 点名了 api 类。
            if let Some((owner, _rest)) = pred_local.split_once("__") {
                if let Some(key) = self.key_of_local(owner) {
                    api_hits.insert(key.to_string());
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetInstruction {
    /// 要取哪个类（SPARQL 侧的名字）。
    pub class: String,
    /// 已知的实例标识（空 = 平台不知道，按类取）。
    pub ids: Vec<String>,
    /// 可下推的过滤字段（来自 `capability.pushdown`；空 = 映射没说）。
    pub pushdown: Vec<String>,
    /// `access#<id>@v<mapping_version>`。
    pub reference: String,
}

/// 一条查询的**源判定**（[`AccessMapping::check`] 的返回值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceCheck {
    /// 只碰库源 ⇒ 正常执行。
    DbOnly,
    /// 碰到 api 源 ⇒ 不执行 SQL，改为 pending（jsonld）/ 报错（rows）。
    Api { pending: Vec<ApiPending> },
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
            SourceCheck::Api { pending } => pending,
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
}
