# 验收标准：REQ-052 碰撞文案区分 + 一条失败信息订正

> 依据 REQ-051 验收（QA 判 PASS 带 2 项非阻断）写成。基线：`a55ffad`（develop）。
>
> **⚠️ 这是纯文案单，判别力零变化。** 派它不是因为有洞，是因为**话说错了**。

---

## 背景：为什么话必须改

QA 实测推翻了 REQ-051 里那句「空 `group_id` 的 adapter **已经踩了 REQ-050 反复计入的 bug**」。

**那是错的。** 实测：两组各 +100，`used = 200` —— **恰是正确总额**。
因为只有**一个键** `("", 0)`，`insert` 是覆盖语义，`totals()` 取最后一次累计
= 正确的整轮总额。**「N 倍计入」只在每组各有不同 `group_id` 时发生，空 `group_id` 恰恰不会。**

⇒ 当前文案在那个场景下说「账本不可信」，**而账其实是对的** —— 文案与事实不符。

**仍然非阻断**（所以只是改文案）：
- 该场景确实违反 `UsageReport.used` 的增量契约
- `derive_group_id`（`src/agent/group.rs:271-273`）恒返回 `table:{name}`，**生产不可达**
- fail-closed 方向安全：**停 = 白跑 ≠ 多花钱**

---

## AC-001 · 区分「契约违规」与「账已经错了」

- 预期结果：碰撞提示**能区分**两种情形：
  1. **契约违规** —— 某 adapter 没递增 `attempt`（键撞了，但**账可能还是对的**，
     典型如空 `group_id` 整轮合计）
  2. **账已经错了** —— 键撞上且**前一笔钱已被顶掉**（真正的资金缺口）
- 类型：文档正确性
- **不要求改触发条件**（见 AC-003）

## AC-002 · 一条失败信息订正

- `tests/test_attempt_numbering.rs:335` 的
  `llm_retry_spend_reaches_the_budget_as_two_separate_entries`
  失败信息写「账上会是 250」，**但 M3 实测是 160**
  （碰撞检测加入后**立刻停**、第二组压根没跑）
- 预期结果：消息里的数字与实际一致
- 类型：文档正确性
- **理由**：诊断信息说错会把下一个排障的人引向错误方向 ——
  250 与 160 指向完全不同的原因（前者是「两笔都记账」、后者是「撞键后停了」）

## AC-003 · ⚠️ **不许收紧触发条件**（本需求最硬的约束）

QA 明确建议：**只改文案，不要收紧触发条件。**

**理由**：收紧会放过「空 `group_id` 恒报 `attempt: 0`」那个 M3 变体 ——
而那**正是 REQ-051 要堵的唯一复发路径**。

- 预期结果：`orchestrate.rs` 的触发条件**保持合取式不变**
  （`insert` 返回 `Some(prev)` **且** `report_same_spend(...) == false`）
- 类型：架构
- **验证方式**：把 REQ-051 的三条变异（**M3** 删 `LlmAgent` 编号、
  **M3b** 删 `AcpAgent` 编号、**M4** ACP 编号 +1）**各跑一遍**，
  **必须仍然全红**（二进制 `test_attempt_numbering`，各 8 条）
- ⚠️ **本条不通过 = 本需求做错了方向**

---

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**574 passed / 0 failed / 1 ignored**（REQ-051 之后的基线，**判别力零变化 ⇒ 条数不应变**）
- **建议同会话基线对照**（基线 `a55ffad` = **574/0/1**）；耗时**别声称精度**
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns` 一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
  - 命令实属 **`ontok-propose`**；`--out` 在 `--stats` 之前；**不传 `--names`**

---

## 环境事实

- ⚠️ `.env` **在仓库根**，各 worktree 内**没有**。库名 `notol` **必须替换成 `ontol`**
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  `relative URL without a base` = **URL 取残缺**，不是环境坏了
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| `src/domain/view.rs` | **禁改** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| **`orchestrate.rs` 的触发条件** | **不许收紧**（AC-003） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req052/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
