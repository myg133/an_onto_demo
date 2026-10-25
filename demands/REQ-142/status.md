# 状态：REQ-142 · 查询默认返回 JSON-LD

## 当前状态

**已完成**（2026-10-25 · 合并 `develop` `f04b2ae` · QA PASS）

## 决策

> 用户：**默认查询返回用 json-ld**。

REQ-140 做成了可选，默认仍是 rows ⇒ 数字员工直接调拿到**表**，不是**图**。

## 改后行为

| 入口 | 默认 | 退回表格 |
| --- | --- | --- |
| REST `POST /api/sparql` | **JSON-LD** | `?format=rows` 或 `Accept: application/json` |
| MCP `sparql_query` | **JSON-LD** | `format: "rows"` |

判定优先级（以代码现状为准）：`?format=` > `body.format` > `Accept` > **默认 jsonld**

## 为什么现在改

| 现在 | 之后 |
| --- | --- |
| 外部消费者**还没接**（Axon 未接） | 接上后再改就是破坏性变更 |

⇒ **这是最后的免费窗口**。

## 我的执行顺序（先 142，再 141）

```
142（默认翻转，1 小时）
  ↓
141（契约端点，把「默认 = jsonld」写进契约）
  ↓
Axon 接入
```

**理由**：契约要描述的是**最终行为**。先翻默认，再写契约，契约才不会一上线就过时。

## 待办

- [ ] Dev（默认翻转 + 测试改法 + 内部消费者排查）
- [ ] QA（重点：`format=rows` 逐字节等于旧默认、`*/*` 走 jsonld、非法 format 仍 400）

## 结果（QA 独立复验）

| 判据 | 结果 |
| --- | --- |
| REST / MCP 默认 | ✅ **JSON-LD**（顶层 `@context`/`@graph`/`summary`/`meta`） |
| `Accept` 四态 | ✅ ld+json→图；application/json→**rows**；`*/*`→**图**；缺失→图 |
| 退回 rows | ✅ 3 查询 × 2 种退回方式 = **6/6 sha 相同**（与旧默认逐字节） |
| JSON-LD 内容 | ✅ 3/3 与改动前 `?format=jsonld` 相同 |
| 非法 format | ✅ 400 + 可行动 hint（⛔ 不静默回退） |
| 内部消费者 | ✅ 工作台查询台已显式 `format:"rows"`（实抓页面 + 实跑验证） |
| 测试 | lib 732→**733**；live 9→**10**；6 处只加 `format`，17 个既有函数 assert **零差异** |

## QA 抓到我三处问题（已处理）

| # | 问题 | 处置 |
| --- | --- | --- |
| N-1 | Dev 自报「6 target / 8 条红」复现不出（实测无夹具 29/183、有 DB 4/5） | 记录；**又一次基线数字失真** |
| N-2 | 欠账清单漏了**仓库根 `skills/ontok-query/SKILL.md`**（与 Deploy 那份同字节，agent 实际读它） | ✅ 两份都改了，仍同字节 |
| **N-3** | **我自己的 demand/status 还写着被 R-1 推翻的优先级** | ✅ 已订正 |

> N-3 值得记：**我发出去的裁定，没有同步回需求文本**。裁定写在 acceptance 里，demand 的表格却还是旧的——下一个人读 demand 就会读到错的。
