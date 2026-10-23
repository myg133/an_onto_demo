# 验收标准：REQ-121 · R2RML 映射生成

> **核心**：R2RML 是本体与数据库之间**唯一的桥**。桥错了，SPARQL 查出来的就是别的东西——而且**不会报错**。

## 一 · AC

### AC-001 · 每个类一个 TriplesMap，且三件套齐全

- `rr:TriplesMap` 数 = 建模视图里的类数（当前 96）
- 每个 TriplesMap 必须有：`rr:logicalTable`（含 `rr:tableName`）、`rr:subjectMap`（含 template + `rr:class`）、≥1 个 `rr:predicateObjectMap`
- 判据：结构断言逐个数；⛔ 不许有「只有 subjectMap 没有 predicateObjectMap」的空壳

### AC-002 · subject template 与本体 IRI 模板一致

- `rr:subjectMap` 的 `rr:template` 必须由类的 `ontok:instanceIri` 展开而来（把 `{id}` 换成 `{主键列}`）
- 判据：抽查 10 个类，template 里出现的列名 = 该类的主键列
- 判据：**所有** template 能被 rdflib 解析（IRI 合法）

### AC-003 · 数据类型属性 → 列 + xsd 类型

- 每个数据类型属性一个 `rr:predicateObjectMap`：`rr:predicate` / `rr:columnName` / `rr:datatype`
- `rr:datatype` 必须与 TTL 里的 `rdfs:range` **一致**
- 判据：抽样 10 条，逐条与 TTL 对账；不一致 ⇒ 0

### AC-004 · `true` 边 → joinCondition

- 每条 `joinVerified "true"` 的对象属性生成 `rr:objectMap` + `rr:joinCondition`
- `rr:child` = 子表外键列，`rr:parent` = 父表目标列（**可能是唯一列，不一定主键**）
- 判据：joinCondition 数 = `true` 边数（72）
- 判据：**与 schema 对账**：child 列必须在子表、parent 列必须在父表（列名逐字相等）
- 判据：每条 join 附带一行注释，写明 `joinEvidence`

### AC-005 · `inherit` 边默认**不生成**

- 默认：`inherit` 边**不出现**在 mapping 里
- `--include-inherit`：生成，但该 `rr:predicateObjectMap` 必须带 `ontok:joinState "inherit"` + `ontok:joinEvidence`
- 判据：两种跑法各一次，比对 joinCondition 数（默认 = 72；带 flag = 73）

### AC-006 · `false` 边**永不生成 join**

- `false` 边在任何模式下都**不产生** `rr:joinCondition`
- 判据：默认与 `--include-inherit` 两种跑法里，`false` 边的 join 数都是 **0**
- 要求：文件尾部有一段注释，列出被排除的 `false` 边 + 原因（供人排查）

### AC-007 · 租户列写进 mapping

- 类的 `ontok:tenantColumn` 出现在该 TriplesMap 上（自定义谓词或注释）
- 判据：带租户列的类（43 个）都有；REQ-122 将据此注入过滤

### AC-008 · 与 schema 对账（**防静默错误**）

- 所有 `rr:tableName` 必须是 `schema.json` 里存在的表
- 所有 `rr:columnName` / `rr:child` / `rr:parent` 必须是该表里存在的列
- 判据：对账 0 失败；失败时**点名**（表.列）

### AC-009 · rdflib 解析通过

- 生成的 `.ttl` 能被 rdflib 解析（0 错误）
- 判据：`rdflib.Graph().parse(...)` 成功 + triple 数

### AC-010 · 确定性

- 同 project+version 两次导出 ⇒ **逐字节相同**
- 判据：sha256

### AC-011 · 复合主键与视图

- 复合主键的类：**跳过**并列入「未生成清单」（R2RML 模板要拼多列，本轮不做）
- 判据：清单里每行有类名 + 原因

### AC-012 · 不退化 / 零依赖 / 出口登记

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` G1–G4 + `/api` 鉴权与路由 |
| ⛔ 禁 | 删测试、`#[ignore]`、放宽断言、**新增依赖**（R2RML 手写 Turtle，不引 rdflib 到 Rust 侧） |
| 出口 | 登记进 `OUTLETS`（内部轨） |
| 基线 | REQ-120 合并后的 lib 414 / doc 4 |

## 二 · 纪律

- **H** R2RML 生成只许一处实现
- **G** 变异三元组：把 `false` 边也生成 join / 把 `inherit` 当 `true` / 删 schema 对账，每条必须被抓
- **S** AC 也会错，发现直接说
