# REQ-123 · 查询服务对外（HTTP API + 远程 MCP）· Pre-merge QA 审核

- **审核对象**：worktree `feature-REQ-123`（分支 `feature/REQ-123`，基线 `develop` `e3da616`）
- **审核日期**：2026-10-23
- **审核范围**：`acceptance.md` 的 11 条 AC + J-1 / J-2 裁定 + 任务书 P0 九项
- **结论**：**FAIL**（1 个功能阻断 + 1 个交付阻断）

| # | 阻断项 | 性质 | 修法体量 |
| --- | --- | --- | --- |
| **B1** | **J-2 口径未落地**：回执无 `join_states_scope`（或等价显式说明）、`--help` 与 MCP `tools/list` 都没写「全部映射边，非本次走到的边」、模块头 `src/query/mod.rs:35` 仍写「**本次用到的** join 边」 | 功能 / 契约（BA 裁定的前置条件未满足） | 小（加 1 个字段 + 2 处文案） |
| **B2** | **分支 0 commit**：`git rev-list --count develop..HEAD` = **0**；`src/query/`（7 文件 ~175 KB）、`tests/test_req123_live.rs`、`tokens.example.json` 全部 **untracked**；工作区还有一个 222 B 的垃圾文件 `NUL` | 交付 / 合并 | 小（提交 + 删 `NUL`） |

**除这两项外，九项 P0 全部实测通过**：两协议逐字节相同（5 条查询 + 1 条错误路径 + 2 条补充查询）、租户 6 项攻击全挡住、`futures-util` 确认无新 crate、限额/SSE/超时/并发/审计/绑定/错误 hint 全部符合，`serve` 与四红线未动，5 条既有红灯在基线上**逐条复现**（非本需求引入）。

---

## 0 · 环境与方法

| 项 | 值 |
| --- | --- |
| 服务 | `ontok query serve --bind 127.0.0.1:8811`（元数据库 `172.17.0.87/anspire_real`，源库 `172.16.120.20:30432/anspire_open_uat`，项目 `anspire_r2rml_excl` 第 2 版，`--timeout-ms 10000 --sse-idle-timeout-ms 4000 --sse-max-connections 4 --max-concurrent-queries 2 --audit-out …`） |
| 超时用例 | 另起一份 `--timeout-ms 1`（127.0.0.1:8812） |
| token 表 | 自造 `qa-req123/artifacts/tokens.json`：A = `f953b797…c12c7`（200 行 / 并发 2）、B = `a62f1fa2…023d`（50 行 / 并发 1）、C = 同 A 租户（3 行 / 并发 1，用于截断与并发） |
| 真库租户选择 | A/B 都在 `balances`+`licenses` 里各有数据（A 33 行两跳，B 1 行），保证「0 行」不是空转 |
| 变异环境 | 独立拷贝 `qa-req123/mutant`（**不动 feature 工作区**）+ 独立 `CARGO_TARGET_DIR`；基线复现用独立 worktree `qa-req123/baseline`（detached `e3da616`） |
| 证据 | `qa-req123/artifacts/`（日志、`sha256` 对拍 JSON、审计文件、变异输出） |

---

## 1 · 两协议逐字节（AC-004）✅ 通过

REST `POST /api/sparql` 的**响应体字节** vs MCP `POST /mcp` → `tools/call` → `sparql_query` 的 **`result.content[0].text` 字节**，同一 token（A）、同一查询：

| 查询 | REST | row_count | REST sha256 | MCP sha256 | 逐字节相同 |
| --- | --- | --- | --- | --- | --- |
| 单跳 `?b :balances__recharge_balance ?amt` | 200 | 1 | `d596ea8bd4eb17a8…` | 同 | ✅ |
| **两跳** `?l :licenses__customers ?c . ?c :customers__customer_name ?n` | 200 | 33 | `548612d8f5cb1ec8…` | 同 | ✅ |
| **聚合** `(COUNT(?b) AS ?n) WHERE { ?b a :balances }` | 200 | 1 | `9fb25de0fd1c141e…` | 同 | ✅ |
| GROUP BY `?c (COUNT(?b)) … GROUP BY ?c` | 200 | 1 | `ad0c30de35febc82…` | 同 | ✅ |
| ORDER BY+LIMIT `ORDER BY DESC(?amt) LIMIT 5` | 200 | 1 | `01d82f449abc21ce…` | 同 | ✅ |
| **错误路径** `?x a :no_such_class` | 400 | — | 正文 = MCP `text`，`code=UNKNOWN_CLASS`，MCP `isError=true` | 同 | ✅ |
| 补充：`deductions`（**有 warnings**，main_only） | 200 | 0 | `10ead902adac911b…` | 同 | ✅ |
| 补充：三表 join（有 `join_states`） | 200 | 33 | `2e4ba5d91dbd7688…` | 同 | ✅ |

`rows / columns / row_count / truncated / warnings / coverage / join_states / limits` 全部一致；结构上成立的原因也查了：REST 走 `Json(SparqlReceipt)`、MCP 走 `SparqlReceipt::serialize()`，**同一个 `Serialize` 实现、同一字段顺序**，且 MCP 的 JSON-RPC 信封是「已编码字符串」直接回写（不二次 `Json(Value)`）。

### 变异三元组（实测）

| # | 变异（只改一处） | 期望 | 实测 | 判 |
| --- | --- | --- | --- | --- |
| **M1** | MCP 路径清空 `warnings`（`http.rs::tools_call` 的 `sparql_query` 分支，`receipt.warnings.clear()`） | 应红 | **全绿**：lib 562/562 + `test_req123_live` 8/8 | ❌ **测试盲区（缺口）** |
| **M3** | REST 路径清空 `coverage`（`http.rs::post_sparql`，`receipt.coverage.clear()`） | 应红 | **红**：`ac004_rest_and_mcp_receipts_are_byte_identical` 失败，逐字节差异打印 | ✅ |
| **M4** | MCP 路径跳过鉴权（`needs_auth` 不再对 `/mcp` 生效） | 应红 | **红 6 条**：401 判据、SSE、initialize/tools、通知 202、跨租户 403 等 | ✅ |

**M1 的缺口说明（非阻断，但建议补）**：三条 AC-004 对拍查询（单跳 / 两跳 / 聚合）**都不产生 warning**，所以「MCP 少带 warnings」在字节上等于没变。代码层面单点构造是对的（M1 只是变异才拆开），但**测试对 `warnings` 的一致性没有约束**——一旦将来有人在 MCP 侧单独组装回执并漏掉 warnings，现有判据抓不到。建议把 `deductions`（`main_only`，必产生 1 条 warning）加进 `ac004` 的对拍集合，即可让这条变异变红。

---

## 2 · 租户只能来自 token（AC-003）✅ 6 项全挡住

| # | 攻击 | 实测 | 判 |
| --- | --- | --- | --- |
| 1 | 无 token（REST `/api/sparql`） | **401** `UNAUTHORIZED`，正文不透露路径/租户/token 是否存在 | ✅ |
| 1b | 无 token（MCP `POST /mcp`） | **401**，同上 | ✅ |
| 2 | 错 token | **401**，正文与无 token 完全相同 | ✅ |
| 3 | token 放 **query string**：`POST /api/sparql?token=…` / `GET /api/catalog?access_token=…` / `GET /mcp?token=…` | 三条都 **401**（只认 `Authorization` 头） | ✅ |
| 4 | body 带**别人**的 tenant（token A + `tenant=B`） | REST **403 `TENANT_MISMATCH`**；MCP `isError=true`、`structuredContent.code=TENANT_MISMATCH`、hint=「租户由凭证决定…」 | ✅ |
| 4b | body 不带 tenant / body 带自己的 tenant | 200（正常），`row_count` 均 1 | ✅ |
| 5 | **A 的 token 用 SPARQL 写死 B 的实例 IRI** `FILTER(?c = <…#customers/{B}>)` | **row_count = 0**；`sql_echo` 注入的是 A（`= 'f953…'`），不含 B | ✅ |
| 6 | **B 查同一条（对照）** | **row_count = 1** ⇒ 上面那条 0 行不是「查询本身写错」 | ✅ |
| 6b | 反向（B 写死 A 的实例） | A token 查 ⇒ 1 行；B token 查 ⇒ 0 行 | ✅ |

补充结构证据：`PlanInput.tenant` 只从 `Caller.token.tenant` 取，请求体 `tenant` 仅参与「比对」分支，不一致在**执行前**返回 403；两个协议的 `Caller` 都只由同一个中间件注入的 `Auth` 构造。

---

## 3 · J-1 `futures-util`：**无新 crate**（三条全过）✅

| 判据 | 实测 | 结论 |
| --- | --- | --- |
| ① `cargo tree` 证明本来就在树里 | `cargo tree -i futures-util` → `futures-util v0.3.34` ← **axum v0.7.9**（另有 axum-core / sqlx-core / tower），且现在多一条 `ontok → futures-util` | ✅ 确实是把既有传递依赖提升为直接依赖 |
| ② `Cargo.lock` 的 `[[package]]` 数量不变 | `develop` = **260**，当前 = **260**；`Cargo.lock` 差异仅 **+1 行**（`ontok` 的依赖列表里多 `"futures-util"`） | ✅ 编译包集合不变 |
| ③ 报告措辞 | `Cargo.toml` 注释写明「`futures-util` 已经在依赖树里（axum 直接依赖它，features=["alloc"]）…**不新增任何 crate**」；`.feature/manifest.json` 也写成「AC-011 的『不引新依赖』在这一行上**如实登记为例外**」并给了落选方案（`tokio-stream` 会拉 `tokio-util`） | ✅ 没有说成「没有新增依赖而不解释」 |

附带核查：`Cargo.toml` 同时把 `tokio` 的 features 显式加了 `time`（不是新 crate）。`cargo tree -e features -i tokio` 显示 **`time` 本来就被 axum 与 sqlx-core 打开**，声明它不改变依赖图。⚠️ 小瑕疵见 §10-N4（manifest 说加了 `sync`，实际没有，且代码注释明确「不用 `tokio::sync`」）。

---

## 4 · J-2 `join_states[]` 口径 ❌ **阻断（B1）**

裁定要求：**两处写清**——① 字段文档/`--help`；② 回执里加显式说明（如 `join_states_scope`）；⛔ 字段名不许暗示「本次走到的边」。

实测（逐条都不满足）：

| 检查点 | 实测 | 判 |
| --- | --- | --- |
| 回执显式字段 | 回执键为 `columns / rows / row_count / truncated / sql_echo / tenant_applied / tables / public_dimensions / warnings / coverage / join_states / limits` —— **没有 `join_states_scope`，也没有任何等价说明字段** | ❌ |
| `ontok query serve --help` | `join_states` **出现 0 次**（全文见 `qa-req123/artifacts/help-serve.txt`） | ❌ |
| MCP `tools/list` 的 `sparql_query.description` | 只列了字段名，未写范围；`join_states_scope`/「全部」均不出现 | ❌ |
| 误导措辞 | `src/query/mod.rs:35` 仍写「`join_states[]`：**本次用到的** join 边」——这正是 J-2 点名禁止的说法；5 行之后的「已知限制」（`:40-42`）又写成「涉及的表之间的全部映射边」，**同一份模块头自相矛盾** | ❌ |

**风险是真的会发生**（不是纸面问题）：实测三表查询 `?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c`，回执 `join_states` 返回 **3 条边**，其中 `balances → customers`（`balances__customers__source_customer_id`）**本次查询根本没用到**。Agent 若按字段名理解为「用到的边」，会把未验证的边当成已验证的 join 证据。

**修法（小）**：`SparqlReceipt` 加 `join_states_scope: "all_edges_between_involved_tables"`（放在 `join_states` 前/后均可，注意两协议同一序列化 ⇒ 自动同步）；`--help` 与 MCP 工具描述各加一句；把 `mod.rs:35` 改成「涉及的表之间的**全部**映射边（≠ 本次走到的边）」。改完需同步 §1 的 sha256（对拍判据只比较「相同」，不需要钉死哈希）。

---

## 5 · 限额与资源（AC-005）✅

| 用例 | 实测 | 判 |
| --- | --- | --- |
| 调用方抬高 `max_rows`（token A 上限 200，传 99999） | `limits={max_rows:200, token_max_rows:200, timeout_ms:10000}` + warning「凭证上限**不能**由调用方抬高」 | ✅ |
| `max_rows=0` | 400 `BAD_REQUEST`（不接受 0 行） | ✅ |
| 截断（token C 上限 3） | 两跳 33 行 ⇒ `row_count=3, truncated=true`，`limits.max_rows=3` | ✅ |
| 请求 `max_rows=2`（低于 token 上限） | `row_count=2, truncated=true`，`token_max_rows=3` | ✅ |
| 超时（`--timeout-ms 1` 的服务） | **504** `TIMEOUT`，message/hint 可读（`SQL` 不泄露） | ✅ |
| 并发超限（token C `max_concurrency=1`，8 并发） | `200×1 / 429×7`，`code=TOO_MANY_CONCURRENT`，hint 给出上限与调法 | ✅ |
| **SSE 第二条连接** | 先建 1 条并保持，0.5 s / 1 s 后再开 ⇒ **429 `TOO_MANY_STREAMS`**（raw socket 3/3 次；`http.client` 同）；6 条并发（token 上限 1）⇒ **1×200 + 5×429** | ✅ |
| SSE 空闲超时 | `--sse-idle-timeout-ms 4000` ⇒ 流在 ~4.0 s 后结束，审计 `event=sse … elapsed_ms=4006 code=OK note=idle_timeout` | ✅ |
| 全局 SSE 上限（4） | 与每 token 上限分别计数；超限均 429 | ✅ |

⚠️ 客户端差异（记录，避免误判）：用 Python `requests` 在线程里开两条 SSE 时偶见**两条都 200**；同一场景改用 raw socket 或 `http.client` **稳定 429**（服务端计数由 `Mutex` 串行化，6 条并发只发 1 个 200）。结论以 raw socket 为准：**服务端限额正确**，`requests` 那条是客户端连接释放/复用行为。

---

## 6 · 审计（AC-007）✅

实际跑了成功 / 401 / 403 / 429 / 超时 / SSE 空闲超时，审计文件 `qa-req123/artifacts/audit.log`（110 行）+ `audit-timeout.log`（1 行）：

| 判据 | 实测 |
| --- | --- |
| 每次调用一行（含失败） | ✅ 事件覆盖 `auth / catalog / sparql / sse`；code 覆盖 `OK / UNAUTHORIZED / TENANT_MISMATCH / TOO_MANY_CONCURRENT / TOO_MANY_STREAMS / TIMEOUT / UNKNOWN_CLASS / UNKNOWN_PROPERTY / TENANT_UNRESOLVED / SPARQL_UNSUPPORTED / BAD_REQUEST` |
| 字段齐全 | ✅ 每行都有 `ts / event / proto / token(sha256:12) / tenant / q(sha256:16) / rows / elapsed_ms / code`（`lines_missing_fields=0`） |
| ⛔ 无 token 明文 | ✅ 3 个 token 原文在 111 行里命中 **0** 次（只有 `token=sha256:…`） |
| ⛔ 无查询原文 | ✅ 查询片段（`balances__recharge_balance` / `licenses__customers` / `no_such_class` / `INSERT DATA` …）命中 **0** 次 |
| 失败也有一行 | ✅ 401（无身份，`token=- tenant=-`）、429、超时各有独立行 |

---

## 7 · 绑定与启动（AC-008）✅

| 用例 | 实测 |
| --- | --- |
| `--bind 0.0.0.0:8813`（无 `--allow-public`） | **拒绝启动**，exit=1：「✗ 拒绝绑定 0.0.0.0:8813：它既不是本机…必须显式加 `--allow-public`…内网写法示例…」 | ✅ |
| 同上 + `--allow-public` | 启动成功（`/api/health` 200），横幅末行 **⚠ 警告**「绑定的是 0.0.0.0:8813（所有网卡）—— 查询服务可被本机以外访问…」 | ✅ |
| 横幅内容 | 绑定地址（仅本机/所有网卡）+ 项目/版本 + **token 3 个** + **租户 2 个（id 列表）** + 语法 v1 + 超时 / 单次最多行数 / 全局并发 + **只读会话读回值**（`transaction_read_only=on`、`standard_conforming_strings=on`）+ 审计口径 | ✅ |

回环绑定（127.0.0.1:8811）横幅无警告，符合「内网不吓人、公网必须警告」。

---

## 8 · 错误可行动（AC-006）✅ 抽 4 个 code

| code | HTTP | hint 能否据此修正 | 判 |
| --- | --- | --- | --- |
| `UNKNOWN_PROPERTY` | 400 | 直接列出该类的 18 个属性名（`:customers__address` …）+ 指向 `/api/catalog` / `catalog` 工具 | ✅ |
| `UNKNOWN_CLASS` | 400 | 列出本版 84 个类（截断到「等 84 个」）+ 指向 catalog | ✅ |
| `TENANT_UNRESOLVED` | 500 | 点名表 `admin_users` + 给 `query-policy.json` 的两条登记写法（`perTable` / `publicDimensions`）+ 说明是部署侧声明缺口、改完要重启、fail-closed 不猜列 | ✅ |
| `TENANT_MISMATCH` | 403 | 「租户由凭证决定：你这张 token 绑定的是 `f953…`；去掉 tenant 字段或换 token」 | ✅ |

---

## 9 · `serve` 未动 + 不退化（AC-009 / AC-011）✅

| 判据 | 实测 |
| --- | --- |
| `git diff develop -- src/server/http.rs` | **空**（0 行） | ✅ |
| 四红线 + `tools.rs` + `guard()` + `property_filter.rs` | `tools.rs` 与 `property_filter.rs` diff 均 **0 行**；`git diff develop -- src` 里 `guard(` 命中 **0** | ✅ |
| `Cargo.lock` 无新 crate | `[[package]]` 260 → 260（见 §3） | ✅ |
| lib / doc / 新增 | lib **562 passed / 0 failed**；doc **4 / 4**；新增 `test_req123_live` **8 / 8** | ✅ |
| 删测试 / `#[ignore]` / 放宽断言 | 测试 diff 只有 `+16/-6`、`+16/-0`（条数口径 48→52、模块与出口函数登记），**没有删除**、**没有新增 `#[ignore]`** | ✅ |
| 出口登记 | `OUTLETS` 48 → **52**（客户 17 / 建模 10 / 内部 **22** / 静态 2 / 错误路径 1）；I14 REST / I15 MCP / I16 SSE / I17 审计，**全部内部轨**；`test_req107_*` 双向对账同步更新且断言仍是精确相等 | ✅ |
| AC-009 只读 | 写类 SPARQL（`INSERT DATA`）⇒ 400 `SPARQL_UNSUPPORTED`「v1 只支持 SELECT…本端点只读」；启动横幅打印**读回**的 `transaction_read_only=on` | ✅ |
| AC-010 真库 5 条 | 单跳 / 两跳 / 聚合 / GROUP BY / ORDER BY+LIMIT 全部 200 且 `row_count` 合理；命中 `main_only` 表（`deductions`）时 `warnings` 点名覆盖范围 | ✅ |
| AC-001 / AC-002 | health 免 token 零数据；catalog 分页（`counts{classes:84, properties:952, relations:70}`）；describe 未知 ⇒ 404 + hint；缺参 ⇒ 400；MCP `initialize` / `tools/list`（4 工具含 `sparql_query`）/ `tools/call` / `GET /mcp` SSE 均可用 | ✅ |

### 5 条既有红灯：基线逐条复现 ⇒ 与本需求无关

在独立 worktree `qa-req123/baseline`（detached `e3da616` = develop HEAD）跑同样的 4 个测试目标，得到**完全相同**的 5 条失败：

| # | 测试 | 失败原因（基线同样失败） | 归因 |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | 读不到 `../.review-workspace/fix-props/schema.json`（本机无此开发路径） | 环境/本地文件，非代码 |
| 2 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | `items` 少一条 `add_link_type`（REQ-059 区域） | 既有缺陷，与本需求无关 |
| 3 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 读不到 `/home/myg133/…/schema.json`（Linux 开发机路径；可用 `ONTOK_SCHEMA_JSON` 指定） | 环境/本地文件 |
| 4 | `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | 名字登记行漂移 | 既有，REQ-084 区域 |
| 5 | `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | 同上 | 既有，REQ-084 区域 |

（全量输出：`qa-req123/artifacts/full-test-all.txt`；基线复现命令见 §11。）

---

## 10 · 非阻断项（建议本轮或下轮处理）

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| **N1** | AC-004 测试盲区（M1 变异存活） | 三条对拍查询都不产生 warning ⇒ MCP「少带 warnings」抓不到。把 `deductions`（`main_only`，必出 warning）加进 `ac004` 对拍集合即可；顺便再对一条 `truncated=true` 的查询（token C）对拍。 |
| **N2** | MCP `structuredContent` 的键序与 `content[0].text` 不同 | `structuredContent` 走 `serde_json::to_value`（`BTreeMap`）⇒ 键按字母序（`columns, coverage, join_states, limits(…timeout_ms 在 token_max_rows 前), …`），而 `text` 是声明序。**值完全相同**，但同一响应里两份表示键序不同；若有客户端拿 `structuredContent` 与 REST 正文做字节对拍会误报。建议：要么两边都从同一字符串解析，要么在文档里说明「以 `content[0].text` 为准」。 |
| **N3** | 仓库根有垃圾文件 `NUL`（222 B，untracked） | Windows 设备名，`git add -A` 会把它带进提交（`git add -n NUL` 显示会 add）。**合并前删除**。 |
| **N4** | manifest 与代码/依赖不一致 | `.feature/manifest.json` 写「tokio features 显式加了 `sync` / `time`（用到 mpsc）」，实际 `Cargo.toml` 只加了 `time`，且 `Cargo.toml` 注释明确「⛔ 也**不用** `tokio::sync`」。改成只写 `time` 即可。 |
| **N5** | SSE 客户端断开不记审计 | 只有空闲超时记 `note=idle_timeout`；客户端主动断开在下一次写失败时才被发现，且**不写审计**（`AuditRecord.note` 的 `client_gone` 预留了但未用）。AC-005 只要求空闲超时记审计，故非阻断；建议登记为已知限制（模块头已写「客户端断开只能在下一次写时发现」，但未说「不记审计」）。 |
| **N6** | `mod.rs:35` 与 `:40` 自相矛盾 | 属 B1 的一部分，修 J-2 时一并改。 |
| **N7** | `requests` 开两条 SSE 偶发双 200 | 客户端连接释放行为（raw socket / `http.client` 稳定 429）。记录以免后续 review 重复调查。 |

---

## 11 · 复现命令与证据

```powershell
# 起服务（真库）
$env:ONTOK_DATABASE_URL="postgresql://postgres:postgres@172.17.0.87:5432/anspire_real"
$env:ONTOK_SOURCE_DATABASE_URL="postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat"
cd feature-REQ-123
.\target\debug\ontok.exe query serve --bind 127.0.0.1:8811 `
  --tokens ..\qa-req123\artifacts\tokens.json --project anspire_r2rml_excl --version 2 `
  --schema ..\code\out\schema.json --policy query-policy.json `
  --timeout-ms 10000 --sse-idle-timeout-ms 4000 --sse-max-connections 4 `
  --max-concurrent-queries 2 --audit-out ..\qa-req123\artifacts\audit.log

# 两协议对拍 / 租户攻击 / 限额 / SSE / 错误 hint
python qa-req123\p0_part1.py            # §1 §2（对拍 + 6 项攻击）
python qa-req123\p0_leak.py             # §2 第 5/6 项（含 B 侧对照）
python qa-req123\p0_part2.py            # §5 §8
python qa-req123\p0_trunc.py            # §5 截断
python qa-req123\p0_rest.py             # AC-001 / AC-002 / main_only 警告
python qa-req123\p0_warn.py             # 有 warnings / 有三表 join_states 的对拍
python qa-req123\sse_raw.py             # §5 SSE 第二条 429
python qa-req123\sse_raw_concurrent.py  # §5 6 并发 SSE ⇒ 1×200
python qa-req123\audit_check.py         # §6 审计否定判据

# 测试基线
cd feature-REQ-123
cargo test --lib                          # 562 passed
cargo test --doc                          # 4 passed
cargo test --test test_req123_live        # 8 passed
cargo test --tests --no-fail-fast         # 5 条既有红灯（见 §9）

# 基线复现（develop e3da616）
git worktree add --detach ..\qa-req123\baseline e3da616
cd ..\qa-req123\baseline
cargo test --test drop_rate --test test_fk_naming --test test_retained_names_req084 --test test_agent_orchestration --no-fail-fast
```

证据文件（`qa-req123/artifacts/`）：
`p0_part1.json|log`、`p0_leak.json|log`、`p0_part2.json|log`、`p0_trunc.json|log`、`p0_rest.log`、`p0_warn.log`、`sse_raw*.log`、`sse_matrix.log`、`audit.log`、`audit-timeout.log`、`audit_check.log`、`help-serve.txt`、`mutation-M1|M3|M4.txt`、`full-test-all.txt`。

---

## 12 · 结论

- **FAIL**。核心安全（租户只来自 token）与核心一致性（两协议逐字节相同）**实测成立**，限额 / 审计 / 绑定 / 错误契约 / 不退化也全部通过；但 BA 在 J-2 里给的**接受条件没有兑现**（回执与 `--help` 都没标 `join_states` 的范围，模块头还留着被明确禁止的「本次用到的边」措辞），且**整条需求还没有一个 commit**（现在合并会把实现全部丢掉）。
- 两项都属小改：加 `join_states_scope` 字段 + 三处文案 → 重跑 `test_req123_live` 与 lib 即可复核；提交时记得删掉 `NUL`。
- 另外建议顺手补 AC-004 的 warning 覆盖（§10-N1），否则这条「防漂移」判据只防住了「无 warning 的查询」。
