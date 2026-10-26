//! 阶段 1 的输入 / 输出数据形状 —— **协议中立**。
//!
//! 这里没有任何 A2A / MCP / ACP / HTTP 的痕迹：没有 `endpoint`、没有 `jsonrpc`、
//! 没有 `task_id`。协议 adapter 负责把 [`ExtractionGroup`] 翻译成那个协议的请求，
//! 再把响应翻译回 [`GroupExtraction`]。**形状变了也只动 adapter，不动本文件。**
//!
//! # 设计点 ①：让 agent 做「映射」，不做「命名」
//!
//! 规格 §7.1 的原话：输出格式从「请你定义这些类型」改成
//! 「**请把每一列映射到下列已有类型，或标记为新类型**」。
//!
//! 于是输入里必须有 [`ExtractionGroup::existing_types`]（已有类型清单），
//! 输出里必须有 [`CandidateType::is_new`]（这一组提的是**新类型**还是
//! **映射到已有类型**）。只有「候选」而没有「已有类型清单」的接口，
//! 等于把开放式命名又塞回给 agent —— 那正是幻觉的来源。
//!
//! # REQ-022：契约补齐到「**从候选能重建提案**」
//!
//! REQ-013 定的形状只够**核对**（设计点③：声明了哪一列是真存在的），
//! 不够**重建**（规格 §7.1 阶段 4 的字面做法）。缺的是：
//!
//! | 提案条目字段 | REQ-013 形状 | REQ-022 起 |
//! | --- | --- | --- |
//! | `payload.description` | ❌ 没有 | ✅ [`CandidateType::description`] |
//! | `payload.aliases` | ❌ 没有 | ✅ [`CandidateType::aliases`] |
//! | `payload.evidence` | ❌ 没有 | ✅ [`CandidateType::evidence`] |
//! | `properties[].note` | ❌ 没有 | ✅ [`CandidateProperty::note`] |
//! | `properties[].nullable` | ⚠️ `Option`（可缺） | ⚠️ 仍是 `Option`，**重建时缺失即报错** |
//! | `properties[]` 名字来源 | ❌ 没有 | ✅ [`CandidateProperty::origin`] |
//! | `add_link_type` 条目 | ❌ 不投影 | ✅ [`CandidateLink`] + [`GroupExtraction::links`] |
//!
//! 全部**向后兼容**（`#[serde(default)]` / `skip_serializing_if`），
//! REQ-013 形状的实现与登记照旧能跑；区别只在「重建路径能不能用它重建出
//! 与规则路径**逐字节相同**的提案」—— 那条等价性由
//! `tests/test_agent_orchestration.rs` 里的判据测试钉住。
//!
//! ⚠️ **`nullable` 为什么不改成 `bool`**：它是**源库的事实**，不是 agent 的判断。
//! 阶段 1 的输入里就有 schema 片段，缺这一项说明 adapter 没读它；
//! 重建路径对此**报错**（fail-closed）而不是猜 `false` ——
//! 猜出来的 `nullable` 会直接写进客户视图的「可空」标记，那是要给人看的合同。

use serde::{Deserialize, Serialize};

use crate::extract::introspect::{SchemaReport, TableMeta};

// ──────────────────────────────────────────────────────────
// 输入（规格 §7.1 阶段 1 的五个输入）
// ──────────────────────────────────────────────────────────

/// 组级溯源 —— 阶段 0 勘查留下的痕迹，随组一路传到 agent 的输入里。
///
/// **为什么单独拎出来**：规格的输入清单里写的是「schema 片段」，
/// 但 [`TableMeta`] 只有表级信息。要让规则实现把它拼回一份
/// [`SchemaReport`]（`propose::extract` 的入参），就得带上报告级的几个字段。
/// 与其让 `ExtractionGroup` 直接内嵌整个 `SchemaReport`（那样「片段」就名不副实），
/// 不如把这几个字段显式列出来。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupSource {
    pub schema: String,
    pub introspected_at: String,
    pub server_version: String,
    /// 勘查时服务端回读的 `transaction_read_only`（AC-101）。
    pub transaction_read_only: String,
    pub foreign_key_count: usize,
}

/// 一列的采样值 —— **脱敏后的少量样本**。
///
/// 规格阶段 2 要核对「采样值是否真的出现在那一列」，所以样本必须随声明一起传。
/// 本 crate 源库只读且零业务数据行，样本只能由外部勘查步骤提供；
/// **阶段 1 的 agent 不负责采样**。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColumnSample {
    pub table: String,
    pub column: String,
    /// 采样值（脱敏后）。空 = 没有采样。
    #[serde(default)]
    pub values: Vec<String>,
}

/// 决策清单约束 —— 阶段 0 从「决策清单」沉淀下来、必须对 agent 生效的硬约束。
///
/// **本批只固定形状**：规格没有给具体词表和阈值，
/// 这里把三个已知的约束项列出来（守门词、证据必须机器可验证、采样命中数下限），
/// 数值与词表留待实测校准（规格 §8.5 明写「数值阈值待切片 1 实测后校准」）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionList {
    /// 禁用词元 —— 命中即判失败的词（客户视图的守门词）。
    #[serde(default)]
    pub forbidden_terms: Vec<String>,
    /// 是否强制要求「属性 ← 来源列」可核对（设计点③）。
    #[serde(default = "default_true")]
    pub require_verifiable_evidence: bool,
    /// 采样值至少要命中几个才算证据成立。
    #[serde(default)]
    pub min_sample_match: usize,
}

fn default_true() -> bool {
    true
}

impl Default for DecisionList {
    fn default() -> Self {
        Self {
            forbidden_terms: Vec::new(),
            require_verifiable_evidence: true,
            min_sample_match: 0,
        }
    }
}

/// 一个**已有类型** —— 阶段 0 之前就在本体里的类型。
///
/// 设计点④「已有类型的 `key` 必须稳定」：`key` 一经生成就不可变，
/// 改名只改 `display_name`。所以这里的 `key` 是**跨轮次比对的锚点**，
/// 不是这一轮临时起的名字。
///
/// | 做法 | 后果 |
/// | --- | --- |
/// | 每轮重新生成 key | 第 2 轮把「设备」抽成「Dev」，diff 显示成「删了设备、加了 Dev」 |
/// | **key 稳定、只改显示名** ✅ | diff 干净，版本历史可读 |
///
/// # REQ-014 · F4：这条不变量**不再只是一句注释**
///
/// REQ-013 时它没有任何代码或测试守着 —— 注释说「不可变」，代码照收不误。
/// 现在由 [`verify_mappings`] 查两件事，各对应一个
/// [`MappingViolation`] 变体：
///
/// - [`MappingViolation::ExistingKeyDeclaredAsNew`]：声称「新类型」却占了个**已存在**
///   的 key —— key 是跨轮次锚点，占用它等于凭空多出一个类型；
/// - [`MappingViolation::ExistingDisplayNameChanged`]：key 命中但 `display_name`
///   与已知值不同 —— 改名**是允许的**（设计点④原话「改名只改 display_name」），
///   但它必须是**看得见的一轮事件**，而不是每轮重跑时悄悄漂掉的副作用。
///
/// 与其余违规一样，**只报错不丢弃**：核对事实与决定取舍是两件事（见函数文档）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistingType {
    pub key: String,
    pub display_name: String,
}

/// **阶段 1 的一次调用的输入** —— 一个组（规格：「每组一次调用」）。
///
/// 规格阶段 0「按表分组」+ 设计点②「分组按表，不按业务域」：
/// **一组的边界就是表的边界**，这里由 [`ExtractionGroup::schema`] 表达。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionGroup {
    /// 组标识 —— 失败时要能指名道姓地说「这一组失败了」（设计点⑤）。
    pub group_id: String,
    pub source: GroupSource,
    /// **作用域** —— 这一组要抽的表（设计点②：一表一组，不做跨表聚类）。
    pub schema: Vec<TableMeta>,
    /// **上下文** —— 全库表名清单，**只供机械式 FK 消歧**，不进作用域。
    ///
    /// # 为什么要有它（REQ-014 · 缺陷 F1）
    ///
    /// 「`customer_id` 指向哪张**真实存在的**表」是**可核验的事实**，
    /// 不是 agent 的判断：设计点①说 agent 做映射不做命名、③说证据必须机器可验证。
    /// 可它过去只在本组表里查 —— FK 目标在组外时，关系就「推不出来」，
    /// 属性名回落成注释值。于是**同一条列在阶段 1 和阶段 3/4 得到两个名字**
    /// （实测：`t_dev.t_customer_id` 阶段 1 =「客户标识」，阶段 3/4 =「所属客户」）。
    ///
    /// # 为什么只有表名，不是整份 schema
    ///
    /// 它是**存在性判据**，不是第二份待抽的 schema。把列级元数据也塞进来，
    /// 就等于在输入里摆了第二份库结构，后人迟早会从组外表抽属性 ——
    /// 那正是设计点②要挡的跨表聚类。**越窄的类型越不容易被误用**，
    /// 真需要更多上下文时再加字段（加字段是廉价的，撤不回越界的抽取）。
    #[serde(default)]
    pub context_tables: Vec<String>,
    /// 样本（脱敏）。
    #[serde(default)]
    pub samples: Vec<ColumnSample>,
    /// 决策清单约束。
    #[serde(default)]
    pub constraints: DecisionList,
    /// 领域提示（「这是一家做 SaaS 计费的厂商」之类）。
    #[serde(default)]
    pub domain_hints: Vec<String>,
    /// 已有类型清单（设计点① —— **必须有**，否则 agent 只能自由命名）。
    #[serde(default)]
    pub existing_types: Vec<ExistingType>,
}

/// **交给抽取器的一份完整输入 = 作用域报告 + 上下文表名**。
///
/// # 为什么不合成一个 `SchemaReport`
///
/// [`crate::extract::introspect::SchemaReport`] 只有 `tables` 一个表清单，
/// 塞上下文就得给它加字段 —— 而那个字段会**跟着 `tables` 一起被遍历**，
/// 于是「上下文」会被当成「作用域」，正是 F1 要拆开的那个混淆。
/// 用一对字段把它表达死，调用方想漏传上下文都得漏得显眼。
///
/// （另有一条硬约束：`tests/test_fk_naming.rs` 与 `tests/drop_rate.rs`
/// 是两条 fail-closed 回归门禁的输入，禁止改动，而前者用**完整结构体字面量**
/// 构造 `SchemaReport` —— 给 `SchemaReport` 加字段会让那个受保护文件编译不过。）
#[derive(Debug, Clone)]
pub struct ExtractionInput {
    /// **作用域**：这一组要抽的表。抽取器只遍历它产出实体与统计。
    pub scope: SchemaReport,
    /// **上下文**：全库表名。只参与 FK 消歧，**不进作用域**。
    pub context_tables: Vec<String>,
}

impl ExtractionGroup {
    /// 把这一组拼回一份 [`SchemaReport`] —— **只有作用域**。
    ///
    /// ⚠️ 这份 report **不含上下文**。要跑抽取请用
    /// [`ExtractionGroup::to_extraction_input`]，否则组外的 FK 目标会被当成
    /// 不存在（缺陷 F1）。
    ///
    /// `counts` 按**本组**的表**重新算**，不沿用全局数字，**也不把上下文表算进来**：
    ///
    /// - 抽取器只遍历 `tables`，实体 / 属性 / 统计**全部**按这一批表产生
    ///   （`stats.tables_seen = report.tables.len()`）；
    /// - 把上下文表算进 `table_count`，`table_count` 就会声称覆盖了一批
    ///   **从没被遍历过**的表 —— 于是 `翻译覆盖 = N / tables_seen` 这类比值的
    ///   分母里混进没看过的表，单组统计当场说谎。
    ///
    /// 上下文表的身份只有一个：**这张表真实存在**。它是存在性判据，不是抽取对象，
    /// 所以既不进 `tables`，也不进 `counts`。
    pub fn to_schema_report(&self) -> SchemaReport {
        let table_count = self.schema.len();
        let column_count: usize = self.schema.iter().map(|t| t.columns.len()).sum();
        let commented_column_count: usize = self
            .schema
            .iter()
            .flat_map(|t| t.columns.iter())
            .filter(|c| c.comment.as_deref().is_some_and(|s| !s.trim().is_empty()))
            .count();
        SchemaReport {
            server_version: self.source.server_version.clone(),
            transaction_read_only: self.source.transaction_read_only.clone(),
            schema: self.source.schema.clone(),
            table_count,
            column_count,
            commented_column_count,
            foreign_key_count: self.source.foreign_key_count,
            tables: self.schema.clone(),
            introspected_at: self.source.introspected_at.clone(),
        }
    }

    /// 抽取器要的那份输入 —— **作用域 + 上下文**（REQ-014 · F1）。
    ///
    /// 这是 [`crate::agent::RuleBasedAgent::extract_group`] 唯一该用的入口：
    /// 走 [`ExtractionGroup::to_schema_report`] 会把上下文丢掉，
    /// 组外的 FK 目标就又「不存在」了。
    pub fn to_extraction_input(&self) -> ExtractionInput {
        ExtractionInput {
            scope: self.to_schema_report(),
            context_tables: self.context_tables.clone(),
        }
    }

    /// 本组的样本查表。
    pub fn sample_for(&self, table: &str, column: &str) -> Option<&ColumnSample> {
        self.samples
            .iter()
            .find(|s| s.table == table && s.column == column)
    }
}

/// 按表分组（规格阶段 0「按表分组」+ 设计点②「分组按表，不按业务域」）。
///
/// **为什么一表一组，而不是随便切块**：设计点②的理由是
/// 「哪些表属于同一业务域」正是要判断的事，交给 agent 分组就成了循环依赖。
/// 表边界是客观的，跨表归并留给阶段 3。所以这里**不做任何聚类**，
/// 一张表 = 一个组，只负责给稳定的 `group_id`。
pub fn derive_group_id(table: &str) -> String {
    format!("table:{table}")
}

// ──────────────────────────────────────────────────────────
// 输出（候选 + 映射声明）
// ──────────────────────────────────────────────────────────

/// **一个 (表, 列) 对** —— 映射声明的最小单位。
///
/// 设计点③要求证据必须机器可验证：agent 说「设备.序列号 ← `t_dev.sn`」，
/// 代码要能去核对 `t_dev` 确实有 `sn` 列。所以映射**必须是结构化的
/// `(表, 列)` 二元组**，不能是一句自然语言，也不能是 `"t_dev.sn"` 这样的裸串
/// —— 裸串还得再解析一次，而解析规则本身就是个可以出错的猜测。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnRef {
    pub table: String,
    pub column: String,
}

impl ColumnRef {
    pub fn new(table: &str, column: &str) -> Self {
        Self {
            table: table.to_string(),
            column: column.to_string(),
        }
    }
}

/// **属性名是从哪儿来的**（REQ-022 · 重建提案所需）。
///
/// ⚠️ **定义在 `extract::propose`**，这里只转出同一个类型 ——
/// 「名字怎么选」是**抽取层**的判断（REQ-005 质量门槛 / REQ-007 值域 /
/// REQ-008 关系推导），契约侧只是如实转述。两处各写一份同名枚举
/// 就是一份会漂移的重复定义，本项目吃过这种亏（ADR-003 记的
/// `ExtractedBy` 同名不同型就是同一类）。
pub use crate::extract::propose::PropertyOrigin;

/// 一个候选属性 —— **名字 + 它来自哪一列**。
///
/// `Default` 的存在只是为了给「最小构造」一个起点（`..Default::default()`）。
/// ⚠️ **它不是一个合法的阶段 1 产出**：`source` 为空 ⇒ 阶段 2 报
/// [`MappingViolation::NoSource`]；`nullable` / `origin` 为 `None` ⇒
///
/// 重建路径**报错**（见 [`crate::agent::assemble`])。也就是说，
/// 「用 Default 拼一个候选」在两道关卡上都过不去 —— 这是有意的。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateProperty {
    /// 客户可见的属性名（中文）。
    pub name: String,
    /// 「属性 ← 来源列」：**这条声明的可核对锚点**（设计点③）。
    pub source: ColumnRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,
    /// **REQ-022**：说明（值域、限定说明…）—— 提案的 `PropertyItem::note`。
    ///
    /// REQ-013 定的形状里没有它，于是「从候选重建提案」会**丢掉全部 note**：
    /// 提案内容变了，而变化是静默的（字段没了，看起来像「本来就没 note」）。
    /// 阶段 1 的输出必须承载阶段 4 要写的东西，否则重建不是等价而是改写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// **REQ-022**：名字的来源（见 [`PropertyOrigin`]）。
    ///
    /// 可选是为了**向后兼容**：REQ-013 之前登记的实现不声明它，
    /// 重建时按「不可重建」处理（重建路径**报错**，不猜），而不是反过来
    /// 让所有旧实现一次性作废。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<PropertyOrigin>,
}

impl CandidateProperty {
    /// 阶段 1 的最小构造（REQ-013 形状）—— 其余字段取「未知」。
    ///
    /// 给只需要 `name` + `source` 的调用方（阶段 2 的判据测试、
    /// 协议 adapter 的最小映射）留一个不会随字段增加而崩的入口。
    pub fn new(name: &str, source: ColumnRef, nullable: Option<bool>) -> Self {
        Self {
            name: name.to_string(),
            source,
            nullable,
            note: None,
            origin: None,
        }
    }
}

/// 一个候选类型（对应源库一张表，或跨表归并前的中间结果）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateType {
    /// **稳定 key**（设计点④：一经生成不可变，改名只改 `display_name`）。
    pub key: String,
    pub display_name: String,
    /// `false` = **映射到已有类型**（`key` 应命中 `existing_types`）；
    /// `true`  = 标记为**新类型**（设计点①的「或标记为新类型」那一支）。
    ///
    /// 故意做成显式布尔而不是靠「key 在不在已有清单里」推断 ——
    /// 推断的话，agent 打错一个已有 key 就被静默当成「映射成功」，
    /// 而它其实提了个四不像。显式声明让不一致可以被 [`verify_mappings`] 查出来。
    pub is_new: bool,
    #[serde(default)]
    pub properties: Vec<CandidateProperty>,
    /// **REQ-022**：实体说明 —— 提案的 `EntityPayload::description`。
    ///
    /// 规则实现填源库的表注释（`TableMeta::comment`），与
    /// [`crate::extract::propose::extract_with_context`] 的取值同源。
    /// REQ-013 定的形状里没有它 ⇒ 「从候选重建提案」会把实体的说明全抹掉，
    /// **提案内容变了，而且是静默地变**（字段消失看起来像「本来就没有」）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// **REQ-022**：别名 —— 提案的 `EntityPayload::aliases`。
    ///
    /// `[]` = 没有别名（**不是缺失**，与 snapshot 层的三态语义同一条纪律）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// **REQ-022**：证据 —— 提案的 `EntityPayload::evidence`。
    ///
    /// 规则实现填 `[{"table": …, "claim": "由源库表结构抽取"}]`。
    /// 留空是**合法**的（提案序列化时该字段本就被 `skip_serializing_if` 省略），
    /// 所以 `Vec::new()` 与「没有证据」在产物里是同一件事，不存在说谎空间。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<serde_json::Value>,
    /// **REQ-022**：这一组里**因注释质量门槛被拒用**的列数（REQ-005 的主动降级痕迹）。
    ///
    /// 统计里的 `props_comment_rejected` 记的就是它，而被拒的列**不会**变成任何
    /// 属性候选 —— 也就是说，从候选里数不出这个数。候选不报上来，重建路径就会把
    /// 真实库上的 6 次降级写成 0 次（REQ-005 的原话：「每一次降级都得留下痕迹」）。
    ///
    /// `None` = 实现没上报 ⇒ **重建路径报错**（与 `origin` 同一处置），
    /// 绝不用 0 顶替。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_comments: Option<usize>,
}

/// **一条候选关系**（REQ-022 · 阶段 3 跨组合成的输入）。
///
/// # 为什么阶段 1 现在会产出关系（与 REQ-013 的写法相反，如实记录）
///
/// REQ-013 时 [`GroupExtraction`] **按设计不投影 Link**，理由写在模块头：
/// 规格把「两张表都有 `device_id` → 是否提出 Link？」放在**阶段 3**。
/// 那个理由本身仍然成立 —— 阶段 3 决定**哪些**关系成为提案条目。
///
/// 但 RN-2 的 BA 裁定把「从候选重建提案」（规格 §7.1 阶段 4 的字面做法）
/// 变成硬要求之后，只投影实体的阶段 1 就**产不出与规则路径等价的提案**：
/// 真实 schema 上 91 个实体之外还有 51 条关系，缺了它们提案就不是同一份提案。
/// 于是形状从「阶段 1 不提关系」改成「**阶段 1 提关系候选，阶段 3 决定取舍**」：
///
/// | | REQ-013 ~ REQ-021 | REQ-022 起 |
/// | --- | --- | --- |
/// | 阶段 1 产出 | 候选类型 + 属性 + 映射声明 | 上面这些 **+ 关系候选** |
/// | 阶段 3 职责 | 跑规则管线决定关系 | **从各组的关系候选里合并取舍** |
///
/// 取舍仍是**代码**在做（[`crate::agent::assemble::rebuild_outcome`]）：
/// 候选声明「我看到 `t_dev.t_customer_id` 指向 `t_customer`」，
/// 「这条关系两端都在提案里吗」由代码核 —— 架构红线不变。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateLink {
    /// 关系的稳定标识（规则实现 = `{表}_{列}__{目标表}`）。
    ///
    /// 刻意**由提出方给全**，而不是重建时按规则拼：拼 key 就是往重建路径里
    /// 塞一份命名规则，那份规则迟早会与提出方漂移，而漂移是静默的。
    pub key: String,
    /// 人看的标签（规则实现 = `设备→客户`）。
    ///
    /// 同上，**不**由重建方从显示名反推：规则实现里的 `to_cn` 取的是
    /// 翻译表或表名，而实体的 `display_name` 还可能是表注释 ——
    /// 两者在真实数据上并不总相等，反推会造出第二份命名规则。
    pub target: String,
    /// 起点实体 key（`from_type`）。
    pub from_type: String,
    /// 终点实体 key（`to_type`）。
    pub to_type: String,
    /// 基数（`1:N` / `N:1`）。
    pub cardinality: String,
    /// 语义 —— ⭐ REQ-157：**来自 `relation-verbs.json`**（见该文件的 `verbs` 与
    /// `forbidden`）。⛔ 不是自由文本：字典外的词会被 `ontok-builder relation verbs
    /// check` 判成 `unknown` 并拒绝；泛词（`uses` / `associated_with` …）判成
    /// `forbidden` 同样拒绝；同义写法（`attaches_to` / `binds`）归一成主键。
    pub semantic: String,
    /// 证据（规则实现 = `[{"table":…, "column":…, "claim":…}]`）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<serde_json::Value>,
}

/// **阶段 1 的一组的产出** —— 候选 + 映射声明。
///
/// # 关系（Link）为什么在这里（REQ-022）
///
/// REQ-013 时本结构**只有 `types`**，`GroupExtraction` 的字段也是证据：
/// 「阶段 1 按设计不投影 Link，跨组结论它说了不算」。
/// 现在多一个 `links`：阶段 1 **声明**关系候选，阶段 3 跨组**决定取舍**
/// （见 [`CandidateLink`] 的文档）。默认空 ⇒ 向后兼容：
/// REQ-013 形状的实现（协议 adapter 的第一版）不受影响。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupExtraction {
    #[serde(default)]
    pub types: Vec<CandidateType>,
    /// **REQ-022**：关系候选（阶段 3 跨组合成的输入）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<CandidateLink>,
}

// ──────────────────────────────────────────────────────────
// 失败（设计点⑤）
// ──────────────────────────────────────────────────────────

/// 一组抽取失败 —— **显式、可命名、带最后一次原因**。
///
/// 进 `extracted_by.failed_groups` 后**不参与收敛判定**（§8.5）：
/// 否则「连续两轮 diff 小于阈值」会把「什么都没抽」误判成「已收敛」。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupFailure {
    pub group_id: String,
    /// 组的代表性表名（阶段 0 一表一组，所以取首表；空组则记 `"(空组)"`）。
    pub table: String,
    /// 实际尝试次数（含第一次）。
    pub attempts: u32,
    /// 最后一次失败的原因。
    pub reason: String,
}

/// 一组的结果 —— **成功与失败是同一个类型**。
///
/// 设计点⑤「不静默跳过」的落地：返回类型里就带着失败这一支，
/// 调用方**不可能**只处理成功而把失败悄悄丢掉 —— 编译器会逼它 match。
///
/// # 真实形状（REQ-013 更正：曾被描述成四个平铺字段）
///
/// `Failed` 是**嵌套**结构，不是平铺。四个字段装在 [`GroupFailure`] 里：
///
/// ```text
/// GroupOutcome::Ok { extraction: GroupExtraction }
/// GroupOutcome::Failed { failure: GroupFailure { group_id, table, attempts, reason } }
/// ```
///
/// 所以构造是两层（`GroupOutcome::Failed { failure: GroupFailure { .. } }`，
/// 见 [`crate::agent::drive_group`]），解构是两层（`Failed { failure }` 再取字段），
/// **不存在** `GroupOutcome::Failed { group_id, table, attempts, reason }` 这种写法。
pub enum GroupOutcome {
    Ok { extraction: GroupExtraction },
    /// 失败详情是**嵌套**的 [`GroupFailure`]，不是平铺字段。
    Failed { failure: GroupFailure },
}

impl GroupOutcome {
    /// 成功的产出；失败则 `None`。
    pub fn ok(&self) -> Option<&GroupExtraction> {
        match self {
            GroupOutcome::Ok { extraction } => Some(extraction),
            GroupOutcome::Failed { .. } => None,
        }
    }

    /// 失败的详情；成功则 `None`。
    pub fn failure(&self) -> Option<&GroupFailure> {
        match self {
            GroupOutcome::Ok { .. } => None,
            GroupOutcome::Failed { failure } => Some(failure),
        }
    }
}

// ──────────────────────────────────────────────────────────
// 阶段 2 的纯代码核对（★ 不可外包）
// ──────────────────────────────────────────────────────────

/// 一条映射声明**核不过去**的原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MappingViolation {
    /// 声明的属性**没有**来源列 —— 证据不可核对（设计点③）。
    NoSource { type_key: String, property: String },
    /// 声明的表在这一组里**不存在**。
    UnknownTable {
        type_key: String,
        property: String,
        table: String,
    },
    /// 表在，但**没有这一列**。
    UnknownColumn {
        type_key: String,
        property: String,
        table: String,
        column: String,
    },
    /// 声明「映射到已有类型」，但 `key` 不在已有类型清单里 —— agent 打错了 key。
    UnknownExistingType { type_key: String },
    /// 声称「新类型」，可这个 `key` **已经存在**（设计点④ · REQ-014）。
    ///
    /// REQ-013 时这一支是**静默通过**的：`verify_mappings` 只查了
    /// `!is_new && 不在清单里`，镜像的那一半没人管。于是 agent 拿一个
    /// **已占用的 key** 声明「这是新类型」可以畅通无阻 ——
    /// 而 key 正是跨轮次比对的锚点，占用它等于凭空多出一个类型，
    /// 下一轮它又会以「已有类型」的姿态出现，diff 立刻失真。
    ExistingKeyDeclaredAsNew { type_key: String },
    /// 已有类型的 `display_name` 被改动（设计点④ · REQ-014）。
    ///
    /// **改名本身是合法的**（④：改名只改 `display_name`，key 不动）。
    /// 这里报的不是「不许改」，而是「**改了必须看得见**」——
    /// 改名是应该进版本历史的一次显式事件，不该是每轮重跑时的静默副作用。
    /// 两个名字都留着，由人判断是「有意改名」还是「抽取漂了」。
    ExistingDisplayNameChanged {
        type_key: String,
        /// 这一轮声明的显示名。
        declared: String,
        /// 已有类型清单里的显示名。
        known: String,
    },
    /// 命中决策清单的禁用词。
    ForbiddenTerm {
        type_key: String,
        property: String,
        term: String,
    },
}

/// **核对一条映射声明**（阶段 2 的第一条，纯代码，不可外包）。
///
/// 规格阶段 2 原文：
///
/// > 每条声明的表 / 列是否真实存在于 schema？　否 → 丢弃
///
/// 本函数**只做这一条**。阶段 2 的另两条（「采样值是否真出现在那一列」
/// → 降权、「与已有快照比对重复」→ 丢弃）需要采样与快照上下文，
/// 属于后续批次 —— 本任务只把**声明的形状**做成可核对的，
/// 证明设计点③的「机器可验证」不是口号。
///
/// 返回空 `Vec` = 这一组每条声明都核过了。
///
/// # 核对的是**事实**，不做**取舍**
///
/// 所有违规一律**只报错、不丢弃**：丢弃与否是调用方（阶段 2 的策略）的决定。
/// 本函数是「查事实」的纯函数，把「打错 key 就扔掉」写在这里等于
/// 顺手替策略做了决定，而策略会随项目变。
///
/// # key 的核对是**双向**的（REQ-014 · F4）
///
/// 很容易只查一半：
/// - `is_new = false` 却不在清单里 ⇒ [`MappingViolation::UnknownExistingType`]（key 打错）；
/// - `is_new = true` 却在清单里 ⇒ [`MappingViolation::ExistingKeyDeclaredAsNew`]（占坑）。
///
/// REQ-013 只查了第一条，第二条静默通过 —— 那正是「key 稳定」变成注释的原因。
/// key 命中后还要比显示名，不一致报
/// [`MappingViolation::ExistingDisplayNameChanged`]（改名合法，但必须可见）。
pub fn verify_mappings(
    extraction: &GroupExtraction,
    group: &ExtractionGroup,
) -> Vec<MappingViolation> {
    let mut out = Vec::new();
    // key → 已知显示名。既要判「在不在」，也要判「名字变没变」，所以留映射而非集合。
    let existing: std::collections::HashMap<&str, &str> = group
        .existing_types
        .iter()
        .map(|t| (t.key.as_str(), t.display_name.as_str()))
        .collect();

    for t in &extraction.types {
        // key 与 `is_new` 必须自洽：**双向**查，缺一半就会漏。
        match (t.is_new, existing.get(t.key.as_str()).copied()) {
            // 「映射到已有类型」却不在清单里 ⇒ key 打错了。
            (false, None) => out.push(MappingViolation::UnknownExistingType {
                type_key: t.key.clone(),
            }),
            // 「新类型」却占了个已存在的 key ⇒ 不是新类型，是占坑（设计点④）。
            (true, Some(_)) => out.push(MappingViolation::ExistingKeyDeclaredAsNew {
                type_key: t.key.clone(),
            }),
            // key 命中已有类型：显示名被改就要报出来（④：改名只改 display_name，
            // 但改名必须是看得见的一轮事件，不是静默漂移）。
            (false, Some(known)) if known != t.display_name => {
                out.push(MappingViolation::ExistingDisplayNameChanged {
                    type_key: t.key.clone(),
                    declared: t.display_name.clone(),
                    known: known.to_string(),
                })
            }
            _ => {}
        }

        for p in &t.properties {
            if p.source.column.trim().is_empty() || p.source.table.trim().is_empty() {
                out.push(MappingViolation::NoSource {
                    type_key: t.key.clone(),
                    property: p.name.clone(),
                });
                continue;
            }
            let Some(table) = group.schema.iter().find(|x| x.name == p.source.table) else {
                out.push(MappingViolation::UnknownTable {
                    type_key: t.key.clone(),
                    property: p.name.clone(),
                    table: p.source.table.clone(),
                });
                continue;
            };
            if !table.columns.iter().any(|c| c.name == p.source.column) {
                out.push(MappingViolation::UnknownColumn {
                    type_key: t.key.clone(),
                    property: p.name.clone(),
                    table: p.source.table.clone(),
                    column: p.source.column.clone(),
                });
                continue;
            }
            // 禁用词只在**属性名**上判 —— 表名/列名是源库给的事实，不归我们管。
            for term in &group.constraints.forbidden_terms {
                if !term.trim().is_empty() && p.name.contains(term.as_str()) {
                    out.push(MappingViolation::ForbiddenTerm {
                        type_key: t.key.clone(),
                        property: p.name.clone(),
                        term: term.clone(),
                    });
                }
            }
        }
    }
    out
}
