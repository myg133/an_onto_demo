# 验收标准：REQ-115 · 概念分组与图上的归并

> **核心承诺**：图上的分组**必须是人签过的**；算法只能提案，不能冒充权威。

## 一 · AC

### AC-001 · 图接口返回权威分组

- `GET /api/graph` 返回 `groups: [{key, display_name, members[], reason?}]`
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
