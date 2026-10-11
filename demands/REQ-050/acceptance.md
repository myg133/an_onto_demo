# 验收标准：REQ-050 编排层同组重试少计成本

> 依据 REQ-034 验收（QA 探针实测）写成。基线：`c082556`（develop）。
>
> **根因在编排层，REQ-034 没碰它** —— 但 LLM 路径把它从边角变成了常态。

---

## 背景：BA 已核实的事实（不照抄 QA 描述）

`src/agent/orchestrate.rs`：

| 位置 | 内容 |
| --- | --- |
| `:367` | `latest_by_group: BTreeMap<String, (u64, Option<MoneyAmount>)>` |
| `:381-383` | `absorb` 里 `self.latest_by_group.insert(r.group_id.clone(), (r.used, r.cost.clone()))` —— **HashMap 覆盖语义** |
| `:401-404` | `totals()` 遍历 `latest_by_group.values()` 求和 |
| `:701` | `drive_group` 每次尝试都用 `group.group_id.clone()`（**同一个 key**） |

⇒ **同 `group_id` 多次 `absorb` ⇒ 只有最后一次的 `used` 保留 ⇒ 前面的静默丢失。**

**关键**：`latest_by_group` 全仓**只有 2 个触点**（`absorb` 写、`totals` 读）——
**没有「展示每组最终用量」的消费者**。所以「latest」这个名字
并没有它看起来承担的职责。

---

## ⚠️ 为什么这条不是「记账瑕疵」而是**资金风险**

`BudgetReport.used` / `cost` 是**整轮累计**，用于两道闸门判定
（`:388` `limit.exceeded_by(used, self.cost)`）。

**少计 ⇒ 费用闸按偏低的数字判定 ⇒ 放过真实超额。**
这不是「显示不准」，是**闸门基于错数放行**。

---

## 三个候选修法（Dev 判断取舍，可组合）

| # | 修法 | 改动面 | 取舍 |
| --- | --- | --- | --- |
| ① | `absorb` 改**累加**语义 | 只改 `:381-383` + 字段名/文档 | 最直观。⚠️ 但 `latest_by_group` 名字与语义会不符，需一并改名 |
| ② | `drive_group` 重试时用**带轮次的 `group_id`** | 改 `:701` 一处 | 对数据模型零改动。⚠️ 但 `group_id` 会**泄漏到报告/提案**里（`failed_groups`、`usage_reports`），下游可能认它 |
| ③ | 每次尝试单独记账、编排层汇总 | 改记账点 | 最啰嗦，但语义最清晰 |

**由 Dev 判断**，但必须说明：
- 选了哪个、**为什么**
- 有没有能让「两条腿都记账但一条被吃掉」复发的路径
- 若选 ②，**核实 `group_id` 泄漏到哪些下游**

---

## AC-001 · 重试的花费全部进账

- 预期结果：同组重试 N 次 ⇒ `BudgetReport` 计**全部 N 次**的 `used` 与 `cost`
- 类型：功能
- **判别力自证（必做）**：
  - 构造「同组重试、两次用量不同」（如 `[120, 160]`）
  - 断言 `used == 280`（**不是 160**）
  - **变异**：把修复回退成覆盖语义 ⇒ **必须变红**
  - 每条给**二进制名 + 运行条数**
- ⚠️ **编译失败不算变异执行**

## AC-002 · 费用闸按真实累计判定

- 预期结果：把 `max_used` / `max_cost` 调到**介于两次尝试之间**（如 200），
  该组仍应**被闸门拦住**（`stopped = true`），
  而不是因为「只算了 160 < 200」而**放行**
- 类型：架构
- **理由**：这是本条需求的真正目的 —— 上一版错在**闸门基于错数放行**，
  只修报表不修闸门等于没修

## AC-003 · 币种不可比时行为不变

- 预期结果：两次尝试**币种不同** ⇒ `cost` 仍返回 `None`（`checked_add` 的既有语义），
  **不得**因为改成累加就把「不可比」变成「相加」
- 类型：功能
- ⚠️ 理由：`:406` 的注释明说「币种不可比就返回 None —— 那不是『加起来』的问题」。

## AC-004 · 单次调用路径不受影响

- 预期结果：**不重试**的组，其 `used` / `cost` 与改动前**逐字节一致**
- 类型：回归
- **理由**：不能为了修「重试少计」把「正常路径」也改错

## AC-005 · 全量与红线

- 至少 1 轮完整 `cargo test --no-fail-fast`
- 预期结果：**559 passed / 0 failed / 1 ignored**（REQ-034 之后的基线）
- **建议同会话基线对照**；耗时**别声称精度**
- 红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动；
  `test_humanize_still_refuses_unknown_columns`（`src/domain/view.rs:1254`）一条断言都没放宽；
  **无测试被删除或 `#[ignore]`**
- 产物 sha **不变**（本需求不碰抽取逻辑）：
  - `stats.json` = `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba`
  - `proposal.json`（`--project extracted`）= `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635`
  - **输入就是 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`**
    （**BA 已实测：直接用它当 `--schema` 输入，两个 sha 全部精确 MATCH** ——
    上一位 Dev 说「复现不出来」是**搜错了目录**）
  - 命令实属 **`ontok-propose`**；`--out` 在 `--stats` 之前；**不传 `--names`**

---

## ⚠️ 一个连带问题要一并判断

Dev 在 REQ-034 里披露过：`absorb` 按 `group_id` **replace** 语义，
并把这条归到「将来做 fallback 时要注意」。

**但它当下就已作用在重试路径上** —— 那次验收没注意到。

**本需求里请判断**：`failed_groups` / `usage_reports` / 提案里的 `group_id`
是不是都假定「一表一组、一个稳定 id」。
若你选的修法**改变了 `group_id` 的形态**，必须说明对这三处的影响。

---

## 环境事实

- ⚠️ **`.env` 在仓库根**（`/home/myg133/Projects/Anchnet/Ontology/.env`），
  各 worktree 内**没有**。库名是 `notol`，**必须替换成 `ontol`**（该库名在实例上不存在）
- 正确取值方式（**上一位 Dev 在这里栽过**）：
  ```bash
  export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
    /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
  ```
  出现 `relative URL without a base` = **URL 取残缺，不是环境坏了**
- 共享本体库**只读**；**禁 DELETE / TRUNCATE / 清空表**
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
| `test_backfill_…` | **不许放松任何一条断言** |
| `.review-workspace/fix-props/schema.json` | **绝不可删、绝不可覆盖**（本需求要拿它验产物 sha） |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **绝对禁止** `ln -s` 指向别的 target（符号链接让两个 worktree 共享构建产物，本会话连踩两次）
- ⚠️ **不要与另一个全量测试并发跑**（本会话已两次互相拖垮）
- **求和不要用 `paste -sd+`**（逐行输出 ⇒ 内容重复即精确翻倍）
- **量化数字必须标口径与命令**（本会话已 6 次因数字没标口径引发返工）
- **编译失败不算变异执行**；**存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req050/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- **BA 分支（`demand`）只由 BA 写** —— 你那个 worktree 的分支上没有 `BA/` 目录，
  **不需要也不应该**去改它
- ⚠️ **边做边把结果落盘** —— 本会话已有 **4 次子 agent 会话丢失**的先例
