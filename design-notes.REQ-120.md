# REQ-120 · 关系推断改进（FK 边补全）— 设计说明与实测

> 一句话：**`*_id` 列指向哪张表，由「这一列的值有没有落在候选表主键里」决定。**
> 名字只负责**提名**，值域负责**判决**；判决结果先出提案，人签过才进本体。

---

## 一 · 三段落点（`src/extract/fk.rs`，新文件）

| 段 | 做什么 | 关键函数 |
| --- | --- | --- |
| ① 提名 | 每个 `*_id` 列生成 1–8 个候选表（词干即表名 / 复数 / 尾缀 / 词元 / 注释提到） | `FkCatalog::candidates`（提名**唯一**实现） |
| ② 判决 | 采样该列 → 去重命中数 ÷ 去重样本数 = 命中率 | `sample_column` / `count_hits` / `decide_column` |
| ③ 提案 | ADR-005 六字段 + 机器可读 `facts` | `FkReport` / `render_report` / `render_unresolved` |
| ③·五 继承 | 同名同义列继承（E-2，五条件） | `VerifiedIndex` / `plan_inherit` |
| 签字 | 只认 `reviewed_by` 非空且**重新过阈值**的那几条 | `load_signed`（五道闸） |
| 落库 | 注入 `add_link_type` 条目（已有边只补标记）；签字边不被分档丢弃 | `inject_signed` / `stamp_naming_only` / `promote_unique_cardinality` / `retain_signed` |

### 为什么独立成文件

`src/extract/propose.rs` 里 130 处真实表名的**行号**被 `tests/retained_names.req084.tsv`
钉死（`include_str!` + 逐行核对），在 `propose.rs` 里增删一行就会整批平移变红。
REQ-087 的 `cardinality.rs` 是同一条理由。

---

## 一·五 · 三态（BA rev.2 的 E-2）

| 值 | 含义 | 谁写 |
| --- | --- | --- |
| `"true"` | **值域重叠实测通过** | `--fk-sign`（`band = strong/medium`） |
| `"inherit"` | **同名同义列继承**：本表没实测，但另一张表的同名列验过、且目标唯一 | `--fk-sign`（`band = inherit`） |
| `"false"` | **仅名字推断**，无任何数据证据 | 通用盖章 |

⛔ `inherit` **绝不**写成 `"true"`；`None`（REQ-120 之前）**不是** `"false"`。

## 一·六 · E-1：join 目标不只主键

候选 = **（表，目标列）** 对，目标列 ∈ `{primary_key} ∪ unique_columns`（去重、主键优先）。

```
llm_model.model_id  与  llm_model.id（uuid）0 命中
                    与  llm_model.model_id（unique 业务键）5/5 命中  ⇒ strong
```

证据里写明档次：`-> llm_model.model_id via unique_column (substring)`。

## 一·七 · E-2：同名同义列继承（六条件）

1. 同名列在**别处**实测通过（`strong/medium`，**不叠继承链**）；
2. 这些验证的**目标唯一**（表 + 列都一致），否则 `inherit_conflicts += 1` 并拒绝；
3. 目标表**在本列候选里**（名字证据与数据证据都指向它）；
4. `confidence = 0.60 + 0.15 × 来源命中率`，**夹在 [0.60, 0.75]**（永远够不着 strong）；
5. **必须人签**（`load_signed` 复查：缺 `facts.inherit` / 置信度越界 / 证据不以
   `inherited from ` 开头 ⇒ **拒绝整份文件**）；
6. **⛔ 租户键列一律不许继承**（F-1，替身复跑抓到的漏洞，见 §一·八）。

## 一·九 · F-4：证据强度写进 TTL 文件头

`ontok:joinVerified "true"` 的取证方式是「每列取 ≤5 个不同值去撞目标键」——
**抽样**，不是全量统计。数字员工看到 `true` 很容易当成「全表成立」，
所以这句话现在**直接写在建模视图的文件头**（客户视图没有三态，不加）：

```
# · ontok:joinVerified 是**抽样证据**（每列取 ≤5 个不同值比对目标键），不是全量统计保证：
#   "true" = 抽样命中率 ≥95%（常见 5/5），"inherit" = 同为名列在别处的验证、本表未实测，
#   "false" = 仅列名推断（joinEvidence 写明是哪一种没验）；两种方向（正向/反向属性）各标一条
```

## 一·八 · F-1 / F-2（替身复跑后的两处裁定）

### F-1 · `inherit` 不得用于租户键列

替身原话：`deductions` 同表并存 `customer_id` 与 `main_customer_id`，**两个都标
tenantKey true** ⇒ 这套库里「叫 `customer_id`」**不能唯一确定**主账号/子账号/付款方；
别处的同名列只证明「这个名字在某张表里指向 `customers.id`」，**没证明本表这一列
指向同一实体**。租户边界关系到安全 ⇒ **必须本表实测**。

落地：`ontok-propose --tenancy <tenancy.json>` 读同一份声明（与
`ontology export --tenancy` 同一格式），`plan_inherit` 首闸即判 `tenant_columns`。
没给 `--tenancy` ⇒ 明确警告「继承闸对所有人放行」（不静默）。

### F-2 · `false` 边必须写明「为什么没验」

替身判词：唯一入口边 `deductions__customers` = `false` 且**零 joinEvidence** ——
数字员工看不出是「**数据不够**」还是「**只看了名字**」。这两种可信度完全不同。

落地：`stamp_naming_only(items, reasons)` 给每条 `false` 边写 `ontok:joinEvidence`，
理由表来自**签字产物**（= 上一次扫描的现场：未签提案 + 未解析清单），
**正向与反向都写**：

| 情形 | 证据前缀 |
| --- | --- |
| 该列在未解析清单 | `insufficient_sample:` / `low_overlap:` / `no_candidate:` / `sample_failed: 后接实测数字` |
| 该列有提案但没签字 | `unsigned: 有 <band> 提案（confidence …）但**未签字** ⇒ 未进快照` |
| 不在 FK 抽样域（`_no`/`_code`/主键） | `naming-only: 该列未进入值域重叠抽样 …` |

只对 `insufficient_sample` / `sample_failed` 做：`low_overlap` 是**数据说了不成立**，
`no_candidate` 连名字证据都没有 —— 两者都不许继承。

**已签字的边不参与按置信度丢弃**（`retain_signed`）：否则 0 行表的继承边
会被分档器静默丢进待查，签了字也进不了本体。

## 二 · 判据（R1–R6 的落点）

| 判据 | 取值 | 为什么 |
| --- | --- | --- |
| 强证据 | 命中率 ≥ 0.95 ⇒ `confidence ≥ 0.9`，`band = strong` | R2 |
| 中等 | 0.5 ≤ 命中率 < 0.95 ⇒ `band = medium`（**必须人签**） | R2 |
| 否决 | < 0.5 ⇒ **一条关系都不产出**，进「被否决清单」+「未解析清单」 | AC-002 / AC-008 |
| 候选表无键列 | 跳过，`candidate_no_pk` | R3 |
| 样本不足 | 窗口 < 3 行**或**去重值 < 3 ⇒ `insufficient_sample` | AC-002 |
| 1:1 / N:1 | 25 行窗口内去重后无重复 **且** 源列在 `unique_columns` 里 ⇒ 1:1；否则 N:1 | R4（宁少勿多） |
| 每列采样 | 扫 25 行（`LIMIT 25`，`ORDER BY 源表主键`），取前 5 个**不同**值去验命中 | R6 |

### 两个量纲不能混（本轮实测踩到并修了）

用「命中数 / 取了几行」算命中率是**错的**：
`deductions` 只有 4 行，`license_id` 4 行全是同一个 UUID，实测这 4 个都**真的**
在 `licenses.id` 里，但按行数算只有 `1/4 = 250‰` ⇒ 会被误否决。

所以分成两个窗口：

| 量 | 窗口 | 用途 |
| --- | --- | --- |
| `window_rows` / `window_distinct` | 25 行 | R4 唯一性判据 |
| `sampled`（= 前 5 个不同值） | 5 个值 | **命中率的分母** |

只扫 5 行也不行：`usage_details.license_id` 按主键序前 5 行同一个授权
（去重 1 个值），扩到 25 行是 4 个值 —— 只扫 5 行会让它永远验不了，
而它恰好是 AC-003 点名要与 `deductions` 同口径的那一列。

---

## 三 · 真实库实测（`anspire_open_uat` · 96 表 / 1111 列 · 只读会话）

```
待判列      : 144 个 *_id（候选 673 个 · 实测计数 427 次）
补出关系    : 91 条（strong 72 · medium 1 · inherit 18，**未签，不进快照**）
值域否决    : 22 个候选（命中率 < 0.5）
未解析      : 53 列
  · no_candidate          19
  · low_overlap           10
  · candidate_no_pk        0
  · insufficient_sample   24   ← F-1 把 16 个租户键列从 inherit 退回这里
  · sample_failed          0
F-1 拒绝继承 : 16 列（租户键：customer_id / main_customer_id / sub_customer_id / …）
继承冲突    : 0 列（没有出现「同名不同义」）
```

签字（本轮 e2e 的**代签**，真实流程必须是人）：**72 条 strong + 1 条
`api_call_logs.license_id` 的 inherit**；未签 18 条（1 medium + 17 inherit）
⇒ 验证「未签不进快照」。`api_call_logs.customer_id` **不再产出 inherit**（F-1）。

注入：**22 条新增 + 51 条既有边补验证标记**，未签 18 条被忽略；
`retain_signed` 把 **5 条**会掉进待查/拒绝的签字边捞回主提案。

TTL（`anspire_fk120_v7` v2，建模视图）：**79 条关系 × 2 = 158 块对象属性**；
`joinVerified "true"` **144**（=72×2）、`"inherit"` **2**（=1×2）、`"false"` **12**（=6×2）；
`joinEvidence` **158**（**每一条边都有**：true 144 + inherit 2 + false 12 —— F-2 的结果）。
重复主语 0 个。

### 12 块 `false` 边的理由分布（F-2 的交付口径）

| 块数 | 边数 | 理由前缀 | 说明 |
| --- | --- | --- | --- |
| 6 | 3 | `insufficient_sample:` | `customer_tpm_rate_limit`（1 行）· `deductions`（4 行）· `frozen_assets`（窗口 25 行仅 1 个不同值） |
| 6 | 3 | `unsigned:` | `deductions→licenses` / `order→customer_subscriptions`（inherit 0.750 未签）· `subscription_priority→customer_subscriptions`（medium 0.767 未签） |
| **0** | **0** | `naming-only:` | 本库里**没有**「既没进抽样、又没提案」的 false 边 |

### 与「只跑抽取、不签字」同口径对比（96 类、同一条流水线）

| 产物 | 52 条关系（未签） | 79 条关系（签了 73） | 增量 |
| --- | --- | --- | --- |
| 建模视图 TTL | 379,417 B | 423,616 B | **+11.7%** |
| 客户视图 TTL | 295,537 B | 312,889 B | **+5.9%** |
| 对象属性块 | 104（52×2） | 158（79×2） | +54 |

⚠️ 与 REQ-119 AC-008 的「215.5 KB ⇒ ≤ +40%」比时请用**同一口径**：那份 215.5 KB 是
84 类的客户视图；本表是 96 类。REQ-120 自己只加了 **5.9%**。
## 四 · 5 个孤儿类逐个结论

| 类 | 结论 | 依据 |
| --- | --- | --- |
| `api_call_logs` | ⚠️ **部分补上**：`license_id → licenses` 以 `inherit` 进快照；`customer_id` **撤销 inherit**（F-1）且**不进快照** | 表 0 行；`license_id` 非租户键 ⇒ 允许继承；`customer_id` 是 tenantKey ⇒ 必须本表实测 ⇒ 无从验证 |
| `package_products` | ✅ 补上（8 条对象属性） | 4 处 `product_id → package_products.id` |
| `service_config` | ❌ **仍未解析**（0 条边） | 三处 `service_id` 全部 `insufficient_sample`，同名列**没有一处实测通过** ⇒ 继承条件 1 不成立 |
| `llm_model` | ✅ 补上（E-1 修好，8 条对象属性） | 4 条 strong 指向 `llm_model.model_id`（unique 业务键）+ 5 条 inherit |
| `customer_account_binding` | ✅ 补上（6 条） | `main_customer_id` / `sub_customer_id` → `customers.id` |

### `api_call_logs` 两条边的最终状态（F-1 的交付）

| 边 | `joinVerified` | 为什么 |
| --- | --- | --- |
| `api_call_logs__licenses`（`license_id`） | **`"inherit"`** | 非租户键；同名 `license_id` 在 `license_quota`(5/5)/`usage_details`(4/4) 都实测指向 `licenses.id`，目标唯一；人签后进快照 |
| `api_call_logs__customers`（`customer_id`） | **不进快照**（TTL 里没有这条边） | F-1：`customer_id` 是 tenantKey。**选择"不进快照"而不是"标 false"**，理由见下 |

**为什么选「不进快照」而不是「标 false」**：

1. 它**没有**任何数据证据（0 行表），REQ-108 的分档本来就会把它放进「待查清单」——
   强行保留等于绕过那道分诊；
2. 租户键关系到安全：一条 `false` 的租户键边仍然会出现在 SPARQL 的候选里，
   数字员工很可能照着 join 出去（只是没标"已验证"）——**缺一条边比给一条
   没有依据的跨租户边更安全**；
3. 它在 `proposal-pending.json` 里**仍然可见**（带 `join_verified: "false"`），
   人若要认，走「待查 → 人工接受」的正规路径，而不是被本需求的继承规则顺带放行。

### `api_call_logs.license_id` 的最终 TTL（继承，人签）

```turtle
:api_call_logs__licenses a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :api_call_logs ; rdfs:range :licenses ;
    ontok:cardinality "N:1" ;
    ontok:joinVerified "inherit" ;
    ontok:joinEvidence "inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)" ;
    owl:inverseOf :licenses__api_call_logs .
```

### `llm_model` 的结论（E-1）

```
llm_provider.model_id -> llm_model.model_id  strong 5/5
llm_model_sort.model_id -> llm_model.model_id  strong 5/5
llm_model_peak_config.model_id -> llm_model.model_id  strong 5/5（1:1）
llm_provider_groups.model_id -> llm_model.model_id  strong 5/5
llm_provider_back*.model_id -> llm_model.model_id  inherit（表 0 行）
```

### `no_candidate 19` 抽查 2 条（真没候选）

| 列 | 为什么真的没候选 |
| --- | --- |
| `deductions.request_id` / `usage_details.request_id` | 词干 `request`：96 表里没有任何表名等于/以 `_request(_s)` 结尾/含 `request` 词元；注释为空 |
| `llm_auth_cache_change_outbox.run_id` / `source_id` / `target_id` | 词干 `run`/`source`/`target`：同样 0 命中（outbox 内部事件坐标，不指向业务表） |

**抽查暴露并修掉一个真 bug**：`customer_provider_permissions.policy_id` 原判
`no_candidate`，而库里有 `customer_provider_permission_policies` —— 复数规则只认
`+s`/`+es`，不认识 `policy → policies`。补上「辅音 + y → ies」后变成 **strong**。

### F-2 的原文样例（`deductions__customers`）

```turtle
:deductions__customers a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :deductions ; rdfs:range :customers ;
    ontok:cardinality "N:1" ;
    ontok:joinVerified "false" ;
    ontok:joinEvidence "insufficient_sample: 有效样本 4 行 / 1 个不同值 < 3 条下限（跳过 NULL 之后）" ;
    owl:inverseOf :customers__deductions .
```

⚠️ 口径说明：`有效样本 N 行` 是**取样窗口**（≤25 行），不是表的总行数。
`deductions` 恰好 4 行（窗口=全表），`frozen_assets` 是「窗口 25 行里只有 1 个不同值」。
## 五 · 1:1 修正（REQ-119 QA 的 N-1）

REQ-119 QA 点名 6 条「其实该是 1:1」的关系，现在**全部 1:1**
（`owl:FunctionalProperty` + `owl:InverseFunctionalProperty` 双标）：

| 关系 | REQ-119 | 现在 |
| --- | --- | --- |
| `balances.customer_id → customers` | N:1 / Functional | **1:1** |
| `customer_rate_limit.customer_id → customers` | N:1 | **1:1** |
| `customer_tpm_rate_limit.customer_id → customers` | N:1 | **1:1** |
| `dim_customer_binding.customer_id → customers` | N:1 | **1:1** |
| `license_quota.license_id → licenses` | N:1 | **1:1** |
| `service_apps.channel_id → marketing_channel_customer` | N:1 | **1:1** |

**另有 2 条**进 TTL 的 1:1（TLL 里一共 **8 条** 1:1 边，16 块 = 8×2）：

| 关系 | 为什么 1:1 | 来源 |
| --- | --- | --- |
| `sub_account_permission.sub_customer_id → customers` | 源列在 `unique_columns` 里（`unique_columns` 判据，非白名单） | 抽取侧 promotion |
| `llm_model_peak_config.model_id → llm_model` | 目标列 `llm_model.model_id` 是**唯一列**，且源列也是唯一列 | E-1 的 unique 目标 |

⚠️ 口径纠正（QA 复核）：早先这里写的是 `internal_staff.customer_id` —— 它在**流水线里**确实被
提升成 1:1，但那条边是低档且未签字，**没有进快照**，所以 TTL 里看不到它。
「有没有提升」与「TTL 里有没有」是两件事，本表说的是后者。

方向说明（**宁少勿多**）：把 1:1 说成 N:1 只少一条约束；把 N:1 说成 1:1 会让开推理
的一方错误合并个体（REQ-119 B-1 的教训）。所以 1:1 需要**两个证据同时成立**：
窗口内去重无重复 **且** 源列在数据库的唯一约束里。

---

## 六 · TTL 证据（AC-007 / 三态）

```turtle
# ① 实测通过
:admin_user_roles__admin_users a owl:FunctionalProperty, owl:ObjectProperty ;
    ontok:cardinality "N:1" ;
    ontok:joinVerified "true" ;
    ontok:joinEvidence "value-overlap 5/5 (100.00%) -> admin_users.id via primary_key (plural)" .

# ② 唯一列当 join 目标（E-1）
:llm_provider__llm_model a owl:FunctionalProperty, owl:ObjectProperty ;
    ontok:joinVerified "true" ;
    ontok:joinEvidence "value-overlap 5/5 (100.00%) -> llm_model.model_id via unique_column (substring)" .

# ③ 继承（E-2）—— 0 行表的唯一出路
:api_call_logs__licenses a owl:FunctionalProperty, owl:ObjectProperty ;
    ontok:joinVerified "inherit" ;
    ontok:joinEvidence "inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)" .

# ④ 仅名字推断 —— **必须带「为什么没验」**（F-2）
:customer_tpm_rate_limit__customers a owl:FunctionalProperty, owl:InverseFunctionalProperty, owl:ObjectProperty ;
    ontok:cardinality "1:1" ;
    ontok:joinVerified "false" ;
    ontok:joinEvidence "insufficient_sample: 有效样本 1 行 / 1 个不同值 < 3 条下限（跳过 NULL 之后）" .
```

四条口径：
1. `"true"` 只给**签过字且过阈值**的边；`"inherit"` 只给**继承且签过字**的边；
2. 名字推断一律 `"false"`（可区分，不是省略）；`None`（REQ-120 之前）**一行都不写**；
3. `joinEvidence` **每条边都写**（F-2）：`true`/`inherit` 写验证证据，`false` 写「**为什么没验**」——
4. 正反**两条都标**，所以 `joinVerified` 条数 = **2 × 签过的边数**（144 = 72×2 等）。

⚠️ TTL 证据**只放数字与目标列**，不放示例值（TTL 是发给数字员工的产物，
示例值属于客户数据；REQ-108 AC-003「不落原值」在这条路径上继续有效）。
## 七 · 确定性 / 性能（AC-009）

| 项 | 结果 |
| --- | --- |
| 两次同一命令的 `fk-proposals.json` | sha256 `7116161b1d3b6d66d7970ab20d…` 两次**逐字节相同** |
| 两次同一命令的 `fk-unresolved.json` | sha256 `943939f8bb762831e2ea0b27…` 两次**逐字节相同** |
| FK 阶段（144 列 / 673 候选 / 427 次计数查询） | **42.5 s** / 41.9 s |
| 全库一次完整抽取（含 REQ-108 采样 + 打分 + 分档） | **103.8 s**（rev.1）；rev.2 的签字轮 **16.1 s** |

建模视图 TTL 两次导出 sha256 相同（同一冻结版本重导两次）。

确定性靠三件事：候选按（提名档, 表名）排序；样本 `ORDER BY` 源主键 + 固定 `LIMIT`；
提案/清单渲染前 `sort()`（`FkReport::sort`）。⛔ 全程没有 `random()` / `TABLESAMPLE` / 时间戳
（`FkSource` 刻意**不记** `introspected_at`，那会让「两次运行相同」当场失效）。

---

## 八 · AC-011 的复跑入口

```
D:\MyCodes\AI\an_onto_demo\feature-REQ-120\out\anspire-req120-model.ttl        ← 补全后（96 类 / 80 关系）
D:\MyCodes\AI\an_onto_demo\feature-REQ-120\out\anspire-req120-unsigned-model.ttl ← 同口径未签对照（52 关系）
```

复现命令（本地库 `anspire_real`，项目 `anspire_fk120_v7` **已冻结在 v2**）：

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
.\target\debug\ontok.exe ontology export --project anspire_fk120_v7 --version 2 `
  --model-view --format ttl --out out\anspire-req120-model.ttl `
  --tenancy tenancy.json --schema D:\MyCodes\AI\an_onto_demo\code\out\schema.json
```

### 对 Q3 / Q4 / Q5 的预判（**不代替替身实验**）

| 题 | 判读 | 依据 |
| --- | --- | --- |
| Q3 额度说出产品名 | ✅ 能写 | `usage_details.product_id → package_products`（true）+ `product_name` 属性 ⇒ 两跳 |
| Q4 用过的 API | ✅ **能写了** | `:api_call_logs__licenses` / `:licenses__api_call_logs`（`joinVerified "inherit"`）；客户→授权那一跳用 `:licenses__customers`（`"true"` 实测 5/5），**不经过**被撤销的 `api_call_logs.customer_id` |
| Q5 套餐下订阅 | ✅ 关系可写 | `customer_subscriptions.template_id → package_templates`（true）+ 反向；跨租户仍未裁定 |

⚠️ Q2（余额）仍然不行 —— 那是业务口径问题，与关系无关（REQ-119 的结论不变）。
## 九 · 偏离与已知限制（如实登记）

1. **提案里带示例值**（AC-001 要求），但**只截断 16 字符、最多 3 个**，且**不进 TTL**。
   REQ-108 AC-003 的「不落原值」针对的是 `SampleEvidence`（类型上就没有原值字段）；
   REQ-120 的提案是给人判真假的，没有示例值无法判。**TTL 侧零示例值。**
2. **`ontok:joinVerified` 用字符串**（`"true"` / `"inherit"` / `"false"`），不是裸布尔；
   与 REQ-119 的 `ontok:tenantKey true` 不统一是**有意的**（不借「统一风格」改别人的验收）。
3. **`link_types.join_verified` 是 TEXT**（三态）。rev.1 落的是 BOOLEAN，所以加了一条
   **无条件但通常空转**的 `DO` 块（[`JOIN_STATE_PATCH`]）：稳态库只读一次
   `information_schema`、一把锁都不取；只有还是 `boolean` 的库才做一次类型转换。
   ⚠️ 类型转换有损（`boolean → text` 只可能得 `true/false`）—— 历史上没有 `inherit` 数据。
4. **`apply.rs` 原来把关系侧 `evidence` 写死成 `vec![]`** ⇒ 「这条关系由哪一列实现」
   从没进过快照。不修它，同一对表的两个 FK 在 TTL 里会写成**同一个 IRI**
   （`customer_account_binding.main_customer_id` / `sub_customer_id` 都是 `customers`）。
   修法：`evidence` 从 payload 映射；TTL 侧**只在真有歧义时**给本地名加列名。
5. **继承只对 `insufficient_sample` / `sample_failed`**：`low_overlap` 是数据说了不成立、
   `no_candidate` 连名字证据都没有 —— 两者都不继承。（偏保守，已登记。）
6. **继承必须人签**，且 `confidence ≤ 0.75`：它永远进不了 strong 档。
   本轮 **18 条 inherit 只签了 1 条**（`api_call_logs.license_id`），其余 **17 条**留在提案里没进快照。
7. **`llm_model.model_id` 自己仍是 `low_overlap`**：它定义业务键，不引用别人。
8. **候选靠名字提名** ⇒ 「名字与目标表毫无词汇关系、只有值重叠」的边仍提不出（天花板）。
9. **复数只认三条规则**（`+s` / `+es` / `辅音+y→ies`），不做不规则复数（`people`）。
10. **F-1 的代价**：16 个租户键列退回 `insufficient_sample`（`inherit` 34 → 18）。
    这是**刻意**的：宁可少 16 条边，也不给数字员工一条没有本表实测的跨租户边。
11. **`--tenancy` 只影响 FK 阶段的继承闸**；不给它时 `--fk-out` 照跑，但会 stderr
    警告「继承闸对所有人放行」。`ontology export --tenancy` 是**另一件**事（标 tenantKey）。
12. **F-2 的 `insufficient_sample` 里那个行数是取样窗口（≤25），不是表行数** ——
    报告与 TTL 都按窗口口径写，别读成「这张表只有 4 行」（`deductions` 恰好两者相同）。
### G-3 · 登记（不改）

- **F-2 的前缀集合是超集**：代码里可能出现 `low_overlap:` / `no_candidate:` / `sample_failed:`，
  而本产物只用到 `insufficient_sample:`（6 块）、`unsigned:`（6 块）、`naming_only` 兜底（0 块）。
  超集是有意的：换一批数据时理由仍然写得出来。
- **F-1 的 CLI 接线已有真进程判据**（`tests/test_req120_tenant_gate.rs` 3 条）：
  手工构造 tenant inherit ⇒ 进程 exit≠0、stderr 点名、**不写产物**；非租户列 inherit 照常通过。
  这补上了「`plan_inherit` 有单测、但 CLI 接线没测」的缺口。
- **QA 的 A/B 跑法值得保留**：V5（无 --tenancy）与 V6（有 --tenancy）两份 TTL 逐边比对，
  16 条差异**全部**落在租户列、**零误伤** —— 比「看计数对不对」强，因为它同时证明了
  「闸生效」与「闸没有多拦」。

## 十 · 测试

| 层 | 数字 |
| --- | --- |
| lib（`cargo test --lib`） | **414 passed**（基线 370 + REQ-120 的 44：`extract::fk` 43 + `domain::ttl` 1） |
| doc（`cargo test --doc`） | **4 passed**（基线 4） |
| 新增集成 | `tests/test_req120_fk.rs` **11** + `tests/test_req120_tenant_gate.rs` **3**（G-1 真进程） |
| 全量（`cargo test --no-fail-fast`，89 个 target） | **1334 passed / 5 failed** |

### 6 条失败逐条交代（**没有一条由 REQ-120 引入**，均为环境或既有）

| # | 用例 | 原因 | 归属 |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | 读不到脱敏 fixture `../.review-workspace/fix-props/schema.json` | 环境（该用例设计为 fail-closed） |
| 2 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 同上（缺 `ONTOK_SCHEMA_JSON` 指向的脱敏 schema） | 环境 |
| 3 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 无 `--src` 时 REQ-108 的分档把唯一那条关系送进「待查」，于是 CLI 的 `items` 比 `extract()` 少一条 | 既有（REQ-116 的 manifest 已登记：「REQ-108 路由与 REQ-015 老用例冲突 1」） |
| 4 | `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | `propose.rs:280:60` 登记的是 `customer_rebate`，HEAD 上那一行是文档句 | 既有（REQ-116 manifest：「develop 上就过期的登记表 2」）；`git show HEAD:src/extract/propose.rs` 即可复现，**本 REQ 未改 `propose.rs`** |
| 5 | `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | `src/domain/concept.rs` 实际 9 处、登记 5 处 | 既有（同上；本 REQ 未改 `concept.rs`） |
| 6 | ~~`t6_every_module_is_registered`~~ | **本 REQ 新增 `extract::fk`** ⇒ 已补登记，复跑 **10 passed** | ✅ 已修（不在最终 5 条里） |

修复动作只有一处：`tests/test_req107_source_enumeration.rs` 的模块登记表加一行
`("src/extract/mod.rs", "fk")`（该表的设计就是「新增模块即红」）。


### Windows 上的 preflight 绕行（⛔ 没有挪文件）

`src/bin/ontok-gate-preflight.rs` 用了 `std::os::unix::fs::MetadataExt` 与 `symlink`，
Windows 上**编不过**，而 `tests/test_retained_names_req084.rs` 用 `include_str!` 钉着它
的**行号**。所以**只做行数不变的 shim**：

| 行 | 改法 |
| --- | --- |
| 587–588 | `#[cfg(unix)] use …` / `#[cfg(unix)] if x.dev() …`（同一行加属性，不增行） |
| 1450 / 1500 / 1526 / 1919 | `#[cfg(unix)] #[test]`（4 条 POSIX 语义用例在 Windows 上不跑：`default_roles_are_two_different_files`、`resolve_roles_rejects_alias_of_baseline`（symlink）、`resolve_roles_rejects_hard_link_of_baseline`（st_ino）、`real_pair_five_missing_columns_are_named`（登记基线是 POSIX 绝对路径）） |

文件总行数 **2758 → 2758**（改前改后相同），`retained_names.req084.tsv` 的
行号/列号判据全部仍然有效。⚠️ 副作用：Windows 上那 **4 条**用例不执行（它们测的是
POSIX 的 `st_dev/st_ino` 语义，在这台机器上本来也无从验证）。
