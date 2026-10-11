# Agent 在线注册表

> 多 Agent 协作时共享 `demand` 分支，以 Git 为协调原语。
> 每次对 demand 的 commit 同时更新本表（心跳）。

| session-id | 角色 | 状态 | 最后活动 | 当前任务 |
| --- | --- | --- | --- | --- |
| ba-bogon-112089-20261002-165609 | ba | **online** | 2026-10-02 16:56 | REQ-002 独立 QA 审核（已派出 verifier） |
| ba-mavis-0243-20260930-2220 | ba | **stale** | 2026-09-30 22:45 | REQ-002 实施（已交付，worktree 未合并） |
| ba-local-145007-20260930-175512 | ba | **stale** | 2026-09-30 18:00 | REQ-002 范围确认与实施文档 |

**Session ID 格式**：`{role}-{hostname}-{pid}-{YYYYMMDD}-{HHmmss}`
**状态流转**：`online` → `offline`（正常注销）/ `stale`（超 30 分钟无 commit）
**追溯依据**：`git log --grep="[session: xxx]" --oneline`

> 两个 stale session 均为 2026-09-30 的历史 BA 会话，工作已交付（REQ-002 实施完成并置「待验证」）。
> 按 30 分钟无 commit 规则标记 stale，保留记录不删除。

## ⚠ 协作机制现状

本仓库**未配置 remote**（`git remote -v` 为空）。规范中的「commit + push 跨 BA 协调」
目前实际不可用，多 BA 并行协作无法进行。push 恢复前，Git 提交仅在本地有效。
