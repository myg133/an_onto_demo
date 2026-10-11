# Worktree 回收与巡检日志

## 已知规范偏离（避免巡检误判为违规）

| 日期 | 偏离项 | 理由 | 决策人 |
| --- | --- | --- | --- |
| 2026-09-28 | workspace 分支除 `README.md` / `.gitignore` 外，还跟踪 `AGENTS.md` 与 `.agents/` | 二者是**跨 worktree 共享的工具层**；放进 `BA/`（demand 分支）会导致 feature worktree 无法读取。不属业务代码，不违反「workspace 禁止业务代码」的核心约束 | 项目负责人 |
| 2026-09-28 | 规范文档的 `git rm -rf .` 改为 `git read-tree --empty` | 前者会**真删工作区文件**，初始化时会误删业务内容 | BA Agent |
| 2026-09-28 | 规范 `.gitignore` 的 `/*/` 改为 `/*` + 白名单 | `/*/` 会无差别忽略第一层子目录，`.agents/` 会被吃掉 | BA Agent |

## 操作规约（本次初始化得出）

> **根 worktree 恒定停留在 `workspace` 分支。**
>
> 任何其他分支的操作都通过 `git worktree add` 建独立 worktree 完成，**不要在根上 `git checkout`**。
> 原因：根上切分支会让工具层（`AGENTS.md` / `.agents/`）消失或冲突 —— 它们只存在于 `workspace` 分支。

## 初始化记录

| 时间 | 动作 | 结果 |
| --- | --- | --- |
| 2026-09-28 | `git init` + 建 `workspace` 分支，扩展白名单 `.gitignore` | 19 个文件，零业务内容 |
| 2026-09-28 | 建 `main`（orphan）/ `develop`（派生自 main） | 占位 |
| 2026-09-28 | 建 `demand`（orphan）/ `deploy`（orphan） | 骨架 |
| 2026-09-28 | 平铺建 `BA/` `code/` `Deploy/` 三个 worktree | 完成 |
| 2026-09-28 | `docs/` `input/` `archive/` 迁入 `BA/` | 完成 |

## 回收日志

| 时间 | worktree | 分支 | 原因 |
| --- | --- | --- | --- |
| 2026-10-09 | `verify-REQ-038` | `verify/REQ-038` | REQ-038 QA 验收完成。树已核实为 develop 祖先（`696aa9f`），安全删除 |
| 2026-10-09 | `feature-REQ-038` | `feature/REQ-038` | REQ-038 已快进合并进 develop（`896c43c`）。**树哈希与 develop 完全相同**（`09ed0ab9d64985055d99c642bc9f940f023db83f`），确认无残留内容 |
| 2026-10-09 | —（分支清理） | `feature/REQ-010,011,013,014,015,019,020,022,023,024,025,026,027,028,029,030,032,033` 共 18 个 | **巡检发现的历史遗留分支** —— worktree 当时已回收，但分支本身漏删，违反登记表「绝不留历史 worktree（完成即清）」。逐个核实均为 develop 祖先后删除。**这是本会话第一次真正巡检出遗留** |
| 2026-10-09 | `verify-REQ-039`、`feature-REQ-039` | `verify/REQ-039`、`feature/REQ-039` | REQ-039 合并并通过合并后全量（523 passed / 1 环境类 / 0 死锁）。`feature/REQ-039` 树哈希与 develop **完全相同**（`8e8de88419fdd330d106402e652ecadbd23dff24`），`verify/REQ-039` 经核实为 develop 祖先，安全删除 |

### 巡检附带发现（未改历史，记此备查）

提交 `d16a060` 的 commit message 标注为 `REQ-025`，但其内容（register 并发改 `ON CONFLICT` +
`migrate()` 锁覆盖全部 DDL + 删「≥3 实现」假许诺）**实为 REQ-016** 的工作，
且已被 REQ-016 的合并提交 `361a163` 吸收。

- **REQ-025（内容类缺口追加重试）至今未实现**，登记表「已就绪」状态正确 —— 是提交信息编号错，不是状态漏更新
- 按纪律「不改历史」，不重写该提交；在此登记以免后续 `git log --grep=REQ-025` 误判

# 2026-09-30 17:57 — 2026-09-30

- 认领 REQ-002（本体内核 MVP Rust 实施）
- 范围确认，10 条暂缓项标注补回时机
- 完成需求文档（demand / acceptance / status）
- 完成实施计划（11 个任务，24.6KB）

## 2026-10-10 — REQ-061 回收 + REQ-062 开单

**回收（延迟一轮的账）**：`feature-REQ-061` / `verify-REQ-061` 两个 worktree
在前一轮被记为「已回收」，**实况是仍在** —— 巡检发现后补做。

- 回收前逐项核实：`merge-base --is-ancestor <branch> develop` = YES（两个都是）、
  `git diff develop..<branch>` = **0 行**、两个 worktree `git status --short -uall` = **0 处**
- 删除分支时 `git branch -d` 报「没有完全合并」—— 原因是它拿**当前分支**（workspace `1d02ad0`）
  做合并判定，而这两个分支合入的是 **develop**。核实 tip 均为 `8aea8ad`（== develop）后用 `-D` 删除，
  **提交全部保存在 develop，零数据丢失**
- ⚠️ **教训**：`git branch -d` 的合并判定基准是 HEAD，不是目标分支。
  跨 worktree 回收时它会给出误导性报错，**先 `merge-base --is-ancestor` 对目标分支核实再删**

**开单**：建 `feature-REQ-062` worktree（基于 develop `8aea8ad`），分支 `feature/REQ-062`。

**基线备份**：`.review-workspace/fix-props/schema.json`（sha256 `b71c2afd…`）
复制到 `.review-workspace/baseline-backup/schema.baseline.json`。
起因见 `demands/REQ-062/acceptance.md` 的 AC-000 ——
**门禁现拉模式会把该文件覆盖成待审对象**，且该文件未被 git 跟踪，无 git 兜底。
