# REQ-050 Dev 分析与实现记录

> session: ba-archlinux-80180-20261007-231834
> worktree: `/home/myg133/Projects/Anchnet/Ontology/feature-REQ-050`
> 基线: `c082556`

---

## 1. 复核 BA 给的行号（不是照抄）

| BA/简报说法 | 实测 | 结论 |
| --- | --- | --- |
| `:367` `latest_by_group` | `orchestrate.rs:367` 一致 | ✅ |
| `:381-383` `absorb` 里 insert 覆盖 | `orchestrate.rs:381-383` 一致 | ✅ |
| `:401-404` `totals()` 求和 | `orchestrate.rs:401-415` | ✅ |
| `:701` `drive_group` 用 `group.group_id` | **`orchestrate.rs:701` 是 `GroupFailure{group_id}`，不是 `drive_group`** | ❌ 行号指错 |

真正的 `drive_group` 在 `src/agent/mod.rs:435`，它**不**产生 `UsageReport`；
`UsageReport` 的 `group_id` 由两个 adapter 在 `record_usage` 里写
（`acp/mod.rs:500`、`llm/mod.rs:424`），两边都是 `group.group_id.clone()`。

`latest_by_group` 全仓触点复核：**确实只有 2 个**（`:381` 写、`:404` 读），
无消费者 —— 简报这点成立。

---

## 2. ★ 关键发现：简报的前提不完整（这决定了修法）

### 2.1 两个 adapter 上报的**已经是增量**，不是会话累计

- ACP：`acp/protocol.rs:448 delta_to_report()` → `used = delta_since(baseline)`
  （会话累计 − 上一次累计），注释自写「**增量** → REQ-022 的协议中立上报」。
- LLM：`llm/mod.rs:426 used = usage.total_tokens()` —— 请求级响应，本组增量。

所以「协议报的是累计值，所以编排要替换」这条**理由本身是错的**（`mod.rs:270`
与 `orchestrate.rs:373-378` 的文档都已过时/自相矛盾 —— ACP 侧文档自己写的是增量）。

⇒ **「替换」对两个 adapter 都是错的**：重试的第二笔会顶掉第一笔。

### 2.2 但直接把 insert 改成累加（修法 ① 朴素版）会**灾难性翻倍**

`agent.usage()` 返回的是**全量历史快照**（`acp/mod.rs:464`、`llm/mod.rs:380`
都是 `clone()` 整个 `Mutex<Vec<UsageReport>>`），而编排在
`orchestrate.rs:735` **每抽完一组就读一次全量**。

朴素累加的轨迹（组 A 用 120，组 B 用 90）：
- 组 A 后 absorb([A:120]) → 120
- 组 B 后 absorb([A:120, B:90]) → **A 被第二遍计入** → 240+90 = 330（应为 210）

⇒ 修法 ① 朴素版会把账算成 3 倍，比少计更危险。

### 2.3 ★★ 拦路虎：现有测试**显式钉死「替换」语义**

`tests/test_agent_rebuild.rs:292-382` 的 `UsageReportingAgent`：
- `:345` 往 log 里塞一条 `{group_id}#dup`、**同组同值**的重复上报；
- `:373-375` 回报时把 `#dup` 后缀去掉 ⇒ 编排看到的是**同一 group_id 的两条上报**；
- `:343-344` 注释：「编排按组取最新 ⇒ 不受影响；若它按『累加所有上报』实现，
  这里就会翻倍。」

`test_budget_stops_the_run_and_records_skipped_groups_separately_from_failures`
（`:393`）的期望值就建立在「同组同值两条 ⇒ 记一次」上。

⇒ **任何「按 log 位置 / 游标 / 序号」的方案都会让这条测试翻倍 ⇒ 红。**
这不是可以顺手改掉的测试（红线：不许放松既有断言）。

### 2.4 因此：只按 `group_id` 或按「log 位置」都**无法**同时满足
「重试两笔都算」与「同组同值两条不翻倍」—— 二者在数据上**不可区分**。

⇒ 必须把「这是同一组的第几笔」变成**一等事实**，而不是靠数组位置去猜。

---

## 3. 选定修法：④（BA 三个候选之外）+ 说明

**做法**：给 `UsageReport` 加一个显式的 `attempt: u32`（该组第几次上报，
从 0 起，`#[serde(default)]`），两个 adapter 在 `record_usage` 里按
「该组已上报几条」编号；编排的 map 键从 `String` 改成 `(String, u32)`。

**为什么不是 ①②③**：

| 候选 | 为什么不选 |
| --- | --- |
| ① `absorb` 改累加 | 违反 2.2：快照是全量历史，朴素累加 ⇒ 3 倍过计；且违反 2.3：既有测试直接钉死「不翻倍」 |
| ② `group_id` 带轮次 | 能让两笔都进账，但把「第几笔」编码进**标识符**：① 破坏「group_id 是组标识」的语义；② `skipped_groups`/`failed_groups` 用的是**真 group_id**，两套 id 混流；③ 靠改 id 传状态是隐式约定。**（好消息：BA 担心的泄漏其实不存在，见 §4）** |
| ③ 每次尝试单独记账 + 编排汇总 | 方向对（就是本方案），但若「汇总」按 log 位置做 ⇒ 同样撞 2.3。必须落到「显式 attempt 标识」才成立 |

**本方案同时满足 2.3 与 AC-001**：
- 快照被重复读：(A,0) 反复 insert 同值 ⇒ 覆盖 ⇒ 不翻倍 ✅ 既有测试保持绿
- 真正的重试：(A,0)=120 与 (A,1)=160 是**两个不同键** ⇒ 都进账 ⇒ 280 ✅

### 3.1 「两条腿都记账但一条被吃掉」能否复发？

**已封死的路径**：
- 同组重试（LLM/ACP）：adapter 各自按组编号 ⇒ 键不同 ⇒ 两笔都在。
- 快照重读：键相同、值相同 ⇒ 覆盖 ⇒ 不重复。

**残留风险（如实登记）**：
1. **新 adapter 忘了编 `attempt`** ⇒ 每条都是 0 ⇒ 退回今天的少计。
   这是本方案唯一的复发路径，靠文档 + 契约注释约束，**代码无法强制**。
   （`require_budget` 只看能力标签，不看这个。）
2. adapter 若把一条账**拆成多次上报**同一 attempt，会被覆盖（少计）——
   对齐 ACP 现状（每轮一次上报），未扩大。
3. `BudgetReport` 在**组粒度**判闸（`orchestrate.rs:735` 每组后判一次），
   重试的两次尝试之间**没有闸门** ⇒ 本次修复只能保证「账算全、闸门按全账判」，
   **不能**保证「第二次尝试前就拦住」。这是既有分层，非本需求引入。

---

## 4. 连带问题：group_id 是不是「一表一组、稳定 id」？

实测下游（`grep usage_reports / failed_groups / skipped_groups`）：

| 出口 | 数据来源 | 与 `UsageReport.group_id` 的关系 |
| --- | --- | --- |
| `failed_groups` | `GroupFailure.group_id`，来自 `mod.rs:450` / `orchestrate.rs:701` | **无关**，用的是组自己的 id |
| `skipped_groups` | `orchestrate.rs:603 group.group_id` | **无关** |
| 提案 `extracted_by.failed_groups` | 同上，经 `mod.rs:130` | **无关** |

**`UsageReport.group_id` 全仓只出现在 `BudgetReport` 那个私有 map 里**，
不进 serde、不进提案、不进 CLI。所以它泄漏面为空 ——
这也意味着修法 ② 的「泄漏风险」在本仓其实不成立（但仍不该选，理由见 §3）。

**结论**：三处确实都假定「一表一组、稳定 id」，
而我的方案**不改变任何 `group_id` 的形态**（只加了一个正交的 `attempt` 字段），
⇒ 对这三处**零影响**。

---

## 5. 改动清单

- `src/agent/mod.rs`：`UsageReport` 加 `attempt`；修正 `used` 的过时文档
  （「累计值」→「本组的增量」，与两个 adapter 的实际行为对齐）
- `src/agent/acp/protocol.rs`：`delta_to_report` 增 `attempt` 形参
- `src/agent/acp/mod.rs`：`record_usage` 编号该组第几次上报
- `src/agent/llm/mod.rs`：`record_usage` 同上
- `src/agent/orchestrate.rs`：map 键 → `(String, u32)`，字段改名 + 文档重写
- `tests/test_agent_rebuild.rs`：两处 `UsageReport` 字面量补字段（**不加断言、不放松**）

