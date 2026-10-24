---
name: ontok-query
description: 用 ontok 本体服务回答业务问题（REST + MCP）。当用户问「我有多少钱 / 用了多少 / 有哪些订阅 / 哪个模型用得最多」这类**来自业务数据库**的问题时使用。含 v1 语法边界、租户语义、警告转达规则与五个可直接跑的模板。
category: scenario
---

# ontok-query · 用本体服务回答业务问题

你面对的不是一张表，而是一个**本体服务**：它把关系库翻译成类/属性/关系，并**在服务端强制注入租户过滤**。

⛔ 你**不需要**、也**不能**在查询里写「哪个客户」——租户由你的访问口令绑定。

---


## 〇 · 连接信息**由部署注入**（本文件不含地址与口令）

⛔ **这份 skill 里没有、也不许有** `base_url` 和 `token`。

| 为什么 | 说明 |
| --- | --- |
| skill 是**共享资产** | 会打包、分发、被复制到多个 Agent 目录 |
| 地址**每套部署一个** | UAT / PROD / 客户私有环境各不相同 |
| token 写进来 = **泄漏** | 进 git、进分发包、进日志截图 |

### 运行时从哪读（按优先级）

| # | 来源 | 变量/字段 |
| --- | --- | --- |
| 1 | **MCP server 配置**（推荐） | 平台侧 `mcpServers` 里的 `url` + `headers` |
| 2 | 环境变量 | `ONTOK_BASE_URL`（如 `https://an-onto.example.com`）、`ONTOK_TOKEN` |

平台用 MCP 时，配置长这样（**token 由平台注入，不落在 skill 里**）：

```json
{
  "mcpServers": {
    "ontok": {
      "type": "sse",
      "url": "${ONTOK_BASE_URL}/mcp",
      "headers": [{ "name": "Authorization", "value": "Bearer ${ONTOK_TOKEN}" }]
    }
  }
}
```

### 拿不到连接信息时怎么办

⛔ **不要猜地址**（不要试 `localhost`，不要试上一套环境的 URL）。
✅ 明确说：「当前运行环境没有注入 `ONTOK_BASE_URL` / `ONTOK_TOKEN`，无法访问本体服务，请联系部署方。」

> 猜一个地址最坏的结果不是报错，而是**连到别的环境**——那会把别人的数据当成本租户的答案。

---
## 一 · 两个入口，选一个

| 入口 | 地址 | 适用 |
| --- | --- | --- |
| **REST** | `POST {BASE}/api/sparql` | 手写查询、脚本 |
| **MCP** | `POST {BASE}/mcp`（JSON-RPC 2.0） | 已接入 MCP 的 Agent |

鉴权（两个入口一样）：`Authorization: Bearer <QUERY_TOKEN>`

四个端点：

| 方法 | 路径 | 作用 |
| --- | --- | --- |
| GET | `/api/catalog?page=1&page_size=50` | **先看这个**：有哪些类/属性/关系 |
| GET | `/api/describe?name=:类名` | 类的细节：属性、枚举值、覆盖范围 |
| POST | `/api/sparql` | 执行查询 |
| GET | `/api/health` | 版本（免 token） |

MCP 工具对等：`catalog` / `describe` / `sparql_query` / `sample`。

---

## 二 · 标准动作顺序（照做，别跳）

```
1. catalog          → 找到你要的类（如 deductions / balances / usage_details）
2. describe <类>    → 看它有哪些属性、枚举值、coverage
3. sparql_query     → 写查询
4. 读回执           → warnings / coverage / truncated 必须转达给用户
```

⛔ **不先看 catalog 就写属性名** = 大概率 400（未知属性）。

---

## 三 · v1 语法边界（**实测结论，不是文档**）

### 支持

- 基本图模式（BGP）：1–3 跳三元组
- `FILTER`：`= != < > <= >=`、`&& || !`、括号
- `LIMIT` / `OFFSET`、`ORDER BY` **单列**
- 聚合：`COUNT / SUM / MIN / MAX / AVG` + `GROUP BY` **单列**
- `SELECT DISTINCT`
- 实例 IRI 常量、正则字符串比较

### 不支持（会 400，别浪费轮次）

| 写法 | 报错 | 替代做法 |
| --- | --- | --- |
| `"2026-10-01"^^xsd:dateTime` | `SPARQL_UNSUPPORTED` | **写普通字符串** `"2026-10-01T00:00:00"`，用 `<` `>=` 比 |
| `GROUP BY ?a ?b`（多列） | 只支持单列 | 分成两次查询，或把一列并进 `
CONCAT` 不需要——用单列分组的多次结果 |
| `SELECT ?id (SUM(?x) AS ?s)`（非聚合列） | 400 | 投影里只能有分组列和聚合值 |
| 未在三元组绑定的变量 | 400 | 每个 SELECT 变量都要在图模式里出现 |
| `OPTIONAL` / `UNION` / 子查询 / `BIND` / 属性路径 `*` `/` `^` | 各自点名的 400 | 改写成多跳 BGP；做不到就如实说「不支持」 |
| `REGEX` / `STR` / `YEAR` 等函数 | 400 | 用字符串比较代替（日期用 ISO 字符串前缀比较） |
| `CONSTRUCT` / `ASK` / `DESCRIBE` | 400 | 只支持 `SELECT` |

### 时间过滤怎么写（最容易踩）

```sparql
# ✅ 正确
FILTER(?ts >= "2026-10-01T00:00:00" && ?ts < "2026-11-01T00:00:00")

# ❌ 会被拒
FILTER(?ts >= "2026-10-01"^^xsd:dateTime)
```

---

## 四 · 五个可复制模板

> `:类名__属性名` 是本体里的属性 IRI 本地名。实际名字以 `catalog` 返回为准。

### Q1 按币种看扣费

```sparql
SELECT ?currency (SUM(?amount) AS ?total)
WHERE {
  ?d :deductions__currency ?currency .
  ?d :deductions__deduction_amount ?amount .
}
GROUP BY ?currency
```

### Q2 余额（⚠️ 口径不唯一，见下节）

```sparql
SELECT ?recharge ?frozen ?transfer
WHERE {
  ?b :balances__recharge_balance ?recharge .
  ?b :balances__frozen_balance ?frozen .
  ?b :balances__transfer_balance ?transfer .
}
```

### Q3 哪个模型用得最多

```sparql
SELECT ?model (SUM(?amount) AS ?used)
WHERE {
  ?u :usage_details__model_name ?model .
  ?u :usage_details__usage_amount ?amount .
}
GROUP BY ?model
```

### Q4 我的订阅与套餐

```sparql
SELECT ?subscription ?package
WHERE {
  ?subscription :customer_subscriptions__package_templates ?package .
}
```

### Q5 用量明细（带时间过滤）

```sparql
SELECT ?ts ?amount
WHERE {
  ?u :usage_details__usage_at ?ts .
  ?u :usage_details__usage_amount ?amount .
  FILTER(?ts >= "2026-10-01T00:00:00")
}
ORDER BY ?ts
LIMIT 50
```

---

## 五 · 回执怎么读（**这一段决定你会不会误导用户**）

回执字段：

```json
{
  "columns": [...], "rows": [...], "row_count": 3, "truncated": false,
  "warnings": ["表 deductions：…覆盖范围 = main_only —— 子账号产生的扣费不在结果内…"],
  "coverage": [{"table":"deductions","coverage":"main_only"}],
  "join_states": [{"from":"deductions","to":"customers","state":"false","used":false}],
  "limits": {"max_rows":200,"timeout_ms":10000},
  "sql_echo": "SELECT ... WHERE customer_id = '…' ..."
}
```

### 必须转达的四件

| 字段 | 含义 | 你必须说的话（示例） |
| --- | --- | --- |
| `warnings` 非空 | **可能漏数据** | 「这个数字只覆盖主账号，子账号的消费不在里面，可能偏低」 |
| `coverage` 含 `main_only` | 同上 | 同上 |
| `truncated: true` | 被截断 | 「只显示前 N 行，还有更多没显示」 |
| `join_states[].used=false` | 涉及但**本次没用到**的边 | 一般不用提；但如果用户问「为什么没有 X」，这里能解释 |

⛔ **`row_count: 0` 不等于「没有业务」。** 先看 `warnings`：
- 有 `main_only` ⇒ 「可见范围里没有」
- 没有数据 ≠ 没花钱；也可能是**数据没更新到当期**（服务不暴露数据截止时间）

---

## 六 · 遇到错误怎么办

| code | 你该做的 |
| --- | --- |
| `UNKNOWN_PROPERTY` | 错误里有 `hint`（列出可用属性）——照它改，别猜 |
| `UNKNOWN_CLASS` | 同样看 `hint` |
| `SPARQL_UNSUPPORTED` | 看上一节的「不支持」表换写法 |
| `TENANT_UNRESOLVED` | 说明这张表没在声明里登记——**不是你能修的**，如实告诉用户 |
| `TENANT_MISMATCH` | 你试图指定客户——删掉租户条件 |
| `TIMEOUT` / `ROW_LIMIT` | 缩小范围或分页 |

---

## 七 · 「余额」为什么没有唯一答案（真实案例）

`balances` 表有 **20+ 金额列、3 种币种**，**没有 `total_balance`**。所以：

```
充值余额 + 赠送额度 − 冻结金额 = ?
```

这个公式**只在文档里**（如果客户给了文档），本体里没有。

**正确做法**：查清各分项，**把口径列出来**让用户确认：

> 「余额有几种口径：① 充值余额 RMB 20.00；② 免费额度 2500（已用 36，剩 2464）；③ 划拨/信用/冻结均为 0。您要看哪一种？」

**错误做法**：自己挑一列加加减减，报一个数。

---

## 八 · 禁忌

1. ⛔ 不要写租户过滤条件（服务端已注入，写了会被忽略或在期限内报错）
2. ⛔ 不要把 `0 行` 说成「没有消费/没有业务」
3. ⛔ 不要在没看 `catalog` 的情况下猜属性名
4. ⛔ 不要把 `join_states` 里的边当成「本次用到的边」——看 `used` 字段
5. ⛔ 不要向用户展示 `sql_echo`（内部 SQL 不外露）
