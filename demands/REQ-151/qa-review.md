# QA 审核：REQ-151 · 本体模式下 api 源查询返回取数指引

| 项 | 值 |
| --- | --- |
| 被测 | `feature-REQ-151` · 分支 `feature/REQ-151` · 提交 `f2bd872`（基线 `develop` `9dfb8e7`） |
| 审核日期 | 2026-10-26 |
| 审核方式 | **自建两份二进制**（HEAD / 基线，各自独立 `CARGO_TARGET_DIR`）+ 真服务 + 真已签 AM + 真元数据库；普通模式自建源库；变异 3 次；全量测试两轮 |
| 结论 | **PASS 带非阻断**（P2 × 2、P3 × 3，见 §12） |

**一句话**：六个场景全对（api jsonld=200 `@pending`、api rows=400 `NOT_DB_SOURCE`、库类/混合/`sample`/无 mapping=503 且与基线**逐字节相同**）、"零连接"用 **TCP tripwire 独立实证**（15 个请求 / 0 次连接）、普通模式 **13×2 个 case 全部逐字节相同**、三条变异全红、全量测试与基线**失败集完全同名（33 条）**；未发现阻断问题。

---

## 1 · 结论与判定依据

| 判据 | 结果 | 证据位置 |
| --- | --- | --- |
| 六种场景状态码 / 正文 | ✅ 全对 | §2 |
| 零连接尝试（本需求最硬） | ✅ 三种独立证法一致 | §3 |
| `@pending` 形状（R5） | ✅ 同一渲染器；无 id 字面量时**整份响应逐字节相同** | §4 |
| 普通模式零回归（R1） | ✅ 8+4（实跑 13）个 case × 2 种配置，HEAD=基线 | §5 |
| 审计 `code` = 客户端实收（R4） | ✅ 三行原文对齐 | §6 |
| 三条变异必红 | ✅ 各 1 条红行（live）+ 交付测试同步红 | §7 |
| 不退化（全量 `cargo test`） | ✅ 1908/33/3 vs 基线 1901/33/3，**失败集同名** | §8 |
| REQ-136 勘误是否成立 | ✅ 成立（有一处未覆盖 R-C，非开脱） | §9 |

---

## 2 · 六种场景（自己起服务）

**部署**：`ontok query serve --ontology-only --access-mapping Deploy/k8s/overlays/uat-ops360/am-tencent.serve.json --project cloud-resource --version 3`，元数据库 `postgresql://postgres:postgres123@172.16.120.20:30432/ontok`，`ONTOK_SOURCE_DATABASE_URL=postgres://nonexistent.invalid:1/x`（故意指不存在的主机）。

AM 加载结果（启动横幅）：`mapping_version=1 · api 类 14 个 · 跨源边 16 条（32 个方向）`；本体 v3 = 30 类（14 api + 16 库源，`eip` 是库类）。

| # | 场景 | 查询 | 状态 | 字节 | sha256（前 16） | 与基线 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 只 api 类（jsonld） | `SELECT ?c WHERE { ?c a :cvm_instance }` | **200** | 977 | `e866c0081a3e5ac6` | 基线 503（预期变化） |
| 2 | 只 api 类（rows） | 同上 `?format=rows` | **400 `NOT_DB_SOURCE`** | 893 | `a925efa1db9460a4` | 基线 503（预期变化） |
| 3 | 只库类 | `SELECT ?e WHERE { ?e a :eip }` | **503 `NO_DATA_SOURCE`** | 734 | `f99816310ff2cbb3` | **相同** |
| 4 | 混合（api+库） | `?c a :cvm_instance . ?e a :eip` | **503** | 734 | `f99816310ff2cbb3` | **相同** |
| 5 | `sample` | MCP `tools/call sample :cvm_instance` | **200 信封 + `isError:true` + `code=NO_DATA_SOURCE`**（= 503 语义） | 1590 | `9392ee9e0e73798b` | **相同** |
| 6 | 无 mapping | 同 #1（另起一个不带 `--access-mapping` 的服务） | **503** | 734 | `f99816310ff2cbb3` | **相同** |

同时验证：`format` 缺省（不传 `?format`、不传 `Accept`）⇒ 走 JSON-LD ⇒ 200 `@pending`（#1 相同字节）；显式 `Accept: application/json` 之外的默认行为与 REQ-142 一致。

### 原文（#1，200）

```json
{"@context":{"ontok":"https://ontok.local/cloud_resource#","xsd":"http://www.w3.org/2001/XMLSchema#"},
 "@graph":[],
 "@pending":[{"@type":"ontok:cvm_instance","ontok:sourceKind":"api","reason":"source_is_api",
   "get":{"class":"cvm_instance","filter":{},"pushdown":["region","zone"],"ref":"access#CvmInstance@v1"}}],
 "summary":{"nodes":0,"pending":1,"row_count":0},
 "meta":{"row_count":0,"truncated":false,"sql_echo":"","tenant_applied":[],"tables":[],"public_dimensions":[],
   "warnings":[],"coverage":[],"join_states":[],"join_states_scope":"all_edges_between_involved_tables",
   "join_states_scope_note":"…","limits":{"max_rows":50,"token_max_rows":50,"timeout_ms":10000},
   "complete":false,"unresolved_classes":["cvm_instance"],"mapping_version":1}}
```

⇒ `@graph:[]` ✅、`@pending` ≥1 ✅、`meta.complete:false` ✅、`get{class,filter,pushdown,ref}` 齐全 ✅。
（`get.filter = {}` 是 **R-B 已裁定的已知限制**，见 §4。）

### 原文（#2，400 —— 不是 503、不是空表）

```json
{"code":"NOT_DB_SOURCE","hint":"① 改要 **JSON-LD**：`?format=jsonld` …② 再按 `get` 去取…③ 平台**不做**跨源 join…④ …看 `access.kind`…⛔ 重试无效：…",
 "message":"查询落在 **API 源**上（cvm_instance）：这些类的数据不在本库，平台**不执行** API 调用，行回执（`format=rows`）无法表达「少了一块」。"}
```

响应里**没有** `rows` 字段 ⇒ 不是"空表"。

### 原文（#3/#4/#6，503 —— 与基线**逐字节相同**）

```json
{"code":"NO_DATA_SOURCE","hint":"这个部署是**本体服务模式**（`query serve --ontology-only`）：只有本体（类 / 关系 / 属性 / TTL），**没有接业务库**。⛔ **重试无效**（这不是暂时故障，也不是你的查询写法问题）。\n  要数据请：① 继续用 GET /api/catalog、GET /api/describe、GET /api/ontology.ttl（或 MCP 的 catalog / describe / resources/read）；② 需要真查数据时，让运维在有源库的环境里以**普通模式**（`ONTOK_SOURCE_DATABASE_URL` + `--schema` + `--policy`，不带 `--ontology-only`）另起一个查询服务。","message":"本部署只提供本体，未接业务库 —— 没有可查询的数据源，SPARQL / 取样都不可用。"}
```

基线（`9dfb8e7`，同一 AM / 同一元数据库）对 #1/#2 给的是**同一份 734 字节 503**（`f9981631…`）—— 这正是本需求要改的那两条。

补充实测（同场景、非 AC 要求）：MCP `sparql_query` 打 api 类 ⇒ 信封 200 + 内嵌同一份 JSON-LD（`isError:false`）；打库类 ⇒ 200 + `isError:true` + `NO_DATA_SOURCE`（与 REST 同码同 hint，符合 REQ-136 R-3）。

---

## 3 · 零连接尝试（本需求最硬的一条）—— 我的独立复现

### 证法 ①（要求的复现）：源库 URL 指向不存在的主机，行为完全不变

- 服务 A：`ONTOK_SOURCE_DATABASE_URL=postgres://nonexistent.invalid:1/x`
- 服务 B：**不设** `ONTOK_SOURCE_DATABASE_URL`

7 个 case（六场景 + 缺省 format）**逐字节相同**：

| case | A 状态/sha | B 状态/sha | 判定 |
| --- | --- | --- | --- |
| api-jsonld | 200 / `e866c0081a3e5ac6` | 200 / `e866c0081a3e5ac6` | SAME |
| api-rows | 400 / `a925efa1db9460a4` | 400 / `a925efa1db9460a4` | SAME |
| db-jsonld | 503 / `f99816310ff2cbb3` | 503 / `f99816310ff2cbb3` | SAME |
| mixed | 503 / `f99816310ff2cbb3` | 503 / `f99816310ff2cbb3` | SAME |
| sample-mcp | 200 / `9392ee9e0e73798b` | 200 / `9392ee9e0e73798b` | SAME |
| 默认 format（api） | 200 / `e866c0081a3e5ac6` | 200 / `e866c0081a3e5ac6` | SAME |

### 证法 ②（我加的）：**TCP tripwire** —— 把 URL 指向一台我在听的真主机

在 `127.0.0.1:19999` 起一个只记"谁连进来"的 TCP 监听器，然后：

```
ONTOK_SOURCE_DATABASE_URL=postgresql://postgres:postgres@127.0.0.1:19999/qa151_src
```

对服务发 **15 个请求**（六场景 + `max_rows=0` + 跨源边 + MCP `initialize`/`tools/list` + `/api/catalog`、`/api/describe`、`/api/ontology.ttl`（两视图）、`/api/health`、`/metrics`）：

- tripwire 日志：**只有我自己做的那 1 次"阳性对照"连接**（证明监听器确实在工作），**服务进程 0 次连接**；
- 服务进程的 socket 只有到元数据库 `172.16.120.20:30432` 的 2 条 + 监听 `127.0.0.1:18856`，**没有到 `127.0.0.1:19999` 的任何连接**；
- 所有请求耗时 0–5 ms（首个请求 259 ms 是冷启动；真去连会体现为 DNS/连接失败或 100 ms 级超时）。

```
LISTENING 1792983225.2157953
CONNECT  1792983237.4977381 from ('127.0.0.1', 55533)   ← 仅阳性对照（我自己连的）
```

### 证法 ③：代码路径（结构事实）

| 位置 | 事实 |
| --- | --- |
| `src/query/serve.rs:564-573` | 本体模式 `let source = None;` 且**整段不读** `ONTOK_SOURCE_DATABASE_URL`（启动时也不连） |
| `src/query/mod.rs:745-750` | `source_pool()`：`if self.mode.is_ontology_only() { return None; }` —— **唯一取池入口**，判据是 `mode` 而不是"调用点记得先看闸门" |
| `src/query/mod.rs:1270` | 业务查询**唯一**拿池处：`let (Some(mapping), Some(source)) = (svc.mapping.as_ref(), svc.source_pool()) else { 503 }` |
| `src/query/mod.rs:1120-1138` | ⓪ 闸门只跑 `source_check`（纯函数：AST + 映射），**不碰连接、不解析 SQL** |
| `src/query/mod.rs:1196-1203` | ③.5 复用 ⓪ 的同一个判定结果（一条查询只解析一次，闸门与分流不会漂） |
| 全仓 grep | `svc.source` 除 `source_pool()`/`has_source()` 外**无其他读者**；`query/http.rs` 无直连 |

结论：即使有人把闸门顺序改错，本体模式在 ④ 也只能拿到 `None` ⇒ 只能答 503，**拿不到池子**。这条与"顺序"解耦，是结构保证而非约定。

---

## 4 · `@pending` 形状一致（R5）vs R-B 已知限制

### 4.1 同一条查询、同一份实现：**整份响应逐字节相同**

普通模式（源库 + `--schema` + `--policy` + 同一 AM）与本体模式对 `SELECT ?c WHERE { ?c a :cvm_instance }` 的响应 **sha256 都是 `e866c0081a3e5ac6…`（977 字节）** ⇒ 不是"另写一份渲染"。（代码侧只有一个渲染器：`render_pending` → `pending_node`，`src/query/jsonld.rs:388-467`；`@pending` 与 `meta.complete` 同源计算。）

### 4.2 并排：带 id 字面量时（R-B 生效）

查询：`SELECT ?c WHERE { ?c a :cvm_instance . ?c :cvm_instance__instance_id "i-1" }`

| 项 | 普通模式 | 本体模式 |
| --- | --- | --- |
| 状态 | 200 | 200 |
| `@pending[0]` 键集合 | `@id, @type, get, ontok:sourceKind, reason` | `@type, get, ontok:sourceKind, reason` |
| `get` 键集合 | `class, filter, pushdown, ref` | **同左（完全一致）** |
| `get.filter` | `{"ids":["i-1"]}` | `{}` |
| `get.ref` | `access#CvmInstance@v1` | 同 |

**判断**：

1. `get{...}` 的键集合**一致**（R5 要求的那部分 ✅）；`reason`/`ontok:sourceKind`/`@type` 一致 ✅。
2. 唯一的键差是 `@id` —— 它是 `PendingNode` 的**可选字段**（`src/query/jsonld.rs:110-111`，`skip_serializing_if = "Option::is_none"`），语义是"认不出实例标识时不出现，⛔ 不编一个假 id"。本体模式拿不到主键 ⇒ 没有 id ⇒ 没有 `@id`。**这是 R-B 的直接后果，不是第二份形状**，不算缺陷。
3. R-B 的 `filter:{}` 已实测确认为 `{}`（普通模式同一条查询是 `{"ids":["i-1"]}`），与裁定描述一致；`reason` 恒 `source_is_api`，`@type` 与 `meta.unresolved_classes` 同源。

---

## 5 · 普通模式零回归（R1）—— 逐字节对拍

**基线二进制自建**：`git archive 9dfb8e7` → 独立目录 → 独立 `CARGO_TARGET_DIR` 编译（`ontok-baseline.exe`，sha256 `662EDE16…`；被测 `ontok-head.exe`，sha256 `6814F7AD…`）。

**普通模式环境**：源库 `qa151_src`（本地 PG，30 张表 / 811 列，按本体 v3 建模视图生成，`tenant_id` 租户列），`--schema`/`--policy` 自写，token tenant=`T1`，`--project cloud-resource --version 3`。

### 5.1 无 mapping（8 case）

| case | 内容 | HEAD | 基线 | 判定 |
| --- | --- | --- | --- | --- |
| n1 | 库类 rows（`?e a :eip`） | 200 `fc48a959a66485e9` | 同 | **SAME** |
| n2 | 库类 jsonld | 200 `c179568e527e745f` | 同 | **SAME** |
| n3 | 库类属性 rows（`?e :eip__address_ip ?ip`） | 200 `c480c590da6317f9` | 同 | **SAME** |
| n4 | `max_rows:0` | 400 `40327b9b56d065c1` | 同 | **SAME** |
| n5 | 租户不符（body tenant=T2） | 403 `f573ae20243ff61e` | 同 | **SAME** |
| n6 | 非法 format（`?format=xml`） | 400 `6288b1f4229e4ab4` | 同 | **SAME** |
| n7 | 未知类 | 400 `e101953c9ee437a8` | 同 | **SAME** |
| n8 | 语法错 | 400 `fb1cc1deab9d1dfd` | 同 | **SAME** |

### 5.2 有 mapping（4 case + 1 附加）

| case | 内容 | HEAD | 基线 | 判定 |
| --- | --- | --- | --- | --- |
| a1 | api 类 jsonld ⇒ 200 + `@pending` | 200 `e866c0081a3e5ac6` | 同 | **SAME** |
| a2 | api 类 rows ⇒ 400 `NOT_DB_SOURCE` | 400 `a925efa1db9460a4` | 同 | **SAME** |
| a3 | 混合（api+库）jsonld ⇒ 200 pending（REQ-145 行为不变） | 200 `e866c0081a3e5ac6` | 同 | **SAME** |
| a4 | 跨源边**被要求往下走** ⇒ 400 `CROSS_SOURCE_UNSUPPORTED` | 400 `cd2ae0df8b06376c` | 同 | **SAME** |
| a5 | 只走跨源边 ⇒ 200 pending（带 `ref`/`local_hint`，无 `get`） | 200 `dbc8ac798b388621` | 同 | **SAME** |

⇒ **13 × 2 配置 = 26 次对拍全部逐字节相同**（含 400/403/503 的码与文案），R1 成立。

---

## 6 · 审计三行（R4：`code` = 客户端实收）

服务 `--audit-out`，`out/…` 实测原文（`event=sparql`，`tenant=-`，token 只记摘要）：

```
audit ts=2026-10-26T02:49:55.187Z event=sparql proto=rest token=sha256:48c184248e34 tenant=- q=sha256:9269a29fb9a7548c rows=0 elapsed_ms=0 code=OK pending=true
audit ts=2026-10-26T02:49:55.230Z event=sparql proto=rest token=sha256:48c184248e34 tenant=- q=sha256:9269a29fb9a7548c rows=0 elapsed_ms=0 code=NOT_DB_SOURCE pending=true
audit ts=2026-10-26T02:49:55.236Z event=sparql proto=rest token=sha256:48c184248e34 tenant=- q=sha256:fc4217824a907f9e rows=- elapsed_ms=0 code=NO_DATA_SOURCE
```

| 客户端实收 | 审计 `code` | 判定 |
| --- | --- | --- |
| #1 200（半成品 + 指引） | `OK`（+`pending=true`） | ✅ 一致 |
| #2 400 `NOT_DB_SOURCE` | `NOT_DB_SOURCE`（+`pending=true`） | ✅ 一致 |
| #3 503 `NO_DATA_SOURCE` | `NO_DATA_SOURCE` | ✅ 一致 |

同一次调用**只有一行**审计（`sample` 也是一行 `code=NO_DATA_SOURCE`；`max_rows=0` 一行 `code=BAD_REQUEST`，与客户端实收 400 一致）。

---

## 7 · 三条变异（都要红）

变异在**被测源码的独立副本**里做（`git archive f2bd872` → 副本 → 独立 target），不动 feature worktree。

| # | 变异 | 期望红 | live 实测（红行） | 交付测试 |
| --- | --- | --- | --- | --- |
| M1 | api 分支改回一律 503（`pure_api=false`） | AC-001 | `01-api-jsonld status=503`（期望 200）**RED**；`02-api-rows` 也 503（期望 400）**RED** | `req151_ontology_mode_pure_api_gets_pending_and_rows_gets_not_db_source` FAILED + 审计用例 FAILED |
| M2 | 库类也走 pending（`DbOnly` → `Api{needs_db:false}`） | AC-003 | `03-db-jsonld status=200` + `"@pending":[]` + `"complete":true`（期望 503）**RED** | `req151_ontology_mode_db_and_mixed_still_503_byte_identical` FAILED |
| M3 | `sample` 走 pending（去掉本体模式早返回 + 无映射时也造类查询） | AC-005 | `05-sample-mcp` ⇒ `isError:true` + `code=NOT_DB_SOURCE`（期望 `NO_DATA_SOURCE`/503）**RED** | `req151_ontology_mode_sample_still_503_and_audit_codes_match` FAILED |

⇒ 交付的测试**确实能抓住**这三类错（不是只有我的 live 探针能抓）。变异脚本与日志留在 `.local/qa151/mut1.ps1`、`mut2.ps1`、`mut3.ps1`、`audit-mut*.log`、`probe-mut*.json`。

---

## 8 · 不退化（全量 `cargo test --no-fail-fast`）

环境：`ONTOK_REQUIRE_FIXTURES=1`、`ONTOK_DATABASE_URL=<共享元数据库>`、`ONTOK_TEST_DATABASE_URL=<本地测试库>`；两份二进制各自独立 target。

| 列 | HEAD `f2bd872` | 基线 `9dfb8e7`（同夹具口径） | 差 |
| --- | --- | --- | --- |
| passed | **1908** | **1901** | +7 = **恰好新增的 7 个测试** |
| failed | **33** | **33** | 0，且**失败用例逐个同名** |
| SKIP / ignored | **3** | **3** | 同名（`ac008_the_session_is_read_only`、`ac005_a_tenant_never_sees_another_tenants_rows`、`i1_partial_coverage_is_announced_on_real_data`） |
| 失败 target | 9 个 | 同 9 个 | `drop_rate`、`test_agent_orchestration`、`test_fk_naming`、`test_req123_live`、`test_req124_live`、`test_req126_live`、`test_req127_live`、`test_req145_live`、`test_retained_names_req084` |

> 说明（如实记录）：我第一次跑基线时是 **34 failed**，多出 `ac002_real_fixture_excludes_28_and_no_excluded_entry`、`live_console_pipeline_is_byte_identical_to_the_cli_and_conflicts_are_409`，少 `live_auth_etag_audit_and_mcp_resource`。逐条查明是**我复制基线源码时没带生成物**（`out/`、父级锚点 `code/out`），导致同一批 live 用例在"缺夹具 → 响亮 FAIL"与"缺夹具 → 早退 vacuous ok"之间切换。把两份树的夹具口径对齐后重跑，**失败集与 HEAD 完全同名**（上表）。

### 33 条失败逐条归因（全部与基线同名，README 口径）

| 归因 | 条数 | 用例 |
| --- | --- | --- |
| **缺环境变量** `ONTOK_REQ123_SCHEMA` | 10 | `ac001_catalog_and_describe_are_identical_across_protocols`、`ac003_a_never_sees_another_tenants_rows`、`ac004_rest_and_mcp_receipts_are_byte_identical`、`ac004_the_same_error_code_on_both_protocols`、`ac005_a_timeout_is_reported_as_timeout`、`ac005_the_token_row_cap_cannot_be_raised`、`ac007_audit_has_no_token_and_no_query_text`、`ac010_five_real_queries_return_receipts`、`b1_the_scope_and_the_unused_edge_are_visible_in_the_receipt`、`req142_default_is_jsonld_on_rest_and_mcp` |
| **缺环境变量** `ONTOK_SOURCE_DATABASE_URL` | 9 | `ac001_ttl_marks_api_classes_in_both_views_and_leaks_nothing`、`ac002_without_mapping_nothing_new_appears`、`ac003_describe_reports_access_for_api_and_db_classes`、`ac004_api_class_query_returns_pending_and_an_empty_graph`、`ac005_rows_on_an_api_class_is_not_db_source_not_an_empty_table`、`ac006_cross_source_edges_carry_no_get_and_traversal_is_unsupported`、`ac007_complete_always_equals_pending_is_empty`、`ac008_mapping_version_appears_only_with_a_mapping`、`r2_audit_code_equals_what_the_client_received` |
| **缺环境变量** `ONTOK_REQ126_TENANT_A` | 6 | `ac001_two_real_tenants_see_disjoint_data_and_no_internals`、`ac004_the_main_only_coverage_warning_is_visible`、`ac010_a_saturated_token_does_not_starve_another_token_on_real_data`、`ac010_a_truncated_answer_says_so`、`ac010_three_concurrent_requests_on_a_cap_2_token_are_2x200_plus_1x429`、`n1_a_hardcoded_tenant_is_caught` |
| **缺 live 夹具**：元数据库项目 `anspire_r2rml_excl` 不存在 | 3 | `live_auth_etag_audit_and_mcp_resource`、`live_export_and_r2rml_are_byte_identical_to_the_cli_on_a_frozen_production_snapshot`、`live_two_views_are_byte_identical_to_the_cli_on_a_frozen_production_snapshot` |
| **缺外部脱敏 schema 文件** | 2 | `real_schema_drop_rate_report`（`../.review-workspace/fix-props/schema.json`）、`test_real_schema_customer_id_has_exactly_one_display_name`（脚本里写死的 `/home/myg133/...`） |
| **既存红灯**（基线同红，与本次 diff 无关） | 2 | `test_req084_every_registered_name_is_still_literally_at_its_line`、`test_req084_no_unregistered_real_name_remains_in_the_registered_files`（REQ-084 登记漂移：`src/extract/propose.rs:280` / `src/domain/concept.rs` 里实际出现 9 处已登记表名，登记表只记 5 处） |
| **既存红灯**（端到端 golden 断言） | 1 | `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`（`items` 与 legacy 抽取逐项比对失败；基线同红） |

**"代码问题" = 0 条**：没有一条失败是 REQ-151 引入的（新增 7 个测试全绿；失败集与基线逐个同名）。

同时确认 AC-009 的禁改项：

- 改动文件只有 7 个：`src/cli.rs`、`src/query/{catalog,http,mod,serve,source_gate,tokens}.rs`；`tools.rs`、`guard()`、`property_filter.rs`、四红线**未动**；
- `Cargo.toml` / `Cargo.lock` **未动**（零新依赖）；
- diff 里**没有删一行** `#[test]` / `assert`（无删测试、无放宽断言）；新增 7 个测试；
- 新增代码里**无厂商名**（grep `tencent|qcloud|aliyun|huawei` 命中 0）。

---

## 9 · REQ-136 勘误是否成立

读了 `BA/demands/REQ-136/acceptance.md` 的 `## 勘误（REQ-151 · 2026-10-26）`（164-180 行），结论：**成立，且不是替产品开脱**。

| 勘误的说法 | 我的核对 | 判定 |
| --- | --- | --- |
| "AC-004/008 原文没有限定哪类查询" | `AC-004` 原文："`POST /api/sparql` ⇒ 503，code=`NO_DATA_SOURCE`"；`AC-008`："`sparql_query` ⇒ 与 REST 同语义（503…）"—— 确实**没限定类** | ✅ 准确 |
| "当时的隐含前提是：本体模式下没有 mapping，所以所有查询都需要库" | access 映射是 REQ-145 才引入的，REQ-136 时代本体模式确实无 mapping | ✅ 准确 |
| "需要库（库类/混合/sample/无 mapping）⇒ 503 不变" | 实测 #3/#4/#5/#6 与基线**逐字节相同** | ✅ 准确 |
| "只涉及 api 源类 ⇒ 收窄为 jsonld 200 + `@pending`；rows 400 `NOT_DB_SOURCE`" | 实测 #1=200（977B）、#2=400（893B） | ✅ 准确 |
| "本体模式下不会碰源库连接（唯一取池入口恒 None + 坏池 tripwire）" | §3 的三种证法都成立 | ✅ 准确 |
| "原文不动，只做收窄说明" | AC-004 / AC-008 原文完好，勘误追加在文末 | ✅ 符合 R-A |

**一处未覆盖（不影响"成立"，但应知道）**：勘误只谈了"查询落在 api 类"的收窄，**没提 R-C**——`max_rows=0` + 纯 api 查询从 503 变成 400 `BAD_REQUEST`（参数闸在 ② 才报，而它现在能走到 ②）。这条也在 AC-004 的字面范围内，建议勘误补一句，或明确指向 REQ-151 的 R-C 条目。

其余判断：勘误既没有把"原来一律 503"说成"一直就是对的"，也没有把"收窄"写成"推翻"，用的是"精确化"这个准确措辞 ⇒ **不是开脱**。

---

## 10 · 污染数量（如实上报，未删任何元数据库项目）

| 库 | 跑前 | 跑后 | 增量 |
| --- | --- | --- | --- |
| 共享元数据库 `ontok`（`172.16.120.20:30432`） | 2 projects / 3 versions | **24 projects / 41 versions** | **+22 / +38** |
| 本地测试库 `ontok_demo`（`127.0.0.1:5432`，`ONTOK_TEST_DATABASE_URL`） | 1025 projects / 1279 versions | **1490 projects / 1855 versions** | **+465 / +576** |

- 新增项目名形如 `req129_ac006_*`、`req129_ac007_s_*`、`t_req124pipe_*`（测试自己建，**一个都没删**）。
- 我自己的**源库** `qa151_src`（不在元数据库里）在收尾时 drop 了；它不是元数据项目，不涉及"别自己删"那条。

---

## 11 · 临时目录与清理

| 目录 | 内容 | 处理 |
| --- | --- | --- |
| `feature-REQ-151/.local/qa151/` | 证据：`probe-*.json`（各端口原始响应 + sha）、`audit-*.log`、`serve-*.log`、`tripwire.log`、`sha-*.txt`、`test-*.log`、`mut*.ps1`、`fail-attrib.json`、`counts-*.json`、`bin/ontok-{head,baseline,mut3}.exe` | **保留**（91.9 MB） |
| `feature-REQ-151/.local/cargo-qa151/` | 我起的构建目录（`target-head` / `target-baseline` / `target-mut` / `baseline-src` / `mut-src`）**共 38 GB** | **已整目录删除**（探针脚本先移进 `.local/qa151/`，两份二进制留 `bin/`） |
| 端口 / 进程 | 起过 13 个服务实例 + 1 个 tripwire | **全部停止**，`127.0.0.1:1885x/1886x/1887x/19999` 无监听 |
| feature worktree | — | **未改动**：`git status` 干净，HEAD 仍是 `f2bd872`，未 commit、未 push |

复现命令（要点）：

```powershell
# 被测 / 基线各一份二进制（各自 target）
$env:CARGO_TARGET_DIR = ".local\cargo-qa151\target-head";     cargo build --bin ontok
git archive 9dfb8e7 | tar -x -C .local\cargo-qa151\baseline-src
$env:CARGO_TARGET_DIR = ".local\cargo-qa151\target-baseline"; cargo build --bin ontok

# 本体模式（真 AM + 元数据库；源库 URL 故意指向不存在）
ontok query serve --bind 127.0.0.1:18851 --tokens .local\qa151\tokens-ontology.json `
  --project cloud-resource --version 3 --ontology-only `
  --access-mapping Deploy\k8s\overlays\uat-ops360\am-tencent.serve.json `
  --audit-out .local\qa151\audit-head-ont.log
```

---

## 12 · 非阻断发现（建议，不挡合并）

| # | 级别 | 发现 | 位置 / 证据 | 建议 |
| --- | --- | --- | --- | --- |
| F1 | **P2** | **R-C 的"写进 `--help` 或已知限制"未落实**：`ontok query serve --help` 全文没有 `max_rows=0` + 纯 api 从 503 变 400 的说明（搜 `0 行` / `max_rows=0` = 0 命中）；只在 `BA/demands/REQ-151/acceptance.md` R-C 里有。行为本身正确（实测 400），但"已知限制"缺一份产品侧文字 | `src/cli.rs` 的 `--ontology-only` 段；`acceptance.md:97` | 在 `--ontology-only` 的 help 里补一句："`max_rows=0` 的 400 在源判定之前报 ⇒ 纯 api 查询也是 400 而非 503" |
| F2 | **P2** | **R-B 的"后果"没写进产品侧已知限制**：裁定要求"写进已知限制并说清后果"（`filter:{}` = 取全量；应先按其 id 属性查 id；本体里目前**没有**主键声明）。交付物里只有代码注释"给不出 ⇒ `filter: {}`（契约规则 7 的形状），⛔ 不猜 id"，**没有**"等于取全量、代价大"这句后果 | `src/query/source_gate.rs:470-471`、`jsonld.rs:154`；`acceptance.md:82-95` | 在 `--access-mapping` 的 help（REQ-145 的 R-3/R-G 就在那儿）补一条 **R-B 已知限制**，把"取全量"的后果与"补 property 级主键声明"的补齐路径写上 |
| F3 | P3 | `QueryService::has_source()` 已**无任何调用者**，且注释仍称它是"横幅与 `run_sparql` 的**唯一判据**"——判据现在是 `source_pool()` + 闸门，注释过期（`pub` 方法所以没有 dead_code 警告，不会被编译提醒） | `src/query/mod.rs:731-734`（全仓 grep 仅此一处） | 删掉它，或把注释改成"横幅/就绪探针用；查询路径的池入口是 `source_pool()`" |
| F4 | P3（供 BA 知悉） | **只走跨源边**（`?n :network_interface__security_group ?s`，不点名 api 类）在本体模式 ⇒ **503**，而普通模式给的是 200 + `@pending`（带 `ref`/`local_hint`、**一行 SQL 都不执行**）。实现取"跨源边的一端在库 ⇒ 这条要库"（`needs_db = db_hits \|\| !cross_hits.is_empty()`，有测试锁住）。语义上"指引里点名了库侧的列"是否算"必须碰库"可再议 | `src/query/source_gate.rs:563-567`、测试 `a_cross_source_edge_needs_the_db`；实测 #a5 vs 本体模式 503 | 若要给指引，改成"跨源只走边 ⇒ 也给 pending"；若保持现状，建议在 help 里写明"跨源遍历在本体模式一律 503" |
| F5 | P3 | 语法错误的查询在本体模式 ⇒ **503 `NO_DATA_SOURCE`**（`source_check` 解析失败返回 `None` ⇒ 按"要库"处理），而不是 400 `SPARQL_UNSUPPORTED`。与基线一致（**非回归**），但调用方会拿到"没有数据源"这种误导性答案 | `src/query/mod.rs:1328-1335` | 可留作后续：解析失败时优先报语法错（需先确认不破 REQ-136 的 503 文案基线） |

> 六场景、零连接、零回归、审计、变异、全量测试**均无阻断问题**；F1/F2 是**文档落实**缺口（行为正确），F3–F5 是可选优化/待裁定项。

---

## 附：证据索引（`feature-REQ-151/.local/qa151/`）

| 文件 | 内容 |
| --- | --- |
| `probe-head-ont.json` / `probe-base-ont.json` | 六场景 HEAD / 基线原始响应 + sha |
| `probe-head-ont-nourl.json` | 不设源库 URL 的对照组 |
| `probe-head-nomap-ont.json` / `probe-base-ont-nomap.json` | 无 mapping 场景 |
| `probe-head-norm.json` / `probe-base-norm.json` | 普通模式（有 mapping）13 case |
| `probe-head-nomap.json` / `probe-base-nomap.json` | 普通模式（无 mapping）13 case |
| `hit-tripwire.json` + `tripwire.log` | 15 个请求 × 零连接的 tripwire 证据 |
| `audit-head-ont.log` / `audit-head-ont-tripwire.log` | 审计原文（三行 + `pending=true` + `BAD_REQUEST`） |
| `rb-normal.json` / `rb-ontology.json` | R5 / R-B 并排（带 id 字面量） |
| `sha-*.txt` / `compare.py` | sha 清单与对拍脚本 |
| `test-head-require.log` / `test-base-normalized.log` | 两轮全量测试原始输出（109 个 test result 行） |
| `fail-attrib.json` / `attrib.py` | 33 条失败逐条归因 |
| `mut1.ps1` / `mut2.ps1` / `mut3.ps1` + `probe-mut*.json` + `audit-mut*.log` | 三条变异与红行 |
| `bin/ontok-head.exe`、`bin/ontok-baseline.exe`、`bin/ontok-mut3.exe` | 三份二进制（便于复核 sha） |
