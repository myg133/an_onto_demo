# 状态：REQ-160 · 构建端两种模式 + 抽取后自动起审核台

## 当前状态

**进行中**（2026-10-27 · 已派 Dev，worktree `feature-REQ-160`，基线 develop `e7c29aa`）

## 一句话

> 两条路进来（**独立 agent 读 skill 自己抽** / **人敲命令驱动 ACP 抽**），**一条路出去**：
> 抽完就起一个本地 console，让人在页面上审核、确认、签字。

## 两种模式

| | agent 模式 | acp 模式 |
| --- | --- | --- |
| 驱动者 | 独立 agent（只读 skill 包） | 人在终端敲命令 |
| 我们的交付 | **skill 包** + 命令 | 命令编排 |
| 收口 | skill 末步指示 agent 跑 `review open` | 编排命令自己跑 `review open` |
| 共同 | **都不自动 apply / freeze**；人签只在 console 页面上发生 | 同左 |

## 收口命令（本需求的核心）

```bash
ontok-builder review open --project <p> [--version <v>] [--bind 127.0.0.1:0] [--no-browser]
```

- 内存一次性 admin token（**不落盘、不进 URL**），同进程复用 `console::serve::serve`
- 端口冲突自动顺延；只绑回环；Ctrl-C 退出即失效
- 进度：三步 —— 校验项目 → 起服务 → 打印 URL + 口令 + 开浏览器

## 还有一条编排（acp 模式）

```bash
ontok-builder build run --project <p> --input <docs> --agent acp --agent-command pi-acp --agent-skill <path>
```

`doc ingest → doc merge → proposal import → review open` 四段串成一条；任一阶段失败即整体失败、零产物。

## 验收（8 条，见 demand.md §七）

AC-001 起得来要口令 · AC-002 URL 无口令 · AC-003 端口顺延 · AC-004 退出即失效 ·
AC-005 只绑回环 · AC-006 acp 一条命令到审核台 · AC-007 不自动签 · AC-008 skill 与二进制同版本

## 待办

- [x] **skill 部分**（用户 11:20 直接点出「skill 里需要一个审核的流程或操作项」）—— 已做，见下
- [ ] **等 REQ-159 两条支线落地再派**（用户拍板：不并行开第三路，避免三路冷编译挤一台机器）
- [ ] 派 Dev：`review open` + `build run`
      （`skill export` **已取消** —— 用户口径：skill 就在 `skills/` 目录里、由人分发）
- [ ] QA：重点 AC-002（URL 无口令）/ AC-004（不落盘）/ AC-007（不自动签）

## skill 部分已完成（2026-10-27，workspace `7cdf241`）

| 文件 | 改了什么 |
| --- | --- |
| `skills/ontok-build/SKILL.md` | 新增 **§三·六 审核台：人签在这一步发生**（起法 `review open` / 退回 `console serve`、页面四步与判据、**四条禁令**、签完才 `bundle pack`）；把路径 A/B 的 `--accept-all` 从主流程降为「人明确全部接受时」；检查单加 4 项；头加「适用工具版本」 |
| `skills/ontok-extract/SKILL.md` | 新增 **§十 抽完之后：交给人审核**（你的任务到此为止、交接口径、**四条不许代签**） |

**顺带修了一处会直接坑人的陈旧**：两份 skill 里的构建命令还写着 `ontok doc ingest` / `ontok ontology export`，
而 REQ-152 之后它们在 `ontok-builder` ⇒ **21 处命令已改**。

> ⚠️ **更正（join 2026-10-27 11:17 实证，我原先写错）**：`console` **也是构建面** ——
> `ontok console serve` 不存在，正确写法是 **`ontok-builder console serve`**。
> `ontok`（服务面）的子命令只有 **`query` / `mcp-server` / `serve`** 三个。
> skill 里那两处由 join 在 `Deploy 38f50e0` 修正；本条 status 原先写的「`console` 仍是服务面」是错的。
>
> **根因**：`console` 名字像控制面，实际在 REQ-152 拆二进制时和构建面一起搬走了 ——
> 判断「哪个二进制」要按**清单**，不能按名字像什么。

⇒ AC-008（skill 与二进制同版本 / 无「自动接受」指令）**现在可判**：两份 skill 里 `--accept-all` 只出现在「⛔ 不许」的上下文。

