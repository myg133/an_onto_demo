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
//! 本模块做三件事，都在**抽取侧**（`src/extract/**`，⛔ 不碰 `src/db/**`）：
//!
//! | # | 做什么 | 谁用 |
//! | --- | --- | --- |
//! | 1 | 把 schema 压成一段**有体积上限**的摘要（表名 / 中文名 / 列名 / 类型 / 既有关系） | `doc ingest` 的 prompt（与 skill 同一条 system 通道） |
//! | 2 | 把模型给的**关系** `maps_to`（`{child_column,parent_column}`）落地并逐列校验 | `doc_ingest::validate`（查不到 ⇒ 丢弃 + 计数） |
//! | 3 | ⭐ REQ-171 · 把模型给的**概念** `maps_to`（`{table,column?}`）落表并校验 | 同上（四类丢弃码，与关系**分开**计数） |
//!
//! # ⭐ REQ-171：两种 `maps_to` 是**两件事**
//!
//! | 标什么 | 形状 | 谁给 |
//! | --- | --- | --- |
//! | **概念 → 表**（这条概念落在哪张表） | `{"table":"…","column":"…"(可省)}` | 每个**概念**断言 |
//! | **关系 → 两列**（这条边由哪两列实现） | `{"child_column":"…","parent_column":"…"}` | 只有真 join 的**关系**断言 |
//!
//! 文档里的关系大多是**抽象概念之间**（「旗舰模型 belongs_to 平台」），没有 join 列；
//! 而概念本身往往能落到一张表上。REQ-170 只开了第二条，于是 `doc_merge` 在等的
//! 「概念 → 表」（`doc_merge.rs` 的 `resolve_concept`）一直没人填 —— 概念只能靠名字/别名
//! 精确命中，对齐率上不去。本模块的 [`SchemaView::resolve_concept_maps_to`] 就是那条通道
//! 的校验面。
//!
//! # 四条硬纪律
//!
//! 1. **小库给全表，大库才挑**（REQ-171 · R4）。表数 ≤ [`DEFAULT_FULL_LIST_MAX`]
//!    （`--schema-full-list-max` 可覆盖）⇒ **直接给全表清单**（compact：表名 + 主键 +
//!    一行注释），⛔ 不做 top-N —— 真实中文库上打分几乎全 0，top-N 会退化成字典序
//!    （实测 96 张里命中 1 张，选出来的却是 `_volc_dts_pg_ddl_*` 复制产物）。
//!    大库仍走打分，但打分**主要看列注释**（中文是唯一的桥），表名只作次要信号。
//! 2. **体积上限仍然在**（[`MAX_DIGEST_CHARS`]）。全表清单只给 compact；列明细只给
//!    「本文档提到过的表」—— 92 张表连列带注释是 4.8 万字符，塞进每块 prompt 的后果
//!    不是「贵一点」，而是**每块都超上下文**、抽取整体失败。
//! 3. **垃圾表不进清单**（[`JUNK_TABLE_PATTERNS`]）：`_*` 前缀、`alembic_version`、
//!    复制 / DTS / 迁移类产物。它们不是业务表，进了清单只会把模型引到错表上。
//!    排除规则**可配**（`--schema-exclude-tables` / `--schema-keep-junk`）且**报数**。
//! 4. **表名 / 列名不猜**。对不上/对上多张 ⇒ 丢弃并计数（[`MapsToProblem`]），
//!    ⛔ 不选一个「最像的」。
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
///
/// ⚠️ REQ-171 之后它**只管大库**：表数 ≤ [`DEFAULT_FULL_LIST_MAX`] 时走全表清单，
/// 这个上限只用来决定「哪些表额外带列明细」（见 [`SchemaView::digest`]）。
pub const DEFAULT_MAX_TABLES: usize = 40;

/// ⭐ REQ-171 · **小库阈值**：表数 ≤ 这个数就**直接给全表清单**（⛔ 不做 top-N）。
///
/// 200 的口径（R4）：真实 UAT 库 96 张表、compact 全表清单（表名 + 主键 + 一行注释）
/// 只要 ≈3.5 KB，远低于摘要预算 12 000；而打分在这个中文库上 **96 张里只有 1 张 > 0**
/// （表注释只有 25/96 非空）—— 于是 top-N 退化成「按表名字典序取前 12」，
/// 选出来的还是 `_volc_dts_pg_ddl_*` 这种复制产物。
///
/// ⇒ 小库上「挑」这个动作既没必要也有害：**少给一张表，概念就少一条落表的路**。
pub const DEFAULT_FULL_LIST_MAX: usize = 200;

/// 摘要正文的**字符**上限（兜底闸，独立于表数）。
///
/// 表数闸管「几行」，字符闸管「行有多长」—— 一张 200 列的表能把 40 张表的摘要顶到
/// 几十 KB。两道闸都过才进 prompt。
pub const MAX_DIGEST_CHARS: usize = 12_000;

/// ⭐ F-2（QA）· **每张表最多带几列**（列闸，独立于表数闸与字符闸）。
///
/// 为什么需要它：字符闸原来只会「丢表」，而**单张超宽表**丢不掉 ——
/// QA 实测一张 3001 列的表让摘要到了 60 478 字符（上限的 5 倍）。
/// 现在先按列截断（剩余写「另有 M 列未列」），列缩到 1 还超限才允许继续丢表。
///
/// 24 这个数：真实源库最宽的表也就几十列，而模型真正要对齐的是**有注释/有语义**的那几列；
/// 候选外键那条线另外还会点出 `*_id` 列（见 `naming_candidates`）。
pub const MAX_COLUMNS_PER_TABLE: usize = 24;

/// ⭐ REQ-171 · **垃圾表**（不是业务表）的缺省匹配式 —— glob：`*` 任意串，其余按字面。
///
/// | 模式 | 挡的是什么 |
/// | --- | --- |
/// | `_*` | 迁移/复制工具留下的下划线前缀表（真实 UAT 库：`_volc_dts_pg_ddl_*` ×3） |
/// | `alembic_version` | 迁移版本表（真实 UAT 库里就有） |
/// | `*dts_pg_ddl*` | 火山 DTS 的复制产物（DDL 中间表，全是 `ddl`/`txid` 这种列） |
/// | `*_bak` / `*_backup` / `*_old` / `*_copy` / `*_tmp` | 手工备份 / 复制 / 临时表 |
/// | `schema_migrations` / `flyway_schema_history` / `django_migrations` | 别家迁移工具的表 |
///
/// ⚠️ 只用**前后缀/全名**，⛔ 不用「包含」：`contains("temp")` 会把
/// `package_templates` 这种真业务表一起排掉（真实库上试过）。
///
/// 可配：`--schema-exclude-tables` 追加模式；`--schema-keep-junk` 整条关掉。
pub const JUNK_TABLE_PATTERNS: &[&str] = &[
    "_*",
    "alembic_version",
    "schema_migrations",
    "flyway_schema_history",
    "django_migrations",
    "*dts_pg_ddl*",
    "*_bak",
    "*_backup",
    "*_old",
    "*_copy",
    "*_tmp",
];

/// 摘要的**选表策略**（屏幕要报「走的是哪条路」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    /// 表数 ≤ 阈值 ⇒ **全表清单**（compact，不做 top-N）。
    FullList,
    /// 大库 ⇒ 按文档打分挑 top-N（只带命中候选）。
    Scored,
}

impl SelectionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SelectionMode::FullList => "全表",
            SelectionMode::Scored => "按打分",
        }
    }
}

/// `digest` 的入参（表数闸 / 全表阈值 / 垃圾表配置）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestOptions {
    /// 大库 top-N 的上限（也限制「额外带列明细」的表数）。
    pub max_tables: usize,
    /// 表数 ≤ 它 ⇒ 全表清单。`0` = 永不走全表（总是打分，可用来复现旧行为）。
    pub full_list_max: usize,
    /// 追加的垃圾表匹配式（与 [`JUNK_TABLE_PATTERNS`] 同一语法）。
    pub extra_excludes: Vec<String>,
    /// `true` = **不禁**垃圾表（用户的表名真的长成 `_foo` 时用）。
    pub keep_junk: bool,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            max_tables: DEFAULT_MAX_TABLES,
            full_list_max: DEFAULT_FULL_LIST_MAX,
            extra_excludes: Vec::new(),
            keep_junk: false,
        }
    }
}

impl DigestOptions {
    /// 只覆盖 top-N 上限（其余用缺省）。
    pub fn with_max_tables(max_tables: usize) -> Self {
        Self { max_tables, ..Self::default() }
    }
}

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
    /// ⭐ REQ-171 · 列注释（真实 UAT 库：1116 列里 **967 列有中文注释**）。
    ///
    /// 它是「中文概念名 ↔ 物理列」唯一的桥：表注释只有 25/96 非空，列注释却有 87%。
    /// 打分（[`Table::score`]）与摘要正文都靠它。
    #[serde(default)]
    comment: Option<String>,
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
    /// ⭐ REQ-171 · 列注释（中文名，真实库里 87% 的列有）。`None` = 没有注释。
    pub comment: Option<String>,
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
    /// ⭐ F-1（QA）：这份 `schema.json` **到底有没有**键事实。
    ///
    /// `tables[i]` 里出现 `primary_key` / `composite_primary_key` / `unique_columns`
    /// 任一键 ⇒ `true`（那是 `ontok-introspect` 产物的形状）。
    /// 三个键都不在 ⇒ `false` = **不知道**（旧产物 / 别家工具手写的 schema）。
    ///
    /// 为什么要这一位：「这张表没有主键」是**事实**，「我不知道它有没有主键」是**不知道**
    /// —— 把后者渲染成「主键：无」会让模型据此编 `maps_to`（F-1 的事故正是它）。
    pub keys_known: bool,
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
        // 两遍：`Value` 用来判「这个键到底在不在」（`Option` 分不出「null」与「缺键」），
        // 类型化那一遍只取需要的字段。
        let doc: serde_json::Value =
            serde_json::from_str(text).context("schema 不是合法 JSON（要 tables[].columns[]）")?;
        let raw: RawReport = serde_json::from_value(doc.clone())
            .context("schema 不是合法 JSON（要 tables[].columns[]）")?;
        let mut keys_known: std::collections::BTreeSet<String> = Default::default();
        if let Some(arr) = doc.get("tables").and_then(serde_json::Value::as_array) {
            for t in arr {
                let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
                let known = ["primary_key", "composite_primary_key", "unique_columns"]
                    .iter()
                    .any(|k| t.get(k).is_some());
                if known {
                    keys_known.insert(name.trim().to_string());
                }
            }
        }
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
                    comment: c.comment.map(|x| x.trim().to_string()).filter(|x| !x.is_empty()),
                    is_primary_key: c.is_primary_key,
                })
                .collect();
            tables.insert(
                name.clone(),
                Table {
                    keys_known: keys_known.contains(&name),
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

    /// 给**这篇文档**挑表，渲染成一段可直接拼进 system prompt 的摘要。
    ///
    /// `corpus` = 本轮所有文档的正文（`\n` 拼接即可）。两条路（R4）：
    ///
    /// | 情形 | 带什么 |
    /// | --- | --- |
    /// | 垃圾表过滤后表数 ≤ `opts.full_list_max` | **全表清单**（compact：表名 + 主键 + 一行注释，⛔ 不做 top-N），另外给「本文档命中」的 ≤ `opts.max_tables` 张表带**列明细** |
    /// | 更大 | 只带打分 > 0 的 top-N（与 REQ-170 同形） |
    ///
    /// ⭐ REQ-171 的关键差别：**小库不挑表**。真实中文库上打分 96 张里只有 1 张 > 0
    /// （表注释 25/96 非空），于是 top-N 只能拿「分数并列 0」的表凑数 ⇒ 退化成字典序，
    /// 选出来的是 `_volc_dts_pg_ddl_*` 复制产物，真正该看的表反而没进去。
    #[allow(clippy::too_many_lines)]
    pub fn digest(&self, corpus: &str, opts: &DigestOptions) -> Digest {
        let max = opts.max_tables.max(1);
        let hay = corpus.to_lowercase();

        // ① 垃圾表：**先剔掉再数**（「共 96 张」里不该混着 DTS 复制产物）。
        let (kept, excluded): (Vec<&Table>, Vec<&Table>) =
            self.tables.values().partition(|t| !is_junk_table(&t.name, opts));
        let mut excluded_names: Vec<&str> = excluded.iter().map(|t| t.name.as_str()).collect();
        excluded_names.sort_unstable();

        // ② 打分（列注释为主、表名为次）+ **稳定**排序（分高在前，同分按表名）。
        let mut scored: Vec<(usize, &Table)> =
            kept.iter().map(|t| (t.score(&hay), *t)).collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let matched = scored.iter().filter(|(s, _)| *s > 0).count();

        // ③ 走哪条路。
        let mode = if kept.len() <= opts.full_list_max {
            SelectionMode::FullList
        } else {
            SelectionMode::Scored
        };
        let mut list: Vec<&Table> = match mode {
            // 全表：**一张都不丢**（这道闸只管「看不看得见表」，不管「相不相关」）。
            SelectionMode::FullList => scored.iter().map(|(_, t)| *t).collect(),
            // 大库：只带命中候选（⛔ 不拿无关的表凑数）。
            SelectionMode::Scored => scored
                .iter()
                .filter(|(s, _)| *s > 0)
                .take(max)
                .map(|(_, t)| *t)
                .collect(),
        };
        // ④ 列明细（体积大头）：只给「本文档命中」的表，且不超过 max 张。
        //    全表模式下其余表是 compact 一行 —— ⛔「没有列明细」不等于「没有这张表」。
        let mut detail: Vec<&str> = scored
            .iter()
            .filter(|(s, _)| *s > 0)
            .take(max)
            .map(|(_, t)| t.name.as_str())
            .collect();
        if mode == SelectionMode::Scored {
            // 大库那条路本来就只带明细表 —— 与 REQ-170 逐字节同形。
            detail = list.iter().map(|t| t.name.as_str()).collect();
        }

        // ⑤ 兜底闸：字符上限（表多 / 表宽都可能顶到它）。
        //
        // ⚠️ F-2（QA）：只「丢表」对**单张超宽表**不生效 —— 一张 3001 列的表能把
        // 摘要顶到 6 万字符，而 `len() > 1` 的保护让循环一次都不进。
        // 所以先按**列**收缩（每表最多带 `MAX_COLUMNS_PER_TABLE` 列，剩余写「另有 M 列未列」），
        // 再丢**列明细**，最后才允许丢表 —— 而且**只在大库那条路上丢**：
        // 全表清单丢表就等于偷偷退回 top-N（R4 明令禁止）。
        let mut col_cap = MAX_COLUMNS_PER_TABLE;
        let mut text =
            self.render(&list, &detail, matched, max, mode, &excluded_names, col_cap);
        while text.chars().count() > MAX_DIGEST_CHARS {
            if col_cap > 1 {
                col_cap = (col_cap / 2).max(1);
                text = self.render(&list, &detail, matched, max, mode, &excluded_names, col_cap);
                continue;
            }
            if !detail.is_empty() {
                detail.clear();
                text = self.render(&list, &detail, matched, max, mode, &excluded_names, col_cap);
                continue;
            }
            if mode == SelectionMode::Scored && list.len() > 1 {
                list.pop();
                col_cap = MAX_COLUMNS_PER_TABLE;
                text = self.render(&list, &detail, matched, max, mode, &excluded_names, col_cap);
                continue;
            }
            break;
        }
        Digest {
            text,
            included: list.len(),
            detail_tables: detail.len(),
            total: self.tables.len(),
            matched,
            max_tables: max,
            mode,
            excluded: excluded_names.len(),
            excluded_samples: excluded_names.iter().take(3).map(|n| n.to_string()).collect(),
        }
    }

    /// 渲染。`list` = 要出现在清单里的表；`detail` = 其中额外带列明细的表名。
    #[allow(clippy::too_many_arguments)]
    fn render(
        &self,
        list: &[&Table],
        detail: &[&str],
        matched: usize,
        max: usize,
        mode: SelectionMode,
        excluded: &[&str],
        col_cap: usize,
    ) -> String {
        let mut out = String::new();
        out.push_str("## 源库结构（只读勘查产物 `schema.json` 的摘要）\n");
        let how = match mode {
            SelectionMode::FullList => format!(
                "本次随块下发 **全表 {} 张**（表数在阈值内 ⇒ 给全表清单，⛔ 不做 top-N）",
                list.len(),
            ),
            SelectionMode::Scored => format!(
                "本次随块下发 **{}** 张（大库 ⇒ 按文档打分取前 {} 张；命中候选 {} 张）",
                list.len(),
                max,
                matched,
            ),
        };
        out.push_str(&format!(
            "库里共 {} 张表 · {} 列 · 真实外键 {} 条；{how}。\n",
            self.tables.len(),
            self.column_count(),
            self.foreign_key_count,
        ));
        if !excluded.is_empty() {
            // ⚠️ 只报**张数**、⛔ 不点名：名字进 prompt 反而会把模型引到那些表上
            //（AC-5 的判据也是「`_volc_dts_pg_ddl_*` 不出现」）。名字进屏幕/stats。
            out.push_str(&format!(
                "已排除 {} 张非业务表（`_*` 前缀 / alembic_version / DTS 复制 / 迁移类）—— \
                 它们**不在**下面的清单里，⛔ 别往上标。\n",
                excluded.len(),
            ));
        }
        // ⭐ REQ-171 · **两类 `maps_to` 都要说清**（R2：模型必须知道有这个动作）。
        out.push_str(
            "\n**怎么用这份清单 —— 两类 `maps_to` 是两件不同的事，能做的都要做**\n\
             1. **概念 → 表**：逐个概念尽量标注它落在哪张表：\
             `\"maps_to\": {\"table\": \"<上面清单里的表名>\", \"column\": \"<该表的列，可省>\"}`。\
             概念整行对应一张表时**省掉 `column`**。给不出就不写，⛔ 不许猜、⛔ 不许编表名。\n\
             2. **关系 → 两列**：只有当这条关系在库里**确实由一对列实现**（外键式 join）时才给\
             `\"maps_to\": {\"child_column\": \"<子表的列>\", \"parent_column\": \"<父表的列>\"}`；\
             抽象概念之间的关系（没有 join 列）**不要给**关系上的 `maps_to`。\n",
        );
        if list.is_empty() {
            out.push_str(
                "\n⚠ 本次一张表都没带（大库打分命中 0）—— **本轮不要给 `maps_to`**\
                 （⛔ 不许编列名、⛔ 不许按名字猜表）。\n",
            );
            return out;
        }
        let detail_set: BTreeSet<&str> = detail.iter().copied().collect();
        let mut detailed = 0usize;
        for t in list {
            let cn = t.comment.as_deref().unwrap_or("无中文名");
            let keys = t.key_columns();
            // ⚠️ F-1（QA）：「主键：无」**只允许**在「schema 明确说没有键」时出现。
            // 三个键字段都不在的旧产物是**不知道**，不是「没有主键」——
            // 把它渲染成「无」会让模型据此编 `maps_to`（只读账号那次的假信息就是这样进 prompt 的）。
            let key_line = if !keys.is_empty() {
                format!("主键：{}", keys.join(" + "))
            } else if t.keys_known {
                "主键：无（这张表真的没有单列/复合主键）".to_string()
            } else {
                "主键：**未勘明**（这份 schema.json 里没有键事实 —— ⛔ 不要当作「没有主键」）"
                    .to_string()
            };
            if !detail_set.contains(t.name.as_str()) {
                // compact：名 + 主键 + 一行注释（R4 要的全表清单形状）
                out.push_str(&format!("- `{}`（{cn}）{key_line}\n", t.name));
                continue;
            }
            detailed += 1;
            out.push_str(&format!("\n表 `{}`（{cn}）{key_line}：\n  ", t.name));
            let shown = t.columns.len().min(col_cap.max(1));
            let cols: Vec<String> = t
                .columns
                .iter()
                .take(shown)
                .map(|c| {
                    let pk = if c.is_primary_key || keys.iter().any(|k| *k == c.name) {
                        " PK"
                    } else {
                        ""
                    };
                    // ⭐ REQ-171：列注释是「中文概念 ↔ 物理列」唯一的桥（真实库 967/1116 有）。
                    match c.comment.as_deref() {
                        Some(cm) if !cm.is_empty() => {
                            format!("{} {}{}（{}）", c.name, c.data_type, pk, cm)
                        }
                        _ => format!("{} {}{}", c.name, c.data_type, pk),
                    }
                })
                .collect();
            out.push_str(&cols.join(" · "));
            if t.columns.len() > shown {
                out.push_str(&format!(
                    " …（另有 {} 列未列，共 {} 列）",
                    t.columns.len() - shown,
                    t.columns.len()
                ));
            }
            out.push('\n');
            let cand = self.naming_candidates(t);
            if !cand.is_empty() {
                out.push_str(&format!(
                    "  既有关系（按 `*_id` 命名约定推出，**未实测**）：{}\n",
                    cand.join(" · ")
                ));
            }
        }
        if detailed < list.len() {
            out.push_str(&format!(
                "\n上面 {} 张是全表清单（只有表名 / 主键 / 注释）；另外 {} 张带了列明细（本文档提到过）。\
                 给概念标 `column` 时只能从**列明细**里挑，⛔ 不许按英文直译造列名。\n",
                list.len() - detailed,
                detailed,
            ));
        }
        out.push_str(
            "\n## 给了表列时的硬约束（违反的条目会被**丢弃并计数**）\n\
             - 概念名优先沿用上表的中文名 / 表名：语义相同就**照抄**那个说法（⛔ 不要另造同义词）。\n\
             - 概念断言：给 `maps_to.table`（可选 `column`），**逐个概念**标，给不出就不写。\n\
             - 关系断言：**只有真 join** 才给 `maps_to`（`child_column` + `parent_column`，\
             两端分属不同表而列名撞车时再加 `child_table` / `parent_table`）。\n\
             - 表/列对不上清单：⛔ **不要给** `maps_to`（不许编列名、不许按名字猜表）——\
             编一个会让**整条断言**被丢弃。\n",
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

    /// ⭐ REQ-171 · 把模型给的**概念** `maps_to` 落到具体表（可选列）并校验。
    ///
    /// 规范形状（契约 §5.3）：`{"table":"customers","column":"id"}`，`column` 可省。
    ///
    /// 表怎么定（顺序固定，**不猜**）：
    ///
    /// | # | 输入 | 结果 |
    /// | --- | --- | --- |
    /// | 1 | 不是对象 / `table` 不是字符串 / 带 `child`·`parent`·`child_column` | [`MapsToReject::BadShape`] |
    /// | 2 | `table` 是 schema 里的表名（先全等、再一次大小写不敏感） | 用它 |
    /// | 3 | 否则把 `table` 当**概念名**（表名 / 中文名 / 双向包含，≥2 字）唯一对上 | 用它；0 张 ⇒ [`MapsToReject::TableUnknown`]；多张 ⇒ [`MapsToReject::TableAmbiguous`] |
    /// | 4 | 没给 `table`、只给 `column`：概念名唯一对表 ⇒ 用它；否则「这一列只在一张表里」⇒ 用它 | 0 张 ⇒ `TableUnknown`；多张 ⇒ `TableAmbiguous` |
    /// | 5 | 表定了之后校验列（给了才校验） | 不在 ⇒ [`MapsToReject::ColumnAbsent`] |
    ///
    /// ⛔ 给了 `table` 但它在 schema 里查不到、也对不上任何中文名 ⇒ **直接丢**：
    /// 不拿 `column` 去反推一张表（那正是「按名字猜表」，`doc_merge` 规则 3 禁止的行为）。
    pub fn resolve_concept_maps_to(
        &self,
        concept: &str,
        raw: &serde_json::Value,
    ) -> std::result::Result<super::doc_ingest::ConceptMapsTo, MapsToProblem> {
        use super::doc_ingest::ConceptMapsTo;

        let obj = raw.as_object().ok_or_else(|| {
            MapsToReject::BadShape.problem(format!(
                "概念「{concept}」的 maps_to 必须是对象（要 {{\"table\":\"…\"}}，可选 \"column\"），\
                 实际是 {}",
                json_kind(raw)
            ))
        })?;
        // 关系那套键**不许**出现在概念上：两种 maps_to 混写会让下游把表/列读成 join。
        for k in ["child", "parent", "child_column", "parent_column", "child_table", "parent_table"] {
            if obj.get(k).is_some_and(|v| !v.is_null()) {
                return Err(MapsToReject::BadShape.problem(format!(
                    "概念「{concept}」的 maps_to 只认 table / column；`{k}` 属于**关系**的 maps_to\
                     （两件事，别混）"
                )));
            }
        }
        let read = |key: &str| -> std::result::Result<Option<String>, MapsToProblem> {
            match obj.get(key) {
                None | Some(serde_json::Value::Null) => Ok(None),
                Some(serde_json::Value::String(s)) => {
                    let t = s.trim();
                    Ok(if t.is_empty() { None } else { Some(t.to_string()) })
                }
                Some(other) => Err(MapsToReject::BadShape.problem(format!(
                    "概念「{concept}」的 maps_to.{key} 必须是字符串，实际是 {}",
                    json_kind(other)
                ))),
            }
        };
        let table_raw = read("table")?;
        let column_raw = read("column")?;

        let table = match table_raw {
            Some(t) => match self.table_by_name(&t) {
                Some(exact) => exact.to_string(),
                None => {
                    let cands = self.tables_for_concept(&t);
                    match cands.len() {
                        1 => cands[0].clone(),
                        0 => {
                            return Err(MapsToReject::TableUnknown.problem(format!(
                                "概念「{concept}」的 maps_to.table = {t:?} 不在 schema.json 里\
                                 （也不是任何表的中文名）"
                            )))
                        }
                        _ => {
                            return Err(MapsToReject::TableAmbiguous.problem(format!(
                                "概念「{concept}」的 maps_to.table = {t:?} 对上多张表（{}）—— \
                                 请直接写**表名**",
                                cands.join(" / ")
                            )))
                        }
                    }
                }
            },
            None => {
                // 没有 table：先用**这个概念名**对表（最常见：模型只写了 column）。
                let by_concept = self.tables_for_concept(concept);
                match by_concept.len() {
                    1 => by_concept[0].clone(),
                    0 => {
                        let Some(col) = column_raw.as_deref() else {
                            return Err(MapsToReject::BadShape.problem(format!(
                                "概念「{concept}」的 maps_to 缺 table（要 {{\"table\":\"…\"}}）"
                            )));
                        };
                        let by_col = self.tables_with_column(col);
                        match by_col.len() {
                            1 => by_col[0].clone(),
                            0 => {
                                return Err(MapsToReject::TableUnknown.problem(format!(
                                    "概念「{concept}」的 maps_to 没有 table，列 `{col}` \
                                     也不在任何一张表里"
                                )))
                            }
                            _ => {
                                return Err(MapsToReject::TableAmbiguous.problem(format!(
                                    "概念「{concept}」的 maps_to 没有 table，列 `{col}` \
                                     出现在多张表里（{}）",
                                    by_col.join(" / ")
                                )))
                            }
                        }
                    }
                    _ => {
                        return Err(MapsToReject::TableAmbiguous.problem(format!(
                            "概念「{concept}」的 maps_to 没有 table，而概念名对上多张表（{}）",
                            by_concept.join(" / ")
                        )))
                    }
                }
            }
        };

        match column_raw {
            None => Ok(ConceptMapsTo { table, column: None }),
            Some(col) => {
                if !self.has_column(&table, &col) {
                    return Err(MapsToReject::ColumnAbsent.problem(format!(
                        "概念「{concept}」的 maps_to = {table}.{col} —— schema.json 里没有这一列"
                    )));
                }
                Ok(ConceptMapsTo { table, column: Some(col) })
            }
        }
    }

    /// 表名 → schema 里的**规范表名**：先全等，再**一次**大小写不敏感；对不上 ⇒ `None`。
    fn table_by_name(&self, name: &str) -> Option<&str> {
        if let Some((k, _)) = self.tables.get_key_value(name) {
            return Some(k.as_str());
        }
        let lower = name.to_lowercase();
        let hits: Vec<&String> =
            self.tables.keys().filter(|k| k.to_lowercase() == lower).collect();
        if hits.len() == 1 {
            Some(hits[0].as_str())
        } else {
            None
        }
    }

    /// 哪些表里有这一列（排序稳定，供「列唯一 ⇒ 表唯一」的兜底用）。
    fn tables_with_column(&self, column: &str) -> Vec<String> {
        self.tables
            .values()
            .filter(|t| t.has_column(column))
            .map(|t| t.name.clone())
            .collect()
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

/// 摘要产物（正文 + 体积口径，屏幕要报「带了多少 / 共多少 / 走哪条路 / 排除了多少」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digest {
    /// 直接拼进 system prompt 的那段文本。
    pub text: String,
    /// 实际带了几张表（全表模式 = 过滤后全量；打分模式 = top-N）。
    pub included: usize,
    /// 其中带了**列明细**的有几张（其余是 compact 一行）。
    pub detail_tables: usize,
    /// schema 里共几张表（**过滤前**）。
    pub total: usize,
    /// 其中「按文档概念名匹配上」的候选有几张（≥ `included` 时说明被表数/字符闸截了）。
    pub matched: usize,
    /// 本次生效的表数上限。
    pub max_tables: usize,
    /// ⭐ REQ-171 · 走的是哪条路（全表 / 按打分）。
    pub mode: SelectionMode,
    /// ⭐ REQ-171 · 被当作**垃圾表**排掉了几张。
    pub excluded: usize,
    /// 排掉的前三张（屏幕/日志用，避免「排除了 4 张」但不知道是哪 4 张）。
    pub excluded_samples: Vec<String>,
}

impl Digest {
    /// 摘要指纹（进日志 / 屏幕；prompt_sha 由 `Prompts` 自己算，**不**用这个值替代）。
    pub fn sha256(&self) -> String {
        crate::extract::sample::sha256_hex(self.text.as_bytes())
    }

    /// 屏幕那一行的「走的是哪条路」。
    pub fn selection_line(&self) -> String {
        match self.mode {
            SelectionMode::FullList => format!("全表 {} 张", self.included),
            SelectionMode::Scored => format!(
                "按打分带 {} 张（命中候选 {} · 上限 {}）",
                self.included, self.matched, self.max_tables
            ),
        }
    }
}

/// 一张表与**这篇文档**的相关度（只用于排序，不参与任何校验）。
///
/// | 证据 | 分 |
/// | --- | --- |
/// | **列注释**（中文）在正文里出现 | 每次 **3** 分（⭐ REQ-171：中文是唯一的桥） |
/// | 中文名（表注释）出现 | 每次 3 分 |
/// | 表名（小写）在正文里出现 | 每次 **1** 分（R4：表名次要） |
/// | 表名去掉下划线后出现（`cloud_disk` ↔ `clouddisk`） | 每次 1 分 |
/// | 表名按 `_` 切出的词（≥4 字符）各自出现 | 每个词 1 分（最多 3 个词） |
/// | 列名（≥4 字符）在正文里原样出现 | 每次 1 分（最多 5 个列） |
///
/// 都是**子串计数**，不做编辑距离：摘要只需要「相关的排前面」，
/// 而「像」到这个位置就够了 —— 最终对齐与否由 `maps_to` 的严格校验说话。
///
/// ⭐ REQ-171 的两处改动：列注释进打分（真实 UAT 库 967/1116 列有中文注释，而表注释
/// 只有 25/96 非空 —— 拿表名/表注释当主信号，96 张里只有 1 张能 > 0 分）；表名的权重
/// 从 3 降到 1（R4 的「表名次要」）。它只影响**大库那条路**与「哪几张带列明细」，
/// 不影响任何校验。
impl Table {
    fn score(&self, hay: &str) -> usize {
        if hay.is_empty() {
            return 0;
        }
        let name = self.name.to_lowercase();
        let mut score = hay.matches(name.as_str()).count();
        let compact = name.replace('_', "");
        if compact != name && compact.len() >= 4 {
            score += hay.matches(compact.as_str()).count();
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
        // ⭐ 列注释（中文，主要信号）：只数**有区分度**的（≥2 字），每条命中 3 分。
        for c in &self.columns {
            if let Some(cm) = c.comment.as_deref() {
                let cm = cm.trim().to_lowercase();
                if cm.chars().count() >= 2 && hay.contains(cm.as_str()) {
                    score += 3;
                }
            }
        }
        // 列名（英文）是很弱的信号：命中也只是「这张表可能相关」，1 分封顶，最多 5 列。
        for c in self.columns.iter().filter(|c| c.name.chars().count() >= 4).take(5) {
            let n = c.name.to_lowercase();
            if hay.contains(n.as_str()) {
                score += 1;
            }
        }
        score
    }
}

// ──────────────────────────────────────────────────────────
// 垃圾表（⭐ REQ-171 · R4）
// ──────────────────────────────────────────────────────────

/// 表名是否属于**非业务表**（迁移 / 复制 / 备份产物）。
///
/// 匹配式见 [`JUNK_TABLE_PATTERNS`]：`*` 是任意串，其余按字面；**大小写不敏感**。
/// `opts.keep_junk` 打开 ⇒ 一律 `false`（用户说「我的表就叫 `_foo`」时别替他做主）。
pub fn is_junk_table(name: &str, opts: &DigestOptions) -> bool {
    if opts.keep_junk {
        return false;
    }
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return false;
    }
    JUNK_TABLE_PATTERNS
        .iter()
        .copied()
        .chain(opts.extra_excludes.iter().map(String::as_str))
        .any(|p| glob_match_ci(p, &n))
}

/// 极小的 glob：只支持 `*`（任意串，含空），其余字符按字面、大小写不敏感。
///
/// 刻意不引第三方 crate、也不做正则：模式集合是**固定的十来条**，一段 30 行的
/// 匹配器比一个依赖更容易审 —— 而且它必须**可预测**（垃圾表判错的代价是把业务表藏起来）。
fn glob_match_ci(pattern: &str, text: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    if p.is_empty() {
        return false;
    }
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == text;
    }
    // 第一段必须在开头，最后一段必须在结尾，中间段按顺序出现（`*` 可匹配空串）。
    let head = parts[0];
    if !text.starts_with(head) {
        return false;
    }
    let mut rest = &text[head.len()..];
    let tail = parts[parts.len() - 1];
    for (i, mid) in parts[1..parts.len() - 1].iter().enumerate() {
        let _ = i;
        if mid.is_empty() {
            continue;
        }
        match rest.find(mid) {
            Some(pos) => rest = &rest[pos + mid.len()..],
            None => return false,
        }
    }
    if tail.is_empty() {
        return true;
    }
    // 最后一段：必须落在**剩下部分的结尾**（`*dts_pg_ddl*` 的 tail 是空 ⇒ 上面已返回）。
    rest.len() >= tail.len() && rest.ends_with(tail)
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
        let d = view().digest(corpus, &DigestOptions::with_max_tables(10));
        assert_eq!(d.total, 3, "库里共 3 张表");
        assert_eq!(d.mode, SelectionMode::FullList, "3 张表 ⇒ 走全表清单");
        assert_eq!(d.included, 3, "全表模式**一张都不丢**：{}", d.text);
        assert_eq!(d.matched, 2, "命中候选只有云硬盘 / 云主机");
        assert_eq!(d.detail_tables, 2, "只有命中候选带列明细");
        assert!(d.text.contains("cloud_disk"), "{}", d.text);
        assert!(d.text.contains("云主机"), "中文名要进摘要：{}", d.text);
        // ⭐ REQ-171 的差别：无关的表**也在清单里**（compact 一行），但**不带列明细**。
        assert!(
            d.text.contains("- `audit_log`"),
            "全表清单要含无关表（模型否则永远看不到它）：{}",
            d.text
        );
        assert!(
            !d.text.contains("表 `audit_log`"),
            "无关表只给 compact 一行，⛔ 不带列明细：{}",
            d.text
        );
        // 列类型也要在（「列名 + 类型」是契约要求的一项）
        assert!(d.text.contains("instance_id text"), "{}", d.text);
        // 既有关系：instance_id 的词干是 instance —— 对不上任何表 ⇒ 不该瞎报
        assert!(!d.text.contains("既有关系"), "{}", d.text);
    }

    #[test]
    fn the_size_gate_takes_the_top_n_and_never_stuffs_everything_in() {
        // 96 张表、`--schema-full-list-max 0`（= 永不走全表）⇒ 只带 1 张（⛔ 不拿无关的表凑数）
        let mut tables = Vec::new();
        for i in 0..96 {
            tables.push(format!(
                r#"{{"name":"t{i:03}","comment":"表{i}","columns":[{{"name":"id","data_type":"text"}}]}}"#
            ));
        }
        let schema = format!(r#"{{"tables":[{}]}}"#, tables.join(","));
        let v = SchemaView::from_json(&schema).expect("合法");
        let opts = DigestOptions { max_tables: 10, full_list_max: 0, ..DigestOptions::default() };
        let d = v.digest("正文里只提到 t000 这一张表", &opts);
        assert_eq!(d.total, 96);
        assert_eq!(d.mode, SelectionMode::Scored, "关了全表 ⇒ 大库按打分");
        assert_eq!(d.included, 1, "只带对得上的那一张：{}", d.text);
        assert_eq!(d.matched, 1);
        assert!(!d.text.contains("t095"), "⛔ 大库不许全塞");
    }

    /// ⭐ REQ-171 · R4 的核心判据：**小库不许做 top-N**（96 张 ⇒ 全表，且垃圾表不在里面）。
    #[test]
    fn a_small_database_gets_the_full_table_list_without_top_n() {
        let mut tables = Vec::new();
        for i in 0..96 {
            tables.push(format!(
                r#"{{"name":"t{i:03}","comment":"表{i}","columns":[{{"name":"id","data_type":"text"}}]}}"#
            ));
        }
        // 真实 UAT 库的那四张垃圾表：3 张 DTS 复制产物 + alembic_version
        for n in ["_volc_dts_pg_ddl_aa", "_volc_dts_pg_ddl_bb", "_volc_dts_pg_ddl_cc", "alembic_version"]
        {
            tables.push(format!(
                r#"{{"name":"{n}","columns":[{{"name":"id","data_type":"bigint"}}]}}"#
            ));
        }
        let schema = format!(r#"{{"tables":[{}]}}"#, tables.join(","));
        let v = SchemaView::from_json(&schema).expect("合法");
        // 正文只提到一张表 —— 但全表模式**不看这个**
        let d = v.digest("正文里只提到 t000 这一张表", &DigestOptions::default());
        assert_eq!(d.total, 100, "schema 里共 100 张");
        assert_eq!(d.mode, SelectionMode::FullList);
        assert_eq!(d.excluded, 4, "四张垃圾表要排掉：{:?}", d.excluded_samples);
        assert_eq!(d.included, 96, "全表清单 96 张（⛔ 不做 top-N）：{}", d.text);
        for n in ["_volc_dts_pg_ddl_aa", "alembic_version"] {
            assert!(
                !d.text.contains(&format!("- `{n}`")),
                "⛔ 垃圾表不许出现在清单里：{n}\n{}",
                d.text
            );
        }
        assert!(d.text.contains("全表 96 张"), "屏幕/正文都要说「全表」：{}", d.text);
        assert!(d.text.chars().count() <= MAX_DIGEST_CHARS, "字符闸仍然生效");
    }

    /// ⭐ REQ-171 · 垃圾表规则**可配**：`--schema-exclude-tables` 追加、`--schema-keep-junk` 关掉。
    #[test]
    fn junk_table_exclusion_is_configurable() {
        let schema = r#"{"tables":[
          {"name":"_dts_copy","columns":[{"name":"id","data_type":"text"}]},
          {"name":"alembic_version","columns":[{"name":"v","data_type":"text"}]},
          {"name":"orders","columns":[{"name":"id","data_type":"text"}]}]}"#;
        let v = SchemaView::from_json(schema).unwrap();
        let d = v.digest("订单 orders", &DigestOptions::default());
        assert_eq!(d.excluded, 2, "内置清单排掉两张");
        assert_eq!(d.included, 1);

        // ① 关掉内置清单 ⇒ 一张都不排
        let keep = DigestOptions { keep_junk: true, ..DigestOptions::default() };
        let d = v.digest("订单 orders", &keep);
        assert_eq!(d.excluded, 0, "keep_junk ⇒ 不禁：{:?}", d.excluded_samples);
        assert_eq!(d.included, 3);

        // ② 追加模式 ⇒ 连真业务表也能按用户的话排掉
        let extra = DigestOptions {
            extra_excludes: vec!["orders".to_string()],
            ..DigestOptions::default()
        };
        let d = v.digest("订单 orders", &extra);
        assert_eq!(d.excluded, 3, "追加了 orders：{:?}", d.excluded_samples);
        assert_eq!(d.included, 0);

        // ③ 内置模式是 glob（`*_tmp` 这类后缀也要挡住，但不能误伤 templates）
        assert!(is_junk_table("x_tmp", &DigestOptions::default()));
        assert!(!is_junk_table("package_templates", &DigestOptions::default()));
        assert!(is_junk_table("_volc_dts_pg_ddl_9f", &DigestOptions::default()));
    }

    /// ⭐ REQ-171 · 大库打分**主要看列注释**（中文是唯一的桥），表名只是次要信号。
    #[test]
    fn column_comments_are_the_main_scoring_signal() {
        let schema = r#"{"tables":[
          {"name":"t_no_comment","columns":[{"name":"id","data_type":"text"}]},
          {"name":"t_with_comment","columns":[
             {"name":"id","data_type":"text"},
             {"name":"quota","data_type":"bigint","comment":"API-Key 限额"}]}]}"#;
        let v = SchemaView::from_json(schema).unwrap();
        // 正文只出现**列注释**里的中文，两个表名都没出现
        let opts = DigestOptions { max_tables: 10, full_list_max: 0, ..DigestOptions::default() };
        let d = v.digest("这一节讲 API-Key 限额怎么算。", &opts);
        assert_eq!(d.matched, 1, "只有列注释命中的那张算相关：{}", d.text);
        assert_eq!(d.included, 1);
        assert!(d.text.contains("t_with_comment"), "{}", d.text);
        assert!(!d.text.contains("t_no_comment"), "{}", d.text);
        // 列注释本身也要进摘要（模型得看得见这层中文）
        assert!(d.text.contains("API-Key 限额"), "{}", d.text);
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
        let opts = DigestOptions { max_tables: 5, full_list_max: 0, ..DigestOptions::default() };
        let d = v.digest(&corpus, &opts);
        assert_eq!(d.included, 5, "上限 5");
        assert_eq!(d.matched, 20, "命中候选 20 —— 被上限截到 5");
        assert_eq!(d.max_tables, 5);
        assert_eq!(d.text.matches("表 `orders").count(), 5, "{}", d.text);
    }

    #[test]
    fn a_document_without_any_matching_table_gets_zero_tables_and_a_warning() {
        // 大库 + 0 命中 ⇒ 一张都不带（⛔ 不拿无关的表凑数），并明确说「本轮不要给 maps_to」
        let opts = DigestOptions { max_tables: 40, full_list_max: 0, ..DigestOptions::default() };
        let d = view().digest("这段文档讲的是采购流程与审批人。", &opts);
        assert_eq!(d.included, 0);
        assert!(d.text.contains("本轮不要给 `maps_to`"), "{}", d.text);
        assert!(!d.text.contains("cloud_disk"), "{}", d.text);

        // ⭐ REQ-171：小库即使 0 命中，**仍然给全表清单**（这才是让概念能落表的前提）。
        let d = view().digest("这段文档讲的是采购流程与审批人。", &DigestOptions::default());
        assert_eq!(d.matched, 0);
        assert_eq!(d.included, 3, "全表清单不因「没命中」而清空：{}", d.text);
        assert!(d.text.contains("cloud_disk"), "{}", d.text);
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

    // ── ⭐ REQ-171 · 概念 → 表（`{table, column?}`）────────────────

    /// 概念 `maps_to` 的**正常路径**：表名 / 中文名 / 可选列 / 只给列（概念名唯一对表）。
    #[test]
    fn a_concept_maps_to_lands_on_a_table_by_name_or_chinese_name() {
        let v = view();
        let cases = [
            (serde_json::json!({"table":"cloud_disk"}), "cloud_disk", None),
            // 模型常把**表注释**当表名写进来 —— 按中文名唯一对上就收
            (serde_json::json!({"table":"云硬盘"}), "cloud_disk", None),
            (
                serde_json::json!({"table":"cloud_disk","column":"instance_id"}),
                "cloud_disk",
                Some("instance_id"),
            ),
            // 只给列：概念「云硬盘」唯一对一张表 ⇒ 用它
            (serde_json::json!({"column":"size_gb"}), "cloud_disk", Some("size_gb")),
        ];
        for (raw, table, column) in cases {
            let m = v.resolve_concept_maps_to("云硬盘", &raw).unwrap_or_else(|p| {
                panic!("{raw} 应当落地，实际被丢（{}）：{}", p.reject.label(), p.detail)
            });
            assert_eq!(m.table, table, "{raw}");
            assert_eq!(m.column.as_deref(), column, "{raw}");
        }
    }

    /// ⭐ REQ-171 · **四类丢弃码各一条**（判据 2 的单元版）。
    #[test]
    fn the_four_concept_maps_to_discard_codes_are_all_reachable() {
        let v = view();
        let code = |raw: serde_json::Value| {
            v.resolve_concept_maps_to("云硬盘", &raw).expect_err("应当被丢").reject
        };
        // ① 形状：不是对象
        assert_eq!(code(serde_json::json!("cloud_disk.id")), MapsToReject::BadShape);
        // ① 形状：把**关系**的键混进概念（两件事，必须拒）
        let p = v
            .resolve_concept_maps_to(
                "云硬盘",
                &maps(serde_json::json!({"table":"cloud_disk","child_column":"x",
                                         "parent_column":"y"})),
            )
            .expect_err("混写必须拒");
        assert_eq!(p.reject, MapsToReject::BadShape);
        assert!(p.detail.contains("child_column"), "{}", p.detail);
        // ① 形状：table 不是字符串
        assert_eq!(code(serde_json::json!({"table": 7})), MapsToReject::BadShape);
        // ② 查不到表：表名与中文名都对不上
        assert_eq!(code(serde_json::json!({"table":"no_such_table"})), MapsToReject::TableUnknown);
        // ③ 对上多张：概念名同时命中「云硬盘」与「云硬盘快照」
        let amb = SchemaView::from_json(
            r#"{"tables":[
              {"name":"disks","comment":"云硬盘","columns":[{"name":"id","data_type":"text"}]},
              {"name":"disk_snapshots","comment":"云硬盘快照",
               "columns":[{"name":"id","data_type":"text"}]}]}"#,
        )
        .unwrap();
        let p = amb
            .resolve_concept_maps_to("云硬盘", &maps(serde_json::json!({"table":"云硬盘"})))
            .expect_err("对上多张必须拒");
        assert_eq!(p.reject, MapsToReject::TableAmbiguous);
        assert!(p.detail.contains("disk_snapshots"), "{}", p.detail);
        // ③ 对上多张：没有 table 时「这一列在多张表里」
        let dup = SchemaView::from_json(
            r#"{"tables":[
              {"name":"a","columns":[{"name":"ref","data_type":"text"}]},
              {"name":"b","columns":[{"name":"ref","data_type":"text"}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            dup.resolve_concept_maps_to("甲", &maps(serde_json::json!({"column":"ref"})))
                .expect_err("列在多张表里必须拒")
                .reject,
            MapsToReject::TableAmbiguous
        );
        // ④ 查不到列：表对了但列不在那张表里
        let p = v
            .resolve_concept_maps_to(
                "云硬盘",
                &maps(serde_json::json!({"table":"cloud_disk","column":"no_such_col"})),
            )
            .expect_err("列不存在必须拒");
        assert_eq!(p.reject, MapsToReject::ColumnAbsent);
        assert!(p.detail.contains("no_such_col"), "{}", p.detail);
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
        let d = v.digest("磁盘挂在服务器上", &DigestOptions::with_max_tables(10));
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

    // ── ⭐ F-2（QA 退回项）· 字符闸对「单表超宽」也必须生效 ──────────

    /// 一张 3001 列的表：摘要必须**按列截断**到上限以内，且**说清还剩多少列没列**。
    ///
    /// 退回前的形态：字符闸只会「丢表」，而单表丢不掉 ⇒ QA 实测 60 478 字符（上限 5 倍）。
    #[test]
    fn a_single_very_wide_table_is_cut_by_columns_and_stays_under_the_char_cap() {
        let cols: Vec<String> = (0..3001)
            .map(|i| format!(r#"{{"name":"c{i:04}","data_type":"text"}}"#))
            .collect();
        let schema = format!(
            r#"{{"tables":[{{"name":"wide_table","comment":"宽表","primary_key":"c0000",
                "columns":[{}]}}]}}"#,
            cols.join(",")
        );
        let v = SchemaView::from_json(&schema).expect("合法");
        let d = v.digest("这张宽表是文档的主角 wide_table", &DigestOptions::with_max_tables(40));
        assert_eq!(d.included, 1, "只有一张表");
        let n = d.text.chars().count();
        assert!(
            n <= MAX_DIGEST_CHARS,
            "⛔ 单表超宽时摘要仍然超限：{n} 字符 > {MAX_DIGEST_CHARS}（F-2 的原始形态是 60478）"
        );
        assert!(
            d.text.contains(&format!("另有 {} 列未列", 3001 - MAX_COLUMNS_PER_TABLE)),
            "截掉多少列必须**说清**（⛔ 不能悄悄少给）：{}",
            d.text
        );
        assert!(d.text.contains("c0000 text PK"), "{}", d.text);
        assert!(
            !d.text.contains("c0024 "),
            "第 {} 列之后不该再列出来：{}",
            MAX_COLUMNS_PER_TABLE,
            d.text
        );
    }

    // ── ⭐ F-1（QA 退回项）· 「主键：无」只在 schema 真的说了时才允许出现 ──

    /// 三种情形必须**分得开**：
    ///
    /// | `schema.json` 里 | 摘要该写什么 |
    /// | --- | --- |
    /// | 有 `primary_key: "id"` | `主键：id` |
    /// | 有键字段但**真的没有**主键（`primary_key: null` + 空数组） | `主键：无（这张表真的没有…）` |
    /// | **一个键字段都没有**（旧产物 / 别家手写） | `主键：**未勘明**`（⛔ 不许写「无」） |
    ///
    /// 第三行就是 F-1 的假信息来源：只读账号下主键读不到，摘要写成「没有主键」，
    /// 模型据此永远不会给对的 `maps_to`。
    #[test]
    fn the_no_primary_key_line_is_only_used_when_the_schema_really_says_so() {
        let with_pk = SchemaView::from_json(
            r#"{"tables":[{"name":"t_pk","comment":"有主键","primary_key":"id",
                 "columns":[{"name":"id","data_type":"text","is_primary_key":true}]}]}"#,
        )
        .unwrap();
        let d = with_pk.digest("有主键 t_pk", &DigestOptions::with_max_tables(10));
        assert!(d.text.contains("主键：id"), "{}", d.text);

        let no_pk = SchemaView::from_json(
            r#"{"tables":[{"name":"t_nopk","comment":"真的没主键","primary_key":null,
                 "composite_primary_key":[],"unique_columns":[],
                 "columns":[{"name":"a","data_type":"text"}]}]}"#,
        )
        .unwrap();
        let d = no_pk.digest("真的没主键 t_nopk", &DigestOptions::with_max_tables(10));
        assert!(
            d.text.contains("主键：无（这张表真的没有单列/复合主键）"),
            "有键事实 + 真没有主键 ⇒ 才允许写「无」：{}",
            d.text
        );

        let unknown = SchemaView::from_json(
            r#"{"tables":[{"name":"t_unknown","comment":"不知道","columns":[{"name":"a","data_type":"text"}]}]}"#,
        )
        .unwrap();
        let d = unknown.digest("不知道 t_unknown", &DigestOptions::with_max_tables(10));
        assert!(
            !d.text.contains("主键：无"),
            "⛔ 键字段都不在（不知道）时**不许**写成「没有主键」：{}",
            d.text
        );
        assert!(
            d.text.contains("未勘明"),
            "要说清「这份 schema.json 没有键事实」：{}",
            d.text
        );
    }
}
