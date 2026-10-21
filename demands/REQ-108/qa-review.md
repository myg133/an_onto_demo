# REQ-108 · 带证据与置信度的关系抽取 —— Pre-merge QA 审核

> **审核对象**：`feature/REQ-108` = `3f32eea`（基线 `develop` `4573269` + 7 个 `[Dev]` 提交）
> **审核方式**：**独立复验 + 主动攻击**。所有 DB 事实用 `psycopg2` 另写一份 oracle 查（**不复用**产品 SQL / 不复用 Dev 的测试），
> 所有产物用我自己的二进制在自己的 worktree 里重跑（不复用 Dev 的产物）。
> **审核 worktree**：`.review-workspace/req108/qa`（`git worktree add --detach … 4573269` → `merge --ff-only feature/REQ-108`；
> 结果是 fast-forward，树 `6833a53` 与 feature tip **同一个 tree**，见 §0.1）
> **产物 / 脚本**：`.review-workspace/req108/qa-out/`、`.review-workspace/req108/qa-verify/`
> **审核时刻**：2026-10-21 16:22–16:45（+08:00）；源库快照 = 该时段的 `anspire_open_uat`
> **本机限制**：全量 `cargo test` 编不过（既有问题，见 §6.3）

---

## 0 · 结论

**PASS（带 4 条非阻断）**。13 条 AC 独立复跑全部成立；**阻断项 0 条**；不退化成立。
4 条非阻断见 §7（其中 N1 = 分数可被改文件绕过，N2 = `importable: false` 声明未落地）；
另有 5 条「AC 自己也会是错的」观察见 §8，均不影响本轮合并，需 BA 登记。

| 项 | 我的独立结论 | Dev 的说法 | 一致？ |
| --- | --- | --- | --- |
| 9 条已知错误落拒绝（AC-006） | **9/9 成立**（7 条「产出且拒绝」+ 2 条「不产出 + 构造复算 0.15」） | 9/9 | ✅（但 E8/E9 的判据形式被改写，见 §2.3） |
| 评分卡能否被绕过 | **评分卡逻辑绕不过；「分数」这个数字可以（改文件）** | 未测 | ❌ 新发现 |
| 两次 CLI 产物逐字节相同 | **4/4 相同**（我自己的全量 sha256 一致） | 是 | ✅ |
| 采样只读 / 不落原值 | **只读 on；42 值 + 我另取 5 值，命中 0** | 是 | ✅ |
| 分档路由 | **高 44 · 中 8 · 低 20 · 拒绝 10**；`proposal.json` 里 low/rejected **0 条** | 同 | ✅ |
| 客户视图零泄漏 | **JSON/HTML 全 0 命中**；更硬的一条：两个分数不同的项目，客户视图**逐字节相同** | 是 | ✅ |
| REQ-107 13 条出口判据 | **70 passed / 0 failed** | 70 | ✅ |
| 70 vs 74 | **口径差**（不是退化，§6.2 有硬证据） | 「没能复现出处」 | ✅ 补上了出处方向 |

### 0.1 我验的确实是那棵树（不是「看起来像」）

```
feature tip  tree = 6833a532a50e6c3e8e4b265b7b0c8aef06d604ab   (3f32eea)
qa worktree  tree = 6833a532a50e6c3e8e4b265b7b0c8aef06d604ab   (merge --ff-only 结果)
merge-base(4573269, feature/REQ-108) = 4573269  ⇒ 可 fast-forward ⇒ 内容无歧义
```

`feature-REQ-108` 工作树我的操作结束后 `git status` 仍是干净（只在**我的** worktree 里加了一个未跟踪的靶场测试 `tests/qa_attack_req108.rs`）；
`src/bin/ontok-gate-preflight.rs` 的 sha256 挪开前/挪回后 **逐字节一致**：`C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`。

---

## 1 · 我的 oracle 与产品实现是**两份**（否则不是复验）

| 判据 | 产品实现 | 我的独立实现 |
| --- | --- | --- |
| 取样 | `sample.rs::sample_sql`：`SELECT (col)::text … WHERE col IS NOT NULL ORDER BY <pk> LIMIT 25`，去重保序取 5 | `psycopg2` 自查 `information_schema` 取列类型 + 自查 `pg_index` 取主键 + 自己拼同样的 SQL |
| 命中 | `match_sql`：`(to_key)::text = ANY($1::text[])` | 自己拼同样的比较 |
| digest | 自实现 SHA-256（NIST 向量钉住） | **Python `hashlib`** |
| 类型族 | `type_family()` 形态映射 | 我自己的映射表（同一份语义，独立代码） |

⇒ 下面 §2 / §4 的「一致」是两份实现的**交叉验证**，不是同一份代码自证。

---

## 2 · 9 条已知错误（AC-006）

### 2.1 逐条实测（源库 `anspire_open_uat`，2026-10-21 16:27）

**这 7 条是「真的产出了、真的被拒绝」**（出现在 `qa-out/run1/proposal-rejected.json`，三份产物互不相交）：

| # | 关系 | 产物落点 | score | band | veto | sampled/matched | digest 与我 oracle 一致 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E1 | `balance_operation_log.batch_id → gift_code_batch` | rejected | **0.20** | **rejected** | `zero_hit` | 5/0 | ✅ `sha256:1deb6586…` |
| E2 | `instance_manager.session_id → simulate_login_sessions` | rejected | **0.20** | **rejected** | `zero_hit` | 5/0 | ✅ `sha256:b67e5a08…` |
| E3 | `internal_operation_records.batch_id → gift_code_batch` | rejected | **0.20** | **rejected** | `zero_hit` | 3/0 | ✅ `sha256:f0b94ee5…` |
| E4 | `llm_model_peak_config.model_id → llm_model` | rejected | **0.20** | **rejected** | `zero_hit` | 4/0 | ✅ `sha256:a6177f79…` |
| E5 | `llm_model_sort.model_id → llm_model` | rejected | **0.20** | **rejected** | `zero_hit` | 5/0 | ✅ `sha256:64c14f73…` |
| E6 | `llm_provider.model_id → llm_model` | rejected | **0.20** | **rejected** | `zero_hit` | 5/0 | ✅ `sha256:81184b2d…` |
| E7 | `llm_provider_groups.model_id → llm_model` | rejected | **0.20** | **rejected** | `zero_hit` | 5/0 | ✅ `sha256:c38b3299…` |

> digest 逐条相同这一列是**最强的证据**：说明产品采样到的**就是我这五个值**（哈希相同 ⇒ 值集合相同），
> 而不是「产品采样器报了 0 命中但采的不是同一批行」。
> 表里只印了前缀；**完整 64 位 hex 也逐条相同**（7/7），例如
> E1 `sha256:1deb65863714f07cea157c57a99826ec2c6f1626c6adacd4a81c265ac087f689`
> = 我的 `hashlib` 独立算出的同一个值（脚本 `qa-verify/digest_full.py`）。

**2.2 这 2 条不产出**（我扫了三份产物，逐一确认三份里都没有该坐标）：

| # | 关系 | 三份产物 | 类型事实（我直接查 `information_schema`） | 构造复算 |
| --- | --- | --- | --- | --- |
| E8 | `dim_customer_binding.channel_code → marketing_channel` | **都不在**（不产出） | 源 `character varying`(Str) vs 目标键 `integer`(Num) | **0.15 / rejected / `type_incompatible`** |
| E9 | `marketing_channel_customer.channel_code → marketing_channel` | **都不在**（不产出） | 同上 | **0.15 / rejected / `type_incompatible`** |

构造复算的实测输出（`cargo test --test test_req108_live -- --nocapture`，**我自己跑、我自己提供产物目录**）：

```
dim_customer_binding.channel_code → marketing_channel
    score=0.15 band=rejected veto=type_incompatible sampled=5 matched=0 produced=false
marketing_channel_customer.channel_code → marketing_channel
    score=0.15 band=rejected veto=type_incompatible sampled=5 matched=0 produced=false
```

### 2.3 E8/E9 的替代证据成不成立？——**成立**，但判据形式变了，必须登记

我的裁定（三条一起看，缺一条就不成立）：

1. **「不产出」有独立理由**：`channel` 词干同时命中 `marketing_channel` 与 `rpt_dws_channel` ⇒ REQ-028 的「唯一尾缀」闸不产出。
   这不是评分卡在兜底，而是抽取器在更前面就不允许这条关系存在。**「不产出」比「拒绝」更强**：拒绝是「提了但被打掉」，不产出是「根本没进候选集」。
2. **「类型不相容」是客观事实**，不是我信 Dev 的：我自己查了 `information_schema.columns`（`varchar` vs `integer`）。
3. **构造复算走的是同一条评分路径**：`candidate_for()` → 真实 `sample_one()`（真库 5 行、0 命中）→ 真实 `score_links()`（真 `SchemaReport` / 真 `KeyCatalog`）→ 0.15。
   它不是「手写一个 0.15 的期望值」，是拿真实坐标跑真实评分。

**但必须记三件事**：
- AC-006 的原文是「9 条**全部**落进拒绝档」。E8/E9 在本实现里**不会进入任何档**（因为它们不是候选），
  所以这条 AC 对 E8/E9 **字面上不可能满足**。它成立的方式是「机制：如果它被提案，会被拒绝」。
  这与 A-1 对 AC-007 的处理是同一类问题 ⇒ **建议 BA 按 S 纪律补一条 A-6，把 AC-006 的判据改成
  「每条要么落拒绝档，要么根本不被产出；前者须有 veto 证据，后者须有构造复算」**。
- 「不产出」依赖 REQ-028 的歧义闸。如果将来 `table_names.json` 或源库新增表改变了词干唯一性，E8/E9 **可能开始产出** ——
  那时挡住它们的是 `type_incompatible`（0.15）。也就是说这条判据的未来可靠性**不依赖**「不产出」这个巧合，构造复算才是真正的保证。
- 结论：**不是「回避判据」**。如果 Dev 只给「不产出」不给构造复算，我会判 FAIL；他给了两份证据，我判成立。

---

## 3 · 评分卡攻击（6 种）

| # | 攻击 | 期望 | 实测 | 判定 |
| --- | --- | --- | --- | --- |
| a | 类型不兼容 + `name_match=1.0` + `target_is_key=1.0` | ≤ 0.15 | **0.15**（band rejected，veto `type_incompatible`；**不加 veto 的基线 = 0.920**） | 🟢 **压住了** |
| b | 0 命中 + 其它信号全 1.0 | ≤ 0.20 | **0.20**（band rejected，veto `zero_hit`；**基线 = 0.550**） | 🟢 **压住了** |
| c | 注释掉 veto 上限那几行（变异） | 单测必须红 | **7 个单测红**（下表） | 🟢 **红得干净** |
| d | `--no-sample` 跑一遍 | 分数不得高于采样过的同一条关系 | **30/82 抬高、52/82 降低**；E1–E7 从 0.20（拒绝）→ 0.40/0.45（低） | 🔴 **字面口径不成立**（机制口径成立，见 §3.4） |
| e | 构造一条 `confidence` 缺 `method` | import exit 1 | **exit 1**，`item 97 … confidence 缺 method —— 没有 method 的分数不可复核（AC-008）` | 🟢 **挡住了** |
| f | 手改 `score=0.99` 但保留 veto 信号 | —— | **能改，全链路无人阻挡**（函数层 / CLI 层 / 落库 / 快照 / 建模视图） | 🔴 **可绕过** |

> a / b 的「基线 > 上限」这一列是判别力证明：如果基线本来就低于上限，那这个测试什么也没证明。

### 3.1 攻击 c 的变异三元组（二进制 sha256 + patch + 逐条结果）

```
confidence.rs  sha256  A5C14A02DA856C8FA2AFD2A5F04F2A885F9C43B869DA01541F1631987BDC1B09
             → 变异       0007A7CBC1DF5B26F831E61CE744FAAF9F12AC751F5E6816C239F808DC8B7EC5
patch  : src/domain/confidence.rs:552
         - let score = round2(baseline.min(min_cap));
         + let score = round2(baseline);              // QA-MUTANT: veto cap 注释掉
lib 测试可执行文件 sha256  CA8F58A7DF9F0CBFE4C6108E3FD0D7CEB21F99D11146D769404ECF082525B694
                       → F3D24C5A216B430C2BF7D4AE82660710957C27F08F6EB3AC7052CAA957567584
结果   : 21 passed / 7 failed（0 ignored）
          multiple_vetoes_take_the_lowest_cap
          no_sample_never_scores_higher_than_sampled_evidence
          veto_partial_hit_caps_at_075_and_never_high
          veto_r2_r3_r4_caps
          veto_type_incompatible_caps_at_015        （实际 0.92）
          veto_zero_hit_caps_at_020                 （实际 0.55）
          vetoes_survive_maxed_structural_signals   （实际 0.92）
```

⇒ **AC-004 的守卫不是「写了一个能拒绝它们的函数」，而是「压顶那一行被删就立刻红」**。这是本轮最扎实的一条。

### 3.2 攻击 f 的完整链路（这是本次审核最重要的发现）

我不是「理论上觉得可以」，是**真的改了一个文件、导进去、应用、然后从建模视图里读出来**：

```
① 取 proposal-rejected.json 里的 E5（llm_model_sort.model_id → llm_model，0.20 / rejected / zero_hit）
② 只改两个字段：score 0.99、band "high"。veto/signals 原样保留（veto.cap=0.20，sample_hit_rate=0/5）
③ ontok propose import --file proposal-tampered-e5.json
   → EXIT=0，148 条全部导入
④ ontok review decide --proposal p1 --accept-all   → EXIT=0
⑤ ontok proposal apply --proposal p1              → EXIT=0 → v2
⑥ DB：proposal_items.confidence = 0.99 / link_types.confidence = 0.99（v2 快照）
⑦ ontok review show --confidence 打出：
      [高] 0.99 模型排序表…→llm_model · 证据 0/5
                 veto zero_hit：采了 5 行、命中 0 行 —— 这是硬反驳
⑧ ontology show --model-view：
      "confidence": { "score": 0.99, "band": "high", …, "veto": {"signal":"zero_hit","cap":0.2} }
```

**谁该挡住 / 谁没挡住**：

| 环节 | 现在校验什么 | 该不该拦 f |
| --- | --- | --- |
| `propose import` → `NewItem::from_json` → `confidence::from_untrusted` | `method` 非空、`score∈[0,1]`、**`band` 必须与 score 一致** | ❌ 只拦「档位对不上」，不拦「分数与 veto/signals 矛盾」 |
| `proposal apply` → `domain::apply::link_from_item` → 又一次 `from_untrusted` | 同上（**同一份判据的第二处调用**，不是第二道判据） | ❌ 同上 |
| `link_types` / `proposal_items` 落库 | 无校验，直接写 | ❌ |
| 建模视图 / 审核界面 | 只渲染 | ❌（但**渲染出了矛盾**：`[高] 0.99 … 证据 0/5 … 硬反驳`） |

**缺失的三条（设计概要 §2.1 已经明文要求第 2 条，实现没做）**：

1. `score ≤ min(veto.cap)` —— 有 veto 就不可能这么高；
2. `signals[].weight` 必须与 `method` 版本对应的权重表逐项一致（**设计概要 §2.1 原文要求「可复核」，实现没校验**）；
3. 若 signals 齐备，`score` 必须等于按权重重算的结果（评分卡是纯函数，这一步零成本）。

> 结论：**评分卡本身不是漏的**（a/b/c 都证明它算得对、压得住、删了就红）；
> **漏的是「分数作为一个数字被搬运的边界」**。风险场景不是「算法错」，而是「有人/某个脚本改了一份产物」——
> 而 REQ-108 的整个卖点就是「可复算、可复核」，所以这条我判**非阻断但强烈建议本轮或下一轮补**（改动约 20 行，只动 `from_untrusted`）。

### 3.3 攻击 e 的对照（证明 f 不是「整个 import 不校验」）

```
e-missing-method → ✗ item 97 的 confidence 不合法：confidence 缺 method —— 没有 method 的分数不可复核（AC-008）
                   EXIT=1
只改 band（score 0.20 + band "high"） → ✗ confidence.band = high 与 score = 0.2 对不上（按阈值应为 rejected）
诚实条目（score 1.0 / high / method 齐）→ 通过
```

⇒ import **确实有三道校验**（method / 范围 / band 一致），只是**不校验分数与证据的自洽**。这个区别必须写清楚，否则会把「缺一道」说成「没有闸」。

### 3.4 攻击 d 为什么「字面红 / 机制绿」

我把 `--no-sample` 与采样过的 **82 条逐条对比**（同一份 schema、同一个 project）：

| 方向 | 条数 | 说明 |
| --- | --- | --- |
| no-sample **更高** | **30** | 全是采样分 < 0.45 的弱关系：0.32/0.40 → **0.45** |
| no-sample **更低** | **52** | 采样分 > 0.45 的强关系：0.92/1.00 → **0.45** |

**7 条已知错误全部落在「更高」那一侧**：

```
E1/E2/E3  batch_id/session_id： 采样 0.20（拒绝/zero_hit） → --no-sample 0.40（低/generic_name_no_evidence）
E4/E5/E6/E7 model_id：          采样 0.20（拒绝/zero_hit） → --no-sample 0.45（低/no_data_evidence）
```

所以「**分数不得高于采样过的同一条关系**」这条字面判据 **不成立**（30 条反例）。

但机制上：
- `--no-sample` 的 82 条**全部**是 `band=low` + `data_evidence=none` ⇒ **一条都不进主提案**（`proposal.json` 里 0 条关系）；
- 语义上「被反驳（0.20）< 不知道（0.45）< 有证据（0.92）」，这个序是对的：**已经证伪的比没看过的更确定地差**；
- A-2 已经把「`--no-sample` ⇒ ≤0.45」写成 BA 采纳的上限。

⇒ 这是**判据措辞**的问题（和 A-1 同源），不是实现偏差。**建议 BA 把该条改写成**：
「`--no-sample` ⇒ `data_evidence=none` 且 `score ≤ 0.45` 且不进主提案；**不得**让「没看」高于「看过且有命中」的那条关系」。
按现写法，任何实现只要让「0 命中 < 没采样」就必然红 —— 而那个序恰恰是对的。

---

## 4 · 幂等与确定性

### 4.1 两次 CLI 运行：4 份产物**逐字节相同**（我自己的全量 sha256）

```
run1 = run2（同一份 schema.json、同一个 --project qa108run、连真源库采样）
proposal.json            1CDE06826DDA0AA4CFBFEA6A0ED3D4EA81521E4D023B641EBEE76FE84119A923
stats.json               F1E6735511C579B8211A114CC5C8ED6D82472FBD6FFF7CC775913C5EA1D06560
proposal-pending.json    0C6F6EA9FE64F311E34F09DDF061FB557BC3148DDEA5CB6E78A5D71B14B366CB
proposal-rejected.json   9528C037D5798455EF46BC1CB8D4E3E9EFA4E0B7E4B3513E87C278FC01B22315
```

> `stats.json` 的哈希与 Dev 报的 `F1E6735511C579B8…` **完全相同**（他的 project 名不同、proposal.json 哈希不同，
> 但 stats 里不含 project/时间戳 ⇒ 两台机/两次运行同一个哈希）。这是一条**跨运行**的强证据。

### 4.2 同进程两次采样：逐字段相同

`test_req108_ac002_sampling_is_read_only_and_deterministic`（我跑，真源库）：
把 9 条坐标的 `SampleEvidence` 采样两遍，**逐元素 + 整体 `assert_eq!`** 通过。
字段数：9 条 × 4 字段（`sampled` / `matched` / `digest` / `reason`）= **36 个字段**。

> ⚠️ Dev 报告写「**42 项**逐字段相同」。42 是 AC-003 的「样本值个数」，不是字段数（36）。
> 这是**报告笔误/口径混用**，判据本身没受影响（`a == b` 的整体断言更强）。非阻断，登记。

### 4.3 采样只读（不是只看注释）

| 证据 | 结果 |
| --- | --- |
| `SHOW transaction_read_only`（我在 psycopg2 里开的只读会话） | `on` |
| `introspect` 的真实运行输出 | `只读确认: transaction_read_only = on` |
| `sample_pool()` 的连接后校验 | 读回 `on` 才继续，否则 `bail`（fail-closed） |
| `sample.rs` 的 SQL 全量扫描（INSERT/UPDATE/DELETE/CREATE/ALTER/DROP/TRUNCATE） | **0 命中**，只有两条 `SELECT` |
| `force_read_only()` | 连接串强制追加 `default_transaction_read_only=on` + `after_connect` 再 `SET`（因为 sqlx 0.8 不透传 startup 参数） |

### 4.4 不落原值（两条独立判据）

| 判据 | 样本值 | 产物 | 命中 |
| --- | --- | --- | --- |
| Dev 的 live 判据（我用自己的产物目录跑） | 42 个（可判别 42 · 短值 0） | 我的 4 份 | **0** |
| **我另写的一份**（不同表/列：`llm_provider.model_id` / `customers.email` / `marketing_channel.code` …，长度 ≥ 4） | 5 个 | 我的 4 份 | **0** |

> 并且有反空转断言：产物里必须出现 `sha256:`（否则「没查到原值」只是因为产物是空的）。
> 我的产物里有 **59 条**关系带 `sample_digest`。

### 4.5 digest 交叉验证（独立 oracle 的 hashlib）

E1–E7 七条关系的 `sample_digest` 与我的 Python `sha256("\x1f".join(sorted(values)))` **7/7 完整 64 位 hex 完全相同**
（比对的是完整串，不是前缀；见 §2.1 与 `qa-verify/digest_full.py`）。⇒ 采样器取的值集合与我独立取的**是同一批**。
这条同时排除了「产品采样器把命中数写错但 digest 随便编」这一类可能：digest 是用**实际取到的值**算的。

---

## 5 · 分档路由与客户视图隔离

### 5.1 路由（AC-009）

| 档 | 条数 | 去向 | 我的实测 |
| --- | --- | --- | --- |
| 高 ≥0.90 | **44** | `proposal.json` | 44 条，`band=high` |
| 中 0.60–0.90 | **8** | `proposal.json` | 8 条，`band=middle` |
| 低 0.30–0.60 | **20** | `proposal-pending.json` | 20 条，`band=low` |
| 拒绝 <0.30 | **10** | `proposal-rejected.json` | 10 条，`band=rejected` |
| 合计 | **82** | —— | 82（与 Dev 完全一致） |

- **`proposal.json` 里 `low` = 0、`rejected` = 0**（我逐条统计 `band` 字段，不是只看日志）。
- 三份 items **互不相交**、并集 = 82：三份的 `(from,col,to)` 集合 distinct 合计 **82**，两两交集 `a∩b=0 · a∩c=0 · b∩c=0`，每份内部也无重复（脚本 `qa-verify/partition.py`）。
- `proposal-pending.json` / `proposal-rejected.json` 都带 `"importable": false` + `note`（但见 §7 N2：**这个字段没有被强制**）。

### 5.2 客户视图零泄漏（AC-010）—— 我用了比 grep 更硬的一条

| 判据 | 结果 |
| --- | --- |
| `ontology show`（JSON 客户视图，59,462 B）grep `confidence` / `score` / `band` / `sample_digest` / `evidence-scorecard` / `veto` / `sampled` / `matched` | **全部 0** |
| `ontology show --html`（34,848 B）同上 | **全部 0** |
| `ontology show --model-view` | `"confidence"` **52 次**（52 条关系 52 条有），`"score"` 52 次 |
| **结构性证据（新）** | 我有两个项目：`qa108run`（诚实）与 `qa108_tamper2`（一条 0.99），**分数不同**，但两份客户视图**逐字节相同**：`customer.txt` = `09CA98BFD03778268EDE7BD393F4DCF074F980104140EE7AAD44AA4911D95D19`、`customer.html` = `4B96D51CA9AAE110F00C886C9F026735979AD38551C846FEB4E4C57D22C852A5` |

> 最后一条比 grep 有力：**改了分数，客户视图一个字节都不动** ⇒ 客户视图根本不读这条数据通路，不是「恰好没打印」。

### 5.3 校准（AC-011）

| 场景 | 输出 | 判定 |
| --- | --- | --- |
| 空项目（`qa108emptyproj`，0 条） | `计分条目: 0 条` + 五档全 `—` + `结论: **样本不足**（没有任何已决的计分条目）` + exit 0 | 🟢 明确报样本不足，**没有**输出 0%/100% |
| 有数据（`qa108run`，52 条已决） | `0.60–0.75: 1 条 → 样本不足` · `0.75–0.90: 7 条 / 100.0%` · `0.90–1.00: 44 条 / 100.0%` + `结论: **样本不足** —— 以下分箱 < 5 条：… 不输出命中率结论、不算 ECE` | 🟢 与 Dev §4.5 的表**逐格相同**；样本不足的箱不印百分比；ECE 不印 |

> 注：`0.75–0.90` 箱 7 条、`0.90–1.00` 箱 44 条都够 5 条，但只要有**任一**箱不足就整体不下结论 —— 这是设计概要 §六的原话，实现照做。

---

## 6 · 不退化

### 6.1 红线与禁止项

| 项 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` | `git diff 4573269..HEAD` **空** |
| `tests/test_fk_naming.rs` | **空** |
| `src/domain/view.rs` | **空** |
| `src/domain/review_view.rs` | **空** |
| `src/mcp/tools.rs`（含 `guard()` 内部） | **空** |
| `src/domain/property_filter.rs`（G1–G4 措辞） | **空** |
| 新增依赖 | `Cargo.toml` / `Cargo.lock` **零改动** |
| 删测试 / `#[ignore]` / 放宽断言 | diff 里**没有**任何 `-` 的 `#[test]` / `#[ignore]` / `assert` 行；新增 `#[ignore]` 0 条 |
| 源库连接串进被跟踪文件 | **无**（`git grep` 无 `postgres123` / `172.16.120.20`；只有库名 `anspire_open_uat`） |

测试文件里那 6 个 `+ confidence: None` 的改动我逐条看了：**都是 `LinkType` 新增字段后的结构体字面量补全**，
`test_req107_source_enumeration.rs` 的 +26 行是**新增登记条目**（两处新打印点 + 两个新函数 + 两个新模块），不是放宽断言。

### 6.2 REQ-107 的 13 条出口判据：**70 vs 74 = 口径差，不是退化**（补上了出处）

**我独立跑（`cargo test --test <name>`，每个都 exit 0）：**

| 套件 | passed | 套件 | passed |
| --- | --- | --- | --- |
| `test_req107_outlet_pipeline` | 12 | `test_req107_red_lines` | 2 |
| `test_req107_guard_granularity` | 4 | `test_property_filter` | 10 |
| `test_req107_zero_visible_change` | 3 | `test_exit_ordering_req106` | 7 |
| `test_req107_pipeline_bytes` | 1 | `test_binary_output_guard` | 7 |
| `test_req107_scope_error_gate` | 4 | `test_graph_server` | 9 |
| `test_req107_track_manifest` | 2 | `test_guard_manifest_req081` | 6 |
| `test_req107_static_texts` | 3 | **合计** | **70 / 0 failed** |

**为什么不是「掉了 4 条」——两条硬证据：**

1. **逐文件 `#[test]` 静态计数，基线 vs HEAD 完全一致**（`git show 4573269:tests/x.rs` vs `HEAD`）：
   13 个文件里每一个的 `#[test]` / `#[tokio::test]` 数量都相同（合计静态 98，含 `cfg(test)` 嵌套模块与 `#[ignore]` 的），
   **没有任何一个文件少了测试**，diff 里也没有删除的测试函数。⇒ 这 13 个套件的测试函数集合与基线**逐一对应**，
   所以「基线上是 74、现在 70」这个假设**不可能成立**（少了 4 条就必有 4 个 `#[test]` 消失）。
2. **74 有一个能对上算术的解释（假设，未能核实）**：REQ-107 的 `qa-review-4.md` 写「本节所有 a/b = **13 个目标**的 passed/failed，基线 = **74**」，
   但**那 13 个目标的名单没有留在文档里**（只留了结论）。把 `test_guard_manifest_req081`(6) 换成
   `test_req107_source_enumeration`(10) —— 正好 **70 − 6 + 10 = 74**；而那 10 条我也跑了：**10 passed**。
   也就是说：**「13 个目标」换了一个成员就能整除出 74**，而两组成员现在都是全绿 ⇒ 无论当初是哪一组，都不存在「掉了 4 条」。

⇒ 结论：**A-5 的「登记口径差」成立**，且我额外证明了「13 个套件里没有任何一条转红」（70/70 全绿、无删测试）。
**不是真退化了 4 条**。

### 6.3 `--lib` / `--doc` / 全量

| 目标 | 我实测 | Dev 报 |
| --- | --- | --- |
| `cargo test --lib` | **217 passed / 0 failed** | 217 |
| `cargo test --doc` | **4 passed / 0 failed** | 4 |
| `cargo test`（全量） | ⛔ **本机不可跑**（见下） | 同 |

**全量不可跑的真实原因（比 Dev 写的更细，两处既有问题叠加）**：

- `src/bin/ontok-gate-preflight.rs` 用 `std::os::unix::fs::MetadataExt`（`dev()`/`ino()`）⇒ **文件在**时 `cargo test --test X` 会先编 bin，直接红；
- 把文件挪走 ⇒ `tests/test_retained_names_req084.rs:52` 的 `include_str!("../src/bin/ontok-gate-preflight.rs")` **编译失败**；
- ⇒ **`test_retained_names_req084` 在本机两种状态下都编不过**（既有问题，非本 REQ 引入，它不在 13 条里，Dev 也没声称跑过它）。

Dev 的绕行（临时挪开 → 跑 `--test` → 挪回 → sha256 前后比对）**是正确且唯一的办法**，我照做并复核：
`C7CF95E2…322D → C7CF95E2…322D`（identical = True，两次挪动都一致）。

### 6.4 附带回归（改动文件的邻域，我自己加跑，16 个套件 218 passed / 0 failed）

`test_apply`(21) · `test_snapshot`(10) · `test_snapshot_write_paths`(8) · `test_proposal`(9) · `test_model_view`(10) ·
`test_view`(26) · `test_review_show_e2e`(11) · `test_review_state`(28) · `test_db`(11) · `test_extracted_by_provenance`(9) ·
`test_review_provenance`(17) · `test_concept_proposal`(19) · `test_diff`(13) · `test_no_write_path`(4) ·
`test_provenance_source`(5) · `test_overview`(17) —— **全绿**，与 Dev 报的数字一致。

### 6.5 `items` 逐字节不变（AC-005 的后半条）

用 Dev 在 `develop` 上生成的 `proposal-baseline.json`（178 条：96 实体 + 82 关系，**0 条带 confidence**）与我的 `run1/proposal.json`（148 条：96 + 52）对比：

| 比对 | 结果 |
| --- | --- |
| 去掉 `confidence` 键后，我的 148 条里「baseline 中不存在」的条目 | **0** |
| baseline 里在我这儿缺失的条目 | **30 = 10 拒绝 + 20 低**（正是 AC-009 要求移出主提案的那些） |
| 96 条实体/属性条目（去 confidence 后规范化 JSON 逐条比对） | **逐条相同且同序** |
| baseline 里带 confidence 的条目数 | 0（说明基线确实是 REQ-108 之前的产物） |

⇒ `attach_confidence` 确实**只加了一个键**，抽取结果没有被改写；30 条的减少是路由的**预期行为**，不是丢条目。

---

## 7 · 阻断项 / 非阻断项

### 阻断项：**0 条**

13 条 AC 全部成立；红线零触碰；13 条出口判据全绿；确定性/只读/不落原值都有独立证据。

### 非阻断项（4 条，按建议优先级排序）

| # | 项 | 证据 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| **N1** | **`confidence` 数字可被「改文件」绕过**（攻击 f） | §3.2 全链路实测：0.99 + `zero_hit` 被 import/apply/建模视图全盘接受 | 「可复算」变成「可伪造」；审核排序可被操纵；但抽取结果本身不变 | `from_untrusted` 增三条：`score ≤ min(veto.cap)`、`signals[].weight` 必须等于 `method` 的权重表、`score` 必须等于按 signals 重算值。**设计概要 §2.1 本来就要求第 2 条** |
| **N2** | **`importable: false` 没有被强制** | 我把 `proposal-rejected.json`（10 条已知错误，含 E4–E7 的 0.20）原样 `propose import` → **EXIT=0，全部导入**；`proposal-pending.json` 同样 | 「不提案」只是**文件约定**；一次 `import` + `--accept-all` 就能把拒绝档灌进本体 | import 读顶层 `importable`（缺省 false 的语义要定清楚），或至少要求显式 `--allow-unscored` 之类的开关 |
| **N3** | **攻击 d 的字面口径不成立**（30/82 抬高；E1–E7 从「拒绝」降为「低」） | §3.4 | 与 A-2 的 0.45 上限一致，且不进主提案 | 按 §3.4 的措辞改写该条判据（属 AC 措辞问题，与 A-1 同源） |
| **N4** | 报告/呈现的三处小瑕疵 | ①「42 项逐字段相同」实为 36 个字段；②「审核界面默认折叠」在 CLI 里是**计数行**（44 条高档仍逐条打印），不存在交互折叠；③ E8/E9 的判据形式改写没有在 AC 里登记 | 不影响判据，影响可追溯性 | 下一轮把这三句改准 |

> 另有 **A-3 已登记的已知缺口**（客户视图守门词表不含 `confidence`）：我认可 BA 的裁定（红线零触碰优先），
> 并且本轮用「两个项目分数不同但客户视图逐字节相同」给它补了一道**结构性**证据。但它仍然是「测试钉住」而不是「词表挡住」——
> 若将来有人给 `SnapshotView` 加一个叫 `confidence` 的字段，词表不会红。

---

## 8 · AC 自己也会是错的（S 纪律，必须说出来）

| # | AC 原文的问题 | 我的裁定 |
| --- | --- | --- |
| S1 | **AC-006 对 E8/E9 字面不可满足**：「9 条必须全部落拒绝档」，但这两条在本实现里**不是候选**，永远不会进任何档 | 判据应改成「落拒绝 **或** 不被产出 + 构造复算」，见 §2.3 |
| S2 | **AC-007 的固定清单**（A-1 已修订）：我独立复算这 5 条的命中率是 `5/5 · 5/5 · 1/2 · 4/5(未产出) · 5/5`，**与 A-1 的判断一致** —— 旧的 1/5、4/5 是另一次非确定性采样的产物 | 修订后的机制判据成立（我实测只有 2 条部分命中，**0 条落高**） |
| S3 | **「`--no-sample` 不得高于采样过的同一条关系」字面不成立**（30 条反例），而语义上「被反驳 < 不知道」是对的 | 建议改写措辞，见 §3.4 |
| S4 | **AC-002 的 `LIMIT 5`** 与实现的「样本上限 5 + 扫描上界 25」不同 | 实现的选择**更正确**（设计概要 §四要求「去重后补足 5 个」；直接 `LIMIT 5` 会让分母随重复值漂移）；确定性未受影响，口径已在报告里写明 |
| S5 | **AC-012 的「74 passed」** | 口径差，见 §6.2；建议在 acceptance 里改成「13 个套件 **70** passed」以免下一轮再对不上 |

---

## 9 · 无法确认的项（如实说）

1. **全量 `cargo test` 的绿**：本机**不可证**（unix-only bin + `include_str!` 冲突，§6.3）。我能证的是「13 条出口判据 + 16 个邻域套件 + lib 217 + doc 4 全绿」，共 **509 个测试**（70 + 218 + 217 + 4）。
   `test_retained_names_req084`（REQ-084 的登记表判据）在本机**两种状态下都编不过**，**它有没有被本 REQ 弄坏，我无法确认** ——
   但它引用的 `src/bin/ontok-gate-preflight.rs` 与 `src/extract/propose.rs` 里，前者本次零改动；
   后者本 REQ 有大量改动（`link_candidates` / `score_links` / 路由），该测试会扫 `propose.rs` 的源码文本，
   **理论上有被新代码触发的可能**。⚠️ 建议在 Linux/CI 上补跑一次。
2. **82 条关系的语义对错**：评分卡只回答「有多可信」，谁对谁错要人判。
3. **权重是否最优**：需要校准数据（当前每箱样本都 < 5 条的箱存在 ⇒ ECE 不出；这就是 AC-011 想要的状态）。
4. **源库快照依赖**：我所有数字只对 2026-10-21 16:27 那一刻的 `anspire_open_uat` 成立（表 96 / 列 1116 / 外键 0 / 单列主键 95 表）。
5. **E8/E9 的「不产出」依赖 REQ-028 的歧义闸**：若词干唯一性变化，它们会重新进入候选（届时 0.15 会挡，已构造验证）。
6. **`--no-sample` 时「不建连接」**：我只从代码（装配层跳过 `sample_pool`）与运行日志（1.2s 完成、无采样）判断，未抓网络包。

---

## 10 · 复现命令（全部自带 `cd`，连接串只从环境变量来）

```powershell
cd D:\MyCodes\AI\an_onto_demo\.review-workspace\req108\qa
$env:CARGO_TARGET_DIR='D:\MyCodes\AI\an_onto_demo\feature-REQ-108\target'
$env:ONTOK_SOURCE_DATABASE_URL='postgresql://…/anspire_open_uat'   # 只读；口令不入库
$env:SRC_URL=$env:ONTOK_SOURCE_DATABASE_URL
$env:ONTOK_DATABASE_URL='postgresql://…/ontok_demo'

# ① 勘查 + 两次运行（逐字节比对）+ --no-sample
pwsh -NoProfile -File ..\..\qa-verify\run_propose.ps1
# ② 真源库判据（AC-002/003/006/007）
$env:REQ108_ARTIFACT_DIR='D:\MyCodes\AI\an_onto_demo\.review-workspace\req108\qa-out\run1'
cargo test --test test_req108_live -- --nocapture --test-threads=1
# ③ 我的攻击靶场（a/b/f/f2 + 对照）—— 期望 f、f2 **红**（红就是发现）
cargo test --test qa_attack_req108 -- --nocapture --test-threads=1
# ④ 13 条出口判据 + 邻域套件
#    （跑 --test 前需临时挪开 src/bin/ontok-gate-preflight.rs，跑完挪回并比对 sha256）
pwsh -NoProfile -File ..\..\qa-verify\run_suites.ps1
pwsh -NoProfile -File ..\..\qa-verify\run_collateral.ps1
# ⑤ 独立 oracle（E1–E9 / 原值回查 / 幂等 / 路由）
python ..\..\qa-verify\oracle_e1e9.py
python ..\..\qa-verify\analyze.py
python ..\..\qa-verify\check_ac003_ac005.py
python ..\..\qa-verify\nosample_cmp.py
```

**审核留痕位置**

| 内容 | 路径 |
| --- | --- |
| 我的两次运行产物 + `--no-sample` | `.review-workspace/req108/qa-out/{run1,run2,nosample}/` |
| 客户视图 / 建模视图导出 | `.review-workspace/req108/qa-out/views/` |
| oracle 与脚本 | `.review-workspace/req108/qa-verify/` |
| 套件结果日志 | `.review-workspace/req108/qa-verify/{suites,collateral,import}.log` |
| 攻击靶场（未跟踪，不随交付） | `.review-workspace/req108/qa/tests/qa_attack_req108.rs` |

---

## 11 · 给 BA 的裁定建议

1. **合并**：13/13 AC 成立、阻断 0 条。
2. **登记 A-6**：AC-006 对「不产出」的关系的判据形式（§2.3 / S1）。
3. **改写攻击 d 对应的措辞**（§3.4 / S3），并顺便把 AC-012 的「74」改成「70」（S5）。
4. **N1（分数的入口一致性校验）**：即使本轮不修，也请写进 `status.md` 的已知缺口 ——
   否则「score 可复算」这句话在**导入边界**上是不成立的（算法可复算，数字可伪造）。
5. **N2**：`importable: false` 要么强制、要么删掉这个字段（留一个「声明了但不生效」的字段比没有更糟）。
