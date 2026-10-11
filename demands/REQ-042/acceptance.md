# 验收标准：REQ-042 覆盖缺口 + 源码判据补洞 + 行序恒等断言

> 依据 REQ-041 验收（QA 判 PASS 带非阻断，7 项非阻断）写成。基线：`4f03d50`（develop）。
> **本文件带一条「已更正的做法」—— 务必先读 §AC-003，那是 QA 推翻 Dev 结论后的正确做法。**

---

## AC-001 · 补齐 6 处建连接覆盖缺口

Dev 上报 5 处，QA 实测**是 6 处**（Dev 少算了一处）：

| # | 位置 | 说明 |
| --- | --- | --- |
| 1 | `tests/test_db.rs:142` | ⚠️ **Dev 说「结构上不能改」是错的** —— 它无 `after_connect`、签名与 `retry_connect` 完全兼容，**可以套重试** |
| 2 | `tests/test_db.rs:164` | 自建 `PgPoolOptions` + `after_connect` |
| 3 | `tests/test_db.rs:323` | 同上 |
| 4 | `tests/test_db.rs:608` | 同上 |
| 5 | **`src/db/apply.rs:219`** | ⚠️ **生产代码**，Dev 说越权不碰 |
| 6 | **CLI 子进程走生产 `pool::connect`** | QA 基线对照实测红的**正是这一处**（`Connection reset os 104`） |

- 预期结果：每处都套上重试，**或**给出「为什么不套」的具体理由
- **第 5、6 处涉及生产代码** —— 动之前先说清：
  - 生产路径加重试会不会掩盖真故障？
  - `pool::connect` 被 CLI/生产共用，重试语义变化会影响谁？
- 类型：功能

## AC-002 · 源码判据的 3 个洞

`test_retry_scope_is_connect_only` 取的是**定长 200 字符裸切片**，
QA 实测窗口**越过函数体末尾吃进后面 3 个 `const`**。三条绕过存活：

| 洞 | 绕过方式 | QA 实测 |
| --- | --- | --- |
| **H1** | helper 名字含 `connect_for_tests` 子串、连上后顺手 migrate | 存活，**且行为面真被绕过**（连上之后的失败被重试了 3 次） |
| **H2** | 填充把 migrate 顶出 200 字符窗口 | 存活 |
| **H4** | 更早处放诱饵 | 存活 |

- 预期结果：三条**逐一复现并被杀死**
- 类型：测试有效性
- **Dev 说的「类型层 + 源码层双保险」不成立** —— 类型层挡不住 H1
  （helper 名带子串时，类型签名完全合法）

> **判据该怎么钉**：别再用定长裸切片。考虑用**结构化解析**（找 `retry_connect(` 的
> 配对括号，取整个调用表达式），或改钉「`retry_connect` 的实参里出现
> `migrate/execute/query/fetch/begin/commit` 这些标识符**」——
> **判据要钉在「被重试的代码里不许有什么」这件事上，而不是「文本长什么样」**。

## AC-003 · 行序自检补恒等断言 ⚠️ **做法已更正，照此做**

### 背景

现有 `assert_insert_order_is_not_dict_order` 断言的是**性质**（插入序 ≠ 字典序）。
REQ-041 时 Dev 的结论是「M13c 杀不掉，要改 SQL 字面量 + 从拼好的 SQL 解析回来」。

### ⚠️ QA 已实测推翻这个做法

**加一条 `assert_eq!(pushed, keys)`（恒等断言）M13c 就被杀** ——
**不改 SQL、不做解析、不动 SQL 构造方式**。

> **照 Dev 的原说法做会造成不必要的大改。** 以本条为准。

- 预期结果：**M13c 被杀死**
- 类型：测试有效性
- **允许继续存活的**：置换写在 `bind` 处的变体 Y ——
  QA 实测证明**单一真源这一类无解**，不要做无底洞

## AC-004 · 变异自证（AC-001/002/003 各自的判别力）

每条**真的跑过**，给**二进制名 + 运行条数**，**编译失败不算**：

- AC-001：把某处套上的重试摘掉 ⇒ 该处对应的判据须红
- AC-002：H1 / H2 / H4 逐一复现 ⇒ 须被修好的判据杀死
- AC-003：M13c ⇒ 须被恒等断言杀死

**存活变异必须逐条论证**是「等价变异」还是「判据有洞」。

## AC-005 · 全量测试不劣化

- 测试步骤：至少 1 轮完整 `cargo test --no-fail-fast`
  **+ 一轮同会话基线对照**（基线 `4f03d50` 的参考值：**534 passed / 0 failed / 1 ignored，6m42s**）
- 预期结果：**534 passed / 0 failed / 1 ignored**
- 类型：回归

> **耗时口径提醒**：REQ-041 验收时两个样本（改动后 376s、基线 342s）
> 与 Dev 那轮（改动后 331s、基线 386s）**方向相反且都不足以分辨 ±30s**。
> 所以耗时这一项**别声称精度**，只报「无可测量劣化」或「劣化 X 秒」。

## AC-006 · 红线未被破坏

- 预期结果：
  - `tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs` 零改动
  - `test_humanize_still_refuses_unknown_columns` 一条断言都没放宽
  - **无测试被删除或 `#[ignore]`**
  - `test_backfill_…` 未放松任何断言
- 类型：架构

## AC-007 · 产物未变

| 产物 | 基线 sha256 |
| --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` |
| `proposal.json`（`--project extracted`） | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` |

- 命令实属 **`ontok-propose`**，在 worktree 下可跑；`--out` 在 `--stats` 之前

---

## 环境事实

- 共享集群 DB 很慢：`SELECT 1` p50 实测 **155~161 ms**；总连接常驻 **32~38 / 100**
- **先探延迟再判断测试慢不慢**；复现率随负载浮动，**不声称稳定值**
- 报告把「代码失败」与「环境类失败」（`PoolTimedOut` / `ConnectionReset`，
  发生在**建连接阶段、迁移之前**）**分开写**
- ⚠️ **运行期判据与环境类失败要严格二分** —— 环境失败**绝不能冒充「杀死」**。
  本项目已发生过多起（前一位 QA 的 runner 就犯过）
- 本机**没有 `psql`、没有 psycopg**
- 共享本体库**禁 DELETE / TRUNCATE / 清空表**；源库只读
- 口令只走环境变量，**绝不写进报告**。测试库 `.env` 库名 `notol` → `ontol`
- ⚠️ **`.env` 里库名就是 `notol`**，忘了替换会让所有 DB 测试秒败，
  而症状看着像「集群故障」—— 本会话已有 Dev 与 QA 各踩过一次

## 红线清单

| 文件 / 约束 | 说明 |
| --- | --- |
| `tests/drop_rate.rs` | 禁改 |
| `tests/test_fk_naming.rs` | 禁改 |
| `src/domain/view.rs` | 禁改 |
| `test_humanize_still_refuses_unknown_columns` | 一条断言都不许放宽 |
| 任何测试 | **不许删除、不许 `#[ignore]`** |
| `test_backfill_…` | **不许放松任何一条断言** |
| `.review-workspace/fix-props/schema.json` | **绝不可删**（门禁输入） |

## 交付纪律

- **绝对禁止** `CARGO_TARGET_DIR` / `--release` / 另建 probe crate
- **量化数字必须标口径与命令**（本会话已 5 次因数字没标口径引发返工）
- **编译失败不算变异执行**
- **存活变异必须逐条论证**
- 临时文件只放 `.review-workspace/req042/`；**禁 `/tmp`、禁 home**
- **写含反引号的 Markdown 必须用 edit/write 工具，禁止用 shell**。
  （本环境对长 Markdown 可能失败；失败就用带引号 heredoc 兜底并登记偏离，**别反复重试**。）
- ⚠️ **不要 `echo` 连接串** —— 本会话已有 Dev 与 QA 各踩过一次口令回显的坑
- 核对文件用**裸 `grep`**。禁 `rm`（本环境已接管成 `mavis-trash`）
- 仓库**无 remote，不要 push**；不 squash、不 force、不改历史
- ⚠️ **边做边落盘**，不要攒到最后一次性输出 —— 本会话已有 **4 次子 agent 会话丢失**的先例
