# 两平台交互契约（本体平台 × 数字员工）

> 状态：**草案 v1，待双方确认**。2026-10-25。
> 参与方：**本体平台**（我们）· **数字员工**（Axon）· **真实系统**（库 / 云 API）

---

## 一 · 三方分工（一句话各自）

| 平台 | 负责 | 不负责 |
| --- | --- | --- |
| **本体平台** | 说清「有什么、怎么关联」；**能算的算完**；**算不了的显式标出来并给取数指引** | 调 API、拼最终答案、执行动作 |
| **数字员工** | 理解意图、编排查询、**按指引取外部数据**、拼答案、决策、执行 | 定义语义、改本体 |
| **真实系统** | 事实源（库 / 云） | —— |

---

## 二 · 一次完整交互（时序）

```
Agent                       本体平台                     云 API
  │                            │                          │
  │ ① GET /api/ontology.ttl    │                          │
  │───────────────────────────>│                          │
  │  TTL（类/属性/关系 + 哪些类来自 api）                    │
  │<───────────────────────────│                          │
  │                            │                          │
  │ ② 按用户问题写 SPARQL        │                          │
  │ ③ POST /api/sparql         │                          │
  │───────────────────────────>│                          │
  │                            │ 只查关系库                │
  │  ④ JSON-LD + @pending      │                          │
  │<───────────────────────────│                          │
  │                            │                          │
  │ ⑤ 看到 complete:false       │                          │
  │    按 @pending[].get 取数    │                          │
  │──────────────────────────────────────────────────────>│
  │  数据                       │                          │
  │<──────────────────────────────────────────────────────│
  │                            │                          │
  │ ⑥ 拼成最终答案（标出哪部分来自 API）                      │
  │                            │                          │
```

**关键**：第 ④ 步不是"失败"，是"**半成品 + 说明书**"。

---

## 三 · 三个产物的形状

### 3.1 TTL：标出「这个类来自 API」

```turtle
:cvm_instance a owl:Class ;
    rdfs:label "CVM 实例"@zh ;
    ontok:sourceKind "api" .          # ← 只在非库源时出现

:bill a owl:Class ;
    rdfs:label "账单"@zh .             # ← 库源，不写 sourceKind（默认 db）
```

| 规则 | 说明 |
| --- | --- |
| **默认 db** | 库源**不写** `sourceKind`（客户视图保持干净） |
| **api 必须写** | 让 Agent **写查询前**就知道"这类查了也没数" |
| ⛔ 不写细节 | 不出现端点、参数名、凭据变量——那些只在 access 契约里 |

### 3.2 查询结果：`@pending` + `meta.complete`

```json
{
  "@context": { "ontok": "https://…/ops360#" },
  "@graph": [
    { "@id": "ontok:order/1001", "@type": "ontok:order",
      "ontok:order__amount": 199.00,
      "ontok:order__customer": { "@id": "ontok:customer/7d49…" } }
  ],
  "@pending": [
    { "@id": "ontok:customer/7d49…",
      "@type": "ontok:customer",
      "ontok:sourceKind": "api",
      "reason": "source_is_api（平台不执行 API 源）",
      "_note": "本节示例是【阶段 2 目标形态】。本期（REQ-145 裁定 R-A）跨源边不下发 get，改为 ref + local_hint；见 REQ-145 acceptance §三",
      "get": {
        "class": "customer",
        "filter": { "ids": ["7d49…"] },
        "pushdown": ["ids"],
        "ref": "access#Customer@v2"        // 指向已签契约里的条目 + 版本
      } }
  ],
  "@context_note": "…",
  "meta": {
    "complete": false,                     // ⭐ 关键
    "unresolved_classes": ["customer"],
    "warnings": [], "coverage": [], "truncated": false,
    "mapping_version": 2
  }
}
```

### 3.3 access 契约：`ref` 指向的那条

```json
{ "contract_version": 1, "mapping_version": 2,
  "entities": [
    { "id": "Customer", "class": "customer", "reviewed_by": "张三@2026-10-25",
      "source": "tencent-crm",
      "operation": { "action": "DescribeCustomers" },
      "iterator": "$.Response.CustomerSet[*]",
      "iri_template": "ontok:customer/{CustomerId}",
      "fields": { "nickname": { "path": "Name" } },
      "capability": { "pushdown": ["ids"], "local_only": [], "max_scan_rows": 5000 } } ] }
```

`@pending[].get.ref` = `access#<entity.id>@v<mapping_version>` —— **可追溯到版本**。

---

## 四 · 七条硬规则

| # | 规则 | 为什么 |
| --- | --- | --- |
| **1** | **`meta.complete` 必须显式**；有 `@pending` ⇒ 必为 `false` | ⛔ 不标就是半个答案 |
| **2** | **`@pending` 不进 `@graph`** | `@graph` 只放有事实的节点；空洞塞进去会污染图 |
| **3** | `get.ref` 必带 `mapping_version` | 映射是人签的，要能追到版本 |
| **4** | **跨源边不下发 `get`** | 给了能力却查不了，比不给更糟 |
| **5** | ⛔ **不回填**（Agent 取到的数据不写回平台） | 回填会让我们变成状态存储，与"本体无状态"冲突 |
| **6** | 只有**内部调用者**能看到 `get` | 客户视图/门户不能拿到"怎么直连我们数据源" |
| **7** | 类**完全**在 API 源时 ⇒ `@graph: []` + `@pending` 一条（不是报错） | Agent 一条链走完，不用试错 |

> 规则 7 的具体形态：
> ```json
> { "@graph": [], "meta": { "complete": false },
>   "@pending": [{ "class": "cvm_instance", "get": { "class": "cvm_instance", "filter": {} } }] }
> ```

---

## 五 · 「快照 + 现场」在同一份结果里并存

这是这套契约最实用的地方：

| 数据 | 来源 | 平台怎么处理 |
| --- | --- | --- |
| 账单、巡检快照、风险项 | **我们落的库**（`sourceKind` 默认 db） | **直接返回** |
| CVM 实例状态、实时告警 | **云 API**（`sourceKind: "api"`） | **留 `@pending` + 给 `get`** |

⇒ **一次查询，两种数据，结构上一眼可分**：

```json
{ "@graph": [ …账单… ],            // 快照：已有
  "@pending": [ …实例… ],          // 现场：待取
  "meta": { "complete": false } }
```

**Agent 补完 `@pending` 后，答案就是"快照 + 现场"的合并。**

---

## 六 · 错误码约定（与现有对齐）

| code | 含义 | Agent 动作 |
| --- | --- | --- |
| `NO_DATA_SOURCE` | 整套部署无数据源（本体模式） | 别重试；只能答本体问题 |
| `NOT_DB_SOURCE` | 查询打了 API 源的类，且平台没加载映射 | 改查 `catalog` 看 `access.kind` |
| `CROSS_SOURCE_UNSUPPORTED` | 查询需要跨源遍历 | **拆成两段** |
| `SPARQL_UNSUPPORTED` | 语法超出 v1 | 按 hint 改 |
| `SPARQL_UNDEFINED_NAME` | 类/属性名不存在 | 先查 `catalog` |

---

## 七 · 边界（写死，避免以后争）

| 不做 | 归属 |
| --- | --- |
| 跨源 join | ⛔ 谁都不做；Agent 分段拼 |
| 联邦执行器 | ⛔ 不在本产品范围 |
| 平台调云 API | ⛔ 凭据不出客户边界 |
| 数据回填 | ⛔ 平台保持无状态 |
| 最终答案拼装 | ✅ 数字员工 |

---

## 八 · 对两边的交付要求

### 本体平台要做的

| # | 内容 |
| --- | --- |
| 1 | TTL 输出 `ontok:sourceKind`（仅 api 类） |
| 2 | 查询结果输出 `@pending` + `meta.complete` + `meta.mapping_version` |
| 3 | `query serve --access-mapping` 加载契约；`catalog`/`describe` 报 `access.kind` |
| 4 | `CROSS_SOURCE_UNSUPPORTED` / `NOT_DB_SOURCE` 两个错误码 |

### 数字员工要做的

| # | 内容 |
| --- | --- |
| 1 | 读 TTL 时**记住** `sourceKind: api` 的类 |
| 2 | 收到 `complete: false` ⇒ **必须**处理 `@pending`，不许直接作答 |
| 3 | 按 `get` 调 API（凭据自己管），处理分页/限流 |
| 4 | 最终答案里标出**哪部分来自 API** |
| 5 | 遇到 `CROSS_SOURCE_UNSUPPORTED` ⇒ 拆两段查，别重试 |

---

## 九 · 待双方确认的三点

| # | 问题 | 我的建议 |
| --- | --- | --- |
| 1 | `@pending` 是顶层数组，还是放在 `meta` 里？ | **顶层**（它和 `@graph` 是并列的"两种东西"） |
| 2 | `get.filter` 里的参数，平台填好还是 Agent 自己推？ | **平台填**（它知道关系两端的 ID）；类级能力放 `catalog` |
| 3 | `mapping_version` 与 `ontology_version` 要不要分开？ | **分开**（映射改得比本体勤） |
