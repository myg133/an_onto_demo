# REQ-114 · 页面图交互升级 —— 验证报告（**QA 首轮 FAIL 后的修复轮**）

> **被测**：worktree `feature-REQ-114` / 分支 `feature/REQ-114` · 基线 `develop` `e9755e8` · **未 push**
> **本轮范围**：QA 阻断 **探针无牙（假绿）** 的修复（A-1 行为级探针 / B 剥注释计数 / C 三项小毛病）
> + 四处文档（D1–D4）。**产品交互逻辑本轮没动**（唯一碰到的产品文件是 `page.rs` 内核段里的一行**注释**，
> 因为 C3 要修它写的探针路径；它改了 `INDEX_HTML` 的字节 ⇒ golden 又重锚了一次）。
> **不碰**：`BA/`、`src/server/http.rs`（`git diff` 0 行）、`Cargo.toml` / `Cargo.lock`（0 行）、四红线。
> **日期**：2026-10-22 · Windows · rustc/cargo 1.96.0 · PG `postgresql://***@172.17.0.87:5432/anspire_real`

---

## ⚠️ 三条必须先读的结论

1. **QA 的 P0 成立**：第一版探针「数原文里的 `collide(ns, GAP);`」—— 把两行**注释掉**或包进
   `if (false) {}` 之后，文本照样被数到 2 处 ⇒ 探针 exit 0。这是**假绿**，不是「可能漏」。
2. **本轮把探针改成行为级**：新增**无斥力场景**（把内核作用域的 `REP` 置 0），此时「不重叠」
   **只可能**由 `collide` 兑现；再加**剥注释后恰好 2 处**的静态判据。五种变异实测**全部 exit 1**。
3. **仍未做真浏览器手测**（本机无 headless 浏览器，也不为此加依赖）。本报告所有「✅」都指
   源码级 / 纯函数级 / 真 HTTP 级判据成立，**不指**交互手感已验证。

---

## 0 · 结论先行

| 项 | QA 实测（修复前） | 修复后（本轮实测） |
| --- | --- | --- |
| **M1a** 两行 `collide(ns, GAP);` 删**一行** | ❌ exit 0 | ✅ **exit 1**（静态 1 ≠ 2 + 场景 2 余量 `-30.94px`） |
| **M1c** 两行改成注释 | ❌ exit 0 ALL PASS | ✅ **exit 1**（剥注释后 0 ≠ 2 + 场景 2 余量 `-32.00px`） |
| **M1d** 两行包进 `if (false)` | ❌ exit 0 ALL PASS | ✅ **exit 1**（静态仍 2 ⇒ 由**场景 2** 抓住：`-32.00px` / 包围盒 `0 × 0`） |
| **M2** `var min = a.r + b.r + gap;` → `0` | ✅ exit 1 | ✅ exit 1（单元 2 条 + 场景 2） |
| **M3** 删内核 / 改 `END` 标记 | ✅ exit 1 | ✅ exit 1（`die()`） |
| **基线** | exit 0 | exit 0（`ALL PASS（场景 1：168 帧 / 余量 15.79px；场景 2：168 帧 / 余量 5.20px）`） |
| 每次变异后还原 | — | sha256 `9E5ABADF…` **前后一致**（逐字节） |

---

## 1 · 探针（`tests/fixtures/req114/layout_probe.js`）

```bash
node tests/fixtures/req114/layout_probe.js   # 全绿 exit 0；任一断言失败 exit 1
```

它**不复制**页面代码：从 `src/server/page.rs` 的 `BEGIN/END` 两条标记之间把「布局内核」段
（`radiusOf` / `collide` / `springs` / `physicsStep` + 常量）**原样抽出**，用 `new Function` 执行同一份源码。
标记找不到 ⇒ `die()` exit 1（不是静默通过）。

### 三层判据（缺一层就会出现假绿）

| 层 | 内容 | 抓什么 |
| --- | --- | --- |
| **静态层** | 抽内核 → **剥 `//` 与 `/* */` 注释** → 数 `collide(ns, GAP);`，要求 **=== 2** | 删一行 / 注释掉 / 多写一处 |
| **单元层** | 直接调 `collide()`：重叠一对被推到「半径和 + GAP」；钉住的节点不动 | `collide` 变成空转 |
| **涌现层 1** | 83 节点同坐标、`REP=400`（页面默认）、300 帧上限 ⇒ 任意两节点中心距 ≥ 半径和 | 整体退化 |
| **涌现层 2** | **同上但 `REP=0`**（`setRep(0)` 改的是内核作用域里的 `REP` 本身）⇒ 只能靠 `collide` 分开 | **`collide` 没执行**（注释/`if(false)`/删调用） |

> **为什么必须有涌现层 2**：只跑真实参数时斥力很强 —— 把 `collide` 的调用**删掉**仍能凑出
> ~12.5px 余量（实测），涌现层 1 会绿。把 `REP` 置 0 之后，不重叠就**只可能**来自 `collide`：
> 变异实测余量直接掉到 `-30.94 / -32.00px`。

### 期望输出（本机实测，内核无 `Math.random`，数字确定）

```text
REQ-114 · 布局内核探针（从 src/server/page.rs **原样抽取**）
内核段 = 3925 字节（radiusOf / collide / springs / physicsStep）
剥注释后内核 = 2255 字节

PASS 剥注释没有把代码剥没（自检）  2255 / 3925 字节
PASS 静态：剥注释后 collide(ns, GAP); 恰好 2 处  实际 2 处（注释/删行/多写都会在这里红）
PASS 页面帧上限 SIM_FRAMES === 300  实际 300
PASS 单元：重叠的一对被推到「半径和 + GAP」  余量 6.00 ≥ GAP 6
PASS 单元：钉住的节点不动（否则拖拽会被下一帧弹回去）  (0, 0)
PASS 单元：让对面的节点让开整个间距  余量 6.00
PASS 场景 1 起点：83 个节点都在 (0,0)，速度 0  83/83
PASS 场景 1 边数 === 52  实际 52

场景 1 · 真实参数（REP=400，页面默认）
  节点数 / 边数        = 83 / 52
  实际推进帧数         = 168（上限 300，停止原因：alpha 掉到 ALPHA_MIN 以下（收敛））
  最紧一对（#6 / #47）中心距 = 42.79，半径和 = 26.99
  余量（距 − 半径和）  = 15.79   ← 断言 ≥ 0
  含 NaN 的节点数      = 0   ← 断言 = 0
  包围盒（世界单位）   = 529 × 528

场景 2 · 无斥力（REP=0 ⇒ 只能靠 collide 分开）
  节点数 / 边数        = 83 / 52
  实际推进帧数         = 168（停止原因：alpha 掉到 ALPHA_MIN 以下（收敛））
  最紧一对（#31 / #74）中心距 = 24.93，半径和 = 19.73
  余量（距 − 半径和）  = 5.20   ← 断言 ≥ 0（collide 没执行就会是 −32 量级）
  含 NaN 的节点数      = 0   ← 断言 = 0
  包围盒（世界单位）   = 288 × 305

ALL PASS（场景 1：168 帧 / 余量 15.79px；场景 2：168 帧 / 余量 5.20px）
exit=0
```

### 五种变异 × 每次还原（原样实测）

| 变异 | 手法 | exit | 关键输出 | 还原 sha256 前 8 | 一致 |
| --- | --- | --- | --- | --- | --- |
| 基线 | 不动 | **0** | `ALL PASS（… 余量 15.79px；… 余量 5.20px）` | `9E5ABADF` | ✅ |
| **M1a** | 两行删一行 | **1** | `FAIL 静态：…恰好 2 处  实际 1 处` + `FAIL 场景 2 … 余量 -30.94px` | `9E5ABADF` | ✅ |
| **M1c** | 两行改成注释 | **1** | `FAIL 静态：…实际 0 处` + `FAIL 场景 2 … -32.00px` + `包围盒 0 × 0` | `9E5ABADF` | ✅ |
| **M1d** | 两行包进 `if(false)` | **1** | `FAIL 场景 2 … -32.00px` + `FAIL 场景 2：节点确实被推开了 … 0 × 0` | `9E5ABADF` | ✅ |
| M2 | `min` 改 0 | **1** | 单元 2 条（`-18.00` / `-17.00`）+ 场景 2（`-32.00px`） | `9E5ABADF` | ✅ |
| M3 | 改 `END` 标记 | **1** | `FAIL 在 page.rs 里找不到布局内核的两条标记（BEGIN/END）` | `9E5ABADF` | ✅ |

### 本轮修掉的三个小毛病（QA 点名）

| # | 问题 | 修法 |
| --- | --- | --- |
| C1 | `pass("起点…", allAtOrigin ? "83/83" : "有节点不在原点")` —— 条件为假也 PASS | 改成 `expect(nodes1.every(…), "场景 1 起点：83 个节点都在 (0,0)，速度 0", "83/83")`；全文件不再有「带条件的 `pass()`」 |
| C2 | 边数没断言 | 两个场景各加 `expect(edges.length === 52, "场景 N 边数 === 52", …)` |
| C3 | 内核注释里的探针路径是 `.review-workspace/...`（不入库） | `page.rs:223` 改成 `tests/fixtures/req114/layout_probe.js`（入库） |

---

## 2 · 产品侧（上一轮已交付，本轮未动逻辑）

| AC | 落点（`src/server/page.rs`） |
| --- | --- |
| 001 拖拽 / 松手固定 / 双击释放 | `505` `pointerdown`（绑在节点上）· `556-559` `d.pin = true`（松手保持）· `601-615` `tapNode` + `release` |
| 002 碰撞消解 | `230` `collide`（位置推离，钉子权重 0）· `288-289` 每帧两遍、物理步最后一步 |
| 003 拖拽 ≠ 点击 | `204` `DRAG_PX = 4` · `533` 位移判定 · `556`/`560` 分支 |
| 004 视图 | `583` 背景平移 · `594` 滚轮缩放（指针锚定）· `205`/`204` 两个按钮 · `380` `VIEW = {k:1,x:0,y:0}` |
| 005 标签避让 | `205` `LBL_K = 0.6` · `426` `showLbl` · `462-475` 重叠跳过 |
| 006 搜索定位 | `611` 输入高亮 · `623` 回车 ⇒ `centerOn` |
| 007 零依赖 / < 60KB | 页面 **32396 字节**；`<script src=` / `<link href=` / `http://` / `https://` / `cdn.` / `integrity=` 各 **0 处** |
| 008 数据面不扩大 | 真 HTTP：`GET /` 200 且与 golden **逐字节相等**；11 个探针词（项目名/对象名）**0 命中**；`/api/{scope,graph,node,ontology.ttl}` 无 token 全 **401** |
| 009 golden 重锚 | `H1_index.html` `96eb5f…`（12479 字节）→ **`d8bd4581c264a483815c0bf67636966da190b3799e8abbaa79c6f5a2a849c5c0`（32396 字节）**；其余 12 份逐字节未动；`PROVENANCE.txt` 写明原因 |
| 010 不退化 | 禁改文件 0 diff；lib 245 / doc 4 |
| 011 如实登记 | 本报告 §4 + `page.rs` 模块文档「已知限制」 |

---

## 3 · 测试（本轮复跑）

```text
lib 245 passed / 0 failed · doc 4 passed / 0 failed
集成 72 个目标：774 passed / 4 failed —— 4 红全部与本需求无关：
  drop_rate / test_fk_naming   → 缺脱敏 schema.json（fail-closed 输入不在版本控制内）
  test_agent_orchestration     → 在基线 page.rs 上同样红（回退复现过）
  test_req107_red_lines        → 跑到它的那一次，为绕行临时挪开了 test_retained_names（单跑 2 passed）
test_retained_names_req084（6 条）→ Windows 上编译不了：它 include_str! 那个 Unix-only 的 bin
页面/守门/golden 专项全绿：pipeline_bytes 1 · zero_visible_change 3 · graph_server 9 ·
  static_texts 3 · red_lines 2 · req110_ttl 15
```

**绕行双证**（`finally` 自动挪回，脚本见交付物）：

```text
BEFORE  src\bin\ontok-gate-preflight.rs  sha256=C7CF95E2…71322D  Test-Path=True
MOVED-AWAY  Test-Path(src)=False  Test-Path(bak)=True
AFTER   sha256=C7CF95E2…71322D  Test-Path=True  bak=False  SHA-MATCH=True
（tests\test_retained_names_req084.rs 同样 SHA-MATCH=True / Test-Path=True）
```

---

## 4 · 如实登记（AC-011，⛔ 不许改写成「已验证交互流畅」）

1. **未做真浏览器手测**：本机无 headless 浏览器；拖拽手感、标签避让的视觉效果、暗色配色
   都只有源码级判据，没有任何一条来自真实浏览器。
2. **标签宽度是估算的**：`paint` 里用 `d.label.length * 11 + 6` 当文本框宽度，没有
   `getComputedTextLength()` 实测 ⇒ 中英混排可能估宽（该显示的被跳过）或估窄（仍可能压字）。
3. **力导向参数是 Node 扫参得出的**：`REP=400` / `CENTER_F=0.4` 让 83 节点收在 ~529×528
   （见探针输出），真实数据下的观感没有在浏览器里看过。

---

## 5 · 文档修正（QA 点名的四条）

| # | 问题 | 修成什么 |
| --- | --- | --- |
| D1 | `PROVENANCE` 把旧 H1 说成「b714f48 那一份」 | 改成「**REQ-110 收口后在 develop 上那一份静态壳**」，并写明 **b714f48 那个基线 worktree 里没有 golden 文件**（golden 是用当时页面现跑生成器得到的） |
| D2 | `PROVENANCE` 说已知限制「写进了交付物」 | 改成**实际落点**：`page.rs` 模块文档「已知限制」小节（核心出处）+ 探针文件头「边界」段；本报告只是照抄 |
| D3 | `.feature/status.md` / `verification-report.md` 还是 REQ-112 | 两份都写 REQ-114 段（旧 REQ-112 原文保留在下方，标「历史」） |
| D4 | `.feature/manifest.json` 还是 REQ-107 | 整份改成 REQ-114（`base_commit` `e9755e8` / `acceptance` `BA/demands/REQ-114/acceptance.md`） |

---

# 历史：REQ-112 · 审核界面接入 AI 建议 —— 验证报告（B-1 阻断修复轮）

> 以下为上一需求原文，保留备查。


# REQ-112 · 审核界面接入 AI 建议 —— 验证报告（**B-1 阻断修复轮**）

# REQ-113 · 排除表：从「脚本预处理」变成产品能力 —— 验证报告

> **被测**：worktree `feature-REQ-113` / 分支 `feature/REQ-113` · 基线 `develop` `e9755e8` · **未 push**
> **日期**：2026-10-22 · Windows · rustc/cargo 1.96.0
> **源库**：`anspire_open_uat`（只读，96 表）· **本地库**：`anspire_real`（新项目 `req113_demo`）
> **清单来源**：`exclusions.json` = BA 的 `code/out/exclusions-ai.json`（`version 0.1.1`，AI 判 13 条 → BA 复核后 12 条）
>
> ⚠️ REQ-112 的验证报告见 `git show 776f956:.feature/verification-report.md`（本文件本轮覆盖为 REQ-113）。

---

## 0 · 结论先行

| AC | 判据 | 结果 |
| --- | --- | --- |
| AC-001 | **①** `items` 段里 grep 表名 = 0；**②** 删掉 `inputs.exclusions` 整段后 grep = 0；实体数 = `tables_extracted` | ✅ ① **0** 次；② **0** 次；实体 **84** = `tables_extracted` |
| AC-002 | `seen − excluded == extracted`（结构性断言） | ✅ 96 − 12 = 84；断言在 `exclusions.rs:887`，不闭合 ⇒ **不写 stats**、exit≠0 |
| AC-003 | 每条有理由/类别/决策人；空理由/非法类别/缺决策人 ⇒ 拒绝文件 | ✅ 三种坏文件各跑一次：exit≠0 且**不产出**任何文件 |
| AC-004 | `inputs.exclusions` = **完整审计段**（`{file, version, sha256, declared_by, reviewed_by, rule, counts, excluded[], flagged[], stale[]}`）；改一字节 sha 变 | ✅ 逐项实测；改一个字符 ⇒ sha256 变；`counts` 与明细自查 |
| AC-005 | 陈旧排除：默认警告 / `--strict-exclusions` exit≠0 | ✅ 两种模式各跑一次（真实库 + 1 条不存在的表） |
| AC-006 | 审核界面只读区块，条数 = `tables_excluded` | ✅ `GET /api/review` → `count=12`、`readonly=true`（真服务、真数据） |
| AC-007 | 排除与「无中文名」分开记，排除优先 | ✅ `excluded_tables ∩ tables_without_cn_name = ∅` |
| AC-008 | 两次运行提案与 stats 逐字节相同 | ✅ proposal `852281ec…` / stats `9c568845…` 两次相同，`git diff --no-index` 无输出 |
| AC-009 | 不退化（红线 0 diff / 不删测试 / 不加依赖） | ✅ 见 §5 |
| AC-010 | 真实库 96 → 排除 12 → 84，12 条理由可读 | ✅ 见 §1.3 |

---

## 1 · 真实库实测（AC-001 / 002 / 007 / 010）

### 1.1 命令

```powershell
$env:SRC_URL = "postgresql://***@172.16.120.20:30432/anspire_open_uat"   # 只读
.\target\debug\ontok-propose.exe --schema ..\code\out\schema.json `
    --names src\extract\table_names.json `
    --out out\proposal.json --stats out\stats.json `
    --project req113_demo --exclude exclusions.json --src $env:SRC_URL
```

### 1.2 三个计数（`stats.json`）

```json
"tables_seen": 96, "tables_excluded": 12, "tables_extracted": 84,
"excluded_tables": [ …12 条，逐条 table/category/reason/decided_by/confidence/evidence… ],
"stale_exclusions": [],
"flagged_tables": [ …11 条 keep_but_flagged（**未被排除**）… ],
"tables_without_cn_name": [ …62 张… ]
```

| 检查 | 值 |
| --- | --- |
| `seen − excluded` | 96 − 12 = **84** = `tables_extracted` = 提案里 `add_entity_type` 条数 |
| **AC-001 ①** `items` 段里 grep 12 个被排除表名 | **0 次** |
| **AC-001 ②** 删掉 `inputs.exclusions` 整段后 grep | **0 次**（表名只允许出现在审计段内；逐名合计 15 次命中都在该段，`llm_provider_back` 是另两张备份表名的前缀） |
| `inputs.exclusions.counts` | `{seen:96, excluded:12, extracted:84, stale:0, flagged:11}`，闭合 |
| `excluded_tables` 与 `tables_without_cn_name` 交集 | **0**（被排除的表根本没进抽取作用域） |

### 1.3 12 条排除（AI 判定 + BA 复核；此处为 `stats.json` 原文摘要）

| 表 | 类别 | 理由 | 决策人 | 置信度 |
| --- | --- | --- | --- | --- |
| `_volc_dts_pg_ddl_18e4b12d…` | cdc_residue | 数据同步工具自动生成的临时 DDL 表，不对应任何客户业务 | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_8f7cc0a6…` | cdc_residue | 同上 | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_9a38a0a4…` | cdc_residue | 同上 | ai-table-judge | 0.97 |
| `alembic_version` | migration_bookkeeping | 数据库迁移工具的版本号记账表，客户业务里没有对应事物 | ai-table-judge | 0.99 |
| `llm_provider_back` | backup_snapshot | llm_provider 的整表备份副本，列结构与主表完全一致 | ai-table-judge | 0.95 |
| `llm_provider_back_20260630` | backup_snapshot | 按日期命名的 llm_provider 备份副本，不是独立业务事物 | ai-table-judge | 0.96 |
| `llm_provider_back_20260701` | backup_snapshot | 同上 | ai-table-judge | 0.96 |
| `llm_auth_cache_change_outbox` | internal_pipeline | 权限缓存变更的内部发件箱，只给同步程序读写，客户不可见 | ai-table-judge | 0.94 |
| `llm_auth_cache_kafka_event_outbox` | internal_pipeline | 内部消息队列发件箱，记录待投递事件而非业务事实 | ai-table-judge | 0.94 |
| `llm_auth_cache_trigger_state` | internal_pipeline | 权限边界判断用的内部状态快照，本身不是业务对象 | ai-table-judge | 0.90 |
| `rpt_dim_customer` | report_intermediate | 报表客户维度表，由客户与渠道数据派生，客户业务里无此事物 | ai-table-judge | 0.82 |
| `rpt_dws_channel` | report_intermediate | 渠道汇总的报表中间结果，不是某一笔业务的发生记录 | ai-table-judge | 0.82 |

> `dim_customer_binding` 被 BA 从排除里**推翻**（有 `cross_currency_enabled` 与渠道归属，是业务状态），
> 现列在 `keep_but_flagged`，**照常进本体**（84 张里有它）。

---

## 2 · 测试与绕行（AC-009）

### 2.1 结果

| 项 | 基线（REQ-112 交付） | 本轮 |
| --- | --- | --- |
| lib | 245 | **272**（+27：`extract::exclusions` 的 27 条单元测试） |
| doc | 4 | **4** |
| 集成套件 | 43 套件 422 | **73 套件**（另有 1 个套件绕行，见 §2.2）；全部用例 **1063 通过** |
| 新增套件 | — | `tests/test_req113_exclusions.rs`（12 条，全绿） |
| 未通过 | 2 处既有红灯 | **3 处，且三处都在基线上逐字复现**（见 §2.3） |

### 2.2 Windows 绕行（`ontok-gate-preflight.rs` 是 Unix-only）

```
sha256(before) = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
sha256(after)  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
sha256(equal)  = True
Test-Path      = True          # finally 里挪回，字节级相同
```

绕行的代价：`tests/test_retained_names_req084.rs` 用 `include_str!` 引那份源文件 ⇒ **该套件编不过**
（REQ-112 已登记的既有红灯，不是本轮引入）。`git status --short` 里**没有 `D` 行**。

### 2.3 三处红灯：**在基线上逐字复现**（用 `git archive e9755e8` 导出基线到临时目录，同样绕行后跑）

| 用例 | 基线 | 本分支 | 归因 |
| --- | --- | --- | --- |
| `drop_rate.rs::real_schema_drop_rate_report` | ❌ 69.8% > 上限 10.0%（表 96 / 属性 1024 / 存活 309） | ❌ 同样 69.8% | **输入 schema**：本机只有 96 张真实表名的 schema，翻译表覆盖 0 ⇒ 属性被丢弃；与 REQ-113 无关（只调 `extract()`/`project()`，本轮未改） |
| `test_fk_naming.rs::test_real_schema_customer_id_has_exactly_one_display_name` | ❌ `agent_white_list/customer_id 仍显示成「客户标识」` | ❌ 同一句 | 同一份 schema 的注释差异，与排除功能无关 |
| `test_agent_orchestration.rs::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract` | ❌ `items` 与库路径不一致 | ❌ 同一处 | REQ-108 分档路由带来的既有差异（REQ-112 已登记为「CLI 端到端」红灯） |

> 另外两条在本轮全量运行里红过、**已修**：`test_anchor_guard`（我的测试用了裸 `env!("CARGO_MANIFEST_DIR")` ⇒ 改走 `common::crate_root()`）、
> `test_req107_source_enumeration`（新模块 / 新 `println!` 未登记 ⇒ 登记 `extract::exclusions` 与那条回执）。

---

## 3 · 确定性（AC-008）

```
proposal.json  run1 = 852281ec94f7acb47c429829d44c93cb41b065c7927bc67c1b8ee55f9485ab66
               run2 = 852281ec94f7acb47c429829d44c93cb41b065c7927bc67c1b8ee55f9485ab66
stats.json     run1 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
               run2 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
git diff --no-index out\proposal.json out2\proposal.json  → exit 0
git diff --no-index out\stats.json    out2\stats.json     → exit 0
```

（两次运行各自独立连源库采样；`sample` 的 digest 是确定性 sha256，且本模块内无 HashMap 迭代、无时间戳。
proposal 变大后 sha 与上一轮不同 —— 上一轮是「元信息 + 侧车」，本轮是「完整审计段」。）

---

## 4 · 陈旧排除（AC-005，真实库）

往 `exclusions.json` 里加一条不存在的表 ⇒ `exclusions-stale.json`：

| 模式 | 退出码 | 输出 |
| --- | --- | --- |
| 默认 | **0** | stderr：`⚠ 陈旧排除 1 条（… 但这份 schema 里没有）：_volc_dts_pg_ddl_0000…`；`stats.stale_exclusions` 记 1 条；`tables_excluded` 仍是 12（陈旧的**不算**排除） |
| `--strict-exclusions` | **1** | `Error: 陈旧排除 1 条（--strict-exclusions ⇒ 失败）：_volc…` + 两种可能；**不产出** proposal / stats |

---

## 5 · 红线 / 禁改（AC-009）

```
git diff --stat develop -- src/server/http.rs src/mcp/tools.rs src/domain/property_filter.rs Cargo.toml Cargo.lock
（空）
```

- `guard()` 实现、`property_filter.rs` G1–G4：**0 行**；
- `Cargo.toml` / `Cargo.lock`：**0 行**（sha256 复用同仓 `extract::sample::sha256_hex`，**未加依赖**）；
- 未删测试、未 `#[ignore]`、未放宽断言；唯一的既有断言变更是
  `src/agent/rule_based.rs` 的 `ExtractionStats` **字段集清单**，从 11 个加到 16 个（AC-002 / 003 / 005 要求的那五个），
  断言仍是**逐字段全等**（不是放宽）。

---

## 6 · 审核界面（AC-006）

真服务真数据（`ontok review serve` + `GET /api/review`，Bearer 口令）：

```json
"exclusions": {
  "present": true, "readonly": true,
  "count": 12, "flagged_count": 11,
  "counts": {"seen":96,"excluded":12,"extracted":84,"stale":0,"flagged":11},
  "file": "exclusions.json", "version": "0.1.1",
  "sha256": "3f77c6b8fd41bded7e5fbe74b507c74a2a385cfc67cccef2b90cada5dacc2c08",
  "declared_by": "ai-table-judge / qwen36-27B",
  "reviewed_by": "ba-michael（人工复核：推翻 1 条排除、接受其余 12 条）",
  "rule": "只排除「不是客户业务世界里的事物」的表：…",
  "tables": [
    {"table":"_volc_dts_pg_ddl_18e4…","category":"cdc_residue","category_label":"CDC / 数据同步残留",
     "reason":"数据同步工具自动生成的临时 DDL 表，不对应任何客户业务",
     "decided_by":"ai-table-judge","confidence":0.97,
     "evidence":"表名为 _volc_dts_pg_ddl_ + 32 位哈希（火山 DTS 同步产物命名）；…"}
  ],
  "flagged": [
    {"table":"internal_staff","why":"名字像内部表，但只有 5 列且无任何列注释…","concern":"若只是后台账号…"}
  ]
}
```

页面两个 **只读** section（`page.rs:163` / `:178`）：`#excluded`「被排除 12 张」+ `#flagged`「保留但存疑 11 张
（**没有被排除** · 已照常进本体 · 待裁决）」。⛔ 无按钮、无输入框、无写接口。

> 数据来源：`proposals.inputs`（`propose import` **只读 proposal.json 这一个文件**，校验审计段后落库；无侧车）。
> 界面**不解析** `exclusions.json`、也没有 schema —— 「哪些表被排除」只有一处实现。

---

## 7 · 已知限制

1. **审计段里含表名**（`inputs.exclusions.excluded[]`）：BA 裁定「单一文件」—— `items` 段 grep = 0，
   且整份文件里表名只允许出现在审计段内。提案因此大 ~4 KB，换「一个文件 = 一条完整审计链」。
2. **`keep_but_flagged` 只展示**，不参与判定（表照常进本体）。
3. **表级**排除（R1）；列级仍是 `property_filter`。
4. **`tables_extracted` 实测自实体条目数**：重建路径若少抽表，闭合断言会让这一轮失败（R5 的本意，
   但相对旧行为是**行为变更**）。
5. **Windows 绕行**导致 `test_retained_names_req084` 编不过（既有红灯）。
6. `drop_rate` / `test_fk_naming` 在本机为红，**基线同样红**（§2.3）。
