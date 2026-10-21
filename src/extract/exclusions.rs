//! REQ-113 · **排除表：从「脚本预处理」变成产品能力**
//!
//! # 这个模块为什么存在（不是「加个过滤参数」）
//!
//! 2026-10-21 用真实库建本体时，BA 用一份 `.local/_filt.py` 把 96 张表**手工过滤**
//! 成 83 张。脚本不在仓库里、不被跟踪、不留记录 —— 于是同一份源库，
//! 另一个人直接跑 `ontok-propose` 得到的是 **96 张表 / 1116 列**，而不是
//! 「83 张 / 947 列」。（数字是当时那份手工脚本的产出；后来 AI 先判一轮后
//! 口径变成 96 − 12 = 84 —— 具体几个不是本模块的事，**可复现**才是。）
//! 三个后果，一个比一个重：
//!
//! | # | 后果 | 本模块怎么消除它 |
//! | --- | --- | --- |
//! | 1 | **不可复现**（脚本在 `.local/`） | 排除是**仓库里的文件**，随代码评审 |
//! | 2 | **不可审计**（stats 里没有「排了谁、为什么」） | `stats.excluded_tables` 逐条记 |
//! | 3 | **数字无出处**（83 vs 96 没有记录） | `seen / excluded / extracted` 三计数 + 闭合断言 |
//!
//! 产品里原本还有一条「不排除」的替代路径，它**更糟**：那批噪声表会先被建成实体
//! （记进 `tables_without_cn_name`），直到客户视图那一步才被过滤 ——
//! **噪声进了本体，只是被挡在出口**。所以排除必须发生在**抽取之前**。
//!
//! # 裁定（需求 R1–R6，逐条落在这里）
//!
//! | # | 裁定 | 落点 |
//! | --- | --- | --- |
//! | R1 | 表级；列级「属性剔除」仍是 `property_filter` 的事 | 本模块只处理整表，一个列都不碰 |
//! | R2 | 必须有理由，且业务能读懂 | [`Entry::reason`] 空 ⇒ 拒文件；报错文案直接引用 AC-003 |
//! | R3 | 类别是**枚举**，不许自由发挥 | [`Category`]，新增类别 = 改枚举 + 升版本 |
//! | R4 | 陈旧排除默认**警告 + 计数**，`--strict-exclusions` 才失败 | [`Run::stale`] |
//! | R5 | 三计数必须闭合（结构性断言） | [`ExtractionStats::stamp_counts`] |
//! | R6 | 与「无中文名」**分开记**，排除优先 | [`Run::report`] 里已无该表 ⇒ 它不可能再进 `tables_without_cn_name` |
//!
//! # 纪律 H：排除**只有这一个实现**
//!
//! 「哪些表被排除了」这个判断在全仓只有一处：本模块的 [`Exclusions::apply`]。
//! 提案（`ontology 抽取`）、`stats.json`、审核界面读的都是**它产出的同一份结果**
//! （`stats.excluded_tables` → 落库 → `GET /api/review`）。
//! 审核界面**不重算**：它连 schema 都没有，只有这份记录 ——
//! 这正是「一个实现」在结构上的含义，不是一句约定。
//!
//! # 确定性（AC-008）
//!
//! 本模块里没有任何 `HashMap` 迭代、没有时间戳、没有随机数：
//! - [`Run::applied`] / [`Run::stale`] 按**文件顺序**产出；
//! - 剔除表时保持 `report.tables` 的原顺序；
//! - [`Exclusions::sha256`] 是对**文件原始字节**的哈希（改一个字节就变，同一份文件恒相同）。
//!
//! ⇒ 同一份 schema + 同一份 exclusions 跑两次，提案与 stats 逐字节相同。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::extract::introspect::SchemaReport;
use crate::extract::sample::sha256_hex;

// ══════════════════════════════════════════════════════════════
// R3 · 类别枚举（⛔ 不许自由发挥）
// ══════════════════════════════════════════════════════════════

/// 排除的**类别** —— 固定七种（需求 R3，原样照抄，一个不多一个不少）。
///
/// 为什么是枚举而不是自由文本：类别是**跨库可比**的那一栏
/// （「这个库排了 N 张，其中几张是备份表」），自由文本会让它退化成第二份理由。
/// 新增类别必须改这里并**升 `version`**（评审里显式说清）—— 让「加一类」成为一次显式的决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// CDC / 数据同步服务自动创建的残留表。
    CdcResidue,
    /// 迁移框架的簿记表（`alembic_version` 这类）。
    MigrationBookkeeping,
    /// 备份 / 快照表（某次改动前的整表复制）。
    BackupSnapshot,
    /// 内部管道表（outbox / 缓存 / 触发器状态）。
    InternalPipeline,
    /// 报表中间结果表（DWD / DIM 中间层）。
    ReportIntermediate,
    /// 已废弃表（功能下线后保留）。
    Deprecated,
    /// 超出本轮建模范围（非对外业务对象）。
    OutOfScope,
}

impl Category {
    /// 全部类别，**按需求 R3 的顺序**（报错文案与界面分组都用这个顺序）。
    pub const ALL: [Category; 7] = [
        Category::CdcResidue,
        Category::MigrationBookkeeping,
        Category::BackupSnapshot,
        Category::InternalPipeline,
        Category::ReportIntermediate,
        Category::Deprecated,
        Category::OutOfScope,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::CdcResidue => "cdc_residue",
            Category::MigrationBookkeeping => "migration_bookkeeping",
            Category::BackupSnapshot => "backup_snapshot",
            Category::InternalPipeline => "internal_pipeline",
            Category::ReportIntermediate => "report_intermediate",
            Category::Deprecated => "deprecated",
            Category::OutOfScope => "out_of_scope",
        }
    }

    /// 给人看的一句话解释（审核界面与报错共用，**不另写一份**）。
    pub fn label(self) -> &'static str {
        match self {
            Category::CdcResidue => "CDC / 数据同步残留",
            Category::MigrationBookkeeping => "迁移框架簿记",
            Category::BackupSnapshot => "备份 / 快照",
            Category::InternalPipeline => "内部管道（outbox / 缓存）",
            Category::ReportIntermediate => "报表中间结果",
            Category::Deprecated => "已废弃",
            Category::OutOfScope => "超出建模范围",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Category::ALL.iter().copied().find(|c| c.as_str() == s)
    }

    /// 解析类别；非法值 ⇒ 报错并**列全**合法值（R3 的「不许自由发挥」要靠文案兜住）。
    fn require(s: &str, table: &str) -> Result<Self> {
        Category::parse(s).ok_or_else(|| {
            anyhow::anyhow!(
                "排除文件里 {table:?} 的 category = {s:?} 不在枚举内。\n\
                 合法类别（R3，共 {} 种）：{}\n\
                 新增类别必须先改 `extract::exclusions::Category` 并升 `version` —— \
                 自由发挥的类别会让「跨库可比」这一栏失效。",
                Category::ALL.len(),
                Category::ALL
                    .iter()
                    .map(|c| format!("{}（{}）", c.as_str(), c.label()))
                    .collect::<Vec<_>>()
                    .join(" / ")
            )
        })
    }
}

// ══════════════════════════════════════════════════════════════
// 文件形状
// ══════════════════════════════════════════════════════════════

/// 一条排除声明 —— **表名 / 类别 / 理由 / 决策人**，四个都不能缺（AC-003）。
///
/// 「谁决定的」不是可选元数据：需求风险 1 是「排除变成悄悄丢数据」，
/// 而一条没有决策人的理由无法被追问。所以 `decided_by` 空 ⇒ **拒绝文件**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// 源库表名（`public` schema 内的表名，原样）。
    pub table: String,
    pub category: Category,
    /// **业务能读懂**的理由（R2）。写「技术原因」四个字不算 —— 见 [`REASON_MIN`]。
    pub reason: String,
    /// 谁做的这个决定（人名 / 角色 + 日期都行，但必须可追）。
    ///
    /// **刻意是自由文本，不是枚举**：AI 判一轮时写 `"ai-table-judge"`，
    /// 人复核之后要能就地改成 `"张三（业务）2026-10-22 确认"` ——
    /// 「谁决定的」这一栏的价值就在**能追到具体的人**，枚举会把它锁死在几个角色上。
    /// 唯一的要求是非空（AC-003）。
    pub decided_by: String,
    /// 这一条排除的**置信度**（0–1，可选）。
    ///
    /// 出现的原因：清单可能由 AI 先判一轮（`decided_by: "ai-table-judge"`），
    /// 而「AI 有多确定」正是评审人**质疑它的入口** —— 藏起来就只剩一句断言。
    /// 缺省 = 没有这个量（人工判断本来就不带分数），与 `0.0` **不是一回事**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    /// 判定依据（可选）：AI 那一轮为什么这么判（命中了什么命名/注释/结构特征）。
    ///
    /// 评审人要「推翻」一条排除，靠的是**依据**而不是结论 ——
    /// 所以这一栏若给了就一路带到审核界面上，不在这里丢掉。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

/// 一条**保留但存疑**的表（`keep_but_flagged`）—— 与排除**语义相反**，⛔ 不许合并。
///
/// - 排除（[`Entry`]）是**已做出的决定**：这张表不进本体；
/// - 待裁决（本类型）是**还没做的决定**：表**照常进本体**，但评审人应当看到疑虑。
///
/// 把两者放进同一个列表，就等于把「已经排除 N 张」和「有 M 张需要人看一眼」
/// 说成同一件事 —— 前者是结论，后者是问题。审核界面必须**分开两块**显示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flagged {
    pub table: String,
    /// 为什么拿不准（一句话，业务能读懂）。
    pub why: String,
    /// 具体担心什么 / 建议谁来看（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concern: Option<String>,
}

impl Entry {
    /// 置信度是否落在 `0.0..=1.0`。没有置信度也算合法（`None` 不是 0 分）。
    pub fn confidence_is_valid(&self) -> bool {
        match self.confidence {
            None => true,
            Some(c) => c.is_finite() && (0.0..=1.0).contains(&c),
        }
    }
}

/// 理由的**最短**长度（字符数）。
///
/// 不设门槛的话，`reason: "x"` 也能过 —— 而 AC-003 判的从来不是
/// 「字段非空」这一件事，是「**可读**」。6 个字符都写不出的话，
/// 这条排除本来就还没想清楚。
pub const REASON_MIN: usize = 6;

/// 原始文件（宽松反序列化：**缺字段**由本模块逐条报人话，而不是把 serde 的
/// 「missing field `reason`」原样倒给用户）。
#[derive(Debug, Deserialize)]
struct RawFile {
    /// 清单**自己的**版本号：接受字符串（`"0.1.1"`）或数字（`1`）。
    version: Option<serde_json::Value>,
    rule: Option<String>,
    declared_by: Option<String>,
    /// 谁复核/签的字（可选）：AI 先判一轮时，`declared_by` 是判的人，
    /// 这一栏是**签字的人** —— 两件事，审核界面都要能看见。
    reviewed_by: Option<String>,
    exclude: Option<Vec<RawEntry>>,
    /// 「保留但存疑」—— 表**照常进本体**，只是要让人看见疑虑。
    keep_but_flagged: Option<Vec<Flagged>>,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    table: Option<String>,
    category: Option<String>,
    reason: Option<String>,
    decided_by: Option<String>,
    /// 可选：AI 先判一轮时带上的置信度。
    confidence: Option<f64>,
    /// 可选：判定依据。
    evidence: Option<String>,
}

/// 逐条记录落在**提案旁边**的那个文件（`--out` 同目录）。
///
/// # 为什么逐条记录不写进 `proposal.json`（这是 AC-001 与 AC-004 的正面冲突）
///
/// - **AC-001**：「`proposal.json` 里 grep 被排除的表名 = 0」——
///   这是**字面判据**，QA 会拿它当命令跑；
/// - **AC-004**：「提案元信息记 `inputs.exclusions = {file, version, sha256}`」。
///
/// 把逐条记录（含表名）塞进提案，AC-004 满足、**AC-001 的 grep 就红了** ——
/// 而那条 grep 的意图是对的：提案里不该出现被排除表的**任何痕迹**，
/// 否则「它到底进没进本体」又得靠人眼逐个分辨。
///
/// ⇒ 拆成两处，各说各的：
/// 1. `proposal.json` 的 `inputs.exclusions`：**只有元信息**（file / version / sha256 / 记录路径）；
/// 2. 本常量指的那个文件：逐条记录（表名 / 类别 / 理由 / 决策人 / 置信度 / 依据 / 陈旧 / 待裁决）。
///
/// 两者的**绑定**是 `sha256`：`propose import` 读这份文件时会先比 sha256，
/// 对不上就拒绝入库（「记录不是这一轮的」）。
/// 命名沿用本仓既有侧文件约定（`proposal-pending.json` / `proposal-rejected.json`）。
pub const RECORD_FILE: &str = "proposal-exclusions.json";

/// 一份**已校验**的排除文件。
///
/// 构造只有两条路：[`Exclusions::load`] / [`Exclusions::parse`]，两条都走
/// [`Exclusions::parse`] 里那**同一套**校验。所以「拿在手里的 `Exclusions` 一定合法」
/// 是**类型层面**的事实 —— 后面的代码不必再防一次。
#[derive(Debug, Clone)]
pub struct Exclusions {
    /// 文件标识：**调用方给的那个路径字符串**（不是绝对路径）。
    ///
    /// 为什么不用 `canonicalize`：产物里的 `file` 要能被人一眼对上「我跑的是哪份」，
    /// 而 `/Users/…/feature-REQ-113/exclusions.json` 与 `exclusions.json` 相比，
    /// 后者才是那条命令里写的东西。绝对路径会把「同一份文件在另一台机器上」
    /// 变成另一个字符串，sha256 却相同 —— 那正好是**可复现**要的证据。
    file: String,
    /// 清单**自己的**版本号（原样，`"0.1.1"` 这种；见 [`version_text`]）。
    version: String,
    rule: String,
    declared_by: String,
    /// 签字的人（可选）：AI 先判一轮时与 [`Self::declared_by`] 分开记。
    reviewed_by: Option<String>,
    /// 文件**原始字节**的 sha256（AC-004：改一个字节就变）。
    sha256: String,
    entries: Vec<Entry>,
    /// 「保留但存疑」的表 —— 它们**照常进本体**，这里只是留痕（⛔ 与排除分开）。
    flagged: Vec<Flagged>,
}

impl Exclusions {
    /// 读文件 + 校验。任何一条不合规 ⇒ **拒绝该文件**（调用方 exit≠0）。
    pub fn load(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| {
            format!(
                "读不到排除文件 {path:?}。\n\
                 排除是**产品能力**而不是脚本开关：这个文件应当随代码入库、可评审。\n\
                 首次使用可参考仓库根的 `exclusions.json`（真实库 anspire_open_uat 那一份）。"
            )
        })?;
        Self::parse(&bytes, path)
    }

    /// 校验一份内存里的文件内容。`file` 只作为标识进产物（不参与解析）。
    pub fn parse(raw: &[u8], file: &str) -> Result<Self> {
        // sha256 只认**原始字节**：这样「改一个字节 sha 就变」是定义，不是巧合。
        let sha256 = sha256_hex(raw);

        let text = std::str::from_utf8(raw)
            .with_context(|| format!("排除文件 {file:?} 不是 UTF-8 文本，拒绝解析"))?;
        let raw_file: RawFile = serde_json::from_str(text).with_context(|| {
            format!(
                "排除文件 {file:?} 不是合法的排除 JSON（期望 \
                 {{version, rule, declared_by, exclude:[{{table, category, reason, decided_by}}]}}）"
            )
        })?;

        // `version` 是**这份清单自己的**版本号，原样记进溯源。
        //
        // 接受字符串（`"0.1.1"`，与 `table_names.json` 同一种风格）或数字（`1`）：
        // 两种写法都在真实文件里出现过，卡死其中一种只会逼人改文件去迁就解析器。
        // 但**空版本**必须拒绝 —— 溯源里一栏空白，等于「不知道这份清单是哪一代」。
        let version = version_text(raw_file.version.as_ref(), file)?;

        let rule = require_text(raw_file.rule.as_deref(), file, "rule", "这份文件按什么规则排除（一句话，评审时最先看的就是它）")?;
        let declared_by = require_text(
            raw_file.declared_by.as_deref(),
            file,
            "declared_by",
            "谁声明了这份排除清单（人名或角色 + 日期；AI 先判时写判它的那个东西）",
        )?;
        // `reviewed_by` 可选：AI 先判一轮时它记**签字的人**。
        // 「谁判的」与「谁签的」是两件事，能分开就别合并（评审要追的是后者）。
        let reviewed_by = raw_file.reviewed_by.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

        let raw_entries = raw_file.exclude.ok_or_else(|| {
            anyhow::anyhow!(
                "排除文件 {file:?} 缺 `exclude` 数组。\
                 连一个条目都没有的排除文件不是「排除了 0 张」，是**还没做决定**。"
            )
        })?;
        if raw_entries.is_empty() {
            bail!(
                "排除文件 {file:?} 的 `exclude` 是空数组。\n\
                 一份不排除任何表的文件等于没有做出排除决定 —— 请删掉它（不传 --exclude 即可），\
                 或者补上真正的条目。留着它只会让「排除了 N 张」这件事凭空缩水。"
            );
        }

        let mut entries: Vec<Entry> = Vec::with_capacity(raw_entries.len());
        for (i, r) in raw_entries.into_iter().enumerate() {
            let n = i + 1;
            let table = require_text(r.table.as_deref(), file, "exclude[].table", "源库表名")?;
            // 重复表名 ⇒ 拒绝：同一条排除出现两次时，
            // 「按文件顺序产出」与「累计 entry 数」会给出两个不同的 excluded 数。
            if let Some(prev) = entries.iter().position(|e| e.table == table) {
                bail!(
                    "排除文件 {file:?} 里表 {table:?} 出现了两次（第 {} 条与第 {n} 条）。\n\
                     重复条目会让「排除了几张表」有两个答案：按条目数是 {n}，按表数是 {}。\n\
                     口径必须唯一，所以这里**拒绝文件**。",
                    prev + 1,
                    entries.len() + 1
                );
            }
            let category_raw = require_text(
                r.category.as_deref(),
                file,
                "exclude[].category",
                "排除类别（R3 的固定枚举）",
            )?;
            let category = Category::require(&category_raw, &table)?;
            let reason = require_text(
                r.reason.as_deref(),
                file,
                "exclude[].reason",
                "**业务能读懂**的排除理由（不是「技术原因」四个字）",
            )?;
            if reason.chars().count() < REASON_MIN {
                bail!(
                    "排除文件 {file:?} 里表 {table:?} 的 reason 只有 {} 个字符，太短。\n\
                     理由要能被**业务**读懂（AC-003 / R2）：写清「这张表是什么、为什么不属于业务模型」，\
                     而不是「技术原因」「无用」这类等于没说的词。",
                    reason.chars().count()
                );
            }
            let decided_by = require_text(
                r.decided_by.as_deref(),
                file,
                "exclude[].decided_by",
                "谁决定排除这张表（必须可追）",
            )?;
            let entry = Entry {
                table,
                category,
                reason,
                decided_by,
                confidence: r.confidence,
                evidence: r.evidence.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            };
            // 置信度是**可选**的，给了就必须是 0–1 的有限数。
            // 不校验的话，一个 `1.5` 会在界面上显示成「150%」或「1.5」——
            // 评审人拿它判断「AI 有多确定」时，那个数已经不等于任何东西了。
            if !entry.confidence_is_valid() {
                bail!(
                    "排除文件 {file:?} 里表 {:?} 的 confidence = {:?} 不在 0.0–1.0 之间。\n\
                     这一栏是「AI 有多确定」的入口（评审人据此决定要不要推翻），\
                     越界的数会让它失去意义。不给这一栏也行（人工判断本来就不带分数）。",
                    entry.table, entry.confidence
                );
            }
            entries.push(entry);
        }

        // 「保留但存疑」：表**照常进本体**，只是要让人看见疑虑。
        // 与 `exclude` 的交集必须为空 —— 同一张表既「已排除」又「保留待裁决」
        // 是自相矛盾的文件，读它的人会得出两个结论。
        let mut flagged: Vec<Flagged> = Vec::with_capacity(
            raw_file.keep_but_flagged.as_ref().map(|v| v.len()).unwrap_or(0),
        );
        for (i, f) in raw_file.keep_but_flagged.unwrap_or_default().into_iter().enumerate() {
            let n = i + 1;
            let table = require_text(Some(f.table.as_str()), file, "keep_but_flagged[].table", "源库表名")?;
            if entries.iter().any(|e| e.table == table) {
                bail!(
                    "排除文件 {file:?} 里表 {table:?} 同时出现在 `exclude` 与 `keep_but_flagged`。\n\
                     一个说「已排除」一个说「保留待裁决」，两句话不能同时为真。"
                );
            }
            if flagged.iter().any(|x| x.table == table) {
                bail!("排除文件 {file:?} 里 `keep_but_flagged` 的 {table:?} 出现了两次（第 {n} 条）");
            }
            let why = require_text(Some(f.why.as_str()), file, "keep_but_flagged[].why", "为什么拿不准（一句话，业务能读懂）")?;
            flagged.push(Flagged {
                table,
                why,
                concern: f.concern.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            });
        }

        Ok(Exclusions { file: file.to_string(), version, rule, declared_by, reviewed_by, sha256, entries, flagged })
    }

    pub fn file(&self) -> &str {
        &self.file
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn rule(&self) -> &str {
        &self.rule
    }
    pub fn declared_by(&self) -> &str {
        &self.declared_by
    }
    /// 签字的人（可选）。AI 先判一轮时，「谁判的」在 [`Self::declared_by`]。
    pub fn reviewed_by(&self) -> Option<&str> {
        self.reviewed_by.as_deref()
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    /// 保留了、但有疑虑的表（⛔ 与排除分开记）。
    pub fn flagged(&self) -> &[Flagged] {
        &self.flagged
    }

    /// **排除判定的唯一实现**（纪律 H）：把文件里点名、且**真的在 schema 里**的表
    /// 从报告里剔掉，返回剔除后的报告 + 逐条记录 + 陈旧清单。
    ///
    /// # 为什么在**抽取之前**剔，而不是抽完再过滤
    ///
    /// 两点，缺一不可：
    /// 1. AC-001 判的是「提案里 grep 被排除的表名 = 0」。抽完再删要保证实体、属性、
    ///    关系、证据、置信度、TTL 里**一处都不剩** —— 那是六个地方，漏一个就是噪声
    ///    进了本体；
    /// 2. 剔除后这些表也**不在「已知表集合」里**了，于是任何 `xxx_id` 都不会再
    ///    推断出一条指向噪声表的关系。抽完再删则会留下**悬空关系**。
    ///
    /// # 为什么 `report` 的计数要重算，而 `foreign_key_count` / 时间戳不动
    ///
    /// `table_count` / `column_count` / `commented_column_count` 是**作用域**的计数，
    /// 剔了表就必须跟着变（否则一份表头写着 96 张、`tables` 里只有 83 张的报告会被下游的
    /// 一致性自检判成「空壳迹象」）。
    /// `server_version` / `introspected_at` / `transaction_read_only` / `foreign_key_count`
    /// 是**勘查事实**（这次勘查看到了什么），排除改的是作用域，不是历史 ——
    /// 改它们等于伪造勘查留痕。
    pub fn apply(&self, report: &SchemaReport) -> Run {
        let excluded: std::collections::HashSet<&str> =
            self.entries.iter().map(|e| e.table.as_str()).collect();

        let mut kept = Vec::with_capacity(report.tables.len());
        for t in &report.tables {
            if !excluded.contains(t.name.as_str()) {
                kept.push(t.clone());
            }
        }

        // 逐条记录按**文件顺序**产出（确定性 + 评审时与文件对得上）。
        let mut applied = Vec::new();
        let mut stale = Vec::new();
        for e in &self.entries {
            if report.tables.iter().any(|t| t.name == e.table) {
                applied.push(e.clone());
            } else {
                stale.push(e.table.clone());
            }
        }

        let mut filtered = report.clone();
        filtered.table_count = kept.len();
        filtered.column_count = kept.iter().map(|t| t.columns.len()).sum();
        filtered.commented_column_count = kept.iter().map(|t| t.commented_count).sum();
        filtered.tables = kept;

        // 「保留但存疑」原样带走：它们**不进**上面的剔除集合，
        // 只是要让评审人看见 —— 所以这里不做「存在性过滤」，
        // 也不因为它们去动作用域（存疑 ≠ 排除）。
        Run { report: filtered, applied, stale, flagged: self.flagged.clone() }
    }

    /// 本轮排除的**记录块**（写进提案旁边那份 `proposal-exclusions.json`，
    /// 也是审核界面从库里读的那一份）。
    pub fn record(&self, run: &Run) -> Record {
        Record {
            file: self.file.clone(),
            version: self.version.clone(),
            sha256: self.sha256.clone(),
            declared_by: self.declared_by.clone(),
            reviewed_by: self.reviewed_by.clone(),
            rule: self.rule.clone(),
            tables: run.applied.clone(),
            stale: run.stale.clone(),
            flagged: run.flagged.clone(),
        }
    }

    /// 提案里的 `inputs.exclusions` 块（AC-004）—— **只有元信息，一个表名都没有**。
    ///
    /// 逐条记录在 [`RECORD_FILE`] 里；两处靠 `sha256` 绑定（[`load_record`]）。
    pub fn inputs_ref(&self, record_file: &str) -> serde_json::Value {
        serde_json::json!({
            "file": self.file,
            "version": self.version,
            "sha256": self.sha256,
            "declared_by": self.declared_by,
            "record": record_file,
        })
    }
}

/// `version` 字段 → 溯源里那一栏的字符串。
///
/// 接受两种写法，因为真实文件里两种都出现过：
/// - **字符串**（`"0.1.1"`）—— 本仓 `table_names.json` 的版本号也是这种风格；
/// - **数字**（`1`）—— 早期手写的排除文件。
///
/// 拒绝空串 / 非标量 / 缺失：溯源里一栏空白等于「不知道这份清单是哪一代」，
/// 而「不知道」在统计与界面里都只能被显示成缺失 —— 那正是要避免的形态。
fn version_text(v: Option<&serde_json::Value>, file: &str) -> Result<String> {
    let Some(v) = v else {
        bail!(
            "排除文件 {file:?} 缺 `version`。\n\
             这一栏是**这份清单自己的版本号**（例如 \"0.1.1\"），会原样进溯源\
             （`inputs.exclusions.version`）。没有它，改过一版也说不清是哪一版。"
        );
    };
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => bail!(
            "排除文件 {file:?} 的 `version` 只能是字符串或数字，收到 {other:?}"
        ),
    };
    if s.is_empty() {
        bail!("排除文件 {file:?} 的 `version` 是空串 —— 空版本号等于没有版本号");
    }
    Ok(s)
}

/// 要求一个非空文本字段，报错里点名「哪份文件、哪个字段、该写什么」。
fn require_text(v: Option<&str>, file: &str, field: &str, want: &str) -> Result<String> {
    let s = v.map(str::trim).unwrap_or("");
    if s.is_empty() {
        bail!(
            "排除文件 {file:?} 缺 `{field}`（或它是空白）。\n\
             这里要写：{want}。\n\
             ⛔ 缺字段不走默认值：排除是**主动决定**，一个靠默认值补出来的字段\
             会让「谁决定的 / 为什么」同时失去出处。"
        );
    }
    Ok(s.to_string())
}

/// 一次 [`Exclusions::apply`] 的结果 —— **三样东西一起产出，不分开算**。
///
/// 拆成「过滤后的报告」「逐条记录」「陈旧清单」三份独立数据会立刻带来
/// 「统计说排了 N 张、提案里却少剔了一张」这类分叉。它们同出一次判定，
/// 所以只可能同时正确或同时错误（后者由下面的闭合断言语义兜住）。
pub struct Run {
    /// 剔除之后交给抽取器的报告（`tables.len() == seen - applied.len()`）。
    pub report: SchemaReport,
    /// 真的剔掉了的表（文件顺序）。
    pub applied: Vec<Entry>,
    /// 文件里点名、schema 里**不存在**的表（R4：默认警告，`--strict-exclusions` 才失败）。
    pub stale: Vec<String>,
    /// 「保留但存疑」的表 —— 它们**在 report 里照常存在**（`kept`），
    /// 这一栏只用于记录与显示。⛔ 任何情况下都不许把它与 [`Self::applied`] 合并。
    pub flagged: Vec<Flagged>,
}

impl Run {
    /// 交给抽取器的表数 = `tables_seen - tables_excluded`。
    pub fn extracted_expected(&self) -> usize {
        self.report.tables.len()
    }

    /// 陈旧排除的**警告正文**（stderr 用）。返回 `None` = 没有陈旧项。
    ///
    /// 文案必须能让人直接动手：点名表 + 说清两种可能（表被删了 / 名字写错了）。
    pub fn stale_warning(&self, file: &str) -> Option<String> {
        if self.stale.is_empty() {
            return None;
        }
        Some(format!(
            "⚠ 陈旧排除 {} 条（{file} 里点名、但这份 schema 里没有）：{}\n\
             \x20 两种可能：① 源库已删掉/改名了这些表（排除该跟着删）；\
             ② 表名写错了（这条排除**从来没生效过**）。\n\
             \x20 默认只警告并要求你处理；要让陈旧排除直接失败，加 --strict-exclusions。",
            self.stale.len(),
            self.stale.join("、")
        ))
    }
}

// ══════════════════════════════════════════════════════════════
// 溯源记录（提案 inputs.exclusions / 数据库 / 审核界面共用）
// ══════════════════════════════════════════════════════════════

/// **本轮排除的可查记录** —— AC-004（溯源）与 AC-006（界面可见）读的同一份。
///
/// # 为什么它比 `{file, version, sha256}` 多带 `tables` / `stale`
///
/// AC-004 只要求提案元信息记三样（`file` / `version` / `sha256`）。
/// 但如果**只**记这三样，审核界面要显示「被排除的表 + 理由」就只能
/// 回头再解析一遍 `exclusions.json` —— 那会引入第二个读取点，而且
/// 「评审时打开的那个文件」与「抽取时用的那个文件」可能不是同一份。
///
/// ⇒ 把逐条记录**一起**装进这份块里：界面读的是**这一轮抽取实际排掉的**
/// 那一份（连陈旧的也记着），不可能与提案对不上。三样必需字段一个不少。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub file: String,
    /// 这份清单自己的版本号（原样，`"0.1.1"` 这种字符串）。
    pub version: String,
    pub sha256: String,
    /// 谁判的（AI 先判一轮时是 `ai-table-judge` 这类标识）。
    pub declared_by: String,
    /// 谁签的字（可选）。与 `declared_by` **分开记**：判的人与签字的人可能不同。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    pub rule: String,
    /// 真的被剔除的表（= `stats.tables_excluded` 条，逐条带理由与决策人）。
    pub tables: Vec<Entry>,
    /// 文件里点名、但这份 schema 里没有的表（R4）。
    #[serde(default)]
    pub stale: Vec<String>,
    /// 「保留但存疑」—— 表**照常进本体**，这里只是留痕。
    ///
    /// ⛔ 与 [`Self::tables`] **语义相反**：前者是「已排除」，这是「还没决定」。
    /// 两栏合并会让「排除了 N 张」与「有 M 张要人看」变成同一个列表。
    #[serde(default)]
    pub flagged: Vec<Flagged>,
}

impl Record {
    /// 从提案的 `inputs` 块里读排除记录。
    ///
    /// 三种结果分别对应三件不同的事，**不能合并**：
    /// - `Ok(None)`：这份提案**没有**排除记录（人工修订 / REQ-113 之前的提案）；
    /// - `Ok(Some(record))`：有，且合规；
    /// - `Err(..)`：文件里有 `inputs.exclusions` 但读不懂 ⇒ 报错。
    ///   **不许**降级成「没有记录」：那会让一份被改坏的溯源看起来像「本来就没有」。
    pub fn from_inputs(inputs: &serde_json::Value) -> Result<Option<Self>> {
        let Some(block) = inputs.get("exclusions").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let rec: Record = serde_json::from_value(block.clone())
            .context("提案 inputs.exclusions 读不懂（期望 {file, version, sha256, declared_by, rule, tables, stale, flagged}）")?;
        rec.validate()?;
        Ok(Some(rec))
    }

    /// 校验一份**已落盘 / 已落库**的记录（与文件入口同一套判据）。
    ///
    /// 为什么要有第二道：`propose import` 收的是**别人给的 JSON**，
    /// 审核界面读的是**库里那一行**。两处都可能被人改过，
    /// 而「被排除的表」正是评审要据以推翻结论的东西 —— 它不能只是「看起来像」。
    pub fn validate(&self) -> Result<()> {
        if self.file.trim().is_empty() {
            bail!("排除记录缺 `file`（这份排除来自哪份文件）");
        }
        if self.version.trim().is_empty() {
            bail!("排除记录缺 `version`（这份清单是哪一代）");
        }
        if !is_sha256_hex(&self.sha256) {
            bail!(
                "排除记录的 sha256 = {:?} 不是 64 位小写十六进制 —— \
                 溯源哈希不可解析时，「改一个字节 sha 就变」这条判据就失效了",
                self.sha256
            );
        }
        if self.declared_by.trim().is_empty() {
            bail!("排除记录缺 `declared_by`（谁声明了这份清单）");
        }
        if self.rule.trim().is_empty() {
            bail!("排除记录缺 `rule`（这份文件按什么规则排除）");
        }
        for (i, e) in self.tables.iter().enumerate() {
            let n = i + 1;
            if e.table.trim().is_empty() {
                bail!("排除记录第 {n} 条缺表名");
            }
            if e.reason.trim().chars().count() < REASON_MIN {
                bail!(
                    "排除记录第 {n} 条（{}）的 reason 太短：排除必须有**业务能读懂**的理由（AC-003）",
                    e.table
                );
            }
            if e.decided_by.trim().is_empty() {
                bail!("排除记录第 {n} 条（{}）缺 `decided_by`：谁决定的必须可追（AC-003）", e.table);
            }
            if !e.confidence_is_valid() {
                bail!(
                    "排除记录第 {n} 条（{}）的 confidence = {:?} 不在 0.0–1.0 之间",
                    e.table,
                    e.confidence
                );
            }
            // 同一张表既「已排除」又「保留待裁决」是自相矛盾 —— 落库的那份也要挡住。
            if self.flagged.iter().any(|f| f.table == e.table) {
                bail!(
                    "排除记录第 {n} 条（{}）同时出现在 `tables` 与 `flagged`：\
                     一个说「已排除」一个说「保留待裁决」，两句话不能同时为真",
                    e.table
                );
            }
        }
        for (i, f) in self.flagged.iter().enumerate() {
            if f.table.trim().is_empty() {
                bail!("排除记录的 flagged 第 {} 条缺表名", i + 1);
            }
            if f.why.trim().is_empty() {
                bail!("排除记录的 flagged 第 {} 条（{}）缺 `why`（为什么拿不准）", i + 1, f.table);
            }
        }
        Ok(())
    }

    /// 逐条数 —— 这就是 `stats.tables_excluded` / 审核界面上那个「被排除 N 张」。
    ///
    /// **它是唯一的算法**：不另存一个 `count` 字段。
    /// 存了就一定会漂（改表不改数、改数不改表），而这里的两处显示必须以同一份为准。
    pub fn count(&self) -> usize {
        self.tables.len()
    }

    /// 审核界面用的 JSON 块（**只读**）。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "file": self.file,
            "version": self.version,
            "sha256": self.sha256,
            "declared_by": self.declared_by,
            "reviewed_by": self.reviewed_by,
            "rule": self.rule,
            "count": self.count(),
            "tables": self.tables.iter().map(|e| serde_json::json!({
                "table": e.table,
                "category": e.category.as_str(),
                "category_label": e.category.label(),
                "reason": e.reason,
                "decided_by": e.decided_by,
                // 可选：AI 先判一轮时的那份置信度。`null` = 没给（不是 0 分）。
                "confidence": e.confidence,
                // 可选：判定依据 —— 评审人要「推翻」一条排除，靠的是依据不是结论。
                "evidence": e.evidence,
            })).collect::<Vec<_>>(),
            "stale": self.stale,
            // ⛔ 与 `tables` **分开**的一栏：这些表**进了本体**，只是「保留但存疑」。
            "flagged_count": self.flagged.len(),
            "flagged": self.flagged.iter().map(|f| serde_json::json!({
                "table": f.table,
                "why": f.why,
                "concern": f.concern,
            })).collect::<Vec<_>>(),
            // AC-006：⛔ 不许可编辑（改排除要改文件重跑）。这一栏是给页面用的
            // 显式声明，页面据此**不渲染**任何按钮 —— 没有写接口，也没有「改一下试试」。
            "readonly": true,
        })
    }
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// **`propose import` 用的那道门**：读提案旁边的 `proposal-exclusions.json`，
/// 并与提案里记的 `inputs.exclusions` **逐项对齐**。
///
/// # 为什么必须校验 sha256（而不是「文件在就读」）
///
/// 「记录文件」和「提案」是两个可以被人分开放、分开拷贝的东西：
/// 拷错一轮、拿旧记录配新提案，都会让审核界面显示**上一轮**的排除清单，
/// 而页面上看不出任何异常。所以绑定用内容哈希：
/// `record.sha256 == inputs.exclusions.sha256`（那是**排除文件原文**的哈希）。
///
/// # 三种失败都报人话（不降级成「没有记录」）
///
/// | 情况 | 处理 |
/// | --- | --- |
/// | 提案有 `inputs.exclusions`，但记录文件不在 | 报错：**没有它，AC-006 显示不了「排了谁、为什么」** |
/// | 记录文件读不懂 | 报错（serde 上下文） |
/// | 记录与提案不是一个 `sha256` | 报错，并打印两个 sha 让人能对上是哪一轮错位 |
pub fn load_record(record_path: &std::path::Path, inputs: &serde_json::Value) -> Result<Record> {
    let block = inputs.get("exclusions").filter(|v| !v.is_null()).ok_or_else(|| {
        anyhow::anyhow!(
            "提案的 `inputs` 里没有 `exclusions` —— 调用方不该在没有记录时调本函数（内部契约错）"
        )
    })?;
    let want_sha = block.get("sha256").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if want_sha.is_empty() {
        bail!("提案的 `inputs.exclusions` 缺 `sha256`：没有它就绑不住记录文件是哪一轮的");
    }

    let raw = std::fs::read(record_path).with_context(|| {
        format!(
            "提案声明了排除记录，但 {record_path:?} 不在提案旁边。\n\
             \n\
             没有这份逐条记录，审核界面只能显示一个 sha256 —— 而 REQ-113 的承诺是\n\
             「**任何一张表被排除，都必须有理由、有记录、能被评审人看见**」（AC-006）。\n\
             \n\
             怎么办：重跑一次 `ontok-propose --exclude <文件>`（它会与提案一起写出这份记录），\n\
             再把**这一对文件**一起 import。⛔ 不要手搓一份记录文件：它必须与 `sha256` 同源。"
        )
    })?;
    let rec: Record = serde_json::from_slice(&raw).with_context(|| {
        format!("{record_path:?} 不是合法的排除记录（期望 {RECORD_FILE} 的形状）")
    })?;
    rec.validate()?;
    if rec.sha256 != want_sha {
        bail!(
            "提案与记录文件不是同一轮：\n  提案记的 exclusions sha256 = {want_sha}\n  \
             记录文件里的 sha256          = {}\n\
             这条绑定正是为了防止「拿上一轮的记录配这一轮的提案」—— 那种错位在页面上看不出来。",
            rec.sha256
        );
    }
    Ok(rec)
}

// ══════════════════════════════════════════════════════════════
// R5 · 三计数的**唯一**写入点 + 闭合断言（AC-002）
// ══════════════════════════════════════════════════════════════

use crate::extract::propose::ExtractionStats;

impl ExtractionStats {
    /// 落下 `tables_seen` / `tables_excluded` / `tables_extracted` 三个计数并**当场断言闭合**。
    ///
    /// # 为什么是「断言」而不是「三个数各自赋值」
    ///
    /// `seen - excluded == extracted` 若只是三行赋值，那它就只是一句**注释**：
    /// 有人改了一处、漏了另一处，stats.json 会安静地写出两个对不上的数，
    /// 而这份文件正是判读「N 张是怎么来的」的唯一凭据。
    /// 所以这里：
    ///
    /// 1. `tables_excluded` / `tables_extracted` **只**在这一个函数里被写；
    /// 2. `extracted` 来自**实测**（提案里 `add_entity_type` 的条数），
    ///    不是由 `seen - excluded` 反推 —— 反推出来的差永远等于差，就不可能发现
    ///    「某张表被静默吃掉了」；
    /// 3. 减法不闭合 ⇒ **返回 Err**，调用方因此**不写出 stats.json**。
    ///
    /// 第 2 条是关键：两张数各自独立取值，等式才携带信息。
    pub fn stamp_counts(
        &mut self,
        seen: usize,
        excluded: &[Entry],
        stale: &[String],
        flagged: &[Flagged],
        extracted: usize,
    ) -> Result<()> {
        let expected = seen.checked_sub(excluded.len()).ok_or_else(|| {
            anyhow::anyhow!(
                "排除数 {} 比勘查到的表数 {seen} 还多 —— 这在构造上不可能，\
                 说明 `--exclude` 与 schema 不是同一轮的输入（拒绝写出对不上的统计）",
                excluded.len()
            )
        })?;

        self.tables_seen = seen;
        self.tables_excluded = excluded.len();
        self.tables_extracted = extracted;
        self.excluded_tables = excluded.to_vec();
        self.stale_exclusions = stale.to_vec();
        self.flagged_tables = flagged.to_vec();

        if expected != extracted {
            bail!(
                "统计不自洽（AC-002）：tables_seen {seen} − tables_excluded {} = {expected}，\
                 但真的产出实体的只有 {extracted} 张表。\n\
                 \n\
                 差 {} 张 —— 有表**被静默吃掉了**：它既没被排除，也没被抽出来。\n\
                 这正是 R5 要防的那件事（「数字对不上但没人知道为什么」），\
                 所以这里**不写出 stats.json**，而不是写一份能看但错的统计。\n\
                 \n\
                 先查两处：① 这次的 schema 与 exclusions 是不是同一轮的（陈旧排除会走另一条路）；\
                 ② 抽取路径是不是在哪儿按 key 去重/丢了表。",
                excluded.len(),
                seen - extracted - excluded.len()
            );
        }
        Ok(())
    }

    /// 三计数是否闭合（判据与 [`Self::stamp_counts`] 同一句，供测试与守卫直接断言）。
    pub fn counts_close(&self) -> bool {
        self.tables_seen
            .checked_sub(self.tables_excluded)
            .map(|n| n == self.tables_extracted)
            .unwrap_or(false)
    }
}

// ══════════════════════════════════════════════════════════════
// 测试
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::introspect::{ColumnMeta, TableMeta};

    fn col(name: &str, comment: Option<&str>) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            data_type: "text".into(),
            nullable: true,
            comment: comment.map(str::to_string),
            is_primary_key: name == "id",
        }
    }

    fn table(name: &str, comment: Option<&str>, cols: usize) -> TableMeta {
        let columns: Vec<ColumnMeta> =
            (0..cols).map(|i| col(&format!("c{i}"), if i == 0 { comment } else { None })).collect();
        TableMeta {
            name: name.into(),
            comment: comment.map(str::to_string),
            column_count: columns.len(),
            commented_count: columns.iter().filter(|c| c.comment.is_some()).count(),
            columns,
        }
    }

    fn report(tables: Vec<TableMeta>) -> SchemaReport {
        SchemaReport {
            server_version: "PostgreSQL 16.14".into(),
            transaction_read_only: "on".into(),
            schema: "public".into(),
            table_count: tables.len(),
            column_count: tables.iter().map(|t| t.columns.len()).sum(),
            commented_column_count: tables.iter().map(|t| t.commented_count).sum(),
            foreign_key_count: 0,
            tables,
            introspected_at: "2026-10-21T00:00:00Z".into(),
        }
    }

    fn file_json(entries: &str) -> String {
        format!(
            r#"{{"version":1,"rule":"测试规则","declared_by":"QA","exclude":[{entries}]}}"#
        )
    }

    fn one(table: &str) -> String {
        format!(
            r#"{{"table":"{table}","category":"cdc_residue","reason":"CDC 同步残留表，不属于业务模型","decided_by":"BA 2026-10-21"}}"#
        )
    }

    // ── R2 / AC-003 · 缺理由 / 类别非法 / 缺决策人 ⇒ 拒绝文件 ────────────

    #[test]
    fn test_empty_reason_is_rejected_and_the_message_quotes_the_rule() {
        let raw = r#"{"version":1,"rule":"r","declared_by":"BA","exclude":[
            {"table":"t1","category":"cdc_residue","reason":"   ","decided_by":"BA"}
        ]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x.json").unwrap_err().to_string();
        assert!(e.contains("reason"), "报错必须点名字段：{e}");
        assert!(e.contains("缺 `exclude[].reason`"), "空白串与缺失同义：{e}");
    }

    #[test]
    fn test_too_short_reason_is_rejected() {
        // 「技术原因」这类等于没说的词挡不住（那是评审的事），
        // 但一个字符的「x」必须挡 —— 它连一句完整的话都不是。
        let raw = r#"{"version":1,"rule":"r","declared_by":"BA","exclude":[
            {"table":"t1","category":"cdc_residue","reason":"x","decided_by":"BA"}
        ]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x.json").unwrap_err().to_string();
        assert!(e.contains("太短"), "{e}");
        assert!(e.contains("业务"), "文案要指向「业务能读懂」：{e}");
    }

    #[test]
    fn test_unknown_category_is_rejected_and_lists_every_legal_value() {
        let raw = r#"{"version":1,"rule":"r","declared_by":"BA","exclude":[
            {"table":"t1","category":"tech_reason","reason":"这是随便编的类别","decided_by":"BA"}
        ]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x.json").unwrap_err().to_string();
        for c in Category::ALL {
            assert!(e.contains(c.as_str()), "报错必须列全合法类别（缺 {}）：{e}", c.as_str());
        }
    }

    #[test]
    fn test_missing_decided_by_is_rejected() {
        let raw = r#"{"version":1,"rule":"r","declared_by":"BA","exclude":[
            {"table":"t1","category":"cdc_residue","reason":"CDC 同步残留表，不属于业务模型"}
        ]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x.json").unwrap_err().to_string();
        assert!(e.contains("decided_by"), "{e}");
    }

    #[test]
    fn test_missing_or_empty_version_is_rejected() {
        let no_v = r#"{"rule":"r","declared_by":"BA","exclude":[]}"#;
        let e = Exclusions::parse(no_v.as_bytes(), "x").unwrap_err().to_string();
        assert!(e.contains("version"), "{e}");

        let empty = r#"{"version":"  ","rule":"r","declared_by":"BA","exclude":[]}"#;
        let e = Exclusions::parse(empty.as_bytes(), "x").unwrap_err().to_string();
        assert!(e.contains("空串"), "{e}");

        // 非标量（对象）不是版本号
        let obj = r#"{"version":{"major":1},"rule":"r","declared_by":"BA","exclude":[]}"#;
        assert!(Exclusions::parse(obj.as_bytes(), "x").is_err());
    }

    #[test]
    fn test_semver_string_and_legacy_number_versions_are_both_accepted() {
        // 真实清单（AI 先判那一轮）用的是 "0.1.1"；早期手写的是 1。
        let s = r#"{"version":"0.1.1","rule":"r","declared_by":"BA","exclude":[
            {"table":"t1","category":"cdc_residue","reason":"CDC 同步残留表，不属于业务模型","decided_by":"BA"}
        ]}"#;
        assert_eq!(Exclusions::parse(s.as_bytes(), "x").unwrap().version(), "0.1.1");
        let n = file_json(&one("t1"));
        assert_eq!(Exclusions::parse(n.as_bytes(), "x").unwrap().version(), "1");
    }

    #[test]
    fn test_empty_exclude_list_is_rejected_not_silently_zero() {
        let raw = r#"{"version":1,"rule":"r","declared_by":"BA","exclude":[]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x").unwrap_err().to_string();
        assert!(e.contains("空数组"), "{e}");
    }

    #[test]
    fn test_duplicate_table_is_rejected_because_it_makes_two_counts() {
        let raw = file_json(&format!("{},{}", one("t1"), one("t1")));
        let e = Exclusions::parse(raw.as_bytes(), "x").unwrap_err().to_string();
        assert!(e.contains("两次"), "{e}");
    }

    // ── AC-004 · sha256 溯源 ─────────────────────────────────────────

    #[test]
    fn test_sha256_is_stable_and_changes_on_a_single_byte() {
        let raw = file_json(&one("t1"));
        let a = Exclusions::parse(raw.as_bytes(), "x.json").unwrap();
        let b = Exclusions::parse(raw.as_bytes(), "y.json").unwrap();
        assert_eq!(a.sha256(), b.sha256(), "同一份文件（哪怕路径不同）sha 必须相同");
        assert!(is_sha256_hex(a.sha256()));

        let mutated = raw.replace("CDC", "CDD");
        let c = Exclusions::parse(mutated.as_bytes(), "x.json").unwrap();
        assert_ne!(a.sha256(), c.sha256(), "改一个字节 ⇒ sha256 必须变");
        assert_eq!(a.file(), "x.json", "file 记的是调用方给的路径，不是内容");
    }

    // ── 剔除 / 陈旧 / 计数 ────────────────────────────────────────────

    #[test]
    fn test_apply_removes_tables_and_keeps_report_counts_consistent() {
        let rep = report(vec![table("a", Some("甲"), 3), table("noise", None, 2), table("b", None, 4)]);
        let raw = file_json(&one("noise"));
        let ex = Exclusions::parse(raw.as_bytes(), "x.json").unwrap();
        let run = ex.apply(&rep);

        assert_eq!(run.report.tables.len(), 2, "剔掉 1 张");
        assert_eq!(run.report.table_count, 2, "作用域计数必须跟着变");
        assert_eq!(run.report.column_count, 7, "3 + 4 列");
        assert_eq!(run.report.commented_column_count, 1);
        assert_eq!(run.report.tables[0].name, "a", "顺序必须原样保留");
        assert_eq!(run.report.tables[1].name, "b");
        assert!(run.stale.is_empty());
        assert_eq!(run.extracted_expected(), 2);
        // 勘查事实不动（排除改作用域，不改历史）
        assert_eq!(run.report.introspected_at, rep.introspected_at);
        assert_eq!(run.report.server_version, rep.server_version);
    }

    #[test]
    fn test_stale_entries_are_named_and_do_not_shrink_the_scope() {
        let rep = report(vec![table("a", None, 1), table("b", None, 1)]);
        let raw = file_json(&format!("{},{}", one("a"), one("gone_table")));
        let ex = Exclusions::parse(raw.as_bytes(), "x.json").unwrap();
        let run = ex.apply(&rep);

        assert_eq!(run.applied.len(), 1, "只有真的存在的表被剔除");
        assert_eq!(run.stale, vec!["gone_table".to_string()]);
        assert_eq!(run.report.tables.len(), 1);
        let warn = run.stale_warning(ex.file()).expect("陈旧必须有警告");
        assert!(warn.contains("gone_table"), "警告必须点名：{warn}");
        assert!(warn.contains("--strict-exclusions"), "警告要告诉人怎么让它失败：{warn}");
    }

    #[test]
    fn test_no_stale_means_no_warning() {
        let rep = report(vec![table("a", None, 1)]);
        let ex = Exclusions::parse(file_json(&one("a")).as_bytes(), "x.json").unwrap();
        assert!(ex.apply(&rep).stale_warning(ex.file()).is_none());
    }

    #[test]
    fn test_applied_and_stale_follow_file_order_not_schema_order() {
        let rep = report(vec![table("a", None, 1), table("b", None, 1), table("c", None, 1)]);
        // 文件顺序故意与 schema 顺序相反
        let raw = file_json(&format!("{},{},{}", one("c"), one("a"), one("zzz")));
        let ex = Exclusions::parse(raw.as_bytes(), "x.json").unwrap();
        let run = ex.apply(&rep);
        assert_eq!(
            run.applied.iter().map(|e| e.table.as_str()).collect::<Vec<_>>(),
            vec!["c", "a"],
            "逐条记录按文件顺序 —— 评审时才能与文件一行行对上"
        );
        assert_eq!(run.stale, vec!["zzz".to_string()]);
    }

    #[test]
    fn test_apply_is_deterministic_across_two_identical_inputs() {
        let rep = report(vec![table("a", None, 2), table("n1", None, 1), table("n2", None, 1)]);
        let raw = file_json(&format!("{},{}", one("n1"), one("n2")));
        let ex1 = Exclusions::parse(raw.as_bytes(), "x.json").unwrap();
        let ex2 = Exclusions::parse(raw.as_bytes(), "x.json").unwrap();
        let j = |r: &Run| {
            serde_json::to_string(&(
                &r.report.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
                &r.applied,
                &r.stale,
                r.report.table_count,
            ))
            .unwrap()
        };
        assert_eq!(j(&ex1.apply(&rep)), j(&ex2.apply(&rep)));
    }

    // ── AC-002 / R5 · 三计数闭合 ─────────────────────────────────────

    #[test]
    fn test_stamp_counts_closes_and_records_every_exclusion() {
        let rep = report(vec![table("a", None, 1), table("b", None, 1), table("noise", None, 1)]);
        let ex = Exclusions::parse(file_json(&one("noise")).as_bytes(), "x.json").unwrap();
        let run = ex.apply(&rep);

        let mut stats = ExtractionStats::default();
        stats
            .stamp_counts(
                rep.tables.len(),
                &run.applied,
                &run.stale,
                &run.flagged,
                run.report.tables.len(),
            )
            .expect("96 - 13 == 83 必须成立");

        assert_eq!(stats.tables_seen, 3);
        assert_eq!(stats.tables_excluded, 1);
        assert_eq!(stats.tables_extracted, 2);
        assert!(stats.counts_close());
        assert_eq!(stats.excluded_tables.len(), 1, "逐条记录：表名 / 类别 / 理由 / 决策人");
        assert_eq!(stats.excluded_tables[0].table, "noise");
        assert_eq!(stats.excluded_tables[0].category, Category::CdcResidue);
        assert!(stats.excluded_tables[0].reason.contains("CDC"));
        assert!(!stats.excluded_tables[0].decided_by.is_empty());
    }

    #[test]
    fn test_stamp_counts_refuses_when_a_table_vanishes_silently() {
        let rep = report(vec![table("a", None, 1), table("b", None, 1), table("c", None, 1)]);
        let ex = Exclusions::parse(file_json(&one("c")).as_bytes(), "x.json").unwrap();
        let run = ex.apply(&rep);

        let mut stats = ExtractionStats::default();
        // 实测只抽出 1 张（模拟「还有一张被静默吃掉」）
        let e = stats
            .stamp_counts(rep.tables.len(), &run.applied, &run.stale, &run.flagged, 1)
            .expect_err("3 - 1 != 1 ⇒ 必须报错，不许写出对不上的统计");
        let msg = e.to_string();
        assert!(msg.contains("统计不自洽"), "{msg}");
        assert!(msg.contains("静默"), "文案要指出这是「被静默吃掉」：{msg}");
    }

    #[test]
    fn test_stamp_counts_rejects_more_exclusions_than_tables() {
        let ex = Exclusions::parse(
            file_json(&format!("{},{}", one("a"), one("b"))).as_bytes(),
            "x.json",
        )
        .unwrap();
        let mut stats = ExtractionStats::default();
        assert!(stats.stamp_counts(1, ex.entries(), &[], &[], 0).is_err());
    }

    // ── AC-007 / R6 · 排除优先于「无中文名」 ──────────────────────────

    #[test]
    fn test_excluded_table_cannot_also_be_without_cn_name() {
        // 一张**既**没中文名（无注释、翻译表里也没有）**又**被排除的表。
        let rep = report(vec![table("a", Some("甲"), 1), table("nameless", None, 2)]);
        let ex = Exclusions::parse(file_json(&one("nameless")).as_bytes(), "x.json").unwrap();
        let run = ex.apply(&rep);

        // 排除后的报告里已经没有它 ⇒ 抽取器根本见不到它 ⇒ 不可能进
        // `tables_without_cn_name`。这不是「记得跳过」，是**结构上不可达**。
        assert!(
            !run.report.tables.iter().any(|t| t.name == "nameless"),
            "被排除的表必须不在抽取作用域里（否则它会以「无中文名」的身份进本体）"
        );
        assert_eq!(run.applied.len(), 1);
    }

    // ── 记录块（inputs.exclusions / 审核界面）────────────────────────

    #[test]
    fn test_record_round_trips_through_inputs_and_counts_match() {
        let rep = report(vec![table("a", None, 1), table("n1", None, 1), table("n2", None, 1)]);
        let ex = Exclusions::parse(
            file_json(&format!("{},{}", one("n1"), one("n2"))).as_bytes(),
            "exclusions.json",
        )
        .unwrap();
        let run = ex.apply(&rep);
        let rec = ex.record(&run);

        let inputs = serde_json::json!({ "exclusions": rec });
        let back = Record::from_inputs(&inputs).unwrap().expect("有记录");
        assert_eq!(back.count(), 2, "条数 = tables_excluded");
        assert_eq!(back.sha256, rec.sha256);
        assert_eq!(back.file, "exclusions.json");

        let ui = back.to_json();
        assert_eq!(ui["count"], 2);
        assert_eq!(ui["readonly"], true, "AC-006：只读，页面不渲染任何编辑入口");
        assert_eq!(ui["tables"][0]["category"], "cdc_residue");
        assert_eq!(ui["tables"][0]["category_label"], Category::CdcResidue.label());
        assert!(ui["tables"][0]["reason"].as_str().unwrap().contains("CDC"));
        assert!(!ui["tables"][0]["decided_by"].as_str().unwrap().is_empty());
    }

    #[test]
    fn test_record_without_exclusions_block_is_none_not_error() {
        // 人工修订 / REQ-113 之前的提案：没有 inputs.exclusions，**不是错误**。
        assert!(Record::from_inputs(&serde_json::json!({})).unwrap().is_none());
        assert!(Record::from_inputs(&serde_json::json!({"exclusions": null})).unwrap().is_none());
    }

    #[test]
    fn test_broken_record_is_an_error_not_silently_absent() {
        let bad = serde_json::json!({"exclusions": {"file": "x", "version": "1", "sha256": "nothex",
            "declared_by": "BA", "rule": "r", "tables": []}});
        let e = Record::from_inputs(&bad).unwrap_err().to_string();
        assert!(e.contains("sha256"), "{e}");
    }

    #[test]
    fn test_record_with_empty_reason_is_rejected_on_read() {
        let bad = serde_json::json!({"exclusions": {
            "file": "x.json", "version": "1", "sha256": "0".repeat(64),
            "declared_by": "BA", "rule": "r",
            "tables": [{"table":"t1","category":"cdc_residue","reason":"  ","decided_by":"BA"}],
            "stale": []
        }});
        let e = Record::from_inputs(&bad).unwrap_err().to_string();
        assert!(e.contains("reason"), "{e}");
    }

    // ── 「保留但存疑」与「排除」**分开记** ────────────────────────────

    /// 一份带 `keep_but_flagged` / `reviewed_by` / `confidence` / `evidence` 的文件
    /// —— 与 AI 先判一轮产出的那份同形。
    fn ai_like_file() -> String {
        r#"{
          "version": "0.1.1",
          "declared_by": "ai-table-judge / fake-model",
          "rule": "只排除不是客户业务世界里的事物；存疑一律保留并标注",
          "reviewed_by": "ba-michael（人工复核：推翻 1 条）",
          "exclude": [
            {"table":"noise","category":"cdc_residue","reason":"数据同步工具自动生成的临时 DDL 表，不对应任何客户业务","decided_by":"ai-table-judge","confidence":0.97,"evidence":"表名 _volc_dts_ 前缀"},
            {"table":"alembic_version","category":"migration_bookkeeping","reason":"数据库迁移工具的版本号记账表，客户业务里没有对应事物","decided_by":"ai-table-judge","confidence":0.99}
          ],
          "keep_but_flagged": [
            {"table":"internal_staff","why":"名字像内部表，但列语义不足以判断","concern":"若只是后台账号则与 admin_users 重复"},
            {"table":"trial_records","why":"只有 4 列，更像试用请求去重台账","concern":"若客户会查「谁试用过什么」就是业务事实"}
          ],
          "keep_normal": ["a", "b"]
        }"#
        .to_string()
    }

    #[test]
    fn test_flagged_is_parsed_and_kept_but_never_excluded() {
        let rep = report(vec![table("noise", None, 1), table("internal_staff", None, 1), table("a", None, 1)]);
        let ex = Exclusions::parse(ai_like_file().as_bytes(), "ai.json").unwrap();
        assert_eq!(ex.version(), "0.1.1");
        assert_eq!(ex.reviewed_by(), Some("ba-michael（人工复核：推翻 1 条）"));
        assert_eq!(ex.flagged().len(), 2);

        let run = ex.apply(&rep);
        assert_eq!(run.applied.len(), 1, "只有 exclude 里的那张被剔");
        assert_eq!(run.flagged.len(), 2, "flagged 原样带走");
        // ⛔ 关键：被标「待裁决」的表**照常进作用域**（它没有被排除）。
        assert!(run.report.tables.iter().any(|t| t.name == "internal_staff"));
        assert_eq!(run.report.tables.len(), 2);
    }

    #[test]
    fn test_flagged_is_a_separate_field_from_excluded_in_stats_and_ui() {
        let rep = report(vec![table("noise", None, 1), table("internal_staff", None, 1)]);
        let ex = Exclusions::parse(ai_like_file().as_bytes(), "ai.json").unwrap();
        let run = ex.apply(&rep);

        let mut stats = ExtractionStats::default();
        stats
            .stamp_counts(2, &run.applied, &run.stale, &run.flagged, 1)
            .unwrap();
        assert_eq!(stats.tables_excluded, 1);
        assert_eq!(stats.excluded_tables.len(), 1);
        assert_eq!(stats.flagged_tables.len(), 2, "待裁决单独一栏");

        let ui = ex.record(&run).to_json();
        assert_eq!(ui["count"], 1, "「被排除 N 张」只数 exclude");
        assert_eq!(ui["flagged_count"], 2);
        assert_eq!(ui["flagged"][0]["table"], "internal_staff");
        assert!(ui["flagged"][0]["why"].as_str().unwrap().contains("列语义"));
        // 两栏互不包含：待裁决的表不在 tables 里，排除的表不在 flagged 里
        let names: Vec<&str> = ui["tables"].as_array().unwrap().iter().map(|t| t["table"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["noise"]);
        assert!(!names.contains(&"internal_staff"));
    }

    #[test]
    fn test_confidence_and_evidence_reach_the_review_ui() {
        let rep = report(vec![table("noise", None, 1)]);
        let ex = Exclusions::parse(ai_like_file().as_bytes(), "ai.json").unwrap();
        let ui = ex.record(&ex.apply(&rep)).to_json();
        assert_eq!(ui["tables"][0]["confidence"], 0.97);
        assert!(ui["tables"][0]["evidence"].as_str().unwrap().contains("_volc_dts_"));
        assert_eq!(ui["reviewed_by"], "ba-michael（人工复核：推翻 1 条）");
    }

    #[test]
    fn test_a_table_cannot_be_both_excluded_and_flagged() {
        let raw = r#"{"version":"1","rule":"r","declared_by":"BA",
            "exclude":[{"table":"t1","category":"cdc_residue","reason":"数据同步残留表，不属于业务模型","decided_by":"BA"}],
            "keep_but_flagged":[{"table":"t1","why":"拿不准"}]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x").unwrap_err().to_string();
        assert!(e.contains("同时出现"), "{e}");
    }

    #[test]
    fn test_out_of_range_confidence_is_rejected() {
        let raw = r#"{"version":"1","rule":"r","declared_by":"BA","exclude":[
            {"table":"t1","category":"cdc_residue","reason":"数据同步残留表，不属于业务模型","decided_by":"BA","confidence":1.5}
        ]}"#;
        let e = Exclusions::parse(raw.as_bytes(), "x").unwrap_err().to_string();
        assert!(e.contains("0.0–1.0"), "{e}");
    }
}
