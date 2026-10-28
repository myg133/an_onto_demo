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

## 五·五 · ⚠️ 升到 0.3.2 后，"重新导入同一个包"**不会**补写 AM

`POST …/bundles/:id/import` 是**幂等**的：包里已记 `imported_version_id` 时**函数一开头就返回**
（`already_imported: true`），而 AM 的校验与写入在那之后 —— 所以：

| 场景 | 结果 |
| --- | --- |
| 0.3.1 上导过 → 升 0.3.2 → 对**同一个包**再 import | **AM 仍然不落库**（幂等早退），看起来"升级没生效" |
| 升 0.3.2 → **重新上传**同一份包（得到新 `bundle_id`）→ import | ✅ AM 正常入库 |

**所以升级后的验证姿势**：⛔ 不要复用旧包，**重新上传一次**。

判定是不是老版本还有一个指纹：**0.3.1 的 import 响应里必有**这条警告
`"包里有 am/（访问映射），但 v1 的 import 只落本体快照…"`；**0.3.2 没有**。

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

## 五·六 · 0.3.3 起：`POST /api/projects` 发的 key **直接可用**

| 版本 | 建项目返回的 `ontok_q_…` |
| --- | --- |
| ≤ 0.3.2 | **401**（运行时只认静态 token 表）⇒ 只能手工往 Secret 抄一条 |
| **≥ 0.3.3** | ✅ 直接能用（静态表未命中时按 `sha256` 查 `project_keys`，命中即按该 key 绑的项目解析） |

- DB 发的 key **恒为 query**（表里没有 kind 列）⇒ 拿不到写权限
- 未命中 / 已吊销 ⇒ **401**；单项目模式下跨项目 ⇒ **403 `key.project_mismatch`**
- 吊销在 **≤60s** 内可见（进程内缓存 TTL）
- token 表里把 `project` 写错 ⇒ **404 `project.not_found`**（0.3.2 及以前是 500，容易误判成服务挂了）

## 五·七 · 多项目模式：**不要**给 `--access-mapping`

AM 的归属是**版本**（随构建包入库，`version_access_mappings`，REQ-174 起按 `active_version_id` 加载）。
`--access-mapping` 是**进程级一份**，那是单项目模式的概念。

| 情况 | 正确做法 |
| --- | --- |
| 单项目（`--project` + `--version`） | 可以给 `--access-mapping`（文件优先，旧部署不变） |
| **多项目**（两个都不给） | ⛔ **不要给**。每套本体用自己的 AM；包没带 AM ⇒ 该类按库源处理（不报错） |

**实测事故（2026-10-28）**：多项目部署里带着 `--access-mapping`，客户体的项目**没有自己的 AM**
⇒ 回落到那份（腾讯云）映射 ⇒ `bind` 对不上（14 个类不存在）⇒ **整个项目 500 打不开**。

> 0.3.4 起：多项目 + `--access-mapping` ⇒ **启动即拒**（exit 2，连库之前），不再静默兜底。

## 五·八 · 业务词恰好等于建模术语时：`--term-allowlist`

发布闸（客户视图不得含建模术语：`外键/基数/主键/证据` + `pk/key/evidence/cardinality/…`）会**误杀**业务词。
例：审计域的实体叫 `evidence`。

```bash
cat > /etc/ontok/term-allowlist.txt <<'EOF'
# 每行一个词；# 开头为注释
evidence
EOF
ontok query serve ... --term-allowlist /etc/ontok/term-allowlist.txt
```

| 保证 | 说明 |
| --- | --- |
| 默认不放松 | 不给这个 flag ⇒ 行为与以前**逐字节一致** |
| 豁免**留痕** | 启动打一行"加载了哪些词"；**每次豁免**打一行（出口 + 词条） |
| fail-closed | 文件不存在 / 行非法 ⇒ **启动失败**并点出行号 |
| 判据仍唯一 | 白名单只在闸门层做减法；`scan_modeling_terms` 一字未改 |

⚠️ 中文术语是**子串**匹配 ⇒ 「证据材料」照样命中，改名时要避开连续的两字。
