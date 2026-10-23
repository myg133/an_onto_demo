# 验收标准：REQ-122 · SPARQL 查询规划与租户注入

> **一句话**：Agent 写 SPARQL，服务端必须保证**它看不到别人的数据**——哪怕它写错了、写全了、写歪了。

## 一 · AC

### AC-001 · 支持的语法真能跑

对以下每条至少一个用例，跑真实库并返回正确结果：

| 能力 | 用例 |
| --- | --- |
| 单跳 | 某客户的扣费明细 |
| 两跳 | 某客户 → 订阅 → 套餐模板 |
| 聚合 | 某客户按币种 SUM 扣费金额 |
| `GROUP BY` | 某客户按 `status` 分组计数 |
| `ORDER BY` + `LIMIT` | 按时间倒序取前 10 |
| `FILTER` 比较 | 金额 > 100 |
| `FILTER` 复合 | `?s = 1 && ?a > 0` |

- 判据：每条有用例、有期望输出（可与直接 SQL 对拍）

### AC-002 · 不支持的语法**必须报错**

| 不支持 | 期望 |
| --- | --- |
| `OPTIONAL` | 报错「v1 不支持 OPTIONAL」 |
| `UNION` | 同上 |
| 子查询 / `BIND` / 属性路径 | 同上 |
| `CONSTRUCT` / `ASK` | 同上 |

- 判据：每种各一个用例，**exit≠0 且错误可读**；⛔ 不许静默忽略子句

### AC-003 · 属性/类写错必须报错

- 未知属性（`:deductions__no_such_column`）⇒ **报错**并列出可用属性（或提示「用 describe 查」）
- 未知类 ⇒ 报错
- 判据：两种各一次；⛔ 不许返回空结果集（空结果会让 Agent 以为「没有数据」）

### AC-004 · 租户强制注入（**由声明驱动**，ADR-006）

| 声明状态 | 期望 |
| --- | --- |
| `perTable` 里登记了过滤列 | 按**声明里写的列**注入（`WHERE <该列> = $tenant`） |
| 表在 `publicDimensions` 里 | 不注入 |
| 表既没登记、也不在公共维度 | **整条查询拒绝**，点名表 + 提示去声明里登记 |
| **完全没有声明文件** | **所有表不可查**，报错 + 给出声明模板路径 |

- 判据：`sql_echo` 里**能看见**注入条件（文本断言，不是「代码里有」）
- 判据：`tenant_applied[]` 覆盖 `tables[]` 里除公共维度外的每一张表
- **判据（关键）**：代码里**搜不到** `"customer_id"` / `"main_customer_id"` 字面量用于注入决策（`git grep` 证据）
- 判据：改声明里的列名（如改成 `tenant_no`）⇒ 注入跟着变，**代码不用动**

- 判据：`sql_echo` 里**能看见**注入条件（文本断言，不是「代码里有」）
- 判据：`tenant_applied[]` 覆盖 `tables[]` 里除公共维度外的每一张表

### AC-005 · **泄漏测试**（本需求最重要）

构造能拿到别人数据的查询，验证注入真的挡住了：

| 攻击 | 期望 |
| --- | --- |
| 主查询不带任何客户条件 | 只返回本租户数据（行数 = 只本租户的数量） |
| `FILTER (?c != :本租户)` 反向筛 | 只返回本租户数据（注入是 AND，不可被 OR 掉） |
| 两跳查询（客户→订阅→套餐）| 每跳涉及的客户表都被过滤 |
| 直接查 `balances`（1:1 表） | 只返回本租户那一行 |

- 判据：**用两个真实客户的数据对拍**：A 客户的查询结果里不得出现 B 客户的任何行（逐行核对 ID）
- ⛔ 不许只测「A 能查到自己的」——必须测「A 查不到 B 的」

### AC-006 · 无法确定租户列 ⇒ fail-closed

- 用一份**不含某表租户列**的公共维度声明 + 查询该表 ⇒ **拒绝**并点名
- 判据：拒绝路径有测试；⛔ 不许「猜一个列名」或「不过滤放行」

### AC-007 · 行数上限与超时

- `--max-rows 10` 对一个大结果集 ⇒ 返回 10 行 + `truncated: true`
- 超时（构造慢查询，如无索引的大表扫描）⇒ 中止 + 可读错误
- 判据：两条各一次

### AC-008 · 只读

- 查询期间 SQL 必为只读会话（`transaction_read_only=on`）
- 判据：写操作（`INSERT`）无法通过 SPARQL 表达——查代码无写路径；运行期 `SHOW transaction_read_only` 或等价证据

### AC-009 · 返回值形状

```json
{ "columns": [...], "rows": [...], "row_count": 12, "truncated": false,
  "sql_echo": "SELECT ... WHERE ... main_customer_id = $1 LIMIT 1000",
  "tenant_applied": [{"table":"deductions","column":"main_customer_id","value":"C001"}],
  "tables": ["deductions","customers"], "warnings": [] }
```

- 判据：字段齐全；`sql_echo` 与 `tenant_applied` 一致（每张非公共表都在里面）

### AC-010 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `/api` 鉴权与路由 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖（先查 `Cargo.toml` 有无可复用解析器；没有就手写子集） |
| 基线 | REQ-121 合并后的 lib 435 / doc 4 |

## 二 · 纪律

- **H** 租户注入只许一处实现（不许 planner 一处、执行器一处）
- **G** 变异三元组：把注入改成 OR / 跳过公共维度判断 / 允许未知属性返回空 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说

---

## 三 · Dev 交付与裁定（I-1~I-4）

### Dev 抛回的问题：声明为什么用 `customer_id` 而不是 `main_customer_id`

BA 实查源库：

```
deductions:           4 行   · main_customer_id 非空 0 条（0%）
dim_customer_binding: 17851 行 · 非空 29 条（0.16%）
```

按 `main_customer_id` 过滤 ⇒ 这 4 条扣费**对任何租户都不可见** ⇒ AC-001/AC-005 都会变空洞。

**裁定：接受 `customer_id`**（Dev 的判断成立）。

### I-1 · **不完整必须让调用方知道**（要加）

`customer_id = :tenant` 的后果：**子账号产生的扣费查不到，且不会报错**。
数字员工会答「您上个月扣费 0 元」，而实际有子账号的扣费——**这比查不到更危险**。

要求（仍声明驱动）：

1. `perTable` 增加可选 `coverage: "complete" | "main_only"`
2. 回执 `warnings[]`：被命中的表若 `coverage != complete` ⇒ 输出可读警告（说明用哪列过滤、可能漏什么）
3. 判据：查 `deductions` ⇒ 有警告；查 `balances`（complete）⇒ **无警告**

### I-2 · `LIMIT N+1` 接受

用 `LIMIT N+1` 判 `truncated` 比 `LIMIT N` 更正确（能区分「正好 N 行」与「被截断」）。
裁定：接受；AC-009 订正为「`LIMIT <= max_rows + 1`」，且 `sql_echo` 要能看出是探针。

### I-3 / I-4 · 接受

| 项 | 裁定 |
| --- | --- |
| 回执多 `public_dimensions[]` | ✅ 保留（否则 AC-004 无法机检） |
| 支持 `SELECT DISTINCT` | ✅ 接受，但必须写进「支持列表」文档，⛔ 不许再悄悄扩 |

### I-5 · Dev 交付的关键数字（待 QA 独立复验）

| 项 | 值 |
| --- | --- |
| 映射一致性 | 84 TriplesMap / 952 属性 / 70 join，与 `r2rml generate` 的 TTL **逐数相同** |
| 声明覆盖 | 42 perTable + 27 publicDimensions + 1 tenantSubject = 70；余 14 张「口径未定 ⇒ 不可查」 |
| 泄漏对拍 | 8 项 PASS，**交集 0**（107 vs 76；balances 1:1；两跳 107；`FILTER != 自己` / `FILTER = 别人` 均 0 行） |
| 拒绝用例 | **26 条**（超出 AC-002 要求） |
| 测试 | lib **520**（第 2 轮 +74；第 5 轮 +4 = N1 三条 + N3 一条）/ doc 4 / `test_req122_sparql` **17** + `test_req122_live` **4**（真库 3 + 库内泄漏 1） |

---

## 四 · QA 裁定（PASS 带 5 条非阻断 → 3 条改判必修）

### QA 独立复验的硬结果（都是它自己重跑的）

| 项 | 结果 |
| --- | --- |
| **泄漏交集** | **0**（自选两组租户，非 Dev 的）；A 返回行集与直接 SQL **完全相等** |
| 反向筛选 | `FILTER != 自己` / `FILTER = 别人` 均 **0 行** |
| **额外攻击（Dev 没测）** | 以**公共维度表当驱动表**、客户表只在 JOIN 一侧 ⇒ **仍被注入**，交集 0 |
| `sql_echo` 是否执行 SQL | **是**：`pg_stat_activity` 抓服务端实际 SQL，与回执**逐字相同** |
| 声明驱动 | `git grep` 非注释行 **0 处**；改声明列名 ⇒ 注入/echo/tenant_applied 全跟着变 |

### M-1 · N1 **改判必修**：没有任何测试断言 `sql_echo` 内容

QA 正向变异：**把 echo 里的租户条件全删掉（执行不变）⇒ lib 516 / sparql 17 / live 3 全绿**。

⇒ 「echo 就是执行的 SQL」**没人守**，而我们所有「注入可视化」验收都建立在它上面。
要求：补断言（echo 含注入条件 + 含 `LIMIT N+1`），并**变异验证**（删 echo 的注入 ⇒ 测试红）。

**→ 第 5 轮已修**：回执组装抽成纯函数 `sparql::receipt()`（`sql_echo` 只从 `planned.sql` 逐字取）；
lib 三条判据把 echo 钉住（`sparql::tests::*`）。同一变异（echo 删注入、执行不变）复跑：
**lib 3 red**（`sql_echo_is_the_executed_sql_…` / `every_applied_tenant_condition_…` / `the_echo_does_not_invent_…`），
`test_req122_live::ac005_testdb_…` 也红。库内测试另含「echo 原样再执行 ⇒ 行集不变」的端到端判据。

### M-2 · N2 **改判必修**：泄漏测试缺 env 时静默 SKIP 且算 pass

AC-005 是**最重要的判据**，CI 上很可能根本没跑。
要求：① SKIP 必须**响亮**（打印「AC-005 未执行」）；② 补一条**不依赖源库**的库内泄漏测试（用测试库造两个租户夹具），保证常规 CI 也能覆盖。

**→ 第 5 轮已修**：① SKIP 四重可见，含**子进程回显**（绕过 libtest 的线程级 output capture）⇒
默认 `cargo test` 输出里就有 `*** SKIP: <测试名> -- missing <ENV> *** … NOT a pass ***`；
② 新增 `ac005_testdb_a_tenant_never_sees_another_tenants_rows`（只需 `ONTOK_TEST_DATABASE_URL`：
自建专属 schema + 两表 + 两租户，走完整 `run_query`，A/B 交集 0、越权 FILTER 0 行）。
空转自证：把 `orders` 改成公共维度 ⇒ A 拿到 4 行 ⇒ 该测试红。

### M-3 · N3 **改判必修**：CLI 的「只读回执」是**写死的字符串**

QA 实测：打印的「`transaction_read_only = on`」是硬编码，不是读回值。——**这是假回执**，与「失败路径不许装作做完了」同一类。
要求：真读回（`SHOW transaction_read_only`）或删掉该断言，⛔ 不许保留没验证的断言。

**→ 第 5 轮选 (a) 真读回**：`connect_read_only` 返回 `ReadOnlyConnection { pool, facts }`，
`SessionFacts::receipt()` 打印的就是 `SHOW` 的实际值（CLI 输出「只读会话（读回）：…」）。
变异自证：把 `receipt()` 里的值换回写死的 `"on"` ⇒ lib 单测红（喂 `off` 时回执必须写 `off`）。

### M-4 · 登记

| # | 项 |
| --- | --- |
| N4 | 声明登记「不存在的表」只 warning ⇒ **下轮改成拒绝**（与「无法确定 ⇒ 拒绝」一致） |
| N5 | acceptance I-5 数字滞后（509/15+2 → 516/17+3）、`.feature/manifest.json` 仍是 REQ-116 ⇒ **本轮顺手更新** |

**→ 第 5 轮已更新**：I-5 的「测试」行改为 lib **520** / doc 4 / sparql **17** / live **4**；
`feature-REQ-122/.feature/manifest.json` 已按 REQ-122 重写（不再是 REQ-116 的内容）。
