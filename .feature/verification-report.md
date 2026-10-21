# REQ-111 · 本地审核界面（HTML 里做双轨审核）—— 交付报告

> 分支 `feature/REQ-111`（worktree `feature-REQ-111/`，基线 `develop` `f624c1a`，REQ-110 已合并）· **未 push**
> 一句话：**把「142 条目 × 2 轨 = 284 次进程启动」收成「一个进程 + 页面上的点击」**，
> 而「`serve` 只读」这条**结构**保证一个字节都不动。
>
> ⚠️ 三条必须先看的结论：
> 1. 决策**只有一个实现**：`db::proposal::decide_item`（`review decide` 与 `POST /api/decide` 都调它，只差一个 `via` 参数）；
> 2. `src/server/http.rs` **零 diff**（`git diff develop -- src/server/http.rs` 输出 0 行），`serve` 的 405 判据原样全绿；
> 3. 批量**没有**服务端写接口：预览拿 `items[]` 白名单，页面逐条 POST ⇒「一键全库」在 HTTP 面上**表达不出来**。

---

## 0 · 结论先行

| 项 | 结果 |
| --- | --- |
| 10 条 AC | **10 条通过**（逐条实测见 §3） |
| 决策实现份数 | **1**（`decide_item`；`review_ui` 里 `UPDATE/INSERT/DELETE/sqlx::query` **0 命中**） |
| `serve` 改动 | **0 行**（路由 + middleware 路径过滤的 `git diff` 为空） |
| 耗时（135 条目 × 2 轨 = 270 次决策） | CLI **59.59 s** ↔ 页面路径 **7.69 s**（**7.75×**） |
| 幂等 | ✅ accept → reject 后终态 `rejected`，条目行数不变（135） |
| 双向一致 | ✅ 页面签 ⇒ `review show` 读到；CLI 签 ⇒ `GET /api/review` 读到（`via=cli`） |
| 冻结拒绝 | ✅ 启动 `exit=1`（点名 `v1 已冻结`）；运行中提交 **HTTP 409** + 具体原因 |
| 已应用提案 | ✅ 服务运行中被 `proposal apply` 后，再 POST ⇒ **HTTP 409** +「已 applied…不能再改判」（QA N9；`apply_after_start_is_a_conflict_and_leaves_the_rows_alone`），库中决策与 apply 时逐行一致 |
| 审计 | ✅ 每轨 `decided_at` + `via`（页面 `ui` / CLI `cli`） |
| 测试 | lib **245**（= 基线）· doc **4**（= 基线）· 32 个基线套件 **313**（**= 基线，一条不少**）· 新增套件 **14** · 合计 36 套件 **351** |
| 新依赖 | ✅ **零**（`Cargo.toml` 未改） |
| 红线 | ✅ 四红线 + `tools.rs` + `guard()` + `property_filter` G1–G4 **零改动**（`git diff --numstat` 为空） |

> 耗时口径：上表的「页面路径 **7.69 s**（7.75×）」是**页面发出的 270 个 HTTP 请求**的墙钟，
> **不含浏览器渲染 / 点击间隔** —— 把页面真正点一遍还要加上人手时间（见 §4 的口径说明）。

---

## 1 · 各文件改动（`path:line`）

| 文件 | 行 | 内容 |
| --- | --- | --- |
| `src/cli.rs` | `261-294` | `ReviewCmd::Serve { project, proposal, bind, token }`（默认 `127.0.0.1:8788`；`--token` 可省，回落 `ONTOK_VIEW_TOKEN`） |
| `src/cli.rs` | `1487-1493` | `review decide` 改为调 **`decide_item(..., VIA_CLI)`** —— 不再自己查条目 + 写库 |
| `src/cli.rs` | `1496-1511` | `Serve` 分支：解析监听地址 → 取口令 → `review_ui::http::serve` |
| `src/cli.rs` | `1948` / `2016` | `resolve_proposal` / `proposal_alias` 改 `pub`（审核界面复用**同一份**别名词典，不抄 SQL） |
| `src/db/proposal.rs` | `36-38` / `68-69` | `VIA_CLI = "cli"` / `VIA_UI = "ui"`（审计入口标识） |
| `src/db/proposal.rs` | `41-61` | `ProposalItemRow` 增 4 栏：`biz_decided_at` / `biz_via` / `model_decided_at` / `model_via`；`ITEM_COLUMNS` 一处列清单供两处 SELECT 共用 |
| `src/db/proposal.rs` | `317-339` | **`decide_item`** —— 序号→条目→`domain::review::apply_decision`→落库+审计，**唯一实现** |
| `src/db/proposal.rs` | `347-378` | **`set_decision_via`** —— 全仓**唯一**的决策 `UPDATE`（`decided_at = now()` 由数据库取时） |
| `src/db/proposal.rs` | `388-396` | `set_decision`（旧签名）→ 委托 `set_decision_via(..., VIA_CLI)`，`apply` / 既有测试零改动 |
| `src/db/pg.rs` | `267` | `StoreError::ItemNotFound { seq }`（CLI 与页面**同一句话**） |
| `src/db/pg.rs` | `487-515` | `ALTER_PATCHES` 9→13：两轨各 `*_decided_at` + `*_via` |
| `src/domain/pipeline.rs` | `308-320` / `376-380` | `OutletId::R1–R5` + `spec_of`（**Internal** 轨、`InternalOnly`） |
| `src/domain/pipeline.rs` | `1148-1202` | `OUTLETS` 登记 5 条新出口：读 / 写回执 / 批量预览 / 页面壳 / 错误通道（各带 needle + 豁免理由） |
| `src/lib.rs` | `25-31` | `pub mod review_ui;`（**不进 `server/`**，理由见 `review_ui/mod.rs`） |
| `src/review_ui/mod.rs` | 全文 | 模块边界 + 三条安全边界 + 已知限制 |
| `src/review_ui/http.rs` | `96-176` | `prepare` —— 四道启动闸门（项目/提案归属、pending、基版本未冻结、快照可读） |
| `src/review_ui/http.rs` | `179-219` | `serve` —— **只绑回环** + 口令长度 + 横幅（打印绑定地址与提案别名） |
| `src/review_ui/http.rs` | `222-246` | `build_router`：`/`、`GET /api/review`、`GET /api/preview`、`POST /api/decide`、fallback |
| `src/review_ui/http.rs` | `252-287` | `check_token`（每个 `/api/*` 的第一行）+ 定长比较 |
| `src/review_ui/http.rs` | `290-430` | `GET /api/review`：条目 + 双轨状态 + 置信度（含 AI 理由）+ 证据 + 候选定义 + 进度 + `frozen`（版本行真实状态，QA N2） |
| `src/review_ui/http.rs` | `415-508` | `GET /api/preview`：**条数** + `items[]` 白名单 + 排除项计数（只读） |
| `src/review_ui/http.rs` | `532-604` | `POST /api/decide`：**唯一写点** → `decide_item(..., VIA_UI)` → 回执（状态 + 审计 + 进度） |
| `src/review_ui/http.rs` | `615-652` | `ensure_unfrozen` —— **每次写前**再查「提案仍 `pending`」（QA N9）+「基版本未冻结」（AC-007） |
| `src/review_ui/page.rs` | `25-` | 独立 HTML（零外部资源、零数据壳）：列表 / 过滤 / 逐条签 / **批量先预览** / 进度条 |
| `tests/test_req111_review_ui.rs` | 全文 | 14 条判据（AC-001~011 的行为面 + 真二进制双向一致 + QA N9/N2 回归） |
| `tests/test_req107_outlet_pipeline.rs` | `354-359` / `678-720` | 登记对视对账**纳入** `src/review_ui/http.rs`；条数 39→44、内部 9→14、ID 集合 +R1–R5 |
| `tests/test_req107_source_enumeration.rs` | `1818-1822` | `MODULES` 登记 `review_ui` / `http` / `page` |
| `tests/test_req108_confidence.rs` / `test_req109_ai_semantic.rs` | 各 +5 行 | `ProposalItemRow` 字面量补 4 个审计栏（全部 `None`） |

**未改**：`src/server/http.rs`（0 行）、`src/server/page.rs`、`src/mcp/*`、`Cargo.toml`、四红线、`tools.rs`、`property_filter.rs`。

---

## 2 · 决策复用的证明（纪律 H）

### 2.1 同一个函数、两个调用点

```text
src/db/proposal.rs:317   pub async fn decide_item(                      ← 定义（唯一实现）
src/cli.rs:1492                  .decide_item(p, item, t, d, VIA_CLI)   ← ontok review decide
src/review_ui/http.rs:564        .decide_item(state.proposal_id, ..., VIA_UI)  ← POST /api/decide
src/db/proposal.rs:338           self.set_decision_via(...)             ← 唯一的落库调用
src/db/proposal.rs:347   pub async fn set_decision_via(                 ← 唯一的决策 UPDATE
src/db/proposal.rs:394           self.set_decision_via(..., VIA_CLI)    ← 旧签名兼容（apply / 既有测试）
```

`git grep -n "decide_item("` 恰好 **3 处**：定义 + CLI 调用 + HTTP 调用（源码注释里那句说明不计）。
`decide_item` 里四步一步不少：**序号解析 → 条目行 → `domain::review::apply_decision`（纯状态机）→ `set_decision_via`（落库 + 审计）**。

### 2.2 HTTP 层没有第二份决策 / 没有 SQL

```text
$ Select-String -Path src/review_ui/*.rs -Pattern "UPDATE |INSERT |DELETE FROM|sqlx::query|set_decision|apply_proposal"
（仅命中两行模块注释；代码 0 命中）
```

页面能写的**只有** `POST /api/decide`，它调的是 `decide_item`；自身不碰 `PgProposalStore` 之外的任何写方法，也没有 `UPDATE` 字面量。

### 2.3 为什么审计不另开日志表

AC-005 要求「同 `item+track` 连签**不产生两条记录**」，AC-009 又要求每次决策有 `decided_at` / `via`。
⇒ 审计栏放在**同一条目行**上（两轨各两栏），后写覆盖前写。追加一张 `decisions_log` 会让这两条判据互相打架。

---

## 3 · 10 条 AC 逐条实测

| AC | 判据 | 实测 | 证据 |
| --- | --- | --- | --- |
| **AC-001** 只绑回环 | `0.0.0.0` ⇒ exit≠0；`127.0.0.1` ⇒ 正常 | ✅ | `--bind 0.0.0.0:8790` → **exit=1**，文案「只允许绑回环地址…审核界面会**写**决策」；`127.0.0.1:8788` → 起服务并打印横幅；`ac001_non_loopback_bind_is_refused`（含回环对照组） |
| **AC-002** 只服务未冻结 | 已冻结 ⇒ 启动失败、点名 `v2 已冻结` | ✅ | 冻结 `req111_live` v1 后重启：**exit=1**「项目「req111_live」的 v1 已冻结（2026-10-21T13:57:23Z）：审核界面只服务**未冻结**的提案…」；`ac002_frozen_base_version_refuses_to_start` |
| **AC-003** 写接口鉴权 | 无 token 401 / 正确 token 200 | ✅ | 实测 `GET /api/review` 无 token **401**、`POST /api/decide` 无 token **401**、带 token **200**；401 后库中仍 `pending`（未写入）；`ac003_both_apis_require_a_token` |
| **AC-004** 同一个领域函数 | 页面签 ⇒ CLI 读得到；反向一致 | ✅ | 页面签 item 2 `biz` ⇒ `review show` 里 `biz=accepted` 计数 **134**（另一条是 item 1 的 rejected）；CLI 签 item 3 `model --reject` ⇒ `GET /api/review` 读回 `model=rejected`；`ac004_page_signs_then_the_cli_reads_it_and_back`（**真二进制**） |
| **AC-005** 幂等覆盖 | accept→reject 终态 reject、不产生两条记录 | ✅ | 实测 item 1 `biz` accept→reject：回执 `biz=rejected`、`via=ui`；`GET` 里**条目总数仍 135**；`ac005_signing_twice_overwrites_and_keeps_one_row`（并断言 `review show` 里不再有 `biz=accepted`） |
| **AC-006** 批量先预览 | 先显示条数；批量接口带 `items[]` 白名单 | ✅ | 未签提案实测：`GET /api/preview?track=biz&min_confidence=0.9` → **count=39**（below=13、no_conf=83）；`model&≥0.60` → **52**；按白名单接受 3 条后 count **36**、decided=3；页面壳含「将接受 N 条」与 `api/preview`，**不含** `api/decide-bulk`；`ac006_batch_preview_shows_the_count_and_the_whitelist`（10 条未签 ⇒ count=3） |
| **AC-007** 失败可见 | 冻结后提交 ⇒ 非 2xx + 具体原因 | ✅ | 服务运行中冻结 ⇒ `POST /api/decide` **HTTP 409**，正文 `{"error":"项目「req111_live」的 v1 已冻结，不能再改判 —— 审核界面只服务未冻结的提案。"}`；且条目仍 `pending`；`ac007_write_after_freeze_is_conflict_with_the_concrete_reason` |
| **AC-008** 与 serve 隔离 | `serve` 一行未改；POST 到 serve 仍 405 | ✅ | `git diff develop -- src/server/http.rs` **0 行**；405 判据原样在跑：`change_verbs_are_405_on_registered_api_paths` / `every_405_carries_a_human_message`（lib 245 全绿）+ `test_graph_server` 9 全绿 |
| **AC-009** 审计可区分 | `decided_at` + `via`（`cli`/`ui`） | ✅ | `ac009_audit_distinguishes_ui_from_cli`：页面签 ⇒ 库 `biz_via="ui"`、`biz_decided_at` 非空、`model_via` 仍 NULL；CLI 签 ⇒ `model_via="cli"`；实测 API 回执 `via.model=cli` |
| **AC-010** 不退化 | 基线一条不少；禁改禁删 | ✅ | lib **245** / doc **4** / 32 套件 **313** 全部 = 基线；四红线 + `tools.rs` + `property_filter` 的 `git diff --numstat` 为空；无 `#[ignore]`、无删测试、`Cargo.toml` 未改 |

> AC-011 不存在（验收只有 AC-001~010）；新套件 14 条是**行为判据**，不是新 AC。

---

## 4 · 耗时对比（同一份 135 条目提案，逐条签 × 两轨）

| 路径 | 命令/请求 | 决策数 | 墙钟 | 单次均值 | 失败 |
| --- | --- | --- | --- | --- | --- |
| **CLI** | `ontok review decide --item N --track biz\|model --accept`（逐次进程启动） | 270 | **59.59 s** | 220.7 ms | 0 |
| **页面路径** | `POST /api/decide`（同一个进程、同一条连接复用，**页面发的就是这串请求**） | 270 | **7.69 s** | 28.5 ms | 0 |

- **7.75×**。CLI 的瓶颈是**每次进程启动**（连接池 + `migrate()` 的列存在性探询 + 进程加载），
  而不是决策本身；页面路径把这些做了一次。
- 两条路径用的是**两份内容相同的提案**（135 条目，同一 `ai-proposal.json`：83 实体 + 52 关系），
  避免「第二次签已签过的条目」影响读数。
- ⚠️ **口径如实说**：本机没有 headless 浏览器，「页面」的墙钟是**页面发出的那 270 个 HTTP 请求**
  从开始到结束的时间（不含浏览器渲染/点击间隔）。把页面真正点一遍还要加上人手的点击时间 ——
  那部分对 270 次决策是分钟级，**不在本数字内**。

---

## 5 · 「`serve` 一行未动」的证据

```text
$ git diff develop --stat -- src/server/http.rs
（空）

$ git diff develop -- src/server/http.rs | Measure-Object -Line
Lines: 0
```

结构上的原因：审核界面是**新模块** `src/review_ui/`，`serve` 的 `AppState` / `build_router` /
`require_token` / `normalize_405` / `serve` 一行都没碰。`POST` 到 `serve` 的端口仍是 405 ——
那是它自己 `#[cfg(test)]` 里的 `change_verbs_are_405_on_registered_api_paths` 在跑（lib 245 全绿）。

---

## 6 · 测试数字 + 绕行双证

```text
# 全量 cargo test 在 Windows 编不过（既有问题：src/bin/ontok-gate-preflight.rs 是 unix-only）
# 绕行：临时挪开该文件 → 逐套件跑 → finally 挪回 → sha256 与 Test-Path 双证

RESTORE moved=True sha256_before=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
             sha256_after=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
             Test-Path=True  Hold-empty=True
```

| 项 | 本轮 | 基线 | 判 |
| --- | --- | --- | --- |
| lib（`cargo test --lib`） | **245** | 245 | ✅ 一条不少 |
| doc（`cargo test --doc`） | **4** | 4 | ✅ 一条不少 |
| 32 个基线回归套件 | **313** | 313 | ✅ **一条不少**（逐套件见下） |
| `test_req110_ttl`（REQ-110 新增） | **15** | 14（REQ-110 报告口径） | ✅ 全绿（该文件 `#[test]` 实测 15 条；本轮未触碰） |
| 新增套件 `test_req111_review_ui` | **14** | — | 新增 |
| 另跑（受影响）`test_read_only_guard` / `test_domain_print_sites_binary_req078` | 5 / 4 | 5 / 4 | ✅ |
| **套件合计（36 个）** | **351** | 336 | ✅ 313 一条不少 + 36 新增/受影响 |

<details>
<summary>逐套件（本轮实测，全部 0 failed）</summary>

| 套件 | passed | 套件 | passed |
| --- | --- | --- | --- |
| `test_req107_outlet_pipeline` | 12 | `test_review_state` | 28 |
| `test_req107_guard_granularity` | 4 | `test_db` | 11 |
| `test_req107_zero_visible_change` | 3 | `test_extracted_by_provenance` | 9 |
| `test_req107_pipeline_bytes` | 1 | `test_review_provenance` | 17 |
| `test_req107_scope_error_gate` | 4 | `test_concept_proposal` | 19 |
| `test_req107_track_manifest` | 2 | `test_diff` | 13 |
| `test_req107_static_texts` | 3 | `test_no_write_path` | 4 |
| `test_req107_red_lines` | 2 | `test_provenance_source` | 5 |
| `test_property_filter` | 10 | `test_overview` | 17 |
| `test_exit_ordering_req106` | 7 | `test_req108_live` | 5 |
| `test_binary_output_guard` | 7 | `test_req108_confidence` | 10 |
| `test_graph_server` | 9 | `test_req107_source_enumeration` | 10 |
| `test_guard_manifest_req081` | 6 | `test_snapshot` | 10 |
| `test_apply` | 21 | `test_snapshot_write_paths` | 8 |
| `test_review_show_e2e` | 11 | `test_model_view` | 10 |
| `test_proposal` | 9 | `test_view` | 26 |
| **小计（32 套件 = 基线 313）** | **313** | | |
| `test_req110_ttl` | 15 | `test_read_only_guard` | 5 |
| `test_domain_print_sites_binary_req078` | 4 | `test_req111_review_ui`（新） | 14 |
| **合计** | **351** | | |

</details>

> 与 REQ-110 报告的口径差异（如实登记）：那份写 `test_req110_ttl = 14`，
> 本轮实测该文件有 **15** 个 `#[test]`（`Select-String -Pattern '^#\[test\]|^\s*#\[tokio::test\]'` 计数 15），
> 全绿。**不是本轮加的**（该文件本轮 0 改动）。
>
> `tests/test_retained_names_req084.rs` 用 `include_str!("../src/bin/ontok-gate-preflight.rs")`，
> 挪开绕行文件时它编不过 —— 既有问题（REQ-110 报告 §8 已登记），本轮未触碰。

---

## 7 · 已知限制（如实登记，⛔ 不写成「已封堵」）

| # | 限制 | 影响 / 为什么本轮不修 |
| --- | --- | --- |
| G1 | **口令校验是第二份实现**（`review_ui/http.rs` 的 `token_matches`） | AC-008 要求 `src/server/http.rs` **一行不动**，所以不能把它的私有 `token_matches` 公开复用。这是**认证**不是**决策**：两份的失败方式都是「拒绝」，不会静默放行。 |
| G2 | **审计只留最后一次改判** | AC-005 明确要求「同 item+track 连签不产生两条记录」；要完整历史就得追加日志表，两条判据会互相打架。需要「改判史」应另立需求。 |
| G3 | **单机单人**：静态共享令牌，无过期/吊销/不区分谁签的 | 与 `serve` 同一条临时身份门（完整方案是 REQ-099）。 |
| G4 | **页面无服务端渲染 / 无自动化浏览器测试** | 页面壳零数据是**要求**；页面的交互（弹窗、过滤）只在 `page.rs` 源码层断言 + 手工跑过。没有 headless 浏览器 ⇒ 无法把「点击」变成判据。 |
| G5 | **`src/review_ui/` 不在 REQ-107 的 `OUTLET_FILES` 里** | 见 §8 第 1 条：它是**新增出口文件**，T1（全目录写点枚举）覆盖它，T5（函数登记）/T9（响应体必须管线产物）不覆盖。理由：它不是客户出口、不过管线；但它确实是一条新的 HTTP 写面 —— **这是本轮最可能被 QA 打回的一点**。 |
| G6 | 预览的档位比较用 `score + ε >= min` | 分数是两位小数近似量；严格 `>=` 会在 `0.9000000000000001` 这类表示误差上把本该入选的条目漏掉。ε 只放宽**浮点表示**，不放宽档位语义。 |

---

## 8 · 我认为最可能被 QA 打回的一点

**`src/review_ui/` 是新增的「出口文件」，但 `OUTLET_FILES`（T5/T9 的扫描面）没把它纳入。**

- 已做的：T1（`src/` **全目录**写点枚举）覆盖它 —— 任何未登记的 `Json(` / `Html(` 都会红；
  `OUTLETS` 登记了 R1–R5，`test_req107_outlet_pipeline::outlet_sources()` 也把它纳入登记表对账。
- 没做的：T5（出口文件里每个 `fn` 必须登记用途）与 T9（`Json(`/`Html(` 的实参必须是管线产物）
  只扫 `cli.rs` / `server/http.rs` / `mcp/serve.rs`。把 `review_ui/http.rs` 加进去会让
  **T9 立刻红** —— 审核界面的 JSON 是**审核对象的原始候选定义**，不该、也不能过客户发布闸。
- 所以这是一个**口径选择**：要么承认「`OUTLET_FILES` 是客户出口的扫描面，审核界面是内部轨」，
  要么给 T9 加一条「Internal 轨豁免」的判据（那要改 REQ-107 的判据本体，超出本需求范围）。
  本轮选前者并**明确登记**，请 QA 裁定是否接受。

次可能被打回的：**「批量签」在 HTTP 层没有批量接口**。AC-006 的原话是「HTTP 层批量接口**若存在**，
必须带 `items[]` 白名单」—— 本实现让它**不存在**：预览返回 `items[]`，页面逐条 POST。
好处是「一键全库」在协议面上表达不出来；代价是「批量」的原子性没有（中途失败会停在半途，
页面显示停在哪一条、原因是什么）。

---

## 9 · 复现命令（照抄即可）

```powershell
# ① 种子数据：req111_live v1（未冻结）+ 同一份 135 条目提案
#    （交付物里的临时 example 已删除；测试侧等价做法见 tests/test_req111_review_ui.rs 的 fixture）

# ② 起审核界面
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo'
.\target\debug\ontok.exe review serve --project req111_live --proposal p1 `
    --bind 127.0.0.1:8788 --token 'req111-local-token'

# ③ 浏览器打开 http://127.0.0.1:8788/ ，填口令后：
#    列表（未签/档位过滤）→ 逐条接受/拒绝 → 「预览批量接受…」→ 弹窗「将接受 N 条」→ 确认
```

---

<div style="border-top:2px solid #999;margin:32px 0"></div>

<!-- 以下为 REQ-110 的交付报告，原样保留（本 worktree 从其基线继承） -->


# REQ-110 · 内置 TTL 导出 + 页面查看与下载 —— 交付报告

> 分支 `feature/REQ-110`（worktree `feature-REQ-110/`，基线 `develop` `5737ecf`）· **未 push**
> 一句话：**把「手写脚本转 TTL + 拖文件」收成产品的一个命令 + 页面上的一个按钮**，
> 同时把「浏览器打不开页面」这个死循环解开（`/` 免 token 的**无数据壳**）。
>
> ⚠️ 两条**必须一起看**的结论：
> 1. **AC-002 与 AC-008 按字面互斥**（见 §10.1）——本实现把闸放在**全部由数据产生的字节**上，
>    把 AC-008 强行要求写进文件头的那个守门词（`cardinality`）排除在扫描外。
> 2. **REQ-107 的 H1 golden 被有意重锚**（见 §10.2）——`/` 从「内嵌项目名的页面」变成
>    「零数据静态壳」是 R4/AC-005 的要求，字节**必然**变；判据本身（逐字节比 golden）没放宽。

---

## 0 · 结论先行

| 项 | 结果 |
| --- | --- |
| 11 条 AC | **11 条通过**（AC-002 的「扫整份文件」按 §10.1 的冲突裁定收窄为「扫数据正文」，已如实登记） |
| 导出计数（`--model-view`） | **83 类 / 866 数据属性 / 52 对象属性 / 5953 三元组**（参照物 5954，差的正是 `dcterms:created` 那一条） |
| 与参照物等价 | ✅ **同构**：`isomorphic(参照物 − dcterms:created, 本实现) = True`（逐条差异见 §3） |
| CLI 与 HTTP 逐字节相同 | ✅ sha256 **`47C2F166…539D98`**（两条都实测，§6） |
| `/` 壳内零数据 | ✅ `GET /` 无 token = 200；与模板**逐字节相同**；`secretproj` / 版本 / 对象名 **0 命中**（§5） |
| `/api/*` 仍 gated | ✅ graph / node / ontology.ttl / scope **全部 401**（含未注册的 `/api/*`） |
| 客户视图导出过守门器 | ✅ 命中「主键」⇒ CLI `exit=2`、stdout **0 字节**、stderr 报词条；HTTP **500**（§4） |
| 确定性 | ✅ 同一版本两次导出逐字节相同（进程级 + 进程内都测；不写生成时间） |
| 新依赖 | ✅ **零**（Turtle 手写；测试也钉住 Cargo.toml 里没有 RDF 库） |
| 红线文件 | ✅ 六个红线文件 + `guard()` + `property_filter` G1–G4 **零改动**（`git diff --numstat` 为空） |
| 测试 | lib **245**（=230+15）· doc **4**（=基线）· 32 个基线回归套件 **313**（**=基线，一条不少**）· 新增套件 **14** |

---

## 1 · 改动清单（`path:line`）

### 1.1 新增

| 文件 | 职责 | 关键位置 |
| --- | --- | --- |
| `src/domain/ttl.rs`（新，1002 行） | 纯函数 Turtle 序列化（两种投影 → Turtle），零 I/O、零依赖 | `DEFAULT_BASE:82` · `TtlOptions:86` · `TtlDocument`（`doc`/`body` 分离）`:120/:127/:132` · `serialize_customer:156` · `serialize_model:203` · `lossy_header:385` · `local_model:738` · `local_customer:763` · 单测 9 条 |
| `tests/test_req110_ttl.rs`（新，14 条） | AC-001/002/003/008/009 + 真库 + 真进程（CLI 退出码 / 逐字节） | `ac001:84` · `ac002:115/:143` · `ac003:157/:178` · `ac008:198/:220` · 纪律 H `:283` · 零依赖 `:303` · 纯函数 `:315` · 真库 `:336` · 真进程 `:497/:536` |

### 1.2 改动

| 文件 | 改了什么 | 关键位置 |
| --- | --- | --- |
| `src/domain/mod.rs` | 注册 `pub mod ttl;` | `:56` |
| `src/domain/pipeline.rs` | 新增 6 条出口 `C6/C7/H4/H5/D9/D10`；新增两个产物 `Product::CustomerTurtle` / `ModelTurtle`；客户 TTL 在管线里**先剔 → 序列化 → 扫正文 → 才产出可写出的字节** | `OutletId:257-278` · `spec_of:325+` · `Product:383/:389` · `render:759/:771` · `pairing_ok:799-801` · `OUTLETS:985/993/1032/1041/1103/1111` · 登记口径 39 |
| `src/cli.rs` | `ontok ontology export --project --version [--model-view] [--out] [--base] [--format]`；两条分支只消费 `publish_customer` 的返回值 | `OntologyCmd::Export:308` · 处理:1777-1845 · `customer_ttl:1827` · `model_ttl:1803` |
| `src/server/http.rs` | `GET /api/ontology.ttl`（要 token）· `GET /api/scope`（要 token，页面壳取作用域）· `require_token` 收窄到 `/api/*` 子路由 · `/` 改静态壳 · `AppState` 保存已过前处理的 `view` 与动作层条数 | `AppState.view:84` · `action_type_count:93` · `load_frozen_view:110` · `index:363` · `get_scope:380` · `get_ttl:406` · `ttl_file_name:451` · `build_router:573` · `state_from_view:606` |
| `src/server/page.rs` | 模板改成**零数据静态壳**：无项目名/版本占位符、无 `js_string_literal`；「导出 TTL」按钮（带 token 拉取 + blob 下载 + 从 `Content-Disposition` 取文件名）；作用域改由 `api/scope` 取 | `INDEX_HTML:49` · `index_html():332` |
| `tests/test_req110_ttl.rs` 外的 9 个测试文件 | REQ-107 登记表口径 33→39、H1 golden 重锚、REQ-106/REQ-093 判据改写（逐条见 §7） | 见 §7 |
| `tests/fixtures/req107/golden/{H1_index.html,SHA256SUMS,PROVENANCE.txt}` | H1 重锚（其余 12 份逐字节未动） | PROVENANCE 追加 REQ-110 说明 |

**⛔ 零改动（`git diff --numstat` 实测为空）**：`tests/drop_rate.rs`、`tests/test_fk_naming.rs`、
`src/domain/view.rs`、`src/domain/review_view.rs`、`src/mcp/tools.rs`（含 `guard()`）、
`src/domain/property_filter.rs`（G1–G4）。

---

## 2 · 导出计数（实测）

命令（真库 `anspire_ai` v2，冻结版）：

```powershell
$env:ONTOK_DATABASE_URL='postgresql://…@172.17.0.87:5432/ontok_demo'
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 --model-view --out model.ttl
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 --out customer.ttl
```

| | 参照物 `code/out/ai-ontology.ttl` | 本实现 `--model-view` | 本实现**默认**（客户视图） |
| --- | --- | --- | --- |
| `owl:Class` | 83 | **83** | **83** |
| `owl:DatatypeProperty` | 866 | **866** | **843** |
| `owl:ObjectProperty` | 52 | **52** | **52** |
| 三元组总数（rdflib 解析） | 5954 | **5953** | **5472** |
| `owl:InverseFunctionalProperty` | 52 | 52 | 52 |
| 文件大小 | 231,948 B | 231,973 B | 220,676 B |

**客户视图为什么是 843 而不是 866**（这是**既定行为**，不是导出漏了）：

| 丢失点 | 条数 | 原因 |
| --- | --- | --- |
| `view::project` 投影 | 12 | 属性名不可当人话用 / 是内部 key（客户视图本来就不发） |
| `property_filter::filter_view` | 11 | 属性名是源库标识符形态（`customer_id` 那类），与 `/api/graph` / `/api/node` **同一份前处理** |
| 合计 | 23 | 866 − 23 = **843** |

> 这 23 条在 stderr 有报告：`⚠ 发布闸前处理：共剔除 13 项（对象名 0 · 整条属性 11 · 只隐去说明 0 · 对象说明 2）…`
> （13 = 11 条属性 + 2 个对象说明；另外 12 条在投影阶段就没进客户视图。）

---

## 3 · 与参照物是否等价（逐条差异，不是「一致」）

判据（rdflib 7.6.0）：

```python
ref2 = 参照物 去掉 dcterms:created 那 1 条三元组
isomorphic(ref2, 本实现的 --model-view)          # → True
```

**⚠️ `isomorphic` 必须对「参照物 − `dcterms:created`」做**：参照物里有 1 条带**生成时间**的
`dcterms:created "2026-10-21T11:40:32.831145+00:00"^^xsd:dateTime`，而 R6 要求两次导出逐字节相同
⇒ 本实现**刻意不写时间**。这是**唯一**的三元组差异（5954 → 5953）。

逐条差异清单：

| # | 差异 | 性质 | 是否三元组差 |
| --- | --- | --- | --- |
| 1 | 缺 `dcterms:created`（1 条） | **刻意**（R6 确定性；设计 §2.2 裁定「省略生成时间」） | **是**（唯一一条） |
| 2 | 类块顺序：参照脚本按 `entity_types` 数组序（`agent_device_version` 开头），本实现按 **key 字典序**（`activity` 开头） | 确定性要求（R6）；**同构** | 否 |
| 3 | 对象属性 `rdf:type` 多值：参照物 rdflib 折行成 `a owl:InverseFunctionalProperty,\n owl:ObjectProperty`，本实现写成一行 `a owl:InverseFunctionalProperty, owl:ObjectProperty` | 排版；**同构** | 否 |
| 4 | 前缀声明：参照物 7 个（无 `skos`，rdflib 省了未用前缀），本实现固定 8 个（含 `dcterms`/`skos`） | 前缀不产生三元组；**同构** | 否 |
| 5 | 文件头多了 `#` 有损声明块（AC-008：`ActionType` / `semantic` / `cardinality`） | 注释，非三元组 | 否 |
| 6 | `prov:Activity` 的 blank node 标签不同（`n80ba…` vs `n88be…`） | bnode 标签无意义；**同构** | 否 |
| 7 | 字面量集合：22 条类注释、8 条属性注释（`note != name` 才写）、52 条关系语义注释，逐条与参照物**相同** | — | 否 |

⇒ **结论：去掉 `dcterms:created` 后 5953 条三元组逐条对应，图同构。**

---

## 4 · AC-002（守门器）怎么验的

### 4.1 机制（为什么能守住）

管线顺序（`pipeline::render` 的 `CustomerTurtle` 分支，`pipeline.rs:759`）：

```
CustomerView ──▶ filter_view（前处理，REQ-104 同一份）──▶ ttl::serialize_customer
             ──▶ gate_text(spec, doc.body())   ← 扫的是**序列化之后的 TTL 正文**
                    │
              命中 ─┴──▶ Err(Leaked{hits}) ⇒ CLI: exit 2 / HTTP: 500
```

**扫的是最终字节，不是结构字段**：属性名/说明里的「主键」会变成 `rdfs:label` / `rdfs:comment`
字面量，IRI 本地名也是从名字生成的 —— 只扫结构字段会漏掉「序列化过程自己引入的文本」。

### 4.2 构造的命中样例 + 拒绝输出（真进程）

构造（测试 `tests/test_req110_ttl.rs:415` `live_frozen_version(..., poisoned=true)`）：
项目 → 第 1 版 → 写快照 → 冻结，实体 `订单` 的 `description = "本表的主键服务中心，按业务口径解读"`。

```
AC-002 现场：project=t_req110bad_15704_0 version=1 · exit=Some(2) · stdout=0 字节
stderr: ⚠ 客户视图泄露了建模术语（出口 C6）：["主键"]
```

- `exit=2` = `EXIT_LEAKED_MODELING_TERMS`；**stdout 0 字节**（拒绝时不写半个字）；
- 词条**只进 stderr**，错误正文不含「主键」（`ac002_a_poisoned_note_is_refused_by_the_customer_export` 断言）；
- 清掉说明后同一套流程 `exit=0` 且产物有内容（对照组，防「恒拒绝」）。

### 4.3 ⚠️ 构造用例时必须避开的一个陷阱（实测踩到过）

第一版用的是 **`"这张表的主键是 order_no"`** —— 结果 **200 / exit 0**，闸根本没命中。
原因：`property_filter` 判定「说明里带源库标识符（`order_no`）」⇒ 整段说明被**前处理隐去**
（REQ-104 行为），发布闸看到的是已经干净的正文。
⇒ 命中样例必须用**不含 ASCII 标识符**的形态（`主键服务中心`）；这条已写进测试注释，
免得下一个人以为闸坏了。

### 4.4 HTTP 侧

`ac002_a_poisoned_note_is_refused_on_the_ttl_endpoint`：同一份脏视图 ⇒
`GET /api/ontology.ttl`（带正确 token）= **500**，响应体不含「主键」、不含对象名、不含 `@prefix`（一个正文字节都不给）。

---

## 5 · AC-005（`/` 壳内零数据）实测

### 5.1 `secretproj` / 第 7 版（真路由，进程内）

`src/server/http.rs:1450`，`state_from_view(&view, "secretproj", 7, TOKEN, 0)`，视图里放一个
名为 `绝密对象` 的对象；**`GET /` 不带 token**：

```
AC-005 现场：project=secretproj version=7 · GET / 无 token = 200 · shell=12479 字节 ·
grep「secretproj」=false 「绝密对象」=false 「第 <b>7</b> 版」=false · body==INDEX_HTML：true
```

⇒ 200、响应体与 `page::index_html()` **逐字节相同**（等于证明「服务端没往里拼任何东西」）。

### 5.2 真 socket + 真库复核（`anspire_ai` v2，83 个对象）

```
GET /                                     -> 200
GET /api/ontology.ttl   (无 token)        -> 401
GET /api/scope          (无 token)        -> 401
GET /api/graph          (无 token)        -> 401
GET /api/ontology.ttl   (带 token)        -> 200
GET /api/scope          (带 token)        -> 200 {"project":"anspire_ai","version":2}
```

`GET /` 响应体（**12479 字节 / 11267 字符**，sha256 与 golden `H1_index.html` **相同**）里逐词 grep：

| 词 | 命中 |
| --- | --- |
| `anspire_ai`（项目名） | **0** |
| `第 2 版`（版本号） | **0** |
| `订单` / `客户` / `API_调用明细`（对象名） | **0 / 0 / 0** |
| `"version"` / `83` | **0 / 0** |

（模板里唯一一个「项目」出现在 `$("#who").innerHTML = "项目 <b>"…` —— 那是**取到 scope 之后**
由 JS 拼的 UI 文案，不在壳的静态字节里；对象名 `客户` 已从模板注释里删掉，golden 里 0 命中。）

---

## 6 · AC-007（CLI 与 HTTP 逐字节相同）的 sha256

```
CLI :  ontok ontology export --project anspire_ai --version 2 --out customer.ttl
HTTP:  GET /api/ontology.ttl  (Authorization: Bearer …)

47C2F166689E0A0017F742CA41815BA4BA8525EE347D28D24FA9834BBB539D98  customer.ttl
47C2F166689E0A0017F742CA41815BA4BA8525EE347D28D24FA9834BBB539D98  http.ttl
```

响应头（实测）：

```
content-type: text/turtle; charset=utf-8
content-disposition: attachment; filename="anspire_ai-v2.ttl"
content-length: 220676
```

**为什么必然相同**：两条路都只调 `pipeline::render(id, Product::CustomerTurtle{…})`
（`publish_customer(C6)` / `get_ttl(H4)`），HTTP 侧的 `state.view` 就是同一份已过前处理的视图；
`filter_view` 幂等（`property_filter.rs` 已钉），所以「剔一次再剔一次」不改变字节。
`ac007_http_and_cli_ttl_bytes_are_identical` 在进程内直接比**字节**（比 sha256 更强）。
纪律 H 另有否定式判据：`cli.rs` / `http.rs` 里直调 `serialize_customer(` 的处数 = **0**（`test_req110_ttl.rs:283`）。

---

## 7 · AC-010：改写了哪条测试（改前 / 改后）

**主判据**（`src/server/http.rs`，原 `unauthenticated_probes_cannot_discover_which_paths_exist`）：

| | 改前 | 改后 |
| --- | --- | --- |
| 测试名 | `unauthenticated_probes_cannot_discover_which_paths_exist` | `unauthenticated_probes_learn_nothing_but_the_shell_exists`（`:986`） |
| `GET /` 无 token | 断言 **401** | 断言 **200**，且响应体含「业务关系图」、**不含项目名** |
| `/api/*` 无 token | 401（含未注册路径、`POST /api/apply`） | **不变**：401（含未注册路径、变更动词） |
| 代价 | 「连有哪些路径都探测不到」 | 「能探测到有个服务在（`/` 200），但**探测不到任何接口、拿不到任何数据**」 |

**配套改写（都是为了 R4/AC-005 的新形态，断言没有放宽）**：

| 文件 | 原测试 | 改后 | 理由 |
| --- | --- | --- | --- |
| `tests/test_graph_server.rs` | `the_page_carries_its_scope`（断言页面**含** `demo3`） | `the_shell_carries_no_scope_and_fetches_it_from_a_gated_api`（断言页面**不含**项目名，且确实从 `api/scope` 取） | 壳内零数据 |
| `tests/test_exit_ordering_req106.rs` | `the_homepage_gate_calls_the_mcp_guard_itself`（page.rs 里出现 `mcp::tools::guard`） | `the_project_name_gate_is_the_mcp_guard_itself_on_its_new_outlet`（page.rs **没有**判据；`/api/scope` 经 `publish_json`；管线仍直调 `guard`） | 项目名出口从首页搬到 H5 |
| 同上 | `the_project_placeholder_occupies_a_whole_js_literal`（模板里有 `%%PROJECT%%`） | `the_shell_has_no_injection_point_at_all`（模板**不许**有占位符 / `js_string_literal`；`index_html()` 必须无参） | 注入点整段删掉 |
| `src/server/http.rs` | `a_hostile_project_name_cannot_break_the_page`（JS 字面量转义面） | `a_hostile_project_name_cannot_break_the_shell_or_the_scope_json`（任意项目名下 `/` 字节**完全相同**；`/api/scope` 的 JSON 能原样还原） | 转义面从页面搬到 JSON 出口 |
| 同上 | `a_project_name_with_modeling_terms_is_refused_on_the_homepage` | `…_on_the_scope_endpoint` | 闸门出口改名 |
| 同上 | `a_clean_project_name_still_serves_the_homepage` | `a_clean_project_name_still_serves_the_scope` | 对照组跟着搬 |
| 同上 | `the_homepage_gate_does_not_bypass_the_token_gate` | `the_public_shell_does_not_open_the_scope_endpoint`（壳公开 ≠ 数据公开） | 语义更强：公开的壳不许顺带打开 scope |
| 同上 | `without_token_every_endpoint_is_401` | `without_token_the_shell_is_public_but_every_api_is_401` | 逐条列 4 个 API 断言 401 |

**没有任何测试被删除、`#[ignore]` 或放宽**（`OUTLETS` 条数 33→39 是**新增 6 条出口**，
分组 13/8/9/2/1 → 17/10/9/2/1 逐项精确相等；每个新出口都带自己的 write-point needle + 证据调用）。

---

## 8 · 测试数字

```powershell
# 全量 cargo test 在 Windows 编不过（既有问题：src/bin/ontok-gate-preflight.rs 是 unix-only）
# 绕行：临时挪开该文件 → 逐套件跑 → 挪回 → sha256 前后一致
sha before = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
sha after  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D   ← 一致
```

| 项 | 本轮 | 基线 | 判 |
| --- | --- | --- | --- |
| lib（`cargo test --lib`） | **245** | 230 | ✅ +15（ttl 9 + http 6） |
| doc（`cargo test --doc`） | **4** | 4 | ✅ 一条不少 |
| 32 个基线回归套件 | **313** | 313 | ✅ **一条不少**（逐套件见下表） |
| 新增套件 `test_req110_ttl` | **14** | — | 新增 |
| 另跑（受本次改动影响）`test_read_only_guard` / `test_domain_print_sites_binary_req078` | 5 / 4 | — | 均绿 |
| **套件合计（35 个）** | **336** | （REQ-109 口径 344） | 见下注 |

> 注：基线「35 套件 344」= 32 个回归套件 + REQ-109 的 3 个新套件（31 条）。
> 本轮 32 个回归套件仍是 **313**；本轮新增 1 个套件 14 条；另加两个受影响套件 9 条 ⇒ 35 套件 **336**。
> 口径不同（基线那 3 个 REQ-109 套件不在本轮 35 个里），比的是**回归套件 313 一条不少**这条。

<details>
<summary>逐套件（本轮实测，全部 0 failed）</summary>

| 套件 | passed | 套件 | passed |
| --- | --- | --- | --- |
| `test_req107_outlet_pipeline` | 12 | `test_proposal` | 9 |
| `test_req107_guard_granularity` | 4 | `test_model_view` | 10 |
| `test_req107_zero_visible_change` | 3 | `test_view` | 26 |
| `test_req107_pipeline_bytes` | 1 | `test_review_show_e2e` | 11 |
| `test_req107_scope_error_gate` | 4 | `test_review_state` | 28 |
| `test_req107_track_manifest` | 2 | `test_db` | 11 |
| `test_req107_static_texts` | 3 | `test_extracted_by_provenance` | 9 |
| `test_req107_red_lines` | 2 | `test_review_provenance` | 17 |
| `test_property_filter` | 10 | `test_concept_proposal` | 19 |
| `test_exit_ordering_req106` | 7 | `test_diff` | 13 |
| `test_binary_output_guard` | 7 | `test_no_write_path` | 4 |
| `test_graph_server` | 9 | `test_provenance_source` | 5 |
| `test_guard_manifest_req081` | 6 | `test_overview` | 17 |
| `test_apply` | 21 | `test_req108_live` | 5 |
| `test_snapshot` | 10 | `test_req108_confidence` | 10 |
| `test_snapshot_write_paths` | 8 | `test_req107_source_enumeration` | 10 |
| **小计（32 套件 = 基线）** | **313** | | |
| `test_req110_ttl`（新） | 14 | `test_read_only_guard` | 5 |
| `test_domain_print_sites_binary_req078` | 4 | **合计** | **336** |

</details>

> `tests/test_retained_names_req084.rs` 用 `include_str!("../src/bin/ontok-gate-preflight.rs")`，
> 挪开绕行文件时它**编不过** —— 既有问题、不在 32 个基线套件里，本轮未触碰。
>
> ✅ `tests/test_anchor_guard.rs::no_bare_env_manifest_dir_outside_anchor_file`
> **REQ-109 引入的红已由 hotfix `abf1c9f` 修复；本分支实测 3 passed / 0 failed。**
> （`abf1c9f` 是基线 `f624c1a` 的祖先；旧的「基线上就是红的」说法已过期，别再把绿灯当红灯。）

**另跑（受 CLI / 依赖面影响，均绿，0 failed）**：`test_req109_cli_gate` 6 · `test_project` 10 ·
`test_hierarchy` 16 · `test_snapshot_freeze_gate` 2 · `test_review_show_binary_req072` 7。

---

## 9 · 与设计/需求的偏差（如实登记）

| # | 项 | 说明 |
| --- | --- | --- |
| D-1 | **AC-002 与 AC-008 字面互斥** | 见 §10.1。取舍：**闸扫数据正文**（`TtlDocument::body`），文件头是常量（只随动作层条数变）。**请 BA 裁定是否接受这一口径**（不接受的话，AC-008 的 `cardinality` 关键词必须改成别的写法，否则每次客户导出都会 500） |
| D-2 | **新增 `/api/scope`（出口 H5）** | `/` 变静态壳后，页面必须在拿到口令之后才知道「看哪个项目、哪一版」。R4 说「数据一律走 gated API」⇒ 作用域也走 gated API，而不是塞进 URL/壳。代价：多一条出口（已登记） |
| D-3 | **H1 golden 重锚** | 见 §10.2。这是需求要求的可见变化，golden 仍由**生产路径**产出，`SHA256SUMS` 同步更新 |
| D-4 | `state_from_view` 多一个参数 `action_type_count` | 只用于 TTL 文件头声明动作层条数。6 处测试调用点同步补 `0`；`test_req107_zero_visible_change.rs` 的「可在 b714f48 上编译」这一历史属性到此为止（生成器是一次性步骤，golden 已锚定） |
| D-5 | 客户视图 TTL 不含 `ontok:key` / `ontok:column` / `ontok:cardinality` | 客户视图里**根本没有** key / 列名 / 基数记号；而且 `key` / `cardinality` 是守门词，进了正文 ⇒ 每次导出必被拒。客户侧只导 TBox 骨架（名字、说明、父子、属性、关系、人话基数→OWL 特征） |
| D-6 | `--format` 只认 `ttl` | demand 的目标命令写的是 `--format ttl`，任务表写的是不带 `--format`；两个都支持：默认 `ttl`，写别的值**直接报错**（不静默回退） |

### 已知限制

| # | 限制 | 影响 |
| --- | --- | --- |
| G1 | 只导 TBox（类/属性/关系），不含实例（ABox）与样例值 | 与设计 §七 G1 一致；客户视图的 `sample` 刻意不导出 |
| G2 | `ActionType` / `Function` 不导出 | OWL 无对应；只在文件头声明**条数** |
| G3 | 不导 SHACL 形状（`nullable` 只作为 `ontok:nullable` 注解） | 后续可加 |
| G4 | 建模视图 IRI 本地名用参照脚本的 `local()`（非 `[0-9A-Za-z_]` → `_`） | **不是单射**（`a-b` 与 `a_b` 撞）；参照物如此，本轮照抄以保持等价。客户视图另用 `local_customer`（保留汉字 + 撞车加 `_2`） |
| G5 | 关系 IRI 用 `from__to`（参照脚本口径） | 同一对类型多条关系会撞成同一 subject；本轮数据 52 条两两不同（`pairs unique = 52`），已在 `ttl.rs` 模块头登记 |
| G6 | `/` 免 token ⇒ 服务存在性可被探测 | ADR-004 记录的取舍；壳内零数据由 AC-005 钉住 |

---

## 10 · 两个必须让 BA 拍板的地方

### 10.1 AC-002 与 AC-008 按字面**不可能同时成立**

| AC | 要求 |
| --- | --- |
| AC-008 | 文件头注释**必须**包含 `ActionType` / `semantic` / **`cardinality`** |
| AC-002 | 序列化后跑 `scan_modeling_terms`，**命中即拒绝导出** |
| 事实 | `cardinality` **正是**守门词表 `MODELING_TERMS` 的一员（`view.rs:898`） |

⇒ 如果「扫整份文件」，那么**每一份合法的客户导出都会被自己的文件头拒掉**（exit 2 / 500）。
本实现的取舍（已写进 `ttl.rs` 模块头，并有两条测试钉住）：

- `ttl::TtlDocument` 把**常量头**与**数据正文**分开；管线只扫 `body`；
- 头是编译期常量 + 一个计数（动作层条数），**不含任何数据字节**，所以排除它不会放过任何泄漏面；
- 反过来说，**任何由数据生成的文本**（IRI 本地名、label、comment 字面量、`ontok:sourceProject`）
  都在扫描范围内 —— 这正是 AC-002 要防的（§4.2 的现场证据）。

**备选方案（若 BA 不接受）**：把 AC-008 的关键词改成 `multiplicity` / `关系数量`，或允许文件头
用 `cardinality` 的**断词写法**（`cardina­lity` 带软连字符）—— 两者都比现在丑，且后者会让 grep 判据失真。

### 10.2 REQ-107 的 H1 golden 被有意重锚

`GET /` 从「内嵌项目名的页面」变成「零数据静态壳」是 **R4 / AC-005 的硬要求**，
所以 H1 的字节**必然**变化。处理方式：

- 新 golden = `src/server/page.rs` 的 `INDEX_HTML` 常量本身（H1 走 `Product::Text`，干净模板过闸后逐字节返回）；
- `SHA256SUMS` 里 H1 那一行同步更新（其余 12 份**逐字节未动**）；
- `PROVENANCE.txt` 追加一段说明「REQ-110 重锚 H1，为什么，怎么重锚」；
- 判据本身**没有放宽**：`test_req107_pipeline_bytes` / `test_req107_zero_visible_change` 仍然是**逐字节**比较。

**QA 可能会问**：「你是不是把 golden 改了让它变绿？」——不是：改的是**被要求改的**那一份，
改动有需求依据（AC-005）、有来源说明（PROVENANCE）、有独立的判据（AC-005 的 0 命中）。

---

## 11 · 我认为最可能被 QA 打回的一点

**§10.1 的 AC-002 口径收窄。**

AC-002 的原话是「序列化后先跑 `scan_modeling_terms`，命中即拒绝导出」。
本实现扫的是 `TtlDocument::body`（全部数据字节），**不是整份文件** —— 因为整份文件里有一行
AC-008 强制要求的 `cardinality`。

我会预期 QA 拿这条来打：
1. 它确实**不是**字面意义的「扫整份文件」；
2. 而且我把文件头排除在扫描外，属于**对判据的解释**，不是判据本身。

我的辩护是：（a）两个 AC 字面互斥，必须有一方让步；（b）让的是**常量**那一侧，不是数据那一侧；
（c）文件头不含任何数据、只随动作层条数变，扫它不可能发现新泄漏；（d）如果 QA 仍不接受，
最小改法是**把 AC-008 的关键词换掉**（一个字符串），而不是放宽 AC-002。

第二可能被打回的是 **§10.2 的 H1 golden 重锚**（「你动了 REQ-107 的基线」）——见上面的说明。

---

## 附录 A · ADR-004 全文（请 BA 落库到 `BA/decisions/ADR-004-页面可达性与门禁.md`）

```markdown
# ADR-004 · 页面可达性与门禁（`/` 免 token 的取舍）

- 日期：2026-10-21
- 状态：已接受（REQ-110 实现，待 BA 落库）
- 相关：REQ-093（只读图服务）/ REQ-106（首页项目名注入）/ REQ-107（出口收口）/ REQ-110（本 ADR 的触发者）
- 代码落点：`src/server/http.rs`（`build_router` / `require_token` / `index` / `get_scope`）、
  `src/server/page.rs`（`INDEX_HTML`）
- 改写（不是删除）：REQ-093 的判据「未登录探测不到任何路径」

## 背景

REQ-093 的只读图服务把身份门 `require_token` 挂在**全局 layer** 上 —— 包括 `GET /`
这个**静态页面壳**。于是出现一个死循环：

- 要看到页面，得先有口令；
- 要填口令，得先看到页面；
- 而浏览器的导航请求**不可能**带 `Authorization: Bearer …` 头。

这条门禁的初衷写在 REQ-093 的判据里：「未登录连有哪些路径都探测不到」。
它的代价在当时被接受，是因为服务只给「已经拿着口令的人」用；
但只要产品要「打开浏览器就能看到页面」，它就把页面本身锁死了。

## 决策

**`GET /` 免 token，`/api/*` 一律要 token。**

| 路由 | token | 内容 |
| --- | --- | --- |
| `GET /` | **免** | **静态页面壳**：HTML/CSS/JS 全部内联，**不含任何本体数据**（没有项目名、没有版本号、没有图、没有对象名） |
| `GET /api/scope` | **必须** | 页面壳拿口令后问「看哪个项目、哪一版」 |
| `GET /api/graph` | **必须** | 整份客户视图（图） |
| `GET /api/node` | **必须** | 单个对象的属性与说明 |
| `GET /api/ontology.ttl` | **必须** | TTL 导出（页面上的「导出 TTL」按钮） |

实现要点：

1. `require_token` 从「全局 layer」改为**只加在 `/api` 子路由**上；
2. `/api` 子路由**自带 fallback**，且身份门排在 fallback **之前** ——
   未注册的 `/api/xxx` 无口令同样是 **401**，`/api` 这一层的路径探测仍然探测不到；
3. `GET /` 的响应体与 `page::index_html()` **逐字节相同**（服务端没有往里拼任何东西）；
4. 页面拿口令后先 `GET /api/scope`，再 `GET /api/graph` / `/api/node`；导出按钮走 `GET /api/ontology.ttl`。

## 取舍与代价（必须一起读）

| 得到 | 付出 |
| --- | --- |
| 浏览器能打开页面（口令框终于等得到人） | **服务存在性可被探测**：无口令的人知道「这里有个服务」 |
| 页面有「导出 TTL」按钮，不再靠手工脚本 | 多一条 gated 出口（H4 / H5），必须登记进 REQ-107 的 `OUTLETS` |
| 项目名出口从「首页整页」收窄到「`/api/scope` 一个 JSON」 | `/api/scope` 成为项目名的唯一回显点，必须过同一道发布闸 |

**没有付出的**（这一条是这次决策能被接受的关键）：

- 数据面**一个字节**都没有多放出去：`/api/graph` / `/api/node` / `/api/ontology.ttl` 无口令全部 401；
- 页面壳**不含任何数据**：用一个叫 `secretproj`、第 7 版、含 `绝密对象` 的项目起服务，
  `GET /` 的响应体里 grep 项目名 / 版本号 / 对象名 **0 命中**（AC-005）；
- 口令仍然**不出现在**响应体、页面源码、日志、错误信息里（REQ-106 的既有判据全部保留）。

## 判据改写（不是删除）

REQ-093 的原判据 `unauthenticated_probes_cannot_discover_which_paths_exist`
（「未登录连有哪些路径都探测不到」）**改写**为
`unauthenticated_probes_learn_nothing_but_the_shell_exists`：

| 请求 | 期望 | 理由 |
| --- | --- | --- |
| `GET /` | **200** | 静态壳，零数据 |
| `GET /api/…`（含**未注册**路径） | **401** | `/api` 这一层的路径探测仍然探测不到 |
| `POST /api/…` | **401**（不是 405） | 变更动词也不许泄露「这个 API 存不存在」 |

⛔ 测试是**改写**，不是删除、不是 `#[ignore]`、不是放宽：
`/api/*` 的每一条断言都还在，只是 `/` 从「401」变成「200 且不含数据」。

## 被拒绝的方案

| 方案 | 为什么不行 |
| --- | --- |
| 把 token 写进页面源码 | 等于把口令发给所有能打开页面的人；口令会进缓存、进截图、进 git |
| token 放 URL query（`/?token=…`） | 进浏览器历史、进 Referer、进代理日志 |
| 只放开 `/api/graph` | 数据面直接公开，等于取消门禁 |
| 给浏览器一条「无 token 的壳数据接口」 | 把数据出口换个名字，门禁形同虚设 |
| 用 Cookie + 登录页 | 需要会话/过期/吊销 —— 那是 REQ-099 的活，本轮不做 |

## 后果

- **正面**：页面可达；导出在产品内；项目名的回显点从「整页 HTML」收窄成一个 gated JSON。
- **负面**：服务存在性可被探测（已接受）；`/` 从 401 变 200 是对 REQ-093 判据的**有意改写**，
  已按 AC-010 要求同步改写测试并写本 ADR。
- **将来**：若需要「连服务存在性都藏起来」，正确做法是**前置一层反代/网关**做统一身份，
  而不是把静态壳再锁回去（那会把页面可达性重新变成死循环）。
```

---

## 附录 B · 复现命令（照抄即可）

```powershell
cd <repo-root>\feature-REQ-110
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo'

# ① 两种导出 + 确定性
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 --model-view --out model.ttl
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 --out customer.ttl
.\target\debug\ontok.exe ontology export --project anspire_ai --version 2 > customer2.ttl
Get-FileHash customer.ttl,customer2.ttl -Algorithm SHA256      # 两个 hash 相同

# ② 与参照物同构（需 python + rdflib）
python -c "import rdflib; from rdflib.compare import isomorphic; from rdflib.namespace import DCTERMS; \
ref=rdflib.Graph(); ref.parse('..\code\out\ai-ontology.ttl',format='turtle'); \
got=rdflib.Graph(); got.parse('model.ttl',format='turtle'); \
r2=rdflib.Graph(); [r2.add(t) for t in ref if t[1]!=DCTERMS.created]; \
print(len(ref),len(got),isomorphic(r2,got))"     # 5954 5953 True

# ③ 页面可达性 + AC-007
$env:ONTOK_VIEW_TOKEN='req110-live-token-8'
.\target\debug\ontok.exe serve --project anspire_ai --version 2 --bind 127.0.0.1:8791
# 另开一个终端：
curl.exe -s -o shell.html -w "%{http_code}`n" http://127.0.0.1:8791/                 # 200
curl.exe -s -o NUL -w "%{http_code}`n" http://127.0.0.1:8791/api/ontology.ttl          # 401
curl.exe -s -H "Authorization: Bearer req110-live-token-8" \
     -D h.txt -o http.ttl http://127.0.0.1:8791/api/ontology.ttl                        # 200
Get-FileHash http.ttl,customer.ttl -Algorithm SHA256                                    # 相同
Select-String -Path shell.html -Pattern 'anspire_ai'                                    # 0 命中
```
