# backlog / refined —— 已梳理、待排期

> 规范位置：`BA/backlog/refined/`（见 `agent-workspace` skill）。
> 本目录与 `inbox/` 一起在 2026-10-20 初始化时补齐。

## 放什么

已想清**范围 / 价值 / 验收口径**、但还**没轮到**的需求。每条应能直接回答：

| 字段 | 要求 |
| --- | --- |
| 一句话价值 | 解决谁的什么问题 |
| 验收口径 | 怎样算做完（可判真假） |
| 依赖 | 阻塞在谁 / 哪个 REQ / 哪个外部条件 |
| 粗估 | S / M / L，只要量级 |
| 不做的后果 | 用来决定排期优先级 |

## 与 `demands/` 的边界

- **`refined/`**：还没认领编号，没有 worktree，不占迭代资源
- **`demands/REQ-xxx/`**：已认领编号，有 `demand.md` / `acceptance.md` / `status.md`，可被派单

排期时把条目升格为 `demands/REQ-xxx/`，并在 `req-registry.md` 认领编号，然后从本目录删除。
