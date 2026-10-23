# REQ-121 · R2RML 映射生成 — Pre-merge QA 审核

> **结论：PASS（带非阻断项）**
> 所有 P0 判据我都用**自己重跑的产物 + 独立解析（rdflib）+ 直接查库 + 直接查 schema.json** 复算过，
> 没有引用 Dev 的日志数字：`false` 在任何模式下都**没有**产生一条 join（§2）、
> `inherit` 默认不生成/带 flag 才生成且带标记（§2）、
> 与 schema 对账**真的会点名到 `表.列` 并 fail-closed**（§3，含变异实测）、
> 类集双向差集为 **0**（§5）。
> 无阻断项；4 条非阻断项（§10），其中 1 条是**变异抓不到**的测试覆盖缺口（§3.4）。

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-121`（分支 `feature/REQ-121`，HEAD `28199ca`，3 个提交：`3c229fe` → `bf252b7` → `28199ca`） |
| 基线 | `develop` `e73d5e9` |
| 本体库 | `postgresql://…@172.17.0.87:5432/anspire_real` |
| 测试库 | `postgresql://…@172.17.0.87:5432/ontok_test_req121`（Dev 建的独立库） |
| 生产快照 | `anspire_r2rml_excl` v2（84 类 / 78 关系：true 70 · inherit 1 · false 7） |
| 对照快照 | `anspire_r2rml` v2（96 类 / 79 关系：true 72 · inherit 1 · false 6）/ `anspire_grouped` v3（84 类 / 52 关系：全部 `false` 且**无** evidence） |
| schema | `code/out/schema.json`（96 表 / 1116 列） |
| 审核方式 | 只读审核 + **独立 detached worktree 变异**（`.review-workspace/req121-qa/mut`，未改 `feature/REQ-121`、未 push，已回收）+ 端到端重跑 + 直接查库复算 |
| 审核日期 | 2026-10-23 |

**我实际跑的四条基线**（产物留在 `.review-workspace/req121-qa/`，命令见附录 A）：

| 跑法 | 文件 | TriplesMap | 数据类型 POM | joinCondition | 字节 | rdflib triples |
| --- | --- | --- | --- | --- | --- | --- |
| `excl` v2 默认（带 `--exclude --tenancy`） | `excl-default.ttl` | **84** | **952** | **70** | 211,552 | **5,878** |
| `excl` v2 `--include-inherit` | `excl-inherit.ttl` | 84 | 952 | **71** | 211,842 | 5,887 |
| `anspire_r2rml` v2 + `--exclude`（未排除快照） | `unfiltered-excl.ttl` | 95 | 1,107 | 72 | 239,844 | 6,681 |
| `grouped` v3 默认 | `grouped-v3.ttl` | 84 | 952 | **0** | 189,308 | 5,264 |

> 交叉验证 BA 的口径：`excl` 快照**不加** `--tenancy` 时是 **5,824 triples**（`5,878 − 54`，54 = 43 个类上的 54 条 `ontok:tenantColumn`），
> 与 BA 报的 5824 逐字节对得上；84 / 70 / 952 也与 BA 一致。

---

## 1 · 一句话结论

**「证据等级决定生成什么」这条核心裁定是真的落到了生成逻辑里，不是只写在文件头注释里。**

- `false` 的 7 条边在**默认与 `--include-inherit` 两种模式下 join 数都是 0**（我用 rdflib 重新解析产物、再回库取 `join_verified` 逐条比对，不是看 stats）；
- `inherit` 的 1 条边默认不生成、带 flag 才生成，两种跑法 join 数**恰好差 1**，且生成的那条带 `ontok:joinState "inherit"` + `ontok:joinEvidence`；
- 变异「让 `false` 也生成 join」**被抓**（两种改法都试了，见 §2.4）；
- 对账不是摆设：把 join 的 child 列名改错一位，真实库跑 `r2rml generate` **退出码 1、不落任何文件、逐条点名 `表 X 缺子列 Y`**（§3.3）。

---

## 2 · `false` / `inherit` 的生成控制（AC-005 / AC-006，含变异）

### 2.1 事实来源（先确认「真值」在哪）

`ontok:joinVerified` 直接从库里取（`link_types.join_verified` / `join_evidence`，按 `version_id`），
**不依赖生成器的 stats**：

| 快照 | true | inherit | false（**有** evidence） | false（**无** evidence = 未标） |
| --- | --- | --- | --- | --- |
| `anspire_r2rml_excl` v2 | 70 | 1 | **7** | 0 |
| `anspire_r2rml` v2 | 72 | 1 | 6 | 0 |
| `anspire_grouped` v3 | 0 | 0 | 0（52 条 `false` 的 `join_evidence` **全部为 NULL**） | **52** |

### 2.2 `false` 永不成 join（AC-006）

用 rdflib 解析三份产物（默认 / 含 inherit / 旧快照），把每条 join 归一成 `(子表, 子列, 父表)`，
与库里 7 条 `false` 边的 `(from_type, evidence[0].column, to_type)` 求交：

| 跑法 | `false` 边生成了 join 的条数 |
| --- | --- |
| `excl` 默认 | **0** |
| `excl --include-inherit` | **0** |
| `grouped` v3（52 条旧布尔 false） | **0**（该文件 join 总数为 0） |

点名要求的这一条 —— **`deductions → customers`（`false`，child `deductions.customer_id`）**：

```
默认：             joinCondition 数 = 0
--include-inherit： joinCondition 数 = 0
grouped v3：       joinCondition 数 = 0
```

它没有消失，而是被写进文件尾**带原因**（原文）：

```
# · deductions → customers（child deductions.customer_id）：false（仅名字推断，未验）：insufficient_sample: 有效样本 4 行 / 1 个不同值 < 3 条下限（跳过 NULL 之后）
```

7 条 `false` 边逐条都在尾注里，理由分三类（实测原文）：
`insufficient_sample`（3 条）/ `unsigned: 有 inherit 提案但**未签字**`（3 条）/ `naming-only`（1 条）。

### 2.3 `inherit` 默认不生成（AC-005）

`api_call_logs → licenses`（`inherit`，child `api_call_logs.license_id`）：

| 跑法 | 是否出现在 join 里 | `ontok:joinState` | `ontok:joinEvidence` |
| --- | --- | --- | --- |
| 默认 | **否**（0 条） | — | —（只在文件尾列为「inherit（本表未实测）… 默认不生成」） |
| `--include-inherit` | **是**（1 条） | **`"inherit"`** | **有**（证据原文整串） |

判据「差 1」成立：**70 → 71**。两份文件的全部差异只有 15 行，且**全部**属于这条边：

```
=>         rr:predicate :api_call_logs__licenses ;
=>         # joinEvidence: inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)
=>         ontok:joinState "inherit" ;
=>         ontok:joinEvidence "inherited from license_quota.license_id (verified 5/5) -> licenses.id via primary_key (plural)" ;
=>             rr:parentTriplesMap map:TM_licenses ;
=>             rr:joinCondition [ rr:child "license_id" ; rr:parent "id" ]
=> # 三态策略：… "inherit" ⇒ 生成并标 ontok:joinState "inherit"（1 条，1 条已生成）…
<= # 三态策略：… "inherit" ⇒ 默认不生成（1 条，文件尾列清单）…
<= # · api_call_logs → licenses（child api_call_logs.license_id）：inherit（本表未实测）：… 默认不生成，--include-inherit 才生成
```

没有「顺手多出别的 join」或「静态列跟着变」的情况 —— 71 − 70 的那 1 条就是它。

### 2.4 变异（独立 detached worktree，未碰 `feature/REQ-121`）

| # | 变异 | 期望 | 实测 |
| --- | --- | --- | --- |
| M1a | `NamingOnly` 分支的 `emit` 由 `false` 改成 `true`（**单行**） | 红 | **11/13 红**。失败原因不是「静默出 join」，而是 fail-closed：`关系 orders_amount__customers… 的父列无法从 ontok:joinEvidence 解析 …（⛔ 不拿主键顶替）` |
| M1b | M1a + 父列解析不出时用父表主键顶替（这才真的会出 join） | 红 | **4/13 红**，其中 `ac006_false_edges_never_produce_a_join_in_either_mode` 断言原文：`false 边生成了 join（include_inherit=false）` |
| M2 | `inherit` 当 `true`（默认也生成） | 红 | **2/13 红**：`ac005 … 默认只生成 true 的那一条（left: 2, right: 1）`、`ac004` |
| M6 | 删掉对账的 fail-closed（`if !failures.is_empty() { return Err }`） | 红 | **1/13 红**：恰为 `ac008_schema_reconciliation_names_every_failure_and_emits_no_document` |

> M1a 顺带证明了一件事：只把 `false` 放行**不会**产出错误映射，它会先撞上「父列必须来自证据」的硬闸。
> 这条闸与 AC-006 是两道独立的防线，这个纵深是对的。

---

## 3 · 与 schema 对账（AC-008：抽样 + 变异 + 覆盖度）

### 3.1 抽样 5 条 join（我自己查 `schema.json`，不用生成器自报）

| # | join | 子列在子表 | 父列在父表 | 父列是主键/唯一列 |
| --- | --- | --- | --- | --- |
| 1 | `admin_user_roles.role_id → admin_roles.id` | ✅ | ✅ | ✅ PK |
| 2 | `admin_user_roles.admin_user_id → admin_users.id` | ✅ | ✅ | ✅ PK |
| 3 | `agent_white_list.customer_id → customers.id` | ✅ | ✅ | ✅ PK |
| 4 | `app_billing_transactions.customer_id → customers.id` | ✅ | ✅ | ✅ PK |
| 5 | `app_billing_transactions.app_id → **service_apps.app_id**` | ✅ | ✅ | ✅ **唯一列（非主键）** |

第 5 条是有意挑的：它证明「`rr:parent` 不必是主键」这条 AC-004 注记是**真按证据走**的。
全量复核同样干净：**70 条 join / 0 处失败**（表不存在 0、列不存在 0、父列非唯一 0）。
`rr:parent` 落在「唯一列而非主键」上的 join 共 **4 条**，全部指向 `llm_model.model_id`（`unique_columns=["id","model_id","model_name"]`）。

### 3.2 全量对账结果（不只是抽样）

- 84 个 `rr:tableName` 全部在 `schema.json` 里；
- 952 个 `rr:columnName` + 70 个 `rr:child` + 70 个 `rr:parent` 全部存在；
- 每个 TriplesMap 恰好 1 个 `logicalTable` / 1 个 `subjectMap` / ≥1 个 `predicateObjectMap`，**空壳 0**；
- 每个 POM 恰好 1 个 `objectMap`；`rr:parentTriplesMap` **悬空 0**；
- 1022 个 POM 的谓词 IRI **两两不同**（0 碰撞），即 `local_model` 在这个数据集上没有把两列压成同一个 IRI。

### 3.3 变异：把某条 join 的 child 列名改错 ⇒ 必须失败并点名

变异内容：生成器写 `rr:child` 时统一加 `_x`（模拟快照里的实现列与 schema 漂移）。
用变异后的二进制跑**真实库**（同一个 `--exclude` 口径）：

```
✗ R2RML 与 schema 对账失败（70 处）—— ⛔ 未写出任何文件（fail-closed）：
  - 表 admin_user_roles 缺子列 admin_user_id_x（关系 admin_user_roles_admin_user_id__admin_users 的 rr:child）
  - 表 admin_user_roles 缺子列 role_id_x（关系 admin_user_roles_role_id__admin_roles 的 rr:child）
  - 表 agent_white_list 缺子列 customer_id_x（关系 agent_white_list_customer_id__customers 的 rr:child）
  …（共 70 行）
```

| 判据 | 实测 |
| --- | --- |
| 退出码 | **1** |
| stdout | **空**（0 字节） |
| `--out` 文件 | **未创建**（fail-closed 成立，没有半份映射） |
| 是否点名 | **是**，`表.列` + 关系 key 三件齐全 |
| 测试是否红 | **是**（11/13 红） |

### 3.4 ⚠ 变异覆盖缺口：三条细粒度 guard **没有专属回归**

同一套变异手法，把 schema 对账里的单条 guard 删掉，再跑 REQ-121 的**全部 34 条**测试
（13 集成 + 21 单测）：

| 变异 | 结果 |
| --- | --- |
| 删 `rr:child` 存在性对账（`表 X 缺子列 Y` 整块） | **34/34 全绿** |
| 删「证据指向的表 == 关系终点」+ `rr:parent` 存在性对账 | **34/34 全绿** |

也就是说：**粗粒度**「删 schema 对账」会被 `ac008` 抓住（M6），但**细粒度**把这三种失败模式
单独拆掉，没有任何测试会红。AC-008 要求的三种列（`rr:columnName` / `rr:child` / `rr:parent`）
里，只有 `rr:columnName` 和「表不存在」有测试。

- 现在**代码是对的**（真实库 0 失败，我逐条核过），所以这是**非阻断**；
- 但验收 §二 G 的「删 schema 对账必须被抓」严格说只成立了一半。
- 建议（不阻塞合并）：补 3 条夹具单测 —— child 列不在 schema / parent 列不在 schema / 证据里的父表 ≠ 关系终点，各断言 `R2rmlError` 的文案含 `表.列`。

---

## 4 · template 与中文 IRI（AC-002）

### 4.1 template 的列 == 该类主键列

**84/84 全部对上**（不是抽 5 个）：把每个 `rr:template` 的 `{…}` 占位符与
`schema.json` 里该表的 `primary_key` 逐字比较，不符 **0** 处。抽 5 个样例：

| 表 | 主键列 | `rr:template` |
| --- | --- | --- |
| `activity` | `id` | `https://ontok.local/anspire_r2rml_excl#activity/{id}` |
| `activity_keys` | `id` | `…/activity_keys/{id}` |
| `activity_recommendations` | `id` | `…/activity_recommendations/{id}` |
| `admin_roles` | `id` | `…/admin_roles/{id}` |
| `admin_user_roles` | `id` | `…/admin_user_roles/{id}` |

更强的一条：template 不是「拼出来的相似串」，而是 **TTL 的 `ontok:instanceIri` 同一个模板展开**——
把 TTL 里 84 条 `ontok:instanceIri` 按 `{class}`→表名、`{id}`→主键列替换后，与 R2RML 的
`rr:template` **逐类比较，不符 0/84**。展开前后都没有残留 `{class}`（未展开占位符 0 处）。

### 4.2 中文 / 编码

| 检查 | 结果 |
| --- | --- |
| **IRI 里含非 ASCII 字符** | **0 处**（遍历 5,878 个三元组的主/谓/宾，凡 `URIRef` 全 ASCII） |
| 中文出现在哪 | `rdfs:label "活动"@zh` 这类**字面量**、以及注释行；**不进 IRI** |
| UTF-8 解码 | 通过（`decode('utf-8')` 无异常），**U+FFFD 替换符 0 个** |
| BOM / 行尾 | **无 BOM**；**CRLF 0 行**（纯 LF） |
| 中文证据串 | 库里 78 条 `join_evidence` 原文**逐字**出现在产物里（78/78，含全角括号、`⇒`、`——`） |
| 乱码 | 无。抽 `service_apps → marketing_channel`：`naming-only: 该列未进入值域重叠抽样（不是 *_id 列或本身是本表主键）—— 仅按列名推断`，与库中原文逐字节相同 |

> 需求风险 #2「中文类名导致 IRI 不合法」在本数据集上**没有触发条件**：类 key 就是表名（全 ASCII），
> IRI 本地名走的是 REQ-119 的 `local_model`（非 `[0-9A-Za-z_]` 一律换 `_`）。
> 所以这里能给出的结论是「**产物里不存在中文 IRI，因此不存在非法中文 IRI**」，
> 而不是「中文 IRI 被正确编码了」—— 后者要有中文 key 的样本才能验，本轮没有。

---

## 5 · 类集一致性：R2RML `rr:class` vs 建模视图 TTL

同一快照（`anspire_r2rml_excl` v2）各导一次：

| 集合 | 来源 | 元素数 |
| --- | --- | --- |
| R2RML `rr:class` | `excl-default.ttl` 的 84 个 `rr:subjectMap` | **84** |
| 建模视图类 | `ontology export --model-view` 的 `owl:Class`（TTL） | **84** |

```
R2RML − TTL = []      ← 空
TTL − R2RML = []      ← 空
对称差 = 0
```

即：**不存在「R2RML 里有、本体里没有」的类**（那会让 SPARQL 查出库里不存在的类型），
也不存在「本体里有、R2RML 查不到」的类（数字员工会以为该类没有实例）。
本轮 84 类**全部**有单列主键，所以「未生成清单」为空（复合主键跳过 0）—— 这是数据本身的性质，不是漏生成。

---

## 6 · fail-visible 实测（A-2.4）

拿**未排除**快照（`anspire_r2rml` v2，96 类 / 79 关系）配 `--exclude` 跑，要求：stderr 警告 + 文件头标注 + **不阻断**。

**stderr 原文**（实测，`--out` 模式下 stdout 为空）：

```
· R2RML（anspire_r2rml v2）：95 个 TriplesMap / 1179 个谓词-宾语映射（数据类型 1107 · join 72）· joinCondition 72 ·
  三态 true 72 / inherit 1（生成 0）/ false（仅名字推断，有据）6 · 未标（无验证信息）0 · 复合主键跳过 1 ·
  跳过已签关系 0 · schema 对账 0 失败
⚠ 排除表：本映射包含 11 张已排除的表（_volc_dts_pg_ddl_18e4b12d…、_volc_dts_pg_ddl_8f7cc0a6…、_volc_dts_pg_ddl_9a38a0a4…、
  alembic_version、llm_auth_cache_change_outbox、llm_auth_cache_kafka_event_outbox、llm_provider_back、
  llm_provider_back_20260630、llm_provider_back_20260701、rpt_dim_customer、rpt_dws_channel）
  —— 这份快照没带 --exclude；生产口径应重建快照（本文件仅供对照，不阻断）
⚠ 租户列未标注：TriplesMap 上一条 ontok:tenantColumn 都没有 —— 这份映射不能直接给数字员工用（--tenancy <file>）
```

**退出码 = 0**（不阻断）✓。**文件头第 13 行**：

```
# 排除表：⚠ 本映射包含 11 张已排除的表（…）—— 这份快照没带 --exclude，生产口径应重建；
  本文件仅供对照（另有 1 张已排除的表因复合主键/无主键未生成）
```

为什么是 11 而不是 12：第 12 张 `llm_auth_cache_trigger_state` 是**复合主键**（`state_scope`/`subject_id`/`subject_type`）
没有 TriplesMap，文件头已显式补了「另有 1 张…未生成」，文件尾两个清单里也各点了一次名。
**12 = 11（在映射里）+ 1（无 TriplesMap）**，账是平的。

对照：生产口径（`excl` 快照 + `--exclude`）文件头是
`# 排除表：与 exclusions.json 一致（0 张已排除的表进入映射；12 张被排除、未生成）`，
且 12 张表的 `rr:tableName` 计数确实**全为 0**（我用 rdflib 数过：映射里 `rr:tableName` 共 84 个，命中排除表 0 个），
12 张表只出现在文件尾注释里。

> A-2.3（`--fk-signed` 审计）也实测过：拿 Dev 的 `out/req121-rebuild/fk-signed.json` 跑，
> stderr 报 `跳过已签关系 2`，文件尾点名两条 ——
> `llm_auth_cache_kafka_event_outbox → llm_auth_cache_change_outbox`、`rpt_dim_customer → customers`，
> 与 `excl-stats.json` 里 `skipped_excluded_edges` 的 2 条**逐字一致**，且**不阻断**（exit 0）。

---

## 7 · `false` / `未标` 对照（A-3）

两种快照各跑一次，`false` 与「未标」**分开报**：

| 快照 | `false`（有 evidence，有据） | `未标`（无验证信息） | join 数 |
| --- | --- | --- | --- |
| **旧** `anspire_grouped` v3 | **0** | **52** | 0 |
| **新** `anspire_r2rml_excl` v2 | **7** | **0** | 70 |

库里的真值支持这个拆分：`grouped` v3 的 52 条边 `join_verified='false'` 且 `join_evidence` **全部为 NULL**
（REQ-120 之前是 BOOLEAN 列，`false` 只表示「没验过」）；新快照的 7 条 `false` **全部**带 `join_evidence`。

**52 条旧边没有一条变成 join**：`grouped-v3.ttl` 的 joinCondition 总数为 **0**，
且 52 条边逐条出现在文件尾，理由是

```
# · admin_user_roles → admin_users（child admin_user_roles.admin_user_id）：未标（旧版布尔 false 没有 joinEvidence —— 只说明「没验过」，不当作 false）
```

文件头也写了口径（原文）：

```
# 未标（没有 joinVerified，或旧版布尔 false 没有 joinEvidence）：52 条，同样不生成
# ⚠ 口径：false ≠「验过但失败」。false 只表示「仅有名字推断、没有数据证据」（REQ-120 的 NamingOnly）；
#   「未标」表示根本没有验证信息。两者都不生成 join，但读法不同 —— 别把未标读成 false。
```

这正是本需求最贵的一种误读（把 52 条「没验」读成「52 条验过但失败」），实测**已被挡住**。

---

## 8 · 确定性（AC-010）

同参数两组，各跑两次独立进程：

| 参数 | sha256（两次） | 结论 |
| --- | --- | --- |
| `--exclude --tenancy` | `67A9EE810DB62A74ED4DB8D62101B7351D096002BBFD1520E705D17A4BDFDB03` ×3 | **逐字节相同** |
| `--exclude`（无 tenancy） | `598F6C196084EB922FB7FB11D19293CE46228B4CDDE01A0DD6F23121B1C2F92A` ×2 | **逐字节相同** |

- 加不加 `--tenancy` **会**改变文件（5,824 → 5,878 triples；209,401 → 211,552 字节），
  所以两次比对都在**同参数**下做，没有拿不同参数的结果冒充确定性。
- 文件里没有时间戳（`dcterms:created` / 「生成于」0 处），类/属性/关系全部排序遍历。
- stdout 模式与 `--out` 模式写的是**同一份字节**（我用 sha256 对过，完全一致），出口只有一处序列化。

---

## 9 · 不退化（AC-012）

### 9.1 测试计数（`cargo test --no-fail-fast`，我重跑）

| 项 | 期望 | 实测 |
| --- | --- | --- |
| lib | 435 | **435 passed / 0 failed** ✅ |
| doc | 4 | **4 passed** ✅ |
| `test_req121_r2rml` | 13 | **13 passed** ✅ |
| `test_req120_fk` | 11 | **11 passed** ✅ |
| `test_req107_outlet_pipeline`（outlet） | 12 | **12 passed** ✅ |
| `test_req107_source_enumeration`（source_enum） | 10 | **10 passed** ✅ |

全量 83 个测试二进制，只有 4 个 suite 红（共 5 条），全部是**既有红灯**（9.2）。

### 9.2 5 条既有红灯 —— 我自己在 `develop` `e73d5e9` 上复现归因

在 `code/`（develop worktree）上跑同样 4 个 suite，**同样的 5 条红、同样的文案**：

| # | 用例 | develop 上是否同样失败 | 归因 |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | **是**（一模一样） | 缺外部夹具 `../.review-workspace/fix-props/schema.json`（gitignore）。环境问题，与 REQ-121 无关 |
| 2 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | **是**（一模一样） | CLI 侧 items 比 legacy 少一条 `add_link_type`（`t_dev → t_customer`）—— REQ-120 FK 补全后遗留的基线漂移；该用例跑 `ontok-propose` 时**不传 `--fk-sign`**，走不到 REQ-121 改动的 `inject_signed`/排除分支，失败点在「CLI 与 legacy 两侧题目生成不一致」，与 R2RML 无因果 |
| 3 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | **是**（一模一样） | 同一份外部夹具缺失 |
| 4 | `test_req084_every_registered_name_is_still_literally_at_its_line` | **是**（一模一样） | `src/extract/propose.rs:280` 登记行漂移；该文件**不在** REQ-121 的 diff 里 |
| 5 | `test_req084_no_unregistered_real_name_remains_in_the_registered_files` | **是**（一模一样） | 同上（同一次登记表漂移的连带） |

> 结论：**没有一条红灯是 REQ-121 引入的**。这 5 条在合入后仍会是红的，与本需求无关（建议另行开单）。

### 9.3 禁改面 diff 为空 + 守卫测试全绿

```
git diff e73d5e9..HEAD -- src/mcp/tools.rs src/domain/property_filter.rs \
    Cargo.toml Cargo.lock src/server/    →   <EMPTY>
```

- `guard()`（`src/mcp/tools.rs:693`）无 diff；
- `property_filter.rs`（G1–G4）无 diff；
- `/api` 鉴权与路由（`src/server/**`）**0 行** diff；
- **零新依赖**：`Cargo.toml` / `Cargo.lock` 无 diff（R2RML 是手写 Turtle）。
- 未删测试、未新增 `#[ignore]`：`git diff … -- tests/` 里 `+#[ignore]` 0 处、被删的 `#[test]` 0 处。

守卫类测试逐个跑过，全绿：

| suite | 结果 |
| --- | --- |
| `test_anchor_guard` | 3 passed |
| `test_guard_manifest_req081` | 6 passed |
| `test_read_only_guard` | 5 passed |
| `test_property_filter`（G1–G4） | 10 passed |
| `test_req107_guard_granularity` | 4 passed |
| `test_binary_output_guard` | 7 passed |
| `test_req084_retained_names` | **4 passed / 2 failed —— 既有红灯，`develop` 上逐字复现**（9.2 #4/#5） |

### 9.4 既有测试被改动的地方（逐处确认没有「放宽断言」）

| 文件 | 改动 | 判定 |
| --- | --- | --- |
| `tests/test_req107_outlet_pipeline.rs` | 出口数 45→**47**、内部轨 15→**17**、`I1..I10`→`I1..I12`、`DOMAIN_PRINT_SITES` 8→**9** | 因新增 I11/I12 而**必须**改；仍是**精确相等**断言，不是 `>=` |
| `tests/test_req107_source_enumeration.rs` | 枚举表里加 `r2rml_cmd` 与 `domain::r2rml` 模块 | 加的是「必须存在」的清单，只会更严 |
| `tests/test_binary_output_guard.rs` | owner 列表加 `r2rml_cmd` | 反向判据仍要求「打印点必须在 `DOMAIN_PRINT_SITES` 里登记」，`r2rml_doc` 已登记（I11） |
| `tests/test_req120_fk.rs` | `inject_signed(..., None)` 多传一个 `None` | 签名变更，断言未动 |
| `src/domain/pipeline.rs` 内嵌单测 | 同上口径 45→47 / 15→17 | 精确断言保留 |

### 9.5 出口登记（AC-012 / 纪律）

`OUTLETS` 新增 **I11**（stdout）/ **I12**（`--out`），均为 `Track::Internal`，`enforcement = InternalOnly`，
与「R2RML 是映射层产物、不是客户视图」的定位一致；`test_req107_track_manifest` 2 passed（客户 17 / 建模 10 的两条不变量未被改宽）。

### 9.6 `.scratch/`

`git status` 只有 `?? .scratch/`，`git ls-files .scratch` **为空** ⇒ **未提交** ✓。
（但它也**不在** `.gitignore` 里，见 §10 N2。）

---

## 10 · 阻断项 / 非阻断项

### 阻断项：**无**

12 条 AC + A-1~A-3 全部满足；三条 P0 生成控制（false / inherit / 对账）都经过**变异验证**。

### 非阻断项（4 条）

| # | 级别 | 项 | 证据 | 建议 |
| --- | --- | --- | --- | --- |
| **N1** | P2 | **AC-008 的三条细粒度 guard 没有回归测试**：删掉 `rr:child` 存在性 / `rr:parent` 存在性 / 「证据表 == 关系终点」对账，**34 条 REQ-121 测试全绿** | §3.4 的两次变异 | 补 3 条夹具单测（各断言 `R2rmlError` 文案含 `表.列`）。不阻塞：真实库对账 0 失败，代码本身是对的 |
| **N2** | P2 | **`.scratch/` 未进 `.gitignore`**。里面有 `r2rml-model.ttl`(327 KB) / `probe.ttl`(326 KB) / `r2rml-proposal.json`(299 KB)，含**全部真实表名**。今天没提交，但一次 `git add -A` 就会泄露 | `git ls-files .scratch` 空；`.gitignore` 里有 `/.review-workspace/` 却没有 `/.scratch/` | 照 REQ-084 N1 的处置，加一行 `/.scratch/` |
| **N3** | P3 | **死代码/永不出现的文案**：`r2rml.rs:552` 的 `if evidence.is_empty()` 分支不可达 —— 空证据会先在 `parse_parent_target("")` 处返回 `None` 并按「父列无法解析」报错 | `parse_parent_target("")` → `rsplit_once("->")` 返回 `None`；两支都进 `continue` | 要么删掉该分支，要么把「没有 joinEvidence」的判定挪到解析之前（文案更贴切） |
| **N4** | P3 | **R2RML 出口无 e2e 字节比对**（`DOMAIN_PRINT_SITES` 里 `e2e_covered: false`，Dev 已如实登记并写明原因）；`r2rml generate` 的 CLI 级行为（退出码 / 不落盘 / stdout==`--out`）只有**我这次的手工证据**，没有恒常判据 | §3.3、§8 均为手工实测 | 若要留恒常判据，补一个「真进程 + 专属 schema 夹具」的用例；不做也不算违约（AC 没要求） |

> 另有一条**口径提示**（不是缺陷）：文件头的「被排除边 8」指的是**不生成 join 的边**
> （inherit 1 + false 7），与「被排除的**表**」是两件事。两者都叫「排除」，读文件时容易混。
> 建议下一轮把前者改称「未生成的边」。

---

## 附录 A · 复现命令（我实际跑的）

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
$exe='D:\MyCodes\AI\an_onto_demo\feature-REQ-121\target\debug\ontok.exe'
$schema='D:\MyCodes\AI\an_onto_demo\code\out\schema.json'
$excl='D:\MyCodes\AI\an_onto_demo\feature-REQ-121\exclusions.json'
$ten='D:\MyCodes\AI\an_onto_demo\feature-REQ-121\tenancy.json'
$o='D:\MyCodes\AI\an_onto_demo\.review-workspace\req121-qa'

# ① 生产口径（默认 / 含 inherit）
& $exe r2rml generate --project anspire_r2rml_excl --version 2 --schema $schema --exclude $excl --tenancy $ten --out "$o\excl-default.ttl"
& $exe r2rml generate --project anspire_r2rml_excl --version 2 --schema $schema --exclude $excl --tenancy $ten --include-inherit --out "$o\excl-inherit.ttl"

# ② fail-visible（未排除快照 + --exclude）
& $exe r2rml generate --project anspire_r2rml --version 2 --schema $schema --exclude $excl --out "$o\unfiltered-excl.ttl"

# ③ 旧快照对照（false / 未标）
& $exe r2rml generate --project anspire_grouped --version 3 --schema $schema --exclude $excl --out "$o\grouped-v3.ttl"

# ④ 建模视图 TTL（类集对账用）
& $exe ontology export --project anspire_r2rml_excl --version 2 --model-view --schema $schema --tenancy $ten --out "$o\model-view.ttl"

# ⑤ 全量测试
$env:ONTOK_TEST_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/ontok_test_req121'
cargo test --no-fail-fast
```

独立复核脚本（我写的，不依赖生成器 stats）：
`verify_mapping.py`（2.1–2.3 / 5 / 6 / 7 的所有数字）、`verify_extra.py`（datatype/租户/证据原文）、
`check_excl.py`、`check_tpl.py`、`check_dup.py`、`check_parent.py`。

变异 worktree：`git worktree add --detach .review-workspace/req121-qa/mut feature/REQ-121`，
`CARGO_TARGET_DIR=.review-workspace/req121-qa/target-mut`；跑完 `git worktree remove --force` 回收，
`feature/REQ-121` 未产生任何提交，`git status` 仍只有 `?? .scratch/`。

---

## 附录 B · AC 对照（简表）

| AC | 判据 | 结果 | 证据位置 |
| --- | --- | --- | --- |
| AC-001 | TM 数 == 快照类数；三件套齐全；无空壳 | ✅ 84 == 84，空壳 0 | 开头基线表 / §3.2 |
| AC-002 | template 由 `instanceIri` 展开，列 == 主键列 | ✅ 84/84；非 ASCII IRI 0 | §4.1 / §4.2 |
| AC-003 | `rr:datatype` == TTL `rdfs:range` | ✅ 952/952 一致，0 不一致 | 附录 A `verify_extra.py` |
| AC-004 | `true` 边 → joinCondition；child/parent 列真实；带证据注释 | ✅ 70 == 70；70 条全带 `# joinEvidence:` | §3.1 / §2.3 |
| AC-005 | `inherit` 默认不生成；`--include-inherit` 生成且带标记 | ✅ 70 vs 71（差 1）；带 `joinState "inherit"` + `joinEvidence` | §2.3 |
| AC-006 | `false` 任何模式都不生成 join；尾注列候选+原因 | ✅ 两种模式各 0；7 条原因齐全 | §2.2 |
| AC-007 | 租户列写进 mapping | ✅ 43 类 / 54 列，与 TTL **逐类相同** | 附录 A `verify_extra.py` |
| AC-008 | 全量 schema 对账，失败点名 | ✅ 0 失败；变异后 exit 1 + 点名 `表.列` + 不落盘 | §3.1–3.3 |
| AC-009 | rdflib 解析通过 | ✅ 5,878 triples（默认）/ 5,887（含 inherit） | 开头基线表 |
| AC-010 | 两次导出逐字节相同 | ✅ sha256 相同（两组参数各两次） | §8 |
| AC-011 | 复合主键跳过并列入清单 | ✅ 生产快照 0 个（全有单列主键）；未排除快照 1 个 `llm_auth_cache_trigger_state`（复合主键，逐列列出） | §6 / 附录 A |
| AC-012 | 不退化 / 零依赖 / 出口登记 | ✅ 435/4/13/11/12/10；`Cargo.*` 无 diff；I11/I12 已登记 | §9 |

**A-1 ~ A-3**：A-1（不写死常数）—— 代码全部用 `maps.len()` / 实际边数动态计数，测试用夹具数而非 96/72 ✅；
A-2（只覆盖业务本体）—— 12 张排除表 `rr:tableName` 计数 0、只出现在尾注，fail-visible 实测通过 ✅；
A-3（false / 未标 拆分）—— 旧 0/52、新 7/0，52 条无一变 join ✅。

---

## 附录 C · 产物与 hash

| 文件 | 字节 | sha256 |
| --- | --- | --- |
| `excl-default.ttl`（生产口径） | 211,552 | `67A9EE81…BDFDB03` |
| `excl-inherit.ttl` | 211,842 | —（与默认的差异仅 §2.3 那 15 行） |
| `unfiltered-excl.ttl`（fail-visible 对照） | 239,844 | —（仅供对照，不可用于生产） |
| `grouped-v3.ttl`（旧快照对照） | 189,308 | — |
| `model-view.ttl`（类集对账用） | 371,356 | — |

> ⚠️ 上面 `.ttl` 都在 `.review-workspace/`（gitignore）下，**没有**进 `feature/REQ-121`。
