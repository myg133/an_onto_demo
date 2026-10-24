# 验收标准：REQ-139

## 一 · AC

### AC-001 · 两个字段都在（本体模式）

- `--ontology-only` 起 ops360 ⇒ `GET /api/health` 含 `"project": "ops360"`、`"ontology_version": 2`
- 判据：响应原文

### AC-002 · 普通模式也有（R3）

- anspire 项目 ⇒ `project` / `ontology_version` 都正确
- 判据：响应原文

### AC-003 · 与 catalog 一致（R4）

- `/api/health` 的 `project`/`ontology_version` == `/api/catalog` 的 `project`/`version`
- 判据：两处数值对比（不是"看起来一样"）

### AC-004 · 现有字段一个不少（R5）

- `service` / `version` / `syntax` / `protocols` / `transports` 原样；`mode` 仍只在本体模式出现
- 判据：与基线响应做键集合对比

### AC-005 · 不泄数据（R2）

- health 里 grep：租户 uuid、连接串、表名（`documents`/`customers` 等）、`password` ⇒ **0 命中**
- 判据：grep 输出

### AC-006 · 免 token

- 不带 Authorization ⇒ 200
- 判据：一次

### AC-007 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言 |
| 基线 | 同机 `develop`；`/metrics` 与 catalog/TTL/SPARQL 的字节不变 |

## 二 · 纪律

- **G** 变异：把 `ontology_version` 恒写 `0` ⇒ AC-003 必须红（说明那条断言不是空转）
- **S** AC 也会错，发现直接说


---

## 三 · 裁定：与 REQ-136 AC-006 的**表面冲突**（Dev 提问，我裁定）

Dev 指出：**普通模式 health 字节变了**（124 → 176 B），而 REQ-136 的 AC-006 写的是「普通模式零行为变化」。

### 裁定：**这是有意变更，不构成回归**

| 口径 | 指什么 | 本次 |
| --- | --- | --- |
| REQ-136 AC-006「零行为变化」 | **数据面**：catalog / TTL / SPARQL / MCP 的响应字节 | ✅ 逐字节相同（catalog 31671 B、TTL 293814/371356 B、SPARQL 5 行） |
| REQ-139 R3「两种模式都要有」 | **身份面**：`/api/health` 的字段 | ✅ 有意增加 |

**理由**：`health` 是**身份端点**，不是数据端点。它的职责就是回答"我是谁、能干什么"。
把新字段藏起来（只在某一模式给）会让 Agent 在两套部署上拿到不同形状的身份信息——**那才是真的不一致**。

⇒ **REQ-136 AC-006 的适用范围**：**数据面**逐字节不变；**身份面**允许按需求演进，但要**只增不减**（R5：旧字段一个不少）。

### 附带确认

| 项 | 证据 |
| --- | --- |
| 旧字段一个不少 | 键集合对比：`added=[ontology_version,project] removed=[]` |
| `mode` 仍只在本体模式 | 普通模式 grep 不到 `"mode"` |
| 与 catalog 同值 | 两模式 `SAME=True`（`ops360`/2、`anspire_r2rml_excl`/2） |
| 零泄漏 | uuid / 连接串 / password / tenant / 表名 ⇒ 全 0 |
| 变异有牙 | `ontology_version` 恒 `0` ⇒ AC-003 红（`left: 0 / right: 2`） |
