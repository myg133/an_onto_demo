//! ⭐ REQ-170 · **库结构摘要**（`schema.json` → 进 prompt 的那一小块 + `maps_to` 的校验面）。
//!
//! # 这个文件补的是链路里断掉的那一节
//!
//! 三样东西本来都在：`introspect`（库 → `schema.json`）、`doc_merge`（吃关系断言上的
//! `maps_to`）、`r2rml generate`（schema + 本体 → TTL）。断点只有一个：
//! **抽取侧从来看不到库** —— `doc ingest` 把每一块原文发给模型时，prompt 里没有一张表、
//! 没有一个列名。于是模型不可能给出 `maps_to`，`doc_merge` 就只能把每条关系记成
//! `doc_unmapped`（`src/extract/doc_merge.rs` 的规则 3）。
//!
//! 本模块做两件事，都在**抽取侧**（`src/extract/**`，⛔ 不碰 `src/db/**`）：
//!
//! | # | 做什么 | 谁用 |
//! | --- | --- | --- |
//! | 1 | 把 schema 压成一段**有体积上限**的摘要（表名 / 中文名 / 列名 / 类型 / 既有关系） | `doc ingest` 的 prompt（与 skill 同一条 system 通道） |
//! | 2 | 把模型给的 `maps_to` **落到具体表**并逐列校验 | `doc_ingest::validate`（查不到 ⇒ 丢弃 + 计数） |
//!
//! # 三条硬纪律
//!
//! 1. **大库不许全塞**。摘要按「文档里出现过的概念名」给表打分，只带 top-N
//!    （[`DEFAULT_MAX_TABLES`]，`--schema-max-tables` 可覆盖），并再兜一道字符上限
//!    [`MAX_DIGEST_CHARS`]。300 张表全塞进每块 prompt 的后果不是「贵一点」，
//!    而是**每块都超上下文**、抽取整体失败。
//! 2. **没匹配上就一张都不带**，⛔ 不拿无关的表凑数。凑数会让模型照着不相干的表名编
//!    `maps_to` —— 那正是 `doc_merge` 规则 3 明令禁止的「按名字猜表」。
//! 3. **表名 / 列名不猜**。`maps_to` 只认两端列名（简报的形状
//!    `{child_column,parent_column}`）；表由「概念名 ↔ 表名/中文名」或「这一列只在一张表里」
//!    唯一确定。对不上/对上多张 ⇒ 丢弃并计数（[`MapsToProblem`]），⛔ 不选一个「最像的」。
//!
//! # 为什么单独成文件
//!
//! `doc_merge` 已经有一份 `SchemaIndex`（表 → 列名集合），但它只服务**合并**（严格的
//! `has_table` / `has_column`），没有中文名、没有打分、也不该有 prompt 文案。放在一起
//! 会让「合并规则的唯一实现」和「给模型的摘要」混成一份，改一个动另一个。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use super::doc_merge::{Endpoint, MapsTo};

/// 每块 prompt 里**缺省**最多带几张表。
///
/// 40 这个数的口径：真实源库（96 张表 / 1068 列）里一篇文档通常只涉及个位数张表，
/// 40 已经比「相关表」宽 4 倍以上；而 40 张表的摘要 ≈ 6~9 KB，对上下文很便宜。
/// 大库（300+ 表）可以用 `--schema-max-tables` 再调。
pub const DEFAULT_MAX_TABLES: usize = 40;

/// 摘要正文的**字符**上限（兜底闸，独立于表数）。
///
/// 表数闸管「几行」，字符闸管「行有多长」—— 一张 200 列的表能把 40 张表的摘要顶到
/// 几十 KB。两道闸都过才进 prompt。
pub const MAX_DIGEST_CHARS: usize = 12_000;

/// schema.json 里一张表的**最小读取形状**。
///
/// ⚠️ 刻意**不**复用 `introspect::SchemaReport`：那个类型要求 `server_version` /
/// `transaction_read_only` 等留痕字段全在（`doc_merge` 的严格读法就是这样）。
/// 摘要是**只读的展示面**，应当能吃下任何带 `tables[].columns[]` 的 schema 文件
///（包括 `r2rml generate --schema` 能吃的那种最小夹具）。多出来的字段一律忽略。
#[derive(Debug, Clone, Deserialize)]
struct RawColumn {
    #[serde(default)]
    name: String,
    #[serde(default)]
    data_type: String,
    #[serde(default)]
    is_primary_key: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RawTable {
    #[serde(default)]
    name: String,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    columns: Vec<RawColumn>,
    #[serde(default)]
    primary_key: Option<String>,
    #[serde(default)]
    composite_primary_key: Vec<String>,
    #[serde(default)]
    unique_columns: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawReport {
    #[serde(default)]
    tables: Vec<RawTable>,
    #[serde(default)]
    foreign_key_count: usize,
}

/// 一列（展示 + 校验都用它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub is_primary_key: bool,
}

/// 一张表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    /// 表注释 —— 真实库里的「中文名」（如 `云硬盘`）。覆盖不均匀，缺了就是 `None`。
    pub comment: Option<String>,
    pub columns: Vec<Column>,
    /// `schema.json` 若是由 `ontok-builder introspect --out` 产的，这里会是**真实主键**；
    /// 只调 `introspect()` 的产物没有这一栏（`None`）。
    pub primary_key: Option<String>,
    pub composite_primary_key: Vec<String>,
    pub unique_columns: Vec<String>,
}

impl Table {
    pub fn has_column(&self, column: &str) -> bool {
        self.columns.iter().any(|c| c.name == column)
    }

    /// 主键列（单列主键优先；复合主键按原顺序全列）。
    fn key_columns(&self) -> Vec<&str> {
        if let Some(pk) = self.primary_key.as_deref() {
            return vec![pk];
        }
        if !self.composite_primary_key.is_empty() {
            return self.composite_primary_key.iter().map(String::as_str).collect();
        }
        self.columns
            .iter()
            .filter(|c| c.is_primary_key)
            .map(|c| c.name.as_str())
            .collect()
    }
}

/// `schema.json` 的只读视图（**摘要**与**校验**共用这一份，不建第二份真相）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaView {
    tables: BTreeMap<String, Table>,
    foreign_key_count: usize,
}

impl SchemaView {
    /// 从 JSON 文本建视图。⛔ 不猜：`tables` 缺失 ⇒ 空视图（调用方会报「一列都没读到」）。
    pub fn from_json(text: &str) -> Result<Self> {
        let raw: RawReport =
            serde_json::from_str(text).context("schema 不是合法 JSON（要 tables[].columns[]）")?;
        let mut tables = BTreeMap::new();
        for t in raw.tables {
            let name = t.name.trim().to_string();
            if name.is_empty() {
                continue;
            }
            let columns = t
                .columns
                .into_iter()
                .filter(|c| !c.name.trim().is_empty())
                .map(|c| Column {
                    name: c.name.trim().to_string(),
                    data_type: c.data_type.trim().to_string(),
                    is_primary_key: c.is_primary_key,
                })
                .collect();
            tables.insert(
                name.clone(),
                Table {
                    name,
                    comment: t.comment.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()),
                    columns,
                    primary_key: t.primary_key.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()),
                    composite_primary_key: t
                        .composite_primary_key
                        .into_iter()
                        .map(|c| c.trim().to_string())
                        .filter(|c| !c.is_empty())
                        .collect(),
                    unique_columns: t
                        .unique_columns
                        .into_iter()
                        .map(|c| c.trim().to_string())
                        .filter(|c| !c.is_empty())
                        .collect(),
                },
            );
        }
        Ok(Self { tables, foreign_key_count: raw.foreign_key_count })
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| {
            format!(
                "读不到 schema 文件 {}（先用 `ontok-builder introspect --out` 产出一份）",
                path.display()
            )
        })?;
        Self::from_json(&text)
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    pub fn column_count(&self) -> usize {
        self.tables.values().map(|t| t.columns.len()).sum()
    }

    pub fn foreign_key_count(&self) -> usize {
        self.foreign_key_count
    }

    pub fn has_table(&self, table: &str) -> bool {
        self.tables.contains_key(table)
    }

    pub fn has_column(&self, table: &str, column: &str) -> bool {
        self.tables.get(table).is_some_and(|t| t.has_column(column))
    }

    pub fn table_names(&self) -> Vec<&str> {
        self.tables.keys().map(String::as_str).collect()
    }

    // ──────────────────────────────────────────────────────
    // ① 摘要（进 prompt）
    // ──────────────────────────────────────────────────────

    /// 按**文档正文**挑出相关的 top-N 张表，渲染成一段可直接拼进 system prompt 的摘要。
    ///
    /// `corpus` = 本轮所有文档的正文（`\n` 拼接即可）。选表规则见 [`TableScore`]。
    pub fn digest(&self, corpus: &str, max_tables: usize) -> Digest {
        let max = max_tables.max(1);
        let hay = corpus.to_lowercase();
        let mut scored: Vec<(usize, &Table)> = self
            .tables
            .values()
            .map(|t| (t.score(&hay), t))
            .collect();
        // **稳定**：分数高的在前，同分按表名（BTreeMap 已有序，再显式排一次免得依赖排序稳定性）
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let matched = scored.iter().filter(|(s, _)| *s > 0).count();
        let mut picked: Vec<&Table> = scored
            .iter()
            .filter(|(s, _)| *s > 0)
            .take(max)
            .map(|(_, t)| *t)
            .collect();

        // 兜底闸：字符上限（表多 / 表宽都可能顶到它）
        let mut text = self.render(&picked, matched, max);
        while text.chars().count() > MAX_DIGEST_CHARS && picked.len() > 1 {
            picked.pop();
            text = self.render(&picked, matched, max);
        }
        Digest {
            text,
            included: picked.len(),
            total: self.tables.len(),
            matched,
            max_tables: max,
        }
    }

    fn render(&self, picked: &[&Table], matched: usize, max: usize) -> String {
        let mut out = String::new();
        out.push_str("## 源库结构（只读勘查产物 `schema.json` 的摘要）\n");
        out.push_str(&format!(
            "库里共 {} 张表 · {} 列 · 真实外键 {} 条；本次随块下发 **{}** 张（按文档里的概念名匹配，\
             上限 {} 张；命中候选 {} 张）。\n",
            self.tables.len(),
            self.column_count(),
            self.foreign_key_count,
            picked.len(),
            max,
            matched,
        ));
        if picked.is_empty() {
            out.push_str(
                "⚠ 没有一张表与本文档的概念名对得上 —— **本轮不要给 `maps_to`**\
                 （⛔ 不许编列名、⛔ 不许按名字猜表）。\n",
            );
            return out;
        }
        for t in picked {
            let cn = t.comment.as_deref().unwrap_or("无中文名");
            let keys = t.key_columns();
            out.push_str(&format!(
                "\n表 `{}`（{cn}）{}：\n  ",
                t.name,
                if keys.is_empty() {
                    "主键：无（没有单列主键）".to_string()
                } else {
                    format!("主键：{}", keys.join(" + "))
                }
            ));
            let cols: Vec<String> = t
                .columns
                .iter()
                .map(|c| {
                    let pk = if c.is_primary_key || keys.iter().any(|k| *k == c.name) {
                        " PK"
                    } else {
                        ""
                    };
                    format!("{} {}{}", c.name, c.data_type, pk)
                })
                .collect();
            out.push_str(&cols.join(" · "));
            out.push('\n');
            let cand = self.naming_candidates(t);
            if !cand.is_empty() {
                out.push_str(&format!(
                    "  既有关系（按 `*_id` 命名约定推出，**未实测**）：{}\n",
                    cand.join(" · ")
                ));
            }
        }
        out.push_str(
            "\n## 给了表列时的硬约束（违反的条目会被**丢弃并计数**）\n\
             - 概念名优先沿用上表的中文名 / 表名：语义相同就**照抄**那个说法（⛔ 不要另造同义词）。\n\
             - 关系若由某一列实现，**必须**给 `maps_to`：\n",
        );
        out.push_str("  `\"maps_to\": {\"child_column\": \"<子表的列>\", \"parent_column\": \"<父表的列>\"}`\n");
        out.push_str(
            "  两端列都必须在上面的表里出现过；两端分属不同表而列名可能撞车时，\
             再加 `\"child_table\"` / `\"parent_table\"` 消歧。\n\
             - 列不存在 / 对不上任何表：⛔ **不要给** `maps_to`（不许编列名、不许按名字猜表）。\n\
             - 概念断言**不写** `maps_to`。\n",
        );
        out
    }

    /// 按命名约定（`*_id` 词干 ↔ 表名）推出来的**候选**关系（未实测，只作提示）。
    ///
    /// 复用 [`super::fk::fk_stem`] / [`super::fk::match_column_to_table`]
    /// —— ⛔ 不写第二份「`product_id` 指的是哪张表」。
    fn naming_candidates(&self, t: &Table) -> Vec<String> {
        let mut out = Vec::new();
        for c in &t.columns {
            let Some(stem) = super::fk::fk_stem(&c.name) else { continue };
            let hits: Vec<&String> = self
                .tables
                .keys()
                .filter(|n| n.as_str() != t.name.as_str())
                .filter(|n| super::fk::match_column_to_table(stem, n.as_str()).is_some())
                .collect();
            if hits.len() == 1 {
                out.push(format!("`{}` → `{}`", c.name, hits[0]));
                if out.len() >= 5 {
                    break;
                }
            }
        }
        out
    }

    // ──────────────────────────────────────────────────────
    // ② `maps_to` 的落地 + 校验
    // ──────────────────────────────────────────────────────

    /// 把模型给的关系 `maps_to` 落到具体表并逐项校验。
    ///
    /// 认两种形状（**都允许**，免得 skill 侧换个写法就要改代码）：
    ///
    /// ```json
    /// {"child_column":"instance_id","parent_column":"id"}
    /// {"child":{"table":"cloud_disk","column":"instance_id"},"parent":{...}}
    /// ```
    ///
    /// 表怎么定（顺序固定，**不猜**）：
    ///
    /// | # | 规则 | 落空时 |
    /// | --- | --- | --- |
    /// | 1 | 显式给了 `child_table` / `child.table` | 不在 schema 里 ⇒ [`MapsToReject::TableUnknown`] |
    /// | 2 | 概念名（subject / object）与表名或**中文名**唯一对上 | 对上多张 ⇒ [`MapsToReject::TableAmbiguous`] |
    /// | 3 | 这一列只在一张表里出现 | 一张都没有 ⇒ `TableUnknown`；多张 ⇒ `TableAmbiguous` |
    /// | 4 | 表定了之后校验列存在 | 不在 ⇒ [`MapsToReject::ColumnAbsent`] |
    pub fn resolve_relation_maps_to(
        &self,
        subject: &str,
        object: &str,
        raw: &serde_json::Value,
    ) -> std::result::Result<MapsTo, MapsToProblem> {
        let obj = raw.as_object().ok_or_else(|| {
            MapsToReject::BadShape.problem(format!(
                "maps_to 必须是对象（要 {{\"child_column\":\"…\",\"parent_column\":\"…\"}}），实际是 {}",
                json_kind(raw)
            ))
        })?;
        if obj.get("table").is_some_and(|v| !v.is_null()) {
            return Err(MapsToReject::BadShape
                .problem("关系断言上的 maps_to 只认 child_column/parent_column，不认 table"));
        }
        let child = Side::parse(obj, "child")?;
        let parent = Side::parse(obj, "parent")?;
        let child_ep = self.resolve_side("child", &child, subject)?;
        let parent_ep = self.resolve_side("parent", &parent, object)?;
        Ok(MapsTo { table: None, child: Some(child_ep), parent: Some(parent_ep) })
    }

    fn resolve_side(
        &self,
        role: &str,
        side: &Side,
        concept: &str,
    ) -> std::result::Result<Endpoint, MapsToProblem> {
        let column = side.column.clone();
        let table = match side.table.as_deref() {
            Some(t) => {
                if !self.has_table(t) {
                    return Err(MapsToReject::TableUnknown.problem(format!(
                        "maps_to.{role}.table = {t:?} 不在 schema.json 里"
                    )));
                }
                t.to_string()
            }
            None => self.pin_table(role, &column, concept)?,
        };
        if !self.has_column(&table, &column) {
            return Err(MapsToReject::ColumnAbsent.problem(format!(
                "maps_to.{role} = {table}.{column} —— schema.json 里没有这一列"
            )));
        }
        Ok(Endpoint { table, column })
    }

    /// 表没显式给：先用概念名对表（名字 / 中文名），再用「这一列只在一张表里」兜底。
    fn pin_table(
        &self,
        role: &str,
        column: &str,
        concept: &str,
    ) -> std::result::Result<String, MapsToProblem> {
        let by_concept = self.tables_for_concept(concept);
        match by_concept.len() {
            1 => return Ok(by_concept[0].clone()),
            0 => {}
            _ => {
                return Err(MapsToReject::TableAmbiguous.problem(format!(
                    "maps_to.{role}：概念「{concept}」对上多张表（{}）—— 请用 \
                     `{role}_table` 点名",
                    by_concept.join(" / ")
                )))
            }
        }
        let with_col: Vec<&String> = self
            .tables
            .iter()
            .filter(|(_, t)| t.has_column(column))
            .map(|(n, _)| n)
            .collect();
        match with_col.len() {
            1 => Ok(with_col[0].clone()),
            0 => Err(MapsToReject::TableUnknown.problem(format!(
                "maps_to.{role}：概念「{concept}」对不上任何表，列 `{column}` 也不在任何一张表里"
            ))),
            _ => {
                let mut names: Vec<&str> = with_col.iter().map(|s| s.as_str()).collect();
                names.sort_unstable();
                Err(MapsToReject::TableAmbiguous.problem(format!(
                    "maps_to.{role}：列 `{column}` 出现在多张表里（{}）—— 请用 `{role}_table` 点名",
                    names.join(" / ")
                )))
            }
        }
    }

    /// 概念名 ↔ 表的**候选**（表名全等 / 中文名相等或互相包含）。
    ///
    /// ⛔ 不做编辑距离 / 词干 / 同音：那会把「唯一确定」变成「最像的那张」，
    /// 而「像」正是 `doc_merge` 规则 3 禁止的猜表。
    fn tables_for_concept(&self, concept: &str) -> Vec<String> {
        let c = concept.trim().to_lowercase();
        if c.is_empty() {
            return Vec::new();
        }
        self.tables
            .values()
            .filter(|t| {
                if t.name.to_lowercase() == c {
                    return true;
                }
                let Some(cn) = t.comment.as_deref() else { return false };
                let cn = cn.trim().to_lowercase();
                if cn.is_empty() {
                    return false;
                }
                if cn == c {
                    return true;
                }
                // 双向包含只对**有区分度的**中文名生效（≥2 个字，避免「表」这种单字到处命中）
                let c_len = c.chars().count();
                let cn_len = cn.chars().count();
                (c_len >= 2 && cn.contains(&c)) || (cn_len >= 2 && c.contains(&cn))
            })
            .map(|t| t.name.clone())
            .collect()
    }
}

fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// `maps_to` 的一端（模型的写法 → 规范化的两栏）。
#[derive(Debug, Clone, Default)]
struct Side {
    column: String,
    table: Option<String>,
}

impl Side {
    fn parse(
        obj: &serde_json::Map<String, serde_json::Value>,
        role: &str,
    ) -> std::result::Result<Self, MapsToProblem> {
        // 形状 A：`child_column` / `parent_column` + 可选 `child_table` / `parent_table`
        let flat_col = obj
            .get(&format!("{role}_column"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let flat_tab = obj
            .get(&format!("{role}_table"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        // 形状 B：`child: {table, column}`
        let nested = obj.get(role).and_then(|v| v.as_object());
        let nested_col = nested
            .and_then(|o| o.get("column"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let nested_tab = nested
            .and_then(|o| o.get("table"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let column = flat_col.or(nested_col).ok_or_else(|| {
            MapsToReject::BadShape.problem(format!(
                "maps_to 缺少 {role}_column（要 {{\"child_column\":\"…\",\"parent_column\":\"…\"}}）"
            ))
        })?;
        Ok(Self {
            column: column.to_string(),
            table: flat_tab.or(nested_tab).map(str::to_string),
        })
    }
}

/// `maps_to` 被丢掉的原因（**稳定码**，屏幕与 stats.json 都报）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapsToReject {
    /// 形状不对（不是对象 / 缺列 / 关系上给了 `table`）
    BadShape,
    /// 表定不下来（显式给的不在 schema 里，或概念名/列名都对不上）
    TableUnknown,
    /// 表对上了多张（不猜，请人/模型点名）
    TableAmbiguous,
    /// 表定了，但列不在那张表里
    ColumnAbsent,
}

impl MapsToReject {
    pub fn code(self) -> &'static str {
        match self {
            MapsToReject::BadShape => "bad_maps_to",
            MapsToReject::TableUnknown => "maps_to_table_unknown",
            MapsToReject::TableAmbiguous => "maps_to_table_ambiguous",
            MapsToReject::ColumnAbsent => "maps_to_column_absent",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MapsToReject::BadShape => "形状不对",
            MapsToReject::TableUnknown => "查不到表",
            MapsToReject::TableAmbiguous => "对上多张表",
            MapsToReject::ColumnAbsent => "查不到列",
        }
    }

    fn problem(self, detail: impl Into<String>) -> MapsToProblem {
        MapsToProblem { reject: self, detail: detail.into() }
    }
}

/// 一条具体的丢弃理由（码 + 人话）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapsToProblem {
    pub reject: MapsToReject,
    pub detail: String,
}

/// 摘要产物（正文 + 体积口径，屏幕要报「带了多少 / 共多少」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    /// 直接拼进 system prompt 的那段文本。
    pub text: String,
    /// 实际带了几张表。
    pub included: usize,
    /// schema 里共几张表。
    pub total: usize,
    /// 其中「按文档概念名匹配上」的候选有几张（≥ `included` 时说明被表数/字符闸截了）。
    pub matched: usize,
    /// 本次生效的表数上限。
    pub max_tables: usize,
}

impl Digest {
    /// 摘要指纹（进日志 / 屏幕；prompt_sha 由 `Prompts` 自己算，**不**用这个值替代）。
    pub fn sha256(&self) -> String {
        crate::extract::sample::sha256_hex(self.text.as_bytes())
    }
}

/// 一张表与**这篇文档**的相关度（只用于排序，不参与任何校验）。
///
/// | 证据 | 分 |
/// | --- | --- |
/// | 表名（小写）在正文里出现 | 每次 3 分 |
/// | 表名去掉下划线后出现（`cloud_disk` ↔ `clouddisk`） | 每次 2 分 |
/// | 中文名（表注释）出现 | 每次 3 分 |
/// | 表名按 `_` 切出的词（≥4 字符）各自出现 | 每个词 1 分（最多 3 个词） |
///
/// 都是**子串计数**，不做编辑距离：摘要只需要「相关的排前面」，
/// 而「像」到这个位置就够了 —— 最终对齐与否由 `maps_to` 的严格校验说话。
impl Table {
    fn score(&self, hay: &str) -> usize {
        if hay.is_empty() {
            return 0;
        }
        let name = self.name.to_lowercase();
        let mut score = 3 * hay.matches(name.as_str()).count();
        let compact = name.replace('_', "");
        if compact != name && compact.len() >= 4 {
            score += 2 * hay.matches(compact.as_str()).count();
        }
        if let Some(cn) = self.comment.as_deref() {
            let cn = cn.trim().to_lowercase();
            if cn.chars().count() >= 2 {
                score += 3 * hay.matches(cn.as_str()).count();
            }
        }
        let mut words: BTreeSet<&str> = BTreeSet::new();
        for w in name.split('_').filter(|w| w.chars().count() >= 4) {
            words.insert(w);
        }
        for w in words.iter().take(3) {
            if hay.contains(*w) {
                score += 1;
            }
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"{
      "server_version": "PostgreSQL 16",
      "foreign_key_count": 1,
      "tables": [
        {"name":"cloud_disk","comment":"云硬盘","primary_key":"id",
         "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                    {"name":"instance_id","data_type":"text"},
                    {"name":"size_gb","data_type":"integer"}]},
        {"name":"cloud_server","comment":"云主机","primary_key":"id",
         "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                    {"name":"zone","data_type":"text"}]},
        {"name":"audit_log","comment":"审计日志","primary_key":"id",
         "columns":[{"name":"id","data_type":"bigint","is_primary_key":true}]}
      ]}"#;

    fn view() -> SchemaView {
        SchemaView::from_json(SCHEMA).expect("夹具 schema 合法")
    }

    /// 只是省一个 `&`：`resolve_relation_maps_to` 收 `&Value`。
    fn maps(v: serde_json::Value) -> serde_json::Value {
        v
    }

    #[test]
    fn only_tables_whose_names_appear_in_the_document_are_sent() {
        let corpus = "云硬盘挂载到云主机，云硬盘的 instance_id 指向云主机。";
        let d = view().digest(corpus, 10);
        assert_eq!(d.total, 3, "库里共 3 张表");
        assert_eq!(d.included, 2, "只有云硬盘 / 云主机相关：{}", d.text);
        assert_eq!(d.matched, 2);
        assert!(d.text.contains("cloud_disk"), "{}", d.text);
        assert!(d.text.contains("云主机"), "中文名要进摘要：{}", d.text);
        assert!(
            !d.text.contains("audit_log"),
            "无关的表不许塞进来（这就是大小闸）：{}",
            d.text
        );
        // 列类型也要在（「列名 + 类型」是契约要求的一项）
        assert!(d.text.contains("instance_id text"), "{}", d.text);
        // 既有关系：instance_id 的词干是 instance —— 对不上任何表 ⇒ 不该瞎报
        assert!(!d.text.contains("既有关系"), "{}", d.text);
    }

    #[test]
    fn the_size_gate_takes_the_top_n_and_never_stuffs_everything_in() {
        // 96 张表、正文提到一张 ⇒ 只带 1 张（⛔ 不拿无关的表凑数）
        let mut tables = Vec::new();
        for i in 0..96 {
            tables.push(format!(
                r#"{{"name":"t{i:03}","comment":"表{i}","columns":[{{"name":"id","data_type":"text"}}]}}"#
            ));
        }
        let schema = format!(r#"{{"tables":[{}]}}"#, tables.join(","));
        let v = SchemaView::from_json(&schema).expect("合法");
        let d = v.digest("正文里只提到 t000 这一张表", 10);
        assert_eq!(d.total, 96);
        assert_eq!(d.included, 1, "只带对得上的那一张：{}", d.text);
        assert_eq!(d.matched, 1);
        assert!(!d.text.contains("t095"), "⛔ 大库不许全塞");
    }

    #[test]
    fn the_n_table_gate_is_a_hard_cap_and_the_screen_numbers_are_truthful() {
        let mut corpus = String::new();
        let mut tables = Vec::new();
        for i in 0..20 {
            corpus.push_str(&format!("订单表 orders{i} 说……"));
            tables.push(format!(
                r#"{{"name":"orders{i}","comment":"订单{i}","columns":[{{"name":"id","data_type":"text"}}]}}"#
            ));
        }
        let v = SchemaView::from_json(&format!(r#"{{"tables":[{}]}}"#, tables.join(","))).unwrap();
        let d = v.digest(&corpus, 5);
        assert_eq!(d.included, 5, "上限 5");
        assert_eq!(d.matched, 20, "命中候选 20 —— 被上限截到 5");
        assert_eq!(d.max_tables, 5);
        assert_eq!(d.text.matches("表 `orders").count(), 5, "{}", d.text);
    }

    #[test]
    fn a_document_without_any_matching_table_gets_zero_tables_and_a_warning() {
        let d = view().digest("这段文档讲的是采购流程与审批人。", 40);
        assert_eq!(d.included, 0);
        assert!(d.text.contains("本轮不要给 `maps_to`"), "{}", d.text);
        assert!(!d.text.contains("cloud_disk"), "{}", d.text);
    }

    #[test]
    fn maps_to_from_the_brief_shape_resolves_through_the_concept_name() {
        let v = view();
        let m = v
            .resolve_relation_maps_to(
                "云硬盘",
                "云主机",
                &maps(serde_json::json!({"child_column":"instance_id","parent_column":"id"})),
            )
            .expect("概念名对得上表、列也在");
        let child = m.child.expect("child");
        let parent = m.parent.expect("parent");
        assert_eq!((child.table.as_str(), child.column.as_str()), ("cloud_disk", "instance_id"));
        assert_eq!((parent.table.as_str(), parent.column.as_str()), ("cloud_server", "id"));
        // 关系上的 maps_to 不带 `table`
        assert!(m.table.is_none());
    }

    #[test]
    fn an_unknown_column_is_rejected_instead_of_being_guessed() {
        let v = view();
        let p = v
            .resolve_relation_maps_to(
                "云硬盘",
                "云主机",
                &maps(serde_json::json!({"child_column":"instance_id","parent_column":"no_such_col"})),
            )
            .expect_err("列不存在必须丢");
        assert_eq!(p.reject, MapsToReject::ColumnAbsent);
        assert!(p.detail.contains("no_such_col"), "{}", p.detail);
    }

    #[test]
    fn an_ambiguous_column_is_rejected_and_asks_for_the_table() {
        let v = SchemaView::from_json(
            r#"{"tables":[
              {"name":"a","columns":[{"name":"id","data_type":"text"},{"name":"ref","data_type":"text"}]},
              {"name":"b","columns":[{"name":"id","data_type":"text"},{"name":"ref","data_type":"text"}]}]}"#,
        )
        .unwrap();
        let p = v
            .resolve_relation_maps_to(
                "甲",
                "乙",
                &maps(serde_json::json!({"child_column":"ref","parent_column":"id"})),
            )
            .expect_err("两张表都有 ref / id，不能猜");
        assert_eq!(p.reject, MapsToReject::TableAmbiguous);
        // 给了表就收
        let m = v
            .resolve_relation_maps_to(
                "甲",
                "乙",
                &maps(serde_json::json!({"child_column":"ref","child_table":"a",
                                        "parent_column":"id","parent_table":"b"})),
            )
            .expect("点名之后就能落");
        assert_eq!(m.child.unwrap().table, "a");
        assert_eq!(m.parent.unwrap().table, "b");
    }

    #[test]
    fn the_nested_shape_is_accepted_too() {
        let v = view();
        let m = v
            .resolve_relation_maps_to(
                "云硬盘",
                "云主机",
                &maps(serde_json::json!({"child":{"table":"cloud_disk","column":"instance_id"},
                                        "parent":{"table":"cloud_server","column":"id"}})),
            )
            .expect("嵌套形状也要收");
        assert_eq!(m.child.unwrap().table, "cloud_disk");
    }

    #[test]
    fn a_maps_to_shape_error_is_reported_as_bad_shape() {
        let v = view();
        for bad in [
            serde_json::json!("cloud_disk.instance_id"),
            serde_json::json!({"child_column":"instance_id"}),
            serde_json::json!({"table":"cloud_disk","child_column":"x","parent_column":"y"}),
        ] {
            let p = v
                .resolve_relation_maps_to("云硬盘", "云主机", &bad)
                .expect_err("形状不对必须丢");
            assert_eq!(p.reject, MapsToReject::BadShape, "{bad}");
        }
    }

    #[test]
    fn naming_convention_candidates_are_labelled_as_unverified() {
        let v = SchemaView::from_json(
            r#"{"foreign_key_count":0,"tables":[
              {"name":"servers","comment":"服务器","columns":[{"name":"id","data_type":"text"}]},
              {"name":"disks","comment":"磁盘","columns":[
                 {"name":"id","data_type":"text"},
                 {"name":"server_id","data_type":"text"}]}]}"#,
        )
        .unwrap();
        let d = v.digest("磁盘挂在服务器上", 10);
        assert!(d.text.contains("server_id"), "{}", d.text);
        assert!(d.text.contains("未实测"), "候选关系必须标明没实测：{}", d.text);
    }

    #[test]
    fn the_view_reads_the_product_shape_ontok_introspect_writes() {
        // 真实 schema.json 带键字段（REQ-108 注入）与留痕字段，摘要读取必须容忍
        let v = view();
        assert_eq!(v.table_count(), 3);
        assert_eq!(v.column_count(), 6);
        assert_eq!(v.foreign_key_count(), 1);
        assert!(v.has_table("cloud_disk"));
        assert!(v.has_column("cloud_disk", "instance_id"));
        assert!(!v.has_column("cloud_disk", "zone"));
        assert!(v.table_names().contains(&"cloud_server"));
    }
}
