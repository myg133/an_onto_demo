# REQ-175 · **建项目发的 key 必须能鉴权**（运行时按摘要查 `project_keys`）

- 优先级：**P1**（生产数字员工接入的前置）
- 状态：已就绪
- 触发：生产 2026-10-28 实测 —— `POST /api/projects` 返回的 `ontok_q_681d60c…` 调 `/api/catalog` 返回 **401**

## 一 · 现状（代码 + 生产实测）

| 事实 | 证据 |
| --- | --- |
| 建项目会发 key（摘要 + 密文入库） | `project_keys.token_sha256` / `token_ciphertext`，`provision::create_key_in` |
| **运行时从不读这张表** | 全仓 `token_sha256` 只出现在 `console/provision.rs`（写/校验/列出）与 `db/bundle_store.rs`（SQL） |
| 多项目鉴权只认静态表 | `src/query/multi.rs:95 tokens: TokenTable`；`TokenTable::parse_multi` 来自 `--tokens` 文件 |
| 生产实测 | `GET /api/catalog` + 该 key ⇒ **401 UNAUTHORIZED**；`GET /api/projects` ⇒ 401（能鉴权的话应是 403 `key.not_admin`） |

⇒ 「建项目 → 发 key → 数字员工用」这条链**是断的**，目前只能手工往 Secret 里抄一条静态 token。

## 二 · 要做的

**静态表没命中时，按摘要查库**：

1. token 不在 `TokenTable` 里 ⇒ `sha256(token)`（与 `provision` 写库同一算法）查
   `project_keys WHERE token_sha256 = $1 AND revoked_at IS NULL`
2. 命中 ⇒ 得到 `project_id` → 项目名；**kind 恒为 `query`**（DB 只有 query key）
3. 未命中 / 已吊销 ⇒ 仍然 **401**（⛔ 不要改成 403，会泄露"这把 key 存在"）
4. **单项目模式**：DB key 绑的项目 ≠ `--project` 指定 ⇒ **403 `key.not_admin` 之外的新码**（建议 `key.project_mismatch`，须在错误码表登记）
5. **多项目模式**：按 key 绑的项目解析 `active_version_id`（与本项目现有路径一致）
6. 性能：加一层**短 TTL 缓存**（建议 60s，键 = sha256），⛔ 不要每请求打库
7. 审计：保持现状（只记摘要，⛔ 不记原文）

## 三 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | 真进程 + 真库：`POST /api/projects` 拿到的 key ⇒ `GET /api/catalog` **200**（多项目模式） |
| AC-002 | 同一把 key ⇒ `GET /api/projects` **403 `key.not_admin`**（读 key 不能写） |
| AC-003 | 伪造 key（同前缀、不同尾）⇒ **401**（⛔ 不能因为"前缀像"就放行） |
| AC-004 | 吊销后（`revoked_at` 非空）⇒ **401**；且**缓存要在 TTL 内失效或强制失效**（判据里直接验：吊销后立刻或 ≤ TTL 内变 401） |
| AC-005 | 单项目模式：DB key 绑**另一个**项目 ⇒ 拒绝（新码 `key.project_mismatch`，403） |
| AC-006 | 静态 token 仍优先：静态表里存在的 token 行为**逐字节不变**（`test_req144_access`/`req159b_multi` 全绿） |
| AC-007 | 审计里不出现 token 原文（只摘要） |

**红绿自证**：把"查库"那一步注掉 ⇒ AC-001 / AC-004红；恢复 ⇒ 绿。

## 四 · 不在本次范围

- key 轮换 / 多 key 并存（`project_keys` 有部分唯一索引，按现状一次一把）
- `GET /api/projects/:p/key` 的解密展示（已有）
