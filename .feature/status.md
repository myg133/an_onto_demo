# 开发状态 · REQ-112 审核界面接入 AI 建议

> worktree `feature-REQ-112` · 分支 `feature/REQ-112` · 基线 `444080f`（QA 首轮 FAIL）· **未 push**
> 本轮性质：**阻断修复（B-1）+ 两条非阻断（N-2 / N-3）** —— 不扩大范围

- **需求**：REQ-112 · 审核界面接入 AI 建议（AC-001~AC-010）
- **当前任务**：B-1 阻断 + N-2 白名单 + N-3 交付物 已改完，**待 QA 复审**
- **状态**：待验证（PR 与状态机流转由 BA 决定，Dev 不发布）
- **日期**：2026-10-21

## 本轮只改了三处（其余一行未动）

| # | 项 | 文件（`path:line`） | 一句话 |
| --- | --- | --- | --- |
| **B-1** | 冲突规则不可达 | `src/domain/confidence.rs:1559-1571`（判据在 `:1575 match c.band` **之前**） | `matched == sampled && sampled > 0 && fit < 0.50` ⇒ `conflict`，**不限档位** |
| **N-2** | 两条批量路径各有一份白名单 | `src/review_ui/http.rs:517`（`batch_gate`）+ `:598` / `:631` 两个调用点 | `by=confidence` 也过 `enters_batch()`；`conflict`/`none`/`review`/`reject` 一律不进 |
| **N-3** | 交付物仍是旧需求 | `.feature/verification-report.md` / 本文件 | 报告写 REQ-112 段（REQ-111 原文保留在下方），A-3 两条如实登记 |

附带一处**口径同步**（N-2 的直接后果）：`src/review_ui/page.rs:436-442`，「按档位」弹窗把
`冲突 / 人工核对 / 拒绝` 三类被排除的条数也说出来 —— 否则「将接受 N 条」会让人以为剩下的也被一起接受。

## 判据实测（摘要，细节见 `verification-report.md`）

| 判据 | 实测 |
| --- | --- |
| 结构全 1.0 + 5/5 + fit=0.0（生产评分函数） | `score=0.85` / `band=middle` / `kind=conflict`（**不是** `lean_accept`） |
| 该条在 `/api/preview?by=suggestion` 的排除里 | 真 serve：`count=2 items=[3,4] excluded.conflict=1` |
| 边界 `fit=0.50` / `4/5` / `sampled=0` | 分别是 `review` / `lean_accept` / `reject` —— **都不是 conflict** |
| N-2：`min_confidence=0.80` | 真 serve：`items=[3,4]`，`score=0.85` 的冲突**不在**白名单 |
| 真实数据不受影响 | 135 条：`accept 29 / lean 9 / review 14 / none 83 / conflict 0`；`by=suggestion` 仍是 **38** |

## 测试

```text
lib 245 · doc 4 · 43 套件 422（基线 416 + 本轮新增 6 条 B-1/N-2 用例）
test_req112_suggestion 27（原 21 + 6）· test_req111_review_ui 14（不变）
两处既有红灯归因不变（test_retained_names_req084 绕行必编不过 / test_agent_orchestration 的 CLI 端到端）
```

## 如实登记（A-3 / A-7）

1. **当前数据集无冲突样本** —— 上面的冲突条目是**构造样本**（`dev112c`），不是真实数据观测；
2. **冲突分组「排最前」只有源码级断言**，**无浏览器验证**（本机无 headless browser）。

## 临时数据（未清理，本机无 psql/删除命令）

`anspire_real` 库新增 3 个项目：`dev112fix`（135 条，真实 p1 副本）/ `dev112c`（6 条，B-1 构造）/ `dev112b`（3 条，边界构造）。
