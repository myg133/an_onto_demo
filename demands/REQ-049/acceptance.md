# 验收标准：REQ-049 preflight 提示措辞 + 表名唯一性启发

> 依据 REQ-048 验收（QA 判 D1 假信心，中等非阻断）写成。基线：`9d6c529`（develop）。
> **这是 backlog 对账后唯一「A 类 + 范围可控 + 价值明确」的干净可派项。**

---

## 背景：问题是什么（BA 与 QA 两次一致确认）

`ontok-gate-preflight` 成功时的提示原文（`src/bin/ontok-ggate-preflight.rs` 附近，**按内容 grep 定位**）：

> 「两个门禁**现在都能读到它了**」

**这句话把「读得到」说成了「门禁会过」。** QA 实测的 D1：

| 产物形态 | preflight | `drop_rate` 门禁 |
|---|---|---|
| 91 张**同名表** + 完整列 | **EXIT=0** + 「两个门禁都能读到它了」 | **EXIT=101 FAILED**（丢弃率 100.0%） |

⇒ **假信心**：preflight 说好了，门禁实际会红。

**⚠️ BA 与 QA 都明确不主张加语义判据**：91 张同名表在**形状上完全合法**
（表数/列数/内部一致性全对），门禁红是**语义**问题（抽取器对同名表产出垃圾），
属**抽取器**的事。让 preflight 去管它管不了的事，是越界。

**本需求只做两件事**：① 把提示改准确 ② 加一条**明确的启发**（不是保证）。

---

## AC-001 · 成功提示改准确（**本条是核心**）

- 预期结果：成功提示**明确区分**「门禁现在**读得到**这份文件」与
  「门禁**会不会通过**取决于内容」
- 类型：文档正确性
- **必须说清的**：
  - 读得到 ≠ 会通过
  - 下一位该做什么（跑哪两个门禁、怎么跑）
- ⚠️ **不许**把提示改成「已验证门禁会过」之类的新过度声称
  （这是本会话第 4 次过度声称的教训，见纪律备忘）

## AC-002 · 表名唯一性作为**启发**（不是保证）

- 预期结果：`validate()` 加一条「表名唯一性」检查 ——
  有重复表名时**警告但不失败**（或按你判断的方式处理）
- 类型：功能
- ⚠️ **关键限定**：这一条**不能**被表述成「有重复表名 ⇒ 门禁会红」。
  **那是过度声称** —— QA 与 BA 都没验证过重复表名**必然**导致门禁失败，
  只实测了「91 张全同名 ⇒ 一定是」。
  合理表述：「**重复表名是新抽取器可能产出垃圾的信号，值得人工核对**」

## AC-003 · 四种失败情形的判据仍成立

- 预期结果：REQ-048 已有的判别力**零退化**：
  - 正常 ⇒ EXIT=0
  - 源库不可达 ⇒ 失败 + 指引可读 + **口令不回显**
  - 产物非合法 JSON ⇒ 失败
  - 表数异常偏少 ⇒ 失败
- **验证方式**：四条各跑一遍
- 类型：回归
- ⚠️ **AC-002 若引入失败路径，必须让上面四条仍然各自可区分**

## AC-004 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**574 passed / 0 failed / 1 ignored**（本需求**不新增测试**，
  条数应**零变化**；若你新增了判据请在报告里说明并更新数字）
- **建议同会话基线对照**（基线 `9d6c529` = **574/0/1**）
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` **零改动**
  （Dev 报告的判定是「改动仅 `src/bin/ontok-gate-preflight.rs`，红线三文件 diff 为空」）
- 产物 sha **不变**（本需求不碰抽取逻辑）：
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
| **门禁的 fail-closed 语义** | **不许削弱**（读不到就失败那条不能动） |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖**（本需求专门为读它而设） |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req049/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- **唯一绝对不能碰的文件**：`/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-049 {描述} (关联: REQ-049) [session: ba-archlinux-80180-20261007-231834]`
