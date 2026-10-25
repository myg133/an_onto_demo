# 验收标准：REQ-145

## 一 · AC

### AC-001 · TTL 标 `sourceKind`（仅 api 类）

- 给了 `--access-mapping`（含 1 个 api 类）⇒ TTL 里该 api 类有 `ontok:sourceKind "api"`
- 库源类**没有**该谓词
- 客户视图与建模视图**都**有
- ⛔ TTL 里不出现端点 / 参数名 / 凭据变量名
- 判据：贴两段 TTL + grep 结果

### AC-002 · 不给映射 ⇒ 零变化（R1）

- 不带 `--access-mapping` ⇒ TTL / catalog / describe / SPARQL（rows 与 jsonld）与基线 `b4a8b56` **逐字节相同**
- 判据：sha 清单（至少 TTL 两视图 + catalog + 一条 SPARQL 两模式）

### AC-003 · catalog / describe 报 `access`

- 给了映射 ⇒ `describe` 某个 api 类时含：
  `access: { kind: "api", queryable_by: ["fetch"], pushdown_fields: [...], freshness: ... }`
- 库源类含 `access: { kind: "db", queryable_by: ["sparql"] }`
- 判据：各一条

### AC-004 · JSON-LD 模式：`@pending` + `complete:false`

- 查询只涉及 api 类 ⇒ `@graph: []`、`@pending` 一条、`meta.complete: false`、`meta.unresolved_classes` 含该类
- `@pending[].get` 含 `class` / `filter` / `pushdown` / `ref`（`ref` 带 `@v<version>`）
- 判据：输出原文

### AC-005 · rows 模式：明确报错（R5）

- 同一条查询用 `format=rows` ⇒ **报错** `NOT_DB_SOURCE` + hint
- ⛔ 不许返回 `rows: []`
- 判据：状态码 + 原文

### AC-006 · 跨源边不下发 get（R4）

- 构造：库源类 `order` 与 api 类 `customer` 之间有边
- ⇒ 该边的 `@pending` 里**没有** `get`，且 `reason` 说明跨源
- 遍历该边 ⇒ `CROSS_SOURCE_UNSUPPORTED`
- 判据：两处

### AC-007 · `complete` 与 `@pending` 一致（R2，**不变式**）

- 对所有输出：`complete == @pending.is_empty()`
- **变异**：让 `complete` 恒 `true` ⇒ 必须有测试红
- 判据：3 组数据（有 pending / 无 pending / 空结果）+ 变异红行

### AC-008 · `meta.mapping_version`

- 有映射时 `meta.mapping_version` 出现且与传参一致；无映射时不出现（保持零变化）
- 判据：各一次

### AC-009 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言 |
| 出口 | 若新增响应构造点，登记 `OUTLETS` |
| 基线 | 同机 `develop` 实测（Dev 自报） |

## 二 · 纪律

- **G** 变异：①`complete` 恒 true（AC-007 红）②把 `sourceKind` 也写给库源（AC-001 红）③rows 模式返回空表（AC-005 红）
- **S** AC 也会错，发现直接说


---

## 三 · 裁定（Dev 指出的 AC-006 与契约冲突）

### R-A · 冲突是真的，而且是我自己两份文档打架

| 文档 | 说法 |
| --- | --- |
| 交互契约 §3.2 示例 | `order → customer` 的 pending **带 `get{filter:{ids:[…]}}`** |
| 本需求 AC-006 | 跨源边的 pending **没有 `get`** |

**裁定：AC-006 为准，契约的示例降级为"阶段 2 目标形态"。**

| 要给"带 ids 的 get"需要 | 现实 |
| --- | --- |
| 从库侧读 `order.customer_id` | 技术上可行 |
| 但当前规划器**不走那条 FK 就生成不了 SQL** | 要加"只读 child 列"的特殊路径 ⇒ 阶段 2 |

**但跨源 pending 必须补两样**（否则 agent 没法用）：

```json
{ "@type": "ontok:customers", "reason": "cross_source_edge",
  "ref": "access#Customer@v2",
  "local_hint": { "uses_column": "customer_id" } }
```

- `ref`：没有它，agent 拿不到映射
- `local_hint.uses_column`：**库侧哪一列有 FK 值**——不给就只能猜

### R-B · 审计 `code` 必须等于客户端收到的（必修）

Dev 写 `code=API_PENDING`，客户端拿到 `NOT_DB_SOURCE`。

⇒ **审计与响应不一致**，看审计的人会以为客户端收到了 `API_PENDING`。
要改：`code` 记实际响应码；"这是 API 源"用独立字段表达，⛔ 不用 `code` 表达两件事。

### R-C · 混合查询整体 pending、不跑 SQL（本期接受）

与契约「能算的算完」有张力。

**接受**（部分求值要拆查询计划，属阶段 2），但要求：
1. `complete:false` + `@pending` 都在
2. 写进 `--help` 与已知限制
3. ⛔ 不许在 `@graph` 里塞半份结果

### R-D · 两条我认可的

| 项 | 判断 |
| --- | --- |
| 点名 api 属性 ⇒ 400 `CROSS_SOURCE_UNSUPPORTED` | ✅ 对 |
| `sample` 打 api 类 ⇒ `NOT_DB_SOURCE` | ✅ 与 R5 同口径 |
