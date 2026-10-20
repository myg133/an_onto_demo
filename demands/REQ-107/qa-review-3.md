# REQ-107 · 第 3 轮修复的**复验**报告（QA 第 4 轮）

> **对象**：`feature/REQ-107` @ `867969b`（基线 `develop b714f48`）。
> **判据**：`acceptance.md §七 A13` —— 三条过关线，不加码。
> **装置**：独立 worktree `.review-workspace/req107/qa3`（detach @ `867969b`，收工时 `git status` 空）。
> **A8 绕行**：`src/bin/ontok-gate-preflight.rs` 临时挪出 → 跑完挪回，sha256 前后 `C7CF95E2…322D` = `C7CF95E2…322D`（SAME=True）。
> ⛔ 没有改 `feature-REQ-107` 的任何被跟踪文件，没有 push。

---

## 1 · 结论

### **FAIL（阻断 2 条）**

一句话：**A13 点名的四手（M6b / M7 / M5d / M10b）全部真红，"老实填表"这一手现在红**；
但 F4 仍有**四行、不改签名、零登记**的活口（`return Err((StatusCode, raw).into_response())`，73/73 全绿），
而 Dev 在 P-2 登记里把这一类形态的代价写成"编译不过、要先改签名、diff 显眼"——
它说的是 `Ok(..)` 通道（那句对），**漏了 `Err` 通道**（`Result<Json<_>, Response>` 的 `Err` 类型本来就是 `Response`）。
按 A13「PASS 条件 2：…且 QA 认可危害评估」,**我不认可**；
另有一条独立于 A13 三条件的 **AC-003 真空**（`prod()` 在第一个 `#[cfg(test)]` 处截断，
`src/server/http.rs` 后 674 行生产代码不被任何判据看见，5 行零登记即可全绿）。

### A13 三条线逐条对照

| A13 PASS 条件 | 我的实测 | 判定 |
| --- | --- | --- |
| 1. F1 / F2 修好，且「老实填表」再试一次仍然红 | `M6b` = **71 passed / 2 failed**（`t8` + `t9` 红）；`M7` = **72 / 1**（`t1` 红） | ✅ **满足** |
| 2. F3 / F4 要么封住、要么按格式登记，**且 QA 认可危害评估** | F3：`M5d` 红 ✅。F4：`M10b` 红 ✅，但 P-2 的**代价/触发条件漏了 `Err` 通道**（`F` 变异：4 行、不改签名、**73/0 全绿**） | ❌ **不满足** |
| 3. 上一轮「未退化」的部分不得退化 | 13 条接线 / 五红线 / `guard()` / 无删测试 / 断言只增 / Z1–Z3 / golden —— **逐项未退化** | ✅ **满足** |

### 这一轮我**认可**的部分（避免被读成全盘否定）

- **A13 点名的两个阻断是真修**，不是换个名字：M6b（老实填表）从"71 全绿"变成 `t8`+`t9` 双红，且失败文案指到了具体那条出口（`H4`）；
  M7 从"71 全绿"变成 `t1` 红，文案直说"白名单 = 合法绕过"。两者都是**运行期判红，不是编译失败**（`log` 里无 `error[E…]`）。
- F3 的封堵是**结构性**的：`write_mcp_line` 把第 2 轮那条"整条 `serve()` 豁免"收成 9 行小函数，`serve()` 重新受约束。
- 未退化项**一条没掉**（§6），Dev 自陈的 73/0、168/2、4/0 与我实测**逐条一致**（除 `property_filter` 那处 sha，见 §6 表内说明）。
- 第 2 轮的 P-1 / P-2 / P-3 这次**都按 A13 的格式登记了**（危害 / 条件 / 接手人），没有一条写成"已覆盖"。问题只在 P-2 的**内容准确性**。

---

## 2 · 装置与基线

| 项 | 值 |
| --- | --- |
| worktree | `.review-workspace/req107/qa3`（detach @ `867969b`） |
| 变异应用 | `qa3-logs/apply.js`（FILE/OLD/NEW，**唯一匹配**否则拒改；pristine 备份 → sha256 前/后；`git diff` 存 patch） |
| 复原 | `qa3-logs/restore.js`（逐字节拷回 + 核 sha == 变异前；**每次变异后都核过**） |
| 13 目标 | `qa3-logs/run.ps1`（`--no-fail-fast`，13 个 `--test`） |
| 单目标 | `qa3-logs/runone.ps1` |
| cargo target | 复用 `qa2/target`（依赖产物；每次仍重编被测 crate） |

**基线（未变异）**：`73 passed / 0 failed`（9 / 12 / 4 / 1 / 2 / 4 / 3 / 2 / 3 / 10 / 7 / 7 / 9）——与 Dev §11.8 一致。
`cargo test --lib`：**168 passed / 2 failed**（`db::apply` / `extract::introspect`，`MissingEnv`，环境不具备）。
`cargo test --doc`：**4 passed**。

> 口径说明：本节所有"红/绿"都是**13 个目标**的结果，除非单独注明"仅 `source_enumeration`"。
> 所有变异**均编译通过**（测试二进制跑起来了），⛔ 没有一次是"编译失败不算挡住"。

---

## 3 · P0-1 · 四个现成变异（复跑，未造新形态）

| # | 攻击 | 被改文件 sha256 前 → 后 | 结果 | 红在哪 |
| --- | --- | --- | --- | --- |
| **M6b** | **老实填表**：`/api/raw` 零闸 + `OUTLETS` 加 `H4`(Pipeline) + `OUTLET_FNS` + 计数 33/13→34/14 + ID 集合 + `outlet_pipeline` 三处 | `http.rs 6BEF88DF→A55496F6`；`pipeline.rs 88C28710→5777AA64`；`test_…_source_enumeration.rs 75AB1461→120AA559`；`test_req107_outlet_pipeline.rs B395D8D0→65DDE32B` | 🟢 **71 / 2** | `t8` + `t9` |
| **M7** | 白名单：`println!` 打客户 JSON + `NON_CUSTOMER_WRITES` 加 needle/计数 | `http.rs 6BEF88DF→A257A467`；`test_…_source_enumeration.rs 75AB1461→A4727419` | 🟢 **72 / 1** | `t1`（`F2`） |
| **M5d** | `std::io::stdout().lock().write_all(raw)` | `cli.rs C5CACC1A→4A7E07B6` | 🟢 **72 / 1** | `t1`（`F3` ×2 命中） |
| **M10b** | 改已有 handler + 条件分支 + `Response::builder().body(Body::from(raw))` | `http.rs 6BEF88DF→90FC9A35` | 🟢 **72 / 1** | `t1`（`F4` ×3 命中） |

**M6b 的实测失败文案**（`qa3-logs/M6b.log`）：

```
---- t8_pipeline_declarations_have_evidence stdout ----
⛔ `Enforcement::Pipeline` 是声明，不是证据（F1：老实填表即合法）：
  - H4（src/server/http.rs）声明了 `Enforcement::Pipeline`，但 src/server/http.rs 里找不到携带
    `OutletId::H4` 的**管线调用** —— 「填表」不是接线（F1）
---- t9_customer_response_bodies_come_from_the_pipeline stdout ----
⛔ 客户响应体不是管线产物（F4：`Response::builder().body(raw)` 的同类形态）：
  - src/server/http.rs:313 的响应体是 `serde_json::to_value(&*state.graph).unwrap_or(...)` —— **不是管线产物**
```

**M7 的实测失败文案**（`qa3-logs/M7.log`）：

```
⛔ 有 1 处白名单把「domain 生产者」登记成了非客户写点（F2：白名单 = 合法绕过）：
  - src/server/http.rs:280 `println!(`（白名单 needle `serde_json::to_string_pretty(&*state.graph)`）
    的窗口里含 domain 生产者 `to_string_pretty(`
```

**M5d / M10b**：分别精确报出 `src/cli.rs:1350 std::io::stdout` + `:1351 write_all(`，`src/server/http.rs:282 Response::builder` + `:285 .body(` + `:285 Body::from(`。

**结论：A13 点名的四手全部真红。** 其中"老实填表"这一手（M6b）现在红 —— 这是本轮最关键的翻绿点，**确实翻过来了**。
（M6b 的 patch 里含 A8 挪出 bin 的删除 hunk，已用 `:(exclude)` 排掉；见 `qa3-logs/M6b.patch`。）

---

## 4 · P0-2 · 打 Dev 新增的 `t8` / `t9`（本轮核心）

`t8` = 每条 `Enforcement::Pipeline` 必须有一条携带 `OutletId::<该 id>` 的**管线调用**，且与登记写点**同函数**（或写点是该调用的适配器）。
`t9` = 每处 `Json(` / `Html(` 的实参必须是 `<绑定>.into_value()` / `.into_text()`，该绑定出现在同一行的 `Ok(<绑定>)` 里，且**上游 20 行内有管线调用**。

> 说明：为了"诚实填表"，攻击 a–d 都**照规矩**把 `H4` 加进 `OutletId` 枚举 / `spec_of` / `OUTLETS` / 计数 / `OUTLET_ID_TOKENS` / `OUTLET_FNS`。
> 这不是偷偷摸摸，就是"照失败文案行事"。生成器：`qa3-logs/gen-attacks.js`。

### 4.1 汇总

| # | 攻击 | 结果 | 谁挡住了 / 谁没挡住 |
| --- | --- | --- | --- |
| **a** | 伪造一次真调用（执行了、结果丢弃）+ `Json(raw)` 手搓 | 🟢 **红**（仅 `source_enumeration`：8 / 1） | **`t8` 被"有一次调用"满足（绿）**；`t9` 卡的是**实参形状**，红 |
| **a′** | 同上 + 本地 newtype 伪造 `.into_value()` + `if false` 里的**死调用** | 🔴 **含 13 目标全绿（73 / 0）** | `t8` 与 `t9` **同时被满足** ⇒ 两件证据都能伪造 |
| **b** | 绑真产物 → `let mut v = out.into_value(); v["x"]=raw;` → `Json(v)` | 🟢 **红**（8 / 1） | `t9` 看见（实参是裸 `v`，不是 `<绑定>.into_value()`）——但见 a′：形状可伪造 |
| **c** | 管线调用挪到 `qa_warm_up()`，写点函数里只剩手搓 `Json` | 🟢 **红**（7 / 2） | `t8` 的"同函数"**真的生效**（名字不同时），另加 `t9` |
| **d** | 内联 `mod qa_shadow` 里放**同名**的 `get_graph` | 🔴 **含 13 目标全绿（73 / 0）** | `OUTLET_FNS` 是**名字集合**、`t8` 的"同函数"按**名字**比 ⇒ 同名即"同函数" |

### 4.2 (a) 伪造一次真调用 —— `t8` 被满足，`t9` 兜住

```rust
async fn get_raw(State(state): State<AppState>) -> Result<Json<serde_json::Value>, Response> {
    let _forged = crate::domain::pipeline::publish_value(
        crate::domain::pipeline::OutletId::H4,
        serde_json::json!({}),
    );                                   // ← 真调用，结果丢弃
    Ok(Json(serde_json::to_value(&*state.graph).unwrap_or(serde_json::Value::Null)))
}
```

- `t8`：`H4` 有调用、调用与写点同在 `get_raw`、反向也没有多余记号 ⇒ **绿**。**"有一次调用"就够，`t8` 不看结果去哪了。**
- `t9`：`⛔ src/server/http.rs:280 的响应体是 serde_json::to_value(...).unwrap_or(...) —— 不是管线产物` ⇒ **红**。
- ⇒ 单次调用 + 手搓 `Json(raw)` **过不了**。这一格 Dev 的判断正确。

### 4.3 (a′) 把 `t9` 的形状也伪造出来 —— **全绿**

```rust
struct QaRaw(serde_json::Value);
impl QaRaw { fn into_value(self) -> serde_json::Value { self.0 } }

async fn get_raw(State(state): State<AppState>) -> Result<Json<serde_json::Value>, Response> {
    if false {                                   // ← 死代码里的"真管线调用"
        let _ = crate::domain::pipeline::publish_value(
            crate::domain::pipeline::OutletId::H4, serde_json::json!({}));
    }
    match Ok::<QaRaw, Response>(QaRaw(
        serde_json::to_value(&*state.graph).unwrap_or(serde_json::Value::Null))) {
        Ok(qa_rendered) => Ok(Json(qa_rendered.into_value())),   // ← 实参"长得像"产物
        Err(e) => Err(e),
    }
}
```

**实测：13 目标 73 / 0；`test_req107_source_enumeration` 9 / 0。**

- `t9` 只做**字符串形状匹配**：`arg.strip_suffix(".into_value()")`。任何本地类型只要有一个叫 `into_value` 的方法，就能满足它。
- `t9` 的"上游 20 行内有管线调用"是**纯文本包含**（而且用的是 `display` 视图，**字符串字面量里的 `publish_json(` 也算**）—— 一个 `if false` 里的死调用即可。
- **危害**：新增 `GET /api/raw` 直接返回**未过闸的整份客户图**（`state.graph` 与 H2 同源），`check_scope` 也不走；
  73 条判据 + 12 条管线判据 + 2 条红线 **全无感**。作者只需"老实登记 + 加一个 8 行的 newtype"。
- **触发条件**：任何一次"新增客户出口"的改动，只要作者愿意写一个 `.into_value()` 适配层（这在 Rust 里很自然，不需要恶意）。
- ⇒ **`t8`/`t9` 是形状判据，不是类型判据；它们挡住了"照失败文案填表"，挡不住"照着判据写形状"。**

### 4.4 (b) 绑真产物后改写内容 —— `t9` 看得见

```rust
let mut v = rendered.into_value();
v["qa_backdoor"] = serde_json::json!({ "smuggled": true });
Ok(Json(v))
```

**实测：8 / 1（仅 `source_enumeration`），`⛔ src/server/http.rs:284 的响应体是 \`v\` —— 不是管线产物`。**

- `t9` 看得见这一格，**但看得见的是"实参没带 `.into_value()` 后缀"，不是"值被改过"**。
- 一旦换成 a′ 的写法（`QaRaw` 里先改再 `into_value`），`t9` **完全看不见**。⇒ 这一格的绿是"拼写绿"，不是"语义绿"。

### 4.5 (c) 把管线调用挪到另一个函数 —— `t8` 的"同函数"**生效**

```
⛔ H4（src/server/http.rs）的写点在第 284 行的 `get_raw` 里，但携带 `OutletId::H4` 的管线调用在
   第 276 行的 `qa_warm_up` 里 —— 声明与证据不在同一条路径上（F1）
⛔ 客户响应体不是管线产物 …（t9）
```

**实测：7 / 2。** `t8` 的"同一个函数"约束在**函数名不同**时是真的生效的 —— 这是 Dev 第 3 轮新加约束里**最硬的一条**。
但它的实现是 `enclosing_fn(lines, idx)` → **只返回函数名**（`fn_name_of`），不看 impl/mod 路径 —— 见 (d)。

### 4.6 (d) 同名的第二个函数 —— **全绿**（登记粒度是"名字集合"）

```rust
mod qa_shadow {
    use super::*;
    pub async fn get_graph(State(state): State<AppState>) -> Result<Json<serde_json::Value>, Response> {
        if false { let _ = crate::domain::pipeline::publish_value(OutletId::H4, json!({})); }
        match Ok::<QaRaw, Response>(QaRaw(serde_json::to_value(&*state.graph).unwrap_or(...))) {
            Ok(qa_rendered) => Ok(Json(qa_rendered.into_value())),
            Err(e) => Err(e),
        }
    }
}
// build_router: .route("/api/raw", get(qa_shadow::get_graph))
```

**实测：13 目标 73 / 0。**

三件事同时成立：

1. `OUTLET_FNS` 的粒度是 **`(文件, 函数名)` 的集合**，不是"文件里的第几个函数/在哪个 impl/mod"。`get_graph` 已经在 `src/server/http.rs` 名下登记过 ⇒ 第二个同名函数**不需要任何新登记**（`t5` 用 `BTreeSet<&str>` 比对）。
2. `t8` 的"同函数"按**名字**比较（`wp_fn == call.enclosing`）⇒ `qa_shadow::get_graph` 与"登记的 `get_graph`"在判据眼里是同一个 ⇒ 同函数**成立**。
3. `t6` 的 `module_decls()` 只认 `mod x;`，**不认内联 `mod x { }`** ⇒ 出口文件里长出一个新模块也不红。

**危害**：新增一条客户出口可以做到**零登记**（除了那个伪造 `into_value` 的 newtype），而 Dev §11.2 声称的"声明与证据不在同一条路径上 ⇒ 红"在**同名**时失效。
**触发条件**：新增函数与已登记函数重名（内联 `mod` / `impl` 块里），并复用 a′ 的 `into_value` 形状。

### 4.7 P0-2 小结（直接回答四个问题）

| 问题 | 答案 |
| --- | --- |
| `t8` 会不会被"有一次调用"满足？ | **会**（a 的 `t8` 全绿）。 |
| `t9` 会不会被"上游 20 行内有调用"满足？ | 不只是"有调用"就够 —— 它先卡实参形状；**但形状可以伪造**（a′ 全绿，且死调用/字符串字面量都能满足 20 行窗口）。 |
| `t8` 的"同一函数"真的生效吗？ | **名字不同时生效**（c 红）；**同名即失效**（d 全绿）。 |
| 登记是按 `(文件, 函数)` 粒度吗？ | **是 `(文件, 函数名)` 的集合**，不是路径/序号级别；同名第二个函数免登记（d 全绿）。 |

---

## 5 · P0-3 · 三条登记的质量（不是要求它封死，是判它诚实不诚实）

> 原文：`verification-report.md §11.7`。三条都写全了"危害 / 什么条件下真漏 / 下一轮谁接手"。

### 5.1 P-1 闭包式本地判据 —— **基本诚实够用（需补一句）**

- Dev 说："能在调管线之前决定写不写/写什么，只作用在重拼串上 ⇒ T2 永远看不到；**单用=静默丢/改（功能性），不直接泄漏 —— 前提是最终写出的仍是管线产物（T2 钉着）；与 P-2 组合才是泄漏**"。
- **我认可"不直接泄漏"这个结论**（与我第 3 轮 M9 的实测一致：闭包只决定"写不写/写哪份"，字节仍来自管线）。
- **但"T2 钉着"这个前提只在 CLI 成立**：`t2` 的 `CUSTOMER_WRITERS` 只有 `emit_domain_text/line`，HTTP/MCP 的"最终字节"是靠 `t9` 与类型层钉的，而 `t9` 已被 a′ 证明可伪造。
  ⇒ 准确写法应是"**与 P-2 或任何能把裸字节塞进响应体的写点组合**才泄漏"。
- 接手人：Dev（第 4 轮）✔ 明确；成本"中等"✔ 与我上轮估的一致。
- **判定：诚实、够用；建议把边界句补上（不阻断）。**

### 5.2 P-2 handler 返回裸 `String` / 元组 —— **不够用**（= 阻断 B1）

- Dev 原文（§11.7 P-2 的"什么条件下会真漏"格）：
  > 改**已有** handler 的**返回类型**（条件分支 `return Ok(raw_string)` 与 `Ok(Json<T>)` 不同型 ⇒ **编译不过**，所以静默后门要先改签名，diff 显眼）；或**新增**一个返回 `String` 的路由（T5 要求登记 fn、T1 无写点）
- 前半句**是对的**：`Ok(raw_string)` 与 `Ok(Json<T>)` 确实不同型、确实编译不过。
- **但它漏了 `Err` 通道**。`get_graph` 的签名是 `Result<Json<serde_json::Value>, Response>`，`Err` 的类型**就是** `Response`，于是只要 4 行、签名一字不动（`F` 变异，13 目标 **73 / 0**）：

  ```rust
  // get_graph：签名未变
  if q.get("qa").is_some() {
      let raw = serde_json::to_string(&*state.graph).unwrap_or_default();
      return Err((StatusCode::OK, raw).into_response());   // ← Err 的类型本来就是 Response
  }
  ```

  `(StatusCode, String)` 实现 `IntoResponse`；这条路径不出现 `Response::builder` / `.body(` / `Body::from(` / `Json(` / `Html(` ⇒ F3/F4 的禁调集合与 `t9` 都看不见。
- **危害**：与 `M10b` 完全同一后果（`?qa=1` 直接吐未过闸的整份客户图，且绕过 `check_scope`），代价从"8 行 + 三处禁调拼写"降到 **4 行 + 一个 `into_response()`**。
- **触发条件（真实）**：改已有 handler 时加一条 `if`，`return Err((StatusCode::OK, raw).into_response())`；**签名不用动**，diff 里只有 4 行新增。
- 接手人：Dev（第 4 轮）✔ 明确；方案（对 3 个数据路由加返回类型判据）✔ 方向对。
- **判定：格式齐、方向对，但"要先改签名、diff 显眼"这句在 `Err` 通道上不成立 —— 这是 A13 要求"QA 认可"的那句，我不认可。**
  ⛔ 修法很便宜：把这个格子补成真实形态（`Err((StatusCode, String).into_response())`），并把它算进 F4 的残余。

### 5.3 P-3 `NON_OUTLET_FILES` 整文件豁免 —— **诚实够用**

- 清单是 `src/mcp/protocol.rs` + 4 个 `src/bin/*`（§11.7 写的是"4 个运维 bin + `mcp/protocol.rs`"，**没漏、没偷换**）；
  我第 3 轮的 M8（往 `ontok-propose` 里加客户写出点）确实是 73/0 全绿，与 Dev 描述一致。
- "今天可接受、下轮收到函数级（`(路径, 函数名)`）"这个处置与 A13 §"允许登记"的原话一致；危害（bin 是"顺手 println 调试"的高频位置、可能被当运维回执发出去）与我的判断一致；成本"低"✔。
- **判定：诚实、够用。**

### 5.4 有没有把"已登记"偷换成"已覆盖"？

| 位置 | 措辞 | 实测 |
| --- | --- | --- |
| §11.1 汇总表 | F1–F4 一律"**封住（运行期判红）**" | F4 只封住了 A13 点名的那一种**拼写**（`M10b` 红），四行的 `into_response()` 形态仍是活的（`F` 全绿） |
| §11.5 结尾 | "⇒「手搓响应体」不登记也红" | **不成立**（a′ / `F` 都全绿） |
| §11.2 结尾 | "声明与证据不在同一条路径上 ⇒ 红" | **同名时不成立**（d 全绿） |

⇒ 这三处的口径应改成"封住**这一种拼写**"。§11.7 的抬头自己写着"登记 ≠ 已覆盖"，**态度是对的**，但 §11.1/§11.2/§11.5 的**结论句夸大了**，而结论句才是被读的那句。

---

## 6 · P0-4 · 未退化复验（快速，逐项）

| 项 | 实测 | 判定 |
| --- | --- | --- |
| 13 条客户出口接线 | `OUTLETS` 里 `Enforcement::Pipeline` 共 16 条（13 客户 = C1–C5 / H1–H3 / M1–M5，另 D1+Z1+Z2+Z3）；每条 `call_site` 在对应文件里都能找到（含 C2 的 `std::fs::write(p, html.as_str())`）；`t1`/`t3`/`t8` 全绿 | ✅ 未退化 |
| 五红线 `numstat` | `git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs src/mcp/tools.rs` → **空** | ✅ |
| `guard()` 零改动 | 同上（`src/mcp/tools.rs` 整体零改动） | ✅ |
| `property_filter` 前 145 行 sha256 | **develop ≠ HEAD**（`D620DCD3…` vs `A1B5101D…`）：差异是第 21–26 行插入的 **6 行模块文档**（第 2 轮改写的说明），**没有删/改任何断言**；且 `4bbd8ac → 867969b` 该文件 **零改动**。⚠️ Dev §11.8 写的 `769763BB…=769763BB…` **我复现不出来**（见 §8-3） | ✅ 未退化；⚠️ 报告数字不可复现 |
| 无新增 `#[ignore]` | 全仓 `#[ignore]` 字样 10 处，**真正的属性行只有 1 处**（`test_mcp_server.rs:553`，`develop` 同位置也有）；新增/改写的 9 个测试文件里 **0 处属性行**（其余 9 处都是注释/字符串里提到它） | ✅ |
| 无删测试 | `git diff --name-status develop -- tests` 只有 `A`/`M`，**无 `D`** | ✅ |
| 断言只增 | `property_filter` 23→30、`exit_ordering_req106` 11→11、`binary_output_guard` 73→79 | ✅ |
| Z1 / Z2 / Z3 | `OutletId::Z1/Z2/Z3` 记号都在（`t3` 绿）；`test_req107_scope_error_gate` 4 passed；`outlet_pipeline` 12 passed | ✅ |
| golden | `4bbd8ac → 867969b` 的 `tests/fixtures` **零改动**；抽 `H1_index.html` / `C3_json.txt` 与 `SHA256SUMS` 互校 **2/2 MATCH**；`test_req107_zero_visible_change` **3 passed**（重生字节比对）、`test_req107_pipeline_bytes` **1 passed** | ✅ |

> `property_filter` 的"G1–G4"：该文件里**没有** `G1–G4` 这个标签（`G-1`/`G-4` 族在 `test_binary_output_guard`，上一轮 A9 #3 的对象）。
> 我按上一轮的口径（前 145 行）做了字节比对，另外单独核了 `test_binary_output_guard` 的 `+297/-38` 与断言 73→79。

---

## 7 · 阻断项（每条一句话）

| # | 阻断 | 一句话（违反哪条） |
| --- | --- | --- |
| **B1** | **F4 / P-2 的危害评估漏了 `Err` 通道，且实测 4 行即可** | `F` 变异证明"改已有 handler + 条件分支 + 手搓响应体"**不需要改签名**：`return Err((StatusCode::OK, raw).into_response())`，4 行、73/0 全绿，直接吐未过闸的客户图 —— **违反 A13「PASS 条件 2」的「且 QA 认可危害评估」**，同时是 AC-003（漏接即红）的一个活口。 |
| **B2** | **AC-003 的"真空"：`prod()` 在第一个 `#[cfg(test)]` 处截断** | `src/server/http.rs` 第 602 行之后的 **674 行生产代码**对 `t1/t2/t3/t5/t8/t9` 全部不可见：把 handler 追加到文件末尾 + 一行 `.route(...)`（**零登记**），13 目标 **73/0 全绿** —— **违反 AC-003（本需求核心：漏接即红）**，也正是 A13「⛔ 不许出现『判据看着绿、但其实没人管』的真空」点名的形态。（注：`prod()` 自第 2 轮就是这样，不是本轮引入的。） |

> **给 BA 的一个明确选择权**：B2 不在 A13 的三条 PASS 条件里（第 3 条列举的未退化项不含它），它是**独立的 AC-003 缺陷**。
> 若 BA 认为"AC-003 只要挡住 A13 点名的形态即可"，可把 B2 降为登记项；**B1 无法这样降**，因为 A13 把"QA 认可危害评估"写进了 PASS 条件。
> 两者的修法都很便宜：B1 只需要改 §11.7 P-2 那一格 + 把它算进 F4 残余；B2 只需要把 `prod()` 的截断从"第一个 `#[cfg(test)]`"改成"跳过 `#[cfg(test)] mod … { … }` 区间"。

---

## 8 · 我无法确认的项（如实列，不填空）

1. **F3 的别名形态未实测**：`use std::io::{self, Write}; writeln!(io::stdout(), "{}", raw)` —— 既不含 `std::io::stdout` 也不含 `write_all(`，且 `writeln!(` 不在 `WRITE_CONSTRUCTS` 里。我没有造这个变异（时间），**不声称它绿也不声称它红**。A13 对 F3 的要求只点名了 `std::io::stdout` / `write_all(`，故不计入本轮判定。
2. **需 DB 的用例**：`--lib` 的 2 条（`db::apply` / `extract::introspect`，`MissingEnv`）**环境不具备**，未计作通过；`test_mcp_server` / `test_domain_print_sites_binary_req078` 本轮**未重跑**（与上一轮同一限制）。
3. **`property_filter` 前 145 行 sha256 对不上 Dev 的数字**：我测到 develop 与 HEAD **不同**（差 6 行文档），Dev 报的是"相同"。我核了 `git diff` 确认**没有断言被改**，所以判"未退化"，但**那个 sha 数字本身不可复现**（纪律 E 的口径问题，非阻断）。
4. **`tests/test_retained_names_req084.rs`：跑不了**（A10：`include_str!` 引用 unix-only bin；挪出时编不过、挪回时 bin 本身在本机编不过）。
5. **全量 `62 suite / 846 passed`**：本机不可复核（既有 bin 的 `std::os::unix`）。
6. **真进程 e2e / 真数据（demo3 v2）上的字节不变**：无 DB，未实测。
7. **变异三元组的完整性**：被改文件 sha256 前/后、patch、逐目标红/绿、失败用例名都有；**测试二进制 sha256 未逐次留存**（cargo 每次重编都会变）——与上一轮同一限制。
8. **a′ / d / E 的"可利用性"只到源码层**：我没有起 HTTP 服务打 `/api/raw`（无 DB），报的是"判据全绿 + 代码路径可达"。

---

## 附录 A · 证据索引（全部未跟踪，可复现）

| 内容 | 路径 |
| --- | --- |
| 变异 worktree（detach @ `867969b`，收工 `git status` 空） | `.review-workspace/req107/qa3` |
| 变异 spec（FILE/OLD/NEW） | `.review-workspace/req107/qa3-patches/{M6b,M7,M5d,M10b,A,A2,B,C,D,E,F,M7v}.txt` |
| 每次变异的 patch（已排除 A8 删除 hunk） | `.review-workspace/req107/qa3-logs/<ID>.patch` |
| 每次变异的完整 cargo 输出 | `.review-workspace/req107/qa3-logs/<ID>.log` |
| 每次变异的 sha256 前/后记录（JSON） | `.review-workspace/req107/qa3-logs/<ID>.json` |
| 基线 13 目标 | `.review-workspace/req107/qa3-logs/baseline.log` |
| 攻击 spec 生成器 / 应用 / 复原 / 运行 | `qa3-logs/{gen-attacks.js,apply.js,restore.js,run.ps1,runone.ps1}` |
| A8 bin 暂存目录（已清空） | `.review-workspace/req107/qa3-stash` |

## 附录 B · 本轮实测数字（供对账）

| 命令 | 实测 |
| --- | --- |
| 13 目标（未变异） | **73 passed / 0 failed**（9/12/4/1/2/4/3/2/3/10/7/7/9） |
| `cargo test --lib` | **168 passed / 2 failed**（DB） |
| `cargo test --doc` | **4 passed** |
| M6b / M7 / M5d / M10b | **71/2 · 72/1 · 72/1 · 72/1** |
| A / B / C（仅 `source_enumeration`） | **8/1 · 8/1 · 7/2** |
| A2 / D / E / F / M7v | **73/0 · 73/0 · 73/0 · 73/0 · 73/0** |
| A8 sha256 前/后 | `C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D` = 同 |

## 附录 C · P0-1 四个变异的关键 hunk（完整 patch 见 `qa3-logs/<ID>.patch`）

**M6b**（`http.rs` + `pipeline.rs` + 两个测试文件）

```rust
// src/server/http.rs —— 新路由 + 零闸 handler
.route("/api/raw", get(get_raw))
async fn get_raw(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&*state.graph).unwrap_or(serde_json::Value::Null))
}
// src/domain/pipeline.rs —— 老实填表（Pipeline + 计数 34/14）
Outlet { id: "H4", path: "src/server/http.rs",
         call_site: "serde_json::to_value(&*state.graph)",
         spec: spec_of(OutletId::H2), enforcement: Enforcement::Pipeline,
         exemption_reason: CUSTOMER },
// tests/…source_enumeration.rs: OUTLET_FNS += ("src/server/http.rs", "get_raw", …)
// tests/…outlet_pipeline.rs:   33→34 / 13→14 / id 集合 += H4
```

**M7**（两处，各一行）

```rust
// src/server/http.rs（get_graph 的 check_scope 之后）
println!("{}", serde_json::to_string_pretty(&*state.graph).unwrap_or_default());
// tests/…source_enumeration.rs（NON_CUSTOMER_WRITES 追加一条）
("src/server/http.rs", "serde_json::to_string_pretty(&*state.graph)", 1, "QA-M7：…"),
```

**M5d**（`cli.rs`，插在 C3 的 `publish_customer` 之前）

```rust
let raw = serde_json::to_string_pretty(&cv)?;
{ use std::io::Write; let mut o = std::io::stdout().lock(); o.write_all(raw.as_bytes())?; }
```

**M10b**（`http.rs`，get_graph 开头）

```rust
if q.get("qa").is_some() {
    let raw = serde_json::to_string_pretty(&*state.graph).unwrap_or_default();
    if let Ok(r) = Response::builder().status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(raw)) { return Err(r); }
}
```

**A2 / D / E / F 的完整 hunk** 就是 §4.3 / §4.6 / §7-B2 / §5.2 里贴的那几段（外加「老实填表」的登记块，与 M6b 同构，只是 `H4` 的 `call_site` 换成各自写点行的唯一子串）。

---

> **给我自己的一句提醒**：这一轮 Dev 确实把 A13 点名的四手全打红了，"老实填表"这一手也真的红了 —— 这是真修。
> 但 `t8`/`t9` 是**形状**判据：`t9` 可以被任何一个叫 `into_value` 的本地方法满足，`t8` 的"同函数"按名字比、`OUTLET_FNS` 也是名字集合。
> 再加上 `prod()` 只扫到第一个 `#[cfg(test)]`，**"新增零闸客户出口"这件事在三种写法下仍然全绿**（a′ / d / E）。
> 我不要求这一轮把所有形态封死（A13 也说了不要求），但 **B1（P-2 的危害评估与实测相反）和 B2（半文件不被枚举）不能靠"已登记"混过去**。
