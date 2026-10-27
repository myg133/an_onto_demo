# UAT 上线前置（按本集群实测值，2026-10-24）

> 集群事实（我查过）：
>
> | 项 | 值 |
> | --- | --- |
> | context | `anspire-k8-admin@anspire-dify` |
> | Ingress | `traefik`（IngressClass `traefik`） |
> | 元数据库候选 | `postgres17-wal-service.middlewave:5432`（用户 `postgres`，现有库 `anspire_hk_open_uat`） |
> | UAT 源库 | `postgres-service.middlewave:5432`（`anspire_open_uat`，NodePort 30432） |
> | 命名空间惯例 | `anspire-*-uat` |

---

## 步骤 0 · 确认两件我要问的事

| # | 问题 | 为什么 |
| --- | --- | --- |
| 1 | **ACR 仓库路径** | overlay 里是 `aigccontainer.azurecr.cn/REPLACE-ME/ontok`，需要真实 repo |
| 2 | **是否允许我在集群里创建 namespace / 数据库 / 只读角色** | 这些是**写操作**，我不会擅自做 |

---

## 步骤 1 · 建命名空间

```bash
kubectl create namespace anspire-ontok-uat
```

## 步骤 2 · 元数据库（在 `postgres17-wal` 上建库）

```bash
kubectl -n middlewave exec postgres17-wal-0 -- \
  psql -U postgres -c "CREATE DATABASE ontok"
```

> 应用连接时会自动 `migrate()` 建表（有 advisory lock 防并发），**不需要单独的迁移 Job**。
> 但因此 **应用账号需要 `CREATE TABLE` 权限**——`postgres` 超户满足；若要最小权限，需先手工建表或提前跑一次迁移。

## 步骤 3 · 源库只读账号（**不要用超户**）

```bash
kubectl -n middlewave exec postgres-service-0 -- psql -U postgres -c "
  CREATE USER ontok_ro WITH PASSWORD '<强口令>';
  GRANT CONNECT ON DATABASE anspire_open_uat TO ontok_ro;
"
kubectl -n middlewave exec postgres-service-0 -- psql -U postgres -d anspire_open_uat -c "
  GRANT USAGE ON SCHEMA public TO ontok_ro;
  GRANT SELECT ON ALL TABLES IN SCHEMA public TO ontok_ro;
  ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO ontok_ro;
"
```

## 步骤 4 · 两个连接串 Secret

**不打印口令**的写法（从已有 Secret 取，不落屏）：

```bash
PW=$(kubectl -n middlewave get secret postgres17-wal-secret -o jsonpath='{.data.POSTGRES_PASSWORD}' | base64 -d)
RO=$(你要给 ontok_ro 设的口令)

kubectl -n anspire-ontok-uat create secret generic ontok-db \
  --from-literal=metadata-url="postgresql://postgres:${PW}@postgres17-wal-service.middlewave.svc.cluster.local:5432/ontok" \
  --from-literal=source-url="postgresql://ontok_ro:${RO}@postgres-service.middlewave.svc.cluster.local:5432/anspire_open_uat"
```

## 步骤 5 · 两套口令 + 三份声明

```bash
# 口令（kind 必须互斥：admin / query）
kubectl -n anspire-ontok-uat create secret generic ontok-secrets \
  --from-file=deploy-tokens/

# 声明（进版本库、可评审）
kubectl -n anspire-ontok-uat create configmap ontok-config \
  --from-file=deploy-config/
```

`deploy-config/` 至少三份：`query-policy.json` / `tenancy.json` / `exclusions.json` + `schema.json`。

## 步骤 6 · 抽一次本体并冻结（**在集群外跑**）

`query serve` 启动时要 `--project` + `--version`，所以**先冻结再部署**：

```bash
ontok-builder introspect --out schema.json
ontok-builder propose --schema schema.json --names table_names.json --out proposal.json
ontok-builder import --file proposal.json
ontok-builder review decide --proposal p1 --project anspire_uat --accept-all
ontok-builder proposal apply --proposal p1 --project anspire_uat
ontok-builder version freeze --project anspire_uat --version 1
```

> 这一步要连**元数据库 + 源库**，在本机能连通时跑最省事；也可以在集群里跑一次性 Job。

## 步骤 7 · 部署

```bash
# 先替换 ACR 路径
kubectl apply -k Deploy/k8s/overlays/uat
```

## 步骤 8 · 冒烟（三条，缺一不可）

```bash
# a) 健康
kubectl -n anspire-ontok-uat port-forward svc/ontok-query 8800:8800 &
curl -s localhost:8800/api/health

# b) 鉴权
curl -s -o /dev/null -w '%{http_code}\n' localhost:8800/api/catalog            # 期望 401
curl -s -H "Authorization: Bearer <query-token>" localhost:8800/api/catalog | head -c 200

# c) 泄漏对拍（最重要）
#    用两个真实租户各查一次，结果集交集必须为空
```

---

## 与 `base/` 的差异

| 项 | base | uat overlay |
| --- | --- | --- |
| namespace | `ontok` | `anspire-ontok-uat` |
| 副本 | 1 | 1 |
| 审计挂载 | `emptyDir` | **PVC `ontok-audit`（5Gi）** |
| 内存 limit | 1Gi | 512Mi |
| Ingress | 无 | traefik × 2 |
| 镜像 | `ontok:dev` | `aigccontainer.azurecr.cn/<repo>/ontok:0.1.0` |

---

## ⚠️ 上线前还欠的两件事

| # | 项 | 状态 |
| --- | --- | --- |
| 1 | `/metrics` 端点 | **REQ-135 在做**；没它之前**不要加** prometheus 注解（假注解比没有更糟） |
| 2 | REQ-134：保留名登记表过期 | **未修**；那是防真实表名泄漏的守卫，上线前应修 |
