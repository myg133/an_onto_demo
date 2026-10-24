# 状态：REQ-135 · 生产可观测性

## 当前状态

**进行中**（2026-10-24 开单）

## 两条缺口（都实测过）

| # | 缺口 | 证据 |
| --- | --- | --- |
| ① | 没有 `/metrics` | `grep -i metrics src/` ⇒ 0 命中 |
| ② | 审计写 `emptyDir` | `console/portal --audit-out /data/audit/*.log` ⇒ Pod 重启即丢 |

**而且我曾在 prod overlay 里写过一个假的 `prometheus.io/scrape` 注解**（已删）——
假注解比没有监控更糟：面板显示"0 指标"，让人以为在监控。

## 六个指标

| 指标 | 为什么 |
| --- | --- |
| `ontok_requests_total` | 谁在调、失败多少 |
| `ontok_request_duration_seconds` | 慢在哪 |
| `ontok_rows_returned_total` | 有没有人拉全表 |
| **`ontok_tenant_denied_total`** | **有人试图越权的唯一可见信号** |
| `ontok_ttl_bytes` | 产物与版本一致性 |
| `ontok_mapping_joins` | 映射规模 |

## 审计为什么改 stdout

**k8s 的标准做法是"进程写 stdout，集群负责采集"**。
写 `emptyDir` 等于把合规证据交给 Pod 的生命周期——重启就没了。

给了 `--audit-out` 时**同时**写文件和 stdout，⛔ 不许只写文件。

## 待办

- [ ] Dev（三服务 `/metrics` + 审计 stdout + 清单）
- [ ] QA（重点：零业务数据泄漏、`tenant_denied` 真能加、审计两条路径）
