# 状态：REQ-124 · 控制面 API

## 当前状态

**已完成**（2026-10-23 · 已合并 `develop` `eaa7c96`）

## 结果

| 项 | 值 |
| --- | --- |
| 合并 | `develop` **`eaa7c96`**；禁改面（`serve`/`query` 路由、四红线、`tools.rs`、`property_filter.rs`、`Cargo.*`）零 diff |
| 端点 | 11 条：projects / import / apply / freeze / export.ttl / r2rml / config / audit / review / decide / health |
| token 隔离 | query token 调管理端点 **403 `TOKEN_KIND_FORBIDDEN`**；admin 调 query serve **401**；同 token 双 kind ⇒ **加载即拒** |
| 与 CLI 逐字节 | 四组 sha256 全等（TTL 客户/建模、R2RML 默认/include-inherit） |
| 声明校验 | 9 类非法全 400 + 点名，**前后 sha256 相同**（不落盘） |
| 冲突 | 重复 apply / 重复 freeze / 冻结后 import ⇒ 409（三码可读） |
| 后门 | 11 条路由全枚举，**无** object/link/ontology 直改入口；`src/console` 写 SQL 命中 0 |
| 审计 | 写成功/失败各一行；`/api/audit` 可数出 401×7 / 403×4；明文 0 |
| 测试 | lib **583** / doc 4 / console 14 / live 2；OUTLETS 56（I18–I21） |

## 关键裁定

| # | 裁定 |
| --- | --- |
| K-1 | TTL 导出**两处 render 调用点**接受（根因：`publish_customer` 命中闸会 `exit(2)`，服务进程不能退出）；判据改为**产物 sha256 相等** + 消费同一份 `TtlPrepared`；登记为已知偏离 |
| K-3 | import 内联时 `proposal.project` 必填；`export.ttl` 的 `schema`/`tenancy` 是服务端路径；`decide` body 多 `project`/`proposal`、`via=console` —— 全接受；但 GET `/api/review` 必须与 `review serve` 逐字节一致（QA 三时点验过：1098 B 相同） |
| K-4 | 审计只覆盖写 + export/r2rml + 鉴权失败（不记读） |

## 非阻断（QA 提示，登记）

1. `/api/audit` 是**进程内存窗口**（最近 1000 条），重启即丢 ⇒ 写进文档
2. 示例 token 原样可加载 ⇒ 建议启动警告「你在用示例 token」
