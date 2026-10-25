# REQ-142 · 查询默认返回 JSON-LD — Pre-merge QA 审核

> QA Agent · 2026-10-25 · 被测 `feature/REQ-142` `972582e`（基线 `develop` `be483e7`，1 个提交）
> 本报告只判「翻默认有没有顺手动坏别的」：两个入口的默认、`Accept` 四态、退回 rows 的逐字节、非法 `format`、内部消费者、测试与不退化。

## 一 · 结论

**PASS（可合入）· 带 3 条非阻断观察**（都不改代码，见 §13）。

| # | 检查项 | 结果 |
| --- | --- | --- |
| 1 | 默认翻转（REST + MCP 两个入口） | ✅ 两入口默认都是图（`@context/@graph/summary/meta`） |
| 2 | `Accept` 四态（含 `*/*`） | ✅ 四态全对；变异 `*/*`→rows **红 2 条** |
| 3 | 退回 rows 逐字节等于旧默认（R2） | ✅ 3 查询 × 2 种退回方式 × 3 组 sha 全同 |
| 4 | JSON-LD 内容没变 | ✅ 3/3 sha 相同（与基线 `?format=jsonld`） |
| 5 | 优先级 `?format` > body > `Accept` > 默认 | ✅ 三行全对（R-1 口径） |
| 6 | 非法 `format`/参数仍 400（不静默回退） | ✅ 400 + 可行动 hint，原文见 §7 |
| 7 | 内部消费者（工作台查询台） | ✅ `page.rs:1071` 显式 `format:"rows"`，页面实抓 + 实跑都对 |
| 8 | 测试没被放宽 | ✅ 6 处只加 `format`（17 个既有函数的断言行**零差异**），总数 1798→1800 |
| 9 | 不退化 | ✅ 红线/`tools.rs`/`guard()`/`property_filter.rs`/`Cargo.*` 零 diff；红集合基线与分支**逐条同名**；非 sparql 端点 5/5 逐字节相同 |

---

## 二 · 默认真的翻了（两个入口）

同一份声明（`code/out/schema.json` + `code/query-policy.json`，两 worktree 的 `query-policy.json` sha 同为 `BCA3156D…`）、同一 token、同一项目/版本；基线二进制 `11D65383…b23e9a`、分支二进制 `DA3670A8…37303b`（都在 `.local/qa142/bin/`）。

| 入口 | 调法 | 顶层键（实测） | 字节 | 结论 |
| --- | --- | --- | --- | --- |
| REST | 无 `format`、无 `?format`、无 `Accept` 头 | `@context, @graph, summary, meta` | 1324 | ✅ **图** |
| MCP | `sparql_query` 省略 `format` | `structuredContent` = `@context, @graph, summary, meta` | 2876（JSON-RPC 信封） | ✅ **图** |

- REST 三条查询（单跳/两跳/聚合）默认全部是图；`@graph` 都是数组、**没有** `rows`/`columns`。
- MCP 省略 `format` 时 `content[0].text` 与 REST 正文**逐字节相同**（3/3：`3cb18064…` / `e8c5d570…` / `a3e66c76…`）。
- 附带的行为面（属预期，但**所有消费者都会看到**）：默认回执的 `Content-Type` 由 `application/json` 变为 **`application/ld+json; charset=utf-8`**。

---

## 三 · `Accept` 四态（本次最大的行为变化）

分支实测（三条查询一致，数字为 q1 单跳）：

| `Accept` | 期望 | 实测 | 字节 | sha（前 16） |
| --- | --- | --- | --- | --- |
| `application/ld+json` | 图 | ✅ 图 | 1324 | `3cb180642b894d95` = 默认 |
| `application/json` | rows | ✅ 表（14 键，有 `columns/rows`） | 1170 | `081e59c99f438931` |
| `*/*` | 图 | ✅ **图** | 1324 | `3cb180642b894d95` = 默认 |
| 缺失（`-H 'Accept:'`） | 图 | ✅ 图 | 1324 | `3cb180642b894d95` = 默认 |

补充：`text/html, */*`、`application/*` 也落默认（图）；`application/ld+json, application/json` 同时出现 ⇒ **图**（R-4，已实测）。

**变异 M1**（独立 worktree `.local/qa142/mut-star`，detached `972582e`）：把 `accept_format()` 里 `*/*` 改回「要表」（REQ-140 语义），patch 见 `out/mut-M1-star-rows.diff`。

```
cargo test --lib --no-fail-fast  →  test result: FAILED. 731 passed; 2 failed
  ✗ query::http::tests::the_response_format_has_one_resolver_for_all_three_entry_points
  ✗ query::http::tests::req142_the_default_is_jsonld_on_both_entries
```

⇒ `*/*` 这条语义**有测试钉住**，改回去必红。⚠️ 边界：它只被 lib 单测钉住（2 条），live 用例不测 `*/*`；够用，但知道这点。

---

## 四 · 退回 rows 必须逐字节等于旧默认（R2，最关键）

基线二进制**默认**（旧行为=rows） vs 分支二进制两条退回路径，3 条查询逐一 sha256：

| 查询 | 退回方式 | 基线（旧默认） | 分支 | 判定 |
| --- | --- | --- | --- | --- |
| 单跳 `?b :balances__recharge_balance ?amt` | `?format=rows` | `081e59c99f43893165b8ed1079550e01089cd1d445925b082eea089ed4c5ce14`（1170 B） | 同左 | ✅ |
| 单跳 | `Accept: application/json` | 同上 | 同左 | ✅ |
| 两跳 `?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c` | `?format=rows` | `0b3bd25d58b093c75169f539f2313d299b3fbe745d87e19b15386eca4e37df6f`（2510 B） | 同左 | ✅ |
| 两跳 | `Accept: application/json` | 同上 | 同左 | ✅ |
| 聚合 `?c (COUNT(?b) AS ?n) … GROUP BY ?c` | `?format=rows` | `a13fa00615ed39d2fcde512a4abc4bad700ec35fd5e24cfbdaa345e962c0c896`（1776 B） | 同左 | ✅ |
| 聚合 | `Accept: application/json` | 同上 | 同左 | ✅ |

- **6/6 相同**；且 `?format=rows` 与 `Accept: application/json` 两条路**彼此也逐字节相同**（3/3）。
- MCP 同口径：`format:"rows"`（增量）== 基线 MCP 默认（旧行为）3/3（`3c92b2cc…` / `6fb7296c…` / `6847d2cc…`）。
- 结论：翻默认**没有**碰 rows 的序列化路径。

---

## 五 · JSON-LD 的内容没变

| 查询 | 基线 `?format=jsonld` | 分支**默认** | 判定 |
| --- | --- | --- | --- |
| 单跳 | `3cb180642b894d956bef9f3a1ea8ef20dfa13ceaa1d365e7f08241871fae4655`（1324 B） | 同左 | ✅ |
| 两跳 | `e8c5d570e0a8554c79e115194d8d0ab2784b70023843f253d5ca825e97f9abba`（2637 B） | 同左 | ✅ |
| 聚合 | `a3e66c7652b07078cb2fcca09cb3c795e0ed43d54a650f7ecbaf8cddca2d41a0`（1889 B） | 同左 | ✅ |

`Accept: application/ld+json`、`Accept` 同时含两者、`?format=jsonld` 三种显式要图的写法，**都与默认逐字节相同**（3×3 全同）⇒ 只有「选哪个」变了，图本身没变。

---

## 六 · 优先级顺序（R-1：`?format=` > body.format > `Accept` > 默认 jsonld）

| 组合 | 期望 | 实测 | sha 判据 |
| --- | --- | --- | --- |
| `Accept: application/json` + `?format=jsonld` | 图 | ✅ 图 1324 B | = 图 sha `3cb18064…` |
| `Accept: application/ld+json` + `?format=rows` | 表 | ✅ 表 1170 B | = rows sha `081e59c9…` |
| `?format=rows` + body `format:"jsonld"` | **表**（证明 `?format` > body） | ✅ 表 1170 B | = rows sha `081e59c9…` |

三行在三条查询上全部成立。代码侧同一条链（`src/query/http.rs` `sparql_format`：显式 `explicit` → `from_body` → `accept_format` → `DEFAULT_FORMAT`）与 R-1 一致；`?format` 与 body 的先后**沿用 REQ-140 已钉的行为，未改**。

---

## 七 · 非法 `format` 仍 400（⛔ 不静默回退）

| 请求 | 状态 | `code` | `message` / `hint` 原文 |
| --- | --- | --- | --- |
| `?format=bogus` | **400** | `BAD_REQUEST` | `format 只认 rows / jsonld，收到 "bogus"。`<br>`默认是 **JSON-LD 上下文子图**，不写 format 就是图；要**行回执**（表格）写 ?format=rows（或 Accept: application/json）。要显式要图写 ?format=jsonld。⛔ 拼错不会静默回退。` |
| `?form=jsonld`（拼错参数名） | **400** | `BAD_REQUEST` | `不认识的查询参数 "form"。`<br>``/api/sparql` 只接受 `format=rows&#124;jsonld`（省略 = JSON-LD 上下文子图）。` |

- 基线/分支同一条请求都是 400（没有从 400 松成 200）。
- hint 里**明写新默认**，且明说「拼错不会静默回退」——可行动。

---

## 八 · 内部消费者（R-6 之外的）

| 调用点 | 结论 |
| --- | --- |
| **工作台查询台** `src/console/page.rs:1071` | ✅ `var body = { query: q, format: "rows" };` —— **显式 rows**。实抓 `console serve` 的 `GET /`（60,482 B）：命中该行 2 处（源码 + 注释）；用页面这个 body 真打一次 query serve ⇒ **200 / 1170 B / `columns,rows,row_count,…`（14 键，无 `@graph`）** ⇒ 页面表格不会空转 |
| `src/query/mod.rs:1263` `sample` | 恒行回执（注释明写「JSON-LD 只有 `sparql_query` 一个入口」）⇒ 不受影响 |
| 门户 `src/portal/http.rs` | 没有 `/api/sparql`（只有 4 个端点）⇒ 不受影响 |
| `tests/fixtures/req125/dev_proxy.ps1` | 只按前缀转发，不解析回执 ⇒ 不受影响 |
| `tests/fixtures/req125/page_probe.js` | 用 `body:"{}"` 只断言 404/403 文案 ⇒ 不受影响 |
| `e2e-test/query/score.js` | 只数 `/api/sparql` 调用次数 ⇒ 不受影响 |
| 其余「会读回执字段」的消费者 = 文档（见 §11，属 R-6 欠账） | — |

---

## 九 · 测试没被放宽

**6 处 live 调用改成显式 rows（`tests/test_req123_live.rs`）**——只加 `"format": "rows"`，断言一个字没动：

| # | 行 | 所属用例 |
| --- | --- | --- |
| 1 | 250 | `ac004_rest_and_mcp_receipts_are_byte_identical`（REST body） |
| 2 | 255 | 同上（MCP `arguments`） |
| 3 | 427 | `b1_the_scope_and_the_unused_edge_are_visible_in_the_receipt` |
| 4 | 540 | `ac003_a_never_sees_another_tenants_rows` |
| 5 | 595 | `ac010_five_real_queries_return_receipts` |
| 6 | 790 | `ac005_the_token_row_cap_cannot_be_raised`（与 `max_rows` 并列） |

**断言行零改动（机器判据）**：把基线/分支两份文件按 `fn` 切段，逐函数比较所有以 `assert` 开头的行（trim 后）：

```
基线函数数=17  分支函数数=18
新增函数：req142_default_is_jsonld_on_rest_and_mcp
（其余 17 个函数：断言行 diff 为空）
```

删除行全量清单（`git diff` 里所有 `-` 行）确认：tests 侧的删除**只有**那 6 行 body/参数行（+ 一处 `max_rows` 逗号行），**没有删断言、没有删测试、没有新增 `#[ignore]`、没有删文件**。

**被翻转的断言只有 R-2 允许的那 2 条**（都在 lib 单测里）：`QueryFormat::default()` 的期望值、`*/*`/`application/json` 一组「默认是什么」的期望值。

**计数（实测，非抄 Dev）**：

| 项 | 基线 `be483e7` | 分支 `972582e` | 判定 |
| --- | --- | --- | --- |
| lib 单测 | **732** | **733** | ✅ +1（`req142_the_default_is_jsonld_on_both_entries`） |
| `test_req123_live` | **9** | **10** | ✅ +1（同名新用例） |
| 全量用例总数（104 个 target） | **1798** | **1800** | ✅ 只多这两条，其它 target 一条不差 |
| lib 红（无夹具） | 2 | 2 | 同名同数（都缺 `ONTOK_TEST_DATABASE_URL`） |

新用例**真的跑了**（不是 SKIP）：真库环境下 `test_req123_live` 基线 `9 passed`、分支 `10 passed`，唯一 SKIP 是 `ac003_a_never_sees_another_tenants_rows`（缺 `ONTOK_REQ123_TENANT_B`，R-5 已裁定接受）。

---

## 十 · 不退化

### 10.1 零 diff（逐个文件核过）

| 对象 | 证据 |
| --- | --- |
| 四红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs` | `git diff --numstat be483e7..972582e -- …` **空** |
| `src/mcp/tools.rs`（`guard()` 实现所在） | numstat **空** |
| `src/domain/property_filter.rs` | numstat **空** |
| `Cargo.toml` / `Cargo.lock` | numstat **空**（零新依赖） |
| 删除的文件 | `--diff-filter=D` **空** |
| 新增 `#[ignore]` | diff 里 **0 处** |

改动面只有 5 个文件（+282 / −69）：`src/cli.rs`、`src/console/page.rs`、`src/query/http.rs`、`src/query/mod.rs`、`tests/test_req123_live.rs` —— 与任务书范围一致。

### 10.2 全量红集合：基线与分支逐条同名（两套环境各测一遍）

| 环境 | 基线 | 分支 | 差异 |
| --- | --- | --- | --- |
| 无夹具（不设 `ONTOK_*`） | 104 target / 1798 例 / **29 target 红、183 例红** | 104 / 1800 / **29 target、183 例** | **只在基线红=0，只在分支红=0** |
| 有元数据库 + 源库 + schema + 租户 | 104 / 1798 / **4 target、5 例红** | 104 / 1800 / **4 target、5 例** | **只在基线红=0，只在分支红=0** |

有夹具环境下那 5 条既有红（与本次改动无因果，全是真实 schema/登记名漂移）：

```
drop_rate::real_schema_drop_rate_report
test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract
test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name
test_retained_names_req084::{test_req084_every_registered_name_is_still_literally_at_its_line,
                             test_req084_no_unregistered_real_name_remains_in_the_registered_files}
```

> ⚠️ **基线数字我自己实测**：无夹具 29 target / 183 例（与 REQ-139 那次 QA 记录的 183 条一致）；有 DB 夹具 4 target / 5 例。**Dev 自报的「6 target / 8 条」在我这两套环境里都复现不出来**（详见 §13 · N-1）——但「基线 ≡ 分支」这条判据在两个环境下都成立。

### 10.3 只翻默认，别的端点没动（字节对拍）

同一份声明、同一 token，基线 vs 分支：

| 端点 | 字节 | 判定 |
| --- | --- | --- |
| `GET /api/health` | 176 / 176 | ✅ 逐字节相同 |
| `GET /api/catalog?page=1&page_size=50` | 8015 / 8015 | ✅ |
| `GET /api/describe?name=:customers` | 14900 / 14900 | ✅ |
| `GET /api/ontology.ttl` | 293814 / 293814 | ✅ |
| `GET /api/ontology.ttl?view=model` | 371356 / 371356 | ✅ |

### 10.4 MCP 契约面（除默认外没有顺手改坏）

`initialize` / `tools/list` 逐叶子路径对比（基线 vs 分支），差异**只有 4 处**：

```
initialize.instructions                        （文案；R-3 已裁定接受）
tools[2].description                           （sparql_query 描述里的默认说明）
tools[2].inputSchema.properties.format.default  rows → jsonld
tools[2].inputSchema.properties.format.description
```

工具清单、其它 3 个工具的参数、`required`、`additionalProperties`、`protocolVersion`、`capabilities` 全部未变。

---

## 十一 · R-6 外部欠账清单：完整性核对（只列，不改）

Dev 列了 6 条。我逐个看原文，结论：**4 条是真欠账，1 条影响极轻，1 条无关；另外缺 1 条**。

| Dev 列的文件 | 原文实际状态 | 是否真欠账 |
| --- | --- | --- |
| `e2e-test/query/BRIEF.md:20` | 20 行示例 `POST /api/sparql` body 只写 `{"query":"SELECT ..."}`（无 format）⇒ 现在拿到**图**；而 28/30 行的评分口径要求 transcript 记 `rows`、必须转达回执里的 `warnings/coverage/truncated`（翻默认后在 `meta` 下） | ✅ **真欠账**（原文没有「默认 rows」字样，但示例与评分口径会让下轮 e2e Agent **拿图当表读**——Dev 的判断方向对，措辞略宽） |
| `e2e-test/agent-workspace/RULES.md:8` | 该行只是工具清单「`sparql_query` —— 执行 SPARQL 查询（租户由服务端绑定…）」，**没有**写默认 rows；11 行「回执里有 warnings 必须转达」在翻默认后仍成立（warnings 在 `meta.warnings`） | ⚠️ **影响极轻**（可改可不改；要改就只加一句「默认图，要表传 `format:"rows"`」） |
| `BA/runbook/ontology-e2e.md:292, 300, 326` | 8.1 REST 示例 body 无 format、8.2 MCP 示例 `arguments` 无 format、9.1 隔离验证两条命令同样无 format；而 8.3「回执里必须读的四样」教人读 `warnings/coverage/join_states/truncated`（翻默认后在 `meta` 下） | ✅ **真欠账** |
| `Deploy/skills/ontok-query/SKILL.md` | §五「普通模式的回执里**除了 `rows`**，还有四样东西」+ §四.1 的查询示例不带 format ⇒ 翻默认后**不再成立**（默认没有 `rows`） | ✅ **真欠账** |
| `Deploy/skills/ops360-ontology/SKILL.md` | 只讲这套部署是 `ontology-only`（SPARQL 恒 503）；第 10 行明写「怎么调接口、怎么读回执 —— 见 `ontok-query` skill，本文件不重复」 | ❌ **无关，不用改**（Dev 多列了这一条） |
| `Deploy/skills/README.md:92` | 工具参数表 `sparql_query` 只列 `query`、`max_rows`，**缺 `format`**，全篇没写默认 | ✅ **真欠账**（缺陷是「缺 format」，不是「写了默认 rows」） |

**Dev 清单缺的一条**：

| 漏掉的文件 | 为什么必须列 |
| --- | --- |
| `skills/ontok-query/SKILL.md`（仓库根，非 git worktree 的部署副本） | 与 `Deploy/skills/ontok-query/SKILL.md` **字节完全相同**（sha256 `4350D0C4902DF630E00B52818820C7E07DB49999DFA7A5942BDAAAFA8CC54B50`）。只改 Deploy 那份，agent 实际读的这份仍是错的 |

**核对到但确认不用改的**：`Deploy/README.md`、`Deploy/compose/README.md`（只列端点，不写回执形状）、`Deploy/OPS360-DEPLOY-LOG.md`（ontology-only 冒烟 503）、`e2e-test/query/score.js`（只数调用次数）、`e2e/`（那份运行目录里 0 处 sparql 字样）。

---

## 十二 · 临时产物与清理

全部在 **`D:\MyCodes\AI\an_onto_demo\.local\qa142\`**（根目录 `D:\` 下未建任何目录）：

| 内容 | 处置 |
| --- | --- |
| `bin/ontok-base.exe`（`11D65383…b23e9a`）、`bin/ontok-feat.exe`（`DA3670A8…37303b`） | **保留**（48 MB，两套被测二进制，复核可直接起） |
| `out/`（1.8 MB）：`manifest.json`（90 条抓取 = 3 查询 × 15 形态 × 2 套二进制）、`{base,feat}-tests.json`、`{base,feat}-env-tests.json`、`mut-M1-star-rows.diff`、全部原始回执字节、`console-index.html`、MCP 契约样本 | **保留**（证据包） |
| `logs/`（1.8 MB）：`base-full.log`、`feat-full.log`、`base-env-full.log`、`feat-env-full.log`、`mut-lib.log`、各服务 stdout/stderr（⚠️ TTL 等启动信息都在 stderr 里，已重定向到文件） | **保留** |
| `*.ps1`（7 个脚本：抓取 / 解析 / 对拍 / 变异 / 端点 / MCP 契约 / 控制台） | **保留**（可复跑） |
| `cargo-base/`、`cargo-feat/`、`cargo-mut/`（≈ 34 GB 构建缓存，`CARGO_TARGET_DIR` 全在 `.local` 下） | **已删除** |
| `mut-star/`（变异 worktree） | **已移除**（`git worktree remove --force`；patch 留在 `out/mut-M1-star-rows.diff`） |

收尾核对：`git worktree list` 回到原来 5 个（无残留变异 worktree）；`feature-REQ-142` 工作区 `git status --porcelain` **空**（未改 feature 分支、未 push）；无残留 `ontok` 进程；D 盘可用 126 GB。

**是否清理：是。** 证据（二进制/日志/回执/脚本）保留，构建缓存与变异 worktree 已清理。

---

## 十三 · 非阻断观察（不改代码，留给 BA / Dev 判断）

| # | 观察 | 建议 |
| --- | --- | --- |
| **N-1** | **Dev 自报的基线红数不可复现**：Dev 说「6 target / 8 条」，我实测无夹具 **29 target / 183 例**、有 DB 夹具 **4 target / 5 例**（两套环境里分支与基线**逐条同名**）。可能是 Dev 设了另一组夹具变量（如 `ONTOK_REQ122_TENANT_A/B`、`ONTOK_REQ126_TENANT_A`，这些我这边缺、对应用例走 SKIP） | 判据本身不受影响（红集合相等）；建议自报基线时**附环境变量清单**，否则数字无法复核 |
| **N-2** | **R2 的逐字节目前只有 QA 外部对拍，仓内没有 golden**：`ac004` 对拍的是「同一次构建里 REST vs MCP」，不是「`?format=rows` == 旧默认字节」。本次靠我 §4 的 sha 清单钉住 | rows 序列化路径本次零改动，可接受；若将来有人碰回执序列化，R2 没有护栏 —— 可考虑加一条 golden sha |
| **N-3** | **BA 自己的文档还留着被 R-1 推翻的优先级**：`BA/demands/REQ-142/demand.md:37-43` 仍写「1. `body.format`（最显式） 2. `?format=`」，`status.md` 也写「判定优先级：`body.format` > `?format=` > `Accept`」。R-1 已裁定按代码（`?format` > body），但这两处正文没跟着订正 | 合入时顺手把这两处改成 `?format=` > `body.format`，否则下一轮又会被当成分歧 |
| N-4（记录，非缺陷） | `Accept` 解析按 `;` 切分即取媒体类型，**忽略 `q` 值**（`application/json;q=0` 仍算「要表」）；`Accept: */*;q=0` 也仍落默认 | 与 REQ-140 同口径、不是本次引入，不在 AC 范围；记着即可 |

---

## 附 · 复现

```powershell
# 两套二进制（已留在证据包）
.local\qa142\bin\ontok-base.exe   # be483e7
.local\qa142\bin\ontok-feat.exe   # 972582e

# ① 起两个 query serve（基线 18801 / 分支 18800）+ 90 条抓取 + sha 对拍
pwsh -NoProfile -File .local\qa142\run-capture.ps1
pwsh -NoProfile -File .local\qa142\analyze.ps1  -Manifest .local\qa142\out\manifest.json
pwsh -NoProfile -File .local\qa142\analyze2.ps1

# ② 工作台页面实抓 + 页面 body 实跑
pwsh -NoProfile -File .local\qa142\probe-console.ps1

# ③ MCP 契约面 / 非 sparql 端点
pwsh -NoProfile -File .local\qa142\probe-mcp-contract.ps1
pwsh -NoProfile -File .local\qa142\probe-other-endpoints.ps1

# ④ 全量测试（两套环境）
$env:CARGO_TARGET_DIR='<任意 .local 下目录>'
cargo test --locked --no-fail-fast                       # 无夹具：183 红（两侧同名）
# 有夹具：另设 ONTOK_DATABASE_URL / ONTOK_TEST_DATABASE_URL / ONTOK_SOURCE_DATABASE_URL
#                / ONTOK_REQ123_SCHEMA / ONTOK_REQ123_TENANT（=7d491821-…-19ee5acd936d）

# ⑤ 变异 M1（`*/*` → rows）
git worktree add --detach .local\qa142\mut-star 972582e
git -C .local\qa142\mut-star apply .local\qa142\out\mut-M1-star-rows.diff
# 然后 cargo test --lib --no-fail-fast ⇒ 必须 2 红（见 §3）
```

> 注：本机 `PATH` 里的 `curl.exe` 是 msys64 版，在 pwsh 下会把 `-w '%{http_code}'` 吃成 `%http_code` ⇒ 脚本统一用 `C:\Windows\System32\curl.exe`。另：`query serve` 读的是 **`ONTOK_TEST_DATABASE_URL`**（不是 `ONTOK_DATABASE_URL`），`local-run/start.ps1` 只设后者，直接照抄 `start.ps1` 起服会报 `✗ 缺少环境变量 ONTOK_TEST_DATABASE_URL`（与本次改动无关，是既有的本机脚本口径问题）。
