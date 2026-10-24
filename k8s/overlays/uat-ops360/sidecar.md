# cloudflared sidecar · token 模式（已上线）

> 目标：给 `ontok-query` Pod 加第二个容器，让 `https://<公网域名>` 直达 `127.0.0.1:8800`。

---

## 一 · 现状（2026-10-24 已部署）

```
Pod ontok-query  2/2 Running
├── query        ontok:0.1.3              :8800
└── cloudflared  cloudflared:2024.12.2    → 127.0.0.1:8800
```

隧道已连接（4 条 QUIC）：

```
Registered tunnel connection connIndex=0..3  location=lax01/08/09/13  protocol=quic
```

| 入口 | 走不走 traefik | 用途 |
| --- | --- | --- |
| `an-onto.local.anspire.cn` | ✅ 走（Ingress） | 内网 |
| `<公网域名>`（CF） | ❌ **不走** | 公网 |

**两条路并存、互不影响。**

---

## 二 · 为什么是 token 模式，以及它的代价

你给的是 `cloudflared tunnel run --token <…>`（**远程管理隧道**）。

| | 结果 |
| --- | --- |
| ✅ 优点 | 只要一个 Secret，配置全在 CF 面板，改规则不用重启 Pod |
| ⚠️ **代价** | **集群里的 `config.yaml` 会被忽略** ⇒ 路径规则改不了 |

### 直接影响：`/metrics` 现在可能对公网可见

我们的 `/metrics` 是**免 token** 的（Prometheus 抓取需要）。
在本地管理模式下我能在 ingress 规则里写 `^/metrics$ → 404`；
**token 模式下只能在 Cloudflare 面板挡。**

⛔ **上线前请在 CF 面板加一条 WAF 规则**：

```
当 http.request.uri.path 等于 "/metrics"  →  Block
```

（或用 Transform Rule 返回 404；效果一样。）

**不加的后果**：任何人都能看这套部署的请求量、耗时、租户拒绝计数——虽无业务数据，但不该公开。

---

## 三 · 还需要配的一样东西：Public Hostname

在 Cloudflare 面板 → Zero Trust → Networks → Tunnels → 该隧道 → **Public Hostname**：

| 字段 | 值 |
| --- | --- |
| Subdomain / Domain | 你的公网域名，如 `onto` + `example.com` |
| **Service → Type** | **HTTP** |
| **Service → URL** | **`localhost:8800`** |

⚠️ **必须写 `localhost:8800`**——cloudflared 与本体服务在**同一个 Pod**，共享网络命名空间；
写 Service 名（`ontok-query:8800`）反而可能不通（取决于 DNS 策略）。

---

## 四 · 上线后必查四条

```bash
# ① 隧道在线（应看到 Registered tunnel connection ×4）
kubectl -n anspire-ontok-uat logs deploy/ontok-query -c cloudflared --tail=20

# ② 公网能访问
curl https://<公网域名>/api/health

# ③ /metrics 必须被挡（404 或 403）—— ⛔ 不通过就不要对外
curl -o /dev/null -w '%{http_code}\n' https://<公网域名>/metrics

# ④ 无 token 必须 401
curl -o /dev/null -w '%{http_code}\n' https://<公网域名>/api/catalog
```

---

## 五 · 安全边界

| 允许公网 | 禁止公网 |
| --- | --- |
| `ontology-only` 部署（无业务数据） | **普通模式**（有客户数据） |
| `/api/health` `/api/catalog` `/api/describe` `/api/ontology.ttl` `/mcp` | `/metrics`（免 token） |

**再加一层**（建议）：Cloudflare Access（SSO）或 WAF 限速/地域限制。
现在这个域名只靠一个 Bearer token 挡着。

---

## 六 · 回滚

```bash
# 关掉公网入口（保留内网）
# 在 kustomization.yaml 里注释掉：
#   - path: sidecar-patch.yaml
kubectl apply -k Deploy/k8s/overlays/uat-ops360
```

Pod 会回到 1 容器，内网入口不受影响。

---

## 七 · 附：为什么不用本地管理模式

一开始我按"本地管理"（credentials.json + config.yaml）做的，因为只有它能在集群侧写路径规则。
你给的是 token ⇒ 改成远程管理。

**如果哪天需要把 `/metrics` 的拦截收回到集群侧**，就得换成本地管理隧道：
`cloudflared tunnel create` 拿凭据 JSON + 在 CF 面板创建 Public Hostname 指向 `http://localhost:8800`。
那时 `cloudflared-config.yaml`（本次保留未用）就能派上用场。
