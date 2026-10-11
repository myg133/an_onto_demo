# REQ-054 验收报告 · QA 独立核实

- worktree：`/home/myg133/Projects/Anchnet/Ontology/verify-REQ-054`
- 分支 `verify/REQ-054` · HEAD `9d6c529` · 基线 `749c1c1`
- 全程未 `push`、未改历史、未动红线、未建 probe crate、未设 `CARGO_TARGET_DIR`、未 `ln -s`
- 结束时 `git status -uall` 为空、HEAD 仍是 `9d6c529`

## 结论：**PASS（带 1 条非阻断发现）**

Dev 拒绝照抄 BA 的 AC-001 **是对的**，且理由经**读码 + 实测**双重独立证实。
三条变异全部复现且数字与 Dev 自述**逐个吻合**。触发条件逐字节零改动。红线零改动。
产物 sha256 两个都 MATCH。全量 **574 passed / 0 failed / 1 ignored**（41 个二进制）。

---

## 1. Dev 拒绝照抄 AC，对吗？—— **对。BA 那句是第 4 次过度声称。**

### 1.1 独立核实「单轮内闸门未被求值」

**读码**（`src/agent/orchestrate.rs`）：

```
:466  for r in reports { … }                  // absorb 遍历**全量快照**
:580  let (used, cost) = self.totals();
:586  if !self.usage_collisions.is_empty() {
:587      self.stopped = true;
:591      self.reason = Some("用量上报碰撞…");
:596      return;                             // ★ 闸门在此之前就返回了
:597  }
:598  let Some(limit) = limit else { return };
:599  if limit.exceeded_by(used, self.cost.as_ref()) {   // ★ 闸门在这里
```
碰撞一旦非空，`:596` 的 `return` 使 `:599` **本轮不可达**。`absorb` 每轮只调一次
（`:964`，在每组跑完之后）。编排侧 `orchestrate` 每轮新建 `BudgetReport`（`:816-820`）。

**实测**（我写的临时 probe，跑完已删）：
会话累计口径、attempt 恒 0、t_b 第一次调用失败触发重试、闸门 `max_used = 350`：

| 量 | 值 |
|---|---|
| 真花 | 300（t_a 100 + t_b 200 + 100） |
| 账上 | **400**（多计 100） |
| 400 vs 上限 350 | **已越闸**，但 reason 以「用量上报碰撞」开头，**不是**「用量闸：已用 400」 |

⇒ **单轮内，多计出来的数够不到闸门**。Dev 的机制陈述成立。

### 1.2 独立核实「只有跨 round 才真会提前停」—— **成立**

同一 agent 连跑两轮（账本每轮重置、会话计数器继续、不重试故零碰撞、`max_used = 650`）：

| 轮 | 账上 | 真花 | stopped | skipped |
|---|---|---|---|---|
| 第 1 轮 | 600 | 600 | false | — |
| 第 2 轮 | **900** | **200** | **true** | `["table:t_c"]` |

reason = 「用量闸：已用 900，上限 650」。
⇒ 跨 round 口径下闸门**真的被求值并提前停**，白跑掉 t_c。**Dev 的限定条件准确。**

### 1.3 BA 那句「② ⇒ 多计、闸门可能提前停 ⇒ 真的出事」—— **是过度声称（第 4 次）**

两处错：

1. **「闸门可能提前停」对「整轮口径」不成立**（见 1.1，够不到）。
2. **更根本：方向反了。** 即便在可达的「会话 / 跨 round」口径下，提前停是
   **fail-closed —— 停得更早 ⇒ 花得更少**。这**不是**资金事故。
   三条里唯一多花钱的方向是 **③ 少计**。`orchestrate.rs:387` 早就写着
   「方向是 fail-closed 的：停 = 白跑，**不会多花钱**」。

⇒ BA 把「② 多计」与「真的出事」连起来，是**从不可观测的真值方向直接推后果**——
与 REQ-051/052/053 同一种错误。**同类第 4 次，根源仍在 BA 的 AC 原文。纪律备忘应再升级一条：
AC 里凡写「⇒ 真的出事 / 真的有资金缺口」，必须先给出「哪个方向会多花钱」的论证。**

### 1.4 Dev 的三分叉有没有把「不会多花钱」说清楚？—— **说清楚了**

- 注释 `orchestrate.rs:507-508`：「方向与少计**相反**：多记**不会**多花钱
  （`exceeded_by` 只会被抬得更高 ⇒ 更早停），但账本偏高会把排障的人带偏」
- 面向人的文案：`② … ⇒ **多计** ⇒ 账偏高（**不会**多花钱；只有口径**跨 round**
  时才可能让闸门提前停，同一轮里碰撞已让本轮停、闸门未被求值）`
- 行动项两问：「这个组报的是累计还是增量；若累计，累计的**口径**是本组 / 整轮·会话 / 跨 round」

⇒ 「不会多花钱」「唯一多花钱的是 ③」「口径要分三类」三点齐备。
**一处措辞可商榷**：「不会多花钱」在**资金**维度完全正确（这就是本项目跟踪的维度），
但读者可能读成「毫无后果」——跨 round 的提前停其实是**白跑**（完整度损失）。
文案本身写了「可能让闸门提前停」，事实已披露，不构成过度声称。**非阻断。**

---

## 2. 「第三种可能」清单穷尽吗？「做不到」是真的吗？

### 2.1 我独立构造的、Dev 没列的形态

| # | 构造 | 实测结果 | 文案覆盖？ | 判断 |
|---|---|---|---|---|
| 1 | **同键 `used` 减少**（280 → 120） | **触发**（1 处碰撞），账上留下 120 | ❌ **没点名** | 见 2.2 |
| 2 | `cost` 由 `None` 变 `Some` | `report_same_spend:438-446` 的 `_ => false` ⇒ 判异 ⇒ **触发** | ✅ 「同键 `used` 相同、只有 `cost` 不同」 | 覆盖 |
| 3 | 同一组跨多个 round 报不同值 | 可达 | ✅ 「跨 round」口径 | 覆盖 |
| 4 | `attempt` 为负 | **类型上不可能**：`attempt: u32`（`mod.rs:287`） | 不适用 | — |
| 5 | `attempt` 溢出 | 回绕到 0 ⇒ 撞上首笔的键 ⇒ **被触发**，不会静默 | 不适用（理论） | 可忽略 |
| 6 | `used` 溢出 | `totals():625` 用 `saturating_add` ⇒ 饱和，**不回绕** | 不适用 | 已防 |
| 7 | 最后一读非最终值（异步补记） | 两个真实 adapter 均**同步** | Dev 明确不覆盖 | 理由成立（见 2.3） |

### 2.2 唯一缺口：**同键 `used` 减少**（非阻断）

实测：agent 对同一组先报 280、再报 120（attempt 恒 0）。
文案逐字是「上一读 used=280 … 本次读 used=120」，然后接 ①②③ 三支 ——
**三支的例子全是递增**（120→280、220→380、120→160），
没有任何一支的名字对得上「280 → 120」。

- **严重度：低**。方向上它仍属「< 真值」⇒ 少计 ⇒ 结论类别与 ③ 一致，
  而文案开头「账本最后留下的是**本次读到的数**」这句已经正确地描述了机制；
  行动项「去核对」对读者仍然有效。**它不会让人得出错误结论，只是分类标签没覆盖这一形态。**
- 归因：Dev 的穷尽性论证轴是「方向分三类（= / > / < 真值）」，**按方向确实穷尽**；
  但**按「值的形态」不穷尽**（少了一个递减形态）。

### 2.3 Dev 说的「做不到」—— **是真的做不到**

- 编排只看到每个键的**两个数**（`prev_used` / `new_used`），真值**不可观测**。
  判定口径需要 ground truth，而账本里没有、agent 接口里也没有。
  ⇒ 只能穷尽在「能推出的方向 + 核对清单」，**声称能定论才是第 4 次过度声称**。
- **不覆盖「异步补记」的理由经核实成立**：`usage()` 在两个 adapter 里都是
  `self.usage.lock().clone()`（`llm/mod.rs:382-384`、`acp/mod.rs:465-467`），
  `record_usage` 同步 push，无后台写入者。
  ⚠️ 这是**对当前两个 adapter 的代码形状成立**，不是结构性保证 —— 将来若引入流式
  用量源，该假设会静默失效。**信息项，非缺陷。**

---

## 3. 触发条件零改动 —— **成立**

**按内容**（非行号）抽取可执行触发块 `let bumped = self` → `if !report_same_spend` 共 5 行：

| 版本 | sha256 |
|---|---|
| `749c1c1` | `927bd4db461436fdb05696111528ca796f640fd13be633996b68c7e7635a94d5` |
| `9d6c529` | `927bd4db461436fdb05696111528ca796f640fd13be633996b68c7e7635a94d5` |

`diff` 逐字节：**IDENTICAL**。

**行号漂移实证**：旧 `508-512` 行与新 `533-537` 行 sha256 同为 `927bd4db…`，
`diff` 无差异 ⇒ **508 → 533** 与 Dev 自述一致。漂移只来自上方新增注释，
**触发块内容一个字节都没动**。

**整个 diff 的控制流扫描**（剔除注释与含引号的字符串行后 grep
`if|else|match|for|while|return|let|fn|=>|{|}`）⇒ **空** ⇒ 无控制流变化。

⚠️ **口径说明**：Dev 自述的触发块 sha `0381c3a0ff649648…` 我用 4 种候选区间
（整个 `absorb` 函数 / 到 `if let` 闭合 / 到碰撞块闭合 / 仅 5 行触发块）
**均未复现**。hash 取决于抽取区间，不同区间本就得不同值；
「内容未变」这一实质结论我已用自己的口径独立证实。**非缺陷，属口径差异。**

---

## 4. 行号锚定换符号名 —— **彻底**

- 旧锚 `src/agent/mod.rs:434-436` **确已失效**：实测该处现在是
  `#[derive(Debug, Clone, Copy, …)]` + `pub enum Assembly`（`mod.rs:435-436`）。
  Dev 属实。真实符号在 `mod.rs:464 pub fn drive_group`，Ok-return 在 `:473`
  ⇒ 新锚 `crate::agent::drive_group` **指对了**。
- 两个改动文件里仅剩 `:333` / `:337` 两处，位于
  `test_attempt_numbering.rs:271` 与 `:347`，
  上下文是「REQ-053 首稿就把 `:333`/`:337` 写进了注释，加完 8 行注释后两处立刻失效」
  ⇒ **历史叙述，不是当前锚点**。Dev 的区分**成立**。

### 独立构造的两个模拟（都在 workspace 副本上做，worktree 未动）

| 模拟 | REQ-053 式（内容指代 + 「下面那条」） | REQ-054 式（符号 `EXPECTED_USED`） |
|---|---|---|
| **A：把整个测试函数块（含 doc 注释）搬到文件末尾** | **存活**（doc 与函数同搬，「下面那条」是相对本函数的，仍指向同函数内断言） | **存活** |
| **B：把断言值 370 改成 400（散文不动）** | **两个锚点当场变死引用**：`:324`/`:331` 仍写「`used == 370`」，而全文件匹配数 = **0** | **存活**（`:273` 一处改、两处断言自动跟随） |

⇒ 单纯的代码重排**杀不掉**任何一种；**改数字才是分水岭**，这才是符号锚定真正的价值。
实测支撑 Dev 的判断：基线里 `budget.used, 370` **确实出现 2 次**（`:345`/`:428`）⇒「不唯一」为真。

⚠️ **措辞（非缺陷）**：Dev 说「两处注释共用 `EXPECTED_USED` 才**唯一**」略过头 ——
`EXPECTED_USED` 同样被 2 处断言引用，它不是唯一的**定位符**。
真正成立的两点是 ① 数字的**单一真相源** ② doc 注释**附着在函数上**（天然无歧义）。

**符号名锚定是不是最好的解法？** 在「同一文件内、跨函数共享一个数字」这个场景下，
是的 —— 它同时解决了行号漂移、内容歧义与重复维护。更好的做法本可再加一层
（如把两条编排测试的参数化成一个共享 helper），但那是**规模性重构**，
不属本 REQ scope，**不建议本轮做**。

---

## 5. 三条变异 —— **全部杀死**

二进制：`tests/test_attempt_numbering.rs`（`cargo test --test test_attempt_numbering`）。
变异执行前已改适配器源码并**重新编译**；三条的
`could not compile` / `error[E` 计数**均为 0** ⇒ 无一属于「编译失败冒充变异执行」。

| 变异 | 改动 | 结果 | 运行条数 | 红了几条 | 红的具体测试 | 只红改动侧？ |
|---|---|---|---|---|---|---|
| **M3** | `src/agent/llm/mod.rs` `attempt` → `0u32` | **killed** | 8 | **2**（6 passed / 2 failed） | `llm_agent_numbers_every_attempt_of_the_same_group`、`llm_retry_spend_reaches_the_budget_as_two_separate_entries` | ✅ 两条 ACP 侧全 ok |
| **M3b** | `src/agent/acp/mod.rs` `attempt` → `0u32` | **killed** | 8 | **2**（6 passed / 2 failed） | `acp_agent_numbers_every_attempt_of_the_same_group`、`acp_retry_spend_reaches_the_budget_as_two_separate_entries` | ✅ 两条 LLM 侧全 ok |
| **M4** | `src/agent/acp/mod.rs` `attempt` → `count() as u32 + 1` | **killed** | 8 | **1**（7 passed / 1 failed） | `acp_agent_numbers_every_attempt_of_the_same_group` | ✅ LLM 侧全 ok |

计数口径：每条变异独立跑 `test_attempt_numbering` 这**一个**二进制，
条数取该二进制 `test result:` 行的 `passed + failed`（8）。
**三条的数字与 Dev 自述（6/2、6/2、7/1）逐个吻合。**

**存活变异论证**：本次**无存活变异** —— 三条全部被杀死，无一条需要论证存活。
每条变异后均用 `git checkout --` 还原，复跑前 `git status -uall` 已验证为空。

---

## 6. 测试 / 红线 / 产物

### 6.1 全量

```
export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
  /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
cargo test --no-fail-fast
```

| 口径 | 值 |
|---|---|
| 二进制数（`test result:` 行数） | **41** |
| passed | **574** |
| failed | **0** |
| ignored | **1**（`tests/test_mcp_server.rs:553`，`todo!()` 缺口清单） |
| `could not compile` / `error[E` | **0 / 0** |

求和口径：逐个 `test result:` 行取 passed/failed/ignored 三列，**awk 累加**（未用 `paste -sd+`）。

**基线对照 —— 我没有重跑基线，而是给出了更强的证明**：
- 全仓库 `#[test]` 属性数：基线 **438**、HEAD **438**（相同）
- 全仓库真实 `#[ignore]` 属性数：基线 **1**、HEAD **1**（相同）
- 整个 diff 中 `#[test]` / `#[ignore]` / `fn ` 增删行数：**0**
⇒ **测试清单逐项相同，条数在数学上不可能变化**；HEAD 实测 574/0/1 与基线记载的
574/0/1 **一致** ⇒ **条数零变化**。
（Dev 的 574/0/1 与基线一致，本轮我独立复现了 HEAD 侧并证明了不变性。）

### 6.2 红线

| 项 | 结果 |
|---|---|
| `git diff --name-only 749c1c1 9d6c529` | 仅 `src/agent/orchestrate.rs`、`tests/test_attempt_numbering.rs` |
| `tests/drop_rate.rs` | **零改动** |
| `tests/test_fk_naming.rs` | **零改动**（复现 6/0） |
| `src/domain/view.rs` | **零改动** |
| `test_humanize_still_refuses_unknown_columns` | 该文件零改动 ⇒ 一条断言都没放宽；本轮实跑 `... ok`（`head-full.log:137`） |
| 测试被删除 / `#[ignore]` | **无**（见 6.1 计数） |

### 6.3 产物 sha256

输入 `.review-workspace/fix-props/schema.json`（**未改动**，mtime 仍为 `2026-10-07 23:55:23`，
sha256 `b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019`）。
命令（`--out` 在 `--stats` 之前、不传 `--names`、命令实属 `ontok-propose`）：

```
./target/debug/ontok-propose \
  --schema <…>/.review-workspace/fix-props/schema.json \
  --out   <…>/verify-req054/qa-proposal.json \
  --stats <…>/verify-req054/qa-stats.json \
  --project extracted
```

| 产物 | 我实测 sha256 | 期望 | 结果 |
|---|---|---|---|
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | **MATCH** |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | **MATCH** |

---

## 7. 发现但未修的问题

| # | 严重度 | 问题 | 触发条件 / 影响 / 证据 |
|---|---|---|---|
| **F1** | **低（非阻断）** | 三分叉文案**没有点名「同键 `used` 减少」**这个形态 | 同键先报 280 再报 120 时实测触发碰撞，但 ①②③ 三支示例全为**递增**（120→280 / 220→380 / 120→160），无一与递减形态对上。**方向结论仍正确**（属「< 真值」⇒ 与 ③ 同类），不会导致误判，只是分类标签不全。建议下轮补一句「也可能**减少**（计数回退/重置），方向同 ③」。 |
| **F2** | 信息 | Dev 称「共用 `EXPECTED_USED` 才**唯一**」略过头 | `EXPECTED_USED` 同样被 2 处断言引用，不是唯一**定位符**；真正价值是单一真相源 + doc 注释附着于函数。不影响正确性。 |
| **F3** | 信息 | Dev 自述触发块 sha `0381c3a0ff649648…` 无法复现 | 我用 4 种候选区间均未得该值；hash 依赖抽取区间。实质结论（内容零改动）已由我独立证实。 |
| **F4** | 信息 | 「不覆盖异步补记」的理由依赖**当前代码形状** | 两个 adapter 现为同步 `Mutex<Vec>` 克隆，但这是实现事实而非结构性保证；将来引入流式用量源会静默失效。 |

## 8. 我自己引入又清掉的问题（如实登记）

| # | 事件 | 处理 |
|---|---|---|
| E1 | 我最初把 probe 文件直接写进 `tests/`，**可能污染正在跑的全量计数** | 立即 `mv` 出 `tests/`；全量重跑是在 probe 不在 `tests/` 的状态下进行的，计数可信 |
| E2 | probe 初版 `TableMeta` 缺 `column_count`/`commented_count`、`SchemaReport` 无 `Default` ⇒ **编译失败** | 照抄真实测试的 `schema()`/`table()` 修正后重跑；**这两次编译失败我没有计入任何变异判定** |
| E3 | probe 的两条**期望值是我算错的**（① 误以为 reason 全文不含「费用闸」——碰撞文案本身含该词；② 跨 round 的会话累计基数估错，正确是 300→400/500） | 改为断言 reason 的**开头**、并按真实累计重算数字后复跑 ⇒ 2 passed / 1 failed（failed 即 F1 的探测器） |
| E4 | 一次 python 正则用 `re.S` + `.*` 导致**灾难性回溯**、命令挂起 | 终止该任务，改写为按行定位后完成 |
| E5 | 三条变异期间临时改动 `src/agent/llm/mod.rs`、`src/agent/acp/mod.rs` | 每条变异后 `git checkout --` 还原；结束时 `git status -uall` 为空、`git diff 9d6c529` 无差异 |
| E6 | 临时 probe `tests/qa_tmp_req054_probe.rs` | 已移出 `tests/`，副本存于 `.review-workspace/verify-req054/` |

**环境类 vs 代码失败，严格二分**：本轮所有测试失败均为**运行期断言失败**；
`could not compile` / `error[E` 全程计数为 0（probe 的两次编译失败已隔离且不计入验收）。
DB 环境按开头正确方式设置（`dbname=ontol`），**未出现** `relative URL without a base`。
未测 `SELECT 1` p50（本机无 `psql` / `psycopg`）。**不声称任何稳定复现率。**

## 9. 纪律备忘建议（针对 BA 的 AC 原文）

同类第 4 次已确认，**根源仍在 BA 的 AC 原文**。建议追加一条硬约束：

> **AC 里凡出现「⇒ 真的出事 / 真的有资金缺口」，必须同时给出**
> ① 该方向在**单轮内**是否可达 ② 方向是 fail-open（多花钱）还是 fail-closed（少花钱/白跑）。
>
> 本轮反例：BA 的「② 整轮/会话口径累计 ⇒ 闸门可能提前停 ⇒ 真的出事」——
> 「整轮」口径下单轮内够不到（碰撞已先 `return`）；「会话」口径下确实可达，
> 但那是 **fail-closed（提前停 = 白跑）**，**不是资金事故**。
