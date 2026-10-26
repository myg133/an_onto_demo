//! ⭐ REQ-157 · **关系动词字典**（`relation-verbs.json`）—— 受控词表 + 校验。
//!
//! # 这解决的是什么病
//!
//! v3 的 34 条关系用了 **15 种动词**，其中三对是同一个意思（`attached_to` /
//! `attaches_to`、`binds` / `binds_to`），另有三个泛词（`uses` / `contains` /
//! `associated_with`）—— 几乎每句话都能套上，抽出来等于没抽。
//!
//! 根因不是 UI，是**抽取契约没有词汇表**：`skills/ontok-extract/SKILL.md` 说
//! 「用具体动宾短语」，而 `src/agent/acp/parse.rs` 的 schema 示例同时把
//! `has` / `belongs_to` / `references` 三个词写死成枚举 —— agent 两头都看，
//! 结果自由发挥。
//!
//! ⇒ 动词来自**一张受控的字典**；字典外的词不许进本体；同义词归一；泛词归零。
//!
//! # 三个消费者（同一份字典，不是三份实现）
//!
//! | 消费者 | 用字典的哪部分 |
//! | --- | --- |
//! | [`crate::domain::view::semantic_cn`] | `label`（客户视图/Studio 的中文标签） |
//! | `ontok-builder relation verbs check` | 全部（`unknown` / `forbidden` / `alias_normalized` / `deprecated`） |
//! | `src/agent/acp/parse.rs` 的抽取契约 | 「从字典的 key 里选一个」（**不枚举**） |
//!
//! # 为什么 `include_str!` 编进二进制
//!
//! 服务面（`ontok`）也要给 `semantic_label` 出中文标签，而**镜像里没有仓库文件**。
//! 运行期读文件会把「服务能不能翻中文」绑到「部署目录里恰好有这份 JSON」上，
//! 而缺文件时的失败是**静默降级**（标签变 `null`，客户看到英文 key）——
//! 那正是 REQ-156 要修的形状。所以**默认那份是嵌入的**；
//! `--verbs <path>` 只是给 builder 试新字典用的覆盖项（⛔ 不影响服务面）。
//!
//! # 加载规则（AC-001：拒绝加载并**点名**）
//!
//! | 规则 | 为什么拒绝而不是「忽略那一项」 |
//! | --- | --- |
//! | 缺 `version` | 字典没有版本 ⇒ 复核时说不清「按哪一版判的」 |
//! | 缺 `verbs` | 空字典会把**所有**动词判成 `unknown`（看起来像数据坏了） |
//! | 主键重复 | `serde_json` 会**静默**用后一个覆盖前一个 ⇒ 少一个动词，没人知道 |
//! | 同一别名出现在两个主键下 | 归一方向有二义性，选谁都可能选错 |
//! | 别名与某个主键同名 | 同上（而且是更难查的那种：两条路径给出不同标签） |
//! | `label` 里含 ASCII 字母 | 标签会带着英文进客户视图 ⇒ 发布闸必然拒发整份视图 |
//! | `forbidden` 与主键/别名冲突 | 同一个词既「允许」又「拒绝」，判据自相矛盾 |
//!
//! ⚠️ **`contains` 不在 `forbidden` 里**：它在字典里是主键，只是标了
//! `deprecated`（保留以兼容 v3）。把它写进 `forbidden` 会让**真实 v3** 直接
//! 判成拒绝，而 v3 是已冻结的数据 —— 那是把「历史数据」判成「新错误」。
//! 所以 `deprecated` 是**警告**，`forbidden` 才是**拒绝**。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

/// 嵌入二进制的字典（相对本文件：`src/domain/` → 仓库根）。
const EMBEDDED: &str = include_str!("../../relation-verbs.json");

/// 字典在仓库里的相对路径 —— 报错/报告里点名用（嵌入那份没有绝对路径）。
pub const EMBEDDED_NAME: &str = "relation-verbs.json";

// ══════════════════════════════════════════════════════════════
// 数据形状
// ══════════════════════════════════════════════════════════════

/// 一个主键（动词）及其别名 / 中文标签 / 弃用说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbEntry {
    /// 主键 —— **进本体的就是它**（`belongs_to` / `attached_to` …）。
    pub key: String,
    /// 中文标签（客户视图 `meaning` / catalog `semantic_label` 用它）。
    pub label: String,
    /// 同义写法：命中 ⇒ 归一到 [`VerbEntry::key`]。
    pub aliases: Vec<String>,
    /// 弃用说明（非空 ⇒ 每次校验都告警，但不拒绝）。
    pub deprecated: Option<String>,
}

/// 整份字典。`verbs` **按 JSON 里的声明顺序**保留 —— 报告要能说「第几项」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbDict {
    pub version: String,
    pub verbs: Vec<VerbEntry>,
    pub forbidden: Vec<String>,
}

/// 字典加载失败。`where_` 是位置（行/列/字节），有的话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictError {
    pub message: String,
    pub where_: Option<String>,
}

impl DictError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), where_: None }
    }

    fn at(mut self, raw: &str, byte: usize) -> Self {
        if self.where_.is_none() {
            let (line, col) = line_col(raw, byte);
            self.where_ = Some(format!("第 {line} 行第 {col} 列（字节 {byte}）"));
        }
        self
    }
}

impl fmt::Display for DictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(w) = &self.where_ {
            write!(f, "（位置：{w}）")?;
        }
        Ok(())
    }
}

impl std::error::Error for DictError {}

/// 一个动词在字典里的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classified<'a> {
    /// 主键本身。
    Primary { key: &'a str, label: &'a str, deprecated: Option<&'a str> },
    /// 别名 ⇒ 归一成 `canonical`。
    Alias { canonical: &'a str, label: &'a str },
    /// 命中 `forbidden`（泛词）。
    Forbidden,
    /// 字典里没有。
    Unknown,
}

// ══════════════════════════════════════════════════════════════
// 加载 + 校验（AC-001）
// ══════════════════════════════════════════════════════════════

static EMBEDDED_DICT: OnceLock<Result<VerbDict, DictError>> = OnceLock::new();

/// 嵌入的那份字典（解析一次、缓存）。
///
/// ⛔ 解析失败**不 panic**：调用方 [`dict_label`] 会退回 `semantic_cn` 的
/// 内置 match（行为与 REQ-156 逐字一致），而「字典坏了」这件事由
/// `relation_verbs_check`（点名到行/列）与单元测试暴露 ——
/// 若在这里 panic，服务面会因为一份静态文件起不来。
pub fn embedded() -> Result<&'static VerbDict, &'static DictError> {
    match EMBEDDED_DICT.get_or_init(|| load_str(EMBEDDED)) {
        Ok(d) => Ok(d),
        Err(e) => Err(e),
    }
}

/// 从一份 JSON 文本加载字典（**纯函数**：不读文件、不碰环境）。
///
/// 失败时错误信息必须能让人直接去改那一行 —— 所以每个分支都点名到
/// **哪个词 / 第几项**，而不是笼统的「字典不合法」。
pub fn load_str(raw: &str) -> Result<VerbDict, DictError> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|e| DictError {
        message: format!("JSON 解析失败：{e}"),
        // serde 自己给的 line/column 比我们算的更准（它知道 `eof` 之类的情形）。
        where_: Some(format!("第 {} 行第 {} 列", e.line(), e.column())),
    })?;
    let root = value
        .as_object()
        .ok_or_else(|| DictError::new("根必须是 JSON 对象（形如 {\"version\":…,\"verbs\":{…}}）"))?;

    // ① 顶层重复键 / ② version
    let members = object_members(raw, 0)?;
    reject_duplicate_keys(raw, &members, "顶层")?;

    let version = match root.get("version") {
        None => {
            return Err(DictError::new(
                "缺少 `version` —— 字典必须自报版本，否则复核时说不清「按哪一版判的」",
            ))
        }
        Some(v) => v.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| {
            DictError::new("`version` 必须是非空字符串（例：\"1.0.0\"）")
        })?,
    };

    // ③ verbs 必须存在且是对象
    let verbs_at = members
        .iter()
        .find(|m| m.key == "verbs")
        .map(|m| m.value_at)
        .ok_or_else(|| {
            DictError::new(
                "缺少 `verbs` —— 空字典会把**所有**动词判成 unknown（看起来像数据坏了），\
                 所以这里拒绝加载而不是当成空表",
            )
        })?;
    let verbs_obj = root
        .get("verbs")
        .and_then(|v| v.as_object())
        .ok_or_else(|| DictError::new("`verbs` 必须是对象（主键 → {label, aliases}）"))?;

    // ④ 主键重复：`serde_json` 会静默覆盖，只能自己扫一遍原文
    let verb_members = object_members(raw, verbs_at)?;
    reject_duplicate_keys(raw, &verb_members, "`verbs`")?;

    // ⑤ 逐个主键的形状
    let mut order: Vec<String> = verb_members.iter().map(|m| m.key.clone()).collect();
    // 防御：serde 的对象按字典序给出，`order` 按原文顺序 —— 两者必须等长，
    // 否则说明扫描器和 serde 对同一份文本理解不一致（应当只发生在手写坏文件上）。
    if order.len() != verbs_obj.len() {
        order = verbs_obj.keys().cloned().collect();
    }

    let mut verbs: Vec<VerbEntry> = Vec::with_capacity(order.len());
    for (idx, key) in order.iter().enumerate() {
        let item = &verbs_obj[key];
        let obj = item.as_object().ok_or_else(|| {
            DictError::new(format!(
                "第 {} 项 `{key}`：值必须是对象（形如 {{\"label\":\"…\",\"aliases\":[…]}}）",
                idx + 1
            ))
        })?;
        let label = obj
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                DictError::new(format!(
                    "第 {} 项 `{key}`：缺少非空 `label`（中文标签，客户视图会直接显示它）",
                    idx + 1
                ))
            })?;
        if label.chars().any(|c| c.is_ascii_alphabetic()) {
            return Err(DictError::new(format!(
                "第 {} 项 `{key}`：`label` 是 `{label}` —— 含 ASCII 字母。\
                 标签会原样进客户视图的 `meaning`，而发布闸的判据就是「值里出现 ASCII 字母 ⇒ 拒发」",
                idx + 1
            )));
        }

        let aliases = match obj.get("aliases") {
            None => Vec::new(),
            Some(v) => {
                let arr = v.as_array().ok_or_else(|| {
                    DictError {
                        message: format!(
                            "第 {} 项 `{key}`：`aliases` 必须是字符串数组（没有就给 []）",
                            idx + 1
                        ),
                        where_: None,
                    }
                })?;
                let mut out = Vec::with_capacity(arr.len());
                for (ai, a) in arr.iter().enumerate() {
                    let s = a.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| {
                        DictError::new(format!(
                            "第 {} 项 `{key}` 的第 {} 个别名：必须是非空字符串",
                            idx + 1,
                            ai + 1
                        ))
                    })?;
                    out.push(s.to_string());
                }
                out
            }
        };

        let deprecated = match obj.get("deprecated") {
            None | Some(serde_json::Value::Null) => None,
            Some(v) => Some(
                v.as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        DictError::new(format!(
                            "第 {} 项 `{key}`：`deprecated` 给了就必须是**非空字符串**\
                             （说明为什么留着它），否则删掉这个字段",
                            idx + 1
                        ))
                    })?
                    .to_string(),
            ),
        };

        verbs.push(VerbEntry {
            key: key.clone(),
            label: label.to_string(),
            aliases,
            deprecated,
        });
    }

    // ⑥ 别名：撞主键 / 自己重复 / 撞另一个主键的别名
    //
    // 顺序是刻意的：**先判「撞主键」再判「撞别名」** —— 否则同一个坏文件
    // 会报出一句「别名出现在两个主键下（depends_on 与 binds_to）」，而真实
    // 病因是「别名就是另一个主键」。错误的病因会把改法指错方向。
    let keys: BTreeMap<&str, ()> = verbs.iter().map(|v| (v.key.as_str(), ())).collect();
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    for (idx, v) in verbs.iter().enumerate() {
        for a in &v.aliases {
            if a == &v.key {
                return Err(DictError::new(format!(
                    "第 {} 项 `{}`：别名 `{a}` 与**主键同名**（自己列自己）—— 删掉这个别名",
                    idx + 1,
                    v.key
                )));
            }
            if keys.contains_key(a.as_str()) {
                return Err(DictError::new(format!(
                    "第 {} 项 `{}`：别名 `{a}` 与主键 `{a}` **同名** —— 同一个词有两条路径\
                     （主键标签 vs 别名标签），归一结果会随查表顺序变",
                    idx + 1,
                    v.key
                )));
            }
            if let Some(other) = owner.get(a.as_str()) {
                if *other == v.key.as_str() {
                    return Err(DictError::new(format!(
                        "第 {} 项 `{}`：别名 `{a}` **在同一主键下重复**",
                        idx + 1,
                        v.key
                    )));
                }
                return Err(DictError::new(format!(
                    "别名 `{a}` 同时出现在两个主键下（`{}` 与 `{}`）—— \
                     归一方向有二义性，选谁都可能选错",
                    other, v.key
                )));
            }
            owner.insert(a.as_str(), v.key.as_str());
        }
    }

    // ⑥·b 中文标签不许重复：`classify` **认标签**（客户视图的 `meaning` 就是标签），
    // 两个主键共用同一个标签 ⇒ 「从视图反查回主键」有二义性。
    let mut label_owner: BTreeMap<&str, &str> = BTreeMap::new();
    for v in &verbs {
        if let Some(other) = label_owner.get(v.label.as_str()) {
            return Err(DictError::new(format!(
                "`{}` 与 `{}` 的 `label` 都是 `{}` —— 客户视图的 `meaning` 就是这个标签，\
                 重复会让「从视图反查主键」有二义性",
                other, v.key, v.label
            )));
        }
        label_owner.insert(v.label.as_str(), v.key.as_str());
    }

    // ⑦ forbidden
    let forbidden = match root.get("forbidden") {
        None => Vec::new(),
        Some(v) => {
            let arr = v
                .as_array()
                .ok_or_else(|| DictError::new("`forbidden` 必须是字符串数组（例：[\"uses\"]）"))?;
            let mut out: Vec<String> = Vec::with_capacity(arr.len());
            for (i, x) in arr.iter().enumerate() {
                let s = x.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| {
                    DictError::new(format!("`forbidden` 的第 {} 项：必须是非空字符串", i + 1))
                })?;
                if out.iter().any(|seen| seen == s) {
                    return Err(DictError::new(format!(
                        "`forbidden` 的第 {} 项 `{s}`：重复出现",
                        i + 1
                    )));
                }
                out.push(s.to_string());
            }
            out
        }
    };
    for f in &forbidden {
        if keys.contains_key(f.as_str()) {
            return Err(DictError::new(format!(
                "`{f}` 同时在 `forbidden` 和**主键**里 —— 同一个词既「允许」又「拒绝」，\
                 判据自相矛盾（也请检查它是不是 `contains` 这类只该标 `deprecated` 的词）"
            )));
        }
        if let Some(o) = owner.get(f.as_str()) {
            return Err(DictError::new(format!(
                "`{f}` 同时在 `forbidden` 和 `{o}` 的**别名**里 —— 同一个词既「允许」又「拒绝」"
            )));
        }
    }

    Ok(VerbDict { version: version.to_string(), verbs, forbidden })
}

// ══════════════════════════════════════════════════════════════
// 查询（服务面 / builder 共用）
// ══════════════════════════════════════════════════════════════

impl VerbDict {
    /// 主键 + 别名的总条数（报告抬头用）。
    pub fn alias_count(&self) -> usize {
        self.verbs.iter().map(|v| v.aliases.len()).sum()
    }

    /// 这个写法是不是**主键或别名**；是就给出顺序里的位置。
    ///
    /// ⚠️ **中文标签也算命中**：`relation verbs check --snapshot` 也吃**客户视图**
    /// （`relations[].meaning` 已经过 `semantic_cn`，是中文标签）。不认标签的话，
    /// 一份**完全正确**的客户视图会被报出 34 条 `unknown` —— 那条命令就没人敢用。
    /// 标签匹配只用于「回查主键」，⛔ 不产生任何新标签。
    pub fn classify(&self, s: &str) -> Classified<'_> {
        for v in &self.verbs {
            if v.key == s {
                return Classified::Primary {
                    key: &v.key,
                    label: &v.label,
                    deprecated: v.deprecated.as_deref(),
                };
            }
            if v.aliases.iter().any(|a| a == s) {
                return Classified::Alias { canonical: &v.key, label: &v.label };
            }
        }
        for v in &self.verbs {
            if v.label == s {
                return Classified::Primary {
                    key: &v.key,
                    label: &v.label,
                    deprecated: v.deprecated.as_deref(),
                };
            }
        }
        if self.forbidden.iter().any(|f| f == s) {
            return Classified::Forbidden;
        }
        Classified::Unknown
    }

    /// 中文标签（主键或别名命中）。`semantic_cn` 的第一档就是它。
    pub fn label_of(&self, s: &str) -> Option<&str> {
        match self.classify(s) {
            Classified::Primary { label, .. } | Classified::Alias { label, .. } => Some(label),
            _ => None,
        }
    }

    /// 归一后的主键（主键自己 ⇒ 自己；别名 ⇒ 主键；其余 ⇒ `None`）。
    pub fn canonical_of(&self, s: &str) -> Option<&str> {
        match self.classify(s) {
            Classified::Primary { key, .. } => Some(key),
            Classified::Alias { canonical, .. } => Some(canonical),
            _ => None,
        }
    }

    /// 最接近的 `n` 个主键（编辑距离；距离相同按字母序，保证确定性）。
    ///
    /// **距离对「主键 + 它的全部别名」取最小值** —— 否则 `attaches_to`（别名）
    /// 会给出一串看不出关系的候选，而人想看到的答案就是 `attached_to`。
    pub fn candidates(&self, word: &str, n: usize) -> Vec<String> {
        let mut scored: Vec<(usize, &str)> = self
            .verbs
            .iter()
            .map(|v| {
                let mut best = levenshtein(word, &v.key);
                for a in &v.aliases {
                    best = best.min(levenshtein(word, a));
                }
                (best, v.key.as_str())
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        scored.into_iter().take(n).map(|(_, k)| k.to_string()).collect()
    }
}

/// 服务面用的**唯一**入口：字典命中就给中文标签，否则 `None`。
///
/// ⛔ 字典坏了/缺字段时**不 panic**：退回 `semantic_cn` 的内置 match
/// （REQ-156 的 6 + 11 条映射），行为与未接入字典时逐字一致。
pub fn dict_label(s: &str) -> Option<String> {
    embedded().ok().and_then(|d| d.label_of(s)).map(|s| s.to_string())
}

// ══════════════════════════════════════════════════════════════
// 校验报告（AC-002 / AC-003 / AC-004）
// ══════════════════════════════════════════════════════════════

/// 一条关系在来源里的位置（报告用它点名，让 FDE 能直接去改）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// 人可读的定位串（`link_types[3] key=alpha_beta from=alpha to=beta`）。
    pub location: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// 一类问题的种类。`blocking()` 决定退出码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssueKind {
    Unknown,
    Forbidden,
    Deprecated,
    AliasNormalized,
}

impl IssueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            IssueKind::Unknown => "unknown",
            IssueKind::Forbidden => "forbidden",
            IssueKind::Deprecated => "deprecated",
            IssueKind::AliasNormalized => "alias_normalized",
        }
    }

    /// 只有这两类让 `relation verbs check` **非 0 退出**。
    pub const fn blocking(self) -> bool {
        matches!(self, IssueKind::Unknown | IssueKind::Forbidden)
    }
}

/// 同一种写法在来源里出现的所有位置（按写法归并，报告不刷屏）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: IssueKind,
    /// 来源里**原样**的那个词。
    pub semantic: String,
    /// 归一后的主键（`alias_normalized` 才有）。
    pub canonical: Option<String>,
    /// 中文标签（认得出来才有）。
    pub label: Option<String>,
    /// `deprecated` 的说明（`deprecated` 才有）。
    pub deprecated: Option<String>,
    /// `unknown` 的 3 个候选。
    pub candidates: Vec<String>,
    pub occurrences: Vec<Occurrence>,
}

impl Finding {
    /// 出现次数（`_ =>` 口径：同一写法出现 N 条关系就是 N）。
    pub fn count(&self) -> usize {
        self.occurrences.len()
    }
}

/// 一次校验的完整结论（纯数据，渲染在 CLI）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// 扫过的关系总数。
    pub relations: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    /// 逐条分类 + 归并。`occurrences` 的顺序决定报告里的出现顺序（稳定）。
    pub fn build(dict: &VerbDict, occurrences: Vec<(String, Occurrence)>) -> Self {
        let relations = occurrences.len();
        // kind → semantic → finding（BTreeMap 保证确定性输出）
        let mut buckets: BTreeMap<(IssueKind, String), Finding> = BTreeMap::new();
        for (semantic, occ) in occurrences {
            let (kind, canonical, label, deprecated, candidates) = match dict.classify(&semantic) {
                Classified::Primary { key: _, label, deprecated } => match deprecated {
                    Some(why) => (
                        IssueKind::Deprecated,
                        None,
                        Some(label.to_string()),
                        Some(why.to_string()),
                        Vec::new(),
                    ),
                    // 字典内、非弃用 ⇒ 不是问题，直接跳过
                    None => continue,
                },
                Classified::Alias { canonical, label } => (
                    IssueKind::AliasNormalized,
                    Some(canonical.to_string()),
                    Some(label.to_string()),
                    None,
                    Vec::new(),
                ),
                Classified::Forbidden => (IssueKind::Forbidden, None, None, None, Vec::new()),
                Classified::Unknown => (
                    IssueKind::Unknown,
                    None,
                    None,
                    None,
                    dict.candidates(&semantic, 3),
                ),
            };
            let e = buckets.entry((kind, semantic.clone())).or_insert_with(|| Finding {
                kind,
                semantic: semantic.clone(),
                canonical,
                label,
                deprecated,
                candidates,
                occurrences: Vec::new(),
            });
            e.occurrences.push(occ);
        }
        Self { relations, findings: buckets.into_values().collect() }
    }

    pub fn count(&self, kind: IssueKind) -> usize {
        self.findings.iter().filter(|f| f.kind == kind).count()
    }

    /// 这一类**关系条数**（同一写法出现 3 次算 3）。
    pub fn relation_count(&self, kind: IssueKind) -> usize {
        self.findings.iter().filter(|f| f.kind == kind).map(|f| f.count()).sum()
    }

    /// 有 `unknown` 或 `forbidden` ⇒ 必须非 0 退出（fail-closed）。
    pub fn blocking(&self) -> bool {
        self.findings.iter().any(|f| f.kind.blocking())
    }

    /// 完全干净的条数（既不在字典里也不是任何一类问题）。
    pub fn clean(&self) -> usize {
        let dirty: usize = self.findings.iter().map(|f| f.count()).sum();
        self.relations.saturating_sub(dirty)
    }

    /// JSON 形态（`--json` 用；人话那份在 CLI）。
    pub fn to_json(&self) -> serde_json::Value {
        let issues: Vec<serde_json::Value> = self
            .findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "kind": f.kind.as_str(),
                    "semantic": f.semantic,
                    "count": f.count(),
                    "canonical": f.canonical,
                    "label": f.label,
                    "deprecated": f.deprecated,
                    "candidates": f.candidates,
                    "occurrences": f.occurrences.iter().map(|o| serde_json::json!({
                        "location": o.location,
                        "from": o.from,
                        "to": o.to,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        serde_json::json!({
            "relations": self.relations,
            "clean": self.clean(),
            "blocking": self.blocking(),
            "counts": {
                "unknown": self.count(IssueKind::Unknown),
                "forbidden": self.count(IssueKind::Forbidden),
                "deprecated": self.count(IssueKind::Deprecated),
                "alias_normalized": self.count(IssueKind::AliasNormalized),
            },
            "issues": issues,
        })
    }
}

// ══════════════════════════════════════════════════════════════
// JSON 原文扫描（只为「主键重复」这一条 —— serde 会静默覆盖）
// ══════════════════════════════════════════════════════════════

/// 对象的一个直接成员。
#[derive(Debug, Clone)]
struct Member {
    key: String,
    key_at: usize,
    value_at: usize,
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && matches!(b[*i], b' ' | b'\t' | b'\n' | b'\r') {
        *i += 1;
    }
}

/// 读一个 JSON 字符串（`i` 指向开引号；返回后停在收尾引号之后）。
fn read_string(raw: &str, i: &mut usize) -> Result<String, String> {
    let b = raw.as_bytes();
    if *i >= b.len() || b[*i] != b'"' {
        return Err(format!("期望字符串的开引号（字节 {i}）"));
    }
    *i += 1;
    let mut out = String::new();
    while *i < b.len() {
        match b[*i] {
            b'\\' => {
                *i += 1;
                if *i >= b.len() {
                    return Err("字符串里的转义没有收尾".to_string());
                }
                // 键名是 ASCII（动词都是 `[a-z_]`），转义只需原样吃掉一个字节。
                out.push(b[*i] as char);
                *i += 1;
            }
            b'"' => {
                *i += 1;
                return Ok(out);
            }
            _ => {
                let ch = raw[*i..].chars().next().ok_or("非法 UTF-8 边界")?;
                out.push(ch);
                *i += ch.len_utf8();
            }
        }
    }
    Err("字符串没有收尾引号".to_string())
}

/// 跳过任意一个 JSON 值（用于「只关心直接成员」的扫描）。
fn skip_value(raw: &str, i: &mut usize) -> Result<(), String> {
    let b = raw.as_bytes();
    skip_ws(b, i);
    if *i >= b.len() {
        return Err("值缺失（文件提前结束）".to_string());
    }
    match b[*i] {
        b'"' => read_string(raw, i).map(|_| ()),
        b'{' | b'[' => {
            let open = b[*i];
            let close = if open == b'{' { b'}' } else { b']' };
            let mut depth = 0i32;
            loop {
                if *i >= b.len() {
                    return Err("容器没有收尾".to_string());
                }
                match b[*i] {
                    b'"' => {
                        read_string(raw, i)?;
                    }
                    c if c == open => {
                        depth += 1;
                        *i += 1;
                    }
                    c if c == close => {
                        depth -= 1;
                        *i += 1;
                        if depth == 0 {
                            return Ok(());
                        }
                    }
                    _ => *i += 1,
                }
            }
        }
        _ => {
            let start = *i;
            while *i < b.len()
                && !matches!(b[*i], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            {
                *i += 1;
            }
            if *i == start {
                return Err(format!("无法识别的值（字节 {start}）"));
            }
            Ok(())
        }
    }
}

/// `raw[start]` 必须是一个 `{`；返回它的**直接成员**（按出现顺序，含重复键）。
fn object_members(raw: &str, start: usize) -> Result<Vec<Member>, DictError> {
    let b = raw.as_bytes();
    let mut i = start;
    skip_ws(b, &mut i);
    if i >= b.len() || b[i] != b'{' {
        return Err(DictError::new(format!("期望一个对象（`{{`）在字节 {start} 处")));
    }
    i += 1;
    let mut out = Vec::new();
    loop {
        skip_ws(b, &mut i);
        if i >= b.len() {
            return Err(DictError::new("对象没有收尾（文件提前结束）").at(raw, i));
        }
        if b[i] == b'}' {
            return Ok(out);
        }
        let key_at = i;
        let key = read_string(raw, &mut i).map_err(|e| DictError::at(DictError::new(e), raw, i))?;
        skip_ws(b, &mut i);
        if i >= b.len() || b[i] != b':' {
            return Err(DictError::new(format!("键 `{key}` 之后缺少 `:`")).at(raw, i));
        }
        i += 1;
        skip_ws(b, &mut i);
        let value_at = i;
        skip_value(raw, &mut i).map_err(|e| DictError::at(DictError::new(e), raw, value_at))?;
        out.push(Member { key, key_at, value_at });
        skip_ws(b, &mut i);
        if i >= b.len() {
            return Err(DictError::new("对象没有收尾（文件提前结束）").at(raw, i));
        }
        match b[i] {
            b',' => i += 1,
            b'}' => return Ok(out),
            _ => {
                return Err(DictError::new("成员之间缺少 `,`").at(raw, i));
            }
        }
    }
}

fn reject_duplicate_keys(raw: &str, members: &[Member], scope: &str) -> Result<(), DictError> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for m in members {
        if let Some(first) = seen.get(m.key.as_str()) {
            return Err(DictError::new(format!(
                "{scope} 的键 `{}` 重复（第 1 次在字节 {first}，第 2 次在这里）—— \
                 `serde_json` 会静默用后一个覆盖前一个，少掉的动词没人看得见",
                m.key
            ))
            .at(raw, m.key_at));
        }
        seen.insert(m.key.as_str(), m.key_at);
    }
    Ok(())
}

fn line_col(raw: &str, byte: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in raw.char_indices() {
        if i >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Levenshtein 编辑距离（⚠️ 自己写，⛔ 不引 crate —— AC 的硬约束）。
///
/// 只给「候选提示」用：小写比较，够用就行。
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur: Vec<usize> = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> VerbDict {
        load_str(EMBEDDED).expect("嵌入的 relation-verbs.json 必须能加载")
    }

    // ── AC-001 · 字典加载与拒绝 ──────────────────────────────

    #[test]
    fn embedded_dictionary_loads_and_has_the_sixteen_verbs() {
        let d = embedded().expect("嵌入字典必须能加载");
        assert_eq!(d.version, "1.0.0");
        assert_eq!(d.verbs.len(), 16, "§三 的字典有 16 个主键");
        assert_eq!(d.forbidden.len(), 6);
        assert!(d.verbs.iter().any(|v| v.key == "contains" && v.deprecated.is_some()));
        // ⛔ `contains` 只标 deprecated，⛔ 不进 forbidden（否则真实 v3 被判成拒绝）
        assert!(!d.forbidden.iter().any(|f| f == "contains"));
    }

    #[test]
    fn missing_version_is_rejected_and_named() {
        let raw = r#"{"verbs":{"has":{"label":"拥有","aliases":[]}}}"#;
        let e = load_str(raw).expect_err("缺 version 必须拒绝");
        assert!(e.to_string().contains("version"), "错误要点名 version：{e}");
    }

    #[test]
    fn missing_verbs_is_rejected_and_named() {
        let raw = r#"{"version":"1.0.0","forbidden":[]}"#;
        let e = load_str(raw).expect_err("缺 verbs 必须拒绝");
        assert!(e.to_string().contains("verbs"), "错误要点名 verbs：{e}");
    }

    /// ⛔ 同一别名出现在两个主键下 ⇒ 拒绝并点名**那个别名 + 两个主键**。
    #[test]
    fn one_alias_under_two_primary_keys_is_rejected_and_named() {
        let raw = r#"{
            "version": "1.0.0",
            "verbs": {
                "belongs_to": {"label": "归属于", "aliases": ["binds"]},
                "binds_to":   {"label": "绑定到", "aliases": ["binds"]}
            },
            "forbidden": []
        }"#;
        let e = load_str(raw).expect_err("同一别名出现在两个主键下必须拒绝");
        let msg = e.to_string();
        assert!(msg.contains("binds"), "要点名那个别名：{msg}");
        assert!(msg.contains("belongs_to") && msg.contains("binds_to"), "要点名两个主键：{msg}");
    }

    /// ⛔ 别名与某个主键同名 ⇒ 拒绝并点名。
    #[test]
    fn alias_colliding_with_a_primary_key_is_rejected_and_named() {
        let raw = r#"{
            "version": "1.0.0",
            "verbs": {
                "binds_to": {"label": "绑定到", "aliases": ["depends_on"]},
                "depends_on": {"label": "依赖", "aliases": []}
            },
            "forbidden": []
        }"#;
        let e = load_str(raw).expect_err("别名撞主键必须拒绝");
        let msg = e.to_string();
        assert!(msg.contains("depends_on"), "要点名撞车的词：{msg}");
        assert!(msg.contains("binds_to"), "要点名是谁的别名：{msg}");
        assert!(msg.contains("同名"), "错误要说明是「同名」：{msg}");
    }

    /// `serde_json` 会静默覆盖重复键 —— 所以必须自己扫原文。
    #[test]
    fn duplicate_primary_key_is_rejected_and_located() {
        let raw = r#"{
            "version": "1.0.0",
            "verbs": {
                "has": {"label": "拥有", "aliases": []},
                "has": {"label": "有了", "aliases": []}
            },
            "forbidden": []
        }"#;
        let e = load_str(raw).expect_err("重复主键必须拒绝");
        let msg = e.to_string();
        assert!(msg.contains("`has`"), "要点名重复的主键：{msg}");
        assert!(msg.contains("第 "), "要给出位置：{msg}");
    }

    #[test]
    fn malformed_json_is_located() {
        let e = load_str("{\n  \"version\": \"1.0.0\",\n  \"verbs\": {,}\n}").expect_err("坏 JSON 必须拒绝");
        let msg = e.to_string();
        assert!(msg.contains("第 3 行"), "解析失败要点名到行：{msg}");
    }

    #[test]
    fn label_with_ascii_letters_is_rejected() {
        let raw = r#"{"version":"1.0.0","verbs":{"has":{"label":"has","aliases":[]}}}"#;
        let e = load_str(raw).expect_err("label 含 ASCII 字母必须拒绝");
        assert!(e.to_string().contains("ASCII"), "{e}");
    }

    #[test]
    fn forbidden_colliding_with_a_primary_key_is_rejected() {
        let raw = r#"{
            "version": "1.0.0",
            "verbs": {"uses": {"label": "使用", "aliases": []}},
            "forbidden": ["uses"]
        }"#;
        let e = load_str(raw).expect_err("同一个词既允许又拒绝必须拒载");
        assert!(e.to_string().contains("forbidden"), "{e}");
    }

    /// 中文标签重复 ⇒ 拒绝（客户视图 `meaning` 就是标签，反查会二义）。
    #[test]
    fn duplicate_label_is_rejected_and_named() {
        let raw = r#"{
            "version": "1.0.0",
            "verbs": {
                "belongs_to": {"label": "属于", "aliases": []},
                "attached_to": {"label": "属于", "aliases": []}
            },
            "forbidden": []
        }"#;
        let e = load_str(raw).expect_err("重复 label 必须拒绝");
        let msg = e.to_string();
        assert!(msg.contains("属于"), "要点名重复的标签：{msg}");
        assert!(msg.contains("belongs_to") && msg.contains("attached_to"), "要点名两个主键：{msg}");
    }

    // ── 分类 ────────────────────────────────────────────────

    #[test]
    fn classify_covers_primary_alias_forbidden_and_unknown() {
        let d = dict();
        assert!(matches!(d.classify("belongs_to"), Classified::Primary { .. }));
        match d.classify("attaches_to") {
            Classified::Alias { canonical, .. } => assert_eq!(canonical, "attached_to"),
            other => panic!("attaches_to 应当是别名：{other:?}"),
        }
        assert_eq!(d.classify("uses"), Classified::Forbidden);
        assert_eq!(d.classify("xyz_unknown"), Classified::Unknown);
        assert_eq!(d.label_of("binds"), Some("绑定到"));
        assert_eq!(d.canonical_of("binds"), Some("binds_to"));
        assert_eq!(d.label_of("uses"), None, "泛词没有中文标签可给");
        // 标签也要能回查（客户视图的 `meaning` 就是标签）：一份正确的客户视图
        // 不许被报成 unknown。
        assert_eq!(d.label_of("归属于"), Some("归属于"));
        match d.classify("挂载于") {
            Classified::Primary { key, .. } => assert_eq!(key, "attached_to"),
            other => panic!("中文标签应能回查主键：{other:?}"),
        }
    }

    /// REQ-156 的 15 个真实动词标签**一个都不变**（AC-005）。
    #[test]
    fn the_fifteen_real_verbs_keep_the_req156_labels() {
        let d = dict();
        for (raw, want) in [
            ("belongs_to", "归属于"),
            ("references", "关联到"),
            ("binds_to", "绑定到"),
            ("binds", "绑定到"),
            ("attached_to", "挂载于"),
            ("attaches_to", "挂载于"),
            ("notifies_via", "通知到"),
            ("applies_to", "适用于"),
            ("targets", "指向"),
            ("bills", "计费到"),
            ("based_on", "基于"),
            ("deployed_to", "部署于"),
            ("has", "拥有"),
            ("depends_on", "依赖"),
            ("records", "记录了"),
            ("derived_from", "算自"),
            ("contains", "包含"),
        ] {
            assert_eq!(d.label_of(raw), Some(want), "字典里 {raw} 的标签变了");
        }
        // `uses` / `associated_with` 是 forbidden，字典**不给**标签
        // ⇒ 它们仍由 `semantic_cn` 的内置 match 给「使用」「关联」（REQ-156 行为不变）。
        assert_eq!(d.label_of("uses"), None);
        assert_eq!(d.label_of("associated_with"), None);
    }

    #[test]
    fn candidates_are_three_and_meaningful() {
        let d = dict();
        let c = d.candidates("xyz_unknown", 3);
        assert_eq!(c.len(), 3, "AC-004 要求恰好 3 个候选：{c:?}");
        // 别名也算距离：`attaches_to` 的最近邻就是它的主键。
        let c = d.candidates("attaches_to", 3);
        assert_eq!(c[0], "attached_to", "别名的最接近候选应当是它的主键：{c:?}");
        // 确定性：同一输入两次相同。
        assert_eq!(d.candidates("attaches_to", 3), d.candidates("attaches_to", 3));
    }

    #[test]
    fn levenshtein_is_not_vacuous() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("belongs_to", "belong_to"), 1);
    }

    // ── 报告 ────────────────────────────────────────────────

    fn occ(i: usize, semantic: &str) -> (String, Occurrence) {
        (
            semantic.to_string(),
            Occurrence {
                location: format!("link_types[{i}]"),
                from: Some("a".into()),
                to: Some("b".into()),
            },
        )
    }

    #[test]
    fn report_groups_by_kind_and_counts_occurrences() {
        let d = dict();
        let r = Report::build(
            &d,
            vec![
                occ(0, "uses"),
                occ(1, "uses"),
                occ(2, "attaches_to"),
                occ(3, "contains"),
                occ(4, "xyz_unknown"),
                occ(5, "belongs_to"),
            ],
        );
        assert_eq!(r.relations, 6);
        assert_eq!(r.count(IssueKind::Forbidden), 1, "`uses` 两次只算一类");
        assert_eq!(r.relation_count(IssueKind::Forbidden), 2);
        assert_eq!(r.count(IssueKind::AliasNormalized), 1);
        assert_eq!(r.count(IssueKind::Deprecated), 1);
        assert_eq!(r.count(IssueKind::Unknown), 1);
        assert_eq!(r.clean(), 1, "belongs_to 是字典内的干净词");
        assert!(r.blocking());
        let j = r.to_json();
        assert_eq!(j["counts"]["forbidden"], 1);
        // 顺序按 (kind, semantic)：Unknown < Forbidden < Deprecated < AliasNormalized
        // —— 会**拒绝**的两类排在最前，告警/提示在后（人先看要改哪条）。
        let kinds: Vec<&str> =
            j["issues"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, vec!["unknown", "forbidden", "deprecated", "alias_normalized"]);
        assert_eq!(j["issues"][0]["candidates"].as_array().unwrap().len(), 3);
    }

    /// 干净的字典内动词 ⇒ 零 issue、`blocking() == false`。
    #[test]
    fn report_is_clean_for_in_dictionary_verbs() {
        let d = dict();
        let r = Report::build(&d, vec![occ(0, "belongs_to"), occ(1, "binds_to")]);
        assert!(r.findings.is_empty(), "{:?}", r.findings);
        assert!(!r.blocking());
        assert_eq!(r.clean(), 2);
    }
}
