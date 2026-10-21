# 验收标准：REQ-113 · 排除表

> **核心承诺**：**任何一张表被排除，都必须有理由、有记录、能被评审人看见。**
> ⛔ 不许出现「数字对不上但没人知道为什么」。

## 一 · AC

### AC-001 · 排除由产品执行，不是预处理

- `ontok-propose --exclude exclusions.json` 产出的提案里，被排除的表**不产生任何实体/属性/关系**
- 判据：`proposal.json` 里 grep 被排除的表名 = **0**；实体数 = `tables_extracted`

### AC-002 · 三个计数必须闭合

- `stats.json` 同时有 `tables_seen` / `tables_excluded` / `tables_extracted`
- 判据：`seen - excluded == extracted`（**结构性断言**，不是人眼核对）
- 真实库：`96 - 12 == 84`（**AI 判 13、BA 复核推翻 1 条** ⇒ 12）

### AC-003 · 每条排除都有理由，且可读

- 每条含 `table` / `category` / `reason` / `decided_by`
- 判据：`reason` 为空 ⇒ **拒绝文件**（exit≠0）；`category` 不在枚举内 ⇒ 拒绝
- 判据：`decided_by` 缺失 ⇒ 拒绝（谁决定的必须可追）

### AC-004 · 溯源可查

- 提案元信息记 `inputs.exclusions = {file, version, sha256}`
- 判据：改一个字节的 exclusions ⇒ sha256 变；同一文件两次运行 ⇒ sha 相同

### AC-005 · 陈旧排除被检测

- exclusions 里的表在 schema 中不存在 ⇒ 进 `stale_exclusions[]` + stderr 警告
- `--strict-exclusions` ⇒ **exit≠0** 并点名哪几条陈旧
- 判据：两种模式各跑一次

### AC-006 · 审核界面可见（只读）

- `GET /api/review` 或独立端点返回被排除的表清单（表名 + 类别 + 理由 + 决策人）
- 页面显示「被排除 N 张」区块
- 判据：区块存在且条数 = `tables_excluded`；⛔ 不许可编辑（改排除要改文件重跑）

### AC-007 · 与「无中文名」分开记

- `tables_without_cn_name` 与 `tables_excluded` 是**两个不同字段**
- 判据：构造一张既被排除、又没中文名的表 ⇒ 只进 `excluded`，不进 `without_cn_name`（排除优先）

### AC-008 · 确定性

- 同一份 schema + 同一份 exclusions 跑两次 ⇒ 提案与 stats **逐字节相同**
- 判据：`git diff --no-index` 无输出

### AC-009 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `src/server/http.rs` |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 基线 | REQ-112 合并后的 lib/doc/套件数字，一条不许少 |

### AC-010 · 真实库实测

- 对 `anspire_open_uat` 跑：`96 → 排除 12 → 84`（清单来源：AI 判断 + BA 复核，`reviewed_by` 已签）
- 输出 13 条理由（业务可读，例如「CDC 同步残留表，由数据同步服务自动创建，不属于业务模型」）
- 判据：贴出完整的 13 条

## 二 · 纪律

- **H** 排除只能有一个实现（不许在 propose 与 review_ui 各算一遍）
- **G** 变异三元组；编译失败不算执行
- **S** AC 也会错，发现直接说
