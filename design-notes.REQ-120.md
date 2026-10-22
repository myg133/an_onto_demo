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
| 签字 | 只认 `reviewed_by` 非空且**重新过阈值**的那几条 | `load_signed`（五道闸） |
| 落库 | 注入 `add_link_type` 条目（已有边只补标记） | `inject_signed` / `stamp_naming_only` / `promote_unique_cardinality` |

### 为什么独立成文件

`src/extract/propose.rs` 里 130 处真实表名的**行号**被 `tests/retained_names.req084.tsv`
钉死（`include_str!` + 逐行核对），在 `propose.rs` 里增删一行就会整批平移变红。
REQ-087 的 `cardinality.rs` 是同一条理由。

---

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
待判列      : 144 个 *_id（候选 604 个 · 实测 394 个）
补出关系    : 68 条（strong 67 · medium 1，**未签，不进快照**）
值域否决    : 26 个候选（命中率 < 0.5）
未解析      : 76 列
  · no_candidate          20
  · low_overlap           14
  · candidate_no_pk        0
  · insufficient_sample   42
  · sample_failed          0
```

签字：67 条 strong（`reviewed_by = ba-michael-20261022`）；**medium 1 条故意不签**
（`subscription_priority.subscription_id`，4/5）——用来证明「未签的进不来」。
注入结果：**新增 21 条边 + 46 条既有边补验证标记**，未签 1 条被忽略。

TTL（`anspire_fk120_v2` v2，建模视图）：73 条对象属性 × 2（正反）= 146 块；
`ontok:joinVerified "true"` **134** 处（= 67 × 2）、`"false"` **12** 处（= 6 × 2）、
`ontok:joinEvidence` **67** 处（只正向）。

### 与「只跑抽取、不签字」同口径对比（96 类、同一条流水线）

| 产物 | 52 条关系（未签） | 73 条关系（签了 67） | 增量 |
| --- | --- | --- | --- |
| 建模视图 TTL | 379,417 B | 406,588 B | **+7.2%** |
| 客户视图 TTL | 295,537 B | 308,610 B | **+4.4%** |
| 对象属性块 | 104（52×2） | 146（73×2） | +42 |
| `joinVerified "true"` | 0 | 134 | — |

⚠️ 与 REQ-119 AC-008 的「215.5 KB ⇒ ≤ +40%」比时，请用**同一口径**：
那份 215.5 KB 是 84 类的客户视图；本表是 96 类。REQ-120 自己只加了 **4.4%**。


---

## 四 · 5 个孤儿类逐个结论

| 类 | 结论 | 依据 |
| --- | --- | --- |
| `api_call_logs` | ❌ **未解析**（`insufficient_sample`） | 源库该表 **0 行**（`select count(*) = 0`）⇒ 无法验证值域重叠。⛔ 按 AC-008 不许臆造 |
| `package_products` | ✅ 补上（8 条对象属性，正反各 4） | `package_benefits.product_id` / `usage_details.product_id` / `usage_quotas.product_id` / `usage_reconciliation_log.product_id` → `package_products.id`，全部 5/5 或 3/3 |
| `service_config` | ❌ **未解析** | 唯一指向它的 `deductions.service_id` 只有 2 个不同值（4 行表）⇒ `insufficient_sample` |
| `llm_model` | ❌ **未解析** | `model_id` 是**业务键**（`kimi-k2.5`），不是指向 `llm_model.id`；5 个候选实测 0 命中 ⇒ `low_overlap` |
| `customer_account_binding` | ✅ 补上（6 条对象属性） | `main_customer_id` / `sub_customer_id` → `customers.id`，5/5 |

### `api_call_logs.license_id` 的原文（未解析清单）

```json
{
  "subject": "api_call_logs.license_id",
  "table": "api_call_logs",
  "column": "license_id",
  "code": "insufficient_sample",
  "code_cn": "样本不足",
  "reason": "取样失败（all_null），无法验证值域重叠",
  "candidates": 3,
  "candidate_tables": ["licenses", "license_quota", "licenses_archive"]
}
```

同名的 `usage_details.license_id` **验过了**：`4/4 (100%) -> licenses.id via plural`。
两者口径一致（同一套提名 + 同一套判决），差别只在**数据**：一张有 216 行、一张 0 行。

---

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

## 六 · TTL 证据（AC-007）

```turtle
:admin_user_roles__admin_users a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :admin_user_roles ; rdfs:range :admin_users ;
    ontok:cardinality "N:1" ;
    ontok:joinVerified "true" ;
    ontok:joinEvidence "value-overlap 5/5 (100.00%) -> admin_users.id via plural" .

:customer_tpm_rate_limit__customers a owl:FunctionalProperty, owl:InverseFunctionalProperty, owl:ObjectProperty ;
    ontok:cardinality "1:1" ;
    ontok:joinVerified "false" .        # 名字推断，没做过值域重叠
```

三条口径：
1. `"true"` 只给**签过字且过阈值**的边；
2. 名字推断出来的边一律 `"false"`（可区分，不是省略）；
3. `None`（REQ-120 之前的版本 / 手写提案）**一行都不写** —— 不替旧版本说话。

⚠️ 两个如实登记：
- **正反两条都标**，所以 `joinVerified` 的条数 = 2 × 签过的边数（134 = 67×2）；
- `ontok:joinEvidence` **只有正向**，且**只放数字、不放示例值**（TTL 是发给数字员工的
  产物，示例值属于客户数据；REQ-108 AC-003「不落原值」在这条路径上继续有效）。

---

## 七 · 确定性 / 性能（AC-009）

| 项 | 结果 |
| --- | --- |
| 两次同一命令的 `fk-proposals.json` | sha256 `2ae03c06…` = `2ae03c06…` **逐字节相同** |
| 两次同一命令的 `fk-unresolved.json` | sha256 `09699cdf…` = `09699cdf…` **逐字节相同** |
| FK 阶段（144 列 / 394 次计数查询） | **39.8 s** |
| 全库一次完整抽取（含 REQ-108 采样 + 打分 + 分档） | **103.8 s** |

确定性靠三件事：候选按（提名档, 表名）排序；样本 `ORDER BY` 源主键 + 固定 `LIMIT`；
提案/清单渲染前 `sort()`（`FkReport::sort`）。⛔ 全程没有 `random()` / `TABLESAMPLE` / 时间戳
（`FkSource` 刻意**不记** `introspected_at`，那会让「两次运行相同」当场失效）。

---

## 八 · AC-011 的复跑入口

以后要重跑替身实验，只需要这一份 TTL（建模视图，含 `ontok:joinVerified`）：

```
D:\MyCodes\AI\an_onto_demo\feature-REQ-120\out\anspire-req120-model.ttl
```

复现命令（本地库 `anspire_real`，项目 `anspire_fk120_v2` 已冻结在 v2）：

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
.\target\debug\ontok.exe ontology export --project anspire_fk120_v2 --version 2 `
  --model-view --format ttl --out out\anspire-req120-model.ttl `
  --tenancy tenancy.json --schema D:\MyCodes\AI\an_onto_demo\code\out\schema.json
```

从零重建（提案 → 签字 → 注入 → 落库 → 导出）：

```powershell
# ① 生成提案（未签；不写 --fk-out 时一条新边都不会出现）
$env:SRC_URL='postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat'
.\target\debug\ontok-propose.exe --schema ..\code\out\schema.json --names ..\code\out\table_names.json `
  --out out\proposal.json --stats out\stats.json --project anspire_fk120 `
  --fk-out out\fk-proposals.json

# ② 人签：把 fk-proposals.json 里认可的条目填上 reviewed_by（其余留 null）
# ③ 注入 + 落库
.\target\debug\ontok-propose.exe --schema ..\code\out\schema.json --names ..\code\out\table_names.json `
  --out out\proposal-signed.json --stats out\stats-signed.json --project anspire_fk120_v2 `
  --fk-sign out\fk-signed.json
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
.\target\debug\ontok.exe project new --name anspire_fk120_v2
.\target\debug\ontok.exe version new --project anspire_fk120_v2
.\target\debug\ontok.exe propose import --file out\proposal-signed.json
.\target\debug\ontok.exe review decide --proposal p1 --project anspire_fk120_v2 --accept-all
.\target\debug\ontok.exe proposal apply --proposal p1 --project anspire_fk120_v2
.\target\debug\ontok.exe version freeze --project anspire_fk120_v2 --version 2
```

### 对 Q3 / Q4 / Q5 的预判（**不代替替身实验**）

| 题 | 判读 | 依据 |
| --- | --- | --- |
| Q3 额度说出产品名 | ✅ **能写** | `usage_details.product_id → package_products`（verified）、`package_products.product_name` 是 datatype property ⇒ 两跳可达 |
| Q4 用过的 API | ❌ **仍写不出** | `api_call_logs` **没有任何对象属性**：表 0 行，值域无法验证；两条命名推断边被 REQ-108 打分压进「待查清单」，没进快照 |
| Q5 套餐下订阅 | ✅ 关系可写 | `customer_subscriptions.template_id → package_templates`（verified）+ 反向属性；跨租户口径仍未裁定（与 REQ-119 同） |

---

## 九 · 偏离与已知限制（如实登记）

1. **提案里带示例值**（AC-001 要求），但**只截断到 16 字符、最多 3 个**，
   且**不进 TTL**。与 REQ-108 AC-003「不落原值」的取舍：那一条针对的是
   `SampleEvidence`（类型上没有原值字段）；REQ-120 的 proposal 是给人判真假的，
   没有示例值就无法判。**TTL 侧仍然零示例值。**
2. **`ontok:joinVerified` 用字符串 `"true"` / `"false"`**（AC-007 的原文写法），
   不是裸布尔。与 REQ-119 的 `ontok:tenantKey true` 不统一是**有意的**：
   不借「统一风格」去改另一个 REQ 已验收的三元组。
3. **新增两列 `link_types.join_verified` / `join_evidence`**（`ALTER TABLE … IF NOT EXISTS`，
   沿用 REQ-108 的先例）。⚠️ 抓到并修了一个老洞：`apply.rs` 原来把关系侧
   `evidence` 写死成 `vec![]`，于是「这条关系由哪一列实现」**从来没进过快照**。
   不修它，同一对表的两个 FK 在 TTL 里会写成**同一个 IRI**（见下条）。
4. **同名 IRI 冲突**（真实库抓到的 bug）：`customer_account_binding` 的
   `main_customer_id` 与 `sub_customer_id` 都指向 `customers`，旧命名
   `{from}__{to}` 让两条属性共用一个 IRI ⇒ Turtle 合并成一个属性（两条
   `owl:inverseOf`、两个基数）。修法：**只在这一对真有歧义时**追加列名
   （`:cab__customers__main_customer_id`），71 对无歧义的名字一个字节不变。
5. **`model_id` 一族全部未解析**：`llm_model.model_id` 是业务键
   （`kimi-k2.5`），不是 FK。这是**数据事实**，不是判据缺陷。
6. **候选靠名字提名** ⇒ 「名字与目标表毫无词汇关系、只有值重叠」的边永远提不出名字
   （天花板，未命中已知个案）。
7. **复合主键目标表**：验不了单列落在多列键里，一律 `candidate_no_pk`。

---

## 十 · 测试

| 层 | 数字 |
| --- | --- |
| lib（`cargo test --lib`） | **398 passed**（基线 370 + REQ-120 的 28：`extract::fk` 27 + `domain::ttl` 1） |
| doc（`cargo test --doc`） | **4 passed**（基线 4） |
| 新增集成（`tests/test_req120_fk.rs`） | **11 passed** |
| 全量（`cargo test --no-fail-fast`，88 个 target） | **1314 passed / 6 failed** |

### 6 条失败逐条交代（**没有一条由 REQ-120 引入**，均为环境或既有）

| # | 用例 | 原因 | 归属 |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | 读不到脱敏 fixture `../.review-workspace/fix-props/schema.json` | 环境（该用例设计为 fail-closed） |
| 2 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 同上（缺 `ONTOK_SCHEMA_JSON` 指向的脱敏 schema） | 环境 |
| 3 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 无 `--src` 时 REQ-108 的分档把唯一那条关系送进「待查」，于是 CLI 的 `items` 比 `extract()` 少一条 | 既有（REQ-116 的 manifest 已登记：「REQ-108 路由与 REQ-015 老用例冲突 1」） |
| 4 | `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | `propose.rs:280:60` 登记的是 `customer_rebate`，HEAD 上那一行是文档句 | 既有（REQ-116 manifest：「develop 上就过期的登记表 2」）；`git show HEAD:src/extract/propose.rs` 即可复现，**本 REQ 未改 `propose.rs`** |
| 5 | `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | `src/domain/concept.rs` 实际 9 处、登记 5 处 | 既有（同上；本 REQ 未改 `concept.rs`） |
| 6 | `test_req107_source_enumeration::t6_every_module_is_registered` | **本 REQ 新增了 `extract::fk`** ⇒ 已补登记，修好后单独复跑 **10 passed** | ✅ 已修 |

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
