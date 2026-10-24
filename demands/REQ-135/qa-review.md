# QA 审核报告：REQ-135 · 生产可观测性（`/metrics` + 审计默认 stdout）

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-135` @ `4cda262`（基线 `develop` `5163cfb`，**1 个提交**） |
| 审核性质 | **Pre-merge**（P0 上线门槛；REQ-136 在等它合入） |
| 审核时间 | 2026-10-24 |
| 审核环境 | Windows · Rust 1.96.0 `cargo build` debug · 元数据库 `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real` · 源库 `ONTOK_SOURCE_DATABASE_URL`（远端 UAT，口令只经环境变量） |
| 三服务实测 | query `:18901` · console `:18902` · portal `:18903`（**避开** Dev 的 REQ-136 实例 `:18800`，未触碰） |
| 变异沙盒 | 源码副本 `D:\tmp\qa135\{wt,mut1,mut2,mut3}`；**每条变异独立 `CARGO_TARGET_DIR`**：`D:\tmp\cargo-mut-135-{0,1,2,3}` |
| 纪律 | ⛔ 未改 feature 分支（HEAD 仍 `4cda262`、`git status` 空）· ⛔ 未 push · ⛔ 未改 `Deploy` 分支与任何既有文件（`BA` 分支只**新增**本报告）· 变异只在 `D:\tmp` 的源码副本里做 |

> 本 REQ 是「上线后看不见」的两条缺口的补丁。下面每一条都按「**判据有没有牙**」验，而不是「接口能不能通」验：三处变异全部要红，格式校验器要能逐条**点名**坏在哪。

---

## 1 · 结论

**PASS（带 6 条非阻断，0 阻断）。**

| 必验项 | 结果 |
| --- | --- |
| 零业务数据泄漏（AC-003） | ✅ 26 个 needle × 3 份真实抓取 **全 0**；标签值全部是编译期常量或路由模板（类型层：`Key` 只收 `&'static str`） |
| 四个安全计数真能加（AC-004 安全信号） | ✅ 真实请求逐个造出：`TENANT_MISMATCH` 1 / `TENANT_UNRESOLVED` 2 / `TOKEN_KIND_FORBIDDEN` 1 / `TENANT_FROM_CLIENT` 1 |
| 六个指标真实性（AC-004） | ✅ `ttl_bytes` == 响应字节 **293,814**；`mapping_joins` == CLI 回执 **70**；`rows` 1→2→3（0 行查询不动）；`requests_total` 3 次 = 3 |
| Prometheus 格式合法（AC-002） | ✅ 三份真实抓取通过；**7 种坏格式逐条被点名拒绝**（不靠「报了点别的错」蒙过） |
| 审计两条路径（AC-005 / AC-006） | ✅ 不给 `--audit-out` ⇒ stdout 11/1/2 行、stderr 0；给了 ⇒ 文件与 stdout **21 = 21 行逐行相同**；20 行可解析、token 明文 0 命中 |
| 三条变异（纪律 G） | ✅ **3/3 全红**，且红在预期判据上（见 §7） |
| 不退化（AC-008） | ✅ lib **702 passed / 0 failed**（基线 689 ⇒ **+13**）；出口 **63** / 内部 **33**（I28）；四红线 + `tools.rs` + `guard()` + `property_filter.rs` + `gate-preflight` + `Cargo.*` **零 diff**；基线 6 条红在 pristine 与本分支**同名同位置**复现 |
| AC-007（k8s 清单） | ⚠️ **本分支不含**（结构上不可能：`feature/REQ-135` 只有代码，清单在 `deploy` 分支）；`deploy` 分支无 REQ-135 提交 ⇒ 见 N-1 |

非阻断项 6 条（§9）：AC-007 清单未交付、console 一处字面量绕开了唯一映射点、交付方的格式判据断言偏弱、portal 两个仪表恒 0、`/metrics` 自观测、S-2 的 27 张表缺口。**均不构成退回理由。**

---

## 2 · 零业务数据泄漏（AC-003，P0 最重要一条）

### 2.1 造真实流量，再抓 `/metrics`

流量（全部打在真库上，非夹具）：

| # | 动作 | 结果 |
| --- | --- | --- |
| ① | `POST /api/sparql` `SELECT ?b ?amt WHERE { ?b :balances__recharge_balance ?amt } LIMIT 5` | 200，`row_count=1` |
| ② | 同上再跑 2 次 | 200，各 1 行 |
| ③ | `SELECT ?d ?amt WHERE { ?d :deductions__deduction_amount ?amt } LIMIT 3`（**已登记**表，真实查询） | 200，0 行 |
| ④ | `tenant` 带别人的 uuid（越权） | **403** `TENANT_MISMATCH` |
| ⑤ | `SELECT ?u WHERE { ?u a :admin_users }` + `:admin_users__username`（**未登记**表） | **500** `TENANT_UNRESOLVED` ×2 |
| ⑥ | query token 打 console `GET /api/projects` | **403** `TOKEN_KIND_FORBIDDEN` |
| ⑦ | portal `POST /q/my_balance` body `{"tenant": "0000…"}` | **403** `TENANT_FROM_CLIENT` |
| ⑧ | portal 真问题 `POST /q/my_balance` `{}` | 200，`row_count=1` |
| ⑨ | console 真数据端点：`GET /api/projects`、`GET /api/audit`、`GET /api/export/ttl`（**293,814 B**）、`POST /api/r2rml`（**70 join**） | 200 |
| ⑩ | `GET /api/catalog` ×3（验 `requests_total` +3） | 200 |

抓取（`curl` 等价，未带 `Authorization`）：query 10,860 B · console 19,163 B · portal 7,727 B，三份均 `200` + `text/plain; version=0.0.4`。

### 2.2 grep 结果（三份真实抓取，逐项计数）

`D:\tmp\qa135\scrapes\final-{query,console,portal}.txt`

| needle | query | console | portal | 说明 |
| --- | ---: | ---: | ---: | --- |
| `7d491821-27c3-4bf8-8c31-19ee5acd936d`（真实租户 uuid） | 0 | 0 | 0 | token 绑定的真租户 |
| `deductions`（真实表名，**且真被查过**） | 0 | 0 | 0 | 查询②的表 |
| `balances` / `usage_details` / `licenses` / `customers` | 0 | 0 | 0 | 真实表名 |
| `SELECT` / `WHERE ` / `JOIN ` | 0 | 0 | 0 | SQL 片段 |
| `FROM `（带空格） | 0 | 0 | 0 | 见下方注 |
| `anspire_r2rml_excl`（项目名） | 0 | 0 | 0 | |
| `customer_id` / `C0001` | 0 | 0 | 0 | 列名 / 夹具租户 |
| `QUERY-ea5ed16c` / `ADMIN-67a3fd41` / `PORTAL-4b63d51f`（三张真 token 前缀） | 0 | 0 | 0 | |
| `my_balance` / `charges_by_currency` / `usage_by_model`（门户问题 id） | 0 | 0 | 0 | |
| `query-policy` / `exclusions.json` / `tenancy.json` / `schema.json`（声明路径） | 0 | 0 | 0 | |
| `/data/` / `postgres` / `sha256` | 0 | 0 | 0 | 落点 / DSN / 摘要 |

**唯一的子串命中**：不带空格的 `FROM` 在 portal 命中 **1** 次，落点是
`ontok_tenant_denied_total{service="ontok-portal",reason="TENANT_FROM_CLIENT"} 1`
—— 那是**编译期常量** `REASON_TENANT_FROM_CLIENT`（`src/metrics.rs:53`），不是 SQL。带空格的 `FROM ` 命中 0。

### 2.3 标签值的**来源**（这是本条判据的真正落点）

| 标签 | 取值 | 代码位置（`feature-REQ-135`） |
| --- | --- | --- |
| `service` | `ontok-query` / `ontok-console` / `ontok-portal` | `src/metrics.rs:45-47` 三个 `pub const`；`Metrics::new(this.service)` `:342-351` |
| `endpoint` | `/api/sparql`、`/metrics`、`/q/:id`、`/api/config/:kind`、`other` … | `endpoint_label()` `src/metrics.rs:195-227`：**静态 match**；带参路由只折叠成模板（`single_segment_after` `:230`）；只由 `track` `:640` 一个调用点喂入 |
| `code` | `200 / 201 / 202 / 204 / 304 / 400 / 401 / 403 / 404 / 405 / 409 / 422 / 429 / 500 / 502 / 503 / 504 / other` | `status_label()` `src/metrics.rs:241-262`（有限集合，其余收口到 `other`） |
| `reason` | 四个 `TENANT_MISMATCH / TENANT_UNRESOLVED / TOKEN_KIND_FORBIDDEN / TENANT_FROM_CLIENT` | 常量 `src/metrics.rs:50-53`；唯一翻译点 `denied_reason()` `:269-277`（`note_denial_code` `:475` 只认这四个） |
| `view` | `customer` / `model` | 常量 `src/metrics.rs:56-57` |
| `le` | `0.005 … 10`、`+Inf` | 常量 `BUCKETS` `src/metrics.rs:60-62` + `bucket_label()` `:574-580` |

**类型层证据**（不是「约定」，是编译不过）：

```rust
// src/metrics.rs:284
type Key = (&'static str, Vec<(&'static str, &'static str)>);
// 三个带标签的写入口都只收 &'static str
pub fn observe_request(&self, endpoint: &'static str, code: &'static str, elapsed: Duration)
pub fn tenant_denied(&self, reason: &'static str)
pub fn set_ttl_bytes(&self, view: &'static str, bytes: u64)
```

唯一的运行时值是**样本值**（`rows` / `bytes` / `join_count` / 耗时），不是标签值。

### 2.4 抓取里实际出现过的**全部**标签值（维度爆炸 + 泄漏的正面证据）

```
query   : code=[200,403,500]  endpoint=[/api/catalog,/api/describe,/api/health,/api/ontology.ttl,
          /api/sparql,/mcp,/metrics]  reason=[TENANT_MISMATCH,TENANT_UNRESOLVED]
          service=[ontok-query]  view=[customer,model]  le=[12 个固定桶]
console : code=[200,400,403]  endpoint=[/,/api/apply,/api/audit,/api/config/:kind,/api/decide,
          /api/export/ttl,/api/freeze,/api/health,/api/import,/api/projects,/api/r2rml,/api/review,/metrics]
          reason=[TOKEN_KIND_FORBIDDEN]  service=[ontok-console]
portal  : code=[200,403]  endpoint=[/,/api/health,/api/questions,/metrics,/q/:id]
          reason=[TENANT_FROM_CLIENT]  service=[ontok-portal]
```

三份抓取的**样本行**分别 111 / 196 / 79 条（全文 123 / 208 / 91 行，其余是 `# HELP`/`# TYPE`），序列数与请求数无关（同一端点跑 100 次仍是一条序列）⇒ 维度不随流量增长。
（`/q/<真实 uuid>` 与 `/api/tenants/<uuid>/…` 这类路径在实测里都落到 `other` 或 `/q/:id`，见 §7 M1 的反证。）

---

## 3 · 四个安全计数真能加（AC-004 的**安全信号**）

逐类真实造一次，然后抓 `/metrics`：

| reason | 造法（真实 HTTP） | 响应 | 抓取里的计数 |
| --- | --- | --- | --- |
| `TENANT_MISMATCH` | query token + body `"tenant":"00000000-0000-…"` | **403** `{"code":"TENANT_MISMATCH",…}` | `tenant_denied_total{service="ontok-query",reason="TENANT_MISMATCH"} 1` |
| `TENANT_UNRESOLVED` | 查**未登记**的表：`SELECT ?u WHERE { ?u a :admin_users }`、`SELECT ?u ?x WHERE { ?u :admin_users__username ?x }` | **500** `TENANT_UNRESOLVED`（规划期 fail-closed，两次都进） | `…reason="TENANT_UNRESOLVED"} 2` |
| `TOKEN_KIND_FORBIDDEN` | **query token** 打 console `GET /api/projects` | **403** `TOKEN_KIND_FORBIDDEN` | `{service="ontok-console",reason="TOKEN_KIND_FORBIDDEN"} 1` |
| `TENANT_FROM_CLIENT` | portal `POST /q/my_balance` body `{"tenant":"00000000-…"}` | **403** `TENANT_FROM_CLIENT` | `{service="ontok-portal",reason="TENANT_FROM_CLIENT"} 1` |

补充（S-2 的现场要求）：`TENANT_UNRESOLVED` 的一条 curl 即可复现（`code/query-policy.json` 里没有 `admin_users`，本体里有该类）——实测两条查询都返回 500 并计数，**不需要**制造库故障。

> **一条环境事实**：`local-run/admin-tokens.json` 里只有 `kind=admin` 一张，console 启动横幅自己会警告「query token ⇒ 403 这条语义在本进程里不会被触发」。为验 `TOKEN_KIND_FORBIDDEN`，QA 用了一份**同时含 admin + query 两行**的 token 表（与 `tests/test_req124_console.rs:82-88` 的夹具同形状），未改仓库任何文件。生产部署若要这条信号有效，两个 token 表必须同源 —— 见 N-6。

---

## 4 · 六个指标的真实性（AC-004）

| 指标 | QA 实测 | 判据 |
| --- | --- | --- |
| `ontok_ttl_bytes{view="customer"}` | **293814** | `GET /api/ontology.ttl` 响应体自己量 = **293814 B**（且 = console `GET /api/export/ttl` 的 293,814 B）✅ |
| `ontok_ttl_bytes{view="model"}` | **371356** | `GET /api/ontology.ttl?view=model` = **371356 B** ✅ |
| `ontok_mapping_joins` | **70** | 自己跑 CLI：`ontok r2rml generate --project anspire_r2rml_excl --version 2 --schema … --tenancy …` 回执 `joinCondition 70`；产物里 `rr:joinCondition` 出现 **70** 次 ✅（console 侧 `POST /api/r2rml` 之后也变 70） |
| `ontok_rows_returned_total{service="ontok-query"}` | 1 → **2** → **3** | 三次 1 行查询各 +1；**0 行**的查询**不动**（3 仍是 3）⇒ 是真行数，不是请求数 ✅ |
| `ontok_requests_total{endpoint="/api/catalog"}` | **3** | 连发 3 次 `GET /api/catalog` = 3；`/api/sparql` 上 `code="200"` 5 / `"403"` 1 / `"500"` 2 与流量一一对上（**S-1：`code` = HTTP 状态码**）✅ |
| `ontok_request_duration_seconds_count` | `/api/sparql` **8**（=5+1+2）、`/api/catalog` **3**、`/api/ontology.ttl` **2**、`/q/:id` **2** | `_count` 与 `requests_total` 同源同量；`+Inf` 桶 == `_count` ✅ |

- `rows_returned_total` 在 console 恒 0（控制面不返行业务行集）、portal 1（一次真问题 1 行）—— 与服务职责一致。
- 两个仪表在启动时由 `ServiceParts` 拍一次（`src/query/mod.rs:517-525`），运行期只在导出成功时重设（`src/console/http.rs:639-646`、`:723`）⇒ 不随请求漂移（R5 单调/可加）。

---

## 5 · Prometheus 格式合法性（AC-002，**反空转**）

校验器：`validate_exposition()` `src/metrics.rs:662-904`（手写；本机**无** promtool，按 AC-002 允许的口径）。

### 5.1 三份**真实**抓取都通过

```
D:\tmp\qa135\scrapes\final-query.txt    → Ok
D:\tmp\qa135\scrapes\final-console.txt  → Ok
D:\tmp\qa135\scrapes\final-portal.txt   → Ok
```

（这不是「测试自己渲染一份」—— 文件就是 §2.1 那三份 curl 出来的字节，含真实流量后的 `code="403"/"500"`、`le` 桶与 `+Inf`。）

### 5.2 七种坏格式：**逐条被点名拒绝**（反空转）

在独立副本 `D:\tmp\qa135\wt` 里用 `tests/qa_req135.rs`（QA 自己写，不属 feature 分支）喂给同一个 `validate_exposition`。**每一种都断言「报错文本里必须出现该错因」**，而不是「错误列表非空」：

| # | 坏格式 | 校验器报出（点名） | 判定 |
| --- | --- | --- | --- |
| ① | 缺 `# TYPE`（删掉 `# TYPE ontok_mapping_joins gauge`） | 第 118 行 样本出现在 TYPE 之前 + `ontok_mapping_joins：缺 # TYPE 行` | ✅ 拒 |
| ② | 类型写错（`gauge` → `counter`） | `ontok_ttl_bytes 的类型是 "counter"，登记表要求 "gauge"` | ✅ 拒 |
| ③ | 取值不是数字（`70` → `seventy`） | `取值 "seventy" 不是数字` | ✅ 拒 |
| ④ | 标签没有引号（`view="customer"` → `view=customer`） | `标签 view 的值没有用双引号括起来` | ✅ 拒 |
| ⑤ | 多一组没登记的标签（`{tenant="7d491821"}`） | `ontok_mapping_joins 的标签 ["tenant"] 与登记 [] 不一致` | ✅ 拒 |
| ⑥ | 重复序列（同一行写两次，7 0 / 71） | `同一组标签出现了两次（同一时刻两个值）` | ✅ 拒 |
| ⑦ | 直方图缺 `_sum`（删掉全部 `_sum` 行） | 7 条 `缺 _sum 行` + 7 条 `有 _count 却没有 _sum`（14 条） | ✅ 拒 |

同一句柄还验了**正向**：完好的文档 `Ok(())`、五个组件的 `# HELP`/`# TYPE`/`_bucket`/`_sum`/`_count`/`+Inf` 齐全 ⇒ 说明上面的「拒」不是因为校验器一律报错（空转的反面）。
（证据原文：`D:\tmp\qa135\validator-seven-cases.txt`）

---

## 6 · 审计两条路径（AC-005 / AC-006）

### 6.1 不给 `--audit-out` ⇒ 审计在 **stdout**

| 服务 | stdout 里 `audit ts=` 行数 | stderr 里 `audit` 行数 |
| --- | ---: | ---: |
| query | **11** | **0** |
| console | **1**（`action=auth … result=TOKEN_KIND_FORBIDDEN`） | **0** |
| portal | **2**（`event=portal … code=TENANT_FROM_CLIENT` ＋ `code=OK`） | **0** |

改造前 stderr 恒写；现在是 stdout 恒写（`src/query/audit.rs:276`），stderr 已无审计行 ⇒ 真的改了落点，不是「两边都写」。

### 6.2 给了 `--audit-out` ⇒ 文件 **+** stdout，且逐行相同

独立起一个 query 实例（`:18911`）带 `--audit-out D:\tmp\qa135\auditdir\query.audit`，跑 20 次真查询：

```
stdout audit lines: 21        （20 × sparql + 1 × health）
file   audit lines: 21
line-by-line identical: True  （Compare-Object 无输出）
```

⛔ 不存在「只写文件」的分支 —— 这一点由 M3 变异反证（§7）。

### 6.3 AC-006：可 parse、字段齐全、无明文

20 行逐行按 `k=v` 解析（`python` 独立解析，不复用被测代码）：

```
audit ts=2026-10-24T08:10:23.285Z event=sparql proto=rest token=sha256:0dad47b2664a \
      tenant=7d491821-27c3-4bf8-8c31-19ee5acd936d q=sha256:002a263d4ff34f50 rows=1 elapsed_ms=159 code=OK
```

| 判据 | 结果 |
| --- | --- |
| 每行都是 `k=v`，无裸 token | ✅ 0 个非 `k=v` 记号 |
| `ts` / `event` / `proto` / `token` / `elapsed_ms` / `code` 齐全 | ✅ |
| `token=sha256:` + **12** 位十六进制 | ✅ 逐行长度校验 |
| ⛔ 无 token 明文 | ✅ 三张真 token 在**全部** QA 日志（stdout+stderr+审计文件）**0 命中** |
| ⛔ 无查询原文 | ✅ `SELECT` / `FROM ` / `WHERE {` 在全部日志 0 命中（只有 `q=sha256:…`） |

唯一不是摘要的行是 health（匿名）`token=-`，属设计（无凭据的调用）；JSON 里也有同样的语义。

---

## 7 · 三条变异（每条必须红）

沙盒：`git` 副本 `D:\tmp\qa135\{mut1,mut2,mut3}`（源 = `4cda262` 的只读副本），**每条独立 `CARGO_TARGET_DIR`**（`D:\tmp\cargo-mut-135-1/2/3`），与 pristine 的 `cargo-mut-135-0` 互不共享 ⇒ 无缓存假象。

| # | 变异（改了哪一行） | 期望 | 实测 | 红在哪 |
| --- | --- | --- | --- | --- |
| **M1** | `endpoint_label` 的兜底分支：路由模板 → **运行时原始路径**（`other => Box::leak(other.to_string()…)`，`src/metrics.rs:215-225`） | AC-003 泄漏用例红 | ✅ **红** | `qa_no_runtime_path_ever_reaches_the_exposition`：`"/q/7d491821-27c3-4bf8-8c31-19ee5acd936d => /q/7d491821-27c3-4bf8-8c31-19ee5acd936d"`、`"/api/tenants/x/tables/deductions => …"` ⇒ **真实租户 uuid 与真实表名原样进标签** |
| **M2** | `tenant_denied()` 改成**恒 0**（`let _ = reason;`，`src/metrics.rs:466-472`） | AC-004 红 | ✅ **红 4 处**（1 条 QA + 3 条交付方判据） | QA `qa_all_four_denial_reasons_really_count`；lib `metrics::tests::every_metric_has_help_type_and_a_valid_rendered_body`、`query::http::tests::a_foreign_tenant_bumps_the_security_counter`（`两次越权尝试必须记 2 … 0`）、`an_unregistered_table_bumps_the_unresolved_counter` |
| **M3** | `AuditSink::record_line` 在**有文件时不再写 stdout**（`if self.file.is_none() { … }`，`src/query/audit.rs:276`） | AC-005 红 | ✅ **红** | `test_req135_audit_stdout::with_a_file_the_audit_goes_to_both_places`：`left: 0, right: 20` —— 「给了 `--audit-out` 也**必须**照样写 stdout」 |

三条变异都**只改一处**，且不触碰任何测试文件（`git diff` 已核对，见 `log-mut*.txt`）。反向也验了：pristine 副本上同一套判据 **5/5 绿**（`D:\tmp\qa135\log-pristine-tests.txt`），说明红不是判据本身恒红。

---

## 8 · 不退化（AC-008）

| 项 | 要求 | 实测 |
| --- | --- | --- |
| lib 单测 | **702**（基线 689 + 13） | ✅ **702 passed / 0 failed**（`env ONTOK_TEST_DATABASE_URL=…`）；pristine `5163cfb` 同机 `cargo test --lib` = **689 passed / 0 failed** ⇒ **+13** |
| 新增 13 条的构成 | 只许加在 `/metrics` 相关 | ✅ `src/metrics.rs` 单测 **8** 条 + `src/query/http.rs` 单测 **5** 条 = 13 |
| 出口登记 | **63**（内部 **33**，I28） | ✅ `domain::pipeline::tests::registry_is_the_declared_baseline` ok；`I28` 在 `src/domain/pipeline.rs:386`（枚举）/`:551`（spec：`Track::Internal` + `Format::Text`）/`:1775`（登记项，`path: src/metrics.rs`、`call_site: "pub fn http_response("`） |
| 四红线 | 零 diff | ✅ blob 逐一相同：`tests/drop_rate.rs` `f3967ecd`、`tests/test_fk_naming.rs` `6c0ae562`、`src/domain/view.rs` `9d085a03`、`src/domain/review_view.rs` `fdd7fcd9` |
| `tools.rs` / `guard()` / `property_filter.rs` / `gate-preflight` | 零 diff | ✅ 四个文件都不在 `git diff --name-only 5163cfb 4cda262` 里（`guard()` 在 `src/mcp/tools.rs:693`，整文件未动） |
| `Cargo.toml` / `Cargo.lock` | 零改动 | ✅ `git diff --name-only … -- Cargo.toml Cargo.lock` 空 ⇒ 零新依赖（R2 手写格式） |
| 基线红 6 条不新增 | 与 pristine 同名同位置 | ✅ pristine `5163cfb` 与本分支**同样是 4 个 target / 6 条 case 红、panic 位置逐条相同**：`drop_rate::real_schema_drop_rate_report`（`tests/drop_rate.rs:82`，缺 fixture 文件）、`test_agent_orchestration` ×2（`tests/common/mod.rs:233`、`tests/test_agent_orchestration.rs:1070`）、`test_fk_naming` ×1（`:316`）、`test_retained_names_req084` ×2（`:270`、`:293`） |
| 交付方自己的判据 | 绿 | ✅ `test_req135_metrics_format` **3 passed**、`test_req135_audit_stdout` **3 passed / 1 ignored**（ignored 是父用例的子进程样本） |

> 口径澄清：任务书写的「基线红 5 条」按**用例**数是 5（drop_rate 1 + fk_naming 1 + retained_names 2 + agent_orchestration 1），但 `test_agent_orchestration` 实际红 **2** 条（`test_cli_default_and_registry_paths_remain_byte_identical` 与 `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`）。实测是 **6 条**，且 pristine 与本分支完全一致 —— 结论不变（无新增红）。

---

## 9 · 阻断项 / 非阻断项

### 阻断项（0 条）

无。P0 三项（零泄漏 / 安全计数 / 格式反空转）+ 审计两条路径 + 三条变异 + 不退化，**全部通过**。

### 非阻断项（6 条，建议但不阻碍合入）

| # | 项 | 事实 | 建议 |
| --- | --- | --- | --- |
| **N-1** | **AC-007（k8s 清单）未交付** | `feature/REQ-135` 结构上只有代码（分支顶层无 `Deploy/`）；`git log --all --grep=REQ-135` 只有 BA×2 + Feat×1，**`deploy` 分支无 REQ-135 提交**。现状：`k8s/overlays/prod` 仍**没有** `prometheus.io/scrape` 注解（注释还写着「产品当前没有 /metrics 端点」，已过时）；`k8s/base` 的 console/portal 仍 `--audit-out=/data/audit/*.log` + `emptyDir`（UAT overlay 已改 PVC） | 上线**前**在 `deploy` 分支单独补：prod 注解（`port` 分别 8800/8899/8900）+ 审计挂载二选一并写明（本 REQ 后即便 emptyDir 也不再丢证据，因为 stdout 恒写）。不阻碍代码合入 |
| **N-2** | console 一处**字面量**绕开唯一映射点 | `src/console/http.rs:209` 写 `note_denial_code("TOKEN_KIND_FORBIDDEN")`，没走常量 `REASON_TOKEN_KIND_FORBIDDEN`（`:50`）。**不是**泄漏（仍是编译期常量，实测计数正确），但违背「原因只在一个地方翻译」（`denied_reason()`）的纪律 | 换成常量；一行改动 |
| **N-3** | 交付方格式判据断言偏弱 | `tests/test_req135_metrics_format.rs:115-120` 只断言 `!errs.is_empty()` ⇒ 任何一条无关错误都能让它「通过」（空转风险）。QA 的独立用例已按「点名到错因」重测（§5.2），结论已兜住 | 改成断言错因关键词（QA 用例可直接抄） |
| **N-4** | portal 的两个仪表恒 0 | `ontok_ttl_bytes{view=*}` 与 `ontok_mapping_joins` 在 portal 永远 0（门户不服务 TTL、不做 R2RML）。指标口径没错，但面板上会显示「0 字节」 | 在 demand/文档的「已知限制」里写明，或看板只对 query/console 取这两个指标 |
| **N-5** | `/metrics` 自观测 | 抓取自身计入 `ontok_requests_total{endpoint="/metrics"}`（实测 9/8/3 次）—— Prometheus 生态的正常行为，但会随 scrape 频率上涨 | 说明即可；看板别把 `/metrics` 计入业务 QPS |
| **N-6** | S-2 的 27 张表缺口（已记，不属本需求） | 本体 96 张表里 27 张不在 `query-policy.json` 的 `perTable`+`publicDimensions`（实测 `admin_users` 查即 500 `TENANT_UNRESOLVED`）。**上线会「客户一问就 500」** | 上线检查项（BA 已记录）。另：console 的 token 表若与 query 不同源，`TOKEN_KIND_FORBIDDEN` 这条安全信号在生产里不会被触发（console 启动横幅会警告） |

---

## 10 · 复现清单（QA 自己的沙盒）

```powershell
# 1) 起三个服务（QA 端口，不碰 Dev 的 18800）
pwsh -File D:\tmp\qa135\start.ps1 -Tag run1          # 日志 D:\tmp\qa135\logs\run1-*.log
# 2) 真实流量 + 抓取
python -X utf8 D:\tmp\qa135\live1.py                 # → D:\tmp\qa135\scrapes\{query,console,portal}-*.txt
python -X utf8 D:\tmp\qa135\console_live.py          # 控制面真数据端点（export/ttl、r2rml）
# 3) 判据（独立副本，不属分支）
$env:CARGO_TARGET_DIR='D:\tmp\cargo-mut-135-0'; cd D:\tmp\qa135\wt
cargo test --test qa_req135                          # 5/5 绿（7 种坏格式 / 3 份真抓取 / 泄漏 / 四计数）
cargo test --lib                                     # 702 passed（需 ONTOK_TEST_DATABASE_URL）
# 4) 变异（每条独立 target）
cd D:\tmp\qa135\mut1; $env:CARGO_TARGET_DIR='D:\tmp\cargo-mut-135-1'; cargo test --test qa_req135
cd D:\tmp\qa135\mut2; $env:CARGO_TARGET_DIR='D:\tmp\cargo-mut-135-2'; cargo test --lib
cd D:\tmp\qa135\mut3; $env:CARGO_TARGET_DIR='D:\tmp\cargo-mut-135-3'; cargo test --test test_req135_audit_stdout
# 5) 基线红复现（pristine 5163cfb = code worktree）
cd D:\MyCodes\AI\an_onto_demo\code
cargo test --no-fail-fast --test drop_rate --test test_agent_orchestration --test test_fk_naming --test test_retained_names_req084
# 6) 清理（沙盒全在 D:\tmp，不在任何 git worktree 里）
Remove-Item -Recurse -Force D:\tmp\cargo-mut-135-0,D:\tmp\cargo-mut-135-1,D:\tmp\cargo-mut-135-2,D:\tmp\cargo-mut-135-3
```

证据文件：`D:\tmp\qa135\{log-*.txt, validator-seven-cases.txt, scrapes\, auditdir\, logs\}`。

**审核结束时状态**：`feature-REQ-135` HEAD `4cda262`、`git status` 空、工作树列表与审核前一致（未新增/未删除任何 git worktree）、**未 push**。
