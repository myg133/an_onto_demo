//! **REQ-172 · 表间关系探索（第二段）：AI 两轮 + 代码校验 + 指纹缓存**
//!
//! ```text
//! candidates.json ──▶ A 轮：每批 ≤10 条候选 + 两端名片 + 样本 ──▶ 成立 / 不成立 / 证据不足
//!                  ──▶ B 轮：每批 ≤12 张表（1 张名片 + 其余一句话索引）──▶ 每表 ≤3 条新边
//!                  ──▶ 校验层：谓词白名单 / 列存在 / 目标必须是键 / 有依据
//!                  ──▶ relations.json（reviewed_by: null, band: low）
//! ```
//!
//! # 判据在 [`Validate`] 里，不在 prompt 里
//!
//! 契约（`Deploy/skills/ontok-relate/SKILL.md`）规定「什么算依据」；本模块负责
//! **投喂**（名片/样本/协议）与**拒收**（代码校验）。模型可以编，代码不许放过：
//!
//! | 丢弃码 | 条件 |
//! | --- | --- |
//! | `unknown_predicate` | 谓词不在 16 主键里（含 `uses` / `related_to` 这些禁用词） |
//! | `unknown_table` / `unknown_column` | 端点不在 schema 里 / 列不存在 |
//! | `type_incompatible` | 两端列类型不可比（`timestamp` vs `text`、`uuid` vs `varchar`…；QA F-1） |
//! | `target_not_key` | 目标列不是主键也不是单列唯一列 |
//! | `no_basis` | 没有 `basis`，或 `basis.kind/detail` 不成立（判据 3 的核心） |
//! | `basis_column_unknown` | `basis.columns` 点了不存在的列 |
//! | `nothing_held` | 模型说「不成立/证据不足」（不是丢弃，是结论） |
//! | `too_many_per_table` | B 轮一张表超过 3 条（按依据强度截断） |
//! | `reverse_duplicate` | 与已有边反向同义（归一） |
//!
//! # 缓存：同一输入第二次 **0 token**
//!
//! 指纹 = `sha256(round + user 正文 + prompt_sha + model)`；`prompt_sha` = skill 原文 + 协议
//! 的 sha256 ⇒ 换契约/换协议/换 agent 命令都会让缓存整体失效（不会回放旧契约下的回答）。
//! 缓存里只存**回答原文**，每次运行照样重新过校验 —— 与 `doc_acp` 同一条纪律。
//!
//! # 失败要响
//!
//! | 情形 | 行为 |
//! | --- | --- |
//! | 程序找不到 / 握手失败 | 调用方在 [`super::doc_acp::connect`] 就 `bail!`（本模块不回落） |
//! | 所有批次调用失败 | `bail!`，**一个产物都不写** |
//! | 一条关系都没留下 | `bail!`（附各丢弃码计数） |
//! | 部分批次失败 | 写产物，但 `stats.failed_batches > 0` + stderr 点名（不静默） |
//!
//! ⛔ 本模块没有「退回只做规则」的分支：没有模型答案就是失败，不是降级。

use super::relation::{is_predicate, ProbeReport, RelationCandidate, TableCard};
use super::sample;
use crate::agent::llm::protocol::{ChatRequest, Message};
use crate::agent::llm::LlmTransport;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────────────────
// 常量
// ──────────────────────────────────────────────────────────

/// 判定方法标识（改判据就改它）。
pub const JUDGE_METHOD: &str = "relation-judge-v1";

/// 判定产物的 `kind`。
pub const JUDGED_KIND: &str = "relation-judged";

pub const JUDGE_VERSION: u32 = 1;

/// A 轮每批最多几条候选（契约要求 ≤10）。
pub const ROUND_A_BATCH: usize = 10;

/// B 轮每批最多几张表（契约要求 ≤12）。
pub const ROUND_B_BATCH: usize = 12;

/// B 轮每张表最多几条（契约 §六 硬约束 1）。
pub const MAX_PER_TABLE_B: usize = 3;

/// 判定产物的缓存格式版本。
pub const CACHE_VERSION: u32 = 1;

/// 每张表取几行样本进 prompt（2~3 行）。
pub const DEFAULT_SAMPLE_ROWS: usize = 2;

/// 样本值进 prompt 前的截断长度。
pub const SAMPLE_CHARS: usize = 24;

// ──────────────────────────────────────────────────────────
// 协议（运行时指令 —— 契约 §一 说「形状由调用方规定」）
// ──────────────────────────────────────────────────────────

/// A 轮（判定）的输出协议。
pub const PROTOCOL_A: &str = r#"
---

## 输出协议 · **判定轮**（本通道的运行时指令）

你收到一个 JSON：`{"round":"A","candidates":[...],"cards":{...}}`。
`candidates[]` 每条是一对端点的候选（`id` / 子表.子列 / 父表.父列 / 已有证据）；
`cards` 是两端表的**名片**（表名、中文名、主键、列清单、2~3 行脱敏样本）。

对**每一条候选**给一条判定。你的整个回答必须是**一个 JSON 对象**（不要 markdown 代码块、
不要解释文字、不要 JSONL）：

{"verdicts": [{"candidate_id": "原样抄 candidates[].id",
               "verdict": "holds",            // holds | rejects | insufficient
               "predicate": "belongs_to",     // 只有 holds 才给，必须来自契约 §四的 16 个主键
               "basis": {"kind": "domain",    // domain | naming | comment | samelenum
                         "columns": ["子列", "父列"],
                         "detail": "一句话点名依据（哪一列 / 哪条注释 / 多少值域命中）"},
               "confidence": 0.8}]}

规则（每条都会被程序校验，不合规的条目**整条丢弃**）：

1. `candidates[]` 里的每一条**都要**有判定，`candidate_id` 原样抄；⛔ 不许增删、不许改名。
2. `verdict` 三选一：`holds`（成立）/ `rejects`（不成立）/ `insufficient`（证据不足）。
   **"证据不足"是合格答案** —— 拿业务常识硬给一条，比少给一条糟得多。
3. 只有 `holds` 才给 `predicate`，且必须逐字来自契约 §四的 16 个主键（小写）。
   ⛔ `uses` / `related_to` / `associated_with` / `has_many` 这些词一条都不许出现。
4. `basis` 必填且必须**可核查**：`kind` 四选一；`columns` 点名到具体列；
   `detail` 说清是哪条值域/命名/注释/同名列 —— ⛔ 不许写「业务上应该有」。
5. 只依据给你的名片与样本判，⛔ 不要读文件、⛔ 不要调用工具、⛔ 不要猜表。
6. 整个回答只允许 JSON。
"#;

/// B 轮（补漏）的输出协议。
pub const PROTOCOL_B: &str = r#"
---

## 输出协议 · **补漏轮**（本通道的运行时指令）

你收到一个 JSON：`{"round":"B","focus_table":"...","focus_card":{...},"tables":[...]}`。
`tables` 是这一批的全部表（≤12 张）：`focus_table` 有完整名片（列清单 + 样本），
其余表只有一句话索引（表名 + 中文名 + 主键）。

请为这一批表提出**表间关系**（每张表最多 3 条，提不出就少提或不提）。
你的整个回答必须是**一个 JSON 对象**（不要 markdown 代码块、不要解释文字）：

{"relations": [{"table": "这条关系属于哪张表（必须是 tables 里的 name）",
                "child_table": "子表", "child_column": "子表列",
                "parent_table": "父表", "parent_column": "父表列（必须是父表主键或唯一列）",
                "predicate": "belongs_to",
                "basis": {"kind": "naming",   // domain | naming | comment | samelenum
                          "columns": ["子表列", "父表列"],
                          "detail": "一句话点名依据"}}]}

规则（每条都会被程序校验，不合规的条目**整条丢弃**）：

1. 两端表必须是 `tables[]` 里的 `name`；两端列必须**真实存在**（索引里没给的列，⛔ 别猜）。
2. 每张表最多 3 条；按依据强度排序，提不出就少提（1 条也行，0 条也行）。
3. `predicate` 必须逐字来自契约 §四的 16 个主键。⛔ 禁止 `uses` / `related_to` /
   `associated_with` / `involves` 这类词，也⛔ 不许发明复合谓词。
4. 目标列（`parent_column`）必须是父表的**主键或单列唯一列** —— 否则 join 定位不到一行。
5. `basis` 必填（`kind` + `columns` + `detail`）：说不清依据就别提；
   ⛔ 不许用「业务上相关」「同一个模块」当依据。
6. 整个回答只允许 JSON。
"#;

// ──────────────────────────────────────────────────────────
// 数据结构
// ──────────────────────────────────────────────────────────

/// 一条判定的依据（来自候选证据或模型自报）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeEvidence {
    /// `naming` / `domain` / `comment` / `samelenum` / `ai`。
    pub source: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled: Option<u32>,
}

/// 一条**判定后**的关系（落提案之前的形状）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgedRelation {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
    pub parent_column: String,
    /// 16 主键之一。
    pub predicate: String,
    pub cardinality: String,
    /// `naming` / `domain` / `ai`（R3 的允许取值）。
    pub source: String,
    pub score: f64,
    /// 恒为 `low`（未签；由人逐条签）。
    pub band: String,
    /// `a` / `b`（哪一轮判出来的）。
    pub round: String,
    /// 判定依据的人话（提案 `payload.evidence[0].claim`）。
    pub basis: String,
    pub evidence: Vec<JudgeEvidence>,
}

impl JudgedRelation {
    pub fn id(&self) -> String {
        format!(
            "{}.{}->{}.{}",
            self.child_table, self.child_column, self.parent_table, self.parent_column
        )
    }
}

/// A / B 一轮的统计。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundStats {
    pub batches: usize,
    pub calls: usize,
    pub cache_hits: usize,
    pub failed_batches: usize,
    pub tokens: u64,
    /// 这一轮 system prompt 的 sha256（进产物：换契约/换协议看得见）。
    pub prompt_sha: String,
}

/// 产物来源（可追溯）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeSource {
    pub candidates_file: String,
    pub candidates_sha256: String,
    pub skill_file: String,
    pub skill_sha256: String,
    /// `acp:<命令行>@<握手名>#<握手版本>`。
    pub model: String,
    pub agent: String,
    pub agent_version: String,
}

/// 判定统计。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeStats {
    pub candidates: usize,
    pub tables: usize,
    pub sample_failures: usize,
    pub relations: usize,
    /// 按来源分档：`naming` / `domain` / `ai`。
    pub by_source: BTreeMap<String, usize>,
    /// B 轮真的提出来自「完全没有 `*_id` 线索」的表的关系数（判据 4 的读数）。
    pub round_b_relations: usize,
    /// B 轮涉及的表数。
    pub round_b_tables: usize,
}

/// **判定产物**（`relations.json`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeReport {
    pub version: u32,
    pub kind: String,
    pub method: String,
    pub source: JudgeSource,
    pub round_a: RoundStats,
    pub round_b: RoundStats,
    /// 判定通过的关系（按 id 排序）。
    pub relations: Vec<JudgedRelation>,
    /// 丢弃码 → 条数（⛔ 不静默：每种丢弃都要有数）。
    pub drops: BTreeMap<String, usize>,
    /// 没有拿到判定的候选（模型漏答）。
    pub unresolved: Vec<serde_json::Value>,
    pub stats: JudgeStats,
}

impl JudgeReport {
    pub fn sort(&mut self) {
        self.relations.sort_by(|a, b| a.id().cmp(&b.id()));
        self.stats.relations = self.relations.len();
        self.stats.by_source.clear();
        for r in &self.relations {
            *self.stats.by_source.entry(r.source.clone()).or_insert(0) += 1;
        }
    }

    pub fn render(&self) -> Result<String> {
        let mut r = self.clone();
        r.sort();
        Ok(format!("{}\n", serde_json::to_string_pretty(&r)?))
    }

    /// 把判定结果变成提案条目（与 `doc merge` 的关系断言同一形状）。
    pub fn link_items(&self, probe: &ProbeReport) -> Vec<serde_json::Value> {
        self.relations
            .iter()
            .map(|r| {
                let child_label = probe
                    .card(&r.child_table)
                    .and_then(|c| c.display_cn.clone())
                    .unwrap_or_else(|| r.child_table.clone());
                let parent_label = probe
                    .card(&r.parent_table)
                    .and_then(|c| c.display_cn.clone())
                    .unwrap_or_else(|| r.parent_table.clone());
                let domain = r.evidence.iter().find(|e| e.source == "domain");
                let target_kind = probe
                    .card(&r.parent_table)
                    .map(|c| {
                        if c.primary_key.as_deref() == Some(r.parent_column.as_str()) {
                            "primary_key"
                        } else {
                            "unique_column"
                        }
                    })
                    .unwrap_or("primary_key");
                super::relation::link_item(
                    &r.child_table,
                    &r.child_column,
                    &r.parent_table,
                    &r.parent_column,
                    target_kind,
                    &r.predicate,
                    &r.cardinality,
                    &r.source,
                    r.score,
                    &r.basis,
                    domain.and_then(|e| e.matched),
                    domain.and_then(|e| e.sampled),
                    &child_label,
                    &parent_label,
                )
            })
            .collect()
    }
}

// ──────────────────────────────────────────────────────────
// prompt 组装
// ──────────────────────────────────────────────────────────

/// 本轮的名片样本：表名 → 2~3 行（列名 → 脱敏后的值）。
pub type TableSamples = BTreeMap<String, Vec<BTreeMap<String, String>>>;

fn mask_value(v: &str) -> String {
    let compact: String = v.chars().filter(|c| !c.is_control()).collect();
    if compact.chars().count() <= SAMPLE_CHARS {
        compact
    } else {
        let mut s: String = compact.chars().take(SAMPLE_CHARS).collect();
        s.push('…');
        s
    }
}

fn card_json(
    card: &TableCard,
    samples: Option<&Vec<BTreeMap<String, String>>>,
) -> serde_json::Value {
    let columns: Vec<serde_json::Value> = card
        .columns
        .iter()
        .map(|c| {
            let mut v = serde_json::json!({
                "name": c.name,
                "type": c.data_type,
                "nullable": c.nullable,
                "primary_key": c.is_primary_key,
            });
            if let Some(cmt) = &c.comment {
                v["comment"] = serde_json::json!(cmt);
            }
            v
        })
        .collect();
    let mut v = serde_json::json!({
        "name": card.name,
        "primary_key": card.primary_key,
        "unique_columns": card.unique_columns,
        "columns": columns,
    });
    if let Some(cn) = &card.display_cn {
        v["display_cn"] = serde_json::json!(cn);
    }
    if let Some(rows) = samples {
        v["samples"] = serde_json::json!(rows);
    }
    v
}

/// A 轮的分批（每批 ≤ [`ROUND_A_BATCH`] 条候选，按产物顺序 ⇒ 确定性）。
pub fn round_a_batches(probe: &ProbeReport) -> Vec<Vec<&RelationCandidate>> {
    probe
        .candidates
        .chunks(ROUND_A_BATCH)
        .map(|c| c.iter().collect())
        .collect()
}

/// B 轮的分批（每批 ≤ [`ROUND_B_BATCH`] 张表，按表名排序 ⇒ 确定性）。
pub fn round_b_batches(probe: &ProbeReport) -> Vec<Vec<&TableCard>> {
    probe
        .tables
        .chunks(ROUND_B_BATCH)
        .map(|c| c.iter().collect())
        .collect()
}

/// A 轮一课的 user 正文。
pub fn round_a_payload(
    probe: &ProbeReport,
    batch: &[&RelationCandidate],
    samples: &TableSamples,
) -> String {
    let mut cards: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for c in batch {
        for t in [&c.child_table, &c.parent_table] {
            if cards.contains_key(t) {
                continue;
            }
            if let Some(card) = probe.card(t) {
                cards.insert(t.clone(), card_json(card, samples.get(t)));
            }
        }
    }
    let candidates: Vec<serde_json::Value> = batch
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id(),
                "child_table": c.child_table,
                "child_column": c.child_column,
                "parent_table": c.parent_table,
                "parent_column": c.parent_column,
                "target_kind": c.target_kind,
                "cardinality_hint": c.cardinality(),
                "evidence": c.evidence,
            })
        })
        .collect();
    serde_json::json!({
        "round": "A",
        "task": "对每条候选判定：成立 / 不成立 / 证据不足；按输出协议回一个 JSON 对象",
        "candidates": candidates,
        "cards": serde_json::Value::Object(cards),
    })
    .to_string()
}

/// B 轮一课的 user 正文。
pub fn round_b_payload(batch: &[&TableCard], samples: &TableSamples) -> String {
    let focus = batch.first();
    let tables: Vec<serde_json::Value> = batch
        .iter()
        .map(|t| {
            let mut v = serde_json::json!({
                "name": t.name,
                "primary_key": t.primary_key,
            });
            if let Some(cn) = &t.display_cn {
                v["display_cn"] = serde_json::json!(cn);
            }
            v
        })
        .collect();
    let mut doc = serde_json::json!({
        "round": "B",
        "task": "为这一批表提出表间关系（每表 ≤3 条）；按输出协议回一个 JSON 对象",
        "tables": tables,
    });
    if let Some(f) = focus {
        doc["focus_table"] = serde_json::json!(f.name);
        doc["focus_card"] = card_json(f, samples.get(&f.name));
    }
    doc.to_string()
}

// ──────────────────────────────────────────────────────────
// 缓存（prompt_sha 进指纹）
// ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CacheHeader {
    version: u32,
    prompt_sha_a: String,
    prompt_sha_b: String,
    model: String,
    skill_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    header: CacheHeader,
    /// 指纹 → 回答原文。
    entries: BTreeMap<String, String>,
}

/// 一轮问答的缓存。**同输入第二次 0 token**。
struct Cache {
    path: PathBuf,
    header: CacheHeader,
    entries: BTreeMap<String, String>,
    dirty: bool,
    /// 换了契约/模型 ⇒ 旧缓存被丢弃（必须让人看见）。
    pub reset_reason: Option<String>,
}

impl Cache {
    fn load(path: &Path, header: CacheHeader) -> Result<Self> {
        let mut reset_reason = None;
        let entries = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<CacheFile>(&text) {
                Ok(f) if f.header == header => f.entries,
                Ok(f) => {
                    reset_reason = Some(format!(
                        "缓存头不匹配（旧 prompt_sha/model，新 {} / {}）⇒ 整体失效",
                        header.prompt_sha_a, header.model
                    ));
                    let _ = f;
                    BTreeMap::new()
                }
                Err(e) => {
                    reset_reason = Some(format!("缓存文件读不懂（{e}）⇒ 整体失效"));
                    BTreeMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "读不到缓存文件 {}：{e}（换一个 --cache 路径）",
                    path.display()
                ))
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            header,
            entries,
            dirty: false,
            reset_reason,
        })
    }

    fn fingerprint(&self, round: &str, system_sha: &str, user: &str) -> String {
        let mut s = String::new();
        s.push_str(round);
        s.push('\n');
        s.push_str(system_sha);
        s.push('\n');
        s.push_str(user);
        sample::sha256_hex(s.as_bytes())
    }

    fn get(&self, fp: &str) -> Option<&String> {
        self.entries.get(fp)
    }

    fn put(&mut self, fp: String, answer: String) {
        self.entries.insert(fp, answer);
        self.dirty = true;
    }

    /// 原子落盘（先写 `.tmp` 再改名 —— 半份 JSON 会让下一次运行整体失效）。
    fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("建不出缓存目录 {}", parent.display()))?;
            }
        }
        let text = serde_json::to_string_pretty(&CacheFile {
            header: self.header.clone(),
            entries: self.entries.clone(),
        })?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, format!("{text}\n"))
            .with_context(|| format!("写不出缓存临时文件 {}", tmp.display()))?;
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("缓存落盘失败（改名到 {}）", self.path.display()))?;
        self.dirty = false;
        Ok(())
    }
}

// ──────────────────────────────────────────────────────────
// 引擎：一问一答 + 记账 + 缓存
// ──────────────────────────────────────────────────────────

/// 判定引擎（持有传输 + 缓存 + 两轮协议）。
struct Engine<'a> {
    transport: &'a mut dyn LlmTransport,
    system_a: String,
    system_b: String,
    prompt_sha_a: String,
    prompt_sha_b: String,
    model: String,
    cache: Cache,
    pub round_a: RoundStats,
    pub round_b: RoundStats,
}

/// 一次问答的结果。
struct AskOutcome {
    text: String,
    cached: bool,
    tokens: u64,
}

impl<'a> Engine<'a> {
    fn new(transport: &'a mut dyn LlmTransport, skill: &str, model: &str, cache: Cache) -> Self {
        let system_a = format!("{}\n{PROTOCOL_A}", skill.trim_end());
        let system_b = format!("{}\n{PROTOCOL_B}", skill.trim_end());
        let prompt_sha_a = sample::sha256_hex(system_a.as_bytes());
        let prompt_sha_b = sample::sha256_hex(system_b.as_bytes());
        Self {
            transport,
            system_a,
            system_b,
            prompt_sha_a,
            prompt_sha_b,
            model: model.to_string(),
            cache,
            round_a: RoundStats::default(),
            round_b: RoundStats::default(),
        }
    }

    fn cache_reset_reason(&self) -> Option<String> {
        self.cache.reset_reason.clone()
    }

    fn ask(&mut self, round: &str, user: &str) -> Result<AskOutcome> {
        let (system, sha) = if round == "a" {
            (&self.system_a, &self.prompt_sha_a)
        } else {
            (&self.system_b, &self.prompt_sha_b)
        };
        let fp = self.cache.fingerprint(round, sha, user);
        if let Some(hit) = self.cache.get(&fp) {
            return Ok(AskOutcome {
                text: hit.clone(),
                cached: true,
                tokens: 0,
            });
        }
        let req = ChatRequest {
            model: self.model.clone(),
            messages: vec![Message::system(system.clone()), Message::user(user)],
            max_tokens: 0,
        };
        let resp = self
            .transport
            .complete(&req)
            .with_context(|| format!("{round} 轮模型调用失败"))?;
        let text = resp
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{round} 轮模型返回了空正文（finish_reason = {:?}）——\
                     空回答不是「没有关系」，是这一批失败。",
                    resp.choices.first().and_then(|c| c.finish_reason.clone())
                )
            })?;
        let tokens = resp
            .usage
            .as_ref()
            .map(|u| u.total_tokens())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{round} 轮模型没有回报 usage —— 这一批花了多少客户端无从判断，\
                     按 0 记账就是「把不知道说成免费」。"
                )
            })?;
        self.cache.put(fp, text.clone());
        Ok(AskOutcome {
            text,
            cached: false,
            tokens,
        })
    }

    /// 跑完 A + B 两轮（调用方负责先备好 samples）。
    fn run_rounds(
        &mut self,
        probe: &ProbeReport,
        samples: &TableSamples,
        validator: &Validate,
    ) -> (
        Vec<JudgedRelation>,
        BTreeMap<String, usize>,
        Vec<serde_json::Value>,
    ) {
        let mut relations: Vec<JudgedRelation> = Vec::new();
        let mut drops: BTreeMap<String, usize> = BTreeMap::new();
        let mut unresolved: Vec<serde_json::Value> = Vec::new();
        let mut bump = |drops: &mut BTreeMap<String, usize>, code: &str| {
            *drops.entry(code.to_string()).or_insert(0) += 1;
        };

        // ── A 轮：判定 ──
        let batches = round_a_batches(probe);
        self.round_a.batches = batches.len();
        self.round_a.prompt_sha = self.prompt_sha_a.clone();
        let mut a_answered: BTreeSet<String> = BTreeSet::new();
        for batch in &batches {
            let payload = round_a_payload(probe, batch, samples);
            match self.ask("a", &payload) {
                Ok(out) => {
                    self.round_a.calls += 1;
                    self.round_a.tokens += out.tokens;
                    if out.cached {
                        self.round_a.cache_hits += 1;
                    }
                    match parse_a(&out.text) {
                        Ok(verdicts) => {
                            a_answered.extend(verdicts.iter().map(|v| v.candidate_id.clone()));
                            apply_a(
                                batch,
                                &verdicts,
                                validator,
                                &mut relations,
                                &mut drops,
                                &mut bump,
                            );
                        }
                        Err(e) => {
                            self.round_a.failed_batches += 1;
                            eprintln!(
                                "⚠ A 轮有一批回答不是协议 JSON（{}）—— 这一批作废，继续下一批",
                                short(&e.to_string(), 160)
                            );
                        }
                    }
                }
                Err(e) => {
                    self.round_a.failed_batches += 1;
                    eprintln!(
                        "⚠ A 轮有一批调用失败（{}）—— 这一批作废，继续下一批",
                        short(&e.to_string(), 160)
                    );
                }
            }
        }
        // 模型漏答的候选：不许静默（每条都记进 unresolved）。
        for b in &batches {
            for c in b {
                if !a_answered.contains(&c.id()) {
                    bump(&mut drops, "no_verdict");
                    unresolved.push(serde_json::json!({
                        "code": "no_verdict",
                        "round": "a",
                        "candidate": c.id(),
                        "reason": "模型没有对这一条给出判定（漏答）",
                    }));
                }
            }
        }

        // A 轮跑完先落一次缓存：**失败也要保住已经花掉的 token**
        //（真跑实测：整轮因校验 bug 失败时缓存没落盘，重跑等于白烧一遍）。
        if let Err(e) = self.cache.save() {
            eprintln!("⚠ 判定缓存落盘失败（A 轮）：{e} —— 重跑会重新调用模型");
        }

        // ── B 轮：补漏 ──
        let batches_b = round_b_batches(probe);
        self.round_b.batches = batches_b.len();
        self.round_b.prompt_sha = self.prompt_sha_b.clone();
        let mut per_table: BTreeMap<String, usize> = BTreeMap::new();
        for batch in &batches_b {
            let payload = round_b_payload(batch, samples);
            match self.ask("b", &payload) {
                Ok(out) => {
                    self.round_b.calls += 1;
                    self.round_b.tokens += out.tokens;
                    if out.cached {
                        self.round_b.cache_hits += 1;
                    }
                    match parse_b(&out.text) {
                        Ok(rels) => apply_b(
                            strings_of(batch),
                            &rels,
                            validator,
                            &mut relations,
                            &mut drops,
                            &mut per_table,
                            &mut bump,
                        ),
                        Err(e) => {
                            self.round_b.failed_batches += 1;
                            eprintln!(
                                "⚠ B 轮有一批回答不是协议 JSON（{}）—— 这一批作废，继续下一批",
                                short(&e.to_string(), 160)
                            );
                        }
                    }
                }
                Err(e) => {
                    self.round_b.failed_batches += 1;
                    eprintln!(
                        "⚠ B 轮有一批调用失败（{}）—— 这一批作废，继续下一批",
                        short(&e.to_string(), 160)
                    );
                }
            }
        }

        // B 轮跑完再落一次（失败也要保住已花的 token）。
        if let Err(e) = self.cache.save() {
            eprintln!("⚠ 判定缓存落盘失败（B 轮）：{e} —— 重跑会重新调用模型");
        }
        (relations, drops, unresolved)
    }
}

fn strings_of<'a>(batch: &'a [&'a TableCard]) -> BTreeMap<String, &'a TableCard> {
    batch.iter().map(|c| (c.name.clone(), *c)).collect()
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

// ──────────────────────────────────────────────────────────
// 校验层（**唯一实现**：判据 5 的三条丢弃都在这里）
// ──────────────────────────────────────────────────────────

/// 校验器：吃候选 + 表名片，判一条边能不能落。
pub struct Validate<'a> {
    cards: BTreeMap<&'a str, &'a TableCard>,
}

impl<'a> Validate<'a> {
    pub fn new(probe: &'a ProbeReport) -> Self {
        Self {
            cards: probe.tables.iter().map(|t| (t.name.as_str(), t)).collect(),
        }
    }

    /// 一条边合法 ⇒ `Ok(())`；不合法 ⇒ 丢弃码。
    pub fn check(
        &self,
        child_table: &str,
        child_column: &str,
        parent_table: &str,
        parent_column: &str,
        predicate: &str,
        basis: Option<&ModelBasis>,
    ) -> std::result::Result<(), &'static str> {
        let Some(child) = self.cards.get(child_table) else {
            return Err("unknown_table");
        };
        let Some(parent) = self.cards.get(parent_table) else {
            return Err("unknown_table");
        };
        if child_table == parent_table {
            return Err("self_reference");
        }
        if !child.has_column(child_column) {
            return Err("unknown_column");
        }
        if !parent.has_column(parent_column) {
            return Err("unknown_column");
        }
        // 需求 Step 4 的「类型可比较」（QA F-1）：`timestamp` 列不可能 join `text` 列。
        // 同族判定在 `relation::comparable`（唯一实现；probe 的值域验证也用它）。
        let child_type = child
            .columns
            .iter()
            .find(|c| c.name == child_column)
            .map(|c| c.data_type.as_str())
            .unwrap_or("");
        let parent_type = parent
            .columns
            .iter()
            .find(|c| c.name == parent_column)
            .map(|c| c.data_type.as_str())
            .unwrap_or("");
        if !super::relation::comparable(child_type, parent_type) {
            return Err("type_incompatible");
        }
        if !parent.key_columns().iter().any(|k| k == parent_column) {
            return Err("target_not_key");
        }
        if !is_predicate(predicate) {
            return Err("unknown_predicate");
        }
        let Some(b) = basis else {
            return Err("no_basis");
        };
        if !matches!(
            b.kind.as_str(),
            "domain" | "naming" | "comment" | "samelenum"
        ) {
            return Err("no_basis");
        }
        if b.detail.trim().is_empty() {
            return Err("no_basis");
        }
        // 依据点名的列必须能落到两端之一（⛔ 不许点「空气列」）。
        //
        // ⚠️ 真跑实测（REQ-172）：模型会写**限定名**（`balances.customer_id`），
        // 而名片里存的是裸列名（`customer_id`）。第一版按裸名列逐字比对，
        // 真 UAT 上 135/140 条 hold 被误丢 —— 这是**校验层的 bug**，不是模型编造。
        // 现在：限定名按「表.列」拆开核对；只要**至少一个**引用能落到实处就算有依据
        // （全都不存在 ⇒ `basis_column_unknown`）。
        if !b.columns.is_empty() {
            let any_lands = b.columns.iter().any(|c| column_ref_lands(c, child, parent));
            if !any_lands {
                return Err("basis_column_unknown");
            }
        }
        Ok(())
    }
}

/// 依据里的一个列引用能不能落到这两张表上（裸名 / `表.列` / 带引号都认）。
fn column_ref_lands(reference: &str, child: &TableCard, parent: &TableCard) -> bool {
    let r = reference.trim().trim_matches('`');
    if r.is_empty() {
        return false;
    }
    let unquote = |s: &str| s.trim().trim_matches('"').trim_matches('`').to_string();
    if let Some((table, column)) = r.rsplit_once('.') {
        let table = unquote(table);
        let column = unquote(column);
        if table == child.name {
            return child.has_column(&column);
        }
        if table == parent.name {
            return parent.has_column(&column);
        }
        return false;
    }
    let column = unquote(r);
    child.has_column(&column) || parent.has_column(&column)
}

/// 模型自报的依据（严格形状）。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelBasis {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AVerdict {
    candidate_id: String,
    verdict: String,
    #[serde(default)]
    predicate: String,
    #[serde(default)]
    basis: Option<ModelBasis>,
    #[serde(default)]
    confidence: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct AAnswer {
    verdicts: Vec<AVerdict>,
}

#[derive(Debug, Clone, Deserialize)]
struct BRelation {
    #[serde(default)]
    table: String,
    child_table: String,
    child_column: String,
    parent_table: String,
    parent_column: String,
    predicate: String,
    #[serde(default)]
    cardinality: String,
    #[serde(default)]
    basis: Option<ModelBasis>,
}

#[derive(Debug, Clone, Deserialize)]
struct BAnswer {
    relations: Vec<BRelation>,
}

fn parse_json<T: for<'de> Deserialize<'de>>(text: &str, what: &str) -> Result<T> {
    let t = text.trim();
    if t.is_empty() {
        bail!("{what}：回答是空的");
    }
    // 严格：只接受一个 JSON 对象。模型若包了代码块/散文，这里就失败（⛔ 不从散文里抠 JSON）。
    serde_json::from_str::<T>(t).map_err(|e| {
        anyhow::anyhow!(
            "{what}：回答不是协议 JSON（{e}）。开头 80 字符：{:?}",
            t.chars().take(80).collect::<String>()
        )
    })
}

fn parse_a(text: &str) -> Result<Vec<AVerdict>> {
    Ok(parse_json::<AAnswer>(text, "A 轮")?.verdicts)
}

fn parse_b(text: &str) -> Result<Vec<BRelation>> {
    Ok(parse_json::<BAnswer>(text, "B 轮")?.relations)
}

fn candidate_evidence(c: &RelationCandidate) -> Vec<JudgeEvidence> {
    c.evidence
        .iter()
        .map(|e| JudgeEvidence {
            source: e.source.clone(),
            detail: e.detail.clone(),
            matched: e.matched,
            sampled: e.sampled,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn apply_a(
    batch: &[&RelationCandidate],
    verdicts: &[AVerdict],
    validator: &Validate,
    out: &mut Vec<JudgedRelation>,
    drops: &mut BTreeMap<String, usize>,
    bump: &mut impl FnMut(&mut BTreeMap<String, usize>, &str),
) {
    let by_id: BTreeMap<String, &RelationCandidate> = batch.iter().map(|c| (c.id(), *c)).collect();
    for v in verdicts {
        let Some(c) = by_id.get(&v.candidate_id) else {
            bump(drops, "unknown_candidate");
            continue;
        };
        match v.verdict.as_str() {
            "rejects" | "insufficient" => {
                bump(drops, "nothing_held");
                continue;
            }
            "holds" => {}
            _ => {
                bump(drops, "unknown_verdict");
                continue;
            }
        }
        // ⚠️ 模型说 domain，但值域验证没确认 ⇒ **降级**，不当作值域证据。
        let mut basis = v.basis.clone();
        if basis.as_ref().map(|b| b.kind.as_str()) == Some("domain")
            && !c.domain.as_ref().map_or(false, |d| d.subset)
        {
            bump(drops, "basis_downgraded");
            if let Some(b) = basis.as_mut() {
                b.kind = if c.evidence.iter().any(|e| e.source == "comment") {
                    "comment".to_string()
                } else if c.evidence.iter().any(|e| e.source == "samelenum") {
                    "samelenum".to_string()
                } else {
                    "naming".to_string()
                };
            }
        }
        if let Err(code) = validator.check(
            &c.child_table,
            &c.child_column,
            &c.parent_table,
            &c.parent_column,
            &v.predicate,
            basis.as_ref(),
        ) {
            bump(drops, code);
            continue;
        }
        let source = if basis.as_ref().map(|b| b.kind.as_str()) == Some("domain") {
            "domain"
        } else if c.evidence.iter().any(|e| e.source == "naming") {
            "naming"
        } else {
            "ai"
        };
        let mut evidence = candidate_evidence(c);
        if let Some(b) = &basis {
            evidence.push(JudgeEvidence {
                source: "ai".to_string(),
                detail: format!("模型判定（{} / {}）：{}", b.kind, v.verdict, b.detail),
                matched: None,
                sampled: None,
            });
        }
        let score = c.score.max(v.confidence.unwrap_or(0.0).clamp(0.0, 1.0));
        out.push(JudgedRelation {
            child_table: c.child_table.clone(),
            child_column: c.child_column.clone(),
            parent_table: c.parent_table.clone(),
            parent_column: c.parent_column.clone(),
            predicate: v.predicate.clone(),
            cardinality: c.cardinality().to_string(),
            source: source.to_string(),
            score: (score * 100.0).round() / 100.0,
            band: "low".to_string(),
            round: "a".to_string(),
            basis: basis
                .as_ref()
                .map(|b| format!("模型判定 holds：{}", b.detail))
                .unwrap_or_else(|| "模型判定 holds".to_string()),
            evidence,
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_b(
    allowed: BTreeMap<String, &TableCard>,
    rels: &[BRelation],
    validator: &Validate,
    out: &mut Vec<JudgedRelation>,
    drops: &mut BTreeMap<String, usize>,
    per_table: &mut BTreeMap<String, usize>,
    bump: &mut impl FnMut(&mut BTreeMap<String, usize>, &str),
) {
    for r in rels {
        // 这一批只对批内的表负责（索引里没有的表，模型不该点）。
        if !allowed.contains_key(&r.child_table) || !allowed.contains_key(&r.parent_table) {
            bump(drops, "unknown_table");
            continue;
        }
        if let Err(code) = validator.check(
            &r.child_table,
            &r.child_column,
            &r.parent_table,
            &r.parent_column,
            &r.predicate,
            r.basis.as_ref(),
        ) {
            bump(drops, code);
            continue;
        }
        // 每表 ≤3：按「属于哪张表」计（表名可能是 child 也可能是 focus）。
        let owner = if r.table.is_empty() {
            r.child_table.clone()
        } else if allowed.contains_key(&r.table) {
            r.table.clone()
        } else {
            bump(drops, "unknown_table");
            continue;
        };
        let n = per_table.entry(owner.clone()).or_insert(0);
        if *n >= MAX_PER_TABLE_B {
            bump(drops, "too_many_per_table");
            continue;
        }
        *n += 1;
        let Some(card) = allowed.get(&r.child_table).copied() else {
            continue;
        };
        let child_unique = card.unique_columns.iter().any(|u| u == &r.child_column);
        let cardinality = if r.cardinality.is_empty() {
            if child_unique {
                "1:1".to_string()
            } else {
                "N:1".to_string()
            }
        } else {
            r.cardinality.clone()
        };
        let basis = r.basis.clone().unwrap_or_default();
        out.push(JudgedRelation {
            child_table: r.child_table.clone(),
            child_column: r.child_column.clone(),
            parent_table: r.parent_table.clone(),
            parent_column: r.parent_column.clone(),
            predicate: r.predicate.clone(),
            cardinality,
            // 判据 4：B 轮一律 `source: ai`（低置信，人签）。
            source: "ai".to_string(),
            score: 0.45,
            band: "low".to_string(),
            round: "b".to_string(),
            basis: format!("模型补漏（{}）：{}", basis.kind, basis.detail),
            evidence: vec![JudgeEvidence {
                source: "ai".to_string(),
                detail: format!("模型补漏（{}）：{}", basis.kind, basis.detail),
                matched: None,
                sampled: None,
            }],
        });
    }
}

/// 合并 A/B 两轮：同边留强、反向同谓词归一、去掉完全重复。
pub fn normalize(relations: &mut Vec<JudgedRelation>, drops: &mut BTreeMap<String, usize>) {
    let bump = |drops: &mut BTreeMap<String, usize>, code: &str| {
        *drops.entry(code.to_string()).or_insert(0) += 1;
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut keep: Vec<JudgedRelation> = Vec::new();
    for r in relations.drain(..) {
        let id = r.id();
        if let Some(&idx) = seen.get(&id) {
            // 同一条边：A 轮优先（证据更硬），否则保留先到的。
            if keep[idx].round == "b" && r.round == "a" {
                keep[idx] = r;
            } else {
                let extra = r.evidence.clone();
                for e in extra {
                    if !keep[idx]
                        .evidence
                        .iter()
                        .any(|x| x.source == e.source && x.detail == e.detail)
                    {
                        keep[idx].evidence.push(e);
                    }
                }
                keep[idx].score = keep[idx].score.max(r.score);
            }
            bump(drops, "duplicate");
            continue;
        }
        // 反向同谓词：`a.b->c.d` 与 `c.d->a.b` 是同一件事的两种说法 ⇒ 归一。
        let rev = format!(
            "{}.{}->{}.{}",
            r.parent_table, r.parent_column, r.child_table, r.child_column
        );
        if let Some(&idx) = seen.get(&rev) {
            if keep[idx].predicate == r.predicate {
                bump(drops, "reverse_duplicate");
                continue;
            }
        }
        seen.insert(id, keep.len());
        keep.push(r);
    }
    *relations = keep;
}

// ──────────────────────────────────────────────────────────
// 样本取数（只读；只进 prompt，不落盘）
// ──────────────────────────────────────────────────────────

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// 每张表取 2~3 行样本（优先主键 + 关系列 + 前几列，列数 ≤8）。
///
/// 只读由调用方的池保证（`sample_pool`）；返回的字符串已经在
/// [`mask_value`] 里截断。样本**只进 prompt**，产物里没有任何样本值。
pub async fn fetch_samples(
    pool: &PgPool,
    schema: &str,
    cards: &[TableCard],
    extra: &BTreeMap<String, BTreeSet<String>>,
    rows: usize,
) -> (TableSamples, usize) {
    let mut out: TableSamples = BTreeMap::new();
    let mut failures = 0usize;
    for card in cards {
        let mut cols: Vec<String> = Vec::new();
        if let Some(pk) = &card.primary_key {
            cols.push(pk.clone());
        }
        if let Some(es) = extra.get(&card.name) {
            for c in es {
                if !cols.contains(c) {
                    cols.push(c.clone());
                }
            }
        }
        for c in &card.columns {
            if cols.len() >= 8 {
                break;
            }
            if !cols.contains(&c.name) {
                cols.push(c.name.clone());
            }
        }
        if cols.is_empty() {
            continue;
        }
        let select: Vec<String> = cols.iter().map(|c| format!("{}::text", q(c))).collect();
        let order = card
            .primary_key
            .as_ref()
            .map(|pk| format!(" ORDER BY {}", q(pk)))
            .unwrap_or_default();
        let sql = format!(
            "SELECT {} FROM {}.{}{} LIMIT $1",
            select.join(", "),
            q(schema),
            q(&card.name),
            order
        );
        let got = tokio::time::timeout(
            super::relation::DOMAIN_TIMEOUT,
            sqlx::query(&sql).bind(rows as i64).fetch_all(pool),
        )
        .await;
        let rows_out = match got {
            Err(_) => None,
            Ok(Err(_)) => None,
            Ok(Ok(rs)) => {
                let mut v: Vec<BTreeMap<String, String>> = Vec::new();
                for row in rs {
                    let mut m = BTreeMap::new();
                    for (i, c) in cols.iter().enumerate() {
                        let val: Option<String> = row.try_get(i).ok();
                        m.insert(
                            c.clone(),
                            val.map(|s| mask_value(&s))
                                .unwrap_or_else(|| "NULL".to_string()),
                        );
                    }
                    v.push(m);
                }
                Some(v)
            }
        };
        match rows_out {
            Some(v) => {
                out.insert(card.name.clone(), v);
            }
            None => failures += 1,
        }
    }
    (out, failures)
}

// ──────────────────────────────────────────────────────────
// 入口：跑两轮 → 产物
// ──────────────────────────────────────────────────────────

/// 判定配置。
#[derive(Debug, Clone)]
pub struct JudgeOptions {
    /// 每表取几行样本（默认 [`DEFAULT_SAMPLE_ROWS`]）。
    pub sample_rows: usize,
    /// 缓存落点（默认 `<out>.cache.json`）。
    pub cache: PathBuf,
    pub out: PathBuf,
    /// 候选文件名（留痕用）。
    pub candidates_file: String,
    pub candidates_sha256: String,
    pub skill_file: String,
    pub skill_sha256: String,
}

/// 跑两轮 + 校验 + 落产物。
///
/// `card_samples` 由调用方先取（[`fetch_samples`]）；`engine` 持有传输与缓存。
/// 部分批次失败会写产物并在 `stats.failed_batches` 里留下数字；
/// **全部失败 / 一条关系都没留下 ⇒ `bail!`**（⛔ 不产半份、不静默降级）。
pub fn run(
    probe: &ProbeReport,
    skill: &str,
    model: &str,
    meta: &JudgeOptions,
    card_samples: TableSamples,
    sample_failures: usize,
    transport: &mut dyn LlmTransport,
) -> Result<JudgeReport> {
    let header = CacheHeader {
        version: CACHE_VERSION,
        prompt_sha_a: sample::sha256_hex(format!("{}\n{PROTOCOL_A}", skill.trim_end()).as_bytes()),
        prompt_sha_b: sample::sha256_hex(format!("{}\n{PROTOCOL_B}", skill.trim_end()).as_bytes()),
        model: model.to_string(),
        skill_sha256: meta.skill_sha256.clone(),
    };
    let cache = Cache::load(&meta.cache, header)?;
    let mut engine = Engine::new(transport, skill, model, cache);
    if let Some(reason) = engine.cache_reset_reason() {
        eprintln!("· 判定缓存：{reason}");
    }

    let validator = Validate::new(probe);
    let (mut relations, mut drops, unresolved) =
        engine.run_rounds(probe, &card_samples, &validator);
    normalize(&mut relations, &mut drops);

    let calls = engine.round_a.calls + engine.round_b.calls;
    let failed = engine.round_a.failed_batches + engine.round_b.failed_batches;
    if calls > 0 && failed >= calls {
        bail!(
            "relation judge 的**每一次模型调用都失败了**（A {} 批 / B {} 批）——\
             本轮不产出任何判定文件。\n\
             ⛔ 没有回落到「只做规则」：那会让「AI 没判」看起来像「AI 判了不成立」。",
            engine.round_a.batches,
            engine.round_b.batches
        );
    }
    if relations.is_empty() {
        let mut codes: Vec<String> = drops.iter().map(|(k, v)| format!("{k}={v}")).collect();
        codes.sort();
        bail!(
            "relation judge 跑完了，但**一条关系都没留下**（A {} 批 / B {} 批，丢弃：{}）——\
             本轮不产出判定文件。\n\
             为什么算失败：空 `relations.json` 与「模型判定全部不成立」在产物里长得一样，\
             而下游会把它当全量吃掉。\n\
             如果是模型确实全部判不成立，请把 `--candidates` 换成更可信的候选集再跑。",
            engine.round_a.batches,
            engine.round_b.batches,
            if codes.is_empty() {
                "无".to_string()
            } else {
                codes.join(" · ")
            }
        );
    }

    let mut out = JudgeReport {
        version: JUDGE_VERSION,
        kind: JUDGED_KIND.to_string(),
        method: JUDGE_METHOD.to_string(),
        source: JudgeSource {
            candidates_file: meta.candidates_file.clone(),
            candidates_sha256: meta.candidates_sha256.clone(),
            skill_file: meta.skill_file.clone(),
            skill_sha256: meta.skill_sha256.clone(),
            model: model.to_string(),
            agent: String::new(),
            agent_version: String::new(),
        },
        round_a: engine.round_a.clone(),
        round_b: engine.round_b.clone(),
        relations,
        drops,
        unresolved,
        stats: JudgeStats {
            candidates: probe.candidates.len(),
            tables: probe.tables.len(),
            sample_failures,
            relations: 0,
            by_source: BTreeMap::new(),
            round_b_relations: 0,
            round_b_tables: 0,
        },
    };
    out.sort();
    out.stats.round_b_relations = out.relations.iter().filter(|r| r.round == "b").count();
    let b_tables: BTreeSet<&str> = out
        .relations
        .iter()
        .filter(|r| r.round == "b")
        .map(|r| r.child_table.as_str())
        .collect();
    out.stats.round_b_tables = b_tables.len();
    engine.cache.save()?;
    Ok(out)
}

/// 给 CLI 的「一次问答」封装：CLI 只负责把候选/样本递进来，协议与记账都在本模块。
///
/// （保留这个薄壳是为了让 `cli` 不直接碰 [`Engine`]，也方便测试只调 [`run`]。）
pub fn prompt_sha_a(skill: &str) -> String {
    sample::sha256_hex(format!("{}\n{PROTOCOL_A}", skill.trim_end()).as_bytes())
}

pub fn prompt_sha_b(skill: &str) -> String {
    sample::sha256_hex(format!("{}\n{PROTOCOL_B}", skill.trim_end()).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::relation::{
        CandidateEvidence, ProbeReport, ProbeSource, ProbeStats, RelationCandidate, TableCard,
    };

    fn card(name: &str, pk: &str, cols: &[&str]) -> TableCard {
        TableCard {
            name: name.to_string(),
            display_cn: Some(format!("{name}中文")),
            primary_key: Some(pk.to_string()),
            unique_columns: vec![pk.to_string()],
            columns: cols
                .iter()
                .map(|c| super::super::relation::ColumnCard {
                    name: c.to_string(),
                    data_type: "character varying".to_string(),
                    nullable: false,
                    is_primary_key: *c == pk,
                    comment: None,
                })
                .collect(),
        }
    }

    /// 带**真实类型**的名片（类型可比性判据要它）。
    fn typed_card(name: &str, pk: &str, cols: &[(&str, &str, bool)]) -> TableCard {
        TableCard {
            name: name.to_string(),
            display_cn: Some(format!("{name}中文")),
            primary_key: Some(pk.to_string()),
            unique_columns: vec![pk.to_string()],
            columns: cols
                .iter()
                .map(|(c, ty, is_pk)| super::super::relation::ColumnCard {
                    name: c.to_string(),
                    data_type: ty.to_string(),
                    nullable: false,
                    is_primary_key: *is_pk,
                    comment: None,
                })
                .collect(),
        }
    }

    fn candidate(id_child: &str, parent: &str, key: &str) -> RelationCandidate {
        RelationCandidate {
            child_table: id_child.to_string(),
            child_column: "customer_id".to_string(),
            parent_table: parent.to_string(),
            parent_column: key.to_string(),
            target_kind: "primary_key".to_string(),
            evidence: vec![CandidateEvidence {
                source: "naming".to_string(),
                detail: "命名：customer_id 命中 customers".to_string(),
                matched: None,
                sampled: None,
            }],
            score: 0.55,
            band: "medium".to_string(),
            domain: None,
            child_unique: false,
        }
    }

    fn probe() -> ProbeReport {
        ProbeReport {
            version: 1,
            kind: "relation-candidates".to_string(),
            method: "relation-probe-v1".to_string(),
            source: ProbeSource {
                schema_file: "s.json".to_string(),
                schema_sha256: "x".to_string(),
                schema: "public".to_string(),
                read_only: "on".to_string(),
                dsn: "h/db".to_string(),
                tables: 3,
                columns: 9,
                sample_rows: 50,
            },
            tables: vec![
                card("customers", "id", &["id", "name"]),
                card("balances", "id", &["id", "customer_id"]),
                card("orphan", "id", &["id", "status"]),
            ],
            candidates: vec![candidate("balances", "customers", "id")],
            unresolved: vec![],
            stats: ProbeStats::default(),
        }
    }

    fn basis(kind: &str) -> ModelBasis {
        ModelBasis {
            kind: kind.to_string(),
            columns: vec!["customer_id".to_string(), "id".to_string()],
            detail: "值域 5/5 全部落在 customers.id".to_string(),
        }
    }

    #[test]
    fn a_round_keeps_holds_and_drops_missing_basis() {
        let p = probe();
        let v = Validate::new(&p);
        let batch: Vec<&RelationCandidate> = p.candidates.iter().collect();
        let verdicts = vec![
            AVerdict {
                candidate_id: "balances.customer_id->customers.id".to_string(),
                verdict: "holds".to_string(),
                predicate: "belongs_to".to_string(),
                basis: Some(basis("domain")),
                confidence: Some(0.9),
            },
            AVerdict {
                candidate_id: "balances.customer_id->customers.id".to_string(),
                verdict: "holds".to_string(),
                predicate: "belongs_to".to_string(),
                basis: None,
                confidence: Some(0.9),
            },
        ];
        let mut out = Vec::new();
        let mut drops = BTreeMap::new();
        let mut bump = |drops: &mut BTreeMap<String, usize>, code: &str| {
            *drops.entry(code.to_string()).or_insert(0) += 1;
        };
        // 第二次是「同一条候选、无依据」—— 真实协议里不会重复，但校验层必须挡住。
        apply_a(&batch, &verdicts, &v, &mut out, &mut drops, &mut bump);
        assert_eq!(out.len(), 1, "无依据的那条必须被丢");
        assert_eq!(drops.get("no_basis"), Some(&1));
        let dropped = {
            let mut o = Vec::new();
            let mut d = BTreeMap::new();
            apply_a(
                &batch,
                &[AVerdict {
                    candidate_id: "balances.customer_id->customers.id".to_string(),
                    verdict: "holds".to_string(),
                    predicate: "belongs_to".to_string(),
                    basis: None,
                    confidence: None,
                }],
                &v,
                &mut o,
                &mut d,
                &mut bump,
            );
            (o.len(), d)
        };
        assert_eq!(dropped.0, 0);
        assert_eq!(dropped.1.get("no_basis"), Some(&1));
    }

    #[test]
    fn a_round_rejects_criterion_5_three_ways() {
        let p = probe();
        let v = Validate::new(&p);
        // 谓词不在词表
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "id",
                "uses",
                Some(&basis("naming"))
            ),
            Err("unknown_predicate")
        );
        // 列不存在
        assert_eq!(
            v.check(
                "balances",
                "nope",
                "customers",
                "id",
                "belongs_to",
                Some(&basis("naming"))
            ),
            Err("unknown_column")
        );
        // 目标不是键
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "name",
                "belongs_to",
                Some(&basis("naming"))
            ),
            Err("target_not_key")
        );
        // 依据点了空气列
        let mut b = basis("naming");
        b.columns = vec!["ghost".to_string()];
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "id",
                "belongs_to",
                Some(&b)
            ),
            Err("basis_column_unknown")
        );
        // 合法
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "id",
                "belongs_to",
                Some(&basis("naming"))
            ),
            Ok(())
        );
    }

    /// 真 pi-acp 会写**限定名**（`表.列`）—— 校验层必须认（REQ-172 真跑踩过）。
    #[test]
    fn qualified_basis_columns_are_accepted() {
        let p = probe();
        let v = Validate::new(&p);
        let mut b = basis("domain");
        b.columns = vec![
            "balances.customer_id".to_string(),
            "customers.id".to_string(),
        ];
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "id",
                "belongs_to",
                Some(&b)
            ),
            Ok(())
        );
        // 一个限定名落地 + 一个怪东西 ⇒ 仍算有依据（全都不落地才丢）
        b.columns = vec![
            "balances.customer_id".to_string(),
            "no_such.thing".to_string(),
        ];
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "id",
                "belongs_to",
                Some(&b)
            ),
            Ok(())
        );
        // 带引号的限定名
        b.columns = vec!["\"balances\".\"customer_id\"".to_string()];
        assert_eq!(
            v.check(
                "balances",
                "customer_id",
                "customers",
                "id",
                "belongs_to",
                Some(&b)
            ),
            Ok(())
        );
    }

    /// QA F-1 · 需求 Step 4 的「类型可比较」：`timestamp` → `text` 必须被丢。
    #[test]
    fn incompatible_column_types_are_dropped_as_type_incompatible() {
        let mut p = probe();
        p.tables.push(typed_card(
            "events",
            "code",
            &[("code", "text", true)],
        ));
        p.tables.push(typed_card(
            "logs",
            "id",
            &[
                ("id", "text", true),
                ("created_at", "timestamp without time zone", false),
            ],
        ));
        let v = Validate::new(&p);
        let b = ModelBasis {
            kind: "naming".to_string(),
            columns: vec!["logs.created_at".to_string(), "events.code".to_string()],
            detail: "命名看起来像".to_string(),
        };
        assert_eq!(
            v.check(
                "logs",
                "created_at",
                "events",
                "code",
                "belongs_to",
                Some(&b)
            ),
            Err("type_incompatible"),
            "timestamp 列不可能 join text 列"
        );
        // 同族（uuid → uuid）仍应通过：新规则只挡跨族
        p.tables.push(typed_card(
            "uuid_parent",
            "id",
            &[("id", "uuid", true)],
        ));
        p.tables.push(typed_card(
            "uuid_child",
            "id",
            &[
                ("id", "uuid", true),
                ("parent_id", "uuid", false),
            ],
        ));
        let v = Validate::new(&p);
        let b2 = ModelBasis {
            kind: "naming".to_string(),
            columns: vec!["uuid_child.parent_id".to_string(), "uuid_parent.id".to_string()],
            detail: "同为 uuid".to_string(),
        };
        assert_eq!(
            v.check(
                "uuid_child",
                "parent_id",
                "uuid_parent",
                "id",
                "belongs_to",
                Some(&b2)
            ),
            Ok(())
        );
    }

    #[test]
    fn domain_claim_without_domain_evidence_is_downgraded_not_trusted() {
        let p = probe();
        let v = Validate::new(&p);
        let batch: Vec<&RelationCandidate> = p.candidates.iter().collect();
        let mut out = Vec::new();
        let mut drops = BTreeMap::new();
        let mut bump = |drops: &mut BTreeMap<String, usize>, code: &str| {
            *drops.entry(code.to_string()).or_insert(0) += 1;
        };
        apply_a(
            &batch,
            &[AVerdict {
                candidate_id: "balances.customer_id->customers.id".to_string(),
                verdict: "holds".to_string(),
                predicate: "belongs_to".to_string(),
                basis: Some(basis("domain")),
                confidence: None,
            }],
            &v,
            &mut out,
            &mut drops,
            &mut bump,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, "naming", "没有值域确认 ⇒ 不许标 domain");
        assert_eq!(drops.get("basis_downgraded"), Some(&1));
    }

    #[test]
    fn b_round_keeps_source_ai_and_caps_three_per_table() {
        let p = probe();
        let v = Validate::new(&p);
        let batch: Vec<&TableCard> = p
            .tables
            .iter()
            .filter(|t| t.name == "orphan" || t.name == "customers")
            .collect();
        let allowed = strings_of(&batch);
        let rels: Vec<BRelation> = (0..5)
            .map(|_| BRelation {
                table: "orphan".to_string(),
                child_table: "orphan".to_string(),
                child_column: "status".to_string(),
                parent_table: "customers".to_string(),
                parent_column: "id".to_string(),
                predicate: "references".to_string(),
                cardinality: String::new(),
                basis: Some(ModelBasis {
                    kind: "comment".to_string(),
                    columns: vec!["status".to_string()],
                    detail: "注释提到 status".to_string(),
                }),
            })
            .collect();
        let mut out = Vec::new();
        let mut drops = BTreeMap::new();
        let mut per = BTreeMap::new();
        let mut bump = |drops: &mut BTreeMap<String, usize>, code: &str| {
            *drops.entry(code.to_string()).or_insert(0) += 1;
        };
        apply_b(
            allowed, &rels, &v, &mut out, &mut drops, &mut per, &mut bump,
        );
        assert_eq!(out.len(), MAX_PER_TABLE_B);
        assert_eq!(drops.get("too_many_per_table"), Some(&2));
        assert!(out.iter().all(|r| r.source == "ai" && r.band == "low"));
    }

    #[test]
    fn cache_is_byte_stable_and_invalidated_by_prompt_sha() {
        let dir = std::env::temp_dir().join(format!(
            "ontok-req172-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.json");
        let h = CacheHeader {
            version: 1,
            prompt_sha_a: "a".into(),
            prompt_sha_b: "b".into(),
            model: "m".into(),
            skill_sha256: "s".into(),
        };
        {
            let mut c = Cache::load(&path, h.clone()).unwrap();
            c.put("fp".into(), "answer".into());
            c.save().unwrap();
        }
        let c = Cache::load(&path, h).unwrap();
        assert_eq!(c.get("fp").map(String::as_str), Some("answer"));
        let h2 = CacheHeader {
            version: 1,
            prompt_sha_a: "a2".into(),
            prompt_sha_b: "b".into(),
            model: "m".into(),
            skill_sha256: "s".into(),
        };
        let c2 = Cache::load(&path, h2).unwrap();
        assert!(c2.get("fp").is_none(), "换 prompt_sha ⇒ 整体失效");
        assert!(c2.reset_reason.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn protocol_texts_carry_the_16_predicate_rule() {
        assert!(PROTOCOL_A.contains("16 个主键"));
        assert!(PROTOCOL_B.contains("每张表最多 3 条"));
        assert!(PROTOCOL_B.contains("主键或单列唯一列"));
    }

    #[test]
    fn reverse_edges_are_normalised() {
        let rel = |child: &str, ccol: &str, parent: &str, pcol: &str| JudgedRelation {
            child_table: child.into(),
            child_column: ccol.into(),
            parent_table: parent.into(),
            parent_column: pcol.into(),
            predicate: "belongs_to".into(),
            cardinality: "N:1".into(),
            source: "naming".into(),
            score: 0.5,
            band: "low".into(),
            round: "a".into(),
            basis: "b".into(),
            evidence: vec![],
        };
        let mut v = vec![
            rel("balances", "customer_id", "customers", "id"),
            rel("customers", "id", "balances", "customer_id"),
        ];
        let mut drops = BTreeMap::new();
        normalize(&mut v, &mut drops);
        assert_eq!(v.len(), 1);
        assert_eq!(drops.get("reverse_duplicate"), Some(&1));
    }

    #[test]
    fn masking_truncates_and_strips_controls() {
        let long = "x".repeat(40);
        let m = mask_value(&long);
        assert_eq!(m.chars().count(), SAMPLE_CHARS + 1);
        assert!(!mask_value("a\u{0}b").contains('\u{0}'));
    }

    #[test]
    fn round_a_batches_never_exceed_ten() {
        let p = probe_with_many(51);
        for b in round_a_batches(&p) {
            assert!(b.len() <= ROUND_A_BATCH);
        }
    }

    #[test]
    fn round_b_batches_never_exceed_twelve() {
        let mut p = probe();
        p.tables = (0..25)
            .map(|i| card(&format!("t{i}"), "id", &["id", "v"]))
            .collect();
        for b in round_b_batches(&p) {
            assert!(b.len() <= ROUND_B_BATCH);
        }
    }

    fn probe_with_many(n: usize) -> ProbeReport {
        let mut p = probe();
        p.candidates = (0..n)
            .map(|i| candidate(&format!("t{i}"), "customers", "id"))
            .collect();
        p
    }

    #[test]
    fn no_verdict_is_recorded_not_silently_ignored() {
        // 直接验证 apply_a 不会把「漏答」当「不成立」：run_rounds 里另有 no_verdict 计数，
        // 这里只确认空 verdict 列表不会产出关系。
        let p = probe();
        let v = Validate::new(&p);
        let batch: Vec<&RelationCandidate> = p.candidates.iter().collect();
        let mut out = Vec::new();
        let mut drops = BTreeMap::new();
        let mut bump = |drops: &mut BTreeMap<String, usize>, code: &str| {
            *drops.entry(code.to_string()).or_insert(0) += 1;
        };
        apply_a(&batch, &[], &v, &mut out, &mut drops, &mut bump);
        assert!(out.is_empty());
        assert!(drops.is_empty());
    }
}
