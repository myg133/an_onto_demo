# REQ-119 · 设计说明：本体可查询化（OBDA 前置）

> 本文件是 Dev 交付物，**请 BA 移动到** `BA/demands/REQ-119/design-notes.md`。
> 代码侧的同一套口径写在 `src/domain/ttl.rs` 模块头（AC-010 要求两处都有）。

## 0 · 一句话

把导出的 TTL 从「给人看的本体」升级成「**给 Agent 查的本体**」：
能反向走（`owl:inverseOf`）、能判断类型（xsd range）、能对上 IRI（`ontok:instanceIri`）、
能隔离租户（`ontok:tenantKey` / `ontok:tenantColumn`）。

## 1 · 落点（只有一处实现）

| 能力 | 实现位置 | 说明 |
| --- | --- | --- |
| 反向属性命名 | `ttl.rs::inverse_candidate` | 唯一入口；两版视图共用 |
| 反向属性块 | `ttl.rs::inverse_object_block` | 唯一入口；domain/range 对调 + `owl:inverseOf` |
| xsd 推断 | `ttl.rs::xsd_range` / `classify_column` / `classify_text` | 列名优先、中文业务名次之 |
| 租户落点 | `ttl.rs::tenancy_report` | 序列化器与 CLI 共用同一份计算 |
| range 统计 | `ttl.rs::range_stats` | 从**最终正文**数；`TtlDocument::stats` 携带 |
| 出口接线 | `src/cli.rs`（`--tenancy` + stderr 回执） | 仍只经 `domain::pipeline`（纪律 H） |

⛔ 未改：抽取 / 评分 / 分组 / 四红线 / `tools.rs` / `guard()` / `property_filter.rs` / `/api` 路由。

## 2 · 反向属性（R1 / R2 / AC-001）

- **命名**：正向 `<主语>__<宾语>` ⇒ 反向 `<宾语>__<主语>`（与正向对称、可读）。
- **双向声明**：正向块写 `owl:inverseOf :<反向>`，反向块写 `owl:inverseOf :<正向>`。
- **domain/range 对调**：反向的 `rdfs:domain` = 正向的 `rdfs:range`，反之亦然。
- **基数特征对调**：`1:N ↔ N:1`（`owl:FunctionalProperty ↔ owl:InverseFunctionalProperty`），
  `1:1` / 多对多不变。
- **计数**：52 条对象属性 ⇒ **52 条反向属性 + 104 条 `owl:inverseOf`**（双向各一）。
- **撞名裁定**：若 `<宾语>__<主语>` 已经是另一条正向属性的 IRI，反向名追加
  `__inverse`（还撞 ⇒ `__inverse2`…）。
  本版数据恰好有一对：`order → customer_subscriptions` 与
  `customer_subscriptions → order` 同时存在，所以那一条反向属性叫
  `customer_subscriptions__order__inverse`。
  **为什么不是 `_2`**：`_2` 看不出「这是反向」，`__inverse` 一眼能读；
  且 `_2` 与 NameSpace 的普通撞车后缀混在一起，无法区分。
- 正/反块都进同一份正文；反向块按 IRI 字典序排列（R6）。

例（客户视图）：

```turtle
:订单__客户 a owl:InverseFunctionalProperty, owl:ObjectProperty ;
    rdfs:label "订单 → 客户"@zh ;
    rdfs:comment "语义：归属于（多对一）"@zh ;
    rdfs:domain :订单 ;
    rdfs:range :客户 ;
    owl:inverseOf :客户__订单 .

:客户__订单 a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:label "客户 → 订单（反向）"@zh ;
    rdfs:domain :客户 ;
    rdfs:range :订单 ;
    rdfs:comment "反向属性：前向为「订单 → 客户」（多对一 → 一对多）"@zh ;
    owl:inverseOf :订单__客户 .
```

## 3 · xsd range（R3 / AC-002）

### 3.1 ⚠️ 必须说清的前提：快照层没有 SQL 类型

`snapshot::Property` 只有 `column` / `nullable` / `note` / `sample`，**没有 `data_type`**；
`ontology export` 只连本体库、不连源库。因此本轮做的是**确定性推断**，
不是「读列类型后映射」。等 REQ-120 的 R2RML 拿到真实列类型时，§3.2 的映射表
就是它的输入规格。

### 3.2 映射表（列名优先 → 中文业务名/注释 → 兜底）

| 判定（按优先级） | xsd |
| --- | --- |
| 布尔：`is_*` / `has_*` / `can_*` / `allow_*` / `enable_*` / `auto_*` / `*_enabled` / `*_flag` / `*_deleted` / `*_active` / `*_visible` / `*_read` / `active`；中文「是否 / 可见 / 软删 / 允许 / 默认自动 / 开关 / 已删除 / 已禁用 / 已启用」 | `xsd:boolean` |
| 时间：`*_at` / `*_time` / `*_on` / `*_until` / `*_start` / `*_end` / `timestamp`；中文「时间 / 时刻 / 到期 / 过期 / 有效期」 | `xsd:dateTime` |
| 日期：`*_date` / `date`；中文「日期」 | `xsd:date` |
| 整数：列名段 `count / cnt / num / nums / number / times / qty / quantity / points / point / tokens / days / hours / minutes / seconds / duration / cycles / seq / priority / sort / rank / qpm / tpm / rpm / percent`；中文「次数 / 数量 / 条数 / 个数 / 人数 / 天数 / 序号 / 排名 / 积分 / 点数 / 优先级 / 排序 / QPM / TPM / 百分比 / Token数」 | `xsd:integer` |
| 小数：列名段 `amount / price / balance / cost / fee / quota / limit / credit / recharge / rebate / discount / rate / ratio / total / sum / used / usage / remain / remaining / debt / threshold / weight / free / frozen / transfer / paid / refund / charge / sale`；中文「金额 / 余额 / 价格 / 费用 / 用量 / 额度 / 合计 / 单价 / 总价 / 折扣 / 比例 / 权重 / 时长 / 分钟 / 小时 / 税率 / 汇率 / 得分 / 上限 / 下限 / 原价 / 费率 / 限额 / 限制 / 赠送量 / 消耗量 / 变化量 / 授信 / 赠送」 | `xsd:decimal` |
| 文本：`*_id / *_key / *_code / *_name / *_title / *_type / *_status / *_url / *_json / *_text / *_by / *_no / *_user / *_cycle / *_config / *_data / *_version …`；中文「名称 / 状态 / 编号 / 编码 / 类型 / 备注 / 描述 / 说明 / 地址 / 电话 / 邮箱 / 链接 / 密码 / 账号 / 客户 / 用户 / 所属 / 信息 / 详情 / 规则 / 授权 / 模型 …」 | `xsd:string` |
| **一条都没命中** | `xsd:string` + `ontok:rangeUnmapped true` + 计数/列出 |

### 3.3 失败（fail-visible，⛔ 不静默）

- 属性照常导出，range = `xsd:string`；
- 属性块写 `ontok:rangeUnmapped true`（机器可查）；
- `range_stats()` 计数并在**清单**里列出；
- CLI 导出回执在 stderr 打出分布与清单，例如：
  `· xsd range（建模视图）：decimal 80 · integer 43 · dateTime 194 · date 0 · boolean 17 · string 536（含未映射 1） · rdfs:Literal 0 · 未映射清单：verify（数值列 decimal+integer = 123）`
- 本版 `anspire_grouped` v3 实测：**`rdfs:Literal` 0 条**；未映射 1 条（`verify`，
  快照里没有类型线索，按 R3 兜底为 string 并列出）。
- 映射失败**不是** `rdfs:Literal`：R3 的裁定是失败 ⇒ `xsd:string`（AC-002 只要求
  `rdfs:Literal` 不许出现在非失败属性上；本轮为 0 条，判据真空满足）。

## 4 · IRI 模板（AC-003）

每个类一条：

```turtle
ontok:instanceIri "https://ontok.local/anspire_grouped#{class}/{id}"
```

| 占位符 | 含义 | R2RML 侧 |
| --- | --- | --- |
| `{class}` | 类的 **key**（建模视图 `ontok:key`）；客户视图没有 key，用类 IRI 的本地名 | 映射到该类对应的表 |
| `{id}` | **主键列的值**（不是列名） | 取主键列 + URL 编码（IRI 安全字符集） |

- 模板由 **`base + project`** 派生（`TtlOptions::base` + project），**不含时间/随机数**。
- 计数 = 类的条数（本版：客户视图 83 / 建模视图 84）。
- **R2RML 必须产出同一个 IRI**：`{class}` 用实体 key，`{id}` 用主键值；
  否则 Agent 按 TTL 写的 SPARQL 会查不到实例。
- 客户视图也带模板（AC-007），且**不含**列名 / key（客户视图的 `name` 才是业务名）。

## 5 · 租户标注（R4 / R5 / AC-004 / AC-005）

### 5.1 声明文件

```json
{
  "columns": ["customer_id", "sub_customer_id", "main_customer_id"],
  "tables": ["customers", "deductions", "order"],
  "reviewed_by": "安全负责人"
}
```

- `columns`：租户键列名（trim / 去重 / 排序）；
- `tables`：可选白名单（实体 key = 源表名）；省略 = 全部实体；
- `reviewed_by`：**必须人签**。`null` / 空白 = 未签；
- 未知字段 ⇒ **解析报错**（fail-closed）。`{"column": ...}` 这种笔误不会被静默忽略。

样例文件：
- `tenancy.json` —— 已签样例（本版 8 个 `*customer_id` 列，命中 54 个属性 / 43 个类）；
- `tenancy.unsigned.example.json` —— `reviewed_by: null`，演示「未签」三处标记；
- `tenancy.tables.example.json` —— 演示 `tables` 白名单。

⚠️ `tenancy.json` 的 `reviewed_by` 是**样例声明**，不是安全评审结论；上线前必须由安全负责人签字。

### 5.2 匹配与标注

- 命中（列 ∈ `columns`，且 `tables` 省略或实体 key ∈ `tables`）⇒
  - 属性：`ontok:tenantKey true`
  - 其类：`ontok:tenantColumn "<列>"`（一个类多个租户列就多行）
- **只有已签声明才落标注**：未签声明不产生 `tenantKey` / `tenantColumn`
  （避免 Agent 把未评审的声明当安全事实）。
- 只出现在**建模视图**；客户视图结构上不读 `TtlOptions::tenancy`。

### 5.3 未标注时的三处标记（R5）

| 位置 | 内容 |
| --- | --- |
| ① 文件头注释 | `# · 租户键：未标注 —— 这份本体不能直接给数字员工用（--tenancy <file>）` |
| ② 本体节点 | `ontok:tenantUnannotated true` |
| ③ stderr | `⚠ 未标注租户键：这份本体不能直接给数字员工用` |

三种「不能用」的状态各有措辞：
- 没给声明文件 ⇒ `未标注` + `tenantUnannotated`；
- 给了但 `reviewed_by` 为空 ⇒ `未签` + `ontok:tenancyReviewed false` + `tenantUnannotated`；
- 已签但一个属性都没命中 ⇒ `声明未命中任何属性` + `tenantUnannotated`（仍写 `tenancyReviewedBy`）。

## 6 · 客户视图 vs 建模视图

| 项 | 客户视图 | 建模视图 |
| --- | --- | --- |
| 反向属性 + 双向 `owl:inverseOf` | ✅（52 + 104） | ✅（52 + 104） |
| `rdfs:range` xsd 类型 | ✅（按业务名推断） | ✅（按列名推断，更准） |
| `ontok:instanceIri` | ✅ 83 | ✅ 84 |
| `ontok:tenantKey` / `tenantColumn` | ⛔ **0** | ✅（已签声明时） |
| `ontok:tenantUnannotated` 等状态标记 | ⛔ **0** | ✅ |
| 属性级剔除（发布闸前处理） | ✅（11 条） | ⛔（建模轨不剔） |

结构保证：`serialize_customer` **不读** `TtlOptions::tenancy`，所以客户视图连
`tenant` 这个词都不会出现（实测 `to_lowercase().matches("tenant") == 0`）。

## 7 · 确定性（R6 / AC-006）与体积（AC-008）

- 排序：类按名字/key；属性按（类, 列名/属性名）；关系按 key；**反向块按 IRI 排序**。
- 所有集合用 `BTreeMap` / `BTreeSet`，没有 HashMap 迭代序。
- 不写生成时间（REQ-110 既有约束）。
- 实测（`anspire_grouped` v3，同版本两次进程级导出）：

| 文件 | 字节 | SHA-256（两次相同） |
| --- | --- | --- |
| 客户视图 | 246,741（基线 220,696，**+11.8%**） | `4E31DCEB…F5DA1E` |
| 建模视图（无声明） | 262,310 | `5D37E2F0…B0098C` |
| 建模视图（`tenancy.json`） | 266,013 | `247A5B6F…FDE4E` |
| 客户视图（带 `--tenancy`） | 246,741（**与不带逐字节相同**） | `4E31DCEB…F5DA1E` |

体积上限 40%（≤ 308,974 字节）—— 实测 +11.8%。

## 8 · 已知限制 / 风险（登记，不粉饰）

| # | 限制 | 影响 | 缓解 |
| --- | --- | --- | --- |
| 1 | xsd 类型是**推断**（快照无 SQL 类型） | 列名不规范的列可能被归到 string | `ontok:rangeUnmapped` + 计数列出；REQ-120 用真实列类型替换 |
| 2 | 反向名撞正向名时追加 `__inverse` | R1 的命名规则有 1 条例外 | 已在模块头/本文件写明；计数仍是 52/104 |
| 3 | 客户视图只能靠中文名推断类型 | 个别业务名含糊的属性会落到 string | 客户视图不是 Agent 查询入口；建模视图为准 |
| 4 | 租户声明是**声明**不是证明 | 写错列名 = 安全漏洞 | 必须 `reviewed_by` 人签；未签不落标注；`tables` 收窄 |
| 5 | 反向属性 vs 已存在的反向关系（order ↔ customer_subscriptions） | 两条关系互为反向，各自仍生成一条反向属性 | 计数按 AC 要求保留 52 条；`__inverse` 后缀区分 |

## 9 · AC-009 的准备（5 个业务问题）

Agent 只读的 TTL：`out/req119-model-tenancy.ttl`
（本次工作区：`D:\MyCodes\AI\an_onto_demo\feature-REQ-119\out\req119-model-tenancy.ttl`；
`out/` 按仓库纪律不入库，可用下方命令重新生成）。

预期能力与落点：

| # | 问题 | TTL 里要用的东西 |
| --- | --- | --- |
| 1 | 某客户上个月所有扣费明细 | `:customers__deductions`（反向）+ `:deductions__created_at`（dateTime）+ `:deductions__amount`（decimal） |
| 2 | 某客户当前余额 | `:customers__balance`（decimal）+ `:customers__customer_id`（string，租户键） |
| 3 | 某订阅还剩多少额度 | `:customer_subscriptions__usage_quotas`（反向）+ `:usage_quotas__*`（decimal/integer） |
| 4 | 某客户用过哪些 API（按授权） | 两跳反向：`:customers__licenses` → `:licenses__usage_details`（或 `:customers__customer_provider_permissions` 分支） |
| 5 | 某套餐下有哪些订阅 | `:package_templates__customer_subscriptions`（反向） |

⚠️ 只验证**方向/类型/字段定位**能不能写对；执行、租户强制注入、限量属 REQ-121。

## 10 · 复现

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
ontok ontology export --project anspire_grouped --version 3 --out out/req119-customer.ttl
ontok ontology export --project anspire_grouped --version 3 --model-view --out out/req119-model.ttl
ontok ontology export --project anspire_grouped --version 3 --model-view --tenancy tenancy.json --out out/req119-model-tenancy.ttl
```
