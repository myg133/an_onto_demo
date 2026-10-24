# 验收标准：REQ-136 · 本体服务模式

## 一 · AC

### AC-001 · 无源库也能起

- 环境**不设** `ONTOK_SOURCE_DATABASE_URL`、**不给** `--schema`，带 `--ontology-only` ⇒ 服务**成功启动**
- `/api/health` ⇒ 200，响应含 `mode: "ontology-only"`
- 判据：贴启动日志（要有 `mode=ontology-only（不接业务库）` 横幅）+ health 响应

### AC-002 · catalog 必须列出**关系**（本需求存在的理由）

- 用 **ops360 真实快照**（v2）⇒ `counts.classes=20`、**`counts.relations=10`**、**`counts.properties=47`**
- ⛔ 不许出现合成出来的 `id` 列（那是自动造 schema）
- ⛔ `relations` 不许是 0；⛔ 不许出现 `excluded_relations=10`
- 判据：catalog 响应原文（含 counts 与至少 2 条 relation 条目）

### AC-003 · describe 有名字

- `describe` 任一实体 ⇒ `display_name` **非空**、`key` 非空、`properties` 数量正确
- 判据：2 个实体各一次（一个中文名、一个带属性多的）

### AC-004 · SPARQL ⇒ 503 `NO_DATA_SOURCE`

- `POST /api/sparql` ⇒ **503**，code=`NO_DATA_SOURCE`
- hint 必须说明：**本部署只提供本体、未接业务库、重试无效**
- ⛔ 不许 200 空结果；⛔ 不许 500
- 判据：状态码 + 响应体原文

### AC-005 · TTL 与 CLI 逐字节相同

- `GET /api/ontology.ttl` ⇒ 与 `ontok ontology export --project ops360 --version 2` 逐字节相同（sha256 相等）
- `?view=model` 同样（若 model 视图要求 schema/tenancy ⇒ 该模式下 `?view=model` 应明确拒绝并说明，不许半成品）
- 判据：两次 sha256 对比

### AC-006 · 普通模式零行为变化

- 不带 `--ontology-only` 时，anspire 项目上：health/catalog/TTL/SPARQL 与基线一致
- **具体数字（BA 订正，实测自 `5163cfb` 二进制）**：
  - catalog：`classes=84` · `properties=952` · `relations=70` · `excluded_relations=8` · `skipped_classes=0`
  - TTL 客户视图：**293,814 B**，sha256 `A420BBCE…C070`
  - SPARQL `SELECT ?m WHERE { ?m a :llm_model } LIMIT 5` ⇒ **5 行**
- ⚠️ **不要把 TTL 的数字和 catalog 的数字混用**：TTL 里是 83 类 / 78 条边（70 true + 1 inherit + 7 false）；catalog 只列可查的 70 条，另报 `excluded_relations=8`

### AC-007 · 模式互斥

- 同时给 `--ontology-only` 与 `--schema` ⇒ **启动失败**并说明原因
- 判据：exit 码 + 报错原文

### AC-008 · MCP 一致

- `/mcp` 工具列表正常（`catalog`/`describe`/`sample`）
- `sparql_query` ⇒ 与 REST 同语义（503 `NO_DATA_SOURCE`）
- 判据：各一次

### AC-009 · `/metrics` 由 REQ-135 提供（本分支不实现）

- 本分支**不写** `/metrics` 代码（1 worktree = 1 req）
- REQ-135 合入 develop 后，本分支 rebase 并补一次集成验证：`/metrics` 200、免 token、六指标在、`ontok_ttl_bytes{view="customer"}` == ops360 TTL 字节（21,335）
- 判据：rebase 后的抓取；若 135 尚未合入 ⇒ 明确标注「依赖未合入」，不许写成已验

### AC-010 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新增依赖 |
| 出口 | 新分支登记进 `OUTLETS`（若新增 HTTP 出口） |
| 基线 | 同机 `develop` 实测 |

## 二 · 纪律

- **H** 「从快照取类/关系」只许一个函数，两个模式只在**过滤**处分叉
- **G** 变异：让 catalog 走 schema 对账（回到现状）/ 让 SPARQL 返回 200 空结果 / 让无源库时仍尝试连接 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说


---

## 三 · Dev 开工前的三条裁定（2026-10-24）

### R-1 · REQ-135 不合进本分支（选 b）

Dev 提出可把 `feature/REQ-135` 快进合入本分支以拿到 `/metrics`。

**裁定：不合。** 理由：**1 worktree = 1 req**；合进来会让本分支同时含「审计 stderr→stdout」这种会改变 AC-006 对照口径的改动，出问题分不清归因。

**执行**：本分支不写 `/metrics`；AC-009 改为**依赖 REQ-135**，rebase 后补集成验证；AC-006 的对照口径**按 `5163cfb` 纯 develop 算**。

### R-2 · **AC-002 是错的：`properties = 47`，不是 67**

我写 `≥67` 是**把合成 schema 的数字当成了本体事实**——那 20 个 `id` 列是 `make_schema.py` 硬塞的，不是本体里的。

| 数 | 来源 | 是否本体事实 |
| --- | --- | --- |
| 47 | `object_types.properties` 合计 | ✅ 是 |
| 67 | 47 + 20 个合成 `id` 列 | ❌ 否 |

**裁定**：`properties = 47`，⛔ 不许合成 `id` 列。

**理由**：对外平台拿到假的 `id` 列，比少 20 条属性**更糟**——他们会以为真有这个字段，然后来问"为什么查不到"。

> **这是「AC 也会错」的一个实例**：我把测试脚手架的产物当成了被测对象的事实。

### R-3 · MCP 保持 JSON-RPC 契约（200 + `isError` + `code`）

| 层 | 语义 | 状态码 |
| --- | --- | --- |
| REST `/api/sparql` | 应用错误 | **503 `NO_DATA_SOURCE`** |
| MCP `sparql_query` | 传输正常、工具执行失败 | **200 + `result.isError=true` + `structuredContent.code="NO_DATA_SOURCE"`** |

**理由**：MCP 客户端把 HTTP 5xx 当**传输故障**会重试——而 R4 恰恰说"重试无效"。

**要求**：两边 `code` 与 `hint` **逐字相同**，并在测试里断言这一点（这才叫"同语义"）。


---

## 四 · 交付与两条 **AC 数字勘误**（2026-10-24）

### E-1 · AC-002 的 `properties` 是 47（不是 67）

已裁定 R-2。原文的 67 = 47 + 20 个**我合成出来的 `id` 列**。已改。

### E-2 · AC-006 的 catalog 数字：**84 类 / 70 关系 / excluded 8**（不是 83 / 78）

**我错了**，实测（`5163cfb` 二进制，我自己跑的）：

```
classes=84 · properties=952 · relations=70 · excluded_relations=8 · skipped_classes=0
```

**错因**：我把 **TTL 的数字**当成了 **catalog 的数字**——

| 口径 | 类 | 关系 |
| --- | --- | --- |
| **TTL**（客户视图） | 83 | **78**（70 `true` + 1 `inherit` + 7 `false`） |
| **catalog**（可查的） | **84** | **70** + `excluded_relations=8` |

**教训**：这两个口径**不是一回事**，TTL 里有证据等级不全的边，catalog 只列能真查的。
写 AC 时必须写清是**哪个口径**的数字。

### S-1 · 「两个模式只在过滤处分叉」——**放宽为「只在过滤与取值处分叉」**

Dev 指出：它在**取值**上也分叉了（`datatype` 走 schema vs 启发式、`primary_key` 有 vs 空、`display_name` 只在有快照事实时出现）。

**裁定：接受**，理由：

| 项 | 判断 |
| --- | --- |
| 枚举来自哪 | 同一个 `snapshot_index()`（`catalog.rs:266`），无分叉 |
| 取值能不能一致 | **不可能**——没有 schema 的模式拿不出 schema 派生的值 |
| 硬要一致会怎样 | 只能**造假**（编一个 `primary_key` 或猜一个 `datatype`），正是需求「不做」的事 |

⇒ **纪律 H 的准确表述**：**枚举同一处，取值按模式如实给（缺就缺），⛔ 不许造假填平。**

### S-2 · MCP `sparql_query` 走 HTTP 200

按 R-3，已实测 `code`/`hint` 与 REST **逐字相同**。这是有意的：MCP 客户端把 5xx 当传输故障会重试，而 R4 说「重试无效」。
