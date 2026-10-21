# REQ-115 · 概念分组与图上的归并 —— 验证报告

> **被测**：worktree `feature-REQ-115` / 分支 `feature/REQ-115` · 基线 `develop` `71ae770` · **未 push**
> **范围**：`/api/graph` 的 `groups` + 节点 `group`、页面的组圈/折叠/组面板/组内引力、
> `ontok concept suggest`（确定性、不写库）、真实库往返、H1 golden 重锚。
> **不碰**：`BA/` / `code/` / `Deploy/`；四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4；
> `/api` 的鉴权与路由语义。
> **日期**：2026-10-22 · Windows · PG `postgresql://***@172.17.0.87:5432/anspire_real`
> **历史报告**：REQ-113 / REQ-114 两轮原文见 `git log -p -- .feature/verification-report.md`（本文件按需求轮次整体替换）。

---

## ⚠️ 三条必须先读的结论

1. **AC-001 的字面字段 `groups[].key` 在线上叫 `id`**。`key` 在发布闸的
   `MODELING_TERMS` 词表里，而闸扫的是**整段序列化文本**（键名也扫）—— 写成线上键名
   会让**每一次带分组的 `/api/graph` 直接 500**。这不是命名偏好：`Node` 早就因为
   同一条纪律把 `key` 序列化成 `id`。本报告 §2.3 有正反两条断言把这条裁定钉住。
   ⇒ **这是 AC 与红线冲突处的取舍，也是最可能被 QA 打回的一点**（见 §8）。
2. **H2 及其余 11 份 golden 逐字节未动**：分组接在**另一个装配入口**
   （`state_from_view_with_groups`），H2 那份 golden 钉的是旧形状
   （`state_from_view`：`groups` 缺席、节点无 `group`）。两种形状由
   `GraphProjection::groups: Option<Vec<_>>` 的三态表达。
3. **未做真浏览器手测**（本机无 headless 浏览器，也不为此加依赖）。本报告所有「✅」
   都指源码级 / 纯函数级（Node 探针）/ 真 HTTP 级判据成立，**不指**折叠动画与组圈观感已验证。

---

## 0 · 结论先行

| AC | 判据 | 实测 |
| --- | --- | --- |
| 001 | `/api/graph` 返回 `groups`，节点 `group` 等于某组 `id` 或 `null` | ✅ 真库 9 组 / 83 节点；`bad=0`；无组项目 `groups:[]` + 83 个 `null` |
| 002 | 组圈 + 折叠（组名 + N）+ 组面板（成员 / 点击定位） | ✅ 源码落点 + Node 探针：折叠态 **7 = 3 组 + 4 未分组** |
| 003 | 无组时如实显示「未分组」，⛔ 无默认组 | ✅ 探针：`groups=[]` ⇒ 渲染单元 = 对象数、**零聚合单元** |
| 004 | `concept suggest` 两次逐字节相同 + 不写库 | ✅ 5 次运行同一 sha256；库指纹前后相同（§4.3） |
| 005 | suggest → import → 双轨签 → apply → 图上出现 | ✅ 真库全链路跑通，`groups=9`（§5） |
| 006 | 页面不许自己算分组 | ✅ 页面只读 `DATA.groups`；剥注释后无任何分组算法词（探针 + Rust 判据） |
| 007 | 零外部依赖 / `GET /` 零数据壳 / `/api/*` 401 | ✅ 页面 49728 字节、< 80KB；无 token → 401；`/` 200 且 0 个对象名 |
| 008 | H1 重锚 + SHA256SUMS + PROVENANCE；其余 12 份不动 | ✅ `test_req107_pipeline_bytes` + `zero_visible_change` 全绿；`git diff --stat` 只碰 H1 |
| 009 | 折叠前后 `ontology show` / TTL 逐字节相同 | ✅ sha256 两两相同（§6） |
| 010 | 声明里 `source: "algorithm-suggest"`、`reviewed_by: null` | ✅ 文件字段 + 单测 |
| 011 | 不退化（禁改 / 禁删测试 / 无新依赖 / 基线数字不减） | ✅ 见 §7 |

---

## 1 · 改动清单

| 文件 | 落点 | 改了什么 |
| --- | --- | --- |
| `src/domain/graph.rs:69` | `GraphProjection.groups: Option<Vec<Group>>`（`:86`） | 三态：`None`=通道未接通（H2 旧形状）/ `Some([])`=接通但没有组 / `Some([...])` |
| `src/domain/graph.rs:109` | `pub struct Group` | 线上 `id` + `display_name` + `members`（节点标识）+ 可选 `scope_hint` |
| `src/domain/graph.rs:133` | `pub struct DeclaredGroup` | 中间形状：成员 = 展示名（声明里是实体 key） |
| `src/domain/graph.rs:184` | `Node.group: Option<Option<String>>` | 外层 `None`=字段缺席（旧格式）/ `Some(None)`=`null`（无组）/ `Some(Some(id))` |
| `src/domain/graph.rs:357` | `pub fn declared_groups(&Snapshot)` | 实体 key → 展示名（只查快照，不判可见性） |
| `src/domain/graph.rs:404` | `pub fn project_grouped(&CustomerView, &[DeclaredGroup])` | 展示名 → 节点标识（只查表）；单归属先到先得 |
| `src/server/http.rs:122` | `load_frozen_view` | 同时返回**同一份快照**里的概念组（载入期冻结，运行期不查库） |
| `src/server/http.rs:650` | `state_from_view_with_groups` | **生产入口**（`serve` 在 `:763` 调它） |
| `src/server/http.rs:662` | `state_from_view_inner` | 两条入口共用的唯一实现（只差一行投影函数） |
| `src/server/page.rs:300` | `GRP_F` / `GRP_PAD` / `GRP_MIN_R` | 组心引力、组圈留白、最小半径 |
| `src/server/page.rs:362` | `physicsStep` 里组心引力 | 同一组节点朝组心额外受力（**不判谁和谁一组**） |
| `src/server/page.rs:418` | `unitsOf` / `:433` `renderedCount` | 折叠态「渲染单元」—— 探针原样抽取的纯函数 |
| `src/server/page.rs:983` | `relayout` | 折叠=聚合节点 / 展开=成员 + 组圈；位置记忆 |
| `src/server/page.rs:1076` | `setFold` / `:1089` `foldAll` | 折叠/展开（**纯前端状态**，无网络调用） |
| `src/server/page.rs:1139` | `showGroups` / `:1167` `showGroup` / `:1182` `locateMember` | 组面板：成员列表、点击定位、折叠按钮 |
| `src/server/page.rs:1105` | `updateGroupInfo` | 组数 / 未分组数 / 当前画了几个节点（`#status`） |
| `src/cli.rs:161` | `ConceptCmd::Suggest` | `--project --version [--out]` |
| `src/cli.rs:1305` | `concept_suggest` | 只读快照 → 声明文件；`--out` 写盘 / 省略打 stdout |
| `src/domain/concept_suggest.rs:110` | `pub fn suggest` | 贪心模块度（整数打分、有序迭代、平局取小） |
| `src/domain/concept_suggest.rs:249` | `fn communities` | 确定性社区检测本体 |
| `src/domain/concept.rs:372` / `:379` | `source` / `reviewed_by` | AC-010 的来源可辨识（**不进 `content_digest`**） |
| `tests/test_req115_groups.rs` | 新增 12 条 | HTTP 形状 / 无组 / 旧形状兼容 / 页面源码 / 折叠无网络 / 401 |
| `tests/fixtures/req115/group_probe.js` | 新增探针 | 折叠态节点数 7=3+4；无组零聚合；页面无分组算法词 |
| `tests/fixtures/req107/golden/H1_index.html` + `SHA256SUMS` + `PROVENANCE.txt` | 重锚 | `d8bd4581…`（32396 字节）→ `e665a0ad…`（49728 字节） |

`git diff --stat`（产品文件）：

```text
 src/cli.rs              | 129 +++++++++-
 src/domain/concept.rs   |  19 ++
 src/domain/graph.rs     | 201 +++++++++++++++
 src/domain/mod.rs       |   5 +
 src/server/http.rs      |  88 +++++++-
 src/server/page.rs      | 465 ++++++++++++++++++++++++++++++++++++++++---
```

**红线零触碰**：`git diff --stat` 里没有 `src/domain/view.rs`、`src/domain/review_view.rs`、
`src/extract/*`、`src/mcp/*`、`src/domain/property_filter.rs`、`Cargo.toml` / `Cargo.lock`。

---

## 2 · `/api/graph` 的实际 JSON（真库，非合成）

### 2.1 有组 —— `anspire_excl_r115` v2（往返后冻结的那一版）

```text
nodes=83 edges=52 groups=9   covered-nodes=45   ungrouped=38
```

```json
"groups":[
 {"display_name":"后台管理员","id":"concept_01","members":["角色分配","后台管理员"],
  "scope_hint":"算法建议：按关系拓扑自动分组，待人审"},
 {"display_name":"客户","id":"concept_02","members":["智能体白名单","余额操作流水","客户余额", …共 19 个],
  "scope_hint":"算法建议：按关系拓扑自动分组，待人审"}
]
"nodes":[{"attribute_count":10,"group":null,"id":"活动","label":"活动","parent":null},
         {"attribute_count":3,"group":"concept_01","id":"角色分配","label":"角色分配","parent":null}]
```

一致性断言（真 HTTP）：`node.group` ∈ {`null`, `groups[].id`} ⇒ **`bad=0`**。

### 2.2 无组 —— `anspire_excl` v2（这版**真的**没有概念组）

```text
nodes=83 groups=0 groups-is-array=True   node group null count = 83 / 83
```

```json
"groups":[],"nodes":[{"attribute_count":10,"group":null,"id":"活动","label":"活动","parent":null}]
```

### 2.3 为什么线上字段是 `id` 而不是 AC 里写的 `key`

`tests/test_req115_groups.rs::why_id_not_key_the_guard_rejects_a_literal_key_field` 正反两面：

- `scan_modeling_terms(r#"{"groups":[{"key":"g1"}]}"#)` ⇒ **非空**（会被闸拒发，HTTP 500）；
- 线上真实形状 `{"groups":[{"id":"g1", …}]}` ⇒ **空**（可发布）。

另有一条真 HTTP 断言：带分组的响应体里不得出现字面键 `"key"`（实测 `raw contains "key": False`）。

---

## 3 · 页面：组圈 / 折叠 / 组面板 / 组内引力

### 3.1 落点

| 功能 | 位置 |
| --- | --- |
| 组圈（背景）+ 组名 | `buildScene` 建 `g.gh`；`paint` 每帧按成员位置算圆心/半径；CSS `#stage svg g.gh` |
| 折叠 / 展开 | `FOLD` + `unitsOf` + `setFold` + `foldAll` + `relayout`；标题栏按钮 `#foldall` / `#unfoldall` |
| 聚合节点（组名 + N） | `relayout` 里 `label = display_name + "（" + members.length + "）"`，id = `@g:<组 id>`；`bindNode` 点它展开 |
| 组面板 | `showGroups`（每组 + 成员 + 折叠按钮）/ `showGroup` / `locateMember` / `wirePanel` |
| 组内聚拢引力 | `physicsStep`：按 `a.gid` 求组心，朝组心加 `GRP_F` 加速度 |
| 无组如实 | `updateGroupInfo` / `showGroups`：`GROUPS.length === 0` ⇒ 「未分组」+ 一句话说明 |

默认**全部折叠**（需求风险表第 3 条）：进页面先看到 N 个组节点 + 未分组对象，点开才展开。

### 3.2 折叠态节点数断言（AC-002 的数据级判据）

判据**入库、可复跑**：`node tests/fixtures/req115/group_probe.js`（失败 exit 1）。
它从 `page.rs` **原样抽出**分组内核段（`unitsOf` / `renderedCount` / `groupOf` / `groupIndex`），
不复制代码。实测：

```text
PASS 折叠态渲染节点数 = 组数 3 + 未分组 4 = 7  实际 7
PASS 折叠态聚合单元数 = 组数  实际 3
PASS 折叠态单元 = 每组一个聚合节点 + 每个未分组对象一个节点  g1,g2,g3,u1,u2,u3,u4
PASS 展开态渲染节点数 = 对象数 10  实际 10
PASS 无组时渲染节点数 = 对象数（没有默认组）  实际 10
PASS 无组时没有任何聚合单元（⛔ 不许出现默认组）
PASS 指向未知组 id 的节点按「未分组」处理（不虚构一个组）
PASS unitsOf 不改入参（折叠只是换一种画法）
PASS 页面 JS（剥注释后）不含分组算法词：louvain / modularity / cluster / community / 聚类 / 贪心 / merge
PASS 页面里有 setFold / foldAll / relayout（折叠/展开的实现位置）
PASS setFold / foldAll / relayout 的函数体里没有网络调用（AC-009）
ALL PASS（折叠态 7 = 组 3 + 未分组 4）
exit=0
```

REQ-114 的布局探针也复跑通过（组心引力对没有 `gid` 的节点整段跳过）：

```text
ALL PASS（场景 1：168 帧 / 余量 15.79px；场景 2：168 帧 / 余量 5.20px）
```

### 3.3 页面不算分组（AC-006）

- 唯一来源：`GROUPS = DATA.groups || []`（`page.rs:960`）；
- `unitsOf` 只回答「一个**已分好的**组折叠时是 1 个单元还是 N 个」，没有邻接/相似度/合并；
- 两条判据：Node 探针（剥注释后 7 个算法词全 0）+ Rust
  `the_page_consumes_groups_and_contains_no_grouping_algorithm`。

---

## 4 · `concept suggest`

### 4.1 算法与「为什么确定性」

算法 = **贪心模块度**（Louvain 的第一相，不做聚合）。每条关系算无向边（权重 = 该对实体的关系条数），
自环单独记；初始一人一社区，反复扫「让模块度增益最大的邻居社区」，没人再动就停
（上限 50 轮，防病态输入）。

确定性的**三个结构原因**（没有随机数、没有时钟、没有哈希表遍历序）：

1. 实体先按 **key 排序**，关系两端都查同一个索引 ⇒ **输入顺序不影响结果**；
2. 迭代顺序 = 排序后的下标顺序（`0..n`），社区遍历用 `BTreeMap`（有序）；
3. 打分**全整数**：`score(d) = W(i,d)·M − k_i·K_d`（与 ΔQ 同号，M=2m），
   平局取更小的社区号 ⇒ 不存在浮点比较的「两次运行可能不同」。

产物是**完整声明**（含 `history` 的内容指纹），直接 `concept import`；
`declared_by = concept-suggest/greedy-modularity-v1`、`source = algorithm-suggest`、
`confirmed_by = null`、`reviewed_by = null`。单成员社区**不成组**（那是换个名字，不是归并），
所以 84 个对象里只有 45 个进了 9 个组，其余 38 个如实「未分组」。

### 4.2 两次运行 sha256（`anspire_excl` v2，真库）

```text
已写出建议声明 a.json（项目 anspire_excl · 基于 v2 · 9 个组 / 84 个对象 · 未写库）
已写出建议声明 b.json（同上）
已写出建议声明 c.json / d.json / e.json（另三次）
sha256(a) = dff0e6eb610ea56f5fb3f713b49d06fe59e8c182a171b6489c20a62082671241
sha256(b) = dff0e6eb610ea56f5fb3f713b49d06fe59e8c182a171b6489c20a62082671241
sha256(c) = dff0e6eb610ea56f5fb3f713b49d06fe59e8c182a171b6489c20a62082671241
sha256(d) = dff0e6eb610ea56f5fb3f713b49d06fe59e8c182a171b6489c20a62082671241
sha256(e) = dff0e6eb610ea56f5fb3f713b49d06fe59e8c182a171b6489c20a62082671241
```

### 4.3 不写库（AC-004 后半）

判据：把 `concept_groups` / `ontology_versions` / `proposals` / `proposal_items`
按规范序拼成一行文本算 FNV-1a 64，在**两次 suggest 之前/之后**各取一次：

```text
before: concept_groups=37 ontology_versions=196 proposals=77 proposal_items=477 hash=b4ad1c172ad37f28
after : concept_groups=37 ontology_versions=196 proposals=77 proposal_items=477 hash=b4ad1c172ad37f28
```

（`ontology_versions` 从 195 变 196 的那一次是**准备往返用的快照副本**，发生在这次对照之前，
不是 suggest 写的 —— 上面的 before/after 是同一基线。）

---

## 5 · 往返实测（AC-005，真库）

```text
# 0) 准备：anspire_excl_r115 v1 = anspire_excl v2 的快照副本（未冻结 ⇒ 可作 base）
SETUP project=anspire_excl_r115 v1_id=894716f6-… v1_frozen=false entities=84 links=52 groups=0

# 1) suggest
ontok concept suggest --project anspire_excl --version 2 --out d.json
  已写出建议声明 d.json（项目 anspire_excl · 基于 v2 · 9 个组 / 84 个对象 · 未写库）

# 2) import（双轨审核通道，不直接生效）
ontok concept import --file d.json --project anspire_excl_r115 --base-version 1
  概念声明 v1.0.0 已导入提案 p1（项目 anspire_excl_r115 · 基于 v1）—— 共 9 个概念组，待双轨审核
  [1] add_concept_group 后台管理员 … [9] add_concept_group 操作流水

# 3) 双轨签
ontok review decide --proposal p1 --accept-all --project anspire_excl_r115
  提案 p1 的 9 个条目 × 2 轨 → accepted

# 4) apply
ontok proposal apply --proposal p1 --project anspire_excl_r115
  提案已应用 → v2（id=481056a3-5445-4f85-af27-1ba62a9d8335）

# 5) 冻结（图服务只服务冻结版）
ontok version freeze --project anspire_excl_r115 --version 2 → 已冻结 v2

# 6) 起只读服务 + 取图
GET /api/graph?project=anspire_excl_r115&version=2  (Bearer)
  nodes=83 edges=52 groups=9
  group id=concept_01 name=后台管理员 members=2
  group id=concept_02 name=客户 members=19
  … concept_09 name=操作流水 members=2
  bad=0（每个 node.group 都是 null 或某个 groups[].id）
```

> 84 个实体 → 图上 83 个节点：有 1 个对象的展示名退化成了 key，被客户视图挡掉（REQ-093 既有行为）。
> 这也是「组成员只算图里看得见的」那条已知限制的现场例证。

---

## 6 · 折叠不改数据（AC-009）

折叠是纯前端内存状态（`FOLD`），页面里 `setFold` / `foldAll` / `relayout` 的函数体
**没有** `fetch(` / `ask(` / `api/`（探针断言）。在同一版上前后各导出一次：

```text
ttl1.ttl  ACA259D906FA3819E847F313A8C0427DA3D3D6C7182BB6D287B67BBC2EDA6CB9
ttl2.ttl  ACA259D906FA3819E847F313A8C0427DA3D3D6C7182BB6D287B67BBC2EDA6CB9
show1.txt FD2A916A568960B3E5A834CE28A98A172BFA7DE5CC74D55B98AE0F504E09FDF7
show2.txt FD2A916A568960B3E5A834CE28A98A172BFA7DE5CC74D55B98AE0F504E09FDF7
ttl identical = True
show identical = True
```

（两次导出之间跑了 `node tests/fixtures/req115/group_probe.js` —— 它执行的就是页面里那份折叠内核。）

---

## 7 · 测试与绕行

### 7.1 数字

```text
lib   284 passed / 0 failed   （基线 272 + 12：concept_suggest 的确定性/单成员/键冲突/声明来源等）
doc     4 passed / 0 failed
集成   74 个目标跑完 · 799 passed / 3 failed（3 条全部是 §7.3 的既有红灯）
       另有 1 个目标（tests/test_retained_names_req084.rs）因 Windows 绕行未编译（§7.2）
       新增：tests/test_req115_groups.rs —— 12 passed / 0 failed
```

逐项：

| 套件 | 结果 |
| --- | --- |
| `test_req115_groups`（新增 12 条） | 12 passed |
| `test_req107_pipeline_bytes`（H1 重锚 + 其余 12 份 golden） | 1 passed |
| `test_req107_zero_visible_change`（SHA256SUMS 互校 + 逐份比对） | 3 passed |
| `test_req107_source_enumeration`（写点/函数/模块登记） | 10 passed |
| `test_graph_server`（页面守门 / 零外链 / 域层零 I/O） | 9 passed |
| `test_req114` 探针（Node，非 cargo） | `ALL PASS`（§3.2） |
| `test_req115` 分组探针（Node，非 cargo） | `ALL PASS`（§3.2） |

> 口径：`lib 272 → 284` 只增不减；`doc 4` 不变；集成目标 `75 → 74 跑 + 1 绕行`（新增 1 个套件，
> 绕行的那个仍是 REQ-113/114 登记过的 `test_retained_names_req084`）。没有任何一条既有用例被删/被
> `#[ignore]`/被放宽。

### 7.1.1 最终一轮的真实输出（尾部）

```text
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
（…共 74 个 target 的 test result…）
error: 3 targets failed:
    `--test drop_rate`
    `--test test_agent_orchestration`
    `--test test_fk_naming`
```

---

### 7.2 Windows 绕行双证

`src/bin/ontok-gate-preflight.rs` 是 unix-only（`std::os::unix::fs::MetadataExt`），
Windows 上编不过。按验收方式挪开，**finally 挪回**：

```text
sha256(before) = c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d
src/bin/ontok-gate-preflight.rs  Test-Path = False   （测试期间）
…
src/bin/ontok-gate-preflight.rs  Test-Path = True    （跑完挪回）
sha256(after)  = c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d
```

`git status --short` 无 `D` 行。代价：`tests/test_retained_names_req084.rs` 用
`include_str!` 引那份源文件 ⇒ **该套件编不过**（与 REQ-113/114 同一口径，既有红灯）。

### 7.3 既有红灯（与本需求无关，均在基线复现）

```text
drop_rate            → 缺 .review-workspace/fix-props/schema.json（fail-closed 门禁；输入不在版本控制内）
test_fk_naming       → 同一份脱敏 schema 缺失
test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract
                     → REQ-108 分档路由带来的既有 CLI 端到端差异（REQ-112/113 已登记）
test_retained_names_req084
                     → Windows 绕行导致编不过（同上，既有）
```

---

## 8 · 已知限制 & 最可能被 QA 打回的一点

### 8.1 最可能被打回：`groups[].key` → 线上 `id`

AC-001 字面写 `groups: [{key, display_name, members[], reason?}]`，而交付的是 `id`。
这是**红线（发布闸）与 AC 的正面冲突**：字面 `"key"` 是 `MODELING_TERMS` 里的词，
写成线上键名会让每次带分组的 `/api/graph` 被闸拒发（500）。可选做法只有三条：

1. 改词表/分词 —— ⛔ 四红线之一，禁；
2. 线上叫 `key` 并接受 500 —— 功能直接不可用；
3. 线上叫 `id`（与 `Node` 同一先例），并在判据里钉住「为什么不能叫 key」—— **本轮选择**。

若 QA 认为 AC 必须逐字，请裁定：要么改 AC（把 `key` 写成 `id`），要么放开 `guard()`
（那是另一条需求）。**代码侧没有第三条路**。

### 8.2 其余已知限制（都已写进 `page.rs` / `graph.rs` 模块文档）

1. **组成员只算图里看得见的**：声明按实体 key 写成员，客户视图可能挡掉某个对象
   （展示名退化成 key）。这类成员不进 `groups[].members`，页面的 N 与点得到的对象永远一致，
   代价是「声明了但看不见」的那几个不参与计数（真库现场：84 实体 → 83 节点）。
2. **同名对象只能落到第一个**（继承 REQ-093 的 L2）：`客户账号绑定#2` 不会被分进组。
3. **未做真浏览器手测**：组圈观感、折叠动画、组内引力手感都只有源码级 + Node 探针。
4. **`scope_hint` 是算法写的固定一句话**（「算法建议：按关系拓扑自动分组，待人审」），
   不做 AI 命名（需求「不做」列表里明说）。
