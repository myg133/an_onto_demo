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

## 一·七 · E-2：同名同义列继承（五条件）

1. 同名列在**别处**实测通过（`strong/medium`，**不叠继承链**）；
2. 这些验证的**目标唯一**（表 + 列都一致），否则 `inherit_conflicts += 1` 并拒绝；
3. 目标表**在本列候选里**（名字证据与数据证据都指向它）；
4. `confidence = 0.60 + 0.15 × 来源命中率`，**夹在 [0.60, 0.75]**（永远够不着 strong）；
5. **必须人签**（`load_signed` 复查：缺 `facts.inherit` / 置信度越界 / 证据不以
   `inherited from ` 开头 ⇒ **拒绝整份文件**）。

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
补出关系    : 107 条（strong 72 · medium 1 · inherit 34，**未签，不进快照**）
值域否决    : 22 个候选（命中率 < 0.5）
未解析      : 37 列
  · no_candidate          19
  · low_overlap           10
  · candidate_no_pk        0
  · insufficient_sample    8
  · sample_failed          0
继承冲突    : 0 列（没有出现「同名不同义」）
```

签字（本轮 e2e 的**代签**，真实流程必须是人）：**72 条 strong + 2 条
`api_call_logs` 的 inherit**，其余 33 条（1 medium + 32 inherit）**故意不签**
⇒ 验证「未签不进快照」。

注入：**22 条新增 + 52 条既有边补验证标记**，未签 33 条被忽略；
`retain_signed` 把 **6 条**会掉进待查/拒绝的签字边捞回主提案。

TTL（`anspire_fk120_v3` v2，建模视图）：**80 条关系 × 2 = 160 块对象属性**；
`joinVerified "true"` **144**（=72×2）、`"inherit"` **4**（=2×2）、`"false"` **12**（=6×2）、
`joinEvidence` **74**（=72+2，只正向）。重复主语 0 个。

### 与「只跑抽取、不签字」同口径对比（96 类、同一条流水线）

| 产物 | 52 条关系（未签） | 80 条关系（签了 74） | 增量 |
| --- | --- | --- | --- |
| 建模视图 TTL | 379,417 B | 415,300 B | **+9.5%** |
| 客户视图 TTL | 295,537 B | 313,539 B | **+6.1%** |
| 对象属性块 | 104（52×2） | 160（80×2） | +56 |

⚠️ 与 REQ-119 AC-008 的「215.5 KB ⇒ ≤ +40%」比时请用**同一口径**：那份 215.5 KB 是
84 类的客户视图；本表是 96 类。REQ-120 自己只加了 **6.1%**。
## 四 · 5 个孤儿类逐个结论

| 类 | 结论 | 依据 |
| --- | --- | --- |
| `api_call_logs` | ✅ **补上（inherit）** | 表 **0 行** ⇒ 本表不可能实测；`license_id → licenses` 继承
`license_quota.license_id (verified 5/5)`、`customer_id → customers` 继承
`agent_white_list.customer_id`。人签后进快照 ⇒ Q4 可写 |
| `package_products` | ✅ 补上（8 条对象属性） | `package_benefits/usage_details/usage_quotas/usage_reconciliation_log.product_id
→ package_products.id` |
| `service_config` | ❌ **仍未解析**（0 条边） | 三处 `service_id`（`deductions` / `free_amounts` /
`service_free_amount_config`）**全部** `insufficient_sample`（本表没数据）⇒ 别名一侧
**没有任何一处实测通过** ⇒ E-2 的条件 1 不成立（继承不能凭空造证据）。这是「数据不够」，
不是判据不肯做 |
| `llm_model` | ✅ 补上（**E-1 修好的**） | `llm_provider.model_id` / `llm_model_sort.model_id` /
`llm_model_peak_config.model_id`（1:1）/ `llm_provider_groups.model_id`
→ **`llm_model.model_id`（unique 业务键）**，4 条 strong + 5 条 inherit |
| `customer_account_binding` | ✅ 补上（6 条） | `main_customer_id` / `sub_customer_id` → `customers.id` |

### `api_call_logs.license_id` 的三段结论（rev.2）

1. **实测不可得**：源库 `select count(*) from api_call_logs` = **0** ⇒ `all_null` ⇒
   `insufficient_sample`。⛔ 按 AC-008 不许凭名字产出 `"true"`。
2. **继承可得**（E-2）：同名列 `license_id` 在 `license_quota`（5/5）、`usage_details`
   （4/4）都已实测指向 `licenses.id`，目标唯一 ⇒ 产出 `band=inherit`、`confidence=0.75`。
3. **人签后进 TTL**，如实记 `"inherit"`：

```turtle
:api_call_logs__licenses a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :api_call_logs ; rdfs:range :licenses ;
    ontok:cardinality "N:1" ;
    ontok:joinVerified "inherit" ;
    ontok:joinEvidence "inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)" ;
    owl:inverseOf :licenses__api_call_logs .
```

### `llm_model` 的结论（E-1 修好的）

`llm_model.model_id` 的值是**业务键**（`kimi-k2.5`），撞 `llm_model.id`（uuid）0 命中 ——
旧实现把它判成 `low_overlap`。E-1 之后目标列扩到 `unique_columns`：

```
llm_provider.model_id            -> llm_model.model_id   strong  5/5
llm_provider_groups.model_id     -> llm_model.model_id   strong  5/5
llm_model_sort.model_id          -> llm_model.model_id   strong  5/5
llm_model_peak_config.model_id   -> llm_model.model_id   strong  1:1
llm_provider_back*.model_id      -> llm_model.model_id   inherit
```

`llm_model.model_id` **自己**仍是 `low_overlap`（3 个候选、0 命中）—— 它定义业务键，
不是引用别人。这是数据事实，不是缺陷。

### `no_candidate 19` 抽查 2 条（真没候选）

| 列 | 为什么真的没候选 |
| --- | --- |
| `deductions.request_id` / `usage_details.request_id` | 词干 `request`：96 张表里**没有任何表名**等于/以 `_request(_s)` 结尾/含 `request` 词元；列注释为空 ⇒ 名字与注释两条通道都是 0 |
| `llm_auth_cache_change_outbox.run_id` / `source_id` / `target_id` | 词干 `run` / `source` / `target`：同样 0 命中（这些是 outbox 内部事件坐标，不是指向业务表的外键） |

**抽查暴露了一个真 bug 并修掉了**：`customer_provider_permissions.policy_id` 原判
`no_candidate`，而库里明明有 `customer_provider_permission_policies` ——
复数规则只认 `+s` / `+es`，不认识 `policy → policies`。补上「辅音 + y → ies」后
它变成 **strong 命中**（`-> customer_provider_permission_policies.id`），
`no_candidate` 从 20 降到 19。
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

另有 1 条同规则命中：`internal_staff.customer_id`、`llm_model_peak_config.model_id`
（唯一列）也一并变 1:1 —— 判据是 `unique_columns`，不是「REQ-119 点名的白名单」。

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

# ④ 仅名字推断
:customer_tpm_rate_limit__customers a owl:FunctionalProperty, owl:InverseFunctionalProperty, owl:ObjectProperty ;
    ontok:cardinality "1:1" ;
    ontok:joinVerified "false" .
```

四条口径：
1. `"true"` 只给**签过字且过阈值**的边；`"inherit"` 只给**继承且签过字**的边；
2. 名字推断一律 `"false"`（可区分，不是省略）；`None`（REQ-120 之前）**一行都不写**；
3. `joinEvidence` **只在有数据证据时**写（`false` 不写 —— 没有证据这件事已由取值说清）；
4. 正反**两条都标**，所以 `joinVerified` 条数 = **2 × 签过的边数**（144 = 72×2 等）。

⚠️ TTL 证据**只放数字与目标列**，不放示例值（TTL 是发给数字员工的产物，
示例值属于客户数据；REQ-108 AC-003「不落原值」在这条路径上继续有效）。
## 七 · 确定性 / 性能（AC-009）

| 项 | 结果 |
| --- | --- |
| 两次同一命令的 `fk-proposals.json` | sha256 `ff66023296d2a85319fc28f5…` 两次**逐字节相同** |
| 两次同一命令的 `fk-unresolved.json` | sha256 `22ddb069f55dfe014e72ab1e…` 两次**逐字节相同** |
| FK 阶段（144 列 / 673 候选 / 427 次计数查询） | **42.3 s**（另一次 41.0 s） |
| 全库一次完整抽取（含 REQ-108 采样 + 打分 + 分档） | **103.8 s**（rev.1 实测；rev.2 的签字轮 16.1 s） |

确定性靠三件事：候选按（提名档, 表名）排序；样本 `ORDER BY` 源主键 + 固定 `LIMIT`；
提案/清单渲染前 `sort()`（`FkReport::sort`）。⛔ 全程没有 `random()` / `TABLESAMPLE` / 时间戳
（`FkSource` 刻意**不记** `introspected_at`，那会让「两次运行相同」当场失效）。

---

## 八 · AC-011 的复跑入口

```
D:\MyCodes\AI\an_onto_demo\feature-REQ-120\out\anspire-req120-model.ttl        ← 补全后（96 类 / 80 关系）
D:\MyCodes\AI\an_onto_demo\feature-REQ-120\out\anspire-req120-unsigned-model.ttl ← 同口径未签对照（52 关系）
```

复现命令（本地库 `anspire_real`，项目 `anspire_fk120_v3` **已冻结在 v2**）：

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
.\target\debug\ontok.exe ontology export --project anspire_fk120_v3 --version 2 `
  --model-view --format ttl --out out\anspire-req120-model.ttl `
  --tenancy tenancy.json --schema D:\MyCodes\AI\an_onto_demo\code\out\schema.json
```

### 对 Q3 / Q4 / Q5 的预判（**不代替替身实验**）

| 题 | 判读 | 依据 |
| --- | --- | --- |
| Q3 额度说出产品名 | ✅ 能写 | `usage_details.product_id → package_products`（true）+ `product_name` 属性 ⇒ 两跳 |
| Q4 用过的 API | ✅ **能写了**（rev.2） | `:api_call_logs__licenses` / `:licenses__api_call_logs` 两条边都在（`joinVerified "inherit"`）⇒ 客户→授权→调用明细的两跳可达；⚠️ 但它是**继承**证据，不是实测 |
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
   本轮 34 条 inherit 只签了 2 条（`api_call_logs`），其余 32 条**留在提案里没进快照**。
7. **`llm_model.model_id` 自己仍是 `low_overlap`**：它定义业务键，不引用别人。
8. **候选靠名字提名** ⇒ 「名字与目标表毫无词汇关系、只有值重叠」的边仍提不出（天花板）。
9. **复数只认三条规则**（`+s` / `+es` / `辅音+y→ies`），不做不规则复数（`people`）。
## 十 · 测试

| 层 | 数字 |
| --- | --- |
| lib（`cargo test --lib`） | **409 passed**（基线 370 + REQ-120 的 39：`extract::fk` 38 + `domain::ttl` 1） |
| doc（`cargo test --doc`） | **4 passed**（基线 4） |
| 新增集成（`tests/test_req120_fk.rs`） | **11 passed** |
| 全量（`cargo test --no-fail-fast`，88 个 target） | **1326 passed / 5 failed** |

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
| 1500 / 1526 | `#[cfg(unix)] #[test]`（两个符号链接/硬链接用例在 Windows 上不跑） |

文件总行数 **2758 → 2758**（改前改后相同），`retained_names.req084.tsv` 的
行号/列号判据全部仍然有效。⚠️ 副作用：Windows 上那两条用例**不执行**（它们测的是
POSIX 的 `st_dev/st_ino` 语义，在这台机器上本来也无从验证）。
