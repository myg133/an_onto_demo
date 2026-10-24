# 验收标准：REQ-141

> **依赖**：REQ-140（JSON-LD 形状）已合入。

## 一 · AC

### AC-001 · `/api/contract` 可用且免 token

- `GET /api/contract` ⇒ 200，含 `contract_version` / `jsonld.shape` / `jsonld.example` / `jsonld.notes` / `compatibility`
- 不带 `Authorization` 也能取
- 判据：响应原文 + 一次无 token 请求

### AC-002 · MCP resource 同内容

- `resources/list` 含 `ontok://contract`
- `resources/read` 取到的 JSON 与 HTTP 一致（sha256 相同）
- 判据：两处对比

### AC-003 · 版本号三处同源（R2）

- `contract` / `health` / 响应 `meta.contract_version` **三处值相等**
- 且代码里只有**一个常量**定义它（贴 `path:line` 证明）
- 判据：三处取值 + 代码位置

### AC-004 · 契约**被真实响应校验**（R3，最重要）

- 3 类真实响应各校验一次：
  1. 普通节点图（含 IRI + 字面量）
  2. 聚合（`summary` 有值、`@graph` 空）
  3. 空结果（`@graph: []`）
- 全部通过；且**校验器真能拒坏数据**
- 判据：三次通过 + 变异红

### AC-005 · 校验器有牙（变异）

| 变异 | 期望 |
| --- | --- |
| 契约里把一个必填字段改名（如 `@graph` → `graph`） | AC-004 的测试**红** |
| 把 `@context` 从 required 移除 | **红**（说明 required 被真的读） |
| 实现里删掉 `meta.warnings` | **红** |

- 判据：三条各贴红行

### AC-006 · 兼容策略写进契约（R4）

- `compatibility.policy` = 只增不减（或等价表述）
- `compatibility.breaking` = `false`
- `compatibility.changelog` 至少一条，含版本与日期
- 判据：响应原文

### AC-007 · 两种模式同契约（R5）

- `--ontology-only` 与普通模式 ⇒ `/api/contract` **字节相同**
- 判据：两份 sha256

### AC-008 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言 |
| 出口 | 新增端点要登记 `OUTLETS` |
| 回归 | JSON-LD 与 rows 回执**字节不变**；`/api/health` 只增字段 |

### AC-009 · 版本号进 `meta` 不破坏既有形状

- rows 回执与 JSON-LD 回执都带 `meta.contract_version`
- ⛔ 但 AC-009（REQ-140）的"默认回执零变化"仍要成立 ⇒ **需要 BA 确认**：
  加 `contract_version` 必然改变 rows 回执字节 ⇒ 见裁定

## 二 · 待 BA 裁定

**AC-009 与 REQ-140 AC-009 冲突**：REQ-140 要求"默认 rows 回执零字节变化"，本需求要往 `meta` 加 `contract_version`。

三种处理：

| 方案 | 影响 |
| --- | --- |
| a | 只往 **JSON-LD 的 `meta`** 加，rows 不动 | rows 零变化；但 rows 消费者看不到版本 |
| b | rows 也加，接受字节变化 | 破坏 REQ-140 的零变化承诺 |
| c | 只加在 `health` 与 `contract` 两处 | 最简单，但"每次调用能否比对"丢失 |

**BA 倾向 a**（先保证 R1 的零变化承诺，Agent 一般在启动时读 health）。

## 三 · 纪律

- **G** 变异见 AC-005
- **S** AC 也会错，发现直接说
