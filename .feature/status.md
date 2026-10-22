# 开发状态 · REQ-116 文档摄取与断言抽取（第一步：文档 → 带证据的断言）

> worktree `feature-REQ-116` · 分支 `feature/REQ-116` · 基线 `develop` `a85383a` · **未 push**
> 本轮性质：**QA 判 PASS（6 条非阻断）→ BA 把 N-2 / N-5 改判阻断**；本轮修完这两条，并补齐 N-1 / N-4 / N-6
> 日期：2026-10-22 · Windows · 共享库 `postgresql://***@172.17.0.87:5432/anspire_real`（**只读指纹**）

- **需求**：REQ-116（R1–R6 / AC-001~AC-011）
- **状态**：进行中（2 条阻断已修，3 条非阻断已补，待复核/合并）

## 一句话

`ontok doc ingest`：`.md` / `.txt` → **确定性分块** → 模型抽 概念/关系 + `quote` → **逐字证据校验**（对不上就丢 + 计数）→ JSONL + `stats.json`。
⛔ 不写库、不改本体、不出图；指纹缓存是**本地文件**（`<out>.cache.json`），不是 `ai_cache` 表。

## 本轮（第 2 轮）的 6 条

| # | QA 判定 | 项 | 落点 | 一句话 |
| --- | --- | --- | --- | --- |
| **N-2** | **改判阻断** | AC-004 的判据**恒真** | `src/extract/doc_ingest.rs:998`（先记原始分布再过闸）、`:1282-1354`（新计数 / 占比）、`:1609`（stats.json 字段）、`src/cli.rs:1351` | 新增 `raw_predicate_counts` / `rejected_by_predicate_gate` / `belongs_to_share{raw,alive}`；`belongs_to` 在**过闸之前**就记，占比按**原始输出**算 |
| **N-5** | **改判阻断** | 隐藏项被**静默跳过**（fail-open） | `src/extract/doc_ingest.rs:365`（注释）/ `:374`（`walk_docs` 不再 `continue`） | 裁定 **(a)**：隐藏项**照常处理、照常报错**；隐藏 `.md` 照常读，隐藏 `.pdf` / 隐藏目录里的 `.pdf` ⇒ exit≠0 |
| **N-1** | 非阻断 | `.feature/` 三份还是 REQ-115 | 本文件 / `verification-report.md` / `manifest.json` | 全部写成本需求，数字用**本轮实测** |
| **N-4** | 非阻断 | M2 变异在集成层漏网 | `tests/test_req116_doc_ingest.rs:573` | 假端点新增「原文前缀 + 原文没有的尾巴」；变异后该用例**红**（已实测，见下） |
| **N-6** | 非阻断 | `.PDF` 报错里显示 `.pdf` | `src/extract/doc_ingest.rs:294-309` / `:398` | 报错用**实际后缀**（`raw_extension`），判「能不能吃」仍用 `lower_extension` |
| **N-3** | **只登记** | 语料过拟合 | — | 4 份自造语料**过度贴合提示词** ⇒ 只能当冒烟夹具；AC-004 的真实验证**等业务文档** |

## 判据实测（摘要，细节见 `verification-report.md`）

| 判据 | 实测 |
| --- | --- |
| N-2 · 构造含 `belongs_to` 的输出 | `raw_predicate_counts={belongs_to:14, 下单:14}`、`rejected_by_predicate_gate={total:14, by_predicate:{belongs_to:14}}`、`belongs_to_share={alive:0/14=0.0%, **raw:14/28=50.0%**}` |
| N-2 · 真语料（14 块 / 51 条原始关系） | `belongs_to` 原始 **0/51 = 0.0%** —— 这是**真 0**（分母 51 ≠ 0，说明判据测得到）；另有 2 条被**证据**丢掉的也计入原始分布（51 − 49 = 2） |
| N-5 · 隐藏目录里的 `.pdf` | `exit=1`、点名 `.pdf`、**不出产物**（第一版是 `exit 0` 一声不吭 —— QA 判的正是这个） |
| N-5 · 隐藏的 `.md` | `exit=0`、1 块、`file=.secret.md` |
| N-6 · `.PDF` | 报错显示 **`.PDF`**、`exit=1` |
| N-4 · 变异（`locate_quote` → 前缀匹配） | lib **3 红** + 集成 `ac003_prefix_plus_invented_tail_is_dropped` **红**（修之前集成 8 条全绿）；还原后 sha256 一致 |
| 证据 / 确定性 / 预算 / 不写库 | 见上一轮实测：101 条 span 独立复核 `bad=0`；第二次 0 token + JSONL 同 sha256；`--budget-tokens 1` ⇒ exit 1 不产文件；7 张表内容 sha256 前后一致 |

## 测试

```text
lib 315 / doc 4 · tests 77 个目标：813 passed / 8 failed（5 个用例，全部既有/环境，详见报告 §7）
bin 单测 3 个目标：14 passed
新增 tests/test_req116_doc_ingest.rs（10 passed，含 N-4 / N-5 两条新判据）
绕行：preflight 挪开 + retained 用例 include_str 单行指到 .moved，均 finally **逐字节还原**
      sha256 前后 C7CF95E2… / 2DB7D22C… 相同、Test-Path=True、git status 无 D 行
      ⚠️ 教训：Copy-Item 还原会保留**旧 mtime** ⇒ cargo 认为「不用重编」，
      本轮因此跑过一次**变异版产物**（假红）；已 touch + cargo clean -p 重跑，这才是报告里的数字
```

## 如实登记

1. **N-3 语料过拟合**（QA 判定）：4 份夹具是在看过提示词之后写的（`04` 的「术语对照」恰好是规则 2 的示例；`03` 逐字复刻规则 4 的「计划/将/待定/尚未」；`01` 首段先把要抽的概念列出来）⇒ **只能当冒烟夹具**；AC-004 的真实分布**等业务文档**。
2. **缓存是本地文件**，不是 `ai_cache` 表 —— 为了让「不写库」是**结构性事实**（本模块没有 `sqlx` / 连接池）。若要复用表：`DocCache` 是注入式 trait，改动面只在存储层。
3. **`.txt` 没有标题层级**（只按段落 + 1200 字符上限分组，绝不切句）。
4. **隐藏项照常处理** ⇒ 别把仓库根 / 家目录当 `--input`（`.git` 之类非文档文件会按 AC-001 报错）。
5. `--budget-tokens` 省略时默认 **500000**（stderr 明说，不是静默）。
6. 本机无业务文档 ⇒ 真实语料只有 4 份自造夹具；「101 条断言」是**冒烟规模**，不是业务规模。

---

# 历史：REQ-115 概念分组与图上的归并

> worktree `feature-REQ-115` · 分支 `feature/REQ-115` · 基线 `develop` `71ae770` · **未 push**
> 本轮性质：**QA 判 FAIL（1 阻断）→ 第 2 轮按 BA 裁定 (C) 修复**：
> C-1 组字段纳入导入闸 / C-2 `suggest` 不写客户字段 / C-3 图与总览同口径

- **需求**：REQ-115 · 概念分组与图上的归并（AC-001~AC-011）
- **状态**：进行中（阻断已修，待 QA 复审）
- **日期**：2026-10-22

## 第 2 轮 · QA 阻断修复（BA 裁定 C）

QA 实测：AI 的 8 组声明走完 import → 签 → apply → freeze 后，`GET /api/graph` **500**，
stderr `守门器命中 1 项建模术语 ["key"]`。根因：`api_entitlement.scope_hint` 里的
「未持 **Key**」被发布闸当成词条 `key`，而 `scope_hint` 被原样带到了线上。

BA 纠正了「scope_hint 是评审注解」这个前提：它在 REQ-010 里就是**客户可见的组说明**
（业务总览 C4/C5/M1 的 `summary`）。所以真正的病灶是**一个字段两种用途**。裁定 (C)：

| # | 项 | 落点 | 一句话 |
| --- | --- | --- | --- |
| **C-1** | 组字段纳入**导入闸** | `src/domain/concept.rs:349` `scan_group_names`（`display_name` / `scope_hint` / `key`）；`src/cli.rs:1211` | 命中即拒，报**组 + 字段 + 词条**；⛔ 不静默剔除、⛔ 不改写 |
| **C-2** | `suggest` **不写** `scope_hint` | `src/domain/concept_suggest.rs:201`（`scope_hint: None`） | 客户字段留空；算法来源在 `declared_by` / `rule` |
| **C-3** | 图与总览**同口径** | `src/domain/graph.rs:125`（`Group.scope_hint` 恢复）；`src/server/page.rs:1160` | 两个出口都带它 —— 因为导入时已过闸（C-1） |

判据（真库）：

```text
# C-1 ①：AI 声明**原样** import ⇒ 被拒（点名组 + 字段 + 词条）
exit 1 · 组 `api_entitlement` 的 scope_hint 含建模术语 `key`
# C-1 ②：display_name 命中（把 customer_account.display_name 改成「客户与 Key 子账号」）
exit 1 · 组 `customer_account` 的 display_name 含建模术语 `key`
# C-1 ③：把「未持 Key」改成「未持凭证」、重算 digest ⇒ 导入成功
概念声明 v1.0.0 已导入提案 p1 … 共 8 个概念组
# 往返：签 → apply → v4 → freeze → serve
GET /api/graph?project=anspire_excl_r115&version=4
  status = 200 · nodes=83 · groups=8 · covered=83
  raw contains literal "key" = False · raw contains scope_hint = True（C-3 同口径）
# 新发现的出口（C4/C5）也验掉：
ontology overview --version 4             → exit 0
ontology overview --version 4 --json      → exit 0
# C-2：suggest 产出 9 组，scope_hint 全为 null
groups with scope_hint = 0 · sha256 两次相同 abc9068a…
```

## 第 3 轮 · QA 复审补项（N-1 阻断 / P1-1 / N-2）

| # | 项 | 落点 | 一句话 |
| --- | --- | --- | --- |
| **N-1** | 导入闸的**接线**用**真进程**钉住 | `tests/test_req115_import_gate.rs`（新增 2 条） | 脏 `scope_hint` ⇒ exit 1 + stderr 点名组/字段/词条 + **提案数不变**；干净 ⇒ exit 0 + 提案 +1 |
| **P1-1** | 边界写准确（不许写成「已封堵」） | `src/domain/graph.rs`（`Group` 文档 / L5）+ `PROVENANCE.txt` | C-1 只挡**新导入**；**已入库的旧快照仍按原行为拒发**（v3 仍 500）；清理需重新导入/重建快照 |
| **N-2** | 词元级匹配，**保持现状** | `src/domain/concept.rs` `scan_group_names` 文档 + 单测 | `api_key_group` / `license_key` **不算命中**（有意）—— 否则「API Key」这个业务词没法用 |

N-1 的**变异实测**（有牙）：把 `src/cli.rs` 的闸调用整段注释掉 ⇒
`test_req115_import_gate::dirty_scope_hint_is_rejected_by_the_real_import_process` **红**：
`left: Some(0), right: Some(1)`，stdout 显示「已导入提案 p1」。恢复后 2/2 绿。
## 本轮改了什么

| # | 项 | 文件 | 一句话 |
| --- | --- | --- | --- |
| 1 | `/api/graph` 返回 `groups` + 节点 `group` | `src/domain/graph.rs` / `src/server/http.rs` | 权威层只有 `concept_groups`；三态 `groups` 保证 H2 golden 不动 |
| 2 | 页面：组圈 / 折叠 / 组面板 / 组内引力 | `src/server/page.rs` | 默认全折叠；折叠 = 一个「组名 + N」的聚合节点；⛔ 页面不算分组 |
| 3 | `ontok concept suggest` | `src/cli.rs` / `src/domain/concept_suggest.rs` | 贪心模块度（整数打分、有序迭代）；只产声明文件、**不写库**；`scope_hint` 留空 |
| 4 | 无组时如实显示「未分组」 | `src/server/page.rs` | `GROUPS.length === 0` ⇒ 一个聚合节点都不画，⛔ 无默认组 |
| 5 | 往返 | 真库 `anspire_excl` → `anspire_excl_r115` | suggest（9 组）与 AI（8 组，改一句措辞后）各走了一遍 import → 签 → apply → freeze → 图 + 总览 |
| 6 | H1 golden 重锚 | `tests/fixtures/req107/golden/*` | `d8bd4581…`（32396）→ `c8d5e148…`（49936）；其余 12 份逐字节未动 |

## 判据实测（摘要，细节见 `verification-report.md`）

| 判据 | 实测 |
| --- | --- |
| `/api/graph` 有组（AI 8 组，C 轮） | `status=200 · nodes=83 · groups=8 · covered=83`，无字面 `"key"`，`scope_hint` 在（C-3） |
| `/api/graph` 有组（suggest 9 组） | `nodes=83 / groups=9 / covered=45 / ungrouped=38` |
| `/api/graph` 无组 | `groups:[]` + 83 个 `group:null`，**没有默认组** |
| `ontology overview`（C4/C5） | v4 上 **exit 0**（C 轮新验的出口，已不再 500） |
| 折叠态节点数（Node 探针） | `7 = 3 组 + 4 未分组`，`exit 0` |
| 无组（Node 探针） | 渲染单元 = 对象数、零聚合单元 |
| `concept suggest` 两次 | sha256 相同（C-2 后 `abc9068a…`，两次一致） |
| suggest 不写库 | 库指纹 `b4ad1c172ad37f28` 跑前跑后相同（代码未动） |
| 折叠不改数据 | TTL / `ontology show` 折叠前后 sha256 两两相同 |
| 页面不算分组 | 剥注释后 7 个算法词全 0；只读 `DATA.groups` |
| C-1 三种实测 | ① scope_hint 命中拒 ② display_name 命中拒 ③ 都干净 ⇒ 导入成功 |
| 审核界面 | `/api/review` 条目 `payload.scope_hint` 仍是完整原文 |
| 401 | 无 token `/api/graph` → 401，正文不回显分组内容 |

## 测试

```text
lib 291 / doc 4 · 集成 75 个目标：802 passed / 3 failed（3 条均为既有红灯，基线复现）
新增 tests/test_req115_groups.rs（13 passed）+ tests/test_req115_import_gate.rs（2 passed · 真进程）
绕行：src/bin/ontok-gate-preflight.rs 挪开 → 跑 → 挪回；sha256 前后 c7cf95e2…；Test-Path=True
      代价 test_retained_names_req084 编不过（与 REQ-113/114 同一口径）
```

## 如实登记

1. **设计缺陷（下一轮拆字段）**：`scope_hint` 一个字段两种用途 —— 既是客户可见的组说明
   （总览 `summary`），又容易被写成内部备注。本轮靠 C-1（导入时过闸）+ C-2（suggest 不写）
   缓解；根本修法是拆成 `scope_hint`（客户可见，过闸）+ `review_note`（内部轨，不过闸），
   要动 REQ-010 的 schema 与快照，**下一轮**。
2. **AC-001 的 `groups[].key` 在线上叫 `id`**（`key` 是闸词，写成键名会 500）——与 C-1 是
   同一道闸的「键名/值」两面。需 BA/QA 裁定 AC 措辞（见报告 §8.1）。
3. **AI 声明需要人改一句**：`api_entitlement.scope_hint` 里的「未持 Key」→「未持凭证」
   （报错会点名；我们不替作者猜业务措辞）。改完重算 digest 即可全链路通过。
4. **未做真浏览器手测**（本机无 headless 浏览器）；组圈观感 / 折叠动画 / 引力手感只有源码级 + Node 探针。
5. **组成员只算图里看得见的**：真库现场 84 实体 → 83 节点；同名对象只顾第一个（REQ-093 L2）。
---

# 历史：REQ-114 页面图交互升级

> worktree `feature-REQ-114` · 分支 `feature/REQ-114` · 基线 `develop` `e9755e8` · **未 push**
> 本轮性质：**QA 首轮判 FAIL（阻断 P0：探针无牙）→ 只修探针 + 三处文档 + 两条非阻断**，产品逻辑不再动

- **需求**：REQ-114 · 页面图交互升级（AC-001~AC-011）
- **当前任务**：布局探针改为**行为级**（无斥力场景 + 剥注释计数）已完成，**待 QA 复审**
- **状态**：进行中（PR 与状态机流转由 BA 决定，Dev 不发布）
- **日期**：2026-10-22

## 本轮改了什么（QA FAIL 后的修复轮）

| # | 项 | 文件 | 一句话 |
| --- | --- | --- | --- |
| **A-1** | 探针**行为级**化 | `tests/fixtures/req114/layout_probe.js` | 新增**无斥力场景**（`REP=0`）：83 节点同坐标时「不重叠」只可能由 `collide` 兑现 ⇒ 删一行 / 注释 / `if(false)` 全都会红 |
| **B** | 静态判据**剥注释 + 恰好 2 处** | 同上 | 先剥 `//` 与 `/* */` 再数 `collide(ns, GAP);`，要求 `=== 2`（原来是裸正则 `>= 1`，注释掉的文本照样被数到） |
| **C1** | `pass()` 假绿 | 同上 | 起点判据原来用 `pass(name, 三元字符串)` —— 条件为假也 PASS；改成 `expect(cond, …)` |
| **C2** | 边数没断言 | 同上 | 加 `edges === 52`（两个场景各一条） |
| **C3** | 内核注释里的探针路径过期 | `src/server/page.rs:223` | `.review-workspace/req114/layout_probe.js`（不入库）→ `tests/fixtures/req114/layout_probe.js`（入库） |
| **D1** | `PROVENANCE` 把旧 H1 说成「b714f48 那一份」 | `tests/fixtures/req107/golden/PROVENANCE.txt` | 改成「REQ-110 收口后在 develop 上的静态壳」，并写明 b714f48 那个 worktree 里根本没有 golden |
| **D2** | `PROVENANCE` 说已知限制「写进了交付物」 | 同上 | 改成实际位置：`page.rs` 模块文档「已知限制」小节 + 探针文件头「边界」段 |
| **D3** | `.feature/*` 还是旧需求 | `.feature/status.md` / `verification-report.md` / `manifest.json` | 三份全部写成本需求（REQ-114） |

⚠️ C3 改的是 `INDEX_HTML` **里面**的注释 ⇒ 页面字节变了 ⇒ golden 又重锚一次（`1f86ea…` 32276 → `d8bd4581…` 32396），`SHA256SUMS` 与 `PROVENANCE` 同步。

## 判据实测（摘要，细节见 `verification-report.md`）

| 判据 | 实测 |
| --- | --- |
| 基线探针 | `ALL PASS（场景 1：168 帧 / 余量 15.79px；场景 2：168 帧 / 余量 5.20px）` **exit 0** |
| M1a 两行删一行 | 静态层 `1 ≠ 2` + 场景 2 余量 `-30.94px` ⇒ **exit 1** |
| M1c 两行改成注释 | 剥注释后 `0 ≠ 2` + 场景 2 余量 `-32.00px` ⇒ **exit 1** |
| M1d 两行包进 `if(false)` | 静态层仍 2 ⇒ 由**场景 2** 抓住（余量 `-32.00px`、包围盒 `0 × 0`）⇒ **exit 1** |
| M2 `min=0`（collide 空转） | 单元 2 条 + 场景 2 ⇒ **exit 1** |
| M3 改 `END` 标记 | `die()`：找不到标记 ⇒ **exit 1** |
| 每次变异后还原 | sha256 `9E5ABADF…` 前后一致（逐字节） |

## 测试

```text
lib 245 · doc 4 · 集成 72 个目标（774 用例），4 个目标红且全部与本需求无关：
  drop_rate / test_fk_naming     → 缺脱敏 schema.json（fail-closed 输入不在版本控制内）
  test_agent_orchestration       → 在基线 page.rs 上同样红（已实测回退复现）
  test_req107_red_lines          → 绕行产物（临时挪开 test_retained_names 才会红；单跑 2 passed）
test_retained_names_req084（6 条）→ Windows 上编译不了（include_str! 那个 Unix-only 的 bin）
页面/守门/golden 专项全绿：pipeline_bytes 1 · zero_visible_change 3 · graph_server 9 ·
  static_texts 3 · red_lines 2 · req110_ttl 15
```

> 口径说明：本机 `cargo test` 的「目标」数含全部 `tests/*.rs` 与 bin 单测（72），与 REQ-112 报告里
> 「43 套件」不是同一口径，别直接比数字。

## 如实登记（AC-011）

1. **未做真浏览器手测**（本机无 headless 浏览器，也不许为此加依赖）；
2. **标签宽度是估算**（`length × 11`），没有 `getComputedTextLength()`；
3. **力导向参数是 Node 扫参得出的**（`REP=400` / `CENTER_F=0.4` ⇒ 83 节点收在 ~529×528），
   真实数据下的观感没有在浏览器里看过。

---

# 历史：REQ-112 审核界面接入 AI 建议（B-1 阻断修复轮）

> 以下为上一需求原文，保留备查。

---

# 开发状态 · REQ-113 排除表：从「脚本预处理」变成产品能力

> worktree `feature-REQ-113` · 分支 `feature/REQ-113` · 基线 `develop` `e9755e8` · **未 push**
> 日期：2026-10-22 · Windows · PG `postgresql://***@172.17.0.87:5432/anspire_real`（测试）/ `anspire_open_uat`（源库，只读）

- **需求**：REQ-113（AC-001 ~ AC-010）
- **当前任务**：全部实现完成，真实库实测 96 → 12 → 84，待 QA 审核
- **真实清单来源**：`exclusions.json`（= BA 提供的 `code/out/exclusions-ai.json`，`version 0.1.1`，
  `declared_by: ai-table-judge / qwen36-27B`，`reviewed_by: ba-michael（推翻 1 条、接受其余 12 条）`）

## 一、改动清单（`path:line`）

| # | 文件 | 关键位置 | 做什么 |
| --- | --- | --- | --- |
| 1 | `src/extract/exclusions.rs`（**新增**） | `:67` `Category`（7 类枚举）/ `:153` `Entry` / `:189` `Flagged` / `:305` `Exclusions::parse`（校验）/ `:490` `apply`（**唯一判定点**）/ `:503` `record`（**完整审计段**）/ `:623` `stale_warning` / `:654` `Record` + `Counts` / `:686` `from_inputs` / `:701` `validate`（含 counts 自查）/ `:771` `to_json` / `:887` `stamp_counts`（**闭合断言**） | 解析 / 校验 / 剔除 / 记录 / 断言，**一个实现**（纪律 H） |
| 2 | `src/bin/ontok-propose.rs` | `:165` `--exclude` · `:166` `--strict-exclusions` · `:217` 先校验文件 · `:271` 抽取前剔除 · `:294` 陈旧警告 · `:1011` `stamp_counts`（+ missing 表名） · `:1044` **审计段进提案** · `:1048` 台账打印 | CLI |
| 3 | `src/extract/propose.rs` | `:157` `Proposal.inputs` · `:243/246/253/256/261` 五个 stats 字段 · `:465` `tables_extracted` 在库路径落地 | 契约 |
| 4 | `src/agent/assemble.rs` | `:427` 重建路径也填 `tables_extracted` · `:453` `inputs: None` | 两条路径同口径 |
| 5 | `src/db/pg.rs` | `:369` `proposals.inputs JSONB` · `:424` `ALTER_PATCHES` 13→14 · `:532` 补列补丁 | 迁移 |
| 6 | `src/db/proposal.rs` | `:285` `set_inputs` · `:295` `inputs` | 存储 |
| 7 | `src/cli.rs` | `:1440-1481` **只读 `--file`**：校验审计段 + 落库（**无侧车、无第二文件**） | `propose import` |
| 8 | `src/review_ui/http.rs` | `:57` 模块头 · `:393` 读 `proposals.inputs` · `:432` `exclusions_block` · `:418` 响应加 `exclusions` | 只读 API |
| 9 | `src/review_ui/page.rs` | `:163` 「被排除」section · `:178` 「保留但存疑」section · `:395` `renderExcluded` | 只读区块 |
| 10 | `tests/test_req113_exclusions.rs`（**新增**，12 条用例） | AC-001/2/3/4/5/6/7/8 + 单文件导入 | 端到端判据 |
| 11 | `exclusions.json`（**新增**，仓库根） | 12 条排除 + 11 条 `keep_but_flagged` | 真实库清单（随代码入库） |

## 二、真实库实测（`anspire_open_uat`，96 表）

```
$ ontok-propose --schema <schema.json> --names src/extract/table_names.json \
    --out out/proposal.json --stats out/stats.json --project req113_demo \
    --exclude exclusions.json --src $SRC_URL
```

| 项 | 值 |
| --- | --- |
| `tables_seen` | **96** |
| `tables_excluded` | **12** |
| `tables_extracted` | **84** |
| 闭合 `seen − excluded == extracted` | **true**（断言在 `exclusions.rs:887`，不闭合 ⇒ 不写 stats、exit≠0） |
| `entities`（提案里 `add_entity_type`） | **84** |
| `stale_exclusions` | 0 |
| `flagged_tables` | 11（**未被排除**，已照常进本体） |
| **AC-001 ①** `items` 段里 grep 12 个被排除表名 | **0 次** |
| **AC-001 ②** 删掉 `inputs.exclusions` 整段后 grep | **0 次**（表名只允许出现在审计段内） |
| `inputs.exclusions.counts` | `{seen:96, excluded:12, extracted:84, stale:0, flagged:11}`（闭合） |
| `excluded_tables ∩ tables_without_cn_name` | **∅**（AC-007：排除优先） |

**12 条排除（逐条，AI 判定 + BA 复核）**

| 表 | 类别 | 理由 | 决策人 | 置信度 |
| --- | --- | --- | --- | --- |
| `_volc_dts_pg_ddl_18e4b12d…` | cdc_residue | 数据同步工具自动生成的临时 DDL 表，不对应任何客户业务 | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_8f7cc0a6…` | cdc_residue | 同上（同族第 2 张） | ai-table-judge | 0.97 |
| `_volc_dts_pg_ddl_9a38a0a4…` | cdc_residue | 同上（同族第 3 张） | ai-table-judge | 0.97 |
| `alembic_version` | migration_bookkeeping | 数据库迁移工具的版本号记账表，客户业务里没有对应事物 | ai-table-judge | 0.99 |
| `llm_provider_back` | backup_snapshot | llm_provider 的整表备份副本，列结构与主表完全一致 | ai-table-judge | 0.95 |
| `llm_provider_back_20260630` | backup_snapshot | 按日期命名的 llm_provider 备份副本，不是独立业务事物 | ai-table-judge | 0.96 |
| `llm_provider_back_20260701` | backup_snapshot | 同上 | ai-table-judge | 0.96 |
| `llm_auth_cache_change_outbox` | internal_pipeline | 权限缓存变更的内部发件箱，只给同步程序读写，客户不可见 | ai-table-judge | 0.94 |
| `llm_auth_cache_kafka_event_outbox` | internal_pipeline | 内部消息队列发件箱，记录待投递事件而非业务事实 | ai-table-judge | 0.94 |
| `llm_auth_cache_trigger_state` | internal_pipeline | 权限边界判断用的内部状态快照，本身不是业务对象 | ai-table-judge | 0.90 |
| `rpt_dim_customer` | report_intermediate | 报表客户维度表，由客户与渠道数据派生，客户业务里无此事物 | ai-table-judge | 0.82 |
| `rpt_dws_channel` | report_intermediate | 渠道汇总的报表中间结果，不是某一笔业务的发生记录 | ai-table-judge | 0.82 |

## 三、确定性（AC-008）

同一份 `schema.json` + 同一份 `exclusions.json`，连跑两次（不同 `--out` 目录）：

```
proposal.json  sha256  run1 = 852281ec94f7acb47c429829d44c93cb41b065c7927bc67c1b8ee55f9485ab66
                       run2 = 852281ec…（相同；明细进提案后文件变大，sha 因此与上一轮不同）
stats.json     sha256  run1 = 9c5688451d61f72e9f2c2dec4c684e966466f5a03470f7fc0f0b77d174dcf829
                       run2 = 9c568845…（相同）
git diff --no-index out/proposal.json out2/proposal.json   → exit 0（无输出）
git diff --no-index out/stats.json    out2/stats.json      → exit 0（无输出）
```

> `--out` 目录里**只有** proposal.json / stats.json（+ REQ-108 既有的 pending / rejected）——
> 侧车 `proposal-exclusions.json` 已删除，实测 `Test-Path out/proposal-exclusions.json = False`。

## 四、陈旧排除两种模式（AC-005，真实库 + 一条不存在的表）

| 模式 | 结果 |
| --- | --- |
| 默认 | **exit 0**；stderr 点名 + `stats.stale_exclusions = ["_volc_dts_pg_ddl_0000…"]`；`tables_excluded` 仍是 12（陈旧的**不算**排除） |
| `--strict-exclusions` | **exit 1**，点名该表并说明两种可能；**不产出** proposal / stats |

## 五、审核界面（AC-006）

```
GET /api/review （Bearer 口令）
exclusions: present=True readonly=True count=12 flagged_count=11
  counts={seen:96, excluded:12, extracted:84, stale:0, flagged:11}
  file=exclusions.json  version=0.1.1
  sha256=3f77c6b8fd41bded7e5fbe74b507c74a2a385cfc67cccef2b90cada5dacc2c08
  declared_by=ai-table-judge / qwen36-27B
  reviewed_by=ba-michael（人工复核：推翻 1 条排除、接受其余 12 条）
tables[0] = {table:"_volc_dts_pg_ddl_18e4…", category:"cdc_residue",
             category_label:"CDC / 数据同步残留", reason:"数据同步工具自动生成的临时 DDL 表…",
             decided_by:"ai-table-judge", confidence:0.97, evidence:"表名为 _volc_dts_pg_ddl_ + 32 位哈希…"}
flagged[0] = {table:"internal_staff", why:"名字像内部表，但只有 5 列且无任何列注释…", concern:"若只是后台账号…"}
```

页面两块（`page.rs:163` / `:178`）：

```html
<section id="excluded">被排除 <b id="x-count">12</b> 张
  <span class="x-note">（只读 · 改排除要改 exclusions.json 重跑，页面上不能编辑）</span>
  …表 / 类别 / 理由 / 决策人 / 置信度（+ 依据）
</section>
<section id="flagged">保留但存疑 <b id="f-count">11</b> 张
  <span class="x-note">（**没有被排除** · 已照常进本体 · 待裁决）</span>
  …表 / 为什么拿不准 / 担心什么
</section>
```

## 六、测试

见 `verification-report.md`（本轮全量运行结果与两处绕行证明）。

## 七、已知限制（如实登记）

1. **审计段里含表名**（`inputs.exclusions.excluded[]`）——这是 BA 裁定的「单一文件」：
   `items` 段里 grep = 0，且**整份文件里这些表名只允许出现在 `inputs.exclusions` 段内**
   （实测：删掉该段后 grep = 0）。所以提案会大 ~4 KB，换来「一个文件 = 一条完整审计链」。
2. **`keep_but_flagged` 只展示、不参与判定**：这些表照常进本体（`tables_extracted` 里有它们），
   界面单独一块列出。要裁决它们，得改 `exclusions.json`（加进 `exclude`）重跑。
3. **排除的粒度是表级**（R1）：列级「属性剔除」仍是 `property_filter` 的事，两者不混。
4. **`tables_extracted` 由实体条目数实测**：重建路径若因分组失败少抽了表，闭合断言会让这一轮**失败**
   （不写 stats），报错里**点名哪几张表没产出实体**。这是 R5 的本意，但属于**行为变更**
   （以前只是安静地写出偏小的 items）。
5. **平台**：`src/bin/ontok-gate-preflight.rs` 是 Unix-only，Windows 上必编不过 —— 全量测试只能绕行
   （挪开 → 跑 → 挪回，见 `verification-report.md` §2），代价是 `test_retained_names_req084` 编不过（既有红灯，REQ-112 已登记）。
6. **`drop_rate` / `test_fk_naming` 两个门禁在本机为红**：它们要一份**含中文名的脱敏 schema**，
   本机只有 `code/out/schema.json`（96 张真实表名、翻译覆盖 0）⇒ 丢弃率 69.8%。**与本次改动无关**
   （两个门禁只调 `extract()` / `project()`，本轮一个字节都没改；基线 `e9755e8` 上逐字复现）。
