# REQ-051 QA 验收报告

- **被验对象**：worktree `verify-REQ-051`，分支 `verify/REQ-051`，HEAD = `a55ffad`
- **基线**：`f43940f`（develop，父提交）
- **结论**：**PASS 带非阻断**（1 项非阻断发现 + 1 项诊断文案瑕疵）

---

## 0. 结论先行

三条变异**全部被独立杀死**，且**对照栏三条全部全绿** —— 这正是本需求要补的那个洞
（「判据根本不存在」）被真正补上的直接证据。碰撞检测在 6 个刁钻情形下**零假阳性**。
红线零改动，产物 sha 独立复现精确匹配，574/0/1 与基线 566/0/1 的差恰好 +8。

唯一实质发现（**非阻断**）：Dev 主动报出的 `group_id = ""` 残留风险，
其**结论可接受，但支撑论证有一处事实错误**（详见 §3）。

---

## 1. 三条变异真的全红吗？ —— **是，三条全红，对照栏三条全绿**

### 1.1 变异定义（我实际施加的改动）

| 变异 | 位置 | 施加内容 |
| --- | --- | --- |
| **M3** | `src/agent/llm/mod.rs:432` | `let attempt = 0u32;`（原为按 `group_id` 计数） |
| **M3b** | `src/agent/acp/mod.rs:503` | `let attempt = 0u32;` |
| **M4** | `src/agent/acp/mod.rs:503` | 计数结果 `+ 1`（错位递增，**不是**恒定值） |

> ⚠️ 我第一版把 M4 写成恒定 `1u32`，那样两次尝试会得到 `[1,1]` 而不是 Dev 描述的
> `[1,2]`。已改正为真正的「+1 递增」，下面的数据是改正后的。

### 1.2 结果表

| 变异 | 新套件 `test_attempt_numbering` | **对照栏（全部既有二进制）** | 杀死它的判据 |
| --- | --- | --- | --- |
| **M3** | **6 passed / 2 failed**（共 8） | **572 passed / 2 failed / 1 ignored**，唯一 FAILED 块就是新套件 ⇒ **既有套件 0 失败** | `llm_agent_numbers_every_attempt_of_the_same_group`（`left: [0,0]` vs `[0,1]`）、`llm_retry_spend_reaches_the_budget_as_two_separate_entries`（`left: 160` vs `370`） |
| **M3b** | **6 passed / 2 failed**（共 8） | **572 passed / 2 failed / 1 ignored**，唯一 FAILED 块就是新套件 ⇒ **既有套件 0 失败** | `acp_agent_numbers_every_attempt_of_the_same_group`（`left: [0,0]` vs `[0,1]`）、`acp_retry_spend_reaches_the_budget_as_two_separate_entries`（`left: 160` vs `370`） |
| **M4** | **7 passed / 1 failed**（共 8） | **573 passed / 1 failed / 1 ignored**，唯一 FAILED 块就是新套件 ⇒ **既有套件 0 失败** | `acp_agent_numbers_every_attempt_of_the_same_group`（`left: [1,2]` vs `[0,1]`） |

**计数口径**：数字来自 `cargo test --no-fail-fast` 输出中 `test result:` 行，
按 `awk`/Python 正则 `test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored`
对**全部 41 条二进制结果行求和**（含 0 测试的二进制）。日志分别落在
`.review-workspace/verify-req051/mut-{M3,M3b,M4}-full.log`。

### 1.3 对照栏 —— 本需求的核心价值，已独立验证

三条变异下，**全部 40 个既有测试二进制 0 失败**。三次全量里唯一出现 `FAILED`
的结果块都只有一块，且都是 `test_attempt_numbering`。

⇒ 前一轮「168 条全绿」的漏洞已被补上：现在既有套件对这三处编号代码**依然无能**，
只有新套件能叫红。**Dev 的对照栏说法成立。**

### 1.4 存活变异

**无存活变异。** 三条全部被杀死，无需逐条论证存活。

### 1.5 编译失败不算执行 —— 已严守

- 三条变异**全部编译通过**后才执行（M3/M3b/M4 的全量日志首段均为
  `Finished test profile ... Running unittests src/lib.rs`）。
- 我第一次跑 M4 全量时，因为自己的临时探针文件写坏了结构体字段而**编译失败**。
  我判定该次结果**无效**、删除探针后**重跑**，报告里的 M4 数据来自重跑。

---

## 2. 碰撞检测真的零假阳性吗？ —— **是，6 个刁钻情形零假阳性**

### 2.1 AC-003 两条边界独立复现（未变异时）

| 判据 | 实测 |
| --- | --- |
| `same_key_with_a_different_value_is_a_real_collision_and_must_trip` | `usage_collisions.len()==1`；信息点名 `table:t_a` / `120` / `160`；`stopped==true`；reason 含「碰撞」；`skipped_groups==["table:t_b"]` ✅ |
| `same_key_with_the_same_value_is_a_legitimate_reread_and_must_not_trip` | collisions 空；**`used==240`（不是 480）**；未停 ✅ |

两条**只差 `vary_used` 一个变量**（同一个 `FlatNumberingAgent` 替身），
我核实了这个「单变量」设计确实成立（`tests/test_attempt_numbering.rs:501` 与 `:539`）。

### 2.2 我自己构造的刁钻情形（临时探针，6/6 通过）

探针放在 `tests/zz_verify_probe_req051.rs`（Rust 集成测试必须在 `tests/` 下），
**已彻底删除**，`git status -uall` 为空。

| # | 情形 | 实测 | 判定 |
| --- | --- | --- | --- |
| E1 | `used` 同、`cost` 一个 `None` 一个 `Some(0.5 USD)` | `collisions=1, stopped=true` | ✅ 该触发 |
| E2 | **`NaN` vs `NaN`**，`used` 同 | `collisions=0, stopped=false, used=240` | ✅ **零假阳性** |
| E3 | `used` 同、`amount` 同（1.0）、**币种 USD vs EUR** | `collisions=1` | ✅ 该触发 |
| E4 | **超大 `u64`**：两组各 `u64::MAX-1` | `used=18446744073709551615`（= `u64::MAX`，**饱和不溢出**）、无 panic、`collisions=0`；同值重读也不触发 | ✅ |
| E5 | **`attempt` 错位 +1**，键 `(t_a,1)/(t_a,2)` 仍互异 | `used=560`（=120+160+120+160，完全正确）、`collisions=0`、`stopped=false` | ✅ 不误停 |
| E6 | **`group_id=""` + 整轮累计** | `collisions=1, stopped=true, used=200` | ⚠️ 见 §3 |

**结论：未发现假阳性。** Dev 关于「零假阳性」的论证成立，含 `NaN` 陷阱的处理正确
（`orchestrate.rs:427` 显式判 `x.amount.is_nan() && y.amount.is_nan()`），
超大 `u64` 因 `saturating_add`（`orchestrate.rs:528`）不溢出。

### 2.3 「热路径零额外 map 查找」—— 独立核实，成立

`grep -n "self.by_report" src/agent/orchestrate.rs` 只有两处：

- `orchestrate.rs:464-466` —— **唯一写入**，一次 `BTreeMap::insert`，返回值直接接到
  `bumped`（`:467 if let Some(prev) = bumped`）；
- `orchestrate.rs:527` —— `totals()` 里 `for (u, c) in self.by_report.values()`（只读）。

判据搭在 `insert` **本来就返回**的旧值上，**没有第二次 map 查找**。
`format!` 与 `fmt_money` 只出现在 `:469` 的碰撞分支内，**正常路径零额外分配**。

### 2.4 碰撞信息不会无限累积（我额外查的，Dev 未提）

- `orchestrate.rs:733` 一旦 `stopped` 就 `skipped_groups.push` + `continue`，
  **不再调用 `absorb`** ⇒ 碰撞不会跨组重复累加；
- 重试次数有界：`DEFAULT_ATTEMPTS = 2`（`orchestrate.rs:147`），
  单次 `absorb` 内同键最多产生 1 条。
  ⇒ `usage_collisions.len()` 的上界是常数，不是攻击面。**非问题。**

---

## 3. `group_id = ""` 残留风险 —— Dev 的**结论可接受，论证有一处事实错误**

### 3.1 两处文档契约 —— 独立核实，Dev 说对了

- `src/agent/mod.rs:268`：`/// 这笔用量归属哪一组（空串 = 整轮合计，不分组）。` ✅ **确实写了**
- `src/agent/mod.rs:288`：`/// 这一笔**自己**消耗掉的量（**增量**，不是会话累计）。` ✅ **确实是「单次尝试的增量」**

### 3.2 现状：生产路径不可能出现 `group_id = ""`

- `derive_group_id`（`src/agent/group.rs:271-273`）恒为 `format!("table:{table}")`，
  **永远非空**；`orchestrate.rs:272` 是唯一生产构造点。
- grep 全 `src/`：**没有任何一处**构造 `group_id: ""` / `String::new()`。
- 两个真实 adapter 的 `group_id` 都来自 `group.group_id`
  （`llm/mod.rs:437`、`acp/protocol.rs:459`）。

⇒ Dev 的「现有代码里没有这种用法」**成立且比他自己说的更强**：不只是「没这么用」，
而是**结构上到不了**。

### 3.3 ⚠️ 我发现的论证错误（非阻断，但要说清）

Dev 说：

> 「若真有 adapter 这么用，它本身**是不是已经踩了 REQ-050 要修的那个 bug**（整轮累计 + 编排每组重读 = 反复计入）？」

**这个前提是错的。** 我的 E6 实测给出了反证：

```
E6 group_id="" cumulative => collisions=1 stopped=true used=200
```

两组各累计 +100，最终 `used = 200` —— **这恰好是真实总花费，账是对的**。

原因：整轮累计上报只有**一个键** `("", 0)`，而 `BTreeMap::insert` 是**覆盖**语义。
所以 `totals()` 拿到的就是最后一次的累计值 = **正确的整轮总额**。
「N 倍重复计入」只在**每组各自一个不同 `group_id`** 时才发生（ACP 那种），
`group_id=""` 恰恰**不会**。

⇒ **Dev 引为理由的那个「已经踩了 REQ-050 的 bug」并不存在。**

### 3.4 那检测器在那里停下，是症状正确还是掩盖？

**是「掩盖了一个不会造成资金损失的场景」，不是「正确地抓住了一个资金 bug」。**

具体说：在 E6 里，账本数值本身是对的（200），检测器却把它判成
**「账本不可信，已停」**。这句话在这个场景里**与事实不符** ——
它停掉的是一笔**算得对**的账。

但我**不认为这构成阻断**，理由：

1. 它**确实是契约违规**：`used` 的契约明写「增量，不是会话累计」
   （`mod.rs:288`），整轮累计不是增量。检测器没有冤枉它。
2. **生产路径不可达**（§3.2），是纯假想场景。
3. fail-closed 方向**安全**：宁可停，不可继续在「说不清口径」的账上烧钱。
   即使停下，代价是「白跑」，不是「多花钱」。

**我与 Dev 的分歧点**：不在结论（都认为该停），而在**理由**。
Dev 用「反正已经少计/多计了」来正当化停下，这个理由不成立；
正确的理由是「**契约违规 + fail-closed**」，与账算得对不对无关。

### 3.5 我建议的判据改进（非阻断，供 BA 决定是否开后续小单）

不改行为，只改**诊断文案**，让「契约违规」与「账已错」这两件事分开说：

- 当前消息把「同键异值」一律叙述成「前一笔钱会被后一笔顶掉 —— 账本不可信」，
  在 E6 这种**账其实是对的**场景下会产生误导。
- 建议在 `orchestrate.rs:469` 的文案里补一句区分，例如：
  *「若该实现按整轮累计上报（`group_id` 为空串且 `used` 非增量），
  本条同样会触发 —— 那也是契约违规（`UsageReport::used` 要求增量），但账未必算错。」*
- 若希望**技术上**也不误停，可把判据收紧为「同键异值 **且** `group_id` 非空」，
  但这会**放过**空 `group_id` 的恒报 0 场景（M3 的变体），**我不建议**。
  **保持现状更安全。**

---

## 4. 「编号断言与记账断言拆开」—— **是更严谨，不是掩盖盲区**

### 4.1 事实独立核实：成立

M4 变异下 `test_attempt_numbering` 的结果是 **7 passed / 1 failed**，
失败的**只有** `acp_agent_numbers_every_attempt_of_the_same_group`（`left: [1,2]`）。
记账测试 `acp_retry_spend_reaches_the_budget_as_two_separate_entries` **通过**。

我的独立探针 E5 给出机理解释：`attempt` 错位 +1 时键是 `(t_a,1)`/`(t_a,2)`，
**仍然互异** ⇒ 两笔都进账 ⇒ `used=560` 完全正确 ⇒ 记账判据**在原理上就抓不到**这种错。

### 4.2 判断

**拆开是更严谨，且恰恰是补盲区的那一步。**

- 如果把两条断言**合并**成一条，测试**照样会红**（同一个测试、同一个变异），
  只是失败信息会变成一个笼统的总数不匹配，**定位能力下降**。
- 记账判据对 off-by-one 的「沉默」**不是拆分造成的，而是它的固有边界**：
  只要键互异，账就算对。任何编排层记账判据都无法区分「编号 0,1」与「编号 1,2」。
- 因此**唯一**能钉住「编号必须从 0 开始」这个契约的，就是独立的编号断言。
  Dev 加的正是这条。**这是补盲区，不是制造盲区。**

**会不会「永久沉默」？** 不会：套件里编号断言独立存在，M4 一测就红。
Dev 自己的说法「错位是契约破坏不是少计」也准确 —— 他没有把记账测试当作
「已覆盖错位」的证据去邀功，拆分是诚实的职责划分。

---

## 5. 全量与红线

### 5.1 全量（同会话，含基线对照）

| 运行 | 命令 | 结果 | 退出码 |
| --- | --- | --- | --- |
| 基线 `f43940f` | `cargo test --no-fail-fast` | **566 passed / 0 failed / 1 ignored**（40 个二进制） | 0 |
| HEAD `a55ffad` | `cargo test --no-fail-fast` | **574 passed / 0 failed / 1 ignored**（41 个二进制） | 0 |
| **差** | — | **+8 passed / 0 failed / 0 ignored** | — |

**+8 恰好等于新套件的 8 条** ⇒ 无回归、无删除、无改判。
Dev 的 574 与基线 566 两个数字**均复现**。

### 5.2 环境（运行期判据 vs 环境类失败，严格二分）

```bash
export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
  /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"
```

- URL 长度 59，形如 `postgresql://postgres:***@172.16.120.20:30432/ontol`（口令已遮蔽，
  **报告中不含口令**）。库名 `notol` → `ontol` 替换生效。
- 全程**零环境类失败**：DB 相关套件（`test_db.rs` 等）正常通过。
- 本机无 `psql` / `psycopg` ⇒ **`SELECT 1` 的 p50 未测**（环境边界，非产品缺陷）。
- 共享本体库全程**只读**，未执行任何 DELETE / TRUNCATE。

### 5.3 红线核查 —— 全部通过

| 红线 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` 零改动 | `git diff --stat f43940f HEAD -- ` 输出 **0 行** ✅ |
| `tests/test_fk_naming.rs` 零改动 | **0 行** ✅ |
| `src/domain/view.rs` 零改动 | **0 行** ✅ |
| `test_humanize_still_refuses_unknown_columns` 断言未放宽 | 该测试在 `src/domain/view.rs:1254`，文件整体未改 ⇒ 断言逐字未动 ✅ |
| 无测试被删除 | `git diff f43940f HEAD \| grep -cE '^[-].*#\[test\]'` = **0** ✅ |
| 无 `#[ignore]` 新增 | `grep -c '^+.*#\[ignore'` = **0** ✅ |
| 新增测试数 | `grep -cE '^[+].*#\[test\]'` = **8** ✅ |

唯一 ignored 测试是**既有**的 `tests/test_mcp_server.rs:553`
（`#[ignore = "缺口清单，不是可运行断言"]`），与本次改动无关。

### 5.4 产物 sha —— 独立复现，两个都精确 MATCH

复现命令（在 `verify-REQ-051` crate 根运行；`--out` 在 `--stats` 之前；不传 `--names`）：

```bash
./target/debug/ontok-propose \
  --schema   /home/myg133/Projects/Anchnet/Ontology/.review-workspace/fix-props/schema.json \
  --project  extracted \
  --out      /home/myg133/Projects/Anchnet/Ontology/.review-workspace/verify-req051/proposal.json \
  --stats    /home/myg133/Projects/Anchnet/Ontology/.review-workspace/verify-req051/stats.json
```

| 文件 | 我实测 sha256 | 期望 | |
| --- | --- | --- | --- |
| `stats.json` | `cda6a9a2a007fcabe2be602a9962b66deda84498e0fb1fb8232d0627632cd7ba` | 同 | ✅ MATCH |
| `proposal.json` | `1eef95414b8eec084b12b09b8eb89d6347c64f45476447ec243e5dd66c5c5635` | 同 | ✅ MATCH |

**唯一绝对不能碰的文件未被触碰**：

- `fix-props/schema.json` sha = `b71c2afde6c103132e4e1ca6b9896aab442bd80abbe1fcc0870f35ddce0f2019`
  （运行前后一致）
- mtime = `2026-10-07 23:55:23.851232039 +0800`，size `219688` —— **未变**

---

## 6. 发现但未修的问题

### 6.1 【非阻断】`group_id=""` 整轮累计场景：检测器会停下一笔**算得对**的账

- **触发条件**：某 adapter 用 `group_id=""` + `attempt` 恒 0 + `used` 报整轮累计。
- **实测影响**：E6 —— `used=200`（**正确总额**），却被判
  `collisions=1, stopped=true`，文案称「账本不可信」。
- **为何非阻断**：契约确实违规（`mod.rs:288` 要求增量）；生产路径结构上不可达
  （`derive_group_id` 恒非空，无任何构造点）；fail-closed 方向安全（停 ≠ 多花钱）。
- **建议**：见 §3.5，只改文案区分「契约违规」与「账已错」，**不要**放宽触发条件。

### 6.2 【非阻断 · 文案瑕疵】`llm_retry_spend...` 的失败信息与实际值不符

- **位置**：`tests/test_attempt_numbering.rs:335`
- **问题**：断言消息写「若编号被删（恒报 0），账上会是 **250**」，
  但 M3 实测 `left: 160`。
- **原因**：REQ-051 加了碰撞检测后，重试组撞键 ⇒ **立刻停** ⇒ 第二组根本没跑，
  所以账上是 160（第一组被覆盖后的值）而非 250（两组的和）。
  该文案是**加碰撞检测之前**写的，现在过时了。
- **影响**：只影响失败诊断的可读性，**不影响判据正确性**
  （测试照样变红，且是它该红的时候红）。
- **建议**：把 250 改成 160，或改成「账上只剩最后一笔，且因碰撞被停」。

### 6.3 【观察，非缺陷】碰撞文案里的固定措辞在 E6 场景产生误导

见 §3.4 / §6.1。与 6.1 同源，一并处理即可。

---

## 7. 我自己引入又清掉的问题（如实报）

| # | 问题 | 处理 |
| --- | --- | --- |
| 1 | **临时探针写错结构体字段**（`SchemaReport.warnings` / `GroupSource::Table` / `ExtractionGroup.relations` 三个字段在本仓库不存在）⇒ **第一次 M4 全量编译失败** | 判定该次 M4 结果**无效**（编译失败 ≠ 变异执行），删除探针后**完整重跑** M4；报告数据取自重跑 |
| 2 | 探针自身 `u32`/`usize` 类型错误两处（索引 + 比较） | 改正后重跑，探针 6/6 通过 |
| 3 | 探针 E4 期望值写错（用了 `u64::MAX/2` 的预算，闸门在饱和前就停了） | 改为 `max_used = u64::MAX` 隔离饱和行为；顺带发现**编排拒绝无预算的 `reports_usage` agent**（fail-closed，符合设计） |
| 4 | `mutate.py` 首次用 `..` 相对路径被文件系统策略拦截 | 改用绝对路径 |
| 5 | 全量日志首次只重定向 stdout，导致 `Running` 行丢失、解析器读到 0 个二进制 | 改用按 `test result:` 行聚合，不依赖 `Running` 行 |
| 6 | **偏离登记**：本会话**未暴露 `edit` / `write` 工具**（仅 read/bash/grep/glob），故源码改动与 Markdown 写入改用带引号 heredoc / Python 脚本 | 已按「失败就用带引号 heredoc 兜底并登记偏离」执行，未反复重试 |

**收尾状态**：worktree 已恢复到 `a55ffad`，`git status -uall` **输出为空**，
`git stash list` 为 0，无遗留分支/引用。临时探针已移出 `tests/`。

---

## 8. 复现命令清单

```bash
cd /home/myg133/Projects/Anchnet/Ontology/verify-REQ-051
export ONTOK_TEST_DATABASE_URL="$(grep '^ONTOK_TEST_DATABASE_URL=' \
  /home/myg133/Projects/Anchnet/Ontology/.env | cut -d= -f2- | sed 's#/notol$#/ontol#')"

cargo test --test test_attempt_numbering          # 新套件，8 条
cargo test --no-fail-fast                         # 全量，41 个二进制
```

变异脚本与探针副本：`.review-workspace/verify-req051/`
（`mutate.py`、`mutate_m4.py`、`probe_final.rs`、`probe_broken_version.rs.bak`、
`NOTES.md`、`mut-{M3,M3b,M4,baseline,head}-full.log`）。

---

## 9. 判定

**PASS 带非阻断。**

- 三条变异全部被独立杀死，对照栏三条全部全绿 ⇒ 本需求要补的洞已补上；
- 碰撞检测 6 个刁钻情形零假阳性，热路径零额外 map 查找经核实成立；
- 574/0/1 与基线 566/0/1 差 +8，红线零改动，产物 sha 独立复现精确匹配。

**遗留（非阻断，建议开小单）**：§6.1 的文案区分 + §6.2 的失败信息订正。

**VERDICT: PASS**
