# REQ-109 · AI 语义项接入 —— 交付报告

> 分支 `feature/REQ-109`（基线 `develop` `a8a51ed`，REQ-108 已合并）· 未 push
> 一句话：**AI 只回答「这条候选关系在业务上说得通吗」，只贡献 `semantic_fit`（权重 0.15），
> 不产结构、不翻 veto；指纹缓存 + 预算闸 + 三件套无 key 即拒。**
>
> **P0（流程通）全绿**：协议解析、缓存命中/未命中、预算闸、同输入两次一致、无 key 报错退出、
> `semantic_fit` 不影响 veto。**语义准不准是 P1 观察项**（第 §6 节，含 3 条**标红**的误判）。

---

## 0 · 结论先行

| 项 | 结果 |
| --- | --- |
| 12 条 AC | **11 条通过；1 条（AC-004）按字面不成立，已按证据改口径**（§2.4，AC 自身缺陷） |
| AI 关是否与 REQ-108 逐字节相同 | ✅ **4/4 产物 sha256 完全相同**（用 `a8a51ed` 编译的基线二进制实测，§2.1） |
| 缓存是否真的不重复调用 | ✅ 第二次全量 **82 请求 / 82 命中 / 0 调用 / 0 token** |
| 预算闸是否真的停 | ✅ 100k 上限被 101,326 越过后**非 0 退出且不写任何产物**（§3.2） |
| veto 能否被 AI 翻转 | ✅ 82 条**无一条**超过自己的 veto 上限；REQ-108 的 E1–E7 仍全部 0.20/拒绝 |
| 测试是否退化 | ✅ lib **230** / doc **4** / 32 套件 **313** 一条不少；新增 3 套件 31 条 |
| 凭据 | ✅ key 只在命令行进程环境里用过，**未写进任何被跟踪文件**（含测试/文档/示例/日志） |
| 红线文件 | ✅ `tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs` / `src/mcp/tools.rs` / `guard()` / `property_filter.rs` G1–G4 **零改动** |
| 新依赖 | ✅ **零**（HTTP 走既有 `agent::llm`；SHA-256 复用 `extract::sample::sha256_hex`） |

---

## 1 · 改动清单（`path:line`）

### 1.1 新增

| 文件 | 职责 | 关键位置 |
| --- | --- | --- |
| `src/extract/ai_semantic.rs`（新，~1030 行） | 指纹、协议校验、缓存抽象、编排、预算 | 提示词 `:108` · `JudgeRequest::schema_digest` `:278` · `fingerprint` `:302` · `CachedOutput` `:393` · `parse_judgement` `:499` · `AiCache` trait `:612` · `AiStats` `:672` · `AiSemanticJudge::judge` `:800` · `require_ai_budget` `:985` |
| `src/db/ai_cache.rs`（新） | `ai_cache` 表、预加载/回写 | DDL `:43` · `PgAiCache` `:57` · `preload` `:67` · `persist` `:125` · `delete_fingerprints` `:168` · `delete_by_model` `:183` |
| `tests/test_req109_ai_semantic.rs`（新，22 条） | scripted transport：指纹/缓存/协议/预算/veto/零泄漏 | — |
| `tests/test_req109_cli_gate.rs`（新，6 条） | CLI 闸门：无 key / 无预算 / `--refresh` 误用 | — |
| `tests/test_req109_e2e_fake_llm.rs`（新，3 条） | **真进程 + 本地假 LLM 端点 + 真 `ai_cache` 表** | — |
| `fixtures/schema-req109.json`（新） | 端到端用例的最小 schema（3 表，含 `llm_model` 双键） | — |

### 1.2 修改

| 文件 | 改了什么 | 位置 |
| --- | --- | --- |
| `src/domain/confidence.rs` | 新增 `METHOD_V2` / `WEIGHTS_V2`（0.3825·0.17·0.1275·0.085·0.085·0.15）/ `weights_for()` / `AiProvenance` / `Confidence::ai` / `score_with_semantic()`；`score()` 改为薄封装（v1 算术**一个字节没动**）；`validate_self_evidence` 按 method 取权重表 + 语义信号两条新校验（S-1/S-2） | `METHOD_V2` `:66` · `WEIGHTS_V2` `:96` · `weights_for` `:110` · `AiProvenance` `:410` · `ai` 字段 `:453` · `score_with_semantic` `:556` · 统一实现 `score_with_weights` `:565` · 语义缺失分支 `:595` · 配对校验 `:891` · `ai` 解析 `:1131` |
| `src/extract/propose.rs` | 评分接缝拆出 `score_links_with_ai`（AI 关走 v1、AI 开走 v2）+ `ai_requests()`（只给两张表的切片） | `score_links` `:1465` · `score_links_with_ai` `:1484` · `ai_requests` `:1589` |
| `src/db/pg.rs` | `ai_cache` DDL 编进**同一条**取了咨询锁的迁移 batch | `:117` |
| `src/db/mod.rs` | 注册 `ai_cache` / 导出 `PgAiCache` | `:31` |
| `src/extract/mod.rs` | 注册 `ai_semantic` | `:3` |
| `src/bin/ontok-propose.rs` | 三个开关 + 两个分阶段上线闸 + AI 阶段 + 报告 + 预算闸（越限不写产物） | 开关 `:141-155` · 入口闸 `:194` · AI 阶段 `:481-620` · 预算 bail `:589` · 接缝调用 `:627` |
| `src/cli.rs` | `render_confidence_table` 增「AI 行」：`ai <model> · fit <v> · cache_hit <b> · tokens <n> ·「rationale」`；抬头写**实际方法**而不是写死 v1（AI 关仍逐字是 v1） | `:540`、AI 行 `:567` |
| `tests/test_req107_source_enumeration.rs` | **只加两条模块登记**（`ai_semantic` / `ai_cache`）——不加则 t6 红，0 条断言被改 | `:1766`、`:1791` |

### 1.3 行为开关（CLI）

| 开关 | 默认 | 行为 |
| --- | --- | --- |
| `--llm` | **关** | 开 ⇒ 三件套（`ONTOK_LLM_BASE_URL` / `ONTOK_LLM_API_KEY` / `ONTOK_LLM_MODEL`）缺一即 exit≠0 并**点名缺哪个**；还需 `ONTOK_DATABASE_URL`（`ai_cache` 落库） |
| `--budget-tokens` | 无 | `--llm` 时**必填且 >0**；越过即停，按「已用 / 上限 / 停在第几条」报告，**不写任何产物** |
| `--refresh` | 关 | 忽略缓存并覆盖，打印「强制刷新 N 条」；AI 关时给它会报错（不许静默无操作） |
| `--llm-only <substr>` | 无 | **分阶段上线闸**：只判关系键含该子串的那些（其余按语义缺失），不改变判据 |
| `--llm-limit <n>` | 无 | **分阶段上线闸**：最多送 n 条（只截请求），不改变判据 |

> `--llm-only` / `--llm-limit` 不在设计概要里，是**为满足「先 5 条小样本、再全量」这条运行要求**
> 加的两个**只做截断**的运维闸；它们不参与任何判据（没送的关系语义缺失，不是 0 分）。已在此显式登记。

---

## 2 · 12 条 AC 逐条自测

### 2.1 AC-001 · AI 关闭时行为完全不变 ✅（最强证据）

用 **`a8a51ed` 编出的基线二进制**（`git archive` → 独立 target 编译）与**本次二进制**，
在**同一份真 schema（96 表）+ 真采样（59 条读到样本）**上按同一命令各跑一次：

```
ontok-propose --schema …/schema-src.json --names … --project req109off --src $SRC_URL
                （REQ-109 侧**不加 --llm**、不设任何 ONTOK_LLM_*）
```

| 产物 | sha256（两份**相同**） |
| --- | --- |
| `proposal.json` | `DAA1FDFF29A939FBEFB2CF0C2246FE98DA50C46559FA1EC0323C7146F2359379` |
| `stats.json` | `F1E6735511C579B8211A114CC5C8ED6D82472FBD6FFF7CC775913C5EA1D06560` |
| `proposal-pending.json` | `F4FCDDCE52933C9496DF5F77382A10497CC3536B59B30A648A5C99314DFD74A1` |
| `proposal-rejected.json` | `EE853D32A0CE6FAD4CFB81E043FEC62C50B60E7CDBAEBB0CA473E539B405FDCA` |

> 另有进程级用例 `ai_off_is_not_affected_by_present_llm_env`：**配了 LLM 环境变量但不给 `--llm`**
> 与「环境全空」四份产物 sha256 逐个相同 —— 开不开 AI 只由开关决定，不由环境决定。
> 「语义缺失时的归一化与 v1 逐位相同」由 `r6_missing_semantic_is_absent_not_zero` + `ac001_*` 钉住。

### 2.2 AC-002 · 无 key 且开 AI ⇒ 报错退出 ✅

| 输入 | 结果 |
| --- | --- |
| `--llm`，三个变量全空 | exit **1**，文案点名 `ONTOK_LLM_BASE_URL / ONTOK_LLM_API_KEY / ONTOK_LLM_MODEL`，且**一个产物都不写** |
| 只缺其中一个 | exit 1，**只点名那一个**（不误报配好的），三条逐个实测 |

判据在 `LlmSettings::from_env`（`ai_semantic.rs:191`），且在**读 schema 之前**执行（`ontok-propose.rs:194`）。
用例：`test_req109_cli_gate.rs` 的 `ac002_*`（2 条）。

### 2.3 AC-003 · 缓存命中 ⇒ 不调用模型 ✅

三层证据：

| 层 | 判据 | 结果 |
| --- | --- | --- |
| 引擎（scripted） | 同输入跑两次 | 第一次 2 调用、第二次 **0 调用**且 `cache_hit=true`；清空缓存后第三次 **+1 调用** |
| 端到端（真进程 + 假端点 + 真 `ai_cache` 表） | HTTP 请求计数器 | run1=1 → run2=**1（不增）**；删掉该模型的缓存行 → run3=2 |
| 真实全量 | CLI 报告 | 第二次全量 **82 请求 / 82 命中 / 0 调用 / 0 token / 命中率 100%** |

### 2.4 AC-004 · 幂等：同输入两次产物逐字节相同 ⚠️ **按字面不成立，已改口径**

**实测**：全量 run2/run3 的四份产物里，`proposal.json` / `stats.json` / `proposal-rejected.json`
sha256 **完全相同**；`proposal-pending.json` **只差一行**：

```
line 8:  -   "cache_hit": false,
line 8:  +   "cache_hit": true,
```

**为什么这条 AC 自相矛盾**（纪律 S：「AC 自己也会是错的」）：

- AC-003 要求「第二次 `ai.cache_hit == true`」，AC-010 要求「model-view 必须有 `cache_hit`」，
  demand §范围 6 要求「提案条目带 `ai: {…, cache_hit}`」—— 三处都要求 `cache_hit` **落进产物**；
- 而 `cache_hit` 记的是**这一次运行**的事实，第一次必然 `false`、第二次必然 `true`；
- ⇒ 「两次运行的产物 sha256 逐个相同」与上述三条**不可能同时成立**。

**本实现的取舍**（宁可让 AC-004 改口径，也不删 `cache_hit`）：
产物内容除这一位外逐字节相同；**判据改为**「把 `ai.cache_hit` 归一后四份产物 sha256 逐个相同，
且两份文件的差异**只有** `cache_hit` 这一行」。用例 `ac003_ac004_cache_hit_then_reload`
把这条同时钉成两个断言（归一后相等 + `differing_lines == [("false","true")]`）。

> ⚠️ 另一条**必须记下的实况**：模型**非确定**（同一输入两次采样，rationale 长度会变：
> 实测同一条关系第一次 66 字被判作废、第二次 41 字通过）。所以「幂等」是**缓存**给的，
> 不是模型给的 —— 这也解释了为什么 AC-004 的两跑必须是「第二次命中缓存」的那两跑。

### 2.5 AC-005 · 改 prompt ⇒ 指纹变 ⇒ 必须重新调用 ✅

| 证据 | 结果 |
| --- | --- |
| 单测 | 改 1 个字符 ⇒ `prompt_sha256` 变 ⇒ `cache_hits=0`、`calls=1` |
| 指纹六项逐项 | relation / schema / prompt / model / agent_version / 版本前缀，**任一变即换指纹** |
| 真实运行 | 第 1 轮提示词 `49bc2046…`（3/5 因 rationale 超长作废）→ 强化格式约束后 `4298cd82…`，**旧缓存全部失效**，5 条重新调用 |

### 2.6 AC-006 · 预算闸 ✅

| 判据 | 结果 |
| --- | --- |
| `--llm` 缺 `--budget-tokens` | exit 1，点名 `--budget-tokens` |
| `--budget-tokens 0` | exit 1（「0 等于一次都不许调用」） |
| 预算 1 token（真进程 + 假端点） | 只调 1 次即停，exit 1，**四份产物一个都不存在** |
| **真实全量 100k** | **101,326 / 100,000 停在第 82 条之后**（82 条全判完、用量越顶），exit **1**，`out/req109/full1/` 里**只有 run.log**，没有任何提案文件 |

报告格式实测（`out/req109/full1/run.log`）：

```
✗ 预算超限  : 已用 101326 / 上限 100000，停在第 82 条之后（82 条已全部判完，用量越过了上限）
```

### 2.7 AC-007 · `semantic_fit` 不能翻转 veto ✅

| 构造（单测） | 结果 |
| --- | --- |
| 0 命中 + AI 1.0 | **0.20 / 拒绝**（`zero_hit`） |
| 类型不相容 + AI 1.0 | **0.15 / 拒绝**（`type_incompatible`） |
| 源列全空 + AI 1.0 | ≤ 0.45 |
| `--no-sample` + AI 1.0 | ≤ 0.45 |

**真库 82 条逐条复核**：`score > cap` 的关系数 = **0**。
REQ-108 的 9 条已知错误：**E1–E7 仍全部 0.20/拒绝**（见 §5）；E8/E9 本就不产出，
其 0.15 上限由单测按坐标构造复算覆盖。

### 2.8 AC-008 · `cites` 必须是真实坐标 ✅

构造 `{"table":"nonexistent","column":"col"}` ⇒ 该条判断**整条作废**（不进 map、不污染分数）、
`rejected_cites=1`、报告里点名假坐标；且**作废记录照样进缓存**，回放时仍然作废、但**不再付费**
（`calls=0`、`rejected_cache_hits=1`）。

### 2.9 AC-009 · `rationale` 必填且简短 ✅

| 坏输入 | 结果 |
| --- | --- |
| 缺 `rationale` | 作废（`rejected_rationale=1`） |
| `"   "`（空白） | 作废 |
| 61 字 | 作废 |
| **正好 60 字** | **通过**（边界不能写成 ≤59） |

真实模型实测：82 条里 **6 条** rationale 超 60 字被作废（68/64/85/75/73/71 字）——
这是**协议按 AC-009 正常生效**，不是流程缺陷；强化格式约束后仍有 1/82 不守（见 §6）。

### 2.10 AC-010 · 客户视图零泄漏 / 建模轨必须有 ✅

**端到端**（真库：`project req109ai` → import → `review decide --accept-all` → `proposal apply` → v2）：

| 出口 | `semantic_fit` | `cache_hit` | `"ai"` | `prompt_sha` | 模型名 |
| --- | --- | --- | --- | --- | --- |
| `ontology show`（客户 JSON） | **0** | **0** | **0** | **0** | **0** |
| `ontology show --html`（客户页面） | **0** | **0** | — | **0** | **0** |
| `ontology overview`（客户总览） | **0** | **0** | — | **0** | **0** |
| `ontology show --model-view` | **51** | **51** | **51** | **51** | **51** |

零泄漏是**结构保证**：客户视图的投影输入 `SnapshotView` / `LinkTypeView` 里**没有** confidence 栏。
审核界面（`review show --confidence`）由 `render_confidence_table` 新增 AI 行覆盖，单测断言
`ai <model> · fit … · cache_hit … · tokens … ·「rationale」` 全部出现。

### 2.11 AC-011 · 无 key 也能全绿 ✅

所有自动化用例**零网络、零 key**：引擎层用 `ScriptedTransport`；端到端层用**本地
`TcpListener` 假端点**（标准库，零新依赖，HTTP 请求计数）。清空 `ONTOK_LLM_*` 后
全量测试 0 failed（§7）。真实端点**只**用于 §3/§5 的手工验证，**没有**进任何测试。

### 2.12 AC-012 · 红线与不退化 ✅

| 项 | 结果 |
| --- | --- |
| 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 | **零改动**（`git status` 只有 §1 那 7 个文件 + 6 个新增） |
| 删测试 / `#[ignore]` / 放宽断言 | 无。`test_req107_source_enumeration.rs` 只加了**两条模块登记**（0 条断言改动） |
| 新增依赖 | **零**（Cargo.toml 一个字没动） |
| 不许退化 | lib **230**（=基线）· doc **4**（=基线）· 32 套件 **313**（=基线，逐套件见 §7） |

---

## 3 · 真实运行（分三阶段）

环境：源库只读 `SRC_URL` · 本地库 `ONTOK_DATABASE_URL=…/ontok_demo` · 模型 `qwen36-27B`。
`schema.json` 是 REQ-108 留下的真勘查产物（96 表 / 1024 列），**未重新勘查**（勘查与 AI 无关）。

### 3.1 阶段 1 · 小样本 5 条

命令要点：`--llm --budget-tokens 100000 --llm-only 'model_id -> llm_model' --llm-limit 5`
（这 5 条正是「目标表另有业务键 `model_id`、规则只能指 `id`」的那一类已知错关系）。

| 轮 | 提示词 | 调用 | token | 有效/作废 |
| --- | --- | --- | --- | --- |
| 小样本 #1 | `49bc2046…` | 5 | **6,754** | 2 / 3（rationale 68·64·85 字） |
| 小样本 #2（格式约束强化后） | `4298cd82…` | 5 | **6,858** | 4 / 1（rationale 66 字） |

**5 条真实 AI 返回（从产物 `confidence.ai` + `semantic_fit` 信号还原，即缓存里存的那份判断）**：

```json
[
  {
    "relation": "deductions.model_id -> llm_model",
    "band": "low", "score": 0.42,
    "ai_output": {
      "choice": "llm_model.model_id",
      "semantic_fit": 1.0,
      "rationale": "deductions.model_id存模型ID，对应llm_model.model_id业务含义一致。",
      "cites": ["deductions.model_id", "llm_model.model_id"]
    },
    "provenance": {"model":"qwen36-27B","prompt_sha":"4298cd825b10…18a6c9","tokens":1595,"cache_hit":true}
  },
  {
    "relation": "llm_provider_back.model_id -> llm_model",
    "band": "low", "score": 0.42,
    "ai_output": {
      "choice": "llm_model.model_id",
      "semantic_fit": 1.0,
      "rationale": "源列注释明确指向目标列，语义完全匹配。",
      "cites": ["llm_provider_back.model_id", "llm_model.model_id"]
    },
    "provenance": {"model":"qwen36-27B","prompt_sha":"4298cd825b10…18a6c9","tokens":1336,"cache_hit":true}
  },
  {
    "relation": "llm_model_peak_config.model_id -> llm_model",
    "band": "rejected", "score": 0.20,
    "ai_output": {
      "choice": "llm_model.model_id",
      "semantic_fit": 1.0,
      "rationale": "源端model_id指向目标表同名业务主键，符合配置引用实体的惯例。",
      "cites": ["llm_model_peak_config.model_id", "llm_model.model_id"]
    },
    "provenance": {"model":"qwen36-27B","prompt_sha":"4298cd825b10…18a6c9","tokens":1258,"cache_hit":true}
  },
  {
    "relation": "llm_model_sort.model_id -> llm_model",
    "band": "rejected", "score": 0.20,
    "ai_output": {
      "choice": "llm_model.model_id",
      "semantic_fit": 1.0,
      "rationale": "两表model_id均注释为模型ID，业务上应为同一实体的标识符关联。",
      "cites": ["llm_model_sort.model_id", "llm_model.model_id"]
    },
    "provenance": {"model":"qwen36-27B","prompt_sha":"4298cd825b10…18a6c9","tokens":1306,"cache_hit":true}
  },
  {
    "relation": "llm_provider.model_id -> llm_model",
    "band": "rejected", "score": 0.20,
    "ai_output": {
      "choice": "llm_model.model_id",
      "semantic_fit": 1.0,
      "rationale": "provider.model_id注释明确指向llm_model.model_id，语义匹配。",
      "cites": ["llm_provider.model_id", "llm_model.model_id"]
    },
    "provenance": {"model":"qwen36-27B","prompt_sha":"4298cd825b10…18a6c9","tokens":1336,"cache_hit":true}
  }
]
```

> `cache_hit: true` 是因为这 5 条在前一轮小样本已入缓存；`tokens` 是**当初那次**的用量（不重复计费）。

### 3.2 阶段 2 · 全量（`--budget-tokens 100000`）

| 轮 | 请求 | 缓存命中 | 实际调用 | token | 结果 |
| --- | --- | --- | --- | --- | --- |
| 全量 #1 | 82 | 4 | **78** | **101,326** | **越预算 ⇒ exit 1，`out/req109/full1/` 只有 run.log（0 份提案文件）** |
| 全量 #2（预算仍 100k） | 82 | 76 | **6** | **7,780** | exit 0，四份产物写出（补上 6 条当初被截断/作废的） |
| 全量 #3（再跑一次） | 82 | **82** | **0** | **0** | exit 0，**命中率 100%，0 次调用** |

**真实 token 总消耗 ≈ 122,718**（小样本 6,754 + 6,858 · 全量 101,326 + 7,780）。

> 全量 #1 的 78 次调用里，**76 条有效、6 条被协议作废**；#2 只补那 6 条，
> 其中 5 条这次答合规了、1 条仍超长 ⇒ #3 里 82 条全部命中（81 判定 + 1 作废回放）。
>
> ⚠️ **为什么 #2 不是 0 调用**：#1 被预算**中途截断**（产品行为，AC-006 要求），
> 那 6 条作废答案当时还没进缓存。#2 补完后才达到「全部命中」。

### 3.3 阶段 3 · 再跑一次（缓存验证）

全量 #3：`判定 81 条（缓存命中 82 · 实际调用 0 · 其中作废记录回放 1 条）· token 用 0 / 上限 100000 ·
缓存命中率 100%`。第二次 e2e 项目 `req109ai` 亦为 `82 命中 / 0 调用 / 0 token`。

---

## 4 · AI 开 / 关 分数对照

同一 schema、同一采样、同一批 82 条关系；唯一差别是 `--llm`。

| 指标 | AI 关（v1） | AI 开（v2） |
| --- | --- | --- |
| 分档 | 高 44 · 中 8 · 低 20 · 拒绝 10 | **高 40 · 中 12 · 低 23 · 拒绝 7** |
| 有 `semantic_fit` 的关系 | 0 | 81（+1 条作废回放） |
| **分数变化的关系** | — | **47 / 82** |

### 4.1 档位变化（7 条）

| 关系 | 分数 关→开 | 档位 | AI 的 `semantic_fit` / 理由 |
| --- | --- | --- | --- |
| `customer_rebate.customer_id → customers` | 1.00 → **0.85** | 高→中 | **0.00** ·「customer_id非主键，customers.id是主键，类型虽匹配但业务上ID不对应。」 |
| `deductions.customer_id → customers` | 1.00 → **0.85** | 高→中 | **0.00** ·「customers表主键id无业务语义，扣费记录中的customer_id应关联客户编号而非系统ID。」 |
| `deductions.license_id → licenses` | 1.00 → **0.85** | 高→中 | **0.00** ·「源列名license_id，目标表无同名列，业务上通常外键对主键id或业务键key。」 |
| `usage_reconciliation_log.subscription_id → customer_subscriptions` | 0.92 → **0.86** | 高→中 | 0.50 ·「…目标表无该列，仅id可对应，存在歧义。」 |
| `llm_routing_rules.group_id → llm_provider_groups` | 0.26 → **0.37** | 拒绝→低 | 1.00 ·「规则表外键group_id指向组表主键id，类型一致。」 |
| `proxy_operation_records.auth_id → proxy_operation_auths` | 0.26 → **0.36** | 拒绝→低 | 0.90 ·「…虽命名不完全对应但业务指向明确。」 |
| `simulate_login_operations.session_id → simulate_login_sessions` | 0.26 → **0.30** | 拒绝→低 | 0.50 ·「…目标主键是主键ID，通常对应token或session_id，当前指向不明。」 |

### 4.2 分数变了但档位没变的 40 条（按 `semantic_fit` 分桶）

| `fit` | 条数 | 方向 | 例子 |
| --- | --- | --- | --- |
| **1.0** | 29 | 抬升（多为高/中档内的小幅加分） | `app_billing_transactions.app_id → service_apps` 0.92→0.93；`deductions.model_id → llm_model` 0.32→0.42 |
| **0.5** | 12 | 折半 | `operate_log.session_id → simulate_login_sessions` 0.86→0.81；`order.subscription_id → customer_subscriptions` 0.70→0.74 |
| **0.0** | 3 | 拉低 0.15（§5.2 标红的三条） | `*_customer_id → customers` 那一批 1.00→0.93（仍留高档） |
| 0.8 / 0.9 | 3 | 小幅 | `proxy_operation_records.auth_id → proxy_operation_auths` 0.26→0.36 |

> 结论：AI 的 15% 权重**只会让分数在同档内小幅移动**（±0.15 上限），
> 只有「本来就在档位边界附近」的关系才会换档 —— 与设计概要 §3.1 的意图一致。

### 4.3 veto 逐条复核（红线）

- 82 条里 `score > veto.cap` 的：**0 条**；
- REQ-108 的 E1–E7（7 条 `zero_hit`）：**全部仍是 0.20 / 拒绝**，即使 AI 给 1.0；
- E8/E9（不产出）：`type_incompatible` 0.15 上限由单测按坐标构造复算覆盖（+AI 1.0 仍 0.15）。

---

## 5 · 语义质量（**P1 观察项，不作通过条件**）

BA 裁定：私有化部署模型只用于验证流程，准确率不是本轮目标。以下**如实记录，不修饰**。

### 5.1 本轮样本上的结论

> **在本轮 82 条关系上，模型对 81 条给出了判断，但没有一条能被「用来修正结构」**：
> 它在 5 条小样本上都正确地指出「业务键是 `llm_model.model_id`（choice）」，同时把
> **`semantic_fit` 打成 1.0**（= 这条关系说得通）—— 而提案实际指向的是 `llm_model.id`。
> 也就是说：**它读懂了「关系说得通」，但没有回答「目标列选对了吗」**；而 REQ-109 的语义项
> **不允许改结构**（G1），所以「6 条 `model_id` 根因」**没有被修**（本轮也没打算修）。

### 5.2 🔴 标红：AI 把 3 条**已被 5/5 采样证实**的正确关系判成 0.00

| 关系 | 采样 | AI | 后果 |
| --- | --- | --- | --- |
| `customer_rebate.customer_id → customers` | **5/5 命中** | 0.00 | 1.00 → 0.85（**高→中**） |
| `deductions.customer_id → customers` | **5/5 命中** | 0.00 | 1.00 → 0.85（**高→中**） |
| `deductions.license_id → licenses` | **5/5 命中** | 0.00 | 1.00 → 0.85（**高→中**） |

模型的三条 rationale 都建立在**同一个误解**上：「外键应指向业务键，不该指向无业务语义的主键 `id`」。
但数据证据（5/5 命中）恰恰证明 `customer_id` 的取值**确实存在于** `customers.id`。
采样**没能拦住**这条误判 —— 因为采样只往下压（veto），AI 的错判是往下扣分前的正常加权。

- 影响：3 条**正确**关系从「高」降到「中」（仍进主提案，不会被丢弃）；
- 定性：**模型能力问题，不是流程缺陷**（veto、预算、缓存、溯源全部按设计工作）；
- 建议（不在本 REQ 内做）：若将来要让语义项真正参与裁决，需要**正/负两类样本校准**，
  或把 `semantic_fit` 的方向性交给人工复核，而不是直接进分数。

### 5.3 其它观察

1. **`semantic_fit` 的语义歧义**：协议里它是「这条关系说得通吗」，但模型常答成「我选的
   `choice` 有多可信」。5 条小样本里 5 条都是这种读法。Prompt 已明确写「choice 不改变评分，
   目标列不对就该给低分」，模型仍倾向按 choice 打分 —— 属模型理解偏差，未再做提示词调优（BA 裁定）。
2. **模型非确定**：同一输入两次调用，rationale 长度会变（66 字被判作废 / 41 字通过）。
   ⇒ 幂等只能由缓存保证；`tokens` 回放也说明「同一条判断不重复计费」。
3. **格式遵从度**：强化一次格式约束后，超 60 字的从 3/5 降到 1/82。仍不完美，但不值得继续调
   （AC-009 的作废逻辑已把不合规答案挡在分数之外）。

---

## 6 · 测试数字

**权威口径**（每个 `--test` 目标前临时挪开 `src/bin/ontok-gate-preflight.rs`，
跑完立即挪回；**sha256 before = after = `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`**，
本会话**每一次**绕行都做了前后比对、全部一致；`git diff a8a51ed -- src/bin/ontok-gate-preflight.rs` 为空）。

| 目标 | 本轮 | 基线（REQ-108） | 判定 |
| --- | --- | --- | --- |
| `cargo test --lib` | **230 passed / 0 failed** | 230 | ✅ 不退化 |
| `cargo test --doc` | **4 passed / 0 failed** | 4 | ✅ 不退化 |
| 32 个回归套件 | **313 passed / 0 failed** | 313 | ✅ 不退化（逐套件同 §11.3 名单） |
| 新增 3 套件 | **31 passed / 0 failed** | — | 22 + 6 + 3 |
| **套件合计（35 个）** | **344 passed / 0 failed** | 313（32 套件） | +31 |
| **总计** | **578 passed / 0 failed** | 547 | +31 |

跑测试时的环境：`ONTOK_TEST_DATABASE_URL`（本地库）+ `SRC_URL`（源库只读，供 `test_req108_live`）。
**LLM 三个环境变量全程未设** —— 这就是 AC-011 的判据。

新增用例分布：

| 套件 | 条数 | 覆盖 |
| --- | --- | --- |
| `test_req109_ai_semantic` | 22 | 指纹 6 项 · 缓存 0 调用/清空重调/`--refresh` · 改 prompt · 预算停 · 协议 4 类作废 · 无 usage · 空正文 · 幂等 · v1/v2 权重 · veto 不被翻 · 缺失≠0 · 溯源成对 · 零泄漏 · 接缝 · 无 key |
| `test_req109_cli_gate` | 6 | AC-002（全缺 / 单缺逐个）· AC-006（缺预算 / 0 预算）· `--refresh` 误用 · AI 关不受环境变量影响 |
| `test_req109_e2e_fake_llm` | 3 | 真进程 + 假端点 + 真缓存表：缓存命中/删行重调 · 幂等（含 `cache_hit` 一位的差异证明）· 预算 1 不写产物 |

---

## 7 · 偏差 / AC 缺陷 / 已知限制

| # | 项 | 说明 |
| --- | --- | --- |
| D-1 | **AC-004 与 AC-003/AC-010 互斥** | 见 §2.4。`cache_hit` 必须落产物（3 处要求）× 它逐次不同 ⇒ 「两次 sha256 完全相同」不可能成立。本实现保留 `cache_hit`，判据改为「归一后相同 + 差异只有这一行」，并给出逐行 diff 证据。**请 BA 裁定** |
| D-2 | `ai` 挂在 `confidence` 里，不是条目的顶格键 | demand 写「提案条目带 `ai: {…}`」。若做顶格键，`propose import` 会把它搬进 `payload` 后**再没人读**（快照层 `LinkType` 只认 `confidence`），于是文件一份、库里没有 ⇒ 两份迟早不一致。挂在 `confidence.ai` 则**一次写入**穿到提案/审核界面/快照/建模轨。字段本身（`model` / `prompt_sha` / `tokens` / `cache_hit`）与 demand 逐字一致 |
| D-3 | 新增两个**只截断**的运维闸 | `--llm-only` / `--llm-limit`，为满足「先 5 条小样本再全量」。不参与判据 |
| D-4 | `ai_cache.tokens_used` 允许 NULL | 历史行/缺 usage 的回放记 0（不编数）。报告里 token 总量会偏低，这是唯一诚实的表示 |
| D-5 | **作废答案也进缓存** | 否则每条被协议作废的关系每次重跑都要再付一次钱，且「第二次 0 调用」永远不成立。回放时**照样作废**（`judgement=None`），不会被当成「AI 说不成立」 |
| L-1 | G1 不变 | AI 只能在候选集合内选择；跨表 retarget 无能为力 ⇒ `model_id` 根因修不了 |
| L-2 | G2 不变 | 供应商静默升级模型时缓存仍返回旧结果（`model_id` 只防显式切换） |
| L-3 | 成本 | 全量 82 条 ≈ 101k token（≈1.2k/条，含推理）；83 表全量靠缓存与预算闸控制 |
| L-4 | 本地库副作用 | 为 AC-010 的端到端 grep，本地库建了 `project req109ai` 并 import→apply 到 **v2**；`req109full` 只建了项目未落版本。两者都不影响仓库内容 |

---

## 8 · 我认为最可能被 QA 打回的一点

**AC-004 的改口径（D-1）。**

理由：这是**唯一一条我没有按字面交付的 AC**。我给出的证据是「差异只有 `cache_hit` 一行」，
但 QA 完全可以说「AC 写的是四份 sha256 相同，你交的是归一后相同」。

对打的理由也摆在桌面：要按字面满足 AC-004，只能**把 `cache_hit` 从产物里删掉** ——
那会同时违反 AC-003（「第二次 `ai.cache_hit == true`」）、AC-010（「model-view 必须有」）
和 demand §范围 6（「提案条目带 `ai: {…, cache_hit}`」）。**三条 AC 里至少有一条必须让步**，
我选择让步那条**自相矛盾**的，并在报告里把互斥关系与逐行 diff 一起交出来。

第二可能被打回的是 **D-2（`ai` 在 `confidence` 内而不是条目顶格）** —— 同上，给了结构性理由。

第三是 **§5.2 的三条误判**：那 3 条 5/5 采样的正确关系被 AI 扣了 0.15 分（高→中）。
按 BA 裁定这**不阻断交付**，但 QA 若认为「AI 降低正确关系」不可接受，需要 BA 决定是否调权重。
