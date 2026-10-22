# 状态：REQ-120 · 关系推断改进（FK 边补全）

## 当前状态

**已完成**（2026-10-22 · 已合并 `develop` `e73d5e9`）

## 最终结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`e73d5e9`**；禁改面零 diff |
| 关系总数 | 52 → **79**（73→80→79，撤销 1 条租户键 inherit） |
| join 三态 | `true` **144**（72 边 × 正反）· `inherit` **2**（1 边）· `false` **12**（6 边） |
| `joinEvidence` | **158 条 = 每条边都有**（`insufficient_sample:` 6 块 / `unsigned:` 6 块 / 纯名字推断 **0**） |
| FK 提案 | 91 条（strong 72 · medium 1 · inherit 18）· 未解析 53 列（`close()` 闭合） |
| 1:1 | 8 对（REQ-119 的 6 条 + `sub_account_permission.sub_customer_id` + `llm_model_peak_config.model_id`） |
| 性能 | FK 阶段 ~43 s，全量 56–104 s |

## 四轮迭代（每一轮都被证据逼出来）

| 轮 | 触发 | 改动 |
| --- | --- | --- |
| rev.1 | 首版 | 值域重叠 + 52→73 |
| rev.2 | **替身实验 Q4 写不出** | E-1 join 目标扩到唯一列（`llm_model` 8 条）+ E-2 `inherit`（`api_call_logs`） |
| rev.3 | **替身第二次推翻 `inherit`** | F-1 租户键禁继承（`deductions` 同表两个 tenantKey 证明「叫 customer_id ≠ 同一实体」）+ F-2 未验证边必须写原因 |
| rev.4 | **QA 发现签字路径可绕过** | G-1 `load_signed`/`inject_signed` 双闸 fail-closed |

## 三条值得写进 ADR 的经验

1. **`true` 是抽样证据，不是统计保证** —— 已写进 TTL 文件头第 17 行（每列取 ≤5 个不同值比对目标键）。
2. **能被文本绕过的规则不算规则** —— 安全闸必须在**每一层**（生成 / 签字 / 注入）；只挡生成等于没挡。
3. **缺一条边比给一条没依据的跨租户边安全** —— 租户键过滤宁可缺边；缺的边仍在 `proposal-pending.json` 里可人工认领。

## 数据事实（决定了哪些边验不了）

```
deductions      = 4 行      ⇒ insufficient_sample
api_call_logs   = 0 行      ⇒ 无法验证
usage_details   = 216 行    ⇒ 可验证
balances        = 17935 行  ⇒ 可验证（1:1）
```

**钱相关的那两张表在 UAT 里几乎是空的** ⇒ 它们的边无法用值域重叠验证。生产有数据即可验。
