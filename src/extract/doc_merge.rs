//! **REQ-129 · 文档断言 → 本体提案** 的机械合并（`ontok doc merge`）。
//!
//! # 一句话
//!
//! REQ-116 把文档变成了**带逐字证据的断言**（`assertions.jsonl`）；本模块把它们
//! **机械地**并进一份提案（`proposal.json`），好让那条链路继续往下走：
//!
//! ```text
//! assertions.jsonl ──合并（本模块，纯函数）──▶ merged.json（与 proposal.json 同形状）
//!                                              └─ propose import → review decide → proposal apply
//! ```
//!
//! # 七条规则落在哪一行（demand §「合并规则」逐条对号）
//!
//! | # | 规则 | 落点 |
//! | --- | --- | --- |
//! | 1 | 只并 `alignable: true`；`future` / `example` 排除并计数 | [`exclusion`]，按模态分档 |
//! | 2 | doc 条目 band 最高 `medium`、`reviewed_by: null`；⛔ 不许 `strong` | [`cap_band`]（**唯一**的压档实现）+ [`DOC_BAND_CAP`] |
//! | 3 | 关系必须有显式 `maps_to`（child 列 + parent 列）；没有 ⇒ `doc_unmapped`；⛔ 不许猜表 | [`Index::resolve_relation`]：三个端点各自对提案校验 |
//! | 4 | 同实体 ⇒ 一条，`origin: "both"`，两侧证据都保留 | [`Slot`] + `payload.doc_evidence`（DB 的 `payload.evidence` 原样不动） |
//! | 5 | 冲突双保留 + `applicable: false` | [`Conflict`]（`doc_says` / `db_says` 都在） |
//! | 6 | 确定性：同一输入两次 ⇒ 逐字节相同 | 无时间戳 / 无随机 / 全程 `BTreeMap` + 稳定排序 + [`render_merged`] |
//! | 7 | 未签 ⇒ apply 时忽略 | 本模块只写 `reviewed_by: null`；忽略发生在 `domain::apply::unsigned_doc_only`（**同一判据**） |
//!
//! # 合并器**只做机械对齐**，不做语义猜测（demand §「关键裁定」）
//!
//! 「文档里的词 = 库里哪张表」只有两条机械入口：
//!
//! 1. **断言自带 `maps_to`** ⇒ 对提案校验存在性后并入（表在不在？列在不在？）；
//! 2. **名字/别名命中**（去首尾空白 + 折 ASCII 大小写后的**全等**）⇒ 并入；
//!    命中 0 个或 ≥2 个都算未映射（⛔ 不挑一个「最像的」）。
//!
//! ⛔ 没有第三条。本模块**没有任何**模糊匹配 / 编辑距离 / 词干 / 向量相似度 ——
//! 「『套餐』这词像 `package_templates` 就自动映射」正是 demand 点名不许做的事。
//!
//! # 校验面的唯一来源是**提案自己**
//!
//! 命令签名里没有 `--schema`（demand 钉死的三条必填参数）。这不是偷懒：
//! `proposal apply` 判定一条关系能不能建的判据就是**提案里的实体集合**
//! （`domain::apply::ApplyError::DanglingLink`）。若合并器拿另一份 `schema.json`
//! 校验，就会出现「合并器说表存在、apply 说两端不存在」的两套事实。
//!
//! ⇒ 表 = `add_entity_type.payload.key`；列 = 该实体 `payload.properties[].column`。
//!
//! # 已知限制（⛔ 不许写成「已支持」）
//!
//! - 🔴 **父端列校验不完整**：抽取侧把主键列从 `properties` 里剥掉了
//!   （`domain::apply` B-2 记着这条），所以 `customers.id` 这类父端列在提案里**查不到**。
//!   本模块只接受「列在 properties 里」或「列名恰好是 `id`」（后者记
//!   [`MergeStats::parent_key_id_convention`]，**可见**）；其它一律 `doc_unmapped`。
//!   严格校验需要 `schema.json`，本轮命令签名里没有它 —— 已登记为限制。
//! - **不许新增实体**：doc 概念映射不到提案里的实体 ⇒ `doc_unmapped`。
//!   理由与上一条同源：凭空造一张表，apply 就会写进一个没有任何列的本体类。
//!   所以 `origin: "doc"` 的**条目只可能是关系**（`add_link_type`）。
//! - **名字命中是精确匹配**：`客户` 命中 `customers` 只因它的 `display_name` 恰好是「客户」；
//!   文档写「顾客」而 DB 名是「客户」、且 DB 侧没登记这个别名 ⇒ 不算命中
//!   （那需要人签，见 demand §「关键裁定」）。
//! - **doc 的谓词不覆盖 DB 的 `semantic`**：`both` 条目里 DB 的边原样保留（`belongs_to`），
//!   doc 谓词留在 `doc_evidence[].predicate` 里。改 `semantic` 是语义决定，
//!   不该由一次机械合并顺手做掉。
//! - **doc 边不标 `join_verified`**：文档说的是「业务上谁引用谁」，不是「值域重叠实测过」。
//!   三态里没有「文档断言」这一态，所以**不写**（比写 `false` 诚实：`false` 在
//!   `r2rml generate` 里是「永不生成 join」，那是个比「未实测」更强的断言）。
//! - **doc 条目没有置信度分数**（`confidence` 块）：它是 ADR-005 的语义提案形状，
//!   需要 `method` + `signals`；文档断言没有这两种东西。所以 doc 条目的置信度只有
//!   `band` 一栏 —— ⛔ 不编一个 0.5 分出来。
//! - **合并器会重置已有签名**：`reviewed_by` 恒写 `null`（规则 2 是绝对的）。
//!   重跑合并 = 重新生成提案 ⇒ 之前的签字作废，计数在 [`MergeStats::reviewed_by_reset`]。
//!
//! # 本模块零 I/O
//!
//! 输入是字符串 + `serde_json::Value`，输出是字符串 + 统计；读写文件由 `cli` 负责。
//! 所以全部 AC 都能用内存夹具跑（含 56 行真实 fixture）。

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::extract::doc_ingest::{Assertion, ConceptAssertion, Modality, RelationAssertion};
use crate::extract::sample::sha256_hex;

// ──────────────────────────────────────────────────────────
// 常量（改语义 = 改这里）
// ──────────────────────────────────────────────────────────

/// 合并器的**格式版本**（进 `doc_merge` 摘要块，下游据此判断能不能读）。
pub const MERGE_VERSION: u32 = 1;

/// ⭐ **规则 2 的唯一阈值**：doc 来源条目允许的最高档位。
///
/// 变异自证（AC-003 的「把 doc 条目设成 strong」）：把它改成 `"strong"`，
/// [`tests::doc_items_never_carry_a_band_stronger_than_medium`] 立刻变红。
pub const DOC_BAND_CAP: &str = "medium";

/// 条目来源（顶层 `origin`，导入时搬进 `payload`）。
pub const ORIGIN_DOC: &str = "doc";
/// 条目来源：doc 与 DB 两侧都指向同一实体 / 同一条边。
pub const ORIGIN_BOTH: &str = "both";

/// doc 侧的逐条证据放在 `payload` 的哪个键下（⛔ 不覆盖 DB 的 `payload.evidence`）。
pub const DOC_EVIDENCE_KEY: &str = "doc_evidence";

/// 统计里最多留几条样例（与 REQ-116/118 的 8 条同口径：够人复核，又不刷屏）。
pub const MAX_SAMPLES: usize = 8;

/// 父端列的唯一「约定」值：抽取侧剥掉主键后，只有 `id` 这个名字还能被接受。
///
/// ⚠️ 收它是因为**提案里根本没有主键列**（见模块头的已知限制），不是因为它「像主键」。
pub const PK_CONVENTION: &str = "id";

// ──────────────────────────────────────────────────────────
// 断言的**对齐键**（REQ-116 的产出方不产它，由人对齐步骤补）
// ──────────────────────────────────────────────────────────

/// 一端：哪张表的哪一列。
///
/// 形状是**对象**而不是 `"table.column"` 字符串：列名里没有点，但「谁猜谁」这件事
/// 必须写在两个具名字段里，读的人不该去 split 一个字符串。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Endpoint {
    pub table: String,
    pub column: String,
}

/// 断言上的显式映射（demand 规则 3 的「child 列 + parent 列」）。
///
/// | 断言 | 必填 | 含义 |
/// | --- | --- | --- |
/// | concept | `table` | 这个概念是哪张表（例：`{"table":"customers"}`） |
/// | relation | `child` + `parent` | 这条边由哪一列实现：`child` 持有外键列，`parent` 被指向 |
///
/// ⛔ 形状不对 ⇒ `doc_unmapped`（`bad_maps_to`），**不**退化成「按名字猜」。
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MapsTo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<Endpoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Endpoint>,
}

/// 读一条断言上的 `maps_to`。形状不对 ⇒ `Err(人话)`（调用方把它记成 `bad_maps_to`）。
pub fn read_maps_to(v: &Value) -> std::result::Result<MapsTo, String> {
    if !v.is_object() {
        return Err(format!(
            "maps_to 必须是对象（concept: {{\"table\":\"…\"}}；relation: \
             {{\"child\":{{\"table\":\"…\",\"column\":\"…\"}},\"parent\":{{…}}}}），实际是 {}",
            json_kind(v)
        ));
    }
    serde_json::from_value(v.clone()).map_err(|e| {
        format!(
            "maps_to 形状不对：{e}（要 {{table}} 或 {{child:{{table,column}},parent:{{table,column}}}}）"
        )
    })
}

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ──────────────────────────────────────────────────────────
// 一条 doc 证据（`payload.doc_evidence[]` 的元素）
// ──────────────────────────────────────────────────────────

/// doc 侧的一条证据：**原文片段 + 位置 + 这条断言说的是什么**。
///
/// 它是 `assertions.jsonl` 那一行的**忠实摘要**（`quote` 一个字都不改），
/// 加上这条断言靠什么并进来的（`maps_to`）。人复核时不需要回头翻 JSONL。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DocClaim {
    /// `concept` / `relation`
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<String>,
    pub modality: String,
    /// 这条断言是**靠哪条显式映射**并进来的（`None` = 靠名字/别名命中）。
    ///
    /// ⚠️ 它保留原样，供 AC-004 的「`maps_to` 的表/列都在 schema 里」复核；
    /// 其中每个端点本模块都校验过（父端列的口径见模块头的已知限制）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maps_to: Option<MapsTo>,
    pub file: String,
    pub chunk_id: String,
    pub quote: String,
    pub span: [usize; 2],
}

impl DocClaim {
    fn concept(c: &ConceptAssertion, maps: Option<&MapsTo>) -> Self {
        Self {
            kind: "concept".into(),
            name: Some(c.name.clone()),
            aliases: c.aliases.clone(),
            description: c.description.clone(),
            subject: None,
            predicate: None,
            object: None,
            cardinality: None,
            modality: c.modality.as_str().to_string(),
            maps_to: maps.cloned(),
            file: c.evidence.file.clone(),
            chunk_id: c.evidence.chunk_id.clone(),
            quote: c.evidence.quote.clone(),
            span: c.evidence.span,
        }
    }

    fn relation(r: &RelationAssertion, maps: &MapsTo) -> Self {
        Self {
            kind: "relation".into(),
            name: None,
            aliases: Vec::new(),
            description: None,
            subject: Some(r.subject.clone()),
            predicate: Some(r.predicate.clone()),
            object: Some(r.object.clone()),
            cardinality: r.cardinality.clone(),
            modality: r.modality.as_str().to_string(),
            maps_to: Some(maps.clone()),
            file: r.evidence.file.clone(),
            chunk_id: r.evidence.chunk_id.clone(),
            quote: r.evidence.quote.clone(),
            span: r.evidence.span,
        }
    }

    /// 排序键：**文件 → 字节起点 → 引文**（人按原文顺序读，不按 JSONL 的行号）。
    fn order_key(&self) -> (&str, usize, usize, &str) {
        (&self.file, self.span[0], self.span[1], &self.quote)
    }
}

// ──────────────────────────────────────────────────────────
// 冲突（demand 规则 5）
// ──────────────────────────────────────────────────────────

/// 一条**基数冲突**：doc 的说法与 DB 推断不一致。
///
/// 两侧都留（`doc_says` / `db_says`），条目 `applicable: false` ——
/// 挑一边是**人的决定**，不是合并器的。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Conflict {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// doc 写的基数（例：`1:1`）。
    pub doc_says: String,
    /// DB 推断的基数（例：`N:1`，来自提案里那条边的 `payload.cardinality`）。
    pub db_says: String,
    pub file: String,
    pub chunk_id: String,
    pub quote: String,
    pub span: [usize; 2],
}

// ──────────────────────────────────────────────────────────
// 统计（AC-008 的五项 + 明细）
// ──────────────────────────────────────────────────────────

/// 一轮合并的统计。
///
/// **五项**（AC-008 的判据）是前五个字段；[`MergeStats::close`] 是它们的闭合式：
/// `merged + doc_unmapped + conflicts + excluded_not_alignable == assertions_read`。
/// 差一条就说明有一条断言既没并入、也没被任何一档记账 —— 那正是「静默丢弃」的形状。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MergeStats {
    // ── AC-008 的五项 ──────────────────────────────
    /// 并入：真正落进某条目的 alignable 断言数（含被去重的重复断言）。
    pub merged: usize,
    /// 排除：`future` / `example`（不可对齐）的断言数。
    pub excluded_not_alignable: usize,
    /// 未映射：无法机械对齐的 alignable 断言数（按 `unmapped_reasons` 分档）。
    pub doc_unmapped: usize,
    /// 冲突：基数不一致的断言数（每条都在 `conflicts` 里留两侧说法）。
    pub conflicts: usize,
    /// 合并为 `both` 的**条目**数（规则 4 的产物）。
    pub merged_both: usize,
    // ── 明细 ──────────────────────────────────────
    /// 新建的 doc-only 条目数（`origin: "doc"`，只可能是关系）。
    pub merged_doc_only: usize,
    pub assertions_read: usize,
    pub assertions_concept: usize,
    pub assertions_relation: usize,
    pub excluded_future: usize,
    pub excluded_example: usize,
    /// `alignable` 与 `modality` 自相矛盾的条数（手改过的文件）：一律**按模态**排除。
    pub excluded_modality_conflict: usize,
    /// 重复断言（同一实体/同一条边的重复行）—— 逐条去重后只留一条证据。
    pub duplicate_claims: usize,
    pub items_in: usize,
    pub items_out: usize,
    pub items_added: usize,
    /// 被 doc 侧触碰过的既有条目数（origin → both）。
    pub items_touched: usize,
    /// 被压档的条目数（输入档位比 [`DOC_BAND_CAP`] 更强，或读不懂）。
    pub band_capped_strong: usize,
    /// 输入里已带签名、被合并器**重置为 null** 的条目数（规则 2：doc 条目必须未签）。
    pub reviewed_by_reset: usize,
    /// 父端列只靠 [`PK_CONVENTION`] 通过的断言数（口径见模块头已知限制）。
    pub parent_key_id_convention: usize,
    /// 关系断言没写基数、用了 `N:1` 默认值的条数。
    pub cardinality_defaulted: usize,
    /// 未映射按原因分档（键是稳定码，`BTreeMap` ⇒ 输出顺序确定）。
    pub unmapped_reasons: BTreeMap<String, usize>,
    pub unmapped_samples: Vec<String>,
    pub conflict_samples: Vec<Conflict>,
    pub assertions_sha256: String,
    pub proposal_sha256: String,
}

impl MergeStats {
    /// 闭合：每一条读进来的断言都恰好落进一档。
    pub fn close(&self) -> bool {
        self.merged + self.doc_unmapped + self.conflicts + self.excluded_not_alignable
            == self.assertions_read
    }

    fn note_unmapped(&mut self, code: &str, detail: String) {
        self.doc_unmapped += 1;
        *self.unmapped_reasons.entry(code.to_string()).or_insert(0) += 1;
        if self.unmapped_samples.len() < MAX_SAMPLES {
            self.unmapped_samples.push(format!("{code}：{detail}"));
        }
    }

    /// 进 `merged.json` 顶层 `doc_merge` 块的那一部分（**只含内容属性**，无时间戳）。
    pub fn summary_json(&self) -> Value {
        serde_json::json!({
            "merge_version": MERGE_VERSION,
            "assertions_sha256": self.assertions_sha256,
            "proposal_sha256": self.proposal_sha256,
            "assertions_read": self.assertions_read,
            "merged": self.merged,
            "excluded_not_alignable": self.excluded_not_alignable,
            "doc_unmapped": self.doc_unmapped,
            "conflicts": self.conflicts,
            "merged_both": self.merged_both,
            "merged_doc_only": self.merged_doc_only,
            "band_cap": DOC_BAND_CAP,
            "unsigned_doc_items_are_ignored_on_apply": true,
        })
    }
}

/// 合并的产物。
#[derive(Debug, Clone)]
pub struct MergeOutcome {
    /// 与 `proposal.json` 同形状的文档（可直接 `propose import`）。
    pub document: Value,
    /// 落盘字节（pretty JSON + 末尾换行）—— AC-001 的逐字节对拍看它。
    pub rendered: String,
    pub stats: MergeStats,
}

/// `--stats` 落盘的字节（pretty JSON + 末尾换行）。
pub fn render_stats(stats: &MergeStats) -> Result<String> {
    let mut text = serde_json::to_string_pretty(stats).context("统计序列化失败")?;
    text.push('\n');
    Ok(text)
}

// ──────────────────────────────────────────────────────────
// 提案索引（**校验面的唯一来源**）
// ──────────────────────────────────────────────────────────

/// 提案里一个实体（表）的可用信息。
#[derive(Debug, Clone, Default)]
struct Entity {
    columns: BTreeSet<String>,
}

/// 待写的槽位：既有条目（按下标）或待新建的 doc 条目（按关系键）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Existing(usize),
    Pending(String),
}

/// 关系的定位结果（child 端 = 持有外键列的那张表）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct LinkTarget {
    table: String,
    column: String,
    parent_table: String,
    parent_column: String,
}

impl LinkTarget {
    /// 与 `ontok-propose` / `fk` 同一套键法：`{child}_{column}__{parent}`。
    fn key(&self) -> String {
        format!("{}_{}__{}", self.table, self.column, self.parent_table)
    }
}

/// 提案的只读索引。
#[derive(Debug, Clone, Default)]
struct Index {
    entities: BTreeMap<String, Entity>,
    entity_items: BTreeMap<String, Slot>,
    /// 归一化名字 → 实体 key（**可能多个** ⇒ 歧义 ⇒ 未映射）。
    by_name: BTreeMap<String, BTreeSet<String>>,
    /// `(child 表, child 列, parent 表)` → 槽位。
    links: BTreeMap<(String, String, String), Slot>,
    /// 已占用的 `payload.key`（新条目撞键 ⇒ 未映射，⛔ 不自动改名）。
    keys: BTreeSet<String>,
}

/// 名字归一化：**只**去首尾空白 + 折 ASCII 大小写。
///
/// ⛔ 不做词干 / 全半角 / 标点归一：那会从「精确命中」滑向「像」，
/// 而「像」正是 demand §「关键裁定」禁止的。
fn norm_name(s: &str) -> String {
    s.trim().to_lowercase()
}

impl Index {
    fn build(items: &[Value]) -> Result<Self> {
        let mut ix = Index::default();
        for (i, it) in items.iter().enumerate() {
            let kind = it.get("kind").and_then(Value::as_str).unwrap_or("");
            let payload = it.get("payload").cloned().unwrap_or(Value::Null);
            if let Some(k) = payload.get("key").and_then(Value::as_str) {
                ix.keys.insert(k.to_string());
            }
            match kind {
                "add_entity_type" => {
                    let key = payload
                        .get("key")
                        .and_then(Value::as_str)
                        .or_else(|| it.get("target").and_then(Value::as_str))
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "提案第 {} 条 add_entity_type 既没有 payload.key 也没有 target —— \
                                 没法把它当成一张表（⛔ 不猜）",
                                i + 1
                            )
                        })?
                        .to_string();
                    let display_name = payload
                        .get("display_name")
                        .and_then(Value::as_str)
                        .unwrap_or(&key);
                    let aliases: Vec<String> = payload
                        .get("aliases")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let columns: BTreeSet<String> = payload
                        .get("properties")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|p| p.get("column").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    for n in std::iter::once(key.as_str())
                        .chain(std::iter::once(display_name))
                        .chain(aliases.iter().map(String::as_str))
                    {
                        let n = norm_name(n);
                        if !n.is_empty() {
                            ix.by_name.entry(n).or_default().insert(key.clone());
                        }
                    }
                    ix.entity_items.insert(key.clone(), Slot::Existing(i));
                    ix.entities.insert(key, Entity { columns });
                }
                "add_link_type" => {
                    let from = payload.get("from_type").and_then(Value::as_str);
                    let to = payload.get("to_type").and_then(Value::as_str);
                    let col = payload
                        .get("evidence")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(|e| e.get("column"))
                        .and_then(Value::as_str);
                    if let (Some(f), Some(t), Some(c)) = (from, to, col) {
                        ix.links
                            .entry((f.to_string(), c.to_string(), t.to_string()))
                            .or_insert(Slot::Existing(i));
                    }
                }
                _ => {}
            }
        }
        Ok(ix)
    }

    /// 概念 → 一张表（`Ok(key)`）或未映射原因（`Err`）。
    fn resolve_concept(
        &self,
        c: &ConceptAssertion,
        maps: Option<&MapsTo>,
    ) -> std::result::Result<String, Unmapped> {
        if let Some(m) = maps {
            if m.child.is_some() || m.parent.is_some() {
                return Err(Unmapped::new(
                    "bad_maps_to",
                    format!(
                        "concept「{}」的 maps_to 只认 `table`，不认 child/parent",
                        c.name
                    ),
                ));
            }
            match m.table.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(t) if self.entities.contains_key(t) => return Ok(t.to_string()),
                Some(t) => {
                    return Err(Unmapped::new(
                        "table_not_in_proposal",
                        format!(
                            "concept「{}」的 maps_to.table = {t:?} 不在提案的实体里",
                            c.name
                        ),
                    ))
                }
                None => {
                    return Err(Unmapped::new(
                        "bad_maps_to",
                        format!("concept「{}」的 maps_to 没有 table", c.name),
                    ))
                }
            }
        }
        // 名字 / 别名命中（精确）
        let mut hits: BTreeSet<String> = BTreeSet::new();
        for n in std::iter::once(c.name.as_str()).chain(c.aliases.iter().map(String::as_str)) {
            if let Some(ks) = self.by_name.get(&norm_name(n)) {
                hits.extend(ks.iter().cloned());
            }
        }
        match hits.len() {
            0 => Err(Unmapped::new(
                "no_maps_to_and_no_name_match",
                format!(
                    "concept「{}」（别名 {}）既没有 maps_to，也没有精确命中的实体",
                    c.name,
                    if c.aliases.is_empty() {
                        "—".to_string()
                    } else {
                        c.aliases.join("/")
                    }
                ),
            )),
            1 => Ok(hits.into_iter().next().unwrap_or_default()),
            _ => Err(Unmapped::new(
                "ambiguous_name",
                format!(
                    "concept「{}」同时命中 {} 个实体（{}）—— ⛔ 不挑一个最像的",
                    c.name,
                    hits.len(),
                    hits.iter().cloned().collect::<Vec<_>>().join(",")
                ),
            )),
        }
    }

    /// 关系 → 一条边的实现（`Ok`）或未映射原因（`Err`）。
    ///
    /// **三个端点逐一对提案校验**（规则 3）：child 表、child 列、parent 表。
    /// parent 列的口径见模块头的已知限制（主键列被抽取侧剥掉了）。
    fn resolve_relation(
        &self,
        r: &RelationAssertion,
        maps: Option<&MapsTo>,
        stats: &mut MergeStats,
    ) -> std::result::Result<LinkTarget, Unmapped> {
        let Some(m) = maps else {
            return Err(Unmapped::new(
                "no_maps_to",
                format!(
                    "{} -[{}]-> {}：关系没有显式 maps_to（child 列 + parent 列）\
                     —— ⛔ 不按名字猜表",
                    r.subject, r.predicate, r.object
                ),
            ));
        };
        if m.table.is_some() {
            return Err(Unmapped::new(
                "bad_maps_to",
                format!(
                    "{} -[{}]-> {}：关系的 maps_to 只认 child/parent，不认 table",
                    r.subject, r.predicate, r.object
                ),
            ));
        }
        let (Some(child), Some(parent)) = (m.child.as_ref(), m.parent.as_ref()) else {
            return Err(Unmapped::new(
                "bad_maps_to",
                format!(
                    "{} -[{}]-> {}：maps_to 必须同时给 child 与 parent（各含 table+column）",
                    r.subject, r.predicate, r.object
                ),
            ));
        };
        if child.table.trim().is_empty()
            || child.column.trim().is_empty()
            || parent.table.trim().is_empty()
            || parent.column.trim().is_empty()
        {
            return Err(Unmapped::new(
                "bad_maps_to",
                format!(
                    "{} -[{}]-> {}：maps_to 里 table/column 有空值",
                    r.subject, r.predicate, r.object
                ),
            ));
        }
        for (role, ep) in [("child", child), ("parent", parent)] {
            if !self.entities.contains_key(&ep.table) {
                return Err(Unmapped::new(
                    "table_not_in_proposal",
                    format!(
                        "{} -[{}]-> {}：maps_to.{role}.table = {:?} 不在提案的实体里",
                        r.subject, r.predicate, r.object, ep.table
                    ),
                ));
            }
        }
        let child_entity = &self.entities[&child.table];
        if !child_entity.columns.contains(&child.column) {
            return Err(Unmapped::new(
                "column_not_in_proposal",
                format!(
                    "{} -[{}]-> {}：maps_to.child = {}.{} —— {} 的属性里没有列 {:?}",
                    r.subject,
                    r.predicate,
                    r.object,
                    child.table,
                    child.column,
                    child.table,
                    child.column
                ),
            ));
        }
        let parent_entity = &self.entities[&parent.table];
        if !parent_entity.columns.contains(&parent.column) {
            if parent.column == PK_CONVENTION {
                stats.parent_key_id_convention += 1;
            } else {
                return Err(Unmapped::new(
                    "parent_column_not_in_proposal",
                    format!(
                        "{} -[{}]-> {}：maps_to.parent = {}.{} —— {} 的属性里没有列 {:?}\
                         （只接受属性列，或约定的主键名 {:?}）",
                        r.subject,
                        r.predicate,
                        r.object,
                        parent.table,
                        parent.column,
                        parent.table,
                        parent.column,
                        PK_CONVENTION
                    ),
                ));
            }
        }
        if child.table == parent.table {
            return Err(Unmapped::new(
                "self_link",
                format!(
                    "{} -[{}]-> {}：child 与 parent 是同一张表（{}）—— 自引用边不在本需求范围",
                    r.subject, r.predicate, r.object, child.table
                ),
            ));
        }
        Ok(LinkTarget {
            table: child.table.clone(),
            column: child.column.clone(),
            parent_table: parent.table.clone(),
            parent_column: parent.column.clone(),
        })
    }
}

/// 未映射的原因（稳定码 + 人读细节）。
#[derive(Debug, Clone)]
struct Unmapped {
    code: &'static str,
    detail: String,
}

impl Unmapped {
    fn new(code: &'static str, detail: String) -> Self {
        Self { code, detail }
    }
}

// ──────────────────────────────────────────────────────────
// 档位（规则 2 的**唯一**实现）
// ──────────────────────────────────────────────────────────

/// 压档：**任何**比 [`DOC_BAND_CAP`] 强的档位一律压到 `medium`。
///
/// 连带把 `high` / `middle`（`domain::confidence` 的词）与 `inherit`（REQ-120 的词）
/// 一起压 —— 三套档位词并存是本仓的历史事实，压档判据只该看「比 medium 强吗」。
/// 认不出的值同样压到 `medium`（fail-safe：⛔ 绝不因为「不认识」而放行更强的档）。
pub fn cap_band(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" | "rejected" => "low",
        _ => DOC_BAND_CAP,
    }
}

/// 两档取低（`low` < `medium`）。
fn band_min(a: &'static str, b: &'static str) -> &'static str {
    if a == "low" || b == "low" {
        "low"
    } else {
        DOC_BAND_CAP
    }
}

/// 两档取高（doc 侧多条主张：有显式映射就够 medium）。
fn band_max(a: &'static str, b: &'static str) -> &'static str {
    if a == "low" && b == "low" {
        "low"
    } else {
        DOC_BAND_CAP
    }
}

/// 条目自己的档位（顶层 `band` 或 `confidence.band`，两边都读；都没有 ⇒ `None`）。
fn item_band(item: &Value) -> Option<String> {
    if let Some(b) = item.get("band").and_then(Value::as_str) {
        return Some(b.to_string());
    }
    item.get("confidence")
        .and_then(|c| c.get("band"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 这个档位是「比 medium 强 / 读不懂」吗（用于统计：规则 2 真的起作用了没有）。
fn is_stronger_than_cap(raw: &str) -> bool {
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "low" | "rejected" | "medium" | "middle"
    )
}

// ──────────────────────────────────────────────────────────
// 规则 1（不可对齐）
// ──────────────────────────────────────────────────────────

/// 一条被排除的断言：稳定码 + 是否与 `alignable` 声明自相矛盾。
#[derive(Debug, Clone, Copy)]
struct Exclusion {
    code: &'static str,
    contradiction: bool,
}

/// **规则 1 的唯一判据**：模态与 `alignable` 任一说不 ⇒ 不并。
///
/// 模态优先：`future` / `example` 是语义事实（文档说的），`alignable` 是派生标记。
/// 两者矛盾时按模态排除，并由 [`MergeStats::excluded_modality_conflict`] 记一笔。
fn exclusion(a: &Assertion) -> Option<Exclusion> {
    let declared = match a {
        Assertion::Concept(c) => c.alignable,
        Assertion::Relation(r) => r.alignable,
    };
    match a.modality() {
        Modality::Future => Some(Exclusion {
            code: "future",
            contradiction: declared,
        }),
        Modality::Example => Some(Exclusion {
            code: "example",
            contradiction: declared,
        }),
        _ if !declared => Some(Exclusion {
            code: "alignable_false",
            contradiction: true,
        }),
        _ => None,
    }
}

/// 记一次排除（按模态分档 + 记矛盾）。
fn count_excluded(stats: &mut MergeStats, ex: Exclusion) {
    stats.excluded_not_alignable += 1;
    match ex.code {
        "future" => stats.excluded_future += 1,
        "example" => stats.excluded_example += 1,
        _ => {}
    }
    if ex.contradiction {
        stats.excluded_modality_conflict += 1;
        if stats.unmapped_samples.len() < MAX_SAMPLES {
            stats.unmapped_samples.push(format!(
                "{}：模态与 alignable 自相矛盾（按模态排除，不按标记）",
                ex.code
            ));
        }
    }
}

/// 断言的短标签（样例里点名用）。
fn assertion_label(a: &Assertion) -> String {
    match a {
        Assertion::Concept(c) => format!("concept「{}」", c.name),
        Assertion::Relation(r) => format!("{} -[{}]-> {}", r.subject, r.predicate, r.object),
    }
}

// ──────────────────────────────────────────────────────────
// 合并（唯一实现）
// ──────────────────────────────────────────────────────────

/// 把 `assertions.jsonl` 与一份提案合并成**同形状**的新提案。
pub fn merge(assertions_jsonl: &str, proposal: &Value) -> Result<MergeOutcome> {
    let items_in = proposal
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "提案顶层没有 `items` 数组 —— 这不是一份提案（⛔ 不猜，也不产一份空合并）"
            )
        })?
        .clone();
    let proposal_sha256 = sha256_hex(
        serde_json::to_string(proposal)
            .context("提案序列化失败")?
            .as_bytes(),
    );
    let index = Index::build(&items_in)?;

    let mut items: Vec<Value> = items_in.clone();
    let mut stats = MergeStats {
        items_in: items.len(),
        assertions_sha256: sha256_hex(assertions_jsonl.as_bytes()),
        proposal_sha256,
        ..Default::default()
    };

    // 三条按槽位攒着的写回队列（循环里不直接改 item：顺序与去重都要先看清楚）。
    let mut claims: BTreeMap<Slot, Vec<DocClaim>> = BTreeMap::new();
    let mut conflicts: BTreeMap<Slot, Vec<Conflict>> = BTreeMap::new();
    // 每个槽位的 doc 侧档位（多条主张取**高**：显式映射 medium / 名字命中 low）。
    let mut band_of_slot: BTreeMap<Slot, &'static str> = BTreeMap::new();
    // 待新建的 doc-only 条目：关系键 → item（键排序 ⇒ 追加顺序确定）。
    let mut pending: BTreeMap<String, Value> = BTreeMap::new();
    // 被 doc 触碰过 / 由 doc 新建的槽位（origin = both / doc）。
    let mut touched: BTreeSet<Slot> = BTreeSet::new();
    let mut doc_slots: BTreeSet<Slot> = BTreeSet::new();

    for (ln, raw) in assertions_jsonl.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        stats.assertions_read += 1;
        let v: Value = serde_json::from_str(line)
            .with_context(|| format!("assertions JSONL 第 {} 行不是合法 JSON", ln + 1))?;
        let assertion: Assertion = serde_json::from_value(v.clone()).with_context(|| {
            format!(
                "assertions JSONL 第 {} 行的断言形状不对\
                 （REQ-116 的契约：kind/name|subject…/evidence）",
                ln + 1
            )
        })?;
        match assertion {
            Assertion::Concept(_) => stats.assertions_concept += 1,
            Assertion::Relation(_) => stats.assertions_relation += 1,
        }

        // ── 规则 1：不可对齐的连 maps_to 都不用看 ──
        if let Some(ex) = exclusion(&assertion) {
            count_excluded(&mut stats, ex);
            continue;
        }

        // ── 对齐键：形状错 ⇒ 未映射（fail-closed，⛔ 不退回按名字猜）──
        let maps: Option<MapsTo> = match v.get("maps_to") {
            None | Some(Value::Null) => None,
            Some(m) => match read_maps_to(m) {
                Ok(m) => Some(m),
                Err(why) => {
                    stats.note_unmapped(
                        "bad_maps_to",
                        format!("{}：{why}", assertion_label(&assertion)),
                    );
                    continue;
                }
            },
        };

        match &assertion {
            Assertion::Concept(c) => merge_concept(
                &index,
                c,
                maps.as_ref(),
                &mut stats,
                &mut claims,
                &mut band_of_slot,
                &mut touched,
            )?,
            Assertion::Relation(r) => merge_relation(
                &index,
                &items,
                r,
                maps.as_ref(),
                &mut stats,
                &mut claims,
                &mut conflicts,
                &mut band_of_slot,
                &mut touched,
                &mut doc_slots,
                &mut pending,
            )?,
        }
    }

    // ── 追加 doc-only 条目（键排序 ⇒ 两次运行同一顺序）──
    for (key, item) in &pending {
        let idx = items.len();
        items.push(item.clone());
        stats.items_added += 1;
        let from = Slot::Pending(key.clone());
        let to = Slot::Existing(idx);
        if let Some(v) = claims.remove(&from) {
            claims.insert(to.clone(), v);
        }
        if let Some(v) = conflicts.remove(&from) {
            conflicts.insert(to.clone(), v);
        }
        if let Some(b) = band_of_slot.remove(&from) {
            band_of_slot.insert(to.clone(), b);
        }
        if touched.remove(&from) {
            touched.insert(to.clone());
        }
        if doc_slots.remove(&from) {
            doc_slots.insert(to.clone());
        }
    }

    // ── 写回：origin / band / reviewed_by / applicable / doc_evidence / conflicts ──
    let mut write_slots: BTreeSet<Slot> = touched.clone();
    write_slots.extend(doc_slots.iter().cloned());
    for slot in write_slots {
        let Slot::Existing(idx) = slot else {
            bail!("内部错误：doc 条目槽位没有落成下标（{slot:?}）");
        };
        let item = items
            .get_mut(idx)
            .ok_or_else(|| anyhow::anyhow!("内部错误：条目下标越界 {idx}"))?;
        let prev_origin = item.get("origin").and_then(Value::as_str).unwrap_or("");
        let origin = if doc_slots.contains(&Slot::Existing(idx)) || prev_origin == ORIGIN_DOC {
            ORIGIN_DOC
        } else {
            ORIGIN_BOTH
        };
        let db_band = item_band(item);
        let doc_band = band_of_slot
            .get(&Slot::Existing(idx))
            .copied()
            .unwrap_or(DOC_BAND_CAP);
        let merged_band = match db_band.as_deref() {
            Some(raw) => {
                if is_stronger_than_cap(raw) {
                    stats.band_capped_strong += 1;
                }
                band_min(cap_band(raw), doc_band)
            }
            None => doc_band,
        };
        set_field(item, "origin", Value::from(origin));
        set_field(item, "band", Value::from(merged_band));
        // 规则 2：doc 条目必须**未签**。已有签名一律重置（并计数 —— 静默丢签名是事故）。
        if item.get("reviewed_by").is_some_and(|v| !v.is_null()) {
            stats.reviewed_by_reset += 1;
        }
        set_field(item, "reviewed_by", Value::Null);
        set_field(item, "applicable", Value::Bool(true));

        // doc 证据：已在文件里的（本模块上一轮的产物）不算「重复断言」，只做去重合并。
        let already = read_existing_claims(item);
        let mut fresh = claims.remove(&Slot::Existing(idx)).unwrap_or_default();
        // 已在文件里的证据（本模块上一轮的产物）先摘掉 —— 它们不是「重复断言」。
        fresh.retain(|c| !already.contains(c));
        let n_fresh = fresh.len();
        fresh.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
        fresh.dedup();
        stats.duplicate_claims += n_fresh.saturating_sub(fresh.len());
        let mut all = already;
        all.extend(fresh);
        all.sort_by(|a, b| a.order_key().cmp(&b.order_key()));
        all.dedup();
        let payload = payload_mut(item)?;
        payload.insert(
            DOC_EVIDENCE_KEY.to_string(),
            serde_json::to_value(&all).context("doc 证据序列化失败")?,
        );

        // 冲突：双保留 + applicable:false（规则 5 / AC-006）。
        let mut cfs = conflicts.remove(&Slot::Existing(idx)).unwrap_or_default();
        cfs.sort();
        cfs.dedup();
        if !cfs.is_empty() {
            stats.conflicts += cfs.len();
            for c in &cfs {
                if stats.conflict_samples.len() < MAX_SAMPLES {
                    stats.conflict_samples.push(c.clone());
                }
            }
            set_field(item, "applicable", Value::Bool(false));
            set_field(
                item,
                "conflicts",
                serde_json::to_value(&cfs).context("冲突序列化失败")?,
            );
        }
    }

    // ── 五项的收尾计数 ──
    stats.items_out = items.len();
    stats.merged_both = items
        .iter()
        .filter(|it| it.get("origin").and_then(Value::as_str) == Some(ORIGIN_BOTH))
        .count();
    stats.merged_doc_only = items
        .iter()
        .filter(|it| it.get("origin").and_then(Value::as_str) == Some(ORIGIN_DOC))
        .count();
    stats.items_touched = stats.merged_both;

    // 闭合：每条断言恰好落进一档（静默丢弃在这里就暴露了）。
    if !stats.close() {
        bail!(
            "内部错误：统计不闭合（读 {} ≠ 并入 {} + 未映射 {} + 冲突 {} + 排除 {}）\
             —— 有一条断言既没并入、也没被任何一档记账，那是静默丢弃",
            stats.assertions_read,
            stats.merged,
            stats.doc_unmapped,
            stats.conflicts,
            stats.excluded_not_alignable
        );
    }

    let rendered = render_merged(proposal, &items, &stats)?;
    let document: Value = serde_json::from_str(&rendered).context("合并结果读不回来")?;
    Ok(MergeOutcome {
        document,
        rendered,
        stats,
    })
}

/// 概念 → 条目（`both`）或未映射。
fn merge_concept(
    index: &Index,
    c: &ConceptAssertion,
    maps: Option<&MapsTo>,
    stats: &mut MergeStats,
    claims: &mut BTreeMap<Slot, Vec<DocClaim>>,
    band_of_slot: &mut BTreeMap<Slot, &'static str>,
    touched: &mut BTreeSet<Slot>,
) -> Result<()> {
    match index.resolve_concept(c, maps) {
        Ok(key) => {
            let slot = index
                .entity_items
                .get(&key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("内部错误：实体 {key} 没有对应的条目"))?;
            // 显式映射 ⇒ medium；只靠名字/别名命中 ⇒ low（对齐更弱，人更该看一眼）。
            let doc_band: &'static str = if maps.is_some() { DOC_BAND_CAP } else { "low" };
            claims
                .entry(slot.clone())
                .or_default()
                .push(DocClaim::concept(c, maps));
            band_of_slot
                .entry(slot.clone())
                .and_modify(|b| *b = band_max(*b, doc_band))
                .or_insert(doc_band);
            touched.insert(slot);
            stats.merged += 1;
        }
        Err(u) => stats.note_unmapped(u.code, u.detail),
    }
    Ok(())
}

/// 关系 → 已有边（`both`）/ 新边（`doc`）/ 冲突 / 未映射。
#[allow(clippy::too_many_arguments)]
fn merge_relation(
    index: &Index,
    items: &[Value],
    r: &RelationAssertion,
    maps: Option<&MapsTo>,
    stats: &mut MergeStats,
    claims: &mut BTreeMap<Slot, Vec<DocClaim>>,
    conflicts: &mut BTreeMap<Slot, Vec<Conflict>>,
    band_of_slot: &mut BTreeMap<Slot, &'static str>,
    touched: &mut BTreeSet<Slot>,
    doc_slots: &mut BTreeSet<Slot>,
    pending: &mut BTreeMap<String, Value>,
) -> Result<()> {
    let target = match index.resolve_relation(r, maps, stats) {
        Ok(t) => t,
        Err(u) => {
            stats.note_unmapped(u.code, u.detail);
            return Ok(());
        }
    };
    let maps = maps.ok_or_else(|| anyhow::anyhow!("内部错误：关系没有 maps_to 却解析成功"))?;
    let tuple = (
        target.table.clone(),
        target.column.clone(),
        target.parent_table.clone(),
    );

    // 已有边（含本轮刚建的同键边）
    if let Some(slot) = index.links.get(&tuple).cloned() {
        let db_card = match &slot {
            Slot::Existing(i) => items
                .get(*i)
                .and_then(payload)
                .and_then(|p| p.get("cardinality"))
                .and_then(Value::as_str)
                .map(str::to_string),
            Slot::Pending(key) => pending
                .get(key)
                .and_then(payload)
                .and_then(|p| p.get("cardinality"))
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        band_of_slot
            .entry(slot.clone())
            .and_modify(|b| *b = band_max(*b, DOC_BAND_CAP))
            .or_insert(DOC_BAND_CAP);
        match (r.cardinality.as_deref(), db_card.as_deref()) {
            // 两侧都说了、且不一致 ⇒ 冲突（规则 5）：**双保留**，条目不可 apply。
            (Some(doc), Some(db)) if doc != db => {
                if matches!(slot, Slot::Pending(_)) {
                    // doc-vs-doc 的分歧不叫「与 DB 冲突」（`db_says` 必须真的是 DB 的）。
                    stats.note_unmapped(
                        "doc_cardinality_conflict",
                        format!(
                            "{} -[{}]-> {}：两条 doc 断言对同一条边的基数不一致（{} vs {}）",
                            r.subject, r.predicate, r.object, doc, db
                        ),
                    );
                    return Ok(());
                }
                conflicts.entry(slot.clone()).or_default().push(Conflict {
                    subject: r.subject.clone(),
                    predicate: r.predicate.clone(),
                    object: r.object.clone(),
                    doc_says: doc.to_string(),
                    db_says: db.to_string(),
                    file: r.evidence.file.clone(),
                    chunk_id: r.evidence.chunk_id.clone(),
                    quote: r.evidence.quote.clone(),
                    span: r.evidence.span,
                });
                touched.insert(slot);
                // 冲突的说法**不算并入**：它没进条目，只进了 `conflicts[]`。
            }
            _ => {
                claims
                    .entry(slot.clone())
                    .or_default()
                    .push(DocClaim::relation(r, maps));
                touched.insert(slot);
                stats.merged += 1;
            }
        }
        return Ok(());
    }

    // 新边：doc-only 条目（规则 3/4 的另一半）。
    let key = target.key();
    if index.keys.contains(&key) {
        stats.note_unmapped(
            "key_collision",
            format!(
                "{} -[{}]-> {}：目标键 {key} 已被占用（⛔ 不自动改名）",
                r.subject, r.predicate, r.object
            ),
        );
        return Ok(());
    }
    let cardinality = match r.cardinality.clone() {
        Some(c) => c,
        None => {
            stats.cardinality_defaulted += 1;
            "N:1".to_string()
        }
    };
    let item = build_doc_link(&target, r, &cardinality);
    pending.insert(key.clone(), item);
    let slot = Slot::Pending(key);
    claims
        .entry(slot.clone())
        .or_default()
        .push(DocClaim::relation(r, maps));
    band_of_slot.insert(slot.clone(), DOC_BAND_CAP);
    touched.insert(slot.clone());
    doc_slots.insert(slot);
    stats.merged += 1;
    Ok(())
}

/// 造一条 `origin: "doc"` 的关系条目。
///
/// `payload.evidence` 的**唯一来源**是 `maps_to`：`{table: child 表, column: child 列}`。
/// 这条证据是 `ModelLinkView.column` 的唯一来源（REQ-120 的教训）——
/// 没有它，同一对 (from, to) 的两条边在 TTL 里会写成同一个 IRI。
fn build_doc_link(t: &LinkTarget, r: &RelationAssertion, cardinality: &str) -> Value {
    let claim = format!(
        "文档断言（REQ-129）：{} -[{}]-> {} · 实现列 {}.{} → {}.{}（未做值域重叠实测）",
        r.subject,
        r.predicate,
        r.object,
        t.table,
        t.column,
        t.parent_table,
        t.parent_column
    );
    serde_json::json!({
        "kind": "add_link_type",
        "target": format!("{}→{}", t.table, t.parent_table),
        "origin": ORIGIN_DOC,
        "band": DOC_BAND_CAP,
        "reviewed_by": Value::Null,
        "applicable": true,
        "payload": {
            "key": t.key(),
            "from_type": t.table,
            "to_type": t.parent_table,
            "cardinality": cardinality,
            "semantic": r.predicate,
            "evidence": [{
                "table": t.table,
                "column": t.column,
                "claim": claim,
            }],
        }
    })
}

// ──────────────────────────────────────────────────────────
// 小工具
// ──────────────────────────────────────────────────────────

fn payload(item: &Value) -> Option<&Map<String, Value>> {
    item.get("payload").and_then(Value::as_object)
}

fn payload_mut(item: &mut Value) -> Result<&mut Map<String, Value>> {
    item.get_mut("payload")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("条目没有 payload 对象"))
}

fn set_field(item: &mut Value, key: &str, v: Value) {
    if let Some(o) = item.as_object_mut() {
        o.insert(key.to_string(), v);
    }
}

/// 读回上一轮合并已写在条目上的 doc 证据（**幂等**：重跑合并不会翻倍）。
fn read_existing_claims(item: &Value) -> Vec<DocClaim> {
    payload(item)
        .and_then(|p| p.get(DOC_EVIDENCE_KEY))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| serde_json::from_value::<DocClaim>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
fn sha256_of(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

/// 渲染合并结果：**保留提案的其余顶层键**，只替换 `items` 并追加 `doc_merge` 摘要。
///
/// 确定性靠两件事：`serde_json::Map` 是 `BTreeMap`（键序固定）、本函数不写任何
/// 时间戳 / 随机数 / 输入顺序敏感的内容。
pub fn render_merged(proposal: &Value, items: &[Value], stats: &MergeStats) -> Result<String> {
    let mut doc: Map<String, Value> = proposal
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("提案顶层必须是 JSON 对象"))?;
    doc.insert("items".to_string(), Value::Array(items.to_vec()));
    doc.insert("doc_merge".to_string(), stats.summary_json());
    let mut text = serde_json::to_string_pretty(&Value::Object(doc)).context("合并结果序列化失败")?;
    text.push('\n');
    Ok(text)
}

// ══════════════════════════════════════════════════════════
// 单测（零 I/O：全部用内存夹具）
// ══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> Value {
        serde_json::json!({
            "project": "demo",
            "base_version": 1,
            "round": 1,
            "items": [
                {
                    "kind": "add_entity_type",
                    "target": "客户",
                    "payload": {
                        "key": "customers",
                        "display_name": "客户",
                        "aliases": ["顾客"],
                        "properties": [
                            {"column": "customer_name", "name": "客户名", "nullable": false}
                        ]
                    }
                },
                {
                    "kind": "add_entity_type",
                    "target": "订单",
                    "payload": {
                        "key": "order",
                        "display_name": "订单",
                        "properties": [
                            {"column": "customer_id", "name": "客户", "nullable": false},
                            {"column": "template_id", "name": "套餐", "nullable": false}
                        ]
                    }
                },
                {
                    "kind": "add_entity_type",
                    "target": "套餐模板",
                    "payload": {
                        "key": "package_templates",
                        "display_name": "套餐模板",
                        "properties": [
                            {"column": "name", "name": "套餐名", "nullable": false}
                        ]
                    }
                },
                {
                    "kind": "add_link_type",
                    "target": "订单→客户",
                    "confidence": {"score": 1.0, "method": "evidence-scorecard-v2", "band": "high"},
                    "payload": {
                        "key": "order_customer_id__customers",
                        "from_type": "order",
                        "to_type": "customers",
                        "cardinality": "N:1",
                        "semantic": "belongs_to",
                        "evidence": [{"table": "order", "column": "customer_id", "claim": "命名推断"}]
                    }
                }
            ]
        })
    }

    fn concept_jsonl(name: &str, aliases: &[&str], modality: &str, alignable: bool) -> String {
        let mut v = serde_json::json!({
            "kind": "concept",
            "name": name,
            "modality": modality,
            "alignable": alignable,
            "evidence": {
                "file": "01.md", "chunk_id": "abc:0-10", "quote": "原文", "span": [0, 6]
            }
        });
        if !aliases.is_empty() {
            v["aliases"] = serde_json::json!(aliases);
        }
        v.to_string()
    }

    fn relation_jsonl(card: Option<&str>, maps: Option<Value>, future: bool) -> String {
        let mut v = serde_json::json!({
            "kind": "relation",
            "subject": "订单",
            "predicate": "下单",
            "object": "客户",
            "modality": if future { "future" } else { "fact" },
            "alignable": !future,
            "evidence": {
                "file": "01.md", "chunk_id": "abc:20-30", "quote": "客户下单", "span": [20, 24]
            }
        });
        if let Some(c) = card {
            v["cardinality"] = serde_json::json!(c);
        }
        if let Some(m) = maps {
            v["maps_to"] = m;
        }
        v.to_string()
    }

    fn maps_order_customer() -> Value {
        serde_json::json!({
            "child": {"table": "order", "column": "customer_id"},
            "parent": {"table": "customers", "column": "id"}
        })
    }

    /// 规则 1：`future` / `example` 一条都不并，且计数分档。
    #[test]
    fn future_and_example_are_excluded_and_counted() {
        let jsonl = [
            concept_jsonl("客户", &[], "future", false),
            concept_jsonl("客户", &[], "example", false),
            concept_jsonl("客户", &[], "fact", true),
        ]
        .join("\n");
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.excluded_not_alignable, 2);
        assert_eq!(out.stats.excluded_future, 1);
        assert_eq!(out.stats.excluded_example, 1);
        assert_eq!(out.stats.merged, 1);
        assert!(out.stats.close());
        // 只有一条证据（被排除的两条没留下痕迹）
        assert_eq!(doc_claims(&out.document, "customers").len(), 1);
    }

    /// 规则 1 的另一半：`modality` 与 `alignable` 自相矛盾时**按模态**排除。
    #[test]
    fn modality_wins_over_a_hand_edited_alignable_flag() {
        let jsonl = concept_jsonl("客户", &[], "future", true);
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.excluded_not_alignable, 1);
        assert_eq!(out.stats.excluded_future, 1);
        assert_eq!(out.stats.excluded_modality_conflict, 1);
        assert_eq!(out.stats.merged, 0);
        assert!(out.stats.close());
    }

    /// 规则 2：doc 条目的 band 只能是 low / medium，**永远不是 strong**。
    ///
    /// 夹具刻意同时造两种档位来源：
    /// - 名字命中 ⇒ `low`
    /// - 显式 `maps_to` 的新边 ⇒ `medium`
    ///
    /// 变异自证（AC-003）：把 [`DOC_BAND_CAP`] 改成 `"strong"`，本测试立刻变红
    /// （`medium` 那条会被写成 `strong`）。
    #[test]
    fn doc_items_never_carry_a_band_stronger_than_medium() {
        let maps = serde_json::json!({
            "child": {"table": "order", "column": "template_id"},
            "parent": {"table": "package_templates", "column": "id"}
        });
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("N:1"), Some(maps), false),
        ]
        .join("\n");
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        let mut seen = 0;
        let mut bands: BTreeSet<String> = BTreeSet::new();
        for it in items(&out.document) {
            if it.get("origin").and_then(Value::as_str).is_some() {
                seen += 1;
                let band = it
                    .get("band")
                    .and_then(Value::as_str)
                    .expect("doc 条目必须有 band")
                    .to_string();
                assert!(
                    matches!(band.as_str(), "low" | "medium"),
                    "doc 条目的 band = {band:?} —— 只许 low / medium（⛔ strong）"
                );
                assert!(
                    it.get("reviewed_by").is_some(),
                    "doc 条目必须显式写 reviewed_by（null）"
                );
                assert!(it["reviewed_by"].is_null(), "doc 条目必须未签");
                bands.insert(band);
            }
        }
        assert!(seen >= 2, "夹具必须同时造出 both 与 doc 条目，否则这条判据会空转");
        assert_eq!(
            bands,
            ["low".to_string(), "medium".to_string()].into_iter().collect(),
            "两种档位来源都要覆盖到（名字命中 low / 显式映射 medium）"
        );
    }

    /// 规则 2 的压档：输入档位是 `strong` ⇒ 合并后必须是 `medium`（且记账）。
    #[test]
    fn a_strong_input_band_is_capped_to_medium() {
        let mut p = proposal();
        p["items"][3]["band"] = serde_json::json!("strong");
        let jsonl = relation_jsonl(Some("N:1"), Some(maps_order_customer()), false);
        let out = merge(&jsonl, &p).expect("合并应当成功");
        let item = link_item(&out.document, "order_customer_id__customers");
        assert_eq!(item["band"], serde_json::json!("medium"), "strong ⇒ 压到 medium");
        assert_eq!(item["origin"], serde_json::json!("both"));
        assert_eq!(out.stats.band_capped_strong, 1);
    }

    /// 规则 3：没有 `maps_to` 的关系 ⇒ 未映射，**且不许按名字猜表**。
    ///
    /// 变异自证（AC-004）：让合并器对无映射的关系按名字猜一张表，
    /// 这里 `doc_unmapped` 会变 0、且 `items` 里会多出一条边 ⇒ 红。
    #[test]
    fn a_relation_without_maps_to_is_unmapped_and_never_guessed() {
        let jsonl = relation_jsonl(Some("N:1"), None, false);
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.merged, 0);
        assert_eq!(out.stats.doc_unmapped, 1);
        assert_eq!(
            out.stats.unmapped_reasons.get("no_maps_to").copied(),
            Some(1)
        );
        assert_eq!(
            items(&out.document).len(),
            4,
            "条目数不许变（猜表会多一条边）"
        );
        assert!(out.stats.close());
    }

    /// 规则 3 的另一半：`maps_to` 指向提案里不存在的表 / 列 ⇒ 未映射。
    #[test]
    fn maps_to_must_resolve_inside_the_proposal() {
        for (maps, code) in [
            (
                serde_json::json!({
                    "child": {"table": "order", "column": "customer_id"},
                    "parent": {"table": "package_templates", "column": "no_such_column"}
                }),
                "parent_column_not_in_proposal",
            ),
            (
                serde_json::json!({
                    "child": {"table": "order", "column": "nope_id"},
                    "parent": {"table": "customers", "column": "id"}
                }),
                "column_not_in_proposal",
            ),
            (
                serde_json::json!({
                    "child": {"table": "order", "column": "customer_id"},
                    "parent": {"table": "ghost", "column": "id"}
                }),
                "table_not_in_proposal",
            ),
        ] {
            let jsonl = relation_jsonl(Some("N:1"), Some(maps), false);
            let out = merge(&jsonl, &proposal()).expect("合并应当成功");
            assert_eq!(out.stats.merged, 0);
            assert_eq!(out.stats.doc_unmapped, 1);
            assert_eq!(
                out.stats.unmapped_reasons.get(code).copied(),
                Some(1),
                "原因码不对：{:?}",
                out.stats.unmapped_reasons
            );
        }
    }

    /// 规则 4：名字/别名命中 ⇒ `origin: "both"`，**两侧证据都在**。
    #[test]
    fn a_name_hit_merges_into_one_both_item_keeping_both_sides_evidence() {
        // 第一条命中 DB 侧的别名（顾客），第二条命中 display_name（客户）。
        let jsonl = [
            concept_jsonl("顾客", &[], "fact", true),
            concept_jsonl("客户", &[], "fact", true),
        ]
        .join("\n");
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.merged, 2);
        assert_eq!(out.stats.merged_both, 1, "不许出现两条指向同一表的实体");
        let item = entity_item(&out.document, "customers");
        assert_eq!(item["origin"], serde_json::json!("both"));
        assert_eq!(doc_claims(&out.document, "customers").len(), 2);
        // DB 侧的 payload 原样（没有被 doc 侧覆盖）
        assert_eq!(
            item["payload"]["display_name"],
            serde_json::json!("客户"),
            "DB 的 display_name 不许被 doc 覆盖"
        );
        // 名字命中 ⇒ 档位是 low（对齐更弱），但仍在 {low, medium} 内
        assert_eq!(item["band"], serde_json::json!("low"));
    }

    /// 规则 4 的去重：同一条断言重复 3 次 ⇒ 1 条证据，计数如实。
    #[test]
    fn duplicate_assertions_collapse_to_one_evidence_entry() {
        let line = concept_jsonl("客户", &[], "fact", true);
        let jsonl = [line.clone(), line.clone(), line].join("\n");
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.merged, 3);
        assert_eq!(out.stats.duplicate_claims, 2);
        assert_eq!(doc_claims(&out.document, "customers").len(), 1);
        assert_eq!(out.stats.merged_both, 1);
    }

    /// 规则 4：显式 `maps_to` 的关系并进**同一条边** ⇒ `both`，基数一致。
    #[test]
    fn a_relation_with_matching_cardinality_becomes_both() {
        let jsonl = relation_jsonl(Some("N:1"), Some(maps_order_customer()), false);
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.merged, 1);
        assert_eq!(out.stats.merged_doc_only, 0);
        let item = link_item(&out.document, "order_customer_id__customers");
        assert_eq!(item["origin"], serde_json::json!("both"));
        assert_eq!(
            item["payload"]["cardinality"],
            serde_json::json!("N:1"),
            "DB 的基数不动"
        );
        assert_eq!(
            item["payload"]["semantic"],
            serde_json::json!("belongs_to"),
            "DB 的 semantic 不动"
        );
        assert_eq!(
            doc_claims(&out.document, "order_customer_id__customers").len(),
            1
        );
    }

    /// 规则 5：基数不一致 ⇒ `conflicts[]` 双保留 + `applicable: false`。
    ///
    /// 变异自证（AC-006）：把 `applicable: false` 去掉，
    /// `domain::apply` 的判据（`applier_rejects_an_inapplicable_item`）与这里的断言都会红。
    #[test]
    fn a_cardinality_mismatch_becomes_a_conflict_and_is_not_applicable() {
        let jsonl = relation_jsonl(Some("1:1"), Some(maps_order_customer()), false);
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.conflicts, 1);
        assert_eq!(out.stats.merged, 0, "冲突的说法不算并入");
        assert!(out.stats.close());
        let item = link_item(&out.document, "order_customer_id__customers");
        assert_eq!(item["applicable"], serde_json::json!(false));
        let c = &item["conflicts"][0];
        assert_eq!(c["doc_says"], serde_json::json!("1:1"));
        assert_eq!(c["db_says"], serde_json::json!("N:1"));
        assert_eq!(c["quote"], serde_json::json!("客户下单"), "doc 侧的原文证据要留");
        assert_eq!(
            item["payload"]["cardinality"],
            serde_json::json!("N:1"),
            "DB 侧也要留"
        );
    }

    /// 规则 3 的正向：新边（doc-only）的 `key` / `cardinality` / 实现列都对。
    #[test]
    fn a_new_doc_edge_becomes_a_doc_only_link_with_column_evidence() {
        let maps = serde_json::json!({
            "child": {"table": "order", "column": "template_id"},
            "parent": {"table": "package_templates", "column": "id"}
        });
        let jsonl = relation_jsonl(Some("1:1"), Some(maps), false);
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.merged_doc_only, 1);
        assert_eq!(out.stats.items_added, 1);
        assert_eq!(
            out.stats.parent_key_id_convention, 1,
            "父端 id 只靠约定通过 ⇒ 必须记账（可见）"
        );
        let item = link_item(&out.document, "order_template_id__package_templates");
        assert_eq!(item["origin"], serde_json::json!("doc"));
        assert_eq!(item["band"], serde_json::json!("medium"));
        assert_eq!(item["payload"]["from_type"], serde_json::json!("order"));
        assert_eq!(
            item["payload"]["to_type"],
            serde_json::json!("package_templates")
        );
        assert_eq!(item["payload"]["cardinality"], serde_json::json!("1:1"));
        assert_eq!(item["payload"]["semantic"], serde_json::json!("下单"));
        assert_eq!(
            item["payload"]["evidence"][0]["column"],
            serde_json::json!("template_id")
        );
        // 证据的表/列**全部**来自 maps_to（没有一个字来自名字猜测）
        assert_eq!(
            item["payload"]["evidence"][0]["table"],
            serde_json::json!("order")
        );
    }

    /// 规则 6：同一输入两次 ⇒ 逐字节相同（AC-001 的 sha256 判据）。
    #[test]
    fn the_same_input_renders_byte_identical_twice() {
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("N:1"), Some(maps_order_customer()), false),
            concept_jsonl("积分转赠", &[], "future", false),
        ]
        .join("\n");
        let a = merge(&jsonl, &proposal()).expect("第一次");
        let b = merge(&jsonl, &proposal()).expect("第二次");
        assert_eq!(a.rendered, b.rendered, "两次渲染必须逐字节相同");
        assert_eq!(
            sha256_hex(a.rendered.as_bytes()),
            sha256_hex(b.rendered.as_bytes())
        );
        assert!(a.rendered.ends_with('\n'));
        assert_eq!(
            sha256_of(&a.rendered),
            sha256_of(&b.rendered),
            "sha256 必须相等"
        );
    }

    /// 规则 6 的加强版：把**自己的产物**再喂一次 ⇒ 条目与统计逐字节相同（幂等）。
    ///
    /// ⚠️ 唯一会变的是 `doc_merge.proposal_sha256` —— 那是**输入文件**的指纹，
    /// 第二次的输入确实换了个人（从 `proposal.json` 变成 `merged.json`）。
    /// 条目本身一个字节都不许动，否则人每重跑一次就要重看一遍 diff。
    #[test]
    fn merging_its_own_output_is_idempotent() {
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("1:1"), Some(maps_order_customer()), false),
        ]
        .join("\n");
        let once = merge(&jsonl, &proposal()).expect("第一次");
        let twice = merge(&jsonl, &once.document).expect("第二次（吃自己的产物）");
        let strip = |doc: &Value| -> String {
            let mut d = doc.clone();
            if let Some(m) = d.get_mut("doc_merge").and_then(Value::as_object_mut) {
                m.remove("proposal_sha256");
            }
            serde_json::to_string_pretty(&d).unwrap()
        };
        assert_eq!(
            strip(&once.document),
            strip(&twice.document),
            "重跑合并的条目与统计必须逐字节相同"
        );
        assert_eq!(
            twice.stats.duplicate_claims, 0,
            "已在文件里的证据不算「重复断言」"
        );
        assert_eq!(twice.stats.merged_doc_only, once.stats.merged_doc_only);
        assert_eq!(twice.stats.merged_both, once.stats.merged_both);
    }

    /// 规则 7 的前半：合并器**只写 null**；输入里若有签名则被重置并记账。
    #[test]
    fn merge_always_writes_an_unsigned_doc_item_and_counts_resets() {
        let mut p = proposal();
        p["items"][0]["reviewed_by"] = serde_json::json!("someone");
        let jsonl = concept_jsonl("客户", &[], "fact", true);
        let out = merge(&jsonl, &p).expect("合并应当成功");
        let item = entity_item(&out.document, "customers");
        assert!(item["reviewed_by"].is_null(), "doc 条目必须未签");
        assert_eq!(out.stats.reviewed_by_reset, 1);
    }

    /// 提案的形状硬闸：没有 `items` ⇒ 报错（不产一份空合并）。
    #[test]
    fn a_proposal_without_items_is_an_error() {
        let err = merge("", &serde_json::json!({"project": "x"})).expect_err("必须报错");
        assert!(format!("{err:#}").contains("items"), "{err:#}");
    }

    /// 坏 JSONL ⇒ 点名第几行（不静默跳过）。
    #[test]
    fn a_broken_jsonl_line_is_reported_with_its_line_number() {
        let jsonl = [concept_jsonl("客户", &[], "fact", true), "{not json".into()].join("\n");
        let err = merge(&jsonl, &proposal()).expect_err("必须报错");
        let msg = format!("{err:#}");
        assert!(msg.contains("第 2 行"), "{msg}");
    }

    /// 空 JSONL ⇒ 合法（0 条断言），但条目数不变、五项全 0。
    #[test]
    fn an_empty_jsonl_keeps_the_proposal_untouched() {
        let out = merge("", &proposal()).expect("空输入也是合法输入");
        assert_eq!(out.stats.assertions_read, 0);
        assert_eq!(out.stats.merged, 0);
        assert_eq!(out.stats.items_out, 4);
        assert!(out.stats.close());
    }

    /// `maps_to` 形状错 ⇒ 未映射（`bad_maps_to`），**不**整份失败。
    #[test]
    fn a_bad_maps_to_shape_is_unmapped_not_a_hard_error() {
        let mut v: Value = serde_json::from_str(&relation_jsonl(Some("N:1"), None, false)).unwrap();
        // 字符串形状：⛔ 不认（不接受 "order.customer_id"，那要 split 一个字符串）
        v["maps_to"] = serde_json::json!("order.customer_id");
        let out = merge(&v.to_string(), &proposal()).expect("形状错不该整份失败");
        assert_eq!(out.stats.doc_unmapped, 1);
        assert_eq!(out.stats.unmapped_reasons.get("bad_maps_to").copied(), Some(1));
    }

    // ── 夹具读取小工具 ──
    fn items(doc: &Value) -> Vec<Value> {
        doc["items"].as_array().cloned().unwrap_or_default()
    }

    fn entity_item(doc: &Value, key: &str) -> Value {
        items(doc)
            .into_iter()
            .find(|it| {
                it["kind"] == "add_entity_type" && it["payload"]["key"] == serde_json::json!(key)
            })
            .unwrap_or_else(|| panic!("找不到实体 {key}"))
    }

    fn link_item(doc: &Value, key: &str) -> Value {
        items(doc)
            .into_iter()
            .find(|it| {
                it["kind"] == "add_link_type" && it["payload"]["key"] == serde_json::json!(key)
            })
            .unwrap_or_else(|| panic!("找不到边 {key}"))
    }

    fn doc_claims(doc: &Value, payload_key: &str) -> Vec<Value> {
        items(doc)
            .into_iter()
            .find(|it| it["payload"]["key"] == serde_json::json!(payload_key))
            .and_then(|it| it["payload"][DOC_EVIDENCE_KEY].as_array().cloned())
            .unwrap_or_default()
    }
}
