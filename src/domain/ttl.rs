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
//! | `PropertyView` | `owl:DatatypeProperty` + `rdfs:domain` + `rdfs:range rdfs:Literal` | `nullable = !required` |
//! | `RelationView` | `owl:ObjectProperty` + `rdfs:domain` / `rdfs:range` | 见下方基数映射 |
//! | `RelationView.how_many` | `owl:FunctionalProperty` / `owl:InverseFunctionalProperty` | 1:N ⇒ Functional；N:1 ⇒ InverseFunctional；1:1 ⇒ 两者 |
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
//! ## 2. xsd range（R3 / AC-002）
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
//! ⚠️ **为什么是推断而不是「读列类型」**：快照层的 `snapshot::Property` 只有
//! `column` / `nullable` / `note` / `sample`，**没有 SQL 类型**，而导出命令只连
//! 本体库、不连源库。所以这里做的是**确定性推断**（列名优先、中文业务名次之），
//! 不是伪造一个「已知类型」。认不出的属性**照样导出**（`xsd:string`），但会在
//! 文件里留 `ontok:rangeUnmapped true`、在 `range_stats()` 里计数、由 CLI 在
//! stderr 逐条列出 —— ⛔ 不许静默。等 REQ-120 的 R2RML 拿到真实列类型时，
//! 映射表就是它的输入规格。
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
}

impl<'a> TtlOptions<'a> {
    /// 用默认 base 与「动作层 0 个」构造（测试与纯函数调用方用）。
    pub fn new(project: &'a str, version: i32) -> Self {
        Self { project, version, base: DEFAULT_BASE, action_type_count: 0, tenancy: None }
    }

    /// base 归一化：去掉结尾 `#`，补齐结尾 `/`。
    fn normalized_base(&self) -> String {
        let b = self.base.trim_end_matches('#');
        if b.ends_with('/') {
            b.to_string()
        } else {
            format!("{b}/")
        }
    }
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
fn effective_column(p: &ModelPropertyView) -> &str {
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

/// range 是靠什么定出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeBasis {
    /// 列名 / 业务名命中了规则。
    Matched,
    /// 一条规则都没命中 ⇒ `xsd:string` 兜底，并且必须计数 + 列出（R3）。
    Unmapped,
}

/// 列名 → 类型。**列名优先**（`Some(column)` 只在建模视图有）。
fn xsd_range(column: Option<&str>, name: &str, note: Option<&str>) -> (XsdType, RangeBasis) {
    if let Some(xsd) = column.and_then(classify_column) {
        return (xsd, RangeBasis::Matched);
    }
    if let Some(xsd) = classify_text(name, note) {
        return (xsd, RangeBasis::Matched);
    }
    (XsdType::String, RangeBasis::Unmapped)
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

/// 一份 TTL 里 `rdfs:range` 的分布 + 未映射清单。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RangeStats {
    /// `decimal` / `integer` / `dateTime` / `date` / `boolean` / `string` /
    /// `literal` / `other` → 条数。
    pub counts: BTreeMap<String, usize>,
    /// 带 `ontok:rangeUnmapped true` 的属性标签（去重、排序）。
    pub unmapped: Vec<String>,
}

impl RangeStats {
    pub fn get(&self, key: &str) -> usize {
        self.counts.get(key).copied().unwrap_or(0)
    }

    /// 数值列条数（AC-002 的「能不能 SUM」）。
    pub fn numeric(&self) -> usize {
        self.get("decimal") + self.get("integer")
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
    for line in ttl.lines() {
        let t = line.trim_start();
        // 新块（主语行）⇒ 清空上一个标签。
        if t.starts_with(':') || t.starts_with('<') {
            label = None;
        }
        if let Some(rest) = t.strip_prefix("rdfs:label ") {
            label = Some(clean_label(rest));
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
        if t.starts_with("ontok:rangeUnmapped ") {
            if let Some(l) = &label {
                stats.unmapped.push(l.clone());
            }
        }
    }
    stats.unmapped.sort();
    stats.unmapped.dedup();
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
    let mut ns = NameSpace::new(opts, local_model(opts.project));

    // ① 先给类分配 IRI 本地名（按名字字典序 ⇒ 分配结果与输入顺序无关）。
    let classes = distinct_customer_classes(view);
    let class_locals: Vec<String> = classes.iter().map(|(name, _)| ns.local_for(name)).collect();
    let iri_template = format!("{}#{{class}}/{{id}}", ns.ontology());

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

    let header =
        ontology_header(ns.ontology(), opts, CUSTOMER_ONTOLOGY_NOTE, None, None);
    finish(
        opts,
        Body {
            ontology: header,
            classes: class_blocks,
            props: prop_blocks,
            rels: rel_blocks,
            inverses: inverse_blocks.into_iter().map(|(_, b)| b).collect(),
        },
        None,
    )
}

/// **建模视图** → Turtle。建模术语在这一轨是**要被裁决的内容**，故不过发布闸
/// （轨道由 `pipeline::spec_of(OutletId::D9)` 显式声明，不是「没调闸所以豁免」）。
///
/// REQ-119：反向属性 / xsd range / IRI 模板 / **租户标注**（唯一会落标注的一轨）。
pub fn serialize_model(view: &ModelTrackView, opts: &TtlOptions<'_>) -> TtlDocument {
    let ns = NameSpace::new(opts, local_model(opts.project));
    let iri_template = format!("{}#{{class}}/{{id}}", ns.ontology());
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
        class_blocks.push(model_class_block(
            &class_local,
            e,
            &iri_template,
            &tenant_columns,
        ));
        let mut props: Vec<&ModelPropertyView> = e.properties.iter().collect();
        props.sort_by(|a, b| a.column.cmp(&b.column).then_with(|| a.name.cmp(&b.name)));
        for p in props {
            let tenant_key = tenancy.annotated()
                && tenancy
                    .hits
                    .get(&e.key)
                    .is_some_and(|cols| cols.contains(effective_column(p)));
            prop_blocks.push(model_datatype_block(&class_local, p, &e.key, tenant_key));
        }
    }

    let mut links: Vec<&ModelLinkView> = view.link_types.iter().collect();
    links.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then_with(|| a.from_type.cmp(&b.from_type))
            .then_with(|| a.to_type.cmp(&b.to_type))
    });

    let mut forward: Vec<(String, String)> = Vec::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for l in &links {
        let from_local = local_model(&l.from_type);
        let to_local = local_model(&l.to_type);
        let raw = format!("{from_local}__{to_local}");
        taken.insert(raw);
        forward.push((from_local, to_local));
    }
    let mut inverse_locals: Vec<String> = Vec::new();
    for (from_local, to_local) in &forward {
        let raw = inverse_candidate(from_local, to_local, &taken);
        taken.insert(raw.clone());
        inverse_locals.push(raw);
    }

    let mut rel_blocks: Vec<String> = Vec::new();
    let mut inverse_blocks: Vec<(String, String)> = Vec::new();
    for (i, l) in links.iter().enumerate() {
        let from_local = &forward[i].0;
        let to_local = &forward[i].1;
        let subject = format!("{from_local}__{to_local}");
        let inv = &inverse_locals[i];
        rel_blocks.push(model_object_block(l, &subject, Some(inv)));
        let features = model_cardinality(&l.cardinality).inverse();
        let label = format!("{} → {}（反向）", l.to_type, l.from_type);
        let comment =
            format!("反向属性：前向为 {} → {}（domain/range 已对调）", l.from_type, l.to_type);
        let extra = vec![format!(
            "ontok:cardinality {}",
            plain(&inverse_cardinality(&l.cardinality))
        )];
        inverse_blocks.push((
            inv.clone(),
            inverse_object_block(
                inv,
                &subject,
                to_local,
                from_local,
                &label,
                Some(&comment),
                features,
                &extra,
            ),
        ));
    }
    inverse_blocks.sort_by(|a, b| a.0.cmp(&b.0));

    finish(
        opts,
        Body {
            ontology: ontology_header(
                ns.ontology(),
                opts,
                &model_ontology_note(opts.action_type_count),
                view.extracted_by.as_ref().map(|e| (e.agent.as_str(), e.version.as_str(), e.round)),
                Some(&tenancy),
            ),
            classes: class_blocks,
            props: prop_blocks,
            rels: rel_blocks,
            inverses: inverse_blocks.into_iter().map(|(_, b)| b).collect(),
        },
        Some(&tenancy.header_line()),
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
    fn new(opts: &TtlOptions<'_>, project_local: String) -> Self {
        Self {
            ontology: format!("{}{}", opts.normalized_base(), project_local),
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

fn ontology_header(
    ontology: &str,
    opts: &TtlOptions<'_>,
    note: &str,
    extracted_by: Option<(&str, &str, i32)>,
    tenancy: Option<&TenancyReport>,
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

    // REQ-119 · 租户状态标记（只在建模视图传入；客户视图传 None）。
    let mut marks: Vec<String> = Vec::new();
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

fn finish(opts: &TtlOptions<'_>, b: Body, tenant_header: Option<&str>) -> TtlDocument {
    // 本体 IRI 与 `NameSpace::new` 同源（A-7：头部默认前缀 == 正文本体 IRI）。
    let ontology = format!("{}{}", opts.normalized_base(), local_model(opts.project));

    // 头：固定前缀 + 有损声明（AC-008）。
    let mut doc = String::new();
    doc.push_str(&format!("@prefix : <{ontology}#> .\n"));
    doc.push_str("@prefix dcterms: <http://purl.org/dc/terms/> .\n");
    doc.push_str("@prefix ontok: <https://ontok.local/schema/> .\n");
    for (p, iri) in PREFIX_TABLE {
        doc.push_str(&format!("@prefix {p}: <{iri}> .\n"));
    }
    doc.push('\n');
    doc.push_str(&lossy_header(opts, tenant_header));
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
    let stats = range_stats(&body);
    TtlDocument { doc, body, stats }
}

/// 文件头的**有损声明**（AC-008：三个关键词一个都不能少）。
///
/// ⛔ 它是**编译期常量 + 一个计数 + 一行租户状态**：不含任何项目数据
/// （尤其不写 `reviewed_by` 人名），所以把它排除在发布闸的扫描之外不会
/// 放过任何数据字节（理由见模块头）。
fn lossy_header(opts: &TtlOptions<'_>, tenant_header: Option<&str>) -> String {
    let mut out = format!(
        "# ── 有损声明：本文件是 OWL 本体的降级表达，逐项如下 ──\n\
         # · ActionType：未导出（{} 个）—— OWL / SHACL / SWRL 均无干净对应（规格 §5.5）\n\
         # · semantic：降级为 rdfs:comment（语义只能作注释，不可推理）\n\
         # · cardinality：只映射 Functional / InverseFunctional（1:1 / 1:N / N:1；多对多无对应）\n\
         # · 不含实例数据（ABox）；不含 SHACL 形状（nullable / 必填不导出）\n",
        opts.action_type_count
    );
    if let Some(line) = tenant_header {
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
        "一对多" => Cardinality::Functional,        // 1:N
        "多对一" => Cardinality::InverseFunctional, // N:1
        "唯一对应" => Cardinality::Both,            // 1:1
        _ => Cardinality::None,                     // 多对多 / 认不出
    }
}

/// 建模视图的 `1:1` / `1:N` / `N:1` / 其他 → 同一套基数枚举。
fn model_cardinality(cardinality: &str) -> Cardinality {
    match cardinality {
        "1:1" => Cardinality::Both,
        "1:N" => Cardinality::Functional,
        "N:1" => Cardinality::InverseFunctional,
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
) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:Class".to_string());
    let label = if e.display_name.trim().is_empty() { e.key.as_str() } else { e.display_name.as_str() };
    lines.push(format!("rdfs:label {}", zh(label)));
    lines.push(format!("ontok:key {}", plain(&e.key)));
    // REQ-119 · 实例 IRI 模板（AC-003）。
    lines.push(format!("ontok:instanceIri {}", plain(iri_template)));
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
    let (xsd, basis) = xsd_range(None, p.name, p.note);
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:label {}", zh(p.name)));
    lines.push(format!("rdfs:domain :{obj_local}"));
    lines.push(format!("rdfs:range {}", xsd.iri()));
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
) -> String {
    let column = effective_column(p);
    let subject = format!(":{class_local}__{}", local_model(column));
    let label = if p.name.trim().is_empty() { column } else { p.name.as_str() };
    let (xsd, basis) = xsd_range(Some(column), label, p.note.as_deref());
    let mut lines: Vec<String> = Vec::new();
    lines.push("a owl:DatatypeProperty".to_string());
    lines.push(format!("rdfs:domain :{class_local}"));
    lines.push(format!("rdfs:range {}", xsd.iri()));
    lines.push(format!("rdfs:label {}", zh(label)));
    lines.push(format!("ontok:column {}", plain(column)));
    lines.push(format!("ontok:nullable {}", bool_lit(p.nullable)));
    // 参照脚本：**只在 note != name 时**输出注释（770 条 note 里只有 8 条满足）。
    if let Some(n) = p.note.as_deref() {
        if !n.trim().is_empty() && n != label {
            lines.push(format!("rdfs:comment {}", zh(n)));
        }
    }
    if let Some(s) = p.sample.as_deref() {
        lines.push(format!("ontok:sample {}", plain(s)));
    }
    // REQ-119 · 租户键（AC-004）：只在建模视图、只在已签声明命中的属性上。
    if tenant_key {
        lines.push("ontok:tenantKey true".to_string());
    }
    if basis == RangeBasis::Unmapped {
        lines.push("ontok:rangeUnmapped true".to_string());
    }
    let _ = entity_key; // subject 的本地名已含类 key；参数留着让调用点可读
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
fn escape(s: &str) -> String {
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
        assert!(d.contains("owl:InverseFunctionalProperty"), "N:1 必须是 inverse-functional：{d}");
        assert!(d.contains("rdfs:subClassOf :root"), "{d}");
        assert!(d.contains("owl:disjointWith :customer"), "{d}");
        // REQ-119：`created_at` 是时间列 ⇒ dateTime，不再是 rdfs:Literal。
        assert!(d.contains("rdfs:range xsd:dateTime"), "{d}");
        assert!(!d.contains("rdfs:range rdfs:Literal"), "不许再一律 Literal：{d}");
        // 动作层不导出（只在文件头声明数量）—— 正文里根本没有动作内容。
        assert!(!doc.body().contains("ActionType "), "动作层不该进正文");
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
        let mut ns = NameSpace::new(&opts(), "demo".to_string());
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
        assert!(fwd.contains("owl:inverseOf :客户__订单"), "{fwd}");
        // 反向：domain/range 对调 + 基数特征 1:N ↔ N:1 对调。
        assert!(inv.contains("rdfs:domain :客户"), "{inv}");
        assert!(inv.contains("rdfs:range :订单"), "{inv}");
        assert!(inv.contains("owl:inverseOf :订单__客户"), "{inv}");
        assert!(inv.contains("a owl:FunctionalProperty"), "N:1 的反向是 1:N：{inv}");
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
