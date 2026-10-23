# QA 审核报告：REQ-128 · `ontok-propose --names` 必填

| 项 | 值 |
| --- | --- |
| 审核类型 | Pre-merge |
| 被测 | `feature-REQ-128` / `feature/REQ-128` / `15a2227` |
| 基线 | `develop` / `1a7e1b4` |
| 审核人 | QA Agent |
| 日期 | 2026-10-24 |
| 结论 | **PASS 带非阻断**（0 阻断 / 3 非阻断，均为文案与记录类，不阻断合并） |

---

## 1 · 结论

**通过。** 6 条 AC 全部实测满足，其中 3 条关键判据做了**变异验证**（默认值加回 / 警告改拒绝 / `>` 改 `>=` / 去掉占位检测），**四条变异全部被抓红**，说明这套判据是「有牙的」，不是「断言恒真」。

| AC | 判据 | 结果 |
| --- | --- | --- |
| AC-001 | 不传 `--names` ⇒ 退出码非 0 + 三样可行动信息 + **不产出** | ✅ 实测退出码 **1**，`proposal.json` / `stats.json` 均未生成 |
| AC-002 | 显式传占位表 ⇒ **退出码 0 + 产物落盘** + stderr 告警（不中止） | ✅ 实测退出码 **0**，`tables_seen=3`，警告文案含「会让实体名全是占位符」 |
| AC-003 | 真实表 ⇒ 无占位警告 | ✅ 实测退出码 0，stderr **不含**「脱敏占位样例 / 占位符」 |
| 阈值边界 | 恰好 80% 不告警、81% 告警（严格 `> 80`） | ✅ 8/10=80% 静默；9/11=81.8% 告警；两条都退出码 0 |
| AC-004 | 既有测试不放宽：不 `#[ignore]`、不删、不改断言 | ✅ 既有测试**一个都没改**（diff 仅 3 文件）；新增 4 条测试 |
| AC-005 | `--help` 标「必填」+ 说明；QUICKSTART 示例带 `--names` | ✅ |
| AC-006 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` 零改；`Cargo.*` 零改；不退化 | ✅ 见 §7 |

**口径更正成立**：基线里确实**没有**测试依赖那个默认值，Dev **不需要**改既有测试 —— 详见 §10（其中数字与简报略有出入，结论不变）。

---

## 2 · AC-001 · 不传 `--names` ⇒ 失败（含变异）

### 2.1 真实调用

```
.\target\debug\ontok-propose.exe --schema fixtures/schema-req109.json \
  --out out\qa-req128\proposal.json --stats out\qa-req128\stats.json --project qa-req128
```

| 断言 | 实测 |
| --- | --- |
| 退出码非 0 | **1** ✅ |
| stderr 含「必须显式提供翻译表」 | ✅ |
| stderr 含「`table_names.json` 是脱敏占位样例」 | ✅（原文：`src/extract/table_names.json 是脱敏占位样例（t001→对象 001）`） |
| stderr 含 `ontok-translate-names` + `ontok name suggest --as-names` 两条生成命令 | ✅ |
| `--out` 路径不存在 | ✅ 未生成 |
| `--stats` 路径不存在 | ✅ 未生成（该目录里只有我重定向的 stdout/stderr 两个文件） |
| 不产出半份 pending/rejected | ✅ 未生成 |

stderr 原文（首段）：

```
Error: 必须显式提供翻译表（--names），它没有默认值。

仓库自带的 src/extract/table_names.json 是脱敏占位样例（t001→对象 001），不能用于真实抽取：
它会抽出「对象 001」这种占位实体名，而且**不报错** —— 白跑一轮，产物看起来和真实抽取一模一样。

真实翻译表怎么生成（二选一）：
  ① ontok-translate-names --schema <schema.json> --out <table_names.json>
  ② ontok name suggest --as-names —— 一条命令同时产出可评审提案与翻译表：
     ontok name suggest --schema <schema.json> --out <names.proposal.json> --as-names <table_names.json>
```

判据点：**先判后跑** —— 这个 `bail!` 排在排除表 / LLM / schema / 翻译表读取之前，所以不会先花钱再失败，也不会留下半份产物。

### 2.2 变异 M1：把默认值加回去

```diff
-    let mut names_path: Option<String> = None;
+    let mut names_path: Option<String> = Some(String::from("src/extract/table_names.json"));
```

`cargo test --test test_req128_names_required` ⇒ **`1 failed`（ac001_without_names_exits_nonzero_with_actionable_error）**，其余 3 条通过。红得正确：加回默认值后程序**退出 0 并写出产物**，正是「静默白跑」，测试立刻抓到。

---

## 3 · AC-002 · 显式传占位表 ⇒ 警告但继续（含变异）

### 3.1 真实调用（`--names src/extract/table_names.json`，91 条占位）

| 断言 | 实测 |
| --- | --- |
| 退出码 0 | **0** ✅ |
| 产物生成 | ✅ `proposal-ph.json` 2463 B、`stats-ph.json` 412 B |
| 确实跑完（不截断） | ✅ `tables_seen=3`、`tables_extracted=3` |
| stderr 有占位警告 | ✅ `91/91 条（100%）` |
| 警告说明「实体名全是占位符」 | ✅ 原文：「这会让实体名全是占位符（「对象 001」这类）」 |
| **不能中止** | ✅ 同一次运行里照常打印后续的租户/采样提示并写出全部产物 |

stderr 原文（占位警告段）：

```
⚠ REQ-128：翻译表 src/extract/table_names.json 看起来是脱敏占位样例：91/91 条（100%）是「t001 → 对象 001」这种占位条目。
  这会让实体名全是占位符（「对象 001」这类），真实抽取请用 `ontok-translate-names` 或 `ontok name suggest --as-names` 按自己的 schema 生成翻译表。
  ⚠ 只警告、不拒绝：本轮继续运行（本地测试需要这份脱敏样例）。
```

### 3.2 变异 M2：把「警告」改成「拒绝」

在警告后追加 `std::process::exit(2);` ⇒ **`2 failed`**：`ac002_placeholder_names_warns_but_still_completes`（断言 `left: 2, right: 0`）与 `ac002_threshold_...` 同时红。

⇒ 「只警告不拒绝」这个口径**被测试守住**：谁改成拒绝，AC-002 立刻红。

---

## 4 · AC-003 · 真实表 ⇒ 无警告

自造 3 条真实中文名（`llm_model → 大模型`、`llm_model_sort → 模型排序`、`orders → 订单`）：

| 断言 | 实测 |
| --- | --- |
| 退出码 0 | **0** ✅ |
| 产物生成 | ✅ `proposal-real.json` 2397 B、`stats-real.json` 412 B |
| stderr 不含「脱敏占位样例」 | ✅ 不含 |
| stderr 不含「占位符」 | ✅ 不含（注意：普通告警里也没有这个词） |

⇒ 告警不是恒真：真实表不会误报。

---

## 5 · 阈值边界（80% / 81%）+ 变异

实现用整数比较 `hits * 100 <= total * 80 ⇒ 不告警`，即**严格大于 80%**。两侧各实测一次：

| 用例 | 占位比例 | 退出码 | 告警 | 产物 |
| --- | --- | --- | --- | --- |
| 8/10 | **恰好 80%** | 0 | ❌ 无（符合 `> 80`） | ✅ 生成 |
| 9/11 | 81.8% | 0 | ✅ 有（`9/11 条（81%）`） | ✅ 生成 |
| 4/5（测试内） | 80% | 0 | ❌ 无 | — |
| 5/6（测试内） | 83.3% | 0 | ✅ 有 | — |

### 变异 M3：把 `>` 改成 `>=`

```diff
-    if hits * 100 <= total * 80 {
+    if hits * 100 < total * 80 {
```

⇒ **`1 failed`（ac002_threshold_is_strictly_above_80_percent）**，红在 80% 那侧：「4/5 = 80% **不**该告警」。边界被钉死。

### 变异 M4（附加，对应 AC-004/G）：去掉占位检测调用

```diff
-    warn_if_placeholder_names(&names_path, &names);
```

⇒ **`2 failed`**（AC-002 占位告警 + 阈值 83% 告警）。检测逻辑不是死代码。

---

## 6 · 交付完整性

| 项 | 实测 | 结果 |
| --- | --- | --- |
| `git rev-list --count develop..HEAD` | **1** | ✅ |
| `git status --short` | 空（测试临时文件都在 gitignore 的 `out/`） | ✅ |
| 变更面 | `M QUICKSTART.md`、`M src/bin/ontok-propose.rs`、`A tests/test_req128_names_required.rs` | ✅ 只碰该碰的 |
| 独立 worktree `cargo check --lib` | `qa-REQ-128-check`（detached `15a2227`，**独立 target**）⇒ `Finished dev profile`，**exit 0**（1m08s） | ✅ |
| 替身/绕行 | 未挪 `src/bin/ontok-gate-preflight.rs`，未加 `#[ignore]`，未删测试（删除文件数 0） | ✅ |
| 未 push | 本地/远程均无 `feature/REQ-128` 远程分支（`git branch -r` 空） | ✅ |
| feature 分支未被污染 | 变异全部在**独立 worktree** `qa-REQ-128-mut`（detached），改完即 `git worktree remove`；feature worktree `git status` 干净 | ✅ |

**方法注记（透明起见）**：变异阶段为省编译时间，让变异 worktree 与 feature 共用过 `CARGO_TARGET_DIR`。收工后首次跑 feature 测试时，出现了**假红** —— 测试二进制里 `env!("CARGO_MANIFEST_DIR")` 还是变异 worktree 的路径，导致 schema 找不到。执行 `cargo clean -p ontok` 后用 feature 源码全量重编，下面 §7 的数字都是**重编后**的干净结果。这不影响被测代码，只影响我的测试工装；不记作产品缺陷。

---

## 7 · 不退化

### 7.1 lib 基线同机实测

| 环境 | `develop 1a7e1b4` | `feature 15a2227` |
| --- | --- | --- |
| 带 `ONTOK_TEST_DATABASE_URL=postgresql://postgres:postgres@172.17.0.87:5432/anspire_real` | **651 passed / 0 failed** | **651 passed / 0 failed** |
| 不导出该变量 | 649 passed / **2 failed**（`db::apply::…unique_conflict…`、`extract::introspect::…read_only_pool…`，均为 `MissingEnv("ONTOK_TEST_DATABASE_URL")`） | 同（环境性失败，非代码） |

⇒ **与 Dev 报的 651 / 0 逐条一致**；那 2 条只是环境变量缺失，不是回归。测试数没有减少（651 = 基线 651）。

### 7.2 涉及 `ontok-propose` 的既有集成测试

| 套件 | 结果 |
| --- | --- |
| `test_req109_cli_gate` | 6 passed / 0 failed |
| `test_req109_e2e_fake_llm` | 3 passed / 0 failed |
| `test_req113_exclusions` | 12 passed / 0 failed |
| `test_req128_names_required`（新增） | 4 passed / 0 failed |

这三个套件正是**拿占位表当夹具**的（`src/extract/table_names.json`），新增的 stderr 警告**没有**破坏它们：断言都是「退出码 + 子串」，且没有「stderr 必须为空/逐字节相等」的断言。Dev 说「全绿、无需改动」——**实测成立**。

### 7.3 那条既有失败：**基线既有，逐条同因**

`test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`

| 分支 | 结果 | 失败点 |
| --- | --- | --- |
| `develop 1a7e1b4` | **FAILED** | `tests/test_agent_orchestration.rs:1070` items 断言：CLI 少一条 `add_link_type`（`t_dev.customer_id → t_customer`） |
| `feature 15a2227` | **FAILED** | 同一行、同一差异（CLI items 3 条 vs legacy 4 条，完全相同） |

⇒ **基线既有，且失败内容逐字相同**，与 REQ-128 无关。原因与 Dev 的说法一致：本机没有 `SRC_URL`（`SRC_URL` / `ONTOK_SOURCE_DATABASE_URL` 实测为空），CLI 侧拿不到采样证据，所以推断不出那条 `tail_match` 关系；这条测试本身**显式传了 `--names`**，根本走不到默认值。

### 7.4 红线 / 禁改

| 项 | 实测 | 结果 |
| --- | --- | --- |
| 四红线 `tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs` | `numstat` **0 行** | ✅ |
| `src/mcp/tools.rs`（含 `guard()`） | `numstat` **0 行**；`git diff develop..HEAD -- src` 里 `guard(` 命中 **0** | ✅ |
| `src/domain/property_filter.rs` G1–G4 | `numstat` **0 行** | ✅ |
| `Cargo.toml` / `Cargo.lock` | `numstat` **0 行**（零新依赖；占位检测用整数比较，没引 regex） | ✅ |

---

## 8 · 文档

### 8.1 `ontok-propose --help`

`--help`（不传 `--names` 也能正常打印，说明 help 在必填校验之前）里新增 REQ-128 段，**标了必填且说明为什么**：

```
REQ-128 · --names 是**必填**（没有默认值）：
  --names <file>  翻译表。必填 —— 不传 ⇒ 退出码非 0，并给出生成方式。
                  仓库自带的 src/extract/table_names.json 是**脱敏占位样例**（91 条 t001→对象 001），
                  只能用来看流程，不能用于真实抽取（会让实体名全是占位符，且**不报错**）。
                  真实表用 ontok-translate-names 或 ontok name suggest --as-names 生成后显式传入。
                  显式传占位表 ⇒ 告警，但**继续运行**（本地测试需要它）。
```

### 8.2 `QUICKSTART.md`

| 位置 | 内容 | 结果 |
| --- | --- | --- |
| `:37-40`（Dev 说 `:37-41`，插入后行号偏移 +5，实质一致） | 「REQ-128 起 `--names` 是必填参数」：不传失败 / 显式传占位表告警但跑完 / 不再悄悄跑出「对象 001」 | ✅ |
| `:299`（dev 说 `:299`） | 命令速查 `ontok-propose --schema --names（必填） --out --stats --project` | ✅ |
| `:163-169` 示例 | `--names ./table_names.json` 显式传入 | ✅ 原本就有，未被改坏 |

`README.md` 是 5 行占位（无命令示例）；runbook 类文档即 QUICKSTART。除上面两处外，全仓 md 里 `ontok-propose` 只出现在 build 命令与「项目名在哪定」的说明句里，不构成示例命令。

---

## 9 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项（记录在案，不阻断合并）

| # | 项 | 说明 | 建议 |
| --- | --- | --- | --- |
| N1 | `demand.md` §四 的计数与实测不符 | demand 写「基线 19 处调用点 / 20 行 `--names`」。实测 **develop `1a7e1b4` 是 18 / 16**；19 / 20 是**加了新测试之后的 feature 分支**数字。结论（无测试依赖默认值）成立，只是把 feature 的数字写成了基线 | BA 顺手更正 demand.md §四 的「基线 develop 的 19 处」→「18 处（feature 加新测试后 19 处）」；不改代码 |
| N2 | 警告里的百分比是截断值 | 9/11 = 81.8%，文案打印 **81%**（整数除法）。比较用 `hits*100 > total*80` 是精确的，只有显示取整 | 可不改；若在意可打印 `{:.1}%`（要引浮点格式化，仍是零依赖） |
| N3 | 占位检测刻意「严格形状」 | 只认 `t`+3 位数字 且 `对象 `+3 位数字。若将来脱敏样例改名（如 `t0001` / `对象 1`）就不再告警 | 符合 AC 定义的检测规则，代码注释已说明「刻意严格」；知悉即可 |

> 附：`--help` 里用了 markdown 的 `**必填**`，终端会打出星号 —— 与周围 help 文案风格一致，属既有风格，未计入非阻断。

---

## 10 · 「口径更正」的独立复核（回答简报里的那条更正）

按简报给的命令在**两个分支各跑一遍**：

| 命令 | `develop 1a7e1b4` | `feature 15a2227` |
| --- | --- | --- |
| `git grep -n 'CARGO_BIN_EXE_ontok-propose' -- tests/ \| wc -l` | **18** | **19** |
| `git grep -n '"--names"' -- tests/ \| wc -l` | **16** | **20** |
| `git grep -c '"--names"' -- tests/` 文件数 | 12 | 13 |

数量接近（且差的那几行来自「用 helper 构造 args」的写法：`gate_args()`、`base_args()`、`name_args()` 等在函数里拼一次 `--names`，被多个用例复用），**没有一个调用点在构造实参时省略 `--names`**。

我逐个核对了 18 个调用点的实参构造：`test_agent_orchestration`（2 处，直接 args 含 `--names`）、`test_budget_report_truth`、`test_extract_cli_req019`、`test_extract_cli_req020`、`test_extracted_by_provenance`、`test_extraction_agent`（3 处）、`test_req109_cli_gate`（`gate_args:67`）、`test_req109_e2e_fake_llm`（`base_args:188`）、`test_req113_exclusions`、`test_req118_proposals`（514 处的 args 构造含 `--names:539`）、`test_req118_provenance_inputs`（`run_propose:119`）、`test_req120_tenant_gate` —— **全部显式传 `--names`**。

⇒ Dev 的实测结论正确：**基线没有测试依赖默认值，既有测试不需要改**。简报里「有些测试没传 `--names`，要改」的假设不成立；Dev 没有去动 19 个测试文件，这个判断是对的（R4「不改判据」也因此自然满足）。

---

## 附录 · 复现命令

```bash
# 基线 / feature 的 lib
cd code && ONTOK_TEST_DATABASE_URL=postgresql://postgres:postgres@172.17.0.87:5432/anspire_real cargo test --lib
cd feature-REQ-128 && ONTOK_TEST_DATABASE_URL=... cargo test --lib

# 新增测试 + 受影响套件
cd feature-REQ-128
cargo test --test test_req128_names_required --test test_req109_cli_gate \
           --test test_req109_e2e_fake_llm --test test_req113_exclusions

# AC-001 真跑
./target/debug/ontok-propose.exe --schema fixtures/schema-req109.json \
  --out /tmp/p.json --stats /tmp/s.json --project qa   # ⇒ exit 1，/tmp/p.json 不存在

# 既有失败（两分支同因）
cargo test --test test_agent_orchestration \
  test_cli_end_to_end_output_is_byte_identical_to_legacy_extract -- --exact
```

> 本次审核自建的临时目录：`feature-REQ-128/out/qa-req128/`（gitignore 内，已清理）。变异 worktree `qa-REQ-128-mut` 与 check worktree `qa-REQ-128-check` 均已移除，未在 feature 分支留下任何改动，未 push。
