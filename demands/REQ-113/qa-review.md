# QA 审核报告：REQ-113 · 排除表（产品能力）—— Pre-merge

> **被测**：worktree `feature-REQ-113` / 分支 `feature/REQ-113`，HEAD `4eb2357`（`f5c4e36` + `4eb2357`，基线 `develop` `e9755e8`，未 push）
> **审核日期**：2026-10-22 · Windows · rustc/cargo 1.96.0
> **源库**：`anspire_open_uat`（只读，96 表）· **本地库**：`anspire_real`（项目名全部新开：`req113_demo` / `qa_req113_badimport` / `qa_req113_singlefile3`）
> **隔离方式**：QA 自建 worktree `.review-workspace/req113/qa`（detached `4eb2357`）做全部实跑；基线 worktree `.review-workspace/req113/baseline`（detached `e9755e8`）只用于红灯复现。**未改 feature 分支、未 push**（`git status` 干净、`git ls-remote origin feature/REQ-113` 为空）。

---

## 1 · 结论

**PASS（带非阻断项）** —— 可以合并。

- **AC-001 两条判据在真实库上都成立**：`items` 段 0 命中；删掉 `inputs.exclusions` 后剩余文本 0 命中；反向确认审计段里 12/12 表名都在（判据不是恒真）。
- **闭合断言真的有牙**：QA 在隔离 worktree 里制造「未排除表在抽取中被静默丢掉」，实测 **exit=1、`stats.json` 不产出、报错点名 `customers`**，不是只信代码里那句 `bail!`。
- **侧车真删干净**：`proposal-exclusions.json` 在所有 `--out` 目录都不存在；`load_record` / `inputs_ref` / `RECORD_FILE` 全仓 0 命中；`propose import` 只读 `--file` 一个文件（在没有 `exclusions.json` 的空目录里导入成功），库内 `proposals.inputs.exclusions` 形状完整。
- **不退化**：lib **272** / doc **4** / 集成 73 套件（70 绿 + 3 红）**787 passed / 3 failed**；3 个红灯在基线 `e9755e8` 上**逐字复现**（同数字、同断言、同 panic 行）。禁改面 diff 为空；`rule_based.rs` 的 stats 字段断言仍是**逐字段全等**（11 → 16，没有放宽）。
- **非阻断项 5 条**（§11），均不改变「排除可执行、可审计、可评审」这条主线。

| AC | 判据 | 结果 |
| --- | --- | --- |
| AC-001 | ① `items` 段 0 命中；② 删审计段后 0 命中；实体数 = `tables_extracted` | ✅ ①0 ②0；84 = 84 |
| AC-002 | `seen − excluded == extracted` 是结构断言，不闭合 ⇒ 不写 stats | ✅ 96−12=84；**变异实测** exit=1、无 stats、点名表 |
| AC-003 | 空理由 / 非法类别 / 缺决策人 ⇒ 拒绝文件 | ✅ 三种各 exit=1 且**零产物**；12 条理由业务可读 |
| AC-004 | 完整审计段 + 单一文件 + sha 可追 | ✅ 单文件导入成功、库内形状完整；改一字节 sha 变，两次运行 sha 相同 |
| AC-005 | 陈旧默认警告 / `--strict-exclusions` 失败 | ✅ 默认 exit=0 + 点名 + `stale_exclusions=1`；strict exit=1 且无 proposal/stats |
| AC-006 | 审核界面只读区块，条数 = `tables_excluded` | ✅ `count=12`、`flagged_count=11`、`readonly=true`，两块分开，无写入口 |
| AC-007 | 排除与「无中文名」分开 | ✅ 交集 = ∅；被排除表 0 个进 entities；11 条 flagged 全部在 entities 里 |
| AC-008 | 同输入两次逐字节相同 | ✅ proposal `280ffe78…` / stats `9c568845…` 两次相同（`git diff --no-index` exit 0） |
| AC-009 | 不退化 | ✅ 见 §10；禁改面 0 diff，断言未放宽 |
| AC-010 | 真实库 96→12→84 + 理由可读 | ✅ 84 实体；12 条理由可读（AC 原文「13 条」见 N3） |

---

## 2 · P0 闭合断言的变异实测（本轮最关键）

### 2.1 怎么造的

在**隔离 worktree**（`.review-workspace/req113/qa`，detached `4eb2357`）里改 `src/extract/propose.rs` 的 `extract_with_context` 循环，模拟「作用域里有这张表、抽取路径把它静默吃掉」：

```rust
for t in &report.tables {
    // QA MUTATION：模拟静默丢表
    if t.name == "customers" { continue; }
    ...
}
```

`customers` **不在** `exclusions.json` 的 12 条里，所以它属于「应该产出实体」的作用域。schema（96 表）、names、exclusions 全部不动 —— 只有产出端少一张，正是 R5 要防的形态。

> 为什么不用题目里举例的「schema 改表名 / 从 names 删表」：
> - 改 schema 表名 ⇒ `tables_seen` 跟着变成 95，等式 **仍然闭合**（且 `customers` 若在排除里会变成 stale，`excluded` 也降为 11）—— 抓不到；
> - 从 `names` 删表 ⇒ 只影响展示名，实体照常产出（`display_name` 为空）—— 等式也闭合；
> - 只有「表在作用域里、却没产出实体」才会让实测值 83 ≠ 96−12，这是唯一忠实的造法。

### 2.2 实跑结果

命令与真实库基线一致，只把输出落到新目录 `out-mut/`：

```text
EXIT = 1
Error: 统计不自洽（AC-002）：tables_seen 96 − tables_excluded 12 = 84，但真的产出实体的只有 83 张表。

差 1 张 —— 有表**被静默吃掉了**：它既没被排除，也没被抽出来。
点名（1 张）：customers
...
所以这里**不写出 stats.json**，而不是写一份能看但错的统计。
```

| 期望 | 实测 |
| --- | --- |
| exit≠0 | ✅ `EXIT=1` |
| 不产出 `stats.json` | ✅ `out-mut/` 里**没有** `stats.json` |
| 不产出 `proposal.json` | ✅ 也没有（断言在两次 `fs::write` 之前） |
| 报错点名哪张表 | ✅ `点名（1 张）：customers` |
| 不是只信代码 | ✅ 先看到 `实体 : 83 个`（同屏打印），再看到断言失败 |

变异前同一份输入：`实体 84 = tables_extracted 84`、`闭合 = true`、exit 0。改回源码（`git checkout -- src/extract/propose.rs`）后重新编译，再次运行恢复正常 —— 断言不是恒真。

### 2.3 边界（非阻断 N4）

这个等式只能抓「**`tables_seen` 已定之后**丢表」。若表在勘查阶段就没了（schema 少一张），`seen` 与 `extracted` 会一起少，等式照样闭合。这是等式本身的边界，不是实现缺陷；已登记为已知限制。

---

## 3 · AC-001 两条判据（真实库 `anspire_open_uat`）

真跑命令：

```powershell
ontok-propose --schema ..\code\out\schema.json --names src\extract\table_names.json `
  --out out3\proposal.json --stats out3\stats.json --project req113_demo `
  --exclude exclusions.json --src $env:SRC_URL
# exit 0；96 − 12 = 84；实体 84
```

| 判据 | 方法 | 结果 |
| --- | --- | --- |
| ① `items` 段 grep 12 个被排除表名 | 解析 JSON，只序列化 `items` 数组后逐名计数 | **0 命中（12 张全 0）** |
| ② 删掉 `inputs.exclusions` 整段后 grep | (a) JSON 级删除后重新序列化；(b) 原文按花括号配平切掉该段（仍能 `json.loads`） | 两种都是 **0 命中** |
| 反向（判据不是恒真） | 对 `inputs.exclusions` 段逐名 grep | **12/12 全在**（`llm_provider_back` 等前缀关系不影响：12 张各自命中） |
| 实体数 = `tables_extracted` | 统计 `kind == add_entity_type` | **84** = `stats.tables_extracted` **84** |
| 被排除表出现在 entities key 里 | 取 `payload.key` 交集 | **∅** |

补充：不给 `--exclude` 时 `proposal.json` **没有 `inputs` 键**（`skip_serializing_if`），stats 记 `96 − 0 = 96`、实体 96 —— 「没有排除决定」与「排除了 0 张」在产物里确实长得不一样。

---

## 4 · 单一文件核对（AC-004 / BA 裁定「明细进提案、删侧车」）

### 4.1 侧车是否真删干净

| 检查 | 结果 |
| --- | --- |
| `--out` 目录里有无 `proposal-exclusions.json` | **不存在**（`out1/out2/out3/out-noex/out-mut/out-stale-default` 全部只有 proposal / stats / pending / rejected） |
| `git grep "RECORD_FILE\|load_record\|inputs_ref"` | **0 命中** |
| `git grep "proposal-exclusions"` | 只剩 1 处**历史注释**（`exclusions.rs:507` 解释为什么删掉侧车）+ `.feature/status.md` 的交付记录；**无代码/无产物** |

### 4.2 `propose import` 是否只读一个文件

- 代码：`propose_cmd` 只 `read_to_string(&file)` **一次**（`src/cli.rs:1339`），全函数无第二个路径、无 sha 比对分支（只把 sha 打印出来）。
- 实测：把 `proposal.json` 复制到一个**只有它一个文件**的空目录（无 `exclusions.json`、无侧车），在該目录执行 `ontok propose import --file proposal.json` ⇒ **exit 0**，136 条目导入成功。

### 4.3 库内形状

`proposals.inputs`（新项目 `qa_req113_singlefile3`）：

```text
inputs keys            = ['exclusions']
exclusions keys        = file, version, sha256, declared_by, reviewed_by,
                         rule, counts, excluded[], flagged[], stale[]
excluded / flagged / stale = 12 / 11 / 0
counts                 = {seen:96, excluded:12, extracted:84, stale:0, flagged:11}
sha256                 = 3f77c6b8fd41bded…（与 exclusions.json 文件哈希一致）
reviewed_by            = ba-michael（人工复核：推翻 1 条排除、接受其余 12 条）
```

数据库里**没有**第二张「排除明细」表（`information_schema` 查 `%exclusion%` = 0）。

### 4.4 sha 溯源

- `exclusions.json` 文件 sha256 = `3f77c6b8…`（与给定值一致）；
- 改一个字节（`onebyte.json`）⇒ 记录 sha 变 `9d784a41…`，且等于改后**原始字节**的哈希（不是重新规范化后的 JSON）；
- 同一文件两次运行 ⇒ 记录 sha 相同。

---

## 5 · 理由校验（AC-003）

三种坏文件各跑一次，全部 **exit=1 且输出目录零产物**（连 pending/rejected 都没有，因为校验发生在读 schema 之前）：

| 坏文件 | exit | 报错（节选） | 产物 |
| --- | --- | --- | --- |
| `bad-empty-reason.json`（`reason=""`） | **1** | `缺 exclude[].reason（或它是空白）…业务能读懂的排除理由（不是「技术原因」四个字）` | 无 |
| `bad-category.json`（`category="tech_reason"`） | **1** | `category = "tech_reason" 不在枚举内 … 新增类别必须先改 Category 并升 version` | 无 |
| `bad-no-decided-by.json`（删 `decided_by`） | **1** | `缺 exclude[].decided_by（或它是空白）…谁决定排除这张表（必须可追）` | 无 |

**理由可读性**：12 条全查，长度 23–33 字，无「技术原因 / 冗余 / 无用 / 其它 / TBD」等不可评审措辞；抽查示例：

- `_volc_dts_pg_ddl_18e4…`：数据同步工具自动生成的临时 DDL 表，不对应任何客户业务（`cdc_residue`, 0.97）
- `llm_provider_back`：llm_provider 的整表备份副本，列结构与主表完全一致（`backup_snapshot`, 0.95）
- `rpt_dim_customer`：报表客户维度表，由客户与渠道数据派生，客户业务里无此事物（`report_intermediate`, 0.82）

类别分布：cdc_residue 3 · backup_snapshot 3 · internal_pipeline 3 · report_intermediate 2 · migration_bookkeeping 1。

---

## 6 · 陈旧排除（AC-005）

在 `exclusions.json` 里加一条 `this_table_does_not_exist_qa`：

| 模式 | exit | 结果 |
| --- | --- | --- |
| 默认 | **0** | stderr 点名 `⚠ 陈旧排除 1 条 … this_table_does_not_exist_qa`；`stats.stale_exclusions = ["this_table_does_not_exist_qa"]`；`tables_excluded` **仍是 12**（陈旧不算排除）、`tables_extracted` 84；审计段 `counts.stale=1` |
| `--strict-exclusions` | **1** | `Error: 陈旧排除 1 条（--strict-exclusions ⇒ 失败）：this_table_does_not_exist_qa` + 两种可能；输出目录**没有 proposal/stats** |

**`--strict-exclusions` 单独给（无 `--exclude`）**：exit=1，报错说明「没有排除文件时不存在陈旧项，给这个开关只会让人以为发生过一次检查」。

> **QA 意见**：**语义合理，可以接受**。理由：该开关的含义是「陈旧就失败」，没有排除文件时它既不会失败也没有检查对象；静默成功会让人误以为「陈旧已经检查过」。这里选择显式拒绝（fail-loud）与本仓「声明了就要管」的一贯纪律一致，且测试已钉住。代价只是命令行组合更严格，用户加 `--exclude` 即可。BA 倾向接受 —— QA 同意。

---

## 7 · 确定性（AC-008）

同一 schema + 同一 exclusions 跑两次（`out1` / `out2`，独立连源库采样）：

```text
proposal.json run1 = 280ffe782111578680fa056c9302ff23b7340c29d216b466adfba25ad1e7e207
              run2 = 280ffe782111578680fa056c9302ff23b7340c29d216b466adfba25ad1e7e207
stats.json    run1 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
              run2 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
git diff --no-index out1/proposal.json out2/proposal.json  → exit 0
git diff --no-index out1/stats.json    out2/stats.json     → exit 0
```

交叉核对 Dev 报告：用 `--project req113_demo` 重跑，proposal sha = `852281ec94f7…`、stats = `9c568845…`，与 Dev 报告**逐位一致**（两次运行 project 名不同 ⇒ 文件字节不同是预期，因为 project 名写进了提案）。

---

## 8 · 审核界面只读区块（AC-006）

真服务、真数据：`ontok review serve --project req113_demo --proposal p1 --bind 127.0.0.1:18788`（数据来自库内 `proposals.inputs`）。

`GET /api/review` → HTTP 200：

```json
"exclusions": {
  "present": true, "readonly": true,
  "count": 12, "flagged_count": 11,
  "counts": {"seen":96,"excluded":12,"extracted":84,"stale":0,"flagged":11},
  "file": "exclusions.json", "version": "0.1.1",
  "sha256": "3f77c6b8…",
  "declared_by": "ai-table-judge / qwen36-27B",
  "reviewed_by": "ba-michael（人工复核：推翻 1 条排除、接受其余 12 条）",
  "tables": [ …12 条，每条含 table / category / category_label / reason / decided_by / confidence / evidence… ],
  "flagged": [ …11 条，与 tables 分开… ]
}
```

| 判据 | 结果 |
| --- | --- |
| 区块条数 = `tables_excluded` | ✅ `count = 12` = `tables.len()` = stats 12 |
| 含 confidence / decided_by / reviewed_by | ✅ 每条都有 `confidence` 与 `decided_by`；顶层 `reviewed_by` 与 `declared_by` **分开**（判的人 / 签的人） |
| 排除与待裁决**分开** | ✅ 两个顶层数组 `tables[]`（已排除）/ `flagged[]`（保留但存疑），交集 = ∅；页面两个独立 `<section id="excluded">` / `<section id="flagged">` |
| 只读、无输入框/按钮 | ✅ 两个 section 内**没有任何 `<input>/<button>`**；服务端只注册 `GET /api/review`、`GET /api/preview`、`POST /api/decide` 三个路由，`/api/decide` 不碰 exclusions；页面文案明写「改排除要改 exclusions.json 重跑」 |

`readonly: true`、`present/count/flagged_count` 都由服务端那份记录产出；界面**不解析** `exclusions.json`、也没有 schema ⇒「哪些表被排除」只有一处实现（纪律 H）。

---

## 9 · 与「无中文名」分开（AC-007）

| 检查 | 结果 |
| --- | --- |
| `tables_without_cn_name` ∩ `excluded_tables` | **∅**（62 ∩ 12 = 0） |
| 被排除表是否进 entities | 0 张 |
| `keep_but_flagged` 的 11 张 | **全部在 entities 里**（`internal_staff` / `dim_customer_binding` / `usage_reconciliation_log` …）——「待裁决」不等于「已排除」 |

实现上排除发生在抽取作用域之前，所以被排除的表根本不会进入「无中文名」的统计；这不是事后相减。

---

## 10 · 不退化 + 红灯归因（AC-009）

### 10.1 数字

| 项 | 基线（REQ-112） | 本分支 | 判定 |
| --- | --- | --- | --- |
| lib | 272（Dev 报告口径） | **272 passed / 0 failed** | ✅ |
| doc | 4 | **4 passed** | ✅ |
| 集成 | — | **73 套件：70 绿 + 3 红；787 passed / 3 failed** | ✅（3 红见 §10.2） |
| 合计（lib+doc+集成） | — | 272 + 787 + 4 = **1063** | 与 Dev 报告「1063」口径一致 |

> 说明：Windows 上 `src/bin/ontok-gate-preflight.rs` 是 Unix-only（`std::os::unix`），cargo 为集成测试构建全部 bin 时会编不过。按 Dev 同一绕行办法：临时挪开该文件 → 跑套件 → **`finally` 挪回**，实测
> ```
> SHA256(before) = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
> SHA256(after)  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
> SHA256(equal)  = True
> Test-Path      = True
> ```
> 绕行代价：`test_retained_names_req084` 用 `include_str!` 引这份源文件 ⇒ 该套件编不过，本轮**73 套件 = 74 个测试文件 − 它**，与 Dev 报告一致（非本轮引入）。

### 10.2 三个红灯：基线与本分支**逐字复现**（各自 `cargo clean -p ontok` 后重编，日志均含 `Compiling ontok v0.1.0 (…baseline)` / `(…qa)`）

环境：`ONTOK_SCHEMA_JSON = code\out\schema.json`（96 表那份，与 Dev 报告同一输入）。

| 用例 | 本分支 | 基线 `e9755e8` | 归因 |
| --- | --- | --- | --- |
| `drop_rate.rs::real_schema_drop_rate_report` | `表 96 · 抽出属性 1024 · 视图存活 309 · 丢弃 715 · 丢弃率 69.8%` → `超过上限 10.0%` | **完全相同** | 输入 schema 的翻译覆盖不足 ⇒ 属性被视图丢弃；与排除功能无关（只调 `extract()`/`project()`，本轮未改） |
| `test_fk_naming.rs::test_real_schema_customer_id_has_exactly_one_display_name` | `agent_white_list/customer_id 仍显示成「客户标识」` | **完全相同** | 同一份 schema 的注释/词表差异，与排除无关 |
| `test_agent_orchestration.rs::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | `left`（CLI items）少一条 `add_link_type`，`right` 有 | **完全相同**（同样少同一条） | REQ-108 分档路由带来的既有差异，REQ-112 已登记，非本轮引入 |

> 提示：不设 `ONTOK_SCHEMA_JSON` 时，前两个用例会先在「读不到输入」处 fail-closed（同样基线与本分支一致）。要复现 Dev 报告里的 69.8% / 「客户标识」，必须把 `ONTOK_SCHEMA_JSON` 指向 `code/out/schema.json`。

### 10.3 禁改面与断言

```text
git diff --stat develop -- src/server/http.rs src/mcp/tools.rs src/domain/property_filter.rs Cargo.toml Cargo.lock
（空）
```

- `guard()`（在 `src/mcp/tools.rs`）0 行；`property_filter.rs` G1–G4 0 行；`Cargo.*` 0 行（未加依赖，sha256 复用 `extract::sample::sha256_hex`）。
- 未删测试、未 `#[ignore]`、未放宽断言。
- `src/agent/rule_based.rs` 的 `test_extraction_stats_fields_are_untouched`：字段清单 **11 → 16**，仍用 `keys == expected`（双方 `sort_unstable` 后）**逐字段全等**，多一个少一个都红 —— **不是放宽**（该测试本轮实测 `ok`）。

---

## 11 · 阻断项 / 非阻断项

### 阻断项（0）

无。

### 非阻断项（5）

| # | 项 | 证据 | 建议 |
| --- | --- | --- | --- |
| N1 | **闭合断言失败时，`proposal-pending.json` / `proposal-rejected.json` 已经写出**（两个 `fs::write` 在 `stamp_counts` 之前） | 变异运行 `out-mut/` 里存在这两个文件，但没有 proposal/stats；exit=1、错误文案已明确「不写出 stats.json」 | 无害（主产物与统计都没写，失败很响），但若下游只读 pending/rejected 会有半份产物。可把这两个写盘也移到闭合断言之后，或失败时删除 |
| N2 | **`propose import` 的审计段校验发生在 `proposals.create()` 之后**：非法记录 ⇒ exit=1，但库里已留下「提案 + 136 条目、`inputs = NULL`」 | `qa_req113_badimport` 项目里实测存在 `status=pending`、136 items、`inputs NULL` 的孤儿提案 | 建议把 `Record::from_inputs` 校验提前到 `create` 之前（或包一个事务）。当前状态下界面会显示「没有输入记录」，不会伪造审计，故非阻断 |
| N3 | **AC-010 文本仍写「13 条理由 / 贴出完整 13 条」**，实际口径是 **12 条**（AI 判 13、BA 推翻 1） | `acceptance.md:66,67` vs 实际 12 条；`demand.md` 也残留 13 | AC 自身笔误，建议 BA 改文本（不影响实现） |
| N4 | **闭合等式抓不到「`tables_seen` 定下来之前就丢表」**（如 schema/勘查少一张表，`seen` 与 `extracted` 同减，等式仍闭合） | 代码路径 + §2.3 推理 | 这是等式固有限制；若将来要防，需要把「勘查表数」与「schema 文件条目数」再对一次（另一条断言），本轮不要求 |
| N5 | **Windows 绕行代价**：挪开 `ontok-gate-preflight.rs` 期间 `test_retained_names_req084` 编不过（73/74 套件） | §10.1 | 既有问题（REQ-112 已登记），本轮未恶化；建议后续给该 bin 加 `#![cfg(unix)]` 或把 `include_str!` 改成 `cfg` 条件 |

---

## 12 · 复现材料

- QA worktree：`.review-workspace/req113/qa`（detached `4eb2357`）
- 基线 worktree：`.review-workspace/req113/baseline`（detached `e9755e8`）
- 产物：`out1/` `out2/`（确定性）、`out3/`（真实库基准）、`out-noex/`（无排除）、`out-mut/`（闭合变异）、`out-stale-default/`（陈旧默认）、`fixtures-qa/`（坏文件）
- 日志：`suite-run.log` + `suite-*.log`（73 套件）、`clean-qa2-*.log` / `clean-baseline-*.log`（三红灯双侧）、`test-lib2.log`、`test-doc.log`
- 审核界面：`api-review.json`、`page.html`；库内项目 `req113_demo` / `qa_req113_singlefile3` / `qa_req113_badimport`

> feature 分支保持 `4eb2357` 不变、工作区干净、未 push。
