# QA 审核：REQ-139 · `/api/health` 报出 `project` + `ontology_version`

| 项 | 值 |
| --- | --- |
| 被测 | `feature-REQ-139` / `feature/REQ-139` / `112fd14` |
| 基线 | `develop` `93e6af2`（`code/` worktree，干净） |
| 审核时间 | 2026-10-24 18:41–19:05（Asia/Hong_Kong） |
| 审核方式 | 代码走读 + 两模式实起服务实测（基线/分支各一套二进制）+ 4 个变异 + 全量 `--lib` / `--tests` 双向对照 |
| 结论 | **PASS（零阻断 · 4 条非阻断）** |

---

## 0 · 结论

**PASS。7 条 AC 全过，无阻断项。**

一句话：`health` 与 `catalog` 的四个值来自**同一个字段**（不是两处各算），两模式实测同值；health 键集合 `removed=[]`；catalog / TTL（客户+建模）/ SPARQL / MCP **16/16 个产物 sha256 逐一相同**；两模式 health 不带 `Authorization` 都是 200；health 原文对租户 uuid / 连接串 / IP / 端口 / password / tenant / 96 张表名 **全 0 命中**；红线 8 文件 blob 与基线逐字节一致；lib 715→717（只多两条新测试，失败集合不变）；全量集成失败集合基线与分支**完全相同**（183 条，逐条同名）。

| AC | 判定 | 证据 |
| --- | --- | --- |
| AC-001 两字段都在（本体模式） | ✅ | `health` 原文：`"project":"ops360","ontology_version":2` |
| AC-002 普通模式也有（R3） | ✅ | `{"ontology_version":2,"project":"anspire_r2rml_excl",…}` |
| AC-003 与 catalog 一致（R4） | ✅ | 两模式四值全等 + 变异 M1 红（`left: 0 / right: 2`） |
| AC-004 现有字段一个不少（R5） | ✅ | 键集合 `added=[ontology_version,project] removed=[]`；`mode` 仍只在本体模式 |
| AC-005 不泄数据（R2） | ✅ | 9 类敏感模式 × 2 模式 × 2 构建 = 全 0 |
| AC-006 免 token | ✅ | 无 `Authorization` ⇒ 200（两模式） |
| AC-007 不退化 / 零依赖 | ✅（见 N1） | 红线/tools.rs/guard()/property_filter.rs/Cargo.* 零 diff；数据面 16/16 sha 相同；`/metrics` 序列集合不变 |

**非阻断 4 条**（详见 §7）：N1 `/metrics` 非逐字节（仅计时值抖动）；N2 普通模式 health 124→176 B（BA 已裁定，记录在案）；N3 两条测试重命名 + `demo` 从禁词表移出（应有变化，建议 PR 点名）；N4 两条 DB 环境测试基线与分支同红。

---

## 1 · R4「同源」核验（最重要）

### 1.1 代码：health 与 catalog 读的是**同一对字段**，health 不二次计算

| 位置 | 事实 |
| --- | --- |
| `src/query/serve.rs:519-537` | `ServiceParts { project: args.project.clone(), version: args.version }` —— **唯一写入点**（启动时一次） |
| `src/query/mod.rs:613-614` | `QueryService { project: p.project, version: p.version }` —— 运行期只读，无 setter |
| `src/query/http.rs:265-266` | health：`"project": svc.project, "ontology_version": svc.version` |
| `src/query/catalog.rs:516-517` | `CatalogPage { project: svc.project.clone(), version: svc.version }` |

`health` 函数体（`src/query/http.rs:253-273`）**只做三件事**：`audit_anonymous` 写一行审计、用常量拼 `service/version/syntax/protocols/transports`、把 `svc.project` / `svc.version` 塞进 JSON。它**不查元数据库、不从 IRI/TTL 反解项目名、没有第二份 `--project/--version` 解析**。⇒ R4 要求的「同一个数，不能两处各算」在结构上成立。

旁证：同一个 `args.version` 也用于生成 TTL 的 `ontok:version`（`serve.rs:415`、`:460`、`:472`），所以 TTL 头部是第三条独立见证。

### 1.2 实测：两模式 · 四值 · 第三源三角验证

实起服务（基线二进制 `93e6af2` 与分支二进制 `112fd14` 各起一套；元数据库 `…/anspire_real`，源库 `…/anspire_open_uat`）：

| 模式 | health.project | catalog.project | 相等 | health.ontology_version | catalog.version | 相等 | TTL 头部 `ontok:sourceProject` / `ontok:version` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `--ontology-only`（ops360） | `ops360` | `ops360` | ✅ | `2` | `2` | ✅ | `ops360` / `2` |
| 普通（anspire） | `anspire_r2rml_excl` | `anspire_r2rml_excl` | ✅ | `2` | `2` | ✅ | `anspire_r2rml_excl` / `2` |

原文（分支二进制，实抓）：

```json
{"mode":"ontology-only","ontology_version":2,"project":"ops360","protocols":["rest","mcp-http"],"service":"ontok-query","syntax":"sparql-v1","transports":["http","sse"],"version":"0.1.0"}
{"ontology_version":2,"project":"anspire_r2rml_excl","protocols":["rest","mcp-http"],"service":"ontok-query","syntax":"sparql-v1","transports":["http","sse"],"version":"0.1.0"}
```

`ontology_version` 是 **JSON 数字** `2`，不是字符串 `"2"`（类型正确）。

### 1.3 变异：`ontology_version` 恒 `0` ⇒ 必须红（红行）

变异（独立 worktree，detached `112fd14`，只改一行 `"ontology_version": svc.version` → `0`）：

```
thread 'query::http::tests::health_identity_equals_catalog_in_both_modes' panicked at src\query\http.rs:1148:13:
assertion `left == right` failed: query：health 的 ontology_version 必须与 catalog 的 version 同值
health={"ontology_version":0,"project":"demo",...}
catalog={"counts":{...,"version":2}...}
  left: Number(0)
 right: Number(2)

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 716 filtered out
```

**这不是空转**：`catalog` 侧仍是 `2`，只有 health 侧变 `0` —— 正是「两处数字漂移」这个失败模式；测试的相等断言当场抓住。同一测试在未变异分支上绿（`3 passed`，见 §6.2）。

**加强变异（我自己加的）**：把 `project` 硬写成 `"wrong_project"`（模拟「两处各算」的另一种漂移）⇒ 同一测试红，`left: String("wrong_project") / right: "demo"`，且 R2 夹具测试也红。⇒ 两个字段都被钉住，不是只钉了版本号。

---

## 2 · 零泄漏（R2）

两模式 health 原文全文扫描（分支 + 基线各一份；96 张表名取自 `code/out/schema.json`）：

| 模式 | 租户 uuid | 任意 uuid | `postgres` | `://` | IPv4 | 端口字面量 | password/pwd | `tenant`(不分大小写) | 92 张表名 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 本体模式 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 普通模式 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

判据原文（脚本逐条计数，见 `.local/qa-req139/logs/leak-scan.log`）：

```
===== feat\norm.health.bin (176 B) =====
  ok   tenant_uuid_7d491821   count=0
  ok   uuid_any               count=0
  ok   postgres              count=0
  ok   conn_scheme           count=0
  ok   ipv4                  count=0
  ok   port_literal          count=0
  ok   password              count=0
  ok   tenant                count=0
  ok   table_name            count=0
```

health 里出现的**唯一**业务相关字符串是 `project`（`ops360` / `anspire_r2rml_excl`）—— 它本来就出现在 TTL（`ontok:sourceProject`）与 `/api/catalog` 里，符合 BA 裁定「项目名可以给」。`version:"0.1.0"` 是产品版本（`CARGO_PKG_VERSION`），不是数据。

---

## 3 · 只增不减（R5）

### 3.1 键集合（基线二进制 vs 分支二进制，实抓）

| 模式 | 基线键集合 | 分支键集合 | added | removed |
| --- | --- | --- | --- | --- |
| 本体模式 | `mode, protocols, service, syntax, transports, version` | `mode, ontology_version, project, protocols, service, syntax, transports, version` | `[ontology_version, project]` | **`[]`** |
| 普通模式 | `protocols, service, syntax, transports, version` | `ontology_version, project, protocols, service, syntax, transports, version` | `[ontology_version, project]` | **`[]`** |

- `mode` 仍**只在**本体模式出现：普通模式 health 全文 grep `"mode"` ⇒ 0 命中（`health_keeps_all_baseline_keys_and_mode_stays_ontology_only` 也断言了这一点）。
- 旧字段**值**未变：`service:"ontok-query"`、`version:"0.1.0"`、`syntax:"sparql-v1"`、`protocols:["rest","mcp-http"]`、`transports:["http","sse"]`，两模式逐一比对相同。
- 字节：普通 124→176 B、本体 147→187 B（只多了两个字段，无格式变化；`json!` 输出按 key 字典序，两模式顺序一致）。

### 3.2 判据有牙（我自己加的变异）

| 变异 | 结果 |
| --- | --- |
| M3：删掉旧字段 `transports` | `health_keeps_all_baseline_keys_and_mode_stays_ontology_only` **红**：`health 的键集合变了`，`left` 里少了 `transports` |
| M4：`mode` 无条件写进两种模式 | 同一测试 **红**：普通模式 health 出现 `"mode":"ontology-only"`（`left` 多出 `mode`） |

⇒ 「只增不减」和「mode 只在本体模式」都有**可失败的判据**，不是空断言。

---

## 4 · 数据面零回归（sha256 清单）

两套二进制、同样参数、同样请求顺序（health → catalog p1/p2 → TTL 客户/建模 → SPARQL ×2 → MCP list/catalog），**逐字节**比对：

| 模式 | 产物 | 字节 | sha256（基线 = 分支） | 相同 |
| --- | --- | --- | --- | --- |
| 本体 | catalog p1 | 13,294 | `1eaffc41b0d663b7f7eb648765cb304fe1d3a07bd940a563b6723ef0da1c6391` | ✅ |
| 本体 | catalog p2 | 214 | `a43090c555ef95a6bf569e872d7ead6044dc8d219aaf68d848cd9ea9b5e7bbbb` | ✅ |
| 本体 | TTL 客户视图 | 21,335 | `ab0530cf5aea7b665b6ed0c668b56fb1a035507e5e43f3b883b384367b4669d5` | ✅ |
| 本体 | TTL 建模视图 | 29,687 | `8058c87d8eabd2d9ab87409cbf12d727c90fb044f2803782df27ac69dc3a4c4b` | ✅ |
| 本体 | SPARQL（ORDER BY） | 734 | `f99816310ff2cbb38cf438c243412fa0b09e7bd38c0ceb6581e043848f1ca1c5` | ✅ |
| 本体 | SPARQL（Dev 原查询） | 734 | `f99816310ff2cbb38cf438c243412fa0b09e7bd38c0ceb6581e043848f1ca1c5` | ✅ |
| 本体 | MCP tools/list | 2,719 | `93abef894a4167e73b28630880685636c017a6c88dcf493130e834235c25eb00` | ✅ |
| 本体 | MCP catalog | 28,648 | `b379a74c427a20b85e710fbd7df4db2f1ebdbd95ff86f93aeb0591348d13d749` | ✅ |
| 普通 | catalog p1 | 31,671 | `f2cd2fb8e847a004d60df507fb6217d0b1c84466345e7b99ffafc79bee4bf739` | ✅ |
| 普通 | catalog p2 | 31,980 | `d2ee13ba3e14b149fe212968b1c1df328c0497f5b62352878d546865442e9e7b` | ✅ |
| 普通 | TTL 客户视图 | 293,814 | `a420bbcefb04074618710ffa8b760de9ed415ce7ed58e7e479c0e35c899cc070` | ✅ |
| 普通 | TTL 建模视图 | 371,356 | `2382cd9aaf22763f67a88fbc2281e8ba6684416c9e0e022e8e05f88ae27fd0d0` | ✅ |
| 普通 | SPARQL（ORDER BY） | 1,456 | `5a663283e120a8b122d507bdb5149aad3f30fbf9a2abb1a80330e98ab7c82174` | ✅ |
| 普通 | SPARQL（Dev 原查询） | 1,437 | `005e4940a7258d19174002e91063d6e3928383e8d67c95cf919aa4aa0abc0046` | ✅ |
| 普通 | MCP tools/list | 2,719 | `93abef894a4167e73b28630880685636c017a6c88dcf493130e834235c25eb00` | ✅ |
| 普通 | MCP catalog | 67,940 | `c4a54471dbf3f773b013f53b7b67f61b615ac6453d3d5c59a4c43c15bb2d20a7` | ✅ |

**16/16 相同**（含 REQ-136 裁定点名的 catalog 31671 B、TTL 293814/371356 B、SPARQL 5 行）。本体模式 SPARQL 两侧都是 **503 + 同字节**（`NO_DATA_SOURCE`，REQ-136 的设计行为）。

`/metrics`：**序列集合完全一致**（两模式各 109 条 series、12 行 HELP/TYPE，`only-base=0 / only-feat=0`）；字节差异只落在 `ontok_request_duration_seconds_sum` 与一个直方图桶的计时值上（同一请求在不同进程的耗时不同）。⇒ **指标面零新增/零删除**，N1 仅记录字节层面不可比。

---

## 5 · 免 token

两模式 `GET /api/health` **不带任何 `Authorization` 头**：

| 模式 | HTTP | 结果 |
| --- | --- | --- |
| 本体模式 | **200** | 含 `project` / `ontology_version` / `mode` |
| 普通模式 | **200** | 含 `project` / `ontology_version`，无 `mode` |

（其余数据端点在同一轮请求里用 token 访问，状态码见 `.local/qa-req139/evidence/*/*.status.txt`。）

---

## 6 · 不退化

### 6.1 改动范围

`git diff --stat develop...HEAD`：**只 2 个文件**，`src/cli.rs` + `src/query/http.rs`，**+142 / −14**。无新依赖、无 `tests/` 改动、无删除测试文件。

### 6.2 单测（`cargo test --lib`，两侧同机同法）

| 侧 | 总数 | passed | failed | 失败项 |
| --- | --- | --- | --- | --- |
| 基线 `93e6af2` | 715 | 713 | 2 | `db::apply::tests::write_snapshot_in_maps_unique_conflict_to_db_not_key_conflict`、`extract::introspect::tests::read_only_pool_really_reads_back_on` |
| 分支 `112fd14` | **717** | **715** | **2** | **同上两条（完全同名）**，原因同为 `MissingEnv("ONTOK_TEST_DATABASE_URL")` |

⇒ +2 = 本次新增的两条测试；**零新增失败**。指定的三条 health 测试在分支上全绿：

```
test query::http::tests::health_needs_no_token_and_carries_no_business_data ... ok
test query::http::tests::health_keeps_all_baseline_keys_and_mode_stays_ontology_only ... ok
test query::http::tests::health_identity_equals_catalog_in_both_modes ... ok
test result: ok. 3 passed; 0 failed
```

### 6.3 全量集成（`cargo test --tests --no-fail-fast`，两侧同一环境）

| 侧 | 有失败的二进制 | 失败用例 | passed | failed | ignored |
| --- | --- | --- | --- | --- | --- |
| 基线 | 29 | 183 | 1,592 | 183 | 2 |
| 分支 | 29 | 183 | 1,594 | 183 | 2 |

逐条用例名 diff：**only-in-feat = 0，only-in-base = 0 ⇒ 失败集合完全相同**（`IDENTICAL FAILURE SET: True`）。

> 口径：`--tests` 把 lib 单测目标也一起跑了，所以 183 = **181 条集成环境依赖失败 + 2 条 lib 环境失败**，与 Dev 报的「181 条环境依赖」一致；`passed` 多 2 正是新增的两条 health 测试。

### 6.4 红线 / 禁改面

`git diff --numstat develop...HEAD` 对这些路径**全空**，且 blob hash 与基线**逐一相同**：

| 文件 | 判定 | blob |
| --- | --- | --- |
| `tests/drop_rate.rs` | SAME | `f3967ecd8215…` |
| `tests/test_fk_naming.rs` | SAME | `6c0ae5620820…` |
| `src/domain/view.rs` | SAME | `9d085a035718…` |
| `src/domain/review_view.rs` | SAME | `fdd7fcd904c6…` |
| `src/mcp/tools.rs`（含 `guard()`，`tools.rs:693`） | SAME | `8f514039946c…` |
| `src/domain/property_filter.rs` | SAME | `d4e93e337ca0…` |
| `Cargo.toml` | SAME | `3c268177ac5a…` |
| `Cargo.lock` | SAME | `3edd02f57d2f…` |

无新增 `#[ignore]` / `#[should_panic]`。两条旧测试是「重命名 + 扩展」，不是删除（见 N3）。

### 6.5 文档（需求交付物 1）

`query serve --help` 已明确区分两者（实跑分支二进制）：

```
GET  /api/health         免 token：产品版本 + 项目名 + **本体**版本 + 语法版本（两种模式都有）
| `version` | **产品**版本 = 这个 `ontok` 二进制的 `CARGO_PKG_VERSION`（升级软件才变） | `"0.1.0"` |
| `ontology_version` | **本体**版本 = `--version` 指向的、已冻结的快照版本号（整数，`ontok version freeze` 才变） | `2` |
⛔ health 仍然**不给**租户 id / 连接串 / 表名 / 口令
```

---

## 7 · 阻断 / 非阻断

### 阻断

**无。**

### 非阻断

| # | 内容 | 影响 | 建议 |
| --- | --- | --- | --- |
| **N1** | `/metrics` 字节不逐字节相同（两模式都是）：差异**仅**在 `ontok_request_duration_seconds_sum` 与一个直方图桶计数（计时抖动）；109 条 series 的**名称+标签集合**两侧完全一致 | 无行为差异；AC-007 里「`/metrics` 字节不变」这句按字面不可满足（任何两次运行都不同） | 建议把该句口径改为「指标序列集合不变」，或明确 `/metrics` 是观测端点、字节不设判据 |
| **N2** | 普通模式 health 124 → 176 B（本体 147→187 B） | BA 已裁定：health 是**身份端点**，只增不减即可（§三 裁定） | 无需动作；仅登记，避免后人误判 REQ-136 AC-006 |
| **N3** | 两条旧测试被**重命名并扩展**：`health_needs_no_token_and_carries_no_data` → `…_business_data`；`…_stays_byte_identical` → `…_has_no_mode_key`。同时禁词表里移除了 `"demo"`（改为正面断言 `v["project"]=="demo"`） | 判据**整体变强**（新增键集合、两模式、同值三重断言），旧的 `mode` 断言原样保留；但「移出禁词」是一次**有意放宽**，容易被误读成「删断言」 | PR 描述里点名这两处；说明 `demo` 移出是因为 `project` 现在**应当**出现 |
| **N4** | 2 条 DB 环境测试（`ONTOK_TEST_DATABASE_URL` 缺失）基线与分支同红 | 环境依赖，非本次引入 | 与 REQ-139 无关，不阻塞 |

> 另记（不是问题）：变异 M2（`project` 漂移）会**同时**打红 `health_needs_no_token_and_carries_no_business_data`（它对 `project="demo"` 有正面断言）。这只是判据交叉覆盖，说明 R2 测试夹具与 R4 断言共用同一份 `test_service()`，无副作用。

---

## 8 · 临时产物与清理

全部临时产物都在 `D:\MyCodes\AI\an_onto_demo\.local\` 下，未在 `D:\` 根或用户目录建任何目录。

| 路径 | 内容 | 状态 |
| --- | --- | --- |
| `.local/qa-req139/`（49 MB，116 文件） | **证据包（保留）**：`evidence/{base,feat}/{ont,norm}.{health,catalog-p1,p2,ttl-customer,ttl-model,sparql-*,mcp-*,metrics}`、`evidence/sha256-manifest.json`、`logs/`（构建/起服/抓取/变异/测试日志 + `integ-diff.json`）、`scripts/`（`run-capture.ps1` / `compare.ps1` / `leak-scan.ps1` / `parse-tests.py` / `run-mutations-2-4.ps1`）、`tokens/`、`bin/`（两套被测二进制） | **保留** |
| `.local/cargo-req139/`（15.7 GB） | 基线构建 `CARGO_TARGET_DIR` | **已删除** |
| `.local/cargo-req139-feat/`（15.7 GB） | 分支构建 `CARGO_TARGET_DIR` | **已删除** |
| `.local/cargo-mut-139-1/`（2.2 GB） | 变异构建 `CARGO_TARGET_DIR` | **已删除** |
| `.local/wt-mut-139-1/` | 变异 worktree（detached `112fd14`），跑完已 `git checkout` 复原 | **已删除**（`git worktree remove` + `prune`） |

**纪律确认**：feature 分支未改（`git status` 干净）、未 commit、未 push；`git worktree list` 与开始时一致（5 条：workspace / BA / code / Deploy / feature-REQ-139）；所有被测服务进程已停止。

---

## 9 · 局限与自查

- 需求「不做」的多套部署目录服务未验（明确不在范围）。
- `ONTOK_TEST_DATABASE_URL` 本会话不可得 ⇒ 2 条 DB 测试只能做「基线与分支同红」对比，无法证明它们在**有测试库**时也全绿（与 REQ-139 改动无关：两条测试都失败在缺环境变量的早退路径上）。
- 我对「同源」的证明是**代码结构 + 运行期同值 + 变异**三重，不是只靠「看起来一样」。
- 我的计数器口径：`--tests` 含 lib 目标，故总数 183；Dev 报 181 指集成部分，两者相加口径一致（181 + 2）。
