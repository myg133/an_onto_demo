# 状态：REQ-175 · DB 发的 key 要能鉴权

**已就绪**（2026-10-28 · 生产实测 401 触发）

## 一句话
`POST /api/projects` 发的 `ontok_q_…` 只写进 `project_keys`，运行时只认静态 token 表 ⇒ 401。
改成：静态表没命中就按摘要查库，命中即按该 key 绑的项目解析（kind 恒为 query）。

## 影响面
`src/query/multi.rs`（鉴权入口）· 可能新增 `src/query/db_keys.rs`（摘要查库 + 60s 缓存）·
`src/db/bundle_store.rs`（按摘要取项目）· 错误码表（`key.project_mismatch`）· 测试
