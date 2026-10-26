# 生产上线就绪评估（2026-10-24 · `develop` `5163cfb`）

> 结论先说：**可以上，但先补 3 个技术缺口、先定 5 个环境问题。**
> 不是"还早"，是"这些不做，出问题时你会看不见"。

---

## 一 · 三道必须补的缺口（我都实测过）

### ① 没有 `/metrics` —— 而我的 prod overlay 曾假装有

`grep -i metrics src/` ⇒ **0 命中**。
但我之前在 `overlays/prod` 里写过 `prometheus.io/scrape: "true"` —— **假的**，抓不到任何东西，只会让监控面板显示"0 指标"而让人以为在监控。**已删。**

**要补**：一个 `/metrics` 端点（Prometheus 文本格式），至少这几个指标：

| 指标 | 为什么 |
| --- | --- |
| `ontok_query_total{code}` | 谁在查、失败多少 |
| `ontok_query_duration_seconds` | 慢在哪 |
| `ontok_rows_returned` | 有没有人拉全表 |
| `ontok_tenant_denied_total` | **跨租户尝试**（安全信号） |
| `ontok_ttl_bytes` / `ontok_mapping_joins` | 产物与版本一致性 |

### ② 审计日志写在 `emptyDir` —— Pod 重启就没了

审计是这套系统的**合规证据**（谁查了什么、谁签了什么）。现在：

```
console/portal: --audit-out /data/audit/*.log  →  emptyDir
query:           走 stderr                         →  跟容器日志一起丢
```

**要定**：审计落到哪（PVC / 日志采集 / 集中式）。**⛔ 不能只留在 emptyDir。**

### ③ 元数据库需要 **DDL 权限**

`migrate()` 在连接时自动建表（有 advisory lock 防并发）。好处是**不需要单独的迁移 Job**，
代价是**应用账号必须有 `CREATE TABLE` 权限**——很多生产库不给。

**要定**：给 DDL 权限，还是提前用一个迁移 Job 建好、应用账号只给 DML。

---

## 二 · 五个必须先定的环境问题

| # | 问题 | 影响 |
| --- | --- | --- |
| 1 | **目标集群与命名空间** | 决定 kustomize overlay 怎么写、Ingress 用哪个 IngressClass |
| 2 | **元数据库形态**：集群内 PG 还是托管 PG | 决定备份/高可用/连接串形态 |
| 3 | **源库网络**：集群能不能直连客户业务库 | 不能直连就得跳板/代理，或改成"抽取在客户侧跑、只上传提案" |
| 4 | **LLM 归属**：抽取时的模型走谁 | 生产抽取是**一次性 Job**；模型可以是内网 ollama 或云 API。⛔ 查询期**不需要 LLM** |
| 5 | **密钥管理**：token 与连接串怎么存 | 现在是文件/Secret；要定轮换方式（换 token = 重启 Pod） |

---

## 三 · 上线步骤（单项目版）

```bash
# 1 建命名空间与密钥
kubectl create ns ontok-prod
kubectl -n ontok-prod create secret generic ontok-db \
  --from-literal=metadata-url='postgresql://ontok:...@pg:5432/ontok' \
  --from-literal=source-url='postgresql://ro_xxx:...@biz:5432/biz'

# 2 构建并推镜像（固定 tag，⛔ 不用 latest）
cd code && podman build -t <registry>/ontok:0.1.0 -f Dockerfile .
podman push <registry>/ontok:0.1.0

# 3 生成声明与口令（本项目一套）
#    query-policy.json / tenancy.json / exclusions.json / portal-questions.json
#    admin/query/portal-tokens.json
kubectl -n ontok-prod create configmap ontok-config --from-file=deploy-config/
kubectl -n ontok-prod create secret generic ontok-secrets --from-file=deploy-tokens/

# 4 抽取与冻结（在能连源库的机器上，一次性）
ontok-builder introspect → ontok-builder propose → review decide → apply → version freeze

# 5 部署
kubectl apply -k Deploy/k8s/overlays/prod

# 6 冒烟（三条，缺一不可）
#    a) /api/health 200
#    b) /api/catalog 带 token 200、不带 401
#    c) 两租户对拍：A 的查询结果里 B 的行数 = 0
```

---

## 四 · 回滚

| 层 | 手段 |
| --- | --- |
| 镜像 | `kubectl rollout undo`（固定 tag 版本） |
| 本体 | **不用回滚**：版本冻结不可变，`--version` 指回旧版即可（服务重启带上旧 `--version`） |
| 声明 | ConfigMap 有 git 历史，`kubectl apply` 旧版本 |
| 数据 | 元数据库备份（**上线前必须先定备份策略**） |

> **本体版本是"重启换版本"，不是"改数据"**——这是这套设计最舒服的地方：新旧共存、随时切回。

---

## 五 · 首版建议范围（canary）

**不要一次上三个面**。建议：

| 阶段 | 开什么 | 给谁 |
| --- | --- | --- |
| **v0** | `query` 内网 + `portal` 一个客户 | 内部验证 |
| **v1** | 加 `console` 内网 | FDE 开始用 |
| **v2** | 数字员工接 MCP | Agent 接入 |
| **v3** | 多项目 | 每项目一套负载 |

**v0 的退出条件**：连续 3 天，两租户对拍零泄漏 + 审计日志完整 + 无 5xx。

---

## 六 · 我建议先做的三件事（按顺序）

| # | 做什么 | 为什么先做 |
| --- | --- | --- |
| 1 | **补 `/metrics`** | 没有它，上线后"看不见"；而且现在有个假注解会误导 |
| 2 | **定审计落地**（PVC 或采集） | 合规证据不能随 Pod 消失 |
| 3 | **REQ-134**（保留名登记表过期） | 那是**防真实表名泄漏**的守卫，现在它失效了 |

前两件是部署问题，第三件是**上线前的安全门槛**。
