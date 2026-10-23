# 需求：REQ-127 · 查询服务提供 OWL TTL 下载

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-127 |
| 优先级 | **P0**（数字员工要能拿到本体文件） |
| 状态 | 进行中 |
| 基线 | `develop`（REQ-126 合并后） |
| 前置 | REQ-119（TTL 序列化）、REQ-123（query serve）、REQ-124（console 已有 export/ttl） |

## 问题

数字员工（pi / 任意 Agent）现在只能用 `/api/catalog` 与 `/api/describe` **一问一答**地了解本体；
拿不到**整份 OWL/TTL**，因此：

- 无法离线推理/加载进图库
- 无法把本体与自己的提示词一起用
- 排查「为什么这个属性查不到」时无据可查

`console serve` 有 `GET /api/export/ttl`（管理面，admin token），但**数字员工用的是 query token**，拿不到。

## 要做

在 **query serve** 上新增一个只读端点：

```
GET /api/ontology.ttl                  → 客户视图（默认）
GET /api/ontology.ttl?view=model       → 建模视图（含主键/枚举/租户列/证据等级）
Authorization: Bearer <QUERY_TOKEN>
```

| 项 | 要求 |
| --- | --- |
| Content-Type | `text/turtle; charset=utf-8` |
| 认证 | 必须有 query token；无 token ⇒ **401**；admin token ⇒ **401**（两套凭证不通用） |
| 租户 | **不注入租户过滤**——本体是共享的（不含任何客户数据） |
| 客户视图 | ⛔ 不含 `ontok:tenant*`、不含 `ontok:joinEvidence`、不含 `ontok:primaryKeyColumn` |
| 建模视图 | 含上述建模信息；需显式 `?view=model`（默认不给） |
| 字节一致 | 与 `ontok ontology export`（同参数）产出的文件 **sha256 相等** |
| 确定性 | 同一版本两次请求逐字节相同 |
| 缓存 | 支持 `ETag` / `If-None-Match` ⇒ 304（本体随版本冻结，几乎不变） |
| 审计 | 每次请求记一行（token 摘要 / view / 字节数 / code） |
| 大小 | 建模视图约 420 KB；⚠️ 不压缩也接受，但支持 `Accept-Encoding: gzip` 更好 |

### 顺带

- MCP 侧：把 TTL 作为 **resource** 暴露（`resources/list` + `resources/read`，uri 如 `ontok://ontology/model.ttl`），让支持资源的 Agent 直接取
  - 若 MCP resource 实现成本高，**可降级为工具** `get_ontology(format:"ttl", view:"model")` —— 但要说明选了哪个

## 不做

- ⛔ 不做写操作
- ⛔ 不新增第二套 TTL 序列化（必须复用 REQ-119 的序列化器）
- ⛔ 不在 TTL 里注入任何客户数据（只导出本体结构）

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | **客户视图默认**：客户 token 不该一眼拿到内部列名；要 modeling 必须显式要 |
| R2 | 两种视图都**不注入租户**——本体不带数据 |
| R3 | **字节一致**：与 CLI 导出对比 sha256，⛔ 不许「另写一个序列化器」 |
| R4 | `ETag` 用内容 sha256 前 16 位；`If-None-Match` 命中 ⇒ 304 且体为空 |
| R5 | 401/403 语义与既有 query 端点一致（只认 `Authorization` 头） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 建模视图暴露内部列名 | 默认客户视图 + 显式 `?view=model` + 文档写明 |
| 2 | 420 KB 每次重导 | ETag + 304；可加进程内缓存（版本不变则复用字节） |
| 3 | 与 CLI 导出漂移 | AC 用 sha256 对拍 |
| 4 | 大文件阻塞单线程 | 已有并发限额；TTL 导出是纯计算 + 内存 |

## 交付物

1. `GET /api/ontology.ttl`（两视图）+ 401/304 行为
2. MCP：resource 或 tool（说明选哪个及理由）
3. 与 CLI 的 sha256 对拍证据
4. 审计样例
5. 已知限制
