# 需求：REQ-166 · AM 契约补齐（`auth.service` 必填 + 数组参数形状）

## 基本信息

| 项 | 值 |
| --- | --- |
| 编号 | REQ-166 |
| 优先级 | **P1**（不补 ⇒ 取数脚本对 AM fail-closed，直接跑不了） |
| 状态 | **已就绪** |
| 归属 | **lion**（AM 生成 = 构建面：`ontok-builder access`） |
| 触发 | join 在 REQ-164 / REQ-165 把 `access_fetch.py` 做成"产品态、不猜"之后暴露的三处 AM 缺口 |
| 关联 | REQ-164（取数脚本）、REQ-165（脚本去厂商耦合）、`Deploy/skills/ontok-query/AM-FIELDS.md` |

---

## 三处缺口（都是实测，不是推测）

| # | 缺口 | 现状 | 要求 |
| --- | --- | --- | --- |
| 1 | **`tc3-hmac-sha256` 模式的 `auth.service` 必填** | 脚本原本取 host 首段（`cvm.tencentcloudapi.com` → `cvm`）——**那是在猜** | 脚本改为**缺它即 `am.invalid`**（REQ-165）；⇒ **AM 必须显式写出** `auth.service` |
| 2 | `sources[].auth` 缺 `signed_headers` / `timestamp` / `scope` | 只有 `mode` + `credential_env` | 生成器把这三个也写出来（缺省值可以给，但**必须是显式的**，而不是"运行时猜") |
| 3 | `CdbInstance.instance_ids` 没声明 `shape: array` | `--param instance_ids=a,b` 会**整串当一个 ID** | 生成器对数组型参数写 `"shape": "array"` |

## 验收

- `ontok-builder access`（或对应生成器）产出的 AM：每个 `tc3` source 都有 `auth.service`；数组参数有 `shape: array`
- `am-tencent.json` 重新生成（或手工补齐 8 个 source）
- `python Deploy/skills/ontok-query/access_fetch.py --check --am <am.json>` 通过（不再报 `am.invalid`）
- 回归：`cargo test --lib access` + 既有的 `access` 相关集成测试全绿；AM schema 若有校验用例，补 3 条（缺 service ⇒ 拒、缺 shape ⇒ 按标量、合法 ⇒ 过）

## 不做

| ⛔ | 理由 |
| --- | --- |
| 改 `access_fetch.py`（脚本） | 那是 REQ-164/165 的面（join） |
| 在 AM 里写"厂商默认值" | 产品态不猜；缺就必须报错，让生成器补齐 |
