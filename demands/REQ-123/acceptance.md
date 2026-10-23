# 验收标准：REQ-123 · 查询服务对外（HTTP API + 远程 MCP）

> **一句话**：两个协议，一份鉴权；**租户只能来自 token**。

## 一 · AC

### AC-001 · REST 基本可用

| 端点 | 判据 |
| --- | --- |
| `POST /api/sparql` | 带 token + 合法查询 ⇒ 200 + 回执；缺 body 字段 ⇒ 400 |
| `GET /api/catalog` | 返回类清单（分页），含**关系证据等级**与枚举值 |
| `GET /api/describe?name=X` | 类/属性详情；未知名字 ⇒ 404 + 相近项 |
| `GET /api/health` | 200 + 版本 + 支持的语法版本；**免 token**（不含任何数据） |

### AC-002 · MCP over HTTP 可用

- `POST /mcp` 支持 `initialize` / `tools/list` / `tools/call`
- 工具清单含 **`sparql_query`**
- `GET /mcp` 能建立 SSE 流（服务端消息）
- 判据：用 JSON-RPC 手工调一次 `tools/call` → `sparql_query`，拿到与 REST 相同的结果

### AC-003 · **租户只能来自 token**（核心安全）

| 攻击 | 期望 |
| --- | --- |
| 请求体带 `tenant: B`，token 绑定 A | **403**（`TENANT_MISMATCH`） |
| 请求体不带 tenant（正常） | 200，注入 A |
| 用 A 的 token 试图查 B 的数据（靠 SPARQL 写条件） | 结果里 **B 的行数 = 0** |
| token 放在 query string | **不生效**（401，因为只认头） |
| 无 token | 401（不透露任何信息） |

### AC-004 · **两个协议行为一致**（防漂移）

同一 token、同一查询，分别经 REST 与 MCP：

- `rows` / `columns` / `row_count` / `truncated` / `warnings` / `coverage` / `join_states` **逐字段相同**
- 出错时 `code` 也相同
- 判据：脚本对拍（序列化后逐字节），不是「看起来一样」

### AC-005 · 限额与资源

| 项 | 期望 |
| --- | --- |
| `max_rows` 超限 | 截断 + `truncated: true`；token 级上限不可被调用方抬高 |
| 超时 | 中止 + `TIMEOUT` |
| 并发超限 | **429** + 可读提示 |
| SSE 连接数超限 | **429** |
| SSE 空闲超时 | 主动断开并记审计 |

### AC-006 · 错误可行动

每个错误码必须有 `hint`：

| code | hint 要求 |
| --- | --- |
| `SPARQL_UNSUPPORTED` | 指出哪个子句不支持 + 支持清单入口 |
| `UNKNOWN_PROPERTY` | 列出该类的属性（或相近项） |
| `TENANT_UNRESOLVED` | 指出哪张表没登记 + 去改哪个声明 |
| `TENANT_MISMATCH` | 说明「租户由凭证决定」 |

- 判据：≥4 个错误码，`hint` 非空且能据此修正

### AC-007 · 审计

- 每次调用（含失败）产生一条审计记录：`时间 / token 摘要 / tenant / query 哈希 / 行数 / 耗时 / code`
- 判据：⛔ **不记 token 明文**；⛔ 不记查询原文（或按显式开关）

### AC-008 · 绑定与启动横幅

- 默认只绑内网地址；`--allow-public` 才能绑 `0.0.0.0`
- 启动横幅打印：绑定地址、项目/版本、token 数量、租户数量、**并在绑公网时警告**
- 判据：两种绑定各一次；绑 `0.0.0.0` 无 flag ⇒ 拒绝启动

### AC-009 · 只读

- 查询走只读连接（与 REQ-122 一致）；写操作无法表达
- 判据：写类 SPARQL ⇒ 400；库级只读会话证据

### AC-010 · 真实库实测

- 至少 5 条：单跳 / 两跳 / 聚合 / GROUP BY / ORDER BY+LIMIT
- 每条给出 `row_count` 与 `warnings`（若命中 `main_only` 表必须有警告）

### AC-011 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `serve` 的路由与鉴权 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 基线 | REQ-122 合并后的 lib 520 / doc 4 |

## 二 · 纪律

- **H** 鉴权中间件与回执构造函数各只许一处（REST/MCP 共用）
- **G** 变异三元组：让 MCP 跳过鉴权 / 允许 body 覆盖 tenant / 回执两条路径各构造一次 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说
