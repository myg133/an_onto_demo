# 验收标准：REQ-151

## 一 · AC

### AC-001 · 本体模式 + api 类（jsonld）⇒ 200 + @pending

- 部署参数：`--ontology-only --access-mapping <已签>`
- 查询只涉及 api 类 ⇒ **200**，顶层含 `@graph`（空）/`@pending`（≥1）/`meta.complete:false`
- `@pending[0]` 必须含 `get{ class, filter/pushdown, ref }`，**形状与普通模式一致**（R5）
- 判据：贴响应原文

### AC-002 · 本体模式 + api 类（rows）⇒ 400 `NOT_DB_SOURCE`

- 同一条查询、`format=rows` ⇒ **400**，`code=NOT_DB_SOURCE`
- ⛔ 不是 503、⛔ 不是 `rows: []`
- 判据：状态码 + 原文

### AC-003 · 本体模式 + 库类 ⇒ 仍 503（R2）

- 查询涉及 mapping 里的**库类**（或完全不在 mapping 里的类，按产品现有语义）⇒ **503 `NO_DATA_SOURCE`**
- ⛔ 文案与码与 `9dfb8e7` 逐字节相同
- 判据：对拍旧版本

### AC-004 · 混合查询 ⇒ 503

- 一条查询同时涉及库类与 api 类 ⇒ **503 `NO_DATA_SOURCE`**
- 判据：一次

### AC-005 · `sample` 仍 503（R3）

- 本体模式下 `sample` 打 api 类 ⇒ **503**（取样必须真连库）
- 判据：一次

### AC-006 · **绝不碰源库**（结构事实，最重要）

- 本体模式下（`svc.source` 为 `None`）走完 AC-001/002 后，**没有任何连接尝试**
- 验证方式（二选一，或都做）：
  1. 把 `ONTOK_SOURCE_DATABASE_URL` 指向**不存在的主机**，行为不变（若尝试连接会超时/报错）
  2. 结构性：`source: Option` 的 `None` 分支在持有连接之前返回；贴代码路径证明
- 判据：贴测量/代码证明

### AC-007 · 审计 `code` 一致（R4）

- AC-001 ⇒ 审计 `code=OK`
- AC-002 ⇒ 审计 `code=NOT_DB_SOURCE`
- AC-003 ⇒ 审计 `code=NO_DATA_SOURCE`
- 判据：三行审计原文

### AC-008 · 普通模式零回归（R1）

- 普通模式（有源库 + schema + policy）下：db 类查询、api 类 pending、rows 模式 400、非法 format 400……
  全部与 `9dfb8e7` **逐字节相同**
- 判据：8 case sha 对拍（自建基线二进制）

### AC-009 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新依赖；删测试；放宽断言 |
| 特别注意 | **REQ-136 的 AC 文案要复核**：它是否只约束 db 类；若是"所有 SPARQL 恒 503"，需在交付里指出并给出本次的精确化（不许默默改它的 AC） |

## 二 · 纪律

- **G** 变异：①把 api 分支改回 503 ⇒ AC-001 必红 ②让 db 类也走 pending ⇒ AC-003 必红 ③让 `sample` 走 pending 分支 ⇒ AC-005 必红
- **S** AC 也会错，发现直接说
