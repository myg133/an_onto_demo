//! **MCP 工具层**（REQ-027）—— 把冻结快照暴露成客户级问答。
//!
//! # 这一层只做三件事
//!
//! 1. **列目录**（`catalog`）：声明有哪些工具、每个工具吃什么参数。
//! 2. **分发**（`call`）：`name` + `arguments` → 调已有的纯函数 → 结构化数据。
//! 3. **不做编解码**：那是 [`super::protocol`] 的事。
//!
//! ⚠️ **本层不许自己写 JSON 文本、不许自己查库做业务判断**。
//! 它只调 `view::project` / `overview::build_overview` 这两个**既有纯函数**
//! —— 与 `ontok ontology overview` 走的是**同一份**投影与同一份总览。
//! 抄一份投影就会漂：「页面上说 A、总览里说 B」，判读人当场就不信了。
//!
//! # 守门器（AC-005）在这里收口
//!
//! [`call`] 返回的是**结构化数据**（还没序列化），真正的
//! `scan_modeling_terms` 闸在 [`guard`] 里，由服务层对**每一个**返回值
//! 无条件执行。收口在一处是刻意的：闸门散落在各工具里，
//! 新加一个工具就会漏掉 —— 而那正是 AC-005 破防的方式。
//!
//! # 标识符清洗（MC-005）是**并列**的第二道，不是第一道
//!
//! [`scrub_identifiers`] 按**形态**清掉自由文本里的源表名/列名。
//! 它与 [`guard`] 分工：守门器抓的是**建模词汇**（外键/基数/key/…），
//! 清洗抓的是**标识符形态**（`llm_model.model_id` / `customer_id`）——
//! 后者按词元切分时保留下划线，守门器**结构上就抓不到**。
//! 详见 [`scrub_identifiers`] 的说明。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::domain::view::{CustomerView, PropertyView};

/// 一个概念分组的确认状态。
///
/// # 为什么要有这个字段
///
/// `concept_groups.transition.json` 的实况：`confirmed_by: null`，
/// `declared_by: "prefix-cluster-seed"`，`rule` 字段自称
/// 「⚠ **过渡声明，不是结论**」。也就是说**今天所有概念分组都未经业务确认**。
///
/// # 为什么**标记**而不是**过滤**
///
/// 过滤掉未确认的分组，会把 24 个未归组对象连同真实结构一起藏掉 ——
/// 客户问「这系统里有啥」，我们回一句��净的断言，而那个断言只覆盖了一部分。
/// 与 ADR-003 的 `source: declared | tool_produced` 同一条纪律：
/// **由通道负责留痕，而不是靠藏起来装作没有**。
/// 下游 LLM 拿到「客户包含限流配置（此分组未经业务确认）」
/// 比拿到一句干净的断言更诚实 —— 它至少有机会转述这句限定。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Confirmation {
    /// `confirmed`（人工修订）/ `unconfirmed`（机器或 agent 声明）
    pub status: &'static str,
    /// 声明里自带「低置信 / 待确认」提示时为 true。
    ///
    /// REQ-028 · F4：判据是 [`crate::domain::concept::ConceptGroup::low_confidence`]
    /// 这个**结构化字段**，不再是 `scope_hint` 里的文字。
    ///
    /// # 为什么换掉（这是本 REQ 修的一个真实 fail-open）
    ///
    /// 旧判据是 `scope_hint.contains("低置信") || contains("待确认")`。
    /// `scope_hint` 是**给人看的说明文字** —— 拿它当机器信号，
    /// 等于把「文档里写了什么」当成「系统处于什么状态」。
    /// 任何一个分组者把「低置信」改成「存疑」，标记就**静默消失**，
    /// 而且**没有任何测试会红**：下游 LLM 照样收到
    /// `low_confidence: false`，而那是从没被核对过的断言。
    ///
    /// 现在 `scope_hint` 的措辞**仍然**会被检查，但角色变了：
    /// 它只是一致性探针，由 `validate_groups` 报
    /// [`crate::domain::concept::ConceptProblem::ScopeHintDrift`]，
    /// 于是「文字改了、字段没改」变成**报错**而不是静默失效。
    pub low_confidence: bool,
    /// 人话。给下游 LLM 直接转述用。
    pub note: &'static str,
}

/// 从一个 [`crate::domain::concept::ConceptGroup`] 推出确认状态。
///
/// 判据：`extracted_by` 为 `None` = 人工修订（ADR-001）⇒ 已确认；
/// 有值 = 某个 agent/规则声明的 ⇒ **未经业务确认**。
///
/// 这条判据之所以可算，是因为 `ConceptGroup` 没有 `confirmed_by` 字段 ——
/// 确认信息只存在于**声明文件**里，而没随分组落库。
/// 所以这是**从已有字段推出的**，不是猜的；推不出的（两者都不是）不发生，
/// 因为 `extracted_by` 只有 `None`（人工）和 `Some`（机器）两态。
///
/// # ⚠️ `status` 是 **per-version 的事实**，不是 per-group 的判断（REQ-028 · F6）
///
/// 这里曾经有一句错误的注释说「`extracted_by` 是 per-version 的，不是
/// per-group 的」—— **那是与代码相反的**：`extracted_by` 是
/// [`crate::domain::concept::ConceptGroup`] 上的**字段，每组独立持有**。
/// 但**结论是对的**，理由是实测值：
///
/// 真实过渡声明 26 个组的 `extracted_by` **完全相同**
/// （`{agent: "prefix-cluster-seed", version: "0.1.0", round: 1, failed_groups: []}`），
/// 所以推出来的 26 个 `unconfirmed` 是**同一个事实被算了 26 遍**。
///
/// **这个字段今天零信息量。** 它表达的是
/// 「**这一版声明**整体来自 `prefix-cluster-seed`」，
/// 而不是「**这一组**没被业务逐组看过」。
/// 下游 LLM 若把它当成逐组判断，就是把 per-version 的事实
/// 当成 per-group 的证据来转述 —— 那正是 ADR-003
/// 「`source` 语义本轮不动」要留给业务侧确认的东西。
/// 要让它有信息量，只能由**业务侧逐组**声明，本轮不动逻辑。
pub fn confirmation_of(group: &crate::domain::concept::ConceptGroup) -> Confirmation {
    let machine_declared = group.extracted_by.is_some();
    let low_confidence = group.is_low_confidence();

    let (status, note) = if machine_declared {
        (
            "unconfirmed",
            "此分组由工具或规则归并得出，未经业务确认，转述时请带上这句限定",
        )
    } else {
        ("confirmed", "此分组已由人工确认")
    };

    Confirmation { status, low_confidence, note }
}

/// 启动时构建一次的只读上下文。
///
/// ⚠️ **为什么整个进程只算一次**：`mcp-server` 只服务**冻结**版本，
/// 而冻结版本的写路径被 `save_snapshot` 的事务内闸门堵死
/// （`frozen_at IS NOT NULL` ⇒ `VersionFrozen`）。所以这份快照
/// **在进程生命周期内不可能变** —— 缓存它不是「图省事的快照」，
/// 而是「一份被写路径保护、因此可缓存」的快照。
/// 顺带保证同一进程内多次问答看到的是同一份数据。
#[derive(Debug, Clone)]
pub struct OntologyContext {
    pub project: String,
    pub version: i32,
    pub frozen_at: String,
    pub view: CustomerView,
    pub overview: crate::domain::overview::Overview,
    /// 实体 key → 展示名
    pub names: BTreeMap<String, String>,
    /// 概念显示名 → 确认状态
    pub confirmation: BTreeMap<String, Confirmation>,
}

impl OntologyContext {
    /// 每一个返回值都带这一段，让下游知道**这份数据是什么、从哪来**。
    pub fn source_block(&self) -> Value {
        json!({
            "project": self.project,
            "version": self.version,
            "frozen": true,
            "frozen_at": self.frozen_at,
        })
    }

    /// 只挑出与某个概念相关的降级说明。
    ///
    /// `build_overview` 的 `notes` 是**全局**的（讲的是整个版本有哪些地方
    /// 被降级），不筛就会把别处的告警混进这一问的答案里。
    ///
    /// # ⚠️ 这是**文本匹配**，会静默失效（REQ-028 · F4，如实标注）
    ///
    /// `n.contains(concept)` 拿**说明文字**去配**概念名**。它有两个已知弱点，
    /// 本轮**只标注、不修**（修它要动 `overview.rs` 的 `notes` 生成口径，
    /// 超出本 REQ 范围）：
    ///
    /// 1. **概念名改一个字，这条说明就再也匹配不上** ⇒ 降级痕迹**静默消失**，
    ///    没有��何测试会红；
    /// 2. **子串误命中**：`concept = "订单"` 会命中「预订单」那一条说明。
    ///
    /// 与同文件 `low_confidence` 的区别是**明确的**：那个字段本 REQ 已改成
    /// 结构化判据；这个函数**仍然是文本匹配**，下游若依赖它的完整性，
    /// 应当知道它会漂。它当前只用于**给人看的话术**（少一条说明不影响
    /// 任何事实断言），所以此处不改。
    fn notes_about(&self, concept: &str) -> Vec<&str> {
        self.overview
            .notes
            .iter()
            .map(String::as_str)
            .filter(|n| n.contains(concept))
            .collect()
    }

    /// 关系表按展示名反查成可读的一问一答。
    fn relations_of(&self, object: &str) -> Vec<Value> {
        self.view
            .relations
            .iter()
            .filter(|r| r.from == object || r.to == object)
            .map(|r| {
                json!({
                    "from": r.from,
                    "relation": r.meaning,
                    "to": r.to,
                    "how_many": r.how_many,
                })
            })
            .collect()
    }
}

// ──────────────────────────────────────────────────────────
// 标识符清洗（MC-005 · B1）
// ──────────────────────────────────────────────────────────

/// 清洗自由文本里的**源库标识符**（B1）。
///
/// # 这道防线补的是哪个洞
///
/// Dev 原报告说「真正挡住标识符的是结构性的那一层（`view::project`
/// 投影时丢了 `key`/`evidence`/`ordinal`）」。**那个定性被真实数据证伪**：
/// 投影层丢的是**字段名**，而源标识符还有一条路 —— **自由文本**。
/// 真实产物里 `PropertyView.note` 就是数据库**列注释原文**，例如
///
/// ```text
/// obj='大模型服务商' prop='关联模型ID(对应llm_model.model_id)'
/// obj='兑换码'         prop='核销人customer_id'
/// ```
///
/// `llm_model.model_id` 与 `customer_id` 都是**源表名/列名**。
/// 而两道既有防线都管不到这条路：
/// - 投影层只丢结构化字段，**不碰 `note` 的内容**；
/// - [`crate::domain::view::scan_modeling_terms`] 的英文判据按词元切分时
///   **保留下划线**（`view.rs` 的 `split(|c| !(c.is_alphanumeric() || c == '_'))`），
///   所以 `customer_rate_limit` 整体是**一个**词元，永远 `!= "key"` ⇒ 放行。
///
/// # 判据是**形状**，不是词表
///
/// 与 [`crate::domain::view::cardinality_notation_hit`]、
/// `untranslated_semantic_hit` 同一纪律：**词表是枚举，形态是判定**。
/// 完整判据（切分规则 + 两条形态 + REQ-029 补的「段里至少一个字母」）
/// 见 [`crate::domain::identifier_shape`] 的模块注释。
///
/// 两份判据（页面层 / MCP 层）此前各写一遍且**已经漂移**，QA 实测三处差异：
/// 1. **分词方式不同** —— MCP 的 ASCII 词元**含 `.`**（中文前缀被甩在词元之外），
///    页面层的点路径判定却跑在 `split_whitespace` 产出的词上
///    ⇒ `对应abc.def` 在 MCP 侧命中（变成 `对应…`），在页面侧**漏抓**
///    （客户看得见的页面上会留着 `对应abcdef` —— 这是**真实泄漏**）；
/// 2. **字面量规则不同** —— 页面层多一条 `key` 字面量
///    （`identifier_shape::is_modeling_literal`），所以 `key`/`KEY`
///    页面层剔除、MCP 层原样透出。
///    注意 `key` 在 MCP 侧**不是漏网**：[`crate::domain::view::scan_modeling_terms`]
///    兜住了（`guard({"note":"这个字段叫 key"})` 返回 `Err`）。是**机制不同**，
///    不是缺失 —— 所以**没有**把它并进 MCP 侧判据：那会让真实列注释
///    `license key`（`licenses.license_key` 的注释原文）被吃成 `license …`，
///    「License 密钥」是业务说法，不是列名；
/// 3. **命中后的动作不同** —— 页面层**整条丢弃**，MCP 层**局部替换**成 `…`。
///    这一条是**有意保留**的：动作本来就该按出口的承受能力不同而不同。
///
/// REQ-029 处置：前两处（**判据**）已合并进 [`crate::domain::identifier_shape`]，
/// 两份副本都已删除；第三处（**动作**）仍各归各的调用方。
///
/// # 为什么**替换**而不是整条丢掉
///
/// BA 裁定：不许用「把 `note` 整个丢掉」绕过 —— `note` 是真实业务信息
/// （属性说明），丢了会伤回答质量。所以命中的标识符串被换成
/// `…`，**周围的业务文字原样保留**：
/// `关联模型ID(对应llm_model.model_id)` → `关联模型ID(对应…)`。
///
/// # 为什么不合并进 `scan_modeling_terms`
///
/// 那个函数住在 `src/domain/view.rs`（**禁改红线**），且它服务的是
/// 「整份客户视图能否发布」的判定；这一道服务的是「**单个字段出库**」。
/// 职责不同、层不同，所以并列而不是塞进去。
pub use crate::domain::identifier_shape::scrub_identifiers;

/// 清洗一个 `Option<String>` 类型的 `note`。
fn scrub_note(note: &Option<String>) -> Option<String> {
    note.as_deref().map(scrub_identifiers)
}

/// 清洗一组属性的 `note`。
///
/// # ⚠️ `sample` **没有**过这道清洗 —— 这是**已知缺口**，如实记在这
///
/// `sample` 是**样例数据值**，不是说明文字。当前可见 fixture 里 `sample`
/// **全为空**（`PropertyView.sample` 全 `None`），**未观察到**命中形态判据的样例值。
///
/// ⚠️ 这里**只能写「未观察到」，不能写「不可达」**：观察集为空时，
/// 「没有一个命中」是**空集的平凡真命题**，**推不出**形态判据在这里不会命中。
/// （早先注释写的「实测 1051 列无一命中 ⇒ 判为不可达」就是把空集当成了证据，
/// 已由 QA 复验纠正 —— 空集不是证据。）
///
/// **为什么不顺手也洗**：形态判据分不出「列名」与「值」——
/// `功能编码，如 jd_api` 里的 `jd_api` 是**样例值**，照样会被形态判据隐去
/// （`note` 那侧已经这样了，见 B1 的误伤面）。对 `sample` 这么做会**直接删掉
/// 客户判断「这是不是我要的东西」的唯一线索** —— 而那正是 `sample` 存在的理由。
/// 代价大于收益。
///
/// 一旦真出现带标识符的 `sample`，正确做法是**按来源判定**（谁写进去的
/// 是不是标识符），而不是把形态判据扩到这里。缺口登记见
/// `tests/test_mcp_server.rs` 的 `todo_identifier_defenses_have_own_assertions`。
fn scrub_properties(props: &[PropertyView]) -> Vec<PropertyView> {
    props
        .iter()
        .map(|p| PropertyView {
            name: p.name.clone(),
            note: scrub_note(&p.note),
            required: p.required,
            sample: p.sample.clone(),
        })
        .collect()
}

// ──────────────────────────────────────────────────────────
// 工具目录
// ──────────────────────────────────────────────────────────

/// 一项工具声明。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

fn no_args() -> Value {
    json!({ "type": "object", "properties": {}, "required": [] })
}

fn one_str_arg(arg: &str, desc: &str) -> Value {
    json!({
        "type": "object",
        "properties": { arg: { "type": "string", "description": desc } },
        "required": [arg]
    })
}

/// **全部工具**。五个，覆盖 §9.5「至少 5 个客户级业务问题」。
///
/// ⚠️ 参数名与字段名一律避开 `scan_modeling_terms` 的守门词
/// （`key` / `evidence` / `cardinality` …）—— 守门器扫的是**整个序列化结果**，
/// 工具参数名也在其中。
pub fn catalog() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_concepts",
            title: "列出业务概念",
            description:
                "这个系统里到底有哪几样东西？返回全部业务概念（含各自的确认状态）\
                 以及尚未归入任何概念的对象。用它回答「系统里有哪些业务对象」。",
            input_schema: no_args(),
        },
        ToolDef {
            name: "describe_concept",
            title: "介绍一个业务概念",
            description:
                "某个业务概念管什么？返回它的成员对象、属性、关系，以及这个分组\
                 是否已经过业务确认。用它回答「客户这个概念包含什么」。",
            input_schema: one_str_arg("name", "业务概念名，如「客户」"),
        },
        ToolDef {
            name: "describe_object",
            title: "介绍一个对象",
            description:
                "某个对象是什么？返回它的说明、属性、所属上层对象、别名，\
                 以及它和别的对象之间的所有关系。用它回答「订单这个对象有哪些信息」。",
            input_schema: one_str_arg("name", "对象名，如「订单」"),
        },
        ToolDef {
            name: "list_relations",
            title: "列出对象之间的关系",
            description:
                "哪些对象之间有关系、朝哪个方向、有多少？可选按某一个对象过滤。\
                 用它回答「订单和什么有关系」。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "object": { "type": "string", "description": "只看与这个对象有关的关系；省略则全部" }
                },
                "required": []
            }),
        },
        ToolDef {
            name: "search_objects",
            title: "按名字找业务对象或概念",
            description:
                "系统里有没有和这个词相关的东西？同时在概念名与对象名里做包含匹配。\
                 用它回答「有没有跟限流有关的东西」。",
            input_schema: one_str_arg("query", "要找的词，如「限流」"),
        },
    ]
}

// ──────────────────────────────────────────────────────────
// 分发
// ──────────────────────────────────────────────────────────

/// 工具调用的失败。
#[derive(Debug, Clone, PartialEq)]
pub enum ToolError {
    /// 工具不存在 → 协议错误 `-32602`（规范 Tools 章的例子就是这个码）。
    Unknown(String),
    /// 参数不对 → 协议错误 `-32602`。
    BadParams(String),
    /// 要找的对象/概念不存在 → 执行错误（`isError: true`）。
    /// 连接是好的，只是这一问没有答案。
    NotFound(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::Unknown(n) => write!(f, "没有这个工具：{n}"),
            ToolError::BadParams(m) => write!(f, "参数不对：{m}"),
            ToolError::NotFound(m) => write!(f, "{m}"),
        }
    }
}

fn str_arg(params: &Value, name: &str) -> Result<String, ToolError> {
    match params.get(name) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(_) => Err(ToolError::BadParams(format!("参数 {name} 必须是非空字符串"))),
        None => Err(ToolError::BadParams(format!("缺少参数 {name}"))),
    }
}

/// 调一个工具，返回**未过守门器**的结构化数据。
///
/// 返回 `Err` 的三种情形都不含「数据」，所以守门器闸在 `Err` 路径上
/// 仍然有意义（防止错误文案里带出术语）。
pub fn call(ctx: &OntologyContext, name: &str, arguments: &Value) -> Result<Value, ToolError> {
    match name {
        "list_concepts" => Ok(json!({
            "source": ctx.source_block(),
            "concepts": ctx
                .overview
                .concepts
                .iter()
                .map(|c| json!({
                    "name": c.name,
                    "summary": c.summary,
                    "member_count": c.members.len(),
                    "members": c.members.iter().map(|m| &m.name).collect::<Vec<_>>(),
                    "property_count": c.properties.len(),
                    "confirmation": ctx.confirmation.get(&c.name),
                }))
                .collect::<Vec<_>>(),
            "objects_not_in_any_concept": ctx
                .overview
                .uncovered
                .iter()
                .map(|u| json!({ "name": u.name, "property_count": u.property_count }))
                .collect::<Vec<_>>(),
            "counts": {
                "concepts": ctx.overview.stats.concept_count,
                "objects_total": ctx.overview.stats.objects_total,
                "objects_in_concepts": ctx.overview.stats.objects_in_groups,
                "objects_not_in_any_concept": ctx.overview.stats.objects_uncovered,
            },
            "notes": ctx.overview.notes,
        })),

        "describe_concept" => {
            let want = str_arg(arguments, "name")?;
            let card = ctx
                .overview
                .concepts
                .iter()
                .find(|c| c.name == want)
                .ok_or_else(|| ToolError::NotFound(format!("没有叫「{want}」的业务概念")))?;
            Ok(json!({
                "source": ctx.source_block(),
                "name": card.name,
                "summary": card.summary,
                "confirmation": ctx.confirmation.get(&card.name),
                "members": card.members,
                "properties": card.properties,
                "relations": card.relations,
                "relations_inside_skipped": card.relations_inside_skipped,
                "duplicate_member_names": card.duplicate_member_names,
                "properties_dropped": card.properties_dropped,
                // **只给与这个概念有关的降级说明** —— 全局 notes 灌进来
                // 会把「别处的 20 条告警」混进这一问的答案里，
                // 下游 LLM 会当成这个概念的问题转述给客户。
                "notes": ctx.notes_about(&card.name),
            }))
        }

        "describe_object" => {
            let want = str_arg(arguments, "name")?;
            let obj = ctx
                .view
                .objects
                .iter()
                .find(|o| o.name == want)
                .ok_or_else(|| ToolError::NotFound(format!("没有叫「{want}」的对象")))?;
            // 属于哪个概念：按成员名反查，不按 key（客户只认名字）
            let concept = ctx
                .overview
                .concepts
                .iter()
                .find(|c| c.members.iter().any(|m| m.name == obj.name))
                .map(|c| c.name.clone());
            Ok(json!({
                "source": ctx.source_block(),
                "name": obj.name,
                // ⚠️ `note` 出库前过标识符清洗（B1）：它是**列注释原文**，
                // 真实数据里带 `llm_model.model_id` / `customer_id`。
                "note": scrub_note(&obj.note),
                "belongs_to": obj.belongs_to,
                "aliases": obj.aliases,
                "properties": scrub_properties(&obj.properties),
                "relations": ctx.relations_of(&obj.name),
                "part_of_concept": concept,
                "confirmation": concept.as_deref().and_then(|c| ctx.confirmation.get(c)),
            }))
        }

        "list_relations" => {
            // 省略 `object` = 全部关系。传了就只给与它有关的。
            let filter = match arguments.get("object") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.trim().is_empty() => None,
                Some(Value::String(s)) => Some(s.clone()),
                Some(_) => return Err(ToolError::BadParams("参数 object 必须是字符串".into())),
            };
            let rows: Vec<Value> = ctx
                .view
                .relations
                .iter()
                .filter(|r| match &filter {
                    None => true,
                    Some(f) => &r.from == f || &r.to == f,
                })
                .map(|r| {
                    json!({
                        "from": r.from,
                        "relation": r.meaning,
                        "to": r.to,
                        "how_many": r.how_many,
                    })
                })
                .collect();
            if filter.is_some() && rows.is_empty() {
                return Err(ToolError::NotFound(format!(
                    "「{}」没有出现在任何关系里",
                    filter.unwrap()
                )));
            }
            Ok(json!({
                "source": ctx.source_block(),
                "filter": filter,
                "count": rows.len(),
                "relations": rows,
            }))
        }

        "search_objects" => {
            let q = str_arg(arguments, "query")?;
            let concepts: Vec<Value> = ctx
                .overview
                .concepts
                .iter()
                .filter(|c| {
                    c.name.contains(&q)
                        || c.members.iter().any(|m| m.name.contains(&q))
                        || c.properties.iter().any(|p| p.name.contains(&q))
                })
                .map(|c| json!({
                    "name": c.name,
                    "matched_members": c.members.iter()
                        .filter(|m| m.name.contains(&q)).map(|m| &m.name).collect::<Vec<_>>(),
                    "confirmation": ctx.confirmation.get(&c.name),
                }))
                .collect();
            let objects: Vec<Value> = ctx
                .view
                .objects
                .iter()
                .filter(|o| {
                    o.name.contains(&q)
                        || o.aliases.iter().any(|a| a.contains(&q))
                        || o.properties.iter().any(|p| p.name.contains(&q))
                })
                .map(|o| json!({ "name": o.name, "note": scrub_note(&o.note), "property_count": o.properties.len() }))
                .collect();
            if concepts.is_empty() && objects.is_empty() {
                return Err(ToolError::NotFound(format!("没有与「{q}」相关的概念或对象")));
            }
            Ok(json!({
                "source": ctx.source_block(),
                "query": q,
                "concepts": concepts,
                "objects": objects,
            }))
        }

        other => Err(ToolError::Unknown(other.to_string())),
    }
}

// ──────────────────────────────────────────────────────────
// 守门器（AC-005）
// ──────────────────────────────────────────────────────────

/// AC-005 闸门：对**每一个**工具返回值执行，命中即拒绝外送。
///
/// # 为什么返回 `Err` 而不是「过滤掉再发」
///
/// 过滤会让泄漏**变得不可见** —— 我们自己都看不出漏了什么，
/// 下一次就再也查不出来。fail-closed 的意思是「这一问不给数据」，
/// 而不是「给一份看起来干净、其实少了一半的数据」。
///
/// # 命中词条只进 **stderr**，不进响应
///
/// 命中的词本身就是**建模黑话**（「外键」「基数」…）。把它写进返回给
/// 客户端的 `message`，等于**用错误响应把守门词送出去了** ——
/// 闸门挡住了正文，却从错误信息里漏了后门。所以响应里只说
/// 「命中 N 项」，具体是哪些词打到 stderr（运维看得见，客户看不见）。
pub fn guard(value: &Value) -> Result<String, String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| format!("返回值序列化失败（这本身就是个 bug）：{e}"))?;
    let leaked = crate::domain::view::scan_modeling_terms(&text);
    if leaked.is_empty() {
        Ok(text)
    } else {
        eprintln!(
            "✗ mcp-server 守门器命中 {} 项建模术语，已拒绝外送：{:?}",
            leaked.len(),
            leaked
        );
        Err(format!(
            "这一问不给数据：内容里出现了 {} 项只有内部使用的术语（为免泄漏，此处不列出具体词条）。这是发布闸门，请联系维护者。",
            leaked.len()
        ))
    }
}

/// **错误文案**也要过守门器 —— 这一条很容易漏。
///
/// 错误信息会把**客户端传进来的字符串**原样回显（「没有叫「外键表」的对象」）。
/// 成功路径的返回值过闸，错误路径若不过，闸门就从后门漏了：
/// 客户端（或读它的 LLM）照样拿到了守门词。
///
/// 所以错误文案一律走这里：命中就换成**不带具体词条**的通用说明。
pub fn safe_error(reason: &str) -> String {
    if crate::domain::view::scan_modeling_terms(reason).is_empty() {
        return reason.to_string();
    }
    "这一问给不出答案：输入的内容里出现了只有内部使用的术语（为免泄漏，此处不列出具体词条）。".to_string()
}

/// 服务级 instructions —— 客户端会把它交给 LLM 当系统提示的素材。
///
/// 目的是让下游知道**怎么处理 `confirmation`**：
/// 未经业务确认的分组，转述时要带上限定，**不要**当既成事实说。
///
/// # ⚠️ 项目名为什么也要过守门器
///
/// 这段文案会进 LLM 的**系统提示**，与 `tools/call` 的返回值是同一条外送路径。
/// 项目名由建项目的人给，**不受 `view::project` 投影约束** —— 那条投影只管
/// 冻结快照里的实体，项目名是**启动参数**（`--project`），走的是另一条路。
/// 所以项目名若含建模术语，闸门在这里就漏了。
///
/// 处置与错误文案同口径（[`safe_error`］）：命中就换成**不含具体词**的通用说法。
/// 注意这**不是**静默改写项目名 —— 说清「项目名未通过发布闸门」，
/// 运维一看就知道该去改项目名，而不是以为项目名正常。
pub fn instructions(ctx: &OntologyContext) -> String {
    let project = if crate::domain::view::scan_modeling_terms(&ctx.project).is_empty() {
        ctx.project.clone()
    } else {
        eprintln!(
            "✗ mcp-server 项目名 {:?} 含建模术语，instructions 里已隐去（项目名不受快照投影约束）",
            ctx.project
        );
        "（项目名未通过发布闸门）".to_string()
    };
    format!(
        "本服务回答关于「{}」项目第 {} 版业务模型的问题，该版本已冻结（业务模型不再变动）。\
         回答业务问题时请使用下列工具。\
         注意：每个概念都带一个 confirmation 字段。status 为 unconfirmed 表示这个分组\
         由工具或规则归并得出、尚未经业务人员确认；转述时请带上「此分组未经业务确认」\
         这类限定，不要把它当成已经确认的事实。",
        project, ctx.version
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据是**形态**不是词表 —— 这条测试就是这句话的执行。
    ///
    /// 输入是**真实 E2E 产物里的注释原文**（`code/out/e2e-schema.json` 可核），
    /// 期望值是「标识符被替换、业务文字保留」。
    #[test]
    fn scrub_replaces_identifiers_by_shape_keeping_business_text() {
        // 真实原文 → 局部替换
        assert_eq!(
            scrub_identifiers("关联模型ID(对应llm_model.model_id)"),
            "关联模型ID(对应…)"
        );
        assert_eq!(scrub_identifiers("核销人customer_id"), "核销人…");
        // 同型但**没在真实数据里出现过**的串也必须被抓 ——
        // 证明判据不是「记住这几个串」，而是「认得出这个形状」。
        assert_eq!(scrub_identifiers("指向 billing_account"), "指向 …");
        assert_eq!(scrub_identifiers("zzz_qqq"), "…");
        assert_eq!(scrub_identifiers("a.b"), "…");
    }

    /// **不许**把清洗做成「整条丢掉」—— 那会伤回答质量（BA 裁定）。
    #[test]
    fn scrub_keeps_clean_text_untouched() {
        for keep in [
            "子账号ID",              // 真实干净注释
            "客户默认QPM",
            "模型系列(如: GPT, Claude, GLM)",
            "0: llm, 1: emb",
            "license key",          // 空格分隔，不是 snake_case
            "GLM-5",                // 连字符
            "POINT/RMB/USD",        // 斜杠分隔
            "供应商配置(base_url, api_key等)", // ← 这个**会**被清洗，见下
        ] {
            if keep == "供应商配置(base_url, api_key等)" {
                continue;
            }
            assert_eq!(scrub_identifiers(keep), keep, "干净文本不该被改动：{keep}");
        }
    }

    /// 真实数据里被清洗的**全部** 6 条注释（E2E 实测：1051 列里 6 列命中）。
    /// 钉住这个集合，防止将来误伤面悄悄扩大。
    #[test]
    fn scrub_hits_exactly_the_six_real_e2e_comments() {
        let hit = [
            "功能编码，如 jd_api",
            "核销人customer_id",
            "供应商配置(base_url, api_key等)",
            "关联模型ID(对应llm_model.model_id)",
        ];
        for h in hit {
            assert_ne!(
                scrub_identifiers(h),
                h,
                "真实脏注释应当被清洗：{h}"
            );
        }
        // ⚠️ `功能编码，如 jd_api` 与 `供应商配置(base_url, api_key等)` 属于
        // **误伤面**：前者是**样例值**、后者是 JSON 字段名，都不是表名/列名。
        // 形态判据分不出「列名」与「值」，所以一并隐去 ——
        // 宁可少一个样例值，不许漏一个源标识符（fail-closed 方向）。
    }
}
