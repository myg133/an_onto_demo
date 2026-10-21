# QA 复审 · REQ-114 · 页面图交互升级（第二轮）

> **被测**：worktree `feature-REQ-114` / 分支 `feature/REQ-114` / HEAD **`182fd6d`**（`e6f0947` + `5d9b483` + `182fd6d`）
> **基线**：`develop` `e9755e8` · **工作区干净** · **未改 feature 分支、未 push**
> **审核方式**：源码级 + 纯函数级（Node 探针）+ 真 HTTP 级 + **独立 worktree 变异（自己跑，不采信 Dev 退出码）**
> **环境**：Windows · node v22.22.2 · rustc/cargo 1.96.0 · 真 PG 16 `172.17.0.87:5432/anspire_real`
> **报告日期**：2026-10-22 · 上一轮：`qa-review.md`（FAIL，B-1 探针假绿）

---

## 1 · 结论

# ✅ PASS —— 可合并

**上一轮唯一的阻断项 B-1（布局探针假绿）已真修好**：我自己复跑的六种变异（基线 + M1a/M1c/M1d/M2/M3）**全部符合期望**，
基线 exit 0（两个场景都跑了），五种变异**全部 exit≠0**；每次还原后 `page.rs` 的 sha256 **逐字节一致**。

同时核过的其余各项：无斥力场景**是真的**（改的是内核作用域里的 `REP` 本身，有代码 + 行为双证）；
剥注释**没有把代码剥掉**且反空转断言**确实会咬**；静态计数**恰好 2 处**（不是 ≥1）；
`pass()` 条件假绿**已消失**；边数 52 **有断言**；
golden **只有该变的那一份变了**（`INDEX_HTML` == `H1` == 真 HTTP 响应体）；
文档 D1–D4 **已全部落到 REQ-114**；**无过度声称**；不退化数字**一条不少**。

**本轮没有发现新的阻断项。** 文末列了 5 条非阻断的残留/建议（含一条：探针仍抓不住「页面 `loop()` 不调 `physicsStep`」这类接线缺失，属本轮 AC 之外的残留覆盖缺口）。

---

## 2 · P0 六种变异（我自己的退出码，未采信 Dev）

**环境**：新建独立 detached worktree `.review-workspace/req114/qa2-mut`（HEAD `182fd6d`），
每次变异只改 `src/server/page.rs`，跑完 `git checkout -- src/server/page.rs` 还原并核对 sha256。
探针从该 worktree 根跑：`node tests/fixtures/req114/layout_probe.js`。**做完已删除该 worktree**。

基线 `src/server/page.rs` sha256 =
`9E5ABADF746CE4B05395D842630CE50C3F7EF45DAFBBEE5829BB48B997FE8DD0`（39887 字节；LF）
探针 sha256 = `C3F52285C1E2E5ACD74C40D4207D3C7E15F873F6FE5E37DFAFB043159692D74E`

| # | 变异 | 手法 | 期望 | **实际 exit** | 关键输出（原文摘） |
| --- | --- | --- | --- | --- | --- |
| **基线** | 无 | 不动 | **0** | **0** ✅ | `内核段 = 3925 字节` / `剥注释后内核 = 2255 字节`；场景 1：168 帧 / 余量 **+15.79** / 529×528；场景 2：168 帧 / 余量 **+5.20** / 288×305；`ALL PASS（…）` |
| **M1a** | `collide(ns, GAP);` 两行删**一行** | exit≠0 | **1** ✅ | `FAIL 静态：剥注释后 collide(ns, GAP); 恰好 2 处  实际 1 处`；`FAIL 场景 2（REP=0）… 最紧余量 -30.94px`（2 项 FAIL） |
| **M1c** | 两行改成 `//` 注释 | exit≠0 | **1** ✅ | `FAIL 静态… 实际 0 处`；`FAIL 场景 2 … 余量 -32.00px`；`FAIL 场景 2：节点确实被推开了（不是全缩在原点）  0 × 0`（3 项 FAIL） |
| **M1d** | 两行包进 `if (false) { … }` | exit≠0 | **1** ✅ | 静态层仍 `实际 2 处`（死代码不是注释，**符合预期**）⇒ 由行为层抓住：`FAIL 场景 2 … -32.00px` + `FAIL … 0 × 0`（2 项 FAIL） |
| **M2** | `var min = a.r + b.r + gap;` → `var min = 0;` | exit≠0 | **1** ✅ | `FAIL 单元：重叠的一对被推到「半径和 + GAP」 余量 -18.00 ≥ GAP 6`；`FAIL 单元：让对面的节点让开整个间距 余量 -17.00`；`FAIL 场景 2 … -32.00px`（4 项 FAIL） |
| **M3** | 改 END 标记文字（`布局内核结束`→`布局内核收尾`） | exit≠0，且必须 `die()` | **1** ✅ | `FAIL 在 page.rs 里找不到布局内核的两条标记（BEGIN/END）…`；`探针无法完成 ⇒ exit 1（⛔ 不许把这种情形当成通过）` |

**基线确认两个场景都真的跑了**（不是只跑一个）：输出里同时有
「场景 1 · 真实参数（REP=400，页面默认）」与「场景 2 · 无斥力（REP=0 ⇒ 只能靠 collide 分开）」两段完整数字，
末行 `ALL PASS（场景 1：168 帧 / 余量 15.79px；场景 2：168 帧 / 余量 5.20px）`。

### 2.1 额外一条（验「恰好 2 处，不是 ≥1」）

| 变异 | 手法 | 实际 exit | 关键输出 |
| --- | --- | --- | --- |
| **M-extra** | 两行之后**再写第三处** `collide(ns, GAP);` | **1** ✅ | `FAIL 静态：… 恰好 2 处  实际 3 处`（+ 场景 2 余量 -23.04，因为三遍碰撞把布局挤成 785×1570） |

⇒ 静态判据对「少一处 / 多一处 / 注释掉」三个方向都咬得住。

---

## 3 · 还原 sha256（每次，逐字节）

（`git checkout -- src/server/page.rs` 后实测）

```text
BASE_SHA            = 9E5ABADF746CE4B05395D842630CE50C3F7EF45DAFBBEE5829BB48B997FE8DD0
post-BASELINE sha   = 9E5ABADF…8DD0  identical=True
M1a RESTORE sha     = 9E5ABADF…8DD0  identical=True   (变异态 02AD94AE…3DB0)
M1c RESTORE sha     = 9E5ABADF…8DD0  identical=True   (变异态 30A62A1B…16C5)
M1d RESTORE sha     = 9E5ABADF…8DD0  identical=True   (变异态 27B62224…E72CB)
M2  RESTORE sha     = 9E5ABADF…8DD0  identical=True   (变异态 5ACC5569…97DFA9)
M3  RESTORE sha     = 9E5ABADF…8DD0  identical=True   (变异态 51CC8B52…5B1372)
M-extra RESTORE sha = 9E5ABADF…8DD0  identical=True   (变异态 0573A9E2…4C82CE)
FINAL sha           = 9E5ABADF…8DD0  identical=True
git status --short  = 空
```

> 与上一轮的 `f3c1b9ec…` 不同是**预期内的**：本轮 `182fd6d` 改了内核段里的一行注释（C3 修探针路径），
> `page.rs` 因此比 `5d9b483` 多 1 行（839→840 行，39767→39887 字节）。全轮 **6 次变异还原**（外加基线后、收尾各一次核对）**全部回到同一个 `9E5ABADF…`**，没有漂移。

**feature 分支未动**：`feature-REQ-114` 的 `src/server/page.rs` sha 仍是 `9E5ABADF…`，
`git status` 干净，HEAD 仍 `182fd6d`；未 push。独立 worktree 用完已 `git worktree remove` + `prune`。

---

## 4 · 「无斥力场景」是否真的成立 —— 成立（代码 + 行为双证）

### 4.1 读代码：`setRep(0)` 改的是内核作用域里的 `REP` 本身

| 事实 | 位置 |
| --- | --- |
| `REP` **声明在**两条标记之间 | `src/server/page.rs:230` `var REP = 400;`，在 `BEGIN`(`:222`) 与 `END`(`:307`) 之间 |
| `physicsStep` **直接读同一个变量** | `:288` `f = REP / d2;` —— 无同名参数、无局部遮蔽；全文件 `var/let/const REP` 只有 `:230` 一处（已用 grep 复核） |
| 探针的注入点是**同一作用域内的赋值** | `layout_probe.js:193-194`：`kernel + "exports.setRep = function (v) { REP = v; };"` 一起塞进**同一个** `new Function` 体 ⇒ 闭包里的 `REP` 就是那个 `var REP`，**不是复制一份参数** |
| 场景 2 用的仍是页面那条 `physicsStep` | 探针只调 `K.physicsStep(...)`（`layout_probe.js:284`，在 `run()` 里；两个场景共用），没有另写一套物理 |

### 4.2 行为证：`setRep(0)` 确实传到了引擎里

用同一段内核（从 `page.rs` 原样抽）在 Node 里：

```text
getRep()  setRep(400) 后 = 400
getRep()  setRep(0)   后 = 0
两节点（A 在原点、B 在 (120,0)）跑 1 帧 alpha=0.5：
  REP=400：A = (-1.3667, 0)   ← A 在原点，向心力为 0，这点位移只可能来自斥力
  REP=0  ：A = ( 0.0000, 0)   ← 斥力真的没了
```

再加上变异里的**失败签名**：M1c/M1d（collide 完全没执行）时场景 2 的包围盒是 **`0 × 0`** ——
83 个节点一帧都没动。若 `REP` 没被置 0，斥力会把它们撑开（场景 1 就是 529×528），
不可能出现「全叠在原点」。⇒ **场景 2 的「不重叠只可能来自 collide」这个前提成立。**

---

## 5 · 剥注释：有没有剥过头 / 反空转断言是否有效

### 5.1 剥注释没有剥掉代码（独立复算，不采信探针自报）

我自己写了一个**独立的**剥注释器（先删 `/* */`、再逐行删 `//`）对同一段内核复算：

```text
kernel bytes = 3925
非注释行里含引号的数量 = 0      ← 内核里没有字符串字面量，简单剥法也安全
stripped bytes = 2255   ratio = 0.575
collide(ns, GAP); count = 2
stripped 里残留 '//' = 0，残留 '/*' = 0
代码 token 全在：function radiusOf / collide / springs / physicsStep、
  var min = a.r + b.r + gap; / f = REP / d2; / var REP = 400; / var GAP = 6;
  var SIM_FRAMES = 300; / var ALPHA_MIN = 0.006; / a.pin ? 0
```

与探针自报的 `2255` / `2 处` **逐项吻合**。另核了内核里**没有 JS 正则字面量**（0 处），
所以剥注释器「把 `/` 当注释起点」的经典误判形态在这里不成立。

### 5.2 反空转断言：**存在，而且真的会咬**

探针 `layout_probe.js:176-178`：

```js
expect(Buffer.byteLength(stripped, "utf8") > Buffer.byteLength(kernel, "utf8") / 2,
  "剥注释没有把代码剥没（自检）", …);
```

我做了两个 meta 变异（改探针副本，不动 feature 分支）：

| meta 变异 | 实际 exit | 关键输出 |
| --- | --- | --- |
| 剥注释器返回空串 | **1** ✅ | `FAIL 剥注释没有把代码剥没（自检）  0 / 3925 字节` + `FAIL 静态… 实际 0 处` |
| 剥注释结果被截断到 1000 字节（模拟过度剥离） | **1** ✅ | `FAIL 剥注释没有把代码剥没（自检）  1000 / 3925 字节` + `FAIL 静态… 实际 0 处` |

**另一个方向也验了**（这条更关键）：把剥注释器改成**完全不剥**（返回原文），再叠加 M1c（两行注释掉）——
静态层果然失效（原文数到 2 处、PASS），但**行为层单独兜住了**：`FAIL 场景 2 … -32.00px`，exit 1。
⇒ 两层判据是**冗余**的，不是互相顶替。

### 5.3 一点保留（非阻断）

反空转阈值是「> 原文一半」，而实测比值 **57.5%**，余量只有 7.5 个百分点。
现在方向是安全的（只会「误红」不会「误绿」），但如果以后内核里注释继续变多，这条自检可能无端变红。
建议改成结构断言（例如「剥完必须仍含 4 个 `function` 名 + 2 处 collide」），比字节比例更稳。

---

## 6 · 静态计数「恰好 2 处」/ `pass()` 假绿 / 边数断言

| 项 | 实测 | 判定 |
| --- | --- | --- |
| 静态计数 | 剥注释后 **2 处**（我自己复算也是 2）；M1a→1、M1c→0、M-extra→3 全部红 | ✅ 是 `=== 2`，不是 `>= 1` |
| 常量绑定 | `WANT_COLLIDE_CALLS = 2`（`layout_probe.js:81`），并在文件头写明「改成 3 遍要同时改本文件」 | ✅ 有意绑定，已声明 |
| `pass()` 条件假绿 | 全文件 `pass(` 只有 3 处：定义（`:85`）、`expect` 内部（`:95`）；**没有任何 `pass(name, 条件/三元)` 形态**；断言一律走 `expect(cond, …)`（共 18 处） | ✅ 已消失 |
| 边数断言 | `expect(edges1.length === 52, …)`（`:316`）+ `expect(edges2.length === 52, …)`（`:367`），`N_EDGES = 52`（`:79`） | ✅ 存在 |
| 起点断言 | 改成 `expect(nodes1.every(x,y,vx,vy 全 0), …)`（`:313-315`） | ✅ 不再是「打印一句话」 |

---

## 7 · golden 与其余 12 份

### 7.1 三处字节一致（自己算的）

| 对象 | 字节 | sha256 |
| --- | --- | --- |
| `page.rs` 的 `INDEX_HTML` 常量本体（`r##"…"##` 之间） | **32396** | `d8bd4581c264a483815c0bf67636966da190b3799e8abbaa79c6f5a2a849c5c0` |
| `tests/fixtures/req107/golden/H1_index.html` | 32396 | **同上** |
| 真 HTTP `GET /`（真二进制 + 真 PG `anspire_real` v2） | 32396 | **同上** |

逐字节比对：**byte-by-byte equal = True**（不是「看起来一样」）。

### 7.2 `SHA256SUMS` 与其余 12 份

- 13 条记录逐条复算：**13/13 OK，0 mismatch**。
- `SHA256SUMS` 的 diff **只有 H1 一行**（`96eb5f…` → `d8bd4581…`），其余 12 行未动。
- `git diff --name-status develop..HEAD -- tests/fixtures/req107/golden/` 只有 `H1_index.html` / `PROVENANCE.txt` / `SHA256SUMS`；
  `C1_html.txt`、`C3_json.txt`、`C4_overview.txt`、`C5_overview_json.txt`、`H2_graph.json`、`H3_node_1.json`、
  `H3_node_2.json`、`M1.json`、`M2.json`、`M3.json`、`M4.json`、`M5.json` **共 12 份逐字节未动** ✅

### 7.3 `PROVENANCE.txt`

- **不再把旧 H1 说成「b714f48 那一份」**：`:30-33` 明确改成「REQ-110 收口后、develop 上那一份静态壳」，
  并写明「b714f48 那个基线 worktree 里**没有 golden 文件**」。我实测 `git ls-tree b714f48 tests/fixtures/req107/golden/`
  **为空** ⇒ 这个更正属实。
- **三次字节变化**：`:39-43` 以「①主体 / ②双击语义+探针入库 / ③探针行为级+注释路径」记录**三处变化的原因**，
  并明说「本文件只保留最终态」。我逐提交实测，三次变化**确有其事**：

```text
develop  96eb5fae2cb4…a5cad902  12479 字节
e6f0947  40d178699cef…5e1df6a   29943 字节   ← ①
5d9b483  1f86eae66e9a…31792e    32276 字节   ← ②
182fd6d  d8bd4581c264…849c5c0   32396 字节   ← ③（最终态）
```

- **一处偏薄（非阻断）**：文件里只写了最终 sha，中间两次（`40d178…` / `1f86ea…`）只有文字没有 sha。
  「三处变化」这个**事实**是如实的，但想从这份文件单独复算中间态就做不到（要查 git 历史）。
  若要让「三次字节变化」可审计，建议把这两行的 sha 也补上。

---

## 8 · 文档 D1–D4

| # | 项 | 实测 | 判定 |
| --- | --- | --- | --- |
| **D1** | `PROVENANCE` 旧 H1 来源标注 | 已改，见 §7.3；旧 H1 的 hash/大小与 `develop` 实测一致 | ✅ |
| **D2** | 「已知限制写进交付物」这句不成立 | 已改成实际落点：`page.rs` 模块文档「已知限制」+ 探针文件头「边界」段；我核对这**两处确实都有** | ✅ |
| **D3** | `.feature/status.md` / `verification-report.md` 仍是 REQ-112 | 两份**首屏都是 REQ-114**（标题、被测、基线 `e9755e8`、本轮范围）；REQ-112/111 原文保留在下方并明确标「# 历史：REQ-112」 | ✅ |
| **D4** | `.feature/manifest.json` 仍是 REQ-107 | 整份已改：`req_id=REQ-114`、`branch=feature/REQ-114`、`base_commit=e9755e8`、`acceptance=BA/demands/REQ-114/acceptance.md` | ✅ |

（`REQ-112` 字样在这两份文件里仍出现，但全部位于显式标注的「历史」段，不构成「文档没更新」。）

---

## 9 · 过度声称

全文扫 `已验证 / 流畅 / 手测 / 浏览器验证 / 彻底 / 保证 / 全部通过 / 已修复 / 完全`：

- **没有任何一处**写「已验证交互流畅」这类结论；
- 「已验证」「流畅」的 5+3 处命中**全部是禁令本身**（「⛔ 不许写『已验证交互流畅』」）；
- 「手测」6 处全部是「**未做**真浏览器手测」；
- `verification-report.md:17` 的「五种变异实测全部 exit 1」我逐条复跑 —— **属实**，数字（`-30.94 / -32.00 / -18.00 / -17.00`、
  余量 `15.79 / 5.20`、帧数 `168`、包围盒 `529×528 / 288×305`）与我自己跑的**逐项相同**，没有编造。
- 唯一「像结论」的一句是 `verification-report.md:5-6`「产品交互逻辑本轮没动，唯一碰到的产品文件是一行注释」——
  `git diff 5d9b483..182fd6d -- src/server/page.rs` 实测**只有那段注释的 3 删 4 增**，属实。

⇒ **无过度声称** ✅

---

## 10 · 不退化（自己跑的）

| 项 | 实测 |
| --- | --- |
| `cargo test --lib` | **245 passed / 0 failed**（exit 0） |
| `cargo test --doc` | **4 passed / 0 failed**（exit 0） |
| `test_req107_pipeline_bytes` | 1 passed / 0 failed |
| `test_req107_zero_visible_change` | 3 passed / 0 failed |
| `test_graph_server` | 9 passed / 0 failed |
| `test_req110_ttl` | 15 passed / 0 failed |
| `test_req107_static_texts` | 3 passed / 0 failed |
| `test_req107_red_lines` | 2 passed / 0 failed（**clean tree 是绿的**，与首轮结论一致） |

> Windows 绕行：临时移开 `src/bin/ontok-gate-preflight.rs`（unix-only）后跑，`finally` 挪回：
> `sha256_before = sha256_after = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`，
> `Test-Path=True`，`bak=False`，`git status --short` 空。

**禁改面 diff（`develop..HEAD`）全部 0 行**：

```text
src/server/http.rs · src/mcp/tools.rs · src/domain/property_filter.rs · src/review_ui/page.rs
Cargo.toml · Cargo.lock · tests/drop_rate.rs · tests/test_fk_naming.rs
src/domain/view.rs · src/domain/review_view.rs
```

无**新增** `#[ignore]`（`develop..HEAD` 对 `tests/test_mcp_server.rs` 的 diff 为 0 行；该文件里那条既有的
`#[ignore = "缺口清单，不是可运行断言"]` 在 `develop` 上就存在）、无删除测试（`--diff-filter=D` 无输出）、无新增依赖。
`develop..HEAD` 共 8 个文件：`page.rs` + 3 份 golden + 探针（新增）+ 3 份 `.feature` 文档。

**零依赖 / 零数据面（真 HTTP 复测）**：`GET /` 200、32396 字节；页面内 `<script src=`/`<link href=`/`https://`/`http://`/`@import`/`integrity=`
各 **0** 处；项目名与 9 个真实对象名 **0 命中**；`/api/{scope,graph,node,ontology.ttl}` 无 token **全 401**；
带 token 的 `/api/graph` **83 nodes / 52 edges**。

---

## 11 · 仍需要修的地方

**阻断项：无。**

非阻断（按重要性排序，都不挡合并）：

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| **R1** | 探针仍抓不住「`loop()` 不调 `physicsStep`」 | 我实测：把 `page.rs:340` 的 `physicsStep(NS, ES, PS, …)` 注释掉，探针**仍 exit 0**（`ALL PASS`）。原因是行为级场景直接调 `physicsStep`，而 `loop()` 在两条标记**之外**。这不违反本轮 AC-002 的判据（它要求的是「碰撞调用被删/注释/`if(false)` 必须红」），但属同类接线的**残留覆盖缺口**。建议下轮把 `loop()` 纳入标记，或加一条静态判据「`loop` 函数体内 `physicsStep(` 恰好 1 处」 |
| **R2** | `verification-report.md` §2 的 `page.rs` 行号是 `e6f0947` 的，**整体过期** | 例：`collide` 写 `230`（HEAD 实际 `:246`）、两处调用写 `288-289`（实际 `:304-305`）、节点 `pointerdown` 写 `505`（实际 `:521`）、背景 `pointerdown` 写 `583`（实际 `:646`）。原因是 `5d9b483` 加了双击/点按代码（内核前 +15 行、背景处理器处 +62 行）、`182fd6d` 再加 1 行。技术结论不受影响，但报告是交付物，建议更新到 HEAD |
| **R3** | `PROVENANCE` 只记最终 sha | 见 §7.3：三次变化有文字无中间 sha（`40d178…` / `1f86ea…`）。建议补上，让「三次字节变化」可单独审计 |
| **R4** | 反空转阈值余量偏小 | 实测 57.5% vs 阈值 50%（见 §5.3）。方向安全（只会误红），建议改成结构断言更稳 |
| **R5** | 触屏无 pinch | 仍只有 `wheel` 缩放；AC-004 未要求，但「已知限制（三条）」里还没提这一条（上轮 N-7 未处置）。建议补一句 |

---

## 12 · 复现命令（我实际执行过的）

```powershell
# 0) 基线探针（feature worktree 根）
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-114
node tests\fixtures\req114\layout_probe.js      # exit 0；168 帧 / 15.79 / 5.20

# 1) 独立 worktree 变异（不碰 feature 分支）
cd D:\MyCodes\AI\an_onto_demo
git worktree add --detach .review-workspace\req114\qa2-mut 182fd6d
#   在 qa2-mut 里改 src/server/page.rs：
#     M1a 删一行 collide(ns, GAP);  / M1c 两行改 //  / M1d 包进 if (false) {}
#     M2  var min = a.r + b.r + gap; -> 0  / M3 改 END 标记 / M-extra 多写一处
#   每次：node tests\fixtures\req114\layout_probe.js; git checkout -- src/server/page.rs
#   逐次核对 (Get-FileHash src\server\page.rs -Algorithm SHA256).Hash  => 9E5ABADF…8DD0

# 2) 不退化（Windows 先临时移开 unix-only bin，finally 挪回 + sha256）
$env:ONTOK_TEST_DATABASE_URL = 'postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo'
cargo test --lib ; cargo test --doc
cargo test --test test_req107_pipeline_bytes ; cargo test --test test_req107_zero_visible_change
cargo test --test test_graph_server ; cargo test --test test_req110_ttl
cargo test --test test_req107_static_texts ; cargo test --test test_req107_red_lines

# 3) golden 三处一致
#    page.rs 的 r##"…"## 之间 32396 字节 / d8bd4581…
#    Get-FileHash tests\fixtures\req107\golden\H1_index.html  => 同上
$env:ONTOK_DATABASE_URL = 'postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
$env:ONTOK_VIEW_TOKEN = 'qa114-review2-token'
.\target\debug\ontok.exe serve --project anspire_real --version 2 --bind 127.0.0.1:8799
curl.exe -s -o page.html -w "%{http_code} %{size_download}" http://127.0.0.1:8799/   # 200 32396
```

---

## 13 · 最终判词

- **P0（探针假绿）已真修好**：五种变异全红、两个场景都跑、静态计数恰好 2、`pass()` 假绿消失、边数有断言。
- **无斥力场景是真的**（不是复制参数），并且**行为层独立于静态层**（不剥注释 + M1c 仍能红）。
- **golden 只该变的那一份变了**；`INDEX_HTML` == `H1` == 真 HTTP，13/13 校验通过。
- **文档 D1–D4 已落 REQ-114**；**没有过度声称**；**不退化数字一条不少**；**禁改面 0 diff**。
- **本轮无新增阻断项 ⇒ PASS，可合并。** 残留项见 §11（R1 是最值得排期的一条）。
