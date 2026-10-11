//! **给 ACP 子进程发什么、把它回的话读成什么**（REQ-023 阶段 1 的协议侧）。
//!
//! # 本模块最重要的一个设计决定：**能自己查的事实，绝不问模型**
//!
//! 阶段 1 的输出契约（[`crate::agent::CandidateType`]）里有些字段是
//! **源库的事实**，不是 agent 的判断：
//!
//! | 字段 | 它到底是什么 | 本模块怎么做 |
//! | --- | --- | --- |
//! | `property.source` | 「属性 ← 来源列」的可核对锚点（设计点③） | 模型给 `(表,列)`，**我在本组 schema 里核**；核不到就报错 |
//! | `property.nullable` | **那一列在源库里可不可空** | **我自己从 schema 填**，不告诉模型有这个东西 |
//! | `evidence` | 「由源库表结构抽取」 | **我自己造**（形状与规则实现一致） |
//!
//! 为什么这三行这么重要：本项目的核心纪律是「**不造计量**」。
//! `nullable` 在输入里就有答案 —— 问模型等于**为了拿一个已知事实去烧 token**，
//! 还附带一个「模型可能答错」的风险，而那个风险会**原样**出现在客户视图里
//! （`assemble::entity_item` 的注释写得很直白：那是**给人看的合同**）。
//!
//! # 另一半：有哪些字段**问不出来**，于是老老实实留 `None`
//!
//! | 字段 | 为什么问不出来 |
//! | --- | --- |
//! | `property.origin` | 「这个名字是**怎么来的**」。三个取值（`Comment` / `WordList` / `Relation`）描述的是**规则管线的三条命名路径**。模型在黑盒里起了个名字，它**没有任何渠道**告诉我们那是照抄注释、还是从关系推的、还是自己造的 |
//! | `type.rejected_comments` | REQ-005 的**质量门槛拒用计数**。本 adapter **没有实现那个门槛**（那个门槛是规则管线的一部分） |
//!
//! 这两个字段正是 `rebuilds_proposal` 的重建路径**逐字段核**的东西。
//! 编一个值填进去，就是 `stats.props_via_*` 与 `stats.props_comment_rejected`
//! 里的一组**编造的数字** —— 而 stats.json 是 E2E 判读器读的东西。
//! **宁可留 `None` 让重建 fail-closed。** 完整论证见 [`crate::agent::acp`] 模块头。
//!
//! # 为什么 prompt 里给的 (表,列) 是**封闭清单**
//!
//! 设计点③要「证据必须机器可验证」。模型自由发挥写出的列名**不可验证**
//! —— 不是「难验证」，是**根本没有那一列**。所以 prompt 把本组的
//! `(表, 列)` 全列出来当**白名单**，解析时再核一遍。
//! 两道关不是为了「严谨」：prompt 那一道提高命中率，解析那一道保证
//! **命中不了就报错**，而不是把编造的列名塞进 `source` 让阶段 2 去发现。

use anyhow::{bail, Context, Result};
use serde_json::json;

use super::super::group::{
    CandidateLink, CandidateProperty, CandidateType, ColumnRef, ExtractionGroup, GroupExtraction,
};

/// 把本组的五个输入（规格 §7.1 阶段 1 的原文清单）编成一条 prompt。
///
/// 规格原文：「schema 片段 + 样本 + 决策清单约束 + 领域提示 + 已有类型清单」。
/// 五个都在下面，一项不缺 —— 少一项模型就只能猜，而「猜」在溯源里长得
/// 和「想清楚了」一模一样。
pub fn build_prompt(group: &ExtractionGroup) -> String {
    let mut p = String::new();

    p.push_str("你是数据库本体抽取助手。下面给你**一组**表（同一张表），请为它给出本体类型候选。\n\n");

    p.push_str(&format!("## 作用域（只能抽这些表，共 {} 张）\n", group.schema.len()));
    for t in &group.schema {
        p.push_str(&format!(
            "\n### 表 {}{}\n",
            t.name,
            t.comment
                .as_deref()
                .filter(|c| !c.trim().is_empty())
                .map(|c| format!("（注释：{c}）"))
                .unwrap_or_default()
        ));
        p.push_str("| 列名 | 类型 | 可空 | 主键 | 列注释 |\n| --- | --- | --- | --- | --- |\n");
        for c in &t.columns {
            p.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                c.name,
                c.data_type,
                if c.nullable { "是" } else { "否" },
                if c.is_primary_key { "是" } else { "否" },
                c.comment.as_deref().unwrap_or("").replace('|', "/")
            ));
        }
    }

    if !group.context_tables.is_empty() {
        p.push_str("\n## 上下文（全库表名，只用于判断外键指向哪张**真实存在**的表，**不要**抽这些表）\n");
        p.push_str(&group.context_tables.join(", "));
        p.push('\n');
    }

    if !group.samples.is_empty() {
        p.push_str("\n## 样本值（脱敏后）\n");
        for s in &group.samples {
            p.push_str(&format!(
                "- {}.{}: {}\n",
                s.table,
                s.column,
                if s.values.is_empty() {
                    "(无)".to_string()
                } else {
                    s.values.join(" / ")
                }
            ));
        }
    }

    let c = &group.constraints;
    p.push_str("\n## 硬约束（必须遵守）\n");
    if !c.forbidden_terms.is_empty() {
        p.push_str(&format!(
            "- 禁用词（**任何字段里都不许出现**）：{}\n",
            c.forbidden_terms.join("、")
        ));
    }
    if c.require_verifiable_evidence {
        p.push_str("- 每个属性都必须来自下面列出的**真实列**，不许发明列名\n");
    }
    p.push_str(&format!(
        "- 采样值至少要命中 {} 个才算证据成立\n",
        c.min_sample_match
    ));

    if !group.domain_hints.is_empty() {
        p.push_str("\n## 领域提示\n");
        for h in &group.domain_hints {
            p.push_str(&format!("- {h}\n"));
        }
    }

    p.push_str("\n## 已有类型（**优先复用**，不要重新造）\n");
    if group.existing_types.is_empty() {
        p.push_str("（无 —— 本次提出的类型都是新类型）\n");
    } else {
        for t in &group.existing_types {
            p.push_str(&format!("- key={} 显示名={}\n", t.key, t.display_name));
        }
    }

    p.push_str(OUTPUT_CONTRACT);
    p
}

/// 输出契约 —— 刻意**短**，且只问「模型的判断」，不问「schema 里的事实」。
const OUTPUT_CONTRACT: &str = r#"
## 输出格式（**严格照办**）

只输出**一个** JSON 对象。不要 markdown 代码围栏（```），不要任何解释文字，不要前后缀。

{"types": [
  {"key": "稳定标识",
   "display_name": "中文显示名",
   "is_new": true,
   "description": "实体说明",
   "aliases": ["别名"],
   "properties": [
     {"name": "中文属性名", "table": "表名", "column": "列名", "note": "限定说明或值域"}
   ]}
 ],
 "links": [
  {"key": "关系稳定标识",
   "target": "关系中文标签",
   "from_type": "起点类型的 key",
   "to_type": "终点类型的 key",
   "cardinality": "N:1 或 1:N",
   "semantic": "has 或 belongs_to 或 references"}
 ]}

规则：
1. `table` / `column` **必须**逐字取自上面「作用域」里列出的列。不许发明、不许改大小写。
2. `is_new` 为 false 时，`key` **必须**逐字取自「已有类型」的 key；否则 `is_new` 为 true 且给一个新 key。
3. `aliases` 与 `note` 没有内容就给空数组 / 空串。
4. 不确定就不提：宁可少提一个属性，也不要编一个不存在的列。
"#;

// ──────────────────────────────────────────────────────────
// 读回：文本 → GroupExtraction
// ──────────────────────────────────────────────────────────

/// 从自由文本里取出那个 JSON 对象。
///
/// # 为什么要有这一层「宽容」
///
/// 实测与经验都指向同一件事：模型即使被要求「只输出 JSON」，也经常包一层
/// ```json 围栏、或前后加一句「好的，以下是结果：」。
/// 这**不是**要不要宽容的问题 —— 是「宽容解析」与「解析失败」这两个结果里，
/// 前者还带着模型真正的判断，后者什么都拿不到。
///
/// 但宽容**有边界**：只剥围栏、只找最外层的那对花括号，
/// **不**做「猜一个像 JSON 的片段」这种猜测。
///
/// 找花括号时**扫字符串字面量**（跳过 `"` 内部并处理 `\\` 转义）：
/// 属性名里出现一个 `}` 是完全正常的（例如描述文本里写了 `}`），
/// 而 `find('}')` 会在这里截断出一个**语法合法但内容残缺**的片段 ——
/// 那比解析失败糟得多。
pub fn extract_json_object(text: &str) -> Result<&str> {
    let t = text.trim();

    // 1) ```json … ``` 围栏（最常见）
    if let Some(inner) = strip_code_fence(t) {
        return Ok(inner);
    }
    // 2) 裸 JSON
    if t.starts_with('{') {
        return Ok(t);
    }
    // 3) 前后有别的字 → 找最外层的那对花括号
    match outermost_object(t) {
        Some(s) => Ok(s),
        None => bail!(
            "ACP 的回应里找不到 JSON 对象。\
             \n\
             回应全文（截断到 2000 字）：\n{}\n\
             \n\
             为什么这必须报错而不是「凑合解析」：凑合出来的东西会被当成\
             「这一组抽不出东西」，与「agent 真的什么都没抽到」**长得一模一样**，\
             没人查得出区别。",
            &t.chars().take(2000).collect::<String>()
        ),
    }
}

/// 剥掉 ```` ```json … ``` ```` 围栏。
fn strip_code_fence(t: &str) -> Option<&str> {
    let start = t.find("```")?;
    let after = &t[start + 3..];
    // 跳过 ``` 之后可能的语言标注（json / JSON / jsonc …）
    let body_start = after.find('\n').map(|i| after[i + 1..].trim_start())?;
    let end = body_start.rfind("```")?;
    let body = body_start[..end].trim();
    if body.starts_with('{') {
        Some(body)
    } else {
        None
    }
}

/// 找最外层的那对 `{` … `}`，**跳过字符串字面量**。
fn outermost_object(t: &str) -> Option<&str> {
    let bytes = t.as_bytes();
    let open = t.find('{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        let c = *b as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(t[open..=i].trim());
                }
            }
            _ => {}
        }
    }
    None
}

/// **模型侧的输出形状** —— 只含「模型该给的判断」。
///
/// 刻意**不含** `nullable` / `origin` / `rejected_comments`：
/// 那三样要么是 schema 里的事实（我自己填），要么**填不出来**（留 `None`）。
/// 让模型去答它答不了的问题，只会把「不知道」变成一个**看起来像答案的值**。
#[derive(Debug, serde::Deserialize)]
struct RawType {
    #[serde(default)]
    key: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    is_new: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    properties: Vec<RawProperty>,
}

#[derive(Debug, serde::Deserialize)]
struct RawProperty {
    #[serde(default)]
    name: String,
    #[serde(default)]
    table: String,
    #[serde(default)]
    column: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RawLink {
    #[serde(default)]
    key: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    from_type: String,
    #[serde(default)]
    to_type: String,
    #[serde(default)]
    cardinality: String,
    #[serde(default)]
    semantic: String,
}

#[derive(Debug, serde::Deserialize)]
struct RawExtraction {
    #[serde(default)]
    types: Vec<RawType>,
    #[serde(default)]
    links: Vec<RawLink>,
}

/// 把累积到的正文读成 [`GroupExtraction`]。
///
/// **能自己查的事实由本函数填**（见模块头表格）；**填不出来的留 `None`**。
/// 任何一条编造的 `(表,列)` 立即报错 —— 不会有一份 `source` 指向不存在列的候选。
pub fn parse_extraction(text: &str, group: &ExtractionGroup) -> Result<GroupExtraction> {
    let json_src = extract_json_object(text)?;
    let raw: RawExtraction = serde_json::from_str(json_src).with_context(|| {
        format!(
            "ACP 回应里的 JSON 不符合约定的输出形状（types[].properties[] / links[]）。\
             \n原文（截断到 2000 字）：\n{}",
            &json_src.chars().take(2000).collect::<String>()
        )
    })?;

    let mut types = Vec::with_capacity(raw.types.len());
    for t in raw.types {
        let t = t.normalize_key();
        if t.key.trim().is_empty() {
            bail!("ACP 提出了一个 key 为空的类型 —— key 是跨轮次锚点，空锚点没法用");
        }
        // 证据要挂在**某一个真实表**上：取第一条属性的来源表。
        // 必须在移动 `t.properties` 的循环**之前**算好。
        let evidence_table = t.table_for_evidence();
        let mut properties = Vec::with_capacity(t.properties.len());
        for p in t.properties {
            if p.name.trim().is_empty() {
                bail!(
                    "ACP 为类型 {:?} 提了一个属性名为空的声明 —— 客户视图里会出现一个无名属性",
                    t.key
                );
            }
            // ★ 自己查事实：这一列在**本组 schema** 里可不可空。
            let (nullable, real_table) = lookup_column(group, &p.table, &p.column).with_context(|| {
                format!(
                    "ACP 为类型 {:?} 的属性 {:?} 声明的来源列 {}.{} 在**这一组**的 schema 里不存在。\n\
                     \n\
                     为什么必须报错而不是照收：设计点③要「证据必须机器可验证」，\
                     而一条指向不存在列的声明**根本没有可核对的对象**。\
                     更要紧的是 `nullable` —— 它是**源库的事实**，本函数要从 schema 里查；\
                     查不到就只能编，而编出来的「可空」标记会**原样**出现在客户视图里。\n\
                     阶段 2 的 `verify_mappings` 也会把这种声明报成 `UnknownColumn`。",
                    t.key, p.name, p.table, p.column
                )
            })?;
            properties.push(CandidateProperty {
                name: p.name,
                source: ColumnRef::new(real_table.as_str(), p.column.as_str()),
                nullable: Some(nullable),
                note: p.note.filter(|n| !n.trim().is_empty()),
                // ★ 留 `None`：见模块头「另一半」—— 模型没有渠道报告它的命名来源。
                // 重建路径会对缺 origin **报错**，那正是我们要的行为。
                origin: None,
            });
        }
        let evidence = vec![json!({
            "table": evidence_table,
            "claim": "由源库表结构抽取（ACP adapter）",
        })];
        types.push(CandidateType {
            key: t.key,
            display_name: t.display_name,
            is_new: t.is_new,
            properties,
            description: t.description.filter(|d| !d.trim().is_empty()),
            aliases: t.aliases,
            // ★ 自己造证据：形状与规则实现一致（`assemble` 只是搬运它）。
            // 问模型「你凭什么这么说」只会得到一句无法核对的自然语言。
            evidence,
            // ★ 留 `None`：本 adapter 没有 REQ-005 的质量门槛，**没有**拒用计数可报。
            // 报 `Some(0)` 会让 `stats.props_comment_rejected` 写 0 ——
            // 真实源库那次抽取是 6，写 0 就是在说谎（`assemble` 的模块头原话）。
            rejected_comments: None,
        });
    }

    let links = raw
        .links
        .into_iter()
        .map(|l| CandidateLink {
            key: l.key,
            target: l.target,
            from_type: l.from_type,
            to_type: l.to_type,
            cardinality: l.cardinality,
            semantic: l.semantic,
            // 关系的证据锚点是**具体的列**（`*_id` 那种），由模型指出来；
            // 拿不到就留空 —— `evidence` 是 `Vec`，空 = 没有证据，
            // 而「没有证据」在契约里是合法的一档（不是缺失）。
            evidence: Vec::new(),
        })
        .collect();

    Ok(GroupExtraction { types, links })
}

impl RawType {
    /// 证据要挂在**某一个真实表**上：取第一条属性的来源表。
    ///
    /// 一组只含一张表（设计点②），而模型可能给一个属性都不提 ——
    /// 那时这条实体的证据就没有可挂的表，**如实留空**（`evidence` 是 `Vec`，
    /// 空 = 没有证据，是契约里合法的一档，不是缺失）。
    fn table_for_evidence(&self) -> String {
        self.properties
            .first()
            .map(|p| p.table.clone())
            .unwrap_or_default()
    }

    /// `key` 缺失时退回 `display_name`。
    ///
    /// **只**做这一件事，而且只在上游已核过 `key` 非空之后由
    /// [`parse_extraction`] 报错兜底 —— 猜模型「想说什么」就是猜，
    /// 所以不做任何别的「智能修正」。
    fn normalize_key(mut self) -> Self {
        if self.key.trim().is_empty() {
            self.key = self.display_name.clone();
        }
        self
    }
}

/// 在**本组**的 schema 里查一列，返回 `(该列的 nullable, 真实表名)`。
///
/// 只查作用域（`group.schema`）—— 上下文表只用于 FK 消歧，**不是抽取对象**
/// （`ExtractionGroup` 的 `context_tables` 文档明写这一点）。
fn lookup_column(group: &ExtractionGroup, table: &str, column: &str) -> Option<(bool, String)> {
    for t in &group.schema {
        if t.name != table {
            continue;
        }
        if let Some(c) = t.columns.iter().find(|c| c.name == column) {
            return Some((c.nullable, t.name.clone()));
        }
    }
    None
}

// ──────────────────────────────────────────────────────────
// 契约缺口 —— 抽取**之后**的守卫（判据的实现住在 assemble）
// ──────────────────────────────────────────────────────────

/// 列出这组候选**承载不下**提案的地方。
///
/// # ★ 它是**抽取后守卫**，不是能力标签的来源
///
/// 判据住在 [`crate::agent::assemble`]（重建契约真正生效的地方），
/// 本函数是它的**重导出**，不是第二份实现 ——
/// 两处各判一次就是一处会漂移的地方，而漂移的方向永远是「判据说没问题、
/// 重建照样丢了东西」。
///
/// 调用时机是**抽取之后**（编排在重建路径上逐组核对，见
/// [`crate::agent::orchestrate::orchestrate`]）。
/// 能力声明是**静态**的、发生在抽取**之前**，两者不许互相推导：
/// 判据先于数据产生，就只能读一个还没发生的状态。
///
/// 重建路径（[`crate::agent::assemble::rebuild_outcome`]）逐字段核候选：
/// 缺 `origin` 就数不出 `stats.props_via_*`，缺 `rejected_comments`
/// 就把 REQ-005 的降级痕迹写成 0，缺 `nullable` 就要编一个源库事实，
/// 缺 `links[].evidence` 则证据会**静默**丢 —— 四者都是**显式**失败。
///
/// 把这个函数做成**可测的纯函数**，是为了让「这批候选载不载得下」
/// 从一句论证变成**一条断言**：对本 adapter 自己的输出跑一遍，
/// 非空 ⇒ 声明 `rebuilds_proposal` 就是撒谎。
pub use crate::agent::assemble::contract_gaps;

/// 本 adapter 这一份候选是否**如实**承载重建契约。
///
/// ⚠️ **逐份答案**的判定，**不是**实现的能力声明。
/// [`AcpAgent::capabilities`](super::AcpAgent::capabilities) 静态地回答
/// 「本实现是否具备这个能力」；两者分开，才不会长出「标签跟着答案翻转」
/// 那种形状（不变量见该方法的文档）。
pub use crate::agent::assemble::can_rebuild_proposal;