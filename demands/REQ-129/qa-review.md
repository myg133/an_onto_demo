# QA 审核报告：REQ-129 · 文档断言合并进本体提案

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-129` @ `10e46d3`（基线 `develop` `8774034`，2 个提交） |
| 审核时间 | 2026-10-24 |
| 审核环境 | Windows · Rust debug · `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`（podman `ontok-pg`，host network） |
| 独立 worktree | 变异 `qa-REQ-129-mut`（detached `10e46d3`）、基线 `qa-REQ-129-base`（detached `8774034`），审核后已删除 |
| 纪律 | 未改 feature 分支、未 push、未手工改 `reviewed_by` 签字 |

## 1 · 结论

**PASS（带 4 条非阻断，0 阻断）。**

9 条 AC 全部独立复跑通过；AC-007「不签没有、签了有」在真实 fixture + 真实库 + 真实 TTL 上成立（52 → 53 条关系，grep 0 → 1）；`applicable: false` 是**真拒绝**（exit 1 并点名），不是忽略；`doc sign` 七条判据逐条实测全过；`--schema` 严格档 0 例外；四条必做变异全部让测试变红。

## 2 · 提交完整性

| 检查 | 命令 / 判据 | 结果 |
| --- | --- | --- |
| 提交数 | `git rev-list --count develop..HEAD` | **2**（`3b7bbb6`、`10e46d3`）✅ |
| 工作区 | `git status --short` | **空** ✅ |
| 独立 worktree 编译 | `qa-REQ-129-mut` 上 `cargo check --lib` | **exit 0**（`Finished dev profile in 1m 10s`）✅ |
| 改动面 | `git diff --name-status develop..HEAD` | 12 文件：`src/cli.rs`、`src/domain/apply.rs`、`src/db/{apply,pg,proposal}.rs`、`src/domain/pipeline.rs`、`src/extract/{doc_ingest,doc_merge,mod}.rs`、`tests/test_req107_*.rs`、`tests/test_req129_doc_merge.rs`；**无删除文件** ✅ |

## 3 · AC-007 端到端（自行重跑，含变异）

### 3.1 夹具

- 提案：真实 `code/out/proposal.json`（135 条：83 实体 + 52 关系），项目名改为唯一值后合并
- 断言：真实 `code/out/req116/cache/assertions.jsonl`（**56 行**）**追加 1 行**带显式映射的文档关系（`api_call_logs.customer_id → customers.id`，`N:1`），共 57 行
- `doc merge --schema code/out/schema.json` ⇒ merged（`items` 136 = 135 + 1 条 `origin: doc` 新边）
- 链路：`doc merge → propose import → review decide --accept-all → proposal apply → version freeze → ontology export --model-view`

### 3.2 两次导出（同一份真实提案，唯一差别是那条 doc 边是否已签）

| 轮次 | 操作 | TTL 类 | TTL 关系 | `api_call_logs_customer_id__customers` 命中 |
| --- | --- | --- | --- | --- |
| 未签 | 直接 import 合并产物（`reviewed_by: null`） | 83 | **52** | **0** |
| 已签 | `ontok doc sign --item relation:API 调用明细→客户 --by qa-req129 --at …` 后 import | 83 | **53** | **1** |

- 关系数**恰好 +1**；签后 TTL 里出现 `:api_call_logs__customers a owl:FunctionalProperty, owl:ObjectProperty`。
- 未签轮 apply 的 stderr 明确报告：`· 未签的 doc 条目 1 条 ⇒ apply 时忽略（规则 7：未签不进快照）`。
- 全程**没有**手工改文件里的 `reviewed_by`，签字只走 `ontok doc sign`。
- 证据：`D:\MyCodes\AI\an_onto_demo\.local\qa-req129\e2e\ttl-u.ttl`（52）/`ttl-s.ttl`（53）；项目 `qa129u122845` / `qa129s122854`。
- 集成测试 `ac007_unsigned_doc_edges_stay_out_of_the_ttl_and_signed_ones_enter_it` 同机通过。

### 3.3 变异 M1：去掉 `unsigned_doc_only()` 的忽略逻辑 ⇒ 测试红

- 变异：`src/domain/apply.rs:132` 的 `unsigned_doc_only` 直接返回 `Vec::new()`。
- `cargo test --lib domain::apply::tests` ⇒ **2 红**：
  - `unsigned_doc_items_are_ignored_and_signed_ones_are_applied` FAILED
  - `an_all_unsigned_doc_proposal_is_nothing_applicable` FAILED
- 端到端同变异：`ac007_...` **FAILED**，断言消息：`**未签**的 doc 边不许出现在 TTL 里（AC-007）` —— **不签也进快照**被真实抓住。
- 结论：该判据有牙，不是「只测函数名」。

## 4 · AC-006：`applicable: false` 是拒绝（不是忽略）

### 4.1 造一份带冲突的合并产物

在 57 行夹具上再加一条 `订单-[下单]->客户 cardinality 1:1`（DB 侧同一条边是 `N:1`）：

- `stats.conflicts = 1`；`stats.doc_unmapped = 14`；`stats.merged = 15`
- 条目 `order_customer_id__customers`：`applicable = false`、`origin = both`、`conflicts[0].doc_says = "1:1"`、`conflicts[0].db_says = "N:1"`（**双保留**）

### 4.2 apply 实测

```
project new / version new / propose import / review decide --accept-all
proposal apply --proposal p1 --project qa129c122851
```

| 判据 | 结果 |
| --- | --- |
| 退出码 | **1**（非 0）✅ |
| 是否被忽略 | **否**（没有生成 v2）✅ |
| 报错原文 | `应用失败：条目被标为不可应用（applicable: false），拒绝 apply：订单→客户（121:add_link_type:订单→客户）` |
| 点名 | 含 `订单→客户` ✅；含 `applicable: false` ✅ |

### 4.3 变异 M2：把「拒绝」改成「忽略」⇒ 测试红

- 变异：删除 `src/domain/apply.rs:161-165` 的 `NotApplicable` 提前返回。
- `cargo test --lib domain::apply::tests` ⇒ `applier_rejects_an_inapplicable_item_and_names_it` **FAILED**
- 集成测试 `ac006_applier_refuses_an_inapplicable_item_and_names_it` **FAILED**，消息：`带 applicable:false 的提案**不许**被 apply（AC-006：拒绝，不是忽略）：提案已应用 → v2…`
- 结论：拒绝语义与测试判据一致，变异可杀。

## 5 · `doc sign` 七条判据（逐条自跑）

| # | 判据 | 实测 | 结果 |
| --- | --- | --- | --- |
| 1 | `--by` 必填、匿名拒绝 | `--by "   "` ⇒ **exit 1** + `⛔ 不许签成匿名`；不给 `--by` ⇒ clap **exit 2** | ✅ |
| 2 | 稳定键 / 序号等价，⛔ 不认数组下标 | `136` / `relation:API 调用明细→客户` / `relation:api_call_logs_customer_id__customers` / `136:add_link_type:API 调用明细→客户` / `relation:API 调用明细->客户` **五种写法 sha256 全同**（`C18E5A69…AEED17`） | ✅ |
| 3 | 只签 `doc` / `both`；DB 轨拒签 | 第 1 条（无 `origin`）⇒ **exit 1**，提示走 `ontok review decide` | ✅ |
| 4 | 只改 `reviewed_by` + `signed_at` | 条目键集合仅多 `signed_at`；其余 **135 条逐条 JSON 全同**；剥掉两栏后逐值相同 | ✅ |
| 5 | 幂等，且不覆盖他人签名 | 同一人重复签（含改 `--at`）⇒ exit 0、**sha256 不变**；换人签 ⇒ **exit 1** 且**不落文件** | ✅ |
| 6 | 缺 `--out` 拒绝、不就地改输入 | 缺 `--out` ⇒ exit 1（不写盘）；`--out` 指向输入 ⇒ exit 1；`--out`+`--in-place` ⇒ exit 1；输入 sha256 全程 `876860EA…DE1C` 未变；显式 `--in-place` 才覆盖 | ✅ |
| 7 | 确定性 | 同输入 + 同参数（含 `--at`）两次 **sha256 相同**；省略 `--at` 取系统 UTC（实测 `2026-10-24T04:28:28Z`，符合 Q-3） | ✅ |

补充：签字输出对非规范格式输入会规范化并警告；输入本就规范时无警告。

## 6 · `--schema` 严格档（Q-2）

| 情形 | 实测 | 结果 |
| --- | --- | --- |
| 给 `--schema code/out/schema.json` | `schema_checked=true`、`schema_tables=96`、`parent_key_id_convention=0`、`parent_column_schema_only=1` ⇒ `customers.id` **靠 schema 严格通过**（不再靠 `id` 约定） | ✅ |
| 从 schema 副本删除 `customers.id` | 那条 doc 关系进 `doc_unmapped`：`merged=14`、`doc_unmapped=15`、原因 `column_not_in_schema: 1`、**items_added=0**（0 例外） | ✅ |
| 不给 `--schema` | `parent_key_id_convention=1`、`schema_checked=false`，stderr 两行：`⚠ 父端列 : 1 条只靠约定主键名 'id' 通过…` + `严格校验 : 否…要 0 例外请给 --schema` | ✅ |

概念侧同样严格：`resolve_concept` 在给 schema 时要求 `maps_to.table` 在真实库（`src/extract/doc_merge.rs:624`）。

## 7 · 合并规则正确性（AC-002 ~ AC-005，含变异）

### AC-002 · 只并 `alignable: true`

- 真实 56 行输入：`excluded_not_alignable = 28`（`future 14` + `example 14`），与输入逐项相等；`merged=14`。
- merged 里**没有** `积分转赠` / `下单示例` 的**条目或断言**。
- ⚠️ 非阻断 N3：`merged.json` 全文 grep `积分转赠` **会命中** —— 它出现在已并入的「客户」断言的**逐字证据 quote** 里（REQ-116 的 quote 是整个 118 字节块，块内含该词）。这不是把被排除断言并进来了（stats 与条目均可证）；建议 AC 的「grep 不到」口径改为按条目/断言匹配，见 §10。

### AC-003 · doc 条目 band 不许 strong

- merged 中 `origin ∈ {doc,both}` 的 2 条：`band = medium` / `low`，`reviewed_by` 全为 `null`。
- 变异 **M3**：`DOC_BAND_CAP` 改成 `"strong"` ⇒ **3 条测试红**（`doc_items_never_carry_a_band_stronger_than_medium`、`a_strong_input_band_is_capped_to_medium`、`a_new_doc_edge_becomes_a_doc_only_link_with_column_evidence`）。✅

### AC-004 · 无映射进 unmapped、不发明表名

- 真实 14 条关系全部无 `maps_to` ⇒ `doc_unmapped=14`，原因 `no_maps_to`；merged 条目数 = 135（无新增边）。
- 57 行夹具中唯一 doc 边：`maps_to` 的 2 个端点（`api_call_logs.customer_id`、`customers.id`）**都在 schema.json 里**（自查 0 缺失）。
- 变异 **M4**：在 `resolve_relation` 无 `maps_to` 时按 subject/object 名字猜表 ⇒ `a_relation_without_maps_to_is_unmapped_and_never_guessed` **FAILED**（条目数从 4 变 5、`doc_unmapped` 0）。✅

### AC-005 · 同实体合并为 `both`

- `customers` 条目**只有 1 条**：`origin=both`、`band=low`、`payload.doc_evidence=14`、DB `payload.evidence=1`（两侧证据都在，共 15 条）。
- 全 merged **无重复 `payload.key`**（不会有两条指向同一张表的实体）。✅

### AC-008 · 真实 fixtures 五项统计

`code/out/req116/cache/assertions.jsonl`（56 行）× `code/out/proposal.json`（135 条）：

| 并入 | 排除不可对齐 | 未映射 | 冲突 | 合并为 both | doc-only |
| --- | --- | --- | --- | --- | --- |
| **14** | **28**（future 14 / example 14） | **14**（no_maps_to） | **0** | **1** | 0 |

`items_out=135`、`items_added=0`，与 Dev 自报一致。

## 8 · 确定性

| 项 | 结果 |
| --- | --- |
| `merge --schema` 两次 | merged sha256 **`876860EA…DE1C`** 两次相同；stats sha256 也相同 ✅ |
| `merge` 不给 schema 两次 | merged sha256 **`E8AA59C9…2507`** 两次相同；stats 相同 ✅ |
| `sign` 同参数（含 `--at`）两次 | sha256 **`C18E5A69…AE17`** 相同 ✅ |
| 输出是否含时间戳/随机 | 无；`doc_merge` 摘要块只含内容属性；每次 sha 由 `assertions_sha256` + `proposal_sha256` 支撑 ✅ |
| 省略 `--at` | 取系统 UTC（事实），与 Q-3 裁定一致；确定性口径 = 同输入 + 同参数 ✅ |

## 9 · 不退化

### 9.1 指定测试集

| 集合 | 结果 |
| --- | --- |
| `cargo test --lib doc_merge` | **30 passed / 0 failed** ✅ |
| `cargo test --lib domain::apply::tests` | **6 passed / 0 failed** ✅ |
| `cargo test --test test_req129_doc_merge`（带库） | **13 passed / 0 failed** ✅ |

### 9.2 全量（带真实库，`--no-fail-fast`）

**1721 passed / 6 failed / 1 ignored** —— 与 Dev 自报一致。6 条失败**全部在基线上同名同文件同行号复现**（`qa-REQ-129-base` @ `8774034`，同机同库）：

| 失败测试 | feature | baseline |
| --- | --- | --- |
| `drop_rate::real_schema_drop_rate_report` | `tests\drop_rate.rs:82` | `tests\drop_rate.rs:82` |
| `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | `tests\test_agent_orchestration.rs:1070` | 同 |
| `test_anchor_guard::no_bare_env_manifest_dir_outside_anchor_file` | `tests\test_anchor_guard.rs:584` | 同 |
| `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | `tests\test_fk_naming.rs:316` | 同 |
| `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | `tests\test_retained_names_req084.rs:270` | 同 |
| `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | `tests\test_retained_names_req084.rs:293` | 同 |

⇒ 与本 REQ 无关的基线问题，不构成退化。

### 9.3 禁改面

`git diff develop..HEAD --numstat` 对以下路径**全为空**：

- 四红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs`
- `src/mcp/tools.rs`（含 `guard()`，定义在 `src/mcp/tools.rs:693`）
- `src/domain/property_filter.rs`、`src/server/`、`src/mcp/`
- `Cargo.toml`、`Cargo.lock`（**新增依赖 0**）

其它纪律：无删除测试、无新增 `#[ignore]`、无 `#[should_panic]`；`doc merge`/`doc sign` 均不连库、不调模型。

### 9.4 出口登记

- `OUTLETS.len() = 62`（客户 17 / 建模 10 / **内部 32** / 静态 2 / 错误路径 1）；I26（合并提案）、I27（签字产物）均已登记，`registry_is_the_declared_baseline` 与 `test_req107_outlet_registry_matches_the_declared_baseline` 均绿。

### 9.5 `doc ingest` 语义未变

- diff 仅追加 `Deserialize` 派生 + `#[serde(default)]`（`Serialize` 侧不变）。
- 实测同一 docs 目录 `doc ingest --chunks-only`：feature 与 develop 二进制产物 sha256 **同为 `8DAD2AC0…CEA2`**（逐字节相同）。✅

## 10 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项（4 条）

| # | 严重度 | 项 | 证据 | 建议 |
| --- | --- | --- | --- | --- |
| N1 | 低·中 | `doc merge` 的「不原地覆盖」只守 `--out`，**`--stats` 未守**：`--stats` 指向 `--proposal`（或 `--assertions`、`--out`）会**静默覆盖**，退出码仍 0 | 实测 `--stats <proposal>` ⇒ 原提案被写成 stats JSON（`{"merged":14,...}`），exit 0 | 把 `same_path` 检查扩到 `--stats`（对两份输入 + `--out` 都比），并在 `--out == --stats` 时拒绝 |
| N2 | 信息 | Q-3 的可选建议 `signed_at_source: "arg" \| "system"` **未实现** | `grep signed_at_source` 无命中 | 维持不阻断（Q-3 已写明「做不了就登记」）；若要审计区分，可在 `SignOutcome`/条目里加一栏 |
| N3 | 低 | AC-002 的「merged 里 grep 不到这些名字」在**真实 fixture** 上有假阳性：被排除断言（`积分转赠`）的名字出现在**已并入**「客户」断言的逐字证据 `quote` 里 | `merged-1.json:1795` 的 `quote` 含「未来规划…积分转赠」；但无对应条目/断言，`excluded=28` 精确 | 判据改为按 `items[].doc_evidence[]` 的 `kind+name/subject+predicate+object` 匹配；或夹具改用不共享块的 quote |
| N4 | 低·流程 | 重跑 `doc merge` 会**清掉**已签条目的 `reviewed_by`/`signed_at`（规则 2 绝对；计数 `reviewed_by_reset` + 文档已登记） | `src/extract/doc_merge.rs:1152-1158`；stderr 有 `⚠ 签名重置` | 不是缺陷；建议流程上明确「先最终合并、再签字」，并在 CLI 帮助中再显式一句 |

### 备注（AC 自身口径问题）

- AC-002 的原文判据是「`M` 里 grep 不到这些名字」。在 REQ-116 的逐字证据模型下，**已并入断言的 quote 可能包含被排除断言的名字**（证据必须逐字，不能清洗）。本次按「条目/断言未被并入 + 计数相等」判 **通过**，并把 grep 口径问题登记为 N3（**AC 也会错**，按纪律 S 记录）。

---

**审核人**：QA Agent（REQ-129 Pre-merge）
**证据目录**：`D:\MyCodes\AI\an_onto_demo\.local\qa-req129\`（`e2e/merged-1.json`、`e2e/signed-1.json`、`e2e/ttl-u.ttl`、`e2e/ttl-s.ttl`、`e2e/stats-*.json`、`full-feature-db.log`、`baseline-6-db.log`、`test-req129.log`）
