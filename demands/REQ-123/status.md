# 状态：REQ-123 · 查询服务对外（HTTP API + 远程 MCP）

## 当前状态

**已完成**（2026-10-23 · 已合并 `develop` `fbae80a`）

## 用户定三项

| # | 决定 |
| --- | --- |
| 1 | 只给内网/本机 |
| 2 | 静态 token 表（token → {tenant, 限额}） |
| 3 | **MCP 走 HTTP/SSE 远程** |

## 最终结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`fbae80a`**；禁改面（`serve` / `tools.rs` / `property_filter.rs` / preflight）零 diff |
| REST | `/api/sparql`、`/api/catalog`、`/api/describe`、`/api/health`（唯一免 token） |
| MCP | `POST /mcp`（JSON-RPC）+ `GET /mcp`（SSE）；工具 `catalog` / `describe` / **`sparql_query`** / `sample` |
| **两协议逐字节** | **8 组 sha256 全等**（单跳 / 两跳 / 聚合 / GROUP BY / ORDER BY / 错误路径 / 有 warning / 三表 join） |
| **租户攻击 6 项** | 全挡住（无 token / 错 token / `?token=` 在 query string / body 带别人 tenant 403 / 越权查询 0 行 / 反向对照 1 行） |
| `futures-util` | **无新 crate**（`[[package]]` 260 → 260，只多一行依赖关系） |
| 审计 | 111 行，**无 token 明文、无查询原文**；含 401/429/超时 |
| 回执 | 14 键：`columns/rows/row_count/truncated/sql_echo/tenant_applied/tables/public_dimensions/warnings/coverage/join_states/join_states_scope/join_states_scope_note/limits` |
| 测试 | lib **563** / doc 4 / live **9** |

## 两轮 QA

| 轮 | 结论 | 阻断 |
| --- | --- | --- |
| 首轮 | **FAIL** | B1 `join_states` 范围未标清（实测列出未用到的边）+ B2 **分支 0 commit**（实现全 untracked） |
| 复审 | **PASS** | B2 真提交（独立 worktree 从零 `cargo check` 通过）；B1 四处落地；M1 变异真红；`LIVE_GATE` 不掩盖问题 |

## 值得记的三条

1. **`used` 字段是 Dev 的超额交付**：我只要「标清范围」，它给每条边加了 `used`（再解析查询比对谓词，不是字符串猜）——从「告诉 Agent 范围」变成「直接给答案」。
2. **B2 是我该记的**：本地能编、能跑，**不代表提交里有**。分支零 commit 时 `cargo check` 照样绿（工作区有文件）。
3. **M1 盲区**：对拍集合里必须有**会产生 warning 的查询**，否则「两协议一致」在最容易漂移的字段上是空的。
