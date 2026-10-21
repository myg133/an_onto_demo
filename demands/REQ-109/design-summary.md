# 设计概要：REQ-109 · AI 语义项接入

> 一句话：**AI 只回答一个很窄的问题——「这条候选关系在业务上说得通吗」，并给出引用坐标；其余一切仍由规则与采样决定。**

---

## 一 · 数据流

```
候选关系（规则产出）
   │
   ├─ 结构信号（REQ-108）
   ├─ 采样证据（REQ-108）
   │
   ├─▶ AI 语义判断（本需求）
   │      fingerprint = sha256(relation + schema_slice + prompt + model + agent_version)
   │      cache hit  → 直接取
   │      cache miss → 调模型 → 校验 cites/rationale → 写缓存
   │      ⇒ semantic_fit ∈ [0,1] + rationale + cites
   │
   ▼
评分卡（REQ-108）→ 分档 → 路由
```

## 二 · 指纹与缓存

### 2.1 指纹构成（**缺一项就是静默错**）

```
fingerprint = sha256_hex(
    "v1\n"
  + relation_key + "\n"                  # 哪条关系
  + schema_digest + "\n"                 # 该关系涉及的表/列的规范化描述
  + prompt_sha256 + "\n"                 # 提示词内容哈希
  + model_id + "\n"                      # 具体模型
  + agent_version + "\n"                 # adapter 版本
)
```

`schema_digest` = 对「源表.列 + 目标表.候选列」的名称/类型/注释做**排序后**序列化再哈希（排序保证确定性）。

### 2.2 表

```sql
CREATE TABLE IF NOT EXISTS ai_cache (
    fingerprint   TEXT PRIMARY KEY,
    output_json   JSONB NOT NULL,        -- {semantic_fit, rationale, cites, raw_tokens}
    model         TEXT NOT NULL,
    prompt_sha    TEXT NOT NULL,
    tokens_used   INTEGER,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 2.3 行为

| 情形 | 行为 |
| --- | --- |
| 命中 | 直接用；**不调用模型**；`cache_hit = true` |
| 未命中 | 调用 → 校验 → 写缓存；`cache_hit = false` |
| `--refresh` | 忽略缓存并**覆盖**；打印「强制刷新 N 条」 |
| 调用失败 | 该条 `semantic_fit` = **缺失**（不是 0）；计入失败数 |

## 三 · 评分卡接入（`evidence-scorecard-v2`）

### 3.1 权重（AI 占 15%，其余等比压缩）

| 信号 | v1 | **v2** |
| --- | --- | --- |
| `sample_hit_rate` | 0.45 | **0.3825** |
| `name_match` | 0.20 | **0.17** |
| `not_all_null` | 0.15 | **0.1275** |
| `type_compatible` | 0.10 | **0.085** |
| `target_is_key` | 0.10 | **0.085** |
| `semantic_fit` | — | **0.15** |
| 合计 | 1.00 | **1.00** |

> **为什么是等比压缩而不是「加一项再归一化」**：等比压缩保证「AI 缺失时」重归一化后的相对比例与 v1 **完全一致** ⇒ AC-001 的逐字节相同才成立。

### 3.2 规则

- `semantic_fit` 缺失 ⇒ 不参与归一化（等同于 v1 行为）
- **veto 仍优先**：语义分不能抬高任何被 veto 的条目（AC-007）
- `method` 字段区分 `evidence-scorecard-v1` / `v2`，自证链按 method 取对应权重表

## 四 · AI 协议（窄到不能再窄）

### 4.1 输入（一次一条关系）

```json
{
  "relation": {"from": "llm_model_sort.model_id", "to_candidates": ["llm_model.id", "llm_model.model_id"]},
  "schema_slice": [
    {"table": "llm_model", "columns": [{"name": "id", "type": "uuid", "comment": null},
                                        {"name": "model_id", "type": "varchar", "comment": "模型ID(如: GLM-5)"}]}
  ],
  "task": "选出最合理的 (table, column)，或回答 none"
}
```

### 4.2 输出（严格 JSON）

```json
{
  "choice": {"table": "llm_model", "column": "model_id"} | null,
  "semantic_fit": 0.9,
  "rationale": "目标表另有业务键 model_id，其它表的 model_id 引用它",
  "cites": [{"table": "llm_model", "column": "model_id"}]
}
```

**校验**：`cites` 每项必须存在于 schema；`rationale` 1–60 字；`semantic_fit ∈ [0,1]`；`choice` 必须在**候选集合内**。

### 4.3 提示词

- 固定模板，**内容哈希进指纹**
- 明确要求：只判语义，不许猜数据；不确定就返回 `null` + 低分
- ⛔ 不许把整份 schema 塞进 prompt（83 表会撑爆且不必要）——只给候选表

## 五 · 开关与预算

| 开关 | 默认 | 行为 |
| --- | --- | --- |
| `--llm` | **关** | 开 ⇒ 必须有三件环境变量，否则 exit≠0 |
| `--budget-tokens` | 无（AI 关时合理） | **AI 开时必填且 >0** |
| `--refresh` | 关 | 忽略缓存、覆盖写入 |

## 六 · 代码落点

| 文件 | 职责 |
| --- | --- |
| `src/extract/ai_semantic.rs` | 指纹、缓存读写、调用编排、结果校验 |
| `src/domain/confidence.rs`（改） | `semantic_fit` 接入 + v2 权重表 |
| `src/db/ai_cache.rs` | `ai_cache` 表与迁移 |
| `src/bin/ontok-propose.rs`（改） | 三个开关、调用点 |
| `tests/test_req109_*.rs` | 用 scripted transport，无 key 全绿 |

## 七 · 已知限制（预先登记）

| # | 限制 | 影响 |
| --- | --- | --- |
| G1 | 只判「候选集合内」的选择 | 若正确目标**不在候选里**（如跨命名域），AI 无能为力 |
| G2 | 缓存不校验模型端是否已变 | 供应商静默升级模型时，缓存仍返回旧结果（`model_id` 只防显式切换） |
| G3 | 无「覆盖率」语义 | 仍只判存在性/合理性，不判数据覆盖 |
| G4 | 成本随表数线性增长 | 83 表全量约几十万 token；靠缓存与预算闸控制 |
