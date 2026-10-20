# backlog / inbox —— 未梳理的原始想法

> 规范位置：`BA/backlog/inbox/`（见 `agent-workspace` skill）。
> 本目录此前**不存在**（2026-10-20 初始化时补齐），但 `README.md` 与 `AGENTS.md` 都已把它当作需求入口。

## 放什么

- 老板 / 会议 / 调研 / 客户现场得来的**原始诉求**，一句话也行
- 还没想清楚范围、优先级、验收标准的想法
- 外部输入（邮件摘录、截图、口头需求的转写）

## 不放什么

- 已经能写验收标准的 → 直接整理成 `BA/demands/REQ-xxx/`
- 大段设计论证 → `BA/.local/drafts/`（不跟踪），定稿再晋升到 `BA/docs/`
- 客户原始材料（含标识符）→ `BA/input/`，且必须脱敏后再入库

## 出这个目录的唯一方式

BA Agent 整理后升格为 `BA/demands/REQ-xxx/`，并在 `BA/dispatch/req-registry.md` 认领编号。
**认领前先查重**，避免同编号两行。

## 命名建议

`YYYY-MM-DD-{来源}-{一句话}.md`，例如 `2026-10-20-客户现场-套餐维度问答.md`。
