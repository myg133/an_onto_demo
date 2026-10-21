# REQ-115 · 概念分组与图上的归并 —— QA Pre-merge 审核

> **被测**：`feature-REQ-115` @ `7718061`（分支 `feature/REQ-115`）
> **基线**：`develop` @ `71ae770` · **未改 feature 分支、未 push**
> **独立变异 worktree**：`qa-REQ-115`（`qa/REQ-115`，从 `7718061` 建，审核完已移除）
> **环境**：Windows · PostgreSQL 16.15 · `postgresql://postgres:***@172.17.0.87:5432/anspire_real`
> **日期**：2026-10-22

## 1 · 结论

**FAIL（1 项阻断）**。

原因只有一条：**用任务指定的 AI 8 组声明走完 import → 双轨签 → apply → freeze 后，`GET /api/graph` 返回 500**，图上 8 个组一个都看不到。声明本身已经成功落库（8 组 / 84 成员），但 `api_entitlement.scope_hint` 里的「未持 Key」触发了发布闸的 `"key"` 词条；`scope_hint` 被原样带到线上，导致**整份图响应被拒**。换掉这一句后同一条链路 200、8 组 / 83 个可见成员。

其余判据（`id` vs `key`、页面无算法、折叠纯视觉、suggest 确定性/不写库、无组如实、折叠探针、golden、不退化）全部通过。

| AC | 结果 | 关键证据 |
| --- | --- | --- |
| 001 | ✅（正常声明） | 算法声明 9 组 / 83 节点、`bad=0`；无组 `groups:[]` + 83 个 `null`；响应无字面键 `"key"` |
| 002 | ✅（源码/数据级） | 真数据折叠 **47 = 9 组 + 38 未分组**；组圈/聚合节点/面板落点齐全 |
| 003 | ✅ | 真库 `anspire_excl` 0 组：`groups:[]`、83/83 `node.group=null`、折叠单元 83、零聚合单元 |
| 004 | ✅ | 3 次 suggest 同为 `dff0e6eb…`；库指纹前后完全一致 |
| 005 | ❌ **B-1** | `concept suggest` 链 ✅；**AI 8 组声明链在 HTTP 层 500** |
| 006 | ✅ | 页面 JS 剥注释后 12 个分组/聚类词全 0；算法只在 `src/domain/concept_suggest.rs` |
| 007 | ✅ | 页面 49728 B、0 外链；`GET /` 零对象名；4 个 `/api/*` 无 token 全 401 |
| 008 | ✅ | `INDEX_HTML` = H1 = 真 HTTP 字节，`e665a0ad…`；其余 12 份与 `develop` 逐字节相同 |
| 009 | ✅ | 折叠前后 TTL `ACA259D9…`、show `7CDC510D…` 两两相同 |
| 010 | ✅（suggest） | `source:"algorithm-suggest"`、`reviewed_by:null`；AI 文件非 suggest 产物（见 N-2） |
| 011 | ✅ | lib 284 / doc 4 / 新增 12；集成 799 passed / 3 既有红灯；禁改面 diff 为空 |

---

## 2 · `id` vs `key` 变异实测（A-2.1）

**结论：这条约束是真的，A-1 裁定成立。**

### 2.1 变异

在独立 worktree 里只改一处：

```diff
 src/domain/graph.rs
 pub struct Group {
-    #[serde(rename = "id")]
+    #[serde(rename = "key")]
     pub key: String,
```

重新 `cargo build --bin ontok`，用**同一份**已冻结的 `anspire_excl_r115` v2（9 组）起只读服务，请求：

```text
GET /api/graph?project=anspire_excl_r115&version=2
Authorization: Bearer ***
```

### 2.2 结果

| 版本 | HTTP | stderr |
| --- | --- | --- |
| 未变异（`rename = "id"`） | **200**，`nodes=83 groups=9 members=45`，响应无字面 `"key"` | 正常 |
| 变异（`rename = "key"`） | **500** | `✗ mcp-server 守门器命中 1 项建模术语，已拒绝外送：["key"]` · `✗ 只读图服务守门器命中建模术语，已拒绝出图。` |

500 正文：

```json
{"error":"这一问不给数据：内容里出现了 1 项只有内部使用的术语（为免泄漏，此处不列出具体词条）。这是发布闸门，请联系维护者。"}
```

### 2.3 判定

- 把线上键名改成 `key`，**真的会被发布闸拒绝**：不是命名偏好，`/api/graph` 会整份不可用。
- 反向也成立：未变异的 `id` 形状 200，且响应里不存在字面键 `"key"`。
- 因此「必须叫 id」不是编的；**不因这一点阻断**。

---

## 3 · AI 8 组声明的往返（AC-005）—— 阻断点

### 3.1 准备：自建副本（未用冻结版作 base）

从 `anspire_excl` v2（已冻结、0 组）复制出 **`qa_r115_ai_copy` v1**（未冻结）：

```text
object_types   84 条  内容 hash（去掉 id/version_id）与源完全相同
link_types     52 条  内容 hash 与源完全相同
action_types    0 条
functions       0 条
concept_groups  0 条
```

AI 声明校验：

```text
ontok concept digest --file code/out/concept-declaration-ai.json
内容指纹        7b957e3c1235909b
契约            一致（可直接 import）
groups          8
members         84（unique 84）
```

### 3.2 链路执行

```text
concept import --file <AI 声明> --project qa_r115_ai_copy --base-version 1
  → 提案 p1，共 8 个概念组（客户与子账号 / 商品、套餐与订阅 / 用量、扣费与余额 /
    API 密钥与授权 / 模型、服务商与服务定价 / 营销、活动与兑换码 /
    智能体实例与任务 / 后台与运营支撑）

review decide --proposal p1 --accept-all --project qa_r115_ai_copy
  → 8 个条目 × 2 轨 accepted

proposal apply --proposal p1 --project qa_r115_ai_copy
  → v2

version freeze --project qa_r115_ai_copy --version 2
  → 已冻结 v2
```

库内结果（直接查 `concept_groups`）：

```text
groups=8  members_total=84
```

### 3.3 图上结果：**500**

```text
GET /api/graph?project=qa_r115_ai_copy&version=2   → 500
stderr: ✗ mcp-server 守门器命中 1 项建模术语，已拒绝外送：["key"]
```

### 3.4 根因（已隔离到唯一字段）

逐字段复刻 `scan_modeling_terms` 的词元规则扫描这 8 组，**唯一命中**：

```text
api_entitlement.scope_hint
  "…限流（普通与 TPM 两套配置）、以及**未持 Key** 的试用请求。…"
  → 词元 "Key"，前一个词元「以及未持」不在 BUSINESS_ALLOWED_PREFIXES
  → 命中词条 "key"（按 Rust `is_alphanumeric` 分词；中文与 `Key` 同属一个词元的边界）
```

- 同一句里的 `API-Key` **不**命中（前词元 `api` 在业务白名单）。
- `graph.rs` 把声明的 `scope_hint` 原样带到线上（`Group.scope_hint`，`project_grouped`）；
- `publish_value` → `guard` → `scan_modeling_terms` 扫整段 JSON，键名/值都扫；
- 于是**声明的自由文本**使**整份图响应**被拒，而不是只拒这一组。

### 3.5 对照与隔离实验

| 项目 | 声明 | HTTP | 结果 |
| --- | --- | --- | --- |
| `anspire_excl_r115` v2 | 算法 suggest 的 9 组（scope_hint 固定为「算法建议…」） | **200** | `nodes=83 groups=9 members=45 bad=0` |
| `qa_r115_ai_copy` v2 | **AI 原声明**（8 组 / 84 成员） | **500** | stderr `["key"]` |
| `qa_r115_ai_safe` v2 | 只把 `api_entitlement.scope_hint` 的「未持 Key」改成「未持凭证」，重算 digest `9b9b46bb…`，其余 100% 相同 | **200** | `nodes=83 groups=8 members=83 nulls=0` |

安全副本的 8 组明细（可见成员）：

```text
customer_account      7   （声明 8；internal_staff 的展示名为空，被客户视图挡掉）
catalog_subscription 11
billing_usage        15
api_entitlement       8
model_provider       11
marketing_growth     10
agent_instance        7
admin_ops            14
合计 83 / 83 节点全部有组，null=0
```

**结论**：
- 分组数据层完全正确（8 组 / 84 声明成员；可见 83，与 Dev 已知限制一致）。
- 阻断点不在分组算法，而在**声明自由文本没有在入口处验证**：import/apply 接受它，服务也能启动，直到请求 `/api/graph` 才 500，且整份图不可用。
- 这正是 AC-005「审核后图上可见」在**任务指定的 AI 声明**上没有兑现。

### 3.6 清理

两个 QA 副本 `qa_r115_ai_copy` / `qa_r115_ai_safe` 已删除（`proposals=1, projects=1` 各）；`anspire_excl` 仍为 0 组，未动。

---

## 4 · 页面不许自己算分组（AC-006）

独立扫描（读 `src/server/page.rs` 的 `INDEX_HTML`，抽出唯一 `<script>`，剥掉 `//` 与 `/* */` 注释后检查）：

```text
page script bytes=42865  stripped bytes=32812
PASS  louvain / modularity / community / cluster / greedy /
      greedy_modularity / label_propagation / 聚类 / 贪心 /
      社区检测 / 标签传播 / merge      全部 0 命中
PASS  GROUPS = DATA.groups
PASS  setFold / foldAll / relayout 函数体内无 fetch( / ask( / api/ / XMLHttpRequest
```

- 分组唯一来源：`GROUPS = DATA.groups || []`（`page.rs:960`）。
- `unitsOf` / `renderedCount` / `groupOf` / `groupIndex` 只回答「已分好的组怎么画」，没有邻接、相似度、合并。
- 算法实现只在 `src/domain/concept_suggest.rs`（`suggest` / `communities`）；`page.rs`、`http.rs`、`graph.rs` 内没有聚类实现（其它文件的「聚类」只出现在注释里）。
- 探针 `group_probe.js` 的 AC-006 部分也全 PASS（含额外的 `merge`）。

---

## 5 · 折叠是纯视觉（AC-009）

在 `anspire_excl_r115` v2（9 组）上做前后对比，中间执行页面折叠内核探针：

```text
before  TTL  ACA259D906FA3819E847F313A8C0427DA3D3D6C7182BB6D287B67BBC2EDA6CB9
after   TTL  ACA259D906FA3819E847F313A8C0427DA3D3D6C7182BB6D287B67BBC2EDA6CB9
before  show 7CDC510DC92BF92F6A1ACE1283C109D056DD82C3B32C5606725EF72EF1419D23
after   show 7CDC510DC92BF92F6A1ACE1283C109D056DD82C3B32C5606725EF72EF1419D23
```

- 逐字节相同；TTL hash 与 Dev 报告一致。
- 源码级：`setFold` / `foldAll` / `relayout` 无网络调用；页面无 POST/PUT/DELETE/PATCH；折叠只改 `FOLD` 内存状态与 `NS/ES/PS` 画法。
- **限制**：未做真浏览器手测（本机无 headless 浏览器），未验证折叠动画/组圈观感；但「不改数据」的字节级判据成立。

---

## 6 · `concept suggest` 确定性 + 不写库（AC-004）

同一 `anspire_excl` v2 连跑 3 次：

```text
sha256(suggest_1.json) = DFF0E6EB610EA56F5FB3F713B49D06FE59E8C182A171B6489C20A62082671241
sha256(suggest_2.json) = 同上
sha256(suggest_3.json) = 同上
groups=9  members=45  source="algorithm-suggest"  reviewed_by=null
```

库指纹（`concept_groups` / `ontology_versions` / `proposals` / `proposal_items` 全表，按规范序拼 JSON 后 SHA-256）：

```text
BEFORE concept_groups=91  ontology_versions=385  proposals=154  proposal_items=700
       aad8e15d… c47dfa14… f3519508… e6196a85…
AFTER  concept_groups=91  ontology_versions=385  proposals=154  proposal_items=700
       完全相同的四个 hash
```

**结论：确定性成立，suggest 没有写库。**

---

## 7 · 无组时如实（AC-003）

真实项目 `anspire_excl` v2（0 个概念组）起服务：

```text
GET /api/graph?project=anspire_excl&version=2  → 200
nodes=83  edges=52  groups=[]（JSON 数组）
node.group = null：83 / 83
响应无字面键 "key"
```

真实数据跑页面折叠内核：

```text
groups=0  → 折叠态渲染单元 83 = 对象 83
           → 聚合单元 0（没有任何默认组）
展开态 83；无组态 83
```

页面源码：`GROUPS = DATA.groups || []`；`updateGroupInfo` / `showGroups` 在 `GROUPS.length===0` 时只显示「未分组」及说明，不构造默认组。

---

## 8 · 折叠探针 + 变异（AC-002）

### 8.1 基线

```text
node tests/fixtures/req115/group_probe.js
PASS 折叠态渲染节点数 = 组数 3 + 未分组 4 = 7  实际 7
PASS 折叠态聚合单元数 = 组数  实际 3
PASS 展开态渲染节点数 = 对象数 10
PASS 无组时渲染节点数 = 对象数（没有默认组） 10
PASS 无组时没有任何聚合单元
ALL PASS   exit=0
```

### 8.2 变异（证明探针有牙）

把 `unitsOf` 的折叠分支改成永不折叠：

```diff
-    if (fold[g.id]) { out.push({ fold: true, id: g.id }); continue; }
+    if (false)       { out.push({ fold: true, id: g.id }); continue; }
```

```text
FAIL 折叠态渲染节点数 = 组数 3 + 未分组 4 = 7  实际 10
FAIL 折叠态聚合单元数 = 组数  实际 0
FAIL 折叠态单元 = 每组一个聚合节点 + 每个未分组对象一个节点
FAIL 未知组对象与真正未分组的对象一样各占一个节点  实际 8
4 条 FAIL   exit=1
```

还原后 `ALL PASS` / `exit=0`。**探针不是空转。**

### 8.3 真数据折叠数

把同一份内核抽出来喂真实图 JSON：

```text
anspire_excl_r115 v2（9 组）：folded 47 = 9 + 38；aggregates 9；expanded 83
anspire_excl v2（0 组）    ：folded 83 = 0 + 83；aggregates 0；expanded 83
```

---

## 9 · golden 重锚（AC-008）

| 检查 | 结果 |
| --- | --- |
| `INDEX_HTML`（`page.rs` 常量）sha256 | `e665a0add9c1e4c5ae8a055ec7d88343e4b38a1c16eafdcd1801926e0abf7155`（49728 B） |
| `tests/fixtures/req107/golden/H1_index.html` | 同 sha，逐字节相等 |
| 真 HTTP `GET /` 原始字节（`curl`，非 PowerShell 字符串重编码） | 49728 B，同 sha |
| `SHA256SUMS` 里 H1 | 同 sha |
| 其余 12 份 golden | 全部匹配 `SHA256SUMS`，且与 `develop` `71ae770` 逐字节相同 |
| `PROVENANCE.txt` | 含「REQ-115」「分组」及 H1 重锚说明 |

> 第一次用 PowerShell `Out-File` 保存 HTTP body 多出 2 B BOM，已改用 `curl.exe -o` 取原始字节复核；以上为原始字节结果。

---

## 10 · 不退化（AC-011）

### 10.1 测试数字

| 套件 | 结果 |
| --- | --- |
| `cargo test --lib` | **284 passed / 0 failed** |
| `cargo test --doc` | **4 passed / 0 failed** |
| `--test test_req115_groups`（新增） | **12 passed / 0 failed** |
| `test_req107_pipeline_bytes` | 1 passed |
| `test_req107_zero_visible_change` | 3 passed |
| `test_req107_source_enumeration` | 10 passed |
| `test_graph_server` | 9 passed |
| 全量 `cargo test --tests --no-fail-fast`（Windows 绕行：移开 unix-only 的 `ontok-gate-preflight.rs`；另暂时移开 `test_retained_names_req084.rs` 以让其余目标可编译） | 79 个 `test result` 行 · **1096 passed / 4 failed**；其中 1 个失败是**本次绕行造成的假失败**（`test_req107_red_lines` 检测到 `test_retained_names_req084.rs` 被移走）。恢复该文件后单独复跑：`2 passed / 0 failed`。 |

折算：**集成套件 799 passed / 3 failed**，与 Dev 报告一致；新增套件 12 条全绿。

### 10.2 三条既有红灯（feature 与基线都复现）

| 套件 | feature `7718061` | baseline `develop 71ae770` | 归因 |
| --- | --- | --- | --- |
| `drop_rate` | 0 passed / 1 failed | 0 passed / 1 failed | 缺 `.review-workspace/fix-props/schema.json`（gitignore，输入不在版本控制内，fail-closed） |
| `test_fk_naming` | 5 passed / 1 failed | 5 passed / 1 failed | 同一份脱敏 schema 缺失 |
| `test_agent_orchestration` | 35 passed / 1 failed | 35 passed / 1 failed | REQ-108 分档路由导致的既有 CLI 端到端字节差异（`add_link_type` 缺失），与 REQ-115 无关 |

> 基线复跑时同样临时移开 `ontok-gate-preflight.rs`，跑完 `finally` 挪回：`sha256=C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D`，`Test-Path=True`。

### 10.3 禁改面

`git diff --stat develop...HEAD` 对以下路径**为空**：

```text
src/domain/view.rs
src/domain/review_view.rs
src/extract/*
src/mcp/*
src/domain/property_filter.rs
Cargo.toml / Cargo.lock
```

`src/server/http.rs` 有改动，但 diff 只涉及：

- `load_frozen_view` 多返回概念组；
- `state_from_view` 拆出 `state_from_view_with_groups` / 共用 `state_from_view_inner`；
- `serve` 改走带分组入口；
- 测试夹具补 `groups: None` / `group: None`。

**没有改** `build_router` 路由表、令牌校验、405/401 语义。`tools.rs` / `property_filter.rs` 未触碰。

### 10.4 页面与鉴权

```text
INDEX_HTML 49728 B（<80KB）
http:// / https:// / <script src / <link / @import / cdn / 外链字体：全 0
GET /                                    → 200，页面不含「客户余额」「套餐模板」「活动」「授权额度」等对象名
GET /api/graph（无 token）               → 401
GET /api/node（无 token）                → 401
GET /api/ontology.ttl（无 token）        → 401
GET /api/scope（无 token）               → 401
错误 token                               → 401
```

---

## 11 · 阻断项 / 非阻断项

### 🔴 B-1（阻断）· 声明 `scope_hint` 未校验，能让整份 `/api/graph` 500

**事实链**：

1. import/apply 完全接受 AI 声明（8 组 / 84 成员），库内正确；
2. 服务正常启动；
3. 第一次 `GET /api/graph` 就 500，8 个组全部不可见；
4. stderr 明确：`守门器命中建模术语 ["key"]`；
5. 唯一来源是 `api_entitlement.scope_hint` 的「未持 Key」；
6. 只改这一句（其余内容/成员/组名完全不变）后，同一套代码 200、8 组 / 83 可见成员。

**为什么是阻断**：

- AC-005 的交付要求是「审核后图上可见」。任务指定的 AI 声明在**真实链路**上不可见，且不是「少一个成员」，是**整份响应 500**。
- 入口把不可发布的自由文本放进权威数据，缺少 `scan_modeling_terms` 级别的校验；这是 fail-open 到 fail-closed 的延迟爆炸，排查成本高（服务启动成功，页面只显示读不到）。
- 发布闸是红线，不能放宽；只能让**入口**对 `scope_hint` 负责。

**建议修复（任一，不应放宽闸）**：

- 在 `concept import`（最好也在 `apply` 前）对 `display_name` / `scope_hint` 跑 `scan_modeling_terms`，命中即拒绝并报出组与词条，提示声明作者改写；
- 或明确规定 `scope_hint` 不上线（只留在审核/建模视图），线上不返回自由文本；
- 修复后重跑本报告 §3 的 AI 原声明链路，期望 `200 + 8 组 + 83 可见成员`。

> 注意：这不是要求把「Key」从 `MODELING_TERMS` 删除。删词/放宽都会破坏红线；正确方向是**入口校验 + 给作者可操作的报错**，与 A-1 里 BA 记的「按位置/上下文判」是两件事。

### 🟡 N-1（非阻断）· 未做真浏览器手测

组圈观感、折叠动画、拖拽与组心引力手感只有源码级 + Node 探针 + 真数据计数，没有浏览器截图。Dev 已在报告中声明；本轮同样无 headless 浏览器，不为此加依赖。

### 🟡 N-2（非阻断）· AI 声明文件没有 `source` / `reviewed_by`

- `concept suggest` 产物有：`source:"algorithm-suggest"`、`reviewed_by:null`（AC-010 通过）。
- 任务给的 AI 声明不是 suggest 产物，文件里没有这两个键，只有 `declared_by:"ai-concept-grouper / qwen36-27B"`、`confirmed_by:null`。按 AC-010 的字面（判「建议层」）不构成违规；但如果 BA 要求**所有**声明文件都标来源/审核状态，需要补一条入口规则。

### 🟡 N-3（非阻断）· 组成员只算图里看得见的（既有已知限制）

AI 声明 84 个成员，图上 83：`internal_staff` 的展示名为空，被客户视图挡掉；`customer_account` 因此 8→7，其余 7 组不变。页面上的 N 与点得到的对象一致，这是 Dev 已登记的 L4 行为，不是本轮引入。

### 🟡 N-4（非阻断）· H2 golden 仍走旧入口

`state_from_view` 保留为「分组通道未接通」的旧形状，只为 `H2_graph.json` 逐字节不变；生产 `serve` 走 `state_from_view_with_groups`。两条入口共用 `state_from_view_inner`，已用 `test_req115_groups::legacy_projection_keeps_the_frozen_h2_shape` 钉住，未见分叉。

---

## 附 · 复跑入口

```powershell
# 独立 worktree（审核时使用，已移除）
git worktree add qa-REQ-115 -b qa/REQ-115 feature/REQ-115

# 探针
node tests/fixtures/req115/group_probe.js

# 既有红灯归因（先移开 unix-only 的 preflight，finally 挪回）
#   sha256 = C7CF95E2A73B8C75BB44B1970396ACA40F9146992D0C7DCCB3636B3BA571322D
cargo test --test drop_rate
cargo test --test test_fk_naming
cargo test --test test_agent_orchestration

# 声明入口校验（建议 B-1 修复后新增）
ontok concept import --file code/out/concept-declaration-ai.json \
  --project <新副本> --base-version 1
```
