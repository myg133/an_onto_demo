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


---

## 三 · Dev 交付与裁定（J-1 / J-2）

### J-1 · `futures-util`：**接受**，但附条件

Dev 报：`Cargo.toml` 多了一行 `futures-util`，理由是把 **axum 已有的直接依赖**提升为直接依赖（同版本、同 feature），**`Cargo.lock` 只多一行 `ontok → futures-util`，没有新增 crate**。

**裁定：接受**，因为 AC-011「不引新依赖」防的是**新供应链**，而这不是新 crate。

**但必须满足三条**：

1. `cargo tree` 证明 `futures-util` **本来就在依赖树里**（经 axum），贴出证据
2. `Cargo.lock` 的 `[[package]]` 数量**不变**（只多一行依赖关系）
3. 报告里写明「这是把既有传递依赖提升为直接依赖」，⛔ 不许写成「没有新增依赖」而不解释

**若将来需要真正的流库**（如 tokio-stream 会拉入 tokio-util）⇒ 必须回到 BA 重新裁定。

### J-2 · `join_states[]` 的口径：**接受 v1，但必须标清范围**

Dev 报：`join_states[]` 给的是**涉及表之间的全部映射边**，不是 planner 实际走过的边。

**裁定：接受**，但**必须在两处写清**：

1. 字段文档/`--help`：`join_states` = 涉及表之间的**全部**映射边（供判断可信度用），**不等于**本次查询用到的边
2. 回执里加一个显式说明（如 `join_states_scope: "all_edges_between_involved_tables"`），让 Agent 不会误以为「列出来的都用了」

⛔ 不许让字段名暗示「本次用到的边」。
**精确版（只列 planner 实际走过的边）** 需要给 `Planned` 加 `join_edges` —— 属 REQ-122 结构变更，**下轮再做**，本轮登记。
