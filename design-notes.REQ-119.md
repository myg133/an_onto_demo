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
| 主键（B-2） | `ttl.rs::SchemaTypes::primary_key` / `model_pk_block` | `--schema` 的单列主键；抽取侧剥掉了就补一条属性 |
| 值域（B-3） | `ttl.rs::parse_allowed_values` | 只从列注释里明写的枚举解析；解析不出不导出 |
| 租户落点 | `ttl.rs::tenancy_report` | 序列化器与 CLI 共用同一份计算 |
| range 统计 | `ttl.rs::range_stats` | 从**最终正文**数；`TtlDocument::stats` 携带 |
| 真实列类型 | `ttl.rs::SchemaTypes::parse` / `xsd_for_sql_type` | `--schema`：SQL `data_type` → xsd；`ontok:rangeSource` 标来源 |
| 出口接线 | `src/cli.rs`（`--tenancy` / `--schema` + stderr 回执） | 仍只经 `domain::pipeline`（纪律 H） |

⛔ 未改：抽取 / 评分 / 分组 / 四红线 / `tools.rs` / `guard()` / `property_filter.rs` / `/api` 路由。

## 2 · 反向属性（R1 / R2 / AC-001）

- **命名**：正向 `<主语>__<宾语>` ⇒ 反向 `<宾语>__<主语>`（与正向对称、可读）。
- **双向声明**：正向块写 `owl:inverseOf :<反向>`，反向块写 `owl:inverseOf :<正向>`。
- **domain/range 对调**：反向的 `rdfs:domain` = 正向的 `rdfs:range`，反之亦然。
- **基数特征对调**：`1:N ↔ N:1`（`owl:FunctionalProperty ↔ owl:InverseFunctionalProperty`），
  `1:1` / 多对多不变。
- **B-1 已修正的方向**（之前整体反了，开推理会把一个套餐下的所有订阅合并成一条）：

  | 基数 | 语义 | 前向标注 | 反向标注 |
  | --- | --- | --- | --- |
  | `N:1`（`deductions → customers`） | 每个 subject 至多一个 object | **`owl:FunctionalProperty`** | `owl:InverseFunctionalProperty` |
  | `1:N`（`customers → deductions`） | 每个 object 至多一个 subject | **`owl:InverseFunctionalProperty`** | `owl:FunctionalProperty` |

  实测分布：`('N:1','Functional') = 52` + `('1:N','InverseFunctional') = 52`。
  客户视图同口径：`多对一` ⇒ Functional、`一对多` ⇒ InverseFunctional。
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
:订单__客户 a owl:FunctionalProperty, owl:ObjectProperty ;
    rdfs:label "订单 → 客户"@zh ;
    rdfs:comment "语义：归属于（多对一）"@zh ;
    rdfs:domain :订单 ;
    rdfs:range :客户 ;
    owl:inverseOf :客户__订单 .

:客户__订单 a owl:InverseFunctionalProperty, owl:ObjectProperty ;
    rdfs:label "客户 → 订单（反向）"@zh ;
    rdfs:domain :客户 ;
    rdfs:range :订单 ;
    rdfs:comment "反向属性：前向为「订单 → 客户」（多对一 → 一对多）"@zh ;
    owl:inverseOf :订单__客户 .
```

## 3 · xsd range（R3 / AC-002 / A-1）

### 3.1 真实列类型优先（`--schema`）

```powershell
ontok ontology export --project anspire_grouped --version 3 --model-view `
  --schema ..\code\out\schema.json --out out\model.ttl
```

`--schema` 吃 `ontok-introspect` 产出的 JSON（`tables[].columns[].data_type`）。
匹配键：**实体 key = 表名、属性列名 = 列名**（trim + 小写）。

| 情形 | range 来源 | TTL 标记 |
| --- | --- | --- |
| 给了 `--schema` 且 `(表, 列)` 匹配到、SQL 类型可映射 | 真实列类型 | `ontok:rangeSource "schema"` |
| 给了 `--schema` 但列没匹配上 / SQL 类型认不出 | 启发式 | `ontok:rangeSource "heuristic"` + stderr 列出 |
| 没给 `--schema` | 全启发式 | 每个属性 `"heuristic"` + 文件头 + `ontok:rangeSourceHeuristic N` + stderr |

⚠️ **快照层没有 `data_type`**（`snapshot::Property` 只有 column/nullable/note/sample），
所以「真实类型」只能由 `--schema` 从外部带进来；这不改快照结构、不改 apply 路径。

SQL → xsd 映射（`ttl.rs::xsd_for_sql_type`）：

| SQL `data_type` | xsd |
| --- | --- |
| `boolean` / `bool` | `xsd:boolean` |
| `integer` / `smallint` / `bigint` / `int*` / `serial*` | `xsd:integer` |
| `numeric` / `decimal` / `real` / `double precision` / `money` | `xsd:decimal` |
| `timestamp` / `timestamp without time zone` / `timestamp with time zone` / `timestamptz` | `xsd:dateTime` |
| `date` | `xsd:date` |
| `character varying` / `varchar` / `text` / `uuid` / `json` / `jsonb` / `bytea` … | `xsd:string` |
| 其他（自定义 enum / 域类型） | 回退启发式 + 计数 |

⛔ **`numeric` 绝不归 integer**（`point_change` 会丢小数）；
`character varying` 即使内容是 `"10"` 也必须是 string（`version_number` 的字符串排序：
`"9" > "10"`，物理类型就是 text）。**不要用业务语义覆盖物理类型**：`status` 真实是
integer 就写 integer，原注释保留在 `rdfs:comment`。

### 3.2 没给 `--schema` 时才走启发式

列名优先 → 中文业务名/注释 → `xsd:string` 兜底：

| 判定（按优先级） | xsd |
| --- | --- |
| 布尔：`is_*` / `has_*` / `can_*` / `allow_*` / `enable_*` / `auto_*` / `*_enabled` / `*_flag` / `*_deleted` / `*_active` / `*_visible` / `*_read` / `active`；中文「是否 / 可见 / 软删 / 允许 / 默认自动 / 开关」 | `xsd:boolean` |
| 时间：`*_at` / `*_time` / `*_on` / `*_until` / `*_start` / `*_end` / `timestamp`；中文「时间 / 时刻 / 到期 / 过期 / 有效期」 | `xsd:dateTime` |
| 日期：`*_date` / `date`；中文「日期」 | `xsd:date` |
| 整数：列名段 `count / num / times / tokens / points / qty / qpm / tpm / priority / sort …`；中文「次数 / 数量 / 积分 / 点数 / 优先级 / 排序」 | `xsd:integer` |
| 小数：列名段 `amount / price / balance / cost / fee / quota / limit / credit / rate / ratio / total …`；中文「金额 / 余额 / 价格 / 费用 / 用量 / 额度 / 限额」 | `xsd:decimal` |
| 文本：`*_id / *_name / *_status / *_code / *_json / *_by …`；中文「名称 / 状态 / 编号 / 类型 / 备注 / 客户 / 用户」 | `xsd:string` |
| **一条都没命中** | `xsd:string` + `ontok:rangeUnmapped true` + 计数/列出 |

### 3.3 fail-visible（⛔ 不静默）

- 未给 schema：文件头写
  `# · xsd 类型来源：**启发式**（未读真实列类型；--schema <schema.json> 可消除）—— schema 0 / heuristic 843`
  + 本体节点 `ontok:rangeSourceSchema 0 ; ontok:rangeSourceHeuristic 843` +
  每个属性 `ontok:rangeSource "heuristic"` + stderr 再次警告。
- 给了 schema 但列没匹配上：`ontok:rangeSource "heuristic"` + 计数 + stderr 清单
  `⚠ --schema 未匹配/类型认不出 N 个属性，已回退启发式：…`。
- 客户端（客户视图）没有列名，`--schema` 对它不生效：文件头写
  `客户视图不含列名，--schema 不生效；类型以建模视图为准`，stderr 同步说明。

**本版实测（`anspire_grouped` v3，870 条数据类型属性）：**

- 给 `--schema`（`code/out/schema.json`）：`rangeSource schema 870 / heuristic 0`，
  与真实 `data_type` **逐条一致 870/870（0 不一致、0 未映射）**；
  `rdfs:Literal 0`；分布：decimal 76 · integer 127 · dateTime 191 · date 0 · boolean 2 · string 474。
- 不给 `--schema`（旧启发式）：`heuristic 870`，与真实类型**不一致 116 条（13.3%）**。
  典型：`activity.status`（integer → 旧 string）、`agent_device_version.version_number`
  （varchar → 旧 integer）、`balance_operation_log.point_change`（numeric → 旧 integer）、
  `activity_keys.daily_ip_limit`（integer → 旧 decimal）。
- 完整不一致清单：`out/schema-vs-ttl.txt`（116 行，前 20 条见交付报告）。

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

> ⚠️ **R5 的三处标记只对建模视图有意义。** 客户视图**天然不含租户信息**
> （R4：租户键是建模/安全信息，客户视图结构上不读 `TtlOptions::tenancy`），
> 所以「未标注租户键」的警告放在客户视图只会误导。验证 R5 时请用
> `--model-view`；客户视图的正确判据是「一个 `tenant*` 都没有」。

## 6 · 客户视图 vs 建模视图

| 项 | 客户视图 | 建模视图 |
| --- | --- | --- |
| 反向属性 + 双向 `owl:inverseOf` | ✅（52 + 104） | ✅（52 + 104） |
| `rdfs:range` xsd 类型 | ✅（按业务名启发式） | ✅（`--schema` 真实列类型优先，未匹配才启发式） |
| `ontok:rangeSource` | ✅（全是 heuristic） | ✅（schema / heuristic 逐属性标注） |
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
| 客户视图（启发式） | 277,289（基线 220,696，**+25.6%**） | `4BBD623F…D64AA3` |
| 建模视图（启发式 + tenancy） | 302,798 | `81DCF0F3…E6F332` |
| 建模视图（`--schema` + tenancy） | 323,965 | `058CBA10…70576C` |
| 客户视图（带 `--tenancy`） | 277,289（**与不带逐字节相同**） | `4BBD623F…D64AA3` |

体积上限 40%（客户视图 ≤ 308,974 字节）—— 实测 **+25.6%**。
建模视图相对 REQ-110 基线 232,921 字节为 **+39.1%**（同样 ≤ 40%）。

### 7.1 · 替身实验修订（B-1 / B-2 / B-3）实测

| 项 | 判据 | 实测 |
| --- | --- | --- |
| B-1 基数方向 | `('N:1','Functional')` / `('1:N','InverseFunctional')` | **52 / 52**（修正前 0 / 0 + 52 条错标 Functional） |
| B-2 主键 | 每个类 `ontok:primaryKeyColumn`；主键属性 `ontok:isPrimaryKey true` | **84 / 84**；`__id` 属性 **82** 条（另 2 个类主键不叫 `id`） |
| B-3 值域 | 从注释确定性解析出的列 | **104** 条 `ontok:allowedValue`（清单见 `out/allowed-values.txt`） |
| FK 边缺失 | 抽取词干匹配问题（`product_id` → `products` ≠ `package_products`） | **登记不修**（另开需求改抽取） |

## 8 · 已知限制 / 风险（登记，不粉饰）

| # | 限制 | 影响 | 缓解 |
| --- | --- | --- | --- |
| 1 | **不给 `--schema` 时** xsd 类型仍是启发式 | 列名不规范的列可能判错（实测 116/870 = 13.3%） | 文件头 + `rangeSource` + stderr 三处明说；**Agent 消费时应带 `--schema`**（870/870） |
| 2 | `--schema` 只对建模视图生效（客户视图无列名） | 客户视图类型不可靠 | 客户视图不是 Agent 查询入口；类型以建模视图为准（文件头明说） |
| 3 | 反向名撞正向名时追加 `__inverse` | R1 的命名规则有 1 条例外 | 已在模块头/本文件写明；计数仍是 52/104 |
| 4 | 租户声明是**声明**不是证明 | 写错列名 = 安全漏洞 | 必须 `reviewed_by` 人签；未签不落标注；`tables` 收窄 |
| 5 | 反向属性 vs 已存在的反向关系（order ↔ customer_subscriptions） | 两条关系互为反向，各自仍生成一条反向属性 | 计数按 AC 要求保留 52 条；`__inverse` 后缀区分 |

## 9 · AC-009 的准备（5 个业务问题）

Agent 只读的 TTL（**真实列类型 + 租户标注**）：
`out/REQ-119-model-view.ttl`
（本次工作区：`D:\MyCodes\AI\an_onto_demo\feature-REQ-119\out\REQ-119-model-view.ttl`；
`out/` 按仓库纪律不入库，可用下方命令重新生成）。

⚠️ 这份 TTL 是 `--schema` 模式：**952** 条数据类型属性全部带 `ontok:rangeSource "schema"`
（870 原始 + 82 补出的主键），84 条主键声明、104 条 `ontok:allowedValue`；
`SUM(?扣费金额)` 这类聚合的类型前提成立，`status` 的魔数也有值域可对照。

预期能力与落点：

| # | 问题 | TTL 里要用的东西 |
| --- | --- | --- |
| 1 | 某客户上个月所有扣费明细 | `:customers__deductions`（反向）+ `:deductions__created_at`（dateTime）+ `:deductions__deduction_amount`（decimal） |
| 2 | 某客户当前余额 | `:customers__balance`（decimal）+ `:customers__customer_id`（string，租户键） |
| 3 | 某订阅还剩多少额度 | `:customer_subscriptions__usage_quotas`（反向）+ `:usage_quotas__*`（decimal/integer） |
| 4 | 某客户用过哪些 API（按授权） | 两跳反向：`:customers__licenses` → `:licenses__usage_details`（或 `:customers__customer_provider_permissions` 分支） |
| 5 | 某套餐下有哪些订阅 | `:package_templates__customer_subscriptions`（反向） |

⚠️ 只验证**方向/类型/字段定位**能不能写对；执行、租户强制注入、限量属 REQ-121。

## 10 · 复现

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
ontok ontology export --project anspire_grouped --version 3 --out out/REQ-119-customer-view.ttl
ontok ontology export --project anspire_grouped --version 3 --model-view --tenancy tenancy.json --out out/REQ-119-model-view.ttl
# A-1：真实列类型（推荐；AC-009 用的就是这份）
ontok ontology export --project anspire_grouped --version 3 --model-view `
  --tenancy tenancy.json --schema ..\code\out\schema.json --out out/REQ-119-model-view.ttl
```
