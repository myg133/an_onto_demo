# QA 验收报告 · REQ-050

- 验收对象：`verify/REQ-050` @ `f43940f`（基线 `c082556`）
- 验收人：QA（Verifier）
- 日期：2026-10-10
- 结论：**PASS（带非阻断观察）**

---

## 0. 环境与口径声明

所有数字均标注口径与命令。求和一律用 `awk` 逐行累加（**未**使用 `paste -sd+`）。
DB 环境变量按约定设置：`ONTOK_TEST_DATABASE_URL` 取仓库根 `.env`，并把库名 `/notol` 替换为 `/ontol`。
未使用 `CARGO_TARGET_DIR`、未用 `--release`、未建 probe crate、未做任何 `ln -s`。
全程只在本 worktree 内工作，结束前已恢复为 `f43940f` 干净状态（`git diff f43940f` 为空）。

**偏离登记**：本会话的 Verifier 角色**没有 `write` / `edit` 工具**（可用工具仅 `read`/`bash`/`grep`/`glob`/task 类），
按纪律中「失败就用带引号 heredoc 兜底并登记偏离」的条款，本报告用带引号 heredoc 写入，未反复重试。

---

## 1. 结论摘要

| 项 | 结论 | 依据 |
| --- | --- | --- |
| Dev 推翻 BA 三个候选 | **成立**（四条逐条独立复现） | 见 §2 |
| 第 ④ 种方案（`attempt`） | **同意**，且优于我试过的其他替代 | 见 §5 |
| AC-001 判别力 | **成立**，签名 `left: 250` 精确复现 | 见 §3 |
| 既不翻倍又全进账 | **同时成立**，含交错场景 | 见 §4 |
| 两个残留风险 | 说清楚了；风险 1 属实但**可低成本代码化** | 见 §6 |
| `incomparable` 额外修复 | **正确且在 scope 内**，基线缺陷已实证 | 见 §7 |
| `#dup` 语义变化 | **仍钉住原意** | 见 §8 |
| 全量 / 红线 / 产物 sha | **全部通过**，与 Dev 声称逐位一致 | 见 §9 |

**唯一非阻断发现（F1）**：两个真实 adapter 里 `attempt` 的编号逻辑**零测试覆盖**，
三个相关变异（M3/M3b/M4）全部存活。这正是 Dev 主动登记的残留风险 1，目前**只能靠文档约束**——
但我验证了它**可以**用约 6 行编排层守卫变成代码强制（§6.1）。

---

## 2. Dev 的四点反驳，逐条独立核实

### 反驳 1：两个 adapter 上报的已经是增量，不是会话累计 —— **成立**

- ACP：`src/agent/acp/protocol.rs:421` `delta_since()` 第一行就是
  `let used = self.used.saturating_sub(used_before);` —— 真差分。
  `delta_to_report()`（`:456`）把 `d.used` 直接放进 `UsageReport.used`。
  baseline 在每次上报后被更新（`acp/mod.rs:509-510`），所以每条上报都是本组的增量。
- LLM：`src/agent/llm/mod.rs:439` `used: usage.total_tokens()`，来自**单次** `ChatResponse`
  （`:409` `fn record_usage(&self, group, resp: &ChatResponse)`）—— 请求级响应，本身即增量。

既有测试 `tests/test_acp_protocol.rs::session_cumulative_usage_is_converted_into_a_per_group_delta`
的断言逐字未动，仍然钉住「会话累计 → 每组增量」。**Dev 的「协议报累计 ⇒ 编排要替换」这条前提确实不成立。**

### 反驳 2：① 朴素累加会把账算成 3 倍 —— **成立（实测 330）**

结构依据：`orchestrate.rs:638` `for group in &groups {` … `:774` `stage.budget.absorb(&agent.usage(), …)`
—— `absorb` 在**每组之后**调用，传入 `agent.usage()` 返回的**全量快照**。

**独立跑了「朴素累加」变体（M5，把 `absorb` 改成每次把快照里每条再加一遍）**，
`cargo test --test test_budget_retry_accounting`：
```
test_non_retrying_run_keeps_its_exact_numbers ... FAILED
assertion `left == right` failed: 不重试 ⇒ 每组一笔 ⇒ 120 + 90 = 210
  left: 330
 right: 210
```
**`left: 330` 与 Dev 声称的 A120+B90⇒330 逐位一致。** 口径：`cargo test --test test_budget_retry_accounting`，
2 组不重试各 1 笔，A=120、B=90；第 1 组后 running=120，第 2 组 absorb 再加快照和 210 ⇒ 330。

同一次 M5 下另有 4 条红（`test_budget_retry_accounting` 2 passed / 5 failed），
其中 `test_rereading_the_same_snapshot_does_not_double_count` 也被杀 —— 说明该测试对「无脑累加」有真判别力。

### 反驳 3：③ 按 log 位置/游标汇总会撞既有测试 —— **成立（实测 2×）**

独立跑了「游标建键」变体（M6，键改为 `(group_id, 快照内下标)`）：
`cargo test --test test_agent_rebuild` → **24 passed / 2 failed**
```
test_budget_stops_the_run_and_records_skipped_groups_separately_from_failures ... FAILED
  left: ["table:t_customer", "table:t_region"]   right: ["table:t_region"]
test_budget_within_limit_never_reports_a_stop ... FAILED
assertion `left == right` failed: 跑满 3 组的累计用量
  left: 60   right: 30
```
`60 = 2 × 30`，正是 `#dup` 重复上报被当成两笔。**Dev 的反驳成立。**

### 反驳 4：`group_id` 不进 serde / 提案 / CLI —— **成立（但需一处澄清）**

- `UsageReport`（`src/agent/mod.rs:267`）确实 derive 了 `Serialize/Deserialize`，
  但全仓 `grep -rn "UsageReport"` 显示它只出现在 `acp/mod.rs`、`acp/protocol.rs`、`llm/mod.rs`、
  `orchestrate.rs:401`（私有 `absorb` 的入参）与测试替身里；`grep -rn "serde_json::to_|to_value|to_string_pretty" src/`
  过滤 usage/budget/stage **零命中** —— 没有任何地方序列化它。
- `BudgetReport`（`orchestrate.rs:350`）与 `StageReport`（`:305`）只 derive `Debug, Clone, Default`，**没有 Serialize**。
- `by_report` 是私有字段，编排之外无从访问。

**需要澄清的一点**：Dev 说「不进 CLI」，就 `UsageReport.group_id` 而言准确；但
`stage.budget` **确实**会进 CLI 输出（`src/bin/ontok-propose.rs:307-423` 打印
`limit_used`/`used`/`cost`/`reason`/`skipped_groups`）。也就是说**本次修复是用户可见的**，
闸门停下来的原因里会带真实累计（AC-002 已断言 `reason` 含 280/200）。
这不推翻 Dev 的论点（他论证的是 `UsageReport.group_id` 不会与 `skipped_groups` 的真 group_id 混成两套 id），
但「不进 CLI」这句若被读成「这次改动对用户不可见」就是错的。

### 「有没有更好的第五种？」—— 我试过两种，结论

- **⑤-a 按 `(group_id, used, cost)` 建键**：看起来很自然（天然吸收 `#dup`）。
  **但更差**：两次重试花费**相同**时（120 → 120，极常见）会落进同一个键 ⇒ 又被吃掉一笔。
  直接违反 REQ-050 本身。**否决。**
- **⑤-b 编排层碰撞检测**（见 §6.1）：在 `absorb` 里检查「同一个 `(group_id, attempt)`
  却带着**不同的** `used` 再来一次」⇒ 判定 adapter 违约。**这是我推荐的补充，不是替代。**

---

## 3. AC-001 判别力 —— 成立

### 3.1 独立复现 Dev 的变异

M1：`absorb` 键 `r.attempt` → 字面量 `0`（`orchestrate.rs:404`）。
**编译通过**（`cargo build --lib --tests` 无 error），因此是有效变异执行。

`cargo test --test test_budget_retry_accounting` → **3 passed / 4 failed**（共 7 条运行）。
与 Dev 声称的 3/4 逐位一致。

### 3.2 `left: 250` 签名核实

M1 下四条失败的实测输出：
```
test_retry_spend_is_all_counted_not_just_the_last_attempt   left: 250  right: 370
test_budget_gate_trips_on_the_real_total_...                left: 250  right: 280
test_budget_gate_trips_even_when_limit_is_between_...        left: 250  right: 370
test_retry_with_two_currencies_still_reports_no_cost        left: 250  right: 370
```
**`250 = 160（t_a 的最后一笔，覆盖把 120 顶掉）+ 90（t_b）** —— 正是「只留最后一笔」的覆盖签名。确认。

### 3.3 Dev 最重的一条 —— 成立

同一次 M1 下，既有 4 个预算/协议套件**全绿**：
`test_agent_rebuild` 26 + `test_acp_protocol` 18 + `test_llm_fallback` 12 + `test_budget_report_truth` 3 = **59/59 passed**。
⇒ **这个资金缺口在旧套件里确实完全没有判据。** 这是 Dev 这次最有价值的发现之一。

### 3.4 绕过尝试（要求补做的三项）

| 变异 | 内容 | 结果 | 判定 |
| --- | --- | --- | --- |
| **M3** | `LlmAgent::record_usage` 的编号整段替换为 `let attempt = 0u32;` | **存活** | 9 个 agent 相关套件 **168 passed / 0 failed** |
| **M3b** | `AcpAgent::record_usage` 同上 | **存活** | 同上 168 全绿 |
| **M4** | ACP 编号错位（`+ 1`，从 1 起） | **存活** | 同上 168 全绿 |
| **M7** | `as u32` 溢出 | 不可达 | 见下 |

口径（9 套件 168 条 = 28+18+36+26+3+7+12+26+12）：
`test_acp_extraction` 28、`test_acp_protocol` 18、`test_agent_orchestration` 36、`test_agent_rebuild` 26、
`test_budget_report_truth` 3、`test_budget_retry_accounting` 7、`test_connect_retry` 12、
`test_extraction_agent` 26、`test_llm_fallback` 12。

**M7（u32 溢出）论证**：`acp/mod.rs:507` 与 `llm/mod.rs:435` 都是 `count() as u32`（截断而非饱和）。
要让单组编号溢出需要 `2^32 ≈ 42.9 亿` 条 `UsageReport`；按每条约 96 字节 + group_id 堆分配估算，
需约 **400 GB 量级内存**。若真发生，后果是编号回绕 ⇒ 键碰撞 ⇒ 少计，
**失败模式与残留风险 1 完全相同**，且没有任何代码守卫。判定：**理论存在、实际不可达**，登记为已知洞，不阻断。

**存活变异逐条论证**：M3/M3b/M4 存活**不是判据失效，而是判据根本不存在**——
`tests/test_budget_retry_accounting.rs` 用的是自己的替身 `RetryingUsageAgent`（`:195` 自己实现 `usage()`），
它重写了一遍「按已记笔数编号」的逻辑，**从不调用真实 adapter 的 `record_usage`**；
而 `test_acp_protocol.rs` 是直接调 `delta_to_report(…, 0)`，把 attempt 当**字面量**传，也不经过编号逻辑。
所以真实 adapter 里那段编号代码是**零覆盖**的。这不是「测试没抓住已知的对实现」，而是「这段代码没有任何测试」。

---

## 4. 「既不翻倍又全进账」—— 同时成立（含交错场景）

我写了**不复用 Dev 替身**的独立探针（临时 `tests/zz_qa_interleave_probe.rs`，验收后已删除），结果 5/5 通过：

| 探针 | 场景 | 期望 | 实测 |
| --- | --- | --- | --- |
| `qa_interleaved_reread_plus_retry_counts_every_spend_exactly_once` | **3 组 × 各重试 1 次 = 6 笔**，快照被读 **3 次** | 780 | **780** ✅ |
| `qa_three_attempts_per_group_all_counted` | 2 组 × 各 **3 次**尝试（`input.attempts = 3`） | 210 | **210** ✅ |
| `qa_no_retry_baseline_unchanged` | 不重试对照组 | 210 | **210** ✅ |
| `qa_interleaved_mixed_currency_stays_none` | 交错 + USD/EUR 混币种 | used 780 且 cost=None | **780 / None** ✅ |
| `qa_broken_adapter_always_zero_undercounts` | 坏 adapter（恒 0）对照组 | 250（重现少计） | **250** ✅ |

780 = 120+160+200+180+50+70，**6 笔各算一次，一次不多**。
探针内还断言了 `a.reads() == 3`（快照确实被读了 3 次），确保这条结论不是「没重读所以没翻倍」的假阳性。

**关于「A 组第 1 笔 → 读快照 → A 组第 2 笔 → 再读快照」这个具体交错**：
我核对过 `orchestrate.rs`，`drive_group` 在**组内**把重试跑完（`:653` 唯一调用点），
`absorb` 只在**组末**（`:774`）调用。所以「同组第 1 笔之后、第 2 笔之前读一次快照」在当前编排里**不会发生**。
真正会发生的交错是「A 组 2 笔完成 → absorb → B 组完成 → absorb（重读 A 的两笔）」，
上表第 1 行就是这个形状，且用 3 组 × 3 次读快照做了推广。

**能否找到「两条腿都记账但一条被吃掉」的复发路径？**
在**编号正确**的前提下，我没有找到：
- 翻倍要求同一 `(group_id, attempt)` 携带**不同** `used` 两次 ⇒ 需要 adapter 改写已入账的条目；
  实测 `grep` 确认两个 adapter 的 `usage` 日志**只 append、从不 clear/重置**
  （`grep "usage\.lock()\.clear\|usage = Mutex\|\*self\.usage\.lock() = "` ⇒ 零命中），
  且 `usage()` 返回 `clone()`（`acp/mod.rs:465`、`llm/mod.rs:382`），调用方拿不到可变引用。
- 少计要求两笔**不同**的花费落进同一个键 ⇒ 需要 `attempt` 冲突；
  `attempt = count(已记同组笔数)` 在 push 前取值，**严格单调递增** ⇒ 同组内不可能冲突。
- 唯一剩下的复发路径就是残留风险 1（adapter 恒报 0），已被 M3/M3b 实证并由探针第 5 行量化（250）。

---

## 5. 两个残留风险

### 5.1 「新 adapter 忘编 attempt ⇒ 无法被代码强制」

**「无法被代码强制」这句不准确。** 现状属实：`require_budget`（`orchestrate.rs:471-493`）
只看 `input.budget.is_some()` 与能力标签 `CAPABILITY_REPORTS_USAGE`，**完全不看 `attempt`**，
所以一个恒报 0 的新 adapter 能顺利通过入口校验（实证：M3/M3b/M4 三个变异全存活）。

**但它可以被强制**（§6.1）。Dev 说「代码无法强制、只能靠契约文档」，在他的实现范围内成立；
作为对 REQ-050 整体方案的判断，这一条偏保守了。

### 5.2 「重试两次之间没有闸门」—— **既有的，不是本 REQ 引入**

独立核实 `git show c082556:src/agent/orchestrate.rs`：

| | 基线 `c082556` | HEAD `f43940f` |
| --- | --- | --- |
| 闸门判 `stage.budget.stopped` | `:602`（`for group in &groups` 之后立刻） | `:641`（同位置） |
| `drive_group` 调用 | `:614` | `:653` |
| `stage.budget.absorb(...)` | `:735`（循环体末尾） | `:774`（循环体末尾） |

结构完全同构：**闸门每组判一次，且在跑这一组之前；组内重试之间没有闸门**。
且 `orchestrate.rs` 里 `drive_group` 只有 `:653` 一个调用点（`grep -n "drive_group"` 确认）。

⇒ **登记为既有已知洞**（不是本 REQ 的缺陷）。本次 REQ 的保证边界是准确的：
「账算全、闸门按全账判」，**不**保证「第二次尝试前就拦住」。
要真正闭合它需要把 `absorb` 下沉进 `drive_group` 的重试循环 —— 那是独立的一笔需求。

---

## 6. `incomparable` 那个额外修复 —— 正确，且在 scope 内

### 6.1 基线缺陷实证（`c082556` 真实代码上跑的）

我在 worktree 里把 `src/` `git checkout c082556` 回基线，跑一个 3 组混币种探针
（USD 0.010 → EUR 0.020 → USD 0.030），实测输出：

```
BASELINE cost = Some(MoneyAmount { amount: 0.03, currency: "USD" })
BASELINE used = 6
```

**假钱数确认存在**：真实花费是 0.010 USD + 0.020 EUR + 0.030 USD，基线却报 **0.03 USD** ——
第一组的 0.010 USD 被静默丢弃、EUR 被当作不存在，只剩第三组的钱被当成总额报出去。
`used = 6`（1+2+3）是对的，所以缺陷**只在 cost 这一路**。
（此探针在验收后已删除，worktree 已恢复干净。）

### 6.2 新键方案下 2 组即可触发

变异 **M2**（把 `totals()` 的 `incompatible` 守卫去掉、恢复旧的四臂 match）：
`cargo test --test test_budget_retry_accounting` → **6 passed / 1 failed**，
唯一被杀的是 AC-003 那条，实测断言消息：
```
USD 与 EUR 不可比 ⇒ cost 必须是 None；… 实际：Some(MoneyAmount { amount: 0.09, currency: "USD" })
```
**`Some(0.09 USD)` 与 Dev 声称的逐位一致**，其余 6 条不受影响 —— 与 Dev 说法一致。

口径说明（这一点 Dev 的表述略含糊，这里精确化）：基线（键 = `group_id`）下重试会覆盖，
所以需要 **3 个组**才能凑出「Some → 不可比 → None → 被救回」这个序列；
**新键方案**下重试多出一个键，因此 **2 个组**（其中一个重试，共 3 个条目）即可触发。M2 实测正是 2 组场景。

### 6.3 是否超 scope？—— **不超，是 AC-003 在新方案下的必要条件**

AC-003 要求「两次尝试币种不同 ⇒ `cost == None`」。重试产生第二个条目之后，
旧的「`None ⇒ Some(c)` 救回」臂**恰好就在这条路径上被触发**（6.2 实测）。
没有这个守卫，AC-003 在新方案下**必然红**。所以它不是顺手修的额外项，而是 AC-003 成立的前提。

### 6.4 修复后 `used` 累加是否仍正确？—— **正确，未被短路**

`orchestrate.rs:435-437`：
```rust
for (u, c) in self.by_report.values() {
    used = used.saturating_add(*u);      // ← 在任何 continue 之前
    let Some(c) = c else { continue };    // ← continue 发生在 used 累加之后
    if incomparable { continue }
```
静态：`used` 的累加在任何 `continue` 之前。
动态：AC-003 那条测试在**混币种**下仍断言 `used == 370` 并通过；
我的交错探针在 USD/EUR 交错下也断言 `used == 780` 并通过。
**两条独立证据确认 `incomparable` 没有短路 `used`。**

### 6.5 我推荐的补充（第五种方案，验证可行）

在 `absorb` 里加碰撞检测：同一 `(group_id, attempt)` 却携带**不同的** `used` 再来一次
⇒ 一定是 adapter 没编 attempt ⇒ 契约违规。

**我实测了它**：
- **零假阳性**：加上守卫后，9 个 agent 相关套件 **168 条全绿**（含 `#dup`、含新文件 7 条）。
  这很关键 —— 快照重读时同键同值，不会误报。
- **能真的抓到**：`catch_unwind` 探针里，一个同键不同值的坏 adapter 触发了检出（断言 `is_err()` 通过）。
- **但**：即使加了守卫，**现有测试仍抓不到 M3** —— 因为没有任何测试让真实 adapter 走同组重试。
  也就是说守卫能防**生产事故**，但要防**回归**还得补一个覆盖真实 adapter 编号逻辑的测试。

---

## 7. 两个测试文件的连带改动 —— 非放松断言

`git diff --numstat c082556 f43940f -- tests/`：`test_acp_protocol.rs` +10/-5、`test_agent_rebuild.rs` +9/-0、
新增 `test_budget_retry_accounting.rs` +393/-0。

`tests/` 下**全部被删的行只有 5 行**，逐条核对：
- 4 行是 `delta_to_report(...)` 的签名更新（多一个 `attempt` 实参）；
- 1 行是 `orchestrate.rs` 注释措辞的更新（`test_acp_protocol.rs:276`）。

**没有删除任何 `#[test]`、没有删除任何断言、没有新增 `#[ignore]`**
（`git diff | grep -E "^\+.*#\[ignore\]"` 零命中）。

### `#dup` 的语义变化 —— **仍钉住原意**

原保证：「同一 `group_id` 多条上报 ⇒ 不翻倍」（因为旧实现按组覆盖）。
现保证：「同一 `(group_id, attempt)` 多条上报 ⇒ 不翻倍」。

**这是收窄，但收窄掉的不是要害**。理由（两条实证）：
1. 该替身制造重复的真实物理场景是**「同一会话被读两次」**（见 `test_agent_rebuild.rs:334-336` 原注释），
   而快照重读产出的正是**完全相同的 `(group_id, attempt)` 键**。语义收窄后，该场景**一对一映射**到新键上，判定力不减。
2. 关键反证：**M5（无脑累加）仍然杀掉 `#dup` 相关的两个测试**（`test_agent_rebuild` 2 failed，`left: 60 right: 30`）
   —— 说明「同键不许翻倍」这条性质被牢牢钉住，且对「累加型实现」仍有判别力。

**诚实的补充**：语义收窄确实带来一个后果 ——
若将来某个 adapter 恒报 `attempt: 0`，`#dup` 测试**不会**报警（它会通过，而实际在少计）。
这与 §3.4 的 M3/M3b 存活是同一个根因，已登记为 F1。

---

## 8. 全量测试、基线对照、红线、产物 sha

### 8.1 全量（`cargo test --no-fail-fast`，DB 环境变量按 §0 设置）

| 版本 | 通过 | 失败 | 忽略 | 耗时 |
| --- | --- | --- | --- | --- |
| HEAD `f43940f` | **566** | **0** | **1** | ~7 min |
| 基线 `c082556` | **559** | **0** | **1** | **ELAPSED=409s** |

口径：`grep "^test result:" <logfile> | awk '{...}'` 逐行累加 40 条 `test result` 行（HEAD）/ 39 条（基线）。
恒等式核对：**559（旧套件）+ 7（新增文件 `test_budget_retry_accounting`）= 566** ✅
新增文件实测 `running 7 tests`。

那 1 条 ignored 在基线与 HEAD **是同一条**：`todo_identifier_defenses_have_own_assertions`
（标注「缺口清单，不是可运行断言」），**非本次引入**。

### 8.2 红线核查 —— 全部通过

`git diff --name-only c082556 f43940f` 共 8 个文件：
```
src/agent/acp/mod.rs      src/agent/acp/protocol.rs   src/agent/llm/mod.rs
src/agent/mod.rs          src/agent/orchestrate.rs
tests/test_acp_protocol.rs  tests/test_agent_rebuild.rs  tests/test_budget_retry_accounting.rs
```
- `tests/drop_rate.rs` —— **不在列表** ✅
- `tests/test_fk_naming.rs` —— **不在列表** ✅
- `src/domain/view.rs` —— **不在列表** ✅
- `test_humanize_still_refuses_unknown_columns`（位于 `src/domain/view.rs:1254`）——
  `git diff ... -- src/domain/view.rs tests/test_view.rs` **输出为空**，一条断言都没动 ✅
- 无测试被删除、无 `#[ignore]` 新增 ✅

### 8.3 产物 sha —— 两个都精确 MATCH

命令（从 crate 根 `verify-REQ-050` 运行，`--out` 在 `--stats` 之前，不传 `--names`）：
```
./target/debug/ontok-propose \
  --schema <root>/.review-workspace/fix-props/schema.json \
  --project extracted \
  --out  <root>/.review-workspace/verify-req050/proposal.json \
  --stats <root>/.review-workspace/verify-req050/stats.json
```

| 文件 | 实测 sha256 | 期望 | |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | ✅ |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | ✅ |

**输入 `schema.json` 未被触碰**：运行前后 sha 均为
`b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019`。

---

## 9. 发现但未修的问题

### F1（中）—— 两个真实 adapter 的 `attempt` 编号逻辑零测试覆盖

- **触发条件**：任何人改动/删除 `src/agent/acp/mod.rs:501-508` 或 `src/agent/llm/mod.rs:433-436`
  里的编号逻辑（哪怕只是漏写、错位、写死）。
- **影响**：回到 REQ-050 修复前的**静默少计**，闸门按偏低的数放行 —— 资金风险。
- **证据**：M3 / M3b / M4 三个变异**全部存活**，9 个 agent 相关套件 168 条全绿。
- **为什么没被抓住**：新增文件用的是替身 `RetryingUsageAgent`（自己实现编号）；
  `test_acp_protocol.rs` 直接把 `0` 当字面量传给 `delta_to_report`，绕过了编号逻辑。
- **建议（非阻断）**：补一个**走真实 adapter 的同组重试**测试；
  并按 §6.5 在编排层加碰撞检测，把「契约违规」变成可检出的运行期失败。

### F2（低）—— 既有死代码，非本次引入

`tests/test_agent_rebuild.rs:337` 的 `let report = UsageReport { … }` 构造后**从未被使用**
（真正驱动 `usage()` 的是下一行 `st.push((format!("{}#dup", …)))`）。
`unused variable: report` 警告在**基线构建输出里同样出现**，本次 REQ 只是在该死结构体字面量里多加了 `attempt: 0`。
基线代码核对：`git show c082556:tests/test_agent_rebuild.rs` 的 337-344 行确认 `report` 在其后从未被引用。
⇒ **既有遗留，本次未引入**，登记备查。

### F3（提示）—— 新增 7 条测试**不能**区分 Dev 的 ④ 与 BA 的 ③

- M6（游标建键，BA 的候选③）在 `test_budget_retry_accounting` 里 **7/7 全过**，
  只被**既有的** `#dup` 测试（`test_agent_rebuild`）挡住。
- 含义：新增测试钉的是「重试的账要全进」，而「同一快照不许翻倍」这条仍然**只能靠既有测试**。
  Dev 在 AC-004 里正是引用了那条既有测试 —— 这个选择是对的，但值得写进交接记录，
  否则后人可能误以为新文件已经独立覆盖了两条腿。

### F4（提示）—— `test_rereading_the_same_snapshot_does_not_double_count` 判别力有限

它与 `test_non_retrying_run_keeps_its_exact_numbers` 用的是同一组数据（`[120]`/`[90]`），
只是少了费用断言。但实测 **M5 会杀掉它**，所以它确实覆盖了「快照重读 × 不许翻倍」这一腿，
并非纯冗余。保留即可，仅记录其判别力弱于 AC-001。

### F5（提示）—— `as u32` 截断不可达

`acp/mod.rs:507` / `llm/mod.rs:435` 是 `count() as u32`（截断），溢出需单组 42.9 亿条上报
（约 400 GB 量级内存）。理论后果 = 键碰撞 = 少计，与 F1 同类。登记为已知洞，**不建议**为此改代码。

### F6（提示）—— 闸门粒度（既有洞）

见 §5.2。基线与 HEAD 结构同构，非本 REQ 引入。建议独立立项：`absorb` 下沉进 `drive_group` 重试循环。

---

## 10. 我自己引入又清掉的问题（如实登记）

1. **`/usr/bin/time` 不存在** —— 首次全量命令直接失败（环境类失败，非产品失败）。
   改用 `date +%s` 计时，基线跑出 `ELAPSED=409s`。
2. **M2 第一版编译失败** —— 我把 `c` 在 `let Some(c) = c else` 之后又写了 `.as_ref()`，
   `error[E0599]`。按纪律「编译失败不算变异执行」，我没有记为变异结果，
   重写为与基线同构的写法后重跑，M2 的结论（6 passed / 1 failed）来自**修正后**的编译通过版本。
3. **我的交错探针自己有 3 个 bug**（不是产品缺陷）：
   - 用 `.unwrap()` 取用量，坏组触发 panic —— 真实 adapter 是 `.ok_or_else(...)` 返回 `Err`；
   - 忘了 `drive_group` 的上限就是 `input.attempts`（默认 2），3 次尝试根本达不到；
   - 函数体内误用 `//!` 内联文档注释（`error[E0753]`）。
   三处均已修正，**最终 5/5 通过**；上表数据来自修正后的版本。
4. **碰撞探针首次编译失败** —— 手写 `ExtractionGroup` 缺 4 个字段；
   改为在 `extract_group` 内部一次性产出冲突的两条上报，绕开该构造。
5. **偏离登记**：临时 Rust 测试文件**必须**放在 `tests/` 下（cargo 的集成测试目录约定），
   无法放进 `.review-workspace/verify-req050/`。所有临时文件
   （`zz_qa_interleave_probe.rs`、`zz_qa_baseline_probe.rs`、`zz_qa_collision_probe.rs`）
   已删除，`git status --short` 与 `git status --porcelain -uall` 均为空。
6. **交付纪律**：worktree 已恢复到 `f43940f` 干净状态（`git diff f43940f --stat` 为空）；
   恢复后复跑 5 个预算/协议套件仍 **66/66 全绿**。

---

## 11. 最终判定

**VERDICT: PASS（带非阻断观察）**

- 需求本体（同组重试不再少计、闸门按真实累计判）**已正确实现**，
  AC-001~AC-004 四条判据**全部有可执行的变异验证**支撑，判别力经独立复现确认。
- Dev 对 BA 三个候选的**四点反驳全部成立**，且我用独立变异给出了硬证据（330、2×、serde 不可达）。
- Dev 的第 ④ 种方案（`attempt`）**正确且优于我试过的其他替代**；
  我未能找到能同时满足「不翻倍 + 全进账 + 不引入新的少计路径」的更优方案。
- `incomparable` 额外修复**在 scope 内且必要**，基线缺陷已实证。
- 唯一需要跟进的是 **F1（adapter 编号逻辑零覆盖）** —— 它是 Dev 已诚实登记的风险，
  不构成 FAIL，但建议在 REQ-050 结项记录里连同 §6.5 的守卫方案一并留档。
