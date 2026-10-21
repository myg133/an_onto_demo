# 验收标准：REQ-108 · 带证据与置信度的关系抽取

> **代码基线**：`develop` `4573269`
> **实测依据**：2026-10-21 用真实库 `anspire_open_uat`（96 表 / 0 外键 / 81 条推断关系）跑出的采样结果
> **本机限制**：全量 `cargo test` 在 Windows 编不过（`src/bin/ontok-gate-preflight.rs` 的 `std::os::unix`，既有问题）⇒ 交付物必须声明跑的是哪些目标

---

## 零 · 本需求的「已知错误集」（AC-006 的判据来源）

这 9 条关系已用 5 行采样实测为错，**必须全部落进「拒绝」档**：

| # | 关系 | 实测 | 错误类型 |
| --- | --- | --- | --- |
| E1 | `balance_operation_log.batch_id → gift_code_batch.id` | 0/5 | 同名不同义 |
| E2 | `instance_manager.session_id → simulate_login_sessions.id` | 0/5 | 同名不同义 |
| E3 | `internal_operation_records.batch_id → gift_code_batch.id` | 0/3 | 同名不同义 |
| E4 | `llm_model_peak_config.model_id → llm_model.id` | 0/4 | 目标列选错（应为 `llm_model.model_id`） |
| E5 | `llm_model_sort.model_id → llm_model.id` | 0/5 | 同上 |
| E6 | `llm_provider.model_id → llm_model.id` | 0/5 | 同上 |
| E7 | `llm_provider_groups.model_id → llm_model.id` | 0/5 | 同上 |
| E8 | `dim_customer_binding.channel_code → marketing_channel.id` | 类型 text→int | 类型不兼容 |
| E9 | `marketing_channel_customer.channel_code → marketing_channel.id` | 类型 text→int | 类型不兼容 |

另有 **5 条部分命中**（1/5 – 4/5），**不得落进「高」档**（允许「中」或「低」）：
`dim_customer_binding.customer_id`（4/5）· `enterprise_recharge_order.customer_id`（1/5）· `order.subscription_id`（1/2）· `service_apps.channel_id`（4/5）· `usage_reconciliation_log.subscription_id`（3/5）

---

## 一 · AC

### AC-001 · 勘查补采主键与唯一索引

- `ontok-introspect` 产物里每张表带 `primary_key`（列名或 null）与 `unique_columns`（数组）
- 判据：以 `anspire_open_uat` 为例，`llm_model.primary_key == "id"`；`marketing_channel.id` 在 `unique_columns` 里
- ⛔ 不许用「列名是不是 `id`」来猜——那正是本需求要消除的近似

### AC-002 · 采样是只读的、且**确定性**

- 连接后 `SHOW transaction_read_only` 必须为 `on`，且只发 `SELECT`
- **同一份数据两次运行，抽到的样本必须完全相同**（`ORDER BY 主键 LIMIT 5`，⛔ 不用 `TABLESAMPLE` / `random()`）
- 判据：两次运行的 `signals.sample_values_digest` 相同（**只存样本的哈希，不存原值**）

### AC-003 · 采样结果**不落原值**

- 产物（`proposal.json` / `stats.json` / 日志）中**不得出现任何业务数据值**
- 只允许：`sampled`（样本数）、`matched`（命中数）、`sample_digest`（样本哈希）
- 判据：对产物做一次「采样值回查」——把采样到的 5 个值拿去 grep 产物，**命中数必须为 0**

### AC-004 · veto 上限优先于加权和

构造四类样本，逐类断言上限生效：

| 类型 | 期望上限 |
| --- | --- |
| 类型族不相容（int vs text） | ≤ 0.15 |
| 采样 0 命中 | ≤ 0.20 |
| 泛化列名 + 无数据证据 | ≤ 0.40 |
| 源列全空（`sampled = 0`） | ≤ 0.45 |

判据：把 `name_match` / `target_is_key` 全部人为置 1.0，**分数仍不得超过上限**（这条证明 veto 没被加权和救回来）。

### AC-005 · 评分可复算，且不改动既有抽取产物

- 同一 schema + 同一数据跑两次：`confidence.score` **逐位相同**
- **`items` 逐字节不变**（REQ-059 口径：`extracted_by` 除外）
- 判据：`git diff` 两次产物的 `items` 段为空；`confidence` 段为空差异

### AC-006 · 9 条已知错误**全部**落「拒绝」（< 0.30）

- 见 §零 的 E1–E9
- 判据：一条不落即 FAIL。**这是本需求最核心、最可证伪的一条。**

### AC-007 · 5 条部分命中不得落「高」

- 见 §零 末尾
- 判据：5 条中任一条 `band == 高` 即 FAIL

### AC-008 · `method` 必填

- 每个 `confidence` 必须带 `method`（如 `evidence-scorecard-v1`）
- 缺 `method` 的条目：**拒绝入库**，并报错说明「没有 method 的分数不可复核」
- 判据：构造一条缺 `method` 的条目，`propose import` 必须非 0 退出

### AC-009 · 分档路由生效

| 分数 | 行为 |
| --- | --- |
| ≥ 0.90 | 进提案，审核界面默认折叠 |
| 0.60–0.90 | 进提案，逐条显示 |
| 0.30–0.60 | 进「待查清单」，**不进主提案** |
| < 0.30 | **不提案**，只记拒绝理由 |

判据：用真实库跑一遍，统计四档条数，且「不提案」档里的关系在 `proposal.json` 中**一条都找不到**。

### AC-010 · 客户视图零泄漏

- `ontology show`（客户视图 JSON / HTML）**不得出现** `confidence` / `score` / `band` 任何字段
- `--model-view` 与审核界面**必须**有
- 判据：客户视图产物 grep 上述字段 = 0；model-view 里每条关系都有

### AC-011 · 校准命令

- 提供一条命令输出**分箱命中率**（如 `0.9–1.0: 41 条 / 人工接受 39 → 95%`）
- 样本不足时必须报「样本不足」，⛔ 不许输出 `0%` 或 `100%` 冒充结论
- 判据：空库运行 → 明确提示样本不足；有数据运行 → 输出分箱表

### AC-012 · 红线与不退化

| 项 | 约束 |
| --- | --- |
| 禁改 | `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` / `src/mcp/tools.rs` / `mcp::tools::guard()` 内部 |
| 禁改 | `property_filter.rs` 的 G1–G4 措辞 |
| 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖（不引新 crate） |
| 不许退化 | REQ-107 的 13 条出口判据（74 passed）不得变红 |

### AC-013 · 交付证据

1. 评分卡规格与权重表（含「为什么是这个权重」）
2. 81 条真实关系的**分档分布**实测
3. 9 条已知错误全部落「拒绝」的实测输出
4. 采样只读 + 确定性的证据（`SHOW transaction_read_only` + 两次样本 digest 相同）
5. 不落原值的证据（采样值回查命中 0）
6. `items` 未变的证据（逐字节 diff）
7. 全量/局部测试数字 + 口径 + **本机不可跑全量**的声明
8. 已知限制

---

## 二 · 纪律（逐条适用）

- **E** · 扫描报「零」先怀疑扫描器；「跑不出」先怀疑环境
- **G** · 变异三元组（二进制 sha256 + patch + 逐条结果）；⛔ 编译失败不算执行
- **H** · 一致性守卫抓不到「忠实的重复实现」；不许写第二份判据
- **I** · 每条命令自带 `cd` 或全用绝对路径
- **J** · ⛔ 不要 `2>/dev/null`
- **L** · 报「环境不具备」前先查变量来源
- **S** · **AC 自己也会是错的** —— 发现某条不成立，直接说，不要迁就实现
- **临时文件** · 只放 `.review-workspace/req108/`；⛔ 禁 `rm`
- **源库** · ⛔ **只读**（`SET default_transaction_read_only = on`，只 SELECT，禁一切写操作）

---

## 三 · 修订（Dev 交付后，BA 裁定）

> Dev 在交付报告 §8「AC 自己也会是错的」里指出：**AC-007 的固定清单与设计的确定性采样互相矛盾**。
> 这个批评成立，按纪律 S 修订。

### A-1 · AC-007 改为**机制判据**（原写法作废）

**原写法**：列出 5 条「部分命中」关系（1/5、4/5），要求它们不得落「高」。
**问题**：那些比例来自我早期用 `LIMIT 20`（**无排序**）跑的一次非确定性采样。而设计**要求**确定性采样（`ORDER BY 主键 LIMIT 5`）。口径不同，样本就不同，比例自然不同 —— Dev 按设计口径得到 5/5，落「高」是**正确行为**，不是缺陷。

**修订后的 AC-007（机制判据）**：

| 判据 | 要求 |
| --- | --- |
| `0 < matched < sampled` | `score ≤ 0.75`（不得落「高」） |
| `matched == sampled` 且 `sampled ≥ 5` | 允许落「高」 |
| `sampled == 0` | 走 `no_data_evidence`（≤0.45），**不得**因「没证据」而比「0 命中」更高 |

⇒ 判据从「这几条必须低分」变成「**部分命中必须被压低**」。前者依赖一次性采样结果，后者才是机制。

⚠️ 但这暴露一个真实缺口（登记，不在本需求修）：**5/5 命中只证明「存在性」，不证明「覆盖率」**。一条关系可能只在 5 个样本上成立，全量覆盖率很低。见 `design-summary.md` §九 G4。

### A-2 · 认可第 9 条 veto `no_data_evidence`

Dev 新增：`--no-sample` 时 `score ≤ 0.45`。
**理由成立**：不加的话，「没采样」会比「采了 0 命中」分更高（因为归一化后少了低分项），直接违反 R1 ④ 的「缺少证据 ≠ 有证据」。**采纳，并写入 design-summary 的 veto 表。**

### A-3 · 客户视图守门词表不新增词（红线）

Dev 未改 `src/domain/view.rs` 的守门词表（红线），零泄漏靠**结构**（`SnapshotView` 无 confidence 字段）+ 测试钉住。
**裁定：接受。** 理由：改红线文件的收益（多一道词表）小于成本（红线为零触碰）。已登记为已知缺口：**若将来有人把 confidence 塞进 `SnapshotView`，词表挡不住，只能靠测试**。

### A-4 · 关系总数 82 ≠ 81，接受

Dev 实测 82 条（多 1 条来自 REQ-028 的尾缀匹配）。
**裁定：接受**，不改代码凑数。`acceptance.md` §零 的「81」读作「至少 81」，判据按实测集合走。

### A-5 · REQ-107 判据 70 vs 74

Dev 报 13 个套件实测 **70 passed**，派单材料写 74。
**裁定**：以 QA 独立复跑为准；若确为 70，说明我派单时的数字来自另一组套件口径（`qa-review-4.md` 写的是「17 条 Pipeline 登记 / 74 passed」），**登记口径差**，不算退化——但 QA 必须核实「REQ-107 的那 13 个套件里没有任何一条转红」。

---

## 四 · 第 2 轮裁定（QA 判 PASS 带 2 条「非阻断」，BA 改判为**阻断**）

> QA 报告 `qa-review.md` 的 N1 / N2。QA 判「无意外触发路径 ⇒ 非阻断」；
> **BA 改判为阻断**：本需求的全部承诺就是「低分关系进不了本体」，而这两条各用**一行**就能推翻它。
> 与 REQ-107 四轮打掉的「判据看着绿、其实没人管」是同一类问题。

### A-6 · AC-006 的证据形式补全

E8/E9 当前抽取器**不产出**（`channel` 在闸 B 下歧义），QA 独立确认三份产物里都没有这两条，并**按坐标构造候选**走真库真评分复算得 `0.15 / type_incompatible`。

**裁定**：AC-006 对 E8/E9 的判据形式改为「**不产出 + 构造复算**」，两者都要有证据。**不产出比拒绝更强**，但必须在报告里写清是哪一种，⛔ 不许混为一谈。

### A-7 · 新增 AC-014 · 置信度**自证链**完整性

> 来源：QA 实测「手改 `score` 为 0.99，veto 原样保留 ⇒ import 成功」。

**三条校验，缺一即拒绝导入**：

| # | 校验 | 为什么 |
| --- | --- | --- |
| 1 | `score ≤ 每个触发 veto 的上限` | 否则 veto 只是注释 |
| 2 | `signals[].weight` 与该 `method` 的权重表**逐项相等** | 设计概要 §2.1 已要求，但没实现也没判据 |
| 3 | 每个 `veto` 信号名必须**出现在 `signals` 里** | 防止「声明了 veto、但没有对应证据」 |

**判据（可证伪）**：构造一份 `score=0.99 + veto=zero_hit` 的提案条目 ⇒ import 必须 exit≠0，报错要指出「score 0.99 超过 veto zero_hit 上限 0.20」。

### A-8 · 新增 AC-015 · `importable: false` 必须强制

> 来源：QA 实测「原样 import `proposal-rejected.json` ⇒ EXIT=0」。

`ontok-propose` 产出的 `proposal-rejected.json` 带 `importable: false`。
**要求**：`propose import` 遇到 `importable: false` 的文件必须 **exit≠0** 并说明原因。

**判据**：直接 import 该文件 ⇒ 非 0 退出；且错误文案要说清「这是被拒绝档，不能导入」。

### A-9 · `--no-sample` 不退回

QA 实测：`--no-sample` 时 30/82 分数抬高，E1–E7 从 0.20（拒绝）升到 0.40/0.45（低）。

**裁定**：**接受**。理由：
- 机制上仍落「低档」，**仍不进提案**；
- 「没有证据」与「有反证」**本就必须不同分**（R1 ④ / A-2）；
- 若强行让 `--no-sample` 也落拒绝，等于用「没查」冒充「查过有问题」——那是更坏的方向。

⚠️ 但交付物必须显式写：**`--no-sample` 下「拒绝」档只覆盖结构可判的错误（类型不兼容），0 命中的硬反驳拿不到**。

---

## 五 · 第 2 轮复验裁定（Dev 交付 `4acbf5a`）

### A-10 · R1 必须本轮修（不留到下一轮）

Dev 登记：`signals` **整段缺失**时 `{"score":0.99,"method":"evidence-scorecard-v1"}` 仍能导入成功，
理由是既有单测 `absent_optional_fields_…` 要求放行（保护老的人工提案）。

**BA 裁定：修，本轮就修。** 理由：
- 这与刚修掉的 N1 是**同一类漏洞**——一个改数字、一个删字段，都只要一行；
- 老提案的保护**不需要**靠放行实现，两者可以精确区分：

| 情形 | 应否放行 |
| --- | --- |
| **没有 `confidence` 字段**（老的人工提案） | ✅ 放行（既有行为不变） |
| **有 `confidence` 但 `signals` 缺失或为空** | ❌ **拒绝**：声明了分数却没有证据链 |
| 有 `confidence` 且有 `signals` | 按 A-7 三条校验 |

⇒ 判据：构造 `{"score":0.99,"method":"…"}`（无 signals）⇒ import 必须 exit≠0；
构造一条**完全没有 confidence 字段**的老提案 ⇒ import 仍然成功（不许误伤）。

### A-11 · `proposal-pending.json` 允许导入（采纳 Dev 的判断）

Dev 把待查档的 `importable` 从 `false` 改成 `true`。**采纳**，理由成立：
- 低档不进**主提案**，但要进**审核系统**才有入口；
- 它带着 `band=low` + veto，仍不进客户视图；
- `proposal.json` 里 low/rejected 仍各 0 条（AC-009 不变）。

⇒ **A-8 只针对 `proposal-rejected.json`**，不适用于 pending。

### A-12 · R2 / R3 / R4 登记为已知限制（本轮不修）

| # | 形态 | 为什么可以留 |
| --- | --- | --- |
| **R2** | 同时伪造 `sampled/matched = 5/5` 再算 0.99，逻辑自洽 | 只能靠 `sample_digest` 回源库复采证伪。**登记**：置信度可被「伪造全套证据」绕过，防的是**诚实错误**不是**恶意篡改**（恶意篡改需要 DB 写权限，那已经是另一个信任边界） |
| **R3** | 3 条结构性 veto 在 signals 里无对应信号 | 它们只**压低**分数、抬不高，不构成伪造高分通路 |
| **R4** | 未实现「按权重重算 score 必须相等」 | 权重与 veto 双向锁死后，单改 score 已过不了闸；重算属于加固，可后续 |

⚠️ 这三条必须写进交付物的「已知限制」，⛔ 不许写成「已封堵」。

---

## 六 · 第 3 轮裁定（QA 复验 PASS 带 4 条非阻断，BA 把 R5 改判为**阻断**）

### A-13 · R5 必须修（`data_evidence: sampled` 却没有采样信号）

QA 实测的完整攻击链：

```json
{ "score": 0.99, "method": "evidence-scorecard-v1",
  "data_evidence": "sampled",
  "signals": [ 只留 name_match / type_compatible / target_is_key / not_all_null ] }
```

**没有 `sample_hit_rate`、也没有 `veto`** ⇒ `import` / `decide` / `apply` **全部 EXIT=0** ⇒
快照里 E5 变成 `0.99 / high`，`data_evidence` 仍是 `sampled`。

**为什么它必须修（而不是像 R2/R4 那样登记）**：

| 项 | R1（已修） | **R5** |
| --- | --- | --- |
| 手法 | 声明了分数，**整个证据链删掉** | 声明了分数，**只删最强的那个证据** |
| 结果 | 已拒绝 | **通过** |

两者是同一件事的粗细版本。QA 说「这是同类里最省事的一条」，正说明它比 R2/R4 更容易被顺手做出来。

**修法（QA 给的，约 5 行）**：

| 条件 | 要求 |
| --- | --- |
| `data_evidence == "sampled"` | `signals` 里**必须**有 `sample_hit_rate`，且其 `sampled/matched` 与顶层一致 |
| `data_evidence == "none"` | 走既有 `no_data_evidence`（≤0.45），且 `signals` 里**不得**有 `sample_hit_rate` |
| `data_evidence` 缺失 | 按 `sampled` 处理（严格侧） |

**判据（可证伪）**：构造上面那份 JSON ⇒ `import` 必须 exit≠0，文案指明「声明了采样证据但没有 sample_hit_rate 信号」。

### A-14 · 三条文档/口径问题（本轮一并修）

| # | 问题 | 要求 |
| --- | --- | --- |
| **N7** | 第 3 轮没更新 `verification-report.md`，**§11 不存在**，报告止于 §10.6 | 补 §11（R1 修复 + `confidence: null` 裁定 + 本轮数字） |
| **N6** | 「31 套件 307 passed」与实测 **302** 差 5（`live` 与 `source_enumeration` 互换的口径） | 报告与交付说明里给出**套件名单**，数字必须能对上名单 |
| **N8** | `test_req107_source_enumeration` 不在标准回归集，导致「31 套件全绿」有覆盖盲区 | 把它**加入回归集**，并在报告里列出回归集名单 |

### A-15 · QA 的两条判断，采纳

1. **t5 补登记 = 遵守规则，不是放宽**（QA 回 `4acbf5a` 复现 9/1 红，且放宽需要改的四个开关全部零改动）⇒ **采纳，合规**。
2. **`confidence: null` 按「未声明」放行**（null 不携带分数，只会降级为未评分，不抬高）⇒ **采纳**。

### A-16 · 已知限制（更新）

R2 / R4 仍按 A-12 登记不动。**R5 修完后**，同一类的剩余风险只有：

> **伪造全套自洽证据**（连 `sampled/matched` 一起改成 5/5 并重算分数）——
> 这需要连 `sample_digest` 一起伪造，而 digest 可回源库复采证伪。
> 置信度防的是**诚实错误**，不是**有 DB 写权限的恶意篡改**（那是另一个信任边界）。
