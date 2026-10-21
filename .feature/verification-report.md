# REQ-107 · 出口收口 —— 交付报告（按 AC-010 九项）

> **代码基线** `develop` `b714f48`（REQ-106 已合并）
> **全量测试基线** `62 suite / 846 passed / 0 failed / 1 ignored @ develop 7f26fa2`（纪律 O）
> **⚠️ 本机不可复核**（`src/bin/ontok-gate-preflight.rs` 的 unix-only 代码；见 §6）
> **分支** `feature/REQ-107`（未 push；第 1 轮 7 个 `[Dev]` 提交 + 第 2 轮 1 个 `[Dev]` 提交（见 `git log`），§7 与 §10）
> **设计文档** `.feature/design-req107.md`（实现前定稿，裁定与理由在那里；本文件只引用不重写）

---

## 0 · 结论先行

| 项 | 结果 |
| --- | --- |
| AC-001 唯一管线 | ✅ `src/domain/pipeline.rs`（1144 行，含判据调用与唯一的类型层门） |
| AC-002 13 条客户出口逐条接线 | ✅ 13/13（§1 逐条 `path:line`） |
| AC-003 漏接即红 | ✅ **类型层**（`compile_fail` doctest ×2，`cargo test --doc`）+ **源码否定式**（判据直调点数 == 0）+ **登记表对账**；6 个变异逐个实测（§4） |
| AC-004 三个卡点 | ✅ 逐个裁定：① JSON 不改名 ② 双粒度但改成**声明** ③ 两级 + 形状唯一（§3） |
| AC-005 零可见变化 | ✅ **13 份 golden 逐字节相等**（含用基线 worktree 生成的 before/after 互比）；唯一变化是 **Z1**（§5） |
| AC-006 Z1 | ✅ 400 正文不再回显项目名；干净项目名**照旧回显**（对照组） |
| AC-007 建模轨显式声明 | ✅ 8 条 `Track::Modeling` + 逐条豁免理由；D1 真走管线（误标 ⇒ 抛闸） |
| AC-008 Z2/Z3 | ✅ 选**运行时闸**（`publish_value(Z2/Z3)`）+ 补 `description`/`title`/`instructions` 断言 |
| AC-009 红线 | ✅ 四文件 `diff --numstat` 为空；`guard()` 未动；G1–G4 三段逐字节未变；无新增 `#[ignore]`；未删测试 |
| AC-010 交付物 | ✅ 本文件 §1–§9 |

**最可能被 QA 打回的一点**：我**改写了三条既有测试**（A3 只点名了一条），其中 `tests/test_binary_output_guard.rs` 的 G-1/G-4 改的是 REQ-078/080 的核心守卫。理由与「没有放宽、反而更严」的论证在 §7 与 §9.1 —— 这是本报告最需要复审者逐行看的地方。

---

## 1 · 出口映射表（AC-010-1 / AC-002）

### 1.1 13 条客户类出口 → 管线调用点（逐条 `path:line`）

> ⭐ **第 2 轮**：管线入口的第一个参数从 `OutletSpec::X` 改成 **`OutletId::X`**（出口只交代身份；
> 轨道 / 格式 / 粒度由 `pipeline::spec_of(id)` **查表**决定，见 §10.3）。下表已按新写法更新，
> 行号是**本轮**改后的行号。

| # | 出口 | 写出点 | 管线调用点 | 声明（格式 / 轨道 / 粒度） |
| --- | --- | --- | --- | --- |
| C1 | `ontology show --html` → stdout | `src/cli.rs:1333` `emit_domain_text(html)` | `src/cli.rs:1305` `let html = publish_customer(OutletId::C1, Product::CustomerHtml{…})` | Html / Customer / Whole |
| C2 | `ontology show --html --out` → 文件 | `src/cli.rs:1322` `std::fs::write(p, html.as_str())` | **同一份 `html`**（与 C1 共用一个管线产物；⚠ 已知 N4：`OutletId::C2` 没有独立调用点） | Html / Customer / Whole |
| C3 | `ontology show` → JSON stdout | `src/cli.rs:1352` `emit_domain_line(json)` | `src/cli.rs:1346` `let json = publish_customer(OutletId::C3, Product::CustomerJson(&cv))` | Json / Customer / Whole |
| C4 | `ontology overview` → 文本 | `src/cli.rs:1386` `emit_domain_text(text)` | `src/cli.rs:1382` `let text = publish_customer(OutletId::C4, Product::Overview(&ov))` | Text / Customer / Whole |
| C5 | `ontology overview --json` | `src/cli.rs:1380` `emit_domain_line(json_text)` | `src/cli.rs:1376` `let json_text = publish_customer(OutletId::C5, Product::Overview(&ov))` | Json / Customer / Whole |
| H1 | `GET /` 首页 | `src/server/http.rs:344` `Html(rendered.into_text())` | `src/server/http.rs:341` `pipeline::render(OutletId::H1, Product::Text(html))` | Html / Customer / Whole |
| H2 | `GET /api/graph` | `src/server/http.rs:200` `Json(rendered.into_value())`（`publish_json` 里那一行） | `src/server/http.rs:280` `publish_json(OutletId::H2, …)` → `pipeline::publish_value` | Json / Customer / Whole |
| H3 | `GET /api/node` | 同上（同一处 `publish_json`，出口不同） | `src/server/http.rs:305` `publish_json(OutletId::H3, …)` → `pipeline::publish_value` | Json / Customer / **PerNode** |
| M1–M5 | 5 个 MCP 工具 | `src/mcp/serve.rs:321` `p::encode_result(id, p::tool_call_result(…))` | `src/mcp/serve.rs:315` `pipeline::publish_value(OutletId::McpTools, value)`（**同一调用点**） | Json / Customer / Whole |

> 机读版是 `src/domain/pipeline.rs` 的 `pub const OUTLETS: &[Outlet]`（33 条，带 `path` + `call_site` needle）。
> 第 1 轮只有「表 → 源码」这一个方向；**第 2 轮加了反向**（`tests/test_req107_source_enumeration.rs`），
> 逐条核 needle 的三条判据仍在 `tests/test_req107_outlet_pipeline.rs`。

### 1.2 33 条口径（分组 13 / 8 / 9 / **2** / 1）

| 组 | 条数 | 轨道 | 执行形态 | 载体 |
| --- | --- | --- | --- | --- |
| C1–C5 | 5 | Customer | Pipeline | `cli.rs` |
| H1–H3 | 3 | Customer | Pipeline | `http.rs` |
| M1–M5 | 5 | Customer | Pipeline（同一调用点） | `serve.rs` |
| D1 | 1 | Modeling | Pipeline（`Track::Modeling` ⇒ 不过闸） | `cli.rs:1254` |
| D2–D8 | 7 | Modeling | DeclaredOnly（登记 + 站点标注，未改道） | `cli.rs` / `serve.rs` |
| 内部 9 | 9 | Internal | InternalOnly（登记 + 站点标注） | `cli.rs` |
| Z2、Z3 | 2 | Static | Pipeline（AC-008 选运行时闸） | `serve.rs:284` / `:252` |
| Z1 | 1 | ErrorPath | Pipeline（`safe_error`） | `http.rs:251` |
| **合计** | **33** | | | |

**Z4**（`INDEX_HTML` 模板）**不计入出口**（AC §七 A1），单列 `STATIC_TEMPLATES`；
被 **H1 的整页运行时扫描** + **测试**双重覆盖（`tests/test_req107_static_texts.rs::…z4_…`）。

---

## 2 · 管线设计（AC-010-2）

### 2.1 类型与签名

> ⭐ **第 2 轮改动（QA B2 / A11 R-B）**：出口只传 `OutletId`，轨道由 `spec_of(id)` **查表**；
> `OutletSpec` 全字段私有、`spec_of` 模块私有、`publish_error_text` **已删除**。
> 下面是**当前**的签名（第 1 轮的版本见 git 历史与 §10.3）。

```rust
// src/domain/pipeline.rs
pub enum Track { Customer, Modeling, Internal, Static, ErrorPath }
impl Track { pub const fn gated(self) -> bool { matches!(self, Track::Customer | Track::Static) } }

pub enum Format { Html, Json, Text }
pub enum Granularity { Whole, PerNode }

/// 出口的**唯一身份**（第 2 轮）：出口只能交代「我是哪条出口」。
pub enum OutletId { C1, C2, C3, C4, C5, H1, H2, H3, McpTools, Z1, Z2, Z3, D1 }

/// 声明本体：字段**全私有**，无公开构造器；`Outlet.spec` 也是私有字段。
pub struct OutletSpec { id: &'static str, track: Track, format: Format, granularity: Granularity }
/// ⛔ **模块私有**：出口拿不到任何 `OutletSpec` 的值（因此也改不了 track）。
const fn spec_of(id: OutletId) -> OutletSpec;

pub enum Product<'a> {
    CustomerHtml { view: &'a CustomerView, sampling: &'a [String] },
    CustomerJson(&'a CustomerView),
    Overview(&'a Overview),
    Text(String),
    ErrorText(String),
}

/// 文本轨唯一入口（CLI 客户出口 / 首页 / 建模投影 / 错误文案）。命中 ⇒ Err；**不自己 exit**。
pub fn render(id: OutletId, product: Product<'_>) -> Result<OutletRender, PipelineError>;
/// 结构化轨唯一入口（HTTP / MCP）。命中 ⇒ Err，消息逐字取自 mcp::tools::guard。
pub fn publish_value(id: OutletId, value: Value) -> Result<GuardedValue, PipelineError>;
/// 装配期前处理（/api/graph /api/node / MCP 五工具共用一份已剔视图）。
pub fn prepare_view(view: &CustomerView) -> FilteredView;

/// ⛔ 字段全私有、无公开构造器 —— 出口只有拿到它才能写出（AC-003 的类型层）。
pub struct OutletRender { text: String, report: DropReport, outlet: &'static str }
pub struct GuardedValue  { text: String, value: Value }

/// 第 2 轮：客户字节的写出函数**只收** `OutletRender`（裸 `&str` 编译不过）。
pub fn emit_domain_text(rendered: OutletRender);   // cli.rs
pub fn emit_domain_line(rendered: OutletRender);   // cli.rs
```


### 2.2 为什么这样切

| 切法 | 理由 |
| --- | --- |
| **「前处理」与「闸」分成两个阶段** | 判据不同（标识符形态 vs 建模术语）、命中后的动作不同（减一条 vs 整份不发）、失败语义不同（静默删条目必须报 stderr vs 拒绝发布）。顺序不可交换：先剔后闸才能让可减的条目减掉、不可减的仍到达闸门（REQ-104 的全部价值建立在这个顺序上）。 |
| **轨道由 `OutletId` 查表、`gated()` 在管线里判** | 「拦不拦」从「出口自觉」变成「身份 + 单一查表点」。第 2 轮把 spec 从出口手里拿走（QA B2）：出口连一个可写轨道字段都没有，误标只能通过「换 ID」，而那会被 `OUTLET_ID_TOKENS` 与 `pairing_ok` 两层抓住（§10.3）。 |
| **包装类型 + 私有构造器** | 让「不经过管线就拿着一个可写出的产物」**表达不出来**。这是唯一能挡住「换一种写法绕过」的判据（源码否定式只能挡拼写）。 |
| **`render` 返回 `Result` 而**不** `exit`** | 判定（拦不拦）与政策（退出码 / 500 / `isError`）分开：CLI 的 `Err → exit 2` 只在 `cli::publish_customer` 一处，出口函数体里一条判红分支都没有（TC-002 直接扫函数体）。 |
| **`publish_value` 复用 `mcp::tools::guard` 本体** | 纪律 H：不抄第二份判据。命中消息因此**逐字**等于收口前，500 / `isError` 正文零变化。 |
| **报告形状收进 `DropReport::stderr_line()`** | 卡点③：客户正文只报数量、运维 stderr 报原名，但**只有一种形状**（抬头常量 `DROP_REPORT_PREFIX`）。 |

### 2.3 type-layer doctest（`cargo test --doc` 实测）

```
test src\domain\pipeline.rs - domain::pipeline (line 27) - compile fail ... ok
test src\domain\pipeline.rs - domain::pipeline (line 39) - compile fail ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

---

## 3 · 三个卡点的裁定（AC-010-3 / AC-004）

> 完整论证在 `.feature/design-req107.md` §三；这里给结论 + 代价。

### ① 改名口径：**JSON 保持原名（不改名）**

- 理由：JSON 的名字是**标识**不是文案 —— `/api/graph` 的 `nodes[].id`、`/api/node?id=`、MCP `describe_object(name)` 全部拿同一个名字做键（`http.rs` 的节点索引直接取自 `graph.nodes[i].key`）；改名会让 13 个出口字节全变（与 AC-005 冲突），且 `sanitize_display` 的「摘 ID / 压空白 / 空括号」在数据通道上是**有损**的。
- **被放弃的方案（统一改名）会漏什么**：寻址面断裂 —— 客户在图上点一个节点 → `/api/node` 404；MCP `describe_object("客户")` 查不到；同一份数据第二次分裂成「HTML 名 / JSON 名」，比原来更难对账。

### ② 闸粒度：**双粒度，但由 `Granularity` 声明**

- 理由：`/api/node` 的语义单位就是一个节点；聚合视图（graph / show JSON / MCP）没有节点边界。
- **被放弃方案 A（统一整份）**：`/api/node` 一个坏节点 ⇒ 整页 91 个节点全 500 —— **REQ-104 修掉的那个事故当场回退**（不是理论风险）。
- **被放弃方案 B（统一逐节点）**：聚合视图只能靠「悄悄删掉一个节点」= 图与节点索引数量不一致（REQ-093 的 P1）+ 客户看不见自己少了什么；且 `guard` 扫的是整份序列化文本，分段扫描看不见**跨节点上下文**的泄漏（`A` 节点名 + `B` 节点说明拼起来才成立的那种）。
- 行为判据：`tests/test_req107_guard_granularity.rs::…cardpoint2_…`（同一份视图：`/api/graph` 500 · 干净节点 `/api/node` **200** · 脏节点 500 —— 三个断言缺一不可）。

### ③ stderr 报告形状：**两级（客户侧数量 / 运维侧原名）+ 形状唯一**

- 裁定：正文只报数量（页面既有措辞一字未动）；stderr 报原名（运维要定位）；所有出口只经 `DropReport::stderr_line()` 产出文本，抬头常量 `DROP_REPORT_PREFIX = "⚠ 发布闸前处理"`。
- **被放弃方案（统一只报数量）会漏什么**：运维失去定位能力 —— 整份拒发时 stderr 是**唯一**能看到「是哪个属性的名字触发的」的地方。
- **被放弃方案（连正文也报原名）会漏什么**：正文重新泄漏技术串，且与 `BA/verify-page.js` 的黑名单冲突。
- 行为判据：`…guard_granularity.rs::…cardpoint3_…`（正文含「另有 1 项」且不含原名；`FilteredView::report()` 与 `DropReport::stderr_line()` 都带原名）。

---

## 4 · 漏接即红的实测（AC-010-4 / AC-003）

### 4.1 三层判据（哪一层管什么）

| 层 | 判据 | 抓得住 | 抓不住 |
| --- | --- | --- | --- |
| 类型层 | `OutletRender`/`GuardedValue` 私有字段 + 无公开构造器 + 2 条 `compile_fail` doctest | 不经过管线就拿到可写出的产物 | 出口自己拼 `String` 直接 `println!` |
| 源码否定式 | 3 个出口文件的 6 个判据名直调点数 == 0（`test_req107_outlet_files_have_zero_criterion_call_sites` / `…m3_…`） | 出口偷调判据（**含 M3 的「保留共享调用 + 另加本地判据」**） | 全新命名的自造判据 |
| 登记表对账 | 33 条 `OUTLETS` 双向对账 + 既有 G-5（`to_string_pretty` 等 producer 出现在未登记 `println!` ⇒ 红） | **新增一个手搓的客户出口**（TC-012 的形态） | 不经过 producer 的新写法 |

### 4.2 变异三元组（纪律 G：二进制 sha256 + patch sha256/内容 + 逐条结果）

> ⚠️ 全部变异都**编译成功**（cargo 输出 0 error）—— 编译失败不算执行。
> 源码扫描型判据另取**被扫文件的工作区 sha256**（纪律 Q）。

**M-1 · 摘掉 C3 的管线接线**（`src/cli.rs`，模拟「漏接一个出口」）

| 项 | 值 |
| --- | --- |
| patch | `.review-workspace/req107/mutations/M-1.patch` |
| patch sha256 | `3c0202f6f870578ebcd04834efb4d0145e3c7aed0b2df3ba97adbdc4f48bf43f` |
| 被扫文件 sha256（变异期） | `916efaab37bfdcc35e8ee6dc9a7c95af19c8fbbe20fe4731448371ba9c3b1880` |
| 测试二进制 sha256 | `test_req107_outlet_pipeline-7ae0761fe6923be9.exe` = `af16601d2c1432da1028b0ec0f9bb9b7a253d290b1a3ade6abae34ed4e4c7fa4`；`test_binary_output_guard-182314fe579f06fe.exe` = `efad480edf39ef7e1342dd6faa116b48a1d51ed41bfcae40bc2c36898a102baf` |
| 结果 | `test_req107_outlet_pipeline`: **9 passed / 3 failed**（`cli_five_customer_outlets_are_wired`、`removing_an_outlet_breaks_the_registry`、`a_hand_rolled_new_customer_exit_would_be_caught`）<br>`test_binary_output_guard`: **4 passed / 3 failed**（`g1_…`、`g4_…`、`g5_…`） |
| 结论 | 摘掉一条接线 ⇒ **6 条判据同时红**。复原后 `cli.rs` sha256 = `fe6c0a9d…`（与变异前一致）。 |

**M-2 · ⭐ M3 形态：保留管线调用 + 另加一份本地判据**（`src/cli.rs` 插一行 `let leaked = scan_modeling_terms(&json);`）

| 项 | 值 |
| --- | --- |
| patch sha256 | `942342cdfd6e22b72195d4841860ec51ffc3a1b2d0bd5761aff5e47cb6d50dad` |
| 被扫文件 sha256（变异期） | `91245d6b5e8aa66abd208f49e8a47b2e86f0ccc786c8b6a6c0361364b0bd3fec` |
| 测试二进制 sha256 | `outlet_pipeline` = `54d03bfefc0de85c6dcfe368b90f833ff268a978a8252953e1a42079a1528ee8`；`test_property_filter` = `fbf2c4e32ee354af590771f1dc7427a77eef877ea10fbeb4ee3e68036ba1a401`；`binary_output_guard` = `1fa46e2693dab5ae90a15bd980e34ab0664f8809e207bf02bc841c90dc4884e1` |
| 结果 | `outlet_pipeline`: **9 / 3 failed**（`outlet_files_have_zero_criterion_call_sites`、`m3_keeps_shared_call_and_adds_local_criterion`、`no_second_judgement_outside_pipeline`）<br>`test_property_filter`: **9 / 1 failed**（`all_three_outlets_call_the_one_shared_function`）<br>`binary_output_guard`: **5 / 2 failed**（`g1_…`、`g4_…`） |
| 结论 | **这就是 QA 变异 M3 在收口前实测「9 条全绿」的那一格** —— 现在 **6 条同时红**。这是本需求 AC-003 的直接证据。 |

**M-3 · `tools/list` 的 `description` 里塞「外键登记」**（`src/mcp/tools.rs`）

| 项 | 值 |
| --- | --- |
| patch sha256 | `a4d75d6878e6cd5946f830523b7d4c979dd9c491e619ccd3601c079ea01abde1` |
| 被扫文件 sha256（变异期） | `fdbdcfd670c3fb8085855e2bd39bdb161f1bad90e443f6b85a11f82387551c1b` |
| 测试二进制 sha256 | `test_req107_static_texts-3df4c56fae816dab.exe` = `7e048a9067009e536c472aacd0e8a74266c782e94b7230886e524cbb3840c87a` |
| 结果 | `test_req107_static_texts`: **2 / 1 failed**（`tools_list_descriptions_and_titles_are_asserted`）；对照组 `test_req107_outlet_pipeline`: **12 / 0**（证明判据不串味） |
| 复原 | `tools.rs` sha256 = `d076c3d438e8b8b08c98c3d4ca9d7d3f013061436dfb663974708ec8dd2fe3fe`（前后一致） |

**M-4 · `instructions` 模板正文里塞「外键」**（`src/mcp/tools.rs`）

| 项 | 值 |
| --- | --- |
| patch sha256 | `f556a66d816253c6d924457e6e36bb6897f8ba16a45c3a60f05fce476a9ccfd8` |
| 被扫文件 sha256（变异期） | `745abbb615f8376c706c7ebe962e7a6914c2fa75223d0b40b68fbb5731465bb5` |
| 测试二进制 sha256 | `3af3b4c3e1bcb1600b7a130d11efcd10677d7b905a72cf509353ac923c5b01ab` |
| 结果 | `test_req107_static_texts`: **2 / 1 failed**（`initialize_instructions_template_body_is_guarded`） |
| 复原 | `tools.rs` sha256 回到 `d076c3d4…`（前后一致） |

**M-5 · 类型层（两步，含一次「判据自身有漏洞」的发现）**

| 变异 | patch sha256 | 被扫文件 sha256（变异期） | 结果 |
| --- | --- | --- | --- |
| M-5a：只把 `text` 改成 `pub` | `81a5ab959e57a4c7a3b736c509ca863ee419da3c14f8525c5a36b4c269d11793` | `17c5d572c408c67b872b49a4d6e68f1a1b05a547b7ad62095f85fe9c3f53b8fb` | `cargo test --doc`: **4 passed / 0 failed**（⛔ **没红**） |
| M-5b：把**三个字段全**改 `pub` | `8f444654986ad5219857b6b294930524a0d3785de75c7908fc0094dcaeacd7b3` | `9aa40675428bb3f89b09f7352c1345581ce31ed7cf66abb1dbde31386ca3c519` | `cargo test --doc`: **3 passed / 1 failed**（`pipeline.rs (line 27) compile fail` **FAILED**） |

> ⚠️ **M-5a 是本轮最有价值的一次变异**：它证明我第一版 doctest **是弱的** ——
> 那条 doctest 只写了一个字段（`OutletRender { text: … }`），所以即使把 `text` 改成
> `pub`，它仍会因「缺字段」而编不过 ⇒ `compile_fail` 照样「通过」，但**证明的不是私有性**。
> 已改成**三个字段全写**（`text` + `report` + `outlet`），M-5b 因此能红。
> 这正是 TC-008 警告的形态（「compile_fail 只能证明这一段编不过，不能证明原因」）。

**M-6 · 登记表误标：D1 的轨道 `Modeling → Customer`**（`src/domain/pipeline.rs`）

| 项 | 值 |
| --- | --- |
| patch sha256 | `40915121ac6271e907b0722e58c6d01270c4bfa6f7c82257ba7c10f423975b2a` |
| 被扫文件 sha256（变异期） | `7c710ef130591637cc36d0f1ba28209e5f4d4f5fb824600a7ebcd9a56c52e4d5` |
| 结果 | `test_req107_track_manifest`: **0 / 2 failed**（`modeling_track_is_explicitly_declared`、`mislabeling_a_modeling_outlet_turns_it_red`）<br>`test_req107_outlet_pipeline`: **11 / 1 failed**（`outlet_registry_matches_the_33_baseline`） |
| 复原 | `pipeline.rs` sha256 = `ae9508a34321af0988ac7f4a82b92f6a028ac54f00c82d872198c13bf70717bd`（与变异前一致） |

---

## 5 · 零可见变化（AC-010-5 / AC-005）

### 5.1 比对方式（可复现）

```powershell
# ① 基线 worktree（只读基线，临时目录在 .review-workspace/req107/）
cd D:\MyCodes\AI\an_onto_demo
git worktree add --detach .review-workspace\req107\base b714f48
Copy-Item feature-REQ-107\tests\common\req107.rs .review-workspace\req107\base\tests\common\
Copy-Item feature-REQ-107\tests\test_req107_zero_visible_change.rs .review-workspace\req107\base\tests\
cd .review-workspace\req107\base; git status --short   # 期望：只有 2 行 ??（实测：只有 2 行 ??，无 M）

# ② before（基线代码产出）
$env:REQ107_OUT = "...\.review-workspace\req107\before2"
cargo test --test test_req107_zero_visible_change -- --nocapture req107_write_golden

# ③ after（收口后代码产出）
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-107
$env:REQ107_OUT = "...\.review-workspace\req107\after"
cargo test --test test_req107_zero_visible_change -- --nocapture req107_write_golden

# ④ 逐字节 diff
cd D:\MyCodes\AI\an_onto_demo
git diff --no-index --stat .review-workspace\req107\before2 .review-workspace\req107\after   # 实测：无输出，exit 0
```

**实测结果**：`git diff --no-index --stat before2 after` **无输出**（exit 0），13 份文件**逐字节相同**。

入库存档的是 `before2/`（= 基线产出）：
`tests/fixtures/req107/golden/{C1_html.txt, C3_json.txt, C4_overview.txt, C5_overview_json.txt,
H1_index.html, H2_graph.json, H3_node_1.json, H3_node_2.json, M1..M5.json}` +
`PROVENANCE.txt`（第一行 `b714f48`）+ `SHA256SUMS`（13 行，测试与内容互校）。

### 5.2 常态判据（不需要数据库，本机可跑）

| 判据 | 结果 |
| --- | --- |
| `tests/test_req107_zero_visible_change.rs` | **3 passed**（含 `SHA256SUMS` 互校、`PROVENANCE` 来源钉死、13 份逐字节、C1>200 字节防「双方都空」） |
| `tests/test_req107_pipeline_bytes.rs` | **1 passed**（**管线产出**逐字节 == 同一份 golden —— 把生产钉在 golden 上） |

> ⚠️ 两条判据并存是刻意的：前者基线兼容（要能在 `b714f48` 跑生成器）所以用生产函数**复刻**，
> 后者跑在收口后的代码上直接调**管线**。只留前者会被「复刻与生产分叉」骗过（纪律 H）。

### 5.3 唯一允许的变化：Z1（AC §七 A2）

| 出口 | 变化前 | 变化后 | 理由 |
| --- | --- | --- | --- |
| Z1 `GET /api/graph?project=wrong&version=2`（服务端绑定名 = `外键登记表`） | `{"error":"本服务只对外提供 外键登记表 第 2 版这一份内容，别的一概不给。"}` | `{"error":"这一问给不出答案：输入的内容里出现了只有内部使用的术语（为免泄漏，此处不列出具体词条）。"}` | AC-006：项目名不得从错误响应漏出（同一名字在首页已被拒发） |

> ⚠️ 上表两段正文是**按代码里的字面文案逐字推得**的（`format!` + `json!` 的 compact 序列化），
> 不是本机截获的原始字节：断言它的是 `tests/test_req107_scope_error_gate.rs`（400 + 不含项目名 + 含 `error`），
> 而**不是**一段写死的字节 golden（否则改文案就得改测试，那会把「变化」钉死成「不许改」）。

对照组（TC-019）：服务端绑定名是干净的 `样例项目` 时，**文案一字未变**（仍回显项目名）——
修复没有退化成「一律换成通用文案」。

### 5.4 不属 AC-005 口径、但**如实登记**的变化：stderr 报告形状

> AC-005 的 golden 比的是 **stdout / 响应体**；stderr 是 AC-004 卡点③ 明确要求「统一形状」的那个面。

| 出口 | 变化前（stderr） | 变化后（stderr） | 理由 |
| --- | --- | --- | --- |
| C1/C2（HTML） | 两行：`⚠ 有 N 个对象名带建模用语，没上页面：…` + `⚠ 有 N 条属性带技术用语，没上页面（前 20 条）：…` | 一行：`⚠ 发布闸前处理：共剔除 N 项（对象名 a · 整条属性 b · 只隐去说明 c · 对象说明 d）。明细（前 20 条）：…` | 卡点③：同一条 stderr 原来有三个措辞，运维 grep 不到同一个抬头 |
| C3（JSON） | `⚠ 有 N 条属性带技术用语，没进客户视图（整条剔除 a · 只隐去说明 b）；另有 …`（`property_filter::report()`） | 同上统一形状 | 同上 |
| H2/H3/M（装配期） | `property_filter::report()` 的原文 | 同上统一形状 | 同上 |
| 全部客户出口（闸命中时） | 三个不同措辞（`⚠ 客户视图页面泄露了建模术语：…` / `⚠ 客户视图泄露了建模术语：…` / `⚠ 业务总览泄露了建模术语：… / 文本 …`） | `⚠ 客户视图泄露了建模术语（出口 {id}）：{hits:?}` | 同上（命中清单**仍然只进 stderr**，REQ-106 AC-002 未被破坏） |

**内容口径的一处**变化（第 2 轮按 QA §6.3 订正）：对象名明细**从「不限条数」变成与前 20 条共享上限**
（`pipeline.rs::DropReport::stderr_line` 的 `SHOW = 20`）。原名、四类计数、`其余 N 条省略` 都保留了，
但**条数上限确实收紧了** —— 上一版写「内容没有减少」不准确，这里如实改成「明细上限统一为 20，
超出部分只报数量」。影响面只限 stderr（AC-005 口径外）。

---

## 6 · 全量数字 + 口径 + 本机限制（AC-010-6）

### 6.1 口径（必须分开写，A5）

| 口径 | 值 | 本机可否复核 |
| --- | --- | --- |
| **代码基线** | `develop` `b714f48` | ✅ |
| **全量测试基线** | `62 suite / 846 passed / 0 failed / 1 ignored @ develop 7f26fa2` | ❌ **不可复核** |
| 本机 `cargo test --lib` | **167 passed / 2 failed / 0 ignored** | 2 条失败**全部**是要 `ONTOK_TEST_DATABASE_URL` 的（`db::apply::tests::write_snapshot_in_maps_unique_conflict_to_db_not_key_conflict`、`extract::introspect::tests::read_only_pool_really_reads_back_on`）⇒ 环境不具备，**不是回归** |
| 本机 `cargo test --doc` | **4 passed / 0 failed** | ✅ |
| 本机 `cargo test --test <name>` | 见 §6.2 | ✅（需 §6.3 的绕行） |

⛔ **`cargo test`（全量）在本机编不过**，原因是**既有代码**：`src/bin/ontok-gate-preflight.rs`
使用 `std::os::unix::fs::MetadataExt`（`dev()` / `ino()`，`:587-588` / `:1514`）——
非本需求引入，**本需求不许去改它**。⇒ 全量 `62/846` 的数字**本报告不声称复核过**。

### 6.2 本机实测（逐条命令 + 结果）

| 命令 | 结果 |
| --- | --- |
| `cargo check --lib` | `Finished dev profile`（0 error） |
| `cargo test --doc` | **4 passed / 0 failed / 0 ignored**（其中 2 条是本需求的 `compile_fail`） |
| `cargo test --lib` | **167 passed / 2 failed / 0 ignored**（2 条需 DB，见上） |
| `cargo test --test test_req107_outlet_pipeline` | **12 passed / 0 failed** |
| `cargo test --test test_req107_guard_granularity` | **4 passed / 0 failed** |
| `cargo test --test test_req107_zero_visible_change` | **3 passed / 0 failed** |
| `cargo test --test test_req107_pipeline_bytes` | **1 passed / 0 failed** |
| `cargo test --test test_req107_scope_error_gate` | **4 passed / 0 failed** |
| `cargo test --test test_req107_track_manifest` | **2 passed / 0 failed** |
| `cargo test --test test_req107_static_texts` | **3 passed / 0 failed** |
| `cargo test --test test_req107_red_lines` | **2 passed / 0 failed** |
| `cargo test --test test_property_filter` | **10 passed / 0 failed**（收口前 9 条 + 新增自检 1 条） |
| `cargo test --test test_exit_ordering_req106` | **7 passed / 0 failed** |
| `cargo test --test test_binary_output_guard` | **7 passed / 0 failed** |
| `cargo test --test test_graph_server` | **9 passed / 0 failed** |
| `cargo test --test test_guard_manifest_req081` | **6 passed / 0 failed** |
| `cargo test --test test_anchor_guard` | **3 passed / 0 failed** |
| `cargo test --test test_read_only_guard` | **5 passed / 0 failed** |
| `cargo test --test test_overview` | **17 passed / 0 failed** |
| `cargo test --test test_view` | **26 passed / 0 failed** |
| `cargo test --test test_model_view` | **10 passed / 0 failed** |
| `cargo test --test test_diff` | **13 passed / 0 failed** |
| `cargo test --test test_no_write_path` | 3 passed / **1 failed**（`test_store_connects_to_exactly_one_database`：需 DB） |
| `cargo test --test test_mcp_server` | 6 passed / **25 failed / 1 ignored** —— **全部**是 `tests/common/mod.rs:225` 的 `MissingEnv("ONTOK_TEST_DATABASE_URL")`（`frozen_context()` 要真库）。⇒ 环境不具备，**不是回归**；本轮对 `serve.rs` 的改动由 `test_req107_*` 的无 DB 判据覆盖 |
| `cargo test --test test_retained_names_req084` | **⛔ 本机不可构建**：它 `include_str!("../src/bin/ontok-gate-preflight.rs")`，而那个 bin 的 unix-only 代码在 Windows 编不过；**任何** integration test 都会因 cargo 构建全部 bin 而失败（见 §6.3） |
| `cargo test --test test_domain_print_sites_binary_req078` / `test_exit_ordering_req106` 之外的真进程 e2e | **需 DB，不可复核**（TC-017 同类） |

### 6.3 ⚠️ 环境事实的一处**更正**（纪律 E）

派单材料写「`cargo test --test <name>` ✅ 可跑」。**实测不成立**：
cargo 在跑任何 integration test 时都会构建**全部 bin target**，而
`src/bin/ontok-gate-preflight.rs` 的 `std::os::unix` 代码在 Windows 编不过 ⇒

```
error[E0599]: no method named `ino` found for struct `Metadata`
  --> src\bin\ontok-gate-preflight.rs:588
error: could not compile `ontok` (bin "ontok-gate-preflight") due to 5 previous errors
```

本轮的绕行（**不改任何被跟踪文件**）：跑测试期间把那个 bin 文件**临时挪到**
`.review-workspace/req107/stash/`，跑完**立即挪回**，并用 sha256 前后互校：

```
bin sha256 before = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
bin sha256 after  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
restored byte-identical: True
```

⇒ **QA 若在本机按「`cargo test --test <name>` 可跑」执行 29 条用例，会全部构建失败。**
这不是本需求引入的，但会直接挡住验收，必须先说清（已列入 §9 异议）。

---

## 7 · `git diff --numstat` + 红线零触碰（AC-010-7）

### 7.1 提交（第 1 轮 **7 个** `[Dev]` 提交 + 1 个 `[BA]` 登记提交；**第 2 轮** 1 个 `[Dev]` 提交（见 `git log`））

```
6ac37d6 [Dev] golden 字节比对（基线 b714f48 生成 + 管线产出对账）与夹具 (关联: REQ-107)
ee3399a [Dev] 结构判据 + 变异验证（否定式计数 / 类型层 doctest 三字段全写 / 33 条对账 / 三卡点行为） (关联: REQ-107)
a1c9263 [Dev] 迁移三条既有守卫（否定式改写：property_filter / REQ-106 时序 / G-1 pipeline 族） (关联: REQ-107)
1eed1d6 [Dev] 13 条客户出口接线 + 33 条出口登记表 + Z1/Z2/Z3 运行时闸 + 建模轨显式标注 (关联: REQ-107)
ccb3e87 [Dev] 出口管线骨架 + 类型层约束（OutletRender/GuardedValue 构造器私有 + compile_fail doctest） (关联: REQ-107)
```

### 7.2 `git diff --numstat develop -- src tests`（节选，完整 34 个文件）

```
125  116  src/cli.rs
10   1    src/domain/diff.rs             ← 只改注释（R2：口径改为「建模人员」）+ 附 R1/R3 理由
3    0    src/domain/mod.rs
1144 0    src/domain/pipeline.rs          ← 新增（管线本体）
12   0    src/domain/property_filter.rs   ← 只追加「出口侧守卫」一段（①–③ 一字未动）
41   13   src/mcp/serve.rs
74   23   src/server/http.rs
153  0    tests/common/mod.rs
169  0    tests/common/req107.rs
266  38   tests/test_binary_output_guard.rs      ← 迁移（§7.3）
66   52   tests/test_exit_ordering_req106.rs      ← 迁移（§7.3）
95   14   tests/test_property_filter.rs           ← A3 改写（§7.3）
986  0    tests/test_req107_outlet_pipeline.rs
395  0    tests/test_req107_zero_visible_change.rs
344  0    tests/test_req107_guard_granularity.rs
258  0    tests/test_req107_static_texts.rs
189+ 0    tests/fixtures/req107/golden/**（13 份 golden + PROVENANCE + SHA256SUMS）
…
35 files changed, 5347 insertions(+), 257 deletions(-)
```

### 7.3 ⚠️ 被改写的**三条**既有测试（A3 只点名了第 1 条）

| # | 文件 | 为什么必须改 | 改法 / 有没有放宽 |
| --- | --- | --- | --- |
| 1 | `tests/test_property_filter.rs` `all_three_outlets_call_the_one_shared_function` | **A3 已裁定** | 正向（「三个文件 contains 判据名」）→ **否定式**（6 个判据名直调点数 == 0 + 必须有管线调用点 + 管线里必须有判据调用防空转）。**更严**：M-2 变异下旧判据绿、新判据红 |
| 2 | `tests/test_exit_ordering_req106.rs` `the_json_exit_judges_before_it_writes` / `…writes_outside_the_refusal_branch` | 它们钉的是 `src/cli.rs` 里的 `let leaked = scan_modeling_terms(&json);` 这一行；**AC-001 要求出口文件里没有判据直调 ⇒ 该锚点必须消失** | 锚点**迁移**到 `src/domain/pipeline.rs`：「闸在产物构造之前」「产物不嵌在拒发分支里（按缩进）」两条断言原样保留。**意图不变、位置随代码移动** |
| 3 | `tests/test_binary_output_guard.rs` G-1 / G-4 | 4 个 `same_binding` 站点的判据与写出由「两条语句」变成「管线一次产出」⇒ 旧判据会报「找不到判据调用」（结构性红，不是回归） | 新增 `WIRING_PIPELINE` 接线形态，判据换成**三条结构式**：① 管线入口在不在 ② 出口函数体里判据直调是否为 0 ③ 写出绑定是否从产物派生。G-4 的逐站变异换成 M-P1/M-P2/M-P3（摘管线 / 加本地判据 / 自拼产物），**每站三个变异都必须红** + `same_binding` 族为 0 被显式登记 |

**没有放宽的证据**：三条都是「把锚点跟着代码搬」或「从正向改否定式」；
新增的断言条数分别 +1 / 持平 / +3 个变异；`test_property_filter` 从 9 条变 10 条（多一条剥注释自检）。
⛔ 一条测试都没删、没有 `#[ignore]`、没有把 `assert_eq!` 改成 `assert!`。

### 7.4 红线零触碰（判据 `tests/test_req107_red_lines.rs`，**2 passed**）

| 项 | 结果 |
| --- | --- |
| `git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs` | **空** |
| `git diff develop -- src/mcp/tools.rs` 的增删行含 `fn guard(` / `已拒绝外送` / `scan_modeling_terms(&text)` | **无**（该文件本轮**零改动**；变异 M-3/M-4 是临时变异，已复原为 sha256 `d076c3d4…`） |
| `property_filter.rs` 的 G1–G4 三段（①→②、②→③、③→「以上三条」）与 `develop` 版 | **逐字节相等** |
| `tests/` 新增 `#[ignore]` | **无** |
| `git diff --diff-filter=D --name-only develop -- tests` | **空**（没删测试文件） |

### 7.5 判据自身的一处假红（如实登记，纪律 E）

`test_req107_red_lines.rs` 第一版把「新增行里出现字面量 `#[ignore]`」当判据 ——
于是**它自己的注释与代码**（以及 `test_req107_zero_visible_change.rs` 里
「刻意不用 `#[ignore]`」那句话）全被算成「新增了 ignore」⇒ 判据**恒红 = 恒假**。

已改成：新增行去掉 `+` 与空白后**必须以 `#[ignore]` 开头**（只认真正的属性），
并补了正/负样本自证（`+    #[ignore]` 必须命中；注释里提到它必须不命中）。
⇒ 「守卫把自己判红」与「守卫认不出目标」这两种病都留了一条可执行的证据。

---

## 8 · 已知限制（AC-010-8）—— 本需求**没有**做什么

1. **D2–D8 没有改道经管线写出**（只登记 + 站点标注）。理由：D2 走 `emit_domain_text_with` 的闭包接线（REQ-072/078 的类型级保证），D3–D8 是人话行且被 `DOMAIN_PRINT_SITES`/G-1/G-5 钉住；改道会同时打穿两套既有守卫。⇒ AC-007 的「显式声明」由 `OUTLETS` 的 `Track::Modeling` + 逐条 `exemption_reason` + 站点 needle 满足；**误标的行为可观察性**由 D1（真走管线）与 TC-022 的行为半证。
2. **内部 9 条只登记、不改写出路径**（AC §四 允许：「可不过闸，但必须在管线里显式标『内部』」）。
3. **Z4 模板没有单独的运行时闸**：它由 H1 的**整页扫描**（模板 + 已转义项目名 + 静态文案）覆盖 + 测试守卫。若将来出现「不经 H1 就读 `INDEX_HTML` 的出口」，这条覆盖就不成立（已登记）。
4. **`diff` 的换行不转义**（R3）**未修**：归建模轨，换行能进 stdout，登记为已知限制（BA 已裁定）。
5. **`overview` 的「两条格式都过闸」是既有过度拦截**，本轮原样保留（AC-005 要求零可见变化）。是否收窄（让 `overview` 只判文本、`--json` 只判 JSON）留给后续需求。
6. **`FilteredView::report()` 仍在**（`property_filter` 的公开 API）：出口已**不再调用**它（统一走 `DropReport`），但它作为 domain 层 API 保留，未删。⇒ 「报告形状唯一」的实现层判据是「出口侧 0 调用」，不是「全仓只有一份文本生成器」。
7. **stderr 报告形状的变化**（§5.4）不在 AC-005 的 golden 口径内，但**是**可见的变化面：若 BA/QA 认为 stderr 也属于「对外可见输出」，则 §5.4 那张表就是 AC-005 的「变化清单」，请按变更项审。
8. **真进程 e2e（TC-017）没有跑**：需要 `ONTOK_TEST_DATABASE_URL`，本机无 PG。⇒ 真进程层面的「零可见变化」**未在本机复核**。
9. **`cargo test` 全量没有跑**（既有 bin 的 unix-only 代码），`62/846` 这个数字本报告**没有复核**。
10. **AC-009-4「不许放宽断言」**：机器只能判「删测试 / 新增 `#[ignore]`」，判不了「`assert_eq!` 改成 `assert!`」。本轮**没有**这类改写，但这条**未封堵**（coverage-map §四-6 已登记）。

---

## 9 · 异议（AC-010-9）

### 9.1 对「只点名一条改写」的异议（**最需要 BA 拍板的一条**）

AC §七 **A3** 只裁定 `tests/test_property_filter.rs:341` 必须改写。
但 AC-001（出口文件里不得有判据直调）**在结构上必然**让另外两条既有守卫失效：

- `tests/test_exit_ordering_req106.rs` 的两条判据把锚点钉在 `src/cli.rs` 的那一行判据调用上；
- `tests/test_binary_output_guard.rs` 的 G-1/G-4 把「判据与写出是两条语句」当成前提。

我按 A3 的**同一套原则**（改写、不许删、不许放宽；锚点随代码迁移）处理了它们，
并逐条给出「改法 + 是否放宽」的论证（§7.3）。**若 BA 认为这超出授权，请在退回意见里明确** ——
替代方案只有两个，都更差：① 保留 `cli.rs` 里的判据调用（AC-001/AC-003 直接不成立）；
② 删掉那三条守卫（违反 AC-009）。

### 9.2 对 **33 口径**的异议（TC-007 自身相加是 34）

- 我**认** 33 这个口径（13 + 8 + 9 + 2 + 1）。
- ⛔ 但 `test-cases.md` 的 TC-007 写「`Static == 3`（Z2,Z3,**Z4**）+ ErrorPath 1」
  ⇒ 13+8+9+3+1 = **34**，与它自己断言的 `OUTLETS.len() == 33` **互相矛盾**。
- 我按 **A1 的裁定**（Z4 不是出口、不占出口位）取 `Static == 2`，Z4 单列 `STATIC_TEMPLATES`。
- ⇒ **请把 TC-007 的 `Static` 期望值改为 2**（否则 QA 照 TC-007 写会必红）。

### 9.3 对**环境事实**的异议（会直接挡住验收）

派单材料写「`cargo test --test <name>` ✅ 可跑」——**实测不成立**（§6.3）：
cargo 会为任何 integration test 构建**全部 bin**，而 `src/bin/ontok-gate-preflight.rs`
在 Windows 编不过。⇒ QA 的 29 条用例若按 `cargo test --test <name>` 执行会**全部构建失败**。
请给 QA 明确口径：要么用 §6.3 的「临时挪开 bin」绕行，要么在派单里承认「integration 测试本机不可执行、只能做源码级复核」。

### 9.4 对 **TC-001 / TC-009 的判据拼写**的异议

TC-001 的 `CRITERION_CALLS` 里有裸 `guard(`。实测它在 `src/cli.rs` 命中
**`install_broken_pipe_guard()`**（`:288` / `:738`）⇒ 判据**恒红 = 恒假**。
我用的针脚是 `tools::guard(` / `mcp::tools::guard(`（只认真正的判据调用）。
⇒ 请把 TC-001 的名单同步改掉（其余 5 个名字不变）。

### 9.5 对 **TC-008 的 compile_fail 判据**的补强建议

TC-008 的「代码块里出现包装类型名与一次字面量构造」这一格，**不足以**证明
「编不过的原因是字段私有」（M-5a 实测：只写一个字段时，把该字段改成 `pub` 也不会红）。
⇒ 已在 doctest 里**把三个字段写全**，并建议把「字段写全」写进 TC-008。

### 9.6 对 **Z4 / `Index_html`** 的裁定（回答 AC-010-9 的提问）

- Z4 **不占出口位**（认 A1）。
- Z4 **不单列运行时闸**，但**被 H1 的整页扫描覆盖**（模板 + 转义后的项目名 + 静态文案一次性扫）——这是「运行时闸」与「测试守卫」之外的第三条路，成本比单独扫模板更低、覆盖面更大。若 BA 要求「必须单列」，请明说，我按运行时闸再补一处。

### 9.7 其它（无异议，仅登记）

- R1/R2/R3（`diff` 归建模轨 / 改注释 / 换行不转义）：**认**，全部落地：
  - **R1**：`diff` 在 `OUTLETS` 里登记为 `Track::Internal`（内部轨、不过闸）；
  - **R2**：`src/domain/diff.rs:53` 的注释改为「给 CLI 与**建模人员**看」，并附上 R1/R3 的理由；
  - **R3**：换行不转义**未修**，登记在 §8-4。

---

## 10 · 第 2 轮修复（QA 3 条阻断 + A11 四条硬性要求）

> 作业单：`BA/demands/REQ-107/qa-review.md`（QA 在独立 worktree 做了 12 次变异）。
> 这一轮**只修阻断**，没有重写第 1 轮的管线 / 13 条接线 / golden / 红线成果。

### 10.1 结论：5 条全部消除，且都用**新名字 / 新路由 / 新出口**做了变异实测

| # | 阻断 / 要求 | 根因（QA 的原话） | 本轮的修法 | 判据 | 变异实测 |
| --- | --- | --- | --- | --- | --- |
| **B1** | AC-003 「漏接即红」 | `OUTLETS` 只做「表 → 源码」**单向** needle 对账 | **反向枚举**：扫 `src/` 全目录的写点，逐个对账登记表（剥注释 + 抹字符串） | `tests/test_req107_source_enumeration.rs::t1` | 攻击 #3 新增 `/api/raw` ⇒ **红**（T1+T5） |
| **B2** | AC-001 「拦不拦不由出口决定」 | `OutletSpec.track` 是 `pub` 字段 | 出口只传 **`OutletId`（enum）**；轨道由 `spec_of(id)` **查表**；`OutletSpec` 全字段私有、`spec_of` 模块私有、`Outlet.spec` 私有 | `t3` + `pipeline.rs:713` 的 `pairing_ok` | 攻击 #4a 字面写法 ⇒ **编译不过**（E0599）；等价可编译形态 4b ⇒ **红**（T3） |
| **B3** | A9 #2「原来能抓的现在抓不到」 | 出口侧「先写后判」失守 | 写出函数**只收 `OutletRender`**（裸 `&str` 编译不过）+ T2 要求实参是管线产物且**赋值在写出之前** + G-4 的 ③ 补上时序 | `cli.rs:645/650` + `t2` + `test_binary_output_guard::pipeline_site_verdict` ③ | 攻击 #5a 字面写法 ⇒ **编译不过**（E0308）；可编译形态 5b/5c ⇒ **红**（T1） |
| **B4** | 公开构造器只挡 `new` 拼写（R-C） | 判据匹配字符串 `pub fn new(` | 枚举**全仓所有 `pub fn`** 是否返回 `OutletRender`/`GuardedValue`（含 `-> Self` 的 impl 方法） | `t4`；`publish_error_text` 一并删除 | 攻击 #2 `pub fn craft(String)` ⇒ **红**（T4） |
| **B5** | 自造本地判据仍全绿 | 只查「有没有直调已登记的名字」 | 出口文件**逐函数登记**（`OUTLET_FNS`）+ 全仓 `mod` 登记（`MODULES`）+ T2 禁止产物被再次交给别的函数 | `t2` / `t5` / `t6` | 攻击 #1 `fn local_ok` + 真改写出 ⇒ **红**（T5+T2） |

> **与 QA 建议的一处偏离（如实登记）**：A11 R-D 建议「写出方法只在 `GuardedValue` 上提供」。
> 我实现成「`cli::emit_domain_text/line` 的**实参类型**是 `OutletRender`」——
> 拿裸 `&str` 调它们同样**编译不过**，强度等价，且不必把 stdout 写进 domain 层
> （`pipeline.rs` 现在仍然一行 stdout 都没有，由 `t7` 钉住）。

### 10.2 B1 · 源码 → 登记表（R-A）

新增 `tests/test_req107_source_enumeration.rs`（7 条判据，T1–T7）。**T1** 的做法：

1. `src_files()` **递归遍历 `src/`**（新文件自动进来，⛔ 没有写死文件清单）；
2. `strip_comments`（词法级，剥注释）→ 再 `blank_strings`（把**字符串字面量内容**抹成空格）。
   为什么多这一步：`src/cli.rs:396-402` 的退出码政策是一段**多行字符串**，里面原样写着
   `emit_domain_text_with(&text.replace(..), |judged| …)` 作为**例子** —— 不抹掉就会把
   文档示例当成真实写点（假红）。两个视图**行号严格对齐**（本轮踩过：转义符吃掉换行会让
   二者错位 17 行，`write_points` 因此改成先定位行、再按括号配平拼窗口）；
3. 枚举写点：`println!` / `print!` / `emit_domain_text` / `emit_domain_line` /
   `emit_domain_text_with` / `write_stdout` / `Json(` / `Html(` / `fs::write(` /
   `encode_result(` / `tool_call_result(`（词边界判定，`eprintln!` 不算 `println!`）；
   写出机制**本体**（`emit_domain_*` / `write_stdout` / `encode_result` 的定义体）整段跳过；
4. 每个写点必须：① 该行落在 `OUTLETS.call_site` 的 needle 集合里，**或**
   ② 落在 `NON_CUSTOMER_WRITES`（**文件 + needle + 期望命中数 + 理由**，22 条），**或**
   ③ 所在文件在 `NON_OUTLET_FILES`（精确路径 + 理由，5 条：`mcp/protocol.rs` 是返回
   `String` 的**构造器**，4 个 `src/bin/*` 是运维二进制）；
5. 白名单**双向**：每条 needle 的实际命中数必须**恰好**等于登记值（少一个 = 过期，多一个 =
   有人照着抄了一份）。
6. 同时把第 1 轮 N1 的洞堵上：登记表 → 源码那一侧也改用**剥注释后**的源码做 needle 命中。

实时数字：`src/` 下 60 个 `.rs`，非管线文件里共 **39** 个写点，其中 17 个由 `OUTLETS` 登记覆盖、
22 个由 `NON_CUSTOMER_WRITES` 覆盖；`src/domain/pipeline.rs` 排除在枚举外（它是登记表与写出机制
的定义处，其 `OUTLETS` 字面量里必然含 needle 文本），另由 **T7** 断言它一行 stdout 宏都没有。

### 10.3 B2 · `OutletId` 查表（R-B）

```rust
// src/domain/pipeline.rs:243
pub enum OutletId { C1, C2, C3, C4, C5, H1, H2, H3, McpTools, Z1, Z2, Z3, D1 }
// src/domain/pipeline.rs:291  —— ⛔ 模块私有
const fn spec_of(id: OutletId) -> OutletSpec { … }
// src/domain/pipeline.rs:636 / 727
pub fn render(id: OutletId, product: Product<'_>) -> Result<OutletRender, PipelineError>
pub fn publish_value(id: OutletId, value: Value) -> Result<GuardedValue, PipelineError>
```

- `OutletSpec` 字段**全私有**（`pipeline.rs:191`），且**没有 `pub const` / `pub fn` 能造出一个值**；
  `Outlet.spec` 也改成私有字段（读轨道走 `o.track()` / `o.format()` / `o.granularity()`）。
- 运行时第二道：`pairing_ok`（`pipeline.rs:713`）—— 客户类产物（`CustomerHtml/CustomerJson/Overview`）
  只能从 `Track::Customer` 入口发布；`ErrorText` 只能走 `Z1`。所以「换一个不过闸的 ID 把客户视图发出去」
  会当场 `Err`，而不是静默绕过。
- 源码第三道：`OUTLET_ID_TOKENS`（`t3`）逐文件登记 `OutletId::` 记号的**次数**；
  换 ID / 多加一处管线调用都让次数对不上。

### 10.4 B3 + B5 · 出口拿不到「未过闸的字节」+ 不许第二份判据（R-D / 纪律 H）

- **类型层**：`emit_domain_text/line` 的实参是 `OutletRender`（`cli.rs:645/650`）⇒
  「先 `println!` 一份手拼字节、再调管线判定」在出口层**编译不过**（实测 E0308，见 §10.6 #5）。
- **T2（源码层）**：每条客户写出的实参必须是**裸标识符**，且其 `let` 绑定**直接**来自
  `publish_customer(` / `pipeline::render(`；赋值行必须在写出行之前；赋值与写出之间，
  该标识符只许出现在 `.dropped_object_count()` / `.report()` / `.outlet()`，或
  **`fs::write(` / `err(` 的实参位**上的 `.as_str()`。
  ⇒ `if !local_ok(json.as_str()) { … }` 这种「产物交给另一个函数再判一遍」当场红（实测见 #1）。
- **T5（函数登记）**：三个出口文件（`cli.rs` / `server/http.rs` / `mcp/serve.rs`）里的
  **每一个 `fn` 定义**都必须登记在 `OUTLET_FNS`（43 条，按文件 + 名字 + 用途）⇒ 新增
  `fn local_ok`（QA 的攻击形态）或新增一个 handler 都会红。
- **T6（模块登记）**：全仓 `mod x;` 声明（52 条）逐个登记 ⇒ 「把判据拆到新模块再调用」也会红。
- **G-4 收敛（既有守卫同步收紧）**：`test_binary_output_guard::pipeline_site_verdict` 的 ③
  从「`let <binding> = <任意>.as_str()`」改成「`let <binding> = <管线入口>(…)` **且**赋值行在写出之前」。
  旧口径正是 QA 实测「先写后判全绿」的那一格；新口径连同 `has_entry`/判据直调一起，
  对四个 `pipeline` 站点逐个跑 M-P1（摘管线）/ M-P2（加本地判据）/ M-P3（自拼产物）三个变异。

### 10.5 B4 · 公开构造器枚举（R-C）

`t4` 枚举**全仓** `pub fn` / `pub const fn` / `pub async fn` 的签名（`fn` 行 + 下两行，覆盖跨行签名），
命中「返回 `OutletRender` / `GuardedValue` / `Result<…>` / impl 内的 `-> Self`」的点，
要求恰好是 `render` / `publish_value` 两个、且都在 `src/domain/pipeline.rs`。
⇒ 名字换不换无所谓（`craft` / `forged` / 别的都行），**只按返回类型判**。
配套删掉了 `publish_error_text`（第 1 轮唯一的公开产物工厂，QA N6）：Z1 现在也走
`render(OutletId::Z1, Product::ErrorText(…))`（`http.rs:251-266`）。

### 10.6 5 种攻击形态的实测（变异三元组，纪律 G）

装置：在本 worktree 就地改一处 → 记**被改文件 sha256** + 只含该处改动的 patch →
跑判据 → 用**改前快照**逐字节还原（`git checkout` 不适用：本轮改动尚未提交）。
⛔ 编译不过的一律**不当作执行结果**，另注「写法已不可编译」。

| # | 攻击 | 被改文件 sha256（攻击前 → 攻击后） | patch 关键内容 | 结果 |
| --- | --- | --- | --- | --- |
| 1 | 自造 `fn local_ok(s) -> bool { !s.contains("key") }` + **真的**用它跳过写出 + 保留管线调用 | `src/cli.rs` `C5CACC1A…BAFC` → `00F69409…4237` | `+fn local_ok(…);`（模块级）+ `+if !local_ok(json.as_str()) { eprintln!(…); return Ok(()); }` 插在管线赋值与 `emit_domain_line(json)` 之间 | **红 ×2**：`t5`（fn 未登记）+ `t2`（产物在写出前被交给别的函数） |
| 2 | 公开构造器改名 `pub fn craft(text: String) -> Self` | `src/domain/pipeline.rs` `88C28710…F034D` → `A7B56ADD…D2FAC` | `+impl OutletRender { pub fn craft(text: String) -> Self { Self { text, report: DropReport::default(), outlet: "C3" } } }` | **红 ×1**：`t4`（实际 `["craft","publish_value","render"]`） |
| 3 | 新增真实路由 `GET /api/raw`（零闸、零剔除、**不入表**） | `src/server/http.rs` `6BEF88DF…3C4C` → `AAA33CD6…2A4C` | `+.route("/api/raw", get(get_raw))` + `+async fn get_raw(…) -> Json<Value> { Json(to_value(&*state.graph)…) }` | **红 ×2**：`t1`（`http.rs:278 Json(` 未登记）+ `t5`（`fn get_raw` 未登记） |
| 4a | 出口一行 `spec.track = Track::Internal` | `src/cli.rs` `C5CACC1A…BAFC` → `84A72CDD…EAF9` | `+let mut spec = OutletSpec::C3; +spec.track = Track::Internal;` 并把 `spec` 传给 `publish_customer` | **编译不过**（`error[E0599]: no associated function or constant named C3`、`E0061` 参数个数）⇒ **该写法已不存在**，不算执行结果 |
| 4b | 等价的可编译形态：把出口身份换成**建模轨** `OutletId::D1` | `src/cli.rs` `C5CACC1A…BAFC` → `8A97AA4D…9591` | `-OutletId::C3 +OutletId::D1`（C3 分支） | **红 ×1**：`t3`（`D1` 2 次/登记 1 次；`C3` 0 次）；运行时另有 `pairing_ok` 把 `CustomerJson` 从建模轨入口拒掉 |
| 5a | 未过闸字节先写 stdout：`emit_domain_line(&raw)` 放在管线调用之前 | `src/cli.rs` `C5CACC1A…BAFC` → `F19895D1…3272` | `+let raw = serde_json::to_string_pretty(&cv)?; +crate::cli::emit_domain_line(&raw);` | **编译不过**（`error[E0308] mismatched types`）⇒ 类型层挡住，不算执行结果 |
| 5b | 同一形态的**可编译**写法：`println!("{raw}")` | `src/cli.rs` `C5CACC1A…BAFC` → `4DBDDA6B…1CBA` | `+let raw = …; +println!("{raw}");` 插在管线调用之前 | **红 ×1**：`t1`（`cli.rs:1340 println!(` 未登记） |
| 5c | 用**闭包式**写出送未过闸字节：`emit_domain_text_with(&raw, \|_\| ())` | `src/cli.rs` `C5CACC1A…BAFC` → `D16F5DA9…38A9` | `+let raw = …; +crate::cli::emit_domain_text_with(&raw, \|_\| ());` | **红 ×1**：`t1`（`emit_domain_text_with(` 未登记；D2 的登记 needle 带 `&text`，抄不过去） |

> 每次攻击后都用快照还原，`git status` 已核过只含本轮真实改动；
> `src/bin/ontok-gate-preflight.rs` 全程只在 A8 里挪出/挪回，sha256
> `C7CF95E2…322D` 前后一致（见 §10.8）。

### 10.7 本轮对既有测试的改动（逐条说明「没有放宽」）

| 文件 | 改了什么 | 为什么不是放宽 |
| --- | --- | --- |
| `tests/test_binary_output_guard.rs` | `pipeline_site_verdict` ③ 从 `.as_str()` 形状 → 「直接由管线入口赋值 + 时序」；`has_entry` 去掉已删除的 `publish_error_text`；M-P3 / `judge_like_line` 的锚点跟着新形状 | ③ **更严**（旧口径不认管线来源、也不管时序）；M-P1/M-P2 两个变异一字未动，M-P3 仍必须红 |
| `tests/test_req107_track_manifest.rs` | TC-022 的「`mislabelled.track = Track::Customer`」改成「同一份产物走客户轨出口 `C3`」+ 新增「`CustomerJson` 不许从 `D1` 进入」 | 原写法在这一版**编译不过**（字段已私有）；新写法验的是同一件事（轨道决定拦不拦），并多钉一条「换 ID 不能关闸」 |
| `tests/test_req107_scope_error_gate.rs` | `publish_error_text(` → `pipeline::render(` + `Product::ErrorText(`；新增 `!body.contains("publish_error_text(")` | 断言只增不减；还多钉了「错误路径的公开工厂已删除」 |
| `tests/test_req107_outlet_pipeline.rs` | C2 needle 改 `std::fs::write(p, html.as_str())`；新增 `let html = publish_customer(` 断言；入口签名清单去掉 `publish_error_text` | 新增的断言钉的是「`html` 必须是管线产物本身」——比只钉「两个分支用同一个变量」更严 |
| `tests/test_req107_guard_granularity.rs` / `test_req107_pipeline_bytes.rs` / `test_req107_static_texts.rs` / `test_property_filter.rs` | 只做 `OutletSpec::X` → `OutletId::X` 的机械替换（含 `MCP_TOOLS` → `McpTools`） | 断言内容逐字未改；`test_property_filter` 的三条 needle 仍然各自命中 |
| `tests/test_exit_ordering_req106.rs` | **一字未改** | 它现在验的是管线内部时序（仍 7 passed）；出口侧时序由 T2 + G-4 ③ 接上（QA B3 指的就是这一格） |

### 10.8 本轮实测命令与结果（本机 Windows + 无 PostgreSQL）

A8 绕行：`src/bin/ontok-gate-preflight.rs` 挪到 `%TEMP%\req107-dev-r2\`，
跑完挪回，**sha256 前后一致**：`C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`。

| 命令 | 结果 |
| --- | --- |
| `cargo test --test test_req107_source_enumeration` | **7 passed / 0 failed**（T1–T7） |
| `cargo test --test test_req107_outlet_pipeline` | 12 passed |
| `cargo test --test test_binary_output_guard` | 7 passed |
| `cargo test --test test_exit_ordering_req106` | 7 passed |
| `cargo test --test test_req107_track_manifest` | 2 passed |
| `cargo test --test test_req107_guard_granularity` | 4 passed |
| `cargo test --test test_req107_pipeline_bytes` | 1 passed |
| `cargo test --test test_req107_scope_error_gate` | 4 passed |
| `cargo test --test test_req107_static_texts` | 3 passed |
| `cargo test --test test_req107_red_lines` | 2 passed |
| `cargo test --test test_req107_zero_visible_change` | 3 passed |
| `cargo test --test test_property_filter` | 10 passed |
| `cargo test --test test_graph_server` | 9 passed |
| `cargo test --test test_guard_manifest_req081` | 6 passed |
| `cargo test --test test_model_view` | 10 passed |
| `cargo test --test test_overview` | 17 passed |
| `cargo test --doc` | **4 passed**（两条 `compile_fail` 仍 ok） |
| `cargo test --lib` | **168 passed / 2 failed** —— 两条都是 `MissingEnv("ONTOK_TEST_DATABASE_URL")`（`db::apply` / `extract::introspect`），**环境不具备**，与本轮改动无关 |
| `cargo test --test test_no_write_path` | 3 passed / 1 failed（同一个 env） |
| `cargo test --test test_retained_names_req084` | **跑不了**（A10：它 `include_str!` 那个 unix-only bin；bin 挪出时编不过，挪回时 bin 本身在本机编不过） |
| `test_mcp_server` / `test_domain_print_sites_binary_req078` | **未跑**（需 PG，环境不具备；与第 1 轮同一限制） |

### 10.9 本轮**没有**做 / 仍然存在的边界（⛔ 不许读成「已封堵」）

1. **枚举认的是构造拼写**（`println!` / `emit_domain_*` / `Json(` / `Html(` / `encode_result(` /
   `fs::write(`）。用别的机制往 stdout 写（自建 `std::io::stdout()`、第三方日志宏）**抓不到**；
   补强在类型层（`emit_domain_*` 只收 `OutletRender`），但 `emit_domain_text_with(&str, …)` 这个
   建模轨闭包入口仍在 —— 它的**新调用点**会被 `t1` 抓住（5c 实测），可它的**实参来源**只由
   REQ-078 的 G-1 守卫覆盖（D2 一处）。
   > **第 3 轮订正（QA N6）**：上面「补强在类型层」的说法**只对 `emit_domain_*` 成立**；
   > 自开 `std::io::stdout()` 这条路径**类型层零作用**。第 3 轮已改为**否定式禁调**（§11.4），
   > 不再依赖类型层叙事。
2. **T2 的窗口**只看「管线赋值 → 写出」之间。「在调用管线**之前**用一个闭包式本地判据决定
   要不要调管线」不在 T2 覆盖内（`fn` 形式会被 T5 抓好，**闭包形式抓不到**）。
3. **HTTP handler 返回裸 `String` / `(StatusCode, String)`** 这类不走 `Json(`/`Html(` 的响应体
   不在 `t1` 的构造枚举里（它在 `http.rs` 里新增函数会被 T5 抓；改的是已有函数则抓不到）。
4. `NON_OUTLET_FILES` 里的 4 个运维二进制只做**整文件**豁免：在 `src/bin/` 里新增写点不会红
   （新增**文件**会红，因为目录是现场遍历的）。
5. `test_retained_names_req084` 在本机**跑不了**（A10），它对 `DOMAIN_PRINT_SITES` 的基线没有复核。
6. 卡点② 的「`Granularity` 参与执行」仍然是**声明性**的（QA §7.3 的 N2 未修，属非阻断）。
   `src/domain/pipeline.rs` 的 `render` 仍然不读 `granularity`（只有 H3 的「逐节点」由
   handler 形态决定）。

**我认为最可能被第 3 轮 QA 打回的一点**：**B5 的「闭包形式本地判据」**。
`let local_ok = |s: &str| s.contains("key");` 不会进 `OUTLET_FNS`（那不是 `fn` 定义），
若它只作用在**重新拼出来的字符串**上（不碰管线产物），T1（没有新写点）/T2（产物没被碰）/
T3–T7 都不会红。要彻底封它，只能把出口函数的**允许语句集**也做成登记表（成本远大于本需求），
或者让 `serde_json::to_string_pretty(` / `render_html(` 这类 domain 生产者在出口文件里
**整体禁用** —— 后者是可行的下一步，但会与 D1（建模投影）的现有写法冲突。

---

## 11 · 第 3 轮修复（QA `qa-review-2.md` 的 2 条阻断 + 2 条收紧）

> **对象**：`BA/demands/REQ-107/qa-review-2.md`（FAIL，B1/B2）+ `acceptance.md §七 A13`。
> **基线**：`4bbd8ac`。**本轮只动两处**：`tests/test_req107_source_enumeration.rs`（判据）
> + `src/mcp/serve.rs`（9 行传输层收窄，见 §11.4）。**13 条出口的接线代码一行未改**。

### 11.1 结论先行

| # | 项 | 结果 | 证据 |
| --- | --- | --- | --- |
| **F1** | 「老实填表」即合法 | **封住（运行期判红）** | QA 现成变异 `M6b` 复跑：**71 passed / 2 failed**（`t8` + `t9`） |
| **F2** | 白名单可加 | **封住（运行期判红）** | QA 现成变异 `M7` 复跑：**72 / 1**（`t1`） |
| **F3** | 直写 stdout | **封住（运行期判红）** | QA `M5d` 复跑：**72 / 1**（`t1`，两行命中） |
| **F4** | 条件分支 + `Response::builder().body(raw)` | **封住（运行期判红）** | QA `M10b` 复跑：**72 / 1**（`t1`，三行命中）+ 新 `t9` |
| — | 13 判据目标（未变异） | **73 passed / 0 failed**（原 71 + 新增 `t8`/`t9`） | §11.8 |
| — | `cargo test --lib` | **168 passed / 2 failed**（`MissingEnv`，环境不具备） | 与 QA 基线逐条一致 |
| — | `cargo test --doc` | **4 passed** | 两条 `compile_fail` 仍在 |

**没有一次是靠「编译失败」挡住的**：8 次变异全部**编译通过**、由运行期判据判红（`log` 里无 `error[E…]`）。
（`run.ps1` 摘要里的 `COMPILE| error: test failed …` 是 cargo 在**测试失败之后**的收尾报错，不是编译错误。）

### 11.2 F1 · 声明必须伴随证据（`Enforcement::Pipeline` 不再是无人校验的枚举值）

QA 的命门：`OUTLETS` 里写 `enforcement: Enforcement::Pipeline` **没有任何判据**去问「这条出口到底调没调管线」。

修法是给每条 `Pipeline` 条目配**三件证据**，全部落在
`tests/test_req107_source_enumeration.rs`：

| 证据 | 实现 | 位置 |
| --- | --- | --- |
| ① 证据记号 = 出口自己的 id | `evidence_token(id)`：只有两个**写死、不可扩展**的共享组（`C2→C1` 同一份产物、`M1–M5→McpTools` 同一次调用），其余 `other => other`。**新登记 `H4` 拿不到 `H2` 的记号** | `:1560` |
| ② 真有一处管线调用 | `pipeline_calls()`：枚举 `publish_customer(` / `publish_json(` / `publish_value(` / `pipeline::render(` 的**调用**（定义行不算），按括号配平取实参里的 `OutletId::<X>` + 所在函数 | `:1513` |
| ③ 调用与写点在**同一条路径** | `t8`：写点所在函数 == 调用所在函数，**或**写点所在函数正是该调用的被调者（HTTP 的 `publish_json`/`publish_value` 适配器形态）；反向：每处携带记号的调用都必须有 `Pipeline` 条目认领；另加「`Pipeline` 条目只许落在三个出口文件」 | `:1569` |

合法形态逐条核过：`C1/C3/C4/C5/D1`（`ontology_cmd` 同函数）、`C2`（同函数 + 共享 C1）、
`H1`（`index` 同函数）、`H2/H3`（写点在 `publish_json` 适配器、调用在 handler ⇒ 适配器规则）、
`Z1`（`check_scope` 同函数）、`M1–M5`（`dispatch` 同函数 + 共享一次调用）、`Z2/Z3`（`dispatch` 同函数）。

**M6b 实测**：`H4（src/server/http.rs）声明了 Enforcement::Pipeline，但 src/server/http.rs 里找不到携带 OutletId::H4 的管线调用` ⇒ 红。
**M11 实测**（我加造的升级版：**伪造一次真调用** `publish_value(OutletId::H4, json!({}))` 再把结果丢掉）：
`H4 的写点在第 282 行的 get_graph 里，但携带 OutletId::H4 的管线调用在第 314 行的 get_raw 里 —— 声明与证据不在同一条路径上` ⇒ 红（另加 `t9` 红，见 §11.5）。

### 11.3 F2 · 白名单 needle 的窗口不得含 domain 生产者

| 项 | 实现 | 位置 |
| --- | --- | --- |
| 生产者清单 | `DOMAIN_PRODUCERS`：`to_string_pretty(` / `to_value(` / `render_html(` / `render_overview_text(` / `filter_view(` / `encode_result(` | `:371` |
| 纯判据 | `whitelist_smuggles_domain_product(construct, window)`：窗口含生产者 ⇒ 该白名单条目**不成立**；`construct` 本身等于生产者时不算夹带（MCP 的 `p::encode_result(id, p::empty_result())`） | `:384`、`:618` |
| 断言 | `t1`：`smuggled.is_empty()`（文案直说「白名单 = 合法绕过」） | `:734` |

反空转：`println!("{}", serde_json::to_string_pretty(&*state.graph))` 必须被判出（M7 形态）；
`encode_result(id, p::empty_result())` 与错误通道的 `Json(json!({"error": msg}))` 必须**不**被判出（防恒红）。

**M7 实测**：`src/server/http.rs:280 println!(`（白名单 needle `serde_json::to_string_pretty(&*state.graph)`）的窗口里含 domain 生产者 `to_string_pretty(` ⇒ 红。

### 11.4 F3 · 出口文件里禁止「绕开全部已登记构造」的写出机制

| 项 | 内容 |
| --- | --- |
| 禁调集合 | `FORBIDDEN_MECHANISMS`：`std::io::stdout`、`write_all(`、`Response::builder`、`.body(`、`Body::from(`（`:399`） |
| 扫描器 | `forbidden_mechanisms(rel, code)`：跳过 `WRITER_FNS` 函数体 + `TRANSPORT_FNS`（`:500`） |
| 断言 | `t1`（`:786`/`:791`），含 5 条正探针 + 2 条反探针 |

**豁免只留一个，且精确到函数**：`src/mcp/serve.rs::write_mcp_line`（`:414`）。
第 2 轮的写法把**整条 `serve()`** 当豁免区（函数体 57 行，往里加一行裸写谁也看不见）；
本轮把传输写收进 9 行小函数（`src/mcp/serve.rs:213`，`serve()` 里只剩 `write_mcp_line(&resp)?`，`:199`），
于是 `serve()` 的函数体重新受 F3 约束。

**M5d 实测**：`src/cli.rs:1350 std::io::stdout` + `:1351 write_all(` ⇒ 红 ×2。
**M12 实测**（我加造：在 `serve()` 里加 `std::io::stdout().write_all(line)`）：⇒ 红 ×2（第 2 轮这一格是绿的）。

### 11.5 F4 · 客户响应体必须是管线产物

两层，**都不是登记**：

1. **禁调**（同上）：`Response::builder` / `.body(` / `Body::from(` 在三个出口文件里出现即红。
2. **`t9`（`:1676`）**：三个出口文件里**每一处** `Json(` / `Html(` 写点（除错误通道白名单），
   实参必须是 `<绑定>.into_value()` / `<绑定>.into_text()`，且该绑定出现在 `Ok(<绑定>)` 分支、
   其上游 20 行内有管线调用。

> ⚠️ **BA 更正（2026-10-20，第 5 轮 QA 实测）**：本句原文结尾是「⇒「手搓响应体」不登记也红」——**这句话过头了**。
> `t9` 只认**字面量** `Json(` / `Html(`，而 QA 实测两种等价拼写可以全绿：
> `Json::from(raw)` 与 `.map(Json)`（3 行、零登记、**74/0 全绿**、同样绕过 `check_scope`）。
> ⇒ 正确说法：**「字面量 `Json(`/`Html(` 形式的手搓响应体会红；`Json::from` / `.map(Json)` 形式尚未覆盖」**，
> 后者已登记为 **P-5**（`acceptance.md` §A15），⛔ 不许再写成「不登记也红」。

⚠️ 第 2 层刻意**不看登记表**：M11 的写点 needle 是 `serde_json::to_value(&*state.graph)`
（不含 `Json(`），若只遍历「call_site 里含 `Json(` 的条目」会漏掉它 —— 这是我一轮内自己打穿过一次的格子。

**M10b 实测**：`Response::builder`@282 / `.body(`@285 / `Body::from(`@285 ⇒ 红 ×3。
**M6b / M11 实测**：`t9` 报 `响应体是 serde_json::to_value(&*state.graph).unwrap_or(...) —— 不是管线产物` ⇒ 红。

### 11.6 六种攻击的变异结果（纪律 G 三元组）

> 装置：`.review-workspace/req107/dev3/{mutate.ps1,run.ps1,restore.ps1}`。
> 每次：**唯一一次精确替换**（`IndexOf` + 子串拼接，UTF-8 无 BOM）→ 记**被改文件 sha256 前/后** →
> `git diff --no-index` 存 patch（`logs/<ID>.patch`）→ 跑 13 个目标（`--no-fail-fast`）→ **从 stash 逐字节复原**并核 sha。
> ⛔ 变异**不碰**生产代码里的登记表以外的东西；复原后 `git status` 只剩本轮应改的两个文件。

| # | 攻击 | 被改文件 sha256 前 → 后 | 结果（13 目标） |
| --- | --- | --- | --- |
| 0 | **基线**（未变异） | — | **73 passed / 0 failed**（9/12/4/1/2/4/3/2/3/10/7/7/9） |
| 1 | **老实填表**：`/api/raw` 零闸 + `OUTLETS` 加 `H4`(Pipeline) + `OUTLET_FNS` + 计数 33/13→34/14 + ID 集合（QA `M6b`） | `http.rs 6BEF88DF…→34F76516…`；`pipeline.rs 88C28710…→E312EA2A…`；`test_…_source_enumeration.rs 75AB1461…→79C1B9D7…`；`test_req107_outlet_pipeline.rs B395D8D0…→22BBD19E…` | 🟢 **红 ×2**：`t8`（无 `OutletId::H4` 调用）+ `t9`（响应体是手搓 `to_value`）。其余 12 目标全绿 |
| 2 | **白名单**：`println!` 打客户 JSON + `NON_CUSTOMER_WRITES` 加一行 needle/计数（QA `M7`） | `http.rs 6BEF88DF…→7385EC1D…`；`test_…_source_enumeration.rs 75AB1461…→A4727419…` | 🟢 **红 ×1**：`t1`（窗口含 `to_string_pretty(`） |
| 3 | **直写 stdout**：`std::io::stdout().lock().write_all(raw)`（QA `M5d`） | `cli.rs C5CACC1A…→4A7E07B6…` | 🟢 **红 ×1**：`t1`（`:1350 std::io::stdout`、`:1351 write_all(`） |
| 4 | **改已有 handler + 条件分支 + `Response::builder().body(raw)`**（QA `M10b`） | `http.rs 6BEF88DF…→54964D46…` | 🟢 **红 ×1**：`t1`（`Response::builder`@282 / `.body(`@285 / `Body::from(`@285） |
| 5 | **运维 bin 整文件豁免里加客户写出点**（QA `M8`） | `ontok-propose.rs A8C0A209…→420E7BA7…` | 🔴 **全绿 73/0** ⇒ 登记 **P-3** |
| 6 | **闭包式判据** `let local_ok = \|s\| …` 只作用在重拼串上（QA `M9`） | `cli.rs C5CACC1A…→3742A588…` | 🔴 **全绿 73/0** ⇒ 登记 **P-1** |
| + | **（我加造）伪造一次真管线调用**丢弃结果再写手搓 JSON（`M11`） | `http.rs 6BEF88DF…→DBB2A4AC…`；`pipeline.rs 88C28710…→74BC5C37…`；`test_…_source_enumeration.rs 75AB1461…→721AD24C…`；`outlet_pipeline B395D8D0…→E28BC139…` | 🟢 **红 ×2**：`t8`（证据不在同一路径）+ `t9`（响应体不是产物） |
| + | **（我加造）`serve()` 里再开一条裸写**（`M12`，验证 F3 豁免收窄） | `serve.rs B4ECD075…→517699C4…` | 🟢 **红 ×1**：`t1`（`serve.rs:199 std::io::stdout` / `write_all(`） |

**逐条说明「没有靠编译期」**：8 次变异全部编译成功（`cargo` 只报 `test failed`），
红的都是**运行期判据 + 失败用例名**。本轮**没有**「编译期挡住」的项。

### 11.7 已知限制（登记 ≠ 已覆盖）

| # | 形态 | 危害 | 什么条件下会真漏 | 下一轮谁接手 / 怎么收 |
| --- | --- | --- | --- | --- |
| **P-1** | 闭包式本地判据 `let local_ok = \|s\| …`（M9，**73/0 全绿**） | 它能在**调管线之前**决定「这条出口写不写 / 写什么」，而且只作用在**重拼出来的串**上 ⇒ T2（只看管线产物被怎么用）永远看不到它 | 单用闭包 = **静默丢/改**（功能性），不直接泄漏——前提是最终写出的仍是管线产物（T2 钉着）。**与 P-2 组合**（手拼响应体）才是泄漏 | Dev（第 4 轮）。方案：给出口函数加「允许语句集」白名单（`let <id> = \|` / 管线调用之前的 `if … { return }` 一律要求登记用途）。成本中等 |
| **P-2** | handler 返回裸 `String` / 元组（不走 `Json(`/`Html(`）<br>⚠️ **第 4 轮已改写：见 §12.7**（本行第 3 轮的「要先改签名、diff 显眼」只对 `Ok` 通道成立，**漏了 `Err` 通道**） | axum 对 `String` 有 `IntoResponse` ⇒ 响应体不进 T1 写点枚举，`Content-Type` 还变成 `text/plain` | 改**已有** handler 的**返回类型**（条件分支 `return Ok(raw_string)` 与 `Ok(Json<T>)` 不同型 ⇒ **编译不过**，所以静默后门要先改签名，diff 显眼）；或**新增**一个返回 `String` 的路由（T5 要求登记 fn、T1 无写点）<br>⛔ **第 4 轮补**：`Result<Json<_>, Response>` 的 **`Err` 类型本来就是 `Response`** ⇒ `return Err((StatusCode::OK, raw).into_response())` **4 行、签名不动**就能吐未过闸的客户图（QA `F` 变异实测 73/0 全绿），已由 §12.2 封住 | Dev（第 4 轮）✔ 方案（对 3 个数据路由加返回类型判据）方向对；**第 4 轮已改为**「`.into_response(` 逐字钉住 + `StatusCode::OK` 禁调」（§12.2），残余 `err(status, &raw)` 已登记（§12.7） |
| **P-3** | `NON_OUTLET_FILES` 对 4 个运维 bin + `mcp/protocol.rs` **整文件**豁免（M8，**73/0 全绿**） | 这 5 个文件里任何客户可见写出点都不会红；`src/bin/` 正是「顺手 println 调试」的高频位置 | 往 `ontok-propose` / `ontok-introspect` / `ontok-translate-names` 里打印客户视图字段或整份 JSON，且被当成运维回执发出去 | Dev（第 4 轮）。方案：`NON_OUTLET_FILES` 从「路径」改成「(路径, 函数名)」——4 个 bin 的写点集中在 `main` + 少数 fn，一次性登记即可（QA 估成本低） |

**新增的一条窄边界（不是 P，但必须写）**：F3 的唯一豁免是 `src/mcp/serve.rs::write_mcp_line`（9 行）。
危害：**只有这个函数体内部**可以裸写 fd。什么条件下会真漏：若有人把它的实参从 `resp`（`handle_line` 的返回值，
已过 `publish_value` 的闸）改成手拼串 —— T1/T9 看不见。残留风险靠「函数只有 3 行、改动显眼」+ 代码评审兜；
**不假装封死**。

### 11.8 本轮实测命令与结果（本机 Windows + 无 PostgreSQL）

```powershell
# A8：临时挪出 unix-only bin（⛔ 不删/不改内容），跑完挪回
#   BEFORE_SHA256 = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
#   AFTER_SHA256  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D   SAME=True
cargo test --test test_req107_source_enumeration --test test_req107_outlet_pipeline `
  --test test_req107_guard_granularity --test test_req107_pipeline_bytes `
  --test test_req107_red_lines --test test_req107_scope_error_gate `
  --test test_req107_static_texts --test test_req107_track_manifest `
  --test test_req107_zero_visible_change --test test_property_filter `
  --test test_exit_ordering_req106 --test test_binary_output_guard `
  --test test_graph_server --no-fail-fast
```

| 命令 | 实测 |
| --- | --- |
| 13 个目标 | **73 passed / 0 failed**（9/12/4/1/2/4/3/2/3/10/7/7/9；原 71 + 新增 `t8`/`t9`） |
| `cargo test --lib` | **168 passed / 2 failed**（`db::apply` / `extract::introspect` = `MissingEnv`，**环境不具备**） |
| `cargo test --lib registry_is_the_33_baseline` | **1 passed**（33/13 数字未动） |
| `cargo test --doc` | **4 passed** |
| `git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs src/mcp/tools.rs` | **空**（五红线零触碰） |
| `property_filter.rs` 前 145 行 sha256（develop vs 现状） | `769763BB…5E94` = `769763BB…5E94`（逐字节相同，G1–G4 措辞未动） |
| 新增 `#[ignore]` / 删测试文件 / 删 `#[test]` | **0 / 0 / 0** |
| 断言计数（三个既有判据文件本轮**未改**） | `property_filter` 30、`exit_ordering_req106` 11、`binary_output_guard` 79 —— 与 QA 上一轮核过的一致 |
| golden（`tests/fixtures/req107/golden`） | 本轮**零改动**（`git diff 4bbd8ac --name-only -- tests/fixtures` 空） |
| `tests/test_retained_names_req084.rs` | **跑不了**（A10：`include_str!` 引用那个 unix-only bin；挪出时编不过、挪回时 bin 本身在本机编不过）——如实报 |

### 11.9 我认为第 4 轮最可能被打回的一点

**P-1（闭包式本地判据）**。A13 允许「登记」，但同一段又写「登记 ≠ 已覆盖」，而 P-1 是三条登记里
唯一**能决定客户字节写不写/写什么**的一条（P-2/P-3 的漏点都要先改签名或新路由，痕迹都更大）。
我给的危害评估是「静默丢/改、不直接泄漏（因为最终字节仍必须是管线产物）」，**这个论证依赖 T2 继续有效**；
如果 QA 认为 T2 的窗口太窄（它只看「管线赋值 ↔ 写出」之间），P-1 就会被判「危害评估不足」。

**第二可能**：`M11` 的残留 —— 「同一个函数里调一次真管线 + 写另一份手搓字节，且手搓字节不走 `Json(`/`Html(`」。
今天这条只剩 P-2 那一个出口（返回裸 `String`），所以它会被算到 P-2 头上。

---

## 12 · 第 4 轮修复（`qa-review-3.md` 的 B1 / B2 + A14 的 B3 / B4 / B5）

> **对象**：`BA/demands/REQ-107/qa-review-3.md`（FAIL：**2 阻断 + 3 种新绿**）+ `acceptance.md §七 A14`。
> **基线**：`867969b`（`develop b714f48`）。
> **本轮只改一个文件**：`tests/test_req107_source_enumeration.rs`（`+562 / −22`）。
> ⛔ `git status --porcelain src` **空** —— `src/` 一行没动，13 条出口接线一行没动，`OUTLETS` 没动。
> ⚠️ 这意味着 B1–B5 **全部是判据侧**的修法（A14 给的修法本身也都是判据侧）。

### 12.1 结论先行

| # | 项 | 结果 | 证据（本机实测） |
| --- | --- | --- | --- |
| **B1** | `Err` 通道 `return Err((StatusCode::OK, raw).into_response())` | **封住（运行期判红）** | QA 现成变异 `F` 复跑：**73 passed / 1 failed**，`t1` 两处命中（`http.rs:284 into_response(` + `:284 StatusCode::OK`） |
| **B2** | `prod()` 在第一个 `#[cfg(test)]` 处截断 | **封住（运行期判红）** | 新增变异 `B2only`（handler 追加到文件**末尾**、零登记）：**71 / 3**，`t1`/`t5`/`t9` 都指到 **`http.rs:1252`** |
| **B3①** | 管线外定义 `fn into_value` / `fn into_text` | **封住（运行期判红）** | 新判据 `t10`；QA `a′` 复跑：**72 / 2**（`t9` + `t10`） |
| **B3②** | `t9` 只认「上游 20 行内出现过管线调用」 | **封住（运行期判红）** | 同上；`t9` 文案指到 `match Ok::<QaRaw, Response>(QaRaw(…))` 被匹配表达式不是管线调用 |
| **B4** | 内联 `mod x { fn get_graph(…) }` | **方案 A：展开纳入扫描**（理由见 §12.4） | 变异 `d2`（内联 mod + 追加到文件末尾 + **老实填表**）：**71 / 3**（`t5`+`t9`+`t10`） |
| **B5** | `OUTLET_FNS` 是 `(文件, 函数名)` 集合 ⇒ 同名第二函数免登记 | **封住（运行期判红）** | `t5` 文案：`` `fn get_graph` 定义了 2 次 —— 登记粒度是 (文件, 函数名)，重名函数等于**免登记**（B5） `` |
| — | 13 判据目标（未变异） | **74 passed / 0 failed**（原 73 + 新增 `t10`；每目标 10/12/4/1/2/4/3/2/3/10/7/7/9） | §12.8 |
| — | `cargo test --lib` | **168 passed / 2 failed**（`MissingEnv`，**环境不具备**） | 与 QA 基线逐条一致 |
| — | `cargo test --doc` | **4 passed** | 两条 `compile_fail` 仍在 |

**7 种攻击全部「运行期判红」**：没有一次是编译失败挡住的（`log` 里无 `error[E…]`，
`run.ps1` 摘出来的 `COMPILE| error: test failed …` 是 cargo 在**测试失败之后**的收尾报错）。

### 12.2 B1 · `Err` 通道（`path:line`）

| 内容 | 位置 |
| --- | --- |
| `into_response(`（**不带前导点**）进禁调集合（并新增 `StatusCode::OK`） | `tests/test_req107_source_enumeration.rs:510`（`FORBIDDEN_MECHANISMS`） |
| 五处**错误通道**的逐字钉住清单 | 同文件 `:545`（`INTO_RESPONSE_SITES`）、窗口宽度 `:568`（`INTO_RESPONSE_WINDOW = 3`） |
| 扫描器（含双向计数） | 同文件 `:675`（`forbidden_mechanisms`） |
| 反空转探针（6 条正 + 5 条反） | 同文件 `t1`（`:917` 起） |

**为什么 needle 是 `into_response(` 而不是 `.into_response(`**：只封带点的那种拼写等于留三种别名 ——
`IntoResponse::into_response((StatusCode::OK, raw))`、`<String as IntoResponse>::into_response(raw)`、
以及「先 `let r = …into_response()` 再 `return Err(r)`」的变体。现在的口径是
**任何一处 `into_response(` 都算**，唯一的例外是**函数定义那一处**（`fn into_response(self) -> Response`，
逐处判：`line[..at].trim_end().ends_with("fn")`），否则 `impl IntoResponse for ApiError` 的签名会恒红。

**为什么是「逐字钉住」而不是「整函数豁免」**：`into_response(` 是「把任意东西变成 `Response`」的
**总闸门**，今天它出现在五处错误通道（正文全是 `{"error": …}`）。如果按函数豁免，`require_token`
（能拿到 `State`）就成了一个新的后门；所以改成**把五处语句形状逐字钉住**：

| pinned needle（必须在 `into_response(` 命中行 + 往上 3 行内出现） | 处 | 为什么允许 |
| --- | --- | --- |
| `(self.status(), Json(json!({ "error": msg })))` | 1 | `impl IntoResponse for ApiError`：错误正文经 `message()→safe_error` |
| `(status, Json(json!({ "error": msg })))` | 1 | `err()` 助手：错误通道的唯一构造点 |
| `StatusCode::METHOD_NOT_ALLOWED.into_response()` | 1 | `fallback` 的 405：**无 body** |
| `这里只读：只能看` | 1 | 405 的固定文案（与 `NON_CUSTOMER_WRITES` 同源） |
| `没有可用的访问口令` | 1 | 401 的固定文案（不回显令牌） |

双向口径与 `NON_CUSTOMER_WRITES` 相同：**多一处 ⇒ 红**（「没被任何 needle 认领」）、
**少一处 / 形状被改 ⇒ 红**（「期望命中 N 次，实际 0 次」）。
⚠️ 这张表与 `NON_CUSTOMER_WRITES` 里那两条 405/401 文案**耦合**：改文案时必须同时改两处，否则红 —— 这是有意的。

另加一条 `StatusCode::OK` 禁调：`Err` 通道回 2xx **没有正当用法**（200 只许来自
`Ok(<管线产物>)` 那条分支），它把 QA 那条「4 行、签名不动」的形态从**语义**上也堵住了
（`err(StatusCode::OK, &raw)`、`(StatusCode::OK, …).into_response()` 都会先撞上它）。
今天三个出口文件的生产代码里 `StatusCode::OK` **零出现**（QA 实测的那 8 处全在 `http.rs` 的测试模块里，
`prod()` 现在会跳过它们）。

### 12.3 B2 · `prod()` 从「截断」改成「跳过区间」（`path:line`）

`tests/test_req107_source_enumeration.rs:124`。

| 项 | 第 3 轮（被 QA 打穿） | 第 4 轮 |
| --- | --- | --- |
| 做法 | `raw.split("#[cfg(test)]").next()` —— **从第一个 `#[cfg(test)]` 起全丢** | 找 `#[cfg(test)]` / `#[cfg(all(test, …))]` **属性行** → 花括号配平找它修饰的 item 的**结束行** → 区间**抹成空行** |
| `src/server/http.rs` 的后果 | 572 行之后的 674 行（测试模块 + **文件尾部一切**）对 `t1/t2/t3/t5/t8/t9` 不可见 | 只跳过 `mod tests { … }`（572–1246），**文件尾部新加的代码全部可见** |
| 行号 | — | **1:1 保留**（抹空不删行）：`t8` 的同函数定位、`t9` 的绑定定位、报错行号都还能对到源码 |
| 配平视图 | — | `blank_strings` 视图（字符串里的 `{` / `}` 不算），否则测试模块里的一句 `"{}"` 就能把配平带偏 |
| 多个 / 嵌套 | — | 由循环 + 配平处理（不是正则、不是「找第一个」） |

**实测（`B2only`）**：把 6 行零闸 handler 追加到 `http.rs` **最后**、加一行 `.route(...)`、零登记 ⇒
`t1`（`:1252 Json(` 未登记）、`t5`（`fn qa_b2_raw` 未登记）、`t9`（响应体是 `serde_json::to_value(...)`）**三条同时红**。
（QA 的第 3 轮同一形态是 73/0 全绿。）

### 12.4 B3 · 两种伪造各堵一处（`path:line`）

| # | 修法 | 位置 |
| --- | --- | --- |
| ① | 新判据 `t10`：全仓（`src/` 递归，`prod()` 视图）**任何** `fn into_value` / `fn into_text` 的**定义**都必须在 `src/domain/pipeline.rs` 里。`guarded_consumers()`：只认定义（`fn into_value(`），调用（`rendered.into_value()`）不算；trait 里的方法签名也算定义 | `:2274`（`guarded_consumers`）、`:2295`（`t10`） |
| ② | `t9` 把「上游 20 行内出现过管线调用」换成 `product_origin()`：产物的**初始化式本身**必须是管线调用 —— `let <id> = <pipeline>(…)` 或 `match <pipeline>(…) { Ok(<id>) => … }`；只在**同一个函数**里找绑定（`enclosing_fn_lo`），`match` 的被匹配表达式取 `match` 与 `{` 之间那段再判**头** | `:2029`（`pipeline_expr_head`）、`:2048`（`enclosing_fn_lo`）、`:2073`（`product_origin`）、`:2141`（`t9`） |

反空转（都写在测试里，不是口头论证）：`let rendered = QaRaw(serde_json::to_value(…))`、
`match Ok::<QaRaw, _>(QaRaw(…))`、`let rendered = if false { publish_value(…) } else { raw }` **三种都必须 Err**；
`let rendered = publish_value(…)` 与 `match crate::domain::pipeline::render(…) { Ok(rendered) => … }` **必须 Ok**。

### 12.5 B4 + B5 · 选**方案 A**（展开），加一条**同名即红**

**选择：方案 A（内联 `mod` 的内容展开纳入扫描），不选「禁止内联 mod」。理由三条：**

1. **与既有语义一致**：`t1` / `t3` / `t5` / `t8` / `t9` 本来就是「按**文件文本**扫」，内联 `mod` 的内容本来就在文件里；
   方案 B 等于要给一个「本来就能看见的代码」再加一条格式禁令，判据数量涨、解释成本也涨。
2. **方案 B 会与 `#[cfg(test)] mod tests` 直接冲突**：三个出口文件里唯一的内联 mod 就是 `http.rs:573` 的
   `mod tests`（B2 要跳过的正是它）。要禁内联 mod，就得先写一条「`#[cfg(test)]` 修饰的除外」的豁免 ——
   凭空的第二条口子。
3. **真正被打穿的不是 mod 本身，是名字**：QA 的 d 之所以全绿，是 `OUTLET_FNS` 用名字集合 + `t8` 按名字比同函数；
   把内联 mod 的内容当同文件之后，**同名函数就是同一文件里的重复定义** ⇒ 一条去重判据同时封住 B4 与 B5。

**B5 的实现**：`duplicate_fns()`（`:1610`）在 `t5`（`:1622`）里对每个出口文件的函数名做去重检查，
重名 ⇒ 红并说明「登记粒度是 (文件, 函数名)，重名等于免登记」。
副作用（好的）：`t8` 的「同函数」按名字比**重新变得可靠** —— 名字在一个文件里唯一 ⇔ 名字就是函数。
**B4 的实现**：`prod()` 不再截断（§12.3）⇒ 内联 mod 的内容自动进扫描；
另加 `inline_mods()`（`:188`）做反空转，证明 `mod x { … }` 确实被当成同文件而不是被当注释跳过。

### 12.6 七种攻击的变异结果（纪律 G 三元组）

> **装置**：`.review-workspace/req107/dev4/{mutate.ps1,run.ps1,restore.ps1}`（与第 3 轮同一套，`root` 改到 `dev4`）。
> 每次：**唯一一次精确替换** → 记**被改文件 sha256 前/后** → `git diff --no-index` 存 patch（`logs/<ID>.patch`）
> → 跑 13 个目标（`--no-fail-fast`）→ **从 stash 逐字节复原**并核 sha（每次都核过）。
> 原文件 sha（本轮基线）：`http.rs 6BEF88DF…`、`pipeline.rs 88C28710…`、`cli.rs C5CACC1A…`、
> `test_req107_outlet_pipeline.rs B395D8D0…`、`test_…_source_enumeration.rs EC5E747B…`。
> （全部 8 次变异在**同一份判据文件**上重跑过一遍：第一次跑完后又把 B1 的 needle 从
> `.into_response(` 扩成 `into_response(` 以覆盖 UFCS 别名，**换基线后 8 次全部复跑**，下表是复跑结果。）

| # | 攻击 | 被改文件 sha256 前 → 后 | 结果（13 目标） | 红在哪 |
| --- | --- | --- | --- | --- |
| 1 | **M6b 老实填表**：`/api/raw` 零闸 + `OUTLETS` 加 `H4`(Pipeline, 借 H2 spec) + `OUTLET_FNS` + 计数 33/13→34/14 + ID 集合 | `http.rs 6BEF88DF→A55496F6`；`pipeline.rs 88C28710→5777AA64`；`test_…_source_enumeration.rs EC5E747B→7D45DC6C`；`outlet_pipeline.rs B395D8D0→65DDE32B` | 🟢 **72 / 2** | `t8`（找不到携带 `OutletId::H4` 的管线调用）+ `t9`（响应体是手搓 `to_value`） |
| 2 | **M7 白名单**：`println!` 打客户 JSON + `NON_CUSTOMER_WRITES` 加 needle/计数 | `http.rs 6BEF88DF→A257A467`；`test_…_source_enumeration.rs EC5E747B→512902BB` | 🟢 **73 / 1** | `t1`（`:280` 白名单窗口含 `to_string_pretty(`） |
| 3 | **M5d 直写 stdout**：`std::io::stdout().lock().write_all(raw)` | `cli.rs C5CACC1A→4A7E07B6` | 🟢 **73 / 1** | `t1`（`cli.rs:1350 std::io::stdout` + `:1351 write_all(`） |
| 4 | **M10b 改已有 handler** + 条件分支 + `Response::builder().body(Body::from(raw))` | `http.rs 6BEF88DF→90FC9A35` | 🟢 **73 / 1** | `t1` **4 处**：`:282 Response::builder`、`:283 StatusCode::OK`（**本轮新增的 B1 检查也命中**）、`:285 .body(`、`:285 Body::from(` |
| 5 | **a′ 本地 newtype 伪造 `.into_value()` + `if false` 死调用**（QA `A2` 原样） | `http.rs 6BEF88DF→BC565CA8`；`pipeline.rs 88C28710→A92C1E69`；`test_…_source_enumeration.rs EC5E747B→73C3DF87`；`outlet_pipeline.rs B395D8D0→29737DA8` | 🟢 **72 / 2** | `t10`（`http.rs` 定义了 `fn into_value`）+ `t9`（`qa_rendered` 的 `match` 被匹配表达式不是管线调用） |
| 6 | **d2 内联 `mod` 同名 `get_graph` + 整块追加到 `http.rs` 文件末尾**（踩 B2）+ 老实填表 `H4` | `http.rs 6BEF88DF→EE0855B1`；`pipeline.rs 88C28710→41F00C31`；`test_…_source_enumeration.rs EC5E747B→173511E3`；`outlet_pipeline.rs B395D8D0→B226C955` | 🟢 **71 / 3** | `t5`（`` `fn get_graph` 定义了 2 次 ``，B5）+ `t9`（`:1273` 绑定不是管线调用）+ `t10`（`fn into_value`） |
| 7 | **B1 的 `Err` 形态**：`if q.get("qa").is_some() { let raw = to_string(&*state.graph); return Err((StatusCode::OK, raw).into_response()); }`（QA `F` 原样，**4 行、签名不动**） | `http.rs 6BEF88DF→A2FE2AC4` | 🟢 **73 / 1** | `t1`：`:284 into_response(`（不在钉住的错误通道里）+ `:284 StatusCode::OK` |
| + | **B2only**（为 B2 单独造）：零登记 handler 追加到文件**末尾** + 一行 `.route(...)` | `http.rs 6BEF88DF→64DE81DC` | 🟢 **71 / 3** | `t1`（`:1252 Json(` 未登记）+ `t5`（`fn qa_b2_raw` 未登记）+ `t9`（`:1252` 响应体不是管线产物） |

**逐条的「谁挡住了」**：

- M6b ⇒ 第 3 轮的 `t8`/`t9` 仍然有效，**未退化**；
- M7 / M5d / M10b ⇒ F2 / F3 / F4 三条**未退化**（M10b 多命中一条，是 B1 的 `StatusCode::OK` 顺手加的）；
- a′ ⇒ 旧版是 **73/0 全绿**，现在 `t9` **和** `t10` 同时红（两道独立的门：形状 + 定义处）；
- d2 ⇒ 旧版（QA 的 d，把 mod 插在生产段）也是 **73/0 全绿**；现在 3 条红。⚠️ 诚实说明：
  **`t8` 在 d2 下不红**（死调用与写点同名同函数，`t8` 的「同函数」判据本身看不见「死代码」），
  挡住它的是 `t5` / `t9` / `t10`；而且**在 B2 修好之前它根本连看都看不见**；
- F（`Err` 形态）⇒ 旧版 **73/0 全绿**，现在 `t1` 红（两道：`into_response(` 未钉住 + `StatusCode::OK`）。

### 12.7 P-2 的登记**改写**（QA 第 4 轮 §5.2 的阻断项）

> 旧登记（§11.7 P-2）只写了 `Ok` 通道：「改返回类型 ⇒ 编译不过 ⇒ diff 显眼」。
> **那句对 `Ok` 通道成立，对 `Err` 通道不成立** —— `Result<Json<_>, Response>` 的 `Err` 类型**本来**就是 `Response`。

| 通道 | 真实形态 | 真实危害 | 触发条件 | 现在谁挡 |
| --- | --- | --- | --- | --- |
| **`Ok` 通道**（返回裸 `String` / 元组 / `Html`） | `return Ok(raw_string)` 与 `Ok(Json<T>)` 不同型 ⇒ **编译不过**；要静默只能**改签名** | 漏出的是一份**未过闸的客户字节**，且 `Content-Type` 变 `text/plain`（浏览器不当 JSON 用，但 `curl`/脚本照收） | 改已有 handler 的签名，或新增一个返回 `String` 的路由 | `t5`（新 fn 必登记）+ `t1`（`String` 本身不是写点，但新路由/新 fn 会红）+ `t9`（`Json(`/`Html(` 的实参） |
| **`Err` 通道**（QA 的 4 行） | `return Err((StatusCode::OK, raw).into_response())` —— **签名一字不动**（`Err` 就是 `Response`），4 行，不走 `Json(`/`Html(`/`Response::builder`/`.body(`/`Body::from(` | **与 M10b 完全同一后果**：`?qa=1` 直接吐未过闸的整份客户图（`state.graph` 与 H2 同源），**而且绕过 `check_scope`**（分支写在它前面就跳过了项目/版本校验）；代价只有 4 行 + 一个 `into_response()` | 在**任何**返回 `Result<_, Response>` 的 handler 里加一条 `if`，`return Err((StatusCode::OK, raw).into_response())`；`raw` 用 `serde_json::to_string(&*state.graph)` 一行就有 | **本轮封住**：`.into_response(` 进禁调集合（只有五处错误通道逐字钉住）+ `StatusCode::OK` 禁调 ⇒ `t1` 红（§12.6 #7 实测） |

**⚠️ 残余（必须写清，不许读成「已封堵」）—— P-2 的第四种拼写：`err(status, &raw)`**

- **形态**：`return Err(err(StatusCode::BAD_REQUEST, &raw));` —— `err()` 的第二参是 `&str`，
  把客户字节塞进 `{"error": "<raw>"}` 的 JSON 字符串里，状态码是 4xx（不是 200，所以 `StatusCode::OK` 禁调挡不住）。
- **危害**：客户数据仍然**字节级外泄**（`curl` 直接可读），只是不再是「浏览器会当页面渲染的那条路径」；
  且同样跳过 `check_scope` / 管线判据。
- **什么条件下会真漏**：有人想「把错误说清楚一点」，把已解析好的 `raw` 直接塞进 `err(…)` 的文案位。
  **这是一种「善意」改动**，比恶意更难防。
- **为什么本轮不封**：`err()` 的 12 个调用点里有 `e.message()` / `&msg` / `rendered.as_str()`（Z1 管线产物）等
  **多种正当形状**，在源码层做「只许这几种」的否定式会立刻误伤 Z1 那条错误通道（那是本需求要保留的）；
  真正的解法是**类型层**：`err()` 收一个只能由 `pipeline` / `safe_error` 造出来的 `ErrorText`。
  这要动 `http.rs` 的错误通道签名（**13 条出口接线**的一部分），**超出本轮「不动接线」的范围**。
- **接手人**：Dev（第 5 轮）/ BA 裁定是否接受为长期登记。方案：`pub struct ErrorText(String)`（无公开构造器）
  + `err(status, msg: ErrorText)` + `safe_error` 返回 `ErrorText`。

### 12.8 未退化复验（逐项实测）

| 项 | 实测 | 判定 |
| --- | --- | --- |
| 13 目标（未变异） | **74 passed / 0 failed**（10 / 12 / 4 / 1 / 2 / 4 / 3 / 2 / 3 / 10 / 7 / 7 / 9）—— 原 73 + 新增 `t10` | ✅ 只增不减 |
| `src/` 改动 | `git status --porcelain src` **空**（`OUTLETS` / 13 条接线 / `pipeline.rs` 零改动） | ✅ |
| 五红线 `numstat` | `git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs src/mcp/tools.rs` → **空** | ✅ |
| 无删测试 | `git diff --name-status develop -- tests` 无 `D`；本轮 diff 只删了**被替换的实现与文档行**（22 行，逐行核过） | ✅ |
| 无新增 `#[ignore]` | 全仓属性行只有 `test_mcp_server.rs:553`（`develop` 同位置也有） | ✅ |
| 断言只增 | `property_filter` 30、`exit_ordering_req106` 11、`binary_output_guard` 79（三个文件本轮**未改**） | ✅ |
| `property_filter` 前 145 行 | `769763BB…5E94` = `769763BB…5E94`（**逐字节相同**）；`git diff develop HEAD` 的唯一差异是 **第 145 行之后插入的 12 行模块文档**（第 2 轮写的），**没有一行断言被动过** | ✅ 未退化。<br>⚠️ QA 上一轮报的 `D620DCD3…` vs `A1B5101D…` 我复现不出来 —— 差异在**取字节的口径**（我按 `git show` 的文本取前 145 行、LF 拼接、末尾补 `\n`）。**结论相同**（第 145 行之后才变），但**数字本身不可复现**，如实登记 |
| golden | `git status --porcelain tests/fixtures` 空；`test_req107_zero_visible_change` 3 passed、`test_req107_pipeline_bytes` 1 passed | ✅ |
| Z1 / Z2 / Z3 | `test_req107_scope_error_gate` 4 passed；`OutletsId::Z1/Z2/Z3` 记号在 `t3` 里仍是 1/1/1 | ✅ |
| 13 条接线 | `OUTLETS` 未改 ⇒ 13 条一条没动；`t8` 绿说明证据仍在 | ✅ |

### 12.9 本轮实测命令与结果（本机 Windows + 无 PostgreSQL）

```powershell
# A8：临时挪出 unix-only bin（⛔ 不删/不改内容），跑完挪回
#   BEFORE = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
#   AFTER  = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D   SAME=True
cargo test --test test_req107_source_enumeration --test test_req107_outlet_pipeline `
  --test test_req107_guard_granularity --test test_req107_pipeline_bytes `
  --test test_req107_red_lines --test test_req107_scope_error_gate `
  --test test_req107_static_texts --test test_req107_track_manifest `
  --test test_req107_zero_visible_change --test test_property_filter `
  --test test_exit_ordering_req106 --test test_binary_output_guard `
  --test test_graph_server --no-fail-fast
```

| 命令 | 实测 |
| --- | --- |
| 13 个目标（未变异） | **74 passed / 0 failed** |
| 7+1 次变异（**两遍**：B1 的 needle 从 `.into_response(` 扩成 `into_response(` 之后换基线**全部复跑**） | `M6b 72/2`、`M7 73/1`、`M5d 73/1`、`M10b 73/1`、`a′ 72/2`、`d2 71/3`、`F 73/1`、`B2only 71/3`（**全部运行期判红，零编译失败**） |
| 复原后复跑（每次变异后都从 stash 逐字节复原并核 sha） | **74 / 0** |
| `git diff --stat HEAD -- tests/test_req107_source_enumeration.rs` | `+562 / −22`（唯一被改的**被跟踪**判据文件；22 行删除全是**被替换的实现/文档**，逐行核过无断言丢失） |
| `cargo test --lib` | **168 passed / 2 failed**（`db::apply` / `extract::introspect` = `MissingEnv`，**环境不具备**） |
| `cargo test --doc` | **4 passed** |
| `tests/test_retained_names_req084.rs` | **跑不了**（A10：`include_str!` 引用 unix-only bin，挪出期间编不过）——如实报 |
| `git status --porcelain` 收工 | 只有 `tests/test_req107_source_enumeration.rs`（+ 本文件待提交）；`src/bin/ontok-gate-preflight.rs` 已挪回、sha 一致 |

### 12.10 我认为第 5 轮最可能被打回的一点

**`err(status, &raw)`（§12.7 的 P-2 残余）**。理由：它是**同一条 `Err` 通道**上剩下的那一种拼写，
而 A14 已经把「`Err` 通道」定成本轮的阻断词；QA 只要把 `F` 变异里的 `into_response()` 换成
`err(StatusCode::BAD_REQUEST, &raw)`，就会看到 `t1` 全绿（`Json(` 在 `err()` 里，且被白名单钉住）。
我把它**如实登记**（危害 / 条件 / 接手人 / 类型层解法），**不写成已封堵** —— 但按 A13 的口径，
「登记」要 QA 认可危害评估才算过关，而这条的危害（字节级外泄、绕过 `check_scope`）不轻。

**第二可能**：`t9` 的「`match` 只找最近一处、不跟嵌套」：在一个 `match` 的 `Ok` 分支里再套一个
`match`（外层是管线、内层是手搓），`product_origin` 会取**内层**的被匹配表达式而判红 —— 这是**偏严**的误红，
不会漏；但如果有人写「先 `let rendered = publish_value(...)?;` 再 `match rendered { ... }`」，
它会**误判为红**（偏严方向，可修，不构成绕过）。

**第三可能**：`B2only` 这一格我只用「追加到文件末尾」验证；如果第 5 轮把恶意代码塞进
**`#[cfg(test)]` 模块之外、但有 `#[cfg(not(test))]`** 的位置，`prod()` 仍然看得见（没有豁免），
但如果有人用 `#[cfg(feature = "x")] mod …` 包一层，`prod()` 照样扫（它只跳过 `cfg(test)`）—— 方向是**偏严**。

---

## 附 · 交付物清单

| # | 交付物 | 位置 |
| --- | --- | --- |
| 1 | 出口映射表 | 本文件 §1 + `src/domain/pipeline.rs::OUTLETS` |
| 2 | 管线设计 | 本文件 §2 + `.feature/design-req107.md` §二 |
| 3 | 三个卡点裁定 | 本文件 §3 + `.feature/design-req107.md` §三 |
| 4 | 漏接即红实测（6 个变异三元组） | 本文件 §4 + `.review-workspace/req107/mutations/*.patch` |
| 5 | 零可见变化比对方式与结果 | 本文件 §5 |
| 6 | 全量数字 + 口径 + 不可复核声明 | 本文件 §6 |
| 7 | `git diff --numstat` + 红线零触碰 | 本文件 §7 |
| 8 | 已知限制 | 本文件 §8 |
| 9 | 异议 | 本文件 §9 |
| 10 | 第 3 轮修复证据（F1–F4 + 8 次变异三元组 + P-1/P-2/P-3 危害评估） | 本文件 §11 + `.review-workspace/req107/dev3/{patches,logs,stash}/` |
| 11 | 第 4 轮修复证据（B1–B5 + 7+1 次变异三元组 + P-2 改写 / P-2 残余） | 本文件 §12 + `.review-workspace/req107/dev4/{patches,logs,stash}/` |
