# 验收标准：REQ-112 · 审核界面接入 AI 建议

> **前置**：REQ-111 已合并（`develop` `4178a4e`）
> **核心约束**：v1 **零模型调用**、**零写库**、**确定性**

## 一 · AC

### AC-001 · 建议由确定性纯函数产出

- `suggestion(confidence) -> {kind, reason, signals}` 是**纯函数**（无 I/O、无时间、无随机）
- 同一 `confidence` 两次调用 ⇒ 结果**逐字段相同**
- 判据：单元测试对同一输入调用两次并 `assert_eq!`

### AC-002 · 规则表逐条落地

对 demand §2 的 7 条规则，每条至少一个用例：

| 条件 | 期望 `kind` |
| --- | --- |
| `veto = zero_hit` / `type_incompatible` | `reject` |
| `band = high` + `fit ≥ 0.8` | `accept` |
| `band = high` + `fit < 0.5` + `matched == sampled` | **`conflict`** |
| `band = middle` | `lean_accept` |
| `band = low` | `review` |
| `band = rejected` | `reject` |
| 无 `confidence` | `none` |

- 判据：7 条各一个断言；⛔ 不许「引用了规则表但只实现一部分」

### AC-003 · 冲突必须显式，不许替人决定

- `matched == sampled` 且 `semantic_fit < 0.5` ⇒ `kind = conflict`
- 判据：构造这样一条 ⇒ 不是 `accept`、不是 `reject`
- 判据：该条**不进入**「按建议批量接受」的白名单

### AC-004 · 零模型调用

- 整个建议链路上**不得**出现 LLM 调用
- 判据：清空所有 `ONTOK_LLM_*` 后起 `review serve`，`/api/review` 正常返回且每条带 suggestion；token 消耗 **0**

### AC-005 · 建议不写库

- 打开界面 / 拉 `/api/review` 之后，库中 `proposal_items` 的任何列**均无变化**
- 判据：两次 `review show` 输出逐字相同；`decided_at` / `via` 保持 NULL

### AC-006 · 批量「按建议接受」必须预览 + 白名单

- 预览接口返回 `{count, items[]}`，`items` 为**建议为 accept/lean_accept 且未签**的序号
- 页面必须先显示条数，确认后才逐条 POST（沿用 REQ-111 唯一写点）
- 判据：不存在任何「批量写」端点；`items` 之外的序号被 POST 时不影响建议集合

### AC-007 · `none` 组不参与批量

- 无置信度的条目（实体）**不得**出现在「按建议接受」的白名单里
- 判据：预览 `items[]` 中不含任何 `confidence = null` 的序号

### AC-008 · 页面展示

- 每条显示：建议标签 + 依据文案（≤40 字）
- 「冲突」单独分组，排在列表最前
- 判据：页面 HTML/JSON 含 `suggestion` 字段；分组计数与后端一致

### AC-009 · 与评分卡不漂移

- 建议规则用的档位/权重常量与 REQ-108 的评分卡**同源**（同一个模块导出）
- 判据：代码审查 + 一条测试：把某条的 `band` 人为改掉，建议必须跟着变

### AC-010 · 不退化

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `src/server/http.rs`（serve） |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、新增依赖 |
| 基线 | REQ-111 合并后的 lib/doc/套件数字，一条不许少 |

## 二 · 纪律

- **H** 建议只能有一个实现（不许页面里再算一遍）
- **G** 变异三元组；编译失败不算执行
- **S** AC 也会错，发现直接说

---

## 三 · Dev 交付后的裁定

### A-1 · 我写的论据是错的（Dev 抓到）

demand §2 原写「REQ-109 实测过 3 条 5/5 命中却被 AI 判 0.0」。
**BA 独立复核（`code/out/*.json`）**：

```
fit 分布（81 条）：1.00 × 51 · 0.50 × 14 · 缺失 × 13 · 0.00 × 3
真「冲突」（matched==sampled 且 fit<0.5）：0 条
fit=0.0 的三条：全部 0/5 命中 + 硬 veto（AI 与证据一致，不是冲突）
```

⇒ **该现象在当前数据集复现不出来**。那是一次性观察（REQ-109 的旧库/旧采样），我把它当成了可复现事实写进需求。
**已订正 demand §2**，并登记 **G1：冲突规则无真实样本，只由构造样本验证**。

**规则本身保留**：它是**行为守卫**——真出现「证据与语义相反」时，程序不许替人决定。但交付说明里 ⛔ 不许写成「已用真实数据验证」。

### A-2 · 规则 ⑧（高档但 AI 未给判断）保持 `review`

Dev 问：高档（fit 缺失或 ∈[0.5,0.8)）共 14 条，是判「人工核对」还是直接「接受」？

**裁定：保持 `review`（建议抽查）。**
理由：**高分不等于已被语义确认**。这 14 条的分数由「结构 + 采样」支撑，AI 那一路没有说话或说得含糊——这恰恰是需要人看一眼的情形。
若改判 accept，52 条里 `review` 会从 14 变 0，等于宣称「高分自动可信」——与引入 AI 语义的初衷相反。

### A-3 · 两处「无真实样本」的如实登记

| 项 | 处理 |
| --- | --- |
| 冲突分组在真实数据上是空组 | 交付物必须写明「本数据集无冲突样本」；⛔ 不许把空组说成「已验证」 |
| 页面「冲突排最前」只有源码级断言 | 如实登记「无浏览器验证」（本机无冲突数据） |

### A-4 · 两处红灯的归因（QA 必须独立复核）

| 灯 | Dev 的归因 | 要求 |
| --- | --- | --- |
| `test_retained_names_req084` 跑不了 | `include_str!` 引 `ontok-gate-preflight.rs`，绕行挪开时必然编不过 | 既有问题，**不算退化** |
| `test_agent_orchestration::test_cli_end_to_end_...byte_identical` 1 红 | A/B 实测：`src/` 还原到基线 `4178a4e` 后同一行同样红；成因是本机无 `ONTOK_SOURCE_DATABASE_URL`，采样默认开 ⇒ 关系条目落 pending | **QA 必须自己 A/B 复现**，不许采信 |
