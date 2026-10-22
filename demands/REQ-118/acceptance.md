# 验收标准：REQ-118 · AI 提案贯穿（名称 / 排除 / 分组）

> **依据 ADR-005**：决策点必须由 AI 先提案；**提案永不权威，未签不得进快照**。

## 一 · AC

### AC-001 · 三命令产出统一形状的提案

- `name suggest` / `exclude suggest` / `concept suggest --ai` 各产一份 JSON
- 每条提案含：`claim` / `evidence[]` / `confidence` / `reason` / `decided_by` / `reviewed_by`
- `reviewed_by` 初始必须为 **`null`**
- 判据：三份文件逐字段检查；⛔ 缺 `evidence` 或 `reason` 的条目不得出现（若模型没给，该条丢弃并计数）

### AC-002 · `name suggest` 与既有流程兼容

- 产出能被 `ontok-propose --names <file>` **直接消费**（即包含 `names` 映射或提供 `--as-names` 转换）
- 判据：真跑一次 `propose --names`（可用小样本），不报格式错

### AC-003 · `exclude suggest` 与导入闸兼容

- 产出能被 `ontok concept import`……**不**，是能被 **REQ-113 的 `--exclude`** 直接消费
- 字段：`table` / `category`（7 类枚举之一）/ `reason` / `decided_by` / `confidence`
- 判据：拿产出直接跑 `ontok-propose --exclude <产出>`，**通过导入校验**（不因格式被拒）

### AC-004 · `concept suggest --ai` 与概念声明兼容

- 产出能被 `ontok concept import --file <产出>` 直接导入（`version` / `declared_by` / `rule` / `history` / `groups`，digest 正确）
- 判据：真跑一次 `concept import`，`digest` 一致（`契约 一致`）

### AC-005 · 指纹缓存（三命令都要）

- 同输入第二次运行 ⇒ **0 token**、输出文件**逐字节相同**
- 判据：三条命令各跑两次，比较 sha256 与 token

### AC-006 · 预算闸

- 三条命令都支持 `--budget-tokens`；超限即**停**并报「已用/上限/停在第几项」
- 判据：`--budget-tokens 1` 各跑一次，确认**不产半份文件**

### AC-007 · 降级必须标注

- `concept suggest`（算法版）产出标 `source: "rule"`；`--ai` 版标 `source: "ai"`
- 判据：两种各跑一次，字段正确；⛔ 算法版不得冒充 `ai`

### AC-008 · 提案不权威（P2）

- 三份提案**都不能**直接产生本体/快照
- 判据：`name suggest` 的产出不喂 `--names` 时，本体里**不出现**这些名字；`exclude suggest` 的产出不喂 `--exclude` 时，表照常进本体
- 判据：`reviewed_by` 为 null 的提案进快照的路径**不存在**

### AC-009 · 证据与理由的质量（**可评审**）

- 每条 `evidence` 至少一条**可核实的依据**（表注释 / 列名 / 关系 / 文档片段）
- 判据：抽查 10 条，人工可据 evidence 判断真伪；⛔ 不许出现「模型认为」这类不可核实的理由

### AC-010 · 真实库实测 + 与对话版对比

对 `anspire_open_uat`（96 表）跑三条命令，并与既有产物逐条对比：

| 命令 | 对比对象 |
| --- | --- |
| `name suggest` | `code/out/table_names.json`（83 条，对话子 agent 产） |
| `exclude suggest` | `code/out/exclusions-ai.json`（12 条） |
| `concept suggest --ai` | `code/out/concept-declaration-ai-v2.json`（8 组） |

- 要求：输出「一致 / 产品版新增 / 对话版缺失」三类，**逐条说明**
- ⛔ **不要求结果相同**；差异本身是信息（对话版带额外上下文）

### AC-011 · 零依赖 / 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `/api` 鉴权与路由 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、**新增依赖** |
| 基线 | REQ-116 合并后的 lib 315 / doc 4 / 77 目标 813 |

## 二 · 纪律

- **H** 提案形状只许一处定义（三命令共用同一个结构体 + 同一个校验）
- **G** 变异三元组：删 `reviewed_by: null` / 去掉缓存 / 让算法版标 `ai`，每条必须被测试抓住
- **S** AC 也会错，发现直接说

---

## 三 · Dev 交付后的裁定

### A-1 · P4 只到文件级 ⇒ **必须修**

Dev 自报：`names` 的提案 sha 只在输入侧 `TableNames._proposal`，`ontok-propose` 解析时丢弃 ⇒ 本体溯源只有 agent/model/version，**没有「哪份提案文件」**；concept 同理。

ADR-005 的 P4 原文：「产物里能查到**这条用的是哪份提案、谁签的**」。查不到 = **审核过的提案无据可查**。

要求：`inputs.names` / `inputs.concepts` 与 `inputs.exclusions` 同构（`file`/`sha256`/`declared_by`/`reviewed_by`，`reviewed_by` 允许 null 但字段必须在）。

### A-2 · 排除过宽（21 vs 12）**不改提示词**

产品版多排 9 张（`usage_reconciliation_log`、`simulate_login_*`、`dim_customer_binding`、`instance_task_manager`…）。

**裁定：保持现状。** 产品版没有对话里的上下文；**为了让结果贴我那份 12 条去调提示词，等于把「对话里的隐性知识」当标准答案**——正是 ADR-005 要避免的。

⇒ 差异本身是产出：它说清了「只看 schema 会排出什么」，而这些正需要人签。
要求：9 条进「需要人工复核」清单，**给建议但不预设答案**。

### A-3 · `groups[]` 字段由 REQ-010 定义

AC-001 的六字段按 `proposals[]` 判；`groups[]` 不叠加 claim/confidence。报告须写清，免得 QA 判缺字段。

### A-4 · `low_confidence` 真实数据 0 条

模型最低置信度 ≥0.6，R4 只在单测验过 ⇒ **如实登记「真实数据未触发」**，⛔ 不许写「已验证」。

### A-5 · concept 单次全量请求的限制

96 实体一次 7.8k token；更大 schema 会撞 4096 `max_tokens` 截断、整份作废 ⇒ 登记已知限制 + 触发条件 + 将来修法（分块 + 合并）。

### A-6 · 值得记的两条实测

1. **缓存有效**：name 40,905 → **0 token**（8/8 命中）；exclude 41,430 → 0；concept 7,842 → 0。三份文件 sha256 两次逐字节相同。
2. **P2 结构保证**：`ProposalItem.reviewed_by` 是**私有字段、无 setter**，唯一构造器恒写 `None`；三条 suggest 模块零 `sqlx/PgPool`；`propose import` 需要 `project/items`，提案文件里没有 ⇒ **物理上进不去**。比「纪律上不许」强。
