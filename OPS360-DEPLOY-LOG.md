# Ops360 部署实录（2026-10-24 首次上线）

> 这份记录的是**实际发生了什么**，不是"应该怎么做"。
> 包括我踩的坑、临时妥协，以及每个妥协的到期条件。

---

## 一 · 现在线上是什么

```
集群    anspire-k8-admin@anspire-dify
命名空间 anspire-ontok-uat
镜像    aigccontainer.azurecr.cn/an_onto/ontok:0.1.1
负载    Deployment ontok-query (1/1)
入口    https://an-onto.local.anspire.cn   （traefik + 通配 TLS）
项目    ops360 / v2（冻结）
```

冒烟（经 ingress 实测）：

| 检查 | 结果 |
| --- | --- |
| `/api/health` | 200 · `mode=ontology-only` |
| `/api/catalog` | 20 类 / 47 属性 / **10 关系** |
| 无 token | **401** |
| `/api/sparql` | **503** `NO_DATA_SOURCE` |
| `/api/ontology.ttl` | 21,335 B · sha `ab0530cf…` · **与交付包逐字节一致** |
| HTTP 访问 | 301 → HTTPS |

---

## 二 · 踩过的四个坑（部署侧，都不是产品 bug）

### ① `unexpected argument '--bind'`

**原因**：我用 JSON6902 `replace` 整体替换 `args`，**把 `query serve` 子命令也替换掉了**。

base 的 args 是 `[query, serve, --bind=…]`，我只写了 `[--bind=…]`。

**教训**：替换 args 必须**连子命令一起写**，并渲染后核对。

### ② `unknown field 'kind'`

**原因**：query token 的形状没有 `kind` 字段（那是 console/portal token 才有的），我照抄了。

```
query token : { label, token, tenant, max_rows, max_concurrency }
console token: { label, token, kind, ... }
```

### ③ `missing field 'tenant'` → **去看 REQ-137**

**原因**：本体模式没有租户，token 却强制要求 `tenant`。

**临时妥协**：填 `00000000-0000-0000-0000-000000000000` 让它跑起来。

⚠️ **这是欠账**：审计里会出现一个"看起来真实"的租户 id。REQ-137 修完后要**把 secret 里的 tenant 删掉并重启**。

### ④ Ingress 404（不是 301、不是 403）

**原因**：本集群 traefik 配了 **HTTP→HTTPS 永久重定向**，且约定：

| 必须 | 否则 |
| --- | --- |
| `spec.tls[].secretName: wildcard-local-anspire-cn` | 404 |
| 证书 secret 在**同一命名空间**（要从 middlewave 复制） | 404 |
| 注解 `traefik.ingress.kubernetes.io/router.tls: "true"` | 404 |

**教训**：404 不一定是"路由没配"，也可能是**TLS 没配**。

---

## 三 · 两个刻意的取舍

### ① 只部署 query，不部署 console/portal

合作方只要本体接口。多开一个面就多一份暴露。

### ② `/metrics` 免 token ⇒ 必须靠网关挡

```yaml
# 现状：ingress 把 / 全转给服务，/metrics 也在里面
```

**风险**：任何人访问 `https://an-onto.local.anspire.cn/metrics` 都能看到指标（无业务数据，但仍不该公开）。

**到期条件**：上线前加 NetworkPolicy 或在 traefik 层加 `/metrics` 的拒绝规则。

---

## 四 · 集群侧的资源（都是显式创建的）

| 资源 | 来源 | 备注 |
| --- | --- | --- |
| namespace `anspire-ontok-uat` | 手工 | |
| 数据库 `ontok`（middlewave/PG17） | 手工 CREATE DATABASE | 空库，应用启动时自动 `migrate()` |
| Secret `ontok-db` | 手工 | metadata-url（**source-url 填的是同一个值**，本体模式不用） |
| Secret `ontok-secrets` | 手工 | 仅 `query-tokens.json`（合作方 token） |
| Secret `wildcard-local-anspire-cn` | 从 middlewave 复制 | TLS |
| Secret `postgres17-wal-registry` | 从 middlewave 复制 | ACR 拉取凭据 |
| ConfigMap `ontok-config` | 手工 | policy/tenancy/exclusions（空声明） |

⛔ **overlay 里刻意删掉了 ConfigMap/Secret**——base 的是空占位版本，apply 会把真 token 抹成空列表。

---

## 五 · 下次部署的检查单

```
1. 镜像 tag 固定（⛔ 不用 latest），冒烟 `ontok query serve --help | grep ontology-only`
2. base 的 args 有 `query serve` 前缀 —— 替换时别丢
3. token 形状按服务的**类型**选（query 没有 kind）
4. 本体模式的 token **不许带 tenant**（见 REQ-137）
5. Ingress 必须带 TLS + 同命名空间证书 + router.tls 注解
6. 用 `kubectl kustomize` 先渲染，核对 args / image / 有没有 ConfigMap
7. 部署后冒烟五条：health / catalog / 无 token 401 / SPARQL 503 / TTL sha
```
