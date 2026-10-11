# 溯源分叉面：独立追链结果

## 赋值来源（裸 grep 实测）

| 位置 | 内容 |
|---|---|
| `src/bin/ontok-propose.rs:168-171` | `record_round(&reg.agent, &reg.version, round, ...)` —— **登记值**，且在 `build_agent`(190) 与 `orchestrate`(229) **之前** |
| `src/agent/orchestrate.rs:709` | `let identity = agent.identity();` |
| `src/agent/orchestrate.rs:721` | `rebuild_outcome(..., &identity.agent, &identity.version)` → `extracted_by.agent/version` |
| `src/agent/assemble.rs:293-302` | `ProposalProvenance { agent, version, .. }` 逐字写入 |
| `src/agent/mod.rs:110-123` | `stamp_provenance` **只**覆写 `round` / `extracted_by.round` / `failed_groups`；**不碰** `agent`/`version` |
| `src/agent/acp/mod.rs:334-363` | `identity_from_handshake`：agent/version 取自 `agentInfo`，即**握手值** |
| `src/agent/acp/mod.rs:379-381` | `AcpAgent::identity()` 返回握手身份 |
| `src/agent/registry.rs:111-117` | `from_registration` = **登记值**（inproc 走这条） |

**分叉结构上成立**：`record_round`(登记) vs `extracted_by`(握手) 是两条独立来源，代码里**无任何交叉核对**。
`validate()`(`src/agent/registry.rs:54-76`) 只查空/模糊版本/协议空，**不比对握手身份**。

## 新引入性
`git diff 563f26f 40b755d -- src/agent/factory.rs`：`"acp" => {...}` 分支为 `+` 新增。
⇒ Dev「之前 acp 根本造不出来」**属实**，该分叉面确为本批新引入。

## 可达性
- `capabilities()`：`src/agent/acp/mod.rs:424-429` **静态**返回 `vec![reports_usage]`，不声明 produces/rebuilds。
- `require_proposal_producing`：`src/agent/orchestrate.rs:486-519` 只放行 produces/rebuilds ⇒ AcpAgent 在**入口被拒**。
- ⇒ **今日 `extracted_by` 永远拿不到握手值**（无提案产出），分叉的**有害形态不可达**。

## 今日**实际**可达的偏差（与 Dev 描述不同）
`record_round` 先落库、`require_proposal_producing` 后拒绝 ⇒ `agent_rounds` 里留下一行
「mcode-agent 跑了第 N 轮」，但该轮**一份提案都没产出**。这是**幽灵轮次**，不是 Dev 描述的双值分叉。
注：`src/bin/ontok-propose.rs` **不在本次 diff 内**（该位置是 REQ-013/015 既有的）。
