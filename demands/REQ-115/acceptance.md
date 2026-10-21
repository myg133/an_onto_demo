# 验收标准：REQ-115 · 概念分组与图上的归并

> **核心承诺**：图上的分组**必须是人签过的**；算法只能提案，不能冒充权威。

## 一 · AC

### AC-001 · 图接口返回权威分组

- `GET /api/graph` 返回 `groups: [{id, display_name, members[], scope_hint?}]`
- ⚠️ **字段名是 `id` 不是 `key`**（BA 裁定 A-1）：`key` 在发布闸词表 `MODELING_TERMS` 里（`src/domain/view.rs:902`），
  写成线上键名会让**每次带分组的 `/api/graph` 被闸拒发**。`Node` 早有同一先例（`graph.rs:110 serde(rename = "id")`）。
- 判据：带分组的响应里**不得出现**线上键 `"key"`；`{"id":…}` 必须过闸、`{"key":…}` 必须被拒（正反两条都要有）
- 每个节点带 `group`（无组则 `null`），且 `node.group` 必须等于某 `groups[].key` 或 `null`
- 判据：真的建一个概念组 ⇒ `groups` 出现、成员节点 `group` 正确；未分组项目 ⇒ `groups: []` 且全 `null`

### AC-002 · 页面按组归并（组圈 + 折叠 + 面板）

- 组以背景圈/凸包 + 组名呈现
- **折叠**：折叠后该组变成一个聚合节点（组名 + N），展开还原
- 组面板：列出成员、点击定位
- 判据（源码级 + 数据级）：折叠/展开的状态切换函数存在；折叠态下渲染节点数 = 组数 + 未分组节点数

### AC-003 · 无组时如实显示

- 无概念组 ⇒ 页面显示「未分组」，**不出现任何虚构分组**
- 判据：`groups: []` 时页面无组圈、无聚合节点；⛔ 不许有「默认组」

### AC-004 · `concept suggest` 确定性且不写库

- 同 project+version 跑两次 ⇒ 声明文件**逐字节相同**
- 跑之前后 `concept_groups` / `ontology_versions` / `proposals` 行数与内容**不变**
- 判据：sha256 相同 + 库快照 hash 相同

### AC-005 · 往返：建议 → 审核 → 图上出现

- `concept suggest --out d.json` → `concept import --file d.json` → 双轨签 → `proposal apply` → `GET /api/graph` 的 `groups` 出现该分组
- 判据：全链路真跑一次，逐步断言（导入后有提案、签完可 apply、apply 后 groups 非空）

### AC-006 · 页面**不许**自己算分组

- 页面 JS 里不得出现聚类/分组算法（社区检测、贪心合并等）
- 判据：源码级——页面只消费 `groups`，无计算；⛔ 违反即判 FAIL

### AC-007 · 零外部依赖 + 数据面不变

- 页面仍零外部依赖、`GET /` 仍零数据壳、`/api/*` 无 token 仍 401
- 判据：重跑 REQ-110 的两条判据

### AC-008 · golden 重锚合规

- `tests/fixtures/req107/golden/H1_index.html` 更新 + `SHA256SUMS` + `PROVENANCE` 写明「REQ-115 加了分组渲染」
- 其余 12 份 golden 逐字节未动

### AC-009 · 折叠不改数据

- 折叠/展开前后，`ontology show` 与 TTL 导出**逐字节相同**
- 判据：对比两次导出 sha256

### AC-010 · 建议的来源可辨识

- 声明文件里 `source: "algorithm-suggest"`；`reviewed_by: null`
- 审核界面若展示分组来源，必须显示这个值
- 判据：文件字段存在；⛔ 不许把建议标成人工声明

### AC-011 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `src/server/http.rs` 的鉴权与路由语义 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 基线 | REQ-113/114 合并后的 lib/doc/套件数字，一条不许少 |

## 二 · 纪律

- **H** 分组只有一处权威（`concept_groups`）；⛔ 页面/接口都不许再算
- **G** 变异三元组；编译失败不算执行
- **S** AC 也会错，发现直接说

---

## 三 · Dev 交付后的裁定

### A-1 · `groups[].key` 改 `groups[].id`（**接受 Dev 的判断**）

Dev 交付时字段叫 `id`，与 AC 字面的 `key` 不符。它的理由：**`key` 在发布闸词表里**。

**BA 独立核实成立**：

```
src/domain/view.rs:902   pub const MODELING_TERMS: &[&str] = &[ "pk", "primary_key", "cardinality",
                            "ordinal", "evidence", "disjoint", "extracted_by", "proposed_by",
                            "approved_by", "key", "外键", "基数", "主键", "证据" ];
```

且 `Node` 早在 `graph.rs:110` 就用 `#[serde(rename = "id")]` 绕开同一问题——**先例一致**。

**裁定：改 AC 为 `id`**。⛔ 不放宽 `guard()`（红线），⛔ 不叫 `key` 然后接受 500。

**顺带记一个真实的产品张力**：`"key"` 是业务词（API Key），却被列入建模术语。现在的处理靠「整体等于 `key`」才命中（`License Key` 不误伤）。**这是词表与业务词汇天然冲突的地方**，下一轮若要治，方向是「按位置/上下文判」而不是「加词/删词」。

### A-2 · 交付里的两点要 QA 独立验

1. **`id` 的选择不是自说自话**：QA 要做变异——把线上的 `id` 改成 `key`，确认 `/api/graph` 真的被闸拒（否则「必须叫 id」这个约束就是编的）。
2. **往返是在副本项目上做的**（`anspire_excl_r115`），因为 `apply` 拒绝冻结 base。QA 要确认：副本与源是**同一份快照**（同 84 实体 / 52 关系），且真实项目 `anspire_excl` 上**确实没有分组**（页面应显示未分组，不是偷偷用了副本的组）。

---

## 四 · QA 首轮裁定（FAIL 1 阻断）

### A-3 · 阻断成立：`scope_hint` 把整份图打挂

QA 用 AI 的 8 组声明走完 `import → 签 → apply → freeze` 后：

```
GET /api/graph → 500
stderr: 守门器命中 1 项建模术语 ["key"]
```

根因：`api_entitlement.scope_hint` 里「未持 **Key**」，被发布闸当词条 `key` 命中，而 `scope_hint` **被原样带到线上** ⇒ 整份响应被拒。

**连带确认 A-2.1**：`id` vs `key` 的约束**是真的**——QA 把 `graph.rs:110` 的 `rename="id"` 改成 `"key"`，真请求 ⇒ **500 + 报词条**；未变异 ⇒ 200。**「必须叫 id」不是编的。**

### A-4 · 修法：`scope_hint` 不上客户图 + `display_name` 导入时过闸

| # | 修法 | 理由 |
| --- | --- | --- |
| **B-1** | `/api/graph` 的 `groups[]` **删掉 `scope_hint`**，只留 `{id, display_name, members}`；`scope_hint` 只在审核界面展示 | `scope_hint` 是**评审人注解**（「算法建议…待人审」），本就不该给客户看。删掉比扫它更根本，且 AI 那份声明**不用改写**就能导入 |
| **B-2** | `concept import` 对 `display_name` 跑 `scan_modeling_terms`，命中 ⇒ **拒绝导入**并报**组名 + 词条** | `display_name` 客户可见，坏声明应**根本进不来**，而不是流到出图阶段才整份 500 |

⛔ 不放宽 `guard()`；⛔ 不静默剔除。

### A-5 · 非阻断（登记）

| # | 项 |
| --- | --- |
| 1 | 无真浏览器手测（一如既往，如实登记） |
| 2 | AI 声明文件本身无 `source` / `reviewed_by`（它不是 `suggest` 产物，是外部声明）——可接受，但审核界面应显示「来源未知/外部声明」 |
| 3 | H2 出口仍走旧入口（已由测试钉住） |
