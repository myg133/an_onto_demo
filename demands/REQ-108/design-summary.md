# 设计概要：REQ-108 · 带证据与置信度的关系抽取

> 配套：`demand.md`（范围与裁定）· `acceptance.md`（13 条 AC）
> 一句话：**把「这条关系有多可信」从口头判断变成一个可复算的数，并用它决定人该先看哪几条。**

---

## 一 · 总体形状

```
introspect（补采 PK/唯一索引）
      │
      ▼
propose：规则抽候选关系
      │
      ├──▶ sampler（只读采样 5 行）──▶ 数据信号
      │
      ▼
confidence（纯函数）──▶ score + method + signals + veto
      │
      ▼
分档路由 ──▶ 提案（≥0.30） / 待查清单（0.30–0.60） / 不提案（<0.30）
      │
      ▼
import → 双轨审核（按档折叠/逐条）→ apply → 冻结
```

**置信度不改变抽取结果，只改变「呈现与审核顺序」。**

---

## 二 · 数据模型

### 2.1 评分结果（内嵌在提案条目里）

```json
"confidence": {
  "score": 0.87,
  "method": "evidence-scorecard-v1",
  "band": "middle",
  "signals": [
    {"name": "name_match",        "value": 1.0, "weight": 0.20},
    {"name": "type_compatible",   "value": 1.0, "weight": 0.10},
    {"name": "target_is_key",     "value": 1.0, "weight": 0.10},
    {"name": "sample_hit_rate",   "value": 0.8, "weight": 0.45,
     "sampled": 5, "matched": 4, "sample_digest": "sha256:…"},
    {"name": "not_all_null",      "value": 1.0, "weight": 0.15}
  ],
  "veto": null,
  "data_evidence": "sampled"
}
```

**硬约束**：

| 字段 | 约束 |
| --- | --- |
| `method` | **必填**。缺失 ⇒ import 拒绝（AC-008） |
| `score` | `0.0–1.0`，两位小数足够 |
| `signals[].weight` | 必须与 `method` 版本对应的权重表一致（可复核） |
| `sample_digest` | 样本的值哈希，**不是原值**（AC-003） |
| `data_evidence` | `sampled` / `none`（`--no-sample` 时） |

### 2.2 落库

| 表 | 新增列 | 说明 |
| --- | --- | --- |
| `proposal_items` | `confidence REAL` / `confidence_method TEXT` / `confidence_signals JSONB` | 审核界面与校准都读它 |
| `link_types`（快照） | 同上三列 | 让 `--model-view` 能显示；**客户视图不读这三列** |

迁移：`ALTER TABLE … ADD COLUMN IF NOT EXISTS`（与既有迁移同风格），默认 NULL ⇒ 老数据不受影响。

---

## 三 · 评分卡 v1（`evidence-scorecard-v1`）

### 3.1 信号

| 信号 | 计算 | 取值 |
| --- | --- | --- |
| `name_match` | 列名 == `目标表 + "_id"` → 1.0；在泛化名单（`batch_id` / `session_id` / `request_id` / `task_id` / `group_id` / `template_id` / `product_id` / `channel_id` / `policy_id` / `auth_id` / `related_id` / `business_ref_id` / `external_task_id`）→ 0.3；其它 → 0.6 | 0.3 / 0.6 / 1.0 |
| `type_compatible` | 类型族相同 → 1.0；`num`↔`str` → 0.2；其它 → 0.0 | 0 / 0.2 / 1.0 |
| `target_is_key` | 主键 → 1.0；单列唯一索引 → 0.9；普通列 → 0.4 | 0.4 / 0.9 / 1.0 |
| `sample_hit_rate` | 5 行命中率查表：5/5→1.0 · 4/5→0.8 · 3/5→0.5 · 2/5→0.3 · 1/5→0.15 · 0/5→0.0 | 0–1 |
| `not_all_null` | 有非空值 → 1.0；全空 → 0.0 | 0 / 1.0 |
| `semantic_fit` | **v1 不接**（AI 预留位，权重不参与归一化） | — |

**为什么命中率查表而不线性**：5 样本下 `1/5` 与 `4/5` 的语义差远大于 0.2；`0/5` 是硬反驳。

### 3.2 权重

| 信号 | 权重 |
| --- | --- |
| `sample_hit_rate` | **0.45** |
| `name_match` | 0.20 |
| `not_all_null` | 0.15 |
| `type_compatible` | 0.10 |
| `target_is_key` | 0.10 |

> 数据证据 0.60 / 结构证据 0.40。依据：列名推断实测只有 58% 准确，不能让名字主导。

### 3.3 计算

```
可用信号 = { 权重保留且值可得的信号 }
基线分   = Σ(wᵢ·sᵢ) / Σ(wᵢ)          ← 只对可用信号归一化
最终分   = min(基线分, 所有触发 veto 的上限)
band     = ≥0.90 高 | ≥0.60 中 | ≥0.30 低 | 否则 拒绝
```

### 3.4 veto（优先于加权和）

| 触发 | 上限 | 信号名 |
| --- | --- | --- |
| 类型族不相容 | 0.15 | `type_incompatible` |
| 采样 0 命中 | 0.20 | `zero_hit` |
| 泛化列名且无数据证据 | 0.40 | `generic_name_no_evidence` |
| 源列全空 | 0.45 | `all_null` |
| 部分命中（0<rate<1） | 0.75 | `partial_hit` |
| 复合键（R2） | 0.50 | `composite_key_unsupported` |
| 多态目标（R3） | 0.40 | `polymorphic_target` |
| 自引用且目标非唯一（R4） | 0.50 | `self_ref_non_unique` |

**多个 veto 触发时取最小上限。**

---

## 四 · 采样器

| 项 | 规定 |
| --- | --- |
| 会话 | `SET default_transaction_read_only = on`；只发 `SELECT` |
| 取样 | `SELECT col FROM t WHERE col IS NOT NULL ORDER BY <主键> LIMIT 5` |
| **确定性** | 固定排序、固定上限 ⇒ 同数据同结果；⛔ 禁止 `random()` / `TABLESAMPLE` |
| 去重 | 取到的值去重后再补足 5 个 |
| 命中判定 | `SELECT COUNT(*) FROM 目标 WHERE 键列 = ANY($1)` |
| 落盘 | 只落 `sampled` / `matched` / `sample_digest`（`sha256(值排序后用 \u001f 连接)`） |
| 关闭 | `--no-sample` ⇒ `data_evidence: "none"`，`sample_hit_rate` 不参与归一化，且**不得因此把分数抬高**（缺少证据 ≠ 有证据） |
| 超时 | 单条关系采样超时（如 2s）⇒ 记 `sampled: 0` + `reason: timeout`，不阻塞整体 |

---

## 五 · 分档路由

| 档 | 分数 | 去向 | 审核界面 |
| --- | --- | --- | --- |
| 高 | ≥0.90 | 提案 | 默认折叠，一键全接受 |
| 中 | 0.60–0.90 | 提案 | 逐条展开 |
| 低 | 0.30–0.60 | **待查清单**（不进提案正文） | 折叠区，标注「证据不足」 |
| 拒绝 | <0.30 | **不提案** | 只在「被拒绝」列表，附 veto 原因 |

> 客户视图只取「高 + 中」。低与拒绝**不进客户视图**（R7）。

---

## 六 · 校准

| 项 | 做法 |
| --- | --- |
| 记录 | 每条关系的 `score` + 人工最终决定（accept / reject / edit） |
| 分箱 | 0.0–0.3 / 0.3–0.6 / 0.6–0.75 / 0.75–0.9 / 0.9–1.0 五档 |
| 输出 | 每档「条数 / 人工接受数 / 接受率」+ ECE |
| 样本不足 | 任一分箱 < 5 条 ⇒ 标「样本不足」，不输出结论 |
| 用途 | 权重与 veto 上限的调整**必须有校准数据支撑**，不许凭感觉调 |

命令（建议）：`ontok confidence calibrate --project X`

---

## 七 · 代码落点

| 文件 | 职责 | 测试 |
| --- | --- | --- |
| `src/extract/sample.rs` | 只读采样 + digests | 集成（需 DB） |
| `src/domain/confidence.rs` | **纯函数**评分卡（信号→分数），零 I/O | 单元（构造样本） |
| `src/extract/introspect.rs`（改） | 补采 `primary_key` / `unique_columns` | 集成 |
| `src/extract/propose.rs`（改） | 组装 confidence、分档路由 | 集成 |
| `src/db/pg.rs` / `snapshot_store.rs`（改） | 迁移 + 读写 confidence | 集成 |
| `src/cli.rs`（改） | `--no-sample` / `confidence calibrate` / 审核界面分档 | 端到端 |
| `tests/test_req108_*.rs` | 见 AC | — |

**关键设计约束**：`confidence.rs` 必须是**纯函数**（无 DB、无时间、无随机）——这样 AC-005 的「逐位相同」才有结构保证，而不是靠约定。

---

## 八 · 实施阶段（Dev 按此分提交）

| 阶段 | 内容 | 验收挂点 |
| --- | --- | --- |
| 1 | introspect 补采 PK/唯一索引 | AC-001 |
| 2 | 采样器 + 确定性 + 不落原值 | AC-002 / AC-003 |
| 3 | 评分卡纯函数 + 单元测试（含 veto 表） | AC-004 / AC-005 |
| 4 | 接线：propose 组装 + 落库 + method 必填 | AC-008 |
| 5 | 分档路由 + 客户视图隔离 | AC-009 / AC-010 |
| 6 | 校准命令 | AC-011 |
| 7 | 真实库实测 + 9 条错误验证 | AC-006 / AC-007 |

---

## 九 · 已知缺口（本轮不做，登记）

| # | 缺口 | 影响 | 下一轮 |
| --- | --- | --- | --- |
| G1 | 复合键只降级不支持 | 复合外键关系会落「低」 | 独立需求 |
| G2 | 多态列只识别不解析 | `target_type + target_id` 关系不可用 | 独立需求 |
| G3 | 无 AI 语义项 | 语义合理但数据无证据的关系拿不到分 | REQ-109 候选 |
| G4 | 采样只验「存在性」，不验「覆盖率」 | 值域稀疏时可能低估 | 可扩展 |
| G5 | 关系方向仍由列名猜（`belongs_to` / `has`） | 动词层仍单薄 | 与 REQ-109 合并考虑 |
