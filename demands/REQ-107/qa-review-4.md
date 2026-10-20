# REQ-107 · 第 4 轮修复的**复验**报告（QA 第 5 轮）

> **对象**：`feature/REQ-107` @ `757f88c`（基线 `develop b714f48`）。
> **判据**：`acceptance.md §七 A13（三条线）+ A14（六种形态 + B2 + 三条登记）` —— **按 BA 划死的线判，不加码**。
> **装置**：独立 worktree `.review-workspace/req107/qa4`（detach @ `757f88c`，收工 `git status` 空）。
> **A8 绕行**：`src/bin/ontok-gate-preflight.rs` 临时挪出 → 跑完挪回，sha256 前后
> `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D` = 同（**SAME=True**，本轮做了 2 次挪出/挪回，两次都核过）。
> ⛔ 没有改 `feature-REQ-107` 的任何被跟踪文件（收工 `git status --porcelain` **空**），没有 push。

---

## 1 · 结论

### **PASS · 带 5 条非阻断（阻断 0 条）**

一句话：**A14 点名的六种形态 + Err 形态全部真红（7/7，全是运行期判红、零编译失败）；B2 真的修好了（文件末尾追加零闸 handler ⇒ 71/3 红，且我用一条独立探针直接读了 `prod()` 的三个性质）；上一轮两条阻断（B1 的 `Err` 通道、B2 的 `prod()` 截断）都实测封住；四条登记逐条对账，与我的实测一致，没有把「已登记」写成「已覆盖」。**
本轮新发现的 `Json::from(..)` / `.map(Json)` 拼写族**确实全绿**（74/0），但按 BA 的尺度（不要求穷尽、只要求已知绕过有交代）它属于**新花样**而非「声称封住却全绿」，因此**列非阻断 + 建议登记**，不升级为阻断。

### A14 过关线逐条对照

| 过关条件 | 我的实测 | 判定 |
| --- | --- | --- |
| A13-1 · F1/F2 修好，「老实填表」再试仍红 | `M6b` = **72 / 2**（`t8` + `t9`） | ✅ |
| A13-2 · F3/F4 封住或登记 + QA 认可危害评估 | `M5d` **73/1**、`M10b` **73/1**、`Err` 形态 **73/1**；P-2 改写覆盖 `Ok`/`Err` 两通道，残余 P-4 实测**与登记一致** | ✅ **认可** |
| A13-3 · 未退化项不退化 | 13 条接线 / 五红线 / `guard()` / 无删测试 / 无新 `#[ignore]` / 断言 23→30·11→11·73→79 / Z1–Z3 / golden 13/13 / 基线 74 | ✅ 逐项 |
| A14 · 六种形态全红 | M6b 72/2、M7 73/1、M5d 73/1、M10b 73/1、a′ 72/2、d2 71/3 **+ Err 形态 73/1** | ✅ 7/7 |
| A14 · 三条登记的危害评估经 QA 认可 | P-1 / P-2 / P-3 + 本轮新登记的 P-4：**四条均诚实、够用**（见 §4） | ✅ |

> 口径：本节所有「a/b」= 13 个目标的 `passed/failed`。基线（未变异）= **74 passed / 0 failed**。
> 所有变异**均编译通过并跑完测试**（日志里无 `error[E…]`；`run.ps1` 摘出的
> `COMPILE| error: test failed …` 是 cargo 在**测试失败之后**的收尾，不是编译失败）。

---

## 2 · P0-1 · 七种现成形态（逐个复跑，全部运行期判红）

**基线 sha256（本轮未变异）**：
`http.rs 6BEF88DF4FC9E5055A3807FC889692335BEAC86B75EF9868D8C3F4C98E5E94CC`、
`pipeline.rs 88C28710230A63A2DC2160A225FFA6A6B3417A476C854AA169707A8A498F034D`、
`cli.rs C5CACC1ACAAFB6C80DCF3A872774514B6CFEE47A9972E45C370ECC82EFD3BAFC`、
`test_…_source_enumeration.rs EC5E747B1187D3B7C42A6130D04693999DE461604602A46FF4541526BDD56E28`、
`test_…_outlet_pipeline.rs B395D8D0C183D3CFBCC8BB5B53F20901108F1CE46A37C9562A9855D2DF3EFC7D`
—— 前缀与 Dev §12.6 记的基线 sha **逐个对上**（我用的是全 64 位）。

| # | 形态 | 被改文件 sha256 前 → 后 | 13 目标 | 红在哪（实测文案摘要） |
| --- | --- | --- | --- | --- |
| 1 | **M6b 老实填表**（`/api/raw` 零闸 + `OUTLETS` 加 `H4`(Pipeline, 借 H2 spec) + `OUTLET_FNS` + 计数 33/13→34/14 + ID 集合） | `http.rs 6BEF88DF→A55496F6`；`pipeline.rs 88C28710→5777AA64`；`src_enum EC5E747B→7D45DC6C`；`outlet_pipeline B395D8D0→65DDE32B` | 🟢 **72 / 2** | `t8`：「H4 声明了 Pipeline，但 http.rs 里找不到携带 `OutletId::H4` 的**管线调用** —— 「填表」不是接线（F1）」；`t9`：`:313 的响应体是 serde_json::to_value(&*state.graph)…` |
| 2 | **M7 白名单**（`println!` 打客户 JSON + `NON_CUSTOMER_WRITES` 加 needle/计数） | `http.rs 6BEF88DF→A257A467`；`src_enum EC5E747B→512902BB` | 🟢 **73 / 1** | `t1`：「`:280 println!(（白名单 needle `serde_json::to_string_pretty(&*state.graph)`）的窗口里含 domain 生产者 `to_string_pretty(`」（F2） |
| 3 | **M5d 直写 stdout**（`std::io::stdout().lock().write_all(raw)`） | `cli.rs C5CACC1A→4A7E07B6` | 🟢 **73 / 1** | `t1`：`cli.rs:1350 std::io::stdout` + `:1351 write_all(`（F3 ×2） |
| 4 | **M10b** 改已有 handler + 条件分支 + `Response::builder().body(Body::from(raw))` | `http.rs 6BEF88DF→90FC9A35` | 🟢 **73 / 1** | `t1` **4 处**：`:282 Response::builder`、`:283 StatusCode::OK`、`:285 .body(`、`:285 Body::from(` |
| 5 | **a′** 本地 newtype 伪造 `.into_value()` + `if false` 死调用（QA 原样） | `http.rs 6BEF88DF→BC565CA8`；`pipeline.rs 88C28710→A92C1E69`；`src_enum EC5E747B→73C3DF87`；`outlet_pipeline B395D8D0→29737DA8` | 🟢 **72 / 2** | `t9`：「`:292 的 `qa_rendered` 绑定自 `match Ok::<QaRaw, Response>(QaRaw(…))` —— 被匹配的表达式**不是管线调用**（B3②）」；`t10`：「http.rs: 定义了 `fn into_value`」 |
| 6 | **d2** 内联 `mod` 同名 `get_graph` + **整块追加到 `http.rs` 文件末尾**（踩 B2）+ 老实填表 `H4` | `http.rs 6BEF88DF→0AD76D64`；`pipeline.rs 88C28710→A92C1E69`；`src_enum EC5E747B→65E51815`；`outlet_pipeline B395D8D0→29737DA8` | 🟢 **71 / 3** | `t5`：「`fn get_graph` 定义了 2 次 —— 登记粒度是 (文件, 函数名)，重名等于**免登记**（B5）」；`t9`：「`:1273` 绑定自 `match Ok::<QaRaw, …>` 不是管线调用」；`t10`：「定义了 `fn into_value`」 |
| 7 | **Err 形态** `return Err((StatusCode::OK, raw).into_response())`（4 行、签名不动，QA 上一轮原样） | `http.rs 6BEF88DF→A2FE2AC4` | 🟢 **73 / 1** | `t1`：「`:284 into_response(` —— 不在 `INTO_RESPONSE_SITES` 钉住的错误通道里」+「`:284 StatusCode::OK` —— Err 通道不许回 2xx」 |

**逐条结论**：
- 七种形态**全部红**，且都是 `source_enumeration` 这一目标红、其余 12 个目标全绿（与被攻击面一致）；
- 没有一次是「编译失败挡住」（编译器错误计数 = 0；每次变异都跑到 `test result:`）；
- 每次变异都记了 `sha256 前/后`（`qa4-logs/<ID>.json`）、`patch`（`qa4-logs/<ID>.patch`）、`完整 cargo 输出`（`qa4-logs/<ID>.log`），并用 `restore.js` 从 stash **逐字节复原 + 核 sha**（每次 `RESTORE_OK` + `CLEAN_AFTER_RESTORE`）。

**这一轮最关键的翻绿点**（上一轮是「全绿」的三格）：
- a′：`73/0` → **72/2**（`t9` 的 B3② + `t10` 两道独立门）；
- d2：`73/0` → **71/3**（`t5` 的 B5 去重 + `t9` + `t10`）；
- Err 形态：`73/0` → **73/1**（`into_response(` + `StatusCode::OK`）。
> 诚实说明（与 Dev §12.6 一致）：d2 下 **`t8` 不红** —— 死调用与写点同名同函数，`t8` 的「同函数」判据看不见死代码；挡住它的是 `t5`/`t9`/`t10`。这不是缺陷，是判据分工。

---

## 3 · P0-2 · B2 是否真修好（本轮最重要的复验）

### 3.1 「测试模块之后追加零闸 handler」—— 现在**必须红**，实测红

**变异 `B2only`**（`http.rs 6BEF88DF→EB59F3AB`；`src_enum EC5E747B→CCB27088`）：
在 `http.rs` **第 1246 行（文件最后一行 `}`）之后**追加 3 行零闸 handler（+1 行 doc 注释）、
在 `build_router` 加一行 `.route("/api/b2raw", get(qa_b2_raw))`、**零登记**：

```rust
async fn qa_b2_raw(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&*state.graph).unwrap_or(serde_json::Value::Null))
}
```

**实测：13 目标 71 / 3，三条同时红，且行号直指末尾 `:1252`**：

```
⛔ 有 1 处客户可见写点**没有登记**（AC-003：新增出口必须当场红）：
  - src/server/http.rs:1252 `Json(` —— 既不在 OUTLETS 登记里，也不在非客户白名单里
⛔ 出口函数登记表与源码不一致：
  - src/server/http.rs: `fn qa_b2_raw` **没有登记在 OUTLET_FNS 里** …
⛔ 客户响应体不是管线产物（F4：`Response::builder().body(raw)` 的同类形态）：
  - src/server/http.rs:1252 的响应体是 `serde_json::to_value(&*state.graph)…` —— **不是管线产物**
```

⇒ **上一轮同一形态是 73/0 全绿，现在是 71/3 红。B2 确实修好了。**

### 3.2 独立探针：直接读真的 `prod()`（不是看测试红不红）

我在**同一个变异**里往判据文件加了一条临时探针（跑完随 stash 复原），直接断言 `prod()` 的三个性质：

```
test qa4_probe_prod_views_the_tail_after_cfg_test ... ok
```

它断言并全部通过：
1. `prod("src/server/http.rs").lines().count() == read_source(…).lines().count()` ⇒ **行号 1:1**（抹空不删行）；
2. `prod()` 含 `async fn get_graph(` / `pub async fn serve(` ⇒ 生产段可见；
3. `prod()` **不含** `fn error_messages_themselves_pass_the_guard` ⇒ `#[cfg(test)] mod tests` **真的被跳过**；
4. `prod()` **含** `fn qa_b2_raw(` ⇒ **测试模块之后追加的代码可见**（这正是上一轮被吞掉的那一段）。

### 3.3 反问一半：`prod()` 会不会「偏严」把测试代码当生产代码？

**变异 `B2skip`**（`http.rs 6BEF88DF→2F7D93C0`）：在测试模块内、约第 **696 行**
（`change_verbs_are_405_on_registered_api_paths` 的函数体开头）埋一个**未登记**的
`let _qa_b2_probe = Json(serde_json::json!({ "qa": 1 }));`
⇒ **13 目标 74 / 0 全绿**。

⇒ `prod()` 是**精确跳过区间**，不是「整段吞掉」也不是「把测试当生产」：
区间内（≈700 行）的写点看不见（正确），区间后的写点看得见（§3.1）。

### 3.4 必须更正我上一轮的一句话（如实登记）

`qa-review-3.md §7-B2` 我写的「`http.rs` 第 **602 行之后的 674 行生产代码**全部不可见」**措辞不准**。
本轮核到的真实结构（`757f88c` 与 `867969b` 的 `http.rs` **完全相同**，都是 1246 行）：

| 区段 | 行 | 性质 |
| --- | --- | --- |
| 生产代码 | 1 – 570 | `load_frozen_view` … `serve()` |
| `#[cfg(test)]` 属性行 | 572 | |
| `mod tests { … }` | 573 – 1246 | **674 行，全部是测试模块，一直到文件末尾** |

所以：**「674」不是生产代码，是测试模块的长度**；真正的真空是「**`#[cfg(test)]` 之后（= 文件末尾）追加的任何代码**」。
这个真空是**真实**的（上一轮「末尾追加零闸 handler」实测 73/0 全绿，本轮 71/3 红），但**范围比我上一轮写的窄**，我当时把「文件尾部的 674 行」误说成「674 行生产代码」。
结论不变（B2 曾经是真空、现在修好），但这个数字必须在案，避免 BA 按错误口径复核。

### 3.5 P0-2 的「第 700 行附近埋 `Json(`」——按 BA 的假设做的事，结论是**不该红**

BA 的假设（「第 602 行之后仍有生产代码」）在本文件**不成立**（见 §3.4）：第 700 行在 `mod tests` 里。
我照原话做了（`B2skip`，埋点就在 ≈696 行）：**74/0 全绿**——这是**正确行为**，不是漏。
真正等价的验证是「**测试模块之后**」（§3.1 + §3.2 的探针第 4 条），已实测红。

---

## 4 · P0-3 · 四条登记的质量（判「诚实 / 够用 / 不成立」，不要求封死）

> 原文：`verification-report.md §11.7`（P-1 / P-3）+ `§12.7`（P-2 改写 + 本轮新登记 P-4）。

### 4.1 P-1 · 闭包式本地判据 —— **诚实、够用（1 处措辞建议）**

| 判什么 | 实测 / 核对 |
| --- | --- |
| 危害描述与实测一致？ | ✅ 一致。闭包只决定「写不写 / 写哪份」，字节仍来自管线；`t5` 的 `fn_definitions` **明确把闭包排除在外**（且有一条反空转断言 `assert!(fn_definitions("let f = |x| x;").is_empty())`），所以「不在 `OUTLET_FNS` 覆盖内」是**如实**的 |
| 「什么条件下会真漏」成立？ | ✅ 成立（「单用 = 静默丢/改，与 P-2 组合才泄漏」），但**前提句要收一下**：Dev 写「前提是最终写出的仍是管线产物（**T2 钉着**）」—— `t2` 的 `CUSTOMER_WRITERS` 只有 `emit_domain_text/line`，**只对 CLI 成立**；HTTP/MCP 侧钉最终字节的是 `t9` + `t10`。准确写法：「与 P-2 / **P-4** 组合才泄漏」 |
| 「下一轮谁接手」明确？ | ✅ Dev（第 4 轮→顺延）、方案（出口函数允许语句集白名单）、成本「中等」都写了 |
| 有没有偷换成「已覆盖」？ | ✅ 没有。原文抬头就是「登记 ≠ 已覆盖」 |

**判定：诚实、够用。** 建议（非阻断）：把「T2 钉着」改成「CLI 由 T2 钉着；HTTP/MCP 由 t9/t10 钉着」，并把组合项写成「P-2 / P-4」。

### 4.2 P-2 · handler 裸响应 —— **改写后诚实、够用（1 处措辞不精确）**

Dev 把原来的单通道登记改写成 **`Ok` 通道 / `Err` 通道**两行 + 一行残余（P-4）。逐项实核：

| 通道 | Dev 的说法 | 我的实测 | 判定 |
| --- | --- | --- | --- |
| `Ok`（返回裸 `String`/元组） | 「与 `Ok(Json<T>)` 不同型 ⇒ **编译不过**，要静默只能改签名」 | 结论对（挡得住），但**这句不精确**：`Ok(Json(raw))` / `Html(raw)` 在**同一个 handler 里签名一字不动**也编译得过（变异 `OKch`：`http.rs 6BEF88DF→7B7CDD76`，13 目标 **72 / 2**，`t1` + `t9` 红）。所以「挡住」靠的是 `t1`/`t9`，不是「编译不过」 | ✅ 结论成立，⚠️ 措辞要收 |
| `Err`（4 行 `into_response()`） | 「签名一字不动、4 行、与 M10b 同一后果、绕过 `check_scope`」 | ✅ 逐字复现（变异 `F`，**73/1**，两道命中） | ✅ 准确 |
| 残余 P-4 `err(status, &raw)` | 「仍可表达、不写成已封堵」（详见 §4.4） | ✅ 实测全绿，与登记一致 | ✅ 准确 |

**判定：诚实、够用。** 建议（非阻断）：把 `Ok` 通道的「要先改签名」改成「`Ok(raw_string)` / 元组**要改签名才编得过**；`Ok(Json(raw))` / `Html(raw)` **不改签名也能编**，由 `t1`/`t9` 抓（`OKch` 实测 72/2）」。

### 4.3 P-3 · `NON_OUTLET_FILES` 整文件豁免 —— **诚实、够用**

- 清单实核：`src/mcp/protocol.rs` + `src/bin/{ontok-gate-preflight, ontok-introspect, ontok-propose, ontok-translate-names}.rs`
  —— **5 个，精确路径**，与 §11.7 的「4 个 bin + `mcp/protocol.rs`」**一一对上，没漏、没偷换**；
- 危害（`src/bin/` 是「顺手 println 调试」的高频位置、可能被当运维回执发出去）与我的判断一致；
- 「今天可接受、下轮收到函数级 `(路径, 函数名)`」与 A13 的原话一致，成本「低」✔；接手人 Dev ✔；
- 上一轮 `M8` 实测（往 `ontok-propose` 加客户写出点 ⇒ 73/0 全绿）本轮判据未动该常量（`git diff 867969b..757f88c` 只改 `prod()`/`t5`/`t9`/`t10` 相关），结论沿用。

**判定：诚实、够用。**

### 4.4 P-4 · `err(status, &raw)`（本轮新登记）—— **诚实、够用，且我的实测与登记完全一致**

**我动手试了**（变异 `P4`：`http.rs 6BEF88DF→AAE51EA3`）：

```rust
// get_graph，签名一字不动、4 行、写在 check_scope 之前
if q.get("qa").is_some() {
    let raw = serde_json::to_string(&*state.graph).unwrap_or_default();
    return Err(err(StatusCode::BAD_REQUEST, &raw));
}
```

**实测：13 目标 74 / 0 —— 全绿。**

| 判什么 | 结论 |
| --- | --- |
| 危害描述与实测一致？ | ✅ 「客户数据仍**字节级外泄**（`curl` 直接可读），只是不再是浏览器会当页面渲染的那条路径；同样跳过 `check_scope` / 管线判据」—— 与实测一致（`Content-Type` 仍是 `application/json`，正文是 `{"error":"<raw>"}`） |
| 「什么条件下会真漏」成立？ | ✅ 成立：「把已解析好的 `raw` 塞进 `err(…)` 的文案位」是**善意改动**，比恶意更难防。我这次只写了 4 行、没碰任何禁调拼写就全绿 |
| 「下一轮谁接手」明确？ | ✅ Dev（第 5 轮）/ BA 裁定是否长期接受；方案（`ErrorText` 无公开构造器 + `err(status, msg: ErrorText)`）方向正确 |
| 有没有把「已登记」偷换成「已覆盖」？ | ✅ 没有：标题就写「⚠️ 残余（必须写清，**不许读成「已封堵」**）」，正文「我把它**如实登记**……**不写成已封堵**」 |

**判定：诚实、够用（QA 认可）。**
⚠️ 一处数字不准（非阻断）：登记写「`err()` 的 **12** 个调用点」，实测**生产段共 13 处**
（`http.rs:194/204/208/225/232/238/267/269/293/299/345/349/392`，`fn err` 定义行不计）。
这个数字不影响论证（「多种正当形状 ⇒ 源码层否定式会误伤」依然成立），但应改准。

---

## 5 · P0-4 · 未退化项（快速核，逐项）

| 项 | 实测 | 判定 |
| --- | --- | --- |
| 基线 13 目标 | **74 passed / 0 failed**（10/12/4/1/2/4/3/2/3/10/7/7/9 按目标字典序：`binary 7 · exit 7 · graph_server 9 · property_filter 10 · guard_gran 4 · outlet_pipeline 12 · pipeline_bytes 1 · red_lines 2 · scope_gate 4 · src_enum 10 · static_texts 3 · track 2 · zero_visible 3`） | ✅ 与 Dev §12.8 一致 |
| 13 条客户出口接线 | `OUTLETS` 里 `Enforcement::Pipeline` 共 **17** 条（13 客户 C1–C5/H1–H3/M1–M5 + **D1 + Z1 + Z2 + Z3**）；**17/17 的 `call_site` 都能在 `path` 指的文件里逐字找到**（含 C2 的 `std::fs::write(p, html.as_str())`、Z1 的 `err(StatusCode::BAD_REQUEST, rendered.as_str())`）；`t1`/`t3`/`t8` 基线全绿 | ✅ 未退化（注：上一轮我写「16 条」，是自己算错，17 = 13+4 才对） |
| 五红线 `numstat` | `git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs src/mcp/tools.rs` → **空** | ✅ |
| `guard()` 零改动 | 同上（`src/mcp/tools.rs` 整体零改动，仍是 `crate::mcp::tools::guard` 单一本体） | ✅ |
| 无删测试 | `git diff --name-status develop -- tests`：**A×25 / M×4 / D×0**；`git diff develop..HEAD -- tests` 里被删的 `#[test]` 行 = **0**；`source_enumeration` 测试数 `867969b: 9 → 757f88c: 10`（只增 `t10`） | ✅ |
| 无新 `#[ignore]` | 全仓属性行只有 `tests/test_mcp_server.rs:553`，**`develop` 同位置也有**（1 = 1） | ✅ |
| 断言只增 | `property_filter` **23→30**；`exit_ordering_req106` **11→11**（否定式改写，未减）；`binary_output_guard` **73→79**；被删的多行 `assert!(` 共 2 处，均已用更强的断言替换（无净损失） | ✅ |
| Z1 / Z2 / Z3 | `OutletId::Z1/Z2/Z3` 记号齐（`t3` 绿）；`test_req107_scope_error_gate` **4 passed**；`test_req107_outlet_pipeline` **12 passed** | ✅ |
| golden | `tests/fixtures` 相对 `develop` 是 **A（新增）**，无删改；`SHA256SUMS` **13/13 全部复算 MATCH**（抽验 `H1_index.html E8753C75…`、`C3_json.txt 1DD509C5…` 均 MATCH）；`test_req107_zero_visible_change` **3 passed**、`test_req107_pipeline_bytes` **1 passed** | ✅ |
| `cargo test --lib` | **168 passed / 2 failed**（`db::apply` / `extract::introspect` = `MissingEnv`，**环境不具备**，未计作通过） | ✅ 与 Dev 一致 |
| `cargo test --doc` | **4 passed**（两条 `compile_fail` 仍在） | ✅ |

---

## 6 · P0-5 · 环境

| 项 | 结果 |
| --- | --- |
| **A8 绕行**（唯一允许） | 挪出 `src/bin/ontok-gate-preflight.rs` → 跑 → 挪回。**本轮挪出/挪回 2 轮**，两次 sha256 都是 `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`（**SAME=True**）。⛔ 没删、没改内容、没动 `Cargo.toml`、没加 `#[cfg(unix)]` |
| 哪些测试是这么跑出来的 | **全部**：13 目标（含基线 + 12 次变异）、`--lib`、`--doc` |
| `tests/test_retained_names_req084.rs` | **跑不了**（A10 原文）：`cargo test --test test_retained_names_req084 --no-run` ⇒ `error: couldn't read tests\../src/bin/ontok-gate-preflight.rs: 系统找不到指定的文件 (os error 2)`。挪回后该 bin 本身在 Windows 编不过。**如实报「跑不了」，不声称通过** |
| 需 DB 的用例 | `db::apply::…` / `extract::introspect::…` 2 条 = **环境不具备**（`MissingEnv("ONTOK_TEST_DATABASE_URL")`），未计作通过 |
| 全量口径 | 全量 `cargo test` / `62 suite / 846 passed` **本机不可复核**（既有 bin 的 `std::os::unix`），须在 Linux 复核 |

---

## 7 · 非阻断项 + 建议登记（**不是**阻断，按 BA 的尺度先登记）

### N1 · `Json::from(..)` / `.map(Json)`：`Json(` 之外的同族拼写，实测**全绿**

| 变异 | 形态（3 行、零登记、写在 `check_scope` 之前） | 结果 |
| --- | --- | --- |
| **J** | `let raw = serde_json::to_value(&*state.graph)…; return Ok(Json::from(raw));` | 🟢 **74 / 0** |
| **J2** | `return serde_json::to_value(&*state.graph).map(Json).map_err(\|e\| err(StatusCode::BAD_REQUEST, &e.to_string()));` | 🟢 **74 / 0** |

- **为什么全绿**：`WRITE_CONSTRUCTS` 枚举的是**字面拼写 `Json(` / `Html(`**；`Json::from(`、`.map(Json)` 都不含 `Json(`。
  `t8` 绿（原有 `publish_json(…OutletId::H2…)` 还在）、`t5` 绿（没加新函数）、`t9` 看不见（没有 `Json(` 写点）、`t10` 无关（没定义 `into_value`）。
- **危害**：与 M10b / `Err` 形态同一后果 —— `?qa=1` 直接吐**未过闸**的整份客户图（`state.graph` 与 H2 同源），且**绕过 `check_scope`**；`Content-Type` 仍是 `application/json`。
- **触发条件**：任何一次「给已有 handler 加一个便捷返回」的改动，作者顺手写 `Json::from` / `.map(Json)`（Rust 里都自然，无需恶意）。
- **为什么我判「非阻断」**：A13 明写「**不要求**穷尽」；Dev 的登记里**没有**任何一句声称这两种拼写被挡住（§12.7 的三行都是具体通道）；按 BA 的「新花样 ≠ 新口子」尺度，这属于**新发现、合理、未登记**，不是「声称封住却全绿」。
- **建议登记 + 便宜收法**（给下一轮 Dev，成本低）：出口文件里**每一处 `DOMAIN_PRODUCERS`**（建议补 `to_string(`）都必须落在**某条登记 `call_site` 的窗口内**；
  这条否定式一次同时覆盖 `M6b`（`to_value(`）、`J`/`J2`（`to_value(`）、`P4`（`to_string(`），误红面只有 `publish_json`/`publish_value` 那几个登记写点本身，可按「窗口」判定。

### N2 · P-2 的 `Ok` 通道措辞不精确（实测 `OKch` 72/2）

`Ok(Json(raw))` / `Html(raw)` 在**同一 handler 里签名不动**也编译得过（不只有「改签名」一条路）；挡住它的是 `t1`/`t9`。
**结论（挡得住）成立**，但「要先改签名、diff 显眼」这句应改成「**裸 `String`/元组**要先改签名；`Json(raw)`/`Html(raw)` 不用改签名，由 `t1`/`t9` 抓」。

### N3 · `§11.5:799` 的旧结论句仍是「已封住」口径，且被 N1 反证

原文：「…该绑定上游 20 行内有管线调用。⇒「手搓响应体」不登记也红。」
两处过期：①「上游 20 行」这条规则**已被 B3② 整体替换**（现在是「初始化式本身必须是管线调用」）；②「手搓响应体不登记也红」被 `J`/`J2` 反证（只对 `Json(`/`Html(` 拼写成立）。
**为什么不升级为阻断**：A14 的 FAIL 触发句说的是「登记里说封住了、实测却全绿」，而这条是**第 3 轮遗留**的结论句、不是本轮登记；本轮登记（§12.7）是具体且准确的；且 A13 不要求穷尽。
**建议**：本轮交付前把该句删掉或限定为「按 `Json(`/`Html(` 拼写枚举的那部分不登记也红」。⛔ 若第 6 轮还留着这句，我下一轮会按 A14 的字面判 FAIL。

### N4 · P-4 的数字 + 「能不能顺手封掉」

- 「12 个调用点」实为 **13**（见 §4.4）；
- **便宜的封法**（比 `ErrorText` 类型层便宜，且不用动 13 条接线）：仿 `INTO_RESPONSE_SITES` 的做法，
  把 `err(` 的第二参**逐字钉住**成允许形状（`e.message()` / `&msg` / `rendered.as_str()` / 字面量），双向计数（多一处 ⇒ 红）。
  预计工作量「小」，能同时守住 P-4 与「善意改文案」这一类。

### N5 · P-1 的边界句（同 §4.1）

「前提是最终写出的仍是管线产物（T2 钉着）」→ 改成「CLI 由 t2 钉着；HTTP/MCP 由 t9/t10 钉着；与 P-2/**P-4** 组合才泄漏」。

---

## 8 · 阻断项

**0 条。**

> 逐条说明为什么不阻断（避免被读成「放水」）：
> - `Json::from` / `.map(Json)`（N1）是**真实可执行**的绕过，但 Dev **从未声称**它被挡住/评估过 ⇒ 不满足「登记质量被推翻」；且 A13 明确不要求穷尽 ⇒ 非阻断 + 建议登记。
> - `§11.5:799` 的过期结论句（N3）是第 3 轮遗留、不是本轮登记；本轮登记（§12.7）准确 ⇒ 非阻断 + 必须在下一轮删掉，否则下一轮按 A14 字面判 FAIL。
> - P-2 的 `Ok` 措辞（N2）、P-4 的「12 vs 13」（N4）、P-1 的「T2 钉着」（N5）都是**精度**问题，结论方向经实测成立。

---

## 9 · 我无法确认的项（如实列，不填空）

1. **真进程 e2e / 真数据（demo3 v2）字节不变**：无 DB，未实测；`test_req107_zero_visible_change` 是**进程内**重生比对。
2. **`test_retained_names_req084`**：跑不了（A10，见 §6）。
3. **全量 `62 suite / 846 passed`**：本机不可复核（既有 bin 的 `std::os::unix`）。
4. **`prod()` 的「多个 / 嵌套 `#[cfg(test)]`」**：`757f88c` 的三个出口文件里只有 `http.rs` 有 **1 处** `#[cfg(test)]`（572），
   所以「多个 + 嵌套 + 配平」这条 Dev 自述**只在本文件的 1 处上被实证**；嵌套场景我**没有**造变异（方向是「偏严」，不会漏）。
5. **`Json::from` / `.map(Json)` 的运行期可利用性**只到源码层：我没有起 HTTP 服务打 `/api/raw`（无 DB），报的是「判据全绿 + 代码路径可达 + 编译通过」。
6. **P-1 闭包形态本轮未重跑**：判据侧与闭包相关的部分（`fn_definitions` 排除闭包）本轮未改，我以静态核对 + 第 4 轮实测为准（A13 已允许登记）。
7. **测试二进制的 sha256 未逐次留存**（cargo 每次重编都会变）—— 记的是**被改源文件**的 sha256（前/后）。
8. **浏览器渲染路径**：P-4「不再当页面渲染」是按 `Content-Type: application/json` + `{"error": …}` 结构**推断**，没有真浏览器实测。

---

## 附录 A · 证据索引（全部未跟踪，可复现）

| 内容 | 路径 |
| --- | --- |
| 变异 worktree（detach @ `757f88c`，收工 `git status` 空） | `.review-workspace/req107/qa4` |
| 变异 spec（FILE/OLD/NEW，**唯一匹配**否则拒改） | `.review-workspace/req107/qa4-patches/{M6b,M7,M5d,M10b,A2,D2,F,B2only,B2skip,P4,J,J2,OKch}.txt` |
| 每次变异的 patch | `.review-workspace/req107/qa4-logs/<ID>.patch` |
| 每次变异的完整 cargo 输出 | `.review-workspace/req107/qa4-logs/<ID>.log` |
| 每次变异的 sha256 前/后（JSON） | `.review-workspace/req107/qa4-logs/<ID>.json` |
| 基线 13 目标 / `--lib` / `--doc` / A10 复现 | `qa4-logs/{baseline,lib,doc,retained084}.log` |
| 应用 / 复原 / 跑 / 驱动 | `qa4-logs/{apply.js,restore.js,run.ps1,driver.ps1}` |
| A8 bin 暂存（已清空，sha 已核） | `.review-workspace/req107/qa4-stash` |

## 附录 B · 本轮实测数字对账

| 命令 / 变异 | 实测 |
| --- | --- |
| 13 目标（未变异） | **74 passed / 0 failed** |
| M6b / M7 / M5d / M10b | **72/2 · 73/1 · 73/1 · 73/1** |
| a′ / d2 / Err 形态（`F`） | **72/2 · 71/3 · 73/1** |
| B2only（末尾追加、零登记） | **71/3**（`t1`+`t5`+`t9`，`:1252`） |
| B2skip（测试模块内埋 `Json(`） | **74/0**（正确跳过） |
| P4（`err(StatusCode::BAD_REQUEST, &raw)`） | **74/0**（与登记一致：仍可表达） |
| J / J2（`Json::from` / `.map(Json)`） | **74/0 · 74/0**（新花样，见 N1） |
| OKch（`Ok(Json(raw))`，签名不动） | **72/2**（`t1`+`t9`） |
| `cargo test --lib` | **168 passed / 2 failed**（DB，环境不具备） |
| `cargo test --doc` | **4 passed** |
| golden `SHA256SUMS` | **13/13 MATCH** |
| A8 sha256 前/后 | `C7CF95E2…322D` = 同（2 次） |

> **给我自己的一句提醒**：这一轮 Dev 确实把 A14 的七种形态全打红了，B2 的两半（「末尾可见」+「测试区间不被误报」）我也用独立探针验了，
> 四条登记**没有一条撒谎**（P-4 我亲手试了，全绿，和它写的一模一样）。
> 剩下的 `Json::from` / `.map(Json)` 是**真口子但不是谎**：按 BA 的线，它该进「非阻断 + 建议登记」，不是第 5 次 FAIL 的理由。
