# 需求：REQ-146 · 多 SQL 方言支持（Postgres → SQLite / MySQL）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-146 |
| 优先级 | **P2（已进计划，暂不排期）** |
| 状态 | **已就绪（未开工）** |
| 基线 | 待定 |
| 触发 | 2026-10-25 用户：演示库先用 PG，但 **SQLite 等方言要进计划** |

## 问题

产品现在**只支持 PostgreSQL**，且**没有方言抽象**：

| 证据 | 位置 |
| --- | --- |
| `sqlx = { features = ["postgres"] }` | `Cargo.toml` |
| introspect 只读 `pg_catalog` / `information_schema` | `src/extract/introspect.rs:14` |
| **全仓 0 处 `Dialect` 抽象** | `grep -ri dialect src/` = 0 |
| 生成的 SQL 含 PG 专有语法（`::text`） | `sql_echo` 实测 |

⇒ 换任何非 PG 的源，**不是改配置，是要动 SQL 生成器**。

## 为什么现在不做、但要规划

| 现在 | 将来 |
| --- | --- |
| 演示/客户都是 PG | 客户可能给 SQLite 文件、MySQL、达梦、人大金仓 |
| 改造成本 3–5 天 | 客户等不了；现场手工导数据反而更贵 |

**先规划的收益**：以后每一处新写的 SQL，都**顺带想一下方言**——比事后改造便宜得多。

## 要抽象的六个点（这就是边界）

**⛔ 不要在每个 SQL 拼接点写 `if sqlite {…}`。** 抽象成一个 `Dialect` 的六件事：

| # | 职责 | PG | SQLite | MySQL |
| --- | --- | --- | --- | --- |
| 1 | **目录勘查** | `pg_catalog` + `information_schema` | `sqlite_master` + `PRAGMA table_info` | `information_schema` |
| 2 | **标识符引用** | `"col"` | `"col"` | `` `col` `` |
| 3 | **类型 → xsd** | `character varying`→`xsd:string`… | `TEXT/INTEGER/REAL/BLOB`（**affinity，弱类型**） | `varchar/int/datetime` |
| 4 | **字面量转字符串** | `x::text` | `CAST(x AS TEXT)` | `CAST(x AS CHAR)` |
| 5 | **只读约束** | `SET TRANSACTION READ ONLY` | `PRAGMA query_only = ON` | `SET SESSION TRANSACTION READ ONLY` |
| 6 | **分页** | `LIMIT n OFFSET m` | 同 | 同（旧版用 `LIMIT m,n`） |

### 还有两处容易漏

| # | 点 | 说明 |
| --- | --- | --- |
| 7 | **注释/主键/唯一约束的来源** | SQLite 没有列注释、`PRAGMA` 拿约束方式不同 ⇒ `display_cn` / `primary_key` 可能缺失，**必须允许为空并如实报告** |
| 8 | **R2RML 的类型声明** | R2RML 本身方言中立（只说 tableName/column），但 `rr:datatype` 要按 **xsd** 而不是源库类型 |

## 关键裁定（先定下来，避免以后跑偏）

| # | 裁定 |
| --- | --- |
| R1 | **方言只影响「读」这一侧**；本体、提案、评审、冻结、TTL 全都不变 |
| R2 | 抽象**只有一个入口**：一个 `Dialect` trait，⛔ 不许散落 `if` |
| R3 | 每种方言必须**如实报告能力缺失**（如 SQLite 无注释 ⇒ `display_cn: null`，⛔ 不猜） |
| R4 | **判据是对拍**：同一份本体 + 同一套查询，两种方言结果**逐字节相同**（或差异被显式解释） |
| R5 | 新方言**不降低**现有的安全约束（只读、行数上限、租户注入） |

## 建议顺序

| 序 | 方言 | 理由 |
| --- | --- | --- |
| 1 | **SQLite** | 单文件、零运维；演示/离线最省 |
| 2 | **MySQL** | 客户库最常见 |
| 3 | 国产库（达梦/金仓） | 有客户才做 |

## 不做

- ⛔ 不做跨方言的**联邦查询**（一查询多库）
- ⛔ 不做方言间的**数据搬迁**
- ⛔ 不改 R2RML 产物的形状（方言差异体现在执行器，不体现在映射）

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | SQL 拼接点很多，改漏 | 先做"把 SQL 生成集中到一处"的重构（无行为变化），再加方言 |
| 2 | SQLite 弱类型 ⇒ 证据等级失真（值域重叠仍可算，但类型判断不可靠） | R3：如实报告，类型来源标 `heuristic` |
| 3 | 测试矩阵翻倍 | R4 用"对拍"而不是"每个方言写一套期望值" |
| 4 | 方言特性差异被当成 bug | 每条差异写进"已知限制" |

## 交付物（开工时）

1. `Dialect` trait + PG 实现（**行为零变化**，逐字节对拍）
2. SQLite 实现 + 端到端（schema → 提案 → 冻结 → TTL → 查询）
3. 能力缺失报告（注释/约束/类型）
4. 已知限制
