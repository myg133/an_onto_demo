# REQ-109 · AI 语义项接入 —— Pre-merge QA 审核

| 项 | 值 |
| --- | --- |
| 被测 | 分支 `feature/REQ-109` @ **`5af349d`**（基线 `develop` `a8a51ed`） |
| 审核范围 | 12 条 AC 的**机制**部分（P0）＋ A-4~A-7 裁定口径；语义准确率只作 P1 观察 |
| 审核时刻 | 2026-10-21 18:52 – 19:15（+08:00） |
| 源库快照 | `anspire_open_uat` @ `2026-10-21T10:55:54Z`（96 表 / 1116 列 / 0 外键 / 只读会话 `on`） |
| 本地库 | `172.17.0.87:5432/ontok_demo`；新项目 `qa109view`（v2） |
| 方法 | **独立 worktree**（`.review-workspace/req109/wt`，从 `5af349d` detached；审核结束已清理，用例副本留在 `qa/qa109_independent.rs`）＋ 自写 19 条 QA 用例 ＋ 本地假 OpenAI 端点 ＋ 真基线二进制（自己从 `a8a51ed` 编译）＋ 3 组变异 |
| 未触碰 | feature 分支被跟踪文件**零改动**（`git status` 全程为空；跑测试时临时挪开 `ontok-gate-preflight.rs`，跑完复原且 sha256 一致）；未 push；未改 demand 分支任何已有文件（本报告为新文件） |

> 源库只读；LLM key 只在进程环境变量里出现，未写进任何文件（含本报告）。

---

## 0 · 结论

**PASS（带 6 项非阻断登记，其中 3 项建议 BA 追认/订正文本，见 §8）。**

| 关注点 | 结论 |
| --- | --- |
| 流程六项是否真通 | ✅ **六项全部独立复现**（缺变量报错 / 缓存 0 调用 / 预算停且不出产物 / `--refresh` 重调 / 幂等 / AI 关逐字节同 REQ-108） |
| AI 能否翻转 veto | ✅ **不能**。82 条 `score > 自身 cap` = **0**；E1–E7（0 命中）在 AI 给 1.0 时**仍 0.20 / 拒绝**；类型不相容仍 0.15 |
| 协议攻防 a–e | ✅ 全部按「该条作废、不进分数、不中断整轮」工作（15 条作废 / 67 条判定 / exit 0） |
| 凭据 | ✅ 被跟踪文件里 key / 口令 / 内网 IP **0 命中**；失败报错文案不含 key 与 `Bearer` |
| 客户视图 | ✅ JSON / HTML / overview 的 AI 字段 **0 命中**；`--model-view` 有 52 条（每条被 AI 判过的关系都有） |
| 与 Dev 报告数字 | ✅ 测试 578 一条不差、缓存 82/82/0 一致、3 条误判 1.00→0.85 一致；**1 处口径差异**（A-5 写的「5/5」实测有两条是 1/1，见 §7.2，不影响结论） |

---

## 1 · 审核方法与证据边界

### 1.1 做了什么（与 Dev 自述无关的独立证据）

1. **自己编译基线**：`code/`（`a8a51ed`，干净）`cargo build --bin ontok-propose`（原 `target/` 里的二进制是 REQ-108 之前的，不认 `--src`，已弃用）。
2. **自己勘查源库**：`ontok-introspect --out` → 96 表 / 1116 列 / `transaction_read_only=on`；schema sha256 `2E183D43…7E81D54F`。
3. **本地假 LLM 端点**：Python `http.server`，OpenAI 兼容，逐请求落 JSONL 计数；模式 `fit1 / mixed / attack`。
4. **真库真路径**：`ai_cache` 表读写、`propose import → review decide --accept-all → proposal apply → ontology show`。
5. **自写 QA 用例 19 条**（独立 worktree，不引用 Dev 测试文件）＋ **变异 3 组**（证明用例是承重的，不是装饰）。
6. **测试全量重跑**：35 套件 + lib + doc，绕行方式同 Dev（临时挪开 `ontok-gate-preflight.rs`，**sha256 前后一致**）。

### 1.2 边界（如实说）

- **没有重跑 82 条真模型全量**（成本 ≈101k token）。Dev 报告里的 `101,326 / 100,000` 这个**具体数字不可复现**，我改用**假端点**独立验证预算闸的行为（§2.3），并用库里 Dev 已缓存的真模型回答做了 1 条**零成本回放**（§7.1）。
- 模型的**非确定性**（Dev §2.4 注）未单独复测。

---

## 2 · 流程六项（逐项证据）

### 2.1 ① `--llm` 缺任一 `ONTOK_LLM_*` ⇒ 非 0 退出并点名

7 组构造，全部 `exit=1`、**0 份产物**：

| 构造 | exit | 产物 | 报错点名 |
| --- | --- | --- | --- |
| 三个全缺 | 1 | 0 | `ONTOK_LLM_BASE_URL / ONTOK_LLM_API_KEY / ONTOK_LLM_MODEL` |
| 只配 BASE_URL | 1 | 0 | `ONTOK_LLM_API_KEY / ONTOK_LLM_MODEL` |
| 只配 API_KEY | 1 | 0 | `ONTOK_LLM_BASE_URL / ONTOK_LLM_MODEL` |
| 只配 MODEL | 1 | 0 | `ONTOK_LLM_BASE_URL / ONTOK_LLM_API_KEY` |
| 只缺 MODEL | 1 | 0 | `ONTOK_LLM_MODEL`（不误报配好的两个） |
| 三件齐全，缺 `--budget-tokens` | 1 | 0 | ``--budget-tokens``（「必填且 >0」） |
| `--budget-tokens 0` | 1 | 0 | 「0 等于一次都不许调用」 |

另测三条相近闸（都不是主判据，但都 fail-closed）：`--llm` 缺 `ONTOK_DATABASE_URL` ⇒ 点名该变量；`--refresh` 不带 `--llm` ⇒ 报错；`--llm-limit 0` ⇒ 报错。

> 闸的**顺序**也是对的：环境变量 → 预算 → 产品库 → 读 schema。即使 schema 路径不存在，也先报缺变量（不会「跑到一半才说没 key」）。

### 2.2 ② 缓存命中 ⇒ 第二次 **0 次模型调用**

假端点逐请求计数（`qa\req-fit1.jsonl`）：

| 轮 | 请求 | 缓存命中 | 实际调用 | token | 说明 |
| --- | --- | --- | --- | --- | --- |
| run1 | 82 | 0 | **82** | 9020 | 冷缓存 |
| run2 | 82 | **82** | **0** | **0** | 请求日志行数 **82 → 82（不增）**，命中率 100% |
| run3 `--refresh` | 82 | 0 | **82** | 9020 | 增量 +82，打印「强制刷新 82 条」 |
| run4：`DELETE FROM ai_cache WHERE model=…` 后 | 82 | 0 | **82** | 9020 | 增量 +82 ⇒ 缓存**真在起作用**，不是「假装命中」 |

真库侧核对：`ai_cache` 该模型 82 行、`tokens_used` 合计 9020；`prompt_sha = 4298cd82…`（与 Dev 的最终提示词一致）。

### 2.3 ③ 预算闸 ⇒ 停止 + 非 0 + **不产出半份提案**

| 构造 | call 数 | 报告 | exit | 产物文件 |
| --- | --- | --- | --- | --- |
| `--budget-tokens 1` | **1** | 已用 **110** / 上限 **1**，停在第 **2** 条（共 82 条） | **1** | **0**（连 `run.log` 都没有） |
| `--budget-tokens 500` | 5（+1 命中） | 已用 **550** / 上限 **500**，停在第 **7** 条 | **1** | **0** |

- 越限**先回写缓存再 bail**：两次越限运行合计在库里留下 6 行（钱已经花了，判断不白费）。
- 目录里没有任何 `proposal*.json` / `stats.json` —— 满足 AC-006「文件要么不存在，要么 `importable:false`」的**更强**分支。

### 2.4 ④ `--refresh` ⇒ 忽略缓存、重新调用、覆盖

见 §2.2 run3：82 次调用、`cache_hit=false`、CLI 打印「⚠ 强制刷新 : 82 条（--refresh：忽略已有缓存并覆盖）」、库里同指纹行被 `ON CONFLICT DO UPDATE` 覆盖（`created_at` 更新）。

### 2.5 ⑤ 幂等（**A-4 口径**：抹掉 `ai.cache_hit` 后逐字节相同）

run1（冷）vs run2（全命中）：

| 产物 | 原始 | 归一后 sha256 | 差异 |
| --- | --- | --- | --- |
| `proposal.json` | RAW_DIFF | `BF5FD00F…5F9C1366` **NORM_SAME** | 52 行，**全部**是 `"cache_hit": false → true` |
| `stats.json` | **RAW_SAME** | `F1E67355…5EA1D06560` | 0 |
| `proposal-pending.json` | RAW_DIFF | `706B2006…B89455CBC` **NORM_SAME** | 23 行，全部是 `cache_hit` |
| `proposal-rejected.json` | RAW_DIFF | `9C217F79…4B05306F` **NORM_SAME** | 7 行，全部是 `cache_hit` |

- 差异行数合计 **82 = 关系数**（52+23+7），非 `cache_hit` 差异 **0 行**。
- 这正是 BA 在 A-4 里裁定的口径，实现与裁定一致；`cache_hit` 确实落进了产物（审计可用）。

### 2.6 ⑥ `--llm` 关闭 ⇒ 与 REQ-108 **逐字节相同**

同一份 schema、同一份采样（真源库 5 行）、同一 `--project`，`feature` 二进制 vs **我自编译的 `a8a51ed` 基线**：

| 产物 | sha256（两侧相同） |
| --- | --- |
| `proposal.json` | `5C545132EE21B70C67873952C245D23516D384EEC86490B31FE8042684E8CDD9` |
| `stats.json` | `F1E6735511C579B8211A114CC5C8ED6D82472FBD6FFF7CC775913C5EA1D06560` |
| `proposal-pending.json` | `13BDEEA8E8F92D1BAED7C33C8AD2E40029DA6ADC439E8CD7CBD24879ED514F9E` |
| `proposal-rejected.json` | `73951ACBFB475E6C7101B51483389F9F9BCEA1CB029D9F89123A9C0047970D8B` |

**加测**：把 `ONTOK_LLM_BASE_URL / API_KEY / MODEL` 全配上、但不给 `--llm`，四份产物与「环境全空」**4/4 逐字节相同** ⇒ 开不开 AI 只由开关决定，不由环境决定。

> `stats.json` 的 sha 与 Dev 报告 §2.1 的 `F1E67355…` **完全一致**（交叉印证）。`proposal.json` 与 Dev 的不同是源库勘查时刻/采样快照不同所致，不影响 AC-001 —— 我比的是**同一次输入下的两个二进制**。

---

## 3 · AI 不能翻转 veto（红线）

### 3.1 三条构造（判据 = 分数，不是「AI 说了算」）

| # | 构造 | AI 返回 | 实测 | 期望 | 结论 |
| --- | --- | --- | --- | --- | --- |
| 1 | 采样 **0 命中**（真实 E1–E7 坐标） | `semantic_fit = 1.0` | **0.20 / 拒绝**（7/7 条全部） | ≤ 0.20 | ✅ |
| 2 | **类型不相容**（`Str → Num`） | `1.0` | **0.15 / 拒绝**（QA 独立用例） | ≤ 0.15 | ✅ |
| 3 | 采样 **5/5 命中** | `0.0` | 1.00 → **0.85 / 高→中** | 可降分，**记录不阻断** | ✅ 记录（P1） |

补充：全空列 + 1.0 ⇒ ≤ 0.45；`--no-sample` + 1.0 ⇒ ≤ 0.45；泛化列名 + 无证据 + 1.0 ⇒ ≤ 0.40（实测 0.37）。

### 3.2 82 条逐条复核（AI 全给 1.0 的那一轮）

- `score > veto.cap` 的关系：**0 条**；
- `zero_hit` 7 条：**全部 0.20 / 拒绝**（与 REQ-108 的 E1–E7 一一对应：`balance_operation_log.batch_id`、`instance_manager.session_id`、`internal_operation_records.batch_id`、`llm_model_peak_config.model_id`、`llm_model_sort.model_id`、`llm_provider.model_id`、`llm_provider_groups.model_id`）；
- 语义项只进 `baseline`，最终分仍是 `min(baseline, min_cap)`；**压顶那一段代码与 REQ-108 一字未改**（`git diff` 该文件仅新增 v2 权重与语义分支，veto 段 0 改动）。

### 3.3 机制层面的结论

AI 唯一能影响的是 `baseline` 里那 0.15 的加权项；veto 上限在基线之后**独立计算并取 min**。所以「AI 给 1.0 翻转 0 命中」在**算术上不可达**，不是「靠约定」。

---

## 4 · 协议攻防 a–e（scripted 假端点，不花钱）

一次全量 82 条，故意给 15 条坏输入：

| # | 注入的坏输入 | 命中条数 | 引擎行为 | 结果 |
| --- | --- | --- | --- | --- |
| a | `cites` 含 `nonexistent.col` | 1 | 整条作废，报错点名假坐标 | `rejected_cites=1`；该条**无 `ai`**、分数 = AI 关（1.00） |
| b | `rationale` 空 / 全空白 / 61 字 | 2 | 作废；**正好 60 字通过**（边界） | `rejected_rationale=2` |
| c | `choice` 不在候选集合 / 不在目标表 | 1 | 作废；`choice: null` 合法 | `rejected_choice=1` |
| d | `semantic_fit` 1.5（另测 −0.1 / 2.0 / −1） | 9 | **作废，不钳制** | `rejected_fit=9` |
| e | 非 JSON 正文 / 截断 JSON | 2 | 该条作废 | `rejected_parse=2` |

整轮表现：`判定 67 条 · 调用 82 次 · 作废 15 条 · exit 0`，四份产物**照常写出** ⇒ **单条坏输入不中断整轮**。

逐条核对 15 条被作废的关系：**全部**没有 `ai` 键、`semantic_fit` **缺失**，且分数与 AI 关**逐位相同**（1.00 / 0.86 / 0.92 / 0.20 / 0.32 原样保留）⇒ 满足 R6「缺失不是 0 分」。

### 4.1 d 的取舍：**作废**比钳制安全（我的判断）

- 钳制会把「模型吐出 1.5」这种**协议违规**静默翻译成一个**合法的高分信号**（1.0），等于让越界值拿到最高权重；
- 作废的语义是「这条判断不可用 ⇒ 按无此信号归一化」，与 AC-008/009 的处置一致，且**可审计**（报告里有 `rejected_fit` 计数与样例）；
- 作废是 fail-closed（少一条加分信号），钳制是 fail-open（多一条可疑信号）——本仓的一贯纪律选前者；
- 实现与报告口径一致（`Rejection::Fit`），不存在「说作废、实际钳制」的两套判据。

---

## 5 · 凭据与客户视图

### 5.1 凭据不落盘

| 检查 | 结果 |
| --- | --- |
| `git grep -i "<LLM key 全串>"` | **0 命中** |
| `git grep -E "sk-[A-Za-z0-9]{6,}"`（任何告警级 key 形状） | **0 命中** |
| `git grep "ONTOK_LLM_API_KEY"` | 9 处，**全部是变量名占位**：`src/agent/llm/mod.rs` 常量、`QUICKSTART.md` 的 `=...`、两份测试用 `test-only-not-a-real-key`、Dev 报告正文。**没有任何真实值** |
| `git grep "<源库口令>"` / 内网 IP | **0 命中** |
| 历史提交扫描（`a8a51ed..HEAD`） | 无 key / 无 `Bearer` 真值 |
| **失败路径回显**：把 key 设成 `sk-QA-SENTINEL-…`、端点指向关闭端口，触发 1 条调用失败 | 输出里 sentinel **0 次**、`Bearer` **0 次**；报错只给 URL 与 `io: 目标计算机积极拒绝` |
| 产物文件 / 日志 | 无 key、无 `Bearer` |

> 结论：key 只以「变量名」形式存在于文档/代码；错误信息**不回显请求头**（`HttpTransport` 只报 URL 与状态码）。

### 5.2 客户视图零泄漏 / 建模轨必须有

真链路：`propose import`（148 条目）→ `review decide --accept-all`（148×2 轨）→ `proposal apply` → **v2**。

| 出口 | `semantic_fit` | `cache_hit` | `"ai"` | `prompt_sha` | 模型名 `qa-fake-fit1` | `confidence` |
| --- | --- | --- | --- | --- | --- | --- |
| `ontology show`（客户 JSON） | **0** | **0** | **0** | **0** | **0** | **0** |
| `ontology show --html`（客户页面） | **0** | **0** | — | **0** | **0** | — |
| `ontology overview`（文本 + JSON） | **0** | **0** | — | **0** | **0** | — |
| `ontology show --model-view` | **52** | **52** | **52** | **52** | **52** | **52** |
| `review show --confidence`（审核界面） | — | — | — | — | — | 有 AI 行 |

审核界面实测行（原样）：

```
ai qa-fake-fit1 · fit 1.00 · cache_hit true · tokens 110 「QA 脚本：关系在业务上说得通。」
```

> 口径说明：AC-010 写的「grep `ai` = 0」若按**裸子串**扫会误伤业务表名（`ai_search_pipelines`、`aisearch` 等），因此按**字段/键级**核（`"ai"`、`semantic_fit`、`cache_hit`、`prompt_sha`、模型名）—— 结果是 0。客户视图的结构性保证在类型上：投影输入 `SnapshotView` / `LinkTypeView` **没有 confidence 栏**（`view.rs:838`），不是靠渲染时删除。

---

## 6 · 不退化

### 6.1 测试数字（我重跑，逐套件与 Dev 日志一致）

| 目标 | 本轮实测 | 基线（REQ-108） | 判定 |
| --- | --- | --- | --- |
| `cargo test --lib` | **230 passed / 0 failed** | 230 | ✅ |
| `cargo test --doc` | **4 passed / 0 failed** | 4 | ✅ |
| 32 个回归套件 | **313 passed / 0 failed** | 313 | ✅ |
| 新增 3 套件（`test_req109_ai_semantic` 22 / `cli_gate` 6 / `e2e_fake_llm` 3） | **31 passed / 0 failed** | — | ✅ |
| **35 套件合计** | **344 passed / 0 failed** | 313 | +31 |
| **总计** | **578 passed / 0 failed** | 547 | +31 |

- 逐套件条数与 Dev 的 `out/req109/suites-final.log` **37 个目标全等**（我用脚本比对，0 差异）。
- 绕行：临时挪开 `src/bin/ontok-gate-preflight.rs`，前后 sha256 = `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`（**一致**），工作区复原。
- 跑测试时 `ONTOK_LLM_*` **未设**（AC-011 的判据）。

### 6.2 红线与依赖

| 项 | 结果 |
| --- | --- |
| `src/domain/view.rs` / `review_view.rs` / `src/mcp/tools.rs` / `property_filter.rs` / `tests/drop_rate.rs` / `tests/test_fk_naming.rs` | `git diff a8a51ed HEAD --` **空**（0 行） |
| `guard()` | 所在文件 `tools.rs` 零 diff ⇒ 未改 |
| `Cargo.toml` / `Cargo.lock` | **0 行 diff**（零新依赖；HTTP 走既有 `agent::llm`） |
| 删测试 | **0 行**（`git diff -- tests` 无 `-` 行） |
| 新增 `#[ignore]` | **0**（唯一真实 `#[ignore]` 在 `tests/test_mcp_server.rs:554`，基线即有） |
| `test_req107_source_enumeration.rs` | 只加 **2 条模块登记**（4 行含注释），无断言改动 |

### 6.3 REQ-108 的 9/9 已知错误（AI 开启时）

| 项 | 结果 |
| --- | --- |
| E1–E7（0 命中） | **全部 0.20 / 拒绝**（AI 给 1.0 也翻不动），7/7 |
| E8/E9（类型不相容） | **三份产物里都不产出**（`channel_code` 0 命中）；按坐标构造复算 = **0.15 / `type_incompatible`**（QA 独立用例，+AI 1.0 仍 0.15） |
| 82 条 `score > cap` | **0 条** |

### 6.4 变异（证明用例承重，不是装饰）

在独立 worktree 里注入 3 个变异，跑「我的 QA 用例 + Dev 的 `test_req109_ai_semantic`」，**全部被杀**，随后 `git checkout` 复原（工作区只剩我那一个未跟踪测试文件）：

| 变异 | QA 用例 | Dev 用例 |
| --- | --- | --- |
| M1 去掉 veto 压顶（`baseline.min(min_cap)` → `baseline`） | 3 失败（zero_hit / type_incompatible / all_null） | `ac007_semantic_fit_cannot_flip_any_veto` 失败 |
| M2 跳过 `cites` 存在性校验 | 2 失败（a、a′） | `ac008_fabricated_cite_voids_the_judgement` 失败 |
| M3 禁用缓存命中（永远调用） | 2 失败（缓存 / 回放） | `ac003` / `ac004` / `ac008` 失败 |

> 编译失败不算执行：三组都**编译通过**并跑出断言失败，复原后 19/19 + 22/22 全绿。

---

## 7 · P1 观察（语义质量 —— **不作通过条件，不阻断**）

### 7.1 三条误判：**真的存在**，且与 A-5 表一致

用库里 Dev 已缓存的**真模型回答**（`model=qwen36-27B`，prompt `4298cd82…`，回放 0 调用 0 token）核对：

| 关系 | 真模型 `semantic_fit` | 真模型 rationale（原文） | 走完整管线后 |
| --- | --- | --- | --- |
| `customer_rebate.customer_id → customers.id` | **0.00** | 「customer_id非主键，customers.id是主键，类型虽匹配但业务上ID不对应。」 | 1.00 高 → **0.85 中** |
| `deductions.customer_id → customers.id` | **0.00** | 「customers表主键id无业务语义，扣费记录中的customer_id应关联客户编号而非系统ID。」 | 1.00 高 → **0.85 中** |
| `deductions.license_id → licenses.id` | **0.00** | 「源列名license_id，目标表无同名列，业务上通常外键对主键id或业务键key。」 | 1.00 高 → **0.85 中** |

我又用假端点把这 3 条**定向置 0.0**（其余给 1.0）跑了一轮：分档 `高 41 · 中 11 · 低 23 · 拒绝 7`，与 Dev 的 `高 40 · 中 12 · 低 23 · 拒绝 7` 同形 —— **降分路径被完整复现**。

定性（同意 A-5）：**模型能力问题，不是流程缺陷**。veto / 预算 / 缓存 / 协议 / 溯源全部按设计工作；采样只设上限，拦不住「把对的往下拉」。登记进「换模型时必须重新评估」清单，**本轮不实施**「AI 只能加分不能减分」的候选修法。

### 7.2 与 A-5 的一处**口径差异**（建议订正验收文本，不阻断）

A-5 写三条都是「采样 **5/5** 证实」。本轮真源库快照上：

| 关系 | 实际采样 |
| --- | --- |
| `customer_rebate.customer_id → customers.id` | **5/5** ✅ |
| `deductions.customer_id → customers.id` | **1/1**（该表只取到 1 个非空样本） |
| `deductions.license_id → licenses.id` | **1/1** |

方向不变（`matched == sampled`，仍是 100% 命中），但「5/5」这个数字对后两条不成立。属**验收文档口径**问题，建议 A-5 表改成「5/5 · 1/1 · 1/1」或直接写「matched == sampled」。

### 7.3 语义项读法歧义（Dev 已登记，我复核成立）

`llm_model_sort.model_id → llm_model.id` 的真模型回答：`semantic_fit = 1.0`、`choice = llm_model.model_id` —— 它用**高分**表达「应该指向另一个列」，而协议问的是「当前这条关系说不说得通」。缓存里 `llm_model_peak_config.model_id` 一条更明显：`fit 0.5` + `choice model_id`。

⇒ 这个通道**修不了 `model_id` 根因**（G1 不变），且「高分 + 另一个 choice」会被读成「这条关系没问题」。Prompt 已写了「choice 不改变评分」，模型仍按 choice 打分 —— 属模型理解偏差，不在本轮调优范围。

### 7.4 AI 能加分，但加不过 cap（一个正向观察）

AI 给 1.0 时，3 条 `generic_name_no_evidence`（cap 0.40）从 0.26 抬到 **0.37**、拒绝→低档 —— **正好卡在上限内**。说明「语义分只能在同档小幅移动、且不越 veto」的意图在真实数据上成立。

### 7.5 其它

- **作废答案也进缓存**（D-5）我复核成立且必要：攻击轮 15 条作废、库里同模型 67 judged + 15 rejected = 82 行；回放时仍是「无判断」，不会被当成「AI 说不成立」。
- **模型非确定性**：本轮未复测；但缓存里同一条关系出现「旧提示词作废记录 + 新提示词通过」两种行，与 Dev 的描述一致。

---

## 8 · 阻断项 / 非阻断项

### 8.1 阻断项

**无。**

### 8.2 非阻断项（登记，建议 BA 追认/知悉）

| # | 项 | 说明 | 建议 |
| --- | --- | --- | --- |
| **N-1** | **D-2：`ai` 挂在 `confidence.ai` 而不是条目顶格** | demand §交付物 6 写「提案条目带 `ai: {…}`」。Dev 的理由成立：顶格键经 `propose import` 会落进 `payload` 后无人读，快照层只认 `confidence`，会变成「文件一份、库里没有」。我端到端验证了它**确实穿到了**提案/审核界面/快照/model-view（`model.json` 52 条） | 建议 BA 追认该结构；若要字面合规，应改 demand 措辞而不是改实现 |
| **N-2** | **Dev 报告的文档缺陷**（不影响行为） | ① §6 提到「逐套件同 §11.3 名单」，但报告只有 §0–§8，**无 §11.3**；② `tests/test_req109_ai_semantic.rs` 模块头提到 `tests/test_req109_ai_cache_pg.rs`，该文件**不存在**；③ §1.1 写 ai_semantic.rs「~1030 行」，实际 **1002 行**；④ `out/req109/full1/run.log` 是「作废答案还没进缓存」那版构建的产物（写入 72 条），与现行代码（写入 82 条：67+15）不一致 —— 报告 §2.4 已解释该差异，但日志本身是旧的 | 下一轮顺手订正，不阻断 |
| **N-3** | **A-5 的「5/5」口径** | 见 §7.2：两条实际是 1/1 | 订正验收文本 |
| **N-4** | **`--llm` 额外要求 `ONTOK_DATABASE_URL`** | 设计概要 §五 只列三件套；实现还要求产品库（缓存落点），报错里点名。合理且必要（否则「命中不再调用」无处安放） | 在设计概要补一行，避免运维漏配 |
| **N-5** | **model-view 的 AI 条数 52 vs Dev 报告 51** | 我 v2 主提案 52 条关系（AI 开时 1.0 把 3 条 generic_name 抬进低档、高档不变），Dev 的 51 条来自真模型的混合打分。**运行相关，不是缺陷**；关键是「每条被 AI 判过且进了 v2 的关系都有」 | 无需动作 |
| **N-6** | **`--llm-only` / `--llm-limit` 不在设计概要里** | BA 已在 A-6 采纳为运维闸。我复核：`--llm-only` 只判 1 条时，其余 **81 条分数与 AI 关逐位相同**，未判的按语义缺失处理（不是 0） | 已在 A-6 登记，无需动作 |

### 8.3 无法确认项

1. Dev 报告的「预算 100k ⇒ 101,326 token ⇒ exit 1」**具体数字**未复现（成本原因）；预算闸的**行为**已用 1 / 500 token 两种构造独立验证。
2. 模型非确定性、以及「同一输入两次调用 rationale 长度会变」未单独复测（只核了缓存里已有两种结果）。
3. 3 条误判的「5/5」表述见 §7.2。

---

## 9 · 复现要点（证据路径）

```powershell
# 独立 worktree（不碰 feature 分支）
git worktree add .review-workspace/req109/wt 5af349d --detach

# 源库只读勘查（连接串只走环境变量）
$env:SRC_URL='postgresql://…@172.16.120.20:30432/anspire_open_uat'
ontok-introspect --out .review-workspace/req109/qa/schema-src.json

# 本地假 LLM 端点（OpenAI 兼容，逐请求计数）
$env:QA_MODE='fit1'; $env:QA_PORT='18099'; $env:QA_LOG='…/req-fit1.jsonl'
python .review-workspace/req109/qa/fake_llm.py

# AI 开：冷/热两跑（热跑 0 调用）
$env:ONTOK_LLM_BASE_URL='http://127.0.0.1:18099/v1'
$env:ONTOK_LLM_API_KEY='test-only'; $env:ONTOK_LLM_MODEL='qa-fake-fit1'
ontok-propose --schema … --names … --project qa109view --src $env:SRC_URL --llm --budget-tokens 100000

# 测试全量（临时挪开 unix-only bin；跑完复原并核 sha256）
cargo test --lib ; cargo test --doc ; cargo test --test <35 个目标>

# 独立 QA 用例（19 条）+ 变异
cargo test --test qa109_independent
```

**证据目录**：`.review-workspace/req109/`（`qa/` 假端点、产物与独立用例 `qa109_independent.rs`、`qa-test.log`、`mutation.log`）。全部在 `.gitignore` 覆盖范围内，**未进仓库**；独立 worktree 已清理（`git worktree list` 恢复为审核前的 5 个）。

---

## 10 · 一句话给 BA

机制层**全通**：AI 只能动 0.15 的加权项，veto 压顶独立且未被碰；缓存/预算/幂等/关断四项在真进程上逐条可复现；凭据与客户视图干净；578 条测试一条不差。语义质量确实不行（3 条正确的被扣分、`model_id` 根因修不了），但按 A-1/A-5 那是换模型时的事 —— **可合**。
