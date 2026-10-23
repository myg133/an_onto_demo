# REQ-123 · 查询服务对外（HTTP API + 远程 MCP）· 复审（rev.2）

- **审核对象**：`feature-REQ-123`（分支 `feature/REQ-123`，HEAD **`09526b0`**，基线 `develop` `e3da616`）
- **审核日期**：2026-10-23
- **本轮只验四件**：B2（分支 0 commit）、B1（`join_states` 范围四处）、M1（AC-004 的 warning 盲区）、是否引入新问题
- **结论**：**PASS 可合并**。上一轮两个阻断项（B1 / B2）都已真落地，M1 变异已能打死；LIVE_GATE 不掩盖真问题。

| 上一轮阻断 | 本轮实测 | 判 |
| --- | --- | --- |
| **B1** 回执无范围字段 / `--help` 无 / MCP description 无 / 模块头自相矛盾 | 四处全部落地，值 = `all_edges_between_involved_tables`，三表回执 `used:false` 实测正确 | ✅ 已修 |
| **B2** 分支 0 commit / 实现全是 untracked / `NUL` 待入库 | `develop..HEAD` = **2**；7 个 `src/query/` 文件 + live 测试 + `tokens.example.json` 都在 HEAD 树；索引无 `NUL`；独立 worktree 从零 `cargo check --lib` 通过 | ✅ 已修 |
| **M1**（上轮非阻断建议）MCP 清空 `warnings` 变异存活 | `ac004` 对拍集合已加 `deductions`（必产生 warning）；只改 MCP 清 `warnings` ⇒ **红**；只改 REST 清 `coverage` ⇒ **红** | ✅ 已补 |

---

## 0 · 环境与方法

| 项 | 值 |
| --- | --- |
| 服务 | HEAD 编译的 `target\debug\ontok.exe query serve --bind 127.0.0.1:8821`（元数据库 `172.17.0.87/anspire_real`、源库 `172.16.120.20:30432/anspire_open_uat`、项目 `anspire_r2rml_excl` 第 2 版、`--timeout-ms 10000`） |
| token 表 | 沿用上一轮的 `qa-req123/artifacts/tokens.json`（A = `f953b797…c12c7` 200 行 / 并发 2；B = `a62f1fa2…023d`；C = 同 A 3 行） |
| 干净编译 | 独立 worktree `qa-req123/review2`（detached `09526b0`）+ **独立** `CARGO_TARGET_DIR=qa-req123/target-review2` |
| 变异 | 两个独立 worktree `qa-req123/mutant2`（M1）、`qa-req123/mutant2b`（M1b），均 detached `09526b0`，**未改 feature 分支** |
| 基线 | 独立 worktree `qa-req123/baseline2`（detached `e3da616` = develop HEAD） |
| 证据 | `qa-req123/artifacts/review2-*`（详见 §7） |
| 数据库环境变量 | 跑 lib 与 4 个既有红灯目标时设 `ONTOK_TEST_DATABASE_URL=…/ontok_test_req122`（详见 §6 的「环境变量口径」） |

> ⛔ 未改 feature 分支、未 push、未提交任何东西；所有改动只落在 `qa-req123/` 下的独立 worktree/拷贝。

---

## 1 · B2 复验：分支真的提交了 ✅

### 1.1 提交数

```text
$ git rev-list --count develop..HEAD
2

$ git log --oneline -2
09526b0 [Dev] REQ-123 修 B1：join_states 范围四处标清… + 每条边加 used + 对拍集合加 warning 查询
5ecca46 [Dev] REQ-123 查询服务对外：REST 四端点 + MCP(JSON-RPC/SSE)…
```

### 1.2 HEAD 树里的关键文件（`git ls-tree -r HEAD`）

| 路径 | blob 大小 | 在 HEAD 树 |
| --- | --- | --- |
| `src/query/mod.rs` | 61,569 B | ✅ |
| `src/query/http.rs` | 49,192 B | ✅ |
| `src/query/serve.rs` | 20,084 B | ✅ |
| `src/query/tokens.rs` | 13,972 B | ✅ |
| `src/query/catalog.rs` | 22,242 B | ✅ |
| `src/query/error.rs` | 22,201 B | ✅ |
| `src/query/audit.rs` | 16,026 B | ✅ |
| `tests/test_req123_live.rs` | 29,389 B | ✅ |
| `tokens.example.json` | 472 B（全是假值） | ✅ |

`src/query/` 恰好 **7 个文件**，与任务书一致。

### 1.3 垃圾文件 `NUL`

```text
$ git ls-files | Select-String '^NUL$'      → 空
$ git ls-tree -r --name-only HEAD | … '^NUL$' → 空
$ git status --porcelain -uall               → 0 行（无 untracked）
$ Test-Path feature-REQ-123\NUL              → False
```

上轮 N3 的 `NUL` 已既不在索引、也不在工作区。

### 1.4 干净 worktree 编译（证明不靠 untracked 文件）

```text
$ git worktree add --detach qa-req123\review2 09526b0
$ $env:CARGO_TARGET_DIR="…\qa-req123\target-review2"   # 空目录，不是 feature 的 target
$ cd qa-req123\review2; cargo check --lib
    Checking ontok v0.1.0 (D:\MyCodes\AI\an_onto_demo\qa-req123\review2)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 12s
```

从零编依赖 + 本 crate，**exit 0**。日志：`artifacts/review2-cargo-check-lib.log`。⇒ 编译不依赖任何 untracked 文件。

---

## 2 · B1 四处是否真落地 ✅

判据：BA 裁定 J-2 要求「两处写清」（① 字段/`--help`；② 回执显式说明），且字段名不许暗示「本次走到的边」。

| # | 位置 | 实测 | 判 |
| --- | --- | --- | --- |
| ① | **回执** | 三表查询回执键序：`… "join_states":[…],"join_states_scope":"all_edges_between_involved_tables","join_states_scope_note":"join_states[] 是「查询涉及的表之间」的全部映射边（来自声明/映射），不是本次查询实际走过的那几条；每条边的 used=false 表示「涉及但未用」，used=true 表示该边的谓词写进了查询。","limits":{…}` | ✅ 有字段、值正确、带人话 |
| ② | **`ontok query serve --help`** | 第 14–22 行单列一段：`⚠️ 回执里的 join_states[] 是什么范围（⛔ 别读成「本次查询走到的边」）… 值恒为 all_edges_between_involved_tables … 只要两张表都出现在 tables[] 里…每一条 join 都会列出来 —— 不论这次查询有没有用到它` + 每条边 `used` 的说明 | ✅ 出现且写明范围 |
| ③ | **MCP `tools/list` 的 `sparql_query.description`** | 实测（`POST /mcp` → `tools/list`）：`…join_states/join_states_scope/join_states_scope_note/limits。⚠️ join_states 的范围 = 「查询涉及的表之间」的全部映射边（join_states_scope 恒为 all_edges_between_involved_tables），⛔ 不是本次查询实际走过的那几条…所以会有 used=false 的条目…` | ✅ 写明范围 + 明说「不是走到的边」 |
| ④ | **`src/query/mod.rs`** | 旧的「`join_states[]`：本次用到的 join 边」**已删**；模块头改为「**涉及的表之间**的全部映射边 … + `used`」并新增「## `join_states[]` 的范围（⛔ 别读成「本次查询走到的边」）」一节；「已知限制」不再与上文矛盾（改成「有冗余条目，靠 `used`/`join_states_scope` 区分」）。全仓 `grep '本次用到的'` 在 `src/query/` 命中 0 处（剩下的都是「**不是**本次…」的否定句） | ✅ 自相矛盾已消 |

### 2.1 三表查询 `used` 实测（REST 与 MCP 同一字节）

查询（QA 自己打真库，不经 Dev 的测试）：

```sparql
SELECT ?l ?c ?b WHERE { ?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c }
```

回执（`artifacts/review2-probe.json`）：

| from | predicate | state | **used** | 本次查询里写了吗 |
| --- | --- | --- | --- | --- |
| balances | `balances__customers__customer_id` | true | **true** | ✅ 写了 |
| balances | `balances__customers__source_customer_id` | true | **false** | ❌ 没写（涉及但未用） |
| licenses | `licenses__customers` | true | **true** | ✅ 写了 |

- `tables = [licenses, customers, balances]`，`row_count = 33`（不是空转）。
- REST 正文 sha256 = MCP `content[0].text` sha256 = `09a7eb2a…2e07`，**逐字节相同**。
- 反证：两表查询 `?l :licenses__customers ?c` ⇒ `tables=[licenses, customers]`，只有 1 条边且 `used:true`（没有硬编码「总是 false」）。

⇒ `used:false` 的那条边**确实是本次未用到的**（它甚至不是 `?b` 与 `?c` 之间写出的谓词），字段名 `join_states_scope` 也不暗示「实走边」。B1 判据成立。

---

## 3 · M1 盲区：已补上，两个方向都能打死 ✅

### 3.1 前提：ac004 对拍集合里现在**有**会出 warning 的查询

`tests/test_req123_live.rs` 的 `ac004_rest_and_mcp_receipts_are_byte_identical` 从 3 条扩到 4 条，第 4 条是：

```rust
("覆盖范围不完整（main_only）",
 "SELECT ?d ?amt WHERE { ?d :deductions__deduction_amount ?amt }", true)
```

实测这条查询确实产生 warning（`query-policy.json` 里 `deductions.coverage = main_only`）：

```
"warnings":["表 deductions：租户过滤用 customer_id，覆盖范围 = main_only —— main_customer_id 全空（0/4）；…"]
```

并且测试里加了三重断言：`must_warn ⇒ REST warnings 非空`、`MCP warnings == REST warnings`、`MCP warnings 非空`。**前提成立**（不是靠巧合：换表名/删声明会先红）。

### 3.2 变异 M1：只让 MCP 路径清空 `warnings`

变异体 `qa-req123/mutant2`（detached `09526b0`，**只改一行**）：

```rust
// src/query/http.rs · tools_call · "sparql_query" 分支
Ok(mut receipt) => { receipt.warnings.clear(); tool_ok(id, &receipt.serialize(), …) }
```

实测（`artifacts/review2-mutation-M1.log`）：

```text
thread 'ac004_rest_and_mcp_receipts_are_byte_identical' (22780) panicked at tests\test_req123_live.rs:244:9:
assertion `left == right` failed: ⛔ 两协议回执不逐字节相同（AC-004）[覆盖范围不完整（main_only）]：
 REST: {…,"warnings":["表 deductions：租户过滤用 customer_id，覆盖范围 = main_only —— main_customer_id 全空（0/4）；…"],…}
 MCP : {…,"warnings":[],…}
test ac004_rest_and_mcp_receipts_are_byte_identical ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 8 filtered out
```

✅ **必须红 ⇒ 红**（且红在新增的第 4 条查询上，正是上轮盲区所在）。

### 3.3 反向 M1b：只让 REST 路径清空 `coverage`

变异体 `qa-req123/mutant2b`：

```rust
// src/query/http.rs · post_sparql
Ok(mut receipt) => { receipt.coverage.clear(); respond(StatusCode::OK, receipt) }
```

实测（`artifacts/review2-mutation-M1b.log`）：

```text
thread 'ac004_rest_and_mcp_receipts_are_byte_identical' (37640) panicked at tests\test_req123_live.rs:244:9:
assertion `left == right` failed: ⛔ 两协议回执不逐字节相同（AC-004）[单跳]：
 REST: {…,"coverage":[],…}
 MCP : {…,"coverage":[{"table":"balances","coverage":"complete"}],…}
test ac004_rest_and_mcp_receipts_are_byte_identical ... FAILED
```

✅ **必须红 ⇒ 红**。

> ⚠️ 方法学备注（避免下次误判）：两个变异体**共用** feature 的 `CARGO_TARGET_DIR` 时，cargo 会因「包名+版本相同」把 M1 的测试二进制当成 M1b 的新构建（实测第一次跑 M1b 时 `Finished in 1.10s`、跑出来的是 M1 的行为）。本轮 M1b 在 `cargo clean -p ontok` 后重编（50.6 s）才得到上面的正确结果。**结论不受影响**（两次都红，且第二次红在「单跳」而非 warning 查询，反证变异真的换了）；下次做多变异时每个变异体应各用一个 target 目录。

---

## 4 · LIVE_GATE 判断：**可接受，不掩盖真问题**

`tests/test_req123_live.rs` 新增 `static LIVE_GATE: Mutex<()>` + `one_at_a_time()`，9 个用例**每个**开头都拿锁（实测 9 处调用）。

判断依据：

| 问 | 实测/推理 | 结论 |
| --- | --- | --- |
| 它是不是只在避免源库连接池爆掉？ | 每个用例各自 `build_service` ⇒ 各自一个只读池（`max_connections = 2`）⇒ 9 个并行峰值 ~18 条连接；源库是 :30432 **代理**，超额会被重置。闸住后峰值 = 1 个池。生产部署只有**一个** `QueryService`（2 条连接），所以这是**测试夹具**的问题，不是产品行为 | ✅ 理由成立，不是「把产品缺陷藏起来」 |
| 有没有让某些用例不再真正并行 / 不再验证并发限额？ | `test_req123_live` 里**没有任何**用例依赖「用例之间并行」或「同一用例内并发打请求」：全文无 `spawn` / `join!` / 429 断言。并发限额的判据在 lib 单测（`query::http::tests::a_second_concurrent_query_is_429` 等，本轮 563 全绿）与 QA 的 HTTP 探针里，不受这把锁影响 | ✅ 没有削弱任何判据 |
| 会不会静默跳用例？ | 不会。锁只是串行；`cargo test` 实测 **9 passed / 0 filtered**，用时 14.31 s。缺环境变量时仍是原来的「响亮 SKIP」 | ✅ |
| 有没有引入新的脆弱点？ | 持锁跨 `.await`（`std::sync::MutexGuard`）—— 当前 `#[tokio::test]` 默认 current_thread，可编译可运行；若将来改成 `flavor = \"multi_thread\"` 会**编译失败**（响亮的、不是静默死锁）。panic 毒化已用 `unwrap_or_else(|e| e.into_inner())` 兜住，不会把 1 个失败放大成 8 个 | ✅ 可接受（小瑕疵见 §7-N2） |

⇒ **LIVE_GATE 只是把「测试夹具同时开 9 个池」串行化，不掩盖真问题，也不改变任何并发限额判据。**

---

## 5 · 不退化 ✅

| 判据 | 实测 | 判 |
| --- | --- | --- |
| lib 测试 | **563 passed / 0 failed**（`artifacts/review2-lib-with-testdb.log`） | ✅ 与声明一致（上轮 562，+1：`the_scope_label_is_not_the_traversed_edges`） |
| doc 测试 | **4 passed / 0 failed** | ✅ |
| live 测试 | **9 passed / 0 failed**（14.31 s，真库，`review2-live-baseline.log`） | ✅（上轮 8，+1：`b1_the_scope_and_the_unused_edge_are_visible_in_the_receipt`） |
| 5 条既有红灯 | HEAD 上复现 **5 条**，与上轮逐条同因；在独立 worktree `baseline2`（`e3da616`）上**同样 5 条**（`review2-red-tests*.log`） | ✅ 非本需求引入 |
| `serve` 未动 | `git diff develop -- src/server/http.rs` = **0 行** | ✅ |
| 四红线 / `tools.rs` / `property_filter.rs` | **跑了判据本身**：`cargo test --test test_req107_red_lines` ⇒ `2 passed`（`test_req107_red_lines_and_guard_are_untouched` 直接 `git diff develop` 四个红线文件 `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` 判空、`src/mcp/tools.rs` 的 `guard(` 实现行判空、`property_filter.rs` G1–G4 逐字节判等）；另外 `tools.rs` diff **0 行**、`property_filter` 无 diff、`git diff develop -- src` 里新增 `guard(` = **0** | ✅ |
| `Cargo.lock` | `[[package]]`：develop **260** → HEAD **260** | ✅ 无新 crate |
| 删测试 / `#[ignore]` | 删除的 `#[test]`/`#[tokio::test]` = **0**；新增 `#[ignore]` = **0**；新增测试 = 52 | ✅ |
| 出口登记 | `OUTLETS` 48 → **52**（I14 REST JSON / I15 MCP / I16 SSE / I17 审计，全部内部轨；客户 17 / 建模 10 / 内部 22 / 静态 2 / 错误路径 1），`test_req107_*` 双向对账同步 | ✅ |
| 提交干净度 | `git status --porcelain -uall` = 0；无 `target/`、无日志、无 `NUL` 入树 | ✅ |

补充：本分支对 `src/domain/ttl.rs` 只有一处改动 —— `ontology_iri()` 由 `pub(crate)` 改为 `pub`（附理由：查询服务与现场判据都要拼实例 IRI，避免抄第二份规则），**无行为变化**；该文件不在四个红线文件之列，红线判据实测通过。

### 5 条红灯（本轮实测，与基线逐条一致）

| # | 测试 | 失败原因 | 归因 |
| --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | 读不到 `../.review-workspace/fix-props/schema.json`（本机无此开发路径） | 环境/本地文件 |
| 2 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | `items` 少一条 `add_link_type`（REQ-059 区域） | 既有缺陷 |
| 3 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 读不到 `/home/myg133/…/schema.json` | 环境/本地文件 |
| 4 | `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | `src/extract/propose.rs:280` 登记行漂移（登记 `customer_rebate`，实际是另一行注释） | 既有，REQ-084 |
| 5 | `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | `src/domain/concept.rs` 实际 9 处已登记表名 / 登记表只记 5 处 | 既有，REQ-084 |

**归因证据**：本分支 `git diff --name-only develop` 共 18 个文件，**没有** `tests/test_{drop_rate,fk_naming,retained_names_req084,agent_orchestration}.rs`、`src/extract/**`、`src/agent/**`、`src/domain/concept.rs`；且 `baseline2`（develop HEAD）跑同样 4 个目标得到**完全相同**的 5 条失败。

### 环境变量口径（重要，避免「红灯数」误判）

不设 `ONTOK_TEST_DATABASE_URL` 时，会**多出 3 条纯环境失败**（不是回归）：

- `cargo test --lib` ⇒ 561 passed / **2 failed**：`db::apply::tests::write_snapshot_…`、`extract::introspect::tests::read_only_pool_really_reads_back_on`，panic 原文都是 `缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库`;
- 4 个红灯目标 ⇒ **6 条**：多出 `test_agent_orchestration::test_cli_default_and_registry_paths_remain_byte_identical`，同样是 `MissingEnv("ONTOK_TEST_DATABASE_URL")`。

设 `ONTOK_TEST_DATABASE_URL=postgresql://postgres:postgres@172.17.0.87:5432/ontok_test_req122` 后即回到 563 / 4 / 9 + 5 条既有红灯。Developer 的「563」口径与此一致。

---

## 6 · 仍缺什么

全部为**非阻断**，不挡合并；建议下轮或后续需求处理。

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| **N1** | `.feature/manifest.json` 的测试备注过时 | 仍写「真库用例并行跑偶尔被源库代理重置 ⇒ 加 `--test-threads=1` 最稳」，并**未登记** `LIVE_GATE`。实际已由闸解决（默认并行也能跑）。建议改成「已加进程内串行闸 `LIVE_GATE`（仅串行化连源库的段），`--test-threads=1` 不再必需」。 |
| **N2** | `LIVE_GATE` 持锁跨 `.await` | `std::sync::MutexGuard` 在 `.await` 期间跨线程持有；current_thread 下正确，但若有人把用例改成 multi-thread runtime 会编译失败。建议后续换成 `tokio::sync::Mutex` 或在注释里钉死「必须 current_thread」。 |
| **N3** | `predicate_local` 与 planner 的 `local_of` 是两份实现 | 口径相同（`#` 优先、其次 `/`），但属重复规则；建议后续抽到一处，或加一条「两者对同一 IRI 结果相同」的单测。 |
| **N4** | `structuredContent` 与 `content[0].text` 键序不同（上轮 N2 仍在） | `structuredContent` 走 `serde_json::to_value`（BTreeMap 字母序），`text` 是声明序。值相同；若客户端拿两者字节对拍会误报。建议文档注明「以 `content[0].text` 为准」，或两边同源。 |
| **N5** | `used` 的判据是「谓词出现在查询里」 | 依赖 planner「只为写出来的关系谓词建 join」这一前提（模块头已写明）。若将来 planner 允许隐式 join/笛卡尔积，`used` 会失真 —— 届时应改成从 `Planned` 取实走边（模块头已把这条列为后续需求）。当前无问题。 |
| **N6** | `?c ^:licenses__customers ?l` 反向语法不支持 | v1 明确拒绝属性路径（400 `SPARQL_UNSUPPORTED`，hint 让写反向谓词名）。`used` 对「写反向谓词名」是认的（`inverse_local` 比对），与 v1 语法一致；只是记录此边界，避免有人以为 `^` 可用。 |

---

## 7 · 复现命令与证据

```powershell
# B2：干净 worktree 编译
git worktree add --detach qa-req123\review2 09526b0
$env:CARGO_TARGET_DIR="D:\MyCodes\AI\an_onto_demo\qa-req123\target-review2"
cd qa-req123\review2; cargo check --lib

# B1：起服务 + 三表查询 / tools/list / warning 查询
cd feature-REQ-123
.\target\debug\ontok.exe query serve --bind 127.0.0.1:8821 `
  --tokens ..\qa-req123\artifacts\tokens.json --project anspire_r2rml_excl --version 2 `
  --schema ..\code\out\schema.json --policy query-policy.json --timeout-ms 10000 `
  --audit-out ..\qa-req123\artifacts\review2-audit.log
python qa-req123\review2_probe.py          # → artifacts/review2-probe.json

# M1 / M1b 变异（每个变异体独立 worktree；⚠️ 不要共用 target 目录）
git worktree add --detach qa-req123\mutant2  09526b0   # tools_call: receipt.warnings.clear()
git worktree add --detach qa-req123\mutant2b 09526b0   # post_sparql: receipt.coverage.clear()
$env:ONTOK_DATABASE_URL=…; $env:ONTOK_SOURCE_DATABASE_URL=…
$env:ONTOK_REQ123_SCHEMA=D:\MyCodes\AI\an_onto_demo\code\out\schema.json
$env:ONTOK_REQ123_TENANT=f953b797-7eeb-4bdb-ab3d-d906d44c12c7
cd qa-req123\mutant2;  cargo test --test test_req123_live ac004_rest_and_mcp_receipts_are_byte_identical -- --nocapture
cd ..\mutant2b;        cargo test --test test_req123_live ac004_rest_and_mcp_receipts_are_byte_identical -- --nocapture

# 测试计数（需测试库环境变量）
$env:ONTOK_TEST_DATABASE_URL="postgresql://postgres:postgres@172.17.0.87:5432/ontok_test_req122"
cd feature-REQ-123
cargo test --lib                 # 563
cargo test --doc                 # 4
cargo test --test test_req123_live   # 9
cargo test --test drop_rate --test test_fk_naming --test test_retained_names_req084 --test test_agent_orchestration --no-fail-fast  # 5 red

# 基线复现
git worktree add --detach qa-req123\baseline2 e3da616
```

证据文件（`qa-req123/artifacts/`）：
`review2-cargo-check-lib.log`、`review2-help-serve.txt`、`review2-probe.json|log`、`review2-live-baseline.log`、`review2-lib-with-testdb.log`、`review2-doc.log`、`review2-red-tests-with-testdb.log`、`review2-red-tests-baseline.log`、`review2-mutation-M1.log`、`review2-mutation-M1b.log`、`review2-serve.log`、`review2-audit.log`。

---

## 8 · 结论

- **PASS 可合并**。
- **B2 真修好**：2 个 commit，实现全部在 HEAD 树里，`NUL` 不在索引，独立 worktree 从零 `cargo check --lib` 通过。
- **B1 四处全部落地**：回执 `join_states_scope` = `all_edges_between_involved_tables` + `join_states_scope_note`；`--help` 有；MCP `tools/list` description 有（且明说「不是本次实际走到的边」）；模块头旧措辞已删且不再自相矛盾。三表查询实测 `used:false` 确实对应「涉及但未用」的边。
- **M1 盲区已补**：ac004 对拍集合新增必产生 warning 的 `deductions` 查询；只清 MCP `warnings` ⇒ 红，只清 REST `coverage` ⇒ 红。
- **LIVE_GATE 可接受**：只是把「每个用例各建一个只读池」串行化，避免代理重置；不用例依赖并行，也不削弱任何并发限额判据。
- 上轮 5 条既有红灯在 HEAD 与基线 `e3da616` 上**逐条一致**，与本需求无关；lib 563 / doc 4 / live 9 全部可达（需 `ONTOK_TEST_DATABASE_URL`，否则多 2–3 条纯环境失败）。
