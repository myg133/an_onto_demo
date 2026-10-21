# 需求：REQ-113 · 排除表：从「脚本预处理」变成产品能力

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-113 |
| 优先级 | **P0**（影响可复现性与可审计性：同一份源库，两个人可能得到不同表数） |
| 状态 | 进行中 |
| 前置 | REQ-112 合并后开工（两者都动 `review_ui`） |
| 触发 | 2026-10-21 用真实库构建本体时，**BA 用 `.local/_filt.py` 手工过滤掉 13 张噪声表** |
| 最终清单 | **AI 逐张判断 96 张 → 13 条 → BA 复核推翻 1 条（`dim_customer_binding`）→ 12 条**（`96 − 12 = 84`）；下文「13 张」指事发当时的人工判断 |

## 需求描述

### 事故：排除是我用脚本做的，产品不知道

```
source DB（96 表）
   └─ ontok-introspect ──▶ schema.json（96 表）
          └─ 【BA 的 python 脚本过滤】 ──▶ schema.filtered.json（83 表）
                 └─ ontok-propose ──▶ 提案（83 实体）
```

**三个后果**：

| # | 后果 | 证据 |
| --- | --- | --- |
| 1 | **不可复现** | 脚本在 `.local/`，不在仓库、不被跟踪。别人直接跑 `ontok-propose` 得到 **96 表**，不是我说的 83 |
| 2 | **不可审计** | `stats.json` 里没有「排除了什么、为什么」；`extracted_by` 只记规则 agent |
| 3 | **数字无出处** | 我报「83 表 / 947 列」，产品自己跑出来「96 表 / 1116 列」，**差异没有任何记录** |

### 产品现在的替代路径也不对

不过滤直接跑：13 张噪声表会**被建成实体**（记进 `tables_without_cn_name`），直到**客户视图**那一步才被过滤。
⇒ **噪声进了本体，只是被挡在出口**。建模视图、提案、TTL 里都有它们。

### 目标：排除是**数据**，由产品执行

```
schema（96 表，产品勘查）
   └─ exclusions.json（被跟踪、带理由、可评审）
        └─ ontok-propose --exclude exclusions.json
             ├─ 排除 13 张，不建实体
             ├─ stats 记录：96 → 排除 13（逐条理由）→ 建本体 83
             └─ 审核界面能看到「哪些表被排除、为什么」
```

## 范围

### 做

1. **`exclusions.json` 格式**（与概念声明同风格：`version` / `rule` / `declared_by` / 条目带 `reason`）
2. **CLI**：`ontok-propose --exclude <file>`
3. **`stats.json` 记录**：`tables_seen` / `tables_excluded`（逐条：表名、类别、理由、决策人）/ `tables_extracted`
4. **溯源**：提案元信息记 `inputs.exclusions = {file, version, sha256}`
5. **审核界面**：新增「被排除的表」只读区块（表名 + 理由 + 决策人），评审人可据此推翻
6. **陈旧排除检测**：exclusions 里的表在 schema 中不存在 ⇒ 记 `stale_exclusions` + 警告；`--strict-exclusions` 时 **exit≠0**
7. **校验**：`reason` 为空 / `category` 非法 ⇒ **拒绝该文件**（exit≠0）

### 不做

- ⛔ 不改抽取/评分逻辑
- ⛔ 不做「按正则批量排除」（避免再次变成不可读的规则）
- ⛔ 不让排除影响已冻结版本（排除只作用于新一轮抽取）
- ⛔ 不做「排除列」（本轮只到表级）

## 关键裁定

| # | 问题 | 裁定 |
| --- | --- | --- |
| R1 | 排除的粒度 | **表级**；列级的「属性剔除」已由 `property_filter` 负责，两件事不许混 |
| R2 | 排除是否要理由 | **必须**。空理由 ⇒ 拒绝文件。理由要能被业务读懂（不是「技术原因」四个字） |
| R3 | 类别枚举 | `cdc_residue` / `migration_bookkeeping` / `backup_snapshot` / `internal_pipeline` / `report_intermediate` / `deprecated` / `out_of_scope` —— 新增类别须改枚举并升版本 |
| R4 | 陈旧排除 | 默认**警告 + 计数**；`--strict-exclusions` ⇒ 失败。理由：库结构会变，硬失败会让老文件一夜之间不能用 |
| R5 | 数字口径 | `stats.json` 必须同时有 **seen / excluded / extracted** 三个数，且 `seen - excluded == extracted` 必须成立（结构性断言） |
| R6 | 与 `tables_without_cn_name` 的关系 | 排除是**主动决定**（有理由、可评审）；无中文名是**被动结果**（缺翻译表）。两者分开记，⛔ 不许合并成一个数 |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 排除变成「悄悄丢数据」 | 三条：stats 逐条记录、审核界面可见、`seen - excluded == extracted` 断言 |
| 2 | 业务不认同某些排除 | 审核界面能看到理由并推翻；改文件重跑 |
| 3 | 排除文件长期漂移 | 陈旧检测 + `--strict-exclusions` |
| 4 | 与「无中文名」混淆 | R6 分开记 |

## 交付物

1. `exclusions.json` 格式 + 一份**真实库的 13 条排除**（含理由，业务可读）
2. `--exclude` / `--strict-exclusions`
3. `stats.json` 的三个计数 + 逐条排除记录
4. 审核界面的只读区块
5. 真实库实测：96 → 83，逐条理由可见
6. 已知限制
