# REQ-115 · 概念分组与图上的归并 —— 验证报告（**第 2 轮 · QA 阻断修复：BA 裁定 C**）

> **被测**：worktree `feature-REQ-115` / 分支 `feature/REQ-115` · 基线 `develop` `71ae770` · **未 push**
> **本轮范围**：QA 判 FAIL（1 阻断）后的修复。BA 纠正了「`scope_hint` 是评审注解」这个前提 ——
> 它在 REQ-010 里就是**客户可见的组说明**（业务总览的 `summary`），病灶是**一个字段两种用途**。
> 按裁定 (C)：**C-1** 组字段纳入导入闸、**C-2** `suggest` 不写客户字段、**C-3** 图与总览同口径。
> **不碰**：`BA/` / `code/` / `Deploy/`；四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4；
> `/api` 的鉴权与路由语义。
> **日期**：2026-10-22 · Windows · PG `postgresql://***@172.17.0.87:5432/anspire_real`
> **历史报告**：第 1 轮（实现）与 B 轮（被 C 取代）见 `git log -p -- .feature/verification-report.md`。

---

## ⚠️ 三条必须先读的结论

1. **QA 阻断的根因是「一个字段两种用途」**，不是某个出口漏了：
   `scope_hint` 在 REQ-010 的**业务总览**里是客户可见的 `summary`（C4/C5/M1），
   却又被 `concept suggest` 当成内部备注写（「算法建议…待人审」）。
   ⇒ 本轮按 (C) 把**客户可见的组字段全部纳入导入闸**（C-1），并让建议层**不再写客户字段**（C-2）。
2. **AI 那份声明现在会被 import 拒绝一次**（这是**正确行为**）：
   `api_entitlement.scope_hint` 里的「未持 **Key**」命中闸 ⇒ 报
   「组 `api_entitlement` 的 scope_hint 含建模术语 `key`」。
   把该句改成「未持**凭证**」、重算 digest ⇒ **全链路通过**（200 / 8 组 / 83 成员）。
   **我们不替作者猜业务措辞**：报错点名到「组 + 字段 + 词条」，改不改、怎么改由声明作者定。
3. **C-3 我选了「图带 `scope_hint`」**（与总览同口径）。因此 QA 第 1 轮的 B-1（从图上删掉）
   **被回滚**；安全边界改由 C-1 在**导入时**兜住 —— 出图时不可能再遇到脏字段。

---

## 0 · 结论先行（对应 BA 的 6 项交付）

| BA 交付项 | 实测 |
| --- | --- |
| ① C-1 实现位置 + 三种拒绝实测 | ✅ `concept.rs:349` / `cli.rs:1211`；三种见 §2（scope_hint 拒 / display_name 拒 / 两者干净通过） |
| ② C-2 改完 `suggest` 的 `scope_hint` 长什么样 | ✅ 9 个组**全部是 `"scope_hint": null`**（§3）；来源写在 `declared_by` / `rule` |
| ③ C-3 选哪个口径 + 总览是否恢复 200 | ✅ 选「图带、总览也带」（同口径）；`ontology overview` 在 v4 上 **exit 0**（§4） |
| ④ AI 原样 import 被拒（报组/字段/词条）→ 改一句 → 全链路 200 + 8 组 | ✅ 见 §2.4 / §4.2 |
| ⑤ `ontology overview`（新发现的 C4/C5 出口） | ✅ C4 `exit 0`、C5 `--json` `exit 0`（§4.3） |
| ⑥ 测试数字 + 绕行双证 | ✅ 见 §7 |

| AC | 判据 | 实测 |
| --- | --- | --- |
| 001 | `/api/graph` 返回 `groups`，节点 `group` 等于某组 `id` 或 `null` | ✅ 8 组 / 83 节点 / `bad=0`；无组项目 `groups:[]` + 83 个 `null` |
| 002 | 组圈 + 折叠（组名 + N）+ 组面板（成员 / 点击定位） | ✅ 源码落点 + Node 探针 7 = 3 + 4 |
| 003 | 无组时如实显示「未分组」，⛔ 无默认组 | ✅ 探针：`groups=[]` ⇒ 零聚合单元 |
| 004 | `concept suggest` 两次逐字节相同 + 不写库 | ✅ C-2 后两次同 sha256 `abc9068a…`；库指纹前后相同 |
| 005 | suggest → import → 签 → apply → 图上出现 | ✅ 9 组与 AI 8 组（改一句后）各跑一遍（§4.2） |
| 006 | 页面不许自己算分组 | ✅ 只读 `DATA.groups`；剥注释后 7 个算法词全 0 |
| 007 | 零外部依赖 / `GET /` 零数据壳 / `/api/*` 401 | ✅ 页面 49936 字节 < 80KB；无 token → 401 |
| 008 | H1 重锚；其余 12 份不动 | ✅ `pipeline_bytes` 1 + `zero_visible_change` 3 全绿 |
| 009 | 折叠前后导出逐字节相同 | ✅ TTL / `ontology show` sha256 两两相同 |
| 010 | `source: "algorithm-suggest"` / `reviewed_by: null` | ✅ 文件字段 + 单测 |
| 011 | 不退化 | ✅ 见 §7 |

---

## 1 · 本轮改动（C-1 / C-2 / C-3）

| 文件 | 落点 | 改了什么 |
| --- | --- | --- |
| `src/domain/concept.rs:321` | `pub struct GroupNameHit` | 「组 + 字段 + 词条」的结构化命中；`Display` 就是报错原文 |
| `src/domain/concept.rs:349` | `pub fn scan_group_names` | **C-1**：扫 `display_name` / `scope_hint` / 组 `key` 三个**客户可见**字段 |
| `src/domain/concept.rs:366` | `fn push_hits` | 判据**借用管线**（`pipeline::group_name_hits`），不新增第二份判据 |
| `src/domain/pipeline.rs:893` | `pub fn group_name_hits` | 术语判据的唯一实现（`scan_modeling_terms`）在管线／守门器侧的入口 |
| `src/cli.rs:1211` | `concept import` | 校验指纹之后、碰库之前：命中即 `bail!`，逐条列出「组 + 字段 + 词条」 |
| `src/domain/concept_suggest.rs:201` | `suggest` 的组构造 | **C-2**：`scope_hint: None`（内部来源在 `declared_by` / `rule`） |
| `src/domain/graph.rs:125` | `Group` | **C-3**：恢复 `scope_hint`（与总览 `summary` 同源、同口径） |
| `src/domain/graph.rs:157` | `DeclaredGroup` | 同步恢复；`declared_groups` / `project_grouped` 照搬 |
| `src/server/page.rs:1160` / `:1174` | `showGroups` / `showGroup` | 组面板照常显示**客户可见**的组说明（C-3） |
| `tests/test_req115_groups.rs` | 13 条 | C-1 三种实测（scope_hint 拒 / display_name 拒 / 干净通过）+ 页面同口径断言 |
| `src/domain/concept.rs` / `concept_suggest.rs` 测试 | +4 条 | scope_hint 拒 / 业务措辞通过 / 组 key 也判 / suggest 不写客户字段 |
| H1 golden | `d8bd4581…`(32396) → **`c8d5e148…`(49936)** | 页面第二次变化（组说明回到面板），其余 12 份逐字节未动 |

> 第 1 轮实现（`/api/graph` 的 `groups`、页面组圈/折叠/面板/引力、`concept suggest`）见 `git log`；
> 本轮**没有**动 `overview.rs`、没有动任何 golden 的内容（只重锚 H1 的页面字节）。

---

## 2 · C-1：导入闸的三种实测（真库）

判据在 `concept import` 的**碰库之前**（`decl.verify()` 之后、`get_or_create` / `proposals.create` 之前）
⇒ 被拒的声明**一条提案都不会留下**。

### 2.1 ① `scope_hint` 命中（AI 声明**原样**）

```text
ontok concept import --file code/out/concept-declaration-ai.json \
                     --project anspire_excl_r115 --base-version 1
exit 1
✗ 概念声明被拒绝导入：有 1 处组字段会撞上客户发布闸（这些字段会出现在 /api/graph 的 groups[] 与业务总览的 summary 里）——
  - 组 `api_entitlement` 的 scope_hint 含建模术语 `key`
请改措辞后重算指纹（ontok concept digest --file …）再导入。⛔ 不会静默剔除，也不会替你改写。
```

### 2.2 ② `display_name` 命中（干净 `scope_hint` + 坏组名）

```text
（把 customer_account.display_name 改成「客户与 Key 子账号」，重算 digest）
exit 1
✗ 概念声明被拒绝导入：有 1 处组字段会撞上客户发布闸（…）——
  - 组 `customer_account` 的 display_name 含建模术语 `key`
```

### 2.3 ③ 两者都干净 ⇒ **导入成功**

```text
（把 api_entitlement.scope_hint 的「未持 Key」改成「未持凭证」，重算 digest）
概念声明 v1.0.0 已导入提案 p1（项目 anspire_excl_r115 · 基于 v1）—— 共 8 个概念组，待双轨审核
  [1] add_concept_group 客户与子账号 … [8] add_concept_group 后台与运营支撑
```

> 字段名**点名到列**（`display_name` / `scope_hint` / `key`），因为三者都可能成为
> 客户出口里的文本：图上 `groups[]` 三个都在，总览 `summary` 来自 `scope_hint`。

---

## 3 · C-2：`suggest` 不写客户字段

`concept suggest` 的组现在是：

```json
{
  "key": "concept_01",
  "display_name": "后台管理员",
  "members": ["admin_user_roles", "admin_users"],
  "scope_hint": null,
  "low_confidence": true,
  "extracted_by": null,
  "evidence": []
}
```

真库实测（`anspire_excl` v2）：

```text
groups = 9
groups with scope_hint = 0          （9 个组全部 null）
sha256(a) = abc9068ac018b93add267d17543489a753c8c72b2a0175e8cf31723f9654e953
sha256(b) = abc9068ac018b93add267d17543489a753c8c72b2a0175e8cf31723f9654e953  （两次相同）
```

内部来源仍在 `declared_by = concept-suggest/greedy-modularity-v1` 与 `rule` 里
（它们**不进客户视图**）；客户字段不再出现「算法建议…」这种内部话。

---

## 4 · C-3 口径 + 两个出口的实测

### 4.1 选择

**图带 `scope_hint`，总览也带** —— 两个出口**同一口径**。理由：
它在产品原有语义里就是客户可见的组说明（总览 `summary`）；删除它等于改 REQ-010 的产品语义，
而那不是本需求的授权范围。安全性由 C-1 在**导入时**保证。

### 4.2 AI 声明：原样被拒 → 改一句 → 全链路 200

```text
review decide --proposal p1 --accept-all --project anspire_excl_r115
  提案 p1 的 8 个条目 × 2 轨 → accepted
proposal apply → 提案已应用 → v4
version freeze --version 4 → 已冻结 v4
```

```text
GET /api/graph?project=anspire_excl_r115&version=4   (Bearer)
  status = 200 · nodes=83 · edges=52 · groups=8 · covered=83 · bad node.group = 0
  raw contains literal "key" = False
  raw contains scope_hint    = True   ← C-3（与总览同口径）
```

```json
"groups":[
 {"display_name":"客户与子账号","id":"customer_account",
  "members":["客户","客户账号绑定","账号渠道归属","客户功能权限","子账号权限","绑定权限快照","子账号额度"],
  "scope_hint":"客户主档、账号绑定关系、客户内部员工与子账号权限、客户功能开关。…"},
 {"display_name":"API 密钥与授权","id":"api_entitlement","members":["授权", …],
  "scope_hint":"客户手上有什么凭证、能调到什么、能调多快：API-Key（…）、以及未持凭证的试用请求。…"},
 …
]
"nodes":[{"attribute_count":10,"group":"marketing_growth","id":"活动","label":"活动","parent":null},
         {"attribute_count":7,"group":"admin_ops","id":"后台角色","label":"后台角色","parent":null}]
```

### 4.3 新发现的出口（C4/C5）已验掉

```text
ontok ontology overview --project anspire_excl_r115 --version 4
  业务总览
  83 个对象 → 8 个业务概念（+ 0 个未归组对象）
  …「管什么：客户主档、账号绑定关系、…」
overview exit = 0
ontok ontology overview --project anspire_excl_r115 --version 4 --json
overview --json exit = 0
```

（C 轮之前这一条是 `⚠ 客户视图泄露了建模术语（出口 C4）：["key"]`、exit 2。）
同一条 `scope_hint` 同时喂饱图与总览，而**导入闸**保证它客户安全。

### 4.4 审核界面仍能看到 `scope_hint`

`GET /api/review` 是内部轨（不过客户闸），条目 `payload` 原样给出：

```text
item kind=add_concept_group target=API 密钥与授权
payload.scope_hint = 「…以及未持 Key 的试用请求。…」（导入前的原文）
payload keys = display_name,evidence,extracted_by,key,members,scope_hint
```

---

## 5 · 页面 / 折叠（第 1 轮交付，本轮只恢复组说明一行）

| 功能 | 位置 |
| --- | --- |
| 组圈 + 组名 | `buildScene` 的 `g.gh` + `paint` 每帧算圆心/半径 |
| 折叠 / 展开 | `FOLD` + `unitsOf`（`:418`）+ `setFold`（`:1076`）+ `foldAll`（`:1089`）+ `relayout`（`:983`） |
| 聚合节点「组名 + N」 | `relayout` 拼 label，id `@g:<组 id>`；点它展开 |
| 组面板 | `showGroups`（`:1139`）/ `showGroup` / `locateMember`（`:1184`）/ `scope_hint`（`:1160`） |
| 组内聚拢 | `physicsStep` 里 `GRP_F`（`:300` / `:362`） |
| 无组如实 | `updateGroupInfo`：`GROUPS.length === 0` ⇒ 「未分组」，零聚合节点 |

折叠态节点数（`node tests/fixtures/req115/group_probe.js`，exit 0）：

```text
ALL PASS（折叠态 7 = 组 3 + 未分组 4）
```

REQ-114 布局探针复跑：`ALL PASS（168 帧 / 余量 15.79px / 5.20px）`。

---

## 6 · 折叠不改数据（AC-009）

折叠是纯前端内存状态（`FOLD`），`setFold` / `foldAll` / `relayout` 函数体里没有
`fetch(` / `ask(` / `api/`（探针断言）。同一版前后各导出一次：

```text
ttl1 = ttl2  = ACA259D906FA3819E847F313A8C0427DA3D3D6C7182BB6D287B67BBC2EDA6CB9
show1 = show2 = FD2A916A568960B3E5A834CE28A98A172BFA7DE5CC74D55B98AE0F504E09FDF7
```

---

## 7 · 测试与绕行

### 7.1 数字

```text
lib   291 passed / 0 failed   （272 → 288 → 290 → 本轮 291：N-2 的复合词正反两面）
doc     4 passed / 0 failed
集成   75 个目标跑完 · 802 passed / 3 failed（3 条既有红灯见 §7.3）
       另 1 个目标 tests/test_retained_names_req084.rs 因 Windows 绕行未编译（§7.2）
       新增 tests/test_req115_groups.rs —— 13 passed / 0 failed
       新增 tests/test_req115_import_gate.rs —— 2 passed / 0 failed（真进程 · N-1）
```

| 套件 | 结果 |
| --- | --- |
| `test_req115_groups`（13 条） | 13 passed |
| `test_req115_import_gate`（**新增 2 条 · 真进程**） | 2 passed（§9.1） |
| `test_req107_pipeline_bytes`（H1 重锚 + 其余 12 份） | 1 passed |
| `test_req107_zero_visible_change`（SHA256SUMS 互校） | 3 passed |
| `test_req107_source_enumeration`（写点/函数/模块登记） | 10 passed |
| `test_req107_outlet_pipeline`（判据直调只许在管线/守门器） | 12 passed |
| `test_graph_server`（页面守门 / 零外链 / 域层零 I/O） | 9 passed |
| `test_req114` / `test_req115` Node 探针 | `ALL PASS` |

> 口径：`lib 272 → 291` 只增不减；`doc 4` 不变；集成 76 个文件 = 75 跑 + 1 绕行。
> 没有任何既有用例被删 / 被 `#[ignore]` / 被放宽。

### 7.2 Windows 绕行双证

`src/bin/ontok-gate-preflight.rs` 是 unix-only（`std::os::unix::fs::MetadataExt`），
Windows 上编不过。按验收方式挪开，**finally 挪回**：

```text
sha256(before) = c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d
src/bin/ontok-gate-preflight.rs  Test-Path = False   （测试期间）
…
src/bin/ontok-gate-preflight.rs  Test-Path = True    （跑完挪回，134877 字节）
sha256(after)  = c7cf95e2a73b8c75bb44b1970396aca40f9146992d0c7dccb3636b3ba571322d
```

`git status --short` 无 `D` 行。代价：`test_retained_names_req084.rs` 用 `include_str!`
引那份源文件 ⇒ **该套件编不过**（与 REQ-113/114 同一口径，既有红灯）。

### 7.3 既有红灯（与本需求无关）

```text
drop_rate            → 缺 .review-workspace/fix-props/schema.json（fail-closed 输入不在版本控制内）
test_fk_naming       → 同一份脱敏 schema 缺失
test_agent_orchestration::test_cli_end_to_end_output_is_byte_identical_to_legacy_extract
                     → REQ-108 分档路由带来的既有 CLI 端到端差异（REQ-112/113 已登记）
test_retained_names_req084 → Windows 绕行导致编不过（同上，既有）
```

---

## 8 · 已知限制 / 下一轮 / 仍需裁定

### 8.1 设计缺陷（**已登记，下一轮拆字段**）

> **一个字段两种用途**：`scope_hint` 既是**客户可见的组说明**（总览 `summary`、图上组说明），
> 又被当成**内部备注**写（`suggest` 原来的「算法建议…待人审」）。

- 本轮缓解：C-1（导入时过客户闸）+ C-2（`suggest` 不写客户字段）；
- 根本修法：拆成 `scope_hint`（客户可见，过闸）+ **`review_note`**（内部轨，不过闸）。
  要动 REQ-010 的 schema 与快照，**不在本轮**；
- ⛔ 下一轮别再靠「某个出口删字段」来止血 —— 那只是把同一个病搬到另一个出口。

### 8.2 仍需裁定：`groups[].key` → 线上 `id`

AC-001 字面写 `groups: [{key, display_name, members[], reason?}]`，交付的是 `id`。
`key` 是发布闸词表里的词，写成线上键名会让每次带分组的 `/api/graph` 被拒（500）。
可选只有：改词表（红线禁）、叫 key 并接受 500、叫 id（`Node` 已有先例）。
本轮维持第三条，并留了正反判据。**若 QA 要求逐字，请裁定改 AC 或放开 guard()。**

### 8.3 AI 声明的处理方式（需用户执行一步）

AI 那份声明的 `api_entitlement.scope_hint` 需要人把「未持 Key 的试用请求」改成
「未持凭证的试用请求」（或任何客户安全的措辞），然后：

```text
ontok concept digest --file <file>     # 拿到新指纹
# 把 history[-1].content_digest 改成新指纹
ontok concept import --file <file> --project … --base-version …
```

**我们不替作者猜业务措辞**：报错已经点名到「组 + 字段 + 词条」；
静默改写等于替声明作者做命名决定（也是本项目一直禁止的 fail-open）。

### 8.4 其余已知限制（都已写进模块文档）

1. **组成员只算图里看得见的**：真库现场 84 实体 → 83 节点；同名对象只顾第一个（REQ-093 L2）。
2. **未做真浏览器手测**：组圈观感、折叠动画、组内引力手感只有源码级 + Node 探针。
3. **`scope_hint` 不做 AI 命名**（需求「不做」列表）；它由声明作者写。

---

## 9 · 第 3 轮 · QA 复审补项（N-1 / P1-1 / N-2）

### 9.1 N-1（**阻断**）：导入闸的接线用**真进程**钉住

QA 实测：把 `src/cli.rs` 的 `scan_group_names` 调用点删掉（函数留着），
`lib 290` 与 `test_req115_groups 13` **全绿** —— 与 REQ-114「探针数文本不看行为」同一类病。

**新测试**：`tests/test_req115_import_gate.rs`（2 条，spawn 真 `ontok concept import`）：

```text
① dirty_scope_hint_is_rejected_by_the_real_import_process
   声明：scope_hint = "…未持 Key 的试用请求"
   → exit 1
   → stderr: 组 `g1` 的 scope_hint 含建模术语 `key`
   → proposals(project) 仍 = 0（校验在碰库之前）

② clean_declaration_is_imported_by_the_real_import_process
   声明：scope_hint = "…未持凭证的试用请求"
   → exit 0 · stdout "共 1 个概念组"
   → proposals(project) = 1（真的落库，防「通过」假绿）
```

**变异实测（有牙）**：把 `src/cli.rs` 的闸调用整段注释掉，再跑 ①：

```text
test dirty_scope_hint_is_rejected_by_the_real_import_process ... FAILED
assertion `left == right` failed: 脏 scope_hint 必须 exit 1（否则闸根本没接线）
stdout=概念声明 v1.0.0 已导入提案 p1（项目 t_req115gate_dirty_… · 基于 v1）—— 共 1 个概念组，待双轨审核
  left: Some(0)
 right: Some(1)
test result: FAILED. 1 passed; 1 failed
```

恢复调用点后 `2 passed; 0 failed`（`src/cli.rs` 用 `git checkout` 还原，无残留标记）。

### 9.2 P1-1：边界写准确（**不是回归，但旧数据仍 500**）

现场：`anspire_excl_r115` **v3** 是 C-1 之前入库的脏快照（`scope_hint` 含「未持 Key」），
它仍然 `GET /api/graph → 500` / `ontology overview → exit 2`（本轮实测 v3 = **500**，v4 = 200）。
C-1 只挡**新导入**，读路径没改。

**改前**（`src/domain/graph.rs` `Group` 文档）：

```text
/// 所以图上带它（与总览口径一致，C-3）。它之所以安全，是因为
/// `concept import` 会先用客户发布闸判它（C-1：命中即拒不导入）——
/// 不会出现「快照里有脏说明、出图时整份 500」。
```

**改后**：

```text
/// - **新导入**的声明不会把脏说明带进来 —— `concept import` 会先用客户发布闸判它
///   （C-1：命中即拒不导入，报「组 + 字段 + 词条」）；
/// - **已入库的旧快照仍按原行为拒发**（`/api/graph` 500、`ontology overview` exit 2）——
///   读路径本轮没改，C-1 只挡新导入。要清理旧数据只能**重新导入**（改措辞、重算 digest）
///   或**重建快照**；⛔ 别把这条写成「已封堵」。
```

同步改：`Group::display_name` 的注释（「坏组名在导入时被拒」→ 只对新声明成立）、
`graph.rs` 的 L5 边界段、`tests/fixtures/req107/golden/PROVENANCE.txt`（同样写明
「新导入」与「旧快照仍拒发 + 清理方式」）。

### 9.3 N-2：词元级匹配（**保持现状，不改**）

`scan_modeling_terms` 按「非字母数字/下划线」切词元、且 `_` 是词内字符 ⇒
`api_key_group` 是一个词元 ≠ `key`，**不拒**；实测它也不会打挂图（200）。

**裁定：保持现状**（导入闸与出图闸是同一把尺子 ⇒ 过得了 import 就不会 500）。
⛔ 不改「含 key 字样就拒」——那会误伤 `api_key` / `license_key` 这些正常业务词。

文档已写进 `src/domain/concept.rs::scan_group_names` 的「⚠️ 匹配是**词元级**的」一节，
并加了单测 `test_compound_words_with_key_are_intentionally_not_hits`（正反两面：
`api_key_group` / `license_key` / `order_key_ref` / `keyring_service` 放行；
单独成词的 `Key` 仍必须被抓）。