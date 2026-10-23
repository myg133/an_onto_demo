# 状态：REQ-121 · R2RML 映射生成

## 当前状态

**进行中**（2026-10-23 开单）

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
