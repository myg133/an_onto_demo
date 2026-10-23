# QA 审核：REQ-125 · 内部工作台前端（Pre-merge）

| 项 | 值 |
| --- | --- |
| 审核对象 | `feature-REQ-125` · `feature/REQ-125` · HEAD `37b6526` · 基线 `develop` `eaa7c96` |
| 审核方式 | 独立 worktree `qa-REQ-125`（detached `37b6526`）+ 自己起服务（真 HTTP）+ 仓库内 Node 假 DOM 探针 + 变异 |
| 审核日期 | 2026-10-23 |
| 审核人 | QA Agent |

---

## 1 · 结论

**FAIL**（阻断 1 项）

| 判定 | 内容 |
| --- | --- |
| 12 条 AC | **无一条失败**：11 条为亲手实测，AC-004 的「逐项数值相等」受 L-3 约束（无浏览器）以结构+端点实测替代，见下表 |
| L-2 / L-3 | **达标** |
| **L-1 ①** | **未达标（阻断）**：只起 `console serve` 时，查询台 / 目录浏览拿到 **404**（只填管理口令）或 **403**（两个口令都填）—— 前者只显示「[NOT_FOUND] 没有这个路径。」+ 一串**管理端点**清单，后者把刚填的 query token 抹掉并让用户「再填一次」（死循环）。两条路都没有 BA 要求的「查询服务未接入：请把 query serve 挂到同源 /api/sparql（见文档）」 |
| **L-1 ②** | **只做到一半**：启动横幅写了（✓），`console serve --help` **没写**（✗） |
| L-1 ③ | 达标（`dev_proxy.ps1` 在仓库、能起、实测分流正确） |

> 一句话：**壳是零数据的、token 只在 sessionStorage、查询台四段元信息真显示且变异能抓住** —— 技术判据都硬。但 BA 在 L-1 里定的「让限制在界面上可见」这条，恰好落在**唯一没写**的那个分支上：查询端点的 404/403。修复面很小（`request()` 里给 `opts.query` 的 404/403 加一个分支 + 补 `--help` 两行），但不该由 QA 放过。

### AC 覆盖矩阵（哪些是本轮亲手跑出来的）

| AC | 本轮证据 | 结论 |
| --- | --- | --- |
| 001 壳零数据 | 自己起服务 + grep（§3） | ✅ 实测 |
| 002 零依赖 + 体积 | grep + 字节数（§4） | ✅ 实测 |
| 003 token 位置 | grep + 点遍 7 个 tab 的 fetch spy（§5） | ✅ 实测 |
| 004 七视图可用 | 七视图容器/端点全消费（源码+测试）、12 个端点真服务可达（§11③ 代理实测 + §6 真回执）；**「逐项数值相等」未做浏览器手测** | ✅ 结构 + 端点实测（L-3 已接受无浏览器） |
| 005 只走既有 API | 路由 diff 只有 `GET /`；12 个调用点全是 `api/*`（§13「不算问题」） | ✅ 实测 |
| 006 危险操作确认 | 三处 spec + 取消 0 fetch + 变异（§7） | ✅ 实测 |
| 007 保存失败保留输入 | 服务端**真 400** 喂页面（§8） | ✅ 实测 |
| 008 查询台元信息 | **真回执** 3 份 + 变异（§6） | ✅ 实测 |
| 009 错误可见 | 网络失败 / 401 / 404 三条路径（§8、§11①）；500 未单独造 | ✅ 3/4 路径实测 |
| 010 无 XSS | 假 DOM 陷阱 + 字面量计数（§9） | ✅ 实测 |
| 011 响应头 | `HEAD /` 三头（§10） | ✅ 实测 |
| 012 不退化 | 基线数字 + 禁改面 + 全仓红灯归因（§12） | ✅ 实测 |

---

## 2 · 提交完整性（PASS）

| 判据 | 实测 |
| --- | --- |
| `git rev-list --count develop..HEAD` | **2** ✓（`302bba5` 主体 + `37b6526` 补 L-2 注释） |
| `git status --short`（feature worktree） | **空** ✓（无 modified / 无 untracked） |
| 独立 worktree 里 `cargo check --lib` | **通过**（`qa-REQ-125` = detached `37b6526`，`Finished dev profile ... in 12.49s`，exit 0）✓ |
| 变异 worktree 是否依赖 untracked 源文件 | 否：`qa-REQ-125` 建立时 `git status --short` 为空，check 只读 tracked 源 ✓ |

> 方法说明（诚实登记）：`qa-REQ-125` 的 `CARGO_TARGET_DIR` 指向 feature worktree 的 `target/`，**只共享编译缓存**（19 GB 预热），源码全部来自 `qa-REQ-125` 自己的 checkout；check 与 build 都在该 worktree 的源码上重新编译 `ontok` 本体（12.5 s / 32.6 s）。没有复制、没有软链任何源文件。

变更面（11 个文件，+2443 / −16）：

```
src/console/http.rs (+35/−2)   src/console/mod.rs (+6)   src/console/page.rs (新, 1423 行)
src/console/serve.rs (+9)      src/domain/pipeline.rs (+45/−16，I22 登记与口径)
tests/test_req125_page.rs (新 427)   tests/fixtures/req125/{README.md,dev_proxy.ps1,page_probe.js} (新)
tests/test_req107_outlet_pipeline.rs / test_req107_source_enumeration.rs（只改口径 56→57 与模块枚举）
```

---

## 3 · 壳零数据（AC-001，PASS）—— 自己起服务自己 grep

### 现场

```
ontok console serve --bind 127.0.0.1:8899 --tokens admin-tokens.json --project secretproj
  --schema code/out/schema.json
  --exclusions qa-decls/exclusions.json --policy qa-decls/policy.json --tenancy qa-decls/tenancy.json
```

- 真实库 `anspire_real` 里建了 `secretproj`，**7 个版本（v1..v7）**；三份声明投毒 `绝密对象`（`reviewed_by`）。
- 启动横幅自证：`· ontok console serve 就绪：绑定 127.0.0.1:8899（仅本机） · 项目 secretproj · 当前第 7 版`。

### 判据

| 字符串 | 来源（服务端确实有） | `GET /` 命中 |
| --- | --- | --- |
| `secretproj` | `/api/projects` → `{"project":"secretproj","versions":[1..7]}` | **0** |
| `第 7 版` / `v7` | 启动横幅「当前第 7 版」 | **0** |
| `绝密对象` | `/api/config/exclusions`、`/api/config/tenancy` 原文含该串（各 200，`contains=True`） | **0** |
| token 明文 `QA-ADMIN-TOKEN…` / `QA-QUERY-TOKEN…` | 本次启动用的凭据 | **0** |
| `admin-tokens` / `exclusions.json` / `policy.json` / `tenancy.json` | 启动参数与路径 | **0** |
| 版本 UUID `c64bfe10…`（v7 的 id） | `/api/projects` | **0** |

- `GET /` → **200**，`content-type: text/html; charset=utf-8`，**58664 字节**。
- **逐字节等于常量**：响应体 sha256 = `9BAD672D…E31E44` = `page.rs` 里 `CONSOLE_HTML` 字面量的 sha256（两者 58664 字节逐字节相同）⇒ 服务端**没有**注入任何东西，壳的零数据不是「这次没拼」而是「结构上没处拼」。

> 说明：`绝密对象` 是以**声明原文**（`/api/config` 可读）的形式投毒的，不是 DB 里的对象名 —— 本项目在真库里没有「对象名」这一栏可直接造。壳的零数据由「逐字节等于常量」兜底：任何项目/版本/对象名都不可能出现在壳里。

---

## 4 · 零依赖 + 体积（AC-002，PASS）

页面常量（58664 字节）内计数：

| 模式 | 次数 |
| --- | --- |
| `<script src=` | 0 |
| `<link href=` / `<link ` | 0 / 0 |
| `https://` / `http://` | 0 / 0 |
| `@import` | 0 |
| `integrity=` | 0 |

- 体积：**58,664 B < 200 KiB**（204,800）✓
- 反空转：**58,664 B > 20 KiB**（20,480）✓，且七个 tab、内联 CSS/JS 都在。

---

## 5 · token 存放（AC-003，PASS）

页面脚本（非注释）计数：

| 位置 | 次数 |
| --- | --- |
| `sessionStorage` | **12**（唯一存储） |
| `localStorage` | **0** |
| `indexedDB` | **0** |
| `document.cookie` | **0** |
| `token=`（查询串） | **0** |
| `location.search` / `location.hash` / `location.` / `history.` | **0 / 0 / 0 / 0** |

「点遍所有视图」实测（假 DOM + fetch spy，7 个 tab 全点）：

```
projects → api/projects                                  auth=Bearer ***
review   → api/review?proposal=p1                        auth=Bearer ***
audit    → api/audit?limit=100                           auth=Bearer ***
catalog  → api/catalog?page=1&page_size=50               auth=Bearer ***
⇒ URL 里 token 命中 0 · 绝对 URL 命中 0 · location./history. 使用 0
```

另：401 实测 ⇒ 清 `sessionStorage`（`before=1 → after=0`）+ 回登录态 + 显示「口令无效或权限不够（HTTP 401 UNAUTHORIZED）：缺 token。」✓

---

## 6 · 查询台元信息（AC-008，PASS；变异**能抓住**）

### 6.1 真实回执（自己起 `query serve` 打真源库，不是 Dev 的样本）

`ontok query serve --bind 127.0.0.1:8898 --project anspire_r2rml_excl --version 2 …`（v2 已冻结），用 query token 打 `POST /api/sparql`：

| 回执 | 查询 | 覆盖到的字段 |
| --- | --- | --- |
| `receipt_deductions.json` | `SELECT ?d ?a WHERE { ?d :deductions__deduction_amount ?a }` | `warnings`（main_only 漏数据）、`coverage=[{deductions, main_only}]`、`join_states=[]`→「0 条」、`truncated:false` |
| `receipt_query_serve.json` | `?l :licenses__customers ?c . ?b :balances__customers__customer_id ?c` | `join_states` 里 `balances→customers (balances__customers__source_customer_id) used=false` 真出现；`coverage` 3 张全 `complete` |
| `receipt_trunc.json` | `SELECT ?m WHERE { ?m a :llm_model }`（`max_rows=1`） | `truncated:true` + warnings「结果超过 1 行…已截断」+ `coverage=[{llm_model, complete}]` |

### 6.2 页面**会显示**什么（探针 `--dump` 打印的就是 `metaLines()` 的产出 = 页面那几行）

`node tests/fixtures/req125/page_probe.js receipt_deductions.json --dump`：

```
1. [warn] ⚠ 可能漏数据（warnings）：表 deductions：租户过滤用 customer_id，覆盖范围 = main_only …（原文）
2. [warn] ⚠ 覆盖面 main_only：表 deductions —— 子账号产生的数据不在结果内，金额 / 条数可能偏小。
3. [info] join_states[].used=false：0 条（本次查询涉及的边上，没有「涉及但未用」的）。
4. [info] truncated：否（row_count=0）
5. [info] limits：max_rows=2 · token 上限=200 · timeout_ms=10000
```

三份真实回执跑探针均 **exit 0**（30 / 31 / 29 条判据全过）。`used=false` 那句在 `receipt_query_serve.json` 下逐字出现「balances → customers … 这些边在本次查询里**没有走到**」。

> ⛔ 不是「只显示表格」：元信息由 `metaLines()`（`page.rs:973`）**一处**产出，表格另画；0 行时警告仍然显示（探针专有一条判据）。

### 6.3 变异（把 `metaLines` 里 warnings 那一支删掉）

```
（worktree 内）删掉 `if (warnings.length) { … } else { … }` 整段 → node page_probe.js receipt_deductions.json
  ✗ warnings 文案可见（AC-008 第一条）
  ✗ 0 行时警告仍然显示（不是「没数据就没风险」）
  ✗ 没有风险时逐项说「0 / 否」
  ⇒ MUT1_EXIT = 1（失败 3 条 / 通过 29 条）
恢复 → EXIT = 0
```

**判定：探针抓住了。** 「必须显示 warnings」这条是被判据守住的，不是靠人看。

---

## 7 · 危险操作（AC-006，PASS；变异**能抓住**）

三处危险动作都过 `guarded()`（`page.rs:542`）：`apply` / `freeze` / 声明保存。用假 DOM 把 `CONFIRM` 换成「记录 spec + 返回 false」实测：

| 动作 | 确认框标题 | 字段 | 取消后 fetch |
| --- | --- | --- | --- |
| apply | 应用提案 p1 | `["目标","后果"]` | **0** |
| freeze | 冻结第 3 版 | `["目标","后果"]` | **0** |
| 声明保存 | 保存声明 exclusions | `["目标","后果"]` | **0** |

（合计 fetch = 0、spec = 3；文案非空，freeze 的后果写「冻结后这一版**不能再改**…冻结不可撤销」，apply 写「写成**新版本**…不可撤销；已应用再 apply ⇒ 409」。）

**变异**（把 `guarded()` 改成不看确认、直接 `run()`）：

```
✗ apply 取消后 fetch 次数为 0
✗ 取消时连错误框都不该出现
✗ 声明保存取消后 fetch 次数为 0
⇒ MUT2_EXIT = 1（失败 3 条）
恢复 → EXIT = 0
```

**判定：探针抓住了。**「取消 ⇒ 不发请求」是被守住的。

---

## 8 · 声明保存失败保留输入（AC-007，PASS）

用**服务端真实 400** 跑页面渲染路径（`PUT /api/config/exclusions` 发一份缺 `rule` 的声明）：

```
status=400
{"code":"DECLARATION_INVALID","message":"exclusions 声明 … 不合法：… 缺 `rule`（或它是空白）…",
 "hint":"`exclude[]` 每条要有 table / category / reason（≥ 6 字）/ decided_by；category 只能是既有七类枚举之一。"}
```

喂给页面（假 DOM，`c-text` 里是含中文/换行/注释的用户输入）：

| 判据 | 实测 |
| --- | --- |
| 编辑器内容**逐字保留** | ✓（比较全等，未被清空/回滚） |
| 显示 `code` | ✓ `[DECLARATION_INVALID]` |
| 显示 `message` | ✓ 服务端原文（含「缺 \`rule\`」） |
| 显示 `hint` | ✓「怎么办：…」 |
| 状态栏明说 | ✓「保存失败：…（编辑器内容已保留，可直接改了再存）」 |

---

## 9 · XSS（AC-010，PASS）

- 假 DOM 里把 `<img src=x onerror=alert(1)>` 交给 `el()` / `makeTable()`：只进 `textContent`，表格里是**原文**；**`innerHTML` 被赋值次数 = 0**（假 DOM 的 `innerHTML` setter 是「赋值即抛」的陷阱）。
- 页面常量里 `innerHTML` / `outerHTML` / `insertAdjacentHTML` / `document.write` / `eval(` **各 0 处**；`textContent` 4 处写点（不是「零写点所以全绿」）。
- 唯一 `fetch(` 调用点 = 请求层一处（`page.rs:418`），视图没法各自造请求。

> ⚠️ 口径说明（不是缺陷，但 AC 字面要交代）：**整个 `page.rs` 文件**（含 Rust 文档注释）里这五个词共出现 6/2/2/2/2 次 —— 全是**注释在解释「本页没有它们」**，没有一处是调用点 / 注入点。判据（`ac010_no_innerhtml_anywhere`）只扫 `CONSOLE_HTML` 常量，这是对的；但「`page.rs` 各 0 处」这句话按字面不成立，报告里必须说清是「常量 0 处、全文件仅注释提及」。

---

## 10 · 响应头（AC-011）+ L-2 注释（PASS）

`HEAD /`（不是 GET）实测：

```
HTTP 200 · Content-Type: text/html; charset=utf-8 · Content-Length: 58664
Content-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline';
  style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self';
  base-uri 'none'; form-action 'none'; frame-ancestors 'none'
X-Content-Type-Options: nosniff
Referrer-Policy: no-referrer
```

三个头都在，且 CSP 逐字等于登记常量 `CSP`（`page.rs:1275`，测试也逐字对）。

**L-2**：`CSP` 常量旁的注释（`page.rs:1244–1277`）三点齐全 —— **为什么需要**（单文件内联 + 零外链的必然代价，拿掉会整段拒执行）、**代价**（不假装已防护：CSP 对内联注入的拦截被削弱，靠「只走 `textContent`」兜底）、**何时必须去掉**（一旦拆成外部 `.js`/`.css` 就必须删掉 `'unsafe-inline'`，改 `'self'`，需要内联片段就用 nonce/hash；并写明「拆了外部文件还留着」是两头都输）。**不是「因为内联」一句话，达标。**

---

## 11 · L-1 三条

### ① 失败要说人话 —— **未达标（阻断）**

实测（只起 `console serve`，`/api/sparql` / `/api/catalog` 都不存在）。**两条腿都断**：

**（a）只填管理口令 ⇒ 404**

```
POST http://127.0.0.1:8899/api/sparql  → 404
{"code":"NOT_FOUND","message":"没有这个路径。",
 "hint":"可用：GET /api/health（免 token）、GET /api/projects、POST /api/import、…"}

页面：q-meta => "[NOT_FOUND]没有这个路径。怎么办：可用：GET /api/health（免 token）、GET /api/projects、POST /api/import、…"
      msg    => "查询失败：没有这个路径。"
```

**（b）两个口令都填了（页面把 query token 用于查询端点）⇒ 403**

实测 `POST /api/sparql` + query token → 403 `TOKEN_KIND_FORBIDDEN`（"这是一张 query token，不能调用管理端点。"）。页面渲染：

```
q-meta => "[TOKEN_KIND_FORBIDDEN]这是一张 query token，不能调用管理端点。
           怎么办：管理端点只接受 kind=admin 的 token；查询 token 请用于 `ontok query serve`。⛔ 同一个 token 不许两用。
                   查询台 / 目录浏览要的是**查询口令**（query token，含租户），不是管理口令；在页面顶部第二个输入框填入后重试。"
```

- 服务端那句「查询 token 请用于 `ontok query serve`」是**唯一**沾到边的话，但**没说「挂到同源 / 反代」**，也没说「本服务不含查询端点」。
- 页面自己又追加一句「要的是**查询口令**…填入后重试」，并**把 `S.qt` 清空**（`page.rs:423–428`）—— 用户刚填的 query token 被抹掉，再填一次还是 403。**这在 L-1 拓扑下是个死循环**：只要 console serve 不挂查询端点，用户永远无法从这条提示里走出去。
- 目录浏览同一路径同样：`[NOT_FOUND] 没有这个路径。…` + `载入失败。`（`loadCatalog` 的 catch）。

**共同问题**：

- ❌ 没有「查询服务未接入」这句话；❌ 没有「把 query serve 挂到同源 /api/sparql」；❌ 没有「见文档」。
- ⚠️ 404 那条 hint **列的是管理端点**：用户读到的暗示是「/api/sparql 不是本服务的东西，用 /api/projects 吧」——**方向和 L-1 的意图相反**。
- 根因在 `request()`（`page.rs:412`）：只对 **401/403** 做了分流（`if (status === 401 || status === 403)`），**404 没有任何分支**；403 的分支是「按 query token 不对」写的，**没有区分「端点根本不在这个服务上」**。
- 附带：`page.rs:49` 的模块文档写着「页面会**明确**报「无法连接 / **404**」并给出该怎么做」—— 这句与实测不符（404 时没给「该怎么做」）。

**满足的那一半（要记清楚，别一棍子打死）**：**连不上**（网络失败）时是达标的 ——

```
q-meta => "[NETWORK]无法连接服务：fetch failed 怎么办：确认服务还在跑（端口 / 进程）；
           本页只允许同源 /api/*（CSP connect-src 'self'）——查询台与目录浏览需要控制面与
           查询服务在同一个源后面（反向代理按路径分流）。"
```

所以 L-1 ① 是「404 / 403 这两条腿没落地，只有网络失败那条腿落地」，不是整条没做。

**最小修复建议**（不动需求边界）：在 `request()` 里，对 `opts.query === true` 的 **404 与 403** 统一给一条固定文案「查询服务未接入：请把 query serve 挂到同源 /api/sparql（见文档；本机调试用 `tests/fixtures/req125/dev_proxy.ps1`）」，**并且不要**在这个场景里 clear `S.qt`、不要追加「再填一次 query token」；404 时把通用 hint 里那串管理端点对查询视图隐去。再加一条探针断言（我这次的 `qa_404_probe.js` 可直接改造成仓库内判据），防回归。

### ② `--help` 与启动横幅 —— **只做到一半**

| 位置 | 写明「不含查询端点，需 query serve + 反代」 |
| --- | --- |
| 启动横幅（`serve.rs:151–158`） | ✅ 两段都写了（工作台/零数据/免 token；查询台与目录浏览属 REQ-123 查询服务，需同源反代，本机用 `dev_proxy.ps1`） |
| `ontok console serve --help`（`cli.rs:664–724` 的 doc comment） | ❌ **没有**。清单里 11 个端点全是 `/api/*` 管理端点，**没有 `GET /`**，也没有一句「本服务不含 /api/sparql，需 query serve + 反代」 |

实测 `ontok console serve --help` 的端点段原文（节选）：

```
GET  /api/health                       免 token，只给版本
GET  /api/projects                     项目 / 版本 / 冻结状态 / 对象·关系计数
…（共 11 行，无 `GET /`，无 sparql/catalog/describe，无 query serve、无反代）
```

L-1 ② 的措辞是「`--help` **与**启动横幅」，两处都要 —— 现在只有一处。

### ③ 仓库内 `dev_proxy.ps1` + 写进文档 —— **达标**

| 判据 | 实测 |
| --- | --- |
| 在仓库 | ✓ `tests/fixtures/req125/dev_proxy.ps1`（99 行，属 feature 分支 tracked 文件） |
| 能起 | ✓ `pwsh -NoProfile -File tests/fixtures/req125/dev_proxy.ps1` → `· REQ-125 调试代理：http://127.0.0.1:8897/` |
| 分流正确 | ✓ `GET /` → 200（58664 B）；`GET /api/projects`（admin）→ 200 `secretproj`；`POST /api/sparql`（query）→ 200；`GET /api/catalog` → 200；`GET /api/describe` → 200 |
| 不吞安全头 | ✓ 三个头原样透传（CSP/nosniff/no-referrer），UTF-8 字节保真（58664 B 完全一致） |
| 写进文档 | ✓ `tests/fixtures/req125/README.md §2`；✓ `BA/runbook/ontology-e2e.md §7.3 · 工作台前端（REQ-125）与「两服务同源」`（第 253–270 行，含 `dev_proxy.ps1` 用法） |

---

## 12 · 不退化（PASS）

### 12.1 基线数字（全部本轮实测）

| 项 | 基线要求 | 实测 | 结果 |
| --- | --- | --- | --- |
| lib | 590 | **590 passed / 0 failed** | ✓ |
| doc | 4 | **4 passed** | ✓ |
| `test_req125_page` | 8 | **8 passed** | ✓ |
| REQ-107 四组 | 27 | **27**（outlet_pipeline 12 + red_lines 2 + source_enumeration 10 + zero_visible_change 3） | ✓ |
| REQ-107 其余 5 个文件 | — | 14 passed（guard 4 / pipeline_bytes 1 / scope 4 / static 3 / track 2） | ✓ |
| REQ-124 | 14 | **14 passed**（`test_req124_console`）；`test_req124_live` 另 2 passed | ✓ |

### 12.2 禁改面

| 判据 | 实测 |
| --- | --- |
| `tools.rs` / `guard()` / `property_filter.rs` | `git diff --name-only develop..HEAD -- src/tools.rs src/query/property_filter.rs src/guard.rs` = **空** ✓ |
| `serve`（`src/server`）/ `query` 路由 | `src/server/**`、`src/query/**` diff = **空** ✓（`src/console/serve.rs` 只改了横幅文案，无路由） |
| `Cargo.toml` / `Cargo.lock` | diff = **空**（零新增依赖）✓ |
| 四红线 | `test_req107_red_lines`（2）绿 ✓ |
| 删测试 / `#[ignore]` / 放宽断言 | diff 里只有「56→57」口径上调与新增，无删改断言 ✓ |
| 出口登记 | `I22` 已登记（`src/domain/pipeline.rs`），`OUTLETS` **56 → 57**，内部轨 **26 → 27**，`registry_is_the_declared_baseline` 与 `test_req107_outlet_pipeline` 逐项相等 ✓ |

### 12.3 既有红灯（自己复现 + 归因）

全仓 `cargo test --offline --no-fail-fast`（lib + doc + bin + 88 个 `tests/*.rs` 目标）：**4 个目标红**，且全部在**基线 `eaa7c96` 上同样红**（我在独立 `qa-baseline` worktree 里逐个复跑，4/4 复现，失败断言逐字相同）：

| 目标 | 失败 | 归因 |
| --- | --- | --- |
| `drop_rate` | `real_schema_drop_rate_report` | 读不到脱敏 `schema.json`，测试**故意**失败（"读不到输入时它必须失败 —— 静默跳过等于没有门禁"）→ 环境缺输入，非代码 |
| `test_fk_naming` | `test_real_schema_customer_id_has_exactly_one_display_name` | 同上：`/home/myg133/…/schema.json` 不存在（另一台机器的路径），需 `ONTOK_SCHEMA_JSON` |
| `test_agent_orchestration` | `test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | 基线即红（★ items 与 legacy 提取不一致），REQ-125 未触碰该链路 |
| `test_retained_names_req084` | 2 条 | 基线即红：`src/domain/concept.rs` 9 处已登记表名 vs 登记表 5 处（登记漂移），本分支未改 `concept.rs` |

⇒ **REQ-125 未引入任何新红**；4 个红点与本次变更面（`src/console/*` + `pipeline.rs` 口径）无交集。

---

## 13 · 阻断项 / 非阻断项

### 阻断（1 项，必须改）

| # | 项 | 证据 | 修法 |
| --- | --- | --- | --- |
| B-1 | **L-1 ①**：查询台 / 目录浏览在 **404**（只填管理口令）和 **403**（填了 query token）两条路上都没有「查询服务未接入：请把 query serve 挂到同源 /api/sparql（见文档）」；404 给的是「没有这个路径 + 管理端点清单」（方向误导），403 会**清空用户刚填的 query token** 并让「再填一次」（L-1 拓扑下走不出去）；`page.rs:49` 的「404 会给出该怎么做」与实测不符 | 真实 404 / 403 响应体 + 假 DOM 渲染（§11①）；`qa_404_probe.js` 4 条红（404 文案、403 文案、403 清 token 循环、目录浏览 404） | `request()` 对 `opts.query` 的 404/403 统一给固定文案；该场景不要 clear `S.qt`；探针补断言 |

### 非阻断（建议顺手处理，不影响合并判断）

| # | 项 | 说明 |
| --- | --- | --- |
| N-1 | **L-1 ②** 的 `--help` 一半没写 | 横幅已写；`cli.rs:664–724` 的 doc comment 补 3 行即可（`GET /` 页面 + 「本服务不含 /api/sparql」+ 「需 query serve + 反代」）。和 B-1 一起改成本最低 |
| N-2 | 查询台 / 目录浏览视图里的**静态说明**没有一句拓扑提示 | 点开就到、不点「执行」时看不到任何「要反代」的字；建议在查询面板的 `.note` 里加一句（与 B-1 同源） |
| N-3 | `page.rs` 全文件仍有 `innerHTML` 等字样（6/2/2/2/2） | 全是注释在解释「本页没有」，不是调用点；若要求「`page.rs` 字面 0 处」，需要把这些注释改成不含字面量的说法（可选） |

### 明确不算问题（已核过，不要重复问）

- 壳里没有 `GET /` 的 token 校验豁免漏洞：`/` 与 `/api/health` 是**精确路径**豁免，其余（含拼错路径）仍过门（`http.rs:188`）；`/api/projects` 无 token 仍 401、query token 仍 403（测试 + 实测）。
- 没有为前端新开端点：`build_router` 相对 develop **只多 `.route(PAGE_PATH, get(index))`** 一行，`/api/*` 一个没加；页面所有请求都是相对 `api/*`（12 处调用点全部如此），只有 1 个 `fetch(` 调用点。
- CSP 的 `'unsafe-inline'` 不是「图省事」：注释已按 L-2 写明代价与退出条件。
- 大结果集：评审 40/页、审计 ≤1000、查询 ≤200 行并写明「只显示前 N / 共 M」，不会一次渲染上万行。

---

## 附 · 复跑命令（本机，全部在 `qa-REQ-125` 里）

```powershell
# 0) 环境
$env:ONTOK_DATABASE_URL       = "postgresql://postgres:postgres@172.17.0.87:5432/anspire_real"
$env:ONTOK_SOURCE_DATABASE_URL= "postgresql://postgres:postgres123@172.16.120.20:30432/anspire_open_uat"
$env:ONTOK_TEST_DATABASE_URL  = $env:ONTOK_DATABASE_URL

# 1) 壳零数据 + 响应头（起服务后）
ontok console serve --bind 127.0.0.1:8899 --tokens admin-tokens.json --project secretproj `
  --schema D:\MyCodes\AI\an_onto_demo\code\out\schema.json `
  --exclusions qa-decls\exclusions.json --policy qa-decls\policy.json --tenancy qa-decls\tenancy.json
curl.exe -sI http://127.0.0.1:8899/            # 三个安全头
curl.exe -s  http://127.0.0.1:8899/ | Select-String "secretproj|绝密对象|第 7 版"   # 0 命中

# 2) 交互判据（假 DOM，一条命令）
node tests\fixtures\req125\page_probe.js                       # 内置样本：32 条全过
node tests\fixtures\req125\page_probe.js receipt_deductions.json --dump   # 看页面会显示的那几行

# 3) 两份真实回执（需 query serve 在 8898）
ontok query serve --bind 127.0.0.1:8898 --tokens qa-query-tokens.json `
  --project anspire_r2rml_excl --version 2 --schema <schema> --policy qa-decls\policy.json

# 4) 我补的三个探针（只在 QA worktree，未进仓库）
node qa_404_probe.js       # L-1 ①（当前 4 条红：404 文案 / 403 文案 / 403 清 token 循环 / 目录浏览 404）
node qa_confirm_probe.js   # AC-006 三处确认 + 取消 0 fetch
node qa_error_probe.js     # AC-007 真 400 保留输入 / AC-009 401 清口令

# 5) 测试
cargo test --lib --doc --offline
cargo test --test test_req125_page --offline
cargo test --test test_req107_outlet_pipeline --test test_req107_red_lines `
           --test test_req107_source_enumeration --test test_req107_zero_visible_change --offline
cargo test --test test_req124_console --offline
```

**证据文件（本机复跑产物，均在 `qa-REQ-125/`，未进 Git）**：
`served_root.html`、`receipt_{deductions,query_serve,trunc}.json`、`real400.json`、`full_suite.log`、
`probe_*.log`、`mut1.log`、`mut2.log`、`qa_404_probe.js`、`qa_confirm_probe.js`、`qa_error_probe.js`、`qa_url_probe.js`。
`qa-REQ-125` 是 detached 临时 worktree（无分支、未 push），审完即可 `git worktree remove --force` 清掉；`qa-baseline`（基线归因用）**已清理**。
本轮在真库 `anspire_real` 建了 `secretproj`（v1~v7，0 对象）用于 AC-001 现场，未改任何既有项目。
