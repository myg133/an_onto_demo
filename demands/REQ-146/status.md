# 状态：REQ-146 · 多方言支持

## 当前状态

**已就绪（未排期）**（2026-10-25 进计划）

## 为什么进计划但不现在做

| 现在 | 将来 |
| --- | --- |
| 演示与客户都用 PG | 客户可能给 SQLite 文件 / MySQL / 国产库 |

**先规划的收益**：以后每写一处 SQL，都顺带想一下方言——比事后改造便宜得多。

## 现状证据（不是"配置问题"）

| 证据 | 位置 |
| --- | --- |
| `sqlx features = ["postgres"]` | `Cargo.toml` |
| introspect 读 `pg_catalog` | `src/extract/introspect.rs:14` |
| **全仓 0 处 `Dialect` 抽象** | grep |
| 生成的 SQL 含 `::text` | `sql_echo` 实测 |

## 抽象边界（六件事 + 两处易漏）

1. 目录勘查（PG `pg_catalog` / SQLite `sqlite_master`+`PRAGMA`）
2. 标识符引用（`"col"` vs `` `col` ``）
3. 类型 → xsd（SQLite 是 **affinity 弱类型**）
4. 字面量转字符串（`::text` vs `CAST(... AS TEXT)`）
5. 只读约束（`SET TRANSACTION READ ONLY` vs `PRAGMA query_only`）
6. 分页语法

**易漏**：
- **注释与约束来源**（SQLite 无列注释 ⇒ `display_cn` 必须允许空且如实报告）
- **R2RML 的类型按 xsd 声明**，不按源库类型

## 顺序

SQLite（省运维）→ MySQL（最常见）→ 国产库（有客户才做）

## 待办

- [ ] 排期后：先做「SQL 生成集中到一处」的重构（行为零变化），再加方言
- [ ] PG 对拍（逐字节）作为第一道判据
