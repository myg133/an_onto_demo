# 状态：REQ-129 · 文档断言合并进本体提案

## 当前状态

**进行中**（2026-10-24 开单）

## 缺的是什么

```
文档 → doc ingest → assertions.jsonl
                          │
                          ⚠️ 断在这里（没有任何命令）
                          ▼
                   proposal.json → 人签 → 本体
```

`ontok doc --help` 原话：「**第一步**：非结构化文档 → 带证据的断言」「本命令**只做第一步**」。

## 要加的命令

```bash
ontok doc merge --assertions <assertions.jsonl> --proposal <proposal.json> --out <merged.json>
```

产出与 `proposal.json` **同形状**，所以后续流程不变。

## 七条合并规则

| # | 规则 |
| --- | --- |
| 1 | 只并 `alignable: true`（`future`/`example` 是设想/举例，排除并计数） |
| 2 | doc 条目**最高 medium**、`reviewed_by: null`（⛔ 不许 strong） |
| 3 | 关系必须有显式 `maps_to`，没有 ⇒ `doc_unmapped`，⛔ 不许发明表名 |
| 4 | 同实体（映射到同一表）⇒ `origin: both`，两侧证据都留 |
| 5 | 冲突（doc 基数 vs DB 推断）⇒ 双保留 + `applicable: false` |
| 6 | 确定性：两次运行逐字节相同 |
| 7 | 未签 ⇒ apply 时忽略（沿用 REQ-118 的 P2） |

## 一个边界我特意写死

**合并器不做语义猜测**。它只做机械对齐：断言自带 `maps_to` 就校验存在性并并入；
没带就进 `doc_unmapped`。

> 「文档说『套餐』→ 库里 `package_templates`」这条判断**本身需要证据 + 人签**，
> 不是合并器该干的事。

## 待办

- [ ] Dev 实现 + 真实 fixtures（56 行断言）
- [ ] QA（重点：AC-007 端到端「不签没有、签了有」）
