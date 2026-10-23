//! REQ-126 · **预置问题声明**（`portal-questions.json`）—— 门户能问的全部东西。
//!
//! ```json
//! {
//!   "version": "1.0.0",
//!   "reviewed_by": "业务负责人@2026-10-23",
//!   "questions": [
//!     {"id": "deduction_by_currency",
//!      "title": "我的扣费明细（按币种汇总）",
//!      "sparql": "SELECT ?cur (SUM(?amt) AS ?amt) WHERE { … }",
//!      "note": "按扣费明细的币种汇总。"}
//!   ]
//! }
//! ```
//!
//! # 这个文件是**客户能看到的唯一查询面**
//!
//! 门户没有 `/api/sparql`：客户能问的问题**只有**这里逐条列出的那些。
//! 于是这份声明承担了三件在自由查询里由「用户当场写」承担的事：
//!
//! | 事 | 在这里怎么落 |
//! | --- | --- |
//! | 问什么 | 一条 `sparql`（v1 子集；与 REQ-122 的 `sparql query` 同一个解析器） |
//! | 口径谁负责 | `reviewed_by` **必填**（文档级，或逐题覆盖） |
//! | 给客户看什么 | `title` / `note`：⛔ 不许出现表名 / 列名 / IRI / SQL |
//!
//! # 启动时逐题校验，**fail-closed**
//!
//! 一条不合格 ⇒ **整份拒绝启动**，点名问题 id 与原因（⛔ 不是跳过那一条继续跑 ——
//! 跳过等于「客户点开一张卡片，看到的是别人的口径或一个 500」）。
//!
//! 判据**不是本模块自己写的解析器**：每条 `sparql` 都过一遍
//! [`crate::sparql::plan::plan`]（与执行期**同一个**规划器、同一套租户注入），
//! 于是「启动时通过」与「运行时能跑」是同一件事：
//!
//! | # | 拒绝理由 | 谁判的 |
//! | --- | --- | --- |
//! | ① | `reviewed_by` 为空（文档级与逐题都空） | 本模块 |
//! | ② | 问题涉及的表没在 `query-policy.json` 里登记 | `plan` 的 `decide_tenants`（fail-closed，点名表） |
//! | ③ | 属性 / 类在目录里不存在、语法超出 v1 子集 | `plan` 的解析 + 名字解析（点名属性） |
//! | ④ | 结果里要投影**实例 IRI**（`…/<类>/<id>`） | 本模块（看 `plan` 生成的 SQL 里有没有 IRI 模板） |
//! | ⑤ | 结果列（变量名）与真实列名同名 | 本模块（客户不该借表头学到列名） |
//! | ⑥ | 涉及的表没有**业务名**（建模轨里 `display_name` 与 key 相同） | 本模块（AC-004：只给业务名） |
//! | ⑦ | `title` / `note` 里出现表名 / 列名 / IRI / SQL 字样 | 本模块（AC-004 的否定面） |
//! | ⑧ | 问题自带 `tenant` / 任何不认识的字段 | serde 的 `deny_unknown_fields`（⛔ 不许问题自带「查谁」） |
//!
//! # 覆盖范围那句话是怎么来的
//!
//! `coverage: main_only` 的告警正文**不直接转发**声明里的 `coverageNote`：
//! 那份 note 是写给评审者看的，可能带列名（真实声明里的扣费表就写着
//! `main_customer_id`）。客户看到的必须是业务语言 ⇒ 见 [`coverage_warning`]：
//! note 里只要出现技术标识符形态（连续小写字母 / 数字 / 下划线）就整句丢掉，
//! 退回标准提示。**宁可少一句话，也不漏一个列名。**
//!
//! # 已知限制（⛔ 不写成「已封堵」）
//!
//! - 校验只看**规划结果**（表 / 属性 / 投影形态），**不执行**：一条能规划但会超时
//!   的问题照样能启动（运行期会得到 `TIMEOUT`，页面可见）；
//! - 变量名与列名同名的判据是**逐表列名清单**的比对，改不了「变量名恰好是另一张表的
//!   业务同义词」这种语义泄漏 —— 那要靠人审（`reviewed_by` 就是为此存在的）；
//! - 问题之间**没有依赖 / 参数**：一个问题是死的（⛔ 门户不接受任何入参）。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

use crate::domain::model_view::ModelTrackView;
use crate::domain::r2rml::R2rmlMapping;
use crate::domain::ttl::SchemaTypes;
use crate::sparql::plan::{plan, PlanInput};
use crate::sparql::policy::{QueryPolicy, TableDecision};

/// 启动校验用的**假租户**。
///
/// 只为让规划器有一条合法的 `tenant`（它会被内联进校验用的 SQL，**但那条 SQL
/// 永远不会被执行** —— 本模块不连库）。用一眼能认出来的名字，免得有人把它
/// 当成真实租户。
pub const PROBE_TENANT: &str = "__portal_startup_probe__";

/// 问题 id 的最大长度（进审计、进 URL 路径）。
const MAX_ID_LEN: usize = 64;

/// 客户看到的**问题卡片**（⛔ 没有 `sparql` 字段 —— 这是唯一的对外形状）。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QuestionCard {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 一条**已校验**的问题。
///
/// ⚠️ `sparql` 是 `pub` 但**本类型不实现 `Serialize`**：想把它发给客户，
/// 必须先经过 [`QuestionCard`]（一个**没有** sparql 字段的类型）。
/// 「序列化一个 Question」这件事在类型层就写不出来。
#[derive(Debug, Clone)]
pub struct Question {
    card: QuestionCard,
    pub sparql: String,
    /// 谁签的这一条（文档级或被逐题覆盖）。
    pub signed_by: String,
}

impl Question {
    pub fn id(&self) -> &str {
        &self.card.id
    }

    pub fn title(&self) -> &str {
        &self.card.title
    }

    pub fn note(&self) -> Option<&str> {
        self.card.note.as_deref()
    }

    /// 对外的形状（克隆一份，⛔ 不带 sparql）。
    pub fn card(&self) -> QuestionCard {
        self.card.clone()
    }
}

/// 已校验的整份声明。
#[derive(Debug, Clone)]
pub struct QuestionSet {
    reviewed_by: String,
    questions: Vec<Question>,
}

impl QuestionSet {
    pub fn get(&self, id: &str) -> Option<&Question> {
        self.questions.iter().find(|q| q.id() == id)
    }

    pub fn cards(&self) -> Vec<QuestionCard> {
        self.questions.iter().map(Question::card).collect()
    }

    pub fn ids(&self) -> Vec<&str> {
        self.questions.iter().map(Question::id).collect()
    }

    pub fn len(&self) -> usize {
        self.questions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.questions.is_empty()
    }

    pub fn reviewed_by(&self) -> &str {
        &self.reviewed_by
    }
}

/// 业务名映射：**表 key → 业务名**（只收真正翻译过的那些）。
///
/// `display_name` 与 key 相同（没翻译）的表**不进这张表** —— 于是
/// 「客户这个问题的结果里有一张没名字的表」会在启动校验里被点名，
/// 而不是在页面上把表名显示给客户。
pub fn business_names(model: &ModelTrackView) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for e in &model.entity_types {
        let key = e.key.trim();
        let name = e.display_name.trim();
        if key.is_empty() || name.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case(key) {
            continue;
        }
        out.insert(key.to_string(), name.to_string());
    }
    out
}

/// 覆盖范围的人话标签。
pub fn coverage_label(code: &str) -> &'static str {
    match code {
        "main_only" => "只覆盖主账号",
        "complete" => "完整",
        _ => "未知",
    }
}

/// `coverage: main_only` ⇒ 那句**必须显著展示**的告警（AC-004-2）。
///
/// `note` 来自声明（`coverageNote`）；只有它**不含技术标识符**时才用，
/// 否则退回标准提示（见模块头「覆盖范围那句话是怎么来的」）。
pub fn coverage_warning(object: &str, coverage: &str, note: Option<&str>) -> Option<String> {
    if coverage != "main_only" {
        return None;
    }
    let tail = match note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if !has_identifier_run(n) => n.to_string(),
        _ => "子账号产生的部分不在结果里，金额可能偏低 —— \
              请不要把偏小的数字读成「没有业务」"
            .to_string(),
    };
    Some(format!("「{object}」只覆盖主账号：{tail}"))
}

/// 这段文字里有没有**技术标识符形态**（连续的小写字母 / 数字 / 下划线，≥3）。
///
/// 判据刻意只看**小写**字母 / 数字 / 下划线：`RMB` / `USD` 这类业务代码不算
/// （大写不是小写）；`main_customer_id` / `deductions` 这类长串算（连续 ≥3），
/// `0/4` 这种两位数字不算。
fn has_identifier_run(text: &str) -> bool {
    let mut run = 0usize;
    for c in text.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            run += 1;
            if run >= 3 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// `text` 里是否以**ASCII 词边界**出现 `needle`（大小写不敏感）。
///
/// 用词边界而不是 `contains`：`orders` 不该在 `orders_archive` 里被算成一次命中
/// （那会把一条合法说明判红，久了就没人信这条判据了）。
fn ascii_word_contains(text: &str, needle: &str) -> bool {
    let hay = text.to_ascii_lowercase();
    let needle = needle.trim().to_ascii_lowercase();
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let bytes = hay.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(&needle) {
        let at = from + rel;
        let end = at + needle.len();
        let before_ok = at == 0 || !is_word_byte(bytes[at - 1]);
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = at + 1;
        if from >= hay.len() {
            break;
        }
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `title` / `note` 里的泄漏：返回值是**命中的词**（用于点名）。
fn text_leaks(text: &str, forbidden: &[String]) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for f in forbidden {
        if ascii_word_contains(text, f) && !hits.iter().any(|h| h.eq_ignore_ascii_case(f)) {
            hits.push(f.clone());
        }
    }
    for kw in ["sparql", "select", "where", "iri"] {
        if ascii_word_contains(text, kw) {
            hits.push(kw.to_string());
        }
    }
    if text.contains("://") {
        hits.push("URL/IRI".to_string());
    }
    hits
}

// ══════════════════════════════════════════════════════════════
// 解析 + 逐题校验（fail-closed）
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    version: String,
    #[serde(default)]
    reviewed_by: Option<String>,
    /// 自由说明（给人看；⛔ 不进页面）。
    #[serde(default)]
    note: Option<String>,
    questions: Vec<RawQuestion>,
}

/// 一条**原始**问题（`deny_unknown_fields`：⛔ 不许问题自带 `tenant` 之类）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawQuestion {
    id: String,
    title: String,
    sparql: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    reviewed_by: Option<String>,
}

/// 解析 + 逐题校验。任何一条不合格 ⇒ `Err(点名到问题 id 的清单)`。
pub fn validate(
    json: &str,
    mapping: &R2rmlMapping,
    schema: &SchemaTypes,
    policy: &QueryPolicy,
    names: &BTreeMap<String, String>,
    ontology_iri: &str,
) -> Result<QuestionSet, String> {
    let doc: Doc = serde_json::from_str(json).map_err(|e| {
        format!(
            "portal-questions.json 不是合法形状：{e}\n  期望 \
             {{version, reviewed_by, questions:[{{id, title, sparql, note?, reviewed_by?}}]}}\
             —— ⛔ 问题里不许出现 tenant 或任何别的字段"
        )
    })?;

    let mut problems: Vec<String> = Vec::new();

    let major = doc.version.trim().split('.').next().unwrap_or("");
    if major != "1" {
        problems.push(format!(
            "version = {:?}：本实现只认 1.x 的问题声明格式",
            doc.version
        ));
    }
    if doc.questions.is_empty() {
        problems.push("questions 是空的 —— 门户一条问题都没有，客户打开只会看到空白".to_string());
    }
    let doc_signed = doc
        .reviewed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut seen: Vec<String> = Vec::new();
    let mut questions: Vec<Question> = Vec::with_capacity(doc.questions.len());
    for raw in &doc.questions {
        let id = raw.id.trim().to_string();
        let at = if id.is_empty() {
            "（没有 id 的那一条）".to_string()
        } else {
            format!("问题 {id}")
        };

        // ① 形状
        if id.is_empty() || id.len() > MAX_ID_LEN {
            problems.push(format!(
                "{at} 的 id 不合法：必须在 1–{MAX_ID_LEN} 字符之间（它要进 URL 与审计）"
            ));
        } else if !id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        {
            problems.push(format!(
                "问题 {id} 的 id 只许小写字母 / 数字 / `_` / `-`（它要进 URL 路径与审计行）"
            ));
        }
        if !id.is_empty() && seen.iter().any(|s| s == &id) {
            problems.push(format!("问题 {id} 的 id 重复 —— 两条问题同名，点开哪一条都会含糊"));
        }
        seen.push(id.clone());
        if raw.title.trim().is_empty() {
            problems.push(format!("{at} 的 title 是空的 —— 客户看到的第一行就是它"));
        }

        // ② 签名（文档级或被逐题覆盖）
        let signed_by = raw
            .reviewed_by
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| doc_signed.clone());
        let Some(signed_by) = signed_by else {
            problems.push(format!(
                "{at} 没有签名：reviewed_by 为空 —— 问题本身就是**业务口径**，必须有人签；\
                 ⛔ 门户不接受未签的口径（生产环境必须真签，不许占位）"
            ));
            continue;
        };

        // ③ 规划器过一遍：语法 / 表登记 / 属性存在性（与执行期同一个函数）
        let input = PlanInput {
            mapping,
            schema,
            ontology_iri,
            policy,
            tenant: PROBE_TENANT,
            max_rows: 1,
        };
        let planned = match plan(&raw.sparql, &input) {
            Ok(p) => p,
            Err(e) => {
                problems.push(format!(
                    "{at} 的查询过不了规划器（启动期 fail-closed，不是等到客户点开才 500）：\n{}",
                    indent(e.message())
                ));
                continue;
            }
        };
        if planned.tables.is_empty() {
            problems.push(format!("{at} 的查询没有涉及任何表 —— 这条问题取不到东西"));
            continue;
        }

        // ④ 表必须登记（规划器已经拦过；这里再判一次是为了**点名**表）
        let mut columns: Vec<String> = Vec::new();
        for t in &planned.tables {
            match policy.decide(t) {
                TableDecision::Filter { .. } | TableDecision::Public { .. } => {}
                TableDecision::Unregistered => problems.push(format!(
                    "{at} 涉及的表 {t} 没有在 query-policy.json 里登记 —— \
                     ⛔ 拒绝启动（客户看到「没有数据」而其实是口径没登记，是最坏的一种错）"
                )),
            }
            if let Some(cols) = schema.column_names(t) {
                columns.extend(cols);
            }
        }

        // ⑤ 业务名（AC-004：客户看到的是业务名，不是表名）
        for t in &planned.tables {
            if !names.contains_key(t) {
                problems.push(format!(
                    "{at} 涉及的表 {t} 没有业务名（建模轨里 display_name 与 key 相同）—— \
                     客户不能看到表名；请先给这张表补一个业务名，再放行这个问题"
                ));
            }
        }

        // ⑥ 结果里不许有实例 IRI：规划器的 IRI 模板长这样 `'<本体>#<类>/'`。
        if planned.sql.contains(&format!("'{ontology_iri}#")) {
            problems.push(format!(
                "{at} 把内部标识（IRI）当成了结果列 —— \
                 客户看到的是 `<项目>#<表>/<id>`，一个字段同时泄漏项目名与表名。\
                 请改投影：只取业务列（如名称 / 状态 / 金额），把主体变量从 SELECT 里去掉"
            ));
        }

        // ⑦ 结果列名不许与真实列名同名（否则表头就是一列列名）
        for c in &planned.columns {
            if columns.iter().any(|col| col.eq_ignore_ascii_case(c)) {
                problems.push(format!(
                    "问题 {id} 的结果列 {c:?} 与真实列名同名 —— \
                     客户会从表头学到列名；请把变量改成一个业务词（如 ?cur / ?amt）"
                ));
            }
        }

        // ⑦b id 会进 URL、/api/questions 与审计行 ⇒ 它自己不许含表名。
        //
        // 真实教训：草稿里有一条 `my_licenses`，而表就叫 `licenses` —— 客户在
        // 浏览器 DevTools 里看到的第一个词就是表名。id 是**业务词**，不是表名缩写。
        for t in &planned.tables {
            if id.to_ascii_lowercase().contains(&t.to_ascii_lowercase()) {
                problems.push(format!(
                    "问题 {id} 的 id 里含表名 {t} —— id 会出现在 URL / 问题列表 / 审计里，\
                     客户一眼就能读到表名；请换一个业务词做 id"
                ));
            }
        }

        // ⑧ 给客户看的文字不许带技术细节
        let forbidden: Vec<String> = planned
            .tables
            .iter()
            .cloned()
            .chain(columns.iter().cloned())
            .collect();
        let mut text = raw.title.clone();
        if let Some(n) = raw.note.as_deref() {
            text.push('\n');
            text.push_str(n);
        }
        let hits = text_leaks(&text, &forbidden);
        if !hits.is_empty() {
            problems.push(format!(
                "{at} 的 title / note 里出现了不该给客户看的东西：{} —— \
                 客户页面只写业务口径（⛔ 表名 / 列名 / IRI / SQL 字样）",
                hits.join("、")
            ));
        }

        questions.push(Question {
            card: QuestionCard {
                id,
                title: raw.title.trim().to_string(),
                note: raw
                    .note
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            },
            sparql: raw.sparql.trim().to_string(),
            signed_by,
        });
    }

    if !problems.is_empty() {
        return Err(format!(
            "门户问题声明不合法（{} 处）—— ⛔ 拒绝启动，一条都不放行：\n  - {}",
            problems.len(),
            problems.join("\n  - ")
        ));
    }
    let _ = doc.note;
    Ok(QuestionSet {
        // 文档级签名缺失时用一句话占位（横幅要打印得出「谁签的」）。
        // ⛔ 这不是「放宽」：每一题都已经被逐题签名校验过（见上面 ②）。
        reviewed_by: doc_signed.unwrap_or_else(|| "（逐题签名）".to_string()),
        questions,
    })
}

/// 判据专用：一条**已构造好**的问题（只给同 crate 的单元判据用）。
///
/// ⛔ 只编进测试：生产代码里 `Question` 只能由 [`validate`] 产出。
#[cfg(test)]
pub(crate) fn test_question() -> Question {
    Question {
        card: QuestionCard {
            id: "my_spend".to_string(),
            title: "我的订单金额".to_string(),
            note: Some("按业务口径汇总".to_string()),
        },
        sparql: format!("SELECT ?amt WHERE {{ ?o <{PROBE_TENANT}> ?amt . }}"),
        signed_by: "测试".to_string(),
    }
}

/// 多行诊断缩进（拼进「问题 X 的查询过不了规划器」下面）。
fn indent(msg: &str) -> String {
    msg.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ══════════════════════════════════════════════════════════════
// 判据（不连库：映射 / schema / 声明都是夹具）
// ══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model_view::{ModelEntityView, ModelLinkView, ModelPropertyView};
    use crate::domain::r2rml::build_mapping;
    use crate::domain::snapshot::{Evidence, JoinState};
    use crate::domain::ttl::TtlOptions;

    const ONT: &str = "https://ontok.local/demo";

    const SCHEMA: &str = r#"{"tables":[
      {"name":"customers","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"customer_name","data_type":"text"}]},
      {"name":"orders","primary_key":"order_id","composite_primary_key":[],"unique_columns":["order_id"],
       "columns":[{"name":"order_id","data_type":"text","is_primary_key":true},
                  {"name":"customer_id","data_type":"text"},
                  {"name":"amount","data_type":"numeric(12,4)"}]},
      {"name":"package_templates","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"title","data_type":"text"}]},
      {"name":"secrets","primary_key":"id","composite_primary_key":[],"unique_columns":["id"],
       "columns":[{"name":"id","data_type":"text","is_primary_key":true},
                  {"name":"v","data_type":"text"}]}
    ]}"#;

    const POLICY: &str = r#"{
      "version": "1.0.0",
      "tenantSubject": "customers",
      "tenantIdColumn": "id",
      "perTable": {
        "orders": {"tenantColumn": "customer_id", "coverage": "main_only",
                   "coverageNote": "main_customer_id 全空（0/4）；子账号产生的订单不在结果内",
                   "reason": "订单归客户"}
      },
      "publicDimensions": [{"table": "package_templates", "reason": "套餐目录对所有租户相同"}]
    }"#;

    fn prop(name: &str, column: &str) -> ModelPropertyView {
        ModelPropertyView {
            name: name.into(),
            column: column.into(),
            nullable: false,
            note: None,
            sample: None,
        }
    }

    fn entity(key: &str, display: &str, props: Vec<ModelPropertyView>) -> ModelEntityView {
        ModelEntityView {
            key: key.into(),
            display_name: display.into(),
            description: None,
            aliases: vec![],
            parent: None,
            disjoint_with: vec![],
            properties: props,
            evidence: vec![],
        }
    }

    fn link(from: &str, to: &str, column: &str, parent_col: &str) -> ModelLinkView {
        ModelLinkView {
            key: format!("{from}_{column}__{to}"),
            from_type: from.into(),
            to_type: to.into(),
            cardinality: "N:1".into(),
            semantic: "belongs_to".into(),
            evidence: vec![Evidence {
                table: from.into(),
                column: column.into(),
                claim: "外键".into(),
            }],
            confidence: None,
            join_verified: Some(JoinState::Verified),
            join_evidence: Some(format!(
                "value-overlap 5/5 (100.00%) -> {to}.{parent_col} via primary_key (plural)"
            )),
            column: Some(column.into()),
        }
    }

    struct Fixture {
        schema: SchemaTypes,
        mapping: R2rmlMapping,
        policy: QueryPolicy,
        names: BTreeMap<String, String>,
    }

    fn fixture() -> Fixture {
        let schema = SchemaTypes::parse(SCHEMA).expect("夹具 schema");
        let view = ModelTrackView {
            seq: 2,
            extracted_by: None,
            entity_types: vec![
                entity("customers", "客户", vec![prop("客户名", "customer_name")]),
                entity(
                    "orders",
                    "订单",
                    vec![
                        prop("金额", "amount"),
                        prop("所属客户", "customer_id"),
                        prop("订单号", "order_id"),
                    ],
                ),
                entity("package_templates", "套餐模板", vec![prop("标题", "title")]),
                // ⚠️ 没有业务名（display_name == key）—— 判据 ⑥ 就靠它。
                entity("secrets", "secrets", vec![prop("值", "v")]),
            ],
            link_types: vec![
                link("orders", "customers", "customer_id", "id"),
                link("orders", "package_templates", "order_id", "id"),
            ],
            action_types: vec![],
            functions: vec![],
        };
        let mut opts = TtlOptions::new("demo", 2);
        opts.schema = Some(&schema);
        let mapping = build_mapping(&view, &opts, false, Default::default()).expect("夹具映射");
        let policy = QueryPolicy::parse(POLICY).expect("夹具声明");
        let names = business_names(&view);
        Fixture {
            schema,
            mapping,
            policy,
            names,
        }
    }

    /// 拼一份问题声明（`S: AsRef<str>`：`&str` 与 `String` 都能进来）。
    fn doc<S: AsRef<str>>(questions: &[S], reviewed_by: &str) -> String {
        let joined: Vec<String> = questions.iter().map(|s| s.as_ref().to_string()).collect();
        format!(
            r#"{{"version":"1.0.0","reviewed_by":{reviewed_by},"questions":[{questions}]}}"#,
            questions = joined.join(",")
        )
    }

    fn q(id: &str, title: &str, sparql: &str) -> String {
        format!(
            r#"{{"id":"{id}","title":"{title}","sparql":"{sparql}","note":"按业务口径汇总"}}"#
        )
    }

    #[test]
    fn a_fully_signed_legit_set_parses() {
        let f = fixture();
        let raw = doc(
            &[q(
                "my_spend",
                "我的订单金额",
                "SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let set = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).expect("合法声明");
        assert_eq!(set.len(), 1);
        assert_eq!(set.reviewed_by(), "业务负责人@2026-10-23");
        let card = &set.cards()[0];
        assert_eq!(card.id, "my_spend");
        assert_eq!(card.title, "我的订单金额");
        // ⛔ 卡片形状里没有 sparql（类型层就写不出来）
        let json = serde_json::to_string(card).unwrap();
        assert!(!json.contains("sparql"), "{json}");
        assert!(!json.contains("SELECT"), "{json}");
    }

    /// ⭐ AC-003 ①：`reviewed_by` 为空 ⇒ 拒绝启动并点名。
    #[test]
    fn an_unsigned_question_is_rejected_and_named() {
        let f = fixture();
        let raw = doc(
            &[q(
                "my_orders",
                "我的订单金额",
                "SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
            )],
            "null",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("my_orders") && e.contains("reviewed_by"), "{e}");
        assert!(e.contains("拒绝启动"), "{e}");
    }

    /// 逐题签名可以覆盖文档级；两者都空才拒绝。
    #[test]
    fn a_per_question_signature_overrides_the_document_one() {
        let f = fixture();
        let raw = format!(
            r#"{{"version":"1.0.0","reviewed_by":null,"questions":[
                {{"id":"my_spend","title":"我的订单金额","sparql":"SELECT ?amt WHERE {{ ?o <https://ontok.local/demo#orders__amount> ?amt . }}","reviewed_by":"财务@2026-10-23"}}]}}"#
        );
        let set = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).expect("逐题签名");
        assert_eq!(set.len(), 1);
    }

    /// ⭐ AC-003 ②：表没在声明里登记 ⇒ 拒绝启动，点名**问题 id + 表名**。
    #[test]
    fn a_table_missing_from_the_policy_is_rejected_and_named() {
        let f = fixture();
        let raw = doc(
            &[q(
                "secret_question",
                "某个内部数据",
                "SELECT ?v WHERE { ?s <https://ontok.local/demo#secrets__v> ?v . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("secret_question"), "必须点名问题 id：{e}");
        assert!(e.contains("secrets"), "必须点名表：{e}");
        assert!(
            e.contains("没有在声明里登记") || e.contains("没有在 query-policy.json 里登记"),
            "{e}"
        );
    }

    /// ⭐ AC-003 ③：属性不存在 ⇒ 拒绝启动，点名属性。
    #[test]
    fn an_unknown_property_is_rejected_and_named() {
        let f = fixture();
        let raw = doc(
            &[q(
                "typo",
                "我的订单金额",
                "SELECT ?x WHERE { ?o <https://ontok.local/demo#orders__amoont> ?x . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("typo"), "{e}");
        assert!(e.contains("orders__amoont") && e.contains("不存在"), "{e}");
    }

    /// ④：投影实例 IRI ⇒ 拒绝（客户不该看到 `<项目>#<表>/<id>`）。
    #[test]
    fn a_question_projecting_an_instance_iri_is_rejected() {
        let f = fixture();
        let raw = doc(
            &[q(
                "iri_leak",
                "我的订单",
                "SELECT ?o ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("iri_leak") && e.contains("IRI"), "{e}");
    }

    /// ⑤：结果列名与真实列名同名 ⇒ 拒绝（表头不许是列名）。
    #[test]
    fn a_result_column_named_like_a_real_column_is_rejected() {
        let f = fixture();
        let raw = doc(
            &[q(
                "col_name",
                "我的订单金额",
                "SELECT ?amount WHERE { ?o <https://ontok.local/demo#orders__amount> ?amount . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("amount") && e.contains("同名"), "{e}");
    }

    /// ⑤b：id 里含表名 ⇒ 拒绝（id 会随问题列表回到浏览器）。
    #[test]
    fn a_question_id_carrying_a_table_name_is_rejected() {
        let f = fixture();
        let raw = doc(
            &[q(
                "my_orders",
                "我的订单金额",
                "SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("my_orders") && e.contains("含表名"), "{e}");
        assert!(e.contains("orders"), "{e}");
        // 反向自证：换一个业务词就过。
        let raw = doc(
            &[q(
                "my_spend",
                "我的订单金额",
                "SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        assert!(validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).is_ok());
    }

    /// ⑥：涉及的行业务名缺失 ⇒ 拒绝（不能退回显示表名）。
    #[test]
    fn a_table_without_a_business_name_is_rejected() {
        let f = fixture();
        // 把 secrets 登记成公共维度 —— 这样它过了 ②，只剩「没有业务名」这一条。
        let policy = QueryPolicy::parse(
            &POLICY.replace(
                "\"publicDimensions\": [",
                "\"publicDimensions\": [{\"table\":\"secrets\",\"reason\":\"夹具\"},",
            ),
        )
        .expect("夹具声明");
        let raw = doc(
            &[q(
                "no_name",
                "某个内部数据",
                "SELECT ?v WHERE { ?s <https://ontok.local/demo#secrets__v> ?v . }",
            )],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("no_name") && e.contains("没有业务名"), "{e}");
    }

    /// ⑦：title / note 里出现表名 / 列名 ⇒ 拒绝（AC-004 的否定面）。
    #[test]
    fn a_title_or_note_carrying_technical_words_is_rejected() {
        let f = fixture();
        let raw = doc(
            &[r#"{"id":"t1","title":"我 orders 表里的金额","sparql":"SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }"}"#],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("t1") && e.contains("orders"), "{e}");

        let raw = doc(
            &[r#"{"id":"t2","title":"我的订单金额","sparql":"SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }","note":"这个口径见 SELECT 语句"}"#],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("select"), "{e}");
    }

    /// ⑧：问题自带 `tenant` ⇒ 解析就失败（⛔ 不许问题自带「查谁」）。
    #[test]
    fn a_question_carrying_its_own_tenant_does_not_even_parse() {
        let f = fixture();
        let raw = doc(
            &[r#"{"id":"t1","title":"我的订单金额","tenant":"C0002","sparql":"SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }"}"#],
            "\"业务负责人@2026-10-23\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("tenant"), "{e}");
        assert!(!f.names.is_empty());
    }

    #[test]
    fn duplicate_ids_and_bad_ids_are_rejected() {
        let f = fixture();
        let one = q(
            "my_orders",
            "我的订单金额",
            "SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
        );
        let raw = doc(&[one.clone(), one.clone()], "\"signer\"");
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("重复"), "{e}");

        let raw = doc(
            &[q(
                "My Orders!",
                "我的订单金额",
                "SELECT ?amt WHERE { ?o <https://ontok.local/demo#orders__amount> ?amt . }",
            )],
            "\"signer\"",
        );
        let e = validate(&raw, &f.mapping, &f.schema, &f.policy, &f.names, ONT).unwrap_err();
        assert!(e.contains("只许小写字母"), "{e}");
    }

    // ── 覆盖范围告警（AC-004-2）──────────────────────────────────

    #[test]
    fn a_main_only_coverage_warning_never_carries_column_names() {
        let note = "main_customer_id 全空（0/4）；子账号产生的订单不在结果内";
        let w = coverage_warning("订单", "main_only", Some(note)).expect("main_only 必须报警");
        assert!(w.contains("订单"), "{w}");
        assert!(!w.contains("main_customer_id"), "列名漏进警告了：{w}");
        assert!(w.contains("只覆盖主账号"), "{w}");
        // complete 不报警（否则「每条都报警」等于没有报警）。
        assert!(coverage_warning("客户", "complete", Some(note)).is_none());
    }

    #[test]
    fn a_clean_coverage_note_is_kept_verbatim() {
        let w = coverage_warning("订单", "main_only", Some("子账号产生的订单不在结果内")).unwrap();
        assert!(w.contains("子账号产生的订单不在结果内"), "{w}");
    }

    #[test]
    fn the_identifier_run_detector_is_not_vacuous() {
        assert!(has_identifier_run("main_customer_id"));
        assert!(has_identifier_run("deductions"));
        assert!(!has_identifier_run("子账号产生的部分不在结果里"));
        assert!(!has_identifier_run("RMB / USD"));
        assert!(!has_identifier_run("0/4"));
    }

    #[test]
    fn word_boundaries_do_not_cry_wolf() {
        assert!(ascii_word_contains("我 orders 表", "orders"));
        assert!(!ascii_word_contains("orders_archive", "orders"));
        assert!(ascii_word_contains("ORDERS", "orders"));
        assert!(!ascii_word_contains("订单", "orders"));
    }
}
