# REQ-108 · 第 2/3 轮修复复验（P0 复验）

> **审核对象**：`feature/REQ-108` = **`4af61f5`**（基线 `develop` `4573269` + **9 个** `[Dev]` 提交）
> **被验树**：HEAD `4af61f533588e679b2b318369e720cab0d71d239`，tree **`4659124aafc199db15bd6564a9e14563e9e41509`**
> 　　（我的独立 worktree `.review-workspace/req108/qa2` detached 在此提交，tree 与 feature tip **逐位相同**；被跟踪文件零改动）
> **审核方式**：另建 worktree + 自己的二进制重跑；DB 事实用 `psycopg2` 另查；攻击全部走 **CLI 真进程**（不是读代码推断），
> 每条记录 **exit code + 首行文案**。
> **审核时刻**：2026-10-21 17:07–17:35（+08:00）；源库快照 = 该时段的 `anspire_open_uat`
> **本轮作业单**：`acceptance.md` §四 A-6~A-9、§五 A-10~A-12
> **上轮报告**：`qa-review.md`（N1 / N2 / R1 的原始复现方式，本轮按同样手法再打一遍）

---

## 0 · 结论

**PASS 带非阻断（阻断 0 条）**。

| 本轮要回答的问题 | 我的独立结论 |
| --- | --- |
| **N1**（手改 `score` 0.20→0.99，veto 原样）是否真封 | ✅ **封住**：`exit 1`，报「score 0.99 超过 veto zero_hit 的上限 0.20」 |
| **N2**（原样 import `proposal-rejected.json`）是否真封 | ✅ **封住**：`exit 1`，报「顶层声明 importable = false —— 这是被拒绝档，不能导入」 |
| **R1**（`{"score":0.99,"method":…}` 无 `signals`）是否真封 | ✅ **封住**：`signals` **缺失 / null / 空数组**三种都 `exit 1` |
| Dev 披露的「`4acbf5a` 上 t5 本来就红」是否属实 | ✅ **属实**（我回到 `4acbf5a` 复现：`9 passed; 1 failed`，失败点正是 `fn ensure_importable` 未登记） |
| 补登记是否等于放宽判据 | ✅ **不是放宽，是遵守规则**（`OUTLET_FILES` / `NON_OUTLET_FILES` / t5 函数体**零改动**；只加了一条用途登记；函数仍在 T1/T2/T9 写点扫描与 B5 重名判定范围内） |
| 补了之后还有没有别的未登记函数 | ✅ **没有**（独立扫描：`cli.rs 24/24`、`http.rs 18/18`、`serve.rs 5/5`，未登记 **0**、重名 **0**） |
| Dev 改的既有测试是否收紧而非放宽 | ⚠️ **净收紧**：删了 **1 条**旧断言（`signals == []` 的旧默认值，该行为已被 A-10 明令禁止），同提交新增的拒绝判据更强；**没有删测试、没有 `#[ignore]`、没有放宽安全断言** |
| 不退化（9/9 / 确定性 / 客户视图 / 测试 / 红线） | ✅ **全部成立**，且 3 份产物的 `items` 与上一轮**逐条 deep-equal** |

**非阻断 4 条**（§6）：新增绕过形态 **R5**（删 `veto` + 删 `sample_hit_rate` 后 0.99 仍能进本体）、测试数字口径、报告第 3 轮未更新、t5 不在标准回归集。

---

## 1 · 我验的确实是那棵树（不是「看起来像」）

```
feature tip 4af61f5  tree = 4659124aafc199db15bd6564a9e14563e9e41509
qa2 worktree HEAD    tree = 4659124aafc199db15bd6564a9e14563e9e41509   ⇒ 同一棵树
qa2 git status       = 空（只有未跟踪的 build.log）
feature-REQ-108      = 空 status；9 个提交；未 push
```

**二进制（我自己在 qa2 里从该树编的）**：

| 二进制 | sha256 |
| --- | --- |
| `ontok.exe` | `9E49D0506F218050164C217B14B4E855E1C1122DB23B28B1C2CA3594B12DC1E3` |
| `ontok-propose.exe` | `95F3CB50319183E056FC71775D2EDC817A6945EE4A874066DC3483BD9A0B5955` |
| `ontok-introspect.exe` | `A5F7F5E7C2FE7FC5FFEE25CD5D4E9CBBB1F5067D19D831EB97B0095DC3DFF2F5` |

**环境**：

| 项 | 事实 |
| --- | --- |
| 本地库 | `postgresql://postgres:***@172.17.0.87:5432/ontok_demo`；**未复用** Dev 污染的 `qa108run`，新项目：`qa108run2`（产物）/ `qa108r3gate`（攻击）/ `qa108r3honest`、`qa108r3tamper`（视图）/ `qa108r3apply3`、`qa108r3r5`（apply 与 R5） |
| 源库 | `172.16.120.20:30432/anspire_open_uat`，连接串只从环境变量来；introspect 自报 `transaction_read_only = on` |
| 源库快照 | **96 张基表 / 1116 列 / 0 外键 / 95 表单列主键**（另有 1 个 view）；勘查时刻 `2026-10-21T09:09:42Z` |
| schema 一致性 | 我重新 introspect 的 `schema-src.json` 与上一轮那份**去掉 `introspected_at` 后逐字段相同** ⇒ 源库没有漂移，本轮数字与上轮可比 |
| 绕行 | 跑 `cargo test --test` 时把 `src/bin/ontok-gate-preflight.rs` 临时挪出（**只在 qa2**），每轮跑完挪回；sha256 前后均为 `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D` |

---

## 2 · P0-1 · 十条攻击逐条（全部走真 CLI + 真库）

基线样本：`run1/proposal-rejected.json` 里的 **E5**（`llm_model_sort.model_id → llm_model`，0.20 / rejected / `zero_hit` / 5 行 0 命中），
每条攻击只改一处，逐条记录 exit code 与首行文案。**全部日志**在 `qa2-out/attacks/*.log`。

| # | 攻击（改了什么） | 期望 | **实测 exit** | 实测文案（首行，节选） | 判定 |
| --- | --- | --- | --- | --- | --- |
| 1 | `score 0.20 → 0.99`、`band → high`，`veto`/`signals` **原样** | ≠0，含「上限 0.20」 | **1** | `score 0.99 超过 veto zero_hit 的上限 0.20 —— 分数与证据不自洽（signals 里 sample_hit_rate 是 sampled=5 / matched=0）` | 🟢 封 |
| 2 | `signals[sample_hit_rate].weight 0.45 → 0.9` | ≠0，含「权重表」 | **1** | `signals 权重与 evidence-scorecard-v1 的权重表不一致：sample_hit_rate 的 weight = 0.9，权重表是 0.45 —— 分数不可复核` | 🟢 封 |
| 3 | 删掉证据信号 `sample_hit_rate`，**保留** `veto: zero_hit` | ≠0 | **1** | `声明了 veto zero_hit，但 signals 里没有它 —— 没有对应证据（veto 不能只写在声明里）` | 🟢 封 |
| 4 | **原样** import `proposal-rejected.json`（10 条，含 E1–E7） | ≠0 | **1** | `… 顶层声明 importable = false —— 这是被拒绝档，不能导入。` | 🟢 封 |
| 5 | `{"score":0.99,"method":"evidence-scorecard-v1"}`（**无 signals**） | ≠0（A-10） | **1** | `confidence 有 score/method 但缺 signals —— 有置信度但缺证据链…（A-10）` | 🟢 封 |
| 6a | `signals: []` | ≠0（A-10） | **1** | `confidence.signals 是空数组 —— 有置信度但缺证据链…（A-10）` | 🟢 封 |
| 6b | `signals: null` | ≠0（A-10） | **1** | 与 5 同一文案（`缺 signals`） | 🟢 封 |
| 7 | **完全没有 `confidence` 字段**的老提案 | =0（不许误伤） | **0** | `提案 p1 已导入，共 1 个条目` | 🟢 放行 |
| 8 | `confidence: null` | =0（Dev 裁定「未声明」） | **0** | `提案 p1 已导入，共 1 个条目` | 🟢 放行（裁定成立，见 §2.2） |
| 9 | `band → high`，`score` 不动（仍 0.20） | ≠0 | **1** | `confidence.band = high 与 score = 0.2 对不上（按阈值应为 rejected）` | 🟢 封 |
| 10 | **整个删掉 `veto`**（score 0.99 / band high / signals 仍 5 行 0 命中） | ≠0 | **1** | `score 0.99 超过 veto zero_hit 的上限 0.20 …（sampled=5 / matched=0）` | 🟢 封 |

> **#1 / #10 的区别值得单独说**：#10 把 `veto` 整段删了，但仍然被挡住 —— 说明上限是**由 `signals` 里的 `sampled/matched` 推出来的**（`evidence_vetoes`），
> 不是读文件里的声明。这正是 A-7 #1 想要的形态。

**额外攻击（不在作业单，我顺手打）**：

| 攻击 | exit | 文案 |
| --- | --- | --- |
| `veto.cap 0.20 → 0.99` | **1** | `confidence.veto.cap = 0.99 与 evidence-scorecard-v1 的记录不符（zero_hit 应为 0.20）—— 上限表被改过` |
| 绕过 import，直接改库 `proposal_items.payload.confidence` 为 0.99/high（seq=136，`partial_hit` 1/2）→ `review decide --accept-all` → `proposal apply` | **1** | `应用失败：关系条目的 confidence 不合法：score 0.99 超过 veto partial_hit 的上限 0.75 —— 分数与证据不自洽（signals 里 sample_hit_rate 是 sampled=2 / matched=1）` |
| 同上但**诚实**（不篡改）的对照 | **0** | 正常写出 v2 |

> apply 侧这条是**第二道闸的实测**：库里的 `payload` 已经被改成 0.99/high，`apply` 在写快照前仍然打回，且失败后该项目的 `ontology_versions` **仍只有 v1**（没有半成品快照）。

### 2.1 阴性对照（证明不是「什么都拒」）

| 文件 | exit | 结果 |
| --- | --- | --- |
| `run1/proposal.json`（148 条：96 实体 + 52 高/中关系） | **0** | `提案 p1 已导入，共 148 个条目` |
| `run1/proposal-pending.json`（20 条低档，`importable: true`） | **0** | `提案 p1 已导入，共 20 个条目` |
| 老提案（无 `confidence`，攻击 #7） | **0** | 1 条导入，payload 里**不补** `confidence` 键 |
| `confidence: null`（攻击 #8） | **0** | 同上，payload 里无 `confidence` 键（单测 `test_req108_ac014_legacy_item_without_confidence_still_imports` 钉住） |

### 2.2 对 #8（`confidence: null`）的裁定：**成立**

Dev 把 `null` 当「未声明」，理由是 JSON 的 `null` 不等于「0 分的分数」。我的判断：

1. **语义上成立**：`null` 不携带任何 `score`，因此没有「凭空抬高可信度」的通路；它降级为「未评分」，与老的人工提案同一档。
2. **行为上可证**：`NewItem::from_json` / `apply::link_from_item` 对 `None | Some(Null)` **都不调用** `from_untrusted`，payload 里也不会补出 `confidence` 键（已有单测断言）。
3. **风险方向是安全的**：`null` 只会让条目「没有分数」，不会让它变成高分；如果将来有人想靠它绕过，得到的是「未评分」，不是「高可信」。
4. **建议（非阻断）**：生产者（`ontok-propose`）永远不要写 `confidence: null`；采样失败应写 `data_evidence: "none"` 的**真实评分卡输出**（0.45 上限），而不是 null。当前生产者不产生 null，所以只是接口约定建议。

### 2.3 ⚠️ 新发现（不在作业单）：R5 · 删「veto + 数据证据信号」后 0.99 仍能进本体

我在打「删 veto」时发现一个 **#10 挡不住的组合**：

```
① 取 E5（0.20 / rejected / zero_hit / 5 行 0 命中）
② 删掉 veto 整段
③ 再删掉 signals 里的 sample_hit_rate（只留 name_match / not_all_null / type_compatible / target_is_key）
④ score → 0.99，band → high（不写 data_evidence）
⑤ ontok propose import  → EXIT=0（149 条全部导入）
⑥ review decide --accept-all → EXIT=0
⑦ proposal apply        → EXIT=0 → v2
⑧ DB / model-view：该关系 confidence = 0.99 / high，signals 里**没有** sample_hit_rate，
   data_evidence 缺省成 "sampled"
```

**为什么能过**（逐条对照 A-7 / A-10）：

| 校验 | 为什么没触发 |
| --- | --- |
| A-10「signals 缺失/空」 | signals **非空**（还剩 4 个结构信号） |
| A-14 #1「score ≤ 触发 veto 的上限」 | `sample_hit_rate` 被删 ⇒ `evidence_vetoes()` **推不出** `zero_hit`；`veto` 又被删 ⇒ 没有任何上限可查 |
| A-14 #2「权重逐项相等」 | 剩下 4 个信号的 weight 都合法 |
| A-14 #3「veto 必须有证据」 | 已经没有 veto 可查 |
| `data_evidence` | 缺省值 = `sampled`，但**没有规则要求 `sampled` 必须带 `sample_hit_rate`** |

**定性**：这是「**证据链完整性**」的缺口，不是「分数与证据矛盾」的缺口。它落在 BA 已登记的 **R4（未实现「按权重重算 score」）** 与 **R2（可伪造自洽证据）** 的同一类里：
只要允许「删掉不利证据」，任何上限/权重校验都可以被绕开，最终只有 `sample_digest` 回源库复采或「重算 score」能证伪。
按 A-12 的现行裁定（R2/R4 本轮不修），我判 **非阻断**；但它是**目前最省事的一条**（2 次删除 + 2 个数字），比 R2 的「伪造 5/5 再算分」更容易。

> **如果 BA 认为「删证据」不能算进 R2/R4 的已接受边界，那这条应改判阻断**，修法很小（`from_untrusted` 里约 5 行）：
> `data_evidence == Sampled` ⇒ 要求 `signals` 里必须出现 `sample_hit_rate`（且带 `sampled/matched`）；
> 或直接做 A-12 的 R4：按 `WEIGHTS` 重算 `score`，与声明值逐位比对。
> 两种都不会误伤真产出：`score()` 在 sampled 时**必定**产出 `sample_hit_rate`，在 `--no-sample` 时 `data_evidence=none` 且 0.45 上限已生效。

---

## 3 · P0-2 · 核实 Dev 主动披露的基线问题（本轮最关键）

### 3.1 「`4acbf5a` 上 `test_req107_source_enumeration::t5` 本来就是红的」——**属实**

我在 qa2 里 `git checkout 4acbf5a`（同一 worktree、同一 target），临时挪开 preflight bin 后跑该套件：

```
running 10 tests
test t5_outlet_files_declare_every_function ... FAILED
…
thread 't5_outlet_files_declare_every_function' panicked at tests\test_req107_source_enumeration.rs:1706:5:
⛔ 出口函数登记表与源码不一致：
  - src/cli.rs: `fn ensure_importable` **没有登记在 OUTLET_FNS 里** ——
    出口文件里出现新函数（例如一份本地判据 `fn local_ok(s) -> bool`）必须显式登记它的用途
test result: FAILED. 9 passed; 1 failed; 0 ignored
```

**对照**：

| 树 | `t5` | 说明 |
| --- | --- | --- |
| `3f32eea`（我上一轮审的树） | **10 passed** | 那时 `ensure_importable` 还不存在（AC-015 在 `4acbf5a` 才加） |
| `4acbf5a`（第 2 轮修复提交） | **9 passed / 1 failed** | 失败点唯一：`ensure_importable` 未登记 |
| `4af61f5`（HEAD） | **10 passed** | 补登记后绿 |

⇒ Dev 的披露**准确**，不是「先说是基线红的来掩盖自己弄红」。而且失败信息**精确指向新增的那个函数**，没有别的红。

### 3.2 补登记 = **遵守规则**，不是放宽判据

逐项核对「放宽」需要动的地方，**一处都没动**：

| 可能被用来「放宽」的开关 | 本轮 diff | 结论 |
| --- | --- | --- |
| `OUTLET_FILES`（被扫描的文件集合） | **零改动**（`src/cli.rs`、`src/server/http.rs`、`src/mcp/serve.rs`） | 没把 `cli.rs` 移出扫描 |
| `NON_OUTLET_FILES`（豁免名单） | **零改动** | 没把函数塞进豁免 |
| `t5` 函数体 / `fn_definitions` / `duplicate_fns` | **零改动** | 没改判据实现 |
| `OUTLET_FNS`（登记表） | **+1 条**：`("src/cli.rs", "ensure_importable", "导入前硬闸（A-8/A-15…）")` | 这正是 t5 要求的动作 |

**语义判定**：t5 的意图是「出口文件里**每一个** `fn` 都必须被显式登记用途」，它是一种 **declare-or-fail** 的登记闸，不是「函数必须长成某个样子」的白名单。
`ensure_importable` 是 `src/cli.rs` 里**真实新增**的函数，用途是「导入前的顶层闸」，登记它**如实描述**了用途；而且：

- 它**仍然**在 t1/t2/t9 的写点扫描范围内（T5 只登记，不豁免别的判据）；
- 它**仍然**在 B5 的重名判定范围内（同名第二个定义即红）；
- 它**不写**任何客户输出字节（只是读文件 + `bail!`），不构成「第二个出口判据」。

⇒ **合规（遵守规则）**。反过来说，如果 Dev 当时选择「把 `ensure_importable` 挪出 `cli.rs`」或「加进豁免名单」，那才是绕过；他没有。

> 唯一要记住的边界：`OUTLET_FNS` 的「用途」是**人写的字符串，测试不校验其真实性**。所以登记闸保证「必须声明」，不保证「声明为真」。这条性质与 REQ-107 的 B5 设计一致，本轮不构成新问题。

### 3.3 补登记之后，还有没有别的未登记函数——**0 条**

我没有复用 t5 的实现，而是自己写了一个等价语义的扫描器（去注释/去字符串 → 用花括号配平剥掉 `#[cfg(test)]` 块 → 行首 `fn` 定义 → 与 `OUTLET_FNS` 求差）：

| 出口文件 | 生产代码 `fn` 定义 | 已登记 | **未登记** | 登记了但源码没有 | 重名 |
| --- | --- | --- | --- | --- | --- |
| `src/cli.rs` | 24 | 24 | **0** | 0 | 0 |
| `src/server/http.rs` | 18 | 18 | **0** | 0 | 0 |
| `src/mcp/serve.rs` | 5 | 5 | **0** | 0 | 0 |

> 补充：如果不剥 `#[cfg(test)]`，`http.rs` 会多出 33 个「未登记」—— 那是测试模块里的 `fn a_clean_graph_is_served` 之类，不是出口函数。
> 我的扫描器显式做了 cfg(test) 剥离，结果与 t5 一致（t5 的 `prod()` 也是这么做的）。

### 3.4 这与「上一轮 300 passed 全绿」矛盾吗——**不矛盾，但有两个教训**

先把数字归位：

- **300 不是我上一轮的数字**，是 Dev 在 `verification-report.md` §10.5 写的「31 个套件 = 70 + 218 + 5 + 7」。
- 我上一轮报的是 **509**（`--lib` 217 + `--doc` 4 + 13 出口套件 70 + 16 邻域套件 218），并且**单独**跑了 `test_req107_source_enumeration` = **10 passed**（当时树是 `3f32eea`，函数还不存在 ⇒ 绿是真的）。
- 两边**都没有把 `test_req107_source_enumeration` 算进那 70/300**。所以 t5 在 `4acbf5a` 红，**不与 300 相加矛盾**——它只是不在那个集合里。

但确实有两个教训，都该写进流程：

1. **「31 套件全绿」这个说法有覆盖盲区**：它漏掉了一个 REQ-107 的出口判据套件，而那个套件在 `4acbf5a` **确实是红的**。数字本身是真的，但「全绿」作为结论**不完整**。
2. **我上一轮的「REQ-107 出口判据没有转红」只对我跑的 13 个套件成立**。我在 `3f32eea` 上单独跑 `source_enumeration` 是 10 passed，这个结论当时成立；但如果有人把它读成「所有 REQ-107 套件在整条分支上都绿」，那在 `4acbf5a` 就是错的。
   ⇒ 建议：**把 `test_req107_source_enumeration` 加进标准回归集**（下面 §9-3）。

---

## 4 · P0-3 · Dev 改动的既有测试：逐行判定

### 4.1 全部被删除的断言（`git diff 3f32eea..4af61f5`）

整轮修复（`4acbf5a` + `4af61f5`）里，**只有 2 行**带 `assert` 的代码被删除：

```diff
- assert!(from_untrusted(&serde_json::json!({"score": 0.9, "method": METHOD})).is_ok());
- assert_eq!(c.signals, vec![]);
```

逐条判：

| 被删断言 | 性质 | 判定 |
| --- | --- | --- |
| `from_untrusted({"score":0.9,"method":…}).is_ok()` | **重新加回**，只是补了 `signals`（仍断言 `.is_ok()`） | 等价，**未放宽** |
| `assert_eq!(c.signals, vec![])`（`absent_optional_fields_…`） | 断言的是**旧默认值**「缺 signals ⇒ 空数组」。A-10 已把该行为**明令禁止**，保留它等于要求实现保留漏洞 | 删除的是**旧的放行行为**，不是安全断言；同提交新增 `ac014_confidence_without_evidence_chain_is_rejected`（3 种形态都必须拒 + 1 条阳性对照）**更强**。⇒ **净收紧** |

> 结论：**没有任何安全断言被删除或放宽**。唯一被删的断言，其语义就是 A-10 要消灭的那个行为本身。

### 4.2 两个被点名的测试

| 测试 | 改动 | 断言数 | 判定 |
| --- | --- | --- | --- |
| `confidence.rs::absent_optional_fields_get_derived_not_invented_values`（注：在 **`src/domain/confidence.rs`** 的 `mod tests` 里，不在 `tests/`） | 两处 fixture 补 `minimal_signals()`；删 `signals == vec![]` 一条 | **6 → 5**（其余 `band` 派生 / `veto` 可空 / `data_evidence` 默认 **全部保留**） | **收紧**（旧行为被新测试反向钉死） |
| `tests/test_req108_confidence.rs::test_req108_review_confidence_table_shape` | 把「手拼的 `Confidence { signals: vec![], data_evidence: None }`」换成**真的 `score(--no-sample)` 输出** | **9 → 10**（+1：`assert_eq!(no_data.data_evidence, None)`），**0 条删除** | **收紧**（原来那个 fixture 在新规则下会被 `render_confidence_table` 判为「没有分数」而静默跳过，断言会变空转；换成真产出后断言才真的在验东西） |
| `tests/test_req108_confidence.rs::test_req108_ac008_import_rejects_item_without_method` | 只给「有 method 的同一份」补 `signals` fixture | **4 → 4**，无删无改 | **未变**（method 必填 / 分数搬进 payload / band 派生校验三条原样） |
| `tests/test_req107_source_enumeration.rs`（`OUTLET_FNS` ~L1594） | **+8 行**：补 `ensure_importable` 登记条目（含用途） | 该文件 0 条断言改动；t5 函数体 0 改动 | **遵守规则**（§3.2） |

> 另外一个佐证：`render_confidence_table`（`src/cli.rs:554`）对读不回来的 confidence 是 `continue`（静默跳过），所以旧 fixture（`signals=[]`）在新规则下**不会**让断言变红，而是让断言**失去验证对象**——Dev 换成真产出是必要的，不是「为了好看」。

### 4.3 全局测试面（不是只改这两个）

| 检查 | 结果 |
| --- | --- |
| 被删除的测试函数（`git diff 4573269..4af61f5` 中的 `-fn test_`） | **0 条** |
| 新增 `#[ignore]` | **0 条**（diff 里仅报告文本提及） |
| 13 个出口套件的静态 `#[test]` 数（基线 vs HEAD） | 逐个 **SAME**（12/2/4/10/3/7/1/7/4/9/2/34/3） |
| `tests/test_req108_confidence.rs` 的测试数 | **6 → 9**（+3：无 signals 拒 / 老提案放行 / importable 硬闸） |
| 新增文件 | `src/domain/confidence.rs`、`src/extract/sample.rs`、`tests/test_req108_confidence.rs`、`tests/test_req108_live.rs` |

---

## 5 · P0-4 · 不退化（全部本轮重跑，不引用 Dev 数字）

### 5.1 真实库：9 条已知错误**仍全部**落拒绝（9/9）

我重跑 introspect（真源库，只读）→ 两次 `ontok-propose`（新项目 `qa108run2`，真源库采样）：

| # | 关系 | 产物落点 | score | band | veto | 样本/命中 | digest（与上一轮实测记录**逐位相同**） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| E1 | `balance_operation_log.batch_id → gift_code_batch` | rejected | **0.20** | rejected | `zero_hit` | 5/0 | `sha256:1deb6586…` |
| E2 | `instance_manager.session_id → simulate_login_sessions` | rejected | **0.20** | rejected | `zero_hit` | 5/0 | `sha256:b67e5a08…` |
| E3 | `internal_operation_records.batch_id → gift_code_batch` | rejected | **0.20** | rejected | `zero_hit` | 3/0 | `sha256:f0b94ee5…` |
| E4 | `llm_model_peak_config.model_id → llm_model` | rejected | **0.20** | rejected | `zero_hit` | 4/0 | `sha256:a6177f79…` |
| E5 | `llm_model_sort.model_id → llm_model` | rejected | **0.20** | rejected | `zero_hit` | 5/0 | `sha256:64c14f73…` |
| E6 | `llm_provider.model_id → llm_model` | rejected | **0.20** | rejected | `zero_hit` | 5/0 | `sha256:81184b2d…` |
| E7 | `llm_provider_groups.model_id → llm_model` | rejected | **0.20** | rejected | `zero_hit` | 5/0 | `sha256:c38b3299…` |
| E8 | `dim_customer_binding.channel_code → marketing_channel` | **三份产物都没有**（`produced=false`） | **0.15**（构造复算） | rejected | `type_incompatible` | 5/0 | —— |
| E9 | `marketing_channel_customer.channel_code → marketing_channel` | **三份产物都没有**（`produced=false`） | **0.15**（构造复算） | rejected | `type_incompatible` | 5/0 | —— |

- E1–E7 的 **7/7 digest 与上一轮完全相同**（不是「也是 0 命中」这种弱一致，是**同一批值集合**）；
- E8/E9 走的是 `test_req108_live` 的**构造复算**（`candidate_for` → 真 `sample_one` 真库 5 行 → 真 `score_links`），输出：
  `score=0.15 band=rejected veto=type_incompatible sampled=5 matched=0 produced=false`；
- 如实区分两种证据：**7 条「产出且拒绝」+ 2 条「不产出 + 构造复算 0.15」**（A-6 要求，不混写）；
- 分档分布：**高 44 · 中 8 · 低 20 · 拒绝 10**（合计 82；`proposal.json` 148 条 = 96 实体 + 52 关系；59 条真的读到样本）。

### 5.2 两次 CLI 运行：4 份产物 sha256 **逐字节相同**

| 文件 | run1 = run2 | sha256 |
| --- | --- | --- |
| `proposal.json` | ✅ IDENTICAL | `5CA6091C3E2104C56621C7D5CECA8B8A41AE13F590EE83E223A8EDC7476D9BD1` |
| `stats.json` | ✅ IDENTICAL | `F1E6735511C579B8211A114CC5C8ED6D82472FBD6FFF7CC775913C5EA1D06560` |
| `proposal-pending.json` | ✅ IDENTICAL | `AAE9594C693E510CCAF665B203DE6A33AE3E3DBFF853D9DD9EBCB71936408D8D` |
| `proposal-rejected.json` | ✅ IDENTICAL | `61C95158F46B3A16F6A99E4FF352E22D6537ADE2D5A152B527DD780C221FC796` |

> `stats.json` 的哈希与上一轮、与 Dev 报告**完全相同**（它不含 project/时间戳）⇒ 跨运行、跨机一致。
> `proposal.json` 的哈希与上一轮不同，差异来自**顶层 `project` 字段**（`qa108run` → `qa108run2`，Dev 污染的旧项目我没复用）；`base_version` 两次都是 1。
> 我另外做了**逐条比对**：三份产物的 `items` 与上一轮 **deep-equal 全同**（`proposal` / `pending` / `rejected` 都是 `True`，含每条 `signals` 的 `sample_digest`），`stats` 也全同 ⇒ 评分/路由/证据**一个字节没动**。

### 5.3 客户视图：grep = 0；两个不同分数项目的视图**逐字节相同**

新建两个项目，喂同一份 148 条提案，只把**一条** high 关系的 `score` 从 `1.0` 改成 `0.99`（其余字段不动，自洽）：

| 判据 | 结果 |
| --- | --- |
| 诚实项目 v2 快照里该关系 | `confidence = 1.0`（`evidence-scorecard-v1`） |
| 篡改项目 v2 快照里该关系 | `confidence = 0.99`（`evidence-scorecard-v1`） |
| 客户视图 JSON（`ontology show`）grep `confidence|score|band|sample_digest|veto|evidence-scorecard` | **0 命中**（55,388 B） |
| 客户视图 HTML（`--html`）同上 | **0 命中**（34,848 B） |
| **两个分数不同项目的客户视图** | `customer.json` **逐字节相同**：`113A190EEFA737E572972BF7E8D42E5E5AF0940C6CF8ACBCF06038CDDF1A5DF6`；`customer.html` **逐字节相同**：`4B96D51CA9AAE110F00C886C9F026735979AD38551C846FEB4E4C57D22C852A5`（与上一轮 HTML 哈希也相同） |
| `--model-view` | `"confidence"` **52 次**（52 条关系每条都有） |

> 最后一条比 grep 更硬：**改了分数，客户视图一个字节都不动** ⇒ 客户视图根本不读这条通路，不是「恰好没打印」。

### 5.4 `--lib` / `--doc` / 31 套件（本轮实测）

**必须说明的一个插曲（如实披露）**：第一次跑 `--lib` 时有 2 条失败、**9 个依赖测试库的邻域套件**整片失败
（其余 7 个纯单测套件照常绿），原因是**我漏设 `ONTOK_TEST_DATABASE_URL`**（`tests/common/mod.rs:225` 与 `src/db/apply.rs:220` 会明文报错）。
补上该变量后**全部转绿**；下面是我补设后的干净数字（这不是代码问题，是我的环境问题，按纪律 L 记录在此）。

| 目标 | 我实测 | Dev 报 / status.md | 判定 |
| --- | --- | --- | --- |
| `cargo test --lib` | **227 passed / 0 failed** | 227 | ✅ |
| `cargo test --doc` | **4 passed / 0 failed** | 4 | ✅ |
| 13 个 REQ-107 出口套件 | **70 passed / 0 failed** | 70 | ✅ 逐套件数字与上一轮**完全相同** |
| 16 个邻域套件 | **218 passed / 0 failed** | 218 | ✅（`test_apply 21` / `test_view 26` / `test_review_state 28` / …） |
| `test_req108_live` | **5 passed / 0 failed**（真源库，29.4 s） | 5 | ✅ |
| `test_req108_confidence` | **9 passed / 0 failed** | 7 → 9 | ✅（+2 为本轮 A-10 新判据） |
| `test_req107_source_enumeration` | **10 passed / 0 failed** | 不在 31 内 | ✅（t5 绿，见 §3） |
| **31 套件合计**（按 §10.5 名单：13+16+live+confidence） | **302 passed / 0 failed** | 307 | ⚠️ **口径差 5 条**（见下） |
| 本轮总计（31 套件 + source_enumeration + lib + doc） | **543 passed / 0 failed** | —— | —— |

**302 vs 307 的解释（不是退化，是口径）**：
`70 + 218 + 5 + 9 = 302`；而 `70 + 218 + 9 + 10 = 307` —— 恰好是「把 `test_req108_live`(5) 换成 `test_req107_source_enumeration`(10)」。
两种口径**都全绿**，但 status.md / Dev 报的 307 **没有写明 31 个套件的名单**，所以对不上。建议下一轮把名单固定下来（§9-3）。

### 5.5 红线 / 禁止项 / 依赖

| 项 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` | `git diff 4573269..4af61f5` **空** |
| `tests/test_fk_naming.rs` | **空** |
| `src/domain/view.rs` | **空** |
| `src/domain/review_view.rs` | **空** |
| `src/mcp/tools.rs`（含 `guard()` 内部） | **空** |
| `src/domain/property_filter.rs`（G1–G4 措辞） | **空** |
| `Cargo.toml` / `Cargo.lock`（新依赖） | **空**（零新 crate） |
| 删测试 / 新增 `#[ignore]` | **0 / 0** |
| 源库连接串进被跟踪文件 | **无**（`git grep postgres123 / 172.16.120.20 / 172.17.0.87` 在 `4af61f5` 上 0 命中） |
| 提交数 | `4573269..4af61f5` = **9 个** `[Dev]` 提交 |

### 5.6 采样只读 / 不落原值（顺带复核）

| 证据 | 结果 |
| --- | --- |
| 我重新 introspect 的输出 | `只读确认: transaction_read_only = on` |
| `test_req108_live` 的 AC-002 | **passed**（同进程两次采样逐字段相同，`SHOW transaction_read_only` 读回 `on`） |
| `test_req108_live` 的 AC-003 | **passed**（4 份产物回查采样值命中 0） |

---

## 6 · 阻断项 / 非阻断项

### 阻断项：**0 条**

三条旧漏洞（N1 / N2 / R1）全部封住；apply 侧第二道闸也实测有效；红线零触碰；9/9、确定性、客户视图、测试全部成立。

### 非阻断项（4 条，按建议优先级）

| # | 项 | 证据 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| **N5** | **R5 · 删「veto + `sample_hit_rate`」后 0.99 仍进本体**（§2.3） | `qa2-out/attacks/r5-delete-evidence.json`：import 0 / decide 0 / apply 0 → 快照 v2 `confidence=0.99/high`，signals 里没有 `sample_hit_rate`，`data_evidence` 缺省成 sampled | 在同一威胁模型（手改提案 JSON）下，**两处删除 + 两个数字**即可让「拒绝档」变「高档」；比 R2 更省事。**按 A-12 的 R2/R4 已接受边界判非阻断**，但它是该边界里最便宜的一条 | 下一轮二选一：①`data_evidence == sampled` ⇒ 必须带 `sample_hit_rate`（约 5 行）；②做 R4「按 `WEIGHTS` 重算 score 必须逐位相等」。⚠️ 若 BA 认为「删证据」不在 A-12 的 R2/R4 覆盖内，则本条应**改判阻断** |
| **N6** | **31 套件数字口径 302 vs 307** | 我实测 `70+218+5+9=302`；`70+218+9+10=307` 也能整除（用 source_enumeration 换 live） | 不影响绿/红，只影响可追溯；下一轮还会对不上 | 在 status/报告里**写明 31 个套件的名单**（或直接按「全部 `test_*.rs`」自动枚举） |
| **N7** | **第 3 轮没有更新交付报告；`verification-report.md` 没有 §11** | `git show --stat 4af61f5` 只改 3 个文件（confidence.rs + 2 个测试）；报告止于 §10.6 | 本轮作业单让读「§10/§11」，但 §11 不存在；第 3 轮行为只能从 commit message / BA status 得知 | 补一节 §11（R1 修复 + t5 基线说明 + 本轮数字 227/4/302），并把「t5 在 4acbf5a 红」写进报告（现在只在 commit message 里） |
| **N8** | **`test_req107_source_enumeration` 不在标准回归集** | Dev §10.5 的 31 套件不含它；它恰是唯一抓到 `ensure_importable` 漏登记的判据 | 流程缺口：以后 `cli.rs` 新增函数仍可能「红了但没人跑」 | 把它加进标准回归集（变成 32 套件），或在 CI 里跑「全部 `test_*.rs`」 |

> 另外两条「已登记、本轮不重复计为问题」：**R2**（伪造完整自洽证据）、**R3**（结构性 veto 无对应信号）、**R4**（未实现重算）——
> 按 A-12 仍是已知限制，**没有被本轮修复，也不应写成「已封堵」**。N5 建议并入 R4 的登记文案。

---

## 7 · 无法确认项（如实说）

1. **全量 `cargo test` 的绿**：本机仍不可证（`src/bin/ontok-gate-preflight.rs` 的 `std::os::unix` + `test_retained_names_req084` 的 `include_str!` 冲突，两者都是既有问题）。
   我能证的是「31 套件 302 + `source_enumeration` 10 + lib 227 + doc 4 = **543 passed / 0 failed**」，以及 13 个出口套件的静态测试数与基线逐一相同。
   `test_retained_names_req084` 在本机两种状态下都编不过，**它有没有被本 REQ 弄坏，我无法确认**（它引用的 `src/extract/propose.rs` 本 REQ 有改动，理论上有被新代码触发的可能）。⚠️ 建议 Linux/CI 补跑一次。
2. **9/9 的语义对错**：评分卡只回答「有多可信」；E1–E9 为什么错，是人判的（AC-006 的原始结论），本轮只复核「仍落拒绝 / 仍不产出」。
3. **R5 的定性是政策判断**：它是不是「可接受边界」由 BA 裁；我按 A-12 判非阻断，并给了改判条件（§6 N5）。
4. **源库快照依赖**：所有数字只对 `2026-10-21T09:09:42Z` 的 `anspire_open_uat` 成立（96 基表 / 1116 列 / 0 外键 / 95 单列主键）。我比对过：与上一轮 schema **去掉时间戳后完全相同**，所以两轮可比。
5. **31 套件的原始名单**：Dev/status 都没留下完整名单；§10.5 的 70+218+5+7 是我能反推的唯一组合，但 307 的组成只能靠算术假设（§5.4）。
6. **`--no-sample` 时「不建连接」**：只从代码与运行日志判断，未抓网络包（与上一轮相同）。

---

## 8 · 复现命令与证据路径

```powershell
cd D:\MyCodes\AI\an_onto_demo\.review-workspace\req108\qa2
$env:CARGO_TARGET_DIR='D:\MyCodes\AI\an_onto_demo\feature-REQ-108\target'
$env:ONTOK_SOURCE_DATABASE_URL='postgresql://…/anspire_open_uat'   # 只读，口令只走环境变量
$env:SRC_URL=$env:ONTOK_SOURCE_DATABASE_URL
$env:ONTOK_DATABASE_URL='postgresql://…/ontok_demo'
$env:ONTOK_TEST_DATABASE_URL=$env:ONTOK_DATABASE_URL

# ① 重新勘查（真源库，只读）→ schema
ontok-introspect --out ..\qa2-out\schema-src.json
# ② 两次运行（确定性）→ qa2-out/run1、run2
ontok-propose --schema ..\qa2-out\schema-src.json --names src/extract/table_names.json `
  --out ..\qa2-out\run1\proposal.json --stats ..\qa2-out\run1\stats.json --project qa108run2 --src $env:SRC_URL
# ③ 攻击（每条一个文件，逐条记 exit）
ontok propose import --file ..\qa2-out\attacks\a1-score.json     # 期望 exit 1
# ④ 真源库判据（AC-002/003/006/007）
$env:REQ108_ARTIFACT_DIR='…\qa2-out\run1'
cargo test --test test_req108_live -- --nocapture
# ⑤ 全部套件（跑 --test 前临时挪开 src/bin/ontok-gate-preflight.rs，跑完挪回并比对 sha256）
pwsh -NoProfile -File ..\qa2-out\run_suites2b.ps1
# ⑥ t5 基线复现（4acbf5a / 3f32eea / 4af61f5）
git checkout -q 4acbf5a; cargo test --test test_req107_source_enumeration
```

**证据留痕**：

| 内容 | 路径 |
| --- | --- |
| 两次运行产物（4 份 ×2） | `.review-workspace/req108/qa2-out/run1/`、`run2/` |
| 攻击文件与逐条日志 | `.review-workspace/req108/qa2-out/attacks/`（`a1-score.log` … `r5-*.json`） |
| 客户视图 / 建模视图 | `.review-workspace/req108/qa2-out/views/` |
| 套件日志（干净版） | `.review-workspace/req108/qa2-out/suites2b.log` |
| t5 三个树的日志 | `.review-workspace/req108/parked/t5-4acbf5a.log`、`t5-HEAD.log`、`t5-3f32eea.log` |
| live 判据 --nocapture | `.review-workspace/req108/qa2-out/live-nocapture.log` |
| 我审的 worktree | `.review-workspace/req108/qa2`（detached `4af61f5`，tree 同 feature tip） |

---

## 9 · 给 BA 的裁定建议

1. **合并**：三条旧漏洞全部封住；阻断 0 条；不退化成立。
2. **R5 定性请拍板**：按 A-12（R2/R4 本轮不修）我判**非阻断并登记**；若你认为「删证据」超出已接受边界，则**改判阻断**，并把「`data_evidence=sampled` ⇒ 必须有 `sample_hit_rate`」列为本轮必修（约 5 行，不误伤真产出）。
   无论哪种，请把 N5 **写进交付物「已知限制」**，⛔ 不要写成「已封堵」。
3. **补测试名单**：把 `test_req107_source_enumeration` 加进标准回归集（32 套件），并在 status/报告里**固定 31（或 32）个套件的名单**，避免 302 vs 307 这种口径差再出现。
4. **补报告 §11**：第 3 轮（R1 修复 + t5 补登记）没有更新 `.feature/verification-report.md`；本轮作业单里的「§11」目前不存在。请 Dev 补齐，并把「t5 在 `4acbf5a` 本就红」的事实写进去（现在只在 commit message 里）。
5. **A-10 的 `confidence: null` 裁定**：我判**成立**（null ≠ 有分数，方向安全）；建议顺手在 design-summary 写一句「生产者不得输出 `confidence: null`，采样失败应输出 `data_evidence=none` 的真评分卡结果」，把这个语义钉住。
6. **A-12 的措辞**：建议把 R4 的登记描述从「未实现重算」扩成「未实现重算 / 未要求证据链完整（删证据可绕开 veto 上限）」，并附上 N5 的复现文件——否则下一位读者会以为「权重 + veto 双向锁死」已经够了。
