# 状态：REQ-135 · 生产可观测性

## 当前状态

**已完成**（2026-10-24 · 代码合并 `develop` `c37b21b`；部署侧 `Deploy` `55cfd99`）

## 结果

| 项 | 值 |
| --- | --- |
| `/metrics` | 三服务 200 · `text/plain; version=0.0.4` · **免 token** |
| 审计 | 默认 **stdout**；给 `--audit-out` 时**文件 + stdout 各一份且逐行相同** |
| lib 测试 | **702 / 0**（基线 689，+13） |
| 出口 | 63 条（内部 33，I28） |
| 依赖 | **Cargo.toml 零改动**（手写 Prometheus 文本格式） |

## 六指标实测（QA 独立复核）

| 指标 | 证据 |
| --- | --- |
| `requests_total` | 3 次 catalog ⇒ `…code="200"} 3` |
| `request_duration_seconds` | `_bucket/_sum/_count` 完整，`+Inf == _count` |
| `rows_returned_total` | `LIMIT 5` ⇒ 计数 5；0 行查询不动 |
| **`tenant_denied_total`** | **四类全真造**：`TENANT_MISMATCH` 1 / `TENANT_UNRESOLVED` 2 / `TOKEN_KIND_FORBIDDEN` 1 / `TENANT_FROM_CLIENT` 1 |
| `ttl_bytes` | `customer=293814` == 响应字节自身量；`model=371356` |
| `mapping_joins` | `70` == CLI `joinCondition 70` == 产物 `rr:joinCondition` 70 次 |

## 零泄漏（QA 用 26 个 needle 验）

真实租户 uuid / `deductions` / `SELECT|WHERE|FROM ` / 项目名 / 三张真 token 前缀 ⇒ **全 0**。

标签值**类型层锁死**：`Key = (&'static str, Vec<(&'static str, &'static str)>)`（`src/metrics.rs:284`）——
**运行时字符串塞不进标签**。唯一子串命中是常量 `TENANT_FROM_CLIENT`（含空格的 `FROM ` 为 0）。

## 格式校验器**反空转**

7 种坏格式（缺 TYPE / 类型错 / 非数字 / 标签无引号 / 多标签 / 重复序列 / 缺 `_sum`）**逐条被点名拒绝**，
且完好文档 Ok、三份真实抓取全通过。

> 只测"好格式通过"不够——**能拒坏格式**才说明校验器有牙。

## 裁定

| # | 裁定 |
| --- | --- |
| **S-1** | `code` 标签 = **HTTP 状态码**；业务原因归 `tenant_denied_total{reason}` |
| **S-2** | `TENANT_UNRESOLVED` 的 live 现场依赖"策略漏登记"，可接受 |
| **N-1** | AC-007（k8s 清单）**移出代码合入门槛**，由 `Deploy` 分支独立交付 ✅ 已完成 |

## 部署侧（`Deploy` `55cfd99`）

| 项 | 内容 |
| --- | --- |
| prod overlay | 三服务加**真的** `prometheus.io/scrape|path|port` 注解 |
| 审计 | prod 也落 **PVC**（与 UAT 一致）；stdout 照写 |
| 镜像 | `aigccontainer.azurecr.cn/an_onto/ontok:0.1.0` |
| ⚠️ 上线注意 | `/metrics` **免 token** ⇒ 必须用 NetworkPolicy 只放行监控命名空间，且 ingress 不暴露该路径 |

## 6 条非阻断（登记）

| # | 项 | 处置 |
| --- | --- | --- |
| N-1 | k8s 清单不在代码分支 | ✅ 已在 Deploy 补 |
| N-2 | `console/http.rs:209` 用字面量而非常量 | 下轮 |
| N-3 | 交付方格式判据只断言 `!errs.is_empty()`（偏弱） | 下轮收紧 |
| N-4 | portal 的 `ttl_bytes`/`mapping_joins` 恒 0 | 合理（门户无此面） |
| N-5 | `/metrics` 自观测计入 `requests_total` | 接受（抓取会自我增长，面板里扣除或不看该端点） |
| N-6 | **27 张表未登记进 `query-policy.json`** | ⚠️ **上线前必须补**（否则客户一问就 500） |
