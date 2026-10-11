# REQ-054 Dev 交付报告

- 分支：`feature/REQ-054`（worktree `feature-REQ-054`），基线 `749c1c1`
- 提交：`9d6c529`（未 push，仓库无 remote）
- session：`ba-archlinux-80180-20261007-231834`（照抄规约，未改写）

## 结论

四条 AC 全部落地。改动只碰 2 个文件（`src/agent/orchestrate.rs`、
`tests/test_attempt_numbering.rs`），**触发条件未动**（四行内容 sha256 前后一致）。

---

## AC-001 · 文案怎么拆两种口径

改动位置：`src/agent/orchestrate.rs` 的 `BudgetReport::absorb`（碰撞人话 + 上方论证注释）。

**非空 `group_id` 分支**从两分叉改成三分叉：

| 口径 | 判据形态 | 账本后果 |
| --- | --- | --- |
| ① **该组口径累计** | 本组 120 → 280，280 即该组总额 | **账是对的**（坏的是增量契约） |
| ② **整轮 / 会话口径累计** | 本组只花 280，连同前组已入账的 100 一起报成 220 → 380 | **多计** ⇒ 前组的钱被再算一遍 |
| ③ **增量** | 120 → 160 | **少计** ⇒ 闸门按偏低的数放行 ⇒ 真的有资金缺口 |

**读的人该核对什么**（行动项改成可执行的两问）：

> ⇒ **去核对两件事**：这个组报的是累计还是增量；若累计，累计的**口径**是
> **本组**、**整轮/会话**、还是**跨 round**。

⚠️ **比 AC 原文更精确的一处**：AC 写「② ⇒ 多计、闸门可能提前停 ⇒ 真的出事」。
文案没有照抄这句，因为**单轮内够不到提前停** —— 碰撞已让本轮 `stopped`、
闸门**未被求值**（`absorb` 在 `if !self.usage_collisions.is_empty()` 里就
`return` 了）。照抄就是本需求第 4 次「过度声称」。实际写的是：

- 多记**不会**多花钱（`exceeded_by` 只会被抬得更高 ⇒ 更早停），危害是账偏高、误导排障；
- 只有口径**跨 round** 时才真会提前停：下一轮是**空账本**
  （`orchestrate` 每轮新建 `BudgetReport`，`orchestrate.rs:783`），
  第一读就偏高、**不再有碰撞**，闸门才被真的求值。

## AC-002 · 「第三种可能」清单

**穷尽性的论证轴**：判据只看两个数（上一读、本次读）。真值不可观测，
但**方向**可分三类 —— 最后一读 **= 真值（账对）/ > 真值（多计）/ < 真值（少计）**。
下面 6 条覆盖全部方向，2 条明确不覆盖并给了理由。

| # | 可能 | 覆盖？ | 落在文案哪句 |
| --- | --- | --- | --- |
| 1 | 该组口径累计（120→280） | ✅ | ① 账是对的 |
| 2 | 整轮/会话口径累计（220→380，QA 情形 4，**单轮内可达**） | ✅ | ② 多计 |
| 3 | **跨 round / 会话累计**（AC 点名的第三种） | ✅ | ② 内「只有口径跨 round 时才可能让闸门提前停」 |
| 4 | 增量（120→160） | ✅ | ③ 少计 ⇒ 唯一的资金缺口方向 |
| 5 | 同键 `used` 相同、**只有 `cost` 不同** | ✅ **本轮新增** | 「用量这一栏可能仍对，错的是**费用闸**」（`report_same_spend` 两个字段都判） |
| 6 | **空 `group_id` + 按组各自报总额** | ✅ **本轮补进条件** | 空分支原文只点了「增量」，已改成「报增量，**或按组各自报总额** ⇒ 一样会少计」 |
| 7 | 该组口径累计，但最后一读**不是**最终值（异步补记） | ❌ 不覆盖 | 编排每组跑完才 `absorb`，同轮内最后一读必是最终值；两个真实 adapter 都不是异步补记。方向仍是「偏小 ⇒ 少计」，与 #4 同类 |
| 8 | **重试恰好花同样的钱** ⇒ 同键同值 ⇒ `report_same_spend` 判同 ⇒ **不触发**、静默少计、零诊断 | ❌ **明确不覆盖** | 判据就是「同键异值」。覆盖它必须改触发条件，而触发条件是 AC-003 的硬约束。已在代码注释里写明「既有盲区，非本需求引入」 |

**第 8 条的推广**：`report_same_spend` 的豁免是按**数值相等**放的，不是按成因 ——
恒报 0、同额重试、同额分片全被一并放过。这不是「一个漏网形状」，是**豁免条件本身**的盲区。

### ⚠️ 做不到的事（直说）

**文案无法替读者判定真实口径。** 编排只看得到两个数，看不到 adapter 内部。
所以「穷尽」只能穷尽在「从这条信息能推出的**方向**与**核对清单**」这个意义上，
文案给的是**三种可能 + 两问核对**，不是「判定结论」。
声称文案本身能定论，就是第 4 次过度声称 —— 宁可承认做不到。

## AC-003 · 三条变异证据

二进制固定为 `test_attempt_numbering`，每轮 **8 条**，均**编译成功**后执行
（`could not compile` / `error[E` 计数 = **0**，编译失败不算变异执行）。

| 变异 | 改动 | 预期 | 实测 | 红的是哪几条 |
| --- | --- | --- | --- | --- |
| **M3** | `src/agent/llm/mod.rs` `attempt` → `0` | 2 红 | **2 红**（6 passed / 2 failed） | `llm_agent_numbers_every_attempt_of_the_same_group`、`llm_retry_spend_reaches_the_budget_as_two_separate_entries` |
| **M3b** | `src/agent/acp/mod.rs` `attempt` → `0` | 2 红 | **2 红**（6 / 2） | `acp_agent_numbers_every_attempt_of_the_same_group`、`acp_retry_spend_reaches_the_budget_as_two_separate_entries` |
| **M4** | `src/agent/acp/mod.rs` `count() as u32 + 1` | 1 红 | **1 红**（7 / 1） | `acp_agent_numbers_every_attempt_of_the_same_group` |

- 三条**都只红改动侧**，另一侧全绿 ⇒ 判据定位准确。
- M4 为什么只红 1 条：`count()+1` 让编号变成 1、2 ⇒ **键仍互不相同** ⇒
  不撞键、账仍合计 370 ⇒ 编排那条断言不红，只有编号断言红。
  （若改成恒报 1（两次都报 1）会红 **2** 条 —— 与 AC 预期不符，故未采用。）
- 三条变异全部回退，`git diff` 相对提交**为空**，二进制恢复 `8 passed / 0 failed`。

**触发条件 sha256（AC-003 硬约束）**：

```
0381c3a0ff649648efd35c721c3d6de04db47275157aaf1f8f394dca0f765edc
```

口径：`sed -n '<start>,<start+3>p' src/agent/orchestrate.rs | sha256sum`，
`<start>` 由 `grep -n 'let bumped = self'` **内容定位**。改前（行 508）改后（行 533）**一致**。

> 顺带一条实证：该块在本次改动里**从第 508 行漂到第 533 行**（上方加了 25 行注释）
> —— 正是 AC-004「行号锚定不可靠」的活证据。

## AC-004 · 行号锚定换符号名

**主锚点**（QA 建议采纳）：`tests/test_attempt_numbering.rs` 新增

```rust
const EXPECTED_USED: u64 = 370;
```

- LLM 与 ACP 两条编排测试的 `assert_eq!` 都改用它，失败信息里也用
  `{EXPECTED_USED}` 内联捕获 ⇒ **单一真相源**。
- 原注释里靠「下面那条」消歧的 `used == 370`（在文件中**出现两次**）改成指
  `EXPECTED_USED` / 测试函数名 ⇒ 锚点**唯一**。

**顺带查出的行号引用残留**：

| 位置 | 原文 | 判定 | 处理 |
| --- | --- | --- | --- |
| `orchestrate.rs`（原 :901） | `` `src/agent/mod.rs:434-436` `` | ❌ **已失效** —— 那几行现在是 `pub enum Assembly`，不是 `drive_group` | 改成符号锚定 `crate::agent::drive_group` 里「Ok 就 return」的那一支 |
| `test_attempt_numbering.rs:334` | `` `:333` / `:337` `` | 保留 —— 它**叙述的是历史教训**（REQ-053 首稿写行号后失效），不是当前代码的锚点 | 保留，并在 `EXPECTED_USED` 的文档注释里重复一次，避免后人再犯 |

两个文件**现已无任何指向当前代码的行号锚点**（`grep -nE '[A-Za-z_/]+\.rs:[0-9]+'` 无命中）。

## 全量与产物

**全量**：`cargo test --no-fail-fast`，`exit=0`，41 个测试二进制。
口径：逐行 `test result:` 用 **awk 累加**（**未**用 `paste -sd+`）：

```
passed=574  failed=0  ignored=1
```

基线 `749c1c1` = **574/0/1** ⇒ **条数零变化**。那 1 条 ignored 是既有的
`tests/test_mcp_server.rs:553`（`#[ignore = "缺口清单，不是可运行断言"]`），未碰。

**产物 sha256**（输入 `/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`，
`cargo run --bin ontok-propose -- --schema … --out … --stats … --project extracted`，
**`--out` 在 `--stats` 之前、不传 `--names`**）：

| 文件 | sha256 | 与 AC |
| --- | --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | ✅ 一致 |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | ✅ 一致 |

输入 `schema.json` 只读，未删未覆盖。

**红线**：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`
相对基线**各 0 处改动**；删除或 `#[ignore]` 的测试行数 = **0**；
`test_humanize_still_refuses_unknown_columns` 所在文件 diff = **0 行**。

## 代码失败 vs 环境类失败

- **代码失败：0**（574/0/1，`exit=0`；三条变异按预期红）。
- **环境类失败：0**。DB 环境变量按规约从**仓库根** `.env` 取、库名 `notol` → `ontol`
  替换后校验形态为 `postgres://…/ontol`（**不回显口令**），无 `relative URL without a base`。
- ⚠️ **未测**：`SELECT 1` p50（本机无 `psql` / `psycopg`）—— **不声称**。
- 复现率类数字**随负载浮动，本轮不给出稳定值**。

## 发现但未修（如实记）

1. **`usage_collisions` 字段文档表格里残留 REQ-052 的过度声称**：原表「非空 ⇒
   **已经错了** ⇒ 真的资金缺口」，与 REQ-053 在 `absorb` 里的降级**自相矛盾**。
   这是同一缺陷类的第 4 处，已一并降级（否则文档与文案互相打架）。
2. **`report_same_spend` 的同值豁免**（AC-002 #8）：静默少计、零诊断。
   **修它必须动触发条件 = 破坏 AC-003 硬约束**，故只记录、不动。
3. 空 `group_id` 分支的口径之分是**退化**的：整轮只占一个键，「本组 vs 整轮」分不开，
   已在代码里写明，避免后人照搬非空分支的两分叉。

## 自己引入又清掉的问题

1. 临时 `println!`（为核对文案**渲染后**的真实文本，加在碰撞测试里）
   ⇒ 已回退，`grep 'REQ054-TEMP-RENDER'` 无残留，`git diff` 相对提交为空。
2. 三条变异标记 `REQ-054-MUTATION-*` ⇒ 全部回退，无残留。

## 偏离登记

- 第一次跑全量时把日志 tee 到 `.review-workspace/req054/`，**该目录当时不存在** ⇒
  tee 失败、输出丢失，任务已取消。`mkdir -p` 后重跑成功（`exit=0`）。
  **未**用 `paste -sd+`；**未**用 `ln -s`；**未**设 `CARGO_TARGET_DIR`；**未**用 `--release`。
- 全程单线程串行，**未与任何其他全量测试并发**（每次跑前确认无 `cargo test` 在跑）。
- 临时文件只落在 `.review-workspace/req054/`；未用 `/tmp`、未用 home。
- 含反引号的 Markdown 全部用 `write`/`edit` 工具写入（commit message 内无反引号，
  用带引号 heredoc 的 `git commit -F -`）。