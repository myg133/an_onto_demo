//! **证据评分卡 v1**（`evidence-scorecard-v1`）—— 纯函数，零 I/O（REQ-108 · AC-004 / AC-005）
//!
//! 把「这条推断关系有多可信」变成**一个可复算的数**：五个信号加权，
//! 再由 **veto 上限**压顶，最后按 0.90 / 0.60 / 0.30 分四档。
//!
//! # 为什么这个文件必须**纯**
//!
//! AC-005 要求「同一 schema + 同一数据跑两次，`score` **逐位相同**」。
//! 靠约定做不到 —— 只要这里能读时间、能取随机数、能查库，就总有一条路径会漂。
//! 所以本模块**没有** `use std::time`、没有随机源、没有任何 I/O；
//! 数据信号由调用方（`extract::sample`）读好后**作为参数**传进来。
//! 判据是结构式的：输入相同 ⇒ 输出逐位相同，不需要「记得别乱来」。
//!
//! # 为什么 veto 优先于加权和
//!
//! 五个信号按权重加起来很容易「救回」一条铁错的关系：
//! `name_match = 1.0` + `target_is_key = 1.0` + `type_compatible = 1.0`
//! 已经把 0.40 的结构分拿满，再加一点点别的就过 0.60 了。
//! 而「类型不相容」或「5 行 0 命中」是**反驳**，不是「扣分项」：
//! 加权和只能让分数变小，不能让它**不超过**某个上限。所以：
//!
//! ```text
//! 基线分 = Σ(wᵢ·sᵢ) / Σ(wᵢ)        ← 只对「权重保留且值可得」的信号归一化
//! 最终分 = min(基线分, 所有触发 veto 的上限中的最小值)
//! ```
//!
//! AC-004 的判据正是构造「把 name_match / target_is_key 人为置 1.0」的样本，
//! 断言分数仍被上限压住 —— 这条在 [`tests::vetoes_survive_maxed_structural_signals`]
//! 里逐类执行。
//!
//! # 权重为什么是这样（AC-013 第 1 项要求把理由写出来）
//!
//! | 信号 | 权重 | 依据 |
//! | --- | --- | --- |
//! | `sample_hit_rate` | **0.45** | 实测：零外键库上「列名推断」只有 58% 准确（`demand.md`），数据证据是唯一能推翻列名猜测的东西 |
//! | `name_match` | 0.20 | 名字仍然有用（`customer_id → customers` 基本不会错），但不能主导 —— 泛化名（`batch_id`）实测 0/5 |
//! | `not_all_null` | 0.15 | 「源列全空」会让命中率失去意义，必须先判；但它是**弱**信号（空列不等于关系错） |
//! | `type_compatible` | 0.10 | 类型不相容是**硬反驳**，已由 veto 兜住；信号本身只做微调，权重给低避免「类型对=可信」的错觉 |
//! | `target_is_key` | 0.10 | 目标是主键/唯一列说明「可被引用」，是**结构合理性**，不是数据证据 |
//!
//! 数据证据合计 0.60、结构证据 0.40 —— 与设计概要 §3.2 一致。
//!
//! # 语义项 `semantic_fit`：v1 不接，v2 接（REQ-109）
//!
//! [`SIGNAL_SEMANTIC_FIT`] 在 v1 的权重表里**没有**（因此不参与归一化）；
//! REQ-109 新增 [`METHOD_V2`] + [`WEIGHTS_V2`]，语义项权重 **0.15**，
//! 其余五项**等比压缩 0.85**（0.45→0.3825 等）。
//!
//! 为什么是「等比压缩」而不是「加一项再归一化」：等比压缩保证**语义缺失时**
//! 重归一化后的相对比例与 v1 **完全一致** ⇒ AI 关（仍走 v1）的产物逐字节不变
//! （AC-001），AI 开但某条没判（语义缺失）时也不会被这一次缺失改变相对权重。
//!
//! ★ 语义缺失 ≠ 0：`None` 表示「没判断」，`0.0` 表示「判断为不成立」。
//! `ScoreInput` 之外的这一位用 [`score_with_semantic`] 的 `Option<f64>` 表达，
//! `None` 直接**不进** `signals`（不参与归一化），绝不当成 0 分参与加权。

use serde::{Deserialize, Serialize};

/// 评分方法标识 —— **必填**（AC-008：没有 method 的分数不可复核）。
pub const METHOD: &str = "evidence-scorecard-v1";

/// REQ-109 · 接了 AI 语义项的评分方法（`semantic_fit` 权重 0.15）。
///
/// 与 [`METHOD`] **并列**而不是替换：AI 关时仍然产出 v1，
/// 「AI 关时行为完全不变」（AC-001）因此是**选哪个方法**的事，不是算术巧合。
pub const METHOD_V2: &str = "evidence-scorecard-v2";

/// AI 语义项的信号名。
pub const SIGNAL_SEMANTIC_FIT: &str = "semantic_fit";

/// 五个信号的权重（**顺序即信号在输出里的顺序**，确定性的一部分）。
///
/// 合计必须 = 1.0（[`tests::weights_sum_to_one_and_match_the_design_table`] 钉住）；
/// 改这里 = 升 method 版本。
pub const WEIGHTS: [(&str, f64); 5] = [
    ("sample_hit_rate", 0.45),
    ("name_match", 0.20),
    ("not_all_null", 0.15),
    ("type_compatible", 0.10),
    ("target_is_key", 0.10),
];

/// REQ-109 · v2 权重（设计概要 §3.1）：v1 五项**等比 ×0.85** + 语义 0.15。
///
/// | 信号 | v1 | v2 |
/// | --- | --- | --- |
/// | `sample_hit_rate` | 0.45 | **0.3825** |
/// | `name_match` | 0.20 | **0.17** |
/// | `not_all_null` | 0.15 | **0.1275** |
/// | `type_compatible` | 0.10 | **0.085** |
/// | `target_is_key` | 0.10 | **0.085** |
/// | `semantic_fit` | — | **0.15** |
///
/// 合计 = 1.0（单元测试钉住）。语义项在**最后** —— 与 v1 五项的相对顺序一致，
/// 于是「语义缺失 ⇒ 前五项按原顺序、原比例求和」这条性质不用额外约定。
pub const WEIGHTS_V2: [(&str, f64); 6] = [
    ("sample_hit_rate", 0.3825),
    ("name_match", 0.17),
    ("not_all_null", 0.1275),
    ("type_compatible", 0.085),
    ("target_is_key", 0.085),
    (SIGNAL_SEMANTIC_FIT, 0.15),
];

/// `method` → 它的权重表。**没有第二份**：`score` 与 `from_untrusted` 都从这里取。
///
/// 未知 method ⇒ `None`（调用方决定是「拒绝」还是「不可能」）。
/// 这是 AC-014「权重逐项相等」能覆盖两个版本的方式 ——
/// 校验拿的永远是**这个方法自己的**那张表，而不是「当前最新的那张」。
pub fn weights_for(method: &str) -> Option<&'static [(&'static str, f64)]> {
    match method {
        METHOD => Some(&WEIGHTS[..]),
        METHOD_V2 => Some(&WEIGHTS_V2[..]),
        _ => None,
    }
}

// ── veto 上限（**数值只在这里写一次**）────────────────────────────
//
// 这些常量同时被 [`score()`]（压顶）与 [`from_untrusted()`]（核对文件里的
// `veto.cap`）使用。为什么不是一个数字写在 `score()` 里、另一个数字写在
// 校验里：那样「上限表」本身就是第二个可被改的地方 —— 校验会拿文件里
// 自己声明的那条上限来检查它自己（QA 攻击 f 的同构）。
const CAP_TYPE_INCOMPATIBLE: f64 = 0.15;
const CAP_ZERO_HIT: f64 = 0.20;
const CAP_GENERIC_NAME_NO_EVIDENCE: f64 = 0.40;
const CAP_ALL_NULL: f64 = 0.45;
const CAP_PARTIAL_HIT: f64 = 0.75;
const CAP_COMPOSITE_KEY_UNSUPPORTED: f64 = 0.50;
const CAP_POLYMORPHIC_TARGET: f64 = 0.40;
const CAP_SELF_REF_NON_UNIQUE: f64 = 0.50;
const CAP_NO_DATA_EVIDENCE: f64 = 0.45;

/// `veto.signal` → 上限（设计概要 §3.4 + A-2 的第 9 条）。
///
/// **这是上限表的唯一真相**：`score()` 压顶用这些值，`from_untrusted()`
/// 核对不可信 JSON 里的 `veto.cap` 也读这张表。把文件里的 `cap` 改大
/// 救不了一条 0 命中的关系 —— 它既过不了 `cap` 一致性，也过不了
/// 「分数 ≤ 按证据推出的上限」那一条。
pub const VETO_CAPS: [(&str, f64); 9] = [
    ("type_incompatible", CAP_TYPE_INCOMPATIBLE),
    ("zero_hit", CAP_ZERO_HIT),
    ("generic_name_no_evidence", CAP_GENERIC_NAME_NO_EVIDENCE),
    ("all_null", CAP_ALL_NULL),
    ("partial_hit", CAP_PARTIAL_HIT),
    ("composite_key_unsupported", CAP_COMPOSITE_KEY_UNSUPPORTED),
    ("polymorphic_target", CAP_POLYMORPHIC_TARGET),
    ("self_ref_non_unique", CAP_SELF_REF_NON_UNIQUE),
    ("no_data_evidence", CAP_NO_DATA_EVIDENCE),
];

/// 查 veto 上限；未知信号名 ⇒ `None`（调用方决定是「拒绝」还是「不可能」）。
pub fn veto_cap(signal: &str) -> Option<f64> {
    VETO_CAPS.iter().find(|(n, _)| *n == signal).map(|(_, c)| *c)
}

/// **结构类** veto：证据在候选关系本身（复合键 / 多态列 / 自引用），
/// 不在 `confidence.signals` 里。
///
/// 所以 AC-014 #3（「veto 必须出现在 signals 里」）对它们只能核到
/// 「上限与表一致、分数不越上限」为止 —— **不是放过它们**：这三条只会
/// 把分数往下压（0.40–0.50），而能把分数抬高到「高」档的那几条
/// （0.15 / 0.20 / 0.75）全部有 `signals` 证据可核。
const STRUCTURAL_VETOES: [&str; 3] = [
    "composite_key_unsupported",
    "polymorphic_target",
    "self_ref_non_unique",
];

/// 泛化列名 —— 名字**不指向任何具体实体**，实测命中率极低（`batch_id` 0/5）。
///
/// 名单来自设计概要 §3.1，**不是**「凡是 `*_id` 都算」：
/// `customer_id` 这种直接展开成表名的列名是好证据（1.0 档），不能一起降级。
const GENERIC_COLUMNS: [&str; 13] = [
    "batch_id",
    "session_id",
    "request_id",
    "task_id",
    "group_id",
    "template_id",
    "product_id",
    "channel_id",
    "policy_id",
    "auth_id",
    "related_id",
    "business_ref_id",
    "external_task_id",
];

/// 多态列的**词干**白名单：`target_id` + `target_type` 同表并存时，
/// 「`target_id` 指向哪张表」由 `target_type` 决定 ⇒ 单列推断不成立（R3）。
const POLYMORPHIC_STEMS: [&str; 8] = [
    "target", "object", "owner", "subject", "entity", "related", "biz", "business",
];

/// 命中率查表：`matched` 从 0 到 5 的取值（5 行样本）。
///
/// **为什么查表而不线性**：5 样本下 `1/5`（0.15）与 `4/5`（0.8）的语义差
/// 远大于线性给出的 0.6；`0/5` 是**硬反驳**（0.0），不是「差了 0.2」。
const HIT_RATE_TABLE: [f64; 6] = [0.0, 0.15, 0.3, 0.5, 0.8, 1.0];

// ── 档位阈值（**数值只在这里写一次**）──────────────────────────
//
// REQ-112 的建议规则要按档位分派，`Band::of` 要按阈值分档。
// 三个数字写两遍（一次在 `of` 里、一次在建议文案里）以后一定会漂 ——
// 于是提成常量，两边都引用它（AC-009「与评分卡同源」的结构保证，
// 不靠「记得同步」）。

/// `≥ 0.90` ⇒ [`Band::High`]。
pub const BAND_HIGH: f64 = 0.90;
/// `≥ 0.60` ⇒ [`Band::Middle`]。
pub const BAND_MIDDLE: f64 = 0.60;
/// `≥ 0.30` ⇒ [`Band::Low`]；低于它 ⇒ [`Band::Rejected`]。
pub const BAND_LOW: f64 = 0.30;

// ── AI 语义分的两个判据阈值（REQ-112 规则表）────────────────────
//
// 放在**评分卡的模块里**而不是建议模块里：这两个数解释的是
// `semantic_fit` 这个信号怎么读，与 0.90/0.60/0.30 是同一类东西
// （「多少算够」），不是审核界面的展示参数。

/// `semantic_fit ≥ 0.80` ⇒ 语义上站得住（配合高档 ⇒ 建议接受）。
pub const FIT_ACCEPT: f64 = 0.80;
/// `semantic_fit < 0.50` ⇒ 语义上被否定（配合「全命中」⇒ 冲突，不许替人决定）。
pub const FIT_CONFLICT: f64 = 0.50;

/// 四档（阈值见 [`Band::of`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    /// ≥ 0.90 · 进提案，审核界面默认折叠
    High,
    /// 0.60–0.90 · 进提案，逐条显示
    Middle,
    /// 0.30–0.60 · 进「待查清单」，**不进主提案**
    Low,
    /// < 0.30 · **不提案**，只记拒绝理由
    Rejected,
}

impl Band {
    /// 阈值（BA 裁定 R6）：`≥0.90` 高 · `≥0.60` 中 · `≥0.30` 低 · 否则拒绝。
    ///
    /// ⚠️ REQ-112：三个数字**只写在这里的常量里**（[`BAND_HIGH`] / [`BAND_MIDDLE`] /
    /// [`BAND_LOW`]），建议文案引用同一批常量 —— 否则「档位阈值」会有第二份写法，
    /// 而那就是 AC-009「与评分卡不漂移」要防的事。
    pub fn of(score: f64) -> Self {
        if score >= BAND_HIGH {
            Band::High
        } else if score >= BAND_MIDDLE {
            Band::Middle
        } else if score >= BAND_LOW {
            Band::Low
        } else {
            Band::Rejected
        }
    }

    /// 产物里的取值（`snake_case`，与 serde 一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Band::High => "high",
            Band::Middle => "middle",
            Band::Low => "low",
            Band::Rejected => "rejected",
        }
    }

    /// 人读标签（**只出现在 CLI 输出**，不进 JSON 字段值 —— 免得同一件事两种拼法）。
    pub fn label_zh(self) -> &'static str {
        match self {
            Band::High => "高",
            Band::Middle => "中",
            Band::Low => "低",
            Band::Rejected => "拒绝",
        }
    }

    /// 进不进主提案（AC-009：低档进「待查清单」，不进主提案）。
    pub fn enters_proposal(self) -> bool {
        matches!(self, Band::High | Band::Middle)
    }
}

/// 数据证据的来源（R1 ④：`--no-sample` 时必须标 `none`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataEvidence {
    /// 真的去源库读过样本（哪怕 0 命中 —— 那是**有**证据的反驳）。
    Sampled,
    /// 没读（`--no-sample`，或源库不可连）。**缺少证据 ≠ 有证据**。
    None,
}

/// SQL 类型族 —— 只用来判「能不能比」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeFamily {
    Num,
    Str,
    Time,
    Bool,
    Json,
    Uuid,
    Binary,
    /// 认不出的类型（自定义类型、枚举、数组……）—— 保守当「不相容」。
    Other,
}

impl TypeFamily {
    /// 类型族相同 ⇒ 1.0；`num` ↔ `str` ⇒ 0.2；其它 ⇒ 0.0（设计概要 §3.1）。
    pub fn compatibility(self, other: TypeFamily) -> f64 {
        if self == other {
            return 1.0;
        }
        match (self, other) {
            (TypeFamily::Num, TypeFamily::Str) | (TypeFamily::Str, TypeFamily::Num) => 0.2,
            _ => 0.0,
        }
    }
}

/// SQL 类型名 → 类型族（**形态判据，不查表**：`bigint`/`int8`/`numeric` 都归 `Num`）。
pub fn type_family(sql_type: &str) -> TypeFamily {
    let t = sql_type.trim().to_ascii_lowercase();
    // `character varying` / `timestamp without time zone` 这类带修饰的写法先归一
    let head = t.split(['(', ' ']).next().unwrap_or("").to_string();
    match head.as_str() {
        "smallint" | "integer" | "int" | "int2" | "int4" | "int8" | "bigint" | "numeric"
        | "decimal" | "real" | "double" | "float4" | "float8" | "money" | "serial" | "bigserial" => {
            TypeFamily::Num
        }
        "character" | "varchar" | "char" | "text" | "citext" | "name" | "bpchar" => TypeFamily::Str,
        "timestamp" | "timestamptz" | "date" | "time" | "timetz" | "interval" => TypeFamily::Time,
        "boolean" | "bool" => TypeFamily::Bool,
        "json" | "jsonb" => TypeFamily::Json,
        "uuid" => TypeFamily::Uuid,
        "bytea" => TypeFamily::Binary,
        _ => TypeFamily::Other,
    }
}

/// 目标列的「是不是键」档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKey {
    /// 单列主键 —— 1.0
    Primary,
    /// 单列唯一索引 —— 0.9
    Unique,
    /// 普通列 —— 0.4
    Plain,
    /// **不知道**（旧 `schema.json` 没有键字段）—— 按 0.4 算，但要在报告里说出来
    Unknown,
}

impl TargetKey {
    pub fn value(self) -> f64 {
        match self {
            TargetKey::Primary => 1.0,
            TargetKey::Unique => 0.9,
            TargetKey::Plain | TargetKey::Unknown => 0.4,
        }
    }
}

/// 一条关系的数据证据 —— **只有聚合，没有原值**（AC-003）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampleFacts {
    pub sampled: u32,
    pub matched: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// 没取到样本的原因（`all_null` / `timeout` / `no_key_column` / `query_error`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 一个信号的实际取值 + 它在 `method` 版本里的权重。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub name: String,
    pub value: f64,
    pub weight: f64,
    /// 只有 `sample_hit_rate` 带这三项（聚合量，**不是原值**）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_digest: Option<String>,
}

impl Signal {
    fn new(name: &str, value: f64, weight: f64) -> Self {
        Self {
            name: name.to_string(),
            value,
            weight,
            sampled: None,
            matched: None,
            sample_digest: None,
        }
    }

    fn with_sample(mut self, facts: &SampleFacts) -> Self {
        self.sampled = Some(facts.sampled);
        self.matched = Some(facts.matched);
        self.sample_digest = facts.digest.clone();
        self
    }
}

/// 触发的一条 veto 上限。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Veto {
    /// 信号名（`type_incompatible` / `zero_hit` / …）
    pub signal: String,
    /// 该信号给出的上限
    pub cap: f64,
    /// 人读理由（进审核界面与报告；**不进客户视图**）
    pub reason: String,
}

/// REQ-109 · **AI 语义判断的溯源**（AC-010 / demand 交付物 6）。
///
/// 四个必填项就是 demand 里点名的 `{model, prompt_sha, tokens, cache_hit}`，
/// 一个字都没改。另外两项（`rationale` / `cites`）是**判断本身**的可复核内容：
/// 没有它们，「AI 说这条不成立」在审核界面里就只是一句话，人无从复核。
///
/// # 为什么只有一份、挂在 `Confidence` 里
///
/// 顶层 item 上再加一个 `ai` 键的话，`propose import` 把它搬进 `payload`
/// 之后就没有任何东西读它了（快照层 `LinkType` 只认 `confidence`）——
/// 于是文件里有一份、库里没有，两份迟早不一致。
/// 挂在 `Confidence` 里则**同一次写入**穿到提案、审核界面、快照与建模轨，
/// 所以「被 AI 判断过的关系在建模轨里看得见」是结构保证。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiProvenance {
    /// 具体模型（从 `ONTOK_LLM_MODEL` 来，进指纹）
    pub model: String,
    /// 提示词内容哈希（进指纹 —— R1：缺它等于静默错）
    pub prompt_sha: String,
    /// 这一次判断花的 token（缓存命中时是**当初那次**的用量，不重复计费）
    pub tokens: u64,
    /// 这一次是否命中缓存（AC-003 的判据之一）
    pub cache_hit: bool,
    /// AI 给的理由（1–60 字，已校验）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// AI 引用的真实坐标（`表.列`，已校验存在）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cites: Option<Vec<String>>,
    /// AI 认为最合理的目标（`表.列`）。**只记录，不改变结构** ——
    /// 提案的 from/to 仍是规则抽出来的（设计概要开篇：「AI 只回答这次关系说-
    /// 得通吗」）。它在这里的唯一用途是让人复核「模型是不是想到了别的目标」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
}

/// 评分结果 —— 内嵌在提案条目里（设计概要 §2.1）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    /// `0.0–1.0`，两位小数
    pub score: f64,
    /// **必填**（AC-008）
    pub method: String,
    pub band: Band,
    pub signals: Vec<Signal>,
    /// 生效的 veto（多条触发时取上限最低的那条；`reason` 里会列出同时触发的）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub veto: Option<Veto>,
    pub data_evidence: DataEvidence,
    /// REQ-109 · AI 语义判断的溯源。
    ///
    /// `None` = 这条关系**没被 AI 判断过**（AI 关 / 缓存未命中且调用失败 /
    /// 预算用尽）。与 `semantic_fit: 0.0` 是两件事 —— 后者是「判断为不成立」。
    ///
    /// ⚠️ 用 `skip_serializing_if` 而不是恒有键：AI 关时产物必须与 REQ-108
    /// **逐字节相同**（AC-001），多一个 `"ai": null` 就是多一个字节。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiProvenance>,
}

/// 评分输入 —— **只有事实，没有 I/O**。
#[derive(Debug, Clone)]
pub struct ScoreInput {
    /// 源列名（判 `name_match` 用）
    pub column: String,
    /// 目标表名（判 `name_match` 用）
    pub to_table: String,
    /// 源列类型族
    pub from_type: TypeFamily,
    /// 目标键列类型族；`None` = 不知道（不判类型 veto，也不给 `type_compatible` 信号）
    pub to_type: Option<TypeFamily>,
    pub target_key: TargetKey,
    /// 数据证据；`None` = `--no-sample` 或源库不可连
    pub data: Option<SampleFacts>,
    /// 目标表是复合主键（R2）
    pub composite_target_key: bool,
    /// 源列是多态列（R3）
    pub polymorphic: bool,
    /// 自引用（`parent_id` 指向本表，R4）
    pub self_ref: bool,
}

/// 列名 → `name_match` 取值（1.0 / 0.3 / 0.6，设计概要 §3.1）。
pub fn name_match_value(column: &str, to_table: &str) -> f64 {
    let col = column.to_ascii_lowercase();
    // ① 直接展开命中：`customer_id` → `customers`
    for form in name_forms(to_table) {
        if col == format!("{form}_id") {
            return 1.0;
        }
    }
    // ② 泛化名：名字不指向任何具体实体
    if GENERIC_COLUMNS.contains(&col.as_str()) {
        return 0.3;
    }
    // ③ 其它（含 `*_code` / 尾缀匹配命中的那些）
    0.6
}

/// 表的两种英文写法：原样 + 去掉一个尾 `s`（`customers` → `customer`）。
///
/// ⚠️ **与 `extract::propose::english_forms` 是同一份** —— 那边改为调用本函数，
/// 免得「单复数怎么归一」出现两套判据（本仓的重复实现纪律）。
pub fn name_forms(table: &str) -> [String; 2] {
    let t = table.to_ascii_lowercase();
    let singular = if t.len() > 3 && t.ends_with('s') && !t.ends_with("ss") {
        t[..t.len() - 1].to_string()
    } else {
        t.clone()
    };
    [t, singular]
}

/// 一列是不是**泛化名**（`batch_id` 这类不指向具体实体的名字）。
pub fn is_generic_column(column: &str) -> bool {
    GENERIC_COLUMNS.contains(&column.to_ascii_lowercase().as_str())
}

/// 同一张表里某列是不是**多态列**（R3）。
///
/// 判据（两条）：
/// 1. 列名是 `<stem>_id`，且同表还有 `<stem>_type` —— 目标由 `*_type` 决定；
/// 2. 或者词干本身在白名单里（`target` / `object` / …），哪怕没配 `*_type`：
///    这类词干**天生**不指向唯一一张表（`target_id` 在真实库里指过 5 张表）。
pub fn is_polymorphic_column(columns: &[&str], column: &str) -> bool {
    let lower = column.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix("_id") else {
        return false;
    };
    if !stem.is_empty() && columns.iter().any(|c| c.eq_ignore_ascii_case(&format!("{stem}_type"))) {
        return true;
    }
    POLYMORPHIC_STEMS.contains(&stem)
}

/// 命中率查表：`matched / sampled` → 0–1（分母不是 5 时按比例映射回 5 档）。
pub fn hit_rate_value(sampled: u32, matched: u32) -> f64 {
    if sampled == 0 {
        return 0.0;
    }
    let m = matched.min(sampled);
    // (m/sampled) 映射到 5 行基准表上最接近的档，再查表。
    let idx = ((m as f64 * 5.0) / sampled as f64).round() as usize;
    HIT_RATE_TABLE[idx.min(5)]
}

/// **评分卡入口**（纯函数）—— v1，**AI 关**走这里。
pub fn score(input: &ScoreInput) -> Confidence {
    score_with_weights(input, METHOD, &WEIGHTS, None)
}

/// **评分卡入口 v2**（REQ-109）—— 接了 AI 语义项。
///
/// `semantic_fit`：
/// - `Some(v)` ⇒ 作为 `semantic_fit` 信号参与归一化（`v ∉ [0,1]` 当缺失处理，
///   因为那种值已被 [`crate::extract::ai_semantic`] 判作废，这里只是二次防线）；
/// - `None` ⇒ **不进 `signals`，不参与归一化** —— 缺的语义项不是 0 分。
///
/// ★ veto 完全不受语义项影响：本函数的压顶段一个字节都没因 v2 改过（AC-007）。
/// 语义分再高，`zero_hit` 仍是 ≤0.20、`type_incompatible` 仍是 ≤0.15。
pub fn score_with_semantic(input: &ScoreInput, semantic_fit: Option<f64>) -> Confidence {
    score_with_weights(input, METHOD_V2, &WEIGHTS_V2, semantic_fit)
}

/// 一个评分入口，两个版本的唯一差别是**权重表与方法名**（以及语义项有没有值）。
///
/// 为什么不做成两份函数：veto 段、命中率段、排序段都是「不许有第二份判据」的地方
/// （REQ-108 的纪律 H）。抄一份出来以后，改一处忘另一处就会让两个 method
/// 的分数不可比 —— 而 v1/v2 的分数**必须**可比（这就是对照表存在的意义）。
fn score_with_weights(
    input: &ScoreInput,
    method: &str,
    weights: &[(&str, f64)],
    semantic_fit: Option<f64>,
) -> Confidence {
    // ── ① 信号取值 ───────────────────────────────────────
    let name_match = name_match_value(&input.column, &input.to_table);
    let type_compatible = input
        .to_type
        .map(|to| input.from_type.compatibility(to));
    let target_is_key = input.target_key.value();
    let data_facts = input.data.as_ref();
    let hit_rate = data_facts.map(|d| hit_rate_value(d.sampled, d.matched));
    // 「不是全空」只有在**真的采过**时才可得：`--no-sample` 时它是**未知**，
    // 不能当成 1.0（那会把「没看」说成「有值」），也不能当成 0.0（那是断言全空）。
    let not_all_null = data_facts.map(|d| if d.sampled > 0 { 1.0 } else { 0.0 });

    // ── ② 基线分：只对**可用**信号归一化 ─────────────────
    //
    // 顺序严格照权重表（浮点加法的结果依赖顺序 ⇒ 顺序也是确定性的一部分）。
    let mut signals: Vec<Signal> = Vec::with_capacity(weights.len());
    for &(name, weight) in weights {
        let value = match name {
            "sample_hit_rate" => hit_rate,
            "name_match" => Some(name_match),
            "not_all_null" => not_all_null,
            "type_compatible" => type_compatible,
            "target_is_key" => Some(target_is_key),
            // ★ 语义缺失 ⇒ None ⇒ 这一项**不进 signals**（不参与归一化）。
            SIGNAL_SEMANTIC_FIT => semantic_fit.filter(|v| (0.0..=1.0).contains(v)),
            _ => None,
        };
        let Some(value) = value else { continue };
        let mut s = Signal::new(name, value, weight);
        if name == "sample_hit_rate" {
            if let Some(f) = data_facts {
                s = s.with_sample(f);
            }
        }
        signals.push(s);
    }
    let sum_w: f64 = signals.iter().map(|s| s.weight).sum();
    let sum_ws: f64 = signals.iter().map(|s| s.weight * s.value).sum();
    let baseline = if sum_w > 0.0 { sum_ws / sum_w } else { 0.0 };

    // ── ③ veto（**优先于加权和**）────────────────────────
    let mut vetoes: Vec<Veto> = Vec::new();
    if let Some(to) = input.to_type {
        if input.from_type != to {
            vetoes.push(Veto {
                signal: "type_incompatible".into(),
                cap: CAP_TYPE_INCOMPATIBLE,
                reason: format!(
                    "源列类型族 {:?} 与目标键列类型族 {:?} 不相容 —— 值永远比不中，关系不成立",
                    input.from_type, to
                ),
            });
        }
    }
    if let Some(f) = data_facts {
        if f.sampled == 0 {
            vetoes.push(Veto {
                signal: "all_null".into(),
                cap: CAP_ALL_NULL,
                reason: format!(
                    "源列取不到任何非空样本（{}）——没有数据证据",
                    f.reason.as_deref().unwrap_or("空列")
                ),
            });
            if is_generic_column(&input.column) {
                vetoes.push(Veto {
                    signal: "generic_name_no_evidence".into(),
                    cap: CAP_GENERIC_NAME_NO_EVIDENCE,
                    reason: format!(
                        "泛化列名 {} + 无数据证据：名字不指向具体实体，数据也没能证明它指向 {}",
                        input.column, input.to_table
                    ),
                });
            }
        } else if f.matched == 0 {
            vetoes.push(Veto {
                signal: "zero_hit".into(),
                cap: CAP_ZERO_HIT,
                reason: format!(
                    "采了 {} 行、命中 0 行 —— 这是硬反驳（{} 里没有这些值）",
                    f.sampled, input.to_table
                ),
            });
        } else if f.matched < f.sampled {
            vetoes.push(Veto {
                signal: "partial_hit".into(),
                cap: CAP_PARTIAL_HIT,
                reason: format!(
                    "部分命中 {}/{} —— 有真实引用，但不完全（值域稀疏或存在多态）",
                    f.matched, f.sampled
                ),
            });
        }
    } else {
        // `--no-sample`：**缺少证据 ≠ 有证据**（R1 ④）。
        // 只算结构信号会把「名字像 + 目标是主键」算成 0.65 —— 那正是
        // 「没看反而分更高」的漏洞，所以这里给一个上限，把它压到「低」档，
        // 让它只能进「待查清单」。
        vetoes.push(Veto {
            signal: "no_data_evidence".into(),
            cap: CAP_NO_DATA_EVIDENCE,
            reason: "未采样（--no-sample 或源库不可连）：只有结构证据，缺少数据证据不算可信"
                .into(),
        });
        if is_generic_column(&input.column) {
            vetoes.push(Veto {
                signal: "generic_name_no_evidence".into(),
                cap: CAP_GENERIC_NAME_NO_EVIDENCE,
                reason: format!("泛化列名 {} + 无数据证据", input.column),
            });
        }
    }
    if input.composite_target_key {
        vetoes.push(Veto {
            signal: "composite_key_unsupported".into(),
            cap: CAP_COMPOSITE_KEY_UNSUPPORTED,
            reason: "目标表是复合主键，v1 不支持复合键引用（R2）".into(),
        });
    }
    if input.polymorphic {
        vetoes.push(Veto {
            signal: "polymorphic_target".into(),
            cap: CAP_POLYMORPHIC_TARGET,
            reason: format!(
                "{} 是多态列（同表有 *_type 或多态词干），目标表由数据决定，不能靠列名定（R3）",
                input.column
            ),
        });
    }
    if input.self_ref && input.target_key != TargetKey::Primary {
        vetoes.push(Veto {
            signal: "self_ref_non_unique".into(),
            cap: CAP_SELF_REF_NON_UNIQUE,
            reason: "自引用且目标列不是主键（R4）：自引用必须指向唯一行".into(),
        });
    }

    // 多条触发 ⇒ 取**上限最低**的那条（同分按信号名排序，保证确定性）。
    vetoes.sort_by(|a, b| {
        a.cap
            .partial_cmp(&b.cap)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.signal.cmp(&b.signal))
    });
    let min_cap = vetoes.first().map(|v| v.cap).unwrap_or(1.0);
    let veto = vetoes.first().map(|v| {
        let mut v = v.clone();
        if vetoes.len() > 1 {
            let others: Vec<&str> = vetoes[1..].iter().map(|x| x.signal.as_str()).collect();
            v.reason = format!("{}（同时触发：{}）", v.reason, others.join(", "));
        }
        v
    });

    // ── ④ 最终分 = min(基线分, 上限)，再取两位小数 ────────
    //
    // 先压顶再取整：取整不会把分数抬过上限（上限本身是两位小数）。
    let score = round2(baseline.min(min_cap));

    Confidence {
        score,
        method: method.to_string(),
        band: Band::of(score),
        signals,
        veto,
        data_evidence: if data_facts.is_some() {
            DataEvidence::Sampled
        } else {
            DataEvidence::None
        },
        // 溯源由接缝层贴（`extract::propose::score_links_with_ai`）——
        // 评分函数只算分，不知道「谁判的、花了几块钱」。
        ai: None,
    }
}

/// 两位小数。**不用 `format!` 解析回浮点**（那条路会引入十进制字符串的舍入差异）。
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// 由 **`signals`（证据）**推出「按评分卡应当触发」的 veto 及上限。
///
/// 关键点：这里**不读文件里的 `veto` 字段**。文件说「我触发了 `zero_hit`」
/// 不算数，要 `signals` 里的 `sampled` / `matched` 说得出来才算。于是
/// 「把 `veto` 整段删掉再写个 0.99」与「把 `veto.cap` 改成 0.99」都失去意义 ——
/// 分数仍然被**证据**给出的上限压住。
///
/// # 为什么 `generic_name_no_evidence` 还要附加条件
///
/// `name_match = 0.3` 说明「列名是泛化名」，但评分卡只在**没有数据证据**时
/// 才把它升成 veto（有数据证据的情况下泛化名只是低分信号）。这里照抄
/// 同一条触发条件 —— 否则一条 5/5 命中的 `batch_id` 关系会被误判成
/// 「本该被 0.40 压顶」，于是合法的高分反被拒。
fn evidence_vetoes(signals: &[Signal], data_evidence: DataEvidence) -> Vec<(&'static str, f64)> {
    let find = |name: &str| signals.iter().find(|s| s.name == name);
    let mut out: Vec<(&'static str, f64)> = Vec::new();
    if let Some(h) = find("sample_hit_rate") {
        if let (Some(sampled), Some(matched)) = (h.sampled, h.matched) {
            if sampled == 0 {
                out.push(("all_null", CAP_ALL_NULL));
            } else if matched == 0 {
                out.push(("zero_hit", CAP_ZERO_HIT));
            } else if matched < sampled {
                out.push(("partial_hit", CAP_PARTIAL_HIT));
            }
        }
    }
    if let Some(t) = find("type_compatible") {
        if t.value < 1.0 {
            out.push(("type_incompatible", CAP_TYPE_INCOMPATIBLE));
        }
    }
    if data_evidence == DataEvidence::None {
        out.push(("no_data_evidence", CAP_NO_DATA_EVIDENCE));
    }
    if let Some(n) = find("name_match") {
        let no_data = data_evidence == DataEvidence::None
            || find("sample_hit_rate").map_or(false, |h| h.sampled == Some(0));
        if n.value <= 0.3 && no_data {
            out.push(("generic_name_no_evidence", CAP_GENERIC_NAME_NO_EVIDENCE));
        }
    }
    out
}

/// 「分数越上限」的报错要能让人**顺着文案找到那一行证据**。
fn evidence_hint(name: &str, signals: &[Signal]) -> String {
    let hit = signals
        .iter()
        .find(|s| s.name == "sample_hit_rate")
        .and_then(|h| match (h.sampled, h.matched) {
            (Some(s), Some(m)) => Some((s, m)),
            _ => None,
        });
    match name {
        "zero_hit" | "partial_hit" | "all_null" => match hit {
            Some((s, m)) => format!("signals 里 sample_hit_rate 是 sampled={s} / matched={m}"),
            None => "signals 里没有 sample_hit_rate 的 sampled/matched".to_string(),
        },
        "type_incompatible" => "signals 里 type_compatible < 1.0（类型族不相容）".to_string(),
        "no_data_evidence" => "data_evidence = none（没采过样）".to_string(),
        "generic_name_no_evidence" => "name_match = 0.3（泛化名）且没有数据证据".to_string(),
        _ => "signals 与 veto 声明不一致".to_string(),
    }
}

/// **AC-014 · 自证链**的三条入口校验（一个函数，两处调用：
/// `propose import` 的 `NewItem::from_json` 与 `apply` 的 `link_from_item`）。
///
/// 背景（QA `qa-review.md` §3.2 攻击 f）：`method` / 范围 / `band` 三道校验
/// 拦不住「把 `score` 从 0.20 改成 0.99、`veto` 原样保留」—— 那份提案会
/// 一路落到 `link_types.confidence = 0.99`，审核界面打出
/// 「`[高] 0.99 … 证据 0/5 … veto zero_hit 硬反驳`」这种自相矛盾的结论。
///
/// 三条判据（缺一即拒绝导入）：
///
/// | # | 校验 | 为什么 |
/// | --- | --- | --- |
/// | 1 | `score ≤ 每个触发 veto 的上限` | 否则 veto 只是注释 |
/// | 2 | `signals[].weight` 与 `method` 的权重表**逐项相等** | 设计概要 §2.1：「可复核」 |
/// | 3 | 每个 `veto` 信号名必须在 `signals` 里有**对应证据** | 防止「声明了 veto、但没有证据」 |
///
/// 另有两条**证据链完整性**判据（A-10 / A-13）也在这里落地：
///
/// | # | 校验 | 为什么 |
/// | --- | --- | --- |
/// | R5-a | `data_evidence = sampled` ⇒ `signals` 必须有完整的 `sample_hit_rate`（含 `sampled` / `matched`） | 否则删掉最强的那条证据后，四个结构信号照样能凑出 0.99（QA R5） |
/// | R5-b | `data_evidence = none` ⇒ `signals` 里**不得**有 `sample_hit_rate` | 「没采过样」与「有一份采样证据」不能同时成立 |
///
/// 第 1 条的「触发」由**证据**决定（[`evidence_vetoes`]），不由文件里的
/// `veto` 字段决定 —— 这一条是这三条校验的关键：只要 `signals` 说
/// 「5 行 0 命中」，`score` 就**不可能**是 0.99，写 `veto` 或删 `veto` 都一样。
///
/// # REQ-109 追加：按 `method` 取**它自己**那张权重表
///
/// 权重表从 [`weights_for`] 取（v1 / v2 各一张），不是「永远拿 v1 那张」——
/// 后者会让 v2 的产物一律被判「权重表之外」。语义项另加两条：
///
/// | # | 校验 | 为什么 |
/// | --- | --- | --- |
/// | S-1 | `semantic_fit` 信号的值 ∈ [0,1] | 它是概率式的判断，越界值是编的 |
/// | S-2 | 有 `semantic_fit` ⇔ 有 `ai` 溯源 | 「谁判的、缓存命没命中」必须和判断一起在，否则语义分无从审计 |
fn validate_self_evidence(
    method: &str,
    score: f64,
    signals: &[Signal],
    veto: Option<&Veto>,
    data_evidence: DataEvidence,
    ai: Option<&AiProvenance>,
) -> Result<(), String> {
    // ── 0. method 必须是有权重表的那一个 ─────────────────────────
    //
    // 若不拦，「把 method 改成 foo 再随便写权重」就能绕过第 2 条。
    let Some(table) = weights_for(method) else {
        return Err(format!(
            "confidence.method = {method:?} 没有对应的权重表（本实现只有 {METHOD} / {METHOD_V2}）—— \
             分数无法复核"
        ));
    };

    // ── 1. AC-014 #2：权重逐项相等 ───────────────────────────────
    for s in signals {
        let Some((_, want)) = table.iter().find(|(n, _)| *n == s.name) else {
            return Err(format!(
                "confidence.signals 里出现了权重表之外的信号 {:?} —— {method} 只有 {} 个信号；\
                 veto 名（如 zero_hit）不是信号名，写进 signals 无从复核",
                s.name,
                table.len()
            ));
        };
        if s.weight != *want {
            return Err(format!(
                "signals 权重与 {method} 的权重表不一致：{} 的 weight = {}，权重表是 {:.2} —— 分数不可复核",
                s.name, s.weight, want
            ));
        }
    }

    // ── 1b. REQ-109 · 语义项的两条（S-1 / S-2）────────────────────
    let semantic = signals.iter().find(|s| s.name == SIGNAL_SEMANTIC_FIT);
    if let Some(s) = semantic {
        if !(0.0..=1.0).contains(&s.value) {
            return Err(format!(
                "signals 里 {SIGNAL_SEMANTIC_FIT} 的 value = {} 越界（必须在 0.0–1.0）",
                s.value
            ));
        }
        if ai.is_none() {
            return Err(format!(
                "信号里有 {SIGNAL_SEMANTIC_FIT}，但 confidence.ai 缺失 —— 语义分必须带溯源\
                 （model / prompt_sha / tokens / cache_hit），否则无从审计它是谁判的"
            ));
        }
    } else if ai.is_some() {
        return Err(format!(
            "confidence.ai 存在，但 signals 里没有 {SIGNAL_SEMANTIC_FIT} —— \
             有溯源却没有对应的语义信号，二者必须成对出现"
        ));
    }

    // ── 2. `sample_hit_rate` 的聚合量与取值必须自洽 ───────────────
    //
    // 拦的是「把 value 从 0.0 改成 1.0」这一类：命中率是 `sampled`/`matched`
    // 的查表结果，不是自由变量。
    if let Some(h) = signals.iter().find(|s| s.name == "sample_hit_rate") {
        if let (Some(sampled), Some(matched)) = (h.sampled, h.matched) {
            if matched > sampled {
                return Err(format!(
                    "signals 自相矛盾：sample_hit_rate 的 matched = {matched} > sampled = {sampled}"
                ));
            }
            let expected = hit_rate_value(sampled, matched);
            if (h.value - expected).abs() > 1e-9 {
                return Err(format!(
                    "signals 自相矛盾：sample_hit_rate 的 value = {} 与 {sampled}/{matched} 的查表值 {expected} 对不上",
                    h.value
                ));
            }
        }
    }

    // ── 3. AC-014 #3：声明的 veto 必须有证据 ─────────────────────
    //
    // ⚠️ veto 表与 `method` **无关**（REQ-109 没有动任何上限，R7）：
    // 上限是「证据能推翻什么」的判据，不是权重的一部分。
    let derived = evidence_vetoes(signals, data_evidence);
    if let Some(v) = veto {
        let Some(canonical) = veto_cap(&v.signal) else {
            return Err(format!(
                "confidence.veto.signal = {:?} 不在 {method} 的 veto 表里 —— 无上限可核",
                v.signal
            ));
        };
        // 上限表是**代码里的常量**，不是文件里写的那个数。
        if v.cap != canonical {
            return Err(format!(
                "confidence.veto.cap = {} 与 {method} 的记录不符（{} 应为 {:.2}）—— 上限表被改过",
                v.cap, v.signal, canonical
            ));
        }
        let evidenced = derived.iter().any(|(n, _)| *n == v.signal);
        if !evidenced && !STRUCTURAL_VETOES.contains(&v.signal.as_str()) {
            return Err(format!(
                "声明了 veto {}，但 signals 里没有它 —— 没有对应证据（veto 不能只写在声明里）",
                v.signal
            ));
        }
    }

    // ── 4. A-13（R5）：`data_evidence` 与 signals 里的采样证据必须一致 ──
    //
    // QA 实测的攻击链：把 `sample_hit_rate`（连同 `veto`）一起删掉，只留四个
    // 结构信号，`data_evidence` 仍是 `sampled`，分数写 0.99 —— 旧实现全放行。
    //
    // 这与 A-10（R1）是同一件事的粗细版本：R1 删**整条**证据链，R5 只删
    // **最强的那一环**。`data_evidence = sampled` 的意思是「我读过源库的数据」，
    // 而这句声明的唯一载体就是 `sample_hit_rate`（sampled/matched/digest 都在
    // 它上面）。把它删掉，剩下 0.40 的结构分照样能凑出高分，于是「有数据证据」
    // 这句话不再有任何东西为它背书。
    //
    // 反向同样要封：声明 `none`（没采过样）却摆出一条 `sample_hit_rate`，
    // 等于既说「没用数据」又拿出一份数据证据 —— 两头都想要。
    //
    // `data_evidence` 缺失时 `from_untrusted` 已按 `sampled` 处理（严格侧），
    // 所以这里只需要处理两个枚举值。
    match data_evidence {
        DataEvidence::Sampled => {
            let Some(hit) = signals.iter().find(|s| s.name == "sample_hit_rate") else {
                return Err(
                    "confidence.data_evidence = sampled —— 声明了采样证据但 signals 里没有 \
                     sample_hit_rate：数据证据（sampled/matched/digest）只挂在这条信号上，\
                     删掉它之后剩下的结构信号不可复核（A-13 · R5）"
                        .to_string(),
                );
            };
            // 只写 `name/value/weight` 的 `sample_hit_rate` 同样推不出任何 veto：
            // `evidence_vetoes` 要 `sampled` / `matched` 才能判定零命中/部分命中。
            // 换句话说，缺聚合量等于换个写法把这一环删掉。
            if hit.sampled.is_none() || hit.matched.is_none() {
                return Err(
                    "confidence.data_evidence = sampled（声明了采样证据），但 signals 里的 \
                     sample_hit_rate 缺 sampled/matched —— 聚合量不完整，命中率与 veto 都 \
                     无从复核（A-13 · R5）"
                        .to_string(),
                );
            }
        }
        DataEvidence::None => {
            if signals.iter().any(|s| s.name == "sample_hit_rate") {
                return Err(
                    "confidence.data_evidence = none（声明没采过样），但 signals 里出现了 \
                     sample_hit_rate —— 声明没有采样证据却又摆出一份采样证据，二者矛盾\
                     （A-13 · R5）"
                        .to_string(),
                );
            }
        }
    }

    // ── 5. AC-014 #1：`score ≤ 每个触发 veto 的上限` ──────────────
    //
    // 先按**证据**推出来的上限判（删掉 `veto` 字段也逃不掉），
    // 再按**声明**的那条判（防止声明一条更大的上限来抬高分数）。
    for (name, cap) in &derived {
        if score > *cap + 1e-9 {
            return Err(format!(
                "score {score:.2} 超过 veto {name} 的上限 {cap:.2} —— 分数与证据不自洽（{}）",
                evidence_hint(name, signals)
            ));
        }
    }
    if let Some(v) = veto {
        if let Some(cap) = veto_cap(&v.signal) {
            if score > cap + 1e-9 {
                return Err(format!(
                    "score {score:.2} 超过 veto {} 的上限 {cap:.2} —— 分数与证据不自洽（{}）",
                    v.signal,
                    evidence_hint(&v.signal, signals)
                ));
            }
        }
    }
    Ok(())
}

/// 从**不可信的 JSON** 读一条 confidence（AC-008 / AC-014 的入口判据）。
///
/// 为什么不让 serde 直接反序列化：`#[derive(Deserialize)]` 对**缺字段**
/// 只有两种态度 —— 报一句 `missing field 'band'`（读者不知道这意味着什么），
/// 或者 `default` 成一个**编出来的值**。而这里需要的是第三条：
///
/// - `method` **缺失/空** ⇒ 拒绝，理由要能直接给到人：
///   「没有 method 的分数不可复核」（AC-008 的原话）；
/// - `score` 缺失 / 越界 ⇒ 拒绝；
/// - `band` 是**派生量**：以 `score` 为准重算；若 JSON 里写了一个跟分数
///   对不上的 `band`，那是**被改过的**数据 ⇒ 拒绝（而不是悄悄改成对的）；
/// - `veto` 可缺省；`data_evidence` 可缺省（缺省 = `sampled`，严格侧），
///   **但 `signals` 不可缺省**（A-10：有 `confidence` 就必须有证据链，见下）；
///   一旦给出，就必须与 `method` 的权重表、与彼此自洽（AC-014），
///   并且 `data_evidence` 必须与 `signals` 里的采样信号一致（A-13 / R5，
///   见 [`validate_self_evidence`]）。
///
/// # A-10（R1）：`signals` 不是「可选字段」
///
/// 本函数**只在条目带了 `confidence` 时被调用** —— `proposal::NewItem::from_json`
/// 与 `apply::link_from_item` 对 `None | Some(Null)` 都直接跳过。所以走到这里
/// 就说明「声明了一份分数」，而 `signals` 缺失 / `null` / 空数组意味着
/// **声明了分数却没有能推出它的证据**：`{"score":0.99,"method":"…"}` 一行
/// 就能冒充「高可信」，且删字段比改数字更省事。
///
/// 因此这里直接拒绝，而不是默认成空证据链。老的人工提案不受影响 ——
/// 它们**没有** `confidence` 键，根本走不到本函数（判据见
/// `tests/test_req108_confidence.rs` 的「legacy item 仍可导入」）。
pub fn from_untrusted(v: &serde_json::Value) -> Result<Confidence, String> {
    let method = v
        .get("method")
        .and_then(|m| m.as_str())
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or_else(|| {
            "confidence 缺 method —— 没有 method 的分数不可复核（AC-008）".to_string()
        })?;
    let score = v
        .get("score")
        .and_then(|s| s.as_f64())
        .ok_or_else(|| "confidence 缺 score（或不是数字）".to_string())?;
    if !(0.0..=1.0).contains(&score) {
        return Err(format!("confidence.score = {score} 越界（必须在 0.0–1.0）"));
    }
    let band = Band::of(score);
    if let Some(declared) = v.get("band").and_then(|b| b.as_str()) {
        // 只接受与分数一致的声明；不一致 = 数据被改过（band 是派生量）
        let declared_band: Band = serde_json::from_value(serde_json::Value::String(
            declared.to_string(),
        ))
        .map_err(|_| format!("confidence.band = {declared:?} 不是合法档位"))?;
        if declared_band != band {
            return Err(format!(
                "confidence.band = {declared} 与 score = {score} 对不上（按阈值应为 {}）",
                band.as_str()
            ));
        }
    }
    // ★ A-10：**有 `confidence` 就必须有证据链**。
    //
    // 缺失与空数组分开报：读的人需要知道自己是「没写」还是「写了个空的」，
    // 但两者都拒绝 —— 一份没有 `signals` 的分数无从复核。
    let signals: Vec<Signal> = match v.get("signals") {
        None | Some(serde_json::Value::Null) => {
            return Err(
                "confidence 有 score/method 但缺 signals —— 有置信度但缺证据链：\
                 分数必须由 signals 里的证据推出来，否则不可复核（A-10）"
                    .to_string(),
            );
        }
        Some(s) => serde_json::from_value(s.clone())
            .map_err(|e| format!("confidence.signals 形状不合法：{e}"))?,
    };
    if signals.is_empty() {
        return Err(
            "confidence.signals 是空数组 —— 有置信度但缺证据链：\
             声明了分数却没有证据，分数不可复核（A-10）"
                .to_string(),
        );
    }
    let veto: Option<Veto> = match v.get("veto") {
        None | Some(serde_json::Value::Null) => None,
        Some(x) => Some(
            serde_json::from_value(x.clone()).map_err(|e| format!("confidence.veto 形状不合法：{e}"))?,
        ),
    };
    let data_evidence = match v.get("data_evidence").and_then(|d| d.as_str()) {
        None => DataEvidence::Sampled,
        Some("sampled") => DataEvidence::Sampled,
        Some("none") => DataEvidence::None,
        Some(other) => return Err(format!("confidence.data_evidence = {other:?} 不是 sampled/none")),
    };
    // REQ-109 · AI 溯源。可缺省（AI 关的产物根本没有这个键），
    // 但**给了就必须读得懂** —— 形状不合法不静默丢弃（那等于把溯源当可选装饰）。
    let ai: Option<AiProvenance> = match v.get("ai") {
        None | Some(serde_json::Value::Null) => None,
        Some(x) => {
            let p: AiProvenance = serde_json::from_value(x.clone())
                .map_err(|e| format!("confidence.ai 形状不合法：{e}"))?;
            if p.model.trim().is_empty() {
                return Err("confidence.ai.model 是空的 —— 溯源必须记具体模型".to_string());
            }
            if p.prompt_sha.trim().is_empty() {
                return Err(
                    "confidence.ai.prompt_sha 是空的 —— 缺 prompt 项等于静默错（R1）".to_string(),
                );
            }
            Some(p)
        }
    };
    // ★ AC-014：分数必须与**证据**自洽（三条，见 `validate_self_evidence`）。
    validate_self_evidence(method, score, &signals, veto.as_ref(), data_evidence, ai.as_ref())?;
    Ok(Confidence { score, method: method.to_string(), band, signals, veto, data_evidence, ai })
}

/// 校准分箱（AC-011）—— 一个分数区间里「多少条 / 人工接受多少」。
///
/// 区间是**左闭右开**（最后一箱右端闭），见 [`calibration_bins`] 的分箱表。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationBin {
    pub low: f64,
    pub high: f64,
    /// 落在这一箱、且**已有人工决定**的条数
    pub count: usize,
    /// 其中人工接受的条数
    pub accepted: usize,
    /// 这一箱的平均分（`count == 0` ⇒ 0.0，打印时显示 `—`）
    pub mean_score: f64,
}

impl CalibrationBin {
    /// 接受率。`count == 0` ⇒ `None`（**不是 0%** —— 空箱没有比率）。
    pub fn acceptance_rate(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.accepted as f64 / self.count as f64)
        }
    }

    /// 这一箱样本够不够下结论（设计概要 §六：任一分箱 < 5 条 ⇒ 样本不足）。
    pub fn sufficient(&self) -> bool {
        self.count >= MIN_BIN_SAMPLES
    }

    pub fn label(&self) -> String {
        format!("{:.2}–{:.2}", self.low, self.high)
    }
}

/// 分箱最小样本数（设计概要 §六）。
pub const MIN_BIN_SAMPLES: usize = 5;

/// 五档分箱的边界（与设计概要 §六 一致）。
pub const CALIBRATION_EDGES: [f64; 6] = [0.0, 0.3, 0.6, 0.75, 0.9, 1.0];

/// **分箱**：`(score, accepted)` 序列 → 五档统计。
///
/// 输入顺序无关（统计量），输出顺序固定（低→高），同一份数据两次运行逐字节相同。
pub fn calibration_bins(samples: &[(f64, bool)]) -> Vec<CalibrationBin> {
    let mut bins: Vec<CalibrationBin> = (0..CALIBRATION_EDGES.len() - 1)
        .map(|i| CalibrationBin {
            low: CALIBRATION_EDGES[i],
            high: CALIBRATION_EDGES[i + 1],
            count: 0,
            accepted: 0,
            mean_score: 0.0,
        })
        .collect();
    let mut sums = vec![0.0f64; bins.len()];
    for (score, accepted) in samples {
        let s = score.clamp(0.0, 1.0);
        // 左闭右开；最后一箱闭合（1.0 必须落进 0.9–1.0，不能被丢弃）
        let idx = (0..bins.len())
            .find(|i| {
                let last = *i == bins.len() - 1;
                s >= bins[*i].low && (s < bins[*i].high || (last && s <= bins[*i].high))
            })
            .unwrap_or(bins.len() - 1);
        bins[idx].count += 1;
        if *accepted {
            bins[idx].accepted += 1;
        }
        sums[idx] += s;
    }
    for (i, b) in bins.iter_mut().enumerate() {
        if b.count > 0 {
            b.mean_score = round2(sums[i] / b.count as f64);
        }
    }
    bins
}

/// **ECE**（期望校准误差）：`Σ (nᵢ/N) · |接受率ᵢ − 平均分ᵢ|`。
///
/// 只在**全部分箱都达到最小样本数**时返回 `Some` —— 样本不足时算出来的 ECE
/// 是个噪声数，印出来会被当成结论（AC-011 明令禁止）。
pub fn expected_calibration_error(bins: &[CalibrationBin]) -> Option<f64> {
    let total: usize = bins.iter().map(|b| b.count).sum();
    if total == 0 || bins.iter().any(|b| !b.sufficient()) {
        return None;
    }
    let ece: f64 = bins
        .iter()
        .map(|b| {
            let rate = b.acceptance_rate().unwrap_or(0.0);
            (b.count as f64 / total as f64) * (rate - b.mean_score).abs()
        })
        .sum();
    Some(round2(ece))
}

/// 四档计数（AC-009 的「统计四档条数」）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandCounts {
    pub high: usize,
    pub middle: usize,
    pub low: usize,
    pub rejected: usize,
}

impl BandCounts {
    pub fn total(&self) -> usize {
        self.high + self.middle + self.low + self.rejected
    }

    pub fn add(&mut self, band: Band) {
        match band {
            Band::High => self.high += 1,
            Band::Middle => self.middle += 1,
            Band::Low => self.low += 1,
            Band::Rejected => self.rejected += 1,
        }
    }

    pub fn of<'a>(bands: impl IntoIterator<Item = &'a Band>) -> Self {
        let mut c = Self::default();
        for b in bands {
            c.add(*b);
        }
        c
    }
}

// ══════════════════════════════════════════════════════════════
// REQ-112 · **建议**（`confidence → {kind, reason, signals}`）
// ══════════════════════════════════════════════════════════════
//
// # 为什么这个函数住在评分卡模块里，而不是审核界面里
//
// 建议是**对已有信号的读法**，不是新的判断来源：它读 `band`（[`Band::of`] 的产物）、
// `signals` 里的 `sampled` / `matched` / `semantic_fit`、以及 `veto`。
// 把这些常量搬到一个「建议模块」里，等于让「多少分算高」出现第二处定义 ——
// 那就是 AC-009 要防的漂移。所以：**同一模块、同一批常量、同一份信号**。
//
// # 为什么它是纯函数（AC-001）
//
// 需求的第一条纪律是**零模型调用**：审核 135 条目若每条问一次模型，
// 十几万 token，而且每审一次就重算 ⇒ 同一份提案两次打开建议会不一样。
// 纯函数把「确定性」变成**结构保证**：本文件读不到时钟、读不到环境、读不到网络，
// 输入相同 ⇒ 输出逐字相同（`tests/test_req112_suggestion.rs` 直接断言两次调用相等）。
//
// # 建议**不是结论**（AC-003）
//
// 七条规则里唯一不许「替人决定」的是**冲突**：`band = high`（证据支持）
// 但 `semantic_fit < 0.50`（语义否定）且 `matched == sampled`（数据全命中）。
// REQ-109 实测过 3 条「5/5 命中却被 AI 判 0.0」—— 把它们判成接受或拒绝
// 都是替人做决定，所以显式标 `conflict`，并**排除在批量白名单之外**。
//
// # 规则表（顺序即优先级，从上往下第一条命中即返回）
//
// | # | 条件 | `kind` |
// | --- | --- | --- |
// | ① | veto 含 `zero_hit` / `type_incompatible` | `reject` |
// | ② | `band = high` 且 `fit ≥ 0.80` | `accept` |
// | ③ | `band = high` 且 `fit < 0.50` 且 `matched == sampled` | `conflict` |
// | ④ | `band = middle` | `lean_accept` |
// | ⑤ | `band = low` | `review` |
// | ⑥ | `band = rejected` | `reject` |
// | ⑦ | 无 `confidence` | `none` |
// | ⑧ | 其余（`band = high` 但 `fit ∈ [0.50, 0.80)`，或**没做**语义判断） | `review` |
//
// ⑧ 是**规则表没覆盖的那一格**，需求没写，但它真的存在（真实数据里
// high 档有一半是「AI 关 / fit = 0.50」）。这里不猜、不默认成 `accept`：
// 落到 ⑧ 的一律 `review`（人工核对），因为「高置信度」本身不等于「可以签」。

/// 建议的取值。**六种，不多不少** —— 页面、预览白名单、JSON 都用这一份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestKind {
    /// 证据与语义都站得住
    Accept,
    /// 证据中等 —— 倾向接受，但建议看一眼
    LeanAccept,
    /// 证据不足 / 高置信度但语义分不够 —— 建议人工核对
    Review,
    /// 硬反驳或分数低
    Reject,
    /// ⚠️ 证据支持但 AI 否定 —— **不许自动签**，也不许判成拒绝
    Conflict,
    /// 没有置信度可依赖（规则抽取的实体条目）
    None,
}

impl SuggestKind {
    /// 线上取值（`snake_case`，与 serde 一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            SuggestKind::Accept => "accept",
            SuggestKind::LeanAccept => "lean_accept",
            SuggestKind::Review => "review",
            SuggestKind::Reject => "reject",
            SuggestKind::Conflict => "conflict",
            SuggestKind::None => "none",
        }
    }

    /// 人读标签（只出现在页面与报告里，**不进 JSON 字段值**）。
    pub fn label_zh(self) -> &'static str {
        match self {
            SuggestKind::Accept => "接受",
            SuggestKind::LeanAccept => "倾向接受",
            SuggestKind::Review => "人工核对",
            SuggestKind::Reject => "拒绝",
            SuggestKind::Conflict => "冲突",
            SuggestKind::None => "无建议",
        }
    }

    /// ⭐ 进不进「按建议接受」的白名单（AC-006 / AC-007）。
    ///
    /// 只有 `accept` / `lean_accept` 进 —— 冲突、存疑、拒绝、无建议**一律不进**。
    /// 这是**唯一**的白名单判据：预览接口与测试都调它，页面不自己判。
    pub fn enters_batch(self) -> bool {
        matches!(self, SuggestKind::Accept | SuggestKind::LeanAccept)
    }

    /// 页面分组顺序：**冲突最前**（AC-008），其余按严重程度递减。
    ///
    /// 冲突排最前的理由：它是**唯一**一类「程序不知道该听谁的」条目，
    /// 混在「接受/拒绝」里就会被当成一条普通建议扫过去。
    pub fn group_rank(self) -> u8 {
        match self {
            SuggestKind::Conflict => 0,
            SuggestKind::Reject => 1,
            SuggestKind::Review => 2,
            SuggestKind::LeanAccept => 3,
            SuggestKind::Accept => 4,
            SuggestKind::None => 5,
        }
    }

    /// 六种取值的**固定顺序**（计数与页面分组都用它，保证确定性）。
    pub const ALL: [SuggestKind; 6] = [
        SuggestKind::Conflict,
        SuggestKind::Reject,
        SuggestKind::Review,
        SuggestKind::LeanAccept,
        SuggestKind::Accept,
        SuggestKind::None,
    ];
}

/// 一条建议所依据的**具体信号**（AC-001 的 `signals` 项）。
///
/// 每个字段都是**置信度里已经有的值**，这里只做取值搬运，不做二次推断：
/// 于是「这条建议为什么是这样」永远能顺着字段找回原始证据。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SuggestionSignals {
    /// 评分卡给出的档位（`high` / `middle` / `low` / `rejected`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// `sample_hit_rate` 的命中数 / 采样数（**只有采过样才有**）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled: Option<u32>,
    /// AI 语义分；**缺失 ≠ 0**（没判过与判成 0 是两件事）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_fit: Option<f64>,
    /// 生效的 veto 信号名
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub veto: Option<String>,
}

impl SuggestionSignals {
    /// 从一条 `confidence` 里把信号搬出来（**只搬不算**）。
    pub fn of(c: &Confidence) -> Self {
        let find = |name: &str| c.signals.iter().find(|s| s.name == name);
        let hit = find("sample_hit_rate");
        Self {
            band: Some(c.band.as_str().to_string()),
            score: Some(c.score),
            matched: hit.and_then(|h| h.matched),
            sampled: hit.and_then(|h| h.sampled),
            semantic_fit: find(SIGNAL_SEMANTIC_FIT).map(|s| s.value),
            veto: c.veto.as_ref().map(|v| v.signal.clone()),
        }
    }

    /// 证据短语（`5/5`）：没采过样时说「无采样」，**不说 `0/0`**
    /// （`0/0` 读起来像「一条都没命中」，而事实是「没看」）。
    fn hits_zh(&self) -> String {
        match (self.matched, self.sampled) {
            (Some(m), Some(s)) => format!("{m}/{s}"),
            _ => "无采样".to_string(),
        }
    }
}

/// 一条建议 —— **只由 [`Confidence`] 确定性推导**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suggestion {
    pub kind: SuggestKind,
    /// 一句人读依据（**≤ 40 字**，AC-008；由下面的测试钉住）。
    pub reason: String,
    /// 依据落在哪些信号上（不在 HTTP 面上暴露，但测试与报告要能用它复核）
    pub signals: SuggestionSignals,
}

impl Suggestion {
    /// 置信度块**存在但解析不了**时的建议：`none` + 明确说「解析不了」。
    ///
    /// 为什么不是「当成 accept/reject」：解析失败意味着不知道该信什么，
    /// 唯一安全的落点是「无建议」—— 而 `none` **不进**批量白名单（AC-007）。
    pub fn unreadable() -> Self {
        Self {
            kind: SuggestKind::None,
            reason: "置信度块无法解析 —— 按无建议处理".to_string(),
            signals: SuggestionSignals::default(),
        }
    }

    /// 页面/接口用的紧凑形态：`{kind, reason}`。
    pub fn brief(&self) -> serde_json::Value {
        serde_json::json!({ "kind": self.kind.as_str(), "reason": self.reason })
    }
}

/// 无置信度时的依据文案（**唯一写法**）。
pub const REASON_NO_CONFIDENCE: &str = "规则抽取，无置信度可依赖";

/// **建议入口**（纯函数，AC-001）。
///
/// 输入 `None`（条目没有 `confidence`）⇒ `none`。
/// 输入 `Some` ⇒ 按上面的规则表**从上往下第一条命中**。
///
/// # 为什么用 `Option<&Confidence>` 而不是 `&Confidence`
///
/// 「无置信度」是需求里的一条**正经规则**（⑦），不是错误分支。
/// 用 `Option` 让调用方在**类型上**必须处理它，而不是靠 `unwrap_or_default()`
/// 编一份空分数出来 —— 那会把「没有判断依据」说成「分数是 0」。
pub fn suggestion(confidence: Option<&Confidence>) -> Suggestion {
    let Some(c) = confidence else {
        return Suggestion {
            kind: SuggestKind::None,
            reason: REASON_NO_CONFIDENCE.to_string(),
            signals: SuggestionSignals::default(),
        };
    };
    let signals = SuggestionSignals::of(c);
    let (kind, reason) = classify(c, &signals);
    Suggestion { kind, reason, signals }
}

/// 规则表本体（顺序即优先级）。见模块注释里的那张表。
fn classify(c: &Confidence, s: &SuggestionSignals) -> (SuggestKind, String) {
    // ── ① veto 硬反驳：**永远第一** ──────────────────────────
    //
    // 「采样 0/N」与「类型不相容」是**反驳**，不是扣分项：
    // 分数再高也不该签（评分卡用上限压顶，建议用优先级表达同一件事）。
    if let Some(v) = &c.veto {
        match v.signal.as_str() {
            "zero_hit" => {
                return (
                    SuggestKind::Reject,
                    format!("采样 {} 全不命中（硬反驳）", hits_zero_first(s)),
                );
            }
            "type_incompatible" => {
                return (SuggestKind::Reject, "类型不相容（硬反驳）".to_string());
            }
            _ => {}
        }
    }

    match c.band {
        // ── ②③⑧ 高档：由 AI 语义分决定「接受 / 冲突 / 人工核对」 ──
        Band::High => match (s.semantic_fit, s.matched, s.sampled) {
            (Some(fit), Some(matched), Some(sampled)) if fit >= FIT_ACCEPT => (
                SuggestKind::Accept,
                format!("采样 {matched}/{sampled} + AI fit {fit:.2}"),
            ),
            // ⚠️ 冲突：证据支持（全命中）但 AI 否定。**不许自动签，也不许判拒绝。**
            (Some(fit), Some(matched), Some(sampled))
                if fit < FIT_CONFLICT && matched == sampled =>
            {
                (
                    SuggestKind::Conflict,
                    format!("{matched}/{sampled} 命中但 AI 判 {fit:.2} —— 需人工判"),
                )
            }
            // ⑧ 中间态（fit ∈ [0.50, 0.80)）或全命中但语义分不够
            (Some(fit), Some(matched), Some(sampled)) => (
                SuggestKind::Review,
                format!("采样 {matched}/{sampled}，AI fit {fit:.2} 未达 {FIT_ACCEPT:.2}"),
            ),
            // ⑧ 没做语义判断（AI 关）或没采样：高档只剩结构信号，不值得替人签
            _ => {
                let reason = match (s.matched, s.sampled) {
                    (Some(m), Some(sampled)) => {
                        format!("采样 {m}/{sampled} 但无 AI 语义判断 —— 建议抽查")
                    }
                    _ => "高档但无采样、无 AI 语义判断 —— 建议人工核对".to_string(),
                };
                (SuggestKind::Review, reason)
            }
        },
        // ── ④ 中档：倾向接受，但要说「看一眼」 ──────────────────
        Band::Middle => (
            SuggestKind::LeanAccept,
            format!("证据中等（{}），建议看一眼再签", s.hits_zh()),
        ),
        // ── ⑤ 低档：证据不足 ───────────────────────────────────
        Band::Low => (
            SuggestKind::Review,
            format!("证据不足（分数 {:.2}）—— 建议人工核对", c.score),
        ),
        // ── ⑥ 拒绝档：分数低于阈值 ─────────────────────────────
        Band::Rejected => (
            SuggestKind::Reject,
            format!("分数 {:.2} 低于 {BAND_LOW:.2} —— 建议拒绝", c.score),
        ),
    }
}

/// `zero_hit` 的分子**永远是 0**（否则就不叫零命中），所以文案按 `0/N` 拼 ——
/// 不写成 `{matched}/{sampled}`，免得哪天数据异常时打出「3/5 全不命中」这种自相矛盾的话。
fn hits_zero_first(s: &SuggestionSignals) -> String {
    match s.sampled {
        Some(sampled) => format!("0/{sampled}"),
        None => "0/0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 构造器：默认一条「结构上看起来很好」的关系 ──────────────

    fn base() -> ScoreInput {
        ScoreInput {
            column: "customer_id".into(),
            to_table: "customers".into(),
            from_type: TypeFamily::Str,
            to_type: Some(TypeFamily::Str),
            target_key: TargetKey::Primary,
            data: Some(SampleFacts { sampled: 5, matched: 5, digest: None, reason: None }),
            composite_target_key: false,
            polymorphic: false,
            self_ref: false,
        }
    }

    /// AC-004 的前提：**把 name_match / target_is_key 人为置 1.0**。
    /// 命中率 5/5、类型相容 —— 加权和能拿满分。
    fn maxed_structural() -> ScoreInput {
        base()
    }

    /// A-10：一份**最小但完整**的证据链（`sample_hit_rate` + `name_match`，权重与表一致）。
    ///
    /// 只用来满足「有 `confidence` 就必须有证据链」这条入口前提 ——
    /// 它不参与分数断言，也不冒充评分卡的真实产出（真实产出永远是五个信号
    /// 里至少 `name_match` + `target_is_key` 两个）。
    ///
    /// ★ A-13（R5）：`data_evidence` 缺省按 `sampled` 处理，所以这份「最小证据链」
    /// 必须带 `sample_hit_rate`（且 `sampled` / `matched` 齐全）—— 否则它描述的
    /// 是一份「声明读过库、却删掉了唯一数据证据」的形状，正是 R5 要拒的。
    fn minimal_signals() -> serde_json::Value {
        serde_json::json!([
            {"name": "sample_hit_rate", "value": 1.0, "weight": 0.45, "sampled": 5, "matched": 5},
            {"name": "name_match", "value": 1.0, "weight": 0.2}
        ])
    }

    /// `--no-sample` 的**真产出形状**（3 个结构信号，没有 `sample_hit_rate`）。
    ///
    /// 用于「`data_evidence: none` 不得带采样信号」的正向对照：它只能配
    /// `data_evidence: none`，配 `sampled` 就是 R5 要拒的那一半。
    fn no_sample_signals() -> serde_json::Value {
        serde_json::json!([
            {"name": "name_match", "value": 1.0, "weight": 0.2},
            {"name": "type_compatible", "value": 1.0, "weight": 0.1},
            {"name": "target_is_key", "value": 1.0, "weight": 0.1}
        ])
    }

    #[test]
    fn weights_sum_to_one_and_match_the_design_table() {
        let sum: f64 = WEIGHTS.iter().map(|(_, w)| w).sum();
        assert!((sum - 1.0).abs() < 1e-12, "权重合计必须为 1.0，实际 {sum}");
        assert_eq!(
            WEIGHTS,
            [
                ("sample_hit_rate", 0.45),
                ("name_match", 0.20),
                ("not_all_null", 0.15),
                ("type_compatible", 0.10),
                ("target_is_key", 0.10),
            ],
            "权重表就是设计概要 §3.2 那张（数据证据 0.60 / 结构证据 0.40）"
        );
        // 语义项**不在**权重表里（v1 不接，不参与归一化）
        assert!(!WEIGHTS.iter().any(|(n, _)| *n == SIGNAL_SEMANTIC_FIT));
    }

    #[test]
    fn band_thresholds_are_exactly_090_060_030() {
        assert_eq!(Band::of(1.0), Band::High);
        assert_eq!(Band::of(0.90), Band::High, "0.90 是高档的**闭**下界");
        assert_eq!(Band::of(0.89), Band::Middle);
        assert_eq!(Band::of(0.60), Band::Middle, "0.60 是中档的闭下界");
        assert_eq!(Band::of(0.59), Band::Low);
        assert_eq!(Band::of(0.30), Band::Low, "0.30 是低档的闭下界");
        assert_eq!(Band::of(0.29), Band::Rejected);
        assert_eq!(Band::of(0.0), Band::Rejected);
        assert!(Band::High.enters_proposal() && Band::Middle.enters_proposal());
        assert!(!Band::Low.enters_proposal(), "低档进「待查清单」，不进主提案（AC-009）");
        assert!(!Band::Rejected.enters_proposal());
        // 四档标签只给 CLI 用；JSON 值用 as_str
        assert_eq!(Band::Rejected.as_str(), "rejected");
        assert_eq!(Band::Rejected.label_zh(), "拒绝");
    }

    /// 全绿的一条关系应当拿满分并落「高」（否则阈值无从谈起）。
    #[test]
    fn a_perfect_relation_scores_one_and_lands_high() {
        let c = score(&maxed_structural());
        assert_eq!(c.score, 1.0);
        assert_eq!(c.band, Band::High);
        assert_eq!(c.method, METHOD, "method 必填（AC-008）");
        assert_eq!(c.veto, None);
        assert_eq!(c.data_evidence, DataEvidence::Sampled);
        assert_eq!(c.signals.len(), 5);
    }

    // ── AC-004 · 四类 veto 上限逐类执行 ────────────────────

    /// ① 类型族不相容（int vs text）⇒ ≤ 0.15。
    #[test]
    fn veto_type_incompatible_caps_at_015() {
        let mut i = maxed_structural();
        i.from_type = TypeFamily::Str;
        i.to_type = Some(TypeFamily::Num);
        let c = score(&i);
        assert!(c.score <= 0.15, "类型不相容必须 ≤ 0.15，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "type_incompatible");
        assert_eq!(c.band, Band::Rejected);
    }

    /// ② 采样 0 命中 ⇒ ≤ 0.20。
    #[test]
    fn veto_zero_hit_caps_at_020() {
        let mut i = maxed_structural();
        i.data = Some(SampleFacts { sampled: 5, matched: 0, digest: None, reason: None });
        let c = score(&i);
        assert!(c.score <= 0.20, "0 命中必须 ≤ 0.20，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "zero_hit");
        assert_eq!(c.band, Band::Rejected);
    }

    /// ③ 泛化列名 + 无数据证据 ⇒ ≤ 0.40。
    #[test]
    fn veto_generic_name_without_evidence_caps_at_040() {
        let mut i = maxed_structural();
        i.column = "batch_id".into();
        i.to_table = "gift_code_batch".into();
        i.data = Some(SampleFacts { sampled: 0, matched: 0, digest: None, reason: Some("all_null".into()) });
        let c = score(&i);
        assert!(c.score <= 0.40, "泛化名 + 无证据必须 ≤ 0.40，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "generic_name_no_evidence");
    }

    /// ④ 源列全空（`sampled = 0`）⇒ ≤ 0.45。
    #[test]
    fn veto_all_null_caps_at_045() {
        let mut i = maxed_structural();
        i.data = Some(SampleFacts { sampled: 0, matched: 0, digest: None, reason: Some("all_null".into()) });
        let c = score(&i);
        assert!(c.score <= 0.45, "全空必须 ≤ 0.45，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "all_null");
        assert_eq!(c.band, Band::Low);
    }

    /// **AC-004 的核心判据**：把结构信号全置 1.0，四类 veto 依然各自压顶。
    ///
    /// 变异自证：把 `score` 里的 `baseline.min(min_cap)` 改成 `baseline`（或被
    /// veto 的 cap 加回去），本测试四个断言同时红 —— 那是「加权和把 veto 救回来」。
    #[test]
    fn vetoes_survive_maxed_structural_signals() {
        // 先证明结构信号确实被置满了（否则下面四条断言是空的）
        let full = score(&maxed_structural());
        let by_name = |n: &str, c: &Confidence| {
            c.signals.iter().find(|s| s.name == n).map(|s| s.value)
        };
        assert_eq!(by_name("name_match", &full), Some(1.0));
        assert_eq!(by_name("target_is_key", &full), Some(1.0));
        assert_eq!(by_name("type_compatible", &full), Some(1.0));

        let cases: [(&str, ScoreInput, f64, Band); 4] = [
            (
                "type_incompatible",
                {
                    let mut i = maxed_structural();
                    i.to_type = Some(TypeFamily::Num);
                    i
                },
                0.15,
                Band::Rejected,
            ),
            (
                "zero_hit",
                {
                    let mut i = maxed_structural();
                    i.data = Some(SampleFacts { sampled: 5, matched: 0, digest: None, reason: None });
                    i
                },
                0.20,
                Band::Rejected,
            ),
            (
                "generic_name_no_evidence",
                {
                    let mut i = maxed_structural();
                    i.column = "session_id".into();
                    i.to_table = "simulate_login_sessions".into();
                    i.data = Some(SampleFacts { sampled: 0, matched: 0, digest: None, reason: Some("all_null".into()) });
                    i
                },
                0.40,
                // 这一类的**基线本身就掉到 0.26**（泛化名 0.3 + 全空 0），
                // 上限 0.40 并没有被用到 —— 正好说明「泛化名 + 无证据」是双重坏消息。
                Band::Rejected,
            ),
            (
                "all_null",
                {
                    let mut i = maxed_structural();
                    i.data = Some(SampleFacts { sampled: 0, matched: 0, digest: None, reason: Some("all_null".into()) });
                    i
                },
                0.45,
                Band::Low,
            ),
        ];
        for (name, input, cap, expect_band) in cases {
            let c = score(&input);
            // 目标列是主键这条事实在这四类里都没变 ⇒ 它必须仍是 1.0。
            assert_eq!(by_name("target_is_key", &c), Some(1.0), "{name}：target_is_key 必须仍是 1.0");
            // ⚠️ `name_match` 在「泛化列名」那一类里**不可能**是 1.0 ——
            // 那个泛化名本身就是触发条件（同一件事实既是信号又是 veto）。
            // AC-004 的字面要求是「把结构信号置满，上限仍生效」；本条对
            // 其余三类逐条断言 name_match = 1.0，对泛化那一类改为断言
            // 「加权和本可以超过上限，但最终分没有」—— 那才是 veto 没被救回来的证据。
            if name != "generic_name_no_evidence" {
                assert_eq!(by_name("name_match", &c), Some(1.0), "{name}：name_match 必须仍是 1.0");
            }
            assert!(
                c.score <= cap,
                "{name}：结构信号全满时分数仍必须 ≤ {cap}，实际 {}",
                c.score
            );
            assert_eq!(c.band, expect_band, "{name}：上限 {cap} 对应的档位");

            // ★ 关键证据：**不做 veto 时加权和会是多少** —— 只要它高于上限，
            //   最终分低于它，就说明「是 veto 压住的，不是加权和自己掉下来的」。
            let sum_w: f64 = c.signals.iter().map(|s| s.weight).sum();
            let sum_ws: f64 = c.signals.iter().map(|s| s.weight * s.value).sum();
            let baseline = sum_ws / sum_w;
            assert!(c.score <= baseline + 1e-12, "{name}：最终分不可能高于未压顶的基线");
            if baseline > cap {
                assert!(
                    c.score < baseline,
                    "{name}：基线 {baseline} 高于上限 {cap}，最终分 {} 必须被压下来",
                    c.score
                );
            }
        }
    }

    /// ⑤ 部分命中（0 < rate < 1）⇒ ≤ 0.75，**不得落「高」**（AC-007）。
    #[test]
    fn veto_partial_hit_caps_at_075_and_never_high() {
        for (sampled, matched) in [(5u32, 1u32), (5, 2), (5, 3), (5, 4), (4, 1), (2, 1), (3, 1)] {
            let mut i = maxed_structural();
            i.data = Some(SampleFacts { sampled, matched, digest: None, reason: None });
            let c = score(&i);
            assert!(
                c.score <= 0.75,
                "{matched}/{sampled} 部分命中必须 ≤ 0.75，实际 {}",
                c.score
            );
            assert_ne!(c.band, Band::High, "{matched}/{sampled} 不得落「高」档（AC-007）");
        }
    }

    /// ⑥ 复合键（R2）⇒ ≤ 0.50。⑦ 多态（R3）⇒ ≤ 0.40。⑧ 自引用非唯一（R4）⇒ ≤ 0.50。
    #[test]
    fn veto_r2_r3_r4_caps() {
        let mut composite = maxed_structural();
        composite.composite_target_key = true;
        let c = score(&composite);
        assert!(c.score <= 0.50, "复合键 ≤ 0.50，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "composite_key_unsupported");

        let mut poly = maxed_structural();
        poly.polymorphic = true;
        let c = score(&poly);
        assert!(c.score <= 0.40, "多态目标 ≤ 0.40，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "polymorphic_target");

        let mut self_ref = maxed_structural();
        self_ref.self_ref = true;
        self_ref.target_key = TargetKey::Unique;
        let c = score(&self_ref);
        assert!(c.score <= 0.50, "自引用非主键 ≤ 0.50，实际 {}", c.score);
        assert_eq!(c.veto.as_ref().unwrap().signal, "self_ref_non_unique");

        // 自引用 + 主键：**不**触发（R4 允许）
        let mut self_ref_pk = maxed_structural();
        self_ref_pk.self_ref = true;
        assert_eq!(score(&self_ref_pk).veto, None);
    }

    /// 多条 veto ⇒ 取**最小上限**，且理由里要列出同时触发的其它信号。
    #[test]
    fn multiple_vetoes_take_the_lowest_cap() {
        let mut i = maxed_structural();
        i.column = "batch_id".into();
        i.to_table = "gift_code_batch".into();
        i.from_type = TypeFamily::Str;
        i.to_type = Some(TypeFamily::Num);
        i.data = Some(SampleFacts { sampled: 5, matched: 0, digest: None, reason: None });
        let c = score(&i);
        let v = c.veto.as_ref().unwrap();
        assert_eq!(v.signal, "type_incompatible", "0.15 是这里的最低上限");
        assert!(v.cap <= 0.15);
        assert!(c.score <= 0.15);
        // 这条输入有**数据证据**（采了 5 行、0 命中）⇒ 触发的是 zero_hit；
        // `generic_name_no_evidence` **不**触发（它专门表示「没有数据证据」，
        // 有证据的反驳由 `zero_hit` 表达 —— 两个信号不许混）
        assert!(
            v.reason.contains("zero_hit"),
            "同时触发的 zero_hit 必须在理由里可见：{}",
            v.reason
        );
        assert!(
            !v.reason.contains("generic_name_no_evidence"),
            "有数据证据时不该再报「泛化名且无证据」：{}",
            v.reason
        );
    }

    // ── R1 ④：`--no-sample` 不许把分数抬高 ─────────────────

    /// 没采样 vs 采了但 0 命中：**后者更低**，而且前者必须被压进「低」档。
    ///
    /// 变异自证：删掉 `no_data_evidence` 那条 veto，结构信号会让
    /// `customer_id → customers` 拿到 0.80（中档），于是「没看反而分更高」。
    #[test]
    fn no_sample_never_scores_higher_than_sampled_evidence() {
        let mut no_data = maxed_structural();
        no_data.data = None;
        let c = score(&no_data);
        assert_eq!(c.data_evidence, DataEvidence::None, "必须标 none（R1 ④）");
        assert!(c.score <= 0.45, "无数据证据必须 ≤ 0.45，实际 {}", c.score);
        assert!(!c.band.enters_proposal(), "无数据证据不得进主提案");

        let mut zero = maxed_structural();
        zero.data = Some(SampleFacts { sampled: 5, matched: 0, digest: None, reason: None });
        assert!(
            score(&zero).score < c.score,
            "「采了但 0 命中」必须比「没采」更确定地低"
        );

        // `--no-sample` 时不给 `sample_hit_rate.capability` —— 它**不参与归一化**
        assert!(
            !c.signals.iter().any(|s| s.name == "sample_hit_rate"),
            "没采就没有命中率信号（不是 0.0，那是断言全不命中）"
        );
        assert!(
            !c.signals.iter().any(|s| s.name == "not_all_null"),
            "没采就不知道有没有值（不能当 1.0，也不能当 0.0）"
        );
    }

    // ── 信号取值本身 ────────────────────────────────────────

    #[test]
    fn name_match_uses_the_design_table() {
        assert_eq!(name_match_value("customer_id", "customers"), 1.0, "直接展开");
        assert_eq!(name_match_value("order_id", "order"), 1.0);
        assert_eq!(name_match_value("batch_id", "gift_code_batch"), 0.3, "泛化名");
        assert_eq!(name_match_value("session_id", "simulate_login_sessions"), 0.3);
        assert_eq!(name_match_value("model_id", "llm_model"), 0.6, "其它");
        assert_eq!(name_match_value("channel_code", "marketing_channel"), 0.6);
        // 泛化名单**不**等于「所有 *_id」
        assert!(!is_generic_column("customer_id"));
        assert!(is_generic_column("batch_id"));
    }

    #[test]
    fn type_families_are_mapped_by_shape() {
        assert_eq!(type_family("bigint"), TypeFamily::Num);
        assert_eq!(type_family("character varying"), TypeFamily::Str);
        assert_eq!(type_family("timestamp without time zone"), TypeFamily::Time);
        assert_eq!(type_family("numeric(10,2)"), TypeFamily::Num);
        assert_eq!(type_family("jsonb"), TypeFamily::Json);
        assert_eq!(type_family("uuid"), TypeFamily::Uuid);
        assert_eq!(type_family("my_custom_enum"), TypeFamily::Other);
        // 相容性三档
        assert_eq!(TypeFamily::Num.compatibility(TypeFamily::Num), 1.0);
        assert_eq!(TypeFamily::Num.compatibility(TypeFamily::Str), 0.2);
        assert_eq!(TypeFamily::Num.compatibility(TypeFamily::Time), 0.0);
    }

    #[test]
    fn target_key_values_are_1_09_04() {
        assert_eq!(TargetKey::Primary.value(), 1.0);
        assert_eq!(TargetKey::Unique.value(), 0.9);
        assert_eq!(TargetKey::Plain.value(), 0.4);
        assert_eq!(TargetKey::Unknown.value(), 0.4, "不知道按普通列算，但报告里要显形");
    }

    #[test]
    fn hit_rate_lookup_table() {
        assert_eq!(hit_rate_value(5, 5), 1.0);
        assert_eq!(hit_rate_value(5, 4), 0.8);
        assert_eq!(hit_rate_value(5, 3), 0.5);
        assert_eq!(hit_rate_value(5, 2), 0.3);
        assert_eq!(hit_rate_value(5, 1), 0.15);
        assert_eq!(hit_rate_value(5, 0), 0.0);
        // 分母不是 5 时按比例映射：0/3 → 0、3/3 → 1.0
        assert_eq!(hit_rate_value(3, 0), 0.0);
        assert_eq!(hit_rate_value(3, 3), 1.0);
        assert_eq!(hit_rate_value(0, 0), 0.0);
        // 表本身必须是设计概要那张（单调递增）
        for i in 1..HIT_RATE_TABLE.len() {
            assert!(HIT_RATE_TABLE[i] > HIT_RATE_TABLE[i - 1]);
        }
    }

    #[test]
    fn polymorphic_columns_are_detected_by_structure_not_by_list_only() {
        let cols = ["id", "subject_type", "subject_id", "status"];
        assert!(is_polymorphic_column(&cols, "subject_id"), "同表有 subject_type");
        assert!(!is_polymorphic_column(&cols, "id"));
        // 白名单词干：没有 *_type 也算多态
        assert!(is_polymorphic_column(&["target_id", "memo"], "target_id"));
        assert!(!is_polymorphic_column(&["customer_id"], "customer_id"));
    }

    // ── AC-005 · 可复算（逐位相同）────────────────────────────

    /// 同一输入 ⇒ `score` **逐位相同**，连信号顺序都一样。
    ///
    /// 注意这条**不能**证明「跨进程一致」（浮点在同一个二进制里是确定的），
    /// 它证明的是：本函数没有隐藏状态（时间/随机/缓存）参与计算。
    #[test]
    fn scoring_is_bit_for_bit_reproducible() {
        let mut i = base();
        i.data = Some(SampleFacts {
            sampled: 5,
            matched: 4,
            digest: Some("sha256:abc".into()),
            reason: None,
        });
        let a = score(&i);
        let b = score(&i);
        assert_eq!(a, b, "同一输入两次评分必须逐字段相同");
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
            "序列化后也必须逐字节相同"
        );
        assert_eq!(a.score.to_bits(), b.score.to_bits(), "浮点位模式相同");
        // 分数是两位小数
        assert_eq!(a.score, round2(a.score));
    }

    /// 分数域：永远在 [0,1]，且 `band` 与分数一致。
    #[test]
    fn score_stays_in_range_and_band_agrees_with_score() {
        let mut cases = vec![maxed_structural()];
        let mut z = maxed_structural();
        z.data = Some(SampleFacts { sampled: 5, matched: 0, digest: None, reason: None });
        cases.push(z);
        let mut n = maxed_structural();
        n.data = None;
        cases.push(n);
        let mut p = maxed_structural();
        p.data = Some(SampleFacts { sampled: 5, matched: 3, digest: None, reason: None });
        cases.push(p);
        for i in cases {
            let c = score(&i);
            assert!((0.0..=1.0).contains(&c.score), "分数越界：{}", c.score);
            assert_eq!(c.band, Band::of(c.score), "band 必须由分数算出");
            assert_eq!(c.data_evidence == DataEvidence::None, c.signals.len() == 3);
        }
    }

    /// 数据证据里带的是**聚合量**，且原值在类型上无处可放（AC-003）。
    #[test]
    fn sample_signal_carries_only_aggregates() {
        let mut i = base();
        i.data = Some(SampleFacts {
            sampled: 5,
            matched: 4,
            digest: Some("sha256:deadbeef".into()),
            reason: None,
        });
        let c = score(&i);
        let s = c.signals.iter().find(|s| s.name == "sample_hit_rate").unwrap();
        assert_eq!(s.value, 0.8);
        assert_eq!(s.weight, 0.45);
        assert_eq!(s.sampled, Some(5));
        assert_eq!(s.matched, Some(4));
        assert_eq!(s.sample_digest.as_deref(), Some("sha256:deadbeef"));
        // 落库/落盘的那份 JSON 里，`signals` 只有这三个聚合字段
        let v = serde_json::to_value(&c).unwrap();
        let sig = &v["signals"][0];
        let keys: Vec<&str> = sig.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        for k in &keys {
            assert!(
                ["name", "value", "weight", "sampled", "matched", "sample_digest"].contains(k),
                "signals 里出现了意料之外的键：{k}（原值不许进产物）"
            );
        }
    }

    /// 四档计数（AC-009 的统计口径）。
    #[test]
    fn band_counts_are_stable() {        let bands = [Band::High, Band::High, Band::Middle, Band::Low, Band::Rejected];
        let c = BandCounts::of(bands.iter());
        assert_eq!((c.high, c.middle, c.low, c.rejected, c.total()), (2, 1, 1, 1, 5));
    }

    /// `Confidence` 的 JSON 形状就是设计概要 §2.1 那张（字段名对齐，method 在内）。
    #[test]
    fn confidence_json_shape_matches_the_design() {
        let mut i = base();
        i.data = Some(SampleFacts {
            sampled: 5,
            matched: 4,
            digest: Some("sha256:x".into()),
            reason: None,
        });
        let v = serde_json::to_value(score(&i)).unwrap();
        for k in ["score", "method", "band", "signals", "data_evidence"] {
            assert!(v.get(k).is_some(), "confidence 缺字段 {k}");
        }
        assert_eq!(v["method"], serde_json::json!(METHOD));
        assert_eq!(v["data_evidence"], serde_json::json!("sampled"));
        // 这一条是 4/5 ⇒ 部分命中，veto 必须**带出来**（审核界面要显示理由）
        assert_eq!(v["veto"]["signal"], serde_json::json!("partial_hit"));
        assert!(v["veto"]["reason"].as_str().unwrap().contains("4/5"));
        // 反序列化回来必须相等（落库 → 读回这一跳）
        let back: Confidence = serde_json::from_value(v).unwrap();
        assert_eq!(back, score(&i));
    }

    // ── AC-008 · `method` 必填的入口判据 ─────────────────────

    /// **缺 method ⇒ 拒绝**，且错误信息要能直接告诉人「为什么」。
    ///
    /// 变异自证：把 `from_untrusted` 里那段 `ok_or_else(...)` 换成
    /// `unwrap_or(METHOD)`，本测试立刻红 —— 而那时的表现是
    /// 「一份没写方法的分数被**冒名**装成 evidence-scorecard-v1」。
    #[test]
    fn confidence_without_method_is_rejected() {
        let no_method = serde_json::json!({"score": 0.9});
        let err = from_untrusted(&no_method).unwrap_err();
        assert!(err.contains("method"), "{err}");
        assert!(err.contains("不可复核"), "错误信息要说出「没有 method 的分数不可复核」：{err}");

        for bad in [serde_json::json!(""), serde_json::json!("   "), serde_json::json!(null)] {
            let v = serde_json::json!({"score": 0.9, "method": bad});
            assert!(from_untrusted(&v).is_err(), "method = {bad:?} 也必须拒绝");
        }
        // 有 method 就放行（A-10：有 confidence 就必须有证据链，故带上最小 signals）
        assert!(from_untrusted(&serde_json::json!({
            "score": 0.9, "method": METHOD, "signals": minimal_signals()
        }))
        .is_ok());
    }

    /// `score` 缺失 / 越界 ⇒ 拒绝；`band` 与分数对不上 ⇒ 拒绝（派生量不许被改）。
    #[test]
    fn confidence_score_and_band_are_validated() {
        assert!(from_untrusted(&serde_json::json!({"method": METHOD})).is_err(), "缺 score");
        for s in [-0.1, 1.1, 2.0] {
            assert!(
                from_untrusted(&serde_json::json!({"method": METHOD, "score": s})).is_err(),
                "score = {s} 越界必须拒绝"
            );
        }
        // band 与 score 一致 ⇒ 通过；不一致 ⇒ 拒绝（A-10：带上最小证据链）
        assert!(from_untrusted(&serde_json::json!({
            "method": METHOD, "score": 0.95, "band": "high", "signals": minimal_signals()
        }))
        .is_ok());
        let err = from_untrusted(&serde_json::json!({
            "method": METHOD, "score": 0.95, "band": "low"
        }))
        .unwrap_err();
        assert!(err.contains("对不上"), "{err}");
        // 杂牌 band 也要拒绝（不是「认不出就当没写」）
        assert!(from_untrusted(&serde_json::json!({
            "method": METHOD, "score": 0.95, "band": "excellent"
        }))
        .is_err());
    }

    /// 缺省字段的语义：`band` 由分数**算**、`data_evidence` 默认 `sampled`、`veto` 可空。
    ///
    /// ★ A-10（第 2 轮）：`signals` **不是**缺省字段 —— 有 `confidence` 就必须有
    /// 证据链（见 [`ac014_confidence_without_evidence_chain_is_rejected`]）。
    /// 所以这里在**带一份最小证据链**的前提下测其余缺省字段；原断言的
    /// band 派生 / `data_evidence` 默认 / `veto` 可空三条**一条没少**，
    /// 只是把「`signals == []`」这条默认值断言换成了「必须有证据」。
    ///
    /// ★ A-13（R5）：`data_evidence` 缺省 = `sampled`（严格侧）⇒ 证据链里必须有
    /// `sample_hit_rate`；而 `data_evidence: none` 的那一半反过来**不许**有它
    /// （用 `no_sample_signals`）。两条断言都还在，只是各自配了自洽的证据形状。
    #[test]
    fn absent_optional_fields_get_derived_not_invented_values() {
        let c = from_untrusted(&serde_json::json!({
            "method": METHOD, "score": 0.72, "signals": minimal_signals()
        }))
        .unwrap();
        assert_eq!(c.band, Band::Middle, "band 是派生量");
        assert_eq!(c.veto, None);
        assert_eq!(c.data_evidence, DataEvidence::Sampled);
        let c = from_untrusted(&serde_json::json!({
            "method": METHOD, "score": 0.1, "data_evidence": "none",
            "signals": no_sample_signals()
        }))
        .unwrap();
        assert_eq!(c.data_evidence, DataEvidence::None);
        assert_eq!(c.band, Band::Rejected);
    }

    /// **A-10（R1）· 有 `confidence` 却缺证据链 ⇒ 拒绝**。
    ///
    /// R1 的原样复现：`{"score":0.99,"method":"evidence-scorecard-v1"}` 一行就能
    /// 冒充「高可信」—— 删字段比改数字更省事，而旧行为会放行。
    ///
    /// 三种「没有证据链」的写法都必须拒：
    ///
    /// | 输入 | 为什么拒 |
    /// | --- | --- |
    /// | 无 `signals` 键 | 分数没有来源 |
    /// | `signals: null` | 显式写了空，同样没有证据 |
    /// | `signals: []` | 空证据链 |
    ///
    /// 变异自证：把 `from_untrusted` 里 `signals` 的 `None | Null` 分支改回
    /// `Vec::new()`（旧实现），本测试第一条立刻红。
    ///
    /// ⚠️ 这**不是**「误伤老提案」：老的人工提案没有 `confidence` 键，
    /// 走不到本函数（见 `test_req108_confidence.rs` 的 legacy 用例）。
    #[test]
    fn ac014_confidence_without_evidence_chain_is_rejected() {
        let cases = [
            serde_json::json!({"method": METHOD, "score": 0.99}),
            serde_json::json!({"method": METHOD, "score": 0.99, "signals": null}),
            serde_json::json!({"method": METHOD, "score": 0.99, "signals": []}),
        ];
        for v in cases {
            let err = from_untrusted(&v).unwrap_err();
            assert!(
                err.contains("有置信度但缺证据链"),
                "文案要说清「有置信度但缺证据链」：{err}"
            );
            assert!(err.contains("不可复核"), "还要说清为什么（不可复核）：{err}");
        }
        // 阴性对照：同一份分数**带上**证据链就放行（否则上面只是「什么都拒」）
        assert!(from_untrusted(&serde_json::json!({
            "method": METHOD, "score": 0.99, "signals": minimal_signals()
        }))
        .is_ok());
    }

    // ── A-13（R5）· 声明了采样证据，就必须有采样信号 ─────────────

    /// QA 实测的原样攻击链：删掉 `sample_hit_rate` + `veto`，四个结构信号凑 0.99。
    ///
    /// 旧行为：`import` / `decide` / `apply` 全部 EXIT=0，快照里变成 `0.99 / high`。
    /// 根因不是「分数算错了」，而是「声明读过库」这句话没有任何信号为它背书。
    ///
    /// 变异自证：把 [`validate_self_evidence`] 第 4 段（R5）删掉，本测试立刻红。
    #[test]
    fn ac013_r5_sampled_without_sample_hit_rate_is_rejected() {
        let attack = serde_json::json!({
            "score": 0.99,
            "method": METHOD,
            "data_evidence": "sampled",
            "signals": [
                {"name": "name_match", "value": 1.0, "weight": 0.2},
                {"name": "type_compatible", "value": 1.0, "weight": 0.1},
                {"name": "target_is_key", "value": 1.0, "weight": 0.1},
                {"name": "not_all_null", "value": 1.0, "weight": 0.15}
            ]
        });
        let err = from_untrusted(&attack).unwrap_err();
        assert!(
            err.contains("声明了采样证据但 signals 里没有 sample_hit_rate"),
            "文案必须点名缺失的那条信号：{err}"
        );
        assert!(err.contains("不可复核"), "还要说清为什么：{err}");

        // `data_evidence` 缺失 ⇒ 按 `sampled`（严格侧）⇒ 同样拒
        let mut no_field = attack.clone();
        no_field.as_object_mut().unwrap().remove("data_evidence");
        let err = from_untrusted(&no_field).unwrap_err();
        assert!(
            err.contains("声明了采样证据但 signals 里没有 sample_hit_rate"),
            "缺省 data_evidence 不能被当成 none 放行：{err}"
        );

        // 换个写法删证据：保留 `sample_hit_rate` 但拿掉 `sampled`/`matched`。
        // 空壳信号推不出任何 veto（`evidence_vetoes` 需要聚合量），等于把这一环删掉。
        let mut hollow = attack.clone();
        hollow["signals"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "sample_hit_rate", "value": 1.0, "weight": 0.45
        }));
        let err = from_untrusted(&hollow).unwrap_err();
        assert!(err.contains("缺 sampled/matched"), "空壳信号不算证据：{err}");
    }

    /// 反向：`data_evidence: none`（没采过样）却带 `sample_hit_rate` ⇒ 拒。
    ///
    /// 不封这一半的话，「声明没采样」与「摆一份采样证据」可以同时成立，
    /// `no_data_evidence` 的 0.45 上限就能被一条自相矛盾的声明绕开。
    #[test]
    fn ac013_r5_none_with_sample_hit_rate_is_rejected() {
        let mut v = serde_json::json!({
            "score": 0.20,
            "method": METHOD,
            "data_evidence": "none",
            "signals": no_sample_signals()
        });
        v["signals"].as_array_mut().unwrap().push(serde_json::json!({
            "name": "sample_hit_rate", "value": 1.0, "weight": 0.45,
            "sampled": 5, "matched": 5
        }));
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("data_evidence = none"), "要点名声明的取值：{err}");
        assert!(err.contains("却又摆出一份采样证据"), "要说清这是自相矛盾：{err}");
    }

    /// **不许误伤** `--no-sample` 的真产出：`data_evidence: none` + 无
    /// `sample_hit_rate` + 三个结构信号 ⇒ 放行，且读回来逐字段相同。
    #[test]
    fn ac013_r5_no_sample_output_still_passes() {
        let no_data = score(&ScoreInput {
            column: "customer_id".into(),
            to_table: "customers".into(),
            from_type: TypeFamily::Str,
            to_type: Some(TypeFamily::Str),
            target_key: TargetKey::Primary,
            data: None,
            composite_target_key: false,
            polymorphic: false,
            self_ref: false,
        });
        assert_eq!(no_data.data_evidence, DataEvidence::None);
        assert!(
            !no_data.signals.iter().any(|s| s.name == "sample_hit_rate"),
            "`--no-sample` 的产出本来就没有采样信号"
        );
        let v = serde_json::to_value(&no_data).unwrap();
        let back = from_untrusted(&v).unwrap_or_else(|e| panic!("真产出被拒了（误伤）：{e}\n{v}"));
        assert_eq!(back, no_data);
    }

    // ── AC-014 · 自证链：分数不能脱离证据 ─────────────────────

    /// 一条「5 行 0 命中」的真实证据五件套（就是 E5 那份的形状）。
    fn zero_hit_signals() -> serde_json::Value {
        serde_json::json!([
            {"name": "sample_hit_rate", "value": 0.0, "weight": 0.45,
             "sampled": 5, "matched": 0, "sample_digest": "sha256:bb"},
            {"name": "name_match", "value": 0.6, "weight": 0.2},
            {"name": "not_all_null", "value": 1.0, "weight": 0.15},
            {"name": "type_compatible", "value": 1.0, "weight": 0.1},
            {"name": "target_is_key", "value": 1.0, "weight": 0.1}
        ])
    }

    fn zero_hit_confidence(score: f64, band: &str) -> serde_json::Value {
        serde_json::json!({
            "score": score,
            "method": METHOD,
            "band": band,
            "signals": zero_hit_signals(),
            "veto": {"signal": "zero_hit", "cap": 0.2, "reason": "采了 5 行、命中 0 行"},
            "data_evidence": "sampled"
        })
    }

    /// **QA 攻击 f 的原样复现**：`score` 0.20 → 0.99，`veto` / `signals` 原样。
    ///
    /// 变异自证：把 [`validate_self_evidence`] 第 4 段（上限判定）删掉，
    /// 本测试立刻红 —— 那时的表现就是 QA 实测的
    /// 「`[高] 0.99 … 证据 0/5 … veto zero_hit 硬反驳`」。
    #[test]
    fn ac014_tampered_score_above_veto_cap_is_rejected() {
        let err = from_untrusted(&zero_hit_confidence(0.99, "high")).unwrap_err();
        assert!(err.contains("0.99"), "要点名被改的分数：{err}");
        assert!(err.contains("上限 0.20"), "要能定位到上限 0.20：{err}");
        assert!(err.contains("不自洽"), "要说出这是「分数与证据不自洽」：{err}");
        assert!(err.contains("sampled=5"), "要指到那一行证据：{err}");
    }

    /// 删掉 `veto` 整段也救不回来：上限由 **signals** 推，不由声明推。
    #[test]
    fn ac014_deleting_the_veto_does_not_rescue_a_high_score() {
        let mut v = zero_hit_confidence(0.99, "high");
        v.as_object_mut().unwrap().remove("veto");
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("超过 veto zero_hit 的上限 0.20"), "{err}");
    }

    /// AC-014 #2：权重改一个数字就拒（0.45 → 0.9）。
    #[test]
    fn ac014_weight_that_differs_from_the_table_is_rejected() {
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["signals"][0]["weight"] = serde_json::json!(0.9);
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("signals 权重与 evidence-scorecard-v1 的权重表不一致"), "{err}");
        assert!(err.contains("0.45"), "要点名表里的值：{err}");
    }

    /// AC-014 #3：保留 `veto: zero_hit`、删掉对应证据信号 ⇒ 拒绝。
    #[test]
    fn ac014_veto_without_its_evidence_signal_is_rejected() {
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["signals"].as_array_mut().unwrap().remove(0); // 拿掉 sample_hit_rate
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("声明了 veto zero_hit"), "{err}");
        assert!(err.contains("没有对应证据"), "{err}");
    }

    /// `veto.cap` 自己也是不可信输入：改大就拒（否则「上限表」可被文件改）。
    #[test]
    fn ac014_veto_cap_cannot_be_raised_by_the_file() {
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["veto"]["cap"] = serde_json::json!(0.99);
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("上限表被改过"), "{err}");
        assert!(err.contains("0.20"), "{err}");

        // 未知 veto 名也不行（没有上限可核）
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["veto"]["signal"] = serde_json::json!("trust_me");
        assert!(from_untrusted(&v).is_err());
    }

    /// 命中率是 `sampled`/`matched` 的查表结果，不是自由变量。
    #[test]
    fn ac014_hit_rate_value_must_match_its_aggregates() {
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["signals"][0]["value"] = serde_json::json!(1.0);
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("查表值"), "{err}");

        // matched > sampled 也拒
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["signals"][0]["matched"] = serde_json::json!(9);
        assert!(from_untrusted(&v).is_err());
    }

    /// 未知 `method` / 权重表之外的信号名 ⇒ 拒绝（否则第 2 条可被绕过）。
    #[test]
    fn ac014_unknown_method_or_signal_name_is_rejected() {
        let mut v = zero_hit_confidence(0.20, "rejected");
        v["method"] = serde_json::json!("my-own-v9");
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("没有对应的权重表"), "{err}");

        let mut v = zero_hit_confidence(0.20, "rejected");
        v["signals"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"name": "zero_hit", "value": 1.0, "weight": 0.0}));
        let err = from_untrusted(&v).unwrap_err();
        assert!(err.contains("权重表之外的信号"), "{err}");
    }

    /// **不许误伤**：真实评分卡产出的五种形状都能原样过闸（且读回来不变）。
    ///
    /// 这条是前面所有「拒绝」判据的对照：只拒被改过的，不拒算出来的。
    #[test]
    fn ac014_every_honest_score_passes_the_gate() {
        let mk = |column: &str, to: &str, data: Option<SampleFacts>, polymorphic: bool| {
            ScoreInput {
                column: column.into(),
                to_table: to.into(),
                from_type: TypeFamily::Str,
                to_type: Some(TypeFamily::Str),
                target_key: TargetKey::Primary,
                data,
                composite_target_key: false,
                polymorphic,
                self_ref: false,
            }
        };
        let facts = |s, m| SampleFacts { sampled: s, matched: m, digest: None, reason: None };
        let cases = vec![
            mk("customer_id", "customers", Some(facts(5, 5)), false),   // 高
            mk("customer_id", "customers", Some(facts(5, 3)), false),   // 中（partial_hit）
            mk("batch_id", "customers", Some(facts(5, 0)), false),      // 拒绝（zero_hit）
            mk("group_id", "customers", Some(facts(0, 0)), false),      // 拒绝（all_null + generic）
            mk("customer_id", "customers", None, false),                // 低（--no-sample）
            mk("target_id", "customers", Some(facts(5, 5)), true),      // 结构 veto（polymorphic）
        ];
        for input in cases {
            let c = score(&input);
            let v = serde_json::to_value(&c).unwrap();
            let back = from_untrusted(&v).unwrap_or_else(|e| {
                panic!("真产出的分数被拒了（误伤）：{e}\n{v}");
            });
            assert_eq!(back, c, "读回来必须逐字段相同");
        }
    }

    /// `score()` 用的每条上限都必须来自 [`VETO_CAPS`]（一张表，不是两份判据）。
    #[test]
    fn ac014_veto_caps_have_exactly_one_source_of_truth() {
        assert_eq!(VETO_CAPS.len(), 9, "设计概要 §3.4 的 8 条 + A-2 的 no_data_evidence");
        assert_eq!(veto_cap("zero_hit"), Some(0.20));
        assert_eq!(veto_cap("type_incompatible"), Some(0.15));
        assert_eq!(veto_cap("partial_hit"), Some(0.75));
        assert_eq!(veto_cap("no_data_evidence"), Some(0.45));
        assert_eq!(veto_cap("nope"), None);

        let facts = |s, m| SampleFacts { sampled: s, matched: m, digest: None, reason: None };
        let mut inputs = vec![];
        for (column, data, poly) in [
            ("customer_id", Some(facts(5, 5)), false),
            ("customer_id", Some(facts(5, 3)), false),
            ("batch_id", Some(facts(5, 0)), false),
            ("group_id", Some(facts(0, 0)), false),
            ("customer_id", None, false),
            ("target_id", Some(facts(5, 5)), true),
        ] {
            inputs.push(ScoreInput {
                column: column.into(),
                to_table: "customers".into(),
                from_type: TypeFamily::Str,
                to_type: Some(TypeFamily::Str),
                target_key: TargetKey::Plain,
                data,
                composite_target_key: false,
                polymorphic: poly,
                self_ref: false,
            });
        }
        inputs.push(ScoreInput { from_type: TypeFamily::Str, to_type: Some(TypeFamily::Num), ..base() });
        inputs.push(ScoreInput { composite_target_key: true, ..base() });
        inputs.push(ScoreInput { self_ref: true, target_key: TargetKey::Plain, ..base() });
        for input in inputs {
            let c = score(&input);
            if let Some(v) = &c.veto {
                assert_eq!(
                    veto_cap(&v.signal),
                    Some(v.cap),
                    "score() 给的 {} 上限与 VETO_CAPS 表不一致（一张表原则）",
                    v.signal
                );
            }
        }
    }

    // ── AC-011 · 校准分箱 ────────────────────────────────────

    /// 五档分箱的**边界**：左闭右开、最后一箱闭合；1.0 不能被丢掉。
    #[test]
    fn calibration_bins_respect_the_edges() {
        let samples = [
            (0.0, true),   // [0.0,0.3)
            (0.29, false), // [0.0,0.3)
            (0.3, true),   // [0.3,0.6)
            (0.6, true),   // [0.6,0.75)
            (0.75, true),  // [0.75,0.9)
            (0.9, true),   // [0.9,1.0]
            (1.0, false),  // [0.9,1.0] ← 上界必须闭合
        ];
        let bins = calibration_bins(&samples);
        assert_eq!(bins.len(), 5, "五档（设计概要 §六）");
        let counts: Vec<usize> = bins.iter().map(|b| b.count).collect();
        assert_eq!(counts, vec![2, 1, 1, 1, 2], "边界归属：{counts:?}");
        assert_eq!(bins.iter().map(|b| b.accepted).sum::<usize>(), 5);
        // 标签与边界可读
        assert_eq!(bins[0].label(), "0.00–0.30");
        assert_eq!(bins[4].label(), "0.90–1.00");
        // 平均分（0.0 与 0.29 的均值 0.145 —— 两位小数取整落 0.14，
        // 因为 0.145 在二进制里是 0.1449999…；这条断言钉的是**取整规则**本身）
        assert!((bins[0].mean_score - 0.14).abs() < 1e-9, "{}", bins[0].mean_score);
        // 越界输入被夹住，不产生第 6 箱
        let odd = calibration_bins(&[(-1.0, false), (2.0, true)]);
        assert_eq!(odd.iter().map(|b| b.count).sum::<usize>(), 2);
        assert_eq!(odd[0].count, 1);
        assert_eq!(odd[4].count, 1);
    }

    /// **样本不足**：任一分箱 < 5 条 ⇒ 不给接受率结论、不算 ECE（AC-011）。
    ///
    /// 变异自证：把 `sufficient()` 改成恒 `true`，`ece` 会变成 `Some`，
    /// 于是「1 条样本」也能印出一个像模像样的 0% / 100% —— 那正是 AC 禁止的事。
    #[test]
    fn calibration_refuses_to_conclude_on_thin_samples() {
        // 每箱都 < 5
        let thin: Vec<(f64, bool)> = (0..4).map(|i| (0.05 + i as f64 * 0.2, i % 2 == 0)).collect();
        let bins = calibration_bins(&thin);
        assert!(bins.iter().any(|b| !b.sufficient()));
        assert_eq!(expected_calibration_error(&bins), None, "样本不足时不算 ECE");

        // 空库（一条已决样本都没有）：每一箱都是 0 条，接受率必须是 None（不是 0%）
        let empty = calibration_bins(&[]);
        assert_eq!(empty.len(), 5);
        assert!(empty.iter().all(|b| b.count == 0));
        assert!(empty.iter().all(|b| b.acceptance_rate().is_none()));
        assert_eq!(expected_calibration_error(&empty), None);

        // 每箱 ≥ 5 ⇒ 才给结论
        let mut thick: Vec<(f64, bool)> = Vec::new();
        for (low, high) in [(0.0, 0.3), (0.3, 0.6), (0.6, 0.75), (0.75, 0.9), (0.9, 1.0)] {
            let mid = (low + high) / 2.0;
            for i in 0..5 {
                // 每箱接受 4/5，平均分是箱中值 ⇒ ECE > 0
                thick.push((mid, i < 4));
            }
        }
        let bins = calibration_bins(&thick);
        assert!(bins.iter().all(|b| b.sufficient()));
        for b in &bins {
            assert_eq!(b.acceptance_rate(), Some(0.8), "5 条里接受 4 条");
        }
        let ece = expected_calibration_error(&bins).expect("样本足够，必须算 ECE");
        assert!(ece > 0.0 && ece < 0.5, "ECE = {ece}");
    }

    /// 同一份样本两次分箱逐字段相同（AC-005 的口径也适用于校准）。
    #[test]
    fn calibration_is_reproducible() {
        let samples: Vec<(f64, bool)> = (0..40).map(|i| ((i as f64) / 40.0, i % 3 == 0)).collect();
        assert_eq!(calibration_bins(&samples), calibration_bins(&samples));
    }
}
