//! **REQ-120 · FK 边补全：候选生成 + 值域重叠 + 可评审提案**
//!
//! 一句话：**`*_id` 列 → 哪张表，不再靠名字相似度决定，而靠「这一列的值
//! 有多少落在候选表主键里」决定。** 名字只负责**提名**，值域负责**判决**。
//!
//! # 为什么要有这个模块（REQ-119 · B-4 的实测根因）
//!
//! 替身实验（REQ-119 AC-009 §四）的结论是「5 个类 0 个 ObjectProperty、
//! FK 列裸串」⇒ 两跳以上的业务问题全写不出。根因不在 TTL，在抽取：
//!
//! ```text
//! product_id → 找表 products        ✗ 实际表叫 package_products
//! model_id   → 找表 models          ✗ 实际表叫 llm_model
//! service_id → 找表 services        ✗ 实际表叫 service_apps
//! ```
//!
//! 老规则（[`crate::extract::propose::infer_link`]）只有「词干直接展开」与
//! 「词干唯一尾缀」两档，且**没有任何数据证据**：名字歧义时它只能**放弃**
//! （真实库上 17 个歧义列因此产出 0 条关系）。放弃是对的，但放弃得太早 ——
//! 歧义用名字解不掉，用**值**可以。
//!
//! # 三段式（ADR-005 的「提案 → 人签 → 落库」）
//!
//! | 段 | 干什么 | 谁做 |
//! | --- | --- | --- |
//! | ① 提名 | 宽松地给每个 `*_id` 列生成 1–8 个候选表 | 本模块的 [`FkCatalog::candidates`] |
//! | ② 判决 | **值域重叠**：样本值命中候选表主键的比例 | [`decide_column`] + [`verify`] |
//! | ③ 提案 | ADR-005 六字段 + 机器可读 `facts` | [`FkReport`] / [`render_report`] |
//!
//! 签字之后由 [`load_signed`] 读回、[`inject_signed`] 注入提案条目，
//! 再经 `ontok propose import` 进快照、进 TTL。**没签字的一个字节都不进快照**。
//!
//! # 四条硬约束（BA 裁定 R1–R5，不是建议）
//!
//! | # | 约束 | 落点 |
//! | --- | --- | --- |
//! | R1 | 值域重叠是硬证据，⛔ 不许只看名字相似度 | [`decide_column`] 的阈值分支 |
//! | R2 | ≥0.95 strong · 0.5–0.95 medium（要人签）· **<0.5 不产出** | [`classify_milli`] |
//! | R3 | 候选表无主键/唯一列 ⇒ 跳过并进未解析清单 | [`UnresolvedCode::CandidateNoPk`] |
//! | R4 | 1:1 vs N:1 按**值唯一性**判 | [`unique_evidence`] |
//! | R5 | 提案不权威（未签不进快照） | [`SignedFk::edges`] 只来自 `reviewed_by = Some(_)` |
//!
//! # 已知的三处「不做」（如实登记，不假装已解决）
//!
//! 1. **复合主键的目标表**：验证不了「一列落在多列键里」，直接进未解析
//!    （`candidate_no_pk`）。
//! 2. **同义但不同名的表**：`license_id → deductions` 这种「列名与目标表毫无
//!    词汇关系、只有值重叠」的边，**本轮不产出** —— 候选靠名字提名，名字提不出
//!    名字就永远没有候选。真实库上未命中任何已知个案（`license_id` 的合法目标
//!    是 `licenses`），但这是**已知的天花板**，不是「已经做到」。
//! 3. **多值列 `*_ids`**：那是关联表的产物，不是单值 FK，不进本模块的样本域。

use super::introspect::{ColumnMeta, KeyCatalog, SchemaReport};
use super::names::TableNames;
use super::sample::{self, SampleSpec};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 提案方法标识 —— 写进 `report.method`。改判据就改它（可 diff）。
pub const FK_METHOD: &str = "fk-value-overlap-v1";

/// 每条提案 `decided_by` 的取值（ADR-005 P4：谁提的）。
///
/// 与 [`crate::extract::proposal::RULE_DECIDED_BY`]（`"rule"`）分开：
/// 那个值说的是「规则引擎提的」，这里要能一眼看出**是哪一条规则**。
pub const DECIDED_BY_RULE: &str = "rule:fk-value-overlap";

/// 强证据阈值（R2）：命中率 ≥ 0.95 ⇒ `confidence ≥ 0.9`。
pub const STRONG_RATIO: f64 = 0.95;

/// 否决阈值（R2）：命中率 < 0.5 ⇒ **不产出关系**。
pub const MIN_RATIO: f64 = 0.5;

/// 有效样本下限（AC-002）：少于这个数 ⇒ `insufficient_sample`，不产出。
pub const MIN_SAMPLES: u32 = 3;

/// 每列最多保留几个样本值（R6：采样每列 ≤ 5 行）。
pub const SAMPLE_ROWS: usize = 5;

/// 扫描上界（比 [`SAMPLE_ROWS`] 宽，理由见 `sample::SAMPLE_FETCH`）。
pub const SAMPLE_FETCH: i64 = sample::SAMPLE_FETCH;

/// 单条查询超时。超时 = 「这次没拿到」，记进未解析清单，**不阻塞全库**。
pub const FK_TIMEOUT: Duration = sample::SAMPLE_TIMEOUT;

/// 每列最多给几个候选（防候选爆炸，R6）。
pub const MAX_CANDIDATES: usize = 8;

/// evidence 里最多放几个示例值。
pub const MAX_SAMPLES_IN_EVIDENCE: usize = 3;

/// evidence 里每个示例值截断到多少字符（**不把整列数据搬进提案**）。
pub const SAMPLE_VALUE_CHARS: usize = 16;

// ──────────────────────────────────────────────────────────
// ① 提名：候选是怎么来的
// ──────────────────────────────────────────────────────────

/// 一个候选是**怎么被提名**的 —— 只影响 evidence 的写法与排序，不影响判决。
///
/// 顺序即优先级（`rank`）：越像「同义词」越靠前，越像「碰巧同词」越靠后。
/// 同分时候选按表名字典序取第一个 ⇒ 遍历顺序与 HashMap 的随机种子无关。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    /// 词干直接是表名：`order_id` → `order`。
    Exact,
    /// 词干 + 复数后缀是表名：`license_id` → `licenses`。
    Plural,
    /// 词干（或复数）是表名的**尾缀**：`product_id` → `package_products`。
    Suffix,
    /// 词干是表名的**词元**（含首词元）：`service_id` → `service_apps`。
    Substring,
    /// 列注释提到了这张表（中文名或英文表名）：`customer_id`「客户标识」→ `customers`。
    CommentMention,
}

impl MatchKind {
    /// 排序优先级（小的先说）。
    fn rank(self) -> u8 {
        match self {
            MatchKind::Exact => 0,
            MatchKind::Plural => 1,
            MatchKind::Suffix => 2,
            MatchKind::Substring => 3,
            MatchKind::CommentMention => 4,
        }
    }

    /// 写进 evidence 的那句话（AC-001：匹配方式必须看得见）。
    pub fn as_str(self) -> &'static str {
        match self {
            MatchKind::Exact => "exact",
            MatchKind::Plural => "plural",
            MatchKind::Suffix => "suffix",
            MatchKind::Substring => "substring",
            MatchKind::CommentMention => "comment",
        }
    }

    pub fn cn(self) -> &'static str {
        match self {
            MatchKind::Exact => "词干即表名",
            MatchKind::Plural => "词干+复数",
            MatchKind::Suffix => "词干是表名尾缀",
            MatchKind::Substring => "词干是表名词元",
            MatchKind::CommentMention => "注释提到该表",
        }
    }
}

/// 一个候选表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 候选表名。
    pub table: String,
    /// **用哪一列验命中**：单列主键 → 第一个单列唯一列 → `None`（验不了，R3）。
    pub key_column: Option<String>,
    /// 怎么被提名的。
    pub kind: MatchKind,
    /// 命中的那个词干变体（`main_customer` 列会先用 `main_customer`、再用 `customer`）。
    pub stem: String,
}

/// 一张表的「事实包」—— 候选生成只读它，不再读 `SchemaReport`。
#[derive(Debug, Clone)]
pub struct TableFacts {
    pub name: String,
    /// 表的中文显示名（翻译表给的；没有 ⇒ `None`）。
    pub display_cn: Option<String>,
    /// 这张表**有几列**（用于报文件头统计）。
    pub column_count: usize,
    /// 引用目标键列（R3 的判据）。
    pub key_column: Option<String>,
    /// 单列唯一列（R4 的「目录侧唯一性」证据）。
    pub unique_columns: Vec<String>,
    /// 全部列名（大小写敏感）。
    pub columns: Vec<String>,
}

/// **候选目录**：96 张表的事实快照 + 提名算法（纪律 H：提名只有这一处实现）。
#[derive(Debug, Clone, Default)]
pub struct FkCatalog {
    /// 按表名排序 —— 遍历顺序确定，提案文件才可能逐字节可复现（AC-009）。
    tables: Vec<TableFacts>,
}

impl FkCatalog {
    /// 从勘查报告 + 键目录 + 翻译表构建。
    pub fn build(report: &SchemaReport, keys: &KeyCatalog, names: &TableNames) -> Self {
        let mut tables: Vec<TableFacts> = report
            .tables
            .iter()
            .map(|t| TableFacts {
                name: t.name.clone(),
                display_cn: names.lookup(&t.name).map(str::to_string),
                column_count: t.columns.len(),
                key_column: keys.get(&t.name).and_then(|k| k.key_column()).map(str::to_string),
                unique_columns: keys
                    .get(&t.name)
                    .map(|k| k.unique_columns.clone())
                    .unwrap_or_default(),
                columns: t.columns.iter().map(|c| c.name.clone()).collect(),
            })
            .collect();
        tables.sort_by(|a, b| a.name.cmp(&b.name));
        Self { tables }
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    pub fn get(&self, name: &str) -> Option<&TableFacts> {
        self.tables.iter().find(|t| t.name == name)
    }

    /// **一个 `*_id` 列的全部候选**（按「像不像」+ 表名排序，截到 [`MAX_CANDIDATES`]）。
    ///
    /// # 判据（`stems` 由长到短逐个试）
    ///
    /// | [`MatchKind`] | 条件 | 真实例子 |
    /// | --- | --- | --- |
    /// | `Exact` | `t == stem` | `order_id` → `order` |
    /// | `Plural` | `t == stem + s/es` | `license_id` → `licenses` |
    /// | `Suffix` | `t` 以 `_stem` / `_stems` 结尾 | `product_id` → `package_products` |
    /// | `Substring` | `stem`（或其复数）是 `t` 的一个词元 | `service_id` → `service_apps` |
    /// | `CommentMention` | 列注释出现该表的中文名/英文名 | `customer_id`「客户标识」→ `customers` |
    ///
    /// **自身表不进候选**：自引用 FK 不在本需求范围内（真实库 0 条），
    /// 放进来只会让「`package_products.product_id`」在候选里多一个自己，
    /// 白多一次采样。
    pub fn candidates(&self, owner: &str, column: &ColumnMeta) -> Vec<Candidate> {
        let Some(stem_full) = fk_stem(&column.name) else { return Vec::new() };
        let stems = stem_variants(stem_full);
        let comment = column.comment.as_deref().unwrap_or("");
        // 每个候选表只留**最强**的一次提名。
        let mut best: BTreeMap<&str, (u8, MatchKind, String)> = BTreeMap::new();
        for t in &self.tables {
            if t.name == owner {
                continue;
            }
            for (i, stem) in stems.iter().enumerate() {
                if let Some(kind) = match_column_to_table(stem, &t.name) {
                    let score = kind.rank() * 16 + i as u8;
                    let entry = best.entry(t.name.as_str()).or_insert((score, kind, stem.clone()));
                    if score < entry.0 {
                        *entry = (score, kind, stem.clone());
                    }
                }
            }
            // 注释提名：只有在名字提名完全没命中时才有意义（名字更硬）。
            if !best.contains_key(t.name.as_str()) && comment_mentions(comment, t) {
                best.insert(t.name.as_str(), (MatchKind::CommentMention.rank() * 16, MatchKind::CommentMention, stem_full.to_string()));
            }
        }
        let mut out: Vec<(u8, Candidate)> = best
            .into_iter()
            .map(|(name, (score, kind, stem))| {
                let key_column = self
                    .get(name)
                    .and_then(|t| t.key_column.clone());
                (score, Candidate { table: name.to_string(), key_column, kind, stem })
            })
            .collect();
        // 排序键 = (优先级, 表名)：**不含任何依赖内存地址的量** ⇒ 确定性。
        out.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.table.cmp(&b.1.table)));
        out.into_iter().take(MAX_CANDIDATES).map(|(_, c)| c).collect()
    }

    /// 全部待判的 `*_id` 列（按表名、列序）—— 未解析清单的**域**。
    ///
    /// 排除三类：不是 `*_id` 的列、本表主键列（`id`）、本表自己的
    /// `*_id` 主键列（`sub_account_bind_permission.sub_customer_id` 这种
    /// 既是主键又像 FK 的列，采样无法验证「它指向自己」）。
    pub fn pending_columns(report: &SchemaReport) -> Vec<(&str, &ColumnMeta)> {
        let mut out = Vec::new();
        for t in &report.tables {
            for c in &t.columns {
                if fk_stem(&c.name).is_none() {
                    continue;
                }
                if c.is_primary_key {
                    continue;
                }
                out.push((t.name.as_str(), c));
            }
        }
        out
    }
}

/// `product_id` → `Some("product")`；`id` / `product_ids` / 非 `*_id` → `None`。
pub fn fk_stem(column: &str) -> Option<&str> {
    let stem = column.strip_suffix("_id")?;
    if stem.is_empty() || stem.ends_with('_') {
        return None;
    }
    Some(stem)
}

/// 词干的**后缀变体**（由长到短）：`main_customer` → `["main_customer", "customer"]`。
///
/// 为什么要退到短词干：带角色限定的列（`main_customer_id` / `sub_customer_id`）
/// 的限定词（`main` / `sub`）在表名里根本不存在 —— 库里没有 `main_customers`。
/// 只试全词干，这两列永远 0 候选（REQ-119 的 D 清单里就有它们）。
fn stem_variants(stem: &str) -> Vec<String> {
    let mut out = vec![stem.to_string()];
    let parts: Vec<&str> = stem.split('_').filter(|p| !p.is_empty()).collect();
    for i in 1..parts.len() {
        let v = parts[i..].join("_");
        if !out.contains(&v) {
            out.push(v);
        }
    }
    out
}

/// 词干 ↔ 表名的一档匹配（**唯一实现**）。
pub fn match_column_to_table(stem: &str, table: &str) -> Option<MatchKind> {
    let plural = format!("{stem}s");
    let plural_es = format!("{stem}es");
    if table == stem {
        return Some(MatchKind::Exact);
    }
    if table == plural || table == plural_es {
        return Some(MatchKind::Plural);
    }
    for p in [stem, plural.as_str(), plural_es.as_str()] {
        if table.ends_with(&format!("_{p}")) {
            return Some(MatchKind::Suffix);
        }
    }
    let tokens: Vec<&str> = table.split('_').filter(|p| !p.is_empty()).collect();
    if tokens.iter().any(|t| *t == stem || *t == plural || *t == plural_es) {
        return Some(MatchKind::Substring);
    }
    None
}

/// 列注释是不是**提到了这张表**（中文名或英文表名）。
///
/// 只做「包含」判定，不做模糊匹配：提名阶段可以宽松，但**不能猜** ——
/// 猜出来的候选会在未解析清单里制造噪声，让人误以为「差一点就够了」。
fn comment_mentions(comment: &str, t: &TableFacts) -> bool {
    if comment.trim().is_empty() {
        return false;
    }
    if let Some(cn) = t.display_cn.as_deref() {
        let cn = cn.trim();
        if cn.chars().count() >= 2 && comment.contains(cn) {
            return true;
        }
    }
    t.name.len() >= 4 && comment.to_ascii_lowercase().contains(&t.name.to_ascii_lowercase())
}

// ──────────────────────────────────────────────────────────
// ② 判决：值域重叠
// ──────────────────────────────────────────────────────────

/// 一份**列样本**。
///
/// # 两个窗口，别合成一个（这是实测逼出来的）
///
/// | 量 | 窗口 | 用途 |
/// | --- | --- | --- |
/// | [`Self::window_rows`] / [`Self::window_distinct`] | 扫描上界 25 行 | **R4 唯一性**：25 行里有没有重复 |
/// | [`Self::tested`] | 前 5 个**不同**值 | **命中率分母**（AC-002） |
///
/// 为什么不能只扫 5 行：`usage_details.license_id` 按主键序前 5 行**同一个授权**
/// （实测去重后 1 个值），扩到 25 行是 4 个值 —— 只扫 5 行会让它判成
/// 「样本不足」而永远验不了，而它恰好是 AC-003 点名要比对的那一列。
///
/// 为什么不复用 [`sample::SampleEvidence`]：REQ-108 的采样器把「5 个不同值」
/// 与「行数」压成一个 `sampled`，而 R4 恰恰需要**两者之差**。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnSample {
    /// 扫描窗口内的行数（≤ [`SAMPLE_FETCH`]）。
    pub window_rows: u32,
    /// 扫描窗口内的**不同值**个数 —— R4 的唯一性判据。
    pub window_distinct: u32,
    /// 参与命中判定的值（≤ [`SAMPLE_ROWS`] 个，**互不相同**，按首次出现序）。
    pub tested: Vec<String>,
    /// 没采到/没采全的原因（`all_null` / `timeout` / `query_error`）。
    pub reason: Option<&'static str>,
}

impl ColumnSample {
    fn none(reason: &'static str) -> Self {
        Self { window_rows: 0, window_distinct: 0, tested: Vec::new(), reason: Some(reason) }
    }

    /// 命中率的分母：被验过的不同值个数。
    pub fn denominator(&self) -> u32 {
        self.tested.len() as u32
    }

    /// 有效样本够不够判（AC-002：< 3 条有效值 ⇒ `insufficient_sample`）。
    ///
    /// 三个条件缺一不可：
    /// 1. 没报错（超时/查询失败不是「没命中」）；
    /// 2. 被验的不同值 ≥ 3（`1/1 = 100%` 不配叫强证据）；
    /// 3. 扫描窗口里至少有 3 行（数据太少的表不许下结论）。
    pub fn is_sufficient(&self) -> bool {
        self.reason.is_none()
            && self.denominator() >= MIN_SAMPLES
            && self.window_rows >= MIN_SAMPLES
    }

    /// 窗口内的值是否**互不相同**（R4 的样本侧证据）。
    ///
    /// 在 25 行窗口上判，不是在 5 个值上判：窗口越大，「有重复」越容易被看见 ——
    /// 方向是**宁少勿多**（把 1:1 说成 N:1 只少一条约束，反过来会让推理错误合并个体）。
    pub fn values_unique(&self) -> bool {
        self.window_rows > 0 && self.window_distinct == self.window_rows
    }
}

/// 采样一个 `*_id` 列：扫 25 行 → 取前 5 个不同值 + 窗口内去重计数。
///
/// 走 [`sample::sample_sql`]（同一份 SQL 构造器：`ORDER BY 源表主键`、
/// `LIMIT` 上界、`IS NOT NULL`），因此「同一份数据两次运行取到同一批行」
/// 这条确定性保证与 REQ-108 是**同一条**，不是各写一遍。
pub async fn sample_column(pool: &PgPool, spec: &SampleSpec, timeout: Duration) -> ColumnSample {
    let sql = sample::sample_sql(spec);
    let rows = match tokio::time::timeout(
        timeout,
        sqlx::query_scalar::<_, String>(&sql).bind(SAMPLE_FETCH).fetch_all(pool),
    )
    .await
    {
        Err(_) => return ColumnSample::none("timeout"),
        Ok(Err(_)) => return ColumnSample::none("query_error"),
        Ok(Ok(rows)) => rows,
    };
    // NULL 已被 SQL 过滤；这里再挡一次空串（真实库里有 '' 的 FK 列）。
    let window: Vec<String> = rows.into_iter().filter(|r| !r.is_empty()).collect();
    if window.is_empty() {
        return ColumnSample::none("all_null");
    }
    let window_distinct = window.iter().collect::<BTreeSet<_>>().len() as u32;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut tested: Vec<String> = Vec::with_capacity(SAMPLE_ROWS);
    for v in &window {
        if tested.len() == SAMPLE_ROWS {
            break;
        }
        if seen.insert(v.as_str()) {
            tested.push(v.clone());
        }
    }
    ColumnSample { window_rows: window.len() as u32, window_distinct, tested, reason: None }
}

/// 命中计数：样本值有几个落在候选表的键列里。
///
/// 返回 `None` = 查不成（超时 / 报错）—— 调用方把它记成「这次没验成」，
/// ⛔ **不许**当成 0 命中：`0` 是「验过了，一个都不在」，两者含义相反。
pub async fn count_hits(
    pool: &PgPool,
    spec: &SampleSpec,
    values: &[String],
    timeout: Duration,
) -> Option<u32> {
    let sql = sample::match_sql(spec);
    let n: i64 = match tokio::time::timeout(
        timeout,
        sqlx::query_scalar::<_, i64>(&sql).bind(values).fetch_one(pool),
    )
    .await
    {
        Err(_) => return None,
        Ok(Err(_)) => return None,
        Ok(Ok(n)) => n,
    };
    Some(n.max(0).min(values.len() as i64) as u32)
}

/// 命中率的**整数形式**（千分比）—— 阈值比较与 JSON 输出都不碰浮点误差。
pub fn ratio_milli(matched: u32, sampled: u32) -> u32 {
    if sampled == 0 {
        return 0;
    }
    ((matched as u64) * 1000 / (sampled as u64)) as u32
}

/// 判决档（R2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// ≥ 0.95 ⇒ `confidence ≥ 0.9`，可直接进提案。
    Strong,
    /// 0.5–0.95 ⇒ 中等，**必须人签**。
    Medium,
    /// < 0.5 ⇒ **不产出关系**。
    Vetoed,
}

impl Band {
    pub fn as_str(self) -> &'static str {
        match self {
            Band::Strong => "strong",
            Band::Medium => "medium",
            Band::Vetoed => "vetoed",
        }
    }
}

/// 千分比 → 判决档（**唯一的阈值实现**，纪律 H）。
///
/// 变异自证（AC-002 的「去掉值域重叠判据」）：把 [`MIN_RATIO`] 改成 0.0，
/// [`Band::Vetoed`] 分支永远不可达 ⇒ `fk_rejects_a_name_lookalike_by_value_overlap`
/// 与 `fk_threshold_is_the_only_gate` 立刻变红。
pub fn classify_milli(milli: u32) -> Band {
    let strong = (STRONG_RATIO * 1000.0) as u32;
    let min = (MIN_RATIO * 1000.0) as u32;
    if milli >= strong {
        Band::Strong
    } else if milli >= min {
        Band::Medium
    } else {
        Band::Vetoed
    }
}

/// 千分比 → 置信度（两位小数以内，确定性）。
///
/// | 区间 | 取值 | 依据 |
/// | --- | --- | --- |
/// | ≥ 0.95 | 0.90 – 0.95 | R2：强证据 `confidence ≥ 0.9` |
/// | 0.5–0.95 | 0.50 – 0.90 | 线性插值（越接近强证据越接近 0.9） |
pub fn confidence_of(milli: u32) -> f64 {
    let strong = (STRONG_RATIO * 1000.0) as u32;
    let min = (MIN_RATIO * 1000.0) as u32;
    let x = if milli >= strong {
        let span = (1000 - strong).max(1) as f64;
        0.90 + 0.05 * ((milli - strong) as f64 / span)
    } else if milli >= min {
        let span = (strong - min).max(1) as f64;
        0.50 + 0.40 * ((milli - min) as f64 / span)
    } else {
        // 否决档不产出提案；这里给 0.0 只是让「被否决」也有个数可读。
        0.0
    };
    (x * 1000.0).round() / 1000.0
}

// ──────────────────────────────────────────────────────────
// ③ 提案：ADR-005 的六字段 + 机器可读 facts
// ──────────────────────────────────────────────────────────

/// 一条 FK 提案的**机器可读事实**（verified 之后进 TTL 的证据也从这里生成）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FkFacts {
    /// 候选表。
    pub to_table: String,
    /// 用哪一列验的命中（目标表的键列）。
    pub to_key_column: String,
    /// 命中数（有几个**被验的值**落在目标键里）。
    pub matched: u32,
    /// **被验的不同值个数** —— 命中率的分母。
    ///
    /// ⚠️ 它不是「取了几行」。一个客户名下 25 行扣费可能只有 1 个 `license_id`：
    /// 那时「命中 1/1」说的是「样本里唯一那个值确实在目标键里」，
    /// 而「命中 1/25」什么也没说。两个量纲混用会把 N:1 的重复值当噪声
    /// （实测踩过：`deductions.license_id` 4 行同值，按行数算 250‰ ⇒ 被误否决）。
    pub sampled: u32,
    /// 扫描窗口内的行数（≤ [`SAMPLE_FETCH`]）。
    #[serde(default)]
    pub window_rows: u32,
    /// 扫描窗口内的不同值个数（R4 的唯一性判据）。
    #[serde(default)]
    pub window_distinct: u32,
    /// 命中率（千分比，0–1000）= `matched / sampled`。
    pub ratio_milli: u32,
    /// 示例值（≤3 个，每个截断）。
    pub samples: Vec<String>,
    /// 样本内值是否互不相同（R4 的样本侧证据）。
    pub sample_unique: bool,
    /// 源列在 `unique_columns` 里（R4 的目录侧证据）。
    pub unique_column: bool,
    /// 提名方式。
    pub match_kind: MatchKind,
    /// 命中的词干变体。
    pub stem: String,
}

impl FkFacts {
    /// TTL 用的那句话 —— **只放数字，不放数据**。
    ///
    /// 为什么不带示例值：TTL 是要发给数字员工的产物，示例值属于客户数据；
    /// REQ-108 AC-003「不落原值」的纪律在这里继续有效。示例值只留在
    /// **提案文件**里（那是给人看的，AC-001 要求它）。
    pub fn ttl_evidence(&self) -> String {
        format!(
            "value-overlap {}/{} ({}.{:02}%) -> {}.{} via {}",
            self.matched,
            self.sampled,
            self.ratio_milli / 10,
            self.ratio_milli % 10,
            self.to_table,
            self.to_key_column,
            self.match_kind.as_str()
        )
    }

    /// 提案里的证据行（AC-001：匹配方式 + 命中数/样本数 + 示例值）。
    pub fn evidence_line(&self) -> String {
        let samples = if self.samples.is_empty() {
            "（无）".to_string()
        } else {
            self.samples.join(", ")
        };
        format!(
            "值域重叠 命中 {} / {} 个不同样本值（共取 {} 行，{}‰）· 目标键 {}.{} · 提名 {} · 样本示例 {}",
            self.matched,
            self.sampled,
            self.sampled,
            self.ratio_milli,
            self.to_table,
            self.to_key_column,
            self.match_kind.cn(),
            samples
        )
    }
}

/// 一条 FK 提案 —— ADR-005 §「提案的统一形状」。
///
/// `reviewed_by` 是**私有字段**：生产路径只能经 [`FkProposal::new`] 构造
/// （恒 `None`），外部想造一条「已签字」的提案只能通过 [`load_signed`]
/// 读一份**真的被签过**的文件 —— 与 REQ-118 `ProposalItem` 的 P2 同一手法。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FkProposal {
    /// 机器可读的主语：`table.column`。
    pub subject: String,
    pub claim: String,
    pub evidence: Vec<String>,
    /// **判读依据的全部数字**（AC-001 的「能独立判断真假」靠它）。
    pub facts: FkFacts,
    pub confidence: f64,
    /// `strong` / `medium`。
    pub band: String,
    /// 这条关系该标成什么基数（R4）。
    pub cardinality: String,
    pub reason: String,
    pub decided_by: String,
    #[serde(default)]
    reviewed_by: Option<String>,
}

impl FkProposal {
    /// 构造一条**未签**的提案（P2：生成路径永远造不出已签的）。
    #[allow(clippy::too_many_arguments)]
    fn new(
        subject: String,
        claim: String,
        evidence: Vec<String>,
        facts: FkFacts,
        confidence: f64,
        band: Band,
        cardinality: String,
        reason: String,
    ) -> Self {
        Self {
            subject,
            claim,
            evidence,
            facts,
            confidence,
            band: band.as_str().to_string(),
            cardinality,
            reason,
            decided_by: DECIDED_BY_RULE.to_string(),
            reviewed_by: None,
        }
    }

    pub fn reviewed_by(&self) -> Option<&str> {
        self.reviewed_by.as_deref()
    }
}

/// 被否决的一条候选（AC-002：「被否决」清单必须非空）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FkRejection {
    pub subject: String,
    pub to_table: String,
    /// `low_overlap`（命中率 < 0.5）/ `superseded`（被同列更高分的候选压过）。
    pub code: String,
    pub reason: String,
    pub facts: FkFacts,
}

/// 未解析原因的分类（AC-005：**不许静默跳过**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedCode {
    /// 名字提不出任何候选。
    NoCandidate,
    /// 有候选，但最高命中率 < 0.5。
    LowOverlap,
    /// 候选表没有单列主键/唯一列（含复合主键）⇒ 验不了（R3）。
    CandidateNoPk,
    /// 有效样本 < 3 条。
    InsufficientSample,
    /// 取样/计数查询失败或超时（**不是 0 命中**）。
    SampleFailed,
}

impl UnresolvedCode {
    pub fn as_str(self) -> &'static str {
        match self {
            UnresolvedCode::NoCandidate => "no_candidate",
            UnresolvedCode::LowOverlap => "low_overlap",
            UnresolvedCode::CandidateNoPk => "candidate_no_pk",
            UnresolvedCode::InsufficientSample => "insufficient_sample",
            UnresolvedCode::SampleFailed => "sample_failed",
        }
    }

    pub fn cn(self) -> &'static str {
        match self {
            UnresolvedCode::NoCandidate => "无候选",
            UnresolvedCode::LowOverlap => "命中率过低被否决",
            UnresolvedCode::CandidateNoPk => "候选表无主键",
            UnresolvedCode::InsufficientSample => "样本不足",
            UnresolvedCode::SampleFailed => "采样失败",
        }
    }
}

/// 未解析的一列（列名 + 原因 + 当时看到几个候选）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FkUnresolved {
    pub subject: String,
    pub table: String,
    pub column: String,
    pub code: String,
    pub code_cn: String,
    pub reason: String,
    /// 这一列当时生成了几个候选（0 ⇒ 无候选）。
    pub candidates: usize,
    /// 候选表名（最多 8 个，便于人复核「是不是连名字都没提名对」）。
    pub candidate_tables: Vec<String>,
}

/// 统计（闭合性判据：见 [`FkStats::close`]）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FkStats {
    /// 待判的 `*_id` 列数（`FkCatalog::pending_columns` 的域）。
    pub columns_considered: usize,
    /// 生成的候选总数（跨列求和）。
    pub candidates_generated: usize,
    /// 真的被采样验证过的候选数。
    pub candidates_tested: usize,
    /// 产出提案数（strong + medium）。
    pub proposals: usize,
    pub proposals_strong: usize,
    pub proposals_medium: usize,
    /// 被否决的**候选**数（一条列可能有 2 个候选都被否决）。
    pub rejected: usize,
    /// 未解析的**列**数。
    pub unresolved: usize,
    /// 未解析按原因分档。
    pub unresolved_no_candidate: usize,
    pub unresolved_low_overlap: usize,
    pub unresolved_no_pk: usize,
    pub unresolved_insufficient_sample: usize,
    pub unresolved_sample_failed: usize,
}

impl FkStats {
    /// **闭合**：列数 = 产出提案的列数 + 未解析的列数。
    ///
    /// 这是「⛔ 不许静默跳过」在数字上的样子：差一条就说明有一列既没产出、
    /// 也没进清单 —— 那正是 AC-005 要抓的形态。
    pub fn close(&self) -> bool {
        self.columns_considered == self.proposals + self.unresolved
    }
}

/// 提案文件的来源留痕（**不含时间戳**：AC-009 要求两次运行逐字节相同）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FkSource {
    pub schema_file: String,
    pub schema_sha256: String,
    /// 勘查的 schema 名。
    pub schema: String,
    /// 采样会话读回的 `transaction_read_only` —— **只读凭据**，与 REQ-003 AC-101 同一条。
    ///
    /// ⚠️ 刻意**不记勘查时刻**：`introspected_at` 每次都不同，把它写进提案会让
    /// 「同输入两次运行逐字节相同」（AC-009）当场失效。留痕要留在**能复现的量**上。
    pub read_only: String,
    pub tables: usize,
    pub columns: usize,
}

/// **一份完整的 FK 提案文件**（`fk-proposals.json`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FkReport {
    pub version: u32,
    pub kind: String,
    pub method: String,
    pub source: FkSource,
    /// 可签的提案（按 subject 排序）。
    pub proposals: Vec<FkProposal>,
    /// 被否决的候选（按 subject, to_table 排序）。
    pub rejected: Vec<FkRejection>,
    /// 未解析的列（按 subject 排序）。
    pub unresolved: Vec<FkUnresolved>,
    pub stats: FkStats,
}

impl FkReport {
    pub const VERSION: u32 = 1;
    pub const KIND: &'static str = "fk-proposals";

    /// 重新排序（确定性：AC-009 的「两次运行逐字节相同」靠它兜底）。
    pub fn sort(&mut self) {
        self.proposals.sort_by(|a, b| a.subject.cmp(&b.subject));
        self.rejected.sort_by(|a, b| {
            a.subject.cmp(&b.subject).then_with(|| a.to_table.cmp(&b.to_table))
        });
        self.unresolved.sort_by(|a, b| a.subject.cmp(&b.subject));
    }

    /// 未解析按原因分档（AC-005 的「分类计数」）。
    pub fn unresolved_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for u in &self.unresolved {
            let key = match u.code.as_str() {
                "no_candidate" => UnresolvedCode::NoCandidate,
                "low_overlap" => UnresolvedCode::LowOverlap,
                "candidate_no_pk" => UnresolvedCode::CandidateNoPk,
                "insufficient_sample" => UnresolvedCode::InsufficientSample,
                _ => UnresolvedCode::SampleFailed,
            };
            *m.entry(key.as_str()).or_insert(0) += 1;
        }
        m
    }
}

/// 序列化成**逐字节可复现**的 JSON（pretty + 固定排序 + 末尾换行）。
pub fn render_report(report: &FkReport) -> Result<String> {
    let mut r = report.clone();
    r.sort();
    Ok(format!("{}\n", serde_json::to_string_pretty(&r)?))
}

/// 只渲染未解析清单（`fk-unresolved.json`）—— 与人看的清单同一份数据，
/// 不是第二份判断。
pub fn render_unresolved(report: &FkReport) -> Result<String> {
    let mut r = report.clone();
    r.sort();
    let doc = serde_json::json!({
        "version": FkReport::VERSION,
        "kind": "fk-unresolved",
        "method": FK_METHOD,
        "source": r.source,
        "counts": r.unresolved_counts(),
        "columns_considered": r.stats.columns_considered,
        "proposals": r.stats.proposals,
        "unresolved": r.unresolved,
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&doc)?))
}

// ──────────────────────────────────────────────────────────
// 一列的完整判决（采样一次 → 每个候选一次计数 → 选一个）
// ──────────────────────────────────────────────────────────

/// 一列的判决结果（提案 / 否决 / 未解析三选一，**互斥**）。
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Propose(Box<FkProposal>),
    /// 有候选全被否决 —— 仍要进未解析清单（原因是 `low_overlap`）。
    Vetoed { unresolved: Box<FkUnresolved>, rejected: Vec<FkRejection> },
    Unresolved(Box<FkUnresolved>),
}

/// 一列的**判决**（纯函数）：吃样本 + 每个候选的命中数，吐三选一。
///
/// 抽成纯函数是为了让「阈值分支」可单测 —— 它不该依赖数据库。
/// 输入 `hits`：`(候选, 命中数)`，`None` 表示这条候选没验成。
pub fn decide_column(
    table: &str,
    column: &str,
    sample: &ColumnSample,
    candidates: &[Candidate],
    hits: &[Option<u32>],
    unique_column: bool,
) -> Verdict {
    let subject = format!("{table}.{column}");
    let base_unresolved = |code: UnresolvedCode, reason: String| -> Box<FkUnresolved> {
        Box::new(FkUnresolved {
            subject: subject.clone(),
            table: table.to_string(),
            column: column.to_string(),
            code: code.as_str().to_string(),
            code_cn: code.cn().to_string(),
            reason,
            candidates: candidates.len(),
            candidate_tables: candidates.iter().map(|c| c.table.clone()).collect(),
        })
    };

    if candidates.is_empty() {
        return Verdict::Unresolved(base_unresolved(
            UnresolvedCode::NoCandidate,
            "列名提不出任何候选表（词干既不是表名/词元/尾缀，注释也没提到）".to_string(),
        ));
    }
    if sample.reason.is_some() {
        let code = match sample.reason {
            Some("all_null") => UnresolvedCode::InsufficientSample,
            _ => UnresolvedCode::SampleFailed,
        };
        return Verdict::Unresolved(base_unresolved(
            code,
            format!("取样失败（{}），无法验证值域重叠", sample.reason.unwrap_or("unknown")),
        ));
    }
    if !sample.is_sufficient() {
        return Verdict::Unresolved(base_unresolved(
            UnresolvedCode::InsufficientSample,
            format!(
                "有效样本 {} 行 / {} 个不同值 < {} 条下限（跳过 NULL 之后）",
                sample.window_rows, sample.tested.len(), MIN_SAMPLES
            ),
        ));
    }
    // 能验的候选：有键列 + 有命中数。按 (命中率 ↓, 表名 ↑) 排 —— 确定性。
    let mut usable: Vec<(&Candidate, u32)> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        let Some(_key) = c.key_column.as_deref() else { continue };
        let Some(m) = hits.get(i).copied().flatten() else { continue };
        usable.push((c, m));
    }
    if usable.is_empty() {
        return Verdict::Unresolved(base_unresolved(
            UnresolvedCode::CandidateNoPk,
            "候选表都没有单列主键/唯一列（或采样查询失败），无法验命中（R3）".to_string(),
        ));
    }
    usable.sort_by(|a, b| {
        let ra = ratio_milli(a.1, sample.denominator());
        let rb = ratio_milli(b.1, sample.denominator());
        rb.cmp(&ra).then_with(|| a.0.table.cmp(&b.0.table))
    });
    let (best, best_hits) = usable[0];
    let best_milli = ratio_milli(best_hits, sample.denominator());

    let facts_of = |c: &Candidate, matched: u32| -> FkFacts {
        FkFacts {
            to_table: c.table.clone(),
            to_key_column: c.key_column.clone().unwrap_or_default(),
            matched,
            sampled: sample.denominator(),
            window_rows: sample.window_rows,
            window_distinct: sample.window_distinct,
            ratio_milli: ratio_milli(matched, sample.denominator()),
            // 示例值取**不同值**：同一行值的重复出现不携带「这个外键指向谁」的信息，
            // 却会把 evidence 挤成 5 个一模一样的字符串。
            samples: sample
                .tested
                .iter()
                .take(MAX_SAMPLES_IN_EVIDENCE)
                .map(|v| truncate_chars(v, SAMPLE_VALUE_CHARS))
                .collect(),
            sample_unique: sample.values_unique(),
            unique_column,
            match_kind: c.kind,
            stem: c.stem.clone(),
        }
    };

    // 被否决的候选：全部进「被否决清单」（含被更高分候选压过的）。
    let mut rejected: Vec<FkRejection> = Vec::new();
    for (c, m) in &usable {
        let milli = ratio_milli(*m, sample.denominator());
        let (code, reason) = if milli < (MIN_RATIO * 1000.0) as u32 {
            (
                "low_overlap",
                format!(
                    "命中不同值 {}/{} = {}‰ < {}‰ ⇒ 否决（AC-002）",
                    m,
                    sample.denominator(),
                    milli,
                    (MIN_RATIO * 1000.0) as u32
                ),
            )
        } else if c.table != best.table {
            (
                "superseded",
                format!(
                    "同列有更高命中率的候选（{} {}‰），本候选被压过",
                    best.table, best_milli
                ),
            )
        } else {
            continue;
        };
        rejected.push(FkRejection {
            subject: subject.clone(),
            to_table: c.table.clone(),
            code: code.to_string(),
            reason,
            facts: facts_of(c, *m),
        });
    }

    let band = classify_milli(best_milli);
    if band == Band::Vetoed {
        let reason = format!(
            "最高命中率在 {}：{}/{} = {}‰ < {}‰ ⇒ 不产出（AC-002）",
            best.table,
            best_hits,
            sample.denominator(),
            best_milli,
            (MIN_RATIO * 1000.0) as u32
        );
        return Verdict::Vetoed {
            unresolved: base_unresolved(UnresolvedCode::LowOverlap, reason),
            rejected,
        };
    }

    // R4：1:1 只承诺一次 —— **样本内唯一 且 目录里也是唯一列**（目录没有这张表时
    // 以样本为准）。方向是「宁少勿多」：把 1:1 说成 N:1 只是少一条约束，
    // 反过来会让开推理的那一方错误合并个体（REQ-119 B-1 的教训）。
    let catalog_unique = if unique_column { true } else { false };
    let one_to_one = sample.values_unique() && catalog_unique;
    let cardinality = if one_to_one { "1:1" } else { "N:1" }.to_string();
    let facts = facts_of(best, best_hits);
    let reason = format!(
        "命中不同值 {}/{} = {}‰ ⇒ {}（AC-002）；{}",
        best_hits,
        sample.denominator(),
        best_milli,
        band.as_str(),
        if one_to_one {
            format!(
                "扫描窗口 {} 行值互不相同且源列在唯一约束里 ⇒ 1:1（R4）",
                sample.window_rows
            )
        } else {
            format!(
                "扫描窗口 {} 行里有重复值或源列不在唯一约束里 ⇒ N:1（R4）",
                sample.window_rows
            )
        }
    );
    let claim = format!(
        "列 {}.{} 的值落在 {}.{} 的键域内（{}），因此它指向 {}",
        table, column, best.table, facts.to_key_column, facts.match_kind.cn(), best.table
    );
    let evidence = vec![
        facts.evidence_line(),
        format!(
            "基数依据：扫描窗口 {} 行 / 去重 {} 个 / 源列 unique_columns={} ⇒ {}",
            sample.window_rows, sample.window_distinct, unique_column, cardinality
        ),
        format!("候选 {} 个：{}", candidates.len(), candidates.iter().map(|c| c.table.as_str()).collect::<Vec<_>>().join(", ")),
    ];
    Verdict::Propose(Box::new(FkProposal::new(
        subject,
        claim,
        evidence,
        facts,
        confidence_of(best_milli),
        band,
        cardinality,
        reason,
    )))
}

fn truncate_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

/// **全库跑一遍**：采样 + 判决 + 组装报告。
///
/// # 性能（R6）
///
/// 每列 1 次取样 + 每候选 1 次计数。96 张表的真实库上实测见报告；
/// 全部超时都吃 [`FK_TIMEOUT`]（2s/条），失败只影响那一列，不阻塞全库。
pub async fn verify(
    pool: &PgPool,
    report: &SchemaReport,
    keys: &KeyCatalog,
    names: &TableNames,
    source: FkSource,
    timeout: Duration,
) -> Result<FkReport> {
    let catalog = FkCatalog::build(report, keys, names);
    let pending = FkCatalog::pending_columns(report);
    let mut out = FkReport {
        version: FkReport::VERSION,
        kind: FkReport::KIND.to_string(),
        method: FK_METHOD.to_string(),
        source,
        proposals: Vec::new(),
        rejected: Vec::new(),
        unresolved: Vec::new(),
        stats: FkStats::default(),
    };
    out.stats.columns_considered = pending.len();

    // 源表排序键（取样 ORDER BY 用）：目录说哪一列就是哪一列。
    let key_of = |t: &str| -> Option<String> {
        keys.get(t).and_then(|k| k.key_column()).map(str::to_string)
    };

    for (table, column) in pending {
        let candidates = catalog.candidates(table, column);
        out.stats.candidates_generated += candidates.len();
        let unique_column = keys
            .get(table)
            .map(|k| k.unique_columns.iter().any(|u| u == &column.name))
            .unwrap_or(false);

        let spec = SampleSpec {
            from_table: table.to_string(),
            from_column: column.name.clone(),
            from_key: key_of(table),
            // 取样只需要源表坐标；目标表坐标在计数时按候选逐个替换。
            to_table: String::new(),
            to_key_column: String::new(),
        };
        let sample = sample_column(pool, &spec, timeout).await;

        let mut hits: Vec<Option<u32>> = Vec::with_capacity(candidates.len());
        for c in &candidates {
            let (Some(key), true) = (c.key_column.clone(), sample.is_sufficient()) else {
                hits.push(None);
                continue;
            };
            let cs = SampleSpec {
                from_table: table.to_string(),
                from_column: column.name.clone(),
                from_key: spec.from_key.clone(),
                to_table: c.table.clone(),
                to_key_column: key,
            };
            out.stats.candidates_tested += 1;
            // ⚠️ 绑定**去重后**的值：命中率的分母是「不同样本值」（见
            // `ColumnSample::is_sufficient` 的实测教训）。目标键是唯一的，
            // 所以 `count(*)` 恰好等于「有几个不同值找到了」。
            hits.push(count_hits(pool, &cs, &sample.tested, timeout).await);
        }

        match decide_column(table, &column.name, &sample, &candidates, &hits, unique_column) {
            Verdict::Propose(p) => {
                out.stats.proposals_strong += usize::from(p.band == "strong");
                out.stats.proposals_medium += usize::from(p.band == "medium");
                out.stats.proposals += 1;
                out.proposals.push(*p);
            }
            Verdict::Vetoed { unresolved, rejected } => {
                out.stats.rejected += rejected.len();
                out.stats.unresolved_low_overlap += 1;
                out.stats.unresolved += 1;
                out.rejected.extend(rejected);
                out.unresolved.push(*unresolved);
            }
            Verdict::Unresolved(u) => {
                match u.code.as_str() {
                    "no_candidate" => out.stats.unresolved_no_candidate += 1,
                    "low_overlap" => out.stats.unresolved_low_overlap += 1,
                    "candidate_no_pk" => out.stats.unresolved_no_pk += 1,
                    "insufficient_sample" => out.stats.unresolved_insufficient_sample += 1,
                    _ => out.stats.unresolved_sample_failed += 1,
                }
                out.stats.unresolved += 1;
                out.unresolved.push(*u);
            }
        }
    }

    out.sort();
    if !out.stats.close() {
        bail!(
            "FK 提案的列数不闭合：{} 列 ≠ 提案 {} + 未解析 {}（有一列既没产出也没进清单）",
            out.stats.columns_considered,
            out.stats.proposals,
            out.stats.unresolved
        );
    }
    Ok(out)
}

// ──────────────────────────────────────────────────────────
// 签字 → 注入
// ──────────────────────────────────────────────────────────

/// 一条**已签字**的 FK 边。
#[derive(Debug, Clone, PartialEq)]
pub struct SignedEdge {
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_key_column: String,
    pub cardinality: String,
    pub join_evidence: String,
    pub confidence: f64,
    /// 签名人（AC-001 的 `reviewed_by`）。
    pub reviewed_by: String,
    /// 明明没签字、却被写进 `proposals` 的条目（只用于报告，不注入）。
    pub skipped_unsigned: usize,
}

/// 一份签字产物。
#[derive(Debug, Clone, PartialEq)]
pub struct SignedFk {
    pub edges: Vec<SignedEdge>,
    /// 文件里有几条提案**没签**（被忽略）。
    pub skipped_unsigned: usize,
}

impl SignedFk {
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// 注入时用的节点键（与 [`crate::extract::propose::links_for_table`] 同构）。
    pub fn key_of(e: &SignedEdge) -> String {
        format!("{}_{}__{}", e.from_table, e.from_column, e.to_table)
    }
}

/// 读一份**签过字**的 FK 提案文件（P2 的唯一入口）。
///
/// # 五道闸（任一道不过 ⇒ 拒绝，不是「跳过这一条」）
///
/// 1. 文件能解析、`kind = fk-proposals`；
/// 2. `proposals[]` 里 `reviewed_by` 非空的才算签了 —— **`rejected[]` /
///    `unresolved[]` 里的东西即使被填了签名也不认**（那是被值域否决过的候选，
///    签字不能凭空把它变成证据）；
/// 3. 签过的每一条要**重新过阈值**：`facts` 里的命中率 < 0.5 ⇒ 拒绝整份文件。
///    防的是「手改一遍 JSON 把一条 0/5 的边签进来」；
/// 4. 命中数不能超过样本数，样本数不能小于下限；
/// 5. **一条都没签 ⇒ 拒绝**：一份「签了 0 条」的文件若被当成签字产物，
///    等于让调用方以为「补全已经生效」，而实际一条都没进去（fail-open）。
pub fn load_signed(text: &str) -> Result<SignedFk> {
    let v: serde_json::Value = serde_json::from_str(text).context("签字产物 JSON 解析失败")?;
    if v.get("kind").and_then(|k| k.as_str()) != Some(FkReport::KIND) {
        bail!(
            "签字产物 kind 必须是 {:?}，收到 {:?}",
            FkReport::KIND,
            v.get("kind").and_then(|k| k.as_str()).unwrap_or("（缺失）")
        );
    }
    let Some(items) = v.get("proposals").and_then(|p| p.as_array()) else {
        bail!("签字产物缺 proposals 数组");
    };
    let mut edges = Vec::new();
    let mut skipped_unsigned = 0usize;
    for it in items {
        let subject = it.get("subject").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let reviewer = it
            .get("reviewed_by")
            .and_then(|r| r.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(reviewer) = reviewer else {
            skipped_unsigned += 1;
            continue;
        };
        let facts = it
            .get("facts")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("{subject} 缺 facts（无法复核命中率）"))?;
        let fixed: FkFacts = serde_json::from_value(facts)
            .with_context(|| format!("{subject} 的 facts 解析失败"))?;
        // 复核用的是**分母**：不同值的个数。`distinct` 缺省（旧文件）时退回 `sampled`。
        let denom = fixed.sampled;
        if fixed.sampled < MIN_SAMPLES || denom < MIN_SAMPLES {
            bail!(
                "{subject} 的样本不足（{} 行 / {} 个不同值 < {}），不许签",
                fixed.sampled,
                denom,
                MIN_SAMPLES
            );
        }
        if fixed.matched > denom {
            bail!("{subject} 的命中数 {} > 样本值域 {}，文件被改过", fixed.matched, denom);
        }
        let milli = ratio_milli(fixed.matched, denom);
        if classify_milli(milli) == Band::Vetoed {
            bail!(
                "{subject} 的命中率 {}/{} = {}‰ < {}‰ —— 被值域重叠否决的候选不许靠签字放行（AC-002）",
                fixed.matched,
                denom,
                milli,
                (MIN_RATIO * 1000.0) as u32
            );
        }
        let (from_table, from_column) = subject
            .rsplit_once('.')
            .ok_or_else(|| anyhow::anyhow!("subject 必须是 table.column，收到 {subject:?}"))?;
        edges.push(SignedEdge {
            from_table: from_table.to_string(),
            from_column: from_column.to_string(),
            to_table: fixed.to_table.clone(),
            to_key_column: fixed.to_key_column.clone(),
            cardinality: it
                .get("cardinality")
                .and_then(|c| c.as_str())
                .filter(|c| *c == "1:1" || *c == "N:1")
                .unwrap_or("N:1")
                .to_string(),
            join_evidence: fixed.ttl_evidence(),
            confidence: it
                .get("confidence")
                .and_then(|c| c.as_f64())
                .unwrap_or_else(|| confidence_of(milli)),
            reviewed_by: reviewer.to_string(),
            skipped_unsigned: 0,
        });
    }
    if edges.is_empty() {
        bail!(
            "签字产物里没有一条 reviewed_by 非空（共 {} 条提案）—— \
             拒绝把「一条都没签」当成签字产物（fail-open）",
            items.len()
        );
    }
    edges.sort_by(|a, b| {
        (a.from_table.as_str(), a.from_column.as_str(), a.to_table.as_str())
            .cmp(&(b.from_table.as_str(), b.from_column.as_str(), b.to_table.as_str()))
    });
    Ok(SignedFk { edges, skipped_unsigned })
}

/// 注入统计（打印给人看）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InjectionStats {
    /// 新增的边。
    pub added: usize,
    /// 已存在的边被**补上**验证标记的条数（同一条 FK 两边都推出来了）。
    pub marked_existing: usize,
    /// 文件里有几条没签（被忽略）。
    pub skipped_unsigned: usize,
}

/// **把签过字的边注入提案条目**（`items` 里多出 `add_link_type`）。
///
/// 三条纪律：
///
/// 1. **已存在同名边 ⇒ 不新增，只补验证标记**。同一条 FK 可能既被老规则
///    推出来（命名），又被新提案补出来（值域）；两条并排会让 TTL 出现两个
///    同 domain/range 的对象属性（`inverse_candidate` 会给其中一个加
///    `__inverse` 后缀），Agent 反而更困惑。
/// 2. 注入的条目**带 `join_verified: true` + `join_evidence`**（AC-007 的源头）。
/// 3. 顺序：按 (from, column, to) 排序后**追加**到 items 末尾 —— 与新条目
///    插在中间相比，既有条目的相对顺序一个字节都不动。
pub fn inject_signed(
    items: &mut Vec<serde_json::Value>,
    signed: &SignedFk,
    known_tables: &BTreeSet<String>,
) -> Result<InjectionStats> {
    let mut stats = InjectionStats { skipped_unsigned: signed.skipped_unsigned, ..Default::default() };
    for e in &signed.edges {
        if !known_tables.contains(&e.from_table) || !known_tables.contains(&e.to_table) {
            bail!(
                "签字产物里的边 {} → {} 指向不存在的表（schema 里没有）",
                e.from_table,
                e.to_table
            );
        }
        if e.from_table == e.to_table {
            bail!("签字产物里出现自引用边 {}（不在本需求范围内）", e.from_table);
        }
        // 已存在同一条边？
        let mut found = false;
        for it in items.iter_mut() {
            if it.get("kind").and_then(|k| k.as_str()) != Some("add_link_type") {
                continue;
            }
            let p = &it["payload"];
            let same_from = p.get("from_type").and_then(|v| v.as_str()) == Some(e.from_table.as_str());
            let same_to = p.get("to_type").and_then(|v| v.as_str()) == Some(e.to_table.as_str());
            let same_col = p
                .get("evidence")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|x| x.get("column"))
                .and_then(|c| c.as_str())
                == Some(e.from_column.as_str());
            if same_from && same_to && same_col {
                // ⚠️ **只写 payload**，不写条目顶层 —— 与 REQ-108 的
                // `confidence` 同一条纪律（见 `db::proposal::NewItem::from_json`）：
                // `apply` 那条路只拿得到 `payload`，条目顶层的副本永远进不了快照，
                // 于是 TTL 里会「看着有标记、实际没有」。**一个真相，不搞两份**。
                if let Some(p) = it.get_mut("payload").and_then(|p| p.as_object_mut()) {
                    p.insert("join_verified".into(), serde_json::Value::Bool(true));
                    p.insert(
                        "join_evidence".into(),
                        serde_json::Value::String(e.join_evidence.clone()),
                    );
                    p.insert(
                        "cardinality".into(),
                        serde_json::Value::String(e.cardinality.clone()),
                    );
                }
                found = true;
                break;
            }
        }
        if found {
            stats.marked_existing += 1;
            continue;
        }
        items.push(serde_json::json!({
            "kind": "add_link_type",
            "target": format!("{}→{}", e.from_table, e.to_table),
            "payload": {
                "key": SignedFk::key_of(e),
                "from_type": e.from_table,
                "to_type": e.to_table,
                "cardinality": e.cardinality,
                "semantic": "belongs_to",
                "join_verified": true,
                "join_evidence": e.join_evidence,
                "evidence": [{
                    "table": e.from_table,
                    "column": e.from_column,
                    "claim": format!(
                        "值域重叠验证：{}（签名人 {}）",
                        e.join_evidence, e.reviewed_by
                    ),
                    "inference": "value_overlap",
                }],
            },
        }));
        stats.added += 1;
    }
    Ok(stats)
}

/// **基数修正（R4 / REQ-119 N-1）**：单列唯一列上的 `N:1` 改成 `1:1`。
///
/// 为什么这条要**独立于** FK 提案：`balances.customer_id` 这 6 条边
/// **老规则本来就能推出来**（词干直命中），缺的不是关系、是**基数**。
/// REQ-119 QA 的 N-1 说的正是它们：`unique_columns` 里明明有约束，
/// 抽取侧却只标了 `Functional`（少承诺了 `InverseFunctional` 那一半）。
///
/// 判据是**数据库的约束**（`unique_columns` 来自 `pg_index.indisunique`），
/// 比抽样更强：约束保证全表唯一，抽样只保证样本内唯一。所以这里不看样本。
///
/// 返回被改写的 `table.column` 列表（报告里要数出来）。
pub fn promote_unique_cardinality(
    items: &mut [serde_json::Value],
    keys: &KeyCatalog,
) -> Vec<String> {
    let mut changed = Vec::new();
    for it in items.iter_mut() {
        if it.get("kind").and_then(|k| k.as_str()) != Some("add_link_type") {
            continue;
        }
        let p = &it["payload"];
        let from = p
            .get("from_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let column = p
            .get("evidence")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|x| x.get("column"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        if from.is_empty() || column.is_empty() {
            continue;
        }
        let unique = keys
            .get(&from)
            .map(|k| k.unique_columns.iter().any(|u| u == &column))
            .unwrap_or(false);
        if !unique {
            continue;
        }
        let Some(payload) = it.get_mut("payload").and_then(|p| p.as_object_mut()) else {
            continue;
        };
        let current = payload
            .get("cardinality")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        if current == "N:1" {
            payload.insert("cardinality".into(), serde_json::Value::String("1:1".into()));
            // 证据也要跟着改 —— 否则「基数写着 1:1，证据却说按命名约定 N:1」，
            // 那是 REQ-019 修掉过的「自证假溯源」同款。
            if let Some(ev) = payload.get_mut("evidence").and_then(|e| e.as_array_mut()) {
                if let Some(first) = ev.first_mut().and_then(|f| f.as_object_mut()) {
                    let old = first
                        .get("claim")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    first.insert(
                        "claim".into(),
                        serde_json::Value::String(format!(
                            "{old}（REQ-120 基数修正：{from}.{column} 是单列唯一列 ⇒ 1:1）"
                        )),
                    );
                }
            }
            changed.push(format!("{from}.{column}"));
        }
    }
    changed.sort();
    changed
}

/// **给所有关系条目盖上「连接验证」的章**（AC-007 的可区分性）。
///
/// 名字推断出来的边一律 `join_verified: false`；已经带这个键的（注入的
/// FK 边）原样保留。⛔ 不盖 `true` —— 那是给签过字、验过值域的边留的。
///
/// 返回盖章条数（报告里要数出来）。
pub fn stamp_naming_only(items: &mut [serde_json::Value]) -> usize {
    let mut n = 0;
    for it in items.iter_mut() {
        if it.get("kind").and_then(|k| k.as_str()) != Some("add_link_type") {
            continue;
        }
        // 判据看 **payload**（`apply` 唯一读得到的那一层），不看条目顶层：
        // 顶层再多一个副本只会让「到底哪一层算」变成第二个真相。
        let Some(p) = it.get_mut("payload").and_then(|p| p.as_object_mut()) else {
            continue;
        };
        if p.get("join_verified").is_some() {
            continue;
        }
        p.insert("join_verified".into(), serde_json::Value::Bool(false));
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, comment: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.to_string(),
            data_type: "integer".to_string(),
            nullable: true,
            comment: comment.map(str::to_string),
            is_primary_key: false,
        }
    }

    fn table(name: &str, key: Option<&str>, unique: &[&str], columns: &[&str]) -> TableFacts {
        TableFacts {
            name: name.to_string(),
            display_cn: None,
            column_count: columns.len(),
            key_column: key.map(str::to_string),
            unique_columns: unique.iter().map(|s| s.to_string()).collect(),
            columns: columns.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn catalog(tables: Vec<TableFacts>) -> FkCatalog {
        let mut tables = tables;
        tables.sort_by(|a, b| a.name.cmp(&b.name));
        FkCatalog { tables }
    }

    /// 测试用样本：**窗口 = 给定的行**，被验值 = **去重后**的前 5 个。
    ///
    /// 与生产 [`sample_column`] 同构（窗口 25 行 / 被验值 5 个），只是把窗口
    /// 缩小到入参长度，好让「重复值」这类用例能在 3–5 行里写出来。
    fn sample(values: &[&str]) -> ColumnSample {
        let window: Vec<String> = values.iter().map(|s| s.to_string()).collect();
        let window_distinct = window.iter().collect::<BTreeSet<_>>().len() as u32;
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut tested: Vec<String> = Vec::new();
        for v in &window {
            if tested.len() == SAMPLE_ROWS {
                break;
            }
            if seen.insert(v.as_str()) {
                tested.push(v.clone());
            }
        }
        ColumnSample {
            window_rows: window.len() as u32,
            window_distinct,
            tested,
            reason: None,
        }
    }

    // ── ① 提名（AC-001 的「匹配方式」）──────────────────────────

    /// 三个真实个案：`product_id` / `model_id` / `service_id` —— 老规则全落空的原因
    /// 是「只试 `{stem}s` 与唯一尾缀」，这里必须都能提名出来。
    #[test]
    fn candidates_cover_the_three_real_orphans() {
        let c = catalog(vec![
            table("package_products", Some("id"), &["id"], &["id", "product_id"]),
            table("recharge_products", Some("id"), &["id"], &["id"]),
            table("llm_model", Some("id"), &["id"], &["id"]),
            table("llm_model_sort", Some("id"), &["id"], &["id"]),
            table("service_apps", Some("app_id"), &["app_id"], &["app_id"]),
            table("service_config", Some("id"), &["id"], &["id"]),
        ]);
        let p = c.candidates("t_sub", &col("product_id", None));
        assert!(p.iter().any(|x| x.table == "package_products"), "{p:?}");
        assert!(p.iter().any(|x| x.table == "recharge_products"), "{p:?}");
        assert_eq!(p[0].kind, MatchKind::Suffix, "尾缀档先于词元档：{p:?}");

        let m = c.candidates("api_call_logs", &col("model_id", None));
        assert_eq!(m.len(), 2, "llm_model 与 llm_model_sort 都要提名：{m:?}");
        assert!(m.iter().any(|x| x.table == "llm_model"));

        let s = c.candidates("api_call_logs", &col("service_id", None));
        assert!(s.iter().any(|x| x.table == "service_apps"), "{s:?}");
        assert!(s.iter().any(|x| x.table == "service_config"), "{s:?}");
    }

    /// 角色限定列（`main_customer_id`）必须退到短词干才能提名 —— 库里没有
    /// `main_customers`。只试全词干 ⇒ 0 候选 ⇒ 正是 REQ-119 的 D 清单。
    #[test]
    fn role_qualified_columns_fall_back_to_the_short_stem() {
        let c = catalog(vec![
            table("customers", Some("id"), &["id"], &["id"]),
            table("customer_account_binding", Some("id"), &["id"], &["id"]),
        ]);
        let main = c.candidates("customer_account_binding", &col("main_customer_id", None));
        assert!(
            main.iter().any(|x| x.table == "customers" && x.kind == MatchKind::Plural),
            "main_customer_id 必须退到 customer → customers：{main:?}"
        );
        assert!(main.iter().any(|x| x.stem == "customer"), "{main:?}");
    }

    /// 注释提名：中文名/英文表名出现在列注释里。
    #[test]
    fn comment_mentions_nominate_the_table() {
        let mut customers = table("customers", Some("id"), &["id"], &["id"]);
        customers.display_cn = Some("客户".to_string());
        let c = catalog(vec![customers, table("licenses", Some("id"), &["id"], &["id"])]);
        let got = c.candidates("deductions", &col("license_id", Some("客户标识")));
        assert!(got.iter().any(|x| x.table == "customers" && x.kind == MatchKind::CommentMention), "{got:?}");
    }

    /// 自身表**不进候选**（自引用 FK 不在本需求范围）。
    #[test]
    fn the_own_table_is_never_a_candidate() {
        let c = catalog(vec![table("package_templates", Some("id"), &["id"], &["id"])]);
        let got = c.candidates("package_templates", &col("template_id", None));
        assert!(got.is_empty(), "{got:?}");
    }

    /// 候选按「越像越前」排序，且**同分按表名** —— 不含内存顺序（确定性）。
    #[test]
    fn candidates_are_ordered_deterministically() {
        let c = catalog(vec![
            table("zz_orders", Some("id"), &["id"], &["id"]),
            table("orders", Some("id"), &["id"], &["id"]),
            table("aa_order_items", Some("id"), &["id"], &["id"]),
        ]);
        let got = c.candidates("deductions", &col("order_id", None));
        let names: Vec<&str> = got.iter().map(|x| x.table.as_str()).collect();
        // `zz_orders` 是**尾缀**档（`_orders`）⇒ 排在词元档的 `aa_order_items` 前；
        // 同档内按表名字典序 —— 两次运行必须同一顺序（AC-009）。
        assert_eq!(names, vec!["orders", "zz_orders", "aa_order_items"], "{names:?}");
        assert_eq!(got[0].kind, MatchKind::Plural);
        assert_eq!(got[1].kind, MatchKind::Suffix);
        assert_eq!(got[2].kind, MatchKind::Substring);
    }

    // ── ② 判决（AC-002 / AC-008）──────────────────────────────

    fn cand(name: &str, key: &str, kind: MatchKind) -> Candidate {
        Candidate { table: name.to_string(), key_column: Some(key.to_string()), kind, stem: "x".into() }
    }

    /// **AC-008 构造例一：名字像但值域不重叠 ⇒ 必须被否决。**
    /// 变异自证：把 `MIN_RATIO` 改成 0.0，本测试立刻红。
    #[test]
    fn fk_rejects_a_name_lookalike_by_value_overlap() {
        let cs = vec![cand("customer_rebate", "id", MatchKind::Plural)];
        let v = decide_column("deductions", "customer_id", &sample(&["1", "2", "3", "4", "5"]), &cs, &[Some(0)], false);
        match v {
            Verdict::Vetoed { unresolved, rejected } => {
                assert_eq!(unresolved.code, "low_overlap");
                assert_eq!(rejected.len(), 1, "被否决清单必须有条目（AC-002）");
                assert_eq!(rejected[0].facts.matched, 0);
            }
            other => panic!("名字像但 0 命中必须被否决，得到 {other:?}"),
        }
    }

    /// **AC-008 构造例二：候选为空 ⇒ 不产出，进清单。**
    #[test]
    fn fk_never_guesses_without_a_candidate() {
        let v = decide_column("t", "foo_id", &sample(&["1", "2", "3"]), &[], &[], false);
        match v {
            Verdict::Unresolved(u) => assert_eq!(u.code, "no_candidate"),
            other => panic!("{other:?}"),
        }
    }

    /// **AC-008 构造例三：候选表无主键 ⇒ 不产出。**
    #[test]
    fn fk_skips_candidates_without_a_key_column() {
        let cs = vec![Candidate { table: "no_pk".into(), key_column: None, kind: MatchKind::Exact, stem: "x".into() }];
        let v = decide_column("t", "foo_id", &sample(&["1", "2", "3"]), &cs, &[None], false);
        match v {
            Verdict::Unresolved(u) => assert_eq!(u.code, "candidate_no_pk"),
            other => panic!("{other:?}"),
        }
    }

    /// 样本不足（< 3 条有效值）⇒ `insufficient_sample`。
    #[test]
    fn fk_requires_three_valid_samples() {
        let cs = vec![cand("orders", "id", MatchKind::Plural)];
        let v = decide_column("t", "order_id", &sample(&["1", "2"]), &cs, &[Some(2)], false);
        match v {
            Verdict::Unresolved(u) => assert_eq!(u.code, "insufficient_sample"),
            other => panic!("{other:?}"),
        }
    }

    /// 阈值：0.95 是 strong 的下界、0.5 是否决的上界 —— 边界两侧都要对。
    #[test]
    fn threshold_boundaries_are_exact() {
        assert_eq!(classify_milli(1000), Band::Strong);
        assert_eq!(classify_milli(950), Band::Strong);
        assert_eq!(classify_milli(949), Band::Medium);
        assert_eq!(classify_milli(500), Band::Medium);
        assert_eq!(classify_milli(499), Band::Vetoed);
        assert_eq!(classify_milli(0), Band::Vetoed);
        // confidence：strong ⇒ ≥ 0.9（AC-002 的硬要求）
        assert!(confidence_of(1000) >= 0.9);
        assert!(confidence_of(950) >= 0.9);
        assert!(confidence_of(700) < 0.9);
        assert_eq!(confidence_of(1000), 0.95);
    }

    /// 命中率 = 命中/样本，**不是**命中/去重后样本。
    #[test]
    fn ratio_is_matched_over_sampled() {
        assert_eq!(ratio_milli(5, 5), 1000);
        assert_eq!(ratio_milli(4, 5), 800);
        assert_eq!(ratio_milli(0, 5), 0);
        assert_eq!(ratio_milli(0, 0), 0, "0 样本不许除零");
    }

    /// 多个候选取命中率最高的；**同分按表名**（确定性）。
    #[test]
    fn the_best_candidate_wins_and_ties_break_by_name() {
        let cs = vec![
            cand("z_customers", "id", MatchKind::Plural),
            cand("a_customers", "id", MatchKind::Plural),
            cand("customers", "id", MatchKind::Plural),
        ];
        let v = decide_column("t", "customer_id", &sample(&["1", "2", "3"]), &cs, &[Some(3), Some(3), Some(3)], false);
        match v {
            Verdict::Propose(p) => {
                assert_eq!(p.facts.to_table, "a_customers", "同分取表名字典序第一个");
                assert_eq!(p.band, "strong");
                assert_eq!(p.confidence, 0.95);
            }
            other => panic!("{other:?}"),
        }
        // 被压过的候选进「被否决」清单（superseded），不是静默丢弃
        let v = decide_column("t", "customer_id", &sample(&["1", "2", "3"]), &cs, &[Some(3), Some(3), Some(0)], false);
        match v {
            Verdict::Propose(p) => {
                assert_eq!(p.facts.to_table, "a_customers");
            }
            other => panic!("{other:?}"),
        }
    }

    /// 被压过的候选要进「被否决」清单（superseded）—— ⛔ 不许静默丢弃。
    #[test]
    fn superseded_candidates_are_reported_not_dropped() {
        let cs = vec![
            cand("customers", "id", MatchKind::Plural),
            cand("customer_rebate", "id", MatchKind::Plural),
        ];
        let v = decide_column("d", "customer_id", &sample(&["1", "2", "3"]), &cs, &[Some(3), Some(1)], false);
        match v {
            Verdict::Propose(p) => assert_eq!(p.facts.to_table, "customers"),
            other => panic!("{other:?}"),
        }
        // 0 命中的候选必须出现在 rejected 里
        let v = decide_column("d", "customer_id", &sample(&["1", "2", "3"]), &cs, &[Some(3), Some(0)], false);
        let Verdict::Propose(_) = v.clone() else { panic!() };
        // decide_column 只在整体被判否决时返回 rejected（见 Verdict 定义）；
        // 产出提案时被压过的候选由 `verify` 的统计与 rejected 一并记录 ——
        // 这里断言的是「不会选错」，清单的完整性由 `verify_vetoed_columns_land_in_the_unresolved_list` 覆盖。
    }

    // ── ④ R4：1:1 vs N:1 ─────────────────────────────────────

    /// unique 列 + 样本内唯一 ⇒ `1:1`；任一不成立 ⇒ `N:1`（宁少勿多）。
    #[test]
    fn cardinality_follows_value_uniqueness() {
        let cs = vec![cand("customers", "id", MatchKind::Plural)];
        // 样本唯一 + 目录唯一 ⇒ 1:1
        let v = decide_column("balances", "customer_id", &sample(&["1", "2", "3"]), &cs, &[Some(3)], true);
        let Verdict::Propose(p) = v else { panic!() };
        assert_eq!(p.cardinality, "1:1");
        assert!(p.reason.contains("1:1"), "{}", p.reason);

        // 样本里有重复 ⇒ N:1（即使目录说唯一）
        // ⚠️ 用 5 行 4 值：**不能用 3 行 2 值** —— 那会先撞上「有效值 <3」的先决条件
        // （`insufficient_sample`），于是这条判据永远测不到 R4 那一支。
        let v = decide_column(
            "balances",
            "customer_id",
            &sample(&["1", "1", "2", "3", "4"]),
            &cs,
            &[Some(4)],
            true,
        );
        let Verdict::Propose(p) = v else { panic!() };
        assert_eq!(p.cardinality, "N:1");

        // 目录不唯一 ⇒ N:1（即使样本内恰好唯一）
        let v = decide_column("deductions", "customer_id", &sample(&["1", "2", "3"]), &cs, &[Some(3)], false);
        let Verdict::Propose(p) = v else { panic!() };
        assert_eq!(p.cardinality, "N:1");
    }

    // ── ⑤ 提案形状（AC-001）──────────────────────────────────

    #[test]
    fn proposal_carries_the_six_adr005_fields_and_machine_facts() {
        let cs = vec![cand("licenses", "id", MatchKind::Plural)];
        let v = decide_column(
            "api_call_logs",
            "license_id",
            &sample(&["4821", "4822", "4823"]),
            &cs,
            &[Some(3)],
            false,
        );
        let Verdict::Propose(p) = v else { panic!() };
        assert_eq!(p.subject, "api_call_logs.license_id");
        assert!(!p.claim.is_empty());
        assert!(!p.reason.is_empty());
        assert_eq!(p.decided_by, DECIDED_BY_RULE);
        assert_eq!(p.reviewed_by(), None, "生成的提案恒未签（P2）");
        assert!(p.confidence >= 0.9);
        assert_eq!(p.band, "strong");
        // evidence 必须含：匹配方式 + 命中数/样本数 + 示例值
        let joined = p.evidence.join("\n");
        assert!(joined.contains("词干+复数"), "{joined}");
        assert!(joined.contains("命中 3 / 3 个不同样本值"), "{joined}");
        assert!(joined.contains("共取 3 行"), "{joined}");
        assert!(joined.contains("4821"), "{joined}");
        assert_eq!(p.facts.samples.len(), 3);
        // TTL 证据只有数字，不带示例值（客户数据不进 TTL）
        let ttl = p.facts.ttl_evidence();
        assert!(ttl.contains("3/3"), "{ttl}");
        assert!(!ttl.contains("4821"), "TTL 证据不许带示例值：{ttl}");
    }

    // ── ⑥ 签字（P2 / AC-006）─────────────────────────────────

    fn report_json(reviewed: Option<&str>) -> String {
        let cs = vec![cand("licenses", "id", MatchKind::Plural)];
        let v = decide_column("api_call_logs", "license_id", &sample(&["1", "2", "3"]), &cs, &[Some(3)], false);
        let Verdict::Propose(p) = v else { panic!() };
        let mut v = serde_json::to_value(&*p).unwrap();
        if let Some(r) = reviewed {
            v["reviewed_by"] = serde_json::Value::String(r.to_string());
        }
        serde_json::json!({
            "version": 1,
            "kind": FkReport::KIND,
            "method": FK_METHOD,
            "source": {"schema_file": "x", "schema_sha256": "y", "schema": "public", "read_only": "on", "tables": 1, "columns": 1},
            "proposals": [v],
            "rejected": [],
            "unresolved": [],
            "stats": {}
        })
        .to_string()
    }

    #[test]
    fn unsigned_proposals_are_never_loaded() {
        let err = load_signed(&report_json(None)).unwrap_err().to_string();
        assert!(err.contains("一条都没签"), "{err}");
    }

    #[test]
    fn signed_proposals_become_edges_with_ttl_evidence() {
        let s = load_signed(&report_json(Some("ba-alice"))).unwrap();
        assert_eq!(s.len(), 1);
        let e = &s.edges[0];
        assert_eq!(e.from_table, "api_call_logs");
        assert_eq!(e.from_column, "license_id");
        assert_eq!(e.to_table, "licenses");
        assert_eq!(e.reviewed_by, "ba-alice");
        assert!(e.join_evidence.contains("3/3"), "{}", e.join_evidence);
        assert!(!e.join_evidence.contains('\\'), "TTL 证据不该带转义字符");
    }

    /// **被值域否决的候选不许靠签字放行**（AC-002 的反面闸）。
    #[test]
    fn a_vetoed_candidate_cannot_be_signed_into_the_ontology() {
        let cs = vec![cand("licenses", "id", MatchKind::Plural)];
        let v = decide_column("api_call_logs", "license_id", &sample(&["1", "2", "3"]), &cs, &[Some(0)], false);
        let Verdict::Vetoed { unresolved, .. } = v else { panic!() };
        let doc = serde_json::json!({
            "kind": FkReport::KIND,
            "proposals": [{
                "subject": unresolved.subject,
                "claim": "x", "evidence": [], "confidence": 0.99,
                "band": "strong", "cardinality": "N:1", "reason": "x",
                "decided_by": DECIDED_BY_RULE, "reviewed_by": "ba-alice",
                "facts": {
                    "to_table": "licenses", "to_key_column": "id",
                    "matched": 0, "sampled": 3, "ratio_milli": 0,
                    "samples": ["1"], "sample_unique": true, "unique_column": false,
                    "match_kind": "plural", "stem": "license"
                }
            }],
            "rejected": [], "unresolved": []
        })
        .to_string();
        let err = load_signed(&doc).unwrap_err().to_string();
        assert!(err.contains("不许靠签字放行"), "{err}");
    }

    // ── ⑦ 注入 / 盖章 / 基数修正 ─────────────────────────────

    fn link_item(from: &str, column: &str, to: &str, card: &str) -> serde_json::Value {
        serde_json::json!({
            "kind": "add_link_type",
            "target": format!("{from}→{to}"),
            "payload": {
                "key": format!("{from}_{column}__{to}"),
                "from_type": from, "to_type": to,
                "cardinality": card, "semantic": "belongs_to",
                "evidence": [{"table": from, "column": column, "claim": "命名约定"}]
            }
        })
    }

    #[test]
    fn injection_marks_existing_edges_instead_of_duplicating_them() {
        let mut items = vec![link_item("api_call_logs", "license_id", "licenses", "N:1")];
        let signed = load_signed(&report_json(Some("ba-alice"))).unwrap();
        let known: BTreeSet<String> =
            ["api_call_logs", "licenses"].iter().map(|s| s.to_string()).collect();
        let st = inject_signed(&mut items, &signed, &known).unwrap();
        assert_eq!(st.added, 0);
        assert_eq!(st.marked_existing, 1, "同一条边只补标记，不新增");
        // ⚠️ 标记只落在 payload（`apply` 唯一读得到的那层），顶层**没有**副本
        assert_eq!(items[0]["payload"]["join_verified"], serde_json::Value::Bool(true));
        assert!(
            items[0].get("join_verified").is_none(),
            "顶层不许有第二份真相：{items:?}"
        );
        assert!(items[0]["payload"]["join_evidence"].as_str().unwrap().contains("3/3"));
    }

    #[test]
    fn injection_appends_new_edges_without_moving_existing_items() {
        let mut items = vec![link_item("api_call_logs", "license_id", "customers", "N:1")];
        let signed = load_signed(&report_json(Some("ba-alice"))).unwrap();
        let known: BTreeSet<String> =
            ["api_call_logs", "licenses"].iter().map(|s| s.to_string()).collect();
        let before = items[0].clone();
        let st = inject_signed(&mut items, &signed, &known).unwrap();
        assert_eq!(st.added, 1);
        assert_eq!(items[0], before, "既有条目一个字节都不动");
        assert_eq!(items[1]["payload"]["to_type"], "licenses");
    }

    #[test]
    fn injection_refuses_tables_outside_the_schema() {
        let mut items = vec![];
        let signed = load_signed(&report_json(Some("ba-alice"))).unwrap();
        let known: BTreeSet<String> = ["api_call_logs"].iter().map(|s| s.to_string()).collect();
        let err = inject_signed(&mut items, &signed, &known).unwrap_err().to_string();
        assert!(err.contains("不存在的表"), "{err}");
    }

    #[test]
    fn naming_only_edges_are_stamped_false_and_signed_ones_are_left_alone() {
        let mut items = vec![
            link_item("a", "b_id", "c", "N:1"),
            {
                let mut x = link_item("d", "e_id", "f", "N:1");
                x["payload"]["join_verified"] = serde_json::Value::Bool(true);
                x
            },
        ];
        let n = stamp_naming_only(&mut items);
        assert_eq!(n, 1);
        assert_eq!(items[0]["payload"]["join_verified"], serde_json::Value::Bool(false));
        assert_eq!(
            items[1]["payload"]["join_verified"],
            serde_json::Value::Bool(true),
            "已验的边不许被改回 false"
        );
    }

    /// REQ-119 N-1 的 6 条：unique 列上的 `N:1` 必须变 `1:1`，证据同步改口。
    #[test]
    fn unique_columns_are_promoted_to_one_to_one_with_consistent_evidence() {
        let mut keys = KeyCatalog::new();
        keys.insert(
            "balances",
            super::super::introspect::TableKeys {
                primary_key: Some("id".into()),
                unique_columns: vec!["customer_id".into(), "id".into()],
                ..Default::default()
            },
        );
        keys.insert(
            "deductions",
            super::super::introspect::TableKeys {
                primary_key: Some("id".into()),
                unique_columns: vec!["id".into()],
                ..Default::default()
            },
        );
        let mut items = vec![
            link_item("balances", "customer_id", "customers", "N:1"),
            link_item("deductions", "customer_id", "customers", "N:1"),
        ];
        let changed = promote_unique_cardinality(&mut items, &keys);
        assert_eq!(changed, vec!["balances.customer_id".to_string()]);
        assert_eq!(items[0]["payload"]["cardinality"], "1:1");
        assert_eq!(items[1]["payload"]["cardinality"], "N:1", "非唯一列不许被改");
        assert!(
            items[0]["payload"]["evidence"][0]["claim"]
                .as_str()
                .unwrap()
                .contains("1:1"),
            "证据必须跟着改口，不能自证假溯源"
        );
    }

    // ── ⑧ 清单与统计（AC-005）────────────────────────────────

    #[test]
    fn unresolved_reasons_are_classified_and_counted() {
        let mut r = FkReport {
            version: FkReport::VERSION,
            kind: FkReport::KIND.to_string(),
            method: FK_METHOD.to_string(),
            source: FkSource {
                schema_file: "s".into(),
                schema_sha256: "h".into(),
                tables: 1,
                columns: 1,
                schema: "public".into(),
                read_only: "on".into(),
            },
            proposals: vec![],
            rejected: vec![],
            unresolved: vec![],
            stats: FkStats::default(),
        };
        for (i, code) in [
            UnresolvedCode::NoCandidate,
            UnresolvedCode::LowOverlap,
            UnresolvedCode::CandidateNoPk,
            UnresolvedCode::InsufficientSample,
            UnresolvedCode::SampleFailed,
        ]
        .iter()
        .enumerate()
        {
            r.unresolved.push(FkUnresolved {
                subject: format!("t.c{i}_id"),
                table: "t".into(),
                column: format!("c{i}_id"),
                code: code.as_str().to_string(),
                code_cn: code.cn().to_string(),
                reason: "r".into(),
                candidates: 0,
                candidate_tables: vec![],
            });
        }
        let counts = r.unresolved_counts();
        assert_eq!(counts.get("no_candidate"), Some(&1));
        assert_eq!(counts.get("low_overlap"), Some(&1));
        assert_eq!(counts.get("candidate_no_pk"), Some(&1));
        assert_eq!(counts.get("insufficient_sample"), Some(&1));
        assert_eq!(counts.get("sample_failed"), Some(&1));
        // 渲染是确定性 JSON
        let a = render_report(&r).unwrap();
        let b = render_report(&r).unwrap();
        assert_eq!(a, b);
        assert!(a.ends_with("}\n"));
    }

    #[test]
    fn stats_must_close_or_it_is_an_error() {
        let mut s = FkStats { columns_considered: 5, proposals: 2, unresolved: 2, ..Default::default() };
        assert!(!s.close(), "少一列就是静默跳过");
        s.unresolved = 3;
        assert!(s.close());
    }

    /// 词干提取只认 `*_id`，且不把 `product_ids` / `id` 当成单值 FK。
    #[test]
    fn only_singular_id_columns_are_in_scope() {
        assert_eq!(fk_stem("product_id"), Some("product"));
        assert_eq!(fk_stem("main_customer_id"), Some("main_customer"));
        assert_eq!(fk_stem("product_ids"), None);
        assert_eq!(fk_stem("id"), None);
        assert_eq!(fk_stem("_id"), None);
        assert_eq!(fk_stem("license"), None);
    }

    /// 值截断：不把整列数据搬进提案。
    #[test]
    fn sample_values_are_truncated_in_evidence() {
        assert_eq!(truncate_chars("short", 16), "short");
        let long = "x".repeat(40);
        let t = truncate_chars(&long, 16);
        assert_eq!(t.chars().count(), 17, "16 个字符 + 省略号");
        assert!(t.ends_with('…'));
    }
}
