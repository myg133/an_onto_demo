# 验收标准：REQ-119 · 本体可查询化

> **一句话**：把 TTL 从「给人看」升级成「给 Agent 查」——**能反向走、能判断类型、能对上 IRI、能隔离租户**。

## 一 · AC

### AC-001 · 反向属性完整且对称

- 每条对象属性都有反向属性：`owl:inverseOf` **双向**声明
- 反向属性的 `rdfs:domain` / `rdfs:range` 与前向**对调**
- 命名规则：`<对象>__<主语>`（与正向 `<主语>__<宾语>` 对称）
- 判据：对象属性 52 条 ⇒ 反向属性 52 条、`owl:inverseOf` 104 条（双向各一条）
- 判据：抽查任意一条，正反两个方向的 domain/range 确实对调

### AC-002 · xsd range 映射

- 数据类型属性的 `rdfs:range` 按列类型映射到 `xsd:decimal` / `xsd:integer` / `xsd:dateTime` / `xsd:string` / `xsd:boolean` / `xsd:date` 等
- 判据：`rdfs:Literal` 只允许出现在**映射失败**的属性上，且这类必须计数
- 判据：数值列的属性 range 必须是 `xsd:decimal` 或 `xsd:integer`（抽查 ≥10 条，含「金额」「次数」「用量」）
- **反判据**：不允许出现「895 条全 `rdfs:Literal`」的现状

### AC-003 · IRI 模板

- 每个类声明实例 IRI 模板（如 `ontok:instanceIri "http://ontok.local/anspire_real#{class}/{id}"`）
- 判据：83 个类**每个都有**；模板里的占位符有文档说明
- 判据：模板是**确定性的**（不含时间/随机）
- 要求：IRI 约定写进模块文档（R2RML 必须产出同一个 IRI）

### AC-004 · 租户标注（安全前提）

- `tenancy.json` 声明：`{columns: [...], tables?: [...], reviewed_by}` ⇒ 命中的属性标 `ontok:tenantKey true`，其类标 `ontok:tenantColumn`
- 判据：声明的列真的被标上；未声明的列**不被标**
- 判据：`reviewed_by` 为 null 时，TTL 里必须体现「未签」状态（R5）
- 判据：**租户标注只出现在建模视图 TTL**，客户视图不含

### AC-005 · 未标注时显式警告（fail-visible）

- 没有 `tenancy.json` ⇒ 导出成功，但：
  - 文件头注释写「**未标注租户键**」
  - TTL 里有 `ontok:tenantUnannotated true`
  - stderr 警告「这份本体不能直接给数字员工用」
- 判据：三处都有；⛔ 不许静默产出「看着能用」的本体

### AC-006 · 确定性

- 同一版本两次导出 ⇒ **逐字节相同**（两次 sha256 一致）
- 判据：反向属性、xsd range、IRI 模板的排序都稳定

### AC-007 · 客户视图 vs 建模视图

| 项 | 客户视图 | 建模视图 |
| --- | --- | --- |
| 反向属性 | ✅ 有 | ✅ 有 |
| xsd range | ✅ 有 | ✅ 有 |
| IRI 模板 | ✅ 有 | ✅ 有 |
| 租户标注 | ⛔ **无** | ✅ 有 |

判据：两份各导一次，逐项核对

### AC-008 · 体积与不退化

- TTL 体积增长 ≤ 40%（当前客户视图 215.5 KB）
- 四红线 + `tools.rs` + `guard()` + `property_filter.rs` 不改；`Cargo.*` 零改动
- 基线：lib 355 / doc 4 / 稳定

### AC-009 · **最终验证：Agent 能不能只读 TTL 写出对的 SPARQL**（不执行）

给 Agent **只看升级后的 TTL**（不给表结构、不给库），让它为 5 个业务问题写 SPARQL：

| # | 问题 | 期望用到的能力 |
| --- | --- | --- |
| 1 | 某客户上个月所有扣费明细 | 反向属性（客户 → 扣费） |
| 2 | 某客户当前余额 | 属性定位 + xsd 类型 |
| 3 | 某订阅还剩多少额度 | 反向 + 数值属性 |
| 4 | 某客户用过哪些 API（按授权） | 两跳反向（客户 → 授权 → 调用明细） |
| 5 | 某套餐下有哪些订阅 | 反向（套餐 → 订阅） |

- 判据：**逐条人工判**「能不能写对」；每条的错因归类（缺反向 / 缺类型 / 缺信息）
- ⛔ 只写不执行——执行与安全限流属 REQ-121
- 判据：至少 4/5 能写出**方向正确**的 SPARQL；写不出的必须归因到具体缺口

### AC-010 · 文档

- IRI 模板约定、反向属性命名规则、xsd 映射表（列类型 → xsd）三份文档
- 判据：写在 `ttl.rs` 模块头 + `BA/demands/REQ-119/design-notes.md`

## 二 · 纪律

- **H** 反向属性生成只许一处实现
- **G** 变异三元组：删反向属性 / range 全改 Literal / 删租户标注，每条必须被测试抓住
- **S** AC 也会错，发现直接说

---

## 三 · Dev 交付后的裁定（含 BA 自己的误判）

### A-1 · xsd range 必须来自**真实列类型**（必修）

Dev 首版按列名/中文关键词**启发式**推断（快照层 `snapshot::Property` 没有 `data_type`）。
BA 用 `code/out/schema.json` 的真实 `data_type` 逐条比对 870 条：

```
一致 754 · 不一致 116 · 一致率 86.7%
```

**典型错误（每一条都会让数字员工算错）**：

| 表.列 | 真实 | 旧 range | 后果 |
| --- | --- | --- | --- |
| `activity.status` | integer | `xsd:string` | `status = 2` 变字符串比较，查不到 |
| `activity_keys.daily_ip_limit` | integer | `xsd:decimal` | 计数当小数 |
| `agent_device_version.version_number` | varchar | `xsd:integer` | 版本 "10" vs "9" 排序反了 |
| `balance_operation_log.point_change` | numeric | `xsd:integer` | **金额丢精度** |

**要求**：`--schema <path>` 匹配到列 ⇒ 用真实类型 + `ontok:rangeSource "schema"`；匹配不到 ⇒ 启发式 + `rangeSource "heuristic"` + 计数；未给 `--schema` ⇒ 三处标记「类型来自启发式」。
**判据**：`--schema` 模式下 870/870 一致（`verify` 例外单列）。

### ⚠️ A-1b · BA 自己的误判，记录在案

第一轮抽查我只看了 8 条（全在 `activity` 表、恰好都是无歧义列）⇒ 得出「启发式很准」的乐观结论。
**样本选窄导致结论偏乐观** —— 与之前「AC-004 恒真」「闸没接线」属同一类：**判据的覆盖面不足**。

### A-2 · 反向命名 1 条例外：接受

`customer_subscriptions__order__inverse`（`order ↔ customer_subscriptions` 互为反向、与正向撞名）。
要求：文档写清为何加 `__inverse` 后缀；加测试断言「撞名时加后缀、计数仍 52/104」。

### A-3 · R5 三处标记只在建模视图：接受

R4 明确「客户视图不含租户信息」，`tenantUnannotated` 本身也是建模/安全信息。
要求：报告写清「客户视图天然不含租户信息，R5 的警告只对建模视图有意义」，免得 QA 用客户视图验 R5 时误判。

---

## 四 · AC-009 替身实验结果（5 题写 SPARQL，只读 TTL、不执行）

### 结论：**够写「形状」，不够写「口径」**

语法层面 5/5 都能写出可执行 SPARQL；**端到端能直接交付客户的只有 1 条**。

| 题 | 结果 | 卡点 |
| --- | --- | --- |
| Q1 扣费明细 | ✅ **高**，可直接交付 | 「上个月」边界需服务端注入（SPARQL 1.1 无 `YEAR()`）；时区未定义；`deductions` 有 2 个 tenant 键，走哪个不明确 |
| Q2 余额 | ❌ **低** | 类选对了，但「余额」落不到任何单列：`recharge_balance` / `transfer_point` / `credit_balance_rmb/usd` 四套互不通约，**无 total、无汇率** |
| Q3 额度 | ⚠️ 中 | 能算数值，**说不出产品名**（`product_id` 裸串，无 ObjectProperty） |
| Q4 用过的 API | ⚠️ 中 | 一条活路（经 `licenses`）；字面最贴题的 `api_call_logs` **整类 0 边** |
| Q5 套餐下订阅 | ⚠️ 中 | 能写，但**默认跨租户**（TTL 无「套餐内订阅项」类） |

**判定（替身原话）**：「宁可说不够。按现在这份 TTL，我不会让数字员工把 Q2/Q3/Q4 的结果直接发给客户。」

### B-1 · **真 bug**：`Functional` / `InverseFunctional` 整体反了

BA 独立核实（全量统计）：

```
('N:1', 'InverseFunctional')  52 条   ← 应为 FunctionalProperty
('1:N', 'Functional')         52 条   ← 应为 InverseFunctionalProperty
```

后果（替身原话）：「56 条 1:N 反向边全被错标成 Functional ⇒ **开推理会把一个套餐下的所有订阅合并成一条**」。

### B-2 · 主键没导出成属性 ⇒ 字符串 FK 无法还原成 IRI

「license 只有 `license_key`/`name`，没有 `id` ⇒ **不能 join 的技术根因**」。
要求：主键列导出为 `owl:DatatypeProperty` + `ontok:isPrimaryKey true`，类上 `ontok:primaryKeyColumn`。

### B-3 · 枚举值域没导出 ⇒ 魔数猜反不报错

`status`(1/2/3/4/5)、`currency`(POINT/RMB/USD) 全在 label 的中文串里。
要求：从列注释**确定性解析**允许值 → `ontok:allowedValue`；解析不出不导出（不许编）。

### B-4 · FK 边缺失（**不在本需求修**，另开需求）

`api_call_logs` / `package_products` / `service_config` / `llm_model` / `customer_account_binding` 整类 0 个 ObjectProperty；
`product_id` / `model_id` / `service_id` / `main_customer_id` / `sub_customer_id` 全是裸串。

**根因是抽取阶段词干匹配太严**（`product_id` → 找 `products`，实际表叫 `package_products`），不是 TTL 的问题。
⇒ 另开需求改抽取，本需求只登记。

### 替身最想要的一样东西

> **把主键暴露成 datatype property + 把所有 FK 列导出成 ObjectProperty（补上 api_call_logs/product_id 等漏掉的）。**
> 「5 个孤岛类 + 主键缺失叠加，导致两跳以上的业务问题全写不出；补上这一样，端到端答对从 1 条变 4 条。」

### 这个实验方法值得保留

它证明了：**「代码全绿」不等于「Agent 能用」**。5 题里 4 题方向对，但只有 1 题能交付——**差的那 3 题全卡在信息缺失，不是语法**。
