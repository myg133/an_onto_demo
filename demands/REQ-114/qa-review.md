# QA 审核 · REQ-114 · 页面图交互升级

> **被测**：worktree `feature-REQ-114` / 分支 `feature/REQ-114` / 提交 `e6f0947` + `5d9b483`（HEAD `5d9b483`）
> **审核环境**：Windows · Node v22.22.2 · cargo/rustc 1.96.0 · 无 headless 浏览器 · 真 PG 16（`172.17.0.87`）
> **审核方式**：源码级 + 纯函数级（Node 探针）+ 真 HTTP 级 + 独立 worktree 变异；**未做真浏览器手测**
> **报告日期**：2026-10-22

---

## 1 · 结论

**FAIL —— 阻断 1 项：P0 的布局探针对 M1 没有牙，且存在可被「注释掉调用」绕过的假绿。**

先划清责任边界，避免误读：

| 层 | 结论 |
| --- | --- |
| **产品代码**（`src/server/page.rs`） | AC-001/003/004/005/006 源码级成立；AC-002 的数据级在**实际代码**上成立（83 节点同点起跑，168 帧，最紧余量 **+15.79px**，NaN 0）；AC-007/008 真 HTTP 成立 |
| **交付的验证物**（`tests/fixtures/req114/layout_probe.js`） | **不成立**：删掉末尾一行 `collide(ns, GAP);` 仍 exit 0；把两处调用**注释掉**（碰撞完全失效）仍 `ALL PASS`；`if (false)` 包住两处调用同样 `ALL PASS` ⇒ 它声称要抓的「写了没调用 = 碰撞形同虚设」抓不住 |
| **golden / 数据面 / 不退化** | 成立（详见 §5–§8） |
| **如实登记** | Dev **没有**写「已验证交互流畅」；`page.rs` 模块文档、探针头、`PROVENANCE.txt` 都明确写了「未做真浏览器手测」和三处已知限制。**这一点是合格的** |

⇒ 本轮判 FAIL **不是因为「没在浏览器里看过」**，而是因为 P0 指定的变异 M1 没让探针变红，且存在真实的假绿路径。产品代码本身 AC-002 是成立的；阻断点在**验证物**，修好探针后可复审。

---

## 2 · 探针有牙：M1 / M2 / M3 与还原证据

### 2.1 基线（实际代码）

```text
node tests/fixtures/req114/layout_probe.js
→ exit 0
内核段 = 3805 字节（从 src/server/page.rs 原样抽取）
节点 83 / 边 52
实际推进 168 帧（上限 300），停止原因 = alpha 掉到 ALPHA_MIN 以下（收敛）
最紧一对 #6/#47：中心距 42.79 / 半径和 26.99 / 余量 +15.79px
含 NaN 节点 0；包围盒 529 × 528
ALL PASS
```

`src/server/page.rs` 基线 sha256：
`f3c1b9ec43d93063bacb703593ad0628fce2c1a10f08be61c3ac299b3a1358bf`
探针 sha256：`5e257cdf20c92b2cf600430a531f7a994389bb561fe224b7f620418a4d86eedf`

所有变异都在**独立 detached worktree** `.review-workspace/req114/qa-mut`（HEAD `5d9b483`）里做；
每次变异后 `git checkout -- src/server/page.rs`，逐字节还原。

### 2.2 变异结果（全部实测）

| # | 变异 | 探针 | 关键输出 | 判定 |
| --- | --- | --- | --- | --- |
| **基线** | 无 | **exit 0** | 168 帧 / 余量 +15.79 / NaN 0 | 实际代码绿 |
| **M1a** | 删掉 `physicsStep` 末尾**一行** `collide(ns, GAP);`（原有两行，剩一行） | **exit 0** | 静态守卫 `命中 1 处`；168 帧 / 余量 +15.69 | ⛔ 不满足 P0「必须 exit≠0」 |
| **M1b** | 删掉**两行** `collide(ns, GAP);` | **exit 1** | `FAIL 内核里 collide 没被接进物理步（命中 0 处）`；但数据断言仍 `余量 12.55`，仅 1 项 FAIL（文本计数） | 只靠字符串计数红 |
| **M1c** | 两行改成 `// collide(ns, GAP);`（**碰撞完全失效**） | **exit 0** | 静态守卫 `命中 2 处`；`ALL PASS（168 帧，最紧余量 12.55px）` | ⛔ **假绿**：零调用却全绿 |
| **M1d** | 两行包进 `if (false) { ... }`（死代码，**碰撞完全失效**） | **exit 0** | 静态守卫 `命中 2 处`；`ALL PASS（12.55px）` | ⛔ **假绿** |
| **M2** | `var min = a.r + b.r + gap;` → `var min = 0;` | **exit 1** | `FAIL collide：重叠的一对被推到「半径和 + GAP」余量 -18.00 ≥ GAP 6`；`FAIL collide：让对面的节点让开整个间距 余量 -17.00`；2 项 FAIL | ✅ 有牙（但由 **collide 单元级**抓住，不是 83 节点数据级） |
| **M3a** | 把整段布局内核（含两条标记）删掉 | **exit 1** | `FAIL 在 page.rs 里找不到布局内核的两条标记（BEGIN/END）… exit 1` | ✅ 没有「抽不到就 return 0」 |
| **M3b** | 只把 END 标记改名 | **exit 1** | 与 M3a 同一句 `FAIL`，未静默通过 | ✅ |

### 2.3 从变异结果能得出的准确结论

1. **M2 / M3 有牙**：改碰撞数学（`min=0`）会红；删内核/改标记会红，且 `die()` 明确 `exit 1`，没有「抽不到就返回 0」。
2. **M1 单行删除没牙**：代码刻意「碰撞跑两遍」（`page.rs:276-277,303-304`），探针的静态守卫只要求 `collisions >= 1`（`layout_probe.js:134-136`）。删掉最后一行后仍命中 1 处，探针 `ALL PASS`。按任务 P0 的字面判据，这是**未通过**。
3. **更严重的是 M1c/M1d**：探针用**裸正则**数原始文本里的 `collide(ns, GAP);`，**不剥注释、不区分死代码**。因此：
   - 把两处调用注释掉（运行时碰撞完全没执行）⇒ 仍 `命中 2 处`、`ALL PASS`；
   - 用 `if (false)` 包住（同样没执行）⇒ 仍 `ALL PASS`；
   - 83 节点数据级断言也抓不住（Dev 自己在探针头注里写了「删掉调用仍能凑出 ~12px 余量，已实测」，实测为 12.55px）。
   
   ⇒ 对「写了没调用 / 碰撞形同虚设」这条**它自己声称要守**的性质，探针是**假绿**。这正好踩中本仓 `tests/common/mod.rs:24-37` 记过的老坑：「裸 grep 会栽在『守卫被删掉、字符串还留在注释里』这个形态上」。
4. **探针自身还有两处不咬人的写法**（非阻断，但降低可信度）：
   - `layout_probe.js:188`：`pass("起点…", allAtOrigin ? "83/83" : "有节点不在原点")` —— 条件为假时仍打印 `PASS`，只是换了一句说明；这正是文件自己在 `:255-256` 警告的 `pass(name, cond)` 反例。
   - `layout_probe.js:242-244`：只 `console.log("边数 = " + edges.length)`，**没有断言** 52 条；`buildEdges` 少生成边不会红。

### 2.4 还原证据

每个变异后都核对 sha256，全部一致：

```text
M1a RESTORE sha=f3c1b9ec…1358bf identical=True
M1b RESTORE sha=f3c1b9ec…1358bf identical=True
M1c RESTORE sha=f3c1b9ec…1358bf identical=True
M1d RESTORE sha=f3c1b9ec…1358bf identical=True
M2  RESTORE sha=f3c1b9ec…1358bf identical=True
M3a RESTORE sha=f3c1b9ec…1358bf identical=True
M3b RESTORE sha=f3c1b9ec…1358bf identical=True
FINAL sha=f3c1b9ec…1358bf identical=True
```

`qa-mut` 的 `git status --short` 为空；`feature-REQ-114` 的 `src/server/page.rs` sha256 仍是
`f3c1b9ec…1358bf`，工作区干净；**未改 feature 分支、未 push**。

### 2.5 建议的修法（供 Dev 复修）

- 抽出内核后**先剥 JS 注释**（或只匹配 `physicsStep` 函数体、排除 `//` 行），再要求 `=== 2`；
- 或者给 `collide` 加运行期哨兵（如 `window.__collideHits++` / 模块内计数器），在推进后断言「至少 N 次」；
- 或让数据级判据对「碰撞是否执行」敏感：把 `REP` 临时设为 0（只留碰撞）再跑 83 节点，这样删掉 collide 会立即出现负余量；
- 把 `起点` 改成 `expect(allAtOrigin, …)`；把 `edges.length === N_EDGES` 变成 `expect`。

---

## 3 · 拖拽 / 固定 / 双击（源码级判据）

均以 `src/server/page.rs` 当前行为准；行号为 HEAD `5d9b483`。

### 3.1 AC-001 · 拖拽、松手固定、双击释放

| 判据 | 证据（`path:line`） | 结论 |
| --- | --- | --- |
| 节点上绑定 `pointerdown` | `page.rs:518-520` `bindNode` → `g.addEventListener("pointerdown", … startDrag(d, ev))` | ✅ |
| 而非背景 | `page.rs:645-653` `#svg` 背景 `pointerdown`，`:647` 遇 `.nd` 直接 return | ✅ 节点/背景分流 |
| pinned 状态 | `page.rs:532-538` `DRAG = { wasPin: d.pin, … }`；`:538` `d.pin = true` | ✅（状态名 `pin`，语义同 `pinned`） |
| 拖动跟随 | `page.rs:548-559` `onMove` 用 `toWorld` + 起始偏移更新 `d.x/d.y`，`:556` 速度清零 | ✅ |
| 松手不回弹 | `page.rs:573-578`：`moved` 分支**保持** `d.pin = true`；物理步 `:300` 对 `a.pin` 直接 `continue`（不写位置），`collide` `:256-257` 对 pinned 权重 0 | ✅ 下一帧不会把位置冲掉 |
| 双击释放 | `page.rs:602-616` `tapNode`：同 id 且 `now - TAP.t <= DBL_MS` → `clearTimeout` + `release(d)`；`:635-642` `d.pin=false` + `kick(0.55)` | ✅ |
| 不直接绑 `dblclick` 释放 | `page.rs:521-523,529-530`：`dblclick` 只 `preventDefault`，避免浏览器先 fire 两次 click 导致开两次面板 | ✅ 理由正确 |

### 3.2 AC-003 · 拖拽不触发点击

- `DRAG_PX = 4`：`page.rs:204`。
- 位移判定：`page.rs:550-552` `if (!DRAG.moved && hypot(dx,dy) > DRAG_PX) DRAG.moved = true;` —— 严格 `> 4px`，与 AC 一致。
- 松手分支：`page.rs:575-584`；只有 `!DRAG.moved` 才 `tapNode(d)`，否则只 `say("已固定")`。✅

### 3.3 双击语义与单击代价

- `DBL_MS = 300`：`page.rs:205`。
- 双击**只释放、不开面板**：`page.rs:604-608` 清掉待发单击、`TAP=null`、`release(d)` 后直接 `return`。✅
- 单击延迟开面板：`page.rs:611-615` 用 `setTimeout(..., DBL_MS)` 才 `pick(d.id)`；`page.rs:597-599` 明确写代价「单击开面板最多慢 300ms」；`flushTap`（`:619-625`）在换目标时立刻补发上一次单击。✅ **代价已如实说明**。
- 触屏退化：`page.rs:600-601` 明确「触屏没有 dblclick，但 pointerdown/up 成对 ⇒ 300ms 内点两下同一节点仍是双击=释放；点不同节点则第一次立刻开面板」。✅ 说明到位。
- 触屏补充（**未声称、但值得知道**）：页面只有 `wheel` 缩放（`:656-668`），没有双指 pinch；触屏只能拖/点/双击，不能缩放。AC-004 只要求滚轮，故不算违约，但这是触屏可用性的已知缺口。

### 3.4 AC-004 / AC-005 / AC-006

| AC | 证据 | 结论 |
| --- | --- | --- |
| AC-004 适应窗口 | `#fit` 按钮 `page.rs:169` + `:219` 绑定 `fitView`；`fitView` `:372-390` 按节点包围盒算 k 与中心 | ✅ |
| AC-004 重置 | `#reset` `:170` + `:220`；`resetView` `:393-398` 明确 `VIEW = { k:1, x:0, y:0 }` | ✅ |
| AC-004 滚轮/平移保留 | `wheel` `:656-668`；背景 `pointerdown`/`PAN` `:645-653,560-565` | ✅ |
| AC-005 标签 | `LBL_K=0.6` `:206`；`showLbl = k >= LBL_K` `:441`；`:468-491` 按「搜索命中优先 → 属性数」排序并做矩形避让，重叠则 `nolbl` | ✅ 源码级 |
| AC-006 搜索 | `#q` `:168`；`input` `:673-684` 高亮/变暗；`keydown Enter` `:685-696` → `centerOn`；`:401-409` 平移视口并高亮 | ✅ |

### 3.5 R2 · 力导向收敛

- 帧循环：`page.rs:337-347`；`frames < SIM_FRAMES (300)` 且 `alpha > ALPHA_MIN (0.006)` 才继续，否则**停帧**；alpha 每帧 `×0.97`（`:342`）。
- 拖拽中才保持 `raf`（`:343` `if (DRAG) { raf = requestAnimationFrame(loop); return; }`），页面注释 `:49-52` 说明「稳态时一个 rAF 都不在跑」。
- 探针实测：168 帧后 alpha 收敛，**不是满帧 300 凑数**。✅

---

## 4 · 碰撞与收敛（数据级）

### 4.1 实际代码（探针输出）

| 指标 | 实测 |
| --- | --- |
| 节点 / 边 | 83 / 52 |
| 推进帧数 | 168（上限 300） |
| 停止原因 | alpha 掉到 `ALPHA_MIN` 以下（收敛） |
| 最紧一对 | #6 / #47：中心距 42.79，半径和 26.99，**余量 +15.79px**（断言 ≥ 0） |
| NaN / 爆冲 | 0 |
| 包围盒 | 529 × 528（世界单位） |

真 HTTP 的 `/api/graph` 也返回 **83 nodes / 52 edges**，与探针场景一致（§6）。

### 4.2 变异下的数据级分辨力（重要）

- **M2（`min=0`）**：83 节点数据级仍是 `余量 12.55px`（绿），红的是 `collide` 的两个单元断言（`-18.00` / `-17.00`）。说明「所有节点同点起跑 + 斥力」本身就能凑出正余量，**单看最紧余量无法证明 collide 的 `min` 正确**；目前靠单元级 `collide` 断言兜底。
- **M1b/M1c/M1d（调用被删/注释/死代码）**：数据级也仍是 `12.55px`（绿）。即数据级判据对「碰撞调用是否存在」完全没有分辨力。
- ⇒ 探针的「有牙」目前**只在 M2（算法数学）和 M3（内核抽取）上成立**；对 M1（调用接线）不成立，且文本守卫可被注释绕过。这是 §1 阻断项的实测依据。

---

## 5 · golden 重锚核对

### 5.1 H1 与三处字节一致

| 对象 | 字节数 | sha256 |
| --- | --- | --- |
| `src/server/page.rs` 的 `INDEX_HTML` 常量本体 | 32276 | `1f86eae66e9a7b6b6c3b1e01fe82ff897c6d2fd2b9f61da46964a8c70231792e` |
| `tests/fixtures/req107/golden/H1_index.html` | 32276 | 同上 |
| 真 HTTP `GET /` 响应体 | 32276 | 同上 |

⇒ **`INDEX_HTML` == H1 == HTTP 响应体，逐字节一致**（不是「看起来一样」）。

### 5.2 `SHA256SUMS` 与其余 12 份 golden

- `SHA256SUMS` 的 H1 行已同步为 `1f86ea…`；13 条记录逐条复算，**0 条 mismatch**。
- 对比 `develop`（`e9755e8`）：golden 目录只有 `H1_index.html` / `PROVENANCE.txt` / `SHA256SUMS` 三份改动；
  `C1_html.txt`、`C3_json.txt`、`C4_overview.txt`、`C5_overview_json.txt`、`H2_graph.json`、
  `H3_node_1.json`、`H3_node_2.json`、`M1.json`、`M2.json`、`M3.json`、`M4.json`、`M5.json`
  **共 12 份逐字节未动**（`git diff --name-status develop..HEAD -- tests/fixtures/req107/golden/` 只列出 H1 + PROVENANCE + SHA256SUMS）。
- `SHA256SUMS` 的 diff 只有 H1 一行变化，其余 12 行未动。✅

### 5.3 `PROVENANCE.txt`

- 已写明「**REQ-114 改了页面交互**」、为什么必须重锚（页面内联 JS/CSS 变化）、
  以及两个 sha：旧 `96eb5f…`（12479 字节）、新 `1f86ea…`（32276 字节）。
- 旧 sha 实测 = `develop:e9755e8` 的 H1（`96eb5f…`，12479 字节）；新 sha 实测 = HEAD H1。✅
- 中间提交 `e6f0947` 的 H1 实测 `40d178…` / 29943 字节，与 PROVENANCE `:43` 的「40d178…（29943 字节）」一致。✅
- **一处措辞不准确（非阻断）**：`:30` 把旧 H1 写成「b714f48 那一份」；`git ls-tree b714f48 tests/fixtures/req107/golden` 为空，b714f48 没有 golden 目录。旧 12479 字节的 H1 实际是 **REQ-110 重锚后的产物**（develop 上那一份）。hash/大小本身正确，只是来源标注错了。

---

## 6 · 零外部依赖 + 页面大小 + 真 HTTP 数据面

### 6.1 零外部依赖

对 `INDEX_HTML` 和真 HTTP 响应体分别扫描：

| 模式 | 命中 |
| --- | --- |
| `<script src=` | 0 |
| `<link href=` | 0 |
| `https://` | 0 |
| `http://` | 0 |
| `@import` | 0 |
| `integrity=` | 0 |

页面只有 1 个**内联** `<script>`；所有请求都是同源相对路径：
`fetch("api/scope")`、`fetch("api/graph?…")`、`fetch("api/node?…")`、`fetch("api/ontology.ttl")`。
SVG 箭头用页内 `<marker>`，字体只用系统字体栈。✅

### 6.2 页面大小

- `INDEX_HTML` 字节数 = **32276**（`< 60KB`；按 UTF-8 字节，不是字符数 27431）。✅

### 6.3 真 HTTP（真二进制 + 真 PostgreSQL 16 + 真 socket）

以 `ONTOK_VIEW_TOKEN=qa114-secret-token`、`ONTOK_DATABASE_URL=…/anspire_real` 起
`ontok serve --project anspire_real --version 2 --bind 127.0.0.1:8797`：

| 请求 | 结果 |
| --- | --- |
| `GET /` | **200**，32276 bytes，sha256 `1f86ea…` |
| `GET /` 内项目名 `anspire_real` | **0 命中** |
| `GET /` 内 9 个真实对象名（客户功能权限/大模型服务商/客户订阅/兑换码/站内信/套餐模板/产品目录/用量明细/客户账号绑定） | **全部 0 命中** |
| `GET /api/scope` 无 token | **401** |
| `GET /api/graph?project=anspire_real&version=2` 无 token | **401** |
| `GET /api/node?project=…&version=2&id=x` 无 token | **401** |
| `GET /api/ontology.ttl` 无 token | **401** |
| `GET /api/graph?…` 带 token | **200**，12376 bytes，**83 nodes / 52 edges** |
| `GET /api/scope` 带 token | **200**，`{"project":"anspire_real","version":2}` |
| `GET /api/ontology.ttl` 带 token | **200**，220684 bytes |

⇒ AC-007 / AC-008 成立；`/` 仍是零数据壳，`/api/*` 仍全部 401，带 token 的 `/api/graph` 正常。✅

---

## 7 · 4 个「红套件」的独立归因（自己复现，未采信 Dev）

> 复现方式：feature worktree，`ONTOK_TEST_DATABASE_URL=…/ontok_demo`；Windows 上先临时移开
> unix-only 的 `src/bin/ontok-gate-preflight.rs`，跑完 `finally` 挪回并核对 sha256。
> 实测 `PREFLIGHT identical=True`，`git status` 干净。

| 套件 | 我实测到的结果 | Dev 归因 | 独立判断 |
| --- | --- | --- | --- |
| `drop_rate` | `1 failed`：`读不到脱敏 schema.json：../.review-workspace/fix-props/schema.json（os error 3）`；`drop_rate.rs:82` fail-closed | 缺脱敏 `schema.json`（输入不在版本控制） | **成立**。`git ls-files` 无该文件；路径不存在；`ontok-gate-preflight.rs` 模块头也明确写「输入不在版本控制内（.review-workspace/ 被 git 忽略）」。与 REQ-114 改动无关 |
| `test_fk_naming` | `5 passed; 1 failed`：`test_real_schema_customer_id_has_exactly_one_display_name`，`读不到脱敏 schema.json：/home/myg133/…/fix-props/schema.json`（Windows 上该 Linux 绝对路径必不存在） | 同上 | **成立**（同一根因）。注意：Windows 上默认路径是 Linux 绝对路径，需 `ONTOK_SCHEMA_JSON` 才能跑；这是环境/输入问题，不是本次改动 |
| `test_agent_orchestration` | `35 passed; 1 failed`：`test_cli_end_to_end_output_is_byte_identical_to_legacy_extract`，`tests/test_agent_orchestration.rs:1070` `left != right`：CLI 产物少一条 `add_link_type`（t_dev.customer_id → t_customer） | 基线 `page.rs` 上同样红 | **成立**。我在独立 worktree `qa-baseline`（`e9755e8`，即 develop）上跑同一套件：**35 passed / 1 failed，同一测试、同一断言**。该失败只涉及 `ontok-propose`/`extract`，与 `page.rs` 无关；不因 REQ-114 引入 |
| `test_req107_red_lines` | **clean tree：2 passed / 0 failed（绿）**。只有按 Windows 绕行、把 `tests/test_retained_names_req084.rs` 也挪开时才红：`tests/test_req107_red_lines.rs:159` `删除了测试文件`（`git diff --diff-filter=D` 命中） | 绕行产物（挪开了 `test_retained_names_req084.rs`） | **归因成立，但表述不准确**：交付态它是**绿**的，不是「红套件」。我复现了它变红的条件：移走 preflight bin 会让 `test_retained_names_req084.rs` 因 `include_str!("../src/bin/ontok-gate-preflight.rs")` **编译失败**，所以绕行必须连它一起移；red_lines 的「不许删测试」判据于是把这次临时移动判红。**不是产品缺陷** |

补充：`test_retained_names_req084` 在移走 preflight bin 后编译失败（`couldn't read tests\../src/bin/ontok-gate-preflight.rs`），这是 Windows 绕行的已知副作用；恢复 bin 后文件 sha256 一致。该套件在 Windows 上无法与 unix-only bin 同时编译/运行，属于既有环境限制，不是 REQ-114 造成。

---

## 8 · 不退化

### 8.1 基线数字（自己跑的）

| 项 | 实测 |
| --- | --- |
| `cargo test --lib` | **245 passed / 0 failed**（exit 0） |
| `cargo test --doc` | **4 passed / 0 failed**（exit 0） |

### 8.2 指定关键套件（自己跑的）

| 套件 | 实测 |
| --- | --- |
| `test_req107_pipeline_bytes` | 1 passed / 0 failed |
| `test_req107_zero_visible_change` | 3 passed / 0 failed |
| `test_graph_server` | 9 passed / 0 failed |
| `test_req110_ttl` | 15 passed / 0 failed |
| `test_req107_red_lines` | 2 passed / 0 failed（clean tree） |

### 8.3 禁改面 diff（`develop..HEAD`）

以下全部 **0 行**：

```text
src/server/http.rs
src/mcp/tools.rs
src/domain/property_filter.rs
src/review_ui/page.rs
Cargo.toml
Cargo.lock
tests/drop_rate.rs
tests/test_fk_naming.rs
src/domain/view.rs
src/domain/review_view.rs
```

`git diff --name-status develop..HEAD` 只有 5 个文件：

```text
M src/server/page.rs
M tests/fixtures/req107/golden/H1_index.html
M tests/fixtures/req107/golden/PROVENANCE.txt
M tests/fixtures/req107/golden/SHA256SUMS
A tests/fixtures/req114/layout_probe.js
```

⇒ 无新增依赖、无 `#[ignore]`、无删除测试、无红线/tools/guard/property_filter/http/review_ui 改动。✅

### 8.4 未复现的数字

Dev 的 `.feature` 交付物（实际是 REQ-112 的旧报告）里的「43 套件 422」**不是 REQ-114 的**，本轮未采信、也未复跑全量 43 套件（Windows 上需绕行 unix-only bin，且部分套件依赖缺失 schema）。本轮以 §8.1–§8.2 的指定判据为准，全部通过。

---

## 9 · 「无浏览器可验的部分」清单（Dev 是否如实登记）

### 9.1 Dev 已如实写的

`src/server/page.rs` 模块文档 `:54-77`：

- `:54-56` 明确「本机没有 headless 浏览器 ⇒ 只有源码级判据」，并立规矩「⛔ 交付物里不许写『已验证交互流畅』这类无法证的结论」；
- `:61-65` 三层验证：源码级 / 纯函数级（探针入库）/ **真浏览器手测：未做**；
- `:67-77`「已知限制（三条）」：
  1. 标签宽度是 `label.length * 11 + 6` 估算，中英混排可能估宽/估窄；
  2. `REP=400` / `CENTER_F=0.4` 是 Node 扫参得出的，**真实数据观感没在浏览器里看过**；
  3. **未做真浏览器手测**：拖拽手感、标签避让视觉效果、暗色模式配色都只有源码级判据。

`tests/fixtures/req114/layout_probe.js:47-48` 也写了：「本脚本证明的是布局算法，不是浏览器里的观感；拖拽手感、标签避让、暗色配色未覆盖；未做真浏览器手测」。

`PROVENANCE.txt:41-42` 同样登记了三条已知限制（含「未做真浏览器手测」）。

### 9.2 未验证/无法验证的完整清单

| 项 | 状态 |
| --- | --- |
| 拖拽手感、指针跟手程度 | **未做真浏览器手测**（源码级 + 物理步级） |
| 松手后视觉上是否真的不回弹 | 源码级：`pin` + 物理步跳过 + collide 权重 0；无浏览器验证 |
| 双击在真实浏览器中的时序/误触 | 源码级：`DBL_MS=300` + `tapNode`；无浏览器验证 |
| 标签避让的实际视觉效果/中英混排宽度 | 源码级估算；Dev 已列为已知限制 1 |
| 暗色模式配色 | 未在浏览器看；Dev 已登记 |
| 触屏双指缩放 | **未实现**（只有 wheel；`pointerdown` 拖拽/双击可用）；AC-004 未要求，但触屏用户不能缩放 |
| 83 节点真实数据下的布局观感 | 探针只证明数值余量；Dev 已登记「未在浏览器看过」 |
| 力导向 168 帧的**真实浏览器**帧率/卡顿 | 未验证；探针只证明 Node 下 168 帧收敛 |

### 9.3 过度声称检查

- 在 `page.rs`、`PROVENANCE.txt`、`layout_probe.js`、`.feature/*` 中搜索「已验证 / 流畅 / 手测 / 浏览器」：**没有任何「已验证交互流畅」式结论**；相反多次明确写「未做真浏览器手测」。✅
- **一处交付物层面的不准确（非阻断）**：`PROVENANCE.txt:41-42` 说三条已知限制「写进了 `page.rs` 的模块文档**与交付物**」，但 `.feature/verification-report.md` / `.feature/status.md` / `.feature/manifest.json` 至今仍是 **REQ-112 / REQ-107** 的旧内容（`REQ-114` 命中数 = 0）。已知限制实际只写在 `page.rs` / `PROVENANCE.txt` / 探针里，没有写进 `.feature` 交付物。信息本身没丢，但「交付物」这句话不成立。

---

## 10 · 阻断项 / 非阻断项

### 阻断项（必须修）

**B-1 · P0 布局探针无牙 / 可被注释绕过（验证物缺陷，不是产品代码缺陷）**

- **事实**：
  - M1 按字面删掉 `physicsStep` 末尾一行 `collide(ns, GAP);` ⇒ 探针 **exit 0**（剩 1 处调用，静态守卫 `命中 1 处`）；
  - 两处都删 ⇒ exit 1，但仅因文本计数 0；83 节点数据断言仍绿（余量 12.55）；
  - 两处**注释掉** ⇒ exit 0、`ALL PASS`（碰撞完全没执行）；
  - 两处包进 `if (false)` ⇒ exit 0、`ALL PASS`（同上）。
- **为什么阻断**：任务 P0 明确要求 M1 必须 exit≠0；探针头也声称要抓「写了没调用 = 碰撞形同虚设」。实际它数的是**原始文本**，不剥注释、不区分死代码，且只要求 `>= 1`，所以对「碰撞接线被移除」这条性质是假绿。本仓 `tests/common/mod.rs` 已经记录过「裸 grep + 注释残留」的假绿教训，这个探针正好复现了它。
- **修复建议**：见 §2.5（剥注释后计数 `=== 2` / 运行期计数器 / `REP=0` 的数据级判据 / `expect` 起点与边数）。
- **复审口径**：修完后重跑 §2.2 的 M1a/M1b/M1c/M1d；M1a 与 M1c/M1d 必须 exit≠0。产品代码本身无需改（AC-002 实测成立）。

### 非阻断项（建议修，不挡合并）

| # | 项 | 说明 |
| --- | --- | --- |
| N-1 | `.feature` 交付物整体过期 | `verification-report.md` / `status.md` 是 REQ-112，`manifest.json` 是 REQ-107，均无 REQ-114；AC-011 的实质内容在 `page.rs`/`PROVENANCE`/探针中，但按流程应把交付物更新到 REQ-114（含「未做真浏览器手测」与源码判据清单） |
| N-2 | 探针起点断言是空转 | `layout_probe.js:188` 用 `pass(name, 条件 ? … : …)`，条件为假仍 PASS；应改 `expect(allAtOrigin, …)` |
| N-3 | 探针边数未断言 | `layout_probe.js:242-244` 只打印 `edges.length`；应 `expect(edges.length === N_EDGES, …)` |
| N-4 | 内核注释里的探针路径过期 | `page.rs:223` 写 `.review-workspace/req114/layout_probe.js`，实际入库路径是 `tests/fixtures/req114/layout_probe.js` |
| N-5 | PROVENANCE 旧 H1 来源标注错 | `PROVENANCE.txt:30` 说旧 H1 是「b714f48 那一份」；b714f48 无 golden 目录，旧 12479 字节实际是 REQ-110 重锚后的 `develop` 产物（hash/大小正确，仅归因错） |
| N-6 | `test_req107_red_lines` 被列为「红套件」不准确 | clean tree 实测 2/2 绿；只有 Windows 绕行（移走 preflight + retained_names）时才红。归因本身对，但交付态不是红 |
| N-7 | 触屏无缩放 | 只有 `wheel` 缩放；触屏可拖/点/双击但无 pinch。AC-004 只要求滚轮，不算违约，建议在已知限制里补一句 |

---

## 11 · 复现命令

```powershell
# 0) 基线探针
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-114
node tests\fixtures\req114\layout_probe.js
# → exit 0；168 帧；最紧余量 15.79px

# 1) 独立 worktree 做变异（不碰 feature 分支）
cd D:\MyCodes\AI\an_onto_demo
git worktree add --detach .review-workspace\req114\qa-mut 5d9b483
# 在 qa-mut 中改 src/server/page.rs：
#   M1a 删最后一行 collide(ns, GAP);
#   M1b 删两行
#   M1c 两行改成 // collide(ns, GAP);
#   M1d 两行包进 if (false) { ... }
#   M2  var min = a.r + b.r + gap; → var min = 0;
#   M3a 删整段内核（含 BEGIN/END）
#   M3b 只改 END 标记
# 每次：node tests\fixtures\req114\layout_probe.js；然后 git checkout -- src/server/page.rs
# 逐次核对 (Get-FileHash src\server\page.rs -Algorithm SHA256).Hash

# 2) lib / doc
$env:ONTOK_TEST_DATABASE_URL = 'postgresql://postgres:postgres@172.17.0.87:5432/ontok_demo'
cargo test --lib ; cargo test --doc

# 3) 关键套件（Windows 先临时移开 unix-only bin，finally 挪回 + sha256）
cargo test --test test_req107_pipeline_bytes
cargo test --test test_req107_zero_visible_change
cargo test --test test_graph_server
cargo test --test test_req110_ttl
cargo test --test test_req107_red_lines

# 4) 真 HTTP
$env:ONTOK_DATABASE_URL = 'postgresql://postgres:postgres@172.17.0.87:5432/anspire_real'
$env:ONTOK_VIEW_TOKEN = 'qa114-secret-token'
target\debug\ontok.exe serve --project anspire_real --version 2 --bind 127.0.0.1:8797
# 另开终端：curl.exe -s -o page.html -w "%{http_code} %{size_download}" http://127.0.0.1:8797/
# 无 token 的 /api/{scope,graph,node,ontology.ttl} → 401；带 token 的 /api/graph → 200（83/52）
```

---

## 12 · 最终判词

- **产品代码**：REQ-114 的交互升级在源码级/纯函数级/真 HTTP 级都有据；AC-002 的数据级在实际代码上成立；golden 只该变的那一份变了；零依赖/零数据面/不退化都成立。
- **验证物**：P0 的布局探针对 M1 没有牙，且注释掉调用即可假绿。这是本轮唯一阻断项。
- **如实登记**：Dev 没有过度声称，明确写了「未做真浏览器手测」与三条已知限制；但 `.feature` 交付物仍是旧需求，需同步。
- **建议**：按 §2.5 修探针 + 同步 `.feature` 交付物 → 复审（预期 PASS）。产品代码不需要因本轮 QA 退回重写。
