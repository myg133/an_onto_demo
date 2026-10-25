//! **REQ-129 · 文档断言 → 本体提案** 的机械合并（`ontok doc merge`）+ 签字（`ontok doc sign`）。
//!
//! # 一句话
//!
//! REQ-116 把文档变成了**带逐字证据的断言**（`assertions.jsonl`）；本模块把它们
//! **机械地**并进一份提案（`proposal.json`），再让一条签了字的条目进入那条链路：
//!
//! ```text
//! assertions.jsonl ──合并（本模块，纯函数）──▶ merged.json（与 proposal.json 同形状）
//!                                              ├─ doc sign（只写 reviewed_by / signed_at）
//!                                              └─ propose import → review decide → proposal apply
//! ```
//!
//! # 七条规则落在哪一行（demand §「合并规则」逐条对号）
//!
//! | # | 规则 | 落点 |
//! | --- | --- | --- |
//! | 1 | 只并 `alignable: true`；`future` / `example` 排除并计数 | [`exclusion`]，按模态分档 |
//! | 2 | doc 条目 band 最高 `medium`、`reviewed_by: null`；⛔ 不许 `strong` | [`cap_band`]（**唯一**的压档实现）+ [`DOC_BAND_CAP`] |
//! | 3 | 关系必须有显式 `maps_to`（child 列 + parent 列）；没有 ⇒ `doc_unmapped`；⛔ 不许猜表 | [`Index::resolve_relation`]：端点逐个校验（给了 `--schema` 就严格到列） |
//! | 4 | 同实体 ⇒ 一条，`origin: "both"`，两侧证据都保留 | [`Slot`] + `payload.doc_evidence`（DB 的 `payload.evidence` 原样不动） |
//! | 5 | 冲突双保留 + `applicable: false` | [`Conflict`]（doc-vs-db：`doc_says`+`db_says`；doc-vs-doc（REQ-143）：`doc_says`+`doc_says_2`） |
//! | 6 | 确定性：同一输入两次 ⇒ 逐字节相同 | 无时间戳 / 无随机 / 全程 `BTreeMap` + 稳定排序 + [`render_merged`] |
//! | 7 | 未签 ⇒ apply 时忽略 | 合并器只写 `reviewed_by: null`；签字走 [`sign`]（文件级、只写两栏）；忽略发生在 `domain::apply::unsigned_doc_only`（**同一判据**） |
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
//! # ⭐ REQ-143 · 纯文档通路（`--allow-doc-only`）
//!
//! 上面那套纪律的前提是「**库里已经有这张表**」。没有关系库的项目（Ops360）里
//! 概念永远命中不了、关系永远没有 `maps_to` ⇒ 并入 0、全量未映射。
//!
//! 打开 `--allow-doc-only` 之后多出的是「概念 / 关系可以**新建**」这条出口。
//! ⚠️ 有一处对既有对齐口径的**刻意偏离**，**只在开关打开时生效**：**名字优先于别名**
//! （见下节；不开开关时仍是 REQ-129 的「名字 ∪ 别名」口径）。其余规则一字不改。
//!
//! | 情形 | 行为 |
//! | --- | --- |
//! | 概念命中实体（`maps_to` / 名字 / 别名） | `origin: both`，合并两侧证据；**名字/别名的先后见下节** |
//! | 概念**没命中** | 新建 `add_entity_type`：key = [`doc_entity_key`]、`display_name` = 概念名、`band` = [`DOC_ONLY_BAND`]、`reviewed_by: null`、`origin: "doc"` |
//! | 关系两端都能解析（命中或本轮新建） | 新建 `add_link_type`：`from_type`/`to_type` = 实体 key、**没有 join 列**（`evidence[0].column` 为空串）、key = [`doc_link_key`] |
//! | 关系任一端解析不到 | 仍进 `doc_unmapped`（码 `endpoint_not_resolved`）—— ⛔ 不许凭空造端点 |
//! | 自环关系（`subject == object`） | 排除并计数（码 `self_link` + [`MergeStats::skipped_self_link`]）—— 与老路径的 `self_link` 同一条纪律 |
//! | 冲突（doc-vs-db，或**同一对端点的两条 doc 断言基数不一致**） | **双保留** + `applicable: false`：doc-vs-db 写 `doc_says`/`db_says`；doc-vs-doc 写 `doc_says`/`doc_says_2`。⛔ 不「先到者赢」（那会静默丢证据、且结果依赖断言行序） |
//! | 不可对齐（`future` / `example` / `alignable:false`） | 与不开开关时完全相同：排除并计数 |
//!
//! ## ⭐ 名字**优先**于别名（只在开关打开时）
//!
//! - **不开开关**：REQ-129 的口径是「名字 ∪ 别名」并起来看，命中 ≠ 1 ⇒ `ambiguous_name`。
//! - **开开关**：先看**名字** —— 名字唯一命中就直接认它；名字不中才用**别名**兜底
//!   （[`Index::resolve_mention`]）。名字本身歧义时**不**退到别名（那只会更不确定）。
//!
//! 为什么必须分开：真实样本里「`CLI 直采` 的别名 = `CLI`」，而 `CLI` **自己也是一个概念**。
//! 按并集看这两条都会命中 2 个实体 ⇒ 双双 `ambiguous_name`，刚为它们新建的两个实体就变成
//! **没有任何证据的空类**。名字优先让「自己的名字」胜过「别人的别名」，两条断言各归各的实体。
//! ⛔ 这不是模糊匹配：名字/别名仍是**全等**，歧义仍然不猜；别名兜底仍是 REQ-129 的老能力。
//!
//! 开关**默认关闭**：不开时这条通路**一行都不执行**，产物与旧版逐字节相同（R5）。
//!
//! ⛔ 开关**不**放松的事：不猜语义（「套餐」不会因为像 `package_templates` 就被并进去）、
//! 不给新条目 `strong`、不给关系编造 join 列、不在歧义时挑一个「最像的」、
//! 不因为「先到」就把后到的冲突说法丢掉。
//! 新建实体的 key **只由名字决定**（无序号 / 时间 / 随机 / 输入顺序，见
//! [`doc_entity_key`]）⇒ 同一份输入两次运行，`merged.json` 逐字节相同。
//!
//! # 校验面：提案是**必答项**，`schema.json` 是**严格档**（`--schema`）
//!
//! `proposal apply` 判定一条关系能不能建的判据就是**提案里的实体集合**
//! （`domain::apply::ApplyError::DanglingLink`）。所以「表是否存在于提案」永远是
//! 必答项 —— 若只信 `schema.json`，就会出现「合并器说表存在、apply 说两端不存在」。
//!
//! ⇒ 表 = `add_entity_type.payload.key`；列 = 该实体 `payload.properties[].column`。
//!
//! 但提案里**没有主键列**（抽取侧把 PK 剥掉了，见 `domain::apply` B-2），父端列
//! （`customers.id`）只能靠约定名顶着。**给了 `--schema` 就严格校验**（表/列都查，
//! 含父端主键列）；**没给**则退回 fallback，并把 fallback 的用量记进 stats
//! （[`MergeStats::parent_key_id_convention`]）—— AC-004 订正后的口径是
//! 「给了 `--schema` 时 0 例外；没给时 fallback 必须可数、可见」。
//!
//! # 已知限制（⛔ 不许写成「已支持」）
//!
//! - **没给 `--schema` 时父端列校验不完整**：`customers.id` 这类父端列在提案里
//!   **查不到**，只接受「列在 properties 里」或「列名恰好是 `id`」（后者记
//!   [`MergeStats::parent_key_id_convention`] + stderr 警告）。要 0 例外请给
//!   `--schema <schema.json>` —— 那条路才真的查主键列。
//! - **默认不许新增实体**（`--allow-doc-only` 是唯一例外）：doc 概念映射不到提案里的
//!   实体 ⇒ `doc_unmapped`。理由：凭空造一张表，apply 就会写进一个没有任何列的本体类。
//!   所以不开开关时 `origin: "doc"` 的**条目只可能是关系**（`add_link_type`）。
//! - **纯文档通路建出来的实体没有任何属性列**：文档只给了「有这个类」，
//!   没给字段 ⇒ `properties: []`、关系没有 join 列（`evidence[0].column` 为空串，
//!   apply 读证据时按「空列」丢弃 ⇒ 快照里这条边没有实现列）。这是**如实记录**，
//!   ⛔ 不拿列名去编。要让属性/join 落地，仍然得回到有库的那条路（`maps_to` + `--schema`）。
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
//! - **合并器会重置已有签名**：`reviewed_by` 恒写 `null`（规则 2 是绝对的），
//!   `signed_at` / `signed_at_source` 一并清掉。重跑合并 = 重新生成提案 ⇒
//!   之前的签字作废，计数在 [`MergeStats::reviewed_by_reset`]。
//! - **签字时间是事实，不是渲染选项**：`doc sign` 省略 `--at` 时取系统时钟，
//!   所以「逐字节可复现」的口径是「同一份输入 + 同一组参数（含 `--at`）」；
//!   重复签同一个人时**不改写**已有的 `signed_at`（幂等）。
//! - **`signed_at_source` 只标注来源，不改变「时间是事实」**：`arg` = 由 `--at`
//!   声明（人给的），`system` = 本次运行取的系统时钟（机器记的）。REQ-129 的老产物
//!   没有这一栏 ⇒ 读回按 `legacy`（未记）处理，**不报错、也不补写**
//!   （补写会把「不知道来源」伪装成某一种来源，审计上比缺字段更糟）。
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

/// ⭐ **REQ-143 的唯一档位**：纯文档通路新建条目的 `band`。
///
/// 比 [`DOC_BAND_CAP`] 低一档是有意的：这些条目**没有任何库侧证据**
/// （名字命中至少还有「库里有这张表」这一层，而这里连表都是文档说的）。
/// 它仍然 ≤ `medium`（规则 2 / R3），并且**永远不是 `strong`**。
///
/// 变异自证（REQ-143 纪律 G③）：把它改成 `"strong"`，
/// [`tests::doc_only_items_never_carry_a_band_stronger_than_medium`] 立刻变红。
pub const DOC_ONLY_BAND: &str = "low";

/// ⭐ REQ-143 · 纯文档通路新建实体的 key 前缀（`doc_…`，一眼可辨「这条来自文档」）。
pub const DOC_ONLY_ENTITY_KEY_PREFIX: &str = "doc_";

/// ⭐ REQ-143 · 纯文档通路新建关系的 key 前缀（`rel_…`）。
///
/// 与 DB 侧的 `{child}_{column}__{parent}` **不同**：doc-only 边没有列，
/// 拿列名拼 key 就是编造。这里用「两端实体 key + 谓词」的 sha256 —— 见 [`doc_link_key`]。
pub const DOC_ONLY_LINK_KEY_PREFIX: &str = "rel_";

/// hash 前缀长度（实体 key / 关系 key 都用 12 个 hex：48 bit，506 个名字的碰撞概率 ~1e-9）。
const DOC_KEY_HASH_LEN: usize = 12;


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

    fn relation(r: &RelationAssertion, maps: Option<&MapsTo>) -> Self {
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
            // ⭐ REQ-143：纯文档通路的关系**没有 maps_to**（端点按精确名字解析）⇒ `None`。
            maps_to: maps.cloned(),
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

/// 一条**基数冲突**：两侧说法不一致，条目 `applicable: false`。
///
/// 两种形态（`db_says` 与 `doc_says_2` **互斥**，各出现一个）：
///
/// | 形态 | 字段 | 场景 |
/// | --- | --- | --- |
/// | doc-vs-db | `doc_says` + `db_says` | doc 断言给了 `maps_to`，与提案里那条边的 `payload.cardinality` 不一致（规则 5） |
/// | doc-vs-doc | `doc_says` + `doc_says_2` | 同一对端点 / 同一谓词的两条 doc 断言基数不一致（REQ-143） |
///
/// 挑一边是**人的决定**，不是合并器的 —— 所以两种形态都**双保留**：
/// 两条说法都在条目里，`applicable: false` 让 `proposal apply` 拒绝并点名。
/// ⛔ 不做「先到者赢」：那会静默丢掉后到的那条证据，而且结果依赖断言行序。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Conflict {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// 第一条 doc 的基数（例：`1:1`）。
    pub doc_says: String,
    /// **第二条 doc** 的基数（只有 doc-vs-doc 冲突才有这一栏）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_says_2: Option<String>,
    /// DB 推断的基数（例：`N:1`，来自提案里那条边的 `payload.cardinality`；只有 doc-vs-db 才有）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_says: Option<String>,
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
    /// ⭐ `--schema` 严格面：本次是否给了真实 schema（`true` ⇒ 下面两个 fallback 计数必须是 0）。
    pub schema_checked: bool,
    /// 严格面的规模（schema 里几张表）—— 「没查」与「查了 0 张」必须可区分。
    pub schema_tables: usize,
    /// 给了 `--schema` 时，child 列只出现在 schema、不在提案 `properties` 里的条数。
    pub child_column_schema_only: usize,
    /// 给了 `--schema` 时，父端列只出现在 schema、不在提案 `properties` 里的条数
    /// （父端主键列的正常路径 —— 这也是「严格校验真的用上了」的证据）。
    pub parent_column_schema_only: usize,
    /// 关系断言没写基数、用了 `N:1` 默认值的条数。
    pub cardinality_defaulted: usize,
    // ── ⭐ REQ-143 · 纯文档通路的计数（**开关关闭时全部省略**，保证旧产物字节不变）──
    /// 本次是否开了 `--allow-doc-only`（`false` 时这个字段不出现在 stats.json 里）。
    #[serde(default, skip_serializing_if = "is_false")]
    pub doc_only: bool,
    /// 由文档**新建**的实体条目数（既有实体的命中不算）。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub doc_only_entities: usize,
    /// 由文档**新建**的关系条目数（没有 join 列）。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub doc_only_links: usize,
    /// ⭐ REQ-143 · 被排除的**自环**关系数（`subject == object`，码 `self_link`）。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skipped_self_link: usize,
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
    ///
    /// REQ-143 的三栏**只在开关打开时**出现：不开开关时这份 JSON 与旧版逐字节相同（R5）。
    pub fn summary_json(&self) -> Value {
        let mut v = serde_json::json!({
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
            "schema_checked": self.schema_checked,
            "schema_tables": self.schema_tables,
            "parent_key_id_convention": self.parent_key_id_convention,
            "unsigned_doc_items_are_ignored_on_apply": true,
        });
        if self.doc_only {
            v["allow_doc_only"] = Value::Bool(true);
            v["doc_only_entities"] = Value::from(self.doc_only_entities);
            v["doc_only_links"] = Value::from(self.doc_only_links);
            v["skipped_self_link"] = Value::from(self.skipped_self_link);
            v["doc_only_band"] = Value::from(DOC_ONLY_BAND);
        }
        v
    }
}

/// `#[serde(skip_serializing_if)]` 的两个小谓词（REQ-143：开关关闭时 stats 字节不变）。
fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &usize) -> bool {
    *n == 0
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
// 可选的真实 schema（`--schema`）—— 严格校验面
// ──────────────────────────────────────────────────────────

/// 真实库 schema 的**只读索引**（`ontok-introspect` 的 `schema.json`）。
///
/// # 为什么需要它（AC-004 的原话）
///
/// 「`M` 中所有 `maps_to` 的表/列都存在于 `schema.json`」。提案里**没有主键列**
/// （抽取侧把 PK 从 `properties` 里剥掉了，见 `domain::apply` B-2），所以
/// `customers.id` 这类**父端列**无法用提案校验 —— 只能靠约定名 `id` 顶着。
/// 给了 `--schema` 就**严格**校验（0 例外）；不给就退回 fallback，
/// 并把 fallback 的用量记进 stats（**可数、可见**，见
/// [`MergeStats::parent_key_id_convention`]）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaIndex {
    tables: BTreeMap<String, BTreeSet<String>>,
}

impl SchemaIndex {
    /// 从 `ontok-introspect` 的报告建索引（**唯一**的构造入口 —— 形状只有一处）。
    pub fn from_report(report: &crate::extract::introspect::SchemaReport) -> Self {
        let mut tables = BTreeMap::new();
        for t in &report.tables {
            let cols = t.columns.iter().map(|c| c.name.clone()).collect();
            tables.insert(t.name.clone(), cols);
        }
        Self { tables }
    }

    /// 表数（报告里报一句「校验面多大」，免得人以为「没查」）。
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    pub fn has_table(&self, table: &str) -> bool {
        self.tables.contains_key(table)
    }

    pub fn has_column(&self, table: &str, column: &str) -> bool {
        self.tables
            .get(table)
            .is_some_and(|cols| cols.contains(column))
    }
}

/// 合并的选项（今天有 `--schema` 与 `--allow-doc-only`；留结构体是为了不生出第二个
/// `merge_*` 变体）。
#[derive(Debug, Clone, Default)]
pub struct MergeOptions {
    /// `Some` ⇒ **严格**校验 `maps_to` 的每个表/列（含父端主键列）。
    pub schema: Option<SchemaIndex>,
    /// ⭐ REQ-143 · **纯文档通路**：概念没命中就新建实体、关系两端能解析就新建无 join 列的边。
    ///
    /// **默认 `false`**（R1：开关必须显式）。`false` 时下面的代码里所有
    /// `allow_doc_only` 分支都不会执行 ⇒ 产物与 REQ-129/130 逐字节相同（R5）。
    pub allow_doc_only: bool,
}

// ──────────────────────────────────────────────────────────
// 提案索引（**校验面的唯一来源**）
// ──────────────────────────────────────────────────────────

/// 提案里一个实体（表）的可用信息。
#[derive(Debug, Clone, Default)]
struct Entity {
    /// 人读名（doc-only 条目的 `target` 用它 —— 与 DB 侧条目的写法一致，
    /// 于是 `apply` 的报错与 `doc sign --item relation:订单→套餐模板` 都是人话）。
    display_name: String,
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
    /// 归一化**名字**（key / display_name）→ 实体 key（**可能多个** ⇒ 歧义 ⇒ 未映射）。
    by_name: BTreeMap<String, BTreeSet<String>>,
    /// 归一化**别名** → 实体 key。
    ///
    /// 与 [`Index::by_name`] 分开是 REQ-143 的需要：纯文档通路先按**名字**解析
    /// （一个概念的**名字**比它的别名更强），名字不中才看别名。
    /// 老路径（不开开关）把两张表**并起来**看，与 REQ-129 完全一致（见 [`Index::name_hits`]）。
    by_alias: BTreeMap<String, BTreeSet<String>>,
    /// ⭐ REQ-143 · 本轮由文档**新建**的实体 key（只在这个集合里的实体享受「名字优先」）。
    doc_keys: BTreeSet<String>,
    /// `(child 表, child 列, parent 表)` → 槽位。
    links: BTreeMap<(String, String, String), Slot>,
    /// 已占用的 `payload.key`（新条目撞键 ⇒ 未映射，⛔ 不自动改名）。
    keys: BTreeSet<String>,
    /// ⭐ REQ-143 · `payload.key` → 槽位（**按 key 找回条目**的唯一索引）。
    ///
    /// 纯文档通路的边没有 join 列，`links`（按 child 列索引）找不到它们；
    /// 重跑合并要吃自己的产物 ⇒ 必须能按 key 认回「上一轮建的那条边」（幂等，不重复建）。
    key_slots: BTreeMap<String, Slot>,
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
                        .unwrap_or(&key)
                        .to_string();
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
                    ix.insert_entity(key, display_name, &aliases, columns, Slot::Existing(i));
                }
                "add_link_type" => {
                    // ⭐ REQ-143 · 只有**关系**条目进 `key_slots`：纯文档通路靠它认回
                    // 「上一轮建的那条边」（幂等），⛔ 不该让实体 key 参与这个查找。
                    if let Some(k) = payload.get("key").and_then(Value::as_str) {
                        ix.key_slots.insert(k.to_string(), Slot::Existing(i));
                    }
                    let from = payload.get("from_type").and_then(Value::as_str);
                    let to = payload.get("to_type").and_then(Value::as_str);
                    let col = payload
                        .get("evidence")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(|e| e.get("column"))
                        .and_then(Value::as_str);
                    // ⭐ REQ-143：空列（纯文档通路的边）**不进** `links` ——
                    // 否则两条不同谓词、同一对端点的 doc 边会挤在同一个 ("", ...) 槽上。
                    // （`maps_to` 里的空列本来就会被 `resolve_relation` 判成 `bad_maps_to`。）
                    if let (Some(f), Some(t), Some(c)) = (from, to, col) {
                        if !c.is_empty() {
                            ix.links
                                .entry((f.to_string(), c.to_string(), t.to_string()))
                                .or_insert(Slot::Existing(i));
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(ix)
    }

    /// 把一个实体登记进索引（`build` 与 REQ-143 的新建实体走**同一条**登记路径）。
    fn insert_entity(
        &mut self,
        key: String,
        display_name: String,
        aliases: &[String],
        columns: BTreeSet<String>,
        slot: Slot,
    ) {
        for n in std::iter::once(key.as_str()).chain(std::iter::once(display_name.as_str())) {
            let n = norm_name(n);
            if !n.is_empty() {
                self.by_name.entry(n).or_default().insert(key.clone());
            }
        }
        for a in aliases {
            let n = norm_name(a);
            if !n.is_empty() {
                self.by_alias.entry(n).or_default().insert(key.clone());
            }
        }
        self.entity_items.insert(key.clone(), slot.clone());
        self.key_slots.insert(key.clone(), slot);
        self.entities.insert(
            key,
            Entity {
                display_name,
                columns,
            },
        );
    }

    /// ⭐ REQ-143 · 把一条**纯文档**新建的实体登记进索引（无属性列）。
    ///
    /// 只在 `--allow-doc-only` 下调用；登记后主循环按名字/别名解析就能命中它，
    /// 关系断言（可能出现在概念之前）也才拿得到端点。
    fn add_doc_entity(
        &mut self,
        key: &str,
        display_name: &str,
        aliases: &[String],
        slot: Slot,
    ) {
        self.keys.insert(key.to_string());
        self.doc_keys.insert(key.to_string());
        self.insert_entity(
            key.to_string(),
            display_name.to_string(),
            aliases,
            BTreeSet::new(),
            slot,
        );
    }

    /// 一条概念断言的名字/别名**精确命中**了哪些实体（0 / 1 / 多个）。
    ///
    /// ⚠️ 这是**老路径**的口味（名字与别名**不分优先级**，并起来看），
    /// REQ-129 的 `ambiguous_name` 判据就是它 —— ⛔ 不要为了 REQ-143 改这里（R5）。
    fn name_hits(&self, c: &ConceptAssertion) -> BTreeSet<String> {
        let mut hits: BTreeSet<String> = BTreeSet::new();
        for n in std::iter::once(c.name.as_str()).chain(c.aliases.iter().map(String::as_str)) {
            let n = norm_name(n);
            if let Some(ks) = self.by_name.get(&n) {
                hits.extend(ks.iter().cloned());
            }
            if let Some(ks) = self.by_alias.get(&n) {
                hits.extend(ks.iter().cloned());
            }
        }
        hits
    }

    /// ⭐ REQ-143 · **名字优先**解析一个「说法」（概念名 / 关系端点）。
    ///
    /// | 顺序 | 判据 | 为什么 |
    /// | --- | --- | --- |
    /// | 1 | **名字**命中唯一实体 | 名字比别名强：本轮新建的「CLI」不该被另一个概念的别名「CLI」拖成歧义 |
    /// | 2 | 名字 0 命中、**别名**命中唯一实体 | 别名命中照旧有效（REQ-129 的老能力） |
    /// | 3 | 其余（名字歧义 / 别名歧义 / 都没中） | `None` —— ⛔ 不挑一个「最像的」 |
    fn resolve_mention(&self, raw: &str, aliases: &[String]) -> Option<String> {
        let n = norm_name(raw);
        if let Some(hs) = self.by_name.get(&n) {
            return if hs.len() == 1 {
                hs.iter().next().cloned()
            } else if hs.is_empty() {
                None
            } else {
                // 名字本身就歧义 ⇒ 不再退到别名（那只会更不确定）。
                return None;
            };
        }
        let mut hits: BTreeSet<String> = BTreeSet::new();
        // 「这个说法本身」也可能是别名（关系端点没有单独的别名列表）。
        if let Some(ks) = self.by_alias.get(&n) {
            hits.extend(ks.iter().cloned());
        }
        for a in aliases {
            if let Some(ks) = self.by_alias.get(&norm_name(a)) {
                hits.extend(ks.iter().cloned());
            }
        }
        if hits.len() == 1 {
            hits.iter().next().cloned()
        } else {
            None
        }
    }

    /// 名字**唯一**命中的实体 key（0 个或多个 ⇒ `None`）。
    fn unique_by_name(&self, raw: &str) -> Option<String> {
        let hits = self.by_name.get(&norm_name(raw))?;
        if hits.len() == 1 {
            hits.iter().next().cloned()
        } else {
            None
        }
    }

    /// 端点解析失败的人话（点名是哪一端、为什么：没命中 / 歧义）。
    fn endpoint_detail(&self, raw: &str) -> String {
        let n = norm_name(raw);
        if let Some(hs) = self.by_name.get(&n).filter(|hs| !hs.is_empty()) {
            return if hs.len() > 1 {
                format!("{raw:?} 按名字命中 {} 个实体（歧义）", hs.len())
            } else {
                format!("{raw:?} 按名字命中 1 个实体")
            };
        }
        match self.by_alias.get(&n) {
            Some(hs) if hs.len() > 1 => format!("{raw:?} 按别名命中 {} 个实体（歧义）", hs.len()),
            Some(hs) if hs.len() == 1 => format!("{raw:?} 按别名命中 1 个实体"),
            _ => format!("{raw:?} 没有精确命中的实体"),
        }
    }

    /// 概念 → 一张表（`Ok(key)`）或未映射原因（`Err`）。
    fn resolve_concept(
        &self,
        c: &ConceptAssertion,
        maps: Option<&MapsTo>,
        opts: &MergeOptions,
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
                Some(t) if self.entities.contains_key(t) => {
                    // ⭐ --schema：概念映射的表也必须在**真实库**里（AC-004 的字面要求）
                    if let Some(schema) = &opts.schema {
                        if !schema.has_table(t) {
                            return Err(Unmapped::new(
                                "table_not_in_schema",
                                format!(
                                    "concept「{}」的 maps_to.table = {t:?} 不在 schema.json 里",
                                    c.name
                                ),
                            ));
                        }
                    }
                    return Ok(t.to_string());
                }
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
        // ⭐ REQ-143 · 纯文档通路：名字**唯一**命中的是本轮新建的实体 ⇒ 认它。
        //
        // 为什么要这条：新建实体之间可能出现「某概念的别名 = 另一概念的名字」
        // （真实样本里 `CLI 直采` 的别名是 `CLI`，而 `CLI` 自己也是一个概念）。
        // 按名字优先解析，两条断言各归各的实体，不会互相把对方挤成 `ambiguous_name`。
        // ⚠️ 只对**本轮新建**的实体生效：既有实体的匹配语义与不开开关时逐字相同（R5）。
        if opts.allow_doc_only {
            if let Some(k) = self.unique_by_name(&c.name) {
                if self.doc_keys.contains(&k) {
                    return Ok(k);
                }
            }
        }
        // 名字 / 别名命中（精确）
        let hits = self.name_hits(c);
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
    /// **端点逐一对齐**（规则 3）：
    /// - 两张表必须在**提案的实体集合**里（`apply` 的 `DanglingLink` 同一条判据）；
    /// - 给了 `--schema` ⇒ 每个表/列（**含父端主键列**）必须在真实 schema 里；
    /// - 没给 `--schema` ⇒ fallback：列必须在 `properties` 里，父端列额外接受约定名
    ///   `id`（并计数 —— AC-004 订正后要求 fallback「可数可见」）。
    fn resolve_relation(
        &self,
        r: &RelationAssertion,
        maps: Option<&MapsTo>,
        stats: &mut MergeStats,
        opts: &MergeOptions,
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
        // ⭐ 严格面（`--schema`）：**每个端点**都要在真实 schema 里，0 例外。
        if let Some(schema) = &opts.schema {
            for (role, ep) in [("child", child), ("parent", parent)] {
                if !schema.has_table(&ep.table) {
                    return Err(Unmapped::new(
                        "table_not_in_schema",
                        format!(
                            "{} -[{}]-> {}：maps_to.{role}.table = {:?} 不在 schema.json 里",
                            r.subject, r.predicate, r.object, ep.table
                        ),
                    ));
                }
                if !schema.has_column(&ep.table, &ep.column) {
                    return Err(Unmapped::new(
                        "column_not_in_schema",
                        format!(
                            "{} -[{}]-> {}：maps_to.{role} = {}.{} —— schema.json 里没有这一列",
                            r.subject, r.predicate, r.object, ep.table, ep.column
                        ),
                    ));
                }
            }
        }
        let child_entity = &self.entities[&child.table];
        if !child_entity.columns.contains(&child.column) {
            // 有 schema 撑着（列确实存在）⇒ 收，但要**计数**：这是「提案里没有的列」。
            if opts.schema.is_some() {
                stats.child_column_schema_only += 1;
            } else {
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
        }
        let parent_entity = &self.entities[&parent.table];
        if !parent_entity.columns.contains(&parent.column) {
            if opts.schema.is_some() {
                // 严格面已经在上面验过它在 schema 里（父端主键列的正常路径）。
                stats.parent_column_schema_only += 1;
            } else if parent.column == PK_CONVENTION {
                stats.parent_key_id_convention += 1;
            } else {
                return Err(Unmapped::new(
                    "parent_column_not_in_proposal",
                    format!(
                        "{} -[{}]-> {}：maps_to.parent = {}.{} —— {} 的属性里没有列 {:?}\
                         （只接受属性列，或约定的主键名 {:?}；给 `--schema` 可严格校验）",
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
// ⭐ REQ-143 · 纯文档通路：确定性 key + 新建实体的计划
// ──────────────────────────────────────────────────────────

/// 概念名 → **新建实体**的 key（`doc_{slug}_{sha256前12位}`）。
///
/// # 为什么是这个形状（R2 / AC-003）
///
/// | 成分 | 来源 | 换输入顺序会变吗 | 换一次运行会变吗 |
/// | --- | --- | --- | --- |
/// | `doc_` 前缀 | 常量 [`DOC_ONLY_ENTITY_KEY_PREFIX`] | 否 | 否 |
/// | `slug` | 名字里的 ASCII 字母/数字（小写、非 ASCII 压成 `_`） | 否 | 否 |
/// | 12 位 hex | `sha256(名字的 UTF-8)` 的前 12 位 | 否 | 否 |
///
/// ⛔ **没有**序号（老脚本的 `doc_01_…` 依赖枚举顺序，换个抽取轮次 key 就漂）、
/// **没有**时间戳、**没有**随机数。所以同一份断言跑两次，key 逐字相同（AC-003）。
/// 中文名 slug 为空 ⇒ key 是 `doc_<hash>`（TTL 的建模视图把非 ASCII 压成 `_`，
/// 把汉字留在 key 里会让「订单」「客户」都变成 `:doc____` —— 那是撞车）。
///
/// `taken` 只用来处理**哈希前缀撞车**（48 bit，506 个名字约 1e-9）：
/// 撞了就加长到 24 / 64 位。调用方按**名字字典序**分配 ⇒ 结果与输入顺序无关。
pub fn doc_entity_key(name: &str, taken: &BTreeSet<String>) -> String {
    derive_doc_key(DOC_ONLY_ENTITY_KEY_PREFIX, name.trim(), taken)
}

/// 关系的定位材料 → **新建 doc-only 边**的 key（`rel_{sha256前12位}`）。
///
/// # 为什么不用 `{child}_{column}__{parent}`
///
/// DB 侧的键法里 `column` 是**实现列**；纯文档通路**没有列**（R4 的另一面：
/// 不编造 join）。拿谓词冒充列名会让下游（`model_link_locals` 的歧义命名、
/// R2RML 的 `rr:predicate`）把「文档关系」当成「有实现列的关系」。
///
/// 所以键 = `sha256("{from}\x1f{predicate}\x1f{to}")` 的前 12 位：
/// 两端是实体 key（本身只由名字决定）+ 谓词，**只由输入内容决定**，无序号/时间/随机。
pub fn doc_link_key(from_type: &str, predicate: &str, to_type: &str) -> String {
    let material = format!(
        "{}\u{1f}{}\u{1f}{to_type}",
        from_type.trim(),
        predicate.trim()
    );
    format!(
        "{}{}",
        DOC_ONLY_LINK_KEY_PREFIX,
        &sha256_hex(material.as_bytes())[..DOC_KEY_HASH_LEN]
    )
}

fn derive_doc_key(prefix: &str, name: &str, taken: &BTreeSet<String>) -> String {
    let digest = sha256_hex(name.as_bytes());
    let slug = ascii_slug(name);
    for len in [DOC_KEY_HASH_LEN, 24, 64] {
        let h = &digest[..len.min(digest.len())];
        let cand = if slug.is_empty() {
            format!("{prefix}{h}")
        } else {
            format!("{prefix}{slug}_{h}")
        };
        if !taken.contains(&cand) {
            return cand;
        }
    }
    // 完整 sha256 也撞：只可能是同一个名字（key 天然相同），交给调用方去重。
    if slug.is_empty() {
        format!("{prefix}{digest}")
    } else {
        format!("{prefix}{slug}_{digest}")
    }
}

/// 名字 → ASCII slug：`[a-z0-9]` 保留，其余压成单个 `_`（首尾不留）。
///
/// 纯中文名 ⇒ 空串（见 [`doc_entity_key`] 的说明）。
fn ascii_slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_us = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_us && !out.is_empty() {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            pending_us = false;
        } else {
            pending_us = true;
        }
    }
    out
}

/// 一条**待新建**的实体（计划阶段的中间形状）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedEntity {
    /// 概念名（已去首尾空白）—— `display_name` 与 key 的来源。
    name: String,
    /// 断言里出现过的别名（已去重 / 排序 / 去掉会与**其他**新实体撞名的那些）。
    aliases: Vec<String>,
}

/// ⭐ REQ-143 · 计划出「纯文档通路要新建哪些实体」。
///
/// 判据**只看**三件事（不做任何语义猜测）：
///
/// 1. 这条概念断言是 `alignable` 的（`future` / `example` / `alignable:false` ⇒ 跳过，
///    与被排除的断言一致：不因为开关就把「设想/举例」造成类）；
/// 2. 它**没有显式 `maps_to`**（形状错 / 指向提案外的表也跳过 —— 人已经说了它属于哪张表，
///    ⛔ 不替他改口去造一个新类）；
/// 3. 名字/别名在原提案里**精确命中 0 个实体**（命中 1 个 ⇒ 照旧 `both`；
///    命中多个 ⇒ `ambiguous_name`，⛔ 不挑一个「最像的」，也不新建第三个）。
///
/// 同名断言只造一个实体（别名取集合口径：排序去重），于是「同一份输入换行序」不影响产物。
///
/// ⚠️ **不把 `description` 搬进 payload**：概念描述是整句原文，客户视图的发布闸会把
/// 「evidence / column」这类建模术语当泄露并**拒绝整份导出**（真实样本里就有一句
/// 「…包含 title、description、evidence…」）。描述仍完整留在 `payload.doc_evidence[]`
/// （审计面），只是不升级成 `rdfs:comment`。
fn plan_doc_entities(assertions_jsonl: &str, index: &Index) -> Result<Vec<PlannedEntity>> {
    let mut planned: BTreeMap<String, PlannedEntity> = BTreeMap::new();
    for (ln, raw) in assertions_jsonl.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(line)
            .with_context(|| format!("assertions JSONL 第 {} 行不是合法 JSON", ln + 1))?;
        let assertion: Assertion = serde_json::from_value(v.clone()).with_context(|| {
            format!(
                "assertions JSONL 第 {} 行的断言形状不对\
                 （REQ-116 的契约：kind/name|subject…/evidence）",
                ln + 1
            )
        })?;
        if exclusion(&assertion).is_some() {
            continue;
        }
        let Assertion::Concept(c) = assertion else {
            continue;
        };
        if matches!(v.get("maps_to"), Some(m) if !m.is_null()) {
            continue;
        }
        if !index.name_hits(&c).is_empty() {
            continue;
        }
        let name = c.name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let e = planned.entry(name.clone()).or_insert_with(|| PlannedEntity {
            name,
            aliases: Vec::new(),
        });
        for a in &c.aliases {
            let a = a.trim();
            if !a.is_empty() && !e.aliases.iter().any(|x| x == a) {
                e.aliases.push(a.to_string());
            }
        }
    }

    // 别名去冲突：新造实体之间「名字 / 别名」必须互不相同 ——
    // 否则主循环按精确名字解析会变成歧义（`ambiguous_name`），
    // 一条本来能对齐的断言会被**我们自己刚造的实体**挤掉。只保留全局唯一的名字串。
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for e in planned.values() {
        *used.entry(norm_name(&e.name)).or_insert(0) += 1;
        for a in &e.aliases {
            *used.entry(norm_name(a)).or_insert(0) += 1;
        }
    }
    Ok(planned
        .into_values()
        .map(|mut e| {
            e.aliases.retain(|a| used.get(&norm_name(a)).copied().unwrap_or(0) == 1);
            e.aliases.sort();
            e.aliases.dedup();
            e
        })
        .collect())
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

/// 把 `assertions.jsonl` 与一份提案合并成**同形状**的新提案（不给 `--schema`）。
///
/// 等价于 [`merge_with`] + 默认选项（fallback 校验面）。
pub fn merge(assertions_jsonl: &str, proposal: &Value) -> Result<MergeOutcome> {
    merge_with(assertions_jsonl, proposal, &MergeOptions::default())
}

/// 同 [`merge`]，但可以给**真实 schema** 做严格校验（`--schema`）。
pub fn merge_with(
    assertions_jsonl: &str,
    proposal: &Value,
    opts: &MergeOptions,
) -> Result<MergeOutcome> {
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
    let mut index = Index::build(&items_in)?;

    let mut items: Vec<Value> = items_in.clone();
    let mut stats = MergeStats {
        items_in: items.len(),
        assertions_sha256: sha256_hex(assertions_jsonl.as_bytes()),
        proposal_sha256,
        schema_checked: opts.schema.is_some(),
        schema_tables: opts.schema.as_ref().map(|s| s.table_count()).unwrap_or(0),
        doc_only: opts.allow_doc_only,
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
    // ⭐ REQ-143 · 纯文档**关系**槽位：它们的基数冲突要等**全部说法都收齐**才判
    // （「谁和谁打架」与断言行序无关，见写回阶段）。
    let mut doc_link_slots: BTreeSet<Slot> = BTreeSet::new();

    // ⭐ REQ-143 · 纯文档通路（开关**默认关闭**：这一段整块跳过）。
    //
    // 为什么在**主循环之前**：关系断言可能出现在它的概念之前 —— 真实样本
    // （Ops360 728 条）就是这样，最后一个概念在第 728 行。端点解析要看到的是一份
    // **完整的实体集合**，否则「概念在后面」的关系会被误判成「端点解析不到」。
    if opts.allow_doc_only {
        let planned = plan_doc_entities(assertions_jsonl, &index)?;
        let mut taken = index.keys.clone();
        for p in planned {
            let key = doc_entity_key(&p.name, &taken);
            taken.insert(key.clone());
            let idx = items.len();
            items.push(build_doc_entity(&key, &p));
            index.add_doc_entity(&key, &p.name, &p.aliases, Slot::Existing(idx));
            // 标成 doc 槽位 ⇒ 写回阶段 origin = doc（不是 both）。
            doc_slots.insert(Slot::Existing(idx));
            stats.items_added += 1;
            stats.doc_only_entities += 1;
        }
    }

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
                opts,
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
                &mut doc_link_slots,
                &mut pending,
                opts,
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
        if doc_link_slots.remove(&from) {
            doc_link_slots.insert(to.clone());
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
        //
        // ⚠️ 签字时间也一起清掉（`doc sign` 写的 `signed_at`）：只清 `reviewed_by`
        // 会留下「没人签、但有签字时间」的自相矛盾状态。
        if item.get("reviewed_by").is_some_and(|v| !v.is_null()) {
            stats.reviewed_by_reset += 1;
        }
        set_field(item, "reviewed_by", Value::Null);
        if let Some(o) = item.as_object_mut() {
            o.remove(SIGNED_AT_KEY);
            // 来源也一起清：只清时间会留下「没人签、但有签字来源」的自相矛盾状态。
            o.remove(SIGNED_AT_SOURCE_KEY);
        }
        set_field(item, "applicable", Value::Bool(true));

        // doc 证据：已在文件里的（本模块上一轮的产物）不算「重复断言」，只做去重合并。
        let already = read_existing_claims(item);
        let mut fresh = claims.remove(&Slot::Existing(idx)).unwrap_or_default();
        // ⚠️ 记账用「本轮这条边上的断言数」（**保留重复**）：纯文档边的 merged/conflicts
        // 在下面统一结算，去掉重复会把「两条一样的断言」漏记成一条。
        let n_fresh_all = fresh.len();
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
        payload_mut(item)?.insert(
            DOC_EVIDENCE_KEY.to_string(),
            serde_json::to_value(&all).context("doc 证据序列化失败")?,
        );

        // ⭐ REQ-143 · 纯文档关系边的**基数裁决**：以这条边上的**全部** doc 说法为准
        // （不看行序），≥2 种基数 ⇒ 每种说法一条 conflict + `applicable:false`。
        let mut doc_vs_doc: Vec<Conflict> = Vec::new();
        if doc_link_slots.contains(&Slot::Existing(idx)) {
            doc_vs_doc = doc_vs_doc_conflicts(&all);
            if doc_vs_doc.is_empty() {
                stats.merged += n_fresh_all;
                // 基数归一：这条边上只有**一种显式**说法 ⇒ 就用它。
                // （否则 item 的 cardinality 取决于「谁先到」：先到的是没写基数的说法时会被
                //  默认成 `N:1`，和后来那条显式说法不一致 —— 同一输入打乱行序就变。）
                let mut cards: BTreeSet<&str> =
                    all.iter().filter_map(|c| c.cardinality.as_deref()).collect();
                if cards.len() == 1 {
                    if let Some(only) = cards.pop_first() {
                        payload_mut(item)?
                            .insert("cardinality".to_string(), Value::from(only));
                    }
                }
            } else {
                // 冲突的断言进 `conflicts` 这一档（**每条说法都算**，不是「后到的才不算」）。
                stats.conflicts += n_fresh_all;
            }
        }

        // 冲突：双保留 + applicable:false（规则 5 / AC-006）。
        let mut cfs = conflicts.remove(&Slot::Existing(idx)).unwrap_or_default();
        cfs.extend(doc_vs_doc);
        cfs.sort();
        cfs.dedup();
        if !cfs.is_empty() {
            // doc-vs-db 的冲突在这里记账；doc-vs-doc 已在上面记过（按断言数）。
            stats.conflicts += cfs.iter().filter(|c| c.db_says.is_some()).count();
            // ⭐ REQ-143 · doc-vs-doc 冲突时把 `payload.cardinality` 归一成**字典序最小**的说法：
            // 条目本来就 `applicable:false`（值只作展示），归一是为了「打乱断言行序 ⇒ 条目不变」。
            let doc_vs_doc_min = cfs.iter().any(|c| c.doc_says_2.is_some()).then(|| {
                all.iter()
                    .filter_map(|c| c.cardinality.as_deref())
                    .min()
                    .map(str::to_string)
            });
            for c in &cfs {
                if stats.conflict_samples.len() < MAX_SAMPLES {
                    stats.conflict_samples.push(c.clone());
                }
            }
            set_field(item, "applicable", Value::Bool(false));
            if let Some(Some(min_card)) = doc_vs_doc_min {
                payload_mut(item)?.insert("cardinality".to_string(), Value::from(min_card));
            }
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
    opts: &MergeOptions,
) -> Result<()> {
    match index.resolve_concept(c, maps, opts) {
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
    doc_link_slots: &mut BTreeSet<Slot>,
    pending: &mut BTreeMap<String, Value>,
    opts: &MergeOptions,
) -> Result<()> {
    // ⭐ REQ-143 · 纯文档通路：**没有 `maps_to`** 的关系不再一律未映射 ——
    // 两端都能按精确名字/别名命中（含本轮新建的实体）就建一条**没有 join 列**的边。
    //
    // ⛔ 有 `maps_to` 的关系**一律走老路**（包括形状错 / 指向提案外表的那些）：
    // 人已经给出了对齐键，开关不该把它改写成「按名字猜」。
    if opts.allow_doc_only && maps.is_none() {
        return merge_doc_only_relation(
            index,
            r,
            stats,
            claims,
            band_of_slot,
            touched,
            doc_slots,
            doc_link_slots,
            pending,
        );
    }
    let target = match index.resolve_relation(r, maps, stats, opts) {
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
    //
    // ⭐ REQ-143 · 开关打开时也要认回**本轮新建**的边：`index.links` 只收提案里的边、
    // 不收 `pending`；不认的话第二条断言会覆盖 pending（last-wins ⇒ 依赖行序）。
    // 开关关闭时保持 REQ-129 的老行为（R5：老路径一个字节都不许动）。
    let existing_slot = if opts.allow_doc_only {
        pending
            .get_key_value(&target.key())
            .map(|(k, _)| Slot::Pending(k.clone()))
            .or_else(|| index.links.get(&tuple).cloned())
    } else {
        index.links.get(&tuple).cloned()
    };
    if let Some(slot) = existing_slot {
        let existing_item = item_of_slot(&slot, items, pending);
        let db_card = existing_item
            .and_then(payload)
            .and_then(|p| p.get("cardinality"))
            .and_then(Value::as_str)
            .map(str::to_string);
        // 这条边是不是**doc 自己建的**（`Pending` 一定是；`Existing` 看 origin）。
        let prior_is_doc =
            matches!(slot, Slot::Pending(_)) || existing_item.and_then(origin_of_value) == Some(ORIGIN_DOC);
        band_of_slot
            .entry(slot.clone())
            .and_modify(|b| *b = band_max(*b, DOC_BAND_CAP))
            .or_insert(DOC_BAND_CAP);
        // ⭐ N-1 · 开关下：doc-vs-doc 的基数分歧一律**双保留**（`doc_says`/`doc_says_2`），
        // ⛔ 不「先到者赢」、也不冒充 `db_says`。不开开关时下面的老分支原样执行（R5）。
        if opts.allow_doc_only && prior_is_doc {
            if let (Some(doc), Some(prev)) = (r.cardinality.as_deref(), db_card.as_deref()) {
                if doc != prev {
                    let new_claim =
                        push_doc_vs_doc_conflict(conflicts, &slot, r, doc, prev, claims);
                    stats.conflicts += 1;
                    claims.entry(slot.clone()).or_default().push(new_claim);
                    touched.insert(slot);
                    return Ok(());
                }
            }
        }
        match (r.cardinality.as_deref(), db_card.as_deref()) {
            // 两侧都说了、且不一致 ⇒ 冲突（规则 5）：**双保留**，条目不可 apply。
            (Some(doc), Some(db)) if doc != db => {
                if matches!(slot, Slot::Pending(_)) {
                    // doc-vs-doc 的分歧不叫「与 DB 冲突」（`db_says` 必须真的是 DB 的）。
                    // （开关打开时上面已经拦掉；这里只剩不开开关的 REQ-129 老口径。）
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
                    doc_says_2: None,
                    db_says: Some(db.to_string()),
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
                    .push(DocClaim::relation(r, Some(maps)));
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
    let item = build_doc_link(&target, r, &cardinality, index);
    pending.insert(key.clone(), item);
    let slot = Slot::Pending(key);
    claims
        .entry(slot.clone())
        .or_default()
        .push(DocClaim::relation(r, Some(maps)));
    band_of_slot.insert(slot.clone(), DOC_BAND_CAP);
    touched.insert(slot.clone());
    doc_slots.insert(slot);
    stats.merged += 1;
    Ok(())
}

/// ⭐ REQ-143 · 没有 `maps_to` 的关系：两端按**精确名字/别名**解析，能解析就建无 join 列的边。
///
/// # 五条边界（AC-005 / R4 / N-1 / N-2）
///
/// | 情形 | 行为 |
/// | --- | --- |
/// | 两端都解析到**唯一**实体 | 建 `add_link_type`（`from_type`/`to_type` = 实体 key，`evidence[0].column` = `""`） |
/// | 任一端解析不到 / 命中多个 | `doc_unmapped`（码 `endpoint_not_resolved`）—— ⛔ **不凭空造端点** |
/// | 自环（`from == to`） | `doc_unmapped`（码 `self_link`）+ [`MergeStats::skipped_self_link`] —— 与老路径同一条纪律 |
/// | 同键的边已经存在 | 并证据，**不重复建**（重跑合并幂等） |
/// | 同一条边上出现**不一致的基数**（≥2 种 doc 说法） | **双保留**：写 `conflicts[]`（每种说法一条，`doc_says` 是它自己、`doc_says_2` 是其余说法）+ `applicable:false`；⛔ 不「先到者赢」 |
///
/// # 基数裁决为什么留到写回阶段
///
/// 「谁和谁打架」要看到这条边上的**全部** doc 说法才判得对：若只在「第二条断言到达时」和
/// 第一条比，冲突条数会取决于 `N:1` 和 `1:1` **谁先出现**（真实样本 728 条打乱行序 ⇒ 2 条 vs 3 条）。
/// 所以这里只收证据、**不记账**（[`MergeStats::merged`] / `conflicts` 都在写回阶段按
/// 该槽位的**全部**说法一次性结算），于是「同一输入打乱顺序 ⇒ conflicts 内容相同」。
///
/// 已有边只可能是 doc 边（key 是 [`doc_link_key`] 的产物，DB 侧的键法不同）；
/// 万一真有同键的 DB 边，`origin` 会按老规矩变成 `both`，⛔ 不覆盖 DB 侧证据。
#[allow(clippy::too_many_arguments)]
fn merge_doc_only_relation(
    index: &Index,
    r: &RelationAssertion,
    stats: &mut MergeStats,
    claims: &mut BTreeMap<Slot, Vec<DocClaim>>,
    band_of_slot: &mut BTreeMap<Slot, &'static str>,
    touched: &mut BTreeSet<Slot>,
    doc_slots: &mut BTreeSet<Slot>,
    doc_link_slots: &mut BTreeSet<Slot>,
    pending: &mut BTreeMap<String, Value>,
) -> Result<()> {
    let (Some(from), Some(to)) = (
        index.resolve_mention(&r.subject, &[]),
        index.resolve_mention(&r.object, &[]),
    ) else {
        stats.note_unmapped(
            "endpoint_not_resolved",
            format!(
                "{} -[{}]-> {}：doc-only 只按**精确名字/别名**解析端点\
                 （subject {}；object {}）—— ⛔ 不凭空造端点",
                r.subject,
                r.predicate,
                r.object,
                index.endpoint_detail(&r.subject),
                index.endpoint_detail(&r.object)
            ),
        );
        return Ok(());
    };
    // ⭐ N-2 · 自环排除：老路径（`resolve_relation`）有 `self_link`，doc-only 不能比它松。
    if from == to {
        stats.skipped_self_link += 1;
        stats.note_unmapped(
            "self_link",
            format!(
                "{} -[{}]-> {}：两端解析到同一个实体（{}）—— 自环不在本需求范围",
                r.subject, r.predicate, r.object, from
            ),
        );
        return Ok(());
    }
    let key = doc_link_key(&from, &r.predicate, &to);

    // 同一条边已经存在？（本轮刚建 / 上一轮合并的产物）
    let existing: Option<Slot> = if pending.contains_key(&key) {
        Some(Slot::Pending(key.clone()))
    } else {
        index.key_slots.get(&key).cloned()
    };
    if let Some(slot) = existing {
        // 同一条边的后续说法：只收证据（顺带把档位记成 low）。
        // ⛔ 这里**不判**基数冲突 —— 判据要看这条边上的全部说法，见函数头。
        claims
            .entry(slot.clone())
            .or_default()
            .push(DocClaim::relation(r, None));
        band_of_slot
            .entry(slot.clone())
            .and_modify(|b| *b = band_max(*b, DOC_ONLY_BAND))
            .or_insert(DOC_ONLY_BAND);
        touched.insert(slot.clone());
        doc_link_slots.insert(slot);
        return Ok(());
    }

    let cardinality = match r.cardinality.clone() {
        Some(c) => c,
        None => {
            stats.cardinality_defaulted += 1;
            "N:1".to_string()
        }
    };
    let item = build_doc_only_link(&from, &to, &key, r, &cardinality, index);
    pending.insert(key.clone(), item);
    let slot = Slot::Pending(key);
    claims
        .entry(slot.clone())
        .or_default()
        .push(DocClaim::relation(r, None));
    band_of_slot.insert(slot.clone(), DOC_ONLY_BAND);
    touched.insert(slot.clone());
    doc_slots.insert(slot.clone());
    doc_link_slots.insert(slot);
    stats.doc_only_links += 1;
    Ok(())
}

/// ⭐ REQ-143 · 记一条 **doc-vs-doc** 基数冲突（双保留 + 与断言行序无关）。
///
/// - 两个基数**排序**后写进 `doc_says` / `doc_says_2`：同一对断言换顺序得到**同一条** conflict；
/// - 证据取两条里 `order_key` 较小的那条（内容序，不看行号）；
/// - 返回「后到那条」的 claim：调用方把它塞进 `doc_evidence`（证据不丢），但**不计 merged**
///   —— 它落在 `conflicts` 这一档，[`MergeStats::close`] 的闭合式仍然成立。
fn push_doc_vs_doc_conflict(
    conflicts: &mut BTreeMap<Slot, Vec<Conflict>>,
    slot: &Slot,
    r: &RelationAssertion,
    doc: &str,
    other: &str,
    claims: &BTreeMap<Slot, Vec<DocClaim>>,
) -> DocClaim {
    let new_claim = DocClaim::relation(r, None);
    let canonical = match claims.get(slot).and_then(|v| v.first()) {
        Some(old) if old.order_key() <= new_claim.order_key() => old.clone(),
        _ => new_claim.clone(),
    };
    let (lo, hi) = if doc <= other { (doc, other) } else { (other, doc) };
    conflicts.entry(slot.clone()).or_default().push(Conflict {
        subject: canonical.subject.clone().unwrap_or_default(),
        predicate: canonical.predicate.clone().unwrap_or_default(),
        object: canonical.object.clone().unwrap_or_default(),
        doc_says: lo.to_string(),
        doc_says_2: Some(hi.to_string()),
        db_says: None,
        file: canonical.file.clone(),
        chunk_id: canonical.chunk_id.clone(),
        quote: canonical.quote.clone(),
        span: canonical.span,
    });
    new_claim
}

/// ⭐ REQ-143 · 一条纯文档边上的**全部** doc 说法 → doc-vs-doc 冲突（与输入顺序无关）。
///
/// - 每种**基数**一条 conflict：`doc_says` = 这种说法，`doc_says_2` = 其余说法按字典序 `" / "` 连接；
/// - 证据取该基数组里 `order_key` 最小的 claim（内容序，不看行号/行序）；
/// - 只有 0/1 种基数 ⇒ 空 vec（不是冲突）。
///
/// 这样 `N:1` 和 `1:1` 谁先出现都不影响输出；三条以上说法也不会因为「和第一条比」而漏记。
fn doc_vs_doc_conflicts(all: &[DocClaim]) -> Vec<Conflict> {
    let mut groups: BTreeMap<&str, &DocClaim> = BTreeMap::new();
    for c in all {
        let Some(card) = c.cardinality.as_deref() else {
            continue;
        };
        groups
            .entry(card)
            .and_modify(|cur| {
                if c.order_key() < cur.order_key() {
                    *cur = c;
                }
            })
            .or_insert(c);
    }
    if groups.len() < 2 {
        return Vec::new();
    }
    let cards: Vec<&str> = groups.keys().copied().collect();
    let mut out = Vec::with_capacity(cards.len());
    for (card, claim) in &groups {
        let others: Vec<&str> = cards.iter().copied().filter(|c| c != card).collect();
        out.push(Conflict {
            subject: claim.subject.clone().unwrap_or_default(),
            predicate: claim.predicate.clone().unwrap_or_default(),
            object: claim.object.clone().unwrap_or_default(),
            doc_says: (*card).to_string(),
            doc_says_2: Some(others.join(" / ")),
            db_says: None,
            file: claim.file.clone(),
            chunk_id: claim.chunk_id.clone(),
            quote: claim.quote.clone(),
            span: claim.span,
        });
    }
    out
}

/// 槽位 → 条目（本轮待建的也算）。
fn item_of_slot<'a>(
    slot: &Slot,
    items: &'a [Value],
    pending: &'a BTreeMap<String, Value>,
) -> Option<&'a Value> {
    match slot {
        Slot::Existing(i) => items.get(*i),
        Slot::Pending(k) => pending.get(k),
    }
}

/// ⭐ REQ-143 · 造一条 `origin: "doc"` 的**实体**条目（纯文档通路新建的概念）。
///
/// - `key` 由 [`doc_entity_key`] 从名字确定性派生；
/// - `display_name` = 概念名（客户视图的 `rdfs:label` 用它，客户看不到 key）；
/// - `band` = [`DOC_ONLY_BAND`]（≤ medium，永远不是 strong）、`reviewed_by: null`（R3）；
/// - `properties: []` —— 文档只说了「有这个类」，没给字段，⛔ 不编列；
/// - **不搬 `description`**（整句原文留在 `doc_evidence[]`；搬成 `rdfs:comment` 会让
///   客户视图的发布闸因为文里的「evidence / column」拒绝整份导出，见
///   [`plan_doc_entities`] 的说明）。
fn build_doc_entity(key: &str, p: &PlannedEntity) -> Value {
    let mut payload = serde_json::json!({
        "key": key,
        "display_name": p.name,
        "properties": [],
    });
    if !p.aliases.is_empty() {
        payload["aliases"] = serde_json::json!(p.aliases);
    }
    serde_json::json!({
        "kind": "add_entity_type",
        "target": p.name,
        "origin": ORIGIN_DOC,
        "band": DOC_ONLY_BAND,
        "reviewed_by": Value::Null,
        "applicable": true,
        "payload": payload,
    })
}

/// ⭐ REQ-143 · 造一条**没有 join 列**的 `origin: "doc"` 关系条目。
///
/// `payload.evidence[0].column` 是**空串**：`domain::apply::evidence_from_json` 会按
/// 「空列」整条丢弃 ⇒ 快照里这条边没有实现列（`ModelLinkView.column = None`）。
/// ⛔ 不拿谓词冒充列名：那会让下游把「文档关系」当成「有实现列的关系」，
/// 还会污染 `r2rml generate` 的 join 判定。
fn build_doc_only_link(
    from: &str,
    to: &str,
    key: &str,
    r: &RelationAssertion,
    cardinality: &str,
    index: &Index,
) -> Value {
    let from_label = index
        .entities
        .get(from)
        .map(|e| e.display_name.as_str())
        .unwrap_or(from);
    let to_label = index
        .entities
        .get(to)
        .map(|e| e.display_name.as_str())
        .unwrap_or(to);
    let claim = format!(
        "文档断言（REQ-143 doc-only）：{} -[{}]-> {} · 两端按精确名字解析到实体 key（{} → {}）；\
         没有关系库 ⇒ 没有实现列（⛔ 不编造 join）",
        r.subject, r.predicate, r.object, from, to
    );
    serde_json::json!({
        "kind": "add_link_type",
        "target": format!("{from_label}→{to_label}"),
        "origin": ORIGIN_DOC,
        "band": DOC_ONLY_BAND,
        "reviewed_by": Value::Null,
        "applicable": true,
        "payload": {
            "key": key,
            "from_type": from,
            "to_type": to,
            "cardinality": cardinality,
            "semantic": r.predicate,
            "evidence": [{
                "table": "",
                "column": "",
                "claim": claim,
            }],
        }
    })
}

/// 造一条 `origin: "doc"` 的关系条目。
///
/// `payload.evidence` 的**唯一来源**是 `maps_to`：`{table: child 表, column: child 列}`。
/// 这条证据是 `ModelLinkView.column` 的唯一来源（REQ-120 的教训）——
/// 没有它，同一对 (from, to) 的两条边在 TTL 里会写成同一个 IRI。
///
/// `target` 用**展示名**（`订单→套餐模板`），与 DB 侧条目同一写法：
/// 它既是 `apply` 报错里点名的那个串，也是 `doc sign --item relation:订单→套餐模板`
/// 认的稳定键（⛔ 不用数组下标 —— 顺序会变）。
fn build_doc_link(
    t: &LinkTarget,
    r: &RelationAssertion,
    cardinality: &str,
    index: &Index,
) -> Value {
    let child_label = index
        .entities
        .get(&t.table)
        .map(|e| e.display_name.as_str())
        .unwrap_or(t.table.as_str());
    let parent_label = index
        .entities
        .get(&t.parent_table)
        .map(|e| e.display_name.as_str())
        .unwrap_or(t.parent_table.as_str());
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
        "target": format!("{child_label}→{parent_label}"),
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
// 签字（`ontok doc sign`）—— 规则 7 的另一半
// ══════════════════════════════════════════════════════════

/// 签字时间字段（与 `reviewed_by` 一起写在**条目顶层**）。
pub const SIGNED_AT_KEY: &str = "signed_at";

/// ⭐ REQ-130 · 签字时间的**来源**（写在条目顶层，紧挨 `signed_at`）。
///
/// 审计要回答的是「这个时间是人给的，还是机器取的」——
/// 前者是**声明**（`arg`，可复现、可解释），后者是**记录**（`system`，只属于那一次运行），
/// 两者可信度不同，所以必须分开标。
pub const SIGNED_AT_SOURCE_KEY: &str = "signed_at_source";

/// `signed_at` 来自 `--at`（调用方给的，声明）。
pub const AT_SOURCE_ARG: &str = "arg";
/// `signed_at` 来自系统时钟（本次运行取的，记录）。
pub const AT_SOURCE_SYSTEM: &str = "system";
/// 老产物（REQ-129 写的，没有这一栏）。**只出现在读回报表里，⛔ 不写进产物**。
pub const AT_SOURCE_LEGACY: &str = "legacy";

/// 签字的结果。
#[derive(Debug, Clone)]
pub struct SignOutcome {
    /// 签完的文档。
    pub document: Value,
    /// 落盘字节（pretty JSON + 末尾换行）—— 与 `doc merge` 同一个渲染器。
    pub rendered: String,
    /// 条目在 `items[]` 里的**1 基**序号（就是 `review decide --item N` 的那个 N）。
    pub seq: usize,
    /// 稳定键（`relation:订单→套餐模板` / `entity:客户`）。
    pub label: String,
    /// 条目来源（`doc` / `both`）。
    pub origin: String,
    pub signed_at: String,
    /// `signed_at` 的来源：`arg` / `system` / `legacy`（老产物缺这一栏，按缺省处理）。
    pub signed_at_source: String,
    /// 这一次是不是**幂等命中**（本来就已由同一个人签过）。
    pub already_signed: bool,
    /// 输入文本是否本来就是规范格式（否则 CLI 会警告「本次输出已规范化」）。
    pub input_canonical: bool,
}

/// `--item` 的三种写法。
///
/// ⛔ **不认数组下标**：`items[]` 的顺序会随抽取轮次变，用下标签字的脚本迟早签错人。
/// 序号（1 基）是**给人用的稳定编号**（`propose import` 按它写 `seq`，
/// `review decide --item N` 也按它指），稳定键是给脚本用的。
#[derive(Debug, Clone, PartialEq, Eq)]
enum ItemSpec {
    /// 1 基序号。
    Seq(usize),
    /// `{kind}:{payload.key 或 target}`（如 `relation:订单→套餐模板`、
    /// `entity:customers`、`relation:order_template_id__package_templates`）。
    KindRef(&'static str, String),
    /// `{seq}:{kind}:{target}` —— `ItemView.id` 的原样（`proposal apply` 的报错里就是它）。
    ItemId {
        seq: usize,
        kind: &'static str,
        target: String,
    },
}

/// 类型别名 → 条目的 `kind`。只认这两类（doc 合并只会产出这两类）。
fn kind_norm(raw: &str) -> Result<&'static str> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "relation" | "link" | "edge" | "add_link_type" => Ok("add_link_type"),
        "entity" | "concept" | "type" | "add_entity_type" => Ok("add_entity_type"),
        other => bail!(
            "--item 里的类型 {other:?} 认不出（可用 relation / entity，\
             或原样写 add_link_type / add_entity_type）"
        ),
    }
}

/// `kind` → 稳定键里的短名（与人读报错一致）。
fn kind_alias(kind: &str) -> &'static str {
    match kind {
        "add_link_type" => "relation",
        "add_entity_type" => "entity",
        _ => "item",
    }
}

/// 箭头的三种写法归一：`->` / `⇒` / `→` 都当 `→`。
///
/// 只归一箭头（不是模糊匹配）：`订单→套餐模板` 与 `订单->套餐模板` 是**同一个东西**
/// 的两种敲法，而 `订单到套餐` 不是 —— 后者依然对不上。
fn norm_arrows(s: &str) -> String {
    s.trim().replace("->", "→").replace('⇒', "→")
}

fn parse_item_spec(spec: &str) -> Result<ItemSpec> {
    let s = spec.trim();
    if s.is_empty() {
        bail!(
            "--item 不能为空：给 1 基序号（如 136）或稳定键\
             （如 relation:订单→套餐模板 / entity:customers）"
        );
    }
    if let Ok(n) = s.parse::<usize>() {
        if n == 0 {
            bail!("--item 的序号从 1 开始（0 不存在）");
        }
        return Ok(ItemSpec::Seq(n));
    }
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    match parts.len() {
        2 => Ok(ItemSpec::KindRef(kind_norm(parts[0])?, parts[1].to_string())),
        3 => {
            let seq = parts[0].trim().parse::<usize>().map_err(|_| {
                anyhow::anyhow!(
                    "--item 的第 1 段应当是序号（{s:?}）—— 三种写法：1 基序号 / \
                     relation:订单→套餐模板 / 136:add_link_type:订单→套餐模板"
                )
            })?;
            if seq == 0 {
                bail!("--item 的序号从 1 开始（0 不存在）");
            }
            Ok(ItemSpec::ItemId {
                seq,
                kind: kind_norm(parts[1])?,
                target: parts[2].to_string(),
            })
        }
        _ => bail!(
            "--item 认不出：{s:?}（三种写法：1 基序号 / relation:订单→套餐模板 / \
             136:add_link_type:订单→套餐模板）"
        ),
    }
}

/// 条目的来源（顶层优先，其次 payload —— 导入后两处都有）。
fn origin_of_value(item: &Value) -> Option<&str> {
    item.get("origin")
        .and_then(Value::as_str)
        .or_else(|| {
            item.get("payload")
                .and_then(|p| p.get("origin"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn item_key(item: &Value) -> &str {
    item.get("payload")
        .and_then(|p| p.get("key"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn item_target(item: &Value) -> &str {
    item.get("target").and_then(Value::as_str).unwrap_or("")
}

fn item_kind(item: &Value) -> &str {
    item.get("kind").and_then(Value::as_str).unwrap_or("")
}

/// 列出现有可签条目（找不到 / 歧义时点名，⛔ 不让调用方自己去数）。
fn signable_listing(items: &[Value]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if matches!(origin_of_value(it), Some(ORIGIN_DOC) | Some(ORIGIN_BOTH)) {
            lines.push(format!(
                "    [{}] {}:{}",
                i + 1,
                kind_alias(item_kind(it)),
                item_target(it)
            ));
            if lines.len() >= 20 {
                lines.push("    …（只列前 20 条）".to_string());
                break;
            }
        }
    }
    if lines.is_empty() {
        "    （这份提案里一条 doc/both 条目都没有 —— 先跑 `ontok doc merge`）".to_string()
    } else {
        lines.join("\n")
    }
}

fn resolve_sign_target<'a>(items: &'a [Value], spec: &ItemSpec) -> Result<(usize, &'a Value)> {
    match spec {
        ItemSpec::Seq(n) => {
            let idx = n - 1;
            items.get(idx).map(|it| (*n, it)).ok_or_else(|| {
                anyhow::anyhow!(
                    "--item {n} 超出范围：这份提案只有 {} 条",
                    items.len()
                )
            })
        }
        ItemSpec::KindRef(kind, needle) => {
            let want = norm_arrows(needle);
            let hits: Vec<(usize, &Value)> = items
                .iter()
                .enumerate()
                .filter(|(_, it)| {
                    item_kind(it) == *kind
                        && (item_key(it) == needle.trim() || norm_arrows(item_target(it)) == want)
                })
                .map(|(i, it)| (i + 1, it))
                .collect();
            match hits.len() {
                0 => bail!(
                    "--item {:?} 在提案里找不到（按 {kind} 的 key 或 target 精确匹配）。\n\
                     可签的条目：\n{}",
                    needle,
                    signable_listing(items)
                ),
                1 => Ok(hits[0]),
                _ => {
                    let names: Vec<String> = hits
                        .iter()
                        .map(|(n, it)| format!("[{n}] {}", item_target(it)))
                        .collect();
                    bail!(
                        "--item {:?} 命中 {} 条（{}）—— ⛔ 有歧义就不签，请改用 1 基序号",
                        needle,
                        hits.len(),
                        names.join(" / ")
                    )
                }
            }
        }
        ItemSpec::ItemId { seq, kind, target } => {
            let (n, it) = resolve_sign_target(items, &ItemSpec::Seq(*seq))?;
            if item_kind(it) != *kind || norm_arrows(item_target(it)) != norm_arrows(target) {
                bail!(
                    "--item {:?} 与第 {n} 条对不上：那是 {}:{} —— ⛔ 不按序号硬签",
                    target,
                    kind_alias(item_kind(it)),
                    item_target(it)
                );
            }
            Ok((n, it))
        }
    }
}

/// **签一条 doc/both 条目**（`ontok doc sign` 的唯一实现，纯函数）。
///
/// # 六条判据落在哪
///
/// | # | 约束 | 落点 |
/// | --- | --- | --- |
/// | 1 | `--by` 必填、不许匿名 | 开头的空串判断 |
/// | 2 | `--item` 认序号 / 稳定键，⛔ 不认下标 | [`parse_item_spec`] + [`resolve_sign_target`]（序号是 1 基的，与 `review decide --item N` 同一个 N） |
/// | 3 | 只签 `doc` / `both` | [`origin_of_value`] 判定，其余一律拒绝并说明 |
/// | 4 | 只加 `reviewed_by` + `signed_at` + `signed_at_source` | 末尾的「其余不变」自检：剥掉这三栏后逐字节相同 |
/// | 5 | 重复签幂等 | 同一个人 ⇒ 原样返回（`already_signed`），值不变 |
/// | 6 | 确定性 | 时间由 `at` 参数决定（不给才取系统时钟），同一份输入 + 同一个 `at` ⇒ 逐字节相同 |
/// | 7 | ⭐REQ-130 · 时间来源可审计 | `at_source`（`arg` / `system`）写进 `signed_at_source`；老产物缺这一栏 ⇒ 读回 `legacy`，不报错、不补写 |
///
/// # 为什么 `signed_at` 是一个参数
///
/// 签字时间是**事实**（ADR-005 的溯源），不能像 TTL 导出那样「不写时间」。
/// 所以确定性这条判据的口径是「同一份输入 + 同一组参数」：要逐字节复现就给
/// `--at <RFC3339>`；不给则取系统时钟（那是**这一次签字**的时间）。
///
/// # 为什么来源要单独一栏
///
/// `--at` 是**可选**的（Q-3 的既有裁定），于是同一个字段有两种来历：
/// 人声明的（可复现、可解释）与机器记录的（只属于那一次运行）。审计要能分辨，
/// 所以写 [`SIGNED_AT_SOURCE_KEY`]；这一栏只是**标注来源**，不改变
/// 「`signed_at` 是事实」的判断。
pub fn sign(
    doc_text: &str,
    spec: &str,
    by: &str,
    at: &str,
    at_source: &str,
) -> Result<SignOutcome> {
    let by = by.trim();
    if by.is_empty() {
        bail!(
            "⛔ 不许签成匿名：`--by` 必填（ADR-005：`reviewed_by` 是「谁认可了这条提案」的\
             溯源字段，空串等于把这一栏作废）"
        );
    }
    let at = at.trim();
    if at.is_empty() {
        bail!("签字时间不能为空（`--at` 省略时由 CLI 取当前 UTC 时间）");
    }
    let at_source = at_source.trim();
    if !matches!(at_source, AT_SOURCE_ARG | AT_SOURCE_SYSTEM) {
        bail!(
            "内部错误：signed_at_source 只能是 {AT_SOURCE_ARG:?} / {AT_SOURCE_SYSTEM:?}\
             （收到 {at_source:?}）"
        );
    }
    // 时间必须是可解析的 RFC3339 —— 否则下游（审计 / 排序）读不懂这一栏。
    chrono::DateTime::parse_from_rfc3339(at).map_err(|e| {
        anyhow::anyhow!(
            "签字时间 {at:?} 不是合法的 RFC3339（{e}）；例：2026-10-24T04:00:00Z"
        )
    })?;

    let doc: Value = serde_json::from_str(doc_text).context("--file 不是合法 JSON")?;
    let items = doc
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("--file 顶层没有 `items` 数组 —— 这不是一份提案"))?;

    let item_spec = parse_item_spec(spec)?;
    let (seq, item) = resolve_sign_target(items, &item_spec)?;

    // ── 约束 3：只签 doc / both ──
    let origin = match origin_of_value(item) {
        Some(o @ (ORIGIN_DOC | ORIGIN_BOTH)) => o.to_string(),
        Some(other) => bail!(
            "第 {seq} 条（{}:{}）的 origin = {other:?} —— `doc sign` 只签 doc / both",
            kind_alias(item_kind(item)),
            item_target(item)
        ),
        None => bail!(
            "第 {seq} 条（{}:{}）没有 `origin` ⇒ 它是 **DB 轨**的条目（`ontok-propose` 的产物）。\n\
             签它要走 `ontok review decide`（那条轨的签字记在数据库的双轨状态上）；\n\
             `doc sign` 只签 `ontok doc merge` 产出的 `origin: doc | both` 条目。",
            kind_alias(item_kind(item)),
            item_target(item)
        ),
    };

    let label = format!("{}:{}", kind_alias(item_kind(item)), item_target(item));
    let existing = item
        .get("reviewed_by")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let existing_at = item
        .get(SIGNED_AT_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    // ⭐ REQ-130 · 老产物（REQ-129 的签字）没有这一栏：**读回按缺省处理**，⛔ 不 panic。
    // 缺省值是 `legacy`（「不知道来源」）而不是 `system` —— 老产物里 `arg` 与 `system`
    // 已经分不出来了，猜一个等于伪造审计结论。
    let existing_source = item
        .get(SIGNED_AT_SOURCE_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let mut signed_doc = doc.clone();
    let mut already_signed = false;
    let final_at;
    let final_source;
    match existing {
        // ── 约束 5：同一个人重复签 ⇒ 幂等（值不变） ──
        Some(prev) if prev == by => {
            already_signed = true;
            match existing_at {
                Some(t) => {
                    final_at = t.to_string();
                    // 已有来源原样保留（⛔ 老产物不补写：补写等于给「不知道」编一个答案）。
                    final_source = existing_source
                        .map(str::to_string)
                        .unwrap_or_else(|| AT_SOURCE_LEGACY.to_string());
                }
                None => {
                    // 手工塞过 reviewed_by 却没写时间 ⇒ 补一次（这是修复，不是改判）。
                    final_at = at.to_string();
                    final_source = at_source.to_string();
                    set_signed_at(&mut signed_doc, seq - 1, &final_at)?;
                    set_signed_at_source(&mut signed_doc, seq - 1, &final_source)?;
                }
            }
        }
        // 别人已经签过：⛔ 不覆盖别人的签名（ADR-005：签名是溯源，不是状态位）
        Some(prev) => bail!(
            "第 {seq} 条（{label}）已被 {prev} 签于 {}；⛔ `doc sign` 不覆盖别人的签名\
             （要换人签，先由本人/负责人明确撤销）",
            existing_at.unwrap_or("（未记时间）")
        ),
        None => {
            final_at = at.to_string();
            final_source = at_source.to_string();
            set_signed_at(&mut signed_doc, seq - 1, &final_at)?;
            set_signed_at_source(&mut signed_doc, seq - 1, &final_source)?;
        }
    }
    if !already_signed {
        set_reviewed_by(&mut signed_doc, seq - 1, by)?;
    }

    // ── 约束 4 的自检：剥掉这三栏之后，文档与输入**逐值相同** ──
    let mut probe = signed_doc.clone();
    strip_signature(&mut probe, seq - 1);
    let mut baseline = doc.clone();
    strip_signature(&mut baseline, seq - 1);
    if canonical_text(&probe)? != canonical_text(&baseline)? {
        bail!("内部错误：签字改动了这三栏以外的内容（⛔ 其余字节必须不变）");
    }

    let input_canonical = canonical_text(&doc)? == doc_text;
    let rendered = canonical_text(&signed_doc)?;
    Ok(SignOutcome {
        document: signed_doc,
        rendered,
        seq,
        label,
        origin,
        signed_at: final_at,
        signed_at_source: final_source,
        already_signed,
        input_canonical,
    })
}

fn item_mut(doc: &mut Value, idx: usize) -> Result<&mut Map<String, Value>> {
    doc.get_mut("items")
        .and_then(Value::as_array_mut)
        .and_then(|a| a.get_mut(idx))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow::anyhow!("内部错误：第 {} 条不存在", idx + 1))
}

fn set_reviewed_by(doc: &mut Value, idx: usize, by: &str) -> Result<()> {
    item_mut(doc, idx)?.insert("reviewed_by".to_string(), Value::from(by));
    Ok(())
}

fn set_signed_at(doc: &mut Value, idx: usize, at: &str) -> Result<()> {
    item_mut(doc, idx)?.insert(SIGNED_AT_KEY.to_string(), Value::from(at));
    Ok(())
}

/// ⭐ REQ-130 · 写 `signed_at_source`（**只**在写 `signed_at` 的同一次动作里写）。
///
/// 两者永远同进同退：单独补一栏会造出「有时间没来源」或「有来源没时间」的半状态。
fn set_signed_at_source(doc: &mut Value, idx: usize, source: &str) -> Result<()> {
    item_mut(doc, idx)?.insert(SIGNED_AT_SOURCE_KEY.to_string(), Value::from(source));
    Ok(())
}

fn strip_signature(doc: &mut Value, idx: usize) {
    if let Some(o) = doc
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .and_then(|a| a.get_mut(idx))
        .and_then(Value::as_object_mut)
    {
        o.remove("reviewed_by");
        o.remove(SIGNED_AT_KEY);
        o.remove(SIGNED_AT_SOURCE_KEY);
    }
}

/// 规范渲染：pretty JSON + 末尾换行（与 `doc merge` 同一个形状）。
fn canonical_text(doc: &Value) -> Result<String> {
    let mut text = serde_json::to_string_pretty(doc).context("序列化失败")?;
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

    // ══════════════════════════════════════════════════════
    // Q-2 · `--schema` 的严格校验面
    // ══════════════════════════════════════════════════════

    /// 造一份 `SchemaReport`（只填本 REQ 用得到的字段）。
    fn schema_index(tables: &[(&str, &[&str])]) -> SchemaIndex {
        let tables_json: Vec<Value> = tables
            .iter()
            .map(|(name, cols)| {
                let columns: Vec<Value> = cols
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "name": c,
                            "data_type": "text",
                            "nullable": false,
                            "comment": null,
                            "is_primary_key": *c == "id",
                        })
                    })
                    .collect();
                serde_json::json!({
                    "name": name,
                    "comment": null,
                    "columns": columns,
                    "column_count": cols.len(),
                    "commented_count": 0,
                })
            })
            .collect();
        let v = serde_json::json!({
            "server_version": "test",
            "transaction_read_only": "on",
            "schema": "public",
            "table_count": tables.len(),
            "column_count": 0,
            "commented_column_count": 0,
            "foreign_key_count": 0,
            "tables": tables_json,
            "introspected_at": "2026-10-24T00:00:00Z",
        });
        let report: crate::extract::introspect::SchemaReport =
            serde_json::from_value(v).expect("schema 夹具");
        SchemaIndex::from_report(&report)
    }

    fn maps_new_edge() -> Value {
        serde_json::json!({
            "child": {"table": "order", "column": "template_id"},
            "parent": {"table": "package_templates", "column": "id"}
        })
    }

    /// 给了 `--schema` ⇒ 父端**主键列**（`package_templates.id`）被严格校验通过，
    /// 且 fallback 计数为 0（AC-004 订正：给了 schema 就 0 例外）。
    #[test]
    fn with_schema_the_parent_key_is_validated_strictly() {
        let schema = schema_index(&[
            ("customers", &["id", "customer_name"]),
            ("order", &["id", "customer_id", "template_id"]),
            ("package_templates", &["id", "name"]),
        ]);
        let jsonl = relation_jsonl(Some("1:1"), Some(maps_new_edge()), false);
        let out = merge_with(
            &jsonl,
            &proposal(),
            &MergeOptions {
                schema: Some(schema),
                ..Default::default()
            },
        )
        .expect("合并应当成功");
        assert_eq!(out.stats.schema_checked, true);
        assert_eq!(out.stats.schema_tables, 3);
        assert_eq!(out.stats.merged_doc_only, 1, "父端主键列应当严格通过");
        assert_eq!(
            out.stats.parent_key_id_convention, 0,
            "给了 schema 就不许再走 `id` 约定 fallback"
        );
        assert_eq!(out.stats.parent_column_schema_only, 1, "走的是 schema 严格校验");
        assert!(out.stats.close());
    }

    /// 给了 `--schema` 而 schema 里**没有**这一列 ⇒ `doc_unmapped`（0 例外）。
    #[test]
    fn with_schema_a_column_missing_from_the_schema_is_unmapped() {
        // 父端表存在、但没有 `id` 这一列
        let schema = schema_index(&[
            ("customers", &["customer_name"]),
            ("order", &["customer_id", "template_id"]),
            ("package_templates", &["name"]),
        ]);
        let jsonl = relation_jsonl(Some("N:1"), Some(maps_new_edge()), false);
        let out = merge_with(
            &jsonl,
            &proposal(),
            &MergeOptions {
                schema: Some(schema),
                ..Default::default()
            },
        )
        .expect("合并应当成功");
        assert_eq!(out.stats.merged, 0, "schema 里没有的列一律不并");
        assert_eq!(
            out.stats.unmapped_reasons.get("column_not_in_schema").copied(),
            Some(1),
            "{:?}",
            out.stats.unmapped_reasons
        );
        assert!(out.stats.close());
    }

    /// 给了 `--schema` 而 schema 里**没有**这张表 ⇒ `doc_unmapped`。
    #[test]
    fn with_schema_a_table_missing_from_the_schema_is_unmapped() {
        let schema = schema_index(&[
            ("customers", &["id", "customer_name"]),
            ("order", &["customer_id", "template_id"]),
        ]);
        let jsonl = relation_jsonl(Some("N:1"), Some(maps_new_edge()), false);
        let out = merge_with(
            &jsonl,
            &proposal(),
            &MergeOptions {
                schema: Some(schema),
                ..Default::default()
            },
        )
        .expect("合并应当成功");
        assert_eq!(out.stats.merged, 0);
        assert_eq!(
            out.stats.unmapped_reasons.get("table_not_in_schema").copied(),
            Some(1),
            "{:?}",
            out.stats.unmapped_reasons
        );
    }

    /// 不给 `--schema` ⇒ 保持 fallback，且用量**可数可见**。
    #[test]
    fn without_schema_the_fallback_is_counted_and_visible() {
        let jsonl = relation_jsonl(Some("N:1"), Some(maps_new_edge()), false);
        let out = merge(&jsonl, &proposal()).expect("合并应当成功");
        assert_eq!(out.stats.schema_checked, false);
        assert_eq!(out.stats.merged_doc_only, 1);
        assert_eq!(
            out.stats.parent_key_id_convention, 1,
            "没给 schema 时父端 `id` 只靠约定 ⇒ 必须记数"
        );
        assert_eq!(out.stats.parent_column_schema_only, 0);
        // 摘要块里也要有这一位（文件读者不看 stderr 也知道有没有严格校验）
        assert_eq!(
            out.document["doc_merge"]["schema_checked"],
            serde_json::json!(false)
        );
    }

    // ══════════════════════════════════════════════════════
    // ⭐ REQ-143 · 纯文档通路（`--allow-doc-only`）
    // ══════════════════════════════════════════════════════

    /// 没有关系库的项目：提案只有骨架、`items` 为空。
    fn empty_proposal() -> Value {
        serde_json::json!({
            "project": "req143_doconly",
            "base_version": null,
            "round": 1,
            "items": []
        })
    }

    fn doc_only_opts() -> MergeOptions {
        MergeOptions {
            allow_doc_only: true,
            ..Default::default()
        }
    }

    /// AC-001 的三条断言：两个概念（订单 / 客户）+ 一条关系，**没有任何 `maps_to`**。
    fn three_assertions() -> String {
        [
            concept_jsonl("订单", &[], "fact", true),
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("N:1"), None, false),
        ]
        .join("\n")
    }

    /// 一条关系断言，但**引文/位置可换**（用来验「冲突与行序无关」）。
    fn relation_at(card: &str, quote: &str, span: [usize; 2]) -> String {
        let mut v: Value = serde_json::from_str(&relation_jsonl(Some(card), None, false)).unwrap();
        v["evidence"]["quote"] = serde_json::json!(quote);
        v["evidence"]["span"] = serde_json::json!(span);
        v.to_string()
    }

    /// AC-001 / R1 / R5 · 开关**默认关闭**：三条断言全未映射、不新建任何条目。
    ///
    /// 变异自证（纪律 G①）：把 `MergeOptions::default()` 改成 `allow_doc_only: true`
    /// （或让 CLI 无条件传 true），本测试立刻变红。
    #[test]
    fn doc_only_is_off_by_default_and_leaves_every_assertion_unmapped() {
        assert!(
            !MergeOptions::default().allow_doc_only,
            "R1：开关必须显式，默认关闭"
        );
        let out = merge(&three_assertions(), &empty_proposal()).expect("合并应当成功");
        assert_eq!(out.stats.merged, 0);
        assert_eq!(out.stats.merged_doc_only, 0, "不开开关 ⇒ doc-only 条目必须是 0");
        assert_eq!(out.stats.doc_unmapped, 3);
        assert_eq!(out.stats.doc_only_entities, 0);
        assert_eq!(items(&out.document).len(), 0, "不开开关 ⇒ 一条新条目都不许有");
        // 不开开关时，产物里连这个开关的名字都不该出现（R5 的字节判据的根）
        assert!(
            !out.rendered.contains("allow_doc_only"),
            "不开开关时 merged.json 不许出现 REQ-143 的新键"
        );
        assert!(out.document["doc_merge"].get("doc_only_entities").is_none());
        assert!(
            !out.rendered.contains("skipped_self_link"),
            "不开开关时 merged.json 也不许出现自环计数"
        );
    }

    /// AC-002 / AC-005 的正向：概念没命中 ⇒ 每个概念一个实体；关系两端能解析 ⇒ 一条无 join 列的边。
    #[test]
    fn doc_only_creates_an_entity_per_unmatched_concept_and_a_link_without_a_join_column() {
        let out = merge_with(&three_assertions(), &empty_proposal(), &doc_only_opts())
            .expect("合并应当成功");
        assert_eq!(out.stats.merged, 3);
        assert_eq!(out.stats.doc_unmapped, 0);
        assert_eq!(out.stats.doc_only_entities, 2);
        assert_eq!(out.stats.doc_only_links, 1);
        assert_eq!(out.stats.items_added, 3);
        assert_eq!(out.stats.merged_doc_only, 3);
        assert!(out.stats.close(), "每条断言必须恰好落进一档");

        let order_key = doc_entity_key("订单", &BTreeSet::new());
        let customer_key = doc_entity_key("客户", &BTreeSet::new());
        let order = entity_item(&out.document, &order_key);
        assert_eq!(order["origin"], serde_json::json!("doc"));
        assert_eq!(order["band"], serde_json::json!("low"));
        assert!(order["reviewed_by"].is_null(), "R3：doc 条目必须未签");
        assert_eq!(order["payload"]["display_name"], serde_json::json!("订单"));
        assert_eq!(order["payload"]["properties"], serde_json::json!([]));
        assert_eq!(doc_claims(&out.document, &order_key).len(), 1);

        let link = out
            .document
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|it| it["kind"] == serde_json::json!("add_link_type"))
            .expect("必须新建一条关系");
        assert_eq!(link["origin"], serde_json::json!("doc"));
        assert_eq!(link["band"], serde_json::json!("low"));
        assert!(link["reviewed_by"].is_null());
        assert_eq!(link["payload"]["from_type"], serde_json::json!(order_key));
        assert_eq!(link["payload"]["to_type"], serde_json::json!(customer_key));
        // R4 的另一面：**没有 join 列**（不是编出来的列名，是空串）
        assert_eq!(link["payload"]["evidence"][0]["column"], serde_json::json!(""));
        assert_eq!(link["payload"]["evidence"][0]["table"], serde_json::json!(""));
        assert_eq!(doc_claims(&out.document, link["payload"]["key"].as_str().unwrap()).len(), 1);
    }

    /// AC-003 / R2 · key **只由名字决定**：两次运行逐字节相同；换行序 key 不变。
    ///
    /// 变异自证（纪律 G②）：给 key 拼一个随机数/时间戳/行号，本测试立刻变红。
    #[test]
    fn doc_only_keys_depend_only_on_the_names() {
        let a = merge_with(&three_assertions(), &empty_proposal(), &doc_only_opts()).expect("第一次");
        let b = merge_with(&three_assertions(), &empty_proposal(), &doc_only_opts()).expect("第二次");
        assert_eq!(a.rendered, b.rendered, "同一输入两次 ⇒ merged.json 逐字节相同");

        // 换行序（关系在前、概念在后）⇒ 同一批 key（没有行号/序号成分）
        let reordered = [
            relation_jsonl(Some("N:1"), None, false),
            concept_jsonl("客户", &[], "fact", true),
            concept_jsonl("订单", &[], "fact", true),
        ]
        .join("\n");
        let c =
            merge_with(&reordered, &empty_proposal(), &doc_only_opts()).expect("换序也应当成功");
        for name in ["订单", "客户"] {
            let key = doc_entity_key(name, &BTreeSet::new());
            assert!(entity_item(&a.document, &key)["payload"]["display_name"].is_string());
            assert_eq!(
                entity_item(&c.document, &key)["payload"]["display_name"],
                serde_json::json!(name),
                "换行序不该改 key"
            );
        }

        // 金值：sha256(名字)[:12] —— 派生规则被钉住（改随机成分 ⇒ 红）
        assert_eq!(doc_entity_key("订单", &BTreeSet::new()), "doc_3d5436ea23d2");
        assert_eq!(doc_entity_key("客户", &BTreeSet::new()), "doc_ad4af312dbc2");
        // 纯 ASCII 名字带 slug（可读），中文名字 slug 为空 ⇒ 只有 hash（TTL 建模视图不撞车）
        assert_eq!(doc_entity_key("CLI", &BTreeSet::new()), "doc_cli_e759793341ff");
        assert_eq!(
            doc_link_key("a", "下单", "b"),
            doc_link_key("a", "下单", "b"),
            "关系 key 也必须只由内容决定"
        );
        assert_ne!(doc_link_key("a", "下单", "b"), doc_link_key("a", "退款", "b"));
    }

    /// AC-005 / R4 · 关系任一端解析不到 ⇒ 仍进 `doc_unmapped`，**不造端点、不造边**。
    #[test]
    fn doc_only_never_invents_a_missing_endpoint() {
        // 只给「客户」概念；关系的 subject 是「订单」（没有任何实体叫这个名字）
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("N:1"), None, false),
        ]
        .join("\n");
        let out =
            merge_with(&jsonl, &empty_proposal(), &doc_only_opts()).expect("合并应当成功");
        assert_eq!(out.stats.doc_only_entities, 1);
        assert_eq!(out.stats.doc_only_links, 0, "⛔ 不许凭空造端点");
        assert_eq!(
            out.stats.unmapped_reasons.get("endpoint_not_resolved").copied(),
            Some(1)
        );
        let all = items(&out.document);
        assert!(
            all.iter().all(|it| it["kind"] != serde_json::json!("add_link_type")),
            "解析不到端点的关系，条目里一条边都不许出现"
        );
        assert_eq!(all.len(), 1);
        assert!(out.stats.close());
    }

    /// AC-002 的另一半：命中的概念照旧 `both`，只有**没命中**的才新建。
    #[test]
    fn doc_only_keeps_matched_concepts_as_both_and_only_adds_unmatched_ones() {
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            concept_jsonl("新概念", &[], "fact", true),
        ]
        .join("\n");
        let out = merge_with(&jsonl, &proposal(), &doc_only_opts()).expect("合并应当成功");
        let matched = entity_item(&out.document, "customers");
        assert_eq!(matched["origin"], serde_json::json!("both"));
        assert!(matched["payload"]["doc_evidence"].is_array());
        // DB 侧证据原样保留（`both` 条目不许被 doc 侧盖掉）
        assert!(out.rendered.contains("customer_name"));

        let new_key = doc_entity_key("新概念", &BTreeSet::new());
        let created = entity_item(&out.document, &new_key);
        assert_eq!(created["origin"], serde_json::json!("doc"));
        assert_eq!(created["payload"]["display_name"], serde_json::json!("新概念"));
        assert_eq!(out.stats.doc_only_entities, 1);
        assert_eq!(out.stats.merged_both, 1);
        assert!(out.stats.close());
    }

    /// AC-004 / R3 · **所有**新建条目 `band ∈ {low, medium}` 且 `reviewed_by: null`。
    ///
    /// 变异自证（纪律 G③）：把 [`DOC_ONLY_BAND`] 改成 `"strong"`，本测试立刻变红。
    #[test]
    fn doc_only_items_never_carry_a_band_stronger_than_medium() {
        let out = merge_with(&three_assertions(), &empty_proposal(), &doc_only_opts())
            .expect("合并应当成功");
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
                    "doc-only 条目的 band = {band:?} —— 只许 low / medium（⛔ strong）"
                );
                assert!(
                    it.get("reviewed_by").is_some(),
                    "doc 条目必须显式写 reviewed_by（null）"
                );
                assert!(it["reviewed_by"].is_null(), "doc 条目必须未签");
                bands.insert(band);
            }
        }
        assert_eq!(seen, 3, "夹具必须造出 2 实体 + 1 关系，否则这条判据会空转");
        assert_eq!(bands, ["low".to_string()].into_iter().collect());
        // 摘要块也要能自证（文件读者不看 stderr 也知道档位上限）
        assert_eq!(
            out.document["doc_merge"]["doc_only_band"],
            serde_json::json!(DOC_ONLY_BAND)
        );
    }

    /// 排除 / 显式映射的纪律在开关下**不变**：`future`、`alignable:false` 不造实体，
    /// 有 `maps_to`（哪怕指向提案外的表）也不造 —— 人已经说了它属于哪张表。
    #[test]
    fn doc_only_does_not_create_entities_for_excluded_or_explicitly_mapped_concepts() {
        let mut mapped: Value =
            serde_json::from_str(&concept_jsonl("孤儿概念", &[], "fact", true)).unwrap();
        mapped["maps_to"] = serde_json::json!({"table": "no_such_table"});
        let jsonl = [
            concept_jsonl("想象概念", &[], "future", false),
            concept_jsonl("不该并的概念", &[], "fact", false),
            mapped.to_string(),
        ]
        .join("\n");
        let out =
            merge_with(&jsonl, &empty_proposal(), &doc_only_opts()).expect("合并应当成功");
        assert_eq!(out.stats.doc_only_entities, 0);
        assert_eq!(items(&out.document).len(), 0);
        assert_eq!(out.stats.excluded_not_alignable, 2);
        assert_eq!(out.stats.doc_unmapped, 1);
        assert_eq!(
            out.stats.unmapped_reasons.get("table_not_in_proposal").copied(),
            Some(1)
        );
        assert!(out.stats.close());
    }

    /// REQ-143 的解析裁定：**名字优先于别名**。
    ///
    /// 夹具来自真实样本的形态：概念 `CLI 直采` 的别名是 `CLI`，而 `CLI` 自己也是概念。
    /// 名字优先 ⇒ 两条断言各归各的实体；若按「名字 ∪ 别名」并起来看，两条都会变成
    /// `ambiguous_name`（这就是真实样本里 13 条概念被自己新建的实体挤掉的那个坑）。
    #[test]
    fn doc_only_resolves_each_concept_by_its_own_name_even_when_an_alias_collides() {
        let jsonl = [
            concept_jsonl("CLI", &[], "fact", true),
            concept_jsonl("CLI 直采", &["CLI"], "fact", true),
        ]
        .join("\n");
        let out =
            merge_with(&jsonl, &empty_proposal(), &doc_only_opts()).expect("合并应当成功");
        assert_eq!(out.stats.doc_only_entities, 2);
        assert_eq!(out.stats.merged, 2);
        assert_eq!(out.stats.doc_unmapped, 0);
        let cli = doc_entity_key("CLI", &BTreeSet::new());
        let cli_direct = doc_entity_key("CLI 直采", &BTreeSet::new());
        assert_eq!(doc_claims(&out.document, &cli).len(), 1);
        assert_eq!(doc_claims(&out.document, &cli_direct).len(), 1);
    }

    /// 规则 6 的加强版（REQ-143）：吃**自己的产物**再跑一次 ⇒ 条目逐字节相同、不重复建。
    #[test]
    fn doc_only_merging_its_own_output_is_idempotent() {
        let once = merge_with(&three_assertions(), &empty_proposal(), &doc_only_opts())
            .expect("第一次");
        let twice =
            merge_with(&three_assertions(), &once.document, &doc_only_opts()).expect("第二次");
        assert_eq!(
            serde_json::to_string_pretty(&once.document["items"]).unwrap(),
            serde_json::to_string_pretty(&twice.document["items"]).unwrap(),
            "重跑合并的条目必须逐字节相同（否则人每跑一次都要重看 diff）"
        );
        assert_eq!(twice.stats.doc_only_entities, 0, "第二轮不该再「新建」实体");
        assert_eq!(twice.stats.doc_only_links, 0, "第二轮不该再「新建」关系");
        assert_eq!(twice.stats.items_added, 0);
        assert_eq!(twice.stats.merged, 3);
        assert_eq!(twice.stats.duplicate_claims, 0, "已在文件里的证据不算重复断言");
    }

    // ══════════════════════════════════════════════════════
    // ⭐ REQ-143 · N-1 / N-2（QA 复审的两条一致性要求）
    // ══════════════════════════════════════════════════════

    /// N-1 · doc-vs-doc 基数冲突：**双保留**（`doc_says`/`doc_says_2` + `applicable:false`），
    /// 且**与断言行序无关**。
    ///
    /// 判据（QA）：同一输入打乱顺序 ⇒ 冲突条目数相同、`conflicts` 内容相同。
    /// 这里把判据加强成「整份 `items` 逐字节相同」—— 因为 `cardinality` 也归一到字典序最小。
    #[test]
    fn doc_only_doc_vs_doc_conflicts_keep_both_sides_and_ignore_line_order() {
        let a = relation_at("1:1", "文档说一对一", [10, 14]);
        let b = relation_at("N:1", "文档说多对一", [20, 24]);
        let head = [
            concept_jsonl("订单", &[], "fact", true),
            concept_jsonl("客户", &[], "fact", true),
        ];
        let in_order = [head[0].clone(), head[1].clone(), a.clone(), b.clone()].join("\n");
        let shuffled = [head[0].clone(), head[1].clone(), b, a].join("\n");

        let one = merge_with(&in_order, &empty_proposal(), &doc_only_opts()).expect("原序");
        let two = merge_with(&shuffled, &empty_proposal(), &doc_only_opts()).expect("换序");

        // 两条关系断言**都**进冲突档（⛔ 不「先到者赢」）：合并数只剩两个概念。
        assert_eq!(one.stats.merged, 2, "2 概念；关系的两条说法都进冲突档");
        assert_eq!(one.stats.conflicts, 2, "每种 doc 说法都算一条冲突断言");
        assert_eq!(one.stats.doc_unmapped, 0, "⛔ 冲突不再被静默算成「未映射」");
        assert!(one.stats.close());

        let link = |doc: &Value| -> Value {
            items(doc)
                .into_iter()
                .find(|it| it["kind"] == serde_json::json!("add_link_type"))
                .expect("必须有一条关系")
        };
        let l1 = link(&one.document);
        assert_eq!(l1["applicable"], serde_json::json!(false), "冲突条目不许 apply");
        let c = &l1["conflicts"][0];
        assert_eq!(c["doc_says"], serde_json::json!("1:1"), "两个说法按字典序排");
        assert_eq!(c["doc_says_2"], serde_json::json!("N:1"));
        assert!(
            c.get("db_says").is_none(),
            "doc-vs-doc 冲突不许冒充 db_says：{c}"
        );
        // 双保留：两条 doc 说法都在 doc_evidence 里（证据不丢）
        assert_eq!(
            l1["payload"]["doc_evidence"].as_array().map(Vec::len),
            Some(2),
            "两条 doc 断言都要留证据：{l1}"
        );
        // 打乱顺序 ⇒ 整份 items 逐字节相同（冲突内容 + 归一后的 cardinality）
        assert_eq!(
            serde_json::to_string_pretty(&one.document["items"]).unwrap(),
            serde_json::to_string_pretty(&two.document["items"]).unwrap(),
            "冲突条目数与内容都必须与行序无关"
        );
        // 冲突文档也幂等：吃自己的产物再跑一次，条目不变
        let again =
            merge_with(&in_order, &one.document, &doc_only_opts()).expect("吃自己的产物");
        assert_eq!(
            serde_json::to_string_pretty(&one.document["items"]).unwrap(),
            serde_json::to_string_pretty(&again.document["items"]).unwrap()
        );
    }

    /// N-1 的另一半：**有 `maps_to`** 的 doc-vs-doc 冲突在开关下也双保留；
    /// 不开开关时保持 REQ-129 的 `doc_cardinality_conflict` 未映射口径（R5）。
    #[test]
    fn doc_vs_doc_conflict_on_a_mapped_edge_only_changes_when_the_flag_is_on() {
        let maps = maps_new_edge();
        let mut r1: Value =
            serde_json::from_str(&relation_jsonl(Some("1:1"), Some(maps.clone()), false)).unwrap();
        r1["evidence"]["quote"] = serde_json::json!("一对一说法");
        r1["evidence"]["span"] = serde_json::json!([10, 14]);
        let mut r2: Value =
            serde_json::from_str(&relation_jsonl(Some("N:1"), Some(maps), false)).unwrap();
        r2["evidence"]["quote"] = serde_json::json!("多对一说法");
        r2["evidence"]["span"] = serde_json::json!([20, 24]);
        let jsonl = [r1.to_string(), r2.to_string()].join("\n");

        // 开关打开：双保留 + applicable:false
        let on = merge_with(&jsonl, &proposal(), &doc_only_opts()).expect("开开关");
        assert_eq!(on.stats.conflicts, 1);
        assert_eq!(on.stats.unmapped_reasons.get("doc_cardinality_conflict"), None);
        let link = link_item(&on.document, "order_template_id__package_templates");
        assert_eq!(link["applicable"], serde_json::json!(false));
        assert_eq!(link["conflicts"][0]["doc_says"], serde_json::json!("1:1"));
        assert_eq!(link["conflicts"][0]["doc_says_2"], serde_json::json!("N:1"));

        // 不开开关：**老路径原样** —— 同键的两条带 `maps_to` 的断言不会进 `conflicts[]`
        // （REQ-129 的单轮行为），`doc_cardinality_conflict` 也不会出现。
        // 这里只验「开关才是那条分界线」；逐字节不变由 AC-001 的基线对拍守着（R5）。
        let off = merge(&jsonl, &proposal()).expect("不开开关");
        assert_eq!(off.stats.conflicts, 0);
        let off_link = link_item(&off.document, "order_template_id__package_templates");
        assert_eq!(off_link["applicable"], serde_json::json!(true));
        assert!(
            off_link.get("conflicts").is_none(),
            "不开开关时不许凭空长出 conflicts[]：{off_link}"
        );
    }

    /// N-2 · 自环关系（`subject == object`）在 doc-only 路径**排除并计数**，与老路径同一条纪律。
    #[test]
    fn doc_only_excludes_self_links_and_counts_them() {
        let mut self_link: Value =
            serde_json::from_str(&relation_jsonl(Some("N:1"), None, false)).unwrap();
        self_link["subject"] = serde_json::json!("客户");
        self_link["object"] = serde_json::json!("客户");
        self_link["predicate"] = serde_json::json!("引用自己");
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            self_link.to_string(),
        ]
        .join("\n");

        let out = merge_with(&jsonl, &empty_proposal(), &doc_only_opts()).expect("合并应当成功");
        assert_eq!(out.stats.doc_only_entities, 1, "概念照常新建");
        assert_eq!(out.stats.doc_only_links, 0, "⛔ 自环不建边");
        assert_eq!(out.stats.skipped_self_link, 1);
        assert_eq!(out.stats.unmapped_reasons.get("self_link").copied(), Some(1));
        assert_eq!(out.stats.doc_unmapped, 1);
        assert!(out.stats.close());
        assert!(
            items(&out.document)
                .iter()
                .all(|it| it["kind"] != serde_json::json!("add_link_type")),
            "条目里不许出现自环"
        );
        assert_eq!(
            out.document["doc_merge"]["skipped_self_link"],
            serde_json::json!(1)
        );
    }

    // ══════════════════════════════════════════════════════
    // Q-1 · `ontok doc sign`（文件级签字）
    // ══════════════════════════════════════════════════════

    /// 一份**已经合并过**的提案文本（含一条 doc-only 边 + 一条 both 实体）。
    fn merged_text() -> String {
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("1:1"), Some(maps_new_edge()), false),
        ]
        .join("\n");
        merge(&jsonl, &proposal()).expect("合并应当成功").rendered
    }

    const SIGNED_AT: &str = "2026-10-24T04:00:00Z";

    /// Q-1 · ①：`--by` 空/空白 ⇒ 拒绝（⛔ 不许匿名签）。
    #[test]
    fn sign_rejects_an_anonymous_signer() {
        for by in ["", "   ", "\t"] {
            let err = sign(&merged_text(), "relation:订单→套餐模板", by, SIGNED_AT, AT_SOURCE_ARG)
                .expect_err("匿名字必须被拒");
            assert!(format!("{err:#}").contains("匿名"), "{err:#}");
        }
    }

    /// Q-1 · ②：序号 / 稳定键 / `apply` 报错里那个 id，三种写法都要认。
    #[test]
    fn sign_accepts_seq_stable_key_and_item_id_forms() {
        let text = merged_text();
        let doc: Value = serde_json::from_str(&text).unwrap();
        let n = items(&doc)
            .iter()
            .position(|it| {
                it["origin"] == serde_json::json!("doc")
                    && it["payload"]["key"] == serde_json::json!("order_template_id__package_templates")
            })
            .map(|i| i + 1)
            .expect("夹具里应当有一条 doc-only 边");

        for spec in [
            n.to_string(),
            "relation:订单→套餐模板".to_string(),
            // 箭头写成 ASCII 也算同一个东西
            "relation:订单->套餐模板".to_string(),
            // 按 payload.key 也认
            "relation:order_template_id__package_templates".to_string(),
            // `proposal apply` 报错里的原样（{seq}:{kind}:{target}）
            format!("{n}:add_link_type:订单→套餐模板"),
        ] {
            let out = sign(&text, &spec, "qa-req129", SIGNED_AT, AT_SOURCE_ARG)
                .unwrap_or_else(|e| panic!("--item {spec:?} 应当认得：{e:#}"));
            assert_eq!(out.seq, n, "--item {spec:?} 指错了条目");
            assert_eq!(out.label, "relation:订单→套餐模板");
        }

        // 认不出 / 歧义 / 越界都要拒绝（⛔ 不猜一个最像的）
        for bad in ["relation:不存在的边", "relation:", "999", "entity:订单"] {
            assert!(
                sign(&text, bad, "qa-req129", SIGNED_AT, AT_SOURCE_ARG).is_err(),
                "--item {bad:?} 应当被拒"
            );
        }
    }

    /// Q-1 · ③：只签 `doc` / `both`；DB 条目拒绝并说明走 `review decide`。
    #[test]
    fn sign_refuses_db_track_items() {
        let text = merged_text();
        let doc: Value = serde_json::from_str(&text).unwrap();
        let db_item = items(&doc)
            .into_iter()
            .find(|it| it.get("origin").is_none())
            .expect("夹具里应当有 DB 轨条目");
        let spec = format!(
            "{}:{}",
            kind_alias(item_kind(&db_item)),
            item_target(&db_item)
        );
        let err = sign(&text, &spec, "qa-req129", SIGNED_AT, AT_SOURCE_ARG).expect_err("DB 条目不许签");
        let msg = format!("{err:#}");
        assert!(msg.contains("review decide"), "要说清该走哪条轨：{msg}");
        assert!(msg.contains("origin"), "{msg}");
    }

    /// Q-1 · ④：只加 `reviewed_by` / `signed_at` / `signed_at_source`，**其余逐值不变**。
    #[test]
    fn sign_only_adds_the_signature_fields() {
        let text = merged_text();
        let out = sign(&text, "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG).expect("签");
        let before: Value = serde_json::from_str(&text).unwrap();
        let after = &out.document;

        // 顶层键一个不多一个不少
        let mut kb: Vec<&String> = before.as_object().unwrap().keys().collect();
        let mut ka: Vec<&String> = after.as_object().unwrap().keys().collect();
        kb.sort();
        ka.sort();
        assert_eq!(kb, ka, "顶层键不许变");

        // 逐条比：目标条目多三栏，其它条目**逐值相同**
        let signed_idx = out.seq - 1;
        for (i, (b, a)) in items(&before)
            .iter()
            .zip(items(after).iter())
            .enumerate()
        {
            if i == signed_idx {
                // 把这三栏还原成签字前的值 ⇒ 其余必须逐值相同
                let mut a2 = a.clone();
                a2["reviewed_by"] = b["reviewed_by"].clone();
                a2.as_object_mut().unwrap().remove(SIGNED_AT_KEY);
                a2.as_object_mut().unwrap().remove(SIGNED_AT_SOURCE_KEY);
                assert_eq!(&a2, b, "目标条目除这三栏外不许变");
                assert_eq!(a["reviewed_by"], serde_json::json!("qa-req129"));
                assert_eq!(a[SIGNED_AT_KEY], serde_json::json!(SIGNED_AT));
                assert_eq!(a[SIGNED_AT_SOURCE_KEY], serde_json::json!(AT_SOURCE_ARG));
            } else {
                assert_eq!(a, b, "第 {} 条被改动了", i + 1);
            }
        }
        assert_eq!(out.signed_at, SIGNED_AT);
        assert_eq!(out.signed_at_source, AT_SOURCE_ARG);
        assert_eq!(out.origin, "doc");
        assert!(!out.already_signed);
        assert!(out.input_canonical, "merge 的产物本来就是规范格式");
    }

    /// Q-1 · ⑤：同一个人重复签 ⇒ 幂等（不报错、值不变、字节相同）。
    #[test]
    fn sign_is_idempotent_for_the_same_signer() {
        let text = merged_text();
        let once = sign(&text, "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG).expect("第一次");
        let twice = sign(&once.rendered, "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG)
            .expect("重复签不许报错");
        assert!(twice.already_signed, "幂等命中要能看出来");
        assert_eq!(twice.rendered, once.rendered, "重复签必须逐字节相同");
        assert_eq!(twice.signed_at, SIGNED_AT, "签字时间不许被改写");

        // 换个人签同一 item ⇒ 拒绝（⛔ 不覆盖别人的签名）
        let err = sign(&once.rendered, "relation:订单→套餐模板", "qa-other", SIGNED_AT, AT_SOURCE_ARG)
            .expect_err("别人的签名不许覆盖");
        assert!(format!("{err:#}").contains("不覆盖"), "{err:#}");
    }

    /// ⭐ REQ-130 · AC-003：给了 `--at` ⇒ 来源写 `arg`（人给的声明）。
    #[test]
    fn sign_records_arg_as_the_source_when_the_time_is_given() {
        let out = sign(&merged_text(), "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG)
            .expect("签");
        let signed = &out.document["items"][out.seq - 1];
        assert_eq!(signed[SIGNED_AT_SOURCE_KEY], serde_json::json!(AT_SOURCE_ARG));
        assert_eq!(out.signed_at_source, AT_SOURCE_ARG);
        assert_eq!(
            signed[SIGNED_AT_KEY],
            serde_json::json!(SIGNED_AT),
            "来源与时间必须同进同退"
        );
    }

    /// ⭐ REQ-130 · AC-003：省略 `--at` ⇒ 来源写 `system`（本次运行取的记录）。
    ///
    /// 判据不能只是「值等于 system」：还要证明**时间确实是本次取的系统时钟**，
    /// 否则「恒写 system + 恒写死时间」也能骗过这条。
    #[test]
    fn sign_records_system_as_the_source_when_the_time_comes_from_the_clock() {
        let now = chrono::Utc::now();
        let at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let out = sign(&merged_text(), "relation:订单→套餐模板", "qa-req129", &at, AT_SOURCE_SYSTEM)
            .expect("签");
        let signed = &out.document["items"][out.seq - 1];
        assert_eq!(signed[SIGNED_AT_SOURCE_KEY], serde_json::json!(AT_SOURCE_SYSTEM));
        assert_eq!(out.signed_at_source, AT_SOURCE_SYSTEM);
        let written = chrono::DateTime::parse_from_rfc3339(&out.signed_at).expect("RFC3339");
        assert!(
            (now - written.with_timezone(&chrono::Utc)).num_seconds().abs() <= 2,
            "system 来源的时间必须就是本次取的时钟：{}",
            out.signed_at
        );
    }

    /// ⭐ REQ-130 · AC-004：老产物（REQ-129 写的签字，没有 `signed_at_source`）
    /// 读回来**不报错、不补写**，按 `legacy` 缺省处理。
    ///
    /// ⛔ 不补写的理由：老产物里 `arg` 与 `system` 已经分不出来，
    /// 给它编一个 `system` 就是**伪造审计结论** —— 缺字段比假字段诚实。
    #[test]
    fn sign_reads_a_legacy_artifact_without_a_source_field() {
        let first = sign(&merged_text(), "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG)
            .expect("第一次签");
        let legacy = without_source_field(&first.rendered);
        assert!(
            !legacy.contains(SIGNED_AT_SOURCE_KEY),
            "夹具必须是「老产物」：没有这一栏"
        );

        let again = sign(
            &legacy,
            "relation:订单→套餐模板",
            "qa-req129",
            SIGNED_AT,
            AT_SOURCE_SYSTEM, // 就算这次是系统时钟，也不许改写老签字
        )
        .expect("老产物必须能读回（⛔ 缺字段不是错误）");
        assert!(again.already_signed, "它本来就已经由同一个人签过");
        assert_eq!(again.signed_at_source, AT_SOURCE_LEGACY);
        assert_eq!(again.rendered, legacy, "老产物不补写 ⇒ 字节不变");
    }

    /// Q-1 · ⑥：确定性 —— 同输入同参数（含 `--at`）⇒ 逐字节相同。
    #[test]
    fn sign_is_deterministic_for_the_same_parameters() {
        let text = merged_text();
        let a = sign(&text, "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG).expect("a");
        let b = sign(&text, "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG).expect("b");
        assert_eq!(a.rendered, b.rendered);
        // 换一个签字时间 ⇒ 只有 signed_at 那一行变（证明「变的是参数，不是随机」）
        let c = sign(&text, "relation:订单→套餐模板", "qa-req129", "2026-10-25T00:00:00Z", AT_SOURCE_ARG)
            .expect("c");
        assert_ne!(a.rendered, c.rendered);
        assert_eq!(
            canonical_text(&a.document).unwrap().lines().count() + 0,
            canonical_text(&b.document).unwrap().lines().count()
        );
    }

    /// Q-1 · 时间必须是 RFC3339（否则下游审计读不懂这一栏）。
    #[test]
    fn sign_rejects_a_non_rfc3339_timestamp() {
        let err = sign(&merged_text(), "relation:订单→套餐模板", "qa", "昨天", AT_SOURCE_ARG)
            .expect_err("非法时间必须拒");
        assert!(format!("{err:#}").contains("RFC3339"), "{err:#}");
    }

    /// 重跑合并会清掉签名**和**签字时间（不留「没人签却有签字时间」的状态）。
    #[test]
    fn re_merging_clears_a_previous_signature() {
        let text = merged_text();
        let signed = sign(&text, "relation:订单→套餐模板", "qa-req129", SIGNED_AT, AT_SOURCE_ARG).expect("签");
        assert!(signed.rendered.contains("signed_at"));
        let jsonl = [
            concept_jsonl("客户", &[], "fact", true),
            relation_jsonl(Some("1:1"), Some(maps_new_edge()), false),
        ]
        .join("\n");
        let again = merge(&jsonl, &signed.document).expect("重新合并");
        let item = link_item(&again.document, "order_template_id__package_templates");
        assert!(item["reviewed_by"].is_null(), "重跑合并 ⇒ 未签");
        assert!(item.get(SIGNED_AT_KEY).is_none(), "签字时间也要清掉");
        assert!(
            item.get(SIGNED_AT_SOURCE_KEY).is_none(),
            "签字时间来源也必须一起清掉（否则「没人签、但有来源」）"
        );
        assert_eq!(again.stats.reviewed_by_reset, 1);
    }

    // ── 夹具读取小工具 ──
    /// 把渲染好的文档**去掉** `signed_at_source` 再规范化 —— 模拟 REQ-129 的老产物。
    fn without_source_field(text: &str) -> String {
        let mut v: Value = serde_json::from_str(text).expect("解析");
        for it in v["items"].as_array_mut().expect("items") {
            if let Some(o) = it.as_object_mut() {
                o.remove(SIGNED_AT_SOURCE_KEY);
            }
        }
        canonical_text(&v).expect("规范化")
    }

    fn items(doc: &Value) -> Vec<Value> {        doc["items"].as_array().cloned().unwrap_or_default()
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
