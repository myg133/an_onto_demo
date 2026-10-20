# 测试用例：REQ-107 · 出口收口

> **性质**：本文件**只设计用例**，不含实现、不改 `feature-REQ-107/` 任何源码。
>
> **被测基线**：`feature/REQ-107` @ `9814199`（源码与 `develop` `b714f48` **逐字节相同** —— 已核：`git diff --numstat develop -- src tests` 输出为空，唯一的差异提交 `9814199` 只改 `.feature/manifest.json`）。
> ⇒ 下文所有 `path:line` 都是 `b714f48` 的行号，可直接对照。
>
> **需求**：`BA/demands/REQ-107/demand.md` · **验收标准**：`BA/demands/REQ-107/acceptance.md`（AC-001 ~ AC-010，逐条覆盖）
>
> **用例总数**：**29 条**（P0 = 17 / P1 = 8 / P2 = 4）

---

## 零 · 运行口径与本机限制（先把「跑不动」说清楚）

| 项 | 事实 | 对用例的影响 |
| --- | --- | --- |
| `cargo check --lib` | ✅ 可跑（本机 0.93s） | 用例里凡是「编不过即红」的，只依赖 lib |
| `cargo test`（全量） | ❌ **本机编不过**：`src/bin/ontok-gate-preflight.rs` 用了 `std::os::unix::fs::MetadataExt` / `symlink`（`587/588/1514/1544` 行）⇒ Windows 上**不成立** | ⛔ 任何用例**都不许**写「全量 62 suite / 846 passed 必须复现」这种本机无法执行的判据 |
| `cargo test --test <name>` | ✅ 可跑（只编 lib + 该测试目标） | **所有用例的运行命令统一写成 `cargo test --test <建议文件名>`** |
| `cargo test --doc` | ✅ 可跑（只编 lib） | TC-008 的「类型层」判据落在 doctest 上，正因为这一点才可执行 |
| 需要 DB 的用例 | 需 `ONTOK_TEST_DATABASE_URL` | 已在用例里逐条标注「需要 DB」，并声明**本机不可复核** |
| 全量数字 | 基线 `62 suite / 846 passed` 只在 Linux 成立（纪律 O 记 `@ develop 7f26fa2`，AC 页首记 `@ b714f48`，**两处 commit 标注不一致**） | 见 TC-028：只要求「报数字必须带口径 + 显式声明本机不可复核」，不要求本机复现数字 |

### 「现行状态」的判定方法

逐条**读代码**得出，给出 `path:line`。判定口径：

- **红** = 该用例写成 `#[test]` 后，在 `9814199`/`b714f48` 的源码上**会失败**；
- **绿** = 会通过（并注明它今天**是否真有判别力** —— 本仓栽过太多次「绿但空转」）；
- **不可执行** = 本机/本环境跑不了，或前置用例还没落地（写清是哪一个）。

---

## 一 · AC-001 · 唯一的客户类出口管线

### TC-001 · 出口文件里对判据的调用点必须为 0（否定式）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-001` |
| 关联 AC | AC-001 |
| 标题 | `src/cli.rs` / `src/server/http.rs` / `src/mcp/serve.rs` 里**不得出现**任何判据/剔除/清洗的直接调用 —— 它们只能出现在管线模块内部 |
| 前置条件 | 纯源码扫描，**不需要 DB**。走 `tests/common::read_source`（`tests/common/mod.rs:16`）。建议文件 `tests/test_req107_outlet_pipeline.rs`，函数 `test_req107_outlet_files_have_zero_criterion_call_sites` |
| 测试步骤 | 1) 先剥注释（否则 `cli.rs:1317-1319` 这类注释里提到守门器会**假红**）—— 复用 `tests/test_binary_output_guard.rs:91` 的 `strip_comments` 算法（建议提到 `tests/common`）；<br>2) 定义 `OUTLET_FILES = ["src/cli.rs","src/server/http.rs","src/mcp/serve.rs"]`；<br>3) 定义 `CRITERION_CALLS = ["scan_modeling_terms(","guard(","guarded(","filter_view(","has_raw_identifier(","sanitize_display("]`；<br>4) 逐文件 `assert_eq!(criterion_sites(&src), 0, "{file} 里还有 {n} 处判据直调：{明细}")`；<br>5) 命令：`cd feature-REQ-107; cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 三个文件各自的命中数**全部等于 0**；报错信息必须列出 `path:line` 与命中的函数名 |
| 优先级 | **P0** |
| 现行状态 | **红**。实测调用点：`src/cli.rs:1253`（`scan_modeling_terms(&page.html)`）、`src/cli.rs:1313`（`scan_modeling_terms(&json)`）、`src/cli.rs:1349`、`src/cli.rs:1351`（overview 两条）、`src/cli.rs:1295`（`property_filter::filter_view`）；`src/server/http.rs:182`（`guard`，经 `guarded` 在 `:239`/`:260` 使用）、`src/server/http.rs:426`（`filter_view`）；`src/mcp/serve.rs:291`（`guard`）、`src/mcp/serve.rs:101`（`filter_view`）。全仓**不存在**任何管线模块（`src/` 下无 `pipeline`/`outlet` 相关文件） |

> ⚠️ **必须提前告知 Dev 的坑**：本条一旦落地，既有测试 `tests/test_property_filter.rs:341` `all_three_outlets_call_the_one_shared_function`（断言这**三个文件**里 `contains("property_filter::filter_view")`）**必然转红**。
> 这不是「放宽断言」能解决的，必须把它**同步改写成同一套否定式判据**（AC-009 允许「改紧」，不允许删/放宽）。⛔ 若 Dev 直接删掉它，AC-009 的「不许删除测试」当场违反。

### TC-002 · 出口层只声明「格式 + 轨道」，不许自带开关

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-002` |
| 关联 AC | AC-001 |
| 标题 | 管线入口的签名里**不得**出现「要不要过闸 / 要不要剔除」这类布尔开关；出口层不得再自行 `if … { exit(2) }` |
| 前置条件 | 纯源码扫描（不需 DB）。同 TC-001 的文件，函数 `test_req107_outlet_only_declares_format_and_track` |
| 测试步骤 | 1) 定位管线入口（Dev 交付的映射表给出 `path:line`），断言其函数签名里**不含** `skip_guard` / `bypass` / `no_guard` / `escape_guard` / `guard: bool` / `filter: bool` 任一子串；<br>2) 断言**出口函数体内**（`fn_span` 定位：`ontology_cmd` / `get_graph` / `get_node` / `index` / `dispatch`）不含 `EXIT_LEAKED_MODELING_TERMS`、不含 `std::process::exit(`、不含 `if !leaked.is_empty()`（判红决策必须在管线内）。<br>⚠️ 判据**必须按函数体**而不是整文件：`EXIT_LEAKED_MODELING_TERMS` 的常量定义本来就在 `src/cli.rs:355`、政策文案在 `:370`，按整文件判会**恒红**（纪律 E：恒红的判据等于没有）；<br>3) 断言管线入口的**实参**里出现「轨道」与「格式」—— 允许 Dev 用枚举/常量名，判据取「实参里至少有 2 个来自管线模块导出的枚举值」（若 Dev 用别的形状，在交付物里说明并同步改本条，⛔ 不许只删断言） |
| 预期结果 | 三步全绿；任一步命中即报出 `path:line` |
| 优先级 | P1 |
| 现行状态 | **红**。`src/cli.rs:1314-1323`（`if !leaked.is_empty() { exit(2) }`）、`src/cli.rs:1254-1257`（HTML 分支同款）、`src/cli.rs:1352-1358`（overview 同款）、`src/server/http.rs:184-189`（`guarded` 自己决定拒发）—— **4 处**出口自决；`EXIT_LEAKED_MODELING_TERMS` 在 `src/cli.rs` 共 5 处：常量定义 `:355`、政策文案 `:370`、**出口处调用 3 处**（`:1256` / `:1322` / `:1357`） |

### TC-003 · 不许抄第二份判据（纪律 H）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-003` |
| 关联 AC | AC-001（「⛔ 不许把 `guard()` 的内部实现抄一份」） |
| 标题 | 全仓 `fn guard` / `fn scan_modeling_terms` / `fn has_raw_identifier` 的**实现定义**各只有一处；任何委托式同名函数，函数体必须逐字等于那一句委托 |
| 前置条件 | 纯源码扫描（不需 DB）。同文件，函数 `test_req107_no_second_criterion_implementation` |
| 测试步骤 | 1) 全仓扫 `fn guard(`：定义数必须 == 1，且位于 `src/mcp/tools.rs`；<br>2) 全仓扫 `fn scan_modeling_terms(`：定义数 == 1，且位于 `src/domain/view.rs`；<br>3) 全仓扫 `fn has_raw_identifier(`：允许 2 处，但**非** `src/domain/identifier_shape.rs` 的那一处的函数体必须**逐字**（去空白后）等于 `identifier_shape::has_raw_identifier(s)`；<br>4) 断言 `src/mcp/tools.rs` 里 `fn safe_error(` 也只有 1 处；<br>5) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 三处定义数分别为 1 / 1 / ≤2 且委托体逐字相等；多出任何一份**带逻辑的**实现即红（报 `path:line` + 该函数体的前 120 字） |
| 优先级 | **P0** |
| 现行状态 | **绿**（今天是守得住的）。实测：`src/mcp/tools.rs:693` `pub fn guard`；`src/domain/view.rs:1115` `pub fn scan_modeling_terms`；`src/domain/identifier_shape.rs:212` `pub fn has_raw_identifier`；`src/domain/html_view.rs:202` 是同名委托（`:203` 逐字 `identifier_shape::has_raw_identifier(s)`）；`src/mcp/tools.rs:719` `pub fn safe_error`。**判别力说明**：本条今天绿 ≠ 空转 —— 收口时最容易发生的正是「在管线里顺手写一份判据」，那时它会红 |

---

## 二 · AC-002 · 13 个客户类出口全部接线（逐条可核对）

> 三条用例的共同前提：Dev 必须在代码里提供一份**机读的出口登记表**（建议 `src/domain/pipeline.rs` 里的 `pub const OUTLETS: &[Outlet]`），每条带
> `{ id: "C1", path: "src/cli.rs", call_site: "<该出口调用管线的源码片段唯一子串>", track: Track::Customer, format: Format::Html }`。
> 判据形状沿用本仓既有风格（`tests/test_property_filter.rs:348` 的 `src.contains(needle)`）：**登记表 needle 必须在 `read_source(path)` 里命中**。
> ⚠️ 已知边界（⛔ 不许写成「已封堵」）：needle 式判据抓得住「登记与源码漂移」，抓不住「语义等价改写」—— 那由 TC-008/009/011 从另外三个方向补。

### TC-004 · CLI 5 条客户类出口逐条接线

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-004` |
| 关联 AC | AC-002（C1~C5） |
| 标题 | `show --html` / `show --html --out` / `show`(JSON) / `overview` / `overview --json` 五条各有一个可核对的管线调用点 |
| 前置条件 | 需登记表（见本节前提）；**不需要 DB**（只读源码）。文件 `tests/test_req107_outlet_pipeline.rs`，函数 `test_req107_cli_five_customer_outlets_are_wired` |
| 测试步骤 | 1) 取 `OUTLETS` 里 `id in {C1..C5}` 的 5 条，逐条 `assert!(read_source(e.path).contains(e.call_site), "{id} 的管线调用点 {call_site:?} 在 {} 里找不到")`；<br>2) 断言 C1 与 C2 **共用同一份产出**：`src/cli.rs` 里 `std::fs::write(p, &page.html)`（现 `:1269`）与 stdout 分支（现 `:1281`）引用**同一个**变量名 —— 用词元级比较（照 `tests/test_binary_output_guard.rs:303` 的 `mentions_path`）；<br>3) 断言 `C1..C5` 的 `track == Customer`、`format` 分别 `Html/Html/Json/Text/Json`；<br>4) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 5 条全部命中；C1/C2 的写出表达式词元相同；track/format 逐条相符 |
| 优先级 | **P0** |
| 现行状态 | **红**。登记表不存在；且今天 5 条各自为政：`src/cli.rs:1269`（--out 写文件）、`:1281`（stdout）、`:1326`（JSON）、`:1361`/`:1363`（overview 两条），判据分别在 `:1253`/`:1313`/`:1349`/`:1351` —— **没有一处**是「调管线」 |

### TC-005 · HTTP 3 条数据出口逐条接线

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-005` |
| 关联 AC | AC-002（H1~H3） |
| 标题 | `/`（首页）、`/api/graph`、`/api/node` 三条都声明格式、都不自己决定过闸 |
| 前置条件 | 需登记表；需 `tower`（dev-dependency 已有，`Cargo.toml:58`）做行为半；**不需要 DB**（`state_from_view` 是**生产接线**且可在进程内直接调，见 `src/server/http.rs:410`）。文件 `tests/test_req107_outlet_pipeline.rs`，函数 `test_req107_http_three_outlets_are_wired` |
| 测试步骤 | 1) 登记表 `id in {H1,H2,H3}` 三条 needle 命中；<br>2) 行为半（进程内）：`let st = state_from_view(&fixture_cv(), "样例项目", 2, "correct-horse-battery");` 然后 `build_router(st).oneshot(...)` 分别打 `/`、`/api/graph?project=样例项目&version=2`、`/api/node?...&id=<第一个节点 key>`，三条都必须 `200`；<br>3) 反向半：fixture 换成**含建模术语**的同构视图（对象名「主键台账」）后，三条都必须**非 200**；<br>4) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 结构半 3 条命中；行为半 3×200；反向半 3×非 200。任一出口「干净也能过、脏也能过」即红 |
| 优先级 | **P0** |
| 现行状态 | **红**（结构半）。行为半今天其实**绿**（`src/server/http.rs:239`/`:260` 都经 `guarded`），但因为「不是经管线」而红。登记点：`src/server/http.rs:234`/`:242`/`:283` |

### TC-006 · MCP 5 个工具出口统一在一个调用点过闸

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-006` |
| 关联 AC | AC-002（M1~M5） |
| 标题 | 五个工具的返回值全部经由**同一处**管线调用写出（不是每个工具各自处理），且未过闸的值不可能到达客户端 |
| 前置条件 | 需登记表；**不需要 DB** —— `OntologyContext` 的 7 个字段全 `pub`（`src/mcp/tools.rs:132-142`），可直接构造；`handle_line` 是 `pub`（`src/mcp/serve.rs:214`）。文件 `tests/test_req107_outlet_pipeline.rs`，函数 `test_req107_mcp_five_tools_share_one_pipeline_call` |
| 测试步骤 | 1) 断言 `src/mcp/serve.rs` 的 `"tools/call"` 分支里，`tools::call(...)` 的结果**只**在**一个**位置被写成响应（今天就是 `p::encode_result` 的两处：`:292` 成功、`:296`/`:311` 错误）；判据：`scan_modeling_terms(`/`guard(` 在该分支内命中数 == 0，且管线调用点命中数 ≥ 1；<br>2) 行为半（无 DB）：构造 `OntologyContext`（`view` 放含「主键」的脏对象、`overview` 用 `build_overview` 现算），对 5 个工具各发一条 `tools/call`，断言**5 条响应**都形如 `{"result":{"isError":true,...}}` 且**不含**「主键」；<br>3) 对照组：`view` 换成干净 fixture，5 条都必须 `isError == false` 且含数据；<br>4) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 结构半：出口文件命中 0（含 `src/mcp/serve.rs`）；行为半：脏 5 条全拒、干净 5 条全过。⛔ 「5 条全拒」不够 —— 没有对照组的全拒可能是「所有调用都坏了」（纪律 E） |
| 优先级 | **P0** |
| 现行状态 | **红**（结构半）。今天 `src/mcp/serve.rs:291` 直接调 `tools::guard` —— 单点，但**不是**管线；行为半今天绿（`:291-298` 对每个 `Ok` 值无条件过闸）。M1~M5 的定义在 `src/mcp/tools.rs:406`/`:414`/`:422`/`:432`/`:451` |

### TC-007 · 出口登记表与 33 条口径对账（双向）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-007` |
| 关联 AC | AC-002 + AC-010-1 |
| 标题 | `OUTLETS` 的条数与分类必须**恰好**是 33 = CLI 22（客户 5 + 建模 8 + 内部 9）+ HTTP 4（H1,H2,H3,Z1）+ MCP 7（M1~M5,Z2,Z3） |
| 前置条件 | 需登记表；不需 DB。同文件，函数 `test_req107_outlet_registry_matches_the_33_baseline` |
| 测试步骤 | 1) `assert_eq!(OUTLETS.len(), 33)`；<br>2) 按 `track` 分组计数断言：`Customer == 13`（C1..C5 + H1..H3 + M1..M5）、`Modeling == 8`（D1..D8）、`Internal == 9`（`concept import`/`concept digest`/`project new`/`project list`/`version new`/`version list`/`version freeze`/`diff 无差异`/`diff 有差异`）、`Static == 3`（Z2,Z3,Z4）、`ErrorPath == 1`（Z1）；<br>3) 断言每条 `id` 唯一、每条都有 `path` 与 `call_site`、且 `id` 集合与 AC 里的编号集合**双向**相等（多一条/少一条都红）；<br>4) 断言 `src/cli.rs:486` 的 `DOMAIN_PRINT_SITES`（8 条）**不是**本表的替代品 —— 两者条数不同必须被显式记录（防 Dev 拿旧表冒名顶替）；<br>5) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 33 条，分组 13/8/9/3/1 逐项相等；任何「改数字让它变绿」都必须先改本用例并在 AC-010-9 提出异议 |
| 优先级 | **P0** |
| 现行状态 | **红**。登记表不存在；现有唯一相关的表是 `src/cli.rs:486-626` 的 `DOMAIN_PRINT_SITES`，**只有 8 条**，与 33 差 25（`BA/demands/REQ-107/status.md:32` 已登记：G-5 反向守卫「放过 14 个出口」） |

---

## 三 · AC-003 · 漏接即红（本需求的核心）

> 本节 5 条用例按 AC 允许/要求的**三条实现路径**分开：类型层（TC-008）、源码否定式（TC-009/TC-010）、变异（TC-011/TC-012）。
> ⛔ 其中 **TC-010 专打「保留共享调用、另加一份本地判据」** —— `src/domain/property_filter.rs:129-142` 记录了 QA 变异 **M3** 已实证这种形态下 9 条既有测试全绿。

### TC-008 · 类型层：客户类输出类型在管线外**构造不出来**（`compile_fail` doctest）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-008` |
| 关联 AC | AC-003（「类型层：客户类渲染只能返回一个只能由管线构造的包装类型（构造器私有）」） |
| 标题 | 管线模块的模块级文档里有一条 `compile_fail` doctest，证明「不经过管线入口就拿不到可写出的客户类产物」；且该 doctest 在全仓**确实存在**（防它被删掉） |
| 前置条件 | 需 Dev 实现包装类型（建议 `pub struct CustomerRender`，字段全私有、无 `pub fn new`）。**不需要 DB**，`cargo test --doc` 只编 lib（本机可跑）。建议文件 `tests/test_req107_outlet_pipeline.rs`，函数 `test_req107_type_layer_blocks_off_pipeline_construction` |
| 测试步骤 | 1) **运行时判据（可机判的半边）**：全仓扫源码，断言存在**至少 1 条** `compile_fail` doctest，且它位于非 `tests/` 文件、其代码块里出现包装类型名与一个「字面量构造」；<br>2) **编译期判据（真正的类型层）**：`cargo test --doc` 必须绿 —— 若哪天有人把构造器改回 `pub`，那条 `compile_fail` 会**编译成功** ⇒ doctest 红；<br>3) 交叉断言：`read_source(管线模块)` 里包装类型**不得**出现 `pub` 字段声明（`pub html:` 这类）与 `pub fn new(`；<br>4) 命令 `cd feature-REQ-107; cargo test --doc`（⚠️ 若 Dev 用 `#[doc(hidden)]` 等手法规避，第 1 步的「代码块里出现包装类型名」会红） |
| 预期结果 | ① 全仓 `compile_fail` doctest 计数 ≥ 1（当前 = 0）；② `cargo test --doc` 绿；③ 包装类型无 `pub` 字段/构造器 |
| 优先级 | **P0** |
| 现行状态 | **红**。全仓**零** `compile_fail` doctest（`grep -c "" ` 见下），也不存在任何包装类型 —— 客户类渲染产物今天就是裸 `String`：`src/cli.rs:1301`（`let json = serde_json::to_string_pretty(...)`）、`:1238`（`page: RenderedPage`）、`src/server/http.rs:179`（`fn guarded<T: Serialize>`）。<br>⛔ **不许用 `trybuild` 交差**：那要加 dev-dependency，`Cargo.toml:52-58` 没有它，且 AC 未授权新增依赖 |

> 备选方案（更弱，Dev 若选必须明说）：把「构造器私有」写成纯源码断言（无 doctest）。那样**只是拼写级**，AC-003 说的「类型层」不成立 —— 本用例按 `compile_fail` 写，是因为它在本机**可执行**（`cargo test --doc` 不编译 `src/bin/`）。

### TC-009 · 源码否定式：除管线外全仓无第二处判据直调

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-009` |
| 关联 AC | AC-003（「源码层：枚举全部客户类写出点，断言每个都经过管线（**否定式断言**）」） |
| 标题 | 判据调用点只允许出现在管线/守门器模块内；出口文件命中数 == 0（与 TC-001 同源，但本条**额外**要求「全仓口径」而不是只看三个文件） |
| 前置条件 | 纯源码扫描，不需 DB。同文件，函数 `test_req107_no_second_judgement_outside_pipeline` |
| 测试步骤 | 1) 剥注释后，全仓扫 `scan_modeling_terms(` 的**调用点**（定义处除外），逐条要求位于「管线模块 ∪ `src/mcp/tools.rs`（`guard` 本体）∪ `tests/`」；<br>2) 同样扫 `filter_view(`，只允许在管线模块内；<br>3) **P1 附加项（可裁定）**：`safe_error(` 是否也收口 —— 若 AC-004/AC-001 裁定「错误文案闸也归管线」，则把它并入第 1 步的列表；**今天不并入**，理由写在用例里（否则会逼 Dev 做超范围的搬迁）；<br>4) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | 第 1/2 步：越界调用点 == 0，报错逐个给 `path:line` + 命中串；第 3 步：不参与判定，只在报错文本里给出当前计数（便于 Dev 裁定） |
| 优先级 | **P0** |
| 现行状态 | **红**。今天越界点：`src/cli.rs:1253/1313/1349/1351`（`scan_modeling_terms` 4 处）、`src/cli.rs:1295`、`src/server/http.rs:426`、`src/mcp/serve.rs:101`（`filter_view` 3 处）。另有合法点：`src/mcp/tools.rs:696/720/742`（判据模块内）、`src/server/page.rs:300`（首页过闸，属 Z4 口径待裁定，见 TC-025） |

### TC-010 · ⭐ M3 专项：**保留共享调用 + 另加一份本地判据** 必须红

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-010` |
| 关联 AC | AC-003（「⛔ 不许只写正向判据」+ `src/domain/property_filter.rs:129-142` 登记的 M3 盲区） |
| 标题 | 判据必须能抓住「三处出口都调了共享函数」+「其中一处**另加**一份真在生效的本地判据」这种形态 —— 这正是 QA 变异 M3 实测全绿的那一格 |
| 前置条件 | 纯源码扫描 + 一个纯函数自证，不需 DB。同文件，函数 `test_req107_m3_keeps_shared_call_and_adds_local_criterion` |
| 测试步骤 | 1) 把计数逻辑抽成**纯函数** `fn criterion_sites(src: &str) -> Vec<(usize, &'static str)>`（可被合成输入直接打，照 `tests/test_binary_output_guard.rs:813` 的 G-4 模式）；<br>2) **自证**（先证明判据能红）：喂一段与今天 `src/cli.rs` 同形的合成串 —— <br>`property_filter::filter_view(&cv);` + `let leaked = view::scan_modeling_terms(&json);` + `if !leaked.is_empty() { std::process::exit(2); }`<br>断言 `criterion_sites(合成串).len() == 2` 且**> 0**；<br>3) **实物**：对 `OUTLET_FILES` 三个文件逐个断言 `criterion_sites(...).is_empty()`；<br>4) **反向自证**（防止判据被改成恒红）：喂一段只有管线调用的合成串 `pipeline::render(OutletSpec::C3, &cv)`，断言计数 == 0；<br>5) 命令 `cargo test --test test_req107_outlet_pipeline` |
| 预期结果 | ② `== 2`、③ 全 0、④ `== 0`。三步缺一不可：只做 ③ 的判据可能是恒红/恒绿（纪律 E） |
| 优先级 | **P0** |
| 现行状态 | **红**。② 自证今天没有；③ 实物命中 6 处（同 TC-001 的 cli 4 + `filter_view` 3，去重后按文件计：`src/cli.rs` 5、`src/server/http.rs` 2、`src/mcp/serve.rs` 2）。**这条用例存在的理由**：`src/domain/property_filter.rs:136` 原文记着「在 `cli.rs` 保留共享调用、另加一份本地判据 ⇒ **9 条全绿**」 |

> ⚠️ **本条自身的天花板（必须照实登记，⛔ 不许写成「已封堵」）**：若「本地判据」是一个**全新命名的自造函数**（`fn local_ok(s:&str)->bool { s.contains("key") }`），第 2/3 步的数调用点**抓不到**（它没调那 6 个名字）。
> 补强在 TC-011 的行为变异那一侧；真正结构化的解法是 TC-008 的类型层。⇒ 三条合起来才勉强够 AC-003 的「至少一种结构性判据」。

### TC-011 · 变异 M-1：把 C3（`show` JSON）从管线摘掉 ⇒ 必须红

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-011` |
| 关联 AC | AC-003（「**变异验证**（纪律 G）：把某个出口从管线摘掉，判据必须红，并贴三元组」） |
| 标题 | 手工变异：`ontology show` 的 JSON 出口改回「不经管线、直接 `emit_domain_line(&json)`」 ⇒ 至少 TC-004 与 TC-001 转红 |
| 前置条件 | 前置：TC-001/TC-004 **先绿**（否则「从绿变红」无从观察）；需重新编译该测试目标；不需 DB |
| 测试步骤 | 1) `git -C feature-REQ-107 diff --stat` 记录改动前状态；<br>2) 在 `src/cli.rs` 的 JSON 出口处把管线调用改回 `crate::cli::emit_domain_line(&json);`（**只改这一处**，其余不动）；<br>3) `cargo test --test test_req107_outlet_pipeline` 跑一次，记录 `passed/failed` 与失败测试名；<br>4) 取三元组：`Get-FileHash target\debug\deps\test_req107_outlet_pipeline-*.exe -Algorithm SHA256`（Linux：`sha256sum`）、变异 patch 的 `git diff > m1.patch` + `sha256sum m1.patch`、逐条结果；<br>5) `git checkout -- src/cli.rs` 复原；<br>6) 临时文件只放 `.review-workspace/req107/` |
| 预期结果 | **TC-004 红**（C3 的 needle 找不到）+ **TC-001 红**（`src/cli.rs` 命中数从 0 变 1）。⛔ 「编译失败」不算执行：若变异导致编不过，必须换一个能编译的变异重做（纪律 G） |
| 优先级 | **P0** |
| 现行状态 | **红 —— 但读法要小心**：今天 TC-004/TC-001 本来就已经红（没有管线），所以**无法**在这条上观察到「从绿到红」。⇒ 本用例的现行状态应记为「**不成立（前置红）**」，必须在 TC-001/004 转绿之后才执行。这正是纪律 G 说的「编译失败/前置不成立不算证据」 |

### TC-012 · 变异 M-2：**新增**一个客户类出口但没接管线 ⇒ 必须红

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-012` |
| 关联 AC | AC-003（「新增一个客户类出口而没接管线时，测试立刻红」） |
| 标题 | 手工变异：在 `src/cli.rs` 里加一条 `println!("{}", serde_json::to_string_pretty(&cv)?)` 冒充新出口 ⇒ 登记表对账（TC-007）与出口枚举必须红 |
| 前置条件 | 前置：TC-007 先绿；需重编译。⚠️ 这条变异**不带任何判据调用**，所以 TC-001/TC-010 抓不到它 —— 它只能由「**写出点枚举 + 登记表双向对账**」抓住 |
| 测试步骤 | 1) 在 `ontology_cmd` 的 Show 分支尾部加一行「新出口」`println!`（内容取 `cv` 的序列化）；<br>2) 跑 `cargo test --test test_req107_outlet_pipeline`；<br>3) 期望红在 **TC-007**（`DOMAIN_PRINT_SITES` 式对账：出现一个未登记的 domain 写出点/未登记的出口）；<br>4) 再跑一次 `cargo test --test test_domain_print_sites_binary_req078` 观察既有 G-5 是否也红（它可能**不红** —— `src/cli.rs:486-626` 只登记 8 条，见 `status.md:32`）；<br>5) 贴三元组，复原 |
| 预期结果 | 至少 TC-007 红，报错指出「新增写出点未登记」或「客户类出口未经管线」；若**两次运行全绿** ⇒ 本需求的核心承诺（漏接即红）**未达成**，必须回来补判据而不是删用例 |
| 优先级 | P1 |
| 现行状态 | **红 —— 同样读作「不成立（前置红）」**：TC-007 今天红（无登记表），且既有 G-5 对新增出口**默认放过**（`status.md:32` 原文：「G-5 反向守卫有 owner 白名单 +『实参提到生产者』判据，**放过 14 个出口**」）|

---

## 四 · AC-004 · 三个卡点逐个裁定（行为差异可观察）

> 本节的写法：**不给裁定**（裁定是 Dev + BA 的事），只把「选 A 得什么 / 选 B 得什么」变成可观察的断言。
> 三条用例**今天都是绿的** —— 它们的价值不是抓 bug，而是让「静默改口径」当场变红，逼 Dev 在 AC-005 变化清单里登记。

### TC-013 · 卡点① 改名口径：HTML 显示「客户编号」，JSON 显示「客户编号ID」

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-013` |
| 关联 AC | AC-004 卡点① |
| 标题 | 同一份 cv：HTML 出口经 `sanitize_display` 摘掉 `ID` 记号，JSON 出口只过 `filter_view`（**不改名**）—— 两个出口对同一属性给出两个名字 |
| 前置条件 | 不需 DB。fixture：`CustomerView { objects: [EntityView{ name:"样例客户", properties: [PropertyView{ name:"客户编号ID", .. }] }], relations: [] }`（合成名，⛔ 不含真实客户表名/列名）。文件 `tests/test_req107_guard_granularity.rs`，函数 `test_req107_cardpoint1_rename_divergence_between_html_and_json` |
| 测试步骤 | 1) `let page = render_html(&cv, &[]);`（`src/domain/html_view.rs:313`）<br>2) `let json = serde_json::to_string_pretty(&filter_view(&cv).view)?;`（`src/domain/property_filter.rs:280`，等价于 `src/cli.rs:1295-1301`）<br>3) 断言 `assert!(!page.html.contains("客户编号ID"))` 且 `assert!(page.html.contains("客户编号"))`；<br>4) 断言 `assert!(json.contains("客户编号ID"))` —— **这一条就是「JSON 不改名」的可观察证据**；<br>5) 断言两者**不可能同时**满足「都改名」或「都不改名」：把两句写成 `assert_ne!(html_renamed, json_renamed)`，`html_renamed = !page.html.contains("客户编号ID")`、`json_renamed = !json.contains("客户编号ID")`；<br>6) 命令 `cargo test --test test_req107_guard_granularity` |
| 预期结果 | `html_renamed == true`、`json_renamed == false`、且 `assert_ne!` 通过。<br>**裁定后**：<br>· 选「JSON 也改名」⇒ 第 4 步变红，Dev 必须把它改成 `assert!(!json.contains("客户编号ID"))`，并在 AC-005 变化清单登记「C3 出口的属性名从 `XID` 变 `X`」，同步更新 TC-016 的 golden；<br>· 选「JSON 保持原名」⇒ 本条原样绿，无需改 |
| 优先级 | **P0** |
| 现行状态 | **绿**（现状 = JSON 不改名）。依据：`src/domain/html_view.rs:275-284`（`sanitize_display` = `name_is_forbidden` ∪ `has_raw_identifier` + `strip_id_token`）、`src/domain/html_view.rs:214-258`（`strip_id_token` 摘 ASCII `id` 记号）；`src/cli.rs:1295-1301` 只调 `filter_view`，而 `filter_view`（`src/domain/property_filter.rs:242-273`）**只剔除、不改名**；既有测试 `tests/test_property_filter.rs:313-332` 明确把「客户ID」列为**不许被剔**的存活项，反向印证它到 JSON 时仍是「客户ID」 |

### TC-014 · 卡点② 闸粒度：`/api/graph` 整份拒，`/api/node` 只拒那一个节点

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-014` |
| 关联 AC | AC-004 卡点②（+ `src/domain/property_filter.rs:90-107` 登记的「零测试覆盖」） |
| 标题 | 同一份视图、只有一个对象脏：`/api/graph` → 500（整份），干净对象 `/api/node` → 200（逐节点），脏对象 `/api/node` → 500。**三个断言缺一不可** |
| 前置条件 | 不需 DB（`state_from_view` 是生产接线，`src/server/http.rs:410`；`build_router` 是 `pub`，`:386`）。fixture：两个对象 —— 干净 `样例客户`、脏 `主键台账`（`主键` 是 `MODELING_TERMS_CJK` 守门词，且**不是**标识符形态 ⇒ `filter_view` 剔不掉，见 `tests/test_property_filter.rs:89-108`）。文件 `tests/test_req107_guard_granularity.rs`，函数 `test_req107_cardpoint2_whole_document_vs_per_node_granularity` |
| 测试步骤 | 1) `let st = state_from_view(&cv, "样例项目", 2, "correct-horse-battery");`<br>2) `GET /api/graph?project=样例项目&version=2` ⇒ 断言 `500` 且 body `!contains("主键")` 且 `!contains("样例客户")`（拒发时不得顺带把干净数据发出去）；<br>3) `GET /api/node?...&id=<样例客户的 key>` ⇒ 断言 **`200`** 且 body 含「样例客户」（**这就是「逐节点」的证据**：一个坏节点没废掉整页）；<br>4) `GET /api/node?...&id=<主键台账的 key>` ⇒ 断言 `500` 且 body `!contains("主键")`；<br>5) 追加「整份」侧的证据（无 DB 的近似）：`assert!(!scan_modeling_terms(&serde_json::to_string_pretty(&filter_view(&cv).view).unwrap()).is_empty())` —— 说明**若**统一到「整份」，C3 侧必然 `exit 2`；<br>6) 命令 `cargo test --test test_req107_guard_granularity` |
| 预期结果 | ② 500、③ **200**、④ 500。**裁定后**：<br>· 选「统一整份」⇒ 第 3 步变红（干净节点也 500），Dev 必须改成 `assert_eq!(500)` 并登记 AC-005「`/api/node` 从逐节点变整份 ⇒ 一个坏节点会废掉整页」；<br>· 选「统一逐节点」⇒ `/api/graph` 侧必须给出「跨节点上下文泄漏」的处置（`/api/graph` 没有「节点」可分），本条第 2 步可能变红；<br>· 保持现状 ⇒ 全绿 + AC-004 必须写明理由与被放弃方案的代价 |
| 优先级 | **P0** |
| 现行状态 | **绿**（现状 = 逐节点）。依据：`src/server/http.rs:179-190`（`guarded` 对**整份** `state.graph` 扫，`:239`）、`src/server/http.rs:254-260`（`/api/node` 只 `guarded(node)` 单个 `EntityView`）、`src/mcp/tools.rs:693-710`（`guard` 整份序列化后全文扫）。<br>⚠️ **这正是 `src/domain/property_filter.rs:90-107` 自陈「全 `tests/` 零断言钉住」的那一格** —— 本条落地后该段登记必须同步改成「已由 `tests/test_req107_guard_granularity.rs::test_req107_cardpoint2_*` 钉住」，否则文档与代码互相打脸 |

### TC-015 · 卡点③ stderr 报告形状：页面只报数量，stderr 报原名

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-015` |
| 关联 AC | AC-004 卡点③（`cli.rs:1243-1247` 报原名 vs `html_view.rs:486-489` 只报数量） |
| 标题 | 同一次剔除：**页面正文**只报数量（不含原名），**stderr 侧**（`FilteredView::report()` / `RenderedPage.skipped_props`）带原名 | 
| 前置条件 | 不需 DB。fixture：`PropertyView{ name:"样例关联的 Key 凭据" }`（`has_raw_identifier` 命中 ⇒ 整条剔除）。文件 `tests/test_req107_guard_granularity.rs`，函数 `test_req107_cardpoint3_report_shape_count_vs_raw_names` |
| 测试步骤 | 1) `let page = render_html(&cv, &[]);`<br>2) 断言页面正文 `!page.html.contains("样例关联的 Key 凭据")` 且 `page.html.contains("另有 1 项")`（`src/domain/html_view.rs:378-382` 的措辞）；<br>3) `let f = filter_view(&cv); let rep = f.report();` 断言 `rep.contains("样例关联的 Key 凭据")`（`src/domain/property_filter.rs:200-229`，`DroppedProp::line()` 在 `:168-174`）；<br>4) 断言 `page.skipped_props` 也带原名（`src/domain/html_view.rs:367`，CLI 在 `src/cli.rs:1259-1265` 把它打到 stderr）；<br>5) **形状统一的可机判形式（建议 Dev 采纳）**：把报告形状抽成一个常量/函数 `REPORT_SHAPE`（或 `fn report_line(&DroppedProp) -> String`），断言**所有**出口（`cli.rs:1243-1247`/`:1259-1265`/`:1297-1298`、`http.rs:427-430`、`serve.rs:102-105`）都只经它产出文本；<br>6) 命令 `cargo test --test test_req107_guard_granularity` |
| 预期结果 | ② 页面无原名 + 有数量；③④ stderr 侧有原名。**裁定后**：<br>· 选「统一只报数量」⇒ ③④ 变红，Dev 改成 `!contains` 并在 AC-005 登记「所有出口的 stderr 不再报被剔原名 ⇒ 运维定位能力下降」；<br>· 选「统一报原名」⇒ ② 变红，且**必须**回答「原名本身带技术串，报出来算不算二次泄漏」（`src/domain/html_view.rs:486-489` 的原始理由就是这个）；<br>· 选「分两级（客户侧数量 / 运维侧原名）」⇒ 本条按现状绿 + ⑤ 必须落地 |
| 优先级 | P1 |
| 现行状态 | **绿**（现状 = 两级并存）。依据：`src/domain/html_view.rs:372-382`（页面正文「另有 {n} 项…没有显示」，不含原名）+ `src/domain/html_view.rs:363/367`（`skipped_props` 带原名）+ `src/cli.rs:1242-1248`（对象原名进 stderr）+ `src/cli.rs:1259-1265`（属性原名进 stderr，前 20 条）+ `src/domain/property_filter.rs:200-229`（`report()` 带前 20 条原名） |

---

## 五 · AC-005 · 零可见变化（含可复现的字节比对方案）

> 完整方案（fixture / 命令 / 比哪几份 / golden 的来历）见 **附录 A**。这里只给两条用例。
>
> ⚠️ **AC-005 与 AC-006 的冲突必须显式处理**：AC-006 要求 Z1 的 400 响应体**变化**（不再含项目名），所以「零可见变化」的**口径不含 Z1**。本节的 golden 集**排除 Z1**，Z1 的变化作为**唯一允许的变化**在 AC-005 变化清单里登记。

### TC-016 · 零可见变化（golden 字节比对，无需 DB）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-016` |
| 关联 AC | AC-005 |
| 标题 | 13 份对外可见字节（覆盖 13 个客户类出口）在收口前后**逐字节相等**；golden 的来源 commit 被钉死为基线 |
| 前置条件 | fixture：`tests/fixtures/req107/cv_107.json`（`CustomerView`，`src/domain/view.rs:699` 有 `Deserialize`）+ `fixture_concept_groups()`/`fixture_names()` 两个 helper；golden：`tests/fixtures/req107/golden/`（含 `SHA256SUMS` 与 `PROVENANCE.txt`）。**不需要 DB**。文件 `tests/test_req107_zero_visible_change.rs`，函数 `test_req107_zero_visible_change_across_13_customer_outlets` |
| 测试步骤 | 1) 断言 `tests/fixtures/req107/golden/PROVENANCE.txt` 第一行 == `b714f48`（**防「用改完的代码生成 golden」的自我实现**，见附录 A.4）；<br>2) 断言 `SHA256SUMS` 里每份 golden 的哈希与文件内容相符（防 golden 被手改）；<br>3) 逐份产出当前字节并与 golden 比较：`assert_eq!(got, want, "出口 {id} 的字节变了 —— 要么改回去，要么在 AC-005 变化清单里登记")`；<br>4) 断言 C1 与 C2 字节相同（`--html` stdout 与 `--out` 文件必须是同一份）；<br>5) 命令 `cargo test --test test_req107_zero_visible_change` |
| 预期结果 | 13 份全部逐字节相等；C1 == C2。任一份不等 ⇒ 红，报错必须打印**第一处差异的字节偏移 + 前后各 40 字节**（⛔ 不许只打印长度） |
| 优先级 | **P0** |
| 现行状态 | **红**。golden 目录不存在（`tests/fixtures/` 这个目录**今天压根不存在**），另需 3 个 helper 与 fixture 文件。**注意**：`src` 未改时，第 3 步在逻辑上应当**绿** —— 所以今天的红纯粹来自「判据资产缺失」，不是「已有字节变化」 |

### TC-017 · 端到端真进程字节比对（需 DB，本机不可复核）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-017` |
| 关联 AC | AC-005（「同一份 fixture 下，改动前后各产出一份输出并 `diff`」的真进程版） |
| 标题 | 真进程 `ontok ontology show/overview` 的 stdout 与 golden 逐字节相等（沿用 REQ-078 的专属 schema 沙箱） |
| 前置条件 | **需要 `ONTOK_TEST_DATABASE_URL`（本机不可复核）**；复用 `tests/test_domain_print_sites_binary_req078.rs:43-258` 的 `Sandbox`（专属 schema + 零 DELETE）与 `:329` 的 `assert_site_bytes`。文件 `tests/test_req107_zero_visible_change.rs`，函数 `test_req107_e2e_stdout_bytes_unchanged` |
| 测试步骤 | 1) 沙箱 seed 一份与 `cv_107.json` **同构**的快照（用 `propose import` + `review decide --accept-all` + `proposal apply`，照 `:82-132`）；<br>2) 逐条跑 5 条 CLI 出口，`assert_site_bytes` 与 golden 比；<br>3) 断言 stderr 里被剔数量的**措辞**与 golden 记录一致（若裁定改了报告形状，这一步红 ⇒ 必须登记）；<br>4) 命令 `cargo test --test test_req107_zero_visible_change -- --nocapture`；<br>5) 交付物里必须写「全量基线本机不可复核」 |
| 预期结果 | 5 条 CLI 出口 stdout 逐字节相等；stderr 措辞一致。本机执行不了 ⇒ 归档时声明「未在本机复核」 |
| 优先级 | P1 |
| 现行状态 | **不可执行（本机无 DB / 无集群）**；在 Linux + 测试库上，今天是 **红**（golden 资产不存在）。⛔ 不许把它写成「全量 846 必须复现」 |

---

## 六 · AC-006 · Z1 零闸出口必须修

> **具体的项目名与断言字符串（本需求的硬要求）**：
> - 服务端绑定的项目名 = **`外键登记表`**（含守门词「外键」，`MODELING_TERMS_CJK` 命中）
> - 请求 = **`GET /api/graph?project=wrong&version=2`**（项目名故意不匹配 ⇒ 进入 `src/server/http.rs:222-230` 那条分支）
> - 断言字符串 = **`"外键登记表"`**，辅助断言 = **`"外键"`**

### TC-018 · Z1 回显项目名 ⇒ 400 响应体必须不含项目名

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-018` |
| 关联 AC | AC-006 |
| 标题 | `GET /api/graph?project=wrong&version=2` 的 400 响应体**不得**出现服务端绑定的项目名 `外键登记表`（也不得出现「外键」） |
| 前置条件 | 不需 DB。`state_from_view(&CustomerView{objects:vec![],relations:vec![]}, "外键登记表", 2, "correct-horse-battery")` + `build_router`（均 `pub`：`src/server/http.rs:410`/`:386`）。文件 `tests/test_req107_scope_error_gate.rs`，函数 `test_req107_z1_scope_error_does_not_echo_the_project_name` |
| 测试步骤 | 1) 造 state（项目名 `外键登记表`，版本 2）；<br>2) `oneshot` 打 `GET /api/graph?project=wrong&version=2`（带 `Authorization: Bearer correct-horse-battery`）；<br>3) 断言 `status == 400`；<br>4) 断言 `!body.contains("外键登记表")`；<br>5) 断言 `!body.contains("外键")`（更严：连词干都不许漏）；<br>6) 断言 body 里**仍有一句人话**：`body.contains("error")` 且 `!body.trim().is_empty()`（⛔ 不许用「返回空 400」过关）；<br>7) 命令 `cargo test --test test_req107_scope_error_gate` |
| 预期结果 | ①② 400；④⑤ 两个 `!contains` 都通过；⑥ 有 `error` 键且非空 |
| 优先级 | **P0** |
| 现行状态 | **红**。依据：`src/server/http.rs:222-229` —— `if project != state.project.as_ref() \|\| version != state.version { return Err(err(400, &format!("本服务只对外提供 {} 第 {} 版这一份内容，别的一概不给。", state.project, state.version))) }`：`state.project` **原样拼进** `format!`，`err()`（`:169-171`）直接把它塞进 `{"error": …}`，**既不过 `guard`、也不走 `safe_error`、也不转义**。对照：同一个项目名在首页 `src/server/page.rs:300` 会被 `guard` 拒发 ⇒ 同一份数据两个出口两种结果 |

### TC-019 · 对照组：干净项目名**仍应**回显（防「一刀切成通用文案」）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-019` |
| 关联 AC | AC-006（对照组，纪律 E） |
| 标题 | 项目名为干净的 `样例项目` 时，同样的错参请求**仍应**在 400 正文里说明「本服务只提供样例项目第 2 版」 |
| 前置条件 | 同 TC-018。同文件，函数 `test_req107_z1_clean_project_name_is_still_echoed` |
| 测试步骤 | 1) 把项目名换成 `样例项目` 重跑同一请求；<br>2) 断言 `status == 400`；<br>3) 断言 `body.contains("样例项目")`（修复**不许**退化成「一律换成通用文案」）；<br>4) 命令同上 |
| 预期结果 | 400 且正文含 `样例项目`。若 Dev 选择「连干净项目名也不回显」（更保守的方案），本条红 ⇒ 必须把断言改成 `!contains` 并在 AC-005 变化清单登记「Z1 的错误文案不再指认项目名」—— ⛔ 但**不许**只删这一条 |
| 优先级 | P2 |
| 现行状态 | **绿**（今天什么都回显，自然含 `样例项目`）。判别力说明：它今天绿但**不是空转** —— 它挡的是「把 `safe_error` 用到过度、所有 400 都变成一句套话」这种修法，那会让运维看不懂「到底是哪个项目」 |

### TC-020 · 其余 4xx 分支也不得回显调用方输入

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-020` |
| 关联 AC | AC-006（顺带把同一函数里另外 3 条分支一起钉住） |
| 标题 | 缺 `project` / 缺 `version` / `version` 非数字 三条分支的文案里，不得出现调用方传进来的原始值 |
| 前置条件 | 同 TC-018。同文件，函数 `test_req107_check_scope_other_4xx_branches_do_not_echo_input` |
| 测试步骤 | 1) 三条请求：`/api/graph`、`/api/graph?project=样例项目`、`/api/graph?project=样例项目&version=<任取一个含「外键」的串>`；<br>2) 每条断言 `400`；<br>3) 第 3 条断言 `!body.contains("外键")`（今天这条已经绿，因为 `src/server/http.rs:216-221` 的文案是固定的「version 要写一个数字（比如 2）。」，**不拼入参**）；<br>4) 命令同上 |
| 预期结果 | 三条 400；第 3 条无「外键」 |
| 优先级 | P2 |
| 现行状态 | **绿**（护栏已存在）。依据：`src/server/http.rs:203-208`（缺 project，固定文案）、`:210-215`（缺 version，固定文案）、`:216-221`（解析失败，固定文案，**不回显 raw**）。本条的价值是防止 Dev 在修 Z1 时「顺手把 raw 拼回去」 |

---

## 七 · AC-007 · 建模类豁免必须显式声明

### TC-021 · 8 个建模出口在代码里显式标注 `track = Modeling`

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-021` |
| 关联 AC | AC-007 |
| 标题 | §三 的 8 条（D1~D8）在 `OUTLETS` 里逐条存在且 `track == Modeling`，并且它们的函数体里**没有**判据调用（豁免是被**声明**的，不是靠「没调闸」推出来的） |
| 前置条件 | 需登记表；不需 DB。文件 `tests/test_req107_track_manifest.rs`，函数 `test_req107_modeling_track_is_explicitly_declared` |
| 测试步骤 | 1) `OUTLETS` 里 `track == Modeling` 的条数 == 8，`id` 集合 == `{D1..D8}`；<br>2) 每条 `path:call_site` needle 命中；<br>3) 对每条所在**函数**（用 `tests/test_binary_output_guard.rs:643` 的 `fn_span` 思路）断言 `criterion_sites(fn_body).is_empty()` —— 建模轨**应当**零判据；<br>4) 断言登记表里每条 Modeling 项都带 `exemption_reason: &'static str` 且非空（「豁免依据」不许沉默）；<br>5) 命令 `cargo test --test test_req107_track_manifest` |
| 预期结果 | 8 条、needle 全中、函数体零判据、理由非空 |
| 优先级 | **P0** |
| 现行状态 | **红**。登记表不存在。今天的「豁免」只存在于**注释与表字符串**里：`src/cli.rs:1203-1205`（`--model-view` 注释）、`src/cli.rs:1066-1068`（`review show` 注释）、`src/cli.rs:508-512`/`:582-600`/`:601-625`（`DOMAIN_PRINT_SITES` 里 `wiring: WIRING_NONE` + `judged_by: &[]`）—— **没有机读的 track 字段**，判定方式是「因为它没调闸所以它是建模轨」，正是 AC-007 要治的 |

### TC-022 · 把 D1 误标成客户类 ⇒ 行为要变、测试要红

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-022` |
| 关联 AC | AC-007（「判据：把某个建模出口误标为客户类时，**行为要变**（被闸拦住）且**测试要红**」） |
| 标题 | 同一条 `--model-view` 出口，`track = Modeling` 时原样输出；改成 `track = Customer` 后**必须**被闸拦住（因为 mv 里必然含 `key`/`cardinality` 这类建模术语） |
| 前置条件 | 需管线把 track 做成**数据**（`OutletSpec` 值，而不是编译期硬编码），这样测试才能构造「误标」的 spec；不需要 DB。同文件，函数 `test_req107_mislabeling_a_modeling_outlet_turns_it_red` |
| 测试步骤 | 1) 造一份建模轨产物 `mv = project_model_track(&snapshot_fixture)`（`src/domain/model_view.rs`），断言 `scan_modeling_terms(&to_string_pretty(&mv)).is_empty() == false`（**前提自检**：它确实含建模术语，否则本条是空转 —— 纪律 E）；<br>2) 用 `OutletSpec{ track: Modeling, format: Json }` 渲染 ⇒ 断言 `Ok`，输出**含**「key」/建模字段；<br>3) 用 `OutletSpec{ track: Customer, format: Json }` 渲染**同一份 mv** ⇒ 断言 `Err`（或 `exit 2` 等价的拒绝信号），且错误正文**不含**具体词条；<br>4) 结构半：把 `OUTLETS` 里 D1 的 `track` 改成 `Customer`（源码变异），断言 TC-021 与 TC-007 都红；<br>5) 命令 `cargo test --test test_req107_track_manifest`（步骤 4 需重编译） |
| 预期结果 | ② Ok 且含建模字段；③ Err 且正文无词条；④ TC-021/TC-007 红。任一步绿 ⇒ AC-007 的「误标要能被观察到」不成立 |
| 优先级 | **P0** |
| 现行状态 | **红（前置不成立）**。今天没有 track 这个数据，**构造不出「误标」**：`src/cli.rs:1206-1209` 的建模分支**无条件**输出，没有任何「轨道」参与决策；因此第 3 步无从执行。⇒ 状态记为「前置不成立」，必须在 TC-021 绿之后才能执行 |

---

## 八 · AC-008 · Z2 / Z3 纳入管线（静态文本）

> `tools/list` 的 `description` / `title` 今天**零断言**：`tests/test_mcp_server.rs:923-948` 只断言 `name`（`:936-940`）与 `inputSchema`（`:941-946`），**没有一个字**碰 `description`/`title`。
> `initialize` 的模板正文同理：`src/mcp/tools.rs:742` 只扫**项目名**，`format!` 出来的正文（`:751-758`）**从不过闸**。

### TC-023 · `tools/list` 的 `description` / `title` 补断言

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-023` |
| 关联 AC | AC-008（「若选『测试守卫』：必须断言 `description` / `title`」） |
| 标题 | 5 个工具的 `description` 与 `title`：非空、长度下界、**零守门词**，且 `tools/list` 的真实响应体里逐字带着同一份文本 |
| 前置条件 | 结构半不需 DB（`tools::catalog()` 是 `pub`，`src/mcp/tools.rs:403`）；响应半也不需 DB —— 可直接构造 `OntologyContext`（字段全 `pub`，`src/mcp/tools.rs:132-142`）后调 `handle_line`（`src/mcp/serve.rs:214`）。文件 `tests/test_req107_static_texts.rs`，函数 `test_req107_tools_list_descriptions_and_titles_are_asserted` |
| 测试步骤 | 1) `for t in tools::catalog()`：`assert!(!t.description.trim().is_empty())`、`assert!(t.description.chars().count() >= 20)`（下界要写清为什么：<20 字不可能说清「用它回答什么」）、`assert!(!t.title.trim().is_empty())`；<br>2) `assert!(scan_modeling_terms(t.description).is_empty())` 与 `assert!(scan_modeling_terms(t.title).is_empty())`（**这是今天完全没有的那一格**）；<br>3) 断言 5 个 `name` 恰为 `list_concepts/describe_concept/describe_object/list_relations/search_objects`（集合相等，多一个少一个都红）；<br>4) 响应半：构造 ctx 后 `handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)`，解析结果，逐条断言 `result.tools[i].description == catalog()[i].description` 与 `title` 相同（防 `src/mcp/serve.rs:260-266` 那个 `json!` 映射把字段丢了/写错）；<br>5) 命令 `cargo test --test test_req107_static_texts` |
| 预期结果 | ①②③④ 全绿；报错必须指出是哪个工具、哪个字段、命中哪个词 |
| 优先级 | **P0** |
| 现行状态 | **绿 —— 但今天零断言（这是「假绿」的典型位置）**。实测今天 5 条 `description`（`src/mcp/tools.rs:408-410`/`:416-418`/`:424-428`/`:434-441`/`:453-455`）与 `title` 都非空且干净，所以本条落地当天就会绿。**它的价值不在这天**，而在 TC-024：没有 TC-024 的变异，本条与一条恒真断言无法区分 |

### TC-024 · 变异：把某个 `description` 改成含「外键」⇒ TC-023 必须红

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-024` |
| 关联 AC | AC-008（纪律 G：证明新断言不是空转） |
| 标题 | 手工变异：`catalog()` 里 `describe_object` 的 `description` 追加「（外键登记）」⇒ TC-023 红 |
| 前置条件 | 前置：TC-023 绿；需重编译。文件同上，变异由 Dev/QA 手工执行，结果贴入交付物 |
| 测试步骤 | 1) 改 `src/mcp/tools.rs:424-428` 的 `description` 加「外键」；<br>2) `cargo test --test test_req107_static_texts`；<br>3) 期望 TC-023 第 ② 步红；<br>4) 再跑 `cargo test --test test_mcp_server`，**记录既有 5 条相关测试是否也红** —— 若全绿，说明「既有断言确实抓不到 description」（这就是零断言的证据，要写进交付物）；<br>5) 贴三元组（二进制 sha256 + patch + 逐条结果），复原 |
| 预期结果 | TC-023 红；且「既有测试全绿」这一点必须被**记录下来**（它反过来证明 AC-008 说的「零断言」是真的） |
| 优先级 | P1 |
| 现行状态 | **红（前置不成立）**：TC-023 尚未落地，无判据可变红 |

### TC-025 · `initialize` 的 `instructions` **模板正文**纳入守卫

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-025` |
| 关联 AC | AC-008（Z3） |
| 标题 | `tools::instructions(&ctx)` 的**整段返回串**（含模板正文 + 项目名 + 版本）零守门词，而不只是项目名 |
| 前置条件 | 结构半不需 DB；响应半不需 DB（自造 ctx）。同文件，函数 `test_req107_initialize_instructions_template_body_is_guarded` |
| 测试步骤 | 1) 用干净 ctx 调 `tools::instructions(&ctx)`，断言 `scan_modeling_terms(&text).is_empty()`；<br>2) 断言 `text` 里确实包含模板正文的**固定句子**（如「该版本已冻结」「confirmation」）—— **前提自检**：若哪天模板被清空，第 1 步会恒绿（纪律 E）；<br>3) 断言 `!ctx.confirmation.is_empty() \|\| text.contains("confirmation")`（模板正文没被误删）；<br>4) **若 Dev 选「运行时闸」**：改为断言 `src/mcp/serve.rs` 的 `"initialize"` 分支（现 `:245`）里，`instructions` 的返回值必须经过 `guard`/`safe_error` 或管线 —— 源码 needle 断言 `p::encode_result(id, p::initialize_result(agreed, &<闸后产物>))`；<br>5) 命令 `cargo test --test test_req107_static_texts` |
| 预期结果 | ①②③ 全绿；④（若选运行时）needle 命中。<br>⛔ **必须二选一并写明理由**（AC-008 原话）：选测试守卫 ⇒ 靠 TC-025/026；选运行时闸 ⇒ 靠 ④ + 一条变异 |
| 优先级 | P1 |
| 现行状态 | **绿 —— 但今天零断言**。依据：`src/mcp/tools.rs:751-758` 的模板正文今天**恰好**不含守门词（所以第 1 步会绿）；而代码里**没有**任何地方扫它：`src/mcp/tools.rs:742` 只扫 `ctx.project`，`src/mcp/serve.rs:245` 直接 `encode_result` ⇒ 今天往模板正文里塞「外键」**不会有任何测试红**（这就是 Z3） |

### TC-026 · 变异：往 `instructions` 模板正文塞「外键」⇒ 必须红

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-026` |
| 关联 AC | AC-008（Z3 的判别力自证） |
| 标题 | 手工变异：在 `src/mcp/tools.rs:752-757` 的模板正文里插入「外键」⇒ TC-025 红；若 Dev 选了运行时闸，则红色可能出现在 TC-025 第 4 步或渲染期 |
| 前置条件 | 前置：TC-025 绿；需重编译 |
| 测试步骤 | 1) 在模板正文里插「外键」；<br>2) `cargo test --test test_req107_static_texts`；<br>3) 期望 TC-025 红；<br>4) 同时跑 `cargo test --test test_mcp_server`，记录既有 `instructions_do_not_leak_a_modeling_term_project_name`（`:898-920`）是否红 —— 它只测项目名，理论上**不红**（需要记录，作为 Z3 存在的证据）；<br>5) 贴三元组，复原 |
| 预期结果 | TC-025 红；既有那条测试**不红**（并记录） |
| 优先级 | P1 |
| 现行状态 | **红（前置不成立）**：TC-025 未落地 |

---

## 九 · AC-009 · 红线零触碰

### TC-027 · 红线文件与守门器实现零改动

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-027` |
| 关联 AC | AC-009 |
| 标题 | `git diff --numstat develop -- <红线四文件 + mcp/tools.rs 的 guard 段>` 为空；G1~G4 措辞逐字节未变 |
| 前置条件 | 需 `git`（本仓开发机都有）；不需 DB。文件 `tests/test_req107_red_lines.rs`，函数 `test_req107_red_lines_and_guard_are_untouched` |
| 测试步骤 | 1) 在测试里调 `Command::new("git").args(["diff","--numstat","develop","--","tests/drop_rate.rs","tests/test_fk_naming.rs","src/domain/view.rs","src/domain/review_view.rs"])`，断言 stdout **为空**；<br>2) `git diff develop -- src/mcp/tools.rs` 的**新增行/删除行**里不得出现 `fn guard` / `scan_modeling_terms` / `已拒绝外送`（守门器实现不动；允许 `guard` 被**调用**）；<br>3) `git show develop:src/domain/property_filter.rs` 的 G1~G4 四段（`//! ## ①` 到 `//! ## ③` 结束）与工作区版本**逐字节相等**（除非 AC-004 明确裁定要动并单独说明）；<br>4) 断言 `tests/` 下没有新增 `#[ignore]`：`git diff develop -- tests | 找出 '+#[ignore]'` 为空；<br>5) 命令 `cargo test --test test_req107_red_lines` |
| 预期结果 | ① 空输出；② 无命中；③ 逐字节相等；④ 空。<br>⚠️ **已知边界**：本条只能证明工作区与 `develop` 的差异里**没有**这四份文件，不能证明「历史里没改过又改回来」—— 那要看 PR 的逐 commit diff（人工项，见 TC-029） |
| 优先级 | P1 |
| 现行状态 | **绿**（今天 `git diff --numstat develop -- src tests` 为空）。它的价值在于 Dev 动手之后 |

---

## 十 · AC-010 · 交付证据

### TC-028 · 全量数字「带口径 + 声明本机不可复核」

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-028` |
| 关联 AC | AC-010-6（+ 页首「本机限制」） |
| 标题 | 交付物里报全量数字时，必须同时出现「口径」与「本机不可复核」两句；⛔ 不许出现「全量 62 suite / 846 passed 已在本机复现」这种无法执行的断言 |
| 前置条件 | **不可由 `cargo test` 判定**（判据对象是交付物文本，不是代码）。执行方式：`grep` 核对 + 人工签字 |
| 测试步骤 | 1) 对交付物（`design-summary.md` 或 Dev 报告）执行 `Select-String -Pattern "62 suite\|846 passed\|口径\|不可复核"`；<br>2) 断言同时命中「62」/「846」与「不可复核」；<br>3) 断言**不**出现「本机已跑通全量」「已复现 846」这类说法；<br>4) 记录本机能跑的实测：`cargo check --lib`、`cargo test --test test_req107_outlet_pipeline` 等的条数；<br>5) 命令：`cargo test --test <本需求各测试文件> ` + 文档核对 |
| 预期结果 | 口径句与不可复核句都在；无伪造数字。⛔ 本机**不要求**复现 846（页首已声明不可复核） |
| 优先级 | P2 |
| 现行状态 | **红**（交付物尚不存在） |

### TC-029 · 交付物 9 项完整性（半人工）

| 字段 | 内容 |
| --- | --- |
| 用例编号 | `TC-029` |
| 关联 AC | AC-010-1~9 |
| 标题 | `acceptance.md` AC-010 列的 9 项逐项存在，且第 1 项（出口映射表）覆盖 33 条、第 4 项（漏接即红实测）含**三元组**、第 9 项（异议）**有内容**（无异议必须写「无」） |
| 前置条件 | 人工核对 + 脚本辅助；不需 DB |
| 测试步骤 | 1) 逐项打勾 9 项；<br>2) 映射表逐条核 33 行，与 TC-007 的 `OUTLETS` 条数一致；<br>3) 漏接实测逐条核：每个变异都有「二进制 sha256 + patch 内容/哈希 + 逐条结果」三样（缺一样即算未执行）；<br>4) 异议项：如果 Dev 认 33 口径、Z4 判定、`diff` 归类这三点**任何一个**没表态 ⇒ 记「未完成」；<br>5) 人工在 `BA/demands/REQ-107/status.md` 侧记录（QA 不直接改） |
| 预期结果 | 9/9 打勾；三元组 3/3 完整；异议项非空。任一项缺失 ⇒ 退回 |
| 优先级 | P2 |
| 现行状态 | **红**（交付物尚不存在） |

---

## 附录 A · AC-005 的**可复现字节比对方案**（完整步骤）

### A.1 比什么（fixture 与出口清单）

| 项 | 值 |
| --- | --- |
| fixture | `tests/fixtures/req107/cv_107.json` —— 一份 `CustomerView`（`src/domain/view.rs:699` 有 `Deserialize`），合成名：对象 `样例客户` / `样例订单`、属性 `客户编号ID`（测改名口径）、`样例关联的 Key 凭据`（测剔除）、对象说明 `来源 wg_order_ref 的订单`（测对象说明隐去） |
| 辅助 fixture | 同一测试文件里的 `fixture_concept_groups() -> Vec<ConceptGroup>` 与 `fixture_names() -> BTreeMap<String,String>`（`build_overview` 的两个入参，`src/domain/overview.rs:133`） |
| ⛔ 合成夹具纪律 | 不得含真实客户表名/中文名/列名/列注释（REQ-104 栽过）—— 一律 `wg_` 前缀 + `样例` 开头，理由见 `tests/test_property_filter.rs:23-49` |

**要比的 13 份 golden 文件**（覆盖 13 条客户类出口；C2 与 C1 共用一份，用等式断言；H3 一条出口采两份，因为两个对象才暴露得出重名/重号问题）：

| # | 出口 | 复刻方式（全部无需 DB） | golden 文件名 |
| --- | --- | --- | --- |
| 1 | C1 `show --html` stdout | `render_html(&cv, &DEFAULT_SAMPLING).html`（`src/domain/html_view.rs:313`） | `C1_html.txt` |
| 2 | C2 `show --html --out` | **断言与 C1 逐字节相同**（同一份 `page.html`，`src/cli.rs:1269`/`:1281`） | 复用 `C1_html.txt` |
| 3 | C3 `show`(JSON) | `serde_json::to_string_pretty(&filter_view(&cv).view)?` + `"\n"`（`src/cli.rs:1295-1326`） | `C3_json.txt` |
| 4 | C4 `overview` | `render_overview_text(&build_overview(&cv,&groups,&names))`（`src/cli.rs:1344`/`:1350`） | `C4_overview.txt` |
| 5 | C5 `overview --json` | `to_string_pretty(&build_overview(...))?` + `"\n"` | `C5_overview_json.txt` |
| 6 | H1 `/` 首页 | `index_html("样例项目", 2).unwrap()`（`src/server/page.rs:289`） | `H1_index.html` |
| 7 | H2 `/api/graph` | `oneshot` 的**响应体原始字节**（不要自己 `to_string_pretty` —— `Json<Value>` 的序列化形状要照实取） | `H2_graph.json` |
| 8 | H3 `/api/node?id=样例客户` | 同上 | `H3_node_1.json` |
| 9 | H3 `/api/node?id=样例订单` | 同上（两个对象各一份，重名/重号问题才暴露得出来） | `H3_node_2.json` |
| 10–14 | M1~M5 | `handle_line(&ctx, <tools/call 请求行>)` 返回的**整行字节**（`src/mcp/serve.rs:214`；`ctx` 用 `OntologyContext` 字面量构造，字段全 `pub`） | `M1..M5.json` |

**明确排除**：`Z1`（`GET /api/graph?project=wrong&version=2`）—— 它的响应体**必须变化**（AC-006）。它的「变化前/变化后」作为**唯一允许的变化**登记在 AC-005 变化清单里，格式：

| 出口 | 变化前 | 变化后 | 理由 |
| --- | --- | --- | --- |
| `Z1` `src/server/http.rs:222-230` | `{"error":"本服务只对外提供 外键登记表 第 2 版这一份内容，别的一概不给。"}` | `{"error":"这一问给不出答案：输入的内容里出现了只有内部使用的术语（为免泄漏，此处不列出具体词条）。"}` | AC-006：项目名不得从错误响应漏出 |

### A.2 一次性产出「改动前」那一份（在基线 commit 上）

```powershell
# 0) 起一个**只读基线**的临时 worktree（临时文件只放 .review-workspace/req107/）
cd D:\MyCodes\AI\an_onto_demo
git worktree add --detach .review-workspace\req107\base b714f48

# 1) 把**只新增的测试资产**拷进基线（⛔ 不碰 src：拷完必须确认 git status 只有 ?? 未跟踪项）
Copy-Item -Recurse feature-REQ-107\tests\fixtures\req107 .review-workspace\req107\base\tests\fixtures\
Copy-Item feature-REQ-107\tests\test_req107_zero_visible_change.rs .review-workspace\req107\base\tests\
cd .review-workspace\req107\base
git status --short        # 期望：只有 ?? 两行，没有任何 M 行；有 M 就停下

# 2) 产出 before
$env:REQ107_OUT = "D:\MyCodes\AI\an_onto_demo\.review-workspace\req107\before"
cargo test --test test_req107_zero_visible_change -- --ignored req107_write_golden

# 3) 回到被测分支，产出 after
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-107
$env:REQ107_OUT = "D:\MyCodes\AI\an_onto_demo\.review-workspace\req107\after"
cargo test --test test_req107_zero_visible_change -- --ignored req107_write_golden

# 4) diff（用 git 自带的，别用 pwsh 的 diff 别名）
cd D:\MyCodes\AI\an_onto_demo
git diff --no-index --stat .review-workspace\req107\before .review-workspace\req107\after
git diff --no-index            .review-workspace\req107\before .review-workspace\req107\after
```

**期望**：`--stat` 输出为**空**且 `--no-index` 无输出（退出码 1 = 有差异，0 = 无差异 —— 注意 `git diff --no-index` 在**无差异**时返回 0）。

Linux 等价：`git worktree add --detach .review-workspace/req107/base b714f48`、`REQ107_OUT=... cargo test --test ... -- --ignored req107_write_golden`、`git diff --no-index --stat before after`。

### A.3 入库 golden 与常态判据

```powershell
# golden 入库：before/ 即「改动前」，把它作为 golden
Copy-Item .review-workspace\req107\before\* feature-REQ-107\tests\fixtures\req107\golden\
# 哈希清单（Linux 用 sha256sum）
Get-ChildItem feature-REQ-107\tests\fixtures\req107\golden\*.txt, *.html, *.json |
  Get-FileHash -Algorithm SHA256 | ForEach-Object { "$($_.Hash)  $([IO.Path]::GetFileName($_.Path))" } |
  Set-Content feature-REQ-107\tests\fixtures\req107\golden\SHA256SUMS
# 来源声明（防「用改完的代码生成 golden」—— 见 A.4）
"b714f48" | Set-Content feature-REQ-107\tests\fixtures\req107\golden\PROVENANCE.txt
```

常态判据（每次 CI/本地都跑，**不需要 DB**）：

```powershell
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-107
cargo test --test test_req107_zero_visible_change
```

### A.4 为什么这套方案防得住「假绿」

| 假绿形态 | 本方案的防线 |
| --- | --- |
| golden 用**改完之后**的代码生成 ⇒ diff 永远空 | `PROVENANCE.txt` 第一行必须 == `b714f48`（TC-016 步骤 1 断言）。要伪造就得手写 `b714f48` —— 那是**显式伪造**，在审查里可见（纪律 G） |
| golden 被手改一个小字符让它「对上」 | `SHA256SUMS` 与文件内容互校（TC-016 步骤 2）；改 golden 必须同时改哈希 ⇒ 两处 diff 都进 PR |
| 只比「长度」或「包含某串」 | 逐字节 `assert_eq!` + 报错打印差异字节偏移与上下文 |
| 只比 stdout，不比文件出口 | C1 == C2 的等式断言（`--out` 那条） |
| 导出空产物（双方都空 ⇒ 恒真） | 每份 golden 断言 `len() > 0`，且 `C1_html.txt` 断言 > 200 字节（照 `tests/test_domain_print_sites_binary_req078.rs:489-494` 的先例） |

### A.5 生成器的形状建议

`tests/test_req107_zero_visible_change.rs`：

```rust
// `#[ignore]` 的那条只用于「产出一份文件」，常态判据不 ignore
#[test] #[ignore] fn req107_write_golden() { /* 读 $env:REQ107_OUT，缺变量即 panic（⛔ 不许默认写 /tmp 或 home） */ }
#[test] fn test_req107_zero_visible_change_across_13_customer_outlets() { /* 逐份 assert_eq! */ }
```

⛔ 生成器**不许**读 `#[cfg(test)]` 以外的开关跳过判据；⛔ 不许写 `/tmp`、不许写 home（`demand.md:83`）。

---

## 附录 B · 变异三元组模板（纪律 G，AC-003/AC-008 都要贴）

```text
变异编号：M-1 / M-2 / M-3 …
目标：src/cli.rs:1326（C3 的管线接线被摘掉）
patch：
  --- a/src/cli.rs
  +++ b/src/cli.rs
  @@ ...
  -        let out = crate::domain::pipeline::render(OutletSpec::C3, &cv)?;
  -        crate::cli::emit_domain_line(out.as_str());
  +        crate::cli::emit_domain_line(&json);
patch sha256：<sha256 of m1.patch>
变异后测试二进制 sha256：<sha256 of target/debug/deps/test_req107_outlet_pipeline-*.exe>
（源码扫描型判据另取**被扫文件的工作区 sha256**：sha256 src/cli.rs —— 纪律 Q）
运行：cargo test --test test_req107_outlet_pipeline
结果（逐条）：
  passed: N
  failed: ["test_req107_cli_five_customer_outlets_are_wired", "test_req107_outlet_files_have_zero_criterion_call_sites"]
  ignored: 0
结论：判据在「摘掉接线」这一格上**确实会红**
⛔ 编译失败不算执行：本次编译成功（cargo 输出 0 error）
```

---

## 附录 C · 用例可执行性分级

| 级别 | 含义 | 用例 |
| --- | --- | --- |
| **A** | 本机可跑（不需 DB、不需变异） | TC-001, TC-002, TC-003, TC-004（结构半）, TC-005（行为半亦可）, TC-006, TC-007, TC-008（`cargo test --doc`）, TC-009, TC-010, TC-013, TC-014, TC-015, TC-016, TC-018, TC-019, TC-020, TC-021, TC-023, TC-025, TC-027 |
| **B** | 需 `ONTOK_TEST_DATABASE_URL`（本机不可复核） | TC-017 |
| **C** | 需手工变异 + 重编译（结果可观察，过程不可自动化） | TC-011, TC-012, TC-022（步骤 4）, TC-024, TC-026 |
| **D** | 不可由测试判定（文档/人工） | TC-028, TC-029 |

---

## 附录 D · QA 对本 AC 的异议与自检发现（纪律 S）

> 这些不是「用例」，是**AC 自身的问题**。按纪律 S，发现 AC 某条不成立**直接说**，不迁就着实现。

| # | 发现 | 影响 | 建议 |
| --- | --- | --- | --- |
| D-1 | **`acceptance.md` 的分类计数相加是 34，与口径 D 的 33 差 1**：§一 13 + §二 4 + §三 8 + §四 9 = 34 | 若 Dev 照 34 建登记表，TC-007 会红；若照 33 建，AC 的「逐条可核对」就有一条无处安放 | 缺的那一条是 **Z4**（`INDEX_HTML` 模板）。它不是一次「写出」，不占出口位 ⇒ **33 是对的，Z4 应单列为「模板守卫」而不计入出口**。已在 TC-007 里按 33 = 5+3+5+1+2+8+9 写死，并单列 `Static(Z4)` 的处置。**请 BA 在 AC 里补一句确认** |
| D-2 | **AC-005 与 AC-006 直接冲突**：AC-006 要求 Z1 的 400 响应体变化，AC-005 要求「零可见变化」 | 若不加处理，Dev 只能二选一：要么违反 AC-006，要么让 AC-005 的 golden 红 | 已在 TC-016 里**显式把 Z1 排除出零变化口径**，并要求把它作为「唯一允许的变化」登记在 AC-005 变化清单（附录 A.1 给了逐字的变化前/后） |
| D-3 | **TC-001/TC-010 落地后，既有测试 `tests/test_property_filter.rs:341` 必红** | Dev 极可能「顺手删掉它」⇒ 违反 AC-009「不许删除测试」 | 该测试必须**改写**成同一套否定式判据（AC-009 允许改紧）。已写进 TC-001 的提示框 |
| D-4 | **`src/domain/property_filter.rs:139` 写着 M3 加固「已划走 REQ-103」**，而 AC-003 明确要求本需求钉住它 | 两处文档互相矛盾；Dev 可能拿「已划走」当理由不写 TC-010 | 本需求 AC-003 优先（它晚于 REQ-103 的划走决定）。**请 BA 裁定并在 `property_filter.rs` 的登记里同步一句**，否则下一轮复审会拿这段自陈来否定 TC-010 |
| D-5 | **纪律 O 写基线 `@ develop 7f26fa2`，AC 页首写 `@ b714f48`** | 「绑定 commit」两处不一致，报数字时口径含糊 | TC-028 只要求「带口径 + 声明不可复核」，不要求复现数字。建议 BA 统一写成 `b714f48`（`7f26fa2` 是脱敏前的本地 commit，AD 里已注明） |
| D-6 | **AC-003 的「类型层」在本仓无现成机制**（无 `trybuild`，`Cargo.toml:52-58`） | Dev 很可能退化成「源码 contains 断言」并宣称做到了类型层 | 已在 TC-008 给出**可执行**的替代：`compile_fail` doctest + `cargo test --doc`（只编 lib，本机可跑）。⛔ 加 `trybuild` = 新增依赖，超出范围 |
| D-7 | `tools/list` 的 `description`/`title` **今天确实零断言**（已核实 `tests/test_mcp_server.rs:936-946` 只查 `name`/`inputSchema`） | 这条不是风险，是**事实已被确认** | 无需动作；TC-023/024 直接补 |

---

## 附录 E · 一句话总表（给 Dev 直接照做）

| 建议测试文件 | 覆盖用例 | 是否需 DB |
| --- | --- | --- |
| `tests/test_req107_outlet_pipeline.rs` | TC-001 ~ TC-012 | ❌ |
| `tests/test_req107_guard_granularity.rs` | TC-013 ~ TC-015 | ❌ |
| `tests/test_req107_zero_visible_change.rs` | TC-016, TC-017（+ 附录 A 的 golden 生成器） | TC-016 ❌ / TC-017 ✅ |
| `tests/test_req107_scope_error_gate.rs` | TC-018 ~ TC-020 | ❌ |
| `tests/test_req107_track_manifest.rs` | TC-021, TC-022 | ❌ |
| `tests/test_req107_static_texts.rs` | TC-023 ~ TC-026 | ❌ |
| `tests/test_req107_red_lines.rs` | TC-027 | ❌ |
| （文档/人工） | TC-028, TC-029 | — |

> ⚠️ **两个实施陷阱**：
> 1. **别给新测试文件加 `//! @guard-manifest: REQ-081` 标记** —— 加了就必须同时登记进 `tests/guard_baseline.req081.txt`，否则 `tests/test_guard_manifest_req081.rs:904` 立刻红。若确实想让新文件进入元守卫层，请连带更新基线（它是「由实物机械生成」的，测试会给出可直接贴回的正文）。
> 2. **`cargo test --test <name>` 与 `cargo test` 的结论口径不同**：前者可以在本机跑，后者不行（`src/bin/ontok-gate-preflight.rs` 的 unix-only 代码）。交付物里必须写清跑的是哪一种。
