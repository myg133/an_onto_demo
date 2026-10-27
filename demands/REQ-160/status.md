# 状态：REQ-160 · 构建端两种模式 + 抽取后自动起审核台

## 当前状态

**已就绪**（2026-10-27 开单 · P1 · lion）

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

- [ ] **等 REQ-159 两条支线落地再派**（用户 11:50 拍板：不并行开第三路，避免三路冷编译挤一台机器）
- [ ] 派 Dev：`review open` + `build run`
      （`skill export` **已取消** —— 用户口径：skill 就在 `skills/` 目录里、由人分发；改成在 `skills/ontok-extract` 与 `skills/ontok-build` 末尾补收口段、头部写适用工具版本）
- [ ] QA：重点 AC-002（URL 无口令）/ AC-004（不落盘）/ AC-007（不自动签）
