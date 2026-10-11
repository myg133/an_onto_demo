# ADR-002 · KH Agent 走 ACP（Agent Client Protocol）stdio，不走 A2A/HTTP

| 项 | 值 |
| --- | --- |
| 状态 | **已采纳** |
| 日期 | 2026-10-08 |
| 决策人 | 项目负责人（2026-10-08 选定「先摸清 mcode ACP 模式实际端点形态再定」） |
| 影响 | 设计规格 §7.1 协议选型表 / 偏差任务列表 RN-2·RN-3·RN-4 / REQ-013 |
| 会话 | `ba-archlinux-80180-20261007-231834` |

---

## 背景

偏差任务列表 RN 批次写着「HK Agent 接入（MCP / A2A / ACP + 内置 LLM fallback）」，
但动手前发现三处对不上的地方：

1. **规格与列表打架。** 规格 `spec:1022` 的协议选型表写着「**ACP（IBM）2025-08 已并入 A2A，
   不再是独立协议**」，而列表 RN-4 仍要求做「ACP 客户端」。
2. **术语与实际不符。** 规格术语表 `spec:341` 写的是「**KH Agent**」，全文没有「HK Agent」。
3. **语言分工悬空。** `spec:1027` 写「A2A 官方参考实现是 Python，spike 用 Python 连
   真实 A2A agent 是阻力最小的路径」，但主体 `ontok` 是 Rust，规格自己没闭合这一处。

项目负责人 2026-10-08 裁定：**先摸清 mcode ACP 模式的实际端点形态再定**。

---

## 实证过程

不是查文档，是实际跑起来的。

### 1 · Connector 层没有 ACP / A2A

```
mcode-tools connector tools --keyword acp   → 0 条
mcode-tools connector tools --keyword a2a   → 0 条
mcode-tools connector tools --keyword agent → 1 条（恒生金融 describe_api，误命中）
```

Connector/App 集成层（恒生金融、多媒体、邮件）**不暴露任何 agent 协议端点**。

### 2 · ACP 模式在 mcode 本体上，且是 stdio

```
$ mcode --help
  acp    Run Minimax Code as an Agent Client Protocol server over stdio
```

### 3 · 握手（实跑原文）

```
$ echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,...}}' | mcode acp
```

```json
{"jsonrpc":"2.0","id":1,"result":{
  "protocolVersion":1,
  "agentCapabilities":{
    "loadSession":true,
    "mcpCapabilities":{"http":true,"sse":true},
    "promptCapabilities":{"image":false,"audio":false,"embeddedContext":false},
    "sessionCapabilities":{"list":{},"fork":{},"resume":{},"close":{}}},
  "agentInfo":{"name":"minimax-code","title":"MiniMax Code","version":"0.5.5"},
  "_meta":{"minimax-code/extensions":{"version":1,
    "methods":["session/activate","mcode/session/steer","mcode/session/queue/*",
               "mcode/session/goal/*","mcode/session/delegation/*"], ...}}}}
```

### 4 · `session/new` 实跑成功

返回真实会话 `mvs_bf2c67997fa24331a1ec4e9a58a2cce0`，并附能力面：

- `modes`：`default` / `plan`
- 可选模型：`MiniMax-M3`、`M3.1-Flash-Preview`、`M2.7`（各带 thinking 变体）
- 可调：`permissionMode`、`model`、`thinkingEffort`
- 会话内命令：`help` `new` `model` `status` `doctor` `context` `skills` `mcp` `usage` `compact`

> `session/prompt` 未跑通：探测脚本里 sessionId 传的是字面量 `REPLACE`（管道输入无法回填）。
> **这是探测脚本的缺陷，不是协议问题。** 完整的 `initialize → session/new → session/prompt`
> 往返由 REQ-013 后续批次验证。

---

## 决策

> **KH Agent 走 `mcode acp` 的 Agent Client Protocol，JSON-RPC 2.0 over stdio，
> 由 `ontok` 以客户端身份拉起子进程对话。不实现 A2A，不实现 IBM ACP。**

同时确立三条推论：

| 推论 | 内容 |
| --- | --- |
| **D1 · 术语** | 本项目的「ACP」一律指 **Agent Client Protocol**（Zed 那套，JSON-RPC over stdio）。规格 `spec:1022` 所说的「已并入 A2A」是 **IBM 的 Agent Communication Protocol**，同名不同物。**两份文档里的 ACP 不是同一个东西**，引用时必须写全称。 |
| **D2 · 语言分工** | **问题消失**。规格 `spec:1027` 纠结 Python vs Rust，源于它假设 A2A 走 HTTP。实际是 stdio + JSON-RPC，Rust 用 `tokio::process` + `serde_json` 直接说即可 —— **不需要 `reqwest`，不需要 Python**，只需给 `tokio` 开 `process` + `io-util`。 |
| **D3 · 规格 §7.1 协议选型表对本地 agent 不适用** | 该表是为「外部 SaaS agent」设计的。我们要接的 agent 就在本机，以子进程形态存在。该表**不构成本次的技术约束**，但其**三条强制义务照旧适用**（见下）。 |

---

## 理由

| 理由 | 说明 |
| --- | --- |
| **D2 让成本比规格设想低一个量级** | 规格设想 HTTP + A2A + 跨语言；实际是拉起子进程说 JSON-RPC。零新依赖 |
| **本地 agent 无鉴权、无网络、无版本漂移** | 规格 §7.1 提的「供应商偷偷升模型」风险在此形态下**大幅降低** —— agent 版本随二进制走，可由 `agentInfo.version` 锁定 |
| **能力面已实证存在** | `loadSession` / `sessionCapabilities{list,fork,resume,close}` 都在，为 REQ-011 的版本锁定与迭代记录提供了抓手 |
| **顺带打通 RM 批次** | `agentCapabilities.mcpCapabilities = {http:true, sse:true}`，且会话内 `/mcp` 命令可用 ⇒ **这个 agent 自己能消费 MCP server**，RM 的 MCP server 做完可直接喂给它 |
| **规格 §7.1 的三条强制义务照旧适用** | 本决策只改「怎么连」，不改「连了之后必须做什么」 |

---

## 规格 §7.1 的三条强制义务（照旧适用，不因 D3 失效）

规格 `spec:1007-1015` 列的外置义务，在本决策下**一条都不能少**：

| 义务 | 落点 |
| --- | --- |
| **agent 版本必须冻结进快照** | `agentInfo.version` + `agentCapabilities` 摘要 → 进 `extracted_by.version`（沿用 ADR-001 口径） |
| **失败必须可见** | `session/prompt` 的失败组必须显式进入 `extracted_by.failed_groups`，**不参与收敛判定**。REQ-013 已落 `GroupOutcome::Failed` |
| **成本闸门** | 每个分组一次子进程会话，91 张表的轮次与 token 消耗**必须提前设闸**，超预算即停并上报 |

> ⚠️ **成本闸门目前尚未落地。** 这是 RN-2（ACP 客户端）必须带上的一条，
> 不因为「协议简单」就可以省 —— 简单的是连接，贵的仍是模型调用。

---

## 后果

### 正面
- 依赖树不变（`tokio` 加两个 feature 即可），无 HTTP/TLS 栈
- 不引入跨语言进程边界
- agent 形态可插拔：将来换别的本地 agent，只需换启动命令，不动 `trait ExtractionAgent`

### 负面 / 待办
| 项 | 说明 |
| --- | --- |
| **N1 · 规格需修订** | `spec:1022` 的协议选型表、`spec:341` 的术语表需加「本地 agent 走 ACP stdio」一条；`spec:1027` 的 Python 分工在本地 agent 场景不适用 |
| **N2 · 偏差列表需修订** | RN-3「A2A 客户端」/ RN-4「ACP 客户端」应合并为「ACP 客户端（stdio JSON-RPC）」 |
| **N3 · 同步 trait 会撞墙** | REQ-013 的 `trait ExtractionAgent` 是同步的（当时无 `async-trait` 且不许加依赖）。接真 adapter 时 `block_on` 在 `#[tokio::main]` 里会 panic —— **RN-2 很可能需要把这个 trait 改成异步**。这是已知的接口形状风险。 |
| **N4 · 成本闸门未落地** | 见上，RN-2 必须带上 |
| **N5 · 版本锁定要跨二进制** | `agentInfo.version` 是 mcode 二进制版本，不是模型版本。若要锁「模型版本」，需在会话建立时固定 `configOptions.model` 并记录 —— 否则同一台机器升级 mcode 就换了模型 |

---

## 验证记录

| 项 | 结果 |
| --- | --- |
| `mcode --help` 暴露 `acp` 子命令 | ✅ 原文「Run Minimax Code as an Agent Client Protocol server over stdio」 |
| `initialize` 握手 | ✅ `protocolVersion:1`，`agentInfo.version = 0.5.5` |
| `session/new` | ✅ 返回 `sessionId` + `modes` + `configOptions` |
| Connector 层有无 ACP/A2A 端点 | ✅ 零命中（`--keyword acp` / `--keyword a2a`） |
| **`session/prompt` 完整往返** | ✅ **2026-10-08 11:35 已实证**（此前标注 ⏳ 的探测脚本缺陷已用真正的驱动进程修掉） |

### 2026-10-08 补：完整往返实证（探测脚本见 `.review-workspace/acp-probe/probe.py`）

```
1 initialize   → protocolVersion 1 · agentCapabilities{loadSession, mcpCapabilities{http,sse},
                 promptCapabilities{image:false,audio:false,embeddedContext:false},
                 sessionCapabilities{list,fork,resume,close}}
                 agentInfo = {name: "minimax-code", title: "MiniMax Code", version: "0.5.5"}
2 session/new  → sessionId = mvs_59fd31365a8f48f2bdabe44b2c341841
                 result 键 = [configOptions, modes, sessionId]   ← 不是 capabilities/agentInfo
3 session/prompt → 11.7s，9 条消息，最终 {"id":3,"result":{"stopReason":"end_turn"}}
```

**三条对实现有直接约束的发现：**

| # | 发现 | 对实现的约束 |
| --- | --- | --- |
| **F-1** | **答案在流式通知里，不在最终响应里。** 最终 `result` 只有 `stopReason`；实际文本走 `session/update` → `sessionUpdate: "agent_message_chunk"` → `content.text` | 客户端**必须累积** chunk，**不能**只读最终响应。否则永远拿到空答案 |
| **F-2** | **ACP 自带 `usage_update` 通知**：`{"sessionUpdate":"usage_update","used":13993,"size":1000000,"cost":{"amount":0,"currency":"USD"}}` | 规格 §7.1 的**成本闸门数据源是现成的**，不必自己造计量。闸门应消费它 |
| **F-3** | `session/new` 的 result 是 `sessionId`/`modes`/`configOptions`；`capabilities`/`agentInfo` 在 **initialize** 的 result 里 | 两处结果形状不同，解析代码不能共用一套假设 |

> ⚠️ 上次探测失败的原因是**脚本缺陷**（shell 管道里无法回填 sessionId，传了字面量 `REPLACE`），
> **不是协议问题**。本次改用真正的驱动进程（`subprocess` + 逐条读写）后一次通过。

**未验证**：`session/load` / `session/resume` / `session/fork` / `session/close`
（`agentCapabilities.sessionCapabilities` 声明支持，但本轮未实测）。

---

## 相关

- 偏差任务列表：`BA/docs/design/2026-10-05-设计偏差任务列表.md`（RN 批次）
- REQ-013 验收报告：`BA/docs/reports/`（RN-1 抽象层与注册表）
- 迁移证据：`BA/docs/reports/evidence/`
- 上游决策：`ADR-001-extracted-by-policy.md`（`extracted_by` 取值口径，本决策不改它）