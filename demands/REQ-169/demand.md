# REQ-169 · `@pending.get` 的配方要带 `auth.service`（否则数字员工拿不到能用的配方）

- 状态：**已就绪**（P1）
- 关联：REQ-166（AM 补 `auth.service`）、REQ-168（脚本认 v2）、REQ-164（取数脚本）
- 触发：实测（join，2026-10-27）

## 一 · 事实（实测，不是推测）

用 **v2 AM（含 8 个 service）** 起 `query serve`，查一个 API 源类，回执里 `@pending.get` 已经带完整配方：

```json
"get": {
  "class": "alarm_policy", "ref": "access#AlarmPolicy@v1",
  "source": {"base_url": "https://monitor.tencentcloudapi.com", "auth_mode": "tc3-hmac-sha256"},
  "operation": {...}, "request": {...}, "iterator": "$.Response.Policies[*]",
  "fields": {...15 条...},
  "auth": {"credential_env": ["TENCENT_SECRET_ID","TENCENT_SECRET_KEY"],
           "signed_headers": [...], "timestamp": {...}, "scope": [...]}
}
```

⚠️ **`auth` 里没有 `service`** —— 尽管 AM 的 `sources[].auth.service` 已经补上（REQ-166）。

## 二 · 后果

`RecipeAuth` 没有 `service` 字段 ⇒ 服务端发不出它 ⇒ 数字员工拿着配方去取数时，
`tc3-hmac-sha256` 签名器**必须**显式 `service`（fail-closed，⛔ 不从 host 猜）⇒ **配方不可用**。

实测：把 `service` 手工加进配方 JSON ⇒ `access_fetch.py --recipe ... --check` **通过（1 sources / 1 entities）**。
⇒ 只差服务端把它带出来这一环。

## 三 · 要做的

| # | 改动 |
| --- | --- |
| 1 | `src/query/source_gate.rs` 的 `RecipeAuth` 增加 `service: Option<String>`（`skip_serializing_if = "Option::is_none"`） |
| 2 | `build_recipe()` 从 AM 的 `sources[].auth.service` 填入 |
| 3 | 契约 v1 的映射里 `Recipe` 全 `None` 的既有行为**不变**（AC-103：v1 ⇒ 一个键都不多） |

## 四 · 验收

| # | 判据 |
| --- | --- |
| AC-001 | v2 AM（含 `auth.service`）⇒ `@pending[].get.auth.service` **存在且等于 AM 里的值**（真进程 + 真库） |
| AC-002 | v1 AM ⇒ `@pending[].get` **仍然只有 4 键**（class/filter/pushdown/ref），⛔ 不因为这个改动多出任何键（AC-103 不回退） |
| AC-003 | v2 AM 但 `auth.service` 缺失 ⇒ 配方里**不出现** `service` 键（⛔ 不塞空串、不猜） |
| AC-004 | 端到端：把 `@pending.get` 原样喂给 `Deploy/skills/ontok-query/scripts/access_fetch.py --recipe - --format rows` ⇒ 能进入取数流程（不因缺 service 被拒） |
| AC-005 | 既有判据全绿（`test_req144_*` / `test_req149_generic` / `source_enumeration`） |

**红绿自证**：把 `build_recipe` 里填 `service` 的那行去掉 ⇒ AC-001 红、AC-004 红。

## 五 · 不做

- ⛔ 不把 `service` 从 `base_url` 的首段推导（那正是本单要消灭的"猜"）
- ⛔ 不改 AM 契约的字段名（`service` 已在 REQ-166 定名）
