# FDE 本体论平台

> 把客户系统里的数据与 know-how，变成 Agent 能理解的业务语义。

本仓库采用 **agent-workspace** 组织规范：仓库根是「根容器 + 工具层」，业务内容全部在 worktree 里。

---

## ⚡ 快速入口

| 我想… | 打开 |
| --- | --- |
| **十分钟了解全部取舍与边界** | [`BA/docs/design/ontology-platform-spec.md` §0.1](BA/docs/design/ontology-platform-spec.md) |
| 给**老板 / 客户**讲这套平台 | [`BA/docs/reports/index.html`](BA/docs/reports/index.html) |
| 开始**写代码** | `code/`（本体内核已合入 `develop`） |
| 提**新需求** | `BA/backlog/inbox/` → BA Agent 整理为 `BA/demands/REQ-xxx/` |

---

## 目录 ↔ 分支对应表

> 所有 worktree 在仓库根**平铺**，互为兄弟目录，无嵌套。

| 目录 | 分支 | 用途 | 谁写 |
| --- | --- | --- | --- |
| *(仓库根)* | `workspace` | **根容器**：导航 + 工具层（`AGENTS.md` / `.agents/`）。**不跟踪任何业务内容** | BA Agent（极少） |
| `BA/` | `demand` | **需求与设计**：设计规格（事实源）、实现计划、HTML 汇报、原始输入、历史存档、需求文档 | BA Agent |
| `code/` | `develop` | **代码**：产品实现（Rust 后端 / Svelte 前端）。PR 目标分支 | Dev Agent |
| `Deploy/` | `deploy` | **部署配置**：helm / 环境变量 / 发布脚本。**只做配置，不做构建** | Deploy Agent / CI |
| `feature-REQ-xxx/` | `feature/REQ-xxx` | 单个需求的开发 worktree，PR 合并后立即回收 | Dev Agent |
| `hotfix-xxx/` | `hotfix/xxx` | 紧急修复，基分支 `main` | Dev Agent |

**孤儿分支**：`workspace` / `demand` / `deploy` 互相无父，各自管理完整根与 `.gitignore`。
**派生关系**：`main` → `develop`（`feature/*` 从 `develop` 切出）；`hotfix/*` 从 `main` 切出。

---

## 本仓库的内容都在哪

| 内容 | 位置 |
| --- | --- |
| **设计规格（唯一事实源）** | `BA/docs/design/ontology-platform-spec.md` |
| 实现计划 | `BA/docs/plans/` |
| 汇报材料（自包含 HTML，可离线双击） | `BA/docs/reports/` |
| 原始输入（老板 / 会议 / 调研，只读） | `BA/input/` |
| 历史版本（仅供追溯） | `BA/archive/` |
| **临时工作区**（审核 / 变异，不跟踪） | `.review-workspace/`（约定见其 README） |
| Agent 工具层（skills + 角色定义） | `.agents/`（workspace 分支，跨 worktree 共享） |
| Agent 入口指令 | `AGENTS.md`（workspace 分支） |

---

## 快速上手

```bash
# 1. 克隆
git clone <url> && cd Ontology

# 2. 建 worktree（路径基准 = 仓库根）
git worktree add BA demand
git worktree add code develop
git worktree add Deploy deploy

# 3. 提需求：写 BA/backlog/inbox/，BA Agent 整理为 REQ-xxx
# 4. 开发：BA 认领 REQ 编号后创建 feature worktree
git worktree add feature-REQ-001 -b feature/REQ-001 develop

# 5. 合并后回收
git worktree remove feature-REQ-001 && git branch -d feature/REQ-001
```

---

## 各角色 Agent 入口

| 角色 | 工作区 | 规范文件 |
| --- | --- | --- |
| **BA**（需求 / 架构） | `BA/` | [`.agents/agents/anspire-open-ba.md`](.agents/agents/anspire-open-ba.md) |
| **Dev** | `feature-REQ-xxx/` | [`anspire-open-developer.md`](.agents/agents/anspire-open-developer.md) |
| **QA** | `feature-REQ-xxx/` → `code/` | [`anspire-open-qa.md`](.agents/agents/anspire-open-qa.md) |
| **Deploy** | `Deploy/` | [`anspire-open-deploy.md`](.agents/agents/anspire-open-deploy.md) |
| **Explore** | 按需 | [`anspire-open-explore.md`](.agents/agents/anspire-open-explore.md) |

统一规范：[`.agents/skills/agent-workspace/SKILL.md`](.agents/skills/agent-workspace/SKILL.md)

---

## workspace 分支的硬约束

`workspace` 分支**不跟踪任何业务内容**。验证：

```bash
git ls-files
```

预期只输出 `README.md` / `AGENTS.md` / `.gitignore` 与 `.agents/` 下的文件。

> **⚠ 已知偏离（2026-09-28）**：agent-workspace 原规范要求「只跟踪 README.md + .gitignore」两项。
> 本项目额外跟踪 `AGENTS.md` 与 `.agents/`，因为它们是**跨 worktree 共享的工具层** ——
> 放进 `BA/` 会导致 feature worktree 无法读取。详见 [`.gitignore`](.gitignore) 顶部注释。

---

## 当前状态

> 更新：2026-10-20（BA 初始化时按 `demand` / `develop` 实况校准；此前的「2026-10-04」版本停在 REQ-004）

| 项 | 状态 |
| --- | --- |
| 设计规格 | v1 事实源（1427 行）+ v2 架构提案与实施规划（2026-10-16） |
| **代码** | `develop` `b714f48`（REQ-106 已合并）· 全量基线 **62 suite / 846 passed / 0 failed / 1 ignored** @ `7f26fa2` |
| 需求账本 | [`BA/dispatch/req-registry.md`](BA/dispatch/req-registry.md) **107 条**：已验证 50 · 已完成 25 · 已就绪 11 · 草稿 12 · 其他 9 |
| 当前迭代 | [`BA/sprint/current.md`](BA/sprint/current.md)（Sprint 2 · v2 出口收口）· Sprint 1 留档见 [`BA/sprint/retrospective.md`](BA/sprint/retrospective.md) |
| 汇报材料 | 三份 HTML + 索引 + v2 模块设计，均自包含零外部依赖 |

> ⚠️ **账本口径**：`BA/demands/` **不是**事实源 —— REQ-103/104/105/107 连目录都没有，
> REQ-106 只有 `acceptance.md`。状态一律以 `req-registry.md` 与 `develop` 提交为准。

### v2 进展（阶段 1 已完成）

| 阶段 | REQ | 状态 |
| --- | --- | --- |
| 1a · user view HTTP server（只读图检查） | REQ-093 | **已验证** |
| 1b · 图投影 + graph view | REQ-094 | **已验证** |
| 0b · 规则所有权与审核（**决策 D2**）★ 硬门槛 | REQ-100 | 草稿 · 等人拍板 |
| 4 · `ItemKind` 扩 action / function | REQ-098 | 草稿 · P0 无依赖，是阶段 5/6 的共同前置 |
| 6 · 推理层 | REQ-102 | 草稿 · 被 REQ-100 阻塞 |
| 出口收口（25 个出口各自决定呈现） | REQ-107 | 待开单（从 REQ-106 拆出） |

### v1 需求进展（留档）

| REQ | 名称 | 状态 | 结果 |
| --- | --- | --- | --- |
| REQ-001 | 抽取可信度 spike | 待验证 | 技术侧 6/7 通过，**AC-005 客户判读待组织** |
| REQ-002 | 本体内核 MVP（Rust） | **已完成** | 两轮审核闭环 → `develop` `723cc87` |
| REQ-003 | 从真实源库抽取本体 | **已完成** | 三轮审核闭环 → `develop` `3815870` |


### 已经能跑的链路

从**真实客户库**（91 表 / 1051 列）到客户视图，全链路已通：

```
只读勘查 → 抽取本体 → 提案 → 双轨审核 → apply → 冻结版本 → 客户视图 → diff
  91 表      91 实体      142 条目   284 次决定            v2       91 个对象   142 处
             964 属性
             51 关系
             零悬空
```

| 指标 | 抽取前 | 现在 |
| --- | --- | --- |
| 客户视图对象数 | 0 | **91** |
| 平均属性 / 对象 | 2.2 | **8.7** |
| 零属性对象 | 14 | **0** |
| 关系 | 0 | 51（零悬空） |

```bash
# 端到端复现（需源库连接串 + 本体库连接串）
cd feature-REQ-003 2>/dev/null || cd code   # worktree 已回收，代码在 develop
export ONTOK_SOURCE_DATABASE_URL="postgresql://…:…@host:port/db"   # 只读勘查器自己会强制只读
export ONTOK_DATABASE_URL="postgresql://…"
bash BA/e2e-req003.sh
```

### 仍未验证的核心问题

> **「客户认得这个本体吗」至今零验证。**
> 抽取质量已达标，但**客户判读（AC-013）需 3 名非开发者外部评审人**才能做。

判读材料与八问清单已备好：[`BA/docs/reports/2026-10-03-ac013-judging-playbook.md`](BA/docs/reports/2026-10-03-ac013-judging-playbook.md)

其中 **Q6「哪些其实是同一个东西」** 直接产出 REQ-004 的概念归并规则
（91 张表实为约 65 个业务概念，19% 的表属于「一个概念拆多表」）。

**治理机制救不了客户不认的本体。** 详见规格 §0.1 G。

---

<footer>
FDE 本体论平台 · workspace 根容器
事实源：<code>BA/docs/design/ontology-platform-spec.md</code> · 进度更新 2026-10-20
</footer>
