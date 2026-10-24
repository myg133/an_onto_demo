# 需求：REQ-135 · 生产可观测性（`/metrics` + 审计落地）

## 基本信息

| 项 | 值 |
| --- | --- |
| 需求编号 | REQ-135 |
| 优先级 | **P0（上线门槛）** |
| 状态 | 进行中 |
| 基线 | `develop` `5163cfb` |
| 来源 | `Deploy/PRODUCTION-READINESS.md` §一①② |

## 问题（两条，都是"上线后看不见"）

### ① 没有 `/metrics`

`grep -i metrics src/` ⇒ **0 命中**。

更要命的是：**prod overlay 里曾经写过 `prometheus.io/scrape: "true"`**——假的。
监控面板会显示"0 指标"，让人以为在监控。已删除（`Deploy` 分支），并在注释里写明原因。

### ② 审计写在 `emptyDir`，Pod 重启就没

审计是**合规证据**（谁查了什么、谁签了什么）。现在：

```
console/portal: --audit-out /data/audit/*.log → emptyDir（重启即丢）
query:          走 stderr                      → 跟容器日志一起滚走
```

## 要做

### 1 · `/metrics` 端点（三个服务都加）

```
GET /metrics
```

- **免 token**（与 `/api/health` 同口径：只给聚合数字，零业务数据）
- **Prometheus 文本格式**（`# HELP` / `# TYPE` / `name{label="v"} 123`）
- ⛔ **不引新依赖**（手写格式，不引 prometheus crate）
- ⛔ 不暴露任何业务数据（租户 id、表名、查询原文**一律不许**）

### 2 · 指标（最小可用集）

| 指标 | 类型 | 标签 | 为什么 |
| --- | --- | --- | --- |
| `ontok_requests_total` | counter | `service`, `endpoint`, `code` | 谁在调、失败多少 |
| `ontok_request_duration_seconds` | histogram | `service`, `endpoint` | 慢在哪 |
| `ontok_rows_returned_total` | counter | `service` | 有没有人拉全表 |
| **`ontok_tenant_denied_total`** | counter | `service`, `reason` | **跨租户尝试（安全信号）** |
| `ontok_ttl_bytes` | gauge | `view` | 产物与版本一致性 |
| `ontok_mapping_joins` | gauge | — | 映射规模（版本切换会变） |

**租户拒绝**那条最重要：它是**有人试图越权的唯一可见信号**。

### 3 · 审计默认走 stdout

| 项 | 现在 | 改成 |
| --- | --- | --- |
| 默认落点 | `--audit-out` 文件（emptyDir） | **stdout**（容器日志 → 集群采集） |
| 文件 | 唯一选项 | **可选**（给了才写；同时仍打 stdout） |
| 格式 | 一行 key=value | 保持（已有 `audit ts=… token=sha256:… action=… result=…`） |

理由：**k8s 的标准做法是"进程写 stdout，集群负责采集"**。写 emptyDir 等于把证据交给 Pod 生命周期。

### 4 · k8s 清单

- `/metrics` 就绪后，`overlays/prod` 加回 `prometheus.io/scrape` 注解（**这次是真的**）
- 审计：base 里去掉 `emptyDir` 挂载（改成 stdout），或保留 PVC 选项（二选一，**必须说明选了哪个**）

## 不做

- ⛔ 不引 Prometheus client 库
- ⛔ 不做 tracing / 分布式追踪
- ⛔ 不做日志聚合方案本身（只保证"写到 stdout 且格式稳定"）
- ⛔ 不改任何业务逻辑

## 关键裁定

| # | 裁定 |
| --- | --- |
| R1 | `/metrics` **免 token**，但**零业务数据**（只有聚合计数） |
| R2 | ⛔ **不引依赖**，手写 Prometheus 文本格式 |
| R3 | 审计默认走 **stdout**；文件是可选 |
| R4 | 标签里⛔ 不许出现租户 id / 表名 / 查询文本（维度爆炸 + 泄漏） |
| R5 | 指标必须**单调且可加**（no 每次导数、no 随机） |

## 风险

| # | 风险 | 缓解 |
| --- | --- | --- |
| 1 | `/metrics` 免 token ⇒ 泄漏面 | 只给聚合数字；⛔ 无租户/表名；建议 ingress 层限制到内网 |
| 2 | 指标基数爆炸 | R4；标签只允许固定小集合 |
| 3 | 审计改 stdout 后没人采 | 文档写明"集群必须配采集"，否则等于没记 |
| 4 | 手写格式出错（Prometheus 解析不了） | 判据：`promtool check metrics` 或按规范逐条断言 |

## 交付物

1. `/metrics` × 3 服务 + 指标清单
2. 审计默认 stdout（+ 可选文件）的实测
3. Prometheus 格式合法性证据
4. k8s 清单更新（注解 + 审计挂载）
5. 已知限制
