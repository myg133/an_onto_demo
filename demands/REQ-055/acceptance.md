# 验收标准：REQ-055 RP-1 FDE 建模轨视图

> 依据 backlog 对账（explore，2026-10-10）拆分自 REQ-035。基线：`7343bb0`（develop）。
> **本需求只做 RP-1（建模轨视图）。** RP-2 另开、RP-3 已做（见登记表）。
>
> ⚠️ 编号说明：本条原拟用 REQ-050，但该编号已被「编排层同组重试少计成本」占用，
> **故取下一个可用编号 REQ-055**，**不重编号** —— 重编号会断掉 `git log --grep` 的追溯链。

---

## 背景：为什么拆

REQ-035 把三件事记成一条（「偏差列表 RP 批次」），但对账核实它们的进度差异极大：

| 块 | 实况 |
|---|---|
| **RP-1 FDE 建模轨视图** | **未做** ← 本需求 |
| RP-2 `extracted_by` 归属修正 | **部分做**（类型/身份链都在，但 CLI 默认值与 `table_names.json` 仍是旧的）→ 另开 |
| RP-3 §4.6 占位 | **已做**（快照层/apply/落库三处都有）→ 关闭 |

⚠️ **照原登记表整条派会重复开发 RP-3。**

---

## 背景：为什么要建模轨

规格 §8.3：**客户没能力判断建模质量**（主键、基数、规范化），让他审只会随便点「通过」；
拆成两轨 —— 业务轨（客户审语义）＋ **建模轨（FDE 审建模质量）**。

对账核实：**`Track::Biz` / `Track::Model` 枚举与按轨分发的状态机已存在**
（`src/domain/review.rs`，**按内容 grep 定位**），
**但只有客户视图一个投影，建模轨没有对应的视图可看** ——
`review.rs` 让 FDE 逐项裁决「建模轨」，**FDE 却拿不到建模轨的数据**。

**这不是判据问题，是产品缺一环。**

---

## 数据已齐备（BA 已逐项核实，**你不需要先去补数据**）

建模轨要保留的东西，**全部已在快照层**：

| 建模维度 | 数据来源 |
|---|---|
| 标识符 / 系统 key | `EntityTypeView.key` |
| 证据 | `EntityTypeView.evidence` |
| 父类 | `EntityTypeView.parent` |
| 不相交 | `EntityTypeView.disjoint_with` |
| 列映射（含 `pk`/`cardinality`/`ordinal` 那些建模术语）| `PropertyTypeView.column` |
| 关系基数 / 语义 | `LinkTypeView.{from_type,to_type,cardinality,semantic}` |
| 抽取归属 | 快照层 `extracted_by` |
| ActionType / Function | 快照层 `action_types` / `functions` |

**客户视图 `project()` 刻意丢弃的清单，正好就是建模轨要保留的**
（`view.rs` 约 752-760 行的文档逐条注明：丢弃 `key`/`evidence`/`ordinal`/
`extracted_by`/`action_types`/`functions`/`disjoint_with`，每一条都是「建模术语或内部实现」）。

⇒ **你要做的是「再加一个投影」，不是「补数据」。**

---

## ⚠️ 最硬的约束：建模轨绝不能混进客户视图

本仓有一道**建模术语黑名单守门**（`src/domain/view.rs` 的 `MODELING_TERMS`，
含 `pk`/`primary_key`/`cardinality`/`ordinal`/`evidence`/`disjoint`/`key` 与中文
`外键`/`基数`/`主键`/`证据`）。

**BA 已核实两件事**（按内容 grep，别信行号）：

1. 守门是**运行时**扫**客户视图的输出**，**不是扫源码**
   ⇒ **你写建模轨代码时可以出现 `primary_key`/`cardinality` 等词，不会误伤**
2. 但 **`tests/` 下也有判据会扫这些词**（如 `test_mcp_server.rs` 有多条
   「建模术语必须被守门器拒绝」「错误文案不得原样回显建模术语」
   「`instructions` 不得把项目名里的建模术语送进 LLM 系统提示」）
   ⇒ **新代码不得让这些词出现在**客户视图 / MCP 返回值 / 错误文案 / LLM 系统提示里

**所以**：建模轨是一个**独立输出**，走独立通道。
⚠️ **别为了让建模轨好读而改客户视图的字段**；
⚠️ 也**别把建模术语加进 `MODELING_TERMS` 之外的地方**。

---

## AC-001 · 建模轨投影存在且与客户视图分离

- 预期结果：有一个**独立**的建模轨投影（函数名由你定），输出 FDE 需要审的建模信息
- 类型：功能
- **判别力自证（必做）**：
  - 构造一份**含建模术语**的快照 ⇒ 建模轨输出**必须含**它们
  - 同一份快照 ⇒ **客户视图输出必须不含**它们
  - 两条各给**二进制名 + 运行条数**
  - **两条必须同时成立** —— 只验一条不构成「分离」的证据
- ⚠️ **编译失败不算变异执行**

## AC-002 · 不破坏既有守门

- 预期结果：REQ-041~054 期间加的判据**零退化**，
  尤其**建模术语黑名单的运行时守门**一条不许放宽
- **验证方式**：全量 + 独立跑一遍既有守门相关的测试二进制

## AC-003 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**574 passed / 0 failed / 1 ignored** + 本需求新增用例数
  （请在报告里写清最终条数；若条数变了，**知会 BA** 以便同步所有引用 574 的文档）
- **建议同会话基线对照**（基线 `7343bb0` = **574/0/1**）
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、**`src/domain/view.rs`** 零改动
  —— ⚠️ 建模轨投影**不许写进 `view.rs`**（那正是黑名单所在处）。**放新文件。**
- 产物 sha **不变**（本需求只加投影，不碰抽取逻辑）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
  - 命令实属 **`ontok-propose`**；⚠️ **`--stats` 必须自带路径参数**
    （本会话此前多处写成「`--out X --stats`」是**错的**，会报 `Error: --stats 需要一个参数`）
  - **不传 `--names`**

---

## 环境事实

- ⚠️ `.env` **在仓库根**，各 worktree 内**没有**。库名 `notol` **必须替换成 `ontol`**
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  `relative URL without a base` = **URL 取残缺**，不是环境坏了（本会话已两轮在这里栽过）
- 共享本体库**只读**（仅 `information_schema` / `pg_catalog`，零业务数据行）；
  **禁 DELETE / TRUNCATE / 清空表**
- 本机**没有 `psql`、没有 `psycopg`** ⇒ **测不了 `SELECT 1` p50**
- 报告把「代码失败」与「环境类失败」**分开写**；复现率随负载浮动，**不声称稳定值**
  （已知 `Connection reset by peer` 是环境类，隔离复跑通常过）
- 口令只走环境变量，**绝不写进代码、文件、日志、报告或 commit**；
  ⚠️ **不要 `echo` 连接串**（本会话已有 Dev 与 QA 各踩过一次口令回显的坑）

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `src/domain/view.rs` | **禁改**（黑名单所在处；建模轨请放新文件） |
| `tests/drop_rate.rs` | **禁改** |
| `tests/test_fk_naming.rs` | **禁改** |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| **客户视图的建模术语守门** | **不许放宽** |
| `test_backfill_…` | **不许放松任何一条断言** |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖** |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- **核对文件用裸 `grep`**；**别信任何行号**（本会话已 4 处行号引用失效）
- 临时文件只放 `.review-workspace/req055/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
- commit 格式：`[Dev] REQ-055 {描述} (关联: REQ-055) [session: ba-archlinux-80180-20261007-231834]`

## 交付

提交到 `feature/REQ-055`（不 push），报告里给出：
1. 建模轨投影放在哪、输出什么、**与客户视图怎么保证分离**
2. **AC-001 的两条判别力证据**（建模轨含建模术语 / 客户视图不含），各给二进制名 + 运行条数
3. AC-002 既有守门零退化的证据
4. 全量数字与耗时、产物 sha
5. 发现但未修的问题 / 你自己引入又清掉的问题（如实报）

诚实优先于好看。**如果你判断「建模轨该是 CLI 子命令、而不该是库函数」或「该先做 RP-2」，
直说** —— BA 已在连续两条里被指出过「登记/规格与实况脱节」。
