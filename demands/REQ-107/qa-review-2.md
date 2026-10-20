# REQ-107 · 第 2 轮修复的**复验**报告（QA 第 3 轮）

> **审的对象**：`feature-REQ-107` @ `4bbd8ac`（工作区干净，`git status --short` 空，HEAD = `4bbd8ac3af35dababbb596bfd1124cbd7f8c4113`）
> **上一轮**：`BA/demands/REQ-107/qa-review.md`（FAIL，3 阻断，12 次变异）
> **本轮装置**：**独立 worktree** `.review-workspace/req107/qa2`（`git worktree add --detach … 4bbd8ac`）。⛔ `feature-REQ-107` 的被跟踪文件**一次都没有改**（末尾 `git status --short` 空为证）。
> **本机**：Windows + **无 PostgreSQL** + rustc 1.96.0
> **审核方式**：自己读源码（`test_req107_source_enumeration.rs` **1215 行全文**、`pipeline.rs` 全文、`cli.rs`/`http.rs` 相关段）+ **自己当攻击者做了 18 次变异**（不采信 Dev 的变异表）
> **证据**：`.review-workspace/req107/qa2-logs/{<ID>.log,<ID>.patch}`、`qa2-patches/<ID>.txt`（变异源码）

---

## 1 · 结论

### **FAIL（阻断 2 条）**

一句话：**上一轮点名的三种绕过（新增未登记路由 / 改轨道 / 先写后判的字面形态）**真的**被堵住了**，类型层与枚举层都动了真格；但**这套判据的本质是「自证清单」**——只要**如实填表**（登记 `OUTLETS` 条目 / 往 `NON_CUSTOMER_WRITES` 加一行 needle），一个**一次都没调管线**的客户出口仍然 **71/71 + lib 全绿**。⇒ AC-003「漏接即红」只对「**没填表**」成立，对「**填了表但没接线**」不成立。

| AC | 本轮我的判定 | 依据 |
| --- | --- | --- |
| AC-001 唯一客户类出口管线 | **成立（类型层真的封死了）** | `OutletSpec` 四字段私有 ⇒ 字面量构造 `E0451`；`OutletSpec::C3` 已不存在 ⇒ `E0599`；出口只能传 `OutletId`（查表）。换 ID 关闸（C3→D1）⇒ 红（T3 + `pairing_ok`）。见 §3.4 |
| AC-002 13 条出口全部接线 | 成立 | 13 条逐条 `path:line`（§6.1），出口层零判据直调 |
| AC-003 漏接即红 | **不成立（阻断）** | 未登记的形态全部红（这是真进步）；但**填表即可绿**：M6b（登记一个零闸 `H4`）**71 passed + lib 1 passed**；M7（新增零闸 `println!` 写点 + 白名单一行）**71 passed**。见 §4.1 / §4.2 |
| AC-004 三个卡点 | 成立（卡点② 的「执行」仍是声明性的，非阻断，与上轮同） | `Granularity` 全仓仍零运行时读取 |
| AC-005 零可见变化 | 成立（限我复核到的范围） | 基线 worktree 独立复算 **6/6 MATCH**；`SHA256SUMS` **13/13** 互校；golden 本轮零改动 |
| AC-006 Z1 | 成立 | `test_req107_scope_error_gate` 4 passed；`http.rs:256-269` 仍走 `render(Z1, ErrorText)` |
| AC-007 建模豁免显式 | 成立 | 8 条 `Track::Modeling` + 逐条 `exemption_reason`；`test_req107_track_manifest` 2 passed |
| AC-008 Z2/Z3 | 成立 | `test_req107_static_texts` 3 passed；`serve.rs:252/284` 仍走 `publish_value` 运行时闸 |
| AC-009 红线零触碰 | 成立 | 五文件 numstat 空；G1–G4 前 145 行 sha256 **逐字节相同**；无新增 `#[ignore]`；无删测试；断言只增不减 |
| AC-010 交付证据 | 成立（§10.9 #1 的措辞要改，非阻断） | 本机数字与 Dev 报告逐条一致（7/12/4/1/2/4/3/2/3/10/7/7/9、lib 168+2、doc 4） |

---

## 2 · 装置、基线、与 A8 绕行

### 2.1 独立 worktree 与 A8

A8 的「挪出 unix-only bin」**在 `qa2` 里做**（不是在被测 worktree 里做，所以 `feature-REQ-107` 全程未被触碰）：

```
BEFORE_SHA256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
AFTER_SHA256 =C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
SAME=True            （挪出 → 全部跑完 → 挪回；与 feature-REQ-107 里的原件同 hash）
```

`qa2` 最终状态：`git status --short` **空**、`HEAD=4bbd8ac…`、bin sha256 与挪出前一致。

### 2.2 13 个判据目标 + lib/doc 的**基线**（未变异）

| 目标 | 结果 |
| --- | --- |
| `test_req107_source_enumeration`（**本轮新增的核心判据 T1–T7**） | **7 passed** |
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
| 合计 | **71 passed / 0 failed** |
| `cargo test --lib` | **168 passed / 2 failed**（`db::apply` / `extract::introspect`，均为 `MissingEnv("ONTOK_TEST_DATABASE_URL")`） |
| `cargo test --doc` | **4 passed**（两条 `compile_fail` 仍 ok） |

⇒ 与 Dev §10.8 的每一个数字**逐条一致**（包括 lib 168/2）。**没有编数。**

### 2.3 变异口径

每次变异：`qa2-patches/<ID>.txt`（FILE + OLD + NEW 三段）→ `apply.ps1` 做**唯一一次精确替换**（替换前记 `MATCHES`，≥1 才写）→ 记**被改文件 sha256 前/后** → `git diff` 存 `qa2-logs/<ID>.patch` → 跑全部 13 个目标（`--no-fail-fast`，一次性编译）→ 逐目标 exit + 失败用例名 → `git checkout` 复原。

> **注意**：每份 `*.patch` 里都含一段 `src/bin/ontok-gate-preflight.rs` 的删除 hunk —— 那是 **A8 挪出**，不是变异内容（见 §2.1）。

---

## 3 · P0-1 · 上一轮那 5 种绕过的复验

> ⛔ 独立重做，不采信 Dev §10.6 的表。判定分三档：**编译期挡住** / **运行期判红** / **全绿**。

### 3.1 汇总

| # | 形态 | 结果 | 判红的判据（我实测的失败用例名） |
| --- | --- | --- | --- |
| 1 | 自造 `fn local_ok` + **真改写出** + 保留管线调用 | 🟢 **红 ×2** | `t5_outlet_files_declare_every_function`、`t2_customer_writes_take_pipeline_products_only` |
| 2 | 公开构造器改名 `pub fn craft(String)` | 🟢 **红 ×1** | `t4_only_one_public_factory_per_guarded_type` |
| 3 | 新增 `/api/raw`（零闸、不入表） | 🟢 **红 ×2** | `t1_every_customer_write_point_is_registered`、`t5_…` |
| 4a | `spec.track = Track::Internal`（字面形态） | 🟢 **编译期挡住** | `error[E0599]` / 字面量构造 `error[E0451]` |
| 4b | 等价可编译形态：出口身份 `C3` → `D1`（换 ID 关闸） | 🟢 **红 ×2** | `t3_outlet_id_tokens_are_registered`、`all_three_outlets_call_the_one_shared_function` |
| 5a | `emit_domain_line(&raw)` 先写未过闸字节（字面形态） | 🟢 **编译期挡住** | `error[E0308]` |
| 5b | 同形态可编译：`println!("{raw}")` | 🟢 **红 ×1** | `t1_every_customer_write_point_is_registered` |
| 5c | 同形态可编译：`write_stdout(&raw, true)`（同模块私有 fn） | 🟢 **红 ×1** | `t1_…` |
| 5d | 同形态可编译：**绕开一切已登记构造**，直接 `std::io::stdout().write_all(raw)` | 🔴 **全绿**（71 passed） | —— |

**⇒ 旧 5 种绕过：4 种被真正堵住（其中 2 种是编译期），1 种（第 5 的「不择手段写 stdout」变体）仍全绿。**
与上一轮对比：上一轮 **挡得住 3 / 挡不住 5**；本轮 **挡得住 7 / 挡不住 1（需绕开全部写出构造）**。

### 3.2 绕过 #1 —— 自造判据（M1）

```rust
// src/cli.rs（C3 分支，管线调用原样保留）
fn local_ok(s: &str) -> bool { !s.contains("key") }
if !local_ok(json.as_str()) { eprintln!("QA-M1: 本地判据命中，本次不写 stdout"); return Ok(()); }
crate::cli::emit_domain_line(json);
```

- `src/cli.rs` sha256：`C5CACC1ACAAFB6C80DCF3A872774514B6CFEE47A9972E45C370ECC82EFD3BAFC` → `6F843593C4DABDBAD5A0505DB6BF20246BC233693F8CE3286C383C375E5B18D6`
- 实测报错原文（T2）：
  ```
  ⛔ 出口层有 1 处写出不是「管线产物本身」：
    - 第 1356 行的管线产物 `json` 在写出之前被用在了别处：`if !local_ok(json.as_str()) {`
      —— 那是一次「出口自己又判了一遍 / 又加工了一遍」
  ```
  T5：`` `fn local_ok` **没有登记在 OUTLET_FNS 里** ``。
- ⚠️ 口径说明：上一轮的原写法 `if !local_ok(json)`（`json` 当时是 `&str`）在这一版**写不出来**（`json` 已是 `OutletRender`），所以我用 `.as_str()` 写成等价可编译形态。**这不是「编译期挡住」，是 T5+T2 判红**。

### 3.3 绕过 #2 —— 公开构造器改名（M2）

```rust
impl OutletRender {
    pub fn craft(text: String) -> Self { Self { text, report: DropReport::default(), outlet: "C3" } }
}
```
- `pipeline.rs` sha256：`88C28710230A63A2DC2160A225FFA6A6B3417A476C854AA169707A8A498F034D` → `0EB50EB1885236744051E267A3CDE334A63480617541D034B5970887F4B33D83`
- **红 ×1**：`t4_only_one_public_factory_per_guarded_type`。
- 判定：**R-C 成立**。`t4` 按**返回类型**枚举（`-> OutletRender` / `-> Self` in impl），不看名字 ⇒ 上一轮「换名字就绿」的那一格真的补上了。

### 3.4 绕过 #3 —— 新增 `/api/raw`（M3，不入表）

```rust
.route("/api/raw", get(get_raw))
async fn get_raw(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(&*state.graph).unwrap_or(serde_json::Value::Null))
}
```
- `http.rs` sha256：`6BEF88DF4FC9E5055A3807FC889692335BEAC86B75EF9868D8C3F4C98E5E94CC` → `FBD49A447BE3D7BE793CB187C11261DBE05BDDC12CD62E60D8AE15FF426E61F0`
- **红 ×2**，原文：
  ```
  ⛔ 有 1 处客户可见写点**没有登记**（AC-003：新增出口必须当场红）：
    - src/server/http.rs:313 `Json(` —— 既不在 OUTLETS 登记里，也不在非客户白名单里
  ```
  ```
  - src/server/http.rs: `fn get_raw` **没有登记在 OUTLET_FNS 里**
  ```
- 判定：**上一轮的阻断 B1（AC-003 对 HTTP 出口不成立）在「未登记」这一格上真的修好了。**

### 3.5 绕过 #4 —— 改轨道（M4a / M4d / M4b）

| 子形态 | 改法 | 结果 |
| --- | --- | --- |
| 4a | `let mut spec = OutletSpec::C3; spec.track = Track::Internal;` | **编译期挡住** |
| 4d | 直接字面量构造 `OutletSpec { id, track, format, granularity }` | **编译期挡住** |
| 4b | 出口身份 `OutletId::C3` → `OutletId::D1`（等价可编译） | **红 ×2** |

编译错误**原文**（这是 P0-1 要求的「区分编译期与运行期」的证据）：

```
error[E0599]: no associated function or constant named `C3` found for struct `OutletSpec` in the current scope
    --> src\cli.rs:1347:65
1347 |             let mut spec = crate::domain::pipeline::OutletSpec::C3;
     |                                                                 ^^ associated function or constant not found in `OutletSpec`
```
```
error[E0451]: fields `id`, `track`, `format` and `granularity` of struct `OutletSpec` are private
    --> src\cli.rs:1348:17
1347 |             let spec = crate::domain::pipeline::OutletSpec {
     |                        ----------------------------------- in this type
```
- M4a `cli.rs`：`C5CACC1A…` → `41DF5BA46E593195733545D540FA738524F097151C2F86F4F41BBAB6B63B3054`
- M4d `cli.rs`：`C5CACC1A…` → `5506E1145F36678D64DC848A940485AEC945FA239824292B95DC6E5CF2B86365`
- M4b `cli.rs`：`C5CACC1A…` → `7537F1C146824404972210D9C0C8CF9ED12EB8C04104B488A175D28A362BD550`，红 2 条，原文：
  ```
  - src/cli.rs: `OutletId::D1` 出现 2 次，登记的是 1 次（换 ID / 多加一处管线调用都会落到这里）
  - src/cli.rs: 登记的 `OutletId::C3`（1 次）在源码里一次都没有
  ontology show（JSON 出口）（src/cli.rs）里找不到管线调用点 `OutletId::C3`
  ```
- 判定：**B2 真的修好了，而且是本需求最强的一处收口**（两条独立路径：类型层私有 + 运行时 `pairing_ok` + 源码层 T3 计数）。

### 3.6 绕过 #5 —— 出口侧「未过闸字节先写 stdout」

| 子形态 | sha256（`src/cli.rs`） | 结果 |
| --- | --- | --- |
| 5a `emit_domain_line(&raw)` | `C5CACC1A…` → `33DC8ABE8AC0E414EA27A887E5444D16F5416B7A2529E031420CDD63C86A34FF` | **编译期挡住** |
| 5b `println!("{raw}")` | → `48A1E314EFEDDC4625F79BC05F342D17000F22AAEF6ABD96E0513936DF13C657` | **红 ×1**（T1） |
| 5c `write_stdout(&raw, true)` | → `3604EA0F8CA9D02A262FB36E283C5EA28F769E22FB31DAD8CA6B85C8AEC974ED` | **红 ×1**（T1） |
| 5d `std::io::stdout().lock().write_all(raw.as_bytes())` | → `4A7E07B6B7CF721DA30E2D69BC34FA807ACFC6CC2FF71CE612B530249499C081` | 🔴 **全绿（71 passed）** |

编译错误原文（5a）：
```
error[E0308]: mismatched types
    --> src\cli.rs:1348:42
1348 |             crate::cli::emit_domain_line(&raw);
     |             ---------------------------- ^^^^ expected `OutletRender`, found `&String`
```
5d 的完整形态（**零红**）：
```rust
let raw = serde_json::to_string_pretty(&cv)?;
{
    use std::io::Write;
    let mut o = std::io::stdout().lock();
    o.write_all(raw.as_bytes())?;
}
let json = publish_customer(OutletId::C3, Product::CustomerJson(&cv));
emit_domain_line(json);
```
⇒ **REQ-106 的原始病（未过闸字节先落 stdout）在语义上仍然可表达**，只是不能再用「已登记的那个写出器」。详见 §5。

---

## 4 · P0-2 · 攻击「登记表本身」（本轮最关键）

### 4.1 汇总（**新 5 种，4 绿 1 红**）

| # | 攻击 | 结果 | 备注 |
| --- | --- | --- | --- |
| a | `/api/raw` 零闸 + **老实填表**（`OUTLETS` 加 `H4` + `OUTLET_FNS` 加 `get_raw` + 同步 33/13 与 ID 集合） | 🔴 **全绿**（71 passed + `--lib` 1 passed） | 登记成 `Enforcement::Pipeline`，**一次都没调管线** |
| b | 新增写点 + 往 `NON_CUSTOMER_WRITES` 加一行（带计数） | 🔴 **全绿**（71 passed） | 白名单 = **合法绕过** |
| c | `NON_OUTLET_FILES` 整文件豁免（4 个运维二进制）里新增客户写出点 | 🔴 **全绿**（71 passed） | 豁免粒度是**整文件** |
| d | 闭包式本地判据 `let local_ok = \|s\| s.contains(...)`，只作用在**重新拼出来的**串上 | 🔴 **全绿**（71 passed） | A12 P-1 实测确认 |
| e | 改**已有** handler，响应体不走 `Json(`/`Html(` | ⚠️ 无条件形态 **红 ×3**；**条件分支形态全绿** | A12 P-2 实测确认（见 4.6） |

### 4.2 (a) 「老实填表但没接线」 —— **M6a / M6b**

**M6a**（只填表，不动测试里的 33/13 与 ID 集合）：

- `OUTLETS` 追加：
  ```rust
  Outlet { id: "H4", path: "src/server/http.rs",
           call_site: "serde_json::to_value(&*state.graph)",
           spec: spec_of(OutletId::H2), enforcement: Enforcement::Pipeline,
           exemption_reason: CUSTOMER },
  ```
- `OUTLET_FNS` 追加 `("src/server/http.rs", "get_raw", "新增路由 handler")`
- 结果：**T1–T7 全绿（7 passed）**；红的只有两处**硬编码数字**：
  - `test_req107_outlet_registry_matches_the_33_baseline`：`left: 34, right: 33`
  - `--lib domain::pipeline::tests::registry_is_the_33_baseline`：`left: 34, right: 33`
- sha256：`http.rs 6BEF88DF…` → `C4D18A0B5CF28903D8DBB1573788385F2F357345A4BFE9F3C09F5B59848739C5`；`pipeline.rs 88C28710…` → `2280D59BE4222F079D6DCC4EF7EF101E2353452FFE8AB6C322C1054F72CD9083`；`test_req107_source_enumeration.rs 3F6DD035…` → `C0BB1A347D222D85CA70511C43281848A263C1C817A534F4C3916A7D5CCE015F`

**M6b**（= M6a + 把「老实填表」做完整：数字 33→34、13→14、ID 集合加 `H4`）：

- 追加改动（`pipeline.rs` 内联测试 / `test_req107_outlet_pipeline.rs`）：
  ```
  assert_eq!(OUTLETS.len(), 34, …);  assert_eq!(outlet_count(Track::Customer), 14);
  want.push("H4".into());
  ```
- **结果：13 个目标 71 passed / 0 failed，`--lib` 1 passed。**
- sha256：`http.rs` → `34F765168FC1596121239F499494884E42BFA157CA126394F58D91CA33D969FC`；`pipeline.rs` → `E312EA2A6EFFECCF68CBEF833E2EB1762074DDB7269CAA8578E0DDB1CEF62316`；`outlet_pipeline.rs B395D8D0…` → `22BBD19E8F5CBCA388EA85B4A340E04B2052CEDBA85D661B623D5AC3091E3FF9`

**⇒ 判定：登记表对「接线」只是仪式。**
`enforcement: Enforcement::Pipeline` 是一个**无人校验的声明**：T1 只问「这个写点有没有出现在某条 `call_site` 里」，从不问「这条出口有没有真的调 `render` / `publish_value`」；T3 只对 `OutletId::` **记号出现次数**对账，而这条新出口**一个 `OutletId::` 都没有**，所以 T3 也看不见它。
**危害**：一个「登记了但没接线」的客户出口，从 7 条源码判据到 12 条既有判据**全部无感**，只有行为测试（若恰好覆盖该路由）才可能间接发现——而新出口默认没有行为测试。

### 4.3 (b) 白名单 = 合法绕过 —— **M7**

```rust
// src/server/http.rs，get_graph 里 check_scope 之后
println!("{}", serde_json::to_string_pretty(&*state.graph).unwrap_or_default());
```
```rust
// tests/test_req107_source_enumeration.rs，NON_CUSTOMER_WRITES 追加一行
("src/server/http.rs", "serde_json::to_string_pretty(&*state.graph)", 1, "QA-M7：手搓的零闸客户 JSON 写出"),
```
- sha256：`http.rs 6BEF88DF…` → `7385EC1D286747E745CF903FB70B1DEDB99DAD44552D7BD60818DF88B740F8E9`；`test_…_source_enumeration.rs 3F6DD035…` → `0E560C1FB3D6EB32500AB1A8686ECAC55CF82FAD8B0E3D74C4414AEBBF605DCD`
- **结果：71 passed / 0 failed**（T1 的白名单命中数 1 == 登记值 1，双向校验通过）。
- **危害（比 (a) 更现实）**：T1 的失败文案**本身就在指路**——
  「既不在 `OUTLETS` 登记里，也不在非客户白名单里」⇒ 一个只想让 CI 变绿的 Dev，最自然的动作就是往白名单加一行。
  白名单条目的第 4 个字段是**自由文本理由**，没有任何机器校验：写「这是运维回执」和写「这是客户视图」在判据眼里一样。
  ⇒ 「非客户写点」这个口径目前是**自证的**。

### 4.4 (c) 整文件豁免 —— **M8**

```rust
// src/bin/ontok-propose.rs（NON_OUTLET_FILES 里的 4 个运维二进制之一）
println!("抽取完成");
println!("{}", serde_json::json!({"schema": &report.schema, "tables": report.tables.len()}));
```
- sha256：`A8C0A209B68A5ADC61668824674477CF2A9C0A6D11156AB40A8F07CC7553EB0D` → `420E7BA744D74D83F87DDFA00C0DEF389CB3878B2661259EAB746D2B5C95902B`
- **结果：71 passed / 0 failed。**
- 原因（代码可证）：`unregistered_write_points` 在枚举写点**之前**就 `if NON_OUTLET_FILES.iter().any(|(p,_)| p == rel) { continue; }`（`test_req107_source_enumeration.rs:422-424`），整文件跳过。
- **判定（A12 P-3 要求我判「可接受」还是「必须收窄到函数级」）**：
  **今天可接受、下轮必须收窄到函数级。** 理由：这 4 个二进制**不承载** 13 条客户出口中的任何一条；但「整文件豁免」的代价是：**任何人往这 4 个文件里加一个客户写出点都不会红**（我实测如此），而 `src/bin/` 恰恰是「顺手打个调试输出」的高频位置。
  收窄成本很低：把 `NON_OUTLET_FILES` 从「路径」改成「(路径, 函数名)」——现有 4 个 bin 的写点都集中在 `main`/少数几个函数里，一次性登记即可。

### 4.5 (d) 闭包式本地判据 —— **M9**（A12 P-1 实测）

```rust
let local_ok = |s: &str| !s.contains("key");
let raw = serde_json::to_string_pretty(&cv)?;          // 只作用在**重新拼出来的**串上
if !local_ok(&raw) { eprintln!("QA-M9: …"); return Ok(()); }
let json = publish_customer(OutletId::C3, Product::CustomerJson(&cv));
emit_domain_line(json);
```
- sha256：`cli.rs C5CACC1A…` → `3742A588694910E30F4FC0A941BD2C366FC19E7E91CC0DF6CCB4115DA28112E0`
- **结果：71 passed / 0 failed。**
- 原因：T5 的 `fn_definitions` 只认 `fn` 定义（它自己还断言 `let f = |x| x;` 是 0 命中）；T2 只看**管线产物**在「赋值→写出」之间被怎么用；闭包作用在 `raw`（另一份串）上，三处都不红。
- **危害**：这份闭包可以**完整地决定「这一条出口要不要写」**，而且可以**反着写**（`if raw.contains("key") { /* 换成干净文案 */ }` 之后再写手拼串——那就更接近「自造一份客户视图」）。它今天只影响 C3 的写出与否，不改变已过闸的字节，所以危害是**功能性的静默丢失/改写**，不是直接泄漏；但它就是「出口侧第二份判据」的形态，AC-003 要消灭的正是这个。
- 判定：**非阻断**（Dev §10.9-2 已如实登记，且 A12 P-1 明确允许「登记为已知限制」）；但**危害评估要补一句**：它可以**不碰管线产物**，所以 T2 那一层永远抓不到它。

### 4.6 (e) handler 返回裸 `String`/元组 —— **M10 / M10b**（A12 P-2 实测）

**M10（无条件替换 `get_node` 的响应体）**：

```rust
let raw = serde_json::to_string_pretty(node).unwrap_or_default();
if let Ok(r) = Response::builder().status(StatusCode::OK)
        .header("content-type","application/json")
        .body(axum::body::Body::from(raw)) { return Err(r); }
```
- sha256：`http.rs 6BEF88DF…` → `B76E4F77FEEDC1BAFFC061A803EBF0F93ADFF1E65534B6F7C4E64579B3523B85`
- **T1–T7 全绿（`test_req107_source_enumeration` 7 passed）**——枚举层完全看不见 `Response::builder().body(...)`。
- 但**行为判据红了 3 条**：`test_req107_cardpoint2_whole_document_vs_per_node_granularity`（脏节点不再 500）、`test_req107_pipeline_bytes_equal_golden`、`test_req107_zero_visible_change_across_13_customer_outlets`。
- ⇒ **A12 P-2 的前半句成立**（「不走 `Json(`/`Html(` 的响应体不在写点枚举内」）；后半句「改已有函数抓不到」**部分成立**：只有当改动破坏了行为判据覆盖的路径时才红。

**M10b（条件分支后门 —— 更真实的形态）**：

```rust
// get_graph 开头，check_scope 之前
if q.get("qa").is_some() {
    let raw = serde_json::to_string_pretty(&*state.graph).unwrap_or_default();
    if let Ok(r) = Response::builder().status(StatusCode::OK)
            .header("content-type","application/json")
            .body(axum::body::Body::from(raw)) { return Err(r); }
}
```
- sha256：`http.rs 6BEF88DF…` → `54964D46E9707007F38A9F9B199CD7C7284D07CC7178D321B6002A09E5BA8F63`
- **结果：71 passed / 0 failed**（含 A12 关心的全部 7 条 T1–T7）。
- **危害（最大的一条）**：这是一个**永久存在的零闸后门**：
  正常请求行为不变 ⇒ 行为判据全绿；响应体不走已登记构造 ⇒ 枚举判据全绿；没有新函数 ⇒ T5 全绿；没有新 `OutletId::` ⇒ T3 全绿；没有新模块 ⇒ T6 全绿。
  而且它**顺手绕过了 `check_scope`**（我把它放在 `check_scope` 之前）。
  ⇒ 只要有人**故意**留后门，7 条源码判据 + 12 条既有判据**一个都拦不住**。

---

## 5 · P0-3 · A9 #2 是否真的补回来了

### 5.1 「未过闸的字节先写出去」现在做不到吗？—— **分两半答**

| 形态 | 能不能写出来 | 证据 |
| --- | --- | --- |
| 用 `emit_domain_text/line` 传裸 `&str` | **不能（编译期）** | `error[E0308]: mismatched types … expected OutletRender, found &String`（§3.6 原文） |
| 用 `println!` / `write_stdout` | **能，但红** | T1 判「该写点未登记」（M5b/M5c，sha256 见 §3.6） |
| 绕开所有已登记构造，直接 `std::io::stdout()` | **能，且全绿** | M5d，71 passed |

⇒ 类型层挡住的是「**经合法写出器传未过闸字节**」；它挡不住「**自己再开一条 stdout**」。
Dev §10.9-1 已如实登记这一点，但同一句里写「补强在类型层（`emit_domain_*` 只收 `OutletRender`）」**不准确**：类型层对这第三种形态**零作用**（`std::io::stdout()` 是 `std` 的公开 API，没有类型能封）。

### 5.2 `test_exit_ordering_req106` 现在能抓什么、不能抓什么

我逐行读了该文件（271 行；本轮**未改动**，`git diff b55d07c..4bbd8ac` 里没有它）：

- **能抓**（`arm_slice` 只切 `src/domain/pipeline.rs` 的 `Product::CustomerJson` / `CustomerHtml` 两个 arm）：
  ① 闸调用 `gate_text(spec, &text)?` 必须出现在 `Ok(OutletRender::build(` **之前**；
  ② 产物构造那一行的**缩进**不得比闸更深（防「挪进拒发分支内部」）；
  ③ `page.rs` 的三条（过 `mcp::tools::guard` 本身 / 不自带判据 / 占位符占整个 JS 字面量）；
  ④ 命中清单不许进 stdout（`eprintln!` 版）。
- **不能抓**：**它完全不看 `src/cli.rs` / `src/server/http.rs` / `src/mcp/serve.rs`**。
  ⇒ 出口在**调用管线之前**写一份手拼字节（M5d）／出口在管线产物之外另写一份（M10b）——这两格它**结构上不可能**发现。

⇒ **A9 #2 的判定（第二版）**：
- 上一轮我判 A9 #2「放宽了」，证据是 M7（出口侧先写后判）全绿。
- 本轮：**审稿的那条路径被补上了两处**——G-4 的 ③ 从「`.as_str()` 形状」改成「`let <binding> = publish_customer(…)` **且**赋值行在写出之前」（`test_binary_output_guard.rs:1073-1116`），T2 也钉了同一件事。
- 但 **A9 #2 原文（`test_exit_ordering_req106` 本身）仍然只钉管线内部**，出口侧由 T2 + G-4 ③ 代偿；而 T2/G-4 都只盯「**那个绑定**」，对「另开一条 stdout」无感（M5d 全绿为证）。
- **判定：显著改善，但未完全补回**。要真正补回，必须有一条判据是「出口函数体里**除了** `emit_domain_*` 之外不得出现任何 stdout 写机制」——这是**否定式枚举**（`std::io::stdout` / `print!` / `write_all` / 第三方日志宏），成本低，且能一次收掉 M5d。

---

## 6 · P0-4 · 上一轮「给分」的部分有没有退化

### 6.1 13 条客户出口是否仍全部接线（我自己读的 `path:line`）

| # | 出口 | 管线入口（我看到的） | 写出点（我看到的） |
| --- | --- | --- | --- |
| C1 | `ontology show --html` → stdout | `src/cli.rs:1305-1311` `publish_customer(OutletId::C1, CustomerHtml{…})` | `src/cli.rs:1333` `emit_domain_text(html)` |
| C2 | 同上 `--out` → 文件 | 同一个产物 `html` | `src/cli.rs:1322` `std::fs::write(p, html.as_str())` |
| C3 | `ontology show` → JSON | `src/cli.rs:1346-1349` | `src/cli.rs:1352` `emit_domain_line(json)` |
| C4 | `ontology overview` → 文本 | `src/cli.rs:1382-1385` | `src/cli.rs:1386` `emit_domain_text(text)` |
| C5 | `ontology overview --json` | `src/cli.rs:1376-1379` | `src/cli.rs:1380` `emit_domain_line(json_text)` |
| H1 | `GET /` 整页 | `src/server/http.rs:340-343` `render(OutletId::H1, Product::Text(html))` | `:344` `Ok(Html(rendered.into_text()))` |
| H2 | `GET /api/graph` | `src/server/http.rs:280-283` `publish_json(OutletId::H2, …)` | `src/server/http.rs:200` `Ok(Json(rendered.into_value()))` |
| H3 | `GET /api/node` | `src/server/http.rs:305-308` | 同上 `:200` |
| M1–M5 | 5 个工具 | `src/mcp/serve.rs:314-317` `publish_value(OutletId::McpTools, value)` | `:318-321` `p::tool_call_result(&text, structured)` |
| Z1 | `check_scope` 不匹配 | `src/server/http.rs:256-262` `render(OutletId::Z1, Product::ErrorText(…))` | `:269` `err(StatusCode::BAD_REQUEST, rendered.as_str())` |
| Z2 | `tools/list` | `src/mcp/serve.rs:284` `publish_value(OutletId::Z2, result)` | `:285` `p::encode_result(id, rendered.into_value())` |
| Z3 | `initialize` | `src/mcp/serve.rs:252` `publish_value(OutletId::Z3, result)` | `:253` 同上 |

（D1 建模轨 `src/cli.rs:1265-1270` 也仍在，不属 13 条。）

### 6.2 五红线 / `guard()` / `property_filter` G1–G4

```powershell
git diff --numstat develop -- tests/drop_rate.rs tests/test_fk_naming.rs src/domain/view.rs src/domain/review_view.rs src/mcp/tools.rs
```
**实测：输出为空**（0 行）⇒ 五个文件零改动（`src/mcp/tools.rs` 仍是完全零改动）。

`property_filter.rs` 前 145 行：

```
develop145 = 90978A8D761512535A44D5D380AB2DF26B629C8272E92DA15E358191E11451F0
current145 = 90978A8D761512535A44D5D380AB2DF26B629C8272E92DA15E358191E11451F0
SAME=True        差分行数 = 0       # git diff --numstat develop = 12 / 0（纯追加）
```
⇒ **与上一轮一致，逐字节未动。**

### 6.3 无 `#[ignore]`、无删测试、断言未放宽

| 项 | 实测 |
| --- | --- |
| 新增 `#[ignore]` 属性行（`git diff develop` 里 `+` 且 trim 后以 `#[ignore]` 开头） | **0** |
| 全库真正的 `#[ignore]` 属性（含 `#[ignore = "…"]`） | 只有 `tests/test_mcp_server.rs:553` 一条，**develop 就有**，未变 |
| 删测试文件（`--diff-filter=D`） | **0** |
| 删除 `#[test]` / `fn test_` 行 | **0 / 0** |
| 断言计数 `develop → 现状` | `test_property_filter` 23→**30**；`test_exit_ordering_req106` 11→**11**；`test_binary_output_guard` 73→**79** |
| 断言计数 `b55d07c → 4bbd8ac`（**本轮**） | `binary_output_guard` 79→**79**；`property_filter` 30→**30**；`outlet_pipeline` 55→**56**；`track_manifest` 11→**12**；`scope_error_gate` 20→**21** |

### 6.4 被改写的既有测试逐条判「改紧还是放宽」

| # | 测试 | 本轮改了什么（`b55d07c..4bbd8ac`） | 我的判定 |
| --- | --- | --- | --- |
| A9-1 | `test_property_filter::all_three_outlets_call_the_one_shared_function` | **只把 `OutletSpec::C3/H3/MCP_TOOLS` 机械改成 `OutletId::…`**（3 行） | **未放宽**（上一轮已判「改紧」，本轮无实质变化） |
| A9-2 | `test_exit_ordering_req106` 的两条 | **本轮一字未改** | **仍是「只钉管线内部」**（见 §5.2）；本轮未恶化也未修复 |
| A9-3 | `test_binary_output_guard` 的 G-4 | ③ 从「`.as_str()` 形状」→「`let <binding> = publish_customer(…)` **且赋值在写出之前**」；`has_entry` 去掉已删除的 `publish_error_text`；M-P3/`judge_like_line` 锚点跟随 | **改紧了**：新增的「时序」这一维正好是上一轮 M7 打穿的那一格；`g4_guard_is_not_vacuous` 里 M-P3 仍必须红（我跑了 7 passed）。**残留**：② 的判据名单仍是 5 个**拼写**，③ 是**形状**匹配 ⇒ M9/M10b 不碰绑定，仍然绕过 |
| 附 | `test_req107_track_manifest` 的「误标」③ | 旧写法 `spec.track = Track::Customer` 已不可编译 ⇒ 同一句断言改成「同一份产物走 `C3`」+ 新增「`CustomerJson` 不许从 `D1` 进入」（2 条新断言） | **未放宽**（**没有删任何 `assert!`**，总数 11→**12**）。语义等价：仍验「轨道决定拦不拦」，并多钉一条「换 ID 不能关闸」。⚠️ 唯一失掉的是「**同一个 spec 值被改标**」这一形态——那个形态在源码里已经不存在了（`E0451` 为证），属**结构性消失**而非放宽。另注：`test_req107_guard_granularity` 里那 2 条「被删掉的 assert」实为 `OutletSpec::C4/C5` → `OutletId::C4/C5` 的**同句替换**（`-`/`+` 成对），不是删除 |

### 6.5 Z1 / Z2 / Z3 修复是否仍在

- Z1：`test_req107_scope_error_gate` **4 passed**；代码路径 `http.rs:256-269` 仍 `render(OutletId::Z1, Product::ErrorText(…))` → `err(400, rendered.as_str())`。
- Z2/Z3：`test_req107_static_texts` **3 passed**；`serve.rs:252/284` 仍走 `publish_value`（`Track::Static` ⇒ 过运行时闸）。
- D1 建模轨误标可观察：`test_req107_track_manifest` **2 passed**。

### 6.6 golden 与零可见变化（抽样复算，用**基线 worktree**）

方法：在 `.review-workspace/req107/base`（detached @ `b714f48`）用**基线 API** 重算（`render_html` / `filter_view` / `render_overview_text` / 基线路由 `state_from_view` + `build_router`），再用 PowerShell 独立取 sha256 与入库 golden 比（不重跑 Dev 的生成器）。base worktree 的 unix-only bin 同样挪出/挪回，sha256 `C7CF95E2…322D` 前后一致。

| 文件 | 复算 = 入库 golden | sha256（前 16） |
| --- | --- | --- |
| `C1_html.txt` | **MATCH** | `48650BCE476B9225…` |
| `C3_json.txt` | **MATCH** | `1DD509C56BE767CC…` |
| `C4_overview.txt` | **MATCH** | `2EE6E71FBE96B927…` |
| `C5_overview_json.txt` | **MATCH** | `7DB9BB0151903AD1…` |
| `H1_index.html` | **MATCH** | `E8753C75B959FCA6…` |
| `H2_graph.json` | **MATCH** | `6B06847A755EAB66…` |

- `SHA256SUMS` 与 13 个 golden **逐个互校通过（13/13）**。
- golden 文件在 `b55d07c..4bbd8ac` 之间 **零改动**（`git diff --name-only` 空）。
- `test_req107_zero_visible_change` **3 passed**、`test_req107_pipeline_bytes` **1 passed**。

⇒ **AC-005 在可复核范围内未退化。**（口径漏洞与上一轮 §6.3 相同：CLI 五条出口是「用生产函数复刻」，不是真进程——Dev 已登记。）

---

## 7 · 阻断项 / 非阻断项

### 7.1 阻断（2 条）

| # | 阻断项 | 一句话 | 证据 |
| --- | --- | --- | --- |
| **B1** | **AC-003 对「老实填表但没接线」不成立** | 新增零闸 `/api/raw`，在 `OUTLETS` 登记成 `Enforcement::Pipeline`（`spec_of(OutletId::H2)`）+ `OUTLET_FNS` 登记 `get_raw` + 同步 33/13/ID 集合 ⇒ **71 passed + lib 1 passed，零红**。`enforcement: Pipeline` 是**无人校验的声明**：T1 只问「写点在不在某条 `call_site` 里」，T3 只数 `OutletId::` 记号，而这条出口一个 `OutletId::` 都没有 | §4.2（M6a/M6b，sha256 + patch） |
| **B2** | **「非客户写点白名单」自证，等价于合法绕过** | 在 `http.rs` 加一行 `println!` 打客户 JSON，再往 `NON_CUSTOMER_WRITES` 加「文件 + needle + 期望 1 次 + 理由」⇒ **71 passed**。第 4 个字段（理由）无任何机器校验；T1 的失败文案本身就在引导这条修法 | §4.3（M7） |

> 这两条同源（**两张表都是自证清单**），但**修法不同**、都很便宜（见 §9），建议一起修。

### 7.2 非阻断（登记，不单独否掉本次）

| # | 项 | 说明 / 建议 |
| --- | --- | --- |
| N1 | **A12 P-2 成立**：改已有 handler + 条件分支 + `Response::builder().body(…)` ⇒ **全绿**（M10b，71 passed），且顺手绕过 `check_scope` | Dev §10.9-3 已登记「改已有函数抓不到」，但**危害评估不足**：这不是「抓不到」，是「一个永久后门，7 条判据 + 12 条既有判据全无感」。修法：枚举**响应构造**（`Response::builder` / `Body::from` / `(StatusCode, String)` 返回类型）并纳入写点枚举，或对 3 个数据路由加「响应体必须来自 `publish_json`」的否定式判据 |
| N2 | **A12 P-1 成立**：闭包式本地判据 ⇒ 全绿（M9，71 passed） | 已登记；补一句危害：它**不碰管线产物**，T2 永远抓不到；可决定「这一条出口写不写/写什么」 |
| N3 | **A12 P-3**：`NON_OUTLET_FILES` 整文件豁免 ⇒ 在 4 个运维二进制里加客户写出点全绿（M8） | **今天可接受，下轮收窄到函数级**（改成 `(路径, 函数名)`） |
| N4 | **M5d**：绕开所有已登记构造、直接 `std::io::stdout()` ⇒ 全绿 | Dev §10.9-1 已登记；但「补强在类型层」这句**不准确**（类型层对它零作用）。建议补一条否定式：出口函数体里除 `emit_domain_*` 外不得出现 `print!` / `println!` / `stdout()` / `write_all` |
| N5 | 卡点② 的 `Granularity` 仍**不参与执行**（上一轮 N2，未修） | `render` 不读 `granularity`；H3 的「逐节点」来自 handler 形态。属声明性，非阻断 |
| N6 | Dev 报告 §10.9-1 的措辞 | 「补强在类型层」应改成「类型层只覆盖 `emit_domain_*`；其它 stdout 机制无类型层屏障」 |
| N7 | `NON_OUTLET_FILES` 不要求路径存在（`test_req107_source_enumeration.rs:457-459` 自陈） | 「删了源码、白名单还在」看不见；A8 绕行的副作用。可接受，已登记 |
| N8 | AC-009-4「不许放宽断言」仍无机器判据 | 与上一轮同；我用断言计数（只增不减）侧证 |

### 7.3 我确认「没问题」的部分（避免被读成全盘否定）

- **上一轮的 3 条阻断里，B1（未登记新出口）与 B2（改轨道）真的修好了**，而且是**结构性**的（枚举 + 类型层 + 运行时 `pairing_ok` 三层），不是换个拼写；
- 上一轮的 5 种绕过里 **4 种已红/编译期挡住**（其中「公开构造器」从「靠拼写」升级为「按返回类型枚举」）；
- 13 条出口**真的**全接线（§6.1 逐条 `path:line`）；出口层**真的**零判据直调；
- 红线零触碰、`guard()` 零改动、`property_filter` G1–G4 逐字节未动、无删测试、无新 `#[ignore]`、断言只增不减；
- golden 来源与内容可复现（6/6 MATCH、`SHA256SUMS` 13/13）、golden 本轮零改动、Z1 仍是唯一可见变化；
- Z1/Z2/Z3 的修复仍在，且**判别力仍在**（上一轮 M9/M11 会红；本轮基线 4+3 passed）；
- Dev 报告的本机数字与我实测**逐条一致**，包括 lib 的 168 passed / 2 failed（DB）——**没有编数**。

---

## 8 · 我无法确认的项（如实列，不用推测填空）

1. **需 DB 的用例**（本机无 PostgreSQL，全部标「环境不具备」，**没有算作通过**）：
   `test_mcp_server`（上一轮 6 passed / 25 failed / 1 ignored）、`test_domain_print_sites_binary_req078`（4 failed）、`--lib` 的 2 条（`db::apply` / `extract::introspect`，`MissingEnv("ONTOK_TEST_DATABASE_URL")`）。本轮我**没有重跑** mcp / print-sites 这两个目标（与上一轮同一限制）。
2. **`tests/test_retained_names_req084.rs`：跑不了**（A10：`include_str!` 引用 unix-only bin；bin 挪出时编不过、挪回时 bin 本身在本机编不过）。
3. **全量 `62 suite / 846 passed @ 7f26fa2`**：本机不可复核（既有 bin 的 `std::os::unix`）。
4. **真进程 e2e / 真数据（demo3 v2）上的字节不变**：无 DB，未实测。
5. **golden 我只独立重算 6/13 份**（C1/C3/C4/C5/H1/H2）；H3 两份与 M1–M5 由 `test_req107_pipeline_bytes`（1 passed）间接覆盖。
6. **变异三元组的完整性**：我采的是「被改文件 sha256 前/后 + `git diff` patch + 逐目标红/绿 + 失败用例名」；**测试二进制 sha256 未逐次留存**（cargo 每次重编都会变）。这一项我不声称做到纪律 G 的完整三元组。
7. **`test_req107_source_enumeration` 的两个辅助函数（`blank_strings` / `write_points`）我只做了「反空转自查通过 + 我自己的 18 次变异」的实证**，没有做形式化验证（例如嵌套块注释、宏内部的多行写点是否会被漏认）。我在自己的变异里没遇到漏认。
8. **上一轮 Dev 第 1 轮的「历史是否改过又改回」**：仍未核（只能看逐 commit diff；我只做了 tip vs develop）。

---

## 9 · 建议的最小修法（给 BA 派单用，不是我在实现）

1. **B1（登记表 ↔ 接线）**：对每一条 `enforcement == Enforcement::Pipeline` 且 `path` 落在三个出口文件里的 `OUTLETS` 条目，**强制**该文件里至少出现一次 `OutletId::<该条目对应 ID>`（`C2` 显式白名单为「复用 C1」）。成本：一张 `id -> token` 映射 + 一条断言；能一次杀掉 M6b。
2. **B2（白名单）**：`NON_CUSTOMER_WRITES` 的每个 needle，**必须**在写点窗口里**同时**不含 domain 生产者（`to_string_pretty(` / `to_value(` / `render_html(` / `render_overview_text(` / `filter_view(` / `encode_result(`）；或给每条白名单加一个「该写点的实参必须全部是字面量或非 domain 值」的静态检查。能一次杀掉 M7。
3. **N1（A12 P-2）**：把「响应构造」纳入写点枚举：`Response::builder` / `Body::from(` / `(StatusCode, String)` / `impl IntoResponse` 的返回类型；并对 3 个数据路由加「响应体必须由 `publish_json` / `render` 产出」的否定式判据。
4. **N4（A9 #2 的残余）**：给三个出口文件加否定式判据 —— 函数体里除 `emit_domain_text/line` 外不得出现 `print!` / `println!` / `stdout()` / `write_all(`（`eprintln!` 放行）。
5. **N3（A12 P-3）**：`NON_OUTLET_FILES` 从「路径」改成「(路径, 函数名)」。
6. **文档订正**：§10.9-1「补强在类型层」→「类型层只覆盖 `emit_domain_*`」；§10.9-3 补危害评估；卡点② 的「管线按声明执行」仍是声明性的。

---

## 附录 A · 证据索引（全部未跟踪，可复现）

| 内容 | 路径 |
| --- | --- |
| 变异 worktree（detached @ `4bbd8ac`，最终 `git status` 空） | `.review-workspace/req107/qa2` |
| 变异源码（FILE/OLD/NEW 三段式） | `.review-workspace/req107/qa2-patches/M*.txt` |
| 每次变异的 patch（含 A8 挪出 bin 的删除 hunk） | `.review-workspace/req107/qa2-logs/M*.patch` |
| 每次变异的完整 cargo 输出 | `.review-workspace/req107/qa2-logs/M*.log` |
| 基线 13 目标 | `.review-workspace/req107/qa2-logs/baseline.log` |
| A8 sha256 前后 | `.review-workspace/req107/qa2-logs/sha.txt` |
| golden 复算产物（base worktree） | `.review-workspace/req107/qa-golden-recompute/r3/` |
| 应用脚本 / 运行脚本 | `.review-workspace/req107/apply.ps1` / `.review-workspace/req107/run.ps1` |

## 附录 B · 我这一轮实测到的基线数字（供对账）

| 命令 | 实测 |
| --- | --- |
| 13 个目标（`--no-fail-fast` 一次性） | **71 passed / 0 failed**（7/12/4/1/2/4/3/2/3/10/7/7/9） |
| `cargo test --lib` | **168 passed / 2 failed**（DB） |
| `cargo test --doc` | **4 passed** |
| `cargo test --lib registry_is_the_33_baseline` | 1 passed |

---

> **给我自己的一句提醒**：这一轮的修复是**真修**——上一轮我打穿的「新增未登记路由」与「改轨道」两条，现在一条红、一条编译期挡住，而且都不是靠拼写。
> 但「漏接即红」这句话的**主语**仍然是**登记表**，不是**接线**：
> 只要有人**如实填表**（甚至不需要说谎，只是照着失败文案填），一个零闸客户出口就能全绿。
> ⇒ **不能按「已收口」合并**；两条阻断都是低成本可修的，修完可以直接复验（M6b/M7 两个用例现成）。
