# 状态：REQ-123 · 查询服务对外（HTTP API + 远程 MCP）

## 当前状态

**进行中**（2026-10-23 开单）

## 用户定三项

| # | 决定 |
| --- | --- |
| 1 | 只给内网/本机 |
| 2 | 静态 token 表（token → {tenant, 限额}） |
| 3 | **MCP 走 HTTP/SSE 远程** |

## 架构

```
POST /api/sparql     --+
GET  /api/catalog     --+--> token -> tenant --> query service（REQ-122）--> SQL
POST /mcp (JSON-RPC)  --+          |
GET  /mcp (SSE)      --+          +--> 限额 / 审计
```

**REST 与 MCP 共用一份鉴权**。各写一套的后果：REST 拦住了、MCP 忘了拦——**而且不会报错**。

## 唯一不能妥协的点

**租户只能来自 token。**

```
x  POST /api/sparql { "tenant": "C0001", ... }   <- Agent 可改，等于没有隔离
OK Authorization: Bearer <token>                 <- 服务端映射到 tenant
```

请求体即使带 tenant，也只能用于校验一致性（不一致 => 403）。token 不许进 query string（日志/Referer 会漏）。

## 因为走远程 MCP 加的三条约束

| 风险 | 约束 |
| --- | --- |
| token 进 URL | 只允许 Authorization 头 |
| SSE 长连接占资源 | 每 token 并发流上限 + 空闲超时 |
| 内网误认为可信 | 默认只绑内网；0.0.0.0 需显式 flag + 启动警告 |

## 待办

- [ ] Dev 实现
- [ ] QA（重点：租户来自 token、两协议回执逐字段相同）
