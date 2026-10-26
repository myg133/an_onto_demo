# REQ-157 · 关系动词治理：字典 + 校验 + skill/schema 对齐 + 重抽 v4

- 状态：**已就绪**
- 提出：2026-10-27
- 关联：REQ-156（透出语义）、REQ-158（修 fail-open）、REQ-147（抽取质量）
- 前置：**REQ-158 必须先完成**（闸修好，重抽才有意义）

---

## 一 · 问题

v3 的 34 条关系用了 **15 种动词**，其中：

| 类别 | 例子 | 问题 |
| --- | --- | --- |
| **同义并存** | `attached_to` / `attaches_to` · `binds` / `binds_to` | 同一件事两种写法 |
| **泛动词** | `uses` / `contains` / `associated_with` | 几乎每句话都有，等于没抽 |
| **口径冲突** | skill 说"用具体动宾短语"，`parse.rs` 的 schema 示例却写 `has 或 belongs_to 或 references` | agent 两头都看了 ⇒ 自由发挥 |

**根因不在 UI**：是**抽取契约的两份文件互相打架**，加上**没有词汇表可校验**。

---

## 二 · 目标

> **动词来自一张受控的字典；字典外的词不许进本体；同义词归一；泛词归零。**

---

## 三 · 动词字典（`relation-verbs.json`，随仓库发布）

```json
{
  "version": "1.0.0",
  "verbs": {
    "belongs_to":   { "label": "归属于", "aliases": ["belong_to", "owned_by"] },
    "attached_to":  { "label": "挂载于", "aliases": ["attaches_to", "attached", "mounts_to"] },
    "binds_to":     { "label": "绑定到", "aliases": ["binds"] },
    "references":   { "label": "关联到", "aliases": ["refers_to"] },
    "notifies_via": { "label": "通知到", "aliases": ["notifies"] },
    "applies_to":   { "label": "适用于", "aliases": ["applied_to"] },
    "forwards_to":  { "label": "转发到", "aliases": ["forwarded_to"] },
    "targets":      { "label": "指向", "aliases": [] },
    "bills":        { "label": "计费到", "aliases": ["billed_to"] },
    "based_on":     { "label": "基于", "aliases": [] },
    "deployed_to":  { "label": "部署于", "aliases": ["deploys_to"] },
    "has":          { "label": "拥有", "aliases": [] },
    "depends_on":   { "label": "依赖", "aliases": ["depends"] },
    "records":      { "label": "记录了", "aliases": [] },
    "derived_from": { "label": "算自", "aliases": [] },
    "contains":     { "label": "包含", "aliases": [], "deprecated": "泛词；仅保留以兼容 v3" }
  },
  "forbidden": ["uses", "associated_with", "related_to", "involves", "has_many", "is_related"]
}
```

**规则**：
- `aliases` 命中 ⇒ **归一**成主 key（`attaches_to` → `attached_to`）
- `forbidden` 命中 ⇒ **拒绝**（fail-closed），并点名
- 主 key 之外、别名之外、且不在 forbidden ⇒ **`unknown`**：拒绝并点名，同时给出"最接近的 3 个候选"（编辑距离，便于人改）
- `deprecated` ⇒ 允许存在但**每次构建告警**（v4 之后应清零）

---

## 四 · 范围（三期）

### P1 · 字典 + 校验命令（代码）

| # | 做什么 |
| --- | --- |
| 1 | `relation-verbs.json` 入库（内容见 §三） |
| 2 | 新增 `ontok-builder relation verbs check --mapping/--snapshot` ：读本体里的 `semantic`，输出三类问题：`unknown` / `forbidden` / `alias_normalized` |
| 3 | 退出码：有 `unknown` 或 `forbidden` ⇒ **非 0**（fail-closed）；只有 `alias_normalized` ⇒ 0 + 提示 |
| 4 | `semantic_cn` 改成**从字典读**（`label`），代码里不再写死映射（⛔ 但要保留一份内置兜底用于无字典场景，行为与现在一致） |

### P2 · skill 与 schema 对齐

| # | 做什么 |
| --- | --- |
| 5 | 删掉 `src/agent/acp/parse.rs:160` 的 `"has 或 belongs_to 或 references"`，改成 `"<从 relation-verbs.json 的 key 里选一个>"`，**不带枚举** |
| 6 | `skills/ontok-extract/SKILL.md` §三 加一节「**关系动词从字典选**」：给出 16 个动词 + 中文标签 + ⛔ forbidden 清单 + 「同义用别名，别自己造」 |
| 7 | `skills/ontok-build/SKILL.md` 补一句：构建前先跑 `relation verbs check`，非 0 不许冻结 |

### P3 · 重抽 v4（**产出提案，不代签**）

| # | 做什么 |
| --- | --- |
| 8 | 用新 skill + 字典重跑文档抽取，产出**命名提案** |
| 9 | 提案里每条关系必须带 `evidence`（复用 `locate_quote`） |
| 10 | 跑 `relation verbs check` ⇒ 必须 exit 0 |
| 11 | **停在"待审"**：⛔ 不代签 `reviewed_by`，⛔ 不冻结 v4 |

---

## 五 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | `relation-verbs.json` 能加载；缺字段/重复别名/别名与主键冲突 ⇒ **拒绝加载并点名** |
| AC-002 | 造 `uses` ⇒ `forbidden` 命中且 **exit 非 0** |
| AC-003 | 造 `attaches_to` ⇒ 归一成 `attached_to`，**不报错**，报告里出现 `alias_normalized` |
| AC-004 | 造 `xyz_unknown` ⇒ `unknown` + **给出 3 个候选** + exit 非 0 |
| AC-005 | `semantic_cn` 从字典读之后，**REQ-156 的 15 个动词标签一个不变**（`test_req156_semantic_cn_invariants` 仍全绿） |
| AC-006 | `parse.rs` 的示例里**不再出现** `has 或 belongs_to 或 references`；schema 里能看出"要查字典" |
| AC-007 | skill 里能查到 16 个动词 + forbidden 清单（人读一遍就懂，不看代码） |
| AC-008 | 重抽提案：34 条关系里 `unknown` = **0**、`forbidden` = **0**、同义并存 = **0**（`attached_to`+`attaches_to` 不许同时出现） |
| AC-009 | 提案**未被签名**（`reviewed_by` 全空），且 v3 数据**一个字节没动** |

---

## 六 · 不做

| 不做 | 理由 |
| --- | --- |
| 冻结 v4 / 发布 v4 | **要人签**，BA 不代签 |
| 改已冻结的 v3 | 版本不可变 |
| 增加动词到 20+ | 先收敛；字典能扩，但要人审 |

---

## 七 · 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 字典太小，重抽时大量 `unknown` | AC-008 允许"有 evidence 的命名 + 无证据标 unmapped"，但**字典外的词一律拒绝**；真不够就扩字典（人审） |
| 2 | 泛词拒绝后关系变少 | 那是**对的**：`uses` 这类关系本来就没有信息量 |
| 3 | `semantic_cn` 改从字典读会动红线（`view.rs`） | 沿用 REQ-156 的授权 + 专属守卫；**改动仍必须是"只加不删"式**，否则守卫会红 |
