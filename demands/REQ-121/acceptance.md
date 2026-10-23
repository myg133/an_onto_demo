# 验收标准：REQ-121 · R2RML 映射生成

> **核心**：R2RML 是本体与数据库之间**唯一的桥**。桥错了，SPARQL 查出来的就是别的东西——而且**不会报错**。

## 一 · AC

### AC-001 · 每个类一个 TriplesMap，且三件套齐全

- `rr:TriplesMap` 数 **== 该快照建模视图的类数**（复合主键类除外，逐个列名列出）
- ⚠️ **不许写死常数**（原写「当前 96」，错：带排除时是 84）
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
- 判据：joinCondition 数 **== 该快照 `joinVerified "true"` 的对象属性数**（⛔ 不写死 72）
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

---

## 三 · Dev 首轮交付与裁定

### Dev 结果

| 项 | 值 |
| --- | --- |
| 提交 | `3c229fe`（接手基线）+ `bf252b7`（false/未标拆分 + 口径） |
| 重建快照 | `anspire_r2rml` v2 冻结（96 对象 / 79 关系 / true 72 · inherit 1 · false 6） |
| R2RML | 95 TriplesMap / 1107 datatype POM / **72 join** / schema 对账 0 失败 |
| rdflib | 6736 triples（默认）/ 6745（含 inherit） |
| 确定性 | sha256 `D3D9F7DD…` 三次相同 |

### A-1 · **BA 的 AC 写死了常数**（订正）

原 AC-001 写「TriplesMap = 96」、AC-004 写「joinCondition = 72」——这两个数**都依赖快照是否带排除**。
**订正为自洽口径**：TriplesMap == 该快照类数；joinCondition == 该快照 `true` 边数。⛔ 验收里不许出现与快照无关的常数。

### A-2 · R2RML **必须只覆盖业务本体**（要改）

Dev 实测：无排除的快照 ⇒ 95 TriplesMap，含 `_volc_dts_pg_ddl_*` / `alembic_version` / `rpt_*` / `*_back` **这 12 张 REQ-113 判定不进本体的表**。

> 数字员工能通过 SPARQL 查到**同步残留表、迁移记账表、备份表**——这不是「多几个类」，是**把不该暴露的东西接到嘴上**。

要求：带 `--exclude` 重建快照再生成；被排除表作为 join 目标的 2 条边**跳过并点名**；**fail-visible**：未排除快照必须 stderr 警告 + 文件头标注「本映射包含 N 张已排除的表」。

### A-3 · `false` / `未标` 的拆分（认可）

| 情形 | 判定 |
| --- | --- |
| `false` **且有** `joinEvidence` | **false**（有据：名字推断 / 未签 / 样本不足） |
| `false` **且无** evidence（REQ-120 前的 BOOLEAN 遗留） | **未标** |

旧快照 `false 0 / 未标 52`、新快照 `false 6 / 未标 0` —— 对照要进报告。

---

## 四 · QA 裁定（PASS 带 4 条非阻断 → 2 条改判必修）

### H-1 · **AC-008 的三条 guard 删掉都不红**（必修）

QA 变异实测：

> 删掉「`rr:child` 存在性」「`rr:parent` 存在性」「证据表 == 关系终点」三条对账逻辑 ⇒ **34 条 REQ-121 测试全绿**。

**这与前几轮的「闸没接线」「探针数文本」是同一类**：代码写对了，但没人证明它在起作用。

要求：补 3 条夹具单测 + 变异验证（删 guard ⇒ 对应测试红）。

### H-2 · `.scratch/` 未进 `.gitignore`（必修）

QA 指出 `.scratch/` 里有 **327KB 真名 TTL / proposal**，与 REQ-084 的 N1 同类泄露风险。
要求：加入 `.gitignore` 并检查是否有其它未忽略的临时产物目录。

### H-3 · 登记不改

| # | 项 |
| --- | --- |
| N3 | `r2rml.rs:552` 的 `if evidence.is_empty()` 分支不可达（空证据先被父列解析拦下） |
| N4 | R2RML 出口 `e2e_covered: false`（CLI 级只有手工证据）——本需求的真实验收由 BA 手工跑过 |

### H-4 · QA 这轮的三个硬验证（记）

1. **变异 M1b（放行 false + 主键顶替）⇒ ac006 红**：说明测试能区分「真的没生成」与「生成了但被跳过」。
2. **对账变异（每条 join 的 child 加 `_x`）⇒ exit 1、stdout 空、不落文件、70 处逐条点名**：失败语义正确（宁可不产出，不产出错的）。
3. **中文 IRI 检查**：非 ASCII IRI **0 处**（中文只在 label/注释，无 BOM、纯 LF、无乱码）——这条我只在需求里提了风险，QA 真去数了。
