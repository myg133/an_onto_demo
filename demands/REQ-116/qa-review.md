# QA 审核报告：REQ-116 · 文档摄取与断言抽取（Pre-merge）

> **被测**：worktree `feature-REQ-116` / 分支 `feature/REQ-116` / 1 个提交 `31838ef`（基线 `develop` `a85383a`）· **未 push**
> **结论**：**PASS（带非阻断）** —— 无阻断项；6 条非阻断（1 条文档、2 条测试层次、1 条 AC 判据自身缺陷、2 条边界/注释不符）
> **日期**：2026-10-22 · Windows · 真库 `postgresql://***@172.17.0.87:5432/anspire_real` · 真模型 `qwen36-27B`
> **审核方式**：独立复验（不 import 被测代码、不读它的块清单）+ 主动攻击（3 种变异 + 3 种自造攻击）
> **临时 worktree**：`.review-workspace/req116-mutant`（变异）、`.review-workspace/req116-baseline`（基线归因），审查完已 `git worktree remove`
> **脚手架**：`.review-workspace/req116-qa/`（python/ps1 脚本，未入库）

---

## 0 · 结论先行

| # | 审核项 | 判据 | 结果 |
| --- | --- | --- | --- |
| 1 | 证据逐字节校验（核心） | 101 条 `span`/`quote` 按 **UTF-8 字节**复核 | ✅ **101/101 通过**（负控 2/2 被抓） |
| 2 | 变异 M1 伪造 quote | 必须丢弃 + 计数 | ✅ 0 条进 JSONL、`no_evidence=42`、文件 0 字节 |
| 3 | 变异 M2 前缀匹配 | 测试必须红 | ✅ **红**（lib 4 条，含 `prefix_only_match_is_not_accepted`）；⚠️ 集成层没抓住 |
| 4 | 变异 M3 删证据校验 | 测试必须红 | ✅ **红**（lib 1 条 + 集成 2 条） |
| 5 | 确定性·分块 | 两次 `--chunks-only` sha256 相同 | ✅ `8DAD2AC0…` 两次相同；CRLF/LF 同 sha |
| 6 | 确定性·抽取 | 第二次 0 token + JSONL 逐字节同 | ✅ run2→run3 **0 token / 14 命中 / sha `A654162F…` 相同** |
| 7 | 格式拒绝（含目录混入） | 3 种格式 exit≠0 + 点名 + 提示；混入当场报错 | ✅ 3/3；混入 ⇒ exit 1 且零产物 |
| 8 | 预算闸 | exit≠0 + 报三个数 + **不产出** | ✅ exit 1 / `1003 / 1 / 停在第 2 块` / 文件不存在 |
| 9 | 不写库 | 结构性 + 实测 | ✅ 剥注释后 0 处 `sqlx`/`PgPool`/`db::`；12 张表 12480 行前后逐字节相同 |
| 10 | 出口登记 | `I10` 内部轨 / 总数 45 / 不接客户视图 | ✅ 45（内 15）；`I10` 内部轨；客户/服务出口 0 命中 |
| 11 | 不退化 | lib 314 / doc 4 / 新增 8 | ✅ 314/314、4/4、8/8；禁改面 diff 为空 |
| 12 | 5 个失败归因 | 独立复现 | ✅ 3 个在 `a85383a` 逐条复现；2 个是 Windows 绕行/既有编译问题 |
| 13 | 语料诚实性 | 是否过度贴合提示词 | ⚠️ **过度贴合**（判断见 §9）——AC-004 的真实验证仍未做 |

**没有任何一条 AC 被我判不达标；但 §10 的 6 条非阻断请一并处理，尤其是 N-1（`.feature/` 还是 REQ-115 的报告）。**

---

## 1 · 证据校验独立复核（核心）

### 1.1 方法（刻意不共用被测代码）

自写 `verify_evidence.py`（`.review-workspace/req116-qa/`），**只吃源文档 + 断言行**：

1. `[System.IO.File]::ReadAllBytes` 等价物 → 原始字节 → 剥 BOM → `CRLF/CR → LF`；
2. 从 `chunk_id` 解析 `sha256(file)[:12]` 与 `<start>-<end>`，**自己算文件 sha256 对账**；
3. 用 `span` 直接在**归一化后的文档字节**上切片；
4. 与 `quote` 的 UTF-8 字节做 `==`（⛔ 全程不用 UTF-16 索引、不用 `Substring`）。

> ⚠️ Dev 说的「PowerShell `Substring` 是 UTF-16 索引」我确认过一次：先用字符索引跑，
> 101 条里 100 条假 MISMATCH；换成字节口径后 101/101 通过。这条坑是真的。

### 1.2 结果（Dev 的 `out/req116/live/assertions.jsonl`，101 条）

```text
总数 = 101   逐字节通过 = 101   失败 = 0
按文件：01-order-flow.md 30 · 02-subscription.txt 16 · 03-points.md 27 · 04-data-dictionary.md 28
模态分布：{example: 1, fact: 86, future: 3, spec: 11}
alignable 与 modality 冲突条数 = 0
关系 49 条 · belongs_to = 0
```

### 1.3 负控（证明我的判据不是恒绿）

把第 0 条 `quote` 末字改成「虚」、第 1 条 `span` 右端 +1 ⇒

```text
总数 = 101   逐字节通过 = 99   失败 = 2
  ✗ #0 … span 切出的字节 != quote 字节
  ✗ #1 … span 切出的字节 != quote 字节
EXIT=1
```

⇒ 我的复核器**有牙**；上面的 101/101 不是假绿。

---

## 2 · 三种变异（每种各一次，每次逐字节还原）

### 2.1 M1 · 模型编造原文没有的句子（**不改编码，攻击真实二进制**）

自造假端点（`fake-llm.js`，Node 标准库，独立于仓里测试的那个），两种模式：

| 模式 | 回答内容 | 期望 | 实测 |
| --- | --- | --- | --- |
| `forged` | 2 concept + 1 relation + 1 `belongs_to`，**全部**用原文没有的句子当 quote | 一条都不留 | **exit 0 · 断言 0 条 · JSONL 0 字节** · `no_evidence=42(=3×14)` · `predicate=14` |
| `mixed` | 真 quote 1 条 + 编造 1 条 + **「原文前缀 + 原文没有的尾巴」** 1 条 | 只留真的 | **28 条保留 / 28 条丢弃**（14 编造 + 14 前缀+尾巴）· `belongs_to` 0 |

```text
✗ 报告里点名：证据对不上：8728a8f5783c:0-118：quote 在该块原文里找不到 —— 「客户用现金购买了一辆汽车…」
             证据对不上：…：「# 电商订单流程\n\n本文说明平台的订单主这条尾巴原文里绝对没有」
```

⇒ AC-003「丢弃且计数」在**真二进制 + 真 HTTP 路径**上成立；且「前缀+尾巴」当场被拒（这是 M2 的行为级对照）。

### 2.2 M2 · `locate_quote` 改成前缀匹配

在独立 worktree 里把 `locate_quote` 换成「quote 的最长前缀在原文里出现就算命中」：

```text
sha256(原文件) = 4AC1B544…A473
sha256(变异后) = 0099E095…5655
cargo test --lib  →  FAILED. 310 passed; 4 failed
  红：doc_ingest::tests::prefix_only_match_is_not_accepted      ← 正面靶子
  红：doc_ingest::tests::forged_quote_is_rejected
  红：doc_ingest::tests::forged_quotes_are_dropped_and_counted
  红：doc_ingest::tests::whitespace_differences_are_tolerated
cargo test --test test_req116_doc_ingest → ok. 8 passed; 0 failed   ← ⚠️ 集成层没抓住（见 N-4）
sha256(还原后) = 4AC1B544…A473   逐字节还原 = True   git status 变更 = 0
```

### 2.3 M3 · 把证据校验整段删掉（`check()` 直接放行）

```text
sha256(变异后) = D89EBA50…5CD9
cargo test --lib → FAILED. 313 passed; 1 failed
  红：doc_ingest::tests::forged_quotes_are_dropped_and_counted
cargo test --test test_req116_doc_ingest → FAILED. 6 passed; 2 failed
  红：ac003_forged_quote_is_dropped_not_fixed
  红：ac003_ac004_ac005_evidence_spans_are_verbatim
sha256(还原后) = 4AC1B544…A473   逐字节还原 = True   git status 变更 = 0
```

**三元组全被抓。** 另：Dispatcher 的纪律 H（「证据校验只许一处实现」）成立 ——
`locate_quote` 是唯一判据，`validate`/`check` 只调它一次，缓存回放也走同一条路。

---

## 3 · 确定性双证

### 3.1 分块（AC-002）

```text
两次 --chunks-only：8DAD2AC02B47E3DA3CECB3EC03417F32C8FF53DFDF09CFDDF5C6626AD0C5CEA2
                    8DAD2AC02B47E3DA3CECB3EC03417F32C8FF53DFDF09CFDDF5C6626AD0C5CEA2
chunks = 14（4 文件）
```

额外：把同一份 `01-order-flow.md` 写成 **CRLF** 再跑 ⇒ 块清单 sha256 与 LF 版**完全相同**
（`0D4E11BF…`），证明「块 id 不随平台漂移」不只是注释。

块规模：字符数 min 35 / 中位 84 / max **451** —— 全部远低于 `MAX_CHUNK_CHARS=1200`，
说明语料**从未触达字符上限边界**（见 §9）。

### 3.2 抽取（AC-006）

真模型、冷缓存、同一 `--out`（缓存落在 `<out>.cache.json`）：

| 轮次 | 断言 | calls | cache_hits | tokens_spent | JSONL sha256 |
| --- | --- | --- | --- | --- | --- |
| run1（冷） | 94 | 14 | 0 | 14414 | `AE8AE5F7…` |
| run2 | 101 | 1 | 13 | 1002 | `A654162F…` |
| run3 | 101 | **0** | **14** | **0** | `A654162F…` ✅ 与 run2 逐字节相同 |

> ⚠️ run1 有一条 `调用失败：… timeout: global`（网络抖动）⇒ 该块**正确地没有进缓存**，
> 所以 run2 仍需真调 1 次。**这不是确定性缺陷，是 fail-closed 生效**：残缺的回答不许冒充答案。
> 完全命中后（run3）0 token + 逐字节相同，AC-006 成立。

**`stats.json` 逐字节相同不是要求**，实测也确实不同（`tokens_spent` / `cache_hits` 如实变化），
`assertions.jsonl` 才是逐字节判据 —— 与 BA 提醒一致，未误判。

### 3.3 指纹口径（独立复算，不 import 代码）

自己抠 `SYSTEM_PROMPT` 原文、自己算 `sha256`：

```text
prompt 994 字符 · sha256 = a484fdf8…f5c5fc
缓存里的 model = qwen36-27B · prompt_sha 与源码一致
用 v1\n{chunk}\n{prompt_sha}\n{model}\ndoc-ingest-1\n 复算：命中 14 / 落空 0
四项（块文本/提示词/模型/逻辑版本）任一更换 ⇒ 指纹变且不命中：OK
```

⇒ 与 demand 的 R3 口径（以及 REQ-109 的构成）一致；缓存换成**本地文件**是 BA 已裁定的有意取舍。

### 3.4 攻击：缓存只省调用，不省校验

把缓存里某一条 `response` 换成「引文原文里没有」的回答，再跑（端点故意指向不可达地址）：

```text
选中块 ffb9c95490f4:0-129：原本产出 1 条断言
exit = 0 · cache_hits=14 calls=0 · 该块残留断言 0 条 · 幽灵概念进 JSONL 0 条
总数 101 -> 100（正好少 1）· rejected.no_evidence 3
⇒ PASS（缓存不省校验）
```

---

## 4 · 格式拒绝（AC-001）

| 场景 | exit | 点名 | 提示先转文本 | 产物 |
| --- | --- | --- | --- | --- |
| `sample.pdf` | 1 | `.pdf` | ✅ `请先把它转成 .md/.txt 再跑（转换后请记下 hash）` + `pandoc …` | 无 |
| `sample.docx` | 1 | `.docx` | ✅ | 无 |
| `sample.doc` | 1 | `.doc` | ✅ | 无 |
| **目录里混入** `01-order-flow.md` + `sample.pdf` | **1** | `.pdf` | ✅ | 无（断言/缓存都没有） |

三种都带「⛔ 不会静默跳过 —— 跳过等于装作处理过（AC-001）」。
目录混入是**当场报错**，不是跳过；且发生在写任何产物之前。

⚠️ 边界：**隐藏文件/隐藏目录会被静默跳过**（见 N-5），且实测与源码注释不符。

---

## 5 · 预算闸（AC-007）

```text
$ ontok doc ingest … --budget-tokens 1
exit = 1
✗ 预算超限：已用 1003 / 上限 1 token，停在第 2 块（共 14 块）
✗ doc ingest 预算超限 —— 本轮**不产出任何断言文件**（AC-007：不产出半份 JSONL）。
已用 / 上限 / 停在第几块：1003 / 1 / 停在第 2 块（共 14 块）。

assertions 存在=False   stats 存在=False   cache 存在=True   cache 条目数=1
```

- 三个数**都报**：已用 / 上限 / 停在第几块 ✅；
- **确认文件真的不存在**（`Test-Path = False`），不是 0 字节占位 ✅；
- 已花掉的那一次回答**落了缓存**（放宽预算重跑直接命中）—— 这是设计，不是漏；
- 语义诚实：闸是「不再发起下一发」，不承诺不超（注释与实测一致：1003 > 1）。

另一处实测（前一版）：首次调用遇到一次网络失败时，闸会**跨过那一块**继续到第 3 块才停 ——
`tokens_spent` 只记真实用量，失败调用不虚拟计费。行为自洽。

---

## 6 · 不写库（结构性，不是纪律）

### 6.1 结构性

自写 `strip_check.py` 剥注释（`//` / 嵌套 `/* */` / 字符串字面量）后扫 `src/extract/doc_ingest.rs`：

```text
原文 66222 字符 / 剥注释后 52904 字符
'sqlx' 0 · 'PgPool' 0 · 'db::' 0 · 'pool.' 0 · 'connect(' 0 · 'DATABASE_URL' 0
```

`cli::doc_cmd` 函数体（`src/cli.rs:1146-1362`）同样 0 处。
分派位置（`src/cli.rs`）：

```text
1057: Cmd::Doc { cmd } => return doc_cmd(cmd).await,      ← 在连库之前
1061: let url = crate::config::require_env(DATABASE_URL_ENV)
1063: let pool: PgPool = crate::db::pool::connect(&url).await?
```

⇒ 不是「Dev 记得不写库」，是**结构上够不到**（连 `PgPool` 类型都不在本模块的依赖面上）。

### 6.2 实测

本轮**所有** `doc ingest` 调用都显式清掉 `ONTOK_DATABASE_URL` / `ONTOK_TEST_DATABASE_URL`，
全部 exit 0。真库整库前后指纹（`db_fingerprint.py`，**12 张表全算**，不只 AC 点名的四张）：

```text
action_types 649 / ai_cache 82 / concept_groups 440 / extraction_agents 210 /
extraction_rounds 40 / functions 550 / link_types 878 / object_types 2299 /
ontology_versions 2096 / projects 1682 / proposal_items 2714 / proposals 840
合计 12480 行 ⇒ 跑后逐表 rows + md5 **完全一致**，结论 = 一致（不写库）
```

> 说明：本库没有叫 `concepts` 的表（概念在 `object_types` 里），所以我按
> `information_schema` 枚举**全部表**，「一行都不许变」的口径比 AC-008 更严。

---

## 7 · 出口登记（AC-009）

| 判据 | 实测 |
| --- | --- |
| `OUTLETS` 里有 `I10` | ✅ `src/domain/pipeline.rs:1350-1367`，`path = src/cli.rs`、`call_site = "std::fs::write(path, bytes)"` |
| 轨道 | ✅ `Enforcement::InternalOnly` / `Track::Internal` ⇒ `gated() == false`（不过客户发布闸） |
| 总数 | ✅ **45**（客户 17 / 建模 10 / **内部 15** / 静态 2 / 错误路径 1），`registry_is_the_declared_baseline` 逐项精确相等 |
| 写点唯一 | ✅ `src/cli.rs:1370` 的 `write_local` 是 `doc ingest` 三份产物（JSONL / stats / 缓存）的唯一 `fs::write` |
| **没接到客户视图** | ✅ `src/server/http.rs` / `src/review_ui/http.rs` / `src/mcp/tools.rs` / `src/domain/view.rs` 对 `doc_ingest`、`assertions.jsonl` **各 0 命中**；全 `src/` 里只有 `cli.rs` 与 `extract/mod.rs` 提到它 |

配套改动（`tests/test_req107_outlet_pipeline.rs` +1 条、`tests/test_req107_source_enumeration.rs` +2 个函数/模块登记）
都是**把新增写点登记上去**，不是放宽：`want` 集合 `I1..I9` → `I1..I10`，其余逐项未动。

---

## 8 · 不退化 + 5 个失败的独立归因

### 8.1 数字（我自己跑的，不是抄报告）

```text
lib         314 passed / 0 failed      ← 与要求的 314 一致（REQ-115 是 291，+23）
doc-tests     4 passed / 0 failed      ← 4
集成         76 个目标 · 813 passed / 4 failed
新增 tests/test_req116_doc_ingest.rs   8 passed / 0 failed   ← 「新增 8」一致
合计         82 个 test result 行 · 1141 passed / 4 failed
```

绕行方式（**两侧都动、finally 挪回，sha256 + Test-Path 双证**）：

```text
src/bin/ontok-gate-preflight.rs     C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
                                    跑前 = 跑后；134877 字节；Test-Path True→False→True；git status D 行 = 0
tests/test_retained_names_req084.rs 2DB7D22C…4CF9 跑前 = 跑后
```

> 为什么要挪**两个**：只挪 bin ⇒ `test_retained_names_req084.rs` 的 `include_str!` 编不过（整个构建失败）；
> 两个都挪 ⇒ 上面那 76 个目标才跑得起来。代价见 §8.2 第 4/5 条。

### 8.2 5 个失败逐条归因

| # | 失败 | 我的复现方式 | 归因 | 与本轮的关系 |
| --- | --- | --- | --- | --- |
| 1 | `drop_rate::real_schema_drop_rate_report` | 在 `a85383a` 的干净 worktree 里跑 | `读不到脱敏 schema.json：../.review-workspace/fix-props/schema.json`（该文件被 gitignore，不在版本控制内，门禁规定读不到必须失败） | ✅ **基线复现，非本轮** |
| 2 | `test_fk_naming::test_real_schema_customer_id_has_exactly_one_display_name` | 同上 | 同一份脱敏 schema 缺失 | ✅ **基线复现，非本轮** |
| 3 | `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 同上 | `left` 少一条 `add_link_type`（REQ-108 分档路由带来的既有差异，REQ-112/113 已登记） | ✅ **基线复现，非本轮** |
| 4 | `test_req107_red_lines::test_req107_red_lines_and_guard_are_untouched` | 全量跑 | **绕行产物**：红线 ①②③ 三组断言**全部通过**后才挂在 `:159`「⛔ 删除了测试文件 tests/test_retained_names_req084.rs」。而 Windows 上只要 bin 在位就编不过（见 5），所以这条在本机**任何配置下都不可能全绿** | ✅ 环境/绕行，非本轮 |
| 5 | `test_retained_names_req084`（**编不过**） | 单独跑 ⇒ `cannot find unix in os` / `no method ino` | Windows 编不了 unix-only 的 `ontok-gate-preflight.rs`，而它 `include_str!` 那个 bin。`git show develop:` 证明**同样 unix-only**、且**本轮一个字节都没改**这两个文件 | ✅ 环境，非本轮 |

### 8.3 特别验：`test_retained_names_req084` 的「登记表在 develop 上就过期」

自己实现它的判据（第 `LINE` 行第 `COL` **字节**列上是否恰好是 `NAME`，且左右词边界成立），
对 `git show develop:<file>` 逐行核对：

```text
登记行数 = 199（测试要求 199）✅
develop 上对不上的行 = 136
  src/extract/propose.rs      130 处（全部）
  tests/test_snapshot_write_paths.rs  4 处
  tests/test_mcp_server.rs            2 处
每文件登记数 vs 测试里手写的 FLOOR_FILE：13 个文件全部相等 ✅（不是「有人偷偷删登记行」）
```

⇒ **Dev 的说法成立**：`propose.rs` 的 130 行登记坐标在 `develop` 上就已经对不上（该文件在 REQ-084 之后被改过，登记没跟着更新）。

- 而且可以**更强地**下结论：本轮改动的 7 个源文件 + 3 个测试文件里，**没有一个**在这张登记表的
  13 个被登记文件之内（`git diff --name-only develop...HEAD` 与登记表取交集 = ∅），
  所以这条测试的判定结果**与 develop 逐位相同**，不可能是本轮改坏的。
- ⚠️ 顺带纠正 Dev 报告的说法：不能写成「`test_retained_names` 因绕行未编译 ⇒ 不算失败」，
  它**本身在 develop 上就是红的**（登记过期），两件事都要说。

### 8.4 禁改面

```text
src/server/http.rs            diff 为空 ✅
src/mcp/tools.rs              diff 为空 ✅
src/domain/property_filter.rs diff 为空 ✅（G1–G4 三段逐字节未变，红线断言实测通过）
Cargo.toml / Cargo.lock       diff 为空 ✅（零新依赖）
红线四文件 tests/drop_rate.rs · tests/test_fk_naming.rs · src/domain/view.rs · src/domain/review_view.rs  diff 为空 ✅
删测试 / #[ignore] / 放宽断言：git diff --diff-filter=D 为空；新增 #<ignore> 0 处；唯一「删掉的断言行」是
  outlet_count(Internal) 14 → 15（登记表口径更新，见 §7）
```

---

## 9 · 语料的诚实性判断（AC-011 / AC-004）

### 9.1 事实

`tests/fixtures/req116/docs/` 是 **Dev 自造**的中文小样本（4 份 / 14 块 / 约 4.3 KB），
测试文件头也如实写了「仓里没有业务文档」。**这一点是诚实的**。

### 9.2 但**过度贴合提示词** —— 我的判断：语料是照着抽取结果/提示词写的

| 证据 | 说明 |
| --- | --- |
| `04-data-dictionary.md` 的「术语对照」一节，四个 bullet 恰好是 **下单 / 扣费 / 退款 / 订阅** | 这正是 `SYSTEM_PROMPT` 规则 2 里列举的**同一个示例集**，字面重合 |
| `03-points.md` 有 `## 未来规划`：平台**计划**…平台**还将**…比例**待定**…**目前尚未实现** | 逐字复刻规则 4「带『将』『计划』『未来』『待』的通常是 future」 |
| `02-subscription.txt` 有「**示例**：用户 A 在 3 月 1 日订阅…」 | 复刻「带『例如』『示例』的通常是 example」 |
| `01-order-flow.md` 首段：「本文说明…以及客户、订单、支付单**三者之间的关系**」 | 把要抽的概念**先列给人看**，等于把答案写在题目里 |
| 每份都自带 `| 对象 | 关键字段 | 说明 |` / `| 字段 | 类型 | 业务含义 |` 表格 | 结构提示过于友好（真实业务文档不长这样） |
| 块字符数 max **451**，从未触达 `MAX_CHUNK_CHARS=1200` | 分块的「字符上限边界」「超长段落不切句」在真实语料上**从未被触发**，只有单测覆盖 |
| 句子普遍是「主体 + 动词 + 客体」的单句 | 谓词自然是动词 ⇒ `belongs_to 0%` 几乎是必然结果 |

### 9.3 结论

1. **AC-004 的真实验证仍未做**（需业务文档）—— 这一句必须写进结论，report 里我照写；
2. 这套语料只能当**冒烟夹具**，不能当「中文业务文档抽取质量」的证据；
3. 另外 AC-004 的判据本身有缺陷（见 N-2）：`belongs_to` 在计数**之前**就被谓词闸拒掉，
   `stats.predicate_counts` 里永远不会出现它 ⇒ 打印出来的「belongs_to 占比」**结构性恒为 0.0%**，
   `< 20%` 恒真。真正该盯的是「属于 / 归属于 / 有 / 针对 / 对应」这类中文结构词（本轮实测各出现 1–3 次），
   而它们**不进任何闸**。

---

## 10 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项（建议合并前处理前 2 条）

| # | 项 | 证据 | 建议 |
| --- | --- | --- | --- |
| **N-1** | **`.feature/` 三份文档还是 REQ-115 的**：`verification-report.md` 标题是「REQ-115 概念分组…第 2 轮」、`manifest.json` 的 `req_id = REQ-115`、`status.md` 也是 REQ-115。**本轮没有任何验证报告**。Dev 报告里「802 passed / 75 目标 / 3 failed」其实是 REQ-115 的数字（我实测本轮集成 813/4、82 个 result 行） | `git diff --name-only develop...HEAD` 不含 `.feature/*`；`head verification-report.md` | 合并前补一份 REQ-116 报告（或至少把三份 `.feature/*` 更新）；否则「101 条断言已复核」这句话在仓库里**没有书面依据**（复核是我做的，结果成立） |
| **N-2** | **AC-004 判据恒真**：`belongs_to` 在 `validate` 里先被 `is_structural_predicate` 拒掉，再 `continue`，`predicate_counts` 只统计**通过**的关系 ⇒ 占比永远 0.0%，`< 20%` 不可能不达标 | `src/extract/doc_ingest.rs:977-1004` | 修订 AC：分母改成「模型**提出**的关系」再算占比；或明确「该指标只用于统计词形分布」。这是 AC 自身缺陷（纪律 S），不是实现缺陷 |
| **N-3** | **语料过度贴合提示词**，AC-004 的真实验证未做（§9） | 同上 | 拿一份真实业务文档跑一遍再看谓词分布；本轮不判质量 |
| **N-4** | **M2（前缀匹配）只有单测抓得住，集成层漏网**：变异后 `test_req116_doc_ingest` 仍 8 passed。因为它的假端点只回「完全无关的句子」，没有「原文前缀 + 编造尾巴」的用例 | §2.2 | 把「前缀+尾巴」加进 `tests/test_req116_doc_ingest.rs` 的 fake 端点（我自造的 `mixed` 模式已证明该用例很好写，且能一次抓住 M1+M2） |
| **N-5** | **隐藏文件/目录被静默跳过，且与源码注释不符**：`walk_docs` 里 `if name.starts_with('.') { continue; }`（无 stderr），而函数注释写「隐藏文件 / 隐藏目录…**跳过并在 stderr 说明**」。实测：目录里放 `.dotfile.pdf` 与 `.secret/hidden.pdf`，命令 **exit 0、一个字都不报**、只处理了 `ok.md` | `src/extract/doc_ingest.rs:342-353`；实测见下 | 要么按注释补一行 stderr（「跳过 N 个隐藏项」），要么对隐藏路径里的不支持格式也报错。AC-001「⛔ 不许静默跳过」在这个角上是 fail-open |
| **N-6** | 边角瑕疵：`.PDF`（大写）报错里点名的是 `.pdf`（扩展名被 `to_ascii_lowercase` 了），路径正确但格式名与实际文件名不完全一致 | `extension_of` | 无伤大雅，可顺手带上原样扩展名 |

> N-5 的两次实测输出：
> ```text
> 目录 = { ok.md, .dotfile.pdf, .secret/hidden.pdf }
> ✓ 只分块（--chunks-only）：1 个文件 → 3 块        exit=0   （两个 .pdf 一声不吭）
> ```

---

## 11 · 复现清单（scratch 脚本，未入库）

| 脚本 | 作用 |
| --- | --- |
| `.review-workspace/req116-qa/verify_evidence.py` | 按 UTF-8 字节复核 span/quote（含负控） |
| `…/fingerprint_check.py` | 独立复算指纹四要素 |
| `…/cache_attack.py` | 往缓存里塞伪造回答，验证「缓存不省校验」 |
| `…/fake-llm.js` + `forged-run.ps1` | 自造假端点跑 M1（forged / mixed） |
| `…/mutate.py` + `run-mutations2.ps1` | 独立 worktree 里打 M2 / M3，跑测、逐字节还原 |
| `…/cli-gates.ps1` | 格式闸 / 目录混入 / 分块确定性 / 预算闸 |
| `…/full-test.ps1` | 全量测试 + 双绕行（sha256 + Test-Path + 还原证明） |
| `…/baseline-tests.ps1` | 在 `a85383a` 上复现 3 条既有红灯 |
| `…/retained_check.py` | 按字节列核对 199 行登记表 vs `git show develop:` |
| `…/db_fingerprint.py` | 12 张表行数 + 内容 md5 前后对比 |

**未改 feature 分支、未 push、未提交任何东西**；两个临时 worktree 已 `git worktree remove`；
`feature-REQ-116` 的 `git status` 为空，HEAD 仍 `31838ef`。
