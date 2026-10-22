# REQ-120 · 关系推断改进（FK 边补全）— Pre-merge QA 审核

> **结论：PASS（带非阻断项）**
> 交付物（TTL / 快照）与 Dev 报的每一组数字都能复现；F-1 的安全闸在**生成侧**真实生效（A/B 两次跑法对比见 §1）；
> 未签提案确实一条都没进快照（§3）；抽样的 `true` 证据逐条回源库复算全部对上（§4）。
> 无阻断项。有 1 条安全加固建议（签字侧不复核租户键，§1.5，P1）和若干文档/口径订正（§9）。

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-120`（分支 `feature/REQ-120`，HEAD `49456b1`，5 个提交） |
| 基线 | `develop` `bf5f89f` |
| 源库（只读） | `postgresql://…@172.16.120.20:30432/anspire_open_uat` |
| 本体库 | `postgresql://…@172.17.0.87:5432/anspire_real`（`anspire_fk120_v6` v2 已冻结） |
| 主产物 | `feature-REQ-120/out/anspire-req120-model.ttl`（423,261 B）/ `anspire-req120-customer.ttl`（312,889 B） |
| 审核方式 | 只读审核 + 独立 worktree 变异（未改 `feature/REQ-120`、未 push）+ 端到端重跑 + 回源库复算 |
| 审核日期 | 2026-10-23 |

---

## 0 · 一句话结论

**F-1 的取舍成立**：`api_call_logs__customers` 在最终 TTL 里确实不存在（正反两侧都没有），
无 tenancy 时会作为 `inherit` 出现、有 tenancy 时被拦在提案之前——闸是「活的」，不是摆设。
**但闸只装在生成侧**：带 `--tenancy` 也无法阻止一份手工/旧的签字产物把这条租户边注入 TTL（§1.5）。

---

## 1 · F-1 安全闸（最要紧）

### 1.1 交付物事实（`anspire_fk120_v6` v2 导出的建模视图）

| 检查项 | 结果 |
| --- | --- |
| `:api_call_logs__customers` | **不存在**（只有 `:api_call_logs__customer_id a owl:DatatypeProperty`，不是对象属性） |
| `:customers__api_call_logs`（反向） | **不存在**（不是「变相保留」） |
| `:api_call_logs__licenses` / `:licenses__api_call_logs` | 存在，两侧均 `ontok:joinVerified "inherit"`，证据 `inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)` |
| 全库 158 块对象属性 | `true` 144 / `inherit` 2 / `false` 12，无第四种状态 |
| 反向一致性 | 79 对边，`inverseOf` 全部成对；正反两侧 `joinVerified` **0 处不一致** |

TTL 里**没有**任何对象属性指向 `api_call_logs.customer_id`。`ontok:tenantKey` 只标在
DatatypeProperty 上（数据列），不被当成关系。

### 1.2 A/B 两种跑法（我重跑的，不是引用 Dev 的日志）

同一份 schema / 同一份翻译表 / 同一源库，唯一差异是 `--tenancy tenancy.json`。
两次都**把全部提案签满**（签同一策略），这样 TTL 差异只能来自闸本身。

| 项 | A：给 `--tenancy` | B：不给 `--tenancy` |
| --- | --- | --- |
| stderr 警告 | 无 | **有**（见下，`eprintln` 实测只进 stderr） |
| 待判列 | 144 | 144 |
| 候选 / 实测 | 673 / 427 | 673 / 427 |
| 补出关系 | **91**（strong 72 · medium 1 · inherit 18） | **107**（strong 72 · medium 1 · **inherit 34**） |
| `inherit_tenant_key_skipped` | **16** | 0 |
| 未解析 | 53（insufficient_sample 24） | 37（insufficient_sample 8） |
| 签满后 TTL 关系数 | 94（91 签 + 3 条既有 `false`） | 107 |
| TTL 三态块数 | true 146 / inherit 36 / false 6 | true 146 / **inherit 68** / false 0 |
| `api_call_logs__customers` | **不存在** | **存在**，`joinVerified "inherit"` |
| `customers__api_call_logs` | **不存在** | **存在**，`joinVerified "inherit"` |

B 的租户边原文（我的重跑产物）：

```turtle
:api_call_logs__customers a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:domain :api_call_logs ; rdfs:range :customers ;
    ontok:cardinality "N:1" ;
    ontok:joinVerified "inherit" ;
    ontok:joinEvidence "inherited from agent_white_list.customer_id (verified 5/5) -> customers.id via primary_key (plural)" ;
    owl:inverseOf :customers__api_call_logs .
```

stderr 警告原文（实测只出现在 stderr，stdout 无）：

```
⚠ 未声明租户键（没给 --tenancy）：**继承闸对所有人放行** —— 租户键列会被允许 inherit。
  租户边界关系到安全，正式跑请给 --tenancy。
```

**16 列差异逐条核对**：B 比 A 多出的 16 条 inherit，**全部**落在 `tenancy.json` 声明的
8 个列名里，没有一条误伤非租户列，也没有一条租户列漏拦：

```
api_call_logs.customer_id            customer_tpm_rate_limit.customer_id
deductions.customer_id               deductions.main_customer_id
family_high_risk_records.main_customer_id   file_parser_task.customer_id
frozen_assets.customer_id            frozen_assets.main_customer_id
internal_messages.related_customer_id       internal_operation_records.related_customer_id
internal_staff.customer_id           ip_whitelist.customer_id
proxy_operation_auths.customer_id    refund_operations.customer_id
sub_account_quota.main_customer_id   sub_account_quota.sub_customer_id
```

声明覆盖面核对：源 schema 里带 `customer` 的列名共 8 类（`customer_id` 37 表、`main_customer_id` 7 表、
`sub_customer_id` 4 表、`related_customer_id` 3 表、`from/to/source/target_customer_id` 各 1 表），
`tenancy.json` 全部覆盖；schema 里没有 `tenant_id` / `org_id` 一类未声明的租户键。
（声明文件里的 `tables` 字段被 `ontok-propose` 忽略、只取 `columns`——方向是**更保守**，不是更松，见 `src/bin/ontok-propose.rs:579`。）

fail-closed 也验了：`--fk-out` 缺 `--src` ⇒ 退出码 1；`--tenancy` 传坏 JSON ⇒ 退出码 1（不静默当成「没有租户键」）。

### 1.3 闸的实现位置与「三层」事实

| 层 | 位置 | 作用 |
| --- | --- | --- |
| ① 生成 | `src/extract/fk.rs:1650` `plan_inherit` 首闸 | 租户列直接 `Skip(TenantKey)`，连「同名列验过没有」都不看 |
| ② 计数 | `src/extract/fk.rs:1461` | `inherit_tenant_key_skipped` 写进报告，**不静默** |
| ③ 路由 | REQ-108（既有） | 0 行表的命名推断边 `all_null` 否决 → 低档 → 待查清单，本来就不进主提案 |

因此最终 TTL 里「不存在」是 ①+③ 共同结果，**F-1 只负责掐掉那条唯一能带 `verified/inherit` 标记的路**
（设计说明 §一·八 的交付口径 1–3 说的是同一件事，表述成立）。
`api_call_logs → customers` 的命名边仍在 `out/proposal-pending.json` 里（`join_verified: "false"`，
veto `all_null`，score 0.4）——**没丢，只是没进快照**，人若要认，走「待查 → 人工接受」的正规路径。

### 1.4 「缺一条边」vs「标 false」——这个取舍成立

我按「谁消费这份 TTL」来判断：

1. **机器消费（数字员工 / SPARQL）**：`joinVerified "false"` 仍然是一个 `owl:ObjectProperty`。
   `false` 只是标签，**不阻止 join**——REQ-119 复跑里 Q1 踩的正是 `deductions__customers` 那条 `false` 边。
   对租户键、且源表 **0 行**（永远拿不到本表实测）的列，留一条可 join 的边就是在给跨租户查询铺路。
2. **人消费（评审）**：`false` + `insufficient_sample` 能让人判断真假；但这个信息在
   `proposal-pending.json` 里**已经完整存在**，不需要在 TTL 里再留一个可被误用的对象属性。
3. **可恢复性**：撤销 ≠ 删除。待查清单保留了完整提议，人工接受仍可走正规路径。

⇒ **「缺边」对默认消费方更安全，且没有牺牲人的可评审性，取舍成立。**
唯一代价是**可见性**：只读 TTL 的人看不出「这里被人为撤掉了一条边」。建议在 TTL 文件头或
验收报告里列出「因 F-1 撤销的租户键边」（见 §9 N-8），非阻断。

### 1.5 ⚠ 额外发现（P1，建议合并前修）：签字侧不复核租户键

`--tenancy` 只接在 `verify()` / `plan_inherit` 上；`load_signed`（`src/extract/fk.rs:1825`）
**不接收** `tenant_columns`。我用一条实验证明了后果：

1. 从**无 tenancy** 的跑法里取 `api_call_logs.customer_id` 的 inherit 提案（0.75），只把它标成已签；
2. 用**带 `--tenancy tenancy.json`** 的命令跑 `--fk-sign`：
   ```
   注入关系    : 0 条新增 + 1 条既有边补验证标记（未签 0 条已忽略）
   保留已签字的边: 1 条
   ```
3. import → apply → freeze → export 后，TTL 里出现：
   ```turtle
   :api_call_logs__customers … ontok:joinVerified "inherit" ;
       ontok:joinEvidence "inherited from agent_white_list.customer_id (verified 5/5) -> customers.id via primary_key (plural)" .
   ```

即：**带 tenancy、无任何警告，租户键 inherit 仍能从签字产物进入快照。**
正常「生成 → 签 → 注入」链路在 F-1 之后产生不出这条提案，所以本次交付物是安全的；
但一份 **F-1 之前留下的签字文件**、或从无 tenancy 跑法签出来的文件，会静默把边带回来。

- 设计说明 §九.11 把 `--tenancy` 明确限定为「只影响 FK 阶段的继承闸」，**属于已披露的范围**，
  所以我判**非阻断**。
- 但既然 F-1 的立论是「租户边界关系到安全，必须本表实测」，租户闸不宜被一个签名绕过。
  最小修法（两处之一）：
  - `load_signed` 增加第 6 道闸：`state == Inherited && tenant_columns.contains(from_column)` ⇒ **拒绝整份文件**（fail-closed，与既有五道闸同风格）；或
  - 签字侧至少 eprintln 警告「签字产物不参与租户闸复核」，别让 `--tenancy` 产生「已复核」的错觉。
- **升级条件**：若安全负责人要求「任何输入都不能让这条边进快照」，则本条升级为阻断。

---

## 2 · F-2：每条 `false` 边都有「为什么没验」

### 2.1 交付物统计（建模视图，158 块）

| 前缀 | 块数 | 边数 | 边 |
| --- | --- | --- | --- |
| `insufficient_sample:` | 6 | 3 | `deductions`（4 行/1 个不同值）· `frozen_assets`（窗口 25 行/1 个不同值）· `customer_tpm_rate_limit`（1 行/1 个不同值） |
| `unsigned:` | 6 | 3 | `deductions → licenses`（inherit 0.750 未签）· `order → customer_subscriptions`（inherit 0.750 未签）· `subscription_priority → customer_subscriptions`（medium 0.767 未签） |
| `naming-only:` | 0 | 0 | 本库确实没有「既没进抽样、又没提案」的 false 边 |
| **合计** | **12** | **6（3 对）** | 与 Dev 报的 3+3 完全一致 |

- `joinVerified "false"` 块数 **12**，带 `ontok:joinEvidence` 的 **12**，空串/缺失 **0**。
- **`deductions__customers` 正反两条都写了**（抽查原文）：
  ```turtle
  :deductions__customers … ontok:joinVerified "false" ;
      ontok:joinEvidence "insufficient_sample: 有效样本 4 行 / 1 个不同值 < 3 条下限（跳过 NULL 之后）" ;
      owl:inverseOf :customers__deductions .
  :customers__deductions … ontok:joinVerified "false" ;
      ontok:joinEvidence "insufficient_sample: 有效样本 4 行 / 1 个不同值 < 3 条下限（跳过 NULL 之后）" ;
      owl:inverseOf :deductions__customers .
  ```
- 三条 `insufficient_sample` 的**实测数字与源库一致**（我自己回源库取的）：
  `deductions` 4 行/1 个不同值；`frozen_assets` 窗口 25 行/1 个不同值（全表 1804 行）；
  `customer_tpm_rate_limit` 1 行/1 个不同值；`api_call_logs` 0 行（`all_null`）。

### 2.2 变异：删掉理由写入 ⇒ 测试必须红

在独立 worktree 里删掉 `stamp_naming_only` 的 `p.insert("join_evidence", …)`（`src/extract/fk.rs:2236`）：

```
test result: FAILED. 411 passed; 1 failed
---- extract::fk::tests::stamping_writes_why_nothing_was_verified stdout ----
panicked at src/extract/fk.rs:3084: called `Option::unwrap()` on a `None` value
```

**被抓到了**，F-2 不是「写了但没测」。变异已还原（`git checkout`）。

### 2.3 口径问题（非阻断，见 §9 N-5）

任务书给的前缀集合是 `{insufficient_sample, unsigned, naming-only}`，而 `stamp_naming_only`
的理由表还会原样透传 `unresolved[]` 的 code——理论上会出现 `low_overlap:` /
`no_candidate:` / `sample_failed:` 的 false 边。本产物里一条都没有（0/12），所以**交付物合规**；
但这条不变量在代码里没有被约束，建议要么把文档集合扩成五类，要么在盖章时把非三类映射成
`naming-only:` 或显式拒绝。

---

## 3 · `retain_signed` 是否被滥用

### 3.1 未签提案确实没进快照（用**冻结快照数据库**逐条对）

`fk-signed.json` / `fk-proposals.json` ↔ `anspire_fk120_v6` 的 `link_types`
（按 `(from_type, to_type, evidence[0].column)` 精确匹配）：

| 项 | 数量 |
| --- | --- |
| 提案总数 | 91（strong 72 · medium 1 · inherit 18） |
| 已签（`reviewed_by` 非空） | **73**（72 strong + 1 inherit） |
| 未签 | **18**（17 inherit + 1 medium） |
| 73 条已签 → 出现在 v6 | **73 / 73**（无一条丢失） |
| 18 条未签 → 出现在 v6 且 `join_verified ∈ {true, inherit}` | **0 条（无泄漏）** |
| 18 条未签的实际去向 | 15 条**完全不在**快照；3 条以既有命名边的身份存在，状态是 **`false`** |

那 3 条「以 false 存在」的是：`deductions.license_id`、`order.subscription_id`、
`subscription_priority.subscription_id`——它们本来就是老规则推出来的命名边，未签提案只贡献了
`unsigned:` 的理由文本，**没有新增边、没有升级状态**。

> 任务书里「91 条提案里只代签 74、33 条不签」这组数字对不上最终产物：**74/33 是无 tenancy
> 跑法（107 条提案）的口径**；F-1 之后是 **91 = 73 + 18**。我已按最终签字文件核对（见上表），
> 结论不变：未签的一条都没进快照。

### 3.2 `retain_signed` 捞回来的到底是什么

v6 的 79 条边里，REQ-108 评分 **< 0.5（低档）的恰好 5 条**，全部是签字边：

| 边 | score | `join_verified` |
| --- | --- | --- |
| `llm_provider_groups → llm_model` | 0.2 | true |
| `llm_model_sort → llm_model` | 0.2 | true |
| `llm_model_peak_config → llm_model` | 0.2 | true |
| `llm_provider → llm_model` | 0.2 | true |
| `api_call_logs → licenses` | 0.4 | inherit |

与 Dev 报的「保留已签字的边: 5 条」逐条一致；**没有一条未签边被捞进来**。
（其余 74 条边的 score ≥ 0.5，本来就在主提案里，不需要捞。）

### 3.3 变异：让 `retain_signed` 把未签的也捞进来 ⇒ 测试红

把 `src/extract/fk.rs:2263` 的判据加上 `|| s == JoinState::NamingOnly.as_str()`：

```
test result: FAILED. 411 passed; 1 failed
---- extract::fk::tests::signed_edges_survive_confidence_routing stdout ----
assertion `left == right` failed: 两条签过的边被捞回来，命名推断的留在待查
  left: 3
 right: 2
```

**有测试盯这个，不是缺口。** 变异已还原。

### 3.4 更严一层：`retain_signed` 的输入只可能来自 `inject_signed`

- `inject_signed` 只对 `load_signed` 里 `reviewed_by` 非空、且重新过阈值/证据闸的边写
  `true/inherit`（`src/extract/fk.rs:2011`）。
- `stamp_naming_only` 只写 `false`。
- 因此 `retain_signed` 的判据（`join_verified ∈ {true, inherit}`）与「已签字」等价，
  在当前代码里不存在「未签但带 true/inherit」的条目。

---

## 4 · 证据是否真实（回源库复算）

复算口径与实现完全一致（`src/extract/sample.rs:110/129`）：
`SELECT (col)::text … WHERE col IS NOT NULL ORDER BY 源表主键 LIMIT 25` → 去重保序取前 5 个值 →
`SELECT count(*) FROM 目标表 WHERE (目标列)::text = ANY(样本)`。

| 边（`joinVerified "true"`） | TTL 声明 | 我复算 | 窗口 | 判定 |
| --- | --- | --- | --- | --- |
| `admin_user_roles.admin_user_id → admin_users.id` | 5/5 (100%) | **5/5** | 5 行 / 5 个不同值 | ✅ |
| `llm_provider.model_id → llm_model.model_id`（E-1 唯一列） | 5/5 (100%) | **5/5** | 25 行 / 16 个不同值 | ✅ |
| `usage_quotas.product_id → package_products.id` | 5/5 (100%) | **5/5** | 24 行 / 10 个不同值 | ✅ |

**`inherit` 抽 1 条**：`api_call_logs__licenses` 写的是
`inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)`。

- 我真去 `license_quota.license_id` 复算：**5/5**（窗口 8 行 / 8 个不同值），命中 `licenses.id`。
- 该来源在 TTL 里自己标的是 **`true`**（`:license_quota__licenses`），不是 inherit 链。
- `license_id` 不是 `tenancy.json` 声明的租户列，F-1 不拦它——**允许**。
- 设计说明引用的另一条来源 `usage_details.license_id` 我也复算：**4/4**（窗口 25 行 / 4 个不同值）。

**F-2 的数字也回源核对过**（不是只读 TTL）：`deductions` 4 行、`api_call_logs` 0 行、
`usage_details` 216 行、`balances` 17935 行、`customer_tpm_rate_limit` 1 行、`frozen_assets` 1804 行
——与验收 §四「数据真相」一致。

> 赞同 F-4 的判断：这些 `5/5`、`4/4` 只说明「抽了 5 行没翻车」，不是统计保证。

---

## 5 · 三态计数与双标一致性

| 检查项 | Dev 报 | 我核 | 结论 |
| --- | --- | --- | --- |
| `joinVerified "true"` 块 | 144 | **144** | ✅ |
| `joinVerified "inherit"` 块 | 2 | **2** | ✅ |
| `joinVerified "false"` 块 | 12 | **12** | ✅ |
| 对象属性块总数 | 158 | **158**（79 对 × 2） | ✅ |
| `joinEvidence` 块数 | 158（每条边都有） | **158**（缺失 0） | ✅ |
| 正反两侧 `joinVerified` 一致 | — | **0 处不一致**（79 对全查） | ✅ |
| `inverseOf` 成对 | — | 158 块全部有 `owl:inverseOf`，79 对 | ✅ |
| 反向 domain/range 对调 | — | **0 处未对调** | ✅ |
| 重复主语 | 0 | **0** | ✅ |
| 状态是**字符串**不是裸布尔 | 三态字符串 | 全部带引号；无 `joinVerified true` 裸布尔 | ✅ |
| 每条边都有状态 | — | 158/158，无「省略」 | ✅ |

证据的四种实际形态：`value-overlap` 144 块、`inherited from` 2 块、
`insufficient_sample:` 6 块、`unsigned:` 6 块。**`joinEvidence` 里没有任何样本原值**
（无 UUID/模型名/邮箱等泄漏），符合 REQ-108「不落原值」的延续。

客户视图（`anspire-req120-customer.ttl`）里 `joinVerified` / `joinEvidence` / `tenantKey` 均为 **0**——
建模元数据没有漏到客户视图，这一点也成立。

---

## 6 · 1:1 与基数（REQ-119 N-1 的回回归）

### 6.1 REQ-119 QA 点名的 6 条：全部仍是 1:1 双标

| 关系 | REQ-119 | 现在 | 依据（`schema.json` 的 `unique_columns`） | 样本唯一 |
| --- | --- | --- | --- | --- |
| `balances.customer_id → customers` | N:1 | **1:1** | 含 `customer_id` | 25 行 / 25 个不同值 |
| `customer_rate_limit.customer_id → customers` | N:1 | **1:1** | 含 `customer_id` | 10/10 |
| `customer_tpm_rate_limit.customer_id → customers` | N:1 | **1:1** | 含 `customer_id` | 未采样（`false` 边，按 DB 约束提基数） |
| `dim_customer_binding.customer_id → customers` | N:1 | **1:1** | 含 `customer_id` | 25/25 |
| `license_quota.license_id → licenses` | N:1 | **1:1** | 含 `license_id` | 8/8 |
| `service_apps.channel_id → marketing_channel_customer` | N:1 | **1:1** | 含 `channel_id` | 7/7 |

### 6.2 新补的边里 1:1 有几条、依据是什么

TTL 里 1:1 共 **16 块 = 8 对**（正反都是 `owl:FunctionalProperty + owl:InverseFunctionalProperty`
双标，我逐块验过）。除上面 6 条外还有 2 条：

| 关系 | 依据 | 说明 |
| --- | --- | --- |
| `llm_model_peak_config.model_id → llm_model.model_id` | 唯一列 + 样本唯一（4/4） | E-1 的 join 目标是 `model_id`（unique），不是 `id` |
| `sub_account_permission.sub_customer_id → customers` | 唯一列 + 样本唯一（25/25） | 同一条 R4 规则命中，不是白名单 |

判据是「**窗口内去重无重复 且 源列在 `unique_columns` 里**」，方向对（宁少勿多）：
N:1 被误说成 1:1 会让开推理的一方错误合并个体，所以 1:1 需要两个证据同时成立。
`customer_tpm_rate_limit` 那条是例外：它没有采样（`false`），1:1 由**数据库唯一约束**给出——
约束比抽样强，这个方向可接受，但读者要能看出「基数是约束给的、join 目标是未验的」。

### 6.3 ⚠ 设计说明 §五 的第 8 条写错了（非阻断）

设计说明说「另有 1 条同规则命中：`internal_staff.customer_id`、`llm_model_peak_config.model_id`」。
实测：`internal_staff.customer_id` 确实被提升成 1:1 了，但那条边随后被 REQ-108
路由进**待查清单**（`pending-signed.json`，`cardinality: "1:1"`, `join_verified: false`），
**不在 TTL 里**；TTL 里真正的第 8 条是 `sub_account_permission.sub_customer_id`。
运行日志把它列进「基数修正」没有错（修正发生在路由前），但设计说明把它当成「已进快照」的
第 8 条是错的。建议订正。

---

## 7 · 不退化

### 7.1 测试数字（我自己跑的，带 `ONTOK_TEST_DATABASE_URL`）

| 套件 | 期望 | 实测 |
| --- | --- | --- |
| `cargo test --lib` | 412 | **412 passed / 0 failed** |
| `cargo test --doc` | 4 | **4 passed** |
| `cargo test --test test_req120_fk` | 11 | **11 passed** |
| 全量 `cargo test --no-fail-fast` | — | **1329 passed / 5 failed / 1 ignored**（88 个 target） |

> Dev 报「全量 1326 / 5」是 **rev.3 之前的数**：rev.3 新增了 3 条 lib 测试
> （`inherit_is_forbidden_for_tenant_key_columns` / `stamping_writes_why_nothing_was_verified` /
> `signed_file_carries_the_reasons_for_unverified_edges`），lib 409 → 412，总数因此 1326 → 1329。
> 失败的 5 条没有任何变化。
> 那 1 条 `ignored` 是既有的 `tests/test_mcp_server.rs:556`（「缺口清单，不是可运行断言」），
> **不是本 REQ 新增**：`git diff bf5f89f` 里零新增 `#[ignore]`。

### 7.2 5 条失败逐条归因（**在基线 `bf5f89f` 上复现，证明全既有**）

我建了一个**只打 preflight 行号 shim** 的 `bf5f89f` worktree（除此之外零改动），跑同样 4 个套件：

| # | 用例 | 失败原因 | 基线 `bf5f89f` |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | 读不到 `../.review-workspace/fix-props/schema.json`（该文件不在盘上；用例设计为 fail-closed） | **同样失败** |
| 2 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 同一份缺失脱敏 schema | **同样失败** |
| 3 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 无 `--src` 时 REQ-108 分档把唯一关系送进待查，CLI 的 items 比 `extract()` 少一条（REQ-116 manifest 已登记） | **同样失败** |
| 4 | `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | `propose.rs` 的行号登记表在 develop 上就过期（本 REQ 未改 `propose.rs`） | **同样失败** |
| 5 | `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | 同上（`concept.rs` 9 处 vs 登记 5 处；本 REQ 未改 `concept.rs`） | **同样失败** |

基线 4 个套件的输出：`drop_rate 0/1`、`test_agent_orchestration 35 passed/1 failed`、
`test_fk_naming 5/1`、`test_retained_names_req084 4/2` —— **与 feature 完全同一批 5 条**。
Dev 的「5 条全既有」**成立**。

### 7.3 红线 / 禁改文件

`git diff bf5f89f` 的 23 个文件里**没有**：

- `Cargo.toml` / `Cargo.lock`（零新依赖）
- `src/mcp/tools.rs`
- `src/mcp/property_filter.rs`
- `src/api/**`（鉴权与路由）
- `tests/test_req107_red_lines.rs`（四红线，全量里 **2 passed**）
- 任何 `fn guard()`（diff 里零命中）

**既有** `tests/*` 的改动只有两类：给结构体字面量补 `join_verified/join_evidence/column` 字段
（编译所需，**未放宽任何断言**），以及 `test_req107_source_enumeration.rs` 的模块登记表加一行
`("src/extract/mod.rs", "fk")`（该表的设计就是「新增模块即红」，补登记是正确动作，全量里
`test_req107_source_enumeration` **10 passed**）。

### 7.4 确定性

| 项 | 结果 |
| --- | --- |
| 我跑两次 `--fk-out`：`fk-proposals.json` | sha256 `7116161b1d3b6d66…` **两次相同** |
| 我跑两次 `--fk-out`：`fk-unresolved.json` | sha256 `943939f8bb762831…` **两次相同** |
| 与 Dev 的 `fk-proposals.json` / `fk-proposals-A.json` | **逐字节相同**（同一对 hash） |
| 同一冻结版本导出两次建模视图 TTL | sha256 `9ce7f7a2…` **两次相同** |
| Dev 的 `out/anspire-req120-model.ttl` vs 我重新导出 v6 | sha256 `fed694f9…` **逐字节相同** |
| 用 Dev 的 `fk-signed.json` 重跑整条签字流水线（新项目），TTL 前缀归一后 | **逐字符相同**（402,694 chars） |

### 7.5 preflight 绕行（⚠ 设计说明只报了 2 条，实际 4 条）

- 文件行数 `2758 → 2758` **不变**，`retained_names.req084.tsv` 的行号判据仍然有效；`⛔` 没有挪文件。
- 实际被 `#[cfg(unix)]` 跳过的是 **4 条**，不是设计说明说的 2 条：
  `default_roles_are_two_different_files`、`resolve_roles_rejects_alias_of_baseline`、
  `resolve_roles_rejects_hard_link_of_baseline`、`real_pair_five_missing_columns_are_named`。
- 我逐条判断过：后 3 条分别依赖 POSIX 符号链接 / `(st_dev, st_ino)` 硬链接语义 /
  `/home/...` 下的实测样本，Windows 上本就不成立；第 1 条我也**实测**过——
  去掉 `#[cfg(unix)]` 后它在 Windows 上因「两个门禁的 POSIX 缺省路径解析到不同文件」而失败，
  属于环境问题，跳过是合理的。**但设计说明应改成 4 条**（非阻断）。

---

## 8 · 性能与体积

| 项 | Dev 报 | 我实测 | 结论 |
| --- | --- | --- | --- |
| FK 阶段 | 42.5 s / 41.9 s | **≈ 43.1 s**（完整跑 55.9 s − 同口径纯抽取 12.8 s） | ✅ < 3 min |
| 完整抽取（含 FK + 置信度采样） | 103.8 s | **55.9 s / 56.0 s**（两次） | ✅ < 3 min（比 Dev 报的更快） |
| 签字轮（`--fk-sign`，无 FK 采样） | 16.1 s | **15.8 s / 17.0 s** | ✅ |
| 建模视图 TTL | 423,261 B | **423,261 B**（逐字节） | ✅ |
| 未签对照建模视图 | 379,417 B | **379,417 B** | ✅ |
| 建模视图增量 | +11.6% | **+11.56%** | ✅ |
| 客户视图 TTL | 312,889 B | **312,889 B** | ✅ |
| 客户视图增量 | +5.9% | **+5.87%** | ✅ |

⚠ 口径提醒（非阻断）：`anspire-req120-unsigned-model.ttl` 生成于 **23:41:56**，早于 rev.3，
**不含 F-2 的 `joinEvidence`**；而签名版有 158 条证据。所以 **+11.56% 里含了证据文本的开销**，
不是纯关系增长的比值。客户视图两版都没有 `joinEvidence`，**+5.87% 是干净口径**。
建议下次体积对比用**同一提交**导出的未签版本。

---

## 9 · 阻断项 / 非阻断项

### 阻断项（0）

按 11 条 AC + §三 E-1/E-2 + §四 F-1/F-2 的文本，**没有必须阻断合并的问题**。
交付 TTL 的 F-1 / F-2 / 三态 / 双标 / 1:1 / 未签不进快照 / 确定性 / 无退化 / 性能体积，
逐项可复现且与 Dev 报的一致。

### 非阻断项（按优先级）

| # | 级别 | 项 | 证据 / 建议 |
| --- | --- | --- | --- |
| **N-1** | **P1（安全加固，建议合并前修）** | 签字侧不复核租户键：带 `--tenancy` 也能把一条租户 inherit 从签字文件注入 TTL，且**无警告** | §1.5；最小修法：`load_signed` 增加租户闸（`Inherited && tenant_columns.contains(from_column)` ⇒ 拒绝整份文件），或至少 stderr 明说「签字产物不参与租户闸复核」。设计说明 §九.11 已披露「只影响 FK 阶段」，故不阻断；若安全口径要求全路径不可绕过，升级为阻断 |
| **N-2** | P2（测试缺口） | F-1 的 **CLI 接线**没有自动化测试：`plan_inherit` 的租户闸有单测（变异必红），但 `--tenancy → verify(tenant_columns) → inherit_tenant_key_skipped` 这条链路**没有任何测试**；`inherit_tenant_key_skipped` 在测试里 0 次引用，`verify()` 只有 CLI 一个调用方 | 建议加一条集成测试：给非空租户声明跑 `verify`，断言 `inherit_tenant_key_skipped > 0` 且不产出 tenant inherit。否则把 `--tenancy` 从 `verify` 调用里摘掉，全部测试仍绿 |
| **N-3** | P2（文档缺失） | **F-4 未落地**：验收要求「`true` 是抽样证据、不是全量保证」写进 **TTL 文件头与文档**；实际 TTL 头部（两版）与 `design-notes` 里都**没有**这句话（`翻车/统计保证/全量保证` 命中 0） | 在 TTL 文件头的「有损声明」里加一行，并在 design-notes 的 §六 补一句即可 |
| **N-4** | P2（文档订正） | 设计说明数字/归属有 4 处不准：① §五 第 8 条 1:1 写成 `internal_staff.customer_id`，它其实在`待查`里、不在 TTL，真正的第 8 条是 `sub_account_permission.sub_customer_id`；② §九.6「本轮 34 条 inherit 只签了 2 条、其余 32 条」是 F-1 之前的数，实际 **18 条、签 1、未签 17**；③ §十「全量 1326」是 rev.3 之前的数（rev.3 +3 条 lib 测试 ⇒ 1329）；④ §十说 preflight 跳过 2 条，实际 **4 条** | 都是表述订正，不影响产物 |
| **N-5** | P2（规格歧义） | F-2 的前缀集合在任务书里是三类，实现会把 `unresolved[].code` 原样透传，理论上能出 `low_overlap:` / `no_candidate:` / `sample_failed:` 的 false 边（本产物 0 条） | 要么把文档集合扩成五类（设计说明 §一·八 的表其实已经这么写），要么在 `stamp_naming_only` 里把非三类显式映射/拒绝——现在「文档说三类、代码允五类」 |
| **N-6** | P3（口径） | `+11.56%` 的未签基线是 rev.3 之前的产物（无 `joinEvidence`），增量里含证据文本开销；客户视图 `+5.87%` 是干净口径 | 下次用同一提交导出的未签版本对比 |
| **N-7** | P3（措辞） | `false` 证据里的「有效样本 25 行」是**取样窗口**（≤25），不是表行数（`frozen_assets` 实际 1804 行）。设计说明 §九.12 解释了，但 TTL 读者看不到 | 建议写成「取样窗口 25 行」 |
| **N-8** | P3（可见性） | F-1 撤销的边只在 `proposal-pending.json` 里可见；只读 TTL 的人看不出「这里被拿掉了一条边」 | 建议在 TTL 文件头/验收报告列一张「因 F-1 撤销的租户键边」表（哪怕 1 行） |

### 我判「不构成问题」的几处（避免后续误报）

- `customer_tpm_rate_limit__customers` 是 **1:1 且 `false`**：1:1 由数据库唯一约束给出（约束强于抽样），
  `false` 说的是「join 目标没实测」——两件事不矛盾，证据里也写了 `insufficient_sample`。
- `api_call_logs` 只有 1 条边（`licenses`，inherit）：BA §四 已裁定这是「0 行表的唯一出路」，
  `license_id` 不是租户键，合规。
- `deductions.customer_id` 实际 1/1 命中 `customers.id`（单值），却因「不同值 < 3」判
  `insufficient_sample`：这是 AC-002 的保守规则 + F-1 的租户闸共同结果，**不是误判**，
  理由也写清了；若要认，走待查 → 人工接受。
- 客户视图里 `joinVerified` 为 0：不是漏标，是客户视图结构上不含建模元数据（REQ-119 R4 的延续），
  且 `anspire-req120-customer.ttl` 的 150 块对象属性没有泄漏 `tenantKey`。

---

## 附录 A · 复现命令（我实际跑的）

```powershell
# 环境
$env:ONTOK_DATABASE_URL = 'postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
$env:NO_COLOR = '1'
$src = 'postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat'

# A：有 tenancy（生成提案）
.\feature-REQ-120\target\debug\ontok-propose.exe `
  --schema code\out\schema.json --names code\out\table_names.json `
  --out <qa>\f1a\proposal.json --stats <qa>\f1a\stats.json `
  --project anspire_qa120_f1a --round 1 --src $src `
  --fk-out <qa>\f1a\fk-proposals.json --tenancy feature-REQ-120\tenancy.json

# B：无 tenancy（唯一差异：不传 --tenancy）
.\feature-REQ-120\target\debug\ontok-propose.exe ... --fk-out <qa>\f1b\fk-proposals.json

# 签满 + 注入 + 落库 + 导出（A/B 各一次）
.\feature-REQ-120\target\debug\ontok-propose.exe ... --fk-sign <qa>\f1x\fk-signed-all.json
.\feature-REQ-120\target\debug\ontok.exe project new --name anspire_qa120_f1x
.\feature-REQ-120\target\debug\ontok.exe version new --project anspire_qa120_f1x
.\feature-REQ-120\target\debug\ontok.exe propose import --file <qa>\f1x\proposal-signed.json
.\feature-REQ-120\target\debug\ontok.exe review decide --proposal p1 --accept-all
.\feature-REQ-120\target\debug\ontok.exe proposal apply --proposal p1
.\feature-REQ-120\target\debug\ontok.exe version freeze --project anspire_qa120_f1x --version 2
.\feature-REQ-120\target\debug\ontok.exe ontology export --project anspire_qa120_f1x --version 2 `
  --model-view --format ttl --out <qa>\f1x\model.ttl --tenancy feature-REQ-120\tenancy.json `
  --schema code\out\schema.json
```

实验用的 4 个项目（`anspire_qa120_f1a/f1b/repro/tenant`）**已在审核结束时从本地库删除**，
`anspire_fk120_v6` 未被触碰（79 条边、已冻结，复导 sha256 与交付 TTL 相同）。

## 附录 B · AC 对照（简表）

| AC | 结论 | 依据 |
| --- | --- | --- |
| AC-001 提案形状 | ✅ | 91/91 含 claim/evidence/confidence/reason/decided_by/reviewed_by + facts；reviewed_by 全 null |
| AC-002 值域硬证据 | ✅ | 阈值常量 + 22 条 rejected（如 `external_task_id` 0/5）+ 单测 |
| AC-003 5 个孤儿类 | ✅ | `package_products` 4 边、`llm_model` 4 边（unique 目标）、`customer_account_binding` 3 边、`service_config` 0 边（同名列无一处实测）、`api_call_logs` licenses=inherit/customers=撤销 |
| AC-004 1:1 按值唯一性 | ✅ | 16 块 1:1，双标 + 反向同步；8 条源列均在 `unique_columns` |
| AC-005 未解析清单 | ✅ | 53 列，全部带 code+reason，无空理由；`stats.close()` 闭合 |
| AC-006 未签不进快照 | ✅ | 18 条未签 → 0 泄漏（15 缺席 + 3 条既有 false） |
| AC-007 TTL 带证据 | ✅ | 158/158 有 `joinEvidence`；三态可区分 |
| AC-008 不臆造 | ✅ | 22 条否决 + 单测覆盖「无候选/无 PK/低命中」 |
| AC-009 确定性 + 性能 | ✅ | 两次 sha256 相同；FK ≈43 s，全程 < 3 min |
| AC-010 不退化 / 零依赖 | ✅ | 412/4/11；全量 5 条失败全部基线复现；Cargo/红线 diff 为空 |
| AC-011 替身实验 | ⚠ 部分 | Q3/Q5 由 true 边支撑；Q4 仍是 `customer → licenses → api_call_logs` 两跳（`api_call_logs__licenses` 为 inherit），未达「直查」——BA §四 已裁定「降级」，非本轮新增缺陷 |
| §三 E-1 唯一列目标 | ✅ | 8 块 `via unique_column`（llm_model.model_id），复算 5/5 |
| §三 E-2 inherit 三态 | ✅ | 1 条 inherit（非租户）；confidence 0.75 ∈ [0.60,0.75]；必须人签；来源 5/5 且自身 true |
| §四 F-1 租户键禁 inherit | ✅（生成侧） | A/B 对比见 §1；签字侧见 N-1 |
| §四 F-2 false 必写原因 | ✅ | 12/12，三类前缀；正反都写；变异必红 |

## 附录 C · 产物与 hash

| 文件 | 大小 | sha256 |
| --- | --- | --- |
| `out/anspire-req120-model.ttl` | 423,261 B | `fed694f9c87490259e39b31642980d99ebe3f508fefe4068b4ab9a64eddaa7ca` |
| `out/anspire-req120-unsigned-model.ttl` | 379,417 B | — |
| `out/anspire-req120-customer.ttl` | 312,889 B | — |
| `out/anspire-req120-unsigned-customer.ttl` | 295,537 B | — |
| `out/fk-proposals.json` | 190,577 B | `7116161b1d3b6d66d7970ab20dbdd5a56f2c812695bbcd5b7f386bf455ecf24c` |
| `out/fk-unresolved.json` | 25,917 B | `943939f8bb762831e2ea0b276c9512c3b0c08d33b5f0ac6f65d0c346e4ab1b7f` |

> 审核用 scratch（脚本、A/B 产物、变异日志）在 `.review-workspace/req120-qa/`，未进任何分支。
> 本报告写在 BA worktree（`demand`），**未 commit、未 push**，由 BA 决定入库。
