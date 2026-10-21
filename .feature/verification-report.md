# REQ-108 · 带证据与置信度的关系抽取 —— 交付报告（按 AC-013 八项）

> **代码基线** `develop` `4573269`（REQ-107 已合并）
> **分支** `feature/REQ-108`（**未 push**）；8 个 `[Dev]` 提交（第 2 轮 +1，见 §6.4）
> **设计文档** `BA/demands/REQ-108/design-summary.md`（实现前定稿；本文件只引用不重写）
> **实测源库** `anspire_open_uat`（**96 表 / 1116 列 / 0 外键**，2026-10-21 只读勘查）
> **本机限制**：全量 `cargo test` 编不过（`src/bin/ontok-gate-preflight.rs` 的
> `std::os::unix`，既有问题，本 REQ **未改那个文件**；绕行与 sha256 见 §6.3）
> **上一份报告**：REQ-107 的交付报告在 `develop` 的同名路径上（本文件已按派单要求覆写该路径）

---

## 0 · 结论先行

| AC | 结果 | 证据位置 |
| --- | --- | --- |
| AC-001 勘查补采主键/唯一列 | ✅ 96/96 表带 `primary_key` + `unique_columns`（连 `pg_catalog`，不猜列名） | §2 / §7.1 |
| AC-002 采样只读 + 确定性 | ✅ `SHOW transaction_read_only = on`；两次采样逐字段相同（含 digest） | §5.1 |
| AC-003 不落原值 | ✅ 42 个可判别样本值 × 4 份产物，**命中 0** | §5.2 |
| AC-004 veto 优先于加权和 | ✅ 4 类上限逐类执行（单测），且证明「不做 veto 时加权和会更高」 | §4.1 |
| AC-005 可复算 + 不改抽取产物 | ✅ 两次运行的 4 份产物**逐字节相同**（sha256 相同） | §4.2 |
| AC-006 9 条已知错误落拒绝 | ✅ **7 条真产出且落拒绝**（`zero_hit` 0.20）+ **E8/E9 不产出 + 按坐标构造复算 0.15**（两种证据，见 §3.2；A-6 已登记） | §3.2 |
| AC-007 5 条部分命中不落高 | ⚠️ **机制 ✅ / AC 的 5 条具体预期在本采样口径下有 3 条不成立** | §8.1（**最该看的一节**） |
| AC-008 `method` 必填 | ✅ 缺 method ⇒ `propose import` 非 0 退出（单测 + 真 CLI） | §4.3 |
| AC-009 分档路由 | ✅ 主提案 52 · 待查 20 · 不提案 10；拒绝档在 `proposal.json` 里 0 条 | §3.1 |
| AC-010 客户视图零泄漏 | ✅ 客户视图 `confidence`/`score`/`band` 全无；model-view 52/52 条都有 | §4.4 |
| AC-011 校准命令 | ✅ 空库 ⇒ 明确「样本不足」；有数据 ⇒ 五档表（样本不足的箱不印百分比） | §4.5 |
| AC-012 红线与不退化 | ✅ 四文件 `git diff` 为空；13 条出口判据 **70 passed / 0 failed** | §6 |
| AC-013 交付证据 | ✅ 本文件 §1–§8 + **§10（第 2 轮）** | — |
| **AC-014 自证链（第 2 轮新增）** | ✅ 三条校验落地：`score ≤ 按证据推出的 veto 上限` / 权重逐项相等 / veto 必须有证据信号。6 种攻击全部 exit≠0，真产物 6 种形状全部放行 | **§10.1** |
| **AC-015 `importable: false`（第 2 轮新增）** | ✅ `propose import` 遇 `false` ⇒ exit 1；被拒绝档导不进；待查档（`true`）仍可导 | **§10.2** |

**最可能被 QA 打回的一点**：AC-007 的**那 5 条具体关系**在本实现规定的采样口径
（`ORDER BY 主键 LIMIT`）下有 3 条是 **5/5 全命中**，因此落「高」——它们不是
「机制坏了」，而是**验收文档里那几个 1/5、4/5 是另一种采样口径测出来的**。
详见 §8.1，我**没有**为了让这条 AC 变绿去改采样口径。

---

## 1 · 各阶段改了什么（逐条 `path:line`）

| 阶段 | 改动 | 挂点 |
| --- | --- | --- |
| 1 | `src/extract/introspect.rs:109` `TableKeys` / `:153` `KeyCatalog` / `:183` `inject_into_product` / `:217` `from_product` / `:257` `fold_keys`（纯函数）/ `:522` `key_catalog`（**独立只读会话**）/ `:579` `introspect_with_keys`；`src/bin/ontok-introspect.rs:94` 注入产物 + `:82` 当场打印主键覆盖 | AC-001 |
| 2 | `src/extract/sample.rs`：`:44` `SAMPLE_TARGET=5` / `:52` `SAMPLE_FETCH=25` / `:55` `SAMPLE_TIMEOUT=2s` / `:63` `SampleEvidence`（**类型上没有原值字段**）/ `:110` 取样 SQL / `:129` 命中 SQL / `:141` `digest_of` / `:173` `sample_pool`（校验 `SHOW transaction_read_only`）/ `:193` `sample_one` / `:275` 自实现 SHA-256 | AC-002/003 |
| 3 | `src/domain/confidence.rs`：`:52` `METHOD` / `:60` `WEIGHTS` / `:103` `Band` / `:396` `score`（**纯函数**）/ `:585` `from_untrusted`（AC-008 的入口判据）/ `:679` `calibration_bins` / `:717` `ECE` | AC-004/005 |
| 4 | `src/extract/propose.rs:1392` `LinkCandidate` / `:1421` `link_candidates` / `:1461` `score_links` / `:1538` `attach_confidence`；`src/db/pg.rs:411` `ALTER_PATCHES`（3→9，六条新列）；`src/db/proposal.rs:88` `NewItem::from_json`（method 必填）/ `:198` INSERT 三栏；`src/domain/snapshot.rs:188` `LinkType::confidence`；`src/db/snapshot_store.rs:228/:403` 读/写三栏；`src/domain/apply.rs:280` apply 带出置信度；`src/bin/ontok-propose.rs:126-128` `--no-sample` / `--src` | AC-008 |
| 5 | `src/extract/propose.rs:1559` `Routing` / `:1592` `route_items`；`src/bin/ontok-propose.rs:561-620` 三份产物（proposal / pending / rejected）；`src/domain/model_view.rs:101/:175` 建模视图 `confidence`；`src/cli.rs:540` `render_confidence_table` + `review show --confidence`（`:259`） | AC-009/010 |
| 6 | `src/cli.rs:880` 分发 / `:946` `confidence_cmd`（`ontok confidence calibrate`） | AC-011 |
| 7 | `tests/test_req108_live.rs`（AC-002/003/006/007 + 分档实测）；`tests/test_req108_confidence.rs`（AC-008/009/010 + 校准表）；`tests/test_req107_source_enumeration.rs` 登记表补 2 条写点 + 2 个函数 | AC-006/007 |

---

## 2 · 评分卡规格与权重表（AC-013-1）

`evidence-scorecard-v1`（`src/domain/confidence.rs`）：

| 信号 | 权重 | 取值 | 为什么是这个权重 |
| --- | --- | --- | --- |
| `sample_hit_rate` | **0.45** | 5/5→1.0 · 4/5→0.8 · 3/5→0.5 · 2/5→0.3 · 1/5→0.15 · 0/5→0.0 | 零外键库上「列名推断」实测只有 58% 准确，**数据证据是唯一能推翻列名猜测的东西** |
| `name_match` | 0.20 | 直接展开→1.0 · 泛化名→0.3 · 其它→0.6 | 名字有用但不能主导（泛化名实测 0/5） |
| `not_all_null` | 0.15 | 有非空→1.0 · 全空→0.0 | 空列会让命中率失去意义，但「空」不等于「关系错」（弱信号） |
| `type_compatible` | 0.10 | 同族→1.0 · num↔str→0.2 · 其它→0.0 | 类型不相容已是**硬反驳**（veto 0.15），信号只做微调 |
| `target_is_key` | 0.10 | 主键→1.0 · 单列唯一→0.9 · 普通/不知道→0.4 | 结构合理性，不是数据证据 |

数据证据 0.60 / 结构证据 0.40。`semantic_fit` **留位不接**（不在权重表里、不参与归一化）。

**算法**：`基线 = Σ(wᵢ·sᵢ)/Σ(wᵢ)`（只对可用信号归一化）→ `最终 = min(基线, 所有 veto 上限的最小值)` → 两位小数 → 分档 0.90/0.60/0.30。

**veto 表**（9 条，含 1 条本 REQ 新增，逐条登记在 §8.3）：

| 触发 | 上限 | 信号 |
| --- | --- | --- |
| 类型族不相容 | 0.15 | `type_incompatible` |
| 采样 0 命中 | 0.20 | `zero_hit` |
| 泛化列名 + 无数据证据 | 0.40 | `generic_name_no_evidence` |
| 源列全空（`sampled = 0`） | 0.45 | `all_null` |
| 部分命中（0<rate<1） | 0.75 | `partial_hit` |
| 复合键（R2） | 0.50 | `composite_key_unsupported` |
| 多态目标（R3） | 0.40 | `polymorphic_target` |
| 自引用非唯一（R4） | 0.50 | `self_ref_non_unique` |
| **`--no-sample`（无数据证据）** | **0.45** | **`no_data_evidence`**（新增，理由见 §8.3） |

---

## 3 · 真实库实测

### 3.1 82 条关系的分档分布（AC-009）

`ontok-propose --src …` 于 2026-10-21 对 `anspire_open_uat` 实测：

| 档 | 条数 | 去向 |
| --- | --- | --- |
| 高（≥0.90） | **44** | `proposal.json` |
| 中（0.60–0.90） | **8** | `proposal.json` |
| 低（0.30–0.60） | **20** | `proposal-pending.json`（**不进主提案**，可导入审核） |
| 拒绝（<0.30） | **10** | `proposal-rejected.json`（**不提案**，带 veto 理由，`importable: false`） |
| 合计 | **82** | 82 条全部有 `method`、全部有分档 |

> ⚠️ **订正（QA N4）**：上一版这张表写「审核界面默认折叠 / 逐条显示」——
> **CLI 里没有交互折叠**。`review show --confidence` 的实际行为是：表头给**四档计数**，
> 然后**所有条目按档位排序逐条打印**（44 条高档也逐条打印）。
> 「默认折叠」是**设计概要 §五 对人机界面的意图**，不是本实现 CLI 的行为；
> 写成实现行为是错的，此处订正。

- 主提案 = 96 实体 + **52** 条关系（44 高 + 8 中）；`stats.json` 与基线一致。
- **「不提案」档在 `proposal.json` 里 0 条**（实测 `band == 'rejected'` → 0；`low` → 0）。
- ⚠️ 验收文档写 **81** 条，本次实测 **82** 条 —— 差 1 条来自 REQ-028 的尾缀匹配
  （`demand.md` 记的 81 是更早一版抽取器的数）。这是**数据事实**，没有改代码去凑 81。
- 82 条里 **59 条真的读到了样本**（其余 23 条源列全空 ⇒ `all_null` / `sampled = 0`）。

### 3.2 9 条已知错误逐条实测（AC-006 · 按 A-6 分成**两种证据**）

> ⚠️ **A-6 要求：这两种不许混写。** 「真产出且落拒绝」与「不产出 + 构造复算」
> 是**不同强度、不同形式**的证据：前者证明「提了、被打掉」，后者证明
> 「压根没进候选集，且如果它进了也会被打掉」。把它们并成一句「9/9 拒绝」会
> 掩盖「本实现其实永远不会产出这两条」这个事实。

#### 3.2.1 E1–E7：**真产出、真被拒绝**（三份产物里出现在 `proposal-rejected.json`）

| # | 关系 | 分数 | 档 | veto | 样本/命中 |
| --- | --- | --- | --- | --- | --- |
| E1 | `balance_operation_log.batch_id → gift_code_batch` | 0.20 | 拒绝 | `zero_hit` | 5/0 |
| E2 | `instance_manager.session_id → simulate_login_sessions` | 0.20 | 拒绝 | `zero_hit` | 5/0 |
| E3 | `internal_operation_records.batch_id → gift_code_batch` | 0.20 | 拒绝 | `zero_hit` | 3/0 |
| E4 | `llm_model_peak_config.model_id → llm_model` | 0.20 | 拒绝 | `zero_hit` | 4/0 |
| E5 | `llm_model_sort.model_id → llm_model` | 0.20 | 拒绝 | `zero_hit` | 5/0 |
| E6 | `llm_provider.model_id → llm_model` | 0.20 | 拒绝 | `zero_hit` | 5/0 |
| E7 | `llm_provider_groups.model_id → llm_model` | 0.20 | 拒绝 | `zero_hit` | 5/0 |

#### 3.2.2 E8/E9：**本实现不产出**（`marketing_channel` 与 `rpt_dws_channel` 双命中 ⇒ REQ-028 闸 B 歧义 ⇒ 不是候选）+ **按坐标构造复算 0.15**

| # | 关系 | 三份产物 | 构造复算（真采样 + 真评分） | 独立类型事实 |
| --- | --- | --- | --- | --- |
| E8 | `dim_customer_binding.channel_code → marketing_channel` | **都不在**（不产出） | **0.15 / 拒绝 / `type_incompatible`**，`sampled=5 matched=0` | `character varying` vs `integer` |
| E9 | `marketing_channel_customer.channel_code → marketing_channel` | **都不在**（不产出） | **0.15 / 拒绝 / `type_incompatible`**，`sampled=5 matched=0` | 同上 |

构造复算走的是**同一条评分路径**（`candidate_for()` → 真 `sample_one()` → 真
`score_links()`），`tests/test_req108_live.rs` 里 `produced=false` 一栏就是这条证据。
「不产出」比「拒绝」更强，但**两者不是同一条判据** —— 所以两条都给。

判据：`cargo test --test test_req108_live -- --nocapture`（需 `SRC_URL`），
`test_req108_ac006_nine_known_errors_all_rejected` 通过。

---

## 4 · 视图与入口的实测

### 4.1 AC-004：veto 没被加权和救回来

`vetoes_survive_maxed_structural_signals` 对四类上限逐条断言，并且**证明了判别力**：
每条都算出「不做 veto 时的加权和」，断言「基线 > 上限 → 最终分 < 基线」。
例：类型不相容那一类的基线是 **0.92**（name_match=1.0、target_is_key=1.0），
最终分 0.15 —— 上限不是靠把结构信号调低换来的。

### 4.2 AC-005：两次运行逐字节相同

同一份 `schema.json` + 同一份数据跑两次 `ontok-propose`：

| 产物 | sha256（前 16 位） | 两次相同 |
| --- | --- | --- |
| `proposal.json` | `58FA79439FD71298` | ✅ |
| `stats.json` | `F1E6735511C579B8` | ✅ |
| `proposal-pending.json` | `40C0AEDB7E51CEBA` | ✅ |
| `proposal-rejected.json` | `5D039F1F939ACBEE` | ✅ |

⇒ `items` 段、`confidence` 段（含 `sample_digest`）**都没有差异**。

`items` 相对 REQ-107 的差异只有两类，且都是本 REQ 要求的：
① 关系条目新增 `confidence` 键；② 拒绝/低档条目**移出**主提案（AC-009 的判据本身）。
实体条目与关系条目的其余字段逐字节未变，由
`test_attach_confidence_adds_one_key_and_changes_nothing_else` 钉住
（去掉 `confidence` 后与原文逐字节相同）。

### 4.3 AC-008：缺 `method` ⇒ 拒绝入库

- 单测 `test_req108_ac008_import_rejects_item_without_method`：`{"score":0.9}` ⇒
  错误信息含「没有 method 的分数不可复核」；`band` 与分数对不上也拒。
- 真 CLI（把 `proposal.json` 里第一条关系条目的 `confidence.method` 删掉后 import）：

```
$ ontok propose import --file …\proposal-nomethod.json
✗ item 97 的 confidence 不合法：confidence 缺 method —— 没有 method 的分数不可复核（AC-008）
exit code = 1
```

### 4.4 AC-010：客户视图零泄漏 / 建模视图必须有

- 客户视图（`ontology show`，JSON 与 HTML 同一份 `CustomerView`）：
  `confidence` / `"score"` / `"band"` / `evidence-scorecard` / `sample_digest` **全部 false**
  （实测 49,039 字节）。**结构性保证**：客户视图的投影输入 `domain::view::SnapshotView`
  根本没有这个字段（`src/domain/view.rs` 是红线，本 REQ 一个字未改）。
- 建模视图：`link_types` **52 条，52 条都有 `confidence`**，`method` 全为 `evidence-scorecard-v1`；
  旧版本是 `null`（键在、值空），不是缺字段。
- 审核界面：`ontok review show --proposal p1 --confidence` 输出四档计数 + 逐条
  档位/分数/证据摘要/veto 理由。

### 4.5 AC-011：校准命令

```
ontok confidence calibrate --project req108empty      # 空库
  → 计分条目 0 条 … 结论：**样本不足**（没有任何已决的计分条目）—— 不输出命中率结论

ontok confidence calibrate --project req108final      # 52 条已决
  分箱          条数   人工接受   接受率     平均分
  0.00–0.30        0         0         —        —
  0.30–0.60        0         0         —        —
  0.60–0.75        1         1     样本不足     0.70
  0.75–0.90        7         7    100.0%     0.84
  0.90–1.00       44        44    100.0%     0.99
  → 结论：**样本不足** —— 以下分箱 < 5 条：…。不输出命中率结论、不算 ECE。
```

「样本不足」的箱**不印百分比**；空箱印 `—`。ECE 只在**每箱都 ≥5** 时才计算。

---

## 5 · 采样三件事（AC-002 / AC-003）

### 5.1 只读 + 确定性

| 判据 | 证据 |
| --- | --- |
| 只读会话 | `sample_pool()` 连上后 `SHOW transaction_read_only` 必须 = `on`，否则 `bail`（fail-closed）。`test_req108_ac002_sampling_is_read_only_and_deterministic` 实测通过 |
| 只发 SELECT | 本模块只有两条 SQL（取样 / 命中），无 INSERT/UPDATE/DELETE/DDL |
| 确定性 | 取样 `ORDER BY <主键> LIMIT $1`（主键来自 AC-001 的目录事实）；无主键时按值排序。**禁止** `random()` / `TABLESAMPLE`（单测对 SQL 字面量断言） |
| 两次相同 | 同一次运行内两次 `sample_all_with` 的 **9 条 × 4 字段 = 36 个字段**逐字段相同（含 digest），且整体 `assert_eq!`；两次 CLI 运行的 `sample_digest` 使产物**逐字节相同**（§4.2） |

> ⚠️ 口径：AC-002 写「`LIMIT 5`」，本实现是「**样本上限 5** + 扫描上界 25」
> —— 前 5 行里有重复值时仍能补足 5 个**不同**的值（设计概要 §四 明文要求「去重后补足」）。
> 排序键与上界都是常量，确定性没有被放宽。

### 5.2 不落原值

`test_req108_ac003_artifacts_contain_no_sampled_values`：
**独立**把 9 条关系的前 5 个原值查出来（不复用生产 SQL），在 4 份产物里做子串回查：

```
样本值 42 个（可判别 42 · 短值不可判别 0）
产物 4 份
命中 0
```

> ⚠️ **订正（QA N4）**：上一版把「42」写成了 §5.1 的「42 项逐字段相同」。
> 42 是**本节的样本值个数**，与「字段数」不是一回事：确定性判据比的是
> 9 条关系 × 4 个字段（`sampled` / `matched` / `digest` / `reason`）= **36 个字段**，
> 且整体 `assert_eq!`（比逐字段更强）。两处已分开写明。

产物里只出现 `sampled` / `matched` / `sample_digest`。`SampleEvidence` 的**类型上**
没有放原值的字段（`sample_signal_carries_only_aggregates` 逐键断言）。

### 5.3 digest 的已知边界（如实登记）

设计概要 §四 规定 `sha256(值排序后用 U+001F 连接)`。这个连接**不是单射**
（`["a","b"]` 与 `["a␟b"]` 同串）—— `test_digest_separator_boundary_is_documented_not_claimed_fixed`
把这条边界钉成**已知行为**而不是假装封堵。digest 只用于「两次运行是否同一批样本」。

---

## 6 · 测试数字与口径（AC-013-7）

### 6.1 能跑 / 不能跑

| 目标 | 结果 |
| --- | --- |
| `cargo check --lib` | ✅ 0 error |
| `cargo test --lib` | ✅ **226 passed / 0 failed**（第 1 轮 217 + 第 2 轮新增 9 条 AC-014 单测；见 §10.5） |
| `cargo test --doc` | ✅ **4 passed / 0 failed** |
| `cargo test --test <name>` | ✅ 需 §6.3 的绕行 |
| **`cargo test`（全量）** | ⛔ **本机不可跑**（见下） |

⛔ 全量编不过的**既有**原因（本 REQ 未引入、未修复）：
`src/bin/ontok-gate-preflight.rs` 使用 `std::os::unix::fs::MetadataExt`
（`dev()` / `ino()`）。**那个文件本次一个字节未改**（§6.3 的 sha256 前后一致）。

### 6.2 13 条 REQ-107 出口判据（+ 本 REQ 相关套件）

| 命令 | 结果 |
| --- | --- |
| `test_req107_outlet_pipeline` | 12 passed |
| `test_req107_guard_granularity` | 4 passed |
| `test_req107_zero_visible_change` | 3 passed |
| `test_req107_pipeline_bytes` | 1 passed |
| `test_req107_scope_error_gate` | 4 passed |
| `test_req107_track_manifest` | 2 passed |
| `test_req107_static_texts` | 3 passed |
| `test_req107_red_lines` | 2 passed |
| `test_property_filter` | 10 passed |
| `test_exit_ordering_req106` | 7 passed |
| `test_binary_output_guard` | 7 passed |
| `test_graph_server` | 9 passed |
| `test_guard_manifest_req081` | 6 passed |
| **13 条合计** | **70 passed / 0 failed** |

> 派单材料写「74 passed」。按 REQ-107 报告 §6.2 的这 13 个套件逐一实测是 **70** ——
> 差的 4 条我**没能复现出处**（不排除当时把另 1 个套件算进去了）。
> 结论不受影响：**这 13 个套件一个都没退化**。

其余跑过且全绿的套件（0 failed）：`test_req107_source_enumeration`(10) ·
`test_read_only_guard`(5) · `test_anchor_guard`(3) · `test_apply`(21) · `test_snapshot`(10) ·
`test_snapshot_write_paths`(8) · `test_proposal`(9) · `test_db`(11) ·
`test_concept_proposal`(19) · `test_model_view`(10) · `test_req108_confidence`(6) ·
`test_review_show_e2e`(11) · `test_review_show_binary_req072`(7) ·
`test_domain_print_sites_binary_req078`(4) · `test_no_write_path`(4) · `test_view`(26) ·
`test_overview`(17) · `test_diff`(13) · `test_mcp_server`(31 + 1 ignored) ·
`test_extract_cli_req019`(8) · `test_extract_cli_req020`(5) · `test_provenance_source`(5) ·
`test_extracted_by_provenance`(9) · `test_review_provenance`(17) · `test_review_state`(28) ·
`test_req108_live`(5)。

**局部合计**：`217（lib）+ 4（doc）+ 70（13 条出口判据）+ 237（其余 24 个套件）= 528` 全绿。

> 本节是**第 1 轮**的数字。第 2 轮（AC-014/015）复跑后的数字见 **§10.5**
> （lib 226、doc 4、31 个套件 300 passed）。第 1 轮报告的 217 条 lib 测试**一条没少**。

### 6.3 bin 绕行与 sha256

```
sha256(src/bin/ontok-gate-preflight.rs) before = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
  → 临时 Move-Item 到 .review-workspace/req108/
  → 跑 cargo test --test …
  → 立即 Move-Item 回来
sha256 after                                    = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
identical = True
```

每一次绕行都做这个前后比对，**全部一致**（会话内共 8 次）。
`git diff develop -- src/bin/ontok-gate-preflight.rs` 为**空**。

### 6.4 提交

```
6dbea24 [Dev] 阶段1：introspect 补采 primary_key / unique_columns（产物契约，连 pg_catalog 不猜列名）
253cc6a [Dev] 阶段2：只读确定性采样器（ORDER BY 主键 + 只落 sampled/matched/digest + 自实现 SHA-256）
9a1c76f [Dev] 阶段3：evidence-scorecard-v1 纯函数评分卡（5 信号 + 9 条 veto 上限 + 四档）+ 单元测试
cc629a3 [Dev] 阶段4：propose 组装 confidence（采样接线）+ 三栏落库 + method 必填拒绝入库
1b2e166 [Dev] 阶段5：分档路由（高/中进提案·低进待查·拒绝不提案）+ 建模视图与审核界面显示置信度 + 客户视图零泄漏
24cac8d [Dev] 阶段6：ontok confidence calibrate 分箱命中率（五档 + 样本不足拒绝给结论 + ECE）
3f32eea [Dev] 阶段7：真实库实测判据（AC-006 9/9 拒绝 · AC-002/003 证据）+ 交付报告
第2轮  [Dev] 第2轮：AC-014 自证链三条校验 + AC-015 importable:false 硬闸；订正报告 A-6/N4
（第 2 轮只有**一个**提交：代码与本节、§6.4 都在里面；提交号 = `git log -1 --format=%h`，分支未 push）
```

---

## 7 · 关键实现取舍（为什么会看到这些「多余」的东西）

1. **键信息走产物契约，不给 `SchemaReport` / `TableMeta` 加字段**：
   这两个类型在 `tests/test_fk_naming.rs`（**红线文件**）与
   `src/bin/ontok-gate-preflight.rs` 里有字面量构造，加字段会让它们编译不过
   —— 而 `src/agent/group.rs:197` 早就记着这条约束。所以键事实落在
   `schema.json` 的 `tables[i]` 上，由 `inject_into_product` / `from_product`
   互为逆运算（单测钉住往返）。
2. **`key_catalog` 是独立的一条只读会话**，没有塞进 `introspect()`：
   `tests/test_read_only_guard.rs` 按 `pub async fn introspect` 切函数体找
   `ensure_read_only(&read_only)?;`，塞进去会让 AC-101 的接线判据**静默掉出覆盖**。
   代价是多一次只读连接（~100ms）。
3. **`confidence` 在 `proposal.json` 里是条目顶层键**（设计概要 §2.1），
   但落库时**搬进 `payload`**：`apply` 那条路只拿得到 `ItemView.payload`；
   而 `NewItem` 加字段会让 10+ 个测试文件编译不过。落库的三栏与 payload 里那一份
   **是同一次解析的同一个值**。
4. **`review show --confidence` 是显式开关**：`review show` 的 stdout 被
   REQ-072/078 的真进程字节比对钉着，默认多打一行就是改契约（不是判据过时）。
5. **校准输出整段只写一次 stdout**：REQ-107 的写点登记表要求 cli.rs 里每处
   写点都登记；十几行逐条 `println!` 会把登记面撑大，改成一次拼好一次写出。

---

## 8 · 已知限制与「AC 自己也会是错的」

### 8.1 ⚠️ AC-007：机制成立，但那 5 条具体预期在本采样口径下有 3 条不成立

实测（`test_req108_live_band_distribution_report`，`--nocapture`）：

| 关系 | 验收文档 | 本实现实测 | 落档 |
| --- | --- | --- | --- |
| `dim_customer_binding.customer_id` | 4/5 | **5/5** | 高（0.99） |
| `enterprise_recharge_order.customer_id` | 1/5 | **5/5** | 高（0.99） |
| `order.subscription_id` | 1/2 | 1/2 ✅ | 中（0.70） |
| `service_apps.channel_id` | 4/5 | **未被推断出来**（闸 B 歧义） | — |
| `usage_reconciliation_log.subscription_id` | 3/5 | **5/5** | 高（0.92） |

**为什么不同**：验收文档的 1/5、4/5 是**另一种取样口径**（很可能是随机 5 行），
而设计概要 §四 规定的是「**按主键排序取前 5 行**」。同一张表里，表头几行与整表
的命中率可以差很远 —— 这正是设计概要 §九 **G4「采样只验存在性、不验覆盖率」**
登记的那个缺口。本次实测把 G4 从「理论缺口」变成了**有数字的缺口**。

**我没有做什么**：没有为了让 AC-007 变绿去改采样口径（比如换成 `md5(主键)` 排序
做伪随机分布）—— 那会让 AC-002「固定排序、可复现」的理由变得暧昧，而且是
**为一条 AC 反推实现**。机制的判据仍然成立并已被单测钉住：
`0 < matched < sampled ⇒ score ≤ 0.75 ⇒ band ≠ high`。

**如果 BA 要的是 AC 的字面结果**，可选的两条路（都需要 BA 拍板）：
① 把采样改成「头尾各取」（`ORDER BY 主键 ASC LIMIT 3` ∪ `DESC LIMIT 2`，
仍确定性、仍 ≤5 行）以提高覆盖率；② 把 AC-007 改写成机制判据。

### 8.2 E8/E9 压根不会被推断出来

见 §3.2。这是 REQ-028 的闸 B 在起作用（`channel` 命中 `marketing_channel` 与
`rpt_dws_channel` 两张表 ⇒ 歧义 ⇒ 不产出）。**没有改抽取规则**去让它们出现
（那会违反「不改既有抽取结果」）。

### 8.3 第 9 条 veto：`no_data_evidence`（本 REQ 新增，**必须由 BA 知悉**）

R1 ④ 要求「`--no-sample` 时分数只由结构信号算，且**不得因此把分数抬高**」。
但「只算结构信号」自然会把 `customer_id → customers` 算到 **0.80**（中档）
—— 比「采了 5 行 0 命中」（0.20）**高得多**，正是「没看反而分更高」。
设计概要的 8 条 veto 里没有一条管这件事，所以新增第 9 条：
`--no-sample` ⇒ 上限 **0.45**（落「低」档，只能进待查清单）。
**这是对设计概要的扩展，不是实现偏差**；如果 BA 不认，删掉它会让 `--no-sample`
路径重新出现「无证据 = 可信」的 fail-open。

### 8.4 客户视图的守门词表**没有**加 `confidence` / `score` / `band`

因为 `src/domain/view.rs` 是**红线**（禁改）。零泄漏靠的是**结构**：
客户视图的投影输入类型里没有这个字段（`tests/test_req108_confidence.rs` 钉住）。
代价：如果将来有人给 `SnapshotView` 加一个叫 `confidence` 的字段，
`scan_modeling_terms` **不会**替我们拦住它（词表里没这个词）。
登记为**已知缺口**，下一轮若允许动 `view.rs` 应当补进 `MODELING_TERMS`。

### 8.5 其它

- **采样窗口**：只看按主键排序的前若干行，不验覆盖率（G4）。
- **复合键**：识别但不支持（R2，上限 0.50）；`llm_auth_cache_trigger_state` 是全库唯一
  的复合主键表，本次实测没有关系指向它。
- **`confidence` 列的精度**：`REAL`（float4）；读出时显式 `::float8`，避免 f32 截断。
- **`--no-sample` 或没有源库连接串** ⇒ 全部关系落「低」档、**不进主提案**，
  并在 stderr 大声报出来。这是 R1 ④ 的直接后果，不是 bug。
- **采样超时**：单关系 2s，超时记 `sampled: 0`（`reason=timeout`）不阻塞整体。
  判据侧用 20s 跑确定性检查 —— 墙钟超时不是采样结果，不能让它把「确定性」变成偶发红。

---

## 9 · 复现命令（全部自带 `cd`）

```powershell
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-108
$env:ONTOK_DATABASE_URL='postgresql://…/ontok_demo'
$env:ONTOK_TEST_DATABASE_URL=$env:ONTOK_DATABASE_URL
$env:SRC_URL='postgresql://…/anspire_open_uat'      # 只读；口令不入库

cargo run --bin ontok-introspect -- --out ..\.review-workspace\req108\schema-src.json
cargo run --bin ontok-propose -- --schema ..\.review-workspace\req108\schema-src.json `
    --names src/extract/table_names.json `
    --out ..\.review-workspace\req108\run1\proposal.json `
    --stats ..\.review-workspace\req108\run1\stats.json --project req108probe
cargo run --bin ontok -- confidence calibrate --project req108probe
cargo test --test test_req108_live -- --nocapture      # AC-002/003/006/007
cargo test --test test_req108_confidence               # AC-008/009/010
```

---

## 10 · 第 2 轮修复（A-6 / A-7 / A-8 / A-9）

> **起因**：QA `qa-review.md` 把 N1（`confidence` 数字可被改文件绕过）与 N2
> （`importable: false` 声明不生效）判为「非阻断」；**BA 在第 2 轮改判为阻断**
> （`acceptance.md` §四 A-7 / A-8）。理由：本需求的全部承诺是「低分关系进不了本体」，
> 而这两条各用**一行**就能推翻它。
>
> **本轮范围**：只修这两条 + 两处报告订正（A-6 / N4）。
> **没动的**：评分卡数值（权重 / 阈值 / 9 条上限）、抽取规则、采样器、视图、依赖。

### 10.1 AC-014 · 自证链：`confidence` 必须与证据自洽

#### 三条校验加在哪（`path:line`）

| # | 校验 | 实现位置 | 实测报错 |
| --- | --- | --- | --- |
| 1 | `score ≤ 每个**触发** veto 的上限`（上限由**证据**推，不由文件声明推） | `src/domain/confidence.rs:793`（`validate_self_evidence` 第 4 段），调用点 `:877` | `score 0.99 超过 veto zero_hit 的上限 0.20 —— 分数与证据不自洽（signals 里 sample_hit_rate 是 sampled=5 / matched=0）` |
| 2 | `signals[].weight` 与 `method` 的权重表**逐项相等** | `src/domain/confidence.rs:735` | `signals 权重与 evidence-scorecard-v1 的权重表不一致：sample_hit_rate 的 weight = 0.9，权重表是 0.45 —— 分数不可复核` |
| 3 | 每个 `veto` 信号名必须在 `signals` 里有**对应证据** | `src/domain/confidence.rs:781` | `声明了 veto zero_hit，但 signals 里没有它 —— 没有对应证据（veto 不能只写在声明里）` |

**一个函数、两个调用点**：`propose import` → `db/proposal.rs:109` → `from_untrusted`；
`proposal apply` → `domain/apply.rs:309` → **同一个** `from_untrusted`。
不存在「入口校验一套、apply 校验另一套」。

#### 为了让这三条真的成立，另外做了四件配套（都在 `confidence.rs`）

| 配套 | 位置 | 它堵住的形态 |
| --- | --- | --- |
| **上限表只有一份**：`VETO_CAPS` 表 + 9 个私有常量；`score()` 原来 9 处字面量改成引用常量 | `:68-101`（常量区）、`:90`（表） | 「把 `veto.cap` 从 0.20 改成 0.99」⇒ 报「上限表被改过」；改代码改一半会被单测抓 |
| **触发由证据推**（`evidence_vetoes`）：`sampled` / `matched` / `data_evidence` / `type_compatible` / `name_match` 决定该触发哪条 veto | `:638` | **整段删掉 `veto`** ⇒ 仍然被「按证据推出的 0.20」压住 |
| **命中率不是自由变量**：`sample_hit_rate.value` 必须等于 `sampled/matched` 的查表值 | `:755` | 「把 0/5 的 `value` 改成 1.0」 |
| **未知 `method` / 未知信号名一律拒** | `:720` / `:735` | 「`method` 改成 `foo`」⇒ 权重表无从核对 ⇒ 第 2 条被架空 |

> 为什么第 1 条要用「证据推出的上限」而不是「文件里声明的上限」：
> 若用后者，把 `veto.cap` 一起改大就同时骗过两条校验 —— 那只是把漏洞挪了个位置。
> 现在 `score` 的上限来自 `sampled` / `matched` 这些**必须与实际采样一致**的聚合量，
> 文件的 `veto` 段只负责「解释」，不负责「定上限」。

#### 6 条自证的实测（命令 / exit code / 文案）

产物在 `.review-workspace/req108/dev2/`（`tamper-*.json` = QA run1 的
`proposal.json` + 追加一条 E5，**只改一处**，所以失败可归因到那一处）。

| # | 自证 | 命令 | exit | 实测文案（首行） |
| --- | --- | --- | --- | --- |
| 1 | 手改 `score` 0.20→**0.99**，`veto: zero_hit` 原样保留 | `ontok propose import --file dev2\tamper-score.json` | **1** | `✗ item 149 的 confidence 不合法：score 0.99 超过 veto zero_hit 的上限 0.20 —— 分数与证据不自洽（signals 里 sample_hit_rate 是 sampled=5 / matched=0）` |
| 2 | 手改 `signals[].weight`：hit 0.45→**0.9** | `… --file dev2\tamper-weight.json` | **1** | `✗ item 149 的 confidence 不合法：signals 权重与 evidence-scorecard-v1 的权重表不一致：sample_hit_rate 的 weight = 0.9，权重表是 0.45 —— 分数不可复核` |
| 3 | 删掉 `zero_hit` 的**证据信号**（`sample_hit_rate`），`veto: zero_hit` 保留 | `… --file dev2\tamper-evidence.json` | **1** | `✗ item 149 的 confidence 不合法：声明了 veto zero_hit，但 signals 里没有它 —— 没有对应证据（veto 不能只写在声明里）` |
| 4 | 原样 import `proposal-rejected.json`（10 条已知错误，`importable: false`） | `ontok propose import --file qa-out\run1\proposal-rejected.json` | **1** | `✗ …\proposal-rejected.json 顶层声明 importable = false —— 这是被拒绝档，不能导入。` |
| 5 | 正常 `proposal.json`（148 条：96 实体 + 52 关系） | `ontok propose import --file qa-out\run1\proposal.json` | **0** | `提案 p1 已导入，共 148 个条目` |
| 6 | 正常 `proposal-pending.json`（20 条低档，重生成后 `importable: true`） | `ontok propose import --file dev2\regen\proposal-pending.json` | **0** | `提案 p1 已导入，共 20 个条目` |

**额外的 4 种攻击（都不在作业单里，但顺手关掉）**：

| 攻击 | exit | 文案 |
| --- | --- | --- |
| 整段**删掉** `veto`，score 仍 0.99 | 1 | `score 0.99 超过 veto zero_hit 的上限 0.20 —— 分数与证据不自洽（… sampled=5 / matched=0）` |
| `veto.cap` 0.20 → **0.99** | 1 | `confidence.veto.cap = 0.99 与 evidence-scorecard-v1 的记录不符（zero_hit 应为 0.20）—— 上限表被改过` |
| `sample_hit_rate.value` 0.0 → **1.0**（不动 `sampled`/`matched`） | 1 | `signals 自相矛盾：sample_hit_rate 的 value = 1 与 5/0 的查表值 0 对不上` |
| **绕过 import，直接改库里的 `payload.confidence`** 为 0.99/high，再 `review decide --accept-all`（exit 0）→ `proposal apply` | **1** | `应用失败：关系条目的 confidence 不合法：score 0.99 超过 veto zero_hit 的上限 0.20 —— 分数与证据不自洽（…）` |

> 最后一条是**第二道判据的实测**（不是「同一个函数再跑一次」的说法）：
> 分数即使已经躺在 `proposal_items.payload` 里，`apply` 也会在写快照前把它打回，
> **不会**出现 QA 实测的 `link_types.confidence = 0.99`。

**失败的 6 次 import 没有建出任何提案**（`qa108run` 里只有「自证 5 成功导入的 148 条 ×2」+
「自证 6 成功导入的 20 条 ×2」+ QA 那轮 applied 的 148 条），
所以「拒绝」发生在**写库之前**，不是「写进去再回滚」。

> ⚠️ **本地测试库 `ontok_demo` 的副作用（自证留下的，不是产品行为）**：
> `qa108run` 新增 4 份提案（148 ×2、20 ×2）；其中 **20 条那份的 `seq=1`** 与
> **148 条那份的 `seq=97`** 的 `payload.confidence` 被手工改成 `0.99 / high`
> （专门用来验证 `apply` 那道闸）。这两份**都没有写进任何快照**（apply 都失败了）。
> QA 复跑请用新的 project 名（如 `qa108run2`），不要复用这两份。

#### 不许误伤：真产出的 6 种形状全部放行

单测 `ac014_every_honest_score_passes_the_gate`（`confidence.rs`）把
**高 / 中（`partial_hit`）/ 拒绝（`zero_hit`）/ 拒绝（`all_null` + 泛化名）/
低（`--no-sample`）/ 结构 veto（多态）** 六种真实形状 `score()` 出来再
`serde_json` → `from_untrusted`，逐字段读回相同。
⇒ 前面 6 条「拒绝」不是靠「一律拒」换来的。

### 10.2 AC-015 · `importable: false` 现在是硬闸

| 项 | 实现 |
| --- | --- |
| 闸的位置 | `src/cli.rs:1220` `ensure_importable()`；`propose_cmd` 在读文件后**第一步**调用（`:1249`） |
| 语义 | 缺省 ⇒ 放行（REQ-108 之前的产物没有这个字段）；**恰好 `true`** ⇒ 放行；其它（`false` / `"false"` / `0` / `null`）⇒ 拒绝（fail-closed：说不清能不能导的文件不猜成能导） |
| 文案 | `… 顶层声明 importable = false —— 这是被拒绝档，不能导入。` + 一句「可导入的是 proposal.json / proposal-pending.json」 |
| 单测 | `tests/test_req108_confidence.rs` `test_req108_ac015_importable_false_is_a_hard_gate`（含 3 种非法取值 + 2 种放行） |

**产出侧也修了**（`src/bin/ontok-propose.rs:590`）：`importable` 必须与
「这份文件到底能不能导」**同真同假** ——

| 文件 | `importable` | 理由 |
| --- | --- | --- |
| `proposal-rejected.json`（10 条 <0.30） | **false** | 拒绝档连提案都不是；一次 `--accept-all` 就进本体 |
| `proposal-pending.json`（20 条 0.30–0.60） | **true** | 待查清单**就是要进审核界面**才能被人工判断（设计概要 §五：折叠区标「证据不足」）；不进主提案 ≠ 不能导入 |

> 原来 `side_file` 两栏都写 `false`，是**同一个 bug 的两面**：
> 它既让「带 false 的文件导得进去」（声明不生效），又让待查清单**永远导不进来**
> （低档因此没有人工审核入口）。本轮把语义取正，闸只按 `false` 拦。

### 10.3 `proposal-pending.json`：**允许导入**（决定 + 理由）

1. **AC 只点名被拒绝档**：A-8 原文是「`ontok-propose` 产出的 `proposal-rejected.json`
   带 `importable: false`……遇到 `importable: false` 的文件必须 exit≠0」。而
   `side_file` 当初把 pending 也写了 `false` —— 扫描这段代码可以看到它是一次复制。
2. **待查档的用途就是「被看」**：设计概要 §五 把低档的去向写成「待查清单（不进提案正文）」、
   审核界面「折叠区，标注『证据不足』」。不进主提案 ≠ 不能进审核系统。
3. **它不会让错误关系悄悄进本体**：低档条目带 `band=low` + veto 理由，`proposal.json` 里
   0 条；要进本体必须有人显式 `review decide --accept-all` + `apply`（人工动作），
   而 AC-009 判的是「拒绝档在 proposal.json 里一条都没有」——本轮实测仍然成立。
4. 若 BA 认为待查档也不该导入：把 `ontok-propose.rs:604` 的 `true` 改回 `false` 即可，
   **闸不用动**（一行）；代价是「低档人工审核」这条路没有入口了。

⚠️ **产物字节变化**（QA 复跑时请以此为准）：重生成的

- `proposal.json`：与 QA run1 **逐字节相同**（`1CDE0682…19A923`，qa `qa-review.md` §4.1 的同一个哈希）；
- `stats.json`：逐字节相同（`F1E67355…D06560`）；
- `proposal-pending.json`：`items` **逐字节相同**（20 条），只有 `importable`（false→true）与 `note` 变了；
- `proposal-rejected.json`：`items` 逐字节相同（10 条），只有 `note` 变了（`importable` 仍是 false）。

⇒ 评分与路由**一个字节没动**，变的只有「这个文件能不能导」这个声明本身。

### 10.4 第 2 轮发现的新绕过形态（如实登记，本轮**不修**）

| # | 形态 | 现在会怎样 | 为什么本轮不修 / 建议 |
| --- | --- | --- | --- |
| R1 | **`signals` 整段缺失**：`{"score":0.99,"method":"evidence-scorecard-v1"}` | **import 成功** | 既有判据 `absent_optional_fields_get_derived_not_invented_values` 明确要求这种形状放行（REQ-108 之前的人工提案没有 `signals`）。本轮判据止于「写了 `signals` 就必须自洽」。**建议下一轮**：`add_link_type` 带 `confidence` 时 `signals` 必填，或在 `apply` 时把「无 `signals` 的高分」按 0.45 封顶 |
| R2 | **自洽的假 `signals`**：把 `sampled`/`matched` 一起改成 5/5，再算 0.99 | **import 成功，且算得上自洽** | 这是「证据本身可信不可信」的问题，不是「分数与证据一致不一致」。QA 建议的「按权重重算 `score` 必须相等」能拦住「值算了但没对上」，拦不住「聚合量本身被改」。真正的解法是拿 `sample_digest` 回源库复采（成本高，且要连源库），属下一轮 |
| R3 | **结构性 veto**（`composite_key_unsupported` / `polymorphic_target` / `self_ref_non_unique`）没有对应信号 | 只核到「上限与表一致 + 分数不越上限」 | 它们的证据在**候选关系**（复合键 / 多态列 / 自引用）里，不在 `confidence.signals` 里；而它们只会**压低**分数（0.40–0.50），抬不高，不构成「伪造高分」的通路。已在 `confidence.rs:114` 用 `STRUCTURAL_VETOES` 写明并注释理由 |
| R4 | 「按权重重算 `score` 必须逐位相等」（QA §3.2 的建议 3） | **未实现** | 超出 A-7 的三条；加它会改变「外部手工提案」的准入面。当前权重 + veto 双向锁死后，单改 `score` 已经不可能过闸 |

### 10.5 不退化（第 2 轮实测，最终代码）

| 目标 | 结果 |
| --- | --- |
| `cargo test --lib` | **226 passed / 0 failed**（第 1 轮 217 + 新增 9 条 AC-014 单测） |
| `cargo test --doc` | **4 passed / 0 failed** |
| 31 个套件（`--test`，绕行挪 bin） | **300 passed / 0 failed**（13 条 REQ-107 出口判据 = **70**；16 个邻域套件 = **218**；`test_req108_live` = 5；`test_req108_confidence` = 7） |
| `tests/drop_rate.rs` / `test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` / `src/mcp/tools.rs` / `property_filter.rs` | `git diff` **仍为空**（本轮零触碰） |
| `Cargo.toml` / `Cargo.lock` | **零改动**（无新依赖） |
| 删测试 / `#[ignore]` / 放宽断言 | 无（本轮的 diff 只有**新增**判据） |
| bin 绕行 sha256 | `C7CF95E2…322D → C7CF95E2…322D`（identical = True） |

### 10.6 报告订正（A-6 / N4）

| 项 | 上一版写法 | 订正为 |
| --- | --- | --- |
| N4-① | §5.1「42 项逐字段相同」 | **36 个字段**（9 条 × `sampled`/`matched`/`digest`/`reason`）；42 是 §5.2 的**样本值个数** |
| N4-② | §3.1「审核界面默认折叠 / 逐条显示」 | CLI **没有交互折叠**：`review show --confidence` 是「四档计数行 + 按档排序逐条打印」（44 条高档也逐条打印）。「折叠」是设计概要 §五的界面意图，不是本实现行为 |
| A-6 | §0 与 §3.2 写「9/9 全落拒绝」 | 拆成两种证据：**E1–E7 真产出且落拒绝**（`zero_hit` 0.20）；**E8/E9 不产出 + 按坐标构造复算 0.15**（`type_incompatible`）。⛔ 不再混写 |
