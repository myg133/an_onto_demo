# QA 审核报告：REQ-112 · 审核界面接入 AI 建议

> **被测**：`feature-REQ-112` / `feature/REQ-112` @ **`444080f`**（基线 `develop` `4178a4e`）
> **审核方式**：独立 worktree（detached `444080f`）+ 自建数据 + 自写探针 + 变异 + A/B
> **日期**：2026-10-21 · **环境**：Windows · rustc/cargo 1.96.0 ·
> PG `postgresql://***@172.17.0.87:5432/anspire_real`（真实数据）· 测试库 `ontok_demo`
> **未改动 feature 分支**（HEAD 仍 `444080f`，`git status` 干净、无 push）

---

## 0 · 结论

**FAIL（1 项阻断）** —— 10 条 AC 里 **9 条通过**，**AC-003 不成立**（被构造样本与端到端双重打穿）。

| # | AC | 结果 | 一句话 |
| --- | --- | --- | --- |
| AC-001 | 纯函数 / 两次调用逐字段相同 | ✅ | 结构上零 I/O，行为上两次调用逐字段相等 |
| AC-002 | 规则表 7 条逐条 | ✅ | 7 条 + ⑧ 全部由**我构造的输入**验过 |
| **AC-003** | **冲突显式、不替人决定、不进白名单** | **❌ 阻断 B-1** | 「5/5 全命中 + AI 判 0.0」⇒ `lean_accept`，**进**批量白名单 |
| AC-004 | 零模型调用 | ✅ | 清空 `ONTOK_LLM_*` 后 3 次 `/api/review` 全 200；`ai_cache` 81→81；8 needle 源码 0 命中 + 反向自检 |
| AC-005 | 建议不写库 | ✅ | `proposal_items` 全 15 列 sha256 **前后逐字节相同**；四审计栏 135/135 NULL |
| AC-006 | 批量先预览 + 白名单 | ✅ | `by=suggestion` ⇒ 38 条 = accept 29 + lean_accept 9；无批量写端点（4 条路由） |
| AC-007 | `none` 不参与批量 | ✅ | 83 个实体全部不在白名单；`excluded.none=83` |
| AC-008 | 页面展示 | ✅（源码级） | 冲突分组在前、建议标签 + 依据（≤40 字，实测最长 26 字）；**无浏览器验证** |
| AC-009 | 与评分卡同源 | ✅ | 5 个常量单处定义；6 组变异全部被抓；HTTP 端改 band 建议跟着变 |
| AC-010 | 不退化 | ✅ | lib 245 / doc 4 / 32 套件 **313** / REQ-111 **14** / 新增 **21**；红线零 diff；两处红灯独立 A/B 复现 |

**阻断项**：`B-1`（AC-003，见 §9）。**其余 5 项非阻断**（`N-1`~`N-5`）。

> ⚠️ 若 BA 把 AC-003 改成「`band = high` 才算冲突」，本报告即降为 **PASS 带非阻断**；
> 但那样做等于**承认这条规则在真实链路上永不触发**（见 §2.4）。

---

## 1 · 建议是纯函数 + 7 条规则逐条（自己构造的输入）

**探针**：`tests/qa112_probe.rs`（我写的 17 个用例，**未复用** Dev 的测试名），
保存在 `.local/qa112/qa112_probe.rs`（gitignored，不进任何分支）。**17/17 通过**。

### 1.1 纯度（AC-001）

| 判据 | 实测 |
| --- | --- |
| 两次 `suggestion(Some(&c))` 逐字段相同 | ✅ 6 组输入（accept / review / lean / low / veto / conflict），`assert_eq!` + 序列化逐字节相同 |
| 建议不改输入 | ✅ 调用前后 `Confidence` 相等 |
| 结构上无 I/O / 时钟 / 随机 | ✅ `confidence.rs` **剥注释后**：`std::time`/`SystemTime`/`Instant`/`std::env`/`env::var`/`rand::`/`std::fs`/`File::`/`TcpStream`/`UdpSocket`/`ureq`/`reqwest`/`tokio::`/`sqlx::`/`Utc::now`/`Local::now` **全部 0 命中** |

### 1.2 7 条规则 + ⑧（每条都是我自己构造的输入）

| # | 我构造的输入 | 实测 `kind` | 实测依据（原文） |
| --- | --- | --- | --- |
| ① | `band=Rejected, 0/5, score=0.18, veto={zero_hit,cap=0.20}` | **reject** | `采样 0/5 全不命中（硬反驳）` |
| ① 优先级 | `band=High, 5/5, fit=1.0, veto={zero_hit}`（**故意与 band 矛盾**） | **reject**（不是 accept） | `采样 0/5 全不命中（硬反驳）` |
| ① | `band=High, 5/5, veto={type_incompatible,cap=0.15}` | **reject** | `类型不相容（硬反驳）` |
| ② | `band=High, 5/5, fit=0.80`（**边界**） | **accept** | `采样 5/5 + AI fit 0.80` |
| ② | `band=High, 5/5, fit=1.00` | **accept** | `采样 5/5 + AI fit 1.00` |
| ③ | **生产评分函数** `score_with_semantic(全结构 1.0, 5/5, fit=0.4)` ⇒ score **0.91** / High | **conflict** | `5/5 命中但 AI 判 0.40 —— 需人工判` |
| ③ 边界 | `band=High, 5/5, fit=0.50`（**恰好不小于 0.5**） | **review**（不是 conflict） | `采样 5/5，AI fit 0.50 未达 0.80` |
| ③ 反例 | `band=High, 4/5, fit=0.20`（非全命中） | **review** | — |
| ④ | `band=Middle, 5/5, fit=0.50` | **lean_accept** | `证据中等（5/5），建议看一眼再签` |
| ⑤ | `band=Low, 2/5, score=0.45` | **review** | `证据不足（分数 0.45）—— 建议人工核对` |
| ⑥ | `band=Rejected, 0/5, score=0.20` | **reject** | `分数 0.20 低于 0.30 —— 建议拒绝` |
| ⑦ | `suggestion(None)` | **none** | `规则抽取，无置信度可依赖` |
| ⑧a | `band=High, 5/5, fit 缺失`（AI 关） | **review** | `采样 5/5 但无 AI 语义判断 —— 建议抽查` |
| ⑧b | `band=High, 5/5, fit ∈ {0.5, 0.6, 0.79}` | **review**（三个都 review） | `… 未达 0.80` |
| ⑧c | `band=High, 无采样, fit 缺失` | **review** | `高档但无采样、无 AI 语义判断 —— 建议人工核对` |

- **规则 ⑧ 符合 BA 裁定 A-2**：高档 + fit 缺失 / fit∈[0.5,0.8) 一律 `review`，**没有**落成 accept。
- 依据文案：**114 条构造输入（4 band × 7 fit × 4 命中形态 + 2 条 veto 用例）+ `none` 分支，全部 ≤ 40 字**，最长 26 字，无空串。
- 真实数据交叉表（`/api/review` 实测）：`high 43 = accept 29（fit=1.0）+ review 14（fit 缺失 8 + fit 0.5 6）`；
  `middle 9 = lean_accept 9（fit 1.0×5 / 0.5×3 / 缺失×1）`；`none 83`。⇒ ⑧ 的 14 条与 BA A-2 的 14 条**逐条对上**。
- `enters_batch()` 矩阵：只 accept / lean_accept 为真；review / reject / conflict / none 全假。
- `Conflict.group_rank() == 0`（页面分组排序依据）。

---

## 2 · 「冲突」的真相（本轮重点）

### 2.1 当前数据分布（我自己统计，`code/out/*.json`）

| 文件 | 带 confidence | band 分布 | fit 分布 |
| --- | --- | --- | --- |
| `proposal.json` | **52** | high 43 · middle 9 | 1.0×34 · 0.5×9 · 缺失×9 |
| `proposal-pending.json` | 18 | low 18 | 1.0×15 · 0.5×1 · 缺失×2 |
| `proposal-rejected.json` | 11 | rejected 11 | 1.0×2 · 0.5×4 · 0.0×3 · 缺失×2 |
| **合计** | **81** | high 43 · middle 9 · low 18 · rejected 11 | **1.0×51 · 0.5×14 · 缺失×13 · 0.0×3** |

> ⚠️ **口径订正（非阻断 N-1）**：这 **81 条不是同一份数据**，是 **3 份不同产物/项目**拼起来的
> （p1 的 52 + `t112_low` 的 18 + 一份 11 条的 rejected 产物）。
> **真正要审的 p1（135 条）里只有 52 条带 confidence**，fit 只有 `{1.0, 0.5, 缺失}`——**一条 0.0 都没有**。
> acceptance A-1 写的「81 条」应标明是合计口径，否则会被读成「同一份 81 条」。

**真冲突：0 条**（两种口径都算了）：

- 严格（AC-002 表）：`band=high && fit<0.5 && matched==sampled` ⇒ **0**
- 不限 band：`fit<0.5 && matched==sampled` ⇒ **0**

`fit=0.0` 的 3 条（全部 `0/5` + 硬 veto + `band=rejected`，即 Dev 的订正成立）：

| 关系 | band | score | 命中 | veto |
| --- | --- | --- | --- | --- |
| `dim_customer_binding→marketing_channel` | rejected | 0.15 | 0/5 | `type_incompatible` |
| `instance_manager→simulate_login_sessions` | rejected | 0.20 | 0/5 | `zero_hit` |
| `marketing_channel_customer→marketing_channel` | rejected | 0.15 | 0/5 | `type_incompatible` |

### 2.2 构造一条真冲突 ⇒ 必须 `conflict` 且**不进**白名单（✅ 通过）

构造方式：**分数与信号保持自洽**（`0.85 + 0.15×0.40 = 0.91` ⇒ `band=high`）。
纯函数层直接走生产评分函数 `score_with_semantic`；端到端层用同一算术写进导入 JSON
（`score=0.91 / band=high / 5/5 / semantic_fit=0.40`，其余信号取真实条目原值）。

- 纯函数层（探针）：`kind = conflict`，`enters_batch() = false`。
- **端到端层**（我建 `qa112c` 项目，导入 5 条构造样本，起真 `review serve`）：

```text
counts = {accept:1, conflict:1, lean_accept:1, none:1, review:1}
#4 QA冲突→客户   kind=conflict  reason=5/5 命中但 AI 判 0.40 —— 需人工判
GET /api/preview?track=biz&by=suggestion&scope=unsigned
  -> count=2  items=[2,3]  excluded={conflict:1, none:1, review:1, decided:0, …}
```

⇒ 冲突**不在**白名单，`none` 也不在。这条 AC-003 的「不替人决定 + 不进白名单」在**这个形状**上成立。

### 2.3 但「真冲突」的触发窗口极窄（这决定了 2.4 的结论）

冲突要求 `band=high`（≥0.90）**且** `fit<0.50`。而 `semantic_fit` 权重是 0.15：

- 所有其它信号拿满时 `score = 0.85 + 0.15·fit`（`round2` 后）；
- ⇒ `fit=0.0` 时分数**上限就是 0.85**（`band=middle`），**不可能**是 high；
- ⇒ 冲突只在 **`fit ∈ [0.30, 0.50)`** 这个窗口里可达（我的探针以 0.01 步长实测：0.30 … 0.49，共 20 个点）。

**而当前模型只给出 `{0.0, 0.5, 1.0}` 三档**（81 条实测分布即如此）——**窗口全空**。
这就是「当前数据 0 条」的**结构性原因**，不是偶然。

### 2.4 对「规则保留但无真实样本」的判断

**只保留规则、不要求真实样本 —— 作为「行为守卫」是可以接受的**，前提是它真的守住了它宣称守的东西。
**但现在没有守住**（⇒ B-1）：

> 这条规则的动机（demand §2、`confidence.rs:1331` 注释）是 **REQ-109 观察到的「5/5 全命中、AI 却判 0.0」**。
> 我把这个形状交给**生产评分函数**：`score_with_semantic(全结构 1.0, 5/5, Some(0.0))` ⇒ **score 0.85 / band middle**
> ⇒ 建议 **`lean_accept`**，**并且 `enters_batch() == true`**。
> 端到端也复现了：`qa112d`（1 条同样的构造样本）⇒ `/api/review` 返回 `lean_accept`，
> `/api/preview?by=suggestion` **count=1 且包含它**。

⇒ 「程序不许替人决定」在**它自己点名的那个案例**上没有生效：批量「按建议接受」会把它一起接受。

**若 BA 认为必须用真实数据验证，怎么造出真实样本（且不伪造数据）**：

1. **可行且不伪造**：对同一份**真实**源库重新跑 `ontok-propose --llm`，换一个模型/提示词版本，
   找到一条「采样本就 5/5 命中」的关系让模型给出 **fit ∈ [0.30, 0.50)** 的判断。
   这时 score / band / signals 全部由生产管线算出 —— 是**新观测**，不是造数。
   代价：需要 `ONTOK_SOURCE_DATABASE_URL` + LLM 预算（本环境没有，故本轮无法执行）。
2. **当前提示词下（输出 0/0.5/1.0 三档）做不到**：窗口 [0.30,0.50) 落在两档之间，
   只要模型仍按 `extract/ai_semantic.rs` 的提示词回答，**真实冲突样本会一直是 0 条**。
3. ⛔ 不能做的：改 `semantic_fit` / 改 `score` / 改 `matched` 让它「看起来像冲突」——
   那是伪造数据，只会把一条死规则伪装成已验证。

**结论**：`G1` 的登记是诚实的，但**只登记「无样本」不够** —— 还要登记「**这条规则在当前评分算术下几乎不可达**」，
否则下一个人会以为它是一道有效的安全阀（见 N-2：即使 band=high 的冲突，也还有另一条批量口径能绕过）。

---

## 3 · 零模型调用（AC-004）✅

**(a) 环境**：起 `review serve` 前清空全部 `ONTOK_LLM_*`（实测清空后计数 **0**），
无 `OPENAI_*` / `ANTHROPIC_*`。

**(b) 端到端**：`GET /api/review` 连续 3 次 → **HTTP 200**，`items=135`，**每条都带 `suggestion`**。

**(c) token 计数**：`ai_cache` 表请求前后 **81 → 81**（AI 调用必落 `ai_cache`，零新增 =
零真实调用）。整轮 QA 期间也没有任何 LLM 端点配置被写入。

**(d) 源码面**（剥注释后扫，8 个 needle：`ONTOK_LLM` / `ai_semantic` / `chat/completions` /
`ureq` / `reqwest` / `llm` / `openai` / `anthropic`）：

| 扫描面 | 命中 |
| --- | --- |
| `src/review_ui/http.rs` / `mod.rs` / `page.rs` | **0** |
| `src/domain/confidence.rs` | 1 —— **假阳性**：`assert_eq!(name_match_value("model_id", "llm_model"), 0.6, "其它")`（单元测试里的**字符串字面量**，不是调用点） |

**(e) 反向自检（证明扫描器不是恒真）**：

| 文件 | 命中 |
| --- | --- |
| `src/extract/ai_semantic.rs` | `ONTOK_LLM`×1、`llm`×26（共 27） |
| `src/agent/llm/transport.rs` | `chat/completions`×1、`ureq`×6、`openai`×1、`llm`×6 |
| `src/agent/llm/protocol.rs` | `chat/completions`×1、`llm`×5 |
| `src/agent/factory.rs` | `chat/completions`×1、`openai`×1、`llm`×11 |

⇒ 扫描器对 8 个 needle 都**能**命中真实调用点；建议链路上确实没有 LLM 调用。

---

## 4 · 零写库（AC-005）✅

对 `qa112` 的 `proposal_items` **全部 15 列**、按 `seq,id` 排序后整体 JSON 取 sha256：

```text
before = 3A33B0D91BD4678C36F1BB559948AFCFF8D33F91B7B7D72301E7EC07F9EC6612
after  = 3A33B0D91BD4678C36F1BB559948AFCFF8D33F91B7B7D72301E7EC07F9EC6612   ← 逐字节相同
rows=135 · 四审计栏（biz_decided_at / biz_via / model_decided_at / model_via）全 NULL = 135/135
biz_status / model_status = 全 pending
```

「after」覆盖了 **3 次 `/api/review` + 2 轨 `/api/preview` + 4 个批量写端点探测（405）** 之后。
⇒ 拉建议**一个字节都没写**，`decided_at` / `via` 仍 NULL。

---

## 5 · 批量白名单（AC-006 / AC-007）✅

| 判据 | 实测 |
| --- | --- |
| `GET /api/preview?track={biz,model}&by=suggestion&scope=unsigned` | 两轨都 **200**，`count=38`、`len(items)=38`、`by="suggestion"`、`total=135` |
| `items[]` 只含 accept / lean_accept | ✅ `{accept:29, lean_accept:9}`，无第三类 |
| 只含**未签** | ✅ 0 条已签（全 pending），独立复算集合与 `items[]` **完全相等** |
| `none`（83 实体）不在白名单 | ✅ 交集 **0** |
| `conflict` 不在白名单 | ✅ 真实数据 0 条；**构造样本**（qa112c #4）也不在（§2.2） |
| `count == len(items)` | ✅ 38 == 38 |
| 排除项可见 | ✅ `excluded={none:83, review:14, conflict:0, reject:0, decided:0, below_band:0, no_confidence:0}` |
| 口径不许混用 | ✅ `by=suggestion&min_confidence=0.9` → **400**（「两种口径混在一起…没有唯一含义」）；`by=nonsense` → 400 |
| **存在任何「批量写」端点吗？** | **不存在**。`build_router` 只有 4 条路由：`GET /`、`GET /api/review`、`GET /api/preview`、`POST /api/decide`；逐个探测 `POST /api/decide-bulk`、`/api/bulk`、`/api/accept-all`、`/api/apply` → **405** +「只有 GET…/POST /api/decide 三个接口」 |
| 页面侧 | `runBatch` 是**逐条** `POST api/decide`，没有批量调用；页面不含 `decide-bulk` |

---

## 6 · 与评分卡同源 + 变异（AC-009）✅

### 6.1 常量只有一处定义

`src/domain/confidence.rs`：

```text
:210 BAND_HIGH   = 0.90      :248 Band::of() 用 BAND_HIGH / BAND_MIDDLE / BAND_LOW
:212 BAND_MIDDLE = 0.60      :1557 classify() 用 FIT_ACCEPT / FIT_CONFLICT
:214 BAND_LOW    = 0.30      :1573 依据文案里的 0.80 也是 ${FIT_ACCEPT:.2}
:223 FIT_ACCEPT  = 0.80
:225 FIT_CONFLICT= 0.50
```

`Band::of` 与 `classify` **共用同一批常量**（不是两处字面量）；`/api/review` 的 `confidence.band`
与建议读的是**同一个值**（`suggestion_of` 只解析、不重算）。

> 非阻断 N-4：`page.rs:313` 仍硬编码 `sc >= 0.9 / 0.75` —— 那是**档位徽章配色**（REQ-111 既有），
> 不是建议规则；建议链路上一处阈值都没有。

### 6.2 变异（6 组，全部被抓；每次改完 `git checkout` 还原）

| # | 变异 | 抓到它的用例 |
| --- | --- | --- |
| M1 | `BAND_HIGH 0.90 → 0.95` | `rule3_high_full_hit_and_fit_below_050_is_conflict`、`ac003_conflict_window_…`（2 failed） |
| M2 | `FIT_ACCEPT 0.80 → 0.90` | `rule2_high_and_fit_ge_080_is_accept`、`ac003_conflict_window_…`（2 failed） |
| M3 | `FIT_CONFLICT 0.50 → 0.30` | `rule3_…`、`ac003_conflict_window_…`（2 failed） |
| M4 | `enters_batch` 放进 `Review` | 我的 `ac006_enters_batch_matrix`；**Dev 套件 3 条红**：`high_band_without_semantic_judgement_is_review_not_accept`、`rule5_low_band_is_review`、`ac006_preview_by_suggestion_lists_only_accept_and_lean_accept` |
| M5 | 冲突规则整条失效（`if false && …`） | `rule3_…`、`ac003_conflict_window_…` |
| M6 | `zero_hit` 匹配臂改名（硬反驳失效） | `rule1_veto_wins_over_high_band`、`rule1_veto_zero_hit_is_reject` |

### 6.3 改 band ⇒ 建议跟着变（端到端）

直接改 `qa112c` #2 的 `payload.confidence.band`（真实 HTTP 面，改完还原）：

```text
high     -> accept       采样 5/5 + AI fit 1.00
middle   -> lean_accept  证据中等（5/5），建议看一眼再签
low      -> review       证据不足（分数 1.00）—— 建议人工核对
rejected -> reject       分数 1.00 低于 0.30 —— 建议拒绝
```

---

## 7 · 不退化（AC-010）✅

### 7.1 数字（我自己跑的，逐套件）

```text
lib  cargo test --lib  -> 245 passed / 0 failed
doc  cargo test --doc  ->   4 passed / 0 failed
32 个基线套件            -> 313 passed（逐套件与基线计数**逐项相等**，见下表）
REQ-111 test_req111_review_ui -> 14
本轮新增 test_req112_suggestion -> 21（文件里 22 个 #[test] 属性，其中 1 个在注释里）
另有 test_req110_ttl 15 / test_read_only_guard 5 / test_domain_print_sites_binary_req078 4 /
     test_req109_cli_gate 6 / test_project 10 / test_hierarchy 16 /
     test_snapshot_freeze_gate 2 / test_review_show_binary_req072 7 / test_anchor_guard 3
合计 43 个套件 416 passed / 0 failed / 0 ignored
```

32 个基线套件逐项（全部 `passed == 基线`）：

```text
req107_outlet_pipeline 12 · req107_guard_granularity 4 · req107_zero_visible_change 3 ·
req107_pipeline_bytes 1 · req107_scope_error_gate 4 · req107_track_manifest 2 ·
req107_static_texts 3 · req107_red_lines 2 · property_filter 10 · exit_ordering_req106 7 ·
binary_output_guard 7 · graph_server 9 · guard_manifest_req081 6 · apply 21 ·
review_show_e2e 11 · review_state 28 · db 11 · extracted_by_provenance 9 ·
review_provenance 17 · concept_proposal 19 · diff 13 · no_write_path 4 ·
provenance_source 5 · overview 17 · req108_live 5 · req108_confidence 10 ·
req107_source_enumeration 10 · snapshot 10 · snapshot_write_paths 8 · model_view 10 ·
view 26 · proposal 9   = 313
```

**结构面**（不靠跑）：`git ls-tree` 对比 `4178a4e` ↔ `444080f`
—— **测试文件 0 删除**，共同文件 `#[test]` 总数 **794 → 794 逐文件相等**，新增 1 个文件。
新增 `#[ignore]` **0**、删除 `#[test]` **0**、`Cargo.toml`/`Cargo.lock` diff **0 行**。

**红线/禁改**（`git diff --numstat develop..HEAD -- <path>` 全为空）：
`src/mcp/tools.rs`、`src/domain/property_filter.rs`（G1–G4）、`src/server/http.rs`（serve）、
`src/domain/view.rs`、`src/domain/review_view.rs`、`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、`Cargo.*`。
本轮 diff 只有 4 个文件：`confidence.rs` / `review_ui/http.rs` / `review_ui/page.rs` / 新测试。

### 7.2 两处红灯的独立 A/B

绕行方式（与要求一致，`finally` 挪回）：`src/bin/ontok-gate-preflight.rs` 移出 → 逐套件跑 → 挪回。

```text
PREFLIGHT sha256_before = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
PREFLIGHT sha256_after  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
PREFLIGHT Test-Path=True · Hold-empty=True · identical=True
```

**红灯 ① `test_retained_names_req084` 跑不了 —— 归因成立（不是退化）**

我自己在绕行状态下编它，得到**编译错误**（不是断言失败）：

```text
error: couldn't read `tests\../src/bin/ontok-gate-preflight.rs`: 系统找不到指定的文件。
  52 | const SRC_PREFLIGHT: &str = include_str!("../src/bin/ontok-gate-preflight.rs");
error: could not compile `ontok` (test "test_retained_names_req084")
```

且 `git show 4178a4e:tests/test_retained_names_req084.rs` 里**同一行原样存在** ⇒ 既有问题，
本轮对该文件 **0 改动**，不在 32 个基线套件内。**不构成退化**。

**红灯 ② `test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` —— 归因成立（基线红）**

我的 A/B（同一环境、同一行、无 `ONTOK_SOURCE_DATABASE_URL`、未设任何 `ONTOK_LLM_*`）：

| 侧 | 代码 | 结果 |
| --- | --- | --- |
| A | `444080f`（`src/` 全量） | **FAILED**：`left(CLI 产物)` 缺 `add_link_type 设备→客户`，`right(进程内 legacy)` 有 —— 断言点 `tests/test_agent_orchestration.rs:1070` |
| B | `src/domain/confidence.rs` + `review_ui/*` **还原到 `4178a4e`** | **FAILED**，**同一行、同一差异**（同样缺 `add_link_type`） |

⇒ 与本提交无关；成因是「采样默认开 + 本机无源库」时 CLI 关系条目落 pending（不写进产物），
既有的端到端判据因此红。**不构成退化**。

---

## 8 · 页面（AC-008，源码级；如实登记无浏览器验证）

| 判据 | 实测 |
| --- | --- |
| 「冲突」单独分组、排最前 | ✅ 源码：`conflict` 先 `push`、`rest` 后 `push`；`groupHead("⚠ 冲突 …")` 位于 `groupHead("其余条目")` **之前**（offset 14881 < 15065），且先 append 冲突组 |
| 每条显示建议标签 | ✅ `"建议：" + suggLabel(sg.kind)`（六种中文标签映射） |
| 每条显示依据文案（≤40 字） | ✅ `"依据：" + sg.reason` **单独一行**（不只藏 tooltip），同时写 `title`；`/api/review` 里 reason 非空、最长 26 字 |
| 分组计数与后端一致 | ✅ 计数直接读 `S.data.suggestion_counts`（后端发的那份），页面不自己数 |
| 建议只在后端算一份（纪律 H） | ✅ 页面不含 `semantic_fit` / `matched == sampled` / `enters_batch` / `fit >= 0.8` / `BAND_HIGH`；只做「后端 kind → 中文标签 / 分组 / 筛选」 |
| 批量先显示条数再逐条 POST | ✅ 弹窗「按建议接受 N 条」+ 排除项（冲突/人工/拒绝/无建议各 N 条），确认后 `runBatch` **逐条** POST |
| **浏览器验证** | ⛔ **无**（本机无 headless browser，且真实数据无冲突样本）。以上全部是**源码级断言**，按 A-3 如实登记 |

---

## 9 · 阻断项 / 非阻断项

### 🔴 B-1（阻断）· AC-003 在「5/5 全命中 + AI 判 0.0」上不成立

**事实（三层证据）**：

1. 生产评分函数：`score_with_semantic(全结构 1.0, 采样 5/5, semantic_fit=0.0)` ⇒ **score 0.85 / band middle**（`0.85 = 1 − 0.15`，语义项权重 0.15）。
2. 纯函数：`suggestion(该 confidence)` ⇒ **`lean_accept`**，`enters_batch() == true`。
3. 端到端（`qa112d` 真 `review serve`）：`/api/review` ⇒ `lean_accept`（依据「证据中等（5/5），建议看一眼再签」）；
   `/api/preview?track=biz&by=suggestion` ⇒ **count=1 且包含它**。

**违反的判据**（acceptance AC-003 原文）：

- 「`matched == sampled` 且 `semantic_fit < 0.5` ⇒ `kind = conflict`」—— 实测是 `lean_accept`；
- 「该条**不进入**「按建议批量接受」的白名单」—— 实测**进入**。

> 顺带一条更直接的证据：**`demand.md` §目标 里自己举的第 3 行示例**
> （`[88] 客户返点→客户  建议：人工  依据：采样 5/5 支持，但 AI 判定 0.00 —— 证据与语义冲突`）
> 按现在的实现**拿不到「人工」**：同样的形状算出来是 0.85 / middle ⇒ `lean_accept`。需求正文的示例本身就复现不出来。

**根因（不是 Dev 写错，是规格自相矛盾）**：

- `demand.md §2` 与 `acceptance.md` 的 **AC-002 表**把冲突限定在 **`band = high`**；
- **AC-003 的正文没有 `band` 条件**；
- 而算术上 `fit = 0.0` 时分数最高 **0.85**（永远到不了 high）⇒ 两条 AC 在 `fit ∈ [0, 0.30)` 上**互斥**。
- 这条规则的全部动机就是 REQ-109 那个 `0.0` 案例 —— 也就是说：**按 AC-002 实现的冲突规则，恰好覆盖不到它自己要防的案例**。

**建议（二选一，需 BA 拍板）**：

| 选项 | 动作 | 代价 |
| --- | --- | --- |
| **A（推荐，保 AC-003）** | `classify()` 在档位分派**之前**加一道闸：`matched==sampled && sampled>0 && fit<FIT_CONFLICT && band∈{High, Middle}` ⇒ `conflict` | 会与 AC-002 的「middle ⇒ lean_accept」冲突，需同步改 AC-002 第 4 行；**当前真实数据分布不变**（p1 的 9 条 middle 里 fit 只有 1.0×5 / 0.5×3 / 缺失×1，**没有 `<0.5` 的行**） |
| **B（保 AC-002）** | 把 AC-003 改成「`band=high` 时…」，并**删掉** demand §2 / `confidence.rs:1331` 注释里「5/5 却被判 0.0 ⇒ 冲突」的叙事 | 承认这条规则**在当前模型输出（0/0.5/1.0）下永不触发**，G1 从「暂无样本」升级为「结构性不可达」，安全阀名存实亡 |

> 若选 B，本报告降级为 **PASS 带非阻断**；阻断项随之关闭。

### 🟡 非阻断

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| **N-1** | 数据口径 | acceptance A-1 的「81 条」= p1 52 + `t112_low` 18 + rejected 产物 11，**三份不同数据**；真正待审的 p1 只有 52 条且无 `fit=0.0`。建议在 G1 里写明「合计口径」与「p1 口径」。 |
| **N-2** | 冲突还有第二条批量口径能绕过 | `GET /api/preview?track=biz&min_confidence=0.9`（REQ-111 的「按置信度档位」模式）在 `qa112c` 上返回 `count=3 items=[2,4,5]` —— **包含 score=0.91 的冲突条目 #4 与 review 条目 #5**。按 AC-003 字面（「按建议批量接受的白名单」）不算违规，但「冲突永不被自动接受」这句话只对 `by=suggestion` 成立。建议页面在「按档位」模式下对 conflict/review 加显式警示。 |
| **N-3** | 交付物缺失 / A-3 未落地 | `.feature/verification-report.md` 仍是 **REQ-111 的报告**（795 行，`git grep REQ-112` **0 命中**），`.feature/status.md` 还是 REQ-002 的旧内容。A-3 要求交付物写明「本数据集无冲突样本」「页面无浏览器验证」——前者只在 BA 的 `status.md` 里出现，后者**无处登记**。建议补一份 REQ-112 交付说明（或在本报告 + status.md 里登记）。 |
| **N-4** | 页面档位阈值第二处写法 | `page.rs:313` 硬编码 `sc >= 0.9 / 0.75` 给档位徽章上色（REQ-111 既有）。建议意见：建议链路没受影响，但「阈值只有一处」这句只对建议成立。可后续把徽章也吃 `BAND_*`（需前端拿到常量，属小改）。 |
| **N-5** | 冲突分组只有源码级验证 | 与 A-3 一致；建议 BA 在验收记录里保留「无浏览器验证」这一条，不要把源码断言写成「已验证页面」。 |

---

## 附录 A · 复现命令（我实际执行过的）

```powershell
# 0) 独立 worktree（detached，不碰 feature 分支）
git -C feature-REQ-112 worktree add --detach ..\qa-REQ-112 444080f

# 1) 测试数据（anspire_real 库）：project new qa112 -> version new -> 改 project 字段后导入
$env:ONTOK_DATABASE_URL='postgresql://***@172.17.0.87:5432/anspire_real'
ontok project new --name qa112 ; ontok version new --project qa112
# proposal.json 的 project: anspire_real -> qa112
ontok propose import --file .local\qa112\proposal-qa112.json     # 135 条

# 2) 零模型调用 + 零写库（清空 LLM 环境变量后起服务）
ontok review serve --project qa112 --proposal p1 --bind 127.0.0.1:8791 --token 'qa112-local-token'
#    GET /api/review  ×3        -> 200，135 条全带 suggestion
#    GET /api/preview?track=biz&by=suggestion&scope=unsigned -> count=38
#    proposal_items 全列 sha256 前后 = 3A33B0D9…6612；ai_cache 81 -> 81

# 3) 7 条规则（我的探针，17 个用例）
$env:CARGO_TARGET_DIR='..\qa-REQ-112-target'
cargo test --test qa112_probe            # 17 passed

# 4) 构造冲突（qa112c：5 条）与「5/5 + fit 0.0」（qa112d：1 条），各起一次 review serve
#    qa112c -> conflict 不在白名单（count=2）；qa112d -> lean_accept 且在白名单（count=1）

# 5) 变异（6 组，见 §6.2）+ 端到端改 band（见 §6.3）
# 6) 回归（绕行挪开 unix-only bin，finally 挪回）
cargo test --lib ; cargo test --doc ; 逐个 cargo test --test <suite>
```

运行后已清理：删掉 `qa112` / `qa112c` / `qa112d`（items 141 / proposals 3 / versions 3），
`anspire_real` 的 p1 仍是 135 条 `applied`，`ai_cache` 仍 81 条；QA worktree 与其 target 已删除，
`src/bin/ontok-gate-preflight.rs` 已归位（sha256 与挪开前一致）。

## 附录 B · 未改动的声明

- feature 分支 HEAD **仍 `444080f`**，`git status` 干净，**未 push**。
- 所有变异/构造/还原都在 **QA 独立 worktree** 与 **`qa112*` 临时项目** 上完成，已清理。
- 本报告不写入 `status.md`（状态由 BA 更新）。
