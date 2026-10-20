# REQ-107 · 出口收口 —— 交付报告（按 AC-010 九项）

> **代码基线** `develop` `b714f48`（REQ-106 已合并）
> **全量测试基线** `62 suite / 846 passed / 0 failed / 1 ignored @ develop 7f26fa2`（纪律 O）
> **⚠️ 本机不可复核**（`src/bin/ontok-gate-preflight.rs` 的 unix-only 代码；见 §6）
> **分支** `feature/REQ-107`（未 push；5 个提交，见 §7）
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

| # | 出口 | 写出点 | 管线调用点 | 声明（格式 / 轨道 / 粒度） |
| --- | --- | --- | --- | --- |
| C1 | `ontology show --html` → stdout | `src/cli.rs:1316` `emit_domain_text(html)` | `src/cli.rs:1294` `publish_customer(OutletSpec::C1, Product::CustomerHtml{…})` | Html / Customer / Whole |
| C2 | `ontology show --html --out` → 文件 | `src/cli.rs:1305` `std::fs::write(p, html)` | **同上**（与 C1 共用同一份 `html`） | Html / Customer / Whole |
| C3 | `ontology show` → JSON stdout | `src/cli.rs:1336` `emit_domain_line(json)` | `src/cli.rs:1329` `publish_customer(OutletSpec::C3, Product::CustomerJson(&cv))` | Json / Customer / Whole |
| C4 | `ontology overview` → 文本 | `src/cli.rs:1372` `emit_domain_text(text)` | `src/cli.rs:1367` `publish_customer(OutletSpec::C4, Product::Overview(&ov))` | Text / Customer / Whole |
| C5 | `ontology overview --json` | `src/cli.rs:1365` `emit_domain_line(json_text)` | `src/cli.rs:1360` `publish_customer(OutletSpec::C5, Product::Overview(&ov))` | Json / Customer / Whole |
| H1 | `GET /` 首页 | `src/server/http.rs:333` `Html(rendered.into_text())` | `src/server/http.rs:329` `pipeline::render(OutletSpec::H1, Product::Text(html))` | Html / Customer / Whole |
| H2 | `GET /api/graph` | `src/server/http.rs:199` `Json(rendered.into_value())`（`publish_json` 里那一行） | `src/server/http.rs:269` `publish_json(OutletSpec::H2, …)` → `pipeline::publish_value` | Json / Customer / Whole |
| H3 | `GET /api/node` | 同上（同一处 `publish_json`，出口不同） | `src/server/http.rs:294` `publish_json(OutletSpec::H3, …)` → `pipeline::publish_value` | Json / Customer / **PerNode** |
| M1–M5 | 5 个 MCP 工具 | `src/mcp/serve.rs:321` `p::encode_result(id, p::tool_call_result(…))` | `src/mcp/serve.rs:315` `pipeline::publish_value(OutletSpec::MCP_TOOLS, value)`（**同一调用点**） | Json / Customer / Whole |

> 机读版是 `src/domain/pipeline.rs` 的 `pub const OUTLETS: &[Outlet]`（33 条，带 `path` + `call_site` needle）。
> 判据 `tests/test_req107_outlet_pipeline.rs::test_req107_cli_five_customer_outlets_are_wired` /
> `…http_three_outlets_are_wired` / `…mcp_five_tools_share_one_pipeline_call` 逐条核 needle。

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

```rust
// src/domain/pipeline.rs
pub enum Track { Customer, Modeling, Internal, Static, ErrorPath }
impl Track { pub const fn gated(self) -> bool { matches!(self, Track::Customer | Track::Static) } }

pub enum Format { Html, Json, Text }
pub enum Granularity { Whole, PerNode }

pub struct OutletSpec { pub id: &'static str, pub track: Track, pub format: Format, pub granularity: Granularity }
// 13 个具名常量：OutletSpec::C1 / … / H3 / MCP_TOOLS / Z1 / Z2 / Z3 / D1

pub enum Product<'a> {
    CustomerHtml { view: &'a CustomerView, sampling: &'a [String] },
    CustomerJson(&'a CustomerView),
    Overview(&'a Overview),
    Text(String),
    ErrorText(String),
}

/// 文本轨唯一入口。命中 ⇒ Err；**不自己 exit**。
pub fn render(spec: OutletSpec, product: Product<'_>) -> Result<OutletRender, PipelineError>;
/// 结构化轨唯一入口（HTTP / MCP）。命中 ⇒ Err，消息逐字取自 mcp::tools::guard。
pub fn publish_value(spec: OutletSpec, value: Value) -> Result<GuardedValue, PipelineError>;
/// 错误路径（Z1）：safe_error（比闸更保守）。
pub fn publish_error_text(spec: OutletSpec, raw: &str) -> OutletRender;
/// 装配期前处理（/api/graph /api/node / MCP 五工具共用一份已剔视图）。
pub fn prepare_view(view: &CustomerView) -> FilteredView;

/// ⛔ 字段全私有、无公开构造器 —— 出口只有拿到它才能写出（AC-003 的类型层）。
pub struct OutletRender { text: String, report: DropReport, outlet: &'static str }
pub struct GuardedValue  { text: String, value: Value }
```

### 2.2 为什么这样切

| 切法 | 理由 |
| --- | --- |
| **「前处理」与「闸」分成两个阶段** | 判据不同（标识符形态 vs 建模术语）、命中后的动作不同（减一条 vs 整份不发）、失败语义不同（静默删条目必须报 stderr vs 拒绝发布）。顺序不可交换：先剔后闸才能让可减的条目减掉、不可减的仍到达闸门（REQ-104 的全部价值建立在这个顺序上）。 |
| **轨道进 spec、`gated()` 在管线里判** | 「拦不拦」从「出口自觉」变成「声明 + 单一判定点」。误标轨道 ⇒ 行为立刻变（TC-022 的行为半）。 |
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

**内容没有减少**：原名、条数、前 20 条上限都保留了（§3·③ 的裁定）。

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

### 7.1 提交（5 个，均为 `[Dev] … (关联: REQ-107)`）

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

`test_req107_red_lines.rs` 第一版把「新增行里出现字面量 ` #[ignore] `」当判据 ——
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
