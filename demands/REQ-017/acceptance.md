# 验收标准：REQ-017 `links_dangling` 受限作用域地雷

> 依据 REQ-014 验收带出写成。基线：`9d6c529`（develop）。
> **这是真缺陷，不是判据问题 —— 它会直接让客户拿不到提案。**

---

## 背景：地雷是什么（BA 已逐行核实）

三处代码摆在一起，构成一颗**已装填**的地雷：

| 位置 | 内容 |
| --- | --- |
| `src/agent/rule_based.rs:264` | 注释明说：会把「**终点实体不在本组**」的关系**全部删掉**（记进 `links_dangling`）|
| `src/bin/ontok-propose.rs:429-430` | `if s.links_dangling > 0 { bail!("提案含 {} 条悬空关系，AC-007 会在审核期拦截") }` |
| `src/agent/assemble.rs:419` | 注释：悬空关系会进 `stats.links_dangling`，**而 CLI 见到它非 0 就拒绝出提案** |

**触发条件**：抽取是**一组一表**（`derive_group_id` = `table:{name}`），
所以「终点实体不在本组」在受限作用域下是**常态而非异常**
—— 每一条跨表关系，在抽它起点那一组时终点都还不在本组。

⇒ **`links_dangling > 0` 几乎必然成立 ⇒ CLI 整轮拒绝出提案。**

**这一条会直接卡住客户交付**（拿不到提案），也卡住 REQ-035/036/037 那些要跑通抽取的需求。

⚠️ BA 核实过登记表里写的 `ontok-propose.rs:175` **行号已漂**（那里现在是别的代码），
真实位置是 **`:429-430`**。**请按内容定位，不要信行号。**

---

## AC-001 · 区分「真悬空」与「跨组未解析」

- 预期结果：受限作用域下，「终点实体不在本组」**不再**被算作 dangling；
  真正悬空（终点**确实不存在**）仍被计入并仍触发 bail
- 类型：功能
- **判别力自证（必做）**：
  - 构造「跨表关系」（终点在本轮另一组里）⇒ **不得**被判 dangling
  - 构造「真悬空」（终点在哪都不存在）⇒ **必须**被判 dangling 且 CLI 仍 bail
  - 两条各给**二进制名 + 运行条数**
- ⚠️ **编译失败不算变异执行**

## AC-002 · 说不清就直说：三个方向由你判断

候选（**不强制**，你判断）：

- **① 语义拆分**：`links_dangling` 拆成「**真悬空**」与「**跨组待解析**」两个计数，
  bail 只看前者
- **② 延后解析**：分批抽取时把跨组关系**攒起来**，等所有组抽完再统一解析/归集
- **③ CLI 侧放宽**：`ontok-propose` 对受限作用域下的 dangling 改警告不 bail

**必须说明**：选了哪个、**为什么**、**有没有代价**。
⚠️ **特别问 ①**：如果同一组被抽了两次（`drive_group` 的重试），
第一条里的「真悬空」判定会不会重复计？或反过来漏计？

## AC-003 · 不要用「放宽判据」掩盖

- 预期结果：**不许**为了跑通而把 `bail` 的阈值改大、或把 dangling 检查整条删掉
- 类型：架构
- **理由**：AC-007 的存在意义就是「不让带着断链的提案流到审核期」。
  **绕过它就是绕过需求本身。**

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**574 passed / 0 failed / 1 ignored**（本需求会新增用例）
- **建议同会话基线对照**（基线 `9d6c529` = **574/0/1**）
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns` 一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**（本需求不碰抽取**逻辑**，只碰 dangling 的**计数口径**）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
  - 命令实属 **`ontok-propose`**；`--out` 在 `--stats` 之前；**不传 `--names`**
  - ⚠️ **若你的修法会改抽取结果本身**（而不仅是计数口径），
    **产物 sha 会变** ⇒ 那时**必须**在报告里说明为什么变，并**知会 BA**

---

## 环境事实

- ⚠️ `.env` **在仓库根**，各 worktree 内**没有**。库名 `notol` **必须替换成 `ontol`**
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  `relative URL without a base` = **URL 取残缺**，不是环境坏了
- 共享本体库**只读**（仅 `information_schema` / `pg_catalog`，零业务数据行）；
  **禁 DELETE / TRUNCATE / 清空表**
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
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
| **AC-007 的 bail** | **不许用放宽阈值的方式绕过** |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req017/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- **唯一绝对不能碰的文件**：`/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-017 {描述} (关联: REQ-017) [session: ba-archlinux-80180-20261007-231834]`
