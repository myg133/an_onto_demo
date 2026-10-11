# 验收标准：REQ-053 文案降级为条件句 + 修一处陈旧数字

> 依据 REQ-052 验收（QA 判 PASS 带 2 项非阻断）写成。基线：`be34129`（develop）。
>
> **本条的根源在 BA**：REQ-052 的 AC-001 原文就写了过度声称的话，Dev 忠实实现。
> **本需求是来修我的规格错误的，不是来修 Dev 的实现的。**

---

## AC-001 · 非空分支文案降级为条件句（F-1）

`orchestrate.rs:514-516` 当前断言：

> 「**账已经错了**」「used **少计**」「**真的资金缺口**」

**QA 构造反例并实测复现三处全为假**：
非空 `group_id` + `attempt` 恒 0 + 报**组累计**（120→280）
⇒ `used` 与真实花费**相等** —— 账**没错**、**没少计**、**没有资金缺口**。
且**可达**（两个独立缺陷叠加即可命中）。

- 预期结果：文案**不再断言**「已经错了 / 少计 / 真金缺口」，
  改为**条件句**：说明「键撞上说明 `attempt` 递增有问题」，
  并**告诉读的人该核对什么**（例如「该组上报的是累计还是增量」）
- 类型：文档正确性
- **判别力零变化**（本需求只改文案与注释）

## AC-002 · 修 `:322` 的陈旧数字（F-2）

`tests/test_attempt_numbering.rs:322` 的注释写「账上是 280，而不是 160」，
**而同一测试 `:333` 断言的是 370**，且是**两组**测试。
**注释与断言自相矛盾，且注释是错的**（基线即错，REQ-052 漏改）。

- 预期结果：注释里的数字与**同测试的实际断言一致**
- 类型：文档正确性
- ⚠️ 上一位 Dev 给的理由是「单组测试，停不停都是 160」—— **该理由不成立**：
  这个测试是**两组**，不停时是 370。理由错 ≠ 结论错，但**理由也要改对**。

## AC-003 · ⚠️ 触发条件仍不许动

- 预期结果：REQ-052 已证明触发块逐字节未改
  （`let bumped = self` … `if let Some(prev)` 四行，sha256 `0381c3a0ff649648…` 两侧一致）。
  **本需求继续保持** —— 只改文案与注释
- 类型：架构
- **验证方式**：三条变异（**M3** 删 `LlmAgent` 编号 `src/agent/llm/mod.rs:432`、
  **M3b** 删 `AcpAgent` 编号 `src/agent/acp/mod.rs:503`、
  **M4** ACP 编号 `+1`）**各跑一遍，每一轮都必须为红**
  （二进制 `test_attempt_numbering`，各 8 条）
- ⚠️ **本条不通过 = 做错了方向**

> ⚠️ **2026-10-10 口径订正（BA 采纳 Dev 的意见）**：原文写「各 8 条…全红」，
> **字面物理上不可能** —— M3 只改 LLM，若 ACP 的 6 条也变红，
> 说明两条链**耦合错了**（那是更严重的问题，不是本条该抓的）。
> **正确口径：「该轮必须为红（变异被杀死）+ 报告红了几条」**。
> 预期：M3 / M3b 各 **2 条红**（只红改动侧）、M4 **1 条红**
> （`+1` 后键仍互异 ⇒ 不撞键、账仍 370 ⇒ 只有精确编号断言能抓住它）。

---

## 验收标准（其余）

- **全量**：至少 1 轮 `cargo test --no-fail-fast`，
  预期 **574 passed / 0 failed / 1 ignored**（条数**零变化**）
  - **建议同会话基线对照**
- **红线**：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs:1254`）一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
  （注意：改**注释**不算删测试；但若为改注释顺手删了断言，**本条不通过**）
- **产物 sha 不变**：
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
  （本会话已有两轮在这里栽过，其中一轮误报「121 failed 环境不可达」）
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
  （注：HEAD 那轮有 1 条 `test_project.rs:143` `ConnectionReset`，
  隔离重跑 3/3 全绿 ⇒ 环境类，不声称稳定复现率）
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**（本会话已有 Dev 与 QA 各踩过一次口令回显的坑）

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| `src/domain/view.rs` | **禁改** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| **`orchestrate.rs` 的触发条件** | **不许动**（AC-003） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req053/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
