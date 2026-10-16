# REQ-093 · `ontok serve` 只读图服务 —— 独立验证报告

- **被验对象**：worktree `feature-REQ-093`，分支 `feature/REQ-093`，提交 `d216b7ac51cc956ad22caf5e8a32dc31cf6268fc`
- **基线**：`04555236ae4b83510ddb01ab1aa13709ecf8b7b1`
- **验证者**：Verifier（无前序上下文，全部结论自行重跑）
- **日期**：2026-10-16
- **产物目录**：`.review-workspace/req093qa/`（唯一临时落点）

---

## 1 · 总判定

> **PASS 带非阻断**

**一句话理由**：只读性、端点行为、红线、全量测试四条硬指标全部独立复核通过（61 suite / 833 passed / 0 failed / 1 ignored，与声称逐字吻合）；但**发现 1 个客户可见的正确性缺陷**（`/api/node` 与图节点标识对不上，重名对象解析方向相反）与 **3 处「守卫其实拦不住」的检验力缺口**，必须在客户过目前处理，不影响本轮验收判定。

---

## 2 · 逐条声称核对表

| # | 开发方声称 | 我的实测 | 结论 |
| --- | --- | --- | --- |
| 1 | 单个提交 `d216b7a`，父 `0455523`，历史未动 | `git log --oneline 0455523..HEAD` 恰 1 条；`HEAD^` = `04555236ae4b…` = `0455523` | **吻合** |
| 2 | 未 push | `git remote -v` 输出为空 | **吻合** |
| 3 | 工作区干净 | `git status --short` 空；`git status` = 「无文件要提交，工作区干净」 | **吻合** |
| 4 | 11 个文件（6 改 5 增） | `--stat` 11 files；`--name-status` = 6×`M` + 5×`A` | **吻合** |
| 5 | 8 个红线文件零改动 | 本 commit 的 `tests/` 只动了新增的 `test_graph_server.rs`；7 个具名红线文件 sha256 逐字节相同（见 §6.1） | **吻合**（注：标准红线是 3 个文件 + 「任何测试不许删/ignore」；交付方的「8 条」清单未随交付物落盘，我按可机械核对的等价集合验证） |
| 6 | `61 suite / 833 passed / 0 failed / 1 ignored @ d216b7a` | 逐个 `^test result` 行求和 = **61 / 833 / 0 / 1**；`FULLTEST_EXIT=0` | **吻合（逐字）** |
| 7 | 基线 `60 / 800 / 0 / 1 @ 0455523`，故 +1 suite、+33 passed | 未重跑基线（需另开 worktree，超出授权范围）。但独立对账：静态测试属性 842→875 = **+33**；新增测试文件恰 1 个；`graph.rs` 8 + `http.rs` 16 + `test_graph_server.rs` 9 = **33** | **算术自洽（见 §6.3 的未验证声明）** |
| 8 | 静态/运行时差 **3 条**，定位 `tests/test_guard_manifest_req081.rs` 行 627/636/650，全在字符串字面量内 | 该文件共 **9** 个 `#[test]` 属性，行号 **627 / 636 / 650 / 689 / 903 / 947 / 994 / 1035 / 1077**。`FIXTURE_A` 起于 620（`r##"…"##`）、闭于 643；`FIXTURE_B` 起于 646（`r#"…"#`）、闭于 654 ⇒ 627/636 落在 [620,643]、650 落在 [646,654]，**确在字面量内**。真实测试 = 689/903/947/994/1035/1077 共 **6** 条 | **吻合（独立复核通过）** |
| 9 | `dropped_edges` 已删除，理由「可证恒为 0」 | 我逐行走了 `view.rs:761-828`：`by_key`(769-774) 与 `objects`(776-810) 是**同一个 `entity_types` 迭代器 + 同一个谓词 `usable_as_human_name(&e.display_name, &keys)`**；`relations`(816-817) 两端都 `by_key.get(...)?`，取不到则 `filter_map` 成 `None`（关系根本不会生成）⇒ 每条 `RelationView` 两端必然在 `objects` 里 | **吻合（证明成立，我未能推翻）** |
| 10 | 替代物 `no_edge_is_ever_dropped_for_a_projected_view` 串起 `view::project` 与 `graph::project` | 源码确认是串联（`graph.rs:417-418`）；**变异验证会红**（§5 变异 A） | **吻合，且我实测了检验力** |
| 11 | 零 I/O 判据同时钉两个数：剥注释后 = 0、连注释 = 9 行不增不减 | 独立复算：按行计 **9**（按行×词元计 **11**，与交付方说明一致）；逐文件基线 blob vs 工作区 apply 1 / snapshot 2 / mod 1 / review_view 5，**全等**；`BASELINE_COMMENT_HITS = 9` **等于**我的按行计数 | **吻合** |
| 12 | 91≠90 是「两张源表都翻成客户账号绑定」的去重效果 | 图 91 节点 / 90 个不同 label，重复项恰为 `客户账号绑定`（`attribute_count` 10 与 9）；`ontology overview` 自身也把 `客户账号绑定` 列了 **2 次**，且 (label, 属性数) 多重集与图**完全相等** | **吻合：是去重，不是丢节点** |
| 13 | 线上字段不能叫 `key`（`Node.key` → `id`） | 实测节点键 = `attribute_count/id/label/parent`，**无 `key`** | **吻合** |
| 14 | `--help` 顶格 + `verbatim_doc_comment`，示例逐行可照抄 | 顶格渲染成立；但**正文含一条不成立的陈述**（见 §7-P5） | **部分吻合（有瑕疵）** |

---

## 3 · 端点实测表（8 场景，全部在真实数据 `demo3` 第 2 版上）

前置：`ONTOK_VIEW_TOKEN` 由 `/dev/urandom` 生成 32 字节，存 `token.txt`（权限 600），**值从未出现在任何输出中**。
服务：`./target/debug/ontok serve --project demo3 --version 2 --bind 127.0.0.1:8787`，启动日志 `· 只读图服务就绪：项目 demo3 第 2 版（只读 · 需访问口令）`。
8787 起前确认未被占用；**18812 上有他人进程，全程未触碰**；我方进程已全部停止，无残留。

| # | 场景 | 期望 | 实测 | 结论 |
| --- | --- | --- | --- | --- |
| 1 | 无令牌 `GET /api/graph?project=demo3&version=2` | 401 | `status=401`，正文 `{"error":"没有可用的访问口令，拿不到内容。"}` | ✅ |
| 2 | 错令牌 | 401 | `status=401`，同上正文（大小写变体同样 401）；**正文不含令牌** | ✅ |
| 3 | `POST /api/graph` | 405 | `status=405`，`Allow: GET,HEAD`；PUT/PATCH/DELETE 同为 405；未注册路径 `POST /api/apply` → 405 + `Allow: GET` + 人话正文 | ✅（细节见 §7-P6） |
| 4 | 绑 `0.0.0.0` | 拒绝启动 | `exit=1`，`✗ 只允许绑回环地址（如 127.0.0.1:8787），收到 0.0.0.0。…`；外部 IP `192.168.1.5` 同样拒绝 | ✅ |
| 5 | 未冻结的 `demo3` 第 1 版 | 拒绝启动 | `exit=1`，`✗ 只读图服务拒绝启动：项目「demo3」的第 1 版还没定稿，暂不给人看。定稿之后再来。` | ✅ |
| 6 | 口令短于 8 字符 | 拒绝启动 | `exit=1`，`✗ 访问口令太短（至少 8 个字符）：…`；缺 `ONTOK_VIEW_TOKEN` → `✗ 缺少环境变量 ONTOK_VIEW_TOKEN…`；**恰好 8 字符成功启动**（边界正确） | ✅ |
| 7 | 缺 `project` / `version` | 4xx + 人话 | 缺 project → `400 {"error":"要说明看哪个项目：请带上 project 参数。"}`；缺 version → `400 {"error":"要说明看哪一版：请带上 version 参数。"}`；`version=abc` → `400 {"error":"version 要写一个数字（比如 2）。"}`；指名 v1 / 别的项目 → `400 {"error":"本服务只对外提供 demo3 第 2 版这一份内容，别的一概不给。"}`。**均非空响应、非 500** | ✅ |
| 8 | 正确令牌 + 完整参数 | `{"nodes":…,"edges":…}` | `status=200`，16380 字节；**顶层键恰为 `["edges","nodes"]`**；`nodes=91`、`edges=79` | ✅ |

**形状核对**

- 节点字段 = `["attribute_count","id","label","parent"]` —— 与期望完全一致
- 边字段 = `["count_from","count_to","from","label","to"]` —— 与期望完全一致
- 边的 `label` 去重集合 = **`{"归属于"}`**（全 79 条同值）；建模术语命中 `belongs_to` / `N:1` / `link_type` / `foreign_key` = **0**
- `count_from` / `count_to` 实测如 `1 个` / `多个`

**与登记验收项对账**（`BA/dispatch/req-registry.md:672`「图内容与 `ontok ontology overview` 逐条对得上」）

```
overview 对象行数 = 91      图节点数 = 91
overview 关系声明 = 79     图边数   = 79
按 (label, attribute_count) 多重集是否相等：True
只在 overview：Counter()    只在图：Counter()
```
⇒ **该验收项通过。**

---

## 4 · 只读性：有没有隐藏的写路径

### 4.1 结论：**HTTP 面确实没有写路径**（但守卫拦不住，见 §7-P2）

独立扫描 `src/server/`（不是靠那条测试）：

```
src/server/http.rs:48  use crate::db::project::PgProjectStore;      ← 唯一的存储层 use
src/server/http.rs:49  use crate::db::snapshot_store::PgStore;      ← 唯一的存储层 use
src/server/http.rs:76  pool: &sqlx::PgPool                          ← 类型标注
src/server/http.rs:80  let ps = PgProjectStore::new(pool.clone());
src/server/http.rs:81  let store = PgStore::new(pool.clone());
src/server/http.rs:344 pool: &sqlx::PgPool
src/server/http.rs:364 load_frozen_view(pool, …)
```

两个 store 在 `serve` 全程只被调用 **3 个方法**，全部是读：`get_by_name`(84) / `version_by_seq`(90) / `get_snapshot`(99)。
`PgStore::new` / `PgProjectStore::new` 实现均为 `Self { pool }`，**无副作用**。
路由表（`http.rs:333-341`）只有 `/`、`/api/graph`、`/api/node` 三条 `get(...)` + 一个 `fallback`。**无提案 / 审核 / apply / freeze 任何入口。**

### 4.2 一处**测试结构性看不到**的写路径（⚠ 重要）

`src/cli.rs:703` 对**所有**子命令无条件执行：

```rust
crate::db::pg::migrate(&pool).await?;
```

`migrate()` 会发 DDL（9 张基表 + ALTER）**并跑回填 `UPDATE`**。
⇒ `ontok serve` 这个**进程**在启动时对数据库有写需求；若本体库账号是只读角色，`serve` 会直接起不来。

- **不是本需求引入**：`git diff 0455523..HEAD -- src/cli.rs | grep migrate` 为空，沿用基线。
- **但守卫生效不了**：`server_has_no_write_path_to_the_ontology` 只扫 `src/server/{mod,http,page}.rs`，结构上看不见 `cli.rs`。
- **表述建议**：「只读」应明确为「**HTTP 暴露面只读**」，而非「进程对库无写需求」。

### 4.3 `server_has_no_write_path_to_the_ontology` 的检验力判断

**它是一个 12 个**拼写**的黑名单，不是写路径检测器。** 属于纪律 H 意义上的「看起来测了、其实拦不住」，**不得写成「已封堵」**。

变异实验（§5 变异 B）给出了双向铁证：

| 注入 | 守卫结果 |
| --- | --- |
| B1：`sqlx::query("DELETE FROM snapshots WHERE id = $1")`（大写，词表内） | 🔴 **FAILED** — 证明扫描器非空转、且读的确实是本 worktree 的文件 |
| B2：同一条真实 DELETE，改写成 `sqlx::query_scalar(...)` + 小写 `delete from snapshots` | 🟢 **9 passed 全绿** |

且在 B2 期间，既有守卫 `test_no_write_path`（4 passed）与 `test_read_only_guard`（5 passed）**也全部放行** —— 全仓无第二条网兜住。

**可绕过的写法（均不在 12 词表内）**：`sqlx::query_as` / `query_scalar` / `query!` 宏、`.execute(`、小写/换行拆分的 `delete from`、`UPDATE` 后换行、任意自定义命名的 store 方法。

> **⚠ 纪律 G 的一个反例，必须登记**：B1 与 B2 的**测试二进制 sha256 完全相同**（`b8884eb2…`），因为 `tests/common::read_source` 是**运行时读工作区文件**（`std::fs::read_to_string`），不是编译期嵌入。⇒ 对这类「源码扫描型守卫」，**变异二进制 sha256 根本不构成证据**，能区分的证据是**被扫文件的工作区 sha256** + patch + 逐条结果。本报告 §5 按此给出完整三元组。

---

## 5 · 变异证据三元组（纪律 G）

> 每个变异：① 变异期二进制 sha256 ② patch 的 sha256/内容 ③ 运行条数与逐条结果。
> **所有编译均 `exit=0`（编译失败不算执行）**；每个变异跑完立即 `git checkout --` 还原，并验证源文件与二进制**逐字节回到基线**。
> 基线（干净）：`src/domain/graph.rs` = `ad24904f…`，`src/server/http.rs` = `78e2cbec…`；
> lib 测试二进制 `ontok-2f3e5d415c098efc` = `9c146a04…`；
> `test_graph_server-d255e73d07b448fe` = `c16c6554…`。

### 变异 A —— 静默丢一条边（验证「`no_edge_is_ever_dropped…` 会不会红」）

- **注入点**：`src/domain/graph.rs` 边循环首行，`if !dropped_one { dropped_one = true; continue; }`
- **① 二进制**：`ontok-2f3e5d415c098efc` = `ef18ecd8c245de09722a1debd7c5e881a815224f833e2f2da2f4c59d1c47a76d`
  （≠ 基线 `9c146a04…` ⇒ 变异确实进入了产物）
- **② patch**：`mutation-A.patch`，sha256 = `45d25dae373649a821c008d4d9389d9e844b56120120d23ff3242c1bff88a1d9`
  内容：在 `for r in &v.relations {` 之后插入 6 行 `qa_mutated_dropped_one` 守卫
- **③ 运行**：过滤 `no_edge_is_ever_dropped`，**1 条**，`0 passed; 1 failed; 154 filtered out`，exit=101

```
thread '…::no_edge_is_ever_dropped_for_a_projected_view' panicked at src/domain/graph.rs:425:9:
assertion `left == right` failed: 每一条客户视图里的关系都必须画得出来 —— 少一条就是静默丢边
  left: 0
 right: 1
```

**⇒ 检验力成立。** 且 `left:0 / right:1` 证明该夹具真实产出 1 条关系，**排除了纪律 E 关心的「0 == 0 空转」**。该测试**不是**纪律 H 空转。

### 变异 B —— 写本体路径的两种拼写（验证只读守卫）

- **B1 阳性对照**
  - **①**：`test_graph_server-d255e73d07b448fe` = `b8884eb29f4de42976f197fd19404446930a06a060d20038379825666b38c456`
  - **②** `mutation-B1.patch`，sha256 = `1907742efcc0cca9b6d50605fb746fa370cafa14603bdee6f570707bab2dd0e9`
  - **③** **9 条**，`8 passed; 1 FAILED`，exit=101
    `server_has_no_write_path_to_the_ontology ... FAILED`
    `只读图服务不得有写本体的路径：src/server/http.rs 里出现了 `DELETE FROM``
- **B2 绕过**
  - **①**：`b8884eb29f4de42976f197fd19404446930a06a060d20038379825666b38c456`（**与 B1 相同** —— 见 §4.3 的纪律 G 反例）
  - **②** `mutation-B2.patch`，sha256 = `33d616ed5434f1610121213874856d41321b6ae1417162ee4919fb4a91409eed`；
    **被扫文件工作区 sha256** = `d554d59c4cbaa94c7559ab10bf10c78dbc1e016837e2870fd0b464df6506bb7c`
  - **③** **9 条**，`9 passed; 0 failed`，exit=0
  - 同期：`test_no_write_path` 4 passed、`test_read_only_guard` 5 passed

**⇒ 该守卫能抓到「显而易见的回归写法」，抓不到「换个写法」。应登记为已知限制。**
（注：注入函数为 `#[allow(dead_code)]` 且**未被调用**，全程未对共享测试库执行任何 DELETE —— 红线「禁 DELETE / TRUNCATE」未触碰。）

### 变异 C —— 静默丢一个节点（找「看起来测了、其实没测」）

- **注入点**：`src/domain/graph.rs` 节点循环，跳过 `name == "客户"`（**刻意不写 `.filter(`**，以绕过拼写守卫）
- **①**：`ontok-2f3e5d415c098efc` = `bd9e488eba249353062e8a67023d692e96a3cde5a79266d99181dccb7b91b778`
- **②** `mutation-C.patch`，sha256 = `0237d898f6aa6acfc3c6a370044e18696e4c1a00d85599157dc782ce562a154e`
- **③ 运行与结果**
  - `domain::graph` 全部单测：**8 条，8 passed, 0 failed** 🟢
  - `test_graph_server` 全部：**9 条，9 passed, 0 failed** 🟢
  - **端到端**：`cargo build --bin ontok` = `cec7e1c1…`，起 `127.0.0.1:8801` 后 `GET /api/graph` → **HTTP 200**
    ```
    干净构建 nodes/edges = 91 / 79
    变异构建 nodes/edges = 90 / 42      ← 少了「客户」，边数从 79 掉到 42
    少了哪些节点: {'客户'}
    ```

**⇒ 静默丢节点 + 连坐丢掉 37 条边，服务照样 200、零报错、全部测试照绿。**
这是本次验证**最严重的覆盖缺口**（详见 §7-P3）。

### 还原验证（每个变异后）

| 变异 | 还原后源文件 sha256 | 还原后二进制 sha256 | `git status` |
| --- | --- | --- | --- |
| A | `ad24904f…` ✅ 回到基线 | `9c146a04…` ✅ 回到基线 | 空 ✅ |
| B | `78e2cbec…` ✅ 回到基线 | `c16c6554…` ✅ 回到基线 | 空 ✅ |
| C | `ad24904f…` / `78e2cbec…` ✅ | `9c146a04…` ✅ | 空 ✅ |

**最终交付态未被改动**：交付时 `git status --short` 为空，`HEAD` = `d216b7a`，无 remote。

---

## 6 · 全量测试

### 6.1 命令与口径

```bash
set -a; . /home/myg133/Projects/Anchnet/Ontology/.env; set +a
cargo test --no-fail-fast            # 未加 -q，完整日志落盘
```

口径：逐个 `^test result` 行求和，**不过滤、不去重**；`FULLTEST_EXIT=0`。

### 6.2 绝对数字（**@ `d216b7ac51cc956ad22caf5e8a32dc31cf6268fc`**，源码 0 处改动）

| 指标 | 值 |
| --- | --- |
| suite（`test result` 行数） | **61** |
| passed | **833** |
| failed | **0** |
| ignored | **1** |
| `FAILED` 行数 / `error` 行数 | **0 / 0** |
| `cargo test` 退出码 | **0** |

**那 1 条 ignored** = `tests/test_mcp_server.rs` 的 `todo_identifier_defenses_have_own_assertions`，
`#[ignore = "缺口清单，不是可运行断言"]`。`git show 0455523:tests/test_mcp_server.rs` 第 550-556 行证明它是**基线既有、本次未改**。✔

### 6.3 基线对账

- 交付方基线 `60 suite / 800 passed / 0 failed / 1 ignored @ develop 0455523` —— **绑定了 commit，符合纪律 O**。
- **我没有重跑基线**（需在仓库根另开 worktree，超出「只在该目录内操作」的授权）。改用两条独立佐证：
  1. 静态测试属性：`git grep -E -c '#\[test\]|#\[tokio::test\]'` —— **基线 842 → HEAD 875，差 33**，与运行时 800 → 833 的差 33 **一致**。
  2. 新增测试逐文件点数：`src/domain/graph.rs` **8** + `src/server/http.rs` **16** + `tests/test_graph_server.rs` **9** = **33**；新增测试二进制**恰好 1 个**（`test_graph_server.rs`）⇒ +1 suite。
- ⇒ **+1 suite / +33 passed 的算术完全闭合**，基线数字虽未重跑但被独立佐证。**列为「未直接验证」项**（见 §9）。

### 6.4 那 3 条静态/运行时差异的独立归因 ✅ 成立

```
tests/test_guard_manifest_req081.rs 的全部 #[test] 属性：
  627, 636, 650, 689, 903, 947, 994, 1035, 1077   （共 9 个）

字符串字面量边界：
  FIXTURE_A: 620 `const FIXTURE_A: &str = r##"…`  →  643 `"##;`
  FIXTURE_B: 646 `const FIXTURE_B: &str = r#"…`   →  654 `"#;`

⇒ 627 ∈ [620,643]、636 ∈ [620,643]、650 ∈ [646,654]  ⇒ 三者确在字面量内部
⇒ 真实测试 = 689 / 903 / 947 / 994 / 1035 / 1077 = 6 条
```

该文件被 `tests/test_review_view_req075.rs:80` 以 `include_str!` 当作**被扫描的数据**，其字面量内的 `#[test]` 会被文本扫描器数成「静态测试」但**永不运行** ⇒ 静态 9 − 运行时 6 = **3**。**归因正确。**

### 6.5 既有编译警告（纪律 P：报告但不扩 scope）

5 条，**全部来自本 commit 未触及的既有测试文件**，REQ-093 新增文件产生 **0 条**警告：

| 警告 | 出处 |
| --- | --- |
| `unused variable: report` | `tests/test_acp_extraction.rs:630` |
| `value assigned to o is never read` | `tests/test_agent_rebuild.rs:337` |
| `unused variable: agent` | `tests/test_concept_store.rs:116` |
| `variable does not need to be mutable` | `tests/test_concept_transition.rs:17` |
| `unused import: RelationView` | `tests/test_overview.rs:267` |

---

## 7 · 找夸大（逐条判断）

### P1 · `/api/node` 与图节点标识对不上 —— **推翻「图内容逐条对得上」的边界**（真实缺陷，中等）

- **触发**：真实数据 `demo3` 第 2 版有两张源表都翻成「客户账号绑定」（属性数 10 与 9）。
- **实测**：
  ```
  GET /api/graph   → 节点 id=客户账号绑定   attribute_count=10
  GET /api/node?id=客户账号绑定          → 返回 9 个属性（账号ID、主账号ID、账号渠道编码…）
  GET /api/node?id=客户账号绑定#2        → 404 {"error":"图里没有这个对象，刷新一下页面就好。"}
  ```
- **根因**：`src/server/http.rs:371-375` 用 `BTreeMap<String, EntityView>` 按 **`o.name`** 建索引，`.collect()` 让**后一个重复项覆盖前一个**（→ 解析到**最后**一个）；而 `src/domain/graph.rs:175` 的 `by_name` 用 `.or_insert()` 让引用落到**第一个**（→ 解析到**第一个**）。**两者方向相反。**
- **用户可见影响**：前端 `page.rs:216-231` 把图的 `data-id` 原样传给 `/api/node`。于是悬浮提示读图的 `attribute_count` 显示「10 项属性」，点开面板读 `/api/node` 的 `properties.length` 显示「9 项属性」—— **同一节点两个数字打架**；而 `客户账号绑定#2` 点开直接报错。这正是「客户过目」场景最忌讳的事。
- **注意**：图侧的重名行为**已作为 L2 诚实登记**（`graph.rs:228-237`），但登记的是「引用落到第一个」，**没有登记 `/api/node` 落到最后一个**这一侧。

### P2 · `server_has_no_write_path_to_the_ontology` —— **纪律 H 空转，不能算「已封堵」**

变异 B 已给出双向铁证（§4.3 / §5）。它是**拼写黑名单**，不是写路径检测器。

**但要说清楚**：**「HTTP 面没有写路径」这个结论本身是真的** —— 我是靠独立读代码（§4.1）得出的，不靠这条测试。所以这是**守卫强度不足**，不是**产品有洞**。

### P3 · 静默丢节点完全无人兜底 —— **最严重的覆盖缺口**

变异 C 证明：丢掉「客户」后边数 **79 → 42**，服务照回 200，`domain::graph` 8 条与 `test_graph_server` 9 条**全部照绿**。

- 边侧有 `no_edge_is_ever_dropped_for_a_projected_view` 兜底；
- **节点侧唯一的守卫 `graph_projection_does_not_filter` 是个拼写黑名单**（只查 `usable_as_human_name` / `.filter(` / `scan_modeling_terms` 三个字符串）。一个 `if … { continue; }`、`.take(n)`、`.skip(n)` 都能绕过。
- ⇒ 「图 = 客户视图，不多不少」这个不变量**只在边上被钉住，在节点上没有**。
- ⚠️ 注意这**不是** `dropped_edges` 的锅（那个计数器本来也只数边），删除它的推理仍然成立；**问题在于删除后没有补上节点侧的对等守卫。**

### P4 · `dropped_edges`「可证恒为 0」—— **✅ 证明成立，未能推翻**

见 §2 第 9 行的完整走查。要补充的一点：`graph::project` 的 `by_name` 是**按名字**建的，而 `CustomerView` 里重名对象的引用本来就只能按名字对齐（L2），因此「重名」只导致**别名**、**不导致丢边** —— 这一点也独立成立。

### P5 · `--help` 里有一条**不成立**的陈述

原文（`src/cli.rs:76` 起的 `verbatim_doc_comment` 块内）：

> ⚠️ **三个参数缺一不可，少了会安静地什么都拿不到**（不报错，只是空）。

实测 `ontok serve --version 2`（缺 `--project`）：

```
exit=2
error: the following required arguments were not provided:
  --project <PROJECT>
```

⇒ 是 **clap 直接报错退出**，**不是**「不报错，只是空」。而且**同一份 help 的下一段又自相矛盾**地说「两个查询参数漏了会得到一句人话 400，不是空响应」。**用户照抄会被误导。**

「顶格渲染 + `verbatim_doc_comment` + 示例可照抄」这三点**成立**（我实测了输出）。

### P6 · 已注册路径的 405 正文是**空的**

`POST /api/graph` → 405、`Allow: GET,HEAD`，但**正文 0 字节**（axum 内建 405）。而未注册路径的 405 有完整人话正文（`这里只读：只能看…`）。
⇒ 两条 405 路径**文案不一致**。不违反任何验收项，仅记为体验不一致。

### P7 · `domain/mod.rs:3` 的验收判据本身是**假的**

```
//! 验收 AC-002：`grep -rn 'sqlx\|PgStore\|db::' src/domain/` 必须无输出。
```

实测 **9 处命中**（基线 9、本次 9）。**本 commit 改过 `src/domain/mod.rs`（+4 行，加 `pub mod graph;`），却没修正这一句。**

- 开发方**并非没意识到**：`graph.rs:19-23` 与测试注释都明确写了「基线 0455523 实测 9 处命中，AC-005 早就不成立」。
- 但**仓库里那份「权威判据」文本仍是错的**，读者只看 `mod.rs` 会被误导。
- 那 9 处注释**本身不误导**（都是准确的说明或 intra-doc 链接），误导的是 `mod.rs:3` 这句**声称判据**。

### P8 · 零 I/O 判据是否真等于「零 I/O」—— **实质成立，但判据仍是词表**

我做了两件事：

1. **反向自检（纪律 E）**：`grep -rn 'sqlx\|PgStore\|db::' src/domain/ | grep -vE '^\s*//'` → **0 行**；9 处命中**全部**是 `//!` / `///` 文档注释。且我拿 `std::fs::read_to_string("a")` 这类已知含词的非注释行验证过过滤器本身有效 ⇒ **零是真的零，不是扫描器空转**。
2. **词表外抽查**：`std::fs` / `tokio::fs` / `File::open` / `read_to_string` / `std::net` / `TcpStream` / `reqwest` / `ureq` / `std::env` / `process::Command` 在 `src/domain/` → **零命中**（同样做了反向自检）。

⇒ **`domain/` 确实零 I/O**，判据偏弱但结论为真。`domain_layer_stays_io_free` 同时钉「剥注释后 0」+「连注释 9 行不增不减」的双数设计是**扎实**的，基线常量 9 与我的独立按行计数一致 ✅。

### P9 · `parent` 字段在真实数据上**从未被触发**

图里 91 个节点的 `parent` **全为 null**。追到底：`ontology show --model-view` 显示 `demo3` 第 2 版 96 个 entity_type **无一带 parent**，`view.rs:783` 的 `belongs_to` 因此恒为 `None`。

⇒ 投影实现**是对的**（数据本来如此），但 `Node.parent` 这条链路（`graph.rs:184-187` 的第二趟回填）**只被合成夹具覆盖，从未在真实数据上跑过**。属**已知限制**，不是缺陷。

### P10 · 其他「看起来测了、其实没测」

| 测试 | 真实检验力 |
| --- | --- |
| `server_only_reads_three_snapshot_methods` | **只有正向**：断言三个方法名「存在」。**没有任何否定断言**（不检查是否用了别的 store 方法）。改用 `store.delete_version(...)` 它照样绿。价值仅限「重构掉这三个读会红」。 |
| `graph_projection_does_not_filter` | 拼写黑名单，**变异 C 已证伪**。 |
| `the_page_pulls_in_nothing_from_outside` | 查 `http://`/`https://`/`//cdn`/`cdn.`/`integrity=`。协议相对外链 `//evil.com/x.js` 可绕过。 |
| `red_line_files_are_still_present_and_readable` | **自认弱**（「本文件证明不了『没被改动』」），文档诚实，不作数。真正的零触碰证据是我在 §6.1 给的 sha256。 |
| `the_stripper_is_not_vacuous` / `the_guard_scanner_is_not_vacuous` / `the_guard_scanner_is_not_vacuous_positive_control` | ✅ **真正有价值**，是纪律 E 的正面落实，建议保留。 |
| `without_token_*` / `change_verbs_*` / `unauthenticated_probes_cannot_discover_which_paths_exist` 等行为测试 | ✅ **强**（真实 `Router::oneshot`，状态码逐条断言）。 |

---

## 8 · 我推翻的东西

| 推翻对象 | 内容 | 证据 |
| --- | --- | --- |
| **需求方/开发方的隐含前提** | 「图内容与 `ontology overview` 逐条对得上」**不足以保证客户过目通过** | §3 对账显示 (label, 属性数) 多重集与 overview **完全相等**，但 `/api/node` 点开返回**另一张表**的属性（§7-P1），悬浮提示与面板数字打架 |
| **开发方的表述（部分）** | 「零 I/O 判据 = 零 I/O」在**判据层面**偏弱 | 判据是 3 词表；我用 10 个词表外手段抽查证明结论为真（§7-P8），但判据本身可被 `std::fs` 之类绕过 |
| **开发方的表述（部分）** | `--help` 「示例逐行可照抄」隐含「文档准确」 | §7-P5：`--help` 正文含一条**实测为假**的陈述，且同文自相矛盾 |
| **未推翻的（明确记录）** | `dropped_edges` 恒为 0 的证明 | §7-P4，我独立走查后**成立** |
| **未推翻的** | 3 条静态/运行时差异的归因 | §6.4，独立复核**成立** |
| **未推翻的** | 61/833/0/1 的全量数字 | §6.2，逐字吻合 |

---

## 9 · 未验证 / 受环境边界限制的部分

1. **基线 `60 / 800 / 0 / 1 @ 0455523` 未重跑**。需在仓库根另建 worktree（`git worktree add`），超出「⛔ 只在该目录内操作」授权。**已用两条独立佐证闭合**（静态属性 842→875 = +33；新增测试二进制恰 1 个），但**不是直接实测**。
2. **`--help` 示例未按字面执行**（会真的起服务）。已用 clap 的 `--project` 缺失报错（exit=2）验证了 §7-P5 的事实，但未逐行跑通整段复制粘贴。
3. **`graph.rs` / `http.rs` 的 `#[cfg(test)]` 单测未逐条人工审阅全部断言强度**，只对变异 A/B/C 涉及的三条做了实证。
4. **性能 / 并发**（91 节点力导向布局、大快照下 axum 行为）未测，不在本需求验收范围。

---

## 10 · 建议（供 BA 决策，我未改任何代码）

| 优先级 | 事项 | 依据 |
| --- | --- | --- |
| **P1（客户过目前必修）** | 让 `/api/node` 与图节点标识同源：`AppState.nodes` 改按**图 id** 建索引并采用**首个同名**语义，与 `graph.rs` 的 `by_name` 对齐 | §7-P1 |
| **P2（登记为已知限制）** | `server_has_no_write_path_to_the_ontology` 在注释里明确降级为「拼写黑名单，非写路径证明」；真正的只读证据是路由表 + 三个只读方法调用 | §4.3 / §7-P2 |
| **P2（补测试）** | 为**节点侧**补一条与边侧对等的不变量测试（`nodes.len() == view.objects.len()` 且 id 集合一致），或在真数据上断言 91/79 | §7-P3 |
| **P3（改文案）** | 修正 `src/domain/mod.rs:3` 的失效判据；修正 `--help` 中「不报错，只是空」的错误陈述 | §7-P7 / §7-P5 |
| **P3（措辞）** | 把「只读」的表述限定为「HTTP 暴露面只读」，并登记 `cli.rs:703 migrate` 使 `serve` 进程仍需库写权限 | §4.2 |
| **P3（登记）** | 把 `Node.parent` 在真实数据上从未触发登记为已知限制 | §7-P9 |

---

## 附：本次验证的证据清单

全部落在 `.review-workspace/req093qa/`：`fulltest.log`（全量）、`mutation-{A,B1,B2,C}.patch`（变异补丁）、
`{base,mutA,mutB1,mutB2,mutC,revertA}-*.sha256`（哈希）、`graph.json` / `graphC.json`（服务实测图）、
`overview.txt` / `mv.txt`（对账基准）、`s1..s7f.txt` / `s3c.txt`（端点响应）、`serve-help.txt`、`serve*.log`。

**工作区最终状态**：`git status --short` 空，`HEAD = d216b7a`，无 remote，我方服务进程无残留。
