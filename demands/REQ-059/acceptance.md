# 验收标准：REQ-059 `extracted_by` 归属可信化（RP-2 上半）

> 依据 REQ-056 Dev 的建议（「在 FDE 通道里猜错归属比不给更危险」）+
> BA 逐行核实（`orchestrate.rs:29/:98` 两处自认「对不上号」）写成。
> 基线：`80464a9`（develop）。
>
> **本需求只做「让归属可信」。让 FDE 通道显示归属是下一条**（见文末「为什么分两步」）。

---

## 背景：归属字段现在可能是**编造的名字**

BA 实测与核实链（**按内容 grep，别信行号**）：

| 环节 | 实况 |
|---|---|
| `ontok-propose.rs` 的 `--agent` 默认值 | **硬编码 `"mcode-agent"`** |
| 真实执行的 agent | `inproc`（REQ-013 建的进程内规则实现） |
| `ontok-propose.rs` 末尾 | **确实调了** `identity_cross_check`，但只 `eprintln!("⚠ {warn}")` —— **只是警告，不报错** |
| `orchestrate.rs:29` / `:98` 的文档 | 两处都写着 `extracted_by.agent = 调用方登记的那个 agent` **← 对不上号** |
| 实跑输出第一行 | `翻译表：mcode-agent (MiniMax Code) v1.2.0` —— 这是**翻译表来源**，而 `--agent` 默认值**恰好同名** |

**两个后果**：

1. **同名混淆** —— 「翻译表来源叫 mcode-agent」与「agent 名叫 mcode-agent」是两回事，
   却在输出里长得一样
2. **产物里可能躺着一个假名** —— `extracted_by.agent` 记的是**调用方登记的值**，
   而那个值在默认路径下是硬编码占位，**不是真实执行者**

⇒ **归属字段当前不可信。**

---

## 为什么分两步（BA 的判断）

REQ-056 的 Dev 建议「先修 RP-2 再把归属接进 FDE 通道」，理由是「猜错比不给更危险」。

**BA 认同，并据此把顺序反过来固化**：

- ❌ 先让 `review show` 显示 `extracted_by` ⇒ **等于把一个可能是假名的字段展示给 FDE**
  —— **那正是 Dev 要避免的危险**
- ✅ 先让归属本身可信 ⇒ 再让 FDE 通道显示它

⇒ **本需求只做可信化，不碰 FDE 通道。**

---

## AC-001 · 默认身份不许是编造的占位

- 预期结果：默认路径下 `extracted_by.agent` **就是真实执行者的身份**，
  不是一个与真实实现无关的硬编码串
- 类型：功能
- **判别力自证（必做，给**二进制名 + 运行条数**）**：
  - 跑一次**不带 `--agent`** 的抽取 ⇒ 检查产物里 `extracted_by.agent` 的取值
  - 跑一次**带 `--agent`** 的抽取 ⇒ 检查它是否按显式值记
  - 两条都要**逐字断言**，不要只断言「非空」
- ⚠️ **编译失败不算变异执行**

## AC-002 · 身份对不上必须**报错**而非警告

- 预期结果：`identity_cross_check` 不一致时，**要么**让 `extracted_by` 记真实身份，
  **要么**拒绝出产物 —— **不接受「只警告、产物照出」**
- 类型：架构
- **判别力自证**：构造一个「`--agent` 声明与实际实现不符」的输入
  （如 `--agent 虚构名` 配 `inproc`）⇒ **必须红**，且报错文案说清「哪里对不上」
- ⚠️ **理由**：`eprintln!` 之后照常出产物 = **让人看见但不阻止**，
  而这个字段进的是**版本快照**（§8.4 要防的正是版本漂移）

## AC-003 · 翻译表来源与 agent 名不再同名混淆

- 预期结果：输出里**「翻译表来源」与「抽取 agent」** 是**两件可分辨的事**
- 类型：文档正确性
- **判别力自证**：造一个「翻译表来源名 == agent 名」的输入
  ⇒ 判据必须**抓住这个歧义**（而不是恰好同名时看不出问题）

## AC-004 · 不改既有内容语义

- 预期结果：`ExtractedBy` 的**字段结构不变**（ADR-001/ADR-003 定过），
  `extracted_by IS NULL = 人工修订` 语义不变
- 类型：回归

## AC-005 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**610 passed / 0 failed / 1 ignored**（本需求会新增用例，**请写清最终条数**）
- **基线对照**：`80464a9` = **610/0/1**（**QA 上一轮实测过**，可作参照）
- 红线：`src/domain/view.rs`、`src/domain/review.rs`、`tests/drop_rate.rs`、
  `tests/test_fk_naming.rs` **零改动**；
  `test_humanize_still_refuses_unknown_columns` 一条断言都不许放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**（本需求只改「记什么值」，不改抽取逻辑）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
  - 命令实属 **`ontok-propose`**；⚠️ **`--stats` 必须自带路径参数**；**不传 `--names`**
  - ⚠️ **若你的修法真的改变了产物内容**（`extracted_by` 变了 ⇒ sha 就会变），
    **必须**说明哪些字段变、为什么变，并**知会 BA 同步所有引用这两个 sha 的文档**

---

## ⚠️ 2026-10-09 流程修正（QA 提出，BA 认领）：4 条既有测试的前提作废

本文件 AC-002 写「`identity_cross_check` 不一致时**要么报错、要么拒绝出产物**」，
但**没点名** Dev 可以改哪几条既有断言。结果 Dev 改了 **4 条既有测试的前提**而本文件**没有登记** ——
与 REQ-057 那次同类漏记。**BA 现在认领如下事实**（QA 已双向实证）：

| 既有测试 | 原前提 | 现状 |
| --- | --- | --- |
| `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`<br>`test_cli_default_and_registry_paths_remain_byte_identical`<br>`test_cli_registry_path_is_byte_identical_when_registry_says_the_same` | 「默认路径 ≡ registry 路径」 | **该前提本身就是缺陷** —— 两者相等**只因登记时照着占位抄了**。QA 实测：基线 `80464a9` 的 610/0/1 中这三条原始断言**全部 ok**（前提当时成立）；HEAD 的 `assert_ne!` 通过（前提已被本修复打破）⇒ **收窄是被逼的，不是机会主义**。已收窄为「`items`/轮次/失败组逐字节一致」+「归属两栏**必须可分辨**」，并**新增正向断言**，净效果更严 |
| `test_the_production_entry_still_reports_the_identity_fork` | 断言「必须**打出**那处告警」 | 翻成「必须**拦住**」。⚠️ **函数名现在名实相反**（名说 reports、实则断言拦住）—— QA 记为遗留项 |

**立规（同时写入纪律备忘）**：改既有测试的前提**不应由 Dev 或 QA 单方决定**。
流程固定为：**Dev 写明理由 → QA 独立复现「前提在基线成立、在 HEAD 不成立」→ BA 认领并写回 acceptance.md**。
三步缺一不可 —— 本次前两步都做了，**第三步（BA 认领）当时漏了，现在补上**。

## 环境事实

- ⚠️ `.env` **在仓库根**，各 worktree 内**没有**。库名 `notol` **必须替换成 `ontol`**
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  `relative URL without a base` = **URL 取残缺**，不是环境坏了
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `src/domain/view.rs` | **禁改** |
| `src/domain/review.rs` | **禁改** |
| `tests/drop_rate.rs` | **禁改**（⚠️ 在 `tests/` 下，**不在** `src/domain/`） |
| `tests/test_fk_naming.rs` | **禁改**（同上） |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| **ADR-001 / ADR-003 定过的 `ExtractedBy` 语义** | **不许改**（含 `IS NULL = 人工修订`） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target
- ⚠️ **不要与另一个全量测试并发跑**
- **求和不要用 `paste -sd+`**
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- **核对文件用裸 `grep`**；**别信任何行号**（本会话已 4 处行号引用失效）
- 临时文件只放 `.review-workspace/req059/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-059 {描述} (关联: REQ-059) [session: ba-archlinux-80180-20261007-231834]`

## 交付

提交到 `feature/REQ-059`（不 push），报告里给出：
1. **默认路径下 `extracted_by.agent` 现在记什么**，附**实跑证据**
2. **AC-002 的判别力证据**（身份对不上 ⇒ 必须红）
3. 翻译表来源与 agent 名的**可分辨性**怎么做的
4. 全量数字与耗时、产物 sha（**若 sha 变了，说明变了哪些字段、为什么**）
5. 发现但未修的问题 / 你自己引入又清掉的问题（如实报）

## ⭐ 你连续三次指出过 BA 的前提偏差，请继续

- REQ-054：你拒绝了「提前停 ⇒ 真的出事」（方向反了）⇒ **成立**
- REQ-055：你报了 P1（范围偏在快照侧）⇒ **成立**
- REQ-056：你建议「RP-2 先修，猜错比不给更危险」⇒ **成立，且本需求就是它的下半**

**如果你发现本条 AC 本身就把问题定位错了，直接说** ——
那是本会话最有效的反馈回路。