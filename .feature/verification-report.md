# REQ-113 · 排除表：从「脚本预处理」变成产品能力 —— 验证报告

> **被测**：worktree `feature-REQ-113` / 分支 `feature/REQ-113` · 基线 `develop` `e9755e8` · **未 push**
> **日期**：2026-10-22 · Windows · rustc/cargo 1.96.0
> **源库**：`anspire_open_uat`（只读，96 表）· **本地库**：`anspire_real`（新项目 `req113_demo`）
> **清单来源**：`exclusions.json` = BA 的 `code/out/exclusions-ai.json`（`version 0.1.1`，AI 判 13 条 → BA 复核后 12 条）
>
> ⚠️ REQ-112 的验证报告见 `git show 776f956:.feature/verification-report.md`（本文件本轮覆盖为 REQ-113）。

---

## 0 · 结论先行

| AC | 判据 | 结果 |
| --- | --- | --- |
| AC-001 | 被排除的表不产生实体/属性/关系；提案里 grep 表名 = 0；实体数 = `tables_extracted` | ✅ 12 个表名在 `proposal.json` 里出现 **0** 次；实体 **84** = `tables_extracted` |
| AC-002 | `seen − excluded == extracted`（结构性断言） | ✅ 96 − 12 = 84；断言在 `exclusions.rs:887`，不闭合 ⇒ **不写 stats**、exit≠0 |
| AC-003 | 每条有理由/类别/决策人；空理由/非法类别/缺决策人 ⇒ 拒绝文件 | ✅ 三种坏文件各跑一次：exit≠0 且**不产出**任何文件 |
| AC-004 | `inputs.exclusions = {file, version, sha256}`；改一字节 sha 变 | ✅ 逐项实测；改一个字符 ⇒ sha256 变 |
| AC-005 | 陈旧排除：默认警告 / `--strict-exclusions` exit≠0 | ✅ 两种模式各跑一次（真实库 + 1 条不存在的表） |
| AC-006 | 审核界面只读区块，条数 = `tables_excluded` | ✅ `GET /api/review` → `count=12`、`readonly=true`（真服务、真数据） |
| AC-007 | 排除与「无中文名」分开记，排除优先 | ✅ `excluded_tables ∩ tables_without_cn_name = ∅` |
| AC-008 | 两次运行提案与 stats 逐字节相同 | ✅ 三个文件 sha256 相同，`git diff --no-index` 无输出 |
| AC-009 | 不退化（红线 0 diff / 不删测试 / 不加依赖） | ✅ 见 §5 |
| AC-010 | 真实库 96 → 排除 12 → 84，13/12 条理由可读 | ✅ 见 §1.3 |

---

## 1 · 真实库实测（AC-001 / 002 / 007 / 010）

### 1.1 命令

```powershell
$env:SRC_URL = "postgresql://***@172.16.120.20:30432/anspire_open_uat"   # 只读
.\target\debug\ontok-propose.exe --schema ..\code\out\schema.json `
    --names src\extract\table_names.json `
    --out out\proposal.json --stats out\stats.json `
    --project req113_demo --exclude exclusions.json --src $env:SRC_URL
```

### 1.2 三个计数（`stats.json`）

```json
"tables_seen": 96, "tables_excluded": 12, "tables_extracted": 84,
"excluded_tables": [ …12 条，逐条 table/category/reason/decided_by/confidence/evidence… ],
"stale_exclusions": [],
"flagged_tables": [ …11 条 keep_but_flagged（**未被排除**）… ],
"tables_without_cn_name": [ …62 张… ]
```

| 检查 | 值 |
| --- | --- |
| `seen − excluded` | 96 − 12 = **84** = `tables_extracted` = 提案里 `add_entity_type` 条数 |
| `proposal.json` 里 grep 12 个被排除表名 | **0 次** |
| `excluded_tables` 与 `tables_without_cn_name` 交集 | **0**（被排除的表根本没进抽取作用域） |

### 1.3 12 条排除（AI 判定 + BA 复核；此处为 `stats.json` 原文摘要）

| 表 | 类别 | 理由 | 决策人 | 置信度 |
| --- | --- | --- | --- | --- |
| `_volc_dts_pg_ddl_18e4b12d…` | cdc_residue | 数据同步工具自动生成的临时 DDL 表，不对应任何客户业务 | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_8f7cc0a6…` | cdc_residue | 同上 | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_9a38a0a4…` | cdc_residue | 同上 | ai-table-judge | 0.97 |
| `alembic_version` | migration_bookkeeping | 数据库迁移工具的版本号记账表，客户业务里没有对应事物 | ai-table-judge | 0.99 |
| `llm_provider_back` | backup_snapshot | llm_provider 的整表备份副本，列结构与主表完全一致 | ai-table-judge | 0.95 |
| `llm_provider_back_20260630` | backup_snapshot | 按日期命名的 llm_provider 备份副本，不是独立业务事物 | ai-table-judge | 0.96 |
| `llm_provider_back_20260701` | backup_snapshot | 同上 | ai-table-judge | 0.96 |
| `llm_auth_cache_change_outbox` | internal_pipeline | 权限缓存变更的内部发件箱，只给同步程序读写，客户不可见 | ai-table-judge | 0.94 |
| `llm_auth_cache_kafka_event_outbox` | internal_pipeline | 内部消息队列发件箱，记录待投递事件而非业务事实 | ai-table-judge | 0.94 |
| `llm_auth_cache_trigger_state` | internal_pipeline | 权限边界判断用的内部状态快照，本身不是业务对象 | ai-table-judge | 0.90 |
| `rpt_dim_customer` | report_intermediate | 报表客户维度表，由客户与渠道数据派生，客户业务里无此事物 | ai-table-judge | 0.82 |
| `rpt_dws_channel` | report_intermediate | 渠道汇总的报表中间结果，不是某一笔业务的发生记录 | ai-table-judge | 0.82 |

> `dim_customer_binding` 被 BA 从排除里**推翻**（有 `cross_currency_enabled` 与渠道归属，是业务状态），
> 现列在 `keep_but_flagged`，**照常进本体**（84 张里有它）。

---

## 2 · 测试与绕行（AC-009）

### 2.1 结果

| 项 | 基线（REQ-112 交付） | 本轮 |
| --- | --- | --- |
| lib | 245 | **272**（+27：`extract::exclusions` 的 27 条单元测试） |
| doc | 4 | **4** |
| 集成套件 | 43 套件 422 | **73 套件**（另有 1 个套件绕行，见 §2.2）；全部用例 **1063 通过** |
| 新增套件 | — | `tests/test_req113_exclusions.rs`（12 条，全绿） |
| 未通过 | 2 处既有红灯 | **3 处，且三处都在基线上逐字复现**（见 §2.3） |

### 2.2 Windows 绕行（`ontok-gate-preflight.rs` 是 Unix-only）

```
sha256(before) = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
sha256(after)  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
sha256(equal)  = True
Test-Path      = True          # finally 里挪回，字节级相同
```

绕行的代价：`tests/test_retained_names_req084.rs` 用 `include_str!` 引那份源文件 ⇒ **该套件编不过**
（REQ-112 已登记的既有红灯，不是本轮引入）。`git status --short` 里**没有 `D` 行**。

### 2.3 三处红灯：**在基线上逐字复现**（用 `git archive e9755e8` 导出基线到临时目录，同样绕行后跑）

| 用例 | 基线 | 本分支 | 归因 |
| --- | --- | --- | --- |
| `drop_rate.rs::real_schema_drop_rate_report` | ❌ 69.8% > 上限 10.0%（表 96 / 属性 1024 / 存活 309） | ❌ 同样 69.8% | **输入 schema**：本机只有 96 张真实表名的 schema，翻译表覆盖 0 ⇒ 属性被丢弃；与 REQ-113 无关（只调 `extract()`/`project()`，本轮未改） |
| `test_fk_naming.rs::test_real_schema_customer_id_has_exactly_one_display_name` | ❌ `agent_white_list/customer_id 仍显示成「客户标识」` | ❌ 同一句 | 同一份 schema 的注释差异，与排除功能无关 |
| `test_agent_orchestration.rs::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | ❌ `items` 与库路径不一致 | ❌ 同一处 | REQ-108 分档路由带来的既有差异（REQ-112 已登记为「CLI 端到端」红灯） |

> 另外两条在本轮全量运行里红过、**已修**：`test_anchor_guard`（我的测试用了裸 `env!("CARGO_MANIFEST_DIR")` ⇒ 改走 `common::crate_root()`）、
> `test_req107_source_enumeration`（新模块 / 新 `println!` 未登记 ⇒ 登记 `extract::exclusions` 与那条回执）。

---

## 3 · 确定性（AC-008）

```
proposal.json  run1 = 91089bd8bce2853debc77437eb3ef5a005254b5de7ab838a2596d482b463223c
               run2 = 91089bd8bce2853debc77437eb3ef5a005254b5de7ab838a2596d482b463223c
stats.json     run1 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
               run2 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
git diff --no-index out\proposal.json out2\proposal.json  → exit 0
git diff --no-index out\stats.json    out2\stats.json     → exit 0
```

（两次运行各自独立连源库采样；`sample` 的 digest 是确定性 sha256，且本模块内无 HashMap 迭代、无时间戳。）

---

## 4 · 陈旧排除（AC-005，真实库）

往 `exclusions.json` 里加一条不存在的表 ⇒ `exclusions-stale.json`：

| 模式 | 退出码 | 输出 |
| --- | --- | --- |
| 默认 | **0** | stderr：`⚠ 陈旧排除 1 条（… 但这份 schema 里没有）：_volc_dts_pg_ddl_0000…`；`stats.stale_exclusions` 记 1 条；`tables_excluded` 仍是 12（陈旧的**不算**排除） |
| `--strict-exclusions` | **1** | `Error: 陈旧排除 1 条（--strict-exclusions ⇒ 失败）：_volc…` + 两种可能；**不产出** proposal / stats |

---

## 5 · 红线 / 禁改（AC-009）

```
git diff --stat develop -- src/server/http.rs src/mcp/tools.rs src/domain/property_filter.rs Cargo.toml Cargo.lock
（空）
```

- `guard()` 实现、`property_filter.rs` G1–G4：**0 行**；
- `Cargo.toml` / `Cargo.lock`：**0 行**（sha256 复用同仓 `extract::sample::sha256_hex`，**未加依赖**）；
- 未删测试、未 `#[ignore]`、未放宽断言；唯一的既有断言变更是
  `src/agent/rule_based.rs` 的 `ExtractionStats` **字段集清单**，从 11 个加到 16 个（AC-002 / 003 / 005 要求的那五个），
  断言仍是**逐字段全等**（不是放宽）。

---

## 6 · 审核界面（AC-006）

真服务真数据（`ontok review serve` + `GET /api/review`，Bearer 口令）：

```json
"exclusions": {
  "present": true, "readonly": true,
  "count": 12, "flagged_count": 11, "stale": [],
  "file": "exclusions.json", "version": "0.1.1",
  "sha256": "3f77c6b8fd41bded7e5fbe74b507c74a2a385cfc67cccef2b90cada5dacc2c08",
  "declared_by": "ai-table-judge / qwen36-27B",
  "reviewed_by": "ba-michael（人工复核：推翻 1 条排除、接受其余 12 条）",
  "rule": "只排除「不是客户业务世界里的事物」的表：…",
  "tables": [
    {"table":"_volc_dts_pg_ddl_18e4…","category":"cdc_residue","category_label":"CDC / 数据同步残留",
     "reason":"数据同步工具自动生成的临时 DDL 表，不对应任何客户业务",
     "decided_by":"ai-table-judge","confidence":0.97,
     "evidence":"表名为 _volc_dts_pg_ddl_ + 32 位哈希（火山 DTS 同步产物命名）；…"}
  ],
  "flagged": [
    {"table":"internal_staff","why":"名字像内部表，但只有 5 列且无任何列注释…","concern":"若只是后台账号…"}
  ]
}
```

页面两个 **只读** section（`page.rs:163` / `:178`）：`#excluded`「被排除 12 张」+ `#flagged`「保留但存疑 11 张
（**没有被排除** · 已照常进本体 · 待裁决）」。⛔ 无按钮、无输入框、无写接口。

> 数据来源：`proposals.inputs`（由 `propose import` 从提案旁边的 `proposal-exclusions.json` 读入并校验 sha256）。
> 界面**不解析** `exclusions.json`、也没有 schema —— 「哪些表被排除」只有一处实现。

---

## 7 · 已知限制

1. **侧车文件**：提案只留 `{file, version, sha256, declared_by, record}`（AC-001 要求提案里 grep 不到表名），
   逐条记录在 `proposal-exclusions.json`；两者由 sha256 绑定，**必须一起搬运**，否则 import 拒绝。
2. **`keep_but_flagged` 只展示**，不参与判定（表照常进本体）。
3. **表级**排除（R1）；列级仍是 `property_filter`。
4. **`tables_extracted` 实测自实体条目数**：重建路径若少抽表，闭合断言会让这一轮失败（R5 的本意，
   但相对旧行为是**行为变更**）。
5. **Windows 绕行**导致 `test_retained_names_req084` 编不过（既有红灯）。
6. `drop_rate` / `test_fk_naming` 在本机为红，**基线同样红**（§2.3）。
