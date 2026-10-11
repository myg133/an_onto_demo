# 验收标准：REQ-061 夹具假注释 + 判据③补一条规则 + 终端观感

> 依据 REQ-060 验收（QA 判 PASS 带 3 项非阻断）写成。基线：`ca8cdc7`（develop）。
> **本需求只做四件小事。** BA 明确**不做**「判据升级到解析层」—— 见文末「BA 的范围判断」。

---

## AC-001 · 修 F-1：夹具注释断言的**假事实**

`tests/test_review_provenance.rs:92-95` 的注释称
「REQ-059 之后产物里**真实出现**的那份归属」，取值却是 `mcode-agent` / `2026.10.07`。

**那是 REQ-059 移除的硬编码占位，今天的 `ontok-propose` 产不出来。**
真实产物是 `extracted_by.agent = "inproc:ontok::agent::RuleBasedAgent"`、
`version = "0.1.0"`（**QA 实测**，BA 亦独立复现）。

⇒ 「**JSONB 往返证明真实形状**」这个说法**名不副实**（机制对任意非空串都成立）。

- 预期结果：注释与**真实产物取值**一致；或注释**明说这是人为构造的夹具**（不声称是真实产物）
- 类型：文档正确性
- ⚠️ **不要**为了「让注释为真」而把夹具改成硬编码真实值 ——
  **夹具该用什么值由它测什么决定**，注释只需**如实说明**。
- **判别力自证**：加一条测试断言「注释里出现的 agent 取值」与
  `factory::INPROC_EXECUTOR_ID`（或产物实测值）**一致**，
  否则注释再次腐烂时**测试会响**（**这是本条真正的价值：让注释腐烂变成可检测的**）

## AC-002 · 判据③补「不得是条目头行」一条规则（F-2 / 存活变异 M2b）

QA 实测的存活变异：**归属只拼进第 1 个条目头行**（` · ` 分隔）⇒ 44 条全绿。
判据③规则②只查「含不含两轨前缀」+「行号是否晚于第一个条目行」，**条目头行两条都躲过**。

- 预期结果：判据③**多一条**规则：归属行**不得**是条目头行形态
- 类型：测试有效性
- **判别力自证**：复现 **M2b** ⇒ **必须变红**（二进制 `test_review_provenance`）

## AC-003 · 修 F-3：终端输出里的 Markdown 星号

`src/domain/review_view.rs:936` 的 `**不是**` 用在**纯文本终端输出**里，星号会原样显示。

- 预期结果：终端输出的措辞**不含** Markdown 标记
- 类型：文档正确性
- ⚠️ **注意别把注释里的 Markdown 一起去掉** —— 注释里的是**合适**的

---

## ⛔ BA 的范围判断：为什么**不**做「判据升级到解析层」

QA 建议把「标注注释不含 `extracted_by`」这条**从字面层升级到解析层**（从 rendered 文本
**反解**结构：行号 / 归属 / 条目头 / 两轨行）。**BA 明确不做**，理由：

1. **成本高**：要写一个 parser 从终端文本反解结构，而那文本是**给人看的**
   —— 格式一变 parser 就脆。
2. **收益递减**：它能多抓的只是「把 `extracted_by` 的**值**（而非标签）塞进条目行」
   —— 一种**新的**绕过形态，不是当前**任何一个存活变异的成因**。
3. **无底洞警告**：本会话在 REQ-044 阶段已经因为「判据加补丁」连开五轮，
   每一轮都能找到新绕过，收益递减明显。**这一轮只补「恰好堵住已知存活变异」的那一条**。
4. **覆盖洞与解析洞要分清**：本会话反复出现的病是「**判据恒真**」与「**注释腐烂**」，
   **不是**「判据不够解析」。AC-001 让注释腐烂可检测，正中这个病的靶心。

---

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**637 passed / 0 failed / 1 ignored** + 本需求新增用例数（**请写清最终条数**）
- **基线对照**：`ca8cdc7` = **637/0/1**（**QA 上一轮自跑实测过**，可作参照）
- 红线：`src/domain/view.rs`、`src/domain/review.rs`、`tests/drop_rate.rs`、
  `tests/test_fk_naming.rs` **零改动**（⚠️ 后两个在 `tests/` 下，**不在** `src/domain/`）
- 产物 sha **不变**（本需求只改注释、判据、措辞）：
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
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
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
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| **判据升级到解析层** | **本需求明确不做**（见「BA 的范围判断」） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target
- ⚠️ **不要与另一个全量测试并发跑**，也**绝不在全量运行期间改源码**（上一轮 Dev 踩过）
- **求和不要用 `paste -sd+`**
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- **核对文件用裸 `grep`**；**别信任何行号**（本会话已 4 处行号引用失效）
- 临时文件只放 `.review-workspace/req061/`；**禁 `/tmp`、禁 home**
  （⚠️ worktree 内 `.review-workspace/` **不被 `.gitignore` 忽略** ——
  上一轮 QA 因此把报告写到了**仓库根**；你照此办理）
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- **唯一绝对不能碰的文件**：`/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-061 {描述} (关联: REQ-061) [session: ba-archlinux-80180-20261007-231834]`

## 交付

提交到 `feature/REQ-061`（不 push），报告里给出：
1. AC-001 的注释改成了什么，**那条「注释腐烂可检测」的判据**怎么写的
2. **M2b 复现的证据**（修复前存活 / 修复后变红），各给二进制名 + 运行条数
3. F-3 的措辞改成什么
4. 全量数字与耗时、产物 sha（应与基线**逐字节一致**）
5. 发现但未修的问题 / 你自己引入又清掉的问题（如实报）

## ⭐ 你已多次指出 BA 的问题，请继续

- REQ-054：拒绝了「提前停 ⇒ 真的出事」（**方向反了**）⇒ 成立
- REQ-055：报了 P1（**范围偏在快照侧**）⇒ 成立
- REQ-056：建议「RP-2 先修，**猜错比不给更危险**」⇒ 成立
- REQ-059：指出 **BA 的 AC 把问题定位错了一层** ⇒ 成立
- REQ-060：指出 **AC-001 把两种 `None` 当一种** ⇒ 成立，BA 已立规

**如果你认为本条的范围切错了（比如该把解析层升级一起做，或反过来还有该修的），直接说。**
