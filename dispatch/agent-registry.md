# Agent 在线注册表

> 多 Agent 协作时共享 `demand` 分支，以 Git 为协调原语。
> 每次对 demand 的 commit 同时更新本表（心跳）。

| session-id | 角色 | 状态 | 最后活动 | 当前任务 |
| --- | --- | --- | --- | --- |
| ba-michael-worksta-14864-20261020-151752 | ba | **online** | 2026-10-21 10:05 | REQ-107 已完成并合入 develop `4573269`；worktree/分支已回收 |
| ba-bogon-112089-20261002-165609 | ba | **stale** | 2026-10-02 16:56 | REQ-002 独立 QA 审核（已派出 verifier） |
| ba-mavis-0243-20260930-2220 | ba | **stale** | 2026-09-30 22:45 | REQ-002 实施（已交付，worktree 未合并） |
| ba-local-145007-20260930-175512 | ba | **stale** | 2026-09-30 18:00 | REQ-002 范围确认与实施文档 |

**Session ID 格式**：`{role}-{hostname}-{pid}-{YYYYMMDD}-{HHmmss}`
**状态流转**：`online` → `offline`（正常注销）/ `stale`（超 30 分钟无 commit）
**追溯依据**：`git log --grep="[session: xxx]" --oneline`

> 两个 stale session 均为 2026-09-30 的历史 BA 会话，工作已交付（REQ-002 实施完成并置「待验证」）。
> 按 30 分钟无 commit 规则标记 stale，保留记录不删除。
> `ba-bogon-112089-…` 原是唯一 `online` 行，**2026-10-20 巡检按同一规则降为 stale**（最后活动 2026-10-02，已 18 天）。

## ⚠ 协作机制现状（2026-10-20 订正）

**原记录已过期**：本表曾写「本仓库未配置 remote，push 不可用」。现在 remote 已配置：

```
origin  git@github.com:myg133/an_onto_demo.git
```

`workspace` / `demand` / `develop` / `deploy` / `main` 五个分支远程均存在，本地 worktree 各分支均跟踪对应远程。

**但下面这条仍然成立，且更要紧**：`demand` 分支的提交历史是**脱敏快照同步**（提交信息形如
`[同步] …（本地 demand 的脱敏快照）`），**全部不含 `[session: …]` 尾注** ——
所以规范里「用 `git log --grep` 追溯 session 活动」在本仓库**当前得不到任何结果**
（实测三条历史 session 的 `--grep` 命中均为 0）。本表的时间戳是目前唯一的活动依据。
