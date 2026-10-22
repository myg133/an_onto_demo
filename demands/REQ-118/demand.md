# 需求：REQ-118 · AI 提案贯穿（名称 / 排除 / 分组）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-118 |
| 优先级 | **P0**（产品级原则的落地） |
| 状态 | 进行中（**等 REQ-116 合并后开工**——两者都改 `pipeline.rs` / `cli.rs`） |
| 基线 | `develop`（REQ-116 合并后） |
| 依据 | **ADR-005**：决策点必须由 AI 先提案 |

## 需求描述

### 三个决策点，现在都不在产品里

| 决策点 | 产品现状 | 实际怎么做的 | 违反 ADR-005 的点 |
| --- | --- | --- | --- |
| **表名/属性名的中文名** | `ontok-translate-names`（REQ-090）**存在但流程没用** | 对话里的子 agent 产出 `table_names.json`（`agent=ba-michael-…`） | 产品的本体用了对话的产物 |
| **哪些表不进本体** | 只接受 `--exclude <file>` | AI 逐张判（REQ-113 的 12 条），判断在产品外 | 判断不可复现、不在产品里 |
| **对象如何分组** | `concept suggest`（**算法**，非 AI） | AI 语义分组 8 组（REQ-115），也在对话里 | AI 语义分组不是产品能力 |

### 目标：三个 `suggest`，一个提案框架

```
ontok name suggest    --schema S --out names.proposal.json
ontok exclude suggest --schema S --out exclusions.proposal.json
ontok concept suggest --ai   --project P --version N --out groups.proposal.json
```

三者共用：

| 共用项 | 要求 |
| --- | --- |
| **提案形状** | ADR-005 §「提案的统一形状」：`claim / evidence / confidence / reason / decided_by / reviewed_by` |
| **指纹缓存** | `sha256(输入 + prompt_sha + model + agent_version)`，命中不调模型 |
| **预算闸** | `--budget-tokens`，超限即停并报告 |
| **降级标注** | 规则兜底时 `source = "rule"`，⛔ 不冒充 `ai` |
| **reviewed_by** | 初始 `null`；只有评审后能变 |

## 范围

### 做

1. **`ontok name suggest`**：把 REQ-090 的能力接进主流程；输出**带理由与置信度**的提案（不是只有名字）
2. **`ontok exclude suggest`**：AI 逐表判「该不该进本体」，输出 `exclusions` 候选（`table / category / reason / confidence / evidence`），**格式与 REQ-113 的 `exclusions.json` 兼容**，可直接走导入闸
3. **`ontok concept suggest --ai`**：AI 语义分组；与现有算法版并存，`--ai` 显式选；算法版保留为**降级路径**（标 `source=rule`）
4. **提案的可评审性**：三份提案都能被 `review` 流程消费（或至少能被人工 diff + 签字），签过的才有 `reviewed_by`
5. **溯源**：本体产物里记录「名字/排除/分组分别用的是哪份提案、谁签的」

### 不做

- ⛔ 不改 ADR-005 里「不是决策点」的三类（计算 / 规则 / 机械映射）
- ⛔ 不让 AI 提案自动生效（P2：未签不得进快照）
- ⛔ 不引新依赖
- ⛔ 不改 `review` 的双轨语义

## 关键裁定

| # | 问题 | 裁定 |
| --- | --- | --- |
| R1 | 三个命令的优先级 | **名称最先**（最影响客户观感），然后排除，最后分组 |
| R2 | 与既有算法版的关系 | 算法版**保留**为降级路径，但 `source=rule`；⛔ 不许悄悄替换 |
| R3 | 提案文件放哪 | 与输入同目录、显式 `--out`；⛔ 不写库（除缓存表） |
| R4 | 置信度低怎么办 | 低于阈值（如 0.6）的提案**照样输出**，但标 `low_confidence: true`，评审界面优先显示 |
| R5 | 成本 | 每个命令都必须支持 `--budget-tokens`；默认缓存 |

## 验证（真实数据）

同一份 `anspire_open_uat`（96 表）：

| 命令 | 期望 |
| --- | --- |
| `name suggest` | 84 张表都有中文名 + 理由；与现有 `table_names.json` 的一致性（差异逐条列出，不要求相同） |
| `exclude suggest` | 输出与 REQ-113 的 12 条对比：一致 / 新增 / 缺失，逐条说明 |
| `concept suggest --ai` | 输出与 REQ-115 的 8 组对比 |

> ⚠️ **不要求与对话里的结果相同**——要求的是**产品自己能产出、且能评审**。差异本身就是有价值的信息（说明对话里的判断带了额外上下文）。

## 交付物

1. 三个 `suggest` 命令 + 共用提案框架
2. 缓存/预算/降级标注
3. 真实库实测三份提案
4. 与对话版产物的逐条对比
5. 已知限制
