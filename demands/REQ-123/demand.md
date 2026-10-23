# 需求：REQ-123 · 查询服务对外（HTTP API + 远程 MCP）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-123 |
| 优先级 | **P0**（数字员工链路的最后一块） |
| 状态 | 进行中 |
| 基线 | `develop` `e3da616` |
| 前置 | REQ-122（`sparql query` + 租户注入）、ADR-006（机制/声明分层） |

## 用户已定的三项

| # | 问题 | 决定 |
| --- | --- | --- |
| 1 | 部署面 | **只给内网/本机** |
| 2 | 凭证 | **静态 token 表**（`token → {tenant, 限额}`） |
| 3 | MCP 传输 | **HTTP/SSE 远程** |

## 架构：一个进程，两种协议，**一份鉴权**

```
POST /api/sparql     ─┐
GET  /api/catalog     ├─▶ token → tenant ─▶ query service（REQ-122）─▶ SQL
POST /mcp (JSON-RPC)  ┤        │
GET  /mcp (SSE)      ─┘        └─▶ 限额 / 审计
```

**REST 与 MCP 都只是适配器**，共用同一张 token 表、同一套限额、同一个 `run_query()`。
⛔ 不许各写一套——否则「REST 拦住了、MCP 忘了拦」这类漏洞**不会报错**。

## 范围

### 做

1. **`ontok query serve --bind <addr> --tokens <file> --project --version --schema --policy`**
2. **REST**：
   - `POST /api/sparql`（body `{query, max_rows?}`）
   - `GET /api/catalog`（分页：类 / 属性 / 关系 + 证据等级 + 枚举值）
   - `GET /api/describe?name=<class|property>`
   - `GET /api/health`（版本 + 支持的语法版本）
3. **MCP over HTTP**：
   - `POST /mcp`（JSON-RPC 2.0：`initialize` / `tools/list` / `tools/call`）
   - `GET /mcp`（SSE 流，服务端消息）
   - 工具：`catalog` / `describe` / **`sparql_query`** / `sample`
4. **鉴权**：`Authorization: Bearer <token>` → 服务端静态表 → `{tenant, max_rows, 并发上限}`
5. **限额**：行数 / 超时 / 并发 / SSE 连接数与空闲超时
6. **审计**：每次查询记 `{token 摘要, tenant, query 哈希, 行数, 耗时, 错误码}` 到 stderr/文件
7. **错误契约**：机器可读 `code` + 人可读 `message` + **可行动 `hint`**
8. **回执**：在 REQ-122 基础上加 `coverage[]` 与 `join_states[]`

### 不做

- ⛔ 不做写端点（这是查询服务）
- ⛔ 不做公网暴露（只绑内网/本机；绑 `0.0.0.0` 必须显式 flag + 警告）
- ⛔ 不在请求里接受 tenant（只能来自 token）
- ⛔ 不做 OAuth / JWT（本轮静态 token 表）
- ⛔ 不改 `serve`（那是视图服务，已有）
- ⛔ 不引新依赖（HTTP/SSE 用既有 axum）

## 关键裁定

| # | 问题 | 裁定 |
| --- | --- | --- |
| R1 | 租户来源 | **只能来自 token**；请求体带 tenant ⇒ 校验一致，不一致 **403** |
| R2 | token 位置 | 只允许 `Authorization` 头；⛔ 不许 query string / cookie |
| R3 | 绑定地址 | 默认只绑内网；`0.0.0.0` 需显式 `--allow-public` 且启动时警告 |
| R4 | 错误可行动 | 每个错误码必须有 `hint`（如列出可用属性）；⛔ 「出错了」不可接受 |
| R5 | SSE 资源 | 每 token 并发流上限 + 空闲超时；超限 **429** |
| R6 | 审计 | 每次调用都记，**含失败的**；token 只记摘要（不记明文） |
| R7 | 一致性 | REST 与 MCP 的 `sparql_query` 必须**逐字段相同**（同一条回执构造函数） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 远程 MCP 的鉴权实现走样 | 与 REST 共用中间件；AC 要求「同一 token 在两种协议下行为一致」 |
| 2 | Agent 无脑全表扫 | 行数上限 + 超时 + 只读 + 并发上限 |
| 3 | 审计日志泄露查询内容 | 记哈希不记原文；或按开关记 |
| 4 | 回执太大撑爆 Agent 上下文 | `rows` 用数组、默认 `max_rows` 小（100–200）、`truncated` 明确 |
| 5 | 内网误认为可信 | 默认只绑内网 + 启动横幅打印绑定地址与租户范围 |

## 交付物

1. `ontok query serve` + REST + MCP(HTTP/SSE)
2. token 表格式 + 示例
3. 回执与错误契约文档
4. **一致性测试**：同一查询经 REST 与 MCP 返回**逐字段相同**
5. 真实库实测（至少 5 条查询，含两跳与聚合）
6. 安全实测：无 token / 错 token / 租户不一致 / 越权查询 / SSE 超限
7. 已知限制
