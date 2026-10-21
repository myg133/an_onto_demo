# REQ-111 · QA Pre-merge 审核报告

> **审核对象**：`feature/REQ-111` @ `ab42894`（基线 `develop` `f624c1a`）
> **审核方式**：独立复验 + 主动攻击。**未改 feature 分支、未 push。**
> 变异在独立 detached worktree `.review-workspace/qa111-mut`（`ab42894`）里做，跑完已还原（`git status` 干净）。
> **日期**：2026-10-21 · **环境**：`postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo` · `rustc/cargo 1.96.0` · Windows

---

## 0 · 结论

**PASS（带非阻断项）。0 个阻断项，9 条非阻断项。**

10 条 AC 全部通过。Dev 的三条核心声明——**决策只有一个实现**、**`review_ui` 不碰客户轨**、**`serve` 一行未动**——经独立 grep、真跑 HTTP、变异测试复验，**都成立**。测试数字一条不少（lib 245 / doc 4 / 32 套件 313 / 新套件 12），且 `test_anchor_guard` 实际是 **3 passed**（Dev 报告里「已知基线红未变」是**过期说法**，详见 §7.2）。

### 0.1 · 10 条 AC 逐条结论

| AC | 结论 | 我实测的证据 |
| --- | --- | --- |
| AC-001 只绑回环 | ✅ | `--bind 0.0.0.0:8790` ⇒ **exit=1**，文案「只允许绑回环地址…审核界面会**写**决策」；`127.0.0.1:8788` 正常起服务 |
| AC-002 只服务未冻结 | ✅ | 冻结 v1 后启动 ⇒ **exit=1**，文案点名「项目「qa111qa_2210」的 v1 已冻结（2026-10-21T14:03:15Z）」 |
| AC-003 写接口鉴权 | ✅ | `GET /api/review` 无 token **401**、`POST /api/decide` 无 token **401**（正文不回显口令）、带 token **200**；401 后库中 item1 仍 `pending`。⚠️ 见 N1（畸形 JSON 在鉴权前被 400/422/415） |
| AC-004 同一个领域函数 | ✅ | 页面签 item1 `model` ⇒ 库 `model=accepted, model_via=ui`；CLI 签 item1–10 `biz` ⇒ `review show` 读到 `biz=accepted`、`/api/review` 读到 `biz_via=cli`（双向真跑） |
| AC-005 幂等覆盖 | ✅ | item1 `biz` accept→reject：两次均 200，终态 `rejected`，条目总数仍 **135**，`via=ui`、`decided_at` 有值 |
| AC-006 批量先预览 | ✅（无 headless 浏览器） | `GET /api/preview?track=biz&min_confidence=0.9&scope=unsigned` ⇒ `count=39`、`items[]` 39 个、`excluded={below_band:13,no_confidence:82}`；model≥0.60 ⇒ 52。页面 `page.rs:332` 用 `p.count` 显示「将接受 N 条」，`runBatch()` 逐条 POST；`/api/decide-bulk` 不存在 |
| AC-007 失败可见 | ✅ | 服务运行中 `version freeze` 后 POST ⇒ **HTTP 409** + `{"error":"项目「qa111qa_2210」的 v1 已冻结，不能再改判 —— 审核界面只服务未冻结的提案。"}`，item3 仍 `pending` |
| AC-008 与 serve 隔离 | ✅ | `src/server/http.rs` 与 `develop` **blob 相同**（`d9bc02f220…`）、`git diff` **0 字节**；带 token POST 到 serve 的 `/api/graph`、`/api/decide`、`/api/apply` **全 405**；`GET /` 免 token **200**、`/api/*` **401**。⚠️ 见 N1 的 401/405 说明 |
| AC-009 审计可区分 | ✅ | 页面签 ⇒ `via.biz/model = "ui"`；CLI 签 ⇒ `"cli"`；两侧 `decided_at` 均非空；没签过的一轨为 `null` |
| AC-010 不退化 | ✅ | lib **245** / doc **4** / 32 基线套件 **313** / 新套件 **12**；`test_anchor_guard` **3 passed**；`test_guard_manifest_req081` 6、`test_read_only_guard` 5；四红线 + `tools.rs` + `property_filter.rs` **blob 逐一相同**；`Cargo.toml` / `Cargo.lock` **blob 相同**；无 `#[ignore]`、无删测试 |

---

## 1 · 决策唯一性（纪律 H）

### 1.1 定义点与全部调用点

`git grep -n "decide_item(" -- src`（只算源码，不算 `.feature/` 报告与文档注释）：

| 位置 | 角色 |
| --- | --- |
| `src/db/proposal.rs:317` | **唯一实现** `pub async fn decide_item(...)` |
| `src/cli.rs:1492` | CLI `ontok review decide` 调用（`VIA_CLI`） |
| `src/review_ui/http.rs:564` | `POST /api/decide` 调用（`VIA_UI`） |

⇒ **定义 1 + 调用 2**，与「恰好两个调用点（CLI / UI）」一致。
`review_ui/http.rs:9` 与 `.feature/verification-report.md` 里的 `decide_item(` 是注释/文档命中，不是调用。

### 1.2 全仓决策 `UPDATE` 只有一处

`git grep -n "UPDATE proposal_items"` ⇒ **仅 `src/db/proposal.rs:362`**。

| 位置 | 角色 |
| --- | --- |
| `src/db/proposal.rs:347` | `set_decision_via` 定义（唯一决策 `UPDATE`） |
| `src/db/proposal.rs:338` | `decide_item` 调用它 |
| `src/db/proposal.rs:394` | 旧签名 `set_decision` 委托它（记 `VIA_CLI`） |

其余 `UPDATE` 都不是决策：`db/apply.rs:115`（`proposals.status='applied'`）、`db/project.rs:180`（`ontology_versions.frozen_at`）、`db/proposal.rs:400`（`proposals.status`）、`snapshot_store.rs:595`（`ontology_versions.extracted_by`）、`pg.rs:559`（`action_types` 迁移）。**没有第二份决策 UPDATE。**

### 1.3 `src/review_ui/` 里的 SQL 命中数

自己 grep（不是采信报告）：

| 模式 | 代码命中 | 说明 |
| --- | --- | --- |
| `UPDATE ` / `INSERT ` / `DELETE FROM` | **0** | 仅 `src/review_ui/mod.rs:9` 的文档注释提到 `UPDATE `（不是代码） |
| `sqlx::query` / `query_as` / `query!` | **0** | 仅 `http.rs:42` 的 `use sqlx::PgPool;`（连接池类型，不是查询） |
| `set_decision` / `apply_proposal` / `freeze(` | **0** | `http.rs` 只调 `decide_item` 与只读方法 |

⇒ **「HTTP 层不直写 SQL」成立**（这是 AC-004 明确写的**代码审查项**，不是运行期判据）。

### 1.4 变异（独立 worktree，不碰 feature 分支）

Worktree：`D:\MyCodes\AI\an_onto_demo\.review-workspace\qa111-mut`（detached @ `ab42894`）。两次变异都是**编译通过后**才跑的（纪律 G）。

#### M1 · `via` 字段写死为 `"cli"`

```diff
-        .decide_item(state.proposal_id, req.item, track, decision, VIA_UI)
+        .decide_item(state.proposal_id, req.item, track, decision, "cli")
```

结果：**判据红。**

```
test ac005_signing_twice_overwrites_and_keeps_one_row ... FAILED
test ac009_audit_distinguishes_ui_from_cli ... FAILED
test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

⇒ 审计面的「页面签必须记 `ui`」被 **AC-005 / AC-009 两条判据**钉住，写死 `via` 当场红。

#### M2 · HTTP 层直写 SQL（第二份决策实现）

把 `decide_item` 调用替换成对 `proposal_items` 的 `UPDATE … SET {col}, {at_col}=now(), {via_col} …`（`http.rs`，+16/−7 行）。

结果：**判据红。**

```
test missing_item_is_a_visible_404 ... FAILED
test result: FAILED. 11 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

**但必须把这句话说准**：M2 红的是「不存在的条目仍返回 404 + 具体原因」，因为直写 SQL 漏掉了 `decide_item` 里的存在性检查；**不是**一条「HTTP 层不许出现 SQL」的自动判据。`tests/test_req107_source_enumeration.rs` 的 T1 枚举的是**构造拼写**（`Json(` / `Html(` / `fs::write(` 等），不认 SQL —— 一个**逐字复刻行为**的第二实现仍可能全绿。

⇒ 纪律 H 的最终防线仍是「grep + 代码审查 + 本次变异」，与 AC-004 把它写成**代码审查项**的口径一致；**不假装它有自动判据**。

### 1.5 残余（非阻断 N5）

`review decide --accept-all`（`src/cli.rs:1469`）与 `proposal apply`（`src/db/apply.rs:188/191`）仍直接调 **`set_decision`**（→ 同一个 `set_decision_via` 单一 UPDATE），**不经过 `decide_item`**。今天没有行为差异，因为 `domain::review::apply_decision` 只是「把被签那一轨赋成 decision」的纯映射；但将来状态机一旦长出规则（例如「已 rejected 能否被抹掉」），这两条路会先漂。这是**既有**结构，不是 REQ-111 引入的，故非阻断。

---

## 2 · A-1 前提：`review_ui` 里 customer-track 输出调用数 = 0

BA 接受「`review_ui` 不进 `OUTLET_FILES`」的**前提**是「它没有任何 customer-track 输出调用」。我在 `src/review_ui/{http,page,mod}.rs` 上逐符号 grep（含代码/注释区分）：

| 符号 | 总命中 | **代码命中** | 位置 |
| --- | --- | --- | --- |
| `publish_customer` | 0 | **0** | — |
| `Product::CustomerTurtle` | 0 | **0** | — |
| `Product::CustomerJson` | 0 | **0** | — |
| `Product::`（任意变体） | 0 | **0** | — |
| `serialize_customer` / `serialize_model` | 0 | **0** | — |
| `local_customer` | 0 | **0** | — |
| `CustomerView` | 0 | **0** | — |
| `publish_json` / `publish_text` / `publish_` | 0 | **0** | — |
| `scan_modeling_terms` | 0 | **0** | — |
| `guard(`（`mcp::tools::guard`） | **0** | **0** | — |
| `pipeline::` | 1 | **0** | 仅 `mod.rs:31` 文档注释（引用 `OUTLETS` 的说明） |
| `render(` | 11 | 11 | **全部是 `page.rs` 里的 JS `render()` 函数**，不是 `pipeline::render` |
| `fs::write` / `std::io::stdout` | 0 | **0** | — |

⇒ **customer-track 输出调用数 = 0，A-1 的前提成立。** 审核界面的五个出口 R1–R5 全部登记为 `Track::Internal` / `Enforcement::InternalOnly`（`src/domain/pipeline.rs:1146-1202`、`spec_of:364+`），登记表 44 条（客户 17 / 建模 10 / 内部 14 / 静态 2 / 错误路径 1），`test_req107_outlet_pipeline` 12 条与 `test_req107_source_enumeration` 10 条全绿。

补充核实 Dev 的覆盖声明：

- **T1 确实覆盖 `review_ui`**：`tests/test_req107_source_enumeration.rs:72-90` 的 `walk_rs` 递归扫 `src/` 下**全部** `.rs`（不是白名单），`review_ui` 的 `Json(` / `Html(` 写点会被枚举；新增 5 条出口的 needle 在 `OUTLETS` 里逐条登记，条数不对就红。
- **T5 / T9 不扫它是有意的**（`OUTLET_FILES` 仍只有 `cli.rs` / `server/http.rs` / `mcp/serve.rs`）：审核 JSON 是「待裁决的原始候选定义」，过客户闸会恒命中——这与 A-1 的裁定一致。
- **G6 仍然成立**：没有一条判据钉住「customer-track 调用数 = 0」，本轮只能靠 grep 复核（本节就是那份复核）。建议下一轮补一条否定式断言（例如在 `test_req111_review_ui` 里读源码断言 `review_ui` 不含 `publish_customer` / `Product::Customer*`）。

**A-1 裁定：前提成立，不是漏网。**

---

## 3 · `serve` 是否真的一行没动

### 3.1 逐字节

| 文件 | `develop` blob | `feature/REQ-111` blob | 相同 |
| --- | --- | --- | --- |
| `src/server/http.rs` | `d9bc02f220…` | `d9bc02f220…` | ✅ |
| `src/server/page.rs` | `90196623b8…` | `90196623b8…` | ✅ |
| `src/server/mod.rs` | `f539e53f09…` | `f539e53f09…` | ✅ |

`git diff develop -- src/server/http.rs` ⇒ **0 行 / 0 字符**（不是只看 `--stat`）。
`git diff --name-only develop` 的改动文件里**没有** `src/server/*`。

### 3.2 真跑 `serve`（`anspire_ai` v2，冻结版，端口 8789）

| 请求 | 实测 |
| --- | --- |
| `GET /`（无 token） | **200**，12479 字节（REQ-110 的 zero-data shell） |
| `GET /api/graph`（无 token） | **401** |
| `GET /api/scope`（无 token） | **401** |
| `GET /api/ontology.ttl`（无 token） | **401** |
| `GET /api/graph`（带 token） | **200** |
| `GET /api/ontology.ttl`（带 token） | **200**，220676 字节，`text/turtle; charset=utf-8`，`attachment; filename="anspire_ai-v2.ttl"` |
| TTL 正文 sha256 | **`47C2F166689E0A0017F742CA41815BA4BA8525EE347D28D24FA9834BBB539D98`**（与 REQ-110 报告逐字相同） |
| `POST /api/graph`（带 token） | **405** `{"error":"这里只读：只能看，没有任何可以改动的地方。"}` |
| `POST /api/decide`（带 token） | **405**（`review` 的写端点打到 `serve` 仍是 405） |
| `POST /api/apply` / `PUT` / `DELETE` / `PATCH`（带 token） | **405**，同一句人话 |
| `POST /api/graph`（**无 token**） | **401**（不是 405） |

**⚠️ 口径必须写准**：AC-008 的「POST 仍 405」成立的是**带 token** 的 POST。无 token 的 `POST /api/*` 返回 **401**，这是 REQ-110 / ADR-004 **刻意**的行为（「变更动词也不许泄露这个 API 存不存在」），本期未改。Dev 报告 §0 / §5 把它简写成「`POST` 到 `serve` 的端口仍是 405」——**不是回归，但不完整**（N1 的同类表述问题，见 §9）。

⇒ **`serve` 零改动成立，REQ-110 的 200/401/405/TTL 行为全部保留。**

---

## 4 · 写接口门禁与冻结（五种攻击 + 两种额外攻击）

服务：`review serve --project qa111qa_2210 --proposal p1 --bind 127.0.0.1:8788 --token qa111-local-token-2210`（135 条目、v1 未冻结）。

| # | 攻击 | 期望 | 实测 | 库状态 |
| --- | --- | --- | --- | --- |
| 1 | `POST /api/decide` 无 token（合法 JSON） | 401 且不写 | **401** `{"error":"没有可用的访问口令，拿不到内容。"}` | item1 前后都 `pending`，总数 135 |
| 2 | 对已冻结提案启动 | exit≠0 + 点名版本 | **exit=1**「v1 已冻结（2026-10-21T14:03:15Z）」 | — |
| 3 | 运行中冻结再 POST | 409 + 具体原因 + 不写 | **409** +「v1 已冻结，不能再改判…」 | item3 仍 `pending` |
| 4 | 越界 item `0` / `99999` | 非 2xx + 说得清 | **404** `{"error":"提案里没有第 0 个条目"}` / `…第 99999 个条目` | 不变 |
| 5 | 同 item+track 连签 accept→reject | 终态 reject、不产生两条 | 200 / 200；终态 `rejected`；`via=ui`；`decided_at` 有值 | **总条目仍 135**，没有第二行 |
| 6（额外） | `item:1, track:"nope"` / `decision:"maybe"` | 400 + 合法取值 | **400** `track 只有两种写法：biz…` / `decision 只有两种写法：accept 或 reject…` | 不变 |
| 7（额外） | 请求体带 `items:[1,2,3]` | 没有批量入口 | **422** `missing field 'item'`（`DecideRequest` 只接受单条） | 不变 |

> 冻结攻击用的是「先起服务、再 `version freeze`、再 POST」的真实时间窗，不是重启后的假场景。

### 4.1 额外攻击：**apply 之后仍能写**（非阻断 N9，但请 BA 看一眼）

启动审核界面（提案 `pending`、基版本 v1 未冻结）→ 在另一个终端 `review decide --accept-all` → `proposal apply`（成功建 v2、提案变 `applied`）→ 再向仍在运行的服务 POST：

```
POST after apply -> HTTP 200
{"biz":"rejected", ... "item":2, "model":"accepted", "ok":true, ...}
```

而对**同一个已 applied 提案**重新启动服务会被拒绝：

```
✗ 提案 p1 的状态是 applied（不是 pending）：已经应用的提案不再接受改判。  (exit=1)
```

原因：`POST /api/decide` 的写前校验 `ensure_unfrozen`（`http.rs:607-622`）**只查基版本是否冻结**，不查 `proposals.status`。`prepare` 的「提案必须 pending」只在启动时查一次。

**影响边界（说清楚，不夸大）**：`decide_item` 只写 `proposal_items`，**不碰** `ontology_versions` / 快照表；已应用版本的快照在 apply 事务里已经固化，所以这次 200 的改判**不会改变 v2 本体**，只会在已 applied 的提案行上留下新的 `via` / `decided_at`，并让 API 回执「成功」。⇒ **数据不脏，回执失信。** 不在 10 条 AC 的判据内，故列为非阻断；建议把 `ensure_unfrozen` 扩成「写前再查提案状态」，或在 `status.md` 登记为已知限制。

---

## 5 · 批量与白名单

### 5.1 HTTP 层有没有批量接口

**没有。** 路由只有 `GET /`、`GET /api/review`、`GET /api/preview`、`POST /api/decide`（`http.rs:222-230`）；未注册的 `POST /api/decide-bulk` 走 fallback ⇒ **405**（`ac006_batch_preview_shows_the_count_and_the_whitelist` 实测断言 405/404，且未落库）。请求体带 `items:[…]` ⇒ **422**，协议上表达不出「一次改一批」。

### 5.2 预览是服务端算的，还是页面自己过滤

**服务端算的。** `GET /api/preview`（`http.rs:422-507`）按 `track + min_confidence + 未签` 在服务端生成 `count` 与 `items[]`：

| 查询 | 实测 |
| --- | --- |
| `track=biz&min_confidence=0.9&scope=unsigned` | `count=39`，`items=[84,85,…]`，`excluded={decided:1,below_band:13,no_confidence:82}` |
| `track=model&min_confidence=0.6&scope=unsigned` | `count=52`，`excluded={below_band:0,no_confidence:83}` |
| `scope=all` | **400**「批量只作用于**未签**条目…审核界面没有「一键全库」」 |
| 缺 `track` | **400**「要说明哪一轨：track=biz 或 track=model」 |
| `min_confidence=1.5` | **400**「要落在 0.00–1.00 之间」 |

页面 `page.rs:329` 拿这份 `p.items`，`page.rs:332` 显示「将接受 N 条」，`runBatch()`（`page.rs:343-363`）**逐条** `POST /api/decide`。**页面自己没有过滤逻辑**，也没有「全选 / 全库」按钮。

### 5.3 `items[]` 是不是服务端白名单

**不是强制的写白名单，是服务端生成的候选清单（advisory）。** `POST /api/decide` 只校验 `item` 在提案里存在（`decide_item` → `item_by_seq`），不校验它是否在上一次 preview 的 `items[]` 里。因此直接 POST 一个「低于档位但存在」的条目会 200（我实测 item2 直接 POST = 200）——这与 CLI 的单条改判完全等价，**不构成批量绕过**，因为协议上没有批量入口。

AC-006 的原话是「HTTP 层批量接口**若存在**，必须带 `items[]` 白名单」；本实现让它**不存在**，所以这条判据是空条件。A-2 也已接受「批量 = 逐条、无原子性」。

### 5.4 不存在的 item 序号

`POST {"item": 0}` / `{"item": 99999}` ⇒ **404** + 「提案里没有第 N 个条目」，**不是静默跳过**。

---

## 6 · 口令是否泄漏

| 面 | 检查 | 结果 |
| --- | --- | --- |
| 页面壳 HTML（`GET /`，15705 字节） | 配置的口令值 `qa111-local-token-2210` | **0 命中** |
| 同上 | 项目名 `qa111qa_2210` / 条目名 `活动` / `p1` | **0 / 0 / 0** |
| 同上 | `Authorization` / `Bearer` | **各 1 次**，且只是 `page.rs:166` 的通用客户端代码 `opts.headers["Authorization"] = "Bearer " + S.token;` —— **不含任何口令值** |
| 同上 | `localStorage` / `sessionStorage` / `document.cookie` / `location.search|hash` / `innerHTML` / `eval(` | **0 命中**（口令只在 `S.token` 内存里） |
| 401 响应体 | 是否含期望口令 | **否**，固定句「没有可用的访问口令，拿不到内容。」 |
| 409 响应体 | 是否含口令 | **否**，只有项目名 + 版本号 + 原因 |
| 服务端日志（review / serve / timing / apply 四份 stderr） | 四个测试口令 | **0 命中** |
| 仓库（`git grep` 四个测试口令） | — | **0 命中**（没有把口令写进代码/文档/测试） |

**关于「页面壳里 `Authorization` / `Bearer` 必须 0」这条**：字面做不到——页面要在浏览器里发 Bearer 头，就必然出现这两个字符串。我按**「口令值不得出现」**验，结果为 0；壳里那两处是协议关键字，不是凭据。建议把判据措辞改成「壳内不得出现**口令值** / `S.token` 的初始化值 / 任何硬编码 Bearer 串」。（N6）

另：Dev 登记的 G1「口令校验是第二份实现」我核过，`review_ui/http.rs:272-283` 与 `server/http.rs` 同算法（定长比较），失败方式都是拒绝；它保护的是**认证**，不是决策，不违反纪律 H。

---

## 7 · 不退化数字（全量重跑）

### 7.1 绕行双证

`src/bin/ontok-gate-preflight.rs` 在 Windows 上挡全量编译，按约定挪开 → 逐套件跑 → **finally 挪回**：

```
BEFORE sha256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D Test-Path=True
MOVED  Test-Path(src)=False backup=True
RESTORE Test-Path=True sha256_before=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
                 sha256_after=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D equal=True
```

### 7.2 数字

| 项 | 本轮实测 | 基线 | 判 |
| --- | --- | --- | --- |
| lib（`cargo test --lib`） | **245 passed / 0 failed** | 245 | ✅ |
| doc（`cargo test --doc`） | **4 passed / 0 failed** | 4 | ✅ |
| 32 个基线套件 | **313 passed / 0 failed** | 313 | ✅ **一条不少** |
| 新套件 `test_req111_review_ui` | **12 passed / 0 failed** | — | ✅ |
| `test_req110_ttl` | **15 passed** | （REQ-110 报告口径 14；该文件本轮 0 改动，实际 15 个 `#[test]`） | ✅ |
| `test_read_only_guard` | **5 passed** | 5 | ✅ |
| `test_domain_print_sites_binary_req078` | **4 passed** | 4 | ✅ |
| `test_guard_manifest_req081` | **6 passed** | 6 | ✅ |
| **`test_anchor_guard`** | **3 passed / 0 failed** | 3 | ✅ |
| 套件合计（36 套件） | 313 + 15 + 5 + 4 + 12 = **349** | 336 | ✅ |

32 个基线套件逐套件（全部 `0 failed`）：

`test_req107_outlet_pipeline 12` · `test_req107_guard_granularity 4` · `test_req107_zero_visible_change 3` · `test_req107_pipeline_bytes 1` · `test_req107_scope_error_gate 4` · `test_req107_track_manifest 2` · `test_req107_static_texts 3` · `test_req107_red_lines 2` · `test_property_filter 10` · `test_exit_ordering_req106 7` · `test_binary_output_guard 7` · `test_graph_server 9` · `test_guard_manifest_req081 6` · `test_apply 21` · `test_review_show_e2e 11` · `test_proposal 9` · `test_review_state 28` · `test_db 11` · `test_extracted_by_provenance 9` · `test_review_provenance 17` · `test_concept_proposal 19` · `test_diff 13` · `test_no_write_path 4` · `test_provenance_source 5` · `test_overview 17` · `test_req108_live 5` · `test_req108_confidence 10` · `test_req107_source_enumeration 10` · `test_snapshot 10` · `test_snapshot_write_paths 8` · `test_model_view 10` · `test_view 26` = **313**。

### 7.3 ⚠️ `test_anchor_guard` 的真实状态：**3 passed，不是「基线红」**

Dev 交付文件第 **577–580 行**（注意：那段在**继承来的 REQ-110 报告**里，不在 REQ-111 报告正文）写着：

> `tests/test_anchor_guard.rs::no_bare_env_manifest_dir_outside_anchor_file` 在**基线上就是红的** …

**这句话已经过期。** 依据：

1. 修复提交 `abf1c9f`「[Merge] Hotfix：anchor_guard 恢复（REQ-109 两处裸 env! 违规）」是 **`f624c1a` 的祖先**（`git merge-base --is-ancestor abf1c9f f624c1a` ⇒ true），所以基线 `f624c1a` **已含 hotfix**；
2. `tests/test_anchor_guard.rs` 与 `develop` **零 diff**（本轮没人改判据）；
3. 我实跑 `cargo test --test test_anchor_guard` ⇒ **3 passed / 0 failed**。

⇒ **不是「已知基线红未变」，而是「基线已修、现在 3 绿」。** 交付说明应改掉这句，否则会让人以为还有一条被默许的红灯。

### 7.4 禁改面

| 检查 | 结果 |
| --- | --- |
| `tests/drop_rate.rs` | blob 相同（`f3967ecd82…`） |
| `tests/test_fk_naming.rs` | blob 相同（`6c0ae56208…`） |
| `src/domain/view.rs` | blob 相同（`9d085a0357…`） |
| `src/domain/review_view.rs` | blob 相同（`fdd7fcd904…`） |
| `src/mcp/tools.rs`（含 `guard()`） | blob 相同（`8f51403994…`） |
| `src/domain/property_filter.rs`（G1–G4） | blob 相同（`d4e93e337c…`） |
| `Cargo.toml` / `Cargo.lock` | blob 相同（`d3334b12af…` / `f817a73c73…`） |
| 删测试 / `#[ignore]` / 放宽断言 | **无**；`tests/` 的 diff 只有 `39 → 44` 的登记表断言更新（+ 新增 5 条 R1–R5），**没有删除任何测试函数** |
| 测试文件改动 | `test_req107_outlet_pipeline`（登记表 44 + `review_ui` 纳入对账）、`test_req107_source_enumeration`（MODULES + review_ui）、`test_req108_confidence` / `test_req109_ai_semantic`（结构体补 4 个 `None` 审计栏）——都是**加**，没有放宽 |

---

## 8 · 耗时口径

Dev 的口径：**CLI 59.59 s vs 页面路径 7.69 s（7.75×，270 次决策）**，并在 §4 注明「不含浏览器渲染/点击间隔，把页面真正点一遍还要加上人手时间」。

### 8.1 是否如实表述

**是。** 证据：

- 报告 §4 第 131–133 行**原样保留了**那半句：「本机没有 headless 浏览器…**不含浏览器渲染/点击间隔**…那部分对 270 次决策是分钟级，**不在本数字内**」；
- 全文没有出现「**用户体验**快 7.75 倍」这种表述；用的是「页面路径」（= 270 个 HTTP 请求的墙钟）；
- 顶部汇总表（第 21 行）只写「7.75×」、没带那半句 —— 这是**呈现位置**问题，不是口径造假（N3）。

⇒ **A-3 成立**。

### 8.2 抽样复验（不跑满 270 次）

同一份 135 条目提案，各抽 10 次（CLI 每次进程启动；HTTP 同进程连接复用，先用 1 次请求预热）：

| 路径 | 10 次墙钟 | 单次均值 |
| --- | --- | --- |
| CLI `review decide` | **2.27 s** | **227.4 ms** |
| `POST /api/decide` | **0.23 s** | **23.5 ms** |
| 比值 | — | **9.69×**（抽样） |

与 Dev 的 220.7 ms / 28.5 ms（7.75×）**同向、同量级**；抽样比值更高，主要是 PowerShell/HttpClient 的控制开销与进程启动抖动，不构成对 Dev 数字的否定。**结论：数字可信，口径如实。**

---

## 9 · 阻断项 / 非阻断项

### 阻断项

**无。**

### 非阻断项

| # | 级别 | 项 | 建议 |
| --- | --- | --- | --- |
| **N1** | 中 | **鉴权发生在 JSON 提取之后**：无 token + 合法 JSON ⇒ 401；但无 token + 畸形 JSON ⇒ **400/422**，无 `Content-Type` ⇒ **415**（`http.rs:535` 的 `JsonRequest` 先于 `538` 的 `check_token`）。攻击者能在未鉴权时探测请求形状、拿到解析错误文本（**不能写、不泄露数据**）。另：`serve` 无 token 的 POST 是 **401 不是 405**（REQ-110 的刻意行为），Dev 报告写「POST 仍是 405」缺了「带 token」这个限定。 | 若要「鉴权绝对第一」，把 token 校验挪到 middleware / 自定义 extractor；报告里把 405 写成「带 token 的 POST」。 |
| **N2** | 低 | `GET /api/review` 的 `"frozen": false` 是**硬编码常量**（`http.rs:335`）。运行中冻结后，读接口仍说 `frozen:false`，而写接口正确返回 409。页面没消费该字段，所以用户看不到这句假话。 | 在 `review_payload` 里查一次 `is_frozen`，或删掉该字段。 |
| **N3** | 低 | 耗时表把「7.75×」放在汇总行，**口径说明在 §4**。 | 汇总行补「（不含渲染/点击）」。 |
| **N4** | 低 | 交付文件第 577–580 行「`test_anchor_guard` 在基线上就是红的」**已过期**（基线已含 hotfix `abf1c9f`；实际 3 passed）。 | 删改这句，避免下一个人把绿灯当红灯。 |
| **N5** | 低 | `--accept-all`（`cli.rs:1469`）与 `apply`（`apply.rs:188/191`）走 `set_decision`，**不经过 `decide_item`**；今天无差异（`apply_decision` 是纯映射），将来状态机长规则会先漂。 | 下轮把这两条也收进 `decide_item`（或显式登记为「仅接受」路径）。 |
| **N6** | 提示 | 页面壳里 `Authorization` / `Bearer` 各 1 次（通用客户端代码，不含口令值）。「grep 必须 0」按字面不可达。 | 判据改成「不得出现口令值 / 硬编码 Bearer 串」。 |
| **N7** | 设计确认 | 预览的 `items[]` 是**候选清单**，`POST /api/decide` 不强制「必须属于上次预览」。因没有批量端点，不构成「一键全库」；但白名单是 advisory。 | 若要把白名单变成硬约束，需在 decide 里带 preview token / 签名；当前 A-2 口径下不必。 |
| **N8** | 低 | `src/domain/pipeline.rs:967` 的文档注释仍写「…= **39**」，而同段分组已改成 17/10/14/2/1（= **44**）。注释与代码不一致。 | 改 44。 |
| **N9** | 中 | **apply 之后仍能写**：启动后 `proposal apply` 成功（提案变 applied、基版本仍未冻结），运行中的服务再 POST ⇒ **200**，改判写进已 applied 的提案行；重启服务则正确拒绝（exit 1）。不改变已应用版本（快照已固化），但 API 回执失信。 | `ensure_unfrozen` 同时查 `proposals.status == "pending"`，返回 409「提案已应用/已否决」。 |

> N1 / N9 的详细复现见 §4.1 与 §6；两条都**不影响 10 条 AC 的判据**，故不阻断合并。

---

## 附录 A · 复现命令（本次实际执行）

```powershell
$env:ONTOK_DATABASE_URL='postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo'
$env:ONTOK_TEST_DATABASE_URL=$env:ONTOK_DATABASE_URL

# 1) 决策唯一性
git grep -n "decide_item(" -- src
git grep -n "set_decision_via(" -- src
git grep -n "UPDATE proposal_items" -- src
Select-String -Path src\review_ui\*.rs -Pattern "UPDATE |INSERT |DELETE FROM|sqlx::query"

# 2) A-1 customer-track
Select-String -Path src\review_ui\*.rs -Pattern "publish_customer|Product::Customer|CustomerView|pipeline::|guard\("

# 3) serve
git diff develop -- src/server/http.rs        # 0 行
.\target\debug\ontok.exe serve --project anspire_ai --version 2 --bind 127.0.0.1:8789

# 4) 审核界面（未冻结项目：qa111qa_2210，135 条目）
.\target\debug\ontok.exe review serve --project qa111qa_2210 --proposal p1 `
  --bind 127.0.0.1:8788 --token qa111-local-token-2210

# 5) 冻结 / 越界 / 幂等
curl -X POST http://127.0.0.1:8788/api/decide -H "Authorization: Bearer qa111-local-token-2210" `
  -H "Content-Type: application/json" -d '{"item":0,"track":"biz","decision":"accept"}'
.\target\debug\ontok.exe version freeze --project qa111qa_2210 --version 1

# 6) 全量回归（先挪开 preflight，finally 挪回）
Move-Item src\bin\ontok-gate-preflight.rs $env:TEMP\ontok-gate-preflight.rs
cargo test --lib
cargo test --doc
cargo test --test test_anchor_guard
cargo test --test test_req111_review_ui
# …32 个基线套件…
Move-Item $env:TEMP\ontok-gate-preflight.rs src\bin\ontok-gate-preflight.rs
Get-FileHash src\bin\ontok-gate-preflight.rs -Algorithm SHA256   # C7CF95E2…
```

## 附录 B · 证据留存（本机临时目录）

| 文件 | 内容 |
| --- | --- |
| `%TEMP%\qa111\regression-summary.txt` | 37 个套件逐条 `test result` + 挪开/还原 sha256 |
| `%TEMP%\qa111\mutation-M1-via-cli-result.txt` | M1：`ac005` / `ac009` FAILED |
| `%TEMP%\qa111\mutation-M2-direct-sql-result.txt` | M2：`missing_item_is_a_visible_404` FAILED |
| `%TEMP%\qa111\mutation-M2.patch` | M2 的完整 diff（+16/−7） |
| `%TEMP%\qa111\{review,serve,timing,apply}.err` | 四个服务的启动横幅（均不含口令） |
| `%TEMP%\qa111\index.html` | 审核页壳（15707 字节，口令值/项目名/条目名 0 命中） |

> 本报告只写入 `BA/demands/REQ-111/qa-review.md`；未 commit、未 push，feature 分支保持 `ab42894` 不变。
