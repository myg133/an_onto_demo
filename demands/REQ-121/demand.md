# 需求：REQ-121 · R2RML 映射生成

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-121 |
| 优先级 | **P0**（数字员工链路的第二块地基） |
| 状态 | 进行中 |
| 基线 | `develop` `e73d5e9` |
| 上游 | 用户确定的架构：TTL → Agent 写 SPARQL → **服务端按 R2RML 查询** → Agent 解释 |

## 需求描述

有 TTL（本体）和 join 证据（REQ-120），但**没有把两者连到关系库的映射**。

R2RML（RDB to RDF Mapping Language）就是这层桥：

```
本体 IRI  ──R2RML──▶  表 / 列 / join
   :deductions            rr:tableName "deductions"
   :deductions__customers rr:joinCondition [child "customer_id"; parent "id"]
```

### 输入

| 输入 | 来源 |
| --- | --- |
| 本体（类 / 属性 / IRI 模板 / 主键 / range / 租户列） | TTL（REQ-119/120） |
| join 证据等级 | `ontok:joinVerified`（true / inherit / false） |
| 真实列类型 | `schema.json`（可选，用于校验列存在） |

### 输出

一份 R2RML Turtle 文件，能被 SPARQL 引擎（Ontop / Morph-RDB / 自研）消费。

## 关键裁定（**证据等级决定生成什么**）

| `joinVerified` | R2RML 里怎么处理 |
| --- | --- |
| **`true`** | 生成 `rr:predicateObjectMap` + `rr:joinCondition`（带证据注释） |
| **`inherit`** | **默认不生成**；`--include-inherit` 时生成，但**必须**带 `ontok:joinState "inherit"` + `ontok:joinEvidence` |
| **`false`** | ⛔ **永不生成 join**；只在文件尾部的注释里列出候选（供人排查） |

> **理由**：把犹豫藏起来，数字员工就会把犹豫当结论。

## 范围

### 做

1. **`ontok r2rml generate --project P --version V --schema S --out mapping.ttl [--include-inherit]`**
2. **每个类**一个 `rr:TriplesMap`
   - `rr:logicalTable [ rr:tableName "..." ]`
   - `rr:subjectMap`：template 取自 `ontok:instanceIri`，类名 `rr:class`
3. **每个数据类型属性** 一个 `rr:predicateObjectMap`：`rr:predicate` + `rr:columnName` + `rr:datatype`
4. **每个 `true` 对象属性** 一个 `rr:predicateObjectMap`：`rr:objectMap [ rr:parentTriplesMap <#Y>; rr:joinCondition [ rr:child "..." ; rr:parent "..." ] ]`
5. **租户列标注**：类的 `ontok:tenantColumn` 写进 mapping（自定义谓词 `ontok:tenantColumn`），供 REQ-122 注入
6. **证据注释**：每条 join 带一行注释写明 `joinEvidence`
7. **文件头**：说明生成来源（项目/版本/快照）、三态处理策略、已知限制
8. 登记进 `OUTLETS`（**内部轨**，不是客户出口）

### 不做

- ⛔ 不实现 SPARQL 引擎（REQ-122）
- ⛔ 不执行任何 SQL
- ⛔ 不做「自动推断 R2RML 中的复杂映射」（如多表 union、视图）——本轮只做 `rr:tableName` + 列 + join
- ⛔ 不引新依赖

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | 生成的 R2RML 语法不合法（手写 Turtle 容易漏） | 用 rdflib 解析验证 + 结构断言（每个 TriplesMap 必须有三件套：logicalTable / subjectMap / 至少一个 predicateObjectMap） |
| 2 | IRI 模板里的中文类名导致 IRI 不合法 | 检查：IRI 必须能被 rdflib 接受；若不行则**百分号编码**并记录 |
| 3 | join 列名写错（大小写/拼写） | 与 `schema.json` 对账：child 列必须存在于子表、parent 列必须存在于父表 |
| 4 | 主键是复合键 | 跳过多列主键的类（R2RML 模板要拼多列，本轮不做），并列入清单 |
| 5 | 客户视图/建模视图混淆 | 只从**建模视图**生成（有租户列、主键、证据） |

## 交付物

1. `ontok r2rml generate` + 生成器
2. 真实库映射文件（约 96 个 TriplesMap）
3. 结构校验报告：TriplesMap 数 / join 数 / 三态分布 / 与 schema 对账结果
4. rdflib 解析通过的证据
5. 确定性（两次 sha256）
6. 已知限制（复合主键、中文 IRI、视图等）
