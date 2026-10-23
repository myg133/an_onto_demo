//! REQ-110 · **Turtle（TTL）序列化器 —— 纯函数，零 I/O，零依赖**。
//!
//! # 一句话
//!
//! 把本平台的两种投影（客户视图 / 建模视图）**确定性地**写成 OWL Turtle。
//! 默认导出客户视图（页面看到什么就导出什么，R1）；`--model-view` 显式选建模视图。
//!
//! # 为什么手写而不是引一个 RDF 库
//!
//! 需求「不做」里写死了：**不引入任何新依赖**（不用 oxrdf / sophia）。
//! 本轮只需要**序列化**（不做解析、不做推理），映射表是规格 §5.5 那九行 ——
//! 引一个 RDF 栈来干这九行，换来的是一个新的供应链面与一次依赖评审。
//!
//! # 确定性（R6 / AC-003）
//!
//! 同一 project + version 的两次导出**逐字节相同**，靠三件事：
//!
//! 1. **排序**：类按 key / 名字字典序，属性按（类, 列名 / 属性名），关系按 key；
//! 2. **不落时间**：`dcterms:created` **刻意不写** —— 它会让两次导出差一个字节。
//!    ⇒ 与外部脚本 `ai-ontology.ttl` 的**唯一**三元组差异就是这一条（已登记）；
//! 3. **不用 HashMap 迭代序**：本文件只用 `BTreeMap` / `sort`。
//!
//! # 有损项必须**显式**声明（R3 / AC-008）
//!
//! 文件头有 `#` 注释块，逐项写清：`ActionType` 未导出（数量）、`semantic` 降级为
//! `rdfs:comment`、`cardinality` 只映射 Functional / InverseFunctional。
//!
//! # ⚠️ AC-002 与 AC-008 的直接冲突（以及本模块的取舍）
//!
//! - AC-008 要求文件头出现 `cardinality` 这个词；
//! - AC-002 要求把序列化结果喂给 `scan_modeling_terms`，命中即拒绝；
//! - 而 `cardinality` **正是**守门词表里的一员（`view::MODELING_TERMS`）。
//!
//! ⇒ 「扫整份文件」与「文件头必须写 cardinality」**不可能同时成立**。
//! 本模块的取舍：**把「文件头」与「数据正文」分开产出**（[`TtlDocument`]），
//! 发布闸只扫 [`TtlDocument::body`]（**全部由数据产生的字节**：IRI、label、
//! comment 字面量、溯源），文件头是**编译期常量**（只随两个计数变化），
//! 由 `tests/test_req110_ttl.rs` 的 `header_declares_the_three_losses` 单独钉住。
//!
//! ⛔ 这不是「扫结构字段」：body 就是最终要写进文件的**那批字节**，只是把
//! **常量头**（与数据无关、且被 AC-008 强行要求含守门词）排除在扫描外。
//! 反过来说：任何**由数据生成**的文本（哪怕它落在注释里）都在扫描范围内。
//!
//! # 与参照物 `code/out/ai-ontology.ttl` 的对应
//!
//! 参照物由外部脚本 `.local/export_ttl.py` 从 `--model-view` 的 JSON 生成
//! （83 类 / 866 数据属性 / 52 对象属性 / 5954 三元组）。本模块的建模视图映射
//! **逐条照它的口径**（含 `ontok:key` / `ontok:column` / `ontok:nullable` /
//! `ontok:cardinality` / `ontok:semantic` / `skos:altLabel` / 属性注释只在
//! `note != name` 时输出），于是**去掉 `dcterms:created` 之后三元组集合同构**。
//!
//! 客户视图的映射**不是**参照物的降级版：客户视图里没有 key / 列名 / 基数记号，
//! 只有人话（对象名、属性名、说明、`belongs_to` 的父对象名、`how_many` 的人话）。
//! 所以客户视图这一侧只导 TBox 骨架：
//!
//! | 客户视图 | Turtle | 说明 |
//! | --- | --- | --- |
//! | `EntityView.name` | `owl:Class` + `rdfs:label` + IRI 本地名 | 名字进 IRI（转义见 [`local_customer`]） |
//! | `EntityView.note` | `rdfs:comment` | 过闸（客户可见文案） |
//! | `EntityView.belongs_to` | `rdfs:subClassOf` | 父对象的**名字** |
//! | `EntityView.aliases` | `skos:altLabel` | 与参照脚本同口径 |
//! | `PropertyView` | `owl:DatatypeProperty` + `rdfs:domain` + `rdfs:range`（xsd） | `nullable = !required` |
//! | `RelationView` | `owl:ObjectProperty` + `rdfs:domain` / `rdfs:range` | 见下方基数映射 |
//! | `RelationView.how_many` | `owl:FunctionalProperty` / `owl:InverseFunctionalProperty` | **N:1 ⇒ Functional；1:N ⇒ InverseFunctional**；1:1 ⇒ 两者（B-1 已修正） |
//! | `RelationView.meaning` | `rdfs:comment` | **降级**（AC-008 已声明有损） |
//! | 样例值（`sample`） | **不导出** | 那是实例面的东西（G1：只导 TBox） |
//!
//! # 已知限制
//!
//! - 建模视图的 IRI 本地名用参照脚本的 `local()`（非 `[0-9A-Za-z_]` 一律换 `_`）：
//!   它**不是单射**（`a-b` 与 `a_b` 会撞）。参照物如此，本轮照抄以保持等价；
//!   客户视图**不用**它（中文名字会被压成一串下划线），另用 [`local_customer`]。
//! - 关系 IRI 是 `from__to`（参照脚本如此）。同一对类型之间若有多条关系，
//!   会撞成同一个 subject —— 本轮数据没有这种情况（已作为已知限制登记）。
//!
//! # REQ-119 · 从「给人看」升级成「给数字员工查」
//!
//! 目标链路（OBDA）：**TTL → Agent 写 SPARQL → 服务端按 R2RML 查库 → Agent 解释**。
//! 本模块补四件让 TTL **可被查询**的东西，两版视图的口径见
//! `BA/demands/REQ-119/design-notes.md`。
//!
//! | 能力 | 客户视图 | 建模视图 | 判据 |
//! | --- | --- | --- | --- |
//! | 反向属性 + 双向 `owl:inverseOf` | ✅ | ✅ | AC-001 |
//! | `rdfs:range` 按 xsd 类型落 | ✅ | ✅ | AC-002 |
//! | `ontok:instanceIri` IRI 模板 | ✅ | ✅ | AC-003 |
//! | 主键列 `ontok:primaryKeyColumn` / `ontok:isPrimaryKey` | ⛔ **无**（无列名） | ✅（有 `--schema` 时） | B-2 |
//! | 值域 `ontok:allowedValue`（从注释解析） | ⛔ **无** | ✅ | B-3 |
//! | 租户标注（`ontok:tenantKey` / `ontok:tenantColumn`） | ⛔ **无** | ✅ | AC-004 / AC-007 |
//!
//! ## 1. 反向属性（R1 / R2 / AC-001）
//!
//! 对象属性 `<主语>__<宾语>` 的反向属性叫 `<宾语>__<主语>`，两向都写
//! `owl:inverseOf`（52 条对象属性 ⇒ 52 条反向属性 + **104** 条 `owl:inverseOf`）。
//! 反向属性的 `rdfs:domain` / `rdfs:range` 与前向**对调**；基数特征也按
//! `1:N ↔ N:1` 对调（`owl:FunctionalProperty ↔ owl:InverseFunctionalProperty`）。
//!
//! **撞名**：如果 `<宾语>__<主语>` 已经是另一条正向属性的 IRI，反向名追加
//! `__inverse`（还撞就 `__inverse2`…）。本版数据里恰好有一对
//! （`order → customer_subscriptions` 与 `customer_subscriptions → order`），
//! 所以那一条反向属性是 `customer_subscriptions__order__inverse`。
//! 为什么不是 `_2`：`_2` 看不出「这是反向」，`__inverse` 一眼能读。
//!
//! **⛔ 反向属性只在本文件生成一次**：命名的唯一入口是 [`inverse_candidate`]，
//! 正/反两个块共用 [`inverse_object_block`]；两版视图只提供各自的标签/附加行。
//!
//! ## 2. xsd range（R3 / AC-002 / A-1）
//!
//! **真实列类型优先**：给 `--schema <ontok-introspect 的 schema.json>` 时，按
//! 「实体 key = 表名 + 属性列名」查 `data_type`，命中就直接用（映射表见
//! [`xsd_for_sql_type`]）；属性写 `ontok:rangeSource "schema"`。
//! 没给 `--schema` / 列没匹配上 / SQL 类型认不出 ⇒ 才走下面的启发式，
//! 写 `ontok:rangeSource "heuristic"`，**文件头 + 本体节点计数 + stderr 三处明说**。
//!
//! ⚠️ **不能只靠启发式**：数字员工看到 `xsd:decimal` 就会写 `SUM()`；实际列若是
//! 字符串，结果要么报错要么是垃圾。所以 `--schema` 模式的验收线是
//! **0 条 heuristic**（本版真实 schema 下 870/870 命中）。
//!
//! 启发式映射表（无 schema 或兜底时）：
//!
//! | 判定（按优先级） | xsd 类型 |
//! | --- | --- |
//! | 布尔：列名 `is_*` / `has_*` / `can_*` / `allow_*` / `*_enabled` / `*_flag` / `active`；中文「是否 / 可见 / 软删 / 允许 / 开关」 | `xsd:boolean` |
//! | 时间：`*_at` / `*_time` / `*_start` / `*_end` / `*_until` / `timestamp`；中文「时间 / 时刻 / 到期 / 过期 / 有效期」 | `xsd:dateTime` |
//! | 日期：`*_date`；中文「日期」 | `xsd:date` |
//! | 整数：列名段 `count` / `num` / `times` / `tokens` / `points` / `qty` / `qpm` / `tpm` / `priority` / `sort` …；中文「次数 / 数量 / 条数 / 积分 / 点数 / 优先级 / 排序」 | `xsd:integer` |
//! | 小数：列名段 `amount` / `price` / `balance` / `cost` / `fee` / `quota` / `limit` / `credit` / `rate` / `ratio` / `total` …；中文「金额 / 余额 / 价格 / 费用 / 用量 / 额度 / 限额 / 比例」 | `xsd:decimal` |
//! | 文本：列名 `*_id` / `*_name` / `*_status` / `*_code` / `*_json` / `*_by` …；中文「名称 / 状态 / 编号 / 类型 / 备注 / 客户 / 用户」 | `xsd:string` |
//! | **认不出** | `xsd:string` + `ontok:rangeUnmapped true`，并计入 [`range_stats`] |
//!
//! ⚠️ **快照层没有 SQL 类型**（`snapshot::Property` 只有 `column` / `nullable` /
//! `note` / `sample`），所以没给 `--schema` 时只能推断 —— 这不是偷偷猜：
//! 文件头写「**启发式**（未读真实列类型）」，每个属性写 `ontok:rangeSource`，
//! 本体节点写 `ontok:rangeSourceSchema` / `ontok:rangeSourceHeuristic` 计数，
//! stderr 再警告一次。认不出的列同样显式标记 + 计数 + 列出。
//!
//! 客户视图**没有列名**（结构上不发布），所以 `--schema` 对它不生效；
//! 文件头会写「客户视图不含列名，--schema 不生效；类型以建模视图为准」。
//!
//! ## 3. IRI 模板（AC-003）
//!
//! 每个类都写一条：
//!
//! ```turtle
//! ontok:instanceIri "https://ontok.local/anspire_grouped#{class}/{id}"
//! ```
//!
//! - `{class}`：类的 `ontok:key`（客户视图没有 key，用类 IRI 的本地名）；
//! - `{id}`：主键列的**值**（R2RML 负责取列 + URL 编码；本模块只给模板）。
//!
//! 模板由 `base + project` 派生，**不含时间 / 随机数** ⇒ 同版本逐字节相同；
//! R2RML 必须产出**同一个** IRI，否则 Agent 写的 SPARQL 查不到实例。
//!
//! ## 4. 租户标注（R4 / R5 / AC-004 / AC-005）
//!
//! `--tenancy <file>` 的 JSON 形状：
//!
//! ```json
//! { "columns": ["customer_id"], "tables": ["customers"], "reviewed_by": "安全负责人" }
//! ```
//!
//! - 命中（列在 `columns`，且省略 `tables` 或实体 key 在 `tables`）⇒
//!   属性 `ontok:tenantKey true`、其类 `ontok:tenantColumn "<列>"`；
//! - **只有 `reviewed_by` 非空（已签）才落标注** —— 未签声明不能当作安全事实；
//! - 没有文件 / 未签 / 声明一个属性都没命中 ⇒ **导出仍成功**，但三处显式标记：
//!   文件头注释、本体节点的 `ontok:tenantUnannotated true`、CLI stderr 警告
//!   「这份本体不能直接给数字员工用」；
//! - **只在建模视图**：客户视图**结构上不读** `TtlOptions::tenancy`，
//!   所以它的正文里连 `tenant` 这个词都不会出现（AC-007）。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView, ModelTrackView};
use crate::domain::view::CustomerView;

/// 默认 IRI 前缀（与参照物 `ai-ontology.ttl` 一致）。
pub const DEFAULT_BASE: &str = "https://ontok.local/";

/// 序列化参数。**不含时间**：时间会让两次导出差一个字节（R6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtlOptions<'a> {
    pub project: &'a str,
    pub version: i32,
    /// IRI 基。尾斜杠会被补齐；`#` 会被去掉（本体 IRI 用 `base + project`，前缀再补 `#`）。
    pub base: &'a str,
    /// 动作层条数 —— 只用于文件头的**有损声明**（`ActionType 未导出：N 个`）。
    /// ⛔ 不导出任何动作内容（OWL 无对应）。
    pub action_type_count: usize,
    /// ⭐ REQ-119 · 租户声明（`--tenancy`）。**只在建模视图生效**：
    /// [`serialize_customer`] 根本不读它 ⇒ 客户视图正文里不会有任何租户标记。
    pub tenancy: Option<&'a TenancyDecl>,
    /// ⭐ REQ-119 · 真实列类型（`--schema <introspect.json>`）。
    ///
    /// 建模视图按「实体 key = 表名 + 属性列名」查真实 `data_type`；命中即以它为准，
    /// 并标 `ontok:rangeSource "schema"`。未命中/未给 schema ⇒ 启发式 + `"heuristic"`。
    /// 客户视图没有列名（结构上不发布），所以这一栏对它不生效（见模块头）。
    pub schema: Option<&'a SchemaTypes>,
}

impl<'a> TtlOptions<'a> {
    /// 用默认 base 与「动作层 0 个」构造（测试与纯函数调用方用）。
    pub fn new(project: &'a str, version: i32) -> Self {
        Self {
            project,
            version,
            base: DEFAULT_BASE,
            action_type_count: 0,
            tenancy: None,
            schema: None,
        }
    }

    /// base 归一化：去掉结尾 `#`，补齐结尾 `/`。
    pub(crate) fn normalized_base(&self) -> String {
        let b = self.base.trim_end_matches('#');
        if b.ends_with('/') {
            b.to_string()
        } else {
            format!("{b}/")
        }
    }

    /// **本体 IRI**：`<归一化 base><项目本地名>`（无尾 `#`）。
    ///
    /// ⛔ REQ-121：R2RML 生成器也读它 —— 两边算出的本体 IRI 必须逐字相同，
    /// 否则 R2RML 产出的实例 IRI 与本体里的实例 IRI 是两套（而且不会报错）。
    ///
    /// REQ-123 起是 `pub`：查询服务（`src/query`）与**现场判据**
    /// （`tests/test_req123_live.rs` 要拼实例 IRI 去写 FILTER）都要它。
    /// 让调用方去 `format!("{base}{project}")` 等于把这条规则抄第二份。
    pub fn ontology_iri(&self) -> String {
        format!("{}{}", self.normalized_base(), local_model(self.project))
    }

    /// **实例 IRI 模板**（`ontok:instanceIri` 的值，AC-003）。
    ///
    /// `{class}` = 类的 key，`{id}` = 主键列的**值**。
    /// 建模视图、客户视图、R2RML 三处**共用**这一个函数（⛔ 不各写一遍）。
    pub(crate) fn instance_iri_template(&self) -> String {
        format!("{}#{{class}}/{{id}}", self.ontology_iri())
    }
}

// ══════════════════════════════════════════════════════════════
// REQ-119 · 真实列类型（`--schema <ontok-introspect JSON>`）
// ══════════════════════════════════════════════════════════════

/// `ontok-introspect` 产出的 schema 报告里，**本模块真正用到的那部分**。
///
/// 收 `tables[].name` / `tables[].columns[].{name,data_type,is_primary_key}` /
/// `tables[].primary_key` / `tables[].composite_primary_key`；其余字段
/// （server_version / 注释 / 外键 …）反序列化时忽略 —— 这样同一份 `--schema`
/// 也可以直接吃 `code/out/schema.json`，不需要另做一份裁剪文件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaTypes {
    /// `(表名, 列名)` → 原始 SQL 类型（都已 trim + 小写）。
    columns: BTreeMap<(String, String), String>,
    /// 表名 → **单列主键列名**（trim + 小写）。复合主键 / 没主键的表不登记。
    primary_keys: BTreeMap<String, String>,
    /// schema 里出现过的**全部表名**（trim + 小写）。REQ-121 用它判 `rr:tableName`。
    tables: BTreeSet<String>,
    /// 表名 → 该表的全部列名（trim + 小写）。REQ-121 用它判 `rr:columnName` / `rr:child` / `rr:parent`。
    table_columns: BTreeMap<String, BTreeSet<String>>,
    /// 表名 → **复合主键**的列（trim + 小写，空 = 不是复合主键）。REQ-121 据此跳过并列入清单。
    composite_primary_keys: BTreeMap<String, Vec<String>>,
    /// 表名 → **唯一列**（含主键；trim + 小写，已排序去重）。
    ///
    /// REQ-121 用它判 `rr:parent` 是不是一个**可以 join 的列**：
    /// 连到一个不唯一的列，一条 SPARQL 会悄悄返回多份结果。
    unique_columns: BTreeMap<String, Vec<String>>,
}

impl SchemaTypes {
    /// 解析 schema JSON。⛔ 解析失败直接报错（fail-closed）：悄悄当成「没给
    /// `--schema`」会退回启发式，而调用方以为用的是真实类型。
    pub fn parse(json: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Doc {
            #[serde(default)]
            tables: Vec<Tab>,
        }
        #[derive(Deserialize)]
        struct Tab {
            name: String,
            #[serde(default)]
            columns: Vec<Col>,
            #[serde(default)]
            primary_key: Option<String>,
            #[serde(default)]
            composite_primary_key: Vec<String>,
            #[serde(default)]
            unique_columns: Vec<String>,
        }
        #[derive(Deserialize)]
        struct Col {
            name: String,
            #[serde(default)]
            data_type: String,
            #[serde(default)]
            is_primary_key: bool,
        }

        let doc: Doc = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut columns = BTreeMap::new();
        let mut primary_keys = BTreeMap::new();
        let mut tables = BTreeSet::new();
        let mut table_columns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut composite_primary_keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut unique_columns: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for t in doc.tables {
            let table = t.name.trim().to_ascii_lowercase();
            if table.is_empty() {
                continue;
            }
            tables.insert(table.clone());
            let mut flagged_pk: Vec<String> = Vec::new();
            for c in t.columns {
                let column = c.name.trim().to_ascii_lowercase();
                if column.is_empty() {
                    continue;
                }
                table_columns.entry(table.clone()).or_default().insert(column.clone());
                let ty = c.data_type.trim().to_ascii_lowercase();
                if !ty.is_empty() {
                    columns.insert((table.clone(), column.clone()), ty);
                }
                if c.is_primary_key {
                    flagged_pk.push(column);
                }
            }
            // 复合主键：记下来（REQ-121 要**跳过并点名**，不能悄悄当成「没有主键」）。
            let composite: Vec<String> = t
                .composite_primary_key
                .iter()
                .map(|c| c.trim().to_ascii_lowercase())
                .filter(|c| !c.is_empty())
                .collect();
            if !composite.is_empty() {
                composite_primary_keys.insert(table.clone(), composite);
            }
            // 唯一列：主键 + 目录里的 `unique_columns`（去重排序）。
            let mut uniq: Vec<String> = t
                .unique_columns
                .iter()
                .map(|c| c.trim().to_ascii_lowercase())
                .filter(|c| !c.is_empty())
                .collect();
            uniq.sort();
            uniq.dedup();
            // 复合主键不导出（AC-119 B-2 只要求单列主键；⛔ 不把多个列拼成一个假列）。
            if t.composite_primary_key.is_empty() {
                let from_field = t
                    .primary_key
                    .as_deref()
                    .map(|k| k.trim().to_ascii_lowercase())
                    .filter(|k| !k.is_empty());
                let pk = from_field
                    .or_else(|| if flagged_pk.len() == 1 { flagged_pk.pop() } else { None });
                if let Some(pk) = pk {
                    if columns.contains_key(&(table.clone(), pk.clone())) {
                        primary_keys.insert(table.clone(), pk.clone());
                        if !uniq.contains(&pk) {
                            uniq.push(pk);
                            uniq.sort();
                        }
                    }
                }
            }
            unique_columns.insert(table, uniq);
        }
        Ok(Self {
            columns,
            primary_keys,
            tables,
            table_columns,
            composite_primary_keys,
            unique_columns,
        })
    }

    /// 查真实 `data_type`（SQL 原始串）。表名 / 列名按 trim + 小写比对。
    pub fn data_type(&self, table: &str, column: &str) -> Option<&str> {
        self.columns
            .get(&(table.trim().to_ascii_lowercase(), column.trim().to_ascii_lowercase()))
            .map(|s| s.as_str())
    }

    /// 查**单列主键**列名（B-2）。复合主键 / 无主键 ⇒ `None`。
    pub fn primary_key(&self, table: &str) -> Option<&str> {
        self.primary_keys.get(&table.trim().to_ascii_lowercase()).map(|s| s.as_str())
    }

    /// ⭐ REQ-121 · schema 里有没有这张表（`rr:tableName` 的判据）。
    pub fn has_table(&self, table: &str) -> bool {
        self.tables.contains(&table.trim().to_ascii_lowercase())
    }

    /// ⭐ REQ-121 · **按名遍历** schema 里的表（已 trim + 小写，BTreeSet ⇒ 有序）。
    ///
    /// 用途：审计「被 `exclusions.json` 排除、因而没有 TriplesMap」的类 ——
    /// 这些类**不在快照里**，只能从 schema 与排除声明两头求差。
    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.iter().map(|s| s.as_str())
    }

    /// ⭐ REQ-121 · 这张表有没有这一列（`rr:columnName` / `rr:child` / `rr:parent` 的判据）。
    pub fn has_column(&self, table: &str, column: &str) -> bool {
        self.table_columns
            .get(&table.trim().to_ascii_lowercase())
            .is_some_and(|cols| cols.contains(&column.trim().to_ascii_lowercase()))
    }

    /// ⭐ REQ-122 · 这张表的**全部列名**（已排序；表不存在 ⇒ `None`）。
    ///
    /// 用途只有一处：**报错时列出可用列**（「声明里写的列在 schema 里不存在」、
    /// 「表里有哪几列可以当租户列」）。⛔ 不用于任何「猜一列」的逻辑。
    pub fn column_names(&self, table: &str) -> Option<Vec<String>> {
        self.table_columns
            .get(&table.trim().to_ascii_lowercase())
            .map(|cols| cols.iter().cloned().collect())
    }

    /// ⭐ REQ-121 · 这张表的**复合主键**列（空 = 不是复合主键）。返回已排序的去重列名。
    pub fn composite_primary_key(&self, table: &str) -> &[String] {
        self.composite_primary_keys
            .get(&table.trim().to_ascii_lowercase())
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// ⭐ REQ-121 · 这一列是不是**主键或唯一列**（`rr:parent` 必须落在这样的列上）。
    ///
    /// 主键天然唯一；`unique_columns` 来自目录（`ontok-introspect` 的 `unique_columns`），
    /// 已含主键列。⛔ 不把「看起来像 id」当唯一 —— 那是猜。
    pub fn is_unique_column(&self, table: &str, column: &str) -> bool {
        let table = table.trim().to_ascii_lowercase();
        let column = column.trim().to_ascii_lowercase();
        if self.primary_keys.get(&table) == Some(&column) {
            return true;
        }
        self.unique_columns.get(&table).is_some_and(|cols| cols.contains(&column))
    }

    /// 已收录的列数（回执用）。
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
}

/// SQL `data_type` → xsd（AC-119 A-1 的映射表）。
///
/// | SQL | xsd |
/// | --- | --- |
/// | `boolean` / `bool` | `xsd:boolean` |
/// | `integer` / `int` / `bigint` / `smallint` / `serial*` | `xsd:integer` |
/// | `numeric` / `decimal` / `real` / `double precision` / `money` | `xsd:decimal` |
/// | `timestamp*` / `timestamptz` / `datetime` | `xsd:dateTime` |
/// | `date` | `xsd:date` |
/// | `character varying` / `varchar` / `text` / `uuid` / `json*` / `bytea` … | `xsd:string` |
/// | 其他 | `None`（退回启发式，并计数/列出） |
///
/// ⚠️ `numeric` **绝不许**归成 integer（`point_change` 这类会丢小数）。
/// `character varying` 即使内容像数字（`version_number = "10"`）也必须是 string：
/// 物理类型就是 text，按字符串排序才是数据库的真实语义。
pub fn xsd_for_sql_type(data_type: &str) -> Option<XsdType> {
    let t = data_type.trim().to_ascii_lowercase();
    // 去掉 `character varying(64)` / `numeric(10,2)` 这类精度括号。
    let base = t.split('(').next().unwrap_or(&t).trim();
    match base {
        "boolean" | "bool" => Some(XsdType::Boolean),
        "integer" | "int" | "int2" | "int4" | "int8" | "smallint" | "bigint" | "serial"
        | "bigserial" | "smallserial" | "serial2" | "serial4" | "serial8" => {
            Some(XsdType::Integer)
        }
        "numeric" | "decimal" | "real" | "double precision" | "float" | "float4" | "float8"
        | "money" => Some(XsdType::Decimal),
        "date" => Some(XsdType::Date),
        "timestamp" | "timestamp without time zone" | "timestamp with time zone"
        | "timestamp without timezone" | "timestamp with timezone" | "timestamptz"
        | "datetime" => Some(XsdType::DateTime),
        "character varying" | "varchar" | "character" | "char" | "bpchar" | "text"
        | "citext" | "name" | "uuid" | "json" | "jsonb" | "bytea" => Some(XsdType::String),
        _ => None,
    }
}

// ══════════════════════════════════════════════════════════════
// REQ-119 B-3 · 值域（从列注释确定性解析，不引模型）
// ══════════════════════════════════════════════════════════════

/// 从列注释里解析**允许值**。解析不出 ⇒ `None`（⛔ 不编）。
///
/// 只认注释里**明写的枚举形态**：
///
/// | 注释形态 | 输出 |
/// | --- | --- |
/// | `状态(0-未开始,1-活动中,2-已结束)` | `0=未开始;1=活动中;2=已结束` |
/// | `状态：1=成功，0=失败` | `0=失败;1=成功` |
/// | `状态 0: 禁用 1: 启用` | `0=禁用;1=启用` |
/// | `充值币种: RMB-人民币, USD-美元, POINT-免费点数` | `POINT=免费点数;RMB=人民币;USD=美元` |
/// | `计费周期：monthly/quarterly/semi_yearly/yearly` | `monthly;quarterly;semi_yearly;yearly` |
/// | `操作结果(success/failed)` | `failed;success` |
///
/// 拒绝：只有一个值、`NULL` / `REQ` 这类伪码、`IPv4/IPv6`（兼容性说明）、
/// `tokens/minute`（单位）、`模型系列(如: GPT, Claude, GLM)`（举例）。
pub fn parse_allowed_values(note: &str) -> Option<String> {
    let normalized: Vec<char> = note
        .chars()
        .map(|c| match c {
            '，' => ',',
            '；' => ';',
            '：' => ':',
            '（' => '(',
            '）' => ')',
            _ => c,
        })
        .collect();
    if normalized.is_empty() {
        return None;
    }
    if let Some(v) = scan_numeric_pairs(&normalized) {
        return Some(v);
    }
    if let Some(v) = scan_code_pairs(&normalized) {
        return Some(v);
    }
    scan_slash_codes(&normalized)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic()
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn read_ident(chars: &[char], mut i: usize) -> (String, usize) {
    let start = i;
    while i < chars.len() && is_ident_char(chars[i]) {
        i += 1;
    }
    (chars[start..i].iter().collect(), i)
}

fn prev_nonspace(chars: &[char], i: usize) -> Option<char> {
    let mut j = i;
    while j > 0 {
        j -= 1;
        if chars[j] != ' ' && chars[j] != '\t' {
            return Some(chars[j]);
        }
    }
    None
}

fn push_pair(
    seen: &mut Vec<(String, String)>,
    code: String,
    label: String,
) -> bool {
    let code_upper = code.to_ascii_uppercase();
    if code_upper == "NULL" || code_upper == "REQ" || label.contains("NULL") || label.starts_with('/')
    {
        return false;
    }
    match seen.iter().find(|(c, _)| *c == code) {
        Some((_, old)) if *old != label => false,
        Some(_) => true,
        None => {
            seen.push((code, label));
            true
        }
    }
}

fn pairs_to_value(pairs: &[(String, String)]) -> Option<String> {
    if pairs.len() < 2 {
        return None;
    }
    Some(pairs.iter().map(|(c, l)| format!("{c}={l}")).collect::<Vec<_>>().join(";"))
}

/// `0-未开始` / `1=成功` / `0: 禁用` / `-1-已删除` 形态。
fn scan_numeric_pairs(chars: &[char]) -> Option<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let start = i;
        let mut j = i;
        if chars[j] == '-' {
            j += 1;
        }
        let digits_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j == digits_start {
            i += 1;
            continue;
        }
        // 左边界：前面不能是字母/数字/下划线（避免把 `x1` 当 code）。
        if start > 0 && (chars[start - 1].is_ascii_alphanumeric() || chars[start - 1] == '_') {
            i = j;
            continue;
        }
        let mut k = j;
        while k < chars.len() && chars[k] == ' ' {
            k += 1;
        }
        if k < chars.len() && matches!(chars[k], ':' | '=' | '-') {
            k += 1;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            let label_start = k;
            while k < chars.len()
                && !matches!(chars[k], ',' | ';' | '(' | ')' | ' ' | '\t' | '\n' | '|')
            {
                k += 1;
            }
            if k > label_start {
                let code: String = chars[start..j].iter().collect();
                let label: String = chars[label_start..k].iter().collect();
                // `1:1` 这类比例不是枚举：label 以数字开头就丢。
                if !label.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && !push_pair(&mut pairs, code, label)
                {
                    return None;
                }
                i = k;
                continue;
            }
        }
        i = j;
    }
    pairs_to_value(&pairs)
}

/// `RMB-人民币` / `true=启用` / `chain=国内` / `rech:成功` 形态。
fn scan_code_pairs(chars: &[char]) -> Option<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if !is_ident_start(chars[i]) || (i > 0 && is_ident_char(chars[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        let (code, mut j) = read_ident(chars, i);
        let mut k = j;
        while k < chars.len() && chars[k] == ' ' {
            k += 1;
        }
        if k < chars.len() && matches!(chars[k], ':' | '=' | '-') {
            k += 1;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            let label_start = k;
            while k < chars.len()
                && !matches!(chars[k], ',' | ';' | '(' | ')' | ' ' | '\t' | '\n' | '|')
            {
                k += 1;
            }
            if k > label_start {
                let label: String = chars[label_start..k].iter().collect();
                if !push_pair(&mut pairs, code, label) {
                    return None;
                }
                i = k;
                continue;
            }
        }
        let _ = start;
        j = j.max(i + 1);
        i = j;
    }
    pairs_to_value(&pairs)
}

/// `monthly/quarterly/...` / `POINT/RMB/USD` / `success/failed` 形态。
///
/// 只认**值域位置**的斜杠串：串首必须是注释开头、`:` 或 `(`；`兼容IPv4/IPv6`
/// （前面是「容」）、`tokens/minute`（前面是「始」）会被挡掉。
fn scan_slash_codes(chars: &[char]) -> Option<String> {
    let mut best: Option<Vec<String>> = None;
    let mut i = 0usize;
    while i < chars.len() {
        if !is_ident_start(chars[i]) || (i > 0 && is_ident_char(chars[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        let (first, mut j) = read_ident(chars, i);
        let mut tokens = vec![first];
        loop {
            let mut k = j;
            while k < chars.len() && chars[k] == ' ' {
                k += 1;
            }
            if k < chars.len() && chars[k] == '/' {
                k += 1;
                while k < chars.len() && chars[k] == ' ' {
                    k += 1;
                }
                if k < chars.len() && is_ident_start(chars[k]) {
                    let (token, next) = read_ident(chars, k);
                    tokens.push(token);
                    j = next;
                    continue;
                }
            }
            break;
        }
        if tokens.len() >= 2 {
            let prev = prev_nonspace(chars, start);
            let in_value_position = matches!(prev, None | Some(':') | Some('('));
            let is_ip_version = tokens.iter().any(|t| t.starts_with("IPv"));
            if in_value_position
                && !is_ip_version
                && best.as_ref().is_none_or(|b| tokens.len() > b.len())
            {
                best = Some(tokens);
            }
        }
        i = j.max(i + 1);
    }
    best.map(|tokens| tokens.join(";"))
}

// ══════════════════════════════════════════════════════════════
// REQ-119 · 租户声明（`--tenancy <file>`）
// ══════════════════════════════════════════════════════════════

/// 租户声明文件的内容。
///
/// ⛔ 这是**声明**，不是证明：`reviewed_by` 为空时它只是一个提案，
/// 序列化器不会落 `ontok:tenantKey`（见 [`TenancyReport::annotated`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenancyDecl {
    /// 租户键列名（如 `customer_id`）。空数组 = 没有声明任何列。
    #[serde(default)]
    pub columns: Vec<String>,
    /// 可选：只在这些实体 key（= 源表名）上认列。省略 = 全部实体。
    #[serde(default)]
    pub tables: Option<Vec<String>>,
    /// 签字的复核人。`null` / 空白 = **未签**（R5：TTL 必须体现未签状态）。
    #[serde(default)]
    pub reviewed_by: Option<String>,
}

impl TenancyDecl {
    /// 解析并归一化（trim / 去重 / 排序）。⛔ 未知字段直接报错（fail-closed）：
    /// 把 `table` 写成 `tables` 的声明若被静默忽略，会得到一份「看着有标注」的本体。
    pub fn parse(json: &str) -> Result<Self, String> {
        let mut d: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        d.columns = d
            .columns
            .into_iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        d.columns.sort();
        d.columns.dedup();
        if let Some(tables) = d.tables.take() {
            let mut t: Vec<String> = tables
                .into_iter()
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
            t.sort();
            t.dedup();
            d.tables = Some(t);
        }
        d.reviewed_by =
            d.reviewed_by.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        Ok(d)
    }

    /// 是否已签（`reviewed_by` 非空）。
    pub fn signed(&self) -> bool {
        self.reviewed_by.as_deref().is_some_and(|r| !r.trim().is_empty())
    }

    /// 一条「实体 + 列」是否命中声明。
    fn matches(&self, entity_key: &str, column: &str) -> bool {
        if self.columns.iter().any(|c| c == column) {
            match &self.tables {
                None => true,
                Some(tables) => tables.iter().any(|t| t == entity_key),
            }
        } else {
            false
        }
    }
}

/// 租户声明的**落点报告**：序列化器与 CLI 共用这一份计算
/// （否则「TTL 里标了」与「stderr 警告没标」会各说各话）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TenancyReport {
    /// 有没有给声明文件。
    pub declared: bool,
    /// `reviewed_by` 非空。
    pub signed: bool,
    /// 签字人（只进 TTL 正文，不进文件头 —— 文件头在发布闸扫描之外）。
    pub reviewed_by: Option<String>,
    /// 实体 key → 命中的租户列（已排序）。
    pub hits: BTreeMap<String, BTreeSet<String>>,
    /// 命中的属性条数（一个属性一条）。
    pub matched_properties: usize,
}

impl TenancyReport {
    /// 只有「已签 + 至少命中一个属性」才算可用的租户标注。
    pub fn annotated(&self) -> bool {
        self.declared && self.signed && !self.hits.is_empty()
    }

    /// 文件头注释行（**不含任何人名**：文件头不在发布闸扫描范围内）。
    pub fn header_line(&self) -> String {
        if !self.declared {
            "租户键：未标注 —— 这份本体不能直接给数字员工用（--tenancy <file>）".to_string()
        } else if !self.signed {
            "租户键：未签（reviewed_by 为空）—— 这份本体不能直接给数字员工用".to_string()
        } else if self.hits.is_empty() {
            "租户键：声明未命中任何属性 —— 这份本体不能直接给数字员工用".to_string()
        } else {
            format!(
                "租户键：已标注（{} 个属性 / {} 个类）",
                self.matched_properties,
                self.hits.len()
            )
        }
    }

    /// stderr 警告（`None` = 已标注，不用警告）。
    pub fn warning(&self) -> Option<String> {
        if self.annotated() {
            return None;
        }
        let why = if !self.declared {
            "未标注租户键"
        } else if !self.signed {
            "租户声明未签（reviewed_by 为空）"
        } else {
            "租户声明未命中任何属性（检查 columns / tables）"
        };
        Some(format!("⚠ {why}：这份本体不能直接给数字员工用"))
    }
}

/// 计算租户落点。**唯一实现**：序列化器与 CLI 都调它。
pub fn tenancy_report(view: &ModelTrackView, decl: Option<&TenancyDecl>) -> TenancyReport {
    let mut report = TenancyReport::default();
    let Some(decl) = decl else {
        return report;
    };
    report.declared = true;
    report.signed = decl.signed();
    report.reviewed_by = decl.reviewed_by.clone();
    if !report.signed {
        return report;
    }
    for e in &view.entity_types {
        for p in &e.properties {
            let column = effective_column(p);
            if decl.matches(&e.key, column) {
                report
                    .hits
                    .entry(e.key.clone())
                    .or_default()
                    .insert(column.to_string());
                report.matched_properties += 1;
            }
        }
    }
    report
}

/// 建模视图里「这个属性对应哪一列」的**唯一**口径（空列名回落到属性名）。
///
/// ⛔ REQ-121：R2RML 生成器也读它 —— 两边对「哪一列」的判断必须逐字相同。
pub(crate) fn effective_column(p: &ModelPropertyView) -> &str {
    if p.column.trim().is_empty() {
        p.name.trim()
    } else {
        p.column.trim()
    }
}

// ══════════════════════════════════════════════════════════════
// REQ-119 · xsd range 推断
// ══════════════════════════════════════════════════════════════

/// 数据类型属性的 range 取值（AC-002）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum XsdType {
    String,
    Boolean,
    Integer,
    Decimal,
    Date,
    DateTime,
}

impl XsdType {
    pub fn iri(self) -> &'static str {
        match self {
            XsdType::String => "xsd:string",
            XsdType::Boolean => "xsd:boolean",
            XsdType::Integer => "xsd:integer",
            XsdType::Decimal => "xsd:decimal",
            XsdType::Date => "xsd:date",
            XsdType::DateTime => "xsd:dateTime",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            XsdType::String => "string",
            XsdType::Boolean => "boolean",
            XsdType::Integer => "integer",
            XsdType::Decimal => "decimal",
            XsdType::Date => "date",
            XsdType::DateTime => "dateTime",
        }
    }
}

/// range 是靠**哪条规则**定出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RangeBasis {
    /// 命中了规则（列/中文名启发式，或真实列类型）。
    Matched,
    /// 一条规则都没命中 ⇒ `xsd:string` 兜底，并且必须计数 + 列出（R3）。
    Unmapped,
}

/// xsd 类型的**来源**（A-1）：真实列类型 / 启发式。
///
/// 每个数据类型属性都写 `ontok:rangeSource "schema"|"heuristic"`，统计口径见
/// [`range_stats`] 与 CLI stderr。⛔ 不许「照旧启发式但不说」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSource {
    /// 来自 `--schema` 的真实 `data_type`。
    Schema,
    /// 没给 schema / schema 没匹配上 / SQL 类型认不出 ⇒ 列名+中文关键词推断。
    Heuristic,
}

impl RangeSource {
    pub fn key(self) -> &'static str {
        match self {
            RangeSource::Schema => "schema",
            RangeSource::Heuristic => "heuristic",
        }
    }
}

/// 定 range 的**唯一入口**：schema 优先 → 列名 → 中文名 → string 兜底。
///
/// ⛔ REQ-121：R2RML 的 `rr:datatype` 也调它 —— 与 TTL 的 `rdfs:range` 必须一致
/// （AC-003 的判据就是「逐条与 TTL 对账」）。
pub(crate) fn xsd_range(
    entity_key: Option<&str>,
    column: Option<&str>,
    schema: Option<&SchemaTypes>,
    name: &str,
    note: Option<&str>,
) -> (XsdType, RangeBasis, RangeSource) {
    // ① 真实列类型（只在建模视图有 entity/column）。
    if let (Some(entity), Some(column), Some(schema)) = (entity_key, column, schema) {
        if let Some(data_type) = schema.data_type(entity, column) {
            if let Some(xsd) = xsd_for_sql_type(data_type) {
                return (xsd, RangeBasis::Matched, RangeSource::Schema);
            }
            // schema 里有这列，但 SQL 类型认不出（自定义 enum / 域类型 …）：
            // 不假装知道 —— 退回启发式，并在回执里由 `heuristic` 计数暴露。
        }
    }
    // ② 列名启发式（建模视图）。
    if let Some(xsd) = column.and_then(classify_column) {
        return (xsd, RangeBasis::Matched, RangeSource::Heuristic);
    }
    // ③ 中文业务名 / 注释启发式（客户视图只能走这条）。
    if let Some(xsd) = classify_text(name, note) {
        return (xsd, RangeBasis::Matched, RangeSource::Heuristic);
    }
    (XsdType::String, RangeBasis::Unmapped, RangeSource::Heuristic)
}

/// 列名规则（见模块头的映射表）。返回 `None` = 认不出。
fn classify_column(column: &str) -> Option<XsdType> {
    let c = column.trim().to_ascii_lowercase();
    if c.is_empty() {
        return None;
    }

    // ① 明确的文本列。放在最前面：`currency` / `timezone` 这类不能被后缀误判。
    if STRING_EXACT.contains(&c.as_str()) {
        return Some(XsdType::String);
    }

    // ② 布尔。
    if BOOL_EXACT.contains(&c.as_str())
        || BOOL_PREFIX.iter().any(|p| c.starts_with(p))
        || BOOL_SUFFIX.iter().any(|s| c.ends_with(s))
    {
        return Some(XsdType::Boolean);
    }

    // ③ 时间 / 日期。
    if c.ends_with("_date") || c == "date" {
        return Some(XsdType::Date);
    }
    if c.contains("timestamp") || DT_SUFFIX.iter().any(|s| c.ends_with(s)) || c == "date_time" {
        return Some(XsdType::DateTime);
    }

    // ④ 明确的文本后缀（`_id` / `_json` / `_by` …）。必须早于数值段匹配：
    //    `charge_detail_json` 不能被 `charge` 判成 decimal，`order_id` 不能被 `order` 判成 int。
    if STRING_SUFFIX.iter().any(|s| c.ends_with(s)) {
        return Some(XsdType::String);
    }

    // ⑤ 数值：按 `_` 切段，整数段优先于小数段（`limit_count` 是次数，不是金额）。
    let segments: Vec<&str> = c.split('_').filter(|s| !s.is_empty()).collect();
    if segments.iter().any(|s| INT_TOKENS.contains(s)) {
        return Some(XsdType::Integer);
    }
    if segments.iter().any(|s| DEC_TOKENS.contains(s)) {
        return Some(XsdType::Decimal);
    }
    None
}

/// 中文业务名 / 注释规则（客户视图没有列名，只能看这一份）。
fn classify_text(name: &str, note: Option<&str>) -> Option<XsdType> {
    let text = format!("{} {}", name.trim(), note.unwrap_or("").trim());
    if text.trim().is_empty() {
        return None;
    }

    if ZH_BOOL.iter().any(|k| text.contains(k)) {
        return Some(XsdType::Boolean);
    }
    if ZH_DATETIME.iter().any(|k| text.contains(k)) {
        return Some(XsdType::DateTime);
    }
    if text.contains("日期") {
        return Some(XsdType::Date);
    }
    if ZH_INTEGER.iter().any(|k| text.contains(k)) {
        return Some(XsdType::Integer);
    }
    if ZH_DECIMAL.iter().any(|k| text.contains(k)) {
        return Some(XsdType::Decimal);
    }
    if ZH_STRING.iter().any(|k| text.contains(k)) {
        return Some(XsdType::String);
    }
    None
}

/// 明确的文本列（整名相等）。
const STRING_EXACT: &[&str] = &[
    "timezone", "weekdays", "currency", "permissions", "services", "options", "extra",
    "keywords", "models", "family", "scene", "tier", "endpoint", "provider", "protocol",
    "replacement", "action", "exception", "version", "name", "title", "code", "status",
    "type", "id", "unit", "email", "phone", "ip", "url", "remark", "description",
    "content", "reason", "alias", "icon", "password", "platform", "region", "source",
    "tag", "mode", "upgrade", "memory_vault",
];

const BOOL_EXACT: &[&str] = &["active"];
const BOOL_PREFIX: &[&str] = &["is_", "has_", "can_", "allow_", "enable_", "auto_", "need_", "should_"];
const BOOL_SUFFIX: &[&str] = &["_enabled", "_flag", "_deleted", "_active", "_visible", "_read"];
const DT_SUFFIX: &[&str] = &["_at", "_time", "_on", "_until", "_start", "_end"];

/// 文本列后缀（`*_id` / `*_json` / `*_by` …）。
const STRING_SUFFIX: &[&str] = &[
    "_id", "_ids", "_key", "_code", "_name", "_title", "_type", "_status", "_url", "_uri",
    "_email", "_phone", "_ip", "_json", "_text", "_desc", "_description", "_remark", "_reason",
    "_message", "_content", "_mode", "_currency", "_unit", "_by", "_source", "_tag", "_tags",
    "_alias", "_icon", "_password", "_agent", "_address", "_region", "_family", "_protocol",
    "_version", "_data", "_config", "_detail", "_details", "_info", "_value", "_token", "_no",
    "_user", "_username", "_cycle", "_fre", "_periods", "_result", "_exception", "_prompt",
    "_messages", "_keywords", "_action", "_replacement", "_options", "_ext", "_inviter",
    "_models", "_whitelist", "_scene", "_prices", "_rules", "_policies", "_metadata",
    "_bindings", "_announcements", "_notices", "_versions", "_image", "_filename",
    "_fingerprint", "_snapshot", "_endpoint", "_provider", "_platform", "_list",
];

/// 整数列名段。
const INT_TOKENS: &[&str] = &[
    "count", "cnt", "num", "nums", "number", "times", "qty", "quantity", "points", "point",
    "tokens", "days", "hours", "minutes", "seconds", "duration", "cycles", "seq", "priority",
    "sort", "rank", "qpm", "tpm", "rpm", "percent",
];

/// 小数列名段。
const DEC_TOKENS: &[&str] = &[
    "amount", "price", "balance", "cost", "fee", "quota", "limit", "credit", "recharge",
    "rebate", "discount", "rate", "ratio", "total", "sum", "used", "usage", "remain",
    "remaining", "debt", "threshold", "weight", "free", "frozen", "transfer", "paid",
    "refund", "charge", "sale",
];

const ZH_BOOL: &[&str] = &["是否", "可见", "软删", "允许", "默认自动", "开关", "已删除", "已禁用", "已启用"];
const ZH_DATETIME: &[&str] = &["时间", "时刻", "到期", "过期", "有效期"];
const ZH_INTEGER: &[&str] = &[
    "次数", "次)", "数量", "条数", "个数", "人数", "天数", "序号", "排名", "积分", "点数",
    "QPM", "TPM", "百分比", "Token数", "优先级", "排序",
];
const ZH_DECIMAL: &[&str] = &[
    "金额", "余额", "价格", "费用", "用量", "额度", "合计", "单价", "总价", "折扣", "比例",
    "权重", "时长", "分钟", "小时", "税率", "汇率", "得分", "上限", "下限", "原价", "费率",
    "限额", "限制", "赠送量", "消耗量", "变化量", "授信", "赠送",
];
const ZH_STRING: &[&str] = &[
    "名称", "姓名", "状态", "编号", "编码", "类型", "备注", "描述", "说明", "地址", "电话",
    "邮箱", "链接", "密码", "账号", "内容", "原因", "标题", "标识", "模式", "货币", "币种",
    "单位", "来源", "标签", "渠道", "地区", "区域", "ID", "id", "值", "配置", "数据", "列表",
    "JSON", "json", "明细", "结果", "客户", "用户", "操作人", "创建人", "修改人", "创建者",
    "编辑者", "更新人", "操作者", "URL", "url", "IP", "ip", "令牌", "密钥", "规则", "选项",
    "场景", "周期", "版本", "所属", "信息", "详情", "消息", "文本", "图标", "属性", "系列",
    "族", "档位", "时区", "频率", "授权", "供应商", "服务商", "端点", "模型", "打包", "升级",
    "搜索", "对接", "库", "指纹", "快照", "参数", "码", "Token", "提示词", "文字", "代表人",
    "异常", "会话", "公钥", "星期", "UA", "code", "权限", "流水号", "动作",
];

// ══════════════════════════════════════════════════════════════
// REQ-119 · 导出后的 range 统计（CLI stderr 用；纯函数）
// ══════════════════════════════════════════════════════════════

/// 一份 TTL 里 `rdfs:range` 的分布 + 未映射清单 + 类型来源。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RangeStats {
    /// `decimal` / `integer` / `dateTime` / `date` / `boolean` / `string` /
    /// `literal` / `other` → 条数。
    pub counts: BTreeMap<String, usize>,
    /// `schema` / `heuristic` → 条数（A-1）。
    pub sources: BTreeMap<String, usize>,
    /// 带 `ontok:rangeUnmapped true` 的属性标签（去重、排序）。
    pub unmapped: Vec<String>,
    /// `ontok:rangeSource "heuristic"` 的属性标签（去重、排序）。
    ///
    /// `--schema` 模式下，这里就是「**schema 没匹配上 / 类型认不出**」的清单；
    /// 没有 schema 时它是全部属性（只用于计数，不在摘要里逐条展开）。
    pub heuristic: Vec<String>,
}

impl RangeStats {
    pub fn get(&self, key: &str) -> usize {
        self.counts.get(key).copied().unwrap_or(0)
    }

    /// 数值列条数（AC-002 的「能不能 SUM」）。
    pub fn numeric(&self) -> usize {
        self.get("decimal") + self.get("integer")
    }

    /// `ontok:rangeSource` 的条数。
    pub fn source(&self, key: &str) -> usize {
        self.sources.get(key).copied().unwrap_or(0)
    }

    /// 一行人类可读摘要（CLI stderr）。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for key in ["decimal", "integer", "dateTime", "date", "boolean"] {
            out.push_str(&format!("{key} {}", self.get(key)));
            out.push_str(" · ");
        }
        out.push_str(&format!(
            "string {}（含未映射 {}） · rdfs:Literal {}",
            self.get("string"),
            self.unmapped.len(),
            self.get("literal")
        ));
        if self.get("other") > 0 {
            out.push_str(&format!(" · other {}", self.get("other")));
        }
        out.push_str(&format!(
            " · rangeSource: schema {} / heuristic {}",
            self.source("schema"),
            self.source("heuristic")
        ));
        if !self.unmapped.is_empty() {
            out.push_str(&format!(" · 未映射清单：{}", self.unmapped.join("、")));
        }
        out
    }
}

/// 把一行 `rdfs:label` 的宾语还原成人读的标签（去掉 `@zh` 与首尾引号）。
fn clean_label(raw: &str) -> String {
    let s = raw.trim_end_matches([' ', ';', '.']);
    let s = s.strip_suffix("@zh").unwrap_or(s);
    let s = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s);
    s.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// 从**导出的 TTL 文本**统计 `rdfs:range` 与未映射项。
///
/// 为什么按文本而不是按视图：客户视图导出前有**属性级剔除**
/// （`pipeline::filter_view`），按视图数会多算被剔掉的属性；按最终字节数
/// 才是「文件里真实存在的东西」。解析对象是本模块自己产出的固定格式。
pub fn range_stats(ttl: &str) -> RangeStats {
    let mut stats = RangeStats::default();
    let mut label: Option<String> = None;
    let mut pending_source: Option<String> = None;
    for line in ttl.lines() {
        let t = line.trim_start();
        // 新块（主语行）⇒ 清空上一个标签 / 待定来源。
        if t.starts_with(':') || t.starts_with('<') {
            label = None;
            pending_source = None;
        }
        if let Some(rest) = t.strip_prefix("rdfs:label ") {
            let l = clean_label(rest);
            if pending_source.as_deref() == Some("heuristic") {
                stats.heuristic.push(l.clone());
            }
            label = Some(l);
            continue;
        }
        if let Some(rest) = t.strip_prefix("rdfs:range ") {
            let value = rest.trim_end_matches([' ', ';', '.']);
            let key = match value {
                "xsd:decimal" => "decimal",
                "xsd:integer" => "integer",
                "xsd:dateTime" => "dateTime",
                "xsd:date" => "date",
                "xsd:boolean" => "boolean",
                "xsd:string" => "string",
                "rdfs:Literal" => "literal",
                v if v.starts_with(':') || v.starts_with('<') => continue, // 对象属性的 range
                _ => "other",
            };
            *stats.counts.entry(key.to_string()).or_insert(0) += 1;
            continue;
        }
        if let Some(rest) = t.strip_prefix("ontok:rangeSource ") {
            let value = rest.trim_end_matches([' ', ';', '.']).trim_matches('"');
            if value == "schema" || value == "heuristic" {
                *stats.sources.entry(value.to_string()).or_insert(0) += 1;
                // 块内顺序两版不同：建模视图是 range→rangeSource→label，
                // 客户视图是 label→…→range→rangeSource。两种顺序都要能收集清单。
                if value == "heuristic" {
                    if let Some(l) = &label {
                        stats.heuristic.push(l.clone());
                    }
                }
                pending_source = Some(value.to_string());
            }
            continue;
        }
        if t.starts_with("ontok:rangeUnmapped ") {
            if let Some(l) = &label {
                stats.unmapped.push(l.clone());
            }
        }
    }
    stats.unmapped.sort();
    stats.unmapped.dedup();
    stats.heuristic.sort();
    stats.heuristic.dedup();
    stats
}

/// 序列化结果：**文件头**（常量 + 两个计数）与**数据正文**分开。
///
/// ⛔ 分开的**唯一**理由写在模块头：发布闸只扫 [`TtlDocument::body`]，
/// 因为文件头按 AC-008 必须出现 `cardinality`（守门词），而数据正文才是
/// 泄漏面。写文件 / 发 HTTP 用的始终是 [`TtlDocument::doc`] 或
/// [`TtlDocument::into_doc`] —— 两者是**同一批字节**（doc = 头 ++ 正文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtlDocument {
    doc: String,
    body: String,
    /// REQ-119 · 正文的 `rdfs:range` 分布 / 未映射清单。
    ///
    /// 由**正文自己**数出来（`range_stats(&body)`），导出层只把它打成 stderr
    /// 回执 —— 出口层因此不需要（也不允许）再碰产物字节。
    stats: RangeStats,
}

impl TtlDocument {
    /// 完整文件（头 ++ 正文）。**写出去的就是它**。
    pub fn doc(&self) -> &str {
        &self.doc
    }

    /// 数据正文（发布闸扫这一份）。
    pub fn body(&self) -> &str {
        &self.body
    }

    /// REQ-119 · range 分布（只读；不进文件字节）。
    pub fn stats(&self) -> &RangeStats {
        &self.stats
    }

    /// 拿走完整文件（只在确实需要 owned `String` 的地方用）。
    pub fn into_doc(self) -> String {
        self.doc
    }

    /// 字节数（回执 / 测试用；不改变内容）。
    pub fn len(&self) -> usize {
        self.doc.len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
    }
}

// ══════════════════════════════════════════════════════════════
// 入口
// ══════════════════════════════════════════════════════════════

/// **客户视图** → Turtle。发布闸要扫 [`TtlDocument::body`]（见模块头）。
///
/// REQ-119：反向属性 / xsd range / IRI 模板在这一轨都有；
/// **租户标注一个字节都没有**（本函数不读 [`TtlOptions::tenancy`]，AC-007）。
pub fn serialize_customer(view: &CustomerView, opts: &TtlOptions<'_>) -> TtlDocument {
    let mut ns = NameSpace::new(opts);

    // ① 先给类分配 IRI 本地名（按名字字典序 ⇒ 分配结果与输入顺序无关）。
    let classes = distinct_customer_classes(view);
    let class_locals: Vec<String> = classes.iter().map(|(name, _)| ns.local_for(name)).collect();
    let iri_template = opts.instance_iri_template();

    // ② 再给属性分配（同一个 NameSpace，跨类别也不撞车）。
    let mut prop_blocks: Vec<String> = Vec::new();
    for (i, (_, c)) in classes.iter().enumerate() {
        let obj_local = class_locals[i].clone();
        for p in distinct_properties(c) {
            let subject_key = format!("{obj_local}__{}", local_customer(p.name));
            let subject = ns.local_for(&subject_key);
            prop_blocks.push(datatype_block(&subject, &obj_local, &p));
        }
    }

    // ③ 关系（按 (from,to,meaning,how_many) 排序）；两端用**已分配**的本地名。
    let relations = distinct_relations(view);
    let mut links: Vec<(&CustomerRelation<'_>, String, String, String)> = Vec::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for r in &relations {
        let from_local = ns.local_for(r.from);
        let to_local = ns.local_for(r.to);
        let raw = format!("{from_local}__{to_local}");
        let subject = ns.local_for(&raw);
        taken.insert(raw);
        links.push((r, from_local, to_local, subject));
    }

    // ④ 反向 IRI：`<宾语>__<主语>`。撞名时的唯一裁定在 [`inverse_candidate`]。
    let mut inverse_locals: Vec<String> = Vec::new();
    for (_, from_local, to_local, _) in &links {
        let raw = inverse_candidate(from_local, to_local, &taken);
        taken.insert(raw.clone());
        inverse_locals.push(ns.local_for(&raw));
    }

    let mut rel_blocks: Vec<String> = Vec::new();
    let mut inverse_blocks: Vec<(String, String)> = Vec::new();
    for (i, (r, from_local, to_local, subject)) in links.iter().enumerate() {
        let inv = &inverse_locals[i];
        rel_blocks.push(object_block_customer(
            from_local,
            to_local,
            subject,
            r,
            Some(inv.as_str()),
        ));
        let features = cardinality_features(r.how_many).inverse();
        let label = format!("{} → {}（反向）", r.to, r.from);
        let comment = format!(
            "反向属性：前向为「{} → {}」（{} → {}）",
            r.from,
            r.to,
            r.how_many,
            inverse_how_many(r.how_many)
        );
        inverse_blocks.push((
            inv.clone(),
            inverse_object_block(
                inv,
                subject,
                to_local,
                from_local,
                &label,
                Some(&comment),
                features,
                &[],
            ),
        ));
    }
    inverse_blocks.sort_by(|a, b| a.0.cmp(&b.0));

    // ⑤ 类块（父对象名同样用已分配的本地名：父/子同名必然同 IRI）。
    let mut class_blocks: Vec<String> = Vec::new();
    for (i, (name, c)) in classes.iter().enumerate() {
        let parents: Vec<String> = c.parents.iter().map(|p| ns.local_for(p)).collect();
        class_blocks.push(customer_class_block(
            &class_locals[i],
            name,
            &parents,
            c,
            &iri_template,
        ));
    }

    let stats = range_stats(&prop_blocks.join("\n"));
    let mode = if opts.schema.is_some() {
        TypeSourceMode::CustomerNoColumns
    } else {
        TypeSourceMode::HeuristicOnly
    };
    let header = ontology_header(ns.ontology(), opts, CUSTOMER_ONTOLOGY_NOTE, None, None, &stats);
    let header_lines = vec![mode.header_line(&stats)];
    finish(
        opts,
        Body {
            ontology: header,
            classes: class_blocks,
            props: prop_blocks,
            rels: rel_blocks,
            inverses: inverse_blocks.into_iter().map(|(_, b)| b).collect(),
        },
        &header_lines,
        stats,
    )
}

/// **建模视图** → Turtle。建模术语在这一轨是**要被裁决的内容**，故不过发布闸
/// （轨道由 `pipeline::spec_of(OutletId::D9)` 显式声明，不是「没调闸所以豁免」）。
///
/// REQ-119：反向属性 / xsd range / IRI 模板 / **租户标注**（唯一会落标注的一轨）。
pub fn serialize_model(view: &ModelTrackView, opts: &TtlOptions<'_>) -> TtlDocument {
    let ns = NameSpace::new(opts);
    let iri_template = opts.instance_iri_template();
    let tenancy = tenancy_report(view, opts.tenancy);

    let mut entities: Vec<&ModelEntityView> = view.entity_types.iter().collect();
    entities.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.display_name.cmp(&b.display_name)));

    let mut class_blocks: Vec<String> = Vec::new();
    let mut prop_blocks: Vec<String> = Vec::new();
    for e in &entities {
        let class_local = local_model(&e.key);
        let tenant_columns: Vec<String> = if tenancy.annotated() {
            tenancy
                .hits
                .get(&e.key)
                .map(|cols| cols.iter().cloned().collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let pk: Option<String> =
            opts.schema.and_then(|s| s.primary_key(&e.key)).map(str::to_string);
        class_blocks.push(model_class_block(
            &class_local,
            e,
            &iri_template,
            &tenant_columns,
            pk.as_deref(),
        ));
        let mut props: Vec<&ModelPropertyView> = e.properties.iter().collect();
        props.sort_by(|a, b| a.column.cmp(&b.column).then_with(|| a.name.cmp(&b.name)));
        let mut emitted_pk = false;
        for p in props {
            let tenant_key = tenancy.annotated()
                && tenancy
                    .hits
                    .get(&e.key)
                    .is_some_and(|cols| cols.contains(effective_column(p)));
            let is_pk = pk.as_deref() == Some(effective_column(p));
            emitted_pk |= is_pk;
            prop_blocks.push(model_datatype_block(
                &class_local,
                p,
                &e.key,
                tenant_key,
                is_pk,
                opts.schema,
            ));
        }
        // B-2：主键列通常被抽取侧从 `properties` 里剥掉了（`licenses` 没有 `id`）。
        // 有 schema 的真实主键、且属性清单里没有 ⇒ **补一条**属性块，否则 FK 串无法还原成 IRI。
        if let Some(pk) = pk.as_deref() {
            if !emitted_pk {
                prop_blocks.push(model_pk_block(&class_local, &e.key, pk, opts.schema));
            }
        }
    }

    let mut links: Vec<&ModelLinkView> = view.link_types.iter().collect();
    links.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.from_type.cmp(&b.from_type))
            .then_with(|| a.to_type.cmp(&b.to_type))
    });

    // ⭐ REQ-120 · 关系属性的本地名（含「同一对多边时带列名」的裁定）。
    // ⛔ 唯一实现抽在 [`model_link_locals`]：REQ-121 的 R2RML 生成器也调它，
    // 于是 `rr:predicate` 与 TTL 的属性 IRI **逐字相同**（抄一份就会漂移）。
    let (forward_locals, inverse_locals) = model_link_locals(&links);

    let mut rel_blocks: Vec<String> = Vec::new();
    let mut inverse_blocks: Vec<(String, String)> = Vec::new();
    for (i, l) in links.iter().enumerate() {
        let from_local = local_model(&l.from_type);
        let to_local = local_model(&l.to_type);
        let subject = &forward_locals[i];
        let inv = &inverse_locals[i];
        rel_blocks.push(model_object_block(l, subject, Some(inv)));
        let features = model_cardinality(&l.cardinality).inverse();
        let label = format!("{} → {}（反向）", l.to_type, l.from_type);
        let comment =
            format!("反向属性：前向为 {} → {}（domain/range 已对调）", l.from_type, l.to_type);
        let mut extra = vec![format!(
            "ontok:cardinality {}",
            plain(&inverse_cardinality(&l.cardinality))
        )];
        // REQ-120 · 反向属性**也**标连接验证。
        //
        // 为什么不只标正向：数字员工问「这个客户用过哪些 API」走的是
        // **反向**边（`api_call_logs → licenses` 的反向）。只在正向标，
        // Agent 走到反向时看到的又是一条「不知道可不可信」的边，
        // AC-011 那条两跳查询就会重新变成猜。
        //
        // ⚠️ 代价如实登记：TTL 里 `ontok:joinVerified` 的**条数 = 2 ×
        // 签过的边数**（正反各一条）。报告里两个数都写。
        if let Some(v) = l.join_verified {
            extra.push(format!("ontok:joinVerified {}", plain(v.as_str())));
            // F-2：**反向也要写为什么没验**。
            // 数字员工问「这个客户扣了哪些钱」走的是反向边（`customers__deductions`）；
            // 只在正向写理由，反向看到的又是一条没有解释的 `false`。
            if let Some(e) = l.join_evidence.as_deref().filter(|e| !e.trim().is_empty()) {
                extra.push(format!("ontok:joinEvidence {}", plain(e)));
            }
        }
        inverse_blocks.push((
            inv.clone(),
            inverse_object_block(
                inv,
                subject,
                &to_local,
                &from_local,
                &label,
                Some(&comment),
                features,
                &extra,
            ),
        ));
    }
    inverse_blocks.sort_by(|a, b| a.0.cmp(&b.0));

    let stats = range_stats(&prop_blocks.join("\n"));
    let mode = if opts.schema.is_some() {
        TypeSourceMode::SchemaPreferred
    } else {
        TypeSourceMode::HeuristicOnly
    };
    let header_lines = vec![
        mode.header_line(&stats),
        tenancy.header_line(),
        // ⭐ REQ-120 · F-4：**证据强度**必须写进文件头。
        //
        // 数字员工看到 `ontok:joinVerified "true"` 很容易理解成「这条关系在全量数据上
        // 成立」。它不是：取证方式是「每列取 ≤5 个不同值，去撞目标键」，命中率 5/5
        // 只说明**抽样没翻车**，不说明「全表都成立」。`inherit` 更弱 —— 本表根本没测。
        JOIN_EVIDENCE_HEADER.to_string(),
    ];
    finish(
        opts,
        Body {
            ontology: ontology_header(
                ns.ontology(),
                opts,
                &model_ontology_note(opts.action_type_count),
                view.extracted_by.as_ref().map(|e| (e.agent.as_str(), e.version.as_str(), e.round)),
                Some(&tenancy),
                &stats,
            ),
            classes: class_blocks,
            props: prop_blocks,
            rels: rel_blocks,
            inverses: inverse_blocks.into_iter().map(|(_, b)| b).collect(),
        },
        &header_lines,
        stats,
    )
}

// ══════════════════════════════════════════════════════════════
// 组装
// ══════════════════════════════════════════════════════════════

/// 固定前缀表（**顺序固定**，R6）。声明哪些前缀不影响三元组；这里把参照物
/// 出现过的那几个都声明上，另加 `skos`（别名要用）。
const PREFIX_TABLE: &[(&str, &str)] = &[
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("prov", "http://www.w3.org/ns/prov#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
];

/// **REQ-120 · F-4**：建模视图文件头对 `ontok:joinVerified` 的**强度声明**。
///
/// 只进建模视图（客户视图没有这三态，也不该出现建模术语）。
/// 措辞刻意说「抽样」与「不构成全量保证」，因为这就是事实：
/// 取证是每列 ≤5 个不同值去撞目标键（见 `extract::fk` 模块头）。
const JOIN_EVIDENCE_HEADER: &str = "ontok:joinVerified 是**抽样证据**（每列取 ≤5 个不同值比对目标键），\
不是全量统计保证：\"true\" = 抽样命中率 ≥95%（常见 5/5），\"inherit\" = 同为名列在别处的验证、\
本表未实测，\"false\" = 仅列名推断（joinEvidence 写明是哪一种没验）；\
两种方向（正向/反向属性）各标一条";

/// 客户视图本体节点上的说明（**不含守门词**：它是正文，要过闸）。
const CUSTOMER_ONTOLOGY_NOTE: &str =
    "本导出为 OWL 本体，只描述世界由什么构成，不描述能对它做什么。动作层未导出，见文件头有损声明。";

/// 建模视图本体节点上的说明（与参照脚本逐字一致，只替换动作层条数）。
fn model_ontology_note(action_types: usize) -> String {
    format!(
        "本导出为 OWL 本体，只描述「世界由什么构成」，不描述「能对它做什么」。\
         源模型中另有 {action_types} 个 ActionType（写回、副作用、权限、审计），\
         OWL/SHACL/SWRL 均无干净对应，故未导出。见规格 §5.5。"
    )
}

/// IRI / 本地名的分配器。
struct NameSpace {
    /// `https://ontok.local/anspire_ai`
    ontology: String,
    /// 名字 → 已分配的本地名。
    assigned: BTreeMap<String, String>,
    /// 已经用过的本地名（防撞车）。
    used: BTreeSet<String>,
}

impl NameSpace {
    fn new(opts: &TtlOptions<'_>) -> Self {
        Self {
            // 本体 IRI 的唯一来源（与 R2RML 生成器共用）—— 本文件不再自己拼一遍。
            ontology: opts.ontology_iri(),
            assigned: BTreeMap::new(),
            used: BTreeSet::new(),
        }
    }

    fn ontology(&self) -> &str {
        &self.ontology
    }

    /// 名字 → 本地名。**同一个名字永远得到同一个本地名**；
    /// 不同的名字若被转义成同一个形状（`a-b` 与 `a_b`），第二个起追加 `_2`、`_3`…
    ///
    /// ⚠️ 分配顺序由**调用方的排序**决定 ⇒ 「同一份输入 ⇒ 逐字节相同」
    /// 不依赖任何哈希序。
    fn local_for(&mut self, name: &str) -> String {
        if let Some(v) = self.assigned.get(name) {
            return v.clone();
        }
        let base = local_customer(name);
        let mut candidate = base.clone();
        let mut n = 1usize;
        while self.used.contains(&candidate) {
            n += 1;
            candidate = format!("{base}_{n}");
        }
        self.used.insert(candidate.clone());
        self.assigned.insert(name.to_string(), candidate.clone());
        candidate
    }
}

struct Body {
    ontology: String,
    classes: Vec<String>,
    props: Vec<String>,
    rels: Vec<String>,
    /// REQ-119 · 反向对象属性（按 IRI 排序，R6）。
    inverses: Vec<String>,
}

/// xsd 类型来源的三种出口口径（决定文件头怎么写）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeSourceMode {
    /// 没给 `--schema`。
    HeuristicOnly,
    /// 给了 `--schema`，建模视图按真实列类型优先。
    SchemaPreferred,
    /// 给了 `--schema`，但客户视图没有列名可匹配。
    CustomerNoColumns,
}

impl TypeSourceMode {
    /// 文件头里的「类型来源」一行（R-1：不让启发式静默冒充真实类型）。
    fn header_line(self, stats: &RangeStats) -> String {
        let schema = stats.source("schema");
        let heuristic = stats.source("heuristic");
        match self {
            TypeSourceMode::HeuristicOnly => format!(
                "xsd 类型来源：**启发式**（未读真实列类型；--schema <schema.json> 可消除）\
                 —— schema {schema} / heuristic {heuristic}"
            ),
            TypeSourceMode::SchemaPreferred => format!(
                "xsd 类型来源：真实列类型（--schema 优先；未匹配/类型认不出回退启发式）\
                 —— schema {schema} / heuristic {heuristic}"
            ),
            TypeSourceMode::CustomerNoColumns => format!(
                "xsd 类型来源：启发式（客户视图不含列名，--schema 不生效；类型以建模视图为准）\
                 —— schema {schema} / heuristic {heuristic}"
            ),
        }
    }
}

fn ontology_header(
    ontology: &str,
    opts: &TtlOptions<'_>,
    note: &str,
    extracted_by: Option<(&str, &str, i32)>,
    tenancy: Option<&TenancyReport>,
    stats: &RangeStats,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("<{ontology}> a owl:Ontology ;\n"));
    out.push_str(&format!(
        "    rdfs:label {} ;\n",
        zh(&format!("{} 本体（v{}）", opts.project, opts.version))
    ));
    out.push_str(&format!("    rdfs:comment {} ;\n", zh(note)));
    if let Some((agent, version, round)) = extracted_by {
        // 参照脚本：`prov:wasGeneratedBy [ a prov:Activity ; prov:wasAssociatedWith "agent" ;
        // ontok:agentVersion "v" ; ontok:round N ]`
        out.push_str("    prov:wasGeneratedBy [ a prov:Activity ;\n");
        out.push_str(&format!("            prov:wasAssociatedWith {} ;\n", plain(agent)));
        out.push_str(&format!("            ontok:agentVersion {} ;\n", plain(version)));
        out.push_str(&format!("            ontok:round {round} ] ;\n"));
    }
    out.push_str(&format!("    ontok:sourceProject {} ;\n", plain(opts.project)));

    // REQ-119 · 类型来源计数（A-1：`rangeSource` 的机器可读口径）。
    let mut marks: Vec<String> = vec![
        format!("ontok:rangeSourceSchema {}", stats.source("schema")),
        format!("ontok:rangeSourceHeuristic {}", stats.source("heuristic")),
    ];

    // REQ-119 · 租户状态标记（只在建模视图传入；客户视图传 None）。
    if let Some(t) = tenancy {
        if t.signed {
            if let Some(name) = &t.reviewed_by {
                marks.push(format!("ontok:tenancyReviewedBy {}", plain(name)));
            }
        } else if t.declared {
            marks.push("ontok:tenancyReviewed false".to_string());
        }
        if !t.annotated() {
            marks.push("ontok:tenantUnannotated true".to_string());
        }
    }

    out.push_str(&format!("    ontok:version {}", opts.version));
    for mark in &marks {
        out.push_str(" ;\n");
        out.push_str(&format!("    {mark}"));
    }
    out.push_str(" .\n");
    out
}

fn finish(
    opts: &TtlOptions<'_>,
    b: Body,
    header_lines: &[String],
    stats: RangeStats,
) -> TtlDocument {
    // 本体 IRI 与 `NameSpace::new` 同源（A-7：头部默认前缀 == 正文本体 IRI）。
    let ontology = opts.ontology_iri();

    // 头：固定前缀 + 有损声明（AC-008）。
    let mut doc = String::new();
    doc.push_str(&format!("@prefix : <{ontology}#> .\n"));
    doc.push_str("@prefix dcterms: <http://purl.org/dc/terms/> .\n");
    doc.push_str("@prefix ontok: <https://ontok.local/schema/> .\n");
    for (p, iri) in PREFIX_TABLE {
        doc.push_str(&format!("@prefix {p}: <{iri}> .\n"));
    }
    doc.push('\n');
    doc.push_str(&lossy_header(opts, header_lines));
    doc.push('\n');

    let mut body = String::new();
    body.push_str(&b.ontology);
    for group in [&b.classes, &b.props, &b.rels, &b.inverses] {
        if group.is_empty() {
            continue;
        }
        body.push('\n');
        body.push_str(&group.join("\n"));
    }
    doc.push_str(&body);
    TtlDocument { doc, body, stats }
}

/// 文件头的**有损声明**（AC-008：三个关键词一个都不能少）。
///
/// ⛔ 它是**编译期常量 + 计数 + 类型来源/租户状态**：不含任何项目数据
/// （尤其不写 `reviewed_by` 人名），所以把它排除在发布闸的扫描之外不会
/// 放过任何数据字节（理由见模块头）。
fn lossy_header(opts: &TtlOptions<'_>, header_lines: &[String]) -> String {
    let mut out = format!(
        "# ── 有损声明：本文件是 OWL 本体的降级表达，逐项如下 ──\n\
         # · ActionType：未导出（{} 个）—— OWL / SHACL / SWRL 均无干净对应（规格 §5.5）\n\
         # · semantic：降级为 rdfs:comment（语义只能作注释，不可推理）\n\
         # · cardinality：只映射 Functional / InverseFunctional（1:1 / 1:N / N:1；多对多无对应）\n\
         # · 不含实例数据（ABox）；不含 SHACL 形状（nullable / 必填不导出）\n",
        opts.action_type_count
    );
    for line in header_lines {
        out.push_str(&format!("# · {line}\n"));
    }
    out
}

// ══════════════════════════════════════════════════════════════
// 客户视图：块
// ══════════════════════════════════════════════════════════════

/// 客户视图里的一个类（同名对象**合并**：客户视图用名字当标识，同名就是同一个东西）。
struct CustomerClass<'a> {
    notes: Vec<&'a str>,
    parents: Vec<&'a str>,
    aliases: Vec<&'a str>,
    properties: Vec<CustomerProp<'a>>,
}

struct CustomerProp<'a> {
    name: &'a str,
    note: Option<&'a str>,
    required: bool,
}

/// 稳定的类清单：**按名字字典序**（R6），同名对象合并。
fn distinct_customer_classes(view: &CustomerView) -> Vec<(String, CustomerClass<'_>)> {
    let mut map: BTreeMap<&str, CustomerClass<'_>> = BTreeMap::new();
    for o in &view.objects {
        let e = map.entry(o.name.as_str()).or_insert_with(|| CustomerClass {
            notes: Vec::new(),
            parents: Vec::new(),
            aliases: Vec::new(),
            properties: Vec::new(),
        });
        if let Some(n) = o.note.as_deref().filter(|n| !n.trim().is_empty()) {
            e.notes.push(n);
        }
        if let Some(p) = o.belongs_to.as_deref().filter(|p| !p.trim().is_empty()) {
            e.parents.push(p);
        }
        for a in &o.aliases {
            if !a.trim().is_empty() {
                e.aliases.push(a.as_str());
            }
        }
        for p in &o.properties {
            e.properties.push(CustomerProp {
                name: p.name.as_str(),
                note: p.note.as_deref().filter(|n| !n.trim().is_empty()),
                required: p.required,
            });
        }
    }
    let mut out: Vec<(String, CustomerClass<'_>)> = Vec::with_capacity(map.len());
    for (name, mut c) in map {
        // 去重 + 排序：输出只取决于**集合**，不取决于输入顺序。
        c.notes.sort_unstable();
        c.notes.dedup();
        c.parents.sort_unstable();
        c.parents.dedup();
        c.aliases.sort_unstable();
        c.aliases.dedup();
        out.push((name.to_string(), c));
    }
    out
}

fn distinct_properties<'a>(c: &'a CustomerClass<'a>) -> Vec<CustomerProp<'a>> {
    let mut map: BTreeMap<&str, CustomerProp<'_>> = BTreeMap::new();
    for p in &c.properties {
        // 同名属性合并：`required` 取「任一必填」（保守：不把必填说成可选）；
        // `note` 取字典序最小的那条 —— 让结果只取决于集合。
        let e = map.entry(p.name).or_insert(CustomerProp {
            name: p.name,
            note: p.note,
            required: p.required,
        });
        e.required = e.required || p.required;
        if let Some(n) = p.note {
            e.note = Some(match e.note {
                Some(cur) if cur <= n => cur,
                _ => n,
            });
        }
    }
    map.into_values().collect()
}

fn customer_class_block(
    local: &str,
    name: &str,
    parent_locals: &[String],
    c: &CustomerClass<'_>,
    iri_template: &str,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:Class".to_string());
    // `rdfs:label` 用**名字**，不是本地名 —— 客户看的是标签，不是 IRI。
    lines.push(format!("rdfs:label {}", zh(name)));
    // REQ-119 · 实例 IRI 模板（AC-003）：每个类都有，不含时间/随机。
    lines.push(format!("ontok:instanceIri {}", plain(iri_template)));
    for n in &c.notes {
        lines.push(format!("rdfs:comment {}", zh(n)));
    }
    for p in parent_locals {
        lines.push(format!("rdfs:subClassOf :{p}"));
    }
    for a in &c.aliases {
        lines.push(format!("skos:altLabel {}", zh(a)));
    }
    block(&format!(":{local}"), lines)
}

struct CustomerRelation<'a> {
    from: &'a str,
    to: &'a str,
    how_many: &'a str,
    meaning: &'a str,
}

/// 稳定的关系清单：按（from, to, meaning, how_many）字典序；完全相同的关系只留一条。
fn distinct_relations(view: &CustomerView) -> Vec<CustomerRelation<'_>> {
    let mut set: BTreeSet<(&str, &str, &str, &str)> = BTreeSet::new();
    for r in &view.relations {
        set.insert((r.from.as_str(), r.to.as_str(), r.meaning.as_str(), r.how_many.as_str()));
    }
    set.into_iter()
        .map(|(from, to, meaning, how_many)| CustomerRelation { from, to, how_many, meaning })
        .collect()
}

fn object_block_customer(
    from_local: &str,
    to_local: &str,
    subject_local: &str,
    r: &CustomerRelation<'_>,
    inverse_of: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(object_type_line(cardinality_features(r.how_many)).to_string());
    lines.push(format!("rdfs:label {}", zh(&format!("{} → {}", r.from, r.to))));
    lines.push(format!(
        "rdfs:comment {}",
        zh(&format!("语义：{}（{}）", r.meaning, r.how_many))
    ));
    lines.push(format!("rdfs:domain :{from_local}"));
    lines.push(format!("rdfs:range :{to_local}"));
    if let Some(inv) = inverse_of {
        lines.push(format!("owl:inverseOf :{inv}"));
    }
    block(&format!(":{subject_local}"), lines)
}

/// 客户视图里的基数**只有人话**（`how_many` 已被 `view::cardinality_cn` 翻译过）。
/// 按规格 §5.5 的表反推 OWL 特征；认不出的（含未翻译的记号）**不给特征** ——
/// 未翻译记号本身会被发布闸的第三档（形态判定）抓住。
enum Cardinality {
    Functional,
    InverseFunctional,
    Both,
    None,
}

impl Cardinality {
    /// REQ-119 · 反向属性的基数特征：`1:N ↔ N:1`（Functional ↔ InverseFunctional），
    /// `1:1` / 多对多不变。
    fn inverse(self) -> Self {
        match self {
            Cardinality::Functional => Cardinality::InverseFunctional,
            Cardinality::InverseFunctional => Cardinality::Functional,
            other => other,
        }
    }
}

fn cardinality_features(how_many: &str) -> Cardinality {
    match how_many {
        // REQ-119 B-1（修正）：N:1（多对一）= 每个 subject 至多一个 object ⇒ Functional；
        // 1:N（一对多）= 每个 object 至多一个 subject ⇒ InverseFunctional。
        // 之前整体反了：开推理会把一个套餐下的所有订阅合并成一条。
        "多对一" => Cardinality::Functional,        // N:1
        "一对多" => Cardinality::InverseFunctional, // 1:N
        "唯一对应" => Cardinality::Both,            // 1:1
        _ => Cardinality::None,                     // 多对多 / 认不出
    }
}

/// 建模视图的 `1:1` / `1:N` / `N:1` / 其他 → 同一套基数枚举。
fn model_cardinality(cardinality: &str) -> Cardinality {
    match cardinality {
        "1:1" => Cardinality::Both,
        // B-1：N:1 ⇒ Functional；1:N ⇒ InverseFunctional（与 `cardinality_features` 同口径）。
        "N:1" => Cardinality::Functional,
        "1:N" => Cardinality::InverseFunctional,
        _ => Cardinality::None,
    }
}

/// 基数的 OWL 特征行（正/反两个方向共用，⛔ 不许各写一份）。
fn object_type_line(c: Cardinality) -> &'static str {
    match c {
        Cardinality::Both => {
            "a owl:FunctionalProperty, owl:InverseFunctionalProperty, owl:ObjectProperty"
        }
        Cardinality::Functional => "a owl:FunctionalProperty, owl:ObjectProperty",
        Cardinality::InverseFunctional => "a owl:InverseFunctionalProperty, owl:ObjectProperty",
        Cardinality::None => "a owl:ObjectProperty",
    }
}

fn inverse_how_many(how_many: &str) -> &str {
    match how_many {
        "一对多" => "多对一",
        "多对一" => "一对多",
        other => other,
    }
}

fn inverse_cardinality(cardinality: &str) -> String {
    match cardinality {
        "1:N" => "N:1".to_string(),
        "N:1" => "1:N".to_string(),
        other => other.to_string(),
    }
}

/// ⭐ REQ-121 · **关系属性本地名的唯一实现**（前向 + 反向），供 TTL 与 R2RML 共用。
///
/// `links` 必须是**已排序**的（TTL 按 `key` / `from` / `to`；R2RML 用同一口径），
/// 因为「同一对 (from, to) 有多条边」的歧义判定是**集合性**的，
/// 而名字分配依赖遍历顺序 —— 排序保证两次运行逐字节相同。
///
/// # 前向名（REQ-120 的裁定，逐字保留）
///
/// 同一对 `(from, to)` 出现**多次**时，本地名追加列名（`{from}__{to}__{col}`）；
/// 只出现一次的那 71 对仍是 `{from}__{to}`（REQ-119 的契约一个字节不动）。
///
/// # 反向名
///
/// 有歧义的那一对也带列名（`{to}__{from}__{col}`），否则会退化成
/// `...__inverse` / `...__inverse2` —— 名字里看不出是哪一列。
///
/// # 为什么抽成公共函数
///
/// R2RML 的 `rr:predicate` 必须是 TTL 里那条属性的**同一个 IRI**。
/// 抄一份算法 = 两份命名规则，漂移一次就是「SPARQL 查不到本体里的实例」，
/// 而且**不会报错**。所以这里的一份实现是唯一的（纪律 H）。
pub fn model_link_locals(links: &[&ModelLinkView]) -> (Vec<String>, Vec<String>) {
    let mut pair_count: BTreeMap<(String, String), usize> = BTreeMap::new();
    for l in links {
        *pair_count.entry((l.from_type.clone(), l.to_type.clone())).or_insert(0) += 1;
    }

    let mut forward: Vec<(String, String, Option<String>)> = Vec::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for l in links {
        let from_local = local_model(&l.from_type);
        let to_local = local_model(&l.to_type);
        let ambiguous = pair_count
            .get(&(l.from_type.clone(), l.to_type.clone()))
            .copied()
            .unwrap_or(0)
            > 1;
        let col = ambiguous.then(|| {
            local_model(
                l.column
                    .as_deref()
                    .unwrap_or(l.key.rsplit("__").next().unwrap_or("column")),
            )
        });
        let raw = match &col {
            Some(c) => format!("{from_local}__{to_local}__{c}"),
            None => format!("{from_local}__{to_local}"),
        };
        // 极端情形（同一列名出现两次）也要唯一：`taken` 在这里兜底。
        let raw = unique_local(raw, &mut taken);
        taken.insert(raw);
        forward.push((from_local, to_local, col));
    }
    let mut inverse_locals: Vec<String> = Vec::new();
    for (from_local, to_local, col) in &forward {
        // 有歧义的那一对：反向名也带列名（`{to}__{from}__{col}`），
        // 否则会退化成 `...__inverse` / `...__inverse2` —— 名字里看不出是哪一列。
        let raw = match col {
            Some(c) => unique_local(format!("{to_local}__{from_local}__{c}"), &mut taken),
            None => inverse_candidate(from_local, to_local, &taken),
        };
        taken.insert(raw.clone());
        inverse_locals.push(raw);
    }
    let forward_locals: Vec<String> = forward
        .iter()
        .map(|(from_local, to_local, col)| match col {
            Some(c) => format!("{from_local}__{to_local}__{c}"),
            None => format!("{from_local}__{to_local}"),
        })
        .collect();
    (forward_locals, inverse_locals)
}

/// 本地名去重：`base` 已被占用就连加 `_2` / `_3`…
///
/// 只在**同一对 (from, to) 有多条边**时才会用到（见 `model_export` 的说明）。
/// 顺序由调用方的确定性排序决定，所以「谁先拿到 base」两次运行相同。
fn unique_local(base: String, taken: &BTreeSet<String>) -> String {
    if !taken.contains(&base) {
        return base;
    }
    let mut n = 2usize;
    loop {
        let candidate = format!("{base}_{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// **反向属性名的唯一实现**（纪律 H）：`<对象>__<主语>`。
///
/// 撞名（该名字已经是一条正向属性）⇒ 追加 `__inverse`；还撞 ⇒ `__inverse2`…
/// 本版数据里只有 `order ↔ customer_subscriptions` 这一对会走到 `__inverse`。
fn inverse_candidate(from_local: &str, to_local: &str, taken: &BTreeSet<String>) -> String {
    let base = format!("{to_local}__{from_local}");
    if !taken.contains(&base) {
        return base;
    }
    let mut candidate = format!("{base}__inverse");
    let mut n = 2usize;
    while taken.contains(&candidate) {
        candidate = format!("{base}__inverse{n}");
        n += 1;
    }
    candidate
}

/// **反向属性块的唯一实现**：domain/range 对调 + `owl:inverseOf` 指回前向。
///
/// 两版视图只提供各自的标签 / 注释 / 额外行（建模视图多一行 `ontok:cardinality`）。
#[allow(clippy::too_many_arguments)]
fn inverse_object_block(
    subject_local: &str,
    forward_local: &str,
    domain_local: &str,
    range_local: &str,
    label: &str,
    comment: Option<&str>,
    features: Cardinality,
    extra: &[String],
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(object_type_line(features).to_string());
    lines.push(format!("rdfs:label {}", zh(label)));
    // ⭐ 对调：反向的 domain = 前向的 range，反向的 range = 前向的 domain（R2）。
    lines.push(format!("rdfs:domain :{domain_local}"));
    lines.push(format!("rdfs:range :{range_local}"));
    for line in extra {
        lines.push(line.clone());
    }
    if let Some(c) = comment {
        lines.push(format!("rdfs:comment {}", zh(c)));
    }
    lines.push(format!("owl:inverseOf :{forward_local}"));
    block(&format!(":{subject_local}"), lines)
}

// ══════════════════════════════════════════════════════════════
// 建模视图：块（口径逐条照 `.local/export_ttl.py`）
// ══════════════════════════════════════════════════════════════

fn model_class_block(
    class_local: &str,
    e: &ModelEntityView,
    iri_template: &str,
    tenant_columns: &[String],
    primary_key: Option<&str>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:Class".to_string());
    let label = if e.display_name.trim().is_empty() { e.key.as_str() } else { e.display_name.as_str() };
    lines.push(format!("rdfs:label {}", zh(label)));
    lines.push(format!("ontok:key {}", plain(&e.key)));
    // REQ-119 · 实例 IRI 模板（AC-003）。
    lines.push(format!("ontok:instanceIri {}", plain(iri_template)));
    // REQ-119 B-2 · 主键列（只在有真实 schema 时写；⛔ 不猜）。
    if let Some(pk) = primary_key {
        lines.push(format!("ontok:primaryKeyColumn {}", plain(pk)));
    }
    if let Some(d) = e.description.as_deref().filter(|d| !d.trim().is_empty()) {
        lines.push(format!("rdfs:comment {}", zh(d)));
    }
    for a in &e.aliases {
        if !a.trim().is_empty() {
            lines.push(format!("skos:altLabel {}", zh(a)));
        }
    }
    if let Some(p) = e.parent.as_deref().filter(|p| !p.trim().is_empty()) {
        lines.push(format!("rdfs:subClassOf :{}", local_model(p)));
    }
    for d in &e.disjoint_with {
        if !d.trim().is_empty() {
            lines.push(format!("owl:disjointWith :{}", local_model(d)));
        }
    }
    // REQ-119 · 租户列（AC-004）：只在已签声明且命中时出现，且只在建模视图。
    for column in tenant_columns {
        lines.push(format!("ontok:tenantColumn {}", plain(column)));
    }
    block(&format!(":{class_local}"), lines)
}

fn datatype_block(subject_local: &str, obj_local: &str, p: &CustomerProp<'_>) -> String {
    let (xsd, basis, source) = xsd_range(None, None, None, p.name, p.note);
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:label {}", zh(p.name)));
    lines.push(format!("rdfs:domain :{obj_local}"));
    lines.push(format!("rdfs:range {}", xsd.iri()));
    lines.push(format!("ontok:rangeSource \"{}\"", source.key()));
    if let Some(n) = p.note {
        lines.push(format!("rdfs:comment {}", zh(n)));
    }
    lines.push(format!("ontok:nullable {}", bool_lit(!p.required)));
    if basis == RangeBasis::Unmapped {
        lines.push("ontok:rangeUnmapped true".to_string());
    }
    block(&format!(":{subject_local}"), lines)
}

fn model_datatype_block(
    class_local: &str,
    p: &ModelPropertyView,
    entity_key: &str,
    tenant_key: bool,
    is_primary_key: bool,
    schema: Option<&SchemaTypes>,
) -> String {
    let column = effective_column(p);
    let subject = format!(":{class_local}__{}", local_model(column));
    let label = if p.name.trim().is_empty() { column } else { p.name.as_str() };
    let (xsd, basis, source) = xsd_range(Some(entity_key), Some(column), schema, label, p.note.as_deref());
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:domain :{class_local}"));
    lines.push(format!("rdfs:range {}", xsd.iri()));
    lines.push(format!("ontok:rangeSource \"{}\"", source.key()));
    lines.push(format!("rdfs:label {}", zh(label)));
    lines.push(format!("ontok:column {}", plain(column)));
    lines.push(format!("ontok:nullable {}", bool_lit(p.nullable)));
    // 参照脚本：**只在 note != name 时**输出注释（770 条 note 里只有 8 条满足）。
    if let Some(n) = p.note.as_deref() {
        if !n.trim().is_empty() && n != label {
            lines.push(format!("rdfs:comment {}", zh(n)));
        }
    }
    // REQ-119 B-3 · 值域：只从注释里**明写的枚举**解析；解析不出就不写（⛔ 不编）。
    if let Some(values) = p.note.as_deref().and_then(parse_allowed_values) {
        lines.push(format!("ontok:allowedValue {}", plain(&values)));
    }
    if let Some(s) = p.sample.as_deref() {
        lines.push(format!("ontok:sample {}", plain(s)));
    }
    // REQ-119 · 租户键（AC-004）：只在建模视图、只在已签声明命中的属性上。
    if tenant_key {
        lines.push("ontok:tenantKey true".to_string());
    }
    // REQ-119 B-2 · 主键（schema 给的 is_primary_key / primary_key）。
    if is_primary_key {
        lines.push("ontok:isPrimaryKey true".to_string());
    }
    if basis == RangeBasis::Unmapped {
        lines.push("ontok:rangeUnmapped true".to_string());
    }
    block(&subject, lines)
}

/// REQ-119 B-2 · **补出来的主键属性块**（抽取侧把主键列剥掉了的情况）。
///
/// 例：`licenses` 的属性里只有 `license_key` / `name`，没有 `id`；没有 `id` 属性，
/// 字符串 FK（`license_id`）就无法还原成 IRI，SPARQL 不能 join。这里用 schema 的
/// 真实主键补一条 `owl:DatatypeProperty`，并标 `ontok:isPrimaryKey true`。
fn model_pk_block(
    class_local: &str,
    entity_key: &str,
    column: &str,
    schema: Option<&SchemaTypes>,
) -> String {
    let subject = format!(":{class_local}__{}", local_model(column));
    let (xsd, basis, source) = xsd_range(Some(entity_key), Some(column), schema, column, None);
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:domain :{class_local}"));
    lines.push(format!("rdfs:range {}", xsd.iri()));
    lines.push(format!("ontok:rangeSource \"{}\"", source.key()));
    lines.push(format!("rdfs:label {}", zh(column)));
    lines.push(format!("ontok:column {}", plain(column)));
    lines.push("ontok:nullable false".to_string());
    lines.push("ontok:isPrimaryKey true".to_string());
    if basis == RangeBasis::Unmapped {
        lines.push("ontok:rangeUnmapped true".to_string());
    }
    block(&subject, lines)
}

fn model_object_block(l: &ModelLinkView, subject_local: &str, inverse_of: Option<&str>) -> String {
    let from_local = local_model(&l.from_type);
    let to_local = local_model(&l.to_type);
    let mut lines: Vec<String> = Vec::new();
    lines.push(object_type_line(model_cardinality(&l.cardinality)).to_string());
    lines.push(format!("rdfs:label {}", zh(&format!("{} → {}", l.from_type, l.to_type))));
    lines.push(format!("rdfs:domain :{from_local}"));
    lines.push(format!("rdfs:range :{to_local}"));
    lines.push(format!("ontok:cardinality {}", plain(&l.cardinality)));
    lines.push(format!("ontok:semantic {}", plain(&l.semantic)));
    // ⭐ REQ-120 · 连接验证（AC-007）：**只有快照说了才写**。
    //
    // | 快照 | 这一行 | 为什么 |
    // | --- | --- | --- |
    // | `Verified` | `ontok:joinVerified "true"` + `ontok:joinEvidence` | 签过字的 FK 提案（值域重叠实测） |
    // | `Inherited` | `ontok:joinVerified "inherit"` + `ontok:joinEvidence` | 同名同义列继承（来源写进证据） |
    // | `NamingOnly` | `ontok:joinVerified "false"`（**不写证据**） | 只是列名后缀推断 |
    // | `None` | **不写** | REQ-120 之前的版本：没说过，替它说就是编 |
    //
    // 值是**字符串字面量**（`"true"` / `"false"`）而不是裸布尔：
    // AC-007 的原文写的就是 `ontok:joinVerified "true"`，判据会照着字面 grep。
    // （`ontok:tenantKey true` 那种裸布尔是 REQ-119 的口径，两者不统一是
    //  有意的 —— 本 REQ 不借「统一风格」去改另一个 REQ 已验收的三元组。）
    match l.join_verified {
        Some(v) => {
            // 三态：`"true"` / `"inherit"` / `"false"`（REQ-120 rev.2）。
            lines.push(format!("ontok:joinVerified {}", plain(v.as_str())));
            // 证据：**三态都写**（REQ-120 rev.3 · F-2）。
            //
            // 早先版本只给 `true` / `inherit` 写证据，理由是「`false` 没证据可写」。
            // 替身复跑否掉了这个理由：数字员工看到 `false` 却不知道是
            // **数据不够**（`insufficient_sample: 源表仅 4 行`）还是**只看了名字**
            // （`naming-only`）—— 这两种可信度完全不同，必须能区分。
            if let Some(e) = l.join_evidence.as_deref().filter(|e| !e.trim().is_empty()) {
                lines.push(format!("ontok:joinEvidence {}", plain(e)));
            }
        }
        None => {}
    }
    if !l.semantic.trim().is_empty() {
        lines.push(format!(
            "rdfs:comment {}",
            zh(&format!(
                "语义 {}（has/belongs_to 无 OWL 对应物，导出有损，仅作注释）",
                l.semantic
            ))
        ));
    }
    if !l.key.trim().is_empty() {
        lines.push(format!("ontok:key {}", plain(&l.key)));
    }
    if let Some(inv) = inverse_of {
        lines.push(format!("owl:inverseOf :{inv}"));
    }
    block(&format!(":{subject_local}"), lines)
}

// ══════════════════════════════════════════════════════════════
// 文本原语
// ══════════════════════════════════════════════════════════════

/// 一个块：`:<local> a … ;\n    谓词 宾语 ;\n    … 谓词 宾语 .`
fn block(subject: &str, lines: Vec<String>) -> String {
    let mut out = String::with_capacity(64 + lines.iter().map(String::len).sum::<usize>());
    out.push_str(subject);
    for (i, l) in lines.iter().enumerate() {
        if i == 0 {
            out.push(' ');
        } else {
            out.push_str("\n    ");
        }
        out.push_str(l);
        if i + 1 == lines.len() {
            out.push_str(" .\n");
        } else {
            out.push_str(" ;");
        }
    }
    out
}

/// 普通字符串字面量：`"…"`（转义见 [`escape`]）。**不带语言标签**。
fn plain(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

/// 中文文本字面量：`"…"@zh`（R6：中文按 UTF-8 原样，不加 `\u` 转义）。
fn zh(s: &str) -> String {
    format!("\"{}\"@zh", escape(s))
}

fn bool_lit(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// Turtle 字符串转义：`\` `"` 换行 / 回车 / 制表 / 其余控制字符。
///
/// ⛔ 中文**不转义**（R6：按 UTF-8 原样输出）。控制字符用 `\u00XX` ——
/// 不这么做的话，一个裸 `\r` 会让 Turtle 文件在 Windows 上被读成两行。
///
/// ⛔ REQ-121：R2RML 生成器也用它（同一个转义器，不抄第二份）。
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// **建模视图**的 IRI 本地名 —— 与参照脚本 `local()` **逐字一致**：
/// 非 `[0-9A-Za-z_]` 一律换 `_`；空 → `unnamed`；数字开头 → 前缀 `_`。
///
/// ⚠️ 不是单射（`a-b` 与 `a_b` 撞）。参照物如此，本轮照抄以保持等价；
/// 客户视图**不用**它（见 [`local_customer`]）。
pub fn local_model(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() {
        s = "unnamed".to_string();
    }
    if s.as_bytes()[0].is_ascii_digit() {
        s.insert(0, '_');
    }
    s
}

/// **客户视图**的 IRI 本地名 —— 客户看到的是人话（中文名），`local_model` 会把
/// 每个汉字压成 `_`（两个汉字的两个对象直接撞车）。
///
/// 这里做的是**保留**：字母（含汉字）、数字、`_`、`-`、`.` 原样；其余字符（空格、
/// 引号、`#`、`/`、`:`、`%` …）换成 `_`；空 → `unnamed`；数字开头 → 前缀 `_`。
///
/// 为什么保留 Unicode 而不是百分号转义：`:` 前缀写法（`:<local>`）里
/// 百分号转义**不合法**；而 Turtle 的 PN_LOCAL 允许 Unicode 字母 ——
/// 汉字原样是最小惊讶，也和 `rdfs:label` 里看到的名字对得上。
///
/// ⚠️ 与其他名字撞车由 [`NameSpace::local_for`] 统一加后缀（`_2` / `_3`）。
pub fn local_customer(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' {
            s.push(c);
        } else {
            s.push('_');
        }
    }
    if s.is_empty() {
        s = "unnamed".to_string();
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView};
    use crate::domain::view::{EntityView, PropertyView, RelationView};

    fn cv() -> CustomerView {
        CustomerView {
            objects: vec![
                EntityView {
                    name: "订单".into(),
                    note: Some("一张订单".into()),
                    belongs_to: None,
                    aliases: vec!["单据".into()],
                    properties: vec![
                        PropertyView {
                            name: "下单时间".into(),
                            note: None,
                            required: true,
                            sample: Some("2026-01-01".into()),
                        },
                        PropertyView {
                            name: "金额".into(),
                            note: Some("含税".into()),
                            required: false,
                            sample: None,
                        },
                    ],
                },
                EntityView {
                    name: "客户".into(),
                    note: None,
                    belongs_to: None,
                    aliases: vec![],
                    properties: vec![],
                },
            ],
            relations: vec![RelationView {
                from: "订单".into(),
                to: "客户".into(),
                how_many: "多对一".into(),
                meaning: "归属于".into(),
            }],
        }
    }

    fn mv() -> ModelTrackView {
        ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                ModelEntityView {
                    key: "order".into(),
                    display_name: "订单".into(),
                    description: Some("一张订单".into()),
                    aliases: vec!["单据".into()],
                    parent: Some("root".into()),
                    disjoint_with: vec!["customer".into()],
                    properties: vec![ModelPropertyView {
                        name: "下单时间".into(),
                        column: "created_at".into(),
                        nullable: false,
                        note: Some("下单时间（列注释）".into()),
                        sample: None,
                    }],
                    evidence: vec![],
                },
                ModelEntityView {
                    key: "customer".into(),
                    display_name: "客户".into(),
                    description: None,
                    aliases: vec![],
                    parent: None,
                    disjoint_with: vec![],
                    properties: vec![],
                    evidence: vec![],
                },
            ],
            link_types: vec![ModelLinkView {
                key: "order_customer".into(),
                from_type: "order".into(),
                to_type: "customer".into(),
                cardinality: "N:1".into(),
                semantic: "belongs_to".into(),
                evidence: vec![],
                confidence: None,
                // REQ-120：默认 `None` = 这一版没有连接验证信息（不写 joinVerified）。
                join_verified: None,
                join_evidence: None,
                column: None,
            }],
            action_types: vec![],
            functions: vec![],
        }
    }

    fn opts() -> TtlOptions<'static> {
        TtlOptions {
            project: "demo",
            version: 2,
            base: DEFAULT_BASE,
            action_type_count: 0,
            tenancy: None,
            schema: None,
        }
    }

    #[test]
    fn header_declares_the_three_losses() {
        let d = serialize_customer(&cv(), &opts());
        for kw in ["ActionType", "semantic", "cardinality"] {
            assert!(d.doc().contains(kw), "文件头必须有 `{kw}`（AC-008）：\n{}", d.doc());
        }
        assert!(d.doc().contains("未导出（0 个）"), "数量必须写出来（AC-008）：\n{}", d.doc());
        // 正文**不许**含那个被 AC-008 强行要求写进文件头的守门词 —— 否则「扫正文」等于必挂。
        assert!(
            !d.body().contains("cardinality"),
            "正文里出现了 `cardinality` —— 发布闸会恒命中：\n{}",
            d.body()
        );
    }

    #[test]
    fn customer_export_is_deterministic_and_order_independent() {
        let a = serialize_customer(&cv(), &opts()).into_doc();
        let b = serialize_customer(&cv(), &opts()).into_doc();
        assert_eq!(a, b, "同一份视图两次导出必须逐字节相同（AC-003）");

        // 反序输入（对象 / 属性 / 关系都倒过来）必须产出同字节 ——
        // 「只取决于集合」这条比「两次调用相同」强：它挡的是 HashMap 迭代序。
        let mut rev = cv();
        rev.objects.reverse();
        for o in &mut rev.objects {
            o.properties.reverse();
        }
        rev.relations.reverse();
        assert_eq!(a, serialize_customer(&rev, &opts()).into_doc(), "输出依赖了输入顺序");
    }

    #[test]
    fn customer_counts_are_one_per_thing() {
        let d = serialize_customer(&cv(), &opts()).into_doc();
        assert_eq!(d.matches("a owl:Class").count(), 2, "{d}");
        assert_eq!(d.matches("a owl:DatatypeProperty").count(), 2, "{d}");
        // REQ-119：1 条正向 + 1 条反向 = 2 条对象属性、2 条 owl:inverseOf。
        assert_eq!(d.matches("owl:ObjectProperty").count(), 2, "{d}");
        assert_eq!(d.matches("owl:inverseOf").count(), 2, "{d}");
        assert_eq!(d.matches("ontok:instanceIri").count(), 2, "{d}");
        assert!(d.contains("skos:altLabel"), "别名应写成 skos:altLabel：{d}");
        assert!(d.contains("rdfs:label \"订单\"@zh"), "中文标签必须原样输出：{d}");
    }

    #[test]
    fn model_export_maps_the_nine_rows() {
        let doc = serialize_model(&mv(), &opts());
        let d = doc.doc();
        assert!(d.contains("ontok:key \"order\""), "{d}");
        assert!(d.contains("ontok:column \"created_at\""), "{d}");
        assert!(d.contains("ontok:nullable false"), "{d}");
        assert!(d.contains("ontok:cardinality \"N:1\""), "{d}");
        assert!(d.contains("ontok:semantic \"belongs_to\""), "{d}");
        // B-1：N:1（每个 subject 至多一个 object）⇒ FunctionalProperty。
        assert!(
            block_of(d, ":order__customer a ").contains("a owl:FunctionalProperty"),
            "N:1 必须是 functional：{d}"
        );
        assert!(
            block_of(d, ":customer__order a ").contains("a owl:InverseFunctionalProperty"),
            "N:1 的反向是 1:N ⇒ inverse-functional：{d}"
        );
        assert!(d.contains("rdfs:subClassOf :root"), "{d}");
        assert!(d.contains("owl:disjointWith :customer"), "{d}");
        // REQ-119：`created_at` 是时间列 ⇒ dateTime，不再是 rdfs:Literal。
        assert!(d.contains("rdfs:range xsd:dateTime"), "{d}");
        assert!(!d.contains("rdfs:range rdfs:Literal"), "不许再一律 Literal：{d}");
        // 动作层不导出（只在文件头声明数量）—— 正文里根本没有动作内容。
        assert!(!doc.body().contains("ActionType "), "动作层不该进正文");
    }

    /// ⭐ REQ-120 · **同一对 (from,to) 的两条边不许写成同一个 IRI**。
    ///
    /// 这是本轮在真实库上抓到的**真 bug**（不是构造的）：
    /// `customer_account_binding` 的 `main_customer_id` 与 `sub_customer_id`
    /// 都指向 `customers`，旧命名 `{from}__{to}` 让两条属性共用
    /// `:customer_account_binding__customers` ⇒ Turtle 合并成一个属性，
    /// 两条 `owl:inverseOf`、两个 `ontok:cardinality` 全挤在同一个主语上。
    ///
    /// 变异自证：把 `pair_count` 的歧义判定去掉（永远走 `{from}__{to}`），
    /// 本测试立刻红（两个块变成同名）。
    #[test]
    fn colliding_link_pairs_get_column_qualified_local_names() {
        let mut v = mv();
        // `mv()` 里已有 order→customer；再加第二条同向边（同一对表、不同列）。
        v.link_types[0].column = Some("created_by".into());
        v.entity_types[0].properties.push(ModelPropertyView {
            name: "客户".into(),
            column: "customer_id".into(),
            nullable: true,
            note: None,
            sample: None,
        });
        v.link_types.push(ModelLinkView {
            key: "order_customer_id__customer".into(),
            from_type: "order".into(),
            to_type: "customer".into(),
            cardinality: "N:1".into(),
            semantic: "belongs_to".into(),
            evidence: vec![crate::domain::snapshot::Evidence {
                table: "order".into(),
                column: "customer_id".into(),
                claim: "列名推断".into(),
            }],
            confidence: None,
            join_verified: Some(crate::domain::snapshot::JoinState::Verified),
            join_evidence: Some("value-overlap 5/5 (100.00%) -> customer.id via plural".into()),
            column: Some("customer_id".into()),
        });
        let d = serialize_model(&v, &opts()).into_doc();

        // 两条正向各自成块，且名字里带列名区分。
        assert!(d.contains(":order__customer__created_by a "), "{d}");
        assert!(d.contains(":order__customer__customer_id a "), "{d}");
        // 反向也对称地带列名，不再是 `__inverse` / `__inverse2`。
        assert!(d.contains(":customer__order__created_by a "), "{d}");
        assert!(d.contains(":customer__order__customer_id a "), "{d}");
        // 关键：**没有**任何一个本地名重复出现两次。
        for name in [
            "order__customer",
            "customer__order",
            "order__customer__inverse",
            "customer__order__inverse",
        ] {
            let exact = format!(":{name} a ");
            assert_eq!(
                d.matches(&exact).count(),
                0,
                "`{name}` 不该再是任何块的主语（有歧义时必须带列名）：{d}"
            );
        }
        // 列名只在这一对有歧义时出现：唯一的那条关系名字不变。
        let mut only = mv();
        only.link_types[0].column = Some("created_by".into());
        let solo = serialize_model(&only, &opts()).into_doc();
        assert!(solo.contains(":order__customer a "), "唯一的一对不许多加后缀：{solo}");
    }

    #[test]
    fn model_comment_only_when_note_differs_from_name() {
        // 参照脚本：`if p.get("note") and p["note"] != p.get("name")`
        let mut v = mv();
        v.entity_types[0].properties[0].note = Some("下单时间".into());
        let d = serialize_model(&v, &opts()).into_doc();
        assert_eq!(
            d.matches("rdfs:comment \"下单时间（列注释）\"").count(),
            0,
            "note == name 时不该输出注释：{d}"
        );
    }

    #[test]
    fn escaping_covers_quotes_backslash_and_controls() {
        let s = escape("a\"b\\c\nd\re\tf\u{1}");
        assert_eq!(s, "a\\\"b\\\\c\\nd\\re\\tf\\u0001");
        assert!(escape("中文").contains("中文"), "中文不许转义成 \\u");
    }

    #[test]
    fn local_names_are_safe_and_distinct() {
        assert_eq!(local_model("order"), "order");
        assert_eq!(local_model("2fa"), "_2fa");
        assert_eq!(local_model("a-b"), "a_b");
        assert_eq!(local_model(""), "unnamed");
        // 客户视图保留汉字；空格 / 引号换成下划线。
        assert_eq!(local_customer("订单"), "订单");
        assert_eq!(local_customer("a b"), "a_b");
        assert_eq!(local_customer("3月"), "_3月");
        let mut ns = NameSpace::new(&opts());
        // 撞车：第二个同形名字拿到后缀，且**同一个名字**永远拿到同一个后缀。
        let a = ns.local_for("a-b");
        let b = ns.local_for("a_b");
        assert_ne!(a, b, "两个不同的名字不许共用一个 IRI");
        assert_eq!(ns.local_for("a-b"), a);
        assert_eq!(ns.local_for("a_b"), b);
    }

    #[test]
    fn base_option_is_normalized() {
        let mut o = opts();
        o.base = "https://example.org/onto";
        let d = serialize_customer(&cv(), &o).into_doc();
        assert!(d.contains("<https://example.org/onto/demo> a owl:Ontology"), "{d}");
        assert!(d.contains("@prefix : <https://example.org/onto/demo#> ."), "{d}");
    }

    #[test]
    fn extracted_by_becomes_prov_activity() {
        let mut v = mv();
        v.extracted_by = Some(crate::domain::snapshot::ExtractedBy {
            agent: "inproc:ontok::agent::RuleBasedAgent".into(),
            version: "0.1.0".into(),
            round: 1,
            failed_groups: vec![],
            source: None,
        });
        let d = serialize_model(&v, &opts()).into_doc();
        assert!(d.contains("prov:wasGeneratedBy [ a prov:Activity"), "{d}");
        assert!(
            d.contains("prov:wasAssociatedWith \"inproc:ontok::agent::RuleBasedAgent\""),
            "{d}"
        );
        assert!(d.contains("ontok:round 1"), "{d}");
    }

    // ══════════════════════════════════════════════════════════
    // REQ-119 · 可查询化
    // ══════════════════════════════════════════════════════════

    /// 取出一个块（主语行到 ` .` 为止）。
    fn block_of<'a>(doc: &'a str, subject: &str) -> &'a str {
        let start = doc.find(subject).unwrap_or_else(|| panic!("找不到 {subject}：\n{doc}"));
        let rest = &doc[start..];
        let end = rest.find(" .\n").map(|i| i + 3).unwrap_or(rest.len());
        &rest[..end]
    }

    #[test]
    fn inverse_is_declared_both_ways_and_swaps_domain_range() {
        let d = serialize_customer(&cv(), &opts()).into_doc();
        let fwd = block_of(&d, ":订单__客户 a ");
        let inv = block_of(&d, ":客户__订单 a ");
        assert!(fwd.contains("rdfs:domain :订单"), "{fwd}");
        assert!(fwd.contains("rdfs:range :客户"), "{fwd}");
        assert!(
            fwd.contains("a owl:FunctionalProperty"),
            "多对一（N:1）的前向必须是 Functional：{fwd}"
        );
        assert!(fwd.contains("owl:inverseOf :客户__订单"), "{fwd}");
        // 反向：domain/range 对调 + 基数特征 1:N ↔ N:1 对调。
        assert!(inv.contains("rdfs:domain :客户"), "{inv}");
        assert!(inv.contains("rdfs:range :订单"), "{inv}");
        assert!(inv.contains("owl:inverseOf :订单__客户"), "{inv}");
        assert!(
            inv.contains("a owl:InverseFunctionalProperty"),
            "一对多（1:N）的反向必须是 InverseFunctional：{inv}"
        );
        assert_eq!(d.matches("owl:inverseOf").count(), 2, "双向各一条：{d}");
    }

    #[test]
    fn inverse_collision_gets_the_documented_suffix() {
        let mut v = cv();
        // 再加一条反向关系：客户 → 订单。它的反向名 `订单__客户` 已被正向占用。
        v.relations.push(RelationView {
            from: "客户".into(),
            to: "订单".into(),
            how_many: "一对多".into(),
            meaning: "拥有".into(),
        });
        let d = serialize_customer(&v, &opts()).into_doc();
        assert!(d.contains(":订单__客户 a "), "正向属性还在：{d}");
        assert!(
            d.contains(":订单__客户__inverse a "),
            "撞名的反向属性必须带 `__inverse` 后缀：{d}"
        );
        assert_eq!(d.matches("owl:inverseOf").count(), 4, "两条关系 ⇒ 4 条 inverseOf：{d}");
    }

    #[test]
    fn xsd_ranges_are_typed_and_unmapped_is_counted() {
        let mut v = cv();
        v.objects[0].properties = vec![
            PropertyView {
                name: "金额".into(),
                note: None,
                required: true,
                sample: None,
            },
            PropertyView {
                name: "下单时间".into(),
                note: None,
                required: true,
                sample: None,
            },
            PropertyView {
                name: "是否启用".into(),
                note: None,
                required: true,
                sample: None,
            },
            PropertyView {
                name: "备注".into(),
                note: None,
                required: true,
                sample: None,
            },
            PropertyView {
                name: "星云常数".into(),
                note: None,
                required: true,
                sample: None,
            },
        ];
        let doc = serialize_customer(&v, &opts());
        let d = doc.doc();
        assert!(d.contains("rdfs:range xsd:decimal"), "金额 ⇒ decimal：{d}");
        assert!(d.contains("rdfs:range xsd:dateTime"), "下单时间 ⇒ dateTime：{d}");
        assert!(d.contains("rdfs:range xsd:boolean"), "是否启用 ⇒ boolean：{d}");
        assert!(!d.contains("rdfs:range rdfs:Literal"), "不许再一律 Literal：{d}");
        // 认不出的 `星云常数` ⇒ xsd:string + 显式标记，并且被统计列出。
        assert!(d.contains("ontok:rangeUnmapped true"), "{d}");
        let stats = range_stats(d);
        assert_eq!(stats.get("literal"), 0, "不允许残留 rdfs:Literal：{d}");
        assert_eq!(stats.get("decimal"), 1, "{d}");
        assert_eq!(stats.get("dateTime"), 1, "{d}");
        assert_eq!(stats.get("boolean"), 1, "{d}");
        assert_eq!(stats.unmapped, vec!["星云常数".to_string()], "未映射必须列出");
    }

    #[test]
    fn column_names_drive_ranges_in_the_model_view() {
        // 列名优先：`is_deleted` 即使中文名是「备注」也必须 boolean；
        // `operation_count` 是整数、`created_at` 是时间、`amount` 是小数。
        let mut v = mv();
        v.entity_types[0].properties = vec![
            ModelPropertyView {
                name: "备注".into(),
                column: "is_deleted".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "操作次数".into(),
                column: "operation_count".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "创建时间".into(),
                column: "created_at".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "金额".into(),
                column: "amount".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "莫名列".into(),
                column: "zzz_unknown".into(),
                nullable: true,
                note: None,
                sample: None,
            },
        ];
        let doc = serialize_model(&v, &opts());
        let d = doc.doc();
        assert!(block_of(d, ":order__is_deleted a ").contains("rdfs:range xsd:boolean"), "{d}");
        assert!(
            block_of(d, ":order__operation_count a ").contains("rdfs:range xsd:integer"),
            "{d}"
        );
        assert!(block_of(d, ":order__created_at a ").contains("rdfs:range xsd:dateTime"), "{d}");
        assert!(block_of(d, ":order__amount a ").contains("rdfs:range xsd:decimal"), "{d}");
        assert!(
            block_of(d, ":order__zzz_unknown a ").contains("ontok:rangeUnmapped true"),
            "{d}"
        );
    }

    #[test]
    fn sql_types_map_to_the_documented_xsd() {
        // A-1 的映射表：numeric 必须是 decimal（point_change 会丢精度，
        // character varying 即使内容是 "10" 也必须是 string）。
        for (sql, want) in [
            ("numeric", XsdType::Decimal),
            ("decimal(10,2)", XsdType::Decimal),
            ("integer", XsdType::Integer),
            ("bigint", XsdType::Integer),
            ("serial", XsdType::Integer),
            ("boolean", XsdType::Boolean),
            ("character varying", XsdType::String),
            ("character varying(64)", XsdType::String),
            ("text", XsdType::String),
            ("uuid", XsdType::String),
            ("jsonb", XsdType::String),
            ("timestamp without time zone", XsdType::DateTime),
            ("timestamp with time zone", XsdType::DateTime),
            ("date", XsdType::Date),
        ] {
            assert_eq!(xsd_for_sql_type(sql), Some(want), "SQL {sql:?} 的映射不对");
        }
        assert_eq!(xsd_for_sql_type("custom_enum"), None, "认不出的 SQL 类型不许瞎猜");
    }

    #[test]
    fn schema_primary_key_is_single_column_only() {
        let s = SchemaTypes::parse(
            r#"{"tables":[
              {"name":"t1","primary_key":"id","columns":[{"name":"id","data_type":"uuid"}]},
              {"name":"t2","primary_key":null,"columns":[
                {"name":"a","data_type":"uuid","is_primary_key":true},
                {"name":"b","data_type":"uuid","is_primary_key":true}
              ],"composite_primary_key":["a","b"]},
              {"name":"t3","columns":[{"name":"k","data_type":"uuid","is_primary_key":true}]}
            ]}"#,
        )
        .expect("合法");
        assert_eq!(s.primary_key("T1"), Some("id"), "单列主键 + 大小写归一");
        assert_eq!(s.primary_key("t2"), None, "复合主键不导出（⛔ 不拼假列）");
        assert_eq!(s.primary_key("t3"), Some("k"), "只有 is_primary_key 标记也能认");
        assert_eq!(s.primary_key("nope"), None);
    }

    #[test]
    fn allowed_value_parser_never_invents() {
        assert_eq!(
            parse_allowed_values("状态(0-未开始,1-活动中)").as_deref(),
            Some("0=未开始;1=活动中")
        );
        assert_eq!(
            parse_allowed_values("币种：POINT/RMB/USD").as_deref(),
            Some("POINT;RMB;USD")
        );
        assert_eq!(parse_allowed_values("模型系列(如: GPT, Claude, GLM)"), None);
        assert_eq!(parse_allowed_values("IP地址（兼容IPv4/IPv6）"), None);
        assert_eq!(parse_allowed_values("保存原始tokens/minute"), None);
        assert_eq!(parse_allowed_values("只有 1-正常运行"), None);
        assert_eq!(parse_allowed_values(""), None);
    }

    fn opts_with_schema(schema: &SchemaTypes) -> TtlOptions<'_> {
        TtlOptions {
            project: "demo",
            version: 2,
            base: DEFAULT_BASE,
            action_type_count: 0,
            tenancy: None,
            schema: Some(schema),
        }
    }

    #[test]
    fn schema_types_override_heuristic_and_mark_the_source() {
        let schema = SchemaTypes::parse(
            r#"{"tables":[{"name":"order","columns":[
                {"name":"is_deleted","data_type":"integer"},
                {"name":"amount","data_type":"character varying"},
                {"name":"created_at","data_type":"timestamp without time zone"},
                {"name":"status","data_type":"integer"}
            ]}]}"#,
        )
        .expect("schema 合法");
        let mut v = mv();
        v.entity_types[0].properties = vec![
            ModelPropertyView {
                name: "备注".into(),
                column: "is_deleted".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "金额".into(),
                column: "amount".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "创建时间".into(),
                column: "created_at".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "状态".into(),
                column: "status".into(),
                nullable: false,
                note: None,
                sample: None,
            },
            ModelPropertyView {
                name: "莫名列".into(),
                column: "zzz_unknown".into(),
                nullable: true,
                note: None,
                sample: None,
            },
        ];
        let doc = serialize_model(&v, &opts_with_schema(&schema));
        let d = doc.doc();
        // 真实类型压过启发式：is_deleted(integer) 不再 boolean、amount(varchar) 不再 decimal。
        assert!(block_of(d, ":order__is_deleted a ").contains("rdfs:range xsd:integer"), "{d}");
        assert!(block_of(d, ":order__amount a ").contains("rdfs:range xsd:string"), "{d}");
        assert!(block_of(d, ":order__created_at a ").contains("rdfs:range xsd:dateTime"), "{d}");
        assert!(block_of(d, ":order__status a ").contains("rdfs:range xsd:integer"), "{d}");
        for col in ["is_deleted", "amount", "created_at", "status"] {
            assert!(
                block_of(d, &format!(":order__{col} a ")).contains("ontok:rangeSource \"schema\""),
                "{col} 必须标 schema 来源：{d}"
            );
        }
        // 没匹配上的列：启发式 + 未映射标记 + 清单。
        let unknown = block_of(d, ":order__zzz_unknown a ");
        assert!(unknown.contains("ontok:rangeSource \"heuristic\""), "{unknown}");
        assert!(unknown.contains("ontok:rangeUnmapped true"), "{unknown}");
        let stats = range_stats(d);
        assert_eq!(stats.source("schema"), 4, "{stats:?}");
        assert_eq!(stats.source("heuristic"), 1, "{stats:?}");
        assert_eq!(stats.heuristic, vec!["莫名列".to_string()]);
        assert!(d.contains("ontok:rangeSourceSchema 4"), "本体节点必须有计数：{d}");
        assert!(d.contains("ontok:rangeSourceHeuristic 1"), "{d}");
    }

    #[test]
    fn no_schema_marks_heuristic_in_header_ontology_and_stats() {
        let doc = serialize_model(&mv(), &opts());
        let d = doc.doc();
        assert!(
            d.contains("# · xsd 类型来源：**启发式**（未读真实列类型"),
            "① 文件头必须明说启发式：{d}"
        );
        assert!(d.contains("ontok:rangeSourceHeuristic 1"), "② 本体节点计数：{d}");
        assert!(
            block_of(d, ":order__created_at a ").contains("ontok:rangeSource \"heuristic\""),
            "③ 每个属性标来源：{d}"
        );
        let stats = range_stats(d);
        assert_eq!(stats.source("schema"), 0, "{stats:?}");
        assert_eq!(stats.source("heuristic"), 1, "{stats:?}");
    }

    #[test]
    fn every_class_declares_an_instance_iri_template() {
        let c = serialize_customer(&cv(), &opts()).into_doc();
        assert_eq!(c.matches("ontok:instanceIri").count(), 2, "{c}");
        assert!(
            c.contains("ontok:instanceIri \"https://ontok.local/demo#{class}/{id}\""),
            "模板必须由 base+project 派生：{c}"
        );
        // 客户视图**不**出现列名/key，但模板照样在。
        assert!(!c.contains("ontok:column"), "客户视图不许出现列名：{c}");
        let m = serialize_model(&mv(), &opts()).into_doc();
        assert_eq!(m.matches("ontok:instanceIri").count(), m.matches("a owl:Class").count());
    }

    fn tenancy_model() -> ModelTrackView {
        let mut v = mv();
        v.entity_types[0].properties.push(ModelPropertyView {
            name: "所属客户".into(),
            column: "customer_id".into(),
            nullable: false,
            note: None,
            sample: None,
        });
        v
    }

    fn opts_with(decl: &TenancyDecl) -> TtlOptions<'_> {
        TtlOptions {
            project: "demo",
            version: 2,
            base: DEFAULT_BASE,
            action_type_count: 0,
            tenancy: Some(decl),
            schema: None,
        }
    }

    #[test]
    fn signed_tenancy_marks_only_in_the_model_view() {
        let decl = TenancyDecl::parse(
            r#"{"columns":["customer_id"],"reviewed_by":"安全负责人"}"#,
        )
        .expect("合法声明");
        let model = tenancy_model();
        let o = opts_with(&decl);
        let m = serialize_model(&model, &o).into_doc();
        assert!(m.contains("ontok:tenantKey true"), "{m}");
        assert!(m.contains("ontok:tenantColumn \"customer_id\""), "{m}");
        assert!(!m.contains("ontok:tenantUnannotated"), "已签且命中 ⇒ 不该标未标注：{m}");
        assert!(m.contains("ontok:tenancyReviewedBy \"安全负责人\""), "{m}");

        // 同一个 opts 走客户视图 ⇒ **一个 tenant 都没有**（AC-007）。
        let c = serialize_customer(&cv(), &o).into_doc();
        assert_eq!(c.to_lowercase().matches("tenant").count(), 0, "{c}");
    }

    #[test]
    fn unsigned_tenancy_is_refused_but_visible() {
        let decl =
            TenancyDecl::parse(r#"{"columns":["customer_id"],"reviewed_by":null}"#).expect("合法");
        let o = opts_with(&decl);
        let m = serialize_model(&tenancy_model(), &o).into_doc();
        assert!(!m.contains("ontok:tenantKey true"), "未签声明不许落标注：{m}");
        assert!(m.contains("ontok:tenancyReviewed false"), "{m}");
        assert!(m.contains("ontok:tenantUnannotated true"), "{m}");
        assert!(m.contains("# · 租户键：未签"), "文件头必须写未签：{m}");
        assert!(
            tenancy_report(&tenancy_model(), Some(&decl))
                .warning()
                .expect("未签必须给 stderr 警告")
                .contains("不能直接给数字员工用")
        );
    }

    #[test]
    fn missing_tenancy_marks_header_ttl_and_warning() {
        let doc = serialize_model(&tenancy_model(), &opts());
        let m = doc.doc();
        assert!(m.contains("# · 租户键：未标注"), "① 文件头：{m}");
        assert!(m.contains("ontok:tenantUnannotated true"), "② TTL 标记：{m}");
        let w = tenancy_report(&tenancy_model(), None).warning().expect("③ stderr 警告");
        assert!(w.contains("不能直接给数字员工用"), "{w}");
        assert!(!m.contains("ontok:tenantKey true"), "没声明就绝不能有 tenantKey：{m}");
    }

    #[test]
    fn tenancy_tables_restrict_the_match() {
        let decl = TenancyDecl::parse(
            r#"{"columns":["customer_id"],"tables":["customer"],"reviewed_by":"签"}"#,
        )
        .expect("合法");
        // `order` 不在 tables 里 ⇒ 它的 customer_id 不该被标。
        let m = serialize_model(&tenancy_model(), &opts_with(&decl)).into_doc();
        assert!(!m.contains("ontok:tenantKey true"), "tables 过滤没生效：{m}");
        assert!(m.contains("ontok:tenantUnannotated true"), "一个都没命中 ⇒ 未标注：{m}");
    }

    #[test]
    fn tenancy_parse_rejects_typos_and_empty_reviewers() {
        assert!(TenancyDecl::parse(r#"{"column":["customer_id"]}"#).is_err(), "未知字段必须报错");
        let d = TenancyDecl::parse(r#"{"columns":[" customer_id "],"reviewed_by":"   "}"#)
            .expect("合法");
        assert_eq!(d.columns, vec!["customer_id".to_string()], "必须 trim");
        assert!(!d.signed(), "空白 reviewed_by = 未签");
    }
}
