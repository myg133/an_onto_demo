# 需求：REQ-122 · SPARQL 查询规划与租户注入

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-122 |
| 优先级 | **P0**（数字员工链路的第三块地基） |
| 状态 | 进行中 |
| 基线 | `develop` `e8f21c8` |
| 依据 | **ADR-006 · 租户口径**（用户拍板三项） |

## 需求描述

有 TTL、有 R2RML，但**没有把 SPARQL 变成 SQL 的东西**。

```
Agent 写:  SELECT ?金额 WHERE { ?d :deductions__customers ?c . ?d :deductions__deduction_amount ?金额 }
                ↓  本需求
服务端:    SELECT d.deduction_amount FROM deductions d
           WHERE d.customer_id = $tenant          ← 强制注入，Agent 看不见也改不了
```

## 范围（v1 刻意收窄）

### 支持（v1）

| 能力 | 说明 |
| --- | --- |
| **BGP**（基本图模式） | 1–3 跳的 triple pattern 连接 |
| `FILTER` 基本比较 | `= != < > <= >=`、`&&`、`\|\|`、数值/字符串/日期字面量 |
| `LIMIT` / `OFFSET` | 强制上限 |
| `ORDER BY` | 单列 |
| **聚合** | `COUNT` / `SUM` / `MIN` / `MAX` / `AVG` + `GROUP BY` 单列 |
| 前缀 + IRI | 复用 TTL 前缀 |

### 不支持（v1 明确拒绝，报可读错误）

- `OPTIONAL` / `UNION` / `MINUS` / 子查询 / `BIND` / 属性路径 `*` `+` `/`
- `CONSTRUCT` / `DESCRIBE` / `ASK`（只支持 `SELECT`）
- 正则 `REGEX`、函数调用（`STR` / `YEAR` / `IF` 等）

> ⛔ 不支持**必须报错**，不许「静默忽略某个子句」——那会让数字员工以为过滤生效了。

## 要做

1. **`ontok sparql query --project P --version V --tenant <id> --query '<sparql>' [--max-rows N]`**
   - 解析 SPARQL → 用 R2RML 映射展开成 SQL → **注入租户条件** → 执行 → 返回 JSON
2. **规划器**：BGP → 连接路径（沿 R2RML 的 joinCondition）
3. **租户注入**（ADR-006：**机制是产品的，口径是声明的**）：
   - 读声明文件 `query-policy.json`：`tenantSubject` / `tenantIdColumn` / `perTable[].tenantColumn` / `publicDimensions[]`
   - 按声明逐表决定：过滤列 / 公共维度不过滤 / **不可查**
   - **声明缺失或表未登记 ⇒ 整条查询拒绝**，点名哪张表 + 提示「请在声明里登记」
   - ⛔ **代码里不得出现 `customer_id` / `main_customer_id` 字面量**用于注入决策（换库只改声明）
4. **安全闸**：
   - `--max-rows`（默认 1000）
   - 超时（默认 10 s，可配）
   - **只允许映射里存在的类与属性**（Agent 写错属性名 ⇒ 报错，不许空结果）
   - 只读会话（复用 `sample.rs` 的做法）
5. **返回**：`{columns, rows, row_count, truncated, sql_echo, tenant_applied[], tables[], warnings[]}`
   - `sql_echo` 里**必须能看见注入的租户条件**（判据：不是「代码里有」，而是「SQL 里有」）

## 不做

- ⛔ **不在代码里硬编码租户列名**（由声明驱动）
- ⛔ 不做 HTTP/MCP 端点（REQ-123）
- ⛔ 不做子账号自动解析（ADR-006 Limitation）
- ⛔ 不做查询缓存（先跑通）
- ⛔ 不做写操作（只读）
- ⛔ 不引新依赖（SPARQL 解析手写子集；或用已有 crate——先查 `Cargo.toml`，若已有则复用）

## 关键裁定

| # | 问题 | 裁定 |
| --- | --- | --- |
| R1 | 不支持的语法 | **报错拒绝**，⛔ 不许静默忽略 |
| R2 | 属性名写错 | **报错**（列可用属性清单），⛔ 不许返回空结果 |
| R3 | 租户无法确定 | **整条拒绝**（fail-closed），点名表 |
| R4 | 注入位置 | SQL 生成后、执行前；**每个涉及的表都要有** |
| R5 | 注入可见性 | `sql_echo` 必须包含注入条件；判据用文本断言 |
| R6 | 行数上限 | 默认 1000，超出 ⇒ `truncated: true` + 明确告知 |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | SPARQL 子集解析手写出错 | 用「支持清单」驱动测试：每条支持语法一个用例，每条不支持语法一个拒绝用例 |
| 2 | 多跳查询在大表上爆炸 | 强制 `LIMIT` + 超时 + 只读 |
| 3 | 注入漏表（尤其是 join 的中间表） | 判据：`tables[]` 与 `sql_echo` 里的每张表都能在 `tenant_applied[]` 找到（或它在公共维度声明里） |
| 4 | 数字员工拿不到错误信息，反复试错 | 错误必须**可读 + 可行动**（如「属性 `:xxx` 不存在，可用列表见 …」） |

## 交付物

1. `ontok sparql query` + 解析器 + 规划器 + 租户注入
2. 支持/不支持语法的测试矩阵
3. 真实库实测：至少 5 条查询（含 2 跳、聚合、过滤）
4. **泄漏测试**：构造「A 客户查询能拿到 B 客户数据」的查询 ⇒ 必须查不到
5. `sql_echo` 样例（能看见注入条件）
6. 已知限制
