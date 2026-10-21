# 开发状态 · REQ-113 排除表：从「脚本预处理」变成产品能力

> worktree `feature-REQ-113` · 分支 `feature/REQ-113` · 基线 `develop` `e9755e8` · **未 push**
> 日期：2026-10-22 · Windows · PG `postgresql://***@172.17.0.87:5432/anspire_real`（测试）/ `anspire_open_uat`（源库，只读）

- **需求**：REQ-113（AC-001 ~ AC-010）
- **当前任务**：全部实现完成，真实库实测 96 → 12 → 84，待 QA 审核
- **真实清单来源**：`exclusions.json`（= BA 提供的 `code/out/exclusions-ai.json`，`version 0.1.1`，
  `declared_by: ai-table-judge / qwen36-27B`，`reviewed_by: ba-michael（推翻 1 条、接受其余 12 条）`）

## 一、改动清单（`path:line`）

| # | 文件 | 关键位置 | 做什么 |
| --- | --- | --- | --- |
| 1 | `src/extract/exclusions.rs`（**新增**） | `:67` `Category`（7 类枚举）/ `:153` `Entry` / `:189` `Flagged` / `:305` `Exclusions::parse`（校验）/ `:490` `apply`（**唯一判定点**）/ `:503` `record`（**完整审计段**）/ `:623` `stale_warning` / `:654` `Record` + `Counts` / `:686` `from_inputs` / `:701` `validate`（含 counts 自查）/ `:771` `to_json` / `:887` `stamp_counts`（**闭合断言**） | 解析 / 校验 / 剔除 / 记录 / 断言，**一个实现**（纪律 H） |
| 2 | `src/bin/ontok-propose.rs` | `:165` `--exclude` · `:166` `--strict-exclusions` · `:217` 先校验文件 · `:271` 抽取前剔除 · `:294` 陈旧警告 · `:1011` `stamp_counts`（+ missing 表名） · `:1044` **审计段进提案** · `:1048` 台账打印 | CLI |
| 3 | `src/extract/propose.rs` | `:157` `Proposal.inputs` · `:243/246/253/256/261` 五个 stats 字段 · `:465` `tables_extracted` 在库路径落地 | 契约 |
| 4 | `src/agent/assemble.rs` | `:427` 重建路径也填 `tables_extracted` · `:453` `inputs: None` | 两条路径同口径 |
| 5 | `src/db/pg.rs` | `:369` `proposals.inputs JSONB` · `:424` `ALTER_PATCHES` 13→14 · `:532` 补列补丁 | 迁移 |
| 6 | `src/db/proposal.rs` | `:285` `set_inputs` · `:295` `inputs` | 存储 |
| 7 | `src/cli.rs` | `:1440-1481` **只读 `--file`**：校验审计段 + 落库（**无侧车、无第二文件**） | `propose import` |
| 8 | `src/review_ui/http.rs` | `:57` 模块头 · `:393` 读 `proposals.inputs` · `:432` `exclusions_block` · `:418` 响应加 `exclusions` | 只读 API |
| 9 | `src/review_ui/page.rs` | `:163` 「被排除」section · `:178` 「保留但存疑」section · `:395` `renderExcluded` | 只读区块 |
| 10 | `tests/test_req113_exclusions.rs`（**新增**，12 条用例） | AC-001/2/3/4/5/6/7/8 + 单文件导入 | 端到端判据 |
| 11 | `exclusions.json`（**新增**，仓库根） | 12 条排除 + 11 条 `keep_but_flagged` | 真实库清单（随代码入库） |

## 二、真实库实测（`anspire_open_uat`，96 表）

```
$ ontok-propose --schema <schema.json> --names src/extract/table_names.json \
    --out out/proposal.json --stats out/stats.json --project req113_demo \
    --exclude exclusions.json --src $SRC_URL
```

| 项 | 值 |
| --- | --- |
| `tables_seen` | **96** |
| `tables_excluded` | **12** |
| `tables_extracted` | **84** |
| 闭合 `seen − excluded == extracted` | **true**（断言在 `exclusions.rs:887`，不闭合 ⇒ 不写 stats、exit≠0） |
| `entities`（提案里 `add_entity_type`） | **84** |
| `stale_exclusions` | 0 |
| `flagged_tables` | 11（**未被排除**，已照常进本体） |
| **AC-001 ①** `items` 段里 grep 12 个被排除表名 | **0 次** |
| **AC-001 ②** 删掉 `inputs.exclusions` 整段后 grep | **0 次**（表名只允许出现在审计段内） |
| `inputs.exclusions.counts` | `{seen:96, excluded:12, extracted:84, stale:0, flagged:11}`（闭合） |
| `excluded_tables ∩ tables_without_cn_name` | **∅**（AC-007：排除优先） |

**12 条排除（逐条，AI 判定 + BA 复核）**

| 表 | 类别 | 理由 | 决策人 | 置信度 |
| --- | --- | --- | --- | --- |
| `_volc_dts_pg_ddl_18e4b12d…` | cdc_residue | 数据同步工具自动生成的临时 DDL 表，不对应任何客户业务 | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_8f7cc0a6…` | cdc_residue | 同上（同族第 2 张） | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_9a38a0a4…` | cdc_residue | 同上（同族第 3 张） | ai-table-judge | 0.97 |
| `alembic_version` | migration_bookkeeping | 数据库迁移工具的版本号记账表，客户业务里没有对应事物 | ai-table-judge | 0.99 |
| `llm_provider_back` | backup_snapshot | llm_provider 的整表备份副本，列结构与主表完全一致 | ai-table-judge | 0.95 |
| `llm_provider_back_20260630` | backup_snapshot | 按日期命名的 llm_provider 备份副本，不是独立业务事物 | ai-table-judge | 0.96 |
| `llm_provider_back_20260701` | backup_snapshot | 同上 | ai-table-judge | 0.96 |
| `llm_auth_cache_change_outbox` | internal_pipeline | 权限缓存变更的内部发件箱，只给同步程序读写，客户不可见 | ai-table-judge | 0.94 |
| `llm_auth_cache_kafka_event_outbox` | internal_pipeline | 内部消息队列发件箱，记录待投递事件而非业务事实 | ai-table-judge | 0.94 |
| `llm_auth_cache_trigger_state` | internal_pipeline | 权限边界判断用的内部状态快照，本身不是业务对象 | ai-table-judge | 0.90 |
| `rpt_dim_customer` | report_intermediate | 报表客户维度表，由客户与渠道数据派生，客户业务里无此事物 | ai-table-judge | 0.82 |
| `rpt_dws_channel` | report_intermediate | 渠道汇总的报表中间结果，不是某一笔业务的发生记录 | ai-table-judge | 0.82 |

## 三、确定性（AC-008）

同一份 `schema.json` + 同一份 `exclusions.json`，连跑两次（不同 `--out` 目录）：

```
proposal.json  sha256  run1 = 852281ec94f7acb47c429829d44c93cb41b065c7927bc67c1b8ee55f9485ab66
                       run2 = 852281ec…（相同；明细进提案后文件变大，sha 因此与上一轮不同）
stats.json     sha256  run1 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
                       run2 = 9c568845…（相同）
git diff --no-index out/proposal.json out2/proposal.json   → exit 0（无输出）
git diff --no-index out/stats.json    out2/stats.json      → exit 0（无输出）
```

> `--out` 目录里**只有** proposal.json / stats.json（+ REQ-108 既有的 pending / rejected）——
> 侧车 `proposal-exclusions.json` 已删除，实测 `Test-Path out/proposal-exclusions.json = False`。

## 四、陈旧排除两种模式（AC-005，真实库 + 一条不存在的表）

| 模式 | 结果 |
| --- | --- |
| 默认 | **exit 0**；stderr 点名 + `stats.stale_exclusions = ["_volc_dts_pg_ddl_0000…"]`；`tables_excluded` 仍是 12（陈旧的**不算**排除） |
| `--strict-exclusions` | **exit 1**，点名该表并说明两种可能；**不产出** proposal / stats |

## 五、审核界面（AC-006）

```
GET /api/review （Bearer 口令）
exclusions: present=True readonly=True count=12 flagged_count=11
  counts={seen:96, excluded:12, extracted:84, stale:0, flagged:11}
  file=exclusions.json  version=0.1.1
  sha256=3f77c6b8fd41bded7e5fbe74b507c74a2a385cfc67cccef2b90cada5dacc2c08
  declared_by=ai-table-judge / qwen36-27B
  reviewed_by=ba-michael（人工复核：推翻 1 条排除、接受其余 12 条）
tables[0] = {table:"_volc_dts_pg_ddl_18e4…", category:"cdc_residue",
             category_label:"CDC / 数据同步残留", reason:"数据同步工具自动生成的临时 DDL 表…",
             decided_by:"ai-table-judge", confidence:0.97, evidence:"表名为 _volc_dts_pg_ddl_ + 32 位哈希…"}
flagged[0] = {table:"internal_staff", why:"名字像内部表，但只有 5 列且无任何列注释…", concern:"若只是后台账号…"}
```

页面两块（`page.rs:163` / `:178`）：

```html
<section id="excluded">被排除 <b id="x-count">12</b> 张
  <span class="x-note">（只读 · 改排除要改 exclusions.json 重跑，页面上不能编辑）</span>
  …表 / 类别 / 理由 / 决策人 / 置信度（+ 依据）
</section>
<section id="flagged">保留但存疑 <b id="f-count">11</b> 张
  <span class="x-note">（**没有被排除** · 已照常进本体 · 待裁决）</span>
  …表 / 为什么拿不准 / 担心什么
</section>
```

## 六、测试

见 `verification-report.md`（本轮全量运行结果与两处绕行证明）。

## 七、已知限制（如实登记）

1. **审计段里含表名**（`inputs.exclusions.excluded[]`）——这是 BA 裁定的「单一文件」：
   `items` 段里 grep = 0，且**整份文件里这些表名只允许出现在 `inputs.exclusions` 段内**
   （实测：删掉该段后 grep = 0）。所以提案会大 ~4 KB，换来「一个文件 = 一条完整审计链」。
2. **`keep_but_flagged` 只展示、不参与判定**：这些表照常进本体（`tables_extracted` 里有它们），
   界面单独一块列出。要裁决它们，得改 `exclusions.json`（加进 `exclude`）重跑。
3. **排除的粒度是表级**（R1）：列级「属性剔除」仍是 `property_filter` 的事，两者不混。
4. **`tables_extracted` 由实体条目数实测**：重建路径若因分组失败少抽了表，闭合断言会让这一轮**失败**
   （不写 stats），报错里**点名哪几张表没产出实体**。这是 R5 的本意，但属于**行为变更**
   （以前只是安静地写出偏小的 items）。
5. **平台**：`src/bin/ontok-gate-preflight.rs` 是 Unix-only，Windows 上必编不过 —— 全量测试只能绕行
   （挪开 → 跑 → 挪回，见 `verification-report.md` §2），代价是 `test_retained_names_req084` 编不过（既有红灯，REQ-112 已登记）。
6. **`drop_rate` / `test_fk_naming` 两个门禁在本机为红**：它们要一份**含中文名的脱敏 schema**，
   本机只有 `code/out/schema.json`（96 张真实表名、翻译覆盖 0）⇒ 丢弃率 69.8%。**与本次改动无关**
   （两个门禁只调 `extract()` / `project()`，本轮一个字节都没改；基线 `e9755e8` 上逐字复现）。
