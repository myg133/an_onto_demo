# QA 审核报告 · REQ-147（抢救版 · C 方案）

> 审核对象：`feature-REQ-147c` / `feature/REQ-147-salvage` / HEAD `0961e70`（1 提交，7 files，+2212/−13）
> 基线：`1137ea8`（develop，`git archive` 独立解包，**自编自测**，未引用 Dev 的任何数字）
> 环境：Windows / rustc 1.96.0 / cargo 1.96.0 / node v22.22.2 / PostgreSQL **未配置**（无 `ONTOK_TEST_DATABASE_URL`）
> 报告人：QA（独立复验，全部数字为本轮实测）

---

## 一 · 结论

**PASS 带非阻断。**

| 维度 | 结论 |
| --- | --- |
| 默认路径零回归（R1 / AC-001） | ✅ **产物逐字节相同**（3 种场景，含两种"有失败"的坏场景） |
| ACP 通路可用（AC-002/003/005） | ✅ 10/10 e2e 通过；自跑一次拿到身份行 / 契约指纹 / `discarded_no_quote` |
| 失败要响（R3 / AC-004） | ✅ 3 个参数用例 + 连不上 + 全块失败：全部 exit=1、零断言产物、**无回落分支**（假内置端点数到 0 次请求） |
| 二进制隔离 | ✅ 服务面 `ontok` 干净（`acp:` = 0、`X-TC-Action` = 0、`doc` 子命令不存在） |
| 护栏登记（T5/T6） | ✅ **只加了两条名字**，无删改判据（diff 原文见 §六） |
| 全量三列 | ✅ HEAD `1797/183/3` vs 基线 `1775/183/3`，**失败用例名集合完全相同（183 = 183，逐名相等）**；+22 通过 = doc_acp 单测 12 + 新 e2e 10 |
| skill 价值证据（AC-006） | ⚠️ **无有效证据**（同模型对照没带过来，见 §八）—— 按你的裁定不阻塞合并 |

**阻断项 0 条**，但有 **1 条判据缺口必须由 BA 裁定**（R-13 的"回答不可用率 > 20% 硬闸"没实现，见 §九）。若你裁定它属于本次交付范围，本报告结论应升级为 **FAIL**；我给出可复现的判定命令，一句话说清。

---

## 二 · P0-1 · 默认路径逐字节零回归（最重要）

### 2.1 方法（自己搭的，没抄 Dev）

- 固定输入：自造 3 篇 `.md`（`a.md` A0D83554… / `b.md` 82200B38… / `c.md` 25A4592A…，sha256 前 16 位）
- 假 OpenAI 端点：**自写 Node HTTP 服务**（`.local/qa147/fake_llm.js`），从请求体里取回块原文，用**原文前 24 字**当 `quote` 回一份确定性抽取（3 概念 + 1 关系）。同一份输入 ⇒ 同一份回答 ⇒ 产物可对拍。
- 两个二进制：基线 `git archive 1137ea8` 自编 `ontok-builder.exe`；HEAD 自编 `ontok-builder.exe`。**都不给 `--agent`**，同 env（`ONTOK_LLM_BASE_URL/API_KEY/MODEL`）、同输入目录、各自独立 `--out/--stats/--cache`（冷缓存）。
- 对拍：`assertions.jsonl` 与 `stats.json` 的 sha256。`stats.json` 里含 `input` 路径 / `model` / `prompt_sha`，两边一致才可能相等 —— 相等即证明指纹口径没漂。

### 2.2 三条场景（结果：全部逐字节相同）

| 场景 | 构造 | HEAD 产物 sha256 | 基线 = HEAD？ |
| --- | --- | --- | --- |
| **clean** | 全部成功 | `a.jsonl` **9A190D14…C6F92**<br>`s.json` **3E0F07CD…0469C** | ✅ 相同（连 `c.json` 缓存都相同 `BFE0C2E5…3580`） |
| **parsefail** | 每第 2 次回非 JSON（`rejected.parse=1`） | `a.jsonl` **F3F1B17C…D816D**<br>`s.json` **B09AADEC…72958** | ✅ 相同 |
| **dropfail** | 每第 2 次直接断连（`failed_calls=1`） | `a.jsonl` **F3F1B17C…D816D**<br>`s.json` **4F37BA9C…49FEB** | ✅ 相同 |

> 关键点：**不只是"一切顺利"时相同**。两种"有失败"的坏场景下，产物依旧逐字节相同 —— 说明新增的提示逻辑只动屏幕、不动数据。

### 2.3 Dev 登记的那条差异：stderr 多两行，可接受吗？——**可接受，理由如下**

| 场景 | 基线 stderr | HEAD stderr | 差异 |
| --- | --- | --- | --- |
| clean | 16 行 | 16 行 | **0 行**（除我给的输出路径不同） |
| parsefail | 16 行 | 18 行 | +2 行（`⚠ 本轮 1 块回答不可用（共 3 块，33%）…` + 出路提示） |
| dropfail | 16 行 | 20 行 | +4 行（失败块 2 行 + 回答不可用 2 行） |

**我的独立判断（不是顺着你说）：**

1. **R1 的范围是"产物"，stderr 不是产物。** AC-001 的判据是 `assertions.jsonl` / `stats.json` 的 sha256；本命令的屏幕信息本来就全走 stderr（我 ACP 那次单独抓了 `stdout.txt`，**0 字节**；默认路径两次运行把 stderr 落盘后 stdout 也没有任何内容），是给人看的诊断流。
2. **这两行是 R-2/A 明确要求的**（"屏幕上必须报"），而且**只在已经出问题时出现**（`failed_calls>0` / `rejected_parse>0`）。一个"全绿"的运行，stderr 与基线**逐行相同**（我实测 16 = 16）。
3. **它不改变任何机器可读契约**：退出码不变（0），产物 sha 不变，缓存 sha 不变。真要说风险，只剩"有下游脚本按 stderr 行数/行号解析"这一种 —— 我查了本仓测试，**没有任何测试把 `doc ingest` 的 stderr 钉成逐字节**；`test_req116_doc_ingest` 等只断言子串。
4. **反过来说，不加这两行才是缺陷**：`rejected_parse` 的块**不进** `failed_calls`，29/95 回答作废时屏幕上会"一切正常"——这正是你们踩过的坑。

⇒ **判：产物范围零回归成立；stderr 的两行属于"该有的告警"，可接受。** 唯一建议（非阻断）：把这两行写成**单行**，别写成两行，可少一类"按行数解析"的误伤。

---

## 三 · P0-2 · ACP 通路真的跑通了

### 3.1 `test_req147_doc_acp`（node v22.22.2 在用）

```
running 10 tests
test strict_deterministic_flag_is_documented ... ok
test ac001_without_agent_still_goes_to_builtin_llm ... ok
test ac004a_missing_agent_program_exits_nonzero_without_output ... ok
test ac004c_budget_exceeded_exits_nonzero_without_output ... ok
test ac005_forged_quote_from_acp_agent_is_dropped_and_counted ... ok
test r2c_strict_deterministic_refuses_unreplayable_output ... ok
test unusable_answers_are_announced_with_their_rate ... ok
test r2a_failed_blocks_are_announced_on_screen ... ok
test ac002_ac003_acp_ingest_is_schema_identical_and_byte_stable ... ok
test ac004b_agent_timeout_exits_nonzero_without_output ... ok

test result: ok. 10 passed; 0 failed; 0 ignored; finished in 1.75s
```

**不是空跑**：测试确实起了真进程 —— `out/req147/*/calls.log` 有内容（`ac002_003` 2 条 prompt、`r2a`/`r2c`/`parse_rate` 各 3 条），产物 `a.jsonl` / `s.json` / `c.json` 都在。若 `node` 缺失，`require_node()` 会打印 SKIP 而**不假装通过**（本机有 node，所以 10 条都真跑了）。

### 3.2 我自己那次 `--agent acp` 运行（`FAKE_ACP_MODE=valid`，3 篇文档 / 3 块）

```
· 抽取实现：acp（node …\tests\fixtures\req147\fake-acp.js）—— 握手身份 fake-acp v0.1.0
✓ 文档摄取
  输入        : …\.local\qa147\corpus（3 个文件 / 3 块）
  断言        : 6 条（概念 3 · 关系 3）
  ⚠ 丢弃      : 0 条（证据对不上 0 · 模态 0 · 谓词 0 · 字段空 0 · 解析 0）
  抽取实现    : acp（node …\fake-acp.js）
  agent 身份  : fake-acp v0.1.0（**握手报的**，不是命令行里写的那个字符串）
  契约(skill) : …\tests\fixtures\req147\skill-fixture.md（sha256 9cc1e6347e2c…）
  预热        : 已发 1 次 · 单轮超时 180s（会话首轮的对端前言由预热消耗掉）
  quote 校验  : 通过 6 / 6 = 100.0%（丢弃 discarded_no_quote = 0）
  缓存        : 命中 0 / 3 块（实际调用 3 次 · 强制失败 0 次）
  token       : 本轮 5000 / 上限 1000000（回放历史 0）· 输出上限 由对端决定
```

**三样都在**：① ACP 身份行（握手名+版本，且明说"不是命令行里写的那个字符串"）② 契约指纹（skill 路径 + sha256 前 12 位）③ `discarded_no_quote` 计数。

再跑一次 **mixed 模式**（每块回一条原文里没有的 quote）验证计数器会动、且重跑确定性：

```
  断言        : 3 条（概念 3 · 关系 0）
  ⚠ 丢弃      : 3 条（证据对不上 3 · …）
  quote 校验  : 通过 3 / 6 = 50.0%（丢弃 discarded_no_quote = 3）
── 第二次重跑（同一缓存）
  预热        : 未发（本轮 0 次调用） · 单轮超时 180s
  缓存        : 命中 3 / 3 块（实际调用 0 次 · 强制失败 0 次）
第一次 sha=43738D46…20D0   第二次 sha=43738D46…20D0   逐字节相同=True
第二次 calls.log 行数 = 0
```

⇒ AC-003（两次 sha 相同 + 第二次 0 次调用，含预热都不发）与 AC-005（编造 quote 被丢并计数）在我这边**独立复现**。

---

## 四 · P0-3 · 失败要响（R3）

### 4.1 三个构造（每个都 exit≠0 且**零产物**）

| 用例 | exit | 断言文件 | stats | cache | 屏幕点名 |
| --- | --- | --- | --- | --- | --- |
| `--agent acp`（缺 `--agent-command`） | **1** | ❌ 无 | ❌ 无 | ❌ 无 | `✗ --agent acp 必须同时给 --agent-command（例如 "pi-acp"）。` |
| `--agent acp --agent-command node …`（缺 `--agent-skill`） | **1** | ❌ 无 | ❌ 无 | ❌ 无 | `✗ --agent acp 必须同时给 --agent-skill（抽取契约的路径…）。` |
| `--agent bogus` | **1** | ❌ 无 | ❌ 无 | ❌ 无 | `✗ --agent "bogus" 没有对应实现 —— 已实现的是：acp。` |

### 4.2 `agent 连不上` + `全部块失败`（并且证明**没有回落**）

**连不上**（`--agent-command definitely-not-a-real-acp-program-req147`，**同时把内置 LLM 指到一个可用的假端点**）：

```
· 未给 --budget-tokens，用默认上限 500000（超过即停，且不产出断言文件）
✗ 找不到 ACP 可执行程序 "definitely-not-a-real-acp-program-req147" —— 在 PATH 里找过：…
⛔ 没有回落到任何别的实现（回落 = 溯源造假，见 agent::factory 模块头）。
c4 exit=1    jsonl存在=False  cache存在=False
**假内置端点本轮请求数 = 0**（服务端每条请求都会打日志；跑完读日志：无新行）
```

> 判据为什么有效：内置那侧**确实可用**。若存在静默回退，它会成功、会出产物、会留下请求日志。三条断言同时指向"没回退"。

**全部块失败**（`FAKE_ACP_MODE=hang` + `--agent-timeout-secs 1`）：

```
⚠ 本轮 3 块失败（共 3 块）：重跑会**重调这 3 块** ⇒ 产物可能变化
⚠ 本轮 3 块**回答不可用**（共 3 块，100%）：…这些块的断言**全部丢失**
✗ ACP 通道**每一次调用都失败了**（3 / 3 块）—— 本轮**不产出任何断言文件**。
c5 exit=1    jsonl存在=False  stats存在=False  cache存在=True（35 B，空缓存）
```

> `cache` 有文件**不是**缺陷：代码刻意"先落缓存、再判失败"（已花的钱留盘，放宽条件重跑不重复计费），而 `assertions.jsonl` / `stats.json` 一个字节都不写 —— 符合 AC-004 的"不产出半份"。

### 4.3 代码里有回落分支吗？——**没有**

| 检查 | 结果 |
| --- | --- |
| `build_doc_transport()` 里 `--agent acp` 那一支 | 全程 `?` 上抛；`connect()` 失败即 `bail!`，**没有 `unwrap_or` / `or_else` 到内置** |
| `doc_acp.rs` 是否引入内置 HTTP 传输 | 只 `use crate::agent::llm::{LlmTransport, protocol::{…}}`（trait + 协议类型），**没有** `HttpTransport` / `LlmSettings` 的实例化 |
| `ingest_all()`（REQ-116 唯一编排） | 传输错误只累计 `failed_calls` / `rejected_parse`，**没有任何第二次尝试换通道** |
| 错误文案 | 连不上 / 握手失败两条都**主动声明**"⛔ 没有回落到内置 LLM"，与代码行为一致（不是只写了句好话） |

---

## 五 · P0-4 · 二进制隔离

`T=D:\MyCodes\AI\an_onto_demo\.local\t147c\release`，`strings -n 4`（**没有**拿 `discarded_no_quote` 当标记）：

| # | 命令 | 期望 | 实测 |
| --- | --- | --- | --- |
| 1 | `strings -n 4 $T/ontok-builder.exe \| grep -c 'acp:'` | > 0 | **1** |
| 2 | `strings -n 4 $T/ontok.exe \| grep -c 'acp:'` | = 0 | **0** |
| 3 | `strings -n 4 $T/ontok.exe \| grep -c 'X-TC-Action'` | = 0 | **0** |
| 4 | `$T/ontok.exe doc ingest --help` | unrecognized subcommand | `error: unrecognized subcommand 'doc'` / exit **2** |

补充对照（顺手做的，用于证明"不是本来就干净"）：

- 基线 `ontok-builder.exe` 的 `acp:` = **0** ⇒ HEAD 的 1 是本次新增，且**只新增在构建面**。
- `ontok-builder.exe` 的 `X-TC-Action` = **4**，**基线也是 4** ⇒ 是 REQ-152 遗留，不是本 REQ 引入（本 REQ 只要求服务面为 0，已满足）。
- `agent-skill` 在二进制里：基线 0 → HEAD 7。
- 顺带纠一条你给的提醒：`discarded_no_quote` **在 builder 二进制里能搜到 2 次**（它出现在 `quote_pass_line()` 的运行时格式串里，不只在注释/测试）。所以它确实不能当"ACP 是否进二进制"的标记 —— 但原因和你记的不完全一样，标记应该用 `acp:`。

---

## 六 · P0-5 · 护栏登记（T5/T6）没放宽断言

`git diff 1137ea8..HEAD -- tests/test_req107_source_enumeration.rs` **原文**（只有 2 个 hunk，+8 行，−0 行）：

```diff
@@ -1758,6 +1758,12 @@ const OUTLET_FNS: &[(&str, &str, &str)] = &[
         "doc_ingest_cmd",
         "子命令 handler（`doc ingest`：文档 → 带证据的断言；**不连产品库**，产物/缓存全在本地）",
     ),
+    // ⭐ REQ-147 · 「这次用哪个抽取实现」的**唯一**装配点（内置 LLM / ACP 子进程）。
+    (
+        "src/cli.rs",
+        "build_doc_transport",
+        "运维辅助（REQ-147：`--agent` 的岔口 —— 默认内置 LLM 原路；`acp` 起子进程并握手）",
+    ),
     // ⭐ REQ-129 · `doc merge`：断言 → 提案的机械合并。写出口 I26 登记在 OUTLETS。
     (
         "src/cli.rs",
@@ -2053,6 +2059,8 @@ const MODULES: &[(&str, &str)] = &[
     ("src/extract/mod.rs", "exclusions"),
     // ⭐ REQ-116 · 文档摄取与断言抽取（格式闸 / 分块 / 证据校验 / 指纹缓存 / 预算）
     ("src/extract/mod.rs", "doc_ingest"),
+    // ⭐ REQ-147 · `doc ingest --agent acp`：同一套编排的另一个搬运工（不另立判据）。
+    ("src/extract/mod.rs", "doc_acp"),
     // ⭐ REQ-118 · AI 提案框架（ADR-005：提案形状 / 校验 / 指纹缓存 / 预算 / 渲染；三命令共用）
     ("src/extract/mod.rs", "proposal"),
```

**判**：只加了 `build_doc_transport`（OUTLET_FNS）与 `doc_acp`（MODULES）**两条名字**；没有删条目、没有改判据、没有把某条断言改成 `assert!(true)`。整个提交也没有删任何测试文件（`git show --stat` 7 files，唯一动的测试文件就是它，+8/−0）。

---

## 七 · P0-6 · 全量三列 + 逐条归因

### 7.1 三列

| 侧 | commit | passed | failed | ignored | EXIT |
| --- | --- | --- | --- | --- | --- |
| **HEAD** | `0961e70` | **1797** | **183** | **3** | 101 |
| **基线** | `1137ea8`（`git archive` 解包自编，target `E:\t147base`） | **1775** | **183** | **3** | 101 |

- **失败用例名集合逐名相等**（`Compare-Object` 双向为空：只在 HEAD 失败 0 条，只在基线失败 0 条）。
- 通过数 +22 的来源**算得清**：`doc_acp` 单测 **+12**（`src/lib.rs` 833 vs 821）+ 新 e2e **+10**（`tests/test_req147_doc_acp`，基线无此 target）。**没有任何一条旧测试从绿变红。**
- 忽略的 3 条（两侧同）：`todo_identifier_defenses_have_own_assertions` / `child_writes_twenty_audit_lines` / `req149_shape_evidence_with_derived_real_mapping`。

### 7.2 183 条逐条归因（按根因聚合成 4 类）

| 类 | 条数 | 证据 | 侧 |
| --- | --- | --- | --- |
| **缺环境**：没有 `ONTOK_TEST_DATABASE_URL` | **178** | 167 条 `tests/common/mod.rs:233` 的 `缺 ONTOK_TEST_DATABASE_URL —— 口令不得入库…`；10 条 `MissingEnv("ONTOK_TEST_DATABASE_U…")`；1 条同因变体 | HEAD = 基线 |
| **缺夹具（生成物）**：脱敏 `schema.json` 不在仓里 | **2** | `读不到脱敏 schema.json：…/.review-workspace/fix-props/schema.json`、`/home/myg133/Projects/…`（绝对路径，来自作者机器） | HEAD = 基线 |
| **基线同红（业务红线测试）** | **2** | `test_req084_every_registered_name_is_still_literally_at_its_line`（登记行号漂移）、`test_req084_no_unregistered_real_name_remains_in_the_registered_files`（`src/domain/concept.rs` 实 9 处 vs 登记 5 处） | HEAD = 基线 |
| **基线同红（字节对照）** | **1** | `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`：`items` 的 `evidence` 多了字段（REQ-059 之后的既有偏差） | HEAD = 基线 |

> **没有一条归因为"代码问题"**：183 条在基线上一模一样地红，且都是环境 / 夹具 / 既有偏差。这也回答了那个常见陷阱 —— **不是"基线比 HEAD 红得多"，而是"两边一样红"**。我特意查过基线是否跑全：见 §7.3。

### 7.3 基线是怎么跑全的（两个坑，都记下来）

1. **D: 盘满**：第一轮基线 `cargo test` 在链接阶段炸了 —— `error: could not exec the linker link.exe … 磁盘空间不足 (os error 112)`，且**日志里 0 条 `test result`**（如果只看"失败 target 数"，会误判成"基线红 0 条"！）。**处理**：删掉 `D:\…\.local\t147base` 腾空间，基线改用**另一个短路径** `E:\t147base`（12 字符，比原来的还短，不触 MSVC D8050）重跑；HEAD 仍用你指定的 `D:\…\.local\t147c`。
2. **"target 名重名"**：Cargo 输出里 `src\lib.rs` 与 `src\main.rs` 等会并列出现，按名字聚合容易把 110 个 suite 数成 109 个 target。我按"唯一 target 名"复核后再做差集，才有 `833 vs 821` 这个可解释的差额。

> 所以本轮的基线是**真跑全**的：109 个 target、1775 通过、183 失败、3 忽略、exactly 与 HEAD 的失败集相同。

---

## 八 · P0-7 · "skill 到底值多少"的证据状态：**无 / 无有效**

| 检查 | 结果 |
| --- | --- |
| 抢救分支带证据了吗 | **没有**。`git show --stat HEAD` 只有 7 个代码文件（cli / doc_acp / mod / 2 夹具 / 2 测试），**零报告、零对比表、零数据文件** |
| 同模型对照的实现痕迹 | `grep set_config_option` 在 `src/` + `tests/` = **0 命中**（BA 登记的"未开始"属实） |
| BA 侧裁定 | `acceptance.md` §R-3 要求同模型对照；§R-7 已接受"**未能分离**"并规定结论**永久降级**为「ACP + MiniMax-M2.7 优于 内置 + deepseek-v4.1-flash」 |

**结论：该结论仍无有效证据。** 原始那组数字（断言 809→2352、泛动词 18.65%→1.31%、关系两端是概念 26.2%→96.4%）以及 §R-16 的 765 vs 252 那一组，**都混了模型变量**（ACP 侧 MiniMax-M2.7 / 内置侧 deepseek-v4.1-flash），⛔ 不能拿来当"skill 更优"的结论；按你的裁定，只能写成"ACP+MiniMax 优于 内置+deepseek"。

按你的话，这一条**不阻塞合并**（与代码质量无关）；但它是本需求**存在的理由**，所以我在结论里保留它，避免下一轮有人把旧数字再引一遍。

---

## 九 · 非阻断发现（按严重度）

### N-1（需你裁定）· R-13 的"回答不可用率 > 20% ⇒ 硬闸"没实现

`acceptance.md` §R-13 第 3 条要求：`--strict-deterministic` 下，`回答不可用率 > 20%` ⇒ **exit≠0 且不产出产物**（与失败块同等待遇）。

**代码现状**（`src/cli.rs:2464`）：只判 `stats.failed_calls > 0`；`rejected_parse` 只触发**屏幕告警**（⑦.45），**不进闸**。屏幕提示语甚至明确写着"把这一类算成失败请用 `--strict-deterministic` **之外的闸**"——等于在代码里承认了这条闸不存在。

**我构造了它点名要拦的那种"脏绿"并实测**（3 块里 1 块回答不可用 = 33% > 20%）：

```
$ ontok-builder doc ingest --input corpus --out a.jsonl --stats s.json --cache c.json --strict-deterministic
strict+parse-fail exit=0
jsonl存在=True  stats存在=True
chunks=3 rejected.parse=1 failed_calls=0
⚠ 本轮 1 块回答不可用（共 3 块，33%）…
✓ 文档摄取
  断言        : 8 条（概念 6 · 关系 2）
```

⇒ **R-13 期望 exit≠0 + 零产物，实际 exit=0 + 有产物。** 如果这个判据算本次交付范围，这就是一条实打实的验收不通过（而且它是"确定性 ≠ 质量"的唯一守门人：那 33% 的回答已进缓存，重跑逐字节相同，所以 `--strict-deterministic` 的确定性承诺**成立**，但质量仍是脏的）。

补法很小（3~6 行 + 1 条 e2e）：在 ⑦.6 的 `if` 里加 `|| stats.rejected_parse * 5 > chunks.len()`（> 20%），并复用同一段 bail 文案。**建议**：要么本轮补，要么由你在 acceptance 里明确"本条留给下一轮"。

### N-2（观察）· `--strict-deterministic` 的 help 与实现口径要一起说

help 写的是"本轮有块失败 ⇒ exit≠0 且不产出断言文件"，**没有**承诺覆盖"回答不可用"。所以它**没有对用户撒谎**，只是比 R-13 少一档。若你决定不补 N-1，建议把 help 改成"只覆盖传输失败；回答不可用请见屏幕告警"，避免下一轮又被当成"承诺了没做到"。

### N-3（观察）· 两条告警各占 2 行

见 §2.3 结论。功能正确、内容正确，只是行数偏多；若下游有"按行数解析 stderr"的习惯，合并成 1 行更稳。**不影响本次结论。**

### N-4（观察，非本 REQ 引入）· 基线本身已有 3 条业务红线/字节测试红

`test_req084_*`（2 条）与 `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`（1 条）在 `1137ea8` 上就红。与本 REQ 无关，但会一直稀释"全绿"信号，建议另开一条清理需求。

---

## 十 · 我认为最可能被下一轮打回的一点

**N-1（R-13 硬闸缺失）。** 理由：

1. 它是**成文判据**（acceptance §五 R-13 第 3 条），不是口味问题；我能用一条命令把它变红：`--strict-deterministic` + 33% 回答不可用 ⇒ 现在 exit 0 且有产物。
2. 它正好守着本需求最容易被误读的地方 —— **"确定性"与"质量"是两件事**。Dev 的实现在"确定性"上是诚实的（失败块不进缓存、有产物即可重放），但在"质量"上仍会产出"31% 回答作废却全绿"的产物；R-13 就是为这个写的。
3. 下一轮审的人只要跑一次 `--strict-deterministic` + `halfgarbage` 夹具，就会看到 exit 0 —— 而 `unusable_answers_are_announced_with_their_rate` 这条测试**只断言了屏幕**，没有任何测试锁住 exit≠0，所以它不会被现有测试网拦住。

**次可能**：有人拿 §R-16 / status.md 的旧数字（765 vs 252 等）继续讲"skill 更优" —— 那组数字混了模型变量（§八），一旦被引用就是第二次翻车。建议在 status.md 里给那组数字加一句"不得单独引用"。

---

## 十一 · 复现附录

| 项 | 位置 |
| --- | --- |
| 假 OpenAI 端点（自写，支持 `parse`/`drop` 失败注入 + `/reset`） | `.local/qa147/fake_llm.js` |
| 固定语料（3 篇，sha 见 §2.1） | `.local/qa147/corpus/` |
| 零回归产物（3 场景 × 2 侧，含 stderr） | `.local/qa147/out/{clean,parsefail,dropfail}/{base,head}/` |
| ACP 自跑产物（valid / mixed / 重跑） | `.local/qa147/out/acp-valid/`、`acp-mixed/` |
| 失败用例产物（应为空） | `.local/qa147/out/fail/{c1…c5}/` |
| 全量测试日志 + 183 条失败名 | `.local/qa147/test-head.log`、`test-base2.log`、`head-failed.txt` |
| HEAD target（你指定的短路径） | `D:\MyCodes\AI\an_onto_demo\.local\t147c` |
| 基线 target（同短路径口径，因 D: 满改用 E:） | `E:\t147base`（源码：`.local\base147`，`git archive 1137ea8`） |

**纪律自检**：本文所有数字均为本轮实测；基线为 `1137ea8` 自编自测；没有引用 Dev 的任何数字；没跑的项目已显式标注（例如 AC-002 的"`doc merge` 端到端吃下 JSONL"本轮**未跑**——不在你给的 P0 清单内，AC-002 只用 10 条 e2e 的 schema 断言覆盖，若需要我补跑请说）。
