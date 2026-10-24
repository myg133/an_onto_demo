# 验收标准：REQ-140

## 一 · AC

### AC-001 · 三个入口都能取到 JSON-LD

- REST：`Accept: application/ld+json` ⇒ 返回 JSON-LD
- REST：`?format=jsonld` ⇒ 同上
- MCP：`sparql_query` 带 `format: "jsonld"` ⇒ `structuredContent` 是图
- 判据：三处各一次，形状一致

### AC-002 · 形状完整

- 顶层含：`@context`（`ontok` 前缀 + `xsd`）、`@graph`（数组）、`meta`
- 每个节点含：`@id`、`@type`、至少一个属性
- 判据：贴一份真实输出

### AC-003 · 同实体合并（**这是"图"与"表"的分水岭**）

- 构造一个查询：同一实体在**多行**出现（如 `?o :order__customer ?c. ?c :customer__name ?n`，两条订单同一个客户）
- ⇒ `@graph` 里该 `@id` **只出现一次**，属性合并
- 判据：节点数 < 行数；贴节点原文

### AC-004 · 节点 vs 字面量

- 类变量 ⇒ `{"@id": "…"}` 形式（或字符串 IRI + `@type: @id`）
- 普通值 ⇒ 字面量（数字/字符串/布尔）
- 判据：三种各一条（IRI / 数字 / 字符串）

### AC-005 · `meta` 必须带上限定条件（**不许吞警告**）

- 用一个**有覆盖警告**的查询（如查 `deductions`）
- ⇒ JSON-LD 的 `meta.warnings` / `meta.coverage` 与 `format=rows` 时**完全一致**
- 判据：两处对比

### AC-006 · 聚合进 `summary`

- `SELECT (COUNT(?o) AS ?n) WHERE { ?o a :order }`
- ⇒ `@graph: []`、`summary.n = 数字`
- ⛔ 不许在 `@graph` 里造假节点
- 判据：贴输出

### AC-007 · 确定性（**硬判据**）

- 同一查询两次请求 ⇒ 响应体 **sha256 相同**
- 且属性键顺序、节点顺序稳定
- 判据：两次 sha256 + 一次 `@graph` 逐字节对比

### AC-008 · 空结果

- 查一个不存在的实体 ⇒ `@graph: []`（数组），`summary` 可缺省
- 判据：贴输出（⛔ 不许 `null`、不许缺字段）

### AC-009 · 默认回执零变化

- 不带 `format` / 不设 Accept ⇒ 与基线 `600ae76` 的 rows 回执**逐字节相同**
- 判据：三条查询对比 sha256

### AC-010 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖（JSON-LD 手动拼 `serde_json` 即可）；删测试；放宽断言 |
| 出口 | 若新增响应构造点，登记进 `OUTLETS` |
| 基线 | 同机 `develop` 实测 |

### AC-011 · 单份输出（不双份）

- `format=jsonld` 时**不回 `rows`**（避免内存与带宽翻倍）
- 判据：响应体里没有 `rows` 字段（或为空）

## 二 · 纪律

- **G** 变异：①节点不排序（确定性必红）②同 `@id` 不合并（AC-003 必红）③`meta` 丢 warnings（AC-005 必红）
- **S** AC 也会错，发现直接说
