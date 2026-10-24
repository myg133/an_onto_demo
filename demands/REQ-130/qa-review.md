# QA 审核报告：REQ-130 · REQ-129 遗留两条收尾（判据修复 + `signed_at_source`）

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-130` @ `7ca0743`（基线 `develop` `267aaec`，**1 个提交**） |
| 审核时间 | 2026-10-24 |
| 审核环境 | Windows · Rust 1.96.0 debug · `postgresql://postgres:postgres@172.17.0.87:5432/anspire_real`（`ontok-pg` 在跑） |
| 独立 worktree | 变异 `qa-REQ-130-mut`（detached `7ca0743`），审核后**已删除**（`git worktree list` 已无此项） |
| 纪律 | ⛔ 未改 feature 分支（`git status` 空、HEAD 仍 `7ca0743`）、⛔ 未 push、⛔ 未挪 `src/bin/ontok-gate-preflight.rs`、⛔ 未手工改任何 `reviewed_by` |

> 本 REQ **不修功能只修判据**，所以下面每一条都问「判据有没有牙」，而不是「功能跑不跑得通」。

---

## 1 · 结论

**PASS（带 4 条非阻断，0 阻断）。**

| 必验项 | 结果 |
| --- | --- |
| 判据按**条目身份**匹配、不读 `evidence` | ✅ 实现里 0 处 `evidence` / `quote` / `claim`；真实产物上绿 |
| 真实 fixture：`excluded_not_alignable == 28` 且「积分转赠」作为条目 **0** 次 | ✅ 28；独立扫描条目身份 0 次 |
| 假阳性场景真实存在（`evidence.quote` 里确实有「积分转赠」） | ✅ 真实产物里出现 **1** 次，位置是第 24 条 `客户` 的 `payload.doc_evidence[].quote` |
| 反向判据三种污染（concept 条目 / relation 端点 / relation 谓词） | ✅ **三种都红，且都点名到「第 N 条 + 哪个字段」**（在真实产物上注入，调用交付的函数） |
| 三条变异（grep / 恒 `arg` / 去老产物兼容） | ✅ 红在预期处，见 §5 |
| `signed_at_source`：`arg` / `system` / `legacy` 三态 + 确定性 + 不伪造 | ✅ 六种情形 CLI 实测，见 §6 |
| 不退化 | ✅ lib **689**、`test_req129_doc_merge` **20**；全量 **1731 passed / 6 failed / 1 ignored**，6 条失败与基线**同名同文件**复现 |
| 只改 3 个文件 | ✅ `src/cli.rs`、`src/extract/doc_merge.rs`、`tests/test_req129_doc_merge.rs`；无第四业务文件 |

非阻断项：R-1 的 `--help` 半边未列 `legacy`（§8 N-1）；relation 身份用 `contains` 带来的**精度**风险（§8 N-2）；测试框架 `missing(...).unwrap()` 的 SKIP 实为 panic（§8 N-3，REQ-129 旧模式）；acceptance AC-001 字面未同步 `semantic`（§8 D-1）。**均不构成退回理由。**

---

## 2 · 判据实现是否真的不看 evidence

被测函数：`tests/test_req129_doc_merge.rs:410` `excluded_entries_in()`（配套 `excluded_terms()` 在 `:473`）。

### 2.1 静态：只读身份字段

| 分支 | 读的字段 | 匹配方式 |
| --- | --- | --- |
| `add_entity_type`（概念） | `target`、`payload.display_name`、`payload.key` | **相等** |
| `add_link_type`（关系） | `target` 整体 + 拆 `→`/`->` 后的两端、`payload.from_type` / `to_type` / `key`、`payload.semantic` | **包含** |
| 其它 kind | —— | 忽略（`doc merge` 只会产出上面两种，够用） |

- 在函数体（410–471 行）里搜 `evidence` / `quote` / `claim` / `doc_evidence`：**0 命中**。
- 它判的是「被排除词有没有成为一个**条目**」，**不读也不判 `alignable`**；被排除身份词从输入 JSONL 的 `alignable:false` 断言里推出（概念取 `name`，关系取 `predicate`，⛔ 不取 subject/object —— 两端可能正是已并入的合法实体）。
- 概念用**相等**、关系用**包含**的非对称是有必要的，见 2.2 —— 如果概念也改包含，真实 fixture 会立刻**假红**。

### 2.2 一个反直觉的细节：真实产物里本来就有「套餐退款」

真实 `merged.json` 第 65 条是提案里**既有**的 `add_entity_type`，`target = payload.display_name = "套餐退款"`；整文件里「退款」出现 **12** 次（第 21 条属性名、第 65 条实体名/属性）。

- 概念用**相等** ⇒ `套餐退款 != 退款`，不误判 ✅
- 若概念改用 `contains` ⇒ 这一条会被当成「被排除的概念被并入」**假红**
- 关系分支只覆盖 `add_link_type`，这两条都是实体，不会进关系分支

⇒ 这说明「判据要按条目、且概念必须按名字相等」不是风格问题，是真实 fixture 逼出来的正确选择。

---

## 3 · 假阳性场景是否真实存在（双向证据）

### 3.1 输入与产物

| 项 | 实测 |
| --- | --- |
| 断言 | `code/out/req116/cache/assertions.jsonl`，**56 行** |
| `alignable:false` | **28**（`future` 14 + `example` 14），全是 concept；`predicate` 类被排除关系**本 fixture 没有** |
| 推出的身份词 | `{下单示例, 积分转赠}`（BTreeSet 序） |
| 产物 | 135 条条目，`items_added 0`、`items_touched 1`、`conflicts 0` |

| stats 键 | 值 |
| --- | --- |
| `assertions_read` | 56 |
| `excluded_not_alignable` | **28** ✅ |
| `excluded_future` / `excluded_example` | 14 / 14 |
| `merged` / `doc_unmapped` / `conflicts` | 14 / 14 / 0 |

### 3.2 双向证据（同一份产物）

| 方向 | 结论 | 证据 |
| --- | --- | --- |
| 「积分转赠」作为**条目**出现 | **0 次** | 我另写了一个独立 walker（不复用被测函数）遍历 135 条：`target` / `display_name` / `key` / `semantic` / `from_type` / `to_type` 全等比较 ⇒ `entry_identity_hits = 0` |
| 「积分转赠」在**引文**里出现 | **1 次** | 第 24 条（`kind=add_entity_type`、`target=客户`、`payload.key=customers`）的 `payload.doc_evidence[].quote`：`"## 未来规划\n\n平台计划在下一财年支持积分转赠"`；独立 walker `quote_mentions = 1` |
| 整文件 grep | 命中 | `raw.contains("积分转赠") == true` |

⇒ **旧判据在这份真实 fixture 上必假红**（引文命中），**新判据必绿**（条目 0 次）。假阳性场景是真实的，判据不是在空转。

---

## 4 · 反向判据三种污染的实测

方法：在**真实产物**（135 条）的副本上分别注入 3 种污染，再调用**交付的** `excluded_entries_in()`（QA 探针只往测试文件 append 一条 `#[test]`，⛔ 不改被测函数本体；跑完即删）。

| # | 注入 | 期望 | 实测 | 点名 |
| --- | --- | --- | --- | --- |
| ① | `{"kind":"add_entity_type","target":"积分转赠","alignable":false,"payload":{...}}` 追加为第 136 条 | 红 | ✅ 红（2 条 leak） | `第 136 条：concept 的 target = "积分转赠"` / `第 136 条：concept 的 payload.display_name = "积分转赠"` |
| ② | `add_link_type` `target="客户→积分转赠"`（端点含被排除概念） | 红 | ✅ 红 | `第 136 条：relation 的 target = "客户→积分转赠" 含…` / `target 的一端 = "积分转赠" 含…` |
| ③ | `add_link_type` `target="客户→订单"`、`payload.semantic="积分转赠"`（两端干净，只有谓词脏） | 红 | ✅ 红 | `第 136 条：relation 的 payload.semantic = "积分转赠" 含被排除的 "积分转赠"` |
| ④ | **反证**：往第 24 条 `客户` 的 `payload.doc_evidence[]` 再塞一条含「积分转赠」的引文 | 绿 | ✅ 绿（且整文件仍能搜到该词） | —— |

补充：

- 交付测试自身在 tiny 夹具上用**被排除谓词「退款」**做第③类污染（`tests/test_req129_doc_merge.rs:557` 的 ②c 段），与我在真实产物上用「积分转赠」做的等价；该测试 **20/20 通过**。
- ③ 这条尤其关键：只查两端的实现会在它上面**空转**（两端是 `客户` / `订单`，都合法）—— R-2 接受 `payload.semantic` 进身份的裁定在这里得到实测支持。
- **结论：三种污染都红，且都点名到第几条 + 哪个字段；「只红一种」的缺口不存在（在这三种形态上）。**

---

## 5 · 三条变异复核

变异全部在独立 worktree `qa-REQ-130-mut`（detached `7ca0743`）上做，逐条 `git checkout -- .` 还原。日志：`.review-workspace/req130-qa/mutations.log`。

| 变异 | 改法 | 期望 | 实测 |
| --- | --- | --- | --- |
| **M1** 判据改回整文件 grep | 把 `excluded_entries_in()` 整体换成 `serde_json::to_string(doc).contains(term)` | **双红**（反向 + 真实 fixture） | ✅ **2 红**：`ac002_real_fixture_excludes_28_and_no_excluded_entry`（:675）、`ac002_reverse_injected_excluded_entry_turns_the_criterion_red`（:546）。`ac002_only_alignable_true_is_merged` 仍绿 —— 小夹具里没有「引文提到被排除词」的底料，旧 grep 本来就抓不到，**这正是「只有真实 fixture 才能暴露 N-3」的旁证** |
| **M2** `signed_at_source` 恒 `arg` | `src/cli.rs:2444` 省略 `--at` 的分支改成 `dm::AT_SOURCE_ARG` | `system` + 「arg≠system 可分」两条红 | ✅ **2 红**：`ac003_signed_at_source_is_system_when_at_is_omitted`（:1456，实际产物里 `"signed_at_source":"arg"`）、`ac003_arg_and_system_are_distinguishable`（:1502）；另 2 条 `ac003*` 绿 |
| **M3** 去掉老产物兼容 | `doc_merge.rs:1871` 的 `unwrap_or_else(|| AT_SOURCE_LEGACY)` 改成 `expect(...)`（缺字段即 panic） | `ac004_legacy` 红 | ✅ **2 红**：lib 单测 `extract::doc_merge::tests::sign_reads_a_legacy_artifact_without_a_source_field`（panic @ `src\extract\doc_merge.rs:1871:26`）、集成 `ac004_legacy_artifact_without_source_is_read_back_without_error`（:1562，同点 panic）。R-3 写的是 `1872`，实际 `1871` —— **变异本身造成的行号偏移，不影响结论** |

---

## 6 · `signed_at_source` 行为（CLI 实测）

夹具：tiny proposal + 5 行断言 → `doc merge` → 签 `relation:订单→套餐模板`（`origin: doc`）。脚本：`.review-workspace/req130-qa/cli-check2.ps1`。

| # | 情形 | 实测 |
| --- | --- | --- |
| A | `--at 2026-10-24T00:00:00Z` | `signed_at_source = "arg"`；stderr：`时间来源 : arg（--at 给的：声明，可复现）` |
| B | 同参数两次签 | 两个产物 SHA256 **相同**（`1112EE17673CFA72…`）⇒ 逐字节可复现 |
| C | 省略 `--at` | `signed_at_source = "system"`；`signed_at = 2026-10-24T05:11:49Z` 落在本次运行窗口 `[05:11:47.788Z … 05:11:51.937Z]` 内（用正则取原文比较，避免 JSON 反序列化把时间转成本地时区） |
| D | 老产物（删掉 `signed_at_source` 行）**重签同一条** + `--in-place` | exit **0**；**字节完全不变**（SHA256 同）；产物里**仍没有**该字段；stderr：`时间来源 : legacy（老产物没有 signed_at_source：按缺省处理，⛔ 不补写）` |
| E | 同一份老产物上**签另一条**（`entity:客户`） | 新条 `signed_at_source = "arg"`；老条**仍缺**该字段（不被顺手补写） |
| F | 幂等重签：先无 `--at` 签成 `system`，再**带 `--at`** 重签同一条 | 仍 `system` + 原时间 `05:11:49Z`，字节不变 ⇒ **不伪造 `arg`、不改写时间**（审计上最要紧的一条） |

`legacy` 不会写进任何产物：源码里它只用于读回报表（`AT_SOURCE_LEGACY` 的 doc comment 明确写「只出现在读回报表里，⛔ 不写进产物」），D 的实测字节比对证实。

---

## 7 · 不退化 + 只改 3 个文件

### 7.1 套件

| 项 | 结果 |
| --- | --- |
| `cargo test --lib` | **689 passed / 0 failed** ✅（与要求一致） |
| `cargo test --test test_req129_doc_merge`（带真实库） | **20 passed / 0 failed** ✅（与要求一致） |
| 全量 `cargo test --no-fail-fast`（带真实库） | **1731 passed / 6 failed / 1 ignored**，5 个 target 失败 |

6 条失败**全部在基线 `develop` `267aaec` 上同名同文件复现**（同机同库）：

| 失败测试 | feature | baseline `267aaec` |
| --- | --- | --- |
| `drop_rate::real_schema_drop_rate_report` | ❌ | ❌ 同 |
| `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | ❌ | ❌ 同 |
| `test_anchor_guard::no_bare_env_manifest_dir_outside_anchor_file` | ❌ | ❌ 同 |
| `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | ❌ | ❌ 同 |
| `test_retained_names_req084::test_req084_every_registered_name_is_still_literally_at_its_line` | ❌ | ❌ 同 |
| `test_retained_names_req084::test_req084_no_unregistered_real_name_remains_in_the_registered_files` | ❌ | ❌ 同 |

⇒ 与 REQ-129 QA 记录的那 6 条完全一致，**本 REQ 无关，不构成退化**。日志：`full-feature.log`、`baseline-develop-5targets.log`。

### 7.2 `doc merge` 的其他行为没变（逐字节）

同一份真实输入（56 行断言 × 135 条提案），用 `develop` `267aaec` 与 `feature` `7ca0743` 各跑一次 `doc merge`：

- `merged.json`：两边 SHA256 **完全相同**（`483ED9E1B04FFF8DD401367906A11A06FDF29937F1CD721E38A9C9FB5ADC14FD`）
- `stats.json`：逐字节相同

⇒ N-2 新增字段没有污染 `doc merge` 的产物；`SIGNED_AT_SOURCE_KEY` 只在「重跑合并清签名」时被一并清掉，对未签产物零影响。

### 7.3 只改 3 个文件 + 禁改面

| 检查 | 结果 |
| --- | --- |
| `git diff --name-only develop...HEAD` | **3 个**：`src/cli.rs`、`src/extract/doc_merge.rs`、`tests/test_req129_doc_merge.rs`；**无第四个业务文件** ✅ |
| 四红线：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`src/domain/view.rs`、`src/domain/review_view.rs` | blob **逐一相同**（`f3967ecd` / `6c0ae562` / `9d085a03` / `fdd7fcd9`）✅ |
| `src/mcp/tools.rs`（含 `guard()`，`tools.rs:693`） | blob 相同 `8f514039` ✅ |
| `src/domain/property_filter.rs` | blob 相同 `d4e93e33` ✅ |
| `Cargo.toml` / `Cargo.lock` | blob 相同（`3c268177` / `3edd02f5`）⇒ **无新依赖** ✅ |
| 删测试 / `#[ignore]` | `#[test]` 14 → 20（+6 集成），lib 新增 3 条单测；`#[ignore]` 仍是 **2**（与 develop 相同）；无删除 ✅ |
| 越权行为 | 未动 `doc ingest`、未动 `doc merge` 的既有语义（7.2 逐字节证明）✅ |

### 7.4 方法学说明（不是缺陷，只是提醒后续 QA）

为省编译时间，我把变异 worktree 的 `target/` 用 junction 指到 feature 的 `target/`。头一次在 feature 侧复跑时，cargo **复用了变异产物的测试二进制**，`ac004_legacy` 假红一次（panic 信息里带 `M3 变异` 字样）。随后在 feature 侧 `cargo clean -p ontok` 重建，得到 **20/20**；本报告所有 feature 侧数字都取自重建后的运行。⇒ **判据本身没问题，是共享 target 的缓存假象**；后续做变异审核建议直接给变异 worktree 独立 `CARGO_TARGET_DIR`。

---

## 8 · 阻断项 / 非阻断项

### 阻断项

**0 条。**

### 非阻断项

| # | 项 | 事实 | 为什么不阻断 |
| --- | --- | --- | --- |
| **N-1** | R-1 的「三个值列全」只做到一半 | `doc sign --help` 只写 `arg` / `system`，**没出现 `legacy` 字面**；模块头（`doc_merge.rs:80`）和运行期输出都列全了。另外 `doc sign --help` 的 **about 首行**仍是「只写 `reviewed_by` + `signed_at`」，与后文「三栏」自相矛盾 | R-1 的**实质裁定**（缺省用 `legacy`、不伪造成 `system`、不补写）已完整实现并实测通过；文档措辞是半行注释的事，不改变任何行为。建议顺手补：`--help` 里明写 `arg \| system \| legacy`，首行改成三栏 |
| **N-2** | relation 身份用 `contains`，对「名字**包含**被排除词」的合法条目会**假红** | 实测：把一条引用**既有实体** `套餐退款` 的合法边 `套餐退款→客户` 塞进真实产物、term 取 `退款` ⇒ 判据红（`target 的一端 = "套餐退款" 含被排除的 "退款"`）。这与 N-3 要修的**同一种病**，只是本 fixture 不触发（28 条被排除全是 concept，身份词是「积分转赠 / 下单示例」，没有边引用包含它们的名字） | 真阳性能力（§4）已证明；这是**精度**风险不是**召回**风险，且当前 fixture 上不会红。建议下一轮把 relation 两端也改成**拆开后相等**、谓词相等（concept 已经是相等），并留一条「包含但不等于 ⇒ 不红」的用例 |
| **N-3** | 测试框架的「响亮 SKIP」实为 **panic** | `missing()` 返回 `None`，调用处一律 `.unwrap()`（`:694`、`:965`、`:1033`），所以缺 DB / 缺 fixture 时不是 SKIP 而是 FAILED。我在变异 worktree 无库环境下实测 `18 passed / 2 failed`（`ac006` / `ac007`） | REQ-129 就有的旧模式，本 REQ 只是沿用到新的 `real_fixture()` 上；「响亮地失败」比「静默当绿」安全，不构成回归。建议改为 `return missing(...);` 并把函数签名改成 `Option<()>` 直返，或显式 `eprintln! + return` |
| **D-1** | acceptance AC-001 的字面没同步 `semantic` | `acceptance.md` AC-001 仍写「不存在 `subject/object` 含它的 relation 条目」，而实现（按 R-2 裁定）还判 `payload.semantic`；R-2 段落里已记录裁定，但 AC-001 正文没改 | R-2 明确「接受，属收紧」；实现比字面**更严**，不会放过真阳性。属 BA 文档同步项，不影响验收结论 |

---

## 附 · 证据位置（均为本地，未入库）

| 文件 | 内容 |
| --- | --- |
| `.review-workspace/req130-qa/merged.json` / `stats.json` | 真实 fixture 的合并产物与统计（28 / 56 / 135） |
| `.review-workspace/req130-qa/mutations.log` | M1 / M2 / M3 的完整 cargo 输出与失败名 |
| `.review-workspace/req130-qa/full-feature.log` | 全量 1731 / 6 / 1 |
| `.review-workspace/req130-qa/baseline-develop-5targets.log` | 基线 `267aaec` 同 5 个 target 的同样 6 条失败 |
| `.review-workspace/req130-qa/sign2/` | `arg.json` / `sys.json` / `legacy.json` / `legacy-plus.json` 与两次签字的字节比对 |
| `.review-workspace/req130-qa/cli-check2.ps1` | §6 六种情形的可重跑脚本 |
| `qa-REQ-130-mut` | 已删除（`git worktree list` 只剩 workspace / BA / code / Deploy / feature-REQ-130） |
