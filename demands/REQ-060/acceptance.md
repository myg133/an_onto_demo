# 验收标准：REQ-060 FDE 通道显示提案级 `extracted_by`

> 依据 REQ-059 验收（归属已可信化）写成。基线：`f90d537`（develop）。
> **这是 RP-2 的下半** —— 上半（可信化）已完成，本条只做「让它被看到」。

---

## 背景：为什么现在才安全

REQ-056 的 Dev 建议「先修 RP-2 再把归属接进 FDE 通道」，理由是
**「在 FDE 通道里猜错归属比不给更危险」**。BA 采纳并**把顺序反过来固化**：

- ❌ 先让 `review show` 显示 `extracted_by` ⇒ **等于把一个可能是假名的字段展示给 FDE**
- ✅ 先让归属本身可信（REQ-059，已完成）⇒ 再让 FDE 通道显示它（**本条**）

**REQ-059 已让归属可信**：产物里 `extracted_by.agent` 记的是
`factory::INPROC_EXECUTOR_ID`（**代码自报**），而混名会在**出产物前被拒**
（`executor_name_collision`）。所以 FDE 现在看到的是可信的归属。

**BA 已核实（按内容 grep，别信行号）**：

| 环节 | 实况 |
|---|---|
| `ProposalRow`（`src/db/proposal.rs`） | 有 `extracted_by: Option<Value>` —— **提案级** |
| `ProposalItemRow` | **没有** `extracted_by`（条目级无归属） |
| `review_view.rs` | `extracted_by` 出现 **0 次** —— **FDE 通道确实还没接** |

⇒ **缺口明确**：FDE 在 `review show` 里**看不到这份提案是哪个 agent 抽的**。

---

## AC-001 · `review show` 显示提案级归属

- 预期结果：FDE 在 `review show` 输出里**看得到**这份提案的抽取归属
- 类型：功能
- **判别力自证（必做，给**二进制名 + 运行条数**）**：
  - 造一份**有 `extracted_by`** 的提案 ⇒ 输出**必须含**它
  - 造一份 **`extracted_by` 为 `None`** 的提案 ⇒ 输出**必须**有明确的「无/人工修订」表述，
    **不能**打空、**不能**省略、**不能**打 `None` 字面量

## AC-002 · 措辞说清这是**提案级**

- 预期结果：输出里的措辞让 FDE 知道这是**整份提案的归属**，**不是逐条目的**
  （`ProposalItemRow` 确实没有条目级归属）
- 类型：文档正确性
- **判别力自证**：变异「把提案级归属**逐条目重复打印**」⇒ 必须被某条判据抓住
  —— 那是**误导**（会让人以为每个条目由不同 agent 抽）

## AC-003 · 归属不可信时的表现

- 预期结果：若归属字段存在但**形态可疑**（如空串、只有空白、缺 `agent`），
  **显式标注可疑**，而不是照打
- 类型：功能
- ⚠️ **理由**：REQ-057 刚证明「判据恒真」这类问题；本条**不许**写一个
  「有就打、没有就打空」然后自称安全的实现
- **判别力自证**：构造上述可疑形态各一条 ⇒ **必须**被标注

## AC-004 · 不破坏既有守门与通道

- 预期结果：
  - **客户视图的建模术语守门一条不许动**
  - FDE 通道的 `audit_fde_channel` 自检**判别力零退化**
    （它在 REQ-057 已被改成读**字段溯源**、不再依赖词表 —— **不要把它改回去**）
  - REQ-056 建的**两轨分界判据**（业务轨行 / 建模轨行可分辨）零退化
- 类型：回归
- ⚠️ **本条最可能踩的坑**：为了显示归属，把 `extracted_by` 塞进**建模轨那一行**，
  而 `sourced_fields` 判的是「有 payload 来源且非空的建模字段」⇒
  `premise_holds` 会因为这个**渲染产物**而变真 ⇒ **REQ-057 的解耦被破坏**。
  **独立核实这一点没被破坏。**

## AC-005 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**619 passed / 0 failed / 1 ignored** + 本需求新增用例数
  （**请在报告里写清最终条数**）
- **基线对照**：`f90d537` = **619/0/1**（**QA 上一轮实测过**，可作参照）
- 红线：`src/domain/view.rs`、`src/domain/review.rs`、`tests/drop_rate.rs`、
  `tests/test_fk_naming.rs` **零改动**（⚠️ 后两个在 `tests/` 下，**不在** `src/domain/`）；
  `test_humanize_still_refuses_unknown_columns` 一条断言都不许放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**（本需求只改审核呈现）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
  - 命令实属 **`ontok-propose`**；⚠️ **`--stats` 必须自带路径参数**；**不传 `--names`**

---

## 环境事实

- ⚠️ `.env` **在仓库根**，各 worktree 内**没有**。库名 `notol` **必须替换成 `ontol`**
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  `relative URL without a base` = **URL 取残缺**，不是环境坏了
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50`
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `src/domain/view.rs` | **禁改** |
| `src/domain/review.rs` | **禁改** |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| **客户视图的建模术语守门** | **一条不许动** |
| **`audit_fde_channel` 的判别力** | **零退化**（它是读**字段溯源**的，别改回读词表） |
| **两轨分界判据** | **零退化** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target
- ⚠️ **不要与另一个全量测试并发跑**
- **求和不要用 `paste -sd+`**
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- **核对文件用裸 `grep`**；**别信任何行号**（本会话已 4 处行号引用失效）
- 临时文件只放 `.review-workspace/req060/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-060 {描述} (关联: REQ-060) [session: ba-archlinux-80180-20261007-231834]`

## 交付

提交到 `feature/REQ-060`（不 push），报告里给出：
1. `review show` 现在的输出长什么样（**贴实测原文**）
2. `extracted_by` 为 `None` 与**形态可疑**时各输出什么
3. **AC-004 的判别力证据** —— 尤其**独立核实 `premise_holds` 没有因为显示归属而变真**
4. 全量数字与耗时、产物 sha（应与基线**逐字节一致**）
5. 发现但未修的问题 / 你自己引入又清掉的问题（如实报）

## ⭐ 你已四次指出 BA 的前提偏差，请继续

- REQ-054：拒绝了「提前停 ⇒ 真的出事」（**方向反了**）⇒ 成立
- REQ-055：报了 P1（**范围偏在快照侧**）⇒ 成立
- REQ-056：建议「RP-2 先修，**猜错比不给更危险**」⇒ 成立，本条是它的收尾
- REQ-059：指出 **BA 的 AC 把问题定位错了一层**（`identity_cross_check` 恒返回 `None`）⇒ 成立

**如果你发现本条 AC 本身又把问题定位错了，直接说** ——
那是本会话最有效的反馈回路。
