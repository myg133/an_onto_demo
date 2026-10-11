# REQ-050 验证记录（边做边落盘）

> session: ba-archlinux-80180-20261007-231834
> 口径：所有数字均标注「命令 + 二进制名 + 运行条数」。

---

## 0. 环境自证（先排除「环境类失败」）

```
$ cargo test --lib extract::introspect::tests::read_only_pool_really_reads_back_on
running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 114 filtered out; finished in 0.66s
```

URL 取值长度 59，结尾为 `/ontol`（**未 echo 连接串**）。
⇒ DB 可达；后续若有失败，**不是**环境类。

---

## 1. 突变证据

### M1 · 把修复回退成「覆盖语义」

改动：`orchestrate.rs` 的 absorb 键 `r.attempt` → 字面量 `0`
（= 只按 `group_id` 建键，**正是修复前的覆盖语义**）。
**编译通过**，所以这是真的变异执行，不是编译失败。

命令：`cargo test --test test_budget_retry_accounting`

| 结果 | 条数 |
| --- | --- |
| 通过 | 3 |
| **变红** | **4** |

变红明细（`left` = 变异后实测值）：

| 测试 | left | right |
| --- | --- | --- |
| `test_retry_spend_is_all_counted_not_just_the_last_attempt` | **250** | 370 |
| `test_budget_gate_trips_on_the_real_total_not_the_last_attempt_only` | **250** | 280 |
| `test_budget_gate_trips_even_when_limit_is_between_the_two_attempts` | **250** | 370 |
| `test_retry_with_two_currencies_still_reports_no_cost` | **250** | 370 |

**250 = 160 + 90** —— 正是「只留最后一笔 160」+「另一组 90」。
AC-001 / AC-002 均被杀。✅

### M1 的反向发现：**既有测试完全看不见这个 bug**

同一次变异下跑既有预算/协议套件（`cargo test --test <名>`）：

| 二进制 | 结果 |
| --- | --- |
| `test_agent_rebuild` | 26 passed / 0 failed |
| `test_acp_protocol` | 18 passed / 0 failed |
| `test_llm_fallback` | 12 passed / 0 failed |
| `test_budget_report_truth` | 3 passed / 0 failed |

**59 条，0 变红。** ⇒ REQ-050 这个资金缺口在旧套件里**完全没有判据** ——
这就是本需求必须新增测试的证据，不是「顺手补测」。

### M2 · 去掉「不可比不可被救回」的守卫

改动：`totals()` 里 `if incomparable { continue }` → `if incomparable && false`。
**编译通过**。

命令：`cargo test --test test_budget_retry_accounting`
→ 6 passed / **1 failed**（只杀 `test_retry_with_two_currencies_still_reports_no_cost`，
实测 `cost = Some(0.09 USD)` —— 即「USD 不可比于 EUR 之后，被后来的 USD 救回成假钱数」）。

判别力干净：**只杀 AC-003 那一条**，其余 6 条不受影响。

---

## 2. 修复后的定向结果

命令：`cargo test --test test_budget_retry_accounting`
→ **7 passed / 0 failed / 0 ignored**

| 测试 | AC |
| --- | --- |
| `test_retry_spend_is_all_counted_not_just_the_last_attempt` | AC-001 |
| `test_budget_gate_trips_on_the_real_total_not_the_last_attempt_only` | AC-002 |
| `test_budget_gate_trips_even_when_limit_is_between_the_two_attempts` | AC-002 |
| `test_budget_gate_does_not_trip_when_the_real_total_is_under_the_limit` | AC-002 反向 |
| `test_retry_with_two_currencies_still_reports_no_cost` | AC-003 |
| `test_non_retrying_run_keeps_its_exact_numbers` | AC-004 |
| `test_rereading_the_same_snapshot_does_not_double_count` | 守住修复的另一半 |

回归（`cargo test --test <名>`）：
`test_agent_rebuild` 26/0、`test_acp_protocol` 18/0、`test_llm_fallback` 12/0、`test_acp_extraction` 28/0。

⚠️ **`test_agent_rebuild` 的 26 条全绿是这次选型的关键约束**：
它里面有一条「同组同值重复上报不翻倍」（`UsageReportingAgent` 的 `#dup`）。
按数组位置/游标记账的方案会让它**翻倍 ⇒ 红**，
所以本方案必须靠**显式 `attempt`** 而不是靠位置。

---

## 2.5 全量（AC-005）

命令：`cargo test --no-fail-fast`（**单独跑，未与任何其它测试并发**）
日志：`.review-workspace/req050/full-test-post.log`

口径：对日志里 40 条 `test result:` 行逐行 awk 求和（**未用 `paste -sd+`**）。

| 口径 | passed | failed | ignored |
| --- | --- | --- |
| **全部** | **566** | **0** | **1** |
| 其中新增二进制 `test_budget_retry_accounting` | 7 | 0 | 0 |
| **排除新增 = 旧套件** | **559** | 0 | 1 |

核对：**559 + 7 = 566** ✓ 与 AC-005 基线 559 精确吻合
（559 是基线数，本需求新增 7 条 AC 测试，故总数必然上升）。

耗时：**401 秒**（wall，`date +%s` 包住整条 `cargo test --no-fail-fast`）。
**不声称精度** —— 复现率与耗时随负载浮动。

`FAILED` / `error` / `panicked` 在整份日志中出现次数：**0**。

### 红线核查

| 项 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` | `git diff --quiet` ⇒ **未改动** |
| `tests/test_fk_naming.rs` | `git diff --quiet` ⇒ **未改动** |
| `src/domain/view.rs` | `git diff --quiet` ⇒ **未改动** |
| `test_humanize_still_refuses_unknown_columns` | 仍在 `src/domain/view.rs:1254`，该文件零改动 |
| 新增 `#[ignore]` | **0** |
| 删除的 `fn test_` | **0** |
| 新增 `fn test_`（已跟踪文件内） | **0**（7 条全在**新文件**里） |

`tests/test_acp_protocol.rs` 的 5 处删除行经逐行核对：**4 处是 `delta_to_report`
调用点补第 3 个实参**（签名变了，必须补），**1 处是改写过时注释**。
**没有删除任何断言。**

### 编译告警

5 条告警全部落在**我没碰的文件**（`test_acp_extraction` / `test_concept_store` /
`test_concept_transition` / `test_overview`）或基线就存在的那条
（`test_agent_rebuild.rs:337` 的死变量 `report` —— 改动前的首次构建输出里
就有 `warning: ontok (test "test_agent_rebuild") generated 1 warning`）。
⇒ **本次改动未引入新告警。**

## 2.6 产物 sha（AC-005）

输入：`/home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json`
（**只读引用**；运行前后 sha 均为 `b71c2afd…f2019`，未被改动）

命令（原样，`--out` 在 `--stats` 之前，**不传 `--names`**）：
```
cargo run --quiet --bin ontok-propose -- \
  --schema /home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json \
  --out <req050>/artifacts/proposal.json \
  --stats <req050>/artifacts/stats.json \
  --project extracted
```

| 产物 | 实测 sha256 | AC-005 期望 | 结论 |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2…2cd7ba` | `cda6a9a2…2cd7ba` | **精确 MATCH** ✅ |
| `proposal.json` | `8dbd32db…3e4c3e` | `8dbd32db…3e4c3e` | **精确 MATCH** ✅ |

上一位 Dev 说「复现不出来」确系搜错目录 —— 用这个路径直接命中。

本机无第二份 baseline 产物可比对，所以用**基线上就存在、且断言逐字未动**的那条：
`tests/test_agent_rebuild.rs::test_budget_stops_the_run_and_records_skipped_groups_separately_from_failures`
在 `c082556` 就钉着 `assert_eq!(run.stage.budget.used, 20, ...)`，
本次修复后**仍然绿** ⇒ 不重试的路径数字没变。
再加本文件 `test_non_retrying_run_keeps_its_exact_numbers` 钉死 210 / 0.210 USD。

---

## 4. 交付

- 分支：`feature/REQ-050`（仓库 **0 个 remote**，**未 push**）
- commit：`f43940f`
- 提交后工作区脏文件数 **0**；提交态复跑：
  `test_budget_retry_accounting` 7/0、`test_agent_rebuild` 26/0、`test_acp_protocol` 18/0

### 口径与纪律自查

- 编译失败**未**计入变异执行：M1 / M2 均**编译通过**后才运行（见 §1）。
- 求和用 awk，**未**用 `paste -sd+`。
- 所有数字均标注了命令 / 二进制名 / 运行条数 / 口径。
- 临时文件全在 `.review-workspace/req050/`（ANALYSIS / VALIDATION / full-test-post.log / artifacts）。
  **未用 `/tmp`、未用 home。**
- **未 `push`**（无 remote）、未 squash、未 force、未改历史。
- 连接串**只经环境变量**传入，全程**未 echo**；口令未进代码/文件/日志/报告/commit。
- 共享本体库**只读**（仅跑既有只读探针），**未执行** DELETE / TRUNCATE / 清空表。
- 报告已把「代码失败」与「环境类失败」分开：本会话**无环境类失败**
  （DB 探针 1 passed / 0.66s 已先行自证）。
