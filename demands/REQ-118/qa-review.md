# QA 审核：REQ-118 · AI 提案贯穿（Pre-merge · 独立复验 + 主动攻击）

> **被测**：worktree `feature-REQ-118` / 分支 `feature/REQ-118` · HEAD `0573fa8` · 基线 `develop` `5f6636f`
> **提交**：`b0a814f`（三条 suggest + 提案框架 + 96 表实测）+ `0573fa8`（A-1：P4 到库）
> **依据**：`BA/decisions/ADR-005-决策点必须由AI先提案.md`（P1–P4）+ `BA/demands/REQ-118/acceptance.md`（11 AC + A-1~A-6）
> **环境**：Windows · `rustc 1.96.0` · LLM `qwen36-27B` @ `https://ollama.apps.anspire.cn/v1` · PG `172.17.0.87:5432/anspire_real` · schema `code/out/schema.json`（96 表）
> **日期**：2026-10-22 · **无 push · feature 分支零改动**
> **QA 产物**：`feature-REQ-118/out/qa-req118/`（本轮所有实测输出，`.local/qa/` 放脚本与日志）

---

## 1 · 结论

**PASS 带非阻断**（0 阻断 · 7 条非阻断 + 2 条观察）

四条 ADR-005 铁律的**代码级判据全部通过**，且本轮最关键的 A-1（P4 进库）是**我自己真跑 CLI + 从 PG 读回来比 sha256**验的，不是读 Dev 的日志：

| ADR-005 | 判据 | 实测结论 |
| --- | --- | --- |
| **P1** 落盘可 diff | 三命令 `--out` 必填、pretty JSON、稳定排序 | ✅ 文件都在，两次运行逐字节相同；缺 `--out` clap exit≠0 |
| **P2** 提案永不权威 | `reviewed_by` 私有 / 无 setter / 生产路径恒 `None` / 零写库 / 提案文件进不了 `propose import` | ✅ 结构性成立（附 1 个 `Deserialize` 旁路，见 N-5） |
| **P3** 可复现 | 指纹缓存命中 ⇒ 0 token + 逐字节相同；负控清缓存后重新计费 | ✅ name/exclude/concept 三条都实测（含**负控**） |
| **P4** 可溯源 | 库 `proposals.inputs` 里 `names` / `exclusions` / `concepts` 的 sha256 == 文件实际 sha256 | ✅ **真进库了**（3 组独立项目 + 3 个 PG 读回） |

**7 条非阻断**里最该在合并前处理的是 **N-1（失败批次 fail-open：exit 0 + 一份可导入的空提案）** 与 **N-2（`comparison.md` 的 name 段只列了 87/96 行）**，理由见 §10。

---

## 2 · 本轮怎么验的（不依赖 Dev 的自述）

| 手段 | 说明 |
| --- | --- |
| 真进程 + 真库 | `ontok-propose` / `ontok propose import` / `ontok concept import` 全部真跑，结果用 **psycopg2** 直接从 PG 读回（不经过被测代码） |
| 独立算 sha256 | 文件 sha 用 `Get-FileHash`，sha256 对比不调用被测函数 |
| 真 LLM | 真端点真计费；端点偶发挂起时改用「真实 schema 的子集（2/3/12 表）」+「Dev 的真实 96 表缓存」两条腿（§5） |
| 变异 | **独立 worktree** `.local/qa-mut-req118`（detached `0573fa8`，独立 `target/`）；5 个变异 + 1 个探针，还原后 `git hash-object` 与 `HEAD:` 逐字节相同 |
| 绕行 | Windows 上 `src/bin/ontok-gate-preflight.rs` 是 unix-only：挪开 → 跑 → 还原 → `touch`。**还原双证见 §11**；变异全在独立 worktree 的独立 target 里做，feature 的 `target/` 从未接触变异源码 |

---

## 3 · P4 实测（A-1：提案文件的 sha 真的进库了吗）

### 3.1 names → PG

```text
文件 out/req118-real/cache-fresh/table_names.as-names.json
  sha256(Get-FileHash) = a70f697a8aa08a0fb5aae417804d766c93dff6e3f7657e29718a2ae51444408a

ontok-propose --schema code/out/schema.json --names <该文件> --project qa_req118_p4 --no-sample   → exit 0（96 items）
ontok propose import --file out/qa-req118/p4-names.proposal.json                                  → exit 0

PG: proposals b7eb436d-128d-484c-82df-592242786b69 · inputs =
{ "names": {
    "agent": "ai:qwen36-27B",
    "file": "out\\req118-real\\cache-fresh\\table_names.as-names.json",
    "proposal": { "file": ".../names.proposal.json",
                  "sha256": "5d991434fafcc1300e47c687266efd8abee85ba960a46de76a2f29f67c844955" },
    "reviewed_by": null,
    "sha256": "a70f697a8aa08a0fb5aae417804d766c93dff6e3f7657e29718a2ae51444408a",
    "version": "0.1.0+ai.qwen36-27B" } }
```

✅ **库里的 `inputs.names.sha256` == 文件实际 sha256**（逐字符相同）。
✅ `reviewed_by` **键存在、值为 null**（不是缺字段）。
✅ 额外记了原始提案文件 `proposal.sha256 = 5d991434…`，与 `cache-fresh/names.proposal.json` 的实际 sha 一致。

**旧翻译表（没有 `_proposal` 块）也验了**（`out/qa-req118/plain-names.json`）：
`inputs.names = {agent, file, reviewed_by: null, sha256: c818f7d6…, version}` —— `proposal` 子块缺席、`reviewed_by` 仍在，符合 A-1「允许 null 但键必须存在」。

### 3.2 names + exclude 同时给（Dev 说旧实现会吞掉 names）

```text
excl = out/req118-real/cache-fresh-excl/excl.proposal.json
  sha256 = 004b2fddd78226a9d6a24badd2c62d49b9a8d7d79a8dbc22719ba2dc847b2a4d

ontok-propose --names <names> --exclude <excl> --project qa_req118_p4b --no-sample  → exit 0（75 items）
ontok propose import                                                               → exit 0

PG: proposals 5504db4a… · inputs 顶层键 = ["exclusions", "names"]      ← 两个都在
    inputs.names.sha256      = a70f697a…08a                          ← 与 names 文件一致
    inputs.exclusions.sha256 = 004b2fdd…4d                           ← 与 exclude 文件一致
    counts = {seen 96, excluded 21, extracted 75, stale 0, flagged 0}
```

✅ 回归缺口（`Record::from_inputs` 命中时只写 `{exclusions}`）**已修**：两个键共存，sha 各自对得上。

### 3.3 concepts → PG

```text
文件 out/req118-real/groups.proposal.json
  sha256 = 5f67e13bd1d88c60f459ae8d8f12745c567a97d28361cb9614a4b1aaedc35cf2

ontok concept import --file <该文件> --project qa_req118_c --base-version 1   → exit 0（9 组）
PG: proposals 40ca2fed… · inputs.concepts =
{ "confirmed_by": null, "declared_by": "ai:qwen36-27B",
  "file": "out\\req118-real\\groups.proposal.json",
  "reviewed_by": null,
  "sha256": "5f67e13bd1d88c60f459ae8d8f12745c567a97d28361cb9614a4b1aaedc35cf2",
  "version": "1.0.0" }
```

✅ 与 A-1 要求的同构形状一致；`reviewed_by` / `confirmed_by` 键存在且为 null。

### 3.4 变异 M1：删掉 `inputs.names` 写入 ⇒ 必须有测试红

```text
变异（独立 worktree，src/bin/ontok-propose.rs）：删掉 inputs.insert("names", …) 整行
cargo test --test test_req118_provenance_inputs
→ FAILED. 1 passed; 2 failed
   names_provenance_is_persisted_with_matching_sha256  ← 文件层就红了（inputs.names = Null）
   names_and_exclusions_both_survive_import            ← "names 块要在"
```

✅ 有牙：文件层 + 库层两级同时抓；还原后 `git hash-object` == `HEAD:`（`f2699de9…`）。

---

## 4 · P2 结构性证据（提案永不权威）

### 4.1 类型层：私有字段 + 唯一构造器

| 落点 | 内容 |
| --- | --- |
| `src/extract/proposal.rs:171` | `reviewed_by: Option<String>`（**无 `pub`**） |
| `:177-187` | `impl ProposalItem` 只有 `reviewed_by()`（只读）与 `is_low_confidence()`；**全仓 grep `set_reviewed_by` = 0** |
| `:395-447` | `finalize_item` 是唯一构造入口，`:439` 恒写 `reviewed_by: None` |

### 4.2 零写库（剥注释后 grep）

对 `proposal.rs` / `name_suggest.rs` / `exclude_suggest.rs` / `ai_group.rs` 四个模块，**去掉 `//` 与 `/* */` 后再 grep** `sqlx|PgPool|INSERT|UPDATE|DELETE`：

```text
proposal.rs      : 0 hits（唯一的 12 个 "insert" 全是 serde_json::Map::insert / HashSet::insert）
name_suggest.rs  : 0 hits（5 个 insert 全是 Map / HashSet）
exclude_suggest.rs: 0 hits（16 个 insert 全是 Map / HashSet）
ai_group.rs      : 0 hits（12 个 insert 全是 Map / HashSet）
```

三条 suggest 命令的 I/O 只有「读 schema / 读快照 / 写文件」；`--ai` 读快照走的是 `Snapshot` 值对象（由 cli 注入），模块本身连 `db::` 都不 import。

### 4.3 真跑：把提案文件直接喂 `propose import`

```text
ontok propose import --file out/req118-real/cache-fresh/names.proposal.json        → exit 1  ✗ 提案缺 project
ontok propose import --file out/req118-real/cache-fresh-excl/excl.proposal.json    → exit 1  ✗ 提案缺 project
ontok propose import --file out/req118-real/groups.proposal.json                   → exit 1  ✗ 提案缺 project
```

`propose_cmd`（`src/cli.rs:2599-2603`）要求 `project` + `base_version` + `items[]`，提案文件只有 `proposals[]`，**物理上进不去**；不是靠「纪律上不许」。

### 4.4 ⚠️ 结构性的一个旁路：`#[derive(Deserialize)]`

`ProposalItem` 同时 `derive(Serialize, Deserialize)`（`proposal.rs:156`）。私有字段挡得住构造字面量，**挡不住反序列化**。我写了一个**不改产品代码**的探针（`tests/qa_probe_forge.rs`，跑完即删）：

```text
从 JSON { … "reviewed_by": "forged-reviewer" } 反序列化：
PROBE reviewed_by via deserialize = Some("forged-reviewer")
PROBE re-serialized = "forged-reviewer"        ← 签字被原样接受并可再序列化
```

⇒ 模块文档「外部模块**构造不出**带签字的条目」这句话**过强**。但：全仓**没有任何路径**把 `ProposalItem` 反序列化回来（`grep ProposalItem` 只有三个 suggest 的构造与渲染），所以**当前不可利用**。记为 N-5（建议 `#[serde(skip_deserializing)]` 或自定义 `Deserialize` 恒写 `None`）。

### 4.5 变异 M2：加 setter 看有没有测试盯

| 变异 | 结果 | 判定 |
| --- | --- | --- |
| **M2** 给 `ProposalItem` 加 `pub fn set_reviewed_by(&mut self, …)` | `lib 351 / integration 11 / provenance 3` **全绿** | ❌ **缺口**：没有测试钉「可写路径不存在」这一层（N-6） |
| **M2b** 让 `finalize_item` 真的写 `Some("qa-mutation")` | lib 1 红（`reviewed_by_is_always_null_on_the_production_path`）+ 集成 1 红（`…_with_null_reviewed_by`） | ✅ 输出层有牙 |

结论：**P2 是结构性的，不是纪律** —— 但「结构」只覆盖到「构造」与「输出」，没覆盖「字段永不可写」这一层（`Deserialize` + 未来随手加的 setter）。

---

## 5 · P3 缓存 + 负控 + 变异

### 5.1 真实 96 表：用 Dev 的缓存复现交付物（0 token、逐字节相同）

把 Dev 的 `.cache.json` 复制到 QA 目录，用**当前二进制**重跑（cache 命中不发请求）：

| 命令 | 第一次 | 第二次 | 与 Dev 交付文件比 |
| --- | --- | --- | --- |
| `name suggest`（96 表） | 0 token · 命中 **8/8** · sha `5d991434…4955` | 0 token · 8/8 · 同 sha | == Dev `cache-fresh/names.proposal.json` ✅ |
| `exclude suggest`（96 表） | 0 token · 命中 **10/10** · sha `004b2fdd…2a4d` | 0 token · 10/10 · 同 sha | == Dev `cache-fresh-excl/excl.proposal.json` ✅ |
| `concept suggest --ai`（84 实体） | 0 token · 命中 **1/1** · sha `5f67e13b…5cf2` | 0 token · 1/1 · 同 sha | == Dev `groups.proposal.json` ✅ |

> 这同时证明：**三份交付提案就是当前这份代码 + 当前这份提示词产出的**（prompt_sha / agent_version / 模型名任一变化都会 miss 缓存 ⇒ 不可能 0 token 还逐字节相同）。

### 5.2 真跑一次「冷缓存 → 热缓存」（真计费）

端点本轮偶发挂起（见 §11 观察 O-1），所以用**真实 schema 的子集**跑通同一条代码路径：

| 命令 | 冷缓存（第一次） | 热缓存（第二次） | 负控（删缓存再跑） |
| --- | --- | --- | --- |
| `name suggest`（真实 12 表） | **4463 token** · 0/1 命中 | **0 token** · 1/1 · sha 同为 `3d7d9a36…` | — |
| `name suggest`（真实 2 表） | **1459 token** | — | **1360 token** ✅ 不为 0 |
| `exclude suggest`（真实 2 表） | **1447 token** · sha `0c128389…` | **0 token** · 1/1 · 同 sha | **1452 token** ✅ 不为 0 |
| `concept suggest --ai`（真实 3 实体快照 `t_cg_redline_a_31016_1`） | **809 token**（无缓存即冷跑）· sha `5cd50699…` | **0 token** · 1/1 · 同 sha | — |

✅ 三条命令的「第二次 0 token + 逐字节相同」都成立；**负控**证明 0 不是统计写死。
✅ 冷跑两次的**输出不同**（name 2 表：`641f5ab1…` vs `4f33777a…`；exclude 2 表：`0c128389…` vs `e9b0e09f…`）—— 见 N-4。

### 5.3 变异 M3：把 `model` 从指纹里去掉

```text
变异 src/extract/proposal.rs：sha256(v1+key+input_digest+prompt_sha+model+agent_version) → 去掉 model
cargo test --lib proposal::tests
→ FAILED. 13 passed; 1 failed
   prompt_model_or_version_change_invalidates_cache
   assert_ne!(fingerprint(…,"m1"), fingerprint(…,"m2"))  ← 左右都成了同一个 sha
```

✅ 换模型会命中旧缓存的变异被测试抓住。还原后 blob == `HEAD:`（`63ac61e0…`）。

---

## 6 · 三条兼容性（真跑）

| AC | 链路 | 实测 |
| --- | --- | --- |
| **AC-002** | `name suggest --as-names` → `ontok-propose --names` | ✅ 96 张表全部进提案（`items=96`，`add_entity_type` 的 `display_name` 是中文名，不是英文 key），exit 0；`_proposal.sha256` 指向的提案文件真实存在且 sha 一致 |
| **AC-003** | `exclude suggest` → `ontok-propose --exclude`（过 REQ-113 导入校验） | ✅ `--exclude` 接受提案文件，`stats = {seen 96, excluded 21, extracted 75}`（闭合），被排除表不进 `items`，exit 0 |
| **AC-004** | `concept suggest --ai` → `concept import`（digest 一致） | ✅ 真导入 9 组；`ontok concept digest` 输出 `8e0217104b786e6d` == `history[-1].content_digest`，文案「契约 一致（可直接 import）」 |

---

## 7 · 预算闸（AC-006）

三条命令各 `--budget-tokens 1`（**全新缓存**，否则命中缓存不花钱、根本不会越预算）：

| 命令 | exit | 报告（原文） | 产物文件 | 缓存 |
| --- | --- | --- | --- | --- |
| `name suggest`（真实 schema 子集 4 表 / 2 批） | **1** | `已用 / 上限 / 停在第几项：951 / 1 token / 停在第 2 批（共 2 批）` | **不存在**（不是 0 字节） | 落盘（已花的钱不丢） |
| `exclude suggest`（同上） | **1** | `1035 / 1 token / 停在第 2 批（共 2 批）` | **不存在** | 落盘 |
| `concept suggest --ai`（真实 3 实体 / 1 批） | **1** | `812 / 1 token / 1 批已全部跑完，但累计用量越过了上限` | **不存在** | 落盘 |

✅ 「已用 / 上限 / 停在第几批」三件都在；⚠️ concept 只发一批，所以停点文案是「1 批已全部跑完」而不是「停在第 N 批」——判据仍成立。

---

## 8 · 降级标注（AC-007）

| 路径 | 真跑结果 |
| --- | --- |
| `concept suggest`（算法版，`anspire_grouped` v2） | `source = "rule"` · `declared_by = "concept-suggest/greedy-modularity-v1"` · `reviewed_by = null` · `confirmed_by = null` · 9 组 |
| `concept suggest --ai` | `source = "ai"` · `decided_by = "ai:qwen36-27B"` · `kind = "concept"` · `reviewed_by = null` |

**变异 M4**：把算法版构造处的 `source: Some(SOURCE_RULE…)` 改成 `Some("ai")`
→ `cargo test --lib concept_suggest` **FAILED**（`test_declaration_says_it_is_an_algorithm_suggestion`：`left: Some("ai") / right: Some("rule")`）。✅ 有牙。

---

## 9 · 真实数据与对比的诚实性

### 9.1 三份提案与 Dev 报告对账

| 提案 | 文件 | 条数（实测） | Dev 报告 | 结论 |
| --- | --- | --- | --- | --- |
| name | `out/req118-real/cache-fresh/names.proposal.json` | proposals **96**，`names` 96，coverage 96/96，丢弃 0 | 96 | ✅ |
| exclude | `out/req118-real/cache-fresh-excl/excl.proposal.json` | proposals **96**（逐表判定），排除 **21** | 96 判定 / 21 | ✅ |
| concept | `out/req118-real/groups.proposal.json` | **9 组 / 74 成员**，未分组 10 | 9 组 | ✅ |

（`out/req118-real/excl.proposal.json` 是**另一轮** 24 条的产物，见 N-3。）

### 9.2 `comparison.md` 是否逐条

| 段 | 覆盖 | 判定 |
| --- | --- | --- |
| **name** | 表头写「一致 9 · 名字不同 74 · 产品版新增 13（合计 96）」，但表里只有 **87 行** —— 缺的 9 行正是**名称完全相同的 9 张**（file_parser_task / instance_task_manager / internal_messages / llm_model_peak_config / llm_routing_rules / marketing_channel / package_benefits / package_templates / recharge_products） | ❌ **不完整**（N-2） |
| **exclude** | 21 行逐条（12 一致 + 9 产品版新增），新增 9 条逐条给了 confidence + 理由 | ✅ |
| **concept** | 对话版 8 组 → 产品版最佳匹配（Jaccard）+ 9 组逐组 claim/reason/members | ✅ |

### 9.3 9 条人工复核清单

✅ 存在：`.feature/req118-notes.md` §A-2（9 行：表 / 产品版类别 / 建议 / 依据），建议用「存疑 / 倾向排除 / 倾向保留」，**没有替代业务拍板**，与 BA「给建议但不预设答案」一致。

### 9.4 有没有偷偷把提示词调成贴合对话版 12 条？

**没有。** 三条独立证据：

1. **提示词里没有那 9 张表的名字**（也不含对话版 12 条的任何一张）：剥注释后 grep `usage_reconciliation_log` / `simulate_login_*` / `dim_customer_binding` / `instance_task_manager` / `file_parser_task` / `proxy_operation_records` / `licenses_archive` 在三份 suggest 模块里 **0 命中**。
   仓库里同名命中全在**既有文件**（`propose.rs` 的 REQ-084 登记、`confidence.rs` 的既有单测、`concept.rs` 的文档），`git diff develop..feature` 对这些文件无改动。
2. **缓存指纹没变**：三份交付提案能被当前二进制 **0 token 命中并逐字节复现**（§5.1）⇒ `prompt_sha` / `agent_version` / model 与产出时完全一致（改过提示词必然 miss）。
3. 提示词是**通用规则**（「业务人员日常会说」「⛔ 不许等于表名」「拿不准一律保留」），没有任何针对对话版结果的枚举或示例表名；唯一像例子的 `客户套餐订阅` 来自 **ADR-005 的提案形状示例**，不是对话版答案。

### 9.5 A-4（低置信）登记

- 三份产物 `<0.60` 的条数 = **0 / 0 / 0**，`low_confidence: true` = **0**（如实登记的口径成立）。
- 但 A-4 写的「name 最低 0.65；exclude 最低 0.70」与产物不符：实测 name 最低 **0.60**（`cache-fresh` 那份是 0.70）、exclude 最低 **0.60**（comparison 用的 21 条那份；顶层 24 条那份是 0.70）、concept 0.70。数值口径混乱，见 N-7。

### 9.6 AC-009 证据可核实性（独立实现复核）

我不复用被测的 `evidence_references_input`，用 Python 独立判「evidence 里是否出现表名 / 列名（词元边界）/ 注释连续 4 字窗口」：

```text
names  96 条 · 无落点 0
exclude 96 条 · 无落点 0
concept 74 个成员全部在 schema 里；proposals[] 9 条有 evidence/reason
```

⛔ 不可核实措辞（模型认为 / AI 认为 / 我觉得）：**0 条**。
（有 2 条 reason 里出现「推测 / 可能」——`llm_provider_back`、`internal_staff`。这是**诚实的不确定表达**，模块文档明确解释了为什么不收进黑名单，不算缺陷。）

---

## 10 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断（按建议优先级排序）

| # | 问题 | 证据 | 建议 |
| --- | --- | --- | --- |
| **N-1** | **失败批次 fail-open**：模型调用失败（超时）时，命令仍 **exit 0** 并落一份**可导入的空提案**（`groups: []` / `proposals: []`），concept 还会打印「声明契约 digest 一致（可直接 concept import）」 | `ONTOK_LLM_TIMEOUT_SECS=1` 冷缓存真跑：concept `exit=0 file_exists=True groups=0 proposals=0 rejected={"call_failed":1}`；name `exit=0 proposals=0 coverage.named=0 missing=4`。**并且与 A-5 的登记矛盾**：A-5 写「整份回答作废……该批记 truncated，**不产出文件**」，实测产出了文件 | 二选一：① 让 `failed_calls > 0` 像预算一样 `bail!`（不产文件、exit≠0）；② 至少 exit≠0 并在文件顶层写 `importable: false`；③ 若坚持现状，**改 A-5 的措辞**（它现在与行为不符） |
| **N-2** | `comparison.md` 的 **name 段缺 9 行**（表头说 96，表里 87）；缺的正是「一致 9」那 9 张 —— 评审人看不到哪 9 个名字是对上的 | §9.2；Python 集合差 = 那 9 个表 | 补 9 行（`一致`），或把表头改成「列 87 行（另 9 行一致见 `name-identical.tsv`）」 |
| **N-3** | 交付目录**混了多轮运行**：顶层 `excl.proposal.json` 是 **24 条**那轮；`comparison.md` / A-2 用的是 **21 条**那轮（`cache-fresh-excl/`）；`excl-product-only.tsv` 是 24 条那轮的 12 行（与 comparison 的 9 条不一致）；`name-diff.tsv` 只有 65 行，与交付的 names 提案对不上（11 个差异缺失、2 个其实相同的被列进去） | §5.2 的两次冷跑结果不同；集合差见 QA 分析 | 合并前用**一个空目录**重跑一遍并只保留那一轮的产物；或给每份产物加 `run_id` 并在 comparison 顶部点名用的是哪一份 |
| **N-4** | **没登记「无缓存时不复现」**：同一输入、清缓存重跑，排除判定 21 vs 24（13 张表翻转），2 表名字两次冷跑输出也不同。P3 的保证只在**有缓存**时成立 | `/tmp` 级别实测；①21 vs 24；②name `641f5ab1…` vs `4f33777a…`；③exclude `0c128389…` vs `e9b0e09f…` | 在 A-5/已知限制里加一条：「冷启动不保证同一结果（未设 temperature / 模型漂移）；可复现性由缓存承担」。否则「可复现」会被误读成「冷跑也一样」 |
| **N-5** | `ProposalItem` `derive(Deserialize)` ⇒ **任何模块都能伪造 `reviewed_by`**；模块文档「外部模块构造不出带签字的条目」过强 | 探针：`Some("forged-reviewer")` 被接受并可再序列化 | 给 `reviewed_by` 加 `#[serde(skip_deserializing)]`（或自定义 `Deserialize` 恒写 `None`）；文档改成「构造路径唯一，反序列化路径见 N-5」 |
| **N-6** | **没有测试盯「字段不可写」这一层**：加 `pub fn set_reviewed_by` 后 351+11+3 **全绿**（只有真写值才红） | 变异 M2 / M2b | 加一条结构判据（源码里 grep 不到 `fn set_reviewed_by`，或用一个 `compile_fail` doctest） |
| **N-7** | A-4 的**最低置信度数字与产物不符**：写的是 name 0.65 / exclude 0.70，实测 name **0.60**（`cache-fresh` 0.70）/ exclude **0.60**（comparison 用的 21 条那份；24 条那份 0.70） | §9.5 | 改成「交付这一轮」的真实最小值，并注明是哪份文件 |

### 观察（不判缺陷，但要登记）

| # | 观察 |
| --- | --- |
| **O-1** | 真实 LLM 端点本轮**偶发挂起**：3 个进程在 `timeout_global=150s` 配置下挂了 **18–28 分钟**仍未返回（`Established` 到 `111.231.18.234:443`）。84 实体的 concept 单请求在 120s 超时（A-5 已登记这条限制）。另有一次同样的 3 实体请求 13s 就成功（809 token）。⇒ 与 REQ-118 代码无关（传输层是 REQ-034 的 `HttpTransport`），但**实测必须避开大请求 + 并发**；`timeout_global` 似乎没有覆盖 body 读取，建议单独开单在传输层确认 |
| **O-2** | `tests/test_req118_proposals.rs::proposal_sources_have_no_database_write_path` 只覆盖 3 个文件（`proposal` / `name_suggest` / `exclude_suggest`），**漏了 `ai_group.rs`**。我手工剥注释 grep 过 `ai_group.rs` 也是 0 命中，所以事实成立，只是判据少一格 |

---

## 11 · 不退化

### 11.1 数字（独立跑出来的）

| 项 | 结果 |
| --- | --- |
| lib | **351 passed / 0 failed**（含 DB 用例；不带 `ONTOK_TEST_DATABASE_URL` 时 349 passed / 2 failed —— 2 条是缺环境变量） |
| doc | **4 passed / 0 failed** |
| 新增集成 | `test_req118_proposals` **11 passed**；`test_req118_provenance_inputs` **3 passed**（11 + 3 = 14） |
| 出口登记 | `test_req107_source_enumeration` **10 passed**（新增 8 个 fn + 4 个 module 登记齐全） |
| 旧红灯 | 4 个，全部**自己复现并归因**（下表） |

### 11.2 四个既有红灯（与本需求无关）

| 用例 | 实测 | 归因 |
| --- | --- | --- |
| `drop_rate::real_schema_drop_rate_report` | FAILED：`读不到脱敏 schema.json：../.review-workspace/fix-props/schema.json` | 该文件被 gitignore、本机没有（fail-closed 门禁） |
| `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | FAILED：同上（报错里还有个 `/home/myg133/Projects/...` 的 Linux fallback） | 同一份脱敏 schema 缺失 |
| `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | FAILED：实际 `items` 少一条 `add_link_type 设备→客户` | REQ-108 分档路由把低置信关系挪进 `proposal-pending.json`（REQ-112/113 已登记），本次改动不碰 `propose` 路径 |
| `test_retained_names_req084` | **编译失败**：`couldn't read ../src/bin/ontok-gate-preflight.rs` | Windows 绕行把该文件挪开的必然结果（既有口径） |

### 11.3 禁改面 / 零依赖

```text
git diff --stat 5f6636f..0573fa8 -- src/review_ui/http.rs src/server/http.rs src/mcp/tools.rs src/domain/property_filter.rs Cargo.toml Cargo.lock
→ （空）
git diff --diff-filter=D --name-only 5f6636f..0573fa8      → （空：没有删测试/删文件）
git diff 5f6636f..0573fa8 | grep '^+' | grep '#\[ignore'  → 0
```

改动面 14 个文件、+5682/−35：`cli.rs` / `ontok-propose.rs` / `extract/{proposal,name_suggest,exclude_suggest,ai_group,mod}.rs` / `domain/{concept,concept_suggest}.rs` / 3 个测试 / 1 个夹具 / `.feature/req118-notes.md`。

### 11.4 绕行双证（还原）

```text
moved : sha=c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d  exists=False
RESTORE: hold_sha = c7cf95e2…571322d
         file_sha = c7cf95e2…571322d   match=True   Test-Path=True   bytes=134877
feature worktree: git status --short → （空）   HEAD = 0573fa8
变异 worktree: 三个被变异的源文件逐个 `git checkout --` 后 `git hash-object` == `git rev-parse HEAD:…`
                （proposal.rs 63ac61e0… / concept_suggest.rs 413b906e… / ontok-propose.rs f2699de9…）；
                gate-preflight 还原后 sha 与 feature 侧一致（c7cf95e2…571322d），
                该 worktree 已 git worktree remove；`git worktree list` 只剩 5 个正式 worktree
```

> 变异全在独立 worktree（独立 `target/`）里做；feature 的 `target/` 没有接触过变异源码，本轮 `ontok.exe` 的构建时间（12:39:20）晚于所有源文件，且 `--help`/三条命令的行为与本轮源码一致。

---

## 12 · 变异汇总（有牙 / 缺口）

| # | 变异 | 位置 | 结果 | 判定 |
| --- | --- | --- | --- | --- |
| M1 | 删掉 `inputs.names` 写入 | `src/bin/ontok-propose.rs` | 2 红（文件层 + 库层） | ✅ 有牙 |
| M2 | 给 `reviewed_by` 加公开 setter | `src/extract/proposal.rs` | 351 + 11 + 3 **全绿** | ❌ **缺口**（N-6） |
| M2b | `finalize_item` 真写 `Some(...)` | 同上 | lib 1 红 + 集成 1 红 | ✅ 输出层有牙 |
| M3 | 指纹里去掉 `model` | 同上 | lib 1 红（`prompt_model_or_version_change_invalidates_cache`） | ✅ 有牙 |
| M4 | 算法版 `source` 写 `ai` | `src/domain/concept_suggest.rs` | lib 1 红（`test_declaration_says_it_is_an_algorithm_suggestion`） | ✅ 有牙 |
| P-0 | `Deserialize` 伪造 `reviewed_by` | 探针（不改产品） | 伪造成功 | ⚠️ 结构旁路（N-5），当前无消费路径 |

---

## 13 · QA 在共享库留下的数据（供 BA 知情）

| 项目 | 用途 | 留下 |
| --- | --- | --- |
| `qa_req118_p4` | P4 names 入库 | 1 条 proposal（96 items，`inputs.names`） |
| `qa_req118_p4b` | P4 names+exclude 入库 | 1 条 proposal（75 items，`inputs.names` + `inputs.exclusions`） |
| `qa_req118_c` | P4 concepts 入库 | 1 条 proposal（9 组，`inputs.concepts`） |

只读使用：`anspire_grouped` v1/v2/v3、`t_cg_redline_a_31016_1`。未删除、未改动任何既有项目的行。

---

## 14 · 复现（关键命令）

```powershell
# 环境
$env:ONTOK_LLM_BASE_URL='https://ollama.apps.anspire.cn/v1'; $env:ONTOK_LLM_API_KEY='<key>'; $env:ONTOK_LLM_MODEL='qwen36-27B'
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'

# P4（names）
ontok-propose --schema ..\code\out\schema.json --names out\req118-real\cache-fresh\table_names.as-names.json `
              --out out\qa-req118\p4-names.proposal.json --project qa_req118_p4 --no-sample
ontok propose import --file out\qa-req118\p4-names.proposal.json
# 读回：.local/qa/dbcheck.py inputs qa_req118_p4   （与 Get-FileHash 对比）

# P3（缓存命中 + 负控）
ontok name suggest --schema <schema12.json> --out out\qa-req118\p3\names12.proposal.json --cache out\qa-req118\p3\names12.cache.json
# 再跑一次（0 token）；删 .cache.json 再跑（token≠0）

# 预算闸
ontok name suggest --schema tests\fixtures\req118\schema.json --out <不存在的路径> --budget-tokens 1
```

**脚本**：`.local/qa/{dbcheck.py, evidence.py, run3.ps1, run2.ps1}`；**日志**：`.local/qa/{mut-*, feature-redlights.log}`；**QA 产物**：`feature-REQ-118/out/qa-req118/`。
