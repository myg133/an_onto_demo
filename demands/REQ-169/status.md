# 状态：REQ-169 · `@pending.get` 配方补 `auth.service`

**已就绪**（2026-10-27 · 实测触发）

## 一句话

服务端已经在 `@pending.get` 里发完整配方（v2 契约），但 `RecipeAuth` 少一个 `service` 字段
⇒ 数字员工拿着配方也签不了名（fail-closed）⇒ 配方不可用。

## 证据

- 实测 v2 AM（8 个 service 已补）：`@pending.get.auth` 有 `credential_env`/`signed_headers`/`timestamp`/`scope`，**无 service**
- 手工把 `service` 加进配方 ⇒ `access_fetch.py --recipe --check` **通过**
⇒ 只差服务端带出这一环

## 改动面

`src/query/source_gate.rs`：`RecipeAuth` 加 `service` + `build_recipe()` 从 AM 取值。
⛔ v1 行为不变（AC-103）。
