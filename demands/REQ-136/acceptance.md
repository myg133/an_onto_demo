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

- 不带 `--ontology-only` 时，anspire 项目上：health/catalog/TTL/SPARQL 与基线一致（catalog counts、TTL 字节数、SPARQL 行数）
- 判据：三个数字对比基线（TTL 293,814 B / catalog 83 类 / SPARQL 5 行）

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
