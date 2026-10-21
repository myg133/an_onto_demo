# 状态：REQ-112 · 审核界面接入 AI 建议

## 当前状态

**已完成**（2026-10-21 · 已合并 `develop` `e9755e8`）

## 为什么 v1 不调用模型

| 理由 | 说明 |
| --- | --- |
| 成本 | 135 条目每条问一次 ≈ 十几万 token，且**每审一次重算** |
| 幂等 | 确定性推导 ⇒ 同一提案两次打开，建议逐字相同 |
| 可解释 | 每条建议必须指向具体信号（采样/veto/AI fit），不是「模型说」 |

## 核心裁定

| # | 裁定 |
| --- | --- |
| R1 | 建议由 `confidence` **确定性**推导；⛔ 不新增模型调用 |
| R2 | 无置信度（83 实体）⇒ `none`，⛔ 不编建议 |
| R3 | `matched == sampled` 但 `semantic_fit < 0.5` ⇒ **`conflict`**，强制人工 |
| R4 | 建议**只读**：不写库、不新增写端点 |
| R5 | 批量「按建议接受」复用 REQ-111 的预览 + 白名单 + 逐条 POST |
| R6 | 档位常量与 REQ-108 评分卡**同源**，不许页面里再算一遍 |

## 待办

- [ ] Dev 实现（纯函数 + `/api/review` 带 suggestion + 页面）
- [ ] QA

## Dev 交付摘要

| 项 | 结果 |
| --- | --- |
| 7 条规则 + 1 条空格兜底 | 全部实现，21 个测试 |
| 真实数据建议分布（135 条） | **accept 29 · lean_accept 9 · review 14 · conflict 0 · reject 0 · none 83** |
| 52 条关系 | accept 29 · lean 9 · review 14 |
| 零模型调用 | `ONTOK_LLM_*` 清空后正常；源码 8 个 LLM needle **0 命中**（反向自检：同一套 needle 在 `ai_semantic.rs` 命中） |
| 零写库 | `review show` 前后 sha256 逐字相同；`proposal_items` 全列 md5 相同 |
| 测试 | lib 245 / doc 4 / 新增 21；32 基线套件 313 一条不少 |

**BA 订正**：demand 原写「REQ-109 实测 3 条 5/5 却被判 0.0」——**当前数据 0 条**（fit=0.0 的三条全是 0/5）。已订正并登记 G1。

## QA 首轮（`qa-review.md`）· **FAIL（1 阻断）**

| 项 | 结果 |
| --- | --- |
| 7 条规则 | ✅ 全对（QA 自写 17 个探针，不看 Dev 测试名） |
| **AC-003 冲突** | ❌ **不成立**：`fit=0.0` ⇒ 分数 **0.85 / middle**，而 AC-002 要求 `band=high` ⇒ 规则**结构性不可达**；现实现给 `lean_accept` 且**进了白名单** |
| 零 token / 零写库 | ✅ 双向成立（`ai_cache` 81→81；`proposal_items` 全 15 列 sha256 前后相同） |
| 批量白名单 | ⚠️ `by=suggestion` 正确（38 = 29+9）；但 `min_confidence` 路径把 conflict 放进去了（N-2） |
| 两处红灯归因 | ✅ QA 独立 A/B 复现，归因成立 |
| 不退化 | ✅ lib 245 / doc 4 / 32 套件 313 / 43 套件 416 |

**根因是 BA 的规格矛盾**（AC-002 有 band 约束、AC-003 没有）。已裁 A-5：**冲突判断提到档位分派之前，不限档位**。

## 第 2 轮修复与合并（`776f956` → `e9755e8`）

| 项 | 结果 |
| --- | --- |
| **B-1 修复** | 冲突闸门移到 `classify` 的**档位分派之前**（`confidence.rs:1559-1573`）：`sampled>0 && matched==sampled && fit<0.50` ⇒ `conflict`，**不限档位** |
| 判据 1 | 结构全 1.0 + 5/5 + fit=0.0 ⇒ **0.85 / middle / conflict**（不是 lean_accept） |
| 判据 2 | 该条**不在** `by=suggestion` 白名单（`excluded.conflict=1`） |
| 边界 | fit=0.50 ⇒ review；4/5 ⇒ lean_accept；sampled=0 ⇒ reject（0/0 不算全命中） |
| **N-2** | 两条批量路径共用 `batch_gate`（唯一判据 `enters_batch`）；`min_confidence=0.80` 下 conflict 与 review 都被排除 |
| 真实数据影响 | `min_confidence=0.90` 白名单 **43 → 29**（14 条 review 不再被自动接受）；`by=suggestion` 仍 38 |
| 测试 | lib 245 / doc 4 / 43 套件 **422 passed**；基线 313 逐项不变 |
| 合并 | `develop` **`e9755e8`**；禁改面（`server/http.rs` 等 9 个文件）**零 diff**；合并后 `cargo check --lib` ✅ |
| 清理 | Dev 的 3 个临时项目 + QA 的 2 个（`t112`/`t112_low`）已从库里删除 |

**如实登记**：当前数据集**无冲突样本**（冲突条目全为构造样本）；冲突分组「排最前」只有源码级断言、无浏览器验证。
