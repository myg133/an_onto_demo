# 状态：REQ-122 · SPARQL 查询规划与租户注入

## 当前状态

**已完成**（2026-10-23 · 已合并 `develop` `e3da616`）

## 最终结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`e3da616`**；禁改面零 diff |
| 映射一致性 | 84 TriplesMap / 952 属性 / **70 join**，与 `r2rml generate` **逐数相同**（同一 `build_mapping`） |
| **泄漏对拍** | QA 自选两组真实租户：**交集全 0**；A 行集与直接 SQL **完全相等**；含「公共维度当驱动表」的攻击 |
| `sql_echo` | `pg_stat_activity` 抓服务端实际 SQL 与回执**逐字相同**；已补 3 条 lib 判据 + 库内端到端判据 |
| 声明驱动 | `git grep` 非注释行 **0 处**；改声明列名 ⇒ 注入/echo/tenant_applied 全跟着变，代码零改动 |
| coverage 警告 | `main_only` 命中即报警（说清漏什么）；改 `complete` ⇒ 消失；**不写 reason ⇒ 整份声明被拒** |
| 拒绝语法 | **26 条**用例全部 exit≠0 + 可读点名（⛔ 不静默忽略子句） |
| 只读 | **真读回** `transaction_read_only = on`（不再是写死字符串） |
| 库内泄漏测试 | 只需 `ONTOK_TEST_DATABASE_URL`，A/B 交集 0，收尾 `DROP SCHEMA` |

## 五轮迭代

| 轮 | 触发 | 改动 |
| --- | --- | --- |
| 1 | 主体 | SPARQL 子集 → SQL；注入；26 条拒绝 |
| 2 | Dev 抛回 `main_customer_id` 0/4 非空 | 声明改用 `customer_id`（数据驱动） |
| 3 | BA 裁定 I-1 | `coverage: main_only` + 回执警告（漏数据比查不到更危险） |
| 4 | QA 变异 | 三条「声称但没验」：echo 无断言 / 泄漏测试静默 SKIP / 只读回执写死 |
| 5 | 修复 | echo 纯函数化 + 断言；库内泄漏测试；真读回只读状态 |

## 一个我必须记的教训

**并发派单**：我把同一条 I-1 既发给原 Dev、又新派了一个，两个 agent 同时改同一 worktree。
原 Dev 主动报警「有另一个 agent 在改我的文件」，我才发现。
⇒ **派单前必须查 `list_agents` 的 running 状态**，同一 worktree 只允许一个写者。
