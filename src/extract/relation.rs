//! **REQ-172 · 表间关系探索（第一段）：候选生成 + 值域验证（纯代码、零 token）**
//!
//! 一句话：**`*_id` 只是线索，不是结论。** 本模块把
//! 「名字提名的候选」拿**真实取值**去撞目标表的键，撞上了才升格成强证据。
//!
//! ```text
//! schema.json ─┬─ 命名（复用 fk::FkCatalog::candidates）──┐
//!              ├─ 同名列 / 共享键列（samelenum）──────────┤
//!              └─ 列注释「XX 编号」vs 表注释（comment）───┤
//!                                                       ▼
//!                             每个 *_id 列 ≤N 个候选 ⇒ 只读采样 SELECT DISTINCT
//!                                                       ▼
//!                          candidates.json（evidence[] + score + domain）
//! ```
//!
//! # 三条硬约束（派单简报 R1）
//!
//! | # | 约束 | 落点 |
//! | --- | --- | --- |
//! | 1 | **只读**：连接串强制 `default_transaction_read_only=on` | [`super::sample::sample_pool`]（唯一实现，连上后再验一次 `transaction_read_only`） |
//! | 2 | **抽样不落盘**：样本值只参与判定，不进产物 | [`ProbeReport`] 里没有任何样本值；`domain` 只有计数 |
//! | 3 | **失败要响**：连不上 / 全部查询失败 ⇒ 非 0 并点名 | [`probe`] 的 `bail!`；单条失败进 `stats` 与 `unresolved` |
//!
//! # 为什么不在本模块里判「成立/不成立」
//!
//! 值域只回答「**这些值在不在**」。它回答不了「这条边在业务上是不是这层意思」
//! —— 那是 `relation judge`（AI 两轮）+ 人签的事。本模块不越界：
//! 它只把「值域对上」作为一条**证据**写进候选，让下一段和页面上的人都看得见。
//!
//! # 与 REQ-120 `fk::verify` 的关系
//!
//! 复用它的候选、样本 SQL 与阈值口径，但**产出形状不同**：`fk` 直接出「可签提案」，
//! 本模块出「候选 + 证据」——因为中间还要过 AI 两轮与代码校验。两者共用
//! [`super::fk::FkCatalog::candidates`]，不另写一份提名规则（纪律 H）。

use super::fk::{self, FkCatalog, MatchKind};
use super::introspect::{ColumnMeta, KeyCatalog, SchemaReport, TableMeta};
use super::names::TableNames;
use super::sample;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 候选文件的方法标识（改判据就改它：产物可 diff）。
pub const PROBE_METHOD: &str = "relation-probe-v1";

/// 候选文件的 `kind`。
pub const CANDIDATES_KIND: &str = "relation-candidates";

/// 候选文件版本。
pub const PROBE_VERSION: u32 = 1;

/// 单条只读查询的超时。超时 = 「这次没拿到」，⛔ 不是「值域不匹配」。
pub const DOMAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// 值域判定至少要看几个**不同值**（`1/1 = 100%` 不配叫强证据）。
pub const MIN_DOMAIN_SAMPLES: u32 = 3;

/// `--sample-rows` 的默认值（每个 `*_id` 列取多少个不同值）。
pub const DEFAULT_SAMPLE_ROWS: u32 = 50;

/// `--max-pairs` 的默认值（候选总数上限，防大库爆炸）。
pub const DEFAULT_MAX_PAIRS: usize = 400;

/// 每列最多真的去撞几个候选目标（其余只留命名证据）。
pub const MAX_TARGETS_PER_COLUMN: usize = 3;

/// 关系谓词的**受控词表**（契约 §四 的 16 个主键，唯一实现）。
///
/// ⛔ 不在这里面的词一律丢弃并计数 —— 这是判据 5 的第一条。
pub const PREDICATES: [&str; 16] = [
    "belongs_to",
    "attached_to",
    "binds_to",
    "references",
    "notifies_via",
    "applies_to",
    "forwards_to",
    "targets",
    "bills",
    "based_on",
    "deployed_to",
    "has",
    "depends_on",
    "records",
    "derived_from",
    "contains",
];

/// 一个谓词在不在受控词表里（大小写敏感：契约就是小写）。
pub fn is_predicate(p: &str) -> bool {
    PREDICATES.contains(&p)
}

/// 未签提案的档位（R3 硬要求：`band: low`、`reviewed_by: null`）。
pub const UNSIGNED_BAND: &str = "low";

// ──────────────────────────────────────────────────────────
// 数据结构（候选文件 = probe 的产物、judge 的输入）
// ──────────────────────────────────────────────────────────

/// 一张表的「名片」（不含样本值；样本由 `relation judge` 现取、只进 prompt）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCard {
    pub name: String,
    /// 中文名（表注释的首句；没有 ⇒ `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_cn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<String>,
    #[serde(default)]
    pub unique_columns: Vec<String>,
    pub columns: Vec<ColumnCard>,
}

/// 名片里的一列。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnCard {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl TableCard {
    pub fn from_report(t: &TableMeta, keys: &KeyCatalog, display_cn: Option<String>) -> Self {
        let k = keys.get(&t.name);
        Self {
            name: t.name.clone(),
            display_cn,
            primary_key: k.and_then(|k| k.primary_key.clone()),
            unique_columns: k.map(|k| k.unique_columns.clone()).unwrap_or_default(),
            columns: t
                .columns
                .iter()
                .map(|c| ColumnCard {
                    name: c.name.clone(),
                    data_type: c.data_type.clone(),
                    nullable: c.nullable,
                    is_primary_key: c.is_primary_key,
                    comment: c.comment.clone(),
                })
                .collect(),
        }
    }

    /// 这张表上可作 join 目标的列（主键 + 单列唯一列）。
    pub fn key_columns(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(pk) = &self.primary_key {
            out.push(pk.clone());
        }
        for u in &self.unique_columns {
            if !out.contains(u) {
                out.push(u.clone());
            }
        }
        out
    }

    pub fn has_column(&self, c: &str) -> bool {
        self.columns.iter().any(|x| x.name == c)
    }
}

/// 一条证据（`source` 只有四个合法值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateEvidence {
    /// `naming` / `domain` / `samelenum` / `comment`。
    pub source: String,
    /// 人话依据（点名到列/注释/计数）。
    pub detail: String,
    /// 值域证据的命中数（只有 `domain` 有）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched: Option<u32>,
    /// 值域证据的样本数（只有 `domain` 有）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled: Option<u32>,
}

/// 值域验证的结果（**只有计数，没有样本值** —— 抽样不落盘）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainCheck {
    /// 真的跑过查询了吗（`false` = 类型不可比 / 没跑）。
    pub tested: bool,
    /// 被撞的不同值个数。
    pub sampled: u32,
    /// 落在目标键里的个数。
    pub matched: u32,
    /// `sampled ≥ 3 && matched == sampled` ⇒ 值域 ⊆ 目标键。
    pub subset: bool,
    /// 没跑/没验成的原因（`type_mismatch` / `timeout` / `permission_denied` …）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl DomainCheck {
    fn skipped(reason: &str) -> Self {
        Self {
            tested: false,
            sampled: 0,
            matched: 0,
            subset: false,
            reason: Some(reason.to_string()),
        }
    }
}

/// 一条候选关系（probe 的产物）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationCandidate {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
    pub parent_column: String,
    /// 目标列是 `primary_key` / `unique_column`。
    pub target_kind: String,
    /// 证据（至少一条）。
    pub evidence: Vec<CandidateEvidence>,
    /// 0–1（确定性：只由证据组合算出来，不含随机量）。
    pub score: f64,
    /// `high` / `medium` / `low`。
    pub band: String,
    /// 值域验证结果（没测 ⇒ 不出现）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<DomainCheck>,
    /// 子列在源表的唯一约束里 ⇒ 这条边是 1:1，否则 N:1。
    pub child_unique: bool,
}

impl RelationCandidate {
    /// 提案/判定用的稳定 id：`child.col->parent.key`。
    pub fn id(&self) -> String {
        format!(
            "{}.{}->{}.{}",
            self.child_table, self.child_column, self.parent_table, self.parent_column
        )
    }

    /// 基数（R4 口径：**只看目录事实**，不靠样本猜）。
    pub fn cardinality(&self) -> &'static str {
        if self.child_unique {
            "1:1"
        } else {
            "N:1"
        }
    }
}

/// 没产出候选的列（⛔ 不许静默跳过）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeUnresolved {
    pub table: String,
    pub column: String,
    /// `no_candidate` / `type_mismatch` / `sample_failed` / `insufficient_sample`。
    pub code: String,
    pub reason: String,
}

/// probe 的统计（闭合性可核对）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeStats {
    /// 参与提名的 `*_id` 列数（`fk::FkCatalog::pending_columns`）。
    pub columns_considered: usize,
    /// 提名出来的候选数（去重后、截断前）。
    pub candidates_generated: usize,
    /// 最终写进产物、进入 AI 判定的候选数。
    pub candidates_kept: usize,
    /// 被 `--max-pairs` 截掉的候选数。
    pub truncated_pairs: usize,
    /// 真的跑了值域查询的候选数。
    pub domain_tested: usize,
    /// 值域确认（`subset = true`）的候选数。
    pub domain_confirmed: usize,
    /// 值域否定的候选数（测了但没全中）。
    pub domain_rejected: usize,
    /// 值域查询失败/超时的次数。
    pub domain_failed: usize,
    /// 带 `samelenum` 证据的候选数。
    pub samelenum: usize,
    /// 带 `comment` 证据的候选数。
    pub comment: usize,
}

/// 产物来源（**不含口令**：DSN 只留 host/db）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeSource {
    pub schema_file: String,
    pub schema_sha256: String,
    pub schema: String,
    /// 只读凭据（连上后读回的 `transaction_read_only`，必须是 `on`）。
    pub read_only: String,
    /// `host:port/db`（⛔ 没有用户名口令）。
    pub dsn: String,
    pub tables: usize,
    pub columns: usize,
    /// `--sample-rows`（进产物：同输入同样本窗口 ⇒ 同一份结果）。
    pub sample_rows: u32,
}

/// **候选文件**（`relation-candidates.json`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeReport {
    pub version: u32,
    pub kind: String,
    pub method: String,
    pub source: ProbeSource,
    /// 表名片（judge 的输入；不含样本值）。
    pub tables: Vec<TableCard>,
    pub candidates: Vec<RelationCandidate>,
    pub unresolved: Vec<ProbeUnresolved>,
    pub stats: ProbeStats,
}

impl ProbeReport {
    /// 排成确定性顺序（两次运行逐字节相同）。
    pub fn sort(&mut self) {
        self.tables.sort_by(|a, b| a.name.cmp(&b.name));
        self.candidates.sort_by(|a, b| {
            a.child_table
                .cmp(&b.child_table)
                .then_with(|| a.child_column.cmp(&b.child_column))
                .then_with(|| a.parent_table.cmp(&b.parent_table))
                .then_with(|| a.parent_column.cmp(&b.parent_column))
        });
        self.unresolved
            .sort_by(|a, b| a.table.cmp(&b.table).then_with(|| a.column.cmp(&b.column)));
    }

    /// 渲染成**逐字节可复现**的 JSON。
    pub fn render(&self) -> Result<String> {
        let mut r = self.clone();
        r.sort();
        Ok(format!("{}\n", serde_json::to_string_pretty(&r)?))
    }

    pub fn card(&self, table: &str) -> Option<&TableCard> {
        self.tables.iter().find(|t| t.name == table)
    }
}

// ──────────────────────────────────────────────────────────
// schema 装载（probe / judge / build run 共用）
// ──────────────────────────────────────────────────────────

/// 一份 schema.json 的读回值（勘查报告 + 键目录）。
#[derive(Debug, Clone)]
pub struct SchemaFile {
    pub report: SchemaReport,
    pub keys: KeyCatalog,
    pub sha256: String,
}

/// 读 `schema.json`（`ontok-builder introspect` 的产物）。
///
/// ⛔ 缺 `primary_key` / `unique_columns` **不是**「这张表没有主键」，
/// 而是「这份文件不是本命令要的那种」—— `KeyCatalog::from_product` 读不到键时
/// 会让所有候选都验不了，静默下去的结果是「0 条关系」看起来像「库里没有关系」。
pub fn load_schema(path: &std::path::Path) -> Result<SchemaFile> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读不到 schema 文件：{}", path.display()))?;
    let product: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("schema 文件不是合法 JSON：{}", path.display()))?;
    let report: SchemaReport = serde_json::from_value(product.clone())
        .with_context(|| format!("schema 文件不是 SchemaReport 形状：{}", path.display()))?;
    let keys = KeyCatalog::from_product(&product);
    if report.table_count > 0 && keys.is_empty() {
        bail!(
            "schema 文件里**没有任何键事实**（primary_key / unique_columns）：{}。\n\
             候选表用哪一列验命中是**猜不出来**的 —— 先跑 `ontok-builder introspect --out` \
             重出一份（REQ-108 之后才带键）。",
            path.display()
        );
    }
    Ok(SchemaFile {
        report,
        keys,
        sha256: sample::sha256_hex(raw.as_bytes()),
    })
}

/// 从表注释里取一个**短中文名**（首个短句，最长 20 字）。
///
/// 为什么不是整段注释：真实库的表注释常常是一句话
/// （「用户套餐订阅的变更记录…」）。整段当名字的话，
/// `comment_mentions`（要求列注释包含整个名字）永远不会命中 —— 等于没有名字。
pub fn display_hint(comment: Option<&str>) -> Option<String> {
    let c = comment?.trim();
    if c.is_empty() {
        return None;
    }
    let head: String = c
        .chars()
        .take_while(|ch| !matches!(ch, '，' | ',' | '。' | '.' | '；' | ';' | '\n' | '\r' | ' '))
        .collect();
    let head = head.trim();
    if head.is_empty() {
        return None;
    }
    Some(head.chars().take(20).collect())
}

/// 由表注释拼一份 `TableNames`（不读外部翻译表 —— UAT 没有翻译表）。
pub fn names_from_comments(report: &SchemaReport) -> TableNames {
    let mut names = std::collections::HashMap::new();
    for t in &report.tables {
        if let Some(h) = display_hint(t.comment.as_deref()) {
            // 与表名相同的中文名会被 `usable_as_human_name` 当退化值 —— 但这里只是
            // 给提名用的，重复也无害；仍挡一道，免得 `comment_mentions` 拿表名当名字。
            if h.to_lowercase() != t.name.to_lowercase() {
                names.insert(t.name.clone(), h);
            }
        }
    }
    TableNames {
        agent: "schema-comments".to_string(),
        version: "v1".to_string(),
        generated_at: String::new(),
        source: "schema.json#table.comment".to_string(),
        names,
    }
}

/// 把 DSN 变成**可入产物**的 `host:port/db`（⛔ 用户名口令一律不进产物）。
pub fn redact_dsn(dsn: &str) -> String {
    let after = dsn.rsplit_once('@').map(|(_, tail)| tail).unwrap_or(dsn);
    let base = after.split('?').next().unwrap_or(after);
    if base.is_empty() {
        "<dsn>".to_string()
    } else {
        base.to_string()
    }
}

// ──────────────────────────────────────────────────────────
// 候选提名（确定性，不碰库）
// ──────────────────────────────────────────────────────────

/// 证据来源的稳定顺序（同一候选多来源时输出顺序确定）。
fn evidence_rank(source: &str) -> u8 {
    match source {
        "naming" => 0,
        "comment" => 1,
        "samelenum" => 2,
        "domain" => 3,
        _ => 4,
    }
}

/// 命名档 → 基础分（**唯一实现**；改分数就是改判据）。
fn naming_score(kind: MatchKind) -> f64 {
    match kind {
        MatchKind::Exact => 0.55,
        MatchKind::Plural => 0.53,
        MatchKind::Suffix => 0.48,
        MatchKind::Substring => 0.42,
        MatchKind::CommentMention => 0.45,
    }
}

/// 把一组证据算成一个 0–1 的分（确定性：顺序无关、无浮点累积）。
pub fn score_of(evidence: &[CandidateEvidence], domain: Option<&DomainCheck>) -> f64 {
    let mut base: f64 = 0.0;
    for e in evidence {
        if e.source == "naming" {
            // `detail` 里不带档位，档位由调用方在生成时折进 `naming_score`；
            // 这里只需要「有命名证据」的下限（真正的分在 push 时给过）。
            base = base.max(0.40);
        } else if e.source == "comment" {
            base += 0.10;
        } else if e.source == "samelenum" {
            base += 0.08;
        }
    }
    if let Some(d) = domain {
        if d.subset {
            let coverage = if d.sampled == 0 {
                0.0
            } else {
                d.matched as f64 / d.sampled as f64
            };
            let s = 0.90 + 0.05 * coverage;
            return (s * 100.0).round() / 100.0;
        }
    }
    // 没有值域证据时**永远低于** 0.90：强证据只能来自数据（或 AI，另一段的事）。
    let s = base.min(0.85);
    (s * 100.0).round() / 100.0
}

/// 分档（供页面与统计用；**不影响判定** —— 判定看证据）。
pub fn band_of(score: f64) -> &'static str {
    if score >= 0.80 {
        "high"
    } else if score >= 0.50 {
        "medium"
    } else {
        "low"
    }
}

/// 从列注释里取「XX 编号」的 **XX**（注释语义提名的入口）。
///
/// 只认中文/字母词干 + 编号类后缀；取不到 ⇒ `None`（⛔ 不拿整句注释去模糊匹配）。
pub fn comment_entity_token(comment: &str) -> Option<String> {
    let c = comment.trim();
    if c.is_empty() {
        return None;
    }
    const SUFFIXES: [&str; 8] = ["编号", "标识", "代码", "编码", "序号", "ID", "Id", "id"];
    for suf in SUFFIXES {
        if let Some(head) = c.strip_suffix(suf) {
            let head = head
                .trim()
                .trim_end_matches(['（', '(', '：', ':', ' ', '的'])
                .trim();
            if head.chars().count() >= 2 {
                return Some(head.to_string());
            }
        }
    }
    None
}

/// 两个中文名是不是**同一实体**的字面匹配（相等 / 前缀，最短 2 字）。
fn same_entity(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    if a.chars().count() < 2 || b.chars().count() < 2 {
        return false;
    }
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// 一条「原始候选」（同 (child, parent, key) 的多次提名合并成一条）。
#[derive(Debug, Clone)]
struct RawCandidate {
    child_table: String,
    child_column: String,
    parent_table: String,
    parent_column: String,
    target_kind: String,
    evidence: Vec<CandidateEvidence>,
    /// 命名档的最高分（没有命名证据 ⇒ `None`）。
    naming_base: Option<f64>,
}

impl RawCandidate {
    fn add_evidence(&mut self, e: CandidateEvidence) {
        if self
            .evidence
            .iter()
            .any(|x| x.source == e.source && x.detail == e.detail)
        {
            return;
        }
        self.evidence.push(e);
        self.evidence.sort_by(|a, b| {
            evidence_rank(&a.source)
                .cmp(&evidence_rank(&b.source))
                .then_with(|| a.detail.cmp(&b.detail))
        });
    }

    fn has(&self, source: &str) -> bool {
        self.evidence.iter().any(|e| e.source == source)
    }
}

fn raw_key(
    child_table: &str,
    child_column: &str,
    parent_table: &str,
    parent_column: &str,
) -> String {
    format!("{child_table}.{child_column}->{parent_table}.{parent_column}")
}

fn card_of<'a>(cards: &'a [TableCard], name: &str) -> Option<&'a TableCard> {
    cards.iter().find(|c| c.name == name)
}

/// 提名：命名 + 注释 + 同名列（**不碰库**）。
///
/// `report` 只提供列清单；键事实来自 `keys`。返回按 `(child, parent, key)` 去重的候选表。
fn nominate(report: &SchemaReport, keys: &KeyCatalog, names: &TableNames) -> Vec<RawCandidate> {
    let catalog = FkCatalog::build(report, keys, names);
    let cards: Vec<TableCard> = report
        .tables
        .iter()
        .map(|t| TableCard::from_report(t, keys, names.lookup(&t.name).map(str::to_string)))
        .collect();
    let mut out: BTreeMap<String, RawCandidate> = BTreeMap::new();

    // ── ① 命名（复用 fk 的提名：词干/复数/尾缀/词元/注释提到表）─────
    for (table, column) in FkCatalog::pending_columns(report) {
        for c in catalog.candidates(table, column) {
            let Some(parent) = card_of(&cards, &c.table) else {
                continue;
            };
            let Some(key) = c.key_column.as_deref() else {
                continue;
            };
            // 目标列必须真的可 join（主键或单列唯一列）——否则这条候选连
            // 「值域验证」的机会都不该有，更不该进 AI 的 prompt 浪费 token。
            if !parent.key_columns().iter().any(|k| k == key) {
                continue;
            }
            let k = raw_key(table, &column.name, &c.table, key);
            let e = RawCandidate {
                child_table: table.to_string(),
                child_column: column.name.clone(),
                parent_table: c.table.clone(),
                parent_column: key.to_string(),
                target_kind: c.target_kind.as_str().to_string(),
                evidence: vec![CandidateEvidence {
                    source: "naming".to_string(),
                    detail: format!(
                        "命名：列 {} 的词干 {:?} 命中表 {}（{}）",
                        column.name,
                        c.stem,
                        c.table,
                        c.kind.cn()
                    ),
                    matched: None,
                    sampled: None,
                }],
                naming_base: Some(naming_score(c.kind)),
            };
            match out.get_mut(&k) {
                Some(prev) => {
                    if let Some(b) = e.naming_base {
                        prev.naming_base = Some(prev.naming_base.map_or(b, |x| x.max(b)));
                    }
                    for ev in e.evidence {
                        prev.add_evidence(ev);
                    }
                }
                None => {
                    out.insert(k, e);
                }
            }
        }
    }

    // ── ② 同名列 / 共享键列（samelenum）─────────────────────────
    //
    // 只在「父表这一列是主键/唯一列」时提名：同名列不是键时，join 过去
    // 定位不到任何一行 —— 那种候选只会被校验层丢掉，白花 AI 的钱。
    let mut by_column: BTreeMap<&str, Vec<(&TableMeta, &ColumnMeta)>> = BTreeMap::new();
    for t in &report.tables {
        for c in &t.columns {
            by_column.entry(c.name.as_str()).or_default().push((t, c));
        }
    }
    for (col_name, places) in by_column {
        if places.len() < 2 {
            continue;
        }
        let key_owners: Vec<&(&TableMeta, &ColumnMeta)> = places
            .iter()
            .filter(|(t, _)| {
                keys.get(&t.name).map_or(false, |k| {
                    k.primary_key.as_deref() == Some(col_name)
                        || k.unique_columns.iter().any(|u| u == col_name)
                })
            })
            .collect();
        if key_owners.is_empty() {
            continue;
        }
        for (parent, _) in &key_owners {
            for (child_t, child_c) in &places {
                if child_t.name == parent.name {
                    continue;
                }
                // 子列自己是主键 ⇒ 这条「同名列」其实是各表自己的 id，不是关系。
                if child_c.is_primary_key {
                    continue;
                }
                let kind = if keys
                    .get(&parent.name)
                    .and_then(|k| k.primary_key.as_deref())
                    == Some(col_name)
                {
                    "primary_key"
                } else {
                    "unique_column"
                };
                let k = raw_key(&child_t.name, col_name, &parent.name, col_name);
                let e = RawCandidate {
                    child_table: child_t.name.clone(),
                    child_column: col_name.to_string(),
                    parent_table: parent.name.clone(),
                    parent_column: col_name.to_string(),
                    target_kind: kind.to_string(),
                    evidence: vec![CandidateEvidence {
                        source: "samelenum".to_string(),
                        detail: format!(
                            "同名列：{} 张表都有 {:?}，且 {}.{} 是{}",
                            places.len(),
                            col_name,
                            parent.name,
                            col_name,
                            if kind == "primary_key" {
                                "主键"
                            } else {
                                "唯一列"
                            }
                        ),
                        matched: None,
                        sampled: None,
                    }],
                    naming_base: None,
                };
                match out.get_mut(&k) {
                    Some(prev) => {
                        for ev in e.evidence {
                            prev.add_evidence(ev);
                        }
                    }
                    None => {
                        out.insert(k, e);
                    }
                }
            }
        }
    }

    // ── ③ 列注释语义（comment）──────────────────────────────────
    //
    // 只在**命名一个候选都没提到这张父表**时补一条（名字比注释硬）。
    for t in &report.tables {
        for c in &t.columns {
            let Some(comment) = c.comment.as_deref() else {
                continue;
            };
            let Some(token) = comment_entity_token(comment) else {
                continue;
            };
            let Some(parent) = cards.iter().find(|p| {
                p.name != t.name
                    && p.display_cn
                        .as_deref()
                        .map(|cn| same_entity(&token, cn))
                        .unwrap_or(false)
                    && !p.key_columns().is_empty()
            }) else {
                continue;
            };
            let key = parent.key_columns()[0].clone();
            let already = out.values().any(|x| {
                x.child_table == t.name && x.child_column == c.name && x.parent_table == parent.name
            });
            if already {
                continue;
            }
            let k = raw_key(&t.name, &c.name, &parent.name, &key);
            out.insert(
                k,
                RawCandidate {
                    child_table: t.name.clone(),
                    child_column: c.name.clone(),
                    parent_table: parent.name.clone(),
                    parent_column: key.clone(),
                    target_kind: "primary_key".to_string(),
                    evidence: vec![CandidateEvidence {
                        source: "comment".to_string(),
                        detail: format!(
                            "注释：列注释 {:?} 的「{}」与表 {} 的中文名「{}」是同一实体",
                            comment.trim(),
                            token,
                            parent.name,
                            parent.display_cn.as_deref().unwrap_or("")
                        ),
                        matched: None,
                        sampled: None,
                    }],
                    naming_base: None,
                },
            );
        }
    }

    let mut v: Vec<RawCandidate> = out.into_values().collect();
    // 确定性：按 (child, col, parent, key) 排 —— 采样顺序也因此固定。
    v.sort_by(|a, b| {
        a.child_table
            .cmp(&b.child_table)
            .then_with(|| a.child_column.cmp(&b.child_column))
            .then_with(|| a.parent_table.cmp(&b.parent_table))
            .then_with(|| a.parent_column.cmp(&b.parent_column))
    });
    v
}

// ──────────────────────────────────────────────────────────
// 值域验证（只读）
// ──────────────────────────────────────────────────────────

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn qualified(schema: &str, table: &str, column: &str) -> String {
    format!(
        "{}.{}.{}",
        quote_ident(schema),
        quote_ident(table),
        quote_ident(column)
    )
}

/// 类型族（只用来判「能不能拿两个值比一比」）。
fn type_family(t: &str) -> &'static str {
    let t = t.to_ascii_lowercase();
    if t.contains("int")
        || t.contains("numeric")
        || t.contains("decimal")
        || t.contains("real")
        || t.contains("double")
        || t.contains("serial")
        || t.contains("money")
    {
        "num"
    } else if t.contains("timestamp") || t.contains("date") || t.contains("time") {
        "time"
    } else if t == "boolean" || t == "bool" {
        "bool"
    } else {
        "text"
    }
}

/// 两列的类型能不能比较（不同族 ⇒ 不测值域，记 `type_mismatch`）。
pub fn comparable(a: &str, b: &str) -> bool {
    type_family(a) == type_family(b)
}

/// 一个值域查询的失败原因（**不吞错**：失败 ≠ 不匹配）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    Timeout,
    PermissionDenied,
    Empty,
    Db(String),
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            DomainError::Timeout => "timeout",
            DomainError::PermissionDenied => "permission_denied",
            DomainError::Empty => "all_null",
            DomainError::Db(_) => "query_error",
        }
    }
}

fn classify_db_error(e: &sqlx::Error) -> DomainError {
    if let sqlx::Error::Database(db) = e {
        if db.code().as_deref() == Some("42501") {
            return DomainError::PermissionDenied;
        }
        return DomainError::Db(db.message().to_string());
    }
    DomainError::Db(e.to_string())
}

/// 取一列的前 N 个**不同**值（`ORDER BY 1` ⇒ 同输入同输出）。
///
/// ⚠️ 返回的 `Vec<String>` **只活在内存里**：调用方只拿它的长度与命中数，
/// 一个值都不写进产物（R1 的「抽样不落盘」）。
pub async fn sample_distinct(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
    limit: u32,
) -> std::result::Result<Vec<String>, DomainError> {
    let sql = format!(
        "SELECT DISTINCT {col}::text AS v FROM {src} \
         WHERE {col} IS NOT NULL AND {col}::text <> '' ORDER BY 1 LIMIT $1",
        col = qualified(schema, table, column),
        src = format!("{}.{}", quote_ident(schema), quote_ident(table)),
    );
    let rows = tokio::time::timeout(
        DOMAIN_TIMEOUT,
        sqlx::query_scalar::<_, String>(&sql)
            .bind(limit as i64)
            .fetch_all(pool),
    )
    .await
    .map_err(|_| DomainError::Timeout)?
    .map_err(|e| classify_db_error(&e))?;
    if rows.is_empty() {
        return Err(DomainError::Empty);
    }
    Ok(rows)
}

/// 有几个样本值落在目标表的键列里（`COUNT(*)`，目标键唯一 ⇒ 等于命中值个数）。
pub async fn count_key_hits(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
    values: &[String],
) -> std::result::Result<u32, DomainError> {
    if values.is_empty() {
        return Ok(0);
    }
    let sql = format!(
        "SELECT COUNT(*)::bigint FROM {dst} WHERE {col}::text = ANY($1::text[])",
        dst = format!("{}.{}", quote_ident(schema), quote_ident(table)),
        col = qualified(schema, table, column),
    );
    let n: i64 = tokio::time::timeout(
        DOMAIN_TIMEOUT,
        sqlx::query_scalar::<_, i64>(&sql)
            .bind(values)
            .fetch_one(pool),
    )
    .await
    .map_err(|_| DomainError::Timeout)?
    .map_err(|e| classify_db_error(&e))?;
    Ok(n.max(0).min(values.len() as i64) as u32)
}

/// 值域判定：样本 ≥ [`MIN_DOMAIN_SAMPLES`] 且全部命中 ⇒ `subset`。
pub fn domain_verdict(sampled: u32, matched: u32) -> DomainCheck {
    DomainCheck {
        tested: true,
        sampled,
        matched,
        subset: sampled >= MIN_DOMAIN_SAMPLES && matched == sampled,
        reason: if sampled < MIN_DOMAIN_SAMPLES {
            Some("insufficient_sample".to_string())
        } else if matched < sampled {
            Some("partial_overlap".to_string())
        } else {
            None
        },
    }
}

// ──────────────────────────────────────────────────────────
// 全库跑一遍
// ──────────────────────────────────────────────────────────

/// probe 的参数。
#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// 每列取多少个不同值（默认 [`DEFAULT_SAMPLE_ROWS`]）。
    pub sample_rows: u32,
    /// 候选总数上限（默认 [`DEFAULT_MAX_PAIRS`]）。
    pub max_pairs: usize,
    /// 每列最多撞几个候选目标（默认 [`MAX_TARGETS_PER_COLUMN`]）。
    pub max_targets_per_column: usize,
    /// 产物里记的 schema 文件名（只做留痕）。
    pub schema_file: String,
    pub schema_sha256: String,
    /// 连接串（**入产物前会脱敏**成 `host:port/db`）。
    pub dsn: String,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            sample_rows: DEFAULT_SAMPLE_ROWS,
            max_pairs: DEFAULT_MAX_PAIRS,
            max_targets_per_column: MAX_TARGETS_PER_COLUMN,
            schema_file: String::new(),
            schema_sha256: String::new(),
            dsn: String::new(),
        }
    }
}

/// 值域采样的一次汇总（按列缓存，候选之间共享）。
struct ColumnProbe {
    values: Option<Vec<String>>,
    error: Option<DomainError>,
}

/// **全库跑一遍**：提名 → 值域验证 → 每列选一条 → 产物。
///
/// 只读由调用方的池保证（[`super::sample::sample_pool`] 强制
/// `default_transaction_read_only=on` 并读回校验）；本函数只发 SELECT。
pub async fn probe(pool: &PgPool, file: &SchemaFile, opts: &ProbeOptions) -> Result<ProbeReport> {
    let report = &file.report;
    let keys = &file.keys;
    let names = names_from_comments(report);
    let cards: Vec<TableCard> = report
        .tables
        .iter()
        .map(|t| TableCard::from_report(t, keys, names.lookup(&t.name).map(str::to_string)))
        .collect();

    // 只读凭据（AC-101 口径：产物里那一行必须是读回来的，不是抄的）。
    let read_only: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(pool)
        .await
        .context("读不回 transaction_read_only，只读保证无法验证")?;
    if read_only != "on" {
        bail!(
            "relation probe 拒绝在 transaction_read_only = {read_only} 的连接上采样：\
             值域验证必须只读（R1）。"
        );
    }

    let mut stats = ProbeStats::default();
    let mut unresolved: Vec<ProbeUnresolved> = Vec::new();
    let raw = nominate(report, keys, &names);
    stats.candidates_generated = raw.len();
    let pending_count = FkCatalog::pending_columns(report).len();
    stats.columns_considered = pending_count;

    // 按列分组：同一列只采样一次。
    let mut groups: BTreeMap<(String, String), Vec<RawCandidate>> = BTreeMap::new();
    for r in raw {
        groups
            .entry((r.child_table.clone(), r.child_column.clone()))
            .or_default()
            .push(r);
    }

    // 先给所有命名列判一次「有没有候选」（`*_id` 列无候选 ⇒ 进未解析）。
    let mut pending_columns: Vec<(String, String)> = FkCatalog::pending_columns(report)
        .into_iter()
        .map(|(t, c)| (t.to_string(), c.name.clone()))
        .collect();
    pending_columns.sort();

    // 其它列（samelenum / comment 提名）只按同名/注释产生，不参与「无候选」统计。
    let mut column_probes: BTreeMap<(String, String), ColumnProbe> = BTreeMap::new();
    let mut queries_attempted = 0usize;
    let mut queries_failed = 0usize;
    let mut first_error: Option<String> = None;
    let mut kept: Vec<RelationCandidate> = Vec::new();
    let mut domain_by_column: BTreeMap<(String, String), DomainCheck> = BTreeMap::new();

    for (group_key, mut cands) in groups {
        let (child_table, child_column) = group_key.clone();
        // 候选排序：命名档优先，再按表名/键列 —— 只撞前 N 个。
        cands.sort_by(|a, b| {
            let sa = a.naming_base.unwrap_or(0.0);
            let sb = b.naming_base.unwrap_or(0.0);
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.parent_table.cmp(&b.parent_table))
                .then_with(|| a.parent_column.cmp(&b.parent_column))
        });

        let child_card = cards.iter().find(|c| c.name == child_table);
        let child_unique = child_card
            .map(|c| c.unique_columns.iter().any(|u| u == &child_column))
            .unwrap_or(false);
        let child_is_id = fk::fk_stem(&child_column).is_some();

        // 值域：只有 `*_id` 列才有资格（派单简报 R1 的原文）。
        let mut chosen_idx = 0usize;
        let mut domain_for_best: Option<DomainCheck> = None;
        if child_is_id {
            let probe_once =
                column_probes
                    .entry(group_key.clone())
                    .or_insert_with(|| ColumnProbe {
                        values: None,
                        error: None,
                    });
            if probe_once.values.is_none() && probe_once.error.is_none() {
                queries_attempted += 1;
                match sample_distinct(
                    pool,
                    &report.schema,
                    &child_table,
                    &child_column,
                    opts.sample_rows,
                )
                .await
                {
                    Ok(v) => probe_once.values = Some(v),
                    // ⚠️ `all_null` **不是**查询失败：这一列没有数据，值域证据就是「没有」。
                    // 把它算进「全部查询失败」会让「空表」被误报成「连不上/没权限」——
                    // REQ-170 的只读角色判据踩过（两张空表 ⇒ probe 直接失败）。
                    Err(DomainError::Empty) => {
                        probe_once.error = Some(DomainError::Empty);
                    }
                    Err(e) => {
                        queries_failed += 1;
                        if first_error.is_none() {
                            first_error = Some(format!(
                                "{child_table}.{child_column}: {}",
                                match &e {
                                    DomainError::Db(m) => m.clone(),
                                    other => other.code().to_string(),
                                }
                            ));
                        }
                        probe_once.error = Some(e);
                    }
                }
            }
            let values = probe_once.values.clone();
            let err = probe_once.error.clone();
            let mut trials = 0usize;
            let mut best: Option<(usize, DomainCheck)> = None;
            for (idx, c) in cands.iter().enumerate() {
                if trials >= opts.max_targets_per_column {
                    break;
                }
                let Some(parent) = cards.iter().find(|p| p.name == c.parent_table) else {
                    continue;
                };
                let Some(col) = parent.columns.iter().find(|x| x.name == c.parent_column) else {
                    continue;
                };
                let Some(child_col) =
                    child_card.and_then(|cc| cc.columns.iter().find(|x| x.name == child_column))
                else {
                    continue;
                };
                if !comparable(&child_col.data_type, &col.data_type) {
                    continue;
                }
                if let Some(e) = &err {
                    best = Some((idx, DomainCheck::skipped(e.code())));
                    break;
                }
                let Some(vals) = &values else { break };
                trials += 1;
                stats.domain_tested += 1;
                queries_attempted += 1;
                match count_key_hits(
                    pool,
                    &report.schema,
                    &c.parent_table,
                    &c.parent_column,
                    vals,
                )
                .await
                {
                    Ok(hits) => {
                        let d = domain_verdict(vals.len() as u32, hits);
                        if d.subset {
                            stats.domain_confirmed += 1;
                            // 撞上了就停：同一列不可能同时指向两张表的键（值域是强证据）。
                            best = Some((idx, d));
                            break;
                        }
                        stats.domain_rejected += 1;
                        if best.as_ref().map_or(true, |(_, x)| x.matched < d.matched) {
                            best = Some((idx, d));
                        }
                    }
                    Err(e) => {
                        stats.domain_failed += 1;
                        queries_failed += 1;
                        if first_error.is_none() {
                            first_error = Some(format!(
                                "{}.{} -> {}.{}: {}",
                                child_table,
                                child_column,
                                c.parent_table,
                                c.parent_column,
                                e.code()
                            ));
                        }
                        best = Some((idx, DomainCheck::skipped(e.code())));
                        break;
                    }
                }
            }
            if let Some((idx, d)) = best {
                chosen_idx = idx;
                domain_for_best = Some(d);
            }
            if let Some(d) = &domain_for_best {
                domain_by_column.insert(group_key.clone(), d.clone());
            }
        }

        // 选一条：值域确认/命中最多的那条优先，其余按命名档（cands 已排序）。
        let c = &mut cands[chosen_idx];
        // 把值域结论挂到**同一个候选**上（没跑成也留原因，不是静默）。
        if let Some(by_col) = domain_by_column.get(&group_key) {
            c_attach_domain(c, by_col.clone());
        }
        let mut evidence = c.evidence.clone();
        // 证据里补上「同名列」与「注释」已经由提名阶段合并；这里只保证命名候选
        // 在父表键列恰好与子列同名时能看见 samelenum 依据（注释里那一列的真实情况）。
        if c.parent_column == c.child_column && !c.has("samelenum") {
            evidence.push(CandidateEvidence {
                source: "samelenum".to_string(),
                detail: format!(
                    "同名列：子列与目标键同名（{}），父表该列是{}",
                    c.child_column, c.target_kind
                ),
                matched: None,
                sampled: None,
            });
        }
        let domain = domain_by_column.get(&group_key).cloned();
        let base = c.naming_base.unwrap_or(0.0);
        let score = if domain.as_ref().map_or(false, |d| d.subset) {
            score_of(&evidence, domain.as_ref())
        } else {
            // 没有值域确认时，命名档 + 弱证据的加成（封顶 0.85）。
            let weak = evidence
                .iter()
                .filter(|e| e.source == "comment" || e.source == "samelenum")
                .count() as f64;
            ((base + 0.10 * weak).min(0.85) * 100.0).round() / 100.0
        };
        let band = band_of(score).to_string();
        let rc = RelationCandidate {
            child_table: c.child_table.clone(),
            child_column: c.child_column.clone(),
            parent_table: c.parent_table.clone(),
            parent_column: c.parent_column.clone(),
            target_kind: c.target_kind.clone(),
            evidence,
            score,
            band,
            domain: domain.clone(),
            child_unique,
        };
        if rc.evidence.iter().any(|e| e.source == "samelenum") {
            stats.samelenum += 1;
        }
        if rc.evidence.iter().any(|e| e.source == "comment") {
            stats.comment += 1;
        }
        kept.push(rc);
    }

    // 未解析：`*_id` 列一个候选都没有（⛔ 不许静默）。
    for (t, c) in &pending_columns {
        if !kept
            .iter()
            .any(|k| &k.child_table == t && &k.child_column == c)
        {
            let code = if column_probes
                .get(&(t.clone(), c.clone()))
                .and_then(|p| p.error.as_ref())
                .is_some()
            {
                column_probes
                    .get(&(t.clone(), c.clone()))
                    .and_then(|p| p.error.as_ref())
                    .map(|e| e.code().to_string())
                    .unwrap_or_else(|| "sample_failed".to_string())
            } else {
                "no_candidate".to_string()
            };
            let no_candidate = code == "no_candidate";
            unresolved.push(ProbeUnresolved {
                table: t.clone(),
                column: c.clone(),
                code,
                reason: if no_candidate {
                    "列名提不出任何候选表（词干既不是表名/词元/尾缀，注释也没提到）".to_string()
                } else {
                    "值域采样失败，这一列没有可判的候选".to_string()
                },
            });
        }
    }

    // 全部查询都失败 ⇒ 失败要响（⛔ 不许退化成「只做命名」）。
    if queries_attempted > 0 && queries_failed == queries_attempted {
        bail!(
            "relation probe 的值域查询**全部失败**（{queries_failed}/{queries_attempted}）：{}。\n\
             ⛔ 不退化输出「只做命名」的候选 —— 那会让「值域没验」看起来像「值域验过但没命中」。\n\
             排查：只读账号是否有 SELECT 权限 / 连接串是否指向正确的库。",
            first_error.unwrap_or_else(|| "（无错误文本）".to_string())
        );
    }

    // 截断（确定性：先按分数降序，再按坐标）。
    kept.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.child_table.cmp(&b.child_table))
            .then_with(|| a.child_column.cmp(&b.child_column))
            .then_with(|| a.parent_table.cmp(&b.parent_table))
    });
    if kept.len() > opts.max_pairs {
        stats.truncated_pairs = kept.len() - opts.max_pairs;
        kept.truncate(opts.max_pairs);
    }
    stats.candidates_kept = kept.len();

    let mut out = ProbeReport {
        version: PROBE_VERSION,
        kind: CANDIDATES_KIND.to_string(),
        method: PROBE_METHOD.to_string(),
        source: ProbeSource {
            schema_file: opts.schema_file.clone(),
            schema_sha256: opts.schema_sha256.clone(),
            schema: report.schema.clone(),
            read_only,
            dsn: redact_dsn(&opts.dsn),
            tables: report.table_count,
            columns: report.column_count,
            sample_rows: opts.sample_rows,
        },
        tables: cards,
        candidates: kept,
        unresolved,
        stats,
    };
    out.sort();
    Ok(out)
}

fn c_attach_domain(c: &mut RawCandidate, d: DomainCheck) {
    if d.tested && d.subset {
        c.add_evidence(CandidateEvidence {
            source: "domain".to_string(),
            detail: format!(
                "值域：样本不同值 {}/{} 全部落在目标键 {} 里（值域 ⊆ 键域）",
                d.matched, d.sampled, c.parent_column
            ),
            matched: Some(d.matched),
            sampled: Some(d.sampled),
        });
    }
}

// ──────────────────────────────────────────────────────────
// ③ 落提案（与 `doc merge` 的关系断言同一形状）
// ──────────────────────────────────────────────────────────

/// 一条判定结果 → 提案条目（`add_link_type`）。
///
/// 形状与 `doc_merge::build_doc_link` **同一套**：顶层 `kind/origin/band/reviewed_by`，
/// `payload.{key,from_type,to_type,cardinality,semantic,evidence[]}`。
/// 差异只在 `evidence[0].source`（`naming|domain|ai`）与 `payload.evidence` 的计数。
pub fn link_item(
    child_table: &str,
    child_column: &str,
    parent_table: &str,
    parent_column: &str,
    target_kind: &str,
    predicate: &str,
    cardinality: &str,
    source: &str,
    score: f64,
    basis: &str,
    matched: Option<u32>,
    sampled: Option<u32>,
    child_label: &str,
    parent_label: &str,
) -> serde_json::Value {
    let key = format!("{child_table}_{child_column}__{parent_table}");
    let mut ev = serde_json::json!({
        "table": child_table,
        "column": child_column,
        "parent_column": parent_column,
        "claim": basis,
        "source": source,
        "score": score,
    });
    if let Some(m) = matched {
        ev["matched"] = serde_json::json!(m);
    }
    if let Some(s) = sampled {
        ev["sampled"] = serde_json::json!(s);
    }
    // ⭐ REQ-120 的三态：值域对上才是 `true`（R2RML 才会生成 joinCondition）；
    // 纯命名 / AI 推断只能是 `false`（有据的弱关系），⛔ 不许冒充实测。
    let verified = if source == "domain" { "true" } else { "false" };
    let join_evidence = if source == "domain" {
        // ⚠️ 格式是 `r2rml::parse_parent_target` 的输入契约：`-> 表.列 via 档次`。
        // 少了它，r2rml 会 fail-closed 拒绝整份映射（真跑踩过：67 处对账失败）。
        format!(
            "value-overlap {}/{}（REQ-172 relation probe，只读采样）-> {parent_table}.{parent_column} via {target_kind}",
            matched.unwrap_or(0),
            sampled.unwrap_or(0)
        )
    } else {
        format!("{source}：只有命名/注释/同名列或模型推断，**没有**值域实测")
    };
    serde_json::json!({
        "kind": "add_link_type",
        "target": format!("{child_label}→{parent_label}"),
        "origin": "db",
        "band": UNSIGNED_BAND,
        "reviewed_by": null,
        "applicable": true,
        "payload": {
            "key": key,
            "from_type": child_table,
            "to_type": parent_table,
            "cardinality": cardinality,
            "semantic": predicate,
            "join_verified": verified,
            "join_evidence": join_evidence,
            "evidence": [ev],
        }
    })
}

/// 候选 → 提案条目（**没有 AI 时**的落法：`source` 取 `domain` / `naming`）。
///
/// 谓词只能从 16 主键里挑：`*_id` 列默认 `belongs_to`，其余同名列/注释候选
/// 默认 `references`（都还要人签 —— 这里不替人决定语义）。
pub fn probe_link_items(report: &ProbeReport) -> Vec<serde_json::Value> {
    report
        .candidates
        .iter()
        .map(|c| {
            let child_label = report
                .card(&c.child_table)
                .and_then(|t| t.display_cn.clone())
                .unwrap_or_else(|| c.child_table.clone());
            let parent_label = report
                .card(&c.parent_table)
                .and_then(|t| t.display_cn.clone())
                .unwrap_or_else(|| c.parent_table.clone());
            let domain = c.evidence.iter().find(|e| e.source == "domain");
            let source = if domain.is_some() { "domain" } else { "naming" };
            let predicate = if fk::fk_stem(&c.child_column).is_some() {
                "belongs_to"
            } else {
                "references"
            };
            let basis = c
                .evidence
                .iter()
                .map(|e| format!("{}：{}", e.source, e.detail))
                .collect::<Vec<_>>()
                .join(" · ");
            link_item(
                &c.child_table,
                &c.child_column,
                &c.parent_table,
                &c.parent_column,
                &c.target_kind,
                predicate,
                c.cardinality(),
                source,
                c.score,
                &basis,
                domain.and_then(|e| e.matched),
                domain.and_then(|e| e.sampled),
                &child_label,
                &parent_label,
            )
        })
        .collect()
}

/// 把关系条目合进基础提案：**同 `payload.key` 的旧条目被替换**，其余原样保留。
///
/// 返回 `(新增, 替换)`。为什么不追加：`ontok-propose` 打底时已经按命名约定
/// 产过同一条边（key 相同），追加会让页面上出现两条一模一样的待签项，
/// 而人签时会以为是两条不同关系。
pub fn merge_into_proposal(
    proposal: &mut serde_json::Value,
    items: Vec<serde_json::Value>,
) -> Result<(usize, usize)> {
    let obj = proposal
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("基础提案不是 JSON 对象"))?;
    let arr = obj
        .get_mut("items")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("基础提案缺少 `items` 数组"))?;
    let mut new_keys: BTreeSet<String> = BTreeSet::new();
    for it in &items {
        if let Some(k) = it.pointer("/payload/key").and_then(|v| v.as_str()) {
            new_keys.insert(k.to_string());
        }
    }
    let before = arr.len();
    arr.retain(|it| {
        if it.get("kind").and_then(|v| v.as_str()) != Some("add_link_type") {
            return true;
        }
        match it.pointer("/payload/key").and_then(|v| v.as_str()) {
            Some(k) => !new_keys.contains(k),
            None => true,
        }
    });
    let replaced = before - arr.len();
    let added = items.len();
    arr.extend(items);
    Ok((added, replaced))
}

// ──────────────────────────────────────────────────────────
// 单测（不连库）
// ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::introspect::{ColumnMeta, SchemaReport, TableMeta};

    fn col(name: &str, ty: &str, comment: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            data_type: ty.to_string(),
            nullable: true,
            comment: comment.map(str::to_string),
            is_primary_key: name == "id",
        }
    }

    fn table(name: &str, comment: Option<&str>, cols: Vec<ColumnMeta>) -> TableMeta {
        TableMeta {
            name: name.to_string(),
            comment: comment.map(str::to_string),
            column_count: cols.len(),
            commented_count: cols.iter().filter(|c| c.comment.is_some()).count(),
            columns: cols,
        }
    }

    fn schema() -> SchemaReport {
        SchemaReport {
            server_version: "test".into(),
            transaction_read_only: "on".into(),
            schema: "public".into(),
            table_count: 3,
            column_count: 9,
            commented_column_count: 4,
            foreign_key_count: 0,
            tables: vec![
                table(
                    "customers",
                    Some("客户"),
                    vec![
                        col("id", "character varying", Some("客户ID")),
                        col("name", "text", None),
                    ],
                ),
                table(
                    "balances",
                    Some("客户余额"),
                    vec![
                        col("id", "character varying", None),
                        col("customer_id", "character varying", Some("客户编号")),
                    ],
                ),
                table(
                    "orders",
                    None,
                    vec![
                        col("id", "character varying", None),
                        col("order_no", "text", None),
                        col("customer_id", "character varying", Some("客户ID")),
                    ],
                ),
            ],
            introspected_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn comment_token_only_accepts_the_id_style_suffix() {
        assert_eq!(comment_entity_token("客户编号").as_deref(), Some("客户"));
        assert_eq!(
            comment_entity_token("License ID").as_deref(),
            Some("License")
        );
        assert_eq!(comment_entity_token("余额变动，用于对账").as_deref(), None);
        assert_eq!(comment_entity_token("ID").as_deref(), None);
    }

    #[test]
    fn display_hint_takes_the_first_short_clause() {
        assert_eq!(
            display_hint(Some("用户套餐订阅的变更记录，含价格")).as_deref(),
            Some("用户套餐订阅的变更记录")
        );
        assert_eq!(display_hint(Some("   ")), None);
    }

    #[test]
    fn domain_verdict_needs_three_values_and_full_hit() {
        assert!(domain_verdict(5, 5).subset);
        assert!(!domain_verdict(5, 4).subset);
        assert!(!domain_verdict(2, 2).subset);
        assert_eq!(
            domain_verdict(2, 2).reason.as_deref(),
            Some("insufficient_sample")
        );
        assert_eq!(
            domain_verdict(5, 4).reason.as_deref(),
            Some("partial_overlap")
        );
    }

    #[test]
    fn only_same_type_family_is_comparable() {
        assert!(comparable("character varying", "text"));
        assert!(comparable("uuid", "character varying"));
        assert!(!comparable("text", "timestamp without time zone"));
        assert!(!comparable("integer", "character varying"));
        assert!(comparable("bigint", "numeric"));
    }

    #[test]
    fn naming_and_comment_produce_candidates_with_evidence() {
        let s = schema();
        let keys = KeyCatalog::from_product(&serde_json::json!({
            "tables": [
                {"name": "customers", "primary_key": "id"},
                {"name": "balances", "primary_key": "id"},
                {"name": "orders", "primary_key": "id"}
            ]
        }));
        let names = names_from_comments(&s);
        let raw = nominate(&s, &keys, &names);
        // `balances.customer_id` 应命中 customers（命名 + 注释都指向同一实体）
        let c = raw
            .iter()
            .find(|c| c.child_table == "balances" && c.child_column == "customer_id")
            .expect("balances.customer_id 应有候选");
        assert_eq!(c.parent_table, "customers");
        assert!(c.evidence.iter().any(|e| e.source == "naming"));
    }

    #[test]
    fn proposal_items_keep_the_doc_merge_shape_and_are_unsigned() {
        let item = link_item(
            "balances",
            "customer_id",
            "customers",
            "id",
            "primary_key",
            "belongs_to",
            "N:1",
            "domain",
            0.95,
            "值域：5/5",
            Some(5),
            Some(5),
            "客户余额",
            "客户",
        );
        assert_eq!(item["kind"], "add_link_type");
        assert_eq!(item["origin"], "db");
        assert_eq!(item["band"], "low");
        assert!(item["reviewed_by"].is_null());
        assert_eq!(item["payload"]["from_type"], "balances");
        assert_eq!(item["payload"]["to_type"], "customers");
        assert_eq!(item["payload"]["cardinality"], "N:1");
        assert_eq!(item["payload"]["semantic"], "belongs_to");
        assert_eq!(item["payload"]["evidence"][0]["source"], "domain");
        assert_eq!(item["payload"]["evidence"][0]["matched"], 5);
        assert_eq!(item["payload"]["key"], "balances_customer_id__customers");
        // 值域实测 ⇒ joinVerified true（R2RML 才会写 joinCondition）
        assert_eq!(item["payload"]["join_verified"], "true");
        assert!(item["payload"]["join_evidence"]
            .as_str()
            .unwrap_or("")
            .contains("value-overlap"));
        // 纯命名 ⇒ false（有据的弱关系，⛔ 不冒充实测）
        let named = link_item(
            "balances",
            "customer_id",
            "customers",
            "id",
            "primary_key",
            "belongs_to",
            "N:1",
            "naming",
            0.55,
            "命名",
            None,
            None,
            "客户余额",
            "客户",
        );
        assert_eq!(named["payload"]["join_verified"], "false");
    }

    #[test]
    fn merge_replaces_the_same_key_and_keeps_user_items() {
        let mut base = serde_json::json!({
            "project": "p",
            "items": [
                {"kind": "add_entity_type", "payload": {"key": "balances"}},
                {"kind": "add_link_type", "payload": {"key": "balances_customer_id__customers", "semantic": "old"}},
                {"kind": "add_link_type", "payload": {"key": "orders_order_no__order"}}
            ]
        });
        let (added, replaced) = merge_into_proposal(
            &mut base,
            vec![serde_json::json!({
                "kind": "add_link_type",
                "payload": {"key": "balances_customer_id__customers", "semantic": "belongs_to"}
            })],
        )
        .unwrap();
        assert_eq!((added, replaced), (1, 1));
        let items = base["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(
            items
                .iter()
                .find(|i| i.pointer("/payload/key").and_then(|v| v.as_str())
                    == Some("balances_customer_id__customers"))
                .unwrap()["payload"]["semantic"],
            "belongs_to"
        );
        assert!(items
            .iter()
            .any(|i| i.pointer("/payload/key").and_then(|v| v.as_str())
                == Some("orders_order_no__order")));
    }

    #[test]
    fn the_predicate_whitelist_is_exactly_the_contract() {
        assert_eq!(PREDICATES.len(), 16);
        for p in ["belongs_to", "targets", "records", "derived_from"] {
            assert!(is_predicate(p), "{p} 应在词表里");
        }
        for p in ["uses", "related_to", "associated_with", "uses_ssl"] {
            assert!(!is_predicate(p), "{p} 不在词表里，必须被丢");
        }
    }

    #[test]
    fn redaction_never_leaks_credentials() {
        let r = redact_dsn(
            "postgresql://postgres:super-secret@172.16.120.20:30432/anspire_open_uat?x=1",
        );
        assert_eq!(r, "172.16.120.20:30432/anspire_open_uat");
        assert!(!r.contains("super-secret"));
    }

    #[test]
    fn score_without_domain_never_reaches_strong() {
        let ev = vec![
            CandidateEvidence {
                source: "naming".into(),
                detail: "命名".into(),
                matched: None,
                sampled: None,
            },
            CandidateEvidence {
                source: "comment".into(),
                detail: "注释".into(),
                matched: None,
                sampled: None,
            },
        ];
        assert!(score_of(&ev, None) <= 0.85);
        let d = domain_verdict(5, 5);
        assert!(score_of(&ev, Some(&d)) >= 0.90);
    }
}
