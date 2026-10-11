# 设计概要：REQ-002 本体内核 MVP（Rust）

> **详版见**：[实施计划](../../../../docs/plans/2026-09-30-REQ-002-kernel-mvp-implementation.md)（24.6KB，11 个任务）
> **范围界定见**：[MVP 范围规划](../../../../docs/plans/2026-09-30-ontology-kernel-mvp.md)
> **规范依据**：[设计规格 §0.2 / §4 / §7.1 / §8.4](../../../../design/ontology-platform-spec.md)

---

## 一句话

把抽取侧产出的**提案**，经过**双轨审核**转成**客户签字的版本**，并让任意两个版本之间**能 diff**。

---

## 核心设计

### 分层

```
src/domain/    纯逻辑，零 I/O   ← 验收 AC-002 用一条 grep 守住
src/store/     PgStore（唯一实现，不抽象）
src/db/        sqlx 行模型
```

**契约是 `Snapshot` 这个类型，不是 trait。** MVP 只有一种存储，所以不做 `Store` 抽象（范围 §3 暂缓项）。

### 快照契约（Task 4，本 MVP 最重要的一个）

```rust
Snapshot {
  version_id, seq,
  extracted_by: { agent, version, round, failed_groups },  // ← 规格 §8.4
  entity_types: [...],   // 含 aliases / parent / disjoint_with
  link_types: [...],
  action_types: [...],   // 切片 1 恒空，但结构必须在
  functions: [...],      // 同上
}
```

**三个刻意的决定：**

| 决定 | 理由 |
|---|---|
| `parent` 存父的 **key** 不是 id | 快照是给人和 diff 用的；存 id 看不出「父从机器人设备变成了设备」 |
| `extracted_by.version` 记**具体版本**不是 "latest" | 供应商偷偷升模型后，diff 变化必须能解释 |
| `failed_groups` 留在快照里 | 静默失败会伪造收敛（规格 §7.1⑤） |

### 占位项（红线，漏一个对应切片就推翻重来）

`ActionType` · `Function` · `WriteBinding`（`value_map` / `supported_values` / `conflict_field` / `key_expr`）· `extracted_by` · `disjoint_with` · `failed_groups`

**验收 AC-003 用 grep 守住**——只建不用容易被后人「清理」掉。

### 状态机口径

| | 通过条件 |
|---|---|
| 生产 | `biz == Accepted && model == Accepted` |
| POC | `biz == Accepted && model ∈ {Accepted, Pending}`（代签） |

**建模轨显式 `Rejected` 永远否决** —— 规格 §8.2。

---

## 与 REQ-001（spike）的关系

spike 用真实库（`<业务库名>`，97 张表）验证了抽取链路：

| 阶段 | 结论 |
|---|---|
| 阶段 1 分组抽取 | ✅ 可外置 |
| **阶段 2 证据验证** | ✅ **不可外包** |
| 阶段 3d 跨组合并 | ✅ 可外置（4/4 判对，含 `t043`/`t044` 语义相反识别） |
| 阶段 3e 跨对象关系 | ✅ 可外置（69 对象下 22/22 连接列验过） |

**本 MVP 承接「阶段 2 那一侧」**：它接收的提案必须已带证据，证据已由抽取侧验证过。

---

## 明确暂缓（**不是永久不做**）

10 项，各标注补回时机，见范围规划 §3。

> ⚠ **不要**把这些搬进设计规格 §11 —— 那是「永久不做」，搬进去等于永久砍掉。

---

## 风险

| # | 风险 | 缓解 |
|---|---|---|
| 1 | 重写引入新缺陷（3600 行旧逻辑无人跑过） | 逐任务对照旧计划定义；每完成一任务跑一次端到端 |
| 2 | serde / sqlx 的 null 语义（`None` / `[]` / `{}` 三态） | AC-010 往返测试，故意放三态 |
| 3 | 契约测试要真 PG | testcontainers 或外部库（**待确认**） |
| 4 | 占位字段被后人删掉 | AC-003 grep 守住 |
| 5 | **MVP 通过 ≠ 客户认** | 内核是通用机制，**不要用 MVP 通过去对客户承诺效果** |

---

## 待确认

- [ ] 测试 PG 方案：testcontainers（需 Docker）还是外部库
- [ ] `ONTOK_TEST_DATABASE_URL` 约定
- [ ] 新版本是否继承 `base` 的 `extracted_by` —— **倾向继承**（新版本是 base 的演进，来源未变，除非新一轮抽取产生新提案）
