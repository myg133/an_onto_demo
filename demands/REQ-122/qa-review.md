# REQ-122 · SPARQL 查询规划与租户注入 — Pre-merge QA 审核

> **结论：PASS（带 5 条非阻断项，0 阻断项）**
>
> 本需求最重要的判据是「A 查不到 B 的数据」。我用**自己从源库挑的两组真实租户**
> （不是 Dev 用的 `ff48ddaa…` / `0864b1aa…`）逐行对拍：大表 3,622 / 1,134 行**行集与直接 SQL 完全相同、
> 交集 0**；`FILTER != 自己` 与 `FILTER = 别人` 都 **0 行**；两跳与「公共维度当驱动表」的 join
> 里每张客户表都带条件、交集 0。
>
> `sql_echo` 我不仅看代码：**从 `pg_stat_activity` 抓了服务端真正收到的 SQL，与回执逐字相同**。
> 但反向变异证明**测试里没有任何一条断言盯着 `sql_echo`**（见 §3.3）—— 记为非阻断 N1。

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-122` · 分支 `feature/REQ-122` · HEAD `00ddabe`（4 个 commit） |
| 基线 | `develop` `e8f21c8` |
| Dev 交付说明 | commit `00ddabe`（I-1 coverage / I-2 LIMIT N+1 / I-4 DISTINCT） |
| 源库（只读） | `postgresql://…@172.16.120.20:30432/anspire_open_uat`（PG 16.14） |
| 元数据库 | `postgresql://…@172.17.0.87:5432/anspire_real` · 测试库 `ontok_test_req122` |
| 生产快照 | `anspire_r2rml_excl` v2（84 TriplesMap / 952 数据类型 POM / 70 join） |
| schema | `code/out/schema.json` · 声明 `feature-REQ-122/query-policy.json`（42 perTable + 27 publicDimensions） |
| 审核方式 | 只读审核 + **独立 detached worktree** `.review-workspace/req122-qa`（`00ddabe`）做变异 + 自写只读探针 + 直接查库对拍 |
| 未做 | ⛔ 未改 `feature/REQ-122`、未 commit、未 push、未挪 `ontok-gate-preflight.rs` |
| 审核日期 | 2026-10-23 |

**我自己重跑的证据**（全部留在 `.review-workspace/req122-evidence/`，未引用 Dev 的日志数字）：

| 文件 | 内容 |
| --- | --- |
| `leak_check.py` / `leak_check_result.json` | 11 项泄漏对拍（我自己的两组租户） |
| `leak_public_driver.py` | 公共维度当驱动表的额外泄漏攻击（2 项） |
| `policy_checks.py` / `policy_checks_result.json` | 21 项：注入完备 / 声明驱动 / coverage / 拒绝语法 / 限额超时 |
| `extra_probe.py` / `extra_probe_result.json` | 5 项：AC-001 支持语法对拍、echo 逐表文本断言、租户值转义 |
| `echo_runtime.py` / `echo_runtime.txt` | `pg_stat_activity` 抓服务端 SQL ↔ `sql_echo` 逐字比对 |
| `readonly_probe.txt` | 自写只读探针（走生产路径 `sparql::exec::connect_read_only`）5/5 写操作被拒 |
| `develop_red.txt` | 5 条既有红灯在 `develop e8f21c8` 上逐条复现 |

---

## 1 · 结论与 P0 判据总览

| # | 判据 | 结论 |
| --- | --- | --- |
| 1 | 泄漏（AC-005） | ✅ 交集 0（11 项 + 2 项额外攻击，逐行证据见 §2） |
| 2 | `sql_echo` == 执行 SQL | ✅ 运行期逐字相同；⚠️ **测试没盯**（变异全绿，N1） |
| 3 | 注入完备性（AC-004） | ✅ 两跳逐表注入；删表/坏列/无声明一律拒绝并点名 |
| 4 | 声明驱动（ADR-006） | ✅ 代码里 `"customer_id"` 0 处（非注释）；改声明 ⇒ 注入跟着变、代码不动 |
| 5 | coverage 警告（I-1） | ✅ 命中 `main_only` 才警告、说清漏什么；`main_only` 无 `reason` ⇒ 整份声明被拒 |
| 6 | 不支持语法（AC-002） | ✅ 6/6 exit≠0 + 可读错误、点名「v1 不支持 X」 |
| 7 | 只读 + 限额 | ✅ 5/5 写操作被拒；`truncated` / `LIMIT N+1` / 超时都实测 |
| 8 | 不退化 | ✅ lib **516** / doc **4** / `test_req122_sparql` **17** / `test_req122_live` **3**；5 红灯在 develop 复现；红线与 `Cargo.*` 零 diff；I13 登记正确 |

---

## 2 · 泄漏测试（AC-005，P0 最重要）

### 2.1 我自己挑的租户（与 Dev 的 `ff48ddaa…` / `0864b1aa…` 不同）

| 组 | 租户 | 数据规模（直接 SQL 实数） |
| --- | --- | --- |
| P1（大表） | **A1** = `08165fbc-c7e7-47a8-921e-0dd2087dd159` | `free_amounts` **3,622** 行 · licenses 5 · order 9 · balances 1 · dim_customer_binding 1 |
| P1（大表） | **B1** = `27b77151-d6eb-466d-a9b6-eff0968d6dca` | `free_amounts` **1,134** 行 · licenses 3 · order 2 · balances 1 |
| P2（两跳） | **C2** = `159b7348-0d75-45b3-8168-0455093d6850` | `usage_details` **17** · `customer_subscriptions` **2** · balances 1 |
| P2（两跳） | **D2** = `254c803f-6f5a-4e10-95ee-40e23c00f405` | `usage_details` **11** · `customer_subscriptions` **2** · balances 1 |

### 2.2 逐条结果（11 项全 PASS）

| # | 攻击 | 期望 | 实测 |
| --- | --- | --- | --- |
| L1 | **大表不带任何客户条件** `SELECT ?f ?x { ?f :free_amounts__free_amount ?x }`（`--max-rows 5000`） | 行集 = 直接 SQL 对拍值；与 B 交集 0 | A1 **3,622 行**（对拍 3,622，行集**完全相等**）、与 B1(1,134) **交集 0**、`truncated=false`；B1 反向同样：1,134 = 1,134、交集 0 |
| L2 | `FILTER (?c != <…customers/A1>)` | **0 行**（注入是 AND） | A1 **0 行**、B1 **0 行**；`sql_echo` 里可见注入条件 |
| L3 | `FILTER (?c = <…customers/B1>)`（A 查 B） | **0 行** | A1→B1 **0 行**、B1→A1 **0 行**；`tenant_applied = [free_amounts.customer_id, customers.id]` |
| L4 | **两跳** `usage_details → customer_subscriptions → package_templates` | 每跳客户表都被过滤；交集 0 | C2 **17 行**（对拍 17）/ D2 **11 行**（对拍 11）；`tables=[customer_subscriptions, package_templates, usage_details]`，`tenant_applied=[customer_subscriptions, usage_details]`，`package_templates` 进 `public_dimensions`；**逐行** `customer_id` 全为本租户；交集 0 |
| L5 | `balances`（1:1） | 只返回本租户那一行 | A1 **1 行**、B1 **1 行**，客户列 = 本租户 |
| L6 | 公共维度 `package_templates` | 两租户**完全相同**、`tenant_applied=[]` | A1 19 行 / B1 19 行，`rows` **完全相同**，`tenant_applied=0`，`public_dimensions=[package_templates]` |

**额外攻击（我自己加的，Dev 的 8 项里没有这一形状）**：`package_templates` 当 **FROM 的第一张表**
（驱动表），客户表只在 JOIN 一侧出现（反向 join `:package_templates__customer_subscriptions`）：

```sparql
SELECT ?t ?n ?s WHERE { ?t :package_templates__template_name ?n .
                        ?t :package_templates__customer_subscriptions ?s }
```

| 租户 | 结果 |
| --- | --- |
| C2 | 2 行，订阅 id = 本租户 2 条，与 D2 的 2 条 **交集 0**；`tenant_applied=[customer_subscriptions]` |
| D2 | 2 行，同上，交集 0 |

`sql_echo` 里 `package_templates` 无条件、`customer_subscriptions` 带 `WHERE` 条件 ——
**「公共维度驱动 join」没有把客户表漏掉**。

### 2.3 一句话

⛔ 我没有只测「A 能查到自己的」：每一条都做了 **A/B 双向**、**与直接 SQL 的行集相等**、
**与另一租户的交集 = 0**、以及**逐行客户列核对**。**交集全部为 0。**

---

## 3 · `sql_echo` 是否就是执行的 SQL

### 3.1 运行期证据（不是读代码猜的）

我起了一个轮询线程读 `pg_stat_activity.query`（PG 记录的就是**服务端收到的 SQL 文本**），
同时反复执行同一条查询；抓到的字符串与回执 `sql_echo` **逐字相同**（`echo_runtime.txt`）：

```
sql_echo : SELECT (('…/#free_amounts/' || t0."id"))::text AS "f", … FROM "free_amounts" t0
           WHERE t0."customer_id" = '08165fbc-…' AND t0."free_amount" IS NOT NULL
           LIMIT 5001 /* REQ-122: 上限 5000 行 + 1 行探针（判断 truncated），探针不进结果 */
captured : （与上面逐字相同）                      exact_match = 1/1
```

结构上也只有一处来源：`src/sparql/mod.rs:101-109` 执行用 `planned.sql`，
`mod.rs:126` `sql_echo: planned.sql` —— **同一个变量**，不存在第二份 SQL。

注入在 `WHERE` **最前面**、`LIMIT = effective_limit + 1` 且带探针注释，肉眼即可核对
（`src/sparql/plan.rs:851-864`、`plan.rs:936-940`）。

### 3.2 反向变异：**注入从执行里拿掉、echo 保留** ⇒ 必须有测试红

在独立 worktree 里改 `run_query`：exec 传入「剥掉全部注入条件」的 SQL，`sql_echo` 保持原样。

```
test ac005_a_tenant_never_sees_another_tenants_rows ... FAILED
panicked at tests\test_req122_live.rs:180: assertion `left == right` failed: 行集必须与直接 SQL 一致
test result: FAILED. 2 passed; 1 failed   （lib 516 passed / test_req122_sparql 17 passed）
```

✅ **被抓**（且是唯一抓住它的判据：真库行级对拍）。这条变异说明 live 测试不是摆设。

### 3.3 正向变异：`sql_echo` 换成**不含租户条件**的字符串（执行仍用真 SQL）

| 变异 | 测试结果 |
| --- | --- |
| M1：echo 去掉**第一条**注入条件 | `test_req122_sparql` **17 passed** / `test_req122_live` **3 passed**；lib 514 passed + 2 failed —— 这两条是**我漏设 `ONTOK_TEST_DATABASE_URL`** 的环境错误（`db/apply.rs:220`、`extract/introspect.rs:778` 的 `.expect(...)`），与 echo 无关 |
| M1b：echo 去掉**全部**注入条件（`tenant_applied.len()` 条，环境补齐后复跑） | lib **516 passed** / `test_req122_live` **3 passed** / `test_req122_sparql` **17 passed** —— **仍然全绿** |

⇒ **没有任何测试断言 `QueryResult.sql_echo` 的内容**（`grep sql_echo tests/` 只出现在
`assert_eq!` 的失败消息与一个序列化字段清单里；单元测试断言的是 `Planned.sql`，不是 echo 字段）。
**记缺口 N1**（非阻断：今天是同一个变量，行为正确；但一旦有人把 echo 另写一份，测试不会红）。

---

## 4 · 注入完备性（AC-004）

| # | 检查 | 结果 |
| --- | --- | --- |
| 4a | 一次两跳：`tables[]` 里每张**非公共维度**表都在 `tenant_applied[]` | ✅ `tables − public = {usage_details, customer_subscriptions}` ⊆ `tenant_applied`，缺 = **0** |
| 4b | `sql_echo` 里**逐表**可见注入条件（文本判据） | ✅ 两跳 echo 里 `"customer_id" = '<tenant>'` 出现 **2 次**（usage_details 的 `t0`、customer_subscriptions 的 `t1`） |
| 4c | 声明里**删掉一张表** ⇒ 查询该表被拒并**点名** | ✅ 删 `usage_details` ⇒ `exit=1`：`✗ 租户过滤无法确定：表 usage_details 既没有在声明里登记过滤列，也不在 publicDimensions —— ⛔ 整条查询拒绝…` |
| 4d | 声明里的列在 schema 里**不存在** ⇒ 拒绝（不许跳过继续） | ✅ `balances.tenantColumn = no_such_column` ⇒ `exit=1`：`✗ 声明里登记的 balances.no_such_column 在 schema 里不存在 —— ⛔ 不会「跳过这一条继续跑」` |
| 4e | **没有 `--policy`** ⇒ 拒绝 + 打印模板 | ✅ `exit=1`，报错含 `--policy`、`query-policy.example.json`，并**完整打印模板**（含 `"tenantSubject"`） |
| 4f | 声明列写成一个**合法但库里不存在**的列名（`tenant_no`） | ✅ `exit=1`，点名 `balances.tenant_no` —— **不猜列、不静默放行** |

另外：`licenses` 查询（`JOIN customers`）的 echo 里**两张表都有注入**：
`t0."customer_id" = '…' AND t1."id" = '…'` —— join 出来的客户表也不是漏点。

---

## 5 · 声明驱动（ADR-006 的核心）

### 5.1 grep 证据

```text
$ git grep -n '"customer_id"' -- src/sparql/
src/sparql/policy.rs:11://! 判据一：`git grep -n '"customer_id"' -- src/` 里没有一处**用于注入决策**；
```

- 全模块只有 **1 处**，且是**文档注释**（在描述这条判据本身）；
- 排除注释行后，`src/sparql/*.rs` 里 `"customer_id"` / `"main_customer_id"` 字面量 **0 处**；
- 注入决策只走 `policy.decide()`（`policy.rs:231`）→ `decide_tenants()`（`plan.rs:686`）→
  `where_clause`（`plan.rs:851`），**唯一一处实现**（纪律 H）。

### 5.2 改声明 ⇒ 注入跟着变（代码一行不动）

| 动作 | 结果 |
| --- | --- |
| `balances.tenantColumn: customer_id` → **`source_customer_id`**（真实存在的另一列） | `tenant_applied` 列名跟着变成 `source_customer_id`；echo 的 WHERE 首条件变成 `t0."source_customer_id" = '…'`，`t0."customer_id"` **不再出现**；返回 **0 行** = 直接 SQL `count(*) where source_customer_id = A1` 的 **0**（改前：1 行 = 对拍 1） |
| `deductions.coverage: main_only` → **`complete`** | 警告**消失**（改前 1 条 → 改后 0 条，见 §6） |

> 代码侧 `git status` 全程干净（feature worktree 未被我改动；声明只改**副本**，
> 副本在 `.review-workspace/req122-evidence/policy-*.json`）。
> 这正是 ADR-006 要的：「换个客户库只改声明」。

---

## 6 · coverage 警告（I-1）

| 判据 | 期望 | 实测 |
| --- | --- | --- |
| 查 `deductions`（`main_only`） | 有警告，且**说清可能漏什么** | ✅ 1 条：`表 deductions：租户过滤用 customer_id，覆盖范围 = main_only —— main_customer_id 全空（0/4）；子账号产生的扣费不在结果内，金额可能偏低（依据见声明里的 reason）—— ⛔ 不要把 0 行或偏小的数字读成「没有业务」` |
| 查 `balances`（`complete`） | `warnings` = 0 | ✅ `[]` |
| 只查公共维度 `package_templates` | 0 | ✅ `[]` |
| `coverage: main_only` 但**没写 reason** | **整份声明被拒** | ✅ `exit=1`：`租户声明 … 不合法（1 处）：… 声明了 coverage = "main_only" 却没写 reason` |

补充：同一条查询里 `complete` 的表**不会**被连带报警 ——
`SELECT ?d ?c { ?d :dim_customer_binding__customers__main_customer_id ?c }`（命中 `dim_customer_binding`
（main_only）+ `customers`（complete），两表都注入）⇒ `warnings` 恰好 **1 条**，只点名
`dim_customer_binding`。警告正文优先取声明里的 `coverageNote`，回退 `reason`（`plan.rs:703-718`）。

---

## 7 · 不支持语法必须报错（AC-002）

6 条抽查，**全部 exit≠0 且错误可读、点名「v1 不支持 X」**，没有一条静默忽略：

| 语法 | exit | 错误首行（截断） |
| --- | --- | --- |
| `OPTIONAL` | 1 | `✗ v1 不支持 OPTIONAL —— 可选匹配会让「没有这条关系」和「值为空」混成一件事（⛔ 不会静默忽略该子句）（位置 60）` |
| `UNION` | 1 | `✗ v1 不支持嵌套的图模式 / UNION / 子查询（`{ … }` 里再出现 `{`）…（位置 19）` |
| 子查询 | 1 | 同上（同一道闸） |
| 属性路径 `+` | 1 | `✗ v1 不支持属性路径（`+`）—— 只支持单个属性 IRI；多跳请写成多个三元组…（位置 44）` |
| `CONSTRUCT` | 1 | `✗ v1 只支持 SELECT，不支持 CONSTRUCT —— CONSTRUCT 产出图、不产出表（位置 1）` |
| `REGEX` | 1 | `✗ v1 不支持函数 `regex` —— 只支持 = != < > <= >=、&& \|\| ! 与括号（位置 50）` |

副作用（正面）：未知属性/未知类同样报错并给可用清单（我误写谓词时亲测：
`✗ 属性 :deductions__customers 不存在 —— 类 deductions 里没有这个属性`），⛔ 不返回空结果。

---

## 8 · 只读 + 限额

### 8.1 只读（AC-008）—— 我自己写的探针

不是读 Dev 的日志、也不相信回执里那行字：在独立 worktree 里写了
`tests/qa_readonly_probe.rs`，**直接调用生产路径** `ontok::sparql::exec::connect_read_only`
（= `sparql_cmd` 第 ⑥ 步用的同一个函数），在它给出的会话里尝试写：

```
① transaction_read_only = on · default_transaction_read_only = on
② CREATE TEMP TABLE  → cannot execute CREATE TABLE in a read-only transaction
② INSERT …           → cannot execute INSERT in a read-only transaction
② UPDATE …           → cannot execute UPDATE in a read-only transaction
② DELETE …           → cannot execute DELETE in a read-only transaction
② CREATE TABLE …     → cannot execute CREATE TABLE in a read-only transaction
被「read-only transaction」拒绝的语句：5/5
```

另外 `standard_conforming_strings = on` 也被 `connect_read_only` 读回校验（不是 `on` 就拒绝执行，
否则反斜杠可拼出第二个引号）。

### 8.2 限额与超时（AC-007）

| 判据 | 实测 |
| --- | --- |
| `--max-rows 10` 对大结果集 | **10 行** + `truncated: true` + echo 里 `LIMIT 11 /* … + 1 行探针 … */` |
| 结果**正好** = 上限（3,622 行，`--max-rows 3622`） | `truncated: false`（探针行确实区分了「刚好 N 行」与「还有更多」） |
| 查询写 `LIMIT 5000` 但 `--max-rows 10` | 压到 10，`truncated: true`，并给 warning「LIMIT 5000 超过 --max-rows 10，已压到 10」 |
| `--timeout-ms 1` | `exit=1`：`✗ 查询超时（1 ms，服务端 statement_timeout 已中止）—— 缩小范围或调大 --timeout-ms。` |
| 超时之后连接是否坏 | 紧接着一条正常查询 `exit=0`、`row_count=1` |
| `--max-rows 0` / `--timeout-ms 0` / `--tenant ''` | 三条都 `exit=1` 且各有一句人话（「0 行等于什么都没查」/「那是禁用不是超时」/「没有租户就没有隔离」） |
| `OFFSET`（在 `LIMIT … /* 注释 */` 之后） | 正常：`LIMIT 4 /* … */ OFFSET 2` 执行成功、返回 3 行 |
| `--tenant` 填经典注入载荷 `x' OR '1'='1` | 转义成**一个**字面量：`t0."customer_id" = 'x'' OR ''1''=''1'` ⇒ **0 行**，不放大 |

---

## 9 · 不退化（AC-010）

### 9.1 我自己重跑的计数（全量 `cargo test --no-fail-fast`）

| 项 | 期望 | 实测 |
| --- | --- | --- |
| lib | 516 | **516 passed / 0 failed** ✅ |
| doc | 4 | **4 passed** ✅ |
| `test_req122_sparql` | 17 | **17 passed** ✅ |
| `test_req122_live` | 3 | **3 passed**（带真库 + 两个真实租户**真跑**，不是 SKIP；M2 变异能让它红，见 §3.2） |
| 全量 | 只有既有红灯 | 4 个 target 红、共 **5 条**（见 9.2） |

守卫类套件逐个单跑，全绿：`test_anchor_guard` 3 · `test_guard_manifest_req081` 6 ·
`test_read_only_guard` 5 · `test_property_filter` 10 · `test_req107_guard_granularity` 4 ·
`test_req107_outlet_pipeline` 12 · `test_req107_source_enumeration` 10 · `test_binary_output_guard` 7。

### 9.2 5 条既有红灯 —— 我在 `develop e8f21c8` 上逐条复现

在 `code/`（develop worktree，`e8f21c8`）跑同 4 个 suite：**同样 5 条红、同样的文案**
（`develop_red.txt`）：

| # | 用例 | 失败首行 | 归因（develop 上同样失败） |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | `读不到脱敏 schema.json：../.review-workspace/fix-props/schema.json（os error 3）` | 外部夹具缺失（该文件在本机不存在），环境问题 |
| 2 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | `items 是提案的实质内容…` —— 右侧多一条 `add_link_type(t_dev→t_customer)` | REQ-120 FK 补全留下的基线漂移，与 SPARQL 无因果 |
| 3 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | `读不到脱敏 schema.json：/home/myg133/…/fix-props/schema.json` | 同一份夹具缺失（且是硬编码的他人路径） |
| 4 | `test_req084_every_registered_name_is_still_literally_at_its_line` | `src/extract/propose.rs:280:60 —— 登记说这里保留着表名 customer_rebate，但该位置上是…` | 登记行漂移；该文件**不在** REQ-122 的 diff 里 |
| 5 | `test_req084_no_unregistered_real_name_remains_in_the_registered_files` | `src/domain/concept.rs 里实际出现 9 处已登记表名，登记表只记了 5 处` | 同一登记表漂移的连带；`concept.rs` 不在 diff 里 |

> 结论：**没有一条红灯是 REQ-122 引入的**。5 条在合入后仍会红（建议另行开单）。

### 9.3 禁改面 / 零依赖 / 无放水

```
$ git diff --numstat e8f21c8..HEAD -- src/mcp/tools.rs src/domain/property_filter.rs \
    src/server/ src/domain/view.rs tests/test_property_filter.rs tests/test_anchor_guard.rs \
    tests/test_guard_manifest_req081.rs tests/test_read_only_guard.rs Cargo.toml Cargo.lock
（空）
```

- `guard()`（`src/mcp/tools.rs`）无 diff；`property_filter.rs` G1–G4 无 diff；
  `/api` 鉴权与路由（`src/server/**`）**0 行** diff；`Cargo.toml` / `Cargo.lock` **零 diff**（零新依赖）。
- `tests/` 里 **没有** 新增 `#[ignore]`、**没有** 删除 `#[test]`；
  既有测试只改了「出口数 47→48」这类**必须改的精确计数**（仍是 `assert_eq!` 精确相等，不是 `>=`）。
- 全部 diff 只有 18 个文件：`src/sparql/**`（新增）、`src/cli.rs`、`src/domain/{pipeline,r2rml,ttl}.rs`、
  `src/lib.rs`、`query-policy*.json`、`tests/test_req122_{sparql,live}.rs`、`tests/test_req107_*`（出口登记）。

### 9.4 新出口 I13 登记正确

| 判据 | 实测 |
| --- | --- |
| `OUTLETS` 有 `I13`，`Track::Internal` + `Enforcement::InternalOnly` + `Format::Json` | ✅ `src/domain/pipeline.rs` 新增条目，`call_site = "\"{}\", serde_json::to_string_pretty(&result)?)"`（`sparql_cmd` 唯一那处 stdout） |
| 条数口径同步 | ✅ 48（客户 17 / 建模 10 / **内部 18** / 静态 2 / 错误路径 1）；`registry_is_the_declared_baseline` 与 `test_req107_outlet_pipeline` 都是**精确相等**断言，且已绿 |
| 模块/函数枚举 | ✅ `test_req107_source_enumeration` 新增 `sparql_cmd` 与 `src/sparql/*` 各模块（10 passed） |
| 为什么不进客户轨 | 条目里写明「它是查询结果，不是客户视图；隔离由声明驱动的注入 + 只读 + 行数上限保证」——与 REQ-121 的 I11/I12 同一口径 |

### 9.5 我自己的污染检查

- `feature-REQ-122` 的 `git status` **干净**（我未改动任何被跟踪文件；`.scratch/` 已 gitignore）；
- 变异只在独立 detached worktree `.review-workspace/req122-qa`（`00ddabe`）里做，已 `git checkout` 还原；
  自写探针 `tests/qa_readonly_probe.rs` 是**未跟踪文件**，只存在于该 worktree（审核结束后回收）；
- 未 push、未 commit、未动 `ontok-gate-preflight.rs`。

---

## 10 · 阻断项 / 非阻断项

### 阻断项：**无**

### 非阻断项（5 条）

| # | 项 | 证据 | 建议 |
| --- | --- | --- | --- |
| **N1** | **没有任何测试断言 `QueryResult.sql_echo`**：把 echo 里的**全部**租户条件拿掉（执行不变），lib 516 / 17 / 3 **全绿** | §3.3（M1b） | 加一条端到端判据：把回执里的 `sql_echo` 原样再执行一次，**行集必须与回执相同**；或至少断言每条 `tenant_applied` 都对应 echo 里的一段 `<alias>."<col>" = '<tenant>'`。这样「echo 与执行漂移」才会红 |
| **N2** | `test_req122_live` 缺环境变量时**打印 SKIP 并算 pass** —— 最重要的 AC-005 在 CI 里可能**静默不跑** | `tests/test_req122_live.rs:66-68`（Dev 是刻意的：不用 `#[ignore]`） | 给它一个 `REQ122_REQUIRED=1`（缺环境 ⇒ fail）模式供 CI 用；或让 CI 统计 SKIP 行数 |
| **N3** | CLI 的只读回执是一行**写死的字符串**（不是读回值）：`eprintln!("· 只读会话：transaction_read_only = on …")` | `src/cli.rs:3918-3920` | 今天由 `connect_read_only` 的读回校验兜住（`exec.rs:34-44`），所以事实为真；但建议把读回的值带出来打印，否则将来有人削弱校验时这行仍会说 `on` |
| **N4** | 声明里登记了**不存在的表**时只 `warning`（表不存在 schema 里也不报错）；只有「schema 里有这张表、但列不存在」才拒绝 | `plan.rs:136-149` | 风险低（查询只能命中映射里的类，未登记的表查不到 ⇒ fail-closed），但一个拼错的表名会得到「看起来生效了」的声明；建议与列不存在的口径统一为**拒绝** |
| **N5** | 文档口径滞后：`acceptance.md` I-5 写「lib 509 / 新增集成 15 + 真库 2」，最终是 **516 / 17 / 3**（`00ddabe` 的 I-1 用例）；另 `feature-REQ-122/.feature/manifest.json` 内容还是 **REQ-116**（process 记录） | acceptance.md:152-160；`.feature/manifest.json` | 请 BA 订正 I-5 数字；manifest 建议按 REQ-122 重写（不影响代码） |

### 已接受的已知限制（不是发现）

- `balance_transfer` 一笔划转有两个客户列，声明只能登记一列 ⇒ 先登记 `from_customer_id`
  （收款方可见性待业务确认）—— 声明里已写明，ADR-006 的已知限制；
- `deductions` / `dim_customer_binding` 用 `customer_id` ⇒ `coverage: main_only`，回执会报警（BA 已裁定）；
- 子账号 id → 主账号的解析层不在本版（ADR-006 Limitation）。

---

## 附录 A · 我跑的命令（可复现）

```powershell
# 环境
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
$env:ONTOK_SOURCE_DATABASE_URL='postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat'
$env:ONTOK_TEST_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/ontok_test_req122'

# 泄漏 / 声明 / 拒绝 / 限额（我自己的脚本）
python .review-workspace/req122-evidence/leak_check.py
python .review-workspace/req122-evidence/leak_public_driver.py
python .review-workspace/req122-evidence/policy_checks.py
python .review-workspace/req122-evidence/extra_probe.py
python .review-workspace/req122-evidence/echo_runtime.py

# 全量测试（feature worktree）
cd feature-REQ-122; cargo test --no-fail-fast   # lib 516 / doc 4 / 122_sparql 17 / 122_live 3；5 红皆为既有

# 独立变异 worktree（detached 00ddabe，未改分支）
git worktree add --detach .review-workspace/req122-qa 00ddabe
# M1b：mod.rs run_query 里 sql_echo 剥掉全部注入；M2：exec 用剥掉注入的 SQL、echo 保留
cargo test --lib --test test_req122_sparql --test test_req122_live

# 自写只读探针（生产路径 connect_read_only）
cargo test --test qa_readonly_probe -- --nocapture
```

## 附录 B · 关键证据原文

**泄漏（L1，A1 查大表）**

```
返回 3622 行 · 对拍 3622 行 · 与另一租户(1134 行)交集 0 · truncated=False · 行集相等=True
```

**两跳 echo（逐表可见）**

```sql
SELECT (('https://ontok.local/anspire_r2rml_excl#usage_details/' || t0."id"))::text AS "u",
       (('…/#customer_subscriptions/' || t1."id"))::text AS "s",
       (('…/#package_templates/' || t2."id"))::text AS "t"
  FROM "usage_details" t0
  JOIN "customer_subscriptions" t1 ON (t1."id" = t0."subscription_id")
  JOIN "package_templates" t2 ON (t2."id" = t1."template_id")
 WHERE t0."customer_id" = '159b7348-…' AND t1."customer_id" = '159b7348-…'
 LIMIT 5001 /* REQ-122: 上限 5000 行 + 1 行探针（判断 truncated），探针不进结果 */
```

（`t2` = `package_templates` 是公共维度，**没有**条件；两张客户表各有条件。）

**声明驱动（改列名）**

```sql
-- 改前
… WHERE t0."customer_id" = '08165fbc-…' AND t0."recharge_balance" IS NOT NULL LIMIT 1001 …
-- 改后（只改 query-policy.json 副本，代码零改动）
… WHERE t0."source_customer_id" = '08165fbc-…' AND t0."recharge_balance" IS NOT NULL LIMIT 1001 …
```

---

> **一句话给 BA**：机制、口径、注入、只读、拒绝、限额都成立，泄漏实测交集 0，可以合。
> 唯一值得在合并前顺手补的是 **N1**（一晚就能补：执行回执里的 `sql_echo` 再对拍一次行集）——
> 它是本需求「`sql_echo` 就是执行 SQL」这条卖点的自动守卫，目前只靠人眼与我这次的手工抓包。
