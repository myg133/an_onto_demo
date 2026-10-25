# QA 审核：REQ-140 · JSON-LD 上下文子图

## 0 · 元信息

| 项 | 值 |
| --- | --- |
| 结论 | **PASS 带 2 条非阻断**（建议合并前花 5 分钟补 R-1a 的测试；见 §10） |
| 被测 | `feature-REQ-140` · 分支 `feature/REQ-140` · 提交 `8d016a7`（基线 `develop` `600ae76`） |
| 审核方式 | 真库真服务 HTTP 级对拍 + 三处变异体 + 全量 lib / 13 套相关集成 / 5 条基线红复现 |
| 环境 | 元数据库 `postgres@172.17.0.87:5432/anspire_real`；**源库** `postgres@172.16.120.20:30432/anspire_open_uat`；项目 `anspire_r2rml_excl` v2；声明 `code/query-policy.json` + `code/out/schema.json`；token 来自 `local-run/query-tokens.json`（tenant `7d491821…`） |
| 服务 | 特性 `127.0.0.1:8821` / 基线 `127.0.0.1:8820`（TTL 全部重定向到 `.local/qa140/logs/*.err.log`） |
| 证据 | `.local/qa140/evidence/{base,feat}/*.bin` + `report.json`（51 条自动判据）+ `sha256-manifest.txt`（51 份原文 sha256） |
| 脚本 | `.local/qa140/scripts/{serve,probe,mutate,mutprobe,runmut}.ps1\|py` |

> 基线二进制 24193024 B / 特性二进制 24414720 B，均从各自 worktree 用 `.local/qa140/cargo-qa140-{1,2}` 独立构建（⛔ 未写 feature 分支、⛔ 未 push；`git status` 0 改动）。

---

## 1 · 结论

**PASS 带非阻断。** 功能面全部达到 AC；三条变异（不排序 / 不合并 / 丢 warnings）**全部按预期变红**，且红在**正确的判据**上（互不串味）。

| 维度 | 结果 |
| --- | --- |
| 合并（图 vs 表的分水岭） | ✅ 105 行 → 61 节点，14 个重复 `@id` 合并为一份 |
| 确定性（两次 + R-3 换行序） | ✅ 两次 sha256 相同；换序后 `@graph` 逐字节相同 |
| `meta` 不丢限定 | ✅ 与 rows 回执**逐字节相同**（含 warnings/coverage/tenant_applied/join_states/truncated） |
| R-1a 冗余不漂 | ✅ 0 行 / 1 行 / 多行（重复实体）三组全部 `nodes==@graph.length`、`row_count==meta.row_count`；⚠️ 但「测试钉死」只落实一半（§10-①） |
| AC-009 默认回执 | ✅ 3 查询 × 3 写法 = 9/9 **逐字节相同** |
| 聚合 / 空结果 / 不双份 | ✅ |
| 边界（`*/*`、非法 format） | ✅ `*/*` 仍走 rows；非法 format 400 + 可行动 hint |
| 不退化 | ✅ lib 717→731（+14，0 failed）；5 条基线红名称与两侧完全一致；13 套相关集成全绿；四红线 / `Cargo.*` / 输出写点零 diff |

---

## 2 · AC-003 同实体合并（含变异）

**判据用真数据构造**：`llm_provider` 105 行 → 61 个不同 `llm_model`（一个模型被多个 provider 引用）。

```sparql
SELECT ?m ?name WHERE {
  ?p :llm_provider__llm_model ?m .
  ?m :llm_model__model_name ?name
}
```

| 指标 | 值 |
| --- | --- |
| rows 回执 `row_count` | **105** |
| JSON-LD `@graph` 长度 | **61** |
| `@graph` 内唯一 `@id` 数 | **61**（无重复） |
| `summary` | `{"nodes":61,"row_count":105}` |
| 重复的 `@id` | 14 个，单个最多在 **5** 行出现 |

同一 `@id` 在 rows 里出现两次、在图里只一份（`Deepseek v4 Pro`）：

```json
// rows（摘 2 行）
["https://ontok.local/anspire_r2rml_excl#llm_model/959a87da-922e-4a99-8ff0-88c091d165b8", "Deepseek v4 Pro"]
["https://ontok.local/anspire_r2rml_excl#llm_model/959a87da-922e-4a99-8ff0-88c091d165b8", "Deepseek v4 Pro"]

// @graph（只一份）
{ "@id": "ontok:llm_model/959a87da-922e-4a99-8ff0-88c091d165b8",
  "@type": "ontok:llm_model",
  "ontok:llm_model__model_name": "Deepseek v4 Pro" }
```

**变异 m2「同 `@id` 不合并（每行一份）」⇒ 红（两处都红）**：

| 判据 | 正常 | 变异 m2 |
| --- | --- | --- |
| 单测 `the_same_id_from_many_rows_is_merged_into_one_node` | pass | **FAIL**（`@graph` 出现 3 个 `ontok:customers/C1`；`summary.nodes=6 / row_count=3`） |
| 线上唯一 `@id` | 61/61 | **105 个节点 / 61 个唯一 id** |
| 线上 `nodes < rows` | 61 < 105 | **105 == 105** |

---

## 3 · AC-007 确定性（含 R-3 换行序）

### 3.1 两次请求

同一查询连续两次：`sha256 = f329f3118742fc0a…`（10024 B），**完全相同**。

### 3.2 R-3 换行序（同一数据集、两种行序）

```
A: SELECT ?p ?m ?name … ORDER BY ?p
B: SELECT ?p ?m ?name … ORDER BY DESC(?p)
```

先证明「确实是同一份数据、顺序真的反了」：

| 检查 | 结果 |
| --- | --- |
| 行数 | 105 / 105 |
| 行集合（多重集） | 相同 |
| 行序列 | `A.rows == reverse(B.rows)` —— **完全逆序** |

再比字节：

| 比什么 | 结果 |
| --- | --- |
| `@graph` 原文（166 节点） | **逐字节相同**（`688b917bf9c2fa33…`） |
| `summary` 原文 | 相同（`{"nodes":166,"row_count":105}`） |
| 整个响应体 | **仅 `sql_echo` 一处不同**（`ORDER BY t0."id"` vs `… DESC`），其余全文相同 |

> **判据口径（需 BA 记一笔，非缺陷）**：R-3 字面写「输出仍逐字节相同」——对**整份响应**不成立，因为 `meta.sql_echo` 逐字包含查询文本，换 ORDER BY 必然变。这不是放水：`sql_echo` 不同恰好反证「`meta` 没被吞」。QA 按 **`@graph` + `summary` 逐字节 + 「除 `sql_echo` 外全文相同」** 双判据执行；变异 m1 下 `@graph` 立刻分叉，判据有牙。

**变异 m1「节点不排序（按行序输出）」⇒ 红（两处都红）**：

| 判据 | 正常 | 变异 m1 |
| --- | --- | --- |
| 单测 `the_same_input_is_byte_identical_twice_and_ignores_row_order` | pass | **FAIL**（`与行序无关` 断言） |
| 线上换行序 `@graph` | 688b917b… == 688b917b… | **4f4636ff… ≠ 6f5439a3…** |
| 线上两次同查询 | 相同 | 相同（**只比两次请求抓不住**，正是 R-3 存在的理由） |

---

## 4 · AC-005 `meta` 不丢限定（含变异）

用**有覆盖警告**的查询（`deductions`，`main_only`）：

```sparql
SELECT ?d ?amount WHERE { ?d a :deductions . ?d :deductions__deduction_amount ?amount } ORDER BY ?d LIMIT 10
```

该租户在 `deductions` 上 0 行，但**警告与覆盖标记照常出现**（这正是最容易在「图好看」时被吞掉的东西）：

> `表 deductions：租户过滤用 customer_id，覆盖范围 = main_only —— main_customer_id 全空（0/4）；子账号产生的扣费不在结果内，金额可能偏低 …—— ⛔ 不要把 0 行或偏小的数字读成「没有业务」`
>
> `coverage: [{"table":"deductions","coverage":"main_only"}]`

**逐字节判据**（不是「值相等」那么弱）：

| 检查 | 结果 |
| --- | --- |
| 行回执去掉 `columns`/`rows` 后的整段原文 | `c7918778430f9420…` |
| JSON-LD `meta` 成员的整段原文 | `c7918778430f9420…` —— **逐字节相同** |
| 键集合 | `meta` == 行回执 − {`columns`,`rows`}（缺 0 个） |
| 解析后值 | 全等（`warnings` / `coverage` / `tenant_applied` / `join_states` / `truncated` / `limits`） |

**变异 m3「`Meta` 删掉 warnings」⇒ 红（两处都红）**：

| 判据 | 正常 | 变异 m3 |
| --- | --- | --- |
| 单测 `meta_carries_the_same_warnings_and_coverage_as_the_rows_receipt` | pass | **FAIL** |
| 单测 `meta_text_is_the_rows_receipt_minus_columns_and_rows` | pass | **FAIL** |
| 线上 `meta` | c7918778… | **ad2019f1…**（少了警告） |

---

## 5 · R-1a 冗余不漂（0 行 / 1 行 / 多行含重复实体）

| 数据集 | `@graph` 长度 | `summary.nodes` | `summary.row_count` | `meta.row_count` | 判据 |
| --- | --- | --- | --- | --- | --- |
| 0 行（不存在的名字） | 0 | 0 | 0 | 0 | ✅ |
| 1 行（`LIMIT 1`） | 1 | 1 | 1 | 1 | ✅ |
| 多行含重复实体（105 行 / 61 实体） | 61 | 61 | 105 | 105 | ✅ |

结构上同源（不是两处各算一次）：`graph` 与 `summary.nodes` 都出自同一个 `nodes: BTreeMap`；`summary.row_count` 与 `Meta.row_count` 都直接取 `receipt.row_count`。

⚠️ **但 BA 要的「必须各有一条测试钉死」没有落实**——见 §10-①（功能成立，测试缺失，非阻断）。

---

## 6 · AC-009 默认回执零变化

3 条查询（模型联表 / 客户 / COUNT 聚合）× 3 种写法（不写 format / `?format=rows` / `Accept: application/json`），特性 vs 基线 `600ae76`：

| # | 查询 | 写法 | 两侧 sha256 | 字节 |
| --- | --- | --- | --- | --- |
| 1 | 模型联表 | 默认 | `fbb3250b903fb780…` | 1626 B 相同 |
| 2 | 模型联表 | `?format=rows` | `fbb3250b903fb780…` | 相同 |
| 3 | 模型联表 | `Accept: application/json` | `fbb3250b903fb780…` | 相同 |
| 4 | 客户 | 默认 | `ffd94902360ff6ff…` | 1162 B 相同 |
| 5 | 客户 | `?format=rows` | `ffd94902360ff6ff…` | 相同 |
| 6 | 客户 | `Accept: application/json` | `ffd94902360ff6ff…` | 相同 |
| 7 | COUNT 聚合 | 默认 | `994c02a13454c5cb…` | 948 B 相同 |
| 8 | COUNT 聚合 | `?format=rows` | `994c02a13454c5cb…` | 相同 |
| 9 | COUNT 聚合 | `Accept: application/json` | `994c02a13454c5cb…` | 相同 |

`Content-Type` 两侧同为 `application/json`。**MCP 也零变化**：`tools/call sparql_query`（默认 / `format:"rows"`）的 JSON-RPC 响应**逐字节相同**（`38613a6683c0…`）；`tools/list` 除了 `sparql_query` 多一个 `format`（`enum:["rows","jsonld"]`、`default:"rows"`）之外，其余三个工具与 `required` / `additionalProperties` **完全未变**。

---

## 7 · AC-006 / AC-008 / AC-011（附 AC-001/002/004）

### AC-006 聚合进 `summary`

| 查询 | 结果 |
| --- | --- |
| `SELECT (COUNT(?m) AS ?n) WHERE { ?m a :llm_model }` | `@graph: []`，`summary: {"n": 85}`（原生数字） |
| `… GROUP BY ?p` | `@graph: []`，`{"n_groups":105,"groups":[…105 组]}` |

⛔ 没有在 `@graph` 里造任何假节点（`@graph` 恒 `[]`）。

### AC-008 空结果

`summary: {"nodes":0,"row_count":0}`，`@graph: []`（数组、非 `null`、字段存在，原文含 `"@graph":[]`）。

### AC-011 不双份

在 5 份不同查询的 JSON-LD 正文里检索：**没有** `"rows"` 字段、**没有** `"columns"` 字段（含 `meta` 内部）。`format=jsonld` 时为单份输出。

### AC-001 / AC-002 / AC-004

| 判据 | 结果 |
| --- | --- |
| REST 三入口（`Accept: application/ld+json` / `?format=jsonld` / body `{"format":"jsonld"}`） | 三份**同一 sha** `f9904e179a30c8e6…`；`Accept: …;q=0.9` 也认 |
| `Content-Type` | `application/ld+json; charset=utf-8` |
| MCP `sparql_query format=jsonld` | `content[0].text` 与 REST 正文**逐字节相同**（`f9904e17…`），`structuredContent` 是图 |
| 形状 | 顶层 `@context`→`@graph`→`summary`→`meta`；`@context` 含 `ontok`（`…anspire_r2rml_excl#`）+ `xsd`；节点含 `@id`/`@type`/≥1 属性 |
| 节点 vs 字面量 | `@id: "ontok:llm_model/<uuid>"`；`type: 0`（整数）；`model_name: "Gemini 3 Pro Preview"`（字符串）；`price_unit: 1000000.0`（数字）；`created_at: {"@type":"xsd:dateTime","@value":"2026-03-10 18:15:37.248253"}`（R-2 值对象） |
| 无主语的字面量 | `{"nodes":0,"row_count":3,"detached":[…3 条并排序]}`，⛔ 不造 `@id` 为空的假节点 |

---

## 8 · 边界

| 场景 | 结果 |
| --- | --- |
| `Accept: */*`（curl 默认） | **仍走 rows**，与基线**逐字节相同**（`fbb3250b903fb780…`）——默认行为没被改掉 |
| `Accept: text/html, */*` / `application/json` | 仍走 rows |
| `?format=jsonld` + `Accept: */*` | 走 JSON-LD（显式参数优先） |
| `?format=rows` + `Accept: application/ld+json` | 走 rows（更具体的那一个说了算） |
| `?format=bogus` | **400 `BAD_REQUEST`**，hint：`行回执（默认）不写 format；要 JSON-LD 上下文子图写 ?format=jsonld（或 Accept: application/ld+json）。⛔ 拼错不会静默回退。`（基线此处 200，说明这是**新增的严格**，不影响默认路径） |
| `?foo=1`（未知参数） | **400 `BAD_REQUEST`**，hint 列合法值 |
| 执行前拦参数错 | 非法 format 在**查询执行前**返回 400（并有审计），不浪费一次查询 |

---

## 9 · 不退化

### 9.1 lib 测试数（⚠️ 口径订正）

| 树 | 命令 | 结果 |
| --- | --- | --- |
| 基线 `600ae76` | `cargo test --lib`（带 `ONTOK_TEST_DATABASE_URL`） | **717 passed / 0 failed** |
| 特性 `8d016a7` | 同上 | **731 passed / 0 failed** |

新增测试 = **14**（`git diff` 统计：13 个 `#[test]` + 1 个 `#[tokio::test]`）。
> 任务书写的「基线 719 + 12 = 731」与实测**不符**：基线是 **717**，增量是 **+14**（和恰好也是 731）。不影响「无退化」结论（两侧 0 failed、无删除测试），但记账要按实测。

### 9.2 5 条基线红：自己复现，与本次无关

带 `ONTOK_TEST_DATABASE_URL` 跑台账里的 5 个 target（`--no-fail-fast`），特性与基线**失败用例名逐条相同**：

| target / 用例 | 两侧 | 性质（台账） |
| --- | --- | --- |
| `drop_rate::real_schema_drop_rate_report` | 均 FAIL | fail-closed 门禁（不修） |
| `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 均 FAIL | 同上 |
| `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 均 FAIL | 真红（REQ-133） |
| `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | 均 FAIL | 真红（REQ-134） |
| `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | 均 FAIL | 真红（REQ-134） |

`test_review_show_e2e` 本轮 11/11 全绿（台账里的 9 条是 PID 复用撞名 flake，未复现，符合台账描述）。
**两侧失败集合：0 条只在特性 / 0 条只在基线。**

### 9.3 相关集成套件（特性，13 套全绿）

`test_req107_source_enumeration`（10）、`test_req107_outlet_pipeline`（12）、`test_req107_red_lines`（2）、`test_req107_static_texts`（3）、`test_req107_zero_visible_change`（3）、`test_req122_sparql`（17）、`test_req123_live`（9）、`test_req119_queryable_ttl`（22）、`test_req121_r2rml`（16）、`test_property_filter`（10）、`test_no_write_path`（4）、`test_guard_manifest_req081`（6）、`test_mcp_server`（31 passed / 1 ignored）。**0 failed。**

### 9.4 改动面 / 红线

- 改动仅 9 个文件（+1276 / −22）：`src/sparql/plan.rs`、`src/query/jsonld.rs`（新）、`src/sparql/mod.rs`、`src/query/mod.rs`、`src/query/http.rs` + `src/portal/mod.rs` 与 3 个测试的字段补全 —— 与任务书列出的范围一致。
- 四红线 / `tools.rs` / `guard()` / `property_filter.rs` / `Cargo.toml` / `Cargo.lock`：**零 diff**；**零删除文件**；**零新依赖**。
- 响应写点：`Json(` 在本文件里仍**只有一处**（I14），JSON-LD 复用 `respond` 只换 `Content-Type`；`src/query/mod.rs` 的 `jsonld` 模块已登记进 `MODULES`（`test_req107_source_enumeration` 绿）。
- `QueryResult.projection` 用 `#[serde(skip)]`，CLI 回执字节不变（`test_req122_sparql` 的字段断言绿）。

---

## 10 · 阻断 / 非阻断 + 临时目录

### 阻断：无

### 非阻断 ①（建议合并前补，~5 分钟）：R-1a 的「测试钉死」只落实一半

- **现状**：`summary.nodes` 只在 3 个夹具里被断言为字面值（其中一处恰与 `graph.len()` 同夹具），**没有任何一条**断言「`summary.nodes == @graph.length`」这个不变式；「`summary.row_count == meta.row_count`」**完全没有任何测试**（`meta_carries…` 只比 warnings/coverage 等，不比 row_count）。
- **为什么仍判非阻断**：QA 在 0 / 1 / 多行三组真数据上实测成立，且两处取值**结构同源**（同一 `nodes` map / 同一 `receipt.row_count`），今天不会漂。缺的是**防未来改渲染时漂移**的钉子。
- **建议**：加一条 `summary_redundancy_never_drifts()`，对 0/1/多行三组断言 `v["summary"]["nodes"] == v["@graph"].as_array().len()` 且 `v["summary"]["row_count"] == v["meta"]["row_count"]`。这正是 BA R-1a 的原话。

### 非阻断 ②（文档失真）：`src/query/http.rs:2474` 引用了不存在的测试文件

注释写「真字节在 `tests/test_req140_jsonld_live.rs`（真库）」，但该文件**从未提交**（`git log --all -- tests/test_req140_jsonld_live.rs` 为空，分支树里也没有任何 `req140` 测试文件）。后果：评审者会以为真库回归已被覆盖。建议改文案指向本次 QA 证据（`.local/qa140/`）或补上该文件。

### 判据口径（不是缺陷，供 BA 记账）

1. **R-3 不能按「整份响应」比**：`meta.sql_echo` 逐字包含查询文本，换 ORDER BY 必然不同。有效判据 = `@graph` + `summary` 逐字节 + 「除 `sql_echo` 外全文相同」。变异 m1 下 `@graph` 立即分叉，判据仍有牙。
2. **lib 数字订正**：基线 **717**（不是 719），增量 **+14**（不是 +12）。

### 我的临时目录（`.local/qa140/`，全部在 `.local` 内，未在 `D:\` 根建任何目录）

| 路径 | 内容 | 是否保留 |
| --- | --- | --- |
| `.local/qa140/evidence/` | 51 份响应原文 + status/headers + `report.json` + `sha256-manifest.txt` | ✅ 保留（0.2 MB） |
| `.local/qa140/logs/` | 构建 / 两服务 stdout+stderr / 5 红 / 13 套回归 / 3 个变异体的单测与服务日志 | ✅ 保留（0.4 MB） |
| `.local/qa140/scripts/` | 起服务 / 51 条判据 / 变异 / 变异探针 | ✅ 保留 |
| `.local/qa140/bin/` | `ontok-base.exe` / `ontok-feat.exe` / 3 个变异体 exe | ✅ 保留（116 MB） |
| `.local/qa140/mut/` | 变异用的独立源码副本（已还原为原始 `jsonld.rs`） | ⬜ 可删（7 MB） |
| `.local/qa140/cargo-qa140-1/`、`cargo-qa140-2/` | 两棵树的构建缓存（合计 **9.2 GB**） | 🧹 **已清理** |

两服务（8820/8821）与变异体（8822–8824）均已停止；feature 分支 `git status` 干净、未 push。
