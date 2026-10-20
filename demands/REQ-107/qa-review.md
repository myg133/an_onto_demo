# REQ-107 · 出口收口 —— QA Pre-merge 独立复验报告

> **审的对象**：`feature-REQ-107` @ `b55d07c`（工作区干净，7 个 `[Dev]` 提交 + 1 个 `[BA]` 登记提交）
> **代码基线**：`develop` `b714f48`
> **本机**：Windows + **无 PostgreSQL** + rustc 1.96.0
> **审核方式**：自己读源码 + 自己跑判据 + **自己当攻击者做 12 次变异**（不采信 Dev 的变异结论）
> **临时产物**：`.review-workspace/req107/qa-run|qa-logs|qa-mut|qa-golden-recompute/`（未跟踪）

---

## 1 · 结论

### **FAIL（阻断 3 条）**

一句话理由：**结构收口的「形」做到了（13 条出口确实接线、golden 可独立复现、红线零触碰），但 AC-003 的「实」没做到 —— 我实测出 4 种绕过形态全绿，其中「新增一个客户类出口而不接管线」正是 AC-003 逐字要求「测试立刻红」的那一幕。**

| AC | 我的判定 | 依据 |
| --- | --- | --- |
| AC-001 唯一客户类出口管线 | **不成立** | 管线确实唯一，但「拦不拦由轨道决定、出口无权决定」是假的：`OutletSpec.track` 是 `pub` 字段，出口一行 `spec.track = Track::Internal` 就把闸关掉，12 个判据目标全绿（变异 M10） |
| AC-002 13 条出口全部接线 | **成立**（判据偏弱，非阻断） | 13 条逐条我自己看到 `path:line`（§3）；但 H2/H3 的登记 needle 会被**文档注释**满足（变异 M2 实测：摘掉 H3 的管线调用后，专用登记判据仍 12 passed） |
| AC-003 漏接即红 | **不成立（本次最严重）** | 4 种绕过全绿：自造判据 / 公开构造器（非 `new` 名）/ 新增未登记出口 / 改轨道。详见 §4 |
| AC-004 三个卡点 | 成立（**卡点② 的「落地」是名不副实**，非阻断） | 三份裁定文本齐全；①③ 行为被钉住；② 只「声明」不「执行」（`Granularity` 全仓零运行时读取） |
| AC-005 零可见变化 | 成立（限我复核到的范围） | 独立复算 6/13 份 golden 全 MATCH；Z1 是唯一可见变化；stderr 变化已登记（但 §5.4 的「内容没有减少」不准确，见 §8） |
| AC-006 Z1 | **成立** | 我自己跑 `test_req107_scope_error_gate`：4 passed（脏名不回显 + 干净名仍回显 + 其余 4xx 不回显 + 反空转） |
| AC-007 建模豁免显式 | 成立 | 8 条 `Track::Modeling` + 逐条 `exemption_reason`；D1 真走管线，误标行为可观察（`track_manifest` 2 passed） |
| AC-008 Z2/Z3 | **成立** | 选「运行时闸 + 补断言」；我的 M9（description 塞「外键」）与 M11（instructions 模板正文塞「外键」）**都红** |
| AC-009 红线零触碰 | 成立 | 五文件 numstat 空；G1–G4 前 145 行**逐字节相同**；无新增 `#[ignore]`；无删测试；断言数只增不减 |
| AC-010 交付证据 | 成立（两处数字/措辞不准，非阻断） | 九项齐全；我复核的本机数字与 Dev 报告**完全一致**（没有编数） |

---

## 2 · 红线与守门器（命令 + 实际输出）

### 2.1 五个红线文件相对 `develop` 的 `git diff --numstat`

```powershell
cd D:\MyCodes\AI\an_onto_demo\feature-REQ-107
git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs src/mcp/tools.rs
```
**实测输出：空**（0 行）。⇒ 五个文件零改动（`src/mcp/tools.rs` 是**完全零改动**，不只是 `guard()` 内部）。

### 2.2 `property_filter.rs` 的 G1–G4 三段逐字节

我不采信「跑一条测试就算过」，直接按行取两边内容做 SHA-256：

```powershell
$base = @(git show develop:src/domain/property_filter.rs); $cur = @(Get-Content src/domain/property_filter.rs)
# 前 145 行（①→②→③→「以上三条均未封堵」整段）逐行比对
```
**实测**：
- 两边前 145 行**只有 0 处差异**（第 146 行起才是新增）；
- `sha256(第 1–145 行, develop) = 90978a8d761512535a44d5d380ab2df26b629c8272e92da15e358191e11451f0`
- `sha256(第 1–145 行, 现状  ) = 90978a8d761512535a44d5d380ab2df26b629c8272e92da15e358191e11451f0`
- ⇒ **逐字节相同**。新增段从第 146 行开始（`//!` + 「出口侧的不变量（REQ-107 · AC-003）」），是 A4 允许的**追加**，且**没有**把已有的 ①②③ 改写成「已封堵」。

`git diff develop -- src/domain/property_filter.rs` 是**单一 hunks、纯追加**（+12 / -0）。

### 2.3 无新增 `#[ignore]` / 无删测试 / 无放宽断言（我自己 grep）

```powershell
# 只看**真正的属性行**（去掉行首 + 与空白后以 #[ignore] 开头）
(git diff develop -- tests src) | ? { $_ -match '^\+' -and ($_ -replace '^\+','').TrimStart().StartsWith('#[ignore]') }
```
**实测输出：空**（只有若干**注释里提到** `#[ignore]` 的行，不算属性）。

| 项 | 实测 |
| --- | --- |
| `git diff develop --diff-filter=D --name-only -- tests` | 空（无删测试文件） |
| `-#[test]` / `-fn test_` 删除行 | 0 行 |
| 断言数 `assert*!`（develop → 现在） | `test_property_filter` 23 → **30**；`test_exit_ordering_req106` 11 → **11**；`test_binary_output_guard` 73 → **79** |
| `#[test]` 条数 | 9 → **10**；7 → **7**；7 → **7** |

⇒ **数量口径上没有任何放宽**。（「`assert_eq!` 改 `assert!`」这种语义放宽仍无机器判据，与 coverage-map §四-6 的登记一致。）

---

## 3 · 13 条客户出口逐条核对（我自己看到的 `path:line`）

> 下面每一行的行号都是我**在改后的文件里直接读到**的，不是抄 Dev 的映射表。

| # | 出口 | 管线调用点（我看到的） | 写出点（我看到的） | 闸在写出前？ | 我的判定 |
| --- | --- | --- | --- | --- | --- |
| C1 | `ontology show --html` → stdout | `src/cli.rs:1294-1300`（`publish_customer(OutletSpec::C1, Product::CustomerHtml{…})`） | `src/cli.rs:1301` `let html = rendered.as_str();` → `:1316` `emit_domain_text(html)` | ✅ | 接线 |
| C2 | 同上 `--out` → 文件 | 与 C1 **同一个** `rendered`（`:1294`） | `src/cli.rs:1305` `std::fs::write(p, html)`（**同一个 `html` 绑定**） | ✅ | 接线（见下方注①） |
| C3 | `ontology show` → JSON | `src/cli.rs:1329-1332`（`OutletSpec::C3` + `Product::CustomerJson`） | `src/cli.rs:1333` `let json = rendered.as_str();` → `:1336` `emit_domain_line(json)` | ✅ | 接线 |
| C4 | `ontology overview` → 文本 | `src/cli.rs:1367-1370`（`OutletSpec::C4`） | `src/cli.rs:1371` `let text = rendered.as_str();` → `:1372` | ✅ | 接线 |
| C5 | `ontology overview --json` | `src/cli.rs:1360-1363`（`OutletSpec::C5`） | `src/cli.rs:1364` `let json_text = …` → `:1365` | ✅ | 接线 |
| H1 | `GET /` 整页 | `src/server/http.rs:329-332`（`OutletSpec::H1` + `Product::Text(html)`） | `:333` `Ok(Html(rendered.into_text()))` | ✅ | 接线（Z4 由这条整页闸覆盖） |
| H2 | `GET /api/graph` | `src/server/http.rs:269-272`（`publish_json(OutletSpec::H2, …)`） | `publish_json` 内 `:204` `Ok(Json(rendered.into_value()))` | ✅ | 接线 |
| H3 | `GET /api/node` | `src/server/http.rs:294-297`（`OutletSpec::H3`） | 同上 | ✅ | 接线（逐节点=每个响应一个节点） |
| M1–M5 | 5 个工具 | `src/mcp/serve.rs:314-316`（**唯一**调用点 `publish_value(OutletSpec::MCP_TOOLS, value)`） | `:318-321` `rendered.text()` / `rendered.into_value()` | ✅ | 接线 |
| Z1 | `check_scope` 不匹配分支 | `src/server/http.rs:251-257`（`publish_error_text(OutletSpec::Z1, …)`） | `:258` `err(400, rendered.as_str())` | ✅（`safe_error` 比闸更保守） | 接线并修复 |
| Z2 | `tools/list` | `src/mcp/serve.rs:284`（`publish_value(OutletSpec::Z2, result)`） | `:285` `encode_result(id, rendered.into_value())` | ✅ | 接线 |
| Z3 | `initialize` | `src/mcp/serve.rs:252`（`publish_value(OutletSpec::Z3, result)`） | `:253` | ✅ | 接线 |

**出口层是否还有 `scan_modeling_terms(` / `tools::guard(` / `filter_view(` 直调？**
我不信判据，另跑了一次剥注释后的 grep：三个出口文件（`src/cli.rs` / `src/server/http.rs` / `src/mcp/serve.rs`）里 **0 处**。`filter_view(` 现在只在 `src/domain/property_filter.rs`（定义）与 `src/domain/pipeline.rs`（调用）。

### ⚠️ 核对时发现的两处「形似而实不至」

1. **`OutletSpec::C2` 是死常量**：`src/cli.rs` 里 `OutletSpec::C2` 的引用数是 **0**（我 grep 过）。C2 只是复用 C1 的 `html` 绑定——**字节正确**（AC 要求的就是同一份已过闸产物），但「13 条各自声明格式+轨道」实际是 **12/13**：C2 的声明没有落到代码里。
2. **H2/H3 的登记 needle 会被文档注释满足**：`src/server/http.rs:178` 的文档注释里有 ``[`OutletSpec::H2`] / [`OutletSpec::H3`]``，而 `missing_call_sites()` 用的是**未剥注释**的 `read_source`。这直接导致变异 M2（摘掉 H3 的真实管线调用）后 `test_req107_http_three_outlets_are_wired` **仍然 12 passed**（详见 §4.2）。

---

## 4 · AC-003 绕过实验（4 种 + 2 个附加形态，逐种给红/绿）

### 4.0 实验装置（先交代，避免把编译失败当执行）

- 变异在**独立 worktree** 做：`.review-workspace/req107/qa-mut`（`git worktree add --detach … b55d07c`），**没有碰 `feature-REQ-107` 的任何被跟踪文件**；
- 每次变异：改文件 → 记**被改文件 sha256** + `git diff` patch → 跑 12 个判据目标 → `git checkout -- .` 复原；
- 每次跑的是 `cargo test --test <name>`（12 个目标逐个跑，逐目标 exit code 判定），**全部编译成功**才算执行；
- 日志与 patch：`.review-workspace/req107/qa-logs/mut-<M>-{summary.txt,patch,test_*.log}`。

**基线（未变异）**：12 个目标 **全绿**（合计 **64 tests**：12+4+1+2+4+3+2+3+10+7+7+9 = 64）。

### 4.1 绕过 #1 —— 自造判据（**保留管线调用 + 另加一份真在生效的本地判据**）

```rust
// src/cli.rs（新增，模块级）
fn local_ok(s: &str) -> bool { !s.contains("key") }

// C3 分支（管线调用**原样保留**）
let json = rendered.as_str();
if !local_ok(json) { eprintln!("QA-M1: 本地判据命中，本次不写 stdout"); return Ok(()); }
crate::cli::emit_domain_line(json);
```
- 被改文件 sha256：`src/cli.rs = 37ecd86230741f050548c023bc5435ded00c7d677451eb07c2f76196c47793b9`
- **结果：12/12 目标全绿，0 红。**

⇒ **这就是历史上前一轮「9 条全绿」的 M3 形态**（新命名 → 不命中那 6 个拼写），而 Dev 的 M-2 用的是**已登记的名字** `scan_modeling_terms(&json)`，所以他们的变异能红。**判据挡不住「换名字」的那一半。**

> 关键：`tests/test_req107_outlet_pipeline.rs::test_req107_m3_keeps_shared_call_and_adds_local_criterion`
> 名字里写着 M3，但它第 ② 步的自证串是 `crate::domain::property_filter::filter_view(&cv); let leaked = …scan_modeling_terms(&json);`
> —— **用的是已知名字**。所以它证明的是「扫描器认得出已知名字」，不是「M3 形态会红」。

### 4.2 绕过 #2 —— 摘掉 H3（`/api/node`）的管线调用

把 `src/server/http.rs:294-297` 的 `publish_json(OutletSpec::H3, …)` 换成 `Ok(Json(serde_json::to_value(node)…))`。
- 被改文件 sha256：`src/server/http.rs = 88ecd64243dda4a3e88bf34a28237860d10b6295b6cac2df321c1eb06d71adf4`
- **结果：红了 2 条**
  - `test_property_filter::all_three_outlets_call_the_one_shared_function` ✅红（它剥注释后找 `OutletSpec::H3`）
  - `test_req107_guard_granularity::…cardpoint2…` ✅红（脏节点不再 500）
- **但**：`test_req107_outlet_pipeline`（含**专门**的 `test_req107_http_three_outlets_are_wired`）**12 passed / 0 failed**。

⇒ **挡得住（有红）**，**但主打 AC-002 的那条判据假绿**：needle 命中的是 `http.rs:178` 的**文档注释**。

### 4.3 绕过 #3 —— 把 `OutletRender` 字段改 `pub` / 加公开构造器

| 子形态 | 改法 | `cargo test --doc` | `test_req107_outlet_pipeline` | 判定 |
| --- | --- | --- | --- | --- |
| 3a 三字段全 `pub` | `pub text` + `pub report` + `pub outlet` | **RED**：`src/domain/pipeline.rs - domain::pipeline (line 27) … FAILED`，报错原文 **`Test compiled successfully, but it's marked compile_fail.`**（正是「私有性被破坏」，不是因为语法错） | RED（`type_layer_blocks_off_pipeline_construction`） | **挡得住（两层都红）** |
| 3b `pub fn new(String) -> Self` | 新增公开构造器，名字叫 `new` | **OK（没红）** —— 两条 doctest 只钉「字面量构造」与 `OutletRender::build`，**公开构造器不在其覆盖内** | RED（但只因 TC-008 里那句字符串 `!pipe.contains("pub fn new(")`） | 挡得住，**但靠拼写** |
| 3c `pub fn craft(String) -> Self` | 同上，**换个名字** | **OK（没红）** | **OK（12 passed）** | **挡不住：全绿** |

- 3c 被改文件 sha256：`src/domain/pipeline.rs = 7bb4e0f16dca9271b777422cbb9a854ab6059cce09138fb264e104d49ef522af`
- 3a 的 doc 实测原文（节选）：
  ```
  test src\domain\pipeline.rs - domain::pipeline (line 39) - compile fail ... ok
  test src\domain\pipeline.rs - domain::pipeline (line 27) - compile fail ... FAILED
  ---- … line 27 stdout ----
  Test compiled successfully, but it's marked `compile_fail`.
  test result: FAILED. 3 passed; 1 failed
  ```
- ⇒ **`compile_fail` doctest 钉的是「三个字段字面量构造」这一件事**；「加一个公开构造器」它**管不着**。3c 意味着：任何出口都能 `OutletRender::craft(未过闸的字节)` 然后写出，**类型层零拦截，源码扫描零拦截**。

### 4.4 绕过 #4 —— 新增一个客户类出口，但**不在 `OUTLETS` 表里**

在 `src/server/http.rs` 新增一条真实路由 `/api/raw`（**零闸、零剔除、不登记**）：

```rust
.route("/api/raw", get(get_raw))
async fn get_raw(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&*state.graph).unwrap_or_else(|_| serde_json::json!({})))
}
```
- 被改文件 sha256：`src/server/http.rs = fefdeac9c388d1c5b0982dadac614f5639e652a2cf3f216f0a6566f9141b6e2b`
- **结果：12/12 目标全绿，0 红。**

⇒ **挡不住**。而这一格正是 AC-003 的逐字要求：**「新增一个客户类出口而没接管线时，测试立刻红」**。
原因很清楚：33 条 `OUTLETS` 是**手工登记表**，判据只做「表 → 源码」单向 needle 对账，**没有任何判据枚举「源码里有哪些客户类写出点」**（TC-012 的 `unregistered_writes` 只扫 `src/cli.rs::ontology_cmd` 的 `println!/emit_domain_*` 行，且写作行必须**同一行**出现 `to_string_pretty` 等 producer，HTTP 路由完全不在扫描范围）。

### 4.5 附加形态 A —— 出口把**轨道**改成 `Internal`（闸被关掉，管线调用与剔除都保留）

```rust
// src/cli.rs C3 分支
let mut spec = crate::domain::pipeline::OutletSpec::C3;
spec.track = crate::domain::pipeline::Track::Internal;   // OutletSpec 的字段是 pub
let rendered = publish_customer(spec, crate::domain::pipeline::Product::CustomerJson(&cv));
let json = rendered.as_str();
crate::cli::emit_domain_line(json);
```
- 被改文件 sha256：`src/cli.rs = 7b1a9fec2a089470dbff8078550f789aca995bcd9cf8c23118bdc31d97fd0120`
- **结果：12/12 目标全绿，0 红。**

⇒ 这条直接证伪 AC-001 的「拦不拦由 `spec.track` 决定，**不由出口代码决定**」——出口**可以**决定，而且所有判据都看不见。

### 4.6 附加形态 B —— 「先写出、后判定」（REQ-106 的病）在出口侧复活

```rust
let raw = serde_json::to_string_pretty(&cv)?;
crate::cli::emit_domain_line(&raw);            // ← 未过闸的字节先出去
let rendered = publish_customer(OutletSpec::C3, Product::CustomerJson(&cv));
let json = rendered.as_str();
crate::cli::emit_domain_line(json);
```
- 被改文件 sha256：`src/cli.rs = b310f09c310ceaf41c9bb308b7c005bbe37082b3f5023c3b31b83bc18119f82d`
- **结果：12/12 目标全绿，0 红**（含 `test_exit_ordering_req106` 的 7 条）。

⇒ 详见 §5.2 —— 这是 A9 第 2 条「改紧还是放宽」的答案。

### 4.7 计分（按你的判定标准）

| 形态 | 结果 |
| --- | --- |
| #1 自造判据（保留管线调用） | 🔴 **全绿（挡不住）** |
| #2 摘掉 H3 的管线调用 | 🟢 红（2 条；但专用登记判据假绿） |
| #3a 三字段全 `pub` | 🟢 红（doctest + 源码判据） |
| #3b 公开构造器 `pub fn new` | 🟢 红（**仅**靠字符串拼写） |
| #3c 公开构造器 `pub fn craft` | 🔴 **全绿（挡不住）** |
| #4 新增未登记客户出口 | 🔴 **全绿（挡不住）** |
| A 出口改轨道 | 🔴 **全绿（挡不住）** |
| B 先写后判 | 🔴 **全绿（挡不住）** |

**⇒ 挡得住 3 种（其中 1 种只靠拼写）/ 挡不住 5 种。**
**按「只要有一种绕过能全绿，AC-003 就不算通过」：AC-003 不通过。**

> 补充一句公道话：Dev 的交付物**自己登记了**「全新命名的自造判据抓不住」（design §六 表格第二行「抓不住」列、`pipeline.rs:84-85`）。**问题是它在同一张表里又写「要么是新造函数 ⇒ 由第三层抓」** —— 第三层（登记对账）只抓「新出口」，抓不住「已登记出口里新加的一份判据」。这句话是错的，而我用 M1 把它证伪了。
> 真正的问题不是「没登记边界」，而是**边界比 AC-003 要求的宽**：AC-003 逐字要求「新增出口 ⇒ 立刻红」，这一条现在是**绿的**。

---

## 5 · A9 三条改写测试的变异结果（逐条判「改紧还是放宽」）

### 5.1 A9 #1 · `test_property_filter::all_three_outlets_call_the_one_shared_function`

**变异**：把 `src/server/http.rs:481` 的 `prepare_view(view)` 换回直调 `filter_view(view)` + 自拼 stderr 报告（模拟「某出口恢复直调判据」）。

- 被改文件 sha256：`src/server/http.rs = b54f7deb10e66ec6451734c922034e4efc0955092d1726464c7987a4dbaf361b`
- **结果：红 4 条**
  - `test_property_filter::all_three_outlets_call_the_one_shared_function` ✅
  - `test_req107_outlet_pipeline::{outlet_files_have_zero_criterion_call_sites, m3_keeps_shared_call_and_adds_local_criterion, no_second_judgement_outside_pipeline}` ✅

**判定：改紧了。** 旧的 `contains("property_filter::filter_view")` 在这个变异下**仍然是绿的**（它只问「调了没有」），新判据红。这一条与 A3 的裁定一致。

### 5.2 A9 #2 · `test_exit_ordering_req106` 的两条（**最危险的那条**）

| 变异 | 被改文件 sha256 | 结果 |
| --- | --- | --- |
| **M6**：把管线里的「判定」挪到「产物构造」之后（`let out = build(...); gate_text(...)?; Ok(out)`） | `pipeline.rs = 5f561fd719a3938f30a48fae34d1d740437d8c96c4ca34a830d2e23a6738713b` | ✅ **红 2 条**（`the_json_exit_judges_before_it_writes`、`the_json_exit_writes_outside_the_refusal_branch`） |
| **M7**（§4.6）：管线**一行不动**，只在**出口**把写出挪到管线调用之前 | `cli.rs = b310f09c…` | 🔴 **7 passed / 0 failed（全绿）** |

**判定：放宽了 —— 且正好放宽在 REQ-106 那条不变量的落点上。**

理由（代码可证）：改写后的两条断言锚定在 `src/domain/pipeline.rs` 的 `Product::CustomerJson(view) => {…}` 与 `Product::CustomerHtml{…} => {…}` **arm 内部**，问的是「闸调用是否在 `Ok(OutletRender::build(…))` 之前」「产物构造的缩进是否不深于闸」。它们**完全不再看 `src/cli.rs`**：
- 出口有没有在调用管线**之前**写出 → 无判据；
- 出口有没有在管线产物之外**另写**一份字节 → 无判据（G-1 的 ③ 只要求「存在一条 `let <binding> = …as_str()`」，M-P3 的变异也只覆盖「把那条派生行改掉」）。

⇒ **「原来能抓的变异（先写后判），现在抓不到」**，触犯 A9 的 ⛔ 判定标准。
（唯一可能补救的是需 DB 的真进程 e2e `test_domain_print_sites_binary_req078` —— 本机不可执行；而且它在**干净夹具**上 raw 与 filtered 字节相同，也抓不到。）

### 5.3 A9 #3 · `test_binary_output_guard` 的 G-1 / G-4

**变异**：把 `src/cli.rs:1301` 的 `let html = rendered.as_str();` 改成 `let html = String::from("客户视图（QA-M8 手拼）");`（写出的绑定不再从产物派生）。

- 被改文件 sha256：`src/cli.rs = 2fdbc682b091beb51661a1c8d9ca2fef7dd4b59146ec94bbd3cd8ec519395802`
- **结果：红 4 条**
  - `test_binary_output_guard::{g1_criteria_receive_exactly_what_is_printed, g4_guard_is_not_vacuous}` ✅
  - `test_req107_outlet_pipeline::{cli_five_customer_outlets_are_wired, removing_an_outlet_breaks_the_registry}` ✅（needle `emit_domain_text(html)` 不再命中）

**判定：覆盖了原有意图，但判别力**换了维度**。**
- 覆盖到的：① 管线入口在不在；② 出口函数体里有没有判据直调；③ 写出绑定是不是 `.as_str()` 派生的 —— 这三条在 M8 下都红，且 G-4 自己逐站跑 M-P1/P-M2/M-P3 三个变异（我读过代码，`same_binding` 族为 0 也被显式断言，不会静默缩成 0 段）。
- **丢掉的**：旧的「判据收到的串 == 打印出去的串」是**逐词元相等**；新的 ③ 只是**形状匹配** `let <binding> = …as_str()`。所以「另有第二条写出路径」这一格它看不见（M7 实证）。
- 另一处：② 的判据名单仍是**拼写**（5 个名字），M1 实证挡不住新名字。

**⇒ 不是「只换了个名字」，但也**不能算「等价覆盖」**：它把「时序」这一维彻底丢了（与 §5.2 是同一个洞，两处判据同时失守）。**

---

## 6 · 零可见变化与 golden 的可信度

### 6.1 `PROVENANCE.txt` / `SHA256SUMS`

- 首行 = **`b714f48`** ✅（第 1 行，我直接 `Get-Content` 读的）。
- `SHA256SUMS` 13 行，与 13 个文件**逐个互校 MATCH**（我用 `Get-FileHash` 独立算，不用测试里的 SHA-256 实现）。
- `PROVENANCE.txt` 写明了 C2 复用 `C1_html.txt`、Z1 不在 golden 集里。

### 6.2 我自己在**基线 worktree** 独立复算（不是重跑 Dev 的生成器，也不是同义反复）

做法：在 `.review-workspace/req107/base`（detached @ `b714f48`）里写了一份**我自己的**临时测试 `tests/qa_golden_recompute.rs`，用**基线 API**（`render_html` / `filter_view` / `render_overview_text` / 基线路由 `state_from_view`+`build_router`）重算 **6 份**字节，再用 PowerShell 独立取 sha256 与入库 golden 比：

| 文件 | 我的复算 sha256 = 入库 golden | 字节数 |
| --- | --- | --- |
| `C1_html.txt` | `48650bce…5495b7` **MATCH** | 5665 |
| `C3_json.txt` | `1dd509c5…e7046a` **MATCH** | 1226 |
| `C4_overview.txt` | `2ee6e71f…80598d` **MATCH** | 986 |
| `C5_overview_json.txt` | `7db9bb01…cdacbf` **MATCH** | 1737 |
| `H1_index.html` | `e8753c75…8df3974` **MATCH** | 11215 |
| `H2_graph.json` | `6b06847a…c7fc8b` **MATCH** | 393 |

⇒ golden 的**来源与内容都可复现**；不是「用改完的代码生成 golden」。

（复算脚本留在 `.review-workspace/req107/base/tests/qa_golden_recompute.rs`，未跟踪；base worktree 的 `src/bin/ontok-gate-preflight.rs` 挪出/挪回后 sha256 一致：`C7CF95E2…322D`。）

### 6.3 零可见变化的**口径漏洞**（非阻断，登记）

- `test_req107_zero_visible_change.rs` 的 C1–C5 是**用生产函数复刻**（基线兼容需要），并不经过 `src/cli.rs`；H1/H2/H3/M1–M5 走真实路由/`handle_line`。⇒ 「CLI 出口的字节」这一格由 `test_req107_pipeline_bytes`（管线产出 == golden）间接保证，**不是真进程**。Dev 已在文件头与 §8-8 登记，我认可这个披露。
- **Z1 是唯一允许的可见变化**：我用 `test_req107_scope_error_gate`（4 passed）复核了「脏名不回显 / 干净名仍回显 / 其余 4xx 不回显 / 反空转」，与 AC §七 A2 一致。
- Dev §5.4 登记了 stderr 报告形状变化 —— 我核对**属实**，但「**内容没有减少**」这句不准确：旧 C1 打印 `page.skipped.join("、")` **不限条数**，新形状把对象名并入「明细（前 20 条）」的**总上限**。影响面只限 stderr（AC-005 口径外），但登记措辞应改。

---

## 7 · Z1 / Z2 / Z3 + 卡点②③ 的复核

### 7.1 Z1（AC-006）—— 双向都对

- 脏项目名 `外键登记表`：400，正文**不含** `外键登记表`、也不含词干 `外键`，且仍有人话（含 `error`）——这是**行为**验证（`oneshot` 真打路由），不是读代码。
- 反向对照组：干净名 `样例项目` **仍原样回显** ⇒ 没有一刀切成通用文案。
- 代码路径：`http.rs:251-258`，`publish_error_text` → `tools::safe_error`（我读了 `safe_error` 本体：`scan_modeling_terms` 命中 ⇒ 整句换成通用说明；未命中 ⇒ 原样返回）。

### 7.2 Z2 / Z3（AC-008）—— 选的是「运行时闸 + 补测试断言」，判别力够

| 变异 | 塞入内容 | 结果 |
| --- | --- | --- |
| M9 `tools/list` 的 `description` | `list_concepts` 的 description 里加「外键」 | ✅ 红：`test_req107_tools_list_descriptions_and_titles_are_asserted`（报 `带建模术语 ["外键"]`） |
| M11 `initialize` 的 `instructions` **模板正文** | 「该版本已冻结（业务模型与**外键**映射不再变动）」 | ✅ 红：`test_req107_initialize_instructions_template_body_is_guarded` |

- 被改文件 sha256：M9 `tools.rs = 052d75e1…12f502f`；M11 `tools.rs = 27cf09ac…9dbf8`。
- 结论：**「运行时闸」这个选择本身判别力是够的**（两处都会红），且 `description`/`title` 从「零断言」补成了「静态扫描 + 响应体逐字比对」。
- ⚠️ 一处口径要写清（非阻断）：Z2/Z3 命中时返回 `p::encode_error_line(id, INTERNAL_ERROR, …)`，即**把 `tools/list` / `initialize` 变成 JSON-RPC 错误**。今天不可能命中（静态文本干净），但这是「一个静态文本命中就废掉整个 MCP 会话发现」的行为，属**政策**选择，建议在设计文档里明写一句。

### 7.3 卡点② 闸粒度 —— **「声明」落地了，「执行」没落地**

- 事实（我 grep 全仓）：`Granularity` 只出现在**定义**、**常量**、**`as_str`** 与**测试断言**里，`render()` / `publish_value()` **一次都没读它**。
- 所以「管线**按声明执行**」（design §三·②）这句话**不成立**：H3 的「逐节点」来自 **HTTP handler 每个响应只装一个节点**，不是来自 `Granularity::PerNode`。把 H3 的 spec 改成 `Whole` 不会有任何行为变化（无判据会红）。
- 行为侧**是**被钉住的（这点要给分）：`test_req107_guard_granularity` 的 ②③④ 三点 —— `/api/graph` 脏节点整份 500、`/api/node` 干净节点 200、`/api/node` 脏节点 500 —— 我跑了，4 passed。
- ⇒ 卡点②的**题干**（整份 vs 逐节点）有裁定、有代价论证、有行为判据；**但「把自决收进 spec」这一层是装饰性的**。判定：非阻断，但 AC-004 的「落地」只能算一半。

### 7.4 卡点③ stderr 报告形状 —— 统一了，但「内容没有减少」不准确

- 形状确实唯一：`DropReport::stderr_line()` + `DROP_REPORT_PREFIX`，出口不再自己拼；`src/**` 里 `.report()`（`FilteredView::report`）现在**零调用**。
- 但：对象名明细原来**不限条数**，现在与属性明细共享「前 20 条」上限（`pipeline.rs:340-348`）。见 §6.3。

### 7.5 管线里「看起来过闸、其实能绕过」的路径（我逐条读出来的）

| # | 路径 | 说明 | 严重度 |
| --- | --- | --- | --- |
| 1 | `OutletSpec` 的 `track` 是 `pub` | 出口可把任何 spec 改成 `Track::Internal`/`Modeling` ⇒ 闸被跳过（M10 全绿）。设计文档写的是「不由出口代码决定」，事实相反 | **高** |
| 2 | `publish_error_text(spec, raw)` 是**公开的 `OutletRender` 工厂** | 任意 `&str` → 可写出的产物；只过 `safe_error`（命中就整句换掉，所以不漏词），但它使「无公开构造器」这句话**不精确**；配合 3c 的 `craft` 就是完整绕过 | 中 |
| 3 | `Product::Text(String)` 接受任意文本 + 非闸轨道 | D1 就是活例子（`cli.rs:1253-1256`）。任何出口都能 `Product::Text(未处理字节)` + `OutletSpec::INTERNAL` 拿到干净产物 | 高（与 #1 同源） |
| 4 | `prepare_view` 返回**未过闸**的 `FilteredView` | 公开 API；出口可以拿它自己序列化后写出（与 `Product::Text` 组合就是绕过） | 中 |
| 5 | `Granularity` 不参与执行 | 见 §7.3 | 低 |
| 6 | `STATIC_TEMPLATES` | 只是常量，判据只断言它「登记了 Z4」；Z4 的实际覆盖来自 H1 的整页闸（这条成立） | 低 |
| 7 | `prepare_view` 幂等性 | 连续两次 `filter_view` 的第二次是 no-op（条目已删）；无测试钉住，但对当前实现无害 | 低 |

---

## 8 · 阻断项 / 非阻断项

### 8.1 阻断项（3 条）

| # | 阻断项 | 一句话说清「哪条 AC 不成立」 | 证据 |
| --- | --- | --- | --- |
| **B1** | **AC-003 的「漏接即红」对 HTTP 出口不成立** | 新增一条客户类路由 `/api/raw`（零闸、不登记）后 **12/12 判据全绿** ⇒ 「新增一个客户类出口而没接管线时测试立刻红」不成立 | §4.4（sha256 `fefdeac9…1b6e2b` + patch） |
| **B2** | **AC-001 的「拦不拦不由出口决定」不成立** | `OutletSpec.track` 是 pub 字段：出口一行 `spec.track = Track::Internal` 即跳过发布闸，**12/12 判据全绿**；同一形态还包括「自造判据」（M1）与「公开构造器 `craft`」（M3c） | §4.5 / §4.1 / §4.3 |
| **B3** | **A9 #2 触犯「原来能抓的变异现在抓不到」** | 把「未过闸的字节先写到 stdout」放回出口（REQ-106 的原始病），`test_exit_ordering_req106` **7 passed**、全部 12 个目标 **0 红** ⇒ 「先判后写」在出口侧已无守卫 | §5.2 |

> B1 与 B2 可以一起修（同一根因：**「管线入口」被当成了「过了闸」的同义词**）。B3 是判据覆盖面回退，需要单独补一条「出口侧时序/唯一写出」断言。

### 8.2 非阻断项（建议在合并前或紧随其后修，但不单独否掉本次）

| # | 项 | 证据 / 建议 |
| --- | --- | --- |
| N1 | H2/H3 的登记 needle 被**文档注释**满足（`http.rs:178`） | 变异 M2：摘掉 H3 真实调用后专用登记判据仍 12 passed。建议 `missing_call_sites` 改用 `strip_comments`（`tests/common/mod.rs` 里已有）并只认代码行 |
| N2 | 卡点② 的「声明」不参与执行（`Granularity` 零读取） | §7.3。要么在 `publish_value`/`render` 里真的按声明分支，要么把设计文档「管线按声明执行」改成「粒度由出口调用形态决定」 |
| N3 | §5.4「内容没有减少」不准确（对象名明细由不限条数变为共享前 20） | §6.3 |
| N4 | `Outlets` 的 `C2` 声明是死常量（`OutletSpec::C2` 出口文件引用 0 次） | §3 注①：字节正确，但「13 条各自声明」实际 12/13；建议 C2 的 spec 至少在写文件处被引用（或文档改成「C2 复用 C1 的声明」） |
| N5 | 报告 §7.1 写「提交（5 个）」，实际 `feature/REQ-107` 有 **7 个 `[Dev]` 提交**（+1 个 BA 登记提交） | `git log --oneline b714f48..HEAD` 实测 8 条；属文档数字不准 |
| N6 | `publish_error_text` 使「无公开构造器」的说法不精确 | §7.5 #2；建议文档措辞改为「唯一公开的产物工厂只有错误路径那一个」 |
| N7 | Z2/Z3 命中时的政策（整条 `tools/list`/`initialize` 变 JSON-RPC 错误）未在设计里明写 | §7.2 |
| N8 | AC-009-4「不许放宽断言」无机器判据 | coverage-map §四-6 与 Dev §8-10 都已登记；我按断言计数侧证「只增不减」 |

### 8.3 我确认「没问题」的部分（避免这份报告被读成全盘否定）

- 13 条客户出口**真的**全部接线（逐条 `path:line`，§3），出口层**真的**零判据直调（剥注释后 grep 0 命中）；
- `guard()` 本体零改动、`property_filter` G1–G4 逐字节未动、四红线文件零改动；
- 三条既有测试**没有删、没有 ignore**，断言数只增不减；
- golden 来源可复现（6 份独立 MATCH）、`SHA256SUMS` 互校通过、Z1 是唯一可见变化；
- Z1/Z2/Z3 的修复**有效且有判别力**（M9/M11 都红）；
- Dev 报告里的本机数字与我实测**逐条一致**（12/4/3/1/4/2/3/2、10/7/7/9/6/10、lib 167+2、doc 4、mcp 6+25+1）——**没有编数**。

---

## 9 · 我无法确认的项（如实列，不用推测填空）

1. **全量 `62 suite / 846 passed @ 7f26fa2`**：本机不可复核（既有 bin 的 `std::os::unix` 代码），我也不打算声称复核过。
2. **需 DB 的用例**（本机无 PostgreSQL，全部如实标「环境不具备」，**没有算作通过**）：
   - `test_mcp_server`：**6 passed / 25 failed / 1 ignored**，25 条**全部**是 `tests/common/mod.rs:225` 的 `MissingEnv("ONTOK_TEST_DATABASE_URL")`；
   - `test_domain_print_sites_binary_req078`：4 failed（同一个 env）；
   - `cargo test --lib`：167 passed / 2 failed（`db::apply` 与 `extract::introspect` 各 1，都是同一个 env）；
   - 真进程 e2e（TC-017 一类）与「真数据下 13 条出口字节不变」**未复核**。
3. **`tests/test_retained_names_req084.rs` 与 A8 绕行的冲突（**新发现的环境事实，Dev 报告没写**）**：该文件 `include_str!("../src/bin/ontok-gate-preflight.rs")`；把 bin 挪出后它**编译失败**（`error: couldn't read src\../src/bin/ontok-gate-preflight.rs`）。⇒ **A8 的绕行只对「不 include_str! 那个 bin」的测试目标成立**；全量 `cargo test --no-run` 在挪出状态下必然失败。我用 `cargo test --test <name>` 逐目标跑，未受影响。
4. **`feature-REQ-107` 历史上有没有「改了又改回去」**：只能看逐 commit diff；我只做了 tip vs `develop` 的比对 + 红线文件零差异。
5. **golden 我只独立重算了 6/13 份**（C1/C3/C4/C5/H1/H2）。H3 两份与 M1–M5 未独立重算；它们由 `test_req107_pipeline_bytes`（生产管线 == golden，我跑了 1 passed）间接覆盖。
6. **变异三元组的完整性**：我采集的是「被改文件 sha256 + `git diff` patch + 逐目标红/绿」（纪律 Q 对源码扫描型判据的口径）；**测试二进制 sha256 未逐次留存**（cargo 每次重编都会换 hash，我没有逐次抓）。这一项我不声称做到了纪律 G 的完整三元组。
7. **`Product::Overview` 的「两条格式都过闸」是否有业务意义**：属既有过度拦截，AC-005 要求原样保留，我未评价其合理性。
8. **真数据（demo3 v2）上 stderr 报告形状变化的运维影响**：无 DB，未实测。

---

## 附录 A · A8 绕行留证（我在 `feature-REQ-107` 里做的两次独立运行）

```
# 第 1 次（16 个目标，跑到第 2 个目标因缺 DB 失败后 cargo 中止；随后单独跑 doc 与 lib）  .review-workspace/req107/qa-logs/sha.txt
BEFORE_SHA256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
AFTER_SHA256 =C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
SAME=True

# 第 2 次（14 个目标，--no-fail-fast）  .review-workspace/req107/qa-logs/sha2.txt
BEFORE_SHA256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
AFTER_SHA256 =C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
SAME=True
```
- 挪出方式：`Move-Item` 到 `.review-workspace/req107/qa-stash/`，跑完 `Move-Item` 挪回；**没有删、没有改内容、没有动 `Cargo.toml`、没有加 `#[cfg(unix)]`**。
- 跑完后 `git status --short` = **空**（工作区干净），`git rev-parse HEAD` = `b55d07c2f30b505486aebbdd8820d32f3f78b19a`。

## 附录 B · 我的基线实测（供对账）

| 命令 | 实测 |
| --- | --- |
| `cargo test --doc` | **4 passed / 0 failed**（两条 `compile_fail` 均 ok） |
| `cargo test --lib` | **167 passed / 2 failed**（2 条需 DB） |
| `test_req107_outlet_pipeline` | 12 passed |
| `test_req107_guard_granularity` | 4 passed |
| `test_req107_pipeline_bytes` | 1 passed |
| `test_req107_red_lines` | 2 passed |
| `test_req107_scope_error_gate` | 4 passed |
| `test_req107_static_texts` | 3 passed |
| `test_req107_track_manifest` | 2 passed |
| `test_req107_zero_visible_change` | 3 passed |
| `test_property_filter` | 10 passed |
| `test_exit_ordering_req106` | 7 passed |
| `test_binary_output_guard` | 7 passed |
| `test_graph_server` | 9 passed |
| `test_guard_manifest_req081` | 6 passed |
| `test_model_view` | 10 passed |
| `test_mcp_server` | 6 passed / **25 failed / 1 ignored**（全部 DB） |
| `test_domain_print_sites_binary_req078` | **4 failed**（DB） |

## 附录 C · 建议的最小修法（给 BA 派单用，不是我在实现）

1. **B1（AC-003）**：加一条「**写出点枚举**」判据 —— 对三个出口文件枚举所有 `route(`、`Html(`、`Json(`、`emit_domain_*(`、`println!` 的**客户数据写出点**（剥注释），要求每个点要么在 `OUTLETS` 里登记、要么在显式的「非客户」白名单里。**不要求 AST**，枚举路由表 + 函数返回类型已经够抓住 M4 这一格。
2. **B2（AC-001/AC-003）**：把 `OutletSpec.track` 改**私有**（配 `pub const fn track()` 只读），并给测试留一个 `#[cfg(test)]` 的 `with_track`；同时把「写出 API 只收 `&OutletRender`」——`emit_domain_text/line` 的入参从 `&str` 改成 `&OutletRender`（或 `&GuardedValue`），让「手拼字节直接写」在**编译期**不成立。这一步能同时收掉 M1/M3c/M10 的一部分。
3. **B3（A9 #2）**：在 `ontology_cmd` 上加一条：**该函数里每一条 `emit_domain_*` 的实参都必须是某个 `let x = …as_str()` 的 `x`，且该 `as_str()` 的源必须是 `publish_customer(` 的返回值；`publish_customer(` 的源码行号必须小于所有写出行号**。这条是纯拼写+行号，成本很低，但能把 M7 变红。
4. **N1**：`missing_call_sites` 改用 `strip_comments`（现成的），否则 needle 永远可能被注释满足。
5. 文档订正：§7.1 提交数（5 → 7）、§5.4「内容没有减少」、design §三·②「管线按声明执行」、`pipeline.rs` 模块文档「无公开构造器」。

---

> **给我自己的一句提醒（防止这份报告被误读）**：Dev 这一轮做的东西**大部分是真的**——
> 管线是真的、13 条接线是真的、golden 是真的、红线是真的、Z1/Z2/Z3 的修复是真的。
> 但 AC-003 是这个需求的**全部意义**（「漏接即红」），而它现在**挡得住 3 种、挡不住 5 种**，
> 其中「新增一个客户出口」这一种是 AC 逐字点名的场景。⇒ **不能按「已收口」合并。**
