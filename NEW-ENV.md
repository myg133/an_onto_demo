# 新环境部署清单（v0.3.0）

> 面向：要给一个新客户 / 新环境起一套 ontok 的人。
> 依据：2026-10-27 v0.3.0 在 UAT 的实际发版（含四个踩过的坑，见 §六）。

---

## 一 · 一句话

**不需要 PVC。状态全在元数据库里。** 需要备的是：**一个 Postgres** + **两把密钥/凭据** + **一份 AM**。

| 项 | 要吗 | 说明 |
| --- | --- | --- |
| **元数据库（Postgres）** | ✅ **必须** | 项目 / 版本 / 快照 / 构建包字节 / DSN 密文 / key 密文，**全在这里** |
| `ONTOK_DATABASE_URL` | ✅ 必须 | 指向上面那个库 |
| `ONTOK_SECRET_KEY` | ✅ 必须 | 64 hex；加密 DSN 与 query key。**缺了拒绝启动** |
| token 表（`query-tokens.json`） | ✅ 必须 | 至少一条 `kind:"admin"`（FDE 用）+ 给数字员工的 query key |
| AM（access mapping） | ⚠️ 有 API 源才要 | 挂 ConfigMap；只查库的项目可以不挂 |
| **PVC / 本地卷** | ❌ **不需要** | 包字节入 `project_bundle_payloads.data`（bytea）；生产路径不落盘 |
| Ingress / TLS | 可选 | 对外就要；只内网可 ClusterIP |

---

## 二 · 两种服务模式（**部署前必须先选**）

| 模式 | 启动参数 | 能服务几个项目 | key 怎么定项目 |
| --- | --- | --- | --- |
| **单项目** | `--project <p> --version vN` | **1 个**（钉死） | 由参数决定，key 不参与 |
| **多项目** | **两个都不给** | **多套** | **key → `project` → `projects.active_version_id`** |
| 只给一个 | — | **拒绝启动** | 参数自相矛盾 |

⚠️ **UAT 现在跑的是单项目**（`--project cloud-resource --version 5`）⇒ 在它上面新建的项目，数字员工**查不到**。
**"一套部署多套本体"必须用多项目模式。**

多项目模式的额外要求（缺一条**拒绝启动**）：

- `query-tokens.json` 里**每一条 `kind:"query"` 都必须有 `project`**
- `kind:"admin"` **必须没有 `project`**（admin 不绑项目，绑了整份表被拒）
- `--ontology-only` 下**不能有 `tenant`**（填了任何值都拒）

---

## 三 · 最小部署（k8s）

### 3.1 一次性 provision（不进版本库）

```bash
NS=<你的命名空间>

# ① 元数据库连接（已存在的 PG）
kubectl -n $NS create secret generic ontok-db \
  --from-literal=metadata-url='postgresql://<user>:<pass>@<host>:<port>/<db>'

# ② 加密密钥（64 hex，生成一次，记进密码管理）
KEY=$(openssl rand -hex 32)
kubectl -n $NS create secret generic ontok-write --from-literal=secret-key="$KEY"
echo "记下来：$KEY   ← 丢了 ⇒ 已加密的 DSN / query key 不可解（v1 不做轮换）"

# ③ token 表：admin（FDE 写面）+ query（数字员工读面）
cat > /tmp/query-tokens.json <<'JSON'
{ "version": "1.0.0",
  "tokens": [
    { "label": "fde-admin", "token": "ADMIN-<随机>", "kind": "admin" }
  ] }
JSON
kubectl -n $NS create secret generic ontok-secrets \
  --from-file=query-tokens.json=/tmp/query-tokens.json && rm -f /tmp/query-tokens.json
```

> 数字员工的 query key **不用手写**：建项目时由 `POST /api/projects` 生成并一次性返回。

### 3.2 AM（有 API 源时）

```bash
# 发版前先验（非 0 就卡住不发）
python Deploy/tools/check_am.py --file <你的 am.json>
kubectl -n $NS create configmap ontok-access-mapping --from-file=am-tencent.json=<你的 am.json>
```

### 3.3 起服务

```bash
kubectl -n $NS apply -k Deploy/k8s/overlays/<你的 overlay>
kubectl -n $NS rollout status deploy/ontok-query --timeout=180s
```

容器参数（overlay 里配）：

```
ontok query serve \
  --bind 0.0.0.0:8800 \
  --tokens /etc/ontok/secrets/query-tokens.json \
  [--access-mapping /etc/ontok/am/am-tencent.json] \
  [--ontology-only] \
  [--project <p> --version vN]        # 给了就是单项目；都不给就是多项目
```

环境变量（Deployment env）：

| 变量 | 来源 |
| --- | --- |
| `ONTOK_DATABASE_URL` | Secret `ontok-db` / `metadata-url` |
| `ONTOK_SECRET_KEY` | Secret `ontok-write` / `secret-key` |

---

## 四 · 建本体 → 上线（**注意用哪把 key**）

| # | 动作 | key | 端点 |
| --- | --- | --- | --- |
| 1 | 建项目 | **admin** | `POST /api/projects {project,label,key_label}` ⇒ 返回**项目 key**（给数字员工，只读） |
| 2 | 上传包 | **admin** | `POST /api/projects/:p/bundles`，`Content-Type: application/octet-stream`，body = `.ontokbundle` 原始字节 |
| 3 | 配 DSN（仅 `requires_dsn`） | **admin** | `PUT /api/projects/:p/database {dsn}` + `POST …/database/test`（**失败也 200**，看 `ok` 字段） |
| 4 | 解析 | **admin** | `POST …/bundles/:id/import` ⇒ `version=vN`（`already_imported:true` 也当成功） |
| 5 | **激活** | **admin** | `POST …/bundles/:id/activate` ⇒ `frozen=true` + `active_version_id` 指向它 |
| 6 | 数字员工查询 | **项目 key** | `GET /api/health` · `/api/catalog` · `/api/describe` · `POST /api/sparql` · `/api/ontology.ttl` · `/mcp` |

⚠️ **写面全部只认 `kind:admin`**；项目 key 调任一写端点 ⇒ **403 `key.not_admin`**。
⚠️ **第 5 步不能漏**：不激活 ⇒ 对外 **503 `project.no_active_version`**。

---

## 五 · 上线验证（五条，缺一条都别宣布成功）

```bash
B=https://<host>; A=<admin key>; Q=<项目 key>

# ① 服务活着 + 版本
curl -sS $B/api/health
#   期望：{"mode":"...","ontology_version":N,"project":"...","version":"0.3.0"}

# ② 读面：用项目 key
curl -sS -H "Authorization: Bearer $Q" "$B/api/catalog?page_size=5"

# ③ 写面：admin 能、项目 key 不能
curl -sS -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $A" $B/api/projects      # 期望 200
curl -sS -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $Q" $B/api/projects      # 期望 403

# ④ 配方可用（api 源才有）：@pending.get.auth.service 必须在
curl -sS -X POST $B/api/sparql -H "Authorization: Bearer $Q" -H 'Content-Type: application/json' \
  -d '{"query":"SELECT ?s WHERE { ?s a <https://<ns>#<class>> } LIMIT 1"}'

# ⑤ 数字员工侧：脚本自检（离线）
python Deploy/skills/ontok-query/scripts/access_fetch.py --selftest   # 98 passed / 0 failed
```

---

## 六 · 四个坑（都是 2026-10-27 实际踩的，**每一个都会"看起来成功、实际不可用"**）

| # | 坑 | 症状 | 防法 |
| --- | --- | --- | --- |
| 1 | **AM 缺 `auth.service`** | 服务**照样起**（实测 2026-10-28）；但**取数时**脚本报 `am.invalid`（exit 2，tc3 签名器要求显式声明）⇒ 表现为"类能查、数据取不到" | 发版前 `check_am.py` 必须 0 error（它挡的是**取数期的失败**，不是启动期） |
| 2 | **没配 `ONTOK_SECRET_KEY`** | **CrashLoopBackOff**（日志：拒绝启动） | Secret `ontok-write` + Deployment env |
| 3 | **没配 admin token** | 服务能起，但 Studio 写面**全 403** | token 表里加 `kind:"admin"`（不绑 project） |
| 4 | **同 tag 重建没重拉** | 老 pod 继续跑**旧镜像**，tag/pod 名都一样 | `imagePullPolicy: Always`；验证看 pod 的 `imageID` |

---

## 七 · 备份与密钥

| 项 | 说明 |
| --- | --- |
| **元数据库** | 备份它 = 保住项目/版本/快照/**构建包字节**/DSN 密文/key 密文 |
| **`ONTOK_SECRET_KEY`** | **必须与数据库一起备份**。只有库没有 key ⇒ 密文不可解（v1 不做密钥轮换） |
| token 表 | 丢了 ⇒ 数字员工全部 401；admin 丢了 ⇒ 写面进不去 |
| 轮换 | 当前**不支持**轮换明文密钥（换 key 会让历史密文失效）⇒ 密钥只生成一次、长期保存 |
