# QA 审核：REQ-119 · 本体可查询化（Pre-merge，B-1/B-2/B-3 返修轮）

| 项 | 值 |
| --- | --- |
| 被测 | `feature-REQ-119`（`feature/REQ-119` @ `a3894b5`，3 个提交：`69165d0` → `8aa2464` → `a3894b5`） |
| 基线 | `develop` `2b55624` |
| QA 时间 | 2026-10-22 22:40–22:56（Asia/Hong_Kong） |
| 环境 | Windows / Rust 1.96.0 / 本地库 `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`（只读导出）＋ `.../ontok_demo`（写测试） |
| schema | `code/out/schema.json`（96 表 / 1116 列 / 真实 `data_type`） |
| 变异 | 独立 worktree `.review-workspace/qa-req119-mut`（detached @ `a3894b5`）；feature worktree 零改动、未 push |
| 报告 | 本文件 |

## 0 · 结论

**PASS 带非阻断（4 条非阻断，0 阻断）**

三条返修（B-1 / B-2 / B-3）**都真修对了**，不是「改到测试绿」：

| # | 判据 | 实测 | 结论 |
| --- | --- | --- | --- |
| B-1 | `(N:1,Functional)=52` + `(1:N,InverseFunctional)=52`，反向 0 | **52 / 52 / 反向 0**；52 对逐对校验 0 异常；OWL 语义独立判定一致 | ✅ 真修对 |
| B-2 | `primaryKeyColumn` / `isPrimaryKey` 84/84；只在 `--schema`；复合主键跳过 | **84 / 84**；未给 schema 0、客户视图 0；复合主键实测跳过 | ✅ |
| B-3 | 104 条 `allowedValue`，值域必须来自列注释 | **104 条 / 104 属性**；逐条对照注释 **0 条编造**；声称拒绝的 4 类形态实测均未导出 | ✅ 真按注释解析 |
| A-1 | `--schema` 870/870 一致 | **952/952 一致（含 82 条补出的主键属性；原始 870 = 868+2）**，heuristic 0、Literal 0 | ✅ 回归通过 |
| 隔离/确定性/体积 | 客户视图 0 个 tenant；两次逐字节相同；≤40% | tenant 0；sha 两次相同；**+25.6%**（上限 +40%） | ✅ |
| 不退化 | lib 370 / doc 4 / REQ-119 22 / REQ-110 15 / 枚举 10 | **全部命中**；3 个既有红灯在 develop 上同样红、失败文本逐字相同 | ✅ |
| 红线 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `Cargo.*` | 与 `develop` 的 `numstat` **全空** | ✅ |

> 一句话：**「代码全绿」这次是「语义也对」**——B-1 我用 OWL 语义逐条反推过，B-3 我用列注释逐字反查过，两者都不是靠计数糊过去的。

---

## 1 · B-1 基数方向（核心，单独成节）

### 1.1 计数分布（QA 自解析，不采信 Dev 数字）

解析 `out/qa/model-schema.ttl`（我自己重新导出，见 §8）全部 104 个 `owl:ObjectProperty`，按「是否有 `ontok:key`」区分正向（52，抽取得到的）/ 反向（52，生成器补出的，`rdfs:label` 带「（反向）」）：

```
(N:1, Functional)              = 52      ← 应为 Functional，是
(1:N, InverseFunctional)       = 52      ← 应为 InverseFunctional，是
(N:1, InverseFunctional)       =  0      ← 反向计数，必须为 0
(1:N, Functional)              =  0      ← 反向计数，必须为 0
owl:inverseOf 语句总数          = 104      ← 双向各一条
既是 Functional 又 InverseFunctional(1:1) = 0
```

客户视图同口径（该视图无 `ontok:key`，按 `rdfs:label` 的「（反向）」+ `rdfs:comment` 的「多对一 / 一对多」判）：

```
(多对一, Functional)             = 52
(多对一|反向, InverseFunctional) = 52
结构错误（缺反向 / domain-range 未对调 / 缺回指） = 0
```

### 1.2 52 对逐对结构校验（不是抽查，是全量）

对每一对 `fwd --owl:inverseOf--> inv` 断言四件事，**0 条失败**：

1. `inv.owl:inverseOf == fwd`（双向回指）；
2. `fwd.domain == inv.range` 且 `fwd.range == inv.domain`（R2 对调）；
3. 基数标签对调：`N:1 ↔ 1:N`；
4. OWL 特征对调：`Functional ↔ InverseFunctional`。

### 1.3 OWL 语义独立判定（**不看计数，自己推**）

判定规则（OWL 2 语义）：

- `FunctionalProperty(P)`：若 `P(a,y)`、`P(a,z)` ⇒ `y = z`，即**每个 subject 至多一个 object**；
- `InverseFunctionalProperty(P)`：若 `P(a,y)`、`P(b,y)` ⇒ `a = b`，即**每个 object 至多一个 subject**（= 反向函数性）。

逐条核对本版 52 条关系的物理事实（用 `schema.json` 校验：52 条的源列全部在真实表里存在，52 个目标类都有单列主键）：

| 关系（示例） | 物理事实 | 正确标注 | 导出标注 | 判定 |
| --- | --- | --- | --- | --- |
| `deductions → customers`（`deductions.customer_id`） | 一条扣费行只存**一个** `customer_id` ⇒ 每个 subject 至多一个 object | **Functional** | `Functional` | ✅ |
| `customers → deductions`（反向） | 一个客户可有多条扣费；但一条扣费只属于一个客户 ⇒ 每个 object 至多一个 subject | **InverseFunctional** | `InverseFunctional` | ✅ |
| `usage_details → licenses` | 同上（单列 FK） | Functional | Functional | ✅ |
| `customer_subscriptions → package_templates` | 一个订阅只挂一个套餐 | Functional | Functional | ✅ |
| `package_templates → customer_subscriptions`（反向） | 一个套餐下多个订阅；每个订阅只属于一个套餐 | InverseFunctional | InverseFunctional | ✅ |

**结论**：`deductions → customers` 现在是 `owl:FunctionalProperty`（对）；`customers → deductions` 是 `owl:InverseFunctionalProperty`（对）。旧版把两者整体对调，**开推理会把一个套餐下的所有订阅合并成一条**——这个后果现在**不再成立**：新标注下的合并语义是「同一条扣费不可能属于两个客户」，这才是数据里真实成立的约束。

**另一条独立的佐证**（顺带发现的正面证据）：6 条关系的源列在 `schema.json` 的 `unique_columns` 里是唯一列（`balances.customer_id`、`customer_rate_limit.customer_id`、`customer_tpm_rate_limit.customer_id`、`dim_customer_binding.customer_id`、`license_quota.license_id`、`service_apps.channel_id`）⇒ 它们其实是 **1:1**。当前把它们标成 `N:1`（只 `Functional`）是**少承诺**：不会错误合并任何个体，只是缺了「每个对象至多一个 subject」那一半约束。归为非阻断项 N-1。

### 1.4 抽查 3 对正反属性（TTL 原文）

```turtle
:deductions__customers a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :deductions ; rdfs:range :customers ;
    ontok:cardinality "N:1" ; ontok:key "deductions_customer_id__customers" ;
    owl:inverseOf :customers__deductions .

:customers__deductions a owl:InverseFunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :customers ; rdfs:range :deductions ;
    ontok:cardinality "1:N" ;
    owl:inverseOf :deductions__customers .

:usage_details__licenses a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :usage_details ; rdfs:range :licenses ;
    owl:inverseOf :licenses__usage_details .

:licenses__usage_details a owl:InverseFunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :licenses ; rdfs:range :usage_details ;
    owl:inverseOf :usage_details__licenses .

:customer_subscriptions__package_templates a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :customer_subscriptions ; rdfs:range :package_templates ;
    owl:inverseOf :package_templates__customer_subscriptions .

:package_templates__customer_subscriptions a owl:InverseFunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :package_templates ; rdfs:range :customer_subscriptions ;
    owl:inverseOf :customer_subscriptions__package_templates .
```

（三段均已核对：domain/range 对调、`owl:inverseOf` 互指、基数方向正确。A-2 的撞名例外 `customer_subscriptions__order__inverse` 也在，计数仍 52/104。）

### 1.5 变异（独立 worktree）

把 `ttl.rs` 里 4 个 match 分支改回旧方向（`多对一→InverseFunctional`、`一对多→Functional`、`N:1→InverseFunctional`、`1:N→Functional`），**不改测试**：

| 变异 | 结果 | 抓住它的测试 |
| --- | --- | --- |
| M1 基数方向回退 | **红** | `domain::ttl::tests::inverse_is_declared_both_ways_and_swaps_domain_range`（src/domain/ttl.rs:2498）<br>`domain::ttl::tests::model_export_maps_the_nine_rows`（ttl.rs:2395）<br>`b1_n_to_one_is_functional_and_one_to_n_is_inverse_functional`（test_req119:586） |

测试输出直接点名语义：「N:1（deductions → customers）必须 Functional」，且回退后该属性确实是 `owl:InverseFunctionalProperty`。**变异必红，已满足。**

---

## 2 · B-2 主键

### 2.1 数量（模型视图，`--schema` 模式）

```
类：84                     ontok:primaryKeyColumn：84
ontok:isPrimaryKey true：84
主键列分布：id × 82 · app_id × 1（service_apps） · sub_customer_id × 1（sub_account_bind_permission）
每个类的 primaryKeyColumn 都有对应 dt 属性且标了 isPrimaryKey：84/84（缺失 0）
```

与 `schema.json` 对照：84 个类的 `primary_key` **逐条相等**（0 不一致、0 类不在 schema 里）；`id` 之外的 2 个真主键（`service_apps.app_id`、`sub_account_bind_permission.sub_customer_id`）也对得上。

属性总数账目自洽：**952 = 870（抽取原有）+ 82（schema 补出）**；84 条 `isPrimaryKey` = 82 条补出 + 2 条抽取侧本来就有的主键列（这两条的 `rdfs:label` 是中文业务名「应用唯一标识，如 mplus」/「子账号ID」，其余 82 条补出块的 label 就是列名）。

### 2.2 只在 `--schema` 模式出现

| 导出 | `primaryKeyColumn` | `isPrimaryKey` |
| --- | --- | --- |
| `--model-view --tenancy`（**无** `--schema`） | **0** | **0** |
| `--model-view --tenancy --schema` | 84 | 84 |
| 客户视图（有/无 `--schema`） | **0** | **0** |

符合设计（主键是建模信息，客户视图无列名）。反判据也成立：**没给 schema 时不会「猜」主键**。

### 2.3 复合主键跳过（代码 + 单测 + 实测三重）

- 代码：`SchemaTypes::parse` 里 `if t.composite_primary_key.is_empty()` 闸住，注释明写「⛔ 不把多个列拼成一个假列」；
- 单测：`domain::ttl::tests::schema_primary_key_is_single_column_only` 断言复合主键 ⇒ `None`（M2 变异会打红，见下）；
- **实测**（不是只看代码）：拿真实 schema 复制一份，把 `activity` 改成复合主键（`composite_primary_key=["id","status"]`、`primary_key=null`），再导出：

```
activity primaryKeyColumn: None
activity__id isPrimaryKey : None
activity__id 块存在        : False      ← 没有拼假列
primaryKeyColumn 总数       : 84 → 83   ← 只少了这一条
customers primaryKeyColumn  : id        ← 其它类不受影响
```

### 2.4 变异（独立 worktree）

`SchemaTypes::primary_key()` 直接返回 `None`（删掉主键导出）：

| 变异 | 结果 | 抓住它的测试 |
| --- | --- | --- |
| M2 主键导出失效 | **红** | `domain::ttl::tests::schema_primary_key_is_single_column_only`（left: None, right: Some("id")）<br>`b2_primary_key_is_exported_even_when_extraction_dropped_it`（left: 0, right: 4） |

---

## 3 · B-3 枚举值域（是否真按注释解析）

### 3.1 条数

```
模型视图（--schema）：ontok:allowedValue 语句 104 条 / 覆盖 104 个属性
模型视图（无 schema）：104（与 schema 无关，值域只看注释）
客户视图：0（结构上没有列名，符合设计）
```

### 3.2 逐字对照（不是抽查 10 条，是全量 104 条按「注释里的 (code, label) 对」反查）

方法：对每条 `ontok:allowedValue` 拆成 `code=label` 对，回列注释里找 `code` + 分隔符（`-` / `:` / `=`，允许空白与全半角） + `label`。**104/104 对得上，0 条编造。**

抽 10 条明细（左=TTL，右=真实列注释，逐字来自 `schema.json`）：

| 属性 | `ontok:allowedValue` | 列注释（`schema.json`） |
| --- | --- | --- |
| `deductions__currency` | `POINT;RMB;USD` | `扣减币种：实际扣减的币种（POINT/RMB/USD）` |
| `customer_subscriptions__status` | `1=生效中;2=已过期;3=已取消;4=已升级;5=已退款` | `状态：1-生效中，2-已过期，3-已取消，4-已升级，5-已退款` |
| `enterprise_recharge_order__currency` | `RMB=人民币;USD=美元;POINT=免费点数` | `充值币种: RMB-人民币, USD-美元, POINT-免费点数 (由应用层枚举校验)` |
| `order__status` | `rech=成功;pending=支付中;free=取消` | `状态 rech:成功 pending:支付中 free:取消` |
| `llm_model__type` | `0=llm;1=emb` | `0: llm, 1: emb` |
| `recharge_products__status` | `active=启用;inactive=停用` | `商品状态:active-启用,inactive-停用` |
| `package_templates__status` | `1=上架;0=下架;2=草稿` | `状态：1-上架（在售），0-下架，2-草稿` |
| `customers__type` | `0=未分类;1=个人用户;2=企业用户` | `0: 未分类, 1: 个人用户, 2: 企业用户` |
| `activity_recommendations__active` | `true=启用;false=禁用` | `是否启用：true=启用，false=禁用` |
| `deductions__payer_account_type` | `1=主账号;2=子账号` | `扣费账号类型：1-主账号，2-子账号` |

（另核 `activity__status`：注释 `状态(0-未开始,1-活动中,2-已结束,3-已暂停,-1-已删除)` ⇒ TTL `0=未开始;1=活动中;2=已结束;3=已暂停;-1=已删除`，负数码也没丢。）

### 3.3 Dev 声称「拒绝」的形态，逐类实测

| 形态 | 命中的真实列 | 是否被导出 | 实测 |
| --- | --- | --- | --- |
| `模型系列(如: GPT, Claude, GLM)` | `llm_provider.family` / `llm_provider_groups.family` / `llm_provider_back*.family` 等 | **否** | 含「如」的 23 个列，`allowedValue` 0 条 |
| `最后一次登录IP地址（兼容IPv4/IPv6）` / `IP地址（兼容IPv4/IPv6）` | `customers.last_login_ip`、`ip_blacklist.ip`、`ip_whitelist.ip` | **否** | 3 列 0 条 |
| `…保存原始tokens/minute` | `customer_tpm_rate_limit.llm_tpm` | **否** | 1 列 0 条 |
| `NULL=…` 伪码 | `family_high_risk_records.whitelist_until`、`ip_whitelist.customer_id`、`package_templates.sale_limit` | **否** | 3 列 0 条 |

另外 17 条「裸码列表」形态（如 `freeze;transfer;unfreeze;reclaim`、`monthly;quarterly;semi_yearly;yearly`）是注释里的**值域位置斜杠串**，导出为码列表是对的（`扣减币种…（POINT/RMB/USD）` 同型）。

### 3.4 变异（独立 worktree）

给解析器加一条「遇到 `如:` 就把后面逗号串当枚举」的分支（即 BA 要求的「让解析器把『如:』也算进去」）：

| 变异 | 结果 | 抓住它的测试 |
| --- | --- | --- |
| M3 「如:」举例被当值域 | **红** | `domain::ttl::tests::allowed_value_parser_never_invents`（left: `Some("GPT;Claude;GLM")`, right: `None`）<br>`b3_allowed_values_come_only_from_explicit_comments`（消息：不该从 `模型系列(如: GPT, Claude, GLM)` 编出值域） |

**这条不是「没有测试盯」**：单元测试与集成测试两层都钉住了。

---

## 4 · A-1 xsd range 回归（870/870）

### 4.1 给 `--schema`：逐条比对（自写脚本，按 UTF-8/JSON 读真实 `data_type`）

```
属性 952 · 按 (表, 列) 命中 schema 952 · 与真实 data_type 映射一致 952 · 不一致 0
rangeSource: schema 952 / heuristic 0 · rdfs:Literal 0 · rangeUnmapped 0
分布：string 550 · dateTime 191 · integer 133 · decimal 76 · boolean 2 · date 0
```

拆开看 A-1 原口径：**870 条原始属性 = 868 条未标主键 + 2 条抽取侧本来就有的主键列，全部一致（870/870）**；额外 82 条补出的主键属性也全部走 schema（同一条映射管线，没有特例）。

### 4.2 不给 `--schema`：三处标记都在

| 位置 | 实测 |
| --- | --- |
| ① 文件头 | `# · xsd 类型来源：**启发式**（未读真实列类型；--schema <schema.json> 可消除）—— schema 0 / heuristic 870` |
| ② 本体节点 | `ontok:rangeSourceSchema 0 ;` + `ontok:rangeSourceHeuristic 870 ;`，且每个属性都有 `ontok:rangeSource "heuristic"`（870/870） |
| ③ stderr | `⚠ xsd 类型来源：未读真实列类型，全部来自启发式（--schema <schema.json> 可消除）` |

同时复现了 A-1 记录的不一致率：**启发式 vs 真实 不一致 116/870 = 13.3%**（`activity.status`→`xsd:string`、`agent_device_version.version_number`→`xsd:integer`、`balance_operation_log.point_change`→`xsd:integer`、`activity_keys.daily_ip_limit`→`xsd:decimal` 全部命中），证明「不给 schema 会算错」不是纸面风险。客户视图 843 条全启发式（无列名，`--schema` 不生效），文件头/stderr 均明说。

---

## 5 · 客户视图隔离 + 确定性 + 体积

### 5.1 隔离（R4 / AC-004 / AC-007）

| 检查 | 实测 |
| --- | --- |
| 客户视图 `tenant` 字样（大小写不敏感） | **0** |
| 客户视图 `ontok:tenantKey` / `tenantColumn` / `tenantUnannotated` | **0 / 0 / 0** |
| 客户视图 + `--tenancy` vs 不带 | **逐字节相同**（同为 277,289 B，sha `4BBD623F…D64AA3`） |
| 建模视图（无 `--tenancy`） | `tenantKey` 54 / `tenantColumn` 54 / `tenantUnannotated true` + 文件头「未标注」+ stderr 警告（三处齐） |

A-3 的口径成立：R5 的三处标记**只对建模视图有意义**；客户视图的正确判据是「一个 tenant 都没有」，已满足。

### 5.2 确定性（AC-006）

同版本进程级两次导出：

| 文件 | 字节 | sha256（两次） |
| --- | --- | --- |
| 客户视图 | 277,289 | `4BBD623F59C6754A588EAA5F393E9E14554A9605851B3AE4D512A3EBA3D64AA3`（×2 相同） |
| 客户视图 + tenancy | 277,289 | 同上 |
| 建模视图 + tenancy + schema | 323,965 | `058CBA10944C3D24020A784317C17E6076139A953FDC48468E2AFF3E3270576C`（×2 相同） |
| 建模视图 + tenancy（无 schema） | 302,798 | `81DCF0F3D58D7DEAB867A7C21481934A46397BEBB37815E8CBBFA8FD1BE6F332` |

### 5.3 体积（AC-008）

| 文件 | 字节 | 对基线 | 判据 |
| --- | --- | --- | --- |
| 客户视图 | 277,289 | 220,696 → **+25.6%** | ≤ +40%（308,974）✅ |
| 建模视图（schema） | 323,965 | 232,921 → **+39.1%** | ≤ +40% ✅（余量 1.0 个百分点，见 N-4） |

---

## 6 · 不退化

### 6.1 测试数字（feature @ `a3894b5`，自跑，非引用 Dev 日志）

| 套件 | 期望 | 实测 |
| --- | --- | --- |
| lib 单测 | 370 | **370 passed / 0 failed** |
| doc-tests | 4 | **4 passed** |
| `tests/test_req119_queryable_ttl.rs` | 22 | **22 passed**（含 2 条真库 live 判据） |
| `tests/test_req110_ttl.rs` | 15 | **15 passed** |
| `tests/test_req107_source_enumeration.rs` | 10 | **10 passed** |
| `tests/test_req107_red_lines.rs` | — | 2 passed |

### 6.2 3 个既有红灯：自己在 develop 上复现（不是听 Dev 说）

同一环境、同一 shim、只跑这 4 个目标，`develop`（`2b55624`）与 `feature/REQ-119` 的失败**文本逐字相同**：

| # | 测试 | feature | develop（对照） | 归因 |
| --- | --- | --- | --- | --- |
| 1 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | FAILED | **FAILED（同文本）** | legacy extract 少一条 `add_link_type`（REQ-059 既有） |
| 2 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | FAILED（`agent_white_list/customer_id 仍显示成「客户标识」`） | **FAILED（同文本）** | 外键显示名回归门禁既有红灯；另在没设 `ONTOK_SCHEMA_JSON` 的机器上表现为「读不到硬编码 Linux 路径 `/home/myg133/...`」 |
| 3 | `test_retained_names_req084`（2 条） | FAILED | **FAILED（同文本）** | ① `src/extract/propose.rs:280` 登记的行漂移；② `src/domain/concept.rs` 真名 9 处 vs 登记 5 处 |
| 4* | `drop_rate::real_schema_drop_rate_report` | FAILED（69.8% > 10%） | **FAILED（同 715/1024 = 69.8%）** | 该测试只在设了 `ONTOK_SCHEMA_JSON` 时才跑；抽取/词表未改，属既有 |

⇒ **4 个红灯全部前置于本分支**，REQ-119 的 3 个提交没有引入新红。Dev 报的「3 个」与实测的「3 个（+1 个环境开关触发的 drop_rate）」一致。

### 6.3 测试绕行（BA 给的方法在本仓不可用，已换法并留证）

- BA 给的方式「挪开 `src/bin/ontok-gate-preflight.rs`」**不成立**：`tests/test_retained_names_req084.rs:52` 对该文件做 `include_str!`，挪走 ⇒ 该测试目标**编译失败**（实测：`couldn't read tests\../src/bin/ontok-gate-preflight.rs`）。此点已记入非阻断项 N-2。
- QA 采用的办法：**行号不变**的 3 行 shim（587/588 行 `#[cfg(unix)]`、1318 行 `#[cfg(all(test, unix))]`），其余字节全保留 ⇒ 既能让 unix-only 的运维二进制在 Windows 编译，又让 req084 的「登记名逐行位置」判据读到**真实内容**。
- 还原证据（每次运行 finally 打印）：

```
RESTORE sha_before=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
        sha_after =C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
        match=True Test-Path=True
git -C feature-REQ-119 status --short   →  空
```

还原后已 `touch`（`LastWriteTime=Now`）避免 mtime 坑；变异 worktree 的 `ttl.rs` / preflight 也逐次校验 sha 一致。

### 6.4 红线与接口 diff

```
git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs \
    src/domain/view.rs src/domain/review_view.rs     → 空
git diff develop -- src/mcp/tools.rs                  → 空（guard() 实现未动）
git diff develop -- src/domain/property_filter.rs      → 空（G1–G4 逐字节未变）
git diff develop -- Cargo.toml Cargo.lock              → 空
```

分支总 diff 只有：`design-notes` / `ttl.rs`（+2040）/ `cli.rs`（+109/-… ）/ `pipeline.rs`（+23，只加只读 `range_stats` 访问器）/ `http.rs`（+6，`TtlOptions` 新字段的构造点，编译必需）/ `tenancy*.json` / 3 个测试文件。**没有改抽取、评分、分组、出口守门器。**

---

## 7 · 阻断项 / 非阻断项

### 阻断（0 条）

无。

### 非阻断（4 条，建议登记，不挡合并）

| # | 问题 | 证据 | 影响 | 建议 |
| --- | --- | --- | --- | --- |
| **N-1** | 6 条实际是 **1:1** 的关系被标成 `N:1`（只 `Functional`，缺 `InverseFunctional`） | `schema.json.unique_columns` 里 `balances.customer_id`、`customer_rate_limit.customer_id`、`customer_tpm_rate_limit.customer_id`、`dim_customer_binding.customer_id`、`license_quota.license_id`、`service_apps.channel_id` 均为唯一列 | **欠约束**：不会错误合并个体；只是推理少了「一个对象只能被一个主体指向」这一半 | 另开小需求：抽取侧把 `unique_columns` 用起来 → `1:1 ⇒ Both`。注意方向：**宁少勿多**（B-1 的教训是「多承诺」才会错合并） |
| **N-2** | BA 给的「挪开 preflight 跑测试」方式不成立 | `test_retained_names_req084.rs:52 include_str!("../src/bin/ontok-gate-preflight.rs")` ⇒ 挪走即编译失败 | 后续 QA 照抄会卡住 | 把验收方式改成「行号 shim」或「`autobins=false` + 显式 `[[bin]]`」，并写进 §3 的环境事实 |
| **N-3** | 客户视图**没有**枚举值域与主键（结构上没有列名） | 客户视图 `allowedValue` 0、`primaryKeyColumn` 0 | 若将来让数字员工读客户视图，Q 类问题（`status=2` 的含义、FK join）能力为 0 | 在 REQ-122 的 Agent 契约里明示「消费入口 = 建模视图 TTL」，别让客户视图承担查询职责 |
| **N-4** | 建模视图体积 **+39.1%**，距 40% 上限只剩 1.0 个百分点 | 323,965 / 232,921 | 下一次加属性/加值域就可能越限 | 体积判据建议从「相对基线百分比」改成「绝对值 + 每类属性预算」，或在 REQ-120 前先决定是否压缩（更短 IRI 前缀 / 去掉 `ontok:key` 冗余） |

### 观察到但不算问题（登记备查）

- 建模视图 952 条 dt 属性 = 870 原有 + 82 补出主键；账目自洽，`rangeSource schema 952 / heuristic 0` 与文件头一致。
- `--schema` 模式下 `ontok:rangeUnmapped` 0 条；无 schema 模式 1 条（`verify`，被 stderr 列出）——符合 R3「必须计数并列出」。
- B-4（`api_call_logs` 等 5 个类 0 边）**不在本需求**，本次未验，仍按登记处理。

---

## 8 · 复现命令（QA 实测用）

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-119
cargo build --bins      # preflight 需 shim/挪开（见 §6.3）

$S='D:\MyCodes\AI\an_onto_demo\code\out\schema.json'
.\target\debug\ontok.exe ontology export --project anspire_grouped --version 3 --model-view --tenancy tenancy.json --schema $S --out out\qa\model-schema.ttl
.\target\debug\ontok.exe ontology export --project anspire_grouped --version 3 --model-view --tenancy tenancy.json --out out\qa\model-noschema.ttl
.\target\debug\ontok.exe ontology export --project anspire_grouped --version 3 --out out\qa\customer.ttl
.\target\debug\ontok.exe ontology export --project anspire_grouped --version 3 --tenancy tenancy.json --out out\qa\customer-tenancy.ttl

# 独立核验脚本（QA 自写，不依赖 Dev 的 out/*.txt）
python D:\MyCodes\AI\an_onto_demo\.local\qa-req119\analyze.py      # 计数 / 基数 / 主键 / 值域 / A-1 / tenant
python D:\MyCodes\AI\an_onto_demo\.local\qa-req119\semantics.py   # B-1 语义 + 值域逐字对照 + 拒绝形态
```

测试（含 shim 与还原、环境变量）：

```powershell
pwsh -File .local\qa-req119\run-tests-shim.ps1 `
  -Workdir D:\MyCodes\AI\an_onto_demo\feature-REQ-119 `
  -Log     D:\MyCodes\AI\an_onto_demo\.local\qa-req119\feature-fulltest.log
# 变异：M1 基数回退 / M2 主键失效 / M3 「如:」被当值域
pwsh -File .local\qa-req119\run-mutation.ps1 -Workdir .review-workspace\qa-req119-mut -Mutation m1 -Log mut-m1.log
```

---

## 9 · 给 BA 的一句话

**可以进 PR 了。** B-1 是这一轮最要紧的那条，我用 OWL 语义自己判了 52 条（不是看计数），方向对了；B-3 的 104 条值域我逐条回到列注释里找过，没有编。留 4 条非阻断：**1:1 欠约束（宁少勿多，别现在改）**、**测试绕行方式要更正文档**、**客户视图不能当查询入口**、**建模视图离 40% 只剩 1 个点**。
