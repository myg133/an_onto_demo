# QA 审核：REQ-126 · 客户门户（Pre-merge）

> **结论：PASS（带 4 条非阻断，0 阻断）**
>
> | 项 | 值 |
> | --- | --- |
> | 被测 | `feature/REQ-126` @ `02f4d2d`（2 个 commit：`8191e1f` + `02f4d2d`，基线 `develop` `d54c130`） |
> | 审核方式 | 独立 worktree `qa-req126`（detached `02f4d2d`）编译 + 真库实测 + 变异 |
> | 审核日期 | 2026-10-23 |
> | 报告 | `BA/demands/REQ-126/qa-review.md` |
> | 证据目录 | `.local/qa-req126/evidence/`（本地、不入库；本报告已内嵌关键原文） |
>
> **一句话**：租户确实锁死在口令上（不是「忽略 tenant」，是三道门拒绝）；响应体里确实没有表名 / 列名 / SQL / 属性 IRI；四个端点之外没有第五个；并发闸 N+1 ⇒ 429 且审计 `elapsed_ms=0` —— 真没排队。
>
> ⚠️ 报告正文里的租户一律用 **A / B / C / D / 对拍替身** 代称，附 `sha256(id)[:12]`；真实 UUID 只出现在审计行的证据里（AC-009 要求记租户），不写进正文叙述。

---

## 0 · 判据总览

| # | P0 项 | 结果 | 关键证据 |
| --- | --- | --- | --- |
| 1 | 提交完整性 | ✅ | `rev-list --count develop..HEAD = 2`、`status --short` 空、独立 worktree `cargo check --lib` 通过 |
| 2 | 租户锁死（AC-001） | ✅ | 6 项攻击 403/400/401 各就各位；双租户对拍按**真值**核对无跨租户行 |
| 3 | 只有四个端点（AC-002） | ✅ | 5 路径 × 5 方法矩阵：只有 4 个 200，其余 405/404 |
| 4 | 启动校验 fail-closed（AC-003） | ✅ | 四种情形各一次（含 M-1 的两条新增拒绝条件） |
| 5 | 结果只给业务信息（AC-004） | ✅ | 覆盖警告可见；响应体 grep 表名/列名/SQL/`https://`/`#` **0 命中** |
| 6 | 页面壳（AC-005/007） | ✅ | 13,692 B、零依赖、零数据、三个安全头 |
| 7 | 凭证隔离（AC-006） | ✅ | 双向加载期拒绝 + 真 admin token 调门户 401 |
| 8 | 并发闸（M-2） | ✅ | 6/6 轮恰好 2×200+1×429；429 审计 `elapsed_ms=0`；变异必红 |
| 9 | 审计（AC-009） | ✅ | 82 行；七项齐全；无明文口令 / 无 SQL / 无表名 |
| 10 | 限额（AC-010） | ✅ | `max_rows=50` ⇒ rows=50 + truncated + 警告；超时 504 固定话术 |
| 11 | 不退化（AC-011） | ✅ | lib 638 / doc 4 / portal 16 / live 5；5 条既有红灯在 `develop` 上逐条复现；禁改面 diff 空；OUTLETS 57→59 |

**BA 已裁定项复核**：
- **M-1（接受「交付问题 ≠ 草稿」）**：复核通过 —— 交付的 5 条问题全部是业务列投影 + 业务 id；且「投影实例 IRI」「id 含表名」这两类**确实是启动期拒绝条件**（§5.2 逐条实测）。
- **M-2（并发闸真生效）**：复核通过 —— 见 §9（含变异）。

---

## 1 · 环境与做法

| 项 | 值 |
| --- | --- |
| 元数据库 | `172.17.0.87:5432/anspire_real`（项目 `anspire_r2rml_excl` 第 2 版，`frozen_at` 非空、有快照） |
| 源库 | `172.16.120.20:30432/anspire_open_uat`（**只读**，仅做少量 `count/select` 真值核对） |
| schema / 政策 / 问题 | `code/out/schema.json`（252,686 B）/ 仓库根 `query-policy.json` / `portal-questions.json` |
| 门户实例 | `ontok portal serve --bind 127.0.0.1:8911 --project anspire_r2rml_excl --version 2 --timeout-ms 10000`（另起 8912/8913/8914 做限额与超时） |
| 控制面实例 | `ontok console serve --bind 127.0.0.1:8899`（只为造一张**真的** admin 口令） |

**租户怎么选的**（⛔ 没有复用 Dev 的任何 id，全部自己从源库 `balances` / `deductions` / `licenses` / `usage_details` / `customer_subscriptions` 里筛）：

| 代称 | `sha256(id)[:12]` | 数据面 | 用途 |
| --- | --- | --- | --- |
| **A** | `0c153ca77658` | balances 1 · licenses 4 · usage_details 17 · customer_subscriptions 2 | 主对拍租户 |
| **B** | `4cd085a69bb5` | balances 1 · licenses 2 · usage_details 11 · customer_subscriptions 2 | 主对拍租户 |
| **C** | `c0f46d4b4089` | **deductions 4 行**（全库只有这一个租户有扣费行） | AC-004 覆盖警告（`charges_by_currency` 只能用它） |
| **D** | `74c666975c27` | usage_details 107 行 / 5 个模型 | AC-010 截断（50 行） |
| **对拍替身** | `7086c3ee9aff` | balances 非零（158,668.92）· licenses 5 | 复跑 live 对拍用例（值与 A 全部不同） |

口令表由我生成（52–53 字符随机串，`kind:"portal"`，分别绑 A/B/C 与 D/E），**不是** `portal-tokens.example.json` 的假值。

---

## 2 · 提交完整性（PASS）

| 判据 | 实测 |
| --- | --- |
| `git rev-list --count develop..HEAD` | **2** ✓（`8191e1f` 客户门户主体、`02f4d2d` 并发闸） |
| `git status --short` | **空** ✓ |
| 独立 worktree 编译 | `qa-req126`（detached `02f4d2d`）`cargo check --lib` → `Finished dev profile ... in 38.82s`，exit 0 ✓ |
| 二进制 | 同一 worktree `cargo build --bin ontok` → exit 0（全部实测都跑这份二进制） |
| 改到的文件 | 17 个：`src/portal/**`（新 8 文件）、`src/cli.rs`、`src/lib.rs`、`src/domain/pipeline.rs`（出口登记）、`portal-questions.json`、`portal-tokens.example.json`、2 个 REQ-107 枚举口径、2 个新测试文件 |

---

## 3 · 租户锁死（AC-001，核心）

### 3.1 六项攻击（真进程，真库）

| 攻击 | 期望 | 实测 | 判定 |
| --- | --- | --- | --- |
| `POST /q/charges_by_currency` body `{"tenant":"<B 的 id>"}` | 403 `TENANT_FROM_CLIENT` | **403** `TENANT_FROM_CLIENT` | ✅ |
| body `tenant_id` / `customer` / `customer_id` / `owner` 各一次 | 403 | **403 × 4**（逐个都点名该字段） | ✅ |
| `GET /api/questions?tenant=…` | 400 `QUERY_PARAM_FORBIDDEN` | **400** `QUERY_PARAM_FORBIDDEN` | ✅ |
| `POST /q/…?tenant=…`（带合法 token） | 400 | **400** `QUERY_PARAM_FORBIDDEN` | ✅ |
| `GET /api/health?tenant=…` | 400 | **400**（健康检查也不吃参数） | ✅ |
| body `sparql` / `query` / `sql` 各一次 | 400 `CUSTOM_QUERY_FORBIDDEN` | **400 × 3**，且**没有执行**（审计里没有对应 OK 行） | ✅ |
| body 其它字段（`{"x":1}`） | 400（不静默忽略） | **400**「不认识的字段：x」 | ✅ |
| 无 token / 错 token / 空 Bearer | 401 | **401 × 3** | ✅ |
| 只把 token 放 query string | 401（只认头） | **401** | ✅ |
| **console 的 admin token 调门户** | 401 | **401**（同一张 token 在 `console /api/projects` 上是 **200** —— 先证明它是真 admin token） | ✅ |
| admin token 执行 `POST /q/<id>` | 401 | **401** | ✅ |

原文（节选）：

```json
// body 带 tenant
{"code":"TENANT_FROM_CLIENT","hint":"租户由访问口令决定 …… ⛔ 门户不会「按你说的租户查」","message":"请求里不能带 `tenant`。"}
// URL 带 tenant
{"code":"QUERY_PARAM_FORBIDDEN","hint":"租户由访问口令决定：⛔ 不接受 `tenant`（或 `token` 之类的参数）……","message":"门户端点不接受 URL 查询参数。"}
// body 带 sparql
{"code":"CUSTOM_QUERY_FORBIDDEN","hint":"只能执行 GET /api/questions 里列出的预置问题 ……","message":"门户不接受自定义查询（请求体里有 `sparql`）。"}
```

### 3.2 双租户对拍（A vs B，5 条问题）

| 问题 | A 状态/行数 | B 状态/行数 | 值级交集 |
| --- | --- | --- | --- |
| `charges_by_currency` | 200 / 0 | 200 / 0 | ∅ |
| `my_balance` | 200 / 1 | 200 / 1 | `{0.00000000}` ← **两边真值都是 0** |
| `usage_by_model` | 200 / 2 | 200 / 1 | ∅ |
| `my_subscriptions` | 200 / 2 | 200 / 2 | ∅ |
| `my_api_keys` | 200 / 4 | 200 / 2 | ∅ |

⚠️ **我没有把「值相同」当成泄漏**：`my_balance` 两边都是 `0.00000000`（我直接查源库确认两个租户的 `recharge_balance` 确实都是 0）。所以除了「值级交集」，我另做了**真值核对**：

| 判据 | 结果 |
| --- | --- |
| A 的回答 ⊆ A 的源库真值（5 条逐条） | ✅ 5/5（行数完全相等：0/1/2/2/4） |
| B 的回答 ⊆ B 的源库真值（5 条逐条） | ✅ 5/5（行数完全相等：0/1/1/2/2） |
| A 独有的行出现在 B 的回答里 | **0**（`usage_by_model` A 独有 2 行、`my_subscriptions` A 独有 2 行、`my_api_keys` A 独有 4 行、B 独有 1/2/2 行 —— 一条都没串） |
| B 独有的行出现在 A 的回答里 | **0** |
| 至少一对两边都有数据（非空转） | ✅ 4 条（`my_balance` / `usage_by_model` / `my_subscriptions` / `my_api_keys`） |
| 两边真值都非空且**不同**（可区分） | ✅ 3 条（`usage_by_model` / `my_subscriptions` / `my_api_keys`） |

**另一组替身对拍（A vs `7086c3ee9aff`）5 条问题值级交集全部为 ∅** —— 这也是我复跑 Dev 的 live 用例所用的那组（见 §12.4）。

### 3.3 代码审查：没有「从请求读 tenant」这条路径

`src/portal/**` 里所有 `tenant` 出现处逐条看过，**唯一**进入查询的赋值是：

```rust
// src/portal/mod.rs:361-369（唯一的 PlanInput 构造）
let input = PlanInput {
    ...
    // ⛔ 唯一的租户来源：口令里那一行。
    tenant: &entry.tenant,
    max_rows: entry.max_rows,
};
```

`http.rs` 里对请求体出现的 `tenant/customer/customer_id/owner/tenant_id` **只有拒绝分支**（`guard_body`），URL 查询串**只有拒绝分支**（`require_token`）。`PortalAnswer` 类型没有 `tenant` / `tenant_applied` 字段。「从请求读租户」在类型层写不出来。✓

---

## 4 · 四端点（AC-002，PASS）

5 路径 × 5 方法全打一遍（带合法 token）：

| 路径 | GET | POST | PUT | DELETE | PATCH |
| --- | --- | --- | --- | --- | --- |
| `/` | **200** | 405 | 405 | 405 | 405 |
| `/api/health` | **200** | 405 | 405 | 405 | 405 |
| `/api/questions` | **200** | 405 | 405 | 405 | 405 |
| `/q/my_balance` | 405 | **200** | 405 | 405 | 405 |
| `/api/sparql` | **404** | **404** | **404** | **404** | **404** |
| `/api/catalog` · `/api/ask` | 404 · 404 | — | — | — | — |

- `GET /api/sparql` ⇒ **404 `NOT_FOUND`**，正文可读并列出「门户只有四个端点 …… ⛔ 没有 /api/sparql」✓
- `POST /q/does_not_exist` ⇒ **404 `UNKNOWN_QUESTION`**，正文列出可用问题 id ✓
- `GET /q/<id>` ⇒ **405** ✓
- `GET /api/questions` 带 token ⇒ 5 条卡片，字段只有 `{id,title,note}`（**没有** `sparql`）✓

---

## 5 · 启动校验 fail-closed（AC-003，PASS）

### 5.1 四种情形各一次

| 情形 | 期望 | 实测（exit=1，绑定 socket 之前） |
| --- | --- | --- |
| `reviewed_by` 为空 | 拒绝 + 点名 | `问题 charges_by_currency 没有签名：reviewed_by 为空 ……` ✓ |
| 表未在 policy 登记（`admin_roles`） | 拒绝 + 点名问题 id + 表名 | `问题 role_catalog 的查询过不了规划器 …… 租户过滤无法确定：表 admin_roles 既没有在声明里登记过滤列，也不在 publicDimensions` ✓ |
| 属性不存在（`deductions__currncy`） | 拒绝 + 点名 | `问题 charges_typo …… 属性 :deductions__currncy 不存在 —— 类 deductions 里没有这个属性。它有的属性：…` ✓ |
| 全合法（交付的 5 条） | 启动成功 | 8911 起服务，横幅打印 5 条 / 签名 / 只读会话读回值 ✓ |

### 5.2 M-1 的两类新增拒绝条件（BA 裁定要求复核）

| 反例 | 实测 |
| --- | --- |
| 投影实例 IRI（`SELECT ?sub ?pkg WHERE { ?sub :customer_subscriptions__status ?pkg }`） | **拒绝启动**：`问题 my_subscriptions 把内部标识（IRI）当成了结果列 —— 客户看到的是 <项目>#<表>/<id>，一个字段同时泄漏项目名与表名` ✓ |
| id 含表名（`my_licenses` + `licenses`） | **拒绝启动**：`问题 my_licenses 的 id 里含表名 licenses —— id 会出现在 URL / 问题列表 / 审计里` ✓ |

⇒ **M-1 的裁定成立**：交付问题与草稿不同是对的，而且这两类问题被做成了**启动期**拒绝（不是运行期 500，也不是跳过继续跑）。

---

## 6 · 结果只给业务信息（AC-004，PASS）

`POST /q/charges_by_currency`（C 的 token，`deductions` 是 `main_only`）→ 200，全文：

```json
{"columns":["cur","total"],"elapsed_ms":106,"limits":{"max_rows":200,"timeout_ms":10000},
 "objects":[{"coverage":"main_only","coverage_label":"只覆盖主账号","name":"扣费明细"}],
 "question":{"id":"charges_by_currency","note":"按扣费明细的币种汇总。这张表只覆盖主账号：子账号产生的扣费不在结果里，金额可能偏低。","title":"我的扣费明细（按币种汇总）"},
 "row_count":1,"rows":[["RMB","0.00011800"]],"truncated":false,
 "warnings":["「扣费明细」只覆盖主账号：子账号产生的部分不在结果里，金额可能偏低 —— 请不要把偏小的数字读成「没有业务」"]}
```

| 判据 | 实测 |
| --- | --- |
| 覆盖警告可见 | ✅ `warnings[0]` 就是那句「只覆盖主账号……金额可能偏低」 |
| 业务对象用业务名（不是表名） | ✅ `objects[0].name = "扣费明细"`，`coverage=main_only`、`coverage_label="只覆盖主账号"` |
| `truncated` 字段存在且会被页面渲染 | ✅ 字段在（本例 `false`）；截断场景见 §11 |
| 响应体 grep `deductions` / `customer_id` / `main_customer_id` / `SELECT` / `sql_echo` / `https://` / `#` | **0 命中**（另加 `select`、`sparql`、`tenant_applied`、`FROM ` 也是 0） |
| 对 A/B 的其余 10 个回答做同样 grep | **0 命中** |

⚠️ 关键点：`query-policy.json` 里 `deductions` 的 `coverageNote` 原文含 `main_customer_id`，而**客户看到的警告里没有它**（`coverage_warning()` 检测到技术标识符形态就整句退回标准话术）—— 这一层是有效的。

---

## 7 · 页面壳（AC-005 / AC-007，PASS）

| 判据 | 实测 |
| --- | --- |
| 体积 | **13,692 B**（< 120 KB，余量 ~9×） |
| `<script src=` / `<link` / `https://` / `http://` / `@import` / `integrity=` / `//cdn` | **各 0** |
| 项目名 / 版本 / 表名 / 问题标题 / 问题 id / 口令 | **0 命中**（逐个 grep：`anspire`、`r2rml`、`第 2 版`、`deductions`、`balances`、`licenses`、`usage_details`、`customer_subscriptions`、`customer_id`、`main_customer_id`、5 条 title、5 个 id、3 张口令） |
| 壳里不出现第 5 个端点 | `/api/sparql`、`/api/catalog`、`/api/describe`、`/api/apply` 各 0 |
| 服务端零注入 | 两次 `GET /` **逐字节相同** |
| 三个安全头 | `content-security-policy`（`default-src 'self'` + `frame-ancestors 'none'`）/ `x-content-type-options: nosniff` / `referrer-policy: no-referrer` 全在 |
| 401 响应也带头 | ✅（连错误正文都带） |
| XSS 面（AC-008 静态面） | 交付 HTML 里 `innerHTML`/`outerHTML`/`insertAdjacentHTML`/`document.write`/`eval(` **各 0**；服务端字符串只经 `txt()`（`textContent`）写入；对象名原样保留的单元判据在 lib 里绿 |

---

## 8 · 凭证隔离（AC-006，PASS，双向）

| 方向 | 命令 | 实测 |
| --- | --- | --- |
| console 读到 `kind:"portal"` | `console serve --tokens portal-tokens.json …` | **拒绝加载**：`token 表不合法：unknown variant 'portal', expected 'admin' or 'query'`，exit 1 |
| portal 读到 `kind:"admin"` | `portal serve --tokens admin-tokens.json …` | **拒绝加载**：`第 1 条（label="qa-ops"）的 kind = "admin" —— ⛔ 管理 / 查询凭据不能当门户口令用`（并指出没有 tenant），exit 1 |
| portal 读到 `kind:"query"` | `portal serve --tokens query-tokens.json …` | **拒绝加载**：`kind = "query" —— ⛔ 管理 / 查询凭据不能当门户口令用`，exit 1 |
| 真 admin token 调门户 | 同一张 token 先在 `console /api/projects` 拿到 **200**，再打 `portal /api/questions` | **401**（门户连「这张 token 是真的」都不承认） |

---

## 9 · 并发闸（M-2，PASS，含变异）

### 9.1 三连击 ⇒ 恰好 2×200 + 1×429（cap=2，**6 轮全中**）

```
trials = [[200,200,429], [200,200,429], [200,200,429], [200,200,429], [200,200,429], [200,200,429]]
```

429 正文（无表名 / 列名 / SQL / IRI，grep 0 命中）：

```json
{"code":"TOO_MANY_CONCURRENT",
 "hint":"同一个访问口令同时最多 2 个问题在跑。等前面的结果出来再点一次（串行最稳）；要更高的并发请让服务方调大你口令的并发上限。",
 "message":"同时在进行的问题太多了，这一次没有执行。"}
```

### 9.2 「不排队」的硬证据：429 的 `elapsed_ms = 0`

| 指标 | 实测 |
| --- | --- |
| 429 审计行条数 | 跑并发判据那一刻 13 条；本轮全部实测结束后累计 **14** 条 |
| 429 的 `elapsed_ms` | **全是 0**（max = 0） |
| 同一台服务的成功查询耗时中位数 | **101 ms** |
| 对比 | 429 的耗时比一次真查询**低两个数量级** ⇒ 它没有等库、没有排队，是「拿不到名额立刻回」 |

审计原文（节选）：

```
audit ts=2026-10-23T10:41:11.068Z event=portal token=sha256:1988bd5954b4 tenant=<A> qid=usage_by_model rows=- elapsed_ms=0 code=TOO_MANY_CONCURRENT
audit ts=2026-10-23T10:41:11.228Z event=portal token=sha256:1988bd5954b4 tenant=<A> qid=usage_by_model rows=2 elapsed_ms=160 code=OK
```

### 9.3 按 token 隔离 + 释放后恢复

| 场景 | 实测 |
| --- | --- |
| A 打满（2 个在跑）时同时发 A 第 3 个 + B 1 个 | **A₃ = 429（6.2 ms）**、**B = 200**（239.8 ms —— B 等的是源库只读池的 2 条连接，不是 A 的名额；这是模块头登记的已知边界） |
| A 释放后再打 | **200**（恢复） |
| 串行 3 连 | **200 × 3**（闸只卡「同时几个」，不卡速率） |

### 9.4 变异：把 `acquire` 改成「等待而非立刻 None」

| 步骤 | 结果 |
| --- | --- |
| 变异（`qa-req126`，未提交） | `acquire` 改为「循环等名额，最多 2 s」 |
| `cargo test --test test_req126_portal ac010_` | **1 failed / 2 passed**：`ac010_the_n_plus_first_request_on_a_token_is_429_and_does_not_wait` **红**，断言原文 `429 必须是**立刻**给的（夹具池 150 ms 才超时）：audit … elapsed_ms=2002` |
| `cargo test --lib portal::limit`（同一变异下） | 6 passed —— **单测层抓不住**（它们只看「N+1 返回 None」，不看「多快返回」） |
| 还原后 | `ac010_` 3 passed（0.18 s）；`git status` 干净 |

⇒ **「不排队」是被判据守住的**（红的是集成判据 `ac010_…_does_not_wait`；单测层不承重，见 N-3）。
⚠️ 变异只改了实现、没提交；`git diff` 已还原为空。

---

## 10 · 审计（AC-009，PASS）

一轮完整实测后 `audit.log` 共 **82 行**（另有 8912/8913/8914 三个实例的独立审计文件）。

| 判据 | 实测 |
| --- | --- |
| 行数 ≥ 40 | **82** ✓ |
| 七项齐全（`ts` / `token` 摘要 / `qid` / `tenant` / `rows` / `elapsed_ms` / `code`） | 82/82 行都齐 ✓ |
| 口令明文 | **0**（3 张口令逐条 grep） ✓ |
| SQL / 查询原文 | **0**（`SELECT`/`select`/`FROM `/`WHERE `/`sql_echo`/`sparql`/`https://`） ✓ |
| 表名 / 列名 | **0**（`deductions`/`balances`/`licenses`/`usage_details`/`customer_subscriptions`/`customer_id`/`main_customer_id`/`usage_amount`/`recharge_balance`） ✓ |
| `token` 只记摘要 | 全部形如 `sha256:<12 位>` 或 `-` ✓ |
| 429 有行 | `code=TOO_MANY_CONCURRENT` × **14** ✓ |
| 401 有行 | `code=UNAUTHORIZED` × **9** ✓ |
| 其它事件 | `OK` 46 · `TENANT_FROM_CLIENT` 5 · `QUERY_PARAM_FORBIDDEN` 3 · `CUSTOM_QUERY_FORBIDDEN` 3 · `UNKNOWN_QUESTION` 1 · `BAD_REQUEST` 1 ✓ |
| 客户端可控 id 的注入 | 无伪造行（`safe_qid` 只放行 `[a-z0-9_-]{1,64}`） ✓ |

---

## 11 · 限额（AC-010，PASS）

| 场景 | 实测 |
| --- | --- |
| `max_rows=50` 的 token + **一条真能出 50+ 行的口径**（QA 临时声明 `usage_lines`，同样过启动校验；租户 D 有 107 行） | **rows=50、truncated=true**、`limits.max_rows=50`，警告「结果超过本次上限（50 行），只显示前 50 行……」✓ |
| `max_rows=3` + 交付的 `usage_by_model`（租户 D 有 5 个模型） | **rows=3、truncated=true** + 同一句截断警告 ✓（页面 `renderAnswer` 会把它写进 `meta` 与 `warnbox`） |
| `max_rows=50` + 只有 5 行的问题 | rows=5、truncated=**false**（不误报截断）✓ |
| `max_rows` 来自口令而非请求 | 回答里 `limits.max_rows` = 口令那一行；请求体带未知字段 ⇒ 400 ✓ |
| 截断响应 grep 表名/列名/SQL/IRI | **0 命中** ✓ |
| 超时（`--timeout-ms 1`） | **504 `TIMEOUT`**，固定话术：「这个问题这次查询超时了，没能取到结果。」+ hint；grep `SELECT`/表名/`https://` **0 命中**（不回显 SQL） ✓ |

> 说明：交付的 5 条问题对**任何**真实租户最多 33 行（`licenses`），拿不到 50 行，所以「50 行」这一条我另用一份 QA 临时声明（`usage_lines`，未提交）验证 —— 它走的是同一个启动校验与同一条查询路径，能出 107 行。

---

## 12 · 不退化（AC-011，PASS）

### 12.1 基线数字（全部本轮实测）

| 项 | 基线要求 | 实测 |
| --- | --- | --- |
| lib | **638** | **638 passed / 0 failed** ✓ |
| doc | **4** | **4 passed** ✓ |
| `test_req126_portal` | **16** | **16 passed** ✓ |
| `test_req126_live` | **5** | 5 条（4 passed，1 条见 N-1；换一组值不撞的租户后 5/5 绿） |
| REQ-107 四组 | — | outlet_pipeline **12** · red_lines **2** · source_enumeration **10** · zero_visible_change **3**，全绿 ✓ |

### 12.2 禁改面

| 判据 | 实测 |
| --- | --- |
| 四红线（`tests/drop_rate.rs` / `tests/test_fk_naming.rs` / `src/domain/view.rs` / `src/domain/review_view.rs`） | `git diff develop..HEAD` 里 **无**这几个文件 ✓ |
| `src/mcp/tools.rs` / `guard()` | diff **空**（`git diff --name-only develop..HEAD -- src/mcp/` 无输出）✓ |
| `src/domain/property_filter.rs` | diff **空** ✓ |
| `console` / `query` 路由 | `src/console/**`、`src/query/**` diff **空** ✓（门户是独立模块 `src/portal/**`） |
| `Cargo.toml` / `Cargo.lock` | **零改动** ✓（零新增依赖） |
| `src/bin/ontok-gate-preflight.rs` | 未动 ✓ |
| 删测试 / `#[ignore]` / 放宽断言 | diff 里**没有**删任何 `#[test]`、没有新增 `#[ignore]`、没有 `>=` 式放宽；唯一的「删除」是 `outlet_count(Track::Internal) 27 → 29`（精确相等，随出口数上调）✓ |
| 出口登记 | `I23`（门户页面壳 HTML）/ `I24`（门户 API JSON）已登记为**内部轨**；`OUTLETS` **57 → 59**，内部 **27 → 29**，`registry_is_the_declared_baseline` 与 `test_req107_outlet_pipeline` 逐项精确相等 ✓ |

### 12.3 5 条既有红灯：自己复现 + 归因

全仓 `cargo test --no-fail-fast`（lib + 4 个 bin + **91** 个 `tests/*.rs` 目标 + doc）在 **HEAD** 上：**4 个既有目标红 / 共 5 条测试**。我在 `code/`（develop worktree，`d54c130`）上逐个复跑同样 4 个目标：**同样 5 条、同样文案、同样数量**。

| 目标 | 失败测试 | develop `d54c130` 上 |
| --- | --- | --- |
| `drop_rate` | `real_schema_drop_rate_report` | 同样 1 failed（读不到 `../.review-workspace/fix-props/schema.json`；测试**故意**失败） |
| `test_fk_naming` | `test_real_schema_customer_id_has_exactly_one_display_name` | 同样 1 failed / 5 passed（同一份脱敏 schema 缺失，需 `ONTOK_SCHEMA_JSON`） |
| `test_agent_orchestration` | `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 同样 1 failed / 35 passed（★ items 与 legacy 不一致，REQ-059 区域） |
| `test_retained_names_req084` | `test_req084_every_registered_name_is_still_literally_at_its_line` + `…no_unregistered_real_name_remains…` | 同样 2 failed / 4 passed（`src/extract/propose.rs:280` 登记行漂移） |

⇒ **5 条全部是既有红灯，与 REQ-126 的改动面（`src/portal/**` + 出口登记 + cli 子命令）无交集。**

### 12.4 第 6 个红点：`test_req126_live::ac001_…`（新，见 N-1）

用 A/B 这组租户跑全量时，它红在「值级交集非空」：

```
· charges_by_currency: A 0 行 / B 0 行 / 交集 0
· my_balance:          A 1 行 / B 1 行 / 交集 0   ← 两边都是 "0.00000000"
```

换成「值与 A 全不相同」的替身租户（`7086c3ee9aff`，余额 158,668.92、授权名与 A 无重叠）后：

```
test ac001_two_real_tenants_see_disjoint_data_and_no_internals ... ok
test result: ok. 1 passed; 0 failed
```

⇒ 这条红**不是产品缺陷**（§3.2 的真值核对证明没有跨租户数据），是**判据口径**问题：它把「两个租户某列的值恰好相同」当成泄漏证据。零余额是极常见的数据形态，所以这条用例在真实租户上**很容易假红**。建议（N-1）改成与「每租户真值」对拍，或只对「含租户特有行」的问题做交集判据。

---

## 13 · 阻断项 / 非阻断项

### 阻断（0 项）

无。AC-001 的租户锁死、AC-004 的业务信息、AC-002 的四端点、M-2 的并发闸都按判据实测通过，且变异能抓住「排队」这种退化。

### 非阻断（4 项，建议后续处理，不影响本次合并）

| # | 项 | 证据 | 建议 |
| --- | --- | --- | --- |
| **N-1** | live 判据 `ac001_two_real_tenants_see_disjoint_data_and_no_internals` 用**行值交集**当泄漏证据，遇到「两边余额都是 0.00000000」这类真值巧合就假红（真实租户上很容易触发） | §12.4：A/B 红、A/替身绿；§3.2 真值核对证明零跨租户行 | 改成与每租户源库真值对拍；或把交集判据限制在「含租户特有行」的问题上（`usage_by_model` / `my_subscriptions` / `my_api_keys`），`my_balance` 这种纯聚合值只判「⊆ 本租户真值」 |
| **N-2** | `GET /?tenant=…` 返回 **200**（页面壳），而 AC-001 表述是「URL 带 tenant ⇒ 400/403」 | §3.1 末行：壳免 token、零数据、不读参数，任何数据面都不受影响 | 口径上二选一：① 在 `index()` 里也拒查询串（与 `/api/health` 一致，最省事）；② 在 AC 里把「壳（匿名、零数据）不接受也不解释参数」写成明文例外 |
| **N-3** | 「不排队」这条性质只有**一条**集成判据（`ac010_…_does_not_wait`）承重：`portal::limit` 的 6 条单测在「等待而非立刻 None」的变异下**全绿**（只是变慢到 2 s） | §9.4 | 给单测加一条「占满后 `acquire` 必须在 ~0 ms 内返回 None」的计时判据（比如 `elapsed < 5 ms`），让退化在单测层也红 |
| **N-4** | 审计行里的 `tenant=` 是**明文 UUID**（AC-009 要求记租户，代码注释也承认「摘要不加盐」只针对口令） | §10 的审计原文 | 已登记为设计选择，不要求改；若客户合规要求「日志里不留客户 id」，可另开一条需求把它改成 `sha256:…` 短摘要（会影响排障时的 grep 习惯） |

---

## 附 · 复跑命令（可复现）

```powershell
# 0) 环境
$env:ONTOK_DATABASE_URL       = 'postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
$env:ONTOK_TEST_DATABASE_URL  = $env:ONTOK_DATABASE_URL
$env:ONTOK_SOURCE_DATABASE_URL= 'postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat'

# 1) 独立 worktree（不碰 feature 分支）
git worktree add qa-req126 --detach 02f4d2d
cd qa-req126; cargo check --lib; cargo build --bin ontok

# 2) 起门户（口令表见 .local/qa-req126/portal-tokens.json，3 张真租户口令）
ontok portal serve --bind 127.0.0.1:8911 --tokens portal-tokens.json `
  --project anspire_r2rml_excl --version 2 --schema <repo>/code/out/schema.json `
  --policy <feature>/query-policy.json --questions portal-questions.json `
  --audit-out audit.log --timeout-ms 10000

# 3) 判据（脚本在 .local/qa-req126/scripts/）
python qa_http.py        # AC-001/002/004
python qa_pair_truth.py  # 双租户真值核对（不把「值相同」当泄漏）
python qa_page.py        # AC-005/007
python qa_conc.py        # M-2（三连击 ×6 + 隔离 + 429 elapsed）
python qa_audit.py       # AC-009
python qa_limits.py 8912 limits | 8913 timeout | 8914 rows50   # AC-010

# 4) 测试与变异
cargo test --no-fail-fast                      # lib 638 / doc 4 / portal 16 / live 5 + 5 条既有红
cargo test --test test_req107_red_lines --test test_req107_outlet_pipeline `
           --test test_req107_source_enumeration --test test_req107_zero_visible_change
# 变异：把 Limiter::acquire 改成「等名额（≤2 s）」⇒ ac010_…_does_not_wait 必红（elapsed_ms=2002）
```

---

*QA · 2026-10-23 · 只做审核与变异，未改 `feature/REQ-126`、未 push；报告文件未提交（由 BA 按其流程落库）。*
