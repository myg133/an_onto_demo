# 状态：REQ-121 · R2RML 映射生成

## 当前状态

**已完成**（2026-10-23 · 已合并 `develop` `e8f21c8`）

## 为什么需要它

```
本体 TTL（类/属性/IRI 模板/主键/range/租户列）
        +
join 证据（true / inherit / false）
        ↓  ← 缺的就是这一层
   R2RML 映射
        ↓
   表 / 列 / joinCondition
        ↓
   SPARQL 引擎（REQ-122）
```

## 核心裁定：证据等级决定生成什么

| `joinVerified` | R2RML 处理 |
| --- | --- |
| `true` | 生成 `rr:joinCondition` + 证据注释 |
| `inherit` | **默认不生成**；`--include-inherit` 时生成且带 `ontok:joinState "inherit"` |
| `false` | ⛔ **永不生成 join**，只在文件尾注释列候选 |

> **把犹豫藏起来，数字员工就会把犹豫当结论。**

## 待办

- [ ] Dev 实现
- [ ] QA
- [ ] 真实库 96 个 TriplesMap + rdflib 验证


## 最终结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`e8f21c8`**（5 个提交）；禁改面零 diff |
| 生产口径 | `anspire_r2rml_excl` v2（84 类 / 78 关系 / true 70 · inherit 1 · false 7） |
| R2RML | **84 TriplesMap / 952 datatype POM / 70 join / 70 joinCondition** / 5824 triples（无 tenancy） |
| 类集一致性 | R2RML `rr:class` 84 == 建模视图 TTL 84，**双向差集 0** |
| 中文 IRI | **非 ASCII IRI 0 处**（中文只在 label/注释，无 BOM、纯 LF） |
| 排除表 | 12 张被排除的表**`rr:tableName` 计数全为 0**，只在文件尾点名 |
| 对账 | child/parent 列必须存在；变异（列加 `_x`）⇒ **exit 1、stdout 空、不落文件、70 处逐条点名** |
| 确定性 | 同参数两次 sha256 相同 |

## 证据等级 → 生成策略（本需求的核心）

| `joinVerified` | 生成 | 结论 |
| --- | --- | --- |
| `true` | ✅ joinCondition | 70 条进映射 |
| `inherit` | ❌ 默认 / `--include-inherit` 才生成 | 1 条（`api_call_logs → licenses`） |
| `false` | ⛔ **永不生成** | 7 条，文件尾逐条点名原因 |

**已签 strong 72 = 在视图 70（生成 join） + 端点被排除 2（点名跳过）** —— 等式两边都能现算。

## 两条「判据没守住」的修补（H-1/H-2）

| # | 问题 | 修法 |
| --- | --- | --- |
| **H-1** | 三条对账 guard（child 存在 / parent 存在 / 证据表==终点）**删掉 34 条测试全绿** | 补 3 条夹具单测 + **逐条变异证明有牙**（M1/M2/M3 各自的红行原文见 QA 报告） |
| **H-2** | `.scratch/` 未进 `.gitignore`（1.4 MB 真名 TTL） | 加入忽略；`git status --ignored` 只剩 `.scratch/` / `out/` / `target/` |

## 附带发现

- **旧快照的 52 条 `false` 其实是 BOOLEAN 遗留**（`join_verified='false'` 但 `join_evidence` 全空）⇒ 判为「未标」而非 false。新旧对照：旧 `false 0 / 未标 52 / join 0`；新 `false 7 / 未标 0 / join 70`。
- `--exclude` 在 R2RML 层**只做核对不删 TriplesMap**——删除必须发生在快照构建，否则映射与本体 IRI 集会不一致。
