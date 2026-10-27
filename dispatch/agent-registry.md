# Agent 在线注册表

> 多 Agent 协作时共享 `demand` 分支，以 Git 为协调原语。
> 每次对 demand 的 commit 同时更新本表（心跳）。

| session-id | 角色 | 状态 | 最后活动 | 当前任务 |
| --- | --- | --- | --- | --- |
| ba-join-michael-workstation-1340-20261027-093836 | ba | **online** | 2026-10-27 09:40 | join · 服务端（与 lion 分工：lion=本地抽取端）|
| ba-lion-michael-workstation-49056-20261027-094244 | ba | **online** | 2026-10-27 09:42 | lion · 本地抽取端（ontok-builder / 抽取 / 提案 / AM 生成 / REQ-153）；入场巡检 + 账本纠偏 |
| ba-michael-worksta-14864-20261020-151752 | ba | **stale** | 2026-10-21 18:20 | REQ-108 已完成（合并 `a8a51ed`）；REQ-109 AI 接入开工 |
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


## 2026-10-27 · 双 BA 分工（join + lion）

| BA | 负责面 | 边界 |
| --- | --- | --- |
| **join**（本会话 `ba-join-michael-workstation-1340-20261027-093836`） | **服务端**：`ontok` 服务面（query serve / Studio / REST+MCP / 契约）、`Deploy`、本体冻结与发布、接入基线 | ⛔ 不改抽取链路（doc ingest / propose / AM 生成） |
| **lion**（本会话 `ba-lion-michael-workstation-49056-20261027-094244`） | **本地抽取端**：`ontok-builder` 的抽取/提案/本体构建、AM 生成、REQ-153 覆盖盘点 | ⛔ 不改服务面与部署 |

**同机协作事实**：lion 与 join 共用同一份工作目录（同一 `.git`），不做「拉取式」同步 ——
双方写的是同一棵树，冲突靠「写前读 + 只改自己负责的文件」避免，不靠 merge。
跨面改动（如 AM 契约形状）先在 `dispatch/handoff.md` 留言，再动手。
