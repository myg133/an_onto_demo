# 需求：REQ-143 · 纯文档通路（把"桥"做成产品命令）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-143 |
| 优先级 | **P0**（FDE 自助建本体的前提） |
| 状态 | 进行中 |
| 基线 | `develop` `f04b2ae` |
| 触发 | 用户要为 FDE 做一套"用文档生成本体"的 skill；而产品现在**不支持纯文档从零生成** |

## 问题（我实测过）

```
空提案 + 3 条断言 → doc merge
  ⇒ 并入 0 · 未映射 3 · doc-only 条目 0
```

`doc merge` 的设计是「**用文档补充已有库本体**」：

- 概念必须**精确命中**已有实体
- 关系必须有**显式 `maps_to`**（child 列 + parent 列）

⇒ **没有库的项目（如 Ops360）走不通**。当时我写了个 Python 脚本把抽取结果转成 `proposal.json` —— **那是权宜之计**。

**为什么必须做成产品命令**：skill 里如果写"运行某个脚本"，路径一变就废、版本会漂、FDE 无法维护。

## 要做

给 `doc merge` 加一个**显式开关**：

```bash
ontok doc merge --assertions a.jsonl --proposal p.json --out merged.json --allow-doc-only
```

### 该开关下（**其余规则一字不改**）

| 情形 | 行为 |
| --- | --- |
| 概念**命中**已有实体 | 照旧（`origin: both`，合并证据） |
| 概念**没命中** | **新建 `add_entity_type`**：`key` 由名字**确定性派生**、`display_name` = 概念名、`band` ≤ `medium`、`reviewed_by: null`、`origin: "doc"` |
| 关系两端**都能解析**（命中或新建） | 新建 `add_link_type`：`from_type`/`to_type` 用实体 key，**没有 join 列**（`column` 空） |
| 关系任一端解析不到 | 仍进 `doc_unmapped`（⛔ 不许凭空造端点） |
| 冲突 | 照旧双保留 + `applicable: false` |
| 不可对齐（`alignable: false`） | 照旧排除并计数 |

### 不开该开关

⛔ **行为必须与现在逐字节相同**（空提案 ⇒ 0 并入 / 全未映射）。

## 不做

- ⛔ 不改 `doc ingest`
- ⛔ 不改 `doc merge` 在没有该开关时的任何行为
- ⛔ 不做语义猜测（"套餐"→`package_templates` 仍然不猜）
- ⛔ 不引依赖

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | 开关**必须显式**（`--allow-doc-only`），默认关闭 |
| R2 | 新建实体的 `key` **必须确定性**（同一输入两次 ⇒ 同一 key，逐字节相同） |
| R3 | 新建条目 `band` **≤ medium**、`reviewed_by: null`（沿用 REQ-129 的纪律） |
| R4 | 关系**不许造端点**：任一端解析不到 ⇒ 仍进 `doc_unmapped` |
| R5 | 不开开关时行为**零字节变化**（回归判据） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 开关被滥用（有库的项目也开）⇒ 造出"文档类"实体混进真本体 | 新建条目 `origin: "doc"`，一眼可辨；`stats.merged_doc_only` 计数 |
| 2 | key 派生不确定 ⇒ 两次构建不幂等 | R2 + 逐字节对拍 |
| 3 | 文档抽出的概念有噪声（728 条断言 / 506 概念）⇒ 一次建 500 个类 | `stats` 报数；文档建议先人工筛（skill 里写明） |

## 交付物

1. `--allow-doc-only` + 上述规则
2. 不开开关时**零字节变化**的证据
3. 纯文档端到端：ingest → merge（开开关）→ import → 签 → apply → freeze → export TTL
4. 确定性（两次 sha 相同）
5. 已知限制
