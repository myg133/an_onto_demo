# access mapping 契约（草案 v1）

> 状态：**草案，待评审**。2026-10-25。
> 决策依据：用户拍板「**可执行描述 + 参考执行器**」；fixture 用「客户提供一次 + 官方示例补形状」。

---

## 一 · 它是什么（一句话）

> **本体说"世界由什么构成"，access mapping 说"这些东西去哪取、怎么取"。**
> **执行者是客户的数字员工**，不是我们。

```
本体（TTL）           ← 类 / 属性 / 关系 / 口径
access mapping        ← 每个类：哪个 API、哪些参数、响应哪一段、能不能过滤
参考执行器（CLI/SDK）  ← 按 mapping 取数，返回 JSON-LD
```

**为什么不让我们的服务执行**：AK/SK 不出客户边界、分页限流归他、我们保持无状态只读。

---

## 二 · 产物形状

```json
{
  "contract_version": 1,
  "project": "ops360",
  "ontology_version": 2,
  "sources": [
    {
      "id": "tencent-cvm",
      "kind": "http-json",
      "base_url": "https://cvm.tencentcloudapi.com",
      "auth": {
        "mode": "tc3-hmac-sha256",
        "credential_env": ["TENCENT_SECRET_ID", "TENCENT_SECRET_KEY"]
      },
      "defaults": { "Region": "ap-guangzhou" }
    }
  ],
  "entities": [
    {
      "class": "cvm_instance",
      "source": "tencent-cvm",
      "reviewed_by": null,

      "operation": {
        "action": "DescribeInstances",
        "version": "2017-03-12",
        "method": "POST",
        "content_type": "application/json"
      },

      "identity": {
        "select": "$.Response.InstanceSet[*]",
        "key_path": "InstanceId",
        "iri_template": "ontok:cvm_instance/{key}"
      },

      "fields": {
        "name":   { "path": "InstanceName" },
        "status": { "path": "InstanceState" },
        "zone":   { "path": "Placement.Zone" },
        "cpu":    { "path": "CPU", "type": "xsd:integer" }
      },

      "request": {
        "required": ["Region"],
        "from_filter": { "zone": "Zone", "ids": "InstanceIds" },
        "paging": {
          "offset_param": "Offset", "limit_param": "Limit",
          "max_limit": 100, "total_path": "$.Response.TotalCount"
        }
      },

      "capability": {
        "pushdown":     ["Region", "Zone", "ids"],
        "local_only":   ["status", "cpu"],
        "max_scan_rows": 5000,
        "cost_hint":    "free"
      },

      "freshness": { "mode": "realtime", "note": "实例状态可能有数秒延迟" },

      "fixture": "fixtures/tencent-cvm-describe.json"
    }
  ],

  "relations": [
    { "predicate": "alarm__resource", "from": "alarm", "to": "cvm_instance",
      "via": { "kind": "join", "local": "resource_id", "remote": "id" } },
    { "predicate": "cvm__app", "from": "cvm_instance", "to": "app",
      "via": { "kind": "field", "field": "app_code" } }
  ]
}
```

---

## 三 · 十条硬规则

| # | 规则 | 为什么 |
| --- | --- | --- |
| 1 | **⛔ 不含任何真凭据**；`auth` 只给**环境变量名** | 契约会随本体分发 |
| 2 | `iri_template` 必须与 TTL 里的 `ontok:instanceIri` **一致** | 否则 agent 拼的 IRI 与本体对不上，图就断了 |
| 3 | **`pushdown` 与 `local_only` 必须显式**，没有"默认都可以" | 不写就等于暗示"什么都能过滤" |
| 4 | 超出 `pushdown` 的过滤条件 ⇒ **本地过滤**，且回执里必须标 `filtered_locally: true` | 否则 agent 以为服务端筛过了 |
| 5 | 扫描行数超 `max_scan_rows` ⇒ **拒绝并说明**，⛔ 不许偷偷全量拉 | 这是"能跑但慢到爆"的防线 |
| 6 | `freshness` 必须给，且**回执要带上** | 运维数字最怕"数字对但时点不对" |
| 7 | `relations.via.join` 只允许**同一 source 内**两实体 | ⛔ 不做跨源 join（v1） |
| 8 | 每个 `entity` 有 `reviewed_by`，未签的**不许用于生产** | AI 生成的映射也是提案（ADR-005） |
| 9 | 有 `fixture` 的实体，执行器 `--dry-run` 必须能跑（不调真 API） | 否则测试永远需要真凭据 |
| 10 | 契约版本变化要**只增不减**；破坏性变更升 `contract_version` | 与 REQ-141 同一纪律 |

---

## 四 · 参考执行器（CLI）

```bash
# 真实调用（凭据从环境变量读）
ontok-exec fetch --mapping access.json --class cvm_instance \
                 --filter '{"zone":"ap-guangzhou-3"}' --limit 50 \
                 --out out/cvm.jsonld

# 离线（用 fixture，⛔ 不调 API）—— CI 与演示都靠它
ontok-exec fetch --mapping access.json --class cvm_instance \
                 --filter '{"zone":"ap-guangzhou-3"}' --dry-run-fixture

# 体检：契约自洽性（字段路径存在、join 两端都在、iri_template 与本体一致）
ontok-exec validate --mapping access.json --ontology ontology.ttl
```

**输出（与我们的查询接口同形状，方便 Agent 复用同一套解析）**：

```json
{
  "@context": { "ontok": "https://…/ops360#" },
  "@graph": [
    { "@id": "ontok:cvm_instance/i-abc123", "@type": "ontok:cvm_instance",
      "ontok:cvm_instance__name": "web-01",
      "ontok:cvm_instance__zone": "ap-guangzhou-3" }
  ],
  "meta": {
    "rows_scanned": 120, "rows_returned": 50, "pages": 2,
    "filtered_locally": ["status"],
    "freshness": { "mode": "realtime", "fetched_at": "…" },
    "capability_warnings": ["status 不能作为请求参数，已在本地过滤"],
    "truncated": false
  }
}
```

**关键**：`meta` 里那几行**和 `query serve` 的 warnings/coverage 是同一套纪律**——
Agent 只要学会读一种 `meta`，两种取数方式都能读。

---

## 五 · 错误分类（Agent 要能区分）

| code | 含义 | Agent 该做什么 |
| --- | --- | --- |
| `AUTH_MISSING` | 环境变量缺 | 提示运维配凭据，**别重试** |
| `AUTH_REJECTED` | 凭据被拒 | 同上 |
| `RATE_LIMITED` | 触发限流 | **可以退避重试**（与 `NO_DATA_SOURCE` 不同） |
| `SCAN_LIMIT_EXCEEDED` | 超 `max_scan_rows` | **改查询**（加更窄的过滤），别重试 |
| `FIELD_PATH_MISSING` | 映射路径在响应里找不到 | **契约错了**，报给我们修 |
| `NOT_IN_PUSHDOWN` | 想下推但 API 不支持 | 执行器自动退回本地过滤（这条一般不报错，只进 warnings） |

> `RATE_LIMITED` 可以重试、`SCAN_LIMIT_EXCEEDED` 不能——**这两个必须分开**，
> 否则 Agent 会对着"扫太多"的错一直重试。

---

## 六 · fixture 层（决定可测试性）

```
fixtures/
├── tencent-cvm-describe.json      ← 客户提供一次真实响应（脱敏后）
├── tencent-cdb-describe.json
└── README.md                      ← 每份：什么时候取的、哪个 region、脱敏做了什么
```

**三条纪律**：

1. fixture 是**脱敏后的真实响应**，不是编的——编的会漏字段，测试就假绿
2. `--dry-run-fixture` 用它跑，CI 不需要凭据
3. 契约 `validate` 要能发现"字段在 fixture 里不存在"（这就是最便宜的正确性检查）

---

## 七 · 与 R2RML 的关系（别混）

| | R2RML | access mapping |
| --- | --- | --- |
| 源 | 关系库 | HTTP API |
| 映射 | 类←表、属性←列 | 类←API 响应节点、属性←字段路径 |
| 执行者 | 我们的 `query serve` | **客户侧执行器** |
| 能否保证一致性 | 能（同一事务快照） | ❌ 不能（每次调用不同时点） |

**所以两者是并列的两条路，不是同一种东西的两种写法。**

---

## 八 · 分阶段（只谈顺序）

| 阶段 | 内容 | 价值 |
| --- | --- | --- |
| **0** | 契约形状 + `validate` + `--dry-run-fixture` | 让"映射对不对"能离线测 |
| **1** | 一个真 API（腾讯云 CVM）跑通 | 证明路走得通 |
| **2** | 多个资源 + 单源内 join | 支撑 demo 的"影响面" |
| **3** | 契约与本体同版本发布（随 TTL 一起） | 消费侧一次拿全 |
| **4** | 映射的 AI 提案 + 人签 | 大规模才需要 |

**阶段 0 和 1 就能支撑 Ops360 的"现场"演示**——不需要等全套。

---

## 九 · 要你确认的三点

| # | 问题 | 我的建议 |
| --- | --- | --- |
| 1 | 参考执行器是**独立二进制**（`ontok-exec`）还是 `ontok` 的子命令（`ontok access fetch`）？ | **子命令**——少一个分发物，且与现有 CLI 同一套凭据/日志 |
| 2 | `relations.via.join` 的**本地 join** 允许吗（两个 API 各拉一批再拼）？ | 允许，但必须声明两边各拉多少；超 `max_scan_rows` 就拒绝 |
| 3 | 契约里的 `capability` 由谁填？ | **AI 提案 + 人签**（和别处一致）；`validate` 只能查自洽，查不了"腾讯云是不是真支持这个参数" |
