# 验收标准：REQ-135

## 一 · AC

### AC-001 · `/metrics` 可用且零 token

- `GET /metrics`（三个服务）⇒ **200**，`Content-Type: text/plain; version=0.0.4`
- ⛔ 不需要 `Authorization`
- 判据：三个服务各一次；无 token 也能取

### AC-002 · Prometheus 文本格式合法

- 输出符合 exposition format：`# HELP` / `# TYPE` / `name{label="v"} value`（可选 `# EOF`）
- 判据：**逐条**断言（至少：每个指标有 TYPE；counter 单调；histogram 有 `_bucket`/`_sum`/`_count`）
- 若有 `promtool` 可用则加一条 `promtool check metrics`；没有就用手写校验器（**不许因为没工具就跳过**）

### AC-003 · 零业务数据泄漏

- `/metrics` 输出里 grep：租户 id（真实值）、表名（如 `deductions`）、SQL 片段（`SELECT`）、查询原文 ⇒ **全 0 命中**
- 判据：用真实查询跑几次后取 `/metrics`，逐项 grep

### AC-004 · 六个指标都在，且随请求变化

| 指标 | 判据 |
| --- | --- |
| `ontok_requests_total` | 发 3 次请求 ⇒ 计数 +3 |
| `ontok_request_duration_seconds` | 有 `_count` 增长 |
| `ontok_rows_returned_total` | 返回 5 行 ⇒ +5 |
| **`ontok_tenant_denied_total`** | 造一次 `TENANT_MISMATCH` ⇒ +1（**安全信号**） |
| `ontok_ttl_bytes` | 请求 `/api/ontology.ttl` ⇒ 等于响应字节数 |
| `ontok_mapping_joins` | 与 R2RML 的 joinCondition 数一致 |

- 判据：六个各一次；`tenant_denied` 那条必须真造出拒绝

### AC-005 · 审计默认走 stdout

- 起服务**不给** `--audit-out` ⇒ 审计行出现在 **stdout**
- 给了 `--audit-out` ⇒ **同时**写文件**和** stdout（⛔ 不许只写文件）
- 判据：两种各一次；stdout 里能 grep 到 `audit ts=`

### AC-006 · 审计行可 parse

- 每行是 key=value 且字段齐全（`ts` / `token` 摘要 / `action` 或 `event` / `result` 或 `code`）
- ⛔ 无 token 明文
- 判据：取 20 行逐行解析

### AC-007 · k8s 清单

- `/metrics` 就绪后才加 `prometheus.io/scrape`：注解存在且端口正确
- 审计挂载：**说明选了哪个**（stdout-only 还是 PVC），清单与说明一致
- 判据：`kubectl kustomize overlays/prod` 渲染通过 + 注解存在

### AC-008 · 不退化 / 零依赖

| 项 | 约束 |
| --- | --- |
| ⛔ 禁改 | 四红线 + `tools.rs` + `guard()` + `property_filter.rs` |
| ⛔ 禁 | 新增依赖（Cargo.toml 零改动） |
| 出口 | `/metrics` 端点登记进 `OUTLETS`（内部轨） |
| 基线 | 同机 `develop` 实测 |

## 二 · 纪律

- **H** 指标登记只许一处（一张静态表，标签集合固定）
- **G** 变异：把 `tenant_denied` 恒 0 / 让 `/metrics` 输出租户 id / 审计只写文件 ⇒ 每条必须被抓
- **S** AC 也会错，发现直接说
