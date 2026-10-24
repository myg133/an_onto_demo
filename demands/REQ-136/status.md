# 状态：REQ-136 · 本体服务模式

## 当前状态

**进行中**（2026-10-24 · P0 紧急）

## 为什么要它

外部数字员工平台依赖我们的**本体接口 + skill + TTL**，而他们的本体来自 **Ops360 文档**——
**Ops360 没有关系库**。

我实测了三条路：

| 尝试 | 结果 |
| --- | --- |
| `query serve` 无源库 | ❌ 拒启（启动必须连源库） |
| `query serve` + 空 schema | ❌ 拒启（schema 一列都没读到） |
| `query serve` + **合成 schema** | ⚠️ 能起，但 `catalog: relations=0 / excluded_relations=10`、`describe` 名字为空 |

**根因**：catalog/describe 走 schema + R2RML 对账，关系要能映射成 join 才列；
文档类本体**只有语义、没有 join 列** ⇒ 关系被全排掉。

⇒ 对方会拿到「没有关系的本体」，**比不给更糟**。

## 要做

```bash
ontok query serve --ontology-only --project ops360 --version 2 --tokens ... --allow-public
```

| 端点 | 该模式行为 |
| --- | --- |
| catalog | 直接读快照，**关系照列**（标 `joinVerified` 等级） |
| describe | display_name 正常 |
| ontology.ttl | 与 CLI 逐字节相同 |
| sparql | **503 `NO_DATA_SOURCE`**（不是 500、不是空结果） |
| metrics | 同 REQ-135 |

## 五条裁定

| # | 裁定 |
| --- | --- |
| R1 | `--ontology-only` 与 `--schema` 互斥（同时给 ⇒ 报错） |
| R2 | 该模式 ⛔ 不许任何请求尝试连源库 |
| R3 | catalog **必须列关系**（本需求存在的理由） |
| R4 | SPARQL ⇒ **503**（「没有数据源」与「查不到」必须区分） |
| R5 | 普通模式**零行为变化** |

## 待办

- [ ] Dev
- [ ] QA（重点：catalog relations=10、SPARQL 503、普通模式回归）
- [ ] 部署（等 ACR 路径）
