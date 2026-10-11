# 验收标准：REQ-054 把「累计」拆成两种口径

> 依据 REQ-053 验收（QA 判 PASS 带非阻断）写成。基线：`749c1c1`（develop）。
>
> **这是「诊断文案过度声称」同类第 3 次**。前两次：① REQ-051（空 `group_id` 会反复计入 —— 证伪）
> ② REQ-052（根源在 BA 的 AC 原文，Dev 忠实实现 —— 证伪）。
> **本轮的教训：把断言降级成条件句还不够，条件本身必须穷尽。**

---

## 背景：Dev 已经按上一轮规约降级了，但分叉不穷尽

REQ-053 的文案现在是：「账对不对**取决于该组报的是累计还是增量**」——
累计 ⇒ 账对；增量 ⇒ 少计、真金缺口。**降级本身做对了。**

**但「累计」这个分叉不穷尽。** QA 实测**情形 4**：

| 步骤 | 账本 | 真实花费 | 差异 |
|---|---|---|---|
| 第 1 组花 100 入账 | 100 | 100 | 0 |
| 第 2 组报**整轮/会话口径**累计 220 → 380，撞键 | **480** | **380** | **+100 多计** |

文案说「账是对的」—— **仍是假**。

**QA 补充了一条比 BA 上轮描述更严重的判断**：
**单轮内就能触发**（前一组先入账、后一组报累计并撞键 ⇒ 把前一组的钱再算一遍），
**生产可达** —— `src/bin/ontok-propose.rs:243` 一次 `orchestrate` 编排 **91 组**。

**为什么仍判非阻断**：碰撞已让本轮 `stopped`、闸门**未被求值** ⇒ **不多花钱**，
危害限于**诊断误导**。

---

## AC-001 · 把「累计」拆成两种口径

- 预期结果：文案区分
  - **该组口径累计**（本组从 120 累到 280）⇒ **账对**（契约破）
  - **整轮 / 会话口径累计**（本组报的是含别的组的总额，220 → 380）⇒ **多计**、
    闸门可能提前停 ⇒ 真的出事
- 类型：文档正确性
- **行动项要写成可执行的**：「核对累计的**口径**（本组口径 vs 整轮/会话口径），
  以及它是累计还是增量」—— 而不只是「核对累计还是增量」

## AC-002 · ⚠️ 条件句的条件必须穷尽且可分辨

**这一条是本需求真正的验收点**（也是那条升级后的规约）：

- 写完条件句后，**自己列出**「有没有第三种可能」
- 对每个可能：**要么文案覆盖、要么明确说明为什么不覆盖**
- ⚠️ **本轮已知的第三种**（QA 上一轮点名、仍未覆盖）：
  **跨 round / 会话累计 ⇒ 多计**。上一轮的情形 4 就是它的一个变体（单轮内可达）
- 另有一个**既有盲区**（非本需求引入，如实记即可）：
  **重试恰好花同样的钱** ⇒ `report_same_spend` 判同、不渲染文案 ⇒ 静默少计、零诊断。
  你可以选择覆盖它，也可以说明为什么难覆盖

## AC-003 · ⚠️ 触发条件仍不许动

- 预期结果：触发块四行（`let bumped = self` … `if let Some(prev)`）
  按**内容**抽出的 sha256 保持 **`0381c3a0ff649648`**
- **验证方式**：三条变异**各跑一遍，每轮必须为红**
  （二进制 `test_attempt_numbering`，各 8 条）：
  - **M3** 删 `LlmAgent` 编号 `src/agent/llm/mod.rs:432`
  - **M3b** 删 `AcpAgent` 编号 `src/agent/acp/mod.rs:503`
  - **M4** ACP 编号 `+1`
  - 预期 M3 / M3b 各 **2 条红**（只红改动侧）、M4 **1 条红**
- ⚠️ **编译失败不算变异执行**；**本条不通过 = 做错了方向**

## AC-004 · 顺带做掉：行号锚定换成符号名（**期望做**）

上一轮 Dev 把 AC-002 注释里的行号引用改成「按断言内容指代」，**但那个内容在文件里出现两次**、
靠「下面那条」消歧 —— 锚点**不唯一**。

- 预期结果：改用**符号名**锚定（QA 建议 `const EXPECTED_USED: u64 = 370;`），
  **单一真相源**，行号漂移与代码重排都不怕
- 类型：文档正确性
- 顺带把**本文件里其余的行号引用**一并检查（`orchestrate.rs:xxx` 这类）

---

## 其余验收

- **全量**：至少 1 轮 `cargo test --no-fail-fast`，预期 **574 passed / 0 failed / 1 ignored**
  （条数**零变化**，本需求只改文案与注释）
  - **建议同会话基线对照**（基线 `749c1c1` = **574/0/1**）
- **红线**：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs:1254`）一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
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
| **`orchestrate.rs` 的触发条件** | **不许动**（AC-003） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req054/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-054 {描述} (关联: REQ-054) [session: ba-archlinux-80180-20261007-231834]`
  ⚠️ **session ID 照抄上面那串**（commit 的**追溯主依据**，本会话错过多次）
