# 验收标准：REQ-136 · 本体服务模式

## 一 · AC

### AC-001 · 无源库也能起

- 环境**不设** `ONTOK_SOURCE_DATABASE_URL`、**不给** `--schema`，带 `--ontology-only` ⇒ 服务**成功启动**
- `/api/health` ⇒ 200，响应含 `mode: "ontology-only"`
- 判据：贴启动日志（要有 `mode=ontology-only（不接业务库）` 横幅）+ health 响应

### AC-002 · catalog 必须列出**关系**（本需求存在的理由）

- 用 **ops360 真实快照**（v2）⇒ `counts.classes=20`、**`counts.relations=10`**、`counts.properties≥67`
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

### AC-009 · `/metrics` 同 REQ-135

- `/metrics` 200、免 token、六指标在；`ontok_ttl_bytes{view="customer"}` = TTL 实际字节
- 判据：一次抓取

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
