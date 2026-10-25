//! `ontok access fetch --dry-run-fixture` —— 用 fixture 跑**完整取数流程**（⛔ 不发任何网络请求）。
//!
//! # 一次取数被拆成六步（每一步都能单独解释、单独判）
//!
//! | # | 步骤 | 判据 |
//! | --- | --- | --- |
//! | 1 | 这张映射**签过**吗（R6） | 未签 ⇒ `UNREVIEWED_MAPPING`，⛔ 一行都不取 |
//! | 2 | 读 fixture、按 `iterator` 取行 | 选不出 ⇒ `ITERATOR_EMPTY` |
//! | 3 | 过滤条件**分类**（pushdown / local_only / 未知） | 未知且不是已声明字段 ⇒ `UNKNOWN_FILTER_FIELD` |
//! | 4 | 模拟服务端下推（pushdown 是「返回更少的行」） | 模拟不了的下推 ⇒ `meta.unsimulated_pushdown` + 警告，⛔ 不假装筛过 |
//! | 5 | **分页**取行，⛔ **取页前**判 `max_scan_rows` 预算（R5） | 会超 ⇒ `SCAN_LIMIT_EXCEEDED`，**不给部分结果** |
//! | 6 | 本地过滤 + 组装 JSON-LD + `meta` | `filtered_locally` / `freshness` / `capability_warnings` 三项必须在 |
//!
//! # 为什么 R5 的预算判在**取页之前**
//!
//! 设计稿规则 5 的原话是「扫描行数超 `max_scan_rows` ⇒ 拒绝并说明，⛔ 不许偷偷全量拉」。
//! 两种实现的差别是**可观测**的：
//!
//! ```text
//! 拉完再判：rows_scanned 已经 = 全量（比如 5 万行），然后报错说「你超预算了」
//! 取页前判：rows_scanned 停在预算之内（最多上界一次），报错时已经花掉的钱是可控的
//! ```
//!
//! 所以这里的判法是：**下一若要取的行数会让 `rows_scanned + 计划页` 超过 `max_scan_rows`
//! ⇒ 立刻拒绝**，⛔ 不先拉回来再判，也⛔ 不返回「已经拿到的部分」。
//!
//! # 「下推」在离线 fixture 上怎么模拟（这是本模块最需要小心的一处）
//!
//! 真 API 的下推是**服务端**行为：它返回的行**本来就少**。fixture 是一份固定响应，
//! 所以「模拟下推」= 先按声明的路径把 fixture 行**筛一遍**，再在筛后的行上分页。
//!
//! 筛的口径只用**契约里已有的信息**：
//!
//! | 过滤名 | 用哪条路径筛 | 为什么 |
//! | --- | --- | --- |
//! | 恰好等于 `identity.key_path` 或字面量 `ids` | `identity.key_path` | 「按 id 取一批」是每个云 API 都有的形态 |
//! | 已声明的字段（`fields.<k>.path`） | 那条路径 | 下推的语义就是「服务端按这个字段筛」 |
//! | 其它（例如请求参数 `Region` 在响应里没有对应字段） | **模拟不了** | ⇒ 进 `meta.unsimulated_pushdown` + `capability_warnings`，⛔ 不假装筛过 |
//!
//! # `meta` 的五个字段（AC-008）为什么是这五个
//!
//! | 字段 | 它回答的问题 |
//! | --- | --- |
//! | `rows_scanned` | 这次**真的读了多少行**（= 花掉多少配额）；与 `rows_returned` 的差就是被筛掉的量 |
//! | `filtered_locally` | 哪些条件是**我们自己**筛的 —— Agent 不能以为服务端筛过了 |
//! | `freshness` | 这批数字是**哪个时点**的（运维最怕「数字对但时点不对」） |
//! | `capability_warnings` | 映射的能力声明与这次请求对不上的地方（不报错，但必须说） |
//! | `truncated` | 给的是**全量**还是被 `--limit` 截过 |
//!
//! 另外三栏是**对齐交互契约**用的（`@pending[].get` 那一侧要能对上号）：
//!
//! | 字段 | 它回答的问题 |
//! | --- | --- |
//! | `mapping_ref` | 取的是**哪一版**映射：`access#<entity.id>@v<mapping_version>`（与 `get.ref` 同一拼法） |
//! | `pushdown_applied` | 哪些过滤条件**真的**在服务端筛过（⛔ 与 `unsimulated_pushdown` 不相交） |
//! | `entity` / `class` | 这条回执属于哪条映射、映射到本体的哪个类 |
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 阶段 0 **没有**真调用：`--dry-run-fixture` 是唯一通路，省略它直接拒绝（`DRY_RUN_REQUIRED`）。
//!   因此 `freshness.fetched_at` 是**本机时钟**（回执用），不参与任何判定；
//!   另给 `freshness.source = "fixture"` 明说「这不是现场数据」。
//! - 下推模拟的「总行数」取 `paging.total_path` 与筛后行数的**较小值**：
//!   fixture 里的 `TotalCount` 是**原始**响应的，筛过之后以实际可返回的为准。
//! - 本地过滤只支持**相等 / 多值包含**（云 API 的过滤几乎都是这两种）；
//!   ⛔ 不做比较运算（`> <`）—— 那需要契约新增语法（版本只增不减）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::{
    expand_iri_template, fill_iri_template, path, AccessError, Capability, Entity, Mapping,
    Ontology, CODE_ENTITY_UNKNOWN, CODE_ITERATOR_EMPTY, CODE_KEY_MISSING, CODE_SCAN_LIMIT,
    CODE_UNKNOWN_FILTER, CODE_UNREVIEWED,
};

use crate::query::jsonld::{ONTOK_PREFIX, XSD_NS};

/// 一个过滤值（`{"ids": ["a","b"]}` 是合法形态，所以不是单一字符串）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterValue {
    /// 单值相等。
    One(String),
    /// 多值包含（`ids` 这种）。
    Many(Vec<String>),
}

impl FilterValue {
    /// 从 `--filter` 的 JSON 值构造：字符串 / 数字 / 布尔 / 字符串数组。
    pub fn from_json(v: &Value) -> Option<Self> {
        match v {
            Value::String(s) => Some(FilterValue::One(s.clone())),
            Value::Number(n) => Some(FilterValue::One(n.to_string())),
            Value::Bool(b) => Some(FilterValue::One(b.to_string())),
            Value::Array(items) => {
                let mut out = Vec::new();
                for it in items {
                    out.push(match it {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => return None,
                    });
                }
                Some(FilterValue::Many(out))
            }
            _ => None,
        }
    }

    /// 命中吗（`null` / 缺失 ⇒ 不命中）。
    fn matches(&self, actual: Option<&Value>) -> bool {
        match actual {
            None | Some(Value::Null) => false,
            Some(v) => {
                let text = scalar_text(v);
                match self {
                    FilterValue::One(want) => text.as_deref() == Some(want.as_str()),
                    FilterValue::Many(want) => {
                        text.map(|t| want.iter().any(|w| w == &t)).unwrap_or(false)
                    }
                }
            }
        }
    }
}

/// 标量 → 文本（对象 / 数组返回 `None`：它们没法当过滤值）。
fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 取数请求。
#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    /// `--filter` 的解析结果（过滤名 → 值）。
    pub filter: BTreeMap<String, FilterValue>,
    /// 客户端要多少行（`--limit`）。
    pub limit: usize,
}

/// `freshness` 的回执形态（规则 6：**回执要带上**）。
#[derive(Debug, Clone, Serialize)]
pub struct FreshnessReceipt {
    /// `realtime` / `daily` / `unknown` —— 原样转发契约里的声明。
    pub mode: String,
    /// 契约里作者写的注意事项（可为空）。
    pub note: Option<String>,
    /// 这批数字的时点（阶段 0 = 本机时钟；⛔ 不参与任何判定）。
    pub fetched_at: String,
    /// `fixture` ⇒ 这是离线夹具，不是现场数据。
    pub source: String,
}

/// 取数回执（与 `query serve` 的 `meta` 是**同一套纪律**，不是同一组字段）。
#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub rows_scanned: usize,
    pub pages: usize,
    pub rows_matched: usize,
    pub rows_returned: usize,
    pub filtered_locally: Vec<String>,
    pub freshness: FreshnessReceipt,
    pub capability_warnings: Vec<String>,
    pub truncated: bool,
    /// 本次生效的 `--limit`。
    pub limit: usize,
    /// 本次生效的 `max_scan_rows`（R5 的预算）。
    pub max_scan_rows: u64,
    /// 声明了下推、但 fixture 里没有对应路径因而**没能模拟**的过滤名。
    pub unsimulated_pushdown: Vec<String>,
    /// 本次**真的**在（模拟的）服务端筛过的过滤名。
    pub pushdown_applied: Vec<String>,
    /// 这份数据对应契约里的哪一条 —— `access#<entity.id>@v<mapping_version>`。
    ///
    /// 与交互契约 §3.3 的 `@pending[].get.ref` **同一个拼法**：
    /// Agent 拿到 `get.ref` 之后，取回来的回执要靠它证明「取的是同一版映射」。
    pub mapping_ref: String,
    /// 本次取的映射条目 id 与它映射到本体里的类。
    pub entity: String,
    pub class: String,
}

/// 一份取数结果（`@context` / `@graph` / `summary` / `meta` —— 与 `query serve` 同形状）。
#[derive(Debug, Clone, Serialize)]
pub struct JsonLdDoc {
    #[serde(rename = "@context")]
    pub context: BTreeMap<&'static str, String>,
    #[serde(rename = "@graph")]
    pub graph: Vec<Value>,
    pub summary: Value,
    pub meta: Meta,
}

impl JsonLdDoc {
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!("{{\"error\":\"JSON-LD 序列化失败：{e}\"}}"))
    }
}

/// 用 fixture 跑一遍完整取数流程。**任何一步失败都不返回部分结果。**
pub fn dry_run(
    mapping: &Mapping,
    ontology_iri: &str,
    entity_id: &str,
    opts: &FetchOptions,
    mapping_dir: &Path,
) -> Result<JsonLdDoc, AccessError> {
    let entity = mapping.entity(entity_id).ok_or_else(|| {
        AccessError::new(
            CODE_ENTITY_UNKNOWN,
            format!(
                "`{entity_id}` 不在这份契约里（有：{}）。",
                mapping
                    .entities
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" / ")
            ),
        )
    })?;

    // ── 1 · R6：未签的映射不许取数 ───────────────────────────
    if !entity.reviewed() {
        return Err(AccessError::new(
            CODE_UNREVIEWED,
            format!(
                "entity `{}` 的 `reviewed_by` 为空 —— 未签的映射不许取数（R6 / ADR-005）。\n\
                 AI 生成的映射也是**提案**：先让人把 `reviewed_by` 签上\
                 （`\"张三@2026-10-25\"`），再跑取数。",
                entity.id
            ),
        ));
    }

    // `--limit 0` 会得到一份「扫了 N 行、返回 0 行、truncated=true」的怪回执 ⇒ 直接拒。
    if opts.limit == 0 {
        return Err(AccessError::new(
            super::CODE_SCHEMA,
            "`--limit` 必须 ≥ 1（0 行没有意义，而回执会看起来像「查了但什么都没有」）。"
                .to_string(),
        ));
    }

    // ── 2 · 读 fixture、按 iterator 取行 ──────────────────────
    let doc = read_fixture(mapping_dir, entity)?;
    let all_rows = path::iterate(&doc, &entity.iterator);
    if all_rows.is_empty() {
        return Err(AccessError::new(
            CODE_ITERATOR_EMPTY,
            format!(
                "`iterator` = `{}` 在 fixture 里选不出任何节点（先跑 `ontok access validate`）。",
                entity.iterator
            ),
        ));
    }

    // ── 3 · 过滤条件分类 ────────────────────────────────────
    let mut local_keys: Vec<String> = Vec::new();
    let mut local_filter: Vec<(String, FilterValue)> = Vec::new();
    let mut pushdown_filters: Vec<(String, FilterValue)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut unsimulated: Vec<String> = Vec::new();

    for (key, want) in &opts.filter {
        let in_pushdown = entity.capability.pushdown.iter().any(|f| f == key);
        let in_local = entity.capability.local_only.iter().any(|f| f == key);
        if in_pushdown && in_local {
            // R4 该拦住的形态（validate 已经报错）；执行器按**更保守**的一侧处理。
            warnings.push(format!(
                "字段 `{key}` 同时写在 `pushdown` 与 `local_only`（R4 不许）：\
                 本次按**本地过滤**处理，⛔ 不假设服务端筛过。"
            ));
            local_keys.push(key.clone());
            local_filter.push((key.clone(), want.clone()));
        } else if in_local {
            local_keys.push(key.clone());
            local_filter.push((key.clone(), want.clone()));
            // 设计稿 §四 的示例回执里就有这一条：Agent 必须知道**是我们筛的**。
            warnings.push(format!(
                "字段 `{key}` 不在 `pushdown` 里（服务端不支持按它筛）⇒ 已在本地过滤"
            ));
        } else if in_pushdown {
            pushdown_filters.push((key.clone(), want.clone()));
        } else if entity.fields.contains_key(key) {
            // 是已声明的字段、却没声明能力面 ⇒ 按本地过滤（保守），并明确告知。
            warnings.push(format!(
                "字段 `{key}` 既不在 `pushdown` 也不在 `local_only` —— 本次按**本地过滤**处理。\
                 建议在契约里显式声明它属于哪一侧（R4：不写等于暗示「都能下推」）。"
            ));
            local_keys.push(key.clone());
            local_filter.push((key.clone(), want.clone()));
        } else {
            return Err(AccessError::new(
                CODE_UNKNOWN_FILTER,
                format!(
                    "过滤条件 `{key}` 不认识：它既不在 `capability.pushdown`、也不在 \
                     `capability.local_only`，还不是 `fields` 里声明的字段。\
                     可用的过滤名：pushdown={:?} local_only={:?} fields={:?}。",
                    entity.capability.pushdown,
                    entity.capability.local_only,
                    entity.fields.keys().collect::<Vec<_>>()
                ),
            ));
        }
    }

    // ── 4 · 模拟服务端下推 ──────────────────────────────────
    let server_rows: Vec<&Value> = {
        let mut rows: Vec<&Value> = all_rows.clone();
        for (key, want) in &pushdown_filters {
            match filter_path(entity, key) {
                Some(p) => rows.retain(|row| want.matches(path::select(row, &p))),
                None => {
                    // 声明了下推，但 fixture 里没有能对上的路径 ⇒ 阶段 0 无法模拟。
                    unsimulated.push(key.clone());
                    warnings.push(format!(
                        "字段 `{key}` 声明为下推，但 fixture 里没有能对上它的路径\
                         （响应里没有这个字段）⇒ 阶段 0 **模拟不了**服务端过滤，\
                         本次结果**未**按 `{key}` 过滤。真实调用时这一步由服务端完成。"
                    ));
                }
            }
        }
        rows
    };

    // ── 5 · 分页 + R5 预算（⛔ 取页前判）─────────────────────
    let page_size = entity
        .paging
        .as_ref()
        .map(|p| p.max_limit)
        .filter(|n| *n > 0)
        .unwrap_or(100);
    let available = server_rows.len();
    let total_from_api = entity
        .paging
        .as_ref()
        .and_then(|p| p.total_path.as_ref())
        .and_then(|tp| path::select(&doc, tp))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);
    // fixture 里的 TotalCount 是**原始**响应的；筛过之后以实际可返回的为准。
    let total = total_from_api.map(|t| t.min(available)).unwrap_or(available);

    let mut rows_scanned = 0usize;
    let mut pages = 0usize;
    let mut matched = 0usize;
    let mut kept: Vec<&Value> = Vec::new();
    let mut offset = 0usize;
    let mut truncated = false;

    while offset < total {
        let planned = page_size.min(total - offset);
        if planned == 0 {
            break;
        }
        // ⭐ R5：**取页前**判预算 —— 超了立刻拒绝，⛔ 不给部分结果。
        if rows_scanned as u64 + planned as u64 > entity.capability.max_scan_rows {
            return Err(AccessError::new(
                CODE_SCAN_LIMIT,
                format!(
                    "需要继续扫描就会超过 `max_scan_rows`：已扫 {rows_scanned} 行（{pages} 页），\
                     再取一页要 {planned} 行 —— 合计会到 {}，预算只有 {}。\n\
                     ⛔ 拒绝返回部分结果。请**改查询**（加更窄的过滤 / 调小 --limit），\
                     ⛔ 不要重试 —— 重试只会再撞同一面墙。",
                    rows_scanned + planned,
                    entity.capability.max_scan_rows
                ),
            ));
        }
        let end = (offset + planned).min(server_rows.len());
        let page = &server_rows[offset..end];
        rows_scanned += page.len();
        pages += 1;
        for row in page {
            // 键取不到 ⇒ 拼不出 IRI：这是**契约错**，不是数据空。
            if !path::exists(row, &entity.key_path) {
                return Err(AccessError::new(
                    CODE_KEY_MISSING,
                    format!(
                        "第 {} 页有一行取不到键 `{}` —— 拼不出实例 IRI（先跑 validate 再取数）。",
                        pages, entity.key_path
                    ),
                ));
            }
            if local_filter.iter().all(|(k, want)| {
                let p = local_path(entity, k);
                match p {
                    Some(p) => want.matches(path::select(row, &p)),
                    // 本地过滤用的字段在响应里没有 ⇒ 该行不命中（⛔ 不放行，
                    // 否则会把「路径写歪」变成「条件被忽略」）。
                    None => false,
                }
            }) {
                matched += 1;
                if kept.len() < opts.limit {
                    kept.push(row);
                }
            }
        }
        offset = end;
        if kept.len() >= opts.limit {
            // 够了：还有没有剩下的行？有 ⇒ 这次是**截断**的。
            truncated = offset < server_rows.len();
            break;
        }
    }
    if matched > kept.len() {
        truncated = true;
    }

    // ── 6 · 组装 JSON-LD + meta ─────────────────────────────
    // 类的本地名以**契约里写的那个类名**为准（`@type` 与谓词前缀都用它，
    // 与 `query serve` 的 `ontok:{class}__{prop}` 同一套约定）。
    let class_local = Ontology::local_name(&entity.class).to_string();
    let template = expand_iri_template(&entity.iri_template, ontology_iri);
    let graph = assemble_graph(&kept, entity, &class_local, &template, &mut warnings);

    let mut context: BTreeMap<&'static str, String> = BTreeMap::new();
    context.insert(ONTOK_PREFIX, format!("{ontology_iri}#"));
    context.insert("xsd", XSD_NS.to_string());

    let mut summary = Map::new();
    summary.insert("nodes".to_string(), json!(graph.len()));
    summary.insert("row_count".to_string(), json!(rows_scanned));

    // ⚠️ `unsimulated` 里的名字**不进** `pushdown_applied`：声称下推了却没筛，
    // 比不声明更糟（Agent 会以为服务端筛过了）。
    let mut pushdown_applied: Vec<String> = pushdown_filters
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !unsimulated.contains(k))
        .collect();
    pushdown_applied.sort();

    let meta = Meta {
        rows_scanned,
        pages,
        rows_matched: matched,
        rows_returned: graph.len(),
        filtered_locally: local_keys,
        freshness: FreshnessReceipt {
            mode: entity.freshness.mode.clone(),
            note: entity.freshness.note.clone(),
            fetched_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            source: "fixture".to_string(),
        },
        capability_warnings: warnings,
        truncated,
        limit: opts.limit,
        max_scan_rows: entity.capability.max_scan_rows,
        unsimulated_pushdown: unsimulated,
        pushdown_applied,
        // 与交互契约 §3.3 的 `@pending[].get.ref` 同一拼法：`access#<id>@v<n>`。
        mapping_ref: format!("access#{}@v{}", entity.id, mapping.mapping_version),
        entity: entity.id.clone(),
        class: entity.class.clone(),
    };

    Ok(JsonLdDoc {
        context,
        graph,
        summary: Value::Object(summary),
        meta,
    })
}

/// 过滤名 → 用来筛 fixture 的路径（下推一侧）。
fn filter_path(entity: &Entity, key: &str) -> Option<String> {
    if key == "ids" || key == entity.key_path {
        return Some(entity.key_path.clone());
    }
    entity.fields.get(key).map(|f| f.path.clone())
}

/// 过滤名 → 本地过滤用的路径（与 [`filter_path`] 同口径，刻意不另立第二套规则）。
fn local_path(entity: &Entity, key: &str) -> Option<String> {
    filter_path(entity, key)
}

fn read_fixture(mapping_dir: &Path, entity: &Entity) -> Result<Value, AccessError> {
    let p: PathBuf = super::validate::resolve_fixture(mapping_dir, &entity.fixture);
    let text = std::fs::read_to_string(&p).map_err(|e| {
        AccessError::new(
            super::CODE_FIXTURE_UNREADABLE,
            format!("读不到 fixture `{}`：{e}", p.display()),
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        AccessError::new(
            super::CODE_FIXTURE_UNREADABLE,
            format!("`{}` 不是合法 JSON：{e}", p.display()),
        )
    })
}

/// 行 → 节点（同 `@id` 合并属性；键序 / 节点序都按字典序 ⇒ 输出确定性）。
fn assemble_graph(
    rows: &[&Value],
    entity: &Entity,
    class_local: &str,
    template: &str,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let mut nodes: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut skipped_objects: BTreeMap<String, usize> = BTreeMap::new();

    for row in rows {
        let Some(key) = path::select(row, &entity.key_path).and_then(scalar_text) else {
            // 上层已经保证键存在；这里只是不 panic 的兜底。
            continue;
        };
        let iri = fill_iri_template(template, class_local, &key);
        let entry = nodes.entry(iri).or_default();
        for (name, f) in &entity.fields {
            let Some(v) = path::select(row, &f.path) else {
                continue; // NULL 不产生三元组（与 R2RML 同口径）
            };
            if v.is_object() || v.is_array() {
                *skipped_objects.entry(name.clone()).or_insert(0) += 1;
                continue;
            }
            entry.insert(format!("{ONTOK_PREFIX}:{class_local}__{name}"), typed_json(v, f.ty.as_deref()));
        }
    }

    for (name, n) in skipped_objects {
        warnings.push(format!(
            "字段 `{name}` 有 {n} 行的值是对象 / 数组 —— JSON-LD 里没法当字面量，已跳过。\
             若要保留它，请在契约里改成一个标量路径（`fields.{name}.path`）。"
        ));
    }

    let mut graph = Vec::with_capacity(nodes.len());
    for (iri, props) in nodes {
        let mut obj = Map::new();
        obj.insert("@id".to_string(), json!(iri));
        obj.insert("@type".to_string(), json!(format!("{ONTOK_PREFIX}:{class_local}")));
        for (k, v) in props {
            obj.insert(k, v);
        }
        graph.push(Value::Object(obj));
    }
    graph
}

/// 响应值 + 声明的 `xsd` 类型 → JSON-LD 值（与 `query::jsonld::literal_json` 同口径）。
fn typed_json(v: &Value, ty: Option<&str>) -> Value {
    let local = ty.unwrap_or("string").trim();
    let local = local.strip_prefix("xsd:").unwrap_or(local);
    match v {
        Value::Number(_) | Value::Bool(_) if local == "string" => v.clone(),
        Value::Number(_) | Value::Bool(_) => v.clone(),
        Value::String(s) => match local {
            "integer" | "int" | "long" => match s.trim().parse::<i64>() {
                Ok(n) => json!(n),
                Err(_) => json!(s),
            },
            "decimal" | "double" | "float" => match s.trim().parse::<serde_json::Number>() {
                Ok(n) => Value::Number(n),
                Err(_) => json!(s),
            },
            "boolean" => match s.trim() {
                "true" | "t" | "1" => json!(true),
                "false" | "f" | "0" => json!(false),
                _ => json!(s),
            },
            "date" => json!({ "@value": s, "@type": "xsd:date" }),
            "dateTime" => json!({ "@value": s, "@type": "xsd:dateTime" }),
            _ => json!(s),
        },
        other => other.clone(),
    }
}

/// `capability` 的一句人读摘要（stderr 用；⛔ 不参与判定）。
pub fn capability_hint(c: &Capability) -> String {
    format!(
        "可下推 {:?} · 仅本地 {:?} · 扫描预算 {} 行",
        c.pushdown, c.local_only, c.max_scan_rows
    )
}

/// 仅供测试与诊断：报告某个过滤名被归到哪一侧。
pub fn classify_filter(entity: &Entity, key: &str) -> &'static str {
    let p = entity.capability.pushdown.iter().any(|f| f == key);
    let l = entity.capability.local_only.iter().any(|f| f == key);
    match (p, l) {
        (true, true) => "overlap",
        (true, false) => "pushdown",
        (false, true) => "local_only",
        (false, false) => {
            if entity.fields.contains_key(key) {
                "undeclared_field"
            } else {
                "unknown"
            }
        }
    }
}

/// 供 CLI 用：把 `--filter` 的 JSON 文本解成 [`FetchOptions::filter`]。
pub fn parse_filter(text: &str) -> Result<BTreeMap<String, FilterValue>, AccessError> {
    let v: Value = serde_json::from_str(text).map_err(|e| {
        AccessError::new(
            super::CODE_SCHEMA,
            format!("`--filter` 不是合法 JSON 对象：{e}（例：'{{\"zone\":\"ap-guangzhou-3\"}}'）"),
        )
    })?;
    let obj = v.as_object().ok_or_else(|| {
        AccessError::new(
            super::CODE_SCHEMA,
            "`--filter` 必须是 JSON **对象**（`{\"zone\":\"…\"}`），不是数组或标量。".to_string(),
        )
    })?;
    let mut out = BTreeMap::new();
    for (k, val) in obj {
        let fv = FilterValue::from_json(val).ok_or_else(|| {
            AccessError::new(
                super::CODE_SCHEMA,
                format!("`--filter` 的 `{k}` 只支持字符串 / 数字 / 布尔 / 字符串数组。"),
            )
        })?;
        out.insert(k.clone(), fv);
    }
    Ok(out)
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

    /// 12 行：zone 交替，status = STOPPED 每 3 行一个（4 行）。
    fn fixture_json(n: usize) -> String {
        let rows: Vec<Value> = (1..=n)
            .map(|i| {
                json!({
                    "InstanceId": format!("ins-{i:04}"),
                    "InstanceName": format!("web-{i:04}"),
                    "InstanceState": if i % 3 == 0 { "STOPPED" } else { "RUNNING" },
                    "Placement": { "Zone": if i % 2 == 0 { "ap-guangzhou-2" } else { "ap-guangzhou-3" } },
                    "CPU": i % 8 + 1
                })
            })
            .collect();
        json!({ "Response": { "TotalCount": n, "InstanceSet": rows } }).to_string()
    }

    struct Env {
        dir: PathBuf,
    }

    impl Env {
        fn new(tag: &str, n: usize) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ontok-access-fetch-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("cvm.json"), fixture_json(n)).unwrap();
            Self { dir }
        }
    }

    fn mapping(reviewed: &str, max_scan: u64) -> String {
        format!(
            r#"{{
  "contract_version": 1,
  "mapping_version": 2,
  "project": "ops360",
  "sources": [{{ "id": "s1", "base_url": "https://cvm.tencentcloudapi.com" }}],
  "entities": [{{
    "id": "cvm_instance",
    "class": "云资源",
    "source": "s1",
    "reviewed_by": {reviewed},
    "operation": {{ "action": "DescribeInstances" }},
    "iterator": "$.Response.InstanceSet[*]",
    "identity": {{ "key_path": "InstanceId" }},
    "iri_template": "ontok:云资源/{{id}}",
    "fields": {{
      "name": {{ "path": "InstanceName" }},
      "status": {{ "path": "InstanceState" }},
      "zone": {{ "path": "Placement.Zone" }},
      "cpu": {{ "path": "CPU", "type": "xsd:integer" }}
    }},
    "paging": {{ "offset_param": "Offset", "limit_param": "Limit", "max_limit": 5, "total_path": "$.Response.TotalCount" }},
    "capability": {{ "pushdown": ["zone", "ids"], "local_only": ["status", "cpu"], "max_scan_rows": {max_scan} }},
    "freshness": {{ "mode": "realtime", "note": "实例状态可能有数秒延迟" }},
    "fixture": "cvm.json"
  }}]
}}"#
        )
    }

    fn run(tag: &str, n: usize, reviewed: &str, max_scan: u64, filter: &str, limit: usize)
        -> Result<JsonLdDoc, AccessError> {
        let env = Env::new(tag, n);
        let m = parse(&mapping(reviewed, max_scan)).expect("映射应当解析成功");
        let o = parse_ontology(TTL);
        let opts = FetchOptions { filter: parse_filter(filter).unwrap(), limit };
        dry_run(&m, &o.iri, "cvm_instance", &opts, &env.dir)
    }

    #[test]
    fn pages_and_local_filter() {
        // 12 行、每页 5 ⇒ 3 页（5/5/2）；status=STOPPED 是 4 行。
        let doc = run("pages", 12, "\"张三@2026-10-25\"", 5000, r#"{"status":"STOPPED"}"#, 50)
            .expect("dry-run 应当成功");
        assert_eq!(doc.meta.pages, 3, "12 行 / 每页 5 ⇒ 3 页");
        assert_eq!(doc.meta.rows_scanned, 12);
        assert_eq!(doc.meta.rows_matched, 4);
        assert_eq!(doc.meta.rows_returned, 4);
        assert_eq!(doc.meta.filtered_locally, vec!["status".to_string()]);
        assert!(!doc.meta.truncated);
        assert!(doc
            .meta
            .capability_warnings
            .iter()
            .any(|w| w.contains("status")));
    }

    #[test]
    fn limit_truncates_and_keeps_order() {
        let doc = run("limit", 12, "\"张三@2026-10-25\"", 5000, "{}", 3).expect("应当成功");
        assert_eq!(doc.meta.rows_returned, 3);
        assert!(doc.meta.truncated, "limit 截断必须标 truncated");
        let ids: Vec<&str> = doc
            .graph
            .iter()
            .map(|n| n["@id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec![
                "https://ontok.local/ops360#云资源/ins-0001",
                "https://ontok.local/ops360#云资源/ins-0002",
                "https://ontok.local/ops360#云资源/ins-0003"
            ],
            "节点按 IRI 升序、且 @id 与模板同形"
        );
    }

    #[test]
    fn pushdown_narrows_server_side() {
        // zone 是 pushdown ⇒ 服务端筛（rows_scanned 变小），且不进 filtered_locally。
        let doc = run("push", 12, "\"张三@2026-10-25\"", 5000, r#"{"zone":"ap-guangzhou-3"}"#, 50)
            .expect("应当成功");
        assert_eq!(doc.meta.rows_scanned, 6, "下推后只有 6 行需要读");
        assert!(doc.meta.filtered_locally.is_empty(), "下推不该进 filtered_locally");
        assert_eq!(doc.meta.rows_matched, 6);
        assert_eq!(doc.meta.pages, 2, "6 行 / 每页 5 ⇒ 2 页");
    }

    #[test]
    fn unreviewed_is_rejected() {
        // ⚠️ 不用 `.unwrap_err()`：`JsonLdDoc` 的 `Debug` 会把每个节点全打出来，
        // 红行会被淹掉。变红时说清「它居然成功了、返回了多少行」更有用。
        match run("unreviewed", 12, "null", 5000, "{}", 5) {
            Ok(doc) => panic!(
                "未签的映射居然取到数了：扫 {} 行、返回 {} 行",
                doc.meta.rows_scanned, doc.meta.rows_returned
            ),
            Err(e) => assert_eq!(e.code, CODE_UNREVIEWED, "{e}"),
        }
    }

    #[test]
    fn scan_limit_is_refused_before_fetching() {
        // 12 行 / 每页 5、预算 7：第 2 页（5+5=10 > 7）就该被拒，而不是拉完再判。
        let err = match run("scan", 12, "\"张三@2026-10-25\"", 7, "{}", 12) {
            Ok(doc) => panic!(
                "超预算居然返回了：扫 {} 行 / {} 页、返回 {} 行（预算 7）",
                doc.meta.rows_scanned, doc.meta.pages, doc.meta.rows_returned
            ),
            Err(e) => e,
        };
        assert_eq!(err.code, CODE_SCAN_LIMIT, "{err}");
        assert!(
            err.message.contains("已扫 5 行"),
            "报错要说清读了多少行、还差多少行：{}",
            err.message
        );
    }

    #[test]
    fn unknown_filter_is_an_error() {
        let err = run("unknown", 12, "\"张三@2026-10-25\"", 5000, r#"{"nope":"x"}"#, 5).unwrap_err();
        assert_eq!(err.code, CODE_UNKNOWN_FILTER, "{err}");
    }

    #[test]
    fn capability_classification() {
        let m = parse(&mapping("\"x\"", 5000)).unwrap();
        let e = &m.entities[0];
        assert_eq!(classify_filter(e, "zone"), "pushdown");
        assert_eq!(classify_filter(e, "ids"), "pushdown");
        assert_eq!(classify_filter(e, "status"), "local_only");
        assert_eq!(classify_filter(e, "name"), "undeclared_field");
        assert_eq!(classify_filter(e, "nope"), "unknown");
    }

    #[test]
    fn unsimulated_pushdown_is_reported_not_faked() {
        // `region` 声明为下推，但响应里没有 region 字段 ⇒ 只能说「没模拟」，⛔ 不假装筛过。
        let m = parse(&mapping("\"x\"", 5000).replace(
            r#""pushdown": ["zone", "ids"]"#,
            r#""pushdown": ["zone", "region"]"#,
        ))
        .unwrap();
        let env = Env::new("unsim", 12);
        let o = parse_ontology(TTL);
        let opts = FetchOptions {
            filter: parse_filter(r#"{"region":"ap-guangzhou"}"#).unwrap(),
            limit: 50,
        };
        let doc = dry_run(&m, &o.iri, "cvm_instance", &opts, &env.dir).unwrap();
        assert_eq!(doc.meta.unsimulated_pushdown, vec!["region".to_string()]);
        assert_eq!(doc.meta.rows_scanned, 12, "没有假装筛过 ⇒ 仍然是全量");
        assert!(doc.meta.capability_warnings.iter().any(|w| w.contains("region")));
    }

    #[test]
    fn jsonld_shape_matches_query_serve() {
        let doc = run("shape", 12, "\"张三@2026-10-25\"", 5000, "{}", 2).unwrap();
        let text = doc.to_text();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v.get("@context").is_some(), "必须有 @context");
        assert!(v.get("@graph").is_some(), "@graph 必须是数组（空也是 []）");
        assert!(v.get("meta").is_some());
        let ctx = v["@context"].as_object().unwrap();
        assert_eq!(ctx["ontok"], "https://ontok.local/ops360#");
        assert!(ctx.contains_key("xsd"));
        // meta 五项（AC-008）必须在
        for k in [
            "rows_scanned",
            "filtered_locally",
            "freshness",
            "capability_warnings",
            "truncated",
        ] {
            assert!(v["meta"].get(k).is_some(), "meta 缺 {k}");
        }
        // 值类型：CPU 是 xsd:integer ⇒ 数字，不是字符串
        let node = &v["@graph"][0];
        assert!(node["ontok:云资源__cpu"].is_number(), "{}", node);
    }

    #[test]
    fn missing_key_in_row_is_refused() {
        let env = Env::new("nokey", 3);
        std::fs::write(
            env.dir.join("cvm.json"),
            r#"{"Response":{"TotalCount":2,"InstanceSet":[{"InstanceId":"ins-1"},{"InstanceName":"x"}]}}"#,
        )
        .unwrap();
        let m = parse(&mapping("\"x\"", 5000)).unwrap();
        let o = parse_ontology(TTL);
        let opts = FetchOptions { filter: BTreeMap::new(), limit: 10 };
        let err = dry_run(&m, &o.iri, "cvm_instance", &opts, &env.dir).unwrap_err();
        assert_eq!(err.code, CODE_KEY_MISSING, "{err}");
    }
}
