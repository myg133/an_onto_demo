# REQ-107 · 出口收口 —— 设计（先设计，后写代码）

> 代码基线 `develop` `b714f48`。本文件是 Dev 侧的**实现前**设计，三个卡点的裁定在此定稿，
> 交付物（`verification-report.md`）只引用、不重写。

---

## 一 · 病根与目标（一句话）

同一份客户视图在 13 个出口上**各自重新决定**「要不要剔除 / 要不要清洗 / 要不要拦」。
⇒ 收口成一条管线：**加载一次 → 统一前处理 → 统一发布闸 → 按出口声明的格式写出**。

出口层从此只提供两样东西：**产物**与**声明（`OutletSpec`：格式 + 轨道 + 闸粒度）**。
「拦不拦」由 `spec.track` 决定，不由出口代码决定。

---

## 二 · 管线本体（`src/domain/pipeline.rs`）

### 2.1 类型

```rust
pub enum Track { Customer, Modeling, Internal, Static, ErrorPath }
pub enum Format { Html, Json, Text }
pub enum Granularity { Whole, PerNode }

pub struct OutletSpec {           // 出口**声明**的唯一载体（Copy）
    pub id: &'static str,         // "C3" / "H3" / "M1-M5" …
    pub track: Track,
    pub format: Format,
    pub granularity: Granularity,
}
impl Track {
    /// 要过发布闸吗？**只看轨道**，出口代码无权决定。
    pub const fn gated(self) -> bool { matches!(self, Track::Customer | Track::Static) }
}
```

| 轨道 | 过闸 | 谁用 |
| --- | --- | --- |
| `Customer` | ✅ | C1–C5 / H1–H3 / M1–M5 |
| `Static` | ✅（AC-008 裁定「运行时闸」） | Z2 `tools/list`、Z3 `initialize` |
| `Modeling` | ❌（豁免，**显式声明**） | D1–D8 |
| `Internal` | ❌（内部运维输出） | 内部 9 条 |
| `ErrorPath` | ❌ 过 `safe_error`（比闸更保守：命中即整句换掉） | Z1 |

### 2.2 产物（`Product`）—— 出口只能给这个，不能给「已拦过的东西」

```rust
pub enum Product<'a> {
    CustomerHtml { view: &'a CustomerView, sampling: &'a [String] }, // 渲染 + 剔除 + 闸
    CustomerJson(&'a CustomerView),                                 // 剔除 + 序列化 + 闸
    Overview(&'a Overview),                                         // 两条格式都渲染、都闸
    Text(String),                                                   // 已渲染文本（首页/建模投影）
    ErrorText(String),                                              // 错误文案 → safe_error
}
```

### 2.3 入口

```rust
/// 文本轨（CLI / 首页）。命中 ⇒ `Err`；**不自己 exit**。
pub fn render(spec: OutletSpec, product: Product<'_>) -> Result<OutletRender, PipelineError>;

/// 结构化轨（HTTP / MCP）。命中 ⇒ `Err`（消息**逐字**取自 `mcp::tools::guard`，
/// 所以 500 正文与 `isError` 文案一个字节都不变）。
pub fn publish_value(spec: OutletSpec, value: Value) -> Result<GuardedValue, PipelineError>;

/// 装配期前处理（`/api/graph` `/api/node` 共用的那份已剔视图；MCP 五工具共用的那份）。
pub fn prepare_view(view: &CustomerView) -> FilteredView;
```

### 2.4 为什么「前处理」与「闸」分开（AC-010-2 要回答的「为什么这样切」）

两者**判的不是同一件事**，混在一个函数里就再也分不开：

| | 前处理（`filter_view` / `sanitize_display`） | 发布闸（`guard` / `scan_modeling_terms`） |
| --- | --- | --- |
| 判据 | 源库标识符**形态**（`customer_id` / `key`） | 建模**术语**（`主键` / `基数` / `1:N` / `cardinality`） |
| 命中后 | **减掉那一条**，其余照发 | **整份不发**（fail-closed） |
| 可逆性 | 是（信息本来就翻不成人话） | 否（说明数据不合规） |
| 失败语义 | 静默删条目 ⇒ 必须报 stderr | 拒绝发布 ⇒ 退出码 / 500 / isError |

**顺序不可交换**：先剔后闸才能让「属性名是 `customer_id`」这类**可减**的条目减掉、
而让「对象名是 `主键台账`」这类**不可减**的条目仍然到达闸门被拒。
REQ-104 的整条价值就建立在这个顺序上；反过来（先闸后剔）会让 REQ-104 修好的 3 个节点
重新变 500。

### 2.5 类型层不变量（AC-003 的根）

```rust
pub struct OutletRender {            // 字段全私有、无 pub 构造器
    text: String,
    report: DropReport,
    outlet: &'static str,
}
pub struct GuardedValue { text: String, value: Value }  // 同上
```

出口**拿不到**这两个类型的实例，除非走 `render` / `publish_value`。
`pipeline.rs` 模块文档里有两条 `compile_fail` doctest 把这件事钉在**编译期**
（`cargo test --doc` 可跑，本机可复核）。

---

## 三 · 三个卡点（AC-004）—— 裁定 + 理由 + 被放弃方案的代价

### 卡点① 改名口径：**JSON 保持原名（不改名）**

**裁定**：`strip_id_token` 只在 `sanitize_display`（HTML / 总览展示层）生效；
JSON 出口继续只过 `filter_view`（只剔不改名）。

**理由**（三条，都是代码可证的）：

1. **JSON 的名字是标识**，不是文案。`/api/graph` 的 `nodes[].id`、`/api/node?id=`、
   MCP `describe_object(name)` / `search_objects(query)` 全部拿**同一个名字**做键
   （`http.rs:446` 的节点索引就取自 `graph.nodes[i].key`）。改名 ⇒ 图上写「客户」、
   点开传「客户」、库里的 key 是「客户ID」⇒ 寻址失败（REQ-093 的 P1 重演）。
2. **改名会让 13 个出口的字节全变**，与 AC-005「零可见变化」直接冲突。
3. `sanitize_display` 是**展示层**动作：它同时做「摘 ID 记号」「压空白」「空括号清理」，
   这些在数据通道上是**有损**的（`创建者(管理员ID)` → `创建者(管理员)`）。

**被放弃的方案：统一改名（JSON 也 `strip_id_token`）会漏什么／坏什么**

- **漏**：寻址面断裂 —— 客户从图上点一个节点，`/api/node` 404（id 对不上）；
  MCP 侧 `describe_object("客户")` 查不到，客户只会被告知「没有这个对象」。
- **坏**：REQ-104 好不容易对齐的「同一份数据、两种结果」再次分裂 ——
  只是这次分裂成「HTML 名」与「JSON 名」，比原来更难对账。
- **代价**：13 份 golden 全部变化；`/api/graph` 与 `/api/node` 的键必须同时改，
  否则触发 REQ-093 那条 `assert_eq!(nodes.len(), graph.nodes.len())` 之外的静默错位。

### 卡点② 闸粒度：**保持双粒度，但把它变成「声明」，不是「出口自决」**

**裁定**：`Granularity` 进 `OutletSpec` —— 聚合视图（C3 / H2 / M1–M5 / Z2 / Z3）声明
`Whole`（整份拒发），`/api/node`（H3）声明 `PerNode`（只拒那一个节点）。
管线**按声明执行**，出口不再自己挑。

**理由**：

- `/api/node` 的语义单位**就是**一个节点。整份拒发 ⇒ 一个坏节点让 91 个节点全打不开
  —— 这正是 REQ-104 修掉的那个事故（`demo3` v2 三个节点 500）。
- `/api/graph` / `show` JSON / MCP 没有「节点」这个可分离单位：整份响应是一棵 JSON 树，
  逐节点降级只能靠「悄悄删掉一个节点」，那会让图与节点索引**对不上**
  （REQ-093 的 P1 正是这么来的），且客户看不见自己少了什么 —— 比拒发更坏。

**被放弃方案 A：统一到「整份」** —— 漏什么：`/api/node` 一个脏节点 ⇒ 整页 91 个节点 500，
REQ-104 的成果当场回退。这是**已经发生过的事故**，不是理论风险。

**被放弃方案 B：统一到「逐节点」（聚合视图也逐节点）** —— 漏什么：
① 图里没有节点边界，只能「删节点」⇒ 图与节点视图数量不一致 + 静默丢数据；
② 跨节点上下文泄漏：`guard` 扫的是**整份序列化文本**，分段扫描看不见
   「A 节点的名字 + B 节点的说明拼起来才成立」的形态（例如一个节点名以「主键」开头、
   另一个以「台账」结尾，分段都不命中，整份才命中）。

### 卡点③ stderr 报告形状：**两级（客户侧数量 / 运维侧原名）+ 单一形状函数**

**裁定**：

- **客户可见正文**：只报**数量**（页面既有措辞 `另有 N 项…没有显示` 一个字不动）。
- **运维 stderr**：报**原名**（运维要靠它定位到具体属性；`html_view.rs:486-489` 的
  「不报原名」只约束**正文**，那条理由（原名本身是技术串）在 stderr 上不成立 ——
  stderr 不进客户眼睛、不进响应体、不进 `> out.json`）。
- **形状唯一**：所有出口的剔除报告只经 `DropReport::stderr_line()` 产出，形状常量
  `DROP_REPORT_PREFIX = "⚠ 发布闸前处理"`：
  `⚠ 发布闸前处理：共剔除 N 项（对象名 a · 整条属性 b · 只隐去说明 c · 对象说明 d）。明细（前 20 条）：…`

**理由**：`cli.rs:1243-1247`（全量原名）与 `html_view.rs:486-489`（只报数量）**不是矛盾**，
它们说的是两个不同的读者面。真正的问题是**同一条 stderr 有三个不同措辞**
（`cli.rs:1243/1259/1297`、`http.rs:427`、`serve.rs:102`），运维 grep 不到同一个抬头。

**被放弃方案：统一只报数量** —— 漏什么：运维失去定位能力。整份拒发（卡点②的 Whole 轨）
发生时，stderr 是**唯一**能看到「是哪个属性的名字触发的」的地方；把它删掉，
下一次事故只能靠猜。

**被放弃方案：统一报原名（含正文）** —— 漏什么：正文重新泄漏技术串，
且与 `BA/verify-page.js` 的黑名单冲突（正文会被判红）。

---

## 四 · 33 条口径与登记表（AC-002 / AC-007 / AC-010-1）

`pub const OUTLETS: &[Outlet]` 放在 `pipeline.rs`，**33 条**：

| 组 | 条数 | 轨道 | 执行形态 |
| --- | --- | --- | --- |
| C1–C5 | 5 | Customer | Pipeline |
| H1–H3 | 3 | Customer | Pipeline |
| M1–M5 | 5 | Customer | Pipeline（同一调用点） |
| D1 | 1 | Modeling | Pipeline（**轨道=建模 ⇒ 不过闸**，行为由 spec 决定） |
| D2–D8 | 7 | Modeling | DeclaredOnly（登记 + 站点标注；不改道，理由见下） |
| 内部 9 | 9 | Internal | InternalOnly（登记 + 站点标注） |
| Z2、Z3 | 2 | Static | Pipeline（AC-008 选**运行时闸**） |
| Z1 | 1 | ErrorPath | Pipeline（`safe_error`） |
| **合计** | **33** | | 13 / 8 / 9 / 2 / 1 |

> ⚠️ **与 TC-007 的差异（纪律 S：AC/用例自己也会错）**：TC-007 写的是
> `Static == 3`（Z2,Z3,**Z4**）+ ErrorPath 1 ⇒ 13+8+9+3+1 = **34**，
> 与它自己断言的 `OUTLETS.len() == 33` 互相矛盾。按 **A1 的裁定**（Z4 不是出口、
> 不占出口位），`Static = 2`。Z4（`INDEX_HTML` 模板）单列 `STATIC_TEMPLATES` 并
> 由**测试守卫 + H1 的运行时整页扫描**双重覆盖（见 §五）。已在交付物「异议」里登记。

### D2–D8 为什么不改道（如实登记为已知限制）

D2（`review show`）走 `emit_domain_text_with` 的**闭包接线**（REQ-072/078 的类型级保证），
D3–D8 是人话行、且已被 `DOMAIN_PRINT_SITES`/G-1/G-5 的「同一份绑定」判据钉住。
把它们改道会**同时**打穿两套既有守卫，属于「为了整齐而制造回归」。⇒ 本轮只做
**显式标注**（`Track::Modeling` + 站点标记 + 豁免理由），不改写出路径。
**误标的可观察性**由 D1（真走管线）与 TC-022 的行为半（同一份产物、两种 track）证。
这条限制在交付物「已知限制」里如实登记。

---

## 五 · Z1 / Z2 / Z3 / Z4（AC-006 / AC-008）

| 位点 | 裁定 | 做法 |
| --- | --- | --- |
| **Z1** `http.rs:222-230` | 修 | `Product::ErrorText` ⇒ 管线内 `mcp::tools::safe_error`；干净项目名**照旧回显**（TC-019） |
| **Z2** `tools/list` | **运行时闸** | `publish_value(OutletSpec::Z2, result)`；命中 ⇒ JSON-RPC `-32603`（今天不可能命中，故零可见变化） |
| **Z3** `initialize` 整段 result | **运行时闸** | `publish_value(OutletSpec::Z3, result)`；模板正文 + serverInfo + 项目名**一起**过闸 |
| **Z4** `INDEX_HTML` 模板 | **不单列运行时闸**，但**被 H1 的整页扫描覆盖** | H1 的管线对**渲染后的整页**（模板 + 已转义项目名 + 数据）过 `scan_modeling_terms`；另有测试守卫（`test_graph_server.rs` 既有 2 条 + 本轮新增） |

**为什么 Z2/Z3 选运行时闸而不是只做测试守卫**（AC-008 要求二选一并给理由）：

1. 选运行时闸后，「MCP 的全部外送」真的收在**一个函数**上（`publish_value`）——
   AC-003 的「漏接即红」自动覆盖 Z2/Z3，不需要为它们单独维护一套测试守卫的登记表。
2. 静态文本**今天**是干净的（TC-023/025 已核），所以运行时闸**零可见变化**；
   代价是每次 `tools/list` / `initialize` 多一次序列化 + 扫描（这两条请求在会话里
   各发生一次，成本可忽略）。
3. 只做测试守卫的代价：守卫只能钉住「今天这 5 条 `description` 干净」，
   钉不住「明天从快照/项目名带进来的动态部分」（Z3 的 `instructions` 里就掺了项目名
   与版本号 —— 那不是静态文本，测试守卫**不可能**枚举）。
4. **仍然补测试断言**（TC-023/025）：`description`/`title` 今天零断言，
   运行时闸只保证「不干净就不发」，不保证「发出了该有的东西」（字段被删是静默的）。

---

## 六 · 漏接即红（AC-003）—— 三层，缺一不可

| 层 | 判据 | 抓得住 | 抓不住 |
| --- | --- | --- | --- |
| **类型层** | `OutletRender` / `GuardedValue` 字段私有 + 无公开构造器 + `compile_fail` doctest（`cargo test --doc`） | 不经过管线就拿着一个「可写出的产物」 | 出口自己拼一个 `String` 直接 `println!`（它绕开的不是类型，是管线） |
| **源码否定式** | 3 个出口文件里 6 个判据名的直调点数 **== 0**；全仓 `scan_modeling_terms(` / `filter_view(` 只许在管线模块 ∪ `mcp/tools.rs` ∪ `tests/` | 出口偷调判据（**包括「保留共享调用、另加一份本地判据」的 M3 形态**：本地判据要么调那 6 个名字 ⇒ 命中，要么是新造函数 ⇒ 由第三层抓） | 全新命名的自造判据（如 `fn local_ok(s)->bool { s.contains("key") }`） |
| **写出点登记对账** | 33 条 `OUTLETS` 双向对账（条数/分组/ID/needle 命中）+ 既有 G-5（`to_string_pretty` 等 `DOMAIN_PRODUCERS` 出现在未登记的 `println!` 里 ⇒ 红） | **新增一个客户类出口但没接管线**（TC-012 的形态：手搓 `println!` + `to_string_pretty`） | 新出口用不经过 `DOMAIN_PRODUCERS` 的写法（如先 `let s = ...; println!("{}", s)`） |

三层合起来才够 AC-003 的「至少一种结构性判据」。类型层是**主判据**（源码否定式是它的补充）。

---

## 七 · 被改写 / 被删除的既有测试（如实登记）

| 文件:行 | 为什么必须改 | 改法 |
| --- | --- | --- |
| `tests/test_property_filter.rs:341` `all_three_outlets_call_the_one_shared_function` | AC §七 A3 已裁定 | 改写成**否定式**：三个出口文件对 6 个判据名的直调点数 == 0，且都必须出现管线调用点 |
| `tests/test_exit_ordering_req106.rs` `the_json_exit_judges_before_it_writes` / `…_writes_outside_the_refusal_branch` | 它们钉的是 `cli.rs` 里的 `let leaked = scan_modeling_terms(&json);` 这一行；AC-001 要求出口文件里**没有**判据直调 ⇒ 该锚点必须消失 | 锚点**迁移**到 `src/domain/pipeline.rs`：同样的「判定 → 拒发 → 才产出可写出的产物」三段次序 + 同一块归属断言；意图（先判后写）不变，位置随代码移动 |
| `tests/test_binary_output_guard.rs` G-1 / G-4 | 4 个 `same_binding` 站点（`page.html`/`json`/`text`/`json_text`）从「判据+写出两个取值点」变成「管线一次产出」，`same_binding` 族为 0 | 新增 `WIRING_PIPELINE` 接线形态：要求写入的是 `CustomerRender`/`OutletRender` 派生的绑定、同函数内有管线调用、且**该函数内判据直调点数为 0**；G-4 的「逐站变异」改为对 `WIRING_PIPELINE` 站点做「把管线调用摘掉」的变异 |

⛔ 一条都没删、没有 `#[ignore]`、没有放宽（逐条在交付物 §7 给出 `--numstat` 与理由）。

---

## 八 · 零可见变化（AC-005）怎么证

1. 13 条客户类出口的**字节**在收口前后比对（`tests/test_req107_zero_visible_change.rs`
   + `tests/fixtures/req107/golden/`，`PROVENANCE.txt` 第一行钉死 `b714f48`）。
2. golden 由**基线 worktree**（`.review-workspace/req107/base` @ `b714f48`）生成，
   不是由改完的代码生成 —— 防「用结果证明结果」。
3. **唯一允许的变化是 Z1**（AC §七 A2）：它的「变化前/变化后」逐字登记在交付物里。
4. stderr 报告形状的变化（卡点③ 的裁定产物）**不在** AC-005 的 golden 口径里
   （golden 比的是 stdout / 响应体），但**逐条登记**在交付物 §5，不静默。
