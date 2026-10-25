# 需求：REQ-142 · 查询默认返回 JSON-LD

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-142 |
| 优先级 | **P0**（Axon 接入前必须定，之后改不动） |
| 状态 | 进行中 |
| 基线 | `develop` `be483e7`（含 REQ-140 JSON-LD） |
| 触发 | 用户决策：**默认查询返回 JSON-LD** |

## 问题

REQ-140 把 JSON-LD 做成了**可选**：

| 调法 | 现在返回 |
| --- | --- |
| REST 不带 `format` / 不带 `Accept` | **rows** |
| REST `?format=jsonld` | JSON-LD |
| MCP `sparql_query` 省略 `format` | **rows** |
| MCP `format:"jsonld"` | JSON-LD |

⇒ **数字员工直接调，拿到的是表，不是图。** 而图才是它要的。

## 要做

默认**翻转**为 JSON-LD：

| 入口 | 默认 | 退回 rows 的方式 |
| --- | --- | --- |
| REST `POST /api/sparql` | **JSON-LD** | `?format=rows` 或 `Accept: application/json` |
| MCP `sparql_query` | **JSON-LD** | `format: "rows"` |

### 判定优先级（显式 > 隐式）

```
1. body.format（最显式）
2. ?format= 查询参数
3. Accept 头
   - application/ld+json → jsonld
   - application/json    → rows
   - */* 或缺失          → **默认 jsonld**（← 本次改变点）
```

## 不做

- ⛔ 不改 REST/MCP 的**其它**语义（鉴权、限额、审计、租户注入）
- ⛔ 不改 JSON-LD 与 rows 各自的**内容**（只改"默认选哪个"）
- ⛔ 不移除 rows 这条路（脚本仍需要表）
- ⛔ 不引依赖

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | 默认 = **JSON-LD**（REST 与 MCP 都是） |
| R2 | `format=rows` / `Accept: application/json` 必须**逐字节**等于旧默认输出（回归判据） |
| R3 | 非法 `format` 仍 **400**（⛔ 不许静默回退到默认） |
| R4 | **既有测试的改法**：凡假设默认 rows 的，改成**显式要 rows**（保留原断言），并**新增**一条"默认是 jsonld"的判据。⛔ 不许删测试、不许放宽断言 |
| R5 | skill 两份 + REQ-141 契约必须写明"默认是图、要表传 rows" |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 内部消费者（Explore agent / 脚本）默认拿到图，解析失败 | 全仓 grep `sparql` 调用点逐一确认；受影响的改成显式 rows |
| 2 | 破坏外部兼容 | **现在还没有外部消费者**（Axon 未接）⇒ 这是最后的窗口 |
| 3 | `Accept: */*`（curl 默认）语义改变 | 这正是要的；但要在文档里写明 |

## 交付物

1. 默认翻转 + 优先级实现
2. 受影响测试的改法清单（每条：改成显式 rows / 新增默认断言）
3. `format=rows` 与旧默认**逐字节相同**的证据
4. skill 与契约的文案更新
5. 已知限制
